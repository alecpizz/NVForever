# The tutorial menu (ShowTutorialMenu)

Plain-language notes for `nvf_play::tutorial_menu` and `fnv_game::Game::show_tutorial`
(2026-10-18, claude/menu-hooks). Sources: the GECK wiki export (owner's, C:\fnv-private\geckwiki),
the game's `menus\tutorial_menu.xml` (owner's extract, C:\fnv-private\menus-extract) and
the masters' scripts decompiled with `fnvtool script decompile`. No game data is kept here.

## 1. What scripts do with it

- `ShowTutorialMenu <message>` "shows the specified Message Box form, similar to
  ShowMessage, but with slightly different visuals. These are the 'VDSG Manual' styled
  message boxes" [documented: GECK wiki ShowTutorialMenu].
- The masters call it twice [observed]: CGTutorial (QUST 0x00059C85) stage 212, entries 0
  and 1, show `HelpHealingLimbs` (MESG 0x00072358) and `HelpHealingLimbsXBox`
  (0x00072359); the entries' conditions pick the platform. Stage 212 is set from the
  quest script's `Begin MenuMode 1` block when a limb of the player is crippled, so the
  menu opens while the Pip-Boy is open.
- The messages are message boxes (DNAM Message Box) with a title (FULL "Healing Limbs")
  and a text (DESC) marked up with `<div>` at the start and `<p>` between paragraphs
  [observed].
