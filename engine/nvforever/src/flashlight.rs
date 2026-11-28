//! The flashlight's light (`nvf_play::flashlight`; owner request 2026-10-11): while it is
//! on, a spot light on the front of the player's body lights a cone ahead of the view.
//! [design: not a light of the game's; colour, reach, cone and brightness are chosen to
//! look like other games' flashlights, in the display units of `lights::spawn_light`.]
//!
//! - It sits on the body, in front of the face a little under the eye, facing where the
//!   player looks (owner, playtest list 5 items 66 and 67): in a view from outside it
//!   lights what is ahead of the body, not the body's back from the camera, and nearly on
//!   the line of the eye its shadows fall behind what casts them instead of beside it.
//! - In a view from outside it is placed from the third-person head's eye
//!   (`PlayerBody::head_eye`), so it moves with the head: from the first-person eye the
//!   head leaned into it walking and running and shaded the cone (owner 2026-10-18). In
//!   first person the head casts nothing on it (`sky::SHADOW_LAYER`) and the view's eye
//!   places it.
//! - It casts shadows: walls stop it instead of it lighting the rooms behind them. It
//!   falls off as the game's lights do (`lights.rs`).
//! - Shadow casters are the world's layer (0), as the light's own layers are: the player
//!   casts none (owner 2026-10-22: aiming in third person the arms shaded the cone). The
//!   first-person arms and weapon (`sky::ARMS_LAYER`), the seen third-person body and its
//!   weapon (`sky::BODY_LAYER`) and the unseen body (`sky::SHADOW_LAYER`) are on layers
//!   of their own. It still lights them: a light lights every view its layers meet. On
//!   first-person shapes (`FIRST_PERSON_DEPTH`, `lights/display_shader.rs`) shadow-casting
//!   spot lights are scaled down (`FIRST_PERSON_FLASHLIGHT_DIFFUSE`,
//!   `FIRST_PERSON_FLASHLIGHT_SPECULAR`) [design, #3121] so aiming down sights into the
//!   beam's inner core does not wash the held weapon and hand out to white, while world
//!   surfaces, third-person body/weapon (`BODY_LAYER`) and non-shadow-casting world spot
//!   lamps (`LIGH` `SPOT`) keep full strength.
//! - Detection does not count it (`detection.rs`).

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use nvf_play::combat::WeaponAnim;
use nvf_play::flashlight::Flashlight;
use nvf_play::pause::Paused;
use nvf_play::player::Player;
use nvf_play::save::SaveLog;
use nvf_play::script::NewGame;
use nvf_play::{Location, PlayState};
use crate::flashlight_ini::{BatteryConfig, BetterFlashlightConfig};
use crate::player_body::PlayerBody;
use crate::sky::SkyState;

/// Marks the flashlight's light entity.
#[derive(Component)]
pub struct PlayerFlashlight;

/// Runtime state for `BetterFlashlightNVSE.ini` battery drain/recharge, warm-up ramp,
/// and sporadic flicker (unused when the INI is absent or disabled).
#[derive(Resource, Debug, Clone)]
pub struct FlashlightRuntime {
    /// Battery charge in `0.0..=100.0` (starts full at `100.0`).
    pub charge: f32,
    /// True after hitting `0.0` until `charge >= recharge_threshold`.
    pub depleted_lockout: bool,
    /// Seconds elapsed since the flashlight was last switched on.
    pub on_elapsed: f32,
    was_on: bool,
    seen_loads: usize,
    seen_new_game: bool,
}

impl Default for FlashlightRuntime {
    fn default() -> Self {
        Self {
            charge: 100.0,
            depleted_lockout: false,
            on_elapsed: 0.0,
            was_on: false,
            seen_loads: 0,
            seen_new_game: false,
        }
    }
}

impl FlashlightRuntime {
    /// Reset battery charge to 100% and clear depletion lockout (on load or new game).
    pub fn reset_on_load(&mut self) {
        self.charge = 100.0;
        self.depleted_lockout = false;
        self.on_elapsed = 0.0;
        self.was_on = false;
    }
}

/// Warm white, as an incandescent bulb.
const COLOUR: [f32; 3] = [1.0, 0.95, 0.86];
/// Brightness at the light, display units (`lights::lumens`).
const BRIGHTNESS: f32 = 2.0;
/// Reach, game units (about 40 m).
const RANGE: f32 = 2800.0;
/// Half-angle of the cone's edge, and of its fully lit middle, degrees.
const OUTER_DEG: f32 = 30.0;
const INNER_DEG: f32 = 14.0;
/// Where the light sits from the head's eye: ahead along the body's heading, and
/// down, game units [design].
const AHEAD: f32 = 14.0;
const DOWN: f32 = 6.0;
/// Shadow near plane, game units.
const NEAR: f32 = 8.0;

pub struct FlashlightPlugin;

impl Plugin for FlashlightPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FlashlightRuntime>()
            .add_systems(Startup, crate::flashlight_ini::load_from_vfs)
            .add_systems(Update, sync.after(nvf_play::PlaySet::Physics));
    }
}

