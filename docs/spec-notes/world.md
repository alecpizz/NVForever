# Spec note: worldspaces, cells, references and landscape (Layer 3 data)

Clean-room description in plain language; `crates/fnv-world` is implemented from it.
Container structure (records, groups, group types) is in `esm.md`; record layouts are in
`schemas/fnv/world.json` and `objects.json`. Tags: **[documented]** (source named),
**[observed]** (checked on the owner's FNV 1.4.0.525 masters, the base game plus all
nine DLC/pack masters in load order, with `fnvtool world check` unless stated otherwise),
**[hypothesis]**.

Sources: UESP "Tes4Mod:Mod File Format" (group layout, LAND); xEdit
`wbDefinitionsFNV.pas` / `wbDefinitionsCommon.pas` (record layouts, flag names), cited per
entry in the schemas.

## 1. Worldspaces and cells

- A worldspace (WRLD) is followed by a world-children group (type 1) labelled with its
  FormID. Inside it, exterior cells sit in block (type 4) and sub-block (type 5) groups;
  each CELL record is followed by its cell-children group (type 6) [documented][observed].
- **Persistent cell** [observed]: every worldspace that has references also has exactly
  one CELL placed directly in the world-children group, outside any block. It has no
  EDID, XCLC grid (0, 0) and no LAND, and it holds the worldspace's persistent
  references (WastelandNV: 6,093). It is not the real cell (0, 0), which also exists.
  Two worldspaces without references (FXInvertedDaylightWorld, NVDLC01TestMap01) have
  no persistent cell.
- **Grid** [documented][observed]: XCLC gives the exterior cell's (x, y). Cells are
  4096 units square: cell (x, y) covers `[4096x, 4096(x+1)) × [4096y, 4096(y+1))`.
  Evidence: of the temporary references in WastelandNV's exterior cells, 142,783 lie in
  their cell's square and 107 (0.07%) do not; the pattern holds in every worldspace.
  No two cells of a worldspace share a grid square [observed].
- **Interior cells** sit in the top-level CELL group in interior block / sub-block groups
  (types 2 / 3) and have DATA bit 0 set [documented][observed: 492 interior cells across
  the load order, all with the flag].
- **Water height (XCLW)** [observed]: 15,963 of 16,397 WastelandNV cells store
  3.4028235e38 (the largest f32), which reads as "no cell-specific water height"
  [hypothesis: the worldspace default applies]. Interior cells without water store
  -2147483648.0. Real values (e.g. 2600.0 in 412 cells) are world-space Z.
- **Overrides** [observed]: DLC masters repeat a cell's FormID to override it, and place
  their own references into base-game cells by labelling a cell-children group with the
  base-game cell's FormID (mapped through their master list). Index result: no
  children group in the load order names a cell that does not exist.
- Inventory of worldspaces (`fnvtool world list`): 33, of which 6 are complete 64 × 64
  grids (4,096 cells, x and y −32..31: TheStripWorldNew, WastelandNVmini, Lucky38World
  and the Zion, Big MT and Divide landscapes). WastelandNV has 16,396 grid cells spanning
  x −65..64, y −65..271, with 16,388 LAND records [observed].

### 1.1 Child worldspaces: data taken from the parent (WRLD WNAM, PNAM)

Issue #2345. Code: `fnv_world::WorldIndex::take_parent_data`, `WorldIndex::lod_world`.

- **The flags** [documented: xEdit wbDefinitionsFNV.pas WRLD; names only]: PNAM bit 0 Use
  Land Data, 1 Use LOD Data, 2 Use Map Data, 3 Use Water Data, 4 Use Climate Data, 5 Use
  Image Space Data.
- **The ten New Vegas masters** [observed: `fnvtool world list`]: every child sets Use
  Map Data only and has its own land, `DNAM`, climate, water and image space.
