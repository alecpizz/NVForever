//! The ground pack test aid (#3322, terrain parallax; docs/spec-notes/pbr-materials.md
//! "Terrain layers: parallax"), default off, behind `NVF_PLAY_TEST_PBR_GROUND_PACK=1`: a
//! generated height-only set (`_p` and a scalar file) for every landscape texture the load
//! order names, written into a fresh folder of the run under an explicit `NVFOREVER_HOME`
//! and mounted over every other loose layer for that run only, as the reflections' mount
//! pack is (`reflect.rs` [`super::reflect::seed_mount_pack`]). No data set ships ground
//! height maps (the ESM's landscape texture sets name none, pbr-materials.md), so this is
//! the only way a run can show the terrain march.
//!
//! [design] What is written, per landscape texture (the caller hands in the diffuse paths of
//! the load order's LTEX texture sets, nvforever terrain.rs `ground_pack_diffuses`): the
//! set's `_p` ([`ground_height_dds`]) and its `<stem>.toml` (`parallax_depth` =
//! [`GROUND_DEPTH`], the scalar range's end), under `textures\pbr\` at the stem
//! `fnv_pbr::stem_for` gives the diffuse, so the index finds a height-only set for it
//! exactly as it would for a pack's (`TextureCache::authored_height`). A diffuse under an
//! `\xbox\` folder gets the `\pc\` twin's set too (the terrain path resolves to the PC file
//! where one exists, and this aid cannot ask the VFS which). Nothing else: no `_orm`, no
//! `_n`, no diffuse; the terrain path reads only the `_p` and the depth.
//!
//! The height comes from a formula ([`ground_height`]), never from a game file: the game's
//! texture paths are used for the file names only. One map for every texture: low (0) almost
//! everywhere, so a layer's whole surface sinks by the set's depth and its texture moves as
//! one under a tilted view (what the readback aid measures, nvforever
//! `terrain_parallax_ab.rs`), crossed by a coarse grid of raised ridges at 1 (full range), so
//! pictures show relief. The map is unsigned BC4 (`BC4U`), [`GROUND_SIDE`] square with a full
//! mip chain down to 1x1, every 4x4 block one value (its endpoint 0, every index 0
//! [documented: a BC4 block is two endpoint bytes and 48 bits of 3-bit indices, Microsoft
//! "Texture Block Compression in Direct3D 11"]), each smaller level the mean of the blocks it
//! covers: within the pack rules (`_p` BC4 unsigned, sides multiples of 4, full mip chains;
//! pbr-materials.md "Compression and sizes").
//!
//! With `NVF_PLAY_TEST_PBR_GROUND_PACK=alpha` (#3789) the pack also replaces every landscape
//! texture's diffuse ([`ground_diffuse_dds`]): one flat colour per texture, by its
//! place in the list, and a checkerboard of 0 and 255 in the alpha, as DXT5 with a full mip chain, and a
//! flat `_n` beside it (the name the game's landscape normal maps have), so the ground has no
//! texture but its shading. The alpha has no part in the landscape's blend, so a spot where one layer covers the ground
//! is of one colour; a renderer that blends by it shows the board in the colours of what lies
//! under the layer. The height sets come with it, so the readback aid finds its spot.
//!
//! With `NVF_PLAY_TEST_PBR_GROUND_PACK=alpha-height` (#3486) there are no sets: every diffuse is
//! replaced by a DXT5 in two shades of its colour, a shade a 4x4 block from a hash, whose
//! alpha is the height map above ([`ground_height`]), with the flat `_n` beside it: a ground
//! pack in the convention whose alpha is a height (nvforever terrain.rs "Heights from a
//! diffuse's alpha").
//!
//! [engineering policy] A test aid: it makes the run unattended, writes no game folder and no
//! mods folder, and nothing it writes is a fact about the original game.

use bevy::color::{Hsva, Srgba};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{create_file, new_output_in};

/// The test aid's switch (`=1`, `=alpha` or `=alpha-height`: [`GroundPack`]).
pub const GROUND_PACK_ENV: &str = "NVF_PLAY_TEST_PBR_GROUND_PACK";

/// The generated `_p`'s side, in texels.
pub const GROUND_SIDE: u32 = 256;

/// The generated sets' depth: the end of the scalar range (`fnv_pbr::PARALLAX_DEPTH_RANGE`),
/// the deepest a pack may ask for, so the shift is as large as a real pack's can be.
pub const GROUND_DEPTH: f32 = *fnv_pbr::PARALLAX_DEPTH_RANGE.end();

