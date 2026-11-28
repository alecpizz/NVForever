//! A drawn texture's texels out of its file a second time (docs/design/rtgi-collect.md): the
//! images themselves go to the GPU and are gone from the main world. Which file an image is
//! ([`TextureCache::sources`]), and of that file the levels that light rays read, as RGBA8
//! ([`read`]). Nothing in the game calls it yet.

use super::{colour_shape_ok, split_cube, TextureCache, TextureUsage};
use crate::data::GameData;
use crate::guard::guarded;
use bevy::image::{ImageAddressMode, ImageSampler};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use fnv_dds::{Dds, Format};
use nvf_rtgi::{Level, Texture, Wrap};
use std::collections::{HashMap, HashSet};
use std::io::ErrorKind;

/// The file a texture of the cache was made from, and as what.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ImageSource {
    /// Its path in the VFS, resolved (`resolved_path`).
    pub path: String,
    pub usage: TextureUsage,
    /// Whether it is the copy with clamped edges.
    pub clamped: bool,
}

/// [design] The most bytes of a file that is read again, stored and decoded: above the 11 MB of
/// the game's own largest (fnv-bsa), for the textures of mods.
pub const MOST_BYTES: usize = 64 << 20;
/// [design] The longest side of a level that is decoded again: 16 MiB as RGBA8. Only a file
/// without smaller levels has none within it.
pub const MOST_SIDE: usize = 2048;

/// What is drawn of a texture, which its file still has to agree with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drawn {
    pub format: TextureFormat,
    pub width: u32,
    pub height: u32,
    pub mips: u32,
    /// Along u and v.
    pub wrap: [Wrap; 2],
}

impl Drawn {
    /// [`Lost::Sampler`] for an image whose sampler is the app's default or clamps to a border:
    /// no texture of a file here has such a one, and what it shows past its edge is not known.
    pub fn of(image: &Image) -> Result<Drawn, Lost> {
        let wrap = |mode| match mode {
            ImageAddressMode::Repeat => Ok(Wrap::Repeat),
            ImageAddressMode::MirrorRepeat => Ok(Wrap::Mirror),
            ImageAddressMode::ClampToEdge => Ok(Wrap::Clamp),
            ImageAddressMode::ClampToBorder => Err(Lost::Sampler),
        };
        let ImageSampler::Descriptor(s) = &image.sampler else { return Err(Lost::Sampler) };
        let (d, wrap) = (&image.texture_descriptor, [wrap(s.address_mode_u)?, wrap(s.address_mode_v)?]);
        Ok(Drawn { format: d.format, width: d.size.width, height: d.size.height, mips: d.mip_level_count, wrap })
    }
}

/// Why a drawn texture's texels are not had again. What is made of it is the caller's to say.
#[derive(Clone, Debug, PartialEq)]
pub enum Lost {
    /// Its file is in no source any more.
    Missing,
    /// The file, or the level that would be read, is over its limit.
    TooLarge(String),
    /// The file is no texture that was drawn so.
    Broken(String),
    /// It is what was drawn, in a format without a decoder to RGBA8 here: BC7, and those of one
    /// and two channels, which are no colours.
    Undecoded(Format),
    /// Its sampler ([`Drawn::of`]).
    Sampler,
}

/// The levels that rays read (`nvf_rtgi::levels_read`) of a texture's file, drawn as `drawn`;
/// `tested`: whether any material with it has an alpha cutoff.
pub fn read(data: &GameData, from: &ImageSource, drawn: &Drawn, tested: bool) -> Result<Texture, Lost> {
    read_within(data, from, drawn, tested, MOST_BYTES)
}

