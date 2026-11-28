# Menu XML: the files the original builds its menus from

How the game's menu files are written and how NVForever reads them into the tile tree
(`crates/fnv-ui`, [design](../design/ui-tile-tree.md)). Written from the files in the
owner's install (read with `fnvtool ui`, never copied into the repo), from mods' files in
the owner's Mod Organizer folders, and from the GECK wiki export's UI function pages.
[design] Only short UIO data-list excerpts are quoted below; no menu XML or complete
mod file is shipped.

Tools: `fnvtool ui menus --data <Data> [--overlay <mod folder>]…` builds every menu and
lists counts and every spot the reader mended (`fnv.ui.menus/1`); `fnvtool ui tree
--menu <file> [--at <path>]` prints one menu's tiles and worked-out trait values
(`fnv.ui.tree/1`). Smoke scenario `ui-menus`.

## 1. The files

- The menu files are under `menus\` in the archives [observed]: 126 files, of them
  `menus\prefabs\` (pieces other files include), `menus\main\` (HUD, inventory, map,
  stats, quick keys, safe zone), `menus\options\` (start, main, pause, load, save,
  credits), `menus\chargen\`, `menus\dialog\`, `menus\generic\` and the rest at the top.
- **49 files hold a `<menu>`** [observed: fnvtool ui menus, 2026-10-26], one each, its
  `name` the menu's name (`HUDMainMenu`, `BarterMenu`, `StartMenu`, …). The engine finds
  a menu's file by that name, not by a table. Files under `menus\` without a `<menu>`
  are pieces (the caravan's four sub-menus, `globals.xml`).
- `menus\master_menu_file.txt` is every menu file joined into one [observed]; nothing
  needs it [hypothesis].
- `menus\globals.xml` is one `<rect name="Strings">` of shared constants (line
  thickness, brightnesses, the Pip-Boy canvas size 1024 × 768, `_Has360Controller`),
  read by other files as `globals()` [observed].

### 1.1 Not strict XML

43 of the 126 files fail a strict XML parser [observed]. What the reader has to take:

1. **Entities no file declares**: `&true;`, `&center;`, `&xbox;`, … (§2.3) and game
   setting strings `&-sName;` (§2.4).
2. **Fragments**: a prefab is several top-level elements (the content of the tile that
   includes it), not one root.
3. **Mismatched end tags**: three vanilla files close an element with another's end tag
   (`quest_added.xml` closes a `<hotrect>` with `</rect>`; `caravan_builddeck_menu.xml`
   and `main_menu.xml` have one end tag too many or too few). Mods do it too: MCM's
   `MCM.xml` opens `<_KeyInut>` and closes `</_KeyInput>`, its `ModListRect.xml` never
   closes one `<onlyif>`.
4. **A start tag cut short**: `<systemcolor</systemcolor>` in `hud_main_menu.xml` and
   `item_stats_display.xml`.
5. **An attribute missing its opening quote**: `name=item_listing_parent"` in
   `item_listing.xml`.

The reader (`fnv_ui::xml`) never fails. Its rules [hypothesis for each; the original's
own rules are not known]:

