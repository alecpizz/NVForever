# Spec note: weapon mods

## Android touch presentation (source increment)

[design] The compact Android maintenance sheet follows the touch ownership and
late-confirmation contract in `repair.md`, "Android touch presentation". It reuses
the existing mod session's offered slots, installed flags, effect descriptions
and attach/detach rules. A row tap selects only; a separate Install or
Remove confirmation acts on the painted weapon unit and mod choice. Back and
Cancel consume no mod. List scroll does not silently select or install a choice.

[design] A refreshed weapon, mod count, slot/installed state, session, modal or
input owner invalidates an old confirmation before the existing mod consumer
changes inventory. A successful action refreshes the actual session; it does not
predict effects in a disconnected display model. Native-file and hand-built menu
configurations use the same alternate mobile sheet, while their existing hardware
input behavior is preserved. Source/App/device validation are reported separately.

Tags as in `nif.md`: `[documented]` cites a page or file, `[observed]` says what was
looked at, `[hypothesis]` is a reading that nothing has confirmed yet. `[design]` marks
what is NVForever's own.

A weapon can take up to three mod items (IMOD records); the player attaches one from the
Pip-Boy's ITEMS page ("Mod X)"), which uses the item up and changes the weapon's figures
and its model. Code: `crates/fnv-combat/src/data.rs` (`WeaponStats::modded`, the slots),
`crates/fnv-sim/src/inventory.rs` (`Extra::mods`, `Inventory::set_mods_one`),
`crates/fnv-actor/src/records.rs` (`Weapon::model_for`), `engine/nvf-play/src/pipboy/
item_mod.rs` (the menu), `engine/nvf-play/src/combat.rs` (the player's modded figures),
`engine/nvforever/src/player_body.rs` (the modded model in hand).

## 1. Sources

- **The game's interface file** in `Fallout - Misc.bsa`: `menus\item_mod_menu.xml`
  ("Pipboy mod menu. Used within Inventory Menu."), with the shared pieces it includes
  (`list_box.xml`, `list_box_template.xml`, `card_info.xml`, `meter.xml`,
  `text_box.xml`, `bracket_vert.xml`, `vertical_fade_line.xml`, `line.xml`), and the
  inventory's `menus\main\inventory_menu.xml` (its Mod button). Described here in plain
  words; nothing of them is shipped.
- **The GECK wiki** (owner's export, `C:\fnv-private\geckwiki`): "Weapon Mods", "Item
  Mods", GetMenuTargetRef, GetEquippedWeaponModFlags, SetEquippedWeaponModFlags,
  WeaponHasModType, SetItemModFlags.
- **xEdit's record definitions** as the schema has them (`schemas/fnv/objects.json`, WEAP
  and IMOD).
- **FalloutNV.esm**, read with `fnvtool esm decode` (Varmint Rifle, 10mm Pistol, their
  mods) and the in-game help (MESG `HelpPipBoyItems`).

## 2. The records

**IMOD (Item Mod)** [documented: schema, xEdit IMOD]: the item fields every carried item
has (EDID, OBND, FULL, MODL, ICON, DESC, sounds YNAM/ZNAM) and `DATA` value (u32) and
weight (f32). "Weapon Mods have no effect in and of themselves - the only data they have
is text description. Multiple different weapons can therefore use the same mod and grant
different bonuses" [documented: GECK wiki Weapon Mods]. FalloutNV.esm has 50 [observed];
`ModNVVarmintRifleExtMags`: FULL "Varmint Rifle Extended Mags", DESC "Increases
ammunition capacity (+3).", ICON `interface\icons\pipboyimages\items\
items_mod_vrifle_mags.dds`, value 75, weight 0 [observed]. A weight on an item mod
crashes the original when the mod is loaded into the world [documented: GECK wiki Item
Mods].

**WEAP, the mod fields** [documented: schema, xEdit WEAP]:

| Field | Meaning |
|---|---|
| `WMI1`, `WMI2`, `WMI3` | the IMOD for slot 1, 2, 3 |
| `DNAM` `effect_mod1..3` | the slot's effect (enum `WeaponModEffect`, below) |
| `DNAM` `value_a_mod1..3`, `value_b_mod1..3` | the effect's two values (f32) |
| `DNAM` `reload_animation_mod` | a reload animation named "(Mod)" |
| `MWD1`…`MWD7` | third-person model paths: Mod 1; Mod 2; Mod 1 and 2; Mod 3; Mod 1 and 3; Mod 2 and 3; Mod 1, 2 and 3 |
| `WNM1`…`WNM7` | first-person model forms (statics), the same seven combinations |
| `WMS1`, `WMS2` | "Sound - Mod 1 - Shoot 3D / Shoot Dist", "Sound - Mod 1 - Shoot 2D" |

- The seven model fields run in the order of the flags as a number: field N is the model
  for the mods whose flags (§3) add up to N [observed: the names above, mask 3 = mods 1
  and 2 = `MWD3`; the Varmint Rifle's `MWD1` `VarmintRifleExt.NIF`, `MWD2`
  `VarmintRifleScp.NIF`, `MWD3` `VarmintRifleScpExt.NIF`, `MWD4` `VarmintRifleSil.NIF`,
  `MWD7` `VarmintRifleScopedSilencedExt.NIF`, its mods 1 extended magazines, 2 scope, 3
  silencer]. "Mods are implemented by swapping the model, not by adding components ...
  8 models for 3 mods" [documented: GECK wiki Weapon Mods].
- A weapon with no `WNAM` draws its `MODL` in first person; with mods on it draws the
  combination's `MWD` model there when it has no `WNM` form for it [observed: a mod's
  weapon, `CM4A1` from MMP, has `MWD` models and no `WNAM`/`WNM`, and showed its mods in
  third person only; hypothesis: the original shows them in both views]. A weapon with a
  `WNAM` but no `WNM` for the combination keeps its plain first-person model.
