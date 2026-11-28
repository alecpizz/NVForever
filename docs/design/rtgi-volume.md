# The shader that reads a bounce light volume (#3009)

An interior's bounced light is to come from a small 3D texture, a grid of points in the room's
air, in place of a part of the flat ambient light. This note is about the shader that reads such a
texture (the bindings, the read, the light made of it and the surfaces that take part) and about
how a texture gets to it. **Built: the shader and its bindings, and what uploads and binds a
texture. Nothing in the game asks for one**: there is no setting and nothing to see; a developer aid
binds one made by hand. It is home-grown: no vendor's, engine's or library's global illumination is
used. All of it is ours [design], none of it is a claim about the original game.

## The bindings

A view that has bevy_pbr's component `NvfRtgiVolumeBindings` (`engine/vendor/README.md`) has three
bindings more in its mesh view bind group, for the fragment stage: 47, a uniform; 48, a filterable
float 3D texture; 49, a filtering sampler. Its pipelines have the key bit
`MeshPipelineKey::NVF_RTGI_VOLUME` (30), its layouts `MeshPipelineViewLayoutKey::NVF_RTGI_VOLUME`
(20). Both come from that one component, so they agree. No shader def follows from the bit: a
material that reads the bindings pushes `NVF_RTGI_VOLUME` itself.

The uniform is 80 bytes (`rtgi_volume.wesl` `NvfRtgiVolume`):

| Row | xyz | w |
|---|---|---|
| origin | the first point that is not of the shell, in Bevy's world and game units | 1 / spacing |
| dimensions | W, H, D, the points without the shell | how far a read is moved off the surface |
| atlas | 1 / the texture's sides | D + 2, a slab's texels along z |
| fit | pay, least, thin | pull |
| light | b, g, share | 0 |

## The texture

Four slabs one after another along z, each W x H x D points with one texel of zeros all around, so
(W + 2) x (H + 2) x 4 (D + 2) texels:

| Slab | RGB | Alpha |
|---|---|---|
| 0 | C0, the mean of the light L around the point | m, 1 at a point that counts, else 0 |
| 1 | Cx, 2 x the mean of L w.x | m v, v the luminance of C0 |
| 2 | Cy | 0 |
| 3 | Cz | 0 |

Everything is 0 where m is 0.

## The read

At the place P with the geometric normal G, on the side the camera sees, and the shading normal n:

```text
q = (P + offset G - origin) / spacing          outside -1 < q < dimensions: E = v = 0
t = (q + 1.5 + (0, 0, s (D + 2))) / atlas      of slab s; linear filter
w = 1 / max(m, thin)                           thin = 0.25
E = max(w (C0 + Cx n.x + Cy n.y + Cz n.z), 0)
v = w (m v)
```

`thin` is 0.25, the least support [design]: one point of eight in reach fades out instead of being
taken for the whole. The shader takes it from the uniform (`fit.z`), so whoever fills the uniform
supplies it, for an empty texture too: with 0 a read without support divides by 0.

G is the triangle's, from the world position's derivatives, taken before anything is discarded;
where a triangle is too small to have one, n. The reads are of an explicit level, so they need no
uniform control flow.

## The light

S is the display-value sum (`lights/display_shader.rs`), A the ambient part of it with the ambient
occlusion in it, occ that occlusion: `diffuse_occlusion`, times the ambient occlusion's factor per
colour channel where the view has it. Where the diffuse light was `diffuse colour x S^2.2`, it is
the diffuse colour times:

```text
k     = least + (1 - least) min(pay v, 1)
light = max(S - (1 - b) k A, 0)^2.2 + g E occ
light += pull (Y(light) was / Y(was) - light)     was = max(S, 0)^2.2, where Y(was) > 0
if share < 1:  light = was + share (light - was)
```

Since the display sum is shown by the sRGB curve (lights/display_shader.rs; docs/spec-notes/world.md
§8 "Dark values of the sum"), the line takes `light` back to a sum, `light^(1/2.2)`, and shows
that as the sum is: `to_linear(min(to_display(diffuse colour) x light^(1/2.2), 1))`. With the
volume's change at 0, `light` is `max(S, 0)^2.2` and the line is the one without the volume.

Y is the luminance (0.2126, 0.7152, 0.0722). The occlusion is in A and once on the bounce, in
linear light. The clip to 1 per channel comes after, as before. Emissive light, the highlights, the
reflections and the fog are as before.

