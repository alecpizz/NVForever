//! Photo mode's lights drawn (`nvf_play::photo_lights` holds their settings): each is one
//! of the engine's dynamic lights, a Bevy point or spot light with the game's attenuation
//! (`lights.rs`), its colour in display values and its brightness in the game's display
//! units ([`crate::lights::lumens`]), as a placed LIGH's. They live only while photo mode
//! is open and are despawned when it closes.
//!
//! Shadows: Bevy gives each shadow-casting point light a cube map and each spot light one
//! map, so [`nvf_play::photo_lights::MAX_LIGHTS`] (4) shadowed lights are well inside what
//! the renderer handles [observed: bevy_pbr 0.20.1 clusters up to 256 lights when storage
//! buffers are available]. A shadowed photo light carries the sun's render layers
//! (`sky::SHADOW_LAYER`, `sky::BODY_LAYER`), so the player's body casts its shadow too
//! (lights.rs's note on spot lights).
//!
//! The markers (a ball in the light's colour, its beam's direction and, for a spot, the
//! cone's rim) are gizmos drawn through walls while the Lights tab is up or a light is
//! edited, and never in the photo (`PhotoMode::markers_shown`).

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use nvf_play::photo_lights::{Kind, PhotoLight, PhotoLightSource};
use nvf_play::photo_mode::PhotoMode;
use nvf_play::space;

/// Gizmos for the markers, drawn in front of everything.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct PhotoLightGizmos;

/// Shadow near plane, game units.
const NEAR: f32 = 4.0;
/// The share of the spot's half angle that is fully lit (a soft rim).
const INNER: f32 = 0.75;
/// Marker sizes, game units.
const BALL: f32 = 10.0;
const BEAM: f32 = 120.0;

pub struct PhotoLightsPlugin;

impl Plugin for PhotoLightsPlugin {
    fn build(&self, app: &mut App) {
        app.init_gizmo_group::<PhotoLightGizmos>()
            .add_systems(Startup, through_walls)
            .add_systems(PostUpdate, (sync, markers).chain().before(bevy::transform::TransformSystems::Propagate));
    }
}

fn through_walls(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<PhotoLightGizmos>();
    config.depth_bias = -1.0;
    config.line.width = 2.0;
}

/// The light's rotation (Bevy space; a spot shines down its -Z, as the camera looks).
fn rotation(l: &PhotoLight, cam_heading: f32, cam_pitch: f32) -> Quat {
    let (h, p) = l.aim(cam_heading, cam_pitch);
    space::look_rotation(h, p)
}

/// The spot's half angle in radians, kept short of a hemisphere.
pub fn half_angle(width_deg: f32) -> f32 {
    (width_deg * 0.5).clamp(1.0, 85.0).to_radians()
}

fn point(l: &PhotoLight) -> PointLight {
    PointLight {
        color: crate::lights::display_color(l.rgb()),
        intensity: crate::lights::lumens(l.brightness, crate::sky::light_unit()),
        range: l.range,
        shadow_maps_enabled: l.shadows,
        shadow_map_near_z: NEAR,
        ..default()
    }
}

fn spot(l: &PhotoLight) -> SpotLight {
    let a = half_angle(l.width);
    SpotLight {
        color: crate::lights::display_color(l.rgb()),
        intensity: crate::lights::lumens(l.brightness, crate::sky::light_unit()),
        range: l.range,
        outer_angle: a,
        inner_angle: a * INNER,
        shadow_maps_enabled: l.shadows,
        shadow_map_near_z: NEAR,
        ..default()
    }
}

/// A drawn photo light's parts.
type Drawn<'a> = (Entity, &'a PhotoLightSource, &'a mut Transform, Option<&'a mut PointLight>, Option<&'a mut SpotLight>, &'a mut Visibility);

