# Spec note: a launcher profile

The launcher's own profile, written from the Mods page. This is not a description
of another program's files. Those, and the public sources they were read from,
are already in
[mod-organizer-profile.md](https://github.com/LootJunkieAzza/NVForever/blob/main/docs/spec-notes/mod-organizer-profile.md).
No code was copied. Nothing here was taken from the original game.

## Where it is written

These are the launcher's own choices, written up before the code in
[launcher-mods.md](../design/launcher-mods.md).

- The folder is `<home>/profiles/<name>/`. `<home>` is the launcher home. `<name>`
  is the name the player typed: one folder name, not a path.
- A dropped folder is read only. The save does not write there.
- The game folder and `Documents\My Games\FalloutNV` are not written
  [documented: docs/design/install-layout.md on NVForever main].

## The profile's files

The launcher's own text files, not a format read out of the original game:

- `modlist.txt`: one mod folder per line, in Mod Organizer 2's form so MO2 can
  read it [design]. `+` is on, `-` is off, and a separator is
  `-<name>_separator`. Lines that are empty or start with `#` are skipped, and so
  are MO2's `*` lines (its foreign mods) when an MO2 profile is read. The winning
  folder is the first line. The page stores that folder last, and reverses the
  file when it reads it [documented: docs/spec-notes/mod-organizer-profile.md,
  `modlist.txt`; `fnv_install::mod_order`]. A list written in the earlier form,
  with `=` before a locked row and `% name` for a separator, is still read.
- `locks.txt`: the locked mods, one name a line. MO2's list has no lock mark
  [design]. It is written with the three lists, copied by Duplicate, and kept
  by Save backup and Load backup.
- `plugins.txt`: plugins that are on, one per line. A leading `*` marks one on
  and is not part of the file name.
- `loadorder.txt`: every plugin on the page, one per line, the first loaded
  first. A leading `*` is ignored if it is present.
- `note.txt`: the note on the profile [design].
- `archives.txt`: archive names stored with the profile, one per line, sorted.
  The mod list does not show them [design].

A save writes all six (code: `App::write_profile_lists`,
`App::write_profile_side`). Saving the same name again replaces these six
files. Any other file in the folder is left in place. **Save backup** writes
only the four lists, into `plugin-backups/lists-<stamp>/`, with
`plugins-<stamp>.txt` beside it (code: `App::save_dated_backup`).
