# Spec note: the barter and container menus

Tags as in `nif.md`: `[documented]` cites a page or file, `[observed]` says what was
looked at, `[hypothesis]` is a reading that nothing has confirmed yet. `[design]` marks
what is NVForever's own. Implementation: `engine/nvf-play/src/menu/look.rs` (the
drawing), `menu/model.rs` (lists, trade, commands), `menu/barter.rs` (prices),
`menu.rs` (opening, keys, pointer), `crates/fnv-game` `ShowBarterMenu`.

## 1. Sources

- **Four pictures from the owner**, 2026-10-11 (kept privately): two screenshots of the
  original's barter menu with the merchant Blake (1280 x 720 and 1600 x 1024, the second
  with a Gold Bar put up for sale), a small screenshot of the container menu on a
  refrigerator (384 x 216), and a cut from a web page showing a container menu with an
  item's figures (DPS, WG, VAL, STR, CND, ammunition, mods). The owner: the container
  menu looks "very similar" to the barter menu, without the caps.
- **The game's interface files** in `Fallout - Misc.bsa`: `menus\barter_menu.xml`,
  `menus\container_menu.xml` and the shared pieces they include
  (`menus\prefabs\list_box.xml`, `list_box_template.xml`, `scrollbar_vert.xml`,
  `item_stats_display.xml`, `card_info.xml`, `text_box.xml`, `bottom_bracket.xml`,
  `line.xml`, `vertical_fade_line.xml`, `box.xml`, `menus\globals.xml`). Described here
  in plain words; nothing of them is shipped.
- **The textures they name**, in `Fallout - Textures2.bsa`: the title arrows
  `textures\interface\main\main_arrow_left.dds` / `_right.dds`, the caps arrows
  `textures\interface\barter\barter_arrow_buy.dds` / `_sell.dds`, the item arrows
  `textures\interface\shared\arrow\left.dds` / `right.dds`, and from the shared atlas
  (`textures\interface\interfaceshared.tai`): `square.dds`, `square_filled.dds`,
  `arrow_up.dds`, `arrow_down.dds`, `vert_marker.dds`, `solid.dds`, `fade_to_top.dds`,
  `fade_to_bottom.dds`. Items' pictures are their ICON paths
  (`interface\icons\pipboyimages\items\...`).
- **The GECK wiki** (owner's export): "Barter System Formula", "Item Condition And
  Value", ShowBarterMenu, "Merchant Restocking", GetBarterGoldAlt, Service Tab.
- **FalloutNV.esm**: the barter settings, Chet's records.

## 2. What the files say

[documented: the two menu files and their pieces] In the interface's units (960 to the
screen's height):

- Both menus are a 1024 x 800 rectangle centred on the screen. Behind it a black
  picture (`interface\shared\background\solid_black.dds`) 40 left of it and 105 above,
  1144 x 970, with the shared background opacity (32 of 255).
- Two halves of 464 x 480: the player's 20 in and 20 down, the other 560 further right.
  Each has a heading: a short line fading downwards at each end, a line along the top,
  the title (font 6) 50 in with an arrow 17.5 x 35 before it at 30 and after it, the
  line resuming 20 after the right arrow, and a figure in font 2 right-aligned at 449,
  20 down. The barter menu's title stands 10 above the line, the container menu's 12.5.
  The player's title is the inventory's word for items; the other half's title is set
  by the code (the container's or merchant's name). The arrows are clickable and so are
  the titles (the mouse wheel on them).
- The barter menu has a figure on both halves (set by the code; the screenshots show
  the caps). The container menu has one on the player's half only (the screenshots show
  the weight carried and the most that can be).
- Each half's list is 15 left of the half and 60 down, 462 (player) or 464 wide and 365
  high, clipped. A scroll bar 32 wide sits at its left, with 16-high arrows at its ends
  and a marker 6 wide, 12 in, at least 32 high; it shows only when the list is longer
  than the window. The list scrolls by 26, half a row. Only one list is enabled at a
  time, the one with the focus; moving the pointer over a half gives it the focus.
- A row's height is its text's and 20. In a row, 32 right of the list's left (after the
  scroll bar's room): the name in font 2 at 40, 20 down, wrapped at 300 in the barter
  menu and 370 in the container menu; in the barter menu a value right-aligned at 403;
  at 15, 15 a 25 x 25 square, filled for an equipped item, open for an item chosen for
  the trade. The row under the pointer gets a box one unit shorter than the row, its
  lines at brightness 200 and its fill at 40 of 255 at half brightness. Rows play
  `UIMenuOK` when clicked and `UIMenuFocus` when the pointer comes on them.
- Each half's footer: lines fading upwards from 440 to 480 at its two ends and a line
  along 480.
- A bottom bracket under both halves 820 down the screen: a line with lines fading up
  40 at its ends.
- Barter only: between the halves, set visible by the code, the caps arrow: a picture
  120 x 360 (`barter_arrow_buy.dds` or `barter_arrow_sell.dds`, chosen by the code's
  direction) and the amount in font 6 centred over the word "Caps" in font 2. The
  arrow's rectangle is 449 right of the player's half and 260 down the screen; with
  the second direction everything moves 20 right, the picture 25 left within it, and
  the figure 7.5 right; a figure wider than 70 moves by its excess.
- Container only: between the halves, set visible by the code, an item arrow 64 x 128
  (`left.dds` or `right.dds`), 474 right of the player's half and 250 down the screen.
- Buttons at the bottom right (text boxes, right-aligned, the right end 10 left of the
  other half's right end, 670 down the screen, each its text and 15 high): Accept and
  Exit in the barter menu, Take All and Exit in the container menu, each followed by its
  PC key hint. Accept is drawn at 128 of 255 when it cannot be used.
- The item's picture (256 x 256) 22 right of the player's half and 472 down from it,
  and the item's figures 535 down from the halves: 287 into the menu (barter) or 307 into
  the player's half (container). The figures are tiles a third of 530 x 120 (176.7 x 40):
  damage, damage per second, resistance or threshold at the left; weight at the
  second third and 10; value at the third; strength needed above the value; condition
  as a bar (62 in, 15 down, 20 high, 72 short of the tile) and the ammunition at the
  second row; effects and mods across. A tile has a line along its top, a line fading
  down its right side, its title at 5 and its value right-aligned 10 before its end,
  both 20 down.
- Container only: a text centred under the menu, 800 down the screen.
- Fonts by number: font 2 is `Monofonto_Large`, font 6 `Glow_Monofonto_VL_dialogs`
  (`fonts.md` §4). Lines are 2 thick on a PC.

## 3. What the screenshots add

[observed, the 1280 x 720 barter screenshot unless said otherwise]

- Positions agree with §2 to a pixel or two: the names start 57 right of the half, the
  values end 420 right of it, the weight and value tiles stand at the second and third
  thirds 555 down, the Accept text's right end at 1024 of the menu.
- Rows are 51 apart (39 pixels at 720 lines, 54 at 1024), seven to a list.
- A row reads "Name (count)" when there is more than one; caps are not listed in the
  barter menu but shown as "Caps  N" in each heading; the container's player heading
  reads "Wg  carried/most". The container lists "Bottle Cap (18)" as an item.
- Rows are sorted by name in byte order: "$5 NCR" and ".223 Round" before digits and
  letters, "LAER+" before "Lad's Life" (capitals before small letters).
- The values are prices, not the items' values: at Barter 100 the Gold Bar (value
  10547) shows 9492, Hydra (55) 50, Jet (20) 18, the .308 Armor Piercing round (9) 10,
  the .308 Round (4) 4, the $5 NCR (2) 2. Items worth nothing show "--"; Embalming Fluid
  shows "0.9". The VAL tile shows the item's own value (10547).