/// The ridges' spacing and width, in 4x4 blocks of the top level: a ridge every 16 blocks
/// (64 texels, four a texture repeat) two blocks (8 texels) wide, so an eighth of each
/// direction is raised and the rest is low.
const RIDGE_EVERY: u32 = 16;
const RIDGE_WIDTH: u32 = 2;

/// What the ground pack writes for each landscape texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroundPack {
    /// The height-only set.
    Heights,
    /// The height-only set and a diffuse of one flat colour with a checkerboard alpha (#3789).
    Alpha,
    /// No set: a diffuse in two shades whose alpha is the height map (#3486).
    AlphaHeight,
}

/// The ground pack [`GROUND_PACK_ENV`] asks for, if any.
pub fn ground_pack_requested() -> Option<GroundPack> {
    ground_pack_named(&std::env::var_os(GROUND_PACK_ENV)?)
}

fn ground_pack_named(value: &std::ffi::OsStr) -> Option<GroundPack> {
    match value.to_str()? {
        "1" => Some(GroundPack::Heights),
        "alpha" => Some(GroundPack::Alpha),
        "alpha-height" => Some(GroundPack::AlphaHeight),
        _ => None,
    }
}

/// The generated height of the top level's block `(bx, by)`: 255 on a ridge (either
/// direction), else 0.
pub fn ground_height(bx: u32, by: u32) -> u8 {
    let on = |b: u32| b % RIDGE_EVERY < RIDGE_WIDTH;
    if on(bx) || on(by) { 255 } else { 0 }
}

/// The blocks of each level, finest first: the top level from `value` (by block), each smaller
/// one the rounded mean of the top-level blocks it covers (one block for the 4x4 level and
/// below, which BC4 and DXT5 still store as a whole block each).
fn levels(value: fn(u32, u32) -> u8) -> Vec<(u32, Vec<u8>)> {
    let top_blocks = GROUND_SIDE / 4;
    let mut out = Vec::new();
    let mut side = GROUND_SIDE;
    loop {
        let blocks = side.div_ceil(4);
        // How many top-level blocks one block of this level covers along a side.
        let cover = (top_blocks / blocks).max(1);
        let mut values = Vec::with_capacity((blocks * blocks) as usize);
        for by in 0..blocks {
            for bx in 0..blocks {
                let mut sum = 0u32;
                for y in 0..cover {
                    for x in 0..cover {
                        sum += u32::from(value(bx * cover + x, by * cover + y));
                    }
                }
                let n = cover * cover;
                values.push(((sum + n / 2) / n) as u8);
            }
        }
        out.push((side, values));
        if side == 1 {
            return out;
        }
        side /= 2;
    }
}

/// The generated `_p`: a legacy DDS header (FourCC `BC4U`, a linear size and a mip count),
/// then every level's blocks, each block one value (endpoint 0, endpoint 1 and every index 0).
pub fn ground_height_dds() -> Vec<u8> {
    let levels = levels(ground_height);
    let mut b = header(b"BC4U", 8, levels.len());
    for (_, values) in &levels {
        for &v in values {
            b.extend_from_slice(&[v, 0, 0, 0, 0, 0, 0, 0]);
        }
    }
    b
}

