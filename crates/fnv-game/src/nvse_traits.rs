//! JIP's sound, weather and image-space traits (N4 of docs/handoff/nvse-next-batch.md)
//! [documented: GetSoundTraitNumeric, GetWeatherTraitNumeric, GetWeatherRGBColor,
//! GetImageSpaceTrait, GetImageSpaceModTrait and their setters, with their trait ID
//! tables; observed: the peer's skysound set in the original game, lined up with the
//! records' decoded fields]. A setter's change is kept for the session and every getter
//! reads it [hypothesis: the sky, sounds and image spaces NVForever draws keep reading
//! the records; wiring the engine to these changes is a follow-up].

use crate::game::Game;
use fnv_script::vm::XVal;
use fnv_sim::FormId;
use serde_json::Value as Json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TRAIT_STATE_ID: AtomicU64 = AtomicU64::new(1);

/// The session's trait changes: (form, function family, trait or colour slot) -> value.
#[derive(Debug)]
pub struct TraitState {
    pub values: HashMap<(FormId, u8, i64), f64>,
    pub sources: HashMap<FormId, String>,
    pub ism: HashMap<(FormId, i64), u32>,
    /// Unique per-`Game` instance identity so renderer caches never reuse session trait
    /// state across a restored or recreated `Game`.
    pub instance_id: u64,
    /// Monotonic revision bumped whenever `SetImageSpaceTrait` (`0..=32`) or
    /// `SetWorldspaceImagespace` changes an active image-space override
    /// (`SetCellImageSpace` applies on next cell load and does not bump this revision).
    pub image_space_revision: u64,
    /// Monotonic revision bumped whenever `SetWeatherTraitNumeric` changes a weather fog
    /// trait (`15..=20`: day/night near/far/power).
    pub weather_fog_revision: u64,
    /// Monotonic revision bumped whenever `SetWeatherRGBColor`,
    /// `SetWeatherImageSpaceMod`, or `SetWeatherTransitionTimeOverride` changes a live
    /// weather visual override.
    pub weather_visual_revision: u64,
}

impl Default for TraitState {
    fn default() -> Self {
        Self {
            values: HashMap::new(),
            sources: HashMap::new(),
            ism: HashMap::new(),
            instance_id: NEXT_TRAIT_STATE_ID.fetch_add(1, Ordering::Relaxed),
            image_space_revision: 0,
            weather_fog_revision: 0,
            weather_visual_revision: 0,
        }
    }
}

impl TraitState {
    /// A session override for an active image-space modifier's renderer consumer.
    pub fn image_modifier_override(&self, form: FormId, trait_id: i64) -> Option<f64> {
        self.values.get(&(form, IMAD, trait_id)).copied()
    }

    /// Record a live change to an `IMGS` trait (`SetImageSpaceTrait`) or worldspace
    /// image-space assignment (`SetWorldspaceImagespace`).
    pub(crate) fn note_image_space_change(&mut self) {
        self.image_space_revision = self.image_space_revision.wrapping_add(1);
    }

    /// Record a live change to a weather fog trait (`15..=20`).
    pub(crate) fn note_weather_fog_change(&mut self) {
        self.weather_fog_revision = self.weather_fog_revision.wrapping_add(1);
    }

    /// Record a live change to a weather RGB colour (`SetWeatherRGBColor`), weather
    /// image-space adapter (`SetWeatherImageSpaceMod`), or weather transition override
    /// (`SetWeatherTransitionTimeOverride`).
    pub(crate) fn note_weather_visual_change(&mut self) {
        self.weather_visual_revision = self.weather_visual_revision.wrapping_add(1);
    }
}

pub const SOUND_TRAIT_FAMILY: u8 = 0;
const SOUND: u8 = SOUND_TRAIT_FAMILY;
const WEATHER: u8 = 1;
const COLOR: u8 = 2;
const IMGS: u8 = 3;
const IMAD: u8 = 4;

fn form(args: &[XVal], k: usize) -> Option<FormId> {
    match args.get(k)? {
        XVal::Ref(0) => None,
        XVal::Ref(r) => Some(FormId(*r)),
        XVal::Num(x) if *x >= 1.0 && x.fract() == 0.0 => Some(FormId(*x as u32)),
        _ => None,
    }
}

fn num(args: &[XVal], k: usize, dflt: f64) -> f64 {
    match args.get(k) {
        Some(XVal::Num(x)) => *x,
        _ => dflt,
    }
}

