//! The volumetric clouds' generated noise (#3032 stage 2b): two tileable 3D textures made by our
//! own code on the CPU when the row is first switched on, deterministic, nothing shipped and
//! nothing read from disk.
//!
//! - **The base** (128³ `R8Unorm`, eight levels): one low-frequency Perlin-Worley scalar, the
//!   value the base shape is carved by. Rounded Worley cells (16, 32 and 64 feature cells across a
//!   repeat, the first the lobes) whose sum is modulated by a periodic gradient-noise sum (8, 16
//!   and 32 lattice cells), so the cells' regularity breaks up. Read by the view and the light.
//! - **The erosion** (32³ `Rgba8Unorm`, six levels): three Worley bands in R, G and B at 3, 6 and
//!   12 feature cells across its own repeat, each fadeable on its own; A constant (255).
//!
//! [documented] The technique: one low-frequency Perlin-Worley 3D noise carving the base shape
//! and a higher-frequency Worley set eroding its edges at a second scale (Schneider, "The
//! Real-Time Volumetric Cloudscapes of Horizon Zero Dawn", SIGGRAPH 2015 Advances in Real-Time
//! Rendering); the single 3D lookup carving the shape that the placement and the height profile
//! define (Bauer, "Creating the Atmospheric World of Red Dead Redemption 2", 2019, slides 17-18);
//! cellular noise (Worley, "A Cellular Texture Basis Function", SIGGRAPH 1996); gradient noise
//! with a quintic fade (Perlin, "Improving Noise", SIGGRAPH 2002). Our own code from those
//! descriptions: no code, texture or constant table from any project, tool or shader collection.
//! [engineering choice] The layout, sizes, octave weights and seeds are ours
//! (2.43 MiB for the pair with their level chains).
//!
//! Generation runs on a thread of its own (never the main thread, never a pool's worker waiting
//! on its own pool): Z slabs in parallel on scoped threads, each voxel a pure function of its
//! coordinates, the range found by min and max (order-free) and the levels reduced in integers
//! `(sum + 4) / 8`, so the bytes are the same for any worker count. Packed level by level
//! (largest first), each in increasing Z slices, Y rows, X texels, channels adjacent, no padding:
//! wgpu's mip-major order for a 3D texture.


use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::render::render_resource::{Extent3d, TextureDataOrder, TextureDimension, TextureFormat, TextureUsages};

/// The base texture's side in texels [engineering choice: eight base
/// texels across a 6,000-unit lobe; a power of two keeps every reduction exact].
pub const BASE_SIZE: u32 = 128;
/// The erosion texture's side in texels.
pub const EROSION_SIZE: u32 = 32;
/// The base texture's repeat in world units [engineering choice: 16
/// feature cells of 6,000 units; a 40,000-unit mass sees 0.42 of a repeat].
pub const BASE_REPEAT: f32 = 96_000.0;
/// The erosion texture's repeat in world units (3 / 6 / 12 cells: 4,000 / 2,000 / 1,000 units).
pub const EROSION_REPEAT: f32 = 12_000.0;
/// The base's Worley feature cells across a repeat, and their weights.
const BASE_WORLEY: [(u32, f32); 3] = [(16, 0.625), (32, 0.25), (64, 0.125)];
/// The base's gradient-noise lattice cells across a repeat, and their weights (signed sum).
const BASE_PERLIN: [(u32, f32); 3] = [(8, 0.57), (16, 0.29), (32, 0.14)];
/// The erosion's Worley feature cells across its repeat, R, G, B.
const EROSION_WORLEY: [u32; 3] = [3, 6, 12];
const SEED_WORLEY: u32 = 0x3c6e_f372;
const SEED_PERLIN: u32 = 0xa54f_f53a;
const SEED_EROSION: u32 = 0x510e_527f;

/// One generated texture: its side, channels per texel, levels, and every level's bytes packed
/// mip-major (level 0 first).
#[derive(Clone, Debug, PartialEq)]
pub struct NoiseVolume {
    pub size: u32,
    pub channels: u32,
    pub levels: u32,
    /// Held at their exact length (no growth reserve kept).
    pub bytes: Box<[u8]>,
    /// Each level's first byte in `bytes`.
    pub offsets: Vec<usize>,
    /// Each channel's mean at level 0, 0..1 (for a fade toward the mean).
    pub means: [f32; 4],
}

