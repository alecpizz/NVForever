//! DDS → GPU textures. BC1/BC2/BC3 and BC7 retain their mip chains; BC5 data and
//! explicitly requested normal maps have separate interpretations (dds-bc5.md); BC4 is
//! linear data only (dds.md §6).

mod filtering;
pub mod levels;

use crate::data::GameData;
use bevy::asset::RenderAssetUsages;
use bevy::image::{
    CompressedImageFormatSupport, CompressedImageFormats, ImageAddressMode, ImageFilterMode, ImageSampler,
    ImageSamplerDescriptor,
};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension};
use bevy::asset::AssetHandleProvider;
use fnv_dds::{to_rgba8, Dds, Format};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, Mutex, Weak};

/// The interpretation requested by a texture consumer. Linear data is never
/// implicitly treated as a normal map (water also uses it for heights).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextureUsage {
    Color,
    #[default]
    Linear,
    /// The game's normal maps: BC5 is expanded to RGBA on the CPU (dds-bc5.md), so every
    /// result is a three-channel map whose alpha may be a specular mask.
    Normal,
    /// An authored set's `_n` (#2981, docs/spec-notes/pbr-materials.md): unsigned BC5 on the
    /// block grid goes to the GPU as it is (`Bc5RgUnorm`), so Bevy rebuilds Z from the two
    /// channels (`TWO_COMPONENT_NORMAL_MAP`, with the vendored clamp, engine/vendor/README.md);
    /// off the grid or without BC support the two channels are stored as they are in
    /// `Rg8Unorm`, and signed BC5 is moved to 0..1 (`x * 0.5 + 0.5`) in `Rg8Unorm`, the range
    /// Bevy's two-channel path expects. Three-channel files (BC7, BC1-3) load as for
    /// [`TextureUsage::Normal`].
    NormalTwoChannel,
    /// The alpha of a DXT5 file alone, as a height map ([`TextureCache::alpha_height`],
    /// #3486): a cache key of its own, never a way to decode a picture.
    AlphaHeight,
}

impl TextureUsage {
    fn from_srgb(srgb: bool) -> Self {
        if srgb { Self::Color } else { Self::Linear }
    }
}

/// The roles a file is read ahead for ([`TextureCache::offer`]; an alpha height reads its own).
const TEXTURE_USAGES: [TextureUsage; 4] = [TextureUsage::Color, TextureUsage::Linear, TextureUsage::Normal, TextureUsage::NormalTwoChannel];

/// An image and whether its source lacked a specular alpha channel. BC5 normal
/// expansion has a synthesized storage alpha which is not a source mask.
struct DecodedTexture {
    image: Image,
    lacks_alpha: bool,
    /// The file's DX10 header names an `_SRGB` format (a hint only; the role decides).
    srgb_code: bool,
    /// The file's own format, before any decode on the CPU (`None`: not decoded from a DDS
    /// file here). An authored `_p` is judged by it (`crate::authored::note_source`).
    source: Option<Format>,
}

impl From<Image> for DecodedTexture {
    fn from(image: Image) -> Self {
        let lacks_alpha = crate::nif_spawn::format_lacks_alpha(image.texture_descriptor.format);
        Self { image, lacks_alpha, srgb_code: false, source: None }
    }
}

type CachedTextures = HashMap<(String, TextureUsage), Option<(Handle<Image>, bool)>>;

/// Loaded textures by (VFS path, usage); `None` remembers a failure.
/// Each loaded handle retains its source's alpha classification.
#[derive(Resource, Default)]
pub struct TextureCache {
    map: CachedTextures,
    /// Copies sampled with clamped edges ([`Self::get_clamped`]), by the same key.
    clamped: CachedTextures,
    /// Files read ahead on a worker thread ([`Self::offer`]), by resolved path, with when
    /// they came in.
    ahead: HashMap<String, (Vec<u8>, f64)>,
    /// Textures decoded on worker threads for models prepared there
    /// (`ModelCtx::models_ready`), shared with them.
    pub(crate) workers: Arc<WorkerTextures>,
    /// Counts changes to `map`, so a copy of it for worker threads is made again only
    /// when it changed ([`Self::snapshot`]).
    generation: u64,
    pub failures: Vec<String>,
}

impl TextureCache {
    /// The state shared with the worker threads, for a background task to ask a texture's
    /// alpha of ([`WorkerTextures::solid_texels`]).
    pub fn worker_textures(&self) -> Arc<WorkerTextures> {
        self.workers.clone()
    }
}

/// Seconds a texture read ahead waits to be loaded before it is dropped.
const AHEAD_KEEP_SECS: f64 = 30.0;

type TextureLoadGates = HashMap<(String, TextureUsage), Weak<Mutex<()>>>;

/// The textures worker threads decoded, by (resolved path, usage): the handle reserved for
/// each, its format, and the image until the main thread puts it in the image store
/// ([`TextureCache::take_workers`]). A worker that needs a texture another one decoded
/// takes the same handle. Entries stay a while after their image went in, so workers with
/// an older copy of the cache's list ([`TextureCache::snapshot`]) still find them.
#[derive(Default)]
pub struct WorkerTextures {
    entries: Mutex<HashMap<(String, TextureUsage), WorkerTexture>>,
    /// Failed keys remain remembered after their one diagnostic is collected. Worker
    /// snapshots contain successes only, so removing failures would retry them forever.
    failed: Mutex<HashMap<(String, TextureUsage), Option<String>>>,
    /// Only workers loading the same resolved texture wait for each other. Weak entries
    /// do not keep a lock alive after its producer and waiters finish.
    loading: Mutex<TextureLoadGates>,
    /// Whether a texture's alpha is a cut-out mask, by resolved path
    /// ([`Self::alpha_is_mask`]).
    masks: Mutex<HashMap<String, bool>>,
    /// Whether every texel of a texture is opaque, by resolved path
    /// ([`Self::alpha_is_opaque`]).
    opaques: Mutex<HashMap<String, bool>>,
    /// Whether a texture has a solid texel, by resolved path ([`Self::solid_texels`]).
    solids: Mutex<HashMap<String, SolidTexels>>,
    /// Authored material sets' scalar files and warnings (`crate::authored::find`), shared
    /// by the main thread and the workers.
    pub(crate) authored: crate::authored::Memo,
}

struct WorkerTexture {
    handle: Handle<Image>,
    format: TextureFormat,
    lacks_alpha: bool,
    image: Option<Image>,
    /// When the image went into the store (real seconds); `None` while it waits.
    stored_at: Option<f64>,
}

/// Seconds a worker texture's entry stays after its image went into the store.
const WORKER_KEEP_SECS: f64 = 30.0;

/// The textures the cache holds, with their formats, as a worker thread sees them.
pub type TextureSnapshot = HashMap<(String, TextureUsage), (Handle<Image>, TextureFormat, bool)>;

/// A texture for a worker thread: from the cache's list, from another worker, or read and
/// decoded here under a handle reserved from `images` (the image store's handle provider).
/// `None`: not in any archive, or not decodable (a load on the main thread reports why).
pub(crate) fn worker_texture(
    raw_path: &str,
    usage: TextureUsage,
    data: &GameData,
    cached: &TextureSnapshot,
    workers: &WorkerTextures,
    images: &AssetHandleProvider,
    bc_supported: bool,
) -> Option<(Handle<Image>, TextureFormat, bool)> {
    let (raw, cube) = split_cube(raw_path);
    let key = (cube_key(resolved_path(raw, |p| data.vfs.exists(p)), cube), usage);
    worker_texture_key(key, data, cached, workers, images, bc_supported)
}

/// [`worker_texture`] for a file at exactly `path` (normalised only, never turned to another
/// file by [`resolved_path`]): an authored set's member, as the index names it (#2981).
pub(crate) fn worker_texture_exact(
    path: &str,
    usage: TextureUsage,
    data: &GameData,
    cached: &TextureSnapshot,
    workers: &WorkerTextures,
    images: &AssetHandleProvider,
    bc_supported: bool,
) -> Option<(Handle<Image>, TextureFormat, bool)> {
    worker_texture_key((fnv_vfs::normalize(path), usage), data, cached, workers, images, bc_supported)
}

fn worker_texture_key(
    key: (String, TextureUsage),
    data: &GameData,
    cached: &TextureSnapshot,
    workers: &WorkerTextures,
    images: &AssetHandleProvider,
    bc_supported: bool,
) -> Option<(Handle<Image>, TextureFormat, bool)> {
    if let Some(found) = cached.get(&key) {
        return Some(found.clone());
    }
    let usage = key.1;
    workers.load_once(key, images, |key_path| {
        let (path, cube) = split_cube(key_path);
        let bytes = data.read(path).ok_or_else(|| "not in any archive".to_string())?;
        let mut decoded = decode(&bytes, cube, usage, bc_supported)?;
        decoded.image.texture_descriptor.label = Some(folder_label(path));
        crate::authored::srgb_warning(&workers.authored, path, usage, decoded.srgb_code);
        let size = decoded.image.texture_descriptor.size;
        crate::authored::note_source(&workers.authored, path, decoded.source, (size.width, size.height));
        Ok(decoded)
    })
}

impl WorkerTextures {
    fn load_gate(&self, key: &(String, TextureUsage)) -> Arc<Mutex<()>> {
        let mut loading = self.loading.lock().expect("worker texture gates");
        if let Some(gate) = loading.get(key).and_then(Weak::upgrade) {
            return gate;
        }
        // Engineering budget: bound idle key metadata without scanning the map for
        // every texture. Active gates must stay discoverable until all waiters leave.
        if loading.len() >= 256 {
            loading.retain(|_, gate| gate.strong_count() != 0);
        }
        let gate = Arc::new(Mutex::new(()));
        loading.insert(key.clone(), Arc::downgrade(&gate));
        gate
    }

    /// Entries decoded by workers (tests).
    #[cfg(test)]
    pub(crate) fn entries_len(&self) -> usize {
        self.entries.lock().expect("worker textures").len()
    }

    /// Reserve one handle and decode once for concurrent requests for this key. The
    /// directory mutex is never held during I/O/decoding or while waiting on a key.
    fn load_once(
        &self,
        key: (String, TextureUsage),
        images: &AssetHandleProvider,
        load: impl FnOnce(&str) -> Result<DecodedTexture, String>,
    ) -> Option<(Handle<Image>, TextureFormat, bool)> {
        let gate = self.load_gate(&key);
        // The gate carries no data. A panicking producer has published no partial
        // image, so a later request may safely retry under the recovered gate.
        let _loading = gate.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let shared = |w: &WorkerTexture| (w.handle.clone(), w.format, w.lacks_alpha);
        if let Some(w) = self.entries.lock().expect("worker textures").get(&key) {
            return Some(shared(w));
        }
        if self.failed.lock().expect("worker textures").contains_key(&key) {
            return None;
        }
        let decoded = match load(&key.0) {
            Ok(decoded) => decoded,
            Err(error) => {
                self.failed.lock().expect("worker textures").insert(key, Some(error));
                return None;
            }
        };
        let mut entries = self.entries.lock().expect("worker textures");
        let w = entries.entry(key).or_insert_with(|| WorkerTexture {
            handle: images.reserve_handle().typed::<Image>(),
            format: decoded.image.texture_descriptor.format,
            lacks_alpha: decoded.lacks_alpha,
            image: Some(decoded.image),
            stored_at: None,
        });
        Some(shared(w))
    }
}

/// The archive path a NIF texture path loads from: normalised, the PC's own form of it if
/// any ([`pc_texture`]).
pub fn resolved_path(raw_path: &str, exists: impl Fn(&str) -> bool) -> String {
    let path = fnv_vfs::texture_path(raw_path);
    pc_texture(&path, exists).unwrap_or(path)
}

/// Anisotropic filtering of the game's textures made from now on (8 unless the
/// renderer's graphics settings say otherwise: nvforever `graphics.rs`).
static ANISOTROPY: AtomicU16 = AtomicU16::new(8);

/// Set the anisotropic filtering (1, 2, 4, 8 or 16) of textures made from now on.
pub fn set_anisotropy(clamp: u16) {
    ANISOTROPY.store(clamp.clamp(1, 16), Ordering::Relaxed);
}

pub fn sampler() -> ImageSampler {
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: ANISOTROPY.load(Ordering::Relaxed),
        ..ImageSamplerDescriptor::linear()
    })
}

/// Build a GPU image from DDS bytes. DXT1/3/5 are decoded to RGBA when the device
/// cannot sample BC; cube maps and unsupported formats still return an error.
pub fn dds_image(bytes: &[u8], srgb: bool, bc_supported: bool) -> Result<Image, String> {
    // A panic while decoding is this texture's failure, not the game's (guard.rs).
    dds_texture(bytes, TextureUsage::from_srgb(srgb), bc_supported, false).map(|d| d.image)
}

/// [`dds_image`] with the largest level only, its smaller ones left out: for a sheet of
/// pictures cut out by rectangles (a font's glyphs), where a smaller level blends each
/// picture with its neighbours. The base game's font textures have one level
/// [observed: fonts.md §2]; a localized or modded one may have more (#1439).
pub fn dds_image_top(bytes: &[u8], srgb: bool, bc_supported: bool) -> Result<Image, String> {
    dds_texture(bytes, TextureUsage::from_srgb(srgb), bc_supported, true).map(|d| d.image)
}

fn dds_texture(bytes: &[u8], usage: TextureUsage, bc_supported: bool, top_only: bool) -> Result<DecodedTexture, String> {
    crate::guard::guarded(|| dds_image_unguarded(bytes, usage, bc_supported, top_only))
}

fn dds_image_unguarded(bytes: &[u8], usage: TextureUsage, bc_supported: bool, top_only: bool) -> Result<DecodedTexture, String> {
    let srgb = usage == TextureUsage::Color;
    let dds = Dds::parse(bytes).map_err(|e| e.to_string())?;
    let levels = if top_only { &dds.levels[0][..1] } else { &dds.levels[0][..] };
    colour_shape_ok(&dds, levels.len() as u32)?;
    let (w, h) = (dds.header.width, dds.header.height);
    // The GPU takes block formats only when the top level's sides are multiples of 4.
    let aligned = w % 4 == 0 && h % 4 == 0;
    // The colour space follows the role, never a DX10 header's `_SRGB` code (#2981,
    // dds.md §6): legacy files state none, and the hint is only reported.
    let (format, data) = match dds.format {
        Format::Dxt1 | Format::Dxt3 | Format::Dxt5 => {
            if !bc_supported || !aligned {
                // Reuse the bounded DDS decoder when BC sampling is unavailable, or
                // dimensions cannot use a block texture (dds.md §3, #2164). Keep all
                // parsed mip levels and the caller's diffuse/linear colour intent.
                let mut out = Vec::new();
                for (m, l) in levels.iter().enumerate() {
                    let (lw, lh) = dds.header.level_dims(m as u32);
                    out.extend(fnv_dds::decode_blocks(dds.format, lw, lh, l).ok_or("block data too short")?);
                }
                (if srgb { TextureFormat::Rgba8UnormSrgb } else { TextureFormat::Rgba8Unorm }, out)
            } else {
                let f = match (dds.format, srgb) {
                    (Format::Dxt1, true) => TextureFormat::Bc1RgbaUnormSrgb,
                    (Format::Dxt1, false) => TextureFormat::Bc1RgbaUnorm,
                    (Format::Dxt3, true) => TextureFormat::Bc2RgbaUnormSrgb,
                    (Format::Dxt3, false) => TextureFormat::Bc2RgbaUnorm,
                    (_, true) => TextureFormat::Bc3RgbaUnormSrgb,
                    (_, false) => TextureFormat::Bc3RgbaUnorm,
                };
                (f, levels.concat())
            }
        }
        // BC7 has no CPU decoder in the tree: it goes to the GPU as it is or not at all.
        Format::Bc7 => {
            if !aligned {
                return Err(format!("BC7 needs sides that are multiples of 4 ({w}x{h}); it is not decoded on the CPU"));
            }
            if !bc_supported {
                return Err("GPU lacks BC texture support (BC7 is not decoded on the CPU)".into());
            }
            (if srgb { TextureFormat::Bc7RgbaUnormSrgb } else { TextureFormat::Bc7RgbaUnorm }, levels.concat())
        }
        // One channel: linear data only (a height or a mask), never a colour or a normal.
        Format::Bc4U | Format::Bc4S => {
            if usage != TextureUsage::Linear {
                return Err(format!("BC4 has one channel and no {} layout; request linear data", if srgb { "sRGB color" } else { "normal-map" }));
            }
            let signed = dds.format == Format::Bc4S;
            if bc_supported && aligned {
                (if signed { TextureFormat::Bc4RSnorm } else { TextureFormat::Bc4RUnorm }, levels.concat())
            } else {
                let mut out = Vec::new();
                for (m, level) in levels.iter().enumerate() {
                    let (lw, lh) = dds.header.level_dims(m as u32);
                    let values = fnv_dds::decode_bc4(dds.format, lw, lh, level).ok_or("invalid BC4 block data")?;
                    if signed {
                        out.extend(values.into_iter().map(|v| (v * 127.0).round() as i8 as u8));
                    } else {
                        out.extend(values.into_iter().map(|v| (v * 255.0).round() as u8));
                    }
                }
                (if signed { TextureFormat::R8Snorm } else { TextureFormat::R8Unorm }, out)
            }
        }
        Format::Bc5U | Format::Bc5S => {
            if srgb {
                return Err("BC5 has no sRGB color layout; request linear data or a normal map".into());
            }
            let signed = dds.format == Format::Bc5S;
            if usage == TextureUsage::NormalTwoChannel {
                if !signed && bc_supported && aligned {
                    (TextureFormat::Bc5RgUnorm, levels.concat())
                } else {
                    let mut out = Vec::new();
                    for (m, level) in levels.iter().enumerate() {
                        let (lw, lh) = dds.header.level_dims(m as u32);
                        let pixels = fnv_dds::decode_bc5(dds.format, lw, lh, level).ok_or("invalid BC5 block data")?;
                        for [r, g] in pixels {
                            // Unsigned: the channels as stored; signed (-1..1): moved to 0..1.
                            let (r, g) = if signed { (r * 0.5 + 0.5, g * 0.5 + 0.5) } else { (r, g) };
                            out.extend([(r.clamp(0.0, 1.0) * 255.0).round() as u8, (g.clamp(0.0, 1.0) * 255.0).round() as u8]);
                        }
                    }
                    (TextureFormat::Rg8Unorm, out)
                }
            } else if usage != TextureUsage::Normal && bc_supported && aligned {
                (if signed { TextureFormat::Bc5RgSnorm } else { TextureFormat::Bc5RgUnorm }, levels.concat())
            } else {
                let mut out = Vec::new();
                for (m, level) in levels.iter().enumerate() {
                    let (lw, lh) = dds.header.level_dims(m as u32);
                    let pixels = fnv_dds::decode_bc5(dds.format, lw, lh, level).ok_or("invalid BC5 block data")?;
                    for [r, g] in pixels {
                        if usage == TextureUsage::Normal {
                            let [x, y] = if signed { [r, g] } else { [r * 2.0 - 1.0, g * 2.0 - 1.0] };
                            // Expand here to use Bevy's three-component path, as every
                            // other game normal map does (the authored `_n` role keeps two
                            // channels: `NormalTwoChannel`).
                            let z = (1.0 - x * x - y * y).max(0.0).sqrt();
                            out.extend([normal_byte(x), normal_byte(y), normal_byte(z), 255]);
                        } else if signed {
                            out.extend([(r * 127.0).round() as i8 as u8, (g * 127.0).round() as i8 as u8]);
                        } else {
                            out.extend([(r * 255.0).round() as u8, (g * 255.0).round() as u8]);
                        }
                    }
                }
                let format = if usage == TextureUsage::Normal { TextureFormat::Rgba8Unorm }
                    else if signed { TextureFormat::Rg8Snorm } else { TextureFormat::Rg8Unorm };
                (format, out)
            }
        }
        other => {
            let mut out = Vec::new();
            for l in levels {
                out.extend(to_rgba8(other, l).ok_or("unsupported layout")?);
            }
            (if srgb { TextureFormat::Rgba8UnormSrgb } else { TextureFormat::Rgba8Unorm }, out)
        }
    };
    let mut img = checked_image(w, h, levels.len() as u32, format, data)?;
    img.sampler = sampler();
    // No specular mask in the alpha: a format without alpha (BC4 and BC5 also when
    // expanded or decoded on the CPU) or a DX10 header stating every alpha opaque. Every
    // successful path, the CPU-decoded DXT one included, ends here. A DX10 BC1 keeps its
    // source's classification when decoded to RGBA8 off the block grid, as it has when
    // uploaded as blocks; a legacy odd-sized DXT1 keeps main's classification (alpha
    // read as a mask) unchanged.
    let lacks_alpha = dds.alpha_opaque()
        || matches!(dds.format, Format::Bc5U | Format::Bc5S | Format::Bc4U | Format::Bc4S)
        || (dds.format == Format::Dxt1 && dds.header.dx10.is_some())
        || crate::nif_spawn::format_lacks_alpha(format);
    Ok(DecodedTexture { image: img, lacks_alpha, srgb_code: dds.srgb_hint() == Some(true), source: Some(dds.format) })
}

