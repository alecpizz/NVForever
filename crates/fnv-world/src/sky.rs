//! Climate (CLMT), weather (WTHR) and water (WATR) records, and the time-of-day model
//! that turns them into sky, light and fog colours. Facts and tags:
//! docs/spec-notes/world.md §6; layouts in `schemas/fnv/sky.json`.

use crate::fields::{formid_of, global, num, winner, Fields};
use fnv_esm::{LoadOrder, Schema};
use serde_json::Value;

/// Weather colour types, in NAM0 order [documented: xEdit wbWeatherColors, FNV].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorType {
    SkyUpper = 0,
    Fog = 1,
    Unused = 2,
    Ambient = 3,
    Sunlight = 4,
    Sun = 5,
    Stars = 6,
    SkyLower = 7,
    Horizon = 8,
    EffectLighting = 9,
}

pub const COLOR_TYPES: usize = 10;

/// Times of day of a colour set: sunrise, day, sunset, night, then (in the six-time
/// layout) high noon and midnight [documented: xEdit wbWeatherTimeOfDay, FNV].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeOfDay {
    Sunrise = 0,
    Day = 1,
    Sunset = 2,
    Night = 3,
    HighNoon = 4,
    Midnight = 5,
}

/// One WLST entry: a weather, its chance, and an optional global variable gating it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeatherChoice {
    pub weather: u32,
    pub chance: i32,
    pub global: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Climate {
    pub form_id: u32,
    pub edid: Option<String>,
    pub weathers: Vec<WeatherChoice>,
    pub sun_texture: Option<String>,
    pub sun_glare_texture: Option<String>,
    /// Sunrise begin/end and sunset begin/end in hours.
    pub sunrise: (f32, f32),
    pub sunset: (f32, f32),
    /// MODL: the night-sky model (NVDefaultClimate: `Sky\Stars.nif`) [observed].
    pub model: Option<String>,
    /// TNAM "moons / phase length" byte as stored (NVDefaultClimate: 131).
    pub moons: u8,
    /// TNAM volatility byte (GetClimateTraitNumeric trait 4 "Volatility" [documented:
    /// GECK wiki]); 0 in every FalloutNV.esm climate [observed]. Its use: [`roll_due`].
    pub volatility: u8,
}

/// Moon phases in order, as named by the textures `textures\sky\masser_<phase>.dds`
/// [observed: those eight files ship; hypothesis: the cycle runs full, waning, new,
/// waxing].
pub const MOON_PHASES: [&str; 8] = ["full", "three_wan", "half_wan", "one_wan", "new", "one_wax", "half_wax", "three_wax"];

impl Climate {
    /// Days per moon phase: the low six bits of the moons byte; the top two bits flag
    /// the moons shown [hypothesis: xEdit names the byte "Moons / Phase Length";
    /// NVDefaultClimate stores 131 = bit 7 and 3].
    pub fn moon_phase_length(&self) -> u8 {
        self.moons & 0x3F
    }

    /// Whether any moon flag (bit 6 or 7) is set.
    pub fn has_moon(&self) -> bool {
        self.moons & 0xC0 != 0
    }
}

/// Index into [`MOON_PHASES`] after `days_passed` game days (phase 0 on day 0)
/// [hypothesis].
pub fn moon_phase(days_passed: f32, phase_length: u8) -> usize {
    if phase_length == 0 {
        return 0;
    }
    ((days_passed / phase_length as f32).floor() as i64).rem_euclid(MOON_PHASES.len() as i64) as usize
}

/// Smooth 0..1 ramp of `x` from `edge0` to `edge1` (either order).
pub fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Weather {
    pub form_id: u32,
    pub edid: Option<String>,
    /// The file name of the plugin whose record this is: the last in the load order to
    /// define it. Lets a reader tell the base game's own data from a mod's (a base-game
    /// weather names a cloud texture no archive has, issue #2408).
    pub plugin: Option<String>,
    /// `colors[type][time]` as RGB bytes; 4 or 6 times per type.
    pub colors: Vec<Vec<[u8; 3]>>,
    /// Fog distances: day near, day far, night near, night far, day power, night power.
    pub fog: [f32; 6],
    pub cloud_textures: [Option<String>; 4],
    /// PNAM `cloud_colors[layer][time]` as RGB bytes; 4 or 6 times per layer, like NAM0.
    pub cloud_colors: Vec<Vec<[u8; 3]>>,
    /// ONAM cloud speed per layer, 0-255 [documented: xEdit wbWeatherCloudSpeed, one byte
    /// per layer; GECK wiki GetWeatherTraitNumeric, traits 0-3 "Layer N Cloud Speed"]. The
    /// unit is not documented: [`Weather::cloud_scroll`].
    pub cloud_speeds: [u8; 4],
    /// LNAM maximum cloud layers.
    pub max_cloud_layers: Option<u32>,
    /// DATA flags bit 2 "Rainy" [documented: xEdit]; set on Honest Hearts' two Zion rain
    /// weathers and no other weather in the masters [observed].
    pub rainy: bool,
    /// DATA wind speed, 0-255 [documented: xEdit]; 50 for most weathers, 127 for
    /// HVDustStorm, 130 for NVHooverFinalBattle [observed].
    pub wind_speed: u8,
    /// DATA Trans Delta and Sun Glare, 0-255 [documented: xEdit; GECK wiki SetWeather and
    /// SetWeatherTransitionTimeOverride: the Trans Delta sets the transition time, at most
    /// 30 s of real time]. Every New Vegas weather stores 255 and 54 [observed]; `None`
    /// (no DATA) reads as those.
    pub trans_delta: Option<u8>,
    pub sun_glare: Option<u8>,
    /// SNAM sounds with their kind, in record order [documented: xEdit wbWeatherSounds].
    pub sounds: Vec<WeatherSound>,
    /// MODL: the precipitation model drawn about the camera [documented: GECK wiki
    /// GetWeatherPrecipitationModel, "the Precipitation model path of a weather"].
    pub precipitation_model: Option<String>,
    /// DATA fade fields and lightning frequency, raw bytes [documented: xEdit
    /// wbDefinitionsFNV WTHR DATA; GECK wiki GetWeatherTraitNumeric traits 8-12].
    pub precipitation_fade: (u8, u8),
    pub lightning_fade: (u8, u8),
    pub lightning_frequency: u8,
    /// DATA "weather classification" flags: 1 Pleasant, 2 Cloudy, 4 Rainy, 8 Snow
    /// [documented: xEdit; GECK wiki GetWeatherTraitNumeric].
    pub classification: u8,
    /// DATA lightning colour (RGB bytes) [documented: xEdit].
    pub lightning_color: [u8; 3],
}

/// The kinds of a weather sound (WTHR SNAM type) [documented: xEdit wbWeatherSounds].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeatherSoundKind {
    Default,
    Precipitation,
    Wind,
    Thunder,
}

/// One WTHR SNAM entry: a SOUN (global FormID) and its kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeatherSound {
    pub sound: u32,
    pub kind: WeatherSoundKind,
}

impl WeatherSoundKind {
    pub fn from_u32(v: u32) -> Option<WeatherSoundKind> {
        Some(match v {
            0 => WeatherSoundKind::Default,
            1 => WeatherSoundKind::Precipitation,
            2 => WeatherSoundKind::Wind,
            3 => WeatherSoundKind::Thunder,
            _ => return None,
        })
    }
}

/// Cloud texture scroll per real second per ONAM speed unit, in texture repeats
/// [hypothesis: neither xEdit nor the GECK wiki gives the unit. 1/25,600 is the first sky
/// pass's placeholder, kept: at it NVWastelandClear's top layer (speed 65) drifts one
/// texture width in about 6.6 minutes. To be measured from two game screenshots of the
/// same view some seconds apart (docs/owner-review.md)].
pub const CLOUD_SCROLL_PER_UNIT: f32 = 1.0 / 25_600.0;

impl Weather {
    /// Texture repeats per real second that cloud layer `layer` (0-3) scrolls by: its ONAM
    /// speed times [`CLOUD_SCROLL_PER_UNIT`] [hypothesis: linear in the byte, 0 still].
    pub fn cloud_scroll(&self, layer: usize) -> f32 {
        self.cloud_speeds.get(layer).map_or(0.0, |&s| f32::from(s) * CLOUD_SCROLL_PER_UNIT)
    }