- The 10mm Pistol [observed]: slot 1 `ModNV10mmPistolExtMags` Increase Clip Capacity 4,
  slot 2 `ModNV10mmPistolLaserSight` Decrease Spread 0.4, slot 3 `ModNV10mmPistolSilencer`
  Silence; clip 12, min spread 0.8, `reload_animation_mod` ReloadA. The Varmint Rifle:
  Increase Clip Capacity 3, Increase Zoom 15, Silence; `reload_animation_mod` ReloadM.
- The `WMS` sounds name mod 1, yet both weapons' silencers are mod 3 and their `WMS1` is
  a `…ModFire3D` sound [observed: Varmint Rifle `WMS1` `WPNRifleVarmintModFire3D`]
  [hypothesis: the mod sounds play once a Silence or Suppressor mod is on, whatever its
  slot]. Not built.

**The effects** (`WeaponModEffect`, 1–16) [documented: schema enum; GECK wiki
WeaponHasModType's list], with what the values do [documented: GECK wiki Weapon Mods,
"Weapon Mod Effects"] and what NVForever does with them:

| Effect | GECK wiki | NVForever (`WeaponStats::modded`) |
|---|---|---|
| Increase Weapon Damage | A: flat bonus to damage | `base_damage + A` |
| Increase Clip Capacity | A: flat bonus to clip size | `clip_size + A` |
| Decrease Spread | A: flat reduction of min spread | `min_spread − A`, not below 0 |
| Decrease Weight | A: flat reduction of weight | `weight − A`, not below 0; the carried weight counts it too (§3) |
| Increase Max Condition | A: flat bonus to max condition | `health + A`; condition stays a fraction of it |
| Increase Rate of Fire | A: extra multiplier, 0.2 = 20 % faster | `fire_rate`, `attack_shots_per_sec` and `attack_mult` × (1 + A) [hypothesis: which figures "rate of fire" means] |
| Increase Zoom | A: flat value subtracted from the zoom field of view | `sight_fov − A`, not below 1 |
| Split Beam | A: projectiles added (negative: taken); B: min spread multiplier (1 keeps it); "the 30% bonus the modification already applies" | `projectile_count + A` (at least 1), `min_spread × B` when B ≠ 0; the 30 % is not applied [hypothesis: the wiki does not say what it applies to] |
| Silence | no values; "Sets the weapon's Sound Detection Level to Silent and switches to a new set of firing sounds" | `sound_level` "Silent"; detection hears each shot at the level of the weapon as fired, the unit's mod flags carried on the shot (`ShotFired::mods`, nvforever `detection::hear_shot`), the player's and other actors' alike (§7) |
| Suppressor | "Sets the weapon's Sound Detection Level to Normal and switches to a new set of firing sounds" | as Silence: `sound_level` "Silent" [hypothesis], which detection now hears (#2117). A retained discrepancy with the documented rule (the wiki: Normal; ours: Silent), unchanged here and a separate question |
| Increase Projectile Speed | A: multiplier of the projectile speed | the shot's speed × A, not for hitscan projectiles ([weapon-data-fields](weapon-data-fields.md) §5) |
| Regenerate Ammo (shots / seconds) | shots: every 4th shot is free; seconds: A added to the recharge time | shots: the player's every fourth shot uses no ammunition; seconds: `regen_rate + A` on a recharging weapon, at least 0.1 s (weapon-data-fields.md §5) |
| Decrease Equip Time, VATS Bonus | (VATS Bonus "unused in game") | kept, not applied (no master mod uses either) |

- The "(Mod)" reload animation is used once a mod with Increase Clip Capacity is on
  [hypothesis: the magazine is the part the reload handles]; NVForever swaps
  `reload_anim` for it then.

## 3. What a weapon carries: the mod flags

- Which mods a weapon has on is three bits: 1 mod 1, 2 mod 2, 4 mod 3 [documented: GECK
  wiki GetEquippedWeaponModFlags, SetEquippedWeaponModFlags]. The flags belong to the
  weapon in the inventory, not the base form [documented: GetWeaponRefModFlags /
  SetItemModFlags work on "this inventory reference's item"].
- NVForever keeps them in the unit's extra data (`Extra::mods`, beside condition, owner
  and equipped), so units with different mods are separate groups, saved with the
  inventory (the save's inventory is the simulation's serde form, `mods` written only
  when not 0) [design].