/// The legacy DDS header of a [`GROUND_SIDE`] square texture: its FourCC, bytes a block and
/// levels.
fn header(four_cc: &[u8; 4], block: u32, levels: usize) -> Vec<u8> {
    let mut b = vec![0u8; 128];
    b[..4].copy_from_slice(b"DDS ");
    let top_size = (GROUND_SIDE / 4) * (GROUND_SIDE / 4) * block;
    // [documented: Microsoft, "DDS_HEADER": flags CAPS|HEIGHT|WIDTH|PIXELFORMAT (0x1007),
    // MIPMAPCOUNT 0x20000, LINEARSIZE 0x80000; pixel format FOURCC 0x4; caps TEXTURE 0x1000,
    // COMPLEX 0x8, MIPMAP 0x400000]
    for (at, v) in [(4, 124u32), (8, 0x1007 | 0x20000 | 0x80000), (12, GROUND_SIDE), (16, GROUND_SIDE), (20, top_size), (28, levels as u32), (76, 32), (80, 4), (108, 0x1000 | 0x8 | 0x40_0000)] {
        b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    b[84..88].copy_from_slice(four_cc);
    b
}

/// The alpha checkerboard's squares, in 4x4 blocks of the top level: 8 blocks are 32 texels,
/// an eighth of a texture repeat (32 world units at the game's 256).
const BOARD_SQUARE: u32 = 8;

/// The generated diffuse's alpha at the top level's block `(bx, by)`: 0 or 255.
pub fn ground_alpha(bx: u32, by: u32) -> u8 {
    if (bx / BOARD_SQUARE + by / BOARD_SQUARE).is_multiple_of(2) { 255 } else { 0 }
}

/// The flat colour of the `i`th texture as RGB565: hues a golden ratio of the circle apart, and
/// five brightnesses in turn, so layers that lie together differ in both.
fn ground_colour(i: usize) -> u16 {
    let hue = ((i as u32).wrapping_mul(40503) & 0xffff) as f32 / 65536.0;
    let c = Srgba::from(Hsva::hsv(hue * 360.0, 0.6, [0.9, 0.25, 0.6, 0.4, 0.75][i % 5]));
    let part = |x: f32, top: f32| (x * top) as u16;
    part(c.red, 31.0) << 11 | part(c.green, 63.0) << 5 | part(c.blue, 31.0)
}

/// The generated diffuse of the `i`th texture: a legacy DDS header (FourCC `DXT5`, a linear size and a mip
/// count), then every level's blocks [documented: a DXT5 block is an alpha half (two
/// endpoints, 3-bit indices) and a DXT1 colour half (two RGB565 endpoints, 2-bit indices),
/// Microsoft "Texture Block Compression in Direct3D 11"]: the alpha's endpoints both the
/// block's value, the colour's both the texture's colour, every index 0.
pub fn ground_diffuse_dds(i: usize) -> Vec<u8> {
    dxt5(ground_alpha, |_, _| ground_colour(i))
}

/// The `i`th texture's diffuse with a height in its alpha ([`GroundPack::AlphaHeight`]).
pub fn ground_alpha_height_dds(i: usize) -> Vec<u8> {
    let c = ground_colour(i);
    // Half of each channel.
    let dark = (c >> 1) & 0x7bef;
    dxt5(ground_height, |bx, by| if (bx.wrapping_mul(0x9e37_79b1) ^ by.wrapping_mul(0x85eb_ca77)).wrapping_mul(0xc2b2_ae3d) >> 31 == 0 { c } else { dark })
}

/// The generated normal map: straight up (RGB565 16, 32, 31), alpha 0 (no specular mask).
pub fn ground_normal_dds() -> Vec<u8> {
    dxt5(|_, _| 0, |_, _| 16 << 11 | 32 << 5 | 31)
}

/// A DXT5 of the alphas `alpha` (as [`levels`] takes them) and the colours `colour`, by the
/// top level's block (a smaller level's block takes its first one's).
fn dxt5(alpha: fn(u32, u32) -> u8, colour: impl Fn(u32, u32) -> u16) -> Vec<u8> {
    let levels = levels(alpha);
    let mut b = header(b"DXT5", 16, levels.len());
    for (side, values) in &levels {
        let blocks = side.div_ceil(4);
        let cover = (GROUND_SIDE / 4 / blocks).max(1);
        for (k, &v) in (0u32..).zip(values) {
            let [lo, hi] = colour(k % blocks * cover, k / blocks * cover).to_le_bytes();
            b.extend_from_slice(&[v, v, 0, 0, 0, 0, 0, 0, lo, hi, lo, hi, 0, 0, 0, 0]);
        }
    }
    b
}

/// The scalar file of every generated set.
fn scalars() -> Vec<u8> {
    format!("# generated by the ground pack test aid (NVForever)\nparallax_depth = {GROUND_DEPTH}\n").into_bytes()
}

/// The set stems to write for a diffuse: its own and, under an `\xbox\` folder, the `\pc\`
/// twin's (`texture::pc_texture`'s rule). A stem whose path could leave the pack folder (a
/// `..` or `.` component, an empty one, a drive or stream colon) is left out: a plugin's
/// texture path is data, and `fnv_pbr::stem_for` keeps its components as written [design,
/// review of #3427].
fn stems_for(diffuse: &str) -> Vec<String> {
    let Some(stem) = fnv_pbr::stem_for(diffuse) else { return Vec::new() };
    let mut stems = vec![stem.clone()];
    if let Some(i) = stem.find("\\xbox\\") {
        stems.push(format!("{}\\pc\\{}", &stem[..i], &stem[i + "\\xbox\\".len()..]));
    }
    stems.retain(|s| safe_relative(&s.replace('\\', "/")));
    stems
}

/// Whether `rel` (forward slashes) names a place inside the folder it is joined to: every
/// component a plain name (not empty, not `.` or `..`, no colon or backslash), so neither an
/// absolute path, a drive, a stream nor a parent step.
fn safe_relative(rel: &str) -> bool {
    !rel.is_empty() && rel.split('/').all(|c| !c.is_empty() && c != "." && c != ".." && !c.contains([':', '\\']))
}

/// Every generated file for the diffuse paths `diffuses`, by its path under the pack folder
/// (forward slashes), by stem and without repeats (two spellings of one texture share a set):
/// each set's `_p`, then its scalar file, then with [`GroundPack::Alpha`] the diffuse itself
/// and its `_n`.
pub fn ground_fixture<S: AsRef<str>>(pack: GroundPack, diffuses: &[S]) -> Vec<(String, Vec<u8>)> {
    let mut stems: Vec<String> = diffuses.iter().flat_map(|d| stems_for(d.as_ref())).collect();
    stems.sort();
    stems.dedup();
    let (height, scalars) = (ground_height_dds(), scalars());
    let rel = |p: String| p.replace('\\', "/");
    stems
        .into_iter()
        .enumerate()
        .flat_map(|(i, stem)| {
            let mut files = Vec::new();
            if pack != GroundPack::AlphaHeight {
                files.push((rel(fnv_pbr::member_path(&stem, fnv_pbr::Member::Height)), height.clone()));
                files.push((rel(fnv_pbr::member_path(&stem, fnv_pbr::Member::Scalars)), scalars.clone()));
            }
            if pack != GroundPack::Heights {
                if let Some(d) = fnv_pbr::diffuse_stem(&stem) {
                    files.push((rel(format!("{d}.dds")), if pack == GroundPack::Alpha { ground_diffuse_dds(i) } else { ground_alpha_height_dds(i) }));
                    files.push((rel(format!("{d}_n.dds")), ground_normal_dds()));
                }
            }
            files
        })
        .collect()
}

/// Write [`ground_fixture`]'s files for `diffuses` into a fresh folder of this run under the
/// explicit home (`NVFOREVER_HOME`, refused when unset or empty, as the mount pack's):
/// `<home>\pbr-ground-pack\run-<stamp>-<process>-<n>`, reserved by creating it, every file
/// created new. On any failure the run's own folder is removed again and nothing is mounted.
/// The caller mounts the folder over every other loose layer for this run only. The folder,
/// the number of sets and the number of files written.
pub fn seed_ground_pack<S: AsRef<str>>(pack: GroundPack, diffuses: &[S]) -> Result<(PathBuf, usize, usize), String> {
    let home = ground_pack_home(std::env::var_os("NVFOREVER_HOME"))?;
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| format!("clock: {e}"))?.as_nanos();
    seed_ground_pack_in(&home, stamp, std::process::id(), pack, diffuses)
}

