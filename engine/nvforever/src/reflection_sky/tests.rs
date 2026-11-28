use super::*;

fn near(a: Vec3, b: Vec3, tol: f32) -> bool {
    (a - b).abs().max_element() <= tol
}

/// A clear day: three distinct dome colours (linear), an ambient, the light 45 degrees up
/// toward +X.
fn day() -> Inputs {
    Inputs::Outdoors {
        horizon: Vec3::new(0.6, 0.65, 0.7),
        lower: Vec3::new(0.3, 0.45, 0.8),
        upper: Vec3::new(0.05, 0.1, 0.6),
        ambient: Vec3::new(0.5, 0.5, 0.55),
        sun: Vec3::new(0.7, 0.65, 0.6),
        sun_dir: Vec3::new(1.0, 1.0, 0.0).normalize(),
        sky_scale: 0.88,
        clip: true,
        ground: None,
    }
}

fn dark(mut inputs: Inputs) -> Inputs {
    if let Inputs::Outdoors { sun, .. } = &mut inputs {
        *sun = Vec3::ZERO;
    }
    inputs
}

fn tony(mut inputs: Inputs) -> Inputs {
    if let Inputs::Outdoors { clip, .. } = &mut inputs {
        *clip = false;
    }
    inputs
}

fn room() -> Inputs {
    Inputs::Indoors { cell: Some(0x1234), ambient: Vec3::new(0.4, 0.35, 0.3), fog: Vec3::new(0.2, 0.25, 0.3), room: None }
}

/// The units identity of nvf_assets `ReflectionCube` ("Units"): with the cube's intensity
/// `light_unit()` and the default exposure E, a texel made from the view's exposed ambient A
/// reflects `texel x intensity x E = A x max(A, 1e-6)^1.2`, about A^2.2.
#[test]
fn a_texel_from_the_ambient_reflects_the_converted_ambient() {
    let exposure = bevy::camera::Exposure::default().exposure();
    for a in [0.0f32, 1e-7, 0.01, 0.2, 0.54, 0.9, 1.0] {
        let reflected = texel(Vec3::splat(a)).x * reflection_cube::light_unit() * exposure;
        let want = a * a.max(1e-6).powf(1.2);
        assert!((reflected - want).abs() <= 1e-6 * want.max(1.0), "A = {a}: {reflected} against {want}");
        if a >= 0.01 {
            assert!((reflected / a.powf(2.2) - 1.0).abs() < 1e-4, "A = {a}: about A^2.2");
        }
    }
    assert_eq!(texel(Vec3::new(-0.5, 0.0, 0.25)).x, 0.0, "held at 0");
}

/// Indoors the zenith is the ambient colour itself (the fog band is gone there): the
/// ambient-matched case, texel = A^2.2 in the identity's terms (A the display value times the
/// interior ambient share, as the ambient light's brightness carries it).
#[test]
fn an_interior_zenith_is_the_ambient_texel() {
    let Inputs::Indoors { ambient, .. } = room() else { unreachable!() };
    let c = SkyContent::new(&room(), &DomeProfile::fallback());
    assert!(near(c.colour(Vec3::Y, 0), texel(ambient * crate::sky::INTERIOR_AMBIENT_SHARE), 1e-5));
}

/// The sky's units: a perfect mirror (texel x intensity x exposure) writes what sky.wesl
/// writes for the dome, so the view's tone map shows both alike. With the clip in place
/// (lights.rs GAME_CLIP) the sky writes its scaled linear colour and the tone map is min(c, 1);
/// with Tony McMapface the sky writes tony_inverse of it, which the curve maps back.
#[test]
fn a_mirror_of_the_sky_shows_the_dome_as_drawn() {
    let exposure = bevy::camera::Exposure::default().exposure();
    let mirror = |t: Vec3| t * reflection_cube::light_unit() * exposure;
    let dome = Vec3::new(0.18, 0.3, 0.62) * 0.88;
    // Clipped display values: the mirror writes the dome's colour itself.
    assert!(near(mirror(sky_texel(dome, true)), dome, 1e-6));
    // Tony McMapface: after the curve the mirror shows the dome's colour.
    let shown = mirror(sky_texel(dome, false)).map(tony_forward);
    assert!(near(shown, dome, 1e-4), "{shown:?} against {dome:?}");
    // The tables invert each other at every sample.
    for (x, y) in TONY_IN.iter().zip(TONY_OUT) {
        assert!((tony_inverse(y) - x).abs() < 1e-4 && (tony_forward(*x) - y).abs() < 1e-6, "{x} {y}");
    }
    assert_eq!(sky_texel(Vec3::new(-1.0, 0.0, 0.5), true).x, 0.0, "held at 0");
}

/// A sky of one colour reads that colour times the sky scale in every direction above the
/// horizon (clip): the dome's linear colours go to the cube once, with the scale sky.wesl
/// applies (params.x), no display conversion and no ambient share.
#[test]
fn a_sky_of_one_colour_is_that_colour_times_the_sky_scale() {
    let colour = Vec3::new(0.25, 0.3, 0.35);
    let inputs = Inputs::Outdoors { horizon: colour, lower: colour, upper: colour, ambient: Vec3::splat(0.4), sun: Vec3::ZERO, sun_dir: Vec3::Y, sky_scale: 0.88, clip: true, ground: None };
    let c = SkyContent::new(&inputs, &DomeProfile::fallback());
    for e in [0.3f32, 0.7, 1.0] {
        let d = Vec3::new((1.0 - e * e).sqrt(), e, 0.0);
        assert!(near(c.colour(d, 0), colour * 0.88, 1e-4), "{e}: {:?}", c.colour(d, 0));
    }
    // Under Tony McMapface the same sky is written through the inverse curve.
    let c = SkyContent::new(&tony(inputs), &DomeProfile::fallback());
    assert!(near(c.colour(Vec3::Y, 0), (colour * 0.88).map(tony_inverse), 1e-4));
}