- **Share** is for a fade: at 0 the light is `was`, at 1 it is the new one, both exactly.
- **No support is not the stock light.** Where the read finds no point that counts, k is `least`, and
  a part of the ambient goes though nothing comes in its place. With b = least = 0.25 a fragment
  that has ambient light alone shows 81.25 % of its display sum there, 63.3 % of its linear light
  (0.8125^2.2) at full share. A uniform with b = 1, g = 0, pull = 0 and share = 0 is the stock light
  whatever the texture holds.

No value of b, g, pay, least, thin, pull or the offset is in this code: all come with the uniform.

## Who reads it

A NIF surface's main-pass pipeline in a view with the bit (`nif_material.rs` `reads_rtgi_volume`),
where the blend is opaque or alpha to coverage and the surface is none of: refracting, first-person
(arms, weapon, Pip-Boy), Pip-Boy screen, added linear light, depth-writing blend (hair), landscape.
So solid decals, actors, faces and the third-person body read. The mesh terrain blend's pipelines
give the def up again. Blended, added and multiplied shapes, water, terrain, grass and trees keep
the stock light; so do unlit shapes, which are in no lighting. A blended cut-out reads while FSR or
DLSS has it drawn as a mask and not while it is blended.

## The hook

`lights.rs` patches bevy_pbr's `pbr_functions` once, at its load: the display-value sum, the sun
visibility, the ambient occlusion, this hook (an import and the diffuse line, `rtgi_volume/hook.rs`),
the reflection gate. It is always put in, and reported as installed only once the patched shader is
stored. When a line is missing or there twice, the source stays as it was and one error is logged
(`HookState::Failed`): then no view may get a volume.

## The owner

The main world's `RtgiRequest` says what the player's view is to have: for which place, the number
of the result (its revision), and a solved volume with its constants and its share, or the empty
texture. It is not there by default. A place is a visit: the interior, the travels so far and the
running game, so a travel into the same cell or a load is another one. The empty texture is one
point that does not count, with the stock light's uniform (b 1, g 0, pull 0, share 0, thin 0.25).

Each extraction makes a plan of it (`rtgi_volume/plan.rs`, a pure function) and carries it out
(`gpu.rs`), before anything is specialised:

- **First bind** of a visit: only under the loading screen of a start or a travel, until it first
  settles, and only with a world camera whose view is drawn in that frame. The screen waits for
  pipelines that were asked for, so those of what is loaded, in range and not culled are asked for
  while it is up. A request that comes later waits for the next visit, in the stock light. What comes into view later asks for its pipeline then and is skipped until it
  is ready, as any other.
- **Another texture of the same visit** (empty to solved, a newer result): at any time, as it changes
  no key and no pipeline. An older result than what is bound is dropped, whatever it is.
- **Unbind**: in the extraction that sees another place, the outdoors or no request. Every world
  camera's view has it, a replaced camera's new one in the same extraction; the mirror, the scope's
  lens and the menus never.
- **Stock to the end of the visit**: the hook failed, the device refused (a solved texture too, where
  the empty one was granted), the request was late, or a pipeline whose fragment has the def failed
  for good (looked for once a frame while something is bound; the component goes in the next
  extraction). A reader whose stock twin is broken too counts as well, as it is told by the def. The
  log has one line a visit for it, beside one for what was unbound and the pipeline cache's own.
- **Late at a start**: a run begins without a game, so the visit is another one when the scripts have
  made it. If that is the frame in which the screen settles, the request is late: the stock light for
  that visit. Known, how often is not measured; left to the part that has the job and the fade.

A texture that is not used any more is let go; the GPU has it until it is done with it. A run
counts what it makes, sends, binds and dispatches, and bevy_pbr's patch counts the view bind groups
and those with the bindings; with the developer aids the log's `rtgi:` line at the exit has all six.

## Admission

Asked before anything is made (`rtgi_volume/admit.rs`), and again whenever the answer may be
another: for other sides, or when the cloud shadows' light volume joins or leaves the view's layout.
RTGI's extraction runs after the sun visibility's, which decides that in the same frame. Refused
when:

- the fragment stage of the fullest pipeline has more sampled textures, samplers or uniform buffers
  than the device allows. That pipeline is the view's fullest layout (`fullest_view_key`, with the
  tonemapping's table, the contact shadows' uniform and the three bindings, without the light
  volume's arrays while they are off), 2 textures, 2 samplers and 4 uniform buffers of the mesh, and
  the most that a material drawn in that view has. Binding arrays count against other limits;
