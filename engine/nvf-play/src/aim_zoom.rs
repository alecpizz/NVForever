//! The view zooms in while the player aims down the sights.
//!
//! - **What zooms**: Block held (`PlayerControl::aiming`) with a gun drawn
//!   (`WeaponAnim::ready`) whose WEAP DNAM Sight FOV is above 0 narrows the player
//!   camera's field of view to that Sight FOV [documented: GECK wiki Weapons, Sight FOV is
//!   the field of view down the sights, 0 "essentially disables iron sights";
//!   docs/spec-notes/combat-data.md §1.1]. The zoom is the weapon's own: a lower Sight FOV
//!   zooms in further (the owner's playtest, 2026-10-16: the Varmint Rifle zooms in more
//!   than the Sturdy Caravan Shotgun).
//! - **Units** [hypothesis]: the Sight FOV is read like the FOV setting, the horizontal
//!   angle of a 4:3 view with the vertical angle kept at other aspects
//!   ([`vertical_fov`], docs/spec-notes/camera.md §3).
//! - **Timing** [design]: the field of view moves between the two over [`ZOOM_SECONDS`],
//!   smoothed at both ends; letting go of Block zooms back out at the same pace.
//! - **Reloads and jams** [design, #1642]: a reload or a jam holds the sights down
//!   (`Combat::player_sights_held_down`): the view zooms out (a scope goes at once) as
//!   its clip begins and zooms back in after it while Block is still held.
//! - The free camera, menus and melee weapons (which block) do not zoom; both points of
//!   view do. Whatever else sets the field of view (the console's SetCameraFOV,
//!   chargen) sets the unzoomed angle: a change this module did not make is taken as
//!   the new base. A conversation's zoom (`crate::dialogue_zoom`) lies over the angle and
//!   is not one.
//! - **Scopes** (docs/spec-notes/weapon-scopes.md §2): a scoped gun (WEAP Has Scope, or
//!   Scope from Mod with an Increase Zoom mod on) at full zoom in first person puts its
//!   scope up ([`Scope`]); nvforever `scope_view.rs` draws the MOD3 picture, the night
//!   vision and the sway, the arms rig and the crosshair go meanwhile.
//! - **The look while aiming** [design: owner 2026-11-15]: the Controls tab's Aim
//!   Sensitivity scales the mouse's and the right stick's turn by the zoom's own eased share
//!   ([`AimZoom::share`], `crate::player::aim_scale`): the chosen speed at the full zoom
//!   (down the sights, or a scope up), the unzoomed look with no zoom, in step between.

use bevy::prelude::*;

use crate::combat::{Combat, WeaponAnim};
use crate::control::PlayerControl;
use crate::free_cam::{vertical_fov, FreeCam};
use crate::{PlayState, PlayerCamera, PlaySet};
use fnv_sim::FormId;

/// Seconds from the unzoomed view to the full zoom and back [design: not captured].
pub const ZOOM_SECONDS: f32 = 0.25;

/// How far the view is zoomed in, and the angles it moves between.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct AimZoom {
    /// 0 unzoomed, 1 the weapon's Sight FOV.
    pub blend: f32,
    /// The view is zooming in (Block held with a gun that has a Sight FOV): true iron
    /// sights (nvforever `iron_sights.rs`) bring the gun up at their own pace meanwhile.
    pub aimed: bool,
    /// The weapon's Sight FOV as a vertical angle (radians), held while zooming out.
    pub sight: Option<f32>,
    /// The unzoomed vertical field of view (radians).
    base: Option<f32>,
    /// The share of the way to the Sight FOV the main view goes: 1, or [`LENS_MAIN_SHARE`]
    /// while a 3D scope shows the zoom in its lens. Held while zooming out.
    main: f32,
    /// The vertical field of view this module last set.
    applied: Option<f32>,
    /// With movement inertia: the zoom's progress along the settling curve, the same curve
    /// and times as the aim clips' cross-over and the true iron sights (`crate::inertia`).
    settled: crate::ease::Settled,
    /// The zoom ran along the settling curve last frame (Smooth weapon animations on).
    settled_on: bool,
    /// The full zoom has been reported (smoke tests read the line).
    told: bool,
    /// How long the zoom has been full, seconds (the Scope Delay tweak).
    full_for: f32,
    /// The wheel's share of a scope's Sight FOV under the Adjustable Scope Zoom tweak
    /// (0: none set, as 1); back to none when the scope goes down.
    scope_zoom: f32,
}

