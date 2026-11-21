//! Modern Feel's camera kick (`ModernFeel::camera_kick`; docs/design/modern-feel-kick.md)
//! [design: every constant an engineering choice, tuned by eye; none of it is the
//! original's]: each of the player's shots punches the view, pitch up, a random turn aside
//! and a small roll, on stiff critically damped springs that come back to rest. A heavy
//! shot also widens the field of view for a moment.
//!
//! - **Only the view**: where the player aims is the game's (`crate::combat`'s own kick and
//!   spread); the punch turns the camera after it is placed (`crate::control::place_camera`,
//!   with the explosion shake, `crate::shake`) and is gone a quarter of a second later. In
//!   first person nvforever's player body turns the arms rig with it
//!   ([`CameraKick::arms_rotation`]), so the gun stays where it is on the screen and the
//!   world moves; Advanced Recoil's kick of the gun (nvforever `view_sway.rs`) and the head
//!   bob's nudge back (`head_bob.rs`) lie on top of it.
//! - **By the weapon** ([`kick_for`]): the damage of a shot, the weight, the kind (a
//!   launcher or a shotgun is heavy, an energy weapon lighter), automatics smaller by their
//!   fire rate.
//! - **Calmer**: down the sights (by the zoom's share), with a scope up, sneaking; third
//!   person at half and no field-of-view punch; none in VATS, a kill camera, the free
//!   camera or a menu.
//! - **The field of view** is widened late in the frame (`PostUpdate`, before the cameras'
//!   projections are worked out) and put back at the next frame's start, so the zoom and
//!   the other modules that set it never see the punch.
//!
//! A `camera kick: weapon …` line for the first shots of each weapon (smoke runs).

use crate::combat::ShotFired;
use crate::control::PlayerControl;
use crate::ease::Spring;
use crate::modern_feel::ModernFeel;
use crate::{PlaySet, PlayState, PlayerCamera};
use bevy::prelude::*;
use fnv_combat::data::WeaponStats;
use fnv_sim::FormId;

/// A shot of strength 1 (a .44 pistol about) peaks at these, degrees: up, aside at most
/// (at random either way), rolled at most (at random either way).
const PITCH_DEG: f32 = 1.3;
const YAW_DEG: f32 = 0.35;
const ROLL_DEG: f32 = 0.9;
/// The most shown however the shots add up, degrees (soft limits).
const MAX_PITCH_DEG: f32 = 4.5;
const MAX_YAW_DEG: f32 = 2.0;
const MAX_ROLL_DEG: f32 = 3.0;
/// The springs' rate (1/s): a punch peaks at 1/ω and is 95 % back in about 4.7/ω.
const OMEGA: f32 = 19.0;
/// An automatic's springs are stiffer, so a burst buzzes instead of climbing.
const AUTO_OMEGA: f32 = 27.0;
/// The field-of-view punch: from this strength it starts, full at the second, this many
/// degrees wider at full; its spring's rate (1/s: peaks in 33 ms, back in about 0.15 s).
const FOV_FROM: f32 = 1.15;
const FOV_FULL: f32 = 2.0;
const FOV_DEG: f32 = 4.0;
const FOV_OMEGA: f32 = 30.0;
/// The shares: fully down the sights, a scope up, sneaking, third person.
const SIGHTS: f32 = 0.45;
const SCOPE: f32 = 0.3;
const SNEAK: f32 = 0.75;
const THIRD: f32 = 0.5;

/// One shot's punch: degrees up, aside at most, rolled at most, wider field of view; and
/// whether the springs are an automatic's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Kick {
    pub strength: f32,
    pub pitch: f32,
    pub yaw: f32,
    pub roll: f32,
    pub fov: f32,
    pub automatic: bool,
}

