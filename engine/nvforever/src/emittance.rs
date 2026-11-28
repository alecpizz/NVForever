//! External emittance (docs/spec-notes/world.md §8): references can name an emittance
//! source (REFR XEMI): a region whose weather's sunlight colour at the current hour (the
//! weather on screen when the region lists it, else its likeliest one), or a
//! light's colour times its fade (the tutorials: emittance carries the light's colour and
//! brightness, level-design.md §2), drives them [hypothesis]. It scales:
//! - shapes whose shader property sets BSShaderFlags bit 29 "External_Emittance": the
//!   surface colour of unlit shapes (light beams and rays), the emitted light of lit ones
//!   (glowing windows);
//! - the colour of LIGH references.
//!
//! The Prospector Saloon shows why: its porch lamp beam and the two lights above the porch
//! name InvertedDaylight regions, whose weathers have black sunlight by day, and the beam
//! is invisible by day in game; its window light rays name NVInteriorRegion, whose
//! weather has warm sunlight by day and black at night, and in game they are warm yellow.
//!
//! Each (material, source) pair gets its own material copy, recoloured when the hour
//! moves; references without XEMI keep the shared material.

use bevy::prelude::*;
use fnv_world::lighting::{emittance, Emittance, RegionEmittance};
use fnv_world::sky::{srgb_to_linear, weather, ColorType, Weather};
use nvf_assets::NifMaterial;
use std::collections::HashMap;

use crate::sky::SkyState;
use crate::world::WorldData;

pub struct EmittancePlugin;

impl Plugin for EmittancePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EmittanceState>()
            .add_systems(Update, (script_drawn_emittance, update_emittance.after(crate::sky::update_sky).after(crate::lights::sync_script_lights)).chain())
            .add_systems(Last, release_copies);
    }
}

/// A material copy driven by an emittance source, with the colours it started from.
struct Driven {
    copy: Handle<NifMaterial>,
    source: u32,
    base_color: LinearRgba,
    emissive: LinearRgba,
    unlit: bool,
}

#[derive(Resource, Default)]
pub struct EmittanceState {
    copies: HashMap<(AssetId<NifMaterial>, u32), Handle<NifMaterial>>,
    driven: Vec<Driven>,
    /// Resolved XEMI targets; `None` remembers one that is neither a region with a
    /// weather nor a light.
    sources: HashMap<u32, Option<Source>>,
    /// Hour of the last update; `None` forces the next one.
    hour: Option<f32>,
    /// The weather on screen (and the fade's progress) at the last update.
    shown: Option<(u32, Option<u32>, f32)>,
    /// Canonical light record revision used by cached light-backed sources.
    light_revision: Option<u64>,
    /// Peak linear luminance (0..1) of active weather-backed (`Source::Weather`) emittance
    /// sources in the current scene (`0.0` at night or when no weather XEMI is active).
    pub weather_daylight_luma: f32,
}

#[cfg(any(test, debug_assertions, feature = "dev-aids"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RoomRayEmittance {
    Daylight,
    OtherRegion,
    Light,
}

enum Source {
    /// A region's weathers by form ID (those that load), and which one shows.
    Weather(RegionEmittance, HashMap<u32, Weather>),
    /// A light's colour × fade, linear (for materials) and as display values (for lights).
    Color(LinearRgba, LinearRgba),
}

/// A light whose colour (display values, lights.rs `display_color`) an emittance source
/// scales.
#[derive(Component)]
pub struct LightEmittance {
    pub source: u32,
    pub color: LinearRgba,
}