/// The pair, and how they were made.
#[derive(Clone, Debug, PartialEq)]
pub struct NoiseSet {
    pub base: NoiseVolume,
    pub erosion: NoiseVolume,
    pub workers: usize,
    pub millis: f32,
}

impl NoiseSet {
    /// Bytes of the pair (both level chains).
    pub fn bytes(&self) -> usize {
        self.base.bytes.len() + self.erosion.bytes.len()
    }
}

/// The levels of a cube of side `n` down to 1³.
pub fn level_count(n: u32) -> u32 {
    32 - n.max(1).leading_zeros()
}

/// A 32-bit hash of three lattice coordinates and a seed, 0..1 [engineering choice: integer
/// mixing of our own].
fn hash(x: u32, y: u32, z: u32, seed: u32) -> u32 {
    let mut h = x.wrapping_mul(0x7a3d_1c95) ^ y.wrapping_mul(0x4f1b_bcdd) ^ z.wrapping_mul(0x2b8f_3a61) ^ seed.wrapping_mul(0x9e37_79b9);
    h = (h ^ (h >> 15)).wrapping_mul(0x93c4_67e5);
    h = (h ^ (h >> 13)).wrapping_mul(0xd6e8_feb9);
    h ^ (h >> 16)
}

fn unit(h: u32) -> f32 {
    (h >> 8) as f32 * (1.0 / 16_777_216.0)
}

fn wrap(i: i32, n: u32) -> u32 {
    i.rem_euclid(n as i32) as u32
}

/// Periodic cellular noise at `p` (in repeats, any value) with `cells` feature cells across a
/// repeat, one feature point per cell placed by the hash of its wrapped cell: 1 at a feature
/// point, falling to 0 a cell's width away (one minus the nearest distance in cells, clamped).
pub fn worley(p: [f32; 3], cells: u32, seed: u32) -> f32 {
    let q = p.map(|c| c * cells as f32);
    let base = q.map(|c| c.floor() as i32);
    let mut best = f32::MAX;
    for dz in -1..=1 {
        for dy in -1..=1 {
            for dx in -1..=1 {
                let c = [base[0] + dx, base[1] + dy, base[2] + dz];
                let (wx, wy, wz) = (wrap(c[0], cells), wrap(c[1], cells), wrap(c[2], cells));
                let f = [unit(hash(wx, wy, wz, seed)), unit(hash(wx, wy, wz, seed ^ 0x68e3_1da4)), unit(hash(wx, wy, wz, seed ^ 0x1f83_d9ab))];
                let d = [c[0] as f32 + f[0] - q[0], c[1] as f32 + f[1] - q[1], c[2] as f32 + f[2] - q[2]];
                best = best.min(d[0] * d[0] + d[1] * d[1] + d[2] * d[2]);
            }
        }
    }
    1.0 - best.sqrt().clamp(0.0, 1.0)
}

/// The twelve edge directions of a cube, the gradients' choices.
const GRADIENTS: [[f32; 3]; 12] = [
    [1.0, 1.0, 0.0],
    [-1.0, 1.0, 0.0],
    [1.0, -1.0, 0.0],
    [-1.0, -1.0, 0.0],
    [1.0, 0.0, 1.0],
    [-1.0, 0.0, 1.0],
    [1.0, 0.0, -1.0],
    [-1.0, 0.0, -1.0],
    [0.0, 1.0, 1.0],
    [0.0, -1.0, 1.0],
    [0.0, 1.0, -1.0],
    [0.0, -1.0, -1.0],
];

fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Periodic gradient noise at `p` (in repeats) on a lattice of `cells` across a repeat: a
/// gradient per wrapped lattice point, the quintic fade between; about -1..1, 0 at the points.
pub fn perlin(p: [f32; 3], cells: u32, seed: u32) -> f32 {
    let q = p.map(|c| c * cells as f32);
    let i = q.map(|c| c.floor());
    let f = [q[0] - i[0], q[1] - i[1], q[2] - i[2]];
    let ii = i.map(|c| c as i32);
    let u = f.map(fade);
    let corner = |dx: i32, dy: i32, dz: i32| {
        let g = GRADIENTS[(hash(wrap(ii[0] + dx, cells), wrap(ii[1] + dy, cells), wrap(ii[2] + dz, cells), seed) % 12) as usize];
        g[0] * (f[0] - dx as f32) + g[1] * (f[1] - dy as f32) + g[2] * (f[2] - dz as f32)
    };
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let x00 = lerp(corner(0, 0, 0), corner(1, 0, 0), u[0]);
    let x10 = lerp(corner(0, 1, 0), corner(1, 1, 0), u[0]);
    let x01 = lerp(corner(0, 0, 1), corner(1, 0, 1), u[0]);
    let x11 = lerp(corner(0, 1, 1), corner(1, 1, 1), u[0]);
    lerp(lerp(x00, x10, u[1]), lerp(x01, x11, u[1]), u[2])
}

/// The base's raw value at `p` (in repeats): the Worley sum (high near rounded feature centres)
/// times a factor the gradient-noise sum moves by up to two fifths either way.
pub fn base_raw(p: [f32; 3]) -> f32 {
    let w: f32 = BASE_WORLEY.iter().enumerate().map(|(k, &(c, wt))| wt * worley(p, c, SEED_WORLEY.wrapping_add(k as u32))).sum();
    let g: f32 = BASE_PERLIN.iter().enumerate().map(|(k, &(c, wt))| wt * perlin(p, c, SEED_PERLIN.wrapping_add(k as u32))).sum();
    w * (1.0 + 0.4 * g.clamp(-1.0, 1.0))
}

/// The erosion's raw bands at `p` (in its repeats).
pub fn erosion_raw(p: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|k| worley(p, EROSION_WORLEY[k], SEED_EROSION.wrapping_add(k as u32)))
}

/// A field of `n`³ voxels with `channels` raw values each, computed in Z slabs on `workers`
/// scoped threads (each slab a pure function of its coordinates: the same for any count).
fn field(n: u32, channels: usize, workers: usize, f: &(dyn Fn([f32; 3]) -> [f32; 3] + Sync)) -> Vec<f32> {
    let n = n as usize;
    let slab = n * n * channels;
    let mut out = vec![0.0f32; n * slab];
    let workers = workers.clamp(1, n);
    let fill = |z: usize, dst: &mut [f32]| {
        for y in 0..n {
            for x in 0..n {
                let p = [(x as f32 + 0.5) / n as f32, (y as f32 + 0.5) / n as f32, (z as f32 + 0.5) / n as f32];
                let v = f(p);
                let at = (y * n + x) * channels;
                dst[at..at + channels].copy_from_slice(&v[..channels]);
            }
        }
    };
    if workers == 1 {
        for (z, dst) in out.chunks_mut(slab).enumerate() {
            fill(z, dst);
        }
        return out;
    }
    let per = n.div_ceil(workers);
    std::thread::scope(|s| {
        for (w, chunk) in out.chunks_mut(per * slab).enumerate() {
            let fill = &fill;
            s.spawn(move || {
                for (k, dst) in chunk.chunks_mut(slab).enumerate() {
                    fill(w * per + k, dst);
                }
            });
        }
    });
    out
}

/// Each channel spread over 0..255 by its own range (order-free min and max), level 0 bytes;
/// `constant` channels (beyond the raw ones) are 255.
fn quantize(raw: &[f32], raw_channels: usize, channels: usize) -> Vec<u8> {
    let texels = raw.len() / raw_channels;
    let mut lo = [f32::MAX; 4];
    let mut hi = [f32::MIN; 4];
    for t in raw.chunks(raw_channels) {
        for c in 0..raw_channels {
            lo[c] = lo[c].min(t[c]);
            hi[c] = hi[c].max(t[c]);
        }
    }
    let mut out = Vec::with_capacity(texels * channels);
    for t in raw.chunks(raw_channels) {
        for c in 0..channels {
            out.push(if c < raw_channels { (((t[c] - lo[c]) / (hi[c] - lo[c]).max(1e-6)).clamp(0.0, 1.0) * 255.0).round() as u8 } else { 255 });
        }
    }
    out
}

