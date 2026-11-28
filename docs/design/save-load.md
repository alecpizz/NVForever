# Save and load (design)

What NVForever must write to its own save file so that a session continues where it left
off, mapped against what the original game keeps in its `.fos` saves
([spec-notes/save-files.md](../spec-notes/save-files.md)), and the format. Built
2026-10-15 with the owner's go-ahead on the recommendations of §5; what is built and what
is not is §6.

Charter points that shape it: NVForever never writes into the game folder or
`Documents\My Games\FalloutNV` (install-layout.md), so saves go to
`Documents\My Games\NVForever\saves\` (the `saves\` folder that layout already reserves). We
do not read or write `.fos` files; our format is our own.

## 1. The principle: save the difference from the plugins

The original saves only what differs from the plugins: a changed-form record per form that
changed, each with flags saying which kinds of change it carries, plus a few global blocks
(globals, weather, player location, statistics) [observed, save-files.md §5–§6]. A 61-hour
game holds about 42,800 such records in 8 MB [observed, save M].

NVForever already works the same way. The sim copies a reference into `World.refs` the
first time it changes and leaves untouched references to the plugins; the game layer keeps
quest, script, actor and player state in maps keyed by form ID. So a save is:

1. a header (for the load menu and for checks before loading);
2. the plugin list it was made with;
3. the game snapshot (`GameSnapshot`, which holds the sim's `Snapshot`), extended with the
   engine-side state listed in §3;

and loading is: load the same plugins, build the world from them as for a new game, then
apply the snapshot.

## 2. What exists today

- `fnv_game::GameSnapshot` (`crates/fnv-game/src/game.rs`) and `Game::snapshot` /
  `Game::restore`. The reader accepts the baseline `fnv.game.state/1`, the retired-scene
  format, and a separate lambda-handle format for each. See
  [Callable identity](../spec-notes/callable-identity.md) for tags and compatibility rules.
  The game snapshot contains `fnv_sim::world::Snapshot` (`crates/fnv-sim/src/world.rs`,
  format `fnv.sim.state/1`) with `World::snapshot` / `restore`. Both derive serde and are
  written as JSON in tests.
- Round-trip tests: `crates/fnv-game/tests/dialogue.rs`, `functions.rs`, `synthetic.rs`,
  `engine/nvf-play/src/dropped/tests.rs`.
- serde and serde_json are already dependencies of fnv-game, fnv-sim, fnv-ai, fnv-combat,
  fnv-actor and nvf-play.
- (Before 2026-10-15) nothing wrote a save file and `Control::QuickSave` had no handler;
  §6 is what was built since.
- In nvf-play the `Game` lives in `ScriptState.rt` and the `World` in `SimState.world`,
  swapped into the game for each run (`engine/nvf-play/src/script.rs`): a save takes both
  at a point where the world is back in `SimState`.

## 3. State inventory

Columns: **Ours** is where NVForever holds it; **Original** is where the `.fos` keeps the
same thing (spec section; "not found" means it was not located in the bytes, not that the
original lacks it); **Save?**: *yes* (must be saved), *have* (already in the snapshot),
*rebuild* (comes back from the plugins or other saved state), *drop* (transient).

### 3.1 World and references (fnv-sim)

| State | Ours | Original | Save? |
|---|---|---|---|
| Reference position, rotation, cell | `RefState.pos/rot/cell` | REFR/ACHR "moved" data: cell ref + six f32 (§6.2) | have |
| Enabled / disabled, deleted | `RefState.enabled/deleted` | REFR flag bit 0, form flags word [hypothesis] | have |
| Container and actor inventories, condition, ownership, equipped | `RefState.inventory` (`Extra{condition, owner, equipped}`) | REFR bits 5/27 item lists with extra-data lists (§6.2) | have |
| Locks | `RefState.lock` | REFR bit 12 on doors [hypothesis] | have |
| Doors and containers open | `RefState.open` | REFR bit 28 animation data [hypothesis] | have |
| Created references (dropped, PlaceAtMe) | entries in `refs` with dynamic IDs, `next_dynamic` | kind-2 refs, created changed forms, next-created counter in global 1 (§5.1, §6.3) | have |
| Merchant restock time | `RefState.last_restock` | not found | have |
| Cell detach times (respawn/reset) | `Snapshot.detached_cells` | CELL records with hours-passed values; ECZN day stamps (§6.2, §6.3) | have |
| Global variables, including the calendar and TimeScale | `Snapshot.globals` (only differing ones) | global table 1 type 3: all 200 globals (§5.1) | have |
| Quest items set by script | `quest_objects` | not found | have |
| Leveled list additions | `leveled_added` | LVLI bit 31 (§6.2) | have |
| Sim RNG | `rng_word_pos` | none | have |
| Physics sleep/velocity | not in the sim; settled pose written back by `engine/nvf-play/src/phys.rs` | not found | drop: objects load at rest |

### 3.2 Game layer (fnv-game)

| State | Ours | Original | Save? |
|---|---|---|---|
| Quests: running/completed/failed, stages done, objectives, delay | `quests: QuestState` | QUST bits 1, 2, 29, 31 (§6.2) | have |
| Quest and reference script variables | `instances` | QUST bit 30, REFR bit 31 index + f64 pairs (§6.2) | have |
| Actor state: dead, actor values (base, permanent, damage, temporary), factions, packages, essential, level, scale | `extra: RefExtra` | ACHR/ACRE records (not decoded) and NPC_ base records (§6.3) | have |
| Hardcore needs, limb condition, karma | actor values in `extra` | player record (not decoded) | have |
| Player: XP, name, sex, level, perks and traits, tag skills | `player: PlayerState` | NPC_ 0x7 (SPECIAL, name), CLAS PlayerClass (§6.2), player record | have |
| Notes, map markers, topics known | `player.notes`, `map_markers`, `topics` | NOTE records, REFR form flags on markers [hypothesis] | have |
| Reputation (fame, infamy) | `player.reputation` | REPU bit 1, two f32 (§6.3) | have |
| Challenges, misc statistics, achievements | `player.challenges`, `misc_stats`, `achievements` | CHAL bit 1; global type 0 (43 u32, in code order: misc-stats.md §1) [observed: values fit the names] | have |
| Crime and enemies | `player.crime_counts`, `crime_enemies`, `enemy_of` | not found | have |
| Faction reactions, names, cell owners, item values changed by script | `forms: FormChanges` | FACT bit 2 (§6.2) | have |
| Form list additions | `form_lists` | FLST bit 31 | have |
| Dialogue: said-once, said-today | `said`, `said_day`, `said_day_days_clock` | INFO bit 31, no data (§6.2) [hypothesis] | have. A save without `said_day_days_clock` holds its said-today hours on an older clock (GameDaysPassed × 24 + GameHour) that cannot be converted: the load drops them, once, so those Say Once a Day lines are free again; `said` stays [design: `Game::restore`, #1579] |
| Active effects, image-space modifiers | `effects`, `image_modifiers` | player/actor records [hypothesis] | have |
| Broken locks | `locks` | not found | have |
| Script clock, frame count, RNG | `time`, `frame`, `rng_word_pos` | none | have |
| Terminals hacked / locked out | `terminals: Terminals` | not found | **yes: add** |
| Script-forced weather | `weather: WeatherState` ("not saved yet") | global type 8 (§5.1) | **yes: add** |
| Pip-Boy radio on/station, NPC radios | `radio: Radios` (`pipboy`, `npc_radios`) | global type 10 holds a song path (§6.3) [hypothesis] | **yes: add** those two fields |
| Long-lived provocations | `provocations.grudges/forgiven` | not found | **yes: add** (cheap) |
| Package scan time | `package_scan_due` | none | rebuild (reset on load) |
| Trespass warnings | `crimes.warnings/trespassing` | not found | drop |
| Request queues, animations, detection values, detection events, Wait/Sleep in progress | marked "Not saved" in `Game` | none | drop |
| Detection states: each actor's state (Normal/Alert/Combat/Lost), alert and target lists, the target fought; StartCombat's forced pairs | `awareness` (`awareness.actors`, `awareness.forced`) | not found | **saved since 2026-11-01** (#971): a fight goes on after a load, and IsInCombat on the player reads the same before the first detection round; the next round measures again as before. Combat groups' tactics, barks and the engine's engage/aim/crouch hints are rebuilt |
| Who has had OnStartCombat and not yet OnCombatEnd | `fighting` | not found | **saved** (#1578): a load runs no OnStartCombat for a fight the save holds [documented: spec-notes/ext-save.md §4], a start still due at the save runs once, and the fight's OnCombatEnd runs when it ends. A created reference dropped on a load (§4.1) leaves it with its detection state. Saves from before this field did not record it, and that history cannot be recovered: their actors in Combat or Lost count as started, Alert does not, so a start still due in such a save does not run [design: `Game::restore`] |
| StartCombat targets not measured since (#3390) | `scripted_combat` | not found | **saved** whenever there are forced pairs, empty too. A save with forced pairs and without it is older: there a target still holding the value StartCombat gave it counts as not measured [design: `Game::restore_scripted_combat`] |
| Config | `cfg: GameConfig` | none | rebuild from settings (seed: see §5) |

### 3.3 AI (fnv-ai, used by `engine/nvforever`)

| State | Ours | Original | Save? |
|---|---|---|---|
| NPC positions | written back into `RefState` through `Game::set_actor_place` | ACHR moved data | have |
| Current package, goal met, chosen-at | `RefExtra.packages/current_package/...` | ACHR flags 18 etc. (not decoded) | have |
| A ResetAI not yet taken by the AI | `RefExtra.ai_reset` (`Game::take_ai_reset`) | not decoded | have (since issue #2119): a save between the call and the AI's next update keeps the request, so the load cannot resume the progress it dropped |
| Agent progress: patrol index, sandbox schedule (energy, rolled sleep/meal times), wait, furniture in use | `Agent` (`crates/fnv-ai/src/carry.rs`, `sandbox.rs`) | ACHR records change with time on every actor (§7 of the spec) | **saved since 2026-10-25** (`fnv_ai::progress`, engine save part `ai_progress`): the patrol point walked to and the sandbox's day (its random numbers, the day's sleep and meal times, energy), put back when the actor takes up the same package again; the rest rebuilt from the packages |
| Process levels, furniture occupancy | `Ai` | none | drop |

### 3.4 Engine (engine/nvf-play, engine/nvforever)

| State | Ours | Original | Save? |
|---|---|---|---|
| Player position and cell | written into the player's `RefState` each frame | global type 1: worldspace, cell grid, cell, position (§5.1) | have |
| Current worldspace / interior | `Location{world, interior}` | global type 1 | rebuild from the player's cell |
| View pitch | `Player.pitch` | not found | **yes: add** |
| First/third person, camera distance | `PlayerControl.pov/distance` | not found | **yes: add** |
| Sneaking, always-run | `PlayerControl.sneak/always_run` | not found | **yes: add** (weapon out is already in `RefExtra`) |
| Rounds in the clip | `Combat.cycle.magazine` | player record [hypothesis] | **yes: add** |
| Flashlight (Pip-Boy light) | `Flashlight.on` | not found | **yes: add** |
| Active quest in the Pip-Boy | `pipboy/data.rs` `DataState.active_quest` | global type 1 quest ref [hypothesis] | **yes: add** |
| Sitting / lying | `PlayerSeat`; NPCs' seats in `Furnishings`/`OnFurniture` (engine/nvforever/src/furniture.rs); the use scripts read in `RefExtra::furniture` | not found | rebuild: a load stands the player and every NPC up (furniture stays free): `Game::restore` clears each actor's saved furniture use (`Game::stand_everyone_up`), so GetSitting and its kin read 0 until the engine seats the body again and reports it (`furniture: … goes to` / `is on`) [design, #1573 for the player, #2797 for NPCs]. The original keeps the seat: Easy Pete saved seated reads GetSitting 3 at his saved spot from the first sample after the load [observed 2026-11-25, observer check 220]; ours differs |
| Current sky weather and fade | `SkyState` (engine/nvforever) | global type 8: two weather refs (§5.1) | **yes: add** the current and previous weather; the fade restarts |
| Corpse poses | `Ragdolls.rest` | REFR/ACHR moved data [hypothesis] | optional: the corpse's `RefState.pos` is enough at first |
| Music, radio playback position | `Music`, `Radio` | global type 10 [hypothesis] | drop: start a new track |
| Menus, messages, dialogue screen, hacking/lockpick sessions, Wait menu, physics world, drawn items | Bevy resources | none | drop (see §5 on saving inside menus) |
| Settings, key bindings, volumes | `Settings`, `Bindings` etc. | the INI files, not the save | **not in the save**: a separate preferences file (`NVForever.ini` in the home folder) |
| Hotkeys | not built | not found | later, when hotkeys exist |

## 4. Format (recommendation)

One file per save, `saves\<name>.nvfsave`:

```
magic   "NVFSAVE\0"            8 bytes
version u32                     container version (1)
header  u32 length + JSON      SaveHeader (below), readable without the body
shot    u32 length + PNG/WebP  thumbnail for the load menu (optional, may be 0)
body    u32 length + zstd(JSON or postcard) of SaveBody
```

- `SaveHeader` (serde): save format version, NVForever version and git hash, real time
  saved, play time, game date and hour, player name, level, karma title, location name,
  current cell/worldspace, the plugin list (file names and sizes or hashes) and a slot kind
  (manual, quick, auto). The load menu reads only this and the thumbnail, as the original's
  header carries name, level, location and play time [observed, spec §3].
- `SaveBody` (serde): `GameSnapshot` (with the sim snapshot inside) plus a new
  `EngineSnapshot` struct for the §3.4 rows and an `AiSnapshot` if §3.3 is taken up.
- Each snapshot keeps its own `format` string (`fnv.game.state/N`, `fnv.sim.state/N`).
  New fields use `#[serde(default)]` so older saves still load (already the pattern in
  both snapshots); a change that cannot default bumps N and adds a migration function
  from N−1, so old saves stay loadable. `RefExtra::killing_hit` is one: the dead of an
  older save have no killing hit, as nothing in the save tells it [design].
  `RefState::made_at` (where a created reference was made, its editor location, #2278;
  saved since 0.8.0) is another, with a fill-in on the load: a created actor of an older
  save, which has none, takes the place it was saved at (`World::made_at_from_saved_place`,
  #3042) [decision: the nearest fact the save holds, an approximation of where it was
  made, since combat, following or a scripted MoveTo may have moved it before the save].
  N stays: every actor `World::place` makes has one, so a missing one marks the
  older save; dropped items have none by design and get none.