    /// The layer's texture offset (0..1 repeats) `secs` real seconds after the clock's
    /// start. Worked in f64 so that a long session keeps a smooth drift.
    pub fn cloud_offset(&self, layer: usize, secs: f64) -> f32 {
        (secs * f64::from(self.cloud_scroll(layer))).rem_euclid(1.0) as f32
    }

    /// Real seconds a change to this weather takes: 30 × Trans Delta ÷ 255, at most the
    /// documented 30 s; 0 is at once [hypothesis: the time grows with the byte;
    /// docs/spec-notes/world.md §6 "Weather transitions and glare"].
    pub fn transition_seconds(&self) -> f32 {
        30.0 * f32::from(self.trans_delta.unwrap_or(255)) / 255.0
    }

    /// The sun glare's strength against a New Vegas weather's (Sun Glare 54, the one the
    /// glare was tuned on) [hypothesis: the byte scales the glare].
    pub fn glare_scale(&self) -> f32 {
        f32::from(self.sun_glare.unwrap_or(54)) / 54.0
    }

    /// The sounds that go round while the weather is shown (all but thunder).
    pub fn loops(&self) -> impl Iterator<Item = &WeatherSound> {
        self.sounds.iter().filter(|s| s.kind != WeatherSoundKind::Thunder)
    }

    /// The thunder sounds, one of which follows each lightning flash.
    pub fn thunder(&self) -> impl Iterator<Item = &WeatherSound> {
        self.sounds.iter().filter(|s| s.kind == WeatherSoundKind::Thunder)
    }

    /// Lightning flashes in this weather: a lightning frequency and a thunder sound or a
    /// Rainy classification [hypothesis: the GECK wiki's TriggerLightningFX "only work[s]
    /// when ... it is raining"; a weather with a thunder sound flashes too].
    pub fn lightning(&self) -> bool {
        self.lightning_frequency > 0 && (self.rainy || self.thunder().next().is_some())
    }
}

fn byte_color(v: &Value) -> Option<[u8; 3]> {
    Some([num(v, "red")? as u8, num(v, "green")? as u8, num(v, "blue")? as u8])
}

/// A climate time byte (10-minute units) in hours [documented: xEdit wbClmtTime].
pub fn climate_hours(v: f64) -> f32 {
    (v as f32) / 6.0
}

pub fn climate(lo: &LoadOrder, schema: &Schema, fid: u32) -> Option<Climate> {
    let (pi, r) = winner(lo, fid)?;
    let f = Fields::new(r, pi, schema).ok()?;
    let mut c = Climate { form_id: fid, edid: f.string(b"EDID"), ..Default::default() };
    for w in f.get(b"WLST").and_then(|v| v.as_array().cloned()).unwrap_or_default() {
        if let Some(id) = w.get("weather").and_then(formid_of).and_then(|l| global(lo, pi, l)) {
            c.weathers.push(WeatherChoice {
                weather: id,
                chance: num(&w, "chance").unwrap_or(0.0) as i32,
                global: w.get("global").and_then(formid_of).and_then(|l| global(lo, pi, l)),
            });
        }
    }
    c.sun_texture = f.string(b"FNAM").filter(|s| !s.is_empty());
    c.sun_glare_texture = f.string(b"GNAM").filter(|s| !s.is_empty());
    let t = f.get(b"TNAM")?;
    let h = |k: &str| num(&t, k).map_or(0.0, climate_hours);
    c.sunrise = (h("sunrise_begin"), h("sunrise_end"));
    c.sunset = (h("sunset_begin"), h("sunset_end"));
    c.model = f.string(b"MODL").filter(|s| !s.is_empty());
    c.moons = num(&t, "moons_phase_length").unwrap_or(0.0) as u8;
    c.volatility = num(&t, "volatility").unwrap_or(0.0) as u8;
    Some(c)
}

pub fn weather(lo: &LoadOrder, schema: &Schema, fid: u32) -> Option<Weather> {
    let (pi, r) = winner(lo, fid)?;
    let f = Fields::new(r, pi, schema).ok()?;
    let mut w = Weather { form_id: fid, edid: f.string(b"EDID"), plugin: lo.plugins.get(pi).map(|p| p.name.clone()), ..Default::default() };
    let flat: Vec<[u8; 3]> =
        f.get(b"NAM0").and_then(|v| v.as_array().map(|a| a.iter().filter_map(byte_color).collect())).unwrap_or_default();
    // Ten colour types; 4 or 6 times each [observed: 160 or 240 bytes].
    if flat.len().is_multiple_of(COLOR_TYPES) && !flat.is_empty() {
        let times = flat.len() / COLOR_TYPES;
        w.colors = flat.chunks(times).map(|c| c.to_vec()).collect();
    }
    if let Some(fog) = f.get(b"FNAM") {
        for (i, k) in ["day_near", "day_far", "night_near", "night_far", "day_power", "night_power"].iter().enumerate() {
            w.fog[i] = num(&fog, k).unwrap_or(0.0) as f32;
        }
    }
    for (i, s) in [b"DNAM", b"CNAM", b"ANAM", b"BNAM"].iter().enumerate() {
        w.cloud_textures[i] = f.string(s).filter(|s| !s.is_empty());
    }
    let clouds: Vec<[u8; 3]> =
        f.get(b"PNAM").and_then(|v| v.as_array().map(|a| a.iter().filter_map(byte_color).collect())).unwrap_or_default();
    // Four layers; 4 or 6 times each [observed: 64 or 96 bytes].
    if clouds.len().is_multiple_of(4) && !clouds.is_empty() {
        w.cloud_colors = clouds.chunks(clouds.len() / 4).map(|c| c.to_vec()).collect();
    }
    if let Some(sp) = f.get(b"ONAM").and_then(|v| v.as_array().cloned()) {
        for (i, v) in sp.iter().take(4).enumerate() {
            w.cloud_speeds[i] = v.as_u64().unwrap_or(0) as u8;
        }
    }
    w.max_cloud_layers = f.get(b"LNAM").and_then(|v| v.as_u64()).map(|v| v as u32);
    w.rainy = f
        .get(b"DATA")
        .and_then(|d| d.get("flags").and_then(|fl| num(fl, "value")))
        .is_some_and(|v| (v as u64) & 4 != 0);
    if let Some(d) = f.get(b"DATA") {
        let b = |k: &str| num(&d, k).map_or(0, |v| v as u8);
        w.wind_speed = b("wind_speed");
        w.trans_delta = Some(b("trans_delta"));
        w.sun_glare = Some(b("sun_glare"));
        w.precipitation_fade = (b("precipitation_begin_fade_in"), b("precipitation_end_fade_out"));
        w.lightning_fade = (b("lightning_begin_fade_in"), b("lightning_end_fade_out"));
        w.lightning_frequency = b("lightning_frequency");
        w.classification = d.get("flags").and_then(|fl| num(fl, "value")).map_or(0, |v| v as u8);
        if let Some(c) = d.get("lightning_color").and_then(|c| c.as_array()) {
            for (i, v) in c.iter().take(3).enumerate() {
                w.lightning_color[i] = v.as_u64().unwrap_or(0) as u8;
            }
        }
    }
    for s in f.subs.iter().filter(|s| &s.sig == b"SNAM" && s.data.len() >= 8) {
        let local = u32::from_le_bytes([s.data[0], s.data[1], s.data[2], s.data[3]]);
        let kind = WeatherSoundKind::from_u32(u32::from_le_bytes([s.data[4], s.data[5], s.data[6], s.data[7]]));
        if let (Some(sound), Some(kind)) = (global(lo, pi, local).filter(|&g| g != 0 && local != 0), kind) {
            w.sounds.push(WeatherSound { sound, kind });
        }
    }
    w.precipitation_model = f.string(b"MODL").filter(|s| !s.is_empty());
    Some(w)
}

/// The climate's most likely weather: the highest chance among the [`eligible`] entries
/// (first on ties).
pub fn default_weather(c: &Climate, global_value: impl Fn(u32) -> Option<f32>) -> Option<u32> {
    eligible(&c.weathers, global_value).into_iter().reduce(|a, b| if b.chance > a.chance { b } else { a }).map(|w| w.weather)
}

