# Spec note: level design facts from Bethesda's GECK tutorial series

Clean-room notes for Layer 3 (cells, lighting, culling), Layer 4 (primitives, physics),
Layer 5 (triggers, quests) and Layer 6 (navmesh, population, dialogue). The source is a
private export of GECK wiki pages that the owner made on 2026-10-09, kept at
`C:\fnv-private\geckwiki\Getting started.xml` and never committed. It holds Bethesda's own
"My First Vault" tutorial series and the pages around it. Everything below is our own
summary; no wiki prose is copied (§12 says how that was checked). How to click through
the editor is out of scope: only facts about data and run-time behaviour are kept.

Tags:
- **[documented: page]** means GECK wiki *page*, owner export 2026-10-09, Bethesda
  tutorial. Short page names used below:

  | Short name | Wiki page |
  |---|---|
  | Setup | Bethsoft Tutorial GECK Setup |
  | Layout | Bethsoft Tutorial Layout |
  | Clutter | Bethsoft Tutorial Clutter |
  | Navmesh | Bethsoft Tutorial Navmesh |
  | NPC population | Bethsoft Tutorial NPC population |
  | Non-NPC population | Bethsoft Tutorial Non-NPC population |
  | Traps | Bethsoft Tutorial Traps |
  | Lighting | Bethsoft Tutorial Lighting |
  | Optimization | Bethsoft Tutorial Optimization |
  | Static Collections | Bethsoft Tutorial Static Collections |
  | Finalize | Bethsoft Tutorial Finalize and Connect |
  | Basic Quest | Bethsoft Tutorial Basic Quest |
  | Primitives | Creating Primitives |
  | Occlusion | Occlusion Culling |
  | Palette | Object palette |
  | Three-way | Three Way Conversation Tutorial |
  | FAQ, Glossary, Tidbits | the pages of those names |
  | What's New | What's New in GECK v1.1? |

  The two wiki help pages in the export (editing style and syntax) were skipped.
- **[observed]**: checked this session on the owner's install (FNV 1.4.0.525,
  `FalloutNV.esm` only unless stated) with `fnvtool esm survey`, `esm decode` and
  `world refs`, read-only, 2026-10-09.
- **[hypothesis]**: neither.

**Caveat on the source.** The series was written for *Fallout 3* and its editor. Two
pages say so about themselves: Static Collections and Occlusion both describe internal
documents that may mention features absent from the retail game. A [documented] fact
below is therefore a statement about the Fallout 3 engine generation, to be confirmed for
New Vegas where it matters; the Occlusion page itself notes that some debug views do not
work in New Vegas.

## 1. Cells, worldspaces and references

- **Interior and exterior cells.** An interior cell is joined to the rest of the game only
  through teleport doors. Exterior cells are the square grid tiles of a worldspace; the
  Finalize page calls them 4000 units wide [documented: Finalize]. Our data puts them at
  4096 (world.md §1 [observed]); the tutorial's figure is a round number (§11).
- **Cell-level settings.** A cell carries its own ambient light, fog and image space;
  copying a cell copies all three [documented: Layout]. This matches XCLL / lighting
  template and XCIM in world.md §8.
- **Cell name.** The cell's display name is what the player sees on the local map and on
  a door leading into the cell, and it is shown for saves in the load menu
  [documented: Layout]. Exterior cells can be named too [documented: Finalize].
- **Navmesh is cell content.** A cell's navmesh is a separate object in it, which is not
  removed with the cell's ordinary references [documented: Layout] (NAVM records in the
  cell-children group, world.md §2).
- **Load doors.** A door becomes a load door by marking it as teleporting and pointing it
  at a door in the other cell. Each side then has a teleport marker (a position and a
  facing) placed by the destination door: that marker is where, and facing which way, an
  actor arrives after using the door on the other side. The marker should face away from
  the door and stand clear of geometry [documented: Finalize]. In the data this is REFR
  `XTEL` (door, position/rotation) [documented: xEdit, simulation.md §5].
- **Doors and AI.** A linked door pair is at first usable by the player only. Actors can
  path through it once both cells' navmeshes are finalized, which puts a "teleport"
  triangle under each door marker (§5) [documented: Finalize].
- **COC marker.** The console's `coc <cell>` puts the player at the cell's
  COCMarkerHeading; at most one belongs in a cell [documented: Layout]. In the data
  [observed]: 133 COCMarkerHeading references (base 0x32) in the interiors and
  WastelandNV, one cell holding two.
- **North marker.** By default the top of the editor's top view is north; a NorthMarker
  overrides that in an interior and does nothing in an exterior cell [documented: Layout;
  Finalize]. What it changes in game (local map, compass) is not stated. [observed]: 199
  interior cells have one, none more than one.
- **xMarker and xMarkerHeading** differ only in their editor model; the heading version
  shows which way an actor standing there will face [documented: NPC population]. Both
  are persistent by default; idle markers are not [documented: NPC population].
  [observed]: all 1,256 XMarker and 2,240 XMarkerHeading references in the interiors and
  WastelandNV are persistent; 718 of 1,214 idle-marker references are.