fn normal_byte(value: f32) -> u8 {
    ((value + 1.0) * 0.5 * 255.0).round() as u8
}

/// The shape a colour texture must have for this loader (and the cloud volume, which follows
/// it: nvforever cloud_shadows.rs `volume_alpha_of`): a plain 2D texture, not a cube map; its
/// sides within [`MAX_TEXTURE_SIDE`]; no more of its `mips` levels than its size allows (as
/// [`checked_image`] checks them, before anything is decoded).
pub fn colour_shape_ok(dds: &Dds, mips: u32) -> Result<(), String> {
    if dds.header.is_cubemap() {
        return Err("cube map (not used by the viewer)".into());
    }
    let (w, h) = (dds.header.width, dds.header.height);
    if w == 0 || h == 0 || w > MAX_TEXTURE_SIDE || h > MAX_TEXTURE_SIDE {
        return Err(format!("size {w}x{h} out of range (1 to {MAX_TEXTURE_SIDE})"));
    }
    let most = 32 - w.max(h).leading_zeros();
    if mips == 0 || mips > most {
        return Err(format!("{mips} mip levels for a {w}x{h} texture (at most {most})"));
    }
    Ok(())
}

/// The end of a texture path that asks for the file as a cube map ([`cube_path`]): the
/// texture caches key a cube map by its path with this added, so a file loaded as a cube
/// map is never handed out for a flat texture slot, nor the other way round.
const CUBE_KEY: &str = "#cube";

/// The path to pass to a texture source (`TextureSource::texture`, [`TextureCache::get`])
/// to have the file at a NIF texture path loaded as a cube map ([`dds_cube_image`]): the
/// environment cube of texture-set slot 4 (docs/spec-notes/nif.md §6 "Window glass and
/// environment maps").
pub fn cube_path(raw_path: &str) -> String {
    format!("{raw_path}{CUBE_KEY}")
}

/// Why a slot-4 file that is a flat texture (no cube-map flag) is not loaded as a cube
/// map ([`dds_cube_image`]). A file that says it is a cube map but does not hold six
/// faces fails with another message, a warning ([`texture_failed`]).
const NOT_A_CUBE: &str = "a flat texture, not a cube map";

/// Report a texture that did not load (`key`: the cache key, a cube map's with
/// [`CUBE_KEY`]). A slot-4 file that is a flat texture, not a cube map (6 shapes of the
/// base and DLC models, docs/spec-notes/nif.md §6 "Environment maps on other shapes"),
/// is the model's own data, not a broken file: one info line per file, kept out of the
/// failure list, and its shape is drawn without the environment map. Every other failure
/// is a warning, as before (`guard::failed`).
fn texture_failed(failures: &mut Vec<String>, key: &str, error: &str) {
    match split_cube(key) {
        (file, true) if error == NOT_A_CUBE => info!("environment map not drawn: {file} is a flat texture, not a cube map"),
        _ => crate::guard::failed(failures, "texture", format!("{key}: {error}")),
    }
}

/// A requested path and whether it asks for a cube map ([`cube_path`]).
fn split_cube(raw_path: &str) -> (&str, bool) {
    raw_path.strip_suffix(CUBE_KEY).map_or((raw_path, false), |p| (p, true))
}

/// The cache key of a resolved path, loaded as a cube map or not.
fn cube_key(path: String, cube: bool) -> String {
    if cube {
        path + CUBE_KEY
    } else {
        path
    }
}

/// The file's image for a cache key: a cube map ([`dds_cube_image`]) for a key that asks
/// for one, else [`dds_texture`] for the usage.
fn decode(bytes: &[u8], cube: bool, usage: TextureUsage, bc_supported: bool) -> Result<DecodedTexture, String> {
    if cube {
        dds_cube_image(bytes, usage == TextureUsage::Color, bc_supported).map(DecodedTexture::from)
    } else {
        dds_texture(bytes, usage, bc_supported, false)
    }
}

/// Build a cube-map GPU image from DDS bytes: the six faces in the file's order (+X, -X,
/// +Y, -Y, +Z, -Z [documented: Microsoft's DDS documentation, "Cubic Environment Maps"]),
/// each with its mip chain, as the six layers of a texture viewed as a cube. An error for a
/// file that is not a cube map with all six faces. Block-compressed faces are decoded when
/// the device cannot sample them or their side is not a multiple of 4, as in [`dds_image`].
pub fn dds_cube_image(bytes: &[u8], srgb: bool, bc_supported: bool) -> Result<Image, String> {
    crate::guard::guarded(|| dds_cube_image_unguarded(bytes, srgb, bc_supported))
}

fn dds_cube_image_unguarded(bytes: &[u8], srgb: bool, bc_supported: bool) -> Result<Image, String> {
    let dds = Dds::parse(bytes).map_err(|e| e.to_string())?;
    if !dds.header.is_cubemap() {
        return Err(NOT_A_CUBE.into());
    }
    if dds.levels.len() != 6 {
        return Err(format!("a cube map with {} face(s), not six", dds.levels.len()));
    }
    let (w, h) = (dds.header.width, dds.header.height);
    if w != h {
        return Err(format!("cube faces of {w}x{h} are not square"));
    }
    let mips = dds.levels[0].len() as u32;
    let block = matches!(dds.format, Format::Dxt1 | Format::Dxt3 | Format::Dxt5);
    let compressed = block && bc_supported && w % 4 == 0;
    let mut data = Vec::new();
    for face in &dds.levels {
        if face.len() as u32 != mips {
            return Err("cube faces with different mip counts".into());
        }
        for (m, l) in face.iter().enumerate() {
            if compressed {
                data.extend_from_slice(l);
            } else if block {
                let (lw, lh) = dds.header.level_dims(m as u32);
                data.extend(fnv_dds::decode_blocks(dds.format, lw, lh, l).ok_or("block data too short")?);
            } else {
                data.extend(to_rgba8(dds.format, l).ok_or("unsupported layout")?);
            }
        }
    }
    let format = match (compressed, dds.format, srgb) {
        (true, Format::Dxt1, true) => TextureFormat::Bc1RgbaUnormSrgb,
        (true, Format::Dxt1, false) => TextureFormat::Bc1RgbaUnorm,
        (true, Format::Dxt3, true) => TextureFormat::Bc2RgbaUnormSrgb,
        (true, Format::Dxt3, false) => TextureFormat::Bc2RgbaUnorm,
        (true, _, true) => TextureFormat::Bc3RgbaUnormSrgb,
        (true, _, false) => TextureFormat::Bc3RgbaUnorm,
        (false, _, true) => TextureFormat::Rgba8UnormSrgb,
        (false, _, false) => TextureFormat::Rgba8Unorm,
    };
    let mut img = checked_image_layers(w, h, mips, 6, format, data)?;
    img.texture_view_descriptor = Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..default() });
    img.sampler = sampler();
    Ok(img)
}

/// The largest side of a texture taken from a file. Above every texture the game ships
/// (4096 at most [observed]) and within what desktop GPUs take; a larger size in a file
/// is a broken header.
pub const MAX_TEXTURE_SIDE: u32 = 16384;

/// A 2D image of `w`×`h` with `mips` levels whose bytes are `data`, or why the GPU would
/// refuse it. Bevy closes the game on a texture the GPU refuses (a zero or oversized
/// side, more levels than the size allows), and the render thread panics when the data
/// is shorter than the size says; so every image made from a file's numbers is built
/// here (docs/design/broken-files.md).
pub fn checked_image(w: u32, h: u32, mips: u32, format: TextureFormat, data: Vec<u8>) -> Result<Image, String> {
    checked_image_layers(w, h, mips, 1, format, data)
}

/// [`checked_image`] with `layers` layers, each with its whole mip chain in turn.
fn checked_image_layers(w: u32, h: u32, mips: u32, layers: u32, format: TextureFormat, data: Vec<u8>) -> Result<Image, String> {
    if w == 0 || h == 0 || w > MAX_TEXTURE_SIDE || h > MAX_TEXTURE_SIDE {
        return Err(format!("size {w}x{h} out of range (1 to {MAX_TEXTURE_SIDE})"));
    }
    let most = 32 - w.max(h).leading_zeros();
    if mips == 0 || mips > most {
        return Err(format!("{mips} mip levels for a {w}x{h} texture (at most {most})"));
    }
    let (bw, bh) = format.block_dimensions();
    let block = format.block_copy_size(None).ok_or("format has no single block size")? as usize;
    let expected: usize = (0..mips).map(|m| ((w >> m).max(1).div_ceil(bw) * (h >> m).max(1).div_ceil(bh)) as usize * block).sum::<usize>() * layers as usize;
    if data.len() != expected {
        return Err(format!("{} bytes of data for a {w}x{h} texture of {mips} levels ({expected} expected)", data.len()));
    }
    // Construct uninitialized so `mip_level_count` is set alongside full mip-chain `data`
    // without triggering Bevy `Image::new`'s single-mip byte-count check on uncompressed formats.
    let mut img = Image::new_uninit(Extent3d { width: w, height: h, depth_or_array_layers: layers }, TextureDimension::D2, format, RenderAssetUsages::RENDER_WORLD);
    img.texture_descriptor.mip_level_count = mips;
    img.texture_descriptor.label = Some("dds texture");
    img.data = Some(data);
    Ok(img)
}

/// The GPU label of a game texture: its folder under `textures\` (`textures\clutter`),
/// so the GPU allocator's report (nvforever `stress_sample.rs`) tells what kinds of
/// texture it holds. One string per folder, made once.
fn folder_label(path: &str) -> &'static str {
    static LABELS: Mutex<Option<HashMap<String, &'static str>>> = Mutex::new(None);
    let mut parts = path.split('\\');
    let folder = match (parts.next(), parts.next(), parts.next()) {
        (Some(a), Some(b), Some(_)) => format!("{a}\\{b}"),
        _ => "textures".to_string(),
    };
    let mut labels = LABELS.lock().expect("texture labels");
    labels.get_or_insert_with(HashMap::new).entry(folder).or_insert_with_key(|k| Box::leak(k.clone().into_boxed_str()))
}

/// The PC's own form of a texture a model names in a console's folder: the same path
/// with `\pc\` for `\xbox\`, when that file exists. [observed: the Pip-Boy models
/// (`pipboy3000\pipboyarm.nif` and the rest) name `textures\pipboy3000\xbox\english\
/// PipBoyArm01.dds`, whose buttons are labelled with the console's LT and RT, and
/// `pipboy3000\pc\english\pipboyarm01.dds` lies beside it, as do `ps3` and `ps3o`
/// forms; the owner's screenshots of the PC game show no such labels. hypothesis: the
/// PC game reads the `pc` folder's file; docs/spec-notes/pipboy.md §1]
pub fn pc_texture(path: &str, exists: impl Fn(&str) -> bool) -> Option<String> {
    let lower = path.to_ascii_lowercase();
    let i = lower.find("\\xbox\\")?;
    let pc = format!("{}\\pc\\{}", &path[..i], &path[i + "\\xbox\\".len()..]);
    exists(&pc).then_some(pc)
}

/// The first-person form of a texture a model names: the same folder with `1stperson`
/// before the file name, when that file exists. [observed: `meshes\weapons\2handrifle\
/// laserrifle.nif`, the only laser rifle model, names `LaserRifle01.dds` (128x128), and
/// `1stpersonlaserrifle01.dds` (1024x1024) lies beside it; 413 of the 455 `1stperson`
/// textures in the archives, all weapons', have such a plain twin and no model names
/// them; the owner's screenshot of the game shows the gun's small print sharp in first
/// person. hypothesis: the game draws the first-person weapon with these;
/// docs/spec-notes/first-person-textures.md §3]
pub fn first_person_texture(path: &str, exists: impl Fn(&str) -> bool) -> Option<String> {
    let path = fnv_vfs::texture_path(path);
    if is_first_person_texture(&path) {
        return None;
    }
    let file = path.rfind('\\').map_or(0, |i| i + 1);
    let first = format!("{}1stperson{}", &path[..file], &path[file..]);
    exists(&first).then_some(first)
}

/// Whether a texture's file name is in its first-person form (starts `1stperson`).
pub fn is_first_person_texture(path: &str) -> bool {
    let path = fnv_vfs::texture_path(path);
    let file = path.rfind('\\').map_or(0, |i| i + 1);
    path[file..].starts_with("1stperson")
}

impl TextureCache {
    /// Drop the textures nothing else holds (no live material uses them), after their
    /// grace time (`crate::release`); the number dropped. Keys: (path, usage, clamped).
    /// The textures held, for the stress census: an entry is idle when only the cache
    /// holds its image (it waits out its keep time).
    pub fn share(&self) -> crate::release::CacheShare {
        let mut s = crate::release::CacheShare::default();
        for (h, _) in self.map.values().chain(self.clamped.values()).flatten() {
            s.entries += 1;
            s.idle += usize::from(!crate::release::held_beyond(h, 1));
            s.images.insert(h.id());
        }
        s
    }

    /// Drop the textures nothing has used for `keep.secs` ([`KeepLimits`](crate::release::KeepLimits)):
    /// counted from when the model or part that last held one went unused, where `hints`
    /// says (release.rs [`ImageHints`](crate::release::ImageHints)).
    pub fn release(&mut self, idle: &mut crate::release::Unused<(String, TextureUsage, bool)>, hints: &crate::release::ImageHints, now: f64, keep: crate::release::KeepLimits) -> usize {
        use crate::release::held_beyond;
        let mut ids = HashMap::new();
        let entries: Vec<_> = [(false, &self.map), (true, &self.clamped)]
            .into_iter()
            .flat_map(|(clamp, map)| map.iter().filter_map(move |((p, usage), h)| h.as_ref().map(|(h, _)| ((p.clone(), *usage, clamp), h))))
            .map(|(k, h)| {
                ids.insert(k.clone(), h.id());
                (k, held_beyond(h, 1))
            })
            .collect();
        self.ahead.retain(|_, (_, at)| now - *at < AHEAD_KEEP_SECS);
        // A worker may lose the canonical-key race while its distinct handle is
        // still sampled by a material. Keep that handle discoverable only while
        // an external owner holds it; canonical copies retain their usual grace.
        let canonical: std::collections::HashSet<_> = ids.values().copied().collect();
        self.workers.entries.lock().expect("worker textures").retain(|_, w| {
            w.stored_at.is_none_or(|t| now - t < WORKER_KEEP_SECS)
                || (!canonical.contains(&w.handle.id()) && held_beyond(&w.handle, 1))
        });
        let drop = idle.sweep_timed(now, entries, keep.secs, keep.max, |k| ids.get(k).and_then(|&id| hints.since(id)));
        for ((p, usage, clamp), _) in &drop {
            (if *clamp { &mut self.clamped } else { &mut self.map }).remove(&(p.clone(), *usage));
        }
        self.generation += u64::from(!drop.is_empty());
        drop.len()
    }

