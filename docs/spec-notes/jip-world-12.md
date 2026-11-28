# JIP LN NVSE, batch 12: world, weather, cells, barter and menus (2026-11-09, #1006)

The climate, the wind, the moons and lightning, the image space modifiers on, the cells
kept and found by grid, the light at a point, fast travel and the load order, the game
settings as a map, purged references and internal markers, the barter menu's trade, the
item menus' filters and lists, the recipe menu and AddRecipeCondition, the quantity and
text input menus, and GetWheelDisabled, in `crates/fnv-game/src/nvse_jip12.rs`. Sources:
the GECK wiki export, each function's page [documented: the page named]. The probe set
`jip12` (`tools/capture/jip_probes.py`, lab `L5.ext.jip12`) asks the original what the
[hypothesis] values below leave open.

34 functions; JIP 142 → 108 on main, 102 → 68 once batch 11 (#1915) is merged
[observed: the extender_census test, 2026-11-09].

List getters with nothing to give return no array [observed: the jip10 probes, for JIP's
list getters; hypothesis: the same here].

The engine tells the game what only it knows, and takes back what the scripts asked for:

| From the engine | `Game` method | Engine side |
|---|---|---|
| the cells kept | `set_buffered_cells` | `nvforever::cell_buffer::tell_game` |
| the detection round's lights | `set_light_field` | `nvforever::detection` |
| a fast travel | `note_fast_travel` | `nvf_play::pipboy::data::travel::depart` |
| a save of another load order | `set_load_order_changed` | `nvf_play::save` (the engine's part of a load) |
| the item menus' filters and lists | `set_item_menu_view` | `nvf_play::menu::publish_jip_menus` |
| the barter menu's trade | `set_barter_view` | the same |
| the recipe menu's category and selection | `set_recipe_menu` | `nvf_play::crafting::draw` |

| To the engine | `Game` method | Engine side |
|---|---|---|
| a climate | `script_climate` | `nvforever::worldspace::script_sky` |
| the wind | `script_wind` | `nvforever::tree::wind_vector` (trees and grass) |
| the moons' textures | `moon_textures` | `nvforever::sky::setup_sky` |
| a lightning flash | `take_lightning` | `nvforever::weather_fx` |
| internal markers | `internal_marker_changes` | `nvforever::world::script_markers` |
| the price multipliers | `barter_price_mults` | `nvf_play::menu::model::Trade::mult_set` |
| AddRecipeCondition | `recipe_conditions_pass` | `nvf_play::crafting::listed` |
| the quantity menu | `quantity_ask`, `finish_quantity` | `nvf_play::script_quantity` |
| the text input menu | `text_input`, `finish_text_input` | `nvf_play::chargen` (the name menu's box) |
| SetWheelDisabled | `wheel_disabled` | `Game::has_companion_wheel` |

## 1. The sky

| Function | NVForever |
|---|---|
| ForceClimate `[climate]` | the climate until ForceClimate without one or SetCurrentClimate [documented]; the sky is made again with it, keeping its weather [hypothesis: the original goes on to the new climate's weathers from there]; GetCurrentClimate reads it |
| SetCurrentClimate `climate` | the climate while the player stays in the worldspace it was set in; does not return the climate set (`retSet != rCl2`, `setRet=0`) [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 + ShowOffNVSE 184, 2026-11-25, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, isolated clean-room staging copy (read-only probe; zero writes to game install or Saves), `w6d2_18_force_set_current_climate_and_isnight` (`tools/capture/nvse/catalogs/wave6d2_set_current_climate_return_zero.json`): `PROBE\|w6d2_18_force_set_current_climate_and_isnight\|cur0z=0\|setRet=0,aftSet=1,frc=0,clr=0\|night=0/0\|m(len0=29,len4=0,len8=0,set=[Textures\Sky\w6d2_moon.dds])`; zero DLL, PDB, or third-party source consulted]; returns `0` [hypothesis]. [decision: the page's "Bugs" note, the climate going back after about 7 seconds, is not followed] |
| GetWorldspaceClimate, SetWorldspaceClimate `world climate` | reads/overrides a `WRLD`'s `CNAM` climate for the session [documented: GECK wiki GetWorldspaceClimate, SetWorldspaceClimate; observed: gaps8, WastelandNV `0008809B` set to the Strip's and back]. Rejects non-`CLMT` replacement forms without mutation [observed: 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 with the 10 official base masters, clean-room script probe `w4c_b1_09_worldspace_clmt_imgs` (`tools/capture/nvse/catalogs/wave4c_extenders.json`, #3678 / PR #3679), calling `SetWorldspaceClimate` and `SetWorldspaceImagespace` on `WastelandNV` (`0x000DA726`) with Stimpak (`0x00015169`): `c0=0008809B,cBad=0008809B(eq0=1)|i0=0008809D,iBad=0008809D(eq0=1)`, plus 2026-11-23 `b3_jip12_01_climate_and_wrld_setters` (`wave4b_extenders.json`): `c:0008809B->0008809B|i:0008809D->0008809D|cur=0000015F`; no DLL or source consulted]; also rejects non-`WRLD` and null arguments without mutation [design]. `Game::worldspace_climate` / `worldspace_climate_override` follow `WNAM` + `PNAM` Use Climate Data (`parent_use::CLIMATE`, or missing child `CNAM`), with child-own overrides taking precedence [documented: xEdit WRLD; design]. `Game::script_climate` and `Game::current_climate` apply `ForceClimate` > `SetCurrentClimate` (in matching worldspace) > `SetWorldspaceClimate` > authored `WRLD CNAM` (returning `None` indoors), reaching `GetCurrentClimate`, `IsNight` (with omitted-climate fallback to `current_climate` [hypothesis]), `GetMoonPhase`, `Game::current_climate_weathers`, and `Game::current_climate_default_weather` [design; hypothesis: engine-side sky/weather rendering consumes `script_climate` and `current_climate_*`] |
| GetWorldspaceImagespace, SetWorldspaceImagespace `world imagespace` | reads/overrides a `WRLD`'s `INAM` image space for the session [documented: GECK wiki GetWorldspaceImagespace, SetWorldspaceImagespace; observed: gaps10, WastelandNV `0008809D`]. Rejects non-`IMGS` replacement forms without mutation [observed: `w4c_b1_09_worldspace_clmt_imgs` and `b3_jip12_01_climate_and_wrld_setters` above]; also rejects non-`WRLD` and null arguments without mutation [design]. `Game::worldspace_image_space` / `worldspace_image_space_override` follow `WNAM` + `PNAM` Use Image Space Data (`parent_use::IMAGE_SPACE`, or missing child `INAM`), with child-own overrides taking precedence [documented: xEdit WRLD; design]. `Game::cell_image_space` (backing `GetCellImageSpace`) prefers `SetCellImageSpace` over authored `CELL XCIM`, and `Game::current_image_space` resolves the current cell's image space ahead of the exterior worldspace's (`Game::current_worldspace_image_space` / `current_worldspace_image_space_override`). `nvforever::world::cell_image_space` (and `worldspace::script_sky` across climate rebuilds) binds explicit exterior `SetCellImageSpace` on cell load ahead of live `SetWorldspaceImagespace` overrides and authored `WRLD INAM`, bumping `Game::image_space_sync_token()` on change [design; hypothesis: live refresh on `SetWorldspaceImagespace`] |
| GetWindDirection, SetWindDirection `angle` | degrees, 0 north, 90 east, -90 west, ±180 south [documented]; the set angle read back as given, 400 as 400 [observed: jip12 setwind]; -57.2958 (-1 radian in degrees) when no script set one [observed: jip12 wind, the probes' start cell 0x000DEB16; hypothesis: the same everywhere until a script sets it]. NVForever's own wind still blows where it did when no script set one [decision]. The trees and grass bend to a set angle |
| GetMoonTexture, SetMoonTexture `phase [path]` | phases 0 full to 7 three waxing [documented: the table]; for a phase no script set, `\Textures\Sky\Masser_<phase>.dds` (the files the sky draws) and "" for 4, the new moon [observed: jip12 moon.0, moon.3, moon.4]; a set path is drawn from then on, a leading backslash dropped |
| TriggerLightningFX | outdoors in the rain only [documented]; a flash in the weather's lightning colour (white without one [hypothesis]) and, in a weather with thunder sounds, a thunder clap |

## 2. The world

| Function | NVForever |
|---|---|
| GetActiveIMODs | the active modifiers on the screen in application order without deduplication (`[00054E13, 001736A8, 00054E13]` when applied in that order on the same frame; `ar_Null` when none) [documented; observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, `jip_42_wind_active_imods_ft` (`-57.2958,-1,0,0`) and `w4d_24_active_imods_order` (`tools/capture/nvse/catalogs/wave4d_extenders.json`, #3679): `n=3,0=00054E13,1=001736A8,2=00054E13`; no DLL or source consulted]; next-tick retention of duplicate entries is `[hypothesis]`. `Game::apply_image_modifier` stacks both animatable and non-animatable modifiers in exact application order (`image-modifiers.md` §3), capped at `Game::MAX_IMOD_STACK = 64` per form while `nvforever::imagespace::screen_effect` draws at most one instance per form [design]; the weather's image space adapters are not modifiers [hypothesis] |
| GetBufferedCells `interiors` | the cells the engine keeps (`nvforever::cell_buffer`: 36 exterior squares, 3 interiors) and the loaded ones, interiors (1) or exteriors (0) [documented]; an empty array when there are none [observed: jip12 buffered.interiors] |
| GetCellBuffered `[cell]` | whether the cell, else the calling reference's, is among them [documented] |
| GetCellNorthRotation `cell` | the Z angle in degrees of a reference of NorthMarker in the cell; 0 without one [documented; observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 with the 10 official base masters, clean-room script probe jip_41_cell_north_rotation (GetCellNorthRotation): 0.000,0.000; no DLL or source consulted; hypothesis: the marker is the static FalloutNV.esm names NorthMarker] |
| GetExteriorCell `world x y` | the worldspace's cell at that grid square [documented], only while it is in memory: 0 for WastelandNV -18 0 from the probes' start cell 0x000DEB16 [observed: jip12 extcell; hypothesis: in memory = the buffered and loaded cells] |
| GetWorldspacePersistentCell `world` | the cell of the WRLD group that holds its persistent references [documented]; `RefTable::world_persistent` |
| GetLightAmountAtPoint `x y z` | 0 to 100 [documented], by the rule the detection round measures an actor with: the base light (the sun's outdoors while it is up, the cell's ambient indoors) plus each light within reach, `strength × (1 − (d / reach)²)`; a point outdoors counts as in the sun [hypothesis: no ray is cast to it]; 0 before a round has measured [hypothesis]. The original gave 100 at the player in the probes' start cell 0x000DEB16 [observed: jip12 light.player]; the lab's candidate has no engine to measure, so that row stays apart |
| GetPCFastTravelled | whether the player fast travelled since the calling script last asked [documented]; a script's first ask counts from the start of the session [hypothesis] |
| GetLoadOrderChanged | the loaded save's plugins moved or are gone [documented: "the load-order has changed"; hypothesis: plugins only added after the save's own do not count] |
| GetGameSettings `[kinds]` | a map of setting name to value: 1 the numbers, 2 the texts, 3 both by default [documented]; the settings the load order's records hold, named as the records spell them [decision: the settings only the executable defines are not listed]. The original lists 3192 numbers and 1769 texts (sOk among them) where the load order's records hold 519 and 130 [observed: jip12 gmst.numbers, gmst.strings, gmst.string]; keys ignore case [observed: gmst.lowercase] |
| GetStringHash `text` | 0 [decision: the page is marked Incomplete and names neither the arguments nor the hash]. It takes a string: `GetStringHash "NVForever"` compiles in the original and gives -439019037 [observed: jip12 hash]; none of FNV-1/1a, djb2, sdbm, ×31, ×131, CRC-32 or Adler-32 gives that on the text as written, lower or upper case, so the hash stays unknown. The compiler takes the argument (encodings.json `extra_params`) |
| TogglePurgeOnUnload `[toggle]` | created references only [documented]; one marked is deleted when its cell leaves the loaded area (`Game::cells_left`) [documented]; returns the previous mark before applying `toggle` (`0` query, `1` returns `0`, `0` returns `1`; non-dynamic reference returns `0`) [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-25, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, isolated clean-room staging copy (read-only probe; zero writes to game install or Saves), `w6d2_23_world_state_internal_marker_and_light` (`tools/capture/nvse/catalogs/wave6d2_toggle_purge_on_unload_prev_return.json`): `PROBE\|w6d2_23_world_state_internal_marker_and_light\|im=1,1,0->0->0\|pg(dr=0,dyn=0,0,1)\|lPt=99.0,lAct=100.0\|ft=0,0,loc=0\|gmst=3189/1769`; zero DLL, PDB, or third-party source consulted] |
| SetInternalMarker `form [toggle]` | whether the form is not drawn, changed when a toggle is given (1-arg returns current state `0`/`1`, 2-arg updates the state and returns `0`) [documented; observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-25, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, isolated clean-room staging copy (read-only probe; zero writes to game install or Saves), `w6d2p4_53_micro_precision_bounds` (`tools/capture/nvse/catalogs/wave6d2_set_internal_marker_two_arg_return.json`): `PROBE\|w6d2p4_53_micro_precision_bounds\|clmNeg=1430\|sf16=1\|mf=1->3:3->7:3\|im=0,0,1,0,0\|rk(nth=[Squire],arr0=[Squire])`; zero DLL, PDB, or third-party source consulted]; by default a form with a marker's model (`marker*` or under `markers\`), or, for a record whose fields the game does not keep (statics), an Editor ID naming a marker [hypothesis]. The engine leaves the form's references undrawn from the next cell drawn [hypothesis: the original's change shows on the next load of the 3D] |

## 3. The barter menu

| Function | NVForever |
|---|---|
| GetBarterGoldAlt | the merchant's caps shown in the trade [documented]; 0 with no trade [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 with the 10 official base masters, clean-room script probe jip_40_barter_and_filter_defaults (GetBarterGoldAlt/GetBarterPriceMult/GetMenuItemFilter): 0,0.000,0.000,1,-1,-1; no DLL or source consulted] |
| GetBarterItems `sold` | inventory references to the items put up, bought (0) or sold (1), one per item; none with none [documented: "Returns 0 if there are no items"] |
| GetBarterPriceMult `sold`, SetBarterPriceMult `sold mult` | the multiplier on an item's value, buying (at least 1) or selling (at most 1) [documented]; set for the open trade only (reopening BarterMenu resets to the base rules) and 0 with no trade [observed: `jip_40_barter_and_filter_defaults`; Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, `w4d_20_barter_price_mult_live` (`tools/capture/nvse/catalogs/wave4d_extenders.json`, #3679): `open1(mm=1053,b=1.482->1.250,s=0.517->0.750,fB=0)|closed(b=0.000,s=0.000,fB=-1)|reopen(mm=1053,b=1.482,s=0.517)`; no DLL or source consulted]. The rows are priced again with it |

## 4. The item and recipe menus

| Function | NVForever |
|---|---|
| GetMenuItemFilter `menu [right]` | 0 all, 1 weapons, 2 apparel, 3 aid, 4 misc, 5 ammo [documented]; the inventory, container and barter menus; inventory (1002) reads 1 before open and while open, whereas barter (1053) reads 0 while open and -1 when closed (and container 1008 reads -1 before open) [observed: jip12 filter.inventory; `jip_40_barter_and_filter_defaults` (`1,-1,-1`); Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, `w4d_19_menu_item_filter_persist` (`f0=1|open1(mm=1002,f=1)`) and `w4d_20_barter_price_mult_live` (`fB=0` open, `fB=-1` closed) (`tools/capture/nvse/catalogs/wave4d_extenders.json`, #3679); no DLL or source consulted]; retaining the inventory filter after close and across reopen is [hypothesis] |
| GetMenuItemListRefs `[all] [right]` | a map of row index to inventory reference [documented]; the rows shown [decision: NVForever's lists hold only the rows the filter lets through, so `all` gives the same and the keys count the shown rows; the Misc folder rows are left out]; the repair menus give none [decision] |
| GetRecipeMenuCategory, GetRecipeMenuSelection | the open recipe menu's category and highlighted recipe [documented] |
| AddRecipeCondition `recipe func param comp value` | a condition joined with AND to the record's, run on the player, for the session: function `func` (its index; an opcode less 0x1000), the parameter, comparison `comp` against `value` [hypothesis: the page is marked "TO DOCUMENT"; `comp` read as the CTDA operator code, 0 `==` 1 `!=` 2 `>` 3 `>=` 4 `<` 5 `<=`] |

## 5. The quantity and text input menus, the wheel

| Function | NVForever |
|---|---|
| ShowQuantityMenu `callback max [initial]` | menu 1016, the item menus' "How many?" prompt on its own, 1 to `max` from `initial` (`max` without one [hypothesis]); the callback gets the amount next frame, -1 for Cancel [documented]. Returns `1` on open and `0` when called again while already open [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, read-only `PrintToConsole` probe `b3_jip12_07_show_qty_text_menu_blocked` (`tools/capture/nvse/catalogs/wave4b_extenders.json`, #3679, calling `ShowQuantityMenu rCb 10 3` twice in a row): `PROBE\|b3_jip12_07_show_qty_text_menu_blocked\|open1=1,whileOpen=0`; no DLL or source consulted]; blocking while other menus are open with no callback (as ShowTextInputMenu) remains [hypothesis; #2018] |
| ShowTextInputMenu `callback width height title …` | the name menu's box (menu 1051) with the formatted title and the box's size [documented]; Ok gives the callback the text [documented]; 1 when it opened, 0 while another menu is open [hypothesis] |
| SetTextInputExtendedProps `x y [min] [max] [switches]` | the length limits (1 and 32,767 by default), digits and the point only (1), Enter closes (2) [documented]; without switch 2 Enter does nothing [decision: the box is one line]; the box stays centred [decision: the offsets are kept, not drawn]; a call while the box is open changes it at once, the text cut to a shorter limit [decision: #2018] |
| SetTextInputString `text …` | the box's text, formatted [documented] |
| GetWheelDisabled | SetWheelDisabled's flag, 0 for a non-actor [documented]; NVForever's wheel now stays shut for such an actor |

## 6. Follow-ups

- GetStringHash: the hash, once a page or probe says what it is.
- GetGameSettings: the settings only the executable defines (NVForever's `EXE_SETTINGS`
  fallbacks).
- GetMenuItemListRefs: the rows the filter leaves out, and the repair menus.
- SetTextInputExtendedProps: the offsets, and a box of several lines.
- GetLightAmountAtPoint: a ray to the sun for a point outdoors.
