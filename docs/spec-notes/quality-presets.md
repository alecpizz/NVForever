# Graphics quality presets (#1966)

What the original's install holds for its quality presets, and how NVForever's settings
page reads them. Implemented in `engine/nvforever/src/quality.rs`; the page's layout in
`engine/nvf-play/src/settings_layout.rs` (docs/design/settings-page.md).

## The files

- The install carries four preset files in the game folder, beside `Fallout_default.ini`:
  `low.ini`, `medium.ini`, `high.ini` and `VeryHigh.ini` [documented: the install's files;
  also cited in impact-decals.md and world.md]. Each is a plain INI of `[Section]` headings
  and `key=value` lines.
- Between them they set **41 keys** in seven sections: `[Display]` 17, `[Water]` 11,
  `[TerrainManager]` 5, `[LOD]` 3, `[Grass]` 2, `[Imagespace]` 2, `[BlurShaderHDR]` 1
  [documented: the four files, read 2026-11-09]. Every file sets the same 41 keys.
- A key name's first letter gives its kind (`b` a switch 0/1, `i` a whole number, `f` a
  number) [hypothesis: by the names; it holds for all 41].
- What the original does with each number is not documented here, only what each name says
  [hypothesis]; the mapping below is NVForever's own [design].

**No value from the files is in NVForever.** The page reads them from the player's install
at run time, only reading them (`fnv_install::GameInstall::open` on `--data`, the root
folder, names matched without case). The unit tests and the smoke scenarios use made-up
files (`tools/smoke/fixtures/quality-presets*`) [design: LEGAL.md].

**Encoding** [design: #1966 review]: the install's files are plain 8-bit text with ASCII
keys and numbers [documented: the four files]. A UTF-8 byte order mark is dropped (it
would hide the first `[Display]` heading); a UTF-16 mark (FF FE or FE FF) decodes the file
as UTF-16; anything else is read as UTF-8, a byte that is not becoming a replacement
character, which no key or number holds. A preset left with no row (UTF-16 without its
mark, or not a preset file) is logged as setting no row.

## Which key sets which row [design: #1966]

Nine keys set a row. A key a file lacks leaves its row out of that preset.

| Section | Key | Row (`[Settings]` key) | How |
| --- | --- | --- | --- |
| `[Display]` | `fShadowLODStartFade` | Shadow distance (`shadow-distance`) | part, see below |
| `[Display]` | `bDrawShadows` | Sun shadows (`sun-shadows`) | on when not 0 |
| `[Display]` | `iShadowFilter` | Shadow filtering (`shadow-filter`) | 0 is Fast, any other number Smooth (the vanilla filter); Temporal is NVForever's own and no preset takes it |
| `[LOD]` | `fLODFadeOutMultActors` | Actor distance (`actor-distance`) | part |
| `[LOD]` | `fLODFadeOutMultObjects` | Object distance (`object-distance`) | part |
| `[LOD]` | `fLODFadeOutMultItems` | Item distance (`item-distance`) | part |
| `[Grass]` | `fGrassStartFadeDistance` | Grass distance (`grass-distance`) | part |
| `[Water]` | `bUseWaterReflections` | Mirrored water reflections (`water-mirror`) | on when not 0 |
| `[BlurShaderHDR]` | `bDoHighDynamicRange` | Light glow (HDR) (`glow`) | on when not 0 |

**A part** (the distance sliders are parts of their full distance, `draw_distance.rs`): the
file's number divided by the largest number the preset files found give that key, so the
fullest preset draws at 100% and the others in proportion; the row then brings it onto its
own range and steps (at least 10%, in 5% steps). The original's own distances are not
copied: the sliders keep NVForever's full distances (`--radius`, `--shadow-distance`,
`--grass-distance`). When only some files are found, the largest of those found is the 100%.
A key that is 0 in every file found sets nothing (no 100% to scale by) and the log says so.

The shadow distance row follows `fShadowLODStartFade` as the nearest counterpart: NVForever's
sun shadows have no other quality key, and the original casts no sun shadows from the world (the Sun shadows row's
note, `engine/nvforever/src/settings.rs`) [design].

