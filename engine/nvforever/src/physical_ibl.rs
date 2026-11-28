//! Procedural HDR sky/horizon/ground/interior cubemap IBL (the Physical lighting row).
//!
//! With Physical lighting on, Bevy's flat `ambient::ambient_light` is bypassed in
//! world views in favour of [`EnvironmentMapLight`] cubemaps (`Rgba16Float`, `LayerMajor`)
//! synthesized from [`SkyState`](crate::sky::SkyState):
//! - Outdoors: normalized sky-dome zenith/lower/horizon irradiance + sunlit Mojave ground
//!   bounce (for diffuse IBL) plus an HDR solar aureole and disc (for GGX specular IBL).
//! - Indoors: cell ambient ceiling/floor bounce + directional room highlight lobe.
//!
//! Cubemap `Handle<Image>` assets are allocated once and kept bound on every world camera;
//! subsequent updates stream all 6 faces and all mip levels directly into the existing GPU
//! textures via [`RenderQueue::write_texture`] in `RenderSystems::PrepareResources` so
//! `RenderAssets<GpuImage>` never drops the handles or skips mips on rebuild frames.

use crate::fly::FlyCam;
use crate::reflection::ReflectionCamera;
use crate::scope_view::LensCamera;
use crate::sky::{AMBIENT_SHARE, INTERIOR_AMBIENT_SHARE, SkyState, interior_light_from, light_unit};
use crate::world::game_to_bevy;
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::EnvironmentMapLight;
use bevy::prelude::*;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::{
    Extent3d, Origin3d, TexelCopyBufferLayout, TexelCopyTextureInfo, TextureAspect, TextureDataOrder,
    TextureDimension, TextureFormat, TextureUsages, TextureViewDescriptor, TextureViewDimension,
};
use bevy::render::renderer::RenderQueue;
use bevy::render::texture::GpuImage;
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderSystems};
use fnv_world::sky::{ColorType, smoothstep};
use std::f32::consts::TAU;

const DIFFUSE_SIZE: u32 = 16;
const SPECULAR_SIZE: u32 = 32;
const SPECULAR_MIPS: u32 = 5;
const SKY_LUT_SIZE: usize = 16;
const BYTES_PER_TEXEL: u32 = 8;

fn cubemap_total_bytes(size: u32, mips: u32) -> usize {
    (0..mips).map(|m| (size >> m).max(1).pow(2)).sum::<u32>() as usize * 6 * BYTES_PER_TEXEL as usize
}

pub struct PhysicalIblPlugin;

impl Plugin for PhysicalIblPlugin {
    fn build(&self, app: &mut App) {
        if !nvf_assets::authored::physical_pbr() {
            return;
        }
        app.init_resource::<PhysicalIblState>()
            .add_systems(Update, update_physical_ibl.after(crate::sky::update_sky).after(crate::emittance::update_emittance));
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render
                .init_resource::<PhysicalIblUpload>()
                .add_systems(ExtractSchedule, extract_physical_ibl)
                .add_systems(Render, write_physical_ibl.in_set(RenderSystems::PrepareResources));
        }
    }
}

#[derive(Resource, Default)]
struct PhysicalIblState {
    last: Option<IblParams>,
    diffuse: Option<Handle<Image>>,
    specular: Option<Handle<Image>>,
    diff_bytes: Vec<u8>,
    spec_bytes: Vec<u8>,
    /// Incremented on every in-place rebuild after initial `Image` asset creation.
    sent: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum IblParams {
    Outdoors {
        zenith: Vec3,
        mid: Vec3,
        horizon: Vec3,
        ambient: Vec3,
        sun_color: Vec3,
        sun_dir: Vec3,
        flash: Vec3,
        clearness: f32,
    },
    Indoors {
        ambient: Vec3,
        dir_color: Vec3,
        dir_to_light: Vec3,
    },
}

impl IblParams {
    #[cfg(test)]
    fn from_sky(sky: &SkyState) -> Self {
        Self::from_sky_with_daylight(sky, 0.0)
    }

    pub(crate) fn from_sky_with_daylight(sky: &SkyState, daylight_luma: f32) -> Self {
        if let Some(l) = sky.interior {
            let day_lift = 1.0 + 0.45 * daylight_luma.clamp(0.0, 1.0);
            let ambient = Vec3::from_array(l.ambient.map(fnv_world::sky::srgb_to_linear)) * (INTERIOR_AMBIENT_SHARE * day_lift);
            let dir_color = Vec3::from_array(l.directional.map(fnv_world::sky::srgb_to_linear)) * (l.directional_fade.clamp(0.0, 1.0) * day_lift);
            let dir_to_light = (game_to_bevy().rotation * interior_light_from(l.directional_rotation_xy, l.directional_rotation_z))
                .normalize_or(Vec3::Y);
            Self::Indoors { ambient, dir_color, dir_to_light }
        } else {
            let zenith = Vec3::from_array(sky.color(ColorType::SkyUpper));
            let mid = Vec3::from_array(sky.color(ColorType::SkyLower));
            let horizon = Vec3::from_array(sky.color(ColorType::Horizon));
            let ambient = Vec3::from_array(sky.color(ColorType::Ambient));
            let clearness = sky.physical_sky_clearness();
            let sun_color = Vec3::from_array(sky.physical_sun_color()) * sky.sun_scale();
            let sun_dir = sky.sun_dir().normalize_or(Vec3::Y);
            let flash = Vec3::from_array(sky.flash);
            Self::Outdoors { zenith, mid, horizon, ambient, sun_color, sun_dir, flash, clearness }
        }
    }

