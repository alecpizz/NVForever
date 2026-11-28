//! The weather scripts ask for and the one on the screen (docs/design/script-stubs.md
//! §3): SetWeather, ForceWeather, ReleaseWeatherOverride, IsRaining and
//! GetIsCurrentWeather, GetCurrentWeatherPercent and classification queries.
//! The sky (`nvforever` `sky.rs`)
//! takes the request and tells
//! back which weather it shows ([`Game::set_shown_weather`]).

use crate::functions::{none, yes, Args, Ctx};
use crate::game::Game;
use fnv_script::vm::Value;
use fnv_sim::FormId;

/// Vanilla `NVDefaultWeather` (`FalloutNV.esm` WTHR `0x00064609`), already active and
/// settled at `1.0` (Pleasant) on a cold interior start before visiting any exterior
/// [observed: w4g_04_weather_cold_start].
pub const NV_DEFAULT_WEATHER: FormId = FormId(0x0006_4609);

/// A weather a script asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeatherRequest {
    pub weather: FormId,
    /// ForceWeather: at once, without the change's fade; SetWeather: the normal change.
    pub instant: bool,
    /// The override flag: the climate does not change the weather again until
    /// ReleaseWeatherOverride.
    pub hold: bool,
}

/// The weather as scripts set it and as the engine shows it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WeatherState {
    /// The latest request, until the sky takes it.
    pub request: Option<WeatherRequest>,
    /// ReleaseWeatherOverride was called and the sky has not taken it yet.
    pub release: bool,
    /// The weather the sky shows (its form), as the engine last told.
    pub shown: Option<FormId>,
    /// The weather shown is classified rainy and the change to it is over.
    pub raining: bool,
    /// The weather shown before the one shown now (ClimateControl's
    /// GetOutgoingWeather), as the engine told the change.
    pub outgoing: Option<FormId>,
    /// Last finite sky measurement; transient and absent before the host reports.
    measured_fraction: Option<f32>,
    /// Destination classification and settled state from the same finite report.
    /// A fraction-only host must not reuse an earlier classification measurement.
    measured_classification: Option<(u8, bool)>,
}

impl WeatherState {
    /// Cold-start weather state: when the load order defines vanilla `NVDefaultWeather`
    /// (`0x00064609`, WTHR), start with it active and settled at progress `1.0` using its
    /// authored WTHR DATA classification (defaulting to Pleasant `1` if `DATA` is omitted)
    /// [observed: w4g_04_weather_cold_start]. Synthetic load orders without `0x00064609`
    /// remain unmeasured (`0.0`) until a sky sample is reported [design].
    pub(crate) fn cold_start(data: &crate::GameData) -> Self {
        if data.sig(NV_DEFAULT_WEATHER) != Some(*b"WTHR") {
            return Self::default();
        }
        let classification = data
            .record_field(NV_DEFAULT_WEATHER, "DATA", "flags")
            .and_then(|v| v.as_u64())
            .map_or(1, |v| v as u8);
        Self {
            shown: Some(NV_DEFAULT_WEATHER),
            raining: classification & 4 != 0,
            measured_fraction: Some(1.0),
            measured_classification: Some((classification, true)),
            ..Self::default()
        }
    }

    /// Clear transient sky measurements when restoring a saved session (`Game::restore`),
    /// so restored sessions wait for the host sky bridge to report their live state.
    pub(crate) fn clear_measurement(&mut self) {
        self.measured_fraction = None;
        self.measured_classification = None;
    }
}

