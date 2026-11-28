# Spec note: FOMOD installers and mod archives, as the launcher reads them

What the launcher's Mods page does when a mod archive or a FOMOD installer is dropped on it
(`crates/nvf-launcher/src/fomod.rs`). FOMOD is the mod community's packaging format, not a
format of the original game, and nothing here comes from Bethesda's code. Where the public
sources leave a rule open, the launcher's choice is tagged `[design]`.

## Sources

- **[S1] "The fomod Documentation"** (fomod-docs), "written for the `5.x` major releases":
  https://fomod-docs.readthedocs.io/en/latest/ , with its pages "Specification"
  (`specs.html`), "A fomod Tutorial" (`tutorial.html`) and the schema reference "XML Schema
  Documentation" for `ModuleConfig.xml` (`_static/ModuleConfig.html`), which gives the schema
  "Version 5.1". The schema file is `ModuleConfig.xsd` in dh-nunes/fomod-schema (formerly
  GandaG/fomod-schema), as of commit `dd86bbe2ada6` (2017-12-03); that repository has no tag
  `5.1` (its tags run to `5.0`). Read 2026-11-23, checked again 2026-11-24.
- **[S2] The Nexus Mods App's developer note "About FOMOD"**: `docs/developers/misc/AboutFomod.md`
  in Nexus-Mods/NexusMods.App, as of commit `60cb48066236` (2024-06-18). Its old web address,
  `https://nexus-mods.github.io/NexusMods.App/misc/AboutFomod/`, answered 404 on 2026-11-23.
  The comment at the top of `fomod.rs` names it in the same words: the Nexus Mods App's
  developer note "About FOMOD".

## Packaging

- FOMOD mods are packed as `.zip`, `.rar` or `.7z`, and "Sometimes a file with the `.fomod`
  extension is actually just a `.zip` file" [documented: S2, "Intro"].
- An archive can hold another: "the outer archive could be a .rar file containing a single
  `.zip` file with `.fomod` extension which then contains the actual mod content"
  [documented: S2, "Packaging"]. The launcher opens one such inner archive, the only file of the
  outer one [design; `unwrap_nested`].