    /// Put the images worker threads decoded into the image store and the cache (a texture
    /// the main thread loaded meanwhile keeps its own; the worker's copy stays for the
    /// materials made with it); the number put in. `now`: real seconds.
    pub fn take_workers(&mut self, images: &mut Assets<Image>, now: f64) -> usize {
        let mut n = 0;
        let mut entries = self.workers.entries.lock().expect("worker textures");
        for (key, w) in entries.iter_mut() {
            let Some(mut image) = w.image.take() else { continue };
            filtering::publish(&mut image);
            if images.insert(w.handle.id(), image).is_ok() {
                self.map.entry(key.clone()).or_insert_with(|| Some((w.handle.clone(), w.lacks_alpha)));
                n += 1;
            }
            w.stored_at = Some(now);
        }
        drop(entries);
        for (key, error) in self.workers.failed.lock().expect("worker textures").iter_mut() {
            let Some(e) = error.take() else { continue };
            texture_failed(&mut self.failures, &key.0, &e);
            self.map.entry(key.clone()).or_insert(None);
        }
        self.generation += u64::from(n > 0);
        n
    }

    /// The textures held now with their formats, for worker threads (`ModelCtx`
    /// keeps the copy while it is unchanged and in use).
    pub fn snapshot(&self, images: &Assets<Image>) -> TextureSnapshot {
        self.map
            .iter()
            .filter_map(|(k, h)| {
                let (h, lacks_alpha) = h.as_ref()?;
                Some((k.clone(), (h.clone(), images.get(h)?.texture_descriptor.format, *lacks_alpha)))
            })
            .collect()
    }

    /// Changes to the cache so far ([`Self::snapshot`]).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The bytes of the texture file at `path` (resolved: [`resolved_path`]), read ahead on
    /// a worker thread, for the first load of it; kept unless it is loaded already.
    pub fn offer(&mut self, path: String, bytes: Vec<u8>, now: f64) {
        let loaded = TEXTURE_USAGES.iter().any(|&usage| self.map.contains_key(&(path.clone(), usage)) || self.clamped.contains_key(&(path.clone(), usage)));
        if !loaded {
            self.ahead.insert(path, (bytes, now));
        }
    }

    /// Whether the texture at the resolved path `path` is loaded (or failed to load), being
    /// put in from a worker thread, or its file read ahead ([`Self::offer`]): loading it
    /// reads no file.
    pub fn has_or_ahead(&self, path: &str) -> bool {
        let key = |usage: TextureUsage| (path.to_string(), usage);
        self.ahead.contains_key(path)
            || TEXTURE_USAGES.iter().any(|&usage| self.map.contains_key(&key(usage)) || self.workers.entries.lock().expect("worker textures").contains_key(&key(usage)))
    }

    /// Textures loaded and held now (failures not counted).
    pub fn len(&self) -> usize {
        self.map.values().chain(self.clamped.values()).filter(|h| h.is_some()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Load (once) the texture at a NIF texture path (the PC's own form of it, if any:
    /// [`pc_texture`]).
    pub fn get(
        &mut self,
        raw_path: &str,
        srgb: bool,
        data: &GameData,
        images: &mut Assets<Image>,
        formats: Option<&CompressedImageFormatSupport>,
    ) -> Option<Handle<Image>> {
        self.load(raw_path, TextureUsage::from_srgb(srgb), false, data, images, formats).map(|(h, _)| h)
    }

    /// Like [`Self::get`], a copy of the texture whose sampler clamps at its edges
    /// instead of repeating (the grass atlas, nvforever `grass.rs`).
    pub fn get_clamped(
        &mut self,
        raw_path: &str,
        srgb: bool,
        data: &GameData,
        images: &mut Assets<Image>,
        formats: Option<&CompressedImageFormatSupport>,
    ) -> Option<Handle<Image>> {
        self.load(raw_path, TextureUsage::from_srgb(srgb), true, data, images, formats).map(|(h, _)| h)
    }

    /// Load for a material normal slot, returning the source's no-alpha flag.
    /// Synthesized BC5 storage alpha must not become a specular mask.
    pub fn normal(
        &mut self,
        raw_path: &str,
        data: &GameData,
        images: &mut Assets<Image>,
        formats: Option<&CompressedImageFormatSupport>,
    ) -> Option<(Handle<Image>, bool)> {
        self.load(raw_path, TextureUsage::Normal, false, data, images, formats)
    }

    /// Load (once) the file at exactly `path` in role `usage`: normalised only, never turned
    /// to another file by [`resolved_path`]. For an authored set's members, by the path the
    /// index names (#2981).
    pub(crate) fn exact(
        &mut self,
        path: &str,
        usage: TextureUsage,
        data: &GameData,
        images: &mut Assets<Image>,
        formats: Option<&CompressedImageFormatSupport>,
    ) -> Option<(Handle<Image>, bool)> {
        self.load_key(fnv_vfs::normalize(path), usage, false, data, images, formats)
    }

    /// The deepest an authored set's height map may be drawn, in texture-coordinate units: the
    /// end of the scalar range (`fnv_pbr::PARALLAX_DEPTH_RANGE`), for marches that share one
    /// step grid across sets (nvforever terrain.rs `PARALLAX_SCALE`).
    pub const AUTHORED_HEIGHT_MAX_DEPTH: f32 = *fnv_pbr::PARALLAX_DEPTH_RANGE.end();

    /// The authored height map (`_p`) for a texture drawn outside the NIF shapes, with its
    /// depth (UV units): the set the index finds for the texture's resolved path, as a shape's
    /// diffuse finds it, of which only the `_p` is loaded (`crate::authored::find_height`).
    /// For the landscape's texture layers (nvforever terrain.rs "Parallax", #3322) [design].
    /// `None` while the run's Authored materials state is off (`crate::authored::enabled`),
    /// for a texture no set may take (`crate::authored::key_eligible`), without a set or
    /// without a height map to draw.
    pub fn authored_height(
        &mut self,
        raw_path: &str,
        data: &GameData,
        images: &mut Assets<Image>,
        formats: Option<&CompressedImageFormatSupport>,
    ) -> Option<(Handle<Image>, f32)> {
        self.authored_height_with(crate::authored::enabled(), raw_path, data, images, formats)
    }

    /// [`Self::authored_height`] with the setting's state given (the tests').
    pub(crate) fn authored_height_with(
        &mut self,
        enabled: bool,
        raw_path: &str,
        data: &GameData,
        images: &mut Assets<Image>,
        formats: Option<&CompressedImageFormatSupport>,
    ) -> Option<(Handle<Image>, f32)> {
        let key = resolved_path(raw_path, |p| data.vfs.exists(p));
        if !crate::authored::key_eligible(&key) {
            return None;
        }
        let workers = self.workers.clone();
        let height = crate::authored::find_height(data, enabled, &key, &workers.authored, |path, usage| {
            let (h, _) = self.exact(path, usage, data, images, formats)?;
            let format = images.get(&h).map(|i| i.texture_descriptor.format);
            Some((h, format))
        })?;
        Some((height.depth_map?, height.depth_scale))
    }

    /// A landscape diffuse's alpha as a height map, for texture packs in the convention of
    /// the original's terrain parallax shaders (#3486; docs/spec-notes/pbr-materials.md
    /// "Heights from a ground pack's diffuse alpha") [design], and its depth
    /// ([`ALPHA_HEIGHT_DEPTH`]): `None` without one, while the run's Authored materials state
    /// is off, and whenever the authored index knows an active set for the diffuse
    /// (`PbrIndex::lookup`), with a `_p` or not: the set alone decides then. The caller asks
    /// only for landscape layers, before their picture is loaded (the file may be read ahead).
    pub fn alpha_height(
        &mut self,
        raw_path: &str,
        data: &GameData,
        images: &mut Assets<Image>,
        formats: Option<&CompressedImageFormatSupport>,
    ) -> Option<(Handle<Image>, f32)> {
        self.alpha_height_with(crate::authored::enabled(), raw_path, data, images, formats)
    }

    /// [`Self::alpha_height`] with the setting's state given (the tests').
    pub(crate) fn alpha_height_with(
        &mut self,
        enabled: bool,
        raw_path: &str,
        data: &GameData,
        images: &mut Assets<Image>,
        formats: Option<&CompressedImageFormatSupport>,
    ) -> Option<(Handle<Image>, f32)> {
        let path = resolved_path(raw_path, |p| data.vfs.exists(p));
        if !enabled || !crate::authored::key_eligible(&path) || data.pbr_index().lookup(&path).is_some() || formats.is_some_and(|f| !f.0.contains(CompressedImageFormats::BC)) {
            return None;
        }
        let key = (path, TextureUsage::AlphaHeight);
        if let Some(h) = self.map.get(&key) {
            return h.clone().map(|(h, _)| (h, ALPHA_HEIGHT_DEPTH));
        }
        // The file may be read ahead for its picture: looked at, not taken.
        let bytes = self.ahead.get(&key.0).map(|(b, _)| std::borrow::Cow::Borrowed(b.as_slice())).or_else(|| data.read(&key.0).map(std::borrow::Cow::Owned));
        let found = match bytes.as_deref().map_or(Ok(None), |b| crate::guard::guarded(|| alpha_height_image(b))) {
            Ok(image) => image.map(|mut image| {
                image.texture_descriptor.label = Some(folder_label(&key.0));
                (images.add(image), true)
            }),
            // A layout left out: said once.
            Err(e) => {
                bevy::log::info_once!("landscape heights: {} {e}; such textures stay flat (the only one said)", key.0);
                None
            }
        };
        self.map.insert(key, found.clone());
        self.generation += 1;
        found.map(|(h, _)| (h, ALPHA_HEIGHT_DEPTH))
    }

    fn load(
        &mut self,
        raw_path: &str,
        usage: TextureUsage,
        clamp: bool,
        data: &GameData,
        images: &mut Assets<Image>,
        formats: Option<&CompressedImageFormatSupport>,
    ) -> Option<(Handle<Image>, bool)> {
        let (raw, cube) = split_cube(raw_path);
        let path = cube_key(resolved_path(raw, |p| data.vfs.exists(p)), cube);
        self.load_key(path, usage, clamp, data, images, formats)
    }

    fn load_key(
        &mut self,
        path: String,
        usage: TextureUsage,
        clamp: bool,
        data: &GameData,
        images: &mut Assets<Image>,
        formats: Option<&CompressedImageFormatSupport>,
    ) -> Option<(Handle<Image>, bool)> {
        if let Some(h) = (if clamp { &self.clamped } else { &self.map }).get(&(path.clone(), usage)) {
            return h.clone();
        }
        // Decoded on a worker thread: its handle (and its image, if still waiting).
        if !clamp {
            let mut entries = self.workers.entries.lock().expect("worker textures");
            if let Some(w) = entries.get_mut(&(path.clone(), usage)) {
                if let Some(mut image) = w.image.take() {
                    filtering::publish(&mut image);
                    let _ = images.insert(w.handle.id(), image);
                    w.stored_at = Some(0.0);
                }
                let h = (w.handle.clone(), w.lacks_alpha);
                drop(entries);
                self.map.insert((path, usage), Some(h.clone()));
                self.generation += 1;
                return Some(h);
            }
            drop(entries);
            // Negative-cache hit from a worker thread: skip repeating archive scans on the
            // main thread for a texture already proven missing or undecodable [design].
            let mut failed = self.workers.failed.lock().expect("worker textures");
            if let Some(err) = failed.get_mut(&(path.clone(), usage)) {
                let err = err.take();
                drop(failed);
                if let Some(e) = err {
                    texture_failed(&mut self.failures, &path, &e);
                }
                self.map.insert((path, usage), None);
                self.generation += 1;
                return None;
            }
        }
        let bc = formats.is_none_or(|f| f.0.contains(CompressedImageFormats::BC));
        let read_span = bevy::log::info_span!("texture::read").entered();
        // The file behind the key: a cube map's key is its path with [`CUBE_KEY`] added.
        let (file, cube) = split_cube(&path);
        let read = self.ahead.remove(file).map(|(bytes, _)| bytes).or_else(|| data.read(file));
        drop(read_span);
        let decode_span = bevy::log::info_span!("texture::decode").entered();
        let result = match read {
            None => Err("not in any archive".to_string()),
            Some(bytes) => decode(&bytes, cube, usage, bc),
        };
        drop(decode_span);
        let handle = match result {
            Ok(DecodedTexture { mut image, lacks_alpha, srgb_code, source }) => {
                image.texture_descriptor.label = Some(folder_label(file));
                crate::authored::srgb_warning(&self.workers.authored, file, usage, srgb_code);
                let size = image.texture_descriptor.size;
                crate::authored::note_source(&self.workers.authored, file, source, (size.width, size.height));
                if clamp {
                    if let ImageSampler::Descriptor(d) = &mut image.sampler {
                        d.address_mode_u = ImageAddressMode::ClampToEdge;
                        d.address_mode_v = ImageAddressMode::ClampToEdge;
                    }
                }
                Some((images.add(image), lacks_alpha))
            }
            Err(e) => {
                texture_failed(&mut self.failures, &path, &e);
                None
            }
        };
        (if clamp { &mut self.clamped } else { &mut self.map }).insert((path, usage), handle.clone());
        self.generation += 1;
        handle
    }
}

/// How a texture's alpha is spread over its top level: the shares of texels that are clear
/// (alpha 8 or less of 255), opaque (247 or more) and in between.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AlphaShares {
    pub clear: f32,
    pub opaque: f32,
    pub partial: f32,
}

impl AlphaShares {
    /// A cut-out mask: a part of the texture is clear and nearly all the rest opaque, the
    /// in-between texels being only the edges between them. The game's corn
    /// (`textures\landscape\plants\maize_d.dds`) is 32% clear, 60% opaque, 8% between
    /// [observed]; dirty glass and smoke are mostly in between.
    pub fn is_mask(self) -> bool {
        self.clear >= MASK_MIN_CLEAR && self.partial <= MASK_MAX_PARTIAL
    }
}

/// Least clear share of a cut-out mask [engineering choice: a texture with no hole is no
/// cut-out].
pub const MASK_MIN_CLEAR: f32 = 0.02;
/// Most in-between share of a cut-out mask [engineering choice from the survey of blended
/// alpha-tested shapes, docs/design/fsr-upscaling.md "Blended cut-outs": plants, lattices and
/// grave flowers lie at or under 0.12 (glass shards 0.12, corn 0.08); the next are hair at
/// 0.14 (excluded otherwise), explosion debris at 0.15, translucent glasses at 0.26 and up].
pub const MASK_MAX_PARTIAL: f32 = 0.13;

/// The alpha shares of a DDS file's top level, read from at most about 16,000 of its
/// texels' blocks spread over it. `None` for files that do not parse and for layouts
/// without alpha.
pub fn alpha_shares(bytes: &[u8]) -> Option<AlphaShares> {
    let dds = Dds::parse(bytes).ok()?;
    let top = *dds.levels.first()?.first()?;
    let (w, h) = (dds.header.width as usize, dds.header.height as usize);
    let mut counts = [0usize; 3];
    let mut add = |a: u8| counts[if a <= 8 { 0 } else if a >= 247 { 1 } else { 2 }] += 1;
    match dds.format {
        Format::Dxt1 | Format::Dxt3 | Format::Dxt5 => {
            let size = dds.format.block_bytes()?;
            let blocks = (w.div_ceil(4) * h.div_ceil(4)).min(top.len() / size);
            let step = blocks.div_ceil(16_384).max(1);
            for b in top.chunks_exact(size).take(blocks).step_by(step) {
                for a in block_alpha(dds.format, b) {
                    add(a);
                }
            }
        }
        Format::A8R8G8B8 | Format::A4R4G4B4 | Format::A8B8G8R8 | Format::A1R5G5B5 | Format::A8L8 => {
            let rgba = to_rgba8(dds.format, top)?;
            let texels = rgba.len() / 4;
            let step = texels.div_ceil(262_144).max(1);
            for p in rgba.as_chunks::<4>().0.iter().step_by(step) {
                add(p[3]);
            }
        }
        // No alpha, or (BC7) no CPU decoder: no alpha information.
        Format::R8G8B8 | Format::X8R8G8B8 | Format::X8B8G8R8 | Format::R5G6B5 | Format::X1R5G5B5 | Format::L8
        | Format::Bc5U | Format::Bc5S | Format::Bc4U | Format::Bc4S | Format::Bc7 => return None,
    }
    let n = counts.iter().sum::<usize>().max(1) as f32;
    Some(AlphaShares { clear: counts[0] as f32 / n, opaque: counts[1] as f32 / n, partial: counts[2] as f32 / n })
}

/// The depth of a height read from a diffuse's alpha, in texture-coordinate units: one value
/// for every pack, which none can set [design, #3486: clean at 0.03 and 0.06 and shifting too
/// far at 0.08 on Sandstone Desert, jojokengly's measurements].
pub const ALPHA_HEIGHT_DEPTH: f32 = 0.03;

/// The least an alpha must vary by, in levels of 255, to be a height: an opaque or constant
/// one is none [design].
const ALPHA_HEIGHT_RANGE: u8 = 16;

/// The alpha of a DXT5 file as a BC4 image of the same size and levels, its blocks copied
/// without decoding: a DXT5 (BC3) block's first 8 bytes are laid out as a BC4 block
/// [documented: Microsoft, "Texture Block Compression in Direct3D 11", BC3 and BC4]. `None`
/// when the alpha does not vary, when the file has none (DXT1, a layout without alpha) and for a
/// file that does not parse (its own load as a picture reports it); an error for a layout whose
/// alpha is not read (the first version reads DXT5 only; a BC7 is not looked into).
fn alpha_height_image(bytes: &[u8]) -> Result<Option<Image>, String> {
    let Ok(dds) = Dds::parse(bytes) else { return Ok(None) };
    let (w, h) = (dds.header.width, dds.header.height);
    match dds.format {
        Format::Dxt5 if dds.header.is_cubemap() || w % 4 != 0 || h % 4 != 0 => return Err(format!("is a DXT5 of {w}x{h} whose alpha is not read as a height (no cube, sides in multiples of 4)")),
        Format::Dxt5 => {}
        Format::Dxt3 | Format::Bc7 | Format::A8R8G8B8 | Format::A8B8G8R8 | Format::A4R4G4B4 | Format::A8L8 => {
            if dds.alpha_opaque() || alpha_all_opaque(bytes) == Some(true) {
                return Ok(None);
            }
            return Err(format!("is {:?}, whose alpha is not read as a height (DXT5 is)", dds.format));
        }
        _ => return Ok(None),
    }
    if dds.alpha_opaque() {
        return Ok(None);
    }
    let levels = &dds.levels[0];
    // Judged on the largest level of at most 256 texels a side (the smallest one if none is).
    let at = (0..levels.len()).find(|&i| { let (lw, lh) = dds.header.level_dims(i as u32); lw.max(lh) <= 256 }).unwrap_or(levels.len() - 1);
    let (mut least, mut most) = (255u8, 0u8);
    for b in levels[at].as_chunks::<16>().0 {
        for a in block_alpha(Format::Dxt5, b) {
            least = least.min(a);
            most = most.max(a);
        }
    }
    if most.saturating_sub(least) < ALPHA_HEIGHT_RANGE {
        return Ok(None);
    }
    let mut data = Vec::with_capacity(levels.iter().map(|l| l.len() / 2).sum());
    for l in levels {
        for b in l.as_chunks::<16>().0 {
            data.extend_from_slice(&b[..8]);
        }
    }
    let mut image = checked_image(w, h, levels.len() as u32, TextureFormat::Bc4RUnorm, data)?;
    image.sampler = sampler();
    Ok(Some(image))
}

/// Whether every texel of a DDS file's top level is opaque (alpha 255), all of them
/// looked at: a layout without alpha is, a DX10 header stating opaque alpha is, and one
/// with alpha is when no texel has less. `None` for a file that does not parse and for
/// BC7 without that statement (no CPU decoder). Unlike [`alpha_shares`] nothing is
/// sampled: one clear texel in a large texture is a hole someone drew.
pub fn alpha_all_opaque(bytes: &[u8]) -> Option<bool> {
    let dds = Dds::parse(bytes).ok()?;
    if dds.alpha_opaque() {
        return Some(true);
    }
    let top = *dds.levels.first()?.first()?;
    let (w, h) = (dds.header.width as usize, dds.header.height as usize);
    match dds.format {
        Format::Dxt1 | Format::Dxt3 | Format::Dxt5 => {
            let size = dds.format.block_bytes()?;
            let blocks = (w.div_ceil(4) * h.div_ceil(4)).min(top.len() / size);
            Some(top.chunks_exact(size).take(blocks).all(|b| block_alpha(dds.format, b).iter().all(|&a| a == 255)))
        }
        Format::A8R8G8B8 | Format::A4R4G4B4 | Format::A8B8G8R8 | Format::A1R5G5B5 | Format::A8L8 => {
            let rgba = to_rgba8(dds.format, top)?;
            Some(rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 255))
        }
        Format::R8G8B8 | Format::X8R8G8B8 | Format::X8B8G8R8 | Format::R5G6B5 | Format::X1R5G5B5 | Format::L8
        | Format::Bc5U | Format::Bc5S | Format::Bc4U | Format::Bc4S => Some(true),
        Format::Bc7 => None,
    }
}

/// The least alpha (of 255) of a solid texel (250 / 255 = 0.98, engineering choice).
pub const SOLID_ALPHA: u8 = 250;

/// How solid a texture is, for a blended shape's solid core (`nif_spawn::solid_core`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SolidTexels {
    /// No solid texel: no core.
    None,
    /// Some solid texels, or an alpha that cannot be read here (BC7): a core of the solid texels
    /// alone, so a thin part elsewhere in the texture stays see-through.
    Some,
    /// Of the texels a lower cut would take (`nif_spawn::SOLID_CORE_CUT`), all but a quarter at
    /// most are solid: the rest are the edges of what is solid (a border, a filtered seam).
    Mostly,
}