/// The weather a game starting at (`x`, `y`) of `world` shows first: the most likely of
/// the overriding region there (the highest priority when several), else the climate's
/// most likely ([`default_weather`]), as the engine's sky starts (`nvforever` sky.rs)
/// [hypothesis: the original's first weather].
pub fn start_weather(regions: &[RegionWeather], climate: Option<&Climate>, world: u32, x: f32, y: f32, global_value: impl Fn(u32) -> Option<f32> + Copy) -> Option<u32> {
    let region = regions.iter().filter(|r| r.overrides() && r.world == Some(world) && r.contains(x, y)).max_by_key(|r| r.priority);
    if let Some(w) = region.and_then(|r| eligible(&r.weathers, global_value).into_iter().reduce(|a, b| if b.chance > a.chance { b } else { a })) {
        return Some(w.weather);
    }
    default_weather(climate?, global_value)
}

/// The entries of a weather list (CLMT WLST, REGN RDWT) a roll draws from now
/// (docs/spec-notes/world.md §6 "Choosing a weather"): an entry with a global counts
/// only while that global is non-zero, and entries whose global is set and non-zero
/// win over the entries without a global; without any, the entries without a global
/// [hypothesis: NVDefaultClimate (and NewVegasRegion) list NVWastelandClearNight gated
/// by VNight, which NightQuestScript sets to 1 from 21:00 to 4:18, next to the ungated
/// NVWastelandClear, both chance 100; MapHooverDam lists NVHooverFinalBattle gated by
/// VStoryEventHooverDamBattleStarted next to the ungated NVHooverDamWeather, both 100.
/// A plain draw would show the day weather on half the nights and the peaceful weather
/// in half the battles, which the scripts setting those globals plainly do not mean].
pub fn eligible(list: &[WeatherChoice], global_value: impl Fn(u32) -> Option<f32>) -> Vec<WeatherChoice> {
    let open = |w: &&WeatherChoice| w.global.is_some_and(|g| global_value(g).is_some_and(|v| v != 0.0));
    let gated: Vec<WeatherChoice> = list.iter().filter(open).copied().collect();
    if !gated.is_empty() {
        return gated;
    }
    list.iter().filter(|w| w.global.is_none()).copied().collect()
}

/// A weighted draw from `list` (the [`eligible`] entries): `u` in 0..1 picks an entry
/// with probability chance / sum of chances; with no positive chance, every entry is
/// equally likely [hypothesis: Fallout 3's WeatherMegaton lists two weathers at chance 0
/// behind globals; the list is still meant to give one].
pub fn roll(list: &[WeatherChoice], u: f32) -> Option<u32> {
    let total: i64 = list.iter().map(|w| w.chance.max(0) as i64).sum();
    if total <= 0 {
        let n = list.len();
        return (n > 0).then(|| list[((u.clamp(0.0, 1.0) * n as f32) as usize).min(n - 1)].weather);
    }
    let mut at = u.clamp(0.0, 1.0) as f64 * total as f64;
    for w in list {
        let c = w.chance.max(0) as f64;
        if at < c {
            return Some(w.weather);
        }
        at -= c;
    }
    list.iter().rev().find(|w| w.chance > 0).map(|w| w.weather)
}

/// Whether the hourly volatility check calls a new roll: with probability volatility /
/// 255, `u` in 0..1 [hypothesis: GetClimateTraitNumeric names the TNAM byte
/// "Volatility" and nothing documents its scale or schedule; read as the chance per game
/// hour that the weather is rolled again. Every FalloutNV.esm climate stores 0, which
/// keeps the Mojave's weather steady, as the unmodded game's is known to be].
pub fn roll_due(volatility: u8, u: f32) -> bool {
    volatility > 0 && u < volatility as f32 / 255.0
}

/// REGN RDAT data type of a weather list [documented: xEdit wbRegionDataEntries type 3,
/// schemas/fnv/sky.json].
const WEATHER_DATA: u64 = 3;

/// REGN RDAT flags bit 0 on a weather block: the region's weathers replace the
/// climate's while the player is in it [hypothesis: JohnnyGuitar's
/// GetRegionWeatherOverride "returns true if Weather Region Data has an override flag"
/// (GECK wiki) names the flag, not its bit. Observed in FalloutNV.esm: of the WastelandNV
/// weather regions, the bit is set on those around one place with a weather of its own
/// (NVUrbanRegion, NVSearchlightRegion, NVJacobstownRegion, NVBlackMountainRegion,
/// NVLegateRegion, MapHooverDam, VMapTheFortRegion) and clear on GSWeatherRegion
/// (NVWastelandGS, beside the test climate NVDefaultClimateGSTest), NVBorderRegion and
/// NVColoradoRiverRegion; the lab's Goodsprings frames of the original name
/// NVWastelandClear].
pub const REGION_WEATHER_OVERRIDE: u8 = 1;

/// A region with a weather list (REGN RDAT type 3 + RDWT).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RegionWeather {
    pub form_id: u32,
    pub edid: Option<String>,
    /// WNAM: the worldspace its areas lie in.
    pub world: Option<u32>,
    /// RPLI/RPLD: each area a polygon of worldspace x, y points.
    pub areas: Vec<Vec<[f32; 2]>>,
    /// The weather block's RDAT flags and priority.
    pub flags: u8,
    pub priority: u8,
    pub weathers: Vec<WeatherChoice>,
}

impl RegionWeather {
    /// Whether the region's weathers replace the climate's ([`REGION_WEATHER_OVERRIDE`]).
    pub fn overrides(&self) -> bool {
        self.flags & REGION_WEATHER_OVERRIDE != 0
    }

    /// Whether a worldspace point lies inside one of the areas (as imposter regions).
    pub fn contains(&self, x: f32, y: f32) -> bool {
        self.areas.iter().any(|pts| crate::imposter::inside(pts, x, y))
    }
}

/// Every region of the load order with a weather list and at least one area, in FormID
/// order.
pub fn region_weathers(lo: &LoadOrder, schema: &Schema) -> Vec<RegionWeather> {
    let mut out = Vec::new();
    for (&g, defs) in &lo.index {
        let Some(d) = defs.last() else { continue };
        let pi = d.plugin as usize;
        let r = lo.records[pi][d.ordinal as usize];
        if &r.header.sig != b"REGN" || r.header.flags & crate::placed::ref_flags::DELETED != 0 {
            continue;
        }
        let Ok(f) = Fields::new(r, pi, schema) else { continue };
        let mut rg = RegionWeather { form_id: g, edid: f.string(b"EDID"), world: f.formid(b"WNAM", lo), ..Default::default() };
        let mut header: Option<Value> = None;
        for s in &f.subs {
            match &s.sig {
                b"RPLI" => rg.areas.push(Vec::new()),
                b"RPLD" => {
                    let pts: Vec<[f32; 2]> = f
                        .decode(s)
                        .as_ref()
                        .and_then(|v| v.as_array())
                        .into_iter()
                        .flatten()
                        .filter_map(|p| Some([num(p, "x")? as f32, num(p, "y")? as f32]))
                        .collect();
                    match rg.areas.last_mut() {
                        Some(a) if a.is_empty() => *a = pts,
                        _ => rg.areas.push(pts),
                    }
                }
                b"RDAT" => header = f.decode(s),
                b"RDWT" if header.as_ref().and_then(|h| h.get("type")?.as_u64()) == Some(WEATHER_DATA) => {
                    let h = header.as_ref();
                    rg.flags = h.and_then(|h| num(h, "flags")).unwrap_or(0.0) as u8;
                    rg.priority = h.and_then(|h| num(h, "priority")).unwrap_or(0.0) as u8;
                    for w in f.decode(s).as_ref().and_then(|v| v.as_array()).into_iter().flatten() {
                        if let Some(id) = w.get("weather").and_then(formid_of).and_then(|l| global(lo, pi, l)) {
                            rg.weathers.push(WeatherChoice {
                                weather: id,
                                chance: num(w, "chance").unwrap_or(0.0) as i32,
                                global: w.get("global").and_then(formid_of).and_then(|l| global(lo, pi, l)),
                            });
                        }
                    }
                }
                _ => {}
            }
        }
        rg.areas.retain(|a| a.len() >= 3);
        if !rg.weathers.is_empty() && !rg.areas.is_empty() {
            out.push(rg);
        }
    }
    out.sort_by_key(|r| r.form_id);
    out
}

