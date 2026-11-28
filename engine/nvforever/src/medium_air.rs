//! The volumetric medium's light in the air (LootJunkieAzza/NVForever#3032, stage 3, the near
//! grid): the CPU half, pure functions only. An addition of NVForever's own for the enhanced
//! look; not a statement about the original's frames, and never used to explain a difference
//! from them. Off unless the settings row Volumetric light (experimental) (`volumetric-light`)
//! or the aid `NVF_PLAY_DEBUG_MEDIUM_LIGHT` asks for it (medium_air_demand.rs) [design].
//!
//! **What it is** [design]: a froxel grid (a frustum-aligned voxel grid) near the camera
//! [documented: Bauer 2019, "Creating the Atmospheric World of Red Dead Redemption 2", SIGGRAPH
//! 2019 Advances in Real-Time Rendering, slide 42; Wronski 2014, "Volumetric Fog", SIGGRAPH
//! 2014 Advances], marched front to back per column, slice by slice [documented: Bauer 2019,
//! slide 47]. The medium in stage 3 is the weather's own fog: the light re-shades the fog's
//! light in the fog's own blending space. Along a view ray the fog gives a surface at distance
//! d the share F(d) (fog.rs `GAME_FOG`); a slice's weight is dF, the difference of F across the
//! slice, so the weights telescope to what a surface at that depth was given.
//!
//! **What a froxel stores and how the composite uses it** [design]: per slice the sums to its far boundary, N = sum of dF x V and M = sum of dF
//! ([`column_nm`]; V the sun's visibility in 0..1). No phase is stored: a sum of neighbouring
//! columns' phases, interpolated and then divided by the pixel's own phase, made a shadow out
//! of a fully lit grid (0.655 at the centre of a 16 x 8 grid with g 0.8). The composite reads
//! the sums at the pixel's slice coordinate with a zero boundary at the camera ([`sums_at`]),
//! forms the lit fraction Vbar = N / M ([`lit_fraction`], 0..=1) and its relative form Vbar_c
//! ([`relative_lit`]), applies the pixel's own phase once (p_eff, [`phase_floor`]), takes the
//! fog share F_near analytically at the pixel's distance held to the grid's end
//! ([`near_share`]) and applies **one continuous form** in display values for every sample
//! ([`far_blend`], [`sample_blend`], [`blended_correction`], [`compose_blended`]):
//!
//! `Y'_new = Y' + corr_t x B_t`, `B_t = mix(C', Y', t)`,
//! `corr_t = max(k a F_near (Vbar_c x mix(p_eff, mix(1, p_eff, SKYPHASE), t) - 1) x mix(1, SKY, t), -min(F_near, 1))`,
//!
//! t = 0 for near world samples (the additive world rule, the fog colour C' as the budget), 1
//! for cleared depth (the sky's multiplicative rule: the pixel's own display value Y' as the
//! budget), a smoothstep between 0.6 x sky.rs `SKY_DEPTH_FROM` and it for far terrain
//! ([`far_blend`]), joined with a smoothstep of F_near from `AIR_FOG_BLEND_FROM` to 1
//! ([`fog_blend`]: a surface the fog covers takes the rule of the sky it stands against), so no
//! distance and no fog has a step; a = clamp(s, 0, 1), the sunlit share alone ([`effective_share`], s
//! from [`scaled_share`]); k = STRENGTH, the one overall scale of the correction, 0..=2 (the
//! settings row's slider, 0 to 200 %, or the aid; [`applied_scale`] packs k x a), applied
//! before the floor, so k 0 changes nothing and the floor holds at any k; on the GPU that scale is
//! times the deck's share D of the clouds' own mean transmittance ([`deck_share`]: under a closed deck
//! the fog's light is mostly the sky's, whatever the lights' share says); a gain is held off pixels that
//! are already bright by the sky's share t of it ([`hold_bright`]); the result held to
//! 0..=[`AIR_HDR_MAX`] in linear light. For a fogged
//! opaque fragment `Y' = (1 - f) S' + f C'` the result is never below `(1 - f) S'` per
//! channel near (shadow loses at most the budget share F_near of B_t).
//!
//! **What [`scaled_share`]'s caller passes** (at this base, sky.rs `update_sky`, the
//! outdoor branch, read after it ran this frame) [verified against the code]:
//! - `direct_luma`: [`display_luma`] of the `Sun` entity's `DirectionalLight::color` (its RGB
//!   holds display values: sky.rs:2228-2231 sets it to `lights::display_color(
//!   sky.color(ColorType::Sunlight).map(lights::linear_to_display))`, lights.rs:38/44).
//! - `direct_scale`: that light's `illuminance / (PI x sky::light_unit())` (sky.rs:201).
//!   `illuminance` is set every frame in `aim_sun` (sky.rs:2288) from
//!   `full = PI x unit x sky.sun_scale() x sun_mod` (sky.rs:2176): the image space's Sunlight
//!   Dimmer (`SkyState::sun_scale`, sky.rs:1006) and the screen modifiers' dimmer ratio
//!   (`sun_mod`, sky.rs:2117-2118) are in it, times the horizon fade
//!   `min(|sun_dir.y| / sin(HORIZON_FADE), 1)` (`HORIZON_FADE` 0.1 rad, sky.rs:95) and the
//!   day/night share `SUN_SHARE` 0.75 by day or `MOON_SHARE` 0.5 for the opposite-direction
//!   night light (sky.rs:192-193). So the quotient is `sun_scale x sun_mod x fade x share`.
//! - `ambient_luma`: [`display_luma`] of `GlobalAmbientLight::color`'s RGB (display values,
//!   the lightning flash added: sky.rs:2232-2233; its alpha -1 is the display-sum marker,
//!   lights.rs:51, not a brightness).
//! - `ambient_scale`: `GlobalAmbientLight::brightness / light_unit()`, i.e.
//!   `AMBIENT_SHARE` 0.9 outdoors (sky.rs:191, :2234).
//!
//! Two things besides: the
//! colours are rewritten only when `SkyState::changed` (the sky's half-minute steps), the
//! illuminance every frame; and by night the "sun" light is the weather's night Sunlight
//! colour from the opposite direction (sky.rs:2282): the moon's light, drawn where the climate has a
//! moon ([`air_source`]) with the moon's constants ([`MoonTune`]).
//!
//! The phase is two Henyey-Greenstein lobes [documented: Henyey and Greenstein 1941, "Diffuse
//! radiation in the galaxy", Astrophysical Journal 93; Bauer 2019, slides 29-31 for mixing
//! lobes], capped and normalised here so that the **final** function has sphere mean 1 and
//! its largest value is the cap. Everything is written from those publications'
//! descriptions; no code from any project.
//!
//! The GPU half (`medium_air_grid.wesl`, `medium_air_compose.wesl`) mirrors
//! [`AirCurve::depth_at`], [`AirCurve::coord`], [`fog_share`], [`NormedPhase::phase`],
//! [`column_nm`], [`sums_at`], [`lit_fraction`], [`relative_lit`], [`deck_transmittance`],
//! [`deck_share`], [`phase_floor`],
//! [`near_share`], [`far_blend`], [`sample_blend`], [`blended_correction`], [`bright_hold`],
//! [`hold_bright`], [`compose_blended`] and [`slices_agree`] (the far form is the sky's modulation: the ray's lit fraction modulates
//! the sky's own light [documented: Bauer 2019, slide 62]); [`pack`] fills the uniform's
//! groups. The mirrors only tests call are `#[cfg(test)]`; the composite's pixel as a whole is
//! mirrored in medium_air_compose.rs (`pixel_mirror`).

#[cfg(test)]
use bevy::math::Mat4;
use bevy::math::{Vec2, Vec3};

// ---------------------------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------------------------

/// The grid's columns and rows at 16:9 and its slices [documented: Bauer 2019, slide 42,
/// "Resolution of 160x88x64"].
pub const AIR_GRID: [u32; 3] = [160, 88, 64];
/// The grid's depth by default, in game units (view depth) [design: judged by eye
/// from stills, 2026-11-20: the cloud plane is about
/// 29,000 units up and a 12,000 grid (the cascades' reach with Longer shadows) cannot reach its
/// shafts; the aid's `DEPTH` still takes it].
pub const AIR_DEPTH: f32 = 48_000.0;
/// The first slice's thickness in game units: the exponential curve's start [design].
pub const AIR_FIRST: f32 = 50.0;
/// Below this total of the scaled direct and ambient lumas [`scaled_share`] falls with the
/// direct light instead of holding d / (d + a): with a black ambient the share would
/// otherwise stay 1 while the sun fades out at the horizon. A dim day's total
/// is about 0.5 (0.75 x luma 0.4 + 0.9 x luma 0.25), so this acts only near black [design].
const SHARE_FLOOR: f32 = 1e-3;
/// [`lit_fraction`]'s M at or below which the lit fraction is 1 (no fog weight yet: no
/// correction) [design]. medium_air_compose.wesl declares the same constant (tested).
#[cfg(test)]
pub const RATIO_EMPTY: f32 = 1e-9;
/// The display transfer of the fog blend: x' = max(x, 0)^(1/2.2) [verified: fog.rs
/// `GAME_FOG`, `bi.x > 0.5`]. medium_air_compose.wesl declares the same constant (tested).
#[cfg(test)]
pub const DISPLAY_GAMMA: f32 = 2.2;
/// The far blend ([`far_blend`]) starts at this share of the sky's distance [design]. Mirrored in the composite.
#[cfg(test)]
pub const SKY_TAPER_FROM: f32 = 0.6;
/// The fog blend ([`fog_blend`]) starts at this fog share F_near and is whole at a share of 1.
/// The additive world rule adds F_near of the fog colour's own gain whatever the surface, so a
/// silhouette the fog nearly covered outshone the sky behind it, whose rule is weighted by SKY; a
/// start near 1 would only move that band nearer the camera (at F_near 0.8 the rule already adds
/// 0.8 of the gain) [design, 2026-11-24: a start value, to be set by stills]. Mirrored in the composite.
#[cfg(test)]
pub const AIR_FOG_BLEND_FROM: f32 = 0.35;
#[cfg(test)]
const _: () = assert!(AIR_FOG_BLEND_FROM > 0.0 && AIR_FOG_BLEND_FROM < 1.0);
/// The clouds' mean transmittance ([`deck_transmittance`]) at and below which the deck's share
/// ([`deck_share`]) is `AIR_DECK_LEAST`, and at and above which it is 1 [design, 2026-11-24].
/// Mirrored in the composite.
#[cfg(test)]
pub const AIR_DECK_FROM: f32 = 0.40;
#[cfg(test)]
pub const AIR_DECK_TO: f32 = 0.85;
/// The deck's share under a closed deck: a quarter is left for its breaks, where the light was gone
/// with none [design, 2026-11-24]. Mirrored in the composite.
#[cfg(test)]
pub const AIR_DECK_LEAST: f32 = 0.25;
/// The least light height (the sine of its elevation) [`deck_transmittance`] takes [design,
/// 2026-11-24]. Mirrored in the grid shader's mean pass.
#[cfg(test)]
pub const AIR_DECK_UP: f32 = 0.1;
/// The least transmittance it raises to a power: at the least height it is just under
/// `AIR_DECK_FROM`, so an opaque deck is one at every height [design, 2026-11-24]. Mirrored in the grid
/// shader's mean pass.
#[cfg(test)]
pub const AIR_DECK_THIN: f32 = 1e-4;
#[cfg(test)]
const _: () = assert!(AIR_DECK_FROM < AIR_DECK_TO && AIR_DECK_TO <= 1.0);
/// The bright hold ([`bright_hold`]): the display value (before tonemapping) up to which a gain may
/// lift a pixel's largest channel untouched, and the one no gain lifts the sky past. Under white by a
/// margin, since the picture's grade comes after this pass and brightens: by about 1.2 in an evening
/// scene, where white was reached at about 0.82 and an end of 0.95 left clouds clipping. How much
/// depends on the scene; the two are a fixed heuristic [design, 2026-11-24: set by stills;
/// docs/design/volumetric-medium.md, "The composite" 8]. Mirrored in the composite.
#[cfg(test)]
pub const AIR_BRIGHT_FROM: f32 = 0.50;
#[cfg(test)]
pub const AIR_BRIGHT_TO: f32 = 0.76;
#[cfg(test)]
const _: () = assert!(AIR_BRIGHT_FROM < AIR_BRIGHT_TO);
/// An excess over [`AIR_BRIGHT_FROM`] at or below this is left alone (far under a display level; no
/// division of nothings). Mirrored in the composite.
#[cfg(test)]
pub const AIR_BRIGHT_LEAST: f32 = 1e-6;
/// The composite's sample agreement: a pixel's depth samples share one volume tap when their
/// view depths, held to the grid's end, lie within this share of a slice of each other
/// ([`slices_agree`]); each sample still takes its own fog share and blend [design]. Mirrored in the composite.
#[cfg(test)]
pub const AIR_AGREE_SLICES: f32 = 0.25;
/// The largest linear value the composite writes, per channel, on every path: inside
/// `Rgba16Float`'s largest finite value 65,504 [documented: IEEE 754 binary16, the largest
/// finite value (2 - 2^-10) x 2^15], so bloom and exposure never meet an infinity [design]. Mirrored in the composite.
pub const AIR_HDR_MAX: f32 = 60_000.0;
const _: () = assert!(AIR_HDR_MAX < 65_504.0);
/// The grid's visibility samples per froxel by default (the tune key `SAMPLES`; the grid pass
/// reads it from the uniform's misc.y) [design].
pub const AIR_SAMPLES_DEFAULT: u32 = 4;
/// How often the tune file is looked at, real seconds [design].
#[cfg(any(test, debug_assertions, feature = "dev-aids"))]
pub const TUNE_POLL_SECONDS: f64 = 0.5;
/// Columns held to this range when they follow the aspect [design].
const AIR_COLUMNS: (u32, u32) = (64, 320);
/// Slices held to this range [design]. The grid pass keeps at most a quarter of the most per
/// invocation (medium_air_grid.rs `GridWalk`, checked against it there).
pub(crate) const AIR_SLICES: (u32, u32) = (8, 128);
/// An asked grid (the aid) is held to these: columns, rows, slices [design].
const AIR_ASKED_COLUMNS: (u32, u32) = (16, 320);
const AIR_ASKED_ROWS: (u32, u32) = (8, 176);
/// The grid's depth held to this range [design].
const AIR_DEPTHS: (f32, f32) = (1_000.0, 200_000.0);
/// A curve whose ratio is within this of 1 is drawn linear (thickness depth / nz, at least the
/// first slice's): `(r^k - 1) / (r - 1)` loses precision in f32 as r nears 1 [design].
const AIR_LINEAR_BELOW: f64 = 1e-3;
/// Midpoint steps over cos theta for the phase's mean [design: at least 2,048 asked].
const PHASE_STEPS: usize = 8_192;
/// The strength's range, the aid's, the tune file's and the settings row's slider (0 to
/// 200 %): the one overall scale k of the correction ([`applied_scale`]) [design].
const AIR_STRENGTHS: (f32, f32) = (0.0, 2.0);

/// The start aids and the tune file's variable (read by the GPU half at start through dev_aids.rs:
/// not in release builds).
pub const ENV_STRENGTH: &str = "NVF_PLAY_DEBUG_MEDIUM_LIGHT";
pub const ENV_VIEW: &str = "NVF_PLAY_DEBUG_MEDIUM_LIGHT_VIEW";
pub const ENV_GRID: &str = "NVF_PLAY_DEBUG_MEDIUM_LIGHT_GRID";
pub const ENV_DEPTH: &str = "NVF_PLAY_DEBUG_MEDIUM_LIGHT_DEPTH";
pub const ENV_OCCLUDERS: &str = "NVF_PLAY_DEBUG_MEDIUM_LIGHT_OCCLUDERS";
pub const ENV_WALK: &str = "NVF_PLAY_DEBUG_MEDIUM_LIGHT_WALK";
#[cfg(any(test, debug_assertions, feature = "dev-aids"))]
pub const ENV_TUNE: &str = "NVF_PLAY_DEBUG_MEDIUM_LIGHT_TUNE";

// ---------------------------------------------------------------------------------------------
// The grid and its slice curve
// ---------------------------------------------------------------------------------------------

/// The froxel grid's layout: columns, rows, slices, the depth it reaches (view depth, along
/// the camera's forward axis) and the first slice's thickness [design].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AirGrid {
    pub nx: u32,
    pub ny: u32,
    pub nz: u32,
    pub depth: f32,
    pub first: f32,
}

impl AirGrid {
    /// The grid for a view's aspect (width / height). Default 160 x 88 x 64 at 16:9: rows stay
    /// 88, columns follow the aspect (rounded to a multiple of 8, held to 64..=320). An asked
    /// grid (`grid`, the aid) is held to 16..=320 x 8..=176 x 8..=128; slices always to
    /// 8..=128. Depth [`AIR_DEPTH`] unless asked, held to 1,000..=200,000 [design].
    pub fn for_view(aspect: f32, grid: Option<[u32; 3]>, depth: Option<f32>) -> AirGrid {
        let aspect = if aspect.is_finite() && aspect > 0.0 { aspect } else { 16.0 / 9.0 };
        let [nx, ny, nz] = match grid {
            Some([x, y, z]) => [x.clamp(AIR_ASKED_COLUMNS.0, AIR_ASKED_COLUMNS.1), y.clamp(AIR_ASKED_ROWS.0, AIR_ASKED_ROWS.1), z],
            None => {
                let rows = AIR_GRID[1];
                let columns = ((rows as f32 * aspect / 8.0).round().min(1_000.0) as u32 * 8).clamp(AIR_COLUMNS.0, AIR_COLUMNS.1);
                [columns, rows, AIR_GRID[2]]
            }
        };
        let depth = depth.filter(|d| d.is_finite()).unwrap_or(AIR_DEPTH).clamp(AIR_DEPTHS.0, AIR_DEPTHS.1);
        AirGrid { nx, ny, nz: nz.clamp(AIR_SLICES.0, AIR_SLICES.1), depth, first: AIR_FIRST }
    }

    /// The grid's texel count.
    pub fn texels(&self) -> u64 {
        self.nx as u64 * self.ny as u64 * self.nz as u64
    }

    /// The grid's size in bytes at `bytes_per_texel` (8 for `Rgba16Float`).
    pub fn bytes(&self, bytes_per_texel: u64) -> u64 {
        self.texels() * bytes_per_texel
    }

    /// The slice curve as the GPU takes it (see [`AirCurve`]).
    pub fn curve(&self) -> AirCurve {
        let r = self.ratio();
        let linear = r <= 1.0;
        AirCurve {
            first: if linear { self.depth / self.nz.max(1) as f32 } else { self.first },
            r: r as f32,
            ln_r: if linear { 0.0 } else { r.ln() as f32 },
            nz: self.nz.max(1) as f32,
            depth: self.depth,
        }
    }

    /// The slices' growth ratio r: slice k spans [z(k), z(k+1)] with
    /// `z(k) = first x (r^k - 1) / (r - 1)` and r > 1 solved by bisection (to 1e-7 relative) so
    /// that z(nz) = depth. 1 (linear, thickness depth / nz) when `depth <= first x nz`, and
    /// when the solved r is within [`AIR_LINEAR_BELOW`] of 1 [design: exponential slices with
    /// a linear start, as froxel grids use them, Wronski 2014].
    pub fn ratio(&self) -> f64 {
        let n = self.nz.max(1) as i32;
        let first = self.first.max(1e-3) as f64;
        let depth = self.depth as f64;
        if n < 2 || !depth.is_finite() || depth <= first * n as f64 {
            return 1.0;
        }
        let reach = |r: f64| first * (r.powi(n) - 1.0) / (r - 1.0);
        let mut lo = 1.0f64;
        // z(nz) >= first x r^(nz - 1), so this r reaches the depth.
        let mut hi = (depth / first).powf(1.0 / (n - 1) as f64).max(1.0 + 1e-9);
        while reach(hi) < depth {
            hi *= 2.0;
        }
        for _ in 0..200 {
            let mid = 0.5 * (lo + hi);
            if reach(mid) < depth {
                lo = mid;
            } else {
                hi = mid;
            }
            if hi - lo <= 1e-7 * hi {
                break;
            }
        }
        let r = 0.5 * (lo + hi);
        if r - 1.0 < AIR_LINEAR_BELOW { 1.0 } else { r }
    }

    /// The view depth where slice k starts (z(k); k = nz gives the depth).
    #[cfg(test)]
    pub fn slice_near(&self, k: u32) -> f32 {
        self.curve().depth_at(k as f32)
    }

    /// The view depth of slice k's middle in the slice coordinate (z(k + 0.5)), so that
    /// `slice_coord(slice_mid(k)) = k + 0.5`, the texel centre a trilinear tap reads.
    #[cfg(test)]
    pub fn slice_mid(&self, k: u32) -> f32 {
        self.curve().depth_at(k as f32 + 0.5)
    }

    /// The continuous slice coordinate of a view depth: 0 at 0, nz at the depth, clamped.
    #[cfg(test)]
    pub fn slice_coord(&self, z: f32) -> f32 {
        self.curve().coord(z)
    }
}

/// The slice curve in the uniform's form (first, r, ln r, nz, depth): the one place its
/// shape is written. Linear when `ln_r` is 0 (then `first` is the thickness depth / nz).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AirCurve {
    pub first: f32,
    pub r: f32,
    pub ln_r: f32,
    pub nz: f32,
    pub depth: f32,
}

impl AirCurve {
    /// z(c) for a continuous slice coordinate c in 0..=nz:
    /// linear `depth x c / nz`; else `first x (exp(c x ln r) - 1) / (r - 1)`. Clamped to the
    /// grid. (Computed in f64 here; the GPU's f32 agrees to its precision while r - 1 >= 1e-3.)
    #[cfg(test)]
    pub fn depth_at(&self, c: f32) -> f32 {
        let c = c.clamp(0.0, self.nz) as f64;
        let z = if self.ln_r <= 0.0 {
            self.depth as f64 * c / self.nz as f64
        } else {
            self.first as f64 * ((c * self.ln_r as f64).exp() - 1.0) / (self.r as f64 - 1.0)
        };
        (z as f32).clamp(0.0, self.depth)
    }

    /// The slice's thickness at view depth z: dz/dc = (first / (r - 1) + z) x ln r (from
    /// z(c) = first (r^c - 1) / (r - 1)), or depth / nz when linear.
    #[cfg(test)]
    pub fn slice_width(&self, z: f32) -> f32 {
        if self.ln_r <= 0.0 {
            return self.depth / self.nz.max(1.0);
        }
        (self.first / (self.r - 1.0) + z) * self.ln_r
    }

    /// The inverse: the continuous slice coordinate of view depth z, clamped to 0..=nz:
    /// linear `z / depth x nz`; else `ln(1 + z x (r - 1) / first) / ln r`.
    #[cfg(test)]
    pub fn coord(&self, z: f32) -> f32 {
        if !z.is_finite() {
            return if z > 0.0 { self.nz } else { 0.0 };
        }
        // At and beyond the grid's end the coordinate is nz exactly (the rounded ratio would
        // give 63.99997 for 64 slices).
        if z >= self.depth {
            return self.nz;
        }
        let z = z.clamp(0.0, self.depth) as f64;
        let c = if self.ln_r <= 0.0 {
            z / self.depth as f64 * self.nz as f64
        } else {
            (1.0 + z * (self.r as f64 - 1.0) / self.first as f64).ln() / self.ln_r as f64
        };
        (c as f32).clamp(0.0, self.nz)
    }
}

// ---------------------------------------------------------------------------------------------
// Froxel geometry
// ---------------------------------------------------------------------------------------------

/// The NDC xy of continuous column coordinates (x across 0..nx, y down 0..ny; column i's
/// centre at i + 0.5). NDC y is up, so row 0 is the top.
#[cfg(test)]
pub fn froxel_ndc(grid: &AirGrid, x: f32, y: f32) -> Vec2 {
    Vec2::new(x / grid.nx as f32 * 2.0 - 1.0, 1.0 - y / grid.ny as f32 * 2.0)
}

/// The NDC xy of column (i, j)'s centre.
#[cfg(test)]
pub fn column_ndc(grid: &AirGrid, i: u32, j: u32) -> Vec2 {
    froxel_ndc(grid, i as f32 + 0.5, j as f32 + 0.5)
}

/// The view-space ray (-z forward) through an NDC point per unit of view depth, for a
/// centred perspective with x and y scales `proj_x` (`clip_from_view.x_axis.x`) and `proj_y`
/// (`.y_axis.y`): (ndc.x / proj_x, ndc.y / proj_y, -1). Unjittered.
pub fn view_ray(ndc: Vec2, proj_x: f32, proj_y: f32) -> Vec3 {
    Vec3::new(ndc.x / proj_x, ndc.y / proj_y, -1.0)
}

/// The ray's length per unit of view depth: distance from the camera = view depth x this.
pub fn ray_len(ndc: Vec2, proj_x: f32, proj_y: f32) -> f32 {
    view_ray(ndc, proj_x, proj_y).length()
}

/// A froxel's world position from continuous froxel coordinates (x, y as [`froxel_ndc`], z the
/// slice coordinate).
#[cfg(test)]
pub fn froxel_to_world(grid: &AirGrid, coord: Vec3, proj_x: f32, proj_y: f32, world_from_view: Mat4) -> Vec3 {
    let ndc = froxel_ndc(grid, coord.x, coord.y);
    let t = grid.curve().depth_at(coord.z);
    world_from_view.transform_point3(view_ray(ndc, proj_x, proj_y) * t)
}

/// The reverse: a world point's continuous froxel coordinates (slice coordinate clamped to
/// 0..=nz); `None` behind the camera (view depth not above 0).
#[cfg(test)]
pub fn world_to_froxel(grid: &AirGrid, world: Vec3, proj_x: f32, proj_y: f32, world_from_view: Mat4) -> Option<Vec3> {
    let view = world_from_view.inverse().transform_point3(world);
    let t = -view.z;
    if t.is_nan() || t <= 0.0 {
        return None;
    }
    let ndc = Vec2::new(view.x * proj_x / t, view.y * proj_y / t);
    let x = (ndc.x + 1.0) * 0.5 * grid.nx as f32;
    let y = (1.0 - ndc.y) * 0.5 * grid.ny as f32;
    Some(Vec3::new(x, y, grid.curve().coord(t)))
}

// ---------------------------------------------------------------------------------------------
// The fog's share
// ---------------------------------------------------------------------------------------------

/// The weather's fog as the camera's `DistanceFog` carries it: near, far, power (fog.rs
/// `falloff`) and strength (the fog colour's alpha) [verified: fog.rs `GAME_FOG`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AirFog {
    pub near: f32,
    pub far: f32,
    pub power: f32,
    pub strength: f32,
}