impl SolidTexels {
    /// The core's alpha cut, `None` for no core.
    pub fn cut(self) -> Option<f32> {
        match self {
            SolidTexels::None => None,
            SolidTexels::Some => Some(crate::nif_spawn::SOLID_CORE_CUT_THIN),
            SolidTexels::Mostly => Some(crate::nif_spawn::SOLID_CORE_CUT),
        }
    }

    /// From how many texels the lower cut would take and how many of them are solid.
    fn of(taken: usize, solid: usize) -> SolidTexels {
        match solid {
            0 => SolidTexels::None,
            n if n * 4 >= taken * 3 => SolidTexels::Mostly,
            _ => SolidTexels::Some,
        }
    }
}

/// How solid a DDS file's top level is ([`SolidTexels`]). A layout without alpha is mostly
/// solid (a shape with one is drawn opaque already: `nif_spawn::blend_is_opaque`). BC7 cannot be
/// told here (no CPU decoder). `None` for a file that does not parse.
pub fn alpha_solidity(bytes: &[u8]) -> Option<SolidTexels> {
    let dds = Dds::parse(bytes).ok()?;
    if dds.alpha_opaque() {
        return Some(SolidTexels::Mostly);
    }
    let top = *dds.levels.first()?.first()?;
    let (w, h) = (dds.header.width as usize, dds.header.height as usize);
    let least = (crate::nif_spawn::SOLID_CORE_CUT * 255.0).ceil() as u8;
    let (mut taken, mut solid) = (0usize, 0usize);
    let mut count = |a: u8| {
        taken += usize::from(a >= least);
        solid += usize::from(a >= SOLID_ALPHA);
    };
    match dds.format {
        Format::Dxt1 | Format::Dxt3 | Format::Dxt5 => {
            let size = dds.format.block_bytes()?;
            let blocks = (w.div_ceil(4) * h.div_ceil(4)).min(top.len() / size);
            let across = w.div_ceil(4).max(1);
            for (i, b) in top.chunks_exact(size).take(blocks).enumerate() {
                // A block past the image's edge holds padding texels.
                let (x, y) = (i % across * 4, i / across * 4);
                for (k, &a) in block_alpha(dds.format, b).iter().enumerate() {
                    if x + k % 4 < w && y + k / 4 < h {
                        count(a);
                    }
                }
            }
        }
        Format::A8R8G8B8 | Format::A4R4G4B4 | Format::A8B8G8R8 | Format::A1R5G5B5 | Format::A8L8 => {
            let rgba = to_rgba8(dds.format, top)?;
            rgba.as_chunks::<4>().0.iter().for_each(|p| count(p[3]));
        }
        Format::R8G8B8 | Format::X8R8G8B8 | Format::X8B8G8R8 | Format::R5G6B5 | Format::X1R5G5B5 | Format::L8
        | Format::Bc5U | Format::Bc5S | Format::Bc4U | Format::Bc4S => return Some(SolidTexels::Mostly),
        Format::Bc7 => return Some(SolidTexels::Some),
    }
    Some(SolidTexels::of(taken, solid))
}

/// A texture's top-level alpha, one byte per texel, rows from the top.
#[derive(Clone, Debug, PartialEq)]
pub struct AlphaMap {
    pub width: usize,
    pub height: usize,
    pub alpha: Vec<u8>,
}

/// The top-level alpha of a DDS file. `None` for files that do not parse and for layouts
/// without alpha.
pub fn alpha_map(bytes: &[u8]) -> Option<AlphaMap> {
    alpha_map_of(&Dds::parse(bytes).ok()?)
}

/// [`alpha_map`] of a texture already parsed (one parse for the reader and its checks).
pub fn alpha_map_of(dds: &Dds) -> Option<AlphaMap> {
    let top = *dds.levels.first()?.first()?;
    let (width, height) = (dds.header.width as usize, dds.header.height as usize);
    if width == 0 || height == 0 {
        return None;
    }
    // Formats without alpha information are refused before the map is allocated.
    match dds.format {
        Format::Dxt1 | Format::Dxt3 | Format::Dxt5
        | Format::A8R8G8B8 | Format::A4R4G4B4 | Format::A8B8G8R8 | Format::A1R5G5B5 | Format::A8L8 => {}
        // No alpha, or (BC7) no CPU decoder.
        Format::R8G8B8 | Format::X8R8G8B8 | Format::X8B8G8R8 | Format::R5G6B5 | Format::X1R5G5B5 | Format::L8
        | Format::Bc5U | Format::Bc5S | Format::Bc4U | Format::Bc4S | Format::Bc7 => return None,
    }
    let mut alpha = vec![0u8; width * height];
    match dds.format {
        Format::Dxt1 | Format::Dxt3 | Format::Dxt5 => {
            let size = dds.format.block_bytes()?;
            let across = width.div_ceil(4);
            let blocks = (across * height.div_ceil(4)).min(top.len() / size);
            for (n, b) in top.chunks_exact(size).take(blocks).enumerate() {
                let (bx, by) = (n % across * 4, n / across * 4);
                for (i, a) in block_alpha(dds.format, b).into_iter().enumerate() {
                    let (x, y) = (bx + i % 4, by + i / 4);
                    if x < width && y < height {
                        alpha[y * width + x] = a;
                    }
                }
            }
        }
        Format::A8R8G8B8 | Format::A4R4G4B4 | Format::A8B8G8R8 | Format::A1R5G5B5 | Format::A8L8 => {
            let rgba = to_rgba8(dds.format, top)?;
            for (a, p) in alpha.iter_mut().zip(rgba.as_chunks::<4>().0) {
                *a = p[3];
            }
        }
        _ => return None,
    }
    Some(AlphaMap { width, height, alpha })
}

impl AlphaMap {
    /// The part of a rectangular cell of the texture that holds every texel above
    /// `clear` alpha, widened by `pad` texels: `[s0, s1, t0, t1]` in the cell's own
    /// coordinates, s from its corner `origin` towards `s_end`, t towards `t_end`, each
    /// 0..1 (texture coordinates, 0..1 over the texture, wrapping). `None` when the cell
    /// holds nothing above `clear`. The cell's edges must follow the texture's axes.
    pub fn kept_part(&self, origin: [f32; 2], s_end: [f32; 2], t_end: [f32; 2], clear: u8, pad: f32) -> Option<[f32; 4]> {
        let (w, h) = (self.width as f32, self.height as f32);
        let lo = [origin[0].min(s_end[0]).min(t_end[0]), origin[1].min(s_end[1]).min(t_end[1])];
        let hi = [origin[0].max(s_end[0]).max(t_end[0]), origin[1].max(s_end[1]).max(t_end[1])];
        let (ds, dt) = ([s_end[0] - origin[0], s_end[1] - origin[1]], [t_end[0] - origin[0], t_end[1] - origin[1]]);
        let det = ds[0] * dt[1] - ds[1] * dt[0];
        if det.abs() < 1e-12 {
            return None;
        }
        // Texels whose centres lie in the cell.
        let (x0, x1) = ((lo[0] * w - 0.5).ceil() as i64, (hi[0] * w - 0.5).floor() as i64);
        let (y0, y1) = ((lo[1] * h - 0.5).ceil() as i64, (hi[1] * h - 0.5).floor() as i64);
        let mut kept: Option<[f32; 4]> = None;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (tx, ty) = (x.rem_euclid(self.width as i64) as usize, y.rem_euclid(self.height as i64) as usize);
                if self.alpha[ty * self.width + tx] <= clear {
                    continue;
                }
                let (u, v) = ((x as f32 + 0.5) / w - origin[0], (y as f32 + 0.5) / h - origin[1]);
                let s = (u * dt[1] - v * dt[0]) / det;
                let t = (ds[0] * v - ds[1] * u) / det;
                let k = kept.get_or_insert([s, s, t, t]);
                *k = [k[0].min(s), k[1].max(s), k[2].min(t), k[3].max(t)];
            }
        }
        // A texel's own half and the pad, in cell coordinates.
        let s_texels = (ds[0] * w).abs().max((ds[1] * h).abs()).max(1.0);
        let t_texels = (dt[0] * w).abs().max((dt[1] * h).abs()).max(1.0);
        kept.map(|[s0, s1, t0, t1]| {
            let (ps, pt) = ((0.5 + pad) / s_texels, (0.5 + pad) / t_texels);
            [(s0 - ps).max(0.0), (s1 + ps).min(1.0), (t0 - pt).max(0.0), (t1 + pt).min(1.0)]
        })
    }
}

/// The 16 alphas of one DXT block [documented: the DXT1/3/5 block layouts; fnv-dds].
fn block_alpha(format: Format, b: &[u8]) -> [u8; 16] {
    match format {
        // Colour 0 not above colour 1: the fourth index is transparent black.
        Format::Dxt1 => {
            let (c0, c1) = (u16::from_le_bytes([b[0], b[1]]), u16::from_le_bytes([b[2], b[3]]));
            let bits = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
            std::array::from_fn(|i| if c0 <= c1 && (bits >> (2 * i)) & 3 == 3 { 0 } else { 255 })
        }
        Format::Dxt3 => std::array::from_fn(|i| ((b[i / 2] >> (4 * (i % 2))) & 15) * 17),
        _ => {
            let (a0, a1) = (b[0] as u32, b[1] as u32);
            let pal: [u32; 8] = if a0 > a1 {
                std::array::from_fn(|k| match k { 0 => a0, 1 => a1, k => ((8 - k as u32) * a0 + (k as u32 - 1) * a1) / 7 })
            } else {
                std::array::from_fn(|k| match k { 0 => a0, 1 => a1, 6 => 0, 7 => 255, k => ((6 - k as u32) * a0 + (k as u32 - 1) * a1) / 5 })
            };
            let bits = b[2..8].iter().rev().fold(0u64, |acc, &x| (acc << 8) | u64::from(x));
            std::array::from_fn(|i| pal[((bits >> (3 * i)) & 7) as usize] as u8)
        }
    }
}

impl WorkerTextures {
    /// Whether the texture at a NIF texture path has a cut-out mask in its alpha
    /// ([`AlphaShares::is_mask`]). Each file is read and its alpha counted once, then
    /// remembered by resolved path for the main thread and the workers alike: many shapes
    /// share a texture.
    pub fn alpha_is_mask(&self, raw_path: &str, data: &GameData) -> bool {
        let path = resolved_path(raw_path, |p| data.vfs.exists(p));
        remembered(&self.masks, path, |path| data.read(path).and_then(|bytes| alpha_shares(&bytes)).is_some_and(|s| s.is_mask()))
    }

    /// Whether every texel of the texture at a NIF texture path is opaque
    /// ([`alpha_all_opaque`]); false for a file that is missing or cannot be told. Read
    /// once per file and remembered, as [`Self::alpha_is_mask`].
    pub fn alpha_is_opaque(&self, raw_path: &str, data: &GameData) -> bool {
        let path = resolved_path(raw_path, |p| data.vfs.exists(p));
        remembered(&self.opaques, path, |path| data.read(path).and_then(|bytes| alpha_all_opaque(&bytes)).unwrap_or(false))
    }

    /// Whether the texture at a NIF texture path is solid ([`alpha_solidity`]), how far;
    /// false for a file that is missing or does not parse. Reads the file: call it on a task
    /// (nvforever `solid_cores.rs` does, and only while cores are wanted). Read once per
    /// file and remembered, as [`Self::alpha_is_mask`].
    pub fn solid_texels(&self, raw_path: &str, data: &GameData) -> SolidTexels {
        let path = resolved_path(raw_path, |p| data.vfs.exists(p));
        remembered(&self.solids, path, |path| data.read(path).and_then(|bytes| alpha_solidity(&bytes)).unwrap_or(SolidTexels::None))
    }
}

/// `compute`'s answer for `key`, worked out the first time only. The map is not held while
/// it is worked out, so two threads asking at once may both compute it, to the same answer.
fn remembered<T: Copy>(known: &Mutex<HashMap<String, T>>, key: String, compute: impl FnOnce(&str) -> T) -> T {
    if let Some(&answer) = known.lock().expect("alpha masks").get(&key) {
        return answer;
    }
    let answer = compute(&key);
    known.lock().expect("alpha masks").insert(key, answer);
    answer
}

#[cfg(test)]
mod tests {
    use super::first_person_texture;
    use super::{alpha_shares, dds_image, remembered, AlphaShares, TextureFormat};
    use std::collections::HashMap;
    use std::sync::Mutex;