    fn needs_rebuild(self, prev: Option<Self>) -> bool {
        let Some(prev) = prev else { return true };
        match (self, prev) {
            (
                Self::Outdoors { zenith: z1, mid: m1, horizon: h1, ambient: a1, sun_color: sc1, sun_dir: sd1, flash: f1, clearness: c1 },
                Self::Outdoors { zenith: z2, mid: m2, horizon: h2, ambient: a2, sun_color: sc2, sun_dir: sd2, flash: f2, clearness: c2 },
            ) => {
                let d = |x: Vec3, y: Vec3| (x - y).abs().max_element() > 0.008;
                d(z1, z2) || d(m1, m2) || d(h1, h2) || d(a1, a2) || d(sc1, sc2) || d(f1, f2) || (c1 - c2).abs() > 0.02 || sd1.dot(sd2) < 0.9985
            }
            (
                Self::Indoors { ambient: a1, dir_color: d1, dir_to_light: l1 },
                Self::Indoors { ambient: a2, dir_color: d2, dir_to_light: l2 },
            ) => (a1 - a2).abs().max_element() > 0.008 || (d1 - d2).abs().max_element() > 0.008 || l1.dot(l2) < 0.9985,
            _ => true,
        }
    }
}

struct CompiledEnvironment {
    outdoors: bool,
    horizon: Vec3,
    mid: Vec3,
    zenith: Vec3,
    sky_scale: Vec3,
    target_sky: Vec3,
    ground: Vec3,
    sun_color: Vec3,
    sun_dir: Vec3,
    dir_color: Vec3,
    dir_to_light: Vec3,
    sky_lut: Option<Box<[[Vec3; 256]; 6]>>,
}

const OUTDOOR_IBL_SCALE: f32 = 0.62;
const INDOOR_IBL_SCALE: f32 = 0.45;

fn sample_cube_lut(lut: &[[Vec3; 256]; 6], dir: Vec3) -> Vec3 {
    let c = Vec3::new(dir.x, dir.y, -dir.z);
    let (ax, ay, az) = (c.x.abs(), c.y.abs(), c.z.abs());
    let (face, sc, tc) = if ax >= ay && ax >= az {
        let ma = ax.max(1e-6);
        if c.x >= 0.0 {
            (0usize, -c.z / ma, -c.y / ma)
        } else {
            (1usize, c.z / ma, -c.y / ma)
        }
    } else if ay >= az {
        let ma = ay.max(1e-6);
        if c.y >= 0.0 {
            (2usize, c.x / ma, c.z / ma)
        } else {
            (3usize, c.x / ma, -c.z / ma)
        }
    } else {
        let ma = az.max(1e-6);
        if c.z >= 0.0 {
            (4usize, c.x / ma, -c.y / ma)
        } else {
            (5usize, -c.x / ma, -c.y / ma)
        }
    };
    let u = (0.5 * sc + 0.5).clamp(0.0, 1.0);
    let v = (0.5 * tc + 0.5).clamp(0.0, 1.0);
    let fx = u * SKY_LUT_SIZE as f32 - 0.5;
    let fy = v * SKY_LUT_SIZE as f32 - 0.5;
    let max_idx = (SKY_LUT_SIZE - 1) as i32;
    let x0 = (fx.floor() as i32).clamp(0, max_idx) as usize;
    let y0 = (fy.floor() as i32).clamp(0, max_idx) as usize;
    let x1 = (x0 + 1).min(SKY_LUT_SIZE - 1);
    let y1 = (y0 + 1).min(SKY_LUT_SIZE - 1);
    let tx = (fx - x0 as f32).clamp(0.0, 1.0);
    let ty = (fy - y0 as f32).clamp(0.0, 1.0);
    let face_lut = &lut[face];
    let c00 = face_lut[y0 * SKY_LUT_SIZE + x0];
    let c10 = face_lut[y0 * SKY_LUT_SIZE + x1];
    let c01 = face_lut[y1 * SKY_LUT_SIZE + x0];
    let c11 = face_lut[y1 * SKY_LUT_SIZE + x1];
    c00.lerp(c10, tx).lerp(c01.lerp(c11, tx), ty)
}

impl CompiledEnvironment {
    fn new(params: IblParams) -> Self {
        match params {
            IblParams::Outdoors { zenith, mid, horizon, ambient, sun_color, sun_dir, flash, clearness } => {
                let target_sky = (ambient * (AMBIENT_SHARE * OUTDOOR_IBL_SCALE) + flash).max(Vec3::ZERO);
                // Integrate raw sky gradient cosine-weighted over upper hemisphere y in [0, 1].
                let raw_dome = |y: f32| -> Vec3 {
                    let lo = horizon.lerp(mid, smoothstep(0.0, 0.35, y));
                    lo.lerp(zenith, smoothstep(0.35, 1.0, y)) + Vec3::splat(0.02)
                };
                let steps = 32;
                let mut avg = Vec3::ZERO;
                for i in 0..steps {
                    let y = (i as f32 + 0.5) / steps as f32;
                    avg += raw_dome(y) * (2.0 * y / steps as f32);
                }
                let luma = |v: Vec3| v.dot(Vec3::new(0.2126, 0.7152, 0.0722));
                let lum_scale = if luma(avg) > 1e-5 { luma(target_sky) / luma(avg) } else { 0.0 };
                let per_ch = Vec3::new(
                    if avg.x > 1e-5 { target_sky.x / avg.x } else { 0.0 },
                    if avg.y > 1e-5 { target_sky.y / avg.y } else { 0.0 },
                    if avg.z > 1e-5 { target_sky.z / avg.z } else { 0.0 },
                );
                let sky_scale = Vec3::splat(lum_scale) * 0.65 + per_ch * 0.35;
                let h_lit = horizon + Vec3::splat(0.02);
                let m_lit = mid + Vec3::splat(0.02);
                let z_lit = zenith + Vec3::splat(0.02);
                let base_ground = (target_sky + sun_color * (sun_dir.y.clamp(0.0, 1.0) * 1.1)) * Vec3::new(0.34, 0.25, 0.16);
                let (sky_lut, ground) = if clearness > 0.0 {
                    let phys_weight = smoothstep(0.10, 0.85, clearness);
                    let day_vis = smoothstep(-0.14, 0.06, sun_dir.y);
                    let weather_sky_at = |y: f32| -> Vec3 {
                        let lo = h_lit.lerp(m_lit, smoothstep(0.0, 0.35, y));
                        lo.lerp(z_lit, smoothstep(0.35, 1.0, y)) * sky_scale
                    };
                    let mut lut = Box::new([[Vec3::ZERO; 256]; 6]);
                    for face in 0..6u32 {
                        for y in 0..SKY_LUT_SIZE {
                            let v = (y as f32 + 0.5) / SKY_LUT_SIZE as f32;
                            for x in 0..SKY_LUT_SIZE {
                                let u = (x as f32 + 0.5) / SKY_LUT_SIZE as f32;
                                let dir = cube_to_world(sample_cube_dir(u, v, face));
                                let phys = crate::sky::eval_physical_sky(dir, sun_dir, clearness)
                                    + target_sky * ((1.0 - day_vis) * 0.75)
                                    + flash;
                                let weather = weather_sky_at(dir.y.max(0.0));
                                lut[face as usize][y * SKY_LUT_SIZE + x] = weather.lerp(phys, phys_weight);
                            }
                        }
                    }
                    let sun_elev = sun_dir.y.clamp(0.0, 1.0);
                    let phys_ground = (target_sky * (1.0 - day_vis * 0.5)
                        + sun_color * (sun_elev * 1.45)
                        + Vec3::new(0.04, 0.09, 0.18) * day_vis)
                        * Vec3::new(0.32, 0.24, 0.15);
                    (Some(lut), base_ground.lerp(phys_ground, phys_weight))
                } else {
                    (None, base_ground)
                };
                Self {
                    outdoors: true,
                    horizon: h_lit,
                    mid: m_lit,
                    zenith: z_lit,
                    sky_scale,
                    target_sky,
                    ground,
                    sun_color,
                    sun_dir,
                    dir_color: Vec3::ZERO,
                    dir_to_light: Vec3::Y,
                    sky_lut,
                }
            }
            IblParams::Indoors { ambient, dir_color, dir_to_light } => Self {
                outdoors: false,
                horizon: Vec3::ZERO,
                mid: Vec3::ZERO,
                zenith: Vec3::ZERO,
                sky_scale: Vec3::ONE,
                target_sky: (ambient * INDOOR_IBL_SCALE).max(Vec3::ZERO),
                ground: Vec3::ZERO,
                sun_color: Vec3::ZERO,
                sun_dir: Vec3::Y,
                dir_color: (dir_color * INDOOR_IBL_SCALE).max(Vec3::ZERO),
                dir_to_light,
                sky_lut: None,
            },
        }
    }

