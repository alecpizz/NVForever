# Spec note: SpeedTree trees (`.spt`, TREE) (Layer 3)

Tags as in `nif.md`. Observations cover all ten `.spt` files in the owner's archives
(`Fallout - Meshes.bsa`, `trees\*.spt`, FNV 1.4.0.525) and the three TREE records of
`FalloutNV.esm` (EuonymusBush01, WhiteOak01, WastelandShrub01) [observed]. No public
description of the file format was found; the SpeedTree tools that wrote these files are
not available to the project. Every field meaning below is therefore [hypothesis] unless
tagged otherwise, inferred by comparing values across the ten files and against the TREE
records, the billboard textures (side views of the finished trees) and screenshots.

## 1. What an `.spt` file is

An `.spt` file holds no mesh. It is a parameter set from which the tree is generated
when the game runs: branch levels described by numbers and curves, leaf cards, textures
and wind settings [observed: files are 5-8 KB, contain 25-41 curves written as text and
no vertex arrays; documented in general terms for SpeedTree: the runtime library builds
the tree from the parameters]. The generation algorithm is the tree library's own and is
not documented; NVForever generates its own geometry from the parameters (§5), so trees
match the original by eye, not exactly.

## 2. Layout [observed]

The file is a flat sequence of entries. Each entry is a u32 tag followed by a payload
whose type depends only on the tag; there are no lengths or counts outside the payloads.
All ten files parse to their exact end with the table in §3. Payload types:

- *none*: the tag alone (structure markers).
- *u8*, *u32*, *f32*: little endian. Some 4-byte payloads are integers (counts, seeds),
  others floats; §3 says which.
- *string*: u32 length, then that many bytes (no terminator). Length 0 occurs.
- *vec3* (12 bytes), *f32 × 4* (16), *f32 × 5* (20), *f32 × 8* (32), *f32 × 13* (52).
- *uv list*: u32 count n, then n × 8 f32.
- *curve*: a string of the form
  `BezierSpline <min>\t<max>\t<variance>\n{\n\n\t<n>\n\t<x> <y> <tx> <ty> <len>\n ... \n}\n`
  (n control points, each a position (x, y) in [0, 1]², a unit tangent (tx, ty) and a
  tangent length).

## 3. Tags

Structure markers (no payload unless noted): 1001-1005, 1007-1013, 1015-1017 open and
close blocks: 1016 … 1017 encloses one branch level, 1007 … 1008 one leaf map; 1014
(u32) is the number of branch levels and 1006 (u32) the number of leaf maps [observed:
the counts match the blocks that follow]. Order of the top-level sections is fixed:

| Section | Tags | Layout | Meaning |
|---|---|---|---|
| Magic | 1000 | string `__IdvSpt_02_` | format identifier [observed in all files] |
| Tree | 1002, then 2000 | string | bark texture path as authored (`C:\…\WastelandShrub01Bark.tga`) |
| | 2001, 2003, 2006, 2007 | f32 | unknown sizes (1100 or 600; 100; 50-200; 10) |
| | 2002 | u8 | unknown (0 in all files) |
| | 2005 | u32 | random seed [observed: equals the TREE record's first SNAM value for all three TREE records] |
| Branch levels | 1014, then per level 1016 … 1017 | | 3-5 levels; the last one describes leaves (§4) |
| Level | 6000-6007 | curve × 8 | §4 |
| | 6008, 6009 | u32 | segments along and around the branch [hypothesis] |
| | 6010, 6011 | f32 | first and last child position along the branch, 0-1 [hypothesis] |
| | 6012 | f32 | children per unit of branch length (0-800) [hypothesis, §4] |
| | 6013, 6014 | f32 | bark texture repeats around and along the branch (WhiteOak01 trunk 6 × 15) [hypothesis] |
| | 6015, 6016 | u8 | flags, unknown |
| | 6017 | curve | unknown |
| Globals | 1015, 1003, 1004, 3000-3010, 1009 | f32 / u8 | unknown (lighting / leaf settings) |
| Leaf maps | 1006 n, then n × (1007, 4000-4007, 1008) | | one per leaf texture |
| Leaf map | 4000 | u8 | unknown |
| | 4001 | vec3 | colour (1, 1, 1 in all files) |
| | 4002, 4007 | f32 | unknown |
| | 4003 | string | leaf texture path as authored |
| | 4004 | vec3 (x, y, 0) | pivot: where the card attaches, as a fraction of its width and height from the bottom left [hypothesis: values 0.5 ± 0.05, 0.2-0.35; the twig bases in the leaf textures sit there] |
| | 4005 | vec3 (w, h, 0) | card size as a fraction of the tree size [hypothesis] |
| | 4006 | vec3 (w, h, 0) | card size in absolute units [hypothesis: ≈ 4005 × 50 in WastelandShrub01, whose 2006 is 50] |
| | 1010, 1005, 1011, 5000-5004 (vec3), 5005 (f32), 5006 (u8), 1012, 1001 | | unknown |
| Materials | 8000, 8002 (f32), 8003 (f32 × 13), 8004, 8005, 8006, 8007, 8008, 8009, 8001 | | three materials: four RGB triples and a power each [hypothesis: ambient, diffuse, specular, emissive, shininess] |
| Lighting | 9000-9014, 9001 (9005, 9006 markers) | f32 | unknown |
| Leaf UVs | 11000, 11002, 11001, 10000, 10002, 10003, 10004, 10001 | 10002: uv list | 10002 has one entry per leaf map: the four corners (u, v) of that leaf's cell in the combined leaf texture (§6) [observed: the cells tile the texture in halves or quarters] |
| Collision | 12000, 12002 (f32 × 4), 12003 (f32 × 5), 12001 | | present in WhiteOak01 only: spheres (x, y, z, r) and capsules (x, y, z, r, h) [hypothesis] |
| Fronds | 13000-13013, 14007, 14008, 14000-14006, 14001 | 13005: curve; 14002: string | frond settings and texture (`ShrubBoxwoodLeaves01.tga`) [hypothesis]; not used |
| LOD | 15000, (15002 u8, 15003 f32) per level, 15001 | | unknown |
| Wind | 16000, 16002-16012 per level, 16001, 16013, 16014 | f32 | unknown per-level settings |
| Other | 18000-18005, 19000-19002, 20000-20005, 21000, 21001, 22000 | 20005: f32 × 8 | 20005 = the four billboard UV corners (1,1 0,1 0,0 1,0) [hypothesis]; 21001 equals the TREE CNAM rock speed in WastelandShrub01 [observed] |

## 4. Branch levels [hypothesis]

Level 0 is the trunk; level i + 1 grows from level i; the last level holds no wood and
places the leaf cards on the branches of the level before it [hypothesis: its length
range is ~0, its angle a constant 90° and its counts 1-1 in all files]. The curves of
one level:

| Tag | Range in files | Meaning used |
|---|---|---|
| 6000 | 0-1, variance 0-50 | unknown; variance used as random bend per segment, degrees |
| 6001 | 0-1 | unknown |
| 6002 | 0-1 | unknown |
| 6003 | 0-1 | unknown |
| 6004 | 0.01-0.8 | branch length, fraction of the tree size, over the position along the parent |
| 6005 | 0.0009-0.08 | branch base radius, fraction of the tree size, over the position along the parent |
| 6006 | 0-1 | radius along the branch (taper; 1 at the base, ~0 at the tip in all files) |
| 6007 | −90 to 90 degrees | branch angle over the position along the parent: out of the plane across the parent, positive towards the parent's tip; on a level springing from a stub (its shortest branch at least twice its parent's longest, §7), from the parent's axis instead (trunk: −90; leaves: 90) |

A curve's value at x: the curve y(x) (a cubic Bézier through the control points, with
the control handles at each point's tangent × length) scaled to [min, max], plus a
random offset in ±variance.

Why these readings [hypothesis, checked against the billboards]: the billboard of each
tree is a square side view fitted to the tree (§6), so the opaque part of the billboard
gives the tree's width : height. With children = 6012 × branch length and the 6007
angle read as in the table above (from the plane across the parent, from the parent's
axis on a level springing from a stub), the generated trees come out at

| Tree | Billboard width : height [observed] | Generated |
|---|---|---|
| WastelandShrub01 | 0.891 : 0.617 = 1.44 | 1.28 |
| WhiteOak01 | 0.611 : 0.990 = 0.62 | 0.63 |
| EuonymusBush01 | 0.846 : 0.650 = 1.30 | 1.18 (2.19 before the stub reading, §7) |