    fn owned_worker_image(srgb: bool) -> super::DecodedTexture {
        super::dds_image(&dds(b"DXT1", 4, 4, &[0; 8]), srgb, true).unwrap().into()
    }

    #[test]
    fn worker_texture_contenders_decode_once_and_share_the_uploaded_handle() {
        use std::sync::{atomic::{AtomicUsize, Ordering}, mpsc};
        use std::time::{Duration, Instant};
        let mut cache = super::TextureCache::default();
        let mut images = bevy::prelude::Assets::<bevy::prelude::Image>::default();
        let provider = images.get_handle_provider();
        let key = ("textures\\owned.dds".to_string(), super::TextureUsage::Color);
        let attempts = AtomicUsize::new(0);
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let handles = std::thread::scope(|scope| {
            let workers = &*cache.workers;
            let (key_ref, provider_ref, attempts_ref) = (&key, &provider, &attempts);
            let producer = scope.spawn(move || workers.load_once(key_ref.clone(), provider_ref, |_| {
                attempts_ref.fetch_add(1, Ordering::Relaxed);
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                Ok(owned_worker_image(true))
            }));
            entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            let gate = workers.load_gate(&key);
            let consumers: Vec<_> = (0..7).map(|_| scope.spawn(|| workers.load_once(key.clone(), &provider, |_| {
                attempts.fetch_add(1, Ordering::Relaxed);
                Ok(owned_worker_image(true))
            }))).collect();
            // The producer and all seven consumers must have reached the same key's
            // gate before it is released, rather than testing eight sequential hits.
            let deadline = Instant::now() + Duration::from_secs(5);
            while std::sync::Arc::strong_count(&gate) != 9 && Instant::now() < deadline {
                std::thread::yield_now();
            }
            let reached = std::sync::Arc::strong_count(&gate);
            release_tx.send(()).unwrap();
            let mut handles = vec![producer.join().unwrap().unwrap()];
            handles.extend(consumers.into_iter().map(|t| t.join().unwrap().unwrap()));
            assert_eq!(reached, 9, "all requests must contend with the held producer");
            handles
        });
        assert_eq!(attempts.load(Ordering::Relaxed), 1);
        assert!(handles.iter().all(|h| *h == handles[0]));
        assert_eq!(cache.take_workers(&mut images, 1.0), 1);
        assert!(images.get(&handles[0].0).is_some());
        assert_eq!(cache.snapshot(&images)[&key], handles[0]);
        assert_eq!(cache.take_workers(&mut images, 2.0), 0);
    }

    #[test]
    fn worker_texture_different_keys_make_progress_while_one_is_loading() {
        use std::sync::mpsc;
        use std::time::Duration;
        let workers = super::WorkerTextures::default();
        let images = bevy::prelude::Assets::<bevy::prelude::Image>::default().get_handle_provider();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        std::thread::scope(|scope| {
            let (workers_ref, images_ref) = (&workers, &images);
            let first = scope.spawn(move || workers_ref.load_once(("first.dds".into(), super::TextureUsage::Color), images_ref, |_| {
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                Ok(owned_worker_image(true))
            }));
            entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            // Its producer releases the first: a single global decode lock deadlocks
            // until the bounded channel timeout and fails this regression.
            let other = workers.load_once(("other.dds".into(), super::TextureUsage::Color), &images, |_| {
                release_tx.send(()).unwrap();
                Ok(owned_worker_image(true))
            }).unwrap();
            assert_ne!(first.join().unwrap().unwrap().0, other.0);
        });
    }

    #[test]
    fn worker_texture_failure_stays_cached_after_its_diagnostic_is_collected() {
        let mut cache = super::TextureCache::default();
        let mut images = bevy::prelude::Assets::<bevy::prelude::Image>::default();
        let provider = images.get_handle_provider();
        let key = ("textures\\missing-owned.dds".to_string(), super::TextureUsage::Color);
        assert!(cache.workers.load_once(key.clone(), &provider, |_| Err("owned missing texture".into())).is_none());
        assert_eq!(cache.take_workers(&mut images, 1.0), 0);
        assert_eq!(cache.failures.len(), 1);
        assert!(cache.workers.load_once(key, &provider, |_| panic!("stale success-only snapshots must not retry failed textures")).is_none());
        cache.take_workers(&mut images, 2.0);
        assert_eq!(cache.failures.len(), 1, "each failure is reported only once");
    }

    #[test]
    fn worker_texture_color_spaces_keep_distinct_handles_and_formats() {
        let workers = super::WorkerTextures::default();
        let images = bevy::prelude::Assets::<bevy::prelude::Image>::default().get_handle_provider();
        let color = workers.load_once(("owned.dds".into(), super::TextureUsage::Color), &images, |_| Ok(owned_worker_image(true))).unwrap();
        let linear = workers.load_once(("owned.dds".into(), super::TextureUsage::Linear), &images, |_| Ok(owned_worker_image(false))).unwrap();
        assert_ne!(color.0, linear.0);
        assert_ne!(color.1, linear.1);
    }

    #[test]
    fn worker_texture_gate_pruning_keeps_active_keys_and_releases_idle_keys() {
        let workers = super::WorkerTextures::default();
        let key = ("active.dds".to_string(), super::TextureUsage::Color);
        let held = workers.load_gate(&key);
        for n in 0..1024 {
            drop(workers.load_gate(&(format!("idle{n}.dds"), super::TextureUsage::Color)));
        }
        let found = workers.load_gate(&key);
        assert!(std::sync::Arc::ptr_eq(&held, &found));
        assert!(workers.loading.lock().unwrap().len() <= 256);
    }


    #[test]
    fn the_kept_part_of_a_cell_holds_its_visible_texels_and_the_pad() {
        use super::AlphaMap;
        // 8 x 8, visible texels at x 4..=5, y 2..=3 only.
        let mut alpha = vec![0u8; 64];
        for y in 2..=3 {
            for x in 4..=5 {
                alpha[y * 8 + x] = 255;
            }
        }
        let map = AlphaMap { width: 8, height: 8, alpha };
        // The whole texture, s along +u, t along +v: x 4..=5 is 4/8..6/8, y 2..=3 is 2/8..4/8.
        let k = map.kept_part([0.0, 0.0], [1.0, 0.0], [0.0, 1.0], 8, 0.0).unwrap();
        assert_eq!(k, [0.5, 0.75, 0.25, 0.5]);
        // One texel of pad each way.
        let k = map.kept_part([0.0, 0.0], [1.0, 0.0], [0.0, 1.0], 8, 1.0).unwrap();
        assert_eq!(k, [0.375, 0.875, 0.125, 0.625]);
        // A cell stored the other way round (t from the bottom up): the same texels.
        let k = map.kept_part([0.0, 1.0], [1.0, 1.0], [0.0, 0.0], 8, 0.0).unwrap();
        assert_eq!(k, [0.5, 0.75, 0.5, 0.75]);
        // A cell with nothing visible.
        assert_eq!(map.kept_part([0.0, 0.5], [0.5, 0.5], [0.0, 1.0], 8, 0.0), None);
    }

