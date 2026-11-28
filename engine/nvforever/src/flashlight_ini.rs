//! Clean-room loader for `Data\Config\BetterFlashlightNVSE.ini`
//! (`docs/spec-notes/better-flashlight-ini.md`).
//!
//! Only the public **Better Flashlight NVSE** v1.1.5 Nexus page
//! (`https://www.nexusmods.com/newvegas/mods/98905`) and shipped v1.1.5
//! `Data\Config\BetterFlashlightNVSE.ini` section/key names, defaults, and inline
//! comments were consulted; no DLL (`BetterFlashlightNVSE.dll`), PDB, shader binary
//! (`.fxo`), or third-party implementation source was inspected.
//!
//! - Lookup runs once at `Startup` through `nvf_assets::GameData::read` (case-insensitive
//!   VFS lookup covering loose files, MO2 layered roots, and archives).
//! - When the INI is absent, or when `[Main] RenderEffects = 0` or `[Flashlight] Enabled = 0`,
//!   NVForever's vanilla Pip-Boy flashlight stays completely unchanged [design].
//! - Parsing operates directly on raw `&[u8]` slices (no `&str` byte-index slicing) so
//!   UTF-8 BOMs, CRLF endings, `;`/`#` comments, non-ASCII text, and invalid UTF-8 bytes
//!   never panic [design].
//! - Unbuilt D3D9 / post-process / HUD sections and keys (`[Hotkeys]`, `[EngineLight]`,
//!   `[MaterialLight]`, `[Volumetric]`, `SoftEdges`, `DisableNVRFlashlight`,
//!   `RenderShadows`, `NearFade`, `HotspotLimit`, `CookieStrength`, and `[Battery]` HUD
//!   icon keys) are skipped cleanly rather than kept as dead struct fields.

use bevy::prelude::*;

/// Case-insensitive VFS path for `Data\Config\BetterFlashlightNVSE.ini`
/// [documented: Better Flashlight NVSE v1.1.5 (`https://www.nexusmods.com/newvegas/mods/98905`),
/// shipped `Data\Config\BetterFlashlightNVSE.ini` install path].
pub const INI_PATH: &str = "config\\betterflashlightnvse.ini";

/// Vanilla inner/outer half-angle softness ratio from `crate::flashlight` (`14 / 30`) [design].
const INNER_OUTER_RATIO: f32 = 14.0 / 30.0;

/// Safe clamp ceilings for native Bevy `SpotLight` mapping [design].
const MIN_OUTER_DEG: f32 = 1.0;
const MAX_OUTER_DEG: f32 = 85.0; // Strictly below Bevy's pi/2 SpotLight outer_angle limit.
const MIN_SPOT_SIZE: f32 = 0.1;
const MAX_SPOT_SIZE: f32 = 4.0;
const MIN_DISTANCE: f32 = 64.0;
const MAX_DISTANCE: f32 = 20_000.0;
const MAX_COLOR: f32 = 4.0;
const MAX_BRIGHTNESS_TERM: f32 = 16.0;
const MAX_WARMUP_SECS: f32 = 10.0;
const MAX_FLICKER: f32 = 2.0;
const MAX_OFFSET: f32 = 200.0;
const MAX_BATTERY_RATE: f32 = 100.0;

/// Brightness profile for `[Interior]` or `[Exterior]` (resolved with `[Flashlight]`
/// legacy fallback).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BrightnessProfile {
    pub dimmer: f32,
    pub intensity: f32,
}

/// `[Battery]` drain, recharge, and post-depletion lockout settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BatteryConfig {
    pub enabled: bool,
    /// Charge points (out of 100) drained per second while the flashlight is on.
    pub drain_rate: f32,
    /// Charge points (out of 100) recharged per second while the flashlight is off.
    pub recharge_rate: f32,
    /// Minimum charge (0..=100) required after hitting 0 before turning back on.
    pub recharge_threshold: f32,
}

impl Default for BatteryConfig {
    /// Fallback defaults when `[Battery]` keys are omitted: `enabled = false` [design:
    /// off unless explicitly enabled] and numeric rates matching the shipped INI baseline.
    fn default() -> Self {
        Self {
            enabled: false,
            drain_rate: 1.0,
            recharge_rate: 2.5,
            recharge_threshold: 10.0,
        }
    }
}

