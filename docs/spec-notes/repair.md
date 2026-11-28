# Spec note: repair

## Android touch presentation (source increment)

[design] Android uses a compact maintenance sheet for player donor repair and
vendor repair. It is an alternate mobile presentation of the existing sessions,
not a claim that touch operates the original menu-file widgets. The same sheet
works when the existing native-file presentation is selected and when its
hand-built fallback is selected. Existing keyboard, mouse and controller routes
and the repair formulas in this note remain authoritative.

[design] `touch-maintenance-v1` is a saved Menus switch, on by default on Android
and off on desktop. Turning it off restores the ordinary maintenance presentation;
changing it cancels pending sheet actions and retains owned contacts until neutral.
Invalid presentation metadata is refused before drawing: logical dimensions and
48-pixel targets must remain finite after UI-scale conversion. Supported normal
scales are unchanged; fresh valid metadata can rebuild the sheet.

[design] A touch selects a row without consuming anything. A separate confirmation
shows the selected target, donor and resulting condition, or the vendor's quoted
condition/caps/skill requirements. Cancel changes neither equipment nor caps.
Scrolling changes the visible list without choosing a different donor or item.
Confirmation carries the painted item/unit/quote identity and is checked again
inside the existing action consumer before any inventory mutation. A changed
selection, missing item, changed quote, modal, source or input owner rejects that
old confirmation. Repeated delivery must not consume a second donor or charge
twice.

[design] The parent Android input coordinator supplies the current full-window UI
camera, logical viewport and safe insets and remains the sole raw touch reader.
The sheet never writes hardware button state. Real hardware intent hides/cancels
the touch sheet; returning touch restores it with an action-free contact. An
already-consumed controller return restores it without costing an additional tap.
The touch surface owns its entire panel and closing drain, so its contacts cannot
also act on the underlying native, hand-built or dialogue surface.

[design] This source increment requires actual App controls for donor consumption,
vendor charging, refusal, cancellation, late refresh and hardware handoff in both
presentation configurations. Authored or merely compiled controls do not establish
runtime or Android device coverage.

Tags as in `nif.md`: `[documented]` cites a page or file, `[observed]` says what was
looked at, `[hypothesis]` is a reading that nothing has confirmed yet. `[design]` marks
what is NVForever's own, `[owner]` a rule the owner set.