/// The home the ground pack may write under: `NVFOREVER_HOME` when it is set and not empty
/// (`fnv_install` takes an empty value for unset, the player's default home), else a refusal.
fn ground_pack_home(value: Option<std::ffi::OsString>) -> Result<PathBuf, String> {
    value.filter(|v| !v.is_empty()).map(PathBuf::from).ok_or_else(|| format!("{GROUND_PACK_ENV}=1 needs an explicit, non-empty NVFOREVER_HOME"))
}

pub(super) fn seed_ground_pack_in<S: AsRef<str>>(home: &Path, stamp: u128, process: u32, pack: GroundPack, diffuses: &[S]) -> Result<(PathBuf, usize, usize), String> {
    let files = ground_fixture(pack, diffuses);
    if files.is_empty() {
        return Err("no landscape texture to write a set for; nothing mounted".into());
    }
    let dir = new_output_in(home, "pbr-ground-pack", stamp, process).map_err(|e| format!("fresh ground pack folder under {}: {e}", home.display()))?;
    match write_files(&dir, &files) {
        Ok(()) => Ok((dir, files.iter().filter(|(rel, _)| rel.ends_with(".toml")).count(), files.len())),
        Err(e) => {
            // The folder is this run's own (just reserved): nothing else is in it.
            let _ = std::fs::remove_dir_all(&dir);
            Err(format!("{}: {e}; nothing mounted", dir.display()))
        }
    }
}

/// `files` under `out`, a folder of the caller's own: every folder made inside it, every file
/// created new.
fn write_files(out: &Path, files: &[(String, Vec<u8>)]) -> std::io::Result<()> {
    // Every path checked before the first write: none may leave `out`.
    let inside = |rel: &str| safe_relative(rel) && Path::new(rel).components().all(|c| matches!(c, std::path::Component::Normal(_)));
    if let Some((rel, _)) = files.iter().find(|(rel, _)| !inside(rel)) {
        return Err(std::io::Error::other(format!("{rel:?} would leave the pack folder; nothing written")));
    }
    for (rel, bytes) in files {
        let path = out.join(rel);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        create_file(&path, bytes)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