impl EmittanceState {
    /// The material copies the light emittance writes (the profile aid's `profile change
    /// sources:` line).
    pub fn copies(&self) -> impl Iterator<Item = AssetId<NifMaterial>> + '_ {
        self.driven.iter().map(|d| d.copy.id())
    }

    fn source(&mut self, world: &WorldData, xemi: u32) -> bool {
        if !self.sources.contains_key(&xemi) { self.light_revision = None; }
        self.sources
            .entry(xemi)
            .or_insert_with(|| match emittance(world.lo, world.schema, xemi)? {
                Emittance::Weather(r) => {
                    let ws: HashMap<u32, Weather> =
                        r.listed.iter().filter_map(|&f| Some((f, weather(world.lo, world.schema, f)?))).collect();
                    ws.contains_key(&r.best).then_some(Source::Weather(r, ws))
                }
                Emittance::Light { color: c, fade } => {
                    let f = fade.max(0.0);
                    let shown = c.map(|b| b as f32 / 255.0 * f);
                    Some(Source::Color(LinearRgba::rgb(srgb_to_linear(c[0]) * f, srgb_to_linear(c[1]) * f, srgb_to_linear(c[2]) * f), LinearRgba::rgb(shown[0], shown[1], shown[2])))
                }
            })
            .is_some()
    }

    pub(crate) fn refresh_session_lights(&mut self, world: &WorldData, game: &fnv_game::Game<'_>) {
        let revision = game.gaps.jip4.light_revision;
        if self.light_revision == Some(revision) { return; }
        self.light_revision = Some(revision);
        for (&form, source) in &mut self.sources {
            if !matches!(source, Some(Source::Color(_, _))) { continue; }
            let Some(original) = fnv_world::lighting::light_data(world.lo, world.schema, form) else { continue };
            let light = game.session_light_data(fnv_sim::FormId(form), original);
            let fade = light.fade.max(0.0);
            let shown = light.color.map(|b| b as f32 / 255.0 * fade);
            *source = Some(Source::Color(
                LinearRgba::rgb(srgb_to_linear(light.color[0]) * fade, srgb_to_linear(light.color[1]) * fade, srgb_to_linear(light.color[2]) * fade),
                LinearRgba::rgb(shown[0], shown[1], shown[2]),
            ));
        }
        self.hour = None;
    }

    /// The material for a shape with external emittance on a reference naming `xemi`:
    /// a copy recoloured by the source, or `None` to keep the shared one.
    pub fn material(&mut self, world: &WorldData, materials: &mut Assets<NifMaterial>, shared: &Handle<NifMaterial>, xemi: u32) -> Option<Handle<NifMaterial>> {
        if !self.source(world, xemi) {
            return None;
        }
        if let Some(h) = self.copies.get(&(shared.id(), xemi)) {
            return Some(h.clone());
        }
        let m = materials.get(shared)?.clone();
        let (base_color, emissive, unlit) = (m.base.base_color.to_linear(), m.base.emissive, m.base.unlit);
        let copy = materials.add(m);
        self.copies.insert((shared.id(), xemi), copy.clone());
        self.driven.push(Driven { copy: copy.clone(), source: xemi, base_color, emissive, unlit });
        self.hour = None;
        Some(copy)
    }

    /// The copies held, for the stress census: each is held twice here (`copies`, `driven`).
    pub fn share(&self, materials: &Assets<NifMaterial>) -> nvf_assets::release::CacheShare {
        let mut s = nvf_assets::release::CacheShare::default();
        for h in self.copies.values() {
            s.material(materials, h, 2);
        }
        s
    }

    /// Classify a placed ray's cached XEMI for the room beam census [design, #4698].
    /// The material path resolves this source before attaching the census mark.
    #[cfg(any(test, debug_assertions, feature = "dev-aids"))]
    pub(crate) fn room_ray_emittance(&self, sky: &SkyState, xemi: u32) -> Option<RoomRayEmittance> {
        match self.sources.get(&xemi)?.as_ref()? {
            Source::Weather(region, weathers) => {
                let weather = weathers.get(&region.best)?;
                let sunlight = |hour| weather.color(&sky.climate, ColorType::Sunlight, hour).iter().sum::<f32>();
                Some(if sunlight(12.0) > sunlight(0.0) {
                    RoomRayEmittance::Daylight
                } else {
                    RoomRayEmittance::OtherRegion
                })
            }
            Source::Color(..) => Some(RoomRayEmittance::Light),
        }
    }

    /// Mark a light as driven by `xemi`, if that resolves.
    pub fn light(&mut self, world: &WorldData, xemi: u32, color: LinearRgba) -> Option<LightEmittance> {
        self.hour = None;
        self.source(world, xemi).then_some(LightEmittance { source: xemi, color })
    }
}

/// Every [`SWEEP_SECS`](nvf_assets::release::SWEEP_SECS), drop the copies no entity holds
/// (each is held twice here, in `copies` and `driven`): their references unloaded, or
/// their model was let go of (its material's id is never asked for again). Without this
/// every copy, its textures and its bindless slot stayed for the session, and each was
/// recoloured every game minute (issue #904). A reference drawn again gets a fresh copy.
fn release_copies(time: Res<Time<Real>>, mut state: ResMut<EmittanceState>, mut since: Local<f32>) {
    *since += time.delta_secs();
    if *since < nvf_assets::release::SWEEP_SECS {
        return;
    }
    *since = 0.0;
    let s = &mut *state;
    if nvf_assets::release::retain_held(&mut s.copies, 2) > 0 {
        let kept: std::collections::HashSet<AssetId<NifMaterial>> = s.copies.values().map(Handle::id).collect();
        s.driven.retain(|d| kept.contains(&d.copy.id()));
    }
}