/// The overriding weather region of worldspace `world` at (x, y): the highest priority
/// among those containing the point, the lowest FormID on ties [hypothesis: the
/// priority picks among overlapping regions; the tie rule is ours].
pub fn region_at(regions: &[RegionWeather], world: u32, x: f32, y: f32) -> Option<&RegionWeather> {
    regions
        .iter()
        .filter(|r| r.overrides() && r.world == Some(world) && r.contains(x, y))
        .reduce(|a, b| if b.priority > a.priority { b } else { a })
}

/// A global variable's value (GLOB FLTV).
pub fn global_value(lo: &LoadOrder, schema: &Schema, fid: u32) -> Option<f32> {
    let (pi, r) = winner(lo, fid)?;
    Fields::new(r, pi, schema).ok()?.get(b"FLTV")?.as_f64().map(|v| v as f32)
}

/// How far outside the climate's sunrise begin and sunset end the night key lies, as a
/// share of the sunrise's (sunset's) length [hypothesis, fitted: with the Mojave's sunrise
/// 6:00-8:00 and sunset 18:00-20:00 the original leaves night about 40 minutes before
/// 6:00 and reaches it about 35 minutes after 20:00. Under Lake Mead, 300 units down, the
/// picture's red rises evenly from about 5:20 to 8:00 (32 at 5:30, 41, 48, 54, 60 and 67 at
/// 6:00 to 8:00 by the half hour) and is night again at 20:30; the dusk sky fades to night
/// from about 19:05 to 20:35 (observer job o35_uw_dawn, 2026-11-27, and world.md §6 "West
/// sky at dusk"). A third of the 2-hour transitions is 40 minutes].
pub const NIGHT_KEY_BEYOND: f32 = 1.0 / 3.0;

/// Blend weights for the keyed times of day at `hour` [hypothesis: piecewise linear
/// between keys: midnight at 0 h, night until [`NIGHT_KEY_BEYOND`] of the sunrise's length
/// before it begins, the sunrise colour in the middle of sunrise, day from its end, high
/// noon at 12 h, day until sunset begins, the sunset colour in its middle, night from
/// [`NIGHT_KEY_BEYOND`] of its length after its end, midnight at 24 h. Without the
/// six-time layout, midnight and high noon fall back to night and day].
pub fn time_keys(c: &Climate, hour: f32, six: bool) -> [(TimeOfDay, f32); 2] {
    use TimeOfDay::*;
    let (mid, noon) = if six { (Midnight, HighNoon) } else { (Night, Day) };
    let sr_mid = (c.sunrise.0 + c.sunrise.1) / 2.0;
    let ss_mid = (c.sunset.0 + c.sunset.1) / 2.0;
    let dawn = (c.sunrise.0 - NIGHT_KEY_BEYOND * (c.sunrise.1 - c.sunrise.0).max(0.0)).max(0.0);
    let dusk = (c.sunset.1 + NIGHT_KEY_BEYOND * (c.sunset.1 - c.sunset.0).max(0.0)).min(24.0);
    let keys = [
        (0.0, mid),
        (dawn, Night),
        (sr_mid, Sunrise),
        (c.sunrise.1, Day),
        (12.0, noon),
        (c.sunset.0, Day),
        (ss_mid, Sunset),
        (dusk, Night),
        (24.0, mid),
    ];
    let h = hour.rem_euclid(24.0);
    for w in keys.windows(2) {
        let ((t0, k0), (t1, k1)) = (w[0], w[1]);
        if h >= t0 && h <= t1 {
            let f = if t1 > t0 { (h - t0) / (t1 - t0) } else { 0.0 };
            return [(k0, 1.0 - f), (k1, f)];
        }
    }
    [(Night, 1.0), (Night, 0.0)]
}

/// The keys the game blends at `hour` for a colour set or adapter list with (`six`) or
/// without the two extra times: [`time_keys`] with high noon, and night where it has
/// midnight [observed 2026-11-16, base game, FalloutNV.esm's WastelandClear, whose high-noon
/// and midnight columns are black: the picture is black at 12:06 and its level by the hour
/// follows the line from day at sunrise end (08:00) to high noon at 12:00 and back to day
/// at sunset begin (18:00); from 21:00 to 05:00 the sky stays (13, 14, 11), so midnight is
/// not blended in. world.md §6 "High noon"].
pub fn keys_at(c: &Climate, hour: f32, six: bool) -> [(TimeOfDay, f32); 2] {
    time_keys(c, hour, six).map(|(t, w)| (if t == TimeOfDay::Midnight { TimeOfDay::Night } else { t }, w))
}

impl Weather {
    /// A colour type at `hour`, linear RGB in 0..1 (bytes treated as sRGB).
    pub fn color(&self, c: &Climate, ty: ColorType, hour: f32) -> [f32; 3] {
        self.color_with(c, ty, hour, None)
    }

    /// [`Self::color`] with optional JIP `SetWeatherRGBColor` session overrides
    /// (`colors[type][time]`) applied in place without allocating.
    pub fn color_with(
        &self,
        c: &Climate,
        ty: ColorType,
        hour: f32,
        overrides: Option<&[[Option<[u8; 3]>; 6]; COLOR_TYPES]>,
    ) -> [f32; 3] {
        let slot_ov = overrides.map(|o| &o[ty as usize]);
        blend_with(self.colors.get(ty as usize), slot_ov, c, hour)
    }

    /// A colour type at `hour` as stored, 0..255 per channel, the times of day blended on
    /// the bytes themselves (no sRGB decoding), as the original's light level at an actor
    /// follows them (docs/design/detection.md §3) [observed: o27_light, 2026-11-06].
    pub fn color_bytes(&self, c: &Climate, ty: ColorType, hour: f32) -> [f32; 3] {
        self.color_bytes_with(c, ty, hour, None)
    }

    /// [`Self::color_bytes`] with optional JIP `SetWeatherRGBColor` session overrides
    /// (`colors[type][time]`) applied in place without allocating.
    pub fn color_bytes_with(
        &self,
        c: &Climate,
        ty: ColorType,
        hour: f32,
        overrides: Option<&[[Option<[u8; 3]>; 6]; COLOR_TYPES]>,
    ) -> [f32; 3] {
        let slot_ov = overrides.map(|o| &o[ty as usize]);
        blend_bytes_with(self.colors.get(ty as usize), slot_ov, c, hour)
    }

    /// Cloud layer `layer`'s colour at `hour`, linear RGB (same time model as [`Self::color`]).
    pub fn cloud_color(&self, c: &Climate, layer: usize, hour: f32) -> [f32; 3] {
        self.cloud_color_with(c, layer, hour, None)
    }

    /// [`Self::cloud_color`] with optional JIP `SetWeatherRGBColor` type-2 (`PNAM`)
    /// session overrides (`cloud_colors[layer][time]`) applied in place without allocating.
    pub fn cloud_color_with(
        &self,
        c: &Climate,
        layer: usize,
        hour: f32,
        overrides: Option<&[[Option<[u8; 3]>; 6]; 4]>,
    ) -> [f32; 3] {
        let slot_ov = overrides.and_then(|o| o.get(layer));
        blend_with(self.cloud_colors.get(layer), slot_ov, c, hour)
    }

    /// Fog near and far at `hour`, blending day and night values with [`day_factor`];
    /// `None` for no fog. A day or night range whose far is not beyond its near is unset:
    /// the other one is used for both, and with neither the weather has no fog
    /// [hypothesis: 6 of the 98 weathers in the masters store such ranges; HVDustStorm's
    /// day range is 0..0 but scripts only set it between 21:00 and 6:00, and the five
    /// all-zero weathers have black fog colours, which taken literally would draw their
    /// worlds solid black. docs/spec-notes/world.md §6].
    pub fn fog_at(&self, c: &Climate, hour: f32) -> Option<(f32, f32)> {
        self.fog_at_with(c, hour, None)
    }

