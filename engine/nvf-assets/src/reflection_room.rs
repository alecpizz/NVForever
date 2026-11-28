//! The analytic room: what a reflection cube shows indoors, and the lamp-lit ground outdoors
//! at night (#2981 reflections, stage 2 version 1; docs/spec-notes/pbr-materials.md
//! "Reflections indoors: the analytic room") [design]. NVForever's own rendering: the
//! original draws no ambient reflection (docs/design/layer3.md), and nothing here describes
//! it. Pure: no ECS, no GPU; nvforever reflection_sky.rs gathers the scene and hands the
//! result to `ReflectionCube::fill`.
//!
//! - **The room** ([`RoomScene`]): a point at the eye inside one box (centre, orthonormal
//!   axes, half extents; Bevy world space, +Y up). Toward a direction d the eye sees the
//!   box's inside where the ray from the eye leaves it; that point is a floor (inward normal
//!   up), a ceiling (down) or a wall, with the albedo [`FLOOR_ALBEDO`], [`CEILING_ALBEDO`]
//!   or [`WALL_ALBEDO`] given as linear colours by the caller. It is lit the way the fork's
//!   display-value sum lights a diffuse surface (nvforever lights/display_shader.rs:22-33,
//!   :105-111): light = ambient + directional x N.L + the sum over the lamps of colour x
//!   brightness x attenuation x N.L (x the spot cone, display_shader.rs:35-47), and the
//!   surface writes the linear colour of min(albedo's display value x light, 1)
//!   ([`lit_texel`], the rule nvforever reflection_sky.rs `ground_texel` uses). The
//!   attenuation is the game's that the shader uses in place of Bevy's:
//!   saturate(1 - d^2 / radius^2) (nvforever lights.rs:25-27,
//!   `GAME_ATTENUATION`, itself a [hypothesis] there) ([`attenuation`]). No shadows: the
//!   engine's point and spot lights cast none (`shadow_maps_enabled: false`, lights.rs:354,
//!   :363, :642, :658), so a lamp behind a wall lights it here as it does on screen. The
//!   cell's fog, when given, mixes in linear light by distance as the patched fog does indoors
//!   (nvforever fog.rs `share`, :126-130).
//! - **Lamps seen directly**: a lamp inside the box (within [`LAMP_LOBE_MARGIN`] of its
//!   walls) adds a small lobe toward itself, its converted colour ([`display_texel`]) times
//!   [`LAMP_LOBE_PEAK`] times cos^[`LAMP_LOBE_POWER`] of the angle to it, so a metal shows
//!   where the lights are.
//! - **The ground outdoors** ([`GroundLamps`]): below the horizon, level ground at a height
//!   under the eye lit by the same lamps; only what they add to the ground stage 1 already
//!   draws (its ambient and sun) goes into the cube, with their lobes.
//! - **Levels** ([`FilteredCube`]): level 0 is the scene per texel direction; each higher
//!   level is a box average of the level below (each texel the solid-angle-weighted mean of
//!   its four); where Bevy's lobe reaches or exceeds the texel's angular span (levels 2
//!   to 5; level 1's, about 3.8 degrees, is inside its 5.6-degree texel), it is instead the box average
//!   at level 2 (8 x 8 a face) convolved on the CPU with the prefilter kernel max(c, 0) x ((1 + c) / 2)^(p / 2), c the cosine between
//!   the texel's and a source texel's direction, p = 2 / a^2 - 2 the Blinn-Phong power of
//!   GGX roughness a = r^2, r = level / (levels - 1) the perceptual roughness Bevy reads that
//!   level for (bevy_pbr `environment_map.wesl` `radiance_level`); (1 + c) / 2 is the
//!   squared cosine of the half-angle, since the reflected ray turns twice as far as the
//!   normal. At the last level (a = 1) this is a cosine lobe. Every texel and weight is
//!   solid-angle weighted, so a level keeps about the mean of level 0.

use bevy::math::{Mat3, Vec3};

use crate::reflection_cube::{level_side, world_direction, SPECULAR_LEVELS};