/// Lit ground: the display-value sum's diffuse, the texture's display value x the light,
/// held to 1, as linear light: the light on level ground is the ambient (x share) plus the
/// directional light times the sine of its elevation; with no light above the horizon, the
/// ambient alone.
#[test]
fn the_ground_is_lit_by_the_ambient_and_the_light_on_level_ground() {
    let albedo = ground_albedo();
    assert!(albedo.x > 0.08 && albedo.x < 0.1 && albedo.z < albedo.x, "a warm grey, linear: {albedo:?}");
    assert!(near(ground_texel(Vec3::splat(0.5), Vec3::splat(1.0)), Vec3::splat(0.5), 1e-6));
    // The shown value is the product of the two display values, dark ones too (a floor
    // of display 0.4 under an ambient of 40 / 255 shows 16 / 255, not the 8 / 255 that
    // light^2.2 through the frame's sRGB curve gave).
    for (texture, light) in [(0.5, 0.5), (0.4, 40.0 / 255.0), (0.2, 0.1), (0.9, 1.0), (0.05, 0.3)] {
        let lit = ground_texel(reflection_room::albedo_linear([texture; 3]), Vec3::splat(light));
        assert!((reflection_room::display_value(lit.x) - texture * light).abs() < 1e-5, "{texture} x {light}: {lit:?}");
    }
    assert!(near(ground_texel(Vec3::ONE, Vec3::splat(2.0)), Vec3::ONE, 1e-6), "held to 1 as the shader holds it");
    let Inputs::Outdoors { ambient, sun, sun_dir, .. } = day() else { unreachable!() };
    let lit = SkyContent::new(&day(), &DomeProfile::fallback()).colour(Vec3::NEG_Y, 0);
    let want = ground_texel(albedo, ambient * crate::sky::AMBIENT_SHARE + sun * sun_dir.y);
    assert!(near(lit, want, 1e-6), "{lit:?} against {want:?}");
    let night = SkyContent::new(&dark(day()), &DomeProfile::fallback()).colour(Vec3::NEG_Y, 0);
    assert!(near(night, ground_texel(albedo, ambient * crate::sky::AMBIENT_SHARE), 1e-6));
    assert!(night.x < lit.x, "the sun brightens the ground");
}

/// Zenith, horizon and nadir at level 0 (no light): the zenith the dome's top colour times
/// the sky scale, the nadir the ground under the ambient, the horizon halfway between the
/// ground and the dome's lowest colour.
#[test]
fn zenith_horizon_and_nadir() {
    let inputs = dark(day());
    let Inputs::Outdoors { horizon, lower, upper, ambient, sky_scale, .. } = inputs.clone() else { unreachable!() };
    let profile = DomeProfile::fallback();
    let c = SkyContent::new(&inputs, &profile);
    let dome = |e: f32| Vec3::from_array(crate::sky::dome_color(profile.weights(e), lower.to_array(), horizon.to_array(), upper.to_array())) * sky_scale;
    let ground = ground_texel(ground_albedo(), ambient * crate::sky::AMBIENT_SHARE);
    assert!(near(c.colour(Vec3::Y, 0), dome(1.0), 1e-4), "zenith");
    assert!(near(c.colour(Vec3::NEG_Y, 0), ground, 1e-6), "nadir");
    assert!(near(c.colour(Vec3::X, 0), (dome(0.0) + ground) * 0.5, 1e-4), "horizon");
    // The fallback's zenith is sky-upper alone.
    assert!(near(dome(1.0), upper * sky_scale, 1e-4));
}

/// The lobe peaks toward the directional light: brighter there than a few degrees off, at
/// every level, and wider (less peaked) at each level.
#[test]
fn the_lobe_peaks_toward_the_light_and_widens() {
    let inputs = day();
    let Inputs::Outdoors { sun_dir, .. } = inputs else { unreachable!() };
    let lit = SkyContent::new(&inputs, &DomeProfile::fallback());
    let unlit = SkyContent::new(&dark(day()), &DomeProfile::fallback());
    let off = (Quat::from_rotation_z(0.15) * sun_dir).normalize();
    let mut ratios = Vec::new();
    for level in 0..SPECULAR_LEVELS {
        let peak = lit.colour(sun_dir, level) - unlit.colour(sun_dir, level);
        let beside = lit.colour(off, level) - unlit.colour(off, level);
        assert!(peak.x > beside.x && peak.x > 0.0, "level {level}");
        ratios.push(beside.x / peak.x);
    }
    assert!(ratios.windows(2).all(|w| w[1] > w[0]), "wider at each level: {ratios:?}");
    // Its total is kept: the peak falls as it widens.
    let peak = |level| (lit.colour(sun_dir, level) - unlit.colour(sun_dir, level)).x;
    assert!(peak(SPECULAR_LEVELS - 1) < peak(0));
}

/// The levels approach each hemisphere's mean: the distance from the mean falls with the
/// level, and the last level is the mean (zenith and nadir, no light).
#[test]
fn levels_approach_the_hemisphere_means() {
    for inputs in [dark(day()), room()] {
        let c = SkyContent::new(&inputs, &DomeProfile::fallback());
        let (up, down) = c.means();
        for (d, mean) in [(Vec3::Y, up), (Vec3::NEG_Y, down), (Vec3::new(0.0, 0.8, 0.6), up)] {
            let dist: Vec<f32> = (0..SPECULAR_LEVELS).map(|l| (c.colour(d, l) - mean).length()).collect();
            assert!(dist.windows(2).all(|w| w[1] <= w[0] + 1e-6), "{inputs:?} {d}: {dist:?}");
            assert!(dist[SPECULAR_LEVELS as usize - 1] < 1e-4, "{inputs:?} {d}: the last level is the mean");
        }
    }
}

/// Indoors the fog colour holds the horizon and fades by the zenith and the nadir.
#[test]
fn an_interior_has_a_fog_band_at_the_horizon() {
    let Inputs::Indoors { fog, ambient, .. } = room() else { unreachable!() };
    let c = SkyContent::new(&room(), &DomeProfile::fallback());
    let fog_t = texel(fog);
    assert!(near(c.colour(Vec3::X, 0), fog_t, 1e-5), "the horizon is the fog colour");
    assert!(near(c.colour(Vec3::NEG_Y, 0), ground_texel(ground_albedo(), ambient), 1e-5), "the nadir the ground under the ambient");
    assert!((c.colour(Vec3::new(0.0, 0.2, 0.98).normalize(), 0) - fog_t).length() < (c.colour(Vec3::Y, 0) - fog_t).length());
}

/// A change below a step does not refill; one above does (module notes, "When").
#[test]
fn small_changes_do_not_refill_and_large_ones_do() {
    let base = day();
    let with = |f: &dyn Fn(&mut Vec3, &mut Vec3, &mut Vec3)| {
        let mut i = base.clone();
        if let Inputs::Outdoors { upper, ambient, sun_dir, .. } = &mut i {
            f(upper, ambient, sun_dir);
        }
        i
    };
    assert!(!base.moved(&base));
    // The ambient (display value) by half a step, then by two.
    assert!(!with(&|_, a, _| a.x += COLOUR_STEP * 0.5).moved(&base));
    assert!(with(&|_, a, _| a.x += COLOUR_STEP * 2.0).moved(&base));
    // A dome colour: compared in display value, so a linear change a little under the step's
    // worth does not count and a larger one does.
    assert!(!with(&|u, _, _| u.z += 0.002).moved(&base));
    assert!(with(&|u, _, _| u.z += 0.03).moved(&base));
    // The light's direction by one degree, then by five.
    let turn = |deg: f32| move |_: &mut Vec3, _: &mut Vec3, d: &mut Vec3| *d = Quat::from_rotation_y(deg.to_radians()) * *d;
    assert!(!with(&turn(1.0)).moved(&base));
    assert!(with(&turn(5.0)).moved(&base));
    // Without a light its direction does not matter.
    assert!(!dark(with(&turn(30.0))).moved(&dark(base.clone())));
    // Indoors against outdoors, and another cell.
    assert!(room().moved(&base) && base.moved(&room()));
    let other = Inputs::Indoors { cell: Some(0x9999), ambient: Vec3::new(0.4, 0.35, 0.3), fog: Vec3::new(0.2, 0.25, 0.3), room: None };
    assert!(other.moved(&room()));
    assert!(!room().moved(&room()));
}

