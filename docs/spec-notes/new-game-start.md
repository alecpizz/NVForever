# Spec note: the new-game start

NVForever's own reading of two public settings. This is not a claim copied from another
program. The game files it cites are the ones the player provides in their own game folder
(`Fallout_default.ini` and the file names in `Data\Video`); none is copied into NVForever.

## Quest

A new game's start quest, when `sCharGenQuest` is not set to something else, is form
`00102037`.

- [documented: Fallout Wiki, "Fallout: New Vegas quest IDs"] The row `00102037` is named
  "Welcome To Fabulous New Vegas". The stage table lists the same id with GECK ID `VCG00`,
  including the opening stages (hood removed, pistol whip).
- [documented: Fallout Wiki, "VCG00ScriptNV"] The script's editor ID starts quest `VCG00`.
  The script's own form id is `00102035`, which is not the quest.
- [documented: GECK Wiki, "INI Settings"] `SCharGenQuest` default value is `00102037`.
- [documented: `Fallout_default.ini` in the player's game folder] `SCharGenQuest=00102037`.
- [documented: Fallout Wiki quest list] `00104C1C` is "Ain't That a Kick in the Head"
  (`VCG01`), the quest after the opening, not the opening itself.
- [observed: `fnvtool esm decode` of FalloutNV.esm] It has no form `001FFFF8`, the start
  quest Tale of Two Wastelands' install guide gives (`TTW_QUEST` in
  `crates/nvf-launcher/src/start.rs`). The observation is recorded at `not_a_quest` in
  `engine/nvf-play/src/game_start.rs`.
- Without the key, NVForever starts `VCG00` (code: `DEFAULT_CHARGEN_QUEST` in
  `engine/nvf-play/src/game_start.rs`).

## Film

The intro a new game plays is `Data\Video\FNVIntro.bik`. It is the base game's own file,
in the game folder the player provides, not a mod's; NVForever ships no film.

- [documented: Fallout Wiki, "Fallout: New Vegas intro"] The opening cinematic plays before
  the first playable part. That page does not name the file.
- [observed: a Steam install's `Data\Video`, 2026-11-23] The folder holds `FNVIntro.bik` and
  no other file. It was written with the game's own files (with `FalloutNV.esm`, seconds
  apart), not by a mod.
- A mod can replace that file with its own `Video\FNVIntro.bik`. The game then plays the
  mod's film under the same name, so neither the launcher nor the INI changes (code:
  `open_film` in `engine/nvf-play/src/intro.rs` takes the loose file that wins among the
  data's layers, through `Vfs::loose_path`).
- [documented: `Fallout_default.ini` in the player's game folder; GECK Wiki, "INI Settings"]
  `sIntroMovie` is `Fallout INTRO Vsk.bik`. The `Data\Video` above has no such file.
- [owner, 2026-10-15] Without the key NVForever plays `FNVIntro.bik`, not the original's
  default `Fallout INTRO Vsk.bik`, which is Fallout 3's film; New Vegas does not ship it and
  TTW brings it (the module doc of `engine/nvf-play/src/game_start.rs`).
- [hypothesis] The opening's own call for `FNVIntro.bik` (VCG00 stage 0, a PlayBink) is a
  script's film and plays whatever `sIntroMovie` names, as in the original (same module doc).
- With no `sIntroMovie` line, NVForever plays `video\fnvintro.bik`, and an empty
  `sIntroMovie=` plays no film (code: `DEFAULT_INTRO` and `film_path` in
  `engine/nvf-play/src/game_start.rs`).

## What TTW off writes

TTW off sets the fields to `00102037` and `FNVIntro.bik`, with "No intro film" off. Save
writes those two lines into the home's ini. A missing key still means the same opening,
so the written lines match a fresh game (code: `GameStart::parse` in
`engine/nvf-play/src/game_start.rs` reads those values as quest `00102037` and film
`video\fnvintro.bik`).

## TTW's lines left in (#3229)

- Since #3229, New Game with a `sCharGenQuest` that names no quest of the load order (TTW's
  `001FFFF8` with TTW off or removed) starts VCG00 instead, logs it and shows a corner message
  naming the setting (code: `not_a_quest` and `fallback_notice` in
  `engine/nvf-play/src/game_start.rs`).
- #3229 asked the launcher to clear or flag those lines. The card flags them
  (`start::ttw_missing` in `crates/nvf-launcher/src/start.rs`), and TTW off clears them in
  one step: it writes the base game's two values above over TTW's.