    fn eval_sky_with_boost(&self, dir: Vec3, lut_boost: f32) -> Vec3 {
        if !self.outdoors {
            let t = smoothstep(-1.0, 1.0, dir.y);
            let grad = 0.68 + 0.38 * t;
            let bounce = self.dir_color * (0.08 * dir.dot(self.dir_to_light).max(0.0).powi(2));
            return self.target_sky * grad + bounce;
        }
        if let Some(lut) = &self.sky_lut {
            if dir.y >= 0.0 {
                return sample_cube_lut(lut, dir) * lut_boost;
            }
            let horiz_dir = Vec3::new(dir.x, 0.0, dir.z)
                .try_normalize()
                .map_or(Vec3::new(1.0, 0.01, 0.0), |xz| (xz + Vec3::new(0.0, 0.01, 0.0)).normalize());
            let h = sample_cube_lut(lut, horiz_dir) * lut_boost;
            let t = smoothstep(0.0, 1.0, (-dir.y).powf(0.55));
            return h.lerp(self.ground, t);
        }
        let sky_at = |y: f32| -> Vec3 {
            let lo = self.horizon.lerp(self.mid, smoothstep(0.0, 0.35, y));
            lo.lerp(self.zenith, smoothstep(0.35, 1.0, y)) * self.sky_scale
        };
        if dir.y >= 0.0 {
            sky_at(dir.y)
        } else {
            let h = sky_at(0.0);
            let t = smoothstep(0.0, 1.0, (-dir.y).powf(0.55));
            h.lerp(self.ground, t)
        }
    }

    fn eval_diffuse_sky(&self, dir: Vec3) -> Vec3 {
        self.eval_sky_with_boost(dir, 0.85)
    }

