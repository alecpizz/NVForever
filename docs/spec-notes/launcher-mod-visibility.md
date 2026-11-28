# Spec note: how a launcher mod is seen

NVForever's own rules for the launcher's mods folder. The page that owns
those rules is [mod-manager.md](mod-manager.md). This is not a claim about
the original game, and it is not another program's deploy step. No code was
copied. The game `Data` folder is not written.

## What Apply does

(code: `App::apply_mods` and `App::save_mods` in
`crates/nvf-launcher/src/app.rs`) **Apply** writes three text files in the
launcher home: `plugins.txt` (which plugins are on), `mod-order.txt` (which
mod folders are on, and their order) and `mod-locks.txt` (the locked folders). It does not copy, hard-link, or symlink
mod files anywhere. A profile **Save** or **Load** writes or reads
`profiles/<name>/` only.

`mod-order.txt` is read by the launcher when it draws the page and when it
starts the game. The game process does not read it. The launcher passes each
enabled folder as `--mod-layer`, lowest priority first (the last one wins)
(code: `plugins::mod_content_layers`, `launch::spawn`).

## What the game process reads

The launcher starts NVForever. NVForever indexes files itself. It does not
ask the original executable to look in a second folder.

(code: `layered_plugin_files` and `plugin_path_layered` in
`crates/fnv-install/src/plugins.rs`, and `engine/nvforever/src/main.rs` when
`--mo2` is not set)

- A plugin is opened from the enabled mod layers, the **highest priority**
  layer that has it first, including a file nested in a subfolder of that mod.
  A turned-off folder is not a layer, so its plugins are not opened from it,
  and when two folders have the same plugin the list order picks, not the
  directory order. A plugin left loose at the top of the mods folder (editor
  mode saves one there) comes next, below every mod folder. The same name in
  `Data` is used when none of those has it. A mod installed in `Data` by hand
  is still read.
- Loose files and archives in `Data` are still read. Each enabled mod folder
  is a layer over that. The same relative path in a later layer wins
  (code: `Vfs::open_launcher_mods`). A mod's meshes and textures are
  read from `mods/<Name>/` (or from `mods/<Name>/Data/` when that folder exists),
  not as `<Name>/meshes/...`.
- An archive is mounted by the base game's rule wherever it sits (Data, a mod
  folder, or loose at the top of the mods folder, the layer just above Data):
  the base archive list, `Update.bsa`, and each active plugin's own archives,
  `<stem>.bsa`, `<stem> - *.bsa` and the numbered pieces, the longest plugin name
  winning, in load order [documented: docs/design/mod-folders.md;
  `Vfs::order_from_owned`, `plugin_archive_piece`, `archive_owner`]. So
  `A.esp` mounts `A.bsa` and `A - Textures.bsa`, not `Armor.bsa`, and an
  archive no active plugin owns is not mounted.
- Folder on/off and the launcher's order are applied only through `--mod-layer`.
  A start that was not given those arguments layers every mod folder in name
  order. On/off is not applied on that path, because the game does not read
  `mod-order.txt` (code: `plugins::mod_folder_layers`).
- (code: `Layout::mod_roots`, `Vfs::open_for_plugins_layered` and
  `plugins::plugin_path_in`) A start with `--mo2` is as on main: each enabled
  mod folder of that instance, then its overwrite folder. A plugin is taken from
  the top of those folders, highest first, then Data. Nothing is linked, copied
  or written into the instance.

## Links

Nothing in the launcher links or deletes files in the game `Data` folder.

(code: `link_into_mods` and `symlink_file` in
`crates/fnv-install/src/plugins.rs`) The only link still made is into the mods
folder, and only when `NVFOREVER_PLUGIN_SEARCH` finds a plugin that is in
neither the mods folder nor `Data`. The destination is refused if it would be
inside the game folder.

The order is the same on both platforms:

1. A hard link of that one file into the mods folder. This works when the
   source and the mods folder are on the same volume. It does not need the
   privilege Windows asks for a symbolic link.
2. If the hard link fails because the paths are on different disks, a symlink:
   `std::os::unix::fs::symlink` on Unix, `std::os::windows::fs::symlink_file`
   on Windows. A directory junction is not created. On Windows that file
   symlink fails unless the account is allowed to create symbolic links.
3. A copy, only when `NVFOREVER_PLUGIN_COPY` is set and the symlink failed.
   The copy is still into the mods folder, not into `Data`.

If every link fails, the plugin is read from the path where it was found.