"Generated" is the generated tree's width (the larger of its x and y spans, leaf cards at
their full width) over its height (up to the cards' top edges): what the renderer scales
by (§5). `fnvtool spt shape "<Data>/Fallout - Meshes.bsa"` prints it for every tree (row
`as_is`, `full_cards.aspect`) beside the other measures and readings of §7; the smoke
scenario `spt-shape` checks the three rows.

Reading 6013/6014 as child counts instead gave WastelandShrub01 about 20 leaf cards and
EuonymusBush01 four, against the dense crowns of their billboards; measuring the angle
from the parent's axis made WastelandShrub01 a column three times taller than wide, and
leaves WhiteOak01's −10° pointing back into the trunk. With these readings
WastelandShrub01 has a 0.1 trunk carrying about 40 stems (400 per unit), each with
5-12 twigs, each with 2-6 leaf cards 0.09-0.11 of the tree size wide (about 2,000
cards, each a picture of a whole twig cluster).

## 5. NVForever's generator (implementation choice, not the original algorithm)

`crates/fnv-spt` builds a tree from §3-§4 with its own deterministic random numbers seeded
by tag 2005: tube branches (segments along and around from 6008/6009, at least 3 sides),
6012 × length children spread along [6010, 6011] at golden-angle steps around the
parent, each turned by the 6007 angle (from the parent's axis when the parent is a stub,
§7) and bent randomly by the 6000 variance; leaf
cards at the child positions of the last wood level, a leaf map picked at random per
card, drawn facing the camera. The tree size is 1; the renderer scales the tree so its
larger dimension (width or height) equals the TREE record's billboard size (larger of
BNAM width and height; OBND max z without BNAM). OBND does not give the tree's size:
WastelandShrub01's OBND z 0-175 equals its BNAM, but its billboard shows a tree 0.62 of
that tall [observed].

## 6. Textures and records [observed]

- Use: WastelandNV places 18,050 TREE references: WastelandShrub01 17,998,
  EuonymusBush01 51, WhiteOak01 1.

- Bark: `textures\trees\branches\<file name of tag 2000, extension .dds>`, lower case
  (`wastelandshrub01bark.dds`; present for every `.spt`).
- Leaves: one combined texture per tree, `textures\trees\leaves\<TREE ICON>`
  (`WastelandShrub01Foliage.dds`, 1024 × 512 DXT3: two twig clusters side by side; the
  names in 4003 are the separate source images). 10002 gives each leaf map's cell.
- Billboard: `textures\trees\billboards\<model stem>.dds` (world.md §2).
- TREE record [documented: xEdit wbDefinitionsFNV.pas, in schemas/fnv]: MODL = the
  `.spt` name, ICON = leaf texture, SNAM = seeds, CNAM = leaf curvature, leaf angles,
  dimming, shadow radius, rock and rustle speeds, BNAM = billboard size.

### Leaf and branch dimming (CNAM) [hypothesis]

The billboards darken from the top of the crown to the bottom, and the darkening is
stronger the lower the record's leaf dimming [observed: mean luminance of the opaque
billboard pixels in four bands, top to bottom, bottom ÷ top]:

| Tree | CNAM leaf dimming | Bands | Bottom ÷ top |
|---|---|---|---|
| WastelandShrub01 | 0.05 | 47 38 32 22 | 0.47 |
| EuonymusBush01 | 0.3 | 63 61 52 37 | 0.59 |
| WhiteOak01 | 0.7 | 71 67 65 70 | ~1 (bottom band is mostly trunk) |

Reading used: leaf dimming is the brightness of the lowest leaves of the crown;
brightness = d + (1 − d) · h^(1/3), with h the leaf's height in the crown (0 at the
lowest leaf, 1 at the highest), applied as a display-space factor (to the power 2.2 in
the linear renderer). Branch dimming does the same for bark, by vertex height. The
cube root is fitted: the square root gave a WastelandShrub01 render 0.36 bottom ÷ top
against the billboard's 0.47, the cube root 0.42 (the rest of the gradient there comes
from the sun). Leaf normals point horizontally out of the crown, tilted up 45°, so the
top-to-bottom darkening is not counted twice. Under NVForever's Physical lighting row only
(graphics-enhanced.md §3.25), a dome around the crown's lowered core replaces that rule:
out from the core, with an upward part of three quarters of the leaf's height above it plus a
quarter of the crown's reach (`canopy_normals`) [design, chosen by eye]; the default look keeps
the rule above.

### Wind [hypothesis; amplitudes and speeds chosen by eye]

