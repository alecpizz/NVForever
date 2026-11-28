//! The volumetric medium's light in the air (LootJunkieAzza/NVForever#3032, stage 3): the main world's side of the light. An engineering choice of NVForever's
//! own for the enhanced look, never a fact about the original game and never used to explain
//! a difference from it [design].
//!
//! Each frame while the light is wanted, in `PostUpdate` after medium_air_grid.rs `follow_air`
//! and its commands (so after the camera, underwater.rs's fog and the transforms, and before
//! the solid cores and the visibility check), [`follow_light`] finishes the main camera's
//! `AirCamera`, which the frame's grid and composite are drawn by:
//! - **the tune file** (`NVF_PLAY_DEBUG_MEDIUM_LIGHT_TUNE`; not in release builds, dev_aids.rs): looked
//!   at every 0.5 s of real time (medium_air.rs `poll_due`), each text parsed over the start
//!   tune (`TuneFollower`), its changes logged; the grid follows its `DEPTH` and `GRID`
//!   (medium_air_grid.rs `prepare_air` makes the volume again when the grid changes);
//! - **the sunlit share s**: medium_air.rs `scaled_share` from the `Sun` light's
//!   displayed colour and `illuminance / (PI U)` and the `GlobalAmbientLight`'s colour and
//!   `brightness / U`, as sky.rs `update_sky` set them this frame (medium_air.rs module doc,
//!   "What scaled_share's caller passes");
//! - **the gate** (medium_air.rs `gate`): outdoors, above water, a sun light, a usable fog
//!   blended in display values, a source (the sun, or the moon where the climate has one, medium_air.rs
//!   `air_source`), a strength above 0, an effective share above 0. Day is
//!   the elevation that aimed the light above 0 (medium_air.rs `day_by_aim`): sky.rs
//!   `update_sky` aims the `Sun` entity from `sun_direction(climate, sun_light_hour())`, and
//!   `SkyState::sun_elevation_at(sun_light_hour())` is that direction's elevation (both public
//!   already: nothing added to sky.rs). No comparison with the stepped hour's sun (a modded climate with a day of a thousandth of an hour fooled it);
//!   without a sky state the gate is closed as indoors before the night test. One line per
//!   change: `volumetric medium: light in the air on: ...` at once (again when a tune change
//!   alters its figures, medium_air.rs `tune_changed`, and when its bounds moved by more than
//!   0.05, looked at every 5 s of real time, medium_air.rs `bounds_moved`), `... off: <reason>`
//!   once the reason held 0.5 s of real time (medium_air.rs `GateLog`: at start the fog shader
//!   is patched a moment after the first frames).
//!
//! - **the start strength** (the overall strength): the aid's value
//!   when the aid asks for the light, else the settings row's Volumetric light strength
//!   (medium_air_demand.rs `AirStrengthSetting`, 0 to 200 %); a move of the slider makes new
//!   start values at once (medium_air.rs `TuneFollower::restart`; a tune file's keys still win; in a
//!   build without the developer aids the slider alone, `slider_tune`).
//! - Sun shafts yield in the render world, per view, in a frame the light's composite drew
//!   there (medium_air_compose.rs `AirComposedViews`, shafts.rs `shafts_skip`), not from
//!   this main-world gate.
//!
//! - **what is drawn for the light alone** (`AirDraws`, solid_cores.rs) follows the gate as its
//!   lines have it, from the "on" line to the "off" line, so it is not made anew for a reason
//!   that comes and goes; in the frames between a closed gate and its line it is hidden
//!   (`AirOpen`: this frame's answer, the composite's own).
//!
//! With the light not wanted nothing runs but the check of the demand, and nothing is logged.