    /// [`Self::fog_at`] with optional JIP `SetWeatherTraitNumeric` fog overrides
    /// (`15..=20`: day near/far, night near/far, day/night power) applied in place
    /// without allocating. Non-finite override values are ignored.
    pub fn fog_at_with(&self, c: &Climate, hour: f32, overrides: Option<[Option<f32>; 6]>) -> Option<(f32, f32)> {
        let fog = self.merged_fog(overrides);
        let range = |near: f32, far: f32| (far > near).then_some((near, far));
        let (day, night) = match (range(fog[0], fog[1]), range(fog[2], fog[3])) {
            (Some(day), Some(night)) => (day, night),
            (Some(r), None) | (None, Some(r)) => (r, r),
            (None, None) => return None,
        };
        let d = day_factor(c, hour);
        Some((night.0 + (day.0 - night.0) * d, night.1 + (day.1 - night.1) * d))
    }

    /// Fog power at `hour` (FNAM day and night power, blended like [`Self::fog_at`]; an
    /// unset range takes the other range's power, and a power that is not above 0 reads
    /// as 1, a linear fog). The renderers' fog share is ((distance − near) / (far −
    /// near))^power [hypothesis, fitted to Dead Money's frames: docs/spec-notes/world.md
    /// §6 "Fog power"]. Stored powers: 0.2 to 1.0 in the masters' 98 weathers [observed].
    pub fn fog_power_at(&self, c: &Climate, hour: f32) -> f32 {
        self.fog_power_at_with(c, hour, None)
    }

    /// [`Self::fog_power_at`] with optional JIP `SetWeatherTraitNumeric` fog overrides
    /// (`15..=20`) applied in place without allocating. Non-finite values are ignored.
    pub fn fog_power_at_with(&self, c: &Climate, hour: f32, overrides: Option<[Option<f32>; 6]>) -> f32 {
        let fog = self.merged_fog(overrides);
        let set = |near: f32, far: f32| far > near;
        let p = |v: f32| if v > 0.0 { v } else { 1.0 };
        let (day, night) = (p(fog[4]), p(fog[5]));
        let (day, night) = match (set(fog[0], fog[1]), set(fog[2], fog[3])) {
            (true, false) => (day, day),
            (false, true) => (night, night),
            _ => (day, night),
        };
        night + (day - night) * day_factor(c, hour)
    }

    fn merged_fog(&self, overrides: Option<[Option<f32>; 6]>) -> [f32; 6] {
        let mut fog = self.fog;
        if let Some(ov) = overrides {
            for (dst, val) in fog.iter_mut().zip(ov) {
                if let Some(v) = val.filter(|v| v.is_finite()) {
                    *dst = v;
                }
            }
        }
        fog
    }
}

/// A per-time colour set blended at `hour`, linear RGB (bytes treated as sRGB). The times
/// are blended on the bytes themselves, then decoded [observed 2026-11-16: with a black
/// high-noon column the picture's level at 10:06, 11:06 and 11:36 is 0.54, 0.21 and 0.08
/// of the day's, the day key's weights 0.475, 0.225 and 0.1; blended in linear light they
/// would show as 0.71, 0.51 and 0.35]. A six-time set uses its high-noon column and never
/// its midnight one ([`keys_at`]). In 29 of the 90 six-time weathers high noon is black
/// [observed, `crates/fnv-world/examples/weather_dump.rs --survey`]; the base game's own
/// `WastelandClear` is one and goes black at noon in the original [observed], the others
/// would too [hypothesis: only that one was looked at]. The Mojave's climate
/// (`NVWastelandClear`, `NVWastelandClearNight`) has high-noon colours of its own.
fn blend_bytes_with(set: Option<&Vec<[u8; 3]>>, overrides: Option<&[Option<[u8; 3]>; 6]>, c: &Climate, hour: f32) -> [f32; 3] {
    let any_ov = overrides.is_some_and(|ov| ov.iter().any(Option::is_some));
    if set.is_none() && !any_ov {
        return [0.0; 3];
    }
    let six = set.is_some_and(|s| s.len() >= 6) || overrides.is_some_and(|ov| ov[4].is_some());
    let mut out = [0.0f32; 3];
    for (k, w) in keys_at(c, hour, six) {
        let idx = k as usize;
        let col = overrides
            .and_then(|ov| ov.get(idx).copied().flatten())
            .or_else(|| set.and_then(|s| s.get(idx).copied()))
            .unwrap_or([0; 3]);
        for i in 0..3 {
            out[i] += w * col[i] as f32;
        }
    }
    out
}

fn blend_with(set: Option<&Vec<[u8; 3]>>, overrides: Option<&[Option<[u8; 3]>; 6]>, c: &Climate, hour: f32) -> [f32; 3] {
    let any_ov = overrides.is_some_and(|ov| ov.iter().any(Option::is_some));
    if set.is_none() && !any_ov {
        return [0.0; 3];
    }
    let bytes = blend_bytes_with(set, overrides, c, hour);
    bytes.map(|v| {
        let s = (v / 255.0).clamp(0.0, 1.0);
        if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    })
}

pub fn srgb_to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// 1 in full day (sunrise end .. sunset begin), 0 at night, linear across sunrise and
/// sunset [hypothesis].
pub fn day_factor(c: &Climate, hour: f32) -> f32 {
    let h = hour.rem_euclid(24.0);
    let ramp = |a: f32, b: f32| if b > a { ((h - a) / (b - a)).clamp(0.0, 1.0) } else { (h >= a) as u8 as f32 };
    (ramp(c.sunrise.0, c.sunrise.1) - ramp(c.sunset.0, c.sunset.1)).clamp(0.0, 1.0)
}

/// Unit vector towards the sun in game space (x east, y north, z up). The sun rises due
/// east at sunrise begin and sets due west at sunset end, moving at a constant rate along
/// a path held `south` radians south of the east-west line: the direction is
/// normalize(cos θ, −tan(south), sin θ) with θ going from 0 to π over that time. Below the
/// horizon at night, where θ goes from π to 2π at the night's own constant rate, so the
/// path closes at sunrise and the moon drawn opposite peaks halfway through the night
/// (one rate for the whole day jumped θ by 51° at midnight in the Mojave). [hypothesis
/// fitted to sun positions in screenshots of the original game at Goodsprings,
/// 08:36-18:21, with south = 9°: 3.6° RMS over 14 positions; the night half is
/// unobserved; docs/spec-notes/world.md §6.]
pub fn sun_direction(c: &Climate, hour: f32, south: f32) -> [f32; 3] {
    use std::f32::consts::PI;
    let (rise, set) = (c.sunrise.0, c.sunset.1);
    let day = (set - rise).clamp(1e-3, 24.0 - 1e-3);
    let t = (hour - rise).rem_euclid(24.0);
    let theta = if t <= day { PI * t / day } else { PI + PI * (t - day) / (24.0 - day) };
    let (s, co) = theta.sin_cos();
    let k = -south.tan();
    let n = (co * co + k * k + s * s).sqrt();
    [co / n, k / n, s / n]
}

/// One of the three scrolling noise layers of a water type (WATR DNAM) [documented: field
/// names, xEdit]. How the engine uses them is a hypothesis (world.md §6).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct NoiseLayer {
    /// Wind direction in degrees.
    pub wind_direction: f32,
    pub wind_speed: f32,
    /// 0 in the masters' NVCleanWater [observed].
    pub uv_scale: f32,
    pub amplitude: f32,
}

/// Ripple settings of a water type for rain drops or for things moving through the
/// water ("displacement") (WATR DNAM) [documented: field names, xEdit]. Their meaning
/// in the engine is a hypothesis (world.md §6).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RippleParams {
    pub starting_size: f32,
    pub force: f32,
    pub velocity: f32,
    pub falloff: f32,
    pub dampner: f32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Water {
    pub form_id: u32,
    pub edid: Option<String>,
    pub noise_texture: Option<String>,
    /// ANAM opacity (0-255).
    pub opacity: u8,
    /// FNAM bit 1 "Reflective" [documented].
    pub reflective: bool,
    pub shallow: [u8; 3],
    pub deep: [u8; 3],
    pub reflection: [u8; 3],
    pub sun_power: f32,
    pub reflectivity: f32,
    pub fresnel: f32,
    pub shininess: f32,
    /// Fog seen through the surface from above: near, far, amount.
    pub above_fog: [f32; 3],
    /// Fog below the surface: near, far, amount.
    pub under_fog: [f32; 3],
    pub noise_normals_scale: f32,
    pub noise_normals_uv_scale: f32,
    /// DNAM "Depth Falloff Start" and "End": "where the alpha fadeoff begins (used for
    /// edges of the water, to blend it smoothly with the land)" and ends [documented:
    /// GECK wiki WaterType; xEdit files them under the noise normals]. The masters store
    /// fractions (0 to 0.8; NVCleanWater 0 to 0.0109) [observed]; what they are fractions
    /// of is not documented ([`Water::edge_falloff`]).
    pub depth_falloff: [f32; 2],
    pub noise_layers: [NoiseLayer; 3],
    pub rain: RippleParams,
    pub displacement: RippleParams,
}