Two ways to repair a weapon or a piece of armour: a vendor does it for caps (the repair
services menu, §2–§4), or the player takes parts from a like item in the Pip-Boy (§5).
Code: `crates/fnv-game/src/repair.rs` (the rules, `ShowRepairMenu`),
`engine/nvf-play/src/repair.rs` (the vendor's menu), `engine/nvf-play/src/pipboy/repair.rs`
(the Pip-Boy's), `crates/fnv-game/src/data.rs` (`GameData::repair_lists`),
`crates/fnv-sim/src/inventory.rs` (`Inventory::set_condition_one`).

## 1. Sources

- **The owner's screenshots** of the original, 2026-10-17, kept privately: eight of the
  repair services menu (C:\fnv-private\repairmenu, 1920 x 1080, a vendor with Repair
  100, the player with 18 caps, nine weapons listed) and eight of the Pip-Boy's repair
  (C:\fnv-private\repairmenuPlayer).
- **The game's interface files** in `Fallout - Misc.bsa`: `menus\repair_services_menu.xml`
  (the vendor's menu), `menus\repair_menu.xml` (the Pip-Boy's) and the inventory's
  `menus\main\inventory_menu.xml` (its Repair button), with the shared pieces they
  include (`list_box.xml`, `list_box_template.xml`, `card_info.xml`, `meter.xml`,
  `text_box.xml`, `bottom_bracket.xml`, `vertical_fade_line.xml`, `line.xml`). Described
  here in plain words; nothing of them is shipped.
- **The GECK wiki** (owner's export): ShowRepairMenu, GetMenuTargetRef, Armor, "Item
  Repair Formula", "Item Condition And Value", GetRepairList, Settings.
- **FalloutNV.esm**: the settings (GMST) and the in-game help (MESG `HelpRepair`,
  `HelpWeapons`, `HelpApparel`, `HelpPipBoyItems`), read with a private script.

## 2. Opening the vendor's menu

- `actor.ShowRepairMenu` (alias `srm`) "opens the repair menu so that the player may
  repair their equipment by paying caps. The repair skill of the calling actor will be
  used, and any caps spent will be added to that actor's inventory." Called without a
  reference (a quest script, the console) "nothing will happen". Called on the player,
  the caps spent come back to the player, "but they must have enough caps in their
  inventory before they can repair an item" [documented: GECK wiki ShowRepairMenu].
- The menu is MenuMode 1058 [documented: GECK wiki GetMenuTargetRef, "1058 The actor
  (vendor) whose skills are used for repairing"]. Scripts see it open until the player
  leaves it.
- Ten quests' scripts or dialogue call it, Raul's (`VDialogueRaul`) among them
  [observed: docs/design/script-function-coverage.md].
- From a dialogue line's end script (issue #2308): FalloutNV.esm has ten lines whose end
  script calls it; seven of them have no Goodbye flag (Mick's
  `VFreeformFreesideVFSMickTopic008`, Old Lady Gibson's, Major Knight's two, Calamity's,
  Dale Barton's, Samuel's at the 188) [observed: `fnvtool dialogue lines`]. Without the
  flag the conversation does not end with the line, so the menu opens over the
  conversation and the topic list comes back when it is closed [hypothesis: as the
  barter menu does; the original's return is not captured]. With the flag (Raul's, the
  safehouse paladin's) the conversation ends and the menu opens in play.

## 3. The vendor's menu

**What the file says** [documented: repair_services_menu.xml], in its units (960 to the
screen's height):

- The menu is a rectangle 1020 x 680 centred on the screen; behind it a black picture
  (`interface\shared\background\solid_black.dds`) 55 left of it and 115 above, 1134 x
  910, at brightness 128 and the shared background opacity (`_background_fill_alpha`,
  32 of 255 [documented: globals.xml]).
- **The shade** (2026-10-19) [observed: the owner's screenshot at 1920 x 1080,
  C:\fnv-private\references\crafting_repair\repair_services.jpg]: the world behind is
  blurred; no edge of a dark sheet shows. The frame's lines stand at x 388 and 1533,
  y 158 and 924 (1145 wide, 1020 in the file's units at 1080/960). Where the file's
  sheet would end (x about 324, 62 pixels left of the frame) the brightness runs
  smoothly (56, 48, 33 of 255 at x 300, 330, 360, y 950); just under the frame's bottom
  line the middle stays as dark as inside (8 to 23) because the shape behind is dark,
  so the darker look inside the frame is the scene, not a fill. NVForever draws the sheet
  at the file's 32 of 255 black (it was the other menus' 0.75, a hard box well past the
  frame). The blur is not built; whether the blur also darkens the world is not known
  [hypothesis, owner-review].
- The headline: a short side fading down at the left, a line 50 long, the title in font 6
  at x 70, 13 above the line ("Repair Services", the string `sRepairServicesTitle`), the
  line again from 20 after the title to a second short side; at the right, two labels
  in font 2 right-aligned 20 down: the vendor's skill, ending 15 before the second side,
  and the player's caps ending at 1005. The second side stands 35 left of the caps
  label (but not left of 860), a third side at 1020.
- The list: 15 left of the menu and 60 down, 480 x 580, the shared list box. A row (the
  template) is 32 in and 440 wide: the name in font 2 at 40, 20 down, wrapped at 260; a
  bar 60 x 16 at 370, 20 down, its back at alpha 64, its fill as wide as the condition
  (set in code); a filled square 25 x 25 at (15, 17.5) for an equipped item. The rows
  click silently (`&nosound;`); their alpha is set in code, "true if we have enough
  money and vendor skill to repair this item". With no rows, the text `sNoItemsToRepair`.
- The item's picture: 400 x 400 at (500, 0).
- The figures from (485, 380): the condition (`sInventoryCondition`) as a card 263 x 60
  with a meter 150 x 20 at 85; beside it at 273 the item's figure (set in code); an
  arrow `interface\shared\arrow\repair_arrow_down.dds` 80 x 80 at (223, 50); the gains in
  font 4 centred at 120 and 400, 90 down; the condition and figure after the repair at
  140 down; the cost in font 6 at 230 down. An error text in font 2 centred at (250,
  160) shows instead of the cards after the repair, "when an item can't be repaired,
  like if the vendor's skill is too low" (set in code).
- The buttons, right-aligned at 1010 from 75 down, 47 apart: Repair (on the Xbox only),
  Repair All (`sRepairAllItems`, PC key A, drawn at alpha 128 unless it can be used) and
  Done (`sDone`, PC key E).
- A bottom bracket at 680, 1020 wide: a line with short sides fading up.

**What the screenshots add** [observed]:

- The words: the title; the skill and the caps, each with its figure two spaces after
  it; Repair All with its total in brackets and its key hint; Done with its hint; the
  condition and damage cards; the gains ("+66 %", the percent sign small; "+3"); the
  cost with its figure. NVForever's hand-built page writes the settings the menu files
  name for the title, skill, caps and buttons (`sRepairServicesTitle`, `sRepairSkill`,
  `sInventoryCaps`, `sRepairAllItems`, `sDone`), and its own "Cost: N caps" [design,
  #4495: in place of the words read from the screen].
- The rows the player can have repaired come first and bright (Minigun, Sturdy Caravan
  Shotgun, Weathered 10mm Pistol, Mercenary's Grenade Rifle at 11, 11, ?, 1 caps), the
  others after them, dim at about two thirds (Straight Razor 32, Varmint Rifle 98, 9mm
  Pistol 130 caps with 18 caps carried; Broad Machete and Binoculars at full condition,
  "Cost: 0 caps"). Within each part the order is not by name or by cost; we sort by name
  [design].
- The Varmint Rifle is marked equipped (the filled square).
- "Repair All (285)" is dim with 18 caps; its figure is the sum of the rows' costs.
- The cost line is dim when the player cannot pay it.
- At vendor skill 100 every item's condition after the repair is full.
- The item's figure is DAM for a weapon (Varmint Rifle 8 → 11, 9mm Pistol 7 → 9).

### 3.1 The menu drawn from its file

Behind `[UI] TileMenus=RepairServicesMenu`, off by default, the menu is drawn from
`menus\repair_services_menu.xml` (issue #1468) [design: `engine/nvf-play/src/tiles/repair_services_menu.rs`,
the card pair in `tiles/repair_cards.rs`, in the shape of the barter menu, barter-menu.md §9].
Off, nothing here runs and the hand-built menu above is unchanged. The rules, the keys,
sounds, refusals and log lines stay `engine/nvf-play/src/repair.rs`'s: the adapter writes
what the model shows and sends what a click asks (`RepairServicesInput`). The world is
blurred behind it, as behind the container's, in place of the hand-built backing the maintainer
asked for (`top_menu_backing`, playtest list 7 item 2), which the default keeps [design].

**The words**, each with its source [design: LEGAL.md]:

| Tile | What it shows | Source |
| --- | --- | --- |
| `RSM_Headline_Title` | not written | the file's `sRepairServicesTitle` |
| `RSM_EmptyRepairListText` | only `visible` | the file's `sNoItemsToRepair` |
| Repair's, Done's `button_text` | not written | the file's `sRepairItem`, `sDone` |
| Repair All's `button_text` | the file's words, a space, the total in brackets | the file's `sRepairAllItems`, the model's figure |
| `RSM_VendorSkillLabel` | setting, two spaces, number | `sRepairSkill` |
| `RSM_CapsLabel` | setting, two spaces, number | `sInventoryCaps` |
| `_PCButtonText` of Repair All, Done | hint | `sPCMenuHintA`, `sPCMenuHintE` (a controller's button name, `pad::game_hint`) |
| a row's `ListItemText` | item's name and count | the record's name; "(n)" is ours |
| `RSM_RepairCostText` | setting, two spaces, number | `sInventoryCaps` |
| `RSM_ErrorText` | one sentence | ours (`repair::BEYOND_WORDS`), the original's is in its code |
| the condition cards' `_Title` | the file's; only where the file has none | `sInventoryCondition` |
| the stat cards' `_Title` | setting | `sInventoryDamage` for a weapon, `sInventoryDamageThreshold` for armour |
| the stat cards' `_Value`, the two gains | numbers, "+" and "%" | the model |

Words the hand-built menu shows that this one does not: "Cost:" and the "caps" after the
figure, on the original's screen alone [observed: §3's screenshots] and not in the strings
table [design: `crates/fnv-ui/src/strings.rs`], so the line reads the setting and the figure; the short card titles "CND" and "DAM" from the same screenshots,
for the settings above.

**A title wider than its place**: the file's meter stands 85 in, which leaves room for a short
word, and the label NVForever supplies for `sInventoryCondition` is longer. The meter then
starts 10 past the title and keeps its right end, so it is shorter than the original's while
that label is in use; a plugin's shorter setting leaves the file's place [design].

**What the captures said** [observed: private jobs run07_repair_services and
run07c_repair_services, 1280x720, read and in their pictures; the second has rows lit]:

- The meters: a row's `RSM_Template_Meter/width` is condition x 60, the back's width; a card's
  meter follows the card's `user0`, which is what is written (the file makes the meter's
  `_Value` copy it).
- `RSM_RepairButton` stays hidden on a PC, by the file, and Repair All takes its place. Its
  `target` reads 1 with no row and on a lit row, 0 on a refused or too dear one.
- Repair All shows the file's words, a space and the total in brackets, even out of reach.
  `target` is 1 when the caps reach the total, 0 otherwise and at a total of 0, and the file
  dims it by `alpha` from that; `brightness` stays 255.
- A row refused or too dear reads `alpha` 128 on its text, fill and square, `brightness` 255. A
  highlighted one stays dim. The cost line reads 128 when the caps fall short.
- The gains carry no space and show at nothing too ("+0%", "+0").
- After a repair the menu stays, and the row stays listed, dim, at the limit; repaired to true
  full it leaves the list. Repair All acts at once. Enter on the row repairs it.
- Two items at one condition are two rows there and no suffix shows; the model's one row of
  several (§4) is the maintainer's rule and stays. The cost line then says what one repair
  takes.
- The order of the rows differs: bright first, dearest first, then dim, cheapest first
  [hypothesis on the key]; the model's stays (§4).
- `RSM_Background/alpha` reads 193.8, as the container's: the file's 32 is replaced while the
  game runs. The arrow shows on a refused row too.
- The labels' gap is about twice a word space [hypothesis: two spaces].
- A selection the pointer leaves hides the cards and the picture in the original. The model
  always has a row selected, so here they stay [design: the model's rule].

**Still open** [hypothesis]: a pointer click on a row in the original; a long name's two-line
row; the sort's key for refused rows. The original's limit at a vendor's skill does not fit the
model's rule (§4's "How far"): that is the maintainer's to weigh, nothing here changes it.

## 4. The vendor's rules

- **What is listed**: the player's weapons and armour worn below full condition, one row
  per group of units at one condition ("Name (count)" for more than one, each repaired
  one at a time) [design for the grouping]. An item at full condition is not listed
  [owner: "an item at 100% condition can't be repaired, so it doesn't appear in the
  list"; the original lists it, dim, at "Cost: 0 caps", §3]. Armour needs a DT of at
  least 1 [documented: GECK wiki Armor, "In Fallout: New Vegas, the item must have at
  least 1 DT to be repaired by a vendor"]. Items without a health (DATA health 0) are
  not listed [design]; neither is non-playable armour (the Pip-Boy) [design].
- **How far**: up to the vendor's Repair skill as a percentage of full condition
  [hypothesis: the in-game help says vendors "require no special gear to perform the
  repair, they are limited by their own skill. Extremely talented repair vendors may be
  able to restore a weapon or piece of apparel to almost perfect condition" (MESG
  `HelpRepair`); at skill 100 the screenshots restore everything to full]. An item at or
  above that limit is beyond the vendor: its row is dim and the error text reads "The
  vendor's skill is too low to repair this." [design: the words; the original's are set
  in its code]. FalloutNV.esm has `fRepairSkillMax` 10.0 [observed]; its role is not
  documented and it is not read.
- **The cost**: the item's value × `fItemRepairCostMult` × the condition gained (a
  fraction), rounded to whole caps, halves up, at least 1 [hypothesis, fitted to the
  screenshots: Varmint Rifle (value 75) from 34 % costs 98 (75 × 2 × 0.655 = 98.3); 9mm
  Pistol (100) from 35 % 130; Straight Razor (35) from 55 % 32 (31.5); Minigun (5500)
  just short of full 11; `fItemRepairCostMult` is 2.0 in FalloutNV.esm, observed]. The
  value is the record's (WEAP/ARMO DATA), not the worn item's lower one; the player's
  Barter skill plays no part [hypothesis: the same fit].
- **Paying**: the caps leave the player and go to the vendor; when the vendor is the
  player nothing changes hands [documented: §2]. A repair the player cannot pay is
  refused.
- **Repair All**: every row the player can have repaired, each unit, for the sum of their
  costs; refused as a whole when the player cannot pay the total [hypothesis].
- **The figure after the repair**: our own damage and armour rules at the new condition
  (combat-rules.md §17: damage from 100 % at full to 50 % at zero, DT the same), rounded
  [design: the original's numbers (8 → 11 for the Varmint Rifle) do not follow our rule].
- **The condition shown**: whole percent, rounded [observed: "+66 %" from 34 %].
- **Sounds**: opening `UIMenuMode`, the pointer or keys on a row `UIMenuFocus` (the list
  box's), a repair `UIRepairWeapon` [observed: the SOUN exists in FalloutNV.esm;
  hypothesis: that it is the repair's sound, armour too], a refusal and Done
  `UIMenuCancel` [design].
- **Keys**: a click on a row or Enter repairs it, A Repair All, E (and Esc, Tab) Done; up
  and down or the wheel move the choice [documented: the file's `_PCButton_A`,
  `_PCButton_E`; design: Enter, the others].

## 5. Repair from the inventory

- The in-game help: "In your Pip-Boy, the Repair menu shows any equipment you currently
  have that can be used to repair the selected piece of equipment. Highlight the piece of
  equipment you would like to sacrifice to make the repair. The efficiency of your
  repair is affected by your character's Repair skill." "Whatever item you choose to
  repair with will be scrapped completely." [documented: FalloutNV.esm MESG `HelpRepair`,
  `HelpPipBoyItems`]
- The formula [documented: GECK wiki "Item Repair Formula"]: with TIC the target's and SIC
  the source's condition on 0 to 10, the gain in percent is `(max(TIC, SIC) + fRepairMin
  + (fRepairMax − fRepairMin) × Repair / 100 + min(TIC, SIC) × fRepairScavengeMult − TIC)
  × 10`, the result capped at 100 % with a minimum of 5 %, "in Fallout: New Vegas, the
  final condition is never rounded up". The settings' defaults: `fRepairMax` 2.0,
  `fRepairMin` 0.5, `fRepairScavengeMult` 0.05 [documented]; FalloutNV.esm sets none of
  them [observed].
- Which items: those on the target's repair list (WEAP/ARMO `REPL`, a form list), and
  items of the same kind always [documented: GECK wiki Armor, "Repair Item List: Form list
  of items that can be used to repair this object. Can be NONE, objects can always be
  repaired by items of the same type"].
- The page drawn from its file: §5.1.
- The ITEMS page's hint reads "Repair R)" or, above 75 % for a weapon and 50 % for
  apparel, "Maintain R)" [observed: the owner's screenshots; documented: the help text
  "At 75% Condition or higher, a weapon does maximum damage and only needs to be
  maintained", "Apparel above 50% CND provides maximum protection and only needs to be
  maintained"].

**What the file says** [documented: repair_menu.xml], in the Pip-Boy's menu rectangle
(855 x 650 at (50, 50) of the 1024 x 768 screen, as the inventory's):

- The headline: the inventory's title (`sInventoryItems`, font 4) between lines, and at
  the right a card 220 wide with `sRepairSkill` and the player's skill.
- The list 15 left and 75 down, 430 x 468; a row 32 in and 393 wide, its name in font 2
  at 40, 20 down, wrapped at 270, a condition bar 60 x 16 at 320 (back at alpha 64), the
  equipped square 25 at (15, 17.5). The rows are silent (`&nosound;` for the pointer and
  the click). Each row carries "the min skill needed to repair with this item; set in
  code" (`user1`): what it holds is not known, and no minimum is applied [hypothesis].
  Brackets border "the selected repair item".
- The item's picture 400 square at (450, 20).
- The figures from (415, 400), 427 x 180: the condition (`sInventoryCondition`, a meter
  60 x 16 15 from the card's right end) and the figure as cards half as wide; the arrow
  `repair_arrow_down.dds` 80 square in the middle (5 above the centre); the gains in font
  4 at a quarter and three quarters, 30 under the arrow's top; the cards after the repair
  at the bottom. A text "choose item" (font 2, centred, set in code) may stand there.
- Repair (Xbox only) and Cancel (`sCancel`, PC key E) right-aligned at 855, 75 down.
- The inventory menu's Repair button (`IM_RepairButton`, `sInventoryRepair`, PC key R)
  at (855, 45), right-aligned, at alpha 128 unless it can be used; the Mod button (X)
  after it.

**What the screenshots add** [observed, C:nv-private
epairmenuPlayer, Repair 17]:

- "Maintain R)" on the NCR Face Wrap Armor at about 55 %, "Repair R)" on the NCR Mantle
  Armor at about 35 % and on a Service Rifle at about 40 %; weapons also show "Mod X)".
  After the Mantle Armor was used up, the Face Wrap's "Maintain R)" is dim.
- The list's first row is the item being repaired, boxed; under it the parts (another
  NCR armour of the same repair list, the other Service Rifle). On the item itself only
  its CND and DT show; on a part, the figures before and after: "+9 %" and "+0" (DT 10)
  for the armour, "+10 %" and "+1" (DAM 8 → 9) for the rifle.
- After the repair the ITEMS list comes back, the part gone (weight 217 → 191).

**What NVForever does**:

- R on the ITEMS page opens the list for the selected weapon or armour; nothing happens
  without a part, or at full condition [owner: "an item at 100% condition can't be
  repaired or merged, so it doesn't appear in the list"]. The parts: the player's units
  of the same item or of one on its repair list (`REPL`, run-time additions to the list
  included), below full condition [owner's rule] and not equipped [design]; another unit
  of the item's own group counts. Each group of units is a row, by name. Full, for the
  item, its parts and closing the list, is from `SHOWN_FULL` (0.995) up, the vendor's
  line [design: `crates/fnv-game/src/repair.rs`, `engine/nvf-play/src/pipboy/repair.rs`],
  so the 99.9 % that AddItem gives counts as full; the log's percentages
  (`fnv_combat::condition::percent`) still print such an item as 99 %.
- Up/Down (W/S) choose, Enter or Space uses the chosen part, E or Esc cancels, Tab puts
  the Pip-Boy away [documented: `_PCButton_E`; design: the rest]. With the pointer
  (2026-10-19) [design: as the ITEMS list]: the row under it is chosen, a click on a row
  chooses it, a click on the part already chosen uses it, a click on "Cancel E)" cancels.
  Log `pipboy repair: clicked row <n>`.
- Using a part: one unit of it leaves the inventory, the item's condition becomes
  `merged_condition` (above) at the player's Repair skill when the list opened;
  `UIRepairWeapon` plays [hypothesis, as the vendor's]. The list then stays open on the
  same item, with the parts left, for the next repair; the ITEMS list comes back only
  when the item is at full condition, when nothing is left to repair it with, or on
  Cancel [design: the owner's rule, playtest list 7 item 16; the original's screenshots
  show the ITEMS list back after one repair]. Log `pipboy repair: stays open: …` or
  `pipboy repair: closed: …`.
- A physical repair or mod-menu key is consumed for the rest of its frame, including
  when it closes the list. ITEMS keeps its open-list guards and also checks `KeysTaken`,
  cleared once per frame in Bevy's `First` schedule; the closing Enter/Space (controller
  A maps to Enter) cannot also toggle equipment. Mouse messages and guarded Android
  maintenance requests do not mark physical keys, and
  a fresh press on the following frame reaches ITEMS normally [design, #2553:
  `engine/nvf-play/src/pipboy.rs`, `pipboy/repair.rs`, `pipboy/item_mod.rs`,
  `pipboy/items.rs`, `engine/nvf-play/src/pad.rs`]. The tile repair page retains these
  physical keys, so it uses the same guard [design:
  `engine/nvf-play/src/tiles/repair_menu.rs::HAND_BUILT_KEYS`].
  Regression coverage: `repair_keys_closing_the_list_preserve_worn_state` drives the
  real handlers with Enter, NumpadEnter and Space on synthetic inventories; smoke
  `pipboy-repair-enter` injects real Enter through `NVF_PLAY_TEST_KEYS` [design:
  `engine/nvf-play/src/pipboy/repair.rs`, `tools/smoke/scenarios.toml`].
- The hint: "Maintain R)" from 75 % (weapons) or 50 % (apparel), "Repair R)" below;
  dim when R would do nothing. "Mod X)" is drawn under it for weapons with mod slots
  (pipboy.md §5); the mod menu itself is not built.
- The figures are our damage and DT at the condition (combat-rules.md §17), as the
  vendor's menu.
- Scripts do not see MenuMode 1035 (`fnv_game::repair::REPAIR_MENU`) while the list is
  open [not built].

- **Weapon Repair Kit** [observed: FalloutNV.esm]: `NVRepairKit` is an ingestible with one
  effect, `RepairKitEffect` ("Repairs Current Weapon"), whose script `RepairKitEffectSCRIPT`
  reads the player's GetWeaponHealthPerc, adds 10 points below Repair 25, 15 below 50, 20
  below 75, 25 below 100 and 30 at 100, and sets the weapon in hand to that with
  SetWeaponHealthPerc (at most 99.83 % [documented: GECK wiki]). Used from the inventory it
  runs as any aid item. Under the Tweaks tab's Repair Kits in the Repair List
  (`bUseWeaponRepairKitsInRepairMenu`) the kits carried are parts in any weapon's repair
  list, after the others, and one raises that weapon by the same rule
  (`fnv_game::repair::kit_condition`) [design: the kit's script is not run, so a mod's kit
  that changes the script but keeps the effect repairs by the master's rule].

### 5.1 The page drawn from its file

Behind `[UI] TileMenus=RepairMenu`, on by default when the list is absent [design, #2215;
ui-tile-tree.md section 6.2], the repair list is drawn from
`menus\main\repair_menu.xml`, painted into the Pip-Boy's image (issue #1468) [design:
`engine/nvf-play/src/tiles/repair_menu.rs`, in the way of the S.P.E.C.I.A.L. page, pipboy.md
§5.1.1; the card pairs in `tiles/repair_cards.rs`]. Off, nothing here runs and the hand-built
list above is unchanged. The rules, keys, sounds and log lines stay
`engine/nvf-play/src/pipboy/repair.rs`'s; a click reaches them as `RepairInput`, as the
hand-built rows' do. The menu's code is 1035 and its tree is built the frame after the code is
set, so the hand-built list stands for the frame the page opens in [design].

**The words**, each with its source [design: LEGAL.md]:

| Tile | What it shows | Source |
| --- | --- | --- |
| `RM_Headline_Title` | not written | the file's |
| the skill card's `_Title` | the file's; only where the file has none | `sRepairSkill` |
| the skill card's `_Value` | a number | the model |
| `RM_RepairButton`, `RM_CancelButton` `button_text` | not written | the file's |
| Cancel's `_PCButtonText` | hint | `sPCMenuHintE` (a controller's button name, `pad::game_hint`) |
| a row's `ListItemText` | item's name and count | the record's name; "(n)" is ours |
| the condition cards' `_Title` | the file's; only where the file has none | `sInventoryCondition` |
| the stat cards' `_Title` | setting | `sInventoryDamage` for a weapon, `sInventoryDamageThreshold` for armour |
| the stat cards' `_Value`, the two gains | numbers, "+" and "%" | the model |
| `RM_ChooseItemText` | one line | ours (`repair_menu::CHOOSE_WORDS`), the original's is in its code |

**What the capture said** [observed: private job run07c_repair_menu, 1280x720, read and in its
pictures]:

- The page reports 1035, and it opens from R on ITEMS; E closes it, back on the same tab.
- Row 0 is the item: `_IsRepairItem` 1. Each row's `user0` is what the item would come to with
  that row, `merged_condition`'s at every point read, the item as if merged with itself; `user1`
  is 0 with the skill over 100. A row's bar is that row's own condition.
- The brackets stand on row 0 in every state, one less high than the row; the highlight follows
  the pointer, so both show with a part hovered.
- Off any row, as it opens: the item's own cards alone. On row 0: those, the arrow and the choose
  text, whose `alpha` pulses (64 to 252 at different reads). On a part: those, the arrow, the cards
  after the repair and the gains. The item's picture stays in all three.
- A card's meter follows the card's `user0`. Repair is hidden on a PC by the file, and row 0 does
  not act.

The model always has a row chosen, so "off any row" here is row 0 chosen with the pointer on none:
a pointer leaving the rows chooses the item again [design].

**Layout inputs and the Cancel check** [design: `tiles/repair_menu.rs::write_file_sizes`,
`usable`; `tiles/menus.rs::write_text_sizes_of`; `fnv_ui::input::hit_candidates`]: the page
writes each non-atlas picture's `filewidth` and `fileheight` from its file's texels and
current numeric zoom (100 for absent, symbolic or nonpositive zoom), including hidden
pictures. The walk covers the buttons, item icon, brackets, cards and generated rows.
Text measurement also includes hidden text; neither measurement depends on what is painted.
The file retains its expressions for button sizes and places. No inner-origin mark is
added to the buttons: their text is directly under the image, unlike the STATS tabs'
inner rects [documented: repair_menu.xml's tile outline].

Cancel's shortcut label stands outside its button [observed: private job
run07c_repair_menu, 1280x720]. The usability check probes `button_text` specifically,
rather than the first text descendant in painting order, which can be the shortcut.
An image's target does not extend to its children, so the external shortcut need not
take the pointer [design: `fnv_ui::input::hit_candidates`]. Cancel's words and every
whole visible row must still take it; an obstruction keeps the hand-built fallback
and its warning names the tile that took the hit, or says nothing did [design].
The fixtures use their own picture dimensions and expressions to cover a hidden
picture sizing a button, a shortcut sorted before its words, changed files and zoom,
and a covering target; they do not reproduce the game's layout (#1512) [design:
`tiles/repair_menu.rs::tests`].
The sizing fixture uses image tiles for both brackets and checks that every picture
under test is an image: a rect with a filename is still excluded by the sizing walk.
This also makes the atlas exclusion check exercise an image [design:
`tiles/repair_menu.rs::write_file_sizes`, `tests::picture_sizes_cover_the_page_and_follow_file_and_zoom_changes`].

**Open** [hypothesis]: a real repair on a part in the original, and what follows it (the model's
rule stands, §5); a row the skill cannot use (none dims here); a two-line row and the brackets on
it; a long list, the brackets moving with the scroll; the choose text's period (2 s here).

**The seam with the ITEMS page**: the page's adapter (`tiles/inventory_menu.rs`, on its own branch)
leaves ITEMS hand-built while a repair list is open, so that its body draws the list. With both
switches on, this page takes that place; the two have to be checked together when both land.

### Android presentation with Main's repair preferences

[design] The optional maintenance sheet reads the same repair Session and Part
records as the ordinary menu. Preview sessions display a disabled preview-only
confirmation, and kit rows display the shared Session::after gain and kit action.
A changed preview mode retires the old scroll/confirmation source. The existing
consumer still owns eligibility, exact unit consumption and inventory refresh.
Main's workbench/mod-skill requirements remain Open-only predicates for both
physical and touch confirmation; this merge adds no stricter touch-only policy.
Any future change to that policy must use a shared all-input effect boundary.
[qualification] New actual-consumer controls cover preview reopen/stale
confirmation, one-kit consumption and matching physical/touch Open-only behavior.
They are source-authored and remain uncompiled/unrun until separately qualified.


### Ordinary vendor-menu finger input and typography

[design] With the saved touch maintenance preference On (the Android default),
vendor repair now uses the ordinary native-file or hand-built menu, with finger
End taps and vertical list dragging. Dragging changes only that list's browse
position; it does not select, repair or pay. A tap on another row selects it; a
tap on the selected row requests the same repair as the existing pointer. The
original consumer remains responsible for condition, skill, caps, exact unit and
quote checks, caps transfer, and the Repair All confirmation tweak. Done follows
the same original close path. The alternate vendor sheet is not prepared by the
new ordinary-menu route. Off retains ordinary physical controls.

[design] When the optional Repair All confirmation is required, idle frames keep
the pending question. A deliberate selection, single repair or close retains its
existing cancellation behavior; another Repair All confirms through the original
cost and inventory consumer. This applies to physical and touch input alike.

Repair menu size changes font metrics/wrapping and measured rows in the existing
clipped viewport, with a 51-unit minimum hand-built row and larger explicit XML
row heights preserved. Paint, current native hit data and End admission use the
same size. Changed effective settings retire pending finger input; equivalent
values preserve it. Late script, creator, loading, device, geometry or source
changes refuse a proposal before the original effect.

This scope is vendor RepairServices. Pip-Boy item donor repair and weapon-mod
selection retain their existing touch presentation and saved On/Off behavior;
normal-menu conversion of those paths is separate work. At source `62e54d8`,
R40 executed the thirteen ordinary service controls: nine passed and four failed.
The failures cover confirmation, target publication and physical row geometry;
they are retained as the correction baseline. The correction's owning input and
geometry checks do not establish a phone outcome.