use crate::fly::FlyCam;
use crate::medium_air::{self, AirGateInput, AirGrid, AirPhase, AirSource, AirTune, GateLog, NormedPhase, ViewBounds};
#[cfg(any(test, debug_assertions, feature = "dev-aids"))]
use crate::medium_air::TuneFollower;
use crate::medium_air_demand::{AirDraws, AirOpen, AirStrengthSetting, AirWanted};
use crate::medium_air_grid::AirCamera;
use bevy::prelude::*;
use std::f32::consts::PI;
#[cfg(any(test, debug_assertions, feature = "dev-aids"))]
use std::sync::LazyLock;

/// The start tune (the start aids, as medium_air_grid.rs reads them, and the grid aid), the
/// strength aid's value when it asks for the light, and the tune file's path, read once.
#[cfg(any(test, debug_assertions, feature = "dev-aids"))]
struct LightAids {
    start: AirTune,
    /// `NVF_PLAY_DEBUG_MEDIUM_LIGHT`'s strength when the aid asks for the light
    /// (medium_air_demand.rs `wanted_from_aid`): it wins over the settings row's slider.
    aid_strength: Option<f32>,
    path: Option<String>,
}

#[cfg(any(test, debug_assertions, feature = "dev-aids"))]
static AIDS: LazyLock<LightAids> = LazyLock::new(|| {
    let var = crate::dev_aids::var;
    let start = AirTune {
        grid: medium_air::debug_grid(var(medium_air::ENV_GRID).as_deref()),
        ..AirTune::start(
            medium_air::debug_strength(var(medium_air::ENV_STRENGTH).as_deref()),
            medium_air::debug_view(var(medium_air::ENV_VIEW).as_deref()),
            medium_air::debug_occluders(var(medium_air::ENV_OCCLUDERS).as_deref()),
            medium_air::debug_depth(var(medium_air::ENV_DEPTH).as_deref()),
        )
    };
    let path = var(medium_air::ENV_TUNE).filter(|p| !p.trim().is_empty());
    let aid = var(medium_air::ENV_STRENGTH);
    let aid_strength = crate::medium_air_demand::wanted_from_aid(aid.as_deref()).then(|| medium_air::debug_strength(aid.as_deref()));
    LightAids { start, aid_strength, path }
});

/// The start strength: the aid's value when the aid asks for the light, else the settings
/// row's slider [design].
#[cfg(any(test, debug_assertions, feature = "dev-aids"))]
pub fn start_strength(aid: Option<f32>, row: f32) -> f32 {
    aid.unwrap_or(row)
}

pub struct AirLightPlugin;

impl Plugin for AirLightPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, follow_light.after(crate::medium_air_grid::follow_air).before(crate::solid_cores::CoreSystems));
    }
}

/// One activation's state (dropped when the light is no longer wanted, so the next one
/// logs again).
#[derive(Default)]
struct LightState {
    active: bool,
    #[cfg(any(test, debug_assertions, feature = "dev-aids"))]
    follower: Option<TuneFollower>,
    /// The strength last drawn with (a build without the developer aids follows the slider alone).
    #[cfg(not(any(test, debug_assertions, feature = "dev-aids")))]
    strength: Option<f32>,
    /// When the tune file was last looked at (real seconds).
    #[cfg(any(test, debug_assertions, feature = "dev-aids"))]
    looked: Option<f64>,
    /// The gate's log (an off reason only once it held 0.5 s).
    log: GateLog,
    /// What the last "on" line was written for (the line again when a tune change alters its
    /// figures or its bounds moved); none while the light is off.
    told: Option<Told>,
    /// The phase last solved for the log ([`solved`]).
    phase: Option<NormedPhase>,
}

/// The solved phase for `phase`: the cached one while its four inputs are the same, else
/// solved again (medium_air.rs `AirPhase::normed`, 8,192 steps and a bisection) and kept
/// [design].
fn solved(cache: &mut Option<NormedPhase>, phase: AirPhase) -> NormedPhase {
    match *cache {
        Some(n) if n.phase == phase => n,
        _ => *cache.insert(phase.normed()),
    }
}

