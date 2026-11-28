# Co-op players in saves (design)

Two ways a save can hold co-op, both NVForever's own design, not a behaviour of the
original (which has one player) [design]:

- **One save for several players** (§1–§4): a save keeps players 2–4 beside the game's
  own player. No mode plays them yet; the game and the engine keep what a save gave
  them and write it into the next save unchanged.
- **Network co-op** (§5): every player keeps its own solo save. A guest in the host's
  world saves its own game with its character as it is now, and the host's story
  reaches it as claimed stages it replays at home (docs/design/coop-shared-progress.md).

The two share the `Character` type and the rule that every co-op part is optional and
absent in a one-player save.

## 1. What is per player, what is shared

**Per player** (each of players 2–4 has its own copy; player 1 keeps the fields it had):

| Part | Player 1 (unchanged) | Players 2–4 (`fnv_game::coop::CoopPlayer`) |
|---|---|---|
| Actor: base form, cell, position, rotation, inventory (equipped items, condition, mods), S.P.E.C.I.A.L. the world holds, scale, equip locks | `sim.refs[0x00000014]` (`fnv_sim::world::RefState`) | `actor` (the same type) |
| Heading and view pitch | the save header's `place` | `heading_deg`, `pitch_deg` |
| Health and every other actor value (base, permanent and temporary modifiers, damage), dead, unconscious, factions, level | `extra[0x00000014]` (`RefExtra`) | `extra` (the same type) |
| Character: name, sex, look, race, level, XP, perks, tag skills and slots, skill points, need stages, misc statistics, kill counts, notes and holotapes, power armour training, Caravan cards | `player` (`PlayerState`) | `character` (`fnv_game::coop::Character`, those fields of `PlayerState`) |
| Magic effects active on the character (aid, abilities, worn enchantments, needs' and radiation effects) | `effects` with target the player | `effects` |
| Engine: first/third person and distance, sneak, always run, flashlight, magazine of the weapon in hand | `EngineSnapshot` | `EngineSnapshot::coop` (`nvf_play::save::CoopEngine`) |

**Shared** (saved once, as before): quests, script variables and instances, the world
(every other reference, globals, cells, form changes), dialogue topics and lines said,
reputation and crimes, factions the player is an enemy of, map markers and fast travel,
challenges, achievements, casino winnings and bans, the controls scripts disabled,
hardcore, the Pip-Boy radio and active quest, weather, time.

## 2. Format

All new fields are optional and absent in a one-player save, so the formats keep their
versions (`fnv.game.state/1`, `nvf.engine.state/1`, `nvf.save.header/1`): an older save
reads as one player, and a one-player save is byte-for-byte what it was.

- **Body, game part**: `GameSnapshot::coop`, a list of `CoopPlayer`, in slot order. Each
  has `slot` (2–4). A list naming a slot outside 2–4 or one slot twice fails the load
  (`fnv_game::coop::checked`).
- **Body, engine part**: `EngineSnapshot::coop`, a list of `CoopEngine` by the same slots.
- **Header**: `SaveHeader::coop`, `{slot, name, level}` per player, for the load list
  (filled from the body's characters when saving).

FormIDs in the new parts are written as everywhere else (`0x%08X` strings), so a save
loaded into another load order remaps them with the rest (`fnv_save::remap_json`); a
co-op player's effects of plugins no longer loaded are dropped
(`GameSnapshot::drop_gone`), and the magazines' weapons are remapped beside player 1's.

## 3. API (fnv-game)

- `Game::coop`: players 2–4 in slot order; `coop_player(slot)`, `set_coop_player(p)`
  (checks the slot), `remove_coop_player(slot)`.
- `CoopPlayer::copy_of_player(&game, slot)`: a player made as a copy of player 1 now (a
  starting point for a joining player, and the tests').
- Test: `coop_players_round_trip_apart_from_the_player` (crates/fnv-game/tests/synthetic.rs).

## 4. Left for the co-op mode

These are for the one-save mode; network co-op (§5) needs none of them.

- References for players 2–4 in the world (so scripts, AI and combat can name them), and
  what `ActiveEffect::target` of a co-op player's effect is then: today it is written as
  it was taken (player 1's ID) and means that character.
- Moving a character between player 1 and a co-op slot (who hosts a loaded save).
- Whether any shared part should be per player after all (reputation, map markers,
  the active quest): an owner decision.

## 5. Network co-op

Each player plays its own game and keeps its own saves; the host's world is the shared
one while a lobby is open (docs/design/coop-shared-progress.md, `nvf_coop`).

| Part | Where it is kept | Code |
|---|---|---|
| A guest's own game as it was when it joined (place, world, quests, time) | in memory while it is a guest | `nvf_play::save::KeepSolo` asks, `SoloBase` holds |
| A guest's save | its own game with the character as it is now: the place, world, quests and time are its own; inventory, values, level, XP and perks are what it has now | `save_now` with `SoloBase`, `GameSnapshot::with_character_of` |
| What the host's story gave or took (its quest items, the perks its scripts gave) | taken off the guest's save, except the unique rewards of the stages it claimed | `fnv_game::coop_story` (`CoopStory::undo_on`) |
| The character's stable ID | `PlayerState::character_id`, made at its first join as a guest | `Game::ensure_character_id` |
| Stages it claimed, reputation from its own acts in them, map markers it found | `PlayerState::claims` (`coop_joint::ClaimLedger`), replayed when its own world next loads, then cleared | `Game::replay_claims`, `nvf_play::save::replay_claims` |
| Which characters were there for each stage the host set | the host's save, `GameSnapshot::coop_presence` (`coop_joint::SessionPresence`) | `Game::coop_joint_stage` |
| Story gear checked in at a checkpoint (the Sierra Madre's) | the guest's save, `GameSnapshot::coop_checked`, `coop_checkpoint_gear` | `Game::coop_check_in`, `coop_check_out` |
| A guest's save request | never a save of the host's kind: at most the host's own autosave comes forward, once per two minutes | `nvf_coop::progress::SaveForward` |

The new fields are optional like §2's: `fnv.game.state/1` stays, a solo game never
writes them, and a load gives it none (test:
`network_co_ops_save_fields_are_absent_solo_and_round_trip_when_set`, beside §3's).