/// Parsed `Data\Config\BetterFlashlightNVSE.ini` configuration
/// [documented: Better Flashlight NVSE v1.1.5 (`https://www.nexusmods.com/newvegas/mods/98905`),
/// shipped `Data\Config\BetterFlashlightNVSE.ini`; plugin DLL and source code were never
/// consulted].
///
/// Every field here is wired into `crate::flashlight::sync`.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct BetterFlashlightConfig {
    /// `[Main] RenderEffects`
    pub render_effects: bool,
    /// `[Flashlight] Enabled`
    pub enabled: bool,
    /// `[Flashlight] AttachToWeapon`
    pub attach_to_weapon: bool,
    /// `[Flashlight] Angle` (base cone half-angle in degrees [hypothesis])
    pub angle_deg: f32,
    /// `[Flashlight] SpotSize` (cone scale multiplier; `1.0 = full Angle`)
    pub spot_size: f32,
    /// `[Flashlight] Distance` (reach in game units)
    pub distance: f32,
    /// `[Flashlight] ColorR`, `ColorG`, `ColorB` (HDR-capable RGB multipliers)
    pub color: [f32; 3],
    /// `[Interior]` brightness profile (with `[Flashlight]` fallback)
    pub interior: BrightnessProfile,
    /// `[Exterior]` brightness profile (with `[Flashlight]` fallback)
    pub exterior: BrightnessProfile,
    /// `[Flashlight] WarmupTime` (seconds; 0 = instant)
    pub warmup_time: f32,
    /// `[Flashlight] Flicker` (0 = off; higher = deeper and more frequent sporadic dips)
    pub flicker: f32,
    /// `[Flashlight] OffsetX`, `OffsetY`, `OffsetZ` (local game-unit `(right, forward, up)`
    /// offset from attach point [hypothesis: FNV axis convention])
    pub offset: [f32; 3],
    /// `[Battery]` settings
    pub battery: BatteryConfig,
}

impl Default for BetterFlashlightConfig {
    /// Omitted-key fallback defaults [design: `spot_size = 1.0` full-angle scale and
    /// `battery.enabled = false` when omitted; remaining numeric defaults match shipped
    /// `Data\Config\BetterFlashlightNVSE.ini`].
    fn default() -> Self {
        let legacy = BrightnessProfile { dimmer: 1.0, intensity: 0.35 };
        Self {
            render_effects: true,
            enabled: true,
            attach_to_weapon: true,
            angle_deg: 18.0,
            spot_size: 1.0,
            distance: 1800.0,
            color: [2.0, 1.85, 1.65],
            interior: legacy,
            exterior: legacy,
            warmup_time: 0.4,
            flicker: 0.25,
            offset: [0.0, 0.0, 0.0],
            battery: BatteryConfig::default(),
        }
    }
}

impl BetterFlashlightConfig {
    /// Whether this INI overrides NVForever's vanilla Pip-Boy flashlight.
    pub fn is_active(&self) -> bool {
        self.render_effects && self.enabled
    }

    /// Active brightness profile for the current cell type.
    pub fn profile(&self, interior: bool) -> BrightnessProfile {
        if interior { self.interior } else { self.exterior }
    }

    /// Inner and outer Bevy `SpotLight` half-angles in radians [design].
    pub fn cone_angles_rad(&self) -> (f32, f32) {
        let outer_deg = (self.angle_deg * self.spot_size).clamp(MIN_OUTER_DEG, MAX_OUTER_DEG);
        let outer_rad = outer_deg.to_radians();
        let inner_rad = (outer_deg * INNER_OUTER_RATIO).to_radians().min(outer_rad * 0.95);
        (inner_rad, outer_rad)
    }