/// A shot's punch for weapon `w`, `explosive` when its projectile explodes (a grenade or
/// missile launcher, whatever its animation type) (module notes) [design].
pub fn kick_for(w: &WeaponStats, explosive: bool) -> Kick {
    let kind = w.anim_type.as_deref().unwrap_or("");
    let pellets = w.projectile_count.max(1);
    // DATA's damage is the whole shot's, shared among a shotgun's pellets.
    let damage = w.base_damage.max(1) as f32;
    // The damage of a shot, as a square root so a rifle is not ten pistols.
    let mut s = (damage / 40.0).sqrt();
    // Weight steadies nothing here: a heavy gun is a big gun, it kicks more.
    s *= 1.0 + 0.25 * crate::ease::smoothstep(4.0, 20.0, w.weight);
    if kind.starts_with("Launcher") || explosive {
        s = s.max(1.4) * 1.3;
    } else if pellets > 1 {
        s *= 1.35;
    }
    if kind.contains("Energy") {
        s *= 0.75;
    }
    if kind.contains("(2 Hand)") && !kind.starts_with("Launcher") {
        s *= 1.1;
    }
    let automatic = w.automatic && w.fire_rate > 0.0;
    if automatic {
        // Smaller the faster it fires: a 10-a-second rifle about a third of its single shot.
        s *= (4.0 / w.fire_rate.max(4.0)).sqrt() * 0.6;
    }
    let s = s.clamp(0.2, 2.4);
    let fov = if automatic { 0.0 } else { FOV_DEG * crate::ease::smoothstep(FOV_FROM, FOV_FULL, s) };
    Kick { strength: s, pitch: PITCH_DEG * s, yaw: YAW_DEG * s, roll: ROLL_DEG * s, fov, automatic }
}

/// A limit that eases in: `x` up to about `max`, never past it.
fn soft(x: f32, max: f32) -> f32 {
    if max <= 0.0 {
        0.0
    } else {
        max * (x / max).tanh()
    }
}

/// The punch under way.
#[derive(Resource, Default, Debug)]
pub struct CameraKick {
    /// Radians: up, to the left, rolled (anticlockwise as seen); the field of view wider.
    pitch: Spring,
    yaw: Spring,
    roll: Spring,
    fov: Spring,
    omega: f32,
    /// This frame's turn of the view (radians: up, left, roll) and field-of-view widening.
    now: [f32; 4],
    /// The turn applies to the first-person arms rig too.
    first_person: bool,
    shots: u32,
    /// The field of view as it was before the punch and as the punch left it.
    restore: Option<(f32, f32)>,
    logged: Vec<FormId>,
    /// For the exit line: shots kicked, the most pitch and field of view shown (degrees).
    pub kicked: u32,
    pub peak_pitch: f32,
    pub peak_fov: f32,
}

impl CameraKick {
    /// The turn of the first-person arms rig that keeps it with the kicked view, in the
    /// rig's frame (x right, y ahead, z up), about the eye; identity when none.
    pub fn arms_rotation(&self) -> Quat {
        if !self.first_person || self.now[..3].iter().all(|v| *v == 0.0) {
            return Quat::IDENTITY;
        }
        let [pitch, yaw, roll, _] = self.now;
        Quat::from_rotation_z(yaw) * Quat::from_rotation_x(pitch) * Quat::from_rotation_y(-roll)
    }

    /// The view's turn now (radians: up, left, roll).
    pub fn turn(&self) -> [f32; 3] {
        [self.now[0], self.now[1], self.now[2]]
    }

    fn rest(&mut self) {
        for s in [&mut self.pitch, &mut self.yaw, &mut self.roll, &mut self.fov] {
            *s = Spring::REST;
        }
        self.now = [0.0; 4];
    }
}