pub fn water(lo: &LoadOrder, schema: &Schema, fid: u32) -> Option<Water> {
    let (pi, r) = winner(lo, fid)?;
    let f = Fields::new(r, pi, schema).ok()?;
    let mut w = Water {
        form_id: fid,
        edid: f.string(b"EDID"),
        noise_texture: f.string(b"NNAM").filter(|s| !s.is_empty()),
        opacity: f.get(b"ANAM").and_then(|v| v.as_u64()).unwrap_or(255) as u8,
        reflective: f.get(b"FNAM").and_then(|v| num(&v, "value")).is_some_and(|v| (v as u64) & 2 != 0),
        ..Default::default()
    };
    if let Some(d) = f.get(b"DNAM") {
        let n = |k: &str| num(&d, k).unwrap_or(0.0) as f32;
        w.shallow = d.get("shallow_color").and_then(byte_color).unwrap_or_default();
        w.deep = d.get("deep_color").and_then(byte_color).unwrap_or_default();
        w.reflection = d.get("reflection_color").and_then(byte_color).unwrap_or_default();
        w.sun_power = n("sun_power");
        w.reflectivity = n("reflectivity_amount");
        w.fresnel = n("fresnel_amount");
        w.shininess = n("shininess");
        w.above_fog = [n("above_fog_near"), n("above_fog_far"), n("above_fog_amount")];
        w.under_fog = [n("under_fog_near"), n("under_fog_far"), n("under_fog_amount")];
        w.noise_normals_scale = n("noise_normals_scale");
        w.noise_normals_uv_scale = n("noise_normals_uv_scale");
        w.depth_falloff = [n("noise_depth_falloff_start"), n("noise_depth_falloff_end")];
        let ripple = |p: &str, size: &str| RippleParams {
            starting_size: n(size),
            force: n(&format!("{p}_force")),
            velocity: n(&format!("{p}_velocity")),
            falloff: n(&format!("{p}_falloff")),
            dampner: n(&format!("{p}_dampner")),
        };
        w.rain = ripple("rain", "rain_starting_size");
        w.displacement = ripple("displacement", "displacement_starting_size");
        for (i, l) in w.noise_layers.iter_mut().enumerate() {
            let k = i + 1;
            *l = NoiseLayer {
                wind_direction: n(&format!("noise_layer{k}_wind_direction")),
                wind_speed: n(&format!("noise_layer{k}_wind_speed")),
                uv_scale: n(&format!("noise_layer{k}_uv_scale")),
                amplitude: n(&format!("noise_layer{k}_amplitude")),
            };
        }
    }
    Some(w)
}

impl Water {
    /// The depth falloff in game units of water the view ray crosses: start and end of
    /// the ramp over which the surface fades in from the shore. The stored fractions are
    /// taken as fractions of the above-water fog's far distance [hypothesis: the masters'
    /// fractions give ramps of a few units to a few tens (NVCleanWater 9, the Potomac's
    /// 37, the Goodsprings blood pools' 5), an edge; game units outright would be
    /// hundredths of a unit]. `None` when there is no ramp.
    pub fn edge_falloff(&self) -> Option<(f32, f32)> {
        let far = self.above_fog[1];
        let [a, b] = self.depth_falloff;
        if far.is_nan() || far <= 0.0 || !a.is_finite() || !b.is_finite() || b <= a {
            return None;
        }
        Some((a.max(0.0) * far, b * far))
    }
}

/// The water type (WATR) of a placeable water (PWAT): its `DNAM` "Water" [documented:
/// xEdit PWAT; observed: `WaterBlood` (the Goodsprings cave's pools) names
/// `WaterTypeBlood`].
pub fn placeable_water_type(lo: &LoadOrder, schema: &Schema, pwat: u32) -> Option<u32> {
    let (pi, r) = winner(lo, pwat)?;
    let f = Fields::new(r, pi, schema).ok()?;
    if &f.sig != b"PWAT" {
        return None;
    }
    global(lo, pi, formid_of(f.get(b"DNAM")?.get("water")?)?)
}

/// Whether a placeable water (PWAT) reflects at all: its `DNAM` flags bit 0 "Reflects"
/// [documented: xEdit PWAT; the GECK's PlaceableWater dialog has a "Reflects" group that
/// "determines which objects can be reflected in this water object"]. Taken as the
/// switch for the whole reflection, the sky's and the sun's highlight included, whatever
/// the other "Reflects - …" bits say [hypothesis, world.md §6 "Placed water": the
/// Ultra-Luxe fountain's PWAT sets "Reflects - Land" and "Reflects - Sky" without it and
/// shows no reflection in the original]. True when the record has no flags to read.
pub fn placeable_water_reflects(lo: &LoadOrder, schema: &Schema, pwat: u32) -> bool {
    let Some((pi, r)) = winner(lo, pwat) else { return true };
    let Ok(f) = Fields::new(r, pi, schema) else { return true };
    pwat_reflects(f.get(b"DNAM").as_ref())
}

