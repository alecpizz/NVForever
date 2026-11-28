# Spec notes: physics and the character controller (Layer 4, workstream 2)

Clean-room description in plain language. Tags: **[documented]** cites a public source,
**[observed]** was measured on the owner's install (FNV 1.4.0.525, the ten masters) or in
this repository's tests, **[hypothesis]** needs verification. Design and workstream split:
docs/design/layer4.md §2.8 and §7. Code: `crates/fnv-phys`, `crates/fnvtool/src/phys_cmd.rs`.

Status 2026-10-07: the world (terrain, reference bodies, characters), queries, the
character controller, dynamic bodies and fixed-step stepping are implemented and tested on
synthetic data; terrain is built from the real LAND data of every worldspace. Placed
references get their mesh collision through workstream 1's converter
(`fnv_collide::nif::collision_objects`, merged from `claude/layer4-collide`) and
CollisionMarker references their placed box or sphere (§4.1); `fnvtool phys cell` builds
both from the owner's install (§7).

## 1. Engine choice

`rapier3d` 0.36 with the `enhanced-determinism` feature (owner decision, layer4.md §2.8;
dependencies.md). That feature forces libm-based maths in its maths dependencies
(`simba/libm_force`, `parry3d/enhanced-determinism`) [documented: rapier3d 0.36
Cargo.toml]. Pure Rust, Apache-2.0, no Bevy coupling. rapier 0.36 uses `glam` vectors
(through `glamx`); `fnv-phys` exposes only plain `[f32; 3]` and row-major `[f32; 9]`
values so the Bevy engine (`engine/`) is free to use its own `glam` version.

## 2. Conventions

- **Units and axes**: the public API uses game units, Z up, rotations as row-major 3×3
  matrices rotating column vectors, like `fnv_collide::shape` and `fnv_world::transform`.
- **Internal scale**: rapier runs in metres. Several of its tolerances are absolute and
  sized for metres; the character controller, for example, counts a contact as ground
  only within `offset + 0.05` of the capsule [documented: rapier3d 0.36
  `control/character_controller.rs`, `predict_ground`], which is under one game unit.
  Run in game units, a character walking across flat terrain lost ground contact at cell
  seams [observed: tests]. Every length is divided by `PhysParams::units_per_metre` on
  the way in and multiplied on the way out. The factor is a numerical choice
  (default 70 [hypothesis: the commonly quoted size of a game unit, ~1.43 cm]); results in
  game units should not depend on it beyond rounding.
- **Placing a reference's collision**: a collision object's body sits at
  `reference ∘ node_transform ∘ body_transform`, where the reference transform is
  translation = position, rotation = `fnv_world::transform::ref_rotation` (world.md §4) and
  a uniform scale = XSCL. Scale is baked into the shapes (dimensions and child offsets) and
  into the node/body translations, because rigid bodies cannot scale. A reference's
  transform **replaces** its model's root transform (world.md §4) [observed for rendering];
  `fnv-phys` therefore assumes `CollisionObject::node_transform` is relative to the root
  *without* the root's own transform [hypothesis: to be confirmed with workstream 1, §8].
- **Shape trees**: rapier forbids composite shapes inside composites, so a
  `Shape::Compound` tree is flattened; each leaf (box, sphere, capsule, convex hull,
  triangle mesh) becomes one collider on the object's body with the leaf's composed pose.
  Leaves that fail (non-positive or non-finite size, degenerate hull, triangle index out
  of range, rotation not orthonormal within 1e-3 or with a reflection) are returned as
  errors and the rest of the body is kept; nothing is dropped silently.

## 3. Terrain

One collider per exterior cell from the LAND heights (`fnv_world::land`, world.md §3):
the 33 × 33 vertices 128 units apart from the cell's south-west corner. Each grid square
is split into two triangles along its **south-west → north-east diagonal**, the same split
the renderer uses (`engine/nvforever/src/terrain.rs`) [hypothesis: the original's split
is unknown; to be checked by standing a probe on a steep square in-game, §6].
`fnv_phys::terrain::height_at` states that surface analytically.

- **Triangle mesh, not rapier's heightfield.** rapier 0.36's heightfield missed vertical
  rays that run exactly along some interior grid lines: on 9 of the 31 interior
  north–south lines (lines 17–20, 22, 24, 25, 28 and 29 counted from the west edge) every
  one of 50 rays missed, on flat and bumpy fields and at two cell positions alike
  [observed: test program in this workstream]. Ground probes at whole
  multiples of 128 are common, so terrain is a triangle mesh with internal-edge fixing
  (`TriMeshFlags::FIX_INTERNAL_EDGES`).
  A different heightfield set-up did not miss [observed, branch `claude/layer4-phys-alt`]:
  heights passed with rows along world x (parry row `i`, column `j` = LAND row `j`,
  column `i`), the collider rotated so parry's x → world y, y → world z, z → world x, every
  cell set to parry's zig-zag split (south-west → north-east in that frame), in game units
  rather than metres: vertical rays at all 33 × 33 vertices of every cell of WastelandNV,
  BigMT, Zion and Freeside (about 27 million rays, all on grid lines) hit, with the
  answering cell's height equal to LAND exactly. The misses above may come from the axis
  mapping or the metre scaling; a heightfield needs less memory than a mesh, should that
  matter for streaming.