    fn eval_specular_sky(&self, dir: Vec3) -> Vec3 {
        let base = self.eval_sky_with_boost(dir, 1.0);
        if !self.outdoors {
            let highlight = self.dir_color * (0.28 * dir.dot(self.dir_to_light).max(0.0).powi(4));
            return base + highlight;
        }
        let horizon_vis = smoothstep(-0.08, 0.15, self.sun_dir.y);
        let ground_mask = smoothstep(-0.05, 0.02, dir.y);
        let cos_s = dir.dot(self.sun_dir).clamp(0.0, 1.0);
        let aureole = 0.22 * cos_s.powi(12) + 1.2 * cos_s.powi(64);
        let disc = smoothstep(0.9985, 0.9996, cos_s) * 8.0;
        base + self.sun_color * (horizon_vis * ground_mask * (aureole + disc))
    }
}

/// Exact inverse of `bevy_pbr::render::utils::sample_cube_dir`.
fn sample_cube_dir(u: f32, v: f32, face: u32) -> Vec3 {
    let sc = 2.0 * u - 1.0;
    let tc = 2.0 * v - 1.0;
    let dir = match face {
        0 => Vec3::new(1.0, -tc, -sc),
        1 => Vec3::new(-1.0, -tc, sc),
        2 => Vec3::new(sc, 1.0, tc),
        3 => Vec3::new(sc, -1.0, -tc),
        4 => Vec3::new(sc, -tc, 1.0),
        _ => Vec3::new(-sc, -tc, -1.0),
    };
    dir.normalize()
}

/// World-space vector matching `environment_map.wesl`'s `sample_dir.z = -sample_dir.z`.
fn cube_to_world(cube_dir: Vec3) -> Vec3 {
    Vec3::new(cube_dir.x, cube_dir.y, -cube_dir.z)
}

/// Branchless orthonormal basis (Duff et al. 2017).
fn tangent_basis(n: Vec3) -> (Vec3, Vec3) {
    let sign = if n.z >= 0.0 { 1.0 } else { -1.0 };
    let a = -1.0 / (sign + n.z);
    let b = n.x * n.y * a;
    let t = Vec3::new(1.0 + sign * n.x * n.x * a, sign * b, -sign * n.x);
    let bitangent = Vec3::new(b, sign + n.y * n.y * a, -n.y);
    (t, bitangent)
}

fn radical_inverse_vdc(mut bits: u32) -> f32 {
    bits = bits.rotate_right(16);
    bits = ((bits & 0x5555_5555) << 1) | ((bits & 0xAAAA_AAAA) >> 1);
    bits = ((bits & 0x3333_3333) << 2) | ((bits & 0xCCCC_CCCC) >> 2);
    bits = ((bits & 0x0F0F_0F0F) << 4) | ((bits & 0xF0F0_F0F0) >> 4);
    bits = ((bits & 0x00FF_00FF) << 8) | ((bits & 0xFF00_FF00) >> 8);
    bits as f32 * 2.328_306_4e-10
}

fn convolve_diffuse(env: &CompiledEnvironment, n: Vec3) -> Vec3 {
    const N: u32 = 32;
    const GOLDEN_ANGLE: f32 = 2.399_963_2;
    let (t, b) = tangent_basis(n);
    let mut sum = Vec3::ZERO;
    for i in 0..N {
        // Cosine-weighted Fibonacci hemisphere: p(w) = cos(theta)/pi, so average = irradiance.
        let r2 = (i as f32 + 0.5) / N as f32;
        let sin_theta = r2.sqrt();
        let cos_theta = (1.0 - r2).max(0.0).sqrt();
        let phi = i as f32 * GOLDEN_ANGLE;
        let l = (t * (sin_theta * phi.cos()) + b * (sin_theta * phi.sin()) + n * cos_theta).normalize();
        sum += env.eval_diffuse_sky(l);
    }
    sum / N as f32
}

fn prefilter_specular(env: &CompiledEnvironment, n: Vec3, mip: u32) -> Vec3 {
    if mip == 0 {
        return env.eval_specular_sky(n);
    }
    let sample_count: u32 = match mip {
        1 => 32,
        2 => 48,
        _ => 64,
    };
    let pr = mip as f32 / (SPECULAR_MIPS - 1) as f32;
    let alpha = (pr * pr).max(0.045 * 0.045);
    let a2 = alpha * alpha;
    let (t, b) = tangent_basis(n);
    let mut sum = Vec3::ZERO;
    let mut weight = 0.0f32;
    for i in 0..sample_count {
        let u = i as f32 / sample_count as f32;
        let v = radical_inverse_vdc(i);
        let phi = TAU * u;
        let cos_theta = ((1.0 - v) / (1.0 + (a2 - 1.0) * v)).max(0.0).sqrt();
        let sin_theta = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
        let h = t * (sin_theta * phi.cos()) + b * (sin_theta * phi.sin()) + n * cos_theta;
        let l = (2.0 * n.dot(h) * h - n).normalize();
        let ndl = n.dot(l).max(0.0);
        if ndl > 0.0 {
            sum += env.eval_specular_sky(l) * ndl;
            weight += ndl;
        }
    }
    if weight > 0.0 { sum / weight } else { env.eval_specular_sky(n) }
}

/// Converts a non-negative `f32` sky radiance sample into IEEE-754 `f16` bits (`[0, 65504]`).
pub(crate) fn f32_to_f16_bits(v: f32) -> u16 {
    if v.is_nan() || v <= 0.0 {
        return 0;
    }
    if !v.is_finite() || v >= 65504.0 {
        return 0x7bff;
    }
    let bits = v.to_bits();
    let exp = ((bits >> 23) & 0xff) as i32 - 127;
    let mant = bits & 0x7f_ffff;
    if exp < -24 {
        return 0;
    }
    if exp < -14 {
        // Subnormal f16
        let shift = (-exp - 14) + 13;
        let full = 0x80_0000 | mant;
        let round = 1u32 << (shift - 1);
        return ((full + round) >> shift) as u16;
    }
    let exp_h = (exp + 15) as u16;
    let rounded_mant = mant + 0x1000;
    ((exp_h << 10) as u32 + (rounded_mant >> 13)).min(0x7bff) as u16
}

fn push_rgba16f(out: &mut Vec<u8>, c: Vec3) {
    for v in [c.x, c.y, c.z, 1.0] {
        out.extend_from_slice(&f32_to_f16_bits(v).to_le_bytes());
    }
}

fn cubemap_image(size: u32, mips: u32, data: Vec<u8>, label: &'static str) -> Image {
    let mut img = Image::new(
        Extent3d { width: size, height: size, depth_or_array_layers: 6 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    img.texture_descriptor.mip_level_count = mips;
    img.texture_descriptor.usage |= TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST;
    img.texture_descriptor.label = Some(label);
    img.data_order = TextureDataOrder::LayerMajor;
    img.texture_view_descriptor = Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..default() });
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..ImageSamplerDescriptor::linear()
    });
    img
}