## Keys not used [design: #1966]

No NVForever row counts for these, so a preset leaves them unused:

- `[Display] iMultiSample`, `iWaterMultisamples`, `bTransparencyMultisampling`: the
  multisampling. Left to the Anti-aliasing row until the maintainer decides (#1966's
  question: whether presets should set anti-aliasing and FSR).
- `[Display] iMaxAnisotropy`: the anisotropic filtering is one of the enhanced effects
  (16x on, 8x off), and the presets never touch those.
- `[Display] iMaxDecalsPerFrame`, `fDecalLOD1`, `fDecalLOD2`: the impact decals have no
  settings row (impact-decals.md).
- `[Display] iTexMipMapSkip`, `iTexMipMapMinimum`: no row sets a texture size.
- `[Display] iActorShadowCountInt`, `iActorShadowCountExt`, `iShadowMapResolution`: no
  row counts actors' shadows; the sun's shadow map size is the Shadow resolution row's
  (`shadow-resolution`, #2843), which no preset sets until the maintainer decides how the
  presets map to it.
- `[Display] fSpecularLODStartFade`, `fLightLODStartFade`: no row fades specular light or
  lights by distance.
- `[TerrainManager] fTreeLoadDistance`, `fBlockLoadDistanceLow`, `fBlockLoadDistance`,
  `fSplitDistanceMult`, `fHighBlockLoadDistanceLow`: distant land and trees are one switch
  (Distant terrain and objects), not a distance.
- `[Grass] b30GrassVS`: no counterpart.
- `[Imagespace] bDoDepthOfField`, `iRadialBlurLevel`: no row.
- `[Water] bUseWaterRefractions`, `bUseWaterReflectionBlur`, `bUseWaterDisplacements`,
  `bUseWaterDepth`, `iWaterReflectWidth`, `iWaterReflectHeight`, `bReflectExplosions`,
  `iWaterBlurAmount`, `bAutoWaterSilhouetteReflections`, `bForceHighDetailReflections`: the
  water has only the mirrored and the screen-space reflections as rows.

A key that is in neither list is logged as not known and left unused.

## The Quality row [design: #1966]

- Under its own heading, Graphics > Quality, the first row of the Graphics tab:
  `Low`, `Medium`, `High`, `Very High` (one per file found) and `Custom`. The names are our
  own words: the load order has no game setting for them [documented: FalloutNV.esm holds
  no "Very High" string, searched 2026-11-09].
- Taking a preset sets each row it maps through that row's own setter, as a change on the
  page would (each logged `settings: <key> <value>`, kept in the INI).
- The row **reads** the first preset whose values every mapped row (of those registered)
  holds, else Custom. Custom is what the row reads, not a choice to take: the list offers it
  only while it is read. Any change to a mapped row shows Custom; moving it back shows the
  preset again.
- A missing or unreadable file leaves its preset out of the list and says so once in the
  log (`quality presets: Medium is not offered: medium.ini not found in …`). With none of
  the four, the row is not registered at all (`… the Quality row is not shown`).
- Kept as `quality=<name>` in `[Settings]` of `NVForever.ini`. The stored name sets
  nothing at the start: the rows are kept on their own and are the truth, so a test run's
  command line still wins over the INI row by row (#1382).
- Defaults unchanged: nothing changes until the player takes a preset. With the owner's
  install, today's defaults (sun shadows on, Smooth filtering, light glow and mirrored water
  on, every distance at 100%) read as `Very High`.
- It never touches the enhanced effects, the anti-aliasing or FSR.

## The Enhanced effects row [design: #1966]

Under Graphics > Picture, its first row: `On` sets every switch `--graphics enhanced` turns
on and `vanilla` leaves off (ambient occlusion, longer sun shadows, bloom, sharpening,
anisotropic filtering 16x, sun shafts, screen-space water reflections; taken from
`graphics::Graphics::preset`, so a new effect joins by itself), `Off` clears them all, and
it reads `Custom` when they are mixed (then offered as the one chosen). The effects keep
their own rows. The light glow, eye adaptation, neon glow and mirrored water are on in both
looks, so they are not effects of it, and the two rows never set the same row. Kept as
`enhanced=On|Off|Custom`. As the Quality row's, the stored name sets nothing at the start:
each effect is kept on its own and read with its own command-line precedence (#1382), so
a test run given `--shadow-distance` keeps Longer sun shadows as the command line set it
even with `enhanced=On` stored [design: #1966 review]. The note names no command-line
switch, so the INI read never reports one overridden.

## Test aid and log

- `NVF_PLAY_TEST_QUALITY_PRESETS=<folder>` reads the preset files from that folder instead
  of the game folder.
- `quality presets: Low, Medium, High, Very High from <folder>; 9 key(s) set rows, 32 not
  used` at the start; `quality presets: <name> from <folder>: <n> row(s) set` when one is
  taken; `quality presets: the Quality row reads <name>` and `… the Enhanced effects row
  reads <name>` at the start and whenever it changes while the pause menu is open.
- Smoke: `settings-quality-presets`, `settings-quality-presets-missing`,
  `settings-quality-presets-part`, `settings-enhanced-effects`.

## NVForever's own rows (#2864)

[design] Beside the files' keys each preset sets Smooth terrain, a row the original does not
have: Off at Low and Medium, 4 x 4 at High and Very High (`quality.rs` `OWN`). On an Intel
N150's integrated GPU at 1920x1080 with the Low preset's rows, 4 x 4 costs 9 ms of a 73 ms
frame and 2 x 2 3 ms [observed: #2864]. The Quality row reads a preset only when this row
matches too. The row, and so a preset's change to it, applies at the next start.

## The Steam Deck preset (#3909)

[design] A fifth choice, **Steam Deck**, NVForever's own: High's rows as `high.ini` gives
them, with Sun shadows Off and Smooth terrain Off (`quality::STEAM_DECK_ROWS`, `OWN`). Like
the others it carries no value from the files: it is made from High's file each run, and is
offered only when that file was read. The two rows it changes are the original's own look
(no sun shadows from the world, docs/spec-notes/world.md §6; no smoothing of the landscape)
and the two that cost a Steam Deck's GPU most besides multisampling.

[design] **A Steam Deck's first start** (`quality::deck_first_start`): with no
`NVForever.ini` yet, or one marked `[Game] FirstStart=1` (`fnv_install::FIRST_START_KEY`:
whatever makes a new INI writes it, the first start, `fnvtool setup` and the launcher;
every start clears it to `0`, on any machine), and the machine a Deck (Linux DMI
`sys_vendor` and `board_vendor` both `Valve`; test aid `NVF_PLAY_TEST_STEAM_DECK=1|0`
decides instead; `first` also treats the start as a first start, for smoke homes that keep
their INI), the start takes Steam Deck
and Anti-aliasing SMAA as picks on the page would, so the INI keeps them and later starts
are the player's. It runs after the INI is read and before the run's smooth terrain is
fixed, so the first run already draws without it. A command line that names a row the
preset sets leaves the preset untaken, and an anti-aliasing switch keeps its value. Other
machines and later starts are unchanged. An INI made before the mark is a player's who has
started the game, even with no settings in it, so an update changes none of their rows.
Windows on a Deck is not recognised.

[observed] Steam Deck LCD, SteamOS, Mesa RADV, 1280x800, uncapped, SMAA, frames a second
in the steady windows: Goodsprings 12:00 59.3 with High's rows, 67.6 with Steam Deck;
Novac 12:00 52.5 and 62.3; Primm 12:00 54.6 and 68.1. With 4x multisampling instead of
SMAA, Steam Deck gave 61.0, 58.8 and 58.4.

## Main-menu edit measurement (#2248)

[design, #2248: engine/nvforever/src/quality_probe.rs;
engine/nvf-play/src/settings.rs] While the main menu is open, each effective
Quality choice or mapped row edit logs `quality measure begin:` and then one
`quality measure:` line. A preset's nested row setters are one measurement. No-op
sliders and already chosen list entries start no measurement. The window follows
the setter for `FOLLOW_SECS` (three real seconds), retaining any frame crossing
its deadline; overlapping windows remain independent. No renderer or gameplay
default is changed by this diagnostic. It is a test aid: it is added only with
`NVF_PLAY_TEST_QUALITY_MEASURE=1` (logged `test aid NVF_PLAY_TEST_QUALITY_MEASURE:
main-menu quality edits measured`); a player's run has none of its systems, its edit
hook or its lines [design, #2248 review: quality_probe.rs `register`].

[design: quality_probe.rs `watching`] Asset and entity sampling runs only while
the main menu is open or an edit's measurement is still active.

[design: quality_probe.rs] `longest_frame_ms` is the largest wall-clock interval
between main-frame starts, including the entire frame containing the edit. It
includes scheduling and renderer backpressure, rather than only CPU execution or
GPU time. `setter_ms` times the outer setter and its persistence. `elapsed_secs`
includes that setter and the follow-up; `frames` counts sampled main intervals.
`main_menu=true` means the menu remained open at every sampled interval; it does
not prove that the black UI cover was drawn on the GPU.

[design: quality_probe.rs, world.rs, graphics.rs, draw_distance.rs, pipeline_gate.rs]
The other fields describe work during the interval, including unrelated work:

- `cell_starts` and `reference_attempts`: cumulative calls to cell construction
  (`begin_cell`) and reference spawning (`spawn_ref`), including attempts which
  produce no visual. These cover new construction as well as repeated work; they
  are not a count of disk reads. Startup construction before an edit is excluded.
- `world_roots` and `visual_parts`: added world-root and RefVisual components;
  multiple parts can belong to the same reference. Deferred additions are counted
  at the end of the frame, rather than at the original request.
- `camera_spawns`, `shadow_updates`: added camera components (including secondary
  views) and calls to the sun-shadow setter (`sky.rs`), even without a sun entity.
- `graphics_applies`, `range_passes`, `range_meshes`, `range_materials`: graphics
  application passes past the unchanged-state guard, whole-scene distance passes,
  meshes visited by those distance passes, and meshes whose material those passes
  marked changed so its pipeline is chosen again with the fade dither (a mesh given
  its first range, e.g. a distance row moved below 100%).
- `pipelines_asked`, `pipelines_pending_peak`, `render_frames`: new pipeline
  requests, the largest pending count sampled by the main world, and completed
  render frames. A compile that starts and ends between samples may not appear in
  the peak. The existing `NVF_PLAY_TEST_PIPELINE_LOG=1` aid adds pipeline labels
  and request-to-ready times; neither aid times individual driver shader compiles.
- `mesh_events`, `image_events`: `[added, modified, removed]` asset events, sampled
  at frame end. Images include menu slides and UI assets; these are not necessarily
  world reloads. Events delayed until frame end can include earlier frame activity.
- `window_changes`: sampled changes to a window's identity, physical size, scale,
  mode or presentation mode. This does not count backend swapchain recreations.
- `ini_write_attempts`: writes of the settings file actually made, including failed
  ones; a held change (a preset and the rows it sets) writes once (`StoreHold`, below).

[design, #2248: tools/smoke/scenarios.toml] `quality-main-menu-measure` opens
Settings from the main menu, takes all four made-up preset fixtures, and then
edits each of the nine mapped rows alone, eight seconds apart. Stage 1 required
the thirteen summary lines, with no frame-time limits or zero-load expectations;
the fix adds two: each preset's line shows `ini_write_attempts=1`, and 0 cell
starts, reference attempts, world roots, visual parts and camera spawns.
It has run on Windows only (below); Linux, the reporter's system, is still
unmeasured, and so is a quiet machine before the fix. Pipeline compilation or
presentation behavior may differ by GPU, driver and window backend [hypothesis,
#2248]; source inspection alone does not establish the reported Linux hitch or the
cover becoming invisible. The repeated synchronous INI writes are present in the shared
setters [design: settings.rs `take`, `flip`, `set_slider`, `store`]. No observer
check or gameplay choice is added.

**What the first run measured** [observed: smoke run `quality-main-menu-measure`,
2026-11-17, Windows, on a machine loaded by builds so every frame took ~250 ms and
`longest_frame_ms` is mostly that baseline]. `main_menu=true` on every line;
`cell_starts`, `reference_attempts`, `world_roots`, `visual_parts`, `camera_spawns`
and `window_changes` were 0 for every preset and row, so the held house was neither
rebuilt nor respawned. `setter_ms` followed `ini_write_attempts`: Low 297.8 ms with 10
writes, Medium 279.9 ms / 8, High 217.2 ms / 8, Very High 168.6 ms / 6, each row alone
25-37 ms / 1, about 27 ms per write of `NVForever.ini` on the main thread. Low also
asked 57 pipelines and re-marked 732 materials (`range_materials=732`): Low is the
first edit to take the distance rows below 100%, so the house meshes that had no
range yet got one and their pipelines were chosen again with the dither. Every
other edit, presets and rows alike, shows `range_materials=0` (the meshes already
had a range). `shadow-filter` alone asked 20 pipelines.

**What the run after the fix measured** [observed: smoke run
`quality-main-menu-measure` on the branch with the fix and the test-aid gate,
2026-11-17, Windows, at about 60 fps]. Every edit shows `ini_write_attempts=1`. `setter_ms`: Low 18.5 ms, Medium
34.4, High 36.1, Very High 36.1; each row alone 29.7-55.1 ms. Low again re-marked 732
materials and asked 57 pipelines; every other edit 0 re-marked materials;
`shadow-filter` again 20 pipelines. The house again was not rebuilt (all cell,
reference, root, part, camera and window counts 0). The two runs were on differently
loaded machines, so the drop per preset is shown by the writes (10/8/8/6 to 1), not
by comparing their milliseconds directly.

**The fix: one INI write per change** [design, #2248: settings.rs `StoreHold`]. A
list choice (`take`, so a preset through `pick` and the test aids' numbers through
`load_text`) and an older key's choice (`pick_alias`) hold the INI write while their
pick runs; the rows they set ask for a write and it is done once, when the outermost
change ends, with every row's new state (also when the pick sets rows and then
refuses). A row changed alone writes once, as before. `ini_write_attempts` counts the
writes actually made, so a preset now reads 1. The write stays on the frame: the
same file is read-modified-written by other synchronous writers, so a write on
another thread could race them and lose lines; with one write per change the cost
is one write, not ten [design]. The pipelines a renderer row asks for the house
behind the menu are left as they are: they are the ones the world needs once the
menu closes, so deferring them would move the wait to New Game.

**What the code says about the house behind the menu** [design, #2248: code read on
main 72b385a57, not yet measured]. The held new game in Doc Mitchell's house is built
once at the start (`nvf-play/src/main_menu.rs` module notes); none of the nine rows'
setters touches the streamer or a cell: the distance rows write `DrawDistances` or
`GrassConfig` (`draw_distance.rs` `set_part`, `set_shadows`, `set_grass`), the shadow
rows `SkyState` and `Graphics`, `water-mirror` and `glow` `Graphics`. So a preset change
neither re-creates nor reloads the house by any path these setters reach; `cell_starts` and
`reference_attempts` will show whether something unforeseen does. The house is,
however, drawn every frame behind the menu: no camera is made inactive while the menu
is open, and the menu's black root (`MenuRoot`, `GlobalZIndex(100)`) only hides it in
the final picture; nothing hides that root itself while the menu is open (only its
children change visibility, `show_entries`, `slideshow`). A renderer row therefore makes
its new pipelines for the house's meshes (and shadow casters) behind the cover: work
for a picture nobody sees. A distance row walks every house mesh (`apply_ranges`,
`range_meshes`) but marks a material changed only for a mesh that has no
`VisibilityRange` yet and is now to be culled (`draw_distance.rs` `apply_ranges`),
not on every change. What the runs measured is in "What the first run measured" and
"What the run after the fix measured" above [observed]: the first edit below 100%
(Low) re-marked 732 materials, every later edit none.