impl Game<'_> {
    /// A decoded subrecord of a record (the first with that signature).
    fn sub(&self, f: FormId, sig: [u8; 4]) -> Option<&Json> {
        self.data.record_fields.get(&self.unclone(f))?.iter().find(|(s, _)| *s == sig).map(|(_, v)| v)
    }

    /// Snapshot any active session `SetSoundSourceFile` and `SetSoundTraitNumeric`
    /// (`0..=12`) overrides from `source` onto `clone` at `TempCloneForm` creation time.
    /// Pre-clone edits on `source` carry onto `clone`, while later edits on `source` stay
    /// isolated from already-created clones.
    pub(crate) fn snapshot_sound_traits(&mut self, source: FormId, clone: FormId) {
        if let Some(path) = self.traits.sources.get(&source).cloned() {
            self.traits.sources.insert(clone, path);
        }
        for slot in 0..=12 {
            if let Some(&v) = self.traits.values.get(&(source, SOUND, slot)) {
                self.traits.values.insert((clone, SOUND, slot), v);
            }
        }
    }

    /// The session `SetSoundSourceFile` override directly stored for `f` (including any
    /// override snapshotted at `TempCloneForm` creation time).
    pub fn sound_source_override(&self, f: FormId) -> Option<&str> {
        self.traits.sources.get(&f).map(String::as_str)
    }

    /// The session `SetSoundTraitNumeric` override directly stored for `f` and trait `slot`
    /// (including any override snapshotted at `TempCloneForm` creation time).
    pub fn sound_trait_override(&self, f: FormId, slot: i64) -> Option<f64> {
        self.traits.values.get(&(f, SOUND, slot)).copied()
    }

    /// Unique `(game_instance_id, image_space_revision)` token for renderer consumers
    /// tracking live `SetImageSpaceTrait` (`0..=32`) updates without work on unchanged
    /// frames. `SetCellImageSpace` follows the documented next-load rule and does not
    /// bump this revision.
    pub fn image_space_sync_token(&self) -> (u64, u64) {
        (self.traits.instance_id, self.traits.image_space_revision)
    }

    /// Unique `(game_instance_id, weather_fog_revision)` token for renderer consumers
    /// tracking live `SetWeatherTraitNumeric` fog updates (`15..=20`) without work on
    /// unchanged frames.
    pub fn weather_fog_sync_token(&self) -> (u64, u64) {
        (self.traits.instance_id, self.traits.weather_fog_revision)
    }

    /// Active session `SetImageSpaceTrait` overrides (`0..=32`) for `form`.
    pub fn image_space_trait_overrides(&self, form: FormId) -> Vec<(i64, f64)> {
        let mut out = Vec::new();
        for slot in 0..=32 {
            if let Some(&v) = self.traits.values.get(&(form, IMGS, slot)) {
                out.push((slot, v));
            }
        }
        out
    }

    /// Active session `SetWeatherTraitNumeric` fog overrides (`15..=20`: day near/far,
    /// night near/far, day/night power) for `weather`, if any slot was overridden.
    pub fn weather_fog_override(&self, weather: FormId) -> Option<[Option<f32>; 6]> {
        let mut slots = [None; 6];
        let mut any = false;
        for (i, dst) in slots.iter_mut().enumerate() {
            if let Some(&v) = self.traits.values.get(&(weather, WEATHER, 15 + i as i64)) {
                let v = v as f32;
                if v.is_finite() {
                    *dst = Some(v);
                    any = true;
                }
            }
        }
        any.then_some(slots)
    }

    /// Active session `SetWeatherTraitNumeric` fog overrides (`15..=20`) across all
    /// overridden `WTHR` forms, keyed by weather FormID.
    pub fn weather_fog_overrides(&self) -> HashMap<u32, [Option<f32>; 6]> {
        let mut out: HashMap<u32, [Option<f32>; 6]> = HashMap::new();
        for (&(form, fam, slot), &v) in &self.traits.values {
            if fam == WEATHER && (15..=20).contains(&slot) {
                let v = v as f32;
                if v.is_finite() {
                    out.entry(form.0).or_insert([None; 6])[(slot - 15) as usize] = Some(v);
                }
            }
        }
        out
    }

    /// Unique `(game_instance_id, weather_visual_revision)` token for renderer consumers
    /// tracking live `SetWeatherRGBColor`, `SetWeatherImageSpaceMod`, and
    /// `SetWeatherTransitionTimeOverride` updates without work on unchanged frames.
    pub fn weather_visual_sync_token(&self) -> (u64, u64) {
        (self.traits.instance_id, self.traits.weather_visual_revision)
    }

    /// Active session `SetWeatherRGBColor` overrides for `NAM0` colour types (`0..=9`,
    /// excluding type `2` which lives in `PNAM` cloud layers) across times `0..=5`, keyed
    /// by weather FormID.
    pub fn weather_color_overrides(&self) -> HashMap<u32, [[Option<[u8; 3]>; 6]; fnv_world::sky::COLOR_TYPES]> {
        let mut out: HashMap<u32, [[Option<[u8; 3]>; 6]; fnv_world::sky::COLOR_TYPES]> = HashMap::new();
        for (&(form, fam, slot), &v) in &self.traits.values {
            if fam != COLOR || slot < 0 || !v.is_finite() {
                continue;
            }
            let layer = slot % 4;
            let ty = (slot / 24) as usize;
            let time = ((slot % 24) / 4) as usize;
            if ty >= fnv_world::sky::COLOR_TYPES || ty == 2 || time >= 6 || layer != 0 {
                continue;
            }
            let packed = v.max(0.0) as i64;
            let rgb = [
                ((packed / 1_000_000) & 255) as u8,
                ((packed / 1_000 % 1_000) & 255) as u8,
                ((packed % 1_000) & 255) as u8,
            ];
            out.entry(form.0).or_insert([[None; 6]; fnv_world::sky::COLOR_TYPES])[ty][time] = Some(rgb);
        }
        out
    }

    /// Active session `SetWeatherRGBColor` type-2 (`PNAM` lower cloud layers `0..=3`)
    /// overrides across times `0..=5`, keyed by weather FormID.
    pub fn weather_cloud_color_overrides(&self) -> HashMap<u32, [[Option<[u8; 3]>; 6]; 4]> {
        let mut out: HashMap<u32, [[Option<[u8; 3]>; 6]; 4]> = HashMap::new();
        for (&(form, fam, slot), &v) in &self.traits.values {
            if fam != COLOR || slot < 0 || !v.is_finite() {
                continue;
            }
            let ty = slot / 24;
            let time = ((slot % 24) / 4) as usize;
            let layer = (slot % 4) as usize;
            if ty != 2 || time >= 6 || layer >= 4 {
                continue;
            }
            let packed = v.max(0.0) as i64;
            let rgb = [
                ((packed / 1_000_000) & 255) as u8,
                ((packed / 1_000 % 1_000) & 255) as u8,
                ((packed % 1_000) & 255) as u8,
            ];
            out.entry(form.0).or_insert([[None; 6]; 4])[layer][time] = Some(rgb);
        }
        out
    }

    /// Active session `SetWeatherImageSpaceMod` overrides (`0IAD..5IAD`, times `0..=5`),
    /// keyed by weather FormID. `Some(0)` represents an explicit clear to no adapter,
    /// distinct from `None` (keep authored record).
    pub fn weather_adapter_overrides(&self) -> HashMap<u32, [Option<u32>; 6]> {
        let mut out: HashMap<u32, [Option<u32>; 6]> = HashMap::new();
        for (&(form, time), &imad) in &self.traits.ism {
            if (0..=5).contains(&time) {
                out.entry(form.0).or_insert([None; 6])[time as usize] = Some(imad);
            }
        }
        out
    }
}