- The text names the controls by key strings: `&-sUActnUse;`, `&sUActnCrouch;`,
  `&sDeviceMouse;` [observed: #1909, Movement and Combat 0x0006D914]. The menu shows
  them as the keys or buttons bound now, as the lockpick help and the notes do
  (`lockpick::help::expand_keys`, lockpicking.md) [hypothesis: the game's words for the
  keys; the strings are the same as the lockpick help's].

## 2. The menu file [documented: menus\tutorial_menu.xml]

Lengths in the interface's units, from the background's top left.

- A black background (`solid_black.dds`) 0.8 of the screen wide and high, centred, at
  the menus' fill alpha (`_background_fill_alpha`, 32 of 255).
- Top: at x 48, y 96 a line fading down (vertical_fade_line.xml, 60 long) and a line 50
  long to the right; the title (font 6, "replaced in code") 10 after that line, 5 lower
  than centred on it; a line on to the page text "??/??" (font 6, right-justified, 10
  before the right stub; "replaced in code"); when the page text is hidden a line fills
  its place; a stub 50 long ending 48 before the right side, and a line fading down there.
- Text: a clipping column from the title's x, under the top fade lines (y 156), to the
  lower fade lines, ending 64 before the Previous button; the text 5 in and 21 down (the
  file's "font adjustment"), wrapping 5 short of the column, moved up by the scroll bar's
  value. A vertical scroll bar beside the column at the left corner line; a step of 30.
- Buttons (text_box.xml, right-justified, right side 81 before the background's right):
  Next (`sNext`) at the column's top, Previous (`sPrevious`) under it, Close
  (`sCloseButton`) standing on the lower right fade line's top. PC key hints
  `sPCMenuHintA`, `_S`, `_E` after them; the PC keys `_PCButton_A` Next, `_S` Previous,
  `_E` Close.
- Bottom: the corners' lines fading up from 96 above the bottom, a stub 50 long, the
  VDSG symbol (`Interface\Loading\glow_loading_VDSGSymbol.dds`, 64 high, its width less
  36 of dead space), the words `sVDSGManual`, a line on to the right corner.

The strings are game settings that are not in the masters (the executable's defaults).

## 3. What NVForever does

- `Game::show_tutorial`: the message is a page of the tutorial menu, which opens over the
  menu that is open and remembers it (`Game::menu_under`); closing the tutorial makes that
  menu the open one again; a message already shown is not added twice; further messages
  while it is open are more pages [hypothesis]. A message box shown meanwhile waits.
  If the engine closes the menu underneath (the Pip-Boy put away), it is forgotten.
- MenuMode 1059 while it is open, and not 1001: it is no message box [observed: capture mh1_tutorial, save nvfstrip, 2026-10-18].
  `MenuMode 1` stays true over the Pip-Boy [hypothesis]. A second ShowTutorialMenu while it
  is open leaves one 1059 menu, and one CloseAllMenus closes it together with the menu that
  was open under it (MenuMode 0 after it) [observed: the same capture]; NVForever's
  `Game::close_all_menus` does the same.
- Drawn as §2 says, with these choices: the words (`sNext`, `sPrevious`, `sCloseButton`,
  `sVDSGManual`) are read by name from NVForever's game strings table,
  the load order's string settings over its own labels (menu-xml.md §2.4; the
  maintainer's decision 2026-10-30) [design]; the key hints after the buttons in the
  menus' usual button style, following the input device: "Close E)" with the keyboard,
  "Close B)" with a controller [observed: the owner's play of the original, issue #1904],
  and Next "A)" / "RT)", Previous "S)" / "LT)" in the same style [design]; NVForever
  writes them "[E]", "[B]", "[A]", "[RT]" … (#4495, docs/design/controller.md "Hint shape")
  (`tutorial_menu::hints`; `sPCMenuHintA/S/E` are in no plugin, and NVForever's own labels
  for them were "[A]"-shaped); the background as dark as the other menus
  (0.75) instead of the file's alpha 32 [design: readable over the Pip-Boy]; the page
  count only with more than one page; a thin scroll mark instead of the scroll-bar
  prefab; the text in font 1 [hypothesis: a text tile without a font]; `<p>` and `<br>`
  start paragraphs and other tags go.
- Keys: A or Right next, S or Left previous, E, Esc or Enter close, Up, Down and the wheel
  scroll; clicks on the buttons. The Pip-Boy's own keys are held while the menu is over
  it.

## 4. Open

- Settled by capture mh1_tutorial (`tools/capture/probes/menu_hooks.toml`): the code 1059.
  Answered by the capture hud_tutorial_more (private, 1280 x 720): a second ShowTutorialMenu
  **replaces** the first. With two messages the title and body are the second's, `TM_page` stays
  hidden, Next and Previous stay hidden and not targets, `ClickMenuButton` on them returns 1 with
  the text unchanged (the keys A and S were not tried: the job taps them only when it returns 0);
  one close closes it [observed]. NVForever's pages (§3, §5) differ from that; not changed yet. Still
  open: which menu was open under
  it in the capture (MenuMode was 1 before the first call, its code not asked), so whether
  the menu under the tutorial answers MenuMode is not settled.

## 5. Drawn from the menu file (T15b part 2)

Behind `[UI] TileMenus=TutorialMenu` (`engine/nvf-play/src/tiles/tutorial_menu.rs`) the open
menu (1059) is drawn from `menus\tutorial_menu.xml` and the hand-built drawing gives way to a
sheet taking the pointer at the same stacking level (109); its pages, keys, wheel and close
stay the hand-built menu's (§3) [design]. Smoke: `tiles-tutorial`, `tutorial-default-drawing`.

The original's values, read at three window sizes [observed: capture b2_tutorial, private;
values in interface units; the message 0x0007235B]:

| tile | 1280 x 720 | 1280 x 1024 |
|---|---|---|
| `TM_Background` | 170.67, 96, 1365.33 x 768, alpha 193.8 | 120, 96, 960 x 768 |
| `TM_title` | 108, 83.33, 145 x 38, font 6 (y 82.89 at 1920 x 1080) | y 82.94 |
| `TM_page` | x 1257.33, 75 x 38, font 6, the file's `??/??`, not on the shot | x 852 |
| `TM_text_container` | 108, 172, 985.33 x 440, `_text_height` 625, `_text_y` 21 | width 580, `_text_height` 811 |
| `TM_text` | 5, 21, 965 x 625, no font trait (font 1), wrap 980.33 | 558 x 811, wrap 575 |
| `TM_scrollbar` | 33.33, 172, 32 x 424, `_number_of_items` 655 | items 841 |

- **The file has no `io()` traits** [documented: tutorial_menu.xml; the review's plan expected
  some]. It gives ids to `TM_text` 0, Next 1, Previous 2, Close 3, `TM_title` 4, `TM_page` 5,
  `TM_scrollbar` 6 and the words after the symbol 7, and says "replaced in code" of the title
  and the count; the body has no string at all. So the model writes the tiles' own traits:
  `string`, and `visible` and `target` on the count and the buttons [design; what the
  original's code writes is not known].
- **The body's lines**: the text is the message's with its line breaks kept, tags gone, each
  line trimmed and runs of empty lines as one (`tiles::tutorial_menu::body_text`). 0x0007235B has
  paragraphs apart by a double break and a list by single breaks; the original shows each item
  on its own line and the double breaks as one empty line, 20 lines [observed: the 1280 x 720
  shot, launchtest tutorial]. `<p>` is an empty line [hypothesis: the hand-built menu's reading,
  no shot of 0x00072358 yet]. The hand-built `plain_text` (§3) joins single breaks, so that list
  runs together there; it is left as it is, for the maintainer to decide [design]. In the tile
  path that join gave 532 high and 122 to scroll, and the break gives 625 and 215 [observed: the
  log before; the original's reading].
- **Sizes**: the body's height is the measured one, font 1's 36 + (lines - 1) x 31 [observed:
  625 = 36 + 19 x 31, 811 = 36 + 25 x 31]. The file carries it on: `_text_height`, and the
  scrollbar's items are that plus its step of 30 [documented; observed: 655, 841]; its visible
  items the column's height, its jump the height less a step [documented]. Every text is
  measured by the renderer's fonts, worked out and measured again until the body stands at its
  final wrap width, each round's fonts loaded as the tree then stands, so each has a face (the
  game's or a stand-in) and every measure is kept, 0 x 0 too [design]. That width follows Previous's place, 985.33 = 1284.33 - 127 - 108 - 64
  [observed], and that place its word's width.
- **The scroll's range** is the file's, items less visible items and at least 0: 215 here, and
  the last step is what is left of the range, not a whole step [documented: scrollbar_vert.xml,
  `_current_value`]. The hand-built `max_scroll` adds the text's 21 and rounds up to steps; it
  is not used [design].
- **The scroll's value** is the hand-built menu's. The prefab's `_current_value` adds the
  arrows' and page areas' clicks to its old value [documented]; in NVForever's tree an
  expression starts from 0 (wait-menu.md §12), so the model writes the value and pokes
  `_SetInCode` on every frame the menu is tiled, as that prefab's comment asks [documented], and
  clicks on the pieces are the hand-built menu's commands: arrows 30, page areas 410 at this
  size, the marker the value under the pointer with the prefab's `_step_height` [documented;
  design: the marker's own drag traits are not built]. The wheel and the keys stay hand-built;
  the file's `wheelmoved` stays 0.
- **The marker** is the prefab's own: `y` is the value x `_step_height`, (424 - 32) / 215 = 1.82,
  so 54.70 at 30, 218.79 at 120 and 392 at the bound, the track's end less the marker's 32
  [documented: scrollbar_vert.xml; observed: the same numbers, hud_tutorial_more]. With the old
  range it stood where the formula says too: at 30 of 122 its middle at 285 pixels, 380 units,
  against 268 + 30 x 392 / 122 + 16 [observed: tiles_tutorial_bottom and the log's line]. That shot
  was at 30, not at 120: the click before it had gone up.
- **The page areas** lie above and below the marker, so one place is either, by the value and the
  range: y 600 on the screen is page up at 120 of 122 and page down at 120 of 215 [documented;
  observed: the log's `Scroll(-410.0)` with the old range]. In the original a page from 120
  reaches 215, `_jump_size` 410 [observed: hud_tutorial_more].
- **Clipping and scrolling are apart.** The file moves the text: `TM_text`'s `y` is
  `_text_y`, 21 less the value [documented]. `clipwindow` on `TM_text_container` crops it
  (menu-xml.md §3.1.1) [observed: a line stops part way at the column's top and bottom edges,
  shots tutorial__scroll_bottom and tutorial__scroll_arrow, hud_tutorial_more]; the model
  turns the reading on for this menu alone, every frame after the scripts and before the
  drawing, so a window injected or enabled later crops at once. `_NVFClipWindow` and
  `_NVFOwnAlpha` are engine-written marks, ordinary traits to the tree.
- **Alpha**: all of the menu's content is inside its background picture, whose alpha is 193.8
  in the original, and its title, body, lines and symbol read full strength, (255, 182, 66)
  [observed: shot 091 of hud_menus, 1280 x 720]. NVForever's layout multiplies alpha down from
  a picture, so the model marks `TM_depth_rect` to take none from above
  (`tiles::layout::OWN_ALPHA`); whether pictures pass alpha on elsewhere is open (observer
  check 83).
- **The backing** is the original's 193.8 [observed], not the hand-built menu's 0.75, which the
  writer used to pass in. The picture is `solid_black.dds`, nearly black with soft edges: its
  alpha falls to 0 over about the outer 18 % of its height and 5 % of its width [observed: the
  texture's alpha channel, Fallout - Textures2.bsa], so the box has no sharp edge and its lines
  stand inside the fade. Across the left side, x 126 to 176 on a 1280 x 720 shot, the original goes
  from 80 to 23, 41 to 12 and 51 to 18, about 0.3; NVForever at 0.75 from 69 to 37 and 100 to 40, 0.4 to
  0.55 [observed; different worlds behind, so rough]. 0.76 in display values leaves 0.24; in linear
  light it leaves 0.24^(1/2.2) = 0.52, which fits ours, so 0.75 to 0.76 cannot close that gap
  [hypothesis: NVForever's UI blends in linear light, the original in display values]. For
  black, an alpha of 1 - (1 - a)^2.2 would give the same display values; not done.
- **The world behind the menu** is blurred in the original, the menu itself sharp [observed:
  shot 017 of hud_tutorial_more, 1280 x 720, private]. So while the menu is drawn from its tiles
  the world has the image space's blur (image-modifiers.md §5), radius 3 and on and off at once,
  as behind the wait menu [design: a shot shows that it is blurred, not its radius or timing].
  It follows the drawing's own decision (`tiles::menus::TileMenuBlur`, written by `tiles::draw`):
  a first frame still drawn by hand, a fallback and the switch off have none, and the hand-built
  drawing stays as it was. Over the Pip-Boy the stronger of the two counts, its near depth stays
  sharp, and the close leaves its blur (smoke `tiles-tutorial`, the shots
  `tiles_tutorial_over_pipboy` and `_closed`). The world is not dimmed for this and the backing
  is untouched: the gap the previous points measure is a separate question.
- **The words after the symbol and the key hints** are `&-sVDSGManual;` and `&-sPCMenuHint*;`
  [documented], which no plugin of the test load order defines: no record in FalloutNV.esm, the nine
  add-ons or the two test plugins [observed: `fnvtool esm decode --edid`]. They read
  NVForever's own labels (`fnv_ui::strings::BUILT_IN`, menu-xml.md §2.4; LEGAL.md), and no code
  writes those tiles. The original shows other words, from outside the data files, not
  transcribed (`_PCButtonText` reads them there [observed: hud_tutorial_more]).
- **The symbol**: `width` is `filewidth` less 36 and the words after it follow, so the file
  alone reads -36 until the code writes `filewidth` [documented]. The picture is 128 x 64
  [observed: the texture's header, Fallout - Textures2.bsa]; it is read from the name the file gives, once.
- **The buttons**: with one page neither Next nor Previous shows [observed: the same shot], and
  the count is hidden with `TM_fill_line` in its place by the file's rule [documented; observed:
  not on the shot]. Previous still keeps its width, so the column keeps its place [observed:
  985.33]. With more pages a button with nowhere to go stops being a target: the file's
  `_line_alpha` dims it and it takes no highlight [documented: text_box.xml; design: the
  hand-built menu's rule; multi-page is not captured, observer check 84]. `n/N` is NVForever's
  (§3). Close is read (206, 147, 53) at its brightest on that shot, against the body's
  (255, 182, 66), with its text's alpha 255 and brightness 255 [observed: hud_tutorial_more], so
  font 2's texels [hypothesis].
- **One press acts once.** A menu a script opens in its tick is drawn by hand that frame and
  built the next, so on the first tiled frame the hand-built buttons are what is on screen: the
  frame's press is theirs and the tile input takes nothing until they have gone with that frame's
  drawing (`tiles::input`), so a Next never turns two pages and a press never falls on a hitbox
  the file moved elsewhere [design]. A menu already built when it is first drawn has no hand-built
  buttons. A click a key makes through the file's `_PCButton_*` or `x…` refs
  acts unless the hand-built menu took that key this frame (`tiles::menus::key_click_acts`).
  The triggers needed care: RT and LT stand for this menu's A and S
  (`crate::pad::menu_buttons`), not for the shared Page Up and Page Down, so the tile input asks
  `pad::menu_key_at` for them, or RT would turn the page by hand and again by the file's
  `xbuttonrt`. RB and LB, which the hand-built menu does not read, page the text through the
  file. A script's MenuTapKey and ClickMenuButton act through the file. A key or hand-built
  button that asks for a turn wins the frame's turn, even at an end of the pages: a tile's Next
  or Previous that same frame is dropped, its Close and scroll still act [design].
- **The order in a frame**: the tile input's commands reach the hand-built `keys` the same
  frame; then the page, its sizes, its range and the scroll are written together, before the
  scripts, so a key or a click never draws a new page at an old scroll. A window resize
  settles in its own frame too (the tree is worked out before the text is measured). The
  writer runs before `run_scripts`, so a page a script adds there reaches the tree the next
  frame: the count and the buttons are one frame behind the game's pages; the page shown
  and its scroll do not change with it [design].
- Off, or in the fallback, a script's queued click on the file's tiles does nothing: the
  commands take the drawing's own decision. A marker drag ends with its press or its menu.
- A file a mod restructured (a model path, a scrollbar piece or a button's words missing)
  keeps the whole hand-built drawing, with
  one warning. The frame a menu opens on its tree is not built yet, so the hand-built drawing
  shows for that one frame [design: as the other T15 menus].
