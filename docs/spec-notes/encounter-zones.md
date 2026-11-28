# Spec notes: encounter zones and the level of leveled lists

ESM gap audit 2026-10-21, priority 1 (`docs/handoff/esm-gap-audit-2026-10-21.md`). Built in
`fnv_sim::zones` (rules), `fnv_game::zones` (cell loads), `fnv_actor::resolve` (drawn
actors, `resolve_changed_at`), `engine/nvforever/src/zones.rs` (the engine's copy).

## 1. Data

- **ECZN** `DATA`, 8 bytes: owner (FormID, NPC_ or FACT), rank (i8), minimum level (i8),
  flags (u8: bit 0 "Never Resets", bit 1 "Match PC Below Minimum Level"), one unused byte
  [documented: xEdit wbDefinitionsFNV.pas ECZN; GECK wiki GetZoneFlag/SetZoneFlag name the
  two bits]. Schema `world.json` ECZN.
  [observed: FalloutNV.esm has 17 ECZN, every `DATA` 8 bytes, owner 0 in all, rank 0 or
  −1, minimum levels 0–22, flags 0 or 1 (no zone sets bit 1).] Examples: Vault3Zone 12,
  RepconHQZone 10, HooverDamZone 10, Vault11Zone 6, BisonSteve 2, GomorrahZone 22.
- **XEZN** (FormID of an ECZN) on CELL, WRLD and placed references (REFR, ACHR, ACRE, …)
  [documented: xEdit; schema `common.json`]. [observed: FalloutNV.esm has it on 84 cells,
  5 ACHR and 11 ACRE (most of the HELIOS One robots), on no WRLD.]
- **XLCM** "Level Modifier" (i32) on placed leveled actors [documented: xEdit wbXLCM]; the
  GECK's Leveled Actor tab offers None, Easy, Medium, Hard, Boss [documented: GECK wiki
  Reference]. [observed: values 0–3 on 297 placed actors of FalloutNV.esm; read as 0 Easy,
  1 Medium, 2 Hard, 3 Very Hard (Boss) after xEdit's enum; no XLCM is "None".]
- **A reference's zone**: its own XEZN, else its cell's, else its worldspace's
  (`RefTable::zone_of`, `cell_zone`) [documented: GECK wiki Reference, "What Encounter
  Zone is this reference attached to?"; hypothesis: the order, and that a worldspace's
  zone covers cells naming none]. The authored cell counts, not where the actor walked to.

## 2. A zone's level

- Set **the first time the player enters it** and kept for good [documented: GECK wiki
  Bethsoft Tutorial NPC population, "When the player enters an encounter zone for the first
  time, the level of that zone is set permanently"].
- The level: the player's level, raised to the zone's minimum [documented: same page, "if
  a second-level player enters an Encounter Zone with a min. level of 10, leveled lists in
  the Zone will spawn actors as though the player were level 10"]. With "Match PC Below
  Minimum Level" a player under the minimum gets the zone at their own level [hypothesis:
  read from the flag's name; no FNV zone sets it]. No maximum level (FNV's ECZN has none).
- "Entering" is the zone's cell coming into the game's loaded cells (the player's cell, or
  the 5 × 5 around it outdoors; `Game::set_loaded`) [hypothesis: the game makes a cell's
  leveled actors when it loads them]. A reference with a zone of its own enters that zone
  when its cell loads.
- Saved: `fnv.sim.state/1` `zone_levels` (absent when empty).
- **Optional tweak (`bNoLockEncounterZoneLevels`, off by default):** [documented: lStewieAl's
  Tweaks 10.05 (`https://www.nexusmods.com/newvegas/mods/66347`) `nvse_stewie_tweaks.ini`
  `[Tweaks]`: `; don't freeze the level of encounter zones when they are first visited` /
  `bNoLockEncounterZoneLevels = 0`; no DLL or source consulted]. When
  `Tweaks::no_lock_encounter_zone_levels` (`tweak-no-lock-encounter-zone-levels`) is enabled,
  entering or querying a zone recomputes its level from the player's current level
  (`level_on_entry`, still respecting `min_level` and "Match PC Below Minimum Level" including
  session `SetZoneMinLevel` / `SetZoneFlag` bit-2 overrides) instead of keeping the first-visit
  level frozen (`World::enter_zone`, `World::level_at_with_zone_lock`, `Game::zone_level` /
  `GetZoneLevel`); already-fixed references keep their fixed `ref_levels` until a cell reset,
  and toggling the option off leaves unmodded vanilla locking unchanged [design: NVForever's own
  runtime integration; the toggle itself is a setting and is not saved in `Snapshot`].

## 3. The level a placed reference reads leveled lists at

- Its zone's level; with no zone, the player's level [documented: GECK wiki Reference,
  Leveled Actor: "If there is no encounter zone, which is common in the wasteland, the
  player's level is used"].
- Changed by XLCM [documented: GECK wiki Reference]: Easy 50 % "All entries in the leveled
  list at or below that level are available"; Medium 75 %, Hard 125 %, Boss 200 % "Only
  the closest entry in the leveled list, without exceeding the value, will be selected";
  None "A random entry equal to or below the calculated level".
  [hypothesis: rounded down, never below 1. Easy forces "all levels"; None, Medium, Hard
  and Boss keep the list's own "Calculate from all levels" flag (the wiki's None and
  Medium/Hard/Boss texts read like the flag on and off; the list's flag is kept because
  the leveled-item captures, simulation.md §4, show the flag deciding).]
- The level is **fixed for the reference when its cell first loads**
  (`World::fix_level`, saved as `ref_levels`) [hypothesis: an actor made at a load stays
  the same actor after the player levels up, until its cell is reset]. Before that, the
  rules give the level the load would fix (its effective zone's level — including session
  `SetRefEncounterZone`, `SetCellEncounterZone`, `SetWorldspaceEncounterZone`,
  `SetZoneMinLevel`, and `SetZoneFlag` bit 2 overrides stored in `WorldSession` — or the
  level that zone would take now, or the player's level), so a query before the load
  agrees with it; `GetInZone` (`functions/refs.rs`) also reads `World::cell_zone_of`.
  `SetZoneMinLevel`, `SetZoneOwner`, and `SetZoneFlag` bit 2 are session-only (not saved
  in `.fos` or `.nvse` cosaves), persist across same-process baseline and mutated save
  loads, and revert to ESM defaults on fresh process restart [observed: Fallout: New Vegas
  1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 + AnhNVSE 131, 2026-11-24, 10 official base
  masters, `GSDocMitchellHouse` (`0x00103DF9`), isolated clean-room save/reload probe
  `w4f_08_zone_setters` (`SetZoneMinLevel 23`, `SetZoneOwner 0x00003130`, `SetZoneFlag 2
  1` on `Vault11Zone` `0x000E9081`): baseline `min=6,owner=00000000,fl2=0`, mutated
  `23,00003130,1`, same-process baseline/mutated reload `23,00003130,1`, fresh-process
  with/without `.nvse` `6,00000000,0`; zero DLL/PDB/source consulted]; the same
  session-only lifetime for `SetRefEncounterZone`, `SetCellEncounterZone`, and
  `SetWorldspaceEncounterZone` remains unprobed [hypothesis: carried across same-run loads
  via `WorldSession` / `Game::Session`, excluded from snapshots].
- What reads it: the actor a placed leveled actor list (LVLN/LVLC) stands for, and every
  `TPLT` chain through a list (factions, packages, name, scripts, actor values, spells,
  `fnv_game::templates`; the drawn actor, `fnv_actor::resolve`; the same seed as before,
  so only the level changes the pick); and the carried items of placed actors and
  containers (leveled item lists, `World::roll_inventory`) [documented: GECK wiki
  LeveledItem: in a container the player's level; "on an actor ... the actor's level";
  hypothesis: both are read as the reference's level here — a zone's level for a
  container in a zone, the modified level for an actor; the actor's own level (PC level
  multiplier, `Game::actor_level`) is not used for its items].
- A cell reset (`fnv_game::respawn`) fixes the level of the actors it brings back again,
  at the player's level then (a zone's level does not change); refilled containers too.
- Scripts' AddItem of a leveled list reads the player's current level [observed:
  simulation.md §4, `L4.sim.leveled.basic`], and so do the carried items of references
  made at run time (PlaceAtMe, PlaceLeveledActorAtMe) [hypothesis: simulation.md §6].
- The carried items of a reference with no fixed level read the level its cell's load
  would fix now (`World::item_level`, `World::level_at`) [design]: its zone's level, or
  the level the zone would take if entered now, else the player's level, changed by its
  level modifier. That covers an actor or container read before its cell loads, a
  merchant chest's restocks included. What the game does for these is not seen
  [hypothesis].
- Locks do not follow this: a leveled lock hardens from a fixed level only, so a door,
  which gets none, keeps its authored level (`World::lock_read_level`, simulation.md §5)
  [design].
- The world holds the player's level for item rolls apart from its configuration, and does
  not save it (`World::set_player_level`, `World::roll_level`): `Game::sync_player_level`
  writes it on SetLevel, on a level-up, on every tick and in `fnv_game::Game::restore`. In
  the engine that restore holds only a stand-in world, so the engine's load
  (`Runtime::restore_save`) calls it again on the played world before anything runs
  [design]. The static viewer (no `--play`) keeps the display level 1.

## 4. Never Resets

- A cell whose zone has "Never Resets" is never reset: its dead Respawn actors stay dead
  and its respawning containers keep what they hold (`World::attach_cell`,
  `Game::cell_entered`) [documented: GECK wiki SetZoneRespawns, "Creatures will no longer
  respawn in the Yao Gaui Den zone"; GECK wiki Reference, references "will respawn
  according to the Encounter Zone setting"]. The zone's own level never resets either way
  (§2). `SetZoneRespawns` (`0x11EA`, saved in `World::zone_respawns`), session
  `SetZoneFlag` bit 1 (`nvse_lutana`), `GetZoneFlag`, `GetZoneRespawns`
  (`nvse_showoff2`) and `Game::cell_never_resets` (`respawn.rs`, also
  consulting `SetCellEncounterZone` / `SetWorldspaceEncounterZone`, and passed
  to `World::attach_cell_with_never_resets` during `Game::cell_entered` so
  respawning containers and actors obey the same effective flag) all read the
  same effective `never_resets` state through `Game::zone`; calling
  `SetZoneRespawns` supersedes any earlier session `SetZoneFlag` bit-1 override
  [design; hypothesis: original-game interaction between `SetZoneRespawns` and
  extender `SetZoneFlag` / `GetZoneRespawns`].

## 5. Engine

- The engine draws actors from its own copy of the records; `zones.rs` copies the game's
  level of every placed actor (`Game::leveled_at`) into `ActorWorld::levels` whenever a
  zone was entered, references were fixed, or the player's level changed, and the drawn
  actor is resolved at it (`resolve_changed_at`, `resolve_creature_at`). Without `--play`
  every actor is drawn at the display level 1 as before.
- Log lines (`nvf-play` script runtime, at each cell change): `zones: encounter zone
  <EDID> (<id>) entered: level L (minimum M, player level P)`, and for each placed leveled
  list in a zone `zones: leveled actor <ref> (<list>) at level L[ (all levels)] in zone
  <EDID> stands for <base> (<base at level 1, or "the same"> at level 1)` (outside zones
  at debug level). Smoke scenario `encounter-zone-level`.

## 6. Functions

- `GetInZone zone` (0x11BE): 1 when the calling reference's current cell has the zone
  (CELL XEZN, else its worldspace's) [documented: GECK wiki GetInZone; hypothesis: a
  reference's own XEZN does not count, the wiki speaks of cells].

## 7. Not built, open

- Leveled locks: REFR XLOC "Leveled" [documented: GECK wiki Bethsoft Tutorial Non-NPC
  population, "The 'leveled' checkbox in the lock tab is a deprecated feature: leave it
  unchecked"; Reference: "will start at the base level specified and become harder as
  the player levels" — no rule for how much is documented]. Built 2026-10-24 with a
  guessed rule, from a level fixed for the reference only (simulation.md §5); observer
  check 88 asks whether the original's door locks change at all.
- The game settings that bound how far below the level an entry may be
  (`iLevItemLevelDifferenceMax` and the like) are not read; the "highest eligible level"
  pool makes them matter only with "Calculate from all levels".
- Zone ownership (owner, rank) is read but unused (crime rules).
- Captures wanted: a zone's level after leaving and coming back at a higher level
  (level-design.md §13 item 9); whether XLCM Medium/Hard/Boss ignore the list's flag;
  whether an actor outside zones re-rolls on level-up without a cell reset.