/// The attach conditions: both rows on, an authored set in the mount, the gate installed and
/// the world camera present; the mount is not asked while a row is off.
#[test]
fn the_cube_is_made_only_when_every_condition_holds() {
    let installed = GateState::Installed;
    assert_eq!(decide(true, true, || true, installed, true), Decision::Allocate);
    assert_eq!(decide(false, true, || panic!("not asked"), installed, true), Decision::Off("setting"));
    assert_eq!(decide(true, false, || panic!("not asked"), installed, true), Decision::Off("Authored materials off"));
    assert_eq!(decide(true, true, || false, installed, true), Decision::Off("no authored sets in the mount"));
    assert_eq!(decide(true, true, || true, GateState::Failed, true), Decision::Off("reflection gate not installed"));
    assert_eq!(decide(true, true, || true, GateState::Pending, true), Decision::Wait);
    assert_eq!(decide(true, true, || true, installed, false), Decision::Wait);
}

fn spawn_world_camera(app: &mut App) -> Entity {
    app.world_mut().spawn((
        FlyCam { yaw: 0.0, pitch: 0.0, speed: 0.0 },
        GlobalTransform::IDENTITY,
    )).id()
}

fn running_producer() -> (App, Entity) {
    let mut app = App::new();
    // Exercise a running producer without touching the process-wide render gate.
    app.insert_resource(ReflectionSky { phase: Phase::On, ..default() })
        .insert_resource(crate::sky::authored_session_sky())
        .init_resource::<Assets<Image>>()
        .init_resource::<Time<Real>>()
        .add_systems(Update, produce);
    let camera = spawn_world_camera(&mut app);
    let cube = ReflectionCube::new(&mut app.world_mut().resource_mut::<Assets<Image>>());
    app.world_mut().entity_mut(camera).insert(cube);
    (app, camera)
}

/// Run the producer until no fill is in flight on the worker (module notes, "Where"): one
/// update asks, a later one applies the finished fill.
fn settle(app: &mut App) {
    for _ in 0..2000 {
        app.update();
        if !app.world().resource::<ReflectionSky>().queue.busy() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("the fill on the worker did not finish");
}

#[test]
fn the_producer_only_records_fills_after_writing_the_current_camera_cube() {
    let (mut app, camera) = running_producer();
    settle(&mut app);
    let previous = app.world().resource::<ReflectionSky>().last.clone();
    assert_eq!(app.world().resource::<ReflectionSky>().fills_outdoors, 1);

    let inherited_light = app.world().get::<ReflectionCube>(camera).unwrap().light();
    app.world_mut().despawn(camera);
    app.world_mut().resource_mut::<SkyState>().interior = Some(default());
    settle(&mut app);
    let state = app.world().resource::<ReflectionSky>();
    assert_eq!(state.last, previous, "no camera means no fill, even on an interior transition");
    assert_eq!((state.fills_outdoors, state.fills_indoors), (1, 0));
    assert_eq!(state.refills, [0; 8]);

    app.world_mut().resource_mut::<SkyState>().interior = None;
    let replacement = spawn_world_camera(&mut app);
    app.world_mut().entity_mut(replacement).insert(inherited_light);
    settle(&mut app);
    assert!(app.world().get::<EnvironmentMapLight>(replacement).is_none(), "an inherited light must wait for the new cube");
    let state = app.world().resource::<ReflectionSky>();
    let cube = app.world().get::<ReflectionCube>(replacement).expect("new camera gets a cube");
    assert_eq!(state.last, previous);
    assert_eq!(state.fills_outdoors, 2);
    assert_eq!(cube.generation(), 1);
    let previous_cube = cube.specular.id();
    let old_light = cube.light();

    app.world_mut().entity_mut(replacement).insert(old_light).remove::<ReflectionCube>();
    settle(&mut app);
    let recovered = app.world().get::<ReflectionCube>(replacement).expect("lost cube is replaced");
    assert_ne!(recovered.specular.id(), previous_cube);
    assert_eq!(recovered.generation(), 1);
    assert!(app.world().get::<EnvironmentMapLight>(replacement).is_none(), "old light must wait for the replacement upload");
    assert_eq!(app.world().resource::<ReflectionSky>().fills_outdoors, 3);

    app.world_mut().entity_mut(replacement)
        .insert(EnvironmentMapLight::default())
        .remove::<ReflectionCube>();
    settle(&mut app);
    assert!(app.world().get::<EnvironmentMapLight>(replacement).is_some(), "an unrelated map stays installed");
}

#[test]
fn removing_the_dome_profile_refills_an_unchanged_outdoor_cube() {
    let (mut app, camera) = running_producer();
    app.insert_resource(DomeProfile([[1.0, 0.0, 0.0]; DOME_PROFILE_BINS]));
    settle(&mut app);
    let previous = app.world().resource::<ReflectionSky>().last.clone();
    assert_eq!(app.world().get::<ReflectionCube>(camera).unwrap().generation(), 1);

    app.world_mut().remove_resource::<DomeProfile>();
    settle(&mut app);
    let state = app.world().resource::<ReflectionSky>();
    assert_eq!(state.last, previous);
    assert_eq!(state.profile_revision, None);
    assert_eq!(state.fills_outdoors, 2);
    assert_eq!(app.world().get::<ReflectionCube>(camera).unwrap().generation(), 2);
    let i = Refill::ALL.iter().position(|&r| r == Refill::Profile).unwrap();
    assert_eq!(state.refills[i], 1);
}

/// The dome profile from vertices: each step the mean of its vertices, empty steps filled
/// between their neighbours, vertices below the horizon left out; no vertex above it gives
/// the fallback.
#[test]
fn the_dome_profile_follows_the_vertices() {
    let at = |e: f32| [(1.0 - e * e).sqrt() * 500.0, 0.0, e * 500.0];
    let positions = [at(0.01), at(0.01), at(0.99), at(-0.5)];
    let colours = [[1.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0], [1.0, 1.0, 1.0, 1.0]];
    let p = DomeProfile::from_vertices(&positions, &colours);
    assert_eq!(p.0[0], [0.5, 0.5, 0.0]);
    assert_eq!(p.0[DOME_PROFILE_BINS - 1], [0.0, 0.0, 1.0]);
    let mid = p.0[DOME_PROFILE_BINS / 2];
    assert!(mid[2] > 0.4 && mid[2] < 0.6 && (mid.iter().sum::<f32>() - 1.0).abs() < 1e-5, "{mid:?}");
    let close = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6);
    assert!(close(p.weights(0.0), [0.5, 0.5, 0.0]));
    assert!(close(p.weights(1.0), [0.0, 0.0, 1.0]));
    assert_eq!(DomeProfile::from_vertices(&[at(-0.3)], &[[1.0; 4]]), DomeProfile::fallback());
    let f = DomeProfile::fallback();
    assert!(f.weights(1.0)[2] > 0.99 && f.weights(0.0)[1] > 0.7);
}