/// With a 3D scope (`Scope3d`) the main view goes only this share of the way to the
/// Sight FOV; the lens shows the full zoom [design: test version, docs/design/scope-3d-lens.md §4.1].
pub const LENS_MAIN_SHARE: f32 = 0.2;

/// What a wheel notch multiplies a scope's field of view by under the Adjustable Scope
/// Zoom tweak (up divides) [design: docs/design/tweaks.md].
const SCOPE_WHEEL_STEP: f32 = 1.1;

impl AimZoom {
    /// The unzoomed vertical field of view (radians), once known.
    pub fn base(&self) -> Option<f32> {
        self.base
    }

    /// How far the view is zoomed for the aim, eased as the field of view moves
    /// ([`zoom_share`] of [`AimZoom::blend`]): 0 unzoomed, 1 at the full zoom. The look's
    /// Aim Sensitivity follows it (`crate::player::aim_scale`).
    pub fn share(&self) -> f32 {
        zoom_share(self.blend)
    }
}

/// `blend` (0–1) eased in and out: the share of the way from the unzoomed field of view to
/// the zoomed one ([`zoomed_fov`]).
pub fn zoom_share(blend: f32) -> f32 {
    let b = blend.clamp(0.0, 1.0);
    b * b * (3.0 - 2.0 * b)
}

/// The field of view (vertical, radians) for `base`, the weapon's `sight` and `blend`
/// (0–1), eased in and out.
pub fn zoomed_fov(base: f32, sight: f32, blend: f32) -> f32 {
    base + (sight - base) * zoom_share(blend)
}

/// The `blend` at which the straight zoom has taken the view `share` of the way (by
/// halving; [`zoom_share`] only rises).
fn zoom_share_inv(share: f32) -> f32 {
    let share = share.clamp(0.0, 1.0);
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let mid = 0.5 * (lo + hi);
        if zoom_share(mid) < share {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// The gun in hand is lined up with the eye down its sights (true iron sights, set by
/// nvforever `iron_sights.rs`): the crosshair is not drawn meanwhile (`crate::reticle`)
/// [design: the owner, 2026-10-20].
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct TrueSights(pub bool);


/// A night-vision scope is up now (whatever the player chose), for the flashlight's key
/// under the player's Night Vision Toggle tweak (`crate::flashlight`).
static NIGHT_SCOPE_UP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// The player's choice under that tweak: 0 none yet, 1 on, 2 off.
static NIGHT_VISION_CHOICE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Whether a night-vision scope is up.
pub fn night_scope_up() -> bool {
    NIGHT_SCOPE_UP.load(std::sync::atomic::Ordering::Relaxed)
}

/// Whether the player wants the scope's night vision: as last switched, else on unless
/// the tweak's Off Until Switched On is set.
pub fn night_vision_wanted(off_first: bool) -> bool {
    match NIGHT_VISION_CHOICE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => true,
        2 => false,
        _ => !off_first,
    }
}

/// Switch the scope's night vision (the tweak); whether it is on now.
pub fn switch_night_vision(off_first: bool) -> bool {
    let on = !night_vision_wanted(off_first);
    NIGHT_VISION_CHOICE.store(if on { 1 } else { 2 }, std::sync::atomic::Ordering::Relaxed);
    on
}

/// The weapon's scope is up (weapon-scopes.md §2): a scoped gun aimed in first person and
/// zoomed all the way to its Sight FOV.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct Scope {
    pub on: bool,
    /// The weapon, its scope's picture (WEAP MOD3) and whether it gives night vision, while
    /// on.
    pub weapon: Option<FormId>,
    pub model: Option<String>,
    pub night_vision: bool,
    /// The Sight FOV (degrees) zoomed to.
    pub sight_fov: f32,
    /// Shown in the first-person weapon's lens (a 3D scope, `Scope3d`) instead of the
    /// full-screen picture: the arms and the weapon stay drawn.
    pub lens_3d: bool,
}