fn f(v: Option<&Json>) -> f64 {
    v.and_then(Json::as_f64).unwrap_or(0.0)
}

/// A sound's trait from its SNDD [observed: EMTMedicalBeep01, 40 and 28 read 200 and
/// 2800, the static attenuation 2643 reads -26.43].
fn sound_trait(g: &Game, s: FormId, t: i64) -> f64 {
    let d = g.sub(s, *b"SNDD");
    let k = |n: &str| f(d.and_then(|d| d.get(n)));
    match t {
        0 => k("min_attenuation_distance") * 5.0,
        1 => k("max_attenuation_distance") * 100.0,
        2 => k("frequency_adjustment_percent"),
        3 => -k("static_attenuation_cdb") / 100.0,
        4 => k("stop_time"),
        5 => k("start_time"),
        6..=10 => f(d.and_then(|d| d.get("attenuation_curve")).and_then(|c| c.get((t - 6) as usize))),
        11 => k("reverb_attenuation_control"),
        12 => k("priority"),
        _ => 0.0,
    }
}

/// A weather's numeric trait [observed: cloud speeds byte/2550 (52 reads 0.0203922),
/// wind, glare, damage, fades and frequency byte/255, trans delta byte/1000 (255 reads
/// 0.255), the fog distances as floats].
fn weather_trait(g: &Game, w: FormId, t: i64) -> f64 {
    let d = g.sub(w, *b"DATA");
    let k = |n: &str| f(d.and_then(|d| d.get(n)));
    let fog = g.sub(w, *b"FNAM");
    let fogk = |n: &str| f(fog.and_then(|d| d.get(n)));
    match t {
        0..=3 => f(g.sub(w, *b"ONAM").and_then(|a| a.get(t as usize))) / 2550.0,
        4 => k("wind_speed") / 255.0,
        5 => k("trans_delta") / 1000.0,
        6 => k("sun_glare") / 255.0,
        7 => k("sun_damage") / 255.0,
        8 => k("precipitation_begin_fade_in") / 255.0,
        9 => k("precipitation_end_fade_out") / 255.0,
        10 => k("lightning_begin_fade_in") / 255.0,
        11 => k("lightning_end_fade_out") / 255.0,
        12 => k("lightning_frequency") / 255.0,
        13 => f(d.and_then(|d| d.get("flags")).and_then(|x| x.get("value"))),
        14 => {
            let c = d.and_then(|d| d.get("lightning_color"));
            let at = |i: usize| f(c.and_then(|c| c.get(i)));
            at(0) * 1_000_000.0 + at(1) * 1000.0 + at(2)
        }
        15 => fogk("day_near"),
        16 => fogk("day_far"),
        17 => fogk("night_near"),
        18 => fogk("night_far"),
        19 => fogk("day_power"),
        20 => fogk("night_power"),
        _ => 0.0,
    }
}