    #[test]
    fn a_dxt5_alpha_map_reads_each_block_in_place() {
        use super::alpha_map;
        // 8 x 4: two DXT5 blocks, the first all alpha 0 (a0 = a1 = 0), the second 255.
        let mut payload = Vec::new();
        payload.extend([0u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        payload.extend([255u8, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let m = alpha_map(&dds(b"DXT5", 8, 4, &payload)).unwrap();
        assert_eq!((m.width, m.height), (8, 4));
        assert_eq!(m.alpha[0], 0);
        assert_eq!(m.alpha[4], 255);
        assert_eq!(m.alpha[3 * 8 + 7], 255);
    }

    /// #3486: the BC4 copy of a DXT5's alpha reads as the decoded alpha does, every level.
    #[test]
    fn an_alpha_height_is_the_dxt5s_alpha_blocks_as_bc4() {
        // Every block its own endpoints (both orders) and indices; two levels.
        let block = |k: u8| [k.wrapping_mul(37), k.wrapping_mul(91).wrapping_add(5), k, k.wrapping_mul(3), 0x92, 0x24, k ^ 0x5a, 0xc7, 0x1f, 0x2e, 0x10, 0x84, 1, 2, 3, 4];
        let payload: Vec<u8> = (0..5).flat_map(block).collect();
        let mut bytes = dds(b"DXT5", 8, 8, &payload);
        bytes[28..32].copy_from_slice(&2u32.to_le_bytes());
        bytes[8..12].copy_from_slice(&(0x1007u32 | 0x20000).to_le_bytes());
        let image = super::alpha_height_image(&bytes).unwrap().expect("a varying alpha");
        assert_eq!((image.texture_descriptor.format, image.texture_descriptor.mip_level_count), (TextureFormat::Bc4RUnorm, 2));
        let copied = image.data.as_deref().unwrap();
        assert_eq!(copied.len(), 5 * 8);
        for (level, (side, at)) in [(8u32, 0usize), (4, 4)].into_iter().enumerate() {
            let blocks = (side / 4 * side / 4) as usize;
            let alpha = fnv_dds::decode_blocks(fnv_dds::Format::Dxt5, side, side, &payload[at * 16..(at + blocks) * 16]).unwrap();
            let height = fnv_dds::decode_bc4(fnv_dds::Format::Bc4U, side, side, &copied[at * 8..(at + blocks) * 8]).unwrap();
            for (a, h) in alpha.as_chunks::<4>().0.iter().zip(&height) {
                assert!((f32::from(a[3]) / 255.0 - h).abs() <= 1.0 / 255.0, "level {level}: {} and {h}", a[3]);
            }
        }
        // Constant, opaque and near-constant alphas are no heights; nor is a DXT1.
        for a in [0u8, 128, 255] {
            assert!(super::alpha_height_image(&dds(b"DXT5", 4, 4, &dxt5_block(a))).unwrap().is_none(), "{a}");
        }
        let two = |a: u8, b: u8| dds(b"DXT5", 8, 4, &[dxt5_block(a), dxt5_block(b)].concat());
        assert!(super::alpha_height_image(&two(200, 215)).unwrap().is_none());
        assert!(super::alpha_height_image(&two(200, 216)).unwrap().is_some());
        assert!(super::alpha_height_image(&dds(b"DXT1", 4, 4, &[0; 8])).unwrap().is_none());
        // A layout whose alpha is not read says so; an unreadable file is an error too.
        assert!(super::alpha_height_image(&dds(b"DXT3", 4, 4, &[0; 16])).is_err());
        assert!(super::alpha_height_image(&rgba8_dds(4, 4, 1, false, &[7; 64])).is_err());
        assert!(super::alpha_height_image(&rgba8_dds(4, 4, 1, false, &[255; 64])).unwrap().is_none());
        assert!(super::alpha_height_image(b"not a dds").unwrap().is_none());
    }

    #[test]
    fn a_mask_is_worked_out_once_per_path() {
        let known = Mutex::new(HashMap::new());
        let mut asked = Vec::new();
        assert!(remembered(&known, "a.dds".into(), |p| { asked.push(p.to_string()); true }));
        assert!(remembered(&known, "a.dds".into(), |p| { asked.push(p.to_string()); false }), "the first answer is kept");
        assert!(!remembered(&known, "b.dds".into(), |p| { asked.push(p.to_string()); false }));
        assert_eq!(asked, ["a.dds", "b.dds"]);
    }

    /// A DDS file of one level: the header (fnv-dds layout) and `payload`.
    fn dds(fourcc: &[u8; 4], w: u32, h: u32, payload: &[u8]) -> Vec<u8> {
        let mut b = vec![0u8; 128];
        b[..4].copy_from_slice(b"DDS ");
        let put = |b: &mut Vec<u8>, at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
        put(&mut b, 4, 124);
        put(&mut b, 8, 0x1007);
        put(&mut b, 12, h);
        put(&mut b, 16, w);
        put(&mut b, 28, 1);
        put(&mut b, 76, 32);
        put(&mut b, 80, 4);
        b[84..88].copy_from_slice(fourcc);
        put(&mut b, 108, 0x1000);
        b.extend_from_slice(payload);
        b
    }

    /// A DXT5 block with every texel at alpha `a` (endpoints a and a, index 0).
    fn dxt5_block(a: u8) -> [u8; 16] {
        let mut blk = [0u8; 16];
        blk[0] = a;
        blk[1] = a;
        blk
    }

    /// A cube map (DDS caps2 0xFE00, six faces: `textures\effects\shinybright_e.dds`)
    /// loads only as a cube: six layers viewed as a cube, the faces' blocks as they are or
    /// decoded for a GPU without BC formats; a flat texture is no cube, and a cube map is
    /// still refused as a flat texture (nif.md §6 "Window glass and environment maps").
    #[test]
    fn cube_maps_load_as_cubes_only() {
        let mut cube = dds(b"DXT1", 4, 4, &[0x55; 6 * 8]);
        cube[112..116].copy_from_slice(&0xFE00u32.to_le_bytes());
        let img = super::dds_cube_image(&cube, true, true).unwrap();
        assert_eq!(img.texture_descriptor.format, TextureFormat::Bc1RgbaUnormSrgb);
        assert_eq!(img.texture_descriptor.size.depth_or_array_layers, 6);
        assert_eq!(img.texture_view_descriptor.as_ref().and_then(|v| v.dimension), Some(super::TextureViewDimension::Cube));
        assert_eq!(img.data.as_ref().unwrap().len(), 6 * 8);
        let rgba = super::dds_cube_image(&cube, false, false).unwrap();
        assert_eq!(rgba.texture_descriptor.format, TextureFormat::Rgba8Unorm);
        assert_eq!(rgba.data.as_ref().unwrap().len(), 6 * 4 * 4 * 4);
        assert!(dds_image(&cube, true, true).is_err());
        assert_eq!(super::dds_cube_image(&dds(b"DXT1", 4, 4, &[0; 8]), true, true).err().as_deref(), Some(super::NOT_A_CUBE), "a flat texture");
        // Five faces flagged (+X..-Z, no +Z... any five): a broken cube, not a flat texture,
        // so it is a warning, not the flat texture's info line (`texture_failed`).
        let mut five = dds(b"DXT1", 4, 4, &[0x55; 5 * 8]);
        five[112..116].copy_from_slice(&0x7E00u32.to_le_bytes());
        let e = super::dds_cube_image(&five, true, true).unwrap_err();
        assert_ne!(e, super::NOT_A_CUBE);
        let mut failures = Vec::new();
        super::texture_failed(&mut failures, &super::cube_path("textures\\a_e.dds"), &e);
        assert_eq!(failures.len(), 1, "a five-face cube is a failure: {e}");
        super::texture_failed(&mut failures, &super::cube_path("textures\\b_e.dds"), super::NOT_A_CUBE);
        assert_eq!(failures.len(), 1, "a flat slot-4 texture is not");
        assert_eq!(super::split_cube(&super::cube_path("textures\\a_e.dds")), ("textures\\a_e.dds", true));
        assert_eq!(super::split_cube("textures\\a.dds"), ("textures\\a.dds", false));
    }

    /// A block texture whose sides are not multiples of 4 becomes an RGBA image of its
    /// own size; one with bytes after its last level loads as if they were not there [#2164].
    #[test]
    fn odd_sized_and_overlong_block_textures_load() {
        // 6x5 DXT5 = 2x2 blocks, every texel opaque.
        let payload: Vec<u8> = (0..4).flat_map(|_| dxt5_block(255)).collect();
        let img = dds_image(&dds(b"DXT5", 6, 5, &payload), true, true).unwrap();
        assert_eq!(img.texture_descriptor.format, TextureFormat::Rgba8UnormSrgb);
        assert_eq!((img.width(), img.height()), (6, 5));
        let data = img.data.as_ref().unwrap();
        assert_eq!(data.len(), 6 * 5 * 4);
        assert!(data.chunks(4).all(|t| t[3] == 255));
        // Decoded on the CPU, so a GPU without BC formats shows it too.
        assert!(dds_image(&dds(b"DXT5", 6, 5, &payload), true, false).is_ok());
        // 8x8 with 8 bytes over: the blocks go to the GPU as they are.
        let mut long = payload.clone();
        long.extend_from_slice(&[0xAB; 8]);
        let img = dds_image(&dds(b"DXT5", 8, 8, &long), false, true).unwrap();
        assert_eq!(img.texture_descriptor.format, TextureFormat::Bc3RgbaUnorm);
        assert_eq!(img.data.as_ref().unwrap().len(), 64);
    }

    #[test]
    fn cut_out_masks_are_told_from_translucent_alpha() {
        // 8x8 = four blocks: two clear, one opaque, one in between.
        let mut payload = Vec::new();
        for a in [0u8, 0, 255, 128] {
            payload.extend_from_slice(&dxt5_block(a));
        }
        let s = alpha_shares(&dds(b"DXT5", 8, 8, &payload)).unwrap();
        assert_eq!(s, AlphaShares { clear: 0.5, opaque: 0.25, partial: 0.25 });
        assert!(!s.is_mask(), "a quarter in between is translucent");
        // Interpolated DXT5 indices: a0 = 255, a1 = 0, index 7 is (1·255 + 6·0)/7 ≈ 36.
        let mut blk = [0u8; 16];
        blk[0] = 255;
        blk[2..8].copy_from_slice(&[0xFF; 6]);
        let mut mask = Vec::new();
        for b in [dxt5_block(0), dxt5_block(255), dxt5_block(255), blk] {
            mask.extend_from_slice(&b);
        }
        let s = alpha_shares(&dds(b"DXT5", 8, 8, &mask)).unwrap();
        assert!((s.partial - 0.25).abs() < 1e-6, "{s:?}");
        let mostly = [dxt5_block(0), dxt5_block(255), dxt5_block(255), dxt5_block(255), dxt5_block(255), dxt5_block(255), dxt5_block(255), dxt5_block(0)];
        let s = alpha_shares(&dds(b"DXT5", 16, 8, &mostly.concat())).unwrap();
        assert!(s.is_mask(), "{s:?}");
        // All opaque: no hole, no cut-out.
        assert!(!alpha_shares(&dds(b"DXT5", 4, 4, &dxt5_block(255))).unwrap().is_mask());
        // DXT1 punch-through: colour 0 ≤ colour 1 and index 3 is clear.
        let mut dxt1 = [0u8; 8];
        dxt1[2] = 1;
        dxt1[4..8].copy_from_slice(&[0xFF, 0xFF, 0, 0]);
        let s = alpha_shares(&dds(b"DXT1", 4, 4, &dxt1)).unwrap();
        assert_eq!(s, AlphaShares { clear: 0.5, opaque: 0.5, partial: 0.0 });
        assert!(s.is_mask());
        // #3230: whether every texel is opaque, all of them looked at.
        assert_eq!(super::alpha_all_opaque(&dds(b"DXT1", 4, 4, &dxt1)), Some(false), "punch-through texels");
        // The same indices with colour 0 above colour 1: four colours, no clear one.
        let mut solid = dxt1;
        (solid[0], solid[2]) = (2, 1);
        assert_eq!(super::alpha_all_opaque(&dds(b"DXT1", 4, 4, &solid)), Some(true));
        assert_eq!(super::alpha_all_opaque(&dds(b"DXT5", 4, 4, &dxt5_block(255))), Some(true));
        assert_eq!(super::alpha_all_opaque(&dds(b"DXT5", 4, 4, &dxt5_block(254))), Some(false));
        // One clear block among many opaque ones is found (nothing is sampled).
        let mut many: Vec<u8> = std::iter::repeat_n(solid, 64 * 64).flatten().collect();
        assert_eq!(super::alpha_all_opaque(&dds(b"DXT1", 256, 256, &many)), Some(true));
        let last = many.len() - 8;
        many[last..].copy_from_slice(&dxt1);
        assert_eq!(super::alpha_all_opaque(&dds(b"DXT1", 256, 256, &many)), Some(false));
        assert_eq!(super::alpha_all_opaque(b"not a dds"), None);
        // A solid texel: alpha 250 or more, in any block; DXT1's clear texels are 0.
        use super::SolidTexels;
        let solidity = |blocks: &[u8], w: u32| super::alpha_solidity(&dds(b"DXT5", w, 4, blocks));
        assert_eq!(solidity(&dxt5_block(255), 4), Some(SolidTexels::Mostly));
        assert_eq!(solidity(&dxt5_block(250), 4), Some(SolidTexels::Mostly));
        assert_eq!(solidity(&dxt5_block(249), 4), Some(SolidTexels::None));
        // Clear texels do not count against it; a pane's do, though one texel is solid.
        assert_eq!(solidity(&[dxt5_block(0), dxt5_block(255)].concat(), 8), Some(SolidTexels::Mostly));
        assert_eq!(solidity(&[dxt5_block(230), dxt5_block(230), dxt5_block(255)].concat(), 12), Some(SolidTexels::Some));
        // A border of 230 on three blocks of 255 is the edge of what is solid.
        assert_eq!(solidity(&[dxt5_block(255), dxt5_block(255), dxt5_block(255), dxt5_block(230)].concat(), 16), Some(SolidTexels::Mostly));
        assert_eq!(super::alpha_solidity(&dds(b"DXT1", 4, 4, &dxt1)), Some(SolidTexels::Mostly));
        assert_eq!((SolidTexels::None.cut(), SolidTexels::Some.cut(), SolidTexels::Mostly.cut()), (None, Some(0.98), Some(0.85)));
        // A 1 texel wide pane of 230 in a block padded with 255: the first column of each row.
        let mut pane = dxt5_block(255);
        pane[..8].copy_from_slice(&{
            // Alpha endpoints 255 and 230, index 0 for 255 and 1 for 230, 3 bits a texel.
            let bits = (0..16u64).filter(|k| k.is_multiple_of(4)).fold(0u64, |bits, k| bits | 1 << (3 * k));
            let mut block = (bits << 16).to_le_bytes();
            (block[0], block[1]) = (255, 230);
            block
        });
        assert_eq!(super::alpha_solidity(&dds(b"DXT5", 1, 4, &pane)), Some(SolidTexels::None));
        assert_eq!(super::alpha_solidity(&dds(b"DXT5", 4, 4, &pane)), Some(SolidTexels::Mostly));
        assert_eq!(super::alpha_solidity(b"not a dds"), None);
        // DXT3: explicit 4-bit alphas, 0x0F → 255 then 0.
        let mut dxt3 = [0u8; 16];
        dxt3[..8].copy_from_slice(&[0x0F; 8]);
        let s = alpha_shares(&dds(b"DXT3", 4, 4, &dxt3)).unwrap();
        assert_eq!(s, AlphaShares { clear: 0.5, opaque: 0.5, partial: 0.0 });
    }

    #[test]
    fn the_first_person_twin_is_used_when_it_exists() {
        let twin = r"textures\weapons\2handrifle\1stpersonlaserrifle01.dds";
        let exists = |p: &str| p == twin;
        assert_eq!(first_person_texture(r"textures\weapons\2handrifle\LaserRifle01.dds", exists).as_deref(), Some(twin));
        // A path as models write it, without the `textures` folder.
        assert_eq!(first_person_texture(r"weapons\2handrifle\LaserRifle01.dds", exists).as_deref(), Some(twin));
        assert_eq!(first_person_texture(r"textures\weapons\2handrifle\LaserRifle02.dds", exists), None);
        // Already a first-person texture: kept.
        assert_eq!(first_person_texture(r"textures\weapons\2handrifle\1stPersonLaserRifle01.dds", |_| true), None);
    }
    // Authored BC blocks, not game data: colour index zero selects the first RGB565
    // endpoint. DXT3 uses explicit 8/15 alpha, DXT5 endpoint96, DXT1 opaque colour.
    fn colour_block(fourcc: &[u8; 4], rgb565: u16) -> Vec<u8> {
        let mut colour = vec![0u8; 8];
        colour[..2].copy_from_slice(&rgb565.to_le_bytes());
        match fourcc {
            b"DXT1" => colour,
            b"DXT3" => { let mut block = vec![0x88; 8]; block.extend(colour); block }
            b"DXT5" => { let mut block = vec![96, 96, 0, 0, 0, 0, 0, 0]; block.extend(colour); block }
            _ => unreachable!(),
        }
    }
    fn alpha_for(fourcc: &[u8; 4]) -> u8 {
        match fourcc { b"DXT1" => 255, b"DXT3" => 136, b"DXT5" => 96, _ => unreachable!() }
    }
    fn mip_fixture(fourcc: &[u8; 4]) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let mut payload = Vec::new();
        let mut expected = Vec::new();
        let mut top = Vec::new();
        for (size, colour, rgb) in [(8u32, 0xf800, [255, 0, 0]), (4, 0x07e0, [0, 255, 0]), (2, 0x001f, [0, 0, 255]), (1, 0xffff, [255, 255, 255])] {
            for _ in 0..size.div_ceil(4).pow(2) { payload.extend(colour_block(fourcc, colour)); }
            let pixel = [rgb[0], rgb[1], rgb[2], alpha_for(fourcc)];
            for _ in 0..size*size { expected.extend(pixel); if size == 8 { top.extend(pixel); } }
        }
        let mut bytes = dds(fourcc, 8, 8, &payload);
        bytes[8..12].copy_from_slice(&0x0002_1007u32.to_le_bytes());
        bytes[28..32].copy_from_slice(&4u32.to_le_bytes());
        bytes[108..112].copy_from_slice(&0x0040_1008u32.to_le_bytes());
        (bytes, expected, top)
    }

    #[test]
    fn aligned_bc_textures_without_bc_support_keep_colour_alpha_mips_and_colour_space() {
        for fourcc in [b"DXT1", b"DXT3", b"DXT5"] {
            let (bytes, expected, _) = mip_fixture(fourcc);
            for srgb in [false, true] {
                let image = dds_image(&bytes, srgb, false).unwrap();
                assert_eq!((image.width(), image.height(), image.texture_descriptor.mip_level_count), (8, 8, 4));
                assert_eq!(image.texture_descriptor.format, if srgb { TextureFormat::Rgba8UnormSrgb } else { TextureFormat::Rgba8Unorm });
                assert_eq!(image.data.as_ref().unwrap(), &expected);
                assert_eq!(expected.len(), (8*8 + 4*4 + 2*2 + 1)*4);
            }
        }
    }
    #[test]
    fn dxt1_cutout_transparency_survives_the_non_bc_fallback() {
        // Equal black endpoints plus colour index3: the authored BC1 transparent entry.
        let block = [0, 0, 0, 0, 255, 255, 255, 255];
        for srgb in [false, true] {
            let image = dds_image(&dds(b"DXT1", 4, 4, &block), srgb, false).unwrap();
            assert_eq!(image.data.unwrap(), vec![0; 4 * 4 * 4]);
        }
    }
    #[test]
    fn top_only_bc_fallback_decodes_only_the_requested_largest_level() {
        for fourcc in [b"DXT1", b"DXT3", b"DXT5"] {
            let (bytes, _, expected) = mip_fixture(fourcc);
            for srgb in [false, true] {
                let image = super::dds_image_top(&bytes, srgb, false).unwrap();
                assert_eq!(image.texture_descriptor.mip_level_count, 1);
                assert_eq!((image.width(), image.height()), (8, 8));
                assert_eq!(image.texture_descriptor.format, if srgb { TextureFormat::Rgba8UnormSrgb } else { TextureFormat::Rgba8Unorm });
                assert_eq!(image.data.as_ref().unwrap(), &expected);
            }
        }
    }
    #[test]
    fn bc_capable_aligned_textures_keep_compressed_format_bytes_and_mips() {
        for (fourcc, linear, srgb_format) in [(b"DXT1", TextureFormat::Bc1RgbaUnorm, TextureFormat::Bc1RgbaUnormSrgb), (b"DXT3", TextureFormat::Bc2RgbaUnorm, TextureFormat::Bc2RgbaUnormSrgb), (b"DXT5", TextureFormat::Bc3RgbaUnorm, TextureFormat::Bc3RgbaUnormSrgb)] {
            let (bytes, _, _) = mip_fixture(fourcc);
            for srgb in [false, true] {
                let image = dds_image(&bytes, srgb, true).unwrap();
                assert_eq!(image.texture_descriptor.format, if srgb { srgb_format } else { linear });
                assert_eq!((image.width(), image.height(), image.texture_descriptor.mip_level_count), (8, 8, 4));
                assert_eq!(image.data.as_ref().unwrap(), &bytes[128..]);
                let top = super::dds_image_top(&bytes, srgb, true).unwrap();
                assert_eq!(top.texture_descriptor.mip_level_count, 1);
                let length = if fourcc == b"DXT1" { 32 } else { 64 };
                assert_eq!(top.data.as_ref().unwrap(), &bytes[128..128+length]);
            }
        }
    }
    #[test]
    fn npot_bc_fallback_crops_edge_blocks_identically_with_and_without_bc_support() {
        for fourcc in [b"DXT1", b"DXT3", b"DXT5"] {
            let mut payload = Vec::new();
            for colour in [0xf800, 0x07e0, 0x001f, 0xffff] { payload.extend(colour_block(fourcc, colour)); }
            let bytes = dds(fourcc, 6, 5, &payload);
            let colours = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255]];
            for srgb in [false, true] {
                let cpu = dds_image(&bytes, srgb, false).unwrap();
                let supported = dds_image(&bytes, srgb, true).unwrap();
                assert_eq!(cpu.data, supported.data);
                assert_eq!(cpu.texture_descriptor.format, supported.texture_descriptor.format);
                assert_eq!((cpu.width(), cpu.height()), (6, 5));
                let pixels = cpu.data.as_ref().unwrap();
                for y in 0..5usize { for x in 0..6usize {
                    let rgb = colours[(y/4)*2+x/4];
                    assert_eq!(&pixels[(y*6+x)*4..(y*6+x)*4+4], &[rgb[0], rgb[1], rgb[2], alpha_for(fourcc)]);
                }}
            }
        }
    }
    #[test]
    fn unsupported_or_malformed_dds_remains_an_error_in_both_capability_paths() {
        for bc in [false, true] {
            assert!(dds_image(&dds(b"ATI2", 4, 4, &[0; 16]), true, bc).is_err());
            assert!(dds_image(&dds(b"DX10", 4, 4, &[0; 16]), true, bc).is_err());
            assert!(dds_image(&dds(b"DXT1", 4, 4, &[0; 7]), true, bc).is_err());
            assert!(dds_image(&dds(b"DXT5", 0, 4, &[0; 16]), false, bc).is_err());
            assert!(dds_image(&dds(b"DXT5", super::MAX_TEXTURE_SIDE + 4, 4, &[0; 16]), false, bc).is_err());
            let mut cube = dds(b"DXT1", 4, 4, &[0; 48]);
            cube[112..116].copy_from_slice(&0x0000_fe00u32.to_le_bytes());
            assert!(dds_image(&cube, true, bc).unwrap_err().contains("cube map"));
        }
    }

    #[test]
    fn checked_image_keeps_validated_multi_level_rgba_data() {
        // Authored bytes for an 8x8, 4x4, 2x2, 1x1 chain. Image::new's debug-only
        // base-level assertion must not reject this valid 340-byte payload.
        let data: Vec<u8> = (0..340).map(|i| (i % 251) as u8).collect();
        for format in [TextureFormat::Rgba8Unorm, TextureFormat::Rgba8UnormSrgb] {
            let image = super::checked_image(8, 8, 4, format, data.clone()).unwrap();
            assert_eq!((image.width(), image.height(), image.texture_descriptor.mip_level_count), (8, 8, 4));
            assert_eq!(image.texture_descriptor.format, format);
            assert_eq!(image.data.as_ref().unwrap(), &data);
        }
    }

    /// A legacy 32-bit `A8R8G8B8` DDS file with `mips` levels, optionally flagged as a
    /// six-face cube map (`caps2 = 0xFE00`).
    fn rgba8_dds(w: u32, h: u32, mips: u32, cube: bool, payload: &[u8]) -> Vec<u8> {
        let mut b = vec![0u8; 128];
        b[..4].copy_from_slice(b"DDS ");
        let put = |b: &mut Vec<u8>, at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
        put(&mut b, 4, 124);
        put(&mut b, 8, 0x21007); // CAPS | HEIGHT | WIDTH | PIXELFORMAT | MIPMAPCOUNT
        put(&mut b, 12, h);
        put(&mut b, 16, w);
        put(&mut b, 28, mips);
        put(&mut b, 76, 32); // pixel format size
        put(&mut b, 80, 0x41); // DDPF_RGB | DDPF_ALPHAPIXELS
        put(&mut b, 88, 32); // RGBBitCount
        put(&mut b, 92, 0x00FF_0000); // R mask
        put(&mut b, 96, 0x0000_FF00); // G mask
        put(&mut b, 100, 0x0000_00FF); // B mask
        put(&mut b, 104, 0xFF00_0000); // A mask
        put(&mut b, 108, 0x0040_1008); // DDSCAPS_COMPLEX | DDSCAPS_TEXTURE | DDSCAPS_MIPMAP
        if cube {
            put(&mut b, 112, 0xFE00);
        }
        b.extend_from_slice(payload);
        b
    }

    #[test]
    fn multimip_uncompressed_dds_and_cubemap_build_without_panic() {
        // 16x16 with 5 mips: 16^2 + 8^2 + 4^2 + 2^2 + 1^2 = 341 texels = 1364 RGBA8 bytes.
        let expected_2d = (16 * 16 + 8 * 8 + 4 * 4 + 2 * 2 + 1) * 4;
        let dds_2d = rgba8_dds(16, 16, 5, false, &vec![0xAB; expected_2d]);
        for (srgb, want_fmt) in [(true, TextureFormat::Rgba8UnormSrgb), (false, TextureFormat::Rgba8Unorm)] {
            let img = dds_image(&dds_2d, srgb, true).unwrap();
            assert_eq!(img.texture_descriptor.format, want_fmt);
            assert_eq!(img.texture_descriptor.size, super::Extent3d { width: 16, height: 16, depth_or_array_layers: 1 });
            assert_eq!(img.texture_descriptor.mip_level_count, 5);
            assert_eq!(img.data.as_ref().unwrap().len(), expected_2d);
        }

        // 8x8 with 4 mips across 6 cube faces: (64 + 16 + 4 + 1) * 4 * 6 = 2040 bytes.
        let expected_cube = (8 * 8 + 4 * 4 + 2 * 2 + 1) * 4 * 6;
        let dds_cube = rgba8_dds(8, 8, 4, true, &vec![0xCD; expected_cube]);
        let cube_img = super::dds_cube_image(&dds_cube, true, true).unwrap();
        assert_eq!(cube_img.texture_descriptor.format, TextureFormat::Rgba8UnormSrgb);
        assert_eq!(cube_img.texture_descriptor.size, super::Extent3d { width: 8, height: 8, depth_or_array_layers: 6 });
        assert_eq!(cube_img.texture_descriptor.mip_level_count, 4);
        assert_eq!(cube_img.data.as_ref().unwrap().len(), expected_cube);

        // Multi-mip odd-sized DXT1 decoded to RGBA8 on CPU: 6x6 (4 blocks) + 3x3 (1) + 1x1 (1).
        let mut odd_dxt1 = dds(b"DXT1", 6, 6, &[0x55; (4 + 1 + 1) * 8]);
        odd_dxt1[8..12].copy_from_slice(&0x21007u32.to_le_bytes());
        odd_dxt1[28..32].copy_from_slice(&3u32.to_le_bytes());
        let decoded = dds_image(&odd_dxt1, false, true).unwrap();
        assert_eq!(decoded.texture_descriptor.format, TextureFormat::Rgba8Unorm);
        assert_eq!(decoded.texture_descriptor.mip_level_count, 3);
        assert_eq!(decoded.data.as_ref().unwrap().len(), (6 * 6 + 3 * 3 + 1) * 4);

        // Multi-mip DXT1 cube decoded to RGBA8 when BC compression is unavailable.
        let mut dxt1_cube = dds(b"DXT1", 8, 8, &[0x55; (4 + 1 + 1 + 1) * 8 * 6]);
        dxt1_cube[8..12].copy_from_slice(&0x21007u32.to_le_bytes());
        dxt1_cube[28..32].copy_from_slice(&4u32.to_le_bytes());
        dxt1_cube[108..112].copy_from_slice(&0x0040_1008u32.to_le_bytes());
        dxt1_cube[112..116].copy_from_slice(&0xFE00u32.to_le_bytes());
        let decoded_cube = super::dds_cube_image(&dxt1_cube, true, false).unwrap();
        assert_eq!(decoded_cube.texture_descriptor.format, TextureFormat::Rgba8UnormSrgb);
        assert_eq!(decoded_cube.texture_descriptor.mip_level_count, 4);
        assert_eq!(decoded_cube.data.as_ref().unwrap().len(), expected_cube);

        // Multi-layer array image via checked_image_layers.
        let expected_layers = (8 * 8 + 4 * 4 + 2 * 2 + 1) * 4 * 3;
        let arr = super::checked_image_layers(8, 8, 4, 3, TextureFormat::Rgba8Unorm, vec![0; expected_layers]).unwrap();
        assert_eq!(arr.texture_descriptor.mip_level_count, 4);
        assert_eq!(arr.texture_descriptor.size.depth_or_array_layers, 3);
        assert_eq!(arr.data.as_ref().unwrap().len(), expected_layers);

        // Truncated (mip-0 only) and overlong buffers are rejected cleanly with Err.
        assert!(super::checked_image(16, 16, 5, TextureFormat::Rgba8Unorm, vec![0; 16 * 16 * 4]).is_err());
        assert!(super::checked_image(16, 16, 5, TextureFormat::Rgba8Unorm, vec![0; expected_2d + 1]).is_err());
    }
}


#[cfg(test)]
pub(crate) mod bc5_tests {
    use super::*;

