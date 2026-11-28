# Spec note: fast travel from the Pip-Boy's world map

**NVForever's words** [design, #2984]: the question is "Travel to <place>?" unless a
plugin sets `sTravelQuestion`, and the refusals are NVForever's own sentences where no
plugin sets their settings (`pipboy::data::travel`); `sNoFastTravelScriptBlock` is
FalloutNV.esm's. The original's question quoted below only says where it stands.

Tags as in `nif.md`: `[documented]` cites a page or file, `[observed]` says what was
looked at, `[hypothesis]` is a reading that nothing has confirmed yet. `[design]` marks
what is NVForever's own. Implementation: `engine/nvf-play/src/pipboy/data/travel.rs`
(the rules, the box's model, the trip) and `pipboy/data.rs` (the click on the map — or
the controller's A on a marker — and the box drawn over it). The Pip-Boy itself:
`pipboy.md` §5.3.

## 1. Sources

- **The owner's screenshot** (2026-10-17, kept privately) of the original's World Map
  in Freeside with the question box up: a black box with a thin border in the middle of
  the Pip-Boy's screen, "Do you want to travel to Goodsprings?" centred at its top,
  "Yes" in a bordered box above "No", both centred.
- **The GECK wiki** (the owner's export, `functions.xml`, `Misc.xml`): the pages
  EnableFastTravel, GetPCCanFastTravel, GetMapMarkerTravel, SetOnFastTravelEventHandler,
  the Reference page's "Map Marker" section, and the list of game settings.
- **xEdit's record definitions** as `schemas/fnv/world.json` records them: CELL `DATA`,
  WRLD `DATA`, REFR `FNAM` flags.
- **The game's interface files** in `Fallout - Misc.bsa`: `menus\message_menu.xml` and
  `menus\main\map_menu.xml`. Described in plain words; nothing of them is shipped.
- **The plugins** (FalloutNV.esm and the DLC masters), read with `fnvtool` and a
  throw-away reader of the records' flags (kept privately).

## 2. When the player may travel

**Which markers** [documented: GECK wiki, Reference, Map Marker]: a map marker's "Can
Travel To" flag (`FNAM` bit 1 [documented: xEdit]) lets the player travel there at once;
without it the player "must discover it before fast travel is allowed". ShowMap's
canTravel and ShowAllMapMarkers' canTravel make markers travel targets too [documented:
GECK wiki ShowMap, ShowAllMapMarkers; `fnv_game::functions::player`]. The interface
file keeps a flag per marker, "True if this marker can be used for fast-traveling"
[documented: map_menu.xml, `MapMarkerTemplate` `user0`]. A click on a known marker that
is not a travel target is answered with the setting `sNoFastTravelUndiscovered`
[hypothesis: by its name].

The World Map includes markers from every worldspace sharing its map root: follow
`WNAM` while `PNAM` Use Map Data (bit 2) is set [documented: xEdit WRLD,
`schemas/fnv/world.json`]. Each uses its own `ONAM` and bounds before projection on
the root's `ICON` / `MNAM`; the root's own offsets apply too [observed: run08 captures
of MapMenu traits, 2026-11-07; formula in `pipboy.md` §5.3]. Discovery within `XRDS`
requires the player to be in the marker's own worldspace. The Local Map and compass
keep that worldspace's markers [design: `engine/nvf-play/src/pipboy/data.rs`].
Travel uses the marker or linked arrival reference's destination worldspace, including
another member of the map family; the clock rule below is unchanged
[design: `engine/nvf-play/src/pipboy/data/travel.rs::go`].

**From where.** The game settings name the refusals [documented: GECK wiki, game
settings list]: `sNoFastTravelScriptBlock`, `sNoFastTravelCell`, `sNoFastTravelInAir`,
`sNoFastTravelCombat`, `sNoFastTravelHostileActorsNear`, `sNoFastTravelOverencumbered`,
`sNoFastTravelAlarm`, `sNoFastTravelUndiscovered`, and `sFastTravelNoTravelHealthDamage`.
Only `sNoFastTravelScriptBlock` has a record in FalloutNV.esm ("Fast travel is currently
unavailable from this location.") [observed]; the others' texts live in the program and
are not read (its code section is encrypted). NVForever uses a plugin's text when one
sets it and its own wording otherwise [design]. What each one means, and the order they
are tried in, are NVForever's reading [hypothesis unless marked]:

1. **A script forbade it**: `EnableFastTravel 0` until `EnableFastTravel 1`, or until
   the player goes from an interior out to an exterior, unless its keep flag was set
   [documented: GECK wiki EnableFastTravel; `fnv_game` keeps the flags]. Text
   `sNoFastTravelScriptBlock`.
2. **The place forbids it**: the worldspace's `DATA` flag "Can't Fast Travel" (bit 1) or
   the cell. Text `sNoFastTravelCell`. In FalloutNV.esm the worldspace flag is set on
   GamorrahWorld, TheFortWorld and Lucky38World; Dead Money sets it on its eight Sierra
   Madre worldspaces [observed].
   - **An interior forbids it**: after `EnableFastTravel 1` too, a player "inside an
     interior cell" is "still … not permitted to fast travel" [documented: GECK wiki
     EnableFastTravel]; the eligibility check includes "restrictions on interior
     departures" [documented: GECK wiki GetPCCanFastTravelWithMessage]. A player reports
     the same of the original (#3434).
   - **CELL `DATA` bit 2** is named two ways. "Invert Fast Travel Behavior" [documented:
     GECK wiki GetCellFlag and SetCellFlag, whose example sets it on the interior
     Gomorrah00; fopdoc's Fallout 3 and New Vegas CELL pages; xEdit 4.1.5's FNV
     definitions]; "Can't Travel From Here", "check to disable fast travel from this
     cell" [documented: GECK wiki Common Data, the editor's checkbox; xEdit 4.1.6 and
     `schemas/fnv/world.json`]. For Skyrim the same bit is *not* Can't Travel From Here,
     interiors only [documented: UESP, Skyrim's CELL page; another game]. NVForever takes
     it as the inversion: set, an interior allows travel and an exterior forbids it
     [hypothesis: the names agree if the editor's box stores the difference from the
     kind's default; observer check 285]. No record settles it: none of the ten masters'
     CELL records, 516 of interiors and 44,076 of exteriors, sets it [observed: a
     throw-away reader of every CELL `DATA`, 2026-11-23], so no interior of the game
     allows travel by the data. Every map marker, and the reference each links to for the
     arrival, is in a worldspace [observed: the same reader, the base game and the four
     story DLCs].
   - An interior that behaves like an exterior (bit 7, "a 'fake exterior', with no
     landscape but sky and weather" [documented: GECK wiki Cell Lighting]) counts as an
     exterior here [hypothesis]; the only one is Dead Money's test cell
     NVDLC01stealthTest [observed].
   - A worldspace's flag forbids travel in all its cells, whatever the cell flag
     [hypothesis]. Both are read as SetCellFlag and SetWorldspaceFlag left them [design].
   - The rule is `fnv_game::fast_travel::place_forbids`, which the map and
     GetPCCanFastTravel share. The player's tweak Fast Travel From Anywhere lifts it for
     both, and for `Game::fast_travel` (docs/design/tweaks.md). The map's other travel
     tweaks (No Fast Travel, in the Air, Near Enemies, Encumbered, No Travel on a Crippled
     Leg, Costs a Nuka-Cola) change the map's answer only [not built for the scripts'
     check].
3. **In the air**: the player's collision body is not on the ground (falling,
   jumping). Text `sNoFastTravelInAir`.
4. **In combat**: a hostile actor near has detected the player — the music's battle
   test (`crate::music`, `MusicCues::combat`) [hypothesis: the player's own combat state
   is not modelled apart from it]. Text `sNoFastTravelCombat`.
5. **Enemies near**: a living hostile actor (`fnv_game::rest::hostile_to_player`)
   within `fHostileActorExteriorDistance` / `fHostileActorInteriorDistance` — the Wait
   menu's test and its distances (wait-menu.md §9) [hypothesis: the same settings serve
   both]. Text `sNoFastTravelHostileActorsNear`.
6. **Over-encumbered** [documented: GECK wiki EnableFastTravel, "if they are
   over-encumbered … they will still be not permitted to fast travel"]: the carried
   weight over the limit, outside god mode (as `crate::player::encumbered_walks`). Text
   `sNoFastTravelOverencumbered`. The perk entry "Has Fast Travel Always" (Long Haul)
   lifts this [documented: GECK wiki perk entry points]; not built.

Not built: the alarm refusal (`sNoFastTravelAlarm`: no alarm state is modelled), the
health refusal (`sFastTravelNoTravelHealthDamage`: what it checks is not known),
Long Haul, and the scripted checks of the script extenders (GetPCCanFastTravel).

## 3. The question box

The interface file of the message box says it draws "on top of the Pipboy"
[documented: message_menu.xml, a comment on its list's clipping], so the question is a
message box over the Pip-Boy's screen. On the screenshot it stands in the middle of the
screen, a black box with a thin line all round, the question centred at the top and
the two answers centred under it, the chosen one ("Yes") in a box [observed]. The text
is the setting `sTravelQuestion` [documented: the settings list; the text is not in the
plugins], which reads "Do you want to travel to Goodsprings?" for Goodsprings
[observed: screenshot]; the answers "Yes" and "No" [observed].

NVForever [design unless marked]: the box is drawn into the DATA page over the map, in
the Pip-Boy's amber, by the screenshot's look: 520 wide, centred on the map window,
the text in font 2 and the answers under it, the chosen one framed as the message box
frames its button (`message-menu.md` §2: the line at brightness 200, the fill at 40
opacity). Each answer's row is its text and 20, the text at the row's bottom, as the message box's buttons (`message-menu.md` §2) [design: the rows were 16 more than the text with the text halfway, which set the word high in its frame (a screenshot from play, 2026-11-07)]. It opens on Yes [observed: the screenshot]. Up and Down (W and S) move the
choice, Enter, Space or E take it, Y and N answer directly; the pointer chooses the
answer it is on and a click takes it. While it is up the map does not pan or follow the
pointer. A refusal is the same box with its text and an "OK" [hypothesis: the original
answers a click with a box; not seen].

## 4. The trip

**The destination** [documented: GECK wiki SetOnFastTravelEventHandler: the reference
the player is sent to "may or may not be an actual Map Marker", the real marker being
linked to it]: a map marker's linked reference (`XLKR`) is where the player arrives.
182 of the 189 named map markers in FalloutNV.esm have one [observed]; Goodsprings'
marker 0x000CDF51 links 0x001658B7, an `XMarkerHeading` named
`GoodspringsArrivalMarker` [observed]. Without one the player arrives at the marker
itself [hypothesis]. The player is put on the ground by the arrival rule every travel
uses (`crate::phys`, simulation.md §7), facing the destination's heading.

**The loading.** The trip goes as every door's does: the Pip-Boy is put away and a
`TravelRequest` brings the loading screen up (`crate::loading`) [design: the same
path for doors, scripts and the console].

**The clock** [hypothesis: no source read gives the original's rule]: the game time the
player would take to walk the straight line from where they stand to the destination at
the observed walking speed (`MoveSpeeds::OBSERVED.walk`, 86.8 units a second) at the
game's TimeScale (30 when the plugins lack it): hours = distance / 86.8 × TimeScale /
3600. Goodsprings' marker to Primm's (about 57,000 units) is about 5.5 hours, to
Novac's (about 100,000) about 9.6 [observed: the markers' places]. The hours
pass for the player's effects and needs (`fnv_game::Game::pass_hours`) and, a whole
hour at a time, for the people (`wait::HourPassed`, as after a wait) [design]. A trip
within one cell's width (4096 units) passes no time [design].

**Followers** [documented: GECK wiki, "Changing cells will teleport the teammate to the
player"]: the teammates following the player come along, put beside them
(`fnv_game::Game::bring_teammates`, as through a load door).

**The people at the destination** [observed, owner, issue #1208]: they are already where
their package has them on arrival (Boone at the dinosaur, fast travelling to Novac at
5am), not walking there. The residents near the destination that the player did not see
in the last game hour are put at their package's place behind the loading screen; those
the player saw stay put (docs/spec-notes/packages.md §8).

## 5. Checking it

Pip-Boy test aid steps (`NVF_PLAY_TEST_PIPBOY`): `travel=<marker FormID>` does what a
click on that marker of the World Map does; `key=use` takes the chosen answer, `key=down`
moves to No. The log says "fast travel: …" for the question, the refusal (with its
reason) and the trip ("fast travel: to … at …, N.NN hours on the clock"). Smoke
scenarios `fast-travel-*` in `tools/smoke/scenarios.toml`.

## 6. Open

The original's travel-time rule and its settings; what CELL `DATA` bit 2 does (check
285); the box's exact size and fonts (the screenshot is the only picture); the
refusals' texts; random encounters on the way (none in New Vegas that we know of).
