# NVForever release build

NVForever is a clean-room reimplementation of the Fallout: New Vegas engine. **It ships no
game content**: you need your own copy of Fallout: New Vegas (1.4.0.525, base game and DLC).
NVForever only reads the game's `Data` folder and never writes into the game folder or
`Documents\My Games\FalloutNV`.

## What is in this archive

| File | What it is |
|---|---|
| `nvforever(.exe)` | the game: opens on the main menu; `--explore` for the free-camera explorer |
| `nvforever-launcher(.exe)` | the launcher: news, Continue / Main menu, the settings, the load order and the game folder without starting the game |
| `nvf-viewer(.exe)` | the asset viewer: meshes, textures and animations from your data |
| `fnvtool(.exe)` | the command-line tool: setup, paths, archive and plugin inspection as JSON |
| `nvforever.cmd` / `.sh`, `nvf-viewer.cmd` / `.sh` | launchers that point the programs at `schemas/` beside them |
| `schemas/` | record, NIF and script-function schemas the programs read at start-up |
| `dxcompiler.dll`, `DXC-LICENSE-*.txt` (Windows) | Microsoft's DirectX shader compiler (DXC) for the DirectX 12 renderer (`WGPU_BACKEND=dx12`), with its licences; the default Vulkan renderer does not use it |

## Running

Unpack anywhere outside the game folder (e.g. `C:\Games\NVForever`) and run `nvforever.exe`
(double-click it) or `nvforever.cmd`. At the first start NVForever asks for your Fallout:
New Vegas folder (the one holding `Data`, e.g.
`F:\SteamLibrary\steamapps\common\Fallout New Vegas`), checks that the game files it needs
are there and remembers the folder in `Documents\My Games\NVForever\NVForever.ini`
(edit or delete that file to change it; `--data` overrides it for one run). Then you are on
the main menu: Continue, New Game, Load Game, Settings, Quit. Other starts:

```
nvforever.cmd                            # the main menu (the same as no launcher)
nvforever.cmd --play --new-game          # straight into the opening
nvforever.cmd --play --cell Primm        # playing, starting in Primm
nvforever.cmd --explore                  # the free-camera explorer (a developer's view)
nvforever.cmd --help                     # every option
nvf-viewer.cmd --mesh meshes\clutter\...
```

The explorer used to be what `nvforever` opened without `--play`; it now needs `--explore`
(or `--cell` / `--pos` without `--play`).

On Linux use the `.sh` launchers and pass `--data` (or set `Path=` under `[Game]` in the
home's `NVForever.ini`). Settings, saves, logs and crash reports go to
`Documents\My Games\NVForever` (or `NVFOREVER_HOME`); caches to `%LOCALAPPDATA%\NVForever\cache`.

`nvforever` finds `schemas/` beside itself, so it runs with or without its launcher.
Run `nvf-viewer` through its launcher (or pass `--esm-schema` and `--nif-schema`): its
built-in schema defaults point at the build machine's checkout.

## Reports

The first start asks once whether NVForever may send a report after each session (the
build, your PC's hardware and settings, your mod list, how it ran, the game's log and any
crash). Nothing is sent unless you choose **Send reports**. Each report is written first to
`Documents\My Games\NVForever\reports` and sent at the next start; no personal details and
no saves are sent. Settings → Gameplay → Reports turns it off or deletes what was sent;
`NVF_REPORTS=0` turns it off for one run. Details: `docs/design/player-reports.md` in the
repository.

## Mods

- **Mod Organizer 2:** add `nvforever.exe` to MO2's executables (the drop-down beside
  **Run**, then **<Edit…>**, **+**, **Add from file…**) and start it from there. It loads the
  plugins ticked in MO2's profile, in MO2's order, and runs on the game folder MO2 manages.
  Details: `docs/mod-organizer-2.md` in the repository.
- **Without a mod manager:** put the mod in the game's `Data` folder and turn its plugin on
  in `Documents\My Games\NVForever\plugins.txt` with a `*` in front of its line; the order
  of the lines is the load order.
- Script extenders (xNVSE, JIP LN, JohnnyGuitar and several smaller ones) are built into
  NVForever: their script functions run without the plugin `.dll` files, which are never
  loaded. A mod that needs a plugin's engine patches, or a function not built yet, may not
  work.

What works and what is not started yet: `docs/status.md` in the repository. Licence:
GPL-3.0-or-later (`LICENSE`); legal notes in `LEGAL.md`; the third-party packages the
programs are built from and their licences in `THIRD-PARTY-LICENSES.txt`.