/// The summary names the reason when off and the counts when on.
#[test]
fn the_summary_line() {
    let mut s = ReflectionSky::new(false, true);
    s.phase = Phase::Off("setting");
    assert_eq!(s.line(), "off (setting)");
    s.phase = Phase::On;
    s.fills_outdoors = 3;
    s.fills_indoors = 1;
    s.unchanged = 40;
    s.room_fills = [1, 0, 0];
    s.refills[1] = 2;
    s.slowest_ms = 0.5;
    s.slowest_gather_ms = 0.12;
    s.slowest_apply_ms = 0.03;
    s.discarded = 1;
    assert_eq!(
        s.line(),
        "4 fills (3 outdoors, 1 indoors), 40 checks skipped as unchanged; 1 room fills (1 marker, 0 rays, 0 default), 0 ground-lamp fills, slowest room or lamp fill 0.50 ms; refills: 0 mode, 2 cell, 0 room, 0 box, 0 moved, 0 lamps, 0 light, 0 profile; main thread slowest gather 0.12 ms, slowest apply 0.03 ms; 1 fills for a replaced camera or cube discarded"
    );
}

// The analytic room (stage 2 version 1).

fn warm_lamp(position: Vec3, radius: f32) -> Lamp {
    Lamp { position, colour: Vec3::new(1.0, 0.8, 0.6), radius, spot: None }
}

/// The room module's copies of stage 1's rules are the same rules: the lit surface, the
/// units identity, the floor's albedo, the fog share.
#[test]
fn the_room_rules_are_stage_1s() {
    for (albedo, light) in [(Vec3::splat(0.2), Vec3::new(0.3, 0.6, 1.4)), (ground_albedo(), Vec3::new(0.0, 0.05, 2.5))] {
        assert_eq!(reflection_room::lit_texel(albedo, light), ground_texel(albedo, light));
    }
    for v in [Vec3::new(0.0, 1e-7, 0.3), Vec3::new(0.9, 1.0, 2.0), Vec3::new(-0.5, 0.2, 0.1)] {
        assert_eq!(reflection_room::display_texel(v), texel(v));
    }
    assert_eq!(reflection_room::FLOOR_ALBEDO, GROUND_ALBEDO);
    assert!(near(reflection_room::Albedo::default().floor, ground_albedo(), 1e-7));
    let fog = reflection_room::Fog { texel: Vec3::ZERO, near: 100.0, far: 900.0, power: 1.7 };
    for d in [0.0f32, 100.0, 300.0, 899.0, 2000.0] {
        assert!((fog.share(d) - crate::fog::share(100.0, 900.0, 1.7, d)).abs() < 1e-6, "{d}");
    }
}

#[test]
fn brightness_inverts_the_lights_lumens() {
    for b in [0.0f32, 0.4, 1.0, 2.7] {
        assert!((brightness(crate::lights::lumens(b, crate::sky::light_unit())) - b).abs() < 1e-5 * b.max(1.0), "{b}");
    }
}

/// A placed light as a lamp: display colour x steady brightness (a moving light's full
/// intensity), range as radius, a spot light's cone along its forward axis.
#[test]
fn a_placed_light_becomes_a_lamp() {
    let unit = crate::sky::light_unit();
    let point = PointLight { color: crate::lights::display_color([1.0, 0.5, 0.25]), intensity: crate::lights::lumens(2.0, unit), range: 300.0, ..default() };
    let l = lamp_of(Vec3::new(1.0, 2.0, 3.0), Vec3::NEG_Z, Some(&point), None, None).unwrap();
    assert!(near(l.colour, Vec3::new(2.0, 1.0, 0.5), 1e-4) && l.radius == 300.0 && l.spot.is_none());
    let steady = lamp_of(Vec3::ZERO, Vec3::NEG_Z, Some(&point), None, Some(crate::lights::lumens(3.0, unit))).unwrap();
    assert!(near(steady.colour, Vec3::new(3.0, 1.5, 0.75), 1e-4), "a flickering light at its full intensity");
    let spot = SpotLight { color: crate::lights::display_color([1.0, 1.0, 1.0]), intensity: crate::lights::lumens(1.0, unit), range: 200.0, outer_angle: 0.5, inner_angle: 0.4, ..default() };
    let s = lamp_of(Vec3::ZERO, Vec3::NEG_Y, None, Some(&spot), None).unwrap();
    assert!(s.spot.is_some_and(|c| near(c.direction, Vec3::NEG_Y, 1e-6) && c.factor(Vec3::NEG_Y) == 1.0 && c.factor(Vec3::X) == 0.0));
    assert!(lamp_of(Vec3::ZERO, Vec3::NEG_Z, None, None, None).is_none());
}

/// The fallback the brief pins: with no lamp in reach and only the default box, the indoor
/// content is stage 1's, texel for texel at every level.
#[test]
fn no_lamp_and_no_box_is_stage_1_exactly() {
    let eye = Vec3::new(10.0, 20.0, 30.0);
    let far = warm_lamp(eye + Vec3::new(3000.0, 0.0, 0.0), 300.0);
    assert_eq!(room_inputs(eye, BoxSource::Default, default_box(eye), &[(1, far)], Vec3::ZERO, Vec3::Y, None), None);
    let profile = DomeProfile::fallback();
    let (content, stage1) = (Content::new(&room(), &profile), SkyContent::new(&room(), &profile));
    assert!(matches!(content, Content::Sky(_)));
    for level in 0..SPECULAR_LEVELS {
        for d in [Vec3::Y, Vec3::NEG_Y, Vec3::X, Vec3::new(0.3, -0.2, 0.9).normalize(), Vec3::new(-0.5, 0.7, 0.1).normalize()] {
            assert_eq!(content.colour(d, level), stage1.colour(d, level), "level {level} {d}");
        }
    }
    // A room marker's box with no lamp is a room (its surfaces under the ambient), not stage 1.
    let marked = room_inputs(eye, BoxSource::Marker(7), default_box(eye), &[], Vec3::ZERO, Vec3::Y, None);
    assert!(marked.is_some_and(|r| r.lamps.is_empty()));
}