/// Take the player's shots and step the springs: before the player body poses the arms rig,
/// so the rig and the camera turn by the same this frame.
#[allow(clippy::too_many_arguments)]
fn step(
    time: Res<Time>,
    state: Res<State<PlayState>>,
    feel: Option<Res<ModernFeel>>,
    control: Res<PlayerControl>,
    combat: Option<Res<crate::combat::Combat>>,
    zoom: Option<Res<crate::aim_zoom::AimZoom>>,
    scope: Option<Res<crate::aim_zoom::Scope>>,
    calm: (Option<Res<crate::vats_mode::VatsMode>>, Option<Res<crate::kill_cam::KillCam>>, Option<Res<crate::free_cam::FreeCam>>),
    mut shots: MessageReader<ShotFired>,
    mut kick: ResMut<CameraKick>,
) {
    let on = feel.as_deref().is_some_and(ModernFeel::camera_kick);
    let (vats, kill, free) = calm;
    let held = vats.as_deref().is_some_and(|v| v.active()) || kill.as_deref().is_some_and(|k| k.active()) || free.as_deref().is_some_and(|f| f.on);
    let third = control.third_person_view();
    if !on || held {
        shots.clear();
        kick.rest();
        return;
    }
    if *state.get() != PlayState::Playing {
        return;
    }
    let share = {
        let sights = zoom.as_deref().map_or(0.0, |z| z.share());
        let scoped = scope.as_deref().is_some_and(|s| s.on);
        (1.0 - (1.0 - SIGHTS) * sights) * if scoped { SCOPE } else { 1.0 } * if control.sneak { SNEAK } else { 1.0 } * if third { THIRD } else { 1.0 }
    };
    for s in shots.read().filter(|s| s.shooter == FormId::PLAYER_REF) {
        let Some(w) = combat.as_deref().and_then(|c| c.forms.weapons.get(&s.weapon)) else { continue };
        let kind = w.anim_type.as_deref().unwrap_or("");
        if fnv_sim::forms::THROWN_ANIM_TYPES.contains(&kind) || kind.starts_with("Land Mine") || kind.starts_with("Mine Drop") {
            continue;
        }
        let explosive = combat.as_deref().and_then(|c| c.forms.projectiles.get(&s.projectile)).is_some_and(|p| p.explosion.is_some());
        let k = kick_for(w, explosive);
        kick.shots = kick.shots.wrapping_add(1);
        let n = kick.shots as f32;
        // Two numbers from the shot count, -1 to 1 (as `view_sway`'s aside).
        let r = |seed: f32| ((n * 12.9898 + seed).sin() * 43_758.547).fract() * 2.0 - 1.0;
        let omega = if k.automatic { AUTO_OMEGA } else { OMEGA };
        kick.omega = omega;
        // From rest a spring set moving at v peaks at v / (ω e).
        let v = omega * std::f32::consts::E * share;
        let rad = std::f32::consts::PI / 180.0;
        kick.pitch.rate += k.pitch * rad * v;
        kick.yaw.rate += k.yaw * rad * r(1.7) * v;
        // The roll leans the way the gun turns, mostly.
        let roll = 0.5 * r(5.3) - 0.5 * r(1.7).signum();
        kick.roll.rate += k.roll * rad * roll.clamp(-1.0, 1.0) * v;
        if !third {
            kick.fov.rate += k.fov * rad * FOV_OMEGA * std::f32::consts::E * share;
        }
        kick.kicked += 1;
        if kick.logged.len() < 32 && !kick.logged.contains(&s.weapon) {
            kick.logged.push(s.weapon);
            info!(
                "camera kick: weapon {} ({:08X}, {kind}, damage {}, weight {:.1}{}) strength {:.2}: pitch {:.2} deg, yaw up to {:.2} deg, roll up to {:.2} deg, fov +{:.1} deg, share {share:.2}{}",
                w.edid.as_deref().unwrap_or("?"),
                s.weapon.0,
                w.base_damage,
                w.weight,
                if k.automatic { format!(", automatic {:.1}/s", w.fire_rate) } else { String::new() },
                k.strength,
                k.pitch * share,
                k.yaw * share,
                k.roll * share,
                if third { 0.0 } else { k.fov * share },
                if third { ", third person" } else { "" }
            );
        }
    }
    let dt = time.delta_secs();
    let kick = &mut *kick;
    let omega = if kick.omega > 0.0 { kick.omega } else { OMEGA };
    kick.pitch.step(0.0, omega, dt);
    kick.yaw.step(0.0, omega, dt);
    kick.roll.step(0.0, omega, dt);
    kick.fov.step(0.0, FOV_OMEGA, dt);
    for s in [&mut kick.pitch, &mut kick.yaw, &mut kick.roll, &mut kick.fov] {
        s.snap(0.0, 1e-5, 1e-4);
    }
    let rad = std::f32::consts::PI / 180.0;
    kick.now = [
        soft(kick.pitch.value.max(-0.2 * MAX_PITCH_DEG * rad), MAX_PITCH_DEG * rad),
        soft(kick.yaw.value, MAX_YAW_DEG * rad),
        soft(kick.roll.value, MAX_ROLL_DEG * rad),
        soft(kick.fov.value.max(0.0), FOV_DEG * 1.5 * rad),
    ];
    kick.first_person = !third;
    kick.peak_pitch = kick.peak_pitch.max(kick.now[0] / rad);
    kick.peak_fov = kick.peak_fov.max(kick.now[3] / rad);
}