- The launcher also reads `.tar` and gzip (`.tar.gz`, `.tgz`) [design]. Its own zip and tar
  readers come first. An outside program (7z, bsdtar, unar, unrar) is used only for a format
  they do not read (`.7z`, `.rar`) or a file they cannot read as that format at all. An entry
  they refuse (one that would land outside the mod's folder, a link, a broken entry) ends the
  install, and no outside program gets the archive after it [design; `extract_archive`]. A tar's
  `./` entry, as `tar -C folder -cf mod.tar .` writes it, is the mod's folder itself and is
  skipped [design; `unpack_tar`].
- An outside program first lists the archive, and every name it lists must pass the
  launcher's path rule (`rel_path`) before it unpacks anything. Every listed entry must also be
  a file or a folder: a link, hard link, junction or device entry is refused before anything is
  unpacked, as the built-in readers refuse it, since a link unpacked first could carry a later
  entry out of the mod's folder before the check for links after unpacking. The type comes from
  each program's own listing: 7z's `-slt` `Attributes`, `Mode`, `Symbolic Link` and
  `Hard Link`; the first letter of each `bsdtar -tvf` line; `unrar lt`'s `Type`; `lsar -j`'s
  link keys [design; `extract_external`; observed: 7-Zip 26.06, bsdtar 3.8 and UNRAR 7.23 listings
  of archives holding links; hypothesis: lsar's keys, not run]. A refused entry, or a listing
  with no names, ends the install [design; `extract_external`].
- The launcher's own readers refuse an archive whose entries add up to more than 100 times the
  archive's size; an archive inside it counts against the same limit, and each entry counts as at
  least 4 KiB (a disk block), so many empty entries are not free. The outside programs are not
  limited this way [design; `unpack_limit`, `entry_cost`].
- A tar's long name or extended header is held whole in memory while it is read, so a tar is
  read no further than twice that limit plus 1 MiB; a longer one is refused [design;
  `unpack_tar`].
- An archive, or a file dropped without a path, is unpacked or written into a new folder of the
  system's temporary folder, made for that one install. On Linux and macOS only the player's
  account can open it. The modes stored in a tar are not used, so nothing there is writable by
  other users or read-only to its removal; if an outside program kept read-only modes, the
  folder is made writable and removed, never following a link [design; `make_temp`,
  `unpack_tar`, `remove_temp`].
- The installer sits in a `fomod` folder holding `ModuleConfig.xml` (the installer) and
  `info.xml`, which has "no defined schema" [documented: S1, "Specification"].
- The launcher reads a `ModuleConfig.xml` of at most 16 MiB, and refuses one whose elements
  nest more than 64 deep [design; `read_xml`, `parse_xml`].
- An installer may copy at most four times the size of its unpacked files in all (never less
  than 16 MiB), each file and folder counting as at least 4 KiB, the folders a destination makes
  too, so a file listed for many places, many empty ones or many deep destinations cannot fill
  the disk; past that the install stops. Only the first 100 000
  paths an installer would write are checked for clashes with other mods, and an installer naming
  a `source` or `destination` longer than 1024 bytes is refused when it opens. Folders are copied from
  a list, not by a call per level, so a deep one cannot use up the stack [design; `copy_items`,
  `copy_children`, `charge_new_folders`, `plan_loose_files`, `paths_fit`].
- An installer's pictures are shown only up to 8 MiB a file and 2048 pixels each way (by the
  picture's header), and up to 256 MiB for all of them once decoded; a picture past that is not
  shown. They are let go when the installer closes [design; `image_fits`, `forget_images`]. A
  picture is loaded from exactly the path that was checked; one whose path holds `%`, `#` or `?`,
  or is not UTF-8, is not shown, since a loader could read another file for it [design;
  `file_uri`].
- The mod's folder is named from `moduleName` (else the archive's name): its last path part,
  without control characters (a line break would split a line of the mod list) or the
  characters Windows does not allow in names [design; `clean_name`].
- A C# script installer (`fomod/script.cs`) is code "that gets compiled on the user's system and
  run in the context of the mod manager" [documented: S2, "FOMOD C#"]. The launcher never
  compiles or runs one. An archive with only a script is refused; with a `ModuleConfig.xml` too,
  the XML is used and the page says the script was not run [design; `stage_tree`].

## Groups

- A group's `type` is one of `SelectAtLeastOne`, `SelectAtMostOne`, `SelectExactlyOne`,
  `SelectAll`, `SelectAny`: "The group type - affects which options can be chosen"
  [documented: S1, schema reference, `group`]. The tutorial calls them "self-explanatory" and
  defines none [documented: S1, "A fomod Tutorial"].
- The launcher reads them by their names [design]: at least one, at most one or exactly one
  usable option chosen; every option chosen; any number. An exactly-one group with more than one
  pick keeps the first. An exactly-one or at-least-one group with none gets one: the first
  `Recommended` option, else the first `Required`, else the first usable one
  [`Wizard::normalize`, `default_pick`].

## Option types

- An option's type is one of `Required`, `Optional`, `Recommended`, `NotUsable`,
  `CouldBeUsable` [documented: S1, schema reference, `pluginTypeEnum`]. The tutorial describes
  three of them, Optional ("where the option is... optional"), Required ("where the user doesn't
  really have a choice") and Recommended ("where the option is usually pre-selected. Be careful
  as implementation of this varies"), and calls the other two "useless" [documented: S1, "A fomod
  Tutorial"].
- The launcher [design]: a `Required` option is always chosen; a `NotUsable` one is never chosen
  and its files are not copied; `Recommended` is the default pick (above); `CouldBeUsable` is
  chosen freely, as `Optional` is.

## Flags and conditions

- A chosen option sets flags: "A flag to set if the plugin is selected" [documented: S1, schema
  reference, `flag`]. A step is shown when its `visible` conditions match: "Otherwise, the
  install step is skipped" [documented: S1, schema reference, `visible`]. For conditional
  installs, "All matching patterns will have their files installed" [documented: S1, schema
  reference, `conditionalFileInstalls`]. A `flagDependency` "Specifies that a condition flag
  must have a specific value" [documented: S1, schema reference]. A `fommDependency`
  "Specifies a minimum required version of FOMM" [documented: S1, schema reference].
- The launcher [design]: flag names and values match without regard to case. A condition list
  with `operator="Or"` holds when any part holds, otherwise when all do. A `fileDependency` is
  judged from the load order and the files found: a plugin is `Active` when it is on,
  `Inactive` when it is present but off, `Missing` when it is not there; another file is
  present when its path is found [`file_state`]. A `gameDependency` is not checked and always
  holds. A `fommDependency` is left out of its condition list, as if it were not there
  [`is_dep_name`].

## Priority

- A file or folder's `priority` defaults to 0, and "A higher number indicates the file or
  folder should be installed after the items with lower numbers. This value does not have to be
  unique" [documented: S1, schema reference, `priority`]. Which copy is kept when two land on the
  same path, and what equal numbers do, is not said.
- "this format is not a fully specified standard, especially on the topic of priorities
  between multiple options containing the same files, the existing spec is fuzzy such that
  different managers, all following the spec correctly, can produce different results"
  [documented: S2, "FOMOD XML"].
- The launcher [design]: files are copied lowest priority first, so where two land on the same
  path the higher priority is copied last and kept. With equal priority the later one in this
  order is kept: the required files (`requiredInstallFiles`), then the chosen options' files in
  step, group and option order, then the conditional installs in pattern order; within one list,
  the order of the file [`Wizard::plan`, `copy_items`].