/// With a lamp in reach the content is the analytic room: brighter toward the lamp than
/// stage 1 there, and the wall near the lamp brighter than the far one.
#[test]
fn a_lamp_in_reach_makes_the_room() {
    let eye = Vec3::new(0.0, 100.0, 0.0);
    let lamp = warm_lamp(eye + Vec3::new(150.0, 60.0, 0.0), 300.0);
    let r = room_inputs(eye, BoxSource::Default, default_box(eye), &[(5, lamp)], Vec3::ZERO, Vec3::Y, None).expect("a room");
    let Inputs::Indoors { cell, ambient, fog, .. } = room() else { unreachable!() };
    let inputs = Inputs::Indoors { cell, ambient, fog, room: Some(r) };
    let profile = DomeProfile::fallback();
    let (content, stage1) = (Content::new(&inputs, &profile), SkyContent::new(&room(), &profile));
    assert!(matches!(content, Content::Room(_)));
    let toward = (lamp.position - eye).normalize();
    assert!(content.colour(toward, 0).x > stage1.colour(toward, 0).x + 0.5, "the lamp's lobe");
    let wall = Vec3::new(1.0, -0.3, 0.0).normalize();
    assert!(content.colour(wall, 0).x > content.colour(-wall, 0).x, "the wall near the lamp is the brighter");
}

/// A room marker's box in game space comes to Bevy space with the eye inside it, only for
/// the interior the rooms belong to; the rays' box needs five hits; the default box puts
/// the floor EYE_HEIGHT under the eye.
#[test]
fn the_box_from_a_marker_the_rays_or_the_default() {
    let mut rooms = crate::rooms::Rooms::default();
    rooms.cell = Some(0x42);
    rooms.rooms.push(crate::rooms::RoomBox { id: 0x0006_0001, center: Vec3::new(100.0, 200.0, 50.0), rot: Mat3::IDENTITY, half: Vec3::new(150.0, 100.0, 80.0) });
    // Bevy (120, 60, -210) is game (120, 210, 60).
    let inside = Vec3::new(120.0, 60.0, -210.0);
    let (id, b) = marker_box(Some(&rooms), Some(0x42), inside).expect("in the room");
    assert_eq!(id, 0x0006_0001);
    assert!(b.contains(inside, 0.0) && near(b.centre, Vec3::new(100.0, 50.0, -200.0), 1e-4));
    assert!(near(b.size(), Vec3::new(300.0, 200.0, 160.0), 1e-4), "sizes in game order: x, y, height");
    assert!((b.exit(inside, Vec3::NEG_Y).distance - 90.0).abs() < 0.1, "the floor at game z -30");
    assert!(marker_box(Some(&rooms), Some(0x43), inside).is_none(), "another interior");
    assert!(marker_box(Some(&rooms), Some(0x42), Vec3::new(1000.0, 0.0, 0.0)).is_none(), "outside every room");
    assert!(marker_box(None, Some(0x42), inside).is_none());

    // Game space: the eye 100 over the floor.
    let eye = Vec3::new(0.0, 0.0, 100.0);
    let hits = [Some(100.0), Some(300.0), Some(50.0), Some(150.0), Some(140.0), Some(100.0)];
    let b = box_from_rays(eye, hits).expect("six hits");
    assert!(near(b.size(), Vec3::new(400.0, 200.0, 240.0), 1e-3));
    let bevy_eye = Vec3::new(0.0, 100.0, 0.0);
    assert!(b.contains(bevy_eye, 0.0) && (b.exit(bevy_eye, Vec3::NEG_Y).distance - 100.0).abs() < 0.1);
    let one_miss = box_from_rays(eye, [Some(100.0), None, Some(50.0), Some(150.0), Some(140.0), Some(100.0)]).expect("five hits");
    assert!((one_miss.size().x - (100.0 + ROOM_RAY_MISS)).abs() < 1e-3);
    assert!(box_from_rays(eye, [Some(100.0), None, None, Some(150.0), Some(140.0), Some(100.0)]).is_none());
    let tight = box_from_rays(eye, [Some(1.0); 6]).unwrap();
    assert!(tight.half.min_element() >= MIN_HALF);

    let d = default_box(bevy_eye);
    assert!(d.contains(bevy_eye, 0.0));
    assert!((d.exit(bevy_eye, Vec3::NEG_Y).distance - EYE_HEIGHT).abs() < 0.1);
    assert!((d.exit(bevy_eye, Vec3::Y).distance - (DEFAULT_HEIGHT - EYE_HEIGHT)).abs() < 0.1);
}

#[test]
fn rotating_a_room_box_about_its_first_axis_can_move_it() {
    let room = default_box(Vec3::ZERO);
    let turn = |degrees: f32| RoomBox {
        axes: room.axes * Mat3::from_quat(Quat::from_rotation_x(degrees.to_radians())),
        ..room
    };
    assert!(!box_moved(&room, &turn(SUN_STEP_DEGREES * 0.5)));
    assert!(box_moved(&room, &turn(SUN_STEP_DEGREES * 2.0)));
}

/// The lamps: the nearest MAX_LAMPS of those bright enough, within reach and passing the
/// test, sorted by id.
#[test]
fn the_nearest_lamps_light_the_room() {
    let eye = Vec3::ZERO;
    let mut candidates: Vec<(u64, Lamp)> = (0..24).map(|i| (100 - i as u64, warm_lamp(Vec3::new(50.0 + 10.0 * i as f32, 0.0, 0.0), 500.0))).collect();
    candidates.push((1, Lamp { colour: Vec3::splat(COLOUR_STEP * 0.5), ..warm_lamp(Vec3::X, 500.0) }));
    candidates.push((2, warm_lamp(Vec3::new(LAMP_DISTANCE + 10.0, 0.0, 0.0), 1e5)));
    let picked = pick_lamps(&candidates, eye, |_| true);
    assert_eq!(picked.len(), MAX_LAMPS);
    assert!(picked.windows(2).all(|w| w[0].id < w[1].id), "sorted by id");
    let ids: Vec<u64> = picked.iter().map(|l| l.id).collect();
    assert_eq!(ids, (0..MAX_LAMPS as u64).map(|i| 100 - i).rev().collect::<Vec<_>>(), "the nearest sixteen; the dim and the far ones left out");
    assert_eq!(pick_lamps(&candidates, eye, |l| l.position.x < 75.0).len(), 3);
}