/// The last "on" line's tune and bounds, and when its bounds were last looked at (real
/// seconds).
#[derive(Clone, Copy)]
struct Told {
    tune: AirTune,
    bounds: ViewBounds,
    looked: f64,
}

/// Whether the bounds are due a look: [`medium_air::BOUND_RELOG_SECONDS`] of real time since
/// the last (a clock that went back counts as due) [design].
pub fn bounds_due(looked: f64, now: f64) -> bool {
    now < looked || now - looked >= medium_air::BOUND_RELOG_SECONDS
}

/// The scaled share s from the lights as sky.rs set them this frame (medium_air.rs module
/// doc for the names): 0 without a sun light.
fn light_share(sun: Option<&DirectionalLight>, ambient: Option<&GlobalAmbientLight>) -> f32 {
    let Some(sun) = sun else { return 0.0 };
    let unit = crate::sky::light_unit();
    let rgb = |c: Color| {
        let l = c.to_linear();
        Vec3::new(l.red, l.green, l.blue)
    };
    let (ambient_luma, ambient_scale) = ambient.map_or((0.0, 0.0), |a| (medium_air::display_luma(rgb(a.color)), a.brightness / unit));
    medium_air::scaled_share(medium_air::display_luma(rgb(sun.color)), sun.illuminance / (PI * unit), ambient_luma, ambient_scale)
}

/// What [`follow_light`] hands on of its gate: this frame's answer and the settled one.
type Gates<'w> = (Option<ResMut<'w, AirOpen>>, Option<ResMut<'w, AirDraws>>);

/// The frame's answer `open` and the settled one `draws` into the resources (where they are).
fn hand_on((open_now, draws_now): Gates, open: bool, draws: bool) {
    if let Some(mut o) = open_now {
        o.set_if_neq(AirOpen(open));
    }
    if let Some(mut d) = draws_now {
        d.set_if_neq(AirDraws(draws));
    }
}

/// The tune this frame: the start values with the strength of the aid or of the row's slider
/// (`row`), made again when the slider moves, and the tune file, when given, parsed over them; its
/// changes logged.
#[cfg(any(test, debug_assertions, feature = "dev-aids"))]
fn followed_tune(state: &mut LightState, row: f32, now: f64) -> AirTune {
    let aids = &*AIDS;
    let start = aids.start.with_strength(start_strength(aids.aid_strength, row));
    let follower = state.follower.get_or_insert_with(|| TuneFollower::new(start));
    for line in follower.restart(start) {
        info!("{line}");
    }
    // The tune file, at most every TUNE_POLL_SECONDS of real time.
    if let Some(path) = aids.path.as_deref().filter(|_| medium_air::poll_due(state.looked, now)) {
        let read = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"));
        for line in follower.feed(read.as_deref().map_err(String::as_str)) {
            info!("{line}");
        }
        state.looked = Some(now);
    }
    follower.tune()
}

/// The tune without the developer aids: the look's constants with the slider's strength (`row`),
/// a move of it logged (the follower's words).
#[cfg(not(any(test, debug_assertions, feature = "dev-aids")))]
fn slider_tune(state: &mut LightState, row: f32) -> AirTune {
    let tune = AirTune::default().with_strength(row);
    if state.strength.replace(tune.strength).is_some_and(|was| was != tune.strength) {
        info!("volumetric medium: light in the air tune: STRENGTH={}", tune.strength);
    }
    tune
}

/// Whether what the "on" line states of the tune changed: without the developer aids only the
/// strength can.
fn tune_moved(was: &AirTune, now: &AirTune) -> bool {
    if crate::dev_aids::ON { medium_air::tune_changed(was, now) } else { was.strength != now.strength }
}