    pub(crate) fn dds(tag: &[u8; 4], w: u32, h: u32, mips: u32, payload: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0; 128];
        bytes[..4].copy_from_slice(b"DDS ");
        for (offset, value) in [(4, 124), (8, 0x21007), (12, h), (16, w), (28, mips), (76, 32), (80, 4)] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes[84..88].copy_from_slice(tag);
        bytes.extend_from_slice(payload);
        bytes
    }

    pub(crate) struct OwnedData {
        root: std::path::PathBuf,
        pub(crate) data: GameData,
    }

    impl OwnedData {
        pub(crate) fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let serial = NEXT.fetch_add(1, Ordering::Relaxed);
            let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            let root = std::env::temp_dir().join(format!("nvf-bc5-owned-{}-{stamp}-{serial}", std::process::id()));
            std::fs::create_dir(&root).unwrap();
            std::fs::create_dir(root.join("textures")).unwrap();
            let mut signed = [0; 16]; signed[0] = 129; signed[8] = 127;
            std::fs::write(root.join("textures/signed.dds"), dds(b"BC5S", 4, 4, 1, &signed)).unwrap();
            std::fs::write(root.join("textures/unsigned.dds"), dds(b"ATI2", 4, 4, 1, &signed)).unwrap();
            std::fs::write(root.join("textures/old.dds"), dds(b"DXT5", 4, 4, 1, &[255; 16])).unwrap();
            let data = GameData::new(Arc::new(fnv_vfs::Vfs::open(&root, &[]).unwrap()), Arc::new(fnv_nif::Schema::default()));
            Self { root, data }
        }
    }

    impl Drop for OwnedData {
        fn drop(&mut self) {
            // Only these authored files and the two directories we created, no recursion.
            for name in ["signed.dds", "unsigned.dds", "old.dds"] {
                let _ = std::fs::remove_file(self.root.join("textures").join(name));
            }
            let _ = std::fs::remove_dir(self.root.join("textures"));
            let _ = std::fs::remove_dir(&self.root);
        }
    }

    pub(crate) fn triangle() -> Mesh {
        use bevy::mesh::{Indices, PrimitiveTopology};
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::MAIN_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]])
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 0.0, 1.0]; 3])
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 0.0], [1.0, 0.0], [0.0, 1.0]])
            .with_inserted_indices(Indices::U32(vec![0, 1, 2]))
    }

    #[test]
    fn signed_and_unsigned_linear_images_keep_format_and_endpoint_meaning() {
        let mut block = [0; 16]; block[0] = 129; block[8] = 127;
        let signed = dds(b"BC5S", 4, 4, 1, &block);
        let native = dds_image(&signed, false, true).unwrap();
        assert_eq!(native.texture_descriptor.format, TextureFormat::Bc5RgSnorm);
        assert_eq!(native.data.as_deref(), Some(block.as_slice()));
        let fallback = dds_image(&signed, false, false).unwrap();
        assert_eq!(fallback.texture_descriptor.format, TextureFormat::Rg8Snorm);
        assert_eq!(fallback.data.as_deref(), Some([129, 127].repeat(16).as_slice()));
        let unsigned = dds(b"ATI2", 4, 4, 1, &block);
        let native_u = dds_image(&unsigned, false, true).unwrap();
        assert_eq!(native_u.texture_descriptor.format, TextureFormat::Bc5RgUnorm);
        assert_eq!(native_u.data.as_deref(), Some(block.as_slice()));
        let fallback_u = dds_image(&unsigned, false, false).unwrap();
        assert_eq!(fallback_u.texture_descriptor.format, TextureFormat::Rg8Unorm);
        assert_eq!(fallback_u.data, fallback.data);
        for bytes in [&signed, &unsigned] {
            assert!(dds_image(bytes, true, true).unwrap_err().contains("no sRGB"));
            assert!(alpha_shares(bytes).is_none());
            assert!(alpha_map(bytes).is_none());
        }
        for format in [TextureFormat::Bc5RgSnorm, TextureFormat::Bc5RgUnorm, TextureFormat::Rg8Snorm, TextureFormat::Rg8Unorm] {
            assert!(crate::nif_spawn::format_lacks_alpha(format));
        }
        assert!(!crate::nif_spawn::format_lacks_alpha(TextureFormat::Rgba8Unorm));
    }

    #[test]
    fn normal_expansion_is_finite_without_shader_bias_or_alpha_confusion() {
        // Outside the unit disk, on its edge, and at zero: independent literal bytes.
        for (x, y, rgba) in [(129, 127, [0, 255, 128, 255]), (127, 0, [255, 128, 128, 255]), (0, 0, [128, 128, 255, 255])] {
            let mut block = [0; 16]; block[0] = x; block[8] = y;
            let bytes = dds(b"BC5S", 4, 4, 1, &block);
            for bc in [false, true] {
                let normal = dds_texture(&bytes, TextureUsage::Normal, bc, false).unwrap();
                assert_eq!(normal.image.texture_descriptor.format, TextureFormat::Rgba8Unorm);
                assert_eq!(normal.image.data.as_deref(), Some(rgba.repeat(16).as_slice()));
                assert!(normal.lacks_alpha, "synthesized alpha is not a specular mask");
                for pixel in normal.image.data.as_ref().unwrap().as_chunks::<4>().0 {
                    let n = Vec3::new(pixel[0] as f32, pixel[1] as f32, pixel[2] as f32) / 127.5 - Vec3::ONE;
                    assert!(n.is_finite() && n.length_squared() > 0.0);
                }
            }
        }
        // Unsigned XY uses its own unbias, not signed byte reinterpretation.
        let mut block = [0; 16]; block[0] = 255;
        let normal = dds_texture(&dds(b"ATI2", 4, 4, 1, &block), TextureUsage::Normal, true, false).unwrap();
        assert_eq!(normal.image.data.as_deref(), Some([255, 0, 128, 255].repeat(16).as_slice()));
    }

    #[test]
    fn bc5_partial_dimensions_and_mips_keep_exact_upload_lengths() {
        let bytes = dds(b"BC5S", 5, 3, 2, &[0; 48]);
        let linear = dds_image(&bytes, false, true).unwrap();
        assert_eq!((linear.width(), linear.height(), linear.texture_descriptor.mip_level_count), (5, 3, 2));
        assert_eq!(linear.texture_descriptor.format, TextureFormat::Rg8Snorm);
        assert_eq!(linear.data.as_ref().unwrap().len(), (15 + 2) * 2);
        let normal = dds_texture(&bytes, TextureUsage::Normal, true, false).unwrap();
        assert_eq!(normal.image.data.as_deref(), Some([128, 128, 255, 255].repeat(17).as_slice()));
        let top = dds_image_top(&bytes, false, true).unwrap();
        assert_eq!(top.texture_descriptor.mip_level_count, 1);
        assert_eq!(top.data.as_ref().unwrap().len(), 30);
        assert!(dds_image(&bytes[..bytes.len() - 1], false, true).is_err());
    }

    #[test]
    fn cache_roles_isolate_normal_linear_color_and_failure() {
        let owned = OwnedData::new();
        let mut cache = TextureCache::default();
        let mut images = Assets::default();
        // A refused color request must not poison either valid interpretation.
        assert!(cache.get("signed.dds", true, &owned.data, &mut images, None).is_none());
        let linear = cache.get("signed.dds", false, &owned.data, &mut images, None).unwrap();
        let normal = cache.normal("signed.dds", &owned.data, &mut images, None).unwrap();
        assert_ne!(linear, normal.0);
        assert!(normal.1);
        assert_eq!(images.get(&linear).unwrap().texture_descriptor.format, TextureFormat::Bc5RgSnorm);
        assert_eq!(images.get(&normal.0).unwrap().texture_descriptor.format, TextureFormat::Rgba8Unorm);
        assert_eq!(cache.normal("SIGNED.dds", &owned.data, &mut images, None).unwrap(), normal);
        assert_eq!(cache.get("signed.dds", false, &owned.data, &mut images, None), Some(linear));
        let old_color = cache.get("old.dds", true, &owned.data, &mut images, None).unwrap();
        let old_linear = cache.get("old.dds", false, &owned.data, &mut images, None).unwrap();
        let old_normal = cache.normal("old.dds", &owned.data, &mut images, None).unwrap();
        assert_ne!(old_color, old_linear);
        assert_ne!(old_linear, old_normal.0);
        assert_ne!(old_color, old_normal.0);
        assert!(!old_normal.1);
        assert_eq!(images.get(&old_color).unwrap().texture_descriptor.format, TextureFormat::Bc3RgbaUnormSrgb);
        assert_eq!(images.get(&old_normal.0).unwrap().texture_descriptor.format, TextureFormat::Bc3RgbaUnorm);
        assert_eq!(images.get(&old_linear).unwrap().data, images.get(&old_normal.0).unwrap().data);
        assert_eq!(cache.failures.len(), 1);
        assert!(cache.has_or_ahead("textures\\signed.dds"));
        cache.offer("textures\\signed.dds".into(), vec![1, 2, 3], 0.0);
        assert!(cache.ahead.is_empty());
    }

    #[test]
    fn normal_workers_share_one_handle_and_preserve_metadata_through_adoption() {
        let owned = OwnedData::new();
        let mut cache = TextureCache::default();
        let mut images = Assets::default();
        let provider = images.get_handle_provider();
        let empty = TextureSnapshot::new();
        let barrier = std::sync::Barrier::new(6);
        let handles = std::thread::scope(|scope| {
            let tasks: Vec<_> = (0..6).map(|_| scope.spawn(|| {
                barrier.wait();
                worker_texture("signed.dds", TextureUsage::Normal, &owned.data, &empty, &cache.workers, &provider, true).unwrap()
            })).collect();
            tasks.into_iter().map(|task| task.join().unwrap()).collect::<Vec<_>>()
        });
        assert!(handles.iter().all(|h| *h == handles[0]));
        assert!(handles[0].2);
        assert_eq!(cache.workers.entries.lock().unwrap().len(), 1);
        // Main adoption runs before take_workers, using the worker's exact identity.
        let normal = cache.normal("signed.dds", &owned.data, &mut images, None).unwrap();
        assert_eq!(normal, (handles[0].0.clone(), true));
        assert_eq!(cache.take_workers(&mut images, 1.0), 0);
        let snapshot = cache.snapshot(&images);
        assert_eq!(snapshot[&("textures\\signed.dds".into(), TextureUsage::Normal)], handles[0]);
        let isolated = WorkerTextures::default();
        assert_eq!(worker_texture("signed.dds", TextureUsage::Normal, &owned.data, &snapshot, &isolated, &provider, true), Some(handles[0].clone()));
        assert!(isolated.entries.lock().unwrap().is_empty(), "snapshot hit must not decode again");
        let linear = worker_texture("signed.dds", TextureUsage::Linear, &owned.data, &snapshot, &cache.workers, &provider, true).unwrap();
        assert_ne!(linear.0, normal.0);
        assert_eq!(linear.1, TextureFormat::Bc5RgSnorm);
        assert_eq!(cache.take_workers(&mut images, 2.0), 1);
        assert_eq!(cache.get("signed.dds", false, &owned.data, &mut images, None), Some(linear.0));
        assert!(worker_texture("signed.dds", TextureUsage::Color, &owned.data, &snapshot, &cache.workers, &provider, true).is_none());
        cache.take_workers(&mut images, 3.0);
        assert_eq!(cache.failures.len(), 1);
        assert!(worker_texture("signed.dds", TextureUsage::Color, &owned.data, &snapshot, &cache.workers, &provider, true).is_none());
        cache.take_workers(&mut images, 4.0);
        assert_eq!(cache.failures.len(), 1);
        assert_eq!(cache.normal("signed.dds", &owned.data, &mut images, None).unwrap(), normal);
    }

    /// The authored `_n` role (#2981): unsigned BC5 on the block grid goes up natively,
    /// otherwise its channels are stored as they are; signed BC5 is moved to 0..1. Both
    /// fall back to `Rg8Unorm`, which Bevy reads as a two-channel normal map too.
    #[test]
    fn two_channel_normals_keep_unsigned_bytes_and_bias_signed_ones() {
        let usage = TextureUsage::NormalTwoChannel;
        // Red endpoint 129, green 127, every index 0: each texel is the first endpoint.
        let mut block = [0; 16]; block[0] = 129; block[8] = 127;
        let unsigned = dds(b"ATI2", 4, 4, 1, &block);
        let native = dds_texture(&unsigned, usage, true, false).unwrap();
        assert_eq!(native.image.texture_descriptor.format, TextureFormat::Bc5RgUnorm);
        assert_eq!(native.image.data.as_deref(), Some(block.as_slice()));
        assert!(native.lacks_alpha, "two channels hold no specular mask");
        let fallback = dds_texture(&unsigned, usage, false, false).unwrap();
        assert_eq!(fallback.image.texture_descriptor.format, TextureFormat::Rg8Unorm);
        assert_eq!(fallback.image.data.as_deref(), Some([129, 127].repeat(16).as_slice()), "unsigned bytes unchanged");
        assert!(fallback.lacks_alpha);
        // Signed: -127 reads as -1 (byte 0), 127 as 1 (byte 255), 0 as 0 (byte 128); never native.
        for (x, y, want) in [(129u8, 127u8, [0u8, 255]), (0, 0, [128, 128]), (64, 192, [192, 63])] {
            let mut block = [0; 16]; block[0] = x; block[8] = y;
            let signed = dds(b"BC5S", 4, 4, 1, &block);
            for bc in [true, false] {
                let t = dds_texture(&signed, usage, bc, false).unwrap();
                assert_eq!(t.image.texture_descriptor.format, TextureFormat::Rg8Unorm, "signed BC5, bc {bc}");
                assert_eq!(t.image.data.as_deref(), Some(want.repeat(16).as_slice()), "signed ({x}, {y}), bc {bc}");
                assert!(t.lacks_alpha);
            }
        }
        // Off the block grid with BC support: decoded, exact length for two levels.
        let odd = dds(b"ATI2", 5, 3, 2, &[0; 48]);
        let t = dds_texture(&odd, usage, true, false).unwrap();
        assert_eq!(t.image.texture_descriptor.format, TextureFormat::Rg8Unorm);
        assert_eq!(t.image.data.as_ref().unwrap().len(), (15 + 2) * 2);
        // Three-channel files load as the game's normal maps do.
        assert_eq!(dds_texture(&dds(b"DXT5", 4, 4, 1, &[0; 16]), usage, true, false).unwrap().image.texture_descriptor.format, TextureFormat::Bc3RgbaUnorm);
        assert!(dds_texture(&dds(b"BC4U", 4, 4, 1, &[0; 8]), usage, true, false).err().unwrap().contains("BC4"));
    }

    /// The two-channel role has handles of its own beside the Normal and Linear roles of the
    /// same file, and is among the roles a read-ahead offer and `has_or_ahead` look at.
    #[test]
    fn two_channel_role_is_cached_apart_from_normal_and_linear() {
        let owned = OwnedData::new();
        let cache = TextureCache::default();
        let images = Assets::<Image>::default();
        let provider = images.get_handle_provider();
        let empty = TextureSnapshot::new();
        let two = worker_texture("unsigned.dds", TextureUsage::NormalTwoChannel, &owned.data, &empty, &cache.workers, &provider, true).unwrap();
        let normal = worker_texture("unsigned.dds", TextureUsage::Normal, &owned.data, &empty, &cache.workers, &provider, true).unwrap();
        let linear = worker_texture("unsigned.dds", TextureUsage::Linear, &owned.data, &empty, &cache.workers, &provider, true).unwrap();
        assert!(two.0 != normal.0 && two.0 != linear.0);
        assert_eq!((two.1, normal.1, linear.1), (TextureFormat::Bc5RgUnorm, TextureFormat::Rgba8Unorm, TextureFormat::Bc5RgUnorm));
        assert!(TEXTURE_USAGES.contains(&TextureUsage::NormalTwoChannel));
        let only_two = TextureCache::default();
        let _ = worker_texture("unsigned.dds", TextureUsage::NormalTwoChannel, &owned.data, &empty, &only_two.workers, &provider, true).unwrap();
        assert!(only_two.has_or_ahead("textures\\unsigned.dds"), "a two-channel load counts as loaded");
    }

    #[test]
    fn release_tracks_each_usage_and_clamped_copy_independently() {
        let owned = OwnedData::new();
        let mut cache = TextureCache::default();
        let mut images = Assets::default();
        let color = cache.get("old.dds", true, &owned.data, &mut images, None).unwrap();
        let linear = cache.get("old.dds", false, &owned.data, &mut images, None).unwrap();
        let clamped = cache.get_clamped("old.dds", false, &owned.data, &mut images, None).unwrap();
        let normal = cache.normal("old.dds", &owned.data, &mut images, None).unwrap();
        assert_ne!(linear, clamped);
        drop((color, linear, clamped));
        let mut idle = crate::release::Unused::default();
        let hints = crate::release::ImageHints::default();
        let keep = crate::release::KeepLimits { secs: 0.0, max: 0 };
        assert_eq!(cache.release(&mut idle, &hints, 1.0, keep), 3);
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.normal("old.dds", &owned.data, &mut images, None).unwrap(), normal);
        drop(normal);
        assert_eq!(cache.release(&mut idle, &hints, 2.0, keep), 1);
        assert!(cache.is_empty());
        assert!(cache.snapshot(&images).is_empty());
    }
}