/// 3D scopes (docs/design/scope-3d-lens.md, a test version): the zoomed world is drawn in
/// the lens of the first-person weapon's scope instead of the full-screen picture.
/// `on` is the settings switch; `lens` is set by nvforever while the first-person weapon
/// model has a known lens (the others keep the picture).
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct Scope3d {
    pub on: bool,
    pub lens: bool,
}

/// A 3D scope's sway (nvforever `scope_view.rs`, weapon-scopes.md §5): the gun, its lens
/// and the shots turn by it (radians, added to the heading and the pitch) while the view
/// stays still. Zero otherwise; the full-screen picture sways the view itself.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct LensSway {
    pub yaw: f32,
    pub pitch: f32,
}

impl LensSway {
    /// The direction (game space) a look along `heading`, `pitch` turns to.
    pub fn look_dir(&self, heading: f32, pitch: f32) -> [f32; 3] {
        crate::space::look_dir(heading + self.yaw, pitch + self.pitch)
    }
}

impl AimZoom {
    /// Zoomed all the way down the sights: `base` unzoomed, `sight` on the camera (tests).
    #[cfg(test)]
    pub(crate) fn down_the_sights(base: f32, sight: f32) -> AimZoom {
        AimZoom { blend: 1.0, aimed: true, sight: Some(sight), base: Some(base), main: 1.0, applied: Some(sight), told: true, ..AimZoom::default() }
    }
}

pub struct AimZoomPlugin;