impl AirFog {
    /// Whether the light can make sense of this fog: false for any non-finite field,
    /// `far <= near`, `power <= 0` or `strength <= 0`. Such a fog is shown the plain way: no
    /// light is drawn [design].
    pub fn usable(&self) -> bool {
        let finite = self.near.is_finite() && self.far.is_finite() && self.power.is_finite() && self.strength.is_finite();
        finite && self.far > self.near && self.power > 0.0 && self.strength > 0.0
    }
}

/// The share of fog a surface at `distance` (from the camera, not view depth) is given, the
/// exact mirror of fog.rs `GAME_FOG`'s `fog_share`: `x = clamp((d - near) / max(far - near,
/// 1), 0, 1)`; `x^power x strength` for x > 0, else 0. A non-finite result is 0 (such a fog is
/// not [`AirFog::usable`] anyway).
pub fn fog_share(distance: f32, fog: AirFog) -> f32 {
    let x = ((distance - fog.near) / (fog.far - fog.near).max(1.0)).clamp(0.0, 1.0);
    let share = if x > 0.0 { x.powf(fog.power) * fog.strength } else { 0.0 };
    if share.is_finite() { share } else { 0.0 }
}

// ---------------------------------------------------------------------------------------------
// The phase
// ---------------------------------------------------------------------------------------------

/// The phase: a forward and a back Henyey-Greenstein lobe mixed by `back_weight`, each scaled
/// to mean 1 over the sphere, capped and normalised so that the **final** function has
/// sphere mean 1 and its largest value is `cap` [documented: Henyey and Greenstein 1941
/// for the lobe; Bauer 2019, slides 29-31, for mixed lobes; design: the cap, since the sky
/// dome and Light glow already draw the sun's aureole, and a capped peak bounds the glare].
///
/// `cap` is the final cap. [`AirPhase::normed`] solves the inner cap c (applied to the mix
/// before normalising) for it and returns it as the [`NormedPhase`]'s `inner_cap` (its
/// `phase` stays as asked), which [`pack`] writes as the uniform's cap, so the GPU's form
/// `min(mix, c) x norm` is unchanged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AirPhase {
    pub g: f32,
    pub back_g: f32,
    pub back_weight: f32,
    pub cap: f32,
}

impl Default for AirPhase {
    /// The first look's values: forward g 0.3, no back lobe (weight 0; its shape -0.3 kept for
    /// when a weight is given), final cap 1.5. With these the uncapped lobe's peak is
    /// (1 + g) / (1 - g)^2 = 1.3 / 0.49 = 2.653 at cos 1 and its least (1 - g) / (1 + g)^2 =
    /// 0.414 at cos -1; the inner cap solves to about 1.2953 with norm 1.1580 (closed form in
    /// the test `phase_has_mean_one_over_the_sphere`).
    fn default() -> Self {
        AirPhase { g: 0.3, back_g: -0.3, back_weight: 0.0, cap: 1.5 }
    }
}

/// One Henyey-Greenstein lobe scaled to mean 1 over the sphere (4 pi x the usual):
/// `(1 - g^2) / (1 + g^2 - 2 g cos)^1.5`. g is held to +-0.99.
fn lobe(g: f64, cos: f64) -> f64 {
    let g = g.clamp(-0.99, 0.99);
    (1.0 - g * g) / (1.0 + g * g - 2.0 * g * cos).powf(1.5)
}

impl AirPhase {
    /// The uncapped mix `(1 - w) x HG(g) + w x HG(back_g)`, HG scaled to mean 1.
    fn mix64(&self, cos: f64) -> f64 {
        let cos = cos.clamp(-1.0, 1.0);
        let w = (self.back_weight as f64).clamp(0.0, 1.0);
        (1.0 - w) * lobe(self.g as f64, cos) + w * lobe(self.back_g as f64, cos)
    }

    /// `min(mix, cap)` with this phase's `cap` applied to the mix as it stands; with the
    /// [`NormedPhase`]'s `inner_cap` in its place this is the GPU's capped mix before the norm
    /// ([`NormedPhase::phase`]). `cos_theta` between the view ray and the direction toward
    /// the light.
    pub fn phase_raw(&self, cos_theta: f32) -> f32 {
        self.mix64(cos_theta as f64).min(self.cap as f64) as f32
    }

    /// The norm of the final function ([`AirPhase::normed`]).
    #[cfg(test)]
    pub fn norm(&self) -> f32 {
        self.normed().norm
    }

    /// The phase with its inner cap and norm solved, for the uniform. The mean over
    /// solid angle is half the integral over cos in [-1, 1], taken by the midpoint rule
    /// ([`PHASE_STEPS`] steps); the norm is 1 / that mean of `min(mix, c)`, so the final
    /// function `min(mix, c) x norm` has sphere mean 1 (exactly, under the same rule) and its
    /// largest value is `c x norm`. The inner cap c is solved so that `c x norm = cap`:
    /// `h(c) = c / mean(min(mix, c))` does not fall with c (its derivative is
    /// `(mean - c x P(mix > c)) / mean^2` and `mean >= c x P(mix > c)`), so bisection finds it
    /// between the mix's least value (h = 1) and its peak (h = peak / mean(mix)).
    /// - A cap at or above the uncapped peak's share (or NaN) caps nothing: c = max(cap, peak).
    /// - A cap at or below 1 gives the constant 1 (a mean of 1 allows no lower peak).
    /// - A mix that is not positive and finite everywhere returns this phase with norm 1.
    pub fn normed(&self) -> NormedPhase {
        let h = 2.0 / PHASE_STEPS as f64;
        let mix: Vec<f64> = (0..PHASE_STEPS).map(|i| self.mix64(-1.0 + (i as f64 + 0.5) * h)).collect();
        let ends = [self.mix64(-1.0), self.mix64(1.0)];
        if !mix.iter().chain(ends.iter()).all(|v| v.is_finite() && *v > 0.0) {
            return NormedPhase { phase: *self, norm: 1.0, inner_cap: self.cap };
        }
        let mean_under = |c: f64| mix.iter().map(|v| v.min(c)).sum::<f64>() / PHASE_STEPS as f64;
        // Both lobes are convex in cos, so the mix's peak is at an end; its least value may be
        // inside, so it is taken over the samples and the ends.
        let peak = ends[0].max(ends[1]);
        let least = mix.iter().copied().fold(ends[0].min(ends[1]), f64::min);
        let want = self.cap as f64;
        let inner = if want.is_nan() || peak / mean_under(peak) <= want {
            self.cap.max(peak as f32)
        } else if want <= 1.0 {
            least as f32
        } else {
            let (mut lo, mut hi) = (least, peak);
            for _ in 0..200 {
                let mid = 0.5 * (lo + hi);
                if mid / mean_under(mid) < want {
                    lo = mid;
                } else {
                    hi = mid;
                }
                if hi - lo <= 1e-12 * hi {
                    break;
                }
            }
            (0.5 * (lo + hi)) as f32
        };
        // The norm from the f32 cap the GPU will use, so the mean stays 1 for it.
        let mean = mean_under(inner as f64);
        let norm = if mean.is_finite() && mean > 0.0 { (1.0 / mean) as f32 } else { 1.0 };
        NormedPhase { phase: *self, norm, inner_cap: inner }
    }

    /// The normalised phase: `normed().phase(cos)` (sphere mean 1, largest value the cap).
    #[cfg(test)]
    pub fn phase(&self, cos_theta: f32) -> f32 {
        self.normed().phase(cos_theta)
    }

    /// The largest value of [`AirPhase::phase`]: the cap when it bites, else the uncapped
    /// peak (both lobes are convex in cos, so the largest value is at an end).
    #[cfg(test)]
    pub fn most(&self) -> f32 {
        self.normed().most()
    }
}

/// An [`AirPhase`] (as asked: `phase.cap` is the final cap) with its solved inner cap and
/// norm computed once (see [`AirPhase::normed`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NormedPhase {
    pub phase: AirPhase,
    pub norm: f32,
    /// The cap applied to the mix before the norm (what [`pack`] writes as the uniform's cap).
    pub inner_cap: f32,
}

impl NormedPhase {
    /// The normalised phase: `min(mix, inner_cap) x norm`, i.e. [`AirPhase::phase_raw`] with
    /// the inner cap, times the norm (the GPU's form, with [`pack`]'s cap).
    pub fn phase(&self, cos_theta: f32) -> f32 {
        AirPhase { cap: self.inner_cap, ..self.phase }.phase_raw(cos_theta) * self.norm
    }

    /// The largest value of [`NormedPhase::phase`] (the larger end of cos in [-1, 1]).
    pub fn most(&self) -> f32 {
        self.phase(1.0).max(self.phase(-1.0))
    }
}

// ---------------------------------------------------------------------------------------------
// The stored sums and the lit fraction
// ---------------------------------------------------------------------------------------------

/// The stored sums of one column (phase-free): per
/// slice k the cumulative `[N, M]` at its far boundary k + 1, `N += dF_k x V_k`, `M += dF_k`.
/// dF_k is the fog share's step over the slice along the column's ray (`ray_len`, distance per
/// unit of view depth), slice 0 starting from F = 0, not F(0) (a fog with near below 0 gives
/// slice 0 the camera's own share, so M telescopes to F at the boundary, what a surface there
/// was given), never negative (taken against the largest share so far; for any usable fog
/// [`fog_share`] does not fall with distance, so that is the plain difference). V is held to
/// 0..=1 (NaN 0). Accumulated in f32 as the GPU does. The CPU mirror of the grid pass's loop
/// (medium_air_grid.wesl) [design].
#[cfg(test)]
pub fn column_nm(grid: &AirGrid, ray_len: f32, fog: AirFog, visibility: &dyn Fn(usize) -> f32) -> Vec<[f32; 2]> {
    let curve = grid.curve();
    let (mut n, mut m, mut before) = (0.0f32, 0.0f32, 0.0f32);
    (0..grid.nz)
        .map(|k| {
            let after = fog_share(curve.depth_at(k as f32 + 1.0) * ray_len, fog);
            let df = (after - before).max(0.0);
            before = before.max(after);
            let v = visibility(k as usize);
            let v = if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) };
            n += df * v;
            m += df;
            [n, m]
        })
        .collect()
}

/// The stored sums at a continuous slice boundary coordinate `c` in 0..=nz (the 1D
/// reference of the composite's texture addressing): texel k holds boundary k + 1, so for
/// 1 <= c < nz the value is linear between texels floor(c) - 1 and floor(c); below 1 it is
/// texel 0 scaled by c (the zero boundary at the camera: inside the first slice the sums grow
/// from 0); at or beyond nz the last texel is carried (the near air in front of far ground
/// keeps its light). 0 for c at or below 0, NaN, or no texels [design].
#[cfg(test)]
pub fn sums_at(nm: &[[f32; 2]], c: f32) -> [f32; 2] {
    let Some(&last) = nm.last() else { return [0.0; 2] };
    if c.is_nan() || c <= 0.0 {
        return [0.0; 2];
    }
    if c >= nm.len() as f32 {
        return last;
    }
    if c < 1.0 {
        return [nm[0][0] * c, nm[0][1] * c];
    }
    // 1 <= c < nz: k in 1..nz, both texels exist.
    let k = c.floor() as usize;
    let t = c - k as f32;
    let (a, b) = (nm[k - 1], nm[k]);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

/// The lit fraction of the air in front of a sample: Vbar = clamp(N / M, 0, 1), the
/// dF-weighted mean of V (no phase in it since the sums are phase-free, [`column_nm`]); 1
/// while M is at or below [`RATIO_EMPTY`] or the quotient is not finite. The `lit` view shows
/// it; the pixel's own phase is applied once afterwards ([`phase_floor`]) [design; documented:
/// Bauer 2019, slide 62].
#[cfg(test)]
pub fn lit_fraction(n: f32, m: f32) -> f32 {
    let v = n / m;
    if m.is_nan() || m <= RATIO_EMPTY || !v.is_finite() {
        return 1.0;
    }
    v.clamp(0.0, 1.0)
}

/// F_near: the fog share evaluated analytically at the sample's radial distance held to
/// the grid's end on its ray (`grid_end_distance` = the grid's depth x the ray's length), so
/// it is never above the sample's own share and beyond the grid it is the share at the end.
/// A NaN distance gives the grid's end (`f32::min` ignores NaN) [design].
#[cfg(test)]
pub fn near_share(distance: f32, grid_end_distance: f32, fog: AirFog) -> f32 {
    fog_share(distance.min(grid_end_distance), fog)
}

/// Whether a pixel's samples share one volume tap (the composite's rule): their view depths,
/// held to the grid's end, lie within `within` of a slice of each other, the slice's thickness
/// taken at the nearest ([`AirCurve::slice_width`]). The volume's value moves by less than a
/// quarter of a slice's change across them. Samples beyond the grid's end, cleared depth
/// included (its view depth is the grid's end), all read the end's value and agree. Only the
/// lookup is shared: each sample still takes its own F_near and blend. True for no samples [design].
#[cfg(test)]
pub fn slices_agree(view_depths: &[f32], curve: &AirCurve, within: f32) -> bool {
    let held = |z: f32| z.min(curve.depth);
    let (lo, hi) = view_depths.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &z| (lo.min(held(z)), hi.max(held(z))));
    view_depths.is_empty() || hi - lo <= within * curve.slice_width(lo)
}

/// The ray contrast of the first look (the tune key `RAYS`, superseded in the composite by
/// [`relative_lit`], which is it at V_ref 1 and SHADE 0): `clamp(1 - RAYS x (1 - Vbar), 0, 1)`.
/// Kept as the tests' reference. A NaN Vbar is taken as 1, a NaN RAYS as 1 [design].
#[cfg(test)]
pub fn ray_contrast(vbar: f32, rays: f32) -> f32 {
    let vbar = if vbar.is_nan() { 1.0 } else { vbar.clamp(0.0, 1.0) };
    let rays = if rays.is_nan() { 1.0 } else { rays.max(0.0) };
    (1.0 - rays * (1.0 - vbar)).clamp(0.0, 1.0)
}

/// The phase the composite applies (the tune key `FLOOR`, 0..=1, default 1): `p_eff =
/// max(p, FLOOR)`, p the normalised capped phase of the pixel's own ray. An **artistic
/// choice** [design]: the mean-one phase is well under 1 away from the sun (0.41 at the back
/// with g 0.3, less with a stronger lobe), so with FLOOR 0 every view away from the sun darkens
/// (12 to 21 levels of 255 at g 0.6 to 0.8, cap 2.5 to 4 [hypothesis: stills of 2026-11-20,
/// docs/design/volumetric-medium.md "The composite"]), while the weather's fog colour was authored for lit open air; FLOOR 1 keeps
/// lit air away from the sun as the weather drew it and lets only the forward side gain.
/// FLOOR 0 is the energy-neutral phase. NaN p is taken as 1, a NaN FLOOR as 1; FLOOR is held
/// to 0..=1, so p_eff <= most.
#[cfg(test)]
pub fn phase_floor(p: f32, floor: f32) -> f32 {
    let p = if p.is_nan() { 1.0 } else { p.max(0.0) };
    let floor = if floor.is_nan() { 1.0 } else { floor.clamp(0.0, 1.0) };
    p.max(floor)
}

/// The uniform's `light.w`: 0 when no light is to be drawn, else `2 - FLOOR` (FLOOR held to
/// 0..=1, NaN as 1), so the flag reads above 0.5 whenever the light draws and is 1 at the
/// default FLOOR 1; [`floor_from_flag`] reads FLOOR back (the composite's mirror) [design: the
/// uniform has no free slot for FLOOR].
pub fn light_flag(draws: bool, floor: f32) -> f32 {
    let floor = if floor.is_nan() { 1.0 } else { floor.clamp(0.0, 1.0) };
    if draws { 2.0 - floor } else { 0.0 }
}

/// FLOOR from the uniform's `light.w` ([`light_flag`]): `clamp(2 - w, 0, 1)`.
#[cfg(test)]
pub fn floor_from_flag(w: f32) -> f32 {
    (2.0 - w).clamp(0.0, 1.0)
}

/// The reference visibility V_ref (the relative lit fraction's average): the tune's `REF`
/// override (held to 0.05..=1) when set, else the clouds' mean transmittance `cloud_mean`
/// (the uniform's `cloud_mode.y`; 0 or not finite taken as 1; held to 0.25..=1). The weather's
/// fog colour was authored for the average light under that weather's cloud, so the light is
/// relative to the average visibility, not to full sun [design: judged by
/// eye from stills, 2026-11-20: every picture darker as a whole while lit air only ever lost light].
#[cfg(test)]
pub fn reference_visibility(reference: Option<f32>, cloud_mean: f32) -> f32 {
    match reference.filter(|r| r.is_finite()) {
        Some(r) => r.clamp(0.05, 1.0),
        None => {
            let m = if cloud_mean.is_finite() && cloud_mean > 0.0 { cloud_mean } else { 1.0 };
            m.clamp(0.25, 1.0)
        }
    }
}

/// The clouds' own mean transmittance for a light from straight above, T, with part of the sky in the
/// light volume: `t_vol` the mean of exp(-optical depth) along the light's slanted way over the covered
/// points (neither the shadows' strength nor their low-sun fade is in it, as both are in K), `covered`
/// the covered share, `t_flat` the flat layers' (medium_air_grid.rs `flat_deck`), which has no way and
/// no light in it. By Beer's law in the path's length a slanted way through the same clouds is 1 / up times
/// as long, so the volume's is `max(t_vol, AIR_DECK_THIN)^up`, `up` the height of the unit direction
/// to the light (the uniform's `light.y`; `follow_air` packs the sun light's back, world y up) held
/// to `AIR_DECK_UP`..=1, NaN as 1. The power is of the points' mean, not of each point's depth before
/// the mean: of clouds about as tall as they are wide the slanted light meets more the lower it
/// stands (the gap fraction falls as the same power of 1 / up), while a point's power keeps an opaque
/// cloud opaque, and what the longer shadows cover would read as more cloud: one clear weather's
/// mean along the light, taken so, was 0.867, 0.879 and 0.893 at noon, 17:30 and 18:36, where each
/// point's own power would give about 0.87 at noon and 0.7 at 18:36 [hypothesis: read back from stills of 2026-11-24
/// with a development build; the steps were not kept]. The known limit is a broken deck without
/// height, whose gaps no slant closes: it reads clearer at a low sun (half clear, half opaque, D is
/// 0.35 overhead and 1 at a height of 0.25). The two are mixed by the covered share. Both describe the same deck, so the share
/// does not change with who covers it: at a low light the ways leave the volume's window, under the
/// shadows' band the volume is not read. The mean pass takes it (medium_air_grid.wesl `air_cloud_mean`)
/// [design, 2026-11-24].
#[cfg(test)]
pub fn deck_transmittance(t_vol: f32, covered: f32, t_flat: f32, up: f32) -> f32 {
    let up = if up.is_nan() { 1.0 } else { up.clamp(AIR_DECK_UP, 1.0) };
    covered * t_vol.clamp(AIR_DECK_THIN, 1.0).powf(up) + (1.0 - covered) * t_flat
}

/// The deck's share D of the correction: `AIR_DECK_LEAST` + the rest x the smoothstep of the clouds' own
/// transmittance T (the uniform's `cloud_mode.z`: [`deck_transmittance`], or the flat layers' alone; a
/// 0 is an opaque deck; the reference visibility keeps the mean of K) from
/// `AIR_DECK_FROM` to `AIR_DECK_TO`, once a pixel, times the applied
/// scale ([`applied_scale`]), so it weakens what is added and what is taken alike, the sky's too.
/// The sunlit share s ([`scaled_share`]) is taken of the weather's lights, which under a closed deck
/// still name a direct part, while the air's light there is mostly the deck's own: the light lifted
/// an overcast picture as a whole. 1 while the tune's `SHARE` is set
/// (`share_set`: an absolute share, a diagnostic). The gate, the log's bounds and [`pack`] are
/// before it: upper bounds [design, 2026-11-24].
#[cfg(test)]
pub fn deck_share(transmittance: f32, share_set: bool) -> f32 {
    if share_set {
        return 1.0;
    }
    let x = ((transmittance - AIR_DECK_FROM) / (AIR_DECK_TO - AIR_DECK_FROM)).clamp(0.0, 1.0);
    AIR_DECK_LEAST + (1.0 - AIR_DECK_LEAST) * (x * x * (3.0 - 2.0 * x))
}

/// The relative lit fraction: `Vbar_c = clamp(1 + RAYS x (Vbar - V_ref) / V_ref, SHADE,
/// LIFT)`: air as lit as the weather's average (Vbar = V_ref) is exactly 1 whatever RAYS is
/// (unchanged), shadowed air falls toward SHADE and lit air rises up to LIFT (1..=3: how far
/// above the average a fully lit ray may rise) [design; an artistic gain]. SHADE (0..=1, the
/// tune key, default 0.9) limits what shadow takes: shadowed
/// air is meant to be no darker than the open haze, while at RAYS 6 a slightly below-average lit
/// fraction went to no light at all [design: judged by eye, 2026-11-20]. With SHADE 0,
/// V_ref = 1 and any LIFT >= 1 it is [`ray_contrast`]. Vbar held to 0..=1 (NaN 1), V_ref to at
/// least 0.05 (NaN 1), RAYS to at least 0 (NaN 1), LIFT to at least 1 (NaN 1), SHADE to 0..=1
/// (NaN 0).
#[cfg(test)]
pub fn relative_lit(vbar: f32, v_ref: f32, rays: f32, lift: f32, shade: f32) -> f32 {
    let vbar = if vbar.is_nan() { 1.0 } else { vbar.clamp(0.0, 1.0) };
    let v_ref = if v_ref.is_nan() { 1.0 } else { v_ref.clamp(0.05, 1.0) };
    let rays = if rays.is_nan() { 1.0 } else { rays.max(0.0) };
    let lift = if lift.is_nan() { 1.0 } else { lift.max(1.0) };
    let shade = if shade.is_nan() { 0.0 } else { shade.clamp(0.0, 1.0) };
    (1.0 + rays * (vbar - v_ref) / v_ref).clamp(shade, lift)
}

// ---------------------------------------------------------------------------------------------
// The composite's one form
// ---------------------------------------------------------------------------------------------