/// The next level of a cube of side `n` (even, or 1): each texel `(sum of its eight children +
/// 4) / 8` per channel.
pub fn reduce(level: &[u8], n: usize, channels: usize) -> Vec<u8> {
    let m = (n / 2).max(1);
    let mut out = vec![0u8; m * m * m * channels];
    for z in 0..m {
        for y in 0..m {
            for x in 0..m {
                for c in 0..channels {
                    let mut sum = 0u32;
                    for (dz, dy, dx) in (0..8).map(|k| ((k >> 2) & 1, (k >> 1) & 1, k & 1)) {
                        let (sx, sy, sz) = ((2 * x + dx).min(n - 1), (2 * y + dy).min(n - 1), (2 * z + dz).min(n - 1));
                        sum += u32::from(level[((sz * n + sy) * n + sx) * channels + c]);
                    }
                    out[((z * m + y) * m + x) * channels + c] = ((sum + 4) / 8) as u8;
                }
            }
        }
    }
    out
}

/// A volume from its level-0 bytes: the level chain reduced and packed mip-major.
pub fn volume(level0: Vec<u8>, size: u32, channels: u32) -> NoiseVolume {
    let levels = level_count(size);
    let ch = channels as usize;
    let texels = (level0.len() / ch).max(1) as f32;
    let means = std::array::from_fn(|c| if c < ch { level0.iter().skip(c).step_by(ch).map(|&b| f32::from(b)).sum::<f32>() / texels / 255.0 } else { 0.0 });
    // The chain's exact length, reserved once.
    let total: usize = (0..levels).map(|l| ((size as usize) >> l).max(1).pow(3) * ch).sum();
    let mut bytes = Vec::with_capacity(total);
    let mut offsets = Vec::with_capacity(levels as usize);
    let mut level = level0;
    let mut n = size as usize;
    for _ in 0..levels {
        offsets.push(bytes.len());
        bytes.extend_from_slice(&level);
        if n > 1 {
            level = reduce(&level, n, ch);
            n /= 2;
        }
    }
    NoiseVolume { size, channels, levels, bytes: bytes.into_boxed_slice(), offsets, means }
}

/// Bake both textures at the given sides on `workers` threads.
pub fn bake_sized(base: u32, erosion: u32, workers: usize) -> NoiseSet {
    let start = std::time::Instant::now();
    let b = field(base, 1, workers, &|p| [base_raw(p), 0.0, 0.0]);
    let e = field(erosion, 3, workers, &erosion_raw);
    NoiseSet {
        base: volume(quantize(&b, 1, 1), base, 1),
        erosion: volume(quantize(&e, 3, 4), erosion, 4),
        workers: workers.max(1),
        millis: start.elapsed().as_secs_f32() * 1000.0,
    }
}

/// Bake the production pair on up to eight threads of the machine's.
pub fn bake() -> NoiseSet {
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 8);
    bake_sized(BASE_SIZE, EROSION_SIZE, workers)
}

/// The level a footprint `width` (world units) reads from a texture of side `size` repeating
/// every `repeat` units with `levels` levels: log2 of the footprint in texels, from 0 (a texel
/// or less) to the last level.
#[cfg_attr(not(test), allow(dead_code))]
pub fn level_for(width: f32, size: u32, repeat: f32, levels: u32) -> f32 {
    let texels = if width.is_finite() { width * size as f32 / repeat.max(1e-3) } else { f32::MAX };
    texels.max(1.0).log2().clamp(0.0, levels.saturating_sub(1) as f32)
}