- "Attached Weapon Mods are effectively destroyed and merged into the weapon itself - they
  do not run their scripts, and cannot be detected using the GetEquipped function"; a
  mod's OnDrop and OnEquip blocks do not run when it is attached [documented: GECK wiki
  Weapon Mods]. NVForever removes one unit of the mod item without running anything
  [design, as documented]. In the original a mod cannot be taken off again [hypothesis:
  the menu offers no way; the wiki names no function but NVSE's
  SetEquippedWeaponModFlags]; NVForever's mod menu takes one off and gives one unit of
  the mod item back, nothing run either [design: the owner's rule, 2026-10-27].
- A modded weapon's row in the inventory lists reads its name with " +" [hypothesis: the
  GECK wiki's SetItemModFlags calls the flag "the inventory '+'"]; units with other mods
  are their own rows.
- A row's actions take that row's units: equipping, using, dropping, moving to a
  container and putting up for barter pick a unit with the row's condition and the row's
  mod flags (`fnv_sim::Pick`), so a modded rifle's row equips or drops the modded rifle
  and the plain row a plain one [design: the owner's rule for rows at one condition,
  2026-10-16, carried over to mods]. Units put up for barter are kept by item,
  condition and mods.
- The carried weight (the inventory's total, encumbrance, `GetAV InventoryWeight`)
  counts a modded weapon at its weight after its Decrease Weight mods, as the figures do
  (`weight − A` per mod on, not below 0) [hypothesis: the effect lowers the weapon's
  weight, the one the inventory adds up; not captured]. The simulation reads the effect
  and its value from WEAP `DNAM` itself (`Item::weight_mods`), as `fnv-combat` does,
  since the inventory's crate sits below combat's.
- **In the world.** A dropped (or script-placed) weapon reference keeps its unit's extra
  data, mods included (simulation.md §5), and a modded one draws the third-person model
  for its flags (`MWD1`…`MWD7`, `Weapon::model_for`), the plain world model when the
  record has none for that combination [hypothesis: "Mods are implemented by swapping the
  model" (GECK wiki Weapon Mods) holds for every view of the weapon; the `MWD` fields are
  the third-person ones]. An NPC holding a modded weapon (the unit it has equipped, else
  the one it would take) draws the same model in its hand [hypothesis, as above]. Its
  shots use the plain figures (only the player's weapon is modded in combat yet), but
  for their sound and their loudness for detection, which take the unit's mods (§7).

## 4. The ITEMS page's Mod hint and key

- `IM_ModButton` [documented: inventory_menu.xml]: text `sInventoryMod`, PC key X
  (`_PCButton_X`), 47 under the Repair button, shown whenever the Repair button is.
  The help: "Repair / Mod: These options take you to the Repair and Mod submenus, where
  you may repair damaged equipment or attach weapon modifications, respectively"
  [documented: MESG `HelpPipBoyItems`].
- NVForever shows it for a weapon with mod slots [hypothesis], always bright, since the
  menu takes mods off as well as putting them on [design: the owner's rule, 2026-10-27].
  X on a weapon with mod slots opens the mod menu; on anything else, and with Shift, X
  does nothing; a right click drops [design: owner 2026-11-02, #1393; pipboy.md §6].
  The original's controller button for Mod is LB (`&xbuttonlb;`) and for Drop X
  (`IM_DropButton`, `&xbuttonx;`) [documented: inventory_menu.xml]; NVForever's
  controller follows both (`pad::menu_buttons`, controller.md §3).

## 5. The mod menu

The menu is MenuMode **1061** [documented: GECK wiki GetMenuTargetRef, "1061 The weapon
being modded (Inventory Reference)"]. NVForever counts it among the Pip-Boy's codes, so
`MenuMode 1` holds under it [hypothesis, as for the repair list's 1035].

**What the file says** [documented: item_mod_menu.xml], in the Pip-Boy's menu rectangle
(855 x 650 at (50, 50), as the inventory's):

- Headline 50 high: the inventory's title (`sInventoryItems`, font 4) between lines as on
  every page, and `IMM_ItemName` (font 4, right-justified) from x 200 to 100 short of the
  rectangle's right edge: the weapon's name [hypothesis: the name; the file calls it
  "Player skill info display", copied from the repair menu].
- The list `IMM_ItemModList` at (−15, 75), 430 x 468, with brackets at 33 and 422 round
  the selected row. Rows (`IMM_ItemModListTemplate`): 32 in, 393 wide, the name in font 2
  at (40, 20) wrapped at 270, the equipped square 25 x 25 at (15, 17.5); rows make no
  sound on pointer or click (`&nosound;`). A row trait `_IsItemModItem` is true for "this
  item" [documented; hypothesis: as the repair list, the first row is the weapon and the
  others the mods].
- The picture `IMM_ItemIcon` 200 square at (450, 20) [hypothesis: the chosen row's
  picture, the weapon's on the first row]; `IMM_ModDesc` at (450, 220), wrapped at 350,
  100 high: the mod's description (IMOD DESC) [hypothesis: "Desc"].
- The comparison `IMM_CompareRect` from x 415 (the list's right edge), y 400, 427 x 180:
  two cards half its width, 60 high, at the top (condition with a 60 x 16 meter 15 from
  its right edge, and a stat), a down arrow 80 square
  (`interface\shared\arrow\repair_arrow_down.dds`) in the middle 5 up, gains in font 4 at
  a quarter and three quarters, 30 under the arrow's top, the two cards after the change
  at the bottom; a `IMM_ChooseItemText` (font 2, centred, wrapped at 400) "set in code".
- Buttons from (855, 75), right-aligned: the Mod button (Xbox only) and `IMM_CancelButton`
  `sExit` with the PC key E (`_PCButton_E`). NVForever writes "Exit E)" [hypothesis: the
  word, as the companion wheel's].

**NVForever's menu** [design unless tagged]:

- The first row is the weapon (boxed always), then one row per mod on it (marked with
  the equipped square) and per carried mod that fits a slot whose mod is not on yet, by
  name, with the count in brackets when more than one is carried. Under each mod's name
  its effect in words ("Ammunition capacity + 4", "Weight - 1.5", "Rate of fire + 20%",
  "Silenced": `item_mod::effect_text`) [design: the owner's picture of a modded menu,
  2026-10-27]. The Mod button's hint "Mod Enter)" first and "Exit E)" 46 under it, the
  words right-aligned at 855 with the keys after them (with a controller "Mod A)" and
  "Exit B)"; `sInventoryMod`, `sExit`, else "Mod", "Exit"), Mod dim on the weapon's own
  row and clickable on a mod [observed: "Mod" over "Exit" on the original's screenshot on
  issue #1896, taken with a controller; hypothesis: the same two with the keyboard and
  mouse] (#1896; until 2026-11-08 "Attach Enter)" or "Remove Enter)" on a mod
  only, and until 2026-11-09 under "Exit E)").
- On the weapon's row: its picture and the top cards (its condition, and DAM). On a mod's
  row: the mod's picture and description, the top cards for the weapon as it is (CND and
  the figure the effect changes: DAM, Clip, Spread, WG, CND max, Rate, Zoom, Beams), the
  arrow, the change in font 4 at three quarters ("+3"), and the bottom cards after
  attaching the mod, or after removing it for one that is on.
- Keys: Up/Down (W/S) choose, Enter or Space attaches or removes the chosen mod, E or
  Esc goes back. The pointer as in the repair list: a row under it is chosen, a click on
  the chosen mod attaches or removes it, a click on "Exit E)" goes back. The menu stays
  open after attaching or removing, on the weapon as it is now with the same mod chosen,
  until the player leaves it (or closes the Pip-Boy) [design: the owner's rule,
  2026-10-27].
- Attaching: one unit of the mod item leaves the inventory, the weapon unit (the
  equipped one first, as the repair list takes it) gets the slot's flag, the ITEMS list
  is refreshed. Removing: the flag comes off and one unit of the mod item comes back.
  If the player is holding that weapon, its figures and model change at once
  (`crate::combat`, the player body). Sound: `UIRepairWeapon` [hypothesis: no mod sound
  is named in the masters' UI sounds; the menu is the repair menu's sibling].
- Log lines: `pipboy mod: open <weapon>: <mods>`, `pipboy mod: <weapon> + <mod> (slot n,
  flags m)`, `pipboy mod: <weapon> - <mod> (slot n, flags m)`, `pipboy mod: cancel`, `pipboy mod: clicked row <n>`.
- Test aid: the Pip-Boy aid's `mod=open|down|up|use|cancel` (`test_aid.rs`).

### 5.1 The menu drawn from its file

Behind `[UI] TileMenus=ItemModMenu`, on by default when the list is absent [design, #2215;
ui-tile-tree.md section 6.2], the mod menu is drawn from
`menus\item_mod_menu.xml`, painted into the Pip-Boy's image (issue #1513) [design:
`engine/nvf-play/src/tiles/item_mod_menu.rs`, in the way of the repair page, repair.md §5.1;
the card pairs in `tiles/repair_cards.rs`, the list in `tiles/list.rs`, both shared with the
repair page]. Off, nothing here runs and the hand-built menu above is unchanged; with the switch
on and the file short of a tile, the whole hand-built menu stands and the log says why once
(`ItemModMenu drawn by hand: <path> missing`). The rules, keys, figures, sounds and log lines stay
`engine/nvf-play/src/pipboy/item_mod.rs`'s (the figures: `item_mod::compare`, which the
hand-built cards now read too); a click reaches them as `ModInput`, as the hand-built rows' do.
MenuMode is 1061 alone while the menu is open (the ITEMS page's 1002 is not open beside it); the
menu's tree is always loaded (`fnv_game::ui_loading::ALWAYS_LOADED`), and a tree built again
(a new menu tile) is written whole, keeping no tile of the one before [design]. While the menu is
open the ITEMS page drawn from its file (`InventoryMenu`) gives way, as for the repair list
(pipboy.md §5.2.1), so with this switch off the hand-built menu draws as before.

**From its file: what the code writes** [design; tile names documented: item_mod_menu.xml]:

| Tile | What is written | Source |
| --- | --- | --- |
| `IMM_Headline_Title` | not written | the file's (`sInventoryItems`) |
| `IMM_ItemName` `string` | the weapon's name, "+" when modded | the record's name; "+" is ours (§5) |
| `IMM_Headline_h2` `_length` | from where the line starts (after the title; the lead line's end in the screen menu) to 20 before the name's words, as the hand-built heading; written last each pass, so a script's own write to it does not last | [design]: the file's own reads `IMM_Headline_PlayerSkillInfo`, a tile only the repair file has, and comes to 0 |
| `WMM_Background` `visible` | 0: the Pip-Boy's own backing | [design, as RM_Background] |
| a row's `ListItemText` `string` | name, count, and under a mod's name its effect on a second line | the record's name; "(n)" and the effect words are ours (`item_mod::effect_text`) |
| a row's `ListItemText` `alpha` | 255 | the file leaves it to code [hypothesis: no row dims] |
| a row's `_IsItemModItem` | 1 on the weapon's row | [design: as `_IsRepairItem`]; `user0`, `user1` not written |
| `IMM_Template_ItemMarker` `visible` | the weapon worn, or the mod on it | the model |
| `IMM_LeftBracket` `y`, `height` | on the weapon's row, scrolled with it, one less than the row | as the repair page (repair.md §5.1) |
| the list's highlight | the model's chosen row | the model |
| `IMM_ItemIcon` | the chosen mod's picture, the weapon's on its row | the records |
| `IMM_ModDesc` `string` | the chosen mod's description; empty on the weapon's row | the record (IMOD DESC) |
| `IMM_ItemHealth`, `IMM_ModItemHealth` | the weapon's condition (`user0`), both sides alike | the model |
| their `_Title` | the file's; only where the file has none | `sInventoryCondition` |
| `IMM_ItemStat`, `IM_ModItemStat` | the figure the chosen mod changes (else DAM), as the model words it: whole ("11"), decimal ("0.40") or words ("Silent") | `item_mod::compare` |
| `IMM_StatImprovementText` | the gain, "+3" or "-0.50"; hidden for words | `item_mod::gain_text` |
| `IMM_HealthImprovementText` | hidden: a mod does not change the condition | the model |
| `IMM_ChooseItemText` | hidden, empty | [design: the hand-built menu has none and the original's was not seen] |
| `IMM_ItemModButton` `target` | 1 on a mod's row; `visible` stays the file's (shown on a controller only) | the model |
| `IMM_CancelButton` `_PCButtonText` | hint | `sPCMenuHintE` (a controller's button name, `pad::game_hint`) |

The card values are words, not numbers [design: `repair_cards::CardValues`]: the repair pages'
whole positive gains ("+N%", "+N") and the mod menu's decimal, negative and worded figures go
through the same card writer; the repair pages' words are unchanged.

**Input** [design]: the pointer is the Pip-Boy's own; a row it comes onto, once it has moved, is
chosen (`Hover`); leaving the rows keeps the choice, as the hand-built rows do. A click on a row
sends `Click` (choose it, or on the chosen mod attach or remove it, `item_mod::resolve`), the Mod
button `Use`, Cancel `Cancel`; the scrollbar's arrows, page areas and marker move the list's own
view. The keys stay the model's (`item_mod_menu::HAND_BUILT_KEYS`); a script's MenuTapKey of E
still clicks Cancel by the file's `_PCButton_E`. A click and a key in the same frame are not
arbitrated: each is acted on in turn, as with the hand-built menu (a click on the chosen mod and
Enter in one frame attach it and take it off again); only one key is kept from acting twice
(`menus::key_click_acts`). A hover and an arrow key in one frame: the model applies the hover
first and the key after, so the key decides the chosen row. The list treats that as a key's move
(and brings the row into view) only when the chosen row changes in the frame; a hover and a key
that end on the row already chosen move nothing [design].

**The description** [design]: the file gives `IMM_ModDesc` a `height` of 100 at y 220 and the
cards stand at y 400, but a text tile is measured to its words here and not clipped by its own
height, so an unlimited description could run over the cards. It is cut to the hand-built menu's
three lines (`item_mod::DESC_LINES`, whole words, `item_mod_menu::first_lines`). Of the two ways
to keep it off the cards, this is the one with a rule already in the code; clipping to the file's
100 would need the renderer to clip a text tile by its height, which no menu does yet, and how
the original fits a long description in that box was not seen [hypothesis].

**Log lines** (smoke reads them): `tiles: ItemModMenu ready: drawn from tiles` and
`tiles: ItemModMenu no longer drawn from tiles after <n> passes: <why>` at each change of the
decision, so a fallback of one frame shows; `tiles: ItemModMenu drawn from tiles: <n> rows (<m>
with an effect line); weapon "<name>"; highlight "<name>"; figures: <cards>` when what it shows
changes; `tiles: ItemModMenu <Click(n)|Hover(n)|Use|Cancel> on its tiles`.

**Known differences from the hand-built menu** (the maintainer's design, kept as the default)
[design]:

- The effect line wraps with the name at the file's 270, not 338: the file's row has one text.
- The Mod hint stands on a controller only (the file's `&xbox;`); on a PC the hand-built column
  shows "Mod Enter)" too (#1896). Enter and a click on the chosen mod still attach or remove it.
- An item with no figure has no stat card; the hand-built menu draws an empty one.
- The description is cut at three lines as the hand-built one, but wrapped at the file's 350 in
  its text tile (above).
- The list scrolls as the file's list does (its own view), not by the model's eight rows.

**Open** [hypothesis]: beyond the controller screenshot on issue #1896 (§5, the two hints), the
original's mod menu was not captured, so these were not seen: what it writes in `user0`, `user1`,
`_IsItemModItem` and the choose text, whether its rows show the effect at all, its cards' words
for each effect, and how it fits a long description.

## 6. Not built

- An NPC's modded figures in combat.
- Built 2026-11-06 (#982): **a modded weapon's value** is the weapon's value at its
  condition (barter.rs, the GECK wiki's "Item Condition And Value") plus the IMOD DATA
  value of each attached mod, which the condition does not lower [observed: campaign
  o26_value, 2026-11-06, `nvfairport`, ShowOff's GetCalculatedItemValue on the player's
  inventory reference, mods set with JIP SetWeaponRefModFlags and condition with
  SetItemRefCurrentHealth: a Varmint Rifle (value 75; Extended Mags 75, Night Scope 125,
  Silencer 100) read 75, 150, 200, 175, 275, 375 with mods 0, 1, 2, 4, 3, 7 at full
  condition; 26, 9, 49 without mods at 50, 25, 75 %; 326, 309, 349 with all three]. The
  page of GetCalculatedItemValue says it returns what the screen shows; the barter
  screen itself was not read [hypothesis that they agree]. NVForever: `Row::mods_value`
  (`FormStore::mods_value`, WEAP WMI1-3) added in `Row::worth`, which the barter prices,
  the item card and the totals read.
- Equip time (no master mod has it) and VATS bonus (unused in game). Increase Projectile
  Speed and both Regenerate Ammo effects are built (weapon-data-fields.md, 2026-10-21).
- Scripts' view: GetWeaponHasMod-style functions are NVSE/JIP (not vanilla) and not
  built. GetMenuTargetRef 1061 answers the weapon being modded and GetSelectedItemRef
  the highlighted mod (or the weapon), as inventory references (ext-menus.md).
  `GetCalculatedWeaponDamage` and `GetCalculatedWeaponDPS` (`crate::nvse_gaps`) resolve
  inventory references (using the owning actor's stats when held by an actor, else the
  player's) and world-placed weapon references (using the player's stats) in addition to
  actor/base-form calls [documented: GECK wiki GetCalculatedWeaponDamage], and apply
  attached `Increase Weapon Damage` (1) / `Increase Rate of Fire` (8) mods, condition
  scaling (`SetConditionDamagePenalty`, `Tweaks::no_condition_damage_penalty`,
  `Tweaks::no_condition_damage_buffer`), and `Tweaks::crit_damage_scale` [hypothesis:
  exact ammo/perk interactions in `GetCalculatedWeaponDamage` remain unbuilt; observer
  check].

## 7. The mod shoot sounds (ESM gap audit row 12)

- **The records** [documented: xEdit wbDefinitionsFNV WEAP; schemas/fnv `objects.json`]:
  `WMS1` "Sound - Mod 1 - Shoot 3D / Shoot Dist" (one or two form IDs in a row, as the
  plain `SNAM` pair) and `WMS2` "Sound - Mod 1 - Shoot 2D". The GECK's Mod Info tab names
  them "Attack Sound(s): third-person attack sounds (close and distant) used when either
  a Suppressor or a Silencer weapon mod is equipped" and "Attack Sound (2D): first-person
  attack sound used when either a Suppressor or a Silencer weapon mod is equipped"; the
  Silence effect "sets the weapon's Sound Detection Level to Silent and switches to a new
  set of firing sounds specified on the Mod Info tab", Suppressor the same with Normal
  [documented: GECK wiki Weapons, Mod Info tab and mod effects]. JIP's GetWeaponSound
  numbers them 9 "Attack Sound 3D (Mod)", 10 "Attack Sound Dist. (Mod)", 11 "Attack
  Sound 2D (Mod)" [documented: GECK wiki GetWeaponSound].
- **The masters** [observed, FalloutNV.esm and the add-ons, 2026-10-21]: `WMS1` carries
  one form ID only (no distant mod sound anywhere); e.g. `Weap10mmPistol` WMS1
  `WPNPistolSilenced22Fire3D`, WMS2 `WPNPistolSilenced22Fire2D`; the varmint rifle,
  sniper rifle, 12.7mm pistol and SMG `WPNRifleVarmintMod*`; Honest Hearts' .45 pistol
  `WPNPistol45Silenced*`. The grenade rifles also carry WMS1/WMS2 (the silenced 10mm's),
  a weapon whose mods are not Silence or Suppressor, so the sounds are not switched by
  any mod. No weapon has a mod looping sound. The weapons silenced from the start (the
  silenced .22 pistol `WeapNVSilenced22Pistol`, the silenced .22 SMG
  `WeapNVSilenced22SMG`) carry their quiet sounds in the plain `SNAM`/`XNAM` and no
  `WMS1`/`WMS2`, so they need nothing more.
- **The rule** (`fnv_combat::fx::WeaponFx::shoot_sounds`): a weapon unit with a mod on
  whose effect is Silence or Suppressor (`WeaponStats::sound_mod_on`) shoots with the mod
  set, when the record has one: 3D `WMS1` first, distant `WMS1` second, 2D `WMS2`; any
  other unit with the plain set (`SNAM` first, `SNAM` second, `XNAM`) [documented, as
  above]. With the mod set, a missing 2D falls back to the mod 3D and the reverse
  [hypothesis, as the plain set's 2D-else-3D]; a silenced weapon whose record has no
  `WMS1`/`WMS2` at all keeps its plain sounds [hypothesis: nothing else to play]; a
  silenced weapon has no distant sound when `WMS1` has no second entry (all the masters')
  [hypothesis: a silenced shot is not heard from afar; the plain distant boom would give
  it away]. The looping sound (`NAM7`) is the same with or without the mod
  [hypothesis: no mod loop field exists].
- **Who hears which**: the player's own shot plays the 2D sound (else the 3D), as before
  (combat-rules.md §16); another actor's shot plays the 3D sound at the muzzle, or its
  distant sound when the player's eye is at or beyond the 3D sound's maximum
  attenuation distance [documented: GECK wiki Weapons, "Attack Sound (Dist): the sound
  form played when the player is outside the Maximum Attenuation Distance of the 3D
  sound"] (the medium list's "WEAP Shoot Dist", built with this row). An NPC's mods are
  those of the unit it has equipped, else the one it would take
  (`Inventory::mods_in_use`), so an NPC with a silenced pistol sounds silenced
  [hypothesis: the same rule as the player's; its other figures stay plain, §3].
- **Detection hears the same unit**: each shot's message (`ShotFired`) carries the mod
  flags of the unit that fired it, as they were at the shot (the player's equipped unit;
  another actor's or a script shot's `Inventory::mods_in_use`), and detection takes the
  shot's loudness from the modded weapon (`WeaponStats::modded(mods).sound_level`, §2),
  not from whatever is equipped when the shot is measured: with a Silence mod on the
  shot is Silent (iSoundLevelSilent), as the shooter's action sound and as the impact
  fallback when the impact has no level of its own (an impact with an IPCT sound level
  keeps it; detection.md §8) [documented: the Silence effect above; design: docs/design/
  detection.md §3]. A Suppressor mod gives Silent too, against the wiki's Normal
  [hypothesis, retained, a separate question; §2]. Before #2117 detection read the base
  weapon's level.
- **Log**: `combat: <weapon> shoots its silenced sound <SOUN>` once each time the
  player's sound changes to a mod sound [design], and at debug level each NPC
  shot's choice (`combat: npc <ref> shot sound <SOUN> (silenced|plain, 3D|distant)`).


## 8. Models changed by scripts

- **The setters**: SetWeaponModel `weapon mask path` sets `MODL` (0) or `MWD1`–`MWD7`
  (1–7) [documented: SetWeaponModel's table]; SetModelPath sets `MODL` [documented];
  SetWeapon1stPersonModel `weapon trait [model]` sets `WNAM` (0) or `WNM1`–`WNM7` (1–7)
  [documented: SetWeapon1stPersonModel]. With no model it clears that form, so first
  person draws the third-person model the weapon has for the combination (§2)
  [hypothesis: WAP Year One's script files set every `MODL`/`MWD` model of a weapon with
  SetWeaponModel and then call SetWeapon1stPersonModel with no model for each of 0–7,
  which only means something as a clear; observed: its `gr_10mmP.txt`]. A model argument
  that is no form (an empty reference, a missing master's form) changes nothing
  [hypothesis: the original's no-op for a form it cannot resolve].
- **Drawn**: the changes hold for the session (`fnv_game::Game::weapon_model_changes`);
  the renderer copies them into its record view (`engine/nvforever/src/weapon_models.rs`),
  so the player's weapon in both views and an NPC's weapon taken up afterwards are drawn
  with them. A change that goes (a load) puts the record's models back. The player's
  weapon is drawn again as soon as its model changes [design].
- **Alternate textures**: a model a script set is drawn without the record's alternate
  texture list for the model it replaced [hypothesis: the list names that model's
  shapes]; a first-person form's list stays with the form, which other weapons may share; the third-person `MOD4` held in place of `MODL` (Lucky's) gives way to a
  script's `MODL` [hypothesis: the mod meant its model to be seen].
- **Not built**: a weapon lying in the world or placed in a cell keeps its record's
  model; an NPC already holding the weapon keeps the old model until it takes up
  another weapon or is drawn anew.


## 9. Sounds changed by scripts

- **The setters**: JIP's `SetWeaponSound weapon soundID [newSound]` and
  `GetWeaponSound weapon soundID` address all 14 `WEAP` sound slots `0..=13`
  (`SNAM` 0/1 Shoot 3D / Shoot Dist, `XNAM` Shoot 2D, `NAM7` Attack Loop, `TNAM` Attack
  Fail, `NAM6` Melee Block, `UNAM` Idle, `NAM9` Equip, `NAM8` Unequip, `WMS1` 0/1 Mod
  Shoot 3D / Shoot Dist, `WMS2` Mod Shoot 2D, `YNAM` Pickup, `ZNAM` Putdown)
  [documented: GECK wiki `GetWeaponSound`, `SetWeaponSound`; xEdit `wbDefinitionsFNV.pas`
  `WEAP`; observed: `gaps4` for slots 0–3]. Omitting `newSound` or passing null (`0`)
  clears the slot for the session [documented: GECK wiki `SetWeaponSound`]; invalid slots,
  non-`WEAP` targets, and non-`SOUN` replacements are rejected without mutating state
  [design].
- **Pickup and putdown sharing**: for `WEAP` records, Lutana's `GetPickupSound` /
  `SetPickupSound` and `GetPutdownSound` / `SetPutdownSound` share live slots 12 (`YNAM`)
  and 13 (`ZNAM`) with `GetWeaponSound` / `SetWeaponSound`, while keeping `SetPickupSound`
  and `SetPutdownSound`'s observed rule that a null sound leaves the existing sound intact
  [observed: `plugins2`].
- **`TempCloneForm` isolation**: cloning a `WEAP` carries over any active session
  overrides (both replacements and clears) onto the clone while later edits on the
  original or the clone stay isolated, and `TempCloneForm` `SOUN` aliases are accepted
  as valid replacement sounds [hypothesis: external JIP LN / xNVSE `TempCloneForm`
  behavior not yet directly probed on `WEAP` sound slots]. NVForever implements this
  snapshot internally via `Game::snapshot_weapon_sounds` [design].
- **Consumer view**: `fnv_game::Game::weapon_sound`, `Game::weapon_sound_override`,
  `Game::set_weapon_sound`, `Game::weapon_sound_changes`, and `Game::weapon_sound_stamp`
  expose the effective and session-changed weapon sound slots in the same pattern as
  `Game::weapon_model_changes` / `Game::weapon_model_stamp` (§8) [design]. Engine playback
  consumption over `WeaponFx` and inventory pickup/putdown audio is documented in
  `docs/spec-notes/weapon-sound-bridge.md`.
