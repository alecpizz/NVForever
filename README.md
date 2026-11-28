# NVForever

An open-source, clean-room reimplementation of the **Fallout: New Vegas** engine, in the
spirit of OpenMW. It runs on the data files of *your own copy* of the game; it ships no
Bethesda content and contains no Bethesda or Gamebryo code.

NVForever is not affiliated with or endorsed by Bethesda Softworks, ZeniMax Media,
Microsoft or Obsidian Entertainment. See [LEGAL.md](LEGAL.md).

## Getting started

1. Own and install *Fallout: New Vegas* (base game; DLC optional).
2. Download a build from the [releases page](https://github.com/LootJunkieAzza/NVForever/releases)
   and unpack it **outside** the game folder.
3. Run `nvforever.exe` (or `./nvforever.sh` on Linux) and point it at your game folder
   when asked.

NVForever never writes into the game folder. Its settings, saves and logs live in
`Documents\My Games\NVForever`.

Optional: NVIDIA DLSS upscaling on an RTX graphics card (Windows), with files downloaded
from NVIDIA: see [docs/dlss.md](docs/dlss.md).

## Mods

- **Mod Organizer 2:** add `nvforever.exe` to MO2's executables (the drop-down beside
  **Run**, then **<Edit…>**) and start it from there. It loads the plugins ticked in MO2,
  in MO2's order, and the game folder MO2 manages. See
  [docs/mod-organizer-2.md](docs/mod-organizer-2.md). `--mo2` only reads that instance.
- **Without a mod manager:** put the mod in the game's `Data` folder, then turn its plugin
  on in `Documents\My Games\NVForever\plugins.txt` with a `*` in front of its line. The
  order of the lines is the load order. NVForever keeps its own list, so the original
  game's mod list is not changed. A plugin, archive, or loose file in `Data` is still read.

The launcher's Mods page is its own manager. Mod folders live in the launcher
home's `mods` folder, not in the game folder: `Documents\My Games\NVForever\mods`
on Windows, `~/.local/share/nvforever/mods` on Linux, or under `NVFOREVER_HOME`
when it is set.

- Drop a folder or an archive on the mod list. An installer that needs a choice
  opens its own window. New plugins from that folder appear in the list.
- Drag a folder to move it. The bottom checked folder wins when two share a file.
  **Conflicts** marks a shared file green when this folder's copy is used, and red
  when a lower folder's copy wins (**Hide conflicts** turns the marks off).
- Drag plugins the same way. **Apply** writes the plugin list and the folder
  order the launcher uses. It does not link or copy files into the game folder.
  The base game and a mod installed in `Data` are still read from there. A file
  of the same name in the mods folder is the one opened.

A profile is a name you type, plus the folder order and the plugin order on the
page. The name is one folder name, not a path.

- **Save** updates the open profile. **Add profile**, or Enter in the name, stores
  the same lists under that name in `profiles` next to the mods folder, and asks
  before it replaces one that already exists. There is no save-location picker.
  **Save backup** keeps a dated copy of the mod list and the load order.
  Lowering how many are kept asks before the oldest backups are deleted.
  The mods themselves stay. **Load backup** writes that copy into a profile.
  None of these copy mod files.
  A profile folder holds:
  - `modlist.txt` — one mod a line, in Mod Organizer 2's form, so MO2 can read it.
    `+` is enabled, `-` is disabled, and a separator is `-Name_separator`.
    Right-click a row for a separator above or below it. Double-click a separator to
    rename it. The winning mod is first in the file. The page shows that mod last.
  - `locks.txt` — the locked mods, one a line (MO2's list has no lock mark).
  - `plugins.txt` — plugins that are enabled, a `*` before the name.
  - `loadorder.txt` — every plugin on the page, first loaded first.
  - `note.txt` — the note on the profile.
  - `archives.txt` — archive names stored with the profile, one a line.
- **Load** puts that profile's mod list and load order on the page. It does not
  copy a mod. Profiles are saved only in the launcher's `profiles` folder. A
  profiles folder dropped on the path line (an older instance's) can be read and
  loaded, and nothing is written into it.

**Apply** and **Load** do not write the game `Data` folder. Loose files in the
launcher's Overwrite folder become a mod from the mod list's right-click menu,
**Make mod below**. The new mod sits under that row. The game folder is not
used. Reading another program's instance with `--mo2` stays read-only.

You can still drop an archive onto the Mods page. **Quick Install** opens.
OK uses the archive as it is. Manual lists the files, and `<data>` is the
folder that becomes the mod. Nothing is copied until OK, and it is not copied
into Data.

Script extenders (xNVSE, JIP LN, JohnnyGuitar and several smaller ones) are built into the
engine: their script functions run without the plugin `.dll` files, which NVForever never
loads. A mod that needs a plugin's engine patches, or a function not built yet, may not work.

## Building from source

Needs [Rust](https://rustup.rs) and Python 3.11+.

```bash
cd engine
cargo run --release --bin nvforever -- --data "<path to Fallout New Vegas>/Data"
```

The launcher is a separate program, from the repository root:

```bash
cargo build --release --locked -p nvf-launcher
```

The program is `target/release/nvforever-launcher` (`nvforever-launcher.exe` on Windows).

Experimental Android source builds: [docs/android.md](docs/android.md) (device qualification pending).

More: [docs/development.md](docs/development.md) (build, test, options, tools) ·
[docs/status.md](docs/status.md) (what's built and what's left) ·
[open issues](https://github.com/LootJunkieAzza/NVForever/issues) (the work to do).

## Community

Chat, questions and playtesting: [Discord](https://discord.gg/3HsVrsXSb).

## Bugs

Report bugs and differences from the original with the
[issue forms](https://github.com/LootJunkieAzza/NVForever/issues/new/choose). Crash
reports are written to `Documents\My Games\NVForever\logs`. Never attach game files.

## Contributing

Pull requests are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) first: the clean-room
rules there decide what can be merged.

## Support

NVForever is free. If you'd like to support it: [ko-fi.com/lootjunkieazza](https://ko-fi.com/lootjunkieazza).
Donations buy nothing extra; you still need your own copy of the game.

## Licence

GPL-3.0-or-later; see [LICENSE](LICENSE) and [LEGAL.md](LEGAL.md#licence).