/// The scale a weather trait is stored at (its setter's quantising).
fn weather_scale(t: i64) -> Option<f64> {
    match t {
        0..=3 => Some(2550.0),
        4 | 6..=12 => Some(255.0),
        5 => Some(1000.0),
        _ => None,
    }
}

/// A weather colour as decimal RRRGGGBBB [observed: Day sunlight 255227170]: NAM0 by
/// type and time, the lower clouds (type 2) from PNAM by layer [observed: layers 0–3
/// read PNAM's].
fn weather_color(g: &Game, w: FormId, ty: i64, time: i64, layer: i64) -> f64 {
    let (sig, i) = if ty == 2 { (*b"PNAM", layer.clamp(0, 3) * 6 + time) } else { (*b"NAM0", ty * 6 + time) };
    let c = g.sub(w, sig).and_then(|a| a.get(i as usize));
    let at = |n: &str| f(c.and_then(|c| c.get(n)));
    at("red") * 1_000_000.0 + at("green") * 1000.0 + at("blue")
}

/// An image space's trait: the DNAM floats in order (14 named, the rest after them),
/// tints (21–23, 29–31) as 0–255 [observed: HazmatImageSpace].
fn imgs_trait(g: &Game, s: FormId, t: i64) -> f64 {
    const NAMED: [&str; 14] = ["eye_adapt_speed", "blur_radius", "blur_passes", "emissive_mult", "target_lum", "upper_lum_clamp", "bright_scale", "bright_clamp", "lum_ramp_no_tex", "lum_ramp_min", "lum_ramp_max", "sunlight_dimmer", "grass_dimmer", "tree_dimmer"];
    let d = g.sub(s, *b"DNAM");
    let v = if (0..14).contains(&t) {
        f(d.and_then(|d| d.get(NAMED[t as usize])))
    } else {
        rest_float(d, (t - 14) as usize)
    };
    if matches!(t, 21..=23 | 29..=31) { (v * 255.0).round() } else { v }
}

/// The `i`th float of a decoded struct's "rest" bytes.
fn rest_float(d: Option<&Json>, i: usize) -> f64 {
    let hex = d.and_then(|d| d.get("rest")).and_then(Json::as_str).unwrap_or("");
    let b = (0..hex.len() / 2).filter_map(|k| u8::from_str_radix(&hex[2 * k..2 * k + 2], 16).ok()).collect::<Vec<u8>>();
    b.get(4 * i..4 * i + 4).map_or(0.0, |w| f64::from(f32::from_le_bytes([w[0], w[1], w[2], w[3]])))
}