/// The far blend t of a sample: 1 for cleared depth (the
/// sky), else the smoothstep `x^2 (3 - 2x)`, x = clamp((d - 0.6 sky_from) / (0.4 sky_from), 0,
/// 1), of its radial distance d: 0 nearer than [`SKY_TAPER_FROM`] x `sky_from`, 1 from
/// `sky_from` on (sky.rs `SKY_DEPTH_FROM`), value and slope continuous at both ends. A
/// degenerate `sky_from` (not above 0, NaN) or a NaN distance gives 1. Opaque terrain lies
/// beyond `sky_from` too (volumetric_clouds.rs), so the class "sky" is cleared depth only and
/// far terrain reaches the sky's form smoothly [design].
#[cfg(test)]
pub fn far_blend(distance: f32, cleared: bool, sky_from: f32) -> f32 {
    if cleared || distance.is_nan() || sky_from.is_nan() || sky_from <= 0.0 {
        return 1.0;
    }
    let from = SKY_TAPER_FROM * sky_from;
    let x = ((distance - from) / (sky_from - from)).clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// The fog blend g of a sample: the smoothstep of its fog share F_near ([`near_share`]) from
/// `AIR_FOG_BLEND_FROM` to 1: 0 where the surface shows through the fog, 1 where the fog covers
/// it, value and slope continuous at both ends [design, 2026-11-24]. A NaN share gives 0.
#[cfg(test)]
pub fn fog_blend(f_near: f32) -> f32 {
    if f_near.is_nan() {
        return 0.0;
    }
    let x = ((f_near - AIR_FOG_BLEND_FROM) / (1.0 - AIR_FOG_BLEND_FROM)).clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// A sample's blend t between the world's rule and the sky's: the far blend t_d ([`far_blend`])
/// joined with the fog blend g ([`fog_blend`]) as `t_d + (1 - t_d) g`: never below either, 1 when
/// either is, and, unlike the larger of the two, with no break in slope where they cross. The same
/// t goes into the correction and the budget, so a surface the fog covers wholly (F_near 1) takes
/// exactly the result the sky dome behind it takes; at and below `AIR_FOG_BLEND_FROM` it is the far
/// blend alone [design, 2026-11-24].
#[cfg(test)]
pub fn sample_blend(t_distance: f32, f_near: f32) -> f32 {
    t_distance + (1.0 - t_distance) * fog_blend(f_near)
}

/// WGSL's `mix(x, y, t)`, written as the shader computes it: `x (1 - t) + y t` (exact at t 0
/// and 1 for finite x and y).
#[cfg(test)]
pub fn mix(x: f32, y: f32, t: f32) -> f32 {
    x * (1.0 - t) + y * t
}

/// One sample's display-space scalar in the one form: `corr_t = a F_near (Vbar_c x mix(p_eff, mix(1, p_eff, SKYPHASE), t) - 1) x mix(1, SKY,
/// t)`, then held to at least `-min(F_near, 1)`: shadow removes at most the budget share F_near
/// of B_t ([`compose_blended`]), whatever the keys or the uniform carry. `a` here is the applied
/// scale k x a ([`applied_scale`], 0..=2: the strength times the share), so this is the
/// strength times the share's correction, floored after the scaling (with the scale at most 1,
/// SKY in 0..=1 and Vbar_c >= 0 the hold never acts; above 1 a deep shadow may reach it: it is
/// the floor's guarantee, not a look).
/// SKYPHASE is held to 0..=1 (NaN 0); SKY and the rest are taken as the uniform carries them.
/// At t = 0 this is the world rule's `a F_near (Vbar_c p_eff - 1)` exactly, at t = 1 the
/// sky's `a F_near (Vbar_c mix(1, p_eff, SKYPHASE) - 1) SKY` [design].
#[cfg(test)]
pub fn blended_correction(vbar_c: f32, p_eff: f32, sky_phase: f32, a: f32, f_near: f32, sky: f32, t: f32) -> f32 {
    let k = if sky_phase.is_nan() { 0.0 } else { sky_phase.clamp(0.0, 1.0) };
    let phase_t = mix(p_eff, mix(1.0, p_eff, k), t);
    let corr = a * f_near * (vbar_c * phase_t - 1.0) * mix(1.0, sky, t);
    corr.max(-f_near.min(1.0))
}

/// The bright hold h: the share of a gain (display values) to hold back from a pixel whose largest
/// channel is `peak`. Nothing while `peak + gain` is at most `AIR_BRIGHT_FROM`; the part over it
/// ("over") is fitted into the room left under `AIR_BRIGHT_TO`, counted from the larger of `peak` and
/// `AIR_BRIGHT_FROM`, as `over x room / (over + room)`: a small excess passes nearly whole, a large
/// one never fills the room, and from `AIR_BRIGHT_TO` on there is no room and h is 1. So with the
/// hold whole the pixel comes out at most at the larger of `peak` and `AIR_BRIGHT_TO`, and a
/// brighter input is never a darker output whatever the gain's strength: with R the room and o the
/// excess, the output's slope above `AIR_BRIGHT_FROM` is `1 - o^2 / (o + R)^2` plus the gain's own,
/// neither below 0, and below it R is fixed and the output rises with o.
///
/// It looks at the lifted value, not at the input's brightness alone: a fade of the gain over an
/// input between the same ends cannot be monotonic at the stronger look's 200 % (a gain of up to 0.96
/// of the sky lifts 0.55 to 1.08, which a 0.80 left alone is darker than) [design, 2026-11-24; tested].
#[cfg(test)]
pub fn bright_hold(peak: f32, gain: f32) -> f32 {
    let over = gain - (AIR_BRIGHT_FROM - peak).max(0.0);
    if over.is_nan() || over <= AIR_BRIGHT_LEAST {
        return 0.0;
    }
    let room = (AIR_BRIGHT_TO - peak.max(AIR_BRIGHT_FROM)).max(0.0);
    over / (over + room) * (over / gain)
}

/// One sample's scalar with the bright hold: a gain (`corr` above 0) becomes `corr x (1 - t h)`, h
/// ([`bright_hold`]) taken of the gain on the pixel's largest channel, `corr x mix(fog_peak, peak,
/// t)` (`peak` and `fog_peak` the largest channels of Y' and C'). One scalar for the three channels,
/// so a hue keeps; whole for the sky (t 1), none near (t 0: the world's rule is as it was), and
/// shadow (`corr` at most 0) is untouched. Between, the output is the t-weighted mean of the two
/// outputs, each rising with the pixel's brightness, so it rises too [design, 2026-11-24].
#[cfg(test)]
pub fn hold_bright(corr: f32, peak: f32, fog_peak: f32, t: f32) -> f32 {
    if corr > 0.0 {
        corr * (1.0 - t * bright_hold(peak, corr * mix(fog_peak, peak, t)))
    } else {
        corr
    }
}

/// The one form's composite: per channel
/// `Y'_new = Y' + corr_t x B_t` with the budget `B_t = mix(C', Y', t)` (the fog colour near,
/// the pixel's own display value far: the sky's multiplicative form is the additive form with
/// the pixel's own colour as the budget), `Y' = max(pixel, 0)^(1/2.2)`, `C' = max(fog_colour,
/// 0)^(1/2.2)`; the result `max(Y'_new, 0)^2.2` held to 0..=[`AIR_HDR_MAX`].
/// Since B_t >= 0, `corr_t >= -F_near` gives `Y'_new >= Y' - F_near B_t`, and near (t = 0) for a
/// fogged fragment `Y' = (1 - f) S' + f C'` with F_near <= f that is `>= (1 - f) S'`.
///
/// **A sample with geometry keeps its surface light at every t**: far, the budget holds the pixel's own Y' and shadow could take part of the
/// surface's light (S' 1, C' 0, f = F_near = 0.5, t 1, full shadow: Y' 0.5 became 0.25). So for
/// such a sample (`geometry_f_near` = Some(F_near)) the change is held per channel to
/// `delta = max(corr_t x B_t, -F_near x C')`: shadow removes at most the fog's own share of the
/// fog colour, `Y'_new >= Y' - F_near C' >= (1 - f) S'` whenever F_near <= f, at every t.
/// Positive changes are left as they are. Cleared depth (`None`) keeps the scalar bound alone
/// (the sky has no surface under it) [design].
#[cfg(test)]
pub fn compose_blended(pixel: Vec3, corr: f32, fog_colour: Vec3, t: f32, geometry_f_near: Option<f32>) -> Vec3 {
    let to_display = |v: Vec3| v.max(Vec3::ZERO).powf(1.0 / DISPLAY_GAMMA);
    let (y, c) = (to_display(pixel), to_display(fog_colour));
    let budget = c * (1.0 - t) + y * t;
    let mut delta = corr * budget;
    if let Some(f_near) = geometry_f_near {
        delta = delta.max(-f_near * c);
    }
    (y + delta).max(Vec3::ZERO).powf(DISPLAY_GAMMA).clamp(Vec3::ZERO, Vec3::splat(AIR_HDR_MAX))
}

// The first look's two forms, the one form's limits (t = 0 and t = 1), kept for the tests to
// compare with.

/// The world rule: `a x f_near x (r - 1)`, r = Vbar_c x p_eff ([`applied_ratio`]).
#[cfg(test)]
pub fn correction(f_near: f32, r: f32, a: f32) -> f32 {
    a * f_near * (r - 1.0)
}

/// A world sample's applied ratio: `R_c = Vbar_c x p_eff` ([`phase_floor`]).
#[cfg(test)]
pub fn applied_ratio(vbar_c: f32, p: f32, floor: f32) -> f32 {
    let vbar_c = if vbar_c.is_nan() { 1.0 } else { vbar_c.max(0.0) };
    vbar_c * phase_floor(p, floor)
}

/// The world rule's composite: per channel `max(max(pixel, 0)^(1/2.2) + corr x
/// max(fog_colour, 0)^(1/2.2), 0)^2.2`.
#[cfg(test)]
pub fn compose_ratio(pixel: Vec3, corr: f32, fog_colour: Vec3) -> Vec3 {
    let to_display = |v: Vec3| v.max(Vec3::ZERO).powf(1.0 / DISPLAY_GAMMA);
    (to_display(pixel) + corr * to_display(fog_colour)).max(Vec3::ZERO).powf(DISPLAY_GAMMA)
}

/// The sky rule: `a x F_near x (Vbar_c x mix(1, p, SKYPHASE) - 1) x SKY`
/// [documented: Bauer 2019, slide 62].
#[cfg(test)]
pub fn sky_modulation(vbar: f32, p: f32, sky_phase: f32, a: f32, f_near: f32, sky: f32) -> f32 {
    let vbar = if vbar.is_nan() { 1.0 } else { vbar.max(0.0) };
    let k = if sky_phase.is_nan() { 0.0 } else { sky_phase.clamp(0.0, 1.0) };
    let r_sky = vbar * (1.0 + (p - 1.0) * k);
    a * f_near * (r_sky - 1.0) * sky
}

/// The sky rule's composite: `max(pixel, 0) x max(1 + corr_sky, 0)^2.2` (Y' x (1 + corr_sky)
/// in display values).
#[cfg(test)]
pub fn compose_sky(pixel: Vec3, corr_sky: f32) -> Vec3 {
    pixel.max(Vec3::ZERO) * (1.0 + corr_sky).max(0.0).powf(DISPLAY_GAMMA)
}

// ---------------------------------------------------------------------------------------------
// The sunlit share
// ---------------------------------------------------------------------------------------------

/// Rec. 709 luma of a colour, held to 0 or above: `0.2126 r + 0.7152 g + 0.0722 b`
/// [documented: ITU-R Recommendation BT.709, its luminance equation]. Taken of the lights'
/// display-value colours as the display-value lighting sum uses them (lights.rs
/// `display_color`) [design].
pub fn display_luma(c: Vec3) -> f32 {
    (0.2126 * c.x + 0.7152 * c.y + 0.0722 * c.z).max(0.0)
}

/// The sunlit share s from consistently scaled lights: `d / (d + a)` with
/// d = `direct_luma x direct_scale`, a = `ambient_luma x ambient_scale` (each factor held to
/// 0 or above), held to 0..=1; 0 for any non-finite input. The denominator is taken as at
/// least [`SHARE_FLOOR`], so a black pair gives 0 and, with a black ambient, s falls with the
/// direct light near black instead of holding 1 while the sun fades out. What the caller
/// passes (sky.rs names and lines): the module doc, "What scaled_share's caller passes"
/// [design].
pub fn scaled_share(direct_luma: f32, direct_scale: f32, ambient_luma: f32, ambient_scale: f32) -> f32 {
    let inputs = [direct_luma, direct_scale, ambient_luma, ambient_scale];
    if !inputs.iter().all(|v| v.is_finite()) {
        return 0.0;
    }
    let d = direct_luma.max(0.0) * direct_scale.max(0.0);
    let a = ambient_luma.max(0.0) * ambient_scale.max(0.0);
    let s = d / (d + a).max(SHARE_FLOOR);
    if s.is_finite() { s.clamp(0.0, 1.0) } else { 0.0 }
}

/// a = clamp(the tune's `SHARE` override, else s, 0, 1): the sunlit share alone; NaN
/// gives 0. The strength is no longer in it: it scales the whole correction
/// ([`applied_scale`]) [design].
pub fn effective_share(s: f32, share_override: Option<f32>) -> f32 {
    let a = share_override.unwrap_or(s);
    if a.is_nan() { 0.0 } else { a.clamp(0.0, 1.0) }
}

/// The scale the composite applies, `k x a`: the strength k held to 0..=2 (NaN 0) times the
/// effective share a ([`effective_share`], 0..=1), so 0..=2. It multiplies the whole
/// correction before the floor (`corr_t' = k x corr_t`, [`blended_correction`] takes it as its
/// `a`): k 0 changes no pixel, k 2 doubles every correction and the floor
/// `-min(F_near, 1)` still holds. Carried in the uniform's `phase2.z`; the composite multiplies it
/// by the deck's share ([`deck_share`]) [design].
pub fn applied_scale(strength: f32, a: f32) -> f32 {
    let k = if strength.is_nan() { 0.0 } else { strength.clamp(AIR_STRENGTHS.0, AIR_STRENGTHS.1) };
    let a = if a.is_nan() { 0.0 } else { a.clamp(0.0, 1.0) };
    k * a
}

/// Whether it is day for the light (the gate's night test):
/// the signed elevation, in degrees, of the sun at the hour that aimed the light above 0.
/// sky.rs `update_sky` aims the `Sun` entity's light from `sun_direction(climate,
/// SkyState::sun_light_hour())` (turned round to the moon by `aim_sun` while that is below the
/// horizon); `SkyState::sun_elevation_at(sky.sun_light_hour())` is the same direction's
/// elevation, so the caller passes that. No comparison with the stepped hour's sun: in a
/// climate whose day lasts a thousandth of an hour (sunrise begin equal to sunset end, which
/// fnv-world's `sun_direction` allows) the sun moves far within one sky step, and such a
/// comparison took real daylight for night. Exactly 0 (the horizon) and NaN are not day
/// [design].
pub fn day_by_aim(elevation_degrees: f32) -> bool {
    elevation_degrees > 0.0
}

/// What the light in the air is lit by this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AirSource {
    /// The sun, above the horizon.
    Sun,
    /// The moon: the sun at or under the horizon, in a climate with a moon.
    Moon,
    /// Nothing: night in a climate without a moon, or an elevation that is not a number.
    None,
}

impl AirSource {
    /// What lights the air, in the "on" line.
    fn lit_by(self) -> &'static str {
        match self {
            AirSource::Sun => "lit by the sun",
            AirSource::Moon => "lit by the moon",
            AirSource::None => "lit by nothing",
        }
    }

    /// The share's name in the "on" line.
    fn share_name(self) -> &'static str {
        if self == AirSource::Moon { "moonlit share" } else { "sunlit share" }
    }
}

/// The light's source from the elevation that aimed the light ([`day_by_aim`]) and whether the climate has
/// a moon (`Climate::has_moon`). At exactly 0 the light's strength, so the share, is 0 (sky.rs `aim_sun`),
/// and the source changes there: the change itself cannot show [design].
pub fn air_source(elevation_degrees: f32, has_moon: bool) -> AirSource {
    if day_by_aim(elevation_degrees) {
        AirSource::Sun
    } else if elevation_degrees <= 0.0 && has_moon {
        AirSource::Moon
    } else {
        AirSource::None
    }
}

// ---------------------------------------------------------------------------------------------
// Aids and the tune file
// ---------------------------------------------------------------------------------------------

/// The debug views (`NVF_PLAY_DEBUG_MEDIUM_LIGHT_VIEW`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AirView {
    /// No view: the light as composed.
    #[default]
    None,
    /// V at the pixel's froxel.
    Visibility,
    /// The slice index, banded.
    Slice,
    /// F at the pixel.
    Share,
    /// The light's difference (for pairs).
    Delta,
    /// The grid made but not composed (for pairs).
    Off,
    /// The phase-free lit fraction Vbar at the pixel as opaque grey ([`lit_fraction`]): the shafts with no look constants in the way.
    Lit,
    /// The signed scalar the light actually applies (corr on world samples, corr_sky on sky
    /// samples) as opaque grey: what the light does, at a glance.
    Rays,
}

impl AirView {
    /// The shader's code, 0..=7 in the order above.
    pub fn code(self) -> u32 {
        match self {
            AirView::None => 0,
            AirView::Visibility => 1,
            AirView::Slice => 2,
            AirView::Share => 3,
            AirView::Delta => 4,
            AirView::Off => 5,
            AirView::Lit => 6,
            AirView::Rays => 7,
        }
    }

    /// The view's name as the aid and the tune file take it.
    #[cfg(any(test, debug_assertions, feature = "dev-aids"))]
    pub fn name(self) -> &'static str {
        match self {
            AirView::None => "none",
            AirView::Visibility => "visibility",
            AirView::Slice => "slice",
            AirView::Share => "share",
            AirView::Delta => "delta",
            AirView::Off => "off",
            AirView::Lit => "lit",
            AirView::Rays => "rays",
        }
    }

    fn named(name: &str) -> Option<AirView> {
        match name.trim().to_ascii_lowercase().as_str() {
            "none" => Some(AirView::None),
            "visibility" => Some(AirView::Visibility),
            "slice" => Some(AirView::Slice),
            "share" => Some(AirView::Share),
            "delta" => Some(AirView::Delta),
            "off" => Some(AirView::Off),
            "lit" => Some(AirView::Lit),
            "rays" => Some(AirView::Rays),
            _ => None,
        }
    }
}

/// Which occluders V reads (`NVF_PLAY_DEBUG_MEDIUM_LIGHT_OCCLUDERS`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AirOccluders {
    #[default]
    All,
    Cascades,
    Terrain,
    None,
    /// The flat clouds' shadow K alone (the grid shader's code 4, f8f86063d; `all` is
    /// K x min(C, T)).
    Clouds,
}

impl AirOccluders {
    /// The shader's code, 0..=4 in the order above.
    pub fn code(self) -> u32 {
        match self {
            AirOccluders::All => 0,
            AirOccluders::Cascades => 1,
            AirOccluders::Terrain => 2,
            AirOccluders::None => 3,
            AirOccluders::Clouds => 4,
        }
    }

    /// The occluders' name as the aid and the tune file take it.
    #[cfg(any(test, debug_assertions, feature = "dev-aids"))]
    pub fn name(self) -> &'static str {
        match self {
            AirOccluders::All => "all",
            AirOccluders::Cascades => "cascades",
            AirOccluders::Terrain => "terrain",
            AirOccluders::None => "none",
            AirOccluders::Clouds => "clouds",
        }
    }

    fn named(name: &str) -> Option<AirOccluders> {
        match name.trim().to_ascii_lowercase().as_str() {
            "all" => Some(AirOccluders::All),
            "cascades" => Some(AirOccluders::Cascades),
            "terrain" => Some(AirOccluders::Terrain),
            "none" => Some(AirOccluders::None),
            "clouds" => Some(AirOccluders::Clouds),
            _ => None,
        }
    }
}

fn number(value: &str) -> Option<f32> {
    value.trim().parse::<f32>().ok().filter(|v| v.is_finite())
}

/// `NVF_PLAY_DEBUG_MEDIUM_LIGHT`: the strength, 0..=2; unset, unreadable or non-finite is 0
/// (off).
pub fn debug_strength(value: Option<&str>) -> f32 {
    value.and_then(number).map_or(0.0, |v| v.clamp(AIR_STRENGTHS.0, AIR_STRENGTHS.1))
}

/// `NVF_PLAY_DEBUG_MEDIUM_LIGHT_VIEW`: `visibility|slice|share|delta|off|lit|rays`;
/// anything else none.
pub fn debug_view(value: Option<&str>) -> AirView {
    value.and_then(AirView::named).unwrap_or_default()
}

/// `NVF_PLAY_DEBUG_MEDIUM_LIGHT_GRID`: `<x>,<y>,<z>` whole numbers; anything else none (the
/// grid's own holding applies in [`AirGrid::for_view`]).
pub fn debug_grid(value: Option<&str>) -> Option<[u32; 3]> {
    let parts: Vec<u32> = value?.split(',').map(|p| p.trim().parse::<u32>()).collect::<Result<_, _>>().ok()?;
    match parts[..] {
        [x, y, z] => Some([x, y, z]),
        _ => None,
    }
}

/// `NVF_PLAY_DEBUG_MEDIUM_LIGHT_DEPTH`: units, a finite number above 0; anything else none
/// (held to range by [`AirGrid::for_view`]).
pub fn debug_depth(value: Option<&str>) -> Option<f32> {
    value.and_then(number).filter(|d| *d > 0.0)
}

/// `NVF_PLAY_DEBUG_MEDIUM_LIGHT_OCCLUDERS`: `all|cascades|terrain|none|clouds`; anything else
/// all.
pub fn debug_occluders(value: Option<&str>) -> AirOccluders {
    value.and_then(AirOccluders::named).unwrap_or_default()
}

/// How the grid pass walks a column, as the start aid asks (medium_air_grid.rs `GridWalk`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AirWalkAid {
    /// By the adapter.
    #[default]
    Auto,
    Serial,
    /// The chunks on any adapter their workgroup fits.
    Chunks,
}

/// `NVF_PLAY_DEBUG_MEDIUM_LIGHT_WALK`: `auto|serial|chunks`; anything else auto.
pub fn debug_walk(value: Option<&str>) -> AirWalkAid {
    match value.map(str::trim) {
        Some("serial") => AirWalkAid::Serial,
        Some("chunks") => AirWalkAid::Chunks,
        _ => AirWalkAid::Auto,
    }
}

/// What the strength is multiplied by in moonlight ([`MoonTune::strength`]) [design: the starting value, to be
/// set with pictures].
pub const MOON_STRENGTH: f32 = 1.0;

/// The light's constants in moonlight where they are not the day's ([`AirTune::lit_by`]): each `None` is the
/// day's value, a tune file's included (`MOON_RAYS` and so on) [design].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoonTune {
    /// `MOON_STRENGTH`: the factor on the strength, 0..=2; the product is held to the strength's range.
    pub strength: f32,
    /// `MOON_RAYS`, `MOON_LIFT`, `MOON_SHADE`, `MOON_G`, `MOON_CAP`, `MOON_SKY`, `MOON_SKYPHASE`: in the
    /// ranges of the day's.
    pub rays: Option<f32>,
    pub lift: Option<f32>,
    pub shade: Option<f32>,
    pub g: Option<f32>,
    pub cap: Option<f32>,
    pub sky_weight: Option<f32>,
    pub sky_phase: Option<f32>,
}

impl MoonTune {
    /// The day's values, unchanged.
    pub const DAY: MoonTune = MoonTune { strength: 1.0, rays: None, lift: None, shade: None, g: None, cap: None, sky_weight: None, sky_phase: None };
    /// The moonlight's start values.
    pub const START: MoonTune = MoonTune {
        strength: MOON_STRENGTH,
        rays: Some(8.0),
        lift: Some(1.6),
        shade: Some(0.4),
        g: Some(0.7),
        cap: Some(3.0),
        sky_weight: Some(0.3),
        sky_phase: Some(0.15),
    };

    /// The values by their tune keys, in their order.
    fn named(&self) -> [(&'static str, Option<f32>); 8] {
        [
            ("MOON_STRENGTH", Some(self.strength)),
            ("MOON_RAYS", self.rays),
            ("MOON_LIFT", self.lift),
            ("MOON_SHADE", self.shade),
            ("MOON_G", self.g),
            ("MOON_CAP", self.cap),
            ("MOON_SKY", self.sky_weight),
            ("MOON_SKYPHASE", self.sky_phase),
        ]
    }
}

/// The look's constants, from the start aids and then the tune file
/// (`NVF_PLAY_DEBUG_MEDIUM_LIGHT_TUNE`, read at most every 0.5 s by the GPU half) [design].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AirTune {
    /// The light's strength, the one overall scale of its correction ([`applied_scale`]),
    /// 0..=2 (0 off; 1 the look as tuned).
    pub strength: f32,
    /// The sunlit share s, overriding [`scaled_share`] when set, 0..=1 (`SHARE`); the deck's share
    /// ([`deck_share`]) is then 1.
    pub share: Option<f32>,
    /// The forward lobe's g, 0..=0.95 (`G`; a forward lobe, never a back one: that is
    /// `BACK`'s).
    pub g: f32,
    /// The back lobe's g, -0.95..=0.95 (`BACK`).
    pub back_g: f32,
    /// The back lobe's weight, 0..=1 (`BACKWEIGHT`).
    pub back_weight: f32,
    /// The phase's final cap (its largest value after normalising), 1..=8 (`CAP`:
    /// 100 let a lit ray write past the HDR target's range, and the
    /// choices by eye were 1.2 to 4).
    pub cap: f32,
    /// The grid's depth, overriding the start aid when set, 1,000..=200,000.
    pub depth: Option<f32>,
    /// SKY: the weight of the far form (`mix(1, SKY, t)` in [`blended_correction`]: cleared
    /// depth takes it whole, far or fogged terrain by its blend t), 0..=1, default 0.25. Only ever a
    /// weight: above 1 it amplified what shadow takes past the fog light a pixel was given.
    pub sky_weight: f32,
    /// SKYPHASE: how much of the phase a sky sample's modulation takes
    /// ([`blended_correction`]), 0..=1; default 0 (the sky dome draws its own glow round the
    /// sun, so the sky only darkens where the near air is shadowed).
    pub sky_phase: f32,
    /// `RAYS`: the ray contrast ([`relative_lit`]), 0..=8; default 6 (1 is physical).
    pub rays: f32,
    /// `FLOOR`: the phase floor ([`phase_floor`]), 0..=1; default 1 (0 is energy-neutral).
    pub floor: f32,
    /// `REF`: the reference visibility's override ([`reference_visibility`]), 0.05..=1;
    /// `auto` (none, the default) takes the clouds' mean transmittance.
    pub reference: Option<f32>,
    /// `LIFT`: how far above the weather's average a fully lit ray may rise
    /// ([`relative_lit`]), 1..=3; default 1.4.
    pub lift: f32,
    /// `SHADE`: the least relative lit fraction shadowed air keeps ([`relative_lit`]), 0..=1;
    /// default 0.9 (0: shadow may take all the fog's light).
    pub shade: f32,
    /// `SAMPLES`: the grid's visibility samples per slice, 1..=8; default 4 (the grid pass
    /// reads it; its uniform slot is the grid writer's).
    pub samples: u32,
    /// `FILTER`: the 3 x 3 filter over the volume after the grid pass (`0` off, `1` on);
    /// default on (medium_air_grid.rs `air_filter_pass`).
    pub filter: bool,
    pub view: AirView,
    pub occluders: AirOccluders,
    /// The grid's columns, rows and slices when asked (`GRID`, or the start aid
    /// `NVF_PLAY_DEBUG_MEDIUM_LIGHT_GRID`); held by [`AirGrid::for_view`].
    pub grid: Option<[u32; 3]>,
    /// The moonlight's own values ([`AirTune::lit_by`]).
    pub moon: MoonTune,
}

impl Default for AirTune {
    /// The Natural look [design, 2026-11-24: docs/design/volumetric-medium.md, "Debug aids, tune
    /// keys and log lines"]: strength 0 (the start's value comes
    /// through [`AirTune::start`] or [`AirTune::with_strength`]: the slider's 100 % is 1),
    /// forward g 0.5 and final cap 1.5 with no back lobe, RAYS 3, LIFT 1.4, SHADE 0.9, FLOOR 1,
    /// sky weight 0.25 and sky phase 0, REF auto, no view, all occluders, SAMPLES 4, FILTER on.
    /// Before it: g 0.6, cap 2.5, RAYS 6, LIFT 1.5, SHADE 0.9, sky weight 0.5 (2026-11-20).
    fn default() -> Self {
        let p = AirPhase::default();
        AirTune {
            strength: 0.0,
            share: None,
            g: 0.5,
            back_g: p.back_g,
            back_weight: p.back_weight,
            cap: 1.5,
            depth: None,
            sky_weight: 0.25,
            sky_phase: 0.0,
            rays: 3.0,
            floor: 1.0,
            reference: None,
            lift: 1.4,
            shade: 0.9,
            samples: AIR_SAMPLES_DEFAULT,
            filter: true,
            view: AirView::None,
            occluders: AirOccluders::All,
            grid: None,
            moon: MoonTune::START,
        }
    }
}

impl AirTune {
    /// The defaults with the start aids' values ([`debug_strength`], [`debug_view`],
    /// [`debug_occluders`], [`debug_depth`]).
    pub fn start(strength: f32, view: AirView, occluders: AirOccluders, depth: Option<f32>) -> AirTune {
        AirTune {
            strength: strength.clamp(AIR_STRENGTHS.0, AIR_STRENGTHS.1),
            view,
            occluders,
            depth: depth.map(|d| d.clamp(AIR_DEPTHS.0, AIR_DEPTHS.1)),
            ..AirTune::default()
        }
    }

    /// These constants with the strength `strength`, held to 0..=2 (NaN 0): the start tune
    /// with the settings row's slider or the aid's value (medium_air_light.rs).
    pub fn with_strength(self, strength: f32) -> AirTune {
        let strength = if strength.is_nan() { 0.0 } else { strength.clamp(AIR_STRENGTHS.0, AIR_STRENGTHS.1) };
        AirTune { strength, ..self }
    }

    /// The constants the light is drawn with under `source`: these by the sun; by the moon its values in their
    /// place (its strength a factor on this one's, held to the strength's range). The result's own moon values
    /// are the day's: used up by night, out of the way of the log by day.
    pub fn lit_by(self, source: AirSource) -> AirTune {
        let day = AirTune { moon: MoonTune::DAY, ..self };
        if source != AirSource::Moon {
            return day;
        }
        let m = self.moon;
        AirTune {
            g: m.g.unwrap_or(day.g),
            cap: m.cap.unwrap_or(day.cap),
            sky_weight: m.sky_weight.unwrap_or(day.sky_weight),
            sky_phase: m.sky_phase.unwrap_or(day.sky_phase),
            rays: m.rays.unwrap_or(day.rays),
            lift: m.lift.unwrap_or(day.lift),
            shade: m.shade.unwrap_or(day.shade),
            ..day.with_strength(day.strength * m.strength)
        }
    }

    /// The phase these constants give.
    pub fn phase(&self) -> AirPhase {
        AirPhase { g: self.g, back_g: self.back_g, back_weight: self.back_weight, cap: self.cap }
    }