/// DX10 files, BC7 and BC4 per role (#2981 stage 1). Every file is generated here.
#[cfg(test)]
mod dx10_tests {
    use super::*;

    /// A DX10 DDS file: the header with FourCC `DX10`, the extension (DXGI code, 2D, an
    /// array of one, `misc` and `flags2` as given), then `payload`.
    fn dx10(dxgi: u32, misc: u32, flags2: u32, w: u32, h: u32, mips: u32, payload: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0; 128];
        bytes[..4].copy_from_slice(b"DDS ");
        for (offset, value) in [(4, 124), (8, 0x21007), (12, h), (16, w), (28, mips), (76, 32), (80, 4), (108, 0x1000)] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes[84..88].copy_from_slice(b"DX10");
        for value in [dxgi, 3, misc, 1, flags2] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(payload);
        bytes
    }

    const OPAQUE: u32 = 3;
    const ROLES: [TextureUsage; 3] = [TextureUsage::Color, TextureUsage::Linear, TextureUsage::Normal];

    #[test]
    fn bc7_takes_its_colour_space_from_the_role_not_the_code() {
        let block: Vec<u8> = (0..16).collect();
        for code in [98, 99] {
            let bytes = dx10(code, 0, 0, 4, 4, 1, &block);
            for (usage, format) in ROLES.into_iter().zip([TextureFormat::Bc7RgbaUnormSrgb, TextureFormat::Bc7RgbaUnorm, TextureFormat::Bc7RgbaUnorm]) {
                let t = dds_texture(&bytes, usage, true, false).unwrap();
                assert_eq!(t.image.texture_descriptor.format, format, "dxgi {code} {usage:?}");
                assert_eq!(t.image.data.as_deref(), Some(block.as_slice()));
                assert!(!t.lacks_alpha, "no alpha mode stated: the alpha may be a mask");
            }
            // An OPAQUE alpha mode says the alpha holds no mask, whatever the role.
            let opaque = dx10(code, 0, OPAQUE, 4, 4, 1, &block);
            for usage in ROLES {
                assert!(dds_texture(&opaque, usage, true, false).unwrap().lacks_alpha);
            }
        }
        assert_eq!(dds_image(&dx10(99, 0, 0, 4, 4, 1, &block), false, true).unwrap().texture_descriptor.format, TextureFormat::Bc7RgbaUnorm);
    }

    #[test]
    fn bc7_off_the_block_grid_or_without_bc_support_is_refused() {
        // 6×5: 2×2 blocks.
        let odd = dx10(98, 0, 0, 6, 5, 1, &[0; 64]);
        for usage in ROLES {
            let e = dds_texture(&odd, usage, true, false).err().unwrap();
            assert!(e.contains("multiples of 4") && e.contains("6x5"), "{e}");
        }
        assert!(dds_image_top(&odd, true, true).unwrap_err().contains("multiples of 4"));
        let even = dx10(99, 0, 0, 8, 8, 1, &[0; 64]);
        for usage in ROLES {
            assert!(dds_texture(&even, usage, false, false).err().unwrap().contains("BC texture support"));
        }
    }

    /// Full chains down to 1×1 upload with whole blocks per level; a chain longer than the
    /// size allows parses (fnv-dds takes up to 32 levels) and is refused at upload.
    #[test]
    fn bc7_and_bc4_full_chains_upload_with_exact_lengths() {
        let total: usize = (0..13).map(|i| (4096usize >> i).max(1).div_ceil(4).pow(2) * 16).sum();
        let big = dx10(99, 0, 0, 4096, 4096, 13, &vec![0; total]);
        let img = dds_image(&big, true, true).unwrap();
        assert_eq!((img.width(), img.texture_descriptor.mip_level_count, img.data.as_ref().unwrap().len()), (4096, 13, total));
        let too_long = dx10(99, 0, 0, 4096, 4096, 14, &vec![0; total + 16]);
        assert!(Dds::parse(&too_long).is_ok());
        assert!(dds_image(&too_long, true, true).unwrap_err().contains("14 mip levels"));
        let small = dx10(98, 0, 0, 8, 4, 4, &[0; 80]);
        let img = dds_image(&small, false, true).unwrap();
        assert_eq!((img.texture_descriptor.mip_level_count, img.data.as_ref().unwrap().len()), (4, 80));
        assert_eq!(dds_image_top(&small, false, true).unwrap().data.as_ref().unwrap().len(), 32);
        // 5×3 BC4, 3 levels (5×3, 2×1, 1×1), off the grid: decoded to R8, one byte a texel.
        let bc4 = dx10(80, 0, 0, 5, 3, 3, &[0; 32]);
        let img = dds_image(&bc4, false, true).unwrap();
        assert_eq!(img.texture_descriptor.format, TextureFormat::R8Unorm);
        assert_eq!((img.texture_descriptor.mip_level_count, img.data.as_ref().unwrap().len()), (3, 15 + 2 + 1));
    }

    #[test]
    fn bc4_is_linear_data_native_or_decoded_to_r8() {
        // Unsigned: endpoints 255/0, indices 0..7 twice; signed: 127/-127.
        let unsigned = [255u8, 0, 0x88, 0xc6, 0xfa, 0x88, 0xc6, 0xfa];
        let signed = [127u8, 129, 0x88, 0xc6, 0xfa, 0x88, 0xc6, 0xfa];
        for (code, block, native, fallback) in [
            (80, unsigned, TextureFormat::Bc4RUnorm, TextureFormat::R8Unorm),
            (81, signed, TextureFormat::Bc4RSnorm, TextureFormat::R8Snorm),
        ] {
            let bytes = dx10(code, 0, 0, 4, 4, 1, &block);
            let t = dds_texture(&bytes, TextureUsage::Linear, true, false).unwrap();
            assert_eq!(t.image.texture_descriptor.format, native);
            assert_eq!(t.image.data.as_deref(), Some(block.as_slice()));
            assert!(t.lacks_alpha);
            let t = dds_texture(&bytes, TextureUsage::Linear, false, false).unwrap();
            assert_eq!(t.image.texture_descriptor.format, fallback);
            assert_eq!(t.image.data.as_ref().unwrap().len(), 16);
            assert!(t.lacks_alpha);
            for usage in [TextureUsage::Color, TextureUsage::Normal] {
                for bc in [true, false] {
                    assert!(dds_texture(&bytes, usage, bc, false).err().unwrap().contains("BC4"), "{usage:?}");
                }
            }
            assert!(alpha_shares(&bytes).is_none());
            assert!(alpha_map(&bytes).is_none());
        }
        // The R8 bytes: 255, 0, then 6/7 … 1/7 of 255, rounded.
        let decoded = dds_image(&dx10(80, 0, 0, 4, 4, 1, &unsigned), false, false).unwrap();
        assert_eq!(&decoded.data.as_ref().unwrap()[..8], &[255, 0, 219, 182, 146, 109, 73, 36]);
        let decoded = dds_image(&dx10(81, 0, 0, 4, 4, 1, &signed), false, false).unwrap();
        assert_eq!(&decoded.data.as_ref().unwrap()[..8], &[127, 129, 91, 54, 18, 238, 202, 165]);
        // The legacy BC4U FourCC reads the same.
        let legacy = bc5_tests::dds(b"BC4U", 4, 4, 1, &unsigned);
        assert_eq!(dds_image(&legacy, false, true).unwrap().texture_descriptor.format, TextureFormat::Bc4RUnorm);
        for format in [TextureFormat::Bc4RUnorm, TextureFormat::Bc4RSnorm, TextureFormat::R8Unorm, TextureFormat::R8Snorm] {
            assert!(crate::nif_spawn::format_lacks_alpha(format));
        }
        for format in [TextureFormat::Bc7RgbaUnorm, TextureFormat::Bc7RgbaUnormSrgb] {
            assert!(!crate::nif_spawn::format_lacks_alpha(format));
        }
    }

    /// The OPAQUE alpha mode survives every path, the early CPU decode of a DXT texture
    /// off the block grid included.
    #[test]
    fn opaque_alpha_mode_holds_on_every_path() {
        let odd_payload: Vec<u8> = (0..4).flat_map(|_| { let mut b = [0u8; 16]; b[0] = 255; b[1] = 255; b }).collect();
        for (flags2, opaque) in [(0, false), (1, false), (OPAQUE, true)] {
            // 6×5 BC3 through DX10: decoded to RGBA8 here.
            let odd = dx10(77, 0, flags2, 6, 5, 1, &odd_payload);
            for usage in ROLES {
                let t = dds_texture(&odd, usage, true, false).unwrap();
                assert!(matches!(t.image.texture_descriptor.format, TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb));
                assert_eq!(t.lacks_alpha, opaque, "{usage:?} miscFlags2 {flags2}");
            }
            // 4×4 BC3 uploaded as blocks, and 1×1 B8G8R8A8 converted to RGBA8.
            let aligned = dx10(78, 0, flags2, 4, 4, 1, &odd_payload[..16]);
            assert_eq!(dds_texture(&aligned, TextureUsage::Normal, true, false).unwrap().lacks_alpha, opaque);
            let bgra = dx10(87, 0, flags2, 1, 1, 1, &[1, 2, 3, 4]);
            let t = dds_texture(&bgra, TextureUsage::Color, true, false).unwrap();
            assert_eq!(t.image.data.as_deref(), Some([3u8, 2, 1, 4].as_slice()));
            assert_eq!(t.lacks_alpha, opaque);
        }
        // DX10 BC1 lacks alpha by its format, as legacy DXT1 does.
        assert!(dds_texture(&dx10(71, 0, 0, 4, 4, 1, &[0; 8]), TextureUsage::Normal, true, false).unwrap().lacks_alpha);
    }

    /// An odd-sized BC1 normal map decoded to RGBA8: through a DX10 header it keeps BC1's
    /// "no alpha mask", as the same file on the block grid has. The legacy DXT1 beside it
    /// keeps main's classification (its alpha read as a mask) on purpose; the difference
    /// is pinned here.
    #[test]
    fn odd_sized_dx10_bc1_normal_keeps_no_mask_unlike_its_legacy_counterpart() {
        // 6×5 = 2×2 blocks of 8 bytes, colour 0 white above colour 1: opaque texels.
        let blocks: Vec<u8> = (0..4).flat_map(|_| [0xFF, 0xFF, 0, 0, 0, 0, 0, 0]).collect();
        let dx10_odd = dds_texture(&dx10(71, 0, 0, 6, 5, 1, &blocks), TextureUsage::Normal, true, false).unwrap();
        let legacy_odd = dds_texture(&bc5_tests::dds(b"DXT1", 6, 5, 1, &blocks), TextureUsage::Normal, true, false).unwrap();
        assert_eq!(dx10_odd.image.texture_descriptor.format, TextureFormat::Rgba8Unorm);
        assert_eq!(legacy_odd.image.texture_descriptor.format, TextureFormat::Rgba8Unorm);
        assert_eq!(dx10_odd.image.data, legacy_odd.image.data);
        assert!(dx10_odd.lacks_alpha, "DX10 BC1 carries its source classification through the expansion");
        assert!(!legacy_odd.lacks_alpha, "legacy odd-sized DXT1: unchanged from main");
        // On the block grid both are BC1 blocks without a mask.
        let dx10_even = dds_texture(&dx10(71, 0, 0, 8, 8, 1, &blocks), TextureUsage::Normal, true, false).unwrap();
        let legacy_even = dds_texture(&bc5_tests::dds(b"DXT1", 8, 8, 1, &blocks), TextureUsage::Normal, true, false).unwrap();
        assert!(dx10_even.lacks_alpha && legacy_even.lacks_alpha);
    }

    #[test]
    fn dx10_cube_maps_and_unknown_codes_are_refused() {
        assert!(dds_image(&dx10(98, 0x4, 0, 4, 4, 1, &[0; 96]), true, true).unwrap_err().contains("cube"));
        assert!(dds_image(&dx10(95, 0, 0, 4, 4, 1, &[0; 16]), true, true).unwrap_err().contains("dxgi=95"));
        // The alpha readers report no alpha information for BC7 (no CPU decoder).
        let bc7 = dx10(98, 0, 0, 8, 8, 1, &[0; 64]);
        assert!(alpha_shares(&bc7).is_none());
        assert!(alpha_map(&bc7).is_none());
        // A DX10 BC3 is read by both like a legacy DXT5.
        let mut blocks = Vec::new();
        for a in [0u8, 255, 255, 255] {
            let mut b = [0u8; 16]; b[0] = a; b[1] = a;
            blocks.extend_from_slice(&b);
        }
        let bc3 = dx10(77, 0, 0, 8, 8, 1, &blocks);
        assert_eq!(alpha_shares(&bc3).unwrap(), AlphaShares { clear: 0.25, opaque: 0.75, partial: 0.0 });
        assert_eq!(alpha_map(&bc3).unwrap().alpha[0], 0);
    }

    #[test]
    fn roles_of_one_dx10_file_keep_their_own_handles_and_a_refusal_poisons_none() {
        let workers = WorkerTextures::default();
        let images = Assets::<Image>::default().get_handle_provider();
        let bc7 = dx10(99, 0, 0, 4, 4, 1, &[0; 16]);
        let load = |usage| workers.load_once(("textures\\pbr\\a.dds".into(), usage), &images, |_| dds_texture(&bc7, usage, true, false));
        let (color, linear, normal) = (load(TextureUsage::Color).unwrap(), load(TextureUsage::Linear).unwrap(), load(TextureUsage::Normal).unwrap());
        assert_eq!((color.1, linear.1, normal.1), (TextureFormat::Bc7RgbaUnormSrgb, TextureFormat::Bc7RgbaUnorm, TextureFormat::Bc7RgbaUnorm));
        assert!(color.0 != linear.0 && linear.0 != normal.0 && color.0 != normal.0);
        let bc4 = dx10(80, 0, 0, 4, 4, 1, &[0; 8]);
        let load4 = |usage| workers.load_once(("textures\\pbr\\a_p.dds".into(), usage), &images, |_| dds_texture(&bc4, usage, true, false));
        assert!(load4(TextureUsage::Color).is_none());
        let height = load4(TextureUsage::Linear).unwrap();
        assert_eq!(height.1, TextureFormat::Bc4RUnorm);
        assert!(height.2);
    }

    #[test]
    fn load_key_reuses_worker_negative_cache_without_rereading_archive() {
        let owned = bc5_tests::OwnedData::new();
        let mut cache = TextureCache::default();
        let mut images = Assets::<Image>::default();
        let provider = images.get_handle_provider();
        let path = "textures\\old.dds".to_string();
        let key = (path.clone(), TextureUsage::Color);
        // Simulate a worker having already proven `(path, Color)` invalid/missing.
        assert!(cache.workers.load_once(key.clone(), &provider, |_| Err("worker missing".into())).is_none());
        let gen_before = cache.generation;
        // Even though `old.dds` exists in `owned.data`, `load_key` short-circuits via `workers.failed`.
        assert!(cache.load_key(path.clone(), TextureUsage::Color, false, &owned.data, &mut images, None).is_none());
        assert_eq!(cache.generation, gen_before + 1);
        assert_eq!(cache.map.get(&key), Some(&None));
        assert_eq!(cache.failures.len(), 1);
    }
}