- Encoding: JSON keeps saves inspectable and is what the tests already use; size is the
  question. Measure a long session's snapshot first: if compressed JSON stays in the low
  megabytes (the original's 61-hour save is 8 MB uncompressed), keep JSON; otherwise
  switch the body to postcard or bincode behind the same container version. The
  compression crate (zstd or similar) and any binary encoder are new dependencies, to be
  added to docs/design/dependencies.md with the licence check there.
- Writing is atomic: write `<name>.nvfsave.tmp`, flush, rename over the old file; the
  previous file of a rolling slot (quick, auto) becomes `.bak`, as the original does with
  `autosave.fos.bak` [observed: the backups held `.fos` and `.fos.bak` pairs]. The backup
  is made as a second name for the old file (a hard link, or a copy), so the save never
  leaves its own name, and one save is written at a time, so two saves to one slot a
  frame apart cannot share the `.tmp` (#1982).
- A name the player gives is refused when it is a Windows device name (`con`, `nul`,
  `com1` …, on every system) or a rolling slot's own name (#1983).

Form IDs in the snapshot are load-order form IDs (mod index = plugin position). The
original stores a plugin list and resolves form IDs by position in it [observed: save M,
spec §6.3]. On load, compare the save's plugin list with the current one: equal is the
normal case; a reordered list is remapped by name (rewrite the high byte of every form
ID); a missing plugin drops the forms from it with a warning. Dynamic (created) IDs are
ours and never remapped.

Built (2026-10-18): equal lists, and plugins added after all of the save's
(`fnv_save::PluginCheck::Appended`: every saved form ID keeps its plugin's place, so the
save loads as it is and the added plugins' objects appear, a save holding only the
differences). Built 2026-10-21 (queue item 29): any other list, §4.1.

### 4.1 Loading into another load order

Facts. A form ID's top byte is its plugin's position in the load order [documented: GECK
wiki FormID, "the first two digits of a FormID correspond with the position of the data
file from which the form originates in your load order", up to 255 files, `FF` for forms
made in play]. Form IDs therefore change when the load order changes [documented: GECK
wiki GetFormIDString, "FormIDs may change on game load, dependent on load order"; the
xNVSE function ValidateModIndex resolves "a mod index which was stored during a previous
game session to account for any changes to the user's load order", or fails when "the mod
is no longer loaded"]. The original's saves hold the plugin list they were made with and
their form IDs index it [observed: save-files.md §4, §6.3]. What the original shows when
plugins are missing is not documented in the GECK wiki export (no setting in
`FalloutNV.esm` names it; its strings would be in the encrypted executable).

What NVForever does (`fnv_save::PluginRemap`, `fnv_save::remap_json`,
`fnv_game::game::GameSnapshot::drop_gone`, `nvf_play::save::read_for_load`):

1. The save's plugins are matched to the current ones by file name, case ignored. Each
   saved index maps to the plugin's index now, or to *missing*.
2. If every index maps to itself (same list, or plugins appended) the body loads as it is.
3. Otherwise every form ID in the body is rewritten: its top byte becomes the plugin's
   index now; created forms (`FF`) and 0 ("none") stay. Form IDs are found by how the
   snapshots write them: strings `0x%08X` (values and map keys) and script variables'
   `{"ref": n}`; the engine part's raw numbers (weapon in hand, active quest) and the
   modules' parts (`SaveHook::remap`: map markers found, reputation titles shown) are
   remapped by their owners.
4. A form of a missing plugin: where it is a map key, a list entry, or in a pair or tuple
   that is a list entry, that entry is removed (a reference of the plugin's, an item in an
   inventory, a quest's state, a global, a form-list entry, a remembered pair).
   A tuple that is a field's value keeps its length, or the save could not be read
   back: one of form IDs beside other values (`package_ends`) is known by its shape, one
   of form IDs alone by its field's name (`fnv_game::game::SAVE_PAIR_FIELDS`) (#1981).
   Anywhere else (a field, such a tuple) it is moved to the *gone* slot `FE` with its low bytes, which no
   plugin uses; then the game layer drops what stands on one: references whose base or
   cell is gone (created ones of the plugin's bases), script instances whose key or script
   is gone, magic effects whose effect, source, target or script is gone, the radio station
   and weather shown. Other fields keep the gone ID, which names no form, as a script's
   reference to a form that does not exist [hypothesis: the original's handling is not
   documented].
5. The player's place: if its worldspace or interior cell is from a missing plugin the
   load is refused (there is nowhere to start); otherwise it is remapped before the world
   opens.
6. A load order of 255 plugins uses the `FE` slot: a save with missing plugins is then
   refused, saying so [hypothesis: our choice, rather than let gone forms alias a real one].
7. The run's output says what happened, e.g. `load: <save> was made with another load
   order: moved ["DeadMoney.esm 01->02", ...], missing now [...], added [...]; N FormIDs
   remapped, M entries of missing plugins dropped ({per plugin}), K fields named a missing
   plugin's form, J entries holding them dropped by the game layer`. When plugins are
   missing the player gets a corner message as the game comes up: "This save relies on
   content that is no longer present: <plugins>. What came from it was removed." (also in
   the console) [hypothesis: our wording and our choice to load with a notice rather than
   ask; the original's is not documented].

Smoke `save-load-order` (run after `save-write`): the round-trip save made with the ten
masters loads with `--plugins FalloutNV.esm GunRunnersArsenal.esm DeadMoney.esm`
[observed 2026-10-21: 455 FormIDs remapped, 536 entries of Honest Hearts, Old World Blues
and Lonesome Road dropped, 36 more by the game layer; the base game's changes back].

## 5. Decisions (owner 2026-10-15: the recommendations below)

1. **When saving is allowed**: in the world, from the pause menu and from the console.
   Not in a conversation, another menu, a script's menu or message box, or under a
   loading screen; a save asked for then says why not, an autosave waits for its moment.
   The snapshot's `menu`, `message_box`, `dialogue` and `speech` are therefore empty in
   every save. Saving happens at the end of a frame, after the script frame's effects
   reached the world, so no request a script made is still queued.
2. **Autosaves**: after each loading screen (door travel, `coc`, a script moving the
   player) and when a script calls `Autosave` [documented: GECK wiki Autosave], each with
   its own switch on the settings page (registered in group Saving, shown under
   Gameplay › Saving: settings-page.md), both on by default. Three more
   switches there, on by default [owner request 2026-10-27]: Save on fast travel (after
   a fast travel's loading screen, in place of the travel switch), Save on wait and Save
   on sleep (when the menu closes with at least one hour passed, also after Cancel). Their timing
   against the original is still to be checked with the owner, who turned the
   original's autosaves off in its INI.
3. **Random numbers**: a load re-seeds (a reload does not replay the same rolls); test aid
   `NVF_PLAY_TEST_SAVE_SAME_ROLLS=1` keeps the saved positions.
4. **AI continuity**: packages start again from where each actor stands. Since
   2026-10-25 a patrol goes on to the point the save had it walk to (one that does not
   repeat started again at its first point and walked back), as the original does
   [observed 2026-11-01, observer check 24, o23_town victor_save and o22_reload: Victor
   (enabled by script) on VGSVictorCircuitPatrol, 11 points, not repeating, saved just
   after point 5 on his way to point 6; after loading that save he went on to points
   6-10 and his home marker and stopped there, the same path as the run that was not
   reloaded, never back to point 1] and a sandbox keeps the
   day's times and its random numbers (§3.3); a wait at a patrol point, walks under way,
   furniture and errands start again.
5. **Physics**: objects load at rest where they were.
6. **Another load order**: forms are remapped by name, those of missing plugins dropped
   with a notice; built 2026-10-21 (§4.1).
7. **Importing a `.fos`**: the game runtime never reads or writes `.fos` files (the
   runtime stays `.nvfsave`-only). An accepted offline
   `fnvtool save convert <input.fos> --out <output.nvfsave>` (and
   `fnvtool save inspect-fos <input.fos>`) utility converts the decoded `.fos` subset
   (`crates/fnv-save/src/fos.rs`, `crates/fnv-game/src/fos_convert.rs`) into a standard
   `NVFSAVE\0` v1 save and emits a structured JSON report separating `converted`,
   `hypothesis_fields` (every `[hypothesis]` `.fos` field written, per
   `docs/spec-notes/save-files.md` §6.6), and `skipped` (`.nvse` co-saves, unsupported
   created `0xFF` kinds such as `ACRE`/`ACHR`, unpersisted item hotkeys `0x4A`,
   perks/active effects, and unmapped changed-form/global layouts).
8. **Thumbnail**: 512×288 from the main camera; built (§6), shown on the pause menu's load
   page.

## 6. What is built

- **`crates/fnv-save`**: the container of §4 (magic, version, header JSON, thumbnail,
  body JSON), atomic writes (`.tmp` then rename; quick and auto slots keep the previous
  file as `.bak`), names kept to safe characters, every path through
  `fnv_install::guard_write`, the save list newest first, the plugin-list check. The body
  is JSON: a short session's save is about 140 KB; measure a long game before choosing a
  binary encoding.
- **Game layer** (fields added to the baseline `fnv.game.state/1` with `#[serde(default)]`):
  terminals hacked or locked out, the Pip-Boy radio (on, station),
  NPC radios, the weather shown and rain, grudges and forgiveness. `Autosave` is no longer
  a stub: it asks the engine for an autosave (`UiRequests::autosaves`); nor are SystemSave
  (the one `systemsave` slot) and ForceSave (a new save as by hand)
  (`UiRequests::system_saves`, `force_saves`; docs/spec-notes/script-saves-and-reticle.md).
- **Engine** (`engine/nvf-play/src/save.rs`): `SaveRequest`, the QuickSave (F5) and
  QuickLoad (F9) keys, autosaves, `EngineSnapshot` (`nvf.engine.state/1`: first/third
  person and distance, sneak, always run, flashlight, the magazine of the weapon in hand,
  the Pip-Boy's active quest, and parts of other crates through `SaveHook`: nvforever's
  sky saves its weather, whether a script holds it and the weather rolls' random state).
  The body is
  `nvf.save.body/1` = game snapshot + engine snapshot.
- **Loading**: `nvforever --load <name|path>` reads the header first (its worldspace,
  cell, position, heading, pitch, hour, day and weather start the world there; the weather is
  kept while the weather system lists it there, not pinned as `--weather` pins it,
  spec-notes/world.md §6), then the body
  once the load order is known (remapped if it differs, §4.1); the script runtime restores the
  game and the simulation as it is made, before its first frame, and sends `RefChanged`
  for every reference the save changed so the drawn world follows; the engine part is put
  back next. Those `RefChanged` are marked `restored`: visuals and collision follow them
  as any, while crime and the world's sounds take what they name as judged and heard
  before the save, so a load judges no theft and plays no pick-up or drop sound. It is a
  mark on each notice and not a gate on the frames of the restore, so a real pick-up or a
  load script's change in those frames has its normal effect [design:
  `crime::report_deeds`, `world_sounds::objects`; smoke `save-theft-write`,
  `save-theft-load`].
  A `MoveToMarkerWithFade` the save caught during its fade is made in the
  restore, on the world that is played, before any script of the load runs; the reference
  is drawn where it ends up, without the fade, and its `RefChanged` is marked `restored`
  too: the move was asked for before the save [design: `Game::restore`,
  `Runtime::restore_save`; smoke `save-fade-move-write`, `save-fade-move-load`].
  The sky takes the save's hold and the rolls' random state as it is
  made, before its first look at the weather lists, not when the engine part is put back:
  a hold set after that look would hold what its roll chose, and setting them later would
  undo a load script's release [design: `SkyState::restore_saved`; smoke
  `save-weather-write`, `save-weather-load`]. The game's clock stays the save's: the residents' start clock goes only into
  a new game, so `--day` never changes a loaded game's GameDaysPassed [design:
  `residents::Core::prepare`; smoke `save-days-write`, `save-days-load`]. The sky alone
  starts on the header's `game_days_passed`, over `--day` (`start_days` in `main.rs`), so
  its weather rolls see no hours pass at the load. A save made before the header held the
  day leaves the sky on `--day` or the plugins' day; it takes the game's day once the game's
  hour has moved half a game minute (about a second of play at TimeScale 30, not while
  paused or at TimeScale 0), with up to 24
  volatility checks at once, which can make one weather roll and replace the weather kept
  from the save [design: `SkyState::follow_climate`, `SkyState::follow_game_clock`]. The
  sky does not follow the game under two flags: with
  `--still-sky` its day stays the one it started on, with `--time-scale` its own clock
  counts on from there. Loading while playing (console `load`, F9, the pause and
  main menus) starts NVForever again with the same arguments and `--load` (those the save decides left out),
  so a save always lands in a world built fresh from the plugins and nothing of the run
  before carries over.
- **The change of run is hidden** (`engine/nvforever/src/handoff.rs`) [design, owner
  2026-10-16: the window closing and another opening was jarring; owner 2026-11-01,
  #1357: the two runs no longer overlap for the whole load]: the old run puts a loading
  screen up (its picture one for the save's place) and holds the game, then starts the
  new run with `--handoff <file>` and its window's place and size (`--window-at`,
  `--size`, `--window-maximized`), and updates only 20 times a second while it waits. The
  new run opens its window hidden; as soon as its own loading screen (or the main menu)
  is up and drawn for 0.25 s, it shows its window over the old one, takes the focus and
  writes the file, and loads behind its screen alone. The old run sees the file, takes it
  away, writes its exit logs and ends the process at once (not taking its world apart,
  which held its memory about 2 s more). The new run makes its game and puts the save
  back only once the file is gone (or 2 s after writing it, for an old run that died):
  on a busy machine the screen's first frames were slow enough for the save to go back
  before the window was shown, the old run still holding its game (#1651). The pause menu's return to the main menu (`--menu`) goes the
  same way. If the new run ends before it is shown (it stops at the start), the screen
  comes down and the old game goes on; a new run that fails later is on its own (its
  crash report). Closing the old window while it waits ends the new run; a new run
  still hidden after 120 s shows itself. The old run never waits for ever (#1903, a
  Linux report of two games playing from one Continue: the new run loaded with its
  window shown and never took part): the new run writes a marker as it starts, and the
  old run ends itself, leaving the new run to play, when the new run is running without
  one after 30 s, or has not said it is up 135 s after it started. The old run logs the
  new run's whole command line. The new run gets none of the old run's test aids
  (`NVF_PLAY_TEST_*`) except `NVF_PLAY_TEST_HANDOFF_EXIT=1` (end once it has taken over
  and its world or main menu is in; smoke `load-handoff`; the smoke runner waits for the
  processes a launch starts).
- **A save that cannot be loaded** [owner 2026-11-01] (made with plugins that cannot be
  loaded now, damaged, of another format; nvforever `start_from_save`): the run opens the
  main menu instead, and once its window is up a message box says the save could not be
  loaded and why (none in unattended runs; the log line `load: <why>; opening the main
  menu` carries it). The same for `--load` given by hand and for a load while playing
  (smoke `load-handoff-refused`). All that the restore itself checks (the formats of the
  body, game, simulation and engine parts, both random-number positions, the co-op
  players) is asked as the save is read, before the world starts at its place
  [design: `nvf_play::save::read_for_load`, `GameSnapshot::check`; smoke
  `save-refused-write`, `save-refused-load`]. No game ever goes on from a save that failed:
  should a restore still fail after that read, the run keeps no runtime (no script runs,
  no engine part is put back, nothing can be saved, the loading screen stays up) and
  starts again on the main menu with the same message [design: `nvf_play::save::LoadRefused`,
  `--load-refused`; smoke `load-refused-late`, through a test aid, as no save reaches it now].
- **Memory** (#1357, load-handoff scenario at Goodsprings, private bytes): one run's
  start peak was 6.9 GB, 3.3 GB of it the script data build holding every record's read
  at once (now read a slice at a time, `fnv_game::data`): 4.1 GB. Old and new run
  together during a load: 10.5 GB before, 7.6 GB now, for the seconds until the new
  window shows; the new run grows to its full size in that time. Holding the new run's
  script build until the swap took the peak to 6.1 GB but the load from 6-12 s to 16 s:
  not done.
- **Console**: `save <name>`, `load [name]` (the quicksave without a name), `saves` (the
  list: name, level, place, game date, kind).
- **Thumbnail** (`save.rs`): the snapshot is taken at the end of the frame as before; the
  render world blits the main 3D view's post-processed texture into a temporary RGBA8 sRGB
  capture texture in `Core3d` after `Core3dSystems::PostProcess` and before `ui_pass` and
  reads it back asynchronously after `RenderGraphSystems::Submit`, so the thumbnail has no
  HUD or menus and no UI node is hidden or blinks on screen (#3808). When the picture
  arrives, a writer thread takes its middle 16:9, scales it to 512×288 by area averaging,
  encodes an RGB PNG (`png` crate, the version Bevy already builds) and writes the file.
  Measured in the smoke run: the frame spends about 1 ms on the snapshot; the file is on
  disk about 65 ms later; a thumbnail is 35–300 KB. No picture within 30 frames: the save
  is written without one. A load (the restart) and quitting wait for saves in flight.
- **Pause menu** (`pause.rs`): *Save* (N) opens the save page listing *New Save* (next
  numbered slot `Save <n>`) followed by existing manual saves newest first; choosing *New
  Save* writes a new slot, while choosing an existing manual save asks to confirm
  overwriting it (defaulting to No; Cancel leaves the existing file untouched, Yes
  overwrites through `SaveRequest` with the existing atomic writer safeguards) [design:
  #3758]. *Load* (L) lists all saves newest first (name, kind, game date per row; the
  chosen save's thumbnail, player and level, place, game date, play time and when it was
  saved), Up/Down, the wheel or the pointer choose, Enter or a click load through
  `LoadRequest`. Only the chosen save's thumbnail is read. Our own layout; nothing of the
  original's menus is copied.
- **Smoke**: `save-write` also checks that its saves carry a 512×288 thumbnail, saves from
  the pause menu, and that the load page lists the saves newest first with their
  thumbnails (test aid `NVF_PLAY_TEST_PAUSE_MENU`).
- **Checks**: `crates/fnv-save` unit tests; the game snapshot round trip in
  `crates/fnv-game/tests/synthetic.rs` covers the new fields and an older snapshot without
  them; smoke `save-write` (changes through the console, a manual save, an autosave after
  `coc` and one from `Autosave`) then `save-load` (`--load`, and the changes are back:
  items, a disabled reference, an actor value, the position).
- **Open: one frozen load run.** One `save-load` run of about 95 (2026-10-15) froze the
  main loop in the frame after the two Gun Runners' Arsenal message boxes closed
  [observed: its run log ends there; only the music decoder, on the audio thread, logged
  for the next four minutes]. 91 later runs, 20 of them with the CPU kept busy, did not
  freeze, so the cause is not known. A smoke launch that runs past its time now leaves
  every thread's stack in `<launch>.stacks.txt` (`tools/smoke/smoke/stacks.py`), so the
  next one shows where it stopped. The GRA boxes showing again after this load is
  expected: `save-write` saves before that quest shows them.

### 6.0 Co-op players (2026-10-26)

Saves keep players 2–4 of a co-op game beside the game's own player: each one's actor
(inventory, place), actor values and health, character progression, effects and engine
state; quests and the world stay shared. Optional fields, so the format versions stay.
Design: [coop-save](coop-save.md).

### 6.1 Deleting a save (2026-10-19)

On the pause menu's load page (also from the main menu's Load Game) Delete, or a click on
"Delete  Del)" right of the thumbnail, asks "Delete this save?" with "Yes  Y)" and "No
N)" under it. Yes (Y, Enter, a click) removes the chosen save's `.nvfsave` and the `.bak`
its slot kept, then lists the saves again; No (N, Esc, Backspace, a click) keeps it. Only
files with the saves' extension in the saves folder are removed [design: the original's
load and save menus carry a Delete hint (`load_and_save_menus.xml`, `save_delete_text`,
shown with a controller); the key, the question and its place are NVForever's]. Smoke
`save-delete-console`.

Not built: renaming saves from the menu, in-place loading
without a new run (by design, see above), AI agent progress, corpse poses beyond their position, the sitting or
lying pose (the player and every NPC stand up, #1573, #2797), playback positions of music and radio.