    /// A `NAME=value` file over `base`: one per line, `#` starts a comment, blank lines
    /// skipped, names case-insensitive (`STRENGTH`, `SHARE`, `G`, `BACK`, `BACKWEIGHT`, `CAP`,
    /// `DEPTH`, `GRID` (`<x>,<y>,<z>`), `SKY`, `SKYPHASE`, `RAYS`, `FLOOR`, `REF`, `LIFT`,
    /// `SHADE`, `SAMPLES` (a whole number), `FILTER` (0 or 1), `VIEW`, `OCCLUDERS`; the moonlight's
    /// `MOON_STRENGTH`, `MOON_RAYS`, `MOON_LIFT`, `MOON_SHADE`, `MOON_G`, `MOON_CAP`, `MOON_SKY`,
    /// `MOON_SKYPHASE`).
    /// Unknown names and unreadable or non-finite values are ignored (the base value kept);
    /// every value is held to its field's range. `SHARE`, `DEPTH`, `GRID` and `REF` take `auto`
    /// to clear the override, the moonlight's keys but its strength to take the day's value.
    #[cfg(any(test, debug_assertions, feature = "dev-aids"))]
    pub fn parse(text: &str, base: &AirTune) -> AirTune {
        let mut t = *base;
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            let Some((name, value)) = line.split_once('=') else { continue };
            let value = value.trim();
            let auto = value.eq_ignore_ascii_case("auto");
            let num = number(value);
            let moon = |was: Option<f32>, lo: f32, hi: f32| if auto { None } else { num.map(|v| v.clamp(lo, hi)).or(was) };
            match name.trim().to_ascii_uppercase().as_str() {
                "STRENGTH" => t.strength = num.map_or(t.strength, |v| v.clamp(AIR_STRENGTHS.0, AIR_STRENGTHS.1)),
                "SHARE" if auto => t.share = None,
                "SHARE" => t.share = num.map(|v| v.clamp(0.0, 1.0)).or(t.share),
                "G" => t.g = num.map_or(t.g, |v| v.clamp(0.0, 0.95)),
                "BACK" => t.back_g = num.map_or(t.back_g, |v| v.clamp(-0.95, 0.95)),
                "BACKWEIGHT" => t.back_weight = num.map_or(t.back_weight, |v| v.clamp(0.0, 1.0)),
                "CAP" => t.cap = num.map_or(t.cap, |v| v.clamp(1.0, 8.0)),
                "DEPTH" if auto => t.depth = None,
                "DEPTH" => t.depth = num.map(|v| v.clamp(AIR_DEPTHS.0, AIR_DEPTHS.1)).or(t.depth),
                "GRID" if auto => t.grid = None,
                "GRID" => t.grid = debug_grid(Some(value)).or(t.grid),
                "SKY" => t.sky_weight = num.map_or(t.sky_weight, |v| v.clamp(0.0, 1.0)),
                "SKYPHASE" => t.sky_phase = num.map_or(t.sky_phase, |v| v.clamp(0.0, 1.0)),
                "RAYS" => t.rays = num.map_or(t.rays, |v| v.clamp(0.0, 8.0)),
                "FLOOR" => t.floor = num.map_or(t.floor, |v| v.clamp(0.0, 1.0)),
                "REF" if auto => t.reference = None,
                "REF" => t.reference = num.map(|v| v.clamp(0.05, 1.0)).or(t.reference),
                "LIFT" => t.lift = num.map_or(t.lift, |v| v.clamp(1.0, 3.0)),
                "SHADE" => t.shade = num.map_or(t.shade, |v| v.clamp(0.0, 1.0)),
                "SAMPLES" => t.samples = num.map_or(t.samples, |v| v.round().clamp(1.0, 8.0) as u32),
                "FILTER" => t.filter = num.map_or(t.filter, |v| v >= 0.5),
                "MOON_STRENGTH" => t.moon.strength = num.map_or(t.moon.strength, |v| v.clamp(AIR_STRENGTHS.0, AIR_STRENGTHS.1)),
                "MOON_RAYS" => t.moon.rays = moon(t.moon.rays, 0.0, 8.0),
                "MOON_LIFT" => t.moon.lift = moon(t.moon.lift, 1.0, 3.0),
                "MOON_SHADE" => t.moon.shade = moon(t.moon.shade, 0.0, 1.0),
                "MOON_G" => t.moon.g = moon(t.moon.g, 0.0, 0.95),
                "MOON_CAP" => t.moon.cap = moon(t.moon.cap, 1.0, 8.0),
                "MOON_SKY" => t.moon.sky_weight = moon(t.moon.sky_weight, 0.0, 1.0),
                "MOON_SKYPHASE" => t.moon.sky_phase = moon(t.moon.sky_phase, 0.0, 1.0),
                "VIEW" => t.view = AirView::named(value).unwrap_or(t.view),
                "OCCLUDERS" => t.occluders = AirOccluders::named(value).unwrap_or(t.occluders),
                _ => {}
            }
        }
        t
    }
}

/// Whether two tunes differ, for the tune file's poller (re-pack, re-solve the phase, log):
/// field by field, numbers by their bits, so a NaN that slipped in compares equal to itself
/// and does not count as a change every poll [design].
pub fn tune_changed(a: &AirTune, b: &AirTune) -> bool {
    let bits = |t: &AirTune| {
        [t.strength, t.share.unwrap_or(f32::NAN), t.g, t.back_g, t.back_weight, t.cap, t.depth.unwrap_or(f32::NAN), t.sky_weight, t.sky_phase, t.rays, t.floor, t.reference.unwrap_or(f32::NAN), t.lift, t.shade].map(f32::to_bits)
    };
    let moon = |t: &AirTune| t.moon.named().map(|(_, v)| v.map(f32::to_bits));
    bits(a) != bits(b) || moon(a) != moon(b) || a.share.is_some() != b.share.is_some() || a.depth.is_some() != b.depth.is_some() || a.view != b.view || a.occluders != b.occluders || a.grid != b.grid || a.samples != b.samples || a.filter != b.filter || a.reference.is_some() != b.reference.is_some()
}

/// The changed keys of `b` against `a`, as `NAME=value` in the tune file's spelling and
/// order (`SHARE`, `DEPTH`, `GRID` print `auto` when cleared); "nothing" when none [design].
#[cfg(any(test, debug_assertions, feature = "dev-aids"))]
pub fn tune_diff(a: &AirTune, b: &AirTune) -> String {
    let mut parts: Vec<String> = Vec::new();
    let numbers = [
        ("STRENGTH", a.strength, b.strength),
        ("G", a.g, b.g),
        ("BACK", a.back_g, b.back_g),
        ("BACKWEIGHT", a.back_weight, b.back_weight),
        ("CAP", a.cap, b.cap),
        ("SKY", a.sky_weight, b.sky_weight),
        ("SKYPHASE", a.sky_phase, b.sky_phase),
        ("RAYS", a.rays, b.rays),
        ("FLOOR", a.floor, b.floor),
        ("LIFT", a.lift, b.lift),
        ("SHADE", a.shade, b.shade),
    ];
    let optional = |v: Option<f32>| v.map_or("auto".to_string(), |v| v.to_string());
    for (name, x, y) in numbers {
        if x.to_bits() != y.to_bits() {
            parts.push(format!("{name}={y}"));
        }
    }
    if a.share.map(f32::to_bits) != b.share.map(f32::to_bits) {
        parts.push(format!("SHARE={}", optional(b.share)));
    }
    if a.depth.map(f32::to_bits) != b.depth.map(f32::to_bits) {
        parts.push(format!("DEPTH={}", optional(b.depth)));
    }
    if a.grid != b.grid {
        parts.push(format!("GRID={}", b.grid.map_or("auto".to_string(), |[x, y, z]| format!("{x},{y},{z}"))));
    }
    if a.view != b.view {
        parts.push(format!("VIEW={}", b.view.name()));
    }
    if a.occluders != b.occluders {
        parts.push(format!("OCCLUDERS={}", b.occluders.name()));
    }
    if a.reference.map(f32::to_bits) != b.reference.map(f32::to_bits) {
        parts.push(format!("REF={}", optional(b.reference)));
    }
    if a.samples != b.samples {
        parts.push(format!("SAMPLES={}", b.samples));
    }
    if a.filter != b.filter {
        parts.push(format!("FILTER={}", u8::from(b.filter)));
    }
    for ((name, x), (_, y)) in a.moon.named().into_iter().zip(b.moon.named()) {
        if x.map(f32::to_bits) != y.map(f32::to_bits) {
            parts.push(format!("{name}={}", optional(y)));
        }
    }
    if parts.is_empty() { "nothing".to_string() } else { parts.join(" ") }
}

/// Whether the tune file is due a look: never looked at, or [`TUNE_POLL_SECONDS`] of real
/// time since the last look (a clock that went back counts as due) [design].
#[cfg(any(test, debug_assertions, feature = "dev-aids"))]
pub fn poll_due(last: Option<f64>, now: f64) -> bool {
    last.is_none_or(|t| now < t || now - t >= TUNE_POLL_SECONDS)
}

/// Follows the tune file (only with the developer aids, dev_aids.rs; without them the tune is the
/// slider's alone, medium_air_light.rs `slider_tune`): each read's text is parsed over the
/// **start** tune (so removing a line returns that key to its start value); a text seen before
/// changes nothing; a missing or unreadable file is the start tune, said once until the file
/// is read again [design].
#[cfg(any(test, debug_assertions, feature = "dev-aids"))]
#[derive(Clone, Debug)]
pub struct TuneFollower {
    start: AirTune,
    tune: AirTune,
    /// The last read: its text, or `Err` while the file could not be read.
    seen: Option<Result<String, ()>>,
}

#[cfg(any(test, debug_assertions, feature = "dev-aids"))]
impl TuneFollower {
    pub fn new(start: AirTune) -> TuneFollower {
        TuneFollower { start, tune: start, seen: None }
    }

    /// The tune now.
    pub fn tune(&self) -> AirTune {
        self.tune
    }

    /// One read of the file: its text, or the error's words. Returns the log lines to write
    /// (`volumetric medium: light in the air tune: ...`), none when nothing changed.
    pub fn feed(&mut self, read: Result<&str, &str>) -> Vec<String> {
        let mut lines = Vec::new();
        let next = match read {
            Ok(text) => {
                if matches!(&self.seen, Some(Ok(seen)) if seen == text) {
                    return lines;
                }
                self.seen = Some(Ok(text.to_string()));
                AirTune::parse(text, &self.start)
            }
            Err(error) => {
                if matches!(self.seen, Some(Err(()))) {
                    return lines;
                }
                self.seen = Some(Err(()));
                lines.push(format!("volumetric medium: light in the air tune: the file cannot be read ({error}): the start values"));
                self.start
            }
        };
        if tune_changed(&self.tune, &next) {
            lines.push(format!("volumetric medium: light in the air tune: {}", tune_diff(&self.tune, &next)));
        }
        self.tune = next;
        lines
    }

    /// New start values (the settings rows changed while the game runs: the strength's
    /// slider): the last text read is parsed over them again (the file's keys still win), an
    /// unreadable or unread file gives the new start values themselves. Returns the log line
    /// of the change (`volumetric medium: light in the air tune: STRENGTH=<k>`), none when the
    /// start values or the tune did not change [design].
    pub fn restart(&mut self, start: AirTune) -> Vec<String> {
        if !tune_changed(&self.start, &start) {
            return Vec::new();
        }
        self.start = start;
        let next = match &self.seen {
            Some(Ok(text)) => AirTune::parse(text, &self.start),
            _ => self.start,
        };
        let mut lines = Vec::new();
        if tune_changed(&self.tune, &next) {
            lines.push(format!("volumetric medium: light in the air tune: {}", tune_diff(&self.tune, &next)));
        }
        self.tune = next;
        lines
    }
}

// ---------------------------------------------------------------------------------------------
// The gate and the log
// ---------------------------------------------------------------------------------------------

/// Why the light is not drawn (only the outdoor display-value fog under the sun or a moon is
/// handled; everything else is shown the plain way) [design].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AirOff {
    Indoors,
    UnderWater,
    NoSun,
    UnusableFog,
    LinearBlend,
    Night,
    NoStrength,
    NoShare,
}

impl AirOff {
    /// The reason in the log line `volumetric medium: light in the air off: <reason>`.
    pub fn reason(self) -> &'static str {
        match self {
            AirOff::Indoors => "indoors",
            AirOff::UnderWater => "the camera is under water",
            AirOff::NoSun => "no sun light",
            AirOff::UnusableFog => "the weather's fog cannot be read (no fog, far not beyond near, power or strength not above 0)",
            AirOff::LinearBlend => "the fog does not blend in display values",
            AirOff::Night => "night, and the climate has no moon",
            AirOff::NoStrength => "the strength is 0",
            AirOff::NoShare => "the sunlit share is 0",
        }
    }
}

/// What the gate looks at, this frame (main world).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AirGateInput {
    /// Interior cell lighting (or no sky at all).
    pub interior: bool,
    /// The camera is under water.
    pub submerged: bool,
    /// A sun light exists.
    pub sun: bool,
    /// The camera's effective fog.
    pub fog: AirFog,
    /// The fog blends in display values (fog.rs `bi.x > 0.5`).
    pub display_blend: bool,
    /// What the air is lit by ([`air_source`]).
    pub source: AirSource,
    /// The strength k ([`applied_scale`]): at 0 the light changes no pixel, so it is not drawn.
    pub strength: f32,
    /// The effective share a ([`effective_share`]).
    pub share: f32,
}

/// The light's gate, checked in this order: indoors, under water, no sun light, an
/// unusable fog, a blend not in display values, nothing to be lit by (night without a moon), a strength not above 0, a share not
/// above 0 (NaN included in both). Open, it names the source.
pub fn gate(input: &AirGateInput) -> Result<AirSource, AirOff> {
    if input.interior {
        Err(AirOff::Indoors)
    } else if input.submerged {
        Err(AirOff::UnderWater)
    } else if !input.sun {
        Err(AirOff::NoSun)
    } else if !input.fog.usable() {
        Err(AirOff::UnusableFog)
    } else if !input.display_blend {
        Err(AirOff::LinearBlend)
    } else if input.source == AirSource::None {
        Err(AirOff::Night)
    } else if input.strength.is_nan() || input.strength <= 0.0 {
        Err(AirOff::NoStrength)
    } else if input.share.is_nan() || input.share <= 0.0 {
        Err(AirOff::NoShare)
    } else {
        Ok(input.source)
    }
}

/// The length per unit of view depth of the view's longest ray, the corner ray through NDC
/// (1, 1), for a centred perspective with x and y scales `proj_x` and `proj_y` ([`ray_len`]):
/// `sqrt(1 + 1 / proj_x^2 + 1 / proj_y^2)`. A scale not above 0 or not finite, or a length
/// not finite, gives infinity (the bound below then takes the fog's whole strength: still a
/// bound) [design].
pub fn corner_ray_len(proj_x: f32, proj_y: f32) -> f32 {
    let len = ray_len(Vec2::ONE, proj_x, proj_y);
    if proj_x > 0.0 && proj_y > 0.0 && len.is_finite() { len } else { f32::INFINITY }
}

/// The whole view's bounds of the light, in display values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewBounds {
    /// Near pixels the fog hardly covers (t = 0): at most `+near x C'`, `near = (LIFT x most - 1) x a x F`.
    pub near: f32,
    /// Far or fogged pixels (0 < t < 1): at most `+far x mix(C', Y', t)` (their budget), `far` the
    /// largest over t of `(LIFT x mix(most, mix(1, most, SKYPHASE), t) - 1) x mix(1, SKY, t)
    /// x a x F` (the shader's phase and weight at that t).
    pub far: f32,
    /// The sky and anything at t = 1 (terrain the fog covers too): at most `x (1 + sky)` on the pixel's own display value,
    /// `sky = (LIFT x mix(1, most, SKYPHASE) - 1) x a x F x SKY`.
    pub sky: f32,
}

/// The blend's steps over which [`ViewBounds::far`] takes its largest factor (t = 0 and 1
/// included) [design].
const BOUND_T_STEPS: u32 = 64;

/// The whole view's bounds of the light ([`ViewBounds`]), with F the fog share at the grid's
/// end on the view's longest ray (`grid.depth x corner`, [`corner_ray_len`]; F does not fall
/// with distance for any fog, so no pixel's F_near is above it) and `a` the applied scale
/// k x a ([`applied_scale`]: the strength scales every bound; before the deck's share and the
/// bright hold, which only lower it). Vbar_c <= LIFT and p_eff <=
/// most (FLOOR <= 1 <= most); each factor is 0 when it is not above 1 (shadow only takes,
/// never adds). A snapshot: F follows the weather's fog and the view's projection, s the hour
/// [design].
pub fn view_bounds(grid: &AirGrid, phase: &NormedPhase, a: f32, tune: &AirTune, fog: AirFog, corner: f32) -> ViewBounds {
    let f = fog_share(grid.depth * corner, fog);
    let most = phase.most().max(1.0);
    let lift = tune.lift.max(1.0);
    let k = tune.sky_phase.clamp(0.0, 1.0);
    let sky_weight = tune.sky_weight.clamp(0.0, 1.0);
    let far_phase = 1.0 + (most - 1.0) * k;
    // The factor at blend t, as the shader applies the phase and the weight there.
    let at = |t: f32| (lift * (most * (1.0 - t) + far_phase * t) - 1.0).max(0.0) * (1.0 - t + sky_weight * t) * a * f;
    let far = (0..=BOUND_T_STEPS).map(|i| at(i as f32 / BOUND_T_STEPS as f32)).fold(0.0f32, f32::max);
    ViewBounds { near: at(0.0), far, sky: at(1.0) }
}

/// How far a logged bound must move before the "on" line is written again, and how often at
/// most it is looked at, real seconds (a weather fade can
/// bring the fog's ramp into reach while the last line still said 0) [design].
pub const BOUND_RELOG_STEP: f32 = 0.05;
pub const BOUND_RELOG_SECONDS: f64 = 5.0;

/// Whether the logged figures moved by more than [`BOUND_RELOG_STEP`] (each rounded to the
/// line's three places) since the line last written [design].
pub fn bounds_moved(told: &ViewBounds, now: &ViewBounds) -> bool {
    let r = |v: f32| (v * 1000.0).round() / 1000.0;
    [(told.near, now.near), (told.far, now.far), (told.sky, now.sky)].iter().any(|&(a, b)| (r(a) - r(b)).abs() > BOUND_RELOG_STEP)
}

/// The figures the "on" line states ([`view_bounds`] at the applied scale of the tune's
/// strength and the share used).
pub fn on_bounds(grid: &AirGrid, phase: &NormedPhase, s: f32, tune: &AirTune, fog: AirFog, corner: f32) -> ViewBounds {
    view_bounds(grid, phase, applied_scale(tune.strength, effective_share(s, tune.share)), tune, fog, corner)
}

/// The log line when the light first draws, again when a tune change alters its figures
/// (medium_air_light.rs, [`tune_changed`]) and when its bounds moved ([`bounds_moved`], looked
/// at every [`BOUND_RELOG_SECONDS`]):
/// `volumetric medium: light in the air on: grid <x>x<y>x<z> to <d> units, lit by the sun | lit by
/// the moon, phase g <g>, cap <c>, sunlit share | moonlit share <s>, strength <k>, near pixels at most +<near> x the fog colour, far pixels at
/// most +<far> x mix(fog colour, pixel, t), sky at most x<1 + sky> (display values; a snapshot
/// for this weather, hour and view), sky <w>, sky phase <q>, rays <r>, floor <f>, ref <v> (set)
/// | ref auto, lift <l>, shade <h>`; s is the share used (the tune's `SHARE` when set), the
/// bounds [`on_bounds`] on the view's longest ray (`corner`, [`corner_ray_len`]; they do not
/// depend on V_ref), so the strength k scales them all. With `REF=auto` the value is the
/// clouds' mean, known on the render side only, so the line says `ref auto`.
/// (The game computes the bounds once and calls [`on_line_with`]; this form is the tests'.)
#[cfg(test)]
pub fn on_line(grid: &AirGrid, phase: &NormedPhase, s: f32, tune: &AirTune, fog: AirFog, corner: f32) -> String {
    on_line_with(grid, phase, s, tune, AirSource::Sun, &on_bounds(grid, phase, s, tune, fog, corner))
}

/// An "at most" figure rounded **up** to the line's three places (2.75044
/// printed as 2.750 understated it): `ceil(1000 v) / 1000`, less one part in a million of the
/// value first so that the f32 noise of an exact figure (2.75 computed as 2.7500002) does not
/// lift it a step; the result is never below the value by more than that part [design].
pub fn round_up_3(v: f32) -> f32 {
    let x = v * 1000.0;
    (x - x.abs().max(1.0) * 1e-6).ceil() / 1000.0 + 0.0
}

/// [`on_line`] with the bounds already computed (medium_air_light.rs computes them once for
/// the line and for the next comparison); the bounds printed rounded up ([`round_up_3`]). `tune` is the
/// one drawn with under `source` ([`AirTune::lit_by`]).
pub fn on_line_with(grid: &AirGrid, phase: &NormedPhase, s: f32, tune: &AirTune, source: AirSource, b: &ViewBounds) -> String {
    let used = tune.share.unwrap_or(s);
    // With REF=auto the reference is the clouds' mean transmittance, known on the render side
    // only: the render side's line `volumetric medium: light in the air: cloud shadow in use
    // (..., mean transmittance <m>, ...)` (medium_air_grid.rs) is where a reader finds it.
    let reference = tune.reference.map_or("ref auto".to_string(), |r| format!("ref {r} (set)"));
    format!(
        "volumetric medium: light in the air on: grid {}x{}x{} to {:.0} units, {}, phase g {}, cap {}, {} {used:.2}, strength {}, near pixels at most +{:.3} x the fog colour, far pixels at most +{:.3} x mix(fog colour, pixel, t), sky at most x{:.3} (display values; a snapshot for this weather, hour and view), sky {}, sky phase {}, rays {}, floor {}, {reference}, lift {}, shade {}",
        grid.nx,
        grid.ny,
        grid.nz,
        grid.depth,
        source.lit_by(),
        phase.phase.g,
        phase.phase.cap,
        source.share_name(),
        tune.strength,
        round_up_3(b.near),
        round_up_3(b.far),
        round_up_3(1.0 + b.sky),
        tune.sky_weight,
        tune.sky_phase,
        tune.rays,
        tune.floor,
        tune.lift,
        tune.shade
    )
}

/// How long an off reason must hold before it is logged, real seconds: at start the fog
/// shader is patched a moment after the first frames (fog.rs `installed`), which would log
/// "the fog does not blend in display values" and then "on" 0.2 s later [design].
pub const OFF_HOLD_SECONDS: f64 = 0.5;

/// The gate's log: "on" at once when the gate opens; an off reason only once it has held for
/// [`OFF_HOLD_SECONDS`]; nothing while the answer is the one last logged (another source is another
/// answer) [design].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GateLog {
    logged: Option<Result<AirSource, AirOff>>,
    /// An off reason not yet logged and since when it holds.
    pending: Option<(AirOff, f64)>,
}

