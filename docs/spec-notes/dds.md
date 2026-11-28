# Spec notes: DDS textures (Layer 2)

Tags as in `nif.md`. Observations cover all 26,907 `.dds` files in the owner's archives
(FNV 1.4.0.525). Public source: Microsoft's DirectDraw Surface documentation
("DDS File Layout", `DDS_HEADER`, `DDS_PIXELFORMAT`), cited below as [documented: MS DDS].

## 1. Layout

1. Magic `DDS ` (4 bytes) [documented: MS DDS; observed in every file].
2. Header of 31 u32 values (124 bytes) [documented: MS DDS]: structure size (always 124
   [observed]), flags, height, width, pitch or linear size, depth, mip-map count,
   11 reserved u32, then the 32-byte pixel format, then four capability u32s and one
   reserved u32. Note that height comes before width.
3. Pixel format [documented: MS DDS]: size (always 32 [observed]), flags, FourCC (4
   bytes), RGB bit count, then red, green, blue and alpha bit masks.
4. Pixel data follows immediately: for each face, every mip level from largest to
   smallest [documented: MS DDS]. There is no DX10 extension header in these files
   (no FourCC `DX10`) [observed]. Files from other tools may have one: then the data
   starts 20 bytes later (§6).

## 2. Formats present [observed]

| Pixel-format flags | FourCC / masks | Name used here | Files |
|---|---|---|---|
| 0x4 (FourCC) | `DXT1` | DXT1 | 19,437 |
| 0x4 | `DXT5` | DXT5 | 5,955 |
| 0x4 | `DXT3` | DXT3 | 1,486 |
| 0x41 (RGB + alpha) | 32 bit, R 0x00FF0000, G 0x0000FF00, B 0x000000FF, A 0xFF000000 | A8R8G8B8 | 13 |
| 0x40 (RGB) | 24 bit, R 0x00FF0000, G 0x0000FF00, B 0x000000FF | R8G8B8 | 8 |
| 0x41 | 16 bit, R 0x0F00, G 0x00F0, B 0x000F, A 0xF000 | A4R4G4B4 | 8 |

