# Crafting: recipes and the recipe menu

The workbench, the campfire and the reloading bench open one menu, the recipe menu
(playtest list 5 item 72). Code: `crates/fnv-game/src/crafting.rs` (records, the check,
`ShowRecipeMenu`), `engine/nvf-play/src/crafting.rs` (the menu).

## 1. The records

**RCPE, a recipe** [documented: GECK wiki "Recipe Entry" (owner's export); xEdit
`wbDefinitionsFNV` RCPE for the layout]:

| Subrecord | Meaning |
|---|---|
| `EDID` | Editor ID |
| `FULL` | The name shown in the menu |
| `CTDA` (any number) | Visibility conditions: a recipe whose conditions fail is not listed |
| `DATA`, 16 bytes | Skill (an actor value code, i32, −1 for none), level (u32), category (FormID), subcategory (FormID) |
| `RCIL` + `RCQY` | An ingredient (FormID) and its quantity (u32) |
| `RCOD` + `RCQY` | An output (FormID) and its quantity (u32) |

[observed: FalloutNV.esm has 105 RCPE; every DATA is 16 bytes; 263 RCIL and 144 RCOD,
and 407 RCQY, one after each. With every add-on loaded there are 291 recipes.]

**RCCT, a recipe category** [documented: GECK wiki "Recipe Entry"; xEdit RCCT]: `EDID`,
`FULL`, `DATA` (one byte of flags, bit 0 set for a subcategory). FalloutNV.esm has 10
[observed]: the categories Workbench, Campfire and Reloading Bench, and the subcategories
Aid, Ammo, Breakdown, Chems, Food, Misc and Weapons.

The wiki says that the skill NONE means the recipe can be crafted at any skill and the
menu shows no skill requirement. It also says a recipe "will only become usable once the
player has all the listed ingredients" [documented].

## 2. Opening the menu

`station.ShowRecipeMenu category` (alias `Craft`) takes a category or a subcategory
[documented: GECK wiki ShowRecipeMenu, whose example is `EDE1REF.Craft AmmoSubRecipes`].
The menu lists the recipes whose category or subcategory matches. The scripts of the
benches' activators call it. The recipe menu is MenuMode 1077 [documented: GECK wiki
GetMenuTargetRef, "1077 The actor whose skills are used for crafting"]. It is an engine
menu: scripts see it as open until the player leaves it.

## 3. The menu

[observed: the owner's screenshots in C:\fnv-private\Crafting; documented: the layout in
`menus\recipe_menu.xml`, see the module notes]

- The list: the recipes sorted by name. The label is the name, with the output count in
  brackets after it when more than one is made ("Antivenom (2)"). NVForever's campfire
  list starts "Antivenom (2), Atomic Cocktail (4), Battle Brew, Bighorner Steak, Black
  Blood Sausage, Black Coffee", the same as the original's [observed, smoke `crafting`].
- The arrows beside the list's title filter the list by subcategory [hypothesis].
- For the chosen recipe, at the right: where it is made, with its category's name; the
  skill as "<skill> (<player's value>/<level>)"; the ingredients as
  "<name> (<have>/<need>)". NVForever's hand-built page writes the titles the file's
  settings give (`sRecipes` in capitals, `sMadeAt`, `sSkillRequirement`, `sIngredients`),
  as the tile page does [design, #4495: in place of the words read from the screen].
- Under the list: the first output's WG and VAL. NVForever's cards read the settings
  `sInventoryWeightUpper` and `sInventoryValue`, as the Pip-Boy's do [design, #4495].
- Accept A) and Exit X).
- **The shade** (2026-10-19) [observed: the owner's screenshot at 1920 x 1080,
  C:\fnv-private\references\crafting_repair\recipes.jpg; documented: `RM_Background`,
  solid black at brightness 128 and `_background_fill_alpha`, 32 of 255 (globals.xml),
  1144 x 970 from (-40, -105)]: only the frame's lines over the blurred world (outer
  frame x 407 to 1559, y 113 to 924), no dark box. NVForever draws the sheet at the
  file's 32 of 255 black, which leaves no visible edge (it was the other menus' 0.75, a
  hard box well past the frame). The blur is not built.