impl Game<'_> {
    /// The sky takes the script's weather request, if one is waiting.
    pub fn take_weather_request(&mut self) -> Option<WeatherRequest> {
        self.weather.request.take()
    }

    /// The sky takes a release of the override, if one is waiting.
    pub fn take_weather_release(&mut self) -> bool {
        std::mem::take(&mut self.weather.release)
    }

    /// The sky tells which weather it shows, and whether it is a rainy one fully faded
    /// in.
    pub fn set_shown_weather(&mut self, weather: FormId, raining: bool) {
        if self.weather.shown.is_some_and(|w| w != weather) {
            self.weather.outgoing = self.weather.shown;
        }
        self.weather.shown = Some(weather);
        self.weather.raining = raining;
        // This legacy setter makes no fraction measurement. The reporting bridge
        // supplies one separately; never retain a measurement across this write.
        self.weather.measured_fraction = None;
        self.weather.measured_classification = None;
    }

    /// Record the actual sky producer's progress after reporting shown weather.
    /// Invalid or unbound measurements become unavailable, not a clamped value.
    pub fn set_shown_weather_fraction(&mut self, fraction: f32) {
        self.weather.measured_fraction = (self.weather.shown.is_some()
            && fraction.is_finite() && (0.0..=1.0).contains(&fraction)).then_some(fraction);
        self.weather.measured_classification = None;
    }

    /// Report one sky observation, without advancing or selecting weather.
    /// `classification` is the shown WTHR DATA byte; `settled` means no prior sky.
    pub fn set_shown_weather_sample(&mut self, fraction: f32, classification: u8, settled: bool) {
        self.set_shown_weather_fraction(fraction);
        self.weather.measured_classification = self.weather.measured_fraction
            .map(|_| (classification, settled));
    }
}

fn request(g: &mut Game, a: &Args, instant: bool) -> Result<Value, String> {
    let weather = a.form(0)?;
    if g.data.sigs.get(&weather).is_some_and(|s| s != b"WTHR") {
        return Err(format!("{} is not a weather", g.data.name(weather)));
    }
    let hold = a.num_or(1, 0.0) != 0.0;
    g.weather.request = Some(WeatherRequest { weather, instant, hold });
    if hold {
        g.weather.release = false;
    }
    none()
}

/// `SetWeather weather [override]`: the weather changes to it the normal way (its fade);
/// with the override flag the weather stays until ReleaseWeatherOverride, else the
/// weather system changes it again as usual [documented: GECK wiki SetWeather].
pub fn set_weather(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    request(g, a, false)
}

/// `ForceWeather weather [override]`: as SetWeather, at once, skipping the change
/// [documented: GECK wiki ForceWeather].
pub fn force_weather(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    request(g, a, true)
}

/// `ReleaseWeatherOverride`: the weather follows the weather system again after a
/// SetWeather or ForceWeather with the override flag [documented: GECK wiki
/// ReleaseWeatherOverride].
pub fn release_weather_override(g: &mut Game, _ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    g.weather.release = true;
    none()
}

/// `IsRaining`: 1 when the current weather is classified rainy and stable (its change
/// over), else 0 [documented: GECK wiki IsRaining: "cannot return a value different from
/// 0 and 1"]. The classification is WTHR DATA's Rainy flag (`fnv_world::sky::Weather`);
/// before the sky has told any weather (a headless run, an interior from the start) it
/// reads 0.
pub fn is_raining(g: &mut Game, _ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    yes(g.weather.raining)
}

/// The last measured transition fraction. The public command's range and alias
/// are documented by GECK GetCurrentWeatherPercent. When vanilla `NVDefaultWeather`
/// (`0x00064609`) is present, cold start begins settled at `1.0` [observed:
/// w4g_04_weather_cold_start]; synthetic load orders without `0x00064609` or
/// invalidated/restored sessions without a sky report return `0.0` [design].
pub fn get_current_weather_percent(g: &mut Game, _ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    Ok(Value::Num(f64::from(g.weather.measured_fraction.unwrap_or(0.0))))
}

/// `IsSnowing`: the documented binary settled-snow query. Returns `0` for cold-start
/// `NVDefaultWeather` (Pleasant) [observed: w4g_04_weather_cold_start] and `0` when
/// unmeasured [design].
pub fn is_snowing(g: &mut Game, _ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    yes(g.weather.measured_classification.is_some_and(|(class, settled)| class & 8 != 0 && settled))
}

fn classified_fraction(g: &Game, flag: u8) -> Result<Value, String> {
    let fraction = if g.weather.measured_classification.is_some_and(|(class, _)| class & flag != 0) {
        g.weather.measured_fraction.unwrap_or(0.0)
    } else { 0.0 };
    Ok(Value::Num(f64::from(fraction)))
}

/// Destination Pleasant classification, weighted by the existing sky fade.
/// [documented: GECK IsPleasant]; no outgoing-weather contribution is invented.
pub fn is_pleasant(g: &mut Game, _ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    classified_fraction(g, 1)
}