fn write_cubemap_bytes(params: IblParams, diff_bytes: &mut Vec<u8>, spec_bytes: &mut Vec<u8>) {
    let env = CompiledEnvironment::new(params);

    diff_bytes.clear();
    diff_bytes.reserve(cubemap_total_bytes(DIFFUSE_SIZE, 1));
    for face in 0..6 {
        let inv = 1.0 / DIFFUSE_SIZE as f32;
        for y in 0..DIFFUSE_SIZE {
            let v = (y as f32 + 0.5) * inv;
            for x in 0..DIFFUSE_SIZE {
                let u = (x as f32 + 0.5) * inv;
                let dir = cube_to_world(sample_cube_dir(u, v, face));
                push_rgba16f(diff_bytes, convolve_diffuse(&env, dir));
            }
        }
    }

    spec_bytes.clear();
    spec_bytes.reserve(cubemap_total_bytes(SPECULAR_SIZE, SPECULAR_MIPS));
    for face in 0..6 {
        for mip in 0..SPECULAR_MIPS {
            let side = (SPECULAR_SIZE >> mip).max(1);
            let inv = 1.0 / side as f32;
            for y in 0..side {
                let v = (y as f32 + 0.5) * inv;
                for x in 0..side {
                    let u = (x as f32 + 0.5) * inv;
                    let dir = cube_to_world(sample_cube_dir(u, v, face));
                    push_rgba16f(spec_bytes, prefilter_specular(&env, dir, mip));
                }
            }
        }
    }
}

#[cfg(test)]
fn build_cubemaps(params: IblParams) -> (Image, Image) {
    let (mut diff_bytes, mut spec_bytes) = (Vec::new(), Vec::new());
    write_cubemap_bytes(params, &mut diff_bytes, &mut spec_bytes);
    (
        cubemap_image(DIFFUSE_SIZE, 1, diff_bytes, "nvf_physical_ibl_diffuse"),
        cubemap_image(SPECULAR_SIZE, SPECULAR_MIPS, spec_bytes, "nvf_physical_ibl_specular"),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CubemapSubresource {
    face: u32,
    mip: u32,
    side: u32,
    offset: usize,
    len: usize,
}

fn cubemap_subresources(size: u32, mips: u32) -> impl Iterator<Item = CubemapSubresource> {
    let mut face = 0u32;
    let mut mip = 0u32;
    let mut offset = 0usize;
    std::iter::from_fn(move || {
        if face >= 6 {
            return None;
        }
        let side = (size >> mip).max(1);
        let len = (side * side * BYTES_PER_TEXEL) as usize;
        let sub = CubemapSubresource { face, mip, side, offset, len };
        offset += len;
        mip += 1;
        if mip >= mips {
            mip = 0;
            face += 1;
        }
        Some(sub)
    })
}

fn write_cubemap_texture(queue: &RenderQueue, gpu: &GpuImage, bytes: &[u8], size: u32, mips: u32) {
    for sub in cubemap_subresources(size, mips) {
        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &gpu.texture,
                mip_level: sub.mip,
                origin: Origin3d { x: 0, y: 0, z: sub.face },
                aspect: TextureAspect::All,
            },
            &bytes[sub.offset..sub.offset + sub.len],
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(sub.side * BYTES_PER_TEXEL),
                rows_per_image: Some(sub.side),
            },
            Extent3d { width: sub.side, height: sub.side, depth_or_array_layers: 1 },
        );
    }
}

#[derive(Resource, Default)]
struct PhysicalIblUpload {
    diffuse: Option<AssetId<Image>>,
    specular: Option<AssetId<Image>>,
    seen: u64,
    diff_bytes: Vec<u8>,
    spec_bytes: Vec<u8>,
    pending: bool,
}

impl PhysicalIblUpload {
    /// Copy rebuilt cubemap bytes from the main world when `sent` advances.
    fn receive(&mut self, state: &PhysicalIblState) {
        self.diffuse = state.diffuse.as_ref().map(Handle::id);
        self.specular = state.specular.as_ref().map(Handle::id);
        if state.sent != self.seen {
            self.seen = state.sent;
            self.diff_bytes.clone_from(&state.diff_bytes);
            self.spec_bytes.clone_from(&state.spec_bytes);
            self.pending = true;
        }
    }

    /// Return the pending diffuse and specular bytes once both GPU textures exist;
    /// until then keep `pending` set so the update waits rather than dropping.
    fn take_write(&mut self, textures_ready: bool) -> Option<(&[u8], &[u8])> {
        if !self.pending || !textures_ready {
            return None;
        }
        self.pending = false;
        if self.diff_bytes.len() != cubemap_total_bytes(DIFFUSE_SIZE, 1)
            || self.spec_bytes.len() != cubemap_total_bytes(SPECULAR_SIZE, SPECULAR_MIPS)
        {
            return None;
        }
        Some((&self.diff_bytes, &self.spec_bytes))
    }
}

fn extract_physical_ibl(state: Extract<Option<Res<PhysicalIblState>>>, mut upload: ResMut<PhysicalIblUpload>) {
    if let Some(state) = state.as_deref() {
        upload.receive(state);
    }
}

/// Write rebuilt cubemap faces and mips into the persistent GPU textures in-place, after
/// `prepare_assets::<GpuImage>` and before rendering. Keeping the `Handle<Image>` assets
/// constant prevents `LightProbePlugin` from dropping `view_cubemap_index` to `-1` while
/// rebuilt handles wait behind `RenderAssetBytesPerFrame`.
fn write_physical_ibl(mut upload: ResMut<PhysicalIblUpload>, images: Res<RenderAssets<GpuImage>>, queue: Res<RenderQueue>) {
    let diff_gpu = upload.diffuse.and_then(|id| images.get(id));
    let spec_gpu = upload.specular.and_then(|id| images.get(id));
    let Some((diff_bytes, spec_bytes)) = upload.take_write(diff_gpu.is_some() && spec_gpu.is_some()) else {
        return;
    };
    let (Some(diff_gpu), Some(spec_gpu)) = (diff_gpu, spec_gpu) else {
        return;
    };
    write_cubemap_texture(&queue, diff_gpu, diff_bytes, DIFFUSE_SIZE, 1);
    write_cubemap_texture(&queue, spec_gpu, spec_bytes, SPECULAR_SIZE, SPECULAR_MIPS);
}