/// An image space modifier's trait: its first keyframe of each array [observed:
/// FadeToBlack9sISFX, the multiply/add pairs 1/0, the tint 255]; the tint (47–50) is
/// 0–255 alpha included [observed: original FNV 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30,
/// 2026-11-20, image-modifiers.md]; the radial blur centre
/// from DNAM's floats 49 and 50 after its first two fields [observed: both 0.5; the
/// offset is a hypothesis]; traits 65–67 0 [hypothesis].
fn imad_trait(g: &Game, s: FormId, t: i64) -> f64 {
    let d = g.sub(s, *b"DNAM");
    let key = |sig: [u8; 4], field: &str| f(g.sub(s, sig).and_then(|a| a.get(0)).and_then(|k| k.get(field)));
    match t {
        0 => f(d.and_then(|d| d.get("duration"))),
        1 => rest_float(d, 49),
        2 => rest_float(d, 50),
        3..=44 => {
            let k = ((t - 3) / 2) as u8;
            let sig = if (t - 3) % 2 == 0 { [k, b'I', b'A', b'D'] } else { [0x40 + k, b'I', b'A', b'D'] };
            key(sig, "value")
        }
        45 => key(*b"BNAM", "value"),
        46 => key(*b"VNAM", "value"),
        47 => key(*b"TNAM", "red") * 255.0,
        48 => key(*b"TNAM", "green") * 255.0,
        49 => key(*b"TNAM", "blue") * 255.0,
        50 => key(*b"TNAM", "alpha") * 255.0,
        51 => key(*b"NAM3", "red") * 255.0,
        52 => key(*b"NAM3", "green") * 255.0,
        53 => key(*b"NAM3", "blue") * 255.0,
        54 => key(*b"NAM3", "alpha"),
        55 => key(*b"RNAM", "value"),
        56 => key(*b"SNAM", "value"),
        57 => key(*b"UNAM", "value"),
        58 => key(*b"NAM1", "value"),
        59 => key(*b"NAM2", "value"),
        60 => key(*b"WNAM", "value"),
        61 => key(*b"XNAM", "value"),
        62 => key(*b"YNAM", "value"),
        63 => key(*b"NAM4", "value"),
        64 => f(d.and_then(|d| d.get("animatable"))),
        _ => 0.0,
    }
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, name: &str, args: &[XVal]) -> Option<Result<XVal, String>> {
    let zero = XVal::Num(0.0);
    let Some(x) = form(args, 0) else {
        return match name {
            "getsoundtraitnumeric" | "setsoundtraitnumeric" | "getsoundsourcefile" | "setsoundsourcefile" | "getweathertraitnumeric" | "setweathertraitnumeric" | "getweatherrgbcolor" | "setweatherrgbcolor" | "getweatherimagespacemod" | "setweatherimagespacemod" | "getimagespacetrait" | "setimagespacetrait" | "getimagespacemodtrait" | "setimagespacemodtrait" => Some(Ok(zero)),
            _ => None,
        };
    };
    let t = num(args, 1, -1.0) as i64;
    let changed = |g: &Game, fam: u8, slot: i64| g.traits.values.get(&(x, fam, slot)).copied();
    let v = match name {
        "getsoundtraitnumeric" => XVal::Num(if (0..=12).contains(&t) { g.sound_trait_override(x, t).unwrap_or_else(|| sound_trait(g, x, t)) } else { 0.0 }),
        // Static attenuation is kept negated and whole [observed: 7 reads -7, -26.43
        // reads -26]; the distances keep their stored steps. Trait 12 is ignored/unwritable
        // [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-24,
        // `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters,
        // `w4c_b1_21_sound_and_diff_bounds`: `SetSoundTraitNumeric rSnd 12 77` left
        // `GetSoundTraitNumeric rSnd 12` at `0`].
        "setsoundtraitnumeric" => {
            if (0..=11).contains(&t) {
                let v = num(args, 2, 0.0);
                let kept = match t {
                    0 => (v / 5.0).trunc() * 5.0,
                    1 => (v / 100.0).trunc() * 100.0,
                    3 => -v.trunc().abs(),
                    _ => v.trunc(),
                };
                g.traits.values.insert((x, SOUND, t), kept);
            }
            zero
        }
        "getsoundsourcefile" => XVal::Str(g.sound_source_override(x).map(str::to_string).unwrap_or_else(|| g.sub(x, *b"FNAM").and_then(Json::as_str).unwrap_or("").to_string())),
        "setsoundsourcefile" => {
            if let Some(XVal::Str(p)) = args.get(1) {
                g.traits.sources.insert(x, p.clone());
            }
            zero
        }
        // [observed: 0 even after PlaySound; hypothesis: the engine does not tell which
        // sounds play]
        "getweathertraitnumeric" => XVal::Num(if (0..=20).contains(&t) { changed(g, WEATHER, t).unwrap_or_else(|| weather_trait(g, x, t)) } else { 0.0 }),
        // Stored at the record's step [observed: 0.5 reads 0.498039].
        "setweathertraitnumeric" => {
            if (0..=20).contains(&t) {
                let v = num(args, 2, 0.0);
                let kept = match weather_scale(t) {
                    Some(s) => (v * s).trunc().clamp(0.0, 255.0) / s,
                    None => v,
                };
                let prev = g.traits.values.insert((x, WEATHER, t), kept);
                if (15..=20).contains(&t) && prev.map(f64::to_bits) != Some(kept.to_bits()) {
                    g.traits.note_weather_fog_change();
                }
            }
            zero
        }
        "getweatherrgbcolor" | "setweatherrgbcolor" => {
            let time = num(args, 2, -1.0) as i64;
            if !(0..=9).contains(&t) || !(0..=5).contains(&time) {
                return Some(Ok(zero));
            }
            let layer_at = if name == "setweatherrgbcolor" { 4 } else { 3 };
            let layer = num(args, layer_at, 0.0) as i64;
            let slot = (t * 6 + time) * 4 + layer.clamp(0, 3);
            if name == "getweatherrgbcolor" {
                XVal::Num(changed(g, COLOR, slot).unwrap_or_else(|| weather_color(g, x, t, time, layer)))
            } else {
                // RRRGGGBBB [observed: rgb-ui, 31 values fit]: above 255255255 the call is
                // ignored; the parts are packed unmasked as R + G·256 + B·65536 and read
                // back as bytes, the rest carried into B (100999999 reads 100232002).
                let v = num(args, 3, 0.0).max(0.0) as i64;
                if v > 255_255_255 {
                    return Some(Ok(zero));
                }
                let w = v / 1_000_000 + (v / 1000 % 1000) * 256 + (v % 1000) * 65536;
                let (r, gg, b) = (w & 255, (w >> 8) & 255, w >> 16);
                let packed = (r * 1_000_000 + gg * 1000 + b) as f64;
                let prev = g.traits.values.insert((x, COLOR, slot), packed);
                if prev.map(f64::to_bits) != Some(packed.to_bits()) {
                    g.traits.note_weather_visual_change();
                }
                zero
            }
        }
        "getweatherimagespacemod" => {
            let time = num(args, 1, -1.0) as i64;
            let ism = g.traits.ism.get(&(x, time)).copied().or_else(|| {
                let sig = [u8::try_from(time).ok()?, b'I', b'A', b'D'];
                g.sub(x, sig).and_then(Json::as_str).and_then(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).ok())
            });
            XVal::Ref(ism.unwrap_or(0))
        }
        "setweatherimagespacemod" => {
            let time = num(args, 1, -1.0) as i64;
            let id = form(args, 2).map_or(0, |f| f.0);
            let prev = g.traits.ism.insert((x, time), id);
            if prev != Some(id) && (0..=5).contains(&time) {
                g.traits.note_weather_visual_change();
            }
            zero
        }
        "getimagespacetrait" => XVal::Num(if (0..=32).contains(&t) { changed(g, IMGS, t).unwrap_or_else(|| imgs_trait(g, x, t)) } else { 0.0 }),
        "setimagespacetrait" => {
            if (0..=32).contains(&t) {
                let v = num(args, 2, 0.0);
                let prev = g.traits.values.insert((x, IMGS, t), v);
                if prev.map(f64::to_bits) != Some(v.to_bits()) {
                    g.traits.note_image_space_change();
                }
            }
            zero
        }
        "getimagespacemodtrait" => XVal::Num(if (0..=67).contains(&t) { changed(g, IMAD, t).unwrap_or_else(|| imad_trait(g, x, t)) } else { 0.0 }),
        "setimagespacemodtrait" => {
            if (0..=67).contains(&t) {
                g.traits.values.insert((x, IMAD, t), num(args, 2, 0.0));
            }
            zero
        }
        _ => return None,
    };
    Some(Ok(v))
}