/// The light's place and rotation (Bevy space) for the player's eye (game space),
/// heading and pitch.
fn placement(eye: [f32; 3], heading: f32, pitch: f32) -> Transform {
    let at = [eye[0] + AHEAD * heading.sin(), eye[1] + AHEAD * heading.cos(), eye[2] - DOWN];
    // A spot light looks down its -z, as a camera does.
    Transform::from_translation(nvf_play::space::to_bevy(at)).with_rotation(nvf_play::space::look_rotation(heading, pitch))
}

/// Vanilla Pip-Boy `SpotLight` parameters (unchanged when `BetterFlashlightNVSE.ini` is
/// absent or inactive).
fn vanilla_spotlight() -> SpotLight {
    SpotLight {
        color: crate::lights::display_color(COLOUR),
        intensity: crate::lights::lumens(BRIGHTNESS, crate::sky::light_unit()),
        range: RANGE,
        outer_angle: OUTER_DEG.to_radians(),
        inner_angle: INNER_DEG.to_radians(),
        shadow_maps_enabled: true,
        shadow_map_near_z: NEAR,
        ..default()
    }
}

/// Incandescent warm-up ramp multiplier in `0.0..=1.0` [design].
pub(crate) fn warmup_factor(elapsed: f32, warmup_time: f32) -> f32 {
    if warmup_time <= 0.0 {
        return 1.0;
    }
    let u = (elapsed / warmup_time).clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

/// Deterministic sporadic-dip flicker multiplier in `0.15..=1.0` [design].
///
/// `0.0` produces a steady `1.0`; larger values increase both dip probability and dip
/// depth while remaining mostly steady with brief dips.
pub(crate) fn flicker_factor(elapsed: f32, strength: f32) -> f32 {
    if strength <= 0.0 {
        return 1.0;
    }
    const SLOT_SECS: f32 = 0.08;
    let t = elapsed.max(0.0);
    let slot_f = (t / SLOT_SECS).floor();
    let slot = slot_f as u64;
    let frac = (t / SLOT_SECS - slot_f).clamp(0.0, 1.0);

    // SplitMix64 mixing on the time slot index.
    let mut z = slot.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    let hash = z ^ (z >> 31);

    let trigger_roll = ((hash & 0xFFFF) as f32) / 65536.0;
    let depth_roll = (((hash >> 16) & 0xFFFF) as f32) / 65536.0;
    let chance = (0.08 * strength).clamp(0.0, 0.55);
    if trigger_roll >= chance {
        return 1.0;
    }
    let pulse = (frac * std::f32::consts::PI).sin();
    let max_dip = (0.45 * strength * (0.5 + 0.5 * depth_roll)).clamp(0.0, 0.85);
    (1.0 - max_dip * pulse).clamp(0.15, 1.0)
}

/// Advance `[Battery]` charge and depletion-lockout state for one frame (`dt` seconds).
pub(crate) fn step_battery(
    runtime: &mut FlashlightRuntime,
    battery: BatteryConfig,
    flashlight: &mut Flashlight,
    dt: f32,
) {
    if !battery.enabled {
        return;
    }
    if runtime.depleted_lockout && flashlight.on {
        flashlight.on = false;
    }
    if flashlight.on {
        runtime.charge = (runtime.charge - battery.drain_rate * dt).max(0.0);
        if runtime.charge <= 0.0 {
            runtime.charge = 0.0;
            runtime.depleted_lockout = battery.recharge_threshold > 0.0;
            flashlight.on = false;
        }
    } else {
        runtime.charge = (runtime.charge + battery.recharge_rate * dt).min(100.0);
        if runtime.depleted_lockout && runtime.charge >= battery.recharge_threshold {
            runtime.depleted_lockout = false;
        }
    }
}

/// Spotlight placement when `BetterFlashlightNVSE.ini` is active [design].
///
/// Aligns with the drawn weapon's muzzle and barrel axis when `AttachToWeapon` is on and
/// a weapon is in hand, otherwise uses the camera/head-eye attach point, then applies the
/// local `(OffsetX, OffsetY, OffsetZ)` offset in game-space `(right, forward, up)` units
/// [hypothesis: FNV axis convention].
pub(crate) fn configured_placement(
    eye: [f32; 3],
    heading: f32,
    pitch: f32,
    weapon_aim: Option<([f32; 3], [f32; 3])>,
    weapon_muzzle: Option<[f32; 3]>,
    config: &BetterFlashlightConfig,
) -> Transform {
    let mut t = if config.attach_to_weapon {
        if let Some((origin, dir)) = weapon_aim {
            let fwd = nvf_play::space::to_bevy(dir).normalize_or_zero();
            if fwd.length_squared() > 0.5 {
                let up = if fwd.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
                Transform::from_translation(nvf_play::space::to_bevy(origin)).looking_to(fwd, up)
            } else {
                Transform::from_translation(nvf_play::space::to_bevy(origin))
                    .with_rotation(nvf_play::space::look_rotation(heading, pitch))
            }
        } else if let Some(origin) = weapon_muzzle {
            Transform::from_translation(nvf_play::space::to_bevy(origin))
                .with_rotation(nvf_play::space::look_rotation(heading, pitch))
        } else {
            placement(eye, heading, pitch)
        }
    } else {
        placement(eye, heading, pitch)
    };
    if config.offset != [0.0, 0.0, 0.0] {
        t.translation += t.rotation * nvf_play::space::to_bevy(config.offset);
    }
    t
}

/// Spotlight parameters when `BetterFlashlightNVSE.ini` is active [design].
pub(crate) fn configured_spotlight(
    config: &BetterFlashlightConfig,
    interior: bool,
    on_elapsed: f32,
) -> SpotLight {
    let profile = config.profile(interior);
    let warmup = warmup_factor(on_elapsed, config.warmup_time);
    let flicker = flicker_factor(on_elapsed, config.flicker);
    let brightness = (BRIGHTNESS * profile.dimmer * profile.intensity * warmup * flicker).max(0.0);
    let (inner_angle, outer_angle) = config.cone_angles_rad();
    SpotLight {
        color: crate::lights::display_color(config.color),
        intensity: crate::lights::lumens(brightness, crate::sky::light_unit()),
        range: config.distance,
        outer_angle,
        inner_angle,
        shadow_maps_enabled: true,
        shadow_map_near_z: NEAR,
        ..default()
    }
}

fn spotlight_differs(a: &SpotLight, b: &SpotLight) -> bool {
    a.color != b.color
        || a.intensity != b.intensity
        || a.range != b.range
        || a.outer_angle != b.outer_angle
        || a.inner_angle != b.inner_angle
        || a.shadow_maps_enabled != b.shadow_maps_enabled
        || a.shadow_map_near_z != b.shadow_map_near_z
}

#[derive(SystemParam)]
struct FlashlightContext<'w> {
    player: Option<Res<'w, Player>>,
    body: Option<Res<'w, PlayerBody>>,
    weapon: Option<Res<'w, WeaponAnim>>,
    location: Option<Res<'w, Location>>,
    sky: Option<Res<'w, SkyState>>,
    config: Option<Res<'w, BetterFlashlightConfig>>,
    time: Option<Res<'w, Time<Real>>>,
    play_state: Option<Res<'w, State<PlayState>>>,
    paused: Option<Res<'w, Paused>>,
    save_log: Option<Res<'w, SaveLog>>,
    new_game: Option<Res<'w, NewGame>>,
}