impl NoiseVolume {
    #[cfg_attr(not(test), allow(dead_code))]
    fn texel(&self, level: u32, x: i32, y: i32, z: i32, c: usize) -> f32 {
        let n = (self.size >> level).max(1);
        let (x, y, z) = (wrap(x, n) as usize, wrap(y, n) as usize, wrap(z, n) as usize);
        let n = n as usize;
        f32::from(self.bytes[self.offsets[level as usize] + ((z * n + y) * n + x) * self.channels as usize + c]) / 255.0
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn trilinear(&self, level: u32, uvw: [f32; 3]) -> [f32; 4] {
        let n = (self.size >> level).max(1) as f32;
        let s = uvw.map(|c| c * n - 0.5);
        let i = s.map(|c| c.floor());
        let f = [s[0] - i[0], s[1] - i[1], s[2] - i[2]];
        let (x, y, z) = (i[0] as i32, i[1] as i32, i[2] as i32);
        let mut out = [0.0f32; 4];
        for (c, o) in out.iter_mut().enumerate().take(self.channels as usize) {
            let mut acc = 0.0;
            for k in 0..8 {
                let (dx, dy, dz) = (k & 1, (k >> 1) & 1, (k >> 2) & 1);
                let w = (if dx == 1 { f[0] } else { 1.0 - f[0] }) * (if dy == 1 { f[1] } else { 1.0 - f[1] }) * (if dz == 1 { f[2] } else { 1.0 - f[2] });
                acc += w * self.texel(level, x + dx, y + dy, z + dz, c);
            }
            *o = acc;
        }
        out
    }

    /// The texture at `uvw` (in repeats, any value: it wraps) at the fractional level `lod`, as a
    /// repeating trilinear sampler with linear mip filtering reads it (the GPU's within its
    /// filtering precision): each level sampled at `uvw x side - 0.5`, the two levels around
    /// `lod` blended, `lod` held to 0..last.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn sample(&self, uvw: [f32; 3], lod: f32) -> [f32; 4] {
        let last = self.levels.saturating_sub(1);
        let lod = if lod.is_finite() { lod.clamp(0.0, last as f32) } else { 0.0 };
        let l0 = lod.floor() as u32;
        let l1 = (l0 + 1).min(last);
        let t = lod - l0 as f32;
        let a = self.trilinear(l0, uvw);
        if t <= 0.0 || l1 == l0 {
            return a;
        }
        let b = self.trilinear(l1, uvw);
        std::array::from_fn(|c| a[c] + (b[c] - a[c]) * t)
    }

    /// The volume as a Bevy image: a 3D texture with its whole level chain, uploaded once (never
    /// changed after `Assets::add`; a new activation makes a new asset). Its bytes are a copy (the
    /// upload's own, which the render world takes): made on the noise's thread, never the main
    /// one (volumetric_clouds.rs `prepare_noise`).
    pub fn image(&self) -> Image {
        let format = if self.channels == 1 { TextureFormat::R8Unorm } else { TextureFormat::Rgba8Unorm };
        let mut img = Image::new_uninit(Extent3d { width: self.size, height: self.size, depth_or_array_layers: self.size }, TextureDimension::D3, format, RenderAssetUsages::RENDER_WORLD);
        img.texture_descriptor.mip_level_count = self.levels;
        img.texture_descriptor.sample_count = 1;
        img.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST;
        img.texture_descriptor.view_formats = &[];
        img.data_order = TextureDataOrder::MipMajor;
        img.data = Some(self.bytes.to_vec());
        img
    }
}