/// [`placeable_water_reflects`] from a decoded PWAT `DNAM`.
fn pwat_reflects(dnam: Option<&Value>) -> bool {
    dnam.and_then(|d| d.get("flags")).and_then(|v| num(v, "value")).is_none_or(|v| (v as u64) & 1 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clim() -> Climate {
        Climate { sunrise: (6.0, 8.0), sunset: (18.0, 20.0), ..Default::default() }
    }

    #[test]
    fn cloud_scroll_from_onam() {
        let w = Weather { cloud_speeds: [52, 0, 0, 65], ..Default::default() };
        assert_eq!(w.cloud_scroll(0), 52.0 / 25_600.0);
        assert_eq!(w.cloud_scroll(1), 0.0);
        assert_eq!(w.cloud_scroll(4), 0.0);
        // A still layer stays put; a moving one wraps at one repeat.
        assert_eq!(w.cloud_offset(1, 1.0e6), 0.0);
        assert!((w.cloud_offset(3, 100.0) - 6_500.0 / 25_600.0).abs() < 1e-6);
        let one_repeat = 25_600.0 / 65.0;
        assert!(w.cloud_offset(3, one_repeat) < 1e-4 || w.cloud_offset(3, one_repeat) > 1.0 - 1e-4);
        // Hours into a session the offset still moves smoothly.
        let (a, b) = (w.cloud_offset(3, 36_000.0), w.cloud_offset(3, 36_001.0));
        assert!(((b - a).rem_euclid(1.0) - 65.0 / 25_600.0).abs() < 1e-5);
    }

    #[test]
    fn color_bytes_blend_the_stored_bytes() {
        // NVWastelandClear's sunlight: sunrise, day, sunset, night [observed: FalloutNV.esm].
        let mut colors = vec![Vec::new(); COLOR_TYPES];
        colors[ColorType::Sunlight as usize] = vec![[255, 157, 111], [255, 227, 170], [255, 173, 83], [166, 191, 210]];
        let w = Weather { colors, ..Default::default() };
        let c = clim();
        assert_eq!(w.color_bytes(&c, ColorType::Sunlight, 12.0), [255.0, 227.0, 170.0]);
        assert_eq!(w.color_bytes(&c, ColorType::Sunlight, 0.0), [166.0, 191.0, 210.0]);
        // Between two keys the bytes are mixed as they are, not their linear values.
        let [(k0, w0), (k1, w1)] = time_keys(&c, 19.0, false);
        let set = &w.colors[ColorType::Sunlight as usize];
        let want: [f32; 3] = std::array::from_fn(|i| w0 * set[k0 as usize][i] as f32 + w1 * set[k1 as usize][i] as f32);
        let got = w.color_bytes(&c, ColorType::Sunlight, 19.0);
        assert!((0..3).all(|i| (got[i] - want[i]).abs() < 1e-3), "{got:?} {want:?}");
        assert_eq!(w.color_bytes(&c, ColorType::Ambient, 12.0), [0.0; 3]);
    }

    #[test]
    fn pwat_reflects_bit_0() {
        // The Ultra-Luxe fountain's DupontCircle01Water: Reflects - Land, Reflects - Sky,
        // the refracts bits and Depth, not Reflects [observed: FalloutNV.esm].
        let fountain = serde_json::json!({ "flags": { "value": 268635716u64 }, "water": "0x0016663E" });
        assert!(!pwat_reflects(Some(&fountain)));
        assert!(pwat_reflects(Some(&serde_json::json!({ "flags": { "value": 0x201u64 } }))));
        assert!(pwat_reflects(None));
    }

    #[test]
    fn water_edge_falloff() {
        let w = Water { above_fog: [-80.0, 850.0, 1.0], depth_falloff: [0.0, 0.0109], ..Default::default() };
        let (a, b) = w.edge_falloff().unwrap();
        assert_eq!(a, 0.0);
        assert!((b - 9.265).abs() < 1e-3);
        assert_eq!(Water { depth_falloff: [0.0, 0.0], ..w.clone() }.edge_falloff(), None);
        assert_eq!(Water { above_fog: [0.0, 0.0, 1.0], ..w }.edge_falloff(), None);
    }

    #[test]
    fn transition_and_glare_from_the_record() {
        let nv = Weather { trans_delta: Some(255), sun_glare: Some(54), ..Default::default() };
        assert_eq!(nv.transition_seconds(), 30.0);
        assert_eq!(nv.glare_scale(), 1.0);
        let at_once = Weather { trans_delta: Some(0), sun_glare: Some(0), ..Default::default() };
        assert_eq!(at_once.transition_seconds(), 0.0);
        assert_eq!(at_once.glare_scale(), 0.0);
        // No DATA: as a New Vegas weather.
        assert_eq!(Weather::default().transition_seconds(), 30.0);
        assert_eq!(Weather::default().glare_scale(), 1.0);
    }

    #[test]
    fn moon_phases() {
        let c = Climate { moons: 131, ..Default::default() };
        assert_eq!(c.moon_phase_length(), 3);
        assert!(c.has_moon());
        assert_eq!(moon_phase(0.0, 3), 0);
        assert_eq!(moon_phase(2.9, 3), 0);
        assert_eq!(moon_phase(3.0, 3), 1);
        assert_eq!(moon_phase(23.9, 3), 7);
        assert_eq!(moon_phase(24.0, 3), 0);
        assert_eq!(moon_phase(-1.0, 3), 7);
        assert_eq!(moon_phase(10.0, 0), 0);
        assert!((smoothstep(0.0, -12.0, -6.0) - 0.5).abs() < 1e-6);
        assert_eq!(smoothstep(0.0, -12.0, 5.0), 0.0);
        assert_eq!(smoothstep(0.0, -12.0, -20.0), 1.0);
    }

    #[test]
    fn keys_cover_the_day() {
        let c = clim();
        let weight = |k: [(TimeOfDay, f32); 2], t: TimeOfDay| k.iter().filter(|x| x.0 == t).map(|x| x.1).sum::<f32>();
        assert!((weight(time_keys(&c, 7.0, true), TimeOfDay::Sunrise) - 1.0).abs() < 1e-6);
        assert!((weight(time_keys(&c, 7.5, true), TimeOfDay::Day) - 0.5).abs() < 1e-6);
        let k = time_keys(&c, 12.0, true);
        assert!(k.iter().any(|&(t, w)| t == TimeOfDay::HighNoon && (w - 1.0).abs() < 1e-6));
        let k = time_keys(&c, 3.0, false);
        assert!(k.iter().all(|&(t, _)| t == TimeOfDay::Night));
        // The night key lies a third of the transition outside it: with sunrise 6-8 and
        // sunset 18-20, night until 5:20 and from 20:40; 6:00 is already part of the way.
        let sr = c.sunrise.0 - NIGHT_KEY_BEYOND * (c.sunrise.1 - c.sunrise.0);
        let ss = c.sunset.1 + NIGHT_KEY_BEYOND * (c.sunset.1 - c.sunset.0);
        assert!((weight(time_keys(&c, sr, false), TimeOfDay::Night) - 1.0).abs() < 1e-5);
        assert!((weight(time_keys(&c, ss, false), TimeOfDay::Night) - 1.0).abs() < 1e-5);
        assert!(weight(time_keys(&c, c.sunrise.0, false), TimeOfDay::Sunrise) > 0.2);
        assert!(weight(time_keys(&c, c.sunset.1, false), TimeOfDay::Sunset) > 0.2);
        for h in 0..48 {
            let k = time_keys(&c, h as f32 * 0.5, true);
            assert!((k[0].1 + k[1].1 - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn day_factor_ramps() {
        let c = clim();
        assert_eq!(day_factor(&c, 2.0), 0.0);
        assert_eq!(day_factor(&c, 7.0), 0.5);
        assert_eq!(day_factor(&c, 12.0), 1.0);
        assert_eq!(day_factor(&c, 19.5), 0.25);
        assert_eq!(day_factor(&c, 23.0), 0.0);
    }

    #[test]
    fn fog_ranges() {
        let c = clim();
        let w = |fog: [f32; 6]| Weather { fog, ..Default::default() };
        // NVWastelandClear-like: both ranges set, blended by day factor.
        let clear = w([-10.0, 200_000.0, -10.0, 100_000.0, 0.6, 0.5]);
        assert_eq!(clear.fog_at(&c, 12.0), Some((-10.0, 200_000.0)));
        assert_eq!(clear.fog_at(&c, 7.0), Some((-10.0, 150_000.0)));
        // HVDustStorm: unset day range falls back to the night one.
        let storm = w([0.0, 0.0, 510.0, 55_000.0, 1.0, 1.0]);
        assert_eq!(storm.fog_at(&c, 12.0), Some((510.0, 55_000.0)));
        assert_eq!(storm.fog_at(&c, 2.0), Some((510.0, 55_000.0)));
        // All zero: no fog.
        assert_eq!(w([0.0, 0.0, 0.0, 0.0, 1.0, 1.0]).fog_at(&c, 12.0), None);
        // Powers blend like the ranges; an unset range takes the other's power.
        assert_eq!(clear.fog_power_at(&c, 12.0), 0.6);
        assert!((clear.fog_power_at(&c, 7.0) - 0.55).abs() < 1e-6);
        assert_eq!(clear.fog_power_at(&c, 23.0), 0.5);
        let storm = w([0.0, 0.0, 510.0, 55_000.0, 0.3, 0.8]);
        assert_eq!(storm.fog_power_at(&c, 12.0), 0.8);
        assert_eq!(w([0.0, 1.0, 0.0, 1.0, 0.0, -1.0]).fog_power_at(&c, 12.0), 1.0);
    }

    #[test]
    fn sun_path() {
        let c = clim();
        let el = |h: f32| sun_direction(&c, h, 9f32.to_radians())[2].asin().to_degrees();
        // Rises at sunrise begin, sets at sunset end, highest half way (90° − 9°).
        let rise = sun_direction(&c, 6.0, 0.0);
        assert!((rise[0] - 1.0).abs() < 1e-5 && rise[2].abs() < 1e-5);
        assert!(el(6.0).abs() < 1e-3 && el(20.0).abs() < 1e-3);
        assert!((el(13.0) - 81.0).abs() < 1e-3);
        let noon = sun_direction(&c, 13.0, 9f32.to_radians());
        assert!(noon[1] < 0.0 && (noon.iter().map(|v| v * v).sum::<f32>() - 1.0).abs() < 1e-5);
        // Still about 12° up at 19:06 (the dusk reference frame), below the horizon at night.
        assert!((el(19.1) - 11.8).abs() < 0.5);
        assert!(el(1.0) < 0.0 && el(21.0) < 0.0);
        // Lowest halfway through the night (01:00), mirroring the noon peak.
        assert!((el(1.0) + 81.0).abs() < 1e-3);
        // No jumps anywhere in the day, midnight and sunrise included: the direction
        // moves less than 0.5° per game minute.
        let dir = |h: f32| sun_direction(&c, h, 9f32.to_radians());
        for m in 0..24 * 60 {
            let (a, b) = (dir(m as f32 / 60.0), dir((m + 1) as f32 / 60.0));
            let cos = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
            assert!(cos > 0.5f32.to_radians().cos(), "jump at minute {m}");
        }
    }

    #[test]
    fn time_units() {
        assert_eq!(climate_hours(36.0), 6.0);
        assert_eq!(climate_hours(111.0), 18.5);
    }

    #[test]
    fn default_weather_is_most_likely() {
        let w = |weather, chance, global| WeatherChoice { weather, chance, global };
        let c = Climate { weathers: vec![w(1, 20, None), w(2, 60, None), w(3, 60, None)], ..clim() };
        assert_eq!(default_weather(&c, |_| None), Some(2));
        let c = Climate { weathers: vec![w(4, 100, Some(9)), w(5, 100, None)], ..clim() };
        assert_eq!(default_weather(&c, |_| Some(0.0)), Some(5));
        assert_eq!(default_weather(&c, |_| Some(1.0)), Some(4));
    }

    #[test]
    fn open_gates_win_over_ungated_entries() {
        let w = |weather, chance, global| WeatherChoice { weather, chance, global };
        let list = [w(4, 100, Some(9)), w(5, 100, None), w(6, 50, None)];
        assert_eq!(eligible(&list, |_| Some(0.0)), vec![list[1], list[2]]);
        assert_eq!(eligible(&list, |_| Some(1.0)), vec![list[0]]);
        assert_eq!(eligible(&list, |_| None), vec![list[1], list[2]]);
    }

    #[test]
    fn rolls_are_weighted_by_chance() {
        let w = |weather, chance| WeatherChoice { weather, chance, global: None };
        let urban = [w(1, 85), w(2, 15)];
        assert_eq!(roll(&urban, 0.0), Some(1));
        assert_eq!(roll(&urban, 0.84), Some(1));
        assert_eq!(roll(&urban, 0.86), Some(2));
        assert_eq!(roll(&urban, 1.0), Some(2));
        let zero = [w(3, 0), w(4, 0)];
        assert_eq!(roll(&zero, 0.2), Some(3));
        assert_eq!(roll(&zero, 0.7), Some(4));
        assert_eq!(roll(&[], 0.5), None);
        let n = (0..1000).filter(|i| roll(&urban, *i as f32 / 1000.0) == Some(2)).count();
        assert_eq!(n, 150);
    }

    #[test]
    fn volatility_is_a_chance_per_hour() {
        assert!(!roll_due(0, 0.0));
        assert!(roll_due(255, 0.999));
        assert!(roll_due(51, 0.19) && !roll_due(51, 0.21));
    }

    #[test]
    fn override_regions_by_priority() {
        let square = vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let r = |form_id, flags, priority| RegionWeather { form_id, world: Some(7), areas: vec![square.clone()], flags, priority, ..Default::default() };
        let rs = [r(1, 0, 200), r(2, 1, 50), r(3, 1, 100), r(4, 1, 100)];
        assert_eq!(region_at(&rs, 7, 5.0, 5.0).map(|r| r.form_id), Some(3));
        assert_eq!(region_at(&rs, 7, 50.0, 5.0).map(|r| r.form_id), None);
        assert_eq!(region_at(&rs, 8, 5.0, 5.0).map(|r| r.form_id), None);
        assert_eq!(region_at(&rs[..2], 7, 5.0, 5.0).map(|r| r.form_id), Some(2));
    }

    #[test]
    fn fog_at_with_applies_session_overrides_and_ignores_non_finite() {
        let c = Climate { sunrise: (6.0, 8.0), sunset: (18.0, 20.0), ..Default::default() };
        let w = Weather { fog: [1000.0, 100_000.0, 500.0, 50_000.0, 1.0, 1.0], ..Default::default() };
        assert_eq!(w.fog_at(&c, 12.0), Some((1000.0, 100_000.0)));
        assert_eq!(w.fog_at(&c, 0.0), Some((500.0, 50_000.0)));
        let ov = [Some(250.0), Some(12_000.0), Some(100.0), Some(8_000.0), Some(0.6), Some(0.4)];
        assert_eq!(w.fog_at_with(&c, 12.0, Some(ov)), Some((250.0, 12_000.0)));
        assert_eq!(w.fog_at_with(&c, 0.0, Some(ov)), Some((100.0, 8_000.0)));
        assert!((w.fog_power_at_with(&c, 12.0, Some(ov)) - 0.6).abs() < 1e-6);
        assert!((w.fog_power_at_with(&c, 0.0, Some(ov)) - 0.4).abs() < 1e-6);
        // Partial override leaves untouched slots at authored values; non-finite is ignored.
        let partial = [Some(f32::NAN), Some(20_000.0), None, Some(f32::INFINITY), None, None];
        assert_eq!(w.fog_at_with(&c, 12.0, Some(partial)), Some((1000.0, 20_000.0)));
        assert_eq!(w.fog_at_with(&c, 0.0, Some(partial)), Some((500.0, 50_000.0)));
    }

    #[test]
    fn weather_color_with_overlays_nam0_and_pnam_without_mutating_authored() {
        let c = Climate { sunrise: (6.0, 8.0), sunset: (18.0, 20.0), ..Default::default() };
        let mut w = Weather {
            colors: vec![vec![[10, 20, 30]; 4]; COLOR_TYPES],
            cloud_colors: vec![vec![[40, 50, 60]; 4]; 4],
            ..Default::default()
        };
        assert_eq!(w.color_with(&c, ColorType::Fog, 12.0, None), w.color(&c, ColorType::Fog, 12.0));
        assert_eq!(w.color_bytes_with(&c, ColorType::Ambient, 12.0, None), [10.0, 20.0, 30.0]);
        assert_eq!(w.cloud_color_with(&c, 0, 12.0, None), w.cloud_color(&c, 0, 12.0));

        // Override Day Fog and HighNoon Ambient (activating 6-time HighNoon on a 4-time base).
        let mut ov = [[None; 6]; COLOR_TYPES];
        ov[ColorType::Fog as usize][TimeOfDay::Day as usize] = Some([200, 100, 50]);
        ov[ColorType::Ambient as usize][TimeOfDay::HighNoon as usize] = Some([120, 140, 160]);
        assert_eq!(w.color_bytes_with(&c, ColorType::Fog, 12.0, Some(&ov)), [200.0, 100.0, 50.0]);
        assert_eq!(w.color_bytes_with(&c, ColorType::Ambient, 12.0, Some(&ov)), [120.0, 140.0, 160.0]);
        // Untouched types and authored fields remain intact.
        assert_eq!(w.color_bytes_with(&c, ColorType::Sunlight, 12.0, Some(&ov)), [10.0, 20.0, 30.0]);
        assert_eq!(w.color_bytes(&c, ColorType::Fog, 12.0), [10.0, 20.0, 30.0]);

        // PNAM lower cloud layer override stays scoped to the overridden layer.
        let mut cloud_ov = [[None; 6]; 4];
        cloud_ov[1][TimeOfDay::Day as usize] = Some([255, 128, 0]);
        let lin = w.cloud_color_with(&c, 1, 12.0, Some(&cloud_ov));
        assert!((lin[0] - 1.0).abs() < 1e-5 && lin[1] > 0.2 && lin[2] < 1e-5);
        assert_eq!(w.cloud_color_with(&c, 0, 12.0, Some(&cloud_ov)), w.cloud_color(&c, 0, 12.0));

        // Weather with empty authored color vectors still resolves explicit overrides.
        w.colors.clear();
        w.cloud_colors.clear();
        assert_eq!(w.color_bytes_with(&c, ColorType::Fog, 12.0, None), [0.0, 0.0, 0.0]);
        assert_eq!(w.color_bytes_with(&c, ColorType::Fog, 12.0, Some(&ov)), [200.0, 100.0, 50.0]);
        assert!(w.cloud_color_with(&c, 1, 12.0, Some(&cloud_ov))[0] > 0.99);
    }
}