/// Spawn, update and despawn the drawn lights to match photo mode's.
fn sync(mut commands: Commands, photo: Option<Res<PhotoMode>>, mut drawn: Query<Drawn>) {
    let Some(photo) = photo else { return };
    let lights = if photo.on { photo.lights.lights.as_slice() } else { &[] };
    let (_, ch, cp) = photo.camera_view();
    let mut have = [false; nvf_play::photo_lights::MAX_LIGHTS];
    for (e, src, mut t, pl, sl, mut vis) in &mut drawn {
        let Some(l) = lights.get(src.0) else {
            commands.entity(e).despawn();
            continue;
        };
        let want_spot = l.kind == Kind::Spot;
        // A light that changed kind is spawned again.
        if want_spot != sl.is_some() || want_spot == pl.is_some() {
            commands.entity(e).despawn();
            continue;
        }
        have[src.0] = true;
        let rot = rotation(l, ch, cp);
        let at = space::to_bevy(l.at);
        if t.translation != at || t.rotation != rot {
            (t.translation, t.rotation) = (at, rot);
        }
        vis.set_if_neq(if l.on { Visibility::Visible } else { Visibility::Hidden });
        if let Some(mut p) = pl {
            let want = point(l);
            if p.color != want.color || p.intensity != want.intensity || p.range != want.range || p.shadow_maps_enabled != want.shadow_maps_enabled {
                *p = want;
            }
        }
        if let Some(mut s) = sl {
            let want = spot(l);
            if s.color != want.color || s.intensity != want.intensity || s.range != want.range || s.outer_angle != want.outer_angle || s.shadow_maps_enabled != want.shadow_maps_enabled {
                *s = want;
            }
        }
    }
    for (i, l) in lights.iter().enumerate() {
        if have[i] {
            continue;
        }
        let t = Transform { translation: space::to_bevy(l.at), rotation: rotation(l, ch, cp), ..default() };
        let vis = if l.on { Visibility::Visible } else { Visibility::Hidden };
        let layers = RenderLayers::from_layers(&[0, crate::sky::SHADOW_LAYER, crate::sky::BODY_LAYER]);
        let mut e = commands.spawn((Name::new("Photo light"), PhotoLightSource(i), t, vis, layers));
        match l.kind {
            Kind::Point => e.insert(point(l)),
            Kind::Spot => e.insert(spot(l)),
        };
        info!("photo lights: light {} drawn ({:?}, hue {:.0}, saturation {:.0}%, width {:.0}, direction {:+.0} tilt {:+.0}, shadows {})", i + 1, l.kind, l.hue, l.saturation * 100.0, l.width, l.yaw, l.pitch, if l.shadows { "on" } else { "off" });
    }
}

/// The markers: never in the photo.
fn markers(photo: Option<Res<PhotoMode>>, mut gizmos: Gizmos<PhotoLightGizmos>) {
    let Some(photo) = photo else { return };
    if !photo.markers_shown() {
        return;
    }
    let (_, ch, cp) = photo.camera_view();
    let s = &photo.lights;
    for (i, l) in s.lights.iter().enumerate() {
        let chosen = i == s.current;
        let [r, g, b] = l.rgb();
        let col = if l.on { Color::srgb(r, g, b) } else { Color::srgb(0.4, 0.4, 0.4) };
        let at = space::to_bevy(l.at);
        let rot = rotation(l, ch, cp);
        let fwd = rot * Vec3::NEG_Z;
        gizmos.sphere(Isometry3d::from_translation(at), if chosen { BALL * 1.5 } else { BALL }, col);
        let beam = if chosen { BEAM * 2.0 } else { BEAM };
        gizmos.arrow(at, at + fwd * beam, col);
        // The cone's rim for a chosen spot: its width at the beam's end.
        if chosen && l.kind == Kind::Spot {
            let radius = beam * half_angle(l.width).tan();
            let rim_at = at + fwd * beam;
            let iso = Isometry3d::new(rim_at, rot);
            gizmos.circle(iso, radius.min(beam * 6.0), col);
            for k in 0..4 {
                let a = k as f32 * std::f32::consts::FRAC_PI_2;
                let edge = rim_at + rot * Vec3::new(a.cos(), a.sin(), 0.0) * radius.min(beam * 6.0);
                gizmos.line(at, edge, col.with_alpha(0.6));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spot_points_where_the_player_aimed_it_not_at_the_camera() {
        let mut l = PhotoLight::new([0.0; 3], 0.0, 0.0);
        (l.yaw, l.pitch) = (90.0, -30.0);
        let fwd = rotation(&l, 2.0, 0.5) * Vec3::NEG_Z;
        let want = space::to_bevy(space::look_dir(90f32.to_radians(), (-30f32).to_radians()));
        assert!((fwd - want).length() < 1e-4, "{fwd} vs {want}: the camera's view does not turn a free light");
    }

    #[test]
    fn width_is_the_full_cone() {
        let mut l = PhotoLight::new([0.0; 3], 0.0, 0.0);
        l.width = 40.0;
        let s = spot(&l);
        assert!((s.outer_angle - 20f32.to_radians()).abs() < 1e-5);
        assert!(s.inner_angle < s.outer_angle);
        assert!(half_angle(170.0) < std::f32::consts::FRAC_PI_2, "a flood stays short of a hemisphere");
        l.shadows = false;
        assert!(!spot(&l).shadow_maps_enabled && !point(&l).shadow_maps_enabled);
    }
}