/// What refills the room: the eye past CENTRE_STEP, the box past BOX_STEP, the lamp set, a
/// lamp's colour past the step, another source; small moves do not.
#[test]
fn the_room_refills_on_a_step() {
    let eye = Vec3::new(0.0, 100.0, 0.0);
    let lamp = warm_lamp(eye + Vec3::new(100.0, 50.0, 0.0), 300.0);
    let Inputs::Indoors { cell, ambient, fog, .. } = room() else { unreachable!() };
    let make = |eye: Vec3, source: BoxSource, lamps: &[(u64, Lamp)]| Inputs::Indoors {
        cell,
        ambient,
        fog,
        room: room_inputs(eye, source, default_box(Vec3::new(0.0, 100.0, 0.0)), lamps, Vec3::ZERO, Vec3::Y, Some([0.0, 3000.0, 1.0])),
    };
    let base = make(eye, BoxSource::Rays, &[(3, lamp)]);
    assert_eq!(base.change(&base), None);
    assert_eq!(make(eye + Vec3::X * (CENTRE_STEP * 0.5), BoxSource::Rays, &[(3, lamp)]).change(&base), None);
    assert_eq!(make(eye + Vec3::X * (CENTRE_STEP + 1.0), BoxSource::Rays, &[(3, lamp)]).change(&base), Some(Refill::Moved));
    assert_eq!(make(eye, BoxSource::Marker(9), &[(3, lamp)]).change(&base), Some(Refill::Room));
    let other = warm_lamp(eye + Vec3::new(-100.0, 50.0, 0.0), 300.0);
    assert_eq!(make(eye, BoxSource::Rays, &[(3, lamp), (4, other)]).change(&base), Some(Refill::Lamps));
    let dimmer = Lamp { colour: lamp.colour - Vec3::splat(COLOUR_STEP * 0.5), ..lamp };
    assert_eq!(make(eye, BoxSource::Rays, &[(3, dimmer)]).change(&base), None, "below the colour step");
    let out = Lamp { colour: lamp.colour * 0.5, ..lamp };
    assert_eq!(make(eye, BoxSource::Rays, &[(3, out)]).change(&base), Some(Refill::Lamps));
    // The room comes and goes with its lamps on the default box.
    assert_eq!(make(eye, BoxSource::Default, &[]).change(&make(eye, BoxSource::Default, &[(3, lamp)])), Some(Refill::Lamps));
    // Another cell, and indoors against outdoors, come first.
    let Inputs::Indoors { room: r, .. } = base.clone() else { unreachable!() };
    assert_eq!(Inputs::Indoors { cell: Some(0x9999), ambient, fog, room: r.clone() }.change(&base), Some(Refill::Cell));
    assert_eq!(day().change(&base), Some(Refill::Mode));
    // A box moved past BOX_STEP.
    let mut moved = r.expect("a room");
    moved.room.half.x += BOX_STEP + 1.0;
    assert_eq!(Inputs::Indoors { cell, ambient, fog, room: Some(moved) }.change(&base), Some(Refill::Box));
}

#[test]
fn a_lit_room_fill_is_logged_after_an_initial_fill_without_lamps() {
    let mut state = ReflectionSky::default();
    let cell = Some(0x1234);
    assert!(state.mark_room_fill_for_log(cell, false), "first room fill");
    assert!(!state.mark_room_fill_for_log(cell, false), "repeated room fill without lamps");
    assert!(state.mark_room_fill_for_log(cell, true), "first room fill with lamps");
    assert!(!state.mark_room_fill_for_log(cell, true), "repeated room fill with lamps");
    assert!(!state.mark_room_fill_for_log(cell, false), "lights disappearing does not reopen the log");
    assert!(state.mark_room_fill_for_log(Some(0x5678), true), "first fill in another interior");
}

#[test]
fn underground_lamps_cannot_take_outdoor_ground_lamp_slots() {
    let eye = Vec3::new(0.0, 120.0, 0.0);
    let buried = warm_lamp(Vec3::new(0.0, -20.0, 0.0), 10.0);
    let mut candidates: Vec<_> = (1..=MAX_LAMPS as u64).map(|id| (id, buried)).collect();
    assert!(ground_inputs(eye, 0.0, &candidates).is_none(), "only buried lamps leave the sky fallback");
    let valid_id = MAX_LAMPS as u64 + 1;
    candidates.push((valid_id, warm_lamp(Vec3::new(0.0, 250.0, -150.0), 500.0)));
    let ground = ground_inputs(eye, 0.0, &candidates).expect("a lamp reaches the ground");
    assert_eq!(ground.lamps.len(), 1);
    assert_eq!(ground.lamps[0].id, valid_id, "closer buried lamps cannot displace it");

    let crossing = warm_lamp(Vec3::new(0.0, -20.0, 0.0), 30.0);
    assert!(ground_inputs(eye, 0.0, &[(1, crossing)]).is_some(), "a sphere can reach the ground from below");
}

/// Outdoors a lamp near the eye adds lit ground below the horizon on top of stage 1, and
/// nothing far above the horizon away from it.
#[test]
fn lamps_light_the_ground_outdoors_at_night() {
    let eye = Vec3::new(0.0, 120.0, 0.0);
    let lamp = warm_lamp(Vec3::new(0.0, 250.0, -150.0), 500.0);
    let g = ground_inputs(eye, 0.0, &[(1, lamp)]).expect("a lamp near");
    assert!(ground_inputs(eye, 0.0, &[(1, warm_lamp(Vec3::new(3000.0, 250.0, 0.0), 500.0))]).is_none(), "too far");
    assert!(ground_inputs(eye, 0.0, &[(1, warm_lamp(Vec3::new(0.0, 900.0, 0.0), 500.0))]).is_none(), "its light does not reach the ground");
    let night = dark(day());
    let Inputs::Outdoors { horizon, lower, upper, ambient, sun, sun_dir, sky_scale, clip, .. } = night.clone() else { unreachable!() };
    let lit = Inputs::Outdoors { horizon, lower, upper, ambient, sun, sun_dir, sky_scale, clip, ground: Some(g) };
    let profile = DomeProfile::fallback();
    let (with, without) = (Content::new(&lit, &profile), Content::new(&night, &profile));
    let below = Vec3::new(0.0, -0.6, -0.8).normalize();
    assert!(with.colour(below, 0).x > without.colour(below, 0).x);
    let away = Vec3::new(0.0, 0.8, 0.6).normalize();
    assert!(near(with.colour(away, 0), without.colour(away, 0), 1e-6));
    assert_eq!(lit.change(&night), Some(Refill::Lamps));
}

// The fill on a worker (maintainer decision 2026-11-22, #3238).

/// The inputs of a lamp-lit room (the default box, two lamps) for the fill tests.
fn lit_room() -> Inputs {
    let eye = Vec3::new(0.0, 100.0, 0.0);
    let lamps = [(5, warm_lamp(eye + Vec3::new(150.0, 60.0, 0.0), 300.0)), (9, warm_lamp(eye + Vec3::new(-120.0, 80.0, 90.0), 260.0))];
    let r = room_inputs(eye, BoxSource::Rays, default_box(eye), &lamps, Vec3::ZERO, Vec3::Y, Some([64.0, 2048.0, 1.0])).expect("a room");
    let Inputs::Indoors { cell, ambient, fog, .. } = room() else { unreachable!() };
    Inputs::Indoors { cell, ambient, fog, room: Some(r) }
}

