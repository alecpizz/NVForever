# Mods folders: an MO2 profile's merge without MO2 running

Mod Organizer 2 shows a program the game's `Data` folder with the profile's mods merged into
it, through its file system hook, usvfs [documented: MO2 source, `OrganizerCore::beforeRun`
and `fileMapping`]. On Windows NVForever only has to notice the hook and read the plugin list
MO2 maps in (`crates/fnv-install/src/mo2.rs`). On Linux there is no MO2 and no hook, so the
engine does the merge itself, inside its own virtual file system (#1526): point it at an MO2
instance and a profile, and the profile's enabled mods' loose files are read as if they sat in
`Data`, the overwrite folder last, and the profile's `plugins.txt` decides the load order.

The instance and the profile stay MO2's: installing mods, ordering them and ticking plugins is
still done in MO2 (on Windows, or in a Windows install shared with a Linux one). NVForever
only reads the profile. Nothing is written into the game folder or into MO2's folders.
Formats and their public sources: [spec note](../spec-notes/mod-organizer-profile.md).

## Using it

```
nvforever --mo2 "<instance folder>" [--profile <name>]
fnvtool vfs mods "<instance folder>" [--profile <name>] [--data <Data> --path <game path>]
```

The instance folder is the one holding `ModOrganizer.ini` (a portable instance, or a global
one under `%LOCALAPPDATA%\ModOrganizer\<name>` copied over). `--profile` defaults to the
instance's `selected_profile`. The game's `Data` folder still comes from `--data` /
`NVForever.ini`, so a profile copied from Windows may keep its Windows `gamePath`.

`fnvtool vfs mods` prints the profile as the merge it means (`fnv.vfs.mods/2`): the mods in
list order with their enabled state, the folders from the lowest priority up, the active
plugins, the profile's archive list and where it came from, and any mod archive the list
does not tick. With `--data` and `--path` it mounts the same archives the engine would,
resolves one game path through the same layers, and names the layer — or the archive and
its file — that wins.

## What is merged, and in which order

Precedence, lowest first:

| Layer | Source | Notes |
|---|---|---|
| 1 | the game's `Data` loose files | as before; archived copies under them |
| 2 | each enabled mod's folder | from the lowest priority up, so a higher priority mod replaces a lower one's file |
| 3 | the overwrite folder | always last [documented: MO2 `Profile::doWriteModlist`, `overwrite` priority] |

Folders come from `ModOrganizer.ini` [documented: MO2 `PathSettings`]: `mod_directory`,
`profiles_directory`, `overwrite_directory` under `[Settings]`, each defaulting to
`%BASE_DIR%/mods`, `%BASE_DIR%/profiles`, `%BASE_DIR%/overwrite`, and `%BASE_DIR%` defaulting
to `[Settings] base_directory`, itself defaulting to the instance's folder. The profile is
`[General] selected_profile`, or `--profile`.

`modlist.txt` is read as MO2 writes it [documented: `Profile::doWriteModlist`]: a header line,
then `+Name` (on), `-Name` (off) and `*Name` (the game's own files, on), **highest priority
first**; the file is the profile's list without the automatic rows (separators, backups). A
mod the list names whose folder is gone is skipped rather than becoming an empty layer.
Disabled mods are not read at all.

`plugins.txt` in the profile is read as MO2 writes it [documented:
`GamebryoGamePlugins::writeList`]: the header line, then the **active** plugins only, one per
line, in load order, in the system's ANSI code page. `loadorder.txt` is not needed: the same
order is in `plugins.txt`, and a profile MO2 has not written yet has neither. A visited plugin
is opened from the highest layer that has it (overwrite, then the mods from the highest
priority down), else from `Data`.

## Archives in mods

A `.bsa` at a mod's root is visible in the merged `Data` under usvfs, and the game loads the
archives its `SArchiveList` names. NVForever mounts them the same way (#1526 phase 2):

- The archive list is the profile's: `archives.txt` — the Archives tab's ticked entries, one
  name per line, lowest precedence first [documented: MO2 `MainWindow::saveArchiveList`] —
  or, when that file is absent or empty, `[Archive] SArchiveList` of the profile's own
  `fallout.ini` [documented: `FalloutNVDataArchives::archives`, which reads that key from
  the profile's INI with local settings on]. An empty list, or neither file, keeps the
  shipped default list. A list that leaves out a base archive gets a warning, since its
  files will be missing. Under `--mo2`, `NVForever.ini`'s `[Archive] SArchiveList` is not
  read at all: the VFS takes one list per run, and the profile's is the game's.
- A name is opened from the highest layer that has it: the overwrite folder, then the mods
  from the highest priority down, then `Data`. A mod's copy of `Fallout - Meshes.bsa` thus
  replaces the game's, as usvfs' merged folder shows one file. A mod archive the list does
  not tick is not mounted — the game would not load it either — and the start names it.
- `Update.bsa` and each active plugin's own archives (`<stem>.bsa`, `<stem> - *.bsa`, the
  numbered pieces) are looked for through the same layers, the plugin ones in load order.
- Loose files stay above every archive, as before. An archive's *entries* are indexed in
  the mounted order, so a later archive's copy of a path replaces an earlier one's; mods
  that ship overlapping archives are the case this does not model beyond that.

## The profile's INIs and its saves (phase 2b)

MO2's profile folder keeps more than the lists: a `settings.ini` with the profile's own
switches, copies of the game's INI files when the profile keeps them, MO2's merged INI tweaks,
and — with local saves — the saves. NVForever reads all of it read-only and writes nothing
into the profile; a save is only ever written to NVForever's own folder.

- **`settings.ini`** is a QSettings INI; a key outside any group is under `[General]`
  [documented: Qt QSettings]. Two keys matter:
  - `LocalSaves` — on: the profile's saves are `<profile>/saves`, which MO2 maps over the
    game's own save folder; off: the player's folder [documented: MO2
    `Profile::localSavesEnabled`, `GamebryoLocalSavegames`]. A profile without the key is read
    as MO2's `Profile::findProfileSettings` reads it: a `saves` folder means on.
  - `LocalSettings` — on: the profile keeps the game's INI files (below) [documented: MO2
    `Profile::localSettingsEnabled`]. A profile without the key is on when it has a
    `Fallout.ini`, since MO2's copies exist only with the switch on.
  MO2's global defaults for both (`profile_local_saves`, `profile_local_inis`, in the
  instance's own settings) are not read: a profile with neither the key nor its folder/copies
  is taken as off.
- **The INI text the game reads**, lowest precedence first:
  1. `<profile>/Fallout.ini`, then `<profile>/FalloutPrefs.ini`, then
     `<profile>/FalloutCustom.ini` — the profile's own copies, only with local settings on.
     A later file wins a key in several, as the game reads them in that order (#4491).
  2. `<profile>/initweaks.ini` — MO2 rewrites this file from the enabled mods' INI tweaks in
     priority order plus the profile's own, and adds `[Archive] bInvalidateOlderFiles=1`
     [documented: MO2 `Profile::createTweakedIniFile`]. Its keys sit above the INIs'.
  Files are matched without case and read as lossy UTF-8 (the game's INIs are in the system
  code page). No file, or a profile with neither, means no INI text.
- **What the text is used for.** The extender's INI functions read it: `GetNumericINISetting`,
  `GetStringIniSetting` and the setters (`fnv_game::nvse_settings`) see the profile's values
  under `--mo2`, where before they had none. The engine's own settings are *not* taken from
  it: its controls, LOD and graphics defaults stay its own (`bindings.rs`, `lod.rs`), and
  `NVForever.ini` keeps its place — merging MO2's INIs into NVForever's own settings is a
  separate step, and the profile's `[Archive] SArchiveList` is already read as the archive
  list above.
- **Saves.** With `LocalSaves` on, a bare `--load <name>` reads `<profile>/saves`, the place
  MO2's mapping shows the original game; with it off, `--load` reads NVForever's home. Every
  save and autosave NVForever makes goes to its own home folder, never into the profile
  (maintainer decision 2026-11-18): the profile's saves are read-only. Nothing else in the
  profile is written either.

## What this does not do (yet)

- **The engine's own settings from the profile's INIs.** Controls, LOD and the graphics
  values stay coded in NVForever (with `NVForever.ini` over them); only the extender's INI
  functions read the profile's text. MO2's global profile switches are not read either (above).
- **Files read outside `Vfs`.** Anything that opens `Data` with plain file calls instead of
  through the virtual file system will not see the mods. The engine's assets go through
  `GameData`/`Vfs`; this is the honest limit of an in-process merge, and worth naming in a
  bug report that looks like "the mod file is there but the game does not see it".
- **usvfs itself.** No file-system hook, no copy-on-write into overwrite, no per-file
  shadowing rules beyond path precedence. Only the paths above are merged.

## Code

- `crates/fnv-install/src/mo2.rs`: `Layout::open` (instance + profile), `mod_roots`
  (lowest first, overwrite last), `profile_plugins`, `profile_archives` (the archive list
  and the file it came from), `mod_archives` (the warning's list), `profile_setting`,
  `local_settings`/`profile_inis`/`ini_texts`/`ini_setting` (the profile's INI text) and
  `local_saves`/`saves_dir`.
- `crates/fnv-vfs/src/lib.rs`: `Vfs::open_for_plugins_layered` indexes each root over `Data`
  in order, later roots replacing earlier files by normalised path (case- and
  slash-insensitive); `archive_files_layered` resolves the base list, `Update.bsa` and the
  plugins' archives through the layers, and `open_files` mounts them; `loose_roots` reports
  the layers; `archive_path` names an archive's file.
- `crates/fnv-install/src/plugins.rs`: `plugin_path_in` looks a plugin up through the roots,
  highest first.
- `engine/nvforever/src/main.rs`: `--mo2`, `--profile`; the profile's plugins take the place
  of the home `plugins.txt` when `--plugins` is not given, and its archive list the place of
  `NVForever.ini`'s; with local saves, a bare `--load` reads the profile's `saves` folder
  (read-only: saves are never written there); the INI text goes to
  `nvf_play::script::set_ini_texts`.
- `engine/nvf-play/src/script.rs`: `set_ini_texts`, put into the `GameData` the script VM is
  built on.
- `crates/fnvtool/src/bsa_cmd.rs`: `vfs mods`, the verification command (`fnv.vfs.mods/3`
  adds `local_settings`, `local_saves`, `saves_dir`, `inis` and two sample settings).

## Tests

- `crates/fnv-install/src/mo2.rs` tests: the roots' order (lowest priority, overwrite last),
  a gone mod folder skipped, `ModOrganizer.ini` folders and `--profile`, archives named once,
  the archive list from `archives.txt` then the profile INI.
- `crates/fnv-vfs/tests/layered.rs`: a higher layer replaces a lower one's file regardless of
  case or slash; a lower layer's untouched files stay readable; absent layers are skipped;
  a mod's archive is mounted from its folder, a mod's replacement of a base archive wins
  whole, and a plugin's archive in a mod folder is mounted by load order.
- Verified on this host against the GOG install (2026-11-17): a synthetic instance with
  `Base`, `Late` and `overwrite` all holding `textures\test\shared.dds` resolved to
  `overwrite`'s copy through `fnvtool vfs mods --path`, and an engine run with the same
  instance read a corrupt `FalloutNV.esm` placed in `overwrite` ("plugin FalloutNV.esm left
  out: cannot be read"), proving the plugin lookup goes through the layers. For phase 2 the
  same instance's `archives.txt` listed the six base archives and `SomeMod - Meshes.bsa`, a
  copy of the game's huge mesh archive in an enabled mod folder: `fnvtool vfs mods --path`
  resolved `meshes\clutter\deathclaw_sign\nv_deathclawsign.nif` to that archive's file with
  the mod folder as its layer, and the mounted list showed the mod's copy in place of the
  base archive (2026-11-17).
