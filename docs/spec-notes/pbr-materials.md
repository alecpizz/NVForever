# Authored PBR material maps (`textures\pbr\`)

NVForever's own file format for texture packs that give a surface physically based material
maps: occlusion, roughness and metallic, a normal map and a height map, beside the game's
diffuse texture (#2981). It is **NVForever's own format**: it describes no behaviour of the
original game, and no file the base game or its add-ons ship is known to lie under
`textures\pbr\` [hypothesis: not checked against the archives]. Every rule below is our
choice, marked **[design]**; facts about the texture formats cite their source
**[documented]**; what the engine does today cites its code.

Version 1. Pack authors may rely on the layout, member names, channels and scalar keys
below; a later version adds to them without changing their meaning.

## Decided (maintainer, 2026-11-20, #3104)

These choices are settled; pack authors and later versions can rely on them.

- [design, owner 2026-11-20, #3104] **Member names**: `<stem>_orm.dds`, `<stem>_n.dds`,
  `<stem>_p.dds` and `<stem>.toml`, with `<stem>_albedo.dds` reserved; the extension is not
  part of the stem, so `foo.dds` and `foo.tga` in one folder share one set (Layout, below).
- [design, owner 2026-11-20, #3104] **Normal map green points down**, the game's own
  convention, so a pack can start from the game's normal maps (Channels, below).
- [design, owner 2026-11-20, #3104] **DDS only**: members are DDS files; KTX2 is not read.
- [design, owner 2026-11-20, #3104] `GameData.vfs` stays a public field
  (`engine/nvf-assets/src/data.rs`); code that replaces the virtual file system goes
  through `GameData::set_vfs`, which drops the cached index of sets so it is rebuilt from
  the new one.

## What the engine implements at this stage

| Part | State |
|---|---|
| Path rules, member names, the index of sets a load order supplies | **implemented** (`crates/fnv-pbr`; `GameData::pbr_index` in `engine/nvf-assets/src/data.rs` builds it on first use and logs one line of counts; materials ask for it only while the Authored materials setting is on) |
| `fnvtool pbr list` (which sets a load order has, and which archive or folder supplies each file) | **implemented** (`crates/fnvtool/src/pbr_cmd.rs`, docs/cli-contract.md) |
| Scalar file reading (`<stem>.toml`) | **implemented**; read once per mount, by the first shape that asks (stage 3) [documented: `engine/nvf-assets/src/data.rs` `GameData::pbr_scalars`] |
| Reading members' pixels | `_orm` (linear) and `_n` (two-channel normal role, `TextureUsage::NormalTwoChannel`) **implemented** (stage 3); `_p` (linear, unsigned only) **implemented** (stage 4); `_albedo` stays reserved |
| Reading BC7 and BC4 files (DDS with the DX10 header, or legacy `BC4U` / `BC4S`) | stage 1 of #2981 (`crates/fnv-dds`) |
| Drawing a surface with its set (ORM, normal) | **implemented** in source (stage 3) [documented: `engine/nvf-assets/src/nif_spawn.rs` `shape_material`, `authored.rs` `retexture`]; the setting Authored materials (`authored-materials`, on by default, read at the start; `--graphics-only no-authored-materials`). Its check is the field test `pbr-authored-field` (below). A sample pack was drawn in the game with the setting off and on by the main session as of head `bebd7841a` (pictures not committed; they show the pack's own values, and no reference look exists to judge them by); those pictures are owed again after the fixes that followed that head |
| Height maps (`_p`, parallax) | **implemented** (stage 4, below: "Height maps"). The game's own height slot (#2979) is drawn too, and a set's `_p` replaces it (below: "The game's own height maps") |
| Height maps on the landscape's texture layers | **implemented** in source, off by default (#3322 stage 1, below: "Terrain layers: parallax"); seen running with the generated ground pack (test aid, 2026-11-22), not yet with a real pack |
| Metallic | read with the ORM; acts where the view has a reflection cube while the setting Reflections on authored materials is on (`authored-reflections`, on by default, read at the start; `--graphics-only no-authored-reflections`), **held at 0** otherwise (section "Reflections on authored materials") |
| A pack validator (`fnvtool pbr check`, `fnvtool pbr selftest`) | **implemented** (`crates/fnvtool/src/pbr_check.rs`, docs/cli-contract.md `fnv.pbr.check/1`) |

Sections marked "planned" describe how the engine will use a set once those stages land.
Wherever this note says a member "will be read" or "will be accepted", stage 3 does so for
`_orm` and `_n`, and stage 4 for `_p`.

## Layout

[design] Every file of a pack lives under `textures\pbr\`, in the same folders as the game
diffuse it belongs to. The set's **stem** is the diffuse's path with its extension removed
(whatever it is), placed under `textures\pbr\`:

| Diffuse the game draws | Stem |
|---|---|
| `textures\architecture\urban\sidewalkquad01.dds` | `textures\pbr\architecture\urban\sidewalkquad01` |
| `textures\clutter\food\tincan.tga` | `textures\pbr\clutter\food\tincan` |

A set's members are the stem plus one suffix:

| File | Role | Required |
|---|---|---|
| `<stem>_orm.dds` | red: ambient occlusion; green: perceptual roughness; blue: metallic | makes the set **authored** |
| `<stem>_n.dds` | tangent-space normal, two channels (X in red, Y in green), green pointing down like the game's normal maps [observed: docs/spec-notes/nif.md] | optional |
| `<stem>_p.dds` | height, brighter is higher | optional; alone it makes a **height-only** set |
| `<stem>.toml` | scalars (below) | optional |
| `<stem>_albedo.dds` | **reserved** for a later version: found and counted, never activates a set, will not be read in version 1 | — |

- [design] A set with `_orm` is authored. A set with `_p` and no `_orm` is valid: the game's
  own material with height only (when height maps land). A set with only `_n`, `.toml` or
  `_albedo` does nothing; the engine counts it as incomplete.
- [design] There is **no plain-name file**: `<stem>.dds` is not a member. In version 1 the
  base colour is always the game's own diffuse (or whatever diffuse replacer wins in the
  load order).
- [design] Paths are matched without regard to slash direction or ASCII case, as every
  game path is (`fnv_vfs::normalize`): only `A`-`Z` fold to `a`-`z`; other letters (accented
  or non-Latin) must match exactly. A diffuse path a mesh writes with a leading `data\` or an
  absolute folder is cut at `textures\` first, as for the diffuse itself
  (`fnv_vfs::texture_path`).
- [design] The extension is not part of the stem, so `foo.dds` and `foo.tga` in one folder
  share one set. That is accepted: a pack cannot give the two different sets.
- [design] The key is the diffuse **as the engine resolves it**: after record texture swaps,
  after the first-person twin is chosen, and after a console path (`\xbox\`) is replaced by
  the PC copy, which happens only when the PC diffuse exists (`resolved_path`,
  `engine/nvf-assets/src/texture.rs`). A set under `\pc\` therefore applies only where the PC
  diffuse is drawn.
- [design] **First person**: when the diffuse's file name starts with `1stperson` and there is
  no active set for it, the set of the same name without the prefix is used. One set
  serves a weapon in both views; a pack can still give the first-person texture its own.

### Reading a file name back: one suffix, once

[design] A file under `textures\pbr\` is assigned to a set by removing **exactly one**
suffix from the end of its file name (the last path component; folder names are never
peeled): `_orm.dds`, `_n.dds`, `_p.dds`, `_albedo.dds` or `.toml`, compared after ASCII case
folding. What is left must be a non-empty name; it is the stem. The suffix is never
removed twice:

| File under `textures\pbr\walls\` | Set (diffuse) | Member |
|---|---|---|
| `foo_n.dds` | `foo` | normal |
| `foo_n_orm.dds` | `foo_n` (a diffuse named `foo_n.dds`) | ORM |
| `foo_n_n.dds` | `foo_n` | normal |
| `foo.dds` | none: **unmatched** | — |
| `foo_orm.png`, `readme.txt` | none: unmatched | — |

Unmatched files are ignored and counted (`fnvtool pbr list` names them).

## Channels and colour spaces

[design] The colour space follows the member's **role**, not the DDS header: every member
of version 1 (`_orm`, `_n`, `_p`) will be read as linear data, even when a DX10 header names
an `_SRGB` format. Authors should still export them as linear (non-sRGB) so other tools show
them correctly.

- `_orm`: red occlusion (1 = unoccluded), green perceptual roughness (0 smooth, 1 rough),
  blue metallic (0 dielectric, 1 metal). The alpha channel will not be read.
- `_n`: X in red, Y in green, each 0..1 standing for -1..1; Z is rebuilt from them. Green
  follows the game's convention, +G pointing down the image [observed: docs/spec-notes/nif.md,
  "Normal maps", from the game's normal and height map pairs], which the engine follows by
  flipping Y for every NIF normal map (`flip_normal_map_y` in
  `engine/nvf-assets/src/nif_spawn.rs`), so a pack can start from the game's normal maps.
  A three-channel normal map (BC7, BC1, BC3) will be accepted and read as X, Y, Z
  (stage 3); its alpha will not be read.
- `_p`: one channel, red; brighter is higher.

## Compression and sizes

- Recommended formats: `_orm` **BC7** (or BC1 when its quality is enough), `_n` **BC5**
  (unsigned), `_p` **BC4** (unsigned). BC1 and BC3 will be accepted for `_orm` and `_n`
  from stage 3, and for `_p` when height maps land (stage 4); `_albedo` stays reserved.
- [design] `_p` must be **unsigned**: signed BC4 (`BC4_SNORM`, FourCC `BC4S`) is refused,
  by `fnvtool pbr check` (an error) and by the engine (one warning; the rest of an authored
  set still applies). A height runs from 0 (lowest) to 1 (highest); a signed sample below 0
  would read deeper than the deepest layer. Signed BC5 stays allowed for `_n`.
  [documented: Microsoft, "Texture Block Compression in Direct3D 11": BC4 stores one channel
  and BC5 two in 4x4-texel blocks of 8 and 16 bytes; BC7 stores RGB or RGBA in 16-byte
  blocks]
  https://learn.microsoft.com/en-us/windows/win32/direct3d11/texture-block-compression-in-direct3d-11
- BC7 files carry the DDS DX10 extension header [documented: Microsoft,
  "DDS_HEADER_DXT10"]. BC4 and BC5 files may carry it too, or a legacy header with the
  FourCC `BC4U` / `BC4S` (BC4) or `ATI2` / `BC5S` (BC5) [documented: Microsoft,
  "Programming Guide for DDS"; docs/spec-notes/dds.md, docs/spec-notes/dds-bc5.md]. The
  engine reads the DX10 header and the BC4 FourCCs from stage 1 of #2981 on.
- [design] The **top level's width and height must be multiples of 4**. The engine has no CPU
  decoder for BC7, so a file it cannot upload whole will be refused rather than decoded. A
  `_p` off the grid is refused by the engine too (stage 4: one warning naming the
  file and its size), though it could decode one, so the engine and `fnvtool pbr check`
  agree on it.
- [design] Ship **full mip chains**, down to 1x1. Smaller mip levels need not be multiples of 4.
- [design] Each member's size is its own: an ORM at 2048 beside a 1024 diffuse and a 4096
  normal map is fine. Nothing ties the sizes together.
- Before shipping, run `fnvtool pbr check <folder>` on the pack (the mod folder, its
  `textures` folder or the `pbr` folder). It reads each file's DDS headers and size and the
  scalar files: formats per member, sides that are multiples of 4, mip chains, cut-off or
  padded files, `_SRGB` codes, incomplete sets and unmatched files, and it estimates the
  pack's GPU memory; it exits non-zero on an error. It does not decode or judge the maps'
  content (whether the roughness or normals look right): view them in the engine.

## Scalars: `<stem>.toml`

A TOML file of plain keys, all optional:

| Key | Meaning | Range | Default |
|---|---|---|---|
| `parallax_depth` | depth of the height map's darkest point below the surface, in UV units (stage 4); 0 draws no height map | 0 to 0.08 | 0.03 (`fnv_pbr::DEFAULT_PARALLAX_DEPTH`) |
| `roughness_scale` | multiplies the ORM's roughness; the product is held to 0..1 when drawn, and to 0.089..1 for the highlight's shape (below) | 0 to 2 | 1.0 |
| `reflectance` | the highlight strength of non-metals; F0 = 0.16 x reflectance² (0 to 16 %) | 0 to 1 | 0.5 (F0 4 %) |
| `metal_gain` | how much brighter than the base colour a metal's reflectance is drawn, in a view with a reflection cube (section "Metal base colour") | 1 to 40 | 12 [design, owner 2026-11-21, #3237] |

```toml
# textures\pbr\walls\brick07.toml
roughness_scale = 0.9
reflectance = 0.35
metal_gain = 8
```

[design: banding and wall streaks in the main session's pictures from 0.06 on a 512 map]
`parallax_depth` stops at **0.08** (it went to 0.25 before 2026-11-20). On a tiled floor
with missing patches (set `offrmfloor01`, a 512 BC4 `_p` derived from the game's normal
map), 0.015 looked convincing; at 0.06 the occlusion method drew stair-step bands on every
patch edge, and the relief method, which removes the bands, drew vertical streaks on the
patch walls instead: texels stretched over a near-vertical wall of a map with nearly two
levels, a property of the map that no march fixes [observed: those pictures, 2026-11-20].
A deeper relief wants a map with gentle slopes and more texels, which the bound leaves to
a later look rather than allowing by default.

[design] Reading is lenient (`fnv_pbr::parse_scalars`): an integer counts as a number; an
unknown key, a table, or a value that is not a finite number is ignored with a warning; a
number outside its range is clamped into it with a warning (judged as the 32-bit float the
value is kept in, so an end written as it is, `0.08`, is inside); a file that is not UTF-8 or
not TOML is ignored whole with a warning. A UTF-8 byte order mark is accepted.
`normal_strength` is not a key: bake the strength into the normal map.

## What a set changes on a surface (stage 3)

[design] When the engine draws a surface whose diffuse has an authored set:

- **Switched off**: the shape's glossiness, the normal map's alpha as a highlight mask, and the
  shape's Specular flag. Roughness comes from the ORM (times `roughness_scale`), the highlight
  strength from `reflectance`. A surface the game drew without highlights therefore gets the
  default 4 % F0 once authored; `reflectance = 0` keeps it matte.
- **Replaced**: the normal map, when the set has `_n`; otherwise the shape's own normal map
  stays, without its alpha mask.
- **Kept**: the base colour (the diffuse), the glow map, alpha blending and alpha testing (from
  the diffuse's alpha, as before), the decal flag, and environment maps once the engine draws
  them (it does not yet: `engine/nvf-assets/src/nif_material.wesl` header).
- **Not covered in version 1**: actor skin, hair and FaceGen heads; terrain layers' `_orm`
  and `_n` (their `_p` is drawn as parallax: "Terrain layers: parallax" below); distant LOD;
  effects (unlit, additive, refraction, particles); the Pip-Boy and interface textures.

## How the engine lights a set

Maps tuned for other renderers need these notes:

- The diffuse light follows the engine's model of the original's: lights are summed in
  display values and the diffuse is multiplied by that sum raised to 2.2, not by a linear
  light sum (`engine/nvforever/src/lights/display_shader.rs`). That the original lights
  surfaces this way is itself a [hypothesis, fitted to the original's frames]
  (docs/spec-notes/world.md, "Light on surfaces in display values").
- Ambient occlusion (ORM red) darkens only the ambient part of that sum, not the sun or
  lamps (`display_shader.rs`, `diffuse_occlusion`).
- The highlight is GGX with the Fresnel factor held at F0 in world views (no brightening at
  grazing angles; `FRESNEL_EDIT`, `engine/nvforever/src/lights.rs`).
- Roughness is perceptual: the ORM's green is multiplied by `roughness_scale`, and the
  surface shader holds the product to 0..1 right after sampling
  (`engine/nvf-assets/src/nif_material.wesl`): a scale above 1 on a rough texel passed 1,
  and the environment term's lookup (`F_AB`, `pbr_functions.wesl`), which takes the value
  as it is, then left its range. [observed: 2026-11-20, the set `offrmfloor01` in the Vault
  21 gift shop with its roughness scaled 2 and more drew white patches before the cap and
  none after, pictures at scales 2, 2.5 and 3.] For the highlight's shape the product is
  further raised to at least 0.089 and then squared (Bevy's
  `perceptualRoughnessToRoughness`, `engine/vendor/bevy_pbr/src/render/pbr_lighting.wesl`),
  so a product below 0.089 draws a highlight as narrow as 0.089's. A raw green below 0.089
  can still end above that floor with a scale above 1.
- **Metallic** acts only where the surface has something to reflect: with the setting
  Reflections on authored materials on and a reflection cube on the view (the world camera
  where the mount has authored sets). Elsewhere, and with the setting off, it is **held at
  0**: a metal surface draws as a dielectric of its diffuse colour with the authored
  roughness, since without a reflection source it would draw black except for lamp
  highlights. See "Reflections on authored materials".

## Where a set does not apply, and checking one (stage 3)

What the engine's source does with a set, as written (not yet compared with a running game
beyond the field test below):

- [design] Which shapes take a set [documented: `engine/nvf-assets/src/authored.rs`
  `shape_eligible`, `key_eligible`; `engine/nvforever/src/actors.rs`]: lit NIF shapes with
  texture coordinates. Never: unlit, additive, multiplied or refracting shapes, particles,
  hair (the hair shader, and the hair model's shapes once an actor wears it), skin (the skin
  shader, and any shape whose diffuse is the actor's race body or hand texture or the head's
  face texture, from the moment that is known, tinted or not), landscape LOD
  (`meshes\landscape\lod\`), and diffuses under `textures\pipboy3000\` or
  `textures\interface\`. Unlit copies made later (the Pip-Boy's and the terminal's screens)
  drop any set.
- [design] The normal map [documented: `authored.rs` `apply`]: the set's `_n` when it has one
  and the drawn mesh has tangents; otherwise the game's own normal map, whose alpha is then
  no specular mask. Tangents are made for a shape whose first normal map is the `_n` when its
  material is made; where a later change of diffuse (a script's texture swap) or a copy of a
  drawn mesh would bring the first normal map to a mesh without them, the `_n` is not applied
  (logged once per set) and the rest of the set is.
- [design] A swapped diffuse takes its own set, or none, from the shape's legacy look; it
  never keeps the previous diffuse's maps [documented: `authored.rs` `retexture`]. A script's
  change of a shape's glossiness goes to the legacy look while a set is on it, so the set's
  roughness stays [documented: `engine/nvforever/src/nif_mirror.rs` `set_material`]. A
  specular colour the script set itself is drawn at once and kept as an override that a set
  put on later (a swap's) keeps drawing; the stored colour the mirror writes back with an
  edit of another value never scales a set's highlight [documented: `authored.rs`
  `set_legacy_specular_tint`, `apply`, `LegacyLook::scripted_tint`]. Several edits of one
  shape in one mirror update go to one private copy [documented: `nif_mirror.rs`,
  `SceneEdit::Material`].
- The setting Authored materials (Graphics > Detail and Distance) is read once at the start.
  Off, the material path builds no index, loads no file under `textures\pbr\` and keeps no
  legacy look [documented: `authored.rs` `find`, `engine/nvf-assets/src/nif_spawn.rs`].
- A set's scalar file is read and its warnings logged once per mount: by the first shape
  that asks, on whichever thread; a new mount of the files reads it afresh [documented:
  `engine/nvf-assets/src/data.rs` `GameData::pbr_scalars`].
- The smoke scenarios `pbr-authored-field` and `pbr-authored-field-off`
  (`NVF_PLAY_TEST_PBR_FIELD=1`, `engine/nvf-assets/src/pbr_field.rs`) are the check of the
  drawn result: generated sets through the real material path on five panels (plain; lit,
  with `_orm` and a two-channel `_n` decoded on a worker; dark, as lit but for the ORM's
  red; reference, lit's `_orm` with the same normal as a three-channel game map; flat,
  `_orm` without a normal map), two GPU readbacks judged by mean luminance. [observed: on
  `7e2de4be1`, the main session's runs, two readbacks each, identical: on, plain 103, lit 67,
  dark 38, reference 67, flat 103; off, plain, lit, dark and flat 103, reference 60.] The
  comparator's thresholds were set from those readbacks with margins [engineering policy:
  occlusion at least 15, lit and reference within 3, reference at least 15 darker than flat
  (on) or plain (off), the off panels within 2]. [observed: the main session's smoke run
  at `c512cd3c8`, the head with the final review's fixes bar the last change to the swap's
  source lookup: `pbr-authored-field` and `pbr-authored-field-off` pass with those
  thresholds, and `rorschach-cards`.]
- Calibration aids, off by default, read at the start [documented: `authored.rs` `PbrView`,
  `metallic_debug`; `engine/nvf-assets/src/nif_material.wesl`]: `NVF_PBR_VIEW` =
  `roughness`, `metallic`, `occlusion`, `normal`, `authored` or (stage 4) `height`
  draws that input in place of the lit colour of forward-drawn lit NIF surfaces;
  `reflection` draws, on an authored
  surface, what a white perfect mirror would write there (the view's reflection cube at
  level 0 along the mirror direction, times the cube's intensity and the exposure, then
  tone-mapped as the picture is), so its scale can be judged against the sky in the same
  frame, and every other surface grey as `authored` does (authored ones orange where the view
  has no cube or reflections are off; only the opted-in fragment reads the cube).
  Refracting shapes return before the
  view; additive draws add the view's colour by their alpha and multiplied draws stay
  neutral, so effects fade out of it; depth and coverage are unchanged.
  `NVF_PBR_METALLIC=1` builds authored materials metallic by the ORM's blue channel and keeps
  a metal's ambient highlight, a preview of a later metallic stage [design]; retired while
  Reflections on authored materials is on (below), and working as before while it is off.
- On the app's exit the log has one line `authored materials summary: ...` (or
  `authored materials summary: off (setting)`): counts of build events, not of visible
  shapes: material builds that applied an ORM set, the distinct ORM set stems seen at those
  builds (with stage 4 also the builds that applied a height-only set's height map),
  builds that skipped a set for one of the recorded reasons (other misses are not counted),
  `_orm` load failures (at a build or a swap alike), role copies made legacy, and calls of
  `retexture` on a shape with a saved legacy look [documented: `authored.rs` `Stats`, `report`].
  Rebuilt or discarded materials count again.

## Height maps (stage 4)

What the branch's source does with a set's `_p`, as written; not compared with any running
game beyond the field test below [documented: `engine/nvf-assets/src/authored.rs`
`find`, `height_gate`, `apply_height`, `take_off`, `retexture`, `recheck_height`;
`engine/nvf-assets/src/nif_spawn.rs` `shape_material_inner`].

- [design] **Which sets.** An authored set's `_p` and a **height-only** set (`_p` without
  `_orm`) both give a height map. A height-only set changes nothing else on the material:
  the game's roughness, highlight, normal map and specular mask stay, the authored mark stays
  clear (so a script's glossiness and specular colour still apply), and its `_n` and its other
  scalars are ignored. The first-person rule of "Layout" holds: a `1stperson` texture's own
  height-only set wins, and the plain texture's set is not used behind it.
- [design] **Which shapes.** Those that may take a set at all (above), and of them only
  shapes that are **opaque** (no alpha test, no blend, no blended cut-out, no hair; an
  alpha test is allowed under the development switch `NVF_PBR_PARALLAX_MASK`, below),
  **single-sided**, **not decals**, with texture coordinates and **tangents on the drawn
  mesh**. Tangents are made for a shape whose first need for them is its `_p`; a copy of a
  drawn mesh (an actor part's kept mesh) and a swap onto a drawn mesh use the tangents it has
  (recorded in the legacy look), and draw no height map without them. Copies that stop
  qualifying drop it: an effect shader's holes (which make an opaque copy `Mask(0)`), the
  fades' blended copies (nvforever `effect_shaders.rs`, nvf-play `fade.rs`) and a cloak's
  refracting copy (`nif_spawn::refraction_material`, used by nvforever `stealth_field.rs`
  for the player and actors alike); unlit copies (Pip-Boy, terminal glass and glare) and
  shapes made the game's own for a role found later (skin, face, hair model) drop it with
  the rest of the set. The engine tracks the overlay on the material
  (`NifSurface::height_overlay`) and touches the parallax fields only where it put one on. A mount with only height-only sets still runs
  the actors' role exclusions (`mount_has_sets` counts every active set).
- [design] **How it is drawn.** As Bevy's parallax: `StandardMaterial::depth_map` = the
  `_p`, `parallax_depth_scale` = `parallax_depth` (default 0.03), the **relief method with 4
  refinement steps** (`PARALLAX_METHOD`: a march through at most **8 layers**,
  `MAX_PARALLAX_LAYERS`, Bevy's default being 16, then a binary search between the last two
  layers, halving the step 4 times, one read each); the development switches below change
  the method and the layers for a run. Why relief: in the main session's pictures of
  2026-11-20 (a tiled floor with missing patches, set `offrmfloor01`, a 512 BC4 `_p`), the
  occlusion method at 8 layers showed the first layering on long patch edges at a low angle
  from depth 0.03 and stair-step bands on every patch edge at 0.06; 16 and 32 layers made
  the bands finer without removing them, while `relief:4` at 8 layers removed them at 0.03
  [observed: those pictures; the frame cost of either is not measured cleanly]. Relief is a
  pipeline variant (`RELIEF_MAPPING`, Bevy's `StandardMaterialKey`), so height-mapped
  materials get pipelines of their own. A depth of 0 means no height map: the `_p` is not
  even loaded. The
  base colour, alpha, glow, ORM and the forward-sampled normal are read at the shifted
  coordinates; the geometry, its depth and its shadow do not move.
- [design] **Height, not depth.** The file stores height (white high); Bevy's march reads
  depth (white deepest). The engine inverts once, in the vendored
  `engine/vendor/bevy_pbr/src/render/parallax_mapping.wesl` `sample_depth_map` (both its
  bindless and its ordinary sample, `1 - r`; engine/vendor/README.md). No other material in
  the engine sets a depth map, but for the game's own height maps (below), which are height
  too.
- [design] **The game's own height maps** (#2979; docs/spec-notes/nif.md §6 "Parallax height
  maps", which keeps the format, the asset-survey counts and what is not known of the
  original apart from this). What NVForever implements: a NIF shape whose
  BSShaderPPLightingProperty sets BSShaderFlags bit 11 and names a texture in slot 3 draws
  it as a height map through the same march, with the same method and layers (and their
  switches), **on by default** at the game depth 0.015. That default and the method are
  **provisional** [hypothesis]: they stand until observer check #265 (how the original's
  relief looks) and a frame-cost measurement on a parallax-heavy view; nothing here says the
  original draws them this way (`authored::GAME_PARALLAX_DEPTH`, switch
  `NVF_GAME_PARALLAX_DEPTH=<0..0.08>`, 0 draws none and loads none). It takes the same
  shapes as a set's height map (the gate above: by default **opaque only**; alpha-tested
  shapes only under `NVF_PBR_PARALLAX_MASK=1`), with tangents on the **drawn** mesh: a copy
  made from an actor part's kept mesh for a texture swap or a record's texture set
  (`Tangents::Keep`) takes a height map, the game's or a set's, only when that mesh already
  has tangents, since the actor draws the original mesh and tangents made on the copy never
  reach it. The legacy look records the drawn mesh's tangents too, so a later swap
  (`retexture`) puts no `_n` or `_p` on such a copy either. **Known limit:** an actor's armour
  whose drawn mesh starts without tangents (no normal map in its own file) and gains a
  slot-3 map only through a record's texture set (TXST TX04) stays flat, as before #2979;
  drawing it would need the actor to draw the copy with its new tangents. Parallax at
  grazing views is bounded by the vendored march's grazing clamp (the step's divisor held
  at a view steepness of at least 0.15; engine/vendor/README.md). A shape whose tangents
  could not be made, or that may not draw the map, does not load the file.
  - [design] **Intentional changes to authored materials too** (#2979, in the shared
    vendored march; engine/vendor/README.md): the grazing clamp also bounds an authored
    `_p`'s step (an unflagged floor seen at |Vt.z| = 0.05 now steps a third as far as
    before), kept for both sources so no view gives runaway or non-finite coordinates;
    the sampled height is held to 0..1 before the inversion (a signed texture cannot leave
    the march's layers); and the occlusion method (`NVF_PBR_PARALLAX_METHOD=occlusion`)
    interpolates only when the march brackets a hit, keeping the last layer otherwise. It applies **whatever the
  Authored materials setting**, since it is the game's material, not a set's
  (`authored::apply_game_height`). Precedence: it is put on before the legacy look is taken,
  so it is that look's parallax baseline (`LegacyLook::parallax`); a set's `_p` (authored or
  height-only) is put over it as an overlay and **wins**; a set without `_p` leaves it
  drawn; taking a set off (a swap's `retexture`, `restore`, `make_legacy`) puts it back.
  Later copies that stop qualifying (fades, cloaks, and any copy with an effect shader's
  holes set-up for its whole life, 255 included, since the holes then open by animation
  alone) drop it as they drop an overlay (`recheck_height` acts on any depth map;
  `height_gate`); unlit copies clear it. The format is not judged: the maps are read as they
  load, red channel [hypothesis: the shipped maps are grey DXT1]. `NVF_PBR_VIEW=height`
  shows them like a set's. The exit line `game height maps summary: ...` (printed whatever
  the setting) gives the depth or "off", then the builds that still draw the game's map,
  those where a set's `_p` replaced it, those left flat, those that could not load it, and
  later copies that dropped one; smoke scenarios `game-parallax-vault34` (authored sets off,
  switches pinned, a non-zero drawn count) and its control `game-parallax-vault34-off`
  (depth 0, nothing drawn).
- [design] **Signed `_p` is refused** (one warning per file; an authored set keeps its other
  maps), as `fnvtool pbr check` refuses it ("Compression and sizes"); so is a `_p` whose file
  is in any format other than those the validator allows for it (BC4 unsigned, DXT1/BC1,
  DXT5/BC3), judged by the file's own format as decoded, so a DXT3 file is refused whether it
  went up as BC2 blocks or, off the 4-texel grid, decoded to `Rgba8Unorm`; and a `_p` that went
  up in a format a height map does not take, or whose format the engine could not tell
  [documented: `authored.rs` `height_source_ok`, `height_format_ok`]. The warning names the
  file's own format (`BC5_SNORM`, `DXT3`). A `_p` whose top level has a side that is not a
  multiple of 4 is refused as well, with one warning naming the file and its size, as the
  validator refuses it (`not-block-aligned`), although the engine could decode an allowed
  format off the grid on the CPU [documented: `authored.rs` `height_size_refusal`]. The size
  is the file's own, noted when it is decoded (`note_source`); a `_p` whose source the
  engine did not note is judged by its uploaded format only.
- [design] **Development switches** (aids for choosing the method, the layers and the
  alpha-tested rule by picture and by frame time without a rebuild; off by default, read once
  at the start, a line in the log when one is set; text that is garbage or out of range is
  refused with one warning and the default is kept) [documented: `authored.rs`
  `parallax_layers`, `parallax_method`, `parallax_mask`, `parse_parallax_*`]:
  - `NVF_PBR_PARALLAX_LAYERS=<n>`, a whole number from 2 to 64: the most layers the march
    takes, in place of 8. At a head-on view the march still takes one layer (Bevy mixes the
    count from the most down to 1 by the view's steepness).
  - `NVF_PBR_PARALLAX_METHOD=occlusion | relief:<steps>`, steps from 1 to 8: Bevy's
    `ParallaxMappingMethod`. `relief:<steps>` (the default is `relief:4`) runs a binary
    search between the last two layers after the march, halving the step `steps` times, one
    read each; `occlusion` instead reads once more and interpolates between them, the
    cheaper method kept for comparing pictures and frame times (vendored
    `parallax_mapping.wesl` `parallaxed_uv`).
  - `NVF_PBR_PARALLAX_MASK=1`: alpha-tested shapes (`AlphaMode::Mask`) may take a height
    map as opaque ones do (they gain tangents for it the same way); blended shapes stay
    refused. Every depth-only pass reads the cut-out's alpha at the **unshifted**
    coordinates: the NIF depth and motion prepass (`nif_prepass.wesl` `test_fragment`), Bevy's
    own prepass fragment (`prepass_sample_color_and_alpha_discard`) and the shadow passes;
    none of them can march, since a depth-only prepass carries no normal or tangent
    (vendored `prepass/io.wesl`: `world_normal`, `world_tangent` only with a normal or deferred
    prepass). The main pass, which samples the base colour at the shifted coordinates, would
    test the shifted texel's alpha and cut a different outline; with a depth prepass it tests
    depth for equality (`nif_material.rs` `depth_equal`), so a fragment the prepass kept and
    the main pass cut would leave a hole showing the clear colour, and one the prepass cut is
    never drawn. Under the switch the main pass therefore reads the alpha again at the
    unshifted coordinates (material × vertex × texel, as the prepass does) and tests that
    (nif_material.wesl, shader def `NVF_PBR_PARALLAX_MASK`): every pass cuts the same
    outline, and the colour, normal and ORM inside it are the shifted texels'. The outline
    does not move with the relief: near the cut, the shifted colour may come from texels the
    cut leaves transparent (drawn opaque). A multisampled view whose prepass writes the soft
    edge (`PREPASS_COVERAGE`) does no alpha test in the main pass at all and agrees already.
- Checking it: `NVF_PBR_VIEW=height` draws the height as stored (white high, the original
  height, not the inverted depth), read at the unshifted coordinates, and dark blue on shapes
  without a depth map. The exit summary line adds the builds that applied a height-only
  set's height map, the builds that applied any height map, the swaps to a loaded height-only
  set (a match with a depth of 0 or a `_p` that did not load counts as a swap to the legacy
  look) and of them those that put its map on, the height maps not put on at a build or swap
  by reason (alpha-tested and blended counted apart), `_p` files that did not load or were refused (signed, in no height map's
  format or a side not a multiple of 4), and later copies that dropped one.
  The smoke scenario `pbr-authored-field-height` (the same field, `NVF_PLAY_TEST_PBR_FIELD=1`)
  draws two more panels tilted 50 degrees about Y: `height` (a height-only set whose `_p` is
  high above and low below, depth 0.08, the scalar range's end; 0.1 before the range was
  cut) and its control without one; it measures a narrow
  stripe's centre in both bands of both panels and wants the low band's stripe moved towards
  decreasing mesh U by the prediction from the shader's geometry (−depth × sin tilt × 128 =
  −7.84 pixels; −9.81 at 0.1) within 2.5 pixels (the relief search stops about 1 % short of
  the flat band's depth, about 0.1 pixel), and the high band's and the control's within 1.5 of their
  place; with the setting off (`pbr-authored-field-off`) neither panel has a depth map and
  neither stripe moves more than 1.5. The height panels report on their own lines
  (`pbr field height: ...`: the shifts, the brightest value and the ground level per
  readback, a verdict per readback, a final PASS), so they never fail the five panels'
  verdict. [observed, field only, on `b4660af00`, the main session's first GPU run with the
  setting on: high +0.00 px, low −9.76 px against −9.81; stripe centres 64.00/54.24 (height)
  and 64.00/64.00 (control); the sRGB 230 stripe then read 255 and failed the clip check, so
  the stripe and ground were darkened to sRGB 180 on 90; that run used depth 0.1 and
  occlusion mapping.] [observed, two readbacks at `ca0e10c9e` with depth 0.08 and four
  relief steps after eight layers: high +0.13 px, low −7.86 px against −7.84; the control
  stayed centred at 64.00/64.00 and both readbacks passed.] The thresholds are an
  engineering policy set from the first readback with margins.

### Height maps: guidance for pack authors (stage 4)

[design, from the main session's pictures of 2026-11-20 named below]

- **Height maps are for tiling surfaces**: floors, walls, paving, where the texture repeats
  across the shape and every shifted read lands on more of the same texture.
- **An atlas sheet** (one texture holding several pieces, each mesh using its own island)
  takes a height map only with **padding between the islands**, and the filler around an
  island must **never drop to height 0**: extend each island's edge height outwards into
  the filler. The shift grows at grazing view angles and can exceed `parallax_depth`. It
  can cross an island's border and read the filler. A filler at 0 draws as a deep
  trench at the piece's edge. [observed: with `NVF_PBR_PARALLAX_MASK=1`, a street's
  alpha-tested sidewalk pieces drawn from an atlas sheet showed thin dark lines across a
  slab and broken pixels at a piece's end; the relief itself looked right. That is the
  sheet's missing padding, not the alpha test.]
- **Depth**: start low. On a 512 floor map with nearly two levels (patches missing from
  tiles), 0.015 looked convincing and 0.03 (the default) showed the method's limits only at
  low angles; 0.06 drew stair-step bands (occlusion) or vertical streaks on the patch walls
  (relief) [observed: set `offrmfloor01`, 2026-11-20], hence the bound of 0.08. Gentle
  slopes take a deeper relief than walls between two flat levels; a map with near-vertical
  walls stretches a few texels over the whole wall at any depth.
- **Alpha-tested shapes** take no height map by default (`NVF_PBR_PARALLAX_MASK=1` is a
  development switch); blended, double-sided and decal shapes never do. A `_p` for a
  texture used only by such shapes is loaded for nothing.

### Known limits (stage 4)

[documented: the files named; none measured]

- **The normal prepass.** Bevy's normal prepass samples the normal map at the unshifted
  coordinates, and a single-sampled view with `NORMAL_PREPASS` may reuse that normal in the
  main pass (`LOAD_PREPASS_NORMALS`, vendored `pbr_fragment.wesl`, `pbr_prepass_functions.wesl`).
  NVForever's views never ask for a normal or deferred prepass: the cameras carry only
  `DepthPrepass` and, with an upscaler, `MotionVectorPrepass` (`engine/nvforever/src/main.rs`,
  `graphics.rs`, `scope_view.rs`, `fsr.rs`, `upscaler.rs`), so a NIF surface's main pass
  always samples its normal itself, at the shifted coordinates. A view that ever adds
  `NormalPrepass` (Bevy's SSAO requires it) would light a height-mapped surface with the
  unshifted normal; nothing guards against it.
- **Top mip level on every read.** The march reads the depth map with
  `textureSampleLevel(..., 0.0)` on every step (Bevy's choice, for the DX12 compiler), so
  distant height-mapped surfaces read the full-size map: a cost and a source of shimmer that
  mip filtering does not remove. The frame-time cost of up to 8 layers on close, grazing,
  screen-filling surfaces is not measured.
- **Double-sided shapes** are left out: the march takes the incoming tangent frame without
  turning it for back faces and divides by `|Vt.z|`.
- **Cut-outs and blends** are left out: the depth prepass, the shadow passes and the holes
  test read alpha at the unshifted coordinates, so a cut-out's edge would not follow the
  shifted texels. A fading or holed copy therefore loses its relief while it fades. Cut-outs
  may take one under `NVF_PBR_PARALLAX_MASK` (above), their edge kept unshifted in every pass;
  the switch stays off by default [design: under it a street's 79 sidewalk builds took their
  height map and looked right but for an atlas sheet's filler (guidance above), and the
  outline that does not move with the relief has not been judged on more shapes].
- **Atlas sheets without padding** read their filler at an island's border (guidance
  above); nothing in the engine detects or guards against it.
- The occlusion debug view rereads the ORM at the unshifted coordinates; the FaceGen tint,
  the Pip-Boy screen and the holes test also read at unshifted coordinates (none of them
  takes a height map today).
- **Unmeasured:** the cost on a close first-person weapon and on grazing, screen-filling
  surfaces, and the relief search's cost over the occlusion method's. One reading on a
  noisy machine (a floor filling a 1080p frame, occlusion at 8 layers, a fast desktop GPU)
  put the march between about +0.2 ms and +1.5 ms on a 2.2 ms frame [observed: the main
  session, 2026-11-20; not a clean measurement]. The picture comparison of 8, 16 and 32
  layers and of `relief:4` is in "How it is drawn" above.
- **What the field proves:** the signed displacement of two flat height bands (the low band
  moves by the predicted amount the predicted way, the high band stays). It does not test
  the march on varying heights (slopes, steps inside the band, self-occlusion), where an
  implementation that only got the two flat ends right would still pass.

### Known limits (stage 3)

From the source reviews of 2026-11-19 and 2026-11-20 [documented: the files named]:

- **A script's material edit during a fade (fixed in #3105).** Previously, the property
  mirror (`engine/nvforever/src/nif_mirror.rs`) copied the worn fade copy and the fade's
  end discarded the edit. Under #3105's single `nvf_play::fade::Fading` owner,
  `mutate_shape_material` resolves `nvf_play::fade::persistent_material`, edits the mesh's
  own private persistent copy, and calls `queue_retarget` to rebuild the see-through copy
  in place, so the edit is drawn during the fade and kept after it ends.
- **A placed shape the mirror gave a private material, then swapped.** The swap matches
  shapes by their original material, and a placed shape without a shape name no longer
  matches once the mirror replaced it, so its old diffuse stays drawn with its authored
  fields (set, mark, metallic). This predates stage 3 (`texture_swap.rs`, `nif_mirror.rs`).

What holds for a texture swap and overlapping fades (#3105):

- **Single owner of `SetActorAlpha` and presence fades (#3105).** `nvf_play::fade::Fading`
  owns one see-through copy per mesh for presence fades (`fade.rs`) and `SetActorAlpha`
  (`effect_shaders.rs`) alike, scaling the mesh's own persistent material's opacity and
  cut-out threshold by `presence * actor_alpha` and restoring `Fading::original()` when
  the last effect ends. Both `SwapTexture` (`texture_swap.rs`) and scripted material edits
  (`nif_mirror.rs`) resolve `nvf_play::fade::persistent_material` and retarget `Fading` in
  place, so all eight overlap/swap start-and-end orders (named and model-backed unnamed
  shapes alike, including two ends in one update and either command-flush ordering) restore
  the mesh's own or swapped material opaque [documented: tests in `fade.rs`,
  `texture_swap.rs`, `effect_shaders.rs`, `nif_mirror.rs`].

## Reflections on authored materials

[design] Surfaces shaded from an authored set reflect a small cube of the world camera's
surroundings, blurred by their roughness, and the `_orm`'s metallic channel acts. NVForever's
own rendering: the original draws no ambient reflection. Legacy materials, terrain, trees,
grass and water get no direct contribution from the cube (the gate below keeps it from their
shaders) [documented: `engine/nvforever/src/lights/reflection_gate.rs`]; that is parity of
the cube's direct contribution, not a pixel comparison with the build before the feature,
which has not been made (water's own screen trace, for one, shows the authored pixels it
reflects as they are now drawn). The setting Reflections on authored
materials (`authored-reflections`, Graphics > Detail and Distance after Authored materials,
on in both looks [design, owner 2026-11-21, #3237], `--graphics-only no-authored-reflections`) is read once at the start
(`engine/nvforever/src/main.rs`, `engine/nvf-assets/src/reflection_cube.rs` `set_enabled`);
it acts only with Authored materials on, and changes no picture without a pack.

- **Who reflects.** [design; documented: the files named] Bevy adds a view's environment map light (its `EnvironmentMapLight`
  cube) to every lit shader of that view. NVForever patches bevy_pbr's `pbr_functions` at run
  time so both environment blocks compile only for a fragment that opts in with the shader
  def `NVF_PBR_REFLECT`, which only the NIF surface's main pass sets while the setting is on
  (`engine/nvforever/src/lights/reflection_gate.rs`, `engine/nvf-assets/src/nif_material.rs`).
  Inside it the reflection is weighted by the authored mark, the ORM's occlusion (red) and,
  under the mesh terrain blend, the mesh's share (1 - w); every other NIF material weighs 0
  and reads no cube (`nif_material.wesl`). If the patch cannot be made, no cube is ever
  attached (a log line says so).
- **Which view.** [design] Only the world camera has a cube, and only when the setting and Authored
  materials are on, the mount has at least one authored set (`authored::mount_has_sets`)
  and the patch is installed (`engine/nvforever/src/reflection_sky.rs` `decide`). Bevy's
  component goes on once the first fill is on the GPU. The water mirror, the scope, the
  minigame views and the terminal have none.
- **Metallic.** [design] With the setting on, authored materials are built with metallic 1, so the
  ORM's blue acts, but the NIF fragment holds it at 0 in any view without a cube of its own
  on the GPU: the shader def `ENVIRONMENT_MAP` that Bevy sets from the view, and the view's
  `view_cubemap_index` at 0 or above, since Bevy also sets the def for a view that only sees
  a light-probe volume and leaves the index at -1 there [documented:
  `engine/vendor/bevy_pbr/src/light_probe/mod.rs`; `nif_material.wesl` `nvf_view_has_cube`,
  which the reflection weight, the metal base lift and the mirror debug view ask too]. So
  those views, and the world camera's first frames, shade an authored metal as with the
  setting off. No view of the game has a light-probe volume today [documented: no light
  probe is spawned in the engine's source, per the code review of 2026-11-20]; the reflection field has no probe-only camera (a probe volume would
  also change what its cube camera draws inside it), so the composition test alone covers it.
  `NVF_PBR_VIEW=metallic` still shows the ORM's blue.
- **The ambient highlight.** [design] Bevy's ambient highlight stays removed in full, also from a
  metal: the cube takes its place. A metal with a black cube and no lamp is black.
- **The cube.** [design] A specular cube of 32x32 RGBA16F faces with six levels and a black 1x1
  diffuse cube (the ambient light stays the game's flat colour), written straight into the
  texture on the GPU after each fill (`reflection_cube.rs`).
- **What it holds** [design] (`reflection_sky.rs` `SkyContent`), per world direction and level:
  - outdoors, above the horizon, the sky dome as it is drawn in that direction (clouds, the
    sun disc and the glare aside): the weather's horizon, sky-lower and sky-upper colours at
    the hour (blended across a weather fade) mixed as the dome mixes them (`sky.rs`
    `dome_color`), by the dome's own weights for that height (`atmosphere.nif`'s vertex
    colours averaged in 16 steps of the sine of elevation, `DomeProfile`), times the sky's
    display scale (`SkyState::sky_scale`, the image space's LUM Ramp No Tex, as sky.wesl
    scales the dome);
  - outdoors, below the horizon, lit ground: an assumed albedo, a neutral warm grey of
    display value (0.33, 0.30, 0.25), linear about (0.089, 0.073, 0.051), under the light
    falling on level ground, the ambient light plus the directional light times the sine of
    its elevation (the light as the sky aims it: the moon's, with its share, at night, and
    the ambient alone where neither is above the horizon);
  - a broad glow toward the directional light (the sun, or the moon at night), in its colour
    and strength, peak 0.5 of it at level 0, added to the sky;
  - indoors, the cell's ambient colour above, ground of the same albedo under the ambient
    alone below (the cell's directional light is left out), and the cell's fog colour as a
    band at the horizon (a Gaussian of half-width 0.2 in sine of elevation), as the water's
    indoor sky takes the fog colour;
  - each level is the same function with a wider kernel, not a filter of the level above:
    each hemisphere mixes toward its mean by level / 5, the edge between sky and ground
    widens from 0.05 to 0.5 (sine of elevation), and the glow's power, 16 at level 0, halves
    per level with its total kept. Bevy reads level = perceptual roughness x 5.
  [design] Every constant is NVForever's choice, chosen by reasoning; none was tuned against
  pictures.
- **Units.** [design; the renderer's units documented in the files named] Linear texels and intensity `light_unit()` (Bevy's default exposure's inverse),
  so a white perfect mirror writes texel x intensity x exposure = the texel into the HDR
  target at the default exposure. Each texel is what the thing it stands for writes there
  before tone mapping, so the tone map and the grade treat mirror and original alike:
  - the sky (`reflection_sky.rs` `sky_texel`): sky.wesl writes the dome's linear colour times
    `params.x` (= `SkyState::sky_scale`, `engine/nvforever/src/sky.rs` `SkyState::light`
    and `sky_scale`) through `tony_inverse` (`engine/nvforever/src/sky.wesl`), which is the
    identity while the main view shows display values clipped (the default,
    `engine/nvforever/src/lights.rs` `GAME_CLIP`) and the inverse of the Tony McMapface table
    under `NVF_TONEMAP=tony`; the cube holds the same value, so a mirror shows the dome as
    drawn;
  - lit ground (`ground_texel`): the fork's display-value sum writes a diffuse surface as
    min(diffuse x (ambient + direct)^2.2, 1) (`engine/nvforever/src/lights/display_shader.rs`,
    the edit before "Total light"), with the ambient as the view's (display value x 0.9) and
    the light as `nvf_directional_display` gives it for an upward normal;
  - indoors (`texel`): no sky is drawn, so the ambient-matched identity stays: a display
    value v with the ambient's share becomes v x max(v, 1e-6)^1.2, so a direction coloured
    like the ambient reflects the converted ambient, about A^2.2.
  The two rules conflict outdoors: an ambient-coloured sky is (linear colour x sky scale) by
  the sky rule and (0.9 x display value)^2.2 by the ambient rule, and outdoors the sky rule
  wins. The glow is the light's display colour times its strength in display units, taken
  to linear light by the ambient rule's conversion and added to the sky in the sky's units
  (exact with the clip; with Tony McMapface the glow goes through the inverse curve on its
  own, an approximation). Sources add in linear light. A check of the scale by eye:
  `NVF_PBR_VIEW=reflection` beside the sky in one frame.
- **Metal base colour** (next section): a metal's reflection is tinted by its base colour,
  lifted by the set's metal gain in a view with a cube.
- **When it is written.** [design] A change of room (indoors against outdoors, another
  interior cell) or a new dome profile refills at once; otherwise the inputs are checked at
  most four times a second (real time) and the cube rewritten only when one moved past a
  step from the last fill: a colour by 1/128 in display value, the light's direction by 3
  degrees [documented: `reflection_sky.rs` `produce`, `room_changed`, `Inputs::moved`]. One log line at
  the first fill (`reflection sky: first fill ...`, with the level-0 zenith, horizon and
  nadir texels) and one at exit (`reflection sky summary: N fills (... outdoors, ...
  indoors), M checks skipped as unchanged`, or `off (reason)`).
- **Not there.** [documented: `reflection_sky.rs`] No clouds, no sun or moon disc, no stars; no scene objects (nothing near the
  camera is drawn into the cube); one cube for the whole view, so no room's own reflection
  and no parallax; no glass or see-through reflection; the game's own environment maps
  (the shapes with the environment-map flag) are not drawn. The lightning flash and the
  exterior fog are not in the cube; indoors the cell's fog colour is only the band at the
  horizon (above).
- **Checking it.** The smoke scenarios `pbr-reflect-field` and `pbr-reflect-field-off`
  (`NVF_PLAY_TEST_PBR_REFLECT_FIELD=1`, `engine/nvf-assets/src/pbr_field/reflect.rs`, the
  setting pinned with `--graphics-only`): thirteen generated panels drawn by a camera of the
  field's own with a cube of a different colour per world face and by one without, under the
  production kind of light (world-view ambient, Bevy's default exposure, MSAA). On: tilted
  smooth metals show the faces their reflections point at, rougher metals lie nearer the
  cube's mean, the legacy and zero-reflectance controls and every panel of the camera
  without a cube are unchanged; a dark bronze metal at the default gain is clearly brighter
  than the same at gain 1 with the same hue. Off: the cube stays on the camera and changes no
  panel. The world camera's cube never replaces the field's: the field's cameras are its own.
  [observed: the main session's reflection smoke runs at `8a9abf4a9` passed, with RGB
  readbacks, before the third row; the comparator's thresholds are set from reasoning and
  the third row's are not yet confirmed against a readback]
  `authored-reflections-sky` and `authored-reflections-off` (the test aid
  `NVF_PLAY_TEST_PBR_MOUNT_PACK=1` writes the field's generated sets into a fresh folder of
  the run under an explicit, non-empty `NVFOREVER_HOME` (`pbr-mount-pack\run-*`, every file
  created new, the folder removed again on a failure) and mounts it over every other loose
  layer for that run only, so the mount has authored sets; no mods folder is written
  [documented: `pbr_field/reflect.rs` `seed_mount_pack`, `fnv_vfs::Vfs::add_loose_root`]): with the setting on, the world camera's cube is
  filled from Goodsprings' sky and attached; off, none is made. Log lines only.
- **Composition.** `engine/nvf-shader-tests/tests/reflection_gate.rs` composes the patched
  `pbr_functions` for the NIF fragment with and without the opt-in and for Bevy's own
  fragment, terrain, grass and tree leaves in a view with a cube: only the opted-in NIF
  fragment reads the cube, and only it compiles the metal base colour's lift.

## Metal base colour

[design] In the physically based model a metal has no diffuse colour: what it shows is its
reflection, tinted by its base colour, which for a metal is its reflectance (steel about
linear 0.56). The game's diffuse is not that: it is a painted look with the light already in
it, and on metal far too dark to be a reflectance [observed: 2026-11-20, the main session's
measurement of the first-person minigun's metal texels, median linear luminance about 0.02,
mean 0.035]. Drawn as it is, an authored metal reflects almost nothing: the minigun at noon
read as near black with reflections on [observed: the main session's pictures of 2026-11-20
at `8a9abf4a9`, then with the experiment's gains at `8e1f196c6`].

- **The gain.** In a view with a reflection cube (the opted-in NIF main pass, where the
  ORM's metallic acts) an authored material's base colour for lighting is
  mix(base, lifted, metallic), with the effective metallic (after the hold in views without
  a cube and the mesh terrain blend's mix) and lifted = base x min(gain, 1 / brightest
  channel), never below base: one gain on all three channels, limited so the brightest
  channel reaches 1, so the hue is kept (`nif_material.wesl` `nvf_metal_base`; its CPU
  mirror `authored.rs` `lifted_base`). The glow keeps the unlifted colour, alpha is
  unchanged, and a dielectric (metallic 0) is untouched. Views without a cube, and the
  setting Reflections on authored materials off, draw the base colour as it is.
- **Per set.** The gain is the scalar file's `metal_gain` (1 to 40), by default 12
  [design, owner 2026-11-21, #3237: the maintainer accepted 12 as a design value]. That
  default was chosen from pictures of **one steel
  weapon**, the first-person minigun at noon in Goodsprings, which read near black at gain 1,
  dark bronze at 6, worn steel at 12 and brighter steel at 20 to 30 [observed: the main
  session's pictures of 2026-11-20 at `8e1f196c6`, with the experiment's per-channel clip];
  it is not a calibration of steel in general. A metal whose diffuse is already bright is held by the hue-preserving cap (its
  brightest channel reaches 1) well before the gain, so the gain matters for dark diffuses. It reaches the shader per material in the NIF
  surface uniform's spare `terrain.y` lane (`NifSurface::metal_gain`, set by `authored.rs`
  `apply`, 0 on every material without a set, on restore and on unlit copies; a swap takes
  the new set's).
- **No `_albedo`.** The base colour stays the game's diffuse: `_albedo` stays reserved
  [design, owner 2026-11-21, #3237, keeping the decision of 2026-11-20, #3104]; giving it a
  meaning is a separate proposal.
- **Tuning by picture.** `NVF_PBR_METAL_GAIN=<1..40>` (read once at the start, logged)
  replaces every set's gain for the run, whatever its scalar file says; a value
  that is no number or out of range is warned about once and ignored (`authored.rs`
  `metal_gain_override`). Not a `NVF_PLAY_TEST_*` aid.
- **Checking it.** The reflection field's third row (above) and the unit tests of
  `authored.rs` (the lane through builds, swaps, restore and unlit copies, the gain's
  defaults, the lift's rule).

## Reflections indoors: the analytic room

[design] Indoors, where a lamp is in reach or a room box is known, the world camera's cube
holds a lit box around the camera instead of stage 1's ambient, ground and fog band, so an
authored metal in a room shows the room's lamp-lit walls, floor and ceiling and where the
lamps are. NVForever's own rendering, nothing of the original's (whose look has no ambient
reflection); computed on the CPU from what the renderer already lights the room with
(`engine/nvf-assets/src/reflection_room.rs`, gathered by `engine/nvforever/src/reflection_sky.rs`).

- **The box**, in this order: the room marker the camera is in (the first room box holding
  it, the test `rooms.rs` uses for the camera's room, only for the interior the markers were
  read for); else, in play mode, six physics rays from the eye (east, west, north, south, up,
  down; past actors, moving objects and what a character walks through; at least five must
  hit within 2048 units, a miss putting that wall 256 units away); else a default box 512 x
  512 units and 320 high, its floor 120 units under the eye. No half extent is under 16.
- **The lamps**: the cell's placed lights that are on (lights.rs `SessionLight`, not hidden),
  with a colour, within 4096 units of the eye and whose sphere of light reaches the box,
  the nearest 16 to the eye. Each brings its display colour times its steady brightness
  (a flickering or pulsing light's full intensity), its radius and, for a spot light, its
  cone as Bevy hands it to the shader.
- **Lighting a wall.** Toward each direction the eye sees the point where the ray leaves
  the box: a floor (assumed display albedo 0.33, 0.30, 0.25, stage 1's ground), a wall (0.40,
  0.38, 0.35) or a ceiling (0.45, 0.45, 0.43). It is lit as the display-value sum lights a
  diffuse surface: light = the cell's ambient + its directional light x N.L + for each lamp
  colour x brightness x the game's attenuation saturate(1 - d^2 / radius^2) x N.L (x the
  cone), and the surface writes the linear colour of min(albedo's display value x light, 1)
  (sRGB curve; world.md §8 "Dark values of the sum"), the units of stage 1's ground.
  The renderer's lamps cast no shadows, so neither do these; a lamp behind a wall lights it
  as on screen. The cell's directional light lights the walls at full (its shadows left
  out). The cell's fog mixes walls toward its colour by distance as the fog does indoors.
- **A lamp seen directly**: a lamp inside the box (within 32 units of the wall behind it)
  adds a small lobe toward itself, its converted colour x 1 at the peak, falling as cos^256
  of the angle (half at about 4 degrees).
- **Levels.** Level 0 is the scene at each texel's centre. Each level above is the
  solid-angle-weighted box average of the level below; where Bevy's lobe for that level is
  wider than a texel (levels 2 to 5), it is instead the level-2 average convolved with the
  kernel max(c, 0) x ((1 + c) / 2)^(p / 2), p = 2 / a^2 - 2 for GGX roughness a =
  (level / 5)^2: a cosine lobe at the last level.
- **Outdoors near lamps.** Below the horizon, stage 1's lit ground also takes what placed
  lamps add to level ground under the eye (found by a ray down in play mode, else 120 units
  under it), for lamps whose sphere reaches the ground and comes within 512 units of the eye,
  with their lobes; nothing changes without such a lamp. By day the lamps add little beside
  the sun.
- **Falls back** to stage 1's interior content exactly when no lamp reaches the box and the
  box is the default one.
- **When it is written.** With a room or ground lamps, besides stage 1's steps: the eye moved
  64 units from the last fill, a box moved 32, another set of lamps, a lamp's colour past
  1/128 or its place or radius past 8. Indoors against outdoors, another interior cell,
  another room marker and a new sky dome profile refill at once, without the quarter-second
  wait. A room fill takes 6,144 level-0 texels x the lamps plus the filter.
- **Where the fill runs** [design, owner 2026-11-22, #3238: the maintainer asked for the
  fill off the main thread, since on the main thread it took up to 4.89 ms (9 lamps, Doc
  Mitchell's house) up to four times a second while walking indoors]. The main thread
  only gathers the inputs into owned data: the sky's state, the directional light, the
  placed lamps, the room marker or the six rays (they need the physics world), and compares
  them with the last ones. The fill itself (the content and every level's texels, a pure
  function of those inputs) runs on Bevy's async compute pool, every fill alike (stage 1's
  sky too). The producer looks at it each frame; the cube keeps the last finished fill
  until the new one is done, and the main thread then only moves the finished bytes into
  the cube, which the render world uploads as before. At most one fill runs: inputs that
  move past a step while it runs wait as one request (newer inputs replace it). A fill
  that finishes is applied even with a request waiting, since it is still newer than what
  the cube shows, and the waiting one starts in the same frame: no work is thrown away and
  a stream of changes cannot keep the cube from updating. The trade-off: the cube shows a
  change one fill later than on the main thread (the fill's few milliseconds, then the next
  frame's upload), two when a fill was already running. Each fill is applied only to the
  camera and cube it was asked for: a fill whose camera or cube was replaced meanwhile is
  discarded, and the replacement asks its own; a light inherited from an older cube is
  removed as soon as the camera or cube changes (the light of any cube a fill went into,
  while the camera holds another cube or none). The hemisphere means the first-fill line
  quotes (a pass over the room's level-0 texels) are taken only for fills asked while
  the camera's current cube was not the last one filled: its first fill, any queued behind
  it, and the first fill after a camera or cube comes back from an earlier replacement.
  Until a camera's first fill is done it has no cube and no environment light, as before
  the first upload; on entering an interior the outdoor cube stays until the room's first
  fill is done
  [documented: `reflection_sky.rs` `produce`, `FillQueue`, `fill`].
- **Log lines.** `reflection sky: room fill in interior <cell>: box from <room marker
  <id>|rays|default> (<x> x <y> x <height> game units, x y height), <n> lamps, level 0 toward
  the nearest lamp [...], filled in <ms> ms on a worker (main thread: gather <ms> ms, apply
  <ms> ms)` at the first room fill in each interior, or
  `reflection sky: interior <cell>: no lamp in reach and no room box, so stage 1's interior
  content`; the exit summary adds the room fills by box source, the ground-lamp fills, the
  slowest fill on the worker, the refills by reason (mode, cell, room, box, moved, lamps,
  light, profile), the main thread's slowest gather and slowest apply and the fills
  discarded for a replaced camera or cube (`...; main thread slowest gather <ms> ms,
  slowest apply <ms> ms; <n> fills for a replaced camera or cube discarded`). The gather
  time starts after the room-marker search that keys the wait, which runs every frame.
- **Not there.** No captured scene: no objects, textures, signs or bulbs' shapes, no
  actors; uniform walls of assumed albedo; one box per fill, centred on the camera, so it
  is exact only near the camera (no parallax), and rooms joined without a marker share the
  rays' or the default box; no shadows from lamps or the directional light; no flicker; no
  probes. Every constant is a guess, chosen by reasoning and not yet judged in pictures.
- **Checking it.** Unit tests in `reflection_room.rs` and `reflection_sky.rs` (an empty
  room is the ambient on each surface, a lamp brightens its wall and its lobe points at it,
  a lamp out of reach adds nothing, the levels approach the mean, the fallback is stage 1's
  content exactly, the refill steps; the worker's fill writes the bytes the main thread
  wrote, the last finished fill stays while one runs, the newest waiting inputs win, and a
  fill finishing while newer inputs wait is applied with the newer fill started; a fill
  for a replaced camera or cube is discarded and the inherited light removed at once). The smoke scenario `authored-reflections-room` in Doc
  Mitchell's house (`GSDocMitchellHouse`, the generated sets of `authored-reflections-sky`):
  the first room fill line and the summary. Log lines only.

## Terrain layers: parallax (#3322, stage 1)

What the source does with ground height maps, as written; seen running only with the
generated ground pack (below, "Checking it without a pack"), not with a real pack
[documented: `engine/nvforever/src/terrain.rs` "Parallax", `TerrainCtx::folded_material`,
`sync_parallax`, `parallax_def`; `engine/nvforever/src/terrain.wesl` `parallax_shift`;
`engine/nvf-assets/src/texture.rs` `TextureCache::authored_height`, `authored.rs`
`find_height`]. The original draws no relief on its landscape: on the ground outside
Goodsprings at noon, gravel painted over dirt beside cracked dirt, seen at pitch 30 and 70
degrees while strafing in 4-unit steps, the painted stones and cracks moved as one rigid
pattern with the ground, no highlight or shadow slid across them and no bump hid texture,
even at a grazing view [observed: the maintainer's run of observer check 268, 2026-11-22, by
eye; judged from stills, not measured]. Terrain parallax is therefore an enhancement of
NVForever's own: it acts only with an authored ground pack (an authored set's `_p`, below)
and is off by default and in both presets [design].

- [design] **Source.** A landscape layer's height map is the `_p` of the set the index finds
  for the layer's diffuse path (the LTEX's texture set's diffuse, resolved as a shape's
  diffuse is: `\xbox\` to `\pc\` where the PC file exists), an authored set's or a
  height-only set's, at its scalars' `parallax_depth` (the default where unset). Only the
  `_p` is loaded: a ground texture's `_orm` and `_n` are not used on the landscape in this
  stage. The same refusals as on shapes apply (signed, wrong format, sides not a multiple of
  4, depth 0), with the same one warning per file. Loaded with the quadrant's folded
  material whenever the run's Authored materials state is on, whether the switch is on or
  not, so the switch flips while the game runs. The data offers no other source:
  `FalloutNV.esm` has 89 LTEX records, 88 of them with a TNAM, naming 70 distinct texture
  sets, and none of those names a `TX04` (height) map; of the ESM's texture sets, 11 carry a
  `TX04` (six `SuburbanRubble01_p` for NIF shapes, five impact decals) [documented:
  FalloutNV.esm, counted with `fnvtool esm decode` on the test install, 2026-11-21]. An
  authored set's `_p` is therefore the landscape's only height source.
- [design] **The base marches too** (#3846; flat in the first stage). Much of the ground is
  base: in WastelandNV's 17,510 quadrants with painted layers it makes up 59% of the blend on
  average and more than half of it at 59% of the vertices; the other 48,042 have no painted
  layer, most of them cells without a base record either, outside the map (world.md §3)
  [observed: the LAND records of FalloutNV.esm and its DLC masters, `ATXT` and `VTXT` as
  stored, read with `fnv_world::land` as `crates/fnv-world/examples/land_layers.rs` reads
  them]. Its height comes from where a layer's does and adds its share x its own depth x (1 -
  height) to the one march. A quadrant without painted layers is drawn by the folded material
  with no layers when its base has an authored height map, or no active set and a BC3 diffuse
  (whose alpha may be one, whatever the switch, so the draws do not change with it), and marches
  with its neighbours; every other one keeps the separate landscape material. The game's own
  ground always does: of the 78 distinct diffuses the base game's and the DLCs' LTEX records
  name, 75 are in the archives, all DXT1 [observed: their DDS headers]. The folded draw also
  darkens burnt ground (docs/design/grass-fire.md), which the separate one does not.
  Authored base height maps are loaded with the material, as the layers' are, whatever the
  switch. No quadrant has more than six painted layers in those records,
  so the layers drawn on their own (flat) are the edge stitching's (world.md §3), each painted
  on an edge row.
- [design] **One march.** Six layers in one draw rule out a march per layer. The folded
  draw marches once, steep parallax with the relief refinement, as NIF shapes march (the
  vendored `parallaxed_uv`'s relief method: its steps, view vector, tangent frame, signs and
  grazing clamp; "The march" below), over one blended depth: the sum over the layers of share x the layer's own depth x (1 - height),
  the share the layer's ordered share (painted opacity, laid
  over the earlier layers as the ordinary blend lays them; the diffuse's alpha has no part in
  it, world.md §3 "Landscape layers: the diffuse's alpha"), and the base's likewise; one without
  a height map adds no depth (flat at the surface). Then every texture of the quadrant, the
  base's and the layers', diffuse and normal map, is read at the one moved coordinate.
  Flat at the surface rather than mid-height so that the shift falls to nothing as a
  height-mapped layer's share does.
- [design] **Weights, once.** The base's and the layers' weights are worked out once, at the
  plain coordinate: the ordered shares of painted opacity, and with
  Terrain height blending on its weights from the texels there (its heights and references
  as its drawn path takes them; without it no texel is read for them). Both the march and the
  drawn blend use them: a layer the height blend leaves out of a pixel does not move what is
  drawn there, and where the shift is nothing the blend is the one without parallax. The
  cheaper of the consistent rules (heights at every march step would add up to twelve reads a
  step); the cost is that where the height blend lets a layer through does not move with the
  relief.
- [design] **Filtering.** Every drawn read (the base's diffuse and normal map, each layer's)
  is made at the moved coordinate with the plain coordinate's screen gradients (one unturned
  tap through the anti-tiling helpers, `parallax_tap`), so the mip level and anisotropy do
  not follow the shift's jumps between neighbouring pixels (which implicit derivatives of the
  moved coordinate would, at relief and hole edges, even where a pixel's own shift is
  nothing). The surface is set up as anti-tiling sets it up (terrain_pbr.wesl), the
  occlusion (SSAO's multibounce) from the base's colour before the layers are folded in, as
  Bevy's set-up takes it on the path without parallax.
- [design] **The ray.** The march follows the view's own ray (Bevy's `calculate_view`: one
  direction in an orthographic view, as the minimap's camera); the fade goes by the
  distance from the camera.
- [design] **Edges.** Where the shares and heights agree on both sides of a quadrant edge
  (the painted opacities of one LAND grid, the same textures), the march agrees too: the
  depth sum is the same (each layer brings its own depth, an unpainted layer adds nothing)
  and every quadrant marches on the same step grid (next item), so the same narrow ridge is
  hit or stepped over on both sides alike (the test
  `a_layer_moves_as_far_whatever_else_its_quadrant_holds`). Not covered: two quadrants whose
  painted opacities differ along their shared edge, and a layer drawn on its own (below).
  With height blending on, its weights (worked out from brightness at the plain coordinate,
  "Weights, once" above) are the march's too: a layer the height blend leaves out of a pixel
  gives that pixel no displacement (a later stage may let a real height map stand in for
  brightness in the height blend itself).
- [design] **Depth and steps.** Each layer keeps its own set's depth (held to the scalar
  range's end); the march's scale is that end, 0.08 (`PARALLAX_SCALE`), the same for every
  quadrant, each layer's depth taken over it (`parallax_scales`). A grid from each
  quadrant's deepest layer, as first written, stepped a 0.02 layer beside an unpainted 0.08
  one four times as coarsely as alone (0.00625 against 0.025 texture-coordinate units a
  step at the same view). At most 16 layers (`PARALLAX_LAYERS`), fewer as the view
  steepens; a shallow layer takes fewer of them (a 0.02 layer about a quarter), the relief
  search refining between them [design: the price of one grid; to judge in pictures with a
  pack]. The depth is in texture-coordinate units, as on NIF shapes: one unit is one texture
  repeat, 256 world units on the landscape, so a layer at 0.08 sinks by about 20 world units
  and, seen 20 degrees above the ground, moves 0.08 / tan 20 degrees = 0.22 of a repeat
  (about 56 texels of a 256 map, 225 of a 1024 texture). The height maps are read at the
  level their screen footprint asks for (not the top level, as the shapes' march reads),
  with the base texture's sampler.
- [design] **The march** (round 2 of #3322, after the main session's pictures of the
  generated ground pack, 2026-11-22: vertical streaks, doubled and stair-stepped stripes,
  smeared texture and seams with the switch on). Round 1 marched at most 8 layers and
  refined with the occlusion method (one linear interpolation between the last two layers,
  parallax occlusion mapping: Tatarchuk, "Dynamic Parallax Occlusion Mapping with
  Approximate Soft Shadows", I3D 2006), with the step divided by the view's steepness
  unbounded. Over the ground pack's sharp ridges (a step from low to high, nothing between)
  the interpolation puts a pixel's hit between the two layers' guesses rather than at the
  ridge's wall, so each ridge showed once per layer reached (the doubled, stair-stepped
  bands); at 8 layers a step at 20 degrees (0.039 of a repeat) was longer than a ridge is
  wide (8/256 = 0.031), so rays also stepped over ridges; and toward the horizon the
  unbounded divisor threw the samples up to several repeats away (streaks; at 3 degrees a
  sunk layer moved 1.5 repeats). The march now is the shapes' relief method
  (`PARALLAX_RELIEF_STEPS`): after the linear search a binary search of 4 halvings between
  the last layer above the surface and the first below it, so the hit ends within a
  thirty-second of a step of where the ray meets the surface (Policarpo, Oliveira and Comba,
  "Real-Time Relief Mapping on Arbitrary Polygonal Surfaces", I3D 2005); a pixel whose plain
  place is already at the surface is not moved; the step's divisor stops at a steepness of
  0.15 (`PARALLAX_GRAZING`, the shapes' grazing clamp, #2979), so the full-depth shift stays
  under about 6.7 x the depth; and 16 layers (Bevy's default count), at which no ridge of
  the pack's width is stepped over from about 12 degrees up. Checked on a CPU copy of the
  march (terrain.rs tests `the_march_moves_a_sunk_layer_by_its_depth_over_the_views_slope`,
  `the_march_is_bounded_at_grazing_views`,
  `the_march_meets_the_ground_packs_ridges_where_the_ray_does`: over the pack's ridges every
  one of 512 start points across two ridge periods ends where its ray first meets the
  surface at 45 to 12 degrees, where round 1's march misplaced 332 of them at 20 degrees).
  Its frame cost (up to about twice round 1's reads at a 20-degree view) is not measured.
  Ruled out in that review, from the code: the tangent frame and signs (east +u, Bevy's
  bitangent -v, the same as the shapes' and as Bevy's tangent generator gives,
  `tangents_match_the_generator`), per-layer tiling (every layer, the base and the height
  maps are read at the one coordinate, one repeat per 256 units), and the drawn reads'
  filtering (the plain place's gradients, "Filtering" above). What the generated pack still
  shows with a correct march [design expectation, to confirm in pictures]: its ridges are
  walls 20 units high and 8 wide every 64 with nothing between, and only the textures'
  place moves (the landscape keeps its own normal map, which knows nothing of the ridges),
  so a wall's face toward the camera shows the ridge top's strip of texture drawn out down
  the wall (streaks along the view over much of the ground at 20 degrees), and a ridge
  running along the view is a seam where the texture beside it moved and its top did not.
  A real pack's smoother height map with a matching normal map is what the look is to be
  judged on.
- [design] **Fade.** Full strength up to 1024 units from the camera, none from 2048
  (`PARALLAX_FADE`): inside the camera's cell and the near half of the next, well inside
  the loaded cells. None head-on (the view vector's tangent part is zero).
- [design] **What stays unshifted.** The depth prepass and the shadows (the main pass alone
  gets `TERRAIN_PARALLAX`), the collision, the mesh terrain blend's surface bake and detail,
  the distant landscape LOD, the separate landscape materials: a quadrant without folded
  layers, every layer where the GPU has no bindless textures (`TerrainMaterials::folds`),
  and the layers past the folded ones when the texture repeat does not divide the cell
  (none at the game's 256). A layer drawn on its own over a shifted folded base does not
  follow the base's shift: a seam that only those configurations show.
- [design] **Not with anti-tiling.** While Terrain anti-tiling is on, a quadrant draws no
  parallax (its pipeline key leaves it off): anti-tiling reads every texture at two or three
  offset and turned places taken from the world, and a march at the plain coordinate would
  measure other texels than the ones drawn. Sharing the taps would multiply every march
  step's reads by the taps' count; left for a later stage.
- [design] **The switch.** Settings row Terrain parallax (`terrain-parallax`, Graphics ›
  Detail and Distance), off by default and in both presets until its cost is measured
  (whether a look turns it on is the maintainer's call). In the game its availability follows
  the run's Authored materials state, not that row's saved value: free while the run has
  them (also after the row is turned off for the next start: the height maps stay loaded),
  greyed "After a restart" while the row is on but the run started without them, greyed
  with the NEEDS words while both are off; the launcher greys it by the saved row, for the
  next start. `--graphics-only terrain-parallax`. A folded
  material without a height map keeps its old pipeline with the switch on and is not
  touched when the switch flips, so without a ground pack nothing changes, the mesh terrain
  blend's captures included (smoke `terrain-parallax-no-pack`). With a pack, the flips edit
  only the materials with height maps, noted for that frame (`ParallaxEdits`), and the mesh
  terrain blend passes over a material's edits only when they are those alone and what the
  capture is made from compares equal (terrain_surface.rs `captured_inputs_unchanged`: the
  bake copies, compared whole, and the detail's resolution; the bake copy drops the
  switch). Any other edit remakes the capture as before, even one that leaves its inputs
  equal: a height-blending or anti-tiling edit in the same frame, one after a while with the
  blend off, a generic one (the tests `the_capture_refresh_compares_its_inputs`,
  `an_edit_after_a_blend_off_interval_still_remakes_the_capture`,
  `a_generic_edit_remakes_the_capture_even_with_equal_inputs`).
- [design] **Heights from a ground pack's diffuse alpha** (#3486; asked for after jojokengly's
  survey there). Ground packs for the original's terrain parallax shaders keep the height map
  in the landscape diffuse's alpha, as DXT5 (Sandstone Desert: 64 of the 67 LTEX diffuses it
  replaces; Tale of Two Landscapes 70 of 72 [observed: jojokengly, #3486]), with the specular mask left
  in the `_n`'s alpha. This is a reader of packs that exist, no part of NVForever's own format:
  nothing here is promised to pack authors, and it can be taken out without touching the
  authored path. The rules:
  - *An authored set always wins.* Where the index knows an active set for the diffuse
    (authored or height-only; an incomplete one is no set here as elsewhere), the set alone
    decides (its `_p`, or none), and the alpha is not looked at.
  - *One height source.* The alpha becomes a BC4 texture at load, its blocks copied without
    decoding (a DXT5 block's alpha half is laid out as a BC4 block [documented: Microsoft,
    "Texture Block Compression in Direct3D 11"]), mips included, and goes into the layer's
    height slot. The shader, the march, the weights and the edges are those above; only the
    depth differs: 0.03 for every pack (`ALPHA_HEIGHT_DEPTH`; no scalar file sets it).
  - *Which textures.* A folded layer's or base's diffuse that is DXT5 with sides in multiples of 4 and
    whose alpha varies by at least 16 of 255 on its largest level of at most 256 texels a side.
    The diffuses FalloutNV.esm's LTEX records name are opaque DXT1 in the base game's archives
    [observed: jojokengly, #3486; world.md §3], so nothing is read from them. Other layouts with alpha
    (DXT3, BC7, uncompressed) are left out and stay flat, one log line for the first. An alpha
    that means something else would be read wrongly [hypothesis: none seen].
  - *Only while Terrain parallax is on* (and the run's Authored materials state is): the
    textures are made when the switch is on, so a player who leaves it off pays nothing. Switched
    on in a game, the loaded materials take theirs over some frames
    (`terrain::sync_alpha_heights`: each file is read on the main thread, a few a frame);
    switched off, they let go at once and the images leave graphics memory with the texture
    cache's usual grace (five minutes, sooner when memory runs short). On, each such layer
    costs half a byte a texel of graphics memory and a third more for the mips (a 2048 square
    one 2.7 MiB), and its file is read once more (not when it was read ahead).
  - *Not read:* a loose `textures\landscape\<name>_p.dds` beside the diffuse (most belong to
    rock and rubble meshes [observed: jojokengly, #3486]).
  - *Log:* `terrain: first folded material with heights from diffuse alphas: n of m layers`,
    the count of materials and textures at each doubling, and at each switch how many of both. Smoke: `terrain-alpha-height`.
- [design] **Checking it without a pack.** (`NVF_PLAY_TEST_PBR_GROUND_PACK=alpha` adds a flat
  diffuse with a checkerboard alpha for each texture, for #3789: world.md §3.) No data set has
  ground height maps, so two test
  aids stand in for a pack. `NVF_PLAY_TEST_PBR_GROUND_PACK=1`
  (`engine/nvf-assets/src/pbr_field/ground.rs`, the list from nvforever terrain.rs
  `ground_pack_diffuses`) writes a height-only set for every landscape texture the load
  order names (each LTEX's texture set's diffuse, and the default ground texture) into a
  fresh folder of the run under an explicit `NVFOREVER_HOME`, mounted over every other loose
  layer for that run only, as the reflections' mount pack is: a `_p` (unsigned BC4, 256
  square, a full mip chain; low almost everywhere, crossed by a grid of raised ridges, from a
  formula: no game pixel is read) and a scalar file at depth 0.08, nothing else.
  A plugin's texture path is data: a stem with a parent step, a dot component or a colon gets
  no set, and the writer checks every path before its first write.
  `NVF_PLAY_TEST_TERRAIN_PARALLAX_AB=<seconds>[:flip|hold][:view|grazing|head-on]` (nvforever
  `terrain_parallax_ab.rs`) takes three screenshots with the switch off, on, off (or left
  alone), each after the pipelines have settled, and logs how the ground's texture in a small
  box at the centre of the view moved: the best whole-pixel translation from the first to the
  second shot, the differences between all three, and a verdict (`moved`, `still`, `bare`,
  `unsettled`, ...). With a pose it first finds its own spot near the player: the nearest
  landscape vertex where the folded layers with height maps make up nearly all of what shows
  (terrain.rs `CellCover`: the cell's stitched layers in drawing order, the folded material's
  height-map bits, the layers past the folded ones drawn over them), with the physics' landscape under it and a grazing line of sight to it, and
  stands the player there. Before the first shot it logs what the centre's physics ray meets
  and the share there the march would move; under half is `bare`, so a view on a quadrant's
  base, on layers drawn over the folded ones or on a placed object never passes as "still".
  What the scenarios claim [design]: at the grazing pose, `changed` (or `moved`, when one
  translation explains it): a change the switch alone makes (the two shots with it off agree)
  that decorrelates the texture at the centre: the normalised cross-correlation (mean
  brightness and contrast taken out, so a brightening or a contrast change alone does not
  count) of A with B and of B with C at most 0.7, A with C at least 0.98. Not the direction: relief over the generated ridges warps and
  stretches the texture rather than sliding it, so the best-fit translation is logged as
  information only. Head-on, `still` (one rule): no fitted translation at all at the view's
  centre, a change of at most 0.3 of the texture's spread, A-C within its limits; not an unchanged picture, since the measured box
  reaches about 2 degrees off the vertical, where the march at depth 0.08 still moves a sunk
  texel by a few pixels, radially, and the marched relief changes what shows at ridge edges.
  `terrain-parallax-ground-pack-off` (the switch off, the pack mounted) wants no
  `TERRAIN_PARALLAX` pipeline and both poses still. [observed, the first run on b76835100,
  looking down Goodsprings' main street from the start: no shift anywhere on screen; the
  street turned out not to be a spot the stage-1 march reaches. observed, the run on
  cc1ee98ea on the aid's spot (-71552, 3200) in cell (-18, 0), shifted share 0.88 to 0.92
  under the centre: the ridge grid in strong relief with the switch on and none with it off;
  grazing A-B 12.29, A-C 0.10, B-C 12.29 levels on a contrast of 14.1, translation (0, 0);
  head-on A-B 5.02, A-C 1.02 on 32.5; held off 0.06 / 0.10 and 1.65 / 1.02. The run on
  555ace098 repeated them; its saved shots' correlations A-B / A-C: grazing 0.231 / 1.000,
  head-on 0.945 / 0.998, held off 1.000 / 1.000 and 0.991 / 0.998.] The limits are an
  engineering policy set from those numbers with margins (the grazing change would pass at
  three times its correlation, the head-on one at about twice its change):
  `terrain_parallax_ab.rs`. The cover under the centre is an estimate (the meshes'
  interpolation of the painted opacities on the stored triangles,
  layers the renderer leaves out skipped), so its limit (half) leaves a margin; the spot
  itself is chosen at vertices, where no interpolation enters.
- **Not done in stage 1** (owed): a passing run of the two ground-pack scenarios; the
  frame-time cost with and without the switch at a ground-heavy and a layer-heavy place;
  before/after pictures with a ground pack (the readback aid saves its shots). Still without
  height maps: the separate landscape materials (GPUs without bindless textures) and the layers
  drawn on their own over the folded draw. (The base's own: #3846, above.)

## Load order: per file, so ship complete sets

[design] Each file resolves on its own through the virtual file system, like every other
texture: plugin archives in load order, then `Data`'s loose files, then NVForever's mods
folder or a Mod Organizer 2 profile's mods (docs/design/mod-folders.md). Two packs can
therefore mix, one's `_orm` with another's `_n`. **Ship complete sets**, and use
`fnvtool pbr list` to see which pack supplies each file.

What is true of archives today (`Vfs::archive_order_in`, `Vfs::archive_files_layered`,
`crates/fnv-vfs/src/lib.rs`):

- Loose files always load, from `Data`, the mods folder or a Mod Organizer 2 mod.
- A pack in a `.bsa` is mounted only when the archive belongs to an **active plugin** by
  name (`<plugin>.bsa`, `<plugin> - <anything>.bsa`, or its numbered pieces), lying in
  `Data`, in NVForever's mods folder or in a Mod Organizer 2 mod; when the archive list
  names it (`[Archive] SArchiveList` in NVForever.ini, for an archive in `Data`; or a Mod
  Organizer 2 profile's ticked archives, in any of its layers); or when it is `Update.bsa`,
  which is mounted after the base list (from `Data`, or under Mod Organizer 2 from the
  highest layer that has it) unless the base list names it, which keeps it at that place,
  or the `NVF_ARCHIVE_ORDER` override sets the whole order without it. A plugin that cannot
  be read or lacks a master is left out, and the archives it owns by name with it; an
  archive list that names one of those archives still mounts it. A lone `.bsa` that none
  of these covers is not mounted.

## Log line and listing

The first time the index is asked for, the engine logs one line, e.g.
`authored materials: 12 sets under textures\pbr\ (11 with _orm, 1 height only; files: 11 _orm,
9 _n, 3 _p, 2 .toml, 0 _albedo reserved), 1 incomplete sets and 2 unmatched files ignored`.
`fnvtool pbr list --data <Data> [plugins…] [--mods <folder> | --mo2 <instance>]` prints
every set with its state and, per member, the archive or loose folder that wins
(`fnv.pbr.listing/1`, docs/cli-contract.md).