- Making a recipe once removes its ingredients from the player and adds its outputs.
  Each output gets an "added" line in the corner.
- **The output's picture** (2026-10-19) [documented: `RM_ItemIcon`, 22 right of the list
  box's left and 472 below its top, 256 square]: the first output's ICON.
- **Its effects** (2026-10-19) [documented: the item stats display (`RM_ItemData`) has an
  EffectsInfo card; hypothesis: drawn as a line "EFFECTS" with the text under the WG and
  VAL cards, written as the Pip-Boy's ITEMS page writes effects]: an aid item's own
  effects, or an apparel's or a weapon's enchantment's ("HP +40, RAD -10").
- **Several at once** (2026-10-19) [documented: `menus\quantity_menu.xml`, the prompt the
  other menus use; hypothesis: the recipe menu asks it]: when the player has the
  ingredients (and the skill) for more than one making, Accept opens "How many?" from 1
  to the most the scarcest ingredient allows; Left/Right, the wheel, the prompt's arrows
  or a press on its meter set the amount, Enter or A makes that many (each output's
  "added" line gives the total; Items Crafted counts each making), E or Esc goes back to
  the list.

Not built:
- the "Made at" category for a recipe whose category differs from the bench's;
- crafting experience (whether the original gives any is not known).

## 4. The menu from its file (issue #1468)

[design: `engine/nvf-play/src/tiles/recipe_menu.rs`, `tiles/vanilla.rs`] With
`[UI] TileMenus=RecipeMenu`, the open recipe menu (1077) draws from
`menus\recipe_menu.xml`. The hand-built view stays the default, and the adapter does nothing
with its switch off. A missing file, a missing required path, or a row template without its
name, value or marker child keeps the whole hand-built view. One decision, the writer's,
supplies the drawing and the input. The list state comes from `tiles/list.rs`, shared with
the container and barter menus.

**What stays the model's** [design: `crafting.rs`]. What is listed and in what order, the
filter, what Accept makes, the prompt and every sound. The tiles ask through
`crafting::RecipeInput`, read in `keys` beside the keys and the hand-built buttons, so a tile
click takes the hand-built button's path. Physical keys stay the model's; a script's MenuTapKey
still follows the file's `_PCButton_A` and `_PCButton_X`. The wheel moves the selection, as in
the hand-built menu. The pointer selects only after moving and does not scroll the list.

**The rows**. Each recipe is a row of `RM_list_template` in `RM_Items_InventoryList`, kept by
its form. `ListItemText` holds the hand-built label, `RM_list_template_ItemValue` is empty, the
marker hidden. A click on another row selects it, a click on the selected row accepts [design:
#1468's plan; the hand-built row click only selects].

**The words** [design: LEGAL.md]. Nothing written here is transcribed from the original:

| Tile | What is written | Source |
|---|---|---|
| `ListItemText` of a recipe | the name, and "(n)" when more than one is made | the record; a number |
| `RM_ItemsTitle` | nothing without a filter; the subcategory's name with one, the file's back after it | the file's `sRecipes`; the record |
| `RM_MadeAtVariable` | the category's name | the record |
| `RM_SkillRequirement` | "{skill} ({have}/{need})", empty with none | the AVIF record (`fnv_game::chargen::skill_name`); numbers |
| `ListItemText` of an ingredient | "{name} ({have}/{need})" | the record; numbers |
| the card's `_Value` | WG, VAL, effects | the output's record; the Pip-Boy's effects text |
| the buttons' `_PCButtonText` | the key hint | `sPCMenuHintA`, `sPCMenuHintX` through `pad::game_hint` |

The titles "Crafted At", "Skill Needed", "Ingredients", Accept's and Exit's words are the file's
settings (`fnv_ui::strings`, or the load order's) and are never written. The hand-built "No
recipes" has no tile: an empty list is empty.

**What the original does** [observed: a private capture at 1280 x 720, the menu opened by
`ShowRecipeMenu` from a script; shots and reads].

- The backing: `RM_Background`'s alpha reads the container's 193.8, once, at the opening.
  [design] The tiled menu takes it and the tile blur in place of the hand-built
  `top_menu_backing` the owner chose for that drawing (playtest list 7 item 2; the default keeps
  it). This is a look difference for the maintainer to see.
- Dimming: `alpha` 127 short and 255 met, at `brightness` 255, on a recipe's and an
  ingredient's `ListItemText` and on the skill line, each by its own measure. The skill line's
  `visible` reads 1 in every state, the text empty with no row.
- Accept: the file ties `_line_alpha` to its `target`, 128 or 255, and its text's `alpha` to
  that [documented: `text_box.xml`], and brightness stays 255, so the adapter writes only `target`
  [design]. Both buttons' `visible` read 1. The key hint is the file's child `PCShortcutLabel`, right of the box, from
  `_PCButtonText`.
- The ingredients: rows of the one template in the hotrect `RM_Items_IngredientList` (a `text`
  of that name stands before it [documented: the file], so the path counts from the last,
  `:0`), the whole of "name (have/need)" in `ListItemText`, `ItemValue` empty. The pointer on
  one does nothing, no hover and no box, though a row reads `target` 1 under a list reading 0.
  [design] This tree's hit test would answer such a row, so the adapter writes its `target` 0.
- `_IsContainerListSelected` reads -1 in every state, the file's, with the list's `_enabled` 1.
  [design] The adapter leaves it.
- The title: the file's without a filter, the record's name in its own case with one, and the
  scroll back to 0 on a change.
- The list's counts fit `list::counts` (barter-menu.md section 8): 41 rows gave 37 items and
  delta 60.75.

**What NVForever does otherwise, in the model, unchanged here** [design: `crafting.rs`]. The
original's list opens with no row chosen and its box leaves with the pointer; here row 0 is
chosen and stays so. A row that comes under a still pointer takes the choice there, not here
(owner 2026-10-22, for the item menus). Its prompt opens on the most that can be made and the
menu closes after making; here the prompt opens on 1 and the menu stays (the owner's note of
2026-10-26 says no change either way on the prompt). The original keeps a filtered row's tile
with `listindex` -1; here it goes. The pointer off the rows, an ingredient's among them, takes
the original's box away; here the choice stays.

**Still open** [hypothesis]: a recipe with no skill (the header stays and the line is empty
here); how a long list of ingredients scrolls, and a two-line ingredient row's height. The model
would write the same for each.

**Not built**. The prompt from `QuantityMenu`'s tiles (it stays hand-built over the tiles),
`RM_Items_TitleButton` and `RM_Container_TitleButton`, the headline images and the mod cards.

**Smoke**: `tiles-recipe-places` (the pointer's places, no click), `tiles-recipe`,
`recipe-default-drawing`.


### Ordinary workbench finger input and typography

[design] The saved touch maintenance preference also enables finger input over
the ordinary workbench, campfire and reloading-bench presentations. The native
file keeps its actual recipe and ingredient lists and scroll consumers; the
hand-built recipe list uses its measured clipped window. A vertical drag browses
without selecting or making anything. Another-row End selects; a selected-row
End or Accept requests the existing quantity prompt. Its original default one,
maximum, arrows, tap-to-amount, confirmation, cancellation and ingredient/output
consumer remain authoritative. Touch does not mark input as a physical key.

Independent workbench size changes glyph metrics and wrapping within the fixed
viewport. Measured hand-built recipe rows retain at least 52 interface units;
XML rows preserve larger explicit heights. Physical navigation and post-make
selection follow the measured window; touch browse is not forced back to a
fixed seven-row selection window. Paint, native input and late admission share
the current size/source. Effective setting changes retire pending input, while
a no-op value preserves it. The native writer publishes readiness; the prompt's
post-input drawing publishes its current amount before the next raw contact.

Continuous workbench quantity-meter dragging and a new hand-built ingredient
scroll state remain follow-ups. Hand-built ingredients retain their original
wrapped explanatory presentation; native ingredient scrolling remains its
original independent list. No separate crafting rule/effect handler is added.
Authored native/hand-built producer-to-consumer, late-requirement, immediate
quantity and physical navigation controls are uncompiled and unexecuted at this
source checkpoint; loaded-game and phone acceptance remain open.
