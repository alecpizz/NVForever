# Launcher (`nvforever-launcher`, #2708)

A small window beside `nvforever.exe` (`crates/nvf-launcher`, egui/eframe on OpenGL) that
shows the project's news, starts the game and changes the settings in `NVForever.ini` without
starting the game first (owner request, 2026-11-15). The game does not need it: a start
without the launcher is the same as before.

## Pages

- **Home**: news posts. First the site's `https://nv-forever.com/news.json`
  (`{"posts":[{"title","date","author","image","body" (Markdown),"url"}]}`, see
  `src/news.rs`), and while the site has none, this repository's GitHub releases (title, date,
  notes). The last list is kept in the home's cache folder (`launcher-news.json`) for offline
  starts. "Fetch news" (`[Launcher] News=0/1`) turns every request off.
- **Settings**: the game's own menu, never a copy that can drift [design: owner 2026-11-16]:
  four of the game's tabs, **Gameplay, Display, Graphics and Tweaks** (`src/catalog.rs`
  `LAUNCHER_TABS`; Audio, Controls, NVF Features and Debug are set in the game only, owner
  2026-11-16), with the same categories and rows in the same order, the Tweaks tab's
  groups too (as "Group › Category" headings). The game writes its page beside the INI at every start
  (`settings-menu.json`, `nvf_settings_layout::menu`, `engine/nvf-play/src/settings_menu.rs`):
  each row's label and note as the page shows them, its kind (switch, slider and its
  steps' words, list and its choices) and its default (the value the game takes while the INI
  has none). Only a player's start writes it: a run given a switch a row names (`--no-voice`,
  `--graphics-only`, …) leaves the file there as it is, and so does a run with fewer rows than
  the same build's file there (a module missing that run); the log says
  `settings: menu for the launcher not written: …` with the reason. The launcher shows that
  file while its version and its table stamp (`nvf_settings_layout::layout_stamp`, a hash of
  the tabs, categories, rows and rules) are the launcher's (`src/catalog.rs`): dev builds
  between releases share a version, the stamp keeps one from showing another build's rows.
  Another build's file still lends its words to the built-in copy (below): a row's label,
  note, kind and default, and a tab's label, where the key is the same and the label is a
  real name, not the key; the built-in rows, order and rules stay (`Catalog::load`,
  `with_game_words`).
  Rows are hidden and greyed as the game's page does it, by
  the same tables (`nvf_settings_layout::SHOWN_WITH`, `NEEDS`, `NEEDS_CHOICE`, the
  controller's rows by `[Controls] Input Device`), live as rows are changed on the page; a
  greyed row keeps its value and says why ("Greyed out: Needs Sun Shadows."). A rule's row
  the INI does not hold yet counts as the game's default for it (the menu file's); with no
  default known (the built-in copy) the rule holds, so the launcher never greys or hides a row
  the game may not. No tab the launcher shows can have every row hidden (only the game's
  Controls tab can). Greying the game decides from the machine (the
  GPU, DLSS's files, a run's command line) is the game's alone; a choice that is an action, not a value (Download DLSS… on
  the Upscaler row), is not in it. A row whose INI value the game's row takes is drawn from the
  game's row before the launcher's own lists (`src/display.rs`), which stay for the rows the INI
  keeps by another word than the choice's name (Display sync `variable`, Language `en`, …). Before the game has written one (a first start, a game
  older than the file, an update not started yet) it shows its built-in copy: the tabs,
  categories and order from the game's own table (`crates/nvf-settings-layout`, `SECTIONS`,
  which the game's pause menu draws from), the labels and notes from
  `docs/settings-reference.md` (a test in the game holds the reference to the table; a
  Tweaks row the reference does not name is shown only while the INI holds it, labelled in
  sentence case from its key, `tweak-aiming-draws-weapon` as "Aiming draws weapon",
  `readable_tweak`). A row with no kind
  from the game goes by the value in the INI: `1`/`0` a switch, `0.50` a
  slider (as a percentage, or the step's words for Difficulty; the words can be typed into it too), anything else text; Kill camera, V.A.T.S. style and the Display rows (Resolution as a Window and a Full screen
  list of the chosen display's sizes, Monitor from the displays Windows lists, Display sync,
  Frame rate limit; `src/display.rs`, words and values the game's)
  get their choices as a list (a test checks the engine still has them). A
  row the game has not written yet shows "game default". There is no tab of the launcher's own: a
  `[Settings]` key the game's menu does not place (an older build's) is not shown. A slider the player
  moves is fitted onto its steps as the game fits it (from 0, to the thousandth); one only
  drawn keeps the INI's value, so opening a tab changes nothing. Save writes only the changed keys over the file as it is on disk then.