- An item chosen for sale leaves the player's list and stands first in the merchant's
  list with the open square (second screenshot: Gold Bar, value 9492); the arrow points
  at the player with "9492 Caps".
- The key hints read "Accept A)", "Exit X)" (barter) and "Take All R)", "Exit E)"
  (container; "Take All A)" on the 2026-11 launchtest shot, §8); the hint stands about 16 after its button's text. Accept is dim with
  nothing chosen and bright with a trade.
- The picture behind darkens the world by about a third at its left edge, with a soft
  edge about 25 wide; the world behind is blurred (second screenshot).
- The container screenshot's note reads "This container respawns and is unsafe to
  store your items."

## 4. Barter rules

- **Prices** [documented: GECK wiki "Barter System Formula"]: buying costs the value
  times `fBarterBuyBase + Barter × fBarterBuyMult / 100`, selling fetches the value times
  `fBarterSellBase + Barter × fBarterSellMult / 100`; a bought item never costs less
  than its value and a sold one never fetches more. FalloutNV.esm sets the four settings
  to the page's defaults: 1.55, -0.45, 0.45, 0.45 [observed: `fnvtool esm decode`].
  Barter is the player's skill; the merchant's does not count [documented].
- **Rounding** [observed: §3, all the screenshot's prices]: to the nearest cap, halves
  up; under one cap one decimal is kept [hypothesis: from the single "0.9" row].
- **Condition** [documented: "Item Condition And Value"]: an item's value is multiplied
  by `fItemConditionValueBase + (1 − base) × fItemConditionValueMult × (condition % /
  10) ^ fItemConditionValueExp`; FalloutNV.esm sets the base to 0 and the multiplier to
  0.0316 and has no exponent, so the page's default 1.5 is used [observed].
- **Discount** [documented: GECK wiki ShowBarterMenu]: `ShowBarterMenu 50` makes the
  merchant sell at half and buy at one and a half of the normal prices; the parameter is
  -100 to 100. It is applied after the limits [hypothesis].
- **The merchant's goods and caps** are its merchant container's (the XMRC of its
  reference) [observed: Chet's `VendorContainerChet` holds his stock and two vendor caps
  lists of 300; his own inventory holds a few items and pocket change]. The merchant's
  own inventory is not offered [hypothesis]. Without a merchant container, the actor's
  own inventory is used [design].
- **Owned items lying at a merchant's stand** are not in its barter list [design: the
  stock is the merchant container's alone]. Open question (#3350, observer check 256):
  Bitter Springs Infirmary Blues (VMS40) sends the player to Blake for two books
  (`VBSPsychologyBook` 0x00134995, `VBSPediatricsBook` 0x00134996). They are not in his
  merchant container or its leveled lists, and no script gives them out. Each is one
  persistent reference owned by Blake (XOWN 0x000F1863) on his stand in WastelandNVmini
  (`VBSPsychologyBookREF` 0x00178DB0 on the upper shelf, `VBSPediatricsBookREF`
  0x00178DAF on the lower one) [observed: FalloutNV.esm]. In NVForever both are drawn
  there and can be taken (a theft from Blake), which completes objectives 20 and 40
  [observed: 2026-11-22, smoke blake-books]. Whether the original also offers them in
  Blake's barter list is not known.
- **What the merchant sells** is limited to its service categories (AIDT "Buys/Sells",
  or its class's): the other items of its stock are held back; it buys anything
  [documented: GECK wiki Class; the categories: vendor-services.md §2].
- **The total** is the sum of each line's rounded price times its count [hypothesis].
  Accept is refused when the player has too few caps [hypothesis; the wording is ours:
  "You don't have enough caps."]. A sale beyond the merchant's caps goes through at the
  caps it holds, and the figure shows that amount [observed 2026-11-06: section 9].
- **Quest items** cannot be sold [documented: GECK wiki Category:Items]; they are not
  listed [hypothesis].
- **Keys (`KEYM`)** are not listed on either side of the barter menu (#3432)
  [hypothesis: issue #3432 reports that keys do not appear in barter in the original
  engine; not yet captured with a parity probe. Excluded from both player and merchant
  lists regardless of item value or Quest Item flag; keys remain listed in the Pip-Boy
  inventory (`Keyring` folder) and in container/pickpocket menus].
- **Restocking** on two days of the week [documented: "Merchant Restocking"] is the
  simulation's (`fnv-sim` merchant restock days), not the menu's.
- **The menu's code** for `MenuMode` is 1053 [hypothesis: the GECK wiki's list as
  remembered; no shipped script tests it].

## 5. What NVForever does

- Draws both menus by §2 and §3 with the game's two fonts and pictures, read from the
  owner's archives at run time; without them the same layout with Bevy's font and plain
  lines.
- The player's half is at the left (as the files have it; the earlier plain container
  menu had it at the right).
- Category arrows switch each list between all items, weapons, apparel, aid, misc and
  ammunition [observed: titles "ITEMS" and "MISC"; the rest hypotheses]. The other
  half's title is its name for all items, else the category's name [hypothesis].
- Names wrap onto a second line as the files say; a two-line row is 35 taller.
- The pointer selects the row it moves onto and gives its half the focus, and never
  scrolls the list: a row half shown at the list's foot stays where it is when the pointer
  takes it; only the wheel and the keys scroll [design: the owner's rule, playtest list 7
  item 11; `MenuModel::pointer_pick`]. A click acts
  on the row (barter: chooses one unit for the trade, or takes it back; container: takes
  or stores one), Shift and a click the whole stack [design: the original asks how many
  with its quantity menu, not built]. The keys are listed in `menu.rs`'s notes: the
  hints' letters (A, X, R, E) as the screenshots show them, Esc and Tab to leave,
  `[` and `]` for the categories [design].
- The item's picture and its figures: the Pip-Boy ITEMS page's (pipboy.md §4–§5,
  `nvf_play::pipboy::items::info_of` and `tiles_in`, the same `item_stats_display.xml`)
  laid out in this menu's 530 × 120: DAM (weapons) or DT (armour), DPS over it, STR over
  the value, CND as a bar for weapons and armour, the ammunition or armour class beside
  it, the effects across [design: the owner's wish, playtest list 7 item 25]; WG and VAL
  stay one unit's, WG with two decimals [observed], VAL at the unit's condition before
  the trade's multiplier. Log `menu: figures of <item>: <tiles>`. The mods tile is not
  built.
- Currency (NCR notes, Legion coins) are the caravan money records (CMNY) that
  inventories hold; their worth is their DATA "Absolute Value" (`Item::money`), the Value
  of the MISC records of the same names [observed: FalloutNV.esm; playtest list 7 item
  7].
- The caps arrow and its figure are drawn at half brightness, as a disabled button is,
  while the side that pays holds fewer caps than the balance [design: the owner's
  request].
- The container menu's note shows for a container whose record has the Respawns flag
  [hypothesis: every such container]; a refused trade's words show in the same place
  [design].
- The world behind is darkened by a third [observed] with the background picture's soft
  edges; its blur is not built (the message box note has the same gap).
- `ShowBarterMenu` (fnv-game) opens the menu for scripts (MenuMode 1053) and asks the
  engine to draw it. From a dialogue choice the menu opens over the conversation, which
  waits under it and goes on when it closes [hypothesis: the original returns to the
  conversation].

Not built: the BarterExit lines merchants say when the menu closes (GECK wiki Service Tab),
the blur, the mods tile, the "Calculate Buy Price" perk entry point, repair at merchants. The
menus from their files: §8.

### 5.1 The merchant's owned loose items (#3350)

- A merchant's barter list also offers the items it owns that lie loose in the world,
  not in any container [observed: observer check 256 and its follow-up, 2026-11-28].
  - Blake (`NVBlakeREF` 0x000F1866, at the Crimson Caravan) listed the two medical books
    of Bitter Springs Infirmary Blues: `VBSPsychologyBook` and `VBSPediatricsBook`.
    These are persistent references owned by him, lying on the ground.
  - He also listed his owned loose Purified Water (0x000151A3, one row of 8) and
    Whiskey (0x00032C75, one row of 4). These references are not persistent.
  - Neither his merchant container (`VendorContainerBlake` 0x000FD56D) nor his own
    inventory held any of them, before the menu opened or while it was open.
  - The loose references stayed enabled while the menu was open.
- NVForever offers the item references in the merchant's cell that are owned by its
  base or its reference, enabled and not taken (`menu::model::owned_loose`).
  - All units of one item make one row, together with any stock row of the same item.
  - A quest item is offered too: Blake's books are quest items.
  - Buying takes the stock's units first, then the loose references. Each reference
    goes into the player's inventory, owned by no one, so it is bought, not stolen,
    and it leaves the world as a pick-up would. The item's OnAdd then runs as for any
    item reaching the player.
- [hypothesis] The reach is the merchant's cell, by the counts. 7 of the 8 waters and
  all 4 whiskeys lie in Blake's cell; the eighth water lies somewhere else.
- [hypothesis] Only ownership by the merchant counts. Ownership by its faction or by
  the cell was not checked.
- [observed: a scan of the ten masters, 2026-11-28] 76 such items lie by 11 merchants.
  Blake has 23, the merchant reference 0x000E6822 has 21 and Genaro (Freeside) 18;
  the other eight have one to four each.

## 6. Looking at one

- `NVF_PLAY_TEST_BARTER=<merchant reference>[:discount]` opens the barter menu with that
  merchant and makes one trade, logging the lists, prices and caps; with
  `NVF_PLAY_TEST_SHOT=<path.png>` it saves `<stem>-barter.png` and
  `<stem>-barter-trade.png`. With `*` it waits for a script to open one.
  `NVF_PLAY_TEST_BARTER_DELAY` gives it a delay of its own (else `NVF_PLAY_TEST_DELAY`),
  so that it can run beside the container check. Its sounds: audio.md §12.
- The dialogue path, Chet in the Goodsprings General Store:
  `NVF_PLAY_TEST_TALK="10:GSChetRef=for sale|leave" NVF_PLAY_TEST_BARTER="*"
  NVF_PLAY_TEST_SCRIPT_MENUS=0.3 nvforever --data <Data> --cell GSGeneralStore --play
  --no-voice --size 1280x720 --run-seconds 50` (expect "menu: a script opens the barter
  menu with 0x00104C79", "menu: barter with 0x00104C79: stock in 0x00104C7A", a trade,
  "menu: close", then the conversation's Goodbye).
- `NVF_PLAY_TEST_MENU=nearest` does the same for the container menu (menu.rs).

Checked this way on 2026-10-11 at 1280 x 720: Chet's menu matches the owner's barter
screenshot's layout (headings, lists, prices' column, tiles, icon, buttons, bracket)
within a few pixels; the refrigerator of GSHouseInterior03 matches the container
screenshot.

## 7. How many? (moving part of a stack)

Sources: `menus\quantity_menu.xml` and its prefabs `meter.xml`, `top_bracket.xml`,
`bottom_bracket.xml` [documented: the owner's extraction of the game's menu files, read in
plain language here]; three screenshots of the original over the Pip-Boy, a stack of six
Varmint Rifles being dropped, at 1920 × 1080 (C:\fnv-private\itemstacks) [observed].

- **When it asks** [owner, 2026-10-15]: moving a stack of more than 5 units (take, store,
  buy, sell, take back from the trade, drop from the inventory). The original's threshold
  is a game setting with no record in FalloutNV.esm; it was not read, so "more than 5"
  is the owner's word. Five or fewer move one unit per press, as before.
- **Layout** (interface units, 960 to the screen's height): a 740 × 370 box centred on the
  screen, on the shared black background picture at the menus' backing; inside it, 22 in
  and 42 down, a frame 690 wide: a top bracket and a bottom bracket 290 lower, each a line
  with 50-long ends fading towards the other. "How many?" (game string `sHowMany`, not in
  the masters; the screenshots spell it) in font 2, centred at 345, 50 down. The meter at
  149.5, 115, 391 × 24, with the left and right arrows (`main_arrow_left/right.dds`,
  17.5 × 35) at 130 and 549. The amount in font 6 centred at 345, 170 down. Ok and
  Cancel right-aligned at 665, 155 and 200 down, with their key hints "A)" and "E)".
- **The meter**: a ground of the interface colour at a quarter of its opacity, and over it
  `hud_tick_mark.dds` repeated as far as the value (0 to 1) reaches. The value shown is
  the amount over the stack; the amount is the value times the stack, rounded. A drag
  sets the value from the pointer; the arrows step it.
- **The screenshots agree** [observed]: the frame is 775 × 327 pixels (690 × 291 units at
  1080 lines), its ends fade over about 50 units, ticks are 9 pixels (8 units) apart,
  and the first screenshot shows the whole stack (6 of 6), the later ones 4 and 3.

What NVForever does (`engine/nvf-play/src/menu/model.rs` `Quantity`, `look.rs`
`spawn_quantity`): the prompt opens at the whole stack, goes from 1 to the stack, Ok does
what asked with that many, Cancel (or closing) does nothing and leaves the menu open. Keys:
Left/Right and the wheel ±1, Page Up/Down ±10, Home/End 1 and the stack, A/Enter/Space
Ok, E/Esc Cancel [documented: A) and E); the rest ours]. While it is open the menu and the
Pip-Boy take no other key. Shift (whole stack) and Shift + right click on the
Pip-Boy's ITEMS page (drop the stack) still move the
stack without asking [design]. Over the Pip-Boy the prompt is drawn centred on the window
[hypothesis: the original draws it on the Pip-Boy's screen; the screenshots show it
centred over it]. Smoke scenario `item-stacks`; `NVF_PLAY_TEST_DROP=<s>:<item>:ask`
leaves it open for a screenshot.

**Drawn from its file (T15a).** With `[UI] TileMenus=QuantityMenu` the prompt of the
container, barter and inventory menus is drawn from the game's `QuantityMenu` tiles
(`engine/nvf-play/src/tiles/menus.rs`), the code above not spawned once the tree has the
file (the frame the prompt opens draws it by hand once; a missing file keeps it hand-built);
the crafting menu's prompt stays drawn by it. With `[UI] TileMenus=ContainerMenu` alone only
the prompt over a container menu actually drawn from its tiles is (§8) [design]: a container
menu that fell back to its hand-built drawing, and the barter and inventory menus, keep the
hand-built prompt. The tree builds the file while the prompt
shows, whatever the switch (`Game::ui_prompt`, as the open menus build; MenuMode does not
report it): with neither switch on its tiles are readable by scripts but nothing writes or
draws them. The code writes the meter's `user0`
(the stack) and `user2` (the amount) and the step sizes `user3`/`user4` (1 and 10, ours)
[documented: the file's comments on them]; the file works out the meter's value, its
fill (the tick picture tiled over the value's part of the meter: one picture, no piece
per tick) and the amount's text. The words are the game strings `sHowMany`, `sOK`,
`sCancel` and the key hints `sPCMenuHintA`/`E` (menu-xml.md §2.4; NVForever's own labels
when the load order has none). A pointer click on OK, Cancel or an arrow does what the
keys do; a press on the meter, held as it drags, sets the amount under the pointer; the
keys stay the ones above (the file's bindings act for a script's MenuTapKey and for the
controller's buttons that stand in for no key). Smoke `tiles-quantity`,
`tiles-container-quantity-alone`; the drop aid's
`ask` now holds the prompt open until it is answered.

## 8. The container menu from its file (T15c)

Behind `[UI] TileMenus=ContainerMenu` (`engine/nvf-play/src/tiles/container_menu.rs`,
`tiles/list.rs`, `tiles/item_card.rs`, issue #1468) the open menu (1008) is drawn from
`menus\container_menu.xml` and the hand-built drawing gives way to a sheet taking the pointer
at its stacking level (100). The model, its keys and the wheel stay the hand-built menu's
(§5). The prompt draws over it from `QuantityMenu`'s file, with no switch of its own, or by
hand when that file is missing or short of a path (§7) [design], and takes the pointer alone
[documented: `&no_click_past;`]. Smoke: `tiles-container`, `tiles-container-quantity`,
`tiles-container-quantity-alone`, `container-default-drawing`. Not the default; what still differs from the original closes this
section.

The original's values [observed: the private `hud_container_*` capture at 1280 x 720, read
with JIP on a footlocker; interface units; parts a to g, with their shots in the latest run]:

- **The rows**: a row is `height` = its text's and 20: 32 and 52 for one line, 67 and 87 for
  two, 102 and 122 for three. `_y` the sum above it. `_selected` stays 0 on the selected row.
  `target` 1 on the rows of both lists. Text x 40, y 20, wrap 370, font 2. A row's tile has
  its item's name, without the count (`GetActiveUIComponentName`); the tree's order is not
  the list's, `listindex` is.
- **Both lists have `_enabled` 1** at every read, against the prefab's comment. The list
  without the selection reads `_highlight_y` -1 and `_selected_height` 0 and shows no
  highlight; so does a list when the pointer leaves all rows (no selection at the opening
  either: `sel none`). The highlight box is 1 less than the row high, at the row's `y` after
  the scroll (a row at `_y` 208 with value 4: -23.11), hudmain at brightness 128, alpha 40
  (message-menu.md).
- **The scrollbar**: `_number_of_visible_items` is always 1. A list that fits reads 1 item; a
  right list with nothing filtered read the file's `_scroll_delta` 26, one with 15 of 16 rows
  filtered 57.2 (below, "Heading, filter"). Longer ones:

  | rows | total height | list | `_number_of_items` | `_scroll_delta` | `_jump_size` | marker height, `_step_height` |
  |---|---|---|---|---|---|---|
  | 15 | 885 | 365 | 10 | 57.78 | 6 | 33.30, 33.30 |
  | 16 | 937 | 365 | 11 | 57.20 | 6 | 32, 30.10 |

  [hypothesis] That is: the value's range is what overflows the list over the rows' average
  height, rounded up (885 less 365 over 59 is 8.8, so 9), and `_scroll_delta` what overflows by
  that range, so a value's move is not a row's. It fits all 15 lists read in five menus, which a test in `tiles/list.rs` holds; the
  rows that fit whole from the top, the first reading here, give these two as well but one
  too few on a list with a row of two lines in the Pip-Boy's inventory. A row's `y` is `_y` less value x `_scroll_delta` (-231.11 at value 4 of 57.78), by
  the file [documented].
- **What moves it**: `ClickMenuTile` on the down arrow: +1, kept every frame after; on a page
  area: 6 up to the end (4 to 9, 8 to 2). The rows move under a resting pointer, and the row
  now there takes the highlight in the original. `SetInCode` read 0 through those.
- **The clip**: at y 390 pixels the pointer is on a row, at 399 on nothing: the list's bottom
  is at 525 units (393.75 pixels). So `clipwindow` crops the pointer too, as the drawing's
  reading has it (menu-xml.md §3.1.1).
- **The alpha of containers**: the scrollbar image reads `alpha` 0, its marker 255, and the
  arrows show [observed: launchtest shot]. `CM_TakeAllButton` and `CM_ExitButton` are images
  and `CM_Subtitle` a rect with `alpha` 0 from `text_box.xml`, and the words show.
  `StrengthReqInfo` at the opening is shown at `alpha` 0: its words do not show, and its top
  line and fading side line do, at about (782, 447) to (912, 470) pixels [observed: launchtest
  shot]; its value text copies its `alpha`, its lines their own `_line_alpha`. So in this
  menu an `alpha` is not passed down to children [hypothesis: elsewhere].
- **The backing**: `CM_Background` `alpha` 193.8 (the file's global is 32), 1144 x 970,
  brightness 128.
- **The card** (`CM_ItemData`): `_Value` and `visible` per card; `--` for no weight; DAM's
  `alpha` reads 0, 8.25, 98, 121.5, 238.75 and 255 between reads of weapons, DPS's title 255,
  both at one place by the file, so it seems they fade into each other [hypothesis]. STR
  `alpha` 255 once anything was selected. `AmmoInfo`'s `_Title` the ammunition `name (n/m)` or
  armour's class, `_Value` empty. `ConditionInfo`'s `user5` the condition, `user6` 1 for
  weapons, 0 for armour; its `CNDArrows` x 140.5 (114.33 with `user6` 0), y 15, 16 x 20 once
  selected, y 17, 16 high before. After the pointer leaves all rows the card keeps its last
  item while the icon and arrow hide.
- **The picture and the arrow**: `CM_ItemIcon` `filename` the item's, a missing one
  `missing_image.dds`; `CM_ArrowIcon` `user0` 1 (the left list's row), 2 (the right's), hidden
  without a selection.
- **The buttons**: Take All 116 x 47 at -116, 0 of `CM_ButtonRect`, Exit 68 x 47 at -68, 45,
  hints "A)" and "E)" [observed: launchtest shot, which reads "Take All A)", unlike §3's
  earlier reading "R)"]. `ClickMenuTile` on Take All takes all and closes, no prompt, no
  message after. The weight label "Wg   52/200", three spaces. NVForever writes the
  setting `sInventoryWeight`'s word there and in the heading ("Weight   52/200" without a
  plugin's text), and `sInventoryCaps`'s for the caps [design, #4495].

What the tile path does with these, and where it differs:

- **Follows the file and the capture**: places, rows, counts, scroll, highlight and its fill,
  bar pieces, backing alpha 193.8, every tile's own alpha, card fields, arrow, icon.
- **Stays the hand-built menu's on purpose**: a row always selected in a list that has rows
  (no "no selection" state); the pointer leaving keeps the selection; the wheel moves the
  selection [owner 2026-10-22, #1192], a list moved under a resting pointer does not take the
  highlight, hover does not scroll [owner's playtest list 7, item 11]; more than five asks
  how many [owner 2026-10-15]; Take All closes [owner, list 5 item 60]; DPS a card above DAM,
  no fade [owner's playtest list 7, item 25]. The keys: physical A focuses the player's list
  and R takes all, [design: `crate::menu`, tagged there as the owner's screenshot's "R)" and
  our own keys, no owner decision]; the file's A acts only for a script's MenuTapKey.
- **The words on the tiles**: the hints come from the strings table, as every key hint of the
  files does: Exit's `sPCMenuHintE` from the file, Take All's `sPCMenuHintR` written in code for
  the key that works; a GMST wins, else `fnv_ui::strings::BUILT_IN`'s label in its bracket form
  (so not the original's or the hand-built menu's letter and bracket). The prompt over the menu
  reads the same way, from `QuantityMenu`'s file: `sPCMenuHintA` and `sPCMenuHintE` after `sOK`
  and `sCancel` [design] (the hand-built prompt, kept for the frame it opens and for a fallback,
  keeps its own letters). The cards' titles read
  settings no plugin defines [observed: menu-xml.md §2.4], so the file shows `BUILT_IN`'s
  labels, and the tile menu leaves them as they are [design: the string rule, LEGAL.md; the
  short words the original shows on its cards are not in the data files and are not
  transcribed]. A plugin's setting, a mod's file or a script sets its own title. Those labels
  do not fit a card (`ConditionMeter` stands 62 in and a title 5 in [documented]; the label of
  the weight measures 72 units [observed: our shot]); how the card makes room for them, or
  what shorter words NVForever writes of its own, is open. The hand-built figures
  (`pipboy/items.rs`) keep their words, which this menu does not change.
- **Heading, filter**: a click on a heading steps its category on, as its right arrow
  [observed: `container__category_heading_click`, 2 to 3]. `_num_filtered` is written, the rows
  the category leaves out [observed: 10 of 15, 12 of 15, 15 of 16]; the original keeps those
  rows' tiles at `listindex` -1 with stale `y`s and this menu unloads them [design], so a mod
  looking for a filtered row's tile finds none. A list that comes to fit keeps its last
  `_scroll_delta`, and has 26 only before it ever overflowed: [observed] once, 57.2 with 15 of 16
  filtered; [hypothesis] for every list, from that one case.
- **The scroll follows** the selected row on a key's or the wheel's move (also one ending on the
  same row) and when the row's place, the list's height or the scroll's scale (a wrap below the
  row) changes under a key's pick; never on a
  pointer's pick [owner's playtest list 7, item 11], and the bar and marker stay free otherwise.
- **The weight** cuts off to whole units [observed: 289.50 reads "Wg   289/200", 49.50 "Wg
  49/200"]; the hand-built label rounds (`look::half_figure`), left as it is.
- **Against the original's shots** [observed: pixels at 1280 x 720, the file's numbers drawn;
  bounding boxes of the amber above a threshold, the same item's card on both sides]: the card's
  lines 641-774 and 781-914 at y 476 against 642-774 and 782-914 at y 477; the picture
  307-426 x 482-571 against 309-426 x 483-571 (`CM_ItemIcon`, 256 square at &scale;, 22 and 472
  from the half: places and size agree); the arrow 626-669 x 192-287 pointing left, 632-674 x
  191-284 right; Take All's words 953-1052 x 512-532 in both; the headings and the weight within
  1 pixel; text colour (198, 141, 49) against (198, 142, 51); list rows, the wrapped row's 66
  pixels, the clip at the list's foot and the highlight box 704-1024 agree. The empty container's
  STR lines 781-913 x 447-470 agree. A bar shows only on a list that overflows, on either side, in
  both.
- **Still different, and why**:
  - The backing reads darker here against the world; the world differs between the shots too,
    so 193.8 stands unchecked. The world now blurs behind the menu (image-modifiers.md, issue
    #1484); these shots were taken before it did.
  - The picture and the fading side lines are brighter here, in both our drawings, one
    picture peaking at red 227 against 197 [observed]. 197 / 254 through the
    sRGB curve is 227 / 254, so the picture's mid-tones may be read as linear [hypothesis]; a
    matter of the shared texture path, for every interface picture, not of this menu.
  - The opening shows a selected row, its highlight, arrow, picture and card; the original shows
    none until the pointer is on a row (`sel none`, `_highlight_y` -1), and hides them again when
    it leaves. This menu keeps a row selected (above); the keys act on it. A question for the
    owner, not changed here.
  - Take All on an empty list is dimmed, its `target` 0; the original's words stay at full
    brightness and the click closes the menu (`container__empty_takeall`). Empty Take All not
    closing is the hand-built menu's rule (§5), and the dim says so.
  - DPS a card above DAM, no fade [owner]; the original's DPS stands in DAM's place, as the shots
    show.
  - Words: the hints, as above.
  - DAM/DPS's fade, `_wheelmoved` and the marker's own drag traits are not built. The tile path's
    `ConditionMeter` and `CNDArrows` follow the numbers read (meter 104.67 wide, image the
    condition's part, arrows x 140.5 or 114.33, y 15, 16 x 20) and are not yet compared by eye:
    `tiles-container`'s `card_*` shots hold them (a weapon, armour, an aid item).


## 9. The barter menu from its file (issue #1468)

[design: `engine/nvf-play/src/tiles/barter_menu.rs`, `tiles/vanilla.rs`] With
`[UI] TileMenus=BarterMenu`, the open barter menu (1053) draws from
`menus\barter_menu.xml`. The hand-built view stays the default. The adapter writes
nothing with its switch off. A missing file, missing required path, or row template
without its name, price or marker child keeps the whole hand-built view. The writer's
single decision supplies drawing, input, and the prompt's drawing; the modal sheet
stands at the hand-built menu's level, 100, beneath its prompt at 101.

[design: `tiles/item_menu.rs`, `tiles/container_menu.rs`] The first commit extracts the
container's row reconciliation, template validation, text measuring, row placement,
counts, scroll, highlight, marker drag state, click meanings, row guards, and alpha/clip
marks. Both adapters use that state with a table of paths and click meanings. The
container's snapshot constructor, weight, subtitle, arrow and Take All writes remain
separate, as do barter's caps and buttons: putting those in one conditional writer
would obscure their different purposes. The container's existing synthetic tests stay
in `container_menu.rs`.

[design: `tiles/input.rs`, `menu.rs`, `menu/model.rs`, `ui_sounds.rs`] Every row,
heading, Accept and Exit action goes through `MenuInput` and `apply_input`, including
its sounds and events. At most one row click acts per frame; a guard checks the row's
identity again when queued commands apply. The pointer selects only after moving,
does not scroll the list, and yields to the wheel's held selection. The wheel still
moves the selected row, or the category over a heading. Physical A accepts and X exits;
the arrow keys change sides, W/S and Up/Down select, Enter/Space act, brackets change
category, and Page Up/Down move the selection. Controller mappings remain `pad.rs`'s.
The adapter suppresses a second tile action for keys already handled by the model;
a script's MenuTapKey still follows the file's bindings. Exit sends only Close, so
it cannot bypass the model's prompt cancellation or menu closing rules.

[design: `tiles/barter_menu.rs::Snapshot::of`, `menu/model.rs::refresh_trade`]
Prices use the model's existing `barter::price_text`, including `--` for no worth
and a tenth for a price below one cap. Caps come from each side's purse. The model
moves an offered row to the opposite list, first, and the adapter shows an open square
for it, a filled square for an equipped row. Accept is dim and untargetable with no
items offered, bright with an offer, including a zero-net trade. The flow is hidden
with a zero balance. A positive balance pays the player and uses
`barter_arrow_sell.dds`; a negative balance uses `barter_arrow_buy.dds` [documented:
section 1 names the two textures]. The amount is what the trade moves
(`Trade::paid`). The flow's picture and texts dim when the payer cannot afford it;
Accept still sends the model's command, which refuses only a player who is short
[design: the maintainer's request in section 5 for the dimming].

[observed 2026-11-06: private job `ov_barter`, the original at 1280 x 720,
Chet, read with JIP] `BM_CapsFlow` is invisible (0) with no trade, visible (1)
with one, at y 260. Accept's `target` is 0 and 1 the same way. The list without
the selection reads `_highlight_y` -1 and `_selected_height` 0; the other reads
the row's `_y` and height (52, or 87 for two lines). Titles read x 50, y -10,
height 38, font 6. `BM_Items_CapsLabel` reads x 449, y 20, height 32, font 2,
justify 4. These match the adapter and the file's layout; no title or purse-label
geometry writes are needed [design: `tiles/barter_menu.rs`, `tiles/list.rs`].

[observed 2026-11-06: private job `ov_barter`, the original at 1280 x 720,
Chet, read with JIP] Selling, with caps coming to the player, reads
`_ArrowDirection` 2 and `BM_CapsFlow/x` 830.33; 1 or no direction gives x 810.33.
The purses show the setting's Caps word, three spaces, and the number (`Caps   900`).
The normal flow alpha is 255; a sale beyond the merchant's purse reads 64 and dims
its icon, amount and Caps word. [documented: `barter_menu.xml`, read privately from the user's
install] Direction 1 selects the icon's `_filename_1`
(`barter_arrow_buy.dds`), direction 2 its `_filename_2` (`barter_arrow_sell.dds`),
through successive copies of the direction and `_filename_`. The flow, icon and
amount shift conditionally for direction 2; the flow's alpha is set in code and
the icon's alpha copies it.

[design: `tiles/barter_menu.rs::write`, `crates/fnv-ui/src/tree.rs::eval_reading`,
`expr.rs::ops_of`] The writer now uses direction 2 for positive balance, 1 for
negative balance and an empty string for zero. A copy replaces the preceding value,
including a leading string; after a number, a copy of a trait ending in `_` looks up
that trait with the number appended. The tree therefore selects the icon's filename;
the adapter no longer writes it. `BM_CapsFlow/_Value` receives the unsigned amount
and the amount text is also written. The writer sets flow alpha to 255 or 64 and
writes that same alpha to the amount and Caps text, since `item_menu::mark_tiles`
marks every tile to take only its own alpha. The icon keeps the file's parent-alpha
copy. None of these three children receives a brightness write. Accept's existing
128/255 brightness writes remain separate.

[observed 2026-11-06: private job `ov_barter`, the original at 1280 x 720,
Chet, read with JIP] `_IsContainerListSelected` is -1 on opening without a selection,
0 with a player row selected and 1 with a merchant row selected; leaving all rows
retains the last value. Moving the pointer onto a row of the other list selects it
and moves focus (`left_selected`, `right_selected_cursor`). [documented:
`barter_menu.xml`, read privately from the user's install]
`BM_Items_FocusRect` (id 0) and `BM_Container_FocusRect` (id 1) are hotrects with
targets following the merchant-focus trait and its inverse. The lists' `_enabled`
follow its inverse for the player and its copy for the merchant. This differs from
the container file's two enabled lists in section 8.

[design: `tiles/barter_menu.rs::write`, `pointer_candidates`, `tiles/input.rs`,
`tiles/item_menu.rs`, `tiles/list.rs`, `menu/model.rs`] The adapter retains the last
side whose snapshot has a selection, starts at -1 when neither has one, and clears
that memory on close. The current model preselects row zero and cannot distinguish
an untouched opening with rows from an explicit selection; the adapter uses that
snapshot selection, so such an opening still reads 0. No model selection rule changes
in this review. `_enabled` is a file trait; row `target` expressions use it, and the
generic pointer reads `target`. The two focus rects now have click meanings. When
one takes the pointer, input resolves hit candidates with that side temporarily
enabled and restores the written focus before stepping the pointer. Thus the row
under it supplies the normal guarded Hover or click, even on a direct click across
sides. Clipping and higher menus still govern hits, and empty space or the wheel's
held selection cannot leave the tree's focus ahead of the model.

[observed 2026-11-06: private job `ov_barter`, the original at 1280 x 720,
Chet, read with JIP] A sale beyond the merchant's caps holds the displayed amount
at that purse, dimmed; Accept still takes it and completes at that amount. The
merchant's 815 caps become 0 and the player's 900 become 1715. Both menus do the same
[design: `Trade::paid`]; they used to refuse it. Filtered category headings remain NVForever's own words
under LEGAL.md; the capture's category wording has no source in the data files.

[hypothesis: container measurements in section 8; comparisons still outstanding]
The shared counts formula, retained scroll delta, row measuring and spacing, clip
handling and backing alpha 193.8 remain borrowed from the container. The card uses
`item_card.rs` unchanged, including the container's condition-arrow y 15 and height
20 and the owner's separate DPS placement; those numbers still need comparison.
No row width, wrapping width, button place or arrow offset is copied into the adapter:
those remain the file's [design: `tiles/barter_menu.rs`].

[design: `tiles/menus.rs`, `tiles/mod.rs`, `menu.rs`] The quantity prompt follows an
actually tiled barter view even with only BarterMenu switched on. With a missing or
short QuantityMenu file it stays hand-built, including the frame before its tree
builds. Either prompt blocks the underlying rows, headings, buttons and marker drag.
A barter view that fell back does not implicitly turn its prompt into tiles; the
explicit QuantityMenu switch still works independently.

[design: LEGAL.md, `tiles/barter_menu.rs`, `tiles/item_card.rs`] String audit:

- All-items heading: `sInventoryItems` through `ui_loading::game_string`, with the
  load order's GMST winning over `fnv_ui::strings` (whose label reads "ITEMS", in capitals
  as NVForever's own hand-built Pip-Boy headings, menu-xml.md §2.4, #2212). Merchant heading: the model's
  record or script name. Filtered categories use our own words: Weapons, Clothing,
  Consumables, Other items, Ammunition. `look::half_title`'s transcribed uppercase
  headings are replaced before anything is written.
- Purse labels and the flow label: `sInventoryCaps` through the same setting lookup;
  the three spaces and purse digits are formatting. `look::half_figure` is not used.
  Amounts and prices are model numbers, with `--` as our no-value notation.
- Row names: the model's record or script names, its mod marker and count formatting
  (`look::row_text` formats only the name and count). The model's separate condition
  suffix helper is not used by this view. No category abbreviation is transcribed.
- Item-card titles, Accept and Exit: left to the files' settings, with the strings
  table's own fallback labels (`sAccept`, `sExit`, and the inventory card settings).
  Hints written by the adapter: `sPCMenuHintA` and `sPCMenuHintX`, adjusted by
  `pad::game_hint` for the controller's existing mapping and label style.
- Card values: `look::item_figures` supplies numbers, condition and one-unit weight
  and value. `pipboy::items::info_of` supplies ammunition names from AMMO records,
  clip/carried numbers, armor-class enum labels (our functional words), and effects
  named by AVIF ANAM/FULL or the engine's actor-value names. `item_card::write` uses
  the DAM/DT identifiers only to choose cards; it leaves their visible titles to the
  settings. Empty strings clear absent data; `--` is our no-value notation. Icon,
  marker and flow filenames identify installed assets, rather than displayed wording.

[design: synthetic tests beside `tiles/barter_menu.rs` and `menu/model/tests.rs`]
Tests cover short files and missing price children, prices/purses/open and filled
marks, unit card figures and untouched titles, both flow directions, insufficient
funds, a zero-net trade, empty Accept, child click meanings, stale rows/guards,
scroll rewrite, pointer picks across disabled lists and focus rects, retained focus
and its reset on close, rendered flow alpha, physical/script key arbitration, prompt fallback,
and a real model sale appearing first on the merchant's side before acceptance.
The fixtures use their own dimensions and expressions, never the game file's (#1512).

[design: `tools/smoke/scenarios.toml`] New scenarios: `tiles-barter`,
`tiles-barter-quantity` (BarterMenu alone, including Cancel and blocked underlying
clicks), and `barter-default-drawing`, through Chet's dialogue in GSGeneralStore
using section 6's talk/barter aids and the currency/pistol recipe. They take 1280x720
shots of empty Accept, offered and returned items, the caps flow, acceptance, category,
scroll and quantity states. The automatic trade aid is delayed beyond the runs so
that the actual tile or hand-built input makes each trade. The input aid now names
Page Up/Down and the bracket keys for those checks. Pointer places and timing are
hypotheses borrowed from the container runs. The file has no
subtitle tile, so the model's own refusal note stays as a small hand-built overlay
at its existing place (`menu/look.rs::spawn_note`), alongside the dimmed flow
[design: the contract's paths and `menu::note`]. Those words are the model's own
functional error message "You don't have enough caps." (`menu/model.rs::accept`),
or the engine's
"Cannot..." errors (`plain_error`). They are not transcribed lines. This piece remains separate to
preserve the feedback without inventing a tile in the file. Blur uses the existing
tile-menu blur path at radius 3
[hypothesis for barter: the same shared presentation as the container].


## Android alternate touch presentation

[design: issue #2436, Android batch after qualified R9]
An optional compact touch sheet presents the existing barter model, independently
of whether its ordinary drawing selected native tiles or the hand-built fallback.
This is an alternate mobile presentation, not native-file widget touch support.
Both inventories have separate category/scroll controls and rows at least48 logical
pixels high. A row tap only selects. Explicit One/All controls stage or take back
units, the existing quantity prompt asks for large stacks, and only explicit Accept
commits the existing trade. Cancel keeps the ordinary menu's Close semantics.

[documented: menu/model.rs, menu.rs::apply_input, menu/barter.rs]
The touch producer emits existing guarded MenuInput commands. Existing prices,
merchant services, quest/live filtering, condition/mod identity, caps limit,
quantity rules, script callbacks, sounds and item-transfer effects remain the
authoritative consumers. No direct transfer, Use, invented discount or relaxed
eligibility is introduced. Before an accepted touch action, the real inventory is
checked again; changed rows or trade totals require a fresh selection/review.

[design]
The same logical rectangles paint and hit the sheet, clipped to the parent's
current safe window and explicit full-window UI camera. A scrolling contact cannot
stage an item or accept a trade. Sticky travel, one contact, guarded release and a
same-batch commit barrier prevent row/category/quantity/Accept click-through.
Quantity changes alone move no items. Small/invalid windows fail closed and retain
the ordinary presentation rather than creating invisible targets.

[design]
KeyboardMouse remains the existing shared device identity for both finger and mouse.
The parent observes actual physical mouse/navigation intent once: it hides/retires
the sheet without altering hardware buttons, and the ordinary mouse/keyboard/pad
consumers keep their behavior. A fresh touch restores the sheet while consuming
that contact; the existing consumed controller-return touch restores presentation
without an extra action or an extra restoration tap. The parent alone reads raw
TouchInput, applies selector/platform/focus/modal admission and prioritizes barter
over the underlying conversation. Original hand-built touch picking is filtered
only for this sheet's owned/retired contacts, never for unrelated menus.

[hypothesis: qualification plan]
Authored App tests must run the actual MenuInput/apply_input consumer in both
native-selected and fallback configurations, including both inventories, quantity,
category, insufficient caps, changed row/trade, cancellation, source handoff and
no duplicate actions. Source review, compilation, CPU execution and phone/readability
acceptance are separate evidence; none is implied by this design note.

[design]
At the command boundary the adapter rechecks current safe insets, UI scale, window
density/size/focus, full-window camera, device/platform admission, keyboard capture,
free camera and fresh-menu state. A stale row, quantity session or quote is refused.
Retired contact IDs are bounded at 32; exceeding that budget abandons proposals and
captures until an actual neutral touch frame, without clearing hardware input.

[design]
Touch dispatch also checks the runtime's current top menu and active character
creator. Barter and its own Quantity publication are admitted; an unrelated script
modal opened after raw input retires the touch proposal. These checks apply only
to this touch producer; ordinary keyboard/mouse/controller consumers are unchanged.

[design]
Accept and quantity controls use a one-shot internal command envelope. The existing
MenuInput consumer resolves it immediately before its ordinary mutation, checking
the exact current basket/quote or quantity row and confirmed amount. Earlier queued
menu commands cannot change what a touch confirmation commits. One typed guard is
retained; rejected, consumed or canceled tokens do not replay. Ordinary commands
and their sounds, callbacks and effects retain the existing execution path.

[design]
The same consumer context check covers selection, staging, scrolling, category and
Close, so a later modal cannot receive a touch intended for the underlying trade.
Scroll resolves a fixed Focus/Scroll pair through the original effects body with no
interleaving. An obsolete token cannot discard a newer live guard; only its matching
token is consumed, including when an existing row guard drops that command.


### Ordinary-menu touch and independent typography

[design] The optional finger route keeps the existing native or hand-built
buying/selling presentation. Its independent menu size changes the existing
font metrics, wrapping and row measurement within the same viewport; the painter
and hit geometry use the same current size. The default 100% retains the prior
row law. Hand-built rows keep at least their original 51-unit height so markers
remain inside adjacent rows; an XML row's larger explicit height is retained.
A changed effective saved size cancels a held proposal, including A-to-B-to-A;
setting an equivalent quantized value preserves it. Other ordinary prompts keep
their existing size; the actual barter quantity prompt follows barter size.

The touch adapter compares the actual painted size, interface-unit scale,
window/camera, insets and live source before the original consumer acts. It does
not zoom the entire canvas or synthesize a mouse click. Quantity-meter dragging
uses the composed original Amount consumer and preserves these size/source
guards, including acknowledged same-geometry repaint. R41 compiled these adapters
and passed all thirteen ordinary barter/typography controls. Its nine quantity-meter
cases failed; R42 subsequently passed all nine original cases. R43 passed all
thirteen, including the adapted context observation distinguished below.


## Ordinary touch quantity meter

[design] The ordinary barter prompt's meter supports a held finger, as its
existing mouse meter does. Start and movement propose only the amount under the
current painted meter, clamped at its ends even when the owned finger leaves
its footprint. A separate Confirm still stages the quantity; a separate trade
Accept still moves inventory/caps. Lists keep their existing browse-only drag.

[design] Every changed sample uses the existing one-shot MenuInput consumer and
its exact quantity/row/trade/frame/source guards. Only an acknowledged original
Amount application updates the held contact's expected quantity. Its own
hand-built redraw may replace the meter entity once at the same geometry;
unrelated replacements and native tile-ID changes do not inherit the owner.
Foreign quantity edits and cancel/reopen in one command queue retire the old
contact. Same-value samples coalesce without dirtying the view. Raw cancellation
discards an unapplied sample and retains the last applied amount; it does not
confirm or undo a previously applied meter edit. Secondary fingers cannot press
Confirm for the owning finger. Hardware, modal, window and settings priority
retain their existing cancellation/neutral/return behavior.

[design] Continuous native samples are guarded Amount commands, without physical
input synthesis or PointerState writes. A stationary native End retains its real
native click pulse only when its endpoint still hits that same meter. A subslop
End just outside it remains an amount clamp, never a click on a nearby button.

[design] Authored Apps exercise actual hand-built Taffy/fill
geometry and the existing native QuantityMenu writer/amount text, actual raw
input and original apply_input, including density/nonunit Bevy UiScale and a
large-stack short edge crossing. Native prompt XML is authored test data; it does
not establish full game-file/class lifecycle, GPU pixels or physical-phone acceptance.

[qualification] R41 source `a18b10ae`, actual checkout `54f8997b`, compiled and
executed all nine meter cases, which failed. Eight native cases reached missing
drawn-meter geometry in their shared fixture; the physical hand-built control
applied amount 2, then failed the next drag expectation of 6. The corrected native
fixture retains a drawn meter parent with its original writer/text identity and a
full-size hit-testable child. All nine complete case bodies and their effect
assertions remain. R42 source `e5b00f0b`, actual checkout `56793aa7`, executed
these corrections and passed all nine original cases. This is a combined-source
CPU result, without physical-phone or full original-menu acceptance.

[design] Only a hand-built Barter meter participates in physical leaf retention.
Its original Amount consumer must acknowledge the exact requested change before
the painter can reuse the already-Pressed leaf across that amount-only redraw.
The full painted prompt, row/trade/status, font size, window/camera, density,
interface scale, insets, original Node style and first accepted geometry must
still agree. Another command, prompt replacement or changed context retires that
leaf; an equivalent saved size preserves it. The first accepted geometry is kept
through the held chain rather than reconstructed after each redraw. This creates
no new raw input reader, PointerState writer, quantity effect or input owner.

[design] Ordinary meters retain their existing PointerState/RelativeCursorPosition
admission without a retention component. Additional geometry is optional for that
route and required only by known painted retention. Partial-window fresh clicks
retain the original consumer without promising retained dragging. Container and
Inventory meters keep their original route; changing only Barter size cannot
retire their unchanged source. A separate Confirm and trade Accept still own the
effects. One retained leaf and its paint snapshot are bounded; no FPS improvement
or allocation reduction is claimed.

[qualification] Four additional App cases cover competing commands, context and
geometry retirement with fresh recovery, partial-viewport original clicks, and an
unrelated Container control. The existing component-only Inventory switching and
release case is unchanged. R42 executed all thirteen: twelve passed and the new
context case failed an interactive-hit witness while observing deliberately
translated, invalidated geometry. The reported right-edge hit was the actual
neighboring amount-step arrow. The follow-up reads computed geometry for only
that transient change observation; the four hit witnesses remain at actual meter
samples and fresh recovery, with all thirteen context causes and later refusal,
hardware, amount and effect assertions retained. This explicit fixture-observation
adaptation was unexecuted at that checkpoint. R43 source `1c1b96bb`, actual
checkout `a53b0978`, subsequently passed all thirteen cases. This is an actual CPU
result for the combined source; production input and quantity effects are
unchanged, and physical-phone acceptance remains open.