- **Persistent references.** A reference that another reference links to (linked
  reference, activate parent), a door a terminal controls and a quest target must be
  persistent; non-persistent references cannot be picked as link targets
  [documented: NPC population; Non-NPC population; Traps; Basic Quest; FAQ].
  [observed]: in a sample of 800 actors, activators, terminals, doors and idle markers,
  all 116 `XLKR` targets and all 5 `XAPR` targets are persistent.
- **Base objects and references.** Editing a base object changes every reference to it;
  a reference takes most of its properties from its base [documented: Layout; Glossary].
- **Map markers** are not covered by these pages.
- **Units.** The player stands 128 units tall, about six feet [documented: Layout];
  physics.md §6 already assumes a 128-unit capsule [hypothesis there].

## 2. Lighting

- **Light objects are separate from fixtures.** A ceiling lamp model gives no light; a
  light object (LIGH) is placed beside it [documented: Lighting].
- **Radius per reference.** A placed light's radius can be changed on the reference; the
  radius sphere shows which geometry the light reaches [documented: Lighting]. In the
  data a placed reference can carry `XRDS` "Radius" [documented: xEdit,
  `schemas/fnv/common.json`]. [observed, §11 row L1]: 7,693 REFRs in FalloutNV.esm carry
  `XRDS`. Of its 7,742 interior light references, 6,484 do, with values from −709 to
  +3,821 (median 43; 2,142 negative), and base radius + `XRDS` is positive in every case
  (`XRDS` / base radius from −0.885 to 73). `XRDS` is added to the LIGH `DATA` radius
  [observed: in-game interior frames, world.md §8 "Placed light radius"]. Example: Doc Mitchell's OfficeLightKeyNeutral 0x00103E6D has base
  radius 200 and `XRDS` 49.0.
- **Cost model.** Every light that reaches a piece of geometry roughly doubles that
  piece's rendering cost; the light-count debug view colours whole pieces, not polygons
  [documented: Lighting]. Big static collections gather more lights on one object for the
  same reason [documented: Static Collections]. So the original assigns lights per object
  (by the object's bounds against the light's sphere) [hypothesis from the cost
  description], unlike a per-pixel clustered renderer.
- **Light kinds are naming conventions**, not data: ambient lights (dim, for bounce light
  where no source is visible), fill (the common case), key (bright, to draw attention) and
  kicker (brightest, used sparingly), each in cool, neutral and warm colour variants
  [documented: Lighting].