/// Shapes drawn by scripts (`nvf_play::script_draw`) whose colour an emittance source
/// drives get their driven material copy, as streamed references do (world.rs).
fn script_drawn_emittance(
    mut commands: Commands,
    world: Option<Res<WorldData>>,
    mut state: ResMut<EmittanceState>,
    mut materials: ResMut<Assets<NifMaterial>>,
    mut shapes: Query<(Entity, &mut MeshMaterial3d<NifMaterial>, &nvf_play::script_draw::WantsEmittance)>,
    mut logged: Local<std::collections::HashSet<u32>>,
) {
    let Some(world) = world else { return };
    for (e, mut m, w) in &mut shapes {
        if let Some(copy) = state.material(&world, &mut materials, &m.0, w.0) {
            m.0 = copy;
            if logged.insert(w.0) {
                info!("emittance: shapes drawn by scripts take source {:#010x}", w.0);
            }
        }
        commands.entity(e).remove::<nvf_play::script_draw::WantsEmittance>();
    }
}

/// The emittance colour of a source at the sky's hour: linear, and as display values for
/// lights (a light's colour × fade, as the display-value lighting sum takes a light's,
/// world.md §8) [hypothesis]. A region's colour is its shown weather's sunlight; while
/// the sky fades between weathers it fades from the outgoing weather's pick to the
/// incoming one's by the same progress [hypothesis, as the sky's own colours fade].
fn color(source: &Source, sky: &SkyState) -> (LinearRgba, LinearRgba) {
    match source {
        Source::Weather(r, ws) => {
            let sun = |current: Option<u32>| {
                let w = ws.get(&r.shown(current)).or_else(|| ws.get(&r.best));
                w.map_or([0.0; 3], |w| w.color(&sky.climate, ColorType::Sunlight, sky.hour))
            };
            let now = sun(Some(sky.weather.form_id));
            let c = match &sky.prev {
                Some(p) => {
                    let before = sun(Some(p.form_id));
                    let t = sky.fade.clamp(0.0, 1.0);
                    [0, 1, 2].map(|i| before[i] + (now[i] - before[i]) * t)
                }
                None => now,
            };
            let d = c.map(crate::lights::linear_to_display);
            (LinearRgba::rgb(c[0], c[1], c[2]), LinearRgba::rgb(d[0], d[1], d[2]))
        }
        Source::Color(c, d) => (*c, *d),
    }
}

fn scale(a: LinearRgba, b: LinearRgba) -> LinearRgba {
    LinearRgba::new(a.red * b.red, a.green * b.green, a.blue * b.blue, a.alpha)
}