/// The floor's assumed albedo as display values (texture bytes / 255): stage 1's ground
/// (nvforever reflection_sky.rs `GROUND_ALBEDO`), so an empty room's floor is stage 1's
/// ground [design].
pub const FLOOR_ALBEDO: [f32; 3] = [0.33, 0.30, 0.25];
/// The walls' assumed albedo as display values, a little lighter and less warm than the
/// floor [design: a guess, to judge in pictures].
pub const WALL_ALBEDO: [f32; 3] = [0.40, 0.38, 0.35];
/// The ceiling's assumed albedo as display values [design: a guess, to judge in pictures].
pub const CEILING_ALBEDO: [f32; 3] = [0.45, 0.45, 0.43];
/// A lamp seen directly: its lobe's peak as a share of the lamp's converted colour x
/// brightness [design: as bright as a white wall the lamp lights at full].
pub const LAMP_LOBE_PEAK: f32 = 1.0;
/// The lobe's power at level 0: cos^256 halves at about 4.2 degrees, a texel and a half of
/// the 32-texel face [design].
pub const LAMP_LOBE_POWER: f32 = 256.0;
/// A lamp counts as inside the box (its lobe drawn) up to this far beyond the wall the
/// eye sees behind it, game units: lamps hang on walls and ceilings and room markers are
/// drawn by hand [design].
pub const LAMP_LOBE_MARGIN: f32 = 32.0;
/// The share of the cell's directional light (x N.L) the box's surfaces take: the shader
/// lights every surface with it, and its shadows (it casts on layer 0 indoors too) are left
/// out here [design: 1, as the shader; whether the cell's own walls shadow it in the drawn
/// picture is to judge in pictures].
pub const DIRECTIONAL_ON_WALLS: f32 = 1.0;
/// Relative kernel weight below which a source texel is left out of a level's convolution.
const KERNEL_CUTOFF: f32 = 1e-4;
/// The finest level a convolution reads (8 x 8 a face): coarse levels are convolved from
/// it, not from their own few texels, which cannot hold a lobe as wide as a face.
const CONVOLVE_SOURCE_LEVEL: u32 = 2;

/// The game's attenuation as the patched shader computes it (nvforever lights.rs:25-27,
/// `GAME_ATTENUATION`: saturate(1 - distanceSquare x inverseRangeSquared), with
/// inverseRangeSquared = 1 / range^2): 1 at the lamp, 0 at and beyond its radius.
pub fn attenuation(distance_squared: f32, radius: f32) -> f32 {
    if radius <= 0.0 {
        return 0.0;
    }
    (1.0 - distance_squared / (radius * radius)).clamp(0.0, 1.0)
}

/// What the display-value sum writes for a diffuse surface of linear colour `albedo` under
/// the display-value light `light`: the linear colour of the albedo's display value x
/// light, held to 1 per channel, both ways by the sRGB curve (nvforever
/// lights/display_shader.rs, the last of `DISPLAY_EDITS`; reflection_sky.rs `ground_texel`
/// is this function).
pub fn lit_texel(albedo: Vec3, light: Vec3) -> Vec3 {
    let shown = (albedo.map(display_value) * light.max(Vec3::ZERO)).min(Vec3::ONE);
    albedo_linear(shown.to_array())
}

/// The display value (sRGB curve) of a linear colour channel, the inverse of
/// [`albedo_linear`]; negative input is held at 0.
pub fn display_value(linear: f32) -> f32 {
    let v = linear.max(0.0);
    if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}

/// The units identity of the cube (nvf_assets `ReflectionCube`, "Units"; nvforever
/// reflection_sky.rs `texel`, a test there checks the two agree): a display value v as the
/// linear texel v x max(v, 1e-6)^1.2. Negative input is held at 0.
pub fn display_texel(display: Vec3) -> Vec3 {
    display.map(|a| {
        let a = a.max(0.0);
        a * a.max(1e-6).powf(1.2)
    })
}

/// A display-value colour (bytes / 255) as the shader's linear diffuse colour (the sRGB
/// curve, as nvforever lodgen.rs `srgb_to_linear`).
pub fn albedo_linear(display: [f32; 3]) -> Vec3 {
    Vec3::from_array(display.map(|c| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }))
}

/// A spot light's cone as Bevy hands it to the shader (bevy_pbr render/light.rs:1366-1367):
/// toward where it points (unit), scale = 1 / max(cos inner - cos outer, 1e-4), offset =
/// -cos outer x scale; the shader takes saturate(cos x scale + offset)^2.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot {
    pub direction: Vec3,
    pub scale: f32,
    pub offset: f32,
}

impl Spot {
    /// From the light's direction and its inner and outer half-angles (radians).
    pub fn new(direction: Vec3, inner: f32, outer: f32) -> Spot {
        let cos_outer = outer.cos();
        let scale = 1.0 / (inner.cos() - cos_outer).max(1e-4);
        Spot { direction: direction.normalize_or_zero(), scale, offset: -cos_outer * scale }
    }