/// Each frame: nothing while the light is not wanted (the state forgotten once); else the
/// tune file, the share, the gate and the grid onto the main camera's `AirCamera`.
#[allow(clippy::type_complexity)]
fn follow_light(
    (wanted, strength_row, gates): (Option<Res<AirWanted>>, Option<Res<AirStrengthSetting>>, Gates),
    time: Res<Time<Real>>,
    (sky, submerged, ambient): (Option<Res<crate::sky::SkyState>>, Option<Res<crate::underwater::CameraSubmerged>>, Option<Res<GlobalAmbientLight>>),
    suns: Query<&DirectionalLight, With<crate::sky::Sun>>,
    mut cams: Query<(&mut AirCamera, &Projection), With<FlyCam>>,
    mut state: Local<LightState>,
) {
    if !wanted.is_some_and(|w| w.0) {
        if state.active {
            *state = LightState::default();
        }
        hand_on(gates, false, false);
        return;
    }
    // No camera: nothing is composed.
    let Some((mut cam, projection)) = cams.iter_mut().next() else {
        return hand_on(gates, false, state.told.is_some());
    };
    let state = &mut *state;
    state.active = true;
    let row = strength_row.map_or(crate::medium_air_demand::STRENGTH_DEFAULT, |s| s.0);
    let now = time.elapsed_secs_f64();
    #[cfg(any(test, debug_assertions, feature = "dev-aids"))]
    let tune = followed_tune(state, row, now);
    #[cfg(not(any(test, debug_assertions, feature = "dev-aids")))]
    let tune = slider_tune(state, row);
    // The sun, the moon or nothing, by the elevation that aimed the light (the module doc), and the
    // constants the light is drawn with under it.
    let source = sky.as_ref().map_or(AirSource::None, |sky| medium_air::air_source(sky.sun_elevation_at(sky.sun_light_hour()), sky.climate.has_moon()));
    let tune = tune.lit_by(source);
    let aspect = match projection {
        Projection::Perspective(p) => p.aspect_ratio,
        _ => 16.0 / 9.0,
    };
    let grid = AirGrid::for_view(aspect, tune.grid, tune.depth);
    // The view's longest ray for the logged bound (the projection's scales, as the grid pass
    // takes them from the extracted view).
    let clip = projection.get_clip_from_view();
    let corner = medium_air::corner_ray_len(clip.x_axis.x, clip.y_axis.y);
    let sun = suns.iter().next();
    let s = light_share(sun, ambient.as_deref());
    let input = AirGateInput {
        interior: sky.as_ref().is_none_or(|s| s.interior.is_some()),
        submerged: submerged.is_some_and(|s| s.0),
        sun: sun.is_some(),
        fog: cam.fog,
        display_blend: cam.display_blend,
        source,
        strength: tune.strength,
        share: medium_air::effective_share(s, tune.share),
    };
    let answer = medium_air::gate(&input);
    // The line and the bounds it states, from the phase solved once per change of its inputs
    // (`solved`: an 8,192-step solve).
    let fog = cam.fog;
    let bounds_of = |phase: &NormedPhase| medium_air::on_bounds(&grid, phase, s, &tune, fog, corner);
    let tell = |phase: &NormedPhase, bounds: ViewBounds| {
        info!("{}", medium_air::on_line_with(&grid, phase, s, &tune, source, &bounds));
        Some(Told { tune, bounds, looked: now })
    };
    match state.log.step(answer, now) {
        Some(Ok(_)) => {
            let phase = solved(&mut state.phase, tune.phase());
            state.told = tell(&phase, bounds_of(&phase));
        }
        Some(Err(off)) => {
            info!("volumetric medium: light in the air off: {}", off.reason());
            state.told = None;
        }
        // The light still on: the line again when the tune changed what it states, or when its
        // bounds moved (a weather fade, the hour, the view), looked at every 5 s at most.
        None if answer.is_ok() => {
            if let Some(told) = state.told {
                if tune_moved(&told.tune, &tune) {
                    let phase = solved(&mut state.phase, tune.phase());
                    state.told = tell(&phase, bounds_of(&phase));
                } else if bounds_due(told.looked, now) {
                    let phase = solved(&mut state.phase, tune.phase());
                    let bounds = bounds_of(&phase);
                    state.told = if medium_air::bounds_moved(&told.bounds, &bounds) { tell(&phase, bounds) } else { Some(Told { looked: now, ..told }) };
                }
            }
        }
        None => {}
    }
    let next = AirCamera { grid, tune, share: s, draws: answer.is_ok(), ..*cam };
    if *cam != next {
        *cam = next;
    }
    // This frame's answer, and as the gate's lines have it: open at once, closed once a reason held.
    hand_on(gates, answer.is_ok(), state.told.is_some());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The phase is solved once per change of its four inputs and reused while they stay.
    #[test]
    fn the_phase_is_solved_once_per_change() {
        let mut cache = None;
        let p = AirTune::default().phase();
        let first = solved(&mut cache, p);
        assert_eq!(first, p.normed());
        // A planted value proves the second call reads the cache instead of solving.
        let planted = NormedPhase { norm: 123.0, ..first };
        cache = Some(planted);
        assert_eq!(solved(&mut cache, p), planted, "same inputs: the cached solve");
        let other = AirPhase { cap: 3.0, ..p };
        assert_eq!(solved(&mut cache, other), other.normed(), "a changed input: solved again");
        assert_eq!(cache.map(|n| n.phase), Some(other));
    }

    /// The bounds are looked at every 5 s of real time (a clock that went back: at once).
    #[test]
    fn the_bounds_are_looked_at_every_five_seconds() {
        assert!(!bounds_due(10.0, 14.9));
        assert!(bounds_due(10.0, 15.0));
        assert!(bounds_due(10.0, 3.0));
    }

    /// The night gate's clock: a degenerate
    /// climate, sunrise begin equal to sunset end (a day of a thousandth of an hour, as
    /// fnv-world's `sun_direction` allows), the sky stepped at 6:00 and the light aimed at
    /// 6:0009, 90 % through that day: the aiming elevation is above 0 (sin 0.9 pi > 0), so it
    /// is day, while the old comparison with the stepped sun (theta 0, due east, against theta
    /// 0.9 pi: a negative dot) read night. A normal climate: day at 13:00, night at 23:00.
    #[test]
    fn day_follows_the_lights_own_clock_in_a_degenerate_climate() {
        use crate::sky::SkyState;
        let aimed_day = |sky: &SkyState| medium_air::day_by_aim(sky.sun_elevation_at(sky.sun_light_hour()));
        let mut sky = SkyState::default();
        sky.climate.sunrise = (6.0, 6.5);
        sky.climate.sunset = (5.5, 6.0);
        sky.hour = 6.0;
        sky.follow_game_clock(6.0009, 0.0);
        assert_eq!((sky.hour, sky.sun_light_hour()), (6.0, 6.0009), "stepped at 6:00, aimed at 6:0009");
        assert!(aimed_day(&sky), "the light is aimed at a sun above the horizon");
        // The old comparison: the aimed direction against the stepped hour's.
        let stepped = sky.sun_dir_game();
        let mut aimed_sky = SkyState::default();
        aimed_sky.climate = sky.climate.clone();
        aimed_sky.hour = 6.0009;
        let aimed = aimed_sky.sun_dir_game();
        assert!(aimed[2] > 0.0);
        let dot: f32 = (0..3).map(|i| aimed[i] * stepped[i]).sum();
        assert!(dot < 0.0, "the old comparison would have read night: {dot}");
        let mut normal = SkyState::default();
        normal.climate.sunrise = (6.0, 7.0);
        normal.climate.sunset = (18.0, 19.0);
        normal.hour = 13.0;
        assert!(aimed_day(&normal));
        normal.hour = 23.0;
        assert!(!aimed_day(&normal));
    }

    /// What is drawn for the light alone follows the gate as it settled: on with the gate, kept
    /// while a reason has not held its half second (and through one that comes and goes), gone
    /// once it has (the strength at 0; the same for a night without a moon, indoors, under water), back at once,
    /// and gone when the light is not wanted.
    #[test]
    fn what_is_drawn_for_the_light_follows_the_settled_gate() {
        let mut app = App::new();
        app.init_resource::<Time<Real>>().init_resource::<AirDraws>();
        open_gate(&mut app);
        let step = |app: &mut App, secs: f32| {
            app.world_mut().resource_mut::<Time<Real>>().advance_by(std::time::Duration::from_secs_f32(secs));
            app.update();
            app.world().resource::<AirDraws>().0
        };
        let strength = |app: &mut App, v: f32| {
            app.insert_resource(AirStrengthSetting(v));
        };
        let open_at = |app: &App| app.world().resource::<AirOpen>().0;
        assert!(step(&mut app, 0.1), "the gate open: at once");
        strength(&mut app, 0.0);
        assert!(step(&mut app, 0.1) && !open_at(&app) && step(&mut app, 0.2), "a reason that has not held, the frame's answer at once");
        strength(&mut app, 1.0);
        assert!(step(&mut app, 0.1) && open_at(&app));
        strength(&mut app, 0.0);
        assert!(step(&mut app, 0.1) && step(&mut app, 0.4), "its hold starts anew");
        assert!(!step(&mut app, 0.2), "held: gone with the gate");
        assert!(!step(&mut app, 5.0));
        strength(&mut app, 1.0);
        assert!(step(&mut app, 0.1), "back at once");
        for (what, close) in CLOSED {
            close(&mut app, true);
            step(&mut app, 0.1);
            assert!(!open_at(&app) && !step(&mut app, 0.6), "{what}");
            close(&mut app, false);
            assert!(step(&mut app, 0.1) && open_at(&app), "{what}: over");
        }
        app.insert_resource(AirWanted(false));
        assert!(!step(&mut app, 0.1) && !open_at(&app), "not wanted: at once");
    }

    /// The solid cores under the real schedule: in the first frame the gate is closed (the frame
    /// the composite stops) a core is hidden before the visibility check and still there, it
    /// comes back in the first open one, and it goes once the reason held.
    #[test]
    fn the_cores_are_hidden_in_the_frame_the_gate_closes() {
        use crate::solid_cores::tests as cores;
        let mut app = cores::app(Some(true));
        app.insert_resource(AirDraws(false));
        open_gate(&mut app);
        let m = cores::add(&mut app, cores::tank());
        let shape = cores::place(&mut app, &m);
        app.world_mut().entity_mut(shape).insert(InheritedVisibility::VISIBLE);
        let step = |app: &mut App, secs: f32| {
            app.world_mut().resource_mut::<Time<Real>>().advance_by(std::time::Duration::from_secs_f32(secs));
            app.update();
            let cores = cores::cores(app);
            cores.iter().map(|&c| (c, app.world().get::<InheritedVisibility>(c).is_some_and(|v| v.get()))).collect::<Vec<_>>()
        };
        let first = step(&mut app, 0.1);
        assert!(matches!(first[..], [(_, true)]), "the first open frame has the core: {first:?}");
        let core = first[0].0;
        let no_strength = |app: &mut App, on: bool| {
            app.insert_resource(AirStrengthSetting(if on { 0.0 } else { 1.0 }));
        };
        for (what, close) in CLOSED.into_iter().chain([("0 %", no_strength as fn(&mut App, bool))]) {
            close(&mut app, true);
            assert_eq!(step(&mut app, 0.1), [(core, false)], "{what}: hidden in the first closed frame, the same core");
            close(&mut app, false);
            assert_eq!(step(&mut app, 0.1), [(core, true)], "{what}: back in the first open one");
        }
        no_strength(&mut app, true);
        step(&mut app, 0.1);
        assert_eq!(step(&mut app, 0.6), [], "gone once the reason held");
        no_strength(&mut app, false);
        assert!(matches!(step(&mut app, 0.1)[..], [(_, true)]), "and made anew in the first open frame");
    }

    /// By night the light is the moon's where the climate has one: the gate stays open and its draws stay
    /// (the cores' too), with the moon's values; without a moon it closes. MoonlightNVSE.ini's factor is in the
    /// light's illuminance, so the share takes it there, once, and from nowhere else.
    #[test]
    fn by_night_the_air_is_lit_by_the_moon() {
        use crate::solid_cores::tests as cores;
        let mut app = cores::app(Some(true));
        app.insert_resource(AirDraws(false));
        open_gate(&mut app);
        let m = cores::add(&mut app, cores::tank());
        let shape = cores::place(&mut app, &m);
        app.world_mut().entity_mut(shape).insert(InheritedVisibility::VISIBLE);
        let step = |app: &mut App| {
            app.world_mut().resource_mut::<Time<Real>>().advance_by(std::time::Duration::from_secs_f32(0.7));
            app.update();
            let mut cams = app.world_mut().query::<&AirCamera>();
            *cams.single(app.world()).expect("one camera")
        };
        let by_day = step(&mut app);
        assert!(by_day.draws && by_day.share > 0.0);
        let night = |app: &mut App, moons: u8| {
            let mut sky = app.world_mut().resource_mut::<crate::sky::SkyState>();
            sky.hour = 1.0;
            sky.climate.moons = moons;
        };
        night(&mut app, 0x83);
        for _ in 0..3 {
            let moonlit = step(&mut app);
            assert!(moonlit.draws && app.world().resource::<AirOpen>().0 && app.world().resource::<AirDraws>().0);
            assert_eq!((moonlit.tune, moonlit.share), (AirTune { moon: medium_air::MoonTune::START, ..by_day.tune }.lit_by(AirSource::Moon), by_day.share));
            assert_eq!(cores::cores(&mut app).len(), 1, "the cores stay");
        }
        // The ini's factor: not applied to the share here, but in the illuminance (sky.rs `aim_sun`).
        app.world_mut().resource_mut::<crate::sky::SkyState>().moonlight = Some(crate::sky::MoonlightConfig { phase_brightness: true, phase_mults: [0.5; 5] });
        assert_eq!(step(&mut app).share, by_day.share);
        let mut lights = app.world_mut().query::<&mut DirectionalLight>();
        lights.single_mut(app.world_mut()).expect("the sun").illuminance *= 1e-6;
        let dim = step(&mut app).share;
        assert!(dim > 0.0 && dim < by_day.share, "{dim} against {}", by_day.share);
        // No moon: closed at once, the draws gone once the reason held.
        night(&mut app, 0x03);
        assert!(!step(&mut app).draws && !app.world().resource::<AirOpen>().0);
        assert!(!step(&mut app).draws && !app.world().resource::<AirDraws>().0);
        assert_eq!(cores::cores(&mut app).len(), 0);
        night(&mut app, 0x40);
        assert!(step(&mut app).draws, "back at once");
    }

    /// The share over the horizon, from the light as sky.rs aims it: 0 at the horizon itself, where the source
    /// changes, next to nothing on both sides, and no step in a sweep of a hundredth of a degree; black and
    /// broken colours give 0.
    #[test]
    fn the_share_is_nothing_at_the_horizon_on_both_sides() {
        let mut world = World::new();
        world.spawn((DirectionalLight::default(), Transform::IDENTITY));
        let mut lights = world.query::<(&mut DirectionalLight, &mut Transform)>();
        let unit = crate::sky::light_unit();
        let ambient = GlobalAmbientLight { color: Color::linear_rgb(0.6, 0.6, 0.6), brightness: 0.9 * unit, ..default() };
        let mut share = |degrees: f32| {
            let e = degrees.to_radians();
            let (mut light, mut t) = lights.single_mut(&mut world).expect("the light");
            crate::sky::aim_sun(&mut light, &mut t, Vec3::new(e.cos(), e.sin(), 0.0), Vec3::Z, PI * unit, 1.0);
            light_share(Some(&*light), Some(&ambient))
        };
        assert_eq!(share(0.0), 0.0);
        assert!(share(0.01) < 0.003 && share(-0.01) < 0.003 && share(0.01) > 0.0 && share(-0.01) > 0.0);
        let mut last = share(-8.0);
        for k in -799..=800 {
            let now = share(k as f32 * 0.01);
            assert!((now - last).abs() < 0.004, "{k}: {last} to {now}");
            last = now;
        }
        for colour in [Color::BLACK, Color::linear_rgb(f32::NAN, 1.0, 1.0), Color::linear_rgb(f32::INFINITY, 1.0, 1.0)] {
            let light = DirectionalLight { color: colour, illuminance: PI * unit, ..default() };
            assert_eq!(light_share(Some(&light), Some(&ambient)), 0.0, "{colour:?}");
        }
        let black = GlobalAmbientLight { color: Color::BLACK, brightness: 0.9 * unit, ..default() };
        assert_eq!(light_share(Some(&DirectionalLight { color: Color::BLACK, illuminance: PI * unit, ..default() }), Some(&black)), 0.0);
    }

    /// Closes the gate (true) or opens it again.
    type Close = fn(&mut App, bool);
    /// What closes the gate and opens it again.
    const CLOSED: [(&str, Close); 3] = [
        ("under water", |app, on| app.world_mut().resource_mut::<crate::underwater::CameraSubmerged>().0 = on),
        ("indoors", |app, on| app.world_mut().resource_mut::<crate::sky::SkyState>().interior = on.then(Default::default)),
        ("night without a moon", |app, on| app.world_mut().resource_mut::<crate::sky::SkyState>().hour = if on { 1.0 } else { 13.0 }),
    ];

    /// An open gate: the light wanted, a day sky, a sun, a camera with a fog the light can read.
    fn open_gate(app: &mut App) {
        let mut sky = crate::sky::SkyState::default();
        sky.climate.sunrise = (6.0, 7.0);
        sky.climate.sunset = (18.0, 19.0);
        sky.hour = 13.0;
        sky.interior = None;
        app.insert_resource(AirWanted(true))
            .insert_resource(AirStrengthSetting(1.0))
            .init_resource::<AirOpen>()
            .insert_resource(sky)
            .insert_resource(crate::underwater::CameraSubmerged(false))
            .add_plugins(AirLightPlugin);
        app.world_mut().spawn((crate::sky::Sun, DirectionalLight { illuminance: 10_000.0, ..default() }));
        let cam = AirCamera {
            grid: AirGrid::for_view(16.0 / 9.0, None, None),
            tune: AirTune::default(),
            fog: medium_air::AirFog { near: -10.0, far: 200_000.0, power: 0.6, strength: 1.0 },
            fog_colour: Vec3::splat(0.5),
            display_blend: true,
            to_light: Vec3::Y,
            has_light: true,
            outdoors: true,
            share: 0.0,
            draws: false,
        };
        app.world_mut().spawn((FlyCam { yaw: 0.0, pitch: 0.0, speed: 0.0 }, Projection::default(), cam));
    }

    /// The start strength is the aid's value when the aid asks for the light, else the row's.
    #[test]
    fn the_aid_wins_over_the_slider_for_the_start_strength() {
        assert_eq!(start_strength(None, 1.5), 1.5);
        assert_eq!(start_strength(Some(0.5), 1.5), 0.5);
        assert_eq!(start_strength(Some(2.0), 0.0), 2.0);
        // Through the tune: held to 0..=2.
        assert_eq!(AirTune::default().with_strength(start_strength(None, 1.5)).strength, 1.5);
    }
}