/// The worker's fill is the main thread's old fill: the same bytes as
/// `ReflectionCube::fill` from `Content::new` (what `produce` ran before the move), for the
/// sky, stage 1's interior, the room and the ground lamps, and the same bytes again on a
/// second call; the samples the log lines quote are the content's texels.
#[test]
fn the_fill_on_a_worker_writes_the_main_threads_bytes() {
    let profile = DomeProfile::fallback();
    let eye = Vec3::new(0.0, 120.0, 0.0);
    let g = ground_inputs(eye, 0.0, &[(1, warm_lamp(Vec3::new(0.0, 250.0, -150.0), 500.0))]).expect("a lamp near");
    let Inputs::Outdoors { horizon, lower, upper, ambient, sun, sun_dir, sky_scale, clip, .. } = dark(day()) else { unreachable!() };
    let ground = Inputs::Outdoors { horizon, lower, upper, ambient, sun, sun_dir, sky_scale, clip, ground: Some(g) };
    for inputs in [day(), room(), lit_room(), ground] {
        let content = Content::new(&inputs, &profile);
        let before = reflection_cube::specular_levels(|d, level| content.colour(d, level));
        let filled = fill(&inputs, &profile, true);
        assert!(filled.levels == before, "the same bytes as the main thread's fill: {inputs:?}");
        let again = fill(&inputs, &profile, false);
        assert!(again.levels == filled.levels, "deterministic, with or without the means");
        assert_eq!(again.samples.means, None, "the means only when asked for");
        assert_eq!(filled.samples.zenith, content.colour(Vec3::Y, 0));
        assert_eq!(filled.samples.nadir, content.colour(Vec3::NEG_Y, 0));
        assert_eq!(filled.samples.means, Some(content.means()));
    }
    let Inputs::Indoors { room: Some(r), .. } = lit_room() else { unreachable!() };
    let nearest = r.lamps.iter().min_by(|a, b| a.lamp.position.distance(r.eye).total_cmp(&b.lamp.position.distance(r.eye))).unwrap();
    let content = Content::new(&lit_room(), &profile);
    assert_eq!(fill(&lit_room(), &profile, false).samples.nearest_lamp, Some(content.colour((nearest.lamp.position - r.eye).normalize(), 0)));
    assert_eq!(fill(&room(), &profile, false).samples.nearest_lamp, None);
}

/// A job of the queue tests: its result once finished.
type Job = Option<u32>;

fn ready(job: &mut Job) -> Option<u32> {
    job.take()
}

/// An idle queue starts a fill at once; while it runs nothing is handed back to apply, so
/// the cube keeps the last finished fill; once finished with nothing newer asked, its result
/// is applied and the queue is idle.
#[test]
fn the_last_finished_fill_stays_while_a_fill_runs() {
    let mut q: FillQueue<&str, Job> = FillQueue::default();
    assert_eq!(q.poll(ready, |_| panic!("nothing to start")), Polled::Idle);
    let mut started = Vec::new();
    q.ask("first", |r| {
        started.push(*r);
        None
    });
    assert_eq!(started, ["first"]);
    assert!(q.busy());
    assert_eq!(q.poll(ready, |_| panic!("nothing waits")), Polled::Running, "not finished: nothing to apply");
    q.running.as_mut().unwrap().1 = Some(1);
    assert_eq!(q.poll(ready, |_| panic!("nothing waits")), Polled::Done("first", 1));
    assert!(!q.busy());
    assert_eq!(q.poll(ready, |_| panic!("nothing to start")), Polled::Idle);
}

/// Inputs asked while a fill runs wait, the newest replacing an older waiting request; when
/// the running fill finishes it is still applied (it is newer than what the cube shows) and
/// the newest waiting request starts in the same poll; the one it replaced never runs.
#[test]
fn a_fill_finishing_while_newer_inputs_wait_is_applied_and_the_newer_fill_starts() {
    let mut q: FillQueue<&str, Job> = FillQueue::default();
    let mut started = Vec::new();
    q.ask("cell A", |r| {
        started.push(*r);
        None
    });
    q.ask("moved", |_| panic!("one fill at a time"));
    assert_eq!(q.waiting(), Some(&"moved"));
    q.ask("cell B", |_| panic!("one fill at a time"));
    assert_eq!(q.waiting(), Some(&"cell B"), "the newest inputs replace the waiting ones");
    q.running.as_mut().unwrap().1 = Some(1);
    assert_eq!(
        q.poll(ready, |r| {
            started.push(*r);
            None
        }),
        Polled::Done("cell A", 1),
        "cell A's room is newer than the cube's: applied"
    );
    assert_eq!(started, ["cell A", "cell B"], "cell B started in the same poll; \"moved\" never ran");
    assert_eq!(q.waiting(), None);
    assert!(q.busy());
    assert_eq!(q.poll(ready, |_| panic!("nothing waits")), Polled::Running);
    q.running.as_mut().unwrap().1 = Some(2);
    assert_eq!(q.poll(ready, |_| panic!("nothing waits")), Polled::Done("cell B", 2));
    assert!(!q.busy());
}

/// In the game: the update that asks a fill leaves the camera's cube as it was (the fill
/// runs on the worker); a later update moves the finished bytes in, the same bytes the
/// main thread wrote before.
#[test]
fn the_producer_applies_the_workers_fill_a_frame_later() {
    let (mut app, camera) = running_producer();
    app.update();
    let state = app.world().resource::<ReflectionSky>();
    assert!(state.last.is_some(), "asked");
    assert_eq!(state.fills_outdoors, 0, "not applied in the update that asked");
    assert_eq!(app.world().get::<ReflectionCube>(camera).unwrap().generation(), 0, "the cube keeps what it held");
    settle(&mut app);
    let state = app.world().resource::<ReflectionSky>();
    assert_eq!(state.fills_outdoors, 1);
    let inputs = state.last.clone().expect("the asked inputs");
    let want = fill(&inputs, &DomeProfile::fallback(), false).levels;
    let cube = app.world().get::<ReflectionCube>(camera).unwrap();
    assert_eq!(cube.generation(), 1);
    assert!(cube.levels() == want.as_slice(), "the worker's bytes, moved into the cube");
}

/// The worker's fills held (the tests' seam) until [`Hold::release`] or the guard drops, so
/// a failing assertion cannot leave a pool thread waiting.
struct Hold(Arc<AtomicBool>);

impl Hold {
    fn release(&self) {
        self.0.store(false, Ordering::Release);
    }
}