    /// The cone's factor at a point `toward` from the lamp (unit): saturate(cos x scale +
    /// offset)^2 (nvforever lights/display_shader.rs:44-46).
    pub fn factor(&self, toward: Vec3) -> f32 {
        let cone = (self.direction.dot(toward) * self.scale + self.offset).clamp(0.0, 1.0);
        cone * cone
    }
}

/// One placed lamp (Bevy world space): its display colour times its brightness (fade x
/// scale, what it adds to a surface it faces at the lamp), its radius (the light's range),
/// and its cone when it is a spot light.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lamp {
    pub position: Vec3,
    pub colour: Vec3,
    pub radius: f32,
    pub spot: Option<Spot>,
}

impl Lamp {
    /// The display-value light this lamp adds to a surface at `p` facing `n` (unit):
    /// colour x attenuation x N.L x the cone (nvforever lights/display_shader.rs:26-47).
    pub fn light_at(&self, p: Vec3, n: Vec3) -> Vec3 {
        let to = self.position - p;
        let d2 = to.length_squared();
        let att = attenuation(d2, self.radius);
        if att <= 0.0 {
            return Vec3::ZERO;
        }
        let dir = to / d2.max(1e-4).sqrt();
        let n_dot_l = n.dot(dir).clamp(0.0, 1.0);
        let cone = self.spot.map_or(1.0, |s| s.factor(-dir));
        self.colour * (att * n_dot_l * cone)
    }

    /// Whether the lamp's sphere of light reaches the box.
    pub fn reaches(&self, room: &RoomBox) -> bool {
        room.distance_squared(self.position) <= self.radius * self.radius
    }
}

/// The lobe a lamp `to` away from the eye (not normalised) adds toward `d` at level 0.
fn lobe(colour: Vec3, to: Vec3, d: Vec3) -> Vec3 {
    let c = d.dot(to.normalize_or_zero());
    if c <= 0.0 {
        return Vec3::ZERO;
    }
    display_texel(colour) * (LAMP_LOBE_PEAK * c.powf(LAMP_LOBE_POWER))
}

/// The room's box (Bevy world space): centre, axes (the columns, orthonormal) and half
/// extents along them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoomBox {
    pub centre: Vec3,
    pub axes: Mat3,
    pub half: Vec3,
}

/// Which surface a ray leaves the box through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    Floor,
    Wall,
    Ceiling,
}

/// Where a ray from inside leaves the box: the distance, the point, the inward normal and
/// the surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub distance: f32,
    pub point: Vec3,
    pub normal: Vec3,
    pub surface: Surface,
}

impl RoomBox {
    /// An axis-aligned box (Bevy axes) from its corners.
    pub fn from_min_max(min: Vec3, max: Vec3) -> RoomBox {
        RoomBox { centre: (min + max) * 0.5, axes: Mat3::IDENTITY, half: ((max - min) * 0.5).abs() }
    }

    /// The squared distance from `p` to the box (0 inside).
    pub fn distance_squared(&self, p: Vec3) -> f32 {
        let l = self.axes.transpose() * (p - self.centre);
        (l.abs() - self.half).max(Vec3::ZERO).length_squared()
    }

    /// Whether `p` is inside, the box grown by `grow`.
    pub fn contains(&self, p: Vec3, grow: f32) -> bool {
        let l = self.axes.transpose() * (p - self.centre);
        (l.abs() - self.half).max_element() <= grow
    }

    /// The box's full sizes along its axes.
    pub fn size(&self) -> Vec3 {
        self.half * 2.0
    }

    /// Where the ray from `eye` toward `d` (unit) leaves the box. An eye outside the box is
    /// taken at the nearest point just inside it.
    pub fn exit(&self, eye: Vec3, d: Vec3) -> Hit {
        let inner = self.half * (1.0 - 1e-4);
        let lo = (self.axes.transpose() * (eye - self.centre)).clamp(-inner, inner);
        let ld = self.axes.transpose() * d;
        let mut best = (f32::INFINITY, 0usize);
        for k in 0..3 {
            if ld[k].abs() > 1e-8 {
                let t = (self.half[k].copysign(ld[k]) - lo[k]) / ld[k];
                if t < best.0 {
                    best = (t, k);
                }
            }
        }
        let (t, k) = (best.0.max(0.0), best.1);
        let start = self.centre + self.axes * lo;
        let normal = -self.axes.col(k) * ld[k].signum();
        let surface = if normal.y > 0.5 {
            Surface::Floor
        } else if normal.y < -0.5 {
            Surface::Ceiling
        } else {
            Surface::Wall
        };
        Hit { distance: t, point: start + d * t, normal, surface }
    }
}