- **Mods**: NVForever's own `plugins.txt` (turn on/off with the box or a click on the name; drag a row anywhere but its box to move it, with a gold line where it lands; FalloutNV.esm stays first). The new game's start is on the Game folder page (#4313). Opening the page runs
  `fnv_install::plugins::sync`, as the game's start does, so new plugins get their lines.
- **Game folder**: `[Game] Path` with the required-files check, the program to start
  (`[Launcher] Exe`, else `nvforever(.exe)` beside the launcher, else this checkout's
  `engine/target/release`), and buttons that open the saves, logs, screenshots and settings
  folders. With no usable game folder (none in the INI that holds the game, no Mod
  Organizer 2 start, no `NVFOREVER_DATA`) the launcher opens on this page and says so
  (#2938). The box then starts on the game's usual place when the game is there (Steam's
  default library: `~/.local/share/Steam` or `~/.steam/steam` on Linux, `C:\Program Files
  (x86)\Steam` on Windows, and GOG's `C:\GOG Games\Fallout New Vegas`); it is stored only
  by Save.
  The same page holds the launcher's other NVForever.ini keys, which are not rows of the
  game's menu (#3483; the Settings page stays the game's own menu, maintainer decision
  2026-11-24): an **NVForever.ini** card with the new game's start first (below),
  `[Launcher] News`, `CloseOnPlay` and `Exe`
  (empty: the program beside the launcher), the game-written `[Controls] Input Device` shown but not changed, and the file's place. The
  card's Save writes the keys that changed over the file as it is on disk and leaves every
  other line; only a changed program is checked, so one that went missing does not stop the
  other keys being saved. Each Save on the page (the folder's, the card's) keeps the other's
  unsaved change. The file is in `Documents\My Games\NVForever` on Windows,
  `~/Library/Application Support/NVForever` on macOS and `~/.local/share/nvforever` (or
  `$XDG_DATA_HOME/nvforever`) on Linux; `NVFOREVER_HOME` overrides all three
  (`fnv_install::EngineHome::default_root`).
  The start's row (`src/start.rs`, #2945; on the Mods page until #4313) is `[General]
  sCharGenQuest` and `sIntroMovie`, written by the card's Save only when it changed (an empty
  field takes its line out, the game's default; "No intro film" writes `sIntroMovie=`). A
  **TTW on/off** switch fills in TTW's values (`001FFFF8`, no film) or the base game's
  (`00102037` (VCG00), `FNVIntro.bik`; docs/spec-notes/new-game-start.md); a gold "custom" tag
  shows while the start is neither (`start::Fields::is_custom`); **Custom…** opens a window
  with the start quest, the intro film (a `.bik` file in `Data\Video`) and "No intro film",
  whose OK puts them on the card (Cancel, Escape or a click outside changes nothing; #4288).
  The values are not shown on the card. Turning the switch back, or Cancel, undoes before
  Save. The row warns while the start quest is TTW's and `TaleOfTwoWastelands.esm` is off or
  not in the load order (`start::ttw_missing`, #3229); for it the page reads `plugins.txt`
  as it is on disk, without the Mods page's sync, so opening the page writes nothing.
- **Co-op**: what co-op is and how the game turns it on (the Co-op row of Settings, under Gameplay, off by default;
  `nvf_play::lobby`), and **Play Co-Op**, also on the bottom bar (every page). Play Co-Op starts the
  same program as Play, at its main menu (`--play --menu`), in the game's own home with its
  saves, settings and mods: co-op is part of the game, so there is no co-op program and
  nothing co-op on the command line [design; review of #3723].
- **Website / GitHub**: open nv-forever.com and the repository in the browser.

## Top and bottom bars

- The site's next-release countdown (the `nextRelease` block of the site's
  `assets/js/config.js`, the same that drives the site's own bar), in the middle of the top
  bar; nothing when the site announces none or the moment has passed.
- "Update to vX": when GitHub's newest release (the highest version of the release list,
  drafts left out; `releases/latest` skips pre-releases) is later than the launcher's version,
  a button that opens the update dialog (see Updater).
- Continue (the newest save, with its picture, name, level and place: `--play --load`),
  Main menu (`--play --menu`), Play Co-Op (`--play --menu`, shown whenever the game exe is found), and More: New game (`--play --new-game`), the explorer
  (`--explore`) and "Close the launcher when the game starts" (`[Launcher] CloseOnPlay`).
  No `--data` is passed: the game reads its folder from the INI, so a launcher start is a
  player's start in every way (reports included, by the player's own choice).
- With no usable game folder every start goes to the Game folder page and says the folder
  must be set first, instead of starting a game that would only ask for it (Windows) or
  quit (elsewhere) (#2938).
- A game that ends with an error is said in the message line, with its exit code and the
  logs folder; a clean exit clears "The game is starting…" (#2938). Not when the launcher
  closed at the start (`CloseOnPlay`).

## Fonts

The system's own interface font, read from its font folder at start and never shipped (Windows:
Segoe UI, Semibold for titles; Linux: Noto Sans or DejaVu Sans when installed), else egui's
built-in fonts. Their taller lines, a 16-point news body and wider paragraph spacing answer the
contributors' note that the news read "very squished together" (2026-11-15). egui has no
line-height setting for Markdown text, so the room comes from the font and its size.

## Rules it keeps

- Writes only NVForever's own files (the INI, `plugins.txt`), through
  `fnv_install::guard_write`; never the game folder or `Documents\My Games\FalloutNV`.
- Sends nothing but GET requests, and only while "Fetch news" is on (it is on at first, so the
  first start asks before the player has seen the switch): the site's `news.json` and
  `assets/js/config.js`, GitHub's release list (at most once a minute, shared by the news and
  the update check; GitHub allows 60 an hour without a sign-in), and each `https://` image a post names, on
  whatever host it names, side by side, at most 8 MiB and 15 s each (20 posts at most). Nothing fetched is
  saved but the news cache, and nothing is run. An update adds two more (the release's
  `SHA256SUMS.txt` and its archive, `https://` only), only when the player presses Update now;
  the archive is saved in the cache and run only after its checksum matched.
- Opens only `https://` pages in the browser (`launch::open_url`): the news, release and
  update links come from the network, and the system's opener would also run a program, a
  `\\host\share` path or a `file:` link; only URL characters pass (no spaces, quotes,
  backslashes or commas). Links inside a post's text go the same way: each frame ends by taking
  them out of egui's output (eframe is built without its own link opener). Its own
  folders open through `launch::open_folder`.
- Test aid: `NVF_LAUNCHER_SHOT=<file.png>` saves the window once the news has arrived and
  closes; `NVF_LAUNCHER_PAGE=home|settings|mods|game|coop` opens that page first (`settings:<tab>`: that tab; `delete-reports`: the reports question, open; `dlss-licence`: NVIDIA's licence before Download DLSS, open; `update`: the
  update dialog over Home, once a newer release is known); `NVF_LAUNCHER_ZOOM=0.6` draws everything smaller (a whole page in one shot).
  Only in debug builds and tests (`update::test_aids`, never in a release's launcher):
  `NVF_LAUNCHER_RELEASES=<url>` reads the release list from that address instead of GitHub (a
  local server with a fixture release), `NVF_LAUNCHER_VERSION=0.6.9` makes the launcher take
  itself for that version so a real release counts as newer (the restarted launcher inherits
  it), and plain `http` to this machine is allowed; a release's launcher fetches over HTTPS
  only.

## DLSS (#2765)

Settings > Graphics starts with an NVIDIA DLSS card (`src/dlss.rs`): the requirements with a
green or red dot (Windows; an NVIDIA RTX card by the name Windows gives the attached
displays' adapters, the game asks the driver itself at start; the four upscaling and four
frame generation files in the `dlss` folder), and Download DLSS…, Open dlss folder, NVIDIA's
release page, Check again. Download DLSS… opens NVIDIA's licence (`nvf_dlss_files::LICENCE`);
**I accept** takes a mouse click only, as in the game, and starts
`nvf_dlss_files::download::start_into`: the game's own download, moved into the shared crate
`crates/nvf-dlss-files` with its pins, signature checks and folder rule, so the two cannot
differ. Its progress shows on the card; the folder is `dlss` beside the game program the
launcher starts (`dlss::folder_for`; the same folder as the launcher's in a release, and
`engine/target/release/dlss` for a source checkout's launcher in `target/release`, where a
folder beside the launcher was never read by the game), `NVF_DLSS_DIR` when set. The launcher
never passes the developer test address.

## Updater (#2713)

`src/update.rs`; the deciding steps are pure functions with tests, the acting ones are tested
on fixture folders and a fixture zip/tar.gz (no real release is downloaded by any test).

- **Check.** Only while "Fetch news" is on (`[Launcher] News`): the release list is fetched
  with the news, so turning news off turns update checks off too. The newest release is the
  highest version of the list, drafts left out (every release so far is a pre-release, so
  `releases/latest` answers 404); it counts when it is later than the launcher's own version
  (the workspace version, the same as the game's in a release archive).
- **Which file.** The archive for this build's target, by the exact name the release workflow
  packs (`.github/workflows/release.yml`): `NVForever-<tag>-x86_64-pc-windows-msvc.zip` on
  Windows, `NVForever-<tag>-x86_64-unknown-linux-gnu.tar.gz` on Linux. Another system, or a
  release without that file, gets the release page only.
- **Integrity.** Every release carries `SHA256SUMS.txt` (the publish job's `sha256sum *`). The
  launcher reads it first, hashes the archive as it arrives and compares; no list, no line for
  the archive, or a different hash: nothing is installed (the partial file is deleted). This
  catches a broken or cut download, not a release replaced on GitHub itself (the list comes
  from the same place); signing the list (minisign or similar) would be the next step. What ties
  the archive and the list to this project is their address: both must be files of that very
  release of this repository, named as the asset is
  (`https://github.com/LootJunkieAzza/NVForever/releases/download/<tag>/<asset name>`,
  `update::fetchable`), or the update is refused.
- **Staging.** `<cache>/updates/<tag>/` in NVForever's cache folder (`EngineHome::cache`, the
  local data folder on Windows): `<archive>.part` while it downloads, the archive once checked
  (a second Update now with a checked archive there skips the download), `unpacked/`. The
  folder goes through `guard_write` (never the game folder or `My Games\FalloutNV`); the tag
  comes from the network, so it must be letters, digits, `.`, `_`, `-`. Unpacking refuses an
  entry that would land outside (`..`, absolute, a drive), links and devices, and an archive
  without `nvforever` in it (releases before v0.8.0 have no launcher: the running one stays
  and restarts as itself). Staging folders of versions no newer than the
  running one are removed at the launcher's start.
- **Where it installs.** The folder the launcher runs from, wherever the player unpacked the
  archive. Refused (the dialog says why and offers the release page) when the launcher is a
  build of a source checkout (an ancestor has `engine/Cargo.toml`: update with git), when
  `nvforever(.exe)` is not beside it, or when the folder is inside the game folder the INI
  names (`[Game] Path`): `check_install` refuses it, every path the swap writes goes through
  `guard_write` with that folder, and the launcher registers it at start (`remember_game`), so
  no write of the launcher's lands in it.
  The home (saves, `NVForever.ini`, `plugins.txt`, mods, logs) is a different folder and is
  never touched: it is not in the archive, and the swap only moves the archive's files and the
  ones the last update listed.
- **The swap** (`update::plan`, then `update::apply`), in two phases so that a launcher
  closed or a PC losing power at any moment never leaves an install that cannot start:
  1. Plan: copy every file of the unpacked release; keep a copy of each of those already in
     the install, and of each file of `update/installed.json` (what the last update installed)
     the new release no longer has. A file the player put in the folder is never in either
     list. A payload path outside the install or in `update/` refuses the whole update.
  2. Stage (`stage`; nothing the install runs from changes): the files to be replaced or
     retired are copied into `<install>/update/previous/v<running version>/` (beside the
     install, not in the cache, which may be on another drive), each new file is copied whole
     beside its place as `<name>.nvf-new`, then the journal `update/swap.json` is written. A
     failure removes what was made and says so (naming anything that could not be removed).
  3. Commit (`commit`; renames only): each `.nvf-new` takes its place (a running program, the
     launcher itself, is first renamed to `<name>.nvf-old`: Windows lets a running program be
     renamed, not replaced), the retired files go, `update/installed.json` is written and the
     journal removed.
  If the second rename of a running program fails (a virus scanner holding the new file), the
  old program is renamed back, so the install never lacks it; the journal stays.
  A launcher that finds the journal at its start (`resume_interrupted`, in the folder it runs
  from even when `nvforever` is not there) finishes the commit: every new file is whole by
  then, so the swap rolls forward. An install or a roll back finishes a pending swap first and
  refuses when it cannot, so a new swap never stages over the kept previous version; when it
  did finish one, it stops there (the install now holds that release, not the running one):
  an install returns that version and the launcher restarts into it, a roll back asks for a
  restart first. The journal names the folder it was written for, and is followed only
  there; its paths are checked before they are followed (plain paths below the install,
  outside `update/`, through `guard_write` with the game folder), and each file to put in
  place must be staged or already in place. The manifest write and the journal's removal go
  through `guard_write` too. Without a journal it removes
  stray `.nvf-new` (a swap cut short while staging; the old release runs as before) and
  `.nvf-old` files. While the swap runs the window does not close (the close is cancelled with
  a word) and Play waits. Afterwards only the one previous version is kept under
  `update/previous/`; the staging folder is deleted.
- **Not while it runs.** The game, the viewer and fnvtool of the install must not be running:
  the launcher waits while the game it started runs ("installs as soon as the game is
  closed"), and before the swap opens each program for writing, which Windows (sharing
  violation) and Linux (ETXTBSY) refuse while it runs; one that is running stops the install
  with its name.
- **Restart.** After the swap the launcher starts the new `nvforever-launcher` in the install
  with `--updated` and closes; the new one says "Updated to vX". A click on the window's X is
  refused only during the swap itself, so the launcher's own close afterwards goes through
  (#3117).
- **Roll back.** The Game folder page's Version card shows "Roll back to vX" when
  `update/previous/v<X>/` holds the game program. It is the same swap with the kept
  folder as the payload, so the current version is kept in its place (a second roll back goes
  forward again); the launcher restarts with `--rolled-back`.
- **The dialog.** Version, download size, the release notes (Markdown), "Your saves, settings
  and mods are not touched", Update now / Later / Release page. Then a progress bar (MB of MB
  while downloading, Cancel), "Checking the download", "Unpacking", "Installing". Offline, a
  failed check or a failed swap: the reason, "The NVForever folder is as it was", Try again /
  Release page. Closing the dialog lets a download carry on; the Update button opens it again.
  Downloads use their own HTTP agent: 15 s to connect and 30 s for any one read, no limit on
  the whole, so a slow line finishes and a dead one ends in an error.
- **Crates.** `zip` 9 (MIT; deflate only, no AES/bzip2/lzma/zstd), `tar` 0.4 (MIT/Apache-2.0,
  no xattr), `flate2` on zlib-rs and `sha2`, both already in the workspace; `cargo deny`
  passes.

## Not yet

- What kind a row is, its default and a list's choices come from the INI and a short table, not
  from the game. Better: the game writes its settings catalogue (kinds, steps' words, choices,
  defaults) to the home at each start, and the launcher reads it.
- Screen readers: eframe's `accesskit` brings zbus on Linux (unmaintained, RustSec), so it is off.
- No folder picker on Linux (rfd's portal backend brings zbus too): the folder is typed.
- Updater: the checksum list is not signed; no automatic install without the player's click;
  no update of a launcher installed outside a release folder (a package manager's copy).