/// A short digest of bytes (FNV-1a, 64 bits), for the determinism pin.
#[cfg_attr(not(test), allow(dead_code))]
pub fn digest(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// One production bake per test process (the calibration's mirror samples the same bytes the
/// shader does).
#[cfg(test)]
pub(crate) fn production() -> &'static NoiseSet {
    static SET: std::sync::OnceLock<NoiseSet> = std::sync::OnceLock::new();
    SET.get_or_init(bake)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The production bytes' digest, pinned from a run of the test [None until then:
    /// the test prints the value to pin].
    const PINNED: Option<(u64, u64)> = Some((0xf3fd_c5ef_6658_fc4a, 0x66be_b14c_36f7_7429));

    #[test]
    fn the_bake_is_deterministic_for_any_worker_count() {
        let a = bake_sized(16, 8, 1);
        for workers in [2, 3, 8] {
            let b = bake_sized(16, 8, workers);
            assert_eq!((&a.base, &a.erosion), (&b.base, &b.erosion), "{workers} workers");
        }
        let p = production();
        let got = (digest(&p.base.bytes), digest(&p.erosion.bytes));
        println!(
            "noise: base {}³ x{} levels, erosion {}³ x{} levels, {} bytes, {} workers, {:.0} ms; digests {got:x?}; base mean {:.3}, erosion means {:?}; texels base[0..4] {:?} erosion[0..8] {:?}",
            p.base.size, p.base.levels, p.erosion.size, p.erosion.levels, p.bytes(), p.workers, p.millis, p.base.means[0], &p.erosion.means[..3], &p.base.bytes[..4], &p.erosion.bytes[..8]
        );
        if let Some(pin) = PINNED {
            assert_eq!(got, pin, "the generated bytes changed");
        }
        assert_eq!(p.bytes(), 2_396_745 + 149_796, "2.43 MiB for the pair");
        assert_eq!((p.base.levels, p.erosion.levels), (8, 6));
    }

    /// Ranges and means: each channel spans 0..255 (spread by its own range), the base's mean in
    /// the middle part, the erosion's alpha constant.
    #[test]
    fn the_channels_span_their_range() {
        let p = production();
        let l0 = &p.base.bytes[..p.base.offsets[1]];
        assert_eq!((*l0.iter().min().unwrap(), *l0.iter().max().unwrap()), (0, 255));
        assert!((0.2..0.8).contains(&p.base.means[0]), "base mean {}", p.base.means[0]);
        let e0 = &p.erosion.bytes[..p.erosion.offsets[1]];
        for (c, &mean) in p.erosion.means.iter().enumerate().take(3) {
            let ch: Vec<u8> = e0.iter().skip(c).step_by(4).copied().collect();
            assert_eq!((*ch.iter().min().unwrap(), *ch.iter().max().unwrap()), (0, 255), "erosion channel {c}");
            assert!((0.2..0.9).contains(&mean), "erosion mean {c}: {mean}");
        }
        assert!(e0.iter().skip(3).step_by(4).all(|&a| a == 255));
    }

    /// Tileable: across the wrap the neighbour pairs differ no more than inside, per axis,
    /// channel and level (levels of side 4 and more).
    #[test]
    fn the_textures_wrap_without_a_seam() {
        let p = production();
        for v in [&p.base, &p.erosion] {
            for level in 0..v.levels {
                let n = (v.size >> level) as i32;
                if n < 4 {
                    continue;
                }
                for c in 0..(v.channels.min(3) as usize) {
                    for axis in 0..3 {
                        let at = |i: i32, a: i32, b: i32| match axis {
                            0 => v.texel(level, i, a, b, c),
                            1 => v.texel(level, a, i, b, c),
                            _ => v.texel(level, a, b, i, c),
                        };
                        let (mut seam, mut inner, mut count) = (0.0f64, 0.0f64, 0.0f64);
                        for a in 0..n {
                            for b in 0..n {
                                seam += f64::from((at(n - 1, a, b) - at(0, a, b)).abs());
                                inner += f64::from((at(n / 2 - 1, a, b) - at(n / 2, a, b)).abs());
                                count += 1.0;
                            }
                        }
                        let (seam, inner) = (seam / count, inner / count);
                        assert!(seam <= 1.5 * inner + 0.01, "size {} level {level} channel {c} axis {axis}: seam {seam:.4}, inside {inner:.4}", v.size);
                    }
                }
            }
        }
    }

    /// Each coarser level is `(sum + 4) / 8` of its eight children; the packing holds every
    /// level at its offset (a hand-filled asymmetric cube exposes swapped axes, wrong offsets and
    /// dropped slices).
    #[test]
    fn the_levels_are_the_rounded_means_of_their_children() {
        let n = 4usize;
        let level0: Vec<u8> = (0..n * n * n).flat_map(|i| [(i * 7 % 251) as u8, (i * 3) as u8]).collect();
        let v = volume(level0.clone(), n as u32, 2);
        assert_eq!(v.levels, 3);
        assert_eq!(v.offsets, vec![0, 128, 144]);
        assert_eq!(v.bytes.len(), 146);
        // Level 1 texel (1, 0, 1), channel 0: children x 2..4, y 0..2, z 2..4.
        let mut sum = 0u32;
        for z in 2..4 {
            for y in 0..2 {
                for x in 2..4 {
                    sum += u32::from(level0[((z * n + y) * n + x) * 2]);
                }
            }
        }
        assert_eq!(v.bytes[v.offsets[1] + 10], ((sum + 4) / 8) as u8);
        assert_eq!(v.texel(1, 1, 0, 1, 0), ((sum + 4) / 8) as f32 / 255.0);
        // The last level: the mean of level 1.
        let l1 = &v.bytes[128..144];
        let s: u32 = l1.iter().step_by(2).map(|&b| u32::from(b)).sum();
        assert_eq!(v.bytes[144], ((s + 4) / 8) as u8);
        let p = production();
        assert_eq!(p.base.offsets[1], 128 * 128 * 128);
        assert_eq!(p.erosion.offsets[1], 32 * 32 * 32 * 4);
    }

    /// The sampler against hand-computed trilinear values, across the wrap on all three axes,
    /// with negative coordinates and a fractional level.
    #[test]
    fn the_sampler_is_trilinear_and_wraps() {
        let n = 4usize;
        let level0: Vec<u8> = (0..n * n * n).map(|i| ((i * 37 + 11) % 256) as u8).collect();
        let v = volume(level0, n as u32, 1);
        let t = |x: i32, y: i32, z: i32| v.texel(0, x, y, z, 0);
        // At a texel centre: the texel.
        assert!((v.sample([1.5 / 4.0, 2.5 / 4.0, 0.5 / 4.0], 0.0)[0] - t(1, 2, 0)).abs() < 1e-6);
        // At the corner of the cube (0, 0, 0): halfway between texels 3 and 0 on every axis.
        let mut want = 0.0;
        for k in 0..8 {
            want += 0.125 * t(if k & 1 == 1 { 0 } else { 3 }, if k & 2 == 2 { 0 } else { 3 }, if k & 4 == 4 { 0 } else { 3 });
        }
        assert!((v.sample([0.0, 0.0, 0.0], 0.0)[0] - want).abs() < 1e-6);
        assert!((v.sample([-1.0, 2.0, 5.0], 0.0)[0] - want).abs() < 1e-5, "whole repeats away, negative too");
        // A quarter of the way from texel (0,1,2) to (1,1,2) in x.
        let got = v.sample([(0.5 + 0.25) / 4.0, 1.5 / 4.0, 2.5 / 4.0], 0.0)[0];
        assert!((got - (0.75 * t(0, 1, 2) + 0.25 * t(1, 1, 2))).abs() < 1e-6);
        // A fractional level blends the two levels; past the last it holds the last.
        let a = v.sample([0.3, 0.6, 0.1], 0.0)[0];
        let b = v.sample([0.3, 0.6, 0.1], 1.0)[0];
        assert!((v.sample([0.3, 0.6, 0.1], 0.25)[0] - (0.75 * a + 0.25 * b)).abs() < 1e-6);
        assert_eq!(v.sample([0.3, 0.6, 0.1], 9.0), v.sample([0.7, 0.2, 0.9], 2.0), "the last level is one texel");
    }

    /// The level rule: never negative, monotone in the footprint, capped at the last level.
    #[test]
    fn the_level_follows_the_footprint() {
        let mut last = -1.0;
        for w in [0.0f32, 100.0, 750.0, 1_300.0, 3_000.0, 1e5, 1e9, f32::INFINITY] {
            let l = level_for(w, BASE_SIZE, BASE_REPEAT, 8);
            assert!((0.0..=7.0).contains(&l) && l >= last, "{w}: {l}");
            last = l;
        }
        assert_eq!(level_for(750.0, BASE_SIZE, BASE_REPEAT, 8), 0.0, "one texel");
        assert!((level_for(1_300.0, BASE_SIZE, BASE_REPEAT, 8) - 0.794).abs() < 0.01);
        assert!((level_for(1_300.0, EROSION_SIZE, EROSION_REPEAT, 6) - 1.794).abs() < 0.01);
        assert_eq!(level_for(f32::NAN, BASE_SIZE, BASE_REPEAT, 8), 7.0);
    }

    #[test]
    fn the_images_carry_their_levels() {
        let p = production();
        let img = p.base.image();
        assert_eq!(img.texture_descriptor.mip_level_count, 8);
        assert_eq!(img.texture_descriptor.dimension, TextureDimension::D3);
        assert_eq!(img.texture_descriptor.format, TextureFormat::R8Unorm);
        assert_eq!(img.data.as_ref().map(Vec::len), Some(2_396_745));
        assert!(matches!(img.data_order, TextureDataOrder::MipMajor));
        let e = p.erosion.image();
        assert_eq!((e.texture_descriptor.format, e.texture_descriptor.mip_level_count, e.texture_descriptor.size.depth_or_array_layers), (TextureFormat::Rgba8Unorm, 6, 32));
    }
}