/// The cell's fog as the cube takes it: its colour as a texel and the fog's near, far and
/// power (nvforever fog.rs `share`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fog {
    pub texel: Vec3,
    pub near: f32,
    pub far: f32,
    pub power: f32,
}

impl Fog {
    /// The share of fog colour at `distance`, as the patched shader computes it (nvforever
    /// fog.rs:126-130; a test in reflection_sky.rs checks the two agree).
    pub fn share(&self, distance: f32) -> f32 {
        let t = ((distance - self.near) / (self.far - self.near).max(1.0)).clamp(0.0, 1.0);
        let power = if self.power > 0.0 { self.power } else { 1.0 };
        if t > 0.0 { t.powf(power) } else { 0.0 }
    }
}

/// The surfaces' albedos as linear colours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Albedo {
    pub floor: Vec3,
    pub wall: Vec3,
    pub ceiling: Vec3,
}

impl Default for Albedo {
    /// [`FLOOR_ALBEDO`], [`WALL_ALBEDO`] and [`CEILING_ALBEDO`] as linear colours.
    fn default() -> Albedo {
        Albedo { floor: albedo_linear(FLOOR_ALBEDO), wall: albedo_linear(WALL_ALBEDO), ceiling: albedo_linear(CEILING_ALBEDO) }
    }
}

impl Albedo {
    pub fn of(&self, s: Surface) -> Vec3 {
        match s {
            Surface::Floor => self.floor,
            Surface::Wall => self.wall,
            Surface::Ceiling => self.ceiling,
        }
    }
}

/// A lit box around the eye (module notes, "The room"). Colours are display values with
/// their shares applied: `ambient` the view's (x the interior ambient share), `directional`
/// the light's colour x its strength, `toward_light` unit.
#[derive(Clone, Debug, PartialEq)]
pub struct RoomScene {
    pub eye: Vec3,
    pub room: RoomBox,
    pub albedo: Albedo,
    pub ambient: Vec3,
    pub directional: Vec3,
    pub toward_light: Vec3,
    pub lamps: Vec<Lamp>,
    pub fog: Option<Fog>,
}

impl RoomScene {
    /// The display-value light on a surface at `p` facing `n`.
    pub fn light_at(&self, p: Vec3, n: Vec3) -> Vec3 {
        let sun = self.directional * (n.dot(self.toward_light).clamp(0.0, 1.0) * DIRECTIONAL_ON_WALLS);
        self.lamps.iter().fold(self.ambient + sun, |sum, l| sum + l.light_at(p, n))
    }

    /// The level-0 texel toward world direction `d` (unit).
    pub fn radiance(&self, d: Vec3) -> Vec3 {
        let hit = self.room.exit(self.eye, d);
        let surface = lit_texel(self.albedo.of(hit.surface), self.light_at(hit.point, hit.normal));
        let fogged = |c: Vec3, distance: f32| match self.fog {
            Some(f) => c.lerp(f.texel, f.share(distance)),
            None => c,
        };
        let mut out = fogged(surface, hit.distance);
        for l in &self.lamps {
            let to = l.position - self.eye;
            let dist = to.length();
            if dist <= 1e-3 {
                continue;
            }
            let wall = self.room.exit(self.eye, to / dist).distance;
            if dist <= wall + LAMP_LOBE_MARGIN {
                let fade = self.fog.map_or(1.0, |f| 1.0 - f.share(dist));
                out += lobe(l.colour, to, d) * fade;
            }
        }
        out
    }
}

/// The lamp-lit ground outdoors (module notes, "The ground outdoors"): level ground at
/// height `ground` (Bevy y) under the eye, of linear albedo `albedo`, already lit by `base`
/// (stage 1's ambient and directional light on level ground, display values).
#[derive(Clone, Debug, PartialEq)]
pub struct GroundLamps {
    pub eye: Vec3,
    pub ground: f32,
    pub albedo: Vec3,
    pub base: Vec3,
    pub lamps: Vec<Lamp>,
}