- an end tag closes the innermost open element of the name it gives, with the ones
  opened inside it, treating `<hotrect>`/`</rect>` and `<rect>`/`</hotrect>` as matching
  rect-family containers so a typo'd `</rect>` closing an inner `<hotrect>` (as in vanilla
  `quest_added.xml` [observed: `Fallout - Misc.bsa`]) closes that `<hotrect>` and keeps
  sibling tiles (e.g. `Button2`) parented under the enclosing `<rect>` rather than popping
  the enclosing `<rect>` and leaving `<menu>` prematurely closed [observed: Fallout: New
  Vegas 1.4.0.525 + xNVSE 6.4.9 + UI Organizer 230 / vanilla XML parser, 2026-11-24,
  `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, isolated clean-room
  staging copy (synthetic prefab XML staged strictly inside the isolated copy and removed
  after exit; read-only `PrintToConsole` probe; zero writes to game install or Saves),
  `w4h_01_quest_added_button2_parent` (`tools/capture/nvse/catalogs/wave4h_reconcile.json`,
  #3802): `PROBE|w4h_01_quest_added_button2_parent|hotrect_rect(ld=1,b1=11,b2Main=22,b2B1=-999,b2Menu=-999)|rect_hotrect(ld=1,in=33,sibMain=44,sibIn=-999,sibMenu=-999)`;
  zero DLL, PDB, or third-party source consulted]; when none of that name is open it
  closes the innermost open element whatever its name [hypothesis: preserving traits and
  operators with typo'd end tags such as Vanilla UI Plus `globals.xml`'s
  `<_VUI+ScreenRatio>…</_VUI+ScreenHeight>`, `vats_menu.xml`'s `<add>…</suaddb>`, and Just
  Assorted Mods `jlm.xml`'s `<_JLMAction1..3>…</_JLMInteract1..3>` [observed: public XML
  files; no third-party DLL or source code consulted]]; with nothing open it is dropped.
  Both MCM spots need this pair of rules to come out as MCM evidently works in the
  original (one needs each rule);
- [hypothesis] an attributeless non-empty start tag `<T>` that is neither a tile/template
  container, `include`, nor an operator closes an open trait of the same name `<T>` when
  that trait already holds non-blank content and only operators (if any) are open inside
  it (mending a forgotten `/` on closing tags such as `<height><copy .../><height>`, with
  a warning);
- [hypothesis] when an enclosing container end tag (a tile or `<template>`) or end-of-file
  implicitly closes an unclosed trait or operator element that has non-blank content
  before one or more contiguous trailing non-operator elements (sibling traits, tiles,
  `include` or `template`), those trailing elements are hoisted onto the enclosing parent
  so they are not dropped as unknown operators (explicitly closed traits keep unknown
  inner tags inside the trait and still warn);
- what is still open at the end of a file is closed there (each included file is read
  on its own, so this stays inside the include);
- a start tag cut short by `<` ends there: open when the cutting tag is its own closing
  tag (`<systemcolor</systemcolor>` in two vanilla files [observed]), and self-closed
  (empty) when cut short by a different start tag or enclosing end tag so following
  sibling traits/tiles are not swallowed [hypothesis];
- an element opened inside 256 open ones is left out with everything in it, up to its own
  end tag [design: `xml::MAX_DEPTH`, NVForever's own guard against running out of stack on
  a broken or crafted file, #3156; no vanilla file comes near it];
- an unquoted attribute value runs to a space or `>`, stray quotes dropped;
- `<!-- -->` comments, `<?…?>` and `<!…>` are skipped; `&amp;` `&lt;` `&gt;` `&quot;`
  `&apos;` `&#N;` are decoded; any other `&name;` is kept by name;
- bytes are UTF-8 when they are, else Latin-1 (every vanilla file is ASCII [observed]).

Every mend is listed (`Ui::warnings`, fnvtool's `warnings`). Vanilla, 2026-10-26: the
two cut-short tags, the three mismatched files, nothing else.

## 2. Elements

An element is one of four things, by its tag (any case):

| Tag | What it is |
|---|---|
| `menu`, `rect`, `image`, `text`, `nif`, `radial`, `hotrect` (and `window`) | a **tile**: a node of the tree, named by its `name` attribute |
| `include src="file.xml"` | the file's content put here, as if written in place |
| `template name="…"` | content kept to make tiles from later, not a tile now |
| anything else | a **trait** of the tile it is in |

[observed: the tags that carry a `name` attribute in the vanilla files are exactly the
seven tile kinds and `template`; `window` is in no vanilla file and is kept as a tile in
case a mod uses it, hypothesis]

### 2.1 Includes

- `src` names a file under `menus\prefabs\`, with a folder when it is in one
  (`MCM\MCM.xml`) [observed: every vanilla include names a bare file that is in
  `menus\prefabs\`; MCM's name files in its own folder there].
- Includes nest (a prefab includes others). NVForever stops at 16 deep [design].
- A missing file leaves nothing and a warning.

### 2.2 Templates

`<template name="…">` holds a tile (or several) to make later: the game's code fills
lists from them (the HUD's quest checkboxes, list items), and JIP's
`AddTileFromTemplate "path|template|name"` makes one under a tile [documented:
AddTileFromTemplate]. Templates sit on the tile they are written in; one is looked for
on the tile named and then on the tiles above it [hypothesis]. Their includes are
spliced in when the file is read, so making a tile later needs no file.

### 2.3 Constants

Entities without a minus are constants: 94 different names in the vanilla files
[observed], most used:

- `&true;` `&false;`;
- justification `&left;` `&center;` `&right;`;
- `&scale;` (a picture's sizing);
- platform flags `&xbox;` `&console;` (guarding console-only values);
- the system colours `&hudmain;` `&hudalt;` `&pipboy;` `&terminal;` `&mainmenu;`
  `&nosystemcolor;` `&uselocalcolor;`;
- tile ids `&noglow_branch;` `&glow_branch;` `&generic;`;
- the controller buttons `&xbuttona;` … `&xbuttonrt;`;
- each menu's class (`&HUDMainMenu;`, `&BarterMenu;`, … in `<class>`);
- `&does_not_stack;` `&no_click_past;` `&nosound;`.

NVForever keeps each as a named constant (`Value::Sym`): two compare equal when
their names do, so `<eq> &center; </eq>` works without a number. The 40 menu class
constants also read as numbers [documented: GECK wiki, Hardcoded UI Shortcuts,
revision 39903 (2026-04-02; the same rows as revision 30746), only the "Menu class ID"
rows]. These are MenuMode's
codes [documented: GetActiveMenuMode]. Other numbers stay as before [hypothesis]:

- `&true;` 1 and `&false;` 0: the meter prefab picks its picture as `_filename_` + its
  `_SolidMeter`, with `_filename_1` the solid one and a comment saying a true
  `_SolidMeter` draws solid;
- `&xbox;` `&console;` `&xenon;` 0, the PC build;
- `&hudmain;` 1, `&hudalt;` 2 [observed: private job hud_jip file_0032;
  hud_hardcore2 hc2-hc5; hud.md §14.8];
- every other constant reads 0 as a number [hypothesis].

Scripts writing entity-shaped strings via `SetUIString` (e.g. `"&false"`, `"&true"`,
`"&center"`) store the string verbatim without resolving it to a numeric entity constant:
`GetUIFloat` reads `0` for all three (`&false`, `&true`, and `&center`, overwriting the
prior numeric value) while `GetUIString` reads `"&false"` verbatim [observed: Fallout:
New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-24, `GSDocMitchellHouse`
(`0x00103DF9`), 10 official base masters, `w4d_43_nif_selector_and_ui_entities`
(`tools/capture/nvse/catalogs/wave4d_extenders.json`, #3679):
`HUDMainMenu/ReticleCenter/visible` `ret=64` -> `SetUIString "&false"` => `f=0 [&false]`,
`SetUIString "&true"` => `t=0`, `SetUIString "&center"` => `c=0`; no DLL or source
consulted]. Accordingly, `Value::Str` parses only numeric text in `Value::num()` (so
`GetUIFloat` / `GetUIFloatAlt` return `0.0` for `"&true"` as well as `"&false"` and
`"&center"`), while static XML entities parsed as `Value::Sym` keep their numeric values
(`&true;` -> `1`, menu classes -> their IDs). NVForever still matches standalone `&name` /
`&name;` ASCII identifier strings (excluding `&-sName;` game-string placeholders and mixed
prose) against `Value::Sym` for `truthy()`, `eq`, `justify`, and suffix-`<copy>` lookup
while keeping the written `Value::Str` verbatim for `GetUIString` [design: `fnv_ui::value`].

### 2.4 Game setting strings

`&-sName;` is the text of the game setting `sName` [hypothesis: the names are game
settings' names, `s` for a string, as the wait menu's `sHowManyWait` and `sCancel`
(wait-menu.md)]. 145 different spellings are in the vanilla files, 143 without case
(`sOK`/`sOk`, `sQuit`/`squit`) [observed: the `&-…;` entities of the 121 `menus\**\*.xml`
in `Fallout - Misc.bsa`, 2026-10-29; no other vanilla archive holds menu files].

Only one has a record in the vanilla load order: `sMorphine`, a GMST of FalloutNV.esm.
The other 144 have none in FalloutNV.esm or the nine official add-on masters (DeadMoney,
HonestHearts, OldWorldBlues, LonesomeRoad, GunRunnersArsenal, Caravan, Classic,
Mercenary and Tribal packs) [observed: `fnvtool esm inventory`, GMST Editor IDs (649 in
all) against the names, 2026-10-29]. One spelling, `sExplosive`, stands only inside a
comment of `hud_main_menu.xml`, so the reader never asks for it: the reader reads 144
spellings, 142 without case [observed: the archive's menu files, 2026-10-30].
Four more uses lack the closing `;` (`&-sChallengeToggle` in `map_menu.xml`, `&-sStats`
and one `&-sStatsGeneral` in `stats_menu.xml`, `&-sNetTotalText` in
`prefabs\caravan_game_menu.xml`) [observed: same files]; the reader keeps them as plain
text, not names [design: `fnv_ui::xml`; what the original does with them is not known].

Which names the original fills in [observed: private capture `t10_gmst`,
`GetStringGameSetting`, 2026-10-29, the 10 official masters and fo_proof.esp loaded, 146
reads, no errors]: of the 142 names without a record (without case, `sExplosive` among
them), 140 read a text and two, `sModffects` and `sReputationSwap`, read empty. A name no game defines
(`sNvfT10NoSuchSetting`) read empty, and both spellings of `sOK`/`sOk` and of
`sQuit`/`squit` read alike, so names are compared without case. The capture is kept
privately for these points only: what the names read is not recorded anywhere in the
repository and is not used (the rule below). That a plugin's GMST overrides the built-in
text is [hypothesis: as `iLockLevelMax…`, read with `GetGameSetting` though no master
defines them, script-function-audit.md].

**The rule for the text** (the maintainer's decision, 2026-10-30, LEGAL.md "built-in
labels the data files lack are written by the project, not transcribed from the
original") [design]:

- The load order's string game settings win.
- Where the plugins define none, the text is an English label NVForever writes itself
  from two things only: the setting's name and where the menu files use it (the tile's
  kind, its menu, its neighbours, its place: a button row, a title, a prompt).
  `sCloseButton`, a button at the foot of the tutorial box, reads "Close"; `sNext` reads
  "Next".
- The original's text is not looked up by any route: not from screenshots, not from
  `GetGameSetting` or `GetStringGameSetting` readings, not from memory of the game, not
  from any list of game-setting values. An observer dump may say only which names the
  original fills in, not what they say.
- Wording is not changed on purpose either: a short generic label ("Close", "Next",
  "Back", "OK") stays what the use calls for even if it is the original's word. A longer
  label (a prompt, a sentence) is written fresh from what the menu needs. The menu files'
  comments are not copied into labels.
- The two heading labels `sInventoryItems` ("ITEMS") and `sData` ("DATA") are in capitals:
  every use of them is a heading (the Pip-Boy's ITEMS and DATA pages, the repair page, the
  weapon mod menu, the container menu, the barter menu's all-items heading), and NVForever's
  own hand-built Pip-Boy pages already write their headings so ("STATS", "ITEMS", "DATA",
  `pipboy::Page::title`); a page drawn from its file then reads as the hand-built one beside
  it. That is consistency with our own words, not a transcription of the original [design:
  #2212]. The buttons and cards beside them (`sInventoryMod`, `sInventoryRepair`,
  `sRepairItem`, `sRepairSkill` and the rest) keep their spelling.

How NVForever answers `Source::game_string` (`fnv_ui::strings::GameStrings`, task T10)
[design]:

1. the load order's string settings, the last plugin's record winning (the form store's
   one value per setting, or a run-time change made before the files are read);
2. `fnv_ui::strings::BUILT_IN`, NVForever's own labels under the rule above, each entry
   with the name, the label and where the label came from (the setting's name and the
   file and place that use it). The two names the original leaves empty
   (`sModffects`, `sReputationSwap`) have no entry and read empty; `sMorphine` comes
   from its record. The engine's own menus that stand in for menu files (the tutorial
   menu, the lockpick menu and its help) read their words from the same table, by the
   names their files use;
3. a name in neither reads empty, as in the original. The reader puts the text in when
   the file is read (a later `SetStringSetting` does not change a tile already built)
   [design].

The tree records each name it read and whether it had text (`Ui::game_strings_read`); the
engine logs `ui: game strings: N read, M without text` at start for the menus it keeps
loaded (with the vanilla files, `sModffects` of `item_stats_display.xml` is the one name
without text there).

**Unprefixed game-string entities (`&sName;` without `-`)** [observed: MCM 1.5's
`Scale.xml` writes `<string> &sAccept; </string>` and `<string> &sCancel; </string>`,
#2270]: mod menu files sometimes omit the leading `-` on game-setting string entities.
Previously, `fnv_ui::expr::literal` turned any lone entity without `-` straight into
`Value::Sym`, rendering raw `&saccept;` / `&scancel;`, and left unprefixed entities raw in
compound text. [design: `fnv_ui::expr::literal_with`, `fnv_ui::tree::Ui::put_trait`] When
an unprefixed entity is not a known numeric or menu-class constant (`sym_number`),
NVForever checks `Source::game_string` (load-order GMSTs first, then `BUILT_IN`, without
case) for both lone entities and compound text; when found, it expands to that text and
records the read in `game_strings_read` (and `built_in_label` for lone built-in labels).
When not found, it stays a named constant `Value::Sym` (or literal `&name;` in compound
text) without recording a missing game-string read [hypothesis: whether the original reads
an unknown unprefixed `&sName;` as empty or literal text has not been observed; keeping
unmatched unprefixed entities as symbols preserves unknown UI constants such as `&center;`
and `&scale;`].

The names with no record in the vanilla masters, by spelling:

`sAccept`, `sAcceptText`, `sAddText`, `sAllCardsText`, `sAntePotText`, `sAutoAttempt`,
`sAutoMatchText`, `sBack`, `sCWheelTitle`, `sCancel`, `sCaravanBiggestAnteText`,
`sCaravanLossesText`, `sCaravanWinLossText`, `sCaravanWinningsText`, `sCardsInDeckText`,
`sCauterize`, `sCloseButton`, `sConfirmWarning`, `sContinue`, `sCrew`,
`sCurrentAnteText`, `sCurrentObjective`, `sData`, `sDevice`, `sDiscardCardText`,
`sDiscardTrackText`, `sDone`, `sEnterName`, `sExit`, `sExplosive`, `sForfeitGameText`,
`sHackingLockout3`, `sHackingLockout4`, `sHitPointsShort`, `sHowMany`, `sIngredients`,
`sInventoryCaps`, `sInventoryCondition`, `sInventoryDamage`,
`sInventoryDamagePerSecond`, `sInventoryDamageResistance`, `sInventoryDamageThreshold`,
`sInventoryDrop`, `sInventoryEffects`, `sInventoryEquip`, `sInventoryHotKey`,
`sInventoryItems`, `sInventoryMod`, `sInventoryRepair`, `sInventoryStrReq`,
`sInventoryValue`, `sInventoryWeight`, `sInventoryWeightUpper`, `sLevelProgress`,
`sLevelUp`, `sLoad`, `sLoadSavedGame`, `sLockLevelText`, `sLockpickSkillText`,
`sMadeAt`, `sMakeActiveQuest`, `sMedicineSkill`, `sModffects`, `sNavigate`,
`sNavigateText`, `sNew`, `sNext`, `sNextChallenge`, `sNoItemsToRepair`, `sOK`, `sOk`,
`sPCMenuHintA`, `sPCMenuHintE`, `sPCMenuHintF`, `sPCMenuHintQ`, `sPCMenuHintR`,
`sPCMenuHintRMB`, `sPCMenuHintS`, `sPCMenuHintW`, `sPCMenuHintX`, `sPicksRemainingText`,
`sPlaceMarker`, `sPlayAudio`, `sPlayCaravanText`, `sPressStart`, `sPrevious`, `sQuit`,
`sRaiseText`, `sRecipes`, `sRedeemCode`, `sRemoveText`, `sRepairAllItems`,
`sRepairItem`, `sRepairServicesTitle`, `sRepairSkill`, `sReputationSwap`, `sReset`,
`sReturn`, `sSave`, `sSaveGame`, `sSelect`, `sSelectCardText`, `sSettings`,
`sShowActiveNotes`, `sShowAllNotes`, `sShowLocation`, `sSkillRequirement`, `sSpace`,
`sStatsAP`, `sStatsCNDAbbrev`, `sStatsCrippled`, `sStatsEFFAbbrev`, `sStatsFODAbbrev`,
`sStatsGeneral`, `sStatsH20Abbrev`, `sStatsHP`, `sStatsLMBAbbrev`, `sStatsLVLAbbrev`,
`sStatsNoEffects`, `sStatsPerks`, `sStatsRADAbbrev`, `sStatsRadResist`,
`sStatsSLPAbbrev`, `sStatsSkills`, `sStatsSpecial`, `sStatsStatus`, `sStatsXP`,
`sStimpak`, `sStopAudio`, `sSwitchLimbs`, `sTakeAll`, `sTotalCardsText`,
`sTotalFundsText`, `sTravel`, `sTune`, `sVDSGManual`, `sVDSGPlate`, `sVatsAiming`,
`sVatsBodyPart`, `sVatsSelect`, `sVatsTarget`, `sWinLossText`, `sXPProgressMarker`,
`squit`.

## 3. Traits

- A trait holds a number or a string; in the original it is both at once [documented:
  AddTraitToTile notes]. NVForever holds one and reads it as the other on demand:
  - text reads as a number when it is one, else 0;
  - a whole number reads as text without a point; a fractional number prints as the
    shortest text that reads back as the same single-precision float (`f32`), falling
    back to `f64` when `f32` overflows or underflows to zero [hypothesis: original-game
    float-to-string formatting for traits is not captured; single precision avoids
    exposing `f64` rounding tails when code-written `f32` fractions such as `user5`
    are multiplied by XML constants like `_VUI+CNDmax`; design: `f64` fallback outside
    finite non-zero `f32` range].
- A trait is written either as a plain value (`<width> 400 </width>`, `<visible>
  &false; </visible>`, `<string> &-sTitle; </string>`) or as operators (§4).
- Names are compared without case. Traits beginning `_` are the files' own (`_Value`,
  `_SolidMeter`, `_Selected`); `user0` … `user23` are the slots the game's code fills
  [observed: the menus' comments, e.g. barter's `user0` "1 if on PS3"].
- A trait written twice in one tile (often: a prefab sets a default, the including file
  sets it again after) keeps the later one [hypothesis].

### 3.1 The traits the engine draws by

[observed: the most used names; what each does is for the drawing task,
docs/handoff/ui-tile-tree-tasks.md]: `x` `y` `width` `height` `depth` `alpha` `visible`
`locus` `id` `target` `clips` `string` `font` `justify` `wrapwidth` `filename`
`texatlas` `zoom` `tile` `cropx` `cropy` `brightness` `red` `green` `blue`
`systemcolor` `clicksound` `mouseoversound`, and the menu-level `class`
`stackingtype` `menufade` `explorefade`. Input: `xbuttona` …, `xdefault`, `xup` `xdown`
`xleft` `xright`, `_PCButton_*`, `wheelable`, `draggable`.

### 3.1.1 Layout interpretation (T5 part A)

- [design] `nvf-play/src/tiles/layout.rs` produces screen-unit rectangles without
  loading assets or drawing. A subtree root retains the contributions of all its
  ancestors. Hidden items remain in the result with `visible = false`.
- [hypothesis] `x` and `y` add to the parent's top-left; missing values are zero.
  `width` and `height` are the box's size, with zero for missing non-text dimensions.
  No conversion between window pixels and the original's interface units is inferred.
- [hypothesis] `depth` adds down the tree, with zero when absent. Parents precede
  children; siblings sort by increasing depth, preserving file order for ties. Neither
  this note's earlier sections nor hud.md establish cumulative depth or traversal order.
- [hypothesis, T15a] A `hotrect`'s own `alpha` is not passed to its children: every
  vanilla text button is a hotrect of alpha 0 (`text_box.xml`) whose text sets its own
  alpha (128, or 255 while it is a target), the message box's list and its rows
  (`list_box.xml`, `list_box_template.xml`) are alpha-0 hotrects over texts the original
  shows (the message box's OK, the prompt's Ok and Cancel on the owner's screenshots,
  message-menu.md §3, barter-menu.md §7), and the list's highlight box, alpha 40, draws
  its lines at full strength. Until T15a it multiplied through as the other tiles' does,
  which hid those texts. [design] The exception now applies only while a T15 menu is
  switched on (`tiles::interface_screen`, the same gate as interface `screen()` units).
  With both T15 menus off, including the tile HUD alone, hotrect alpha still multiplies
  through mod HUD tiles and other layouts as before T15a. Applying the exception
  unconditionally is a candidate for its own PR.
- [hypothesis] `alpha` is 0–255, defaults to 255, and multiplies through ancestors as
  fractions of 255. [design] Clamp each local alpha to that range and retain fractional
  results without rounding.
- [hypothesis] A false `visible` hides the entire subtree; missing means true. A true
  child cannot override a hidden parent.
- [hypothesis] True `clips` crops children to this tile's rectangle. Nested ancestor
  clips intersect; a disjoint intersection is empty. The tile's own box is unchanged.
  [observed: #4685, a player's screenshot of the original's World Map] Not for the map's
  dotted line: `PathLineTemplate`'s line is a child of a `WaypointTemplate` node (about 17
  units, `clips` true) and is drawn from the arrow to the far marker, far outside the
  node's box. As with list rows (pipboy.md, "List-row `<clips>`"), NVForever marks the
  lines `OUTSIDE_CLIP`, so the map's window above the node cuts them and the node does
  not [design]. Whether `clips` crops children anywhere in the original is still open.
- [observed] True `clipwindow` crops a tile's descendants to its rectangle in the
  drawing: the tutorial menu's scrolled text stops part way through a line at the column's
  top and bottom edges (private capture hud_tutorial_more, 1280 x 720, shots
  tutorial__scroll_bottom and tutorial__scroll_arrow). [hypothesis] The same for the pointer
  (`fnv_ui::input::clips_children`): `list_box.xml`'s highlight box takes its `clips` from
  its parent's `clipwindow` [documented: the file]; nothing captured says so (observer
  check 196). It stays apart from scrolling: nothing here moves a tile; a file moves its
  text by the `y` it writes. [documented: the vanilla files] It is true in the book,
  lockpick, map, stats, credits, load, save, trait and tutorial menus and in the prefabs
  `item_listing`, `list_box` and `load_and_save_menus`, so in every menu that includes
  them, the message box and the start menu among them; not in the HUD's files.
  [design] Mods' tiles carry it too (MCM's `ModList.xml`), and the default game draws
  mods' HUD tiles, so it is read only where a menu's model turned it on
  (`fnv_ui::input::mark_clip_windows`, the private trait `_NVFClipWindow`): today the
  tutorial menu alone, while drawn from tiles. Off, every menu lays out and takes the
  pointer as before. Reading it everywhere is a follow-up, with the other menus' checks.
- [design] `menu` and `hotrect` contribute layout to children but emit no drawing item;
  hotrect input is T8 (§3.2). `rect`, `image`, `text`, `nif`, `radial` and `window` retain
  distinct kinds; producing a layout item does not implement model or radial drawing.
- [design] Image `filename` is passed through (empty if absent); `texatlas` is retained
  as a raw value for later atlas resolution using interface.rs's existing `.tai` reader.
  [hypothesis] Missing `zoom` means 100 percent. [design] Explicit `zoom`, `tile`,
  `cropx` and `cropy` are passed as numbers without changing the box; absent tile/crop
  inputs remain absent. The renderer applies `cropx`/`cropy` to a picture drawn at its
  texels' size times its zoom: the point of the zoomed picture put at the box's top left
  [documented: GECK wiki "Tutorial: Understanding XML" and "Cropx", "applied after the
  zoom trait"] (`tiles::draw::Piece::crop`); a stretched or repeated picture takes no crop
  [design].
- [design] Text `string` (empty if absent), numeric `font` (zero if absent) and optional
  `wrapwidth` go to a caller's measuring callback. Only absent width/height are replaced
  by the measured dimensions; explicit zero dimensions stay zero.
- [hypothesis] Named `justify` constants `&left;`, `&center;`, `&right;` anchor the text
  box's left, centre or right edge at x, leaving y at the top. [design] Numeric `justify`
  is retained for the renderer; its unknown mapping is not invented. Missing, unknown
  or numeric justification uses a left anchor. Named constants also retain their
  alignment separately from their numeric view (§2.3).
- [hypothesis] Local `red`, `green`, `blue` and `brightness` default to 255.
  [design] RGB clamps to 0–255; brightness passes unchanged. Colour does not multiply
  through ancestors. Numeric `systemcolor` is retained for the renderer to map; unknown
  named constants retain their names as well, and absence stays absent.
- [design] `locus`, `id`, `target`, input traits, menu fades and stacking traits have
  no extra layout effect yet. No undocumented meaning is assigned to them.

### 3.1.2 Drawing interpretation (T5 part C)

`nvf-play/src/tiles/draw.rs` draws the tiles mods added to `HUDMainMenu` over
NVForever's own HUD, from the layout of §3.1.1. What it takes the traits to mean:

- [documented: hud.md §14] Places are interface units, 960 to the view's height at
  every size (`screen()` is 960 × width/height wide); [design] one unit is
  height/960 window pixels (`interface::u`), as the current HUD.
- [design] Which tiles: §7's `mod_added_tiles` roots, and inside them each tile
  `mod_tile_drawn` accepts: Injected and Template always, Code never, File tiles only
  against the archived file's name paths. Since #975 `fnv_vfs::Vfs` keeps the highest
  archive's copy under each loose file (`Vfs::read_archived`), and the game and `fnvtool
  ui mod-tiles` both compare a loose replacement with it (`Ui::archived_name_paths`);
  before, the game drew no File tile, as the VFS kept one location per path. `fnvtool`'s
  comparison over the test install (2026-10-29, overlays [observed]): vanilla 0 tiles,
  Clean Vanilla Hud's loose HUD 0, Vanilla UI Plus 5, its `VUI+` outline tiles in the
  prompt box.
- [design] A `rect` draws a filled box only when it sets `red`, `green` or `blue`
  itself; otherwise it only places its children. `nif`, `radial` and `window` tiles,
  and empty pictures and strings, draw nothing in v1 (the whole HUD's drawing draws
  `nif` tiles, §3.1.3).
- [design] An `image` is its `filename` stretched over its box: the piece of
  `interfaceshared.tai` of that file name when the atlas has it, else the file under
  `textures\` (`interface::Pictures`). Another `.tai` named by `texatlas`, and `zoom`,
  `tile`, `cropx`, `cropy`, are not applied to the mods' tiles in v1 (the whole HUD's
  drawing applies `texatlas`, `zoom` and `tile`, §3.1.3).
- [hypothesis] `font`: 1 to 9 is the INI's `sFontFile_N` [documented:
  Fallout_default.ini]; none, 0 or another number is font 1. The sneak word's tile has no
  `font`, and the original's widths for it (hud.md §14.1: 106 "[HIDDEN]", 136, 14) are
  font 1's glyph advances summed for "[HIDDEN]", "[DETECTED]" and one space [observed:
  the font files, 2026-10-29].
- [observed: the font files against hud.md §14.1] A text's width is the sum of its
  glyphs' advances (fonts.md §3): font 7 "Button Text" 156 and "Talk" 54, font 8 "+0" 50
  and "+9" 49, as the original wrote. A `wrapwidth` of 0 does not wrap (the prompt's
  `button_text` has 0 and measured one line). [hypothesis] Each line is set left,
  centred or right inside the box by `justify`; [observed: hud_menus capture
  2026-10-31, fonts.md §3.1] the lines stand one pitch of their font apart
  (`draw::line_pitch`: 31, 35, 32, 36, 25, 38, 38, 52 for fonts 1-8), which for fonts 2,
  3, 6 and 7 is not the font file's size (until this change NVForever set them the size
  apart).
- A text's first line stands under its top by its font. [observed: hud_textbase capture
  2026-10-31, hud.md §14.10] Measured from the capitals' ink in the original against
  where NVForever's glyph pictures put it (fonts.md §3), the lines stand 24.9 units under
  the top in font 7, 30.5 in font 8 and 21.0 in font 1 (a tile with no `font`).
  [observed: hud_menus capture 2026-10-31, fonts.md §3.1] Every font, measured the same
  way at three window sizes: font 1 20.3, fonts 2-5 about 16.5, font 6 22.4, font 7
  24.5, font 8 30.5. [design: NVForever's calibration, anchored on font 7]
  `draw::text_base` keeps font 7 at the earlier readings' 24 and stands every other font
  as far from it as the hud_menus capture shows: font 1 20, fonts 2-5 16, font 6 22,
  font 8 30 (29.5 from hud_textbase until then; fonts.md §3.1 says why 30); font 9, which
  draws nothing in the original, takes font 7's 24 [design]. Texts of fonts 7 and 8 set at one `y` do not
  stand on one line: `radiation_text` ("RAD/SEC", font 7) and `radiation_text_value`
  ("+9", font 8), both at y 65, end 25.0 and 31.7 under it. T12c (2026-10-29) stood
  every font's line 24 under the top [hypothesis then: the two radiation texts read on
  one line 23.7 under their `y`, the sneak word (font 1, `SneakMeter` y 30) 22, the
  prompt's lines (font 7) 24.5-25.5 under the captures' places, private shots
  `hud_corners` rad_coc_spot, `hud_jip` s4_sneak_drawn, hud.md §14.8]; the font-8
  reading was wrong (#979). Until T12c each font's tallest glyph set the line
  (`interface::line_base`: 28, 33, 48 for fonts 1, 7, 8).
- [observed: hud_textbase and hud_menus captures 2026-10-31] A text tile's `height`,
  read in the original, is its font's tallest glyph picture whatever its string, plus
  one pitch for each line after the first: one line 36, 32, 31, 37, 28, 38, 42, 64 for
  fonts 1-8, two lines 67, 67, 63, 73, 53, 76, 80, 116 (fonts.md §3.1). NVForever's
  `draw::measure` gives that height (`tallest_glyph` + (lines - 1) × `line_pitch`; until
  this change lines × the font's size); the layout uses it for a text tile without its
  own `height` (§3.1.1), and the code writes no text tile's `height` into the tree
  [design: not yet needed].
- [hypothesis] Colour: a tile's own `systemcolor`; else its own non-black
  `red`/`green`/`blue` when it sets any; else the nearest tile above's `systemcolor` (so
  child prefabs that copy missing `red`/`green`/`blue` traits from a parent — evaluating to
  0/0/0 — still inherit an enclosing menu's `systemcolor`, while `&nosystemcolor;` /
  `&uselocalcolor;` / `0` on the tile or ancestor yields local RGB); else its own
  `red`/`green`/`blue` when no ancestor sets `systemcolor`; else system colour 1, the
  HUD's [documented: GECK wiki Color UI directives, "Systemcolor is by default 1 and always
  assign"; T12c: until then its own, white, which drew `AmmoTypeLabel`, a menu child with
  no colour above it, white].
  `&hudmain;` is the HUD's colour (`interface::hud_rgb`, the settings'), `&pipboy;` the
  Pip-Boy's, `&nosystemcolor;` and `&uselocalcolor;` the tile's own, other names and
  unknown non-zero numbers the HUD's [hypothesis]; numeric 1/2 map to hudmain/hudalt
  [observed: private job hud_jip file_0032; hud_hardcore2 hc4-hc5]. `&hudalt;` is 255 67 42 [observed: Fallout_default.ini
  `iSystemColorHUDAlt*`, radiation.md §6]. `brightness` (clamped to 0–255) scales the
  colour, `alpha` (§3.1.1's product) is its opacity.
- [hypothesis] Stacking: the pieces draw in order of their summed `depth`, ties in
  layout order; all over NVForever's HUD and its messages, under the dialogue menu and
  the Pip-Boy [design: `GlobalZIndex` 60]. `clips` crops through a clipping node at the
  clip rectangle.
- [observed: hud.md §14.1, the font files] The code writes three HUD text tiles' `width`
  as their text's measured width, before scripts: `SneakMeter/sneak_nif`,
  `Info/justify_center_hotrect/button_text` (its hotrect's width follows by the
  prefab's own expression, the child's width plus `_horbuf` [documented: text_box.xml])
  and `RadiationMeter/radiation_text_value` (`fnv_game::ui_hud::write_text_widths`),
  only with the game's fonts. A tile without a `string` keeps its width [design].
- The other HUD text tiles whose file gives no `width` (the ones mods add) are measured the
  same way, in their own font and wrap, again when their string changes; the engine marks
  the width it wrote (`_NVFMeasured`) so it keeps measuring that tile (#4493)
  [hypothesis: the original measures every text tile; observed in FadedLight's Nuclear
  Sunset fork: MAPMO's clock centres itself by its own width, read 0 and ran off the right
  edge] (`fnv_game::ui_hud::unmeasured_texts`, `nvf-play/src/tiles/draw.rs`).

### 3.1.3 The whole menu, `zoom`, `tile`, `texatlas` (T12a)

With `[UI] TileMenus=HUDMainMenu` (ui-tile-tree.md §6.2; hud.md §14.7) the renderer draws
every tile of the HUD, not only the mods' (`nvf-play/src/tiles/draw.rs`,
`Select::Whole`); then, and only then, it applies these. With the switch off §3.1.2
stands unchanged: every picture stretched, the shared atlas only.

- [documented: GECK wiki Zoom, revision 39942] `zoom` multiplies an image's size and
  "cannot exceed the specified height and width"; the default is 100; -1 is free scaling
  ("Scales any texture to 256x256 regardless its source size"). [hypothesis] A number
  draws the picture at its texels' size times `zoom` / 100, one texel one interface unit
  at 100, from the box's top left, cut to the box: the compass window (345 x 64, zoom 100)
  then shows 345 of the 1024-texel strip, which turns 1024 units [observed: hud.md §14.4
  `filewidth` 1024, §14.5 the scale]. The anchor is not observed.
- [hypothesis, T12c] No `zoom` (`draw::Fit::Natural`, `draw::placed`): an atlas piece
  is stretched over its box, a picture read from its own file stands at its texels' size
  (zoom's default 100) from the box's top left, cut to the box. The corner brackets'
  atlas pieces are 255 x 127 texels [observed: interfaceshared.tai, an atlas of 1024 x
  1024 by its heading] in 512 x 256 boxes the original fills on the screen; the
  radiation pointer's 63 x 31 piece is drawn shrunk in its 46 x 24 box (its triangle
  about 10 pixels across at 720 lines, 15 at its texels' size); the radiation bracket,
  its own 512 x 128 file in a 567 x 120 box, stands unstretched: its notches 60 units
  apart where its file has them 60 texels apart, its upright at W - 57 on the screen,
  where x -135 + 494 texels puts it (stretched it stood at the view's edge, T12b)
  [observed: private shot `hud_corners` rad_coc_spot]. Until T12c every unzoomed picture
  was stretched.
  [hypothesis] `&scale;` (number 0, §2.3) stretches: its pictures (the compass ticks
  32 x 32 in 32 x 32 boxes, the condition arrows, the sneak bracket) fit their boxes.
- [hypothesis] `tile` true repeats the picture over the box from its top left at its
  texels' size times a numeric `zoom` / 100 (100 without one), both ways: the bars'
  8-texel tick picture in widths the code sets in steps of 8 [observed: hud.md §14.3];
  the template's `_ImageWidth` 8 says the same [documented: hudtemplates.xml
  `template_meter`]. Zoom as a multiplier of the picture's size [documented: GECK wiki
  Zoom]; no vanilla tiled picture has a zoom; T12c's condition fill is given one
  (`vanilla::CONDITION_FILL_ZOOM`, hud.md §14.7) [design].
  [observed: #4685, a player's screenshot of the original's World Map] `tile` `&scale;`
  (only `PathLineTemplate`'s dotted line; `&scale;` is the number 0, so not a true
  `tile`) repeats the picture along the box at the box's height, its shape kept: the
  original's line is a row of whole dots. Read as a false `tile`, the 64 x 16 picture
  stood once at its texels' size and the 5.8-high line showed a sliver of one dot
  (`draw::Fit::TiledToHeight`) [design: the repeat at the box's height, as the hand-built
  line's `PATH_TILE`].
  [design, T12d] The repeats are drawn as one image: the picture's texels (its atlas
  piece's or its own file's, decoded on the CPU) repeated into an RGBA image the box's
  size in whole texels, stretched over the box (cut to it) and sampled linearly
  (`nvf-play/src/repeat.rs`), so a repeat stands `width x scale` pixels from the next at
  any window size; a repeat per quad (Bevy's `NodeImageMode::Tiled`, T12a-T12c) put each
  on whole pixels, the bars' 8-unit ticks 6 and 7 pixels apart by turns at 1280 x 800
  [observed: the owner, T12d; hypothesis: the original's even, slightly soft ticks are
  one repeated texture filtered across the bar, hud.md §14.9]. A box over 4M texels, or
  a picture whose texels cannot be read, keeps Bevy's tiles.
- [design, T12d; maintainer's review of #902] The renderer keeps each piece's nodes in a
  slot of its own (by tile, stacked by the piece's place in the list) and compares the
  pieces with the last frame's one by one (`draw::change`): a piece that only moved or
  changed colour (the compass marks every frame while the player turns, the fades and
  flashes) has its nodes moved and recoloured; a piece of another look or size is
  spawned again alone; a piece that comes or goes adds or removes its slot.
- [observed, T12c] An image without `texatlas` reads its own file, not the shared atlas
  (`draw::Source::Own`): the radiation bracket, whose file comments its `texatlas` line
  out (radiation.md §6), shows its own file's slanted ends and single line on the
  original's screenshots, where the shared atlas's piece of that name has square ends
  and a doubled line. The mods' tiles with the switch off keep §3.1.2's lookup.
- [observed, T12c] **The atlas pieces are other art than the pictures' own files**, not
  smaller copies: the corner brackets' pieces have cut corners, stepped arrow ends and a
  doubled long line, the compass strip's a doubled line beside the letters and thick
  middles, where the own files have square corners and single lines (both decoded from
  `Fallout - Textures2.bsa`). The original's screenshots of the corners (`hud_corners2`
  c0, c4) show the atlas pieces' shapes, as corner-messages.md §2.2 found for the
  message bracket; the tile drawing draws them. The hand-built HUD (`hud.rs`) draws the
  own files.
- [documented: GECK wiki Texatlas; interfaceshared.tai's heading] `texatlas` names a
  `.tai` index under `textures\`; each line gives a picture's file name, the atlas file
  (`InterfaceShared<idx>.dds`) and its place as parts of the atlas. Any index a tile names
  is read once and kept (`interface::Pictures::locate`); [hypothesis] its atlas textures
  stand in the index's folder, as the shared one's. [design] A picture the named index
  lacks is looked up in the shared index, then as its own file.
- [design] `clips` as §3.1.2, a clipping node, and `clipwindow` where §3.1.1 reads it;
  the HUD's own clip tiles (`compass_icon`) clip their children only.
- [hypothesis, T12e] A text's `zoom` scales neither its glyphs nor its laid-out box: the
  Hardcore words read zoom 75 [observed: private job hud_hardcore2 hc0-hc11] and their
  tiles' widths are font 7's at 100 (45 for "H20", as `HitPoints`' "HP" 30) [observed:
  the capture, the font file], and on the original's screen they are as tall as "HP"
  (17 pixels at 720 lines) [observed: hud_compare needs_limbs, hud.md §13]. T12b scaled
  both by zoom/100 [hypothesis, corrected T12e]. The trait is kept as read
  (`layout::Text::zoom`).
- [design] The compass strip is the one picture moved by code with no trait (hud.md
  §14.4): `HUDMainMenu/HitPoints/compass_window`'s picture is drawn twice side by side
  and moved by the heading as the hand-built HUD moves its own (1024 units a turn, the
  marks' heading at x 148 of the window [observed: hud.md §14.5], the strip's N at 154 at
  heading 0 [observed: hud.md §14.10, T12e]), faded out from x 273 to 341 of the window
  as the original's [observed: hud.md §14.10]: the window's part of the strip baked into
  one image with each texel column's alpha on the ramp, as the hand-built HUD's
  (`crate::compass_strip`) [design: one image, no seams between parts].
- [design] `nif` tiles (the HUD's one, the breath meter) are drawn with the whole menu:
  the `filename` under `meshes\`, its flat textured squares as turned pictures in the
  tile's colour and alpha, the `animation` sequence's vis keys at the time NVForever's
  own `_NVFSequenceFraction` gives (hud.md §14.12, `tiles/model.rs`). [hypothesis] The
  model's origin stands at the tile's `x`, `y`, a model unit an interface unit; the
  tile's `width` and `height` do not scale it.

### 3.1.4 Turned tiles: `rotateangle`, `rotateaxisx`, `rotateaxisy`

What the traits mean, as read:

- [documented: GECK wiki "Rotation UI directives", revision 39961] `rotateangle` turns a
  tile by an angle in radians (its example: 1.570796, "Pi/2 rotation"); `rotateaxisx` and
  `rotateaxisy` name the point turned about, "relative to X coordinate of an element" and
  to its Y coordinate (its example: `width` / 2, "the X center of the element"); "Those
  traits won't work if element has children".
- [documented: `menus\main\map_menu.xml`] Only the map menu uses them, and only the axis
  traits: the two player arrows (`MM_LocalMapCursor`, `MM_WorldMapCursor`) turn "about its
  center" (`width` / 2, `height` / 2), and `PathLineTemplate`'s dotted line about its
  "leftmost-center point" (0, `height` / 2). No file writes `rotateangle`: the original's
  code writes it (the arrows by the player's heading, the lines by their direction
  [hypothesis: by what they show]). No file turns a text, a rect or a tile with children;
  none of the turned tiles has `target`.
- [hypothesis] A positive angle turns clockwise on the screen (the view's y runs down):
  the wiki gives no sense, and a heading runs clockwise from north. Without the axis
  traits the point is the tile's top left (0, 0).

What the code does (`nvf-play/src/tiles/layout.rs` `Turn`, `tiles/draw.rs` `turned`):

- [design] Layout keeps the tile's unturned box and adds its turn (angle, and the point
  from the box's top left in interface units) to its item; none at angle 0 or an angle that
  is not a number, so an unturned tile behaves as before (no transform, rounded as before); none for a tile with
  live children (the wiki's rule), and a turn never passes to children.
- [design] A box's or a picture's piece is drawn turned (its outer node takes a Bevy
  `UiTransform`: the rotation, and the move that puts the turn about the point instead of
  the node's middle, `Turn::shift`); a piece only turned further is turned in place, its
  nodes kept. A turned node and its clip wrapper are laid out unrounded (Bevy's
  `LayoutConfig`), as the shift is worked out from the box's unrounded size and place and
  Bevy turns about the middle of the size it lays out; unturned nodes are rounded as
  before. The clips above it (`clips`, a turned-on `clipwindow`) stay unturned boxes
  and cut the turned quad on the screen (Bevy clips a transformed node by its clip
  rectangles). A `tile` picture is turned whole, its repeats with it.
- [design] Texts, models and the compass strip are not turned (no file turns one).
- [design] The pointer is tested against a turned tile's unturned box (`fnv_ui::input`),
  as no turned tile in the files takes the pointer.

### 3.1.5 Pictures the code sets: runtime images

[documented: `menus\main\map_menu.xml`] Some images have no picture in their file and are
given one by the original's code (the Local Map's `MM_LocalMap_ParentImage`; pipboy.md
§5.3.1). NVForever's code makes such pictures itself (the Local Map's outline,
`nvf-play/src/pipboy/data/outline.rs`). What the code does (`nvf-play/src/tiles/runtime_images.rs`):

- [design] A `filename` that starts with `nvf:` (any case) names a picture the code made,
  kept in the `RuntimeImages` resource by name with its texture, its size in texels and a
  revision. No game path starts so (the menu files name pictures under `textures\` by
  relative paths; in Windows paths a colon only follows a drive letter). It is looked up
  before the atlas indexes and the files, and never enters the file-name cache.
- [design] Each `RuntimeImages::set` or `touch` is a new revision. The renderer stamps the
  picture's piece with its entry's revision and size before comparing the frame's pieces
  with the last ones, so a new texture, the same texture changed, an entry set late,
  removed or resized repaints the piece under the same name.
- [design] It fills its box as any picture by `zoom` and `tile` (§3.1.3; `zoom` -1
  stretches it over the box). Its `tile` repeats are drawn by Bevy's tiles, not baked
  into one image (the baking reads files); SetUITexOffset and SetUIAlphaMap are not
  applied to it (they read files); each said once in the log. A name with no entry draws
  nothing and says so once.

### 3.2 The pointer and keys (T8)

What the game's code writes for input, and how `fnv_ui::input` reads the files for it.
Wiki pages in our own words [documented: the GECK wiki's Target, Mouseover, Clicked,
Shiftclicked, Ref, Input, Hotrect, Wheelmoved pages and "Tutorial: Understanding XML"
§3.2 c), export of 2026-10-28].

- [documented] `target` true makes a tile react to the pointer, the keys and a
  controller; `mouseover`, `clicked`, `clicksound`, `mouseoversound`, `wheelable`,
  `draggable` need it. A hotrect with `target` true makes its children targets too,
  even with their own `target` 0. A hidden tile (`visible` false, its own or an
  ancestor's) is neither moused over nor clicked, but a script can still click it.
- [documented] `mouseover` is 1 while the tile has the pointer (or a controller's
  highlight) and 0 otherwise. [hypothesis] Only on the tile that takes the event (below),
  not on the tiles above it; observer check §8 item 9.
- [documented, disagreeing] `clicked`: "set to 1 by the engine for a single frame after
  the element is clicked by the mouse" (the tutorial) against "it will count how many
  times a element has been clicked" (the Clicked page). [hypothesis] The single frame
  (`fnv_ui::input::CLICKED_RULE`, `ClickedRule::OneFrame`; `Count` is the other); a
  press, not the release, clicks. Observer check §8 item 8. `shiftclicked` is 1 for a
  frame when Shift is held during the click [documented].
- [documented: tutorial] A click sets `clicked` on a tile with an `id`; the Clicked page
  says on any target. [hypothesis] `clicked` on any target, the `id` being what the
  menu's code reads.
- [documented: Target] `target` has no effect on a `rect` (the tutorial says "rect or
  image"); [design] `rect` and `menu` tiles are never under the pointer.
- [observed: the vanilla menu files, read privately] Every text button is the text_box
  prefab: a target hotrect with `alpha` 0 and its text, box and key hint as children;
  the save list's rows and the scrollbars are target hotrects at alpha 0 too.
  [hypothesis] `alpha` therefore does not stop the pointer; a hit on a child the hotrect
  made a target goes to the nearest tile above it whose own `target` is true.
- [hypothesis] Boxes are the layout's (§3.1.1): `x`/`y` added to the parent's top-left,
  `width`/`height`, a text measured when it has no size and moved by its `justify`,
  `clips` (or a `clipwindow` turned on, §3.1.1) cropping the children; `locus` is not
  read, as the layout does not.
- [documented: tutorial, the higher `depth` is drawn on top] [hypothesis] Where targets
  overlap, the highest summed `depth` takes the pointer, a tie going to the one drawn
  later; a menu opened later is over an earlier one whatever their depths. Observer
  check §8 item 7.
- [observed: the vanilla files] `_PCButton_<key>` appears only on the `<menu>` element
  (61 times, any case: `_PCButton_E`, `_PCBUTTON_S`), its value the name of a tile; the
  letters used are A, B, E, F, Q, R, S, W and X. Pressing the key clicks that tile
  [documented: the menus' key hints, e.g. lockpicking.md §2 "E is Exit"]; LStewieAl's
  Tweaks "extends vanilla XML keybindings" to digits, F1-F12, Caps, Tab and Alt
  [documented], so the unmodded game reads letters. [hypothesis] The tile is the first of
  that name in the menu, and it must be shown and a target.
- [documented: Input] The controller's traits `xbuttona` `xbuttonb` `xbuttonx`
  `xbuttony` `xbuttonlt` `xbuttonrt` `xbuttonlb` `xbuttonrb` `xbuttonstart`, and `xup`
  `xdown` `xleft` `xright`, which the arrow keys press too; the stick buttons are
  `&xbuttonls;` `&xbuttonrs;` [documented: Hardcoded UI Shortcuts]. `xdefault`'s meaning
  is unknown [documented: Input] and is not read. [observed: the vanilla files] Their
  value is a `<ref src="…" trait="clicked"/>` on the `<menu>` element (the button the
  key clicks) or `<ref … trait="mouseover"/>` on list items (`xup`/`xdown` to the
  neighbour); one `xscroll` refs `_value_adjust_poke`. [documented: Ref, Input] A `ref`
  to `mouseover` moves the highlight there; one to `clicked` clicks that tile.
  [hypothesis] The highlighted tile's trait first, else the menu's, also when the
  tile's ref does nothing (it names no tile, or one hidden or not a target); a key-moved
  highlight holds until the pointer moves or its tile is hidden or stops being a target;
  a ref to another trait is reported, not followed [design].
- [documented: ClickMenuButton, SetOnMenuClickEventHandler] `Menu#<id>` names the tile
  of that `<id>` in the menu. [hypothesis] The first in file order when two share one.
- [documented] Wheelmoved, Draggable (`dragx` …), Repeathorizontal: not built (T8 left
  them; `wheelable` is the next step for lists).
- [design] NVForever: `fnv_ui::input::hit` (the tile under a pointer in interface units,
  960 to the view's height as the tile renderer draws), `Pointer::step` (one frame:
  pulses ended, the pointer followed, the press, the keys), `fnv_game::ui_input::step`
  (that and the JIP events); the engine feeds only the menus drawn from tiles that take
  input (none in play yet: the HUD takes none; the test aid `NVF_PLAY_TEST_POINTER`
  makes it take input). GetActiveUIComponentName/FullName/ID read the highlighted tile,
  empty and -1 with none [hypothesis: observer check §8 item 10]; ClickMenuButton (a
  target with an `id`, `clickTimes`) and ClickMenuTile (any target, hidden too) click as
  a press does.

## 4. Operators

Inside a trait, operators work the value out from the first to the last, each on the
value so far.

### 4.1 The operators

[observed: every operator tag in the vanilla files; the meaning of each is a hypothesis
from its name and use unless marked]

| Tag | Does |
|---|---|
| `copy` | the value becomes the argument |
| `add` `sub` `mul` (`mult`) `div` `mod` | arithmetic with the argument; dividing by 0 leaves the value |
| `min` `max` | the smaller / larger of the value and the argument |
| `floor` `ceil` `round` `trunc` `abs` | round the value; the argument (always 0 or 1 in the files) is not used |
| `rand` | a whole number from 0 below the argument |
| `eq` `neq` `lt` `lte` `gt` `gte` | the value becomes 1 or 0 by comparing it with the argument |
| `and` `or` | 1 or 0 from the value and the argument as true/false |
| `not` | 1 when the argument is false, else 0 |
| `onlyif` / `onlyifnot` (`onlynotif`) | when the argument is false (true), the value becomes 0 (§4.3) |
| `ref` | the value becomes the argument; a live link (input traits use it) |

Both `mul` (193 uses) and `mult` (120) are in the files; `onlynotif` (3 uses) is read as
`onlyifnot`. MCM's `start_menu.xml` uses a `<neg>` no vanilla file has; it is ignored
with a warning.

### 4.2 Arguments

An operator's argument is one of:

- **a literal or constant** written inside it: `<add> 2 </add>`, `<eq> &center; </eq>`;
- **another tile's trait**: `<copy src="parent()" trait="width"/>`;
- **operators nested inside it**, worked out on their own first:
  `<sub> <copy src="parent()" trait="width"/> <div> 2 </div> </sub>`.

The `src` forms [observed: counts over the vanilla files; meanings hypothesis unless
marked]:

| `src` | Uses | The tile |
|---|---|---|
| `parent()` | 2473 | the tile's parent |
| `me()` | 1798 | the tile itself |
| `sibling(name)` | 1660 | the tile of that name under the same parent |
| `sibling()` | 2 | the tile just before it under the same parent |
| `screen()` | 451 | the view: traits `width`, `height`, `cropx`, `cropy`, `resolutionconverter`, set by the engine. Width is 960 × window width/height, height 960 and `resolutionconverter` 960 / window height [observed: hud_menus capture 2026-10-31, three window sizes] (1280 x 720, 1920 x 1080 and 1280 x 1024). NVForever uses those interface units only while `[UI] TileMenus` names a menu drawn from its file (`tiles::interface_screen`), and writes `cropx` and `cropy` only while `DialogMenu` draws a conversation (dialogue-menu.md §6.2); otherwise it keeps window pixels and no converter [design: preserve default play; applying the observed units unconditionally is a follow-up]. `fnvtool ui tree --screen` writes the size it is given [design] |
| `globals()` | 244 | `globals.xml`'s tile (`gloabls()`, once, read the same) |
| `io()` | 242 | the menu the tile is in, whose traits the game's code reads and writes (barter reads `io()`'s `_IsContainerListSelected`, which its `<menu>` declares) |
| `child(name)` | 148 | the child of that name; `child()` the first child |
| `grandparent()` | 87 | the parent's parent |
| `parent(name)` | 31 | the nearest tile above of that name |
| `strings()` | 10 | interface strings (`_weapons`, `_apparel`, `_button`, …); which ones, and from where, is not known |
| a bare name | ~1100 | the first tile of that name in the same menu, else the menu of that name (`StatsMenu`, `MM_Tabline`) |

A trait that is not there reads 0 [hypothesis].

[design] Named expression sources keep their existing precedence: the first
descendant in file order, then a menu of that name, then a bare built-in source
such as `parent` or `me`. `Ui::find_named` indexes the first descendant of each
case-insensitive name once per tree shape. Making, unloading, renaming or otherwise
reshaping tiles invalidates that index; value writes reuse it. The index contains
only the current loaded tree, so menu reloads and reused slots cannot retain old
IDs. The fallback order and expression/write-override rules are unchanged.

Owned synthetic controls call `Ui::update` with bare-source expressions on many
rows, bound total name-lookup node visits to linear work, and report fixed-workload
times without using wall time as a pass threshold. Separate controls retain
named-before-built-in precedence, nested duplicate order, another-menu fallback,
unloading, injection and menu reload. These are core implementation checks, not
original-game captures or a measured installed-mod frame-rate result.

### 4.3 `onlyif`

`onlyif` never comes first: 196 are last in their list and 99 in the middle, always
after a `copy` [observed]. Read as "when the condition is false the value so far
becomes 0, and the operators after it go on from there" [hypothesis], which fits every
use found:

- `copy 40, onlyif _box_visible` is 40 or 0;
- `copy X, onlyif C, add Y` is X + Y or Y;
- inside a nested `sub` (`copy width, div 2, onlyif centred`) the whole subtraction is
  0 when the condition fails;
- `globals.xml`'s `_line_thickness` (`copy 1, onlyif &console;, add 2, mul
  resolutionconverter`) is 2 × the converter on a PC.

### 4.4 A trait name that ends in `_`

`<copy src="me()" trait="_filename_"/>` after the value is a number n reads the trait
`_filename_n` [hypothesis: the meter and barter files pick a picture that way; 15
names end in `_` in the vanilla files, all after a numeric value]. After a constant it
reads the constant's number (§2.3; `&false;` 0, `&true;` 1) [hypothesis, T15a: the meter
prefab copies `_SolidMeter` and `vertical_fade_line.xml` copies `_bottom_to_top`, both
`&false;` or `&true;`, before the copy, and their `_filename_0`/`_Filename_1` are the two
pictures; until T15a the name was read without a number, and the message box's corner
lines and the prompt's meter had no picture].

## 5. Paths

[documented: the GetUIFloat, SetUIFloatAlt, ModUIFloat, GetUIString and InjectUIXML
pages]

- A path is `MenuName/Tile/…/Tile/trait`, segments split by `/` or `\`, names without
  case.
- `*` matches any child; `name:N` matches the N-th child of that name and `*:N` the
  N-th child [documented: "`*:0/*:5/*:9`"]. How N counts depends on the function:
  - JIP's functions (`GetUIFloatAlt` and the rest) count from zero **from the last
    matching child**, reversing file/creation order [observed: private jobs hud_hardcore
    hc0, hud_xp_limbs all, hud_prompt2 p0-p8, all read with GetUIFloatAlt; hud.md §14.8].
  - The extender's own `GetUIFloat`, `SetUIFloat`, `SetUIString` and `SetUIStringEx`
    count **from the first child, from 1**, with 0 the last and nothing past the one
    before the last [observed: GetUIFloat under the pause column, `*:1` its first child,
    `*:3`-`*:7` the rows Continue to Help, `*:0` Quit, `*:8` of eight nothing, §10.1;
    documented: MCM Guide v6, `StartMenu/MCM/*:4` is MCM's fourth child and `*:1/*:2`
    Option2, mcm.md §2.1] (`fnv_ui::Count`, #2298). MCM mods write their pages by these
    places; counted from the last they reached the wrong tiles.
  Unindexed matching still searches in file order [hypothesis; the pause column's
  `lb_item_hotrect` read Quit, the last, with GetUIFloat, §10.1: open].
- A source written without brackets (`src="parent"`, `src="me"`, `src="sibling"`,
  `src="globals"`, …) is the source it names when no tile bears the name [observed:
  Vanilla UI Plus writes them so throughout its menus; mcm.md §2.5].
- With `*`, the first child that leads to a full match is taken [hypothesis].
- GetUIFloat returns -999 when the trait is not found [documented: GetUIFloat];
  SetUIFloatAlt, ModUIFloat return 1 on a change and 0 otherwise [documented].

## 6. Updates

The original's update order is not known. NVForever's: every trait with operators is
worked out on load; after that, a trait is worked out again when a trait it read has
changed since, until nothing changes (at most 32 passes; a loop of traits reading
each other is reported) [design]. A value a script or the engine sets holds until
something the trait's operators read changes [hypothesis: MCM and HUD mods set traits
that also have operators; observer check §8 item 3].

Vanilla, 2026-10-26: 3,529 tiles, 40,583 traits, 19,541 with operators, over the 49
menus; every menu settles [observed: fnvtool ui menus; loading all 49 takes 0.4 s].

### 6.1 Structural `childcount` in expressions

[observed: installed Vanilla UI Plus menu XML, 2026-11-17] Its StartMenu
verification reads `childcount` on a diagnostic container and expects four.
That container includes the same two-image fragment twice. The ordinary
list-item fragment adds traits, with no child tiles. MCM's injected fragment
separately declares its registration trait. Successful injection therefore
does not by itself satisfy the structural verification expression. No menu
XML or displayed warning text is included here.

[design] When an expression reads an absent `childcount` trait on a resolved
tile, read the number of direct child tiles. Includes, injected tiles and
instantiated templates contribute their actual children; template definitions,
traits and grandchildren do not. Existing explicit XML or code traits take
precedence, including zero. Keep `Tile::get` and trait-presence APIs unchanged:
the pause-menu column uses an absent explicit `childcount` as its initialization
sentinel. This fallback does not add a trait or implement structural
`GetUIFloat` queries.

[design] A per-tile value-generation stamp records actual child insertion and
removal. Expression reads depend on that stamp as well as the existing tree
shape invalidation, so an empty injected child updates its readers even when
no ordinary trait changed. Unrelated expression values written from outside
retain their existing lifetime policy. Count lookup takes constant time and
does not allocate.

[design] Independently authored CPU controls cover repeated includes and a
UIO-injected registration trait through the real load/update consumer, empty
children, nested children, template instantiation, removal, slot reuse and menu
reload, plus explicit zero/nonzero overrides and retained neighboring values.
These controls do not establish original-game parity or complete UIO/VUI
compatibility.

[design: CPU validation record, 2026-11-17] Normal locked release Cargo builds
of `fnv-ui` produced the baseline and candidate library-test executables. The
same three fixtures failed at their intended numeric assertions with the
accepted implementation and passed with this fallback. All 116 unchanged
baseline controls passed; all 119 candidate controls passed, with none ignored.
Candidate package-wide strict Clippy passed. Both compile controllers retained
a failure from their unchanged descendant-shutdown deadline despite Cargo
exiting zero. The emitted images were subsequently verified and executed in
fresh contained jobs, with exact source/index/mtime seals and unchanged image
hashes. These historical results qualify this package's CPU behavior. Hosted
whole-workspace and engine checks for each published source revision are recorded
separately in [PR #2858](https://github.com/LootJunkieAzza/NVForever/pull/2858);
they do not change the local controller results or transfer automatically to
later revisions. Native parity, rendered installed-menu and complete sentinel
acceptance remain unqualified; structural script getter queries remain outside
this change.

## 7. How mods change menus

[observed in the owner's mod folders, 2026-10-26, F:\Modlists\VNV\mods]

1. **Replacing a file**: a loose `menus\…` file takes the archive's place (Clean
   Vanilla HUD's `hud_main_menu.xml`, Vanilla UI Plus's menus, MCM's
   `menus\options\start_menu.xml`).
2. **An include the replaced file adds**: MCM's `start_menu.xml` includes
   `includes_StartMenu.xml`, which includes `MCM\MCM.xml`, so other mods can add to the
   same `includes_` file.
3. **UIO include lists.** [observed: the data files below, read 2026-10-29 from
   `E:\Games\Viva New Vegas\mods`; no DLL or source read] The lists pair an include
   line with a condition. Folder case varies (`UIO\Public`, `uio\public`, `Menus`).
   Short literal pairs from those files:

   ```text
   ; UIO - User Interface Organizer/uio/supported.txt
   wmm\wmm.xml:inventorymenu:glow_branch
   ?The Weapon Mod Menu.esp
   mcm\mcm.xml:startmenu
   ?The Mod Configuration Menu.esp	||	?Project Nevada - Core.esm
   jetpack\b3jetpack_hud.xml:hudmainmenu
   ?Jetpack.esp	&&	!darnui
   ; Just Assorted Mods/uio/public/JHM.txt
   JHM\JHM.xml::HUDMainMenu
   true
   ; MCM Extender/UIO/Public/MCMExtender.txt
   MCMExtender\ListRect\text.xml::@Prefabs\MCM\ListRect.xml::text
   true
   ; UI Improvements/uio/public/UII.txt
   UII\CharGenBox.xml::CharGenMenu::TM_DescriptionBox
   true
   ; Pip-Boy UI Tweaks/UIO/Public/PipBoyUITweaks.txt
   Stentorious\PipBoyUITweaks\_ySI\ButtonIcon.xml::@Prefabs\UII\PerkBrowser.xml::PerkBrowser
   #UII\PerkBrowser.xml && #..\ySI\ySI.xml
   ```

   [observed: those lists] Both `:` and `::` separate include, target and optional
   tile name. Targets are menu names in mixed case or `@...` paths relative to `menus\`
   (`@Prefabs\...`, `@globals.xml`, etc.) [documented: UIO Nexus mod-author guide,
   "Adding support for your mod"; no plugin DLL or source read]. `;`
   lines and blank lines separate groups. Conditions contain `true`, `?plugin`,
   `#path`, `!`, `&&`, and also `||` (the MCM alternative in supported.txt).
   [observed: Just Assorted Mods/Menus/Prefabs/JHM/JHM.xml;
   MCM Extender/Menus/Prefabs/MCMExtender/ListRect/text.xml;
   UI Improvements/menus/prefabs/UII/CharGenBox.xml] Include names resolve below
   `menus\prefabs` with `.` and `..` segments collapsed so sibling paths under `menus\`
   resolve identically in `<include>` and UIO lists [design]. `@` targets that are empty
   or escape `menus\` via `..` are rejected as malformed (`unknown prefab target`), and
   untargeted `@` patches on a menu file append inside its `<menu>` root while prefab
   fragments and `globals.xml` append to top-level nodes [design]. [observed: UI Improvements/menus/prefabs/UII/PerkBrowser.xml;
   Pip-Boy UI Tweaks/UIO/Public/PipBoyUITweaks.txt] The `#UII\PerkBrowser.xml`
   operand names an existing file there. The advrecon/primaryneedshud/filmgrain
   operands of supported.txt have no matching files in the inspected mod folders.
   [hypothesis] `#` uses that same prefab base, with `..` resolved before the VFS
   lookup; `#..\ySI\ySI.xml` is consistent with a sibling directory under menus,
   but that optional file is absent here.

   [observed: MCMExtender.txt and supported.txt; directory listing of the installed
   mods' uio/public files] `darnui` is used both positive and negated; no
   `uio\public\darnui.txt` was found. [observed: `PipBoyUITweaks.txt`, `ItemCards.txt`,
   `MCMExtender.txt`, and `supported.txt` in public UIO mod manifests] Public lists
   also use `darn` alongside `darnui` for DarNified UI and `vui+` for Vanilla UI Plus
   (for example `vui+`, `vui+ || darn`, `vui+ && #..\ySI\ySI.xml`, and
   `!vui+ && !darn && #..\ySI\ySI.xml`). [hypothesis] UIO's Nexus Mods 57174 mod-author
   guide does not publish the built-in condition detection rules, and UIO's DLL and
   source code were not consulted; NVForever evaluates `vui+` as true when
   `Vanilla UI Plus.esp` is active or `menus\prefabs\vui+\settings.xml` or
   `menus\prefabs\vui+\hudcenteredtext.xml` exists, and evaluates `darn`/`darnui` as
   true when `DarNifiedUINV.esp` is active or `menus\prefabs\duinvsettings.xml` or
   `menus\prefabs\darn\text_box.xml` exists (excluding standalone font packs such as
   `menus\prefabs\DarN_Fonts\*`). Literal `false` evaluates false without warning
   [design]. Any other unrecognized bare word remains `Condition::Unknown`: NVForever
   treats the atom as false and logs it even in a short-circuited branch. No
   filename-based alias is assumed.

   [observed: `AimCollisionIndicator.txt`, `Colorful Icons Support - ySI.txt`, and
   `ySIAssortedFixes.txt` in public UIO mod manifests] Some public lists ship 3-line
   entries pairing an include/target line and a condition line (such as `#JDC\JDC.xml`,
   `#JLM\JLM.xml`, or `!#VUI+\hudcenteredtext.xml`) with a trailing `true` line.
   [hypothesis] UIO's public Nexus Mods 57174 guide documents only the 2-line format
   and UIO's DLL and source code were not consulted; NVForever consumes at most one
   optional trailing `true` line after a valid 2-line entry while retaining line 2's
   condition. Extra trailing lines beyond a single `true`, standalone `true` lines not
   following a valid entry, and unobserved 3rd-line tokens (such as `false`) still
   emit `malformed include/target` warnings [design].

   [design] `fnv_ui::uio::{Lists, Condition, Report}` parses forgivingly, records
   malformed lines with list/line locations, and evaluates against a `Source` plus
   an active-plugin predicate. `Source::list(dir)` defaults empty; `MemSource`, the
   game's `UiFiles` and fnvtool's overlay source discover files recursively with
   case/slash-insensitive paths. supported.txt goes first, then immediate
   public/*.txt alphabetically. Identical normalized include/target/tile/condition
   entries occur once. Negation binds before AND before OR [hypothesis: precedence
   for mixed expressions is not established by these lists].

   [design] `Ui::configure_uio` runs before loading menus. Menu entries apply after
   the file's ordinary includes; prefab entries modify the fragment before expansion,
   including template bodies. Named targets use the first matching tile in file order
   [hypothesis: duplicate-name selection is unmeasured]. An untargeted entry appends
   to the menu root or prefab fragment; a named one appends within that tile
   [hypothesis: UIO's insertion position is unmeasured; appending preserves existing
   order and allows later included traits to override earlier ones]. Includes retain
   their own `Origin::File { path, loose }`. A loose UIO addition and its file
   descendants pass `mod_tile_drawn` without an archived name comparison, including
   name collisions; the archived baseline is built without UIO [design].

   [design] Reports distinguish unique applied entries from application counts
   (prefabs can be read repeatedly), and skip reasons condition false, file missing,
   target missing. Target missing also means not loaded yet in the current tree.
   The game uses `GameData::load_order` and logs decisions at startup and new
   applications during menu lifetime sync. `fnvtool ui menus|tree|mod-tiles` includes
   a `uio` report; it discovers top-level .esm/.esp names in Data and overlays for
   `?`, or uses an explicit repeatable `--plugin` set. Overlays count for `#`.
4. **Scripts at run time**: JIP's `InjectUIXML "path" "data\menus\…xml"` and
   `InjectUIComponent "path" "<xml with %g>"` put XML into a loaded menu
   [documented: InjectUIXML, InjectUIComponent]. Most menus are loaded only while open
   and must be injected again each time; `HUDMainMenu`, `InventoryMenu`, `MapMenu`,
   `RepairMenu`, `StatsMenu` and `ItemModMenu` stay loaded [documented].
5. **Trait functions**: SetUIFloat, SetUIString and friends on paths (§5).

- [design] `Tile::origin` distinguishes `File { path, loose }`, `Injected`, `Template`
  and `Code`. Menu loading and ordinary includes record the exact path passed to
  `Source::read`, with each included file's own provenance. Injection and template
  creation stamp every tile they create, including included descendants, with their
  creator's origin; later trait writes do not change it. Pseudo-tiles start as Code;
  loading globals records its file origin.
- [documented: crates/fnv-vfs/src/lib.rs, Vfs::source_of] The VFS exposes whether the
  winning source is loose. [design] `Source::is_loose(&self, path: &str) -> bool`
  defaults to false; `UiFiles` implements it using that VFS query. The result is stored
  at load time, so the source need not remain available to classify tiles later.
- [design] `Ui::mod_added_tiles(&self, menu: TileId) -> Vec<TileId>` lists top-most
  live Injected, Template or loose File tiles in tree order, including the menu itself
  when loose. Descendants of a returned root are not listed separately; unloaded tiles
  and other menus are excluded. Loose-file replacement selects the whole replaced
  subtree; it is not a comparison against the vanilla file's contents.
- [design] `Ui::add_code_tile_from_template(&mut self, src: &dyn Source, at: &str,
  template: &str, name: Option<&str>) -> Option<TileId>` makes all copies from the body
  Code, including multiple top-level tiles, and returns the first as `add_from_template`
  does. Code tiles are not mod roots; injections underneath them can be. A future
  renderer drawing selected subtrees must still exclude Code items inside those roots.

## 8. Observer checks

What the original can settle (copied to docs/handoff/observer-checks.md):

1. **The numbers of `&true;` and `&false;`.** Answered 2026-10-28: 1 and 0, and
   `&does_not_stack;` 6008 [observed: hud.md §14]. `GetUIFloat` on a trait the file sets to
   `&true;` and nothing changes (e.g. `HUDMainMenu/locus`), and on one set to `&false;`
   (`HUDMainMenu/_CrosshairVisible` before any aiming). Also `&center;`, `&right;` (a
   `justify`) and `&scale;` while there.
2. **How the original reads mismatched end tags.** SUP's `DumpTileInfo`
   [documented: DumpTileInfo, origin SUP] (or `GetUIFloat` with `:N` paths) on
   `QuestAddedMenu`'s two buttons and on MCM's `StartMenu/MCM` tree: which tile ends up
   holding what.
3. **Whether a set trait holds.** `SetUIFloat` on a trait that has operators (a HUD
   piece's `x`), then change what it reads (a screen size change will do), and read it
   again.
4. **Whether SetUIFloat makes a missing trait.** `SetUIFloat "HUDMainMenu/_NewTrait" 5`
   then `GetUIFloat` on it.
5. **What the deprecated forms' "children tiles bug" is.** The wiki says SetUIFloat,
   GetUIFloat and SetUIStringEx are "bugged for templated children tiles" and MCM
   depends on the bug, but its "Bug: Children Tiles" page is not in the export. Compare
   SetUIFloat and SetUIFloatAlt on a list item made from a template (`BarterMenu` rows).
6. **`name:N` / `*:N` counting: answered 2026-10-29.** Last matching child first,
   zero-based [observed: private jobs hud_hardcore hc0 (LMBs *:0), hud_xp_limbs all
   (Right_Leg/XPLevelUp *:0), hud_prompt2 p0-p8 (VAL left:0, WG left:1, hotrect *:8);
   hud.md §14.8]. These reads used JIP GetUIFloatAlt; deprecated xNVSE GetUIFloat
   parity remains untested [hypothesis]. No contrary vanilla menu/script dependency
   was established by these captures; other menus/scripts still need checking.
7. **Which target takes the pointer where two overlap.** Ours: the highest summed `depth`,
   a tie to the one drawn later; a later menu over an earlier one (§3.2). In a menu with
   overlapping buttons (the level-up menu's list over its buttons, or two tiles injected
   with InjectUIComponent at the same place and different depths), GetActiveUIComponentName
   with the cursor over the overlap.
8. **How long `clicked` holds.** Ours: 1 for the frame of the press, then 0 (the
   tutorial), not a count (the Clicked page). A tile injected with
   `<_n> <copy src="me()" trait="_n"/> <add src="me()" trait="clicked"/> </_n>` and one
   with `<_c> <copy src="me()" trait="clicked"/> </_c>`, clicked twice; GetUIFloat on
   both after each click (a count: `_c` 1 then 2; a frame: `_c` back to 0, `_n` 1 then 2),
   and whether it is the press or the release that sets it.
9. **Whether `mouseover` reaches the tiles above.** With the cursor on a text_box button
   (its text child), GetUIFloat on the hotrect's, the text's and the parent rect's
   `mouseover` (ours: the hotrect 1, the others 0 or absent).
10. **The pointer functions with no menu, and on a tile without an id.**
    GetActiveUIComponentName, GetActiveUIComponentFullName and GetActiveUIComponentID in
    game mode with no menu open (ours: "", "" and -1), and over a target with no `<id>`
    (ours: its name and -1); ClickMenuTile's return value.
11. **Where MCM's shorthands point** (#976). With MCM installed and a mod's page open,
    GetUIFloat on `StartMenu/MCM/*:1/*:1/_optionID` and on
    `StartMenu/MCM/MCM_Options/Option1/_optionID`, and `GetMCMFloat 1 1 "_optionID"`,
    `GetMCMFloat 1 0 "_optionID"` with the pointer on the first row (ours: the
    shorthands count from 1 in file order, 0 stopping, §10; `*:N` from the last child).

## 9. The rest of the tile functions (task T9)

[design] Built headless in `crates/fnv-game`: `ui_more` (dispatch, after
`ui_functions` in `nvse::call`), `ui_gradients` (changes over time), `ui_cache`
(CacheUI), `ui_hudbar` (SUP's bars); tests `crates/fnv-game/tests/ui_more.rs`, the
units in those files, and `fnv_ui` `a_trait_by_a_path_under_a_tile`
(`Ui::find_trait_in`). Every page is the GECK wiki export of 2026-10-28; the tags per
rule are in the code beside it. Smoke: `ui-gradual-in-game`, `ui-string-dimensions`,
`ui-cursor-inspection`, `ui-typewriter-tile-script`.

| Functions | Page(s) | What NVForever does | Open |
|---|---|---|---|
| SetUIFloatGradual | SetUIFloatGradual [documented] | modes 0-3 on real time; stop with the path, stop and set with a start; the engine steps them after the scripts, before `Ui::update` (`engine/nvf-play/src/tiles/mod.rs`) | linear in time [hypothesis]; a missing duration ends at once [hypothesis] |
| GetUIFloatGradualActiveForTile, …ForTileTrait, …Mode, …Timer | the four SUP pages [documented] | read JIP's table; Mode -1 when none [documented] | Timer -1 when none, a repeating change's time left is its round's [hypothesis] |
| SetUIStringReveal, CacheUIStringReveal(Alt) | SetUIStringReveal, CacheUIStringReveal [documented] | letters by the time's share, from the left or right, inverted, ease in as the square [hypothesis for the rounding, the right and the ease] | none |
| SetUIFloatMultiple | SetUIFloatMultiple [documented] | every tile inside of the type, that has the trait; the count back [design] | whether it makes the trait [hypothesis: not] |
| AttachUIXML, AttachUIComponent | AttachUIXML, AttachUIComponent [documented] | a single `rect` into `HUDMainMenu`'s root, kept in `UiExtra::attached` with the reference and node; 1 or 0; the VM formats the component. Replaces the one attached before to the same (reference, node, rect name), keeps it hidden until placed, drops it when the reference is deleted, disabled or outside the loaded cells [design]; placed over the node on screen each frame, hidden off screen or without the node (`ui_more::place_attachments`, `nvforever` `ui_attach.rs`, #1835) [design] | the rules on screen are our own (below) |
| AddTraitToTile | AddTraitToTile [documented] | makes or sets the trait; an empty name makes none (0) [design] | the original's list of trait ids it allows is not known: any other name taken [hypothesis] |
| GetUIValueType, GetUIElementType | the SUP pages [documented] | 1/2/3 and -1; 1-5 by tile kind; GetUIElementType uses the live cursor tile for a missing or nonexistent path, -1 with none [design: `ui_more::inspection_tile`] | `nif` 4, other kinds 0 [hypothesis] |
| SortUIListBox | SortUIListBox [documented] | keys `-`, `#`, `@`; `$destination$increment` (default `_y`, `height`); numeric keys in a total order, NaN keys at the ends (`total_cmp`) [design] | which children are items (those with the destination), the first value (the smallest), the case [hypothesis each] |
| GetStringUIDimensions | GetStringUIDimensions [documented] | width, height and actual `Font::lines` count through the VM's output variables; owned metrics installed before startup/load scripts (`tiles::MenuFiles`, `script.rs`), using the renderer's localized `font_file`, font selection and wrapping [design] | unavailable metrics write three zeros; failed reads retry at most once a second on query, successful metrics are cached without GPU atlas uploads [design: NVForever policy, not documented original behavior] |
| DumpTileInfo, DumpTileInfoALL, DumpTileInfoToArray | the three SUP pages [documented] | `[ Tilename ] : name` and `[ trait ] : value` lines (the ToArray page's example output) to the console, or to NVForever's log for DumpToFile (never `SUPNVSE.log` in the game folder); ToArray a string map; DumpTileInfo uses the live cursor tile for a missing or nonexistent path (path argument 1); ALL lists menus; ToArray still requires a path [design: `ui_more`] | none |
| CacheUI, …Float, …String, …ModFloat, …GetFloat, …GetString, …Range, …RangeMod, …Erase, …Dump, …Gradual, …RangeGradual, …GradualDelay, …RangeGradualDelay, each with its Alt | the CacheUI pages (CacheUIModFloat's is `CacheUIMod`) [documented] | caches per calling plugin, Alt per key; delay pulls the value then, flags 0/1/2 and the lock, `match` ends a delay; modes 0-3, 5, 6 | mode 4 (rumble) mentioned but insufficiently specified: waveform, frequency, randomness, decay curve and final value are not given; remains refused (`ui_gradients::mode_built`); RangeMod's page and example differ in argument order, both taken [design]; the script runners' shared cache |
| CUICreateQueue, CUIGetQueue, CUIGetQueueSize, CUIQueueCycle, CUIQueueFloat | the five pages [documented] | per plugin; toggle, forced add and remove, duplicates | cycle direction 0 [hypothesis] |
| HudBar… (35 functions) | SUP HUD Bar Functions and each HudBar page [documented] | a `rect NVFHudBar<n>` with `Meter`, `Frame`, `Text`, `ImageEx` in `HUDMainMenu`, per plugin (console: first of the name), written after each change and after a load; types 0-5, value/max/percentage, text with prefix, postfix and max, alpha, colour, system colour, depth, zoom, rotation, size, indent, alt size, texture, text font and justify, auto position, bar and meter traits, all-bars arrays, element paths, debug lines to the log | bound values (ext-sup7.md §3), the visibility conditions 0 and 21-27 with their fades 41-44/61-64 and HudBarGetVisible's -1: hud-bars.md (#1834); reading a texture's size (`iCalculateSize`), the rotation on screen (the traits are written, the tile renderer turns no tile yet); the meter's width as indent plus the share of the rest, type 4 without a built-in `%`, the default max 100 and which elements show by default [hypothesis] |

[design] Numbers that are not numbers: SetUIFloatMultiple, AddTraitToTile, CacheUIFloat /
ModFloat / Range / RangeMod (and their Alt forms), a HUD bar's place at creation and its
element, bar, meter and text setters write 0 for a NaN or an infinity (`ui_more::finite`;
a CacheUI sum that is one too); HudBarSetValueFloat, …Max and …Percentage refuse one with
-1. The pages say nothing about such numbers; these are NVForever's own safeguards.

[design] `ui_more::inspection_tile` resolves GetUIElementType and DumpTileInfo before
flushing deferred HUD writes. The resolved tile's menu controls the flush, including a
cursor fallback from an invalid path; another menu or no live tile leaves writes pending.
DumpTileInfo retains `toFile path`, with the path at argument 1. `Pointer::active`
(`fnv-ui/src/input.rs`) rejects unloaded tiles.

Implemented consumers and remaining exclusions:

- **SetUITypewriter** [documented: [SetUITypewriter](https://geckwiki.com/index.php?title=SetUITypewriter)]
  requires authored XML linking `_TSn`/`_TAn` into text/alpha. [design: `nvse_small.rs`,
  `Game::small_tick`; `game.rs`, `Game::tick`] Letters and fades advance each frame;
  ordinary tile evaluation and drawing consume linked text/alpha (`tiles/mod.rs`,
  `tiles/draw.rs`). The command generates no XML. Ordering modes retain the hypotheses
  beside `typewriter_rank` and `typewriter_tick`.
- **SetTileScript, RemoveTileScript** [documented: [SetTileScript](https://geckwiki.com/index.php?title=SetTileScript)]
  register/replace/remove a callback per trait. [design: `nvse_small.rs`, `small_tick`]
  The callback runs each frame while the trait is nonzero and can change drawn traits;
  registration itself draws nothing. Menu mouseover/click events also have consumers
  (`ui_input.rs`, `step`), rather than a blanket menu-events exclusion.
- **SetUITexOffset, SetUIAlphaMap** [design: `nvse_itr1.rs`, dispatch; `tiles/texmod.rs`,
  #1833] store offsets (wrapped into 0-1) and alpha-map paths by tile id, so a tile
  unloaded or replaced at its path drops them (`Game::itr_drop_gone_tile_textures`, each
  frame). The tile renderer bakes a picture with either into one image in its place: its
  texels (atlas piece or own file) sampled shifted by the offset in whole texels, wrapping
  inside the piece, nearest texels [design]; the alpha multiplied by the alpha of the
  mask, an "alpha mask texture" bound beside the unchanged base texture and sampled at
  "the same UVs as the base texture", clamped to its edge texels (`CLAMP_S_CLAMP_T`)
  [documented: GECK wiki SetUIAlphaMap]. The offset does not move the mask
  [documented: SetUIAlphaMap notes; the mask not scrolling is read from its advice to
  scroll a strip's base texture with SetUITexOffset on a normal-sized quad under a
  mask, which only works if the scroll leaves the mask's stage alone]. So a picture
  whose coordinates run 0-1 (stretched, natural, zoomed and cut to the box, clipped)
  has the mask over it once; a `tile` picture, whose coordinates run past 1 with its
  repeats [documented: the same note], has it over its first repeat from the box's top
  left and the mask's edge texels over the rest; an atlas piece, whose coordinates are
  its part of the atlas (the index gives them as parts of the atlas, corner-messages.md
  §2.2), has the mask's same part over it [hypothesis: "same UVs" read for an atlas
  piece]. NVForever reads no `cropoffsetX`, so no tile's coordinates start elsewhere.
  The image has the picture's texels, more where the mask is more detailed, up to a
  texel an interface unit of the drawn size (a 1 x 1 picture over a panel keeps the
  mask's detail). A masked `tile` box too large to bake at its repeats' texels is baked
  at fewer, at most `repeat::MAX_TEXELS`, so the mask still lies once (its picture
  coarser); without a mask such a box keeps Bevy's tiling of the shifted piece [design].
  Radial tiles stay undrawn. The compass strip's texels are moved on by the offset
  before the heading scrolls them (the two add up) and its window is baked under the
  mask stretched over it (`compass_strip::StripWindow::masked`) [design]: the documented
  rule is not applied there, as NVForever's code, not a trait, scrolls the vanilla strip
  (hud.md §14.4) and the coordinates of the original's window (which part of the strip,
  scrolled how) are not known [hypothesis]. Textures are decoded
  only up to `repeat::MAX_TEXELS`, read from the file's header first, by these bakes and
  by `crate::repeat`'s (the tiled pictures, the compass strip), whose oversized textures
  keep Bevy's drawing; when a source texture exceeds `repeat::MAX_TEXELS` (such as a
  4096 x 4096 modded UI atlas), `crate::repeat` and `compass_strip` decode only the
  requested sub-rectangle directly from level 0 when that region fits within
  `repeat::MAX_TEXELS` [design]. An offset that changes alone re-bakes the kept image, or moves the
  compass strip on, in place (same nodes and handle), only when it is another texel
  shift, within `texmod::BAKE_BUDGET` texels a frame (unspent budget accrues up to three
  largest images, the most one bake costs: its output and both textures decoded again, or
  for the compass strip its strip, its window and its source; the piece waiting longest is served first, so scrolling pieces take turns; a
  texture to decode again is part of the cost checked against that reservation, and a
  texture still cached is taken first so decoding the other cannot evict it).
  Decoded textures are kept within 64 MiB a drawing, every decoded texel counted, at each
  insertion and each frame's prune: those no kept bake names go first, then the least
  recently used, a kept bake decoding its texture again when it next needs it. Texels held
  outside the cache (a strip window's mask, a bake under way) are never let go, so the
  cache can pass 64 MiB only while they are held. Those nothing names any more are
  released at once [design: #1833 review]. Which way the offset moves (the texel at the
  coordinate plus the offset) is [hypothesis: no page gives it]. The mask's path is
  under `Data\Textures\` [documented: SetUIAlphaMap, "relative to Data\Textures\"]; a
  path already starting `textures\` is kept as one [decision]
  (`nvse_itr1::alpha_map_path`). SetUIAlphaMap returns 0 when the tile is not found or
  the texture fails to load [documented]; the script side takes a texture not in the
  data (loose or in an archive) as the failure [hypothesis], and while the host gives no
  answer for the archives takes it as there [decision]. A texture there that cannot be
  read (too large, not decoded) leaves the tile without it, said once (smoke
  `ui-texoffset-alphamap`, `ui-texoffset-compass`).
- **Attached components** [documented: [AttachUIComponent](https://geckwiki.com/index.php?title=AttachUIComponent)]
  require a loaded, case-sensitive model node and one parent rect under HUDMainMenu.
  [design: `ui_more.rs`, `attach`; `tiles/mod.rs`, update] Attachments remain hidden until
  placed. Occlusion, clipping, distance scaling and offscreen behavior are not settled by
  that page. [design: `ui_more.rs`, `place_attachments`; `engine/nvforever/src/ui_attach.rs`,
  #1835] Each frame, after the animated pose and the camera are worked out, the node (by
  its exact name: an actor's skeleton node, or a node of the reference's own animated
  model, `nvf_play::RefNodes`; statics' baked shapes and worn or held models keep none)
  goes through the player's camera to the HUD's interface units (960 to the window's
  height divided by NVForever's UI scale, as the HUD is drawn; the view taken to fill the
  window), and the rect's `x`/`y` (its top left) are put on that point. When it is
  attached its `x`/`y` lose any expression and stay plain numbers (an authored constant as
  written, else 0) that scripts can read and write until the next placement; the
  component centres itself through its children. `visible` is written 1 when it comes on screen and 0 when
  it leaves. Hidden: behind the camera, nearer than the near plane or farther than the
  camera's far distance (checked on the camera-space depth: Bevy's perspective matrix
  has no far plane), the point outside the view's sides, no such node, no drawn model
  that names nodes or that model hidden, no active camera. A rect whose node is on screen
  draws whole, cut only by the window's edge; there is no occlusion test and no scaling
  with distance. The drawing shows the new place a frame behind the model.
- **HudBar live value bindings and conditional visibility**: built (#1834), choices and
  tags in docs/spec-notes/hud-bars.md.
- **CacheUI mode 4** [documented: [CacheUIRangeGradual](https://geckwiki.com/index.php?title=CacheUIRangeGradual)]
  mentions bounded, decaying rumble but is insufficiently specified: no waveform,
  frequency, randomness, decay curve or final value. [design: `ui_gradients.rs`,
  `mode_built`] It remains unimplemented; no waveform is invented.

## 10. MCM: its places and its menu scripts (tasks T7, T11; issue #976)

Described from MCM's data files (its `menus\prefabs\MCM\*.xml` and
`includes_StartMenu.xml`, version 1.5 as installed in the owner's Data folder) and one
mod's script source (FOV Slider 2.6's `FOVSliderConfigScript`); no extender or DLL code
read.

- **Where MCM lives** [observed: the files]: MCM's `start_menu.xml` replaces the
  vanilla one and includes `includes_StartMenu.xml`, which includes `MCM\MCM.xml`
  (§7 item 2). That file is one `rect` named `MCM`, hidden (`visible` false), with the
  traits the mods and MCM's own script talk through: `_ActiveMod`, `_ActiveMenu`,
  `_ActiveOption`, `_ActiveSubMenu`, `_Value`, `_NewValue`, `_Reset`, `_Default`,
  `_DefaultScale`, `_ShowScale`, `_ShowList`, `_ShowMultiScale`, `_Value1`-`_Value3` with
  their `Min`, `Max`, `Increment` and `Decimal` traits, `_MCM` (1.5, its version) and
  others. Its children, in file order: `MCM_Options`, `MCM_Scale`, `MCM_List` (from the
  includes it starts with), then `MCM_Trigger`, `MCM_ModList`, the background, brackets,
  titles and buttons, `MCM_Version` last (19 in all).
- **The rows** [observed: the files]: `MCM_Options` begins with `Option1`-`Option36`
  (hotrects, each with `_optionID` 1-36, `_title`, `_enable` and the rest from
  `OptionRect.xml`), then its arrows and scroll bar; its own `_optionID` adds up its
  rows' `_optionIDX`, which is a row's `_optionID` while the pointer is on it, so it is
  the id of the row under the pointer. `MCM_List` begins with `Item1`, `Item2`, ….
- **The shorthands' places** [hypothesis, from the above]: `GetMCMFloat child grandchild
  "setting"` and its setters count `child` and `grandchild` from 1 in file order under
  the `MCM` rect, and 0 stops at the level above: `0 0` is the `MCM` rect itself, `1 0`
  `MCM_Options`, `1 3` `Option3`. FOV Slider reads `GetMCMFloat 0 0 "_ActiveMod"` (a
  trait only the `MCM` rect has) and `GetMCMFloat 1 0 "_optionID"` (the row under the
  pointer) and sets `SetMCMFloat 0 0 "_NewValue" 0`. The GECK wiki writes `SetMCMFloat
  1 1 "_enable" 1` as `SetUIFloat "StartMenu/MCM/*:1/*:1/_enable" 1` [documented:
  SetMCMFloat]; the extender's GetUIFloat and SetUIFloat count `*:N` from the first child
  (§5, #2298), JIP's from the last; the shorthands find their tile by place directly
  (`fnv_game::ui_functions::mcm_trait`). A trait or place that is not there reads 0; the
  setters make nothing (as SetUIFloat, §5) [hypothesis].
- **A mod's menu script** [observed: FOV Slider's source]: a quest script with a
  `Begin MenuMode 1013` block (the start menu, `fnv_game::game::menus::START`). It
  returns unless `IsModLoaded "The Mod Configuration Menu.esp"`, and acts only while
  `GetMCMFloat 0 0 "_ActiveMod" == GetSelfModIndex`, i.e. while MCM shows this mod's page.
  Then, one at a time: `_Reset` 1 means fill in the page again; `_NewValue` 1 means a
  value was entered (`_Value`, for the option `_ActiveOption`), which the script takes,
  then sets `_NewValue` 0 and `_Reset` 1; `_ShowScale` 1 asks it to fill in the scale
  (it sets 2); `_DefaultScale` 1 asks for the default. At `GetGameRestarted` the mod adds
  a form to MCM's list (`ListAddForm` on MCM's form `ae6` from `GetFormFromMod`) and
  checks `_MCM`'s global (`b0d`) for version 1.5 or later.
- **GetSelfModIndex** is "the mod index of the script that calls the function"
  [documented: GetSelfModIndex]: the calling unit's plugin's place in the load order;
  0 for a script typed at the console or run as a query [observed: plugins1, 0]. MCM
  mods depend on it being their own index.
- **Built (#976, T7)**: the shorthands as above; GetSelfModIndex; a headless test
  (`crates/fnv-game/tests/ui_mcm.rs`) that builds `StartMenu` through MCM's include
  chain while the start menu is open and runs a compiled `MenuMode 1013` quest script
  written in FOV Slider's way, checking the traits it sets. The files and the script are
  written for the test; no mod data is used.

### 10.1 The pause column MCM waits for (observer check 73, 2026-11-12)

Measured in the original with nvf_capture (job `mcm976*`, save `nvftest`, load order with
MCM 1.5 and Just Assorted Mods active, window 1280 x 720), with the pause menu open
(`GetActiveMenuMode` 1013, `MenuMode 1013` true) [observed 2026-11-12]:

- `StartMenu/NOGLOW_BRANCH/main_container`: `childcount` 8, `x` 1301.67, `y` 336,
  `width` 375, `height` 288, `_alpha` 255, `_enabled` 1. `StartMenu/_isPauseMenu` reads 0
  (so it is not the pause menu's mark; MCM tests it only against DarN's start menu).
  `StartMenu/MCM/visible` 1, `StartMenu/MCM/_MCM` 1.5.
- Of these the file sets `x`, `y` (from `parent()`'s size), `width` (375) and `height`
  (`_number_of_visible_items` times `_item_height` 48: 288 is 6 rows); the code writes
  `childcount`, `_alpha` and `_enabled` (the file leaves `_alpha` empty and `_enabled`
  false) [observed: start_menu.xml; the values above].
- MCM's `MenuMode 1013` script shows its trigger only while that `childcount` is 8 and
  places it from `main_container`'s `x`, `y`, `width`, `_alpha` and `_enabled` and
  `StartMenu/_item_height` [observed: MCM's script source in its plugin (SCTX)].
- **What the player sees** [observed: the owner's screenshots of the original, 2026-11-12,
  1280 x 720]: "Mod Configuration" stands as the first row of the pause column, above
  Continue, Save, Load, Settings, Help, Quit. Chosen, it opens MCM's window left of the
  column (the column stays, dimmed, with "Mod Configuration" lit): the mod list on the
  left (one row per mod, Just Assorted Mods as "Just Mods"; a mod with sub-menus shows them
  indented under it once chosen), the chosen page's title in the top bracket, its
  options in two columns of label and value (check boxes, words, numbers), arrows at the
  foot of each list, an info box under the window for the option under the pointer, and
  Defaults and Back at the bottom right of the screen. With no mod chosen the window
  shows MCM's logo, its version (v1.50) at the top right and "Select a mod to configure
  from the list on the left." in the info box (MCM's own text, from its files).
- **The column's rows** [observed, the same run]: `main_container`'s children are the
  file's `lb_highlight_box` and `lb_scrollbar` and six copies of the file's
  `lb_item_template` (`lb_item_hotrect`, `string` Continue, Save, Load, Settings, Help and
  Quit in the original's words, `listindex` 0-5, `y` 0 to 240 in steps of 48, `height` 48,
  `width` 370, `target` 1, `_enabled` 1): 8 children. `GetUIFloat` with `*:N` under it
  read `*:1` the highlight box, `*:2` the scroll bar, `*:3`-`*:7` the rows Continue to
  Help, `*:0` the last row (Quit) and `*:8` nothing; `lb_item_hotrect` read Quit and
  `lb_item_hotrect:1` Continue.
- **MCM's trigger in the original** [observed]: once placed, `MCM_Trigger` has `x` 1301.67
  (the column's), `y` 288 (the column's less `_item_height` 48), `width` 370, `height` 48,
  `_alpha` 255, `_enabled` 1, and its text "Mod Configuration". `SetUIFloat` of its
  `_clicked` to 1 opened MCM's window (MCM's `_Menu` 1, `_Reset` 1, the mod list's
  `_visible` 1 and `_count` 2, Mod1 "Just Mods" with `_modIndex` 12, Just Assorted Mods'
  load index).
- **MCM's quests in the original** [observed 2026-11-12, jobs mcm976q, mcm976r, mcm976s,
  save nvftest]: right after the load MCM's quest runs and its mod list (`MCMMods`)
  already holds Just Assorted Mods' item (count 1), so the mod added it before the save
  loaded, under the main menu (its `MenuMode 1013` block adds it after half a second).
  `MCMModList` does not run in play nor under the pause menu; two frames after MCM's
  trigger was clicked it runs, the list's `_count` is 2 and Mod1 reads "Just Mods"
  (MCM's stage 0 starts it). ResetQuest from a job script stopped a running `MCMList`
  and left the stopped `MCMList` and `MCMModList` stopped (as o41, layer5-captures.md
  §5.7).
- **NVForever** (`nvf-play` `tiles::mods`, `script`) [design unless marked]:
  - when the Mods page opens it writes the column's code traits (`childcount` 8,
    `_alpha` 255, `_enabled` 1), StartMenu's `alpha` 255 (its fade-in done, as the VATS
    menu's) and one code `lb_item_hotrect` row (MCM reads only its `target`), then holds
    MCM's trigger clicked until MCM's window opens and lets go in that frame (a click
    held into the next frame closes MCM again, by MCM's script); MCM's window closing
    (`_Menu` 0) closes the page;
  - when a game starts, the start menu's `MenuMode 1013` blocks run for a second
    (`Game::came_through_start_menu`), standing for the main menu the player came
    through, where MCM mods add themselves [hypothesis from the observations above]; no
    shipped script has such a block;
  - GetSecondsPassed in a block run under the page counts the page's own clock
    (`Game::page_time`), so a mod's MenuMode timer runs there [hypothesis: Just Assorted
    Mods' does in the original];
  - the page lays MCM's tiles out by the `locus` rule (a tile's `x`, `y` count from the
    nearest tile above with `locus` on) [documented: GetUIFloatInherited], measures the
    start menu's texts as the code does (`MCM_ModList_Size`, 31 high; "Just Mods" 117
    wide, both as in the original), and writes `screen()` in interface units while it is
    up (MCM centres its window by it: `x` 353.33 on a 16:9 view in the original);
  - MCM's trigger is not drawn (NVForever's column and its Mods entry stand for that
    row) and the pause menu's own Back is not drawn while MCM's window, which has its
    own Back and Defaults, is open.
  - Open: the list's `_count` read 1 here where the original's read 2 with the same
    mod; the main menu's column.

### 10.2 MCM.dll's list functions (2026-11-12)

No page or source describes them; their call shapes are MCM 1.5's own scripts (its
plugin's source text), their effects measured in the original with job `mcm976e`
(pause menu open, MCM's window open), each call read before and after in one script
[observed 2026-11-12]. Child numbers count MCM's children from 1 in file order (§10):
1 `MCM_Options`, 3 `MCM_List`, 5 `MCM_ModList`.

- **SetMCMFloatMass child first last setting value**: sets `setting` on child `child`'s
  children `first` to `last`, both included, counted from 1. `3 1 2 "alpha" 77`: Item1
  and Item2 0 to 77, Item3 and MCM_List unchanged. `3 0 1 "alpha" 66`: Item1 only.
  `3 3 2 "alpha" 55`: nothing. `1 1 2 "image/depth" 21`: each row's `image` child (a
  path below the row works). A trait that is not there is not made (`"_nvfm"` stayed
  missing). MCM's own `5 24 33 "_enable" 0` reaches SubMenu1-SubMenu10, MCM_ModList's 24th
  to 33rd children (after Mod1-Mod23). One reading differs: `1 36 37 "depth" 33` set
  Option36 but not ArrowUp, MCM_Options' 37th child in the file; why is open (observer
  check 118).
- **SetMCMStringMass child first last setting "format" values…**: the same for text, the
  format filled as SetMCMString's: `3 1 2 "text/string" "nvf"` set Item1 and Item2, not
  Item3; `"a%gb" 5` gave "a5b"; a missing trait was not made.
- **SetMCMModList row enable modIndex value form**: MCM_ModList's row `row` (Mod`row`)
  gets the form's name as its `text/string`, `enable` as `_enable`, `modIndex` as
  `_modIndex` and `value` as `_modIndex2`; nothing else of the row changed.
  `22 1 6 5 caps`: "Bottle Cap", 1, 6, 5; `22 2 7 9`: 2, 7, 9 (still Mod22); `21 0 0 0`:
  the name and zeros. `3 2 7 9` left SubMenu3 as it was. MCM calls it with the row, 1,
  the mod's load index and its form's value.
- **GetMCMListWidth list row**: the `width` of a mod-list row's text: list 1 Mod`row`
  (`1 1` 117, Mod1's "Just Mods"; 14 for an empty row), list 2 SubMenu`row` (`2 1` 14,
  empty); `1 0` and `3 1` give 0. Read in the frame a string was set, it gave the width
  before (the text's own `width` too, so it reads that trait, re-measured later).
- Built in `fnv_game::ui_functions` (names in `schemas/script/functions.json` from
  `tools/capture/script_functions.py`'s `PAGELESS`); the setting of every MCM shorthand
  may be a path below the tile (`"ArrowUp/y"`, `"/visible"`, as MCM's scripts pass).

### 10.3 What MCM needs of a click and of the frames before the game (2026-11-12)

Found in the owner's runs with MCM 1.5, CASM with MCM, B42 Quickthrow and Project Nevada
(the mod log, `nvf_play::mod_log`):

- **A click on a list row is read in two frames running.** MCM's main script sees the
  list's `_index` (a row's `_modIndex` while its `_clicked` is 1; `_clicked` is the row's
  `clicked` less MCM's `_UnClick`), leaves the frame ("Skip frame to let UI set
  variables"), and its list script reads the clicked row's `_modID` in the next frame,
  then sets `_UnClick` for one frame [observed: MCM's script source and menu files]. With
  `clicked` at 1 for one frame (§3.2) the second read found nothing: the pointer's clicks
  landed on the rows and nothing was selected [observed: the run's log, `clicked 1` on
  Mod1 and Mod2, `_selected` left at -1]. So under the pause menu's Mods page a
  pointer's click holds `clicked` for two steps (`fnv_ui::input::Pointer::click_steps`)
  [hypothesis: the original's `clicked` reads 1 in exactly two MenuMode frames; one frame
  more would pass MCM's one-frame `_UnClick` mask and click twice]. Elsewhere, and for a
  script's click, it stays one step. Observer check §8 item 8 settles it.
- **The start menu's frames before the game run with the start menu's tiles.** MCM's
  install check (`iInstallCheck`, set by its GameMode block, so 1 in any save) reads
  `StartMenu/MCM/_MCM` on its first MenuMode 1013 frame and shows "cannot access its menu
  files" without it. A run that begins in a save (a settings change restarts the game
  into it) has no main menu, and a new game's start menu is dropped before those frames,
  so the tree had no StartMenu and the box came up [observed: the owner's run]. The
  frames now build it first (`nvf_play::script`, `ui_loading::sync` as under the main
  menu); the frame's own sync drops it again.
- **`src="me"` without brackets is the tile itself.** MCM_Options places its two columns
  by `<_x1> <copy src="me" trait="x"/> <add> 20 </add> </_x1>` (and `_x2`); read as a
  tile's name it gave 0, and every option row stood at the screen's left instead of in
  the right pane [observed: MCM 1.5's Options.xml; the owner's run, CASM's options about
  500 units left of the pane]. A tile that bears the name is still the one meant
  (`fnv_ui::Ui::resolve`).
- **The pointer is matched by the rule the page is drawn with.** The Mods page is laid
  out by the `locus` rule (§10.1, `layout_by_locus`), the pointer's tiles were placed by
  adding every parent's `x` and `y`; MCM's window rect has an `x` and no `locus`, so the
  rows were looked for that much to the right of where they were drawn [observed: the
  owner's run, rows highlighting beside the pointer]. The page's pointer now takes
  `fnv_ui::input::hit_candidates_by_locus`.
- **A mod that fills MCM's list without being selected** shows its sub menus over the
  mod names. JIP Companions Command & Control's MCM script compares `_ActiveMod` with a
  global, `iJIPCCCModIndex`, that no script sets; JIP LN sets it (and `iJIPCCCStartup`)
  from inside [hypothesis: nothing in the plugin writes either]. Left at 0 it equals "no
  mod selected". Not built: it needs those values and the mod's `CCC…` functions read in
  the original.

### 10.4 The MCM author's guide, item by item (#2298)

The guide's contract for a mod's page (its traits, option types 0-9, lists, sliders,
sub-menus, columns, colours) is checked against NVForever in mcm.md, with the faults the
Blue Moon list showed: `*:N` counted from the last child, `_Has360Controller` never
written (MCM took the pointer for a controller), bracket-less sources read as names
(Vanilla UI Plus's option titles all at y 0), and JIP CCC's sub-menu lines with no mod
chosen.

## 11. Modern Minimap's map picture (ModernMinimap.dll)

[observed: menus\prefabs\stentorious\modernminimap\main.xml, its "ModernMinimap.dll
Traits"; NVSE\CompileScript\ModernMinimap\OnRender.gek; nvse.log "ModernMinimap; version
1"] The mod's HUD window `HUDMainMenu/ModernMinimap` leaves the map picture to its plugin.
The file computes `_Render` (true when `_ForceUpdate`, or when `MapRender`'s offset from
the window's centre passes `_UpdateWidth`; the mod's scripts force it by setting
`_MapOriginX` to -999999 on load, cell change, fast travel and key presses), and reads the
render's centre back from `_MapOriginX`/`_MapOriginY` to place the picture
(`_PosXOffset = (_MapOriginX - _PlrX) * _WorldScale`, `_WorldScale = _MapScaleLocal /
_Frustum`) and every icon. The picture spans `_FrustumSize` (`_Frustum` 2048 /
`_MapScaleLocal`) game units each way, from a camera `_CameraOffset` (10,000) over the
player. The mod's render-update handler writes `_PlrX`/`_PlrY`/`_PlrZ` and `_PlrDeg`.

NVForever [design] (`nvf_play::tiles::modern_minimap`, `nvforever::minimap_render`): while
`_Render` reads true, a camera straight down over `_PlrX`/`_PlrY` renders the picture,
given to `MapRender` as the runtime picture `nvf:modern_minimap`, and the render's
centre is written to `_MapOriginX`/`_MapOriginY`. The file's own tint (`systemcolor`,
`brightness`) colours it. Engine design choices: the camera draws the world layer only
(not the player's body or the water's own layer), without fog, clearing to black, with
no MSAA; it draws to 20,000 units below its height over the player; when `_CameraOffset`
does not read, it uses the file's 10,000. At startup, `nvforever::minimap_render` reads
`NVSE\Plugins\ModernMinimap.ini` case-insensitively through the VFS ([documented: Modern
Minimap - ESPless v4.2.1 (by Stentorious & xConfused,
`https://www.nexusmods.com/newvegas/mods/90574`), shipped
`Data\NVSE\Plugins\ModernMinimap.ini`, comment `"; Controls the texture size of the
minimap render"`, default `[General] textureSize=1024`; no DLL or plugin source
consulted]) using a BOM-, comment-, and UTF-8-safe byte parser [design]; positive finite
`[General] textureSize` values are clamped to `256..=2048` and snapped to the nearest
power of two (`256`, `512`, `1024`, `2048`) [design], driving both the render-target
`Image` extent and the size published to `RuntimeImages`. When the INI is absent, omits
`textureSize`, or specifies a non-finite / `< 1` / invalid value, NVForever defaults to
`1024` so unmodded vanilla behavior is unchanged [design].

[hypothesis] The plugin's camera is orthographic with north up. This has not been observed
in the original game. Open: the plugin's exact look (classic mode's shading, interiors
under a ceiling 10,000 over the player) and any other INI settings future versions may add
are not read; the picture is the lit scene from above.

## 12. BookMenu layout variants and JIP TextInput layout (`BookMenu`, `TextEditMenu`)

No third-party plugin DLL or source code was consulted; this section relies solely on
NVForever's existing menu-index rules, public GECK wiki documentation, and public XML
menu layouts.

### 12.1 `BookMenu` layout variants (`fnv_ui::MenuIndex`)

- [documented: `menus\book_menu.xml`, audio.md; GECK wiki Hardcoded UI Shortcuts,
  revision 39903 (`BookMenu` class ID 1026)] The base game defines `BookMenu` in
  `menus\book_menu.xml` with `<menu name="BookMenu"><class>&BookMenu;</class>`.
- [design: `crates/fnv-ui/src/index.rs`] Book-display mods (such as BookMenuRestored)
  ship additional alternate-background `<menu name="BookMenu"><class>&BookMenu;</class>`
  XML files directly under `menus\` beside `menus\book_menu.xml`. Because ASCII sorting
  places `menus\book_menu.xml` (`_` = 95) before names like `menus\bookbackground1.xml`
  (`b` = 98), the generic later-sorted-path-wins rule previously clobbered
  `menus\book_menu.xml` and emitted two duplicate warnings per variant. When canonical
  `menus\book_menu.xml` is present alongside alternate `BookMenu` XML files,
  `MenuIndex::build` keeps `menus\book_menu.xml` as the primary `files` and `classes`
  entry regardless of alphabetical order, records the alternate paths in sorted order
  under `MenuIndex::variants("BookMenu")`, and suppresses duplicate-menu / duplicate-class
  warnings for those variants. When `menus\book_menu.xml` is absent, or for any other menu
  name/class, `MenuIndex::build` retains its existing later-sorted-path-wins and duplicate
  warning behavior. Unmodded vanilla (`menus\book_menu.xml` alone) is unchanged.

### 12.2 `TextEditMenu` and JIP `ShowTextInputMenu` (`fnv_game::ui_text_input`)

- [documented: `menus\dialog\texteditmenu.xml`, chargen.md §5; GECK wiki Hardcoded UI
  Shortcuts, revision 39903 (`TextEditMenu` class ID 1051)] Vanilla `GetPlayerName` /
  `ShowNameMenu` opens menu `1051` (`menus::NAME`) with `menus\dialog\texteditmenu.xml`
  (`TEM_MainRect/textedit_prompt`, `TEM_MainRect/textedit_text`,
  `TEM_MainRect/textedit_button_ok`).
- [observed: JIP LN NVSE Plugin 57.30 (`JIP LN NVSE Plugin-58277-57-30-1716662080`),
  shipped `NVSE\Plugins\textinput\texteditmenu.xml` read directly; only the XML layout
  was read, not the DLL or source] JIP LN 57.30 ships
  `NVSE\Plugins\textinput\texteditmenu.xml` declaring
  `<menu name="TextEditMenu"><class>&TextEditMenu;</class>` with root numeric traits
  `user0` (width, default 400), `user1` (height, default 200), `user2` (x offset,
  default 0), and `user3` (y offset, default 0), plus `NOGLOW_BRANCH/TEM_TitleText`,
  `NOGLOW_BRANCH/TEM_TextInputBox/TEM_InputText`, and `NOGLOW_BRANCH/TEM_OKButton`.
- [design: `crates/fnv-game/src/ui_text_input.rs`, `crates/fnv-game/src/ui_loading.rs`,
  `crates/fnv-game/src/nvse_jip12.rs`, `engine/nvf-play/src/chargen.rs`]
  - While `ShowTextInputMenu` is active (`Game::text_input().is_some()`),
    `ui_loading::sync` prefers `nvse\plugins\textinput\texteditmenu.xml` when present in
    the VFS, falling back to the indexed `TextEditMenu` class path
    (`menus\dialog\texteditmenu.xml`) when absent. Vanilla `GetPlayerName` /
    `ShowNameMenu` (`Game::text_input().is_none()`) always loads the vanilla indexed path
    with no extra trait writes.
  - `MenuLifetime::unavailable` keys by `(menu_code, jip_text_input)` so a missing vanilla
    `TextEditMenu` XML does not block a present `textinput\texteditmenu.xml` (or vice
    versa), and switching layouts directly while menu `1051` stays open unloads the stale
    root and rebuilds from the active layout.
  - On build and on `SetTextInputExtendedProps` / `SetTextInputString` / live typing in
    `nvf_play::chargen`, `ui_text_input` writes only traits that already exist in the
    loaded XML (`Ui::set_existing`): `user0`/`user1` when positive dimensions are given,
    `user2`/`user3` offsets, title string (`TEM_TitleText` or `textedit_prompt`), and live
    input string (`TEM_InputText` or `textedit_text`). Live text updates touch only the
    input text tile so script `SetUIFloat`/`SetUIString` writes on other traits are not
    overwritten.
- [hypothesis] Whether JIP LN's original plugin updates `TEM_InputText/string` on every
  keystroke immediately or on a deferred menu tick, and whether it uses additional internal
  `user*` traits on `TEM_InputText` for scrollbar line math (`user0`/`user1`/`user2`),
  have not been observed in the original game; NVForever does not render `TextEditMenu`
  from its XML tiles yet (the hand-built `chargen::name` box continues to draw it).

### 12.3 Shared `GenericMenu` class indexing (`fnv_ui::MenuIndex`)

- [observed: vanilla `Fallout - Misc.bsa` `menus\generic\quest_added.xml` and
  `menus\generic\skill_perk.xml`] Multiple distinct vanilla menus under `menus\generic\`
  declare distinct `<menu name="...">` roots (`QuestAddedMenu`, `SkillPerkMenu`) while
  sharing `<class>&GenericMenu;</class>`. `GenericMenu` is not one of the 40 single-file
  `MENU_CLASSES` entries (`crates/fnv-ui/src/value.rs`).
- [design: `crates/fnv-ui/src/index.rs`] When multiple files declare
  `<class>&GenericMenu;</class>`, `MenuIndex::build` keeps the first sorted path in
  `classes` (`class_path("GenericMenu")`), records additional paths in sorted order under
  `variants("GenericMenu")`, and does not emit a spurious `duplicate menu class
  genericmenu` warning. Each menu's `<menu name="...">` entry (`path(name)`) and all 40
  `MENU_CLASSES` lookups (`code_path(code)`) are unchanged, and duplicate `<menu name="...">`
  collisions still emit `duplicate menu ...` warnings as before.