fn read_within(data: &GameData, from: &ImageSource, drawn: &Drawn, tested: bool, most: usize) -> Result<Texture, Lost> {
    // Guarded as the decode is: the test aid's broken files panic in the read.
    match guarded(|| Ok(data.read_at_most(&from.path, most, most))) {
        Ok(Some(Ok(bytes))) => levels(&bytes, from.usage, drawn, tested),
        Ok(Some(Err(e))) => Err(match e.kind() {
            ErrorKind::FileTooLarge => Lost::TooLarge(e.to_string()),
            ErrorKind::NotFound => Lost::Missing,
            _ => Lost::Broken(e.to_string()),
        }),
        Ok(None) => Err(Lost::Missing),
        Err(panic) => Err(Lost::Broken(panic)),
    }
}

/// [`read`] of a file's bytes.
pub fn levels(bytes: &[u8], usage: TextureUsage, drawn: &Drawn, tested: bool) -> Result<Texture, Lost> {
    guarded(|| Ok(levels_unguarded(bytes, usage, drawn, tested))).unwrap_or_else(|panic| Err(Lost::Broken(panic)))
}

fn levels_unguarded(bytes: &[u8], usage: TextureUsage, drawn: &Drawn, tested: bool) -> Result<Texture, Lost> {
    let dds = Dds::parse(bytes).map_err(|e| Lost::Broken(e.to_string()))?;
    let mips = dds.levels[0].len() as u32;
    colour_shape_ok(&dds, mips).map_err(Lost::Broken)?;
    let (width, height) = (dds.header.width, dds.header.height);
    // The GPU takes blocks only where both sides are multiples of 4 (`dds_image_unguarded`).
    let blocks = !drawn.format.is_compressed() || (width.is_multiple_of(4) && height.is_multiple_of(4));
    if (width, height, mips) != (drawn.width, drawn.height, drawn.mips) || !blocks || !drawn_as(dds.format, usage, drawn.format) {
        return Err(Lost::Broken(format!("{} {width}x{height}, {mips} levels; drawn for {usage:?} as {drawn:?}", dds.format.name())));
    }
    if matches!(dds.format, Format::Bc7 | Format::Bc4U | Format::Bc4S | Format::Bc5U | Format::Bc5S) {
        return Err(Lost::Undecoded(dds.format));
    }
    let dims = |m: u32| {
        let (w, h) = dds.header.level_dims(m);
        [w as usize, h as usize]
    };
    let sides: Vec<[usize; 2]> = (0..mips).map(dims).collect();
    let read = nvf_rtgi::levels_read(&sides, tested);
    // All of them before the first is decoded.
    if let Some([w, h]) = read.iter().map(|l| sides[*l]).find(|[w, h]| *w.max(h) > MOST_SIDE) {
        return Err(Lost::TooLarge(format!("its level of {w}x{h} is over the {MOST_SIDE} a side allowed")));
    }
    let level = |l: &usize| -> Result<Level, Lost> {
        let [width, height] = sides[*l];
        let texels = texels(dds.format, drawn.format.is_compressed(), width, height, dds.levels[0][*l]);
        Ok(Level { width, height, texels: texels.ok_or_else(|| Lost::Broken(format!("level {l} is not decoded")))? })
    };
    Ok(Texture { levels: read.iter().map(level).collect::<Result<_, _>>()?, srgb: drawn.format.is_srgb(), wrap: drawn.wrap })
}

/// Whether the loader makes a texture of this format from such a file for this usage
/// (`dds_image_unguarded`), at sides that allow it.
fn drawn_as(file: Format, usage: TextureUsage, drawn: TextureFormat) -> bool {
    use TextureFormat::*;
    use TextureUsage::{Linear, Normal, NormalTwoChannel};
    let made: &[TextureFormat] = match (file, usage) {
        (Format::Dxt1, _) => &[Bc1RgbaUnorm, Rgba8Unorm],
        (Format::Dxt3, _) => &[Bc2RgbaUnorm, Rgba8Unorm],
        (Format::Dxt5, _) => &[Bc3RgbaUnorm, Rgba8Unorm],
        (Format::Bc7, _) => &[Bc7RgbaUnorm],
        (Format::Bc4U, Linear) => &[Bc4RUnorm, R8Unorm],
        (Format::Bc4S, Linear) => &[Bc4RSnorm, R8Snorm],
        (Format::Bc5U, Linear | NormalTwoChannel) => &[Bc5RgUnorm, Rg8Unorm],
        (Format::Bc5S, Linear) => &[Bc5RgSnorm, Rg8Snorm],
        (Format::Bc5S, NormalTwoChannel) => &[Rg8Unorm],
        (Format::Bc5U | Format::Bc5S, Normal) => &[Rgba8Unorm],
        (Format::Bc4U | Format::Bc4S | Format::Bc5U | Format::Bc5S, _) => &[],
        _ => &[Rgba8Unorm],
    };
    made.contains(&drawn.remove_srgb_suffix()) && drawn.is_srgb() == (usage == TextureUsage::Color)
}