fn linear_luma(c: LinearRgba) -> f32 {
    (0.2126 * c.red + 0.7152 * c.green + 0.0722 * c.blue).clamp(0.0, 1.0)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn update_emittance(
    sky: Res<SkyState>,
    world: Option<Res<WorldData>>,
    script: Option<Res<nvf_play::script::ScriptState>>,
    mut state: ResMut<EmittanceState>,
    mut materials: ResMut<Assets<NifMaterial>>,
    mut lights: Query<(&LightEmittance, Option<&mut PointLight>, Option<&mut SpotLight>)>,
    changed: Query<(), Changed<LightEmittance>>,
    mut removed_lights: RemovedComponents<LightEmittance>,
) {
    if let (Some(world), Some(game)) = (world, script.as_ref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game)) {
        state.refresh_session_lights(&world, game);
    }
    let moved = state.hour.is_none_or(|h| (h - sky.hour).abs() > 1.0 / 60.0);
    let shown = (sky.weather.form_id, sky.prev.as_ref().map(|p| p.form_id), if sky.prev.is_some() { sky.fade } else { 1.0 });
    let weather_moved = state.shown.is_none_or(|(w, p, f)| w != shown.0 || p != shown.1 || (f - shown.2).abs() > 0.01);
    let removed = !removed_lights.is_empty();
    removed_lights.clear();
    if !moved && !weather_moved && changed.is_empty() && !removed {
        return;
    }
    state.hour = Some(sky.hour);
    state.shown = Some(shown);
    let colors: HashMap<u32, (LinearRgba, LinearRgba)> =
        state.sources.iter().filter_map(|(&k, s)| Some((k, color(s.as_ref()?, &sky)))).collect();
    for d in &state.driven {
        let Some(&(e, _)) = colors.get(&d.source) else { continue };
        // Only a colour that changes is written: taking the material mutably marks it
        // changed, and its bindless slab is sent to the GPU again whatever was written.
        let Some(m) = materials.get(&d.copy) else { continue };
        if d.unlit {
            let c = Color::LinearRgba(scale(d.base_color, e));
            if m.base.base_color != c {
                if let Some(mut m) = materials.get_mut(&d.copy) {
                    m.base.base_color = c;
                }
            }
        } else {
            let c = scale(d.emissive, e);
            if m.base.emissive != c {
                if let Some(mut m) = materials.get_mut(&d.copy) {
                    m.base.emissive = c;
                }
            }
        }
    }
    let mut weather_luma = 0.0f32;
    for (l, point, spot) in &mut lights {
        let Some(&(lin_e, disp_e)) = colors.get(&l.source) else { continue };
        if matches!(state.sources.get(&l.source), Some(Some(Source::Weather(..)))) {
            weather_luma = weather_luma.max(linear_luma(lin_e));
        }
        let c = Color::LinearRgba(scale(l.color, disp_e));
        if let Some(mut p) = point {
            p.color = c;
        }
        if let Some(mut s) = spot {
            s.color = c;
        }
    }
    state.weather_daylight_luma = weather_luma;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_per_channel() {
        let s = scale(LinearRgba::new(1.0, 0.5, 0.25, 0.3), LinearRgba::rgb(0.5, 1.0, 0.0));
        assert_eq!((s.red, s.green, s.blue, s.alpha), (0.5, 0.5, 0.0, 0.3));
    }

    #[test]
    fn weather_daylight_luma_follows_time_and_presence() {
        assert_eq!(linear_luma(LinearRgba::BLACK), 0.0);
        assert!(linear_luma(LinearRgba::rgb(0.9, 0.8, 0.5)) > 0.7);

        let mut w = Weather { form_id: 0x15e, colors: vec![vec![[0; 3]; 4]; 10], ..Default::default() };
        // 4 times: sunrise, day, sunset, night; ColorType::Sunlight == 2.
        w.colors[ColorType::Sunlight as usize] = vec![
            [200, 160, 100],
            [255, 227, 170],
            [200, 140, 90],
            [0, 0, 0],
        ];
        let mut ws = HashMap::new();
        ws.insert(w.form_id, w);
        let region = RegionEmittance { listed: vec![0x15e], best: 0x15e };

        let mut world = World::new();
        let mut sky = SkyState::default();
        sky.climate.sunrise = (5.0, 7.0);
        sky.climate.sunset = (18.0, 20.0);
        sky.weather.form_id = 0x15e;
        sky.hour = 12.0;

        let mut state = EmittanceState::default();
        state.sources.insert(1, Some(Source::Weather(region, ws)));
        world.insert_resource(sky);
        world.insert_resource(state);
        world.insert_resource(Assets::<NifMaterial>::default());

        let light_e = world
            .spawn((LightEmittance { source: 1, color: LinearRgba::WHITE }, PointLight::default()))
            .id();

        let mut schedule = Schedule::default();
        schedule.add_systems(update_emittance);

        // Noon: weather source lights up and reports positive daylight luminance.
        schedule.run(&mut world);
        assert!(world.resource::<EmittanceState>().weather_daylight_luma > 0.5);

        // Midnight: weather source turns black and daylight luminance drops to 0.
        world.resource_mut::<SkyState>().hour = 0.0;
        schedule.run(&mut world);
        assert_eq!(world.resource::<EmittanceState>().weather_daylight_luma, 0.0);

        // Back to noon, then despawn the cell's light: removal triggers reset to 0.
        world.resource_mut::<SkyState>().hour = 12.0;
        schedule.run(&mut world);
        assert!(world.resource::<EmittanceState>().weather_daylight_luma > 0.5);
        world.despawn(light_e);
        schedule.run(&mut world);
        assert_eq!(world.resource::<EmittanceState>().weather_daylight_luma, 0.0);
    }
}