- **Check on the data** (`fnvtool phys terrain`, §7): for every landscape cell of every
  worldspace of the ten masters, 126 vertical rays per cell (64 scattered points, the 31
  interior vertices on the cell's diagonal and 31 points on grid lines) against
  `height_at`: 30 worldspaces, 41,983 cells, 5,289,858 rays, **0 misses, 0 rays hitting
  another collider**, largest error 0.19 units (WastelandNV cell (25, −59), about 240,000
  units from the origin, where f32 spacing is about 0.016 units) [observed].
- 484 LAND records carry no heights (no VHGT), e.g. WastelandNV (23, −65) and (24, −65),
  whose LAND holds only DATA flags and vertex colours [observed]; those cells get no
  terrain collision. What the original does there is unknown [hypothesis: they lie
  outside the playable area].

## 4. Bodies from collision objects

| Collision object | Body | Tag |
|---|---|---|
| `Motion::Static` | fixed | [hypothesis] |
| `Motion::Keyframed` | fixed (posed by teleport, never moved smoothly) | [hypothesis] |
| `Motion::Dynamic` | dynamic; collider densities scaled so the body mass equals `mass` (volume-derived mass kept when `mass` ≤ 0) | [hypothesis] |

Friction and restitution are passed to rapier unchanged [hypothesis: Havok's coefficients
and combine rules may differ from rapier's averaging]. Triangle-mesh colliders keep their
per-triangle Havok material code, returned by ray hits (footstep sounds, impact effects
later).

**Collision layers.** Every reference collider keeps its Havok layer
(`Owner::Ref::layer`). The layer numbers 0–42 and their names (0 Unidentified, 1 Static,
2 AnimStatic, 3 Transparent, 4 Clutter, ... 12 Trigger, 13 Terrain, 15 Non Collidable,
27 Invisible Wall, 30 Char Controller, 32 Collision Box, ... 42 Dropping Pick) are those of
the REFR XTRI field [documented: xEdit wbDefinitionsFNV.pas, re-expressed in
`schemas/fnv/world.json`; `fnv_phys::layers` checks its table against the schema]. That
mesh collision objects use the same numbering is [hypothesis] (collision.md §4). The
original's layer matrix (which layers collide with which) is unknown. Queries take
`Filter::exclude_layers`; the character controller ignores, by default, the layers whose
names describe volumes or picks rather than obstacles: Water, Trigger, Non Collidable,
Portal, Acoustic Space, Actor Zone, Projectile Zone, Cloud/Gas Trap, Avoid Box, Camera
Sphere, Door Detection, the pick layers, Spell Explosion, Debris Small and Shell Casing
(`layers::character_ignores`) [hypothesis: chosen from the names; capture question §6].

`fnvtool phys cell` skips deleted references, initially disabled ones and ones disabled
through their enable parent (the renderer's rule, `engine/nvforever/src/world.rs`):
disabled references are not drawn and have no collision [observed by the owner in game, 2026-10-08]. The
enable state at run time belongs to `fnv-sim`, which will add and remove bodies as
references change state. Models come from
the game's archives and loose files (`fnv_vfs`, default archive order) and are converted
once per model; SpeedTree models (`.spt`, trees) hold no Havok blocks and get no collision
[observed: they are not NIF files; whether trees block the player in-game is a capture
question, §6].

**Placed actors** (ACHR, ACRE: base NPC_ or CREA) get no collision from this rule. The
model such a base names is its skeleton, and the skeleton's collision objects are
keyframed bodies on the Biped layer, one per bone [observed: `fnvtool phys cell --cell
GSProspectorSaloonInterior --list-refs`, 2026-10-10, before this rule: Sunny Smiles,
the Goodsprings settler 0x00104F08 and Cheyenne each listed with Biped keyframed
objects at their placement]. They are the actor's own bodies and move with its bones
[hypothesis: not captured; they are what ragdolls are built from, ragdoll.md §1]. Added
as a reference's collision they stood, in the skeleton's bind pose, where the actor was
placed, and stayed there when it walked off or sat down [observed in play by the owner,
2026-10-10: the settler sat on a saloon stool, its collision still where it had stood].
`refs.counts` reports them as `actor`. What stands in the player's way instead is in
§6.8; shots hit the actor's hit volumes (combat.md §5.2). On the owner's install the rule leaves out 3 actors with 57 colliders in the Prospector
Saloon (316 → 313 references with collision) and 11 actors with 173 colliders around
Goodsprings (838 → 827 references, 1,367 → 1,194 colliders: Easy Pete, two settlers, five
bighorners, three ravens) [observed: `fnvtool phys cell --list-refs`, 2026-10-10].

On the owner's install, Goodsprings with the cells around it (radius 1): 1,136 placed
references; 839 get collision (1,369 colliders from 167 of the 194 distinct models, none
unreadable), 211 use a model without collision objects, 43 are trees, 25 have no model, 8
are initially disabled, 10 are disabled through their enable parent, 8 CollisionMarker
primitives [observed: `fnvtool phys cell --cell Goodsprings`, 2026-10-08; before the
enable-parent rule the 10 were included, 849 references and 1,430 colliders]. 64 probe
characters dropped on an 8 × 8 grid over the cell all come to rest, 21 of them on
references (rocks, buildings), the others on terrain [observed, before the enable-parent
rule].

### 4.1 Placed collision primitives (CollisionMarker)

Some references carry their own shape instead of a model: REFR XPRM holds bounds (three
floats), a colour and a type (1 box, 2 sphere, 3 "portal box"), and XTRI a collision
layer [documented: xEdit wbDefinitionsFNV.pas REFR]. In the ten masters [observed:
census of the winning REFR records]:

| Base object | XPRM type | References |
|---|---|---|
| STAT 0x00000021 "CollisionMarker" | 3 | 2,901 |
| STAT 0x00000021 "CollisionMarker" | 1 (box) | 1,155 |
| STAT 0x00000021 "CollisionMarker" | 2 (sphere) | 149 |
| 0x00000020 (no record; portal) | 3 | 1,228 |
| STAT 0x0000001F "RoomMarker" | 1 | 1,044 |
| 0x00000017 (no record; occlusion plane) | 3 | 264 |
| ACTI trigger activators (e.g. `NVDLC01ToxicCloudTrigger`) | 1 | ~1,000 |

8,819 references have XPRM; only 3 have XTRI. So primitives cannot be selected by layer:
`fnv-phys` turns the primitives of **CollisionMarker** references into static colliders
(types 1 and 3 as boxes, type 2 as a sphere) [hypothesis: CollisionMarkers are the invisible
walls; the name suggests it], and leaves portals, rooms, occlusion planes and trigger
activators out (they are culling volumes and script triggers by their bases [hypothesis];
triggers will become sensors when scripts need them, Layer 5). A primitive without XTRI
is an Invisible Wall (#1545, #1665): it stops actors, and shots pass it
(`layers::shot_ignores`). So is one whose XTRI is **Transparent** (#2223): TTW's load
order has 277 references with XTRI, Transparent the commonest (96, then Invisible Wall
60, Actor Zone 57, Static 31) [observed: `fnvtool esm fields`, 2026-11-12], and its
Vault 101 BB gun range has a Transparent Portal Box marker across the hall between the
player and the targets the birthday quest needs hit [hypothesis: the original lets
shots through such a marker]. Other XTRI layers are kept as stored.

**Bounds are read as half extents** (sphere: the first value is the radius). Evidence
[observed]: for the 3,007 exterior CollisionMarker boxes without x/y rotation, the box
bottom against the terrain under the reference: read as half extents, 24.4% of the boxes
float above the ground (median bottom 115 units *below* it); read as full extents, 76.9%
float (median bottom 160 units *above* it), which would leave walls the player could walk
under. Still a [hypothesis] until checked in game (§6). Reproduce:
`cargo run --release -p fnv-phys --example primitive_bounds -- <Data> schemas/fnv`.

**Layer** (issue #1545, 2026-11-05) [hypothesis: what a CollisionMarker is for]: a
CollisionMarker primitive without XTRI is on the **Invisible Wall** layer (27), not
Unidentified (0). Characters collide with it, as before. Shots, VATS mode's line of fire
and the aim ray pass it (`layers::shot_ignores`, the character's layers plus Invisible
Wall and, since #2340, Transparent; melee swings take `layers::melee_ignores`, without
Transparent: docs/spec-notes/combat-rules.md), and so do sight (`detection.rs`
`see_through`) and blast cover lines, which already passed layer 27. Before the change, a 10mm round fired at the box 0x00176AD0 in Goodsprings
stopped on it (smoke `shot-passes-collision-marker`).

### 4.2 Collision layer matrix (B11): design and predictions

Which collision layers stop the player is unknown (§4); `layers::character_ignores` is a
choice from the names. In and around Goodsprings the references with collision use the
layers Unidentified (0, CollisionMarker primitives), Static, AnimStatic (keyframed doors,
containers), Clutter and Props (dynamic), Biped (the ragdoll bodies of placed actors'
skeletons), Trees (Havok trees), Terrain (rocks), Non Collidable (plants, static and
dynamic) and Transparent Small (yuccas, chain-link fences) [observed: `fnvtool phys cell
--cell Goodsprings --list-refs`]. Note: placed NPCs and creatures get the bodies of their
skeleton models as keyframed obstacles on the Biped layer (173 objects here); in game
actors have their own character controller [hypothesis], so those bodies probably should
not stop the player; not tested by B11 (actors move).

Test (`parity-lab/inputs/layer4/b11_goodsprings.drops.json`, scenario
`L4.phys.drops.b11_goodsprings`, job `mv_b11_drops`): the player is put 200 units above
the top a vertical ray finds at a reference's position (300 above a SpeedTree's base) and
falls; its final feet are compared with `fnvtool phys cell --drops-file` (tolerance 8,
because `SetPos` drops in game rested 0–7.5 above the contact shell, §6.2). Predictions of
the candidate, written 2026-10-08 before any capture of this job (character ignores the
§4 list; dynamic Props start at rest):

| Drop | Target (layers) | Drop z | Top z | Predicted feet z | Predicted support |
|---|---|---|---|---|---|
| collision_marker_a | 0x0010AE97 CollisionMarker (Unidentified) | 8573.0 | 8373.0 | 8373.50 | 0x0010AE97 (Unidentified) |
| collision_marker_b | 0x0011E6E6 CollisionMarker (Unidentified) | 8771.0 | 8571.05 | 8571.53 | 0x0011E6E6 (Unidentified) |
| collision_marker_c | 0x00176AD0 CollisionMarker (Unidentified) | 8951.0 | 8750.99 | 8751.49 | 0x00176AD0 (Unidentified) |
| static_fence_rail | 0x0017B647 NVFencePickBurntMid01 (Static) | 8414.4 | 8214.4 | 8215.11 | 0x0017B647 (Static) |
| static_mailbox | 0x00107583 MailboxDestroyed (AnimStatic, Static) | 8652.8 | 8452.8 | 8453.61 | 0x00107583 (Static) |
| animstatic_gate | 0x0010757E NVFencePickBurntGate (AnimStatic, Static) | 8969.4 | 8769.39 | 8770.10 | 0x0010757E (AnimStatic) |
| havok_tree_a | 0x001076CC TreeHardwoodPebblesDirt01 (Trees) | 8756.5 | 8556.49 | 8582.72 | 0x001076CC (Trees) |
| havok_tree_b | 0x00107504 TreeHardwoodPebblesDirt01 (Trees) | 8987.9 | 8787.87 | 8813.61 | 0x00107504 (Trees) |
| props_crate | 0x0010B8D0 NVGSCrateContainer (Props) | 8616.0 | 8415.96 | 8417.45 | 0x0010B8D0 (Props) |
| props_barrel | 0x0016CCF5 NVWoodBarrel (Props) | 8614.5 | 8414.49 | 8415.18 | 0x0016CCF5 (Props) |
| terrain_rock_a | 0x001075A0 NVNRockCanyon08 (Terrain) | 8523.0 | 8323.01 | 8323.62 | 0x001075A0 (Terrain) |
| terrain_rock_b | 0x00107587 NVNRockCanyon08 (Terrain) | 8686.4 | 8486.44 | 8515.13 | 0x00107587 (Terrain) |
| noncollidable_agave | 0x0010B1C5 nevadaagave02 (Non Collidable) | 8904.2 | 8704.21 | 8691.51 | terrain |
| noncollidable_maize | 0x0010B1BB MaizePickable (Non Collidable) | 8989.2 | 8789.22 | 8706.19 | terrain |
| noncollidable_agave_pickable | 0x0016AF20 NevadaAgavePickable (Non Collidable) | 8748.0 | 8548.04 | 8529.83 | terrain |
| noncollidable_maize_group | 0x0010638F MaizeWitheredGroup2b (Non Collidable) | 8766.7 | 8566.68 | 8499.50 | terrain |
| transparent_small_fence_end | 0x001075CC CLFenceEndLT01 (Transparent Small) | 9180.3 | 8980.35 | 8981.05 | 0x001075CC (Transparent Small) |
| transparent_small_yucca | 0x0010A17D YuccaCNV (Transparent Small) | 8603.0 | 8403.0 | 8404.40 | 0x0010A17D (Transparent Small) |
| speedtree_shrub | 0x00106357 WastelandShrub01 ((SpeedTree, no Havok blocks)) | 8639.7 | - | 8354.61 | terrain |
| control_open_ground | - LAND (terrain) | 8552.0 | - | 8355.51 | terrain |

So the candidate predicts: every layer stops the player except Non Collidable (agave,
maize) and the SpeedTree shrub (no collision in our world); dynamic Props hold the
player at least while it lands. A difference of more than 16 units on any row changes
the matrix. The clutter row (job `mv_b11_clutter`: a whiskey bottle, Clutter layer,
placed with `PlaceAtMe` and moved 200 units above eleven of these targets) has no
candidate yet: dynamic bodies collide with every layer in `fnv-phys` today.

**Results** (capture `mv_b11_drops`, run by the coordinator 2026-10-08 on the re-made
`nvfdoc`; golden `phys/b11_goodsprings.drops.json`) [observed]. The first comparison
passed 12 of 20 rows; what the differences showed:

| Row | Game feet z | Candidate before → now | Finding |
|---|---|---|---|
| collision_marker_a | 8484.33 | 8373.50 → 8484.74 | the player rested on the Goodsprings school (SCOL 0x00177C9E, placed in the next cell south), which the probe's world lacked: `phys cell` now loads the cells around each probe's own square too [hypothesis: the game loads cells around the player wherever it is] |
| animstatic_gate | 8691.58 (on the terrain, x/y unchanged) | 8770.10 → 8691.51 | the gate 0x0010757E carries ONAM "Open by Default" [documented: xEdit]: its panel is open in game. The keyframed parts of open-by-default doors are left out (their open pose comes from the door's animation, not read) |
| collision_marker_b | 8285.81 (fell through, x/y moved 0.1) | 8571.53 (unchanged) | a Portal Box marker whose box is 2 units thick: dropped onto its top the player fell through it; but a run into another Portal Box marker stopped 22 units from its plane (`L4.phys.walk.portal_wall`, §6.4), so Portal Box markers stay solid. Open: why the drop passed |
| havok_tree_a / _b | 8637.98 / 8879.92 | 8582.72 / 8813.61 → 8624.07 / 8869.53 (§6.4.1) | still open, 13.9 and 10.4 low. The game's player rests 55 and 66 higher in the branches of `treewastelandhardwood01.nif`; a capsule of radius 21 or 24 does not explain it (8590, 8598); a dropped bottle came to rest at 8564.07 on tree a, near our top at the centre (8556.5), so the geometry is probably right and the player's shape against branches differs [hypothesis]. Diagnostic grid `mv_diag_trees_rocks` below |
| terrain_rock_a / _b | 8333.09 / 8506.51 | 8323.63 / 8515.13 | rock b: the game's own spread (six drops over 7.8 units); rock a: the candidate lands on a step's edge and slides down it, open (follow-up below) |
| noncollidable_maize | z 8705.24, x/y moved 14 | x/y unchanged | a landing slide downhill on the 5° LAND that the game makes in about one landing in four or five; not the maize (follow-up below) |
| props_barrel | not observed | — | the player did not fall: it hung at 8615.39, 9 units sideways (golden row without feet) |

Now 15 of 20 rows agree (14 since Portal Box markers became solid again, §6.4, which
brought back the `collision_marker_b` difference). **The player row of the matrix** [observed]: Static, AnimStatic
(the mailbox, a closed gate part), Trees (Havok), Terrain (rocks), Transparent Small
(chain-link fence end, yucca), Props (a crate, at least while landing) and CollisionMarker
boxes stop the player; Non Collidable plants (static and dynamic) and SpeedTree shrubs do
not. `layers::character_ignores` already agrees on every layer tested; nothing changed.
Not tested: Water, Trigger, Invisible Wall, Collision Box, Debris, Biped (placed actors'
skeleton bodies, §4.2 note).

**Rock and maize follow-up: facts and predictions** (written 2026-10-08 before the captures
`mv_diag_maize` and `mv_diag_rocks`, `tools/capture/jobs/make_b11_followup_jobs.py`; drops
in `diag_maize.drops.json` and `diag_rocks.drops.json`; each drop is traced every game
loop from the moment the player starts to fall, followed by a scan for actors within 600
units).

Facts gathered for the maize row [observed: `fnvtool phys cell --list-refs`, `fnvtool
world refs`, `fnvtool collide objects`, `fnvtool nif decode`, rays]:

- The game's player ended at (−74583.1, 1406.89): moved (+9.90, −9.91), exactly along a
  diagonal. The LAND there is one plane over squares (25, 10)–(26, 12) of cell (−19, 0),
  sloped 5.05° with its downhill direction (+0.062, −0.062): **the push points straight
  downhill**. The rest height at the new point is 3.87 above LAND, the usual terrain band
  (§6.2).
- But the B11 `noncollidable_agave_pickable` drop landed on LAND of the same 5.05°
  slope (downhill (−0.062, −0.062)), from 218 units instead of 284, and did not move;
  the `speedtree_shrub` drop (3.6° slope, 285 units) did not move; `collision_marker_b`
  fell through onto an 18.6° slope and moved 0.15. So a slope alone does not make the
  player slide after a `SetPos` drop.
- `maize_healthy01.nif`'s only collision is a box of 12 triangles (packed strips under a
  MOPP), 7 × 9 × 87 units, layer 15 (Non Collidable) on the body and on its one sub-shape
  [documented: sub-shape filter field, nif.xml `hkSubPartData`]; its top is flat at
  z 8789.2 and covers the drop point (rays every 2 units). The agaves are layer 15 convex
  hulls (33 × 30 × 16) that the player fell straight through. Were the maize box solid to
  the player, a drop on its axis would rest on its flat top (8789.9), not on the ground.
- Placed references within 300 units (any type, also without collision in our world):
  only plants: `MaizeWitheredGroup1` 0x0017747D (STAT, 67 units north, no collision
  object in its mesh), six agaves, three honey mesquites and one agave pickable (130–300
  units away). No markers, SpeedTree shrubs or actors are placed there; actors can walk
  by (Goodsprings has wandering NPCs and creatures).

Hypotheses and what each predicts for `mv_diag_maize` [hypothesis]:

- **H-actor** (something moving, e.g. an actor, pushed the player): the push does not
  repeat at the same place; when it happens an actor is within reach (actor scan).
- **H-slide** (a landing slide on the slope whose occurrence depends on the landing): the
  trace shows the sideways motion starting at touchdown (feet ≈ 8706) and running downhill
  (+x, −y); it also happens on the bare slope 49 units away (`slope_bare_*`) and with the
  maize disabled; larger from higher drops.
- **H-stalk** (the maize box acts on the player after all): sideways motion starts near
  the box top (feet ≈ 8790) or, for the drop started below the top (`maize_inside`), at
  once; drops ±30 from the axis and drops with the maize disabled fall straight.
- The candidate (Non Collidable ignored, no landing slide) predicts every drop straight
  down: maize_c1–c3, maize_high, maize_low, maize_inside, maize_disabled_1/2 at
  (−74593.0, 1416.5, 8706.19); maize_+30x 8704.31; maize_−30x 8708.06; maize_+30y
  8708.05; maize_−30y 8704.31; slope_bare 8701.82; agave_pickable 8529.83.

For the rocks: the candidate's centre drops end at rock a (−69874.3, 714.7, 8323.62),
having slid (+3.3, −3.9) on a 10.6° face to a capsule rest 0.6 above the surface, where
the game's B11 drop stayed at the drop point 9.5 higher; on rock b (−69656.2, 1619.8,
8515.13), 29 units above the surface under the axis (the capsule's side rests on a higher
part of the rock), where the game's two drops gave 8506.51 and 8512.78. Open ground
8355.51 (game 8355.85). Rock a grid (candidate z): −30/−30 8340.90, −30/+0 8347.03,
−30/+30 8348.76, +0/−30 8336.33, +0/+30 8353.62, +30/−30 8328.27, +30/+0 8318.60,
+30/+30 8318.43. **H-spread**: the four repeats on each rock scatter over several units
(rock b already 6.3) and the candidate lies inside or near that scatter; **H-systematic**:
repeats agree within about 1 unit and stay 9.5 above (rock a) / 8.6 below (rock b) the
candidate; on rock a a systematic difference would come with the game not sliding where
the candidate slides (the grid shows where the rock's surface rises within the capsule's
radius). Every drop's trace is checked for the `SetPos` hang (resting 7–11 above the
support, §6.2).

**Results of the follow-up** (captures `mv_diag_maize`, `mv_diag_rocks`, run unattended by
this workstream 2026-10-08 on `nvfdoc`, 0 failed steps) [observed]. Two drops are left out:
`maize_disabled_1` stopped in mid-air 100 units above the ground three loops into its fall
and stayed there while the clock ran (the `SetPos` hang, §6.2), and `agave_pickable_2`
stopped 39 units up with the game clock frozen too (the world standing still after a
`SetPos`, §6.3). No actor was within 600 units at any drop (the nearest 1,743–2,128 units
away).

Maize (13 usable drops on the same 5.05° LAND plane, plus the B11 drop):

| Drop | Game feet (x, y, z) | Moved (x, y) | Touchdown loop |
|---|---|---|---|
| maize_c1 | (−74593.0, 1416.8, 8706.48) | 0 | 32 ms, 8.65 above rest |
| maize_c2 | (−74593.1, 1416.78, 8706.48) | (−0.10, −0.02) | 31 ms, 11.81 |
| maize_c3 | (−74580.9, 1403.66, 8704.90) | **(+12.10, −13.14)** | 31 ms, 4.14 |
| maize_high | (−74591.5, 1414.97, 8706.27) | (+1.50, −1.83) | landed in a 31-ms loop, moved in the next (63 ms) |
| maize_low | (−74593.0, 1416.8, 8706.48) | 0 | 31 ms, 5.39 |
| maize_inside | (−74593.0, 1416.8, 8709.53) | 0 | 15 ms, 3.71 (ended 3 above the band) |
| maize_+30x | (−74552.0, 1404.7, 8703.16) | **(+11.00, −12.10)** | 31 ms, 5.49 |
| maize_−30x / +30y / −30y | 8708.35 / 8708.35 / 8704.60 | 0 | 31–47 ms |
| slope_bare_1 / _2 | (−74560.0, 1380.0, 8702.12) both | 0 | 31 ms / 62 ms |
| agave_pickable_1 | (−67457.0, 7781.1, 8530.12) | 0 | 32 ms (B11: the same) |
| maize_disabled_2 | (−74593.0, 1416.79, 8706.48) | (0, −0.01) | 15 ms |

- **The push is a landing slide, not the maize**: it came in 3 of 14 drops on that plane
  (B11, c3, +30x) and a small one in `maize_high`; always downhill (+x, −y), within
  3° of the slope's direction (the small one 6°); always in the single loop of touchdown (c3: 4.0 down and 16.4 sideways in
  one loop, then 1.7 more in the next and nothing after; +30x likewise), so the vertical
  drop plus the sideways move of that loop add up to about the 19 units the fall speed
  would have carried the player in it. It happened 30 units from the maize's axis
  (`maize_+30x`, the capsule clear of the 7 × 9 box), so **H-stalk is refuted**; the
  repeats at the B11 point slid once in three, so whether it happens is the game's own
  spread, and **H-actor is refuted** (no actor near). A long touchdown loop (31 ms and
  more) came with every slide but also with seven straight landings; what decides it is
  not known. One valid drop with the maize disabled fell straight, which says nothing
  given the rate.
- So the B11 row's golden (moved 14) shows a slide that the game makes in roughly one
  landing in four or five here; the candidate falls straight, as the game did in 10 of 14
  drops. Nothing in the candidate changed [not modelled: a random landing slide; a model
  would need the rule that triggers it].

Rocks (repeats at the B11 drop points, interleaved; x/y unchanged unless noted):

| Target | Game feet z (drops) | Range | Mean | Candidate |
|---|---|---|---|---|
| rock a 0x001075A0 | B11 8333.09; 8333.45, 8329.03 (moved 1.16), 8336.16, 8327.10 (moved 3.46) | 9.06 | 8331.77 | 8323.62 (slid 5.1) |
| rock b 0x00107587 | B11 8506.51 (moved 2.8); `mv_diag_trees_rocks` 8512.78; 8512.59, 8514.35, 8510.37, 8512.63 | 7.84 | 8511.54 | 8515.13 |
| open ground (control) | B11 8355.85; 8355.85 four times | 0 | 8355.85 | 8355.51 |

Rock a grid (game / candidate z; the game moved 0 at every point): −30/−30 8339.97 /
8340.90; −30/+0 8344.17 / 8347.03; −30/+30 8349.07 / 8348.76; +0/−30 8334.76 / 8336.33;
+0/+30 8353.51 / 8353.62; +30/−30 8326.12 / 8328.27; +30/+0 8318.65 / 8318.60; +30/+30
8324.53 / 8318.43 (the candidate slid 6.3 there). 7 of 8 within 3 units.

- **Geometry, Havok scale and shells are right on both rocks**: the grids agree within 3
  (rock a, 7 of 8) and 4 (rock b, 7 of 9) units; flat ground repeats to 0.01 and the
  candidate is 0.34 under it (the band of §6.2); the differences below are 3–20 units,
  far more than any shell (0.7 on the rock, 3.5 on terrain).
- **Rock b is within the game's spread**: four drops at the same x/y rested at four
  heights over 4 units, six drops over 7.8; the candidate is 0.8 above the highest, 3.6
  above the mean, and the B11 golden is the lowest of the six (the one drop that moved).
  On flat ground identical drops agree to 0.01, so the spread comes with resting on the
  rock's uneven surface [observed], not with the drops themselves.
- **Rock a is partly systematic**: the candidate is 3.5 below the lowest of five game drops
  and 8.2 below their mean. Cause [observed: `fnvtool phys walk` trace of the same drop]:
  27 units above the surface under the axis a near-vertical step (face normals with
  z 0.11–0.19) runs about 12.7 units from the drop axis; the candidate's capsule
  (radius 18) touches the step's top edge at 8343.15, is flagged sliding, and creeps down
  the face at about 11 units/s for 1.8 s to 8323.63. The game's player comes to rest at
  touchdown somewhere on that face (8327.10–8336.16) and does not move in the next 300
  loops; in drop c1 it passed 8341.34 without being stopped, where the candidate's
  capsule already stands on the edge. Both the higher first contact and the slow slide
  differ. No single change fits [hypothesis, checked with `phys walk` radius overrides]:
  a capsule that stops where it lands would stay at 8343 (10 above the B11 golden), a
  radius of 12 or less misses the edge and lands at 8323.9; the game's rest heights look
  like "stops where the touchdown loop ends, on a face the candidate's shape cannot reach
  that low" [hypothesis]. Same family as the Havok tree rows (the game rests on steep
  branch faces where rapier's controller slides off, §6.4), which the jump-trees
  workstream owns; a shape measurement at a clean vertical edge is designed as backlog
  B13 (layer4-captures.md §3). **Update 2026-10-08 (jump-trees workstream)**: B13 was run
  as `mv_diag_fence_edge` / `_low` (§6.4.1); with the body derived from it (radius 22, cone
  bottom, no slide while falling) the candidate's rock a centre drop rests at 8333.87,
  inside the game's five-drop range 8327.10–8336.16, without sliding off the step edge.
- No change made to the candidate and no tolerance or golden touched. **Question for the
  owner** (scenario design is the owner's): rock b and maize differ from their goldens by
  the game's own spread (rock b 7.8 units over six identical drops; the maize slide in 3
  of 14 landings); with one capture per row and a tolerance of 8, such rows pass or fail
  by which landing the capture caught. Options: (a) keep the rows and their goldens as
  they are (they stay red until the candidate models the spread's cause); (b) build drop
  goldens from repeated drops (e.g. the median of 4–6) with a tolerance set from the
  observed spread; (c) compare only z for drop rows, or move rows on uneven surfaces
  (rocks, slopes) to a separate scenario. Rock a would fail under any of these (the
  candidate is outside the game's range).
- **Owner decision (2026-10-08): option (b), the median of repeated drops.** The B11
  golden is rebuilt from five repeats of every drop, captured in two game sessions
  (`mv_b11_drops_rep_a`, 3 rounds, and `mv_b11_drops_rep_b`, 2 rounds; each round runs
  all 20 drops in order, so the repeats of a drop are minutes apart): per row, the median
  of the repeats that fell, per coordinate (`capture_moves.py golden … --also …`, which
  also records each repeat and the spread). The median also keeps a single `SetPos` hang
  (§6.2) or a single landing slide out of the golden. It goes to a new file,
  `phys/b11_goodsprings.median.drops.json`; the first capture's golden stays as it was.
  The tolerance stays 8 (the owner chose the median; a tolerance from the spread was
  not decided).

  Predictions for the median golden (written before the capture; the candidate is
  unchanged): rock b passes (the game's six earlier drops have median 8512.6, the
  candidate 8515.13); the maize row passes (the game fell straight in 10 of 14 drops;
  median x/y at the drop point, z near 8706.5 against 8706.19); rock a fails (earlier
  median 8333.09 against 8323.62); the Havok tree rows fail (43–303 units at every
  point of the grid, §6.4); the other 13 observed rows still pass. Open bets:
  `collision_marker_b` (the game fell through the 2-unit Portal Box once; if that was
  chance, the median rests on its top, 8571.53 in the candidate, and the row passes) and
  `props_barrel` (no reading yet; the candidate rests on the barrel at 8415.18). So 16 of
  20 rows with the two bets lost, up to 17 of 19 observed if they are won.

  **Results** (captures `mv_b11_drops_rep_a` and `_rep_b`, 2026-10-08, this workstream,
  `nvfdoc`; 7 failed steps, all "never started to fall", kept out of the median)
  [observed]: 16 of 20 rows pass, as predicted; both bets lost. Per row (repeats that fell,
  median z, spread of z):

  | Row | n | Median z | Spread | Candidate | |
  |---|---|---|---|---|---|
  | collision_marker_a | 5 | 8484.15 | 2.70 | 8484.74 | pass |
  | collision_marker_b | 5 | 8285.65 | 0.02 | 8571.53 | fail: fell through all five times, so not chance |
  | collision_marker_c, static_fence_rail, static_mailbox | 5 | 8752.04, 8215.44, 8453.86 | 0 | | pass |
  | animstatic_gate | 5 | 8691.85 | 0.26 | 8691.51 | pass |
  | havok_tree_a / _b | 5 / 4 | 8638.39 / 8879.15 | 4.2 / 4.2 | 8582.72 / 8813.61 | fail (jump-trees workstream) |
  | props_crate | 4 | 8423.54 | 2.30 | | pass |
  | props_barrel | 0 | — | — | 8415.18 | the player never started to fall, five times out of five (it hangs at the drop point, 9 units sideways as in the first capture); row without feet |
  | terrain_rock_a | 5 | 8333.27 | 6.61 | 8323.62 | fail (−9.65); four of five within 0.5 of 8333.5 |
  | terrain_rock_b | 5 | 8508.04 | 6.10 | 8515.13 | pass (+7.09) |
  | noncollidable_maize | 5 | 8706.48 | 0 | 8706.19 | pass: fell straight all five times |
  | the other seven rows | 5 | | 0 | | pass |

  **With the jump-trees body** (radius 22 on a cone bottom, a falling body that does not
  slide off steep contacts; §6.4.1) [observed, 2026-10-08]: **17 of 20**. terrain_rock_a
  8333.87 (+0.60, passes), terrain_rock_b 8512.99 (+4.95, passes), noncollidable_maize
  8706.41 (−0.07), havok_tree_a / _b 8624.07 / 8869.53 (−14.32 / −9.61, fail),
  collision_marker_b unchanged (8571.55, fail: the body still stops on the Portal Box),
  props_crate −5.96, the other rows within 2.7; props_barrel still has no game row. The
  diagnostics with this body: `mv_diag_rocks` repeats all within the game's spread (rock a
  +0.42 … +6.77 against four drops spread over 9.1, rock b −1.36 … +2.62), the rock a ±30
  grid within 2.4 at all eight points, the rock b grid within 3 at seven of nine (+4.9 at
  −30/+0); `mv_diag_maize`: the straight drops within 0.1, the game's landing slides
  (3 of 14) and the two hangs not modelled, as before.

  Rock b passes with 0.9 to spare: over eleven game drops now (8506.51–8514.35; median
  8510.37) the candidate stays at the top of the range, so the row stays sensitive to which
  five drops a capture catches; recorded, tolerance left alone. Rows on statics, terrain,
  plants and markers repeat to 0.3 units or exactly; the spread lives on the
  uneven surfaces (rocks, trees) and the dynamic crate. Rebuild:
  `python tools/capture/capture_moves.py golden C:/fnv-private/capture/out/mv_b11_drops_rep_a.<time>.jsonl parity-lab/inputs/layer4/b11_goodsprings.drops.json --also C:/fnv-private/capture/out/mv_b11_drops_rep_b.<time>.jsonl -o C:/fnv-private/goldens/phys/b11_goodsprings.median.drops.json`.

**Clutter row** (`mv_b11_clutter`) [observed]: the bottles bounced or rolled 50–300 units
from the drop point (the control drop on open ground ended 183 units away), so most rows
cannot be read. Two can: on Havok tree a the bottle stayed at (-74300.6, -1170.0, 8564.07)
and on rock a it rested at 8325.87, on the rock; Trees and Terrain stop clutter too.

### 4.3 Dropped items (references created at run time)

`engine/nvf-play` gives an item dropped from the inventory (a reference `fnv-sim` creates,
FormID from 0xFF000000) the collision objects of its base form's model through
`PhysWorld::add_ref`, at the simulation's pose with scale 1, loaded and unloaded with the
physics cell containing it. Dynamic objects fall and settle like placed clutter; while the
body moves, the reference pose `R = R_body R_localᵀ`, `t = t_body − R t_local` (local = the
object's node and body transforms) is written back to the simulation, for models with
exactly one collision object. Earlier evidence was mixed (a scripted `player.Drop` read
the same z for 10 s in `ld_drop_refs`, simulation.md §5 "After the drop"); **settled
2026-10-09: dropped items fall** [observed, capture `pc_drop_fall` (tools/capture/jobs/
make_closeout_jobs.py), save nvfdoc, just outside Doc Mitchell's front door, the player's
feet at 8693.43 on terrain at about 8690, heading 72.8°]:

| Case | Reading (GetPos, GetAngle X Y Z) |
|---|---|
| scripted `player.Drop` stimpak | 5 loops after: z 8705.9 (the drop point is 8723.4, 30 above the feet); from 1 s on at rest at (−73109.3, 1334.4, 8696.82), angles (86.7, −17.2, 0.5): on its side, 7.7 units from the drop point |
| the same, `SetPos Z` +1 and then +40 | stays where it is put (8697.8, then 8737.8, for 3 s each): a resting dropped item is not woken by `SetPos` |
| `Drop` with the Pip-Boy open (`TapControl 14`, `MenuMode` 1) | in the menu it stays at the drop point (−73115.2, 1327.2, 8723.43), angles (0, 0, 72.81) = the player's heading; after `CloseAllMenus` it falls (8704.9 after 5 loops) and rests at (−73109.6, 1335.1, 8696.93), angles (86.4, −16.3, 0.7) |
| `PlaceAtMe` stimpak | created at the player's feet, turned at random, rests at 8695.5 |
| scripted `Drop` of a tin can | falls, rolls 30 units, at rest on its side at (−73102.8, 1296.5, 8696.99), angles (86.4, 31.2, 59.8) |
| the player runs 40 loops over the resting stimpak | it moves 2.7 units and turns (angle Y −15.7 → 23.0); the player is not stopped (216 units in 0.67 s) |

So a drop starts at its drop point turned as the holder faces (X and Y 0, Z the holder's
heading), falls under its model's collision when the world runs, and rests lying down.
`nvf-play` now turns a dropped item to the player's heading (`menu.rs`; `World::drop_item`
leaves 0). Why `ld_drop_refs` read no fall is not known [hypothesis: its reference scan
may have read a reference whose 3D was not loaded].

**Clutter rests on the terrain's contact shell** (since 2026-10-09; `PhysWorld::
insert_terrain` gives the terrain collider a contact skin of `TERRAIN_SHELL`, 3.5): the
same drops in `fnvtool phys walk` with the item as a prop at the game's drop point and
heading (§7) rested 4.2 units lower than the game's stimpak and stood the tin can upright
1.4 units above the terrain; with the skin the stimpak rests at (−73111.2, 1334.7,
8696.21), angles (86.4, −21.9, 0) against the game's (−73109.3, 1334.4, 8696.82), (86.7,
−17.2, 0.5), and the can lies on its side at 8697.47 against 8696.99 (it rolls 17 units
less) [observed; that the excess is the shell the player rests on (§6.2) is the same
hypothesis]. Statics keep no skin for dynamic bodies (no capture of clutter resting on a
static yet). Parity-lab L4 results are identical with the skin (the player's controller
does not use contact skins).

The skin needs a wider contact prediction distance (since 2026-10-11; `PhysWorld::new`
sets rapier's to its default 0.02 m plus `TERRAIN_SHELL`, 0.07 m): with the default,
clutter resting on the terrain never fell asleep and crept about 10 units a second, the
owner's "bikes and crates jittering" outside the Prospector Saloon [observed: in `fnvtool
phys cell` around WastelandNV (−17, 0) the three Motorcycle01Static references, two
NVGSCrateContainer (one stacked on the other) and an NVWoodBarrel were all awake after
20 s, one motorcycle 52 units from where it was added; a terrain skin of 1.4 units rested
them, 2 did not]. With the wider distance all were asleep within 2 s, 3.5 units higher
than without the skin. Why a skin wider than the prediction distance keeps bodies moving
is not known (rapier's narrow phase) [hypothesis: contacts dropping in and out of the
solver at the skin's edge]. Test `clutter_on_the_terrain_shell_falls_asleep`. Since
2026-11-13 the distance is widened only while the world holds terrain: indoors the wider
one kept clutter trembling awake (§5.5).

In our engine [observed, `nvforever --play`, 2026-10-08]: a stimpak (one dynamic Clutter
object) dropped 30 units above Doc Mitchell's floor (z 7360) came to rest on its side at
z 7360.97, 29.7 lower and 8.7 units from the drop point, 1.58 s after the menu closed
(its body asleep); dropped just outside the front door (the drop point the game reported
for the same spot within 0.3 units, simulation.md §5) it came to rest on the ground at
8692.71, 30.9 lower. Repeated runs came to rest within 0.1 units of each other (the steps
are fixed, but how many run per frame depends on frame times).

**Sinking through the terrain (#2753: the corpses' cause found, the weapons' open).** Players' logs show weapons dropped by dying
NPCs and corpses at rest on the terrain in exteriors going through it, the same creatures at
the same spots on many installs [observed: player reports, build 0.7.0, issue #2753]; the
safety nets (`dropped 0x… fell through the world`, 10,000 units under the drop point, and
the ragdoll's `fell through at`, 150 under the terrain under its first bone) put them back.
Synthetic fnv-phys experiments did not reproduce it (square reloads, waking, the contact
shell, characters, slow motion, shots, blasts). Diagnostic lines (`engine/nvf-play`
`phys/sinking.rs` [design, #2753]): `dropped 0x… woke after resting S s … touching …` when a
resting drop moves again, with what its colliders touch; `dropped 0x… sank: …` and `ragdoll:
actor … sank: bone …` once, the first frame a body's centre is more than 20 units under the
terrain at its own x, y, with its speed, its rest or settle state, every collider a ray finds
straight down there (whether the terrain collider is in place where the surface says) and,
for a drop, what it touches. A ragdoll `fell through` line with no `sank` line before it is
the guard's reading on a slope (it measures under the first bone), not a body under the
ground. Smoke probes `sinking-drop-*-spot` and `sinking-corpse-*-spot` run at the reported
spots.

What the probes found [observed: smoke runs of 241f1581d, 2026-11-18, logs in the session's
notes]:

- **The corpses** of the reports (Bighorner 001710BB, Radscorpion 0015A0C4, Mole rat
  00161732) are leveled spawns (`VSpawnTier2BigHornerMed75` …) whose leveled choice in those
  runs was the invisible `nvvoid` placeholder (scale 0.1, no shapes; actors.md §10): with
  Health 0 a new game lays it out dead (dead-on-spawn.md) where its reference is placed,
  and its two-body ragdoll starts when the cell loads. 001710BB's root started about 3 units
  under the terrain and fell straight down (0.2 s later 20 under at 158 units/s, then caught
  152 under the terrain, the reports' figure); 0015A0C4's started 360 under (the reports'
  355), where a terrain-layer rock (001613EC) stands 49 over the heightfield; 001710B3 nearby
  did as 001710BB. A body whose centre starts under the two-sided terrain mesh is pushed
  down and falls through it. Players see nothing of these (the model has no shapes); the
  fall-through warnings and the lifts were all there was.
- **The dropped weapons** did not reproduce: the settler of the drop probes held its weapon
  holstered (SetRestrained, no combat), so it dropped nothing (combat.rs `death_drops` takes
  the weapon in the hand); the player's own drops came to rest. An NPC's death drop is
  placed 50 units in front of it and 30 over its feet (`fnv_sim::world::DROP_AHEAD`,
  `DROP_ABOVE`): on a slope rising more than 0.6 ahead of it, or under feet drawn below the
  surface, it starts in the ground, and the reports' weapons first sank slowly (8 to 24
  units/s) and then fell, as a body pushed out downwards does [hypothesis].

So [design, #2753]:

- A ragdoll about to start with a body's centre under the terrain at its own x, y is lifted
  whole until every such centre is 5 units over what stands there: the first collision a ray
  finds coming down from 100 units over the terrain, so a rock or foundation on the terrain
  rather than the heightfield inside it (`ragdoll: actor … starts with bone … under the
  terrain: lifted N units onto …`). It is left as it is when anything other than the terrain
  lies within 100 units under any such body or under its lowest body (a floor built below
  the surface, a tunnel). A saved pose (`XRGD`, or where it lay before) is lifted only past
  20 units under, since it was authored on the stored ground and the smoothed terrain
  (#2149) lies up to 8 units off it; lifted, it starts awake and comes down, instead of
  hovering at rest.
- A dropped item whose centre of mass starts under the terrain at its own x, y, with nothing
  other than the terrain under it, is lifted onto the contact shell of what stands there
  (`collision of dropped 0x… started with its centre … under the terrain …: lifted … onto …`).
  One at rest on a slope has its centre over the terrain, however low its downhill end
  reaches, so the reloads of its cell leave it alone.
- The safety nets stay. The drop probes were set up again (the settler fights the player,
  its weapon drawn, and dies about 2 s later on the spot); a fifth runs at the shotgun's spot
  (WastelandNVmini, 10798, 17343, flat at z 9920). Their lift line is marked known failing
  until a run shows it: the weapons' cause stays a hypothesis until then.
- Whether a shapeless `nvvoid` placeholder laid out dead should get a ragdoll at all is a
  question for the owner (owner-review.md).

### 4.3.1 Clutter at cell load: asleep, jointed objects held (`engine/nvf-play`, 2026-10-11)

**The playtest.** Loading Chet's store (GSGeneralStore) threw the shelves' items about
(owner's playtest of main, 2026-10-11). The test aid `NVF_PLAY_TEST_PHYS_TRACE=<s>[:<every>]`
(`nvf-play` `phys/trace.rs`) logs every placed reference with a dynamic body for `s`
seconds after its cell loads: its bodies' offsets, turns, speeds, and the overlaps its
contacts had after the first step. Before the change [observed, owner's install, `--cell
GSGeneralStore --play`, 2026-10-11], 53 of the 67 dynamic references there moved:

- **Jointed objects flew apart.** Three references have two or more dynamic bodies, the
  parts the model's constraints join: the globe on its stand (`clutter\school\globe01.nif`,
  two bodies overlapping each other by 5.3 units, one limited-hinge constraint), the
  bucket and its handle (`clutter\junk\bucket01.nif`) and a hanging office lamp
  (`dungeons\office\lights\offrmlighthanging01.nif`, a static mount and two dynamic
  parts with two constraints). Constraints are not modelled for placed references, so the
  globe's parts pushed each other apart (one part ended 116 units away, turned 164°), the
  lamp's parts fell 223 units from the ceiling, and they knocked the items beside them
  (an iron and a paperweight were still sliding after 3.5 s). These references are drawn
  where they were placed, whatever their bodies do (only references with one dynamic
  body are drawn where physics puts them, `PhysState::placed_dynamic`), so the parts
  were invisible. Of the 190 meshes in `Fallout - Meshes.bsa` outside skeletons with two
  or more dynamic objects, 114 hold constraints [observed: `fnvtool collide objects` and
  `fnvtool collide ragdoll`]; the others are weapons with moving parts, hulks and gore.
- **Items with nothing under them fell.** Two books inside a display case
  (`DisplayCase02`, whose collision is its outer shell: 86 triangles, top at 600, floor at
  544) fell 31 units to the case's floor, a magazine 10 units onto the shelf's lowest
  board; in the game they stand where they were placed [observed by the owner: they do
  not fall].
- **Piles settled with a jump.** Produce in a milk crate overlaps its neighbours by up to
  4 units; rapier pushed the pile apart (turns up to 27°), and the rest of the clutter
  dropped 0.3–5 units onto its shelves.

**What the engine does now** [design; hypothesis about the original: a placed Havok
object stays where it was placed, inactive, until something disturbs it — the books in the
display case, and a resting dropped item not woken by `SetPos` (§4.3, `pc_drop_fall`)]:

- Placed references' dynamic bodies are added **asleep** (`PreparedRef::asleep`).
  rapier wakes both sides of every contact that starts, so a body added asleep
  woke as soon as its first contacts were found; `PhysWorld` finds the contacts of bodies
  added asleep before the next step and puts them back to sleep (`update_queries`). They
  wake when a body moves against them (the player, an actor, another item falling), when
  they are struck (shots, explosions: `push_ref`, `push_dynamic`) or when a body they
  touch is removed (an item picked up from under them) [observed: fnv-phys tests
  `clutter_added_asleep_*`]. Dropped items (§4.3) are added awake, as before.
- A placed **item** (WEAP, ARMO, AMMO, MISC, ALCH, BOOK, KEYM, NOTE, IMOD, CHIP, CMNY,
  CCRD) whose model is **two or more dynamic bodies** and nothing else (the teddy bear:
  six on five limited hinges) is joined into **one rigid dynamic body**, as a dropped one
  is (`one_dynamic_body`, #1604; `join_jointed_item`, tallied `jointed_item_joined`). It
  moves as one piece in the pose its file draws [design: the hinges are not simulated]:
  the grab key holds it, and shots and blasts move it (#3428). One posed from its XRGD
  (the bear south of Goodsprings, 00172D8B, lying as its designer left it) is joined in
  that pose.
- Any other placed reference with **two or more dynamic bodies keeps them keyframed** at its
  placement (`hold_jointed` in `phys/prepare.rs`; tallied `jointed_held`): solid where it
  is drawn, never moving [engineering choice until constraints are modelled for placed
  references; then they would hang and swing as in the game]. Two kinds are let go
  later, their bodies then an object ragdoll the drawing follows: a swinging trap once
  the game has it live (traps.md §4), and a destroyed object's hulk, whose loose parts
  (no constraints) fall once its collision is built from it (destruction-stages.md §4,
  issue #1410).

`fnvtool phys cell` keeps the shared rule (dynamic bodies awake, jointed parts dynamic):
it simulates only its probes, and the parity lab's L4 scenarios are built on it.

After the change [observed, the same runs, 2026-10-11]: GSGeneralStore 0 references moved
by physics in 10 s (3 held as jointed), GSHouseInterior03 0 (was 7: cleaners on the oven,
the lamp, Buffout, BB gun and BBs on the desk, all overlapping the containers' keyframed
bodies by about 2 units), GSProspectorSaloonInterior 0; the trace's only motion in the
store is the office fan (an MSTT) nudged 2 units. Shot bottles still fall: the CLAUDE.md
world-sounds test knocked Sunny's three bottles 117–128 units ("impacts by material: glass 3").

### 4.3.2 Statics with Havok models never move (`engine/nvf-play`, playtest list 5 item 59)

The three motorcycles outside the Goodsprings saloon could be pushed by the player; in the
game they do not move, while tumbleweeds and Sarsaparilla crates do [observed: owner,
playtest list 5 item 59]. The tumbleweeds roll with the weather's wind: [wind](wind.md)
(their model's body flag, issue #1401).

- They are STAT references (`Motorcycle01Static`, 0x00106341, 0x00106342, 0x0010750C)
  [observed: `fnvtool world refs --world WastelandNV`]. Their model,
  `vehicles\motorcycle01.nif`, has one dynamic body: layer 10 (Props), mass 600
  [observed: `fnvtool collide objects --filter motorcycle01`]. The same model is used by the
  moveable static (MSTT) `Motorcycle01`, placed elsewhere.
- So the model's Havok data cannot be what keeps them still; the record type is. 1,131 STAT
  and 19 SCOL references in FalloutNV.esm use a model with dynamic bodies, most of them the
  `1stPerson…` weapon statics that share the weapons' models [observed: the same two
  commands, every WastelandNV cell and every interior].
- **What the engine does now** [hypothesis: the game simulates Havok bodies only for the
  record types meant to move — MSTT, items, containers, activators — and a STAT or SCOL
  is fixed]: a placed STAT or SCOL reference's dynamic bodies become fixed
  (`fix_static_base` in `phys/prepare.rs`; tallied `static_base_fixed`), its keyframed
  bodies are kept. `fnvtool phys cell` keeps the shared rule (the parity lab's L4 scenarios
  are built on it), like `jointed_held` above.

## 5. Determinism and precision

- The world advances only in fixed steps of `PhysParams::dt`; `Stepper` turns frame time
  into whole steps and carries the remainder (at most `max_substeps` per call), in `f64`
  so it does not drift over hours [design]. `Stepper::alpha` is the one place the share of
  a step the remainder stands for is worked out: whatever is drawn between the last two
  steps blends by it. How many steps a second play takes is a setting (§5.4); the tools,
  the tests and the parity lab step at 60 unless told otherwise (`fnvtool phys walk`'s
  `hz`).
- Two worlds built the same way and stepped the same way are bit-identical, with and
  without a character moving and jumping among a dozen tumbling boxes (`state_digest`
  over every body's position and velocity bits, every step) [observed: tests, same
  process]. Cross-platform identity is what `enhanced-determinism` is for
  [hypothesis: not yet compared across machines; CI runs the tests on Linux only and
  compares nothing between machines].
- Bodies are created in call order, and rapier's results depend on it [hypothesis, as for
  any solver]; the tools add terrain by grid and references by FormID. Queries that
  return several colliders sort them.
- **Building off the world**: `add_ref` and `add_terrain` are `PreparedRef::new` /
  `PreparedTerrain::new` (pure: shapes, poses, no world) followed by `insert_ref` /
  `insert_terrain`, so the shapes can be built on another thread; the two halves give the
  same bodies in the same order as the one call [observed: parity-lab L4 results
  identical to the code before the split]. The game prepares cells on a loader thread and
  inserts them in the order it asked for them, a cell's references by FormID, whatever
  order the thread finishes in; its tests find the world bit-identical (state digest,
  loaded cells, stats) whether the loader finishes cells forwards or backwards, to loading
  every cell on the main thread, and between two runs [observed: tests in
  `engine/nvf-play/src/phys/tests.rs`] (docs/design/layer4-play.md §6).
- **Snapshots**: `PhysWorld::snapshot` copies the whole world (bodies, colliders, contact
  graph, islands, broad phase); stepping the copy and the original with the same inputs
  stays bit-identical, a character included [implementation evidence: test
  `snapshot_continues_identically`].
- **Query refresh backends**: [implementation history: registry collision-only route]
  Bringing queries up to date between steps ran rapier's collision-only
  pipeline, which consumes the change flags of newly inserted bodies without registering
  them for simulation; a dynamic body added and then queried (or added while a character
  moves, since `move_character` updates queries first) never moved [implementation evidence: test
  `bodies_added_then_queried_still_simulate`, rapier 0.36.0]. That route flags such
  bodies changed again so the next step registers them. Re-check when upgrading rapier.
  For that registry route (#2638), the rearm must also keep a kinematic body's queued next pose. `set_position`
  replaces both its current and next pose; `character::apply_move` has already queued
  the next translation before the fixed step [documented: rapier3d 0.36.0,
  `dynamics/rigid_body.rs`, `set_position` and `set_next_kinematic_position`;
  `crates/fnv-phys/src/character.rs`, `apply_move`]. Copy the next pose before rearming
  and restore it afterward for kinematic bodies [hypothesis: engineering policy]. This leaves registration
  deferred until the real step. The library's `world::query_motion_tests` compare the
  queried world with a direct step of the same snapshot: position- and velocity-driven
  motion, an unrelated body's queued force and torque, and a real `move_character`
  followed by an unrelated insertion and query. The query must not advance time or
  erase the movement that the next step consumes. These are authored implementation
  controls, not measurements of the original game.
  The collision-only query also loses the wake-up of a sleeping kinematic body that is teleported
  (`place_character` on a character that was not moved for a few seconds): the body
  stayed where it was put while the character walked on, so rays and other characters
  met it there [implementation evidence: test `a_placed_character_takes_its_body_along`]. Placed
  bodies are woken again after the query, like struck ones (`PhysWorld::body_placed`).
  The same pipeline loses the wake-up of a sleeping body that was struck
  (`push_ref`, `push_dynamic`) since the last step: the body counts as awake, keeps its
  new velocity and never moves [observed: test
  `a_sleeping_body_struck_then_queried_still_moves`; in play, a bottle shot off a fence
  stayed put]. `update_queries` puts such a body to sleep and wakes it with its velocity
  again.
  The list of struck bodies missed one: a sleeping corpse struck by a shot or a VATS hit
  (`push_ragdoll`). The bone woken was left in its sleeping set: the corpse froze, and once
  a piece landed on it its island held bodies of both the awake set and a sleeping one.
  When that island fell asleep rapier panicked [observed: the owner's crash of
  2026-11-11, `extract_sleeping_sub_island` island.rs:54, after a VATS head kill; tests
  `a_sleeping_corpse_struck_before_a_query_wakes_whole` and
  `rapiers_islands_hold_under_random_play` (seed 4) in `tests/islands.rs`]. So after every
  refresh `fnv_phys::islands` checks rapier's islands whatever made the change: a body awake
  outside the awake set, or one in a sleeping set whose island has a body in the awake set,
  is woken with its island; a fixed body still listed in an island has its type told again
  for the next step. The exit log's "island repairs after query refreshes" line counts them.
  [documented: current selected backend] This branch instead uses the vendored
  Rapier 0.36.1 zero-dt physics-registration path, including its deferred-BVH join
  and sleeping-bootstrap corrections. It does not rearm inserted bodies with
  `set_position`. Each quarantine report is consumed before another pipeline call
  clears it; at most one additional zero-time pass publishes remedy changes, while
  unexpected pending marks remain dirty. See
  [the query-refresh lifecycle note](query-refresh-island-lifecycle.md).
  The three observable motion contracts remain unchanged. The registry rearm
  correction remains valid for consumers of that route; it is not concatenated
  with this distinct registration backend. Neither source description establishes
  new runtime or original-game evidence.
- **Drawing is not stepping**: frames come at their own rate, between the fixed steps;
  what physics moves is drawn between its last two steps (§5.2), which reads the world
  and never writes it. Everything the simulation sees (the poses written back to the
  simulation, GetPos, saves, queries, the corpses' rest rule) stays on the stepped pose
  [observed: test `drawing_leaves_the_steps_bit_identical`].
- **NaN or infinite state** is taken out of the simulation by rapier and dealt with after
  each step (§5.3).
- **f32 range**: a ray–sphere test from about 10,000 units away was off by about one unit
  (cancellation in the quadratic), from 100 units by under 0.01 [observed: tests]. Probes
  and activation rays should start near what they test; `fnvtool phys` starts ground rays
  1,000 units above the highest loaded terrain.

### 5.1 Actor hit volumes in rapier (2026-11-03)

Actor hit capsules (combat-rules.md §7 and §12, `fnv_phys::actors`) are rewritten every
frame from the posed skeleton, about 19 per drawn actor. How they are placed in rapier
decides what the next query refresh costs.

- **Before**: each capsule was a collider attached to the actor's kinematic body and
  placed with `Collider::set_position_wrt_parent`. In rapier 0.36 that call always flags
  the collider `PARENT`, with no equality test [design: rapier3d 0.36.0
  `geometry/collider.rs:439-444`]. The broad phase treats `PARENT` as a re-parent. At the
  next update it removes the leaf and inserts it again as a new one
  [design: `geometry/broad_phase_bvh/update.rs:79-94`]. Any such re-insertion forces a
  refit of the whole tree [design: same file `:272`]. Every overlap of the leaf is then
  reported again and rejected by the pair filter. All of this happened even for an
  identical pose. On the towns route, the refresh under the player controller cost
  1.7-2.6 ms a frame, most of it these writes [observed: tracy spans
  `phys::refresh_detect` under `phys::player_move`, 2026-11-03].
- **Now**: the capsules are colliders of no body, placed with `Collider::set_position`.
  That flags only `POSITION`, which takes the capsules off the forced re-parent path
  (removal, re-insertion as a new leaf, forced full refit). It does not end every
  re-insertion or refit: a capsule inside its margin leaves the tree untouched, one that
  leaves it is updated in place or, when only a small batch of leaves moved, re-inserted
  by SAH descent [design: `update.rs:139`, `:216-223`], and a frame can still take a
  full refit for other reasons [design: `:288`].
  - Each kinematic body stays as it was, moved as before. The body set, `state_digest`
    and the body counts are unchanged.
  - A write queues `(collider, body, pose on the body)`. `PhysWorld::flush_hit_volumes`
    places the capsule at `body pose * pose on the body`, at the start of every query
    refresh that has work and at the start of every `step`
    [design: `crates/fnv-phys/src/world.rs:820,865`, `actors.rs:311`]. That is the same
    product, from the same operands, at the same point where rapier places an attached
    collider [design: rapier `pipeline/user_changes.rs:24`]. So poses read after a
    refresh are bit-identical to the attached ones, and direct reads before a refresh
    (aim points, part centres) are exactly as stale as before.
  - A new capsule starts where rapier puts a new attached one
    [design: rapier `dynamics/rigid_body_components.rs:1242`]. Removing the actor
    removes its capsules explicitly [design: `actors.rs:284`].
  - `update_queries_for` and `step` are the only rapier pipeline entry points in
    `fnv-phys`. Any new direct call into rapier's pipeline needs a flush first.
  - A parentless collider gets no soft-CCD or motion margin from a parent
    [design: rapier `geometry/collider.rs:623-651`]. That only changes broad-phase
    culling: the exact shape tests decide every hit.
- **Identical writes are skipped**: a `set_actor_parts` call whose volume and parts
  match the last write to the bit writes nothing and dirties nothing
  [design: `actors.rs:234`]. This holds in both representations.
- **Switching back**: `NVF_PHYS_HITVOL_ATTACHED=1` (any value but empty or `0`) gives
  the attached capsules again, for comparison
  [design: `engine/nvf-play/src/phys.rs:158-172`]. The library switch is
  `PhysWorld::set_standalone_hit_volumes`, on by default. The startup log says which is
  in use (`actor hit volumes: standalone colliders` or `... attached to their bodies`).
  The `phys-refresh:` line ends with `hitvol_standalone=<0|1>`.
- **Measured** [observed: release builds, the same exe run with and without the switch,
  this project's towns route, settled frame means, attached then standalone, counterbalanced
  order, 2026-11-03]: 23.81 → 22.56 ms (−1.25), 23.69 → 20.95 ms (−2.74) and
  21.51 → 19.79 ms (−1.72); three route pairs, median paired gain 1.72 ms (range
  1.25-2.74). The route's baseline itself varies by about 2 ms between launches, so the
  per-pair difference is the number to read, not either launch's mean.
- **Exact ties.** A first-hit `ray` or `sweep` with two candidates at the same time of
  impact returns the first in tree-traversal order: parry replaces a result only on a
  strictly smaller cost, and it prunes a branch whose bound is not strictly smaller
  [design: parry3d 0.31.1 `partitioning/bvh/bvh_traverse.rs:406,422` (replacement),
  `:402,418` (pruning)].
  This happens with a ray starting inside two overlapping capsules (distance 0), with
  coincident actors, or with mirrored capsules. `ray_all` orders by distance, then
  owner, and the owner includes the part [design: `world.rs:1226`]. So two segments of
  one part at the same distance keep traversal order too.
  - The two representations shape the tree differently, so they can pick a different
    winner among exactly tied hits: another actor, not only another part (coincident
    actors: form 0x10 in one, 0x11 in the other; combat reads both actor and part from
    the hit), or the other order of same-owner hits in `ray_all`. A sweep's witness
    point and normal belong to the winner, so they differ with it.
  - The winner already depended on the tree's history with attached capsules. An
    attached world that writes an identical pose again picks other winners than one
    that skips it. So the tie ordering is history-dependent, as it was before;
    identical histories still give identical answers. The standalone capsules change
    which arbitrary winner a given history picks.
  - Differential tests in `crates/fnv-phys/tests/synthetic.rs` (`hitvol_diff_*`,
    `hitvol_ties_*`) run the same script on both representations
    [observed: `cargo test -p fnv-phys --test synthetic hitvol`, 2026-11-03]:
    - 16 tests: 8 scenarios run twice, once exact (`hitvol_diff_*`, every compared
      value to the bit) and once relaxed (`hitvol_ties_*`: distances, points of rays,
      fractions, hit sets, overlaps and the non-query state, not which tied candidate
      won).
    - 2 exact tests pass: remove/re-add with handle reuse plus a part-count change, and
      20 actors × 19 parts over 50 frames with falling boxes and a walking character
      (the crowd without coincident twins).
    - 8 relaxed tests pass.
    - 6 exact tests diverge on exact ties; they are kept `#[ignore]`d with the observed
      values.
    - Ray distances and points, sweep fractions, hit sets, overlaps, aim points, other
      bodies' trajectories and the character's steps matched in every test. Sweep
      witness points and normals differ where the winner differs (witness z 85.9849
      for part 0 against 85.729836 for part 7).
    - The relaxed tests enforce the diagnosis: every answer they let through that differs
      to the bit must be shown, in both worlds, to be an exact tie (`tied`). For a ray,
      each world's winner is among the other world's `ray_all` hits with the same owner,
      distance and normal. For `ray_all`, the order of distances is the same. For a sweep,
      each world's winner is touched (the swept shape grown by 0.05 units) where the other
      world's sweep stopped. `hitvol_tie_check_rejects_what_is_no_tie` checks that the test
      rejects non-ties. All 400 such differences, in the six relaxed scenarios that
      have them pass [observed: `cargo test -p fnv-phys --test synthetic hitvol`,
      2026-11-03].
    - Canonical tie-break (time of impact, then owner): not done; issue #1459. Done
      since, see the next item.
- **Canonical tie-break** [design, #1459]. First-hit `ray` and `sweep` now return, of all
  candidates, the one with the smallest time of impact and, among exactly equal times
  (the same f32), the smallest by:
  1. owner (`Owner`'s order: kind, then for an actor's capsule form id, layer, part);
  2. segment: which of the owner's colliders it is, stored in `user_data` bits 104..120
     (`Owner::encode_segment`): the index of the part capsule in the list given to
     `set_actor_parts`, or of the leaf in its collision object; 0 elsewhere (terrain,
     characters, whole-actor capsules, ragdolls, gore);
  3. the hit's own geometry, as bits: for a ray the normal then the sub-shape (triangle),
     for a sweep the witness point then the normal on the hit collider. Two candidates
     equal in all of these give the same answer, so which collider it came from no longer
     shows.
  - `ray_all` sorts equal distances by the same keys (owner, segment, normal, sub-shape).
  - How: a custom leaf cost over parry's `Bvh::find_best`, not collect-all-then-sort
    (`first_hit`, `crates/fnv-phys/src/world.rs`). The best (time, key) is kept beside the
    traversal; a leaf the rule takes reports the next float above its time to
    `find_best`, so nodes whose bound equals the best time are still visited and each
    collider is asked with that bound (a tie is returned, not cut off). The key is worked
    out only on exact ties. A sweep casts each collider with the full options, as parry's
    composite cast does [design: parry3d 0.31.1
    `query/shape_cast/shape_cast_composite_shape_shape.rs:15-60`], so every answer without
    a tie is the one parry gave before.
  - The answer no longer depends on the tree's history: the six exact tests above pass
    without `#[ignore]` (attached and standalone capsules, skipped and forced writes,
    snapshots, coincident twins in a crowd) [observed: `cargo test -p fnv-phys --test
    synthetic hitvol`, 2026-11-06].
  - Combat: with coincident actors the hit now goes to the lower form id, and inside two
    overlapping parts to the lower part index (then the earlier segment), always. Before,
    either could win depending on the tree.

### 5.2 Drawing between steps (2026-11-11)

The world steps at the physics rate, 60 times a second of the game's clock by default (`PhysParams::dt`, §5.4), the frames come
at whatever rate the machine and the screen give (up to 152 a second on the owner's
variable-refresh screen). On a frame between two steps, a body drawn at its last step's
pose moves in uneven jumps: at 152 frames a second a step lands every 2 or 3 frames
(152 / 60 = 2.53). The player, the walked
actors and the corpses were already drawn between their last two steps; everything else
physics moves was not (knocked clutter, dropped items, the item the grab holds, gore,
wreck parts, swing traps, tumbleweeds). Now every one is [design, the usual answer for a
fixed step at any frame rate; rapier's own Bevy plugin does the same with its
`TimestepMode::Interpolated` [documented: bevy_rapier in rapier's repository]]:

- **The pose before the step** (`fnv_phys::drawn`). At the start of every
  `PhysWorld::step` the world keeps the pose of each body that can move in it: the
  dynamic bodies awake (rapier's active set), those added, struck or woken since the last
  step and the held body. Bodies asleep, fixed or kinematic are not kept (nothing to
  blend). The table is refilled each step in the same storage; reading it allocates
  nothing but the lists the readers return.
- **The share of a step** (`PhysState::draw_alpha` in `engine/nvf-play`): the stepper's
  remainder over the step, in [0, 1], the one place it is worked out. 0 draws the pose
  before the last step, 1 the pose after it.
- **Blending**: translations linearly, rotations by slerp the short way round. The readers
  mirror the stepped ones: `drawn_ref_bodies` / `drawn_ref_body` (`ref_bodies`),
  `drawn_ragdoll_nodes` (`ragdoll_nodes`), `drawn_gore_state` (`gore_state`); `drawn_refs`
  lists the references with a body drawn between steps.
- **Not across a jump.** A body added, placed by hand (`body_placed`, `place_character`,
  `pose_ref_object`), built again (a ragdoll set at rest, roused or lifted; a reference's
  collision built again for a script's SetPos, a new body) or frozen after NaN state (§5.3)
  since the last step has no pose kept: it is drawn where it is until the next step. A
  body that went more than `DRAWN_JUMP` (100 units) along an axis in one step was put
  somewhere, not moved, and is drawn where it is too (the corpses' old guard, now for
  every body).
- **Woken by a contact** during a step (asleep at its start), a body has no pose kept for
  that step and is drawn where it is until the next. It starts from rest, so that step
  moves it little [hypothesis: not measured].
- **Who draws what** (`engine/`):
  - placed clutter physics has moved (`PhysState::placed_dynamic`, tumbleweeds and the
    held item included) and dropped items with one dynamic body (`PhysState::created`):
    `PhysState::drawn_ref_pose` turns the drawn body into the reference pose;
    `PhysState::between` lists them every frame while they move and once after (drawn
    where they came to rest). `moved.rs`, `dropped.rs` and `script_draw.rs` draw them
    there; `settle_placed` and `settle_created` still write the stepped pose to the
    simulation. `moved.rs` keeps the entities of each reference it moves, so a reference
    drawn every frame is not looked for among every drawn entity each frame;
  - corpses (`nvforever` `ragdoll.rs`): the kept poses, exact however many steps the
    frame ran (before, the pose before the last step was estimated from the last frame's,
    off when a frame ran several steps); the corpse the grab drags too;
  - gore pieces (`dismember.rs`), wreck parts (`wrecks.rs`) and swing traps
    (`swing_traps.rs`): drawn poses for the drawing, the stepped ones for their rest and
    hit rules.
- The exit report says how much was drawn between steps: `physics: drawn between steps: N
  references at most in a frame, over F frames; B bodies at most kept before a step; …`.
- **Cost**: one pose copy per moving body per step, and for drawing a lookup and a blend
  per drawn body per frame. Steps and their results are unchanged: the steps never read
  the kept poses (test `drawing_leaves_the_steps_bit_identical`, the digests and traces of
  the other tests unchanged).
- **Latency** is unchanged: what a frame draws is up to one step behind the newest step,
  as the player's camera already was.

### 5.3 NaN or infinite state: rapier's quarantine (2026-11-11)

rapier 0.35 and later check each step for state that went NaN or infinite: such a body is
rolled back to its last finite pose (when it has one), its velocities and forces zeroed
and the body disabled; a collider whose own geometry went non-finite is disabled; both are
listed for that step in `PhysicsWorld::quarantine()` [documented: rapier3d 0.36.1
`src/pipeline/physics_pipeline/quarantine.rs`]. After each step `PhysWorld::step` reads
the list (`fnv_phys::quarantine`):

- **Reported once**: each body or collider is logged the first time
  (`physics: quarantined the body of reference 0001E9F2 object 0 (layer 4) at step N: …`),
  with what it belongs to, listed in `PhysWorld::quarantined` and counted in
  `Stats::quarantined_bodies` / `quarantined_colliders`; the engine's exit report repeats
  the counts.
- **Not left disabled out of sight** [design]: a disabled body has no contacts and no
  query finds it, while the engine would still draw its object: an item nothing could
  touch.
  - A reference's dynamic body (clutter, a dropped item) or a gore piece is made a fixed
    body where it was rolled back to and switched on again: drawn where it was, solid and
    found by rays and the crosshair, never simulated again. Simulating it again could meet
    the same NaN every step; taking it out of the world would make the item vanish (a
    quest item too) and leave the engine's tables (references, write-back, the grab)
    naming a body that is gone. The grab lets go of it.
  - A ragdoll body takes its whole ragdoll to rest where it lies
    (`set_ragdoll_at_rest`), the grab letting go first.
  - A kinematic body (a character, a hit volume) is switched on again: its owner places it
    before every step.
  - A body with no finite pose to go back to, and a collider with bad geometry, stay
    disabled (nothing sensible to draw or collide); the report says so.
- Nothing NVForever does is known to make NaN state; the reports are there so that one
  would be seen [observed: tests `a_body_gone_nan_is_reported_once_and_frozen_in_place`,
  `a_corpse_gone_nan_is_held_at_rest`, `a_collider_gone_nan_is_reported_once`, NaN set by
  hand].

### 5.4 The physics rate (2026-11-11)

How many fixed steps a second play takes [design: the owner's approved plan,
`dev/plans/physics-timestep-and-rapier.md` §5 phase 2; `engine/nvf-play/src/phys_rate.rs`].

- **The choice**: the settings page's Physics rate (Display > Screen, `physics-rate` in
  `[Settings]` of NVForever.ini): 60 (the default, the rate the captures and the parity
  lab's goldens were taken at), 120, 144, 160, or Match FPS (the owner's name,
  2026-11-11; Match display before): the frame rate the game holds, from 60 up to a
  ceiling, the frame rate the display's pace allows. The ceiling is the cap in effect (the
  page's limit, variable refresh's, frame generation's real-frame cap, the lowest), no
  more than the refresh rate with vsync; with no cap, the display's refresh rate
  (`display_hz`); at most 160. `--physics-hz <60..160|fps>` sets it for one run, over the
  setting (the row then says so and cannot be changed); it is kept across a load's
  restart. `fps` is stored; `display` (the INI of a build before the rename) is read as it.
- **240 was taken out** (2026-11-11) [design: the owner, "remove the 240hz option if it
  would cause issues"]: in battle-50 its steps took 17.8 ms of a 39.5 ms frame (45 %, just
  under the cost guard's half, so the guard would not step down), pushed objects slid
  farthest there (+34 %, below) and it gives nothing on a 160 Hz screen. An INI of an older
  build with `physics-rate=240` (any rate over 160 up to 240) loads as 160 without a
  complaint and is saved as 160 (`RateChoice::parse_stored`). The measurements at 240
  below are kept for the record. The slow-motion floor stays 1/240 s (below).
- **Match FPS follows the frame rate** (`phys_rate::FpsFollow`) [design: the owner's
  example, 144 fps dropping to 80 in an intense scene, and the coordinator's spec]:
  - It counts the main world's updates over whole real seconds (`Time<Real>`; under
    frame generation the rendered frames, not the presented ones), only frames that are
    played (not a menu, a conversation, a loading screen, a cell not loaded yet), not
    paused, not under a slowed or sped-up clock (VATS, kill cameras) and not within 3 s
    of a cell coming into the physics world.
  - Its target is that frame rate rounded down to tens, between 60 and the ceiling (80
    fps: 80 Hz; 147: 140; at or over a ceiling of 152: 152).
  - Down when each of the last 2 seconds ran at least 10 % under the rate in effect, to
    their target; up when each of the last 5 seconds (the cost guard's wait, doubled by a
    fall within 30 s of a rise, up to 80) has a target above it, to the lowest of their
    targets. Rounding down to tens leaves a rise at least 10 % over the target before a
    fall.
  - The cost guard stays on top: the rate in effect is the lower of Match FPS's and the
    guard's limit, never under 60. Each switch goes through `Stepper::rescale` and is
    logged (`phys: physics rate 152 Hz -> 80 Hz: the frame rate held 84 fps over 2 s
    (Match FPS, up to 152 Hz)`); the exit line counts the guard's and Match FPS's
    switches. Smoke `physics-rate-fps` holds the frames down with the test aid
    `NVF_PLAY_TEST_SLOW_FRAMES` and checks the step down and back up.
- **The INI and older builds**: the key is read only by builds that know it. The settings
  reader looks up the keys it registered and leaves every other line of the file as it is
  when it writes (`settings::write`, `fnv_install::ini_set`), so an older build sharing
  the INI ignores `physics-rate=` and keeps it, and this build reads an INI without it as
  60 [observed: the code]. Nothing about the rate is in a save: a save loads the same
  whatever the rate, in either build.
- **Fixed steps at every rate**: each step is `1/rate` s of the game's clock, so each rate
  is reproducible on its own. While the clock runs slow (VATS, a kill camera) the step
  shrinks with it, `1/rate × speed` down to `SLOWEST_PACED` (2 %) and never under 1/240 s (below), so the steps a real
  second stay the rate's; faster than `FASTEST_PACED` (4×, a script's
  SetGlobalTimeMultiplier) the step grows instead of the steps a real second, which are
  thus never more than four times the rate [design; before 2026-11-11 a faster clock kept
  the step at any speed]. A frame runs at most a quarter second of steps at the rate
  (`catch_up_steps`: 15 at 60, 38 at 152, 40 at 160), what the virtual clock passes on in
  its longest frame; time beyond it is dropped, so a slow moment cannot grow a backlog
  (no "spiral of death": the game slows down instead).
- **Slow motion (VATS, kill cameras)** [observed and design, 2026-11-11]:
  - The step never goes below 1/240 s (`phys::SHORTEST_STEP`), whatever the rate and the
    clock (240 is no longer a menu rate; the floor is the step of the slowed clock). rapier keeps positions in f32 metres from the worldspace's origin, and a
    shorter step moved a slow body by less than its position can show: at Goodsprings a
    body at 5-20 units/s stood still over steps of 1/3000 s (the step of a 2 % clock at
    60 Hz, and of a 5 % one at 152 Hz), 150,000 units out everything under 200 units/s
    did [observed: `slow_bodies_far_from_the_origin_move_at_the_shortest_step` and the
    ignored `probe_slow_steps_far_from_the_origin` in crates/fnv-phys/tests/synthetic.rs].
    So under a clock slower than a quarter there are fewer steps a real second than the
    rate (a kill camera at 0.25: 60; VATS marking at 0.05: 12); what moves is drawn
    between steps (§5.2), so it still moves smoothly. Moving everything relative to an
    origin near the player would let shorter steps resolve [hypothesis: not done].
  - Every timer that judges a body counts the game time stepped
    (`PhysWorld::stepped_secs`, or the steps since the last frame times their step): the
    corpses' calm and awake timers, the wrecks' calm and fall timers (they counted the
    frames' time until 2026-11-11), the dropped items' log marks (they counted the steps
    times the step now). rapier's own sleep compares a body's motion per step with
    0.05 m/s times the step and counts the steps' time [documented: rapier3d 0.36.1
    `RigidBodyActivation::update_energy`], so it holds under any step.
  - The corpses' awake guard (`ragdoll::MAX_AWAKE`) counts full-speed time only: a
    corpse is not held at rest while the clock is slowed, and moves on, slowed, until
    the clock is back [design: the owner, 2026-11-11; a kill camera of 8 real seconds at
    half speed had held its corpse at rest 3 game seconds in].
  - The cost guard leaves out the frames of a slowed or sped-up clock.
  - The kill camera logs how its target's drawn place moved over the camera and in the
    frame after it (`kill camera: the target's drawn place moved …`); smoke
    `vats-kill-cam-slowed-60`, `-120`, `-144`, `-152`, `-160` and `-display` check that it moves, never jumps
    and is not set at rest before the camera ends.
- **A change of rate in play** (the page, Match FPS, the guard)
  keeps the share of a step the clock is into (`Stepper::rescale`), so nothing drawn
  between steps jumps; the remainder's game time changes by less than a step, once.
  While not playing (a menu, a conversation, a loading screen, a cell still loading) no
  time reaches the stepper and its remainder is kept, so the blend (§5.2) holds and
  nothing shifts when a menu opens or closes; until 2026-11-11 the remainder was zeroed
  there, which drew everything at its pose before the last step under a menu.
- **The cost guard** (`phys_rate::Guard`): the time in the player's `step` system (the
  steps and what runs between them) is read every played frame (two `Instant`s). Over half
  the frame time over 2 s, the rate goes down a notch of 60/120/144/160 (never under
  60); under a quarter for 5 s, and foreseen under 40 % at the next notch, it goes back up
  towards the chosen rate. A fall within 30 s of a rise doubles the wait before the next
  rise (up to 80 s), so the rate does not flap. Every switch is logged (`phys: physics rate
  160 Hz -> 144 Hz: ...`); the `--log-frame-times` physics line names the rate
  (frame-rate-audit.md), and the exit report says how many switches there were.
- **Rules that counted steps now count seconds**, so they last as long at any rate and as
  long as before at 60:
  - a pushed player held by something is not pushed again for `blocking::HELD_SECS`
    (0.5 s, was 30 steps);
  - a stuck character's full move comes `character::STUCK_RETRY_SECS` (1/3 s, was every
    20th step) after the move that held it;
  - the drawn-height roughness tally counts walking as over 30 units/s (was half a unit a
    step; a report figure only).
  A duration summed from steps is reached on the step that brings the sum within half a
  step of it (`secs_reached`), so the sum's rounding never adds or drops a step: at 60 the
  rules act on the same steps as before [observed: tests]. These are game seconds: under
  VATS slow motion they last longer in real time than the 30 or 20 steps did (each step is
  shorter there). Checked and already in seconds or distances: the lunge's carry and a
  knockback (exact over any step), the swim stroke cycle, the air control gain and the
  landing table (`air.rs`), the jump and an actor's jump (taken on the first step after the
  press, kept through frames with no step), fall damage (`PLACED_JUMP` is a jump between
  steps, not a speed), pushes (`blocking::clip`, distances over `dt`), the grab and
  buoyancy (rates over `dt`), the drawn height's spring (exact over `dt`), the ragdolls'
  and wrecks' calm rules (seconds), the residents' stuck rule (frame seconds). The step
  counts left (`steps_blocked`, `held_steps`, the status line's) are report tallies.
- **What changes with the rate** [observed: `fnvtool phys walk` with the
  `parity-lab/inputs/layer4/move_*.walk.json` scripts scaled to the same seconds at each
  rate, and `character_rules_hold_at_every_physics_rate` in
  `crates/fnv-phys/tests/synthetic.rs`; with the exact gravity below, 2026-11-11; the air
  time is the airborne steps over the rate]:

  | | 60 | 120 | 144 | 152 | 240 |
  |---|---|---|---|---|---|
  | jump apex (units) | 61.58 | 61.60 | 61.60 | 61.60 | 61.60 |
  | running jump: air time (s) | 0.833 | 0.842 | 0.840 | 0.842 | 0.842 |
  | running jump: landing speed (units/s) | 290.9 | 290.9 | 290.9 | 290.9 | 290.9 |
  | a box pushed for 1 s at 120 units/s (units) | 33.8 | 45.5 | 42.2 | 38.9 | 37.5 |
  | 60° ramp, how high a walker gets (units) | 3.1 | 4.0 | 3.9 | 3.9 | 4.2 |
  | portal wall walk: end vs 60 (units) | 0 | 1.19 | 1.19 | 1.27 | 1.17 |

  - **The jump and falls are the same at every rate** since 2026-11-11: gravity is exact
    over a step (`character::gravity_step`: the feet move `v·dt − ½·g·dt²`, then the speed
    drops by `g·dt`). The apex is the jump height, 61.6, within `g·dt²/8` (0.024 at 60),
    and the air time is within a step of `2·v0/g` (0.847 s): the original's standing jump
    rose 61.6 and took 0.845 s (§6.3). Before, the speed was lowered first and the feet
    moved at the new speed (semi-implicit Euler). That undershot the apex by about
    `v0·dt/2`: 59.20, 60.39, 60.59, 60.65 and 61.00 at the five rates, with air times of
    0.817-0.838 s and landings at 281.4-287.4 units/s. A fall went `½·g·t·dt` farther
    than gravity takes it (5.7 units after 1 s at 60).
  - **The touchdown speed** (`CharacterStep::landed_speed`) is the speed at the height where
    the feet rest on the support. A fall keeps `v² + 2·g·z`, so it follows from the step's
    starting speed and height. The speed at the step's end came up to `g·dt` later, and
    where a step's end fell in rapier's grounded band depended on the step. A walked-off
    character's step down (§6.4.2) is not a fall and adds no speed. A running jump lands
    at its take-off speed (290.9); a 300-unit fall lands at 641.9, which is `sqrt(2·g·300)`,
    at every rate (639-641.5 before).
  - **A fall onto a static floor** lands up to about 3.4 units above where the feet rest,
    because rapier calls the capsule grounded that far beyond the small gap kept over
    statics. The next step puts it on the floor. Before, it sank by a step's gravity from
    rest each step: about 0.2 s to settle at 60 Hz and 0.8 s at 240. The exact gravity
    would have halved that speed again [observed: tests].
  - **Pushes differ the most**: a pushed box goes 12-34 % farther at 120-240 than at 60,
    and not in proportion to the rate (rapier's character impulses per step,
    `solve_character_collision_impulses`) [hypothesis].
  - Unchanged within 0.05 units at every rate: rest heights on terrain (3.5) and on a
    static box (0.5 above it), a 300-unit fall's time (within a step of 0.935 s),
    step-ups (0.6 step heights) and the REPCONN stairs, the 25° ramp climbed and the 60°
    one refused, a level run's distance.
  - The walk traces stay within 0.5 units of 60's on the fence and hill runs, and the
    porch's 0.25 s observations within 0.9. The portal-wall run down a mountainside ends
    1.2-1.3 units from 60's (1.7-2.8 before the exact gravity) and no longer loses the
    ground at 152. All of these are inside the parity lab's 10-unit tolerance, which
    judges 60 only. The running jump's observations agree within 2 units at every rate.
  - Whether a character in a crease runs out of the controller's sweeps (and is held,
    above) depends on the step: in the groove test fewer walks stick at 120 than at 60
    [observed: `a_held_character_waits_the_same_seconds_at_any_step_rate`]. With the
    exact gravity a standing character presses into a crease by `½·g·dt²` a step, half as
    far as before (0.095 units at 60, 0.006 at 240), so fewer walks stick. At 240 almost
    none do: one walk along the 50° crease, after 1.9 s. Walks into the 70° and 80° walls
    ride a couple of units up them and creep 0.01 units a step instead. In a probe test
    (not committed), the held walks in the groove cost the same CPU as before: 0.41 s at
    60 (0.45 before), 1.21 at 152 (1.13), 1.84 at 240 (1.88) [observed].
  - The drawn-height roughness in the exit report (`vertical acceleration ... units/s2`)
    divides each step's change by dt², so it grows with the rate (5538 units/s² of the
    feet at 144 against hundreds at 60 on similar ground) [observed: smoke
    `physics-rate-setting`]; compare it only at one rate.
- **What it costs** [observed: release build, `--log-frame-times 5`, the guard off
  (`NVF_PHYS_RATE_GUARD=0`), 1920×1080, uncapped, one launch each through tools/stress's
  launcher, the first two periods left out; 2026-11-11, the owner's machine, other
  sessions' games sometimes running]:

  | | ticks a frame | ms a frame in `player::step` | ms a tick | frame (ms) | share |
  |---|---|---|---|---|---|
  | Goodsprings, running, 60 Hz | 0.38 | 0.15 | 0.38 | 6.44 | 2 % |
  | Goodsprings, 120 Hz | 0.77 | 0.26 | 0.32 | 6.44 | 4 % |
  | Goodsprings, 152 Hz | 0.97 | 0.29 | 0.29 | 6.39 | 5 % |
  | Goodsprings, 240 Hz | 1.56 | 0.44 | 0.27 | 6.50 | 7 % |
  | battle-50, 60 Hz | 1.68 | 9.7 | 5.5 | 28.0 | 35 % |
  | battle-50, 120 Hz | 4.07 | 14.4 | 2.7 | 33.9 | 42 % |
  | battle-50, 152 Hz | 3.93 | 7.7 | 1.9 | 25.8 | 30 % |
  | battle-50, 240 Hz | 9.49 | 17.8 | 1.8 | 39.5 | 45 % |

  - In Goodsprings 152 Hz fits easily: 0.3 ms of a 6.4 ms frame.
  - In battle-50 a tick costs less the shorter it is (5.5 ms at 60, 1.9 at 152): each
    step's character moves and contacts are shorter [hypothesis]. The frame's physics
    grows less than the rate: 9.7 ms at 60, 7.7-17.8 above. The battle differs from launch
    to launch (who fights whom, when they fall), so one launch per rate is a rough
    figure (three launches at 152 gave 1.75-2.16 ms a tick, 7.7-13.3 ms a frame). At its
    26-40 ms frames none of the rates matches the frame rate: the frame is the problem
    there, not the step (#2081, #1426).
  - rapier's `parallel` feature with a configured thread pool, battle-50 at 152 Hz: 2.20
    ms a tick on the global pool, 1.90 with 4 threads, 2.11 with 8, all within the plain
    build's 1.75-2.16 [observed: a probe build, not committed]. No gain, as upstream PR
    #1801 found; not adopted.
  - `Stats::solver_us[4]` (rapier's `ccd_time`) reads 0 in rapier 0.36.0 whatever CCD
    costs (its timer is never started); CCD's work is in `counters.ccd` (#1798).

### 5.5 Loose clutter at rest (2026-11-13)

Clutter knocked about indoors stayed awake, trembling, some of it sunk into tables
(the owner's report of 2026-11-11: "clutter stuck in geometry, twitching and never
sleeping"). Two causes, both in how rapier picks a contact's points.

- **rapier's sleep rule**: a body sleeps once its farthest point has moved slower than
  twice its threshold (0.05 m/s: 7 units/s) at every step for 0.5 s, and only with every
  body it touches (islands sleep whole) [documented: rapier3d 0.36.1
  `RigidBodyActivation::update_energy`, `IslandManager::update_islands`].
- **The prediction distance indoors**: rapier keeps at most four points of a contact,
  the deepest and then those spread farthest from it among the points within the
  prediction distance [documented: rapier3d 0.36.1 `reduce_manifold_naive`]. Raised by
  the terrain's 3.5-unit shell everywhere (§5, `PhysWorld::new`, for clutter creeping on
  the terrain), it let points of a surface a unit or two below the one a body lies on (a
  shelf board's lower edge) displace the points it lies on: books, pots and bottles shook
  at 10-15 units/s from step to step while staying within about a unit of where they lay,
  and kept their neighbours awake. Since 2026-11-13 it is raised only while the world
  holds terrain (`PhysWorld::fit_prediction`); interiors use rapier's own (0.02 m, 1.4
  units). Test `a_book_over_a_lower_lip_falls_asleep_indoors`.
  **`NVF_PHYS_INDOOR_PREDICTION=0`** (a test aid, read once) keeps it raised everywhere.
  Exteriors keep the raised distance, so clutter on an outdoor shelf can still tremble
  [hypothesis: not measured outdoors].
- **Large hull faces**: parry gives rapier only the first four corners of a convex hull's
  face, whatever its size [documented: parry3d 0.31.1
  `ConvexPolyhedron::local_support_feature`, a TODO says the four should span the most
  area]. A hull merges coplanar triangles into one face, so a table top of 11 corners
  became a sliver along one edge; a body lying on the rest of it kept one contact point,
  sank into the table and rocked there awake. `fnv_phys::hull` gives the four corners
  that span the most of such a face (or, for a contact outside them, the most with the
  corner nearest it). Test `a_stone_on_a_table_top_of_many_corners_rests_on_it`.
  **`NVF_PHYS_HULL_FACES=0`** (a test aid, read once) builds parry's plain hull.
- **Creased ground (#2663)**: rapier merges the contacts of one pair whose normals lie
  within about 5° into one cluster and solves it with one normal [documented: rapier3d
  0.36.1 `contact_clustering.rs`, `COS_MERGE_ANGLE` 0.996; `contact_clustering` on by
  default]. LAND's heights come in 8-unit steps over 128 units (world.md §3), so two
  neighbouring terrain triangles often meet at 3.6°; a flat body lying across them got
  one normal for both, tipped one way, then the other (two tilts about 0.4° apart), its
  farthest point moving at 10-27 units/s and its centre creeping about 0.3 units/s, and
  never slept, on the stored landscape and the smoothed one alike [observed: issue #2663].
  The contact skin, the prediction distance, `FIX_INTERNAL_EDGES`, contact recycling,
  solver iterations, friction and restitution made no real difference; clustering off
  for the whole world rests them, but changes the contacts of every compound and
  triangle-mesh pair and rapier has no per-pair switch [observed: issue #2663]. Instead
  placed dynamic bodies (references' bodies, not ragdolls, gore, characters or bodies
  the wind blows) sleep once their farthest point moves slower than
  **`PLACED_SLEEP_SPEED` = 21 units/s** for 0.5 s, three times rapier's 7 [design,
  #2663]. It changes only when such a body may sleep, not how it moves awake; a body
  sliding or rolling slower than that for half a second stops a little sooner, and one
  asleep wakes as before when struck, pushed, grabbed or touched by a moving body. Tests
  `placed_clutter_on_creased_ground_falls_asleep` (64 drops each of a garment, a folded
  cloth, a box and a stone on a stepped and a smoothed hill, 60 and 160 Hz: all asleep on
  the ground after 12 s; with rapier's threshold 3-8 of the 256 stayed awake in each
  set-up, creeping at 0.2-4 units/s [observed: that set-up, 2026-11-17]) and
  `a_ball_rolling_slowly_stays_awake` (started at 40 and at 21 units/s, still rolling 3 s
  later; the slower one's centre rolls under 21, its top at twice that, and rapier
  measures the farthest point).
- **Measured** [observed: a probe of `fnvtool phys cell`, not committed, 2026-11-13:
  every dynamic reference of the cell added awake where it is placed, 20 s of settling,
  then a kick from below at 400 units/s and 20 s more; bodies awake at the end of each,
  and the deepest overlap with static geometry]:

  | cell, rate | before | prediction only | hull only | both |
  |---|---|---|---|---|
  | GSDocMitchellHouse (169), 60 Hz | 32 / 18, 12.7 deep | 6 / 3 | 38 / 16 | 1 / 4, 2.3 deep |
  | GSDocMitchellHouse, 160 Hz | 40 / 15, 8.3 deep | 4 / 2 | 34 / 8 | 1 / 2, 2.4 deep |
  | GSGeneralStore (68), 60 Hz | 7 / 11 | 4 / 2 | 4 / 5 | 1 / 2 |
  | GSGeneralStore, 160 Hz | 4 / 4 | 3 / 3 | 4 / 2 | 1 / 1 |
  | GSProspectorSaloonInterior (110), 60 Hz | 11 / 13 | 6 / 6 | 8 / 9 | 2 / 1 |
  | GSProspectorSaloonInterior, 160 Hz | 13 / 12 | 4 / 7 | 9 / 2 | 2 / 4 |

  With both, what is still awake at the end is rolling (Doc Mitchell's two globes) or
  thrown far by the kick. A small item fell through a floor after the kick in 5 of the
  24 kicked runs (3 before, 2 with both), not this change's doing.

## 6. Character controller and capture questions

A kinematic vertical body standing on its feet point, over rapier's
`KinematicCharacterController` with up = +Z: slides along walls, steps up ledges up to
`step_height`, refuses and slides down slopes steeper than `max_slope_deg`, settles down
onto the ground on descents (within `step_height`), integrates its own vertical speed under
gravity, jumps with `v = sqrt(2·g·jump_height)` when grounded, stops rising when the head
hits something, reports touchdown speed (fall damage input) and pushes dynamic bodies.
Since 2026-10-08 (§6.4.1) the body is a cylinder of radius 22 on a cone bottom (flat tip of
radius 3.2, rising 1.58 per unit), off walkable ground the vertical motion does not slide
(a falling body stays where it touches, at any steepness), and the horizontal velocity
follows the game's air and landing rules (`fnv_phys::air`).
Its kinematic body makes dynamic bodies collide with it. Activation targeting is a ray
from the eye that excludes the caster (`Filter::excluding(character.key())`).

Values of `CharacterParams` / `PhysParams` and how each is (or will be) measured. Jobs
named here run unattended (`tools/capture/jobs/make_movement_jobs.py`, §6.3); "owner"
means someone at the game.

| Parameter | Value used | Status / how measured |
|---|---|---|
| gravity | 686.7 units/s² (9.81 m/s² at 70 units/m) | **[observed]** 683.8 (300-unit fall) and 685.5 (standing jump), §6.3; kept |
| fixed step `dt` | 1/60 s | unknown; the game ran at 60–64 loops per second during captures [observed] |
| body radius | 22 | **[observed, §6.4.1]** from `mv_diag_fence_edge`: drops rested on the fence edge 22.6 off it (radius + 0.7 shell ≥ 22.6) and fell at 25.7; the game stops 21.45 from the fence face (`move_fence_stop`, face measured to 0.05), which would give 20.75: about 1.2 apart, not explained; the candidate now stops 22.7 from the face (keeping a static shell's 0.7 from walls, no longer the terrain's 3.5) |
| bottom shape | cone: flat tip of radius 3.2, then rising 1.58 per unit to the radius | **[observed, §6.4.1]** fitted to 26 fence-edge drops from 50 and 200 units (RMS 2.9); the depth depends on the speed of the fall, which the model does not have |
| capsule height | 128.9, with a top mirroring the bottom cone | **[observed, capture `pc_head`, 2026-10-09]** see "Capsule height" below; crouched: not captured |
| step height | 24 | [hypothesis]; `move_porch_steps` exercises steps of Doc Mitchell's porch; the limit needs ledges of graded heights (design open, §6.3); steps with shallow tops are climbed by the controller's own step-up (§6.9) |
| max slope | 45° | [hypothesis]; an unattended run up steep LAND squares is possible (design open, §6.3) |
| jump apex | 61.6 | **[observed]** apex 61.6 above the standing feet, §6.3 (`fJumpHeightMin` reads 64) |
| air control | gain 0.30 per 1/60 s towards the keys' velocity (full when a key was held at take-off or pressed within 4 steps after it, else 14.7 units/s); release decays with the same gain | **[observed]** `mv_air`, `mv_air2` (§6.4.1); `fJumpMoveMult` reads 0.30 |
| landing | fixed speed profiles for 29 steps after a touchdown above 100 units/s, forward and backward | **[observed]** `mv_air`, `mv_air2` (§6.4.1); the 100 threshold and sideways landings [hypothesis] |
| max fall speed | none | none: a 2,500-unit fall reached 1,841 units/s still accelerating (fit g = 686.0, RMS 0.09) [observed: `mv_values`] |
| walk / run speed | `nvf-play` uses 86.8 / 344 | **[observed]**, §6.3 |
| eye height | 112 (`nvf-play`) | about 123 above the feet if the crosshair pick is a sphere of `fActivatePickSphereRadius` 16 cast from above the feet (both distances agree); a plain ray fits only with the camera 10.6 units behind the feet (110 above them) [observed: `mv_values`, §6.3; the model is hypothesis]; not changed yet |
| activation reach | 150 (`nvf-play` `pick::REACH`) | `iActivatePickLength` reads 150, `fActivatePickSphereRadius` 16; at 30° down the crosshair found the fence up to a feet-to-face distance of 120 and not from 123 [observed: `mv_values`]: 122–126 along the view with the sphere model, 139–142 for a plain ray; not changed yet |
| push mass | 80 | **[observed agreement, capture `pc_push`, 2026-10-09]** see "Pushing clutter" below: unchanged |
| layers the player passes through | §4 list | B11 job `mv_b11_drops` and scenario `L4.phys.drops.b11_goodsprings`, ready (§4.2) |
| CollisionMarker bounds | half extents | three CollisionMarker tops in `mv_b11_drops`: read as full extents they would be 6.5, 65 and 34.5 units lower (§4.2) |

**Capsule height** (capture `pc_head`, unattended, save nvfdoc, Doc Mitchell's house,
`make_closeout_jobs.py`): standing jumps (TapControl 12) after `SetPos` to the floor
(7360; the player read 7364.0 before jumping, so it hovered as after other `SetPos`
drops, §6.2) under ceilings whose height `fnvtool phys cell --ray` gives, twice each
[observed]:

| Spot (x, y) | Ceiling at the axis | Game apex (GetPos z) | Ceiling − apex | NVForever (128.9, cone top) |
|---|---|---|---|---|
| open room (1536, 1340) | 7616 | 7425.54 / 7425.37 (rise 61.5) | — | rise 59.2 from 7360.7 (not limited) |
| doorway beam (1536, 1468) | 7536.1 | 7405.61 / 7405.61 | 130.5 | 7406.48 (+0.87) |
| sloping ceiling (1864, 2116) | 7534.2 | 7404.20 / 7404.20 | 130.0 | 7403.43 (−0.77) |
| sloping (1880, 2116) | 7528.4 | 7398.14 / 7398.41 | 130.1 | 7397.61 (−0.69) |
| sloping (1896, 2116) | 7522.6 | 7392.64 / 7392.66 | 130.0 | 7391.79 (−0.86) |

The head stops 130.0–130.5 below the ceiling (at the axis), whatever the slope: the
player is about 130 units tall including the ceiling's shell. NVForever's character is
now 128.9 tall with its top a mirror of the bottom cone (`fnv_phys::character::HEIGHT`);
the old flat top at 128 overshot the beam by 1.8, a flat top fitted to the beam
stopped 5–8 units low under the slope. The editor documentation says the player "stands
128 units tall" [documented: GECK Layout, level-design.md]. A sneaking jump in the open
rose as high (61.5) [observed]; the crouched height is not captured (the sneak toggle did
not take before the beam jump), so sneaking keeps the standing shape. Walking under the
176-unit doorway passes, standing and sneaking; sneaking walk speed 49.5 units/s =
86.8 × 0.57 [observed: pc_head], the fraction `nvf-play` uses.

**Pushing clutter** (capture `pc_push`, same house, a lane of flat floor at x 1536):
the player runs (Always Run) or walks into an item lying on the floor 150 / 100 units
ahead, forward held 40 loops running or 150 walking, positions sampled every loop
[observed]; NVForever: the same trials in `fnvtool phys walk` with the item as a prop (§7):

| Trial | Game | NVForever (push mass 80) |
|---|---|---|
| tin can, running (3 runs) | carried ahead at the player's speed while touching (84, 78, 106 units in the first 0.26 s), then rolls on at about 290 units/s; 372–451 units in all, through the doorway | carried at the player's speed (77 in 0.25 s), then slows from 260 to 160 units/s; stopped at the wall 198 units on |
| tin can, walking (2 runs) | pushed along at the walking speed: 41 and 30 units | 46 units |
| stimpak, running | the player runs over it; it moves 12.4 units | 12.1 units |
| whiskey bottle | rolled away (180 units) while settling, before the player started: no push | (stays) |

The push itself agrees (an item touched by the player moves with it; a flat item is run
over), so the push mass stays 80. The game's tin can keeps its speed longer after the
contact (less rolling friction or damping) [observed]; not tuned.

Settings stored in FalloutNV.esm whose names suggest movement or falling (values
[observed] from the GMST records; meanings [hypothesis] from the names only):
`fMoveBaseSpeed` 77, `fMoveRunMult` 4, `fMoveSneakMult` 0.57, `fJumpFallHeightMin` 600,
`fJumpFallHeightExponent` 1.65, `fJumpFallVelocityMin` 2785, `fJumpSwimmingMult` 2.
Settings that are not overridden in the masters keep defaults inside the executable,
which is not read (charter). Questions for the owner:

1. Can you record the measurements in the table on a pinned save (console `getpos` /
   `setpos`), or should an xNVSE logging plugin sample the player's position per frame?
2. ~~Is the player's `GetPos Z` the feet (bottom of the capsule)?~~ Yes [observed by the owner in game, 2026-10-08]:
   in Doc Mitchell's house the lowest object, a rug, has `GetPos Z` 7360 and the player
   standing on it 7361.05 (a first aid box on the lowest shelf 7389.94); a capsule centre
   would be about half the capsule height higher.
3. ~~Do disabled references block movement?~~ No: they are not drawn, have no collision
   and no interaction [observed by the owner in game, 2026-10-08] (§4).
4. **Collision layer matrix** (open): which layers block the player and which block
   dropped items. Designed and ready (§4.2): jobs `mv_b11_drops` (player) and
   `mv_b11_clutter` (a bottle), no one at the controls.

### 6.1 The player in the game (`engine/nvf-play`, Layer 4 Part P)

`nvforever --play` drives the controller above with the default `CharacterParams`. Values
it adds:

| Value | Used | Status |
|---|---|---|
| walk speed | 86.8 units/s (`MoveSpeeds::OBSERVED`) | [observed: §6.3, a level-1 character, Speed Mult 100; the formula (encumbrance, sneaking, weapon out) is unknown; `fMoveBaseSpeed` is 77] |
| run speed | 344 units/s | [observed: §6.3; `fMoveBaseSpeed × fMoveRunMult` would be 308] |
| run by default, Shift walks | | [observed: Always Run is on in the owner's game and the Run control is bound to left Shift, §6.3] |
| Caps Lock toggles Always Run (then Shift runs), Ctrl toggles sneaking, Q toggles Auto Move | since 2026-10-09 | [observed: the owner's control map binds Always Run, Crouch/Sneak and Auto Move to these keys; sneaking toggles (mv_values); that Shift runs with Always Run off and that Forward/Back end Auto Move is hypothesis] (docs/design/layer4-play.md §6, input audit) |
| sneaking speed | 197.7/344 of the speed otherwise | [observed running forward, mv_values §6.3; walking and other directions hypothesis]; the capsule and the eye stay as standing (see capsule height) |
| backwards at 0.79 of the forward speed (`BACK_SPEED_FRACTION`); sideways and diagonals at the forward speed, diagonals not faster | | [observed: backwards 272.8 against 344, §6.3; sideways and diagonals hypothesis] |
| eye height above the feet | 112 units | [hypothesis: placeholder for a standing human at about 70 units per metre] |
| the player reference's z rotation is its compass heading in radians | written to the simulation each frame | [hypothesis: door teleport markers' rotations are read the same way] |
| physics (and the game clock) stop in menus | | [hypothesis: not captured for the container menu] |
| swimming | water per loaded cell; swim speed 339 (Shift 86) in every direction along the view, with the stroke cycle of `fnv_phys::swim` for running; a start above deep water falls in | [observed: §6.6] |

Engineering choices, not game values: mouse look 0.0025 rad per mouse count, pitch
limited to ±89°; physics loads the 3×3 exterior cells around the player (one more ring
before unloading) or the one interior, each exterior square with the references placed in
other squares whose bounds reach into it (below), at most one cell per frame, the player's own cell
first, and the character waits while its cell is not loaded; the start drops the
character on whatever a downward ray from the start eye finds (outdoors from far above if
nothing; indoors the start moves to a door arrival, §6.7), on the hit's contact shell (§6.2); `Travel` puts the character where the arrival rule of
§6.2 says (`PhysState::arrive_player` → `PhysWorld::arrival`); a character more than 1,000 units below the
terrain is put back on the ground (logged as a warning); indoors, §6.7.

References get collision by the same rule as `fnvtool phys cell`
(`fnv_phys::ref_collision`); whether one is in the world is the simulation's: not
deleted, enabled following its enable parents (`World::is_enabled`), not taken, whether or
not the simulation holds state for the reference: under an enable parent its own Initially
Disabled flag does not count [design: `engine/nvf-play/src/phys.rs` `in_world`; that the
original's collision follows `GetDisabled` here is [hypothesis], simulation.md §1], while
`fnvtool phys` still lets the flag win [design: `crates/fnvtool/src/phys_cmd.rs`]. In
Goodsprings' 3×3 that gives 839
references with collision against `fnvtool phys cell`'s 849 [observed: `nvforever --play`
debug log: 18 references not in the world, the 8 that `fnvtool` also counts as Initially
Disabled and 10 that the simulation disables through their enable parents].

An exterior square holds the references its cell record lists, the worldspace's persistent
references standing in it, and the references placed in another square whose bounds reach
into it: the base record's OBND box, scaled, turned by the reference's rotation and moved
to its position, against the 4,096-unit grid (`fnv_world::reach`, built once per
worldspace, used by `fnvtool phys cell` too) [design, #2795]. HeliosPowerPlant (REFR
0x000B0A0E, placed in square 5,-1) is HELIOS One's front plaza down to y -8801, in square
5,-3; with only the references placed in the 3×3 a player in 5,-3 stood on the terrain
under the plaza (z 5435-5464 against the plaza's 5476) [observed: `fnvtool phys cell`
rays at radius 1 and 2, #2795]. A reference held by several loaded squares is in the world
once, added with the first and removed with the last. That a base's OBND covers its
model's collision is [hypothesis] (true of HeliosPowerPlant [observed: #2795]). Every
square a box covers lists the reference, with a cell record there or not (physics loads
squares without one too); nothing is cut. A box reaching more than 32 squares from its
own (more likely a broken record: the largest buildings reach two or three) is kept as
a rectangle the squares check by containment, and logged as a warning naming the
reference and its squares [design, #2795]. The index's
build time and size are logged (`physics: worldspace …: N references reach beyond their own
square`) and not measured yet [hypothesis: well under a second for WastelandNV].

Seen in NVForever, not in the game [observed]: walking east into the rock 0x000EDE82
(layer Terrain) at about (-31043, -18055) the capsule jittered in place (about 85 units/s
of back-and-forth path) and crept up 0.2 units/s; `fnvtool phys walk` from
(-31455, -18200) at velocity (600, 0) reproduced it to the unit, so it was the
controller's, not the game integration's. **Cause** [observed: per-step trace]: the
capsule was pressed into a concave corner of the rock's mesh; rapier's controller slides
along one face at a time, the slide along one face pushed it into the other, and the
position cycled with a period of three steps, about 2 units of y per step. **Fix**
(`fnv_phys::character::wedged`): the walls hit while sliding, plus the walls the capsule
already touches (contact manifolds within its gap + 0.5), are collected as horizontal
normals; when every wall the motion pushes into would, after sliding along it, push into
another, the step is re-run without horizontal motion (the multi-plane clip of sliding
controllers: the only free direction is up the crease, which a wall too steep to climb
does not allow). The same walk now stops at (-31047.1, -18053.9) and stays there to
0.01 units, z constant within 0.007 (test `character_wedged_in_a_corner_stays_put` uses
notches of 20°, 30° and 45°; without the rule it fails with 0.5 units of back-and-forth
per second). A slow settling deeper into a narrow notch (under 1 unit/s, monotone) can
remain; it is real free space, not oscillation.

A second rapier quirk found while testing [observed: rapier 0.36.0]: when the sweep meets
an exactly level surface (normal straight up) its slope tangent direction is degenerate,
and if the computed tangent dips a hair below horizontal the whole remaining horizontal
motion of the step is dropped: once in 60 steps of a walk along a terrain cell seam the
character moved 0.06 units instead of 1.28. `move_character` continues such a step with
the horizontal motion it lost, only when every hit of the step was level (test
`walking_on_level_ground_loses_no_step`). Re-check both when upgrading rapier.

A third, about cost (issue #1797) [observed: rapier 0.36.0 source and a per-move timer in
the 50-a-side battle at Goodsprings, 2026-11-07]: the controller's move sweeps the shape at
most 20 times, sliding along what each sweep meets, with the autostep's casts and a
contact pass for the ground after each; pressed into a crease between two meshes every
slide meets the other surface and the normal nudge keeps the motion left over above the
stop threshold, so the move used all 20 and went nowhere. One actor between the terrain
and a canyon rock's mesh (NVNRockCanyon02) took 3 ms a step, one perched between two
yuccas' meshes 22 ms, against about 0.1 ms for the others, every step, the feet exactly
where they were; with up to 8 steps a long frame the battle fell into frames of 400 ms.
A groove of two planes steeper than walkable reproduces it (test
`a_stuck_character_is_held_with_the_steps_it_would_have_had`). **Rule**
(`Character::hold_when_stuck`, on for the actors the game walks, off for the player and
the tools' probes) [engineering choice]: a move whose first controller move met something
on all 20 sweeps and left the feet exactly where they were holds the character there; the
next 19 steps return the same standing step without sweeping, the 20th is a full move
again (which holds it again, or moves it on). A character in the same place with the same
input gets the same step from the controller, so the hold only differs when the input or
the world changed during the third of a second; the test shows the same steps held and
not held in the groove, and none held walking up steps, along a wall, into a steep ramp
and into a corner.

### 6.2 Rest height after a teleport (arrival rule)

Question: why does the player stand at z 8693.87 after leaving Doc Mitchell's house when
the door's XTEL destination is at z 8686.43 (`L4.sim.trace.goodsprings_doc_mitchell`)?

Facts:

- Under the destination (-73162.93, 1312.43) there is only terrain in our collision
  world: the LAND surface, split along the south-west → north-east diagonal (§3), is at
  8690.03 there; the nearest collision of the house (SCOL 0x001788D9, its porch ramp)
  ends about 25 units west; the navmesh triangle under the point (NAVM 0x00177F70,
  triangle 333, the door's XNDP) is at 8689.72 [observed: `fnvtool phys cell` probes on
  a 20-unit grid; NAVM read as NVVX 12-byte vertices and NVTR 16-byte triangles,
  [documented: xEdit]]. So the destination is 3.6 units *under* the terrain and the
  player ends 3.84 above it; no other object, DLC record or enable state explains it
  [observed: the same with all ten masters].
- **Every NIF collision shape of the corpus stores a `radius` of 0.1** besides its
  geometry (box, convex vertices, packed and plain triangle strips: 12,000+ objects in
  `Fallout - Meshes.bsa`; the rest are sphere/capsule-only bodies whose radius is their
  geometry) [observed: `fnvtool collide survey` `shell_radius_by_layer`; field
  [documented: nif.xml]]. At the Havok scale of 7 that is 0.7 game units.
- In-game rest heights [observed, unattended captures 2026-10-08 on the `nvfdoc` save,
  jobs in `tools/capture/jobs/`]. Δ is the player's `GetPos Z` minus the bare surface
  under the point (LAND surface or our collision's static floor):

  | Arrival | Destination vs surface | Δ (settled) |
  |---|---|---|
  | Doc Mitchell's front door (golden; rerun three times) | 3.6 below terrain | 3.844 (rises over ~300 loops from 3.29 at the first loop) |
  | doors 0x00105653, 0x00105690, 0x00105D48 | on the terrain surface | 3.848, 3.848, 3.848 |
  | door 0x00105615 | 8.0 below terrain | 3.484 |
  | door 0x00105652 | 3.53 above terrain | 3.323 (sank 0.21) |
  | door 0x00108F3F | 26.5 above terrain | 3.395 (fell) |
  | door 0x00105BD6 | 2.45 below a static (0x00101CA6) | 0.537 |
  | door 0x00106373 (interior) | 0.53 above the floor | 0.601 |
  | door 0x00105228 (interior) | 24 above the floor | 0.572 (fell) |
  | Doc Mitchell's house from outside (twice) | on the floor | 0.586, 0.553 |
  | `SetPos` drop onto Doc Mitchell's porch (twice) | 61 above the SCOL floor | 1.040, 1.040 |
  | standing on a rug in the house (owner) | on the floor | 1.05 |

  So the player rests in a band **0.54–1.05 above static floors (0.7 − 0.16 to
  0.7 + 0.35) and 3.32–3.85 above the LAND surface**: the same band shifted by 2.8. The
  destination's own height hardly matters (from 8 below to 26 above). Which height in
  the band an arrival ends at is not explained (two arrivals on the bare terrain surface
  and one on a bare floor went to opposite ends).
- Placements by `SetPos` behave alike near the surface (placed 0.5 above flat terrain the
  player rose to 3.39 within 360 loops; placed at 3.85 it stayed) but drops after
  `SetPos` are not reliable evidence: the player sometimes hung in the air for hundreds of
  loops or stopped 7–11 units above the terrain (`dt_rest_detail`) [observed]; why is
  open.
- The terrain's triangulation is confirmed in game: two drops in the non-planar square
  under the destination rested 3.853 and 3.857 above the south-west → north-east split
  and 5.85 / 5.46 above the other [observed: `dt_teleport_rest` p11, p12].

**Rule** (`fnv_phys::arrival`, used by `fnvtool sim run` and `nvforever --play`'s door
travel): a vertical ray from half a capsule above the destination to four capsules below
finds the support; the feet rest on its **contact shell**: `z = surface + shell`, with
the shell of a model's collision object = its largest stored `radius` × 7 × the
reference's scale (`CollisionObject::shell`) and the terrain's `TERRAIN_SHELL` = 3.5
(0.7 + 2.8 from the table) [observed: derived]. That the shells are Havok convex radii,
the terrain's being 0.5 Havok units, is [hypothesis]; so is the band above the shell (the
upper end, +0.35, would be a character "keep distance" of 0.05 Havok units). The rule
reproduces every arrival above within −0.18…+0.35 (the band); for Doc Mitchell's door it
gives 8693.53 against the observed 8693.87. Without a support in range the actor stays
at the destination. History: a first version put the feet at shell + 0.35 (fitted to
Doc Mitchell's door and the porch); its predictions for `dt_door_arrivals` were written
before that capture, which then showed arrivals at both ends of the band, so the rule
now uses the shell, near the middle of what was observed (Δ − shell averages +0.10 over the
fifteen values in the table), rather than the end that matches
the scenario's golden.

**The walking controller keeps the same shells** (`CharacterParams::rest_gap`, one rule
for both: the support's shell, at least the numerical `skin`). Each step a short
downward sweep of the body finds the support and its gap; a body closer than that to a
walkable support (after walking from a static floor onto terrain, after a
`place_character`, on landing) is lifted to it, never into a ceiling, and after the move
it settles onto the support at exactly that gap (pulled down within `step_height` when the
step started on the ground). The distance rapier's controller keeps from everything it
sweeps against (its offset) is a static shell, 0.7, or the support's gap when smaller:
until 2026-10-08 it was the support's gap, so a character on terrain kept 3.5 from walls
(§6.4.1). On level ground a character dropped or walking comes to rest at
`surface + shell`, the arrival rule's height, and stays there (within 0.01 over a
second, the controller's nudges) [observed: tests `character_rests_on_contact_shells`,
`arrival_rests_on_the_support_shell`; `fnvtool phys cell` probes at Doc Mitchell's door
rest 3.55 above the terrain under the feet, 8693.56 against the arrival rule's 8693.53 and
the game's 8693.87, and at the porch drop of `dt_rest_detail` 0.71 above the SCOL floor
against the game's 1.04]. On a slope the capsule keeps the gap along the surface normal,
as a convex radius would, so its feet stand `gap / cos(slope)` above the surface under
the capsule's lowest point [hypothesis: unmeasured in game]. Consequences of rapier
having one offset per character: every obstacle is kept at 0.7 (terrain walls too, whose
own shell would be 3.5) [hypothesis]; walking from terrain onto a static floor flush with it the feet drop by 2.8,
and back again they rise by 2.8 within a step, which matches the band shift but whose
timing in game is not observed. Open: what places an arrival inside the band (−0.18 …
+0.35), and the in-game rest band of a standing player over time.

### 6.3 The player measured in game (`mv_trial`, unattended)

Capture `tools/capture/jobs/mv_trial.steps.json` (2026-10-08, save `nvfdoc`: a level-1
character in Doc Mitchell's house, god mode and AI detection off; the job leaves through
the front door and works on flat LAND at z 8352 around (-72400, 4100)). Method facts
[observed]:

- Held keys move the player with no one at the controls: xNVSE `HoldKey` / `ReleaseKey`
  with the scan code `GetControl <control>` returns (forward 17 = W, back 31, crouch 29,
  run 42 = left Shift, always run 58, jump 57 = Space), and `TapControl 12` jumps
  [documented: GECK wiki, control codes; observed: they work].
- Time base: `GetCurrentTime` (game hours) at TimeScale 30 gives real milliseconds as
  `(hours − start) × 120000`; its steps (15–16 ms, 31 ms when a loop took two frames)
  match `GetSecondsPassed`, which works in a reused query (the plugin compiles each text
  once). Positions lag that clock by one loop: pairing each position with the previous
  loop's clock, a quadratic fits a fall with 0.15 units RMS and a jump with 0.05 (without
  the lag 1.66 and 0.51). After a `SetPos` the world sometimes stood still (positions and
  clock frozen) for up to ~170 loops.
- `$` of a float gives six significant digits ("1234.57"); positions as text keep 0.01.

Values [observed]:

| Quantity | Value | Notes |
|---|---|---|
| gravity | 683.8 (fall from 300 above the ground), 685.5 (standing jump) units/s² | quadratic fits, 54 and 51 samples |
| standing jump | take-off 290.6 units/s, apex 61.6 above the standing feet (8355.85 → 8417.54), 0.845 s in the air | `fJumpHeightMin` reads 64 [observed: GetNumericGameSetting]; how it relates is [hypothesis] |
| run (Always Run on, forward held) | 344.0 units/s over 1 s; 0.25-s windows 322–355; positions within 3.2 of a constant speed | full speed from the first loop (5.44 units in the first 16 ms) |
| walk (Run key held too; IsRunning 0) | 86.8 units/s over 1.45 s; windows 84–91 | run / walk = 3.96 (`fMoveRunMult` 4) |
| rest heights | standing 8355.85 (3.85 above LAND); after the fall 8355.85; after the jump 8357.08 (5.08) | the jump landing is 1.2 above the band of §6.2 |
| settings | `fMoveBaseSpeed` 77, `fMoveRunMult` 4, `fJumpHeightMin` 64, `iActivatePickLength` 150; INI `fJumpAnimDelay:HAVOK` 0.75, `fActivatePickSphereRadius:Interface` 16; Speed Mult 100, scale 1, TimeScale 30, IsRunning 1 | read with GetNumericGameSetting / GetNumericINISetting; setting names [documented: GECK wiki GetCrosshairRef (`iActivatePickLength`), public console guides (`fJumpHeightMin`), stepmodifications.org list of FNV INI defaults (the INI names)] |

Used: `CharacterParams::jump_height` = 61.6; `nvf-play` walks at 86.8 and runs at 344
(`MoveSpeeds::OBSERVED`); gravity stays 686.7 (within 0.4% of both fits). Run and walk
are 11.7% and 12.7% above `fMoveBaseSpeed × fMoveRunMult` and `fMoveBaseSpeed`; the
formula (animation, equipment, encumbrance) is unknown. The male movement clips' root motion is
close to the measured speeds: 85.3 walking, 353.9 running, 282.5 running backwards (a
backward share of 0.798 against the measured 0.793) [observed: animation.md §9.2];
whether the game derives its speeds from the clips is [hypothesis].

Movement scenarios designed from this (`L4.phys.walk.*`, inputs
`parity-lab/inputs/layer4/move_*.walk.json`, job `mv_traces`, golden builder
`tools/capture/capture_moves.py`): the candidate walks at the observed 344 units/s along
the heading the game uses, and its feet every 0.25 s after the run starts are compared
with the game's positions interpolated at the same times after the movement onset
(tolerance 10). The trial's own run is not used as a golden (its speed was measured from
it). Predictions of the candidate (2026-10-08, before capture), feet (x, y, z):

- `move_fence_slide` (start [-72400.0, 4100.0, 8355.5]): t0.25 (-72435.3, 4160.1, 8355.5); t0.50 (-72433.6, 4219.0, 8355.5); t0.75 (-72432.6, 4275.2, 8355.5); t1.00 (-72432.6, 4336.0, 8355.5); t1.25 (-72432.6, 4396.8, 8355.5); t1.50 (-72432.6, 4457.6, 8355.5)
- `move_fence_stop` (start [-72400.0, 4100.0, 8355.5]): t0.25 (-72436.0, 4101.5, 8355.5); t0.50 (-72436.0, 4104.0, 8355.5); t0.75 (-72436.1, 4106.4, 8355.5); t1.00 (-72436.2, 4108.7, 8355.5); t1.25 (-72436.2, 4111.1, 8355.5); t1.50 (-72436.3, 4113.5, 8355.5)
- `move_running_jump` (start [-72400.0, 4100.0, 8355.5]): t0.25 (-72400.0, 4186.0, 8405.3); t0.50 (-72400.0, 4272.0, 8412.2); t0.75 (-72400.0, 4358.0, 8376.2); t1.00 (-72400.0, 4444.0, 8355.5); t1.25 (-72400.0, 4530.0, 8355.7); t1.50 (-72400.0, 4616.0, 8355.5)
- `move_porch_steps` (start [-73162.93, 1312.43, 8693.53]): t0.25 (-73230.4, 1291.4, 8717.2); t0.50 (-73299.1, 1270.0, 8739.6); t0.75 (-73314.6, 1264.4, 8739.3); t1.00 (-73314.3, 1263.5, 8739.3); t1.25 (-73314.0, 1262.7, 8739.3); t1.50 (-73313.8, 1261.9, 8739.3)

Expected differences worth watching: the stop distance from the fence (`move_fence_stop`:
the candidate keeps radius 18 + its 3.5 gap from walls while standing on terrain, 21.3 from
the face), the slide speed along the fence, the jump arc (apex 61.6 now), and whether the
porch steps are climbed at the same places.

Still open, designs not made yet: the step-height limit (needs ledges of graded heights
in reach of a save), the slope limit (a run up LAND squares of known steepness), the
capsule height (owner), push mass (owner).

**Scenario captures** (`mv_traces`, `mv_values`, 2026-10-08, coordinator) [observed]:

- `move_fence_slide` and `move_fence_stop` agree 6/6 (within 1 unit in x and y; the game
  stands 3.85 above LAND against the candidate's 3.5).
- `move_porch_steps`: up the 32° porch ramp the game moved 288 units/s horizontally and
  134 vertically, 316 along the surface after the flat run of 340 (and 333 along the
  30° LAND of `move_hill_run`, 306 + 134): the run speed is kept along a walkable slope
  (the cosine), where sliding the flat velocity gave the cosine squared (244). The
  controller now keeps the speed along uphill walkable slopes, in the vertical plane of
  the heading (`move_hill_run` showed the game keeps its heading across a side slope). It
  passes 6/6; still open: at the top the game stood 7.7 above the porch floor (8746.34
  against 8739.3) after rising above the ramp's end.
- `move_running_jump` 0/6, open. Jump and run started in the same loop: the horizontal
  speed rose 102, 173, 223 … 340 over 0.2 s (the gap to 340 shrinking by 0.70 per loop)
  while the candidate runs at 344 from the first step; after landing two loops at 81
  units/s and later dips to 144–239. Calibration `mv_air` [observed]: jumping out of a
  run at full speed keeps 340 in the air; releasing forward in mid-air decays the speed
  by about 0.84 per loop (to 27 in 0.22 s); pressing forward only after leaving the
  ground gives 15 units/s. **Resolved 2026-10-08** (§6.4.1): one rule fits all four
  (gain 0.30 per loop towards the keys' velocity when a key was down at take-off, else
  towards 14.7; `fJumpMoveMult` reads 0.30), and the landing slowdown is a fixed speed
  profile after every landing with a key held; the scenario passes 6/6.
- Other speeds (`mv_values`) [observed]: backwards 272.8 units/s (0.79 of forward; used by
  `nvf-play`), sneaking forward 197.7 (= 344 × 0.575; `fMoveSneakMult` is 0.57; used by
  `nvf-play` since 2026-10-09); the strafe
  and diagonal runs hit the yard's house and are not usable.

### 6.4 Follow-up captures: predictions (written 2026-10-08 before the runs)

After the first scenario captures (results in §4.2 and §6.3) the candidate changed: probes
load the cells around their own square, open-by-default doors lose their keyframed parts,
Portal Box CollisionMarkers are left out, and the speed up walkable slopes is kept along
the surface. Predictions of that candidate for the next jobs (`mv_followup_traces`,
held-out scenarios `L4.phys.walk.hill_run` and `.portal_wall`; `mv_diag_trees_rocks`,
diagnostic, final feet z):

- `move_hill_run` (start [-76900.0, -3840.0, 8330.75] ): t0.25 (-76979.4, -3851.2, 8360.9); t0.50 (-77058.3, -3865.0, 8389.4); t0.75 (-77137.2, -3878.7, 8417.9); t1.00 (-77214.3, -3893.9, 8449.9); t1.25 (-77288.4, -3913.1, 8486.3); t1.50 (-77363.9, -3931.6, 8520.3)
- `move_portal_wall` (start [-77757.7, 4910.6, 10651.98] ): t0.25 (-77699.7, 4847.0, 10592.9); t0.50 (-77641.7, 4783.5, 10500.2); t0.75 (-77583.7, 4720.0, 10370.6); t1.00 (-77525.7, 4656.5, 10330.5); t1.25 (-77467.7, 4593.0, 10247.6); t1.50 (-77409.7, 4529.5, 10121.7)
- `diag_trees_rocks`: tree_a_-30_-30 8575.0; tree_a_-30_+0 8582.7; tree_a_-30_+30 8582.7; tree_a_+0_-30 8588.8; tree_a_+0_+0 8582.7; tree_a_+0_+30 8368.4; tree_a_+30_-30 8545.8; tree_a_+30_+0 8545.4; tree_a_+30_+30 8358.7; rock_b_-30_-30 8515.3; rock_b_-30_+0 8516.1; rock_b_-30_+30 8480.5; rock_b_+0_-30 8514.6; rock_b_+0_+0 8515.1; rock_b_+0_+30 8482.2; rock_b_+30_-30 8509.1; rock_b_+30_+0 8496.4; rock_b_+30_+30 8498.6; tree_a_high 8582.7

`mv_air` (calibration, no prediction used): horizontal motion when forward is pressed in
mid-air, when jumping out of a run at full speed, when forward is released in mid-air,
and in a walking jump.

**Results of the follow-ups** (captures by this workstream, 2026-10-08) [observed]:

- `move_portal_wall`: the game stopped 22.1 units from the plane of the Portal Box marker
  0x001685B9 (−77689.5, 4840.2) and stayed: the prediction (pass through, from the rule
  derived from one drop) was wrong; Portal Box markers are solid again. With the rule
  reverted the candidate also stops there. On the way down (a 42° slope) the game left
  the ground and fell (vertical speed growing like gravity) where the candidate keeps
  snapping to the slope: z differs by 47 at t0.25; snapping down steep slopes is open.
- `move_hill_run`: the game ran straight west at 306 horizontal + 134 vertical; the
  prediction drifted 87 units south (the slope projection also turned the motion
  downhill), fixed by keeping the heading (above). Golden onset: the builder first took
  the movement onset at a 0.08–0.1 change in z one sample (0.031 s of game clock) before
  the run started, so the first goldens' observation times were 0.031 s early. The
  builder now uses the horizontal position (`capture_moves.py`), and on the owner's
  decision (2026-10-08) both goldens were replaced by a rebuild from the same raw capture
  (layer4-captures.md §1, log). Against the rebuild hill_run passes 6/6 and portal_wall
  5/6: only the airborne z at t0.25 differs (−46.7). Before the replacement, the same
  candidate scored 3/6 and 4/6. Both results were seen before the rebuild was installed,
  so these two scenarios are no longer independent held-out evidence. A fresh
  re-capture to confirm the onset within the game's spread is still to be made.
- `mv_diag_trees_rocks` (drops on a ±30 grid; candidate as now):

| Drop | Game feet (x, y, z) | Candidate z | Δz |
|---|---|---|---|
| tree_a_-30_-30 | (-74330.2, -1196.8, 8693.22) | 8575.04 | +118.2 |
| tree_a_-30_+0 | (-74330.2, -1166.8, 8625.73) | 8582.72 | +43.0 |
| tree_a_-30_+30 | (-74330.2, -1136.8, 8667.81) | 8582.72 | +85.1 |
| tree_a_+0_-30 | (-74300.2, -1196.8, 8697.58) | 8588.77 | +108.8 |
| tree_a_+0_+0 | (-74300.2, -1166.8, 8638.53) | 8582.72 | +55.8 |
| tree_a_+0_+30 | (-74300.2, -1136.8, 8593.50) | 8368.37 | +225.1 |
| tree_a_+30_-30 | (-74270.2, -1196.8, 8697.56) | 8545.81 | +151.7 |
| tree_a_+30_+0 | (-74270.2, -1166.8, 8644.50) | 8545.38 | +99.1 |
| tree_a_+30_+30 | (-74270.2, -1136.8, 8662.17) | 8358.75 | +303.4 |
| rock_b_-30_-30 | (-69686.2, 1588.6, 8515.01) | 8515.33 | -0.3 |
| rock_b_-30_+0 | (-69687.1, 1619.1, 8505.72) | 8516.12 | -10.4 |
| rock_b_-30_+30 | (-69686.2, 1648.6, 8480.76) | 8480.50 | +0.3 |
| rock_b_+0_-30 | (-69656.2, 1588.6, 8514.92) | 8514.61 | +0.3 |
| rock_b_+0_+0 | (-69656.2, 1618.6, 8512.78) | 8515.13 | -2.4 |
| rock_b_+0_+30 | (-69656.2, 1648.6, 8481.66) | 8482.17 | -0.5 |
| rock_b_+30_-30 | (-69625.6, 1588.9, 8505.07) | 8509.06 | -4.0 |
| rock_b_+30_+0 | (-69626.2, 1618.6, 8494.76) | 8496.36 | -1.6 |
| rock_b_+30_+30 | (-69626.2, 1648.6, 8496.67) | 8498.64 | -2.0 |
| tree_a_high | (-74300.3, -1166.8, 8628.46) | 8582.72 | +45.7 |

  Rock b agrees within 4 units at 7 of 9 points (and the game's own repeat of the B11
  centre drop came 6.3 higher than the first), so the rock geometry is right and the B11
  rock rows differ by the game's spread and edge effects [observed]. On the tree the game
  rests 43–303 units higher at every point, where it also stands on nothing in our mesh
  (+0/+30, +30/+30, which the candidate falls through to the ground); its heights follow
  the highest vertex of the branch mesh within 18 units of the drop (within 0–20 at 6 of
  9 points) rather than its convex hull (250–330 higher). Hypothesis: the game's
  controller comes to rest on thin, steep branch triangles and their edges, where rapier's
  controller slides off them. Confirmed in part (§6.4.1): the game's falling player does
  not slide off steep contacts, and its bottom is not a capsule's; the controller now
  rests within 8 at 4 of these 9 points and 8–18 low at the others (tree centre open).

### 6.4.1 Air control and the player's bottom: analysis and predictions (2026-10-08, before `mv_air2` and `mv_diag_fence_edge`)

**Air control, from `mv_air` and the running-jump capture, loop by loop** [observed;
speeds from consecutive positions; loops are 15–16 ms, occasionally 31]:

- With a movement key held in the air the horizontal speed approaches a target by a fixed
  fraction per loop: in the running jump (Forward and Jump in the same loop, taking off
  in the loop the motion starts) 102.1, 172.9, 223.5, 257.8, 282.1, 299.5 … 339.5: the gap
  to 339.5 shrinks by 0.70 every loop (first loop 0.30 × 339.5 = 101.9). Jumping out of a
  run at speed, the speed just after take-off was about 366 and fell back to 339.5 the same
  way (gap × 0.7). Forward pressed 8 loops after the take-off of a standing jump: 7.3,
  10.0, 11.2, 12.0, 13.1, 13.7 … about 14.7 (gap × about 0.7), and stayed there.
- With no key (Forward released 8 loops after a running take-off) the speed falls 214,
  181, 153, 128, 108, 88, 70.5, 54, 38.7, 26.8, 18.7, 13.7 …: × 0.845 per loop above
  100 units/s, × 0.70 below 40.
- **The landing is a fixed speed profile.** After every landing with a movement key held
  the per-loop displacements are the same, loop for loop, whatever the speed in the air
  (15 or 340) and whether walking or running: about 81 units/s for two loops, then 341,
  365, 386, 391, 329, 293, 280, 288, 309, 312, 320, 319, 308, 308, 276, 274, 207, 192,
  160, 144, 175, 239, 379, 382, 394, then 339.5 (four captures: the running-jump
  scenario, `mv_air` run-then-jump, late forward and walk jump; a walking jump lands into
  run speeds). With no key held the player stays put after landing. That is the "two loops
  at 81 and dips to 144–239" of the scenario; the profile looks like an animation's root
  speed [hypothesis].
- No game setting stored in the masters mentions air control, acceleration or friction
  (all GMSTs searched: movement settings are `fMoveBaseSpeed`, `fMoveRunMult`,
  `fMoveSneakMult`, armour/leg penalties, `fJump*` fall damage and `fJumpSwimmingMult`)
  [observed]. The game's INI `[HAVOK]` section has `fJumpAnimDelay` 0.75 and `fMaxTime`
  0.016 among unnamed-purpose values [observed: Fallout_default.ini, read-only]. For
  Oblivion's engine `fJumpMoveBase` and `fJumpMoveMult` are said to govern air control
  [documented: UESP CS wiki, Category:Jumping]; whether this game has them is read by
  `mv_air2`.

Hypothesis H (to be tested by `mv_air2`): in the air the horizontal velocity moves 0.30
of the way to a target each loop; the target is the key's ground velocity when a
movement key was held at take-off, and a small air control (about 15 units/s, 0.043 of
the run speed) when it was not; with no key the speed decays (× 0.845, faster when slow).
Predictions of H for `mv_air2`: *repress* back to 340 (gain 0.3); *back in air* reverses
towards −272 within about 10 loops; *right in air* turns towards the forward-right
diagonal at 340; *back jump* rises to 272 backwards like the running jump forward, then
the landing profile backwards; *forward after 1* (key down before the take-off) 340,
*forward after 3* 15; *walk release* and *apex release* decay as above; *fall forward*
(no take-off) 15 in the air; *press after landing*: the landing profile from its third
loop if it is tied to the landing, plain 340 if tied to the key press.

**The player's bottom against thin geometry.** Sweeping shapes straight down onto our
collision of the B11 tree (`fnvtool phys cell` with a sweep probe, not kept) [observed]:
an 18-unit capsule's first contact is 9–40 units from the game's rest height, both above
and below; the candidate's controller ends 43–303 units under any first contact,
because rapier's controller slides a falling capsule off contacts steeper than the slope
limit, while the game's player stays where it is (its x/y did not change in any tree
drop). No single capsule or flat cylinder radius puts every first contact at or above
the game's rest height except capsules of 26 or more, and those leave the game 4–19
lower. On rock b an 18-unit capsule's first contact matches within 4 at 7 of 9 points.

`mv_diag_fence_edge` (drops 200 above the yard fence's top, which is 3.5 units wide at
8422.4 with LAND at 8352 on both sides, at offsets d from its centre line). Predictions:
the candidate (radius 18, sliding) rests on the top for |d| ≤ 8 (8423.1 at 0, 8421.9 at
+8), slides off to the ground (8355.5) at +12, ±16, ±20, holds 8420.35 at −12, and ends on
the ground beyond. A non-sliding capsule of radius r would rest at 8423.1 − (r −
√(r² − (|d| − 1.75)²)) for |d| − 1.75 < r (r = 18: 8419.6 at 12, 8415.7 at 16; r = 24:
8414.2 at 20, 8407.2 at 24); a flat bottom would stay at 8423.1 out to |d| = r + 1.75.

**Predictions for `mv_diag_fence_low`** (written 2026-10-08 after `mv_diag_fence_edge` and
before this run; the same fence offsets dropped from 10 and 50 above the top, and three
tree-a points from 12 and 50 above the game's earlier rest). Two hypotheses: (C) the
bottom is a cone, sinking 1.46 per unit off the edge whatever the fall: every fence rest
as in `mv_diag_fence_edge` (8411.9 at ±8, 8406.0 at ±12, 8400.2 at ±16, 8394.3 at ±20,
8388.5 at ±24) and the tree points back at their earlier rests or lower; (I) the depth
grows with the speed of the fall (the fence drops fell about 230 units and sank up to 31,
the tree-centre drop fell 118 and sank about 0, the tree drop from 400 above rested 10
lower than from 200, and the `shallow_a` drop that fell 14 units rests above every
shape's first contact): from 10 above the fence rests near a capsule of radius 22.5
(8420 at ±8, 8418 at ±12, 8414 at ±16, 8412 at ±20, 8408 at ±24) and the tree points
at or above their earlier rests.

**Results** (captures by this workstream, 2026-10-08, save `nvfdoc`) [observed]:

- `mv_air2` (a first run recorded no motion at all: held keys and tapped jumps did nothing
  while `SetPos` still worked; the rerun moved normally; cause not known, possibly the
  game window without input focus). Settings: **`fJumpMoveMult` 0.30**, `fJumpMoveBase` 0,
  `fJumpDoubleMult` 0.5, `fJumpFallTimeMin` −1, `fJumpHeightMin` 64 (the 0.30 equals the
  air gain measured loop by loop; that the setting is that gain is [hypothesis]).
  - *repress*: Forward released 10 loops after a running take-off (speed 232, 189, 156,
    126, 103, 82: × 0.81–0.83 per loop), pressed again: back to 340 with the gap × 0.70 per
    loop. H confirmed: a key held at take-off keeps the full target for the whole jump.
  - *back in air*: the velocity reverses towards −271 (the backward run speed) with the
    gap × 0.70; *right in air*: it turns towards the forward-right diagonal at 340 (240,
    240) the same way (then the player hit the yard's house). The full target is the keys'
    ground velocity in any direction.
  - *back jump* (Back and Jump together): the take-off came one loop after the key, so
    the player left the ground already at −271 and kept it. **Landing with Back held** is a
    profile of its own: −292 for two loops, then −242 … −223 … −243 for about 25 loops,
    then −271 (alike in *back in air*).
  - *forward after 1* and *forward after 3* (Forward 1 and 3 loops after Jump; the take-off
    is 2 loops after Jump): both approach 340 like the running jump. In `mv_air` Forward
    pressed 8 loops after Jump (6 after the take-off) gave 14.7: a key pressed up to 2
    loops after the take-off counts as held at it, 6 loops after does not.
  - *walk release* (85 units/s): × 0.70–0.77 per loop; *apex release* (340 at the apex):
    × 0.69–0.70 per loop. Early in the rise from run speed the decay is slower (× 0.81–0.845,
    `mv_air`, *repress*). No single rule found; the controller decays with the gain
    (× 0.70), exact at the apex and when slow; a release early in a running jump travels
    about 18 units less than in game [observed difference, not modelled].
  - *fall forward* is not usable (the 150-unit fall ended during the settling loops,
    before the key); *press after landing*: Forward pressed about 5 loops after a standing
    jump's landing: no motion at all for 44 loops (0.7 s), then 339 at once with no
    landing profile [observed, one case; not modelled: the candidate runs at once].
- `mv_diag_fence_edge` (fence top measured to 0.05 afterwards: 3.54 wide at y 4100, faces
  at x −72460.96 and −72457.42, normals 1.7° off x): the player rested on the fence top at
  every offset up to 22.6 beyond the bare edge, fell to the ground at 25.7 and beyond, and
  kept its x within 0–5 units (some outward slides of 4–6). Its rest sank **1.46 units per
  unit off the edge** (onset 1.3 beyond the edge; RMS 1.3 over 14 drops) — more than any
  rounded bottom allows (a sphere touching an edge e off its axis sinks at most e).
- `mv_diag_fence_low`: the same offsets from 50 above the top sank less (1.3, 5.6,
  12.5, 21.5, 26.4 at offsets 8, 12, 16, 20, 24, means of both sides, against 7–13, 15,
  21–26, 23.5, 30–33 from 200 above): **the depth depends on the speed of the fall** (hypothesis I; C refuted).
  From 10 above the centre drop stopped 6.6 above the usual rest (the `SetPos` hang of
  §6.2), so those drops are biased upwards. Two 200-unit repeats: 8399.73 (8400.26 before),
  and 8402.95 at +12 after sliding 6.2 outwards (8408.48 before without sliding). The
  three tree points from 12 and 50 above the earlier rest ended −0.5 … +10.5 from it
  (8643.2 / 8638.1 at the centre against 8638.5, 8636.2 / 8629.6 against 8625.7,
  8702.0 / 8697.3 against 8697.6).

**Model** (`CharacterParams` defaults; `crates/fnv-phys/src/air.rs`, `character.rs`):

- Air: the horizontal velocity closes `AIR_GAIN` 0.30 of the gap to its target per 1/60 s;
  the target is the keys' velocity when a key was held at take-off (or pressed within
  `AIR_FULL_WINDOW`, 4 steps, after leaving the ground, jumping or walking off an edge),
  else the keys' direction at `AIR_CONTROL_SPEED` 14.7; no key: decay with the same gain.
  The take-off starts from the last step's velocity (a jump pressed with the first
  running step starts from 0).
- Landing: after a touchdown faster than 100 units/s (`LANDING_MIN_SPEED` [hypothesis]),
  29 steps follow `LANDING_SPEEDS` (forward or any key not pointing behind the view) or
  `LANDING_SPEEDS_BACK` (a key behind the view, `CharacterInput::view`), in the keys'
  direction, then the keys' velocity. No key, no motion. Walking landings run too
  [observed]. Not modelled: the sideways sway of up to ±30 units/s during the forward
  profile (about 2 units of drift).
- Body: radius 22, cone bottom (flat tip 3.2, slope 1.58: one fit to the 26 fence drops
  from 50 and 200 units, residuals −5 … +5 split by fall speed); off walkable ground the
  vertical motion does not slide; walls keep 0.7.

**Checks** (candidate after these changes; `fnvtool phys cell` on every drop of the four
drop files, 88 drops) [observed]: 71 within 8 units (33 with the old capsule of 18 that
slid off steep contacts). Against the B11 median golden (merged after these
numbers were taken) the scenario passes 17 of 20 (§4.2). `L4.phys.walk.running_jump` 6/6 (it was 0/6); the other walk
scenarios unchanged. Misses: the tree centre rows (below), the 10-unit fence drops (the
hang), two fence drops where the game slid 5–6 units outwards, `collision_marker_b`
(unchanged), `noncollidable_maize` (unchanged; another workstream). The B11 rock rows
pass with this body (the rock investigation belongs to workstream "B11 rocks and maize",
whose rows these numbers only report). One row moved out of tolerance:
`L4.phys.drops.swim_h2o` `shallow_a` (three of its other rows fail for another reason, §6.6)
ends at −246.3 against the game's −236.0 (it was −241.4 with the old capsule, inside the
8-unit tolerance). The drop starts 14 units above a step: the 30-unit riser of the
catwalk stairs 0x030144E3 (top at −240, floor −270) runs about 7 units west of the
player's axis [observed: vertical rays every 2 units; traced to that riser by the swim
workstream]. The game rests at −236.0, 4 above the step top, like the 10-unit fence drops
that also stopped a few units above the usual rest (the `SetPos` hang of §6.2); the
cone, fitted to falls of 50 and 200 units, sinks about 6 below the top, where slow falls
sink least (`mv_diag_fence_low`). Open, with the fall-speed dependence [observed
difference; not tuned].

**Open: the tree centre.** At five tree-a points (the centre, the B11 drop, and three
grid points) the game rests 10–20 units higher than the model at every fall height
(12, 50, 118, 200 and 400 units), held by branches about 22 off the axis that it barely
sinks onto (the branch at 225° from the centre is 0.9 above the rest), while on the fence
an edge 22 off made it sink 30. Our tree collision is one packed triangle mesh (469
vertices, 834 triangles, layer Trees, body translated, not rotated) and the reference's
rotation follows the engine convention (the opposite tilt sign puts the drops 66–193
too high). The fence top is a triangle mesh too, so "mesh against box" does not explain
it. Ideas for a capture: drops onto a single straight branch or a second thin static edge
at several speeds; a drop from below the branches' level (e.g. `SetPos` 5 above a branch
at the rim).

### 6.4.2 Running down slopes: staying on the ground (playtest list 4 item 8, 2026-10-13)

The owner's report: running down a steep road south of (-64168, -8679) the player played
the jump loop at drops far too small for a fall and kept switching between falling and
running.

- **Cause** [observed: `fnvtool phys walk` from (-64168, -8679) at 344 units/s for 5 s,
  headings 135-225 in 15-degree steps]: at 150 and 210 degrees the character left the
  ground for 22 and 57 steps. Each time one step's settling missed the ground: the first
  support under the capsule after the move was a face steeper than `max_slope_deg` (the
  edge of a static mesh, n.z 0.57; at 210 degrees the 56-62-degree face of rock
  0x0010710A), so the character was not pulled down. From the next step the controller
  no longer counted it as standing (the terrain was 10 units below, beyond the 1-unit
  `SUPPORT_MARGIN`), and it fell freely until its arc met the slope again: on a slope of
  angle a at speed v that takes 2 v tan(a) / g, 0.36 s on the 20-degree road, past the
  body's 0.15 s jump-loop delay and its 0.35 s landing threshold. The touchdown, at
  150-370 units/s, started the landing speed profile (`air::LANDING_SPEEDS`, 81 units/s
  for two steps): the stop-and-go.
- **Rule now** (`fnv_phys::character`): a character that walked off the ground (left it
  without a jump) is put on walkable ground as soon as that ground is within
  `step_height` of the height where it left (`Character::walked_off_z`) - the step-down
  the controller already made from a standing start, kept for the steps after a miss
  [engineering choice; the original's rule is not documented: the ten masters' GMSTs
  hold only `fJumpFallHeightMin` 600, `fJumpFallHeightExponent` 1.65 and
  `fJumpFallVelocityMin` 2785 on falls, all for damage]. The landing profile starts only
  after a drop of more than `step_height` from the highest point in the air
  (`landing_profile`) [hypothesis: it was seen after jumps of 61.6, §6.4.1].
- **Result** [observed]: heading 150 now leaves the ground for 1 step (in `phys walk` and
  in the engine, 60/61 steps grounded at 340-347 units/s); 165-195 unchanged (none);
  210 still leaves it on the rock face steeper than the slope limit (runs of 12, 14 and
  31 steps), a slide the slope limit makes [hypothesis: 45 degrees for the player is not
  captured]. NPC paths: `fnvtool nav walk` from Victor's patrol start to the wells, 7
  steps in the air before, 1 after; none at running speed either way.
- **The body** (`engine/nvforever/src/player_body.rs`): the jump loop also needs the
  ground farther than 24 units below the feet (`Character::ground_gap`, the support found
  at the start of the step) or out of reach, and the landing clip plays only after the
  jump loop [engineering choice]. A player put on something by a door, a test aid or
  furniture counts as on the ground until its next step (the steps wait behind a
  loading screen, a conversation or a menu; meanwhile the body had counted time in the
  air and played the jump loop and a landing after the loading screen).
- Checks: `fnv-phys` tests `running_off_a_low_edge_keeps_the_ground_and_the_speed`
  (fails on the earlier controller), `running_off_a_high_edge_still_falls`,
  `the_landing_profile_needs_a_drop_above_the_step_height`; smoke scenario
  `falling-loading`. The lab's L4 physics walks were not re-run with this change.

### 6.5 Ladders

New Vegas has no climbable ladders: the only ladder-like uses are door-style transitions
that teleport the player up or down [observed by the owner in game, 2026-10-08]. They are
load doors (or activators with a teleport) and go through the existing door/teleport path
(`Travel`, the arrival rule of §6.2); the controller has no ladder mode.

### 6.6 Swimming

**Water in the data** [observed: the ten masters, read-only, 2026-10-08]. A water surface
belongs to a cell with CELL DATA bit 1 ("has water"): at its XCLW height, or at the
worldspace's default (WRLD DNAM second float) when XCLW is absent or the 3.4e38 sentinel
(world.md §6; the fallback is [hypothesis] there and here). WastelandNV's default is
−2300 (DNAM −2500 land, −2300 water); 44,025 exterior cells of all worldspaces carry the
flag, most with the surface far below their land. In WastelandNV 767 cells have land below
their surface: 421 at −2300 (the Colorado River below Hoover Dam, Cottonwood Cove), 330 at
2600 (Lake Mead: Boulder Beach, Callville Bay, the Fisherman's Shack), Lake Las Vegas
(3000, 4 cells), the Devil's Throat (2290), the Gypsum train yard (4050), Camp Forlorn Hope
(5360) and five single cells (4100 to 7800). Interior cells with the flag and a real XCLW:
47, e.g. `SLGoodspringsCaveINT` (204), `Vault22b` (7320.1), the Old Vegas sewers
(`OVWestSewers01` 5360 …), `NVDLC03SLH2OINT` (−232); others store 14095.9 or heights
above their floors whose meaning is not checked.

Chosen for captures: **Boulder Beach** on Lake Mead, `BoulderBeachCampground`
(WastelandNV (13, 9), surface 2600 in XCLW of the shore cells). From the shore point
(56992, 37536), LAND 12 above the surface, the bed falls east at about 12°: 44 below the
surface 256 units out, 140 at 768, 326 at 1536, about 800 at the deep point (59500, 38500);
`fnvtool phys walk` east from the shore touches only terrain for 1,735 units.

Settings in the masters whose names mention water or swimming [observed: GMST records]:
`fJumpSwimmingMult` 2, `fSwimRadiationDamageMult` 0.1, `fExplosionWaterRadiusRatio` 0.1;
no breath or drowning setting is stored in the masters (any would be an executable
default, not read). Script functions: `IsSwimming`, `IsInDangerousWater` [documented:
script-functions-catalogue.md]; no function to read breath exists without JIP (not
installed).

**Predictions (written 2026-10-08, before the captures `sw_trial` and `sw_fall_breath`,
`tools/capture/jobs/make_swim_jobs.py`).** Game, all [hypothesis], from nothing but a
human of 128 units in water:

- (a) Walking in, `IsSwimming` turns 1 when the feet are about 90 units below the surface
  (water at the chest, 0.7 of the capsule) and the feet stop following the bed; walking
  out it turns 0 at the same depth (no hysteresis).
- (b) Floating, the feet stay about 90 below the surface (eye just above it), within a
  unit, no bobbing.
- (c) Swimming is slower than running: forward about 100–150 units/s with Always Run, the
  Run key (walk) slower, backwards slower than forwards, sideways like forwards.
- (d) Forward with the view pitched moves along the view (vertical speed = speed ×
  sin(pitch)); pitch alone does nothing; released under water the player rises back to
  the floating height; the Jump key at the surface does nothing or a small jump
  (`fJumpSwimmingMult` 2 suggests a multiplier for jumping out of water).
- (e) A fall into deep water stops in the water with no damage; the same falls onto land
  cost health from 700 units up.
- (f) Held under water with god mode off, health starts falling after some tens of
  seconds (drowning).

Candidate before this work (`fnv-phys` ignores layer 11 Water and has no water volumes):
walking in, the character keeps walking along the bed and ends 780 loops later at about
2380 (`phys walk` above: z falls 2617 → 2233 over 20.5 s); it never swims, floats or
dives,
and a fall into the lake ends on the bed.

**Captures** (unattended, save `nvfdoc`, out of Doc Mitchell's house, `coc
BoulderBeachCampground`, god mode and AI detection off; outputs kept privately in
`C:\fnv-private\capture\swim\`). Method facts [observed]: a script variable named `fW` does
not compile (`fSw` does); arriving at the beach (by `coc` or by `SetPos` from Goodsprings)
leaves `MenuMode` 1 with the game clock stopped until the console's `CloseAllMenus`
(`sw_diag`, `sw_diag2`); a first `sw_trial` run from `nvftest` recorded nothing for that
reason. `IsSwimming` compiles and reads **0 throughout**, also while the player floats and
swims (so the swimming state is read from positions, not from it); `IsInDangerousWater`
0; `GetAV Rads` does not compile.

`sw_trial` (2026-10-08, 8,352 steps) [observed]; S = the surface, 2600; depths are of the
feet below S; speeds are linear fits over the held windows with each position paired with
the previous loop's clock (§6.3):

- **Walking in, the player never swims.** Walking (84.7 units/s) and running (334.9 along
  the ground) east down the 12° bed it kept walking on the bed to 213 and 353 below S, and
  stood there (head far under water). (a) as predicted is refuted: being on the ground
  wins over the water.
- **Entering by falling.** `SetPos` to S over 800 units of water from the land: free fall
  (accelerating like gravity) until the feet passed S − 100 (the last loop above had them
  94.5 below, the first one below 100.2), then the fall stopped at once and the player sank
  slowly to **S − 102.52** and stayed there (b: the floating height; predicted about 90).
- **Sinking to the floating height**: 12.0–12.5 units/s (fits over 13 placements at S while
  already swimming; `SetPos` keeps the swimming state, so a placement above the floating
  height sank at this speed instead of falling).
- **Swim speeds** (c): forward 352.2, backward 343.3, right 341.1, left 341.5, forward-right
  346.1, forward with the Sneak control tapped 351.6 (`IsSneaking` stayed 0: no sneaking in
  water), along the view while diving 351.2–351.7; walking (Run key held) 100.8. So the
  run speed (344 on land) in every direction, backward included (0.79 on land), and walking
  16% faster than on land (86.8).
- **Diving** (d): forward moves along the view: pitch 30° down 304.4 horizontal / 176.0
  down, 60° 175.8 / 304.6, 89° 6.1 / 351.1 (tan of the ratios 30.0°, 60.0°, 89.0°). The view
  pitched with no key: no motion (still sinking at 12.2 to the floating height). Released
  under water the player **stays at its depth** (5 s, z unchanged to 0.01): no buoyancy.
  Swimming up at 45° (240 up, 240 forward) stops at the floating band: once the feet were
  above S − 100 the upward motion stopped and the 12-units/s sink took over, so the feet
  went up and down between S − 97.5 and S − 102.5 while forward was held. Back on the bed
  (89° down) the player stayed on it.
- **Leaving the water**: swimming (sinking towards the floating height) towards the shore
  until the feet reached the bed's contact shell (4.3 above LAND), then walking up the bed
  at the ground speed.
- **Jump** (d): Jump tapped while sinking towards the floating height (61 below S): a
  ballistic jump of apex **124.2** above the take-off (2 × 61.6 = 123.2: the land jump
  height × `fJumpSwimmingMult` 2) under gravity. Afterwards the player hung in the air
  4.75 above S for the rest of the run (positions frozen while the clock ran, through
  further `SetPos`; like the hanging after `SetPos` drops of §6.2): the later jump and
  sneak-key tests are not usable.

**Predictions for `sw_fall_breath`** (written before its run, 2026-10-08; jump from the
floating height, falls of 300, 700, 1500 and 3000 units into the lake and onto the land
west of the beach with god mode off and Health raised by 5,000, two minutes held at the
bottom): from `sw_trial`, a fall into the water stops at S − 100 and the player then
sinks at 12 units/s to S − 102.5, losing no health at any height [hypothesis]; the land
falls lose health from 700 up (`fJumpFallHeightMin` 600) [hypothesis]; the jump from the
floating height rises 123 [hypothesis]; health falls while held under water after some
seconds [hypothesis].

`sw_fall_breath` (2026-10-08) [observed]; it ended at its last land fall (3000 units:
Health 3094.7 → −6324.9, the player died, the game reloaded the save and the plugin aborted
the job each time until the run was stopped), so the part under water was not reached:

- **Jump at the floating height**: Jump tapped twice with the feet settled at S − 102.5:
  no motion at all (4 s each). With `sw_trial`'s jump (taken while still sinking 61 below
  S) the jump works only above the floating height.
- **Entering from a fall** (`SetPos` from the land to S, standing start, then 300, 700,
  1500, 3000 above S): the fall stopped with the feet 100.0 below S (from S: the step that
  crossed ended at 2500.00 exactly, then the 12-units/s sink to S − 102.5), 105.97 (300),
  114.79 (700), 123.75 (1500) and 149.41 (3000) below S; after the stop the player stayed at
  that depth (below the floating height: no rise). Falling at 340–2,030 units/s the last
  steps before the stop were 5.3–33 units; at 1500 and 3000 the player moved one more
  loop's step after the loop that crossed S − 100 (at 700 it did not), so the stop is not a
  clamp at the line; which loop the game decides on is open.
- **No fall damage in water** (e): Health unchanged (5200) after every fall into the lake.
  On land: 300 → none; 700 → −46.7; 1500 → −1858.6; 3000 → −9419.6 (dead). With h the
  height fallen (start feet to landing feet: 695.6, 1495.4, 2995.3) the damage fits
  k·(h − 600)^1.65 with k ≈ 0.0252 within 0.8% (exponent from the three falls 1.647 and
  1.649): `fJumpFallHeightMin` 600 and `fJumpFallHeightExponent` 1.65 [the link to the
  settings is a hypothesis]. Two later drops of 40 Health each, 1–3 s after the 700 and
  1500 landings (once with the player pushed 130 units sideways), are not explained.
- **Falls damage a leg** [observed: o6_00, 2026-10-31, nvfairport runway, god mode off,
  Health raised by 6000, `player.SetPos z` +700, +1000, +1300 and Health read for 12 s]:
  −49.88, −491.86 and −1236.13 Health (the formula above gives 50.3, 494.7 and 1246), and
  each fall hurt **one** leg: +700 took the right leg from 100 to 75.06 (24.94, half the
  Health lost), +1000 crippled the left (0), +1300 the right (0); the other limbs stayed
  100 [hypothesis: half the fall's damage to one leg chosen at random]. No later drops
  in the 12 s after any landing (the 40-point drops above did not recur).

**Predictions for `sw_breath`** (written before its run): two minutes pressed against the
bed 800 below S with god mode off: Health drops after a breath time of some tens of seconds
and then keeps dropping [hypothesis]; the 700 fall stops 100–117 below S [hypothesis: one
step of 16.5 past the line at most, by the controller's rule below].

`sw_breath` (2026-10-08) [observed]:

- **Drowning** (f): the player dived (89° down, forward held) to the bed 800 below S
  within 2.2 s and was held against it. Health (5200 after `ModAV Health 5000`, god mode
  off) stayed at 5200 for **10.0 s** after the dive started (the head was under water
  within a tenth of a second: the eye is about 123 above the feet, §6.3, and the feet
  started at S − 102.5), then fell linearly at **40.0 Health per second** (fit over 100 s,
  39.97) to 1.17, then 0.57, when the player shot 60 units up and rose at about 100
  units/s (dead, [hypothesis]). Whether the 10 s and the 40 per second depend on the
  character (Endurance, level, the 200 base Health) is not known (one character).
  Released at the bed after 128 s the player stayed there (no rise) while drowning.
- The 700-unit fall into the water stopped at exactly the same place as in
  `sw_fall_breath` (2485.21, 114.79 below S; same last steps 2518.52, 2501.86, 2485.21).

**The swim rules in `fnv-phys`** (`CharacterParams::swim_depth` 100, `float_depth` 102.5,
`swim_sink_speed` 12.1, `swim_jump_mult` 2; `crates/fnv-phys/src/character.rs`,
`swim_vertical`; water areas `crates/fnv-phys/src/water.rs`). Water is a flat surface over
each loaded exterior cell's square with `Cell::water_surface` (CELL XCLW, else the
worldspace default, for cells flagged "has water": the renderer's rule), plus the box of
each placed water reference (below); no collider. (Interiors first used their XCLW too;
`sw_h2o` refuted that, below.) A character **starts swimming** after a step that ends with it off
the ground, falling, with its feet `swim_depth` or more below the surface: its fall stops
there, with no landing (so no fall damage) [observed]; the stop is after that step, not
clamped to the line (the game's extra step at high speed, above, is not modelled). While
swimming there is no gravity; the caller's horizontal velocity applies and its
`vertical` velocity (forward speed × sine of the view pitch in `nvf-play`) applies when
diving, and when rising only from the floating height or deeper; above the floating
height the character otherwise sinks at 12.1 units/s and stops at it; below it nothing
moves it vertically (no buoyancy) [all observed]. Jump works only above the floating
height and gives a jump of `swim_jump_mult` × `jump_height` that leaves the water
[observed: the two cases above]. Swimming **ends** when the swimmer moves down onto
walkable ground (it then walks) [observed: leaving the lake], when it jumps, or where there
is no water at all [hypothesis]; feet above the surface do not end it [observed:
`swim_h2o_drops`, below; the first implementation ended it there]. A character on the ground never starts swimming,
however deep [observed: walking in] (**superseded 2026-10-18**: a character on the ground
starts swimming `wade_depth` below the surface, see *Walking in again* below). The swimming state survives `place_character`, as
the game's does across `SetPos` [observed]. Walking speeds are the caller's: `nvf-play`
swims at 348 (Always Run) and 100.8 (walk) in every direction, along the view
(`SWIM_SPEEDS`, `swim_velocity`) [observed: fits 341–352; walking 100.8]. Breath and
drowning are not the controller's: drowning.md. Not modelled: the hanging after the jump in
`sw_trial`, swimming state reported to scripts (`IsSwimming` reads 0 in the game).

`fnvtool phys walk` scripts take `swim_velocity` ([x, y, z], used instead of `velocity`
while the character swims) per segment and the swim parameters in `params`; traces and
observations report `swimming`, `phys cell`/`walk` documents list `water` (§7).

**Held-out scenarios** (designed after the Boulder Beach captures, at two other waters;
inputs `parity-lab/inputs/layer4/swim_*.walk.json`, job `sw_scenarios` made by
`make_swim_jobs.py` through `capture_moves.py` with the new `reset_at` and `pitch_deg`
capture fields; the player first stands on land near the water, so it is not swimming
when placed at the surface). **Predictions of the candidate** (written 2026-10-08, before
the capture), feet (x, y, z):

- `swim_cove_dive` (Cottonwood Cove, surface −2300 from the worldspace default; start
  [70200, −91264, −2300], heading 0, pitch 40° down, forward held): t0.25 (70200.0,
  −91197.3, −2458.4); t0.50 (70200.0, −91130.6, −2514.3); t0.75 (70200.0, −91064.0,
  −2570.3); t1.00 (70200.0, −90997.3, −2626.2); t1.25 (70200.0, −90930.6, −2682.1); t1.50
  (70200.0, −90863.9, −2738.0). Before the keys: floating at −2402.5.
- `swim_lake_shore` (Lake Las Vegas, XCLW 3000; start [25784, 55100, 3000], heading 270,
  forward held): t0.25 (25697.0, 55100.0, 2897.5); t0.50 (25610.0, 55100.0, 2897.5); t0.75
  (25523.0, 55100.0, 2897.5); t1.00 (25436.0, 55100.0, 2897.5); t1.25 (25348.9, 55100.0,
  2897.5); t1.50 (25261.9, 55100.0, 2897.5, walking from here); t1.75 (25185.6, 55099.7,
  2932.9); t2.00 (25109.8, 55099.4, 2970.7).

Watch: whether the Cottonwood Cove water is at −2300 (the worldspace-default
[hypothesis]); the swim speed 348 against the run's 344 (the onset alignment); where the
swimmer meets the shore and how fast it walks up a 27° bank out of water.

**Held-out results** (`sw_scenarios`, 2026-10-08; goldens `phys/swim_cove_dive.walk.json`,
`phys/swim_lake_shore.walk.json`) [observed]:

- Cottonwood Cove floats at −2402.52: the surface there is the worldspace default −2300
  (the fallback of `Cell::water_surface` is confirmed for one place). The dive followed
  the view at 40°. Lake Las Vegas floated at 2895.22 (104.78 below 3000; the entry from a
  standing start overshot the floating height there).
- Against the predictions: `swim_cove_dive` within 1.4 at t0.25 and t0.50 and 9.2 at t0.75,
  then up to 27 in y and 23 in z off (lab 3/6); `swim_lake_shore` within 2.3 to t0.50, then
  up to 35 off, and the exit differs (lab 3/8). **Refuted: the constant swim speed.** The game's speed varies with time
  since the motion started, the same way in both captures and in every forward run of
  `sw_trial` once looked at in sixteenths of a second: 339 units/s for 6/16 s, then a cycle
  of 23/16 s rising to 412 and falling to 160 whose mean is 339.1; backward and sideways
  runs have flatter cycles (313–370–284 and 288–379) of the same period and mean
  (`sw_trial`: back, right, left, forward-right and sneak-forward runs; the walk at 86
  units/s varies too, 82–122 over 2 s, no full cycle seen). The constant 348 of the first
  implementation was a 2-s average over part of a cycle. The candidate now models the
  cycle (`fnv_phys::swim`, tables from `sw_trial` only: forward = mean of the four forward
  and diving runs, backward and sideways from their runs; walking constant at 86
  [hypothesis]); `fnvtool phys walk` applies it to a segment's `swim_velocity` with
  `"stroke": "forward" | "back" | "side"`, and `nvf-play` from the keys. The two scenario
  inputs above were left as they were (constant 348, no stroke): they keep failing.
- **Refuted / open: leaving the water up a steep bank.** At Lake Las Vegas (bank about 27°)
  the swimmer never walked: it kept the swimming state, its z stayed at the floating
  height (then sank at 12 units/s) while it moved into the bank until its feet were 22–31
  below the LAND surface under them, then rose about 30 in one loop to the surface, and
  repeated (six steps up), ending 74 above the water surface still sinking at 12 units/s
  and blocked. At Boulder Beach (12°) the swimmer walked out. The candidate, which lets a
  swimmer that reaches walkable ground walk, walked up the 27° bank. Not modelled (one
  capture; the feet 30 under the terrain suggest a different collision shape while
  swimming [hypothesis]).

**Predictions for the second held-out set** (written 2026-10-08 before `sw_heldout2` and
`sw_h2o`; candidate with the stroke cycle), feet (x, y, z):

- `swim_dam_forward` (Lake Mead above Hoover Dam, XCLW 2600; start [77696, 33024, 2600],
  north, forward held, stroke forward): t0.25 (77696.0, 33108.7, 2497.5); t0.50 (77696.0,
  33195.1, 2497.5); t0.75 (77696.0, 33294.4, 2497.5); t1.00 (77696.0, 33396.8, 2497.5);
  t1.25 (77696.0, 33492.3, 2497.5); t1.50 (77696.0, 33571.1, 2497.5); t1.75 (77696.0,
  33624.0, 2497.5); t2.00 (77696.0, 33706.6, 2497.5); t2.25 (77696.0, 33807.6, 2497.5); t2.50
  (77696.0, 33909.2, 2497.5).
- `swim_bay_back` (Lake Mead off Callville Bay, XCLW 2600; start [50560, 62720, 2600],
  facing north, Back held, stroke back): t0.25 (50560.0, 62635.3, 2497.5); t0.50 (50560.0,
  62554.1, 2497.5); t0.75 (50560.0, 62469.1, 2497.5); t1.00 (50560.0, 62379.1, 2497.5); t1.25
  (50560.0, 62287.3, 2497.5); t1.50 (50560.0, 62200.3, 2497.5); t1.75 (50560.0, 62125.1,
  2497.5); t2.00 (50560.0, 62044.5, 2497.5).
- `swim_h2o_drops` (interior NVDLC03SLH2OINT, XCLW −232; drops at −227): deep_a (−300,
  −1000), deep_b (−300, −800), mid_a (−500, −200), mid_b (−700, −200) all floating at
  −334.5 (the floors are 456, 496, 136 and 136 below the surface); shallow_a (−1098.0,
  −550.0, −242.0), standing on the floor 38 below the surface (−270 + its shell).

**Results of the second held-out set** (`sw_heldout2`, `sw_h2o`, 2026-10-08; goldens
`phys/swim_dam_forward.walk.json`, `phys/swim_bay_back.walk.json`,
`phys/swim_h2o_drops.drops.json`) [observed]:

- `swim_dam_forward`: 10/10 within 2.6 units (the game floated at 2494.9, 105.1 below the
  surface, as at Lake Las Vegas: the standing-start entry overshot the floating height);
  `swim_bay_back`: 8/8 within 2.8. The stroke cycle (forward and backward tables from
  `sw_trial`) holds at two other places.
- `swim_h2o_drops` **refutes interior water at XCLW**: dropped 5 above the interior's XCLW
  (−232) the player fell freely through it and stopped at −432.36 (deep_a, then stayed);
  placed again at −227 while swimming it sank at 12 units/s through −289 and −350 (deep_b,
  mid_a, mid_b, still sinking after 10 s). That fits a surface near −330: the cell holds a
  placeable water reference, `NVDLC03CleanWater2048x2048` (PWAT 0x03014467, model
  `water_placeable\waterp2048x2048.nif`, scale 2) at z −328, centred at (−584, −656), whose
  floating height would be −430.5. shallow_a ended at −236.04, 34 above the floor our
  collision has there (−270), not explained. The interior part of the swim implementation
  (water at an interior's XCLW) is **reverted**: interiors have no water in `fnv-phys`
  until placed water is modelled (its extent from the model, its height from the
  reference: [hypothesis], one interior). Exterior cell water is unaffected (four places
  agree). The renderer still draws interior water planes at XCLW (Layer 3, unchanged).

**Should `phys walk` apply the stroke cycle by default?** (decision 2026-10-08) No: a
segment swims at its `swim_velocity` unless it names a `stroke`. The game's speed follows a
stroke whenever a movement key is held [observed: every run], but (1) which stroke depends
on the key (forward, back, sideways cycles differ), and a segment's velocity does not say
which key the game held; (2) the cycle is a fraction of the game's 339, so applied to a
segment's own speed it reproduces the game only when that speed is 339; the two old
inputs ask for 348 (the 2-s average of the first implementation), and treating
`swim_velocity` as a direction only would make an explicit number meaningless (and the
walking swim, 86, has no cycle [hypothesis]). So `swim_cove_dive` and `swim_lake_shore`
cannot express what the game does; with the inputs unchanged they keep failing. Checked
with scratch copies of the inputs (not in the lab): `"stroke": "forward"` at 339 along the
same directions brings `swim_cove_dive` to 6/6 (within 2.1 units; **the dive's 23–27-unit
drift from t0.75 is the stroke cycle**: at t1.00–t1.50 the game is past the stroke's peak
of 412 units/s) and `swim_lake_shore` to 5/8 before the bank (within 2.3); the stroke at 348
drifts by 2.7 % (9.3 units at t1.50 of the dive). Question for the owner: retire the two
scenarios and replace them with inputs that name the stroke at 339 (and, for the shore, a
capture with the cell loaded; below)?

**Retired and redesigned (owner decision 2026-10-09).** `L4.phys.walk.swim_cove_dive` and
`.swim_lake_shore` are retired (their inputs removed; the goldens stay private and unused):
their inputs ask for a constant 348 units/s that the game never swims, and the shore's bank
rows were captured with the cell not loaded. Replacements, at places no capture has used,
with the forward stroke at 339 and each capture loading its cell with `cow` first (job
`sw_redesign`, `make_swim_jobs.py` `redesign()`):

- `L4.phys.walk.swim_cove_stroke`: Cottonwood Cove (surface −2300) from (70250, −90750),
  heading 45°, view 30° down, forward held 2 s over 604–625 units of water (terrain only).
- `L4.phys.walk.swim_lake_exit`: Lake Las Vegas (3000) from (29875, 56623), heading 132.3°,
  level, forward held 2.5 s to the 24.1° bank of cell (7, 13) and up it (terrain only).

**Predictions of the candidate** (written 2026-10-09 before the capture), feet (x, y, z):

- `swim_cove_stroke`: t0.25 (70301.9, −90698.1, −2444.9); t0.50 (70354.8, −90645.2, −2488.1);
  t0.75 (70415.6, −90584.4, −2537.7); t1.00 (70478.3, −90521.7, −2588.9); t1.25 (70536.8,
  −90463.2, −2636.7); t1.50 (70585.1, −90414.9, −2676.1); t1.75 (70617.5, −90382.5, −2702.5);
  t2.00 (70668.0, −90332.0, −2743.8).
- `swim_lake_exit`: t0.25 (29937.7, 56566.0, 2897.5); t0.50 (30001.6, 56507.9, 2897.5); t0.75
  (30075.1, 56441.0, 2897.5); t1.00 (30150.8, 56372.1, 2897.5); t1.25 (30220.9, 56308.4,
  2900.9, walking); t1.50 (30279.0, 56255.4, 2935.6); t1.75 (30337.2, 56202.4, 2970.4); t2.00
  (30396.1, 56148.8, 3002.6); t2.25 (30452.0, 56098.0, 3043.9); t2.50 (30508.9, 56046.2,
  3081.3).

Watch: the floating height after a standing-start entry (102.5–105.1 below in earlier
captures; the candidate 102.5), and the walk up the 24° bank (the candidate climbed the
steeper banks of `sw_banks*` faster than the game).

**Results** (`sw_redesign`, 2026-10-09; goldens `phys/swim_cove_stroke.walk.json`,
`phys/swim_lake_exit.walk.json`; MenuMode 0 and the clock running throughout) [observed]:
`swim_cove_stroke` 8/8: x and y within 1.2 at every row; z 3.5–4.5 lower in the game, which
floated at −2405.98 (106.0 below the surface, the entry overshoot) where the candidate
floats at 102.5. `swim_lake_exit` 10/10: within 1.4 while swimming (to t1.25, when both
reach the bed); walking up the 24° bank the candidate moves ahead, 3.6 at t1.50 growing to
9.7 at t2.50 (inside the tolerance of 10; the faster climb on slopes seen in `sw_banks*`,
a land-controller question). The stroke cycle at 339 and the exit onto the bank hold at
two more places.

**Leaving the water: what the two shores show** (re-read of `sw_scenarios` and `sw_trial`
against the LAND under each sample, `fnvtool phys cell` rays with and without meshes,
2026-10-08) [observed]:

- At Lake Las Vegas the swimmer met no LAND at all. Floating at 2895.22 it moved west at
  the stroke speed into the 27.3° bank with z unchanged, so its feet went 0 → 29.3 below
  the LAND under them over ten loops; in the next loop z rose 30.5 and the feet were
  **exactly on the LAND surface** (0.01 above it, no contact shell); then the same again:
  the depth before each rise was 28.3–30.6 and after it 0.01, 0.02, 1.1, 2.5, 2.8 and 0.7
  below the LAND, six times, with the 12-units/s sink between rises and the horizontal
  speed unchanged throughout (5.5–6.6 units per loop). It ended blocked by the rock
  references at the top of the bank (layer Terrain), 7.6 below the LAND under it.
- At Boulder Beach the swimmer collided with the LAND as a walking capsule does: floating
  at S − 102.49 and walking towards a 33.9° slope, z stayed level until the LAND under the
  feet was 7.7 below them, then the feet rose with the bed 4.4–4.8 above it (walking); the
  run-out line, swum while still sinking, met 28° and rose 4.3 above the LAND. So the
  slopes at the two places are alike (the run-out line's 28–34° is not the 12° average of
  the walk-in line) and the bank angle does not explain the difference.
- That after each rise the feet are on the bare surface, not on its 3.5 shell, points to a
  placement from the LAND heights (or a ray), not to the swept capsule [hypothesis].

Candidate explanations [hypothesis]: (P) the place: the swimmer's capsule does not collide
with the LAND at Lake Las Vegas (why is open), and something puts the feet back on the LAND
surface when they are about 30 below it; (S) the state: the Lake Las Vegas swimmer had
overshot to 104.78 below the surface while the Boulder Beach one floated at 102.49, or ran
where the other walked, and that changes its collision.

**Predictions for `sw_banks`** (written 2026-10-08 before its run; job
`tools/capture/jobs/make_swim_jobs.py` `banks()`). Each trial stands on land, is put at the
surface 450 units out from where the bed reaches the floating height (enters by falling),
floats for 5 s and then holds forward (Always Run; one trial walks) up the bank for 4 s
(8 s walking). Banks of terrain only, found from the LAND heights: Lake Mead (S 2600) at
12.5°, 16.4°, 25.3°, 30.4°, 41.5°, 45.1°, 49.3° and 58.3°; Lake Las Vegas (S 3000) at 14.3°
and 24.7°; and the two lines above again (Lake Las Vegas running and walking, Boulder
Beach running from the floating height). The game, from (P): every Lake Las Vegas trial
shows the saw-tooth (feet into the LAND by about 30, then on its surface) whatever the
slope and speed; every Lake Mead trial meets the bed with its capsule, walks out up to
the steepest walkable slope (45° in the candidate, itself [hypothesis], §6.3) and stays
in the water below steeper banks. From (S): a trial that floats 102.5 below the surface
walks out wherever the slope allows, at either lake. The candidate (stroke cycle forward,
walking 86; feet when swimming ends, seconds after the keys, and after 4 s):

| Trial | Floats at | Stops swimming | Feet after 4 s (8 s walking) |
|---|---|---|---|
| `llv_line` | 2897.5 | (25260.7, 55100.0, 2897.5), 1.42 s | (24853.9, 55178.2, 3096.8), walking |
| `llv_line walk` | 2897.5 | (25263.6, 55100.0, 2897.5), 6.05 s | (25120.8, 55097.7, 2965.0), walking |
| `bb_line` | 2497.5 | (57654.8, 37536.0, 2497.5), 0.13 s | (56383.2, 37551.1, 2823.3), walking |
| `lm_12` | 2497.5 | (53036.9, 41260.9, 2497.5), 1.03 s | (52376.3, 40600.2, 2855.5), walking |
| `lm_16` | 2497.5 | (52732.2, 41467.0, 2497.5), 1.22 s | (52273.9, 40727.0, 2855.9), walking |
| `lm_25` | 2497.5 | (127744.0, 31263.2, 2497.5), 1.12 s | (127746.2, 30366.3, 2886.9), walking |
| `lm_30` | 2497.5 | (126233.8, 96529.7, 2497.5), 1.12 s | (125606.1, 96021.3, 2949.9), walking |
| `lm_41` | 2497.5 | (148483.0, 29976.2, 2497.5), 1.13 s | (148414.1, 29270.0, 3068.6), walking |
| `lm_45` | 2497.5 | (73210.5, 43005.3, 2497.5), 1.18 s | (73328.8, 43001.9, 2574.0), stopped on the slope |
| `lm_49` | 2497.5 | (126474.7, 101636.6, 2497.5), 1.17 s | (126336.5, 101799.5, 2498.2), at the foot of the bank |
| `lm_58` | 2497.5 | (78840.5, 37877.0, 2497.5), 1.17 s | (78844.3, 37875.0, 2498.1), at the foot of the bank |
| `llv_14` | 2897.5 | (28980.2, 58903.3, 2897.5), 1.05 s | (28086.8, 58507.9, 3066.6), walking |
| `llv_25` | 2897.5 | (32261.8, 59390.7, 2897.5), 1.22 s | (33160.1, 59263.3, 3103.6), walking |

(The candidate ends swimming on any ground it is pushed onto, walkable or not, and then
stands at the foot of the 49° and 58° banks.)

**`sw_banks` results: the Lake Las Vegas saw-tooth was a capture artefact** (2026-10-08)
[observed]. The first run travelled between the banks by long `SetPos` moves only: the
owner saw the land drawn as distant LOD around the player (the full cells not loaded),
three trials froze (positions and clock still) and the game crashed. `sw_scenarios` had
reached Lake Las Vegas the same way (a `SetPos` from Goodsprings), so its swimmer met a bank
without terrain collision; what lifted it onto the bare LAND surface every ~30 units is
whatever the game does for an actor below an unloaded landscape (not studied). The job
now loads each bank's own cell with the console's `cow WastelandNV <x> <y>` before short
`SetPos` moves (`bank_trial`). In the second run the game lost the focus to another
program's window after four trials and hung (the owner: other sessions' engine runs take
the focus; Fallout stops running in the background); those four trials, compared with the
candidate at equal times after the motion onset (the game ran at about 50 loops per
second, so sample counts are not times):

| Trial | Feet − LAND, lowest | Game leaves the water | Candidate − game (x, y, z) |
|---|---|---|---|
| `llv_line` (27°, running) | +4.15 | at the bank, 1.5 s | within 5 except t2.0–2.5 (−20, +1, +11: meeting the bank); t4.0 (+5, −7, +2) |
| `llv_line walk` | +4.39 | 6.1 s | swimming ±21 along the line (the walking stroke varies 82–122, the candidate holds 86 [hypothesis]); walking within 8 |
| `bb_line` (28–34°, running from the float) | +4.05 | at once | walking up: the candidate up to 17 ahead and 12 higher by t4.0 (land walking up slope, not swimming) |
| `lm_12` (12.5°) | +3.95 | 1.2 s | within 3 to t2.0 (frozen from 2.3 s, the focus loss) |

So, with its cell loaded, the game's swimmer meets a 12–34° bank with its capsule (never
below the LAND's contact shell) and walks out, as the candidate does; the "different
shape while swimming" [hypothesis] is withdrawn.

The other banks (`sw_banks_b`, `sw_banks_c`, same job per trial; in `sw_banks_b` the game
clock stopped while five trials waited in the water although MenuMode read 0 after the
travel, so `sw_banks_c` closes menus again just before the keys and reran those five
[observed: MenuMode 0 there, every trial ran]; why the clock stopped is not known, a
level-up menu after discovering map markers is [hypothesis]):

| Bank | Game | Candidate − game while swimming / after |
|---|---|---|
| `lm_16` (16.4°) | walks out at 1.2 s | within 2 / within 9 to t3.5 |
| `lm_25` (25.3°) | walks out | within 2 / within 10 |
| `lm_30` (30.4°) | walks out | within 2 / within 10 to t3.0 |
| `lm_41` (41.5°) | walks out up the 41.5° bank | within 2 / the candidate 16 ahead and 16 higher from t2.0 (faster up the steep slope) |
| `lm_45` (45.1°) | walks up the bank's 105 units of 45.4° (z +107 in 0.75 s) to the cliff above it, then edges along its foot | within 2 / the candidate stops 35–46 lower (its 45° limit) |
| `lm_49` (49.3°) | stands at the bank's foot (4.4 above the floating height) and drifts sideways along it, 75 units in 3 s | within 2 / z within 4; the candidate drifts sideways faster (+62 at t3.5) |
| `lm_58` (58.3°) | stands at the foot, creeps 1 unit | within 2 / within 10 |
| `llv_14` (14.3°), `llv_25` (24.7°) | walk out | within 2 / within 14 |

So the swimming part and the moment of leaving the water agree at every bank, and the game
leaves the water exactly where the candidate does: moving onto ground ends swimming
[observed]. **The steepest walkable slope lies between 45.1° and 49.3°** [observed: walked
up 45.1–45.4°, stood below 49.3° and 58.3°]; the candidate's 45° (§6, a [hypothesis]) is
inside that range only just, and the 41.5° and 45.1° banks also show the candidate climbing
steep slopes faster than the game. Both are land-controller questions (§6.3, the slope
limit and the speed along steep slopes) and are left to that work; nothing here changes
the swim rules.

**Walking in again** (2026-10-18). The owner reports that in the game the player swims
once it wades in deep enough, and that NVForever's player walks along the bed instead
[observed: the owner's play of the game and of NVForever]. That contradicts `sw_trial`'s
walk-in, which had god mode on, so job `sw_walkin` (`make_swim_jobs.py` `walkin()`) walked
and ran in from the same shore with god mode **off** and Health raised by 5000, then ran
in once more with god mode on [observed, 2026-10-18, second run; the first run and that
run's first trial had MenuMode 1 and the clock standing still, so they recorded nothing;
`CloseAllMenus` now runs before each trial]:

- Walking in, god mode off: the feet followed the bed down to 308.2 below S (17.6 s,
  85.5 units/s) and stayed there. Health held at 5200 until about 10 s after the head went
  under, then fell at 40 per second (5079.4 after the trial): drowning, as in `sw_breath`.
  `IsSwimming` read 0 on every sample.
- Running in with god mode on: the bed to 524 below S, `IsSwimming` 0, as in `sw_trial`.

So in these scripted captures the player never swims when walking in, god mode or not.
The captures and the owner's play disagree. The capture setup is the likelier odd one
out [hypothesis]: `IsSwimming` reads 0 in every capture, even while the player plainly
floats after a fall, so the scripted player may never reach the game's real swimming state
(what differs, such as the keys held through `HoldKey`, is open). **Decision
(2026-10-18, following the owner's report):** a character on the ground starts swimming
with its feet `CharacterParams::wade_depth` (115) or more below the surface, at its depth
(no buoyancy lifts it). A swimmer that reaches the ground ends swimming only where the
ground is shallower than that, and swims on against a deeper bed. The depth is
[hypothesis]. It is deeper than every floating height seen (102.5–107.4), so the swimmers
of `sw_banks*` and `swim_lake_exit`, which meet their banks at the floating height, still
walk out and do not start swimming again; with it the eye (about 123 above the feet) is
just above the surface. A human check in the game would settle the depth: walk into Lake
Mead at Boulder Beach (east from (56992, 37536)) and note where swimming starts. The bed
falls about 12°, 44 below S at 256 units out, 140 at 768.

**Actors standing on the bed** (2026-11-21, #2980). Actors give the controller no
vertical input, so the rule above leaves an actor whose character is live on a bed deeper
than `wade_depth` swimming from that character's first step, held at the bed with no
buoyancy, and it played its swimming idle (`mirelurkking\swimidle.kf` for a lakelurk)
whenever it stood still there. Only an actor on its way keeps a character: `residents.rs`
takes it from an actor that stands idle, and from a fighter that holds its ground in
range (`fnv_ai` alert, `Search::Holding`, activity There) unless an attack carries it;
such an actor plays its standing set whatever the water. A swimmer stands still with a
character only for moments (waiting at the end of its way for a target that moves). Whether this swimming idle is the T-pose reported for idle lakelurks
in #2980 is therefore not shown [hypothesis]; the pose watch (test aid
`NVF_PLAY_TEST_POSE_WATCH`) and the base layer warning (`actor base layer fault`) report
what a lakelurk plays. The physics is unchanged (the player's and every swimmer's depths
stay as above). The animation reads the swimming state otherwise: an actor whose
character swims with its feet within 2 units of the support under them
(`Character::ground_gap`, 0 against the bed, at least 9 for a swimmer floating at
`float_depth` over a bed deep enough to swim on), standing still (slower than
`STAND_SPEED`, no walk or run playing) and with its stop faded into the swimming idle,
plays its standing clips; setting off, it walks on its standing walk until that has faded
in and then changes to the swimming walk, so no clip still showing is cut off (and while
such a change of set fades a creature's walk waits for the fade's end; other changes of
set start it at once as before); moving, it swims
on its swimming clips as before
(`engine/nvforever/src/swimming.rs`, `rests_now`) [design]. "Swimming is when your feet do
not touch solid ground" [documented: GECK wiki fSwimRadiationDamageMult]; how the original
poses such an actor is observer check 269 [hypothesis]. Smoke `lakelurk-idle-pose` (idle,
no character) and `lakelurk-rests-on-bed` (following the player in, with one; it depends
on a lakelurk standing still while it keeps its character, so its checks are marked
known failing until a run shows one).

**Placed water** (2026-10-08) [observed: the masters and `Fallout - Meshes.bsa`, read-only].
The reference in `NVDLC03SLH2OINT` is REFR 0x03014467, base PWAT 0x03014465
(`NVDLC03CleanWater2048x2048`: OBND ±1024 × ±1024 × 0, MODL
`Water_Placeable\WaterP2048x2048.NIF`, DNAM 8 bytes whose second u32 is the WATR
0x0301454D `NVDLC03CleanWater`), scale 2, no rotation, at (−584, −656, −328). The model
holds a 2048 × 2048 quad at z 0 (the visible surface) and a `bhkSimpleShapePhantom` on
Havok layer 11 (Water) whose shape is a box of half extents (146.3, 146.3, 73.2) Havok units
moved down by 73.2: in game units (× 7) the column under the quad, 2048 × 2048 and 1024
deep, its top at the quad. All eight placeable water models (the 11 PWAT bases of the ten
masters use them) hold the same: a Water-layer phantom box under the quad, top at the quad
(1024 deep, the bathtub's 13; `WaterPCircle512` has another shape type under its phantom).
No other mesh of the game's archives has a Water-layer phantom (the 40 phantoms of all
archives: triggers, traps, the water models). 86 PWAT references stand in interiors, some
with an XCLW, some without.

Model [hypothesis: the phantom box is the water, its top the surface]: a reference whose
model holds Water-layer phantoms is water inside each phantom's box, transformed by the
reference (`fnv_phys::PlacedWater`; `fnv_collide::nif::collision_and_phantoms` reads the
phantoms, which never collide); the surface is the box's top. A reference's water goes with the reference (`PhysWorld::remove_ref`) but not with a re-add of its bodies (`add_ref`), so water and solid collision can be added in either order; no model of the game has both [observed 2026-10-09: the eight Water-layer phantom models have no rigid body; the other 32 phantom meshes are on layers 12 and 24]. For `NVDLC03SLH2OINT`: surface
−328, footprint x −2632…1464, y −2704…1392, bottom −2376. Checked against `swim_h2o_drops`
[observed]: deep_a now stops at −433.2 (game −432.36); with the swimming state carried
from one drop to the next as the capture did (a walk script whose segments start with
`place`), deep_b, mid_a and mid_b sink from −227 at 12 units/s to −287.5 after 5 s and
−348.0 after 10 s (game −289.0/−349.6, −290.1/−350.7, −289.3/−349.9). So the golden is
consistent with water at −328 *and* with the swimming state surviving 101 units above
the surface; the lab's drops are fresh probes and cannot carry that state, so those three
rows keep failing (the scenario's design, not the golden; question for the owner below).
shallow_a: the drop point is 10 units from the 30-unit riser of the catwalk stairs
0x030144E3 (steps at −240 and −270); the game stood at −236.04, 4 above the upper step,
the candidate on the step's edge at −241.3 with the capsule (within the scenario's 8) and
−246.3 with the cone-bottomed body of §6.4.1 (outside it); not a water question (§6.4.1).
**Refuted: "swimming ends when the feet are above the surface"** (the first
implementation's [hypothesis]): the h2o player swam 101 units above the placed surface,
and the Lake Las Vegas one 74 above the lake [observed]. Swimming now ends only on ground,
on a jump, or where there is no water at all [the last: hypothesis].

**`swim_h2o` retired and redesigned (owner decision 2026-10-09).** `L4.phys.drops.swim_h2o`
is retired (input removed, golden kept unused): its capture placed the player while it still
swam from the previous drop, which the candidate's fresh probes cannot reproduce. Its
replacement `L4.phys.drops.swim_h2o_fresh` (job `sw_h2o_fresh`, a fresh `coc` before every
drop, as `sw_guardian`) keeps the five points at −227 and adds two over floors past the
swimming depth (122.5 and 130.4 below the placed surface) and two short of it (75.3 and 78.9
below). **Predictions of the candidate** (written 2026-10-09 before the capture), feet z:
deep_a −433.2 and deep_b −433.2 (swimming; the fall stops past S − 100 at about S − 105);
mid_a −366.6 and mid_b −366.6 (standing on floors 40 below the surface); shallow_a −246.3 (the
stair edge, §6.4.1); past_depth_a −433.2 and past_depth_b −433.2 (swimming, 17 and 25 above
their floors); short_depth_a −402.2 and short_depth_b −405.7 (standing in water, on floors
75–79 below the surface).

**`sw_h2o_fresh` results** (2026-10-09; golden `phys/swim_h2o_fresh.drops.json`; MenuMode 0
before every drop) [observed]: deep_a −432.36, deep_b −432.87, past_depth_a −432.87,
past_depth_b −435.40 (swimming; within 2.2 of the candidate), mid_a and mid_b −366.45 (on
their floors, 40 below the surface; within 0.2), short_depth_a −402.29 and short_depth_b
−405.81 (on their floors 75–79 below the surface, not swimming; within 0.1): lab 8/9. So the
game's swimming depth lies between 79 and 122 below the surface (the candidate's 100 is
inside), and a fresh drop into placed water stops 104.4–107.4 below its surface.
shallow_a, beside the stair riser: −238.07 in this fresh drop (−236.04 when the old
capture's swimmer sank onto it at 12 units/s); the candidate −246.3 with the cone-bottomed
body, 8.2 off (tolerance 8). A step-edge question with a fall-speed dependence (§6.4.1),
not water.

**Predictions for `sw_guardian`** (held-out scenario `L4.phys.drops.swim_guardian`,
input `parity-lab/inputs/layer4/swim_guardian_drops.drops.json`; written 2026-10-08 before
the run). `CampGuardianCaves` holds `NVCleanWater2048x2048` (0x0013F31A) at scale 3 and
z 6528 (footprint y −8704…−2560) while its XCLW is 6712. Drops 5 above the placed surface,
each after a fresh `coc` (not swimming): inside the box `in_deep` (−4900, −8450), `in_edge`
(−4800, −8650), `in_north` (−5250, −7800) → swimming, stopped at 6425.5 (the fall stops
one step past S − 100); just outside its south edge `out_near` (−4800, −8760), `out_mid`
(−4800, −8800), `out_far` (−4850, −8850) → no water, standing on the floor at 6206.0,
6224.9, 6256.1. If the cell's XCLW were the water, every drop would stop at once (the
drop height is already 179 below 6712); if the water reached past the box, the `out_*`
drops would swim at about 6425.

**`sw_guardian` results** (2026-10-08; golden `phys/swim_guardian_drops.drops.json`)
[observed]: in_deep 6420.82, in_edge 6425.52, in_north 6421.33 (swimming: stopped 102.5–107.2
below the placed surface), out_near 6206.29, out_mid 6221.17, out_far 6260.48 (on the floor):
every z within 4.7 of the prediction. The water is the placed reference's box, not the
cell's XCLW (6712), and it ends at the box's edge (54 units inside: swimming; 56 outside:
on the floor). The lab gave 5/6 with the capsule (`out_far` landed on a sloping floor and
slid 10.4 units in y where the game stayed: a landing question, not water) and gives 6/6
with the cone-bottomed body of §6.4.1.

### 6.7 Interior starts and falls: safe spots (`engine/nvf-play`, `player/safe.rs`)

Problem [observed 2026-10-08, `nvforever --play --cell GSGasStation`]: Ringo's cash box
0x001629C0 (CaravanCashRingo, at 681.9, −4815.7, 7552.3) stands outside the gas station's
room (the game reaches it through barter). A start 60 units beside it (`--pos
681.9,-4755,7552.3 --eye-height 112`) found nothing below and fell for ever (feet 7552 →
−9152 in 8 s); a start on it stood on the box's top (its collision, layer 10, z 7587.7)
and walking off fell for ever. Interiors have no terrain, so the exterior rule (1,000
units under the terrain) never caught it.

What the data offers: **XTEL** gives each door's destination (door, position, rotation)
[documented: xEdit; simulation.md §5]; a door teleports its activator to that position
in the destination door's cell [hypothesis for the default activation, simulation.md §5;
observed for Doc Mitchell's front door]. So the XTEL positions of the doors whose
destination door is in a cell are the places the game itself puts the player in that
cell. GSGasStation has one: door 0x00109040 (outside, in WastelandNV; locked, level 255)
→ door 0x0010903F inside, arrival feet (511, −5951, 7554.2), heading 0° [observed]. Other
markers (XMRK map markers, a "COCMarkerHeading" base) are not used: their meaning is not
in these notes, and what the console's `coc` does in an interior is not documented here.

Rules (`nvf_play::player::safe`) [hypothesis unless tagged otherwise]:
- **Candidates**: the door arrivals into the cell (XTEL position and z rotation of every
  door whose destination door is in it, one-way doors included), nearest to the
  requested position first [engineering choice].
- **Standing test** [engineering choice]: the arrival rule's ray (half a capsule above to
  four capsules below, §6.2) hits a support whose normal is within `max_slope_deg` and
  that is not the reference the spot is for; the capsule (lifted by two skins) overlaps
  nothing there. The first candidate that passes is used and the arrival rule settles
  the feet; the player faces the XTEL heading.
- **Start** (`place_initial`): indoors, when the downward ray from the start eye finds
  nothing, the start moves to that spot, with a warning naming both positions (the far
  above ray stays for exterior starts under the terrain; indoors it would land on a roof).
- **Door destination** with nothing under it (checked once after each `Travel` into an
  interior): moved the same way.
- **Fall** (`interior_net`): feet more than **800 units** below the lowest bound of the
  cell's static collision (`PhysWorld::fixed_bounds`) [engineering choice, like the
  exterior's 1,000] → back to where the character last stood (if it can still stand
  there), else to a door arrival; with neither it is held at that depth (warned once).
  "Last stood" is cleared by `Travel`.

Results [observed 2026-10-08]: the start beside the box lands on the floor at the door
arrival; walking off the box is caught 800 units under the cell's lowest collision
(7532) and put at the door arrival (its last footing, the box's edge, no longer passes the
standing test), from where it walks into the room.

**Captured 2026-10-09** (`pc_coc`, unattended, save nvftest; `make_closeout_jobs.py`)
[observed]:

| `coc` into | Player (GetPos, GetAngle Z) | What is there |
|---|---|---|
| GSDocMitchellHouse | (2351.98, 1603.60, 7360.59), 270° | its COCMarkerHeading 0x00103E68 (2351.98, 1603.60, 7360), z rotation 4.712 rad |
| GSGasStation | (512.30, −5970.55, 7554.08), 0° | its one XMarker 0x00105D4B (512.3, −5970.55, 7553.5); not the door arrival (511.4, −5950.8) |
| GSProspectorSaloonInterior | (−3.94, 243.61, 3460.58), 0° | its one XMarker 0x0015F9C3, exactly |
| GSGeneralStore | (−13.77, 598.76, 544.61), 0° | no marker; the XTEL position of door 0x00106373 into it (−13.77, 598.76, 544.53), whose rotation is −90° |

Over nothing in the gas station (`SetPos` 682, −4755, 7552): the player fell (7422.9
after 0.59 s, free fall) and by 1.12 s stood at (512.30, −5970.55, 7554.09), the `coc`
spot, and stayed there for 20 s. No save was written by `coc` (the run's save guard saw no
change).

So the rules (`nvf_play::player::safe`) are now: **the cell's own spot**
(`safe::cell_spot`): COCMarkerHeading (its heading), else an XMarker, else the first door
arrival into the cell (facing 0) [observed for one cell each; which of several XMarkers or
doors is hypothesis]. An interior start with no `--pos` goes there (`PlayData::
start_at_cell_spot`), as `coc` does; a start, door destination or fall with nothing
under it goes there first, else to the nearest door arrival; falls are caught 256 units
under the cell's lowest collision (`INTERIOR_FELL_THROUGH`, was 800) [engineering choice
inside the observed window of about 100–410 units]. The last place stood is still tried
first after a fall [hypothesis; in the capture it was the same spot]. The XMarker and
door headings of 0 are [observed] for these cells only.
`fnv-game`'s `coc_destination` (Layer 5, `crates/fnv-game/src/runner.rs`) still takes the
first placed reference when a cell has no COCMarkerHeading; the XMarker / door-arrival
rule above would replace it (left to the Layer 5 owner).

### 6.8 Actors in the player's way (`engine/nvf-play`, `blocking.rs`)

The original's collision shape is unseen [hypothesis: every actor has a character
controller of its own on the Char Controller layer, 30; nothing here depends on it].
Large-creature spacing is measured below (#2535). What the engine does is [design]:

- Each frame, every drawn, living actor has a **block** where its body is now: the line
  on the ground under its torso's bones (pelvis to neck, from the posed skeleton that
  also places its hit capsules), widened by 0.17 of the height of its render bounds (22
  units for a person 128 tall, the player's own radius; at least 10, at most 60), from
  the bottom of the bounds to their top. A standing or sitting person is a circle, a
  sleeper or a four-legged creature a strip along its body. Without posed parts the
  line is the middle of the bounds. The block follows the actor through walking,
  furniture clips and beds, because it is read from the skeleton as drawn.
- Before each fixed step the player's wanted horizontal velocity is clipped against the
  blocks whose height overlaps the player's: no closer to a block than touching it
  (the part of the velocity along the block's edge is kept, so the player slides
  round), and a player caught inside one (an actor walked or sat down into them) is
  moved out at 80 units a second and may walk out faster. The character controller
  then moves the player as before, so walls still hold.
- Blocks are not colliders: rays, shots, dynamic clutter and the walking actors'
  characters do not meet them, and they cannot wedge the character. The dead have none
  (their ragdoll's bodies are in the world), and neither have perched birds (creatures
  that cannot walk and have the Flies flag, the ravens outside Doc Mitchell's house;
  `ActorBlocks::in_nobodys_way`, actor-behaviour.md §5.8) [engineering choice, 2026-10-11].
- **Actors kept apart** (2026-10-15, owner: NPCs walked through each other and two
  took the same spot). The navmesh walker meets nobody, and it walks the last stretch
  to every place, every actor away from the player and any the controller gave up on.
  After each AI update, two living actors standing or walking closer than 18 units each
  (36 between them) are moved apart, at most 80 units a second, along the navmesh (not
  off it): half the way each, or all of it for the one that may move when the other is
  walked by the character controller or rooted. Actors on furniture are left out. One
  within 52 units of its path's end that touches another counts as arrived and stands
  beside it. `fnv_ai::apart`; `--actors-pass-through` switches it off [design].
- **Characters are blocks too** (2026-10-10). An actor the character controller walks
  (`PhysState::npcs`) has its block where its character is, and so has the player for
  those actors; these blocks are made anew every fixed step (`nvf_play::player`), and
  the characters pass through each other's bodies (`Character::passes_characters`), so
  this rule alone keeps characters apart. Before, the controllers met each other's
  kinematic bodies, and rapier's controller carries a character along with a kinematic
  body within its contact distance, as on a moving platform: Easy Pete on his noon
  walk took the standing player with him at his own speed, from 3.5 units away, all
  the way to the saloon door [observed: `--play --npc-packages EasyPeteRef --hour 12`
  with the player put on his way, 163 units, rapier 0.36.0].

**Walking creatures' widths (#2535).** [hypothesis: fits the observed spacing; the
original's shape was not seen] A creature with a skeleton `BSBound` extra named
`BBX` blocks other walking actors and the player with a circle of radius at least
its BBX half-width x × Base Scale × reference scale. The existing character radius
wins when larger; NPC_ people and creatures without BBX keep their existing radii.
The radius is used for both the walker and its block, including carried lunges and
steps while the player is on furniture or has collision off [design: `player.rs`,
`blocking::walking_radius`]. It changes neither the world capsule nor navmesh paths:
larger world capsules would not fit the doors and paths actors walk [design:
`phys::npc_params`]. `actor_block_radii` is refreshed beside `actor_heights` from
visible living creature roots; their transforms already include both scales
[design: `actors::spawn_creature`, `hitvol::sync_volumes`].

[design: `residents::think`, `hitvol::sync_volumes`, #2535] A stopped fighter
loses its walking character. Its standing block keeps the scaled BBX circle at
the creature root's feet when wider than the existing drawn block, including
while an attack holds the block in place. This preserves both creatures' widths
when either stops to fight; the posed torso's narrower block cannot replace one
side of the pair. Standing pushes and attack anchoring retain their existing rules.

[observed: the main session's runs of NVForever, 2026-11-15, reproduction on
Searchlight Airport's runway: the Radscorpion + Giant Radscorpion pair against
a god-mode player who backs away from 16 s to 28 s; pair distance read once
a second; three runs of upstream main 99faa23b7 and six of this branch]
Standing and fighting, the pair stays 66–84 units apart on upstream main and
113 on this branch (the designed sum). Chasing, both builds stay mostly
62–64 units apart. On this branch, in four of six runs one reading late in
the chase, as the pair comes back to a stop, fell to 7–39 units for about a
second. Upstream main showed no such dip in its runs.
[design, #2535] The widened block keeps standing creatures apart but not
moving ones, and the change from walking to standing can let one creature
pass briefly into the other. Both are left for the follow-up on the character
controller's movement path.

[observed: FalloutNV.esm and Fallout - Meshes.bsa, fnvtool esm decode / nif decode,
main session 2026-11-15] The radscorpion skeleton's BBX centre is (0, 0, 30.21),
half sizes (70.56, 125.84, 30.21). Other skeleton half sizes: `characters\_male`
(23.10, 17.64, 66.22), centre (0, 0, 66.22); deathclaw (50.00, 95.14, 76.18);
dog (29.56, 62.75, 35.51); brahmin (40, 80, 40). The pair in combat-rules.md §10
has base scales 0.6 and 1.0, giving radii 42.336 and 70.56, sum 112.896 (about
113). Before #2535 NVForever's pair settled only 62–84 units centre to centre,
despite drawn widths 85/141 [observed: main session's measured Searchlight runway
run on 63db0209c, 2026-11-15].

[observed: capture run15b_b2, 2026-11-15; xNVSE 6.4.9, JIP LN 57.30] The same
pair against a standing god-mode player on Searchlight Airport's runway, sampled
at 30 Hz for 17 s: while both were within 300 units of the player (452 samples),
GetDistance between them had median 131, tenth percentile 93, ninetieth 283 and
minimum 32 briefly. This measures spacing, not a rigid non-overlap invariant.

[design: `npc_combat::creature_spacing`, `Fighters::lunge_room`, `attack_anims::carry`]
Combat body gaps and lunge stopping use the walking block's radius too. Creature
reach remains centre to centre, (RNAM + body reach) × scale: for this pair,
(64 + 124) × 0.6 = 112.8 and × 1.0 = 188, beyond their respective blocks plus
the player's radius (64.336/92.56). People count melee reach beyond body radii.
[design: `npc_combat::CombatBodies`, `use_weapon::melee_range`, #2535] Ordinary
combat and scripted melee share the body lookup. When either participant has
a widened block, combat also supplies the other participant's ordinary character
block if absent from the drawn list. Both attack directions therefore have a body
gap and summed radii, including a walking humanoid against a walking or standing
creature. Widened eligibility uses the published BBX width against the character's
capsule radius (or `phys::npc_params` from its height once the character is removed),
so stopping does not remove the ordinary walker's body gap.
Pairs without a widened participant keep the existing drawn-block lookup.
[design: `nvf_play::combat::PhysView::melee_targets_near`,
`fnv_combat::projectile::swing_reaching`, #2535] The player's ground-reach fallback
uses max(24, the target's published scaled BBX radius) in place of the fixed
24-unit target margin. The player's own capsule radius remains part of its reach.
Thus a short weapon still reaches a low creature at its widened blocking stop,
looking level or down; ordinary targets keep their existing reach. The same
per-target margin expands candidate lookup, and the obstruction rays still must
meet the target first.
The separate navmesh `fnv_ai::Ai::keep_apart` still uses 18 per actor; it does
not read these blocks. The original's transient overlap, unseen shape, extra
attack pause and backing-away cadence remain open in observer check 244.

**Pushing** (2026-10-10, owner request: "the player or npc moving needs to be able to
push another actor if they are stood still"). What the game does when one character
walks into another is not captured yet; the job is ready (below). Until its numbers are
in, the engine does this [design]:

- **Who gives way.** An actor that stands still: looked after by its packages
  (`nvforever`'s residents), in the world, alive, drawn, not on its way anywhere, not on
  furniture (or about to use some, or getting off it), not waiting at a door and not in
  a conversation it started (`fnv_ai::Ai::stands`). The residents list these actors and
  where each stands every frame (`ActorBlocks::standing`); their blocks are *pushable*,
  and stand where the residents have them (the drawn pose follows a frame or two
  later). The player gives way when it stands still on the ground (no movement key, not
  swimming, not in the air).
- **Who pushes.** Whoever walks under the character controller: the player, and the
  actors it walks near the player (`PhysState::npcs`). These actors now meet the blocks
  of the others (they walked through them before), each other's and the player's.
- **How far.** Of the speed at which the walker comes at the standing actor (the part
  of its velocity towards it, beyond what takes it up to touching), the two keep
  `PUSH_SHARE` = **0.5** and move on together; the part along the actor's edge is kept
  whole, so a walker that is not aimed at its middle pushes it aside and passes
  [design: a placeholder; the capture gives the game's figure].
- **Room.** A pushed actor moves over the navmesh only (`fnv_ai::Ai::push`): to the
  navmesh under the place the push asks for, else to the nearest point of its edge
  within the push's length (so it slides along walls and furniture, which the navmesh
  keeps clear of), never onto navmesh more than 32 units higher or lower, and not at
  all where it stands on no navmesh. One that got less than a quarter of a push is
  left standing for 1.5 s (it blocks like a seated actor). The pushed player moves
  through its character controller, so walls hold it; when it gets less than half of a
  push, the actors treat it as solid for 30 steps (they slide round).
- **Afterwards.** An actor pushed out of the place its package has it at walks back
  once nothing has pushed it for 2 s (`PUSHED_BACK_SECONDS`, actor-behaviour.md §5.7)
  [hypothesis: not captured]. The pushed actor slides: no step animation is played.

**Checked in the engine** [observed 2026-10-10, owner's install, `--play`, Goodsprings;
the exit report's "actors in the player's way" and "actors pushed aside" lines and the
residents' "was pushed aside" line]:

- The player runs at a standing settler (`NVF_PLAY_TEST_FACE=46:00104F0A
  NVF_PLAY_TEST_MOVE=46.3:1:0,49:0:0`, `--hour 12`; he sandboxes, so he has to be
  standing at that moment): pushed 117 units in 47 steps, from (−71274, 3610) to
  (−71268, 3723), then the player slid past him. He stayed there (a sandboxing actor
  does not go back).
- Easy Pete walks into the standing player (the smoke test in CLAUDE.md): the player
  is pushed for 206 steps, from (−67790, 3450) to (−67709, 3564), and Easy Pete goes on
  through the saloon door.
- The player runs at a settler seated on a saloon stool: stopped 0.6 units from
  touching him; he is not pushed.
- 100 s of Goodsprings with every resident (`--hour 11.9`): 25 residents, 14 packages
  chosen, 2 trips, 2 arrivals, 0 unreachable, as before. A block changed a walking
  actor's step in 40 of about 6,000 steps; no walking actor met a standing one. The
  character controller gave up on an actor twice (once in each 50-second run); whether
  it did before this change was not measured.
- Not seen in a game run: a walking actor pushing a standing one, and the walk back
  (both covered by the `nvf-play` and `fnv-ai` tests).

**Capture `pc_actor_push`** (`tools/capture/jobs/make_actor_push_jobs.py`, save `nvfdoc`,
unattended, about 4 minutes, god mode, AI detection left on; the lane of `pc_push` in
Doc Mitchell's house). Run into: Doc Mitchell as the save has him, and a settler made
with `PlaceAtMe`. The player, the actor (position, heading, IsMoving, AI procedure,
GetSitting) and the clock are sampled every loop while the player runs at the actor,
every second or third loop in the longer trials. Trials per actor:

| Trial | What | What it settles |
|---|---|---|
| alone | the run and the walk with nobody in the lane | the speeds to compare with |
| A | run and walk straight at the standing actor, twice each; 10 s of samples after the second | is the actor moved at all, the two speeds while pushing (`PUSH_SHARE`), the distance between the two when they touch (the actor's radius, given the player's 22), does the actor go back |
| B | the same from 14 units to the side | does the player slide round, is the actor pushed sideways |
| C | forward held 4 s running, 6 s walking, the wall 280 units behind the actor | does a push go on, what happens at the wall |
| D | A with the actor restrained (`SetRestrained 1`) | does an actor that may not move give way: a push by physics, or by its AI stepping aside |
| E | the player put on the actor's very spot (`SetPos`), nothing held | how two overlapping characters are taken apart and which of them moves (`PUSH_OUT`) |
| F | the actor left to walk off by itself, then again with the player standing on the way it took | does a walking actor push the standing player, wait, or go round |

Predictions of the engine as it is [design]: A and C, the actor moves ahead of the
player at half the player's speed (172 running, 43 walking) for as long as the key is
held and stops at the wall, where the player stops too; B, the actor goes ahead and to
the far side while the player passes; D, the same as A (the engine knows no restraint);
E, the player is moved out at 80 units a second and the actor stays; F, the player is
pushed ahead of the actor at half its walking speed. Every one of these is a guess to
be replaced.

Not done: an actor that walks on the navmesh surface (no character: the last 64 units
of a walk, an actor the character controller gave up on, everyone without `--play`) is
stopped by nobody and pushes nobody; two walkers do not push each other (each stops at
the other and slides round it; head on, the actors rely on the stuck rule); actors that are not residents
(`--no-npc-packages`, the test aids) are never pushed; size and weight play no part;
and a wide, low creature (a radscorpion) is narrower than it looks.

### 6.9 Steps with shallow tops (playtest list 5 item 17, 2026-10-16)

The owner ran up the door steps of the camper at the Powder Ganger camp south of
Goodsprings (`Camper01`, 0x0010771A) in the game; here they needed a jump. Measured with
`fnvtool phys cell --ray` [observed]: from the ground (6150) a 10-unit step with a top 24
deep, then a 20-unit step onto the camper's floor (6182-6190), headroom about 150 inside.

- **Cause** [observed: a debug trace of `move_character`]: rapier's autostep ends with a
  check that casts the shape down onto the step and refuses a contact normal steeper than
  the slope limit. The character's cone bottom (§6.4.1) meets a step's edge with its own
  face, 57.7° from level, until the flat tip (radius 3.2) is over the top: at the first
  contact the axis stands 3.2 + rise / 1.58 short of the edge (16 for a 20-unit step), so
  with any `min_width` short of that plus the tip the check failed, and a longer one ran
  into the next riser. Raising `step_height` changed nothing.
- **Fix** (`fnv_phys::character::step_up_move`) [engineering choice]: when a step's
  horizontal motion on the ground is blocked by a wall-like contact to under 90 % of what
  was asked, the controller lifts the shape by the headroom above it (at most
  `step_height`), moves it horizontally (sliding along walls), and puts it down on what is
  below; it keeps this if the feet came down higher and further along than without it and
  a walkable top lies within 4 below to 26 above the feet straight under the axis or 19
  units ahead of it (the cone resting on the edge). A slope steeper than the limit is not
  climbed so (its surface is not walkable), nor a wall higher than `step_height`.
  rapier's autostep still runs first and is unchanged. `step_height` stays 24
  [hypothesis]; `step_width` (the autostep's `min_width`, 22) became a parameter for
  `phys walk` scripts.
- **Checked**: `phys walk` north from (−53760…−53710, −24200) at run speed ends inside
  the camper on its floor (before: stuck at (−53707, −24101) on the ground); the test
  `character_climbs_shallow_steps_without_jumping`; the lab's `L4.phys.walk.*` 9/10 as on
  main (`portal_wall` 5/6, the accepted residual); smoke scenario `camper-steps`.

### 6.9.1 Catwalk stairs at REPCONN (owner's playtest, 2026-10-23)

The owner reported that the steep steps up to Chris Haversam's and Jason Bright's
intercom in the REPCONN Facility (`RocketLabMid`, intercom 0x000CD684 on the catwalk at
z 506) are hard to walk up and usually need a jump.

- **The stairs** [observed: `fnvtool phys cell --ray`, vertical rays every 1-2 units]:
  catwalk flights of `UtlCatStairsEndGround01` (0x00082FDB), `UtlCatStairsTurnHalfL01`
  (0x00082FEE) and `UtlCatStairsTurnFullL01` (0x00082FE5), every riser 24.0 and every tread
  24 deep (a 45-degree climb), from the floor (−128) by way of catwalks at −6, 66, 194, 250,
  320 and 450 to the intercom catwalk.
- **Cause** [observed: a debug trace of `move_character`]: on each step's edge the cone
  bottom comes to rest with its flat tip just short of the top; rapier's controller refuses
  the cone's steep face as a wall, and `step_up_move` (§6.9), which measured its rise from
  the feet, found rises of 0.48, 0.33 and 0.04 units there, and on deeper perches slightly
  below zero, under its 0.5 minimum, so the player stood on the edge until a jump. Which
  edge it stuck on depended on the speed and on where the walk started. Raising
  `step_height` to 26 or 30 changed nothing.
- **Fix** (`MIN_STEP_RISE`) [engineering choice]: the step-up's shape is put down onto the
  bare surface while the feet rest the support's gap above theirs, so the rise is now
  measured from where flat ground would put the shape (the feet less the gap): flat ground
  and descents measure 0 or lower and stay refused, a perched cone measures about the gap
  (0.7) or more, and a step up counts from 0.25. The settle then lifts the feet to the gap
  on the step's top. The step height stays 24.
- **Checked** [observed]: `fnvtool phys walk` straight up each of the seven flights at 115,
  150 and 340 units/s from 8 start points 3 units apart: 168/168 reach the top (115/168
  before), none leaves the ground; the test `character_climbs_steep_stairs_without_jumping`
  (24-unit stairs from 8 starts at walk and run; fails before the fix); smoke scenario
  `repconn-catwalk-stairs`. `fnvtool nav walk` from the floor to the intercom still sticks
  at (847, 4060, −127) with and without the fix: its path cuts under the stairs and never
  reaches them (NPC path following, not this controller).

### 6.9.2 Caravan thresholds: lips with no top and ramp crests (issue #2494, 2026-11-14)

Players reported that walking out of the wrecked caravans (STAT `MobileHomeASNV`
0x000928F3, placed 46 times in WastelandNV) stops at the door: the player stands on the
threshold or sits in its gap until a jump.

- **The threshold** [observed: `fnvtool phys cell --ray` and the mesh's collision, issue
  #2494]: the floor inside ends at the door, where the inner skin rises past it as a
  zero-thickness lip 10.3 above the floor (no top face); between the inner and the outer
  skin the space is open down to the terrain, about 10-22 wide and about 50 deep. At
  0x001070C1 (cell −17, −1) the floor ends at y −2385.7 and the terrain outside lies 48
  below it. Where a plank ramp stands at the door (0x00137E77, cell −8, −25, ramp
  0x00177D7C) the ramp's upper end hangs over the gap about 17.5 above the floor and slopes
  down outwards at about 24°.
- **The original** [observed: capture of the original, 2026-11-14; the player placed on
  the floor, facing out, Forward held, positions every 10 loops]: walks out of both
  caravans without a jump at walk and at run pace. At 0x001070C1 the feet pass y −2384 at
  z 8159 and peak at 8166.5 running (+14.2 over the floor at 8152.3) and 8167.6 walking
  (+15.3), then drop to the terrain (8107.8) and walk on. At 0x00137E77 the feet peak at
  6856.4 running (+8.7 over the floor at 6847.7) and 6863.7 walking (+16.0), then go down
  the ramp to the terrain (6804). The feet rise about 9-16 over the floor while crossing.
- **Cause** [observed: `fnvtool phys walk` and a debug trace of `move_character`]:
  rapier's autostep refuses the lip (the cone meets its edge with its own 57.7° face,
  §6.9), and the step-up (§6.9) climbs only onto a walkable top within 4 below to 26
  above the feet under the axis or 19 ahead. Perched on the lip's edge the floor under the
  axis lies more than 4 below the feet and 19 ahead is the gap; perched in the gap the
  ramp 19 ahead lies more than 4 lower. So the cone rested on the lip's edge (0 of 30
  walks out at 0x001070C1, 2 of 30 at 0x00137E77). Walking along a door jamb into the
  corner it makes with the lip, the wedge rule (§6.1) also stopped the step's motion
  before the step-up was tried.
- **Fix** (`fnv_phys::character::on_crest`, `step_up_move`) [design]: the shape the
  step-up puts down may also rest on a **crest**: the contact it came down on holds it up
  (its normal points up), lies at most `step_height` above a base height
  (`crest_base`), and 2 units beyond it along the motion the surface falls away by more
  than 2 (steeper than 45°) or is walkable and no higher than the edge. A lip with no top,
  the top edge of a thin board and a ramp's upper end count; a steep slope does not (it
  goes on rising beyond the contact, and across it sideways the surface stays about as
  high and is not walkable), nor a thin wall higher than `step_height`.
  - **The base**: during a run of step-ups (`Character::perch_base`) the height the run
    started from (or the feet less the gap, when lower), so feet that earlier steps of the
    run raised onto an edge do not raise the limit. The run starts with the first step-up
    taken and ends when a step starts on walkable ground, in the air or with a jump, or
    with a placement. Outside a run: the walkable surface straight under the feet within
    `step_height` (the floor under a cone placed, dropped or walked onto an edge), or,
    with none so near, the feet less the gap (a cone that jumped or fell onto an edge high
    above the ground steps on from where it rests).
  - Measured from the step's own feet (the first version of this fix), a slow approach to
    a 25-unit thin wall first perched 2-4 above the ground through the walkable top 19
    ahead (the ground beyond the wall) and then climbed it; measured from the last walkable
    support (the second version) the same climb remained after a placement or a drop onto
    a low perch beside a 26-unit wall, and a jump onto an edge far above the ground
    counted from the ground it left (reviews of the fix; tests
    `thin_walls_are_stepped_over_up_to_the_step_height`, walls of 24, 25, 26 and 28 at 30,
    115.5 and 344 units/s, `a_thin_wall_too_high_blocks_after_a_placement_or_a_drop_onto_its_edge`
    and `a_jump_onto_a_high_edge_steps_on_over_it`).
- **Edges a little lower, wedges** [design]: an edge met by the cone's face (off the
  flat tip by more than 0.5) that is a crest counts even when the shape comes down up to 2
  lower than it was held: crossing the gap onto a ramp's end it sinks a little. A step the
  wedge rule stopped is tried by the step-up too. The cone stays (§6.4.1's fence-edge fit),
  as do the step height (24) and the other rules of §6.9. All of this applies to actors
  and creatures as well: they walk with the same controller (`plan_move`) and
  `npc_params` keeps the step height of 24 (§6.8, §6.8.1).
- **Actors' slope limit (#2511)** [design]: `fnv_phys::character::ACTOR_MAX_SLOPE_DEG`
  is 56°, shared by `nvf_play::phys::npc_params` and the synthetic tests. The cone's
  `bottom_slope = 1.58` gives a face about 57.7° from level (§6.4.1/§6.9); the roughly
  1.7° margin prevents the controller treating an edge against that face as a walkable
  slope, while admitting the 46–49° paths cited in design/navmesh.md §5. The old 60°
  limit was an engine choice, not the maintainer's or an original-game measurement.
  [observed: headless runs reported in issue #2511] At 60°, a person-sized actor crossed
  24–28-unit thin walls, and a radius-10 creature crossed 24–26 but stopped at 28,
  before and without the step-up. [design: synthetic.rs]
  `thin_walls_are_stepped_over_up_to_the_step_height` now requires the player, person
  and small creatures to cross 24 and block 25/26/28 at 30/115.5/344 units/s,
  on both terrain and a static floor;
  `actors_walk_navmesh_grade_slopes_below_the_cone_face_angle` covers 46/47/48/49°
  ramps at walk/run speeds. Original actors blocking these walls remains [hypothesis];
  observer check 166 asks for that measurement.
- **Step-up clearance (#2511)** [design: `step_up_move`]: when the bare downward cast
  misses or only finds level ground without a rise, retry with the horizontal sweep's
  wall gap (at least the numerical skin). This fallback must meet a crest off the flat
  tip, and its rise check subtracts that clearance so flat ground alone is not a step. A slow small
  creature can approach an edge while still outside its bare shape; a zero-clearance
  cast refused its 24-unit wall after lowering the slope limit [observed: synthetic
  controller run for #2511, radius 10, height 30, speed 30, stopped at x = 87.324
  before the wall at x = 100]. The crest's height limit remains `step_height` above
  `crest_base`; the clearance does not raise that limit.
- **Checked** [observed: `fnvtool phys walk`, `CharacterParams::default()`, 5 offsets
  across the door (−15 to +15) × 3 headings (−10°, 0, +10°), 6 s at walk (115.5) and run
  (344) from the floor]:

  | Run | Before | After | Peak over the floor (after) |
  |---|---|---|---|
  | 0x001070C1, walk | 0/15 out | 15/15 | 11.3-13.7 (straight 11.4) |
  | 0x001070C1, run | 0/15 out | 15/15 | 11.3-14.8 (straight 11.4) |
  | 0x001070C1, from the gap (3 offsets), walk and run | 0/6 out | 6/6 | |
  | 0x00137E77 with the ramp, walk | 0/15 out | 15/15 | 15.4-17.7 (straight 17.5) |
  | 0x00137E77 with the ramp, run | 2/15 out | 15/15 | 15.3-17.7 (straight 17.5) |

  The peaks lie within the original's 9-16 at 0x001070C1 and up to 1.7 above it at the
  ramp. At the time of #2494, with `npc_params` for a person (slope limit 60°), all 66
  walks got out before and after, the same peaks; for a creature drawn 30 tall
  (radius 10, `step_width` 10) 56 of
  66 before and 65 after (the one left ran along the inside wall past the door at 10°). Unchanged: the parity lab's ten `L4.phys.walk.*` scripts and the `diag_fence_edge`
  / `diag_fence_low` drops give byte-identical traces before and after; the camper's door
  steps (§6.9) from x −53760 end at the same place, and from 5 starts further east the
  player now steps over the corner of the floor mattress `BedMattress01FloorL` 0x0010771B
  (13.6 high) inside the camper instead of stopping at it. Tests
  `character_walks_out_over_a_threshold_lip` (fails before the fix; the player's, an
  actor's and two small creatures' parameters) and
  `thin_walls_are_stepped_over_up_to_the_step_height`,
  `a_thin_wall_too_high_blocks_after_a_placement_or_a_drop_onto_its_edge` (fails on the
  second version) and `a_jump_onto_a_high_edge_steps_on_over_it` (passes on the second
  version too: in the landings built for it rapier's controller or the walkable floor
  beyond the lip carried the character over, so it guards the rule without having failed);
  smoke scenario `caravan-door-walk-out`.

### 6.8.1 Dead actors' characters, small actors' characters (playtest list 7 items 14 and 35, 2026-10-23)

- **Item 14** [observed: owner, "bodies glide along the floor ... like they are still
  following their last pathfinding movement", and an invisible collision pushed the
  player after killing a charging creature]. **Cause** [observed in the code]: the
  residents keep a character for an actor while its activity is Travelling; the AI stops
  stepping a dead actor (`fnv_ai` `step_actor_inner` returns for the dead, restrained and
  unconscious), so its activity stayed Travelling, its character stayed in
  `PhysState::npcs` and was moved at every fixed step by the velocity of the last walk;
  the body is drawn at its character (`npc_drawn_feet`) and the character is a block in
  the player's way (§6.8: the characters' blocks were made for every character, dead or
  not). **Now** the residents take away the character of an actor that is dead,
  restrained or out cold in the frame after it happens (log line `ai [..] <name> <id>:
  its character stops (dead, restrained or out cold) at (..), walking at N units/s`); the
  body stays where it is drawn and the ragdoll (ragdoll.md) takes over there. Smoke
  `death-stops-character`.
- **Item 35** [observed: owner; the nymphs `NVCrGiantMantisNymph` 0x000EF411 in
  `SLNHPStationINT`, Base Scale 0.3, OBND 132 × 102 × 99 unscaled, script
  `CrGiantMantisNymphScript` 0x000EF407: in an interior, `GetDistance player < 50` →
  `KillActor player`; `fnvtool esm decode`, `script decompile`]. **Cause** [observed: the
  code, and a run with the player run north from (2380, 1840) at the nymph 0x00153386:
  it ran at the player, died by its script at once, and the player then stopped at y
  2017, 19 units short of the wall at 2058; that the dead nymph's character stopped it
  is [hypothesis]]: every character the residents made was the
  player's capsule (radius 22, 128.9 tall), so a nymph walked near the player stood in
  the way as a person, and once it died that person-sized character stayed where it
  was (item 14). **Now**: the dead have no character (above), and an actor drawn under
  0.75 of the player's height gets a character of its own size (`phys::npc_params`): as
  tall as its drawn bounds (at least 20) and as wide as its block (0.17 of the height,
  at least 10, at most the player's 22) [design]; larger actors keep the player's capsule
  (a wider one would not fit the doors and paths they walk). The ragdolls of the dead
  never met the player (ragdoll.md). Smoke `mantis-nymph-step-on`. Not done: the player
  does not step *onto* a living small creature (its block stops the player at 32 units
  from a nymph; the nymph's own script kills it at 50, so in that cell this never shows).

### 6.10 Heavy bodies do not give way to characters (playtest list 7 item 37, 2026-10-23)

The cars could be pushed by running into them; the owner reports that in the game they
do not move [observed: owner]. They are moveable statics (MSTT `Car01`, `Car02`,
`Car03`, 48 placed in WastelandNV) whose models each have one dynamic body on the Props
layer, Havok mass 600, 1000 and 800 [observed: `fnvtool esm decode`, `fnvtool world
refs`, `fnvtool collide objects --filter vehicles\`], so §4.3.2's rule for statics does
not apply. A character pushed them two ways: rapier's controller impulses
(`solve_character_collision_impulses`, push mass 80) and the solver's contacts between
the character's kinematic body and the dynamic body (a 600-unit box of a car's size was
moved 6.7 units by a run into it with the impulses off) [observed: test
`character_does_not_push_heavy_bodies`].

- **Now** [design; what the game's character does to a heavy body is not captured]: a
  dynamic body heavier than `PUSH_MAX_MASS` = **250** gets no impulse from a character
  and no contact pair with the characters' kinematic bodies (collision and solver group
  `CHARACTER_SOLVER` left out of its filters), so a touch neither shoves nor wakes it
  (the Car02 0x0015206B south of Primm still rises 3.4 and slides 2.5 units, 4.1 in
  all, when the player runs at it, the same with the forces off and with the contacts
  off, and stays put with nobody near [observed: smoke `car-not-pushed` and two runs,
  2026-10-23]: it wakes and comes out of the ground it is placed a little into; that
  the raven perched on its roof, 0x0017467E, wakes it is [hypothesis]); queries
  test no groups, so the controller stops at it as at a wall. Shots, explosions, grabs
  and other bodies still move it. Light clutter (the `pc_push` trials, a few units of
  mass) is pushed as before.
- Bodies over 250 among the vehicles: the car bodies, the motorcycle (600; its STAT is
  fixed anyway), `Car01HulkTrunk` (500), `TruckFlatbed`/`Truck0x`/`NV_FireTruck` (static
  already). Hulk doors, hoods and grills (25-175) stay pushable.
- The exit report lists every placed dynamic body physics has moved
  (`physics: placed <id> moved by physics: D units from where it was added, at exit`).
  Smoke `car-not-pushed`.

### 6.11 Drawn height over steps and bumps (playtest list 7 item 9, 2026-10-23)

The owner found the character "way too flickery when going over small bumps in a road":
the body jumped up and down at once with each change of height [observed: owner]. The
controller moves the feet up a step or down off a bump within one fixed step (rapier's
autostep, `step_up_move` §6.9, the snap down of §6.4.2), and the drawn ground under the
feet (`drawn_sink`, floating-actors.md §6-7) changes height from one step to the next
over models with drawn floors; the body, and the view that is the body's eye, were drawn
there at once.

**First rule** (2026-10-23, PR #600): the change of the feet's height beyond the
horizontal distance walked × the slope walked lately (the size of the recent changes,
filtered over 0.25 s, at least 0.35) went into the lift, which sprang back with
ω = 25/s. The owner found walking down a road still rough: the body and camera "dropping/
raising far too often" (2026-11-07) [observed: owner]. Traced, the feet follow every
facet of the road models' collision: the height runs in straight lines with a kink at
each edge, ±0.05 per unit along the Goodsprings road, and drops 5.5 units over two steps
where the road piece 0x00104FB6 meets the ground [observed: `fnvtool phys walk` along y
2312 from x −71900, 2026-11-07]. Running at 300 units/s a step covers 5 units, so up to
1.75 units a step passed as "slope", and the slope measure (sizes, not signs) grew on
the ripples themselves: on the road the first rule smoothed nothing. Over rough ground
the controller also snaps the feet down 4-6 units in a step (§6.4.2) and climbs steep
rock (0x00107B90) in 3-unit steps every other fixed step.

**Now** [design; `engine/nvf-play/src/phys/step_lift.rs`]: every character (the player
and the actors the controller walks) has a drawn lift. In each fixed step on the ground,
the change of the feet's height beyond what the predicted vertical speed gives is taken
into the lift with the opposite sign; the prediction is the vertical speed on the ground
filtered over 0.2 s, each sample kept within 0.4 × the distance walked of the prediction,
so a step does not feed it and is not overshot. A change of the drawn ground is taken in
whole. The lift goes back to 0 as a critically damped spring, ω = 8/s for a small lift
and stiffer the larger it is (ω × (1 + |lift| / 6)), at most 48 units either way (24,
the step height, until 2026-11-17: a 27-unit let-down off a road piece's edge after a
crouched jump, on top of 12 still being eased, went past it and the view dropped the
excess in one frame; `step_lift::MAX_LIFT`); a
change over 48 units in a step (a placement), the air and swimming take nothing in and
restart the prediction from 0. The feet, the capsule, the reference's position and every
query stay where the controller has them; the player's body, its first- and third-person
view and the actors' drawn bodies use the lift (`PhysState::drawn_lift_of`,
`npc_drawn_feet`). A long hill is drawn as walked once the prediction has caught up;
running onto a 30° slope the body is drawn up to 8 units below the feet for a moment.
Replayed on the traces [observed: `fnvtool phys walk` traces through the filter,
2026-11-07], the RMS vertical acceleration of the drawn height (feet / first rule / now,
units/s²): road run 741 / 741 / 284, road walk 239 / 206 / 85, rough desert run
2621 / 1072 / 504, the hill run `move_hill_run` 787 / 660 / 334, Doc Mitchell's porch
steps 2078 / 1690 / 629; largest lift 6.6, 4.2, 16, 7.9 and 9.9 units. The broken
side of the road piece (WastelandRoadStraightLongB 0x00104FB5, lanes 160-240 units off
its centre line) is the roughest: the feet move 2-3.4 units in single steps over its
broken parts; there 2093 / 1367 / 260 and 2477 / 1965 / 388. In game on that lane
[observed: smoke `road-run-smooth`, 2026-11-07] the exit report gave 2081 for the feet
and 427 for the drawn height over 597 steps. The exit report says how many steps the
player's lift took in, its largest value, and the RMS vertical acceleration of the feet
and of the drawn height while walking on the ground (`StepLift::rough`); smoke
`camper-steps`, `road-run-smooth`.

**Stopping and the top of a slope** (2026-11-07, review of PR #1806) [design]: the
prediction is never more than 2 × the horizontal distance walked per step (a 63° grade,
`MAX_GRADE`), so standing still it is 0; without that, stopping after running up a 30°
slope left the prediction at 173 units/s and the body drawn 11.4 units above the feet
for as long as it stood (7.8 after a walk). Remaining overshoot [observed: synthetic
replays in the module's tests, 2026-11-07]: running (5 units a step) up a 30° slope onto
the level, the body is drawn up to 9.9 units over the top, walking (2.5) 6.1; at the top
of a flight of stairs (8-unit risers, 12-unit treads) up to 4.2 over the top step
running and 1.2 walking: the prediction carries on past the kink or the last riser for
about 0.2 s, and the spring brings the body back down. The first rule never overshot.
Tried and left out [observed: smoke `road-run-smooth`, 2026-11-07]: setting the
prediction to the step's sample at once where the ground falls short of it by more than
the step allowance while the body is already drawn past the feet halved the slope's
overshoot (4.8 / 2.7) but doubled the drawn height's RMS on the broken road lane
(426 → 826 units/s², drawn/feet 0.20 → 0.40); smoother damping of the prediction
(shorter filter while the body is ahead of the feet) cost rough ground as much for less.
With the bound alone the road run gave 2086 / 426 (0.20), as before.

**Easing onto curbs** (2026-11-12, player feedback that walking onto obstacles of
varying height "seems a bit jarring") [design; `step_lift.rs`]: the spring started the
lift back towards 0 at full acceleration in the very step a curb was climbed or dropped
off, so the view went from still to about 50 units/s within one fixed step. The lift now
goes back through three first-order stages of rate 22/s one after the other (a triple
critically damped pole; the change beyond the prediction is taken into all three), so
after a curb the drawn height starts with no vertical speed and no vertical acceleration
and is within a tenth of the step after about 0.24 s. The stiffening with the size of the
lift is gone; the prediction, its bounds, the air, swimming and placements are as
before. Replayed [observed: synthetic replays in the module's tests, 2026-11-12;
before / now]: on a 10-unit curb the largest change of the drawn vertical speed from one
step to the next is 0.66 / 0.34 units a step at a walk and 0.73 / 0.42 at a run, and
the first step on it moves the drawn height 0.5 / 0.06 units; over the top of a flight
of stairs the body is drawn 4.2 / 2.6 units over the top step running, 1.2 / 0.5
walking. The cost: running onto and over a 30° slope the body lags and then overshoots
by up to 13.3 units (9.9 before), walking 6.6 (6.1). A rate of 18/s was gentler
still on a curb but made a run up the stairs far rougher (RMS vertical acceleration of
the drawn height 356 → 1838 units/s² in the replay); 22/s also kept the replayed road
ripples a little smoother than before (218 → 205). In game on the broken road lane
[observed: smoke `road-run-smooth`, 2026-11-12] the exit report gave 2083 for the feet and
365 for the drawn height over 596 steps (427 before, drawn/feet 0.21 → 0.18), largest lift
14.3 units; `camper-steps`, `repconn-catwalk-stairs`, `pete-porch` and `pete-terrain`
pass.

**Small drops count as the ground** (2026-11-12, the same feedback, narrowed to walking
off a road onto the terrain and over small rock clusters) [design; `step_lift::footing`]:
running over rocks the controller leaves the ground for 2-4 fixed steps each time the feet
come off one, dropping 16-20 units [observed: `fnvtool phys walk` running south-east over
the desert south of Goodsprings, from (−70500, −2500) at (250, −150) units/s,
2026-11-12]; drawn as a fall, the view dropped with the feet and stopped dead on landing.
The lift now takes in a step in the air too while the character walked off an edge (the
controller's `walked_off_z`, not set by a jump) and its feet are no more than the step
height (24) below where it left the ground; a jump, swimming and any drop deeper than that
are the air as before, so a real fall is not slowed. Replayed on that trace the drawn
height's RMS vertical acceleration while moving fell from 7409 to 549 units/s² and its
largest from 66 218 to 2712 (the feet 8153 / 68 609); walks along and across the road
that never leave the ground are unchanged.

**The view and walking pace** (2026-11-12, the owner's video and save walking west over
Novac's broken road, cell −13,−13, from (−52595, −52475) facing 269°) [design;
`step_lift.rs`, `player_body.rs`]: two things still moved the view there. (1) The body is
drawn on the drawn ground (`drawn_sink`) and the eye counts from the feet, but a change of
the drawn ground went whole into the lift, which the eye includes: a crack in the road
model's drawn floor moved the view at once and eased it back. The lift now keeps the
drawn ground's share apart (`StepLift::ground_at`, eased the same way), and the eye, the
first-person rig and the third-person pivot leave it out. (2) The feet drop off the
broken road pieces' edges in quick 8-10 unit drops [observed: `fnvtool phys walk` on
that line at 75 units/s, 2026-11-12]; eased in 0.24 s at a walk they showed as a dip.
The stages' rate now follows the horizontal speed: 22/s at 250 units/s and faster, in
proportion below, at least 10/s, so a step is eased over about the same distance walked.
In game on that line at a walk [observed: smoke `novac-broken-road-walk`, 2026-11-12]
the RMS vertical acceleration of the view fell from 203 to 61 units/s² (the body 203 →
70, the feet 2534); replayed, a walk along the Goodsprings road 102 → 82, the broken lane
walked 145 → 88. Runs are unchanged (the road run's view 290 for the body's 363). The
cost: walking, the body is drawn up to 22 units off the feet for a moment after those
drops, and 9.1 over the top of a 30° slope (6.6).

### 6.12 Fall damage (playtest list 7 item 21, 2026-10-23; `engine/nvf-play/src/falls.rs`)

- **Settings** [observed: FalloutNV.esm GMST]: `fJumpFallHeightMin` 600,
  `fJumpFallHeightExponent` 1.65, `fJumpFallVelocityMin` 2785. No plugin sets
  `fJumpFallHeightMult` (an exe setting; its value is not read, the exe is encrypted).
- **Measured** [observed: capture `sw_fall_breath`, §6.6]: land falls of 695.6, 1495.4 and
  2995.3 units (start feet to landing feet) cost 46.7, 1858.6 and 9419.6 health; 300 cost
  nothing; no fall into water cost anything.
- **Rule built**: damage = `fJumpFallHeightMult` × (h − `fJumpFallHeightMin`)^
  `fJumpFallHeightExponent` for a landing on land from h units above, none at or below
  the minimum [hypothesis: the formula's shape is the fit k·(h − 600)^1.65 of the three
  falls, within 0.8 %, and the link to the settings is from their names]. The factor
  defaults to the fitted 0.0252 [observed fit] when no plugin sets `fJumpFallHeightMult`.
  h runs from the highest point of the fall (the feet's since leaving the ground, so a
  jump's apex counts) to the landing [hypothesis: the capture's falls started at rest].
  A character put somewhere else in mid-air (a teleport, `SetPos`) falls from there
  [design]. Entering water ends the fall without damage [observed].
- **Applied** as a hit with no attacker through the combat hit path (god mode spares the
  player; death as from any hit). Armour does not reduce it [hypothesis: unmeasured].
  `fJumpFallVelocityMin` is not used (what it limits is open).
- **One leg** (issue #1129) [observed: capture o6_00, 2026-10-31, nvfairport runway, god
  mode off, Health raised by 6000, `player.SetPos z` +700 / +1000 / +1300]: Health fell
  49.88, 491.86 and 1236.13 (the rule above gives 50.3, 494.7 and 1246, within 1 %);
  each fall hurt one leg and no other limb: the right leg 100 → 75.06 (−24.94, exactly
  half the Health lost), then the left leg to 0, then the right leg to 0. Health did not
  drop again in the 12 s after landing.
  **Rule built**: the hit carries one leg, Left or Right Mobility Condition, which loses
  half the fall's damage (after the difficulty's multiplier, as the Health does) as
  condition points straight off its condition, not as a share of the leg's pool
  (crippled-limbs.md) and without `fCombatPlayerLimbDamageMult` or the Adjust Limb
  Damage perks; stopped at 0, crippled there with every crippled-leg effect (slower
  pace, the HUD's limb, Crippled lines) [observed for the player: with 6,000 extra
  Health a pool share would be far below half]. The leg is picked at random, from a
  generator seeded the same each run [hypothesis: right, left, right cannot tell random
  from alternating; design: the seed]. NPCs the same [hypothesis: unmeasured].
- **Sound** (issue #1104): a damaging landing plays `FSTLandHardHeavy` or
  `FSTLandHardLight` (audio.md §14.2), in god mode too.
- **Who**: the player, and the NPCs whose characters the physics walks
  (`PhysState::npcs`) by the same rule [hypothesis: no NPC-specific fall setting exists in
  FalloutNV.esm]. Actors moved by other systems (nav travellers, ledge jumps,
  actor-jumps.md) take none.
- **Checked**: unit tests in `falls.rs` (the capture's three falls within 1 %; the apex;
  water; a mid-air placement; half the damage off one leg, both legs picked), fnv-game's
  `limb_damage_in_points` (75.06 after 24.94, crippled at 0), smoke `falls-player` and
  `falls-player-leg`.

## 7. CLI contract

Both commands take the load order like `fnvtool world` (`<data_dir> <plugins...>
--schema <dir>`), print one JSON document and add `"load_order"` (plugin names).

### `fnvtool phys cell ... (--cell <edid|formid> | --world <edid|formid> --x X --y Y) [--radius N] [--drop x,y[,z]]... [--drops-file F] [--seconds S] [--list-refs] [--ray x,y,z,dx,dy,dz[,max]]...` → `fnv.phys.cell/1`

Builds the world around a cell (for an exterior cell, the exterior cells within `N`
grid squares, default 1; the worldspace's persistent references standing in a loaded
square are included) and drops one probe character per `--drop` point (default: the
centre of the cell), each simulated alone for `S` seconds (default 5) with no input.
Without `z` the probe starts 100 units above what a downward ray finds. Negative
coordinates need `--drop=-1,2` or `--x=-18`.

```json
{"schema": "fnv.phys.cell/1", "cell": "0x000DAEBB", "edid": "Goodsprings",
 "world": "0x000DA726", "grid": [-18, 0], "radius": 1,
 "params": {"dt": 0.016666668, "gravity": 686.7, "units_per_metre": 70.0, "max_substeps": 8},
 "character": {"radius": 18.0, "height": 128.0, "step_height": 24.0, "max_slope_deg": 45.0,
               "jump_height": 61.6, "skin": 0.5, "max_fall_speed": null, "push_mass": 80.0},
 "cells": [{"cell": "0x000E1AA8", "grid": [-19, -1], "land": true, "refs": 112}],
 "terrain": {"cells": 9, "no_heights": 0},
 "refs": {"counts": {"placed": 1136, "initially_disabled": 8, "no_model": 33,
                     "awaiting_collision": 1095},
          "by_base_type": {"STAT": 883, "TREE": 43},
          "collision_source": null},
 "world_stats": {"terrain_cells": 9, "refs": 0, "bodies_fixed": 0, "bodies_kinematic": 0,
                 "bodies_dynamic": 0, "colliders": {"trimesh": 9}, "steps": 300},
 "probes": [{"start": [-73000.0, 2000.0, 8706.0],
             "ground_ray": {"owner": {"kind": "terrain", "grid": [-18, 0]}, "z": 8606.0,
                            "normal": [0.236, 0.236, 0.943]},
             "terrain_z": 8606.0, "landed_after_s": 0.533, "landing_speed": 366.2,
             "final_feet": [-73000.0, 2000.46, 8607.5], "grounded": true, "sliding": false,
             "rest_above_terrain": 1.5, "touched": [{"kind": "terrain", "grid": [-18, 0]}]}]}
```

(The example above predates mesh collision; with it, Goodsprings counts
`"with_collision": 839, "colliders": 1369`, `"disabled_by_parent": 10` and no `awaiting_collision`, §4.) Without
`--drop`, an exterior probe falls at the cell's centre and an interior one at the middle of
the placements that have collision; ground rays span the loaded terrain and placements
with 1,000 units to spare.

Both documents list `water`: `[{"grid": [x, y] | null (the interior), "surface": z}]`, the
water areas of the loaded cells (`Cell::water_surface`, §6.6), then one entry per placed
water volume `{"grid": null, "ref", "surface", "centre", "half_extents"}` (§6.6; `refs.counts`
gains `water_volume` for references whose model holds one); probes report `swimming` and
`water_surface` (the surface above the final feet, or null).

`refs.counts` keys appear only when non-zero: `placed`, `unreadable`, `deleted`,
`initially_disabled`, `disabled_by_parent` (enabled itself but disabled at the start by
its enable-parent chain, §4), `no_base`, `actor` (placed NPCs and creatures, which get no
collision here, §4), `no_model`, `collision_primitive` (CollisionMarker
primitives added), `bad_primitive`, `primitive_model` (a primitive, a trigger, whose
base names a model: the model is left out, level-design.md §4.1), `speedtree`, `model_without_collision`,
`model_unreadable`, `awaiting_collision` (meshes not loaded), `with_collision`
(references added to the world), `colliders` (their collider count) and `object_errors`.
`by_base_type` counts references with a readable base by its record type.
`collision_source` is null without mesh collision, else `{"kind": "fnv_collide::nif",
"models", "models_with_collision", "models_unreadable", "unreadable_examples"}`. Mesh
collision needs the NIF schema: `--nif-schema <dir>`, by default `nif` next to the
`--schema` directory (skipped when absent); `--no-meshes` leaves it out. `--list-refs` adds `ref_list`: every reference in the world with `formid`, `base`
(`formid`, `type`, `edid`), `position`, `scale`, `objects` (`layer`, `layer_name`, `motion`,
`shell` × scale) and `top_hit` (the first hit, any layer, of a vertical ray at its position
from above the loaded content: `owner`, `z`, `normal`, `shell`). `--ray x,y,z,dx,dy,dz[,max]`
(repeatable) adds `rays`: `origin`, `dir` and the first `hit` (`owner`, `point`, `normal`,
`distance`, `shell`) or null. `--drops-file <file>` (`fnv.phys.drops/1`: `{"seconds"?,
"drops": [{"label", "at": [x, y] | [x, y, z], ...}]}`, other fields ignored) drops one probe
per entry, `seconds` replacing `--seconds`, and adds `observations`: `[{"label", "feet",
"support"}]` in file order (the lab compares `feet`, keyed on `label`). Owners are
`{"kind": "terrain", "grid": [x, y]}`, `{"kind": "ref", "formid", "object", "layer",
"layer_name"}` or `{"kind": "character", "id"}`.
`terrain_z` is `height_at` at the drop point (null outside loaded land);
`rest_above_terrain` = final feet z − `terrain_z` (the capsule rests its support's gap
above the surface along the contact normal, §6.2, so slightly more than the gap on
slopes). `gap` is the gap kept in the last step (`CharacterParams::rest_gap` of the
support: 3.5 on terrain, a model's shell, at least `skin`); `support` (`owner`, bare
surface `z`, `shell`) is what the arrival rule's ray finds under the final feet (from
half a capsule above to four below, ignoring the layers the character passes through,
the probe itself excluded; null if nothing) and `rest_above_support` = final feet z −
`support.z`, which on level ground equals the support's shell when the controller and
the arrival rule agree.
Output is deterministic (no timings).

### `fnvtool phys walk ... --script <file> (--world <edid|formid> | --cell <edid|formid>) [--radius N] [--every N] [--nif-schema <dir>] [--no-meshes]` → `fnv.phys.walk/1`

Builds the world as `phys cell` does around the cell holding the script's start (for
`--world`) or the given interior cell, adds one character and runs the script, stepping
the world once per script step. The script (`fnv.phys.walk-script/1`):

```json
{"schema": "fnv.phys.walk-script/1",
 "start": [-71680.0, 2048.0],
 "hz": 60,
 "params": {"step_height": 24.0},
 "segments": [{"steps": 30},
              {"steps": 300, "velocity": [250.0, 0.0]},
              {"steps": 1, "velocity": [250.0, 0.0], "jump": true}]}
```

`start` is the feet `[x, y]` (then on the contact shell of what a downward ray finds,
ignoring the layers the character passes through: `z + rest_gap(shell)`, §6.2) or
`[x, y, z]`;
`hz` the step rate (default 60, sets `dt`); `params` overrides `gravity` or any character
parameter (`radius`, `height`, `step_height`, `max_slope_deg`, `jump_height`, `skin`,
`max_fall_speed`, `push_mass`, `bottom_slope`, `bottom_flat`, and the swim parameters `swim_depth`, `float_depth`,
`swim_sink_speed`, `swim_jump_mult`, §6.6); each segment holds a horizontal velocity (units/s, world
axes, default 0) for `steps` steps, and `jump` presses jump on its first step; the
optional `view` ([x, y], the horizontal view direction) picks the backward landing profile
for a velocity pointing behind it (§6.4.1; without it the forward profile applies). The
velocity is the keys' velocity: the controller applies the air and landing rules. While the
character swims, a segment's optional `swim_velocity` ([x, y, z], units/s) is used
instead, scaled by the stroke cycle of its optional `stroke` (`"forward"`, `"back"` or
`"side"`; `fnv_phys::swim`, §6.6) counted from the first swimming step with a non-zero
`swim_velocity`; without `stroke` the speed is constant (§6.6 says why that stays the
default). A segment's optional `place` ([x, y, z]) puts the feet there before its first
step, keeping the swimming state, as the game's `SetPos` does. Output:
the `phys cell` document's world fields (`cell` ... `refs`, with `params` and `character`
as used), then

```json
{"start": [-71680.0, 2048.0, 8369.0],
 "summary": {"steps": 781, "seconds": 13.017, "end": [-70061.3, 3268.3, 8352.5],
             "horizontal_path": 2850.5, "min_z": 8344.5, "max_z": 8423.6, "jumps": 1,
             "landings": 2, "airborne_steps": 52, "swimming_steps": 0,
             "touched": [{"owner": {"kind": "terrain", "grid": [-18, 0]}, "steps": 359}]},
 "trace": [{"step": 1, "feet": [-71680.0, 2048.0, 8368.5], "vertical_speed": 0.0,
            "grounded": true, "sliding": false, "landed_speed": null, "gap": 3.5,
            "swimming": false, "touched": []}]}
```

`trace` holds every `--every`th step and the last. A script may add `"observe": {"from_step":
s, "every": n, "count": k}`: the output then holds `observations`, the feet (and `grounded`, `swimming`)
after steps s + n, s + 2n, … s + kn, labelled `"t<seconds after step s, two decimals>"`
(`t0.25` …); it must end within the script. Optional `"props": [{"label", "model", "at":
[x, y, z], "rot"?: [x, y, z]}]` (since 2026-10-09) adds each model's collision before the
walk as a reference with FormID 0xFF000000 + its index (needs the meshes), e.g. a clutter
item for the character to push; the output then has `props` (`label`, `model`, `formid`,
`bodies`, `havok_mass` (the objects' stored masses summed), `end`, `end_rotation`,
`asleep`), each trace entry `props` (first body's position per prop) and each observation
`props`. Without `props` the output is unchanged. Other top-level fields (e.g. the `capture`
block that `tools/capture/capture_moves.py` reads) are ignored. Runs are deterministic: the same
inputs give byte-identical output [observed: the Goodsprings script below, run twice].
`crates/fnv-phys/scripts/goodsprings_walk.json` is such a script (synthetic input, not a
capture); once the owner records a real walk (§6), a script with the same inputs and the
recorded positions become a parity scenario.

### `fnvtool phys heights ... --world <edid|formid>` → `fnv.phys.heights/1`

The physics terrain's height under eight fixed points of every landscape cell of a
worldspace, for comparison with an independent reader of the LAND data (parity lab
`L4.phys.heights.*`, oracle `tools/capture/oracle_phys.py`).

```json
{"schema": "fnv.phys.heights/1", "world": "0x000DA726", "edid": "WastelandNV",
 "points": [[5.0, 9.0], [27.0, 3.0], [13.0, 30.0], [30.0, 22.0],
            [8.2, 8.6], [24.6, 7.3], [7.7, 23.2], [23.9, 24.4]],
 "cells": [{"grid": [-64, -64], "heights": [1920.0004, 1951.9989, 1848.0, 1904.0009,
                                             1881.6007, 1924.8005, 1848.0, 1883.9999]}],
 "load_order": ["FalloutNV.esm", "..."]}
```

- **Cells**: every exterior cell of the worldspace's grid (one cell per grid square, the
  same selection as `fnvtool world heights`, world.md) whose CELL has a LAND record. A
  LAND without height data (no VHGT) gives `"heights": null`.
- **Points**: `points[i] = [u, v]` in grid steps (128 units) east and north of the cell's
  south-west corner `(gx · 4096, gy · 4096)`; the first four are vertices of the 33 × 33
  grid, the last four lie inside grid squares, away from both diagonals.
- **Heights**: `heights[i]` is the z where a vertical ray at
  `x = gx · 4096 + u · 128, y = gy · 4096 + v · 128` hits that cell's terrain collider
  (each cell alone in a world; `null` on a miss). The surface is the one §3 describes: at a
  vertex, the vertex height (world.md §3: VHGT integrated, × 8); inside the square with
  south-west vertex `(c, r)`, with `fu = u − c`, `fv = v − r` and `h00`, `h10` (east),
  `h01` (north), `h11` (north-east) its corner heights, the square is split along its
  south-west → north-east diagonal: if `fu ≥ fv`, `h = h00 + fu·(h10 − h00) + fv·(h11 − h10)`;
  otherwise `h = h00 + fv·(h01 − h00) + fu·(h11 − h01)`.
- **Precision**: the physics works in f32 at world coordinates up to about 2.6 × 10⁵ units
  and in metres internally; against the exact surface the rays are off by up to 0.19 units
  (`phys terrain`, §3). The lab compares with an absolute tolerance of 0.5 units, keyed on
  `grid`. On the owner's install all 30 `L4.phys.heights.*` scenarios pass; on WastelandNV
  the largest difference from the oracle is 0.154 units [observed]. Three of the four
  interior points fall in the south-east triangle of their square and one ([8.2, 8.6]) in
  the north-west one.

### `fnvtool phys terrain ... --world <edid|formid> [--samples N]` → `fnv.phys.terrain/1`

```json
{"schema": "fnv.phys.terrain/1", "world": "0x000DA726", "edid": "WastelandNV",
 "cells": 16386, "land_without_heights": [[23, -65], [24, -65]], "samples_per_cell": 126,
 "rays": 2064636, "misses": 0, "wrong_owner": 0, "mean_abs_error": 0.0018,
 "max_abs_error": 0.187,
 "worst": {"grid": [25, -59], "point": [105327.08, -239317.11], "physics_z": 441.36,
           "analytic_z": 441.18}}
```

Each landscape cell is built alone and probed with `N` scattered points (default 64) plus
62 points on its diagonal vertices and grid lines; errors are |ray hit z − `height_at`|.
A self-consistency check of the terrain colliders against the LAND data (no oracle).

## 8. Notes for workstream 1 (shape API)

- `fnv-phys` consumes `fnv_collide::{Shape, Iso, CollisionObject, Motion}` unchanged.
  It keeps its own uniform-scale helper (`convert::leaves` with a scale) rather than
  depending on `Shape::scaled`.
- Resolved: `fnv_collide::nif::collision_objects` gives node transforms in the mesh root's
  frame with the root's own transform excluded (collision.md), which is what §2 assumes;
  `fnvtool phys cell` calls it per model (fnv-phys itself reads no NIF blocks).
- Open: whether mesh collision objects use the XTRI layer numbering (§4), and which
  layers the original lets the player pass.

## 9. Collision view (`tcg`, 2026-11-11)

A developer view of what physics sees: every collider's outline drawn in the world as
lines, coloured by what it belongs to, within a radius of the camera (default 3000
units). Code: `crates/fnv-phys/src/debug_lines.rs` (the segments, feature `debug-lines`,
on by default), `engine/nvf-play/src/collision_view.rs` (drawing, Bevy gizmos).

- **Switch**: the console's `tcg` (ToggleCollisionGeometry), the original's console
  command for a colour-coded view of collision geometry [documented: Fallout Wiki,
  "Fallout: New Vegas console commands", `tcg`; GECK wiki ToggleCollisionGeometry, seen in
  search summaries; the command is not in our function catalogue]. `tcg <radius>` switches
  it on with that radius. At start: `NVF_COLLISION_VIEW=1` (default radius) or
  `=<radius>` (game units, above 1). The original colours by the layer a modeller gave a
  shape; ours by what the shape is in our physics [design]:

  | Colour | What |
  |---|---|
  | brown | terrain heightfields |
  | light grey | static collision of references |
  | cyan | keyframed parts (doors, animated collision) |
  | bright green / dark green | dynamic bodies awake / asleep |
  | yellow | character controllers (player, walking NPCs) |
  | orange | actor hit volumes (whole or part capsules) |
  | magenta | ragdoll bones |
  | red | gore pieces |
  | blue | volumes the character passes (the trigger-like layers of `layers::character_ignores` less small debris and shell casings), rapier sensors, and the script trigger volumes (`script_trigger`, which physics does not hold) |

- **How**: rapier's debug-render pipeline outlines the colliders (heightfields and
  meshes triangle by triangle, hulls by their edges, boxes, balls and capsules by
  rapier's outlines); a collider whose bounding box misses the sphere is skipped, and of
  the rest only segments passing within the radius are kept, at most 200,000. A character
  whose bounding box holds the camera (the player's own capsule in first person) is left
  out [design]. It only
  reads the world: `state_digest` and traces are unchanged (unit test). The poses are the
  last step's, so a moving body's outline can trail its interpolated model by up to a
  step.
- **Cost**: off, a run condition a frame and a disabled gizmo group. On, terrain and static
  outlines are a retained gizmo, gathered and rebuilt only when the camera moved 32 units,
  a collider came or went, or every 2 s; the rest are gathered after each physics step and
  drawn each frame. The log line `collision view: <n> segments within <r> units …` (at the first
  draw, then every 10 s) gives the counts per kind and the mean gather time.
- Smoke: `collision-view` in `tools/smoke/scenarios.toml`.
