# The bounce-light solver (`engine/nvf-rtgi`, #3009)

The plan is a crate that works out the light that an interior's lamps throw back from its
surfaces, as a small 3D texture, on the CPU and without Bevy. It is built in parts, each with
tests on scenes it makes itself. **Built: part 1, the ray scene, part 2, the light gathered at
points of the air, part 3, the volume, and part 4, the fit of its constants.** Nothing in the game
solves anything yet and there is no setting; of the crate the game has the volume and its read alone
([rtgi-volume](rtgi-volume.md)). It ships no data.

The direction now is a live solve on the GPU from the first frame of a visit, with this solver as its
reference ([below](#for-another-tracer-the-live-solve)). "The whole" is the earlier plan, for the CPU
alone, and is kept for its parts 1 to 4, which both use.

## The whole (the earlier CPU plan)

The bounced light is to take the place of a part of the flat ambient, fitted so that the mean
brightness of a room stays what it was. Interiors only, off by default, one bounce, no ray tracing hardware. The
parts, in order:

1. **The ray scene** (built): the triangles of a cell and what a ray meets.
2. **The light** (built): the engine's light sum at a place, the bounce gathered around a point in
   the air, and the grid of points that can be reached.
3. **The solve** (built): the points packed as a texture, and its reading on the CPU.
4. **The fit** (built): how much of the ambient goes and how strong the bounce in its place is.
5. The shader that reads such a texture, with nothing drawn by default.
6. The collector: a room's geometry, textures and lamps from the running game
   ([rtgi-collect](rtgi-collect.md); built: the reader of meshes and textures).
7. The job: a solve on entry, the fade from stock light, and what happens when there is none.
8. Light that changes during a visit, and the night.
9. The setting.

Parts 1 to 4 change nothing in the game. This note grows with them.

## The ray scene

Input is plain data: a cell's meshes, instances, materials and textures (RGBA8 with mips).
`prepare` puts all triangles into one soup in world space and builds a bounding volume tree over
it (binned surface area heuristic, the halves built in parallel on at most as many threads as it
is told; the same tree from any number of them).

- A ray against a triangle is the watertight test of Woop, Benthin and Wald in single precision,
  the edges again in double precision where one is 0: no ray slips between two triangles that
  share an edge [documented: "Watertight Ray/Triangle Intersection", Journal of Computer Graphics
  Techniques 2(1), 2013].
- [design] A ray stops at every surface, on either side. A hit on a side that is not drawn is flagged
  (`Hit::hidden`); light that reaches it is the caller's business (a solver takes it for black).
- [design] Below the alpha cutoff a ray goes on.
- [design] Of hits at exactly the same distance the first in the scene's order is met. Two different
  triangles in one plane (a decal on its wall) can differ by a rounding, so either may be met.
- An instance that mirrors keeps its front, its shading normal and its texture.
- A ray that runs along a box's face meets what ends on that face: an axis the ray does not move
  along is tested against the closed box. [design] A component below 1e-20 of the largest, or
  whose reciprocal overflows, counts as none.

## The light

A cell's light is plain data as well (`LightState`): ambient, directional lights and lamps, in
display values, for one state of the light. `solve` takes it with a prepared scene and the seeds,
places in the air to start from, and makes the volume (below). `reach` takes the same and gives the
points that can be reached with what is gathered at each (`Reached`), without a texture. Both begin
with the same set-up and refuse the same input.

**The sum** at a surface is the same formula as the engine's
[documented: `engine/nvforever/src/lights/display_shader.rs`]:

```text
S    = ambient + sum over directionals (display x saturate(N.L))
               + sum over lamps (display x attenuation x cone x saturate(N.L))
out  = min(albedo x max(S, 0)^2.2, 1), per channel
```

The attenuation is `saturate(1 - d^2 / range^2)` [documented: `engine/nvforever/src/lights.rs`
`GAME_ATTENUATION`], the cone the square of `saturate((cos - cos outer) / max(cos inner - cos
outer, 1e-4))` [documented: bevy_pbr 0.20.1 `render/light.rs`, `render/pbr_lighting.wesl`].
Right at a lamp N.L is shorter (over at least 0.01 units) and the cone's direction is not, as
there. It is the formula, not the engine's bits: the engine adds its terms in another order.
Whether the sum is the original's is the engine's question, not this crate's. Left out of it:
ambient occlusion, what the engine does to a flashlight on first-person shapes, and its weights
for grass.

**What is gathered** at a point comes from rays over the sphere, the same directions at every
point. What a ray brings depends on what it meets:

- [design] A lit surface sends on what the lamps **add** to what it shows: the surface under
  ambient, directional and lamps, less the same under ambient and directional alone, each raised
  to the power and clipped first. So a room without lamps bounces nothing, whatever its ambient,
  and with a brighter ambient the same lamp adds more, up to the clip.
- [design] A lamp counts where a shadow ray finds it. The directional part is taken without one,
  on both sides of the difference.
- [design] A single-sided surface's back is black. A double-sided one is lit on the side that is met,
  by the interpolated normal turned round, and its rays leave from that side.
- [design] A surface drawn on both sides without being double-sided keeps its front's normal. A lamp
  on the side met is then behind it, unless the interpolated normal is tilted towards the lamp.
- [design] An unlit surface (a window, a screen) sends a share of its own colour, clipped to what
  is displayed, lamps or not.
- [design] A miss brings nothing. There is one bounce.

[design] Any surface can stand in a lamp's way, whatever it is made of. **A lamp in a closed
shade bounces nothing** outside of it, while on the screen a lamp without a shadow lights its
room through the shade: the engine draws placed lamps without one unless its ALC shadows row is
on [documented: `engine/nvforever/src/light_shadows.rs`]. A test pins this as it is. How many of
a room's lamps it concerns is not counted.

Kept are the two lowest bands of the light L from the directions w (`Moments`): C0 = mean(L) and
Cj = 2 mean(L wj). The light on a surface that faces n is C0 + C.n [design]. With exact means
over the sphere that is exact for light that has only those bands; the means of 512 rays are near
them (a test holds two such lights to 0.004). It can be below 0 on the side turned away from a
small source: what to do then is the reader's business.

**The points** are the multiples of the spacing in the cell's own coordinates. A seed starts those
of the 8 around it that it sees and that are in the air, else those of the 64 around it. From
there the flood goes on to neighbours along an axis, inside the box about the triangles, with
nothing on the way. A point is in the air when its rays meet something and at most a quarter of it
shows its back. `Coverage` says how a flood went: seeds that found nothing, points, and the parts
they are in. A `Probe` says what its rays did: `lamps` counts the lamps that were looked at, all
those of each lit hit's cell of the lamps' grid, whether their spheres hold the hit or not.

The same input gives the same points and the same light on any number of threads, on one machine
and toolchain. Between toolchains that is not shown.

## The volume

`solve` packs the points that are reached as a 3D texture, with the box about them (`SolvedVolume`):
rgba16float, little-endian, x fastest, then y, then z, without row padding [design]. It holds four
slabs one after another along z, each of the grid's dimensions with one texel of zeros all around:

| Slab | RGB | Alpha |
|---|---|---|
| 0 | C0, the mean of L | m, 1 at a point that is reached, else 0 |
| 1 | Cx, 2 x the mean of L w.x | m v, v the luminance of C0 |
| 2 | Cy | 0 |
| 3 | Cz | 0 |

So a grid of W x H x D points is a texture of (W + 2) x (H + 2) x 4 (D + 2) texels of 8 bytes, and
point (x, y, z) of slab s is texel (x + 1, y + 1, z + 1 + s (D + 2)). All is 0 where m is 0:
the values are weighed by m already, so a filtered read takes nothing from a point that does not
count. A point that is reached and black has an m of 1.

**The read** at a place P with the geometric normal G, on the side the camera sees, and the shading
normal n [design]:

```text
q = (P + offset G - origin) / spacing          outside -1 < q < dimensions: all is 0
t = (q + 1.5 + (0, 0, s (D + 2))) / atlas      of slab s; linear, clamped to the edge
support = m,  w = 1 / max(support, 0.25)
E = max(w (C0 + Cx n.x + Cy n.y + Cz n.z), 0)
v = w (m v)
```

`origin` is the first point that is not of the shell. All four slabs are read at the same place, and
the division is made once, after the filter; so is the clamp of E. The texel of zeros keeps the
filter inside its slab and fades a read out over one spacing around the box.

E is what a white surface sends back of the light around: its irradiance over pi, as far as two
bands hold it. Light that is the same from all around gives itself on every side, and half a sky
gives (1 + cos a) / 2 on a surface turned by a from the zenith; both are tested through a solve.
`Reader` is this read on the CPU, with whole filter weights where hardware rounds them. It is what a
shader is to be held against, and it is tested with numbers worked out by hand.

A solve takes one state of the light. It says what it looked at (`Counts`, `Coverage`) and what it
took (`Stats`); the peak of its memory is a sum of its parts' sizes, not measured.

## The fit

`fit` makes the constants of a cell from its volume (`Fitted`); `solution` is a solve and a fit. A
lit surface is to show, per channel and linear [design],

```text
k     = least + (1 - least) min(pay v, 1)
was   = max(S, 0)^2.2
light = max(S - (1 - b) k A, 0)^2.2 + g E
light = light + pull (Y(light) was / Y(was) - light)          where Y(was) > 0
out   = min(albedo light, 1)
```

with the display sum S, the ambient A in it, E and v of the read, and Y the luminance. b is the
share of the ambient that stays where k is 1, g the strength of the bounce, and `pay` turns the
light around into the part of the taken ambient that it is worth. Where all is dark around, k is
`least`. `pull` gives the light back a part of the colour that it had, at its own luminance. A test
holds the fit's own arithmetic against this equation as it stands here.

**What is kept** [design] is the mean, over what rays meet, of the luminance of the display values
(sRGB transfer, 0 to 255): with the constants it is what it is without. Unlit surfaces are in the
mean as they are. A back that is not drawn and a miss are left out, and every place whose rays met
something weighs the same.

**The places** [design] are above the floor that can be walked on (`Floor`, of a navmesh's
triangles): one in each of as many strata of the same area, at the eye's height. The triangles are
first put in the order of their room (the smallest box that holds them), storey, strip and place,
so that a stratum is near in its room. The rays are a spiral over the whole sphere, turned about
the vertical from place to place. `Seeds::of` a floor are the eyes above its triangles' middles.
For seeds that are no floor's (`Seeds::at`, such as an arrival's place) the places are some of the
reached points, spread over them.

**The whole sphere** [design: decided 2026-11-25] means that the mean is kept over all directions
around the places, not over those of a level view. A room whose roof and floor are both much
brighter than what a level view shows can get a larger g than level views ask for and come out a
little brighter in them, as one of the 19 rooms below does [observed]. That such a roof and floor
do it in other rooms too is a [hypothesis]: they alone do not settle a gain. A fit over the
directions near level would keep the mean of level views, and not that of a look up.

**In two steps** [design]. First k is 1 everywhere, b is 1 - share, and g is bisected. That g over
the light of the ambient that is taken makes `pay`. Then g again, with the k of that. The mean
rises with g and with b. Of several g that keep it (all that is met is at full brightness) it is
the largest. Where g would pass the cap it is the cap, and b is bisected instead: less is taken.
Each bisection halves 24 times. A g that comes out below 2^-12 of the cap is bisected on, up to 64
times more, until it is within 2^-12 of itself; one above is so already, and nothing more is done
to it.

Known of it, and kept as it is:

- `pay` values the taken ambient by itself, without the lamps beside it in the power of 2.2, so k
  reaches 1 earlier than by the true cost. g, and where it is capped b, still keep the mean.
- On the back of a double-sided surface S is by the normal turned round, as E and the gathering
  are. The engine turns it where a mesh has no tangents and where it has a normal map [documented:
  bevy_pbr 0.20.1 `render/pbr_functions.wesl`, `prepare_world_normal` and `apply_normal_mapping`].
- [design: decided 2026-11-25] Where a read has no support E and v are 0, so k is `least`, as where
  all is dark: the surface loses that part of the share (0.1875 of the ambient at the defaults) and
  gets nothing. The fit keeps the mean with such surfaces in it and counts them: `Met::unsupported`
  those of a support of at most 1e-4, where a read can still carry some light, `Met::thin` all
  below 0.25.
- The constants are for this equation alone. Another k, there or elsewhere, is another behaviour
  for what shows them even where the API stays as it is: the fit and the shader have to change
  together, and the constants to be fitted again.
- S is the sum without shadows and without ambient occlusion.
- What unlit surfaces put into the field is made stronger by g like the rest of it: the field is a
  pattern, scaled to pay for the ambient.
- The constants are for one state of the light, as the volume is.

## How near the fit comes

An offline comparison of constants, made on 2026-11-24 with private captures of 19 interiors
[observed]. The fit's constants were held against others that were fitted over 60 camera views with
the light gathered at every pixel. Both were shown with the same solved field, in 60 other views of
each room of 64 x 36 pixels. A room passes when the median over the views of the absolute
difference of the mean brightness is at most 2 of 255 displayed steps and the 95th percentile at
most 5. Each room had to meet both limits separately on 60 held-out views and on 8 look views. In
17 of 19 rooms the fit's constants pass. Two do not, both too bright:

- A tent cell (median about 2.9, 95th percentile 3.4). Its lamp-lit roof and the floor are outside
  of what the views, near level, show: the roof is met by 19.5 % of the rays and seen in 0.11 % of
  the pixels. With the same constants, views in directions over the whole sphere differ by 0.2 and
  0.3, views pitched up by 25 degrees by 0.7 and 0.9 [observed, 2026-11-25]. That is the whole
  sphere, above.
- A small house (about 2.4 and 3.5). There the fit's constants come nearer than the compared ones
  to the light gathered by rays at every pixel (0.95 and 3.34 against 1.13 and 4.28) [observed], so
  the light field is taken for the cause, not the fit [hypothesis].

The gate is a coarse one: it is of mean brightness, and the picture without any bounce light passes
it in 6 of the 19. It says nothing of the field. Against light gathered by rays at every pixel the
field itself loses local peaks: up to about 25 steps in the worst 8 x 8 patch of a view [observed].
None of it is a claim about the game's own picture, and the light on a surface is not shown to look
the same.

## What is open

- **Places without support.** Where the filter has no point that counts, and more than a spacing
  outside of the box, a read is 0, E and v both: in rooms no seed reaches, away from their walls to
  one that is. k is `least` there (decided, above): in a room that no seed reaches a plain
  darkening.
- **Leaks.** A read near a wall thinner than the spacing can take some of what is behind it: the
  filter knows no walls. A test holds one such read.
- **Lost peaks.** Two bands at this spacing do not hold the peak of the light right beside a lamp.
  How much is lost at such a point is not measured. The 25 steps above are another thing: the mean
  of 8 x 8 pixels, the worst patch of a view.
- The volume and the constants are the same for all times of a visit: nothing says yet when they
  are out of date.
- What a solve or a fit takes in time and memory in play is not measured here.

## For another tracer: the live solve

[design] The plan has moved on from a solve on entry: the bounce light is to be solved on the GPU with
ray queries from the first frame of a visit, live by default, with this CPU solver as the reference that
the GPU's results are held against. Both are NVForever's own, home-grown for this engine. It comes in
slices; the first adds only plain data and the seams for it, on the CPU, and changes nothing that a solve
computes:

- **What the GPU reads.** `PreparedScene::export` gives the scene as it is after `prepare` (corners by
  triangle number, faces, materials, the two levels a texture is read at), `LampTable::of` the lamps'
  table of a solve in that light, built by the same code and refusing the same lamps (a spot of no
  direction, a place or range past 2^21), `SolvedVolume::points` the points of a volume and their texels,
  and `PreparedScene::near` the triangles near a place, conservatively (it never omits one within the
  radius, and may include one beyond it by no more than its width across its longest side and a small allowance for rounding). `Sheared::of` is a direction as the rays see it: with its
  still axes at 0, which the tree, the triangle test and the side met use, beside the direction as given,
  which the moments use.
- **A reach whose points are classified elsewhere.** `reach_by` floods as `reach()` does, with the same
  config, seeds, limit of points and refusals in the same order, but a point's validity comes from counts
  that a classifier gives (hits, and hidden sides of them); nothing is lit. With `classified`, the CPU's
  own counts, it is `reach()`'s reach. A classifier's counts that cannot be are the crate's fault.
- **Light gathered elsewhere, packed as a solve packs it.** `SolvedVolume::of_points` makes the same box,
  texture and bytes from the same places and moments, and refuses what its reader would (a luminance
  below 0, a box no texture can hold).
- **`nvf-rtgi-live`.** A solve's inputs packed on the CPU for the future GPU solve: the scene, a light and
  a volume's points as the words a kernel reads, made on any thread. What a shader could not compute as
  the solver does is refused there with the reason (corners past 1e5, colours or factors past 1e10, a
  transformed texture coordinate that could pass 1e5 anywhere between the corners, or one as given past 1e30, lights brighter than
  1e6 together, a surface's albedo past 2^80 on a channel, a lamp's range above 0 below 1e-6, where a device
  could overflow or drop a decisive subnormal) and stays the CPU solver's. Beyond that the GPU may treat subnormal
  values (below f32::MIN_POSITIVE) as zero (WGSL allows it): a lamp's contribution below the smallest normal float,
  and a spot's direction within about 2^-63 units of its lamp, may then be zero on the GPU where the CPU has a
  subnormal value. The light differs by less than 1.2e-38 and a lamp or shadow-ray count may differ by such lamps;
  nothing visible changes.
- **The kernel and its batches (`nvf-rtgi-live`, second slice).** `kernel.wgsl` gathers a point's rays with
  ray queries over one bottom level structure of all triangles (those with an alpha test a geometry of their
  own) and a top level one of it alone, built once for fast tracing, never compacted. Entry points: `solve`
  (each point's moments and its hits, backs, lamps and shadow rays), `reach` (hits and backs alone, nothing
  lit), and `replay` and `meetings` (each ray, the triangle test) for checks. It follows the solver's rules:
  the triangles near a point (its packed list) and those about the structure's nearest hit are decided by the
  solver's triangle test in the solver's order of roundings, a candidate's alpha by the same correctly rounded
  weights as the winner's; the direction as the rays go for the structure, the test and the side met, as given
  for the moments; the lamps' table takes a place rounded onto its far edge as the last cell, its cell from the
  correctly rounded quotient; a spot's cone from the unit direction from the lamp, none right at it, apart from
  the floored one that a surface's facing uses. Away from a point the structure's own barycentrics decide an
  alpha exactly at the cutoff and where a shadow ray ends. `Live` holds a room's points, the light and a
  batch's uniform; `encode` records one batch, `read_back` returns what the batches wrote. The device and the
  queue come from outside; nothing of it is in the game yet.
- **The check.** `cargo run --release -p nvf-rtgi-live --example rtgi-live-check -- [--validation]
  [--adapter <part of a name>]` builds scenes in code (no game data), runs the kernel and holds it against
  `nvf_rtgi`: each ray's surface, light (within 5e-4), lamps and shadow rays against `bounces`, each point's
  counts and moments against their sums and its reach against `classified`, and 100,000 triangle tests against
  `meet`, bit for bit (0 and -0 held as one, and counted). Where a ray's light may differ by design (a shadow ray
  that starts in another surface's plane, a limit the structure decides) it is listed ray by ray and bounded by
  answers the CPU computes on its own, the lamps it sees too. Exit 0 when all pass, 1 on a failure,
  2 without a Vulkan adapter that has ray queries. One known difference, ray 865 from the edge of two walls near
  the origin, is pinned exactly and reported as KNOWN with its reason; its point is held to it and nothing else.

Tests hold the seams to the solver on made scenes: a reach by the CPU's counts is `reach()`'s and
`solve()`'s, `reach()`'s points packed again are `solve()`'s bytes, and the table names the lamps the
solver's lookup names, at the grid's edges too.

## Units

Bevy's world space in game units (y up, -z north), linear colour unless called a display value.

## Numbers

All are ours, none is a claim about the original game.

| Number | Value | Why |
|---|---|---|
| alpha and colour mips (`levels_read`) | the largest level of at most 256 and of at most 64 texels a side, else the smallest there is | [design] one fixed level a lookup: a ray has no pixel to choose it by |
| rays' lift | 0.01 units, or 4e-6 of the largest coordinate | [design] a ray does not meet the surface it leaves |
| roundings | a box's far side 6e-7 of itself further, boxes kept 4e-6 of it past the nearest hit | [design] without the first, rays aimed at edges and corners slip out of a closed room; without the second a nearer hit can be passed over. A test fails without either |
| tree | 16 bins, leaves of at most 8, halves by count below depth 48 | [design] the stack of a walk is bounded |
| limit of a scene | 2,097,152 triangles of all instances together (`SceneLimits`) | [design] a budget for play: about 288 bytes each while they are prepared, 576 MiB for the triangles alone by their sizes, not measured (the input and its textures come on top), refused before any is made |
| spacing | 64 units; 0.001 to 1,000,000 may be asked for | [design] a room's corners and door frames still have points of their own |
| rays a point | 512, on a Fibonacci spiral | [design] fixed directions, the same for every point: no noise from place to place |
| emission | 0.25, at most 1 | [design] counting an unlit surface's whole colour makes windows the brightest lamps of a room |
| flood | 8 points around a seed, then 64; 6 neighbours | [design] a seed in a corner still finds a point; only open ways along the axes count |
| inside geometry | more than 1/4 backs | [design] a point beside one open back still counts |
| the lamps' grid | 32 cells along its longest side, 1 unit at least; a sphere 0.1 % and 1 unit more (of the square), a cell 8 x f32's epsilon of the largest coordinate wider on each side | [design] a ray's end looks at the lamps near it; the additions are for roundings, so that none that adds light is left out |
| furthest seed, lamp | 1e9 spacings, 2^21 units | [design] further off is in no cell, and what the lamps' cells are wider by stays small. A seed there finds nothing, a lamp there is input out of range |
| read offset | 24 units up the geometric normal | [design] a surface reads the air in front of it; less reads more of what is behind a wall |
| least support | 0.25 (`THIN`) | [design] one corner of eight in reach fades out instead of being taken for the whole |
| luminance | 0.2126, 0.7152, 0.0722 of linear red, green, blue | [documented: ITU-R BT.709] |
| half floats | the nearest, ties to even [documented: IEEE 754, binary16]; a value of more than 65,504 is refused | [design] the largest half float: no texel is infinite |
| a side of the texture | at most 2,048 texels | [documented: wgpu-types 30.0.1 `Limits::default`, `max_texture_dimension_3d`] |
| limit of a texture | 64 MiB (`SolveConfig::atlas_bytes`) | [design] a budget for play |
| the eye's height | 112 units above the floor (`EYE`) | [design] near where a camera is; nothing is measured at it |
| the floor's order | rooms, asked half the eye's height up, storeys of 256 units, strips of 512 | [design] what is near by area is near in the room |
| limits of a floor | 65,536 triangles, 1,024 rooms (`FloorLimits`) | [design] a budget for play: every triangle is looked for in every room |
| share of the ambient | 0.75 | [design] chosen by eye among shares of it, with the mean brightness kept |
| cap of g | 10; at most 1,000 may be asked for | [design] a room with little bounce keeps more of its ambient instead of a lamp's light made huge; g is found to 2^-24 of the cap, a small one to 2^-12 of itself |
| least | 0.25 | [design] where no light is around, a quarter of the share still goes: dark corners get darker, not black |
| colour pull | 0.7 | [design] a strongly coloured bounce does not turn the room's colour over |
| places and rays of a fit | 64 and 16; at most 4,096 places and 131,072 rays in all | [design] by equal area of the floor; "How near the fit comes" above |
| negligible ambient | 0.5/255 | [design] half a display step: nothing to be taken |
| largest ambient, and sum on a surface, of a fit | 1,000,000 in display values | [design] no light of a fit overflows `f32`; times a finite albedo it may, to an infinity that shows as 1, or as 0 below 0 |
| the sRGB transfer | 0.0031308, 12.92, 1.055, 2.4, 0.055 | [documented: IEC 61966-2-1] |
| limits of a gathering | 40,000 points, reached or not; 65,536 seeds (`GatherLimits`) | [design] a budget for play; 1,024 seeds' candidates are held at a time |
| looks at the caller's check (scene) | every 65,536 instances counted and triangles placed, tree nodes of 4,096 triangles and more | [design] cheap against the work between two of them |
| fixed limits | 2^26 triangles, 64 threads, 4,096 rays of a point, 4,096 lamps and as many directionals, 2^24 entries of the lamps' table | [design] no count can overflow |

## When there is nothing

`prepare`, `reach`, `solve`, `Floor::new`, `fit` and `solution` give a typed failure (`Failed`):
cancelled, unavailable with a reason (too many triangles, points, seeds or lamps, no seeds, nothing
reached, a number out of range, a texture that is too large, light out of a half float's range), or
a fault of the crate itself. A texture that may not be is refused before any of its box is made or
walked, but after the flood. A light whose numbers are not finite is refused, a spot without a
direction too. A seed whose numbers are not is one that found nothing. What is degraded but usable
of a scene is left out or replaced, and counted in `SceneStats`: broken triangles, missing
textures, normals, texture coordinates, vertex colours or materials with numbers that are none. A
material whose cutoff is no number is opaque: no light goes through it. Its other numbers become
white and the plain texture coordinates.

A fit's config is refused before any work, a solution's before its solve: a number out of its
range, an emission or an offset that is not the volume's, an ambient below 0 or too large. A volume
that `Reader` refuses is refused too, also where the cell would stay as it is for another reason.
That a volume is of the scene and the light it is fitted with is the caller's word. Of a floor's
triangles those without an area, or too far off for `f32`, are left out without a count, and a room
whose size is no finite number holds nothing. A flat one holds what is exactly in its plane, and is
the smallest there.

`fit` gives `Stock` with a reason where there are no constants, and the cell is to keep its stock
light: no place to look from, no lit surface met, nothing bounced on any, hardly any ambient, or on
a surface a sum that is too large or a colour that is no number. Seeds of a floor without a
triangle are no seeds for a solve; a fit does not look from elsewhere for them (`NoReach`).
Constants are always numbers: b from 1 - share to 1, g from 0 to the cap, and pay is 0 where the
first step takes nothing.

## Cancelling

The caller's check is asked every 65,536 instances that are counted, at every instance that is put
in place and every 65,536 of its triangles, at every axis of the tree's larger nodes, and at every
texture. While light is gathered it is asked at every cell of the lamps' table, before every point,
before every group of up to 64 segments of one start, and every 4,096 steps of counting parts. A
solve asks every 4,096 points of the packing too. A floor asks every 1,024 triangles. A fit asks at
its start, at every cell of the lamps' table, every 4,096 texels of the volume, before every place,
and before every candidate's mean over all that was met: up to 50 of them, 370 where gains are
small. It may be asked on any thread, and it must be cheap.

What still runs on without a question, at the limits: the arrays of boxes and the copy into the
tree's order for 2,097,152 triangles, a few passes over them on one thread; one pass of the root's
bins; one whole texture; counting what is wrong with a scene; one point of 4,096 rays; a pass over
the seeds, and over the points of one step of the flood or of its box; allocating and clearing 64
MiB of texture; putting 65,536 triangles of a floor in order; 1,024 of them against all rooms,
however many a caller allows; one place of 4,096 rays, one mean over 131,072 surfaces, of which the
two of the stock light are not asked for at all, and, for seeds without a floor, putting the
reached points in order. `Reader::new` looks at a whole texture and is not asked. None of these is
timed.