/// Destination Cloudy classification, weighted by the existing sky fade.
/// Contract qualification is recorded in docs/spec-notes/current-weather-percent.md.
pub fn is_cloudy(g: &mut Game, _ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    classified_fraction(g, 2)
}

/// `GetIsCurrentWeather weather`: 1 when that weather is the current one [documented:
/// GECK wiki GetIsCurrentWeather]: the one the sky shows, or, before the sky has taken
/// it, the one a script just set [hypothesis: the change is current from the call].
pub fn get_is_current_weather(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let w = a.form(0)?;
    let current = g.weather.request.map(|r| r.weather).or(g.weather.shown);
    yes(current == Some(w))
}

/// The host's first weather for an exterior place: (worldspace, position) -> weather.
pub type StartWeather = std::sync::Arc<dyn Fn(FormId, [f32; 3]) -> Option<FormId> + Send + Sync>;

static START: std::sync::Mutex<Option<StartWeather>> = std::sync::Mutex::new(None);

/// Set the first-weather hook for a run without the engine's sky (fnvtool).
pub fn set_start_weather(f: StartWeather) {
    if let Ok(mut g) = START.lock() {
        *g = Some(f);
    }
}

impl Game<'_> {
    /// Without the engine's sky, the weather of the exterior the player is now in, from
    /// the host's hook (the runner's go_to_cell) [hypothesis: the sky's first weather
    /// there]. Nothing when the sky tells the weather itself or no hook is set.
    pub fn settle_start_weather(&mut self) {
        let Some(hook) = START.lock().ok().and_then(|g| g.clone()) else { return };
        let Some(ws) = self.world.worldspace_of(FormId::PLAYER_REF) else { return };
        let Some(pos) = self.world.peek(FormId::PLAYER_REF).map(|p| p.pos) else { return };
        if let Some(w) = hook(ws, pos) {
            if self.weather.shown != Some(w) {
                let class = self
                    .data
                    .record_field(w, "DATA", "flags")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u8);
                self.set_shown_weather(w, class.is_some_and(|c| c & 4 != 0));
                if let Some(class) = class {
                    self.set_shown_weather_sample(1.0, class, true);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{Game, GameConfig};
    use crate::GameData;
    use fnv_esm::{LoadOrder, Plugin, Schema};
    use fnv_script::vm::XVal;
    use fnv_script::FunctionTable;
    use fnv_sim::{FormStore, RefTable, SimConfig, World};
    use serde_json::Value as Json;

    fn sub(sig: &[u8; 4], data: Vec<u8>) -> Vec<u8> {
        let mut out = Vec::with_capacity(6 + data.len());
        out.extend_from_slice(sig);
        out.extend_from_slice(&(data.len() as u16).to_le_bytes());
        out.extend_from_slice(&data);
        out
    }

    fn record(sig: &[u8; 4], id: u32, subs: &[Vec<u8>]) -> Vec<u8> {
        let body: Vec<u8> = subs.iter().flat_map(|s| s.iter().copied()).collect();
        let mut out = Vec::with_capacity(24 + body.len());
        out.extend_from_slice(sig);
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&body);
        out
    }

    fn top(sig: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(24 + body.len());
        out.extend_from_slice(b"GRUP");
        out.extend_from_slice(&((24 + body.len()) as u32).to_le_bytes());
        out.extend_from_slice(sig);
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(body);
        out
    }

    fn tes4() -> Vec<u8> {
        let mut hedr = Vec::new();
        hedr.extend_from_slice(&1.34f32.to_le_bytes());
        hedr.extend_from_slice(&0u32.to_le_bytes());
        hedr.extend_from_slice(&0x800u32.to_le_bytes());
        record(b"TES4", 0, &[sub(b"HEDR", hedr)])
    }

    fn wthr(id: u32, edid: &str, classification: u8) -> Vec<u8> {
        let mut data = vec![0u8; 15];
        data[3] = 255;
        data[11] = classification;
        let mut name = edid.as_bytes().to_vec();
        name.push(0);
        record(b"WTHR", id, &[sub(b"EDID", name), sub(b"DATA", data)])
    }

    fn build_game_data(with_nv_default: bool) -> (GameData, FormStore, RefTable) {
        let mut wthrs = wthr(0x0000_015E, "DefaultWeather", 1);
        if with_nv_default {
            wthrs.extend(wthr(NV_DEFAULT_WEATHER.0, "NVDefaultWeather", 1));
            wthrs.extend(wthr(0x0012_37D7, "NVGoodspringsWeather", 1));
        }
        let bytes = [tes4(), top(b"WTHR", &wthrs)].concat();
        let plugins = [Plugin::parse(&bytes, "ColdWeather.esm".into()).unwrap()];
        let lo = LoadOrder::new(&plugins).unwrap();
        let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
        let json: Json = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let table = FunctionTable::load(&path).unwrap();
        let forms = FormStore::build(&lo, &schema);
        let refs = RefTable::build(&lo, &schema);
        let data = GameData::build(&lo, &schema, table, &json, &refs);
        (data, forms, refs)
    }

    fn query(g: &mut Game, expr: &str) -> XVal {
        g.run_query(&format!("Begin Function {{}}\nSetFunctionValue ({expr})\nEnd\n"), Default::default()).unwrap()
    }

    #[test]
    fn cold_start_resolves_nv_default_weather_when_present_in_load_order() {
        let (data, forms, refs) = build_game_data(true);
        let world = World::new(&forms, &refs, SimConfig::default());
        let mut g = Game::new(&data, world, GameConfig { strict: true, ..Default::default() });
        assert_eq!(query(&mut g, "GetCurrentWeather"), XVal::Ref(NV_DEFAULT_WEATHER.0));
        assert_eq!(query(&mut g, "GetCurrentWeatherPercent"), XVal::Num(1.0));
        assert_eq!(query(&mut g, "IsPleasant"), XVal::Num(1.0));
        assert_eq!(query(&mut g, "IsSnowing"), XVal::Num(0.0));
        assert_eq!(query(&mut g, "IsRaining"), XVal::Num(0.0));
        assert_eq!(query(&mut g, "IsCloudy"), XVal::Num(0.0));
        assert_eq!(query(&mut g, "GetIsCurrentWeather NVDefaultWeather"), XVal::Num(1.0));
        assert_eq!(query(&mut g, "GetIsCurrentWeather DefaultWeather"), XVal::Num(0.0));
        // Entering an exterior replaces NVDefaultWeather with the exterior weather.
        g.set_shown_weather(FormId(0x0012_37D7), false);
        g.set_shown_weather_sample(1.0, 1, true);
        assert_eq!(query(&mut g, "GetCurrentWeather"), XVal::Ref(0x0012_37D7));
        assert_eq!(query(&mut g, "GetIsCurrentWeather NVGoodspringsWeather"), XVal::Num(1.0));
        assert_eq!(query(&mut g, "GetIsCurrentWeather NVDefaultWeather"), XVal::Num(0.0));
        assert_eq!(g.weather.outgoing, Some(NV_DEFAULT_WEATHER));
        assert_eq!(query(&mut g, "GetCurrentWeatherPercent"), XVal::Num(1.0));
        assert_eq!(query(&mut g, "IsPleasant"), XVal::Num(1.0));
    }

    #[test]
    fn synthetic_load_order_without_nv_default_weather_stays_unmeasured() {
        let (data, forms, refs) = build_game_data(false);
        let world = World::new(&forms, &refs, SimConfig::default());
        let mut g = Game::new(&data, world, GameConfig { strict: true, ..Default::default() });
        assert_eq!(query(&mut g, "GetCurrentWeather"), XVal::Ref(0));
        assert_eq!(query(&mut g, "GetCurrentWeatherPercent"), XVal::Num(0.0));
        assert_eq!(query(&mut g, "IsPleasant"), XVal::Num(0.0));
        assert_eq!(query(&mut g, "IsSnowing"), XVal::Num(0.0));
        assert_eq!(query(&mut g, "IsRaining"), XVal::Num(0.0));
        assert_eq!(query(&mut g, "IsCloudy"), XVal::Num(0.0));
        assert_eq!(query(&mut g, "GetIsCurrentWeather DefaultWeather"), XVal::Num(0.0));
    }
}