    /// Parse raw INI bytes strictly on `&[u8]` slices.
    pub fn parse(bytes: &[u8]) -> Self {
        let mut cfg = Self::default();
        let mut legacy_dimmer = cfg.interior.dimmer;
        let mut legacy_intensity = cfg.interior.intensity;
        let mut interior_dimmer: Option<f32> = None;
        let mut interior_intensity: Option<f32> = None;
        let mut exterior_dimmer: Option<f32> = None;
        let mut exterior_intensity: Option<f32> = None;

        let body = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
        let mut section = Section::None;

        for raw_line in body.split(|&b| b == b'\n') {
            let line = strip_comment(raw_line.strip_suffix(b"\r").unwrap_or(raw_line));
            let line = trim_ascii(line);
            if line.is_empty() {
                continue;
            }
            if line[0] == b'[' {
                if let Some(end) = line.iter().position(|&b| b == b']') {
                    let name = trim_ascii(&line[1..end]);
                    section = Section::from_bytes(name);
                }
                continue;
            }
            let Some(eq) = line.iter().position(|&b| b == b'=') else {
                continue;
            };
            let key = trim_ascii(&line[..eq]);
            let val = trim_ascii(&line[eq + 1..]);
            if key.is_empty() || val.is_empty() {
                continue;
            }

            match section {
                Section::Main => {
                    if key.eq_ignore_ascii_case(b"RenderEffects") {
                        if let Some(b) = parse_bool(val) {
                            cfg.render_effects = b;
                        }
                    }
                }
                Section::Flashlight => {
                    if key.eq_ignore_ascii_case(b"Enabled") {
                        if let Some(b) = parse_bool(val) {
                            cfg.enabled = b;
                        }
                    } else if key.eq_ignore_ascii_case(b"AttachToWeapon") {
                        if let Some(b) = parse_bool(val) {
                            cfg.attach_to_weapon = b;
                        }
                    } else if key.eq_ignore_ascii_case(b"Angle") {
                        if let Some(v) = parse_positive_f32(val) {
                            cfg.angle_deg = v.clamp(MIN_OUTER_DEG, MAX_OUTER_DEG);
                        }
                    } else if key.eq_ignore_ascii_case(b"SpotSize") {
                        if let Some(v) = parse_positive_f32(val) {
                            cfg.spot_size = v.clamp(MIN_SPOT_SIZE, MAX_SPOT_SIZE);
                        }
                    } else if key.eq_ignore_ascii_case(b"Distance") {
                        if let Some(v) = parse_positive_f32(val) {
                            cfg.distance = v.clamp(MIN_DISTANCE, MAX_DISTANCE);
                        }
                    } else if key.eq_ignore_ascii_case(b"ColorR") {
                        if let Some(v) = parse_f32(val) {
                            cfg.color[0] = v.clamp(0.0, MAX_COLOR);
                        }
                    } else if key.eq_ignore_ascii_case(b"ColorG") {
                        if let Some(v) = parse_f32(val) {
                            cfg.color[1] = v.clamp(0.0, MAX_COLOR);
                        }
                    } else if key.eq_ignore_ascii_case(b"ColorB") {
                        if let Some(v) = parse_f32(val) {
                            cfg.color[2] = v.clamp(0.0, MAX_COLOR);
                        }
                    } else if key.eq_ignore_ascii_case(b"Dimmer") {
                        if let Some(v) = parse_f32(val) {
                            legacy_dimmer = v.clamp(0.0, MAX_BRIGHTNESS_TERM);
                        }
                    } else if key.eq_ignore_ascii_case(b"Intensity") {
                        if let Some(v) = parse_f32(val) {
                            legacy_intensity = v.clamp(0.0, MAX_BRIGHTNESS_TERM);
                        }
                    } else if key.eq_ignore_ascii_case(b"WarmupTime") {
                        if let Some(v) = parse_f32(val) {
                            cfg.warmup_time = v.clamp(0.0, MAX_WARMUP_SECS);
                        }
                    } else if key.eq_ignore_ascii_case(b"Flicker") {
                        if let Some(v) = parse_f32(val) {
                            cfg.flicker = v.clamp(0.0, MAX_FLICKER);
                        }
                    } else if key.eq_ignore_ascii_case(b"OffsetX") {
                        if let Some(v) = parse_f32(val) {
                            cfg.offset[0] = v.clamp(-MAX_OFFSET, MAX_OFFSET);
                        }
                    } else if key.eq_ignore_ascii_case(b"OffsetY") {
                        if let Some(v) = parse_f32(val) {
                            cfg.offset[1] = v.clamp(-MAX_OFFSET, MAX_OFFSET);
                        }
                    } else if key.eq_ignore_ascii_case(b"OffsetZ") {
                        if let Some(v) = parse_f32(val) {
                            cfg.offset[2] = v.clamp(-MAX_OFFSET, MAX_OFFSET);
                        }
                    }
                }
                Section::Interior => {
                    if key.eq_ignore_ascii_case(b"Dimmer") {
                        if let Some(v) = parse_f32(val) {
                            interior_dimmer = Some(v.clamp(0.0, MAX_BRIGHTNESS_TERM));
                        }
                    } else if key.eq_ignore_ascii_case(b"Intensity") {
                        if let Some(v) = parse_f32(val) {
                            interior_intensity = Some(v.clamp(0.0, MAX_BRIGHTNESS_TERM));
                        }
                    }
                }
                Section::Exterior => {
                    if key.eq_ignore_ascii_case(b"Dimmer") {
                        if let Some(v) = parse_f32(val) {
                            exterior_dimmer = Some(v.clamp(0.0, MAX_BRIGHTNESS_TERM));
                        }
                    } else if key.eq_ignore_ascii_case(b"Intensity") {
                        if let Some(v) = parse_f32(val) {
                            exterior_intensity = Some(v.clamp(0.0, MAX_BRIGHTNESS_TERM));
                        }
                    }
                }
                Section::Battery => {
                    if key.eq_ignore_ascii_case(b"Enabled") {
                        if let Some(b) = parse_bool(val) {
                            cfg.battery.enabled = b;
                        }
                    } else if key.eq_ignore_ascii_case(b"DrainRate") {
                        if let Some(v) = parse_f32(val) {
                            cfg.battery.drain_rate = v.clamp(0.0, MAX_BATTERY_RATE);
                        }
                    } else if key.eq_ignore_ascii_case(b"RechargeRate") {
                        if let Some(v) = parse_f32(val) {
                            cfg.battery.recharge_rate = v.clamp(0.0, MAX_BATTERY_RATE);
                        }
                    } else if key.eq_ignore_ascii_case(b"RechargeThreshold") {
                        if let Some(v) = parse_f32(val) {
                            cfg.battery.recharge_threshold = v.clamp(0.0, 100.0);
                        }
                    }
                }
                Section::None | Section::Other => {}
            }
        }

        cfg.interior = BrightnessProfile {
            dimmer: interior_dimmer.unwrap_or(legacy_dimmer),
            intensity: interior_intensity.unwrap_or(legacy_intensity),
        };
        cfg.exterior = BrightnessProfile {
            dimmer: exterior_dimmer.unwrap_or(legacy_dimmer),
            intensity: exterior_intensity.unwrap_or(legacy_intensity),
        };
        cfg
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    None,
    Main,
    Flashlight,
    Interior,
    Exterior,
    Battery,
    Other,
}

impl Section {
    fn from_bytes(name: &[u8]) -> Self {
        if name.eq_ignore_ascii_case(b"Main") {
            Self::Main
        } else if name.eq_ignore_ascii_case(b"Flashlight") {
            Self::Flashlight
        } else if name.eq_ignore_ascii_case(b"Interior") {
            Self::Interior
        } else if name.eq_ignore_ascii_case(b"Exterior") {
            Self::Exterior
        } else if name.eq_ignore_ascii_case(b"Battery") {
            Self::Battery
        } else {
            Self::Other
        }
    }
}

fn strip_comment(line: &[u8]) -> &[u8] {
    match line.iter().position(|&b| b == b';' || b == b'#') {
        Some(pos) => &line[..pos],
        None => line,
    }
}

fn trim_ascii(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|&b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
        .unwrap_or(bytes.len());
    let rest = &bytes[start..];
    let end = rest
        .iter()
        .rposition(|&b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
        .map_or(0, |i| i + 1);
    &rest[..end]
}

fn parse_f32(bytes: &[u8]) -> Option<f32> {
    std::str::from_utf8(bytes)
        .ok()?
        .parse::<f32>()
        .ok()
        .filter(|n| n.is_finite())
}

fn parse_positive_f32(bytes: &[u8]) -> Option<f32> {
    parse_f32(bytes).filter(|&n| n > 0.0)
}

fn parse_bool(bytes: &[u8]) -> Option<bool> {
    if bytes.eq_ignore_ascii_case(b"true")
        || bytes.eq_ignore_ascii_case(b"yes")
        || bytes.eq_ignore_ascii_case(b"on")
    {
        return Some(true);
    }
    if bytes.eq_ignore_ascii_case(b"false")
        || bytes.eq_ignore_ascii_case(b"no")
        || bytes.eq_ignore_ascii_case(b"off")
    {
        return Some(false);
    }
    parse_f32(bytes).map(|n| n != 0.0)
}

/// Startup system: reads `Data\Config\BetterFlashlightNVSE.ini` from the mounted VFS if
/// present, leaving NVForever's vanilla flashlight untouched when absent.
pub(crate) fn load_from_vfs(
    mut commands: Commands,
    data: Option<Res<nvf_assets::GameData>>,
    existing: Option<Res<BetterFlashlightConfig>>,
) {
    if existing.is_some() {
        return;
    }
    let Some(bytes) = data.and_then(|d| d.read(INI_PATH)) else {
        return;
    };
    let cfg = BetterFlashlightConfig::parse(&bytes);
    if cfg.is_active() {
        let (_, outer) = cfg.cone_angles_rad();
        info!(
            "flashlight: loaded {INI_PATH} (active, range={:.0}, outer={:.1}deg, attach_weapon={}, battery={})",
            cfg.distance,
            outer.to_degrees(),
            cfg.attach_to_weapon,
            cfg.battery.enabled
        );
    } else {
        info!(
            "flashlight: loaded {INI_PATH} (inactive: RenderEffects={}, Enabled={}; keeping vanilla Pip-Boy light)",
            cfg.render_effects, cfg.enabled
        );
    }
    commands.insert_resource(cfg);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn parses_full_public_ini_layout_and_ignores_unbuilt_sections() {
        let ini = concat!(
            ";Better Flashlight NVSE config\r\n",
            "[Main]\r\n",
            "RenderEffects = 1\r\n",
            "[Flashlight]\r\n",
            "Enabled = 1\r\n",
            "AttachToWeapon = 1\r\n",
            "SoftEdges = 1\r\n",
            "DisableNVRFlashlight = 1\r\n",
            "RenderShadows = 0\r\n",
            "Angle = 18\r\n",
            "Distance = 1800\r\n",
            "SpotSize = 1.5\r\n",
            "NearFade = 150\r\n",
            "HotspotLimit = 1\r\n",
            "CookieStrength = 0.25\r\n",
            "ColorR = 2\r\n",
            "ColorG = 1.85\r\n",
            "ColorB = 1.65\r\n",
            "Dimmer = 1.0\r\n",
            "Intensity = 0.35\r\n",
            "Flicker = 0.25\r\n",
            "WarmupTime = 0.4\r\n",
            "OffsetX = 1.5\r\n",
            "OffsetY = -2.25\r\n",
            "OffsetZ = 3.0\r\n",
            "[Interior]\r\n",
            "Dimmer = 1\r\n",
            "Intensity = 0.35\r\n",
            "[Exterior]\r\n",
            "Dimmer = 3.20\r\n",
            "Intensity = 0.40\r\n",
            "[Hotkeys]\r\n",
            "DimmerIncrease = 78\r\n",
            "[EngineLight]\r\n",
            "Enabled = 0\r\n",
            "Dimmer = 0.12\r\n",
            "[MaterialLight]\r\n",
            "Enabled = 1\r\n",
            "Intensity = 0.3\r\n",
            "[Volumetric]\r\n",
            "Enabled = 1\r\n",
            "Strength = 2.5\r\n",
            "[Battery]\r\n",
            "Enabled = 1\r\n",
            "DrainRate = 1\r\n",
            "RechargeRate = 2.5\r\n",
            "RechargeThreshold = 10\r\n",
            "IconPosX = 0.97\r\n",
            "IconSet = 2\r\n",
        );

        let cfg = BetterFlashlightConfig::parse(ini.as_bytes());
        assert!(cfg.is_active());
        assert!(cfg.attach_to_weapon);
        assert_eq!(cfg.angle_deg, 18.0);
        assert_eq!(cfg.spot_size, 1.5);
        assert_eq!(cfg.distance, 1800.0);
        assert_eq!(cfg.color, [2.0, 1.85, 1.65]);
        assert_eq!(cfg.interior, BrightnessProfile { dimmer: 1.0, intensity: 0.35 });
        assert_eq!(cfg.exterior, BrightnessProfile { dimmer: 3.20, intensity: 0.40 });
        assert_eq!(cfg.warmup_time, 0.4);
        assert_eq!(cfg.flicker, 0.25);
        assert_eq!(cfg.offset, [1.5, -2.25, 3.0]);
        assert_eq!(
            cfg.battery,
            BatteryConfig {
                enabled: true,
                drain_rate: 1.0,
                recharge_rate: 2.5,
                recharge_threshold: 10.0,
            }
        );

        let (inner, outer) = cfg.cone_angles_rad();
        assert!((outer.to_degrees() - 27.0).abs() < 1e-4, "18 * 1.5 = 27 deg, got {}", outer.to_degrees());
        assert!(inner > 0.0 && inner < outer);
    }

    #[test]
    fn legacy_flashlight_dimmer_and_intensity_fill_missing_profile_keys() {
        let ini = b"\
[Flashlight]
Dimmer = 2.5
Intensity = 0.6
[Interior]
Dimmer = 0.8
[Exterior]
Intensity = 0.9
";
        let cfg = BetterFlashlightConfig::parse(ini);
        assert_eq!(cfg.interior, BrightnessProfile { dimmer: 0.8, intensity: 0.6 });
        assert_eq!(cfg.exterior, BrightnessProfile { dimmer: 2.5, intensity: 0.9 });
    }

    #[test]
    fn master_gates_disable_active_state() {
        let off_main = BetterFlashlightConfig::parse(b"[Main]\nRenderEffects = 0\n[Flashlight]\nEnabled = 1\n");
        assert!(!off_main.is_active());

        let off_light = BetterFlashlightConfig::parse(b"[Main]\nRenderEffects = 1\n[Flashlight]\nEnabled = 0\n");
        assert!(!off_light.is_active());
    }

    #[test]
    fn handles_bom_comments_case_non_ascii_invalid_utf8_nan_inf_and_negatives() {
        let mut raw = Vec::new();
        raw.extend_from_slice(b"\xEF\xBB\xBF"); // UTF-8 BOM
        raw.extend_from_slice(b"# hash comment with \xC3\xA9 and \xF0\x9F\x94\xA6\r\n");
        raw.extend_from_slice(b"; invalid utf8 in comment \xFF\xFE\x80\r\n");
        raw.extend_from_slice(b"  [mAiN] ; inline comment\r\n");
        raw.extend_from_slice(b"  rEnDeReFfEcTs = true # inline hash\r\n");
        raw.extend_from_slice(b"[fLaShLiGhT]\r\n");
        raw.extend_from_slice(b"enabled = YES\r\n");
        raw.extend_from_slice(b"attachtoweapon = false\r\n");
        // Invalid/non-finite/non-positive values for strictly positive fields are ignored.
        raw.extend_from_slice(b"Angle = NaN\r\n");
        raw.extend_from_slice(b"SpotSize = inf\r\n");
        raw.extend_from_slice(b"Distance = -500\r\n");
        raw.extend_from_slice(b"Angle = -15\r\n");
        raw.extend_from_slice(b"SpotSize = 0\r\n");
        // Negative values for non-negative fields clamp to 0; oversized values clamp to ceiling.
        raw.extend_from_slice(b"ColorR = -2.0\r\n");
        raw.extend_from_slice(b"ColorG = 99.0\r\n");
        raw.extend_from_slice(b"ColorB = \xFF\xFE\r\n");
        raw.extend_from_slice(b"Dimmer = -3.0\r\n");
        raw.extend_from_slice(b"Intensity = inf\r\n");
        raw.extend_from_slice(b"WarmupTime = -0.5\r\n");
        raw.extend_from_slice(b"Flicker = 99.0\r\n");
        // Offsets allow negative values and clamp symmetrically.
        raw.extend_from_slice(b"OffsetX = -500.0\r\n");
        raw.extend_from_slice(b"OffsetY = 12.5\r\n");
        raw.extend_from_slice(b"OffsetZ = 500.0\r\n");
        // Non-ASCII section and garbage lines must not panic or leak into Battery.
        raw.extend_from_slice(b"[\xC3\xA9Section\xFF]\r\n");
        raw.extend_from_slice(b"Enabled = 0\r\n");
        raw.extend_from_slice(b"[BATTERY]\r\n");
        raw.extend_from_slice(b"Enabled = 1\r\n");
        raw.extend_from_slice(b"DrainRate = -4.0\r\n");
        raw.extend_from_slice(b"RechargeRate = NaN\r\n");
        raw.extend_from_slice(b"RechargeThreshold = 150.0\r\n");

        let cfg = BetterFlashlightConfig::parse(&raw);
        let defaults = BetterFlashlightConfig::default();

        assert!(cfg.is_active());
        assert!(!cfg.attach_to_weapon);
        assert_eq!(cfg.angle_deg, defaults.angle_deg);
        assert_eq!(cfg.spot_size, defaults.spot_size);
        assert_eq!(cfg.distance, defaults.distance);
        assert_eq!(cfg.color[0], 0.0);
        assert_eq!(cfg.color[1], MAX_COLOR);
        assert_eq!(cfg.color[2], defaults.color[2]);
        assert_eq!(cfg.interior.dimmer, 0.0);
        assert_eq!(cfg.interior.intensity, defaults.interior.intensity);
        assert_eq!(cfg.warmup_time, 0.0);
        assert_eq!(cfg.flicker, MAX_FLICKER);
        assert_eq!(cfg.offset, [-MAX_OFFSET, 12.5, MAX_OFFSET]);
        assert!(cfg.battery.enabled);
        assert_eq!(cfg.battery.drain_rate, 0.0);
        assert_eq!(cfg.battery.recharge_rate, defaults.battery.recharge_rate);
        assert_eq!(cfg.battery.recharge_threshold, 100.0);
    }

    #[test]
    fn cone_angle_clamps_strictly_below_bevys_half_pi_limit() {
        let cfg = BetterFlashlightConfig::parse(b"[Flashlight]\nAngle = 70\nSpotSize = 3.0\n");
        let (inner, outer) = cfg.cone_angles_rad();
        assert!((outer.to_degrees() - MAX_OUTER_DEG).abs() < 1e-4);
        assert!(outer < std::f32::consts::FRAC_PI_2);
        assert!(inner > 0.0 && inner < outer);
    }

    #[test]
    fn startup_vfs_loader_reads_case_insensitively_and_skips_when_absent() {
        let tmp = std::env::temp_dir().join(format!(
            "nvf-flashlight-ini-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).expect("create temp data dir");

        let schema = Arc::new(fnv_nif::Schema::from_strs([]).expect("empty schema"));

        // Absent INI: Startup leaves BetterFlashlightConfig uninserted (vanilla unchanged).
        let vfs_empty = Arc::new(fnv_vfs::Vfs::open(&tmp, &[]).expect("open loose vfs"));
        let mut app_empty = App::new();
        app_empty
            .insert_resource(nvf_assets::GameData::new(vfs_empty, schema.clone()))
            .add_systems(Startup, load_from_vfs);
        app_empty.update();
        assert!(app_empty.world().get_resource::<BetterFlashlightConfig>().is_none());

        // Present INI under mixed case `Config\BetterFlashlightNVSE.ini`: Startup loads it.
        let cfg_dir = tmp.join("Config");
        std::fs::create_dir_all(&cfg_dir).expect("create Config dir");
        std::fs::write(
            cfg_dir.join("BetterFlashlightNVSE.ini"),
            b"[Main]\nRenderEffects = 1\n[Flashlight]\nEnabled = 1\nDistance = 2222\n",
        )
        .expect("write INI");
        let vfs_present = Arc::new(fnv_vfs::Vfs::open(&tmp, &[]).expect("open loose vfs with ini"));
        let mut app_present = App::new();
        app_present
            .insert_resource(nvf_assets::GameData::new(vfs_present, schema))
            .add_systems(Startup, load_from_vfs);
        app_present.update();
        let loaded = app_present
            .world()
            .get_resource::<BetterFlashlightConfig>()
            .expect("INI should be loaded from VFS");
        assert!(loaded.is_active());
        assert_eq!(loaded.distance, 2222.0);

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
