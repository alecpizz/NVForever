# JohnnyGuitar NVSE, batch 9 (2026-11-09, #1007)

Sounds played from a file's path, the scripted holotapes, terminal menu items added and
removed, the audio markers, the player's and actors' moves, and the values scripts read
from the engine, in `crates/fnv-game/src/nvse_jg9.rs`. Sources: the GECK wiki export
[documented: the page named, in our own words]; the data files [observed: fnvtool esm
inventory, bsa list]. Not probed yet: probe set `jg9` (`tools/capture/jip_probes.py`)
and lab scenario `L5.ext.jg9` are written, not run. Tests: `crates/fnv-game/tests/jg9.rs`;
smoke scenario `jg9-script-sounds-moves`.

31 functions. Census on this branch: JohnnyGuitar 109 → 78 (47 with batch 8, #1918).

## 1. Sounds from files

A path is "relative to the New Vegas folder" [documented: PlaySoundFile]. NVForever plays
Data's files only: `Data\` is taken off and the rest is looked up as the archives spell
it, in any case; a path outside Data, with a drive or `..`, plays nothing and returns 1
[design]. `PlaySoundFile` returns 1 even when the file does not exist [observed: Fallout:
New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528 + JIP LN 57.30, 2026-11-23,
`GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, read-only `PrintToConsole`
probe (zero writes to game install or Saves), `b3_jg9_01_sound_runspeed_compass_pipboy`
(`tools/capture/nvse/catalogs/wave4b_extenders.json`, #3679):
`PROBE|b3_jg9_01_sound_runspeed_compass_pipboy|ps=1,ss=0,rsP=338.8,rsD=338.8,hostDir=-1,pb=0`;
zero DLL, PDB, or third-party source consulted]. `PlaySoundFromPath` likewise returns 1
for a missing file [observed: the original, 2026-11-10].

Each file a script plays gets a sound of its own for the session: a form ID from
`0xFFF00000` up, one per file, source and looping [engineering choice]. The engine's sfx
(`nvf-play` sfx.rs) registers it as a SOUN with that file, no flags but Loop, the GECK's
default curve and a reach of 100 to 2,000 units [engineering choice: the pages name no
reach], and from then on plays it as a script's SOUN: IsSoundPlaying, StopSound and the
engine's answers work for it.

| Function | NVForever |
|---|---|
| PlaySoundFile `path [forcePlay] [shouldLoop] [playInMainMenu]` | plays the file, not placed; forcePlay and playInMainMenu let it play in menu mode too [hypothesis] |
| StopSoundFile | stops what PlaySoundFile plays [documented] |
| PlaySoundFromPath `path [fadeTime] [isVoice] [systemSound] [shouldLoop]` | plays the file, fading in over fadeTime seconds; isVoice changes nothing [hypothesis: the voice volume is not applied] |
| PlaySound3DFromPath `path [fadeTime] [isVoice] [shouldLoop]` | at the calling reference [documented] |
| StopSoundFromPath `path [fadeTime]` | stops "all instances" of the file [documented], fading out over fadeTime; 0 [observed: with the file just played, §7; hypothesis: it returns nothing] |
| StopSound3DFromPath `path [fadeTime]` | those from the calling reference [documented] |
| PlaySoundFade `sound fadeTime` | a SOUN at the calling reference, the player without one [documented], faded in |
| StopSoundLooping `sound` | the page stops it "by triggering its Loop End point"; NVForever plays no loop points, so what a script plays of it stops [hypothesis] |

A fade raises or lowers the playing's volume in a straight line [hypothesis: the pages
give no curve] (`ScriptFade`); a sound faded out is gone at its end.

## 2. Holotapes

PlayHolotape `note [playStartStopSound]` plays a note that has something to play, as one
picked in the Pip-Boy's Notes list (`nvf-play` holotape.rs); the note playing goes on
rather than pausing, as picking it again would [hypothesis]. StopHolotape
`[playStartStopSound]` (1 by default [documented]) ends the tape, without the end script
of a Voice note's line [hypothesis: the page says only that it stops]. The start/stop
sounds are UIPipBoyHolotapeStart and UIPipBoyHolotapeStop [observed: FalloutNV.esm SOUN
0007DFF4, 0007DFF5]. A form that is no note with a tape returns 0.

## 3. Terminal menu items

AddTerminalMenuItem `terminal` adds a new, empty item [documented], at the end
[observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528 with the 10 official base masters, clean-room script probe jg_33_add_remove_terminal_menu_item (AddTerminalMenuItem/RemoveTerminalMenuItem): 1,1; no DLL or source consulted]. RemoveTerminalMenuItem `terminal index` removes one [documented]; the items
after it move up, with a script's changes to them (Set…TerminalMenuItem…, `nvse_jg3`).
What stands at each position is kept for the session (`terminal::slots`); every reader of
a terminal's items goes through it. An open menu holds positions, so a removal also asks
for the redraw RefreshTerminalMenu gives: otherwise picking an item the menu still shows
would run the one that moved into its place (#2018) [design: whether the original
redraws by itself is not observed]. RefreshTerminalMenu (`rtm`) reads the open menu's
list again [documented: "Redraws Terminal menu if it's currently open"]; with no
terminal open it does nothing. It returns 0 [hypothesis: the page gives no value].

## 4. Audio markers

The engine's music reads every audio marker from the plugins (REFR MMRK, XRDS, CNAM, MNAM,
NNAM; `fnv_world::audio`), tells the game their values once, and tells it every frame
which marker's music plays (`nvf-play` music.rs `script_markers`).

| Function | NVForever |
|---|---|
| AudioMarkerGetController (AMKGetCtrl) | the marker's media location controller [documented] |
| AudioMarkerGetCurrent (AMKGetCur) | the marker whose music plays now [documented] |
| AudioMarkerGetProperty (AMKGetProp) `n` | 0 radius, 1 layer 2 trigger %, 2 layer 3 trigger % [documented]; the triggers as stored, fractions [hypothesis] |
| AudioMarkerSetController (AMKSetCtrl) `controller`, AudioMarkerSetProperty (AMKSetProp) `n value` | for the session; the music picks from the changed markers from then on |

A loaded game starts with the plugins' values again (a script's changes last the
session, as the other batches' state). Without the engine (fnvtool) no marker is known.

## 5. Moves

| Function | NVForever |
|---|---|
| Jump | the actor jumps [documented]: the player as with its Jump key, another actor as for a ledge hop, both only from the ground |
| SetAutoMove `0/1` | the player's Auto Move [documented] |
| SetPlayerMovementFlags `flags` | the player moves as the flags say in the next frame: 1 forward, 2 back, 4 left, 8 right, 256 walking, 512 running, 1024 sneaking, 4096 jumping [documented: the flags; hypothesis: one frame, as the keys would] |
| SetVelEx `x y z` | the three axes of SetVel at once [documented]: a character goes up or down at z and is pushed along the ground at (x, y) as the console's SetVel pushes it; a loose object's bodies take all three |
| GetLinearVelocity `node xOut yOut zOut` | "in units of 0.1 Meter/sec, or 7 game Units/sec" [documented]: the reference's velocity as GetActorVelocity follows it (`nvse_jip2`), over 7; the node's own body is not told apart [hypothesis] |
| GetRunSpeed | the run speed the engine's pace gave the actor (people's, creatures'; the player's run with its Speed Mult and perks), units per second; 0 for one not drawn [hypothesis] |

The engine carries these out in `nvf-play` script_input.rs `script_moves`, after the
controls are read.

## 6. Values the engine tells

| Function | NVForever |
|---|---|
| GetPlayerKarmaTitle `0/1` | the title or the tier [documented], as the Pip-Boy's stats page shows them; the table and functions moved to `fnv_game::karma_titles` |
| GetItemEffectString `item` | the item card's effect line for the player [documented]: an aid item's changes as taking it would make them, else its enchantment's (apparel); the Pip-Boy's text, moved to `fnv_game::item_text` |
| GetCalculatedSpread | the actor's spread now [documented]: the player's as the crosshair has it, another's at its last shot; 0 before any |
| GetGrenadeHoldTime | seconds the player has held a thrown weapon's trigger, kept after the throw until the next hold [documented] (`nvf-play` combat.rs) |
| GetNearestCompassHostileDirection `[skipInvisible]` | 0 front … 7 front left [documented], eight equal sectors about the heading [hypothesis]; -1 with no hostile mark [observed, §7]; invisible is Invisibility or Chameleon above 0 (`nvf-play` compass.rs) |
| GetCurrentSkyColor `type rOut gOut bOut` | the sky's colour of that type now [documented: 0 sky upper … 9 clouds upper], as normalized `0.0..=1.0` floats (populated in interiors too, returning `1`), by the weather colour types' order [hypothesis: the page's ids are NAM0's]; an out-of-range type (`< 0` or `>= 10`) returns `0` and leaves the output variables unchanged [observed: 2026-11-24, Fallout: New Vegas 1.4.0.525 (`040020D0`) + xNVSE 6.4.9 + JohnnyGuitarNVSE 528 + JIP LN 57.30 (`5730`), 10 official base masters, isolated clean-room staging copy (read-only `PrintToConsole` probe; zero writes to game install or Saves), `GSDocMitchellHouse` (`0x00103DF9`, interior) and Goodsprings (`0x000CDF51`, exterior), committed probes `w6_t1_09_jg_sky_color_interior` and `w6_t1_15b_raycast_ref_mat_reticle_sky` (`tools/capture/nvse/catalogs/wave6_t1_jg_sky_color.json`): `PROBE\|w6_t1_09_jg_sky_color_interior\|int=1\|t0=1(0.286,0.267,0.188)\|t9=1(1.000,1.000,1.000)\|t10=0(111.0,222.0,333.0)` and `sky0=1(0.281,0.356,0.622)\|sky10=0(r=111)`; no DLL, PDB, or third-party source consulted] (`nvforever` sky.rs) |
| GetPlayerCamFOV `0/1/2` | the world FOV, the first person's, or the one the camera uses now [documented], as GetCameraFOV reads them (a SetCameraFOV value, else the settings page's Field of view / Viewmodel field of view, else the INI's); the first person's until the engine tells the view [observed: 55 in the probe's save, §7] |
| GetPipBoyMode | 0 closed, 1/2 opening, 3 opened, 4/5 closing [documented: GECK wiki GetPipBoyMode; observed: 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528 + JIP LN 57.30 with the 10 official base masters, clean-room script probe `w4c_b3_06_toggle_pipboy_tab` (`tools/capture/nvse/catalogs/wave4c_extenders.json`, #3678 / PR #3679), toggling `TogglePipBoy 1003` and polling `GetPipBoyMode`: `init(pb=0)|open1003(mm=1003,pb=2)|close(mm=1003,pb=3)|reopenNoArg(mm=0,pb=0)`, plus 2026-11-23 `b3_jg9_01_sound_runspeed_compass_pipboy` (`wave4b_extenders.json`): `ps=1,ss=0,rsP=338.8,rsD=338.8,hostDir=-1,pb=0`; no DLL or source consulted]; `nvf-play` syncs opening (`2`, `Phase::Raising`) and closing (`4`, `Phase::Lowering`) via `Game::ui.pipboy_mode` [design], while resting `Down`/`Up` phases and headless `fnv-game` retain the live `Game::menu_open(menus::PIPBOY)` `3`/`0` fallback (any Pip-Boy page, including underneath a tutorial overlay) [not built: animation sub-phases 1/5] |

## 7. Observed (set jg9 at 5d1ab3447, 2026-11-10)

The observer session ran the set in the original (xNVSE 6.4.9, JohnnyGuitar 528, save
nvfairport); 4 of 11 rows matched [observed]. The golden is private.

| Row | Original | fnvtool then | Now |
|---|---|---|---|
| karma | `Drifter\|Neutral` | same | |
| effect.stimpak | `HP +40` | `HP +39` | the potency follows the player's Medicine, and fnvtool's player is not the save's; row `medicine` added to show both |
| effect.leather | `""` | same | |
| fov | `75 55 55` | `75 55 75` | the view is first person until the engine tells |
| term | `5 1 6 [] 1 5` | same | |
| sound.path | `1 1 0` | `1 0 1` | PlaySoundFromPath 1 for a missing file; StopSoundFromPath 0 |
| grenade | `0` | same | |
| compass | `-1 -1` | `8 8` | -1 with no mark |
| marker | did not compile | `00000000` | asked again through a ref variable |
| sky | did not compile | `0 0 0 0` | dropped: output variables do not compile in that build (as jg8's GetSystemColorAlt) |
| spread | `0.00 339` | `0.00 0` | GetRunSpeed is told by the engine, which fnvtool lacks; 339 against the 344 the engine measured running (`nvf-play` player.rs) is a follow-up (§8) |

The one rerun (set at 94a702c5c, 2026-11-10): all 11 rows compiled and 7 matched
(karma, effect.leather, fov, term, sound.path, grenade, compass) [observed]. The other four:

| Row | Original | fnvtool | Why |
|---|---|---|---|
| medicine | `17` | `15` | fnvtool's player is the plugins' default (every attribute 5: 2 + 2×5 + ⌈5×0.5⌉ = 15 [documented: GECK wiki Derived Skill Settings]); 17 fits the save's character with Intelligence 6 [hypothesis: the save's attributes were not read] |
| effect.stimpak | `HP +40` | `HP +39` | follows from medicine: 30 × (1 + 2 × 0.17) = 40.2 against 30 × 1.30 = 39 (`aid::potency`) |
| marker | `001676D0` | `00000000` | 001676D0 is the persistent audio marker (Audio Location 0x00166C03, radius 12000) around the save's place at the airport [observed]: the marker whose radius holds the player. The engine's music tells the current marker; fnvtool has none |
| spread | `0.00 339` | `0.00 0` | GetRunSpeed is told by the engine (§8) |

## 8. Follow-ups (engine)

A smoke check that AudioMarkerGetCurrent names the marker around the player at the airport (001676D0 in the original); GetRunSpeed's 339 for a standing player against the 344 units a second the engine measured running [observed both];
Loop End points for StopSoundLooping; a node's own body for GetLinearVelocity; the voice
channel's volume for isVoice.