#[cfg(test)]
thread_local! {
    /// Levels decoded on this thread.
    static DECODED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// One level as RGBA8. Blocks that the GPU has (`compressed`) by the decoder that made the
/// solver's reference pictures, all else by the loader's, a row (of blocks) at a time.
fn texels(format: Format, compressed: bool, w: usize, h: usize, src: &[u8]) -> Option<Vec<[u8; 4]>> {
    #[cfg(test)]
    DECODED.set(DECODED.get() + 1);
    let code = match format {
        Format::Dxt1 => Some(texpresso::Format::Bc1),
        Format::Dxt3 => Some(texpresso::Format::Bc2),
        Format::Dxt5 => Some(texpresso::Format::Bc3),
        _ => None,
    };
    if let Some(code) = code.filter(|_| compressed) {
        let mut out = vec![[0; 4]; w * h];
        code.decompress(src, w, h, out.as_flattened_mut());
        return Some(out);
    }
    let mut out = Vec::with_capacity(w * h);
    for row in src.chunks_exact(format.level_size(w as u32, 1)) {
        let bytes = match code {
            Some(_) => fnv_dds::decode_blocks(format, w as u32, (h - out.len() / w).min(4) as u32, row)?,
            None => fnv_dds::to_rgba8(format, row)?,
        };
        out.extend_from_slice(bytes.as_chunks::<4>().0);
    }
    (out.len() == w * h).then_some(out)
}

impl TextureCache {
    /// Where these images came from, those that are held here or that a worker decoded. `None`
    /// for a cube map, and for one that two keys claim, which no loader here makes: it is not
    /// known as one flat texture of one file.
    pub fn sources(&self, wanted: &HashSet<AssetId<Image>>) -> HashMap<AssetId<Image>, Option<ImageSource>> {
        let mut out: HashMap<AssetId<Image>, Option<ImageSource>> = HashMap::new();
        let mut add = |(path, usage): &(String, TextureUsage), id: AssetId<Image>, clamped: bool| {
            if !wanted.contains(&id) {
                return;
            }
            let (path, cube) = split_cube(path);
            let source = (!cube).then(|| ImageSource { path: path.into(), usage: *usage, clamped });
            let held = out.entry(id).or_insert_with(|| source.clone());
            if *held != source {
                *held = None;
            }
        };
        for (clamped, map) in [(false, &self.map), (true, &self.clamped)] {
            for (key, (handle, _)) in map.iter().filter_map(|(k, h)| Some((k, h.as_ref()?))) {
                add(key, handle.id(), clamped);
            }
        }
        for (key, w) in self.workers.entries.lock().expect("worker textures").iter() {
            add(key, w.handle.id(), false);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::super::bc5_tests::{dds, OwnedData};
    use super::super::{dds_image, dds_texture, worker_texture, TextureSnapshot};
    use super::*;

    /// Block data of a whole chain, each level of one colour: its red is 8 x the level.
    fn chain(tag: &[u8; 4], w: u32, h: u32, mips: u32) -> Vec<u8> {
        let mut out = Vec::new();
        for m in 0..mips {
            let blocks = (w >> m).max(1).div_ceil(4) * (h >> m).max(1).div_ceil(4);
            let red = (m as u16) << 11;
            let color = [&red.to_le_bytes()[..], &red.to_le_bytes(), &[0; 4]].concat();
            let alpha: &[u8] = match tag {
                b"DXT1" => &[],
                b"DXT3" => &[0x77; 8],
                _ => &[100, 100, 0, 0, 0, 0, 0, 0],
            };
            out.extend([alpha, &color[..]].concat().repeat(blocks as usize));
        }
        dds(tag, w, h, mips, &out)
    }

    /// A8R8G8B8, or R8G8B8 without alpha.
    fn plain(alpha: bool, w: u32, h: u32, mips: u32) -> Vec<u8> {
        let texels: u32 = (0..mips).map(|m| (w >> m).max(1) * (h >> m).max(1)).sum();
        let bytes: Vec<u8> = (0..texels * if alpha { 4 } else { 3 }).map(|i| (i * 37 + 11) as u8).collect();
        let mut out = dds(&[0; 4], w, h, mips, &bytes);
        let (flags, bits) = if alpha { (0x41u32, 32u32) } else { (0x40, 24) };
        for (at, v) in [(80, flags), (88, bits), (92, 0x00FF_0000), (96, 0xFF00), (100, 0xFF), (104, if alpha { 0xFF00_0000 } else { 0 })] {
            out[at..at + 4].copy_from_slice(&v.to_le_bytes());
        }
        out
    }

    /// BC7 of at most two blocks.
    fn bc7(w: u32, h: u32) -> Vec<u8> {
        let header = [98u32, 3, 0, 1, 0].map(u32::to_le_bytes);
        dds(b"DX10", w, h, 1, &[header.as_flattened(), &[0; 32]].concat())
    }

    fn drawn(bytes: &[u8], bc: bool) -> Drawn {
        Drawn::of(&dds_image(bytes, true, bc).unwrap()).unwrap()
    }

    /// Of a colour.
    fn levels(bytes: &[u8], d: &Drawn, tested: bool) -> Result<Texture, Lost> {
        super::levels(bytes, TextureUsage::Color, d, tested)
    }

    fn sides(t: &Texture) -> Vec<[usize; 2]> {
        t.levels.iter().map(|l| [l.width, l.height]).collect()
    }

    /// A level of one colour each, of the three block formats: the levels are the sampler's,
    /// and the 256 one is read only where an alpha test asks. Fails with another level's data.
    #[test]
    fn the_levels_rays_read_come_with_their_texels() {
        for (tag, alpha) in [(b"DXT1", 255), (b"DXT3", 119), (b"DXT5", 100)] {
            for (bc, made) in [(true, 11), (false, 9)] {
                let bytes = chain(tag, 1024, 512, made);
                let d = drawn(&bytes, bc);
                assert_eq!(d.format.is_compressed(), bc);
                for (tested, read) in [(false, vec![4usize]), (true, vec![2, 4])] {
                    let t = levels(&bytes, &d, tested).unwrap();
                    assert_eq!(sides(&t), read.iter().map(|m| [1024usize >> *m, 512 >> *m]).collect::<Vec<_>>());
                    for (l, m) in t.levels.iter().zip(&read) {
                        // Five bits as eight.
                        let red = ((*m << 3) | (*m >> 2)) as u8;
                        assert!(l.texels.len() == l.width * l.height && l.texels.iter().all(|c| *c == [red, 0, 0, alpha]), "{m}: {:?}", l.texels[0]);
                    }
                    assert!(t.srgb && nvf_rtgi::levels_read(&sides(&t), tested).len() == read.len(), "read all");
                }
            }
        }
        // One level, and one just within each lookup's side.
        for (w, h, tested, side) in [(260, 4, true, 260), (260, 4, false, 260), (256, 64, true, 256), (64, 8, true, 64)] {
            let bytes = chain(b"DXT1", w, h, 1);
            assert_eq!(sides(&levels(&bytes, &drawn(&bytes, true), tested).unwrap()), [[side, h as usize]]);
        }
    }

    /// What the loader decodes itself is what it drew: odd sides, a device without BC and the
    /// uncompressed layouts, byte for byte.
    #[test]
    fn what_the_loader_decodes_is_decoded_the_same() {
        let noise = |tag: &[u8; 4], w: u32, h: u32, mips| {
            let mut bytes = chain(tag, w, h, mips);
            for (i, b) in bytes.iter_mut().enumerate().skip(128) {
                *b = (i * 151 + 7) as u8;
            }
            bytes
        };
        let files = [
            (noise(b"DXT1", 6, 5, 3), true),
            (noise(b"DXT3", 7, 9, 1), true),
            (noise(b"DXT5", 8, 8, 4), false),
            (noise(b"DXT1", 64, 32, 6), false),
            (plain(true, 5, 3, 2), true),
            (plain(false, 16, 2, 1), false),
        ];
        for (bytes, bc) in files {
            for srgb in [true, false] {
                let image = dds_image(&bytes, srgb, bc).unwrap();
                let t = super::levels(&bytes, TextureUsage::from_srgb(srgb), &Drawn::of(&image).unwrap(), true).unwrap();
                let [level] = &t.levels[..] else { panic!("one level for both") };
                assert_eq!([level.width as u32, level.height as u32], [image.width(), image.height()]);
                let held = image.data.as_ref().unwrap();
                assert!(level.texels.as_flattened() == &held[..level.texels.len() * 4] && t.srgb == srgb);
            }
        }
    }

    /// Both axes, each its own; none is guessed.
    #[test]
    fn the_wrap_is_the_samplers() {
        let bytes = chain(b"DXT1", 4, 4, 1);
        let mut image = dds_image(&bytes, true, true).unwrap();
        use ImageAddressMode::{ClampToBorder, ClampToEdge, MirrorRepeat, Repeat};
        assert_eq!(levels(&bytes, &Drawn::of(&image).unwrap(), false).unwrap().wrap, [Wrap::Repeat; 2]);
        for (modes, wrap) in [([ClampToEdge, MirrorRepeat], Ok([Wrap::Clamp, Wrap::Mirror])), ([ClampToBorder, Repeat], Err(Lost::Sampler)), ([Repeat, ClampToBorder], Err(Lost::Sampler))] {
            let ImageSampler::Descriptor(s) = &mut image.sampler else { panic!("a sampler of its own") };
            [s.address_mode_u, s.address_mode_v] = modes;
            assert_eq!(Drawn::of(&image).map(|d| levels(&bytes, &d, false).unwrap().wrap), wrap);
        }
        image.sampler = ImageSampler::Default;
        assert_eq!(Drawn::of(&image), Err(Lost::Sampler));
    }

    /// The chosen level's side is limited, not the file's: a larger one with small levels is
    /// read.
    #[test]
    fn a_level_over_the_limit_is_refused() {
        let at = chain(b"DXT1", 2048, 4, 1);
        assert_eq!(sides(&levels(&at, &drawn(&at, true), true).unwrap()), [[2048, 4]]);
        for (w, h) in [(2052, 4), (4, 2052)] {
            let over = chain(b"DXT1", w, h, 1);
            let (over, before) = (&over, DECODED.get());
            let lost = levels(over, &drawn(over, true), false).unwrap_err();
            assert_eq!(DECODED.get(), before, "nothing is decoded first");
            assert_eq!(lost, Lost::TooLarge(format!("its level of {w}x{h} is over the 2048 a side allowed")));
        }
        let long = chain(b"DXT1", 4096, 4, 13);
        assert_eq!(sides(&levels(&long, &drawn(&long, true), true).unwrap()), [[256, 1], [64, 1]]);
    }

    /// Each way to lose a texture is told from the others, and none is a panic.
    #[test]
    fn a_lost_texture_says_how() {
        let whole = chain(b"DXT1", 16, 16, 3);
        let d = drawn(&whole, true);
        let broken = |bytes: &[u8], d: &Drawn| matches!(levels(bytes, d, true), Err(Lost::Broken(_)));
        assert!(levels(&whole, &d, true).is_ok());
        for cut in [0, 3, 127, 128, whole.len() - 1] {
            assert!(broken(&whole[..cut], &d), "cut at {cut}");
        }
        assert!(broken(&[0xA5; 400], &d));
        // Not what was drawn: sides, levels, format.
        for other in [chain(b"DXT1", 16, 8, 3), chain(b"DXT1", 16, 16, 2), chain(b"DXT5", 16, 16, 3), plain(true, 16, 16, 3)] {
            assert!(broken(&other, &d));
        }
        assert!(broken(&plain(true, 4, 4, 1), &drawn(&chain(b"DXT1", 4, 4, 1), true)) && broken(&chain(b"DXT3", 4, 4, 1), &drawn(&chain(b"DXT5", 4, 4, 1), true)));
        // Six faces.
        let mut cube = dds(b"DXT1", 4, 4, 1, &[0; 48]);
        cube[112..116].copy_from_slice(&0xFE00u32.to_le_bytes());
        assert!(broken(&cube, &drawn(&chain(b"DXT1", 4, 4, 1), true)));
        // Formats without a decoder here, as the GPU has them.
        let bc7 = bc7(4, 4);
        assert_eq!(levels(&bc7, &drawn(&bc7, true), true).unwrap_err(), Lost::Undecoded(Format::Bc7));
        assert!(broken(&bc7, &drawn(&chain(b"DXT1", 4, 4, 1), true)), "drawn as another");
    }

    /// Every block format at sides on and off the block grid: taken (BC1 to BC3) or undecoded in
    /// exactly the formats the loader makes of it for a usage, with and without BC support; in
    /// any other, and for a usage it refuses, not what was drawn.
    #[test]
    fn blocks_are_taken_only_as_they_were_drawn() {
        use TextureFormat::*;
        let formats = [Bc4RUnorm, Bc4RSnorm, R8Unorm, R8Snorm, Bc5RgUnorm, Bc5RgSnorm, Rg8Unorm, Rg8Snorm, Rgba8Unorm, Rgba8UnormSrgb, Bc1RgbaUnorm, Bc1RgbaUnormSrgb, Bc2RgbaUnorm, Bc2RgbaUnormSrgb, Bc3RgbaUnorm, Bc3RgbaUnormSrgb, Bc7RgbaUnorm, Bc7RgbaUnormSrgb];
        let (mut undecoded, mut taken) = (0, 0);
        for (w, h) in [(4, 4), (5, 4), (4, 5)] {
            let two = |tag| dds(tag, w, h, 1, &[0; 32]);
            let files = [two(b"DXT1"), two(b"DXT3"), two(b"DXT5"), two(b"BC4U"), two(b"BC4S"), two(b"ATI2"), two(b"BC5S"), bc7(w, h)];
            for (bytes, usage) in files.iter().flat_map(|f| super::super::TEXTURE_USAGES.map(|u| (f, u))) {
                let file = Dds::parse(bytes).unwrap().format;
                let made = [true, false].map(|bc| dds_texture(bytes, usage, bc, false).ok().map(|t| t.image.texture_descriptor.format));
                for format in formats {
                    let d = Drawn { format, width: w, height: h, mips: 1, wrap: [Wrap::Repeat; 2] };
                    let is = match super::levels(bytes, usage, &d, true) {
                        Ok(_) => Some(&mut taken),
                        Err(Lost::Undecoded(f)) if f == file => Some(&mut undecoded),
                        Err(Lost::Broken(_)) => None,
                        Err(lost) => panic!("{lost:?}"),
                    };
                    assert_eq!(is.is_some(), made.contains(&Some(format)), "{w}x{h} {file:?} {usage:?} {format:?}");
                    *is.unwrap_or(&mut 0) += 1;
                }
            }
        }
        // BC4 and BC5 on the grid (2 + 2 + 5 + 4) and off it (1 + 1 + 3 + 3), BC7 on it; BC1 to
        // BC3 as blocks or decoded, off the grid decoded alone.
        assert_eq!((undecoded, taken), (13 + 2 * 8 + 4, 3 * 4 * (2 + 1 + 1)));
    }

    /// Out of the mounted files: the drawn image's own file, one that is gone and one over the
    /// limit, which is its size and not one less.
    #[test]
    fn a_file_is_read_again_within_its_limit() {
        let owned = OwnedData::new();
        let (mut cache, mut images) = (TextureCache::default(), Assets::<Image>::default());
        let image = cache.get("old.dds", true, &owned.data, &mut images, None).unwrap();
        let Some(source) = cache.sources(&HashSet::from([image.id()])).remove(&image.id()).flatten() else { panic!("its source") };
        let d = Drawn::of(images.get(&image).unwrap()).unwrap();
        let t = read(&owned.data, &source, &d, true).unwrap();
        assert_eq!((sides(&t), t.levels[0].texels[0], t.srgb), (vec![[4, 4]], [255; 4], true));
        assert!(read_within(&owned.data, &source, &d, true, 144).is_ok());
        assert!(matches!(read_within(&owned.data, &source, &d, true, 143), Err(Lost::TooLarge(e)) if e.contains("143")));
        let other = |name: &str| ImageSource { path: format!(r"textures\{name}"), ..source.clone() };
        assert_eq!(read(&owned.data, &other("none.dds"), &d, true).unwrap_err(), Lost::Missing);
        assert!(matches!(read(&owned.data, &other("signed.dds"), &d, true), Err(Lost::Broken(_))), "drawn as BC3");
    }

    /// The three places an image is held at: the cache's list, its clamped copies and the
    /// workers' entries, before and after they are taken in. A cube map is refused, and so is
    /// an id of two keys.
    #[test]
    fn a_source_is_found_wherever_the_image_is_held() {
        let owned = OwnedData::new();
        let (mut cache, mut images) = (TextureCache::default(), Assets::<Image>::default());
        let plain = cache.get("old.dds", true, &owned.data, &mut images, None).unwrap();
        let clamped = cache.get_clamped("old.dds", true, &owned.data, &mut images, None).unwrap();
        let worker = worker_texture("signed.dds", TextureUsage::Normal, &owned.data, &TextureSnapshot::new(), &cache.workers, &images.get_handle_provider(), true).unwrap().0;
        // The file holds no cube map, so the key's entry is made by hand.
        let cube = images.add(Image::default());
        cache.map.insert((super::super::cube_path(r"textures\old.dds"), TextureUsage::Color), Some((cube.clone(), false)));
        let (torn, other) = (images.add(Image::default()), images.add(Image::default()));
        for usage in [TextureUsage::Color, TextureUsage::Linear] {
            cache.map.insert((r"textures\torn.dds".into(), usage), Some((torn.clone(), false)));
        }
        cache.map.insert((r"textures\other.dds".into(), TextureUsage::Color), Some((other, false)));
        cache.map.insert((r"textures\gone.dds".into(), TextureUsage::Color), None);
        let wanted = HashSet::from([&plain, &clamped, &worker, &cube, &torn].map(|h| h.id()));
        let at = |path: &str, usage, clamped| Some(ImageSource { path: format!(r"textures\{path}"), usage, clamped });
        let expected = HashMap::from([
            (plain.id(), at("old.dds", TextureUsage::Color, false)),
            (clamped.id(), at("old.dds", TextureUsage::Color, true)),
            (worker.id(), at("signed.dds", TextureUsage::Normal, false)),
            (cube.id(), None),
            (torn.id(), None),
        ]);
        assert_eq!(cache.sources(&wanted), expected);
        cache.take_workers(&mut images, 1.0);
        assert_eq!(cache.sources(&wanted), expected, "held in the list and by the workers");
    }
}