impl Drop for Hold {
    fn drop(&mut self) {
        self.release();
    }
}

fn hold_fills(app: &mut App) -> Hold {
    let hold = Arc::new(AtomicBool::new(true));
    app.world_mut().resource_mut::<ReflectionSky>().hold = Some(hold.clone());
    Hold(hold)
}

/// Two lit interiors whose cubes differ: a warm and a cool ambient, with a fog colour.
fn warm_interior() -> fnv_world::lighting::CellLighting {
    fnv_world::lighting::CellLighting { ambient: [150, 110, 70], fog_color: [60, 50, 40], ..default() }
}

fn cool_interior() -> fnv_world::lighting::CellLighting {
    fnv_world::lighting::CellLighting { ambient: [40, 90, 150], fog_color: [30, 40, 70], ..default() }
}

/// The fixture is not degenerate: both fills have colour and their bytes differ.
fn assert_distinct(a: &Inputs, b: &Inputs) {
    let (fa, fb) = (fill(a, &DomeProfile::fallback(), false), fill(b, &DomeProfile::fallback(), false));
    assert!(fa.samples.zenith.max_element() > 0.0 && fb.samples.zenith.max_element() > 0.0, "both lit: {a:?} {b:?}");
    assert!(fa.levels != fb.levels, "the two fills' bytes differ");
}

/// A camera's fill still running when the camera is replaced never reaches the new camera:
/// the new camera loses the light inherited from the old cube at once, not after a fill;
/// the old fill is discarded when it finishes; the new camera gets its own fill (the cool
/// interior's bytes, never the old camera's warm ones).
#[test]
fn a_fill_for_a_replaced_camera_is_discarded() {
    let (mut app, camera_a) = running_producer();
    settle(&mut app);
    let hold = hold_fills(&mut app);
    // A's fill of new inputs (a warm interior), held on the worker.
    app.world_mut().resource_mut::<SkyState>().interior = Some(warm_interior());
    app.update();
    let a_inputs = app.world().resource::<ReflectionSky>().last.clone().unwrap();
    assert!(a_inputs.indoors() && app.world().resource::<ReflectionSky>().queue.busy());
    let inherited = app.world().get::<ReflectionCube>(camera_a).unwrap().light();

    // B replaces A, carrying A's light, in a cool interior, so B's inputs differ from A's fill.
    app.world_mut().despawn(camera_a);
    let camera_b = spawn_world_camera(&mut app);
    app.world_mut().entity_mut(camera_b).insert(inherited);
    app.world_mut().resource_mut::<SkyState>().interior = Some(cool_interior());
    app.update();
    assert!(app.world().get::<EnvironmentMapLight>(camera_b).is_none(), "the inherited light goes when the camera changes");
    assert!(app.world().get::<ReflectionCube>(camera_b).is_none(), "A's fill is not B's");
    let b_inputs = app.world().resource::<ReflectionSky>().last.clone().unwrap();
    assert_distinct(&a_inputs, &b_inputs);
    app.update();
    assert!(app.world().get::<EnvironmentMapLight>(camera_b).is_none() && app.world().get::<ReflectionCube>(camera_b).is_none());

    hold.release();
    settle(&mut app);
    let state = app.world().resource::<ReflectionSky>();
    assert_eq!(state.discarded, 1, "A's fill discarded");
    assert_eq!(state.fills_indoors, 1, "only B's interior fill applied");
    let cube = app.world().get::<ReflectionCube>(camera_b).expect("B's own fill made its cube");
    assert_eq!(cube.generation(), 1, "only B's fill went in");
    assert!(cube.levels() == fill(&b_inputs, &DomeProfile::fallback(), false).levels.as_slice(), "B's bytes");
    assert!(cube.levels() != fill(&a_inputs, &DomeProfile::fallback(), false).levels.as_slice(), "never A's");
}

/// The same for a cube replaced on the same camera while its fill runs: the old cube's
/// light goes at once, the old fill is discarded, the new cube gets only its own fill.
#[test]
fn a_fill_for_a_replaced_cube_is_discarded() {
    let (mut app, camera) = running_producer();
    settle(&mut app);
    let old = app.world().get::<ReflectionCube>(camera).unwrap().clone();
    app.world_mut().entity_mut(camera).insert(old.light());
    let hold = hold_fills(&mut app);
    app.world_mut().resource_mut::<SkyState>().interior = Some(warm_interior());
    app.update();
    let old_inputs = app.world().resource::<ReflectionSky>().last.clone().unwrap();
    assert!(app.world().resource::<ReflectionSky>().queue.busy());

    let replacement = ReflectionCube::new(&mut app.world_mut().resource_mut::<Assets<Image>>());
    let replacement_id = replacement.specular.id();
    app.world_mut().entity_mut(camera).insert(replacement);
    app.world_mut().resource_mut::<SkyState>().interior = Some(cool_interior());
    app.update();
    assert!(app.world().get::<EnvironmentMapLight>(camera).is_none(), "the old cube's light goes when the cube changes");
    let new_inputs = app.world().resource::<ReflectionSky>().last.clone().unwrap();
    assert_distinct(&old_inputs, &new_inputs);
    assert_eq!(app.world().get::<ReflectionCube>(camera).unwrap().generation(), 0);

    hold.release();
    settle(&mut app);
    assert_eq!(app.world().resource::<ReflectionSky>().discarded, 1);
    let cube = app.world().get::<ReflectionCube>(camera).unwrap();
    assert_eq!(cube.specular.id(), replacement_id);
    assert_eq!(cube.generation(), 1, "only the new cube's own fill went in");
    assert!(cube.levels() == fill(&new_inputs, &DomeProfile::fallback(), false).levels.as_slice());
    assert!(cube.levels() != fill(&old_inputs, &DomeProfile::fallback(), false).levels.as_slice());
}

/// A camera that comes back holding the light of an older cube (not the last one filled)
/// loses it too: any cube a fill went into counts, so `attach_lights` can attach the
/// camera's own cube later.
#[test]
fn a_light_of_any_older_cube_is_removed() {
    let (mut app, camera_a) = running_producer();
    settle(&mut app);
    let a_light = app.world().get::<ReflectionCube>(camera_a).unwrap().light();
    // B takes over and gets its own cube and fill; A's cube is now the older one.
    app.world_mut().despawn(camera_a);
    let camera_b = spawn_world_camera(&mut app);
    settle(&mut app);
    assert!(app.world().get::<ReflectionCube>(camera_b).is_some());
    // A camera comes back with A's light and no cube.
    app.world_mut().despawn(camera_b);
    let back = spawn_world_camera(&mut app);
    app.world_mut().entity_mut(back).insert(a_light);
    app.update();
    assert!(app.world().get::<EnvironmentMapLight>(back).is_none(), "the older cube's light goes too");
}