impl GateLog {
    /// This frame's answer at real time `now`: what to log, if anything.
    pub fn step(&mut self, answer: Result<AirSource, AirOff>, now: f64) -> Option<Result<AirSource, AirOff>> {
        match answer {
            Ok(source) => {
                self.pending = None;
                (self.logged != Some(Ok(source))).then(|| {
                    self.logged = Some(Ok(source));
                    Ok(source)
                })
            }
            Err(off) => {
                if self.logged == Some(Err(off)) {
                    self.pending = None;
                    return None;
                }
                let since = match self.pending {
                    Some((p, t)) if p == off => t,
                    _ => {
                        self.pending = Some((off, now));
                        now
                    }
                };
                (now - since >= OFF_HOLD_SECONDS).then(|| {
                    self.logged = Some(Err(off));
                    self.pending = None;
                    Err(off)
                })
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The uniform's packing
// ---------------------------------------------------------------------------------------------

/// The grid pass's and the composite's uniform, the groups this file owns (the GPU half
/// derives `ShaderType` and adds matrices and the rest).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AirUniformData {
    /// nx, ny, nz, depth (view depth, game units).
    pub grid: [f32; 4],
    /// The slice curve: first (the thickness when linear), r, ln r (0: linear); w: RAYS
    /// ([`relative_lit`]).
    pub curve: [f32; 4],
    /// The fog: near, far, power, strength (fog.rs `GAME_FOG`).
    pub fog: [f32; 4],
    /// The phase: g, back g, back weight, the inner cap ([`AirPhase::normed`]'s solved cap,
    /// applied before the norm).
    pub phase: [f32; 4],
    /// The phase's norm, its largest value, the applied scale k x a ([`applied_scale`]: the
    /// strength k, 0..=2, times the effective share a = clamp(s, 0, 1), [`effective_share`];
    /// the composite multiplies the whole correction by it before the floor), the view's code.
    pub phase2: [f32; 4],
}

/// Packs the groups: `sunlit` is the scaled share s ([`scaled_share`]), replaced by the tune's
/// `share` when set; `phase2.z` is the applied scale k x a ([`applied_scale`]). The scale shares the share's slot: the composite only ever
/// used the product, so the shader's arithmetic is unchanged and no other field moved
/// [design].
pub fn pack(grid: &AirGrid, fog: AirFog, phase: &NormedPhase, tune: &AirTune, sunlit: f32) -> AirUniformData {
    let curve = grid.curve();
    let a = applied_scale(tune.strength, effective_share(sunlit, tune.share));
    AirUniformData {
        grid: [grid.nx as f32, grid.ny as f32, grid.nz as f32, grid.depth],
        curve: [curve.first, curve.r, curve.ln_r, tune.rays],
        fog: [fog.near, fog.far, fog.power, fog.strength],
        phase: [phase.phase.g, phase.phase.back_g, phase.phase.back_weight, phase.inner_cap],
        phase2: [phase.norm, phase.most(), a, tune.view.code() as f32],
    }
}

// ---------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::Quat;

    fn grid_at(depth: f32) -> AirGrid {
        AirGrid::for_view(16.0 / 9.0, None, Some(depth))
    }

    /// The commonest Mojave fog: NVWastelandClear's day values (task table).
    const CLEAR: AirFog = AirFog { near: -10.0, far: 200_000.0, power: 0.6, strength: 1.0 };
    const HAZY: AirFog = AirFog { near: 1_800.0, far: 30_000.0, power: 0.6, strength: 1.0 };

    /// The mean over the sphere of f(cos): half the integral over [-1, 1], midpoint rule.
    fn sphere_mean(steps: usize, f: impl Fn(f32) -> f64) -> f64 {
        let h = 2.0 / steps as f64;
        (0..steps).map(|i| f((-1.0 + (i as f64 + 0.5) * h) as f32)).sum::<f64>() / steps as f64
    }

    #[test]
    fn air_grid_follows_the_aspect() {
        let g = AirGrid::for_view(16.0 / 9.0, None, None);
        assert_eq!((g.nx, g.ny, g.nz, g.depth, g.first), (160, 88, 64, AIR_DEPTH, AIR_FIRST));
        // 88 x 7/3 = 205.3 -> 26 x 8; 88 x 4/3 = 117.3 -> 15 x 8; very wide held to 320.
        assert_eq!(AirGrid::for_view(21.0 / 9.0, None, None).nx, 208);
        assert_eq!(AirGrid::for_view(4.0 / 3.0, None, None).nx, 120);
        assert_eq!(AirGrid::for_view(48.0 / 9.0, None, None).nx, 320);
        assert_eq!(AirGrid::for_view(0.2, None, None).nx, 64);
        assert_eq!(AirGrid::for_view(f32::NAN, None, None).nx, 160);
        assert_eq!(AirGrid::for_view(1.0, None, Some(500.0)).depth, 1_000.0);
        assert_eq!(AirGrid::for_view(1.0, None, Some(1e9)).depth, 200_000.0);
        assert_eq!(AirGrid::for_view(1.0, None, Some(f32::NAN)).depth, AIR_DEPTH);
        let asked = AirGrid::for_view(1.0, Some([1_000, 2, 1_000]), None);
        assert_eq!((asked.nx, asked.ny, asked.nz), (320, 8, 128));
        assert_eq!(g.texels(), 160 * 88 * 64);
        assert_eq!(g.bytes(8), 160 * 88 * 64 * 8);
    }

    #[test]
    fn air_grid_curve_spans_the_depth() {
        for depth in [12_000.0f32, 48_000.0, 2_000.0, 3_300.0] {
            let g = grid_at(depth);
            let rel = |a: f32, b: f32| (a - b).abs() / b.abs().max(1e-6);
            assert_eq!(g.slice_near(0), 0.0, "{depth}");
            assert!(rel(g.slice_near(g.nz), depth) < 1e-3, "{depth}: {}", g.slice_near(g.nz));
            for k in 0..g.nz {
                assert!(g.slice_near(k + 1) > g.slice_near(k), "{depth} slice {k} not growing");
                assert!(g.slice_mid(k) > g.slice_near(k) && g.slice_mid(k) < g.slice_near(k + 1));
                assert!((g.slice_coord(g.slice_near(k)) - k as f32).abs() < 1e-3, "{depth} slice {k}");
                assert!((g.slice_coord(g.slice_mid(k)) - (k as f32 + 0.5)).abs() < 1e-3);
            }
            assert_eq!(g.slice_coord(-5.0), 0.0);
            assert_eq!(g.slice_coord(depth * 2.0), g.nz as f32);
        }
        // Exponential at 12,000 and 48,000: the first slice is exactly `first`.
        for depth in [12_000.0f32, 48_000.0] {
            let g = grid_at(depth);
            assert!(g.ratio() > 1.0);
            assert!((g.slice_near(1) - AIR_FIRST).abs() / AIR_FIRST < 1e-3, "{}", g.slice_near(1));
        }
        // Linear at 2,000 (<= 50 x 64): thickness 2,000 / 64 = 31.25.
        let lin = grid_at(2_000.0);
        assert_eq!(lin.ratio(), 1.0);
        assert!((lin.slice_near(1) - 31.25).abs() < 1e-3);
        // 3,300 solves to r = 1.00097 (< 1 + 1e-3): drawn linear, first slice 51.56 >= 50.
        let near_one = grid_at(3_300.0);
        assert_eq!(near_one.ratio(), 1.0);
        assert!(near_one.slice_near(1) >= AIR_FIRST);
    }

    #[test]
    fn air_grid_curve_figures() {
        // Prototype figures (f64, same bisection): r 1.036066 / last slice 466.0 at 12,000;
        // r 1.067636 / 3,087.7 at 48,000.
        for depth in [12_000.0f32, 48_000.0] {
            let g = grid_at(depth);
            let r = g.ratio();
            let last = g.slice_near(g.nz) - g.slice_near(g.nz - 1);
            println!("air grid to {depth}: r {r:.6}, last slice {last:.1} units, first {:.2}", g.slice_near(1));
            assert!(r.is_finite() && last.is_finite());
        }
    }

    #[test]
    fn froxel_geometry_round_trips() {
        let g = grid_at(12_000.0);
        // A 16:9 view with a vertical field of 1 rad: proj_y = 1 / tan(0.5).
        let proj_y = 1.0 / 0.5f32.tan();
        let proj_x = proj_y * 9.0 / 16.0;
        // A modest translation: the f32 inverse's error over the first slices (25 units out)
        // grows with the camera's distance from the origin.
        let world_from_view = Mat4::from_rotation_translation(Quat::from_rotation_y(0.7) * Quat::from_rotation_x(-0.2) * Quat::from_rotation_z(0.1), Vec3::new(100.0, 25.0, -300.0));
        for (i, j) in [(0, 0), (g.nx - 1, g.ny - 1), (0, g.ny - 1), (g.nx / 2, g.ny / 2)] {
            for k in [0.5f32, 10.5, 40.5, 63.5] {
                let coord = Vec3::new(i as f32 + 0.5, j as f32 + 0.5, k);
                let world = froxel_to_world(&g, coord, proj_x, proj_y, world_from_view);
                let back = world_to_froxel(&g, world, proj_x, proj_y, world_from_view).expect("in front");
                assert!((back - coord).abs().max_element() < 2e-3, "{coord} -> {world} -> {back}");
                // The distance from the camera is view depth x ray_len.
                let ndc = column_ndc(&g, i, j);
                let dist = (world - world_from_view.transform_point3(Vec3::ZERO)).length();
                let expect = g.curve().depth_at(k) * ray_len(ndc, proj_x, proj_y);
                assert!((dist - expect).abs() / expect < 1e-4, "{dist} {expect}");
            }
        }
        // Behind the camera: none.
        let behind = world_from_view.transform_point3(Vec3::new(0.0, 0.0, 100.0));
        assert_eq!(world_to_froxel(&g, behind, proj_x, proj_y, world_from_view), None);
        assert_eq!(ray_len(Vec2::ZERO, proj_x, proj_y), 1.0);
        assert_eq!(column_ndc(&g, 0, 0), Vec2::new(-1.0 + 1.0 / 160.0, 1.0 - 1.0 / 88.0));
    }

    #[test]
    fn fog_share_mirrors_the_game_fog() {
        // Hand figures: (3,010 / 200,010)^0.6 = 0.0806; (12,010 / 200,010)^0.6 = 0.1850;
        // (50,010 / 200,010)^0.6 = 0.4353; (10,200 / 28,200)^0.6 = 0.5433.
        assert!((fog_share(3_000.0, CLEAR) - 0.0806).abs() < 1e-3);
        assert!((fog_share(12_000.0, CLEAR) - 0.1850).abs() < 1e-3);
        assert!((fog_share(50_000.0, CLEAR) - 0.4353).abs() < 1e-3);
        assert!((fog_share(12_000.0, HAZY) - 0.5433).abs() < 1e-3);
        assert_eq!(fog_share(1_000.0, HAZY), 0.0);
        assert_eq!(fog_share(40_000.0, HAZY), 1.0);
        // The camera's own share when near is below 0: (10 / 200,010)^0.6 = 0.00263.
        assert!((fog_share(0.0, CLEAR) - 0.00263).abs() < 2e-5);
        assert!(CLEAR.usable() && HAZY.usable());
    }

    #[test]
    fn fog_share_survives_extreme_weathers() {
        let fogs = [
            AirFog { near: 0.0, far: 0.0, power: 0.6, strength: 1.0 },
            AirFog { near: -10.0, far: 1e6, power: 0.6, strength: 1.0 },
            AirFog { near: 100.0, far: 20_000.0, power: 0.05, strength: 1.0 },
            AirFog { near: 100.0, far: 20_000.0, power: 8.0, strength: 1.0 },
            AirFog { near: f32::NAN, far: 20_000.0, power: 0.6, strength: 1.0 },
            AirFog { near: 100.0, far: 20_000.0, power: f32::NAN, strength: 1.0 },
            AirFog { near: 100.0, far: 20_000.0, power: 0.6, strength: 0.3 },
            AirFog { near: -1e6, far: 1e6, power: 0.6, strength: 1.0 },
        ];
        for fog in fogs {
            let mut last = 0.0f32;
            for i in 0..=400 {
                let d = i as f32 * 500.0;
                let s = fog_share(d, fog);
                assert!(s.is_finite() && (0.0..=1.0).contains(&s), "{fog:?} at {d}: {s}");
                assert!(s >= last, "{fog:?} not monotone at {d}");
                last = s;
            }
            assert_eq!(fog_share(f32::NAN, fog), 0.0);
        }
        assert!(!AirFog { near: 0.0, far: 0.0, power: 0.6, strength: 1.0 }.usable());
        assert!(!AirFog { near: 10.0, far: 5.0, power: 0.6, strength: 1.0 }.usable());
        assert!(!AirFog { near: 0.0, far: 5.0, power: 0.0, strength: 1.0 }.usable());
        assert!(!AirFog { near: 0.0, far: 5.0, power: 0.6, strength: 0.0 }.usable());
        assert!(!AirFog { near: f32::NAN, far: 5.0, power: 0.6, strength: 1.0 }.usable());
        assert!(!AirFog { near: 0.0, far: f32::INFINITY, power: 0.6, strength: 1.0 }.usable());
        assert!(AirFog { near: -10.0, far: 1e6, power: 0.05, strength: 0.3 }.usable());
    }

    #[test]
    fn phase_has_mean_one_over_the_sphere() {
        let phases = [
            AirPhase::default(),
            AirPhase { g: 0.9, cap: 4.0, ..AirPhase::default() },
            AirPhase { cap: 100.0, ..AirPhase::default() },
        ];
        for p in phases {
            let n = p.normed();
            // A finer rule than the norm's own (200,000 steps against 8,192); the kink at the
            // cap costs the midpoint rule O(h^2), far below 1e-4.
            let mean = sphere_mean(200_000, |c| n.phase(c) as f64);
            assert!((mean - 1.0).abs() < 1e-4, "{p:?}: mean {mean}");
            // The final peak is the cap (or the uncapped peak when that is lower).
            assert!(n.most() <= p.cap + 1e-5, "{p:?}: most {}", n.most());
            assert!(n.most() >= n.phase(0.3));
        }
        let d = AirPhase::default();
        assert_eq!((d.g, d.back_g, d.back_weight, d.cap), (0.3, -0.3, 0.0, 1.5));
        // The cap bites for every cap below the uncapped peak's share 2.653 (g 0.3: peak
        // (1 + g) / (1 - g)^2, mean 1): the final peak equals it.
        for cap in [1.2f32, 1.5, 2.5] {
            let p = AirPhase { cap, ..d };
            assert!((p.most() - cap).abs() < 1e-5, "cap {cap}: most {}", p.most());
        }
        // Closed form for one lobe (w = 0): with u(x) = 1 + g^2 - 2 g x,
        // integral of (1 - g^2) u^-1.5 dx from -1 to x0 = (1 - g^2) / g x (u(x0)^-0.5 - 1 / (1 + g)),
        // and HG(x0) = c at u(x0) = ((1 - g^2) / c)^(2/3); mean(min(HG, c)) = (that integral +
        // c (1 - x0)) / 2. (x0 = 1 gives (1 - g^2) / g x 2g / (1 - g^2) / 2 = 1: the lobe's
        // mean.) Solving c / mean(c) = cap by bisection gives the inner cap and norm 1 / mean:
        // for g 0.3 and cap 1.5, c = 1.29532, norm 1.15802 (cap 1.2: 0.85478, 1.40386; cap
        // 2.5: 2.49676, 1.00130).
        let closed = |g: f64, cap: f64| {
            let mean = |c: f64| {
                let x0 = ((1.0 + g * g - ((1.0 - g * g) / c).powf(2.0 / 3.0)) / (2.0 * g)).clamp(-1.0, 1.0);
                let integral = (1.0 - g * g) / g * ((1.0 + g * g - 2.0 * g * x0).powf(-0.5) - 1.0 / (1.0 + g));
                0.5 * (integral + c * (1.0 - x0))
            };
            let (mut lo, mut hi) = (lobe(g, -1.0), lobe(g, 1.0));
            for _ in 0..200 {
                let mid = 0.5 * (lo + hi);
                if mid / mean(mid) < cap { lo = mid } else { hi = mid }
            }
            let c = 0.5 * (lo + hi);
            (c, 1.0 / mean(c))
        };
        for cap in [1.2f32, 1.5, 2.5] {
            let n = AirPhase { cap, ..d }.normed();
            let (c, norm) = closed(0.3, cap as f64);
            assert!((n.inner_cap as f64 - c).abs() / c < 1e-4, "cap {cap}: inner {} vs {c}", n.inner_cap);
            // The asked phase is kept as it was (a cache comparing it does not re-solve).
            assert_eq!(n.phase, AirPhase { cap, ..d });
            assert!((n.norm as f64 - norm).abs() / norm < 1e-4, "cap {cap}: norm {} vs {norm}", n.norm);
        }
        let n = d.normed();
        assert!((n.inner_cap - 1.29532).abs() < 1e-3 && (n.norm - 1.15802).abs() < 1e-3, "{n:?}");
        // A cap above the uncapped peak caps nothing: the inner cap stays the asked one and
        // the norm is the lobe's own (mean 1: norm 1).
        let loose = AirPhase { cap: 100.0, ..d }.normed();
        assert_eq!(loose.inner_cap, 100.0);
        assert!((loose.norm - 1.0).abs() < 1e-4);
        assert!((loose.most() - 1.3 / 0.49).abs() < 1e-3, "{}", loose.most());
        // A cap of 1 (or below): the constant 1.
        for cap in [1.0f32, 0.5] {
            let flat = AirPhase { cap, ..d }.normed();
            for c in [-1.0f32, -0.3, 0.0, 0.5, 1.0] {
                assert!((flat.phase(c) - 1.0).abs() < 1e-5, "cap {cap} at {c}: {}", flat.phase(c));
            }
        }
        // A NaN cap caps nothing; a NaN g gives norm 1 (no panic, finite inner cap).
        assert!((AirPhase { cap: f32::NAN, ..d }.most() - 1.3 / 0.49).abs() < 1e-3);
        assert_eq!(AirPhase { g: f32::NAN, ..d }.norm(), 1.0);
        let iso = AirPhase { g: 0.0, back_g: 0.0, back_weight: 0.15, cap: 2.5 };
        for c in [-1.0f32, -0.3, 0.0, 0.5, 1.0] {
            assert!((iso.phase(c) - 1.0).abs() < 1e-6, "{c}: {}", iso.phase(c));
        }
    }

    #[test]
    fn debug_aids_parse() {
        assert_eq!(debug_strength(None), 0.0);
        assert_eq!(debug_strength(Some("1.5")), 1.5);
        assert_eq!(debug_strength(Some(" 9 ")), 2.0);
        assert_eq!(debug_strength(Some("-1")), 0.0);
        assert_eq!(debug_strength(Some("NaN")), 0.0);
        assert_eq!(debug_strength(Some("inf")), 0.0);
        assert_eq!(debug_strength(Some("bright")), 0.0);
        assert_eq!(debug_view(Some("Visibility")), AirView::Visibility);
        assert_eq!(debug_view(Some("slice")), AirView::Slice);
        assert_eq!(debug_view(Some("share")), AirView::Share);
        assert_eq!(debug_view(Some("delta")), AirView::Delta);
        assert_eq!(debug_view(Some(" off")), AirView::Off);
        assert_eq!(debug_view(Some("Lit")), AirView::Lit);
        assert_eq!((AirView::Lit.code(), AirView::Lit.name()), (6, "lit"));
        assert_eq!(debug_view(Some("rays")), AirView::Rays);
        assert_eq!((AirView::Rays.code(), AirView::Rays.name()), (7, "rays"));
        assert_eq!(debug_view(Some("glow")), AirView::None);
        assert_eq!(debug_view(None), AirView::None);
        let codes: Vec<u32> = [AirView::None, AirView::Visibility, AirView::Slice, AirView::Share, AirView::Delta, AirView::Off].iter().map(|v| v.code()).collect();
        assert_eq!(codes, vec![0, 1, 2, 3, 4, 5]);
        assert_eq!(debug_grid(Some("160, 88,64")), Some([160, 88, 64]));
        assert_eq!(debug_grid(Some("160,88")), None);
        assert_eq!(debug_grid(Some("160,88,64,2")), None);
        assert_eq!(debug_grid(Some("a,b,c")), None);
        assert_eq!(debug_grid(None), None);
        assert_eq!(debug_depth(Some("48000")), Some(48_000.0));
        assert_eq!(debug_depth(Some("0")), None);
        assert_eq!(debug_depth(Some("NaN")), None);
        assert_eq!(debug_depth(None), None);
        assert_eq!(debug_occluders(Some("Cascades")), AirOccluders::Cascades);
        assert_eq!(debug_occluders(Some("terrain")), AirOccluders::Terrain);
        assert_eq!(debug_occluders(Some("none")), AirOccluders::None);
        assert_eq!(debug_occluders(Some("Clouds")), AirOccluders::Clouds);
        assert_eq!((AirOccluders::Clouds.code(), AirOccluders::Clouds.name()), (4, "clouds"));
        assert_eq!(debug_occluders(Some("glow")), AirOccluders::All);
        assert_eq!(AirTune::parse("OCCLUDERS=clouds", &AirTune::default()).occluders, AirOccluders::Clouds);
        assert_eq!(debug_occluders(None), AirOccluders::All);
    }

    /// The look the hand figures of the tests below are worked for (the defaults of 2026-11-20).
    fn hand(t: AirTune) -> AirTune {
        AirTune { g: 0.6, cap: 2.5, sky_weight: 0.5, rays: 6.0, lift: 1.5, shade: 0.9, ..t }
    }

    #[test]
    fn tune_file_parses() {
        let base = AirTune::start(1.0, AirView::Slice, AirOccluders::Terrain, Some(48_000.0));
        assert_eq!((base.strength, base.view, base.occluders, base.depth), (1.0, AirView::Slice, AirOccluders::Terrain, Some(48_000.0)));
        // The Natural look (2026-11-24): sky weight 0.25, sky phase 0.
        assert_eq!((base.sky_weight, base.sky_phase), (0.25, 0.0));
        // The start tune's phase: g 0.5, cap 1.5, no back lobe.
        assert_eq!(base.phase(), AirPhase { g: 0.5, back_g: -0.3, back_weight: 0.0, cap: 1.5 });
        // Empty text, comments and blank lines keep the base.
        assert_eq!(AirTune::parse("", &base), base);
        assert_eq!(AirTune::parse("# nothing\n\n   \n", &base), base);
        let text = "strength = 1.5  # brighter\nShare=0.4\nG=0.7\nBACK=-0.2\nbackweight=0.1\ncap=3\nDEPTH=12000\nsky=0.5\nview=delta\noccluders=cascades\n";
        let t = AirTune::parse(text, &base);
        assert_eq!(t.strength, 1.5);
        assert_eq!(t.share, Some(0.4));
        assert_eq!((t.g, t.back_g, t.back_weight, t.cap), (0.7, -0.2, 0.1, 3.0));
        assert_eq!(t.depth, Some(12_000.0));
        assert_eq!(t.sky_weight, 0.5);
        assert_eq!((t.view, t.occluders), (AirView::Delta, AirOccluders::Cascades));
        // Held to ranges.
        let held = AirTune::parse("STRENGTH=10\nSHARE=2\nG=1\nBACK=-3\nBACKWEIGHT=-1\nCAP=0\nDEPTH=1\nSKY=9\n", &base);
        // The strength is held to 0..=2 (the slider's 0 to 200 %).
        assert_eq!(held.strength, 2.0);
        assert_eq!(held.share, Some(1.0));
        assert_eq!((held.g, held.back_g, held.back_weight, held.cap), (0.95, -0.95, 0.0, 1.0));
        assert_eq!(held.depth, Some(1_000.0));
        assert_eq!(held.sky_weight, 1.0);
        // The ranges: G 0..=0.95 (a forward lobe), CAP 1..=8, SKY 0..=1.
        let wide = AirTune::parse("G=-0.5\nCAP=100\nSKY=-1", &base);
        assert_eq!((wide.g, wide.cap, wide.sky_weight), (0.0, 8.0, 0.0));
        // The keys that did nothing are gone: ignored like any unknown name.
        assert_eq!(AirTune::parse("SKYDEGREES=30\nFADE=0.9", &base), base);
        // Unknown names, unreadable, non-finite and empty values, lines with no `=`: ignored.
        let ignored = AirTune::parse("COLOUR=3\nSTRENGTH=abc\nG=NaN\nCAP=inf\nSKY=\nVIEW=glow\nOCCLUDERS=sky\njust words\n=5\n", &base);
        assert_eq!(ignored, base);
        // `auto` clears the overrides.
        let with = AirTune::parse("SHARE=0.3", &base);
        assert_eq!(AirTune::parse("share=auto\ndepth=AUTO", &with).share, None);
        assert_eq!(AirTune::parse("depth=auto", &base).depth, None);
        // VIEW=none returns to no view.
        assert_eq!(AirTune::parse("VIEW=none", &base).view, AirView::None);
        // Later lines win.
        assert_eq!(AirTune::parse("G=0.2\nG=0.3", &base).g, 0.3);
    }

    #[test]
    fn uniform_packs_the_groups() {
        let g = grid_at(12_000.0);
        let tune = hand(AirTune::parse("STRENGTH=2\nVIEW=share", &AirTune::default()));
        let phase = tune.phase().normed();
        let u = pack(&g, CLEAR, &phase, &tune, 0.6);
        assert_eq!(u.grid, [160.0, 88.0, 64.0, 12_000.0]);
        let curve = g.curve();
        // curve.w is RAYS.
        assert_eq!(u.curve, [AIR_FIRST, curve.r, curve.ln_r, 6.0]);
        assert!((u.curve[2] - u.curve[1].ln()).abs() < 1e-6);
        assert_eq!(u.fog, [-10.0, 200_000.0, 0.6, 1.0]);
        // The phase (g 0.6, final cap 2.5) with its solved inner cap: the closed
        // form of phase_has_mean_one_over_the_sphere gives inner 1.62387, norm 1.53953 (below
        // the final 2.5 because the norm is above 1).
        assert_eq!(u.phase, [0.6, -0.3, 0.0, phase.inner_cap]);
        assert!(phase.inner_cap < 2.5 && (phase.inner_cap - 1.62387).abs() < 1e-3, "{}", phase.inner_cap);
        assert!((phase.norm - 1.53953).abs() < 1e-3, "{}", phase.norm);
        assert!((u.phase2[1] - 2.5).abs() < 1e-5);
        assert_eq!(u.phase2[0], phase.norm);
        assert_eq!(u.phase2[1], phase.most());
        // The applied scale k x a: strength 2 x clamp(s 0.6, 0, 1) = 1.2 (no longer held to 1).
        assert!((u.phase2[2] - 1.2).abs() < 1e-6, "{}", u.phase2[2]);
        assert!((pack(&g, CLEAR, &phase, &AirTune { strength: 1.0, ..tune }, 0.6).phase2[2] - 0.6).abs() < 1e-6);
        assert_eq!(pack(&g, CLEAR, &phase, &AirTune { strength: 0.0, ..tune }, 0.6).phase2[2], 0.0);
        assert_eq!(u.phase2[3], 3.0);
        // The tune's share wins over the computed one: 2 x 0.25.
        let over = AirTune { share: Some(0.25), ..tune };
        assert!((pack(&g, CLEAR, &phase, &over, 0.6).phase2[2] - 0.5).abs() < 1e-6);
        // A linear curve packs r 1, ln r 0 and the thickness as `first`.
        let lin = pack(&grid_at(2_000.0), CLEAR, &phase, &tune, 0.6);
        assert_eq!((lin.curve[0], lin.curve[1], lin.curve[2]), (31.25, 1.0, 0.0));
    }

    /// The visibility patterns the bound tests walk: alternating, a near shadow, a far
    /// shadow, a ramp of fractions.
    fn pattern(which: usize, k: usize) -> f32 {
        match which {
            0 => (k % 2) as f32,
            1 => if k < 20 { 0.0 } else { 1.0 },
            2 => if k < 20 { 1.0 } else { 0.0 },
            _ => (k as f32 * 0.37).fract(),
        }
    }

    #[test]
    fn column_nm_stores_bounded_sums() {
        for (fog, depth, rl) in [(CLEAR, 12_000.0f32, 1.0f32), (HAZY, 12_000.0, 1.13), (CLEAR, 48_000.0, 1.4), (HAZY, 48_000.0, 1.0)] {
            let g = grid_at(depth);
            let lit = column_nm(&g, rl, fog, &|_| 1.0);
            assert_eq!(lit.len(), g.nz as usize);
            // M telescopes to F at every boundary (slice 0 from 0; usable fogs do not fall
            // with distance, so every step is the plain difference). An f32 sum of 64
            // differences of values in 0..=1 errs by under 64 x 2^-24 = 3.8e-6.
            for (k, s) in lit.iter().enumerate() {
                let at = fog_share(g.slice_near(k as u32 + 1) * rl, fog);
                assert!((s[1] - at).abs() < 1e-5, "{fog:?} {depth} boundary {}: {} vs {at}", k + 1, s[1]);
            }
            assert!((lit[g.nz as usize - 1][1] - fog_share(g.depth * rl, fog)).abs() < 1e-5);
            // V = 1: N = M exactly (df x 1 is df, the same additions): no phase in the sums.
            assert!(lit.iter().all(|s| s[0] == s[1]));
            // V = 0: N = 0 exactly, M unchanged.
            let dark = column_nm(&g, rl, fog, &|_| 0.0);
            assert!(dark.iter().zip(&lit).all(|(d, l)| d[0] == 0.0 && d[1] == l[1]));
            // Any pattern: M never falls, 0 <= N <= M (each term df x V <= df; a sum of terms
            // each no larger than M's rounds no larger, the f32 slack covers the order).
            for which in 0..4 {
                let mut last = 0.0f32;
                for s in column_nm(&g, rl, fog, &|k| pattern(which, k)) {
                    assert!(s[1] >= last && s[0] >= 0.0, "{which}: {s:?}");
                    assert!(s[0] <= s[1] * (1.0 + 1e-5) + 1e-7, "{which}: {s:?}");
                    last = s[1];
                }
            }
        }
        // V outside 0..1 or NaN is held: NaN as 0, 7 as 1, so N <= M.
        let g = grid_at(12_000.0);
        let held = column_nm(&g, 1.0, CLEAR, &|k| if k.is_multiple_of(2) { f32::NAN } else { 7.0 });
        assert!(held.iter().all(|s| s[0].is_finite() && s[0] <= s[1]));
    }

    #[test]
    fn sums_at_reads_the_boundaries() {
        let nm = vec![[0.1f32, 0.2], [0.15, 0.4], [0.3, 0.5], [0.3, 0.9]];
        // Texel k holds boundary k + 1: exact there.
        for (k, s) in nm.iter().enumerate() {
            assert_eq!(sums_at(&nm, (k + 1) as f32), *s);
        }
        // The zero boundary at the camera; NaN and below 0 also 0; nothing stored, 0.
        assert_eq!(sums_at(&nm, 0.0), [0.0, 0.0]);
        assert_eq!(sums_at(&nm, -3.0), [0.0, 0.0]);
        assert_eq!(sums_at(&nm, f32::NAN), [0.0, 0.0]);
        assert_eq!(sums_at(&[], 2.0), [0.0, 0.0]);
        // Inside the first slice texel 0 scales with c (halving a float is exact).
        assert_eq!(sums_at(&nm, 0.5), [0.05, 0.1]);
        // Halfway between boundaries 2 and 3: the mean of texels 1 and 2.
        let mid = sums_at(&nm, 2.5);
        assert!((mid[0] - 0.225).abs() < 1e-7 && (mid[1] - 0.45).abs() < 1e-7, "{mid:?}");
        // At and beyond nz the last texel is carried.
        for c in [4.0f32, 4.5, 1e9, f32::INFINITY] {
            assert_eq!(sums_at(&nm, c), [0.3, 0.9]);
        }
        // Continuous at boundary 1 from below; M never falls along c.
        let below = sums_at(&nm, 1.0 - 1e-6);
        assert!((below[1] - 0.2).abs() < 1e-6);
        let mut last = 0.0f32;
        for i in 0..=500 {
            let m = sums_at(&nm, i as f32 * 0.01)[1];
            assert!(m >= last, "c {}: {m} < {last}", i as f32 * 0.01);
            last = m;
        }
    }

    #[test]
    fn correction_and_near_share_are_bounded() {
        // The bound for the world rule (the one form at t = 0): with r = Vbar_c x p_eff in
        // 0..=most and a in 0..=1, -F_near <= a F_near (r - 1) <= (most - 1) F_near. Exact in
        // f32: r - 1 >= -1 and <= most - 1 under monotone rounding, (a x f) <= f, and a product
        // with the same factor rounds monotonically, so no slack is needed.
        let most = AirPhase::default().most();
        for f_near in [0.0f32, 0.05, 0.185, 0.77, 1.0] {
            for i in 0..=30 {
                let r = (i as f32 * 0.1).min(most);
                for a in [0.0f32, 0.35, 1.0] {
                    let c = correction(f_near, r, a);
                    assert!(-f_near <= c && c <= (most - 1.0) * f_near, "f {f_near} r {r} a {a}: {c}");
                }
            }
        }
        assert_eq!(correction(0.4, 1.0, 1.0), 0.0);
        assert_eq!(correction(0.4, 0.0, 1.0), -0.4);
        // F_near: the analytic share held to the grid's end.
        assert_eq!(near_share(3_000.0, 12_000.0, CLEAR), fog_share(3_000.0, CLEAR));
        assert_eq!(near_share(50_000.0, 12_000.0, CLEAR), fog_share(12_000.0, CLEAR));
        assert_eq!(near_share(f32::NAN, 12_000.0, CLEAR), fog_share(12_000.0, CLEAR));
    }

    #[test]
    fn compose_ratio_keeps_the_fogged_floor() {
        // The fog drew Y' = (1 - f) S' + f C' (display values, fog.rs GAME_FOG). The
        // correction adds a f_near (r - 1) C' with a in 0..=1, r in 0..=most, f_near <= f:
        // since r - 1 >= -1 the term is >= -a f_near C' >= -f C' (C' >= 0), so the result is
        // >= (1 - f) S' per channel. f32 slack, in display values: the pixel passes Y' ->
        // ^2.2 -> ^(1/2.2) -> + corr C' -> ^2.2 -> ^(1/2.2) here, each power within a few
        // ulp (2^-24 relative each), under 2e-6 x max(1, Y') in all; 1e-5 x max(1, Y') leaves
        // five times that.
        let most = AirPhase::default().most();
        let gamma = |v: f32| v.max(0.0).powf(1.0 / 2.2);
        let colours = [Vec3::new(0.8, 0.7, 0.6), Vec3::new(0.05, 0.04, 0.03), Vec3::new(1.6, 1.2, 0.9), Vec3::ZERO];
        for c in colours {
            let cd = c.max(Vec3::ZERO).powf(1.0 / 2.2);
            for s in [0.0f32, 0.02, 0.5, 4.0] {
                for f in [0.05f32, 0.185, 0.77, 1.0] {
                    let yd = Vec3::splat((1.0 - f) * gamma(s)) + cd * f;
                    let pixel = yd.powf(2.2);
                    let floor = (1.0 - f) * gamma(s);
                    let slack = 1e-5 * yd.max_element().max(1.0);
                    for a in [0.0f32, 0.35, 1.0] {
                        for r in [0.0f32, 0.5, 1.0, most] {
                            for f_near in [0.0, 0.5 * f, f] {
                                let out = compose_ratio(pixel, correction(f_near, r, a), c);
                                assert!(out.is_finite() && out.min_element() >= 0.0, "{out}");
                                let shown = out.powf(1.0 / 2.2);
                                assert!(shown.min_element() >= floor - slack, "c {c} S {s} f {f} a {a} r {r} near {f_near}: {shown} < {floor}");
                            }
                        }
                    }
                    // Full shadow (r 0, a 1, f_near f): corr is exactly -f, so the display
                    // value is Y' - f C' = (1 - f) S' up to the slack above.
                    let dark = compose_ratio(pixel, correction(f, 0.0, 1.0), c).powf(1.0 / 2.2);
                    assert!((dark - Vec3::splat(floor)).abs().max_element() <= slack, "c {c} S {s} f {f}: {dark} vs {floor}");
                    // r = 1: corr is exactly 0, the pixel comes back after one round trip of
                    // the powers (about 2.2 + 1 ulp, under 1e-6 relative per channel).
                    let same = compose_ratio(pixel, correction(f, 1.0, 1.0), c);
                    assert!((same - pixel).abs().cmple(pixel * 1e-6).all(), "{same} vs {pixel}");
                }
            }
        }
        // A counter-example: S' = 2, C' = 1, f = 1/2, a = 1. Fogged
        // Y' = 0.5 x 2 + 0.5 x 1 = 1.5; full shadow leaves (1 - f) S' = 1.0. The old floor
        // (the unfogged surface, linear 2^2.2 = 4.59) is already above the fogged pixel
        // (1.5^2.2 = 2.44) with the light off, so it could never hold.
        let pixel = Vec3::splat(1.5f32.powf(2.2));
        let shadowed = compose_ratio(pixel, correction(0.5, 0.0, 1.0), Vec3::ONE);
        assert!((shadowed - Vec3::ONE).abs().max_element() < 1e-5, "{shadowed}");
        assert!(2.0f32.powf(2.2) > pixel.x);
        // a = 0.35: 1.5 - 0.35 x 0.5 = 1.325 in display values.
        let part = compose_ratio(pixel, correction(0.5, 0.0, 0.35), Vec3::ONE).powf(1.0 / 2.2);
        assert!((part - Vec3::splat(1.325)).abs().max_element() < 1e-5, "{part}");
    }

    #[test]
    fn the_light_chain_keeps_the_floor() {
        // Column sums -> the lookup -> the lit fraction -> the relative lit fraction -> F_near
        // -> the fog blend -> the one form, for real fogs and world samples nearer than the far
        // blend's start (t_d = 0, t = g) in front of, at and beyond the grid's end: the floor (1 - f) S' holds for any RAYS, SHADE, LIFT, FLOOR.
        let phase = AirTune::default().phase().normed();
        let gamma = |v: f32| v.max(0.0).powf(1.0 / 2.2);
        let colour = Vec3::new(0.8, 0.7, 0.6);
        let cd = colour.powf(1.0 / 2.2);
        for (fog, depth) in [(CLEAR, 12_000.0f32), (HAZY, 12_000.0), (HAZY, 48_000.0)] {
            let g = grid_at(depth);
            let rl = 1.13f32;
            let end = g.depth * rl;
            for which in 0..4 {
                let nm = column_nm(&g, rl, fog, &|k| pattern(which, k));
                for d in [10.0f32, 300.0, 2_000.0, 0.7 * end, end, 3.0 * end, 90_000.0] {
                    let [n, m] = sums_at(&nm, g.slice_coord(d / rl));
                    let vbar = lit_fraction(n, m);
                    assert!((0.0..=1.0).contains(&vbar));
                    let f = fog_share(d, fog);
                    let f_near = near_share(d, end, fog);
                    assert!(f_near <= f + 1e-6, "{d}: {f_near} > {f}");
                    for (rays, shade, floor) in [(1.0f32, 0.0f32, 0.0f32), (8.0, 0.0, 1.0), (4.0, 0.5, 1.0)] {
                        let vc = relative_lit(vbar, 0.6, rays, 3.0, shade);
                        for p in [phase.phase(-1.0), 1.0, phase.most()] {
                            // a is the applied scale: up to strength 2 x share 1 = 2.
                            for (s, a) in [(0.0f32, 1.0f32), (0.5, 0.35), (4.0, 1.0), (0.0, 2.0), (0.5, applied_scale(2.0, 0.6)), (4.0, 2.0)] {
                                let yd = Vec3::splat((1.0 - f) * gamma(s)) + cd * f;
                                let t = sample_blend(0.0, f_near);
                                let c = blended_correction(vc, phase_floor(p, floor), 0.0, a, f_near, 1.0, t);
                                let out = compose_blended(yd.powf(2.2), c, colour, t, Some(f_near)).powf(1.0 / 2.2);
                                let floor_d = (1.0 - f) * gamma(s);
                                assert!(out.min_element() >= floor_d - 1e-5 * yd.max_element().max(1.0), "{fog:?} {d} {which} {p}: {out} < {floor_d}");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn scaled_share_is_consistent() {
        // Equal lumas and scales: d = a, s = 1/2 exactly.
        assert_eq!(scaled_share(0.5, 2.0, 0.5, 2.0), 0.5);
        assert_eq!(scaled_share(0.0, 0.75, 0.3, 0.9), 0.0);
        assert_eq!(scaled_share(0.0, 0.75, 0.0, 0.9), 0.0);
        assert_eq!(scaled_share(0.0, 0.0, 0.0, 0.0), 0.0);
        let good = [0.8f32, 0.75, 0.3, 0.9];
        for i in 0..4 {
            for bad in [f32::NAN, f32::INFINITY] {
                let v: [f32; 4] = std::array::from_fn(|j| if j == i { bad } else { good[j] });
                assert_eq!(scaled_share(v[0], v[1], v[2], v[3]), 0.0, "{bad} at {i}");
            }
        }
        // Noon-like numbers: d = 0.8 x 0.75 = 0.6, a = 0.3 x 0.9 = 0.27, s = 0.6 / 0.87.
        assert!((scaled_share(0.8, 0.75, 0.3, 0.9) - 0.6 / 0.87).abs() < 1e-6);
        // Negative factors are held to 0.
        assert_eq!(scaled_share(-1.0, 1.0, 1.0, 1.0), 0.0);
        assert_eq!(scaled_share(1.0, 1.0, -1.0, 1.0), 1.0);
        // The horizon fade scales direct_scale: s falls with it.
        let mut last = 1.0f32;
        for i in (0..=10).rev() {
            let s = scaled_share(0.8, 0.75 * i as f32 / 10.0, 0.3, 0.9);
            assert!(s <= last && (0.0..=1.0).contains(&s));
            last = s;
        }
        assert_eq!(last, 0.0);
        // A black ambient: 1 at full sun; near black s falls with the direct light
        // (d = 0.8 x 0.75 x 1e-4 = 6e-5 against the floor 1e-3: 0.06), 0 at none.
        assert_eq!(scaled_share(0.8, 0.75, 0.0, 0.9), 1.0);
        assert!((scaled_share(0.8, 0.75 * 1e-4, 0.0, 0.9) - 0.06).abs() < 1e-6);
        assert_eq!(scaled_share(0.8, 0.0, 0.0, 0.9), 0.0);
        // The effective share a: the share alone, held to 0..=1.
        assert_eq!(effective_share(0.5, None), 0.5);
        assert_eq!(effective_share(0.5, Some(0.1)), 0.1);
        assert_eq!(effective_share(f32::NAN, Some(0.3)), 0.3);
        assert_eq!(effective_share(f32::NAN, None), 0.0);
        assert_eq!(effective_share(1.5, None), 1.0);
        assert_eq!(effective_share(-1.0, None), 0.0);
        // The applied scale k x a: the strength held to 0..=2 (NaN 0) times a held to 0..=1.
        assert_eq!(applied_scale(1.0, 0.5), 0.5);
        assert_eq!(applied_scale(2.0, 0.5), 1.0);
        assert_eq!(applied_scale(2.0, 1.0), 2.0);
        assert_eq!(applied_scale(4.0, 1.0), 2.0);
        assert_eq!(applied_scale(0.0, 1.0), 0.0);
        assert_eq!(applied_scale(-1.0, 1.0), 0.0);
        assert_eq!(applied_scale(f32::NAN, 1.0), 0.0);
        assert_eq!(applied_scale(1.0, f32::NAN), 0.0);
        assert_eq!(applied_scale(1.0, 3.0), 1.0);
        // Rec. 709 luma.
        assert!((display_luma(Vec3::ONE) - 1.0).abs() < 1e-6);
        assert_eq!(display_luma(Vec3::splat(-1.0)), 0.0);
    }

    /// The far blend: 0 near, 1 for cleared depth and from the sky's distance on,
    /// a smoothstep between (1/2 halfway), monotone, continuous at both ends.
    #[test]
    fn the_far_blend_runs_from_world_to_sky() {
        let from = crate::sky::SKY_DEPTH_FROM;
        assert_eq!(far_blend(0.0, false, from), 0.0);
        assert_eq!(far_blend(0.6 * from, false, from), 0.0);
        assert!((far_blend(0.8 * from, false, from) - 0.5).abs() < 1e-5);
        assert!(far_blend(0.6 * from + 1.0, false, from) < 1e-8);
        // One unit short of the end the smoothstep is 1 - 7.6e-10 exactly; an f32 can only say
        // "within a few steps of 1" there (its spacing under 1 is 6e-8).
        assert!(far_blend(from - 1.0, false, from) > 1.0 - 1e-6);
        assert_eq!(far_blend(from, false, from), 1.0);
        assert_eq!(far_blend(f32::INFINITY, false, from), 1.0);
        for d in [0.0f32, 0.7 * from, 1e9] {
            assert_eq!(far_blend(d, true, from), 1.0);
        }
        // Degenerate inputs: the sky's form.
        assert_eq!(far_blend(f32::NAN, false, from), 1.0);
        assert_eq!(far_blend(10.0, false, 0.0), 1.0);
        assert_eq!(far_blend(10.0, false, f32::NAN), 1.0);
        let mut last = 0.0f32;
        for i in 0..=100 {
            let t = far_blend(from * (0.6 + 0.4 * i as f32 / 100.0), false, from);
            assert!(t + 1e-6 >= last && (0.0..=1.0).contains(&t));
            last = t;
        }
        // mix as WGSL computes it: exact at its ends.
        assert_eq!(mix(0.3, 7.0, 0.0), 0.3);
        assert_eq!(mix(0.3, 7.0, 1.0), 7.0);
    }

    /// The deck's share: its least up to its start (an opaque deck's 0 among it), 1 from its end, the
    /// smoothstep's 0.104 and 1/2 of the rest at a fifth and half of the way, monotone and continuous;
    /// with `SHARE` set it is 1.
    #[test]
    fn the_deck_share_follows_the_clouds_transmittance() {
        let at = |x: f32| AIR_DECK_FROM + x * (AIR_DECK_TO - AIR_DECK_FROM);
        let rest = |s: f32| AIR_DECK_LEAST + (1.0 - AIR_DECK_LEAST) * s;
        assert_eq!(deck_share(0.0, false), AIR_DECK_LEAST);
        assert_eq!(deck_share(AIR_DECK_FROM, false), AIR_DECK_LEAST);
        assert_eq!((deck_share(AIR_DECK_TO, false), deck_share(1.0, false)), (1.0, 1.0));
        // x^2 (3 - 2x): 0.04 x 2.6 = 0.104, 0.5.
        assert!((deck_share(at(0.2), false) - rest(0.104)).abs() < 1e-5);
        assert!((deck_share(at(0.5), false) - rest(0.5)).abs() < 1e-5);
        let mut last = AIR_DECK_LEAST;
        for i in 0..=1_000 {
            let d = deck_share(i as f32 / 1_000.0, false);
            assert!((AIR_DECK_LEAST..=1.0).contains(&d) && d + 1e-6 >= last && d - last <= 1.5 * 0.001 / (AIR_DECK_TO - AIR_DECK_FROM) + 1e-6, "{i}: {last} -> {d}");
            last = d;
        }
        for t in [0.0f32, 0.3, 1.0] {
            assert_eq!(deck_share(t, true), 1.0, "SHARE set, {t}");
        }
    }

    /// The volume's transmittance taken to a light from straight above, and its mix with the flat
    /// layers': one clear weather at three hours is one share, Beer's law exactly for a sheet, an
    /// opaque deck closed at every height, the covered share's weights, the height held.
    #[test]
    fn the_decks_transmittance_is_the_vertical_one() {
        // The mean along the light at 12:21, 17:30 and 18:36 (heights 0.975, 0.525, 0.306), as read
        // back on 2026-11-24 (K at strength 0.6 turned into T): 0.867, 0.879 and 0.893, the share 1.
        for (t, up, above) in [(0.8633f32, 0.975f32, 0.8665f32), (0.7817, 0.525, 0.8787), (0.6917, 0.306, 0.8933)] {
            let deck = deck_transmittance(t, 1.0, 0.0, up);
            assert!((deck - above).abs() < 1e-3, "{t} {up}: {deck}");
            assert_eq!(deck_share(deck, false), 1.0, "{t} {up}");
        }
        for up in [0.1f32, 0.25, 0.6, 1.0] {
            assert!((deck_transmittance((-0.4f32 / up).exp(), 1.0, 1.0, up) - (-0.4f32).exp()).abs() < 1e-5, "{up}");
        }
        for up in [-1.0f32, 0.0, 0.05, 0.1, 0.5, 1.0] {
            assert_eq!(deck_share(deck_transmittance(0.0, 1.0, 0.0, up), false), AIR_DECK_LEAST, "opaque, {up}");
        }
        assert!((deck_transmittance(0.25, 0.5, 0.9, 0.5) - 0.7).abs() < 1e-6, "half the root of 0.25, half the flat 0.9");
        assert_eq!(deck_transmittance(0.25, 0.0, 0.9, 0.5), 0.9, "none covered: the flat layers' alone");
        // The known limit: a broken deck without height, half clear and half opaque, whose mean is 0.5
        // whatever the light's height, reads clearer at a low sun: D 0.345 overhead (x 0.222, 0.126
        // of the rest), 0.999 at 0.25 (0.5^0.25 = 0.841).
        assert!((deck_share(deck_transmittance(0.5, 1.0, 0.0, 1.0), false) - 0.345).abs() < 1e-3);
        assert!((deck_share(deck_transmittance(0.5, 1.0, 0.0, 0.25), false) - 0.999).abs() < 1e-3);
        assert_eq!(deck_transmittance(0.5, 1.0, 0.0, f32::NAN), 0.5);
        assert_eq!(deck_transmittance(0.5, 1.0, 0.0, 0.0), deck_transmittance(0.5, 1.0, 0.0, AIR_DECK_UP));
    }

    /// The bright hold: none while the lifted pixel stays at or under its start, all of the gain
    /// from its end on, between them the excess fitted into the room (hand figures), in 0..=1,
    /// and never a lift past the end.
    #[test]
    fn the_bright_hold_fits_the_gain_under_its_end() {
        let (from, to) = (AIR_BRIGHT_FROM, AIR_BRIGHT_TO);
        for peak in [0.0f32, 0.3, from - 0.2] {
            assert_eq!(bright_hold(peak, 0.1), 0.0);
        }
        for gain in [0.01f32, 0.2, 5.0] {
            assert_eq!(bright_hold(to, gain), 1.0);
            assert_eq!(bright_hold(40.0, gain), 1.0);
        }
        // At the start itself an excess as large as the room (to - from) keeps half.
        let room = to - from;
        assert!((bright_hold(from, room) - 0.5).abs() < 1e-6);
        // Halfway up, room / 2 left, a gain of twice that room: 2 / 3 held back.
        assert!((bright_hold(from + 0.5 * room, room) - 2.0 / 3.0).abs() < 1e-5);
        // Below the start by a room, a gain of two rooms: the excess is one room, half of it held
        // back: a quarter of the gain.
        assert!((bright_hold(from - room, 2.0 * room) - 0.25).abs() < 1e-5);
        for i in 0..=150 {
            let peak = i as f32 / 100.0;
            for gain in [0.0f32, 1e-7, 1e-4, 0.05, 0.3, 1.0, 8.0, 400.0] {
                let h = bright_hold(peak, gain);
                assert!((0.0..=1.0).contains(&h), "{peak} {gain}: {h}");
                assert!(peak + gain * (1.0 - h) <= peak.max(to) + 1e-4_f32.max(gain * 1e-6), "{peak} {gain}: {h}");
            }
        }
        assert_eq!(bright_hold(0.0, 0.0), 0.0);
        assert_eq!(bright_hold(f32::NAN, 0.3), bright_hold(from, 0.3), "no number reads as the start");
        // The sample's scalar: none of it near, shadow untouched.
        assert_eq!(hold_bright(0.8, 2.0, 1.0, 0.0), 0.8);
        assert_eq!(hold_bright(-0.8, 2.0, 1.0, 1.0), -0.8);
        assert_eq!(hold_bright(0.8, 2.0, 1.0, 1.0), 0.0);
        assert!((hold_bright(0.8, 2.0, 1.0, 0.5) - 0.4).abs() < 1e-6);
    }

    /// The fog blend: 0 up to its start, 1/2 halfway, 1 at a full share, monotone, flat at both
    /// ends; joined with the far blend it is never below either, exactly the far blend while the
    /// fog is thin, 1 when either is 1, and continuous.
    #[test]
    fn the_fog_blend_joins_the_far_blend() {
        let from = AIR_FOG_BLEND_FROM;
        assert_eq!(fog_blend(0.0), 0.0);
        assert_eq!(fog_blend(from), 0.0);
        assert!((fog_blend(0.5 * (from + 1.0)) - 0.5).abs() < 1e-5);
        assert_eq!(fog_blend(1.0), 1.0);
        assert_eq!(fog_blend(1.5), 1.0);
        assert_eq!(fog_blend(f32::NAN), 0.0);
        // Flat at both ends: a hundredth of the way in, at most 3 x 1e-4 from the end's value.
        let step = 0.01 * (1.0 - from);
        assert!(fog_blend(from + step) < 3.1e-4 && fog_blend(1.0 - step) > 1.0 - 3.1e-4);
        let mut last = 0.0f32;
        for i in 0..=200 {
            let g = fog_blend(i as f32 / 200.0);
            // The smoothstep's slope is at most 1.5 per unit of x.
            assert!(g + 1e-6 >= last && g - last <= 1.5 * 0.005 / (1.0 - from) + 1e-6, "{i}: {last} -> {g}");
            last = g;
        }
        for i in 0..=20 {
            let t_d = i as f32 / 20.0;
            assert_eq!(sample_blend(t_d, 0.5 * from), t_d, "thin fog: the far blend alone");
            assert!((sample_blend(t_d, 1.0) - 1.0).abs() < 1e-6, "{t_d}");
            assert_eq!(sample_blend(1.0, t_d), 1.0);
            for j in 0..=20 {
                let f = j as f32 / 20.0;
                let t = sample_blend(t_d, f);
                assert!(t <= 1.0 + 1e-6 && t >= t_d - 1e-6 && t >= fog_blend(f) - 1e-6, "{t_d} {f}: {t}");
            }
        }
        assert_eq!(sample_blend(0.0, 1.0), 1.0);
    }

    #[test]
    fn tune_changed_sees_every_field() {
        let a = AirTune::start(1.0, AirView::None, AirOccluders::All, None);
        assert!(!tune_changed(&a, &a));
        assert!(!tune_changed(&a, &AirTune::parse("# nothing", &a)));
        let changed = [
            AirTune { strength: 2.0, ..a },
            AirTune { share: Some(0.3), ..a },
            AirTune { g: 0.4, ..a },
            AirTune { back_g: -0.5, ..a },
            AirTune { back_weight: 0.1, ..a },
            AirTune { cap: 3.0, ..a },
            AirTune { depth: Some(48_000.0), ..a },
            AirTune { sky_weight: 0.6, ..a },
            AirTune { sky_phase: 1.0, ..a },
            AirTune { rays: 5.0, ..a },
            AirTune { floor: 0.0, ..a },
            AirTune { reference: Some(0.6), ..a },
            AirTune { lift: 2.0, ..a },
            AirTune { shade: 0.0, ..a },
            AirTune { samples: 2, ..a },
            AirTune { view: AirView::Delta, ..a },
            AirTune { occluders: AirOccluders::Terrain, ..a },
            AirTune { grid: Some([80, 44, 32]), ..a },
            AirTune { moon: MoonTune { strength: a.moon.strength + 0.1, ..a.moon }, ..a },
            AirTune { moon: MoonTune { rays: Some(3.5), ..a.moon }, ..a },
            AirTune { moon: MoonTune { lift: Some(1.45), ..a.moon }, ..a },
            AirTune { moon: MoonTune { shade: Some(0.95), ..a.moon }, ..a },
            AirTune { moon: MoonTune { g: Some(0.55), ..a.moon }, ..a },
            AirTune { moon: MoonTune { cap: Some(1.55), ..a.moon }, ..a },
            AirTune { moon: MoonTune { sky_weight: Some(0.255), ..a.moon }, ..a },
            AirTune { moon: MoonTune { sky_phase: Some(0.05), ..a.moon }, ..a },
        ];
        for b in changed {
            assert!(tune_changed(&a, &b), "{b:?}");
            assert!(tune_changed(&b, &a), "{b:?}");
        }
        // A NaN compares equal to itself (no change every poll); Some(NaN) against None differs.
        let nan = AirTune { strength: f32::NAN, ..a };
        assert!(!tune_changed(&nan, &nan));
        assert!(tune_changed(&AirTune { share: Some(f32::NAN), ..a }, &a));
        // Every key reaches its field through the parser.
        let t = AirTune::parse("STRENGTH=2\nSHARE=0.4\nG=0.2\nBACK=-0.1\nBACKWEIGHT=0.2\nCAP=1.2\nDEPTH=48000\nSKY=1\nOCCLUDERS=terrain\nVIEW=off", &a);
        assert_eq!((t.strength, t.share, t.g, t.back_g, t.back_weight, t.cap), (2.0, Some(0.4), 0.2, -0.1, 0.2, 1.2));
        assert_eq!((t.depth, t.sky_weight, t.occluders, t.view), (Some(48_000.0), 1.0, AirOccluders::Terrain, AirView::Off));
    }

    #[test]
    fn air_light_bounds_figures() {
        // The glare bound in numbers for six weathers (day near, far, power; the
        // camera clamps a negative near to 0 outdoors), on the centre ray (F at the grid's end
        // = fog_share(depth)), with the default phase. Per row: F; the peak display gain
        // (most - 1) x a x F as a share of C'; the full-shadow loss a x F (share of C'); the
        // linear-light gain of a pixel whose fogged value equals C: (1 + gain)^2.2 - 1.
        let weathers = [
            ("WastelandClear", -10.0f32, 200_000.0f32, 0.6f32),
            ("WastelandGS", 10.0, 120_000.0, 0.5),
            ("WastelandHazy", 1_800.0, 30_000.0, 0.6),
            ("Strip/UrbanClear", 2_000.0, 200_000.0, 0.35),
            ("HooverDam", -1_000.0, 100_000.0, 0.6),
            ("ColoradoRiver", -100.0, 75_000.0, 0.5),
        ];
        let phase = AirPhase::default();
        let most = phase.most();
        println!("air light bounds: phase g {} back weight {} cap {} -> most {most:.4}", phase.g, phase.back_weight, phase.cap);
        for (name, near, far, power) in weathers {
            let fog = AirFog { near: near.max(0.0), far, power, strength: 1.0 };
            for depth in [12_000.0f32, 48_000.0] {
                let f = fog_share(depth, fog);
                for a in [0.35f32, 1.0] {
                    let gain = (most - 1.0) * a * f;
                    let loss = a * f;
                    let linear = (1.0 + gain).powf(2.2) - 1.0;
                    println!("{name:<17} depth {depth:>6}  a {a:.2}  F {f:.4}  peak gain {gain:.4} C'  shadow loss {loss:.4} C'  linear gain at Y=C {linear:.4}");
                    assert!(f.is_finite() && gain.is_finite() && loss.is_finite() && linear.is_finite());
                }
            }
        }
    }

    /// F_near and M agree at the grid's end (the composite's convention follows column_nm's):
    /// M telescopes to F at the end (slice 0 owns the camera's own share when near is below 0),
    /// and near_share there is the same F; within the f32 sum's error (3.8e-6, see
    /// column_nm_stores_bounded_sums). Below the end F_near is analytic and M interpolated:
    /// only their ratio's numerator N / M enters, so they need not agree there.
    #[test]
    fn near_share_meets_the_stored_sum_at_the_grid_end() {
        for fog in [CLEAR, HAZY, AirFog { near: -1_000.0, far: 100_000.0, power: 0.6, strength: 0.8 }] {
            for (depth, rl) in [(12_000.0f32, 1.0f32), (12_000.0, 1.21), (48_000.0, 1.07)] {
                let g = grid_at(depth);
                let end = g.depth * rl;
                let m = column_nm(&g, rl, fog, &|_| 1.0)[g.nz as usize - 1][1];
                for d in [end, 1.5 * end, 150_000.0] {
                    let f_near = near_share(d, end, fog);
                    assert!((f_near - m).abs() < 1e-5, "{fog:?} {depth} {rl} at {d}: {f_near} vs {m}");
                }
                // The lookup carries the end's M beyond the grid (c held to nz).
                let nm = column_nm(&g, rl, fog, &|_| 1.0);
                assert_eq!(sums_at(&nm, g.slice_coord(2.0 * g.depth))[1], m);
            }
        }
    }

    #[test]
    fn the_gate_names_the_first_reason() {
        let ok = AirGateInput { interior: false, submerged: false, sun: true, fog: CLEAR, display_blend: true, source: AirSource::Sun, strength: 1.0, share: 0.4 };
        assert_eq!(gate(&ok), Ok(AirSource::Sun));
        assert_eq!(gate(&AirGateInput { source: AirSource::Moon, ..ok }), Ok(AirSource::Moon));
        let cases = [
            (AirGateInput { interior: true, ..ok }, AirOff::Indoors),
            (AirGateInput { submerged: true, ..ok }, AirOff::UnderWater),
            (AirGateInput { sun: false, ..ok }, AirOff::NoSun),
            (AirGateInput { fog: AirFog { strength: 0.0, ..CLEAR }, ..ok }, AirOff::UnusableFog),
            (AirGateInput { fog: AirFog { near: f32::NAN, ..CLEAR }, ..ok }, AirOff::UnusableFog),
            (AirGateInput { display_blend: false, ..ok }, AirOff::LinearBlend),
            (AirGateInput { source: AirSource::None, ..ok }, AirOff::Night),
            (AirGateInput { source: AirSource::Moon, share: 0.0, ..ok }, AirOff::NoShare),
            (AirGateInput { source: AirSource::Moon, interior: true, ..ok }, AirOff::Indoors),
            (AirGateInput { strength: 0.0, ..ok }, AirOff::NoStrength),
            (AirGateInput { strength: f32::NAN, ..ok }, AirOff::NoStrength),
            (AirGateInput { share: 0.0, ..ok }, AirOff::NoShare),
            (AirGateInput { share: f32::NAN, ..ok }, AirOff::NoShare),
            // Strength 0 is named before a zero share, after night.
            (AirGateInput { strength: 0.0, share: 0.0, ..ok }, AirOff::NoStrength),
            (AirGateInput { source: AirSource::None, strength: 0.0, ..ok }, AirOff::Night),
            // The order: indoors before everything, night before a zero share.
            (AirGateInput { interior: true, submerged: true, source: AirSource::None, share: 0.0, ..ok }, AirOff::Indoors),
            (AirGateInput { source: AirSource::None, share: 0.0, ..ok }, AirOff::Night),
        ];
        for (input, off) in cases {
            assert_eq!(gate(&input), Err(off), "{input:?}");
            assert!(!off.reason().is_empty());
        }
        assert_eq!(AirOff::Night.reason(), "night, and the climate has no moon");
    }

    #[test]
    fn the_on_line_states_the_bound() {
        let g = grid_at(12_000.0);
        let tune = hand(AirTune::start(1.0, AirView::None, AirOccluders::All, None));
        let phase = tune.phase().normed();
        let fog = AirFog { near: 0.0, ..CLEAR };
        // The centre ray (corner 1): F = 0.06^0.6 = 0.184880; a = 0.5; most 2.5, LIFT 1.5
        // (`hand`): near (1.5 x 2.5 - 1) x 0.5 x F = 0.25421 (far the same: its factor
        // is largest at t = 0); sky at SKYPHASE 0, SKY 0.5: (1.5 - 1) x 0.5 x F x 0.5 = 0.02311.
        let line = on_line(&g, &phase, 0.5, &tune, fog, 1.0);
        assert_eq!(
            line,
            "volumetric medium: light in the air on: grid 160x88x64 to 12000 units, lit by the sun, phase g 0.6, cap 2.5, sunlit share 0.50, strength 1, near pixels at most +0.255 x the fog colour, far pixels at most +0.255 x mix(fog colour, pixel, t), sky at most x1.024 (display values; a snapshot for this weather, hour and view), sky 0.5, sky phase 0, rays 6, floor 1, ref auto, lift 1.5, shade 0.9"
        );
        // A far fog: DEPTH 12,000, fog from 15,000 to 18,000: nothing on the centre
        // ray, but the corner ray of a 75 degree field at 16:9 (proj_y 1 / tan 37.5 = 1.30323,
        // proj_x 0.73307: length sqrt(1 + 1.86087 + 0.58879) = 1.85733) reaches 22,288 units,
        // past the whole ramp: F = 1, world (2.5 x 1.75 - 1) x 1 = 3.375.
        let ramp = AirFog { near: 15_000.0, far: 18_000.0, power: 0.6, strength: 1.0 };
        let proj_y = 1.0 / 37.5f32.to_radians().tan();
        let corner = corner_ray_len(proj_y / (16.0 / 9.0), proj_y);
        assert!((corner - 1.85733).abs() < 1e-4, "{corner}");
        let full = AirTune { share: Some(1.0), ..tune };
        assert_eq!(view_bounds(&g, &phase, 1.0, &full, ramp, 1.0), ViewBounds { near: 0.0, far: 0.0, sky: 0.0 });
        let b = view_bounds(&g, &phase, 1.0, &full, ramp, corner);
        let (world, sky) = (b.near, b.sky);
        // Far pixels: the factor is largest at t = 0 (both the phase and the weight fall with
        // t), so the far bound is the near one here.
        assert_eq!(b.far, b.near);
        // With share 1 and F 1 at LIFT 1.5 and SKY 0.5: near 1.5 x 2.5 - 1 = 2.75,
        // sky (1.5 - 1) x 0.5 = 0.25.
        assert!((world - 2.75).abs() < 1e-4 && (sky - 0.25).abs() < 1e-5, "{world} {sky}");
        assert!(on_line(&g, &phase, 0.5, &full, ramp, corner).contains("near pixels at most +2.750 x the fog colour, far pixels at most +2.750 x mix(fog colour, pixel, t), sky at most x1.250 (display values;"));
        // SKYPHASE 1 and SKY 0.5: the sky's factor takes the phase and the weight:
        // (1.75 x 2.5 - 1) x 0.5 = 1.6875.
        let sky = view_bounds(&g, &phase, 1.0, &AirTune { sky_phase: 1.0, sky_weight: 0.5, ..full }, ramp, corner).sky;
        // (1.5 x 2.5 - 1) x 0.5 = 1.375 at LIFT 1.5.
        assert!((sky - 1.375).abs() < 1e-4, "{sky}");
        // A projection the bound cannot read takes the fog's whole strength.
        assert_eq!(corner_ray_len(0.0, 1.0), f32::INFINITY);
        assert_eq!(corner_ray_len(f32::NAN, 1.0), f32::INFINITY);
        assert!((view_bounds(&g, &phase, 1.0, &full, AirFog { strength: 0.7, ..CLEAR }, f32::INFINITY).near - 2.75 * 0.7).abs() < 1e-4);
        assert!(on_line(&g, &phase, 0.5, &AirTune { reference: Some(0.6), ..tune }, fog, 1.0).contains("ref 0.6 (set), lift 1.5, shade 0.9"));
        // The tune's SHARE is the share shown.
        assert!(on_line(&g, &phase, 0.5, &AirTune { share: Some(0.2), ..tune }, fog, 1.0).contains("sunlit share 0.20"));
    }

    /// The strength is one overall scale (the slider): the logged bounds
    /// are k times those at strength 1 (view_bounds is linear in its scale), the correction
    /// of every sample is k times the share's before the floor (so 0 at strength 0, monotone in
    /// k: lit air brightens and shadowed air darkens more), and the floor -min(F_near, 1) holds
    /// at strength 2.
    #[test]
    fn the_strength_scales_the_whole_correction() {
        let g = grid_at(12_000.0);
        let fog = AirFog { near: 0.0, ..CLEAR };
        let base = hand(AirTune::start(1.0, AirView::None, AirOccluders::All, None));
        let phase = base.phase().normed();
        // At strength 1 (the_on_line_states_the_bound): near 0.254, sky 0.0231 at s 0.5.
        let one = view_bounds(&g, &phase, applied_scale(1.0, 0.5), &base, fog, 1.0);
        let (w1, s1) = (one.near, one.sky);
        let mut last = (-1.0f32, -1.0f32);
        for k in [0.0f32, 0.5, 1.0, 2.0] {
            let b = view_bounds(&g, &phase, applied_scale(k, effective_share(0.5, None)), &base, fog, 1.0);
            let (w, s) = (b.near, b.sky);
            assert!((b.far - k * one.far).abs() < 1e-6);
            assert!((w - k * w1).abs() < 1e-6 && (s - k * s1).abs() < 1e-6, "{k}: {w} {s}");
            assert!(w >= last.0 && s >= last.1, "monotone in the strength");
            last = (w, s);
        }
        // The line at these values: F = 0.06^0.6 = 0.184880; near and far
        // (1.5 x 2.5 - 1) x k x 0.5 x F = 0.25421 k, sky 1 + (1.5 - 1) x k x 0.5 x F x SKY 0.5 =
        // 1 + 0.023110 k.
        let line = |k: f32| on_line(&g, &phase, 0.5, &AirTune { strength: k, ..base }, fog, 1.0);
        assert!(line(0.0).contains("strength 0, near pixels at most +0.000 x the fog colour, far pixels at most +0.000 x mix(fog colour, pixel, t), sky at most x1.000 ("), "{}", line(0.0));
        // (Printed rounded up: 0.127105 -> 0.128, 1.011555 -> 1.012; 0.254210 -> 0.255,
        // 1.023110 -> 1.024; 0.508421 -> 0.509, 1.046220 -> 1.047.)
        assert!(line(0.5).contains("strength 0.5, near pixels at most +0.128 x the fog colour, far pixels at most +0.128 x mix(fog colour, pixel, t), sky at most x1.012 ("), "{}", line(0.5));
        assert!(line(1.0).contains("strength 1, near pixels at most +0.255 x the fog colour, far pixels at most +0.255 x mix(fog colour, pixel, t), sky at most x1.024 ("), "{}", line(1.0));
        assert!(line(2.0).contains("strength 2, near pixels at most +0.509 x the fog colour, far pixels at most +0.509 x mix(fog colour, pixel, t), sky at most x1.047 ("), "{}", line(2.0));
        // Each sample's scalar: k times the strength-1 scalar while the floor is not reached;
        // 0 at k 0; monotone in k for lit air (Vbar_c p_eff above 1) and for shadow (below 1).
        let f_near = 0.3f32;
        for t in [0.0f32, 0.5, 1.0] {
            let at = |vc: f32, k: f32| blended_correction(vc, 1.5, 0.0, applied_scale(k, 0.6), f_near, 1.0, t);
            for vc in [1.4f32, 1.0 / 3.0] {
                assert_eq!(at(vc, 0.0), 0.0);
                let one = at(vc, 1.0);
                let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
                for k in [0.0f32, 0.25, 0.5, 1.0, 1.5, 2.0] {
                    let c = at(vc, k);
                    if one > 0.0 {
                        assert!((c - k * one).abs() < 1e-6 && c >= hi.max(0.0) - 1e-7, "lit, t {t} k {k}: {c}");
                        hi = c;
                    } else {
                        assert!((c - k * one).abs() < 1e-6 && c <= lo.min(0.0) + 1e-7 && c >= -f_near.min(1.0), "shadow, t {t} k {k}: {c}");
                        lo = c;
                    }
                }
            }
        }
        // The floor at strength 2: full shadow (Vbar_c 0) with share 1 and F_near 0.8 would be
        // 2 x 0.8 x (0 - 1) = -1.6 before the floor; it is held to -0.8 (shadow takes at most the
        // budget share), and a fogged white pixel keeps (1 - f) S' = 0.2 in display values.
        let c = blended_correction(0.0, 1.0, 0.0, applied_scale(2.0, 1.0), 0.8, 1.0, 0.0);
        assert_eq!(c, -0.8);
        let shown = compose_blended(Vec3::ONE, c, Vec3::ONE, 0.0, Some(0.8)).powf(1.0 / DISPLAY_GAMMA);
        assert!((shown - Vec3::splat(0.2)).abs().max_element() < 1e-5, "{shown}");
    }

    /// A sample with geometry keeps its surface light at every
    /// blend t. One case, one channel: S' 1, C' 0, f = F_near = 0.5, a = SKY = t
    /// = 1, full shadow: Y' = 0.5 stays exactly 0.5 (it was 0.25 at SHADE 0 and 0.375 at SHADE
    /// 0.5); a bright surface in a dark fog (S' 2, C' 0.1, f = F_near = 0.4, strength 2) never
    /// goes below (1 - f) S' = 1.2 at t 0, 0.5 and 1; cleared depth keeps the scalar bound (the
    /// sky has no surface: it still darkens); positive changes are untouched.
    #[test]
    fn a_sample_with_geometry_keeps_its_surface_light_at_every_t() {
        let lin = |d: f32| Vec3::splat(d.powf(DISPLAY_GAMMA));
        let disp = |v: Vec3| v.powf(1.0 / DISPLAY_GAMMA);
        for shade in [0.0f32, 0.5] {
            let vc = relative_lit(0.0, 1.0, 8.0, 1.75, shade);
            let c = blended_correction(vc, 1.0, 0.0, 1.0, 0.5, 1.0, 1.0);
            assert!(c < 0.0, "shadow: {c}");
            let out = disp(compose_blended(lin(0.5), c, Vec3::ZERO, 1.0, Some(0.5)));
            assert!((out - Vec3::splat(0.5)).abs().max_element() < 1e-6, "SHADE {shade}: {out}");
            // Without the per-channel hold (as the sky is treated): 0.5 x (1 + c).
            let sky = disp(compose_blended(lin(0.5), c, Vec3::ZERO, 1.0, None));
            assert!((sky.x - 0.5 * (1.0 + c)).abs() < 1e-6 && sky.x < 0.5, "SHADE {shade}: {sky}");
        }
        let (s, cf, f) = (2.0f32, 0.1f32, 0.4f32);
        let y = (1.0 - f) * s + f * cf;
        for t in [0.0f32, 0.5, 1.0] {
            for k in [1.0f32, 2.0] {
                let c = blended_correction(0.0, 1.0, 0.0, applied_scale(k, 1.0), f, 1.0, t);
                let out = disp(compose_blended(lin(y), c, lin(cf), t, Some(f)));
                assert!(out.min_element() >= (1.0 - f) * s - 1e-5, "t {t} k {k}: {out}");
            }
            // A lit sample is unchanged by the hold: the same with or without it.
            let lit = blended_correction(1.75, 2.5, 0.0, 1.0, f, 1.0, t);
            assert!(lit > 0.0);
            assert_eq!(compose_blended(lin(y), lit, lin(cf), t, Some(f)), compose_blended(lin(y), lit, lin(cf), t, None));
        }
    }

    /// "At most" figures are printed rounded up; an exact figure with f32 noise stays put.
    #[test]
    fn at_most_figures_round_up() {
        assert_eq!(round_up_3(2.75044), 2.751);
        assert_eq!(round_up_3(2.75), 2.75);
        assert_eq!(round_up_3(2.7500002), 2.75, "f32 noise of an exact figure");
        assert_eq!(round_up_3(0.25421), 0.255);
        assert_eq!(round_up_3(0.0), 0.0);
        assert!(round_up_3(0.0).is_sign_positive(), "no -0.000");
        assert_eq!(round_up_3(1.0), 1.0);
        assert_eq!(format!("{:.3}", round_up_3(1.0231101)), "1.024");
        for v in [0.0001f32, 0.31203, 1.06934, 3.375, 47.25] {
            assert!(round_up_3(v) >= v * (1.0 - 1e-6), "{v}");
        }
    }

    /// The bound's refresh: moved when a rounded figure moves by more than 0.05.
    #[test]
    fn the_bounds_move_by_more_than_a_twentieth() {
        let b = ViewBounds { near: 0.312, far: 0.312, sky: 0.069 };
        assert!(!bounds_moved(&b, &b));
        assert!(!bounds_moved(&b, &ViewBounds { near: 0.36, ..b }), "0.048: not more than 0.05");
        assert!(bounds_moved(&b, &ViewBounds { near: 0.363, ..b }));
        assert!(bounds_moved(&b, &ViewBounds { far: 0.2, ..b }));
        assert!(bounds_moved(&ViewBounds { near: 0.0, far: 0.0, sky: 0.0 }, &ViewBounds { sky: 0.06, ..b }));
    }

    /// The settings row's slider moves the start strength while the game runs: the follower
    /// takes the new start, the tune file's keys still win over it, and an unchanged start
    /// changes nothing.
    #[test]
    fn new_start_values_are_followed_under_the_tune_file() {
        let start = AirTune::start(1.0, AirView::None, AirOccluders::All, None);
        assert_eq!(start.with_strength(1.5).strength, 1.5);
        assert_eq!(start.with_strength(9.0).strength, 2.0);
        assert_eq!(start.with_strength(f32::NAN).strength, 0.0);
        let mut f = TuneFollower::new(start);
        // No file read: the new start is the tune.
        assert_eq!(f.restart(start.with_strength(1.5)), vec!["volumetric medium: light in the air tune: STRENGTH=1.5".to_string()]);
        assert_eq!(f.tune().strength, 1.5);
        assert!(f.restart(start.with_strength(1.5)).is_empty(), "the same start: nothing");
        // A file without STRENGTH keeps the slider's; one with it wins, also over a later move.
        f.feed(Ok("CAP=3"));
        assert_eq!((f.tune().strength, f.tune().cap), (1.5, 3.0));
        assert_eq!(f.restart(start.with_strength(0.5)), vec!["volumetric medium: light in the air tune: STRENGTH=0.5".to_string()]);
        assert_eq!((f.tune().strength, f.tune().cap), (0.5, 3.0));
        f.feed(Ok("CAP=3\nSTRENGTH=2"));
        assert!(f.restart(start.with_strength(1.0)).is_empty(), "the file's STRENGTH wins: the tune does not change");
        assert_eq!(f.tune().strength, 2.0);
        // An unreadable file: the start values, the slider's strength among them.
        f.feed(Err("gone"));
        assert_eq!(f.tune(), start.with_strength(1.0));
    }

    #[test]
    fn the_tune_file_is_followed_over_the_start() {
        let start = AirTune::start(1.0, AirView::None, AirOccluders::All, Some(12_000.0));
        let mut f = TuneFollower::new(start);
        assert_eq!(f.tune(), start);
        // A first text: its keys over the start, one line naming them.
        // (SKY 0.6: away from the default 0.25.)
        let lines = f.feed(Ok("CAP=3\nSKY=0.6\n"));
        assert_eq!(lines, vec!["volumetric medium: light in the air tune: CAP=3 SKY=0.6".to_string()]);
        assert_eq!((f.tune().cap, f.tune().sky_weight, f.tune().strength), (3.0, 0.6, 1.0));
        // The same text again: nothing.
        assert!(f.feed(Ok("CAP=3\nSKY=0.6\n")).is_empty());
        // A line removed: that key returns to its start value.
        assert_eq!(f.feed(Ok("SKY=0.6\n")), vec!["volumetric medium: light in the air tune: CAP=1.5".to_string()]);
        // A new text that changes nothing (a comment): no tune line.
        assert!(f.feed(Ok("SKY=0.6\n# note\n")).is_empty());
        // DEPTH and GRID, and auto clearing them.
        f.feed(Ok("DEPTH=48000\nGRID=80,44,32\nVIEW=delta"));
        assert_eq!((f.tune().depth, f.tune().grid, f.tune().view), (Some(48_000.0), Some([80, 44, 32]), AirView::Delta));
        // (SKY already returned to 0.25 with the text before, which had no SKY line; the start's
        // DEPTH 12,000 is cleared by `auto`.)
        assert_eq!(f.feed(Ok("DEPTH=auto\nGRID=auto")), vec!["volumetric medium: light in the air tune: DEPTH=auto GRID=auto VIEW=none".to_string()]);
        // Unreadable: the start values, said once (and the change).
        f.feed(Ok("STRENGTH=2"));
        let lines = f.feed(Err("not found"));
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines[0].contains("cannot be read (not found)") && lines[1].ends_with("tune: STRENGTH=1"), "{lines:?}");
        assert_eq!(f.tune(), start);
        assert!(f.feed(Err("not found")).is_empty());
        // Readable again with the same text as before: followed again.
        assert_eq!(f.feed(Ok("STRENGTH=2")), vec!["volumetric medium: light in the air tune: STRENGTH=2".to_string()]);
        assert_eq!(tune_diff(&start, &start), "nothing");
        // The poll's pace: 0.5 s of real time.
        assert!(poll_due(None, 0.0));
        assert!(!poll_due(Some(10.0), 10.4));
        assert!(poll_due(Some(10.0), 10.5));
        assert!(poll_due(Some(10.0), 3.0));
    }

    #[test]
    fn the_lit_fraction_is_phase_free_and_held() {
        // N = sum dF V: N / M is the dF-weighted mean of V.
        let g = grid_at(12_000.0);
        let half = column_nm(&g, 1.0, CLEAR, &|k| if k < 32 { 0.0 } else { 1.0 });
        let [n, m] = half[g.nz as usize - 1];
        let near_m = half[31][1];
        assert!((lit_fraction(n, m) - (m - near_m) / m).abs() < 1e-5);
        // All lit: exactly 1 (N = M bit for bit); all dark: 0.
        let all = column_nm(&g, 1.0, CLEAR, &|_| 1.0)[g.nz as usize - 1];
        assert_eq!(lit_fraction(all[0], all[1]), 1.0);
        assert_eq!(lit_fraction(0.0, 0.3), 0.0);
        // Empty M, NaN, above M (interpolation, f16): held.
        assert_eq!(lit_fraction(0.0, 0.0), 1.0);
        assert_eq!(lit_fraction(0.3, 1e-12), 1.0);
        assert_eq!(lit_fraction(f32::NAN, 0.3), 1.0);
        assert_eq!(lit_fraction(0.3, f32::NAN), 1.0);
        assert_eq!(lit_fraction(2.0, 0.3), 1.0);
        assert_eq!(lit_fraction(-0.1, 0.3), 0.0);
        assert_eq!(lit_fraction(0.15, 0.3), 0.5);
    }

    #[test]
    fn the_sky_is_modulated_within_its_bounds() {
        let phase = AirPhase::default().normed();
        let most = phase.most();
        // With SKY 1: 1 - a F <= 1 + corr <= 1 + a F (most - 1) at SKYPHASE 1, <= 1 at 0.
        // r_sky = Vbar x mix(1, p, k) lies in [0, max(1, p)] for Vbar in 0..1 and p in
        // (0, most]; the f32 slack covers 1 + (p - 1) k's rounding.
        for vbar in [0.0f32, 0.3, 1.0] {
            for p in [phase.phase(-1.0), 1.0, most] {
                for a in [0.0f32, 0.35, 1.0] {
                    for f in [0.0f32, 0.185, 0.77, 1.0] {
                        let one = 1.0 + sky_modulation(vbar, p, 1.0, a, f, 1.0);
                        assert!(one >= 1.0 - a * f - 1e-6 && one <= 1.0 + a * f * (most - 1.0) + 1e-6, "{vbar} {p} {a} {f}: {one}");
                        let zero = 1.0 + sky_modulation(vbar, p, 0.0, a, f, 1.0);
                        assert!(zero >= 1.0 - a * f - 1e-6 && zero <= 1.0, "{vbar} {p} {a} {f}: {zero}");
                    }
                }
            }
        }
        // Vbar 1 and SKYPHASE 0: exactly no change (r_sky = 1, corr 0; 1^2.2 = 1 exactly).
        assert_eq!(sky_modulation(1.0, most, 0.0, 0.7, 0.4, 1.0), 0.0);
        for pixel in [Vec3::ZERO, Vec3::new(0.02, 0.5, 4.0), Vec3::new(1e-6, 0.3, 30.0)] {
            assert_eq!(compose_sky(pixel, 0.0), pixel);
        }
        // Full shadow: Y' x (1 - a F) in display values: 2^2.2 x (1 - 0.5)^2.2 = 1.
        let pixel = Vec3::splat(2.0f32.powf(2.2));
        let dark = compose_sky(pixel, sky_modulation(0.0, 1.0, 0.0, 1.0, 0.5, 1.0));
        assert!((dark - Vec3::ONE).abs().max_element() < 1e-5, "{dark}");
        // SKY scales the scalar; SKY 2 with a F 1 would go below 0: held at black, never
        // negative.
        assert!((sky_modulation(0.0, 1.0, 0.0, 0.5, 0.4, 0.5) - (-0.1)).abs() < 1e-7);
        assert_eq!(compose_sky(pixel, sky_modulation(0.0, 1.0, 0.0, 1.0, 1.0, 2.0)), Vec3::ZERO);
        // NaN Vbar is taken as lit, NaN SKYPHASE as 0.
        assert_eq!(sky_modulation(f32::NAN, most, f32::NAN, 1.0, 0.5, 1.0), 0.0);
        // Negative channels count as 0, as the fog's display transfer has them.
        assert_eq!(compose_sky(Vec3::new(-1.0, 0.5, 0.5), 0.0).x, 0.0);
    }

    /// The night gate's clock: day while the aiming
    /// elevation is above 0; the horizon and NaN are not day (medium_air_light.rs tests the
    /// whole clock with a degenerate climate).
    #[test]
    fn day_follows_the_elevation_that_aimed_the_light() {
        assert!(day_by_aim(0.01) && day_by_aim(80.0));
        assert!(!day_by_aim(0.0) && !day_by_aim(-0.01) && !day_by_aim(-60.0) && !day_by_aim(f32::NAN));
    }

    /// The source: the sun above the horizon whatever the climate, the moon at and under it where the climate
    /// has one, nothing otherwise and for an elevation that is not a number.
    #[test]
    fn the_air_is_lit_by_the_sun_the_moon_or_nothing() {
        for has_moon in [false, true] {
            for e in [0.01f32, 4.0, 90.0] {
                assert_eq!(air_source(e, has_moon), AirSource::Sun);
            }
            for e in [0.0f32, -0.0, -0.01, -60.0] {
                assert_eq!(air_source(e, has_moon), if has_moon { AirSource::Moon } else { AirSource::None }, "{e}");
            }
            assert_eq!(air_source(f32::NAN, has_moon), AirSource::None);
        }
    }

    /// The moon's values: the day's constants (a tune file's included) unless set, the strength a factor held to
    /// the strength's range; the sun's constants and what is packed of them bit for bit what they are without them.
    #[test]
    fn the_moonlight_has_its_own_values() {
        assert_eq!((AirTune::default().moon, MoonTune::START.strength), (MoonTune::START, MOON_STRENGTH));
        let start = AirTune { moon: MoonTune::DAY, ..AirTune::start(1.0, AirView::None, AirOccluders::All, None) };
        let day = AirTune::parse("G=0.4\nRAYS=2", &start);
        // With nothing set the moonlight is drawn with the day's.
        for source in [AirSource::Sun, AirSource::Moon, AirSource::None] {
            assert_eq!(day.lit_by(source), day);
        }
        let text = "MOON_STRENGTH=0.5\nMOON_RAYS=6\nmoon_lift=2\nMOON_SHADE=0.7\nMOON_G=0.8\nMOON_CAP=8\nMOON_SKY=1\nMOON_SKYPHASE=1";
        let t = AirTune::parse(text, &day);
        // By the sun: the day's, the same bits packed, whatever the moon's values.
        let grid = grid_at(48_000.0);
        let packed = |t: &AirTune| pack(&grid, CLEAR, &t.phase().normed(), t, 0.4);
        for source in [AirSource::Sun, AirSource::None] {
            assert_eq!(t.lit_by(source), day);
            assert_eq!(packed(&t.lit_by(source)), packed(&day));
        }
        let m = t.lit_by(AirSource::Moon);
        assert_eq!((m.strength, m.rays, m.lift, m.shade, m.g, m.cap, m.sky_weight, m.sky_phase), (0.5, 6.0, 2.0, 0.7, 0.8, 8.0, 1.0, 1.0));
        assert_eq!((m.floor, m.samples, m.share, m.reference, m.depth, m.grid, m.view), (day.floor, day.samples, day.share, day.reference, day.depth, day.grid, day.view));
        assert_eq!(m.lit_by(AirSource::Moon), m, "used once");
        // The slider and the moon's factor multiply, held to 2.
        assert_eq!(AirTune { strength: 1.5, ..t }.lit_by(AirSource::Moon).strength, 0.75);
        assert_eq!(AirTune::parse("MOON_STRENGTH=2", &day.with_strength(2.0)).lit_by(AirSource::Moon).strength, 2.0);
        assert_eq!(AirTune::parse("MOON_STRENGTH=0", &day).lit_by(AirSource::Moon).strength, 0.0);
        // Ranges, `auto`, unreadable values, the change's words.
        let held = AirTune::parse("MOON_STRENGTH=9\nMOON_RAYS=99\nMOON_LIFT=0\nMOON_SHADE=-1\nMOON_G=-1\nMOON_CAP=99\nMOON_SKY=7\nMOON_SKYPHASE=7", &day).moon;
        assert_eq!(held, MoonTune { strength: 2.0, rays: Some(8.0), lift: Some(1.0), shade: Some(0.0), g: Some(0.0), cap: Some(8.0), sky_weight: Some(1.0), sky_phase: Some(1.0) });
        assert_eq!(AirTune::parse("MOON_RAYS=auto\nMOON_G=dark\nMOON_STRENGTH=auto", &t).moon, MoonTune { rays: None, ..t.moon });
        assert_eq!(tune_diff(&day, &AirTune::parse("MOON_STRENGTH=0.5\nMOON_CAP=8", &day)), "MOON_STRENGTH=0.5 MOON_CAP=8");
        assert_eq!(tune_diff(&t, &AirTune::parse("MOON_G=auto", &t)), "MOON_G=auto");
        // The line's words.
        let phase = m.phase().normed();
        let line = on_line_with(&grid, &phase, 0.37, &m, AirSource::Moon, &on_bounds(&grid, &phase, 0.37, &m, CLEAR, 1.0));
        assert!(line.contains(" units, lit by the moon, phase g 0.8, cap 8, moonlit share 0.37, strength 0.5, "), "{line}");
    }

    /// A halo round the moon can be had in the sky and on far land: with the moon's phase on the sky
    /// (MOON_SKYPHASE, MOON_SKY, MOON_G, MOON_CAP) an open ray at the moon gains about as much again as the
    /// sky's own light there, and near nothing well away from it; with the day's values (SKYPHASE 0) an open ray's
    /// sky is left alone in every direction.
    #[test]
    fn a_halo_round_the_moon_is_in_the_keys_reach() {
        let day = AirTune { moon: MoonTune::DAY, ..AirTune::start(1.0, AirView::None, AirOccluders::All, None) };
        let moon = AirTune::parse("MOON_G=0.8\nMOON_CAP=8\nMOON_SKY=1\nMOON_SKYPHASE=1", &day).lit_by(AirSource::Moon);
        // A clear night's order of share and fog at the grid's end, an open ray.
        let sky = |t: &AirTune, degrees: f32, blend: f32| {
            let p = phase_floor(t.phase().normed().phase(degrees.to_radians().cos()), t.floor);
            blended_correction(1.0, p, t.sky_phase, applied_scale(t.strength, 0.37), 0.42, t.sky_weight, blend)
        };
        for blend in [1.0, 0.5] {
            let at_moon = sky(&moon, 0.0, blend);
            assert!(at_moon > 0.9, "{blend}: {at_moon}");
            let mut last = at_moon;
            for d in (5..=180).step_by(5) {
                let now = sky(&moon, d as f32, blend);
                assert!(now <= last + 1e-5 && now >= 0.0, "{blend}, {d}: {now}");
                last = now;
            }
            assert!(sky(&moon, 45.0, blend) < 0.2 * at_moon, "{}", sky(&moon, 45.0, blend));
        }
        for d in [0.0, 20.0, 90.0] {
            assert_eq!(sky(&day.lit_by(AirSource::Moon), d, 1.0), 0.0);
        }
    }

    #[test]
    fn the_gate_log_holds_off_reasons() {
        let mut log = GateLog::default();
        // At start: the fog not yet patched for 0.2 s, then on: no off line at all.
        assert_eq!(log.step(Err(AirOff::LinearBlend), 0.0), None);
        assert_eq!(log.step(Err(AirOff::LinearBlend), 0.2), None);
        let (sun, moon) = (Ok(AirSource::Sun), Ok(AirSource::Moon));
        assert_eq!(log.step(sun, 0.25), Some(sun));
        assert_eq!(log.step(sun, 0.3), None);
        // An off reason that holds 0.5 s is logged once.
        assert_eq!(log.step(Err(AirOff::Night), 10.0), None);
        assert_eq!(log.step(Err(AirOff::Night), 10.4), None);
        assert_eq!(log.step(Err(AirOff::Night), 10.5), Some(Err(AirOff::Night)));
        assert_eq!(log.step(Err(AirOff::Night), 11.0), None);
        // A new reason starts its own hold; on again logs at once.
        assert_eq!(log.step(Err(AirOff::NoShare), 12.0), None);
        assert_eq!(log.step(Err(AirOff::Indoors), 12.3), None);
        assert_eq!(log.step(Err(AirOff::Indoors), 12.8), Some(Err(AirOff::Indoors)));
        assert_eq!(log.step(sun, 13.0), Some(sun));
        // Another source is said at once, through the frame at the horizon whose share is 0.
        assert_eq!(log.step(Err(AirOff::NoShare), 13.1), None);
        assert_eq!(log.step(moon, 13.2), Some(moon));
        assert_eq!(log.step(moon, 13.3), None);
        assert_eq!(log.step(sun, 20.0), Some(sun));
    }

    /// RAYS amplifies the shadow's depth but keeps every bound: Vbar_c = clamp(1 - k (1 -
    /// Vbar), 0, 1) lies in 0..=1 for any k >= 0, so (world) R_c = Vbar_c x p lies in 0..=p,
    /// within 0..=most, the correction a f_near (R_c - 1) in [-f, (most - 1) f] and the
    /// composed pixel keeps the floor (1 - f) S'; (sky) Vbar_c in place of Vbar keeps
    /// 1 - a F <= 1 + corr_sky <= 1 + a F (most - 1) (SKYPHASE 1) and <= 1 (SKYPHASE 0).
    #[test]
    fn the_ray_contrast_keeps_every_bound() {
        let phase = AirPhase::default().normed();
        let most = phase.most();
        let gamma = |v: f32| v.max(0.0).powf(1.0 / 2.2);
        let colour = Vec3::new(0.8, 0.7, 0.6);
        for rays in [0.0f32, 1.0, 2.5, 8.0] {
            for i in 0..=20 {
                let vbar = i as f32 / 20.0;
                let vc = ray_contrast(vbar, rays);
                assert!((0.0..=1.0).contains(&vc), "{rays} {vbar}: {vc}");
                // Monotone in Vbar, and never lighter than Vbar for RAYS >= 1.
                assert!(vc <= ray_contrast((vbar + 0.05).min(1.0), rays));
                if rays >= 1.0 {
                    assert!(vc <= vbar + 1e-6);
                }
                for p in [phase.phase(-1.0), 1.0, most] {
                    // The world ratio from sums with that lit fraction: N = Vbar M.
                    let m = 0.3f32;
                    let r = applied_ratio(ray_contrast(lit_fraction(vbar * m, m), rays), p, 0.0);
                    assert!((0.0..=most + 1e-6).contains(&r), "{rays} {vbar} {p}: {r}");
                    for (a, f) in [(1.0f32, 0.185f32), (0.35, 0.77), (1.0, 1.0)] {
                        let c = correction(f, r, a);
                        assert!(c >= -f - 1e-6 && c <= (most - 1.0) * f + 1e-6, "{c}");
                        for s in [0.0f32, 0.5, 4.0] {
                            let yd = Vec3::splat((1.0 - f) * gamma(s)) + colour.powf(1.0 / 2.2) * f;
                            let out = compose_ratio(yd.powf(2.2), c, colour).powf(1.0 / 2.2);
                            assert!(out.min_element() >= (1.0 - f) * gamma(s) - 1e-5 * yd.max_element().max(1.0));
                        }
                        let one = 1.0 + sky_modulation(vc, p, 1.0, a, f, 1.0);
                        assert!(one >= 1.0 - a * f - 1e-6 && one <= 1.0 + a * f * (most - 1.0) + 1e-6);
                        let zero = 1.0 + sky_modulation(vc, p, 0.0, a, f, 1.0);
                        assert!(zero >= 1.0 - a * f - 1e-6 && zero <= 1.0);
                    }
                }
            }
        }
        // RAYS 1 is the physical Vbar; RAYS 0 takes the air as all lit; a strong RAYS darkens.
        assert_eq!(ray_contrast(0.8, 1.0), 0.8);
        assert_eq!(ray_contrast(0.3, 0.0), 1.0);
        assert!((ray_contrast(0.85, 4.0) - 0.4).abs() < 1e-6);
        assert_eq!(ray_contrast(0.85, 8.0), 0.0);
        assert_eq!(ray_contrast(f32::NAN, 4.0), 1.0);
        assert_eq!(ray_contrast(0.6, f32::NAN), 0.6);
        // The keys.
        let t = AirTune::parse("RAYS=4\nSAMPLES=6", &AirTune::default());
        assert_eq!((t.rays, t.samples), (4.0, 6));
        let held = AirTune::parse("RAYS=99\nSAMPLES=0", &AirTune::default());
        assert_eq!((held.rays, held.samples), (8.0, 1));
        assert_eq!(AirTune::parse("SAMPLES=2.6\nRAYS=-1", &AirTune::default()).samples, 3);
        assert_eq!(AirTune::parse("RAYS=-1", &AirTune::default()).rays, 0.0);
        assert_eq!(AirTune::parse("SAMPLES=many", &AirTune::default()).samples, 4);
        assert_eq!((AirTune::default().rays, AirTune::default().samples), (3.0, 4));
        assert_eq!(tune_diff(&AirTune::default(), &t), "RAYS=4 SAMPLES=6");
        // The phase floor (FLOOR 1 by default): a fully lit sample's correction is >= 0 and
        // <= a F (most - 1) everywhere; shadow still takes at most a F. p_eff = max(p, 1) is
        // in [1, most], R_c = Vbar_c p_eff in [0, most], and Vbar_c = 1 when fully lit.
        assert_eq!(AirTune::default().floor, 1.0);
        for p in [phase.phase(-1.0), phase.phase(0.0), 1.0, phase.phase(0.8), most] {
            let pe = phase_floor(p, 1.0);
            assert!((1.0..=most).contains(&pe), "{p}: {pe}");
            for rays in [1.0f32, 4.0] {
                for (a, f) in [(1.0f32, 0.185f32), (0.35, 0.77), (1.0, 1.0)] {
                    let lit = correction(f, applied_ratio(ray_contrast(1.0, rays), p, 1.0), a);
                    assert!(lit >= -1e-6 && lit <= a * f * (most - 1.0) + 1e-6, "{p} {rays}: {lit}");
                    let dark = correction(f, applied_ratio(ray_contrast(0.0, rays), p, 1.0), a);
                    assert!((dark + a * f).abs() < 1e-6, "{dark}");
                    // The sky with the floored phase in its mix: lit and SKYPHASE 1 never darkens.
                    assert!(sky_modulation(1.0, pe, 1.0, a, f, 1.0) >= 0.0);
                }
            }
        }
        assert_eq!(phase_floor(0.4, 0.0), 0.4);
        assert_eq!(phase_floor(0.4, 0.7), 0.7);
        assert_eq!(phase_floor(1.3, 1.0), 1.3);
        assert_eq!(phase_floor(0.4, 5.0), 1.0);
        assert_eq!(phase_floor(f32::NAN, 0.0), 1.0);
        assert_eq!(phase_floor(0.4, f32::NAN), 1.0);
        let fl = AirTune::parse("FLOOR=0.25", &AirTune::default());
        assert_eq!(fl.floor, 0.25);
        assert_eq!(AirTune::parse("FLOOR=3", &AirTune::default()).floor, 1.0);
        assert_eq!(tune_diff(&AirTune::default(), &fl), "FLOOR=0.25");
        // FLOOR rides in light.w as 2 - FLOOR (1 at the default: the old flag).
        assert_eq!(light_flag(true, 1.0), 1.0);
        assert_eq!(light_flag(true, 0.0), 2.0);
        assert_eq!(light_flag(false, 0.3), 0.0);
        assert_eq!(light_flag(true, f32::NAN), 1.0);
        for f in [0.0f32, 0.25, 0.5, 1.0] {
            assert_eq!(floor_from_flag(light_flag(true, f)), f);
            assert!(light_flag(true, f) > 0.5);
        }
        // RAYS rides in the uniform's curve.w (the superseded fade_from slot).
        let g = grid_at(12_000.0);
        assert_eq!(pack(&g, CLEAR, &t.phase().normed(), &t, 0.5).curve[3], 4.0);
    }

    /// The slice width is the slope of z(c), and the composite's one-tap rule.
    #[test]
    fn samples_agree_within_a_quarter_slice() {
        let g = grid_at(12_000.0);
        let curve = g.curve();
        // dz/dc against a central difference of depth_at, at several slices.
        for c in [0.5f32, 10.0, 40.0, 63.0] {
            let z = curve.depth_at(c);
            let slope = (curve.depth_at(c + 0.01) - curve.depth_at(c - 0.01)) / 0.02;
            assert!((curve.slice_width(z) - slope).abs() / slope < 2e-3, "{c}: {} vs {slope}", curve.slice_width(z));
        }
        // Linear: depth / nz.
        assert!((grid_at(2_000.0).curve().slice_width(500.0) - 31.25).abs() < 1e-4);
        // The first slice is 50 units: at z 0 a quarter slice is about 12.3 (the slope at the
        // camera is first x ln r / (r - 1) = 49.1, a little under 50).
        let near = curve.slice_width(0.0) * AIR_AGREE_SLICES;
        assert!(near > 12.0 && near < 12.6, "{near}");
        assert!(slices_agree(&[10.0, 10.0 + near * 0.9], &curve, AIR_AGREE_SLICES));
        assert!(!slices_agree(&[10.0, 10.0 + near * 2.0], &curve, AIR_AGREE_SLICES));
        // Far: a quarter slice at 10,000 is about 1.0 % of the depth (ln r 0.0354).
        let far = curve.slice_width(10_000.0) * AIR_AGREE_SLICES;
        assert!((far / 10_000.0 - 0.0354 * (1.0 + 1_389.0 / 10_000.0) / 4.0).abs() < 2e-4, "{far}");
        // Beyond the grid's end everything agrees (one value is carried), cleared depth too
        // (its view depth is the grid's end): only the lookup is shared, never the fog.
        assert!(slices_agree(&[13_000.0, 90_000.0, 12_500.0], &curve, AIR_AGREE_SLICES));
        assert!(slices_agree(&[13_000.0, g.depth], &curve, AIR_AGREE_SLICES));
        // A silhouette: a rock at 300 against ground at 900 spans slices.
        assert!(!slices_agree(&[300.0, 900.0, 300.0, 300.0], &curve, AIR_AGREE_SLICES));
        assert!(slices_agree(&[], &curve, AIR_AGREE_SLICES));
    }

    /// The relative lit fraction: V_ref 1 is the old rule; average air (Vbar = V_ref) is
    /// exactly 1 whatever RAYS; monotone in Vbar; 0..=LIFT; the bounds: shadow takes at most
    /// a F, a world sample gains at most a F (LIFT x most - 1), a sky sample at SKYPHASE 0 at
    /// most a F (LIFT - 1).
    #[test]
    fn the_relative_lit_fraction_is_average_neutral_and_bounded() {
        let phase = AirPhase::default().normed();
        let most = phase.most();
        for rays in [0.0f32, 1.0, 2.5, 8.0] {
            for i in 0..=20 {
                let vbar = i as f32 / 20.0;
                // V_ref 1 with any LIFT >= 1: the old ray contrast, exactly.
                for lift in [1.0f32, 1.5, 3.0] {
                    assert_eq!(relative_lit(vbar, 1.0, rays, lift, 0.0), ray_contrast(vbar, rays), "{rays} {vbar} {lift}");
                }
            }
            for v_ref in [0.25f32, 0.6, 0.9] {
                // Average air is unchanged.
                assert_eq!(relative_lit(v_ref, v_ref, rays, 1.5, 0.0), 1.0);
                assert_eq!(relative_lit(v_ref, v_ref, rays, 1.75, 0.5), 1.0);
                let mut last = 0.0f32;
                for i in 0..=40 {
                    let vbar = i as f32 / 40.0;
                    let vc = relative_lit(vbar, v_ref, rays, 1.5, 0.0);
                    assert!((0.0..=1.5).contains(&vc) && vc >= last, "{rays} {v_ref} {vbar}: {vc}");
                    last = vc;
                    for p in [phase.phase(-1.0), 1.0, most] {
                        for floor in [0.0f32, 1.0] {
                            for (a, f) in [(1.0f32, 0.185f32), (0.35, 0.77), (1.0, 1.0)] {
                                let c = correction(f, applied_ratio(vc, p, floor), a);
                                assert!(c >= -a * f - 1e-6 && c <= a * f * (1.5 * most - 1.0) + 1e-6, "{c}");
                                let sky = sky_modulation(vc, phase_floor(p, floor), 0.0, a, f, 1.0);
                                assert!(sky >= -a * f - 1e-6 && sky <= a * f * 0.5 + 1e-6, "{sky}");
                            }
                        }
                    }
                }
            }
        }
        // Fully lit air under a broken sky (V_ref 0.6) rises; RAYS 1: 1 + 0.4 / 0.6 = 1.667,
        // held to LIFT 1.5.
        assert_eq!(relative_lit(1.0, 0.6, 1.0, 1.5, 0.0), 1.5);
        assert!((relative_lit(1.0, 0.6, 1.0, 3.0, 0.0) - 1.0 / 0.6).abs() < 1e-6);
        assert_eq!(relative_lit(0.0, 0.6, 1.0, 1.5, 0.0), 0.0);
        assert_eq!(relative_lit(f32::NAN, 0.6, f32::NAN, f32::NAN, f32::NAN), 1.0);
        // The reference: set, auto, auto with nothing published, held.
        assert_eq!(reference_visibility(Some(0.6), 0.3), 0.6);
        assert_eq!(reference_visibility(Some(0.01), 0.3), 0.05);
        assert_eq!(reference_visibility(None, 0.7), 0.7);
        assert_eq!(reference_visibility(None, 0.1), 0.25);
        assert_eq!(reference_visibility(None, 0.0), 1.0);
        assert_eq!(reference_visibility(None, f32::NAN), 1.0);
        assert_eq!(reference_visibility(None, f32::INFINITY), 1.0);
        assert_eq!(reference_visibility(Some(f32::NAN), 0.7), 0.7);
        // The keys.
        let t = AirTune::parse("REF=0.6\nLIFT=2", &AirTune::default());
        assert_eq!((t.reference, t.lift), (Some(0.6), 2.0));
        assert_eq!(AirTune::parse("REF=auto", &t).reference, None);
        let held = AirTune::parse("REF=0\nLIFT=9", &AirTune::default());
        assert_eq!((held.reference, held.lift), (Some(0.05), 3.0));
        assert_eq!((AirTune::default().reference, AirTune::default().lift, AirTune::default().shade), (None, 1.4, 0.9));
        assert_eq!(tune_diff(&AirTune::default(), &t), "LIFT=2 REF=0.6");
        assert_eq!(tune_diff(&t, &AirTune::parse("REF=auto\nLIFT=2", &t)), "REF=auto");
    }

    /// SHADE limits what shadow takes: Vbar_c >= SHADE, so a world sample's correction is at
    /// least a F (SHADE p_eff - 1) (with FLOOR 1 and SHADE 0.5: shadow takes at most half the
    /// fog's light) and the sky factor at SKYPHASE 0 is at least 1 - a F (1 - SHADE); SHADE 0
    /// is the rule without it; average air is still exactly 1.
    #[test]
    fn the_shade_limit_bounds_what_shadow_takes() {
        let phase = AirTune::default().phase().normed();
        let most = phase.most();
        for shade in [0.0f32, 0.25, 0.5, 1.0] {
            for v_ref in [0.6f32, 0.84, 1.0] {
                for rays in [1.0f32, 4.0, 8.0] {
                    for i in 0..=20 {
                        let vbar = i as f32 / 20.0;
                        let vc = relative_lit(vbar, v_ref, rays, 1.75, shade);
                        assert!(vc >= shade && vc <= 1.75, "{shade} {v_ref} {rays} {vbar}: {vc}");
                        if shade == 0.0 {
                            assert_eq!(vc, (1.0 + rays * (vbar - v_ref) / v_ref).clamp(0.0, 1.75));
                        }
                        for p in [phase.phase(-1.0), 1.0, most] {
                            for floor in [0.0f32, 1.0] {
                                let pe = phase_floor(p, floor);
                                for (a, f) in [(1.0f32, 0.185f32), (0.35, 0.77), (1.0, 1.0)] {
                                    let c = correction(f, applied_ratio(vc, p, floor), a);
                                    assert!(c >= a * f * (shade * pe - 1.0) - 1e-6, "{c}");
                                    let sky = 1.0 + sky_modulation(vc, pe, 0.0, a, f, 1.0);
                                    assert!(sky >= 1.0 - a * f * (1.0 - shade) - 1e-6, "{sky}");
                                }
                            }
                        }
                    }
                    assert_eq!(relative_lit(v_ref, v_ref, rays, 1.75, shade), 1.0_f32.max(shade));
                }
            }
        }
        // FLOOR 1 and SHADE 0.5: full shadow takes exactly half (p_eff >= 1 gives at most half).
        let c = correction(0.4, applied_ratio(relative_lit(0.0, 0.84, 4.0, 1.75, 0.5), 1.0, 1.0), 1.0);
        assert!((c + 0.2).abs() < 1e-6, "{c}");
        // The key, its range and the uniform's slot (rays2.z, packed in medium_air_grid.rs).
        let t = AirTune::parse("SHADE=0.2", &AirTune::default());
        assert_eq!(t.shade, 0.2);
        assert_eq!(AirTune::parse("SHADE=3", &AirTune::default()).shade, 1.0);
        assert_eq!(tune_diff(&AirTune::default(), &t), "SHADE=0.2");
        assert_eq!(relative_lit(0.0, 0.84, 4.0, 1.75, f32::NAN), 0.0);
        // The grid's default depth is 48,000 (q 1.067636, last slice 3,088).
        assert_eq!(AIR_DEPTH, 48_000.0);
        let g = AirGrid::for_view(16.0 / 9.0, None, None);
        assert!((g.ratio() - 1.067636).abs() < 1e-5, "{}", g.ratio());
    }
}