impl Plugin for AimZoomPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AimZoom>().init_resource::<TrueSights>().init_resource::<Scope>().init_resource::<Scope3d>().init_resource::<LensSway>().add_systems(Update, zoom.in_set(PlaySet::Sync));
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn zoom(
    time: Res<Time<Real>>,
    state: Res<State<PlayState>>,
    control: Res<PlayerControl>,
    anim: Res<WeaponAnim>,
    combat: Option<Res<Combat>>,
    (free, photo): (Option<Res<FreeCam>>, Option<Res<crate::photo_mode::PhotoMode>>),
    mut z: ResMut<AimZoom>,
    mut scope: ResMut<Scope>,
    lens: Res<Scope3d>,
    talk: Option<Res<crate::dialogue_zoom::DialogueZoom>>,
    inertia: Option<Res<crate::inertia::MovementInertia>>,
    mut cams: Query<&mut Projection, With<PlayerCamera>>,
    tweaks: Option<Res<crate::tweaks::TweakSettings>>,
    scroll: Option<Res<bevy::input::mouse::AccumulatedMouseScroll>>,
) {
    let Ok(mut proj) = cams.single_mut() else { return };
    // Read first: a write every frame would mark the projection changed.
    let Projection::Perspective(p) = &*proj else { return };
    // Someone else set the field of view: that is the unzoomed angle now. What a
    // conversation's zoom laid over it is not.
    let seen = talk.as_deref().map_or(p.fov, |t| t.underlying(p.fov));
    if z.applied != Some(seen) {
        z.base = Some(seen);
        z.applied = None;
    }
    let playing = *state.get() == PlayState::Playing && !free.is_some_and(|f| f.on) && !photo.as_deref().is_some_and(|p| p.on);
    // A reload or a jam holds the sights down (`combat::sights_held_down`): the zoom and a
    // scope go, and come back after it while Block is still held.
    let sight = combat.as_deref().filter(|c| !c.player_sights_held_down()).and_then(Combat::sight_fov).filter(|_| playing && control.aiming && control.weapon_out && anim.ready);
    // The player's tweak: the wheel zooms a scope in (up) and out, from half to double
    // its Sight FOV; set again each time the scope comes up [design: docs/design/tweaks.md].
    if !scope.on {
        z.scope_zoom = 0.0;
    } else if tweaks.as_ref().is_some_and(|t| t.scope_wheel_zoom) {
        let notches = scroll.as_ref().map_or(0.0, |s| s.delta.y);
        if notches != 0.0 && !crate::quick_loot::holds_input() {
            z.scope_zoom = (if z.scope_zoom > 0.0 { z.scope_zoom } else { 1.0 } * SCOPE_WHEEL_STEP.powf(-notches)).clamp(0.5, 2.0);
        }
    }
    let wheel = if z.scope_zoom > 0.0 { z.scope_zoom } else { 1.0 };
    let sight = sight.map(|(w, deg)| (w, if scope.on { deg * wheel } else { deg }));
    if let Some((_, deg)) = sight {
        z.sight = Some(vertical_fov(deg));
    }
    z.aimed = sight.is_some();
    // The player's tweak times a scope's zoom its own way [design: docs/design/tweaks.md]:
    // a share of whichever time the zoom takes (the game's, or Weapon Handling's).
    let scope_time = if scope.on { tweaks.as_ref().map_or(1.0, |t| t.scope_zoom_time).max(0.1) } else { 1.0 };
    let was = z.blend;
    // With smooth weapon animations the zoom runs along the settling curve over the aim's
    // times; without, straight over ZOOM_SECONDS and eased by `zoomed_fov`.
    // Smooth weapon animations switched on or off part-way through a zoom: the other way
    // goes on from where the view is, instead of from the start (review, 2026-11-15).
    // Photo mode holds the zoom where it opened: it closes on the view it opened on.
    let held = photo.as_deref().is_some_and(|p| p.on);
    let settled = if held {
        z.settled_on.then(|| z.settled.value())
    } else {
        inertia.as_deref().filter(|i| i.smooth_hands()).map(|i| {
            if !z.settled_on {
                let at = zoom_share(z.blend);
                z.settled.place(at, sight.is_some());
            }
            // Weapon Handling's pace (heavier weapons come up slower), else the game's.
            let v = z.settled.step(sight.is_some(), time.delta_secs().min(0.25), i.aim_seconds(true) * scope_time, i.aim_seconds(false) * scope_time);
            // `blend` stays the progress the rest reads: 1 only when fully in, 0 only when out.
            z.blend = if z.settled.is_up() { 1.0 } else if z.settled.is_down() { 0.0 } else { v.clamp(1e-4, 1.0 - 1e-4) };
            v
        })
    };
    if !held && settled.is_none() {
        if z.settled_on {
            z.blend = zoom_share_inv(z.settled.value());
        }
        // Weapon Handling's pace for the straight zoom too, else the game's.
        let seconds = inertia.as_deref().filter(|i| i.handles()).map_or(ZOOM_SECONDS, |i| i.aim_seconds(sight.is_some())) * scope_time;
        let step = time.delta_secs().min(0.25) / seconds;
        z.blend = if sight.is_some() { (z.blend + step).min(1.0) } else { (z.blend - step).max(0.0) };
    }
    if !held {
        z.settled_on = settled.is_some();
    }
    // The scope: up at full zoom in first person, down as soon as the aim is let go.
    let scoped = sight.filter(|_| !control.third_person_view()).and_then(|(w, deg)| {
        let stats = combat.as_deref()?.player_stats(w)?;
        // The player's Night Vision Toggle tweak can switch the scope's night vision off
        // [design: docs/design/tweaks.md].
        let capable = stats.night_vision_scope();
        NIGHT_SCOPE_UP.store(capable, std::sync::atomic::Ordering::Relaxed);
        let wanted = !tweaks.as_ref().is_some_and(|t| t.night_vision_toggle) || night_vision_wanted(tweaks.as_ref().is_some_and(|t| t.night_vision_off_first));
        Some(Scope { on: true, weapon: Some(w), model: stats.scope_model().map(str::to_string), night_vision: capable && wanted, sight_fov: deg, lens_3d: lens.on && lens.lens })
    });
    let scoped = scoped.filter(|s| s.model.is_some());
    // A 3D scope zooms the main view only a little, from the start of the aim.
    if let Some(s) = &scoped {
        z.main = if s.lens_3d { LENS_MAIN_SHARE } else { 1.0 };
    } else if tweaks.as_ref().is_some_and(|t| t.no_sight_zoom) {
        // The player's tweak: iron sights do not zoom [design: docs/design/tweaks.md].
        z.main = 0.0;
    } else if sight.is_some() || z.main == 0.0 {
        z.main = 1.0;
    }
    // The player's tweak holds the scope back a while after the zoom is full [design: docs/design/tweaks.md].
    z.full_for = if z.blend >= 1.0 { z.full_for + time.delta_secs().min(0.25) } else { 0.0 };
    let delay = tweaks.as_ref().map_or(0.0, |t| t.scope_delay_ms) / 1000.0;
    let scoped = scoped.filter(|_| z.blend >= 1.0 && (delay <= 0.0 || z.full_for >= delay)).unwrap_or_default();
    if scoped != *scope {
        if scoped.on && (scope.weapon != scoped.weapon || !scope.on) {
            let edid = combat.as_deref().and_then(|c| c.forms.weapons.get(&scoped.weapon?)).and_then(|ws| ws.edid.clone()).unwrap_or_else(|| "?".into());
            info!(
                "scope: {edid} ({}) up: {}, sight FOV {:.1}°, {}",
                scoped.weapon.map_or_else(String::new, |w| w.to_string()),
                scoped.model.as_deref().unwrap_or("?"),
                scoped.sight_fov,
                if scoped.night_vision { "a night-vision scope" } else { "a plain scope" }
            );
            if scoped.lens_3d {
                info!("scope: shown in the weapon's lens (3D scope), the main view zoomed {LENS_MAIN_SHARE} of the way");
            }
        } else if !scoped.on && scope.on {
            info!("scope: down");
        }
        *scope = scoped;
    }
    if was == 0.0 && z.blend == 0.0 {
        return;
    }
    let (Some(base), Some(target)) = (z.base, z.sight) else { return };
    let goal = base + (target - base) * z.main;
    let fov = match settled {
        Some(v) => base + (goal - base) * v,
        None => zoomed_fov(base, goal, z.blend),
    };
    if let Projection::Perspective(p) = &mut *proj {
        p.fov = fov;
    }
    z.applied = Some(fov);
    if z.blend == 1.0 && !z.told {
        z.told = true;
        if let Some((w, deg)) = sight {
            let edid = combat.as_deref().and_then(|c| c.forms.weapons.get(&w)).and_then(|ws| ws.edid.as_deref()).unwrap_or("?");
            info!("aim zoom: {edid} ({w}) sight FOV {deg:.1}° (vertical {:.1}°, unzoomed {:.1}°)", target.to_degrees(), base.to_degrees());
        }
    } else if z.blend == 0.0 {
        z.told = false;
        z.sight = None;
        z.main = 1.0;
        info!("aim zoom: back to the unzoomed view (vertical {:.1}°)", base.to_degrees());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_zoom_eases_from_the_base_to_the_sights() {
        let (base, sight) = (vertical_fov(75.0), vertical_fov(55.0));
        assert_eq!(zoomed_fov(base, sight, 0.0), base);
        assert!((zoomed_fov(base, sight, 1.0) - sight).abs() < 1e-6);
        assert!((zoomed_fov(base, sight, 0.5) - (base + sight) / 2.0).abs() < 1e-6);
        // Slow at the ends: the first tenth covers less than a tenth of the way.
        assert!(base - zoomed_fov(base, sight, 0.1) < (base - sight) * 0.1);
    }

    #[test]
    fn a_zoom_changing_curves_part_way_goes_on_from_where_it_is() {
        // Straight to the settling curve: the same share of the way, either direction.
        for (blend, up) in [(0.3, true), (0.8, false), (1.0, true), (0.0, false)] {
            let mut s = crate::ease::Settled::default();
            s.place(zoom_share(blend), up);
            assert!((s.value() - zoom_share(blend)).abs() < 1e-4, "{blend} {up}: {}", s.value());
        }
        // And back: the straight zoom's blend for the share the settling curve had.
        for share in [0.0, 0.2, 0.5, 0.93, 1.0] {
            assert!((zoom_share(zoom_share_inv(share)) - share).abs() < 1e-4, "{share}");
        }
    }

    #[test]
    fn a_lower_sight_fov_zooms_further() {
        assert!(vertical_fov(40.0) < vertical_fov(60.0));
    }
}