The records give a wind strength and two speeds, not the motion: WTHR DATA wind speed
(0-255; 50 for most weathers, 127 HVDustStorm, 130 NVHooverFinalBattle [observed]) and
TREE CNAM rock speed and rustle speed (WastelandShrub01 0.07 and 0.35; EuonymusBush01
and WhiteOak01 1.0 and 1.0 [observed]). No record gives a wind direction [observed:
WTHR DATA has none]; NVForever uses a fixed one. NVForever's motion (tree.wesl): the
whole tree leans along the wind and swings about that lean, the offset growing with
the square of the height in the tree, up to a quarter of the tree's height at full
strength (wind speed 255); each leaf card also tilts (up to 0.35 rad) and flutters
along the wind with its own phase. Rock and rustle speeds map to angular speeds
2π(0.2 + 0.6 rock) and 2π(0.5 + 1.5 rustle): WastelandShrub01 rocks about every 4 s
and rustles about once a second. The same motion runs in the shadow and depth passes.
The `.spt` wind section (16000-16014, per level) is not used: its meaning is unknown.

### Distant trees

The archives hold no tree LOD (no meshes, lists or atlases for trees; world.md §7)
[observed]. The Launcher presets set `fTreeLoadDistance` (section `[TerrainManager]`) to
0 / 25,000 / 26,000 / 40,000 from Low to Very High; the owner's value is 40,000
[observed]. The renderer draws every TREE reference outside the loaded cells and within
that distance as its billboard (BNAM width × height × the reference's scale, foot at the
reference's position, upright, turned towards the camera) [hypothesis: what the game
draws for distant trees; to compare with the long-view reference frames]. The game's
`[SpeedTree] fLODTreeMipMapLODBias=-0.75` (owner's INI) suggests sharper mip levels for
distant tree pictures; not applied.

## 7. Open questions

- Meanings of 2001/2003/2006/2007; the remaining curves (6001-6003, 6017).
- Whether the frequency counts over the whole branch or only [6010, 6011].
- Whether the stub reading for EuonymusBush01 (below) is the original's rule or a fit;
  WhiteOak01's OBND (1567 high) is twice its billboard (768).
- How SNAM seeds beyond the first are used (WhiteOak01 has five: variants per reference?).
- Leaf curvature and leaf angles (CNAM), 3000-3010, 9000-9014; whether dimming also
  depends on depth into the crown, not only height; the `.spt` wind section
  (16000s); the wind direction and how the original maps the CNAM speeds.
- Fronds (13000/14000 sections).

### EuonymusBush01's width (#3126)

The generated bush came out 2.19 times as wide as tall, its billboard 1.30 (§4). A
player's report placed a "very spread out" shrub in the Citadel's B ring (TTW); the
records place the bush in `CementPlanterRd01` pots there (2 references) and about 150
times in Tenpenny Tower's cells [observed: `fnvtool world refs` on Fallout3.esm, the
#3126 triage].

Parameters of the three trees [observed: `fnvtool spt shape` on the test install,
2026-11-21; lengths in tree sizes, angles in degrees, ± the curve's variance]:

| Tree | Trunk (level 0) | Level 1 | Level 2 |
|---|---|---|---|
| EuonymusBush01 | 0.01 long, 800 per unit (8 stems over 0-0.8) | 0.1-0.2 long, 10-40° ± 15 (40 at the base falling to 10), bend 40, 35 per unit over 0.2-0.75 | 0.02-0.04 ± 0.03 long, 50-60° ± 15, 400 per unit; cards 0.04-0.05 |
| WastelandShrub01 | 0.1 long, bend 50, 400 per unit | 0.05-0.12 long, 0-75°, 100 per unit over 0.15-1 | 0.01-0.03 long, 0-45°, 200 per unit; cards 0.09-0.11 |
| WhiteOak01 | 0.5 ± 0.15 long, 50 per unit over 0.25-0.93 | 0.12-0.18 ± 0.03 long, −10-60° ± 25, 15 per unit over 0.1-0.8 | 0.068-0.101 ± 0.03 long, −10-75° ± 15, 600 per unit; cards 0.08-0.1 |

Width : height under the readings of `fnv_spt::probe` (full cards / card pivots only /
wood only) [observed: the same run, fnvtool built from 46a60e632, before the change
below]:

| Reading | EuonymusBush01 | WastelandShrub01 | WhiteOak01 |
|---|---|---|---|
| as is (plane reading everywhere) | 2.19 / 2.29 / 2.29 | 1.28 / 1.20 / 1.20 | 0.63 / 0.53 / 0.53 |
| level 1 angle from the parent's axis | 1.18 / 0.99 / 0.99 | 1.30 / 1.22 / 1.22 | 0.52 / 0.39 / 0.39 |
| level 2 angle from the parent's axis | 2.10 / 2.29 / 2.29 | 1.30 / 1.20 / 1.20 | 0.70 / 0.59 / 0.59 |
| no 6004 length spread | 2.34 / 2.52 / 2.52 | 1.28 / 1.20 / 1.20 | 0.63 / 0.54 / 0.54 |
| no 6000 bend | 2.83 / 2.75 / 2.75 | 1.24 / 1.22 / 1.22 | 0.67 / 0.58 / 0.58 |
| 6012 over [6010, 6011] only | 1.96 / 2.05 / 2.05 | 1.23 / 1.24 / 1.24 | 0.62 / 0.54 / 0.54 |

Which variants reach the billboard's range: the leaf cards are small on the bush
(0.04-0.05) and widen it less than they raise it (without them it is wider still, 2.29);
its x and y spans are within 4 % of each other; dropping the length spread, the bend or
counting 6012 over [6010, 6011] only leaves it at 1.96-2.83. Only the level-1 angle read
from the axis brings it into the range, and its stems are the case the plane reading fits
worst: 10-40° from the plane across an upright trunk is a plate of stems, 10-40° from the
trunk's axis a vase. Read from the axis on every tree, WhiteOak01 drops to 0.52
[observed, above], outside 10 % of its 0.62, while its branches on a 0.5 trunk fit the
plane reading (0.63).

Reading used [hypothesis: fitted to these three trees]: a level springs from a stub when
its shortest branch (smallest 6004 length minus spread) is at least twice its parent's
longest (largest length plus spread), and its 6007 angle is then measured from the
parent's axis; every other level keeps the plane reading. The factor 2 is a margin of
ours [design]: EuonymusBush01's stems are 10 times its trunk (0.1 over 0.01),
WastelandShrub01's half (0.05 over 0.1), WhiteOak01's level-1 branches about a seventh
of its trunk (0.09 over 0.65), so any factor above 0.5 and up to 10 splits them the same
way; 2
keeps a parent only a little shorter than its branches on the plane reading. Of the three
TREE records' trees only EuonymusBush01's level 1 qualifies, so WastelandShrub01 and
WhiteOak01 are built as before. The generator takes the spread as 1 − r on such a level,
which builds the branches of the measured "level 1 from the axis" row up to rounding:
EuonymusBush01 1.18 (within 10 % of 1.30), WastelandShrub01 1.28, WhiteOak01 0.63
[observed: `fnvtool spt shape` built at 258a20341, the first version of this reading,
which had no margin, 2026-11-21; the margin changes none of the three].

Other files: under the first version (any parent shorter than its shortest branch) the
reading also changed three `.spt` files that no TREE record of FalloutNV.esm uses,
oasistreetop01 (3.52 → 1.19) and oasiselm01/02 (2.52 → 1.57) [observed: the 258a20341
run];
which of the ten files qualify under the factor 2 is in `stub_levels` of `fnvtool spt
shape`. A mod (or TTW) whose TREE records use such a file gets the changed tree. The
probe's `angle_from_plane_everywhere` gives the generator before the change.

What changed in the game: both ratios are above 1, so the renderer sets the bush's width
to BNAM × the reference's scale (§5) before and after, and only its height changes. In
the Citadel planters (scale 0.87, BNAM 256) the bush is 223 units wide; it was about 102
tall (223 ÷ 2.19) and is now about 189 (223 ÷ 1.18). If the original's bush is that wide,
its billboard's 1.30 makes it about 171 tall: the bush was too flat rather than too wide
[hypothesis until the same view of a planter in both games]. To confirm: the bush in a
Tenpenny Tower or Citadel planter, the same view in both games.

Separate from the aspect: the renderer sets the tree's larger side to the whole BNAM size
(§5), while the billboards' opaque part covers 0.85-0.99 of the square (§4). If the
billboard shows the tree at the BNAM size, the original bush is about 0.85 × BNAM wide,
so NVForever's would be about 18 % larger [hypothesis].