- **Fallout 3's children in a Tale of Two Wastelands load order** [observed: `fnvtool
  world list`, `world heights`, `esm decode` on Fallout3.esm, 2026-11-18], all children
  of Wasteland:

  | Child | PNAM | Own records |
  |---|---|---|
  | MegatonWorld | land, map | no `DNAM`; own climate, water, image space; LAND on 12 of 16 cells |
  | Oasis | land, map | no `DNAM`; own climate, water, image space; no LAND on any of 34 cells |
  | GNRroofWorld | land, LOD, map | no `DNAM`; own climate, water, image space; LAND on 85 of 143 cells |
  | ParadiseFalls | land, map, water, climate, image space | no `DNAM`, `CNAM`, `NAM2`, `INAM`; LAND on 2 of 12 cells |
  | MonumentWorld, StatesmanRoofWorld, TestQAWorld | LOD, map | own `DNAM` (land 12288, water 0), climate, water, image space |

  So a child with a flag leaves the matching subrecords out, and a child without it has
  them. The land-sharing children lie on their parent's grid squares: MegatonWorld's own
  LAND has the same heights as the parent's cell on the same square on 10 of its 12
  squares (2 differ); ParadiseFalls's 2 and GNRroofWorld's 85 all differ from the
  parent's (GNRroofWorld's are flat at about 12,280, over ground at about 10,700).
- **As built** [hypothesis on the observations above]:
  - Use Land Data: the default land and water heights (`DNAM` holds both) are the
    parent's when the child has none; every cell takes the LAND of the parent's cell on
    the same grid square, its own LAND too, and keeps its own only where the parent has
    none (#3755). Drawn, ParadiseFalls's own LAND on (-9, 16) (11,704 to 13,536 against
    the parent's 11,504 to 11,984) hung a strip of ground in the sky behind the
    entrance, with steep stretched faces down to the parent's cells, which players of
    the original TTW do not report [observed: NVForever, TTW data, 2026-11-25]; and
    GNRroofWorld's own LAND, flat at about 12,280 over ground at about 10,700, would be
    a floor at roof height over the plaza. So the parent's wins [hypothesis; observer
    check 225 asks the original].
  - Use Water Data: the water type (`NAM2`) and the default heights are the parent's
    when the child has none. Per-cell water (`XCLW`, `XCWT`) is not taken from the
    parent's cells [not known].
  - Use Climate Data: the climate is the parent's. Use Image Space Data: the renderer
    already reads the parent's `INAM` when the child has none.
  - Use LOD Data: the distant LOD files read are the parent's (`lodsettings\<parent>`,
    `meshes\landscape\lod\<parent>`).
  - Use Map Data: the Pip-Boy's map (pipboy data, `read.rs`).
- **Seen in the game** [observed, TTW data, 2026-11-18]: started in Oasis at
  (-10240, 116736) the player stands on terrain at z 17,938 (the parent's LAND for
  square (-3, 28)); before, there was no floor and the player fell without end (#2345's
  quest-pilot report). MegatonWorld loads with the parent's land on the 4 squares that
  had none.

## 2. References

- References are records in persistent (type 8) and temporary (type 9) cell-children
  groups: REFR (objects), ACHR (NPCs), ACRE (creatures), PGRE (placed grenades); NAVM
  (navmesh) and LAND also live there and are not references [documented][observed].
- A later plugin can override a reference, including moving it to another cell's group;
  the index takes the cell of the winning definition [documented: last definition
  wins, esm.md].
- **Placement** [documented]: `NAME` = base object FormID; `DATA` = position (3 × f32)
  then rotation (3 × f32, radians); `XSCL` = uniform scale (default 1); `XESP` = enable
  parent (reference, flags; bit 0 = opposite state). Header flags: 0x20 deleted, 0x400
  persistent, 0x800 initially disabled, 0x8000 visible when distant.
- **Base objects** referenced from WastelandNV [observed]: STAT 114,443 (112,329 with a
  model), TREE 18,050, MSTT 3,235, MISC 2,225, ACTI 2,135, CREA 1,956, NPC_ 1,253, FURN
  1,108, CONT 1,032, SCOL 954, IDLM 590 (no model), DOOR 484, SOUN 410 (no model),
  LIGH 361 (115 with a model), plus small counts of items. The model path (`MODL`) is
  relative to `meshes\`, except TREE models: SpeedTree `.spt` files named without a
  folder, which live in `trees\` [observed: all ten `.spt` files in the archives are
  there, e.g. `trees\wastelandshrub01.spt`; WastelandNV has 18,050 TREE references].
  `.spt` is SpeedTree's own format, not NIF (speedtree.md). Each `.spt` has a
  billboard texture with the same stem, `textures\trees\billboards\<stem>.dds` (square,
  256 or 512 px, DXT3/DXT5) [observed], and TREE `BNAM` gives the billboard width and
  height in game units (WastelandShrub01: 175 × 175) [documented: xEdit; observed].
- **Imposters**: a reference with bit 1 (0x2) of `XSRF` "Special Rendering Flags" set
  is an imposter [documented: xEdit wbDefinitionsFNV.pas REFR]; it "is displayed always
  when the player is in the region" whose imposter list names it [documented: GECK wiki
  "Reference", owner's export]. The list is the region's `RDID`, a list of REFR FormIDs
  after an `RDAT` of type 8 [documented: xEdit wbRegionDataEntries type 8; observed: 18
  REGN records in FalloutNV.esm have one]. The other `XSRF` bits vary with no pattern
  and `XSRD` is mostly 0 or 4 [observed: the 301 references with `XSRF`]. Outside every
  region that lists it an imposter is not drawn, also when its cell is loaded
  [hypothesis: WastelandNV's imposters (`architecture\strip\imposter\*.nif`, flat
  stand-ins for the Strip walls, hotels and the South Vegas buildings, persistent)
  stand on the full buildings they replace, e.g. Bldg03Imposter02 0015EFBC on the BLD11
  pieces south-east of the Poseidon gas station; StripImposter, StripImposterBlackMountain
  and StripImposterJacobstown, far vantage points, list them, while StripImposterClose,
  which covers the ground near Vegas, lists only near versions of the Strip's light
  glows. Drawn there, they hid the ruins behind flat blocks: owner's screenshot,
  2026-10-11]. The renderer takes the camera's position for the player's
  (`engine/nvforever/src/imposters.rs`).
- **Engine-defined base objects** [observed]: 102 WastelandNV references name a base
  FormID below 0x800 that no master defines (e.g. 0x17 on references carrying portal /
  occlusion box data in XPRM/XOCP). These are built into the engine [hypothesis] and are
  invisible markers; the renderer skips them.

## 3. Landscape (LAND)

One LAND per exterior cell, in the cell's children (type 9) group [documented][observed].
All grids are 33 × 33 vertices covering the cell, so neighbouring vertices are 128 units
apart and the last row/column coincides with the neighbour's first.

- **Heights (VHGT)** [documented; layout observed]: an f32 offset, 33 × 33 signed byte
  steps, 3 unused bytes. Integrate row by row: the first value of each row is a step from
  the first value of the previous row (the first row starts from the offset); every other
  value is a step from its western neighbour. Multiply by 8 for world units.
  Rows run south → north (y), columns west → east (x).
  Evidence: with this reading, neighbouring cells' shared edges are **identical** for
  16,233 of 16,257 east–west and 16,242 of 16,258 north–south pairs in WastelandNV (and
  for every pair in most other worldspaces). Reading rows as x instead matches 1 pair.
  The mismatching pairs lie mostly outside the playable map area (e.g. y = −55..−64 and
  y = 63, where the map spans x −35..25, y −25..35 per WRLD MNAM); the few inside differ
  by one step (8 units).
- **Normals (VNML)** [observed]: 33 × 33 × (x, y, z) **signed** bytes, same layout as the
  heights (the xEdit layout names them as unsigned). Against normals computed from the
  heights by central differences, the mean angle is 1.06° read as signed, 59.9° as
  unsigned, 23.0° with x and y swapped (288,300 vertices sampled).
- **Vertex colours (VCLR)**: 33 × 33 × RGB bytes, same layout [documented].
- **Texture layers** [documented; details to be confirmed by rendering]: the cell is
  split into four quadrants (0 bottom-left / south-west, 1 bottom-right, 2 top-left,
  3 top-right). `BTXT` gives a quadrant's base texture (LTEX FormID; null = worldspace
  default). Each `ATXT` adds a layer (LTEX, quadrant, layer number) whose opacity per
  vertex is given by the following `VTXT`: a list of (position, opacity f32) where
  position indexes a 17 × 17 grid over the quadrant. LTEX names a texture set (TXST).
  In the engine each layer is a blended mesh over the quadrant's opaque base, drawn in
  layer order and before every other blended mesh of the world (after the sky)
  [engine rule, `nvf_assets::nif_material::sort_first`]: a blended mesh has no depth,
  so a layer drawn after one would paint the ground behind it over it (seen on the
  Goodsprings corn rows, docs/spec-notes/nif.md §6 "Alpha").
- **Landscape layers: the diffuse's alpha** (#3789). A layer covers by its painted opacity
  alone; its diffuse texture's alpha has no part in it. [observed: FalloutNV.exe 1.4.0.525,
  native 1920 x 1080 pictures of its window, Goodsprings (-71552, 2892) heading 0 at noon,
  pitch 25 and 50 down, the lower half, mean absolute difference of 255] The 61 DXT1 files of
  `textures\landscape` in `Fallout - Textures.bsa` and `Fallout - Textures2.bsa` that are no
  normal map or LOD, as DXT5 with the same colour blocks and a 40/255 checkerboard of
  16 texels in the alpha draw as without it: 0.002 and 0.013, where two runs with the game's
  own differ by 0.002 and 0.002; solid magenta in their place (the control) differs by 66.6
  and 91.1. The test was jojokengly's first [observed: jojokengly, #3486,
  FalloutNV.exe 1.4.0.525, unmodified shader packages, Goodsprings (-71552, 2892) at noon, the
  game's 512 x 288 save-header picture, mean absolute difference of the ground half] The
  game's own ground textures with a 40/255 checkerboard put in their alpha draw as without it
  (0.11 of 255, plants moving), and Sandstone Desert 3.00 as shipped (DXT5, alpha about 0.41)
  as the same colours in opaque DXT1 (0.05); Sandstone Desert against the game's own is 14.4.
  All 69 landscape diffuses the base game ships that FalloutNV.esm's LTEX records name are
  opaque DXT1 [observed: jojokengly, the same comment], so only texture packs show the
  difference: several keep a height map in that alpha. Until #3789 `nvforever` multiplied the
  opacity by it. Now none of its landscape draws read it: the folded layers with or without
  height blending, anti-tiling and parallax, the layers drawn on their own, the mesh terrain
  blend's copies of both, and the generated LOD (which never did). Road strips and other NIF
  shapes keep their alpha. Smoke: `terrain-layer-alpha`, `terrain-layer-alpha-parallax` and
  `terrain-layer-alpha-separate`.
- **Alpha grid orientation** [observed]: VTXT position = row × 17 + column, rows south →
  north, columns west → east. Neighbouring quadrants share an edge of the grid; for a
  texture layered on both sides, the edge opacities agree (within 1/255) for 38,039 of
  44,753 edges inside cells and 35,100 of 43,039 across cells in WastelandNV. The best
  of the other seven orientations of the square agrees for 21,946 and 20,033.
  Reproduce with `crates/fnv-world/examples/land_layers.rs`. Those counts leave out
  edges where a texture is layered on one side only: at (−55,420, −65,575) the south
  cell (−14, −17) paints LTEX 000357BA along its north edge at up to 1.00 and cell
  (−14, −16) has no layer of it, which drew as a straight texture seam (issue #2089,
  owner's save 36) [observed]. **Stitching** [design; what the original does at such
  edges is not known]: the renderer reads the four neighbouring cells' layers with a
  cell's own and, at every vertex of a shared quadrant edge, gives each texture layer
  the larger of the two sides' opacities; a quadrant lacking a texture its neighbour
  paints on the edge gets a layer of it over its own, painted on the edge row only (it
  fades out across the next grid square). A texture that is the quadrant's own base is
  not added; bases are not changed; diagonal corners are not looked at. Only what is
  drawn changes: grass and footsteps read the record's layers
  (`nvforever` terrain.rs `stitch_layers`).
- **Texture repeat** [observed, 2026-10-10]: landscape textures repeat every **256 world
  units** (16 times across a cell, once per two grid squares), with u = x / 256 and
  v = y / 256 in world coordinates: v grows **northwards**, so the image's top edge lies
  to the south and the texture is mirrored when seen from above. Method: for the
  reference frames `goodsprings_hill_lod` and `goodsprings_noon` (camera.md §2), each
  ground pixel's world position was found by intersecting its view ray with the LAND
  heights, the cell's base texture and layers (blended by VTXT opacity) were sampled
  there for a candidate repeat and direction, and the result was correlated with the
  frame (luminance, high-passed at 6 pixels, bare ground only, best shift within ±24
  pixels). Repeats of 4096 / n for n = 2..48 in both v directions score 0.01–0.04, except
  n = 16 with v northwards: 0.147 (hill) and 0.124 (noon); n = 15.75 and 16.25 fall back
  to 0.02. `nvforever`'s own render of the hill camera scores 0.051 with the earlier
  guess (4096 / 6 units, v southwards) and 0.153 with this rule, at the same shift as the
  simulation (6, −10 pixels: camera registration, not resolved further). The textures
  are 512 × 512 (all ten LTEX of cells (−18, 0), (−17, 0) and (−16, −1)), so the ground
  carries 2 texels per unit; the earlier guess stretched them to 0.75.
  `Fallout_default.ini` has `fLandTextureTilingMult=2.0000` under `[Landscape]`
  [documented]; how it enters the repeat is not known (256 = 2 × the 128-unit vertex
  spacing fits [hypothesis]).
- **Landscape normal maps**: an LTEX's texture set names a normal map in TX01
  [documented], 512 × 512 for the ten LTEX above: six DXT5 or DXT3 with a sparse alpha
  (mean 0.01–0.11) and three DXT1 (alpha reads 1) [observed]. Each LTEX also stores a
  "texture specular exponent" (SNAM, 30 on all ten) [documented: schema; observed]. The
  reference frames show per-pixel relief on the ground at noon (pebbles and cracks with
  a lit and a shadowed side) [observed]. `nvforever` lights each base texture and layer
  with its normal map read as a tangent-space map in the frame of the texture
  coordinates (red towards +u = east, green towards +v = north, the DirectX-style green
  of nif.md §6) [hypothesis]. Support: at the hill camera the change the normal maps make
  to the render correlates with the reference frame at 0.16–0.22 on the near ground
  (noise 0.03), i.e. the bumps are lit from the same side; it does not separate the
  frame convention from the light's direction. The relief in the game is stronger than
  in the render at noon (high-pass contrast of the same ground: game 0.070, render 0.058
  with normal maps, 0.056 without, 0.048 before the repeat was corrected). Open: whether
  the normal map's alpha is a specular mask on terrain and how the exponent is used (the
  renderer draws terrain with a constant low specular); the game's light direction on
  terrain at a given hour.
- **Grass**: landscape textures name the grass types that grow on them (LTEX `GNAM`);
  see grass.md.
- **Default texture** [documented: the game's `Fallout_default.ini`,
  `SDefaultLandDiffuseTexture=DirtWasteland01.dds`]: 51,000 of 65,552 WastelandNV
  quadrants have no BTXT (most of them outside the map; cell (−14, −6) south of
  Goodsprings has one), and some ATXT layers name a null LTEX [observed]. The renderer
  uses `textures\landscape\dirtwasteland01.dds` for both. It is the only file of that
  name in the archives, so it is probably the INI's file [hypothesis]; its normal map is
  `SDefaultLandNormalTexture=DirtWasteland01_N.dds` [documented]. Before this change
  those quadrants were drawn untextured, which showed as hard-edged patches along quadrant
  edges (owner's screenshot, 2026-10-08) [observed].
- **Smoothing** [design, #2149: NVForever's own choice, not the original's behaviour]. The
  grid is coarse (128 units), heights come in steps of 8 and every square is split along
  the same south-west → north-east diagonal (a [hypothesis] about the original), so ridges
  zig-zag and gentle slopes show steps; the VNML lighting is already smooth, the facets
  are in the geometry. The maintainer decided on 2026-11-13: "terrain smoothing should be
  the default have the collision follow the smoothed surface". The default recipe,
  **detail, 4 × 4, bound 8, thresholds 2:6**, is the contributor's pick from nine variants
  compared in game and screenshots on 2026-11-15, for the maintainer to confirm
  [design, #2149]. Interpolate keeps the 8-unit VHGT steps; fit flattens
  real features; detail removes the steps while keeping bumps, crests and ridges.
  The 4 × 4 subdivision gives rounder silhouettes, and bound 8 keeps placed objects
  in contact by limiting the distance from the stored ground to one height step.
  The rule
  (`crates/fnv-world/src/land_surface.rs`, one surface for drawing, collision and every
  height query; the run's recipe is a `SurfaceSpec` fixed at the start and carried by
  every job that builds a surface):
  - each grid square is cut into `subdiv` × `subdiv` squares (4 by default, or 2), split
    along the same diagonal, so every new triangle lies inside one stored triangle;
  - the new vertices lie on a Catmull-Rom bicubic patch over control heights at the LAND
    vertices. Interpolating the controls are the stored heights, so every
    LAND vertex keeps its stored height exactly. Fitting, each control is the stored
    height H moved towards the binomial mean A of its 3 × 3, (1 2 1) × (1 2 1) / 16, by
    at most K = min(4, bound): F = H + clamp(A − H, −K, K), every F from stored heights
    only, rows then columns in one fixed order; a plane stays a plane and the staircase
    `0, 0, 8, 8, 16, 16` becomes the even slope `…, 2, 6, 10, 14, …` (each knot moved
    2 units); at a LAND vertex the surface is F. With detail (the default, a feature-preserving fit)
    only the part of the move that looks like a quantisation step is taken: with
    d = A − H, F = H + clamp(d · (1 − smoothstep(t0, t1, |d|)), −K, K), t0 = 2 and t1 = 6
    units by default [design, #2149: in the contributor's comparisons fit 4 × 4 within 8 was
    the best compromise but "overly smooths things"; an 8-unit step on a slope moves a vertex by
    at most 2 (the staircase above gives ±2), while a lone vertex 16 above flat
    neighbours moves by 12 and a ridge 16 high by 8, so steps even out as with fit and
    bumps, crests, ridges and dips keep their height];
  - the patch's distance from the *stored* triangle is clamped to ± the bound (8 units by
    default, one stored step; 0 to 16 with the test aid): cliffs and sharp edges keep
    their shape, placed objects neither float nor sink by more than that. The bound is
    the whole of it, fitted or not (K ≤ bound);
  - along a cell edge the patch reads the neighbouring cells' LAND (all eight, so the
    corners too): a vertex outside the cell comes from the cell that owns it, else the
    other one that has it; where no loaded cell has it, it carries on the nearest lines
    of present vertices (for the least step k of 1 up to the padding, 2 interpolating and
    3 fitting, the mean over the eight grid directions of 2·a − b from the vertices k and
    2k away, never an extrapolated one), a rule of the world vertex alone, so every cell
    that reads it gets the same value; each new vertex is worked out in one fixed square,
    so both cells of an edge compute it from the same numbers and their edges are
    identical (heights and normals). Everything an edge's vertices read lies within 9
    LAND vertices of the edge (2 for the patch and the normals' margin, 1 for the fit's
    3 × 3, 6 for extrapolation), inside the cells both sides read; this assumes the two
    LAND records of a shared edge agree on its heights, as the stored triangles do;
  - the normals are central differences of the final (clamped) heights (one vertex of
    margin beyond the cell); the vertex colours and the texture layers' opacities are
    interpolated on the stored triangles, so the paint lies where it did; a fitted (or
    detail) surface keeps the stored heights (4.3 KiB a cell) so the lift is measured from the
    stored triangles;
  - collision (`fnv-phys` terrain, `collider_surface`), `PhysState::terrain_z`, grass,
    the feet and detection probes use the same surface, so actors and objects stand on
    what is drawn.
  The setting `smooth-terrain` (Graphics) takes effect at the next start and is a list
  [design, #2149, #2676]: **Off** (the stored triangles drawn and collided exactly as
  before; `--no-smooth-terrain`), **2 x 2** and **4 x 4** (each grid square cut 2 × 2 or
  4 × 4 everywhere, drawn and collided at that level; 4 × 4 is the default, smoothing on
  by the maintainer's decision of 2026-11-13 and 4 × 4 the contributor's pick of 2026-11-15
  for the maintainer to confirm; 2 × 2 is a
  quarter of the geometry, 4,225 vertices and 8,192 triangles a cell against 16,641 and
  32,768), and **4 x 4 near**; `--smooth-terrain off|2x2|4x4|4x4-near`, kept in the INI
  in those words; the INI's older `1`/`0` reads as 4 x 4 and Off. Whether 4 x 4 near
  becomes the default is the maintainer's call. 4 x 4 near [design, #2676; NVForever's
  own rule, `engine/nvforever/src/terrain_lod.rs`]:
  - *the same surface at both levels*: a 2 × 2 surface's vertices are exactly every other
    vertex of the 4 × 4 surface of the same recipe (a new vertex is worked out from its
    place in its stored square alone, and 2/4 is 1/2), so a quadrant drawn 2 × 2 is the
    4 × 4 surface's even vertices with their 4 × 4 normals
    (`fnv_world::land_surface::QuadrantDraw`; tested bit for bit);
  - *the rule*: each loaded quadrant is drawn 4 × 4 once the camera comes within 2,048
    units of its square seen from above (0 over it; one quadrant's side), and 2 × 2 again
    only beyond 2,560 (512 of hysteresis), keeping its level in between so it does not
    flicker at the boundary; at that distance the two levels, about a unit apart in
    height, differ by less than a pixel at 1080p (a unit spans about 0.7 pixels at 1,500
    units with a 75° view). The distances are to be tuned from pictures
    (`nvf_play::smooth_terrain::FINE_NEAR`, `FINE_HYSTERESIS`; test aid
    `NVF_PLAY_TEST_SMOOTH_NEAR=<units>`). A cell's quadrants are first drawn 2 × 2 (new
    cells load at the edge of the loaded grid);
  - *no cracks*: a 4 × 4 quadrant's side next to a quadrant drawn 2 × 2 (or one not
    loaded, which loads as 2 × 2) is drawn coarse: each vertex between two of the 2 × 2
    side's vertices is put halfway between them, with the normalised sum of their normals,
    so the side lies on the 2 × 2 side's straight segments with no gap (the T-junctions stay,
    collinear); the normals agree at the shared vertices and differ slightly between them
    (the moved vertex's normal is normalised, the 2 × 2 side interpolates linearly);
    colours and layer opacities need no change (the stored
    triangles are linear along a quadrant's side). A level change also redraws the 4 × 4
    neighbours whose sides depend on it; cell edges stay identical from both sides;
  - *redraws* run one batch at a time on the read pool's worker threads, from the
    landscape, surface and layers the cell was spawned with (no record read again); a
    built batch is spawned hidden, with the visibility range of the cell's fade as it is
    that frame (after the fade's own update), else the fully drawn range, and the old
    meshes stay until the render world reports every new mesh uploaded and given its
    buffer slabs (the shared GPU upload limit can hold meshes back for frames). After 120
    frames without that word the batch is given up: the old meshes stay, the new are
    dropped, and redraws pause (120 frames, twice as long after each batch given up in a
    row, at most 3,600; the first is warned, all counted in the exit summary). At the swap
    the batch is checked against the cells loaded then: a quadrant whose sides would no
    longer fit its neighbours' drawing (a cell unloaded or loaded again while it was
    built) keeps its old meshes, until every swapped one fits; the rest swap in one
    frame, so neighbours change together. Retired cells are forgotten at once and their
    quadrants leave a waiting batch. The readiness covers the meshes' upload, not their
    materials or pipelines (reused from the old meshes in the usual case);
  - *collision* keeps one surface per loaded cell at the finest chosen level: 2 × 2 under
    2 x 2, 4 × 4 under 4 x 4 and 4 x 4 near. Actors near the player stand on what is drawn
    there once their quadrant is redrawn 4 × 4 (at the start, after a teleport and while a
    batch waits, the drawn ground there is briefly 2 × 2, within about a unit); at a distance the drawn 2 × 2 ground is within about a unit of the
    collision, below what an actor's feet show from that far, and the collision never
    changes while cells are loaded (no rebuilt colliders, nothing re-grounded when the
    camera moves) [design, #2676];
  - the terrain wireframe shows each quadrant as drawn, and the start line names the rule
    (`…; drawn 4 x 4 within 2048 units of the camera, 2 x 2 beyond 2560 (4 x 4 near)`); the
    redraws log `terrain levels: …` (the first, then at most every 10 s) and the exit's
    `terrain summary` adds their totals; test aid `NVF_PLAY_TEST_TERRAIN_LEVELS=1` logs
    `terrain levels: settled: …` each time the levels settle after redraws.
  Test aids choose another recipe for comparing them, without changing the
  setting or the INI [design, #2149]:
  `NVF_PLAY_TEST_SMOOTH_MODE=interpolate|fit|detail`, `NVF_PLAY_TEST_SMOOTH_SUBDIV=2|4`
  (over the row's level; 2 with 4 x 4 near is 2 × 2 everywhere),
  `NVF_PLAY_TEST_SMOOTH_BOUND=<0..16>`, `NVF_PLAY_TEST_SMOOTH_DETAIL=<t0>:<t1>` (detail's
  thresholds, 0 ≤ t0 < t1 ≤ 16; detail only) (a bad value warns once and keeps that
  field's default; with smoothing off they do nothing). Detail logs its thresholds:
  `mode=detail(2:6)`. The start logs one line, by default `terrain:
  mode=detail(2:6) subdiv=4 bound=8 land-shift=4; drawing, collision and queries use this
  surface` (off: `mode=stored subdiv=1 bound=0 land-shift=0`), and the exit's `terrain
  summary` counts cell draws over the run (a cell drawn again counts again) and names the
  recipe. Cost of the default [observed: 2026-11-15, 1920x1080 unpaced, RTX 5070 Ti, three
  launches each on a Goodsprings hillside and two fast flights (1500 units/s) over
  Goodsprings]: standing frame time 3.78 ms (2 x 2 interpolate 3.74, off 3.78), the
  flight's worst frame 32.9 ms (2 x 2 32.4), peak working set about 255 MB above 2 x 2
  (3,038 against 2,783 MB) and video memory about 130 MB above it; the costs of 2 x 2
  and 4 x 4 near against it are still to be measured (#2676). The Pip-Boy local map's outline reads the same surface beyond the loaded
  collision, and so do the console's CenterOnWorld and CenterOnExterior (the cell's
  centre on the run's surface; `coc` uses arrival markers). An actor standing without a physics character at
  an authored or navmesh height over the bare landscape (within 32 units of the stored
  triangles, the landscape the first collision under it, not walking up to, on or
  getting off furniture) is drawn as far above the smoothed surface as it stood above
  the stored triangles (`nvf_play::phys` `landscape_lift`); one whose feet rest on the
  collision is drawn on it as before, and feet a physics character left are drawn as the
  character was (never lifted twice). The answer is kept per actor and looked at again
  when its feet change or collision loads or unloads next to it. Known limit [design,
  #2149]: that "left by a character" record is not kept across a reset (a door to an
  interior and back) or a save and load, so an actor that stopped on displaced, sloping
  ground and has not moved since loses the character's drawing offset and is lifted again:
  it can then be drawn off the drawn ground by the lift (at most the bound, 8 units by
  default) plus the
  controller's slope clearance it stood with (for example 5 + 8 = 13 units) until it next
  walks. Keeping walker feet in stored-ground space instead was tried
  and dropped: the simulation's positions feed script moves, script shots and raycasts
  and ragdoll start-up as world coordinates. Distant LOD is not touched: its chunks are sunk
  under the loaded cells with skirts (§7), and the loaded edge moves by at most the bound
  (between the LAND vertices; fitting, the LAND vertices themselves by at most K). With
  the previous interpolate/2/bound-8 recipe, on Goodsprings' cells (−17, 0) and (−18, 0), 1,084 and
  1,541 of the 3,136 new vertices move by more than half a unit, the 95th percentile by
  3.3 and 4.6 units, and 19 and 43 reach the bound [observed: `fnvtool world land`
  heights of the nine cells around each (FalloutNV.esm, WastelandNV), run through a
  Python port of the rule above on 2026-11-14; a calculation, not a game run]: that is
  why a larger bound alone changes little there, and why the fit moves the controls.
  Cost per cell [design, #2149: arithmetic from the grid sizes; frame cost and memory to
  be measured]:

  | | 2 × 2 | 4 × 4 |
  |---|---:|---:|
  | Vertex spacing | 64 units | 32 units |
  | Surface and collision vertices | 4,225 | 16,641 |
  | Collision triangles | 8,192 | 32,768 |
  | Vertices per quadrant mesh | 1,089 | 4,225 |
  | Heights and normals | 66.0 KiB | 260.0 KiB |
  | Collision position and index arrays | 145.5 KiB | 579.0 KiB |
  | Base render geometry (64-byte vertices, u32 indices) | 368.3 KiB | 1,440.3 KiB |

  Blended layers and folded opacities add to the render figure; Rapier's own structures
  are not counted. Nine LAND height reads per cell instead of one either way.
  **Terrain wireframe** [design, #2149]: the Debug row `terrain-wireframe` (live, off by
  default; `--terrain-wireframe`, engine/nvforever/src/terrain_wireframe.rs) draws every
  triangle edge of the loaded, shown cells' surfaces once (rows, columns, diagonals as
  line strips; not the texture layers, not distant LOD) as magenta 1-pixel gizmo lines,
  depth-tested with a small bias. On switching on, a read-pool job per cell builds the
  surface again from the LAND records with the run's recipe; cells loaded or shown while
  on get one, cells unloaded or hidden lose theirs; off, nothing is built, kept, uploaded
  or drawn (12,416 edges a cell at 2 × 2, 49,408 at 4 × 4, 3,136 stored).

## 4. Reference rotation convention

Matrices below rotate column vectors (the NIF convention, nif.md §6); `Rx(a)` etc. are
counter-clockwise rotations by `a` about the axis.

**The world rotation of a reference is `Rx(−x) · Ry(−y) · Rz(−z)`** for stored angles
(x, y, z) [observed]. Equivalently: clockwise rotations about the fixed world axes, z
first, then y, then x. Evidence: a static collection (SCOL) lists its parts as
(STAT, position, rotation, scale) and also ships a combined mesh with one node per part
at the part's position. Of 1,590 parts matched to a node, 308 have x or y rotation (the
only ones that can tell orders apart). This convention reproduces the node's rotation
within 1e-3 for 286 of them; the best alternative fits 85. Of all 45 misfits, 41 belong
to six collections (0x001680B1–B6) whose meshes probably predate edits to the record
[hypothesis]. Single-axis checks fix the sign: for z-only rotations only the clockwise
reading fits. Reproduce with `crates/fnv-world/examples/scol_rotation.rs`. The owner
confirmed in the renderer that placements and terrain heights look as in the original
game (visual check, 2026-10-07) [observed].

A reference's world transform is then translation = position, rotation as above, uniform
scale = XSCL. **It replaces the transform of the model's root node**; the root's own
translation, rotation and scale are not applied, while every node below the root composes
as usual [observed]. 442 models placed by 12,687 references (9,517 exterior, 3,170 interior)
have a non-identity root, almost always a rotation about z (modular kit pieces such as the
`nv_craftsmanhomesinterior` walls, sidewalk and retaining-wall corners). Evidence: modular
pieces are built so that the open (boundary) edges of neighbouring pieces coincide.
Placing every reference of every cell that holds a rotated-root model both ways and counting
open edges shared by two references (one of them rotated-root): exterior cells 30,223 with
the root replaced against 10,655 with it composed (250 cells favour replaced, 11 composed),
interior cells 17,749 against 4,616 (82 against 1). GSDocMitchellHouse alone: 374 against
40; with the root composed its walls stand on the wrong sides of their floor squares, and
with it replaced the layout matches the owner's GECK screenshot (2026-10-07). Caveat:
static-collection combined meshes do compose the root (all 44 SCOL parts whose model has a
rotated root carry placement ∘ root on their node); the collection mesh is the drawn
model, so this does not change how a SCOL reference is drawn [hypothesis: the combined
meshes were baked by a tool that composed the root]. Reproduce with
`crates/fnv-world/examples/root_transform.rs`. Geometry drawn without a reference (LOD
blocks and chunks) keeps the root transform [hypothesis: their roots are identity in
practice].

## 5. Field layouts used by the world commands

All little-endian; text is Windows-1252, NUL-terminated (esm.md). FormIDs inside a record
are local to the plugin that holds the record and are mapped to the load order through
that plugin's master list (esm.md). Layouts are [documented: xEdit] and decode exactly
for every occurrence in the ten masters [observed: `fnvtool esm coverage`]. A subrecord
whose size does not match its layout is treated as absent.

| Record | Subrecord | Layout |
|---|---|---|
| any | `EDID` | Editor ID, zstring |
| CELL, WRLD | `FULL` | display name, zstring |
| CELL | `DATA` | u8 flags; bit 0 = interior (size 1) |
| CELL | `XCLC` | i32 x, i32 y, then optionally u8 land flags + 3 unused bytes (size 8 or 12) |
| CELL | `XCLW` | f32 water height (size 4) |
| REFR, ACHR, ACRE, PGRE | `NAME` | u32 base FormID (size 4) |
| REFR, ACHR, ACRE, PGRE | `DATA` | f32 position x, y, z, f32 rotation x, y, z in radians (size 24) |
| any reference | `XSCL` | f32 scale (size 4) |
| any reference | `XESP` | u32 enable-parent FormID, u8 flags (bit 0 opposite), 3 unused (size 8) |
| base objects | `MODL` | model path, zstring |
| LAND | `VHGT` | f32 offset, 33 × 33 i8 steps (row by row), 3 unused (size 1096) |

When a record has several subrecords with the same signature, the first one counts.

## 6. Climate, weather and water

Layouts: `schemas/fnv/sky.json` [documented: xEdit], checked with `fnvtool esm coverage`
on the ten masters: every CLMT, WTHR and WATR subrecord decodes exactly, except the
six per-time-of-day image-space adapter links in WTHR (signatures starting with a 0x00..0x05
byte, not needed yet) and 8 WATR records whose DATA holds an older 186-byte layout
(visual data plus damage) [observed].

- **Worldspace → climate and water**: WRLD `CNAM` (climate) and `NAM2` (water type);
  child worldspaces may take them from their parent (PNAM flags) [documented]. The
  Mojave uses NVDefaultClimate [observed].
- **Child worldspaces** [observed: `fnvtool world list`, `fnvtool esm decode`, the ten
  masters]: 17 WRLD name a parent (`WNAM`: 12 the Mojave, 1 Big MT, 4 NVDLC04DivideVistaWorld).
  Every one sets `PNAM` = 4, "Use Map Data" only, and carries its own `CNAM`, `NAM2`,
  `INAM` and `DNAM`; the Freeside worldspaces use NVUrbanClearClimate, TheStripWorldNew
  NVtheStripClimate, NVDLC03Higgs NVDefaultClimate. NVForever reads each worldspace's
  own records (renderer and physics); taking land, water, climate or image space from
  the parent by the other PNAM bits is not implemented, since no shipped worldspace sets
  them (`nvf-play` logs a warning when one does).
- **Climate (CLMT)**: `WLST` = list of (weather, chance i32, global variable) [documented];
  `FNAM`/`GNAM` sun and glare textures; `TNAM` = sunrise begin/end, sunset begin/end,
  volatility, moons, with times in 10-minute units (value / 6 = hours) [documented]. The
  Mojave: sunrise 6:00-8:00, sunset 18:00-20:00 [observed].
- **Choosing a weather** (`fnv_world::sky::{eligible, roll, roll_due, region_weathers,
  region_at}`, engine `sky.rs`; built 2026-10-21, ESM gap audit row 8). What the data
  holds [observed, FalloutNV.esm]: every climate's TNAM volatility is 0. NVDefaultClimate
  (and the worldless NewVegasRegion) lists NVWastelandClearNight (chance 100, gated by the
  global `VNight`, 0 in the masters, set to 1 from 21:00 until GetCurrentTime 4.30, 4:18, by NightQuestScript) and
  NVWastelandClear (chance 100, no global). 31 regions carry a weather block (REGN `RDAT`
  type 3 then `RDWT`, laid out as `WLST` [documented: xEdit]); in WastelandNV the RDAT
  flags' bit 0 is set on NVUrbanRegion (NVUrbanClear 85, NVWastelandClear 15),
  NVSearchlightRegion, NVJacobstownRegion, NVBlackMountainRegion, NVLegateRegion,
  MapHooverDam (NVHooverDamWeather 100, and NVHooverFinalBattle 100 gated by
  VStoryEventHooverDamBattleStarted) and VMapTheFortRegion, all priority 100, and clear on
  GSWeatherRegion (NVWastelandGS, made beside the test climate NVDefaultClimateGSTest),
  NVBorderRegion (priority 90) and NVColoradoRiverRegion. The rules NVForever follows:
  - The list followed is the climate's or an overriding region's, sampled by its
    outline at the camera's x/y. The outline is looked up at the start and when that
    point enters another exterior cell (4096-unit grid); between those events the
    sampled region stays, even across an outline inside one cell [observed: captures
    run15b_w and run15b_w2, 2026-11-15, #2531]. A script MoveTo samples again and changes
    weather at once [observed: same captures]. Load doors, other teleports, fast travel,
    loaded games, worldspace changes and coming outdoors also refresh placement
    [hypothesis: inferred from the script move; design: sky.rs::place_outdoors,
    SkyState::set_interior, world.rs::travel and worldspace.rs retain placement paths].
    Walking into a cell fades a changed weather; a new placement snaps [observed for
    walking and script MoveTo: same captures; hypothesis for the other travel cases].
    A same-world climate rebuild retains that sampled cell and region (rebound by
    FormID), so ForceClimate/SetCurrentClimate do not trigger a fresh outline lookup;
    current simulation gate values seed the rebuilt sky before its first selection,
    so a stored-zero/live-nonzero gate does not briefly select ungated weather or
    consume extra rolls. Travel invalidation and worldspace rebuilds still refresh placement [design,
    #2531: sky.rs::carry_region_placement, worldspace.rs::carry_climate_sky and switch;
    implementation of the settled lookup rule, not an additional capture claim].
    Travel stays before weather lookup and streaming. The test-hit producer precedes
    death handling so a test kill can start its ragdoll in that frame's PostUpdate;
    the next frame's Physics volume sync (including its optional comparison pipe)
    then precedes Activation dismemberment through the existing PlaySet chain [design,
    #2531: engine/nvforever/src/main.rs, deaths.rs::DeathsPlugin, ragdoll.rs::RagdollPlugin;
    engine/nvf-play/src/knockouts.rs::TestHitSet, lib.rs::PlayPlugin;
    regression scenario `hitvol-marked-corpse-parts`].
    Indoors the list stays as it was [hypothesis]. Among overlapping overriding regions,
    highest priority then lowest FormID wins [hypothesis: the flag is named by
    JohnnyGuitar's GetRegionWeatherOverride and priority by GetRegionWeatherPriority
    (GECK wiki); bit 0 is read from the data above; design: existing tie rule in
    fnv_world::sky::region_at]. The cell's XCLR list is not the membership test
    [observed: same captures].
    - At the Legate's stairs, NVLegateRegion (0x0015E248) belongs to WastelandNV
      (0x000DA726). Its one area has RPLI fall-off 1024 and RPLD points
      (101288.5, 23835.9), (109583.6, 23835.9), (109495.4, 17570.5),
      (100670.8, 18100.0). Its RDAT type 3, flags 1, priority 100 lists
      NVlegateCampWeather (0x001735E9), chance 100. The east outline is x 109513 at
      y 18842; CampLog03 stairs near y 18885 span x 109330..109815. LegateCampArena
      cell 26,4 spans x 106496..110592, y 16384..20480 and lists the region in XCLR,
      as do cells 24..26 x 4..5; cell 27,4 does not. Climate weather there by day is
      NVWastelandClear (0x000FFC88) [observed: FalloutNV.esm, fnvtool esm decode,
      2026-11-15, supplied records work for #2531].
    - With first-person view, GameHour 12, xNVSE 6.4.9 and JIP LN 57.30,
      GetCurrentWeather/GetCurrentWeatherPercent after script moves read clear at
      (110686, 19125) in cell 27,4 and (109775, 18882) in cell 26,4, 260 units outside
      the outline (clear for 40 s); the camp weather at (109330, 18852), inside; clear
      again on moving back outside. Every new weather was complete (percent 1.0) at
      the first sample, 4 s after the move [observed: captures run15b_w and run15b_w2,
      2026-11-15].
    - Walking east along y 18849 crossed both the outline (x 109513) and fall-off
      distance (x 110537) without changing weather. Last camp sample: x 110567.9;
      first clear sample: x 110596.0, percent 0.000, across cell border x 110592.
      Percent rose 0.033/s, complete after about 30 s. Walking west reentered cell
      26,4 outside the outline; clear persisted to (109067, 18849), 450 units inside,
      and for 49 s standing there [observed: captures run15b_w and run15b_w2,
      2026-11-15].
  - Eligible entries [hypothesis]: an entry with a global counts only while the global is
    non-zero, and such entries win over the ungated ones; with none open, the ungated
    entries. A plain draw over all open entries would give the day weather on half the
    nights and the peaceful Hoover Dam weather in half the battles, which the scripts
    setting those globals do not mean. Globals are read live from the scripts (`--play`),
    except VNight, which follows the hour exactly as NightQuestScript sets it.
  - A roll is a weighted draw, chance / sum of chances (all chances 0: equally likely)
    [hypothesis: GECK "chance" read as a weight]. A roll is made when the list followed
    changes (start, entering or leaving an overriding region) or its eligible entries
    change (VNight at 21:00 and 4:18, the battle global), and, at each whole game hour,
    with probability volatility / 255 [hypothesis: GetClimateTraitNumeric names the byte
    "Volatility" (GECK wiki); its scale and schedule are not documented; with the shipped
    0 the Mojave's weather never changes by itself, as the unmodded game's is known not
    to]. The roll's random numbers come from a splitmix64 stream seeded by
    `--weather-seed` (a fixed default, so every run rolls alike) and kept in saves.
  - The change fades over `WEATHER_FADE_HOURS` (0.5 game hours, a placeholder: WTHR DATA
    "trans delta", whose unit is not known; SetWeather's wiki page says the vanilla
    transition is at most 30 s) while the game clock runs, and is at once at the start, on
    a worldspace change and without a clock.
  - A script's weather (SetWeather, ForceWeather) shows until the next roll; with the
    override flag until ReleaseWeatherOverride and then until the next roll [documented:
    GECK wiki SetWeather/ForceWeather "the weather will change back in the normal fashion
    according to the weather system"; hypothesis: that is the next roll].
  - A held script weather survives a door into another worldspace, even when that
    world's lists do not include it [hypothesis, #1997; observer check 209 in
    `docs/handoff/observer-checks.md`]. The lifetime follows the wording of both
    [ForceWeather](https://geckwiki.com/index.php?title=ForceWeather) and
    [SetWeather](https://geckwiki.com/index.php?title=SetWeather): "If OverrideWeatherFlag
    is 1, the weather will not change again until ReleaseWeatherOverride is called."
    [documented: offline GECK wiki snapshot 2026-10-28].
    [ReleaseWeatherOverride](https://geckwiki.com/index.php?title=ReleaseWeatherOverride)
    says to "allow the weather to resume normal dynamic operation" [documented: same
    snapshot]. The cross-door interpretation remains provisional pending check 209.
    The replacement sky keeps the script/pin/resume state, shown and fading-out weather,
    fade progress and those weathers' image-space adapters; the destination supplies
    its own climate and region lists for later rolls. Natural unpinned weather starts
    from the destination's sky [design: `worldspace.rs::carry_sky`,
    `sky.rs::SkyState::carry_weather_from`]. A save after the door uses the same shown
    weather and held flag as a save before it; load restores the hold before the first
    placement [design: `sky.rs::SAVE_HOOK`, `SkyState::restore_saved`,
    `nvf-play/src/save.rs::header`, `nvforever/src/main.rs` load setup].
  - A loaded game shows its saved weather and keeps it, without a roll, while the list
    followed at its place lists it; `--weather` pins one weather for the whole run (the
    parity lab passes it for every exterior frame; interior frames do not use the sky).
    A weather a script held when the game was saved is held again from the first frame,
    listed there or not: the list's roll at the start is still made, and only decides
    what shows after the release and the next roll [design: `SkyState::restore_saved`].
    A script's weather not held is not kept: at the start's roll the list's weather
    replaces it, unless the list holds it.
- **Weather sound, precipitation and lightning** (`fnv_world::sky::Weather::{sounds,
  precipitation_model, lightning}`, engine `weather_fx.rs`; built 2026-10-21, ESM gap
  audit row 9). The fields [documented: xEdit wbDefinitionsFNV WTHR, wbWeatherSounds]:
  `SNAM` (repeating) = a SOUN and a kind, 0 Default, 1 Precipitation, 2 Wind, 3 Thunder;
  `MODL` = the precipitation model (GECK wiki GetWeatherPrecipitationModel: "the
  Precipitation model path of a weather"); `DATA` bytes after wind speed: trans delta,
  sun glare, sun damage, precipitation begin fade in, precipitation end fade out,
  lightning begin fade in, lightning end fade out, lightning frequency, the "weather
  classification" flags (1 Pleasant, 2 Cloudy, 4 Rainy, 8 Snow; GECK wiki
  GetWeatherTraitNumeric traits 8-14 name the same fields) and the lightning colour (RGB).
  What the masters hold [observed, `weather_dump --sounds` over the ten masters]:
  - No WTHR has a `MODL`: no weather in the masters draws precipitation by itself.
  - Sounds: the seven `DLC01Pitt…` weathers (Fallout 3 leftovers, no NV worldspace uses
    their climates) name a Default sound 0x000922EA that is not in the load order; Honest
    Hearts' two rain weathers (NVDLC02ZVRainWeather, NVDLC02TheNarrowsRainWeather) name
    one Thunder sound, AMBNVDLC02ThunderDis1. No other weather has a sound.
  - Lightning: the two Honest Hearts rain weathers are the only Rainy ones (class 4,
    lightning frequency 138, fades 128/128, colour white); the clear weathers store
    frequency 255 and a black colour with the Pleasant class; the overcast ones (class
    Cloudy) frequency 15 or 50.
  - Honest Hearts' rain and Hidden Valley's dust storm are drawn by scripts, not the
    weather: NVDLC02rainscript moves the activators NVDLC02Rain/NVDLC02RainUp
    (`meshes\nvdlc02\sky\nvdlc02rain*.nif`; see **Rain** below) to the player and enables the rain-sound
    reference NVDLC02AmbRainEffectREF while IsRaining; VHiddenValleyDustStormQuestScript
    sets HVDustStorm (a weather with fog and wind 127 but no sound or model) and enables
    the placed HVSandStormNight01-10 references, and HVSandStormAudioScript plays the
    storm loop. Those run through the script engine and the placed references as any
    script does; nothing here is special to them.
  The rules NVForever follows:
  - **Outdoors only.** Indoors (the cell's own lighting) no weather sound plays, no
    precipitation is drawn and no lightning flashes [documented: GECK wiki
    TriggerLightningFX "will only work when the player is outdoors"; hypothesis for sounds
    and precipitation: the rain script plays its own interior rain sound, so the weather's
    does not reach indoors].
  - **Sounds.** Default and Wind sounds loop at the listener while their weather is shown
    (from the start of the change to it until the start of the change away);
    Precipitation sounds loop while its precipitation falls (below) [hypothesis]. They play
    through the effects volume, as every SOUN does (`nvf_play::sfx::LoopSound`). Log:
    `weather fx: <SOUN> (<kind>) loops for <weather>` / `… stops`.
  - **Precipitation** falls while the weather's change has passed its begin-fade-in byte
    (as a share of 255 of the change) and, when changing away, until the change passes its
    end-fade-out byte [hypothesis from the field names; a weather shown without a change
    rains at once]. Its model is drawn about the camera: a holder at the camera's place,
    moved with it every frame, with the model's particle systems emitting there (world
    space particles stay where they fell). Log: `weather fx: precipitation <model> on for
    <weather>` / `… off`.
  - **Lightning** flashes in a weather with a lightning frequency above 0 that is Rainy
    or has a Thunder sound [hypothesis: TriggerLightningFX "only work[s] when ... it is
    raining"; the clear weathers' 255 would otherwise flash], over the same share of the
    change as precipitation but with the lightning fade bytes. The time to the next flash
    is drawn evenly from 0.5 to 1.5 times `LIGHTNING_MEAN_MIN + (LIGHTNING_MEAN_MAX −
    LIGHTNING_MEAN_MIN) × (1 − frequency / 255)` real seconds (4 to 40: 138 gives about 22)
    [hypothesis: the byte's scale is not documented; higher is read as more often]. A
    flash adds the lightning colour to the ambient light and the sky's glow, falling off
    over `FLASH_SECONDS` (0.35 s) with a second, weaker pulse [hypothesis, a look]. A
    Thunder sound (one of the weather's, at random) follows each flash after 1 to 4 s, as
    from a strike some hundreds of metres to over a kilometre away [hypothesis]. Log:
    `weather fx: lightning flash in <weather> (colour r,g,b), next in <s> s` and
    `weather fx: thunder <SOUN> <s> s after the flash`.
  - Test aid `NVF_PLAY_TEST_WEATHER_FX=precip=<model>;sound=<SOUN EDID>` gives the shown
    weather a precipitation model and a Precipitation sound, as a mod's weather would
    have (no master weather has either); the smoke scenario `weather-sound-precip` uses it
    with Honest Hearts' rain model and rain loop beside NVDLC02ZVRainWeather's own
    lightning and thunder.
  - Not built: the Snow class, the sun damage byte, the wind's direction (the speed moves
    trees and grass, `tree.rs`, `grass.rs`; the sun glare byte is drawn since
    claude/em8-visuals, "Choosing a weather" above), GetWeatherTraitNumeric and the JIP weather setters, and lightning lighting
    the clouds by their own colour.
- **Weather colours (WTHR NAM0)**: ten colour types in the order sky-upper, fog, unused,
  ambient, sunlight, sun, stars, sky-lower, horizon, effect lighting [documented]; each
  has one RGBA byte colour per time of day: sunrise, day, sunset, night and, in the
  6-time layout, high noon and midnight [documented]. 90 weathers store 6 times (240
  bytes), 8 store 4 (160 bytes) [observed]. Cloud colours (`PNAM`) follow the same
  pattern per cloud layer (96 or 64 bytes) [observed]. Cloud textures: `DNAM`, `CNAM`,
  `ANAM`, `BNAM` (layers 0-3) [documented].
- **Fog (WTHR FNAM)**: day near/far, night near/far, day/night power [documented].
  NVWastelandClear: near -10, far 200,000, power 0.6 / 0.5 [observed].
- **Unset fog ranges** [observed, `crates/fnv-world/examples/weather_dump.rs --fog`]: 6
  of the 98 weathers in the ten masters store a day or night range whose far is not
  beyond its near; every other range has far > near (the shortest is Dead Money's
  NVDLC01ToxicWeather, day 16..768, so short ranges are real). HVDustStorm stores day
  0..0 and night 510..55,000. InvertedDaylightWeather, InvertedDaylightWeatherWarm,
  InvertedDaylightWeatherWarmNV, InvertedDaylightWeatherTenPenny and Old World Blues'
  NVDLC03crystalEmWeather store 0..0 for both, and all five have black fog colours at
  every time. No climate lists HVDustStorm; only scripts set it
  (HVSandstormWeatherAddingScript, HVBunkerDoorToOutsideScript,
  HVSandStormFastTravelScript, VHiddenValleyDustStormQuestScript), always under
  `Time > 21.0 || Time < 6.0`, and the quest script forces NVWastelandClear back by
  day [observed: SCTX]. With the Mojave's sunrise 6:00-8:00 and sunset 18:00-20:00,
  its day range is never weighted in. InvertedDaylightWeather (the climate and region
  of the same name; cells in FXInvertedDaylightWorld) has a black sky and black day
  light but white night ambient and sunlight; taken literally, its 0..0 black fog
  would draw that white-lit world solid black.
  The renderers therefore treat such a range as unset: the other range is used for
  both day and night, and with neither the weather has no fog [hypothesis]. A weather
  fade to or from a weather without fog fades the fog's strength.
- **Time-of-day blending** [hypothesis, `fnv_world::sky::time_keys`]: piecewise linear
  between keys at 0 h (midnight), sunrise begin (night), mid-sunrise (sunrise), sunrise end
  (day), 12 h (high noon), sunset begin (day), mid-sunset (sunset), sunset end (night),
  24 h (midnight). The sun's path: next item. Verify against in-game screenshots at
  several hours.
- **Sun path and shadows** [observed 2026-10-09: screenshots of the original game by the
  capture plugin (tools/capture README section 7), private, at the player position
  (−70500, 1200, 8347) in Goodsprings, NVWastelandClear forced, FOV 75 read as Hor+
  (vertical 59.6° at 16:9), camera pitch −15 to −60; the sun's centre located as
  the centroid of the saturated disc]. Sun elevation and azimuth (compass, 90 = east):
  08:36 26.8 / 102; 09:06 36.4 / 102; 11:06 67.6 / 115; 12:06 78.3 / 135; 13:06 82.6 /
  186; 14:06 76.6 / 231; 15:06 65.0 / 248; 16:06 50.4 / 254; 16:36 42.5 / 256; 17:06
  about 35 / 258; 17:36 25.8 / 258; 17:51 23.7 / 259; 18:06 20.0 / 260; 18:21 17.3 / 261;
  from 18:36 the sun is behind the hills west of that spot (about 15° high there).
  So the sun passes about 7° south of the zenith shortly after 13:00 and stays about
  10° south of due east / west when low, and it is still well up at 19:00, the
  middle of the climate's sunset (the old model, rising and setting at mid-sunrise /
  mid-sunset on a path tilted 26° south, put it below the horizon at 19:00 and
  missed these positions by 12.9° RMS). The renderer's model [hypothesis, fitted
  to the 14 positions above, 3.6° RMS]: the sun rises due east at sunrise begin
  and sets due west at sunset end (the Mojave: 6:00 and 20:00), moving at a constant rate
  in angle theta from 0 to 180°, in the direction normalize(cos theta,
  −tan 9°, sin theta) (x east, y north, z up): highest 81° at 13:00, about
  12° up at 19:06. Letting it rise and set 15 minutes later / earlier fits
  slightly better (3.0°); the residuals (model about 4° high around 17:30–
  18:20, 3° low around noon) suggest the real motion is not uniform, which these
  frames cannot settle. Sunrise was not seen directly (hills to the east).
  Below the horizon theta runs on from 180° to 360° at the night's own constant rate
  (the Mojave: 10 hours), so the path closes at sunrise begin and the moon, drawn
  opposite the sun, rises at sunset end and peaks at 01:00 [hypothesis; the night
  sky was not measured]. Running the day's rate on through the night instead made
  theta jump 51° at midnight, which flipped the moonlight's shadows east to west in
  the time-lapse [observed in nvforever].
  **No sun shadows from the world** [observed]: the reference frame goodsprings_dusk
  (19:06, looking north-east) shows the crate's front, the boulder's west flank and the
  ground around them lit warm by the sun, and no long shadows east of the boulder or
  the crate, although from the same spot the sun is already hidden behind the rocks to
  the west (screenshot looking west at 19:12: no sun disc, purple sky). At 17:36 looking
  east down the road at (−69500, 2500), with the sun low behind the camera, the 8–10 m
  telegraph pole and the trailer cast no shadow on the road. The renderer drew sun
  shadows by default from 2026-10-09 (owner decision); since 2026-10-13 they are off by
  default, as in the game, until their frame cost and flicker are fixed (owner decision;
  `nvforever --sun-shadows` or the settings page turns them on), to 8,000 units in 3 cascades since 2026-10-12 (4 on Quality and 3 on Performance, staggered, proposed in #2843) for the frame
  rate (docs/design/graphics-enhanced.md §5.3). With shadows on
  and the corrected sun, the rock formations west of Goodsprings shade the whole town
  from about 18:40 [observed: renderer]. Whether actors cast shadows in the game is not
  known from these frames (nvforever draws no actors yet).
- **Light on surfaces facing away from the sun** [observed, same screenshots; open]: a
  boulder face lit only by ambient light shows (35..45, 27..36, 24..28) in the game
  between 17:06 and 19:12, warm, and (24, 23, 29) at 20:36; the renderer shows (17..23,
  16..19, 20..25), bluish, and (35, 39, 48) at 20:36. Part of the gap is Bevy's: its
  diffuse ambient is EnvBRDFApprox(diffuse, F_AB(1, NdotV)) = diffuse × 0.452 − 0.002
  [documented: bevy_pbr 0.19.1 pbr_ambient.wgsl], so the ambient colour reaches surfaces
  at 45% of the texture × ambient model. Compensating it (× 2.21) matched the brightness
  by day and at dusk and raised the SSIM of all four reference frames slightly, but kept
  the bluish tint (the weather's ambient colours are all bluish: sunset (183, 183, 242))
  and made 20:36 twice as bright as the game. So the game's light on those faces is
  warmer than the ambient colour by day and dimmer at night; how is not known (a wrapped
  or bounced share of the sunlight, or an ambient scaled by time of day, are candidates
  to test). Not changed yet. Eye adaptation is an unlikely cause: NVDefaultExterior's HDR
  luminance ramp min and max are both 1.0 [observed: `fnvtool esm decode`], which would
  hold the adaptation fixed if they bound it [hypothesis].
- **Dusk sky timing** [observed, same screenshots looking west; open]: the mean colour of
  the upper-right sky (clouds and sky) is (126, 122, 122) at 18:06, (123, 97, 104) at
  19:12, (92, 75, 84) at 19:36, (56, 49, 58) at 20:06 and (13, 15, 23) at 20:36; relative
  to 19:12 in linear light 1, 0.54, 0.20, 0.02. The renderer's keys (sunset colours at
  19:00, night at 20:00) give 1, 0.53, 0.05, 0.05; night at 20:30 instead gives 1, 0.72,
  0.35, 0.05. Neither fits clearly better and the dusk reference frame is the same with
  both, so the keys are unchanged. Still open after the sky changes below: at 20:00
  looking west the game's sky at 20-45° is (48, 43, 53) and the renderer's (9, 10, 18)
  [observed, diag frame `diag_west_200`]; the renderer is at full night colours there.
- **Sky and grade across two worldspaces (2026-11-16; this replaces the fitted items
  marked below)** [observed: own captures with `tools/capture/capture_frames.py`, the
  original at 1280 x 720 and our renders at the cameras it recorded; private, in
  `C:\fnv-private\capture`]. Sets: Tale of Two Wastelands' Capital Wasteland (Springvale
  and by Vault 101, `WastelandClear`, 07:00 to 23:00, 12 frames); the Mojave at Goodsprings
  (`NVWastelandClear`, 10 frames); the base game's own `WastelandClear` at Goodsprings by
  the hour (22 frames) and at five elevations and two headings at 09:09 (10 frames).
  Before, mean distance between the original's and our mean colours over the sky and
  ground regions (0-255, over RGB): Capital Wasteland 119.3, Mojave 17.7; the Capital
  Wasteland's noon sky was (97, 84, 76), dark brown, against the original's (174, 200,
  173). With the five points below: 32.0 and 14.6; that sky (166, 180, 156).
  - **Adapter tint.** The weather's adapter (IMAD `TNAM`, time 0: a colour and an alpha)
    counts as one more tint beside the image space's own: colours averaged by their
    amounts, the larger amount kept, the rule for several modifiers' tints [documented:
    GECK wiki "ImageSpace Modifiers"; hypothesis that a weather's adapter is one of them].
    Fitting the tint amount freely (our render without the grade, our grade formula, one
    exposure factor) to the original's ground: Capital Wasteland noon 0.31 (base amount 0,
    adapter 0.314: `WastelandBaseImageSpace`, `WastelandDayISFXDC`), 19:00 0.53, 23:00 0.58
    (adapters 0.392 and 0.627); Mojave noon 0.36, 16:00 0.31 (base 0.33, adapter 0.392).
    The same fit on the sky dome gave 0.36 under the base game's `WastelandClear` (rule:
    0.33). The earlier rule, the base amount times the adapter's alpha, gave 0 and 0.13.
    The Fallout 3 adapters set every value (multiply 0, add the value), the Mojave's scale
    the base (multiply, add 0) [observed: `fnvtool esm decode`].
  - **High noon.** A six-time colour set uses its high-noon column: day at the end of
    sunrise, high noon at 12:00, day at the start of sunset, linear between, on the stored
    bytes. FalloutNV.esm's own `WastelandClear` has black high-noon and midnight columns:
    in the base game the whole picture is black at 12:06, and the sky's blue is 237, 135,
    52, 19, 0, 18, 39, 84, 142 at 9:06, 10:06, 11:06, 11:36, 12:06, 12:36, 13:06, 14:06,
    15:06 (climate `NVDefaultClimate`: sunrise 6:00-8:00, sunset 18:00-20:00). The sky is
    (13, 14, 11) at every hour from 21:06 to 5:06, so the midnight column is not blended
    in. The weather's high-noon adapter is taken the same way [hypothesis]. In the
    Mojave's `NVWastelandClear` cloud layers 1 and 2 are white by day and black at high
    noon, and sky-upper is (72, 91, 159) for (50, 71, 135).
  - **Dome weights.** `atmosphere.nif`'s vertex colours: red weighs the horizon colour,
    green sky-lower, blue sky-upper, mixed as display values. Base game `WastelandClear`
    at 09:09 (upper (40, 46, 255), lower (123, 156, 242), horizon (162, 194, 204), each
    x 0.71 for the high-noon share), looking away from the sun, grade taken out: red and
    green at 46, 31 and 25 degrees up are (52, 59), (74, 93), (84, 105); 1.4 x (blue x
    upper + green x lower) gives (53, 65), (76, 93), (81, 101); with green as the horizon
    colour (91, 108) at 31 degrees. Near the zenith the original has about (25, 20) more
    red and green than the model [not explained]. The fitted horizon curve and its
    twilight factor are gone.
  - **Sky level** [hypothesis, fitted to two worldspaces]. The sky's display colours
    (dome and clouds) are scaled by the image space's "LUM Ramp No Tex" with the
    adapter on it: 0.88 by day in the Mojave (`NVDefaultExterior` 1.1 x `NVWastelandIS`
    0.8), 1.55 in the Capital Wasteland (`WastelandDayISFXDC` sets it). Scaled by the
    emissive mult (1.5 and 1.4) the Mojave's noon sky was (189, 181, 170) against the
    original's (145, 137, 129) and the Capital Wasteland's (156, 169, 146) against (174,
    200, 173); by this value (151, 143, 133) and (166, 180, 156). What the value means in
    the original's HDR pass is not known.
  - **Taken out**: the daytime dimming of the sky to 0.4 with a share of the sunlight
    colour added, and the sunlight hue in the clouds' and the fog's colour ("Fog colour by
    day" below). They were fitted in the Mojave under the weak tint and without the
    high-noon column, and made the Capital Wasteland's teal sky dark brown.
  - **Cloud layer 0 was never drawn.** Freeside's and the Strip's weathers (`NVUrbanClear`,
    `NVTheStripWeather`) put their only cloud texture, `sky\NV_WastelandUpperSky2.dds`, on
    layer 0, which goes on `clouds.nif`'s first shape, `CloudDome`; the Mojave's clear
    weather uses layer 3 (`LowerLayer`). The original shows scattered clouds over Freeside
    at noon; ours showed none, in the 3 November build as well. `CloudDome` is 0 to 6 %
    farther from the centre than `atmosphere.nif`'s dome, and the renderer's sort bias on
    the blended sky parts also acts as a depth bias of about 14 % of the distance, so the
    shape failed the depth test against the dome at its own size, at 0.97 and at 0.9 of
    it; at 0.75 it is drawn. The same texture on layer 3 (`NVWastelandHazy`) always showed
    [observed: own renders; the bias mechanism is a hypothesis that fits these figures].
  - **The Strip and Freeside** [observed 2026-11-17, base game, 20 frames]. The Strip's
    noon sky is a cool grey-purple in the original, (93, 90, 99) in one open patch; ours
    was (117, 111, 106) in the 3 November build and is (98, 96, 109) now. Freeside's near
    the noon sun is (141, 139, 123); ours (110, 111, 103), still darker (the sky near the
    sun, below).
  - **Sweep of the other worldspaces** [observed 2026-11-17: 112 frames of the original
    under Tale of Two Wastelands, one worldspace per climate (27), each climate's weather
    forced, noon and 17:00, level and 45 degrees up; our renders of the 58 noon cameras].
    Close to the original in colour and look: Sierra Madre (Villa, both towns), Big MT,
    Zion (clear and rain), the Divide (NVDLC04DivideClimate, LR, HR, the vista), the
    Citadel, Adams Air Force Base, DC's Mall, Monument top and Mama Dolce's (ours a little
    lighter), The Pitt's market and uptown (our sky darker than the original's glow),
    TTW's Lucky 38 climate. Off:
    - **Point Lookout** (`DLC04PointLookoutBoardwalkWthr`, fog 1 to 14,000 units): the
      original's sky and distant fog are bright and pale, (214, 227, 202) level; ours
      (134, 146, 126). Its worldspace's image space is `DefaultImageSpaceExterior`, tint
      (0.60, 0.54, 0.39) at 0.6; the adapter's tint is (0.02, 1, 1) at 0.078 and its LUM
      Ramp No Tex 1.5. With the adapter's tint in place of the base's the sky is (166,
      194, 181): lighter but too blue, and the Mojave's night ground goes from (44, 43,
      48) to (32, 41, 62) against the original's (48, 47, 51) (mean distance over the
      Mojave frames 14.6 to 18.1), so the rule above stays. [hypothesis] The sky's
      colours times 1.5 are past the display range, and the original's grade works on
      them before they are held to 1: 1.5 x (220, 233, 225) tinted olive at 0.5 gives about
      (255, 255, 240). Here the sky is held to 1 first and the tint then darkens it. The
      fog colour gets no such factor here at all.
    - **Anchorage** (`DLC02AnchWthrClear01`): the original's sky is a bright cyan, (61,
      104, 120) up and (67, 146, 163) level; ours (57, 75, 79) and (52, 99, 107).
    - **The Pitt's bridge** (`DLC01PittWthrBridge01`): the original is green fog all
      round; our frame is black with a pale patch [not looked into: the camera may be
      under our land there].
    - **The nuked Divide areas** (`NVDLC04NukedWeather`): our sky is yellow, (253, 238,
      174) up, where the original's is near white, (255, 255, 232).
    Not compared: Tranquility Lane and the GNR roof (the original did not travel there
    with `cow`), and the 17:00 frames (captured, ours not rendered).
  - **Left open.** The Capital Wasteland's ground is still too bright (ours x 0.79 at
    noon and x 0.54 at 16:00 would match). Its 07:00 sky is (213, 199, 147) against (246,
    235, 170). Colours past the display range lose their excess before the grade here
    (the base game `WastelandClear` sky's blue is 104-123 against 138-149): the
    original's grade evidently works on values over 1. The original's noon sky near the
    sun is brighter than the model (glare, not measured).
- **Sky colours** (its fitted horizon curve, twilight factor and daytime dimming are superseded by the 2026-11-16 item above) [observed 2026-10-09: the four reference frames (tools/capture README
  section 7) and 30 diagnostic screenshots of the original game at Goodsprings
  (07:00-20:30, NVWastelandClear, private), compared region by region with nvforever
  renders of the same cameras; "dark 10%" is the mean of the darkest tenth of a region,
  which picks the gaps between clouds]. The game's sky at noon is a dark, nearly neutral
  slate with bright, warm clouds and a warm, bright band along the horizon: looking
  north-east at 12:06 the gaps are (114, 115, 130) at 29° elevation, (140, 138, 142) at
  20°, (172, 166, 169) at 12° and (195, 184, 165) at 7°; the brightest tenth of the
  clouds is (250, 239, 186). Near the zenith, away from the sun, the gaps are (80, 79,
  96). The renderer drew (197, 203, 208) at 29° and (223, 227, 218) at 7°, nearly white,
  with clouds barely visible. Causes and the renderer's model:
  - The horizon colour reached too high. Undoing the image-space grade (as the renderer
    applies it) on the dusk reference frame (19:06, looking away from the sun) and
    solving gap colour = w·horizon + blue·sky-upper per elevation, with the weather's
    sunset colours, gives w = 0.94 at 4°, 0.86 at 7°, 0.74 at 9.5°, 0.64 at 12°, 0.55 at
    15°, 0.45 at 20° and 0.28-0.32 from 22° to 29° (residuals within 16 per channel);
    the vertex green is a flat 0.55-0.64 from 4° to 17°, and mixing linear light
    weights a bright colour more still. The renderer mixes the colours as display (sRGB)
    values with sky-upper × the vertex blue and horizon × w(e) = 0.72·exp(−(e/14°)²) +
    0.28·exp(−(e/60°)²), turning to sky-lower by 10° below the horizon [hypothesis,
    fitted]. At night the same reading keeps 30-45° nearly black, as in the game at
    20:36 (0-6 per channel).
  - **Night sky** (2026-10-23). That night check was made while the grade read the night
    adapter's track 0x12 as contrast (1.32 around 0.2, which crushed dark values); read
    as the average luminance (below, "IMAD cinematic track order") the night grade is
    1.1 around 0.24 and the floor term lit the whole night sky: (10, 10, 17) near the
    zenith against the original's (0, 0, 3) at 23:00 [observed: own captures
    `C:\fnv-private\references\stars\game`, renders of the same cameras]. Neither
    grade reading fits dusk and night together (the old one makes the 19:06 sky too
    light). Solving the original's frames for the horizon weight under the current grade,
    per elevation (sky-upper weight free): at 19:06 (sun +11°, sunset colours) 0.25-0.28
    from 15° to 50° and 0.2 at 60-67°, the floor as fitted; at 23:00 (sun −54°) under
    0.08 from 25° up, and 0.7, 0.5, 0.3 and 0.1 at 12°, 15°, 18° and 21°, the near term
    alone. The vertex weights do not change with the hour, and the sunset fog or ambient
    colour in place of the horizon does not fit the dusk floor. Fitting the floor's share
    alone: 1.1 at 19:06, 0.6 at 20:30 (sun −9°, looking west), 0 at 23:00 (three
    cameras). The renderer scales the floor by a twilight factor, smoothstep from the sun
    at −18° (0, the end of astronomical twilight) to 0° (1) [hypothesis, fitted]. Mean
    sky-row error against the original (0-255 per channel): Mojave 23:00 views 15-16 → 3-5
    (the view 60° up: see "Eye adaptation"); 19:06, 20:30 and noon unchanged within 1;
    the 22:00 ambient frames' sky 10 % / 50 % / 90 % levels (3, 21, 43) original,
    (20, 32, 52) before, (8, 17, 33) after. Left open: at 20:00 the original's sky looking
    west is still a dusk sky (37-57 per channel at 20-40°) where ours is already night;
    the colour keys put full night at sunset end (20:00 here).
  - The sky went through the renderer's tonemapper and then the grade. The game's sky
    colours behave like display values that the grade changes directly (the dusk fit
    above needs no other factor). The tonemapper (Bevy's Tony McMapface) maps linear
    0.1 to display 89, 0.5 to 163, 1.0 to 195, 2.2 to 224, 5 to 241 and 100 to 253
    [observed, renders of a flat grey sky without the grade]; sky parts now pass their
    colour through the inverse of that response (sky.wesl) [chosen: lit surfaces keep
    the tonemapper, so noon ground brightness is unchanged].
  - By day the game's sky is darker and warmer than this model through the grade: the
    noon gaps fit about 0.63 × (the dusk-fitted mix of day colours) plus 0.1 × the
    sunlight colour, in display values, at every elevation from 7° to 29° and near the
    zenith; at dusk and at night no such change is needed. The renderer scales sky and
    cloud colours by 1 − 0.6·d in linear light and adds 0.05·d × the sunlight colour,
    d rising from 0 at sun elevation 0° to 1 at 40° (`SkyLight`) [hypothesis, fitted;
    the game's mechanism is unknown: eye adaptation (a brighter noon picture exposed
    darker) and bloom would both look like this, and would also brighten the dusk ground,
    which the renderer draws darker than the game]. Replacing the grade's tint amount by
    the adapter's tint alpha (0.39 by day) explains the noon sky's hue but not the dusk
    and night frames (it needs a blue dusk sky and an orange night sky before the
    grade) and makes the noon ground too yellow, so the grade is unchanged.
  - The clouds: `NVCloudlight.dds` (1024², DXT5) has light blue-grey colour (166-234)
    and a mean alpha of 0.23 (41% of texels below 0.12) [observed]. Over the darker sky
    they now stand out as in the game. The game's noon cloud highlights are warmer than
    the PNAM colour × texture, about by the sunlight colour's hue; the renderer moves the
    cloud colour half way to that hue in full daylight (`cloud_tint`) [hypothesis,
    fitted]. The cloud pattern's offset differs from the game's (scrolling starts at
    program start), so cloud structure lowers SSIM even where colours match.
- **Sun and glare** [observed, same screenshots]: CLMT FNAM `Sky\Sun.dds` (512², DXT5)
  is one colour (230, 218, 189) with an alpha of 0.6 at the centre falling roughly
  linearly to 0 at 0.9 of the half-width; GNAM `sky\NV_sunglare.dds` (512², DXT5) is
  white, alpha 1 at the centre, 0.5 at half the half-width, 0 from 0.85 [observed,
  decoded]. In the game the sun is a white disc saturated out to about 2° from its
  centre, with a glow: looking up at 12:00 the ring medians are (249, 225, 191) at 4-6°,
  (222, 201, 171) at 8-12°, (187, 170, 156) at 16-20°, (162, 149, 132) at 32-36° and
  about (134, 129, 129) beyond 44°. The renderer drew a faint 4° disc and no glare. It
  now draws the sun texture over a 20° square around the sun at 1.8 × the weather's sun
  colour (into the dome, behind the clouds) and the glare texture over a 130° square at
  0.12 × the sun colour (into the dome and the clouds), both as colour × alpha, fading
  out as the sun goes from 3° to −2° [hypothesis, fitted to the ring profile above:
  from 8° outwards the renderer's ring medians are within about 20 per channel of the
  game's (bluer channel lower); its saturated core reaches about 4° and is slightly
  yellow where the game's is white].
  With the sun at 37° (09:00) and 25° (18:00) the game's glow is much larger and also
  washes over the terrain near the sun; the renderer's is not [open: bloom?].
- **High-noon and midnight colours look unset** [observed,
  `crates/fnv-world/examples/weather_dump.rs --survey`]: of the 90 six-time weathers, 67
  have a black midnight ambient (64 of them with a non-black night ambient, among them
  NVWastelandClear), 59 a black midnight sunlight, and 29 black high-noon colours.
  NVWastelandClear's midnight horizon is (43, 200, 213), a saturated cyan, and its
  high-noon "unused" and "effect" entries are pure green. Blending them in made
  midnight black under a cyan sky in the renderer. The renderers use only sunrise,
  day, sunset and night (midnight → night, high noon → day) [hypothesis].
- **Sky meshes** [observed, `meshes\sky\` in `Fallout - Meshes.bsa`; no record names them,
  so the engine presumably loads them by path]: `atmosphere.nif` is one dome (radius
  ~503, z −164..250) with a SkyShaderProperty (sky object type 2) and vertex colours that
  act as blend weights: red is 1 below the horizon and fades out by z ≈ 137, green peaks
  around z 50..140 (just above the horizon), blue rises to ~0.5-0.7 towards the zenith.
  By elevation seen from the centre [observed, `fnvtool nif decode`]: red 1.0 at −10°
  and below, 0.69 at 0°, 0.46 at 5°, 0.26 at 10°, 0.11 at 15°, 0 from 20°; green 0.35 at
  0°, 0.59 at 5°, 0.64 at 10°, 0.63 at 15°, 0.57 at 20°, 0.45 at 25°, 0.35 at 35°, 0.21
  at 45°, 0.08 at 55°, 0 at 90°; blue 0.04 at 5°, 0.30 at 10°, 0.53 at 20°, 0.66 at 25°,
  0.70 at 45°, 0.55 at 70°, 0.51 at 90°. Read at first as red·sky-lower + green·horizon
  + blue·sky-upper; the renderer now uses blue·sky-upper with a horizon weight fitted to
  the original game (next items). `clouds.nif` holds four shapes,
  in order `CloudDome` (z 0..250), `HorizonLayerClear`, `HorizonLayerOvercast` and
  `LowerLayer` (z −38..100), each with a default cloud texture, vertex colour (1, 0, 0)
  and an alpha that fades the layer out towards the horizon. `stars.nif` is a small
  starfield sphere (`SkyStars.dds`). The renderer puts weather cloud layer i (DNAM, CNAM,
  ANAM, BNAM; colours PNAM, 4 or 6 per layer like NAM0) on the i-th shape [hypothesis:
  NVWastelandClearNight fills layer 0 with `NVWastelandStarfieldSky.dds`, NVWastelandClear
  fills layer 3 with `NVCloudlight.dds`, both leave the others at `sky\alpha.dds`].
  Cloud speeds (ONAM) are 52, 0, 0, 65 for both; their unit is unknown.
- **Cloud scroll** (2026-10-24): ONAM is one byte of speed per cloud layer [documented:
  xEdit wbWeatherCloudSpeed; GECK wiki GetWeatherTraitNumeric, traits 0-3 "Layer N Cloud
  Speed"]; neither source gives a unit. Each layer's texture scrolls along u by
  speed × 1/25,600 texture repeats per real second (`fnv_world::sky::CLOUD_SCROLL_PER_UNIT`,
  `Weather::cloud_scroll`) [hypothesis: linear, 0 still; the scale is the first sky pass's
  placeholder, so NVWastelandClear's top layer (65) drifts one texture width in about 6.6
  minutes; to be measured from two game screenshots of the same view some seconds apart].
  During a weather fade the outgoing weather's layers keep their own speeds and the
  incoming weather's theirs, each faded by the transition (the two cloud sets of
  `nvforever::sky::update_clouds`). The renderer logs `clouds: <weather> layer speeds
  [..] (ONAM), scrolling .. texture repeats/s` when a weather first shows (smoke
  `cloud-scroll-onam`).
- **Fog colour by day** (taken out 2026-11-16, see the item above) [observed 2026-10-21, the eight exterior reference frames against
  renders of the same cameras, mean colour per 30-pixel row]: distant land fades into a
  warm haze in the game and into a light blue-grey in the renderer. At
  spring_mountains_east (camera 18,448 up, looking east over the valley) the land rows
  are (173, 161, 131) to (100, 89, 58) in the game and (162, 168, 169) to (87, 83, 71) in
  the renderer; without fog (`--fog 0,100000000`) the renderer's land is (93, 78, 50) to
  (69, 55, 27), so the fog's share is about right and its colour is not: solving
  game = (1 - f)·unfogged + f·F with the renderer's f gives an on-screen fog colour of
  about (226, 217, 185) in the game against (220, 228, 230) in the renderer. The
  weather's day fog colour (150, 168, 190) cannot give that through the same grade that
  matches the near ground (the near ground's colours agree within a few levels at
  Goodsprings and Black Mountain). The renderer now moves the fog colour towards the
  sunlight colour's hue by day, as it does the clouds (`SkyState::fog_color`, half way
  in full daylight) [hypothesis, fitted; the game's mechanism is not known]. Full
  sunlight hue fitted the colour best (mean colour error of spring_mountains_east 0.077
  to 0.013, hoover_ridge_lake 0.048 to 0.011) but pushed black_mountain_north's SSIM just
  under the 0.6 threshold (0.602 to 0.5996; run-to-run noise is about 0.001), so half is
  used (0.046 and 0.030). SSIM, which the frame scenarios judge, moves by less than 0.005
  either way: it compares luminance structure, not hue. The same treatment of the sky's
  horizon colour (also too blue-white in the renderer: (235, 236, 210) against (224, 216,
  181) a few degrees up) made the mean colour worse at both shares tried (0.5, 1.0) and
  was dropped. The FNAM fog powers (day 0.6, night 0.5 [documented: xEdit names]) are
  were unused then; they are used since 2026-10-23 ("Fog power" below), which kept these
  frames as they were or better.
- **What holds the exterior frames under the threshold** [observed 2026-10-21]: with the
  game's own pixels pasted over the renderer's top quarter (the sky, mostly clouds), the
  eight exterior frames would score SSIM 0.63-0.85, all passing (goodsprings_noon 0.477
  to 0.653, goodsprings_dusk 0.487 to 0.677, goodsprings_hill_lod 0.557 to 0.700,
  lake_shore_noon 0.505 to 0.643, black_mountain_north 0.602 to 0.692, goodsprings_east
  0.439 to 0.634, spring_mountains_east 0.709 to 0.852, hoover_ridge_lake 0.562 to 0.741).
  The top-row tiles score 0.0-0.4 because the cloud pattern lies elsewhere, so the cloud
  placement, not the light's strength, decides the threshold; the rows below (near
  grass, shrubs and rocks) are the second gap (row SSIM 0.2-0.5 at Goodsprings).
- **Cloud texture placement does not match the game's** [observed 2026-10-21]: a render
  with the cloud layer's own texture coordinates written as colour (an experiment, not
  kept) gave each sky pixel's (u, v) for the four Goodsprings/Lake frames. Correlating
  the game's sky luminance (smooth gradient removed) with `NVCloudlight.dds`'s alpha at
  (u, v) + offset over every offset (1/128 steps, refined to 1/1024), with u and v
  swapped, negated and scaled by 0.5, 1 and 2, gave no clear peak for goodsprings_noon
  (best correlation 0.15-0.24, 2.2-3.8 standard deviations above the mean of 16,384
  offsets, what chance gives). So the game does not draw that texture with the
  LowerLayer shape's texture coordinates shifted by an offset: the layer-to-shape
  pairing, the use of the shape's coordinates, or the texture's scale differs
  [hypothesis: the game may project cloud textures by view direction instead]. Matching
  the clouds needs (1) the game's mapping, from frames of a still sky (one camera twice,
  a known number of seconds apart, and once looking straight up), and (2) each reference
  frame's seconds since the save was loaded (above: placement follows that time), which
  the frame manifest and scenarios do not carry; only the owner may add it
  (parity-lab/AGENT.md).
- **Stars and moon** [observed, `fnvtool esm decode --edid NVDefaultClimate`]: the
  climate has a `MODL` subrecord, `Sky\Stars.nif` (the starfield sphere above, additive:
  NiAlphaProperty 0x1001 = blend ONE/ONE), and its TNAM moons byte is 131 (0x83): bit 7 set,
  low bits 3, read as one moon flag and a 3-day phase length [hypothesis; the schema keeps
  the byte raw]. `textures\sky\` ships `masser_{full,new,half_wax,half_wan,one_wax,one_wan,
  three_wax,three_wan}.dds`, `skymoonfull.dds` and `moonshadow.dds`; which one the engine
  draws for that flag is not known. NVWastelandClearNight's cloud layer 0
  (`sky\NVWastelandStarfieldSky.dds`) exists in no archive; the renderer leaves that layer
  without a texture and, the weather being the base game's own, says so at debug only (a
  mod's weather naming a missing texture is still warned about; issue #2408; what the
  original draws on that layer is not captured). The NAM0 "stars" colour of
  NVWastelandClear is black at sunrise and white at day, sunset and night, so day-time
  hiding of the stars must come from elsewhere (engine) [hypothesis]. The renderer
  fades the stars in as the sun sinks from 0° to −12° and the moon from +5° to −6°,
  puts the moon opposite the sun, and picks `masser_<phase>` as
  floor(days passed / phase length) mod 8 in the order full, three_wan, half_wan,
  one_wan, new, one_wax, half_wax, three_wax [hypothesis]. Days passed start from the
  `GameDaysPassed` global (5.0 in the masters [observed], `crates/fnv-sim/src/clock.rs`).
- **Atmosphere dome shape**: about 1,000 units across but only 250 units above its rim
  [observed]. Sky objects drawn inside it (stars, sun, moon) must stay within about half
  its radius, or the opaque dome hides them overhead.
- **Night weather**: NVWastelandClearNight is gated by the global `VNight` (0 in the
  masters). NightQuestScript sets it: `If GetCurrentTime >= 21 || GetCurrentTime < 4.30`
  → `Set VNight to 1`, else 0 [observed: script source (SCTX) in FalloutNV.esm]. Until
  scripts run (Layer 5) the renderer evaluates that condition itself and fades between
  the weathers over half a game hour [placeholder; WTHR DATA "trans delta" not
  understood]. NVWastelandClearNight has no clouds (`sky\alpha.dds` on layers 1-3) and a
  darker night ambient (121, 128, 213) than NVWastelandClear (172, 170, 215).
- **Star domes** (2026-10-23). The data [observed, `fnvtool nif decode`, BSA extracts]:
  both domes are the same 16-unit sphere (434 vertices, UVs −1..3 × −2..2), NiAlphaProperty
  0x1001 (blend ONE/ONE, so the blend itself ignores alpha), SkyShaderProperty sky object
  type 5. Mojave `Sky\Stars.nif`: `SkyStars.dds` 512² DXT5, colour in 21,309 texels
  (6,225 above 15), alpha in only 436, every one of them with colour 192-255; shader
  flags 0x80000100 (Alpha_Texture); emissive × 1. Big MT `NVDLC02\sky\stars.nif`:
  `Skystars.dds` 1024² DXT1, alpha 255 in every texel, colour in 112,126 texels (4,028
  at 224+); flags 0x80000000; emissive × 10. Both weathers' night "stars" colour is about
  white (Big MT 245, 251, 254). On a 1280 × 720 frame at FOV 75 (~14.6 pixels a degree)
  the Mojave dome has ~12 texels a degree, the Big MT's ~24 (so mip filtering halves its
  single-texel stars).
  The original [observed: own captures, clear Mojave night 23:00 at −70500, 1200,
  `C:\fnv-private\references\stars\game`; Big MT 22:00, `references\owb\game`]: the
  Mojave sky shows ~19 stars per 10⁴ pixels (peaks p50 39, p90 69, max 143 grey); the
  Big MT ~76-91 (p50 39-54, p90 75-126, max 110). Rule [hypothesis]: a dome adds its
  texture colour × the weather's stars colour × `STAR_STRENGTH` (0.25), weighted by
  texture alpha only where the shader flags set Alpha_Texture; the emissive multiplier is
  not used (× 10 would saturate every coloured texel). With alpha weighting the Mojave
  star count matches the original at any strength; 0.25 fits the Mojave's peaks
  (40, 76, 130). It leaves the Big MT's stars about 1.7 × the original's (p90 126 vs 75),
  the same factor as our Big MT night background (24, 24, 50 vs 15, 13, 27), so the rest
  is the grade (owb-comparison F6), not the stars. Which part of the original's pipeline
  the 0.25 stands for is not known. Our night sky background was also lighter than the
  original's in the Mojave (10, 10, 17 vs 0, 0, 3) and did not change with the star
  strength [observed]: the horizon colour's floor, now faded out with twilight ("Sky
  colours", "Night sky").
- **Image space** [documented: xEdit wbDefinitionsFNV.pas IMGS, IMAD, WTHR, WRLD]: WRLD
  `INAM` names the worldspace's image space (IMGS); WTHR `\x00IAD`..`\x05IAD` name one
  image space adapter (IMAD) per time of day (sunrise, day, sunset, night, high noon,
  midnight). IMGS `DNAM` holds HDR, bloom, get-hit, night-eye and cinematic values
  (saturation, contrast average luminance, contrast, brightness, tint colour and value);
  xEdit places the cinematic block at byte 100 from form version 10 (skin dimmer added) and
  at 96 before [documented]; DNAM sizes 152 (69 records), 148 (12) and 132 (11) [observed].
  In `FalloutNV.esm` the 44 records with 152 bytes (form versions 14-15) read sensibly at
  byte 100, but the 12 with 148 bytes (form versions 11-13) and the 11 with 132 bytes
  read sensibly only at byte 96 (saturation near 1, contrast 1-2, tint amount 0-1; at
  100, e.g. ShackInteriorWaste01 gets saturation 0.07, the Prospector Saloon turned
  grey) [observed]. The renderers take byte 100 for a 152-byte DNAM and 96 otherwise
  [hypothesis: the skin dimmer arrives with the 152-byte layout].
  IMAD keeps multiply/add key lists (time, value) per parameter (`\x11IAD`..`\x14IAD`
  multiply and `QIAD`..`TIAD` add for the cinematic ones) and a tint colour key list
  (TNAM: time, RGBA). WastelandNV → NVDefaultExterior: saturation 1.1, contrast 1.1 around
  luminance 0.2, brightness 1.0, tint (0.98, 0.57, 0.00) at 0.33 [observed]. NVWastelandClear
  (and ClearNight) link NVWastelandSunriseIS, NVWastelandIS (day and high noon),
  NVWastelandSunsetIS and NVWastelandNightIS. At time 0 their keys are: day brightness
  ×1.3, amber tint (1.0, 0.74, 0.05) at 0.39; sunset saturation ×0.8, track 0x12 +0.3, tint
  (1.0, 0.44, 0.10) at 0.39; night track 0x12 ×1.2, tint (0.22, 0.47, 0.93) at 0.63
  [observed]. Later keys look like defaults (tint white at 0 or pure blue at 1).
  The renderer grades the tonemapped picture with the base image space × each adapter's
  time-0 keys (value × mult + add), the adapter tint counting as one more tint beside the
  base's (colours averaged by amount, the larger amount; "Adapter tint" above, 2026-11-16;
  before that the adapter's alpha scaled the base amount), blended by time of day like the
  weather colours.
  Formula on display values: saturation around luminance, tint towards
  luminance × tint colour, brightness factor, contrast around the average luminance
  [hypothesis]. The IMGS flags byte (saturation, contrast, tint, brightness) is 0 in
  NVDefaultExterior; its role is unknown. HDR eye adaptation is not applied.
- **HDR and bloom** (IMGS DNAM, 2026-10-23). The 14 HDR floats and the bloom block
  [documented: xEdit wbDefinitionsFNV.pas IMGS: eye adapt speed, blur radius, blur
  passes, emissive mult, target lum, upper lum clamp, bright scale, bright clamp, lum
  ramp no tex, min, max, sunlight, grass and tree dimmer; skin dimmer from form version
  10; bloom: blur radius, alpha mult interior, alpha mult exterior] are read by
  `fnv_world::imagespace` (`Hdr`, indexed in the IMAD track order `\x00IAD`..`\x10IAD`;
  adapters scale them as the cinematic values). NVDefaultExterior (the Mojave and the
  Strip) [observed]: eye adapt speed 0.9, blur radius 7, passes 3, emissive mult 1.5,
  target lum 1.4, upper lum clamp 1.0, bright scale 2.0, bright clamp 0.6, sunlight
  dimmer 1.1; skin dimmer 0.55; bloom radius 0.03, interior 0.8, exterior 0.2. Hoover
  Dam's lower level (0x0011C0C1): blur radius 6, passes 4, emissive mult 1.0, bright
  scale 2.4, bright clamp 0.9; bloom 3.0, 0.8, 0.2. The bloom block equals the owner's
  `[BlurShader]` INI values (fBlurRadius 0.03, fAlphaAddExterior 0.2, interior 0.7) and
  belongs to the non-HDR mode; the owner plays with `bDoHighDynamicRange=1`, where the
  HDR block applies [hypothesis from the names]. What the original computes from them
  is not documented. The renderer's light glow (docs/design/graphics-enhanced.md §3.11)
  uses Bevy's bloom on an HDR view, its strength scaled by Bright Scale / 2
  [engineering choice]; eye adaptation: next item. Above the Mojave's Bright Scale 2 a
  bright pass is added (`post.rs` `BrightPass`): on the HDR picture before the tonemapper,
  what is over its threshold (each pixel held to 16) is taken at a quarter of the size,
  blurred and added back [engineering choice]. Bevy's energy-conserving bloom made stronger
  spread every pixel's light, and the enhanced Bloom pass works after the tonemapper, where
  Gomorrah's tube (36 short quads at white × 25) averages under its threshold: the original
  merges them into one white-cored line with an orange halo. The enhanced Bloom adds to the
  light glow instead of replacing it (and with it the HDR view and eye adaptation).

  Settings since 2026-11-07 (#1540), fitted to the original's frames at 1280×720 with the
  street lights settled [engineering choice]: the light glow at 0.09 and no boost of its
  widest blur levels; the bright pass over 7, not 1, at 0.4 per unit of Bright Scale over 2
  (2 on the Strip at night, where NVTheStripNightIS sets 7), with a Gaussian about 17
  quarter-size texels wide. With the glow off the Lucky 38's and the Tops' letters read and
  the sky is dark [observed], so the old settings (0.15, 0.7, over 1, 0.08 per unit, 8
  texels wide) were what ran them together and hazed the sky. Only tubes pass the threshold;
  cut-out signs are under it ([nif.md §6 "Unlit shapes that are pictures, and cut-out
  signs"](nif.md#6-conventions-verified-on-the-data)). Share of pure white on the Lucky sign,
  original 9.5 %, old settings 27.4 %, these 10.7 %; on the Tops sign 8.2 %, 19.3 %, 9.7 %
  [observed]. Gomorrah's tubes gain width (share at full brightness in a channel: original
  28.9 %, old 18.7 %, these 28.7 %) and its walls lose most of the old settings' red wash; it
  has no white core in either, where the original does: the tonemapper holds the colour,
  and the plain path of blended shapes skips `hot_light` [design]. A modifier that lowers
  Bright Clamp (the birth scene) keeps a threshold of 1, 0.08 per unit and the narrower blur,
  for lamps blooming over a sharp picture (`post.rs` `modified_bright_pass`).
- **Eye adaptation** (2026-10-23). The HDR tab's Eye Adapt Speed "controls how quickly
  the adapted scene lum chases the actual lum", and Target LUM exists ("changing this
  just seems to break everything") [documented: GECK wiki ImageSpace, HDR tab]; what the
  adapted lum does to the picture is not documented. Measured on 31 original frames
  against renders of the same cameras without adaptation [observed: mean log2 luminance
  of the 10-90 % pixels, display values linearised]: the lab's eight Mojave exterior
  frames (−3.2 to −1.8 EV) agree within 0.9 EV (most within 0.3); the saloon (Target LUM
  0.75) agrees, Doc Mitchell's house (1.0) is 0.5-0.9 EV darker in the renderer; the
  effects audit's night spots above −7 EV agree within ±0.8; darker pictures are lifted
  in the original: Jacobstown lodge −9.6 → −7.9, Hoover Dam's lower level −10.5 → −9.2,
  Dead Money's Villa −9.1 → −6.5, −10.3 → −7.8 and −11.7 → −7.8, Puesta del Sol (whose
  adapter triples Target LUM) −10.0 → −6.6. As built [hypothesis, fitted to those
  numbers]: the renderer meters its HDR view (Bevy's auto exposure, 10-90 % of a log
  histogram) and brightens only, by 0.85 EV per EV the average lies under a reference,
  at most 5 EV; the reference is −6.8 EV at Target LUM 1.4 (NVDefaultExterior) and moves
  by log2(Target LUM / 1.4) with the image space and its adapters (Puesta del Sol −5.2,
  the saloon −7.7). Adaptation runs at 3 EV a second either way [engineering choice; Eye
  Adapt Speed is not used]. It needs the HDR view, so it is on with the light glow (the
  default) and off without it (`nvforever/src/eye_adapt.rs`). With the fog changes
  below, the Dead Money frames come out within 1 EV of the original's; the lab's frames
  move by under 0.1 EV.
  Until #2892 the exposure worked out from the meter was not exactly the lift: Bevy's
  pass read its curve as one byte entry per 0.086 EV, so every view was darkened by 0 to
  about 0.15 EV more than the lift says (0.075 on average), by an amount that jumped
  with the view [observed: noon by the road at WastelandNV (−17, 0), the picture with
  adaptation on 2.4 to 5.6 levels of 255 darker than with it off, differing by pitch;
  0.01 to 0.05 with the curve read exactly (`engine/vendor/bevy_post_process`)]. The
  fitted numbers above were made with that in them; they were not fitted again, the
  shift being a tenth of the fit's own spread (the frames agree within 0.3 to 1 EV).
  The sky counts 1/16 of a world pixel in the meter, from a mask drawn each frame from
  the depth prepass (`eye_mask.wesl`) [hypothesis, 2026-10-23]: once our night sky was
  as dark as the original's ("Night sky" above), metering it with the world brightened
  the Mojave 23:00 views by several EV (windmill and buildings far lighter than the
  original's, whose frames match ours without adaptation). With the mask those views
  match; the Dead Money exteriors, the Strip and Primm (fx audit spots) move by 0.1 EV
  or less. Still lifted: a view that is mostly sky with a dark object (the windmill seen
  60° up, 10 % / 50 % / 90 % levels (0, 0, 1) original, (10, 27, 131) ours): the dark
  object alone averages under the reference. The original's meter may be a plain
  average, which bright lights and the moon would hold up [hypothesis, unchecked].
  The Strip at night (2026-10-25): its adapter NVTheStripNightIS sets Target LUM 4, so
  the reference is −5.3 EV, and with Bevy's default meter (the 10-90 % of the histogram)
  the frames of the owner's spot (also `fx_strip_b`) were lifted about 0.3 EV (sidewalk
  81 against 75 without adaptation and 63 in the original). The meter now keeps the
  brightest tenth (`eye_adapt.rs` `METER_FILTER`, 10-100 %), where the Strip's neon and
  lamps lie, and that view is not lifted (sidewalk 74). Not rechecked against the 31
  frames of the fit (their cameras are not kept); the Dead Money lantern spot
  (`lantern-display-light`) keeps its brightness (mean 62.9 before and after, the
  original's `dm_lamp_front` 61.4). Frames of the Strip at night must be taken
  after its light show (script 0x00127E9E: the three street light groups switch on 0,
  about 30 and 60 s after arriving, so a frame at 12 s has two groups dark).
- **Emissive mult** (2026-10-23). "Emissive Mult: (non-imagespace) Multiplier applied to
  the emittance of an object" [documented: GECK wiki ImageSpace, HDR tab]. Values
  [observed]: NVDefaultExterior 1.5 (the Mojave's adapters leave it), interiors 1.0-1.3;
  Dead Money's NVDLC01VillaExtIS ×3.5 (5.25) and NVDLC01WestTownIS ×2 (3.0). In the
  original's frame of Puesta del Sol the lantern glass (emissive (0.76, 0.55, 0.20) ×
  10) burns near white; ours was dim orange. As built [hypothesis]: outdoors, emitted
  light (lit shapes' emissive and unlit shapes' emissive brightness) is scaled by the
  adapted value over the worldspace base image space's own (the Mojave 1, the Villa
  3.5, Puesta del Sol 2); indoors 1. Carried to the NIF shader in the view's fog
  uniform (`nvforever/src/fog.rs`).
  Not on additive shapes since 2026-10-25 (nif.md §6 "Additive blend"): the Strip's lamp
  beams blew out under the Strip's night value 4. Dead Money's lantern glow card (additive)
  lost its scaling with it: its halo is smaller than in #643's frames, the glass as bright.
- **Sunlight dimmer** in Dead Money (2026-10-23): NVDLC01WestTownIS sets it ×0.4 + 0.1
  (1.1 → 0.54), about half the Mojave's day value (1.21), and Puesta del Sol's original
  frames are lit by its yellow ambient ((119, 81, 9)) with the blue channel at 0, where
  the renderer's white moonlight ((182, 188, 226)) still shows. The rule for every world
  is being built with the Old World Blues comparison (docs/handoff/owb-comparison-2026-10-23.md);
  a trial that only dimmed (never above 1, relative to the base image space) moved
  Puesta del Sol's median from (10, 0, 0) to (28, 13, 0) against the game's (36, 18, 0).
- **IMAD as a screen effect**: the same record type played by scripts (fade colour,
  blur, double vision, radial blur, every key of every track) is in
  docs/spec-notes/image-modifiers.md; the weather grade above reads only the first
  key of the cinematic tracks and the tint.
- **IMAD cinematic track order** [documented: xEdit wbDefinitionsFNV.pas IMAD names
  `\x11IAD`..`\x14IAD` saturation, contrast, contrast avg lum, brightness; IMGS DNAM
  stores saturation, contrast avg lum, contrast, brightness]. The data fits the IMGS order
  better, 0x12 = contrast avg lum and 0x13 = contrast [hypothesis], and the renderers
  read it so. Evidence [observed, `crates/fnv-world/examples/weather_dump.rs --imad` over
  the ten masters, 299 IMAD and 92 IMGS records]: IMGS contrast lies between 0.8 and 1.9,
  avg lum between -0.06 and 5.0 (0 in 14 records). 59 adapters set 0x13 outright
  (multiply 0, add v) and every v lies in 0.8..1.9. 65 set 0x12 outright; 10 of those set
  it to 0 (among them NVWastelandHazyIS, NVWastelandHazyNoonIS, NVWastelandSunriseHazyIS,
  WastelandDayISFX and FriendOfTheNightInteriorISFX), one
  to 0.05 and one to 0.3. As a contrast, 0 flattens the picture to one grey. The time-0
  multipliers on 0x12 also reach 4.0 (NVWastelandHeavyHazeIS), 6.5 (Dead Money's
  NVDLC01WestTownIS) and 0.2 (NVUrbanNightIS), far outside the contrast range.
- **Freeside and Big MT at noon** [observed 2026-10-08, renderer screenshots at 12:00]:
  both worldspaces use the Mojave's base image space NVDefaultExterior (WRLD INAM
  0x0008809D). NVUrbanClear (NVUrbanClearClimate) links NVWastelandSunriseIS,
  NVWastelandHeavyHazeIS (day and high noon), NVWastelandSunsetIS and NVUrbanNightIS
  (night and midnight). HeavyHaze's time-0 keys equal NVWastelandIS's plus saturation
  ×0.8, 0x12 ×4.0 and 0x13 ×0.8; NVWastelandIS leaves 0x11..0x13 at ×1 + 0.
  NVDLC03BigMTWeather links NVDLC03BigMTSunRiseISFX, -HighNoonISFX (day and high noon),
  -SunsetISFX and -NightISFX, which all multiply 0x12 by 2.0 (saturation ×0.5..0.675,
  tint (0.33, 0.33, 0.72)). Read as contrast, the grade became 4.4 around 0.16 in
  Freeside (after its brightness ×1.43) and 2.2 around 0.2 in Big MT: display values
  above about 0.25 (Freeside) and 0.56 (Big MT) clipped to white, hence the white skies
  and the overexposed, oversaturated Freeside. With `--no-grade` both frames looked normal. Read in the IMGS order, Freeside
  gets avg lum 0.8 and contrast 0.88 (lifted blacks, desaturated: a haze, as the
  adapter's name says) and Big MT avg lum 0.4 and contrast 1.1. The Mojave at noon is
  unchanged; its sunset grade becomes contrast 1.1 around 0.5 (was 1.4 around 0.2) and
  its night contrast 1.1 around 0.24 (was 1.32 around 0.2), slightly darker and flatter.
- **Capture to confirm** (owner, in game, no mods that change image spaces): at 12:00
  (`set GameHour to 12`), screenshots in Freeside (FreesideNorthWorld, cell 2,1, near
  8288, 6896, 1131 facing 220°), in Big MT (NVDLC03BigMT near 5616, -33232, -805
  facing north) and at Goodsprings at 19:00 and 02:00 as references. Expected under this
  hypothesis: Freeside's noon hazy and slightly washed out, never clipped white; Big MT's
  sky blue-grey with clouds; the Goodsprings sunset slightly flatter than a high-contrast
  grade.
- **Lighting and sky captures, 2026-10-09 afternoon** [observed: 66 screenshots of the
  original game by `tools/capture/capture_frames.py` (plugin v7, save `nvftest`,
  NVWastelandClear forced unless named, 1280 × 720, FOV 75), private under
  `C:\fnv-private\l3sky\`; renders of the same cameras by `nvforever`]. Cameras: four views
  at Goodsprings ((−70500, 1200) headings 45 and 225; (−69500, 2500) headings 90 and 270)
  at 07:36, 09:06, 12:06, 15:06, 17:36, 19:06, 20:06, 22:06 and 02:06; the sky cameras of
  `C:\fnv-private\sky\cap\cameras.json` plus the sun at 09:06 and 18:06 and the west sky at
  20:06 and 20:36; Primm and Lake Mead (17:36); Freeside at 12:06. Big MT failed (the
  plugin's BitBlt failed there).
  - *No view-dependent eye adaptation*: with NVWastelandClearNight (no clouds) at 12:06,
    the same sky direction has the same colour in frames pitched 0°, 30°, 60° and 89°
    (25° up: (116, 117, 132) / (119, 119, 133); 45°: (108, 106, 118) / (109, 106, 118);
    75°: (162, 146, 127) twice), although the frames range from mostly ground to all sky
    and sun glow. So the dark noon sky is not exposure following the view; the HDR
    setting (`bDoHighDynamicRange=1` in the owner's FalloutPrefs.ini) does not change the
    exposure with what is in view, or not measurably [observed].
  - *Cloud placement follows the time since the save was loaded*, not the game hour:
    today's trial at 12:06 (22 s after loading) has the same clouds as the reference frame
    goodsprings_noon taken in another session 22 s after loading (mean difference 0.14
    grey levels over the sky), and so does the 07:36 frame taken 22 s after loading
    (correlation 0.89), while the 12:06 frame of the same camera taken 171 s after loading
    differs (−0.13). Between goodsprings_noon (22 s) and goodsprings_dusk (41 s) the
    pattern moves about 70 px right and 26 px down at the top of the 1280 × 720 frame
    (camera heading 45°). The reference frames were taken 22, 41, 59 and 79 s after
    loading (noon, dusk, hill, lake). So the game's cloud pattern in a frame is
    reproducible only with that time, which the scenarios do not carry; the renderer
    scrolls from its own start [observed; matching it needs a "seconds since load" input
    per frame and the game's scroll direction and rate in texture space, not done].
  - *Sun glow*: ring medians around the sun (degrees from its centre): 12:06 looking up
    without clouds (ClearNight) (233, 209, 184) at 6-8°, (190, 170, 155) at 12-16°,
    (141, 130, 124) at 20-26°; with clouds (Clear) white to 8°, (241, 220, 182) at
    12-16°, (168, 153, 134) at 20-26°; 09:06 (sun 36° up) with clouds white to 16°,
    (235, 222, 195) at 20-26°, (180, 169, 158) at 32-40°; 18:06 (sun 20° up) white to
    12°. The larger glow comes with clouds near the sun and grows as the sun gets lower
    [observed]; the mechanism is not known.
  - *West sky at dusk*: at 20:06 looking west the sky 15-35° up is about (60, 52, 62);
    at 20:36 it is (3..10, 4..12, 11..19). With the earlier dusk series (green channel of
    the upper-right region 97 at 19:12, 75 at 19:36, 49 at 20:06, 15 at 20:36), the sky
    fades from sunset to night roughly linearly in display values from about 19:05 to
    20:35 [observed], later than the renderer's night key at sunset end (20:00). Not
    changed: moving the night key or blending in display values changes every weather
    colour and needs its own check. (Moved on 2026-11-27, to 40 minutes after sunset end:
    "Night key beyond sunrise and sunset".)
  - *Light on surfaces* [observed, offline fit: game frames with the grade undone (the
    renderer's grade at that hour) against ambient-only and sun-only renders without
    tonemapper, grade or shadows, 160 × 90, 28 frames 07:36-20:06]: a lighting sum in
    display values, texture × (a·ambient + b·sunlight + c·sunlight·N·L), fits every frame
    better than the same sum in linear light (mean error 0.03-0.06 against 0.04-0.12 per
    frame). Within the renderer's linear model the ambient as now (Bevy's 0.452 included)
    is about right overall and the sun is too strong: the best constant factor on the
    sunlight is 0.6-0.66 (mean error 0.0601 → 0.0547), best per hour about 0.5 at 09:06,
    15:06 and 17:36 but 0.9-1.2 at 07:36, 12:06 and 19:06, so a constant is a compromise
    [hypothesis to test]. 0.66 equals (1 / 1.21)^2.2, the day image space's Sunlight
    Dimmer (NVDefaultExterior HDR 1.1 × NVWastelandIS 0x0B multiplier 1.1) as a divisor of
    display values; the same reading at sunrise (×1.5) and sunset (+0.2,
    NVWastelandSunsetIS `KIAD`) made those frames worse. Faces turned away from the sun
    (boulder at 15:06-17:36, crate at 12:06) get an effective ambient of about (96..107,
    100..110, 106..114) in display values where the weather gives (87, 105, 138): nearly
    neutral. The clearest case is Primm at 12:06: a north-facing wall (sun 78° up in the
    south-east) is brightly and warmly lit in the game and nearly black-blue in the
    renderer, so the game lights faces turned away from a high sun much more than
    texture × ambient (two-sided or wrapped sunlight are candidates). Not changed in the
    renderer yet (no validated change).
  - *Freeside at 12:06* (NVUrbanClear, HeavyHaze adapter): the game's picture is normal,
    slightly hazy, never clipped white; the renderer's (IMGS-order reading of 0x12/0x13)
    is close in tone (frame SSIM 0.575). This confirms the PR #43 reading [observed].
- **Water**: a cell with DATA bit 1 ("has water") has a water surface at XCLW, or at
  the worldspace's default water height (WRLD DNAM) when XCLW is the 3.4e38 sentinel
  [hypothesis]; its type is CELL `XCWT`, else WRLD `NAM2`. Lake Mead cells store 2600
  [observed]. WATR: noise texture `NNAM`, opacity `ANAM`, flags `FNAM` (bit 1
  reflective), visual data `DNAM` (colours shallow/deep/reflection at bytes 48/52/56
  among 49 fields, fog near/far/amount above and under water, fresnel and
  reflectivity amounts, shininess, three noise layers with wind direction, wind speed,
  UV scale and amplitude) [documented: xEdit names].
- **Lake Mead water** [observed]: cells around Boulder Beach (13,9) use XCWT
  NVCleanWater (0x1009CA): ANAM 50, FNAM reflective, shallow (52,75,66), deep
  (22,41,34), reflection (21,32,23), above fog -80 / 850 / 0.75, under fog
  -2500 / 5500 / 1.0, reflectivity 0.6, fresnel 0.75, shininess 500, sun power 826,
  noise normals scale 13.41 and UV scale 1000, layers (direction°, speed, UV scale,
  amplitude) (180, 0.065, 0, 0.30), (10, 0.033, 0, 0.525), (67, 0.029, 0, 0.138).
  Nearly every WastelandNV cell sets "has water" (DATA bit 1); those without XCLW use the
  worldspace default -2300, below the landscape.
- **Noise texture** [observed]: `textures\water\wastelandwaterpotomac.dds` is
  256 × 256 A8R8G8B8, alpha constant 255; R, G and B are uncorrelated (|r| < 0.07 for R
  against G and B; G/B 0.62), each tileable, with feature sizes growing from R to B
  (autocorrelation at a 16-texel lag 0.56 / 0.77 / 0.92). Read as three height layers,
  one per noise layer, R = layer 1 [hypothesis]; it is not a normal map (the RGB
  vectors average length 0.52).
- **How the engine draws water** [hypothesis, the renderer's model; compared with
  in-game frames at Boulder Beach, the Ultra-Luxe fountain and the Goodsprings Source
  trough, #1787]: seen from above, the water body's colour goes from the shallow to the
  deep colour, linearly from the above-fog near to far plane, by how far below the
  surface the opaque surface behind the pixel lies (the near and far planes are "the
  distance in game units below the surface of the water" of the shallow and the deep
  colour plane [documented: GECK wiki WaterType, Fog Properties tab]); its opacity goes
  over the same near-to-far ramp by the length of the view ray inside the water, opaque
  beyond the far plane; for placed water (PWAT) only up to the above fog's amount (Fog
  Amount "controls the transparency of the Deep Color water plane", 0 transparent, 1
  opaque [documented: same page]). Capping the fog at the amount left the lake bed
  visible far out in Lake Mead (NVCleanWater, amount 0.75: about a fifth of the bed
  shows through at any depth), unlike the game, so cell and distant water are not
  capped [design, until a far-out Lake Mead comparison is made (§9 item 3); the
  Ultra-Luxe fountain needs the cap, below, Placed water]. The depth-based colour is
  used for all water (Boulder Beach's near-shore crop moves towards the original with
  it, the table below). Under water both follow
  the distance from the camera (the under fog's planes are distances "from the player",
  its amount "does not appear to have any effect" [documented: same page]). Fogging by
  the distance from the camera made the water opaque right at the shoreline when seen
  from a few metres up (an abrupt clear-to-opaque edge), unlike the game. Until #1787
  the colour followed the view ray too, and the amount was unused: a grazing look into
  a shallow pool then gave the deep colour (below, Placed water). The water reflects the world when reflective
  (otherwise its reflection colour): the renderer mirrors the camera in the water plane
  nearest it, renders what lies above that plane into a texture and samples it at the
  fragment's screen position, offset along the wave-perturbed reflected ray; water at
  other heights reflects the weather's sky gradient. Seen in-game: the dock posts and
  the far hills at Boulder Beach are reflected with a ripple wobble [observed, owner's
  screenshot]. How far the ripples displace the reflection (the DNAM "distortion
  amount", 600 for NVCleanWater, may be it) is unknown; the renderer's offset is tuned by
  eye. The reflection is weighted with weight reflectivity × mix(1, Schlick fresnel,
  fresnel amount); the sun adds a highlight with exponent = shininess. Noise layer i
  repeats every UV scale units (0 → the noise normals UV scale), scrolls at wind speed
  repeats per second towards its compass wind direction, and its height is channel i ×
  amplitude × noise normals scale × 2 (factor tuned by eye). What lies behind the surface
  is refracted: sampled with a screen offset along the wave normal's tilt, growing over
  the first 200 units of water (tuned by eye; the engine's rule is unknown). Under water
  the camera's distance fog becomes the water's under fog (near / far, -2500 / 5500 for
  NVCleanWater) in its deep colour lit by the sun and ambient light; looking up, the sky
  shows only within Snell's window (refractive index 1.33) and the surface reflects the
  water body outside it. The under fog "amount" is unused. Unknown: the roles of ANAM
  opacity, sun power and the four unnamed leading DNAM floats.
- **Planar-reflection camera visibility** [design]: for nearby water at the selected
  plane in the main camera's view, count fragments that pass the opaque-depth check.
  The one-in-16 sample grid rotates through all 16 offsets so it does not always miss
  the same narrow strip. Reset the shared counter before drawing and read it back after
  the draw with the probe generation, source frame and time. A change in world, plane
  height or CPU view eligibility retires the previous probe. Late or out-of-order
  results cannot qualify a new plane or make missing frames count as empty.
  A recent positive sample starts the mirror; after two seconds with no result for the
  probe the CPU view test can start it alone. While active, the mirror survives short
  gaps in coverage. It shuts down after at least four seconds of consecutive zero
  results spanning a full sample cycle, or one second out of the CPU view. No eligible
  plane, hidden water, or a disabled mirror shuts it down immediately. The camera and
  material switch in the same frame, and the mirror draws before the main view.
  In the 2026-11-18 NVForever playtest near SewerMainlineExit and the Potomac, 15 camera
  shutdowns were attributed to the sampled-pixel gate, and two to no nearby water.
  That switches the selected plane between mirror and sky/screen-space reflections;
  it does not establish the cause of individual-object flicker or coplanar artifacts.
  SSR was enabled; the DLSS Quality request fell back to Off (missing runtime files).
  CPU tests deliver authored completion packets to the scheduled observer, with the
  actual water-plane/material producer and camera consumer. They do not execute the
  shader, GPU copy or asynchronous map. Visual confirmation still requires the river
  turnout replay with mirror on/off and SSR held fixed, separate object captures, and
  controls for offscreen/occluded water and active DLSS.
- **Depth falloff** (2026-10-24, ESM audit gap 27): DNAM "Depth Falloff Start" and
  "End", "where the alpha fadeoff begins (used for edges of the water, to blend it
  smoothly with the land)" and "where the alpha fadeoff ends" [documented: GECK wiki
  WaterType; xEdit files them under the noise normals]. The masters store fractions: start
  0 in all but WaterTypeOasisClean (0.029), end 0.0109 for NVCleanWater and its copies,
  0.0797 for the Potomac types, 0.1268 for NVCleanWater02, 0.3152 for WaterTypeBlood, up
  to 0.8 [observed: the ten masters' 74 WATR]. Taken as fractions of the above fog's far
  distance [hypothesis: that gives an edge of a few units to a few tens, NVCleanWater
  0 to 9 units, the Potomac's 0 to 37, the Goodsprings blood pools' 0 to 5; as game units
  they would be hundredths of a unit]: seen from above, the whole surface, reflection
  included, fades in from the start to the end of that ramp of water crossed by the view
  ray, so the reflecting surface meets the shore without a seam
  (`fnv_world::sky::Water::edge_falloff`, water.wesl). The refraction's 200-unit ramp is
  another thing and stays; distortion, light radius and brightness are still unused.
- **Placed water** (2026-11-06, #1780): a placeable water (PWAT) names its own water type
  in DNAM "Water" and is drawn with that WATR, not the cell's or the worldspace's
  [documented: xEdit PWAT]. The Ultra-Luxe fountain (REFR 0x00165AD0, PWAT
  DupontCircle01Water 0x000A1A9D) uses VStripULFountain (0x0016663E): FNAM Reflective,
  shallow (53,79,49), deep (10,33,24), reflection (58,116,95), reflectivity 1.0, fresnel
  0.23, above fog 0 / 36 / 1.0, noise normals scale 0.7, layer UV scales 100 [observed:
  FalloutNV.esm]. Its PWAT DNAM flags are Reflects - Land, Reflects - Sky, the refracts
  bits and Depth, but not bit 0 "Reflects" [observed]. In the original, from the
  observer-checks item 87 spot at noon, the pool is an even, dull olive green
  (about sRGB 61,77,39 across the surface) with no reflection of the obelisk, the
  buildings or the sky and no visible waves [observed: capture `ul_fountain`,
  2026-11-06]; drawn with the type's reflection weight (1.0 × mix(1, Schlick, 0.23), at
  least 0.77) it was the pale sky (about 170,190,193). Rule [hypothesis]: PWAT bit 0
  "Reflects" switches the whole reflection; a placed water without it shows only its
  water body (fogged shallow to deep colour, lit; refraction where it is thin), with no
  world, sky or reflection colour and no sun highlight, whatever its WATR's
  reflectivity and the other "Reflects - …" bits say (the GECK dialog's "Reflects" group
  "determines which objects can be reflected in this water object" [documented: GECK
  wiki PlaceableWater]; what each sub-bit adds is not documented and unused). Cell water
  has no such flags and always uses its type's reflection settings. In the masters 3 of
  the 29 PWAT lack bit 0: the fountain's, NVCleanWater1x402 (NVCleanWaterGS; the
  Goodsprings Source troughs, cells -16,-5 and -16,-4) and NVCleanWater1x1
  (NVCleanWater03) [observed]. The Goodsprings Source trough (REFR 0x000E38C7) in the
  original at noon shows no reflection either, not of the sky nor of the pipe above it,
  only a dim grey-olive body (about sRGB 70,69,54) [observed: capture `gs_source`,
  2026-11-06], which fits the rule. Since the change both are drawn without reflection.
  Still different, and not this rule's: the body colour. The fountain is darker in ours
  (about 17,40,21, the deep colour lit, against the original's 61,77,39, about the
  shallow colour lit) and the trough bluer (about 70,82,83, NVCleanWaterGS's shallow
  colour 91,102,145 through thin fog). The view ray reaches the fountain's far distance
  (36 units) at a grazing look into a pool about 14 units deep [observed: the fountain
  model's pool floor disc ULFountainCC at model z 68, scale 0.5, so z 1046 under the
  water at 1060]. Settled in #1787 (the measures in "How the engine draws water"
  above). Mean sRGB of one crop of the water per place, at noon, original against the
  renderer [observed, 2026-11-07; the renderer's camera is not exactly the original's]:

  | place | original | ray for both (before) | depth for both | depth colour, ray × amount opacity (as built) |
  |---|---|---|---|---|
  | Ultra-Luxe fountain (fog 0 / 36 / 1.0) | 64,80,41 | 36,52,27 | 173,156,107 | 67,87,41 |
  | Goodsprings Source trough (7 / 58 / 0.72) | 66,65,54 | 70,85,89 | 73,78,83 | 90,96,110 |
  | Boulder Beach, near water (-80 / 850 / 0.75) | 101,99,82 | 77,86,79 | 91,97,85 | 93,101,88 |

  By depth alone the fountain's pale floor disc showed through (alpha 14 / 36), where the
  original is an even opaque olive. As built the fountain matches; Boulder Beach (its
  crop measured with the amount cap; cell water is now drawn without it, above) moves
  towards the original (its bed shows further out, as in the original's frame
  `lake_mead`, cell 13,9 looking east). The trough is still bluer and lighter than the
  original's dim grey-olive body (with a pale haze along its inside walls); what makes
  it dimmer is open (the body's lighting, or ANAM opacity 84, are candidates
  [hypothesis]).
- **Ripples** [hypothesis, the renderer's model]: DNAM's "rain" and "displacement"
  groups (starting size, force, velocity, falloff, dampner; NVCleanWater: rain 0.05 /
  0.1 / 0.6 / 0.985 / 2.0, displacement 0.01 / 0.4 / 0.6 / 0.985 / 10.0) are read as two
  damped wave simulations: force = how far a drop or a moving body pushes the surface
  down, velocity = the wave-equation coefficient (kept at the 2-D stability limit 0.5),
  falloff = damping per step (60 steps a second). Starting size and dampner are unused
  (unknown). Rain drops fall while the weather is "Rainy"; a body (the player's feet,
  or the free camera) near the surface leaves a wake and splashes when it crosses it.
  Grid (256 × 256 cells of 4 units around the camera), drop and wake radii, rain rate and
  ripple height are tuned by eye; the game's own simulation is not known.
- **Rain** [observed]: WTHR DATA flags bit 2 "Rainy" is set on exactly two weathers in
  the masters, Honest Hearts' NVDLC02ZVRainWeather and NVDLC02TheNarrowsRainWeather (none
  in FalloutNV.esm, Dead Money, Old World Blues, Lonesome Road; no weather sets bit 3
  "Snow"). The weather record names no rain texture or model: the falling rain is
  content. Four activators (NVDLC02Rain, NVDLC02Rain2, NVDLC02RainUp, NVDLC02RainUp2,
  models `meshes\nvdlc02\sky\nvdlc02rain*.nif`) are driven by the quest
  NVDLC02RainQuest's script NVDLC02rainscript. In plain terms, every frame while it
  rains and the player is outdoors and above a height of 4878 units (Zion Canyon's
  default water height is 5000), the script moves two rain references onto the player
  and enables them (one at an offset given by a global), switches to the "up" variants
  while the player looks steeply up or down, keeps them going for a few seconds after
  the rain stops, and toggles the rain ambience sound and an interior rain-sound marker.
  So drawing Zion rain needs the script engine (Layer 4); the renderer does not draw it.

### Weather transitions and glare (ESM audit group 7, 2026-10-23)

- **Trans Delta** (WTHR DATA byte 3) [documented: GECK wiki SetWeather, "the transition
  time is effected by the Trans Delta setting of the weather"; SetWeatherTransitionTimeOverride,
  the Trans Delta "effectively limits the transition time to 30 seconds max", real-time
  seconds]. Values [observed]: every New Vegas weather 255; Fallout 3 leftovers 0 (19),
  87, 122. As built [hypothesis: the time grows with the byte]: a change to a weather
  fades over 30 × delta ÷ 255 real seconds (255: the documented 30 s; 0: at once), as
  game hours at the default TimeScale 30 [documented: GECK wiki TimeScale]. It replaces
  the placeholder half game hour.
- **Sun Glare** (byte 4): every New Vegas weather 54; others 0, 127, 176, 255
  [observed]. The glare's strength was fitted on New Vegas skies, so it is scaled by
  Sun Glare ÷ 54 [hypothesis], blended across a weather fade.

### Interior clip distance (ESM audit group 7, 2026-10-23)

- The interior's Fog Clip Distance (CELL XCLL, or its lighting template's) is its far
  clip distance [documented: GECK wiki SetClipDist, "overriding far clip distance in
  interior cells"; GetInteriorLightingTraitNumeric trait 15 "Fog clip distance"]. Values
  in 388 interiors [observed]: 6,600 (73), 0 (43), 1,800 (42), 4,000 (39), 3,300 (37);
  in 55 it is nearer than the fog's end (WinsResidence 1,800 against 3,300). As built:
  indoors the camera's far plane is the clip distance, 0 for none [hypothesis: 0 is no
  clip]; outdoors 400,000 units as before.
- **Interior unset fog ranges** [observed, Fallout3.esm and FalloutNV.esm; #3038]: some
  interiors store a fog range whose far is not beyond its clamped near
  (`fog_far <= fog_near.max(0.0)`, most commonly `0..0` with black fog colour): 57 of 420
  interiors in `Fallout3.esm` (48 without a lighting template, including `zGrandmaSparkle`,
  and 9 whose `LTMP` also stores an unset range) and 9 of 388 in `FalloutNV.esm` (7 without
  a template, 2 via `LTMP`). Cells such as `GermPoliceHQ01` store `0..0` in raw `XCLL` but
  inherit a non-zero fog range (`64..6500`) from their `LTMP` (`NeoclassicalDefaultTemplateDC`).
  Taken literally through `fog::falloff`'s `far.max(near + 1.0)` clamp, an opaque `0..0`
  black fog would draw the room solid black past 1 game unit; matching exterior weathers
  (*Unset fog ranges* above), `nvforever/src/sky.rs::apply_cell_fog` treats
  `fog_far <= fog_near.max(0.0)` as no fog (`alpha = 0`, `NO_FOG_FAR`) [hypothesis].
- **Fog power** (XCLL, LGTM, WTHR) varies (1.0 in 189 interiors, 0.5–2.0 in the rest;
  0.2–1.0 in the masters' 98 weathers, the Mojave's 0.6 by day and 0.5 by night, Dead
  Money's 0.21–0.5 [observed, `weather_dump.rs --fog`]). What curve it sets is not
  documented. As built since 2026-10-23 [hypothesis, fitted to the frames below]: the
  fog's share at a distance d is ((d − near) / (far − near))^power, clamped to 0..1, so
  a power under 1 fogs near things more than a straight line. Outdoors the fog colour is
  blended in display values (gamma 2.2), indoors in linear light. Every world shader
  reaches it through Bevy's fog function, which the renderer patches
  (`nvforever/src/fog.rs`); the weather's fog colour, distances and fades are as before.
  Evidence (original frames against renders of the same cameras; SSIM as the lab's
  frame scenarios score it):
  - Dead Money's Villa (NVDLC01VillaWeather 0..75,000, power 0.35, fog (119, 46, 28)):
    in the original the far buildings read through a red haze ((50, 23, 22) on a facade
    about 2,000 units away; the mountains (56, 19, 11), the colour of the sky just above
    them). Straight-line fog leaves 3 % of fog there and the facades black ((10, 0, 0));
    the power curve mixed in linear light washes the picture in orange ((75, 16, 5),
    mountains (104, 29, 12)); mixed in display values the facades are (35, 2, 2) and the
    picture reads like the original's. Worked through by hand in display values
    (texture × ambient, then fog, then the grade) the curve predicts (54, 15, 19) for that
    facade and (35, 7, 19) for a wall 200 units away (game (24, 5, 12)).
  - The lab's Mojave frames: the power curve mixed in linear light lowered every exterior
    frame (spring_mountains_east 0.703 → 0.625, black_mountain_north 0.605 → 0.584, under
    the 0.6 threshold); mixed in display values they score as before or better
    (spring_mountains_east 0.726, goodsprings_dusk 0.508 → 0.514, lake_shore_noon 0.515 →
    0.524, black_mountain_north 0.605). In the original the land in the middle distance
    is clear and the haze gathers at the horizon, as this curve does.
  - Interiors: the power curve in linear light left Doc Mitchell's house better (bedroom
    0.713 → 0.758) and the saloon as it was (0.811 → 0.807); in display values the
    saloon fell to 0.715-0.663, its dark corners losing a faint neutral veil the original
    has ((14, 12, 12) in the game, (5, 4, 5) in display values, (13, 11, 16) before). So
    indoors the blend stays linear. Why the two differ is not known.
  - Not checked: the fog clip distance's interplay, Honest Hearts' and the other add-ons'
    worldspaces (Honest Hearts' archive is a lab holdout), the underwater fog (still
    linear, water.md).

### Under-water colour (#2916, 2026-11-18)

Code: `engine/nvforever/src/underwater.rs` (`fog_bytes`, `DEEP_AT`).

- **The original** [observed: own captures, `tools/capture/capture_frames.py`, god mode,
  the player set in place under water; mean display colour of the whole 1280×720 frame]:
  - Lake Mead (WastelandNV cell (28, 16), surface 2,600, bed 1,272; water Potomac
    0x00030009: shallow (52, 75, 52), deep (27, 39, 24), under fog −1,000..700), clear
    weather, noon, by the player's feet below the surface:

    | Feet down | 150 | 300 | 500 | 700 | 900 | 1,100 | 1,250 |
    |---|---|---|---|---|---|---|---|
    | Frame | 72, 81, 38 | 67, 75, 34 | 60, 68, 31 | 53, 60, 26 | 45, 51, 21 | 37, 43, 16 | 36, 42, 15 |

    At 50 down the camera is above the water. The fall is even (about 3.8 levels of green
    per 100 units) and stops near 1,100.
  - The frame is one colour whichever way the camera looks at 300 down: level (67, 76,
    34), 45° up (67, 75, 35), 45° down (68, 77, 35).
  - By the hour at 300 down: 06:00 (42, 48, 34), 09:00, 12:00, 15:00 and 18:00 all
    (67, 75, 34), 20:00 (38, 43, 29), 22:00 and 00:00 (32, 42, 37). Midnight by depth:
    150 (35, 46, 40), 500 (29, 38, 33), 900 (20, 27, 23), 1,250 (16, 22, 17).
  - The Pitt's river (Tale of Two Wastelands, DLC01PittWorld cell (0, 1), surface −2,100;
    water DLC01PittWorldWater: shallow (56, 60, 34), deep (17, 17, 4), the same fog), feet
    180 down, weather DLC01PittWthrBridge01 at noon: (46, 53, 33).
- **As built** [hypothesis, fitted to the above]: the camera's fog under water is the
  water type's shallow colour at the surface and its deep colour 1,000 units down, in
  proportion between, as display values; it is not lit. The picture's grade then gives
  the day and night levels. Ours for the same views: Lake Mead at noon 150 / 500 / 900 /
  1,250 down (74, 83) / (61, 69) / (47, 52) / (37, 42) in red and green, midnight 150
  (36, 47) and 900 (21, 28); the Pitt (49, 55, 34).
- **Before**: the deep colour times the light on a sunlit surface, which in the Pitt's
  dim weather came out nearly black ((15, 20, 10) against the original's (46, 53, 33)):
  #2916's "The Pitt's bridge is black". The sweep's camera there was under the river.
- **Left**: (a) looking up from just below the surface the original shows the sky and
  clouds faintly, near the water's colour ((67, 70, 47) at the top of the Pitt frame);
  ours shows the sky brightly ((128, 133, 116)): the sky is not fogged by the camera's
  fog. (b) Settled below ("Night key beyond sunrise and sunset"). (c) Whether 1,000 is
  the water's under fog near or a constant: both waters measured store −1,000.

### Night key beyond sunrise and sunset (#2916, 2026-11-27)

Code: `crates/fnv-world/src/sky.rs` `time_keys`, `NIGHT_KEY_BEYOND`.

- **The original** [observed: observer job o35_uw_dawn, the Lake Mead spot above (cell
  (28, 16), feet 300 below the 2,600 surface), NVWastelandClear forced, mean display colour
  of the whole 1280×720 frame, the clock 0.04 h past each hour; NVDefaultClimate's TNAM:
  sunrise 6:00-8:00, sunset 18:00-20:00 (36, 48, 108, 120 in tens of minutes)]:

  | Hour | 04:30-05:00 | 05:30 | 06:00 | 06:30 | 07:00 | 07:30 | 08:00 |
  |---|---|---|---|---|---|---|---|
  | Under water | 32, 42, 37 | 32, 42, 36 | 41, 48, 34 | 48, 52, 33 | 54, 59, 34 | 60, 66, 35 | 67, 76, 34 |
  | Sky, 45° up above the surface | 5, 6, 11 | 9, 10, 17 | 66, 60, 77 | 101, 86, 110 | 117, 103, 123 | 121, 112, 119 | 128, 124, 122 |

  | Hour | 18:00 | 18:30 | 19:00 | 19:30 | 20:00 | 20:30 | 21:00 |
  |---|---|---|---|---|---|---|---|
  | Under water | 66, 75, 34 | 58, 62, 28 | 48, 48, 24 | 43, 44, 22 | 39, 43, 28 | 32, 42, 37 | 32, 42, 37 |
  | Sky | 128, 124, 121 | 128, 113, 112 | 123, 100, 101 | 108, 85, 88 | 59, 51, 59 | 7, 8, 14 | 6, 7, 13 |

  The under-water red rises evenly from about 5:20 to 8:00 and falls back to night by
  20:30. The dusk sky series further up ("West sky at dusk") ends near 20:35 too.
- **As built** [hypothesis, fitted]: the night key lies a third of the transition's length
  outside it, 40 minutes for the Mojave's two hours: night until 5:20, the sunrise
  colours at 7:00, day from 8:00; day until 18:00, the sunset colours at 19:00, night from
  20:40. It moves every keyed colour and adapter (weather colours, the grade, the sun
  dimmer's blend), not only the water's. Before, night held until 6:00 and came back at
  20:00.
- **Ours, the same spot** (explorer camera 180 units down, where the original's eye was;
  ours reads about +5 per channel at night and at noon alike, from the camera depth):
  05:00 (37, 47, 42), 05:30 (39, 48, 41), 06:00 (46, 53, 39), 06:30 (51, 56, 38),
  07:00 (56, 60, 39), 07:30 (64, 69, 39), 08:00 (72, 81, 38); 18:00 (71, 79, 38),
  18:30 (61, 65, 32), 19:00 (50, 49, 25), 19:30 (48, 49, 28), 20:00 (44, 48, 33),
  20:30 (39, 47, 39), 21:00 (37, 47, 42). With the 5 taken off, every hour is within 4 of
  the original. Before the change 06:00 and 20:00 were both full night.
- Not checked: other climates' transitions (the third is fitted to 2-hour ones only), the
  sky colours by the hour against the table above, and the parity lab's
  `L3.frame.goodsprings_dusk` frame (19:06).

### Sunlight Dimmer (Old World Blues comparison F1, 2026-10-23)

- **Where it is** [documented: xEdit wbDefinitionsFNV.pas IMGS DNAM, IMAD]: the twelfth
  HDR float of IMGS DNAM (byte 44, in front of the size-dependent part, so the same in the
  152-, 148- and 132-byte layouts), "Sunlight Dimmer"; each IMAD multiplies and adds to it
  with track `\x0bIAD` (multiply) and `KIAD` (0x4B, add). Read with the other HDR values
  ("HDR and bloom" above; `Hdr::v[HDR_SUNLIGHT_DIMMER]`), at the time-0 key like the
  cinematic tracks.
- **Values** [observed: fnvtool esm decode; base × multiply + add]: NVDefaultExterior 1.1.
  The Mojave's clear weathers (NVWastelandClear, ClearNight, GS, Borders, UrbanClear,
  BlackMountain, ClearWindy) give 1.65 at sunrise, 1.21 by day, 1.3 at sunset, 1.1 at
  night. Others: NVWastelandHazy 0.75 / 1.0 / 1.3 / 1.1; NVTheStripWeather night 0.6;
  NVHooverDamWeather 0.77 by day; NVSearchlightWeather 0.44 by day; NVWastelandClearEast
  1.21 from sunrise to sunset. Big MT (NVDLC03BigMTWeather) 3.3 / 2.475 / 2.2 / 2.2.
  Dead Money: NVDLC01VillaWeather 1.1 at every hour, NVDLC01WestTownWeather 0.54 (×0.4 + 0.1),
  NVDLC01EastTownWeather and DefaultWeather 1.3 (+0.2), each on NVDefaultExterior (the
  Villa's INAM; the other Dead Money worlds not checked); NVDLC01FogWeather and
  VistaWeather link Fallout 3's Pitt adapters (`DLC01Pitt01*ISFX`), which set it outright
  (multiply 0, add 2.1 by day, 2.3 at night, 2.5 at sunset).
- **What it does** [hypothesis: the dimmer scales the directional (sun and moon) light
  directly]. Evidence [observed, OWB comparison `docs/handoff/owb-comparison-2026-10-23.md`]:
  in the original the Big MT's directly lit surfaces are about twice the brightness of
  ours (Think Tank ground 162 against 82 grey), and its dimmer is about twice the Mojave's;
  a test build with the sun × 2.05 moved that ground to 112 and a night ground from 18
  to 31 (original 26). The earlier Mojave fit above read the noon factor 0.66 as the day
  dimmer used as a divisor; at sunrise and sunset that reading made frames worse, and the
  Big MT rules it out (a divisor would halve the Big MT's light).
- **As built**: the outdoor directional light is × dimmer(hour) ÷ the Mojave reference
  (hour), where dimmer(hour) is the image space's value with the weather's adapters,
  blended between the four times of day like the grade (and across a weather fade), and
  the reference is the Mojave clear weathers' 1.65 / 1.21 / 1.3 / 1.1 blended the same
  way (`fnv_world::imagespace::sun_scale`). The Mojave's light was fitted under its clear
  weathers, so they give exactly 1 at every hour and their frames do not change; every
  other weather and world is lit relative to them, by one rule. Interiors keep XCLL's
  light unchanged (their image spaces' dimmers are not checked yet). The log's
  `sky: ...` line ends with `sun x<scale>`.
- **Checked** [observed, 1280 × 720 renders of the OWB comparison cameras against the
  original's, medians of 8-bit regions]: Big MT ×2.05 by day, ×2.0 at sunrise and night,
  ×1.7 at sunset. Think Tank by day frame mean 63.8 → 72.0 (original 72.1); X-8 26.7 →
  35.0 (31.6); X-12 at sunset 54.9 → 60.9 (76.3); Little Yangtze sunrise 78.5 → 82.9
  (103.0); Think Tank night ground (11, 12, 22) → (21, 23, 32) (original (22, 20, 35)).
  Z-38 at night overshoots (25.0 → 37.4, original 22.7): its east-facing facades are lit,
  which the original leaves dark (moonlight direction, OWB F5). Mojave Goodsprings in
  NVWastelandClear at 12:06 and 19:06: ×1.00, region medians unchanged. Dead Money: the
  Villa ×1.00 at night (unchanged); Puesta del Sol ×0.49 at night takes out some of the
  greenish-white moonlight on the lantern wall, which the original does not show, and
  the frame mean goes 24.3 → 21.9 (original 38.4, lit by warm ambient and the lantern:
  a separate gap).
- **Open**: whether the dimmer's own change through the day also applies in the Mojave
  (the reference would then be the day value 1.21 alone: sunrise × 1.36, night × 0.91).
  The Mojave fit's per-hour sun factors (0.9-1.2 at 07:36 against 0.5 at 09:06) lean that
  way but are noisy; not changed without a check against Mojave sunrise frames.

### Live session `IMGS` traits, `SetCellImageSpace`, exterior `WTHR` fog, and GMSTs

- **Image spaces (`SetImageSpaceTrait`, `SetCellImageSpace`)** [documented: GECK wiki
  `GetImageSpaceTrait` / `SetImageSpaceTrait` traits `0..=32`, `SetCellImageSpace` notes;
  hypothesis: live in-place refresh for `SetImageSpaceTrait`]:
  - `fnv_world::imagespace::apply_image_space_traits` overlays session `SetImageSpaceTrait`
    values onto an `IMGS` record's `Cinematic` grade: HDR (`0..=14`), Bloom (`15..=17`),
    and Cinematic (`25..=32`, converting tint RGB traits `29..=31` from 0–255 to 0–1).
    Traits `18..=24` (`GetHit` / `NightEye`) remain stored in `Game::traits` for script
    readback (`GetImageSpaceTrait`) and have no rendered `Cinematic` field.
  - `SetCellImageSpace` preserves the original game's documented next-load behaviour
    [documented: GECK wiki `SetCellImageSpace`, notes]: `nvforever::world::cell_image_space`
    binds an interior cell's active `IMGS` FormID (`SetCellImageSpace` override, else
    authored `CELL XCIM`) only when the cell loads (on travel, cell entry, or when the
    script runtime with a loaded save's changes becomes ready). Calling
    `SetCellImageSpace` on the cell the player is already in does not switch the active
    grade until the cell next loads.
  - Live `SetImageSpaceTrait` refresh on the already-loaded active `IMGS` (interior bound
    `IMGS` or exterior `WRLD INAM`) and live `SetWorldspaceImagespace` (`WRLD INAM/`,
    following `WNAM` + `PNAM` Use Image Space Data inheritance) are keyed on
    `(game_instance_id, image_space_revision)` from `Game::image_space_sync_token()` —
    doing zero work on unchanged frames while refreshing `SkyState::interior_grade` and
    `SkyState::image_space` in place without re-reading pending `SetCellImageSpace`
    assignments [hypothesis: not yet independently verified by an original-game capture].
    Outdoors, explicit `SetCellImageSpace` assignments on the current exterior cell bind
    on cell load ahead of `SetWorldspaceImagespace` / `WRLD INAM` while leaving unmodded
    exterior cells unchanged, and `worldspace::script_sky` preserves active worldspace /
    exterior image-space overrides across same-worldspace climate rebuilds [design].
- **Exterior weather fog (`SetWeatherTraitNumeric` traits `15..=20`)** [documented: GECK
  wiki `GetWeatherTraitNumeric` / `SetWeatherTraitNumeric` traits `15..=20` (day/night
  near, far, power); hypothesis: live in-place refresh on active weather]:
  - `nvforever::sky::sync_weather_fog` syncs session fog overrides from `Game::traits`
    into `SkyState::weather_fog_overrides` keyed on `(game_instance_id,
    weather_fog_revision)` (zero work on unchanged frames) and calls `SkyState::refresh()`.
  - `SkyState::fog()` and `SkyState::fog_power()` evaluate `Weather::fog_at_with` and
    `Weather::fog_power_at_with` in place (zero per-frame allocations) across both the
    current and fading-out weather. Explicit console/CLI `SkyState::fog_override` retains
    precedence.
- **Exterior weather colours, image-space adapters, and transition time
  (`SetWeatherRGBColor`, `SetWeatherImageSpaceMod`, `SetWeatherTransitionTimeOverride`)**
  [documented: GECK wiki `GetWeatherRGBColor` / `SetWeatherRGBColor` (`NAM0` colour types
  `0..=9` and `PNAM` cloud layers `0..=3` across times `0..=5`), `GetWeatherImageSpaceMod`
  / `SetWeatherImageSpaceMod` (`0IAD..5IAD`), and `SetWeatherTransitionTimeOverride`;
  hypothesis: live in-place refresh on active weather]:
  - `nvforever::sky::sync_weather_visuals` syncs session colour, cloud-colour, weather
    `IMAD` adapter, and transition-time overrides from `Game` into
    `SkyState::weather_visual_overrides` keyed on `(game_instance_id,
    weather_visual_revision)` (`Game::weather_visual_sync_token()`), doing zero work on
    unchanged frames and clearing cleanly on `Game` restore or runtime drop [design].
  - `SkyState::color`, `SkyState::color_bytes`, `SkyState::cloud_color_for`,
    `SkyState::cinematic`, `SkyState::sun_scale`, and `SkyState::advance_fade` apply
    these overrides in place (zero per-frame allocations) across both current and
    fading-out weathers, reaching `DistanceFog`, `GlobalAmbientLight`, `DirectionalLight`,
    `ClearColor`, `SkyDome`, `CloudLayer`, `GetCurrentSkyColor`, and `ImageSpaceGrade`.
    An explicit `SetWeatherImageSpaceMod weather time 0` clears that time slot's adapter
    rather than falling back to the authored ESM record [design].
- **Game settings (`SetNumericGameSetting`, `SetStringSetting`)**:
  - Vanilla numeric (`f*`, `i*`) and string (`s*`) GMST edits write directly to
    `fnv_sim::World::setting_changes` and are visible live to `World::setting_f32`,
    `World::setting_value`, and script getters. Non-vanilla shader/injector settings
    authored for third-party D3D9 hooks (such as New Vegas Reloaded) are not ESM `GMST`
    records and remain outside NVForever's clean-room charter; `SetNumericGameSetting`
    returns `0` cleanly for unknown names.

## 7. Distant LOD

Pre-built LOD ships per worldspace [observed: `Fallout - Meshes.bsa`, `Fallout -
Textures2.bsa`, `Fallout - Misc.bsa`, DLC archives]:

- **Terrain**: `meshes\landscape\lod\<world>\<world>.level<L>.x<X>.y<Y>.nif` for levels
  L = 4, 8, 16, 32; a chunk covers L × L cells starting at cell (X, Y), with X and Y
  multiples of L. WastelandNV: 1,024 / 256 / 64 / 16 chunks covering cells −64..63 in
  both axes. Each chunk is a BSMultiBoundNode translated to the chunk's south-west corner
  (e.g. x = −20 → −81,920 = −20 × 4096) holding one NiTriShape (about 850 vertices at
  level 4) with a BSShaderPPLightingProperty whose texture set names the chunk's own
  diffuse and normal textures,
  `textures\landscape\lod\<world>\{diffuse,normals}\<world>.n.level<L>.x<X>.y<Y>.dds`
  (1,360 of each, one per chunk). The geometry also
  carries an NiAdditionalGeometryData stream of one f32 per vertex (meaning unknown;
  in level4.x12.y8 it equals the vertex's z for all 1,299 vertices [observed]).
  Level-4 textures are 256 × 256 DXT1 with mips, i.e. 64 texels per cell side
  [observed: x12.y8 diffuse and normals]. In level4.x12.y8 [observed]: the terrain
  NiTriShapeData has no vertex normals or tangents; its UVs are u = 1 − x / 16,384,
  v = y / 16,384 in chunk-local units (all 1,299 vertices), so image row 0 is the
  south edge and column 0 the east edge; the normal map is in model space, red = +x
  (east), green = +y (north), blue = +z (up) — the slope triangles' face normals agree
  with it on average at 0.71 (dot product) against at most 0.52 for any other
  sign or axis-swap convention. A second shape (BSSegmentedTriShape, 682 vertices, no
  properties, no UVs, flat at z = 2,600, the Lake Mead water height) follows the
  terrain shape [hypothesis: a water LOD surface].
- **Objects**: `...\<world>\blocks\<world>.level4.x<X>.y<Y>.nif` (289 for WastelandNV,
  plus 6 `.high.` variants), merged geometry of distant buildings and landmarks over the
  same 4 × 4-cell squares, textured from one atlas `...\blocks\<world>.buildings.dds`
  (+ `_n`). Separately, 692 `*_lod.nif` meshes sit next to their full models.
  Block geometry [observed: blocks x−16.y0, x−12.y0, x−16.y−4]: one BSSegmentedTriShape
  with vertex normals, tangents and bitangents, translated to the square's south-west
  corner (the root node is not), clamp mode 0; its segments split the triangle list by
  cell, segment i holding cell (i / 4, i mod 4) of the square (x-major): in 39 of the 40
  non-empty segments that cell holds most of the segment's triangle centres, 83% of all
  triangles (the rest belong to objects reaching into the next cell). A block has up to
  16 segments (x−12.y0: 14, 8 of them empty).
  Segments over all blocks [observed: `crates/fnv-nif/examples/lod_segment_survey.rs`
  over `Fallout - Meshes.bsa`]: each of the 355 block shapes is a BSSegmentedTriShape
  with 1 to 16 segments (216 have 16), 5,009 in all, every one with flags 0. In every
  shape the segments tile the triangle list in order: a non-empty segment's start index
  is three times the triangles before it (start index counts indices, the count
  triangles [documented: nif.xml BSGeometrySegmentData]) and together they cover the
  list. Of the 3,577 non-empty segments 3,469 have most of their triangle centres in
  cell (i / 4, i mod 4); 393,732 of the 2,059,770 triangles (19%) have their centre in
  another cell. No vertex is used by two segments, and of the 544,846 connected pieces
  of geometry (triangles sharing a vertex index) 23,121 reach over more than one cell
  and none over more than one segment: a segment holds whole objects, not the
  triangles inside a cell's square. Block x−20.y0, segment 1 (cell (−20, 1)): 345 of
  its 464 triangle centres lie in that cell, 74 in cell (−19, 1) and 45 in the cells
  north of the two; cell (−20, 1) holds two cliffs flagged visible when distant
  (NVNCliffCanyonL01 at x −78,499 and CliffVertiA2 at x −78,385 [observed: `fnvtool
  world refs`]), 675 and 561 units west of the edge to cell (−19, 1) at x −77,824. So
  a segment reads as the LOD of the references placed in its cell [hypothesis: only
  this segment was compared with the references], and that the game hides a loaded
  cell's segment is [hypothesis]. The renderer splits blocks by segment (lod.rs).
  Splitting by triangle centre instead cut those cliffs in two with cells (−19, −3) to
  (−15, 1) loaded: the triangles in cell (−19, 1) went, the full models of cell
  (−20, 1) were not drawn, and the peak north-west of Goodsprings showed holes where
  the game shows it whole [observed: owner's frames from −66,426, −3,568, 8,738,
  2026-10-10].
  Block alpha [observed: `crates/fnv-nif/examples/lod_alpha_survey.rs` over `Fallout -
  Meshes.bsa`]: all 355 block shapes (295 of them WastelandNV's) have a
  BSShaderPPLightingProperty with BSShaderFlags2 bit 2 "LOD_Building" [documented:
  nif.xml] and no NiAlphaProperty; no other shape in the archive sets that bit (the 2,308
  textured terrain LOD shapes set bit 1 "LOD_Landscape"). Yet the atlas carries cut-outs:
  `wastelandnv.buildings.dds` is 2,048 × 1,024 DXT3 with 12 mips, 14.1% of its texels at
  alpha 0 (near black in colour: Joshua trees, palms, pylons, the water tower's legs,
  railings), 82.4% at 255 and 3.5% between. Over the 289 WastelandNV blocks, 220,916 of
  1,704,051 triangles sample a texel below alpha 128 and only 617 lie wholly in such
  texels (28 samples per triangle; `fnvtool nif decode` of each block against the atlas's
  top mip), so the alpha is a cut-out mask for geometry made for
  it, not data left over from opaque sources. Block x−16.y−4's first trees are crossed
  planes with a quad facing each way (four per tree, back-to-back pairs about 2 units
  apart), so they need no two-sided drawing. The `*_lod.nif` meshes set neither LOD bit
  and state their alpha themselves: of 758 visible shapes 612 have no NiAlphaProperty, 128
  only test (TEST_GREATER; thresholds 128: 45, 0: 28, 160: 15, 70: 13, 64: 9, eight others
  at most 4 each; `watertower01_lod.nif`: 127), 14 only blend and 4 do both. That the game
  alpha-tests the blocks is [hypothesis: the owner's frame at the Goodsprings sign shows
  the tower's legs as a lattice and trees on the hill, where drawing the block opaque gave
  a solid tower base and black boxes]; its cutoff is not known, the renderers use alpha
  above 128 (`engine/nvf-assets/src/nif_spawn.rs`, `shape_alpha_mode`).
- **What goes into the object blocks** (#3014): the editor's Generate Objects makes LOD
  "for all refs in the world that are marked visible when distant", from LOD meshes that
  sit in the full mesh's folder and end in `_lod.nif`; a base's "Visible When Distant"
  flag adds its references "if a corresponding LOD model exists" [documented: GECK wiki
  "World LOD", LOD Object Generation; "Static", Flags]. The flag is header bit 15 (32768)
  on the base record and on the reference (§2) [documented: GECK wiki "SetFlagsLow",
  "GetReferenceFlag"]. Measured from the data in a read-only study, 2026-11-18, to be
  measured again [observed]: all 692 base-game `*_lod.nif` sit beside their full models,
  with low-resolution textures of their own, a median 219 triangles against 4,968 for the
  full model; community LOD packs add about 650 more in the same naming, and one pack
  names them `_lod_0.nif` / `_lod_1.nif` with no `_lod` sibling. In WastelandNV 15,333
  references carry the reference flag, every one with a `_lod` model for its base; 1,716
  more have a flagged base without the reference flag. A widely used LOD plugin flags 835
  more base records, which puts about 74,600 unflagged references under a flagged base,
  and the blocks it regenerates hold them. So whether a block may hold a reference
  depends on the reference's flag and its base's, in any plugin's version of either, and
  on the load order the blocks were made for. Unflagged STAT, SCOL and MSTT references
  whose bound radius is at least 0.01 of their distance: about 1,200-2,000 (130-260
  distinct models) in the cells 3 to 8 steps around Goodsprings, Primm and Novac,
  1,900-2,700 out to 12 steps; at 0.004 about 2.5 to 3 times as many.
- **Settings**: `lodsettings\<world>.dlodsettings`, 24 bytes. For WastelandNV:
  u32 4, u32 32, u32 128, i16 −64, i16 −64, i16 63, i16 63, u32 4 [observed], read as
  smallest and largest level, grid size in cells, south-west and north-east cells, and
  object LOD level [hypothesis: matches the files present].
- **Coarser levels** [observed: level4.x12.y8, level8.x8.y8, level16.x0.y0,
  level32.x0.y0 and level32.x−32.y−32 decoded]: every level is laid out like level 4 —
  the root translated to the chunk's south-west corner (level32.x−32.y−32 → −131,072,
  −131,072), one textured NiTriShape, UVs u = 1 − x / side, v = y / side for every
  vertex, the extra f32 per vertex equal to its z for every vertex in all five, and the
  untextured shape at water heights. The textured shape keeps about the same size at
  every level (level 4: 1,299 vertices / 4,198 triangles; 8: 1,110 / 4,116; 16:
  1,112 / 4,155; 32: 1,074 / 3,971 and 1,083 / 4,144), so the detail per cell drops
  fourfold per level step; the vertices are irregular (235 distinct x values at
  level 8), an adaptive triangulation. All 1,360 diffuse and 1,360 normal textures are
  256 × 256 DXT1 with 9 mips at every level [observed: `fnvtool dds list`], so a
  level-L texel spans 16 × L units.
- **Chunk edges** [observed]: neighbouring chunks do not share their edge vertices.
  Along x = 49,152, level4.x8.y8 has 16 edge positions and level4.x12.y8 52, 14 of them
  in common (heights equal there); level8.x0.y8 / level8.x8.y8 along x = 32,768: 23 and
  18, 6 in common; level4.x8.y8 against level8.x8.y8 along the same line: 12 and 18,
  8 in common. Side by side they leave T-junction cracks, at the same level and across
  levels; the renderer hangs skirts from every chunk's open edges (layer3.md).
- **No tree LOD** [observed]: the archives (base game and Update.bsa) hold no tree LOD
  meshes, lists or atlases; the only per-tree distant data are the billboards
  (`textures\trees\billboards\`, §2).
- **Load distances** in the INI files [observed: the owner's `FalloutPrefs.ini`; the
  game folder's `Fallout_default.ini` and the Launcher presets `low.ini`, `medium.ini`,
  `high.ini`, `VeryHigh.ini`; both are only read]. Section `[TerrainManager]` also sets
  `bUseNewTerrainSystem=1`, `bUseDistantObjectBlocks=1`, `fBlockMorphDistanceMult=0.70`,
  `fHighBlockLoadDistanceLow=20000` (all presets).

  | Setting | Owner | Low / Medium / High / Very High |
  |---|---|---|
  | `[General] uGridsToLoad` | 5 | — |
  | `[General] uExterior Cell Buffer` | 36 | — |
  | `[General] uGridDistantCount`, `uGridDistantTreeRange` | 20, 5 | — |
  | `[TerrainManager] fBlockLoadDistance` | 125,000 | 125,000 in all |
  | `[TerrainManager] fBlockLoadDistanceLow` | 50,000 | 25,000 / 50,000 / 50,000 / 50,000 |
  | `[TerrainManager] fSplitDistanceMult` | 1.5 | 0.75 / 1.0 / 1.0 / 1.5 |
  | `[TerrainManager] fTreeLoadDistance` | 40,000 | 0 / 25,000 / 26,000 / 40,000 |
  | `[LOD] fLODFadeOutMultObjects` | 15 (`Fallout.ini`: 5) | 2.5 / 2.5 / 7.5 / 15 |
  | `[LOD] fLODMultObjects`, `fLODMultTrees`, `fLODFadeOutPercent` | 5, 0.5, 0.6 | — |
  | `[Grass] fGrassStartFadeDistance` | 7,000 | 0 / 3,000 / 5,000 / 7,000 |

  Readings [hypothesis]: fBlockLoadDistance is how far object blocks are drawn (first
  read as the terrain chunks' reach too; the original draws terrain farther, "Terrain
  reach" below), fBlockLoadDistanceLow how far the finest (level 4) chunks reach,
  fSplitDistanceMult scales the distances at which coarser chunks give way to finer
  ones (at Very High and Low, fSplitDistanceMult × 8 cells × 4,096 = 49,152 and 24,576
  lands within 2% of fBlockLoadDistanceLow; at Medium and High it does not), and
  fTreeLoadDistance how far distant trees are drawn (0 at Low: none). uGridDistantCount
  and uGridDistantTreeRange look like the older distant-land settings (cell counts) that
  the new terrain system replaces. Which INI file wins when both set a value is not
  known. What NVForever does with each: layer3.md "Distant LOD".
- **Terrain reach**: at one pose, the hill above Goodsprings (-65347, 5770, z 13970,
  heading 27°, night), the original shows a jagged mountain range along the whole
  horizon toward Vegas; with terrain chunks cut off at 125,000 ours shows a flat, low
  horizon there. Of the `--lod-distance` values tried (that sweep, before terrain got its
  own reach, also moved the object blocks and the distant water), 150,000, 175,000 and
  200,000 did not match the original's skyline and 250,000, the first one that did,
  matched it along its whole width [observed: capture of the original over xNVSE and our
  runs at the same pose, 2026-11-19, #3055]. So 250,000 is a default calibrated at that
  pose, not a measured distance: how far the original actually draws terrain (whole
  chunks reach past any cut-off, so a smaller reach of its own could give the same
  skyline), and whether it is one fixed distance for every place and preset, is
  [hypothesis]. No key in `[TerrainManager]` of `Fallout_default.ini` or the four
  presets gives 250,000 [observed: the files]. NVForever takes one fixed terrain reach
  independent of fBlockLoadDistance, which then bounds only the object blocks
  [hypothesis]; nothing showed the object blocks' reach to be wrong.
- **A chunk without its diffuse** (#3096): a player's report showed the distant hills
  north and east of Goodsprings (about -67758, 2867) flat white at night, shaded only by
  their slopes [observed: report R-ZXT6AC4J's screenshot; no log]; that they took on
  their textures once the player came closer is the player's own account, not seen. In
  the renderer a material whose diffuse is named but not yet in the image store is not
  drawn at all, while one whose diffuse is missing (not named, or failed to load) draws
  white [documented: Bevy's `AsBindGroup` derive, where a texture handle not in the
  store retries and `None` takes the white fallback image; `nvf_assets::nif_spawn`
  leaves the diffuse `None` when its file does not load, and the failure is cached, not
  retried]. White at night fits a missing diffuse with the normal map still there: an
  albedo of 1 under the moon and ambient, several times the terrain textures', shaded
  by the model-space normal map; nothing in the chunk material differs at night
  [hypothesis]. The stock archives hold 1,360 diffuse DDS files, all 256 × 256 DXT1 with
  9 mips by a DDS inventory (`fnvtool dds list`, above), a format the loader takes; that
  each one decodes was not checked file by file. So a file missing or unreadable in that
  install is the likely cause; the report's cause is not confirmed [hypothesis: the
  report carries no log]. The renderer's rules since then
  [design]: a game chunk is spawned, and counts as covering the chunks it replaces, only
  once no shape it draws (the ones with UVs whose triangles the loaded cells do not all
  cut out) waits for its diffuse; until then the old chunks stay drawn. A shape whose
  diffuse is missing is drawn with a neutral fallback colour instead of white, the
  generated LOD's colour for a missing base texture (sRGB 0.45, 0.42, 0.38, as
  terrain.rs draws one; `lodgen.rs`), its normal map kept; the chunk counts as covering
  like any other and is logged once, when it is spawned ("terrain chunk level L xX yY
  drawn with the fallback colour (shape i bound to base colour sRGB …, no base
  texture): <path>: <error>", the colour read back from the material the entity is bound
  to and the path the model named), which pins the cause if it happens again. Each
  spawned shape gets its own copy of the material, owned by its entity alone and
  released with it. A chunk
  whose every textured triangle is cut out draws nothing and is not counted. The exit
  log's "distant LOD terrain surfaces" line counts the chunks drawn with the fallback
  colour and the shapes drawn while their diffuse was still loading (0 expected); smoke
  scenarios `lod-night-diffuse` and `lod-missing-diffuse`. Handover between levels is
  as before: an outgoing chunk stays drawn until every chosen chunk over its area is
  spawned, and the replacements show as each is spawned, over it meanwhile (both sit
  behind loaded terrain by the same depth bias). Known limit, as before #3096 [design]:
  the gate asks the main world's image store (`engine/nvforever/src/lod.rs` `admit` in
  `update_lod`, about line 1013; the outgoing chunk goes at about line 1034), not the
  GPU; under the per-frame upload budget the renderer may put a texture's upload off to
  a later frame (`engine/vendor/bevy_render/src/render_asset.rs`, about line 454), and
  until it is up the replacement's material is skipped, so the area can show a hole for
  a few frames after the outgoing chunk goes; never white.
- How the game hides LOD under loaded cells, and exactly when it switches levels, is not
  known; the renderer's rules are [hypothesis] (layer3.md, `engine/nvforever/src/lod.rs`).

## 8. Interior lighting and lights

- **Interior cells** carry `XCLL`: ambient, directional and fog colours, fog near/far,
  directional rotation (two i32 angles), directional fade, fog clip distance, fog power
  [documented; observed: GSDocMitchellHouse fog 64..5000]. `LTMP` names a lighting
  template (LGTM, `DATA` = the same ten fields, 40 bytes [documented; observed: all 40
  LGTM records decode exactly]) and `LNAM` holds "inherit" flags, one per field
  [documented]. The renderer takes a field from the template when its bit is set
  [hypothesis]. The two rotation angles are read as azimuth about z and pitch, in
  degrees, giving the direction the light shines in [hypothesis]. Of the 31 LGTM
  templates in FalloutNV.esm, 22 store pitch 270 (every "Default" template among them:
  caves, basement, office, shack, wasteland) and the rest 220-278 [observed]; of 388
  CELL records with their own XCLL, 140 store 270, 121 store 0 (all with azimuth 0) and
  45 store 250 [observed]. Read as the direction towards the light, 270 put it beneath
  the floor and cast ceiling fixtures' shadows onto the ceiling (#1645, GSGasStation:
  azimuth 180, pitch 250) [observed: renderer]. Where the azimuth's zero lies and which
  way it turns is not known [hypothesis].
- **Lights (LIGH DATA)**: time i32, radius u32, colour (RGBA bytes), flags u32
  (dynamic, can be carried, negative, flicker, off by default, flicker slow, pulse,
  pulse slow, spot, spot shadow, ...), falloff exponent f32, FOV f32, value u32, weight
  f32 [documented]. Doc Mitchell's house holds 25 LIGH references without a model
  [observed]. LIGH `FNAM` is the "Fade value" [documented: xEdit], read as a brightness
  multiplier (the GECK's light "Fade") [hypothesis]: e.g. ExtLightWarm, the Prospector
  Saloon porch lamp, has radius 200, colour (254, 209, 73) and fade 2.2 [observed].
  The renderer draws each enabled, non-negative, not-off-by-default light as a point
  light (spot light for spot types) with range = radius. Each adds colour x fade x
  (1 - (d / radius)^2) x N.L to the display-value lighting sum ("Light on surfaces in
  display values" below), with no inverse square [hypothesis: the falloff exponent, 1
  for most lights, is not used]. With Bevy's physically based inverse-square falloff instead, the
  porch lamp lit a small hot spot and left the door and wall dark, where in game they are
  lit warm [observed, owner's screenshots].
- **Lamp strength against the original** (2026-10-23, Dead Money's wall lantern; frames
  private in `C:\fnv-private\references\fx_audit\game\dm_lamp_*.png`). Light
  `NVDLC01ExtSconceKeyWT` (radius 200, (224, 255, 117), fade 2.5) stands 91 units in front
  of the wall at Puesta del Sol (ground z 1028). Captured with the weather forced to
  NVWastelandClear (no Dead Money grade) and rendered with the same weather, the same
  camera: wall within about 110 units of the light, original clipped white (255, 255,
  250), renderer (129, 145, 117); unlit wall 140+ units away, original (50, 48, 48),
  renderer (47, 44, 42) [observed]. So the ambient light matches and the lamp is at
  most half as strong in the renderer, and greenish where the original is white. With
  Dead Money's weather the original's lit wall is saturated yellow (255, 232, 82): the
  yellow is the weather's grade, the brightness is the lamp. The numbers fit the lamp's
  light being summed in display values (colour bytes / 255 × fade × falloff × N·L, added
  to the ambient and times the texture in display values, clipped), which in linear terms
  makes a fade-2.5 lamp two to three times stronger near its core; "Light on surfaces"
  above found the same for daylight. Tried and left out [observed, lab L3 frames]: placed
  lights alone at fade^2.2 or (fade × falloff)^2.2 (Doc Mitchell's living room −0.04, the
  saloon up to −0.15 with clipping), and lights scaled by the image space's Emissive Mult
  (no visible change: the eye adaptation lifts the brighter scene less). Since the lab's
  ambient, sun and lights were all fitted to the linear sum, the way forward is a
  refit of the whole lighting sum in display values against the lab frames, the effects
  audit's night spots, Dead Money and these lantern frames, not a lamp-only change.
  Also tried for the grade: an adapter's tint at its own alpha, or at the base amount
  with the adapter's colour; both made every Mojave exterior frame's colour error 4-8
  times larger (amber far too strong by day; the Strip purple at night) and barely
  changed Puesta del Sol, so the tint rule stays (§6).
- **Placed light radius**: a placed light's `XRDS` "Radius" [documented: xEdit] is an
  offset added to its base's DATA radius [observed, below]. Interior LIGH references:
  10,087 in the ten masters, 8,524 with `XRDS` (`crates/fnv-world/examples/light_survey.rs
  --xrds`); `XRDS` / base radius runs from -0.893 to 73.2 (median 0.21), and base +
  `XRDS` is positive for every one, while 2,142 values in FalloutNV.esm alone are
  negative, so `XRDS` cannot be an absolute radius. Whole interiors are built from lights
  whose base radius is useless alone: every light of the REPCONN basement
  (RocketLabBasement) has base radius 16 or 20 and `XRDS` 134 to 1,171. Against the
  in-game frames (tools/capture/README.md section 7, `tools/capture/frames/interiors.json`,
  2026-10-09), adding `XRDS` raised SSIM on all seven interior frames: Doc Mitchell's
  house 0.588 -> 0.697, 0.421 -> 0.683, 0.594 -> 0.762, 0.589 -> 0.703 and the Prospector
  Saloon 0.750 -> 0.806, 0.721 -> 0.742, 0.701 -> 0.721; in the living-room frame the base
  radius left the left wall dark where the game lights it [observed]. The renderer's range
  is max(0, base + `XRDS`) (`fnv_world::lighting::placed_light_radius`).
- **Black LIGH colours**: some lights store colour 0, 0, 0 in `DATA` and hold their
  colour in their model instead [observed: `LamplightWarm` (0x0003A595, the Goodsprings
  gas station's lantern light, radius 312) has a black DATA colour; its model
  `dungeons\caves\lamplight\lamplightwarm.nif` holds an `NiPointLight` with diffuse
  (0.78, 0.58, 0.16), dimmer 1, under an `Idle` sequence whose `NiLightColorController`
  flickers it between about 0.62 and 0.78 red over 3 s, and an `NiAmbientLight`]. In
  game the lantern lights the counter warm (owner's screenshot, playtest list 5 item 6).
  The renderer takes, for a black DATA colour, the diffuse colour times the dimmer of the
  first `NiPointLight` or `NiSpotLight` of the LIGH's model, as display values like the
  DATA colour [hypothesis], and draws no light when there is none
  (`nvf_assets::model_light`, nvforever `lights.rs`). The flicker is played from the
  model's keys since 2026-10-23 ([model-lights](model-lights.md)).
- **External emittance**: a placed reference's `XEMI` names a region (REGN) or a light
  (LIGH) as its "Emittance" [documented: xEdit wbDefinitionsFNV.pas REFR], and
  BSShaderFlags bit 29 is "External_Emittance" [documented: nif.xml]. REGN `RDAT` heads
  each data block (type 3 = weather, then flags and priority); `RDWT` lists weathers with
  chance and global, laid out as CLMT `WLST` [documented: xEdit; observed: these decode
  exactly, schemas/fnv/sky.json]. Around the Prospector Saloon [observed]:
  - the porch lamp beams (FXLightBeamBrtW02, FXLightBeamW02) name
    InvertedDaylightWarmRegionNV, whose weather InvertedDaylightWeatherWarmNV has black
    sunlight at sunrise, day, sunset and noon and (255, 207, 117) at night. The two
    VaultWhite256 lights above the porch name InvertedDaylightRegion (sunlight white at
    night only). The beam shader sets bit 29. In game the beam is invisible at noon
    (owner's screenshot).
  - the window light rays inside (IndFXLightRays01, IndFXLightRaysLeft01) name
    NVInteriorRegion, whose weather NVWastelandInterior has sunlight (255, 227, 170) by
    day and black at night; their shader sets bit 29 too. In game they are warm yellow
    by day.

  The renderer resolves XEMI to a region's weather or the light's colour times its fade.
  The region's weather is the one on screen when the region lists it, else the region's
  highest-chance weather (the only one before 2026-10-24); while the sky fades between
  weathers, the colour fades from the outgoing weather's pick to the incoming one's
  [hypothesis: a region's weather is rolled only where the camera stands, so a region the
  camera is not in shows its likeliest; `fnv_world::lighting::RegionEmittance::shown`;
  headless callers without a sky get the highest-chance weather]. The light's
  colour times its fade (the GECK tutorials: emittance takes the light's colour and
  brightness, level-design.md section 2) and scales by its sunlight colour at the current
  hour (that colour times fade for a LIGH): the surface colour of unlit shapes with bit 29, the emitted light of lit ones,
  and the colour of LIGH references [hypothesis]. Shapes on references without XEMI are
  unchanged. Before this the beam drew at full strength day and night and the rays were
  white. References that scripts enable at run time (`nvf_play::script_draw`) take the
  same driven material (2026-10-23): the Strip's 32 street lamp beams
  (`StripLightBeamAct`, enable parents 0x0013BAE1-3, switched on at night) name the light
  `stripStreetLight` (0x0012762B, colour (150, 198, 102), fade 4) and had drawn white.
  The Prospector Saloon's window models `indfxlightrays01`, `indfxlightraysleft01` and
  `indfxlightraysnleftsm01` contain unlit additive shapes with bit 29 set. Their flat cards
  share a sweep direction. Vertex alpha rises from zero near the window, peaks around
  0.10–0.12 near that end, and fades to zero at the far end [observed, 2026-11-24,
  vertex positions and colours read from the NIFs in Fallout - Meshes.bsa]. The geometry
  and emittance source are enough to identify beam candidates. See the [beam census
  design](../design/room-beams.md) for this engine's selection rules.

  XEMI on light references [observed, `light_survey --xemi`]: 2,508 of 10,087 interior
  lights carry one (1,497 name a LIGH, 1,011 a REGN such as NVInteriorRegion or
  NewVegasRegion); outdoors 304 of 1,070 (246 REGN, mostly InvertedDaylightRegion). In
  Doc Mitchell's house most lights name another light (e.g. OfficeLightKeyNeutral names
  DLC01FXLightMillSmoke, colour (255, 162, 85)). Which rule the game applies to a light
  named by a light is not settled: against the four Doc Mitchell frames, scaling the
  light's colour by the target's colour times fade (kept), by its colour alone, replacing
  the colour, or ignoring LIGH targets all score within 0.03 SSIM of each other; ignoring
  them gives a lower mean colour error (0.008-0.028 against 0.033-0.049), but the near
  walls of the entry frame are warm in game, as with scaling [observed].
- **Interior XCLW is not water** (issues #1546, #1078, 2026-11-06). The Tops' 13th floor
  (`2ETopsBennysFloor`, 0x0010E429) and presidential suite (`2ETopsPresidentialSuite`,
  0x0010E19B) set CELL DATA "Has water" with XCLW 14,095.9, while every reference in them
  lies between z 7,228 and 9,284 [observed: `fnvtool world refs`]. The renderer's
  under-water fog read that XCLW as a surface over the camera and drew the worldspace's
  water (WastelandNV NAM2 Potomac, under fog −1,000..700, linear, deep colour
  (27, 39, 24) × the light) round it: walls 59 % fogged at the camera and black past
  about 700 units, the bulbs farther down the hall still showing [observed: our frame
  from (3300, 2375, 7240) heading 270, 2026-11-06; the owner's "circle of darkness
  around the player"]. Physics and audio had already stopped reading an interior's
  XCLW as water (physics.md §6.6, `swim_h2o_drops` and `sw_guardian`: the original swims
  at placed water objects, not XCLW; audio.md §15.2). The under-water fog now follows the
  same rule: indoors the camera is under water only in physics' water (placed volumes),
  coloured by the water type of the placed volume's base PWAT (DNAM, the type the renderer
  draws that pool with), else the cell's (XCWT), else the worldspace's [hypothesis for the
  colour]. Interiors with PWAT water but no XCLW (Vault 34, Vault 22, the Lake Mead and
  Zion caves, among others, per a review of the install) get this fog for the first time;
  coloured by XCWT or the worldspace they would take the wrong water (Vault 34's toxic
  pools fogged as the Potomac) [observed: review of PR #1779]. Without physics
  (`PhysState`: the viewer, no `--play`) an interior has no under-water fog. Twelve interiors store an XCLW above every reference they hold (the two Tops
  cells, HooverDamIntPowerPlant01 and 01b at 15,067, the 3C boat rental and BS caves,
  CampSearchlightHiddenCave, the OV sleep cells) [observed: `world cells` against
  `world refs --interiors`, FalloutNV.esm]; all were fogged before. Both Tops cells' own
  lighting is unchanged: ambient (46, 36, 27) from their template StatesmanHotelTemplate
  (LNAM bit 0), the 13th floor's fog (74, 57, 43) 512..6,000 power 1 and clip 8,000, the
  suite's fog (32, 47, 81) 64..6,500 (template) power 0.8 and clip 2,048 [observed: `esm
  decode`]. Still drawn: the renderer's interior water plane at XCLW (above the ceiling
  in these cells). Against the original [observed: capture `tops_suite_door`,
  2026-11-06 21:55, save nvfairport, hour 21, from (1990, 1216, 8040) heading 90; frame
  private]: the original lights the suite evenly to its far wall (bar, pillars, the
  wall's bands of light, posters, sofa and rug); ours from the same spot showed the rug
  and sofa only, black beyond, before the change, and the whole room after it, a little
  darker on the right (kept: interior brightness, owner 2026-10-27). The 13th-floor
  hallway (from (3300, 2375, 7240) heading 270) now reads to its far end; before, it was
  black past about 700 units. The suite's clip distance (2,048) hid nothing visible in
  our frame from that spot; not checked from the room's far corners.
- **Interior reference frames** (2026-10-09, plugin v7, save nvfdoc, 1280 x 720,
  `fDefaultFOV=75`) [observed]: `capture_frames.py` cameras with a `cell` travel by `coc`.
  Seven frames exist (Doc Mitchell's house x 4, Prospector Saloon x 3). The Vault 21 and
  REPCONN basement cameras reached their poses (recorded position and angles match) but
  the plugin's screenshot step failed there in both runs ("BitBlt failed"), only in those
  two cells; why is not known. Remaining differences: the game's lamp bulbs and sunlit
  windows glow with a halo (bloom) the renderer lacks, and Doc Mitchell's house is a
  little warmer and brighter in game.
- **Interior image space**: CELL `XCIM` names the cell's image space [documented:
  xEdit]. In interiors the renderer grades with it instead of the worldspace's; e.g. the
  Prospector Saloon's ShackInteriorWaste01 (saturation 0.82, tint (1.0, 0.71, 0.27) at
  0.4) gives the warm brown look of the owner's in-game screenshot [observed]. Cells
  without XCIM are not graded [hypothesis].
- **Light on surfaces in display values** (2026-10-24, the Dead Money lantern) [hypothesis,
  fitted to the original's frames]: the game adds its lights as display values — colour
  bytes / 255 × fade × attenuation × N.L for each light, the sun's (or moon's, or the
  interior's directional) colour × N.L, plus the ambient colour — multiplies the texture
  by the sum and shows the result held to 1 per channel. Evidence: the original's wall
  beside the Puesta del Sol lantern (light `NVDLC01ExtSconceKeyWT`, radius 200, colour
  (224, 255, 117), fade 2.5, 91 units in front of the wall; frames `dm_lamp_*.png`,
  private) is clipped to (255, 255, 255) under NVWastelandClear and to (255, 201, 71)
  under Dead Money's own weather, saturated, a few metres from the lamp. Summed in linear
  light and shown through Bevy's Tony McMapface (as before), the same light gave
  (141, 162, 136) and (100, 92, 52): the linear sum spends the light, and Tony McMapface
  turns bright light pale and never reaches white (light × 20 still gave
  (229, 243, 255), bluish). Lamp-only rescalings (light fade^2.2) had failed the lab: Doc Mitchell's lights already matched.
  The renderer now [observed: bevy_pbr 0.19.1, nvforever `lights.rs`]:
  - patches `apply_pbr_lighting`: in a world view (marked by a negative ambient-light
    alpha; the minigame views keep Bevy's sum) Bevy's diffuse terms are left out and the
    sum is formed from the same lights, whose colours there hold display values; the
    surface gets the linear colour of (diffuse colour's display value × sum), held to 1
    per channel, both ways by the sRGB curve (until 2026-11-25: diffuse colour × sum^2.2;
    "Dark values of the sum" below). The lights' and the
    ambient light's highlights (specular) take their colours back as linear light;
    taken as display values they put white sparkles on the floors of Jacobstown's lodge.
    On first-person shapes (`FIRST_PERSON_DEPTH`: arms, held weapon, Pip-Boy),
    shadow-casting spot lights (the player flashlight, `flashlight.rs`) are scaled down in
    both display diffuse (`0.15`) and specular (`0.10`) (`lights/display_shader.rs`)
    [design, #3121] so aiming into the beam's inner core does not clip the weapon and hand
    to white, while world geometry, third-person body/weapon (`BODY_LAYER`), and
    non-shadow-casting world spot lamps (`LIGH` `SPOT`) stay at full strength;
  - shows the main view's colour clipped to 1 in the place of Tony McMapface (its curve
    in Bevy's tonemapping functions is swapped; the pass keeps running, and with it the
    eye adaptation's exposure, which Bevy applies there); the sky leaves out its inverse
    of Tony McMapface;
  - shares in the sum [fitted]: interiors XCLL's ambient and directional colours × 1.0;
    outdoors the weather's ambient × 0.9, its sunlight × 0.75 by day (× the Sunlight
    Dimmer's scale) and its night "sunlight" (moonlight) × 0.5; lights × 1 (colour ×
    fade); grass's own weights 1.3 and 0.25 (grass.md §5). Fitting showed: interiors
    want the ambient as stored (× 1.0: every Doc Mitchell frame then beats the linear
    sum); stronger lights help the saloon and hurt Doc Mitchell's house, so the lights
    stay at their records' strength; outdoors by day more ambient and less sun than the
    first guess (0.697 = Bevy's old 0.452 as a display value, and 1.0); at night the
    moonlight at the sun's share made Primm and the Strip bright and blue (Primm street
    median (39, 46, 65), original (41, 41, 49)), halving it fixes that.
  - Results against the linear sum (SSIM / mean colour error, lab comparator): lab L3
    frames goodsprings noon 0.498/0.011 → 0.502/0.026, dusk 0.514/0.063 → 0.532/0.058,
    hill 0.579/0.022 → 0.582/0.041, lake shore 0.524/0.087 → 0.523/0.106, black mountain
    0.605/0.009 → 0.605/0.026, east 0.467/0.021 → 0.469/0.037, spring mountains
    0.723/0.036 → 0.717/0.042, hoover ridge 0.572/0.016 → 0.580/0.035, Doc's entry
    0.706/0.033 → 0.714/0.008, living 0.725/0.023 → 0.737/0.013, kitchen 0.777/0.018 →
    0.778/0.007, bedroom 0.762/0.023 → 0.777/0.019, saloon south 0.807/0.014 →
    0.799/0.004, north 0.750/0.019 → 0.709/0.023, bar 0.730/0.026 → 0.716/0.032 (mean
    SSIM 0.649 both; every frame passes its threshold). Night frames of the effects audit
    (22:00): lantern front 0.536/0.136 → 0.606/0.061, side 0.544/0.111 → 0.636/0.053,
    front under NVWastelandClear 0.502/0.089 → 0.554/0.033; Jacobstown lodge 0.606 →
    0.777, Hoover Dam lower level 0.729 → 0.796, Primm entrance 0.617 → 0.683, Strip
    0.450/0.500 → 0.499/0.548, Dead Money Villa three frames +0.01 to +0.04; lower:
    HELIOS One 0.500 → 0.452, Lake Mead cave 0.780 → 0.746, Vault 19 0.627 → 0.606 (its
    colour error 0.051 → 0.095). The lantern's wall now reads lit: (218, 231, 244) and
    (166, 147, 70) where the original clips to (255, 255, 255) and (255, 201, 71).
  - Open: the original's lamp pools are still brighter and more saturated close to the
    lamp (the lantern's) and the saloon's north and bar frames darker than the
    original's (means (22, 15, 11) against (28, 21, 14)), while Doc Mitchell's lights,
    under the same rule, are right; the attenuation's shape (1 − (d / r)², the falloff
    exponent unused) is the next candidate. Outdoors by day the frames came out a little
    bluer than the original (the weather's bluish ambient takes a larger share).
- **Dark values of the sum** (2026-11-25; the Vikki and Vance Casino's floor, cell
  000D7845 `VikkiAndVance`). The rule above shows texture × sum as a display value. The
  renderer formed it as diffuse colour × sum^2.2 in linear light, while the texture is
  read, and the frame shown, through the sRGB curve [observed: nvforever
  `lights/display_shader.rs` before this date]. The two agree for mid and bright values
  and part in the dark, where the sRGB curve is a straight line: for a texture of display
  value 0.4, a product of 102 / 255 was shown as 102, 51 as 47, 26 as 19 and 16 as 7
  [observed: the two curves]. The sum is now made linear by the sRGB curve itself
  (`nvf_linear_to_display`, `nvf_display_to_linear`), so the shown value is the product
  at every level [design: the rule as stated; whether the original's dark end is that
  product is the rule's own hypothesis].
  - The casino's records [documented: FalloutNV.esm]: XCLL ambient (31, 27, 24) with the
    template's inherited (LNAM bit 0: Vault19Template, (40, 40, 40)); directional
    (34, 21, 13) at rotation 0 / 0, which the reading above takes as level, so it adds
    nothing on a floor; 25 LIGH references, all `BasementLight…` fills, keys and kickers
    of base radius 200, placed radii 147 to 502, hung 59 to 225 units over the two floors
    (z 7296 and the pit's 7232), each with an XEMI naming another light. With 1 − (d / r)²
    × N·L, a lamp adds more than 0.05 to the sum on 41 % of the upper floor's area and
    35 % of the pit's, and more than 0.3 on 15 % and 12 % (`fnvtool world refs`, a grid of
    50 units). The rest of the floor is lit by the ambient alone.
  - Measured in the renderer [observed: 1920 × 1080, post effects, grade, lamp shadows,
    ambient occlusion and eye adaptation off; a temporary switch, not kept, drew the
    ambient, the lamps and the highlight apart]:
    the pit's floor away from the lamps, ambient alone, (8, 5, 3) before and
    (17, 13, 10) after; the floor under the kicker lamp 0016A82B (radius 502, 145 units
    up), (134, 89, 32) before and (134, 89, 36) after, of which ambient alone
    (8, 5, 3) → (17, 13, 10) and lamps alone (117, 74, 20) → (116, 75, 26): after the
    change the parts add up to the whole in display values, as the rule says. The
    highlight is 1-2 % of the light looking down at one's feet and at most about a third
    in the streak towards a lamp; the floor that reads bright from a distance is a
    lamp's diffuse pool, not its highlight.
  - Still open, each of which would lift this floor further and none of which this
    change touches [hypothesis]: the attenuation's shape and reach (above); what a
    directional rotation of 0 / 0 means (121 of 388 cells store it); the rule for a light
    whose XEMI names a light (it dims these lamps to 0.4-0.9 of their own colour × fade
    and turns the door's fill orange); the original's bloom and eye adaptation in this
    cell's image space. No frame of the original in this cell exists yet. The saloon's
    north and bar frames above, darker than the original's by 6 of 255, are the size of
    difference this change makes at that level and are to be measured again.
- **Highlights at grazing angles** (2026-11-13, #2266): a fire barrel in Primm,
  Barrel03Fire1024 (ref 000E89EF, a flickering LIGH of radius 1024 [documented:
  FalloutNV.esm LIGH]) standing on the urban sidewalk kit (texture `sidewalkquad01`,
  Specular flag set, glossiness 60, a normal map without alpha [documented: the kit's
  NIFs]), drew a long orange glossy streak on the shaded sidewalk toward the camera
  [observed: a tester's run, 2026-11-12]. The original, captured at the same spots by
  night and by day (3, 6 and 12 m, standing and crouched, from the north and from the
  casino's arcade), shows no streak at any of them [observed 2026-11-13]. NVForever with
  Bevy's Fresnel drew it by day from the arcade side, crouched at 6 m [observed
  2026-11-13]. A test aid that switched off in turn the Fresnel rise (F = F0, or
  F90 = 0) or the whole GGX lobe (a Blinn-Phong lobe with the glossiness as exponent and
  no Fresnel) removed the streak each time: the sidewalk's red and green means in the
  streak, 165/124 with Bevy's Fresnel, came to 148/114 with each switch [observed
  2026-11-13]. So the streak is Bevy's Fresnel rise: Schlick's curve towards
  f90 (saturate(49.5 × F0) for a grey F0) (bevy_pbr 0.20.1 `pbr_lighting.wesl` `fresnel`) lifts the
  sidewalk's faint highlight (reflectance 35/255, F0 ≈ 0.003, nif_spawn.rs
  `UNMASKED_SPECULAR`) up to about 50 times at a low view. NVForever's lights.rs
  `FRESNEL_EDIT` now keeps the Fresnel factor of the direct lights' isotropic highlight
  (the sun's included) at F0 in world views; the GGX distribution and visibility still
  vary with the angle [design]. Highlights the original may show at low angles on glass
  or metal lose the rise too; no scene of the original with such a highlight has been
  compared yet. Open: whether the original's highlight is Blinn-Phong as nif.md "Specular"
  supposes, which needs a scene where the original does show highlights (the casino
  floor's captures failed: the capture plugin took no shots indoors).

## 9. Open questions

1. Engine-defined base objects (FormIDs < 0x800): which are visible? (Assumed none.)
2. Texture-layer details: quadrant order and blending of `layer` numbers. (The alpha
   grid orientation and the default texture are in §3.) Verify by comparing renders with
   in-game screenshots.
3. XCLW sentinel 3.4028235e38: confirm that the worldspace water height applies.
   Water drawing (§6): ANAM opacity, the trough's dimmer body (#1787), whether the fog
   amount caps cell water's opacity too (compare Lake Mead far from shore, where the
   bed is deep, with the original; ours draws only placed water capped, #1789), noise
   layer units; to settle with in-game captures at Lake Mead at several heights and
   distances from the shore.
4. Weather selection rule and the meaning of WLST globals (see §6).
5. Light brightness and falloff shape (the falloff exponent), the XCLL rotation angles,
   fog power and clip distance, and the rule for a light whose XEMI names a light (section
   8). The radius is settled (section 8, "Placed light radius"). Interior frames of Vault
   21 and the REPCONN basement are still to capture (section 8).
6. The engine's handling of SCOL references: whether it draws the combined mesh (assumed)
   or the parts.

## Water surface ownership during streaming

[design] Loaded exterior cells own their local water planes. Distant water fills
eligible cells outside that loaded set, inside the configured LOD range. A cell
must not retain both owners after stream reconciliation. Cell count alone does
not identify that set: replacing one loaded cell with another can leave its size
unchanged.

[design] Distant-water invalidation includes camera cell, effective LOD radius
(including off/interior) and the exact loaded-cell keys. Compare stable keys
without allocating each frame; retain a copy only when coverage changes, and clear
those keys alongside planes/surfaces on worldspace switches (`DistantWater::clear`).
Then remove no-longer-eligible planes, create newly eligible planes and retain the
same entities/materials for unaffected cells. Order `update_water_sky` and
`update_ripples` after `world::stream` and `distant_water` so newly streamed water
materials receive current sky, sun, water-effects, and ripple parameters on their
first rendered frame rather than flashing placeholder defaults. Existing water type,
depth, reflection, alpha, material and mesh policy is unchanged.

[hypothesis] Coplanar retained distant/local planes are a causal renderer boundary
to test for reported water flicker, not yet proof of every observed flicker.
Controls must invoke the actual renderer producer with same-count replacements,
steady coverage, LOD off/range changes and cell/world lifecycle; inspecting only
an invalidation-key getter is insufficient. No original assets or GPU outcome is
claimed by authored CPU fixtures.


#### Current-camera underwater fog handoff

[design: engine render scheduling] The water fog override is composed after the current
camera transform propagates, before auxiliary world cameras copy that fog (the reflection
camera in `reflection.rs`, the 3D scope lens fog sync in `scope_view.rs`, and JIP extra
cameras in `extra_camera.rs`). Active `RemapWaterType` remaps (`WaterRemaps`) apply to
underwater fog in both classic and enhanced water modes, matching `water::water_material`.
The weather/interior composition is shared with the ordinary sky updater. Leaving water
restores its current values in the same frame, including weather, lighting, HDR emissive
and interior-fog preferences changed while submerged; it does not replay a pre-entry
snapshot [hypothesis: that the original's fog returns in the frame the camera leaves the
water; not observed in the original game]. The WATR under-water fog colour rule (§6
"Under-water colour") is unchanged by this handoff. No HUD alpha, breath recovery,
radiation, image modifier, or water shader policy is changed by this handoff.

The prior Update override read the previous propagated camera and requested a
later sky refresh on exit. This is a source-predicted temporal mismatch, not proof
that it explains the entire reported persistent cyan tint. Controls must move the
real camera Transform through enter/exit/re-entry, assert the extracted current
fog, and cover current exterior/interior restoration. GPU/original-profile
acceptance remains separate from CPU producer results.


### CPU validation scope

The water producer/camera-fog and live-IMAD grade controls exercise CPU consumers;
revision-specific CI and failure history are recorded in PR #2861, with follow-on CPU
controls covering nonfinite/f32-overflow guards, nonanimated HDR/fade/radial/DoF/motion
traits, worldspace clearing, first-frame water sky sync, classic-mode `WaterRemaps`
underwater fog, and auxiliary-camera fog handoff with HDR emissive restoration. The
identical-fixture baseline comparisons, native behavior, original mod initialization and
installed-profile/persistent-cyan/GPU/FPS outcomes are not established by those controls.