impl GroundLamps {
    /// What the lamps add toward `d` (unit) at level 0: below the horizon the lit ground
    /// less stage 1's ground, and each lamp's lobe (no walls outdoors).
    pub fn added(&self, d: Vec3) -> Vec3 {
        let mut out = Vec3::ZERO;
        let height = self.eye.y - self.ground;
        if d.y < -1e-4 && height > 0.0 {
            let p = self.eye + d * (height / -d.y);
            let lit = self.lamps.iter().fold(self.base, |sum, l| sum + l.light_at(p, Vec3::Y));
            out += (lit_texel(self.albedo, lit) - lit_texel(self.albedo, self.base)).max(Vec3::ZERO);
        }
        for l in &self.lamps {
            out += lobe(l.colour, l.position - self.eye, d);
        }
        out
    }
}

/// The texel of a level a world direction falls in: (face, row, column), the inverse of
/// `reflection_cube::world_direction` (whose cube direction is the world's with z negated).
pub fn texel_of(d: Vec3, side: u32) -> (usize, u32, u32) {
    let c = Vec3::new(d.x, d.y, -d.z);
    let a = c.abs();
    let (face, s, t) = if a.x >= a.y && a.x >= a.z {
        if c.x > 0.0 { (0, -c.z / a.x, -c.y / a.x) } else { (1, c.z / a.x, -c.y / a.x) }
    } else if a.y >= a.z {
        if c.y > 0.0 { (2, c.x / a.y, c.z / a.y) } else { (3, c.x / a.y, -c.z / a.y) }
    } else if c.z > 0.0 {
        (4, c.x / a.z, -c.y / a.z)
    } else {
        (5, -c.x / a.z, -c.y / a.z)
    };
    let cell = |u: f32| (((u + 1.0) * 0.5 * side as f32).floor().max(0.0) as u32).min(side - 1);
    (face, cell(t), cell(s))
}

/// The centre's (s, t) of a texel.
fn centre(side: u32, row: u32, col: u32) -> (f32, f32) {
    (2.0 * (col as f32 + 0.5) / side as f32 - 1.0, 2.0 * (row as f32 + 0.5) / side as f32 - 1.0)
}

/// The solid angle of a texel, at its centre: its area on the unit cube face over the cube
/// of its distance.
fn solid_angle(side: u32, row: u32, col: u32) -> f32 {
    let (s, t) = centre(side, row, col);
    let area = (2.0 / side as f32).powi(2);
    area / (1.0 + s * s + t * t).powf(1.5)
}

/// Every texel of a level: its world direction and solid angle, in the layout of
/// `reflection_cube::specular_levels` (faces in layer order, rows top down).
fn texels(level: u32) -> Vec<(Vec3, f32)> {
    let side = level_side(level);
    let mut out = Vec::with_capacity(6 * (side * side) as usize);
    for face in 0..6 {
        for row in 0..side {
            for col in 0..side {
                let (s, t) = centre(side, row, col);
                out.push((world_direction(face, s, t), solid_angle(side, row, col)));
            }
        }
    }
    out
}

/// The Blinn-Phong power of the GGX lobe Bevy reads `level` for (module notes, "Levels").
pub fn level_power(level: u32) -> f32 {
    let r = level as f32 / (SPECULAR_LEVELS - 1) as f32;
    let a = (r * r).max(1e-3);
    (2.0 / (a * a) - 2.0).max(0.0)
}

/// The prefilter kernel at cosine `c` for a level of power `p`.
pub fn kernel(c: f32, p: f32) -> f32 {
    if c <= 0.0 {
        return 0.0;
    }
    c * ((1.0 + c) * 0.5).powf(p * 0.5)
}

/// The angle (radians) at which [`kernel`]'s half-angle part falls to half: 90 degrees for
/// a power of 0.
pub fn kernel_half_angle(p: f32) -> f32 {
    if p <= 0.0 {
        return std::f32::consts::FRAC_PI_2;
    }
    (2.0 * 0.5f32.powf(2.0 / p) - 1.0).clamp(-1.0, 1.0).acos()
}

/// A cube's every level, computed on the CPU from level 0 (module notes, "Levels").
#[derive(Clone, Debug)]
pub struct FilteredCube {
    levels: Vec<Vec<Vec3>>,
}