/// Turn the placed camera by the punch (after `place_camera`, as the shake).
fn apply(state: Res<State<PlayState>>, kick: Res<CameraKick>, mut cams: Query<&mut Transform, With<PlayerCamera>>) {
    if *state.get() != PlayState::Playing {
        return;
    }
    let [pitch, yaw, roll] = kick.turn();
    if pitch == 0.0 && yaw == 0.0 && roll == 0.0 {
        return;
    }
    let Ok(mut t) = cams.single_mut() else { return };
    t.rotation *= Quat::from_euler(EulerRot::YXZ, yaw, pitch, roll);
}

/// Widen the field of view by the punch, late in the frame.
fn widen(mut kick: ResMut<CameraKick>, mut cams: Query<&mut Projection, With<PlayerCamera>>) {
    let wider = kick.now[3];
    if wider <= 0.0 {
        return;
    }
    let Ok(mut proj) = cams.single_mut() else { return };
    let Projection::Perspective(p) = &mut *proj else { return };
    let was = p.fov;
    p.fov = (was + wider).min(3.0);
    kick.restore = Some((was, p.fov));
}

/// Put the field of view back at the frame's start, unless something else set it since.
fn narrow(mut kick: ResMut<CameraKick>, mut cams: Query<&mut Projection, With<PlayerCamera>>) {
    let Some((was, wrote)) = kick.restore.take() else { return };
    let Ok(mut proj) = cams.single_mut() else { return };
    if let Projection::Perspective(p) = &mut *proj {
        if p.fov == wrote {
            p.fov = was;
        }
    }
}

fn report(mut exit: MessageReader<AppExit>, kick: Res<CameraKick>) {
    if exit.read().next().is_some() && kick.kicked > 0 {
        info!("camera kick summary: {} shots kicked, most pitch {:.2} deg, most fov +{:.2} deg", kick.kicked, kick.peak_pitch, kick.peak_fov);
    }
}

pub struct CameraKickPlugin;

impl Plugin for CameraKickPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraKick>()
            .add_message::<ShotFired>()
            .add_systems(First, narrow)
            .add_systems(Update, step.in_set(PlaySet::Physics).before(crate::player::BodySet).before(crate::control::place_camera))
            .add_systems(
                Update,
                apply.in_set(PlaySet::Physics).after(crate::control::place_camera).after(crate::script_camera::script_camera).after(crate::free_cam::fly),
            )
            .add_systems(PostUpdate, widen.before(bevy::camera::CameraUpdateSystems))
            .add_systems(Last, report);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn weapon(kind: &str, damage: i32, weight: f32) -> WeaponStats {
        WeaponStats { anim_type: Some(kind.into()), base_damage: damage, weight, projectile_count: 1, ..Default::default() }
    }

    #[test]
    fn heavier_guns_kick_harder_and_automatics_less() {
        let pistol = kick_for(&weapon("Pistol - Ballistic (1 Hand)", 18, 3.0), false);
        let magnum = kick_for(&weapon("Pistol - Ballistic (1 Hand)", 40, 4.0), false);
        let sniper = kick_for(&weapon("Rifle - Ballistic (2 Hand)", 100, 20.0), false);
        let smg = kick_for(&WeaponStats { automatic: true, fire_rate: 10.0, ..weapon("Rifle - Automatic (2 Hand)", 12, 4.0) }, false);
        assert!(pistol.pitch < magnum.pitch && magnum.pitch < sniper.pitch);
        assert!(smg.pitch < pistol.pitch && smg.fov == 0.0);
        assert!(sniper.fov > 0.0 && pistol.fov == 0.0);
    }
}
