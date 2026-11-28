# Launcher mods: a named profile, and where new files go

The launcher's own manager keeps each mod as one folder under the home's `mods`
directory. This note is the design for saving that page as a named profile, and
for a later output folder. It is not an import of another program's code. Reading
an existing instance with `--mo2` stays a separate, read-only path
([mod-folders.md](https://github.com/LootJunkieAzza/NVForever/blob/main/docs/design/mod-folders.md),
[#1526](https://github.com/LootJunkieAzza/NVForever/issues/1526)).
The file layout that reader already understands is written up in
[mod-organizer-profile.md](https://github.com/LootJunkieAzza/NVForever/blob/main/docs/spec-notes/mod-organizer-profile.md).
The files this page writes are in
[launcher-profile.md](../spec-notes/launcher-profile.md).
The rules for the Mods page and for a start without `--mo2` are in
[mod-manager.md](../spec-notes/mod-manager.md).
This note does not add facts about the original game.

No settings row is added. If one is added later it follows
[settings-page.md](https://github.com/LootJunkieAzza/NVForever/blob/main/docs/design/settings-page.md) §0.

## A profile is a name and two lists

The player types a name. **Add profile**, and **Save** while a profile is open,
write the profile: the folder order on the left and the plugin order on the right. The name is one
plain folder name, not a path. An empty name is refused.

The profile is a folder under the launcher home, `profiles/<name>/`. Saving
writes these six text files (`write_profile_lists` and `write_profile_side`):

| File | What it holds |
|---|---|
| `modlist.txt` | One mod folder per line. `+` means on, `-` means off. The winning folder is first in the file. The page shows that folder last, which is the order the page already uses when it reads a list. |
| `plugins.txt` | The plugins that are on, one per line, a `*` before the name. |
| `loadorder.txt` | Every plugin on the page, first loaded first. |
| `locks.txt` | The locked mod folders, one name per line. Mod Organizer 2's list has no lock mark, so locks are kept beside it. |
| `note.txt` | The note on the profile. |
| `archives.txt` | Archive names stored with the profile, one per line. The mod list does not show them. |

Saving the same name again replaces those six files and leaves any other file
in the folder. Nothing is copied. The mods folder is not the place a profile is
written.

A dropped folder is only for reading an older list. The drop fills the path
line. The path cannot be typed. **Read** lists profiles in that folder, and
**Load** puts the chosen lists on the page. A save does not write into the
dropped folder, so an older instance is left as it was.

**Load** of a profile this page saved puts both lists back. The plugin backup
list (the dated copies made when the live list is saved) is a different store.
**Save backup** used to write `plugins.txt.bak.1` beside the live list, which
that dated list never shows. It now writes a dated copy into `plugin-backups/`:
a `lists-<stamp>/` folder with the four list files (`modlist.txt`,
`plugins.txt`, `loadorder.txt`, `locks.txt`) and `plugins-<stamp>.txt`
(`save_dated_backup`). It does not write the named profile.

## What is not written

The profile save does not write the game folder, `Documents\My Games\FalloutNV`,
or the original game's plugin list. That is the install rule
([install-layout.md](https://github.com/LootJunkieAzza/NVForever/blob/main/docs/design/install-layout.md)).

**Apply** writes the launcher home's `plugins.txt` and `mod-order.txt`. The same two files are written when the mod list or the load order changes, with no profile open and without pressing **Save** [design]. That write does not make a backup. A dated copy is made only when **Save backup** is pressed. Those home files are what the next launch of the launcher reads. The game process does not read `mod-order.txt`. When the launcher starts the game it passes each enabled folder as `--mod-layer`, lowest priority first. An open profile is updated as well and still overrides the home lists when that profile is opened. **Save**
writes the profile's six text files and **Load** reads them. None of them link or copy
files into the game `Data` folder. The game still reads a plugin, an archive, or a loose file that was installed in `Data`. The facts of what the game process then reads are in
[launcher-mod-visibility.md](../spec-notes/launcher-mod-visibility.md).

## Output folder, not built yet

Files the game or a tool creates while running, other than the saves the
launcher already keeps, should land in one folder under the home, `overwrite`.
It is the last layer: the same relative path in `overwrite` wins over a mod
folder. It is not a profile, and it is not a copy of `Data`. It is not created
by the named-profile save.

## What stays read-only

`--mo2` still only reads another program's instance and profile. The launcher
does not write that instance's profile folders, its output folder, or its INI
files.