impl FilteredCube {
    /// Level 0 from `level0(world direction)` at each texel's centre, the rest filtered.
    pub fn new(level0: impl Fn(Vec3) -> Vec3) -> FilteredCube {
        let base: Vec<Vec3> = texels(0).iter().map(|(d, _)| sanitise(level0(*d))).collect();
        let mut boxed = vec![base];
        for level in 1..SPECULAR_LEVELS {
            let next = box_down(&boxed[level as usize - 1], level);
            boxed.push(next);
        }
        let levels = (0..SPECULAR_LEVELS)
            .map(|level| {
                let side = level_side(level);
                // At the last level the angles are equal; the cosine lobe still crosses
                // face boundaries and needs convolution.
                if level == 0 || kernel_half_angle(level_power(level)) < std::f32::consts::FRAC_PI_2 / side as f32 {
                    return boxed[level as usize].clone();
                }
                let source = level.min(CONVOLVE_SOURCE_LEVEL);
                convolve(&boxed[source as usize], source, level)
            })
            .collect();
        FilteredCube { levels }
    }

    /// The texel toward world direction `d` at `level` (the texel `d` falls in; the cube is
    /// asked at texel centres, so this is exact there).
    pub fn colour(&self, d: Vec3, level: u32) -> Vec3 {
        let level = level.min(SPECULAR_LEVELS - 1);
        let side = level_side(level);
        let (face, row, col) = texel_of(d, side);
        self.levels[level as usize][((face as u32 * side + row) * side + col) as usize]
    }

    /// A level's mean over the sphere, solid-angle weighted.
    pub fn mean(&self, level: u32) -> Vec3 {
        let t = texels(level);
        let total: f32 = t.iter().map(|(_, w)| w).sum();
        t.iter().zip(&self.levels[level as usize]).map(|((_, w), c)| *c * *w).sum::<Vec3>() / total
    }

    /// A level's means above and below the horizon, solid-angle weighted.
    pub fn hemisphere_means(&self, level: u32) -> (Vec3, Vec3) {
        let (mut up, mut wu, mut down, mut wd) = (Vec3::ZERO, 0.0, Vec3::ZERO, 0.0);
        for ((d, w), c) in texels(level).iter().zip(&self.levels[level as usize]) {
            if d.y >= 0.0 {
                up += *c * *w;
                wu += w;
            } else {
                down += *c * *w;
                wd += w;
            }
        }
        (up / f32::max(wu, 1e-9), down / f32::max(wd, 1e-9))
    }
}

/// A value that is not finite as 0, the rest held at 0 and above (as the upload does).
fn sanitise(c: Vec3) -> Vec3 {
    if c.is_finite() { c.max(Vec3::ZERO) } else { Vec3::ZERO }
}

/// `level` from the level above it: each texel the solid-angle-weighted mean of its four.
fn box_down(above: &[Vec3], level: u32) -> Vec<Vec3> {
    let (side, up) = (level_side(level), level_side(level - 1));
    let mut out = Vec::with_capacity(6 * (side * side) as usize);
    for face in 0..6u32 {
        for row in 0..side {
            for col in 0..side {
                let (mut sum, mut weight) = (Vec3::ZERO, 0.0);
                for (r, c) in [(2 * row, 2 * col), (2 * row, 2 * col + 1), (2 * row + 1, 2 * col), (2 * row + 1, 2 * col + 1)] {
                    let (r, c) = (r.min(up - 1), c.min(up - 1));
                    let w = solid_angle(up, r, c);
                    sum += above[((face * up + r) * up + c) as usize] * w;
                    weight += w;
                }
                out.push(sum / weight);
            }
        }
    }
    out
}

/// Level `level` as the box average of level `source` convolved with `level`'s kernel over
/// the whole sphere.
fn convolve(source_texels: &[Vec3], source: u32, level: u32) -> Vec<Vec3> {
    let from = texels(source);
    let p = level_power(level);
    // The cosine below which the kernel is under the cutoff (relative to its peak of 1).
    let c_min = if p > 0.0 { (2.0 * KERNEL_CUTOFF.powf(2.0 / p) - 1.0).max(0.0) } else { 0.0 };
    texels(level)
        .iter()
        .map(|(n, _)| {
            let (mut sum, mut weight) = (Vec3::ZERO, 0.0);
            for ((s, omega), c) in from.iter().zip(source_texels) {
                let cos = n.dot(*s);
                if cos <= c_min {
                    continue;
                }
                let w = kernel(cos, p) * omega;
                sum += *c * w;
                weight += w;
            }
            if weight > 0.0 { sum / weight } else { Vec3::ZERO }
        })
        .collect()
}

#[cfg(test)]
mod tests;
