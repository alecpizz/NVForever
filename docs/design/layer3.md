# Layer 3 design: world and rendering

Spec section 5 Layer 3 and milestone M3 ("walk a free camera through the Mojave with
correct terrain and objects"), in the section 9 format. Status 2026-10-07: **world data
layer done and checked on the owner's install; the world renderer draws terrain, static
objects, a weather-driven sky with time of day, water and distant LOD around a free
camera; world-data parity 92/92; clouds, SpeedTree and screenshot parity not started.**
Since then (2026-10-25, docs/status.md §2): SpeedTree trees are drawn (`fnv-spt`), clouds
are drawn with their placement still the gap (§5 item 5), and the reference frames meet the
image threshold in 7 of 7 interiors and 2 of 8 exteriors.

## 1. Summary and assumptions

Task 3: cell and worldspace streaming, landscape (LAND/LTEX), object placement (REFR),
lighting, water, sky and weather, LOD, rendered through Bevy and emulating the original
shading intent rather than D3D9 fixed-function state. Parity (spec §8): rendered frames at
fixed cameras (image metric), cell reference lists and terrain heights (keyed).

Assumptions:
- Built on Layers 1 and 2 as they stand (PR #2 not yet merged; this branch is stacked on
  it).
- Record layouts stay schema-as-data: new record types needed by the renderer (WATR,
  WTHR, CLMT, LGTM, IMGS, TREE, MSTT, ...) are added to `schemas/fnv` from xEdit's
  public definitions as in Layer 1, never hard-coded.
- Actors (ACHR/ACRE) are placed by Layer 3 only as markers; drawing NPCs and creatures
  needs records from Layers 4–6 (race, body parts, equipment). Static NPCs assembled as
  one entity per actor are planned as workstream 5 (docs/design/static-actors.md).
- The first target is static and correct (M3). Physics, scripts and AI stay out.

## 2. Findings and design

### 2.1 World data (`crates/fnv-world`, no engine dependencies)

| Module | Role |
|---|---|
| `fields` | schema-decoded subrecords of one record; plugin-local → load-order FormIDs |
| `index` | `WorldIndex`: worldspaces, cells (grid, flags, water), which references and LAND belong to which cell, across the load order with overrides |
| `placed` | a reference's placement (base, position, rotation, scale, flags, enable parent) and its base object's model |
| `land` | heights, normals, colours and texture layers of a LAND |
| `transform` | the reference rotation convention |

Findings (details and tags in `docs/spec-notes/world.md`):
- Every worldspace with references has one persistent cell directly in its world group;
  exterior cells are 4096 units square and 99.9% of temporary references lie inside their
  cell's square [observed].
- LAND heights integrate row by row, rows south → north; shared cell edges then agree
  exactly for 99.85% of neighbouring pairs, and normals are signed bytes matching the
  height gradient to ~1° [observed].
- Reference rotation is `Rx(−x)·Ry(−y)·Rz(−z)`, settled by matching static collections'
  part placements to their combined meshes (286 of 308 multi-axis parts; next best
  candidate 85) [observed].
- Building the index over all ten masters takes about a second; no orphaned groups, no
  unreadable records.

`fnvtool world list | cells | refs | land | heights | check` expose this as JSON
(`docs/cli-contract.md`, Layer 3). `world check` is the evidence report behind the
findings above.

Schema additions: SCOL (parts and placements), PGRE (placed grenade).

### 2.2 Rendering architecture (ADR-0002)

The Bevy code is one workspace, `engine/` (renamed from `viewer/`), so the viewer and the
game share the asset plugins:

| Crate | Role |
|---|---|
| `nvf-assets` | ArchivePlugin (VFS), NIF → entities, prepared static models shared by all references, DDS → images, KF playback |
| `nvf-viewer` | the M2 asset viewer, unchanged in behaviour |
| `nvforever` | the game binary; at M3 a free-camera explorer. `world.rs` (streaming, references), `terrain.rs`, `fly.rs` |

`nvforever --data <Data> [--world W] [--cell EDID|x,y] [--pos x,y,z] [--heading deg]
[--pitch deg] [--radius n] [--screenshot out.png] [--size WxH] [--hide text,...]`

What it does now:
- Streams the (2r+1)² cells around the camera, nearest first, one per frame; unloads with
  one cell of hysteresis. The worldspace's persistent references are assigned to cells by
  position (world.md §1).
- References: drawn when enabled at game start (not deleted, not initially disabled,
  enable parent in the required state) and the base is a static-like type with a NIF
  model; actors, sounds, idle markers, editor marker meshes and SpeedTree trees are
  counted, not drawn. Transform = position · `Rx(−x)Ry(−y)Rz(−z)` · scale.
- Terrain: per quadrant, base texture + alpha-blended layers (ADR-0002 §4), stored
  normals, vertex colours (sRGB → linear). Textures repeat every 256 units and are lit
  with their normal maps (world.md §3 "Texture repeat", "Landscape normal maps").
- Materials as in the viewer, plus: nodes named `EditorMarker*` are skipped, additive and
  multiplicative NIF blend modes are honoured, alpha test wins over blending, and
  BSShaderNoLightingProperty is unlit (nif.md §6). NIF shapes use `NifMaterial`
  (StandardMaterial + `nif_material.wesl`): emissive light × diffuse texture and vertex
  colour, and no ambient specular (nif.md §6 "Emissive light", "Specular"). Emissive = colour × multiplier ×
  glow map from texture-set slot 2 (neon signs, lit windows; nif.md §6 "Emissive
  light"); material colour and emittance controllers are not evaluated yet. Helper shapes (properties but no
  shader property and no texture: bone boxes, collision proxies) are skipped (nif.md §6).
- Animated and skinned objects: skinned shapes are drawn in their bind pose. Models with
  an embedded `Idle` sequence (flags, vultures, dust effects) are spawned per reference
  as their own node hierarchy, skinned shapes bound to it, and the sequence loops
  (`nvf_assets::model::Rig`, `anim_play::Playing`); overlay "animated" counts them.
  Morph controllers (the flags' small ripples) are not evaluated yet.
- Sky, sun, ambient light and fog from the worldspace's climate and weather at a time
  of day (`--hour`, `[`/`]` keys; `--weather`; `--fog near,far` override): the game's
  `meshes\sky\atmosphere.nif` dome coloured with sky-upper (its blue vertex weight) and
  horizon (a fitted function of elevation), mixed as display values, and the four
  `clouds.nif` layers with the weather's cloud textures and colours (world.md §6 "Sky
  colours"), a directional light with the weather's sunlight colour along a sun path,
  the climate's sun and glare textures drawn around the sun (world.md §6 "Sun and
  glare"), ambient and fog colours. Sky parts use their own material (`sky.wesl`): their
  colours are display values passed through the inverse of the tonemapper, so the
  image-space grade sees them as the game's grade does; by day they are dimmed and
  warmed and the clouds take on part of the sunlight's hue [hypothesis, fitted to the
  reference frames]. Colours
  use the four main times of day only (world.md §6). Light levels: a colour of 1.0 lights
  a white surface to 1.0 on screen (sun π / exposure, ambient 1 / exposure), from the
  game's texture × (ambient + sunlight · N·L) model [hypothesis]; this replaced levels
  tuned by eye that were about 4× too bright in sunlight. Sun path fitted to the original
  game's sun positions: rises at sunrise begin, sets at sunset end, 9° south of the
  east-west line (world.md §6). Sun shadows: four cascades out to `--shadow-distance`, three on Performance (three and two until #2843) [engineering choice, #2843: for the maintainer to decide]
  (default 8,000 units since 2026-10-12, #254; 0 turns them off). The original game casts none from the world
  (world.md §6); they were on by default by owner decision (2026-10-09) and are off by
  default, behind `--sun-shadows`, from 2026-10-13 (owner decision: frame cost and flicker), and on by default with `--play` again since 2026-10-26 (owner, the first-person flicker fixed; `--no-sun-shadows`; runs without `--play` keep them off). The directional light
  fades out within ~6° of the horizon, where it switches between sun and moon
  [interim]. Observed in the time-lapse: NVWastelandClear's night ambient (luminance
  0.42) is brighter than its day ambient (0.14), so nights look brighter than late
  afternoon; the weather's per-time image-space links (world.md §6, not read yet)
  probably adjust this in game.
- Night sky (world.md §6): the climate's star model and a phased moon disc, faded in by
  sun elevation. The climate's weather follows the hour: the renderer stands in for
  NightQuestScript's `VNight` rule (night weather 21:00-04:18) and crossfades weathers
  (colours, fog and a second set of cloud layers) over half a game hour. `--day <n>`
  sets the days passed (moon phase; default the `GameDaysPassed` global).
- Image-space grade (world.md §6, `imagespace.rs`/`imagespace.wesl`): a full-screen pass
  after tonemapping with the worldspace's image space and the weather's adapters for the
  time of day (warm amber by day, darker grey-navy at night); `--no-grade` turns it off.
  The same pass draws the image space modifiers scripts play on the screen (fade to a
  colour, double vision, radial blur, changes to the grade), after a two-pass blur
  that runs only while a modifier blurs (`imagespace_blur.wesl`;
  docs/spec-notes/image-modifiers.md §5); `--imod <record>[@seconds]` shows one.
  Compared with the owner's screenshots: night ground still a little bright (HDR eye
  adaptation not applied), and the game still shows sunset colours at 20:00 where the
  renderer is already at night (time-of-day keys to revisit).
- Distant LOD (`--no-lod` turns it off; lod.rs): the game's terrain chunks at levels
  4, 8, 16 and 32 and its level-4 object LOD blocks (world.md §7). Which chunks are drawn
  [hypothesis]: a quadtree over the level-32 grid; no terrain chunk beyond
  `--lod-terrain-distance` (250,000, or `--lod-distance` when that is larger: a default
  calibrated at one pose above Goodsprings, where 250,000 was the first value tried at
  which our skyline matched the original's [observed: capture of the original and our
  runs, 2026-11-19, #3055; world.md §7 "Terrain reach"]; the original's own terrain
  distance, and one fixed reach everywhere, are [hypothesis], no INI key gives it); a
  coarser chunk gives way to its four quarters while the camera is nearer to it (horizontally, closest point) than the larger of
  `--lod-near-distance` (fBlockLoadDistanceLow, 50,000) and `--lod-split`
  (fSplitDistanceMult, 1.5) × its side. All levels have 256 texels per chunk side, so
  level 4 reaches 50,000 units, level 8 98,304, level 16 196,608 and level 32 the rest.
  Distant water planes cover the cells with water under the chosen chunks, so they end
  where the terrain does. The choice is redone when the camera enters another cell and
  levels switch at once (the game's `fBlockMorphDistanceMult` suggests it morphs between
  levels; not done). Neighbouring chunks share only some edge vertices (world.md §7), so
  every chunk's open edges get skirts hanging 128 units per cell of chunk side.
  Object blocks are drawn over every 4 × 4-cell square nearer than `--lod-distance`
  (fBlockLoadDistance, 125,000).
  Distant trees (treelod.rs, `--tree-distance`, fTreeLoadDistance 40,000; 0 off): beyond
  the loaded cells every TREE reference is drawn as its billboard, an upright card
  turned towards the camera in the vertex shader, one mesh per cell and TREE base; the
  game ships no tree LOD (world.md §7) [hypothesis: distant trees are these billboards].
  They crossfade with their cells like the object blocks and cast no shadows.
  The settings page's Tree distance row (#3016) is a part of the distant LOD's far limit
  (`--lod-distance`, 125,000 by default) in steps of 0.02 (2,500 units at the default),
  shown as the distance it gives; it ends at that limit so the billboards stop where the
  object blocks around them stop; distant terrain reaches farther, `--lod-terrain-distance`,
  250,000 by default (#3055) [design]. It starts at `--tree-distance`'s share
  (0.32, 40,000 units at the defaults; a flag beyond the limit is taken as the limit), so an
  untouched game draws as before and the slider can draw the billboards farther than the
  flag; `--tree-distance 0` still draws none whatever the slider. Applied live: the wanted
  cells are chosen again at once when it moves.
  Load distances follow the owner's `FalloutPrefs.ini` (world.md §7 table): uGridsToLoad
  5 → `--radius 2` (5 × 5 cells, unchanged), fBlockLoadDistance, fBlockLoadDistanceLow,
  fSplitDistanceMult and fTreeLoadDistance as above. Not applied: uExterior Cell Buffer
  (no cell cache; cells unload one ring outside the loaded square), uGridDistantCount and
  uGridDistantTreeRange (older distant-land settings [hypothesis]), fLODFadeOutMultObjects,
  fLODMultObjects, fLODFadeOutPercent and fLODMultTrees (objects in loaded cells are not
  faded by distance; level-design.md C5). fGrassStartFadeDistance is `--grass-distance`
  (grass, work item 11). Terrain
  LOD gets a negative depth bias; where a chunk overlaps the loaded cells, triangles
  entirely inside are dropped and the vertices inside are sunk 2,000 units so the
  border triangles slope under the loaded terrain. LOD is cut out only for cells that
  are actually loaded (cells stream in one per frame; cutting it for the cells merely
  wanted left the far edge empty until they arrived, the "pop" seen at a distance).
  Object blocks are drawn whole over squares with no loaded cell and split by cell
  elsewhere, by the block's own segments: each holds the objects of one cell whole, so
  an object reaching over the edge of the loaded cells is neither cut nor drawn twice
  (world.md §7 "Objects"). Cells streamed in after the start fade in, and cells streamed out fade
  out before they are dropped (`--cell-fade`, default 1 s, 0 off; fade.rs): their
  meshes are dithered with Bevy's visibility-range crossfade driven by time, and the
  cell's part of the object block with the complementary pattern; the LOD terrain
  under a fading cell stays solid behind it until the cell is fully drawn. The game
  fades objects at its LOD transitions [observed: owner]; dithering and the duration
  are [hypothesis]. Terrain chunks are shaded by their model-space normal
  maps alone (world.md §7): each vertex gets an up normal and an east tangent so the
  standard tangent-space normal mapping reads the map as model-space normals; the
  untextured water-height shape is skipped (water planes cover it).
  Within `--lodgen-radius` chunks (default 4) the game's chunks are replaced by terrain
  generated from the LAND records at 4 × their texture resolution, cut out exactly
  around the loaded cells and cached in NVForever's home (docs/design/generated-lod.md). Interim rules, to compare with screenshots. Dark boxes on
  ridges (the "checkerboard squares" seen east of Goodsprings) are object LOD block
  geometry over cells beyond the 5 × 5 loaded square, not a fade dither: they stay with
  `--tree-distance 0`, vanish with `--no-lod` (with the hill) and with `--radius 4`, which
  draws that hill's rocks and trees as the game's `goodsprings_east` frame shows them
  [observed 2026-10-09]. Cause [observed 2026-10-10]: the boxes were the blocks' tree
  cards (and the solid base under the water tower its legs) drawn opaque. The block atlas
  cuts them out in its alpha channel, but the block meshes have no NiAlphaProperty, only
  the "LOD_Building" shader flag; shapes with that flag are now alpha-tested (world.md §7
  "Objects", `nvf-assets` `shape_alpha_mode`; cutoff [hypothesis]).
- SpeedTree trees (TREE; distant ones as billboards, see Distant LOD): geometry generated from the `.spt` parameters by `fnv-spt`
  (speedtree.md §5): bark tubes with the standard material, leaf cards turned towards
  the camera by a vertex shader (tree.wesl, also in the prepass so depth and shadows
  match), scaled to the billboard size. Leaf cards cast shadows but receive none (their
  own light-facing shadow copies blackened the lower half of every bush); instead the
  crown darkens towards its bottom by the TREE record's CNAM leaf and branch dimming,
  as vertex colours (speedtree.md §6). Other objects still shade them: a card takes
  the sun's shadow at the point where the ray towards the sun leaves the tree's crown
  sphere (the cards' places plus a card's reach, rocking with the tree), past its own
  cards and before anything else (tree.wesl `NVF_CROWN_SHADOW`; vendored bevy_pbr
  `pbr_functions.wesl`). Without it, shrubs in a cliff's shadow were lit as in full sun
  [observed 2026-11-23, 0x00109682, WastelandNV (-18, -3)]. Lamps' shadows stay off
  for the cards. Trees rock and their leaves rustle in the
  weather's wind (WTHR wind speed, TREE CNAM rock and rustle speeds; tree.wesl, also in
  the shadow pass). A tree that fails to generate falls back to its
  billboard: one upright quad turned towards the camera every frame, normals up.
- Interiors: `--cell <interior Editor ID>` loads that one cell with its own lighting
  (XCLL merged with its lighting template, world.md §8) instead of sky, LOD and terrain;
  the camera starts at the middle of its references. Lights: every enabled LIGH
  reference becomes a point (or spot) light, indoors and out, reaching its base radius
  plus the reference's XRDS (world.md §8; `--light-scale`, tuned by eye in
  GSDocMitchellHouse). Interior frames (Doc Mitchell's house, Prospector Saloon) are
  image scenarios like the exterior ones (`L3.frame.int_*`, tag `interior`).
- Test aids: `--time-scale <h/s>` runs the clock at that many game hours per real second
  once the start cells are loaded, logging a "time-lapse" sample (sun elevation, light
  and sky luminance) every game hour; `--timelapse <dir>` also saves a frame per hour
  (the day/night smoke test in CLAUDE.md), or every `--timelapse-step <game minutes>` for
  a video (frames `NNNN_HHMM.png`; at `--time-scale 0.25` a 3-minute step gives 480
  frames per game day, none dropped at 1600x900). `--auto-fly <speed>` flies the camera forward, `--run-seconds <n>` exits
  after n seconds; together a headless smoke test of streaming and LOD rebuilds (40 s at
  4000 units/s streams 200-260 cells). Meshes on the GPU are render-world only and cannot
  be read back; code that derives meshes later (LOD trimming) asks the model cache for
  CPU copies (`ModelCtx::model_with_cpu`).
- Water (`engine/nvforever/src/water.rs`, `water.wesl`): one plane per "has water"
  cell at its water height, in loaded cells and, out to the LOD range, in every cell
  whose water lies above its lowest landscape point (distant water). A custom material
  fogs what lies behind the surface by the view ray's length in the water (depth prepass),
  shallow to deep colour, reflects the world with a fresnel weight, adds a sun
  highlight and ripples from three scrolling noise layers. World reflections
  (`reflection.rs`): a second camera, the main one mirrored in the water plane nearest
  it, renders the scene above that plane (oblique near clip plane, as in Bevy's
  `mirror` example) at half the window size into an HDR texture; water lives on its own
  render layer, which that camera leaves out; it is off indoors and when no water lies
  within two cells below the camera. Refraction: the water is drawn after the opaque
  pass with Bevy's view transmission texture (a copy of it), which it samples with a
  wave offset and composites itself over. Under water (`underwater.rs`), the camera's
  fog becomes the water type's under fog; Snell's window from below. Ripples
  (`ripples.rs`): two damped wave simulations on a grid around the camera, fed by rain
  drops while the weather is "Rainy" and by the camera or player moving through the
  surface, added to the wave normal. The model and its hypotheses: world.md §6.
- Rain itself is not drawn: in the masters only Honest Hearts' two Zion weathers are
  rainy, and their rain is scripted content (activators that a quest script moves onto
  the player, world.md §6), so it comes with the script engine (Layer 4).

Measured on the owner's machine (i7-7700HQ, GTX 1050): 5 × 5 cells around Goodsprings
(1,948 references drawn, 3,085 shapes, 224 models, 506 terrain layers) load in 2.5 s;
7 × 7 cells (2,386 references, 280 models) in 2.6 s. No model or texture failures.

**Owner's visual check (2026-10-07)** [observed]: flying through the Mojave in
`nvforever`, object placement (positions and rotations of NIF models in the worldspace)
and terrain heights look as expected compared with the original game. Still open:
terrain texturing details, lighting, sky, water, LOD and SpeedTree (below).

Terrain texture repeat: measured against the reference frames on 2026-10-10 as 256 units
with v growing northwards (world.md §3 "Texture repeat"; `--terrain-tile` now defaults to
256, the earlier guess was 4096/6). The tangent frame of the landscape normal maps is a
hypothesis and terrain specular is open (world.md §3 "Landscape normal maps").

Rendering hypotheses to check against screenshots: alpha-grid orientation, layer order, triangle
diagonal, the alpha test/blend rule, the default texture for the 43 quadrants without a
base layer seen around Goodsprings (drawn neutral grey).

Coordinate mapping as in the viewer: game Z-up → Bevy Y-up at a single root transform;
game units unchanged.

### 2.3 Work items, in order

1. **World data + checks** — done (2.1).
2. **Parity for world data**: scenarios comparing `world refs` (keyed by reference
   FormID) for a sample of cells per worldspace, and `world heights` (keyed by grid) per
   worldspace. Oracle: an independent clean-room Python reader (`tools/capture/
   oracle_world.py`, written by a separate agent from the spec notes and CLI contract
   only), plus an optional FNVEdit export procedure.
3. **Terrain** (first version done): one mesh per cell (33 × 33 grid, stored normals, vertex colours), texture
   splatting with a custom material (base texture per quadrant + up to N alpha layers),
   matched against in-game screenshots.
4. **Objects** (first version done): spawn each reference's model with its transform; skip deleted,
   initially-disabled and marker references; reuse meshes and materials across
   references of the same base object.
5. **Streaming** (synchronous version done): load the cells within a radius of the camera (the original uses a
   square of `uGridsToLoad` cells, 5 [observed: the owner's `FalloutPrefs.ini` and
   `Fallout.ini`], so `--radius 2`); load on background tasks; despawn outside a
   hysteresis margin. Distant load distances follow the owner's INI (Distant LOD above).
6. **Lighting, sky, weather** (first version done; interiors: cell lighting, placed light
   radius (XRDS) and emittance done, seven interior reference frames; clouds, stars, light
   falloff shape, fog power and Vault/REPCONN frames to do): sun direction and colours from the climate/weather records
   by time of day; fog; interior lighting from XCLL / lighting templates; point lights
   from LIGH references.
7. **Water** (second version done: WATR fog, fresnel world reflections, refraction, under-water fog, noise layers, rain and wake ripples, distant water; falling rain to do with the script engine, Layer 4): worldspace / cell water height, WATR records, placeable water (PWAT).
8. **SpeedTree** (done: geometry, leaf dimming, wind, distant billboards out to fTreeLoadDistance):
   TREE references (18,050 in WastelandNV, nearly all WastelandShrub01) use SpeedTree
   `.spt` parameter files from `trees\`; format study in speedtree.md, parser and
   generator in `crates/fnv-spt`, `fnvtool spt list|dump`.
9. **LOD** (done: terrain levels 4-32 with switching by distance, skirts, object blocks,
   distant trees, distances from the owner's INI; level morphing not done): distant
   terrain and object LOD from the game's LOD meshes and textures (world.md §7).
10. **Image parity** (tooling done, waiting for reference frames): the lab's `image`
   comparator (`parity-lab/fnvlab/image.py`: PNG/BMP decoding, SSIM, colour error, masks,
   tile breakdown; self-tests), `nvforever --fov --eye-height --frame-json --no-overlay`
   (window scale factor pinned to 1 so frames are exactly `--size`),
   `parity-lab/tools/gen_layer3_frames.py` (owner's manifest → private scenarios), and
   the capture procedure in `tools/capture/README.md` §7. Rendering is deterministic:
   two runs of the same camera give identical images (SSIM 1.0, MAE 0) [observed].
   First reference frames (2026-10-09, four cameras at 16:9): `--fov` now follows the
   game's Hor+ rule (the setting is the horizontal angle at 4:3; 75 → 91.3° at 16:9),
   which aligned all four frames [hypothesis for the rule; docs/spec-notes/camera.md].
   Originally planned: an image comparator in the lab (perceptual metric with region masks,
   standard library only), a capture procedure for the original game, and fixed-camera
   scenarios.
12. **Anti-aliasing** (done): the main view is multisampled (`--msaa`, default 4; the
   owner's game runs 8), and the cut-out edges of alpha-tested shapes (plants, fences,
   hair, tree leaves and billboards, grass) are smoothed with it, standing in for the
   game's transparency multisampling (nif.md §6 "Alpha", soft edges; grass.md §5).
   Engine-level improvements beyond the original's look (ambient occlusion, finer shadows,
   bloom, sharpening, 16x anisotropic filtering) are optional: `--graphics enhanced`,
   docs/design/graphics-enhanced.md.
11. **Grass** (done: generation from the landscape textures, soft strand edges, wind,
   distance fade; the placement rule is a hypothesis and the light weights are fitted,
   both checked against the owner's screenshots at the Goodsprings sign): LTEX `GNAM` →
   GRAS, `fnv_world::grass`, `engine/nvforever/src/grass.rs`, `--no-grass`,
   `--grass-distance`, `--grass-density`, `--grass-min-size`, `--grass-light`; facts and
   open questions in docs/spec-notes/grass.md.

## 3. Code, schema and tests

- `crates/fnv-world` (unit tests per module), `crates/fnvtool/src/world_cmd.rs`,
  `crates/fnv-world/examples/scol_rotation.rs` (evidence program).
- `engine/` (ADR-0002): `nvf-assets` (model cache, shared materials, blend modes,
  editor markers; unit tests), `nvforever` (world streaming, terrain, free camera).
- Schemas: `objects.json` SCOL, `world.json` PGRE.

## 4. Validation plan and exit criterion (M3)

M3 is met when a free camera can travel the Mojave with terrain and objects that match
in-game screenshots at a set of fixed cameras (image scenarios), backed by keyed
scenarios for references and heights. The image threshold and masks are set with the
owner once the first comparisons exist; they are then frozen like any other tolerance.

## 5. Open questions and verification steps for the owner

1. ~~**Reference screenshots** (needed for work items 3–9).~~ → Done without the owner
   at the game since 2026-10-09: `tools/capture/capture_frames.py` places the camera through
   the `nvf_capture` plugin and saves the game's own frame (plugin v7 screenshot step);
   `parity-lab/tools/gen_layer3_frames.py` turns the manifest into `L3.frame.*` image
   scenarios (tools/capture/README.md §7). The owner reviews the frames for lighting and
   the world's look; actors in shot need no masks.
2. ~~**INI values**~~ → read from the owner's `FalloutPrefs.ini` (read-only, 2026-10-09)
   [observed]: `uGridsToLoad=5`, `uExterior Cell Buffer=36`, `uGridDistantCount=20`,
   `uGridDistantTreeRange=5`, `fBlockLoadDistance=125000`, `fBlockLoadDistanceLow=50000`,
   `fTreeLoadDistance=40000`, `fLODMultObjects=5`, `fLODMultTrees=0.5`,
   `fLODFadeOutMultObjects/Items/Actors=15`, `fLODFadeOutPercent=0.6`,
   `fGrassStartFadeDistance=7000`, `fDefaultFOV=75`, `fDefault1stPersonFOV=55`.
   `Fallout.ini` has the same grid values but smaller fade multipliers (Items 3, Objects 5,
   Actors 6); the game reads `FalloutPrefs.ini` for these display settings [hypothesis].
3. A first look: `cargo run --release --bin nvforever -- --data <Data> --cell Goodsprings`
   (from `engine/`) and fly around; anything that looks wrong compared with the game is
   useful (missing or extra objects, wrong terrain textures).
4. ~~Confirm in game that initially-disabled references are hidden at the start of a
   new game in the cells we test (expected for quest-gated objects).~~ → Owner
   2026-10-08: disabled references are not drawn, have no collision and no interaction.
5. **Exterior reference frames** (queue item 31, 2026-10-21): 2 of 8 exterior frames meet
   the 0.6 SSIM threshold (spring_mountains_east 0.71, black_mountain_north 0.60, on the
   line); all 7 interior ones do. Lab runs (`python -m fnvlab run --scenario "L3.frame.*"`,
   main's build, then the branch's), SSIM before to after: goodsprings_noon 0.477 to 0.476,
   goodsprings_dusk 0.487 to 0.487, goodsprings_hill_lod 0.557 to 0.557, lake_shore_noon
   0.505 to 0.504, black_mountain_north 0.602 to 0.601, goodsprings_east 0.439 to 0.438,
   spring_mountains_east 0.709 to 0.712, hoover_ridge_lake 0.562 to 0.559; mean colour
   error spring 0.077 to 0.046, hoover 0.048 to 0.030, black mountain 0.025 to 0.015. The
   change: the fog takes on half the sunlight's hue by day (world.md §6 "Fog colour by
   day"). The light's strength is not what fails the frames: the near ground's colours
   agree within a few levels. What does: the clouds lie elsewhere (with the game's sky
   pasted in, all eight would pass at 0.63-0.85), and the renderer's cloud texture
   placement does not match the game's by any offset (world.md §6 "Cloud texture
   placement"). *For the owner:* (a) add each frame's seconds since the save was loaded
   to `frames/manifest.json` (22, 41, 59 and 79 s for noon, dusk, hill_lod and lake per
   world.md §6; the other four are not recorded) so the scenarios can pass it on, and
   (b) captures of a still sky to learn the game's cloud mapping: one camera twice, a
   known number of seconds apart, and once looking straight up.

## 6. Updated spec notes

`docs/spec-notes/world.md` (new): world structure, persistent cells, cell size, water
height sentinels, references and their flags, LAND layout with evidence, the rotation
convention with evidence, open questions.
`docs/spec-notes/grass.md` (new): grass types, the textures that grow them, the INI
settings, NVForever's generation rule and how grass is drawn.
`docs/spec-notes/level-design.md`: Bethesda's GECK tutorials on rooms, portals,
occlusion planes, object fade, static collections and lights, with what the renderer does
differently (its §11.2 rows C1-C5, L1-L4).
