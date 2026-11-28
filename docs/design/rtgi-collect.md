# Collecting a room for the bounce-light solver (#3009)

Part 6 of [the solver's plan](rtgi-solver.md): what the solver takes (meshes and textures as plain
data) out of the running game. All of it is home-grown, none of it a claim about the original game.
**Built: the reader**, the arrays of a drawn mesh and the texels of a drawn texture that light rays
read. **Nothing in the game calls it.** The capture of what a room draws and the putting together
are still to come.

## The files are read again

Meshes and textures leave the main world when they go to the GPU (`RenderAssetUsages::RENDER_WORLD`)
[documented: `nvf-assets` `nif_spawn.rs` `build_mesh_desktop`, `texture.rs` `checked_image_layers`].
[design] A worker reads the same files a second time through the same mounted VFS; nothing is kept
while loading but one `u32` per static or rig entry of a prepared model.

- `ModelCache::sources`: for mesh ids the model's path (the cache key before its first `|`), the
  shape's NIF block and name. [design] A block names a shape where an ordinal would not: a skinned
  shape has a second mesh, a hidden one that a controller shows is the rig's alone, other hidden,
  helper, editor and rejected ones are nowhere, and it is the same in every variant of a file.
- `TextureCache::sources`: for image ids the resolved path, the usage and whether it is the clamped
  copy; the workers' entries too. A cube map is refused.
- [design] An id with two different answers is `None`. No loader makes one.

[design] **Assumption: files do not change while the game runs**; a loose one is opened again by its
name. Compared is that a block is a shape (not of which kind), its name and its positions' box bit for
bit, and a texture's sides, levels and format. What changed and kept all of these is not told apart.

## A model's shapes

`model::geometry::geometry` gives the wanted blocks' positions, normals, first texture coordinates,
colours and indices in the drawn mesh's order, by the loader's own functions (`shape_props`,
`NumericRejections::admit`, `fix_vertex_colors`). It makes no handle, material, tangent or clip, and
counts and logs nothing (but for the game's warning of a panic that is caught).

- [design] It does not ask whether the loader draws the shape: it is asked for drawn meshes.
- A skinned shape is its bind pose, as its static copy is drawn. Vertex morphs are the stored pose:
  those meshes stay on the CPU and the caller takes them from there.
- [design] One shape that is not there, has another name or has nothing to draw is the whole file's
  error, and so is a panic in a broken file: a lost wall lets light through.

## A texture's levels

`texture::levels::read` reads the whole file and decodes only the levels that rays read, as RGBA8:
`nvf_rtgi::levels_read`, the rule of `Sampler::new`, the alpha's level where **any** material with the
texture has a cutoff. BC1 to BC3 that the GPU has as blocks by `texpresso`, all that the loader
decodes as it does; BC7, BC4 and BC5 are not decoded. The colour space and both axes' wrap are the
drawn image's; the app's default sampler and a border are refused, not guessed. A texture that is not
had again says why (`Lost`); what is made of it is the caller's to say, which knows of alpha tests.

## Limits

[design] Ours, each looked at before the work it bounds.

- `Vfs::read_at_most`, by the caller's numbers: an archived file's stored size before its buffer is
  made and the size its block declares before it is inflated; a loose file by the opened file's
  length, and not read on past it. A refusal names the path and the number. Other reads count in
  `fnv_vfs::read_stats`.
- A texture file 64 MiB, stored and decoded; a level that is decoded 2,048 texels a side.

What a decoded NIF takes beside its bytes, and the time and memory of a room, are not measured.