Names follow the Direct3D 9 format names for those masks [documented: MS "Map Direct3D 9
Formats to Direct3D 10"]. Masks are little-endian: an A8R8G8B8 pixel is stored B, G, R, A.

### 2.1 Layouts read beyond the base game's (#1439)

No file of the base game uses these; a mod's or a localized install's textures may (a
font texture in one of them was otherwise drawn with a stand-in font). Flags and masks
as [documented: MS DDS, `DDS_PIXELFORMAT`: `DDPF_RGB` 0x40, `DDPF_ALPHAPIXELS` 0x1,
`DDPF_LUMINANCE` 0x20000; "Using Texture Formats", the masks of each Direct3D 9 format]:

| Flags | Bits and masks (R, G, B, A) | Name |
|---|---|---|
| 0x40 | 32: 0x00FF0000, 0x0000FF00, 0x000000FF, - | X8R8G8B8 |
| 0x41 | 32: 0x000000FF, 0x0000FF00, 0x00FF0000, 0xFF000000 | A8B8G8R8 |
| 0x40 | 32: 0x000000FF, 0x0000FF00, 0x00FF0000, - | X8B8G8R8 |
| 0x40 | 16: 0xF800, 0x07E0, 0x001F, - | R5G6B5 |
| 0x41 | 16: 0x7C00, 0x03E0, 0x001F, 0x8000 | A1R5G5B5 |
| 0x40 | 16: 0x7C00, 0x03E0, 0x001F, - | X1R5G5B5 |
| 0x20000 | 8: luminance 0xFF | L8 |
| 0x20001 | 16: luminance 0xFF, alpha 0xFF00 | A8L8 |

A layout without alpha reads as opaque; luminance is copied to all three colours
[documented: MS "Map Direct3D 9 Formats to Direct3D 10"]. Alpha-only `DDPF_ALPHA`
(A8) is not read: Direct3D samples it with black colour, so what it would look like
in the game is not known.

A FourCC file may also set `DDPF_ALPHAPIXELS` (flags 0x5): it reads as the 0x4 row of
its FourCC, the bit adds nothing [observed: Honest Hearts' tree textures, e.g.
`textures\nvdlc02\landscape\trees\nvdlc02treebark.dds`, `DXT5`/`DXT1` with flags 0x5,
which the original draws textured; refused, they drew white (#4488)].

## 3. Sizes [observed: exact for all 26,907 files]

- Width and height are powers of two in every file, 4096 at most [observed]. A texture
  with a side of 0 or above 16384 is refused as a broken header (`fnv_dds::MAX_SIDE`,
  the engine's own cap) [design: #1995]; the listing still reports the header as it is.
- Number of levels = the stored mip count, or 1 when it is 0. 2,214 files leave the
  "mip count valid" header flag (0x20000) clear; using the stored count anyway still gives
  the exact size, so the flag can be ignored [observed].
- Each level halves width and height (rounding down, minimum 1).
- Block formats store 4×4 texel blocks: 8 bytes per block for DXT1, 16 for DXT3/DXT5;
  blocks per level = max(1, ceil(w/4)) × max(1, ceil(h/4)) [documented: MS DDS].
  Uncompressed levels take w × h × (bit count / 8) bytes.
- Cube maps (capabilities-2 value 0xFE00: cube map with all six faces [documented: MS
  DDS]) store six faces one after another, each a full mip chain. Six files are cube maps.
- The bytes after the 128-byte header equal the sum above exactly; no file has trailing
  data.
- Mods' files are not all exact [observed: Classic Fallout Weapons, 78 of its 468
  textures, DXT1 and DXT5, carry 8 to 32 bytes more than their size and mip count
  need, e.g. `sten.dds`: 1024×1024 DXT1, 10 levels counted, 8 bytes over; the owner
  sees these weapons textured in the original game]. The levels the header counts are
  read and what follows is left alone [hypothesis: the original does the same]; a file
  with less data than the layout needs is refused (#2164).
- A mod's block-format texture may have sides that are not multiples of 4 [observed:
  the same mod's `YK32_innerpanel.dds`, DXT5, 179×196, 8 levels]. Its blocks cover
  ceil(w/4) × ceil(h/4) and the texels past the edge are not part of the picture
  [documented: MS DDS]. The engine decodes such a texture to RGBA on the CPU, as GPUs
  take block formats only at multiples of 4 [design: #2164].

## 4. Naming conventions [hypothesis, to verify against shaders and the NIF texture sets]

Base texture `x.dds`, normal map `x_n.dds`, glow map `x_g.dds`, and environment mask
`x_m.dds` / cube map `*_e.dds`. The NIF `BSShaderTextureSet` lists the texture paths
explicitly, so rendering does not rely on the naming convention.

## 5. CLI: `fnvtool dds list <archive>` → `fnv.dds.listing/1`

```json
{"schema": "fnv.dds.listing/1", "archive": "Fallout - Textures.bsa",
 "files": [{"path": "textures\\x.dds", "width": 512, "height": 512, "mip_count": 10,
            "levels": 10, "format": "DXT1", "dxgi_format": null, "cubemap": false,
            "faces": 1, "array_size": null, "expected_size": 174776,
            "actual_size": 174776, "error": null}]}
```

`mip_count` is the stored value; `levels` the number used (§3). `format` is a name from
§2 (a row matches on the whole pixel-format flags value; 0x5 reads as 0x4, §2.1), or, for anything else,
`unknown(flags=0x%x,fourcc=<4 ASCII chars, or 0x + 8 hex digits>,bits=%d,masks=0x%08x/0x%08x/0x%08x/0x%08x)`
with `expected_size` `null`. For a file with a DX10 header (§6), `format` is the DXGI
name of its code (`BC7_UNORM_SRGB`), or `unknown(dxgi=N)` for a code not read, and
`dxgi_format` is the code itself; `dxgi_format` is `null` for every other file.
`cubemap` = capabilities-2 bit 0x200 (or the DX10 cube flag); `faces` = faces per array
element: for a legacy file the number of the six face bits set (1 when not a cube map),
for a DX10 cube map always 6, one cube per array element (§6.1). `array_size` is a DX10
header's array size (`null` for every other file). `expected_size` covers every array
element, face and level, and for a DX10 volume (3D) each level's slices; it is `null`
for a layout the listing does not size: a legacy volume (capabilities-2 bit 0x200000
[documented: MS "DDS_HEADER", dwCaps2]), and for a DX10 header a resource dimension
other than 1D, 2D or 3D, an array size of 0, a volume that is an array or a cube, or a
1D cube, a cube being whatever `cubemap` says. Sizes count the
bytes after the headers: 128 bytes, or 148 with a DX10 header. The listing reads the
headers only; it does not apply the refusals of §6.3. `error` is set when the file is
not a valid DDS (bad magic, short header, a DX10 file shorter than 148 bytes); then the
other fields are `null`.

Lab comparator (`parity-lab/scenarios/layer2_dds.json`): **keyed** on `path`, with no
field list, so each row compares every field the independent oracle
(`tools/capture/oracle_dds.py`) writes: `path`, `width`, `height`, `mip_count`,
`levels`, `format`, `cubemap`, `faces`, `expected_size`, `actual_size` and `error`
(`compare_keyed` in `parity-lab/fnvlab/comparators.py` skips fields the oracle row
lacks). `dxgi_format` and `array_size` are fnvtool's own: the oracle writes neither, so
they are outside that comparison and are checked by fnvtool's unit tests only.

## 6. DX10 header (files from other tools, #2981)

No file of the base game has one (§1). Texture tools write it for formats the legacy
pixel format cannot name (BC7, sRGB variants) and for arrays, and may write it for
formats a legacy FourCC also names (BC1-BC5, §6.2): the format read is the same either
way.

### 6.1 Layout

- The header follows when the pixel format has the FourCC flag and the FourCC `DX10`
  [documented: MS "Programming Guide for DDS", DDS file layout; MS "DDS_HEADER_DXT10",
  remarks]. It sits between the 128 bytes of §1 and the pixel data, so the data starts
  at byte 148, and a shorter file is cut off [documented: MS "Programming Guide for
  DDS", DDS variants: such a file is at least 148 bytes].
- Five little-endian u32 values, 20 bytes [documented: MS "DDS_HEADER_DXT10"]:
  1. the `DXGI_FORMAT` code of the data;
  2. the resource dimension: 2 a 1D, 3 a 2D, 4 a 3D texture;
  3. miscellaneous flags: 0x4 marks a 2D texture as a cube map;
  4. the array size: how many textures the file holds (for a cube map, how many cubes,
     six faces each);
  5. a second flags value whose low three bits are the alpha mode: 0 unknown (what
     legacy files have), 1 straight, 2 premultiplied, 3 opaque (every alpha fully
     opaque), 4 custom (the alpha is a fourth data channel); the other bits are
     reserved.
- The levels are laid out as in §3, each array element a full chain [documented: MS
  "Programming Guide for DDS", texture arrays]; a cube map's element is its six faces.
  A volume's level holds its slices together, half as many each level, at least one
  [documented: MS "DDS Volume Texture Example"]. Block formats keep whole 4×4 blocks
  per level; BC1 and BC4 blocks are 8 bytes, the other BC formats 16 [documented: MS
  "Programming Guide for DDS", pitch of block-compressed formats].

### 6.2 Codes read

[documented: MS "DXGI_FORMAT enumeration", the numbers of `dxgiformat.h`]

| Code | DXGI name | Read as |
|---|---|---|
| 28, 29 | R8G8B8A8_UNORM, _SRGB | A8B8G8R8 (§2.1): bytes R, G, B, A |
| 71, 72 | BC1_UNORM, _SRGB | DXT1 |
| 74, 75 | BC2_UNORM, _SRGB | DXT3 |
| 77, 78 | BC3_UNORM, _SRGB | DXT5 |
| 80, 81 | BC4_UNORM, BC4_SNORM | BC4: one channel, 8-byte blocks (legacy `BC4U` or `ATI1`, `BC4S`) |
| 83, 84 | BC5_UNORM, BC5_SNORM | BC5 (dds-bc5.md; legacy `ATI2`, `BC5S`) |
| 87, 91 | B8G8R8A8_UNORM, _SRGB | A8R8G8B8 (§2): bytes B, G, R, A |
| 98, 99 | BC7_UNORM, _SRGB | BC7: four channels, 16-byte blocks |

The byte orders of 28 and 87 are those of the legacy masks Microsoft gives for the
same formats [documented: MS "Programming Guide for DDS", common formats table].
BC4 and BC5 also come with legacy headers: Microsoft's guide lists the FourCCs `BC4U`
and `BC4S` for BC4_UNORM and BC4_SNORM, and `ATI2` and `BC5S` for BC5_UNORM and
BC5_SNORM; all four are read, with data at byte 128 and no sRGB hint or alpha mode.
`ATI1` and `BC5U` are not in that table [documented: MS "Programming Guide for DDS",
common formats table]. BC7 has no legacy FourCC in that table, so BC7 is read from a
DX10 header only.

`ATI1` is read as BC4_UNORM, the same as `BC4U` (#3151). It is the FourCC of ATI's
one-channel "3Dc+" format, the older name of BC4 [documented: Wikipedia, "3Dc", the
3Dc+ section: FourCC `ATI1`, also known as BC4], and Microsoft's own DDS library reads
`ATI1` and `BC4U` alike as BC4_UNORM [documented: DirectXTex wiki, "DDS I/O Functions",
the Remarks table of legacy FourCCs]. The OpenGL name for the format, RGTC1, is "BC4
unsigned" [documented: Khronos EXT_texture_compression_rgtc, table C.3]. `BC5U` is
still not read: the same DirectXTex table gives it as BC5_UNORM, but no file with it
has been seen.

[observed: the loose `.dds` files of the two mod list installs, Blue Moon and High and
Dry, headers only, 2026-11-21: 60,826 files; no FourCC (masks) 23,257, `DXT1` 19,980,
`DXT5` 14,806, `DXT3` 2,737, DX10 BC3_UNORM_SRGB 41, `ATI2` 3, `BC5S` 1, DX10 BC7 1;
none with `ATI1`, `BC4U`, `BC4S`, `BC5U` or a BC6H code. Archives were not looked in.]
So `ATI1` is read on the documents' word; no file with it was at hand to draw.

BC6H (DXGI 95 `BC6H_UF16`, 96 `BC6H_SF16`; high dynamic range colour in 16-byte
blocks) is not read and stays refused as `unknown(dxgi=N)` [design, #3151]: it has no
legacy FourCC in Microsoft's table, so it needs a DX10 header; the engine has no CPU
decoder for it and no use that takes values above 1 from a texture file (the sky and
the lights are computed, not sampled from HDR images). [hypothesis: the codes 95 and 96
are from memory of `dxgiformat.h`; Microsoft's BC6H page names the two formats without
their numbers.] A mod texture in BC6H is drawn as missing and listed in the log.

### 6.3 What NVForever does with it [design: #2981]

- Refused (`Unsupported`): any other code (the listing's `unknown(dxgi=N)`), a resource
  dimension other than 2D, an array size other than 1 (0 included), and a cube map, by
  the DX10 flag or the legacy capability bit. The engine draws no DX10 cube or array.
- The `_SRGB` codes are a hint (`DdsHeader::srgb_hint`): the colour space is chosen by
  what the texture is used for, as for legacy files, never by the code. A diffuse is
  sRGB, data and normal maps are linear.
- An OPAQUE alpha mode (`DdsHeader::alpha_opaque`) marks the texture as holding no
  alpha mask, so a normal map with that mode gets the no-mask specular of nif.md §6.
  Without it a BC7 or BC3 normal map's alpha is read as its specular mask, as for a
  legacy DXT5 one. A DX10 BC1 holds no mask on every path, also when decoded to RGBA8
  for sides off the block grid; a legacy DXT1 decoded so keeps the engine's earlier
  reading (its alpha taken as a mask), unchanged by this work.
- The BC4 and BC5 rules hold whichever header the file has. BC7 has no CPU decoder in
  the engine: it goes to the GPU as blocks when the top level's sides are multiples of
  4 and the GPU samples BC formats, and is refused otherwise. BC4 is linear data only (a height or a mask): native when aligned and
  supported, else decoded on the CPU to one byte a texel; a colour or normal-map use
  is refused. The CPU readers of alpha (cut-out detection, alpha maps) report no alpha
  for BC4 and BC7, and the generated terrain LOD bake reads no BC4 or BC7 texture.
- Like legacy files, `Dds::parse` takes up to 32 levels and leaves bytes after the
  counted ones alone (§3); the GPU upload refuses more levels than the size allows.

## Device without BC sampling

[documented: wgpu 30 `Features::TEXTURE_COMPRESSION_BC`,
https://docs.rs/wgpu/30.0.0/wgpu/struct.Features.html]
BC1/BC2/BC3 GPU textures require the renderer's BC compression feature.

[design, Android #2436] The shared texture loader uses the existing fnv-dds DXT1/3/5
block decoder when BC sampling is unavailable, as it already does for non-block-aligned
source dimensions (#2164). Decode each validated mip into RGBA8 at that mip's dimensions;
retain the caller's sRGB diffuse or linear data intent and normal sampler. A BC-capable
renderer keeps aligned compressed bytes and formats. This changes no DDS format facts,
worker-cache coordination or source game files. Cubemaps and unsupported/truncated DDS
remain errors. Authored colour/alpha/mip/capability controls are source-only until run;
GPU output and the reported exterior symptom are not established by this policy.