- **Emittance.** A glow, light beam, particle system or special static such as a window
  can take its colour and brightness from a light (the reference's emittance); the editor
  offers one choice for interiors and one for exteriors [documented: Lighting]. Effects
  and lights can also follow daylight [documented: What's New]. This is REFR `XEMI`,
  which names a LIGH or a REGN (world.md §8).
- **Particles** (e.g. smoke wisps) are placed objects and take emittance like glows
  [documented: Lighting].
- **Alternative lighting sets.** A level may hold two sets of lights for the same space,
  for instance dim lights replaced by bright ones when the player starts a generator; the
  sets are never enabled together in game [documented: Lighting]. So enabling and
  disabling lights at run time is a designed mechanism (§11 row L3).
- **Image space, fog and ambient** come with the cell (§1).
- **Shadows and light fade** are not described on these pages.

## 3. Culling and performance

### 3.1 Rooms, portals and the unbounded space

- **Room markers** are boxes. The objects inside a room marker belong to it. With the
  camera inside room A, A's objects are drawn; objects of another room are drawn only if
  that room is seen through a portal. Outside a room, its objects are culled
  [documented: Optimization].
- **Portals** join two rooms. A visible portal makes the rooms it joins visible; without a
  portal between two rooms, neither shows from the other [documented: Optimization].
  Linking a portal to a room snaps it to the room's face [documented: Optimization].
- **The unbounded space.** Once a cell has rooms, everything outside them is the
  unbounded space, which acts as one more room: a portal is needed to see it from a room
  and to see a room from it. Missing portals show as "the void" [documented:
  Optimization; Occlusion].
- **The run-time rule.** With the camera in a portal space, each portal of that space is
  tested against the view frustum. If one is visible, the space behind it is gathered
  with a new frustum, the intersection of the current one with the portal's projected
  volume, and the step repeats from there (the page names the structure
  `BSCompoundFrustum`). A portal hidden by an occlusion plane is not followed at all, nor
  are the portals beyond it [documented: Occlusion].
- **Room edges must meet exactly.** Gaps or overlaps between rooms render wrongly;
  portals that are too small or not flush with their rooms make the level vanish at some
  angles [documented: Optimization].
- **Moving objects keep their room.** An object belongs to the room it was placed in.
  While Havok moves it, it is drawn regardless; once it comes to rest it is drawn again
  only with its original room, even if it now lies in another one, where it is then
  invisible [documented: Optimization].
- **Data** [observed]: room markers are base 0x1F "RoomMarker" with `XPRM` type 1 (box)
  and `XMBO`; portals are base 0x20 (no record) with `XPRM` type 3 ("portal box", one unit
  thick) and `XPOD` naming two references. All 849 interior portals with `XPOD` name only
  room markers; 188 have one null entry, which we read as the unbounded space
  [hypothesis, matching the tutorial]. No `XMBR`, `XRMR` or `XLRM` occurs in FalloutNV.esm,
  so room membership is not stored per object and must be computed by containment when
  the cell loads [hypothesis]. All 677 interior room markers are persistent, none of the
  851 portals is. WastelandNV has no rooms or portals.
- **Linked rooms** [documented: xEdit's record definitions name `XRMR` "Linked Rooms
  Count" and `XLRM` "Linked Room"; observed: `fnvtool esm survey`, TTW 3.4 load order,
  2026-11-08]: no New Vegas master has them, but Fallout 3's do, as Tale of Two
  Wastelands loads them (REFR `XLRM` / `XRMR`: Fallout3.esm 320 / 265, Anchorage 16 / 13,
  The Pitt 40 / 29, Broken Steel 38 / 29, Point Lookout 24 / 19, Mothership Zeta 69 / 56,
  TaleOfTwoWastelands.esm 50 / 43). On a room marker each `XLRM` names another room
  marker; the data names links both ways (Vault 101: 0x060ABBF5 → 0x060ABBF6, and
  0x060ABBF6 → 0x060ABBF5 and 0x060ABBF7). The rule we build [hypothesis, from the name
  and from that room: the space outside the G.O.A.T. classroom has no portal but the
  classroom door, yet the corridor it opens onto is meant to be seen]: linked rooms are
  seen together as if they were one room, through the same frustum (`engine/nvforever/src/rooms.rs`).
  Owner report 2026-11-08: without them the corridor was culled from there.
- **Primitive bounds are half extents** [observed, settles physics.md §4.1's
  hypothesis for room markers]: of the 1,341 (portal, room) pairs with an unrotated room,
  1,298 have the portal's centre on a face of the room box when `XPRM` bounds are read as
  half extents, and 0 when read as full extents. Example: rooms 0x0008D447 (z 432 ± 144)
  and 0x0008D448 (z 112 ± 176) meet at z 288, where their portal 0x0008D44E lies.

### 3.2 Multibounds and occlusion planes

- **Multibounds** group objects so the culling system treats them as one: if any part
  is visible, all of it is drawn. They are meant mainly for exteriors [documented:
  Optimization]. Multibounds are static, so their visibility is computed once per frame,
  before drawing starts, and a change of their contents does not change the test
  [documented: Occlusion].
- **Occlusion planes** are flat primitives. Each frame every multibound is tested
  against every plane's occlusion frustum (the plane projected away from the camera); a
  multibound wholly inside it is skipped with all its contents [documented: Occlusion].
  The cost grows with planes × multibounds, so they are placed sparingly [documented].
- **Debug commands** [documented: Occlusion]: `TFC 2` freezes what is drawn and shows
  multibounds and planes colour-coded; `MBC` turns off all multibound and room/portal
  culling. The room views of `TFC 2/3/4` are said not to work in New Vegas.
- **Data** [observed]: occlusion planes are base 0x17 (no record) with `XPRM` type 3,
  `XOCP` (size, position, rotation) and `XMBO`; 44 in the interiors and WastelandNV, 8 with
  `XORD` (linked planes), all four links null. The plane's `XOCP` width and height equal
  its `XPRM` x and z bounds (e.g. 0x001330BB: 1,971.7 × 310.7), so `XOCP` sizes are half
  sizes too if §3.1's reading holds [hypothesis]. `XMBO` equals the `XPRM` bounds on the
  rooms and collision markers decoded; on portals and planes its thin axis is 0, and one
  plane (0x000CD790) has a different height (504 against 384). Two REFRs carry `XMBP`
  "MultiBound Primitive Marker". Which references are multibounds in
  New Vegas is open (§13).
- **Plane bounds are half extents** [observed, fnvtool `esm decode` of the 44 planes,
  2026-11-06]: the five planes of RocketLabMid (0x000CD790–0x000CD794, all turned by a
  multiple of 90° about z) tile the walls of one room edge to edge when their `XPRM` x
  and z bounds are read as half extents, and leave gaps when read as full sizes: along
  x = 3,328, 0x000CD790 spans y 576–1,152, 0x000CD792 (above a doorway, z 80–544) 1,152–1,408
  and 0x000CD791 1,405–1,981; 0x000CD793 and 0x000CD794 run along y = 576 and y = 1,984
  from x 2,752 to 3,328, meeting the first two at their ends. The planes of
  OVWestSewers02 meet at their corners the same way (0x00178AA9 and 0x00178AAA at about
  (−1,285, −3,570)). So `XOCP`'s sizes, equal to the bounds, are half sizes too.
- **`XOCP`'s other fields** [observed, same decode]: its position is within a few units
  of the reference's `DATA` position (0x001330BB: 28,724.1, 31,738.9 against 28,723.9,
  31,741.1); its four rotation numbers read as an axis and an angle, not a unit
  quaternion: `(0, 0, 1, a)` or `(0, 0, −1, 2π − a)` for a `DATA` z turn of `a`
  (0x000CD793: z 3.14, `(0, 0, 1, 3.142)`; 0x000CD790: z 4.71, `(0, 0, −1, 1.571)`), and
  `(1, 0, 0, 0)` unturned. All 44 planes are turned about z only and have scale 1.
- **NVForever's rule** (#1434; `engine/nvforever/src/occlusion.rs`, `rooms.rs`): a plane
  is the rectangle of its `DATA` position and rotation and `XPRM` half extents, thin axis
  left out (as a portal's; `XOCP` and `XORD` are not read). Each frame, for each plane not
  edge-on to the eye (within 1 unit), the volume it hides from the eye is built (beyond
  the plane, inside the four planes through the eye and its edges). Multibounds are not
  used: every placed object's and item's mesh is tested by its own axis-aligned bounds in
  game space [engineering choice, while §13 question 8 is open], and hidden when all eight
  corners lie in one plane's volume (the volume is convex, so the mesh is wholly behind
  that plane); planes are not joined. A portal whose four corners lie in one plane's
  volume is not followed (§3.1). Actors, dropped items, lights and effects are never
  hidden, as with rooms. `--no-occlusion-planes` turns it off.
- **Interiors only, by default** [engineering choice]: not every exterior plane lies
  inside its wall. Primm's 0x00177B3D (x −57,480 to −53,480, z 5,776–6,256, half extents)
  runs along a retaining wall (rwstraightstonenosidewalk02_nv, scale 0.75, base z 5,744)
  whose mesh reaches z 6,168 [observed: the NIF's vertices], so 88 units of the plane
  stand above the wall, and with per-mesh tests the letters of a roof sign seen over it
  were hidden [observed: NVForever screenshots with and without, 2026-11-06]. Its length
  matches the wall's corner stones only as half extents, so the reading is not in doubt.
  The original tests multibounds only, so that sign would stay unless it belonged to
  one; per-mesh tests are not safe outdoors. `--occlusion-planes-outdoors` uses the
  exterior planes too (for measuring). In the interiors checked, the pictures with and
  without were the same.

### 3.3 Distance, LOD and static collections

- **Object fade depends on bounds.** An object fades in at a distance that grows with
  its bounds: a sign built from small letters appears only close to the player, and
  joining small parts into one bigger object makes them appear from farther away
  [documented: Static Collections].
- **Static collections (SCOL)** combine several static references into one new model,
  written to `meshes\scol\`; the first selected part gives the pivot. Only statics can be
  collected, and collections cannot be nested [documented: Static Collections].
  [observed]: all 954 SCOL references in WastelandNV use a model in `meshes\scol\`.
- **SCOL and LOD.** If parts have LOD models, a LOD model is built from those parts only,
  and the collection is marked visible when distant; changes to the parts reach the
  collection only when it is rebuilt [documented: Static Collections]. [observed]: 101 of
  the 954 SCOL references in WastelandNV have the visible-when-distant flag.
- **Why collect**: fewer objects and draw calls, but only for parts that share textures;
  hidden geometry can be removed from the combined model; very large collections attract
  more lights [documented: Static Collections].
- **SCOL collision**: after collecting, collision mostly works but impact decals stop,
  because the editor reattaches the collision objects to nodes without geometry
  [documented: Static Collections]. A note for the renderer and for decals later.
- **LOD** covers terrain and large buildings, so they do not appear abruptly
  [documented: Static Collections].

## 4. Primitives and trigger volumes

- **Primitives** are editor-made boxes or spheres that can be scaled differently along
  each axis. They serve as trigger volumes, multibounds and occlusion planes, and they
  have a colour of the designer's choice (no colour convention) [documented: Primitives;
  What's New]. A trigger is an activator (ACTI) whose reference has a primitive; the
  activator may be new for each trigger [documented: Primitives; Basic Quest].
- **Flags.** An activator primitive has an **ActorZone** flag: with it the trigger checks
  collision only with actors, and "most triggers" use it. A **Projectile** flag makes a
  trigger respond to projectiles; the page calls it untested [documented: Primitives].
  So without ActorZone a trigger reacts to any colliding object, not only actors
  [hypothesis from the wording].
- **Detection is a collision check** [documented: Primitives]: larger triggers cost more,
  and very thin ones can be missed, especially when the frame rate is low. We read this as
  an overlap test of the object's collision against the volume once per physics step,
  with no sweep between steps [hypothesis].
- **Blocks** [documented: Basic Quest; What's New]:
  - OnTriggerEnter and OnTriggerLeave run when something enters or leaves; OnTrigger runs
    **every frame** while it is inside, which costs more than the enter/leave blocks.
  - The block parameter (`player` in the example) limits it to that reference; without
    one, any actor counts ("while an actor is within").
  - A trigger's script runs only while its cell is loaded; quest scripts, by contrast,
    always run and update less often.
- **Collision markers and collision layers** are not described on these pages. [observed]:
  no reference in FalloutNV.esm has `XTRI`, and the Vigor Tester trigger 0x00104C16 and its
  activator 0x00104C07 carry no record flags, so where the GECK stores ActorZone and
  Projectile is unknown (§13). In Fallout3.esm, `MS01TriggerJunkyard1Ref` (0x000651DA),
  `MS01TriggerJunkyard2Ref` (0x0002E738, both `OnHit` shooting-range triggers), and
  `0x00054468` (`FFBaseballSCRIPT`: `OnTriggerEnter`, `OnHitWith`) carry `XTRI` 23
  [documented: xEdit names layer 23 `Projectile Zone`; GECK wiki Creating Primitives: the
  Projectile flag is "Used for triggers which can be activated by a projectile";
  hypothesis: `XTRI` 23 is how the Projectile flag is stored, #3662].

### 4.1 What a primitive shows in game

- **A primitive shows nothing in game.** The editor draws it as a coloured, see-through
  shape, and only while marker viewing is on [documented: Creating Primitives, "If your
  primitive isn't visible, you may have marker viewing turned off"]. The console switch
  for primitives is ToggleAllPrimitives [documented: ToggleAllPrimitives]. So a reference
  with `XPRM` is drawn as its primitive or not at all, never as its base object's model
  [hypothesis: the pages describe the editor; not captured in game, but no primitive
  with a model is ever meant to be seen, see the next point].
- **Copied triggers keep the model** [observed: survey of the `XPRM` references in the
  six base and DLC ESMs, 2026-11-05]. FalloutNV.esm has 5,677 references with `XPRM`:
  3,438 on CollisionMarker, 922 room, portal and occlusion markers with no base record,
  1,200 activators with no model, 49 acoustic spaces, 2 sounds, and **66 activators
  whose base names a real model**: 64 `Architecture\Wasteland\WaterTowerPipes01.NIF`
  (most named "Gas Valve", e.g. Hoover Dam's battle triggers, `v34AmbushVolume`,
  `REPHQEnterTrigger`) and 2 `Dungeons\Vault\Accessories\ControlPanelVault01.NIF`
  (Sloan's `QJSnufflesCrippleTrigger`, 0x00140C37, whose only script block is OnLoad).
  Old World Blues and Lonesome Road add one pipe trigger each. The trigger was made by
  copying an activator, and the copy kept its model and name.
- **NVForever's rule** (#1599): a reference with `XPRM` is not drawn (render streaming
  and script Enable), gets no collision from its base model (only CollisionMarker boxes
  collide, physics.md §4), and is not offered under the crosshair, whatever FULL its
  activator has. Its trigger shape is the primitive (`script_events.rs`, T2). Code:
  `fnv_world::placed::Placement::draws_base_model`; `engine/nvforever/src/world.rs`,
  `engine/nvf-play/src/script_draw.rs`, `crates/fnv-phys/src/placed.rs`
  (`primitive_model` count), `engine/nvf-play/src/activate.rs` (`describe`).

## 5. Navmesh (Layer 6 groundwork)

- A navmesh is a set of triangles telling actors where they can walk; it replaces Oblivion's
  path-grid nodes [documented: Navmesh; What's New]. Triangles close to equilateral path
  best; long thin ones are to be avoided [documented: Navmesh].
- **Obstacles at run time.** Doors and objects with the obstacle setting cut the navmesh
  in game; movable (Havok) objects need no hole, the navmesh runs under them and the game
  handles them. Fixed big clutter must be cut out of the navmesh by hand; actors do not
  know about rocks placed over navmesh and will try to walk there [documented: Navmesh;
  Finalize]. "Big clutter" is defined as anything that changes where actors can walk
  [documented: Clutter].
- **Stairs** are bridged by triangles from the top edge to the bottom edge
  [documented: Navmesh].
- **Cover.** Cover edges store the height of cover, which actors use in combat and in
  ordinary pathing; each cover piece has sides from which an actor shoots. Navmesh can
  also mark ledges and areas under water [documented: Navmesh].
- **Finalizing** works per cell: it joins the cell's border edges to the neighbouring
  cells' navmeshes and makes the door "teleport" triangles that let actors path between
  an interior and an exterior. A balance (automatic re-triangulation) pass removes cover
  and finalize data [documented: Navmesh; Finalize]. In the data the door link is REFR
  `XNDP` (navmesh, triangle) [documented: xEdit]; 1,097 REFRs carry it [observed].
- **Plugins with navmesh** should carry the master (ESM) flag, or the game may misbehave
  and crash [documented: Navmesh; Layout].
- Navmesh errors are found when the editor saves; how the game treats a broken triangle
  is not said.

## 6. Population

- **Leveled actors.** Hostile actors are placed as leveled-list references; a list picks
  from its entries by the player's level together with the cell's encounter zone
  [documented: NPC population].
- **Encounter zones** set a minimum level for an area: a level-2 player in a zone of
  minimum 10 gets actors as if level 10. Zones also group cells into one area
  [documented: NPC population; What's New]. [observed]: 17 ECZN records; `XEZN` on 84
  cells, 5 placed NPCs and 11 placed creatures.
- **No recalculation.** A zone's level is fixed the first time the player enters it; the
  actors leveled lists produced there never become harder later [documented: NPC
  population]. Whether respawned actors reuse the fixed level is not said (§13).
- **Thin lists.** Most Fallout leveled lists hold a few tiers; the stats of a tier do not
  scale with the player. Asked for level 10 from tiers at 1, 9 and 16, the list yields the
  level-9 tier [documented: NPC population]. This agrees with fnv-sim's "highest eligible
  level" rule (simulation.md §4).
- **Level modifiers** on the placed reference (`XLCM`) shift the difficulty relative to the
  zone: None, Easy, Medium, Hard, Boss. Easy may yield any actor of the list that counts
  as easy, which adds variety for high-level players [documented: NPC population].
  [observed]: 178 NPC and 119 creature references carry `XLCM`.
- **Leveled turrets** take their difficulty from the turret's modifier and the zone
  [documented: Non-NPC population].
- **Default packages.** Most basic enemy leveled lists bring generic patrol and sandbox
  packages [documented: NPC population].
- **Patrols** follow a chain of linked references (xMarkerHeading or idle markers). The
  actor may be linked to any point and starts at the nearest one. An open chain is walked
  back and forth (1-2-3-4-3-2-1), a closed one in a loop. A point can carry an idle time
  (`XPRD`); at an idle marker the actor plays one of its animations if the wait allows,
  and idle markers must suit the actor type [documented: NPC population]. [observed]:
  1,347 `XPRD` in FalloutNV.esm.
- **Sandbox.** An actor not linked to anything falls back to sandboxing: wandering the
  area and using furniture and idle markers within 512 units of where it is
  [documented: NPC population].
- **Furniture markers.** Several figures on a furniture model are entry points; the
  approach side needs navmesh. A furniture marker that includes a visible object places
  that object in game [documented: NPC population].
- **Mixed groups** of melee and ranged enemies, and lists by weapon type (melee, gun,
  "special" heavy weapons) are design conventions [documented: NPC population].
- **Containers and respawn** are not covered by these pages.

## 7. Locks, terminals, traps, clutter and ownership

- **Lock levels and skill**: the Lockpick skill needed is 0 for Very Easy, 25 for Easy,
  50 for Average, 75 for Hard and 100 for Very Hard; Requires Key cannot be picked. A lock
  can only be attempted with Lockpick at least that high. A
  "requires key" lock without a key can never be opened in play. The "leveled" lock
  setting is deprecated [documented: Non-NPC population]. This agrees with the levels
  0/25/50/75/100/255 in simulation.md §5 and supports reading 255 as "requires key".
- **Inaccessible doors** can never be opened [documented: Non-NPC population]; where the
  flag is stored is open (§13).
- **Terminals.** A terminal's hacking difficulty is on its base object, not the
  reference. A terminal controls the door, container or turret that is its linked
  reference (`XLKR`): it reads its state (locked, turret status) and unlocks or commands
  it [documented: Non-NPC population; FAQ].
- **Activated traps.** A trap is the activate *child* of its trigger (tripwire, pressure
  plate), the activate *parent*; one parent may set off several traps and a trap may have
  several parents. Swapping the trigger's base keeps the relation [documented: Traps;
  Non-NPC population]. In the data: `XAPR` (parent reference and delay) and `XAPD`
  "parent activate only" [documented: xEdit; observed 68 and 216 REFRs].
- **Stand-alone traps** react to the player's proximity, damage or similar. Gas traps go
  off when hit by a weapon in form list `ExplodesGasTrapsOnHit`, by explosions, when
  touched with a weapon in `ExplodesGasTrapsOnTouch` or while burning, or when a weapon is
  fired inside them; neighbouring gas traps set each other off [documented: Traps].
- **Mines** are placed projectiles, not activators [documented: Traps; Non-NPC
  population]. In the Fallout 3 tutorial they trigger with the player within 100 units
  indoors and 160 outdoors, a frag mine's blast reaches 192 units, and mines placed close
  together set each other off [documented: Non-NPC population]; New Vegas values are not
  checked (§13).
- **Ownership.** Mines owned by a faction are not set off by that faction's members
  [documented: Traps; Non-NPC population]. [observed]: 108 of 174 placed grenades/mines
  (PGRE) in FalloutNV.esm carry `XOWN`.
- **Turrets** turn through 360°; leveled turret lists are set up not to attack the
  faction they belong to [documented: Non-NPC population].
- **Clutter settling.** Movable clutter is placed slightly above a surface and dropped
  with the editor's physics simulation, so what the plugin stores is the settled pose
  [documented: Palette; Non-NPC population]. Decals and flat "poster" objects sit one
  unit above their surface to avoid flicker [documented: Palette].
- **Destructible objects** are not covered by these pages.

## 8. Quests

The Basic Quest page is already a source of quests-dialogue.md. New here: a trigger is
the efficient way to start a quest stage on entering an area, and notes cannot carry
scripts, so picking one up is detected from a trigger with `GetHasNote`
[documented: Basic Quest]. Quest targets need persistent references (§1).

## 9. Dialogue: conversations between three actors

- `Say` (to nobody in particular) and `SayTo` (to a given actor) start a line. When the
  line ends, the speaker's script runs its `Begin SayToDone <topic>` block for that
  topic; the example uses this after `Say` as well as after `SayTo`
  [documented: Three-way]. (One line of the page's example names the wrong speaker; only
  the pattern is used here.)
- A conversation is a chain: one actor's GameMode starts it, and each SayToDone block
  makes the next actor speak, with no timers or variables [documented: Three-way].
  Layer 6 must therefore raise SayToDone at the end of the line on the speaker, whichever
  function started it.

## 10. Glossary, FAQ and Tidbits

- FormIDs are the run-time identifiers; Base IDs are FormIDs of base objects and
  Reference IDs those of placed objects. Editor IDs are for the editor and scripts and
  never shown in game; they cannot contain underscores (removed on save), spaces or
  special characters [documented: Glossary].
- A record is any unit saved in a plugin: base objects, references, game settings
  [documented: Glossary].
- Fallout 3 and New Vegas NIF files are a different version from Oblivion's
  [documented: Glossary]; Oblivion-only block types do not apply (nif.md).
- A new plugin saved with a `.esm` name gets the master flag in its header
  [documented: FAQ; Setup].
- The rest of FAQ and Tidbits is editor advice (settings, snapping), out of scope.

## 11. Cross-check with our notes and code

### 11.1 Agreements

- Cell lighting, fog and image space as cell data (world.md §8) — §1.
- XTEL arrival marker per door (simulation.md §5) — §1. The marker's facing as arrival
  facing supports simulation.md's [hypothesis] on the arrival heading.
- COC goes to the COCMarkerHeading (`crates/fnv-game/src/runner.rs`, `go_to_cell`) — §1.
- Leveled lists pick the highest eligible level (simulation.md §4,
  `crates/fnv-actor/src/resolve.rs` `leveled_pool`) — §6.
- Lock levels (simulation.md §5) — §7.
- External emittance from a LIGH or REGN (world.md §8, `engine/nvforever/src/emittance.rs`)
  — §2, for the choice of source.
- SCOL models are combined meshes in `meshes\scol\` (layer3.md §2.1, world.md §9 q6
  "assumed") — §3.3; the tutorial and the data both support drawing the combined mesh.
- Rooms, portals and occlusion planes are culling volumes, not collision
  (physics.md §4.1 leaves them out of physics) — §3.1, §3.2.
- Basic Quest facts already in quests-dialogue.md — §8.

### 11.2 Differences and settled hypotheses

| # | Area | Tutorials say | Our notes / code | Where | Kind |
|---|---|---|---|---|---|
| T1 | Trigger detection: who | any actor can fire OnTriggerEnter/Leave/OnTrigger; a block parameter limits it to one reference; without ActorZone even non-actors; with the Projectile flag projectiles [Primitives; Basic Quest; What's New] | since 5D round 2: the player and every present, living NPC and creature of the loaded cells; the runtime matches a block parameter (`OnTriggerEnter player`) against the actor; non-actors and projectiles do not fire them (ActorZone / Projectile not read, T5) | `engine/nvf-play/src/script_trigger.rs`, docs/design/layer5-play.md §3 | agrees for actors; non-actors open |
| T2 | Trigger detection: how | a collision check of the object against the volume; thin triggers can be missed at low frame rates [Primitives] | since 5D round 2: the actor's capsule (the player's physics capsule; `CharacterParams::default()` × scale for others [hypothesis]) against the box or sphere, once per frame | `engine/nvf-play/src/script_events.rs` (`Capsule`, `TriggerShape::touches`), layer5-play.md §3 | agrees in method (geometric, not physics sensors, T6); capsule sizes hypothesis |
| T3 | OnTrigger frequency | every frame while inside [Basic Quest] | every frame inside, entering frame included, OnTriggerEnter first on that frame [hypothesis] | script_trigger.rs, layer5-play.md §3 | settled for frequency; order on the first frame still open (§13 question 2) |
| T4 | Trigger scripts and cell loading | trigger scripts run only while their cell is loaded [Basic Quest] | candidates are scripted references of loaded cells | script_trigger.rs | agrees |
| T5 | Trigger flags | ActorZone and Projectile flags on the primitive [Primitives] | `XTRI 23` (`Projectile Zone`) scripted primitives (`OnHit`/`OnHitWith`) are crossed by flying and hitscan shot segments without stopping the shot (`script_trigger::projectile_zones`, `combat::fly` -> `Game::hit_object`) [hypothesis: #3662]; ActorZone still unread (no `XTRI` in FalloutNV.esm) | `crates/fnv-phys/src/primitive.rs`, `engine/nvf-play/src/script_trigger.rs`, `engine/nvf-play/src/combat.rs` | Projectile Zone built (hypothesis); ActorZone open |
| T6 | "Physics sensors (5D)" | detection is the physics engine's collision | layer5.md §3.4 table already says physics sensors; the implementation is geometric | docs/design/layer5.md §3.4 vs layer5-play.md §3 | consistent with the plan, not with the interim code |
| C1 | Room/portal culling | rooms, portals, unbounded space, portal frusta (§3.1) | built (#606): in interiors only the rooms seen through portals are drawn; membership by box overlap grown by 32 units, the narrower frustum a screen rectangle, everything drawn with the camera in no room (never the void); portals wholly behind an occlusion plane are not followed (C3); `--no-room-culling` turns it off | `engine/nvforever/src/rooms.rs`, smoke `rooms-repconn` | simplified (performance; the original can show "the void" where portals are missing, NVForever does not) |
| C2 | Moved objects and rooms | a settled object is drawn only with its original room [Optimization] | dropped items, actors and effects are never room-culled; moved objects are always drawn | `engine/nvf-play/src/dropped.rs`, world.rs | difference (fidelity edge case) |
| C3 | Multibounds and occlusion planes | per-frame occlusion test of multibounds against plane frusta [Occlusion] | built (#1434): occlusion planes in interiors hide the placed objects' meshes wholly behind one plane, each by its own bounds, and stop portals wholly behind one; outdoors only with `--occlusion-planes-outdoors` (a Primm plane pokes above its wall, §3.2); multibounds are not used (§13 question 8); `--no-occlusion-planes` turns it off | `engine/nvforever/src/occlusion.rs`, `rooms.rs`, smoke `occlusion-planes-vault21` | simplified (performance only; per-mesh bounds instead of multibounds) |
| C4 | XPRM bounds | (room edges must meet; portals snap to faces) | physics.md §4.1: half extents [hypothesis] | `crates/fnv-phys/src/primitive.rs`, physics.md §4.1 | **settled by our data**: 1,298 of 1,341 portal centres on a room face as half extents, 0 as full extents (§3.1) |
| C5 | Object fade by bounds | objects fade in at a distance growing with their bounds [Static Collections] | all objects of loaded cells are drawn at any distance; fades exist only per cell and for LOD | `engine/nvforever/src/fade.rs`, layer3.md §2.2 | difference |
| L1 | Light radius | radius set per reference [Lighting] | range = base radius + `XRDS` (`placed_light_radius`); SSIM up on all seven interior frames (world.md §8) | `engine/nvforever/src/lights.rs`, `crates/fnv-world/src/lighting.rs`, world.md §8 | **settled** (2026-10-09) |
| L2 | Lights per object | cost per piece of geometry per light [Lighting; Static Collections] | per-pixel clustered lights (Bevy) | lights.rs | difference in model; matters if the game caps lights per object (open) |
| L3 | Lights enabled by scripts | alternative light sets switched by enable state [Lighting] | a light a script enables or disables keeps its streamed state | layer5-play.md §7, `engine/nvf-play/src/script_draw.rs` | difference (already listed as open there) |
| L4 | Emittance brightness | emittance carries the light's colour *and brightness* [Lighting] | a LIGH emittance source passes colour × fade; for a light naming a light the rule is open (world.md §8) | `engine/nvforever/src/emittance.rs`, `crates/fnv-world/src/lighting.rs` (`Emittance::Light`) | agrees for shapes; open for lights |
| P1 | Encounter zones | zone minimum level; level fixed on first entry; modifiers None…Boss [NPC population] | built 2026-10-21: zone level on first entry, references fixed at their level, `XLCM` modifiers, Never Resets | [encounter-zones](encounter-zones.md), `fnv_sim::zones`, `fnv_game::zones` | same by the documented rules (the percentages' rounding and the flag handling are hypotheses) |
| P2 | PlaceLeveledActorAtMe | level modifier and zone apply as for placed leveled actors | modifier and zone arguments ignored | `crates/fnv-game/src/functions/objects.rs` `place_leveled_actor_at_me` | difference (already marked Layer 6 there) |
| P3 | Respawn | not covered | fnv-sim's 72 h cell respawn and merchant restock | simulation.md §3 | nothing to compare |
| D1 | SayToDone | runs on the speaker after a `Say` line as well as `SayTo` [Three-way] | since 5D round 2 `fnv-game` runs it after either; the runner input says "a SayTo or Say line finished" | `crates/fnv-game/src/game.rs` (`update_speech`), scripts.md §7.2, §7.7 | agrees |
| W1 | Exterior cell size | 4000 units [Finalize] | 4096 [observed] | world.md §1 | tutorial imprecise; ours stands |
| W2 | Player height | 128 units [Layout] | capsule height 128 [hypothesis] | physics.md §6 | supports ours (Fallout 3 figure) |

## 12. How this note was checked

- Facts are re-expressed; nothing is quoted beyond page titles, editor IDs, form-list
  names, function names and console commands.
- A script compared this file with the export's 20 used pages: lower-cased word tokens,
  no 8-word run in common.

## 13. Open questions to capture

1. **Who fires triggers** (T1, T2): with an xNVSE capture job (xnvse-capture.md), walk an
   NPC (a follower, or one on a package) and throw an item through an OnTriggerEnter trigger without a `player`
   parameter and record whether the block runs and with which action reference. Also
   whether a trigger fires for an actor whose capsule touches it but whose axis does not.
2. **Order on the entering frame** (T3): does OnTrigger run on the same frame as
   OnTriggerEnter, before or after it?
3. **Where ActorZone and Projectile are stored** (T5): find a trigger the GECK shows with
   and without ActorZone and compare the records (FNVEdit).
4. ~~**`XRDS`** (L1): confirm it adds to the base radius.~~ Settled with interior frames
   (world.md §8); a REPCONN basement frame (base 16, `XRDS` 134–1,171) would make it
   starker, but its screenshot failed.
5. **Lights per object** (L2): does the original cap the number of lights on one object?
   An interior with many overlapping lights photographed at known spots would show it.
6. **Object fade distances** (C5): which setting turns bound size into a fade distance
   (the INI fade settings), and how objects pop in at the edge of the loaded cells.
7. **Portal culling in New Vegas** (C1): `MBC` on and off in a portalled interior, and a
   clutter object knocked through a portal (C2), to confirm the rules hold in New Vegas.
8. **Multibounds** (§3.2): which New Vegas references act as multibounds (`XMBP`, or a
   base), and whether exteriors use them.
9. **Encounter zones** (P1): a zone's level after leaving and coming back at a higher
   player level; whether respawned actors keep the first level; the zone's own data
   (ECZN `DATA`) against the tutorial's description.
10. **Mines in New Vegas**: trigger distance indoors and outdoors and the blast radius,
    against the Fallout 3 figures (100 / 160 / 192).
11. **Inaccessible doors**: which record field holds the flag, and what activation does.
12. **North marker in game**: whether it turns the interior local map and compass.