fn sync(
    mut commands: Commands,
    mut flashlight: ResMut<Flashlight>,
    mut runtime: ResMut<FlashlightRuntime>,
    ctx: FlashlightContext,
    mut lights: Query<(Entity, &mut Transform, &mut SpotLight), With<PlayerFlashlight>>,
) {
    let active_cfg = ctx.config.as_deref().filter(|c| c.is_active());

    let Some(cfg) = active_cfg else {
        // Strict vanilla-unchanged gate: when BetterFlashlightNVSE.ini is absent or
        // disabled, keep NVForever's vanilla Pip-Boy spotlight 100% unchanged.
        let Some(player) = ctx.player.as_deref().filter(|_| flashlight.on) else {
            for (e, _, _) in &lights {
                commands.entity(e).despawn();
            }
            return;
        };
        let eye = ctx.body.as_deref().and_then(|b| b.head_eye()).unwrap_or_else(|| player.eye());
        let place = placement(eye, player.heading, player.pitch);
        if let Some((_, mut t, _)) = lights.iter_mut().next() {
            *t = place;
            return;
        }
        commands.spawn((Name::new("Flashlight"), PlayerFlashlight, vanilla_spotlight(), place));
        return;
    };

    // Reset battery charge to 100% on save load or new game.
    if let Some(log) = ctx.save_log.as_deref() {
        let loads = log.0.iter().filter(|line| line.starts_with("Loaded ")).count();
        if loads > runtime.seen_loads {
            runtime.reset_on_load();
        }
        runtime.seen_loads = loads;
    }
    let has_new_game = ctx.new_game.is_some();
    if has_new_game && !runtime.seen_new_game {
        runtime.reset_on_load();
    }
    runtime.seen_new_game = has_new_game;

    let playing = ctx.play_state.as_deref().is_none_or(|s| *s.get() == PlayState::Playing)
        && !ctx.paused.as_deref().is_some_and(|p| p.0);
    let dt = if playing {
        ctx.time.as_deref().map_or(0.0, |t| t.delta_secs().max(0.0))
    } else {
        0.0
    };

    step_battery(&mut runtime, cfg.battery, &mut flashlight, dt);

    if flashlight.on {
        if !runtime.was_on {
            runtime.on_elapsed = 0.0;
            runtime.was_on = true;
        }
        runtime.on_elapsed += dt;
    } else {
        runtime.on_elapsed = 0.0;
        runtime.was_on = false;
    }

    let Some(player) = ctx.player.as_deref().filter(|_| flashlight.on) else {
        for (e, _, _) in &lights {
            commands.entity(e).despawn();
        }
        return;
    };

    let eye = ctx.body.as_deref().and_then(|b| b.head_eye()).unwrap_or_else(|| player.eye());
    let weapon_aim = ctx.body.as_deref().and_then(PlayerBody::weapon_aim);
    let weapon_muzzle = ctx.weapon.as_deref().and_then(|w| w.muzzle);
    let place = configured_placement(eye, player.heading, player.pitch, weapon_aim, weapon_muzzle, cfg);

    let interior = ctx
        .location
        .as_deref()
        .map(|l| l.interior.is_some())
        .or_else(|| ctx.sky.as_deref().map(|s| s.interior.is_some()))
        .unwrap_or(false);
    let desired_spot = configured_spotlight(cfg, interior, runtime.on_elapsed);

    if let Some((_, mut t, mut spot)) = lights.iter_mut().next() {
        *t = place;
        if spotlight_differs(&spot, &desired_spot) {
            *spot = desired_spot;
        }
        return;
    }
    commands.spawn((Name::new("Flashlight"), PlayerFlashlight, desired_spot, place));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_light_is_ahead_of_the_eye_and_looks_where_the_player_looks() {
        // Facing east (+x in game space), level.
        let t = placement([100.0, 0.0, 120.0], std::f32::consts::FRAC_PI_2, 0.0);
        let at = nvf_play::space::to_game(t.translation);
        assert!((at[0] - (100.0 + AHEAD)).abs() < 1e-3 && at[1].abs() < 1e-3, "{at:?}");
        assert!((at[2] - (120.0 - DOWN)).abs() < 1e-3);
        let dir = nvf_play::space::to_game(t.forward().as_vec3());
        assert!(dir[0] > 0.99, "{dir:?}");
    }

    /// CPU mirror of `nvf_spot_display` (`lights/display_shader.rs`) + `GAME_ATTENUATION`
    /// (`lights.rs`) for `PlayerFlashlight` at `light`, illuminating a surface at `target`
    /// (Bevy world space) facing `normal`.
    fn spot_diffuse_display(
        light: &Transform,
        target: Vec3,
        normal: Vec3,
        first_person: bool,
        casts_shadows: bool,
    ) -> [f32; 3] {
        let to_light = light.translation - target;
        let d2 = to_light.length_squared();
        let inv_range_sq = 1.0 / (RANGE * RANGE);
        let attenuation = (1.0 - d2 * inv_range_sq).clamp(0.0, 1.0);
        let dir_to_light = to_light * d2.max(0.0001).sqrt().recip();
        let n_dot_l = normal.normalize().dot(dir_to_light).clamp(0.0, 1.0);
        let spot_dir = light.forward().as_vec3();
        let cd = (-spot_dir).dot(dir_to_light);
        let cos_outer = OUTER_DEG.to_radians().cos();
        let cos_inner = INNER_DEG.to_radians().cos();
        let cone = ((cd - cos_outer) / (cos_inner - cos_outer)).clamp(0.0, 1.0);
        let scale = crate::lights::spot_display_scale(first_person, casts_shadows);
        let factor = BRIGHTNESS * attenuation * n_dot_l * cone * cone * scale;
        COLOUR.map(|c| c * factor)
    }

    /// Final clipped linear diffuse colour (`lights/display_shader.rs` + `GAME_CLIP`).
    fn lit_clipped_linear(albedo: f32, ambient: f32, direct: [f32; 3]) -> [f32; 3] {
        direct.map(|d| nvf_assets::reflection_room::lit_texel(Vec3::splat(albedo), Vec3::splat(ambient + d)).x)
    }

    #[test]
    fn aimed_in_first_person_weapons_and_hand_do_not_wash_out() {
        // Player at origin facing north (+y game, -z Bevy), level, indoors with typical dim ambient.
        let eye = [0.0, 0.0, 120.0];
        let light = placement(eye, 0.0, 0.0);
        let ambient = 0.25;
        // Aimed-in weapon & gloved hand sit directly ahead of the flashlight inside its 14 deg
        // inner core (25 units past the light), facing back toward the light.
        let aimed_pos = light.translation + light.forward().as_vec3() * 25.0;
        let facing_light = -light.forward().as_vec3();
        // Lit world wall in the same beam 300 and 1200 units ahead.
        let near_wall = light.translation + light.forward().as_vec3() * 300.0;
        let far_wall = light.translation + light.forward().as_vec3() * 1200.0;

        let unscaled_direct = spot_diffuse_display(&light, aimed_pos, facing_light, false, true);
        let fp_direct = spot_diffuse_display(&light, aimed_pos, facing_light, true, true);
        let wall_near_direct = spot_diffuse_display(&light, near_wall, facing_light, false, true);
        let wall_far_direct = spot_diffuse_display(&light, far_wall, facing_light, false, true);

        // Reproduce the #3121 failing boundary: without the first-person scale, a bright
        // revolver (Ranger Sequoia silver, albedo 0.55) and gloved hand (0.35) blow out to white.
        assert_eq!(lit_clipped_linear(0.55, ambient, unscaled_direct)[0], 1.0, "unscaled revolver clipped");
        assert_eq!(lit_clipped_linear(0.35, ambient, unscaled_direct)[0], 1.0, "unscaled hand clipped");

        // With the fix, aimed-in revolver (0.55), energy weapon (0.45), gloved hand (0.35),
        // and rifle (0.25) all stay well below white clipping, receive gentle warm fill above
        // flashlight-off, and remain strictly dimmer than the lit world wall ahead.
        for (label, albedo) in [("revolver", 0.55), ("energy", 0.45), ("hand", 0.35), ("rifle", 0.25)] {
            let off = lit_clipped_linear(albedo, ambient, [0.0; 3]);
            let on = lit_clipped_linear(albedo, ambient, fp_direct);
            let wall_near = lit_clipped_linear(albedo, ambient, wall_near_direct);
            let wall_far = lit_clipped_linear(albedo, ambient, wall_far_direct);
            for c in 0..3 {
                assert!(on[c] < 0.75, "{label} channel {c} must not wash out: {on:?}");
                assert!(on[c] > off[c], "{label} channel {c} keeps subtle warm fill over off: {on:?} vs {off:?}");
                assert!(on[c] < wall_near[c] && on[c] < wall_far[c], "{label} channel {c} stays dimmer than lit world wall: {on:?} vs near {wall_near:?} / far {wall_far:?}");
            }
        }

        // Specular on first-person shapes is attenuated as well to stop white highlights.
        let fp_spec = crate::lights::spot_specular_scale(true, true);
        assert_eq!(fp_spec, crate::lights::FIRST_PERSON_FLASHLIGHT_SPECULAR);
        assert!(fp_spec < 0.25);
    }

    #[test]
    fn non_aimed_third_person_world_and_world_spot_controls_hold() {
        let eye = [0.0, 0.0, 120.0];
        let light = placement(eye, 0.0, 0.0);
        let facing_light = -light.forward().as_vec3();

        // Non-aimed hip-held weapon sits off to the right and below the beam axis: weaker than aimed-in.
        let aimed_pos = light.translation + light.forward().as_vec3() * 25.0;
        let hip_pos = light.translation + light.forward().as_vec3() * 25.0 + light.right().as_vec3() * 11.0 - light.up().as_vec3() * 6.0;
        let aimed = spot_diffuse_display(&light, aimed_pos, facing_light, true, true);
        let hip = spot_diffuse_display(&light, hip_pos, facing_light, true, true);
        assert!(hip[0] < aimed[0] && hip[0] >= 0.0, "hip {hip:?} weaker than aimed {aimed:?}");

        // Third-person body/weapon (BODY_LAYER) and world surfaces (!FIRST_PERSON_DEPTH)
        // keep full flashlight diffuse and specular strength.
        assert_eq!(crate::lights::spot_display_scale(false, true), 1.0);
        assert_eq!(crate::lights::spot_specular_scale(false, true), 1.0);
        let world_wall = spot_diffuse_display(&light, light.translation + light.forward().as_vec3() * 200.0, facing_light, false, true);
        assert!(world_wall[0] > BRIGHTNESS * 0.98, "world illumination unchanged: {world_wall:?}");

        // Non-shadow-casting world spot lamps (LIGH SPOT records) still light first-person
        // weapons and hands at full strength.
        assert_eq!(crate::lights::spot_display_scale(true, false), 1.0);
        assert_eq!(crate::lights::spot_specular_scale(true, false), 1.0);
    }

    fn test_player(heading: f32, pitch: f32) -> Player {
        Player {
            heading,
            pitch,
            eye_height: 120.0,
            feet: [100.0, 200.0, 0.0],
            body_eye: None,
            body_view: None,
            placed: None,
        }
    }

    fn test_app(config: Option<BetterFlashlightConfig>) -> App {
        let mut app = App::new();
        app.insert_resource(Flashlight { on: true, toggles: 1 })
            .insert_resource(test_player(0.0, 0.0))
            .insert_resource(Time::<Real>::default())
            .insert_resource(State::new(PlayState::Playing))
            .insert_resource(Paused(false))
            .insert_resource(Location { world: 0x3C, interior: None })
            .insert_resource(SaveLog::default())
            .add_plugins(FlashlightPlugin);
        if let Some(cfg) = config {
            app.insert_resource(cfg);
        }
        app
    }

    fn step_app(app: &mut App, dt_secs: f32) {
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_secs_f32(dt_secs));
        app.update();
    }

    fn current_spot(app: &mut App) -> Option<(Transform, SpotLight)> {
        let mut q = app.world_mut().query_filtered::<(&Transform, &SpotLight), With<PlayerFlashlight>>();
        q.iter(app.world()).next().map(|(t, s)| (*t, *s))
    }

    #[test]
    fn absent_ini_keeps_vanilla_spotlight_bit_for_bit_identical() {
        let mut app = test_app(None);
        step_app(&mut app, 0.016);
        let (t, spot) = current_spot(&mut app).expect("vanilla flashlight spawned");
        let want_t = placement(test_player(0.0, 0.0).eye(), 0.0, 0.0);
        let want_s = vanilla_spotlight();
        assert_eq!(t, want_t);
        assert_eq!(spot.color, want_s.color);
        assert_eq!(spot.intensity, want_s.intensity);
        assert_eq!(spot.range, want_s.range);
        assert_eq!(spot.outer_angle, want_s.outer_angle);
        assert_eq!(spot.inner_angle, want_s.inner_angle);
        assert_eq!(spot.shadow_maps_enabled, want_s.shadow_maps_enabled);
        assert_eq!(spot.shadow_map_near_z, want_s.shadow_map_near_z);
    }

    #[test]
    fn disabled_main_or_flashlight_keeps_vanilla_identity_and_skips_battery() {
        for ini in [
            b"[Main]\nRenderEffects = 0\n[Flashlight]\nEnabled = 1\nDistance = 900\n[Battery]\nEnabled = 1\nDrainRate = 50\n".as_slice(),
            b"[Main]\nRenderEffects = 1\n[Flashlight]\nEnabled = 0\nDistance = 900\n[Battery]\nEnabled = 1\nDrainRate = 50\n".as_slice(),
        ] {
            let mut app = test_app(Some(BetterFlashlightConfig::parse(ini)));
            step_app(&mut app, 1.0);
            let (t, spot) = current_spot(&mut app).expect("vanilla flashlight spawned");
            let want_s = vanilla_spotlight();
            assert_eq!(t, placement(test_player(0.0, 0.0).eye(), 0.0, 0.0));
            assert_eq!(spot.intensity, want_s.intensity);
            assert_eq!(spot.range, want_s.range);
            assert_eq!(spot.outer_angle, want_s.outer_angle);
            assert_eq!(app.world().resource::<FlashlightRuntime>().charge, 100.0);
        }
    }

    #[test]
    fn interior_and_exterior_profiles_switch_dynamically() {
        let cfg = BetterFlashlightConfig::parse(
            b"[Main]\nRenderEffects = 1\n[Flashlight]\nEnabled = 1\nWarmupTime = 0\nFlicker = 0\n[Interior]\nDimmer = 1.0\nIntensity = 0.25\n[Exterior]\nDimmer = 3.0\nIntensity = 0.50\n",
        );
        let mut app = test_app(Some(cfg));

        // Exterior cell (Location.interior == None).
        step_app(&mut app, 0.016);
        let (_, ext_spot) = current_spot(&mut app).expect("exterior spotlight");
        let want_ext = crate::lights::lumens(BRIGHTNESS * 3.0 * 0.50, crate::sky::light_unit());
        assert!((ext_spot.intensity - want_ext).abs() < 1e-3, "{} vs {}", ext_spot.intensity, want_ext);

        // Transition into an interior cell.
        app.world_mut().resource_mut::<Location>().interior = Some(0x0010_0001);
        step_app(&mut app, 0.016);
        let (_, int_spot) = current_spot(&mut app).expect("interior spotlight");
        let want_int = crate::lights::lumens(BRIGHTNESS * 1.0 * 0.25, crate::sky::light_unit());
        assert!((int_spot.intensity - want_int).abs() < 1e-3, "{} vs {}", int_spot.intensity, want_int);
        assert!(ext_spot.intensity > int_spot.intensity);

        // Also verify SkyState::interior fallback when Location resource is absent.
        app.world_mut().remove_resource::<Location>();
        let mut sky = SkyState::default();
        sky.interior = Some(fnv_world::lighting::CellLighting::default());
        app.insert_resource(sky);
        step_app(&mut app, 0.016);
        let (_, sky_int_spot) = current_spot(&mut app).expect("sky interior spotlight");
        assert!((sky_int_spot.intensity - want_int).abs() < 1e-3);
    }

    #[test]
    fn spotlight_maps_angle_spot_size_distance_color_and_rotated_offsets() {
        let cfg = BetterFlashlightConfig::parse(
            b"[Main]\nRenderEffects = 1\n[Flashlight]\nEnabled = 1\nAttachToWeapon = 0\nAngle = 20\nSpotSize = 1.25\nDistance = 1500\nColorR = 1.2\nColorG = 1.4\nColorB = 1.8\nWarmupTime = 0\nFlicker = 0\nOffsetX = 10\nOffsetY = 20\nOffsetZ = -5\n",
        );
        let mut app = test_app(Some(cfg));
        // Face east (heading = pi/2, game +X): local +X (right) is game -Y, local +Y (forward) is game +X, local +Z (up) is game +Z.
        app.world_mut().resource_mut::<Player>().heading = std::f32::consts::FRAC_PI_2;
        step_app(&mut app, 0.016);

        let (t, spot) = current_spot(&mut app).expect("configured spotlight");
        assert_eq!(spot.range, 1500.0);
        assert!((spot.outer_angle.to_degrees() - 25.0).abs() < 1e-4);
        assert!((spot.inner_angle.to_degrees() - (25.0 * INNER_DEG / OUTER_DEG)).abs() < 1e-4);
        assert_eq!(spot.color, crate::lights::display_color([1.2, 1.4, 1.8]));
        // Preserves native shadow flags so #3121 first-person anti-washout scaling stays active.
        assert!(spot.shadow_maps_enabled);
        assert_eq!(spot.shadow_map_near_z, NEAR);

        let base = nvf_play::space::to_game(placement(test_player(std::f32::consts::FRAC_PI_2, 0.0).eye(), std::f32::consts::FRAC_PI_2, 0.0).translation);
        let got = nvf_play::space::to_game(t.translation);
        assert!((got[0] - (base[0] + 20.0)).abs() < 1e-3, "forward +20 along east (+X): {got:?} vs {base:?}");
        assert!((got[1] - (base[1] - 10.0)).abs() < 1e-3, "right +10 facing east (-Y): {got:?} vs {base:?}");
        assert!((got[2] - (base[2] - 5.0)).abs() < 1e-3, "up -5 (-Z): {got:?} vs {base:?}");
    }

    #[test]
    fn attach_to_weapon_uses_weapon_aim_and_falls_back_cleanly() {
        let cfg_weapon = BetterFlashlightConfig::parse(
            b"[Main]\nRenderEffects = 1\n[Flashlight]\nEnabled = 1\nAttachToWeapon = 1\nWarmupTime = 0\nFlicker = 0\n",
        );
        let mut app = test_app(Some(cfg_weapon));

        // 1. With weapon drawn (PlayerBody::weapon_aim), spotlight uses muzzle origin and barrel direction.
        let mut body = PlayerBody::default();
        body.set_weapon_aim_for_test(Some(([150.0, 250.0, 110.0], [1.0, 0.0, 0.0])));
        app.insert_resource(body);
        step_app(&mut app, 0.016);
        let (t_aim, _) = current_spot(&mut app).expect("weapon-attached spotlight");
        let pos_aim = nvf_play::space::to_game(t_aim.translation);
        let dir_aim = nvf_play::space::to_game(t_aim.forward().as_vec3());
        assert!((pos_aim[0] - 150.0).abs() < 1e-3 && (pos_aim[1] - 250.0).abs() < 1e-3 && (pos_aim[2] - 110.0).abs() < 1e-3, "{pos_aim:?}");
        assert!(dir_aim[0] > 0.99, "aligned with barrel +X: {dir_aim:?}");

        // 2. Without PlayerBody::weapon_aim, falls back to WeaponAnim::muzzle + player look rotation.
        app.world_mut().resource_mut::<PlayerBody>().set_weapon_aim_for_test(None);
        app.insert_resource(WeaponAnim { ready: true, muzzle: Some([140.0, 240.0, 105.0]), ..default() });
        step_app(&mut app, 0.016);
        let (t_muzzle, _) = current_spot(&mut app).expect("muzzle-fallback spotlight");
        let pos_muzzle = nvf_play::space::to_game(t_muzzle.translation);
        let dir_muzzle = nvf_play::space::to_game(t_muzzle.forward().as_vec3());
        assert!((pos_muzzle[0] - 140.0).abs() < 1e-3 && (pos_muzzle[1] - 240.0).abs() < 1e-3 && (pos_muzzle[2] - 105.0).abs() < 1e-3);
        assert!(dir_muzzle[1] > 0.99, "player facing north (+Y): {dir_muzzle:?}");

        // 3. When holstered/unarmed (no weapon aim or muzzle), falls back to camera/head-eye placement.
        app.world_mut().resource_mut::<WeaponAnim>().muzzle = None;
        step_app(&mut app, 0.016);
        let (t_unarmed, _) = current_spot(&mut app).expect("unarmed camera fallback");
        assert_eq!(t_unarmed, placement(test_player(0.0, 0.0).eye(), 0.0, 0.0));

        // 4. With AttachToWeapon = 0, ignores weapon aim and stays on camera/head-eye.
        app.world_mut().resource_mut::<PlayerBody>().set_weapon_aim_for_test(Some(([150.0, 250.0, 110.0], [1.0, 0.0, 0.0])));
        app.world_mut().resource_mut::<BetterFlashlightConfig>().attach_to_weapon = false;
        step_app(&mut app, 0.016);
        let (t_cam_only, _) = current_spot(&mut app).expect("camera-only spotlight");
        assert_eq!(t_cam_only, placement(test_player(0.0, 0.0).eye(), 0.0, 0.0));
    }

    #[test]
    fn warmup_ramp_and_flicker_modulate_intensity_cleanly() {
        // Warmup unit curve.
        assert_eq!(warmup_factor(0.0, 0.0), 1.0);
        assert_eq!(warmup_factor(0.0, 0.4), 0.0);
        assert!((warmup_factor(0.2, 0.4) - 0.5).abs() < 1e-5);
        assert_eq!(warmup_factor(0.4, 0.4), 1.0);
        assert_eq!(warmup_factor(2.0, 0.4), 1.0);

        // Flicker unit curve: 0 is steady, >0 is mostly 1.0 with occasional dips, higher = deeper/more often.
        for i in 0..200 {
            assert_eq!(flicker_factor(i as f32 * 0.037, 0.0), 1.0);
        }
        let sample = |strength: f32| {
            let mut dips = 0usize;
            let mut min_val = 1.0f32;
            for i in 0..400 {
                let v = flicker_factor(i as f32 * 0.08 + 0.04, strength);
                assert!((0.15..=1.0).contains(&v), "out of bounds: {v}");
                if v < 0.999 {
                    dips += 1;
                    min_val = min_val.min(v);
                }
            }
            (dips, min_val)
        };
        let (low_dips, low_min) = sample(0.25);
        let (high_dips, high_min) = sample(1.5);
        assert!(low_dips > 0 && low_dips < 60, "sporadic dips at 0.25: {low_dips}");
        assert!(high_dips > low_dips, "higher flicker dips more often: {high_dips} vs {low_dips}");
        assert!(high_min < low_min, "higher flicker dips deeper: {high_min} vs {low_min}");

        // Live ECS warmup progression and reset on off->on toggle.
        let cfg = BetterFlashlightConfig::parse(
            b"[Main]\nRenderEffects = 1\n[Flashlight]\nEnabled = 1\nWarmupTime = 1.0\nFlicker = 0\n[Exterior]\nDimmer = 1.0\nIntensity = 1.0\n",
        );
        let mut app = test_app(Some(cfg));
        step_app(&mut app, 0.25);
        let (_, early) = current_spot(&mut app).expect("warming up");
        step_app(&mut app, 0.25);
        let (_, mid) = current_spot(&mut app).expect("mid warmup");
        step_app(&mut app, 0.75);
        let (_, full) = current_spot(&mut app).expect("warmed up");
        assert!(early.intensity < mid.intensity && mid.intensity < full.intensity);

        // Turning off and back on restarts the warm-up ramp.
        app.world_mut().resource_mut::<Flashlight>().on = false;
        step_app(&mut app, 0.1);
        assert!(current_spot(&mut app).is_none());
        app.world_mut().resource_mut::<Flashlight>().on = true;
        step_app(&mut app, 0.1);
        let (_, restarted) = current_spot(&mut app).expect("restarted warmup");
        assert!(restarted.intensity < mid.intensity);
    }

    #[test]
    fn battery_drains_locks_out_recharges_pauses_and_resets_on_load_and_new_game() {
        let cfg = BetterFlashlightConfig::parse(
            b"[Main]\nRenderEffects = 1\n[Flashlight]\nEnabled = 1\nWarmupTime = 0\nFlicker = 0\n[Battery]\nEnabled = 1\nDrainRate = 25\nRechargeRate = 10\nRechargeThreshold = 15\n",
        );
        let mut app = test_app(Some(cfg));

        // 1. Drain while on and playing: 2.0s * 25/s = 50 remaining.
        step_app(&mut app, 2.0);
        assert!((app.world().resource::<FlashlightRuntime>().charge - 50.0).abs() < 1e-4);
        assert!(app.world().resource::<Flashlight>().on);

        // 2. Paused / menu state stops drain.
        app.world_mut().resource_mut::<Paused>().0 = true;
        step_app(&mut app, 3.0);
        assert!((app.world().resource::<FlashlightRuntime>().charge - 50.0).abs() < 1e-4);
        app.world_mut().resource_mut::<Paused>().0 = false;
        app.insert_resource(State::new(PlayState::Menu));
        step_app(&mut app, 3.0);
        assert!((app.world().resource::<FlashlightRuntime>().charge - 50.0).abs() < 1e-4);
        app.insert_resource(State::new(PlayState::Playing));

        // 3. Full depletion turns flashlight off and enters RechargeThreshold lockout.
        step_app(&mut app, 2.5);
        assert_eq!(app.world().resource::<FlashlightRuntime>().charge, 0.0);
        assert!(app.world().resource::<FlashlightRuntime>().depleted_lockout);
        assert!(!app.world().resource::<Flashlight>().on);
        assert!(current_spot(&mut app).is_none());

        // 4. Attempting to turn on before reaching RechargeThreshold (15) is blocked and keeps recharging.
        app.world_mut().resource_mut::<Flashlight>().on = true;
        step_app(&mut app, 1.0); // recharges to 10 (< 15)
        assert!(!app.world().resource::<Flashlight>().on, "blocked during lockout");
        assert!(app.world().resource::<FlashlightRuntime>().depleted_lockout);
        assert!((app.world().resource::<FlashlightRuntime>().charge - 10.0).abs() < 1e-4);
        assert!(current_spot(&mut app).is_none());

        // 5. Recharging past RechargeThreshold (15) clears lockout and allows turning back on.
        step_app(&mut app, 0.6); // 10 + 6 = 16 >= 15
        assert!(!app.world().resource::<FlashlightRuntime>().depleted_lockout);
        assert!((app.world().resource::<FlashlightRuntime>().charge - 16.0).abs() < 1e-4);
        app.world_mut().resource_mut::<Flashlight>().on = true;
        step_app(&mut app, 0.1); // drains 2.5 -> 13.5, stays on!
        assert!(app.world().resource::<Flashlight>().on);
        assert!(current_spot(&mut app).is_some());

        // 6. Save load resets charge to 100% and clears lockout.
        app.world_mut().resource_mut::<FlashlightRuntime>().charge = 0.0;
        app.world_mut().resource_mut::<FlashlightRuntime>().depleted_lockout = true;
        app.world_mut().resource_mut::<Flashlight>().on = false;
        app.world_mut().resource_mut::<SaveLog>().0.push("Loaded quicksave.nvfsave".into());
        step_app(&mut app, 0.0);
        assert_eq!(app.world().resource::<FlashlightRuntime>().charge, 100.0);
        assert!(!app.world().resource::<FlashlightRuntime>().depleted_lockout);

        // 7. New game also resets charge to 100% and clears lockout.
        app.world_mut().resource_mut::<FlashlightRuntime>().charge = 12.0;
        app.world_mut().resource_mut::<FlashlightRuntime>().depleted_lockout = true;
        app.insert_resource(NewGame);
        step_app(&mut app, 0.0);
        assert_eq!(app.world().resource::<FlashlightRuntime>().charge, 100.0);
        assert!(!app.world().resource::<FlashlightRuntime>().depleted_lockout);
    }
}