#[allow(clippy::type_complexity)]
fn update_physical_ibl(
    mut commands: Commands,
    sky: Res<SkyState>,
    emittance: Option<Res<crate::emittance::EmittanceState>>,
    mut images: ResMut<Assets<Image>>,
    mut state: ResMut<PhysicalIblState>,
    mut cameras: Query<
        (Entity, Option<&mut EnvironmentMapLight>),
        (With<Camera3d>, Or<(With<FlyCam>, With<ReflectionCamera>, With<LensCamera>)>),
    >,
) {
    // With authored reflections this run, their cube keeps the view's environment map
    // (lights.rs `gating`): the sky's light is not made or attached.
    if nvf_assets::reflection_cube::enabled() {
        return;
    }
    let daylight = emittance.as_deref().map_or(0.0, |e| e.weather_daylight_luma);
    let params = IblParams::from_sky_with_daylight(&sky, daylight);
    if params.needs_rebuild(state.last) {
        let state = &mut *state;
        let initial = state.diffuse.is_none() || state.specular.is_none();
        write_cubemap_bytes(params, &mut state.diff_bytes, &mut state.spec_bytes);
        if initial {
            state.diffuse = Some(images.add(cubemap_image(
                DIFFUSE_SIZE,
                1,
                state.diff_bytes.clone(),
                "nvf_physical_ibl_diffuse",
            )));
            state.specular = Some(images.add(cubemap_image(
                SPECULAR_SIZE,
                SPECULAR_MIPS,
                state.spec_bytes.clone(),
                "nvf_physical_ibl_specular",
            )));
        } else {
            state.sent += 1;
        }
        state.last = Some(params);
    }

    let (Some(diffuse_map), Some(specular_map)) = (&state.diffuse, &state.specular) else {
        return;
    };
    let intensity = light_unit();

    for (entity, existing) in &mut cameras {
        if let Some(mut env) = existing {
            if env.diffuse_map != *diffuse_map {
                env.diffuse_map = diffuse_map.clone();
            }
            if env.specular_map != *specular_map {
                env.specular_map = specular_map.clone();
            }
            if (env.intensity - intensity).abs() > 1e-3 {
                env.intensity = intensity;
            }
        } else {
            commands.entity(entity).insert(EnvironmentMapLight {
                diffuse_map: diffuse_map.clone(),
                specular_map: specular_map.clone(),
                intensity,
                rotation: Quat::IDENTITY,
                affects_lightmapped_mesh_diffuse: true,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f16_to_f32(bits: u16) -> f32 {
        let exp = ((bits >> 10) & 0x1f) as i32;
        let mant = (bits & 0x3ff) as u32;
        if exp == 0 {
            return (mant as f32) * 2.0f32.powi(-24);
        }
        if exp == 31 {
            return f32::INFINITY;
        }
        (1.0 + mant as f32 / 1024.0) * 2.0f32.powi(exp - 15)
    }

    fn read_texel_rgb(data: &[u8], offset_bytes: usize) -> Vec3 {
        let r = f16_to_f32(u16::from_le_bytes([data[offset_bytes], data[offset_bytes + 1]]));
        let g = f16_to_f32(u16::from_le_bytes([data[offset_bytes + 2], data[offset_bytes + 3]]));
        let b = f16_to_f32(u16::from_le_bytes([data[offset_bytes + 4], data[offset_bytes + 5]]));
        Vec3::new(r, g, b)
    }

    #[test]
    fn f32_to_f16_conversion_handles_normal_subnormal_and_clamping() {
        assert_eq!(f32_to_f16_bits(0.0), 0);
        assert_eq!(f32_to_f16_bits(-1.0), 0);
        assert_eq!(f32_to_f16_bits(f32::NAN), 0);
        assert_eq!(f32_to_f16_bits(100_000.0), 0x7bff);
        assert_eq!(f32_to_f16_bits(f32::INFINITY), 0x7bff);
        for v in [0.0001f32, 0.05, 0.5, 1.0, 2.5, 12.25, 600.0] {
            let decoded = f16_to_f32(f32_to_f16_bits(v));
            assert!((decoded - v).abs() <= v * 0.002 + 1e-4, "v={v} decoded={decoded}");
        }
    }

    #[test]
    fn cubemap_byte_sizes_and_uniform_irradiance_match_spec() {
        let uniform = IblParams::Indoors {
            ambient: Vec3::new(0.4, 0.5, 0.6),
            dir_color: Vec3::ZERO,
            dir_to_light: Vec3::Y,
        };
        let (diff, spec) = build_cubemaps(uniform);
        let diff_len = diff.data.as_ref().unwrap().len();
        let spec_len = spec.data.as_ref().unwrap().len();
        assert_eq!(diff_len, (DIFFUSE_SIZE * DIFFUSE_SIZE * 6 * 8) as usize);
        let expected_spec_texels: u32 = (0..SPECULAR_MIPS).map(|m| (SPECULAR_SIZE >> m).pow(2)).sum::<u32>() * 6;
        assert_eq!(spec_len, expected_spec_texels as usize * 8);
        let indoor_env = CompiledEnvironment::new(uniform);
        let indoor_irr = convolve_diffuse(&indoor_env, Vec3::Y);
        assert!(indoor_irr.x < 0.4 && indoor_irr.y < 0.5 && indoor_irr.z < 0.6, "indoor IBL stays below raw ambient: {indoor_irr}");

        let pure_uniform = IblParams::Outdoors {
            zenith: Vec3::splat(0.5),
            mid: Vec3::splat(0.5),
            horizon: Vec3::splat(0.5),
            ambient: Vec3::splat(0.5 / AMBIENT_SHARE),
            sun_color: Vec3::ZERO,
            sun_dir: Vec3::Y,
            flash: Vec3::ZERO,
            clearness: 0.0,
        };
        let (diff_u, _) = build_cubemaps(pure_uniform);
        let env = CompiledEnvironment::new(pure_uniform);
        let irr_up = convolve_diffuse(&env, Vec3::Y);
        assert!((irr_up - Vec3::splat(0.5 * OUTDOOR_IBL_SCALE)).abs().max_element() < 0.01, "upper hemisphere avg matches target: {irr_up}");
        assert!(read_texel_rgb(diff_u.data.as_ref().unwrap(), 0).x > 0.0);

        let blue_sky = CompiledEnvironment::new(IblParams::Outdoors {
            zenith: Vec3::new(0.10, 0.25, 0.70),
            mid: Vec3::new(0.25, 0.40, 0.65),
            horizon: Vec3::new(0.60, 0.58, 0.52),
            ambient: Vec3::new(0.45, 0.36, 0.28),
            sun_color: Vec3::new(1.0, 0.9, 0.75),
            sun_dir: Vec3::new(0.0, 0.8, -0.6).normalize(),
            flash: Vec3::ZERO,
            clearness: 0.0,
        });
        let blue_irr = convolve_diffuse(&blue_sky, Vec3::Y);
        assert!(blue_irr.z > blue_irr.x, "chromatic sky zenith keeps cool sky fill above: {blue_irr}");
    }

    #[test]
    fn specular_sun_peak_aligns_with_bevy_z_flip_convention() {
        // Sun is at North in Bevy (-Z), 45 degrees above horizon.
        let sun_dir = Vec3::new(0.0, 1.0, -1.0).normalize();
        let params = IblParams::Outdoors {
            zenith: Vec3::new(0.1, 0.2, 0.4),
            mid: Vec3::new(0.2, 0.3, 0.5),
            horizon: Vec3::new(0.5, 0.4, 0.3),
            ambient: Vec3::splat(0.3),
            sun_color: Vec3::new(1.0, 0.9, 0.7),
            sun_dir,
            flash: Vec3::ZERO,
            clearness: 0.0,
        };
        let (_, spec) = build_cubemaps(params);
        let data = spec.data.as_ref().unwrap();
        let face_stride = (0..SPECULAR_MIPS).map(|m| (SPECULAR_SIZE >> m).pow(2)).sum::<u32>() as usize * 8;
        // Look up face 4 (+Z in cube space -> -Z in world space because cube_to_world flips Z), row y = 0.
        let center = (SPECULAR_SIZE / 2) as usize;
        let north_y0 = read_texel_rgb(data, 4 * face_stride + center * 8);
        // Compare against opposite face 5 (-Z in cube space -> +Z in world space, opposite sun).
        let south_mid = read_texel_rgb(data, 5 * face_stride + (center * SPECULAR_SIZE as usize + center) * 8);
        assert!(north_y0.x > south_mid.x * 2.0, "sun glow lies in -Z world (cube +Z face): north={north_y0} south={south_mid}");
    }

    #[test]
    fn cubemap_subresources_cover_layer_major_layout_without_gaps() {
        let diff_subs: Vec<_> = cubemap_subresources(DIFFUSE_SIZE, 1).collect();
        assert_eq!(diff_subs.len(), 6);
        let mut cursor = 0usize;
        for (face, sub) in diff_subs.iter().enumerate() {
            assert_eq!(sub.face, face as u32);
            assert_eq!(sub.mip, 0);
            assert_eq!(sub.side, DIFFUSE_SIZE);
            assert_eq!(sub.offset, cursor);
            cursor += sub.len;
        }
        assert_eq!(cursor, cubemap_total_bytes(DIFFUSE_SIZE, 1));

        let spec_subs: Vec<_> = cubemap_subresources(SPECULAR_SIZE, SPECULAR_MIPS).collect();
        assert_eq!(spec_subs.len(), 6 * SPECULAR_MIPS as usize);
        cursor = 0;
        let mut idx = 0;
        for face in 0..6u32 {
            for mip in 0..SPECULAR_MIPS {
                let sub = spec_subs[idx];
                let expected_side = SPECULAR_SIZE >> mip;
                assert_eq!(sub.face, face);
                assert_eq!(sub.mip, mip);
                assert_eq!(sub.side, expected_side);
                assert_eq!(sub.offset, cursor);
                cursor += sub.len;
                idx += 1;
            }
        }
        assert_eq!(cursor, cubemap_total_bytes(SPECULAR_SIZE, SPECULAR_MIPS));
    }

    #[test]
    fn upload_waits_for_gpu_textures_and_coalesces_updates() {
        let mut images = Assets::<Image>::default();
        let diff_len = cubemap_total_bytes(DIFFUSE_SIZE, 1);
        let spec_len = cubemap_total_bytes(SPECULAR_SIZE, SPECULAR_MIPS);
        let mut state = PhysicalIblState {
            last: None,
            diffuse: Some(images.add(cubemap_image(DIFFUSE_SIZE, 1, vec![0; diff_len], "d"))),
            specular: Some(images.add(cubemap_image(SPECULAR_SIZE, SPECULAR_MIPS, vec![0; spec_len], "s"))),
            diff_bytes: vec![0; diff_len],
            spec_bytes: vec![0; spec_len],
            sent: 0,
        };
        let mut upload = PhysicalIblUpload::default();

        // Initial image handles already contain their frame-0 data; no queue write needed yet.
        upload.receive(&state);
        assert!(!upload.pending);
        assert!(upload.take_write(true).is_none());

        // Rebuild while GPU textures aren't ready yet stays pending, and newer updates overwrite it.
        state.sent = 1;
        state.diff_bytes.fill(0x11);
        state.spec_bytes.fill(0x12);
        upload.receive(&state);
        assert!(upload.pending);
        assert!(upload.take_write(false).is_none());
        assert!(upload.pending);

        state.sent = 2;
        state.diff_bytes.fill(0x21);
        state.spec_bytes.fill(0x22);
        upload.receive(&state);
        let (d, s) = upload.take_write(true).expect("ready textures consume pending bytes");
        assert!(d.iter().all(|&b| b == 0x21));
        assert!(s.iter().all(|&b| b == 0x22));
        assert!(!upload.pending);
        assert!(upload.take_write(true).is_none());

        // Re-extracting unchanged `sent` does not re-queue a write.
        upload.receive(&state);
        assert!(!upload.pending);
    }

    #[test]
    fn rebuilds_preserve_handles_and_advance_upload_counter() {
        let mut world = World::new();
        world.insert_resource(SkyState::default());
        world.insert_resource(Assets::<Image>::default());
        world.insert_resource(PhysicalIblState::default());
        let cam = world.spawn((Camera3d::default(), ReflectionCamera)).id();

        let mut schedule = Schedule::default();
        schedule.add_systems(update_physical_ibl);
        schedule.run(&mut world);

        let first_diff = world.resource::<PhysicalIblState>().diffuse.as_ref().unwrap().id();
        let first_spec = world.resource::<PhysicalIblState>().specular.as_ref().unwrap().id();
        assert_eq!(world.resource::<PhysicalIblState>().sent, 0);
        assert_eq!(world.resource::<Assets<Image>>().len(), 2);
        let env = world.entity(cam).get::<EnvironmentMapLight>().unwrap();
        assert_eq!(env.diffuse_map.id(), first_diff);
        assert_eq!(env.specular_map.id(), first_spec);

        let mut upload = PhysicalIblUpload::default();
        upload.receive(world.resource::<PhysicalIblState>());
        assert!(!upload.pending);

        // Trigger a sky change exceeding rebuild threshold; handles must stay identical.
        world.resource_mut::<SkyState>().flash = [0.5, 0.5, 0.5];
        schedule.run(&mut world);

        let state = world.resource::<PhysicalIblState>();
        assert_eq!(state.diffuse.as_ref().unwrap().id(), first_diff);
        assert_eq!(state.specular.as_ref().unwrap().id(), first_spec);
        assert_eq!(state.sent, 1);
        assert_eq!(world.resource::<Assets<Image>>().len(), 2);
        let env = world.entity(cam).get::<EnvironmentMapLight>().unwrap();
        assert_eq!(env.diffuse_map.id(), first_diff);
        assert_eq!(env.specular_map.id(), first_spec);

        upload.receive(state);
        assert!(upload.pending);
        assert!(upload.take_write(true).is_some());
    }

    #[test]
    fn cube_lut_round_trips_all_six_faces_and_builds_physical_desert_ibl() {
        let mut lut = [[Vec3::ZERO; 256]; 6];
        for face in 0..6u32 {
            for y in 0..SKY_LUT_SIZE {
                let v = (y as f32 + 0.5) / SKY_LUT_SIZE as f32;
                for x in 0..SKY_LUT_SIZE {
                    let u = (x as f32 + 0.5) / SKY_LUT_SIZE as f32;
                    let dir = cube_to_world(sample_cube_dir(u, v, face));
                    lut[face as usize][y * SKY_LUT_SIZE + x] = dir * 0.5 + Vec3::splat(0.5);
                }
            }
        }
        for face in 0..6u32 {
            for (u, v) in [(0.5, 0.5), (0.25, 0.75), (0.8125, 0.1875)] {
                let dir = cube_to_world(sample_cube_dir(u, v, face));
                let want = dir * 0.5 + Vec3::splat(0.5);
                let got = sample_cube_lut(&lut, dir);
                assert!((got - want).abs().max_element() < 0.004, "face {face} uv ({u},{v}): got={got} want={want}");
            }
        }

        let env = CompiledEnvironment::new(IblParams::Outdoors {
            zenith: Vec3::new(0.05, 0.15, 0.45),
            mid: Vec3::new(0.15, 0.25, 0.45),
            horizon: Vec3::new(0.45, 0.48, 0.50),
            ambient: Vec3::new(0.28, 0.25, 0.22),
            sun_color: Vec3::new(1.0, 0.92, 0.78),
            sun_dir: Vec3::new(0.0, 0.55, -0.835).normalize(),
            flash: Vec3::ZERO,
            clearness: 1.0,
        });
        assert!(env.sky_lut.is_some());
        let up_irr = convolve_diffuse(&env, Vec3::Y);
        let ground_irr = convolve_diffuse(&env, Vec3::NEG_Y);
        assert!(up_irr.z > up_irr.x, "clear desert upper hemisphere IBL has cool cobalt sky fill: {up_irr}");
        assert!(ground_irr.x > ground_irr.z, "lower hemisphere IBL has warm Mojave ground bounce: {ground_irr}");
    }

    #[test]
    fn windowed_daylight_lifts_indoor_ibl_while_night_and_windowless_stay_unchanged() {
        let mut sky = SkyState::default();
        let lighting = fnv_world::lighting::CellLighting {
            ambient: [80, 72, 64],
            directional: [120, 108, 92],
            directional_fade: 1.0,
            ..Default::default()
        };
        sky.interior = Some(lighting);

        let night = IblParams::from_sky_with_daylight(&sky, 0.0);
        let base = IblParams::from_sky(&sky);
        assert_eq!(night, base);

        let day = IblParams::from_sky_with_daylight(&sky, 1.0);
        let night_irr = convolve_diffuse(&CompiledEnvironment::new(night), Vec3::Y);
        let day_irr = convolve_diffuse(&CompiledEnvironment::new(day), Vec3::Y);
        assert!(day_irr.x > night_irr.x * 1.4, "day={day_irr} night={night_irr}");
    }
}
