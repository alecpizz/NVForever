# Install layout: NVForever runs apart from the original game

Owner requirement: NVForever must not run from the game's folder or interact with it
beyond reading the game data.

## Three separate places

| Place | Example (Windows) | Who writes | Contents |
|---|---|---|---|
| **Original game** | `F:\SteamLibrary\steamapps\common\Fallout New Vegas` | Steam only | `Data\*.esm`, `*.bsa`, loose files: **read-only input** for NVForever |
| **NVForever program** | `C:\Games\NVForever\` (any folder outside the game) | installer / you | `nvforever.exe`, `fnvtool.exe`, `schemas\` |
| **NVForever home** | `Documents\My Games\NVForever\` (or `NVFOREVER_HOME`) | NVForever | `NVForever.ini`, `plugins.txt` (its own load order), `saves\`, `logs\` (run logs, crash reports), `mods\` |
| NVForever cache | `%LOCALAPPDATA%\NVForever\cache\` (`<home>\cache` with `NVFOREVER_HOME`) | NVForever | generated LOD; can be deleted |

The home is beside the original game's own `My Games\FalloutNV` (owner request
2026-10-16), never inside it. The cache stays in Local AppData [our choice]: it can grow to
gigabytes and Documents is often synced (OneDrive).

The program folder is written by NVForever only on the player's Download DLSS... in the
settings: the DLSS folder `<program>\dlss\` (or `NVF_DLSS_DIR`), through a staging folder
beside it ([dlss-upscaling.md](dlss-upscaling.md), "Download"), never the game's folder.

Never touched at all: the original game's user folder
`%USERPROFILE%\Documents\My Games\FalloutNV` (its `Fallout.ini`, `FalloutPrefs.ini`,
`FalloutCustom.ini` and saves) and its plugin list `%LOCALAPPDATA%\FalloutNV\plugins.txt`
[observed: there, not in `My Games`, on the owner's PC]. NVForever keeps its own settings,
saves and load order, so playing either game never changes the other.

The one exception: a start from Mod Organizer 2 reads that `plugins.txt` path, where MO2's
hook shows the program its profile's list instead of the game's file (next section).

## First start (`engine/nvforever/src/setup.rs`)

The game's folder comes from, in order:

1. `--data` / `NVFOREVER_DATA` (developers and the smoke tests): never stored, never a
   dialog, so a scripted run fails instead of waiting.
2. `NVForever.ini` in the home (an existing file whose name differs only in case, such as
   `nvforever.ini` on a case-sensitive system, is the one read and written;
   `EngineHome::config_file`):
   ```ini
   [Game]
   Path=F:\SteamLibrary\steamapps\common\Fallout New Vegas
   ```
3. Otherwise NVForever asks: a welcome box, then the system's folder picker (opened at
   Steam's or GOG's default folder when one exists). The player picks the **game's root
   folder** (the one holding `FalloutNV.exe` and `Data`; picking `Data` itself is accepted
   and stored as its parent). A folder that cannot be used is refused with the reason and
   the picker comes back; Cancel ends the start. The folder is then stored in the INI.

A stored folder that no longer works (moved, files missing) brings back the picker with the
reason. Saving the INI changes only `Path` and keeps other lines and comments.

A player's start is `nvforever.exe` with no arguments (double-clicked, or through
`tools/release/nvforever.cmd` / `.sh`): the folder comes from step 2 or 3, then the game
opens on its main menu, as `--play --menu` (the rule and its list of world and mode
arguments: `engine/nvforever/src/main.rs`, `MODE_ARGS`). The schemas are read from
`schemas/` beside the program when they are there, as in the release archive.

The settings page's switches (pause menu and main menu) are kept in the same INI, one
`key=1` / `key=0` line each under `[Settings]` (`nvf_play::settings`). Every flip writes
all of them; each start, including the restart that loads a save, sets the listed
switches to their stored values, so the INI wins over the command line. Unattended runs
(test aids) neither read nor write the INI of the player's own home.

One more `[Settings]` line is read at start only and never written: `cpu-cache-placement`,
`auto` (the default) or `off`. On a processor whose L3 cache is split into domains of
different sizes (dual-CCD X3D Ryzens), `auto` runs the game on the largest one; every
other machine is left as it is; `off` turns it off. `--cpu-cache-placement auto|off`
wins over it for one run. The rules and measurements: frame-rate-audit.md §6
(`engine/nvforever/src/cpu_placement.rs`, #1842).

The player's keys are kept there too, under `[Controls]` (`nvf_play::bindings`): one line
per control by the game's control name, the key and mouse button by name
(`Forward=W`, `Block=Left Alt, Right Mouse`, `Auto Move=None`), plus `Always Run By
Default`, `Invert Y` and `Mouse Sensitivity`. Each start reads them and writes any line
an existing INI lacks, so the file lists every control to edit by hand; the settings
page's Controls tab rebinds a control and writes the section again.
`Mouse Sensitivity` is radians of turn a mouse count (the game's `fMouseSensitivity`
0.0020 is 100%); the Controls tab's slider runs from 5% to 300% of it on a logarithmic
meter with steps about a tenth apart (`bindings::SENSITIVITY_STEPS`) [design: #1451,
floor lowered from 25% for 1600 dpi mice in #1632]. A hand-edited value under 5% (or a
`[Controller]` `Stick Sensitivity` under 0.05) is raised to it when read, not refused;
values above 300% are kept. `Mouse Vertical Sensitivity` (`[Controls]`) and `Stick Vertical
Sensitivity` (`[Controller]`) are each device's vertical look as a multiple of its
horizontal, two decimals, 1.00 as the game has it, on the same steps and floor [design:
#2201]; an INI without them reads 1.00 and gets the lines written. `Mouse Aim Sensitivity` in
`[Controls]` and `Stick Aim Sensitivity` in `[Controller]` is each device's look while
aiming down the sights or through a scope as a multiple of its unzoomed look, kept and read
the same way (1.00 leaves the look as unzoomed) [design: owner 2026-11-15].

[design, #2200; `nvf_play::bindings`] `[Controls]` also keeps `Input Device` as
`Keyboard and Mouse` or `Controller` and `Automatic Input Switching` as `1` (default)
or `0`. The Controls tab puts the switch beside Input Device. With it on, fresh use of
the other device changes and saves Input Device (controller.md ?1); an existing INI
without the switch keeps its device value and gains `Automatic Input Switching=1`.

What a new game starts with is kept under `[General]`, by the original's own key names so
a mod's INI lines can be copied in (`nvf_play::game_start`): `sCharGenQuest` is the quest
New Game starts, by FormID (`001FFFF8`) or Editor ID, VCG00 without the key (the
original's `00102037`); `sIntroMovie` is the film New Game plays from `Data\Video`, none
when empty, `FNVIntro.bik` without the key. Tale of Two Wastelands asks for

```ini
[General]
SCharGenQuest=001FFFF8
SIntroMovie=
```

The launcher's Mods page writes these two lines for the player with one box (launcher.md,
Mods). Both are read once at start and never written by the game; unattended runs read them only under their
own `NVFOREVER_HOME`.

These lines go in NVForever's own `NVForever.ini`, not the original's
`Fallout.ini` / `FalloutCustom.ini` (NVForever never reads or writes those — see above), and
the start quest must be in NVForever's own load order. A quest named here but not loaded
(e.g. TTW's `001FFFF8` when Tale of Two Wastelands is not turned on in `<home>\plugins.txt`)
is not started: New Game logs a warning and leaves the player at the New Vegas opening, so
a missing TTW start looks like the INI line being ignored (issue #1394).

**Before every start** the folder must pass (`fnv_install::GameInstall`):

- `Data\FalloutNV.esm` exists (the folder is a game install at all);
- every file of `REQUIRED_DATA_FILES` is in `Data` and not empty: `FalloutNV.esm`, the six
  archives of the game's `SArchiveList` (`fnv_vfs::DEFAULT_ARCHIVE_LIST`, a test keeps the
  two lists together) and `Update.bsa` [observed in the owner's 1.4.0.525 install]. Names
  match without case. The message names each missing file;
- the separation rules below.

The add-on masters (Dead Money … Caravan Pack) are optional: with `--data` and without
`--plugins` the game loads the ones present and says which are missing.

## Plugins: NVForever's own list (`crates/fnv-install/src/plugins.rs`)

Which plugins load, and in what order, is `<home>\plugins.txt` (owner decision 2026-10-16:
NVForever's own list, not the original game's). Every `.esm`/`.esp` in the game's `Data`
has a line; `*` in front turns it on; the lines' order is the load order; `;` and `#`
start comments. Format and rules are our choice:

- The first start writes it: the game's masters present (`FalloutNV.esm`, then the add-on
  masters in their usual order) on, every other plugin in `Data` off. The original game's
  list is not read, so a mod on there is not on here until the player turns it on.
- At each start a plugin new in `Data` gets a line: off (a game master: on); a new `.esm`
  after the last `.esm` line, a new `.esp` at the end [hypothesis: the extension stands
  for the header's master flag].
- Lines are never removed. A plugin on in the list but no longer in `Data` is skipped
  with a warning; `FalloutNV.esm` always loads first, whatever the file says.
- A plugin on whose master is off stops the start with the load order's error naming
  both (`fnv_esm::LoadOrder`).
- Only a player's start reads it: `--plugins` replaces it, and `--data` (developers, the
  smoke tests) keeps the fixed masters so a run never depends on a player's list.

The original game orders its plugins by the files' modified dates, not by its list
[hypothesis: modding tools' documentation, not checked]; NVForever uses the lines' order.

### Started from Mod Organizer 2 (`crates/fnv-install/src/mo2.rs`)

MO2 starts programs with its usvfs hook, which merges the profile's mods into `Data` and
puts the profile's `plugins.txt` at `%LOCALAPPDATA%\FalloutNV\plugins.txt` [documented:
MO2 and modorganizer-game_gamebryo source, cited in `mo2.rs`]. A player's start that finds
`usvfs_x64.dll` loaded in its own process and MO2's header line at the top of that file
loads the plugins listed there, in their order (MO2's right pane), skipping any not in
`Data`, with `FalloutNV.esm` first. The home's `plugins.txt` is then not read or written.
Such a start also runs on the game folder MO2's instance manages
(`gamePath` in the instance's `ModOrganizer.ini`; modlists often manage a copy of the game;
a `@ByteArray(...)` value whose escapes, `\\` and `\xNN` for the UTF-8 bytes of a folder name
outside ASCII, are read as Qt writes them [hypothesis: only `\\` seen, #1996]),
not the INI's, and does not store it. Without the hook, or without MO2's header, nothing changes. Player guide:
docs/mod-organizer-2.md.

On the first start of the new home, the settings and saves of the earlier home
(`%LOCALAPPDATA%\NVForever`, builds before 2026-10-16) are **copied** over once (the old
folder is left alone; saves already in the new home are not replaced). `nvforever.json`
there is still read when no INI exists.

## Rules enforced in code (`crates/fnv-install`)

- `GameInstall::open` accepts the install root or its `Data` folder and requires
  `Data\FalloutNV.esm`. Game files are only ever opened for reading.
- `check_separation` fails if: the home is inside the game install or the reverse; the
  NVForever executable is inside the game install; the home overlaps the original game's
  user folder.
- `guard_write` is the gatekeeper for every file NVForever writes; it refuses paths inside
  the game install or the original user folder. `fnvtool bsa extract` already uses it.
- `fnvtool setup --game <folder>` creates the home and records the game location in
  `NVForever.ini`, refusing overlapping layouts and installs that lack required files;
  `fnvtool paths` prints the resolved layout, the separation check, missing files and the
  add-ons present.

## Mods

Mods for NVForever go into `<home>\mods\` and are layered over the read-only game Data
by NVForever's virtual file system (planned with Layer 2/3). Nothing is ever installed
into the game's `Data` folder, so the original game keeps running exactly as before.

Paths come from the `directories` crate: the home is under the Documents known folder and
the cache under the Local AppData known folder, so a redirected Documents folder (e.g.
OneDrive) is followed. Off Windows the home is the platform data directory (no dialog
there yet: pass `--data` or write the INI).

## Not yet handled

- Symlinks/junctions are resolved by canonicalization only for paths that exist.
- No in-game way to change the game folder; edit or delete `NVForever.ini`.