- rgba16float cannot be filtered, sampled or written;
- a side is larger than the device allows, or the texture than 32 MiB. An old and a new one may then
  be there side by side: 128 MiB of payload for one replacement, the staging copy and the main
  world's counted; revisions that follow each other faster than the GPU lets go add to it.

While bevy_pbr's layouts or the hook are not there yet it is asked again in the next frame. What it
cannot promise: an allocation may still fail, which ends the game as any other.

## The check on the GPU

`NVF_PLAY_DEBUG_RTGI_MADE=1` (builds with the developer aids): in every interior the empty texture
under the screen and, after it, a volume of 3 x 2 x 4 points made by hand around the camera: a texture
of 5 x 4 x 24, every point with values of its own, three that do not count and a black one. Once, a
compute pass (`rtgi_readback.wesl`) makes 96 reads of it through the view's own buffer, texture and
sampler with `nvf_rtgi_read`, and the log says how far they are from `nvf_rtgi::Reader`'s:

- at texel centres that the normalised coordinates hold exactly on every axis and in every slab (x 1
  and z 0 or 3 here), and outside: within A = 1e-4 + 1/2048 of the value, as nothing is filtered;
- at the other centres, between the points, with thin support and in the shell: within a bound worked
  out for each read. With e = 1/256 + 1e-4 texels of filter error on an axis a filtered channel is
  off by at most e (Rx + Ry + Rz), R the largest difference of two texels next to each other along an
  axis in all the cells that e reaches (on both sides of a centre); with N the numerator's error, D the
  support's and d = max(m, thin), E is off by at most (N + |E| D) / (d - D), and v the same. A on top.

The log says `readback passed` or `readback FAILED`, of the numbers as they are, and the largest
difference over its bound of each kind. That e is a threshold for the machines the smoke test runs on
(`rtgi-made-volume`), 8 fractional bits rounded or cut. It is no claim about a GPU's precision, and
a miss alone does not tell coarse filtering from a wrong slab, place or uniform; right exact centres
with wrong fractions point at the filter. The light's formula and the surfaces that read are not
checked there: the tests have them. The job belongs to the visit and the revision it was made for: its
buffers are made when that is bound and go with it, and one answer ends it. One of another length is
a failure.

**Run** [observed, in our own runs on 2026-11-25, NVIDIA RTX 5070 Ti, Vulkan]: both smoke scenarios
pass; the readback passed at 96 of 96 reads (the largest difference was 0.000 of its bound at the exact
centres and 0.33 of it between points) with 4x MSAA, TAA, FSR and DLSS; the runs with the made volume
while turning and walking made no validation error and no failed pipeline. Off against main at eight
views: equal up to what two runs of one build differ by. What the smoke test sees is that
pipelines with the def were made and that bind groups had the entries, not which camera drew with them.
Other GPUs and Android are not run.

## What nothing bound means

- No view's layout has an entry at 47-49, no pipeline has the def, and every composed shader
  declares what it declares without this [tested without a GPU, `rtgi_volume/tests.rs`: bevy_pbr's
  own layouts and view key systems on wgpu's no-op device, and the shaders composed and validated
  with and without the hook, over the shader defs listed there].
- With no request the plugin makes, sends and binds nothing in an interior, over a travel and
  outdoors [tested without a GPU, in a world that has no device to do it with]. The smoke scenario
  `rtgi-off-interior` checks the same of a run, with bevy_pbr's own count.

What a run without a volume does differ in:

- The numbers of mesh pipeline keys: the packed fields above the new bit (samples, blend,
  tonemapping, shadow filter, projection, transmission, target format) are one bit higher. They are
  only in memory; the disk caches of pipelines hold none.
- One shader asset more (`rtgi_volume.wesl`), the edits in the text of `pbr_functions` and
  `nif_material.wesl`, and one log line ("RTGI volume hook installed").
- The cache of composed shaders is keyed by its sources' text, so the entries of shaders that use
  those two are composed once more after an update that brings this. How long that takes is not
  measured here.
- Two systems of the render world, which return at once, their empty state, and bevy_pbr's two
  counters. With the developer aids the log's `rtgi:` line at the exit.

What the lighting costs with a volume bound is not measured here.
