//! Soft edges and contact shadows for the lamp shadows (`light_shadows.rs`; the settings
//! row ALC shadows; docs/design/graphics-enhanced.md §3.17 "Soft edges and contact
//! shadows"). Both only on Quality [engineering choice: each costs per pixel and per
//! light; Performance keeps the hard edges of the camera's shadow filter].
//!
//! - **Soft edges**: every casting point light (`CastingLight`) takes percentage-closer soft
//!   shadows (`soft_shadows_enabled`, the `experimental_pbr_pcss` feature for its samplers)
//!   with a light size of [`SOFT_RADIUS`] (an explosion's [`BLAST_RADIUS`]): sharp where a
//!   shape meets the surface its shadow falls on, softer further away, as a lamp's are. The
//!   size goes to the renderer apart from the light's `radius`
//!   (`HoldLightShadowMaps::soft_size`), so the lighting does not change. The filter
//!   is the patched renderer's own (bevy_pbr shadow_sampling.wesl
//!   `nvf_sample_shadow_cubemap_soft`, engine/vendor/README.md "ALCS soft shadows"): the
//!   blocker search and the penumbra in world units, each tap compared against the
//!   fragment's own surface. Bevy's takes the light's size as a plain number, a fraction of
//!   a metre in mind: with the game's units its search swept most of the cube and every lit
//!   surface shadowed itself in stripes (playtest 2026-11-24). Spot lamps keep the camera's
//!   shadow filter (Bevy's soft shadows for them have the same fault).
//! - **Contact shadows**: the world camera takes Bevy's `ContactShadows` (a ray march
//!   along the depth buffer towards each light, [`CONTACT`]), and every casting placed light,
//!   grass fire and attached light within the sun's shadow distance has
//!   `contact_shadows_enabled`: small shadows where things touch (a bottle on a bar, a foot
//!   on a floor) that the lamps' 512-texel maps miss. Only the lights that cast take them
//!   (since 2026-11-25; until then those past the cap did too, in full whatever the lamps'
//!   shadows weighed). The camera then needs a depth prepass (`DepthPrepass`, which
//!   `ContactShadows` requires), as FSR and DLSS already do. Only while a temporal
//!   method is on (TAA, FSR or DLSS: the camera jitters): without one the march's
//!   per-pixel noise speckles walls and ceilings [observed 2026-11-18, General Store
//!   pictures]. With FSR or DLSS the picture is drawn below its output size into the
//!   top-left of full-size textures (`MainPassResolutionOverride`); Bevy's march read the
//!   whole depth texture as the view and threw hard-edged false shadows that changed as
//!   the camera moved [observed 2026-11-18: playtest videos, a walk along the
//!   Prospector Saloon's porch]. The vendored march now reads the main pass's viewport
//!   only (bevy_pbr raymarch.wesl `depth_uv_scale`, engine/vendor/README.md).
//! - The sun keeps neither: its soft edges are the cascades' filter, and Bevy's contact
//!   shadows on it speckled near surfaces at a grazing angle (§4, tried and dropped).

use bevy::camera::visibility::VisibilitySystems;
use bevy::core_pipeline::prepass::DepthPrepass;
use bevy::light::SimulationLightSystems;
use bevy::pbr::ContactShadows;
use bevy::ecs::entity::EntityHashMap;
use bevy::pbr::HoldLightShadowMaps;
use bevy::prelude::*;
use bevy::render::camera::TemporalJitter;
use bevy::transform::TransformSystems;

use crate::light_shadows::{CastingLight, LightShadows, ShadowSource, SourceKind};
use crate::lights::SessionLight;

/// How far from a light its fixture is looked for (game units): the game hangs its lights
/// under their lamps, half of them within 89 units and most within 160 (light_shadows.rs
/// `LightFixture`, the census there).
pub const FIXTURE_REACH: f32 = 160.0;

/// A lamp's light size from its fixture: this share of the fixture shape's size (the mean
/// of its box's two larger half extents), within [`SOFT_MIN`]..[`SOFT_MAX`] [engineering
/// choice, 2026-11-24, [design: "soft-shadow width per lamp type/size"]: a candle or a bare
/// bulb throws crisp shadows, a shade or a strip light softer ones]. The sizes are a bulb's
/// and a shade's, 3 to 9 cm [2026-11-25]: at the first ones (0.8 of the fixture, 4 to 40
/// units, 24 before that) a lamp was a ball half a metre and more across, and the game
/// hangs its lamps close over what they light, so a character's shadow on the floor was
/// blurred away, where Performance drew it whole (playtest 2026-11-24: "character shadows
/// look better on Performance"; compared in Doc Mitchell's house at 2, 6, 12, 24 and the
/// fixtures' sizes).
pub const FIXTURE_SHARE: f32 = 0.2;
pub const SOFT_MIN: f32 = 2.0;
pub const SOFT_MAX: f32 = 6.0;

/// The light sizes of the other sources (game units): a muzzle flash is a point, a grass
/// fire a patch of flames [engineering choice].
pub const FLASH_RADIUS: f32 = 2.0;
pub const FIRE_RADIUS: f32 = 6.0;

/// A lamp's light size ([`FIXTURE_SHARE`]; [`SOFT_RADIUS`] without a fixture: the fill and
/// ambient lights a room is lit with) and whether a fixture gave it. `fixtures`: the
/// fixture shapes with their centre's distance from the light and their world half
/// extents; the nearest within [`FIXTURE_REACH`] is the lamp's.
pub fn lamp_soft_size(fixtures: impl Iterator<Item = (f32, Vec3)>) -> (f32, bool) {
    let nearest = fixtures.filter(|(d, _)| *d <= FIXTURE_REACH).min_by(|a, b| a.0.total_cmp(&b.0));
    match nearest {
        Some((_, half)) => {
            let h = half.abs();
            // The two larger half extents: a strip light is long, a cord thin.
            let emitter = (h.x + h.y + h.z - h.min_element()) * 0.5;
            ((emitter * FIXTURE_SHARE).clamp(SOFT_MIN, SOFT_MAX), true)
        }
        None => (SOFT_RADIUS, false),
    }
}

/// The light size of a lamp's soft shadow without a fixture's (game units, about 6 cm)
/// [engineering choice; 24 until 2026-11-25, see [`FIXTURE_SHARE`]].
pub const SOFT_RADIUS: f32 = 4.0;

/// An explosion's light size (game units: a fireball) [engineering choice].
pub const BLAST_RADIUS: f32 = 8.0;

/// The contact shadows' ray march: steps, the assumed thickness of what the depth buffer
/// shows, and the ray's length (game units: 4 and 24, about 6 and 34 cm) [engineering
/// choice: short and thin, unlike the sun's dropped 40 and 12, so a surface seen at a
/// grazing angle does not shadow itself].
pub const CONTACT: ContactShadows = ContactShadows { linear_steps: 8, thickness: 4.0, length: 24.0 };

/// `NVF_LIGHT_SHADOW_SOFT=<size>` / `NVF_LIGHT_SHADOW_CONTACT=0` (test aids): another light size for
/// the soft edges (0: none), no contact shadows.
fn soft_aid() -> Option<f32> {
    static V: std::sync::OnceLock<Option<f32>> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("NVF_LIGHT_SHADOW_SOFT").ok().and_then(|v| v.trim().parse::<f32>().ok()))
}

fn contact_aid_off() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("NVF_LIGHT_SHADOW_CONTACT").is_ok_and(|v| v.trim() == "0"))
}

/// [`CONTACT`], or `NVF_LIGHT_SHADOW_CONTACT_MARCH=<steps>,<thickness>,<length>` (a test aid).
fn contact_settings() -> ContactShadows {
    std::env::var("NVF_LIGHT_SHADOW_CONTACT_MARCH")
        .ok()
        .and_then(|v| {
            let p: Vec<f32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            (p.len() == 3).then(|| ContactShadows { linear_steps: p[0] as u32, thickness: p[1], length: p[2] })
        })
        .unwrap_or(CONTACT)
}

/// Whether the choice (`LightShadows::choice`) is Quality.
pub fn detailed(choice: usize) -> bool {
    choice == 2
}

/// The soft-shadow size of a source by what it is; `None` for a placed light and a light a
/// script attached, whose size is their fixture's ([`lamp_soft_size`]).
pub fn soft_radius(kind: Option<SourceKind>) -> Option<f32> {
    match kind {
        Some(SourceKind::Explosion) => Some(BLAST_RADIUS),
        Some(SourceKind::MuzzleFlash) => Some(FLASH_RADIUS),
        Some(SourceKind::GrassFire) => Some(FIRE_RADIUS),
        Some(SourceKind::Attached) | None => None,
    }
}

pub struct LightShadowDetailPlugin;

impl Plugin for LightShadowDetailPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            (soften, contact)
                .after(TransformSystems::Propagate)
                .before(VisibilitySystems::VisibilityPropagate)
                .before(SimulationLightSystems::AssignLightsToClusters)
                // With the row Off nothing of the ALC shadows runs; for a few frames after
                // it is switched off these give the soft edges and contact shadows back.
                .run_if(crate::light_shadows::live),
        );
    }
}

type Lit<'a> = (Entity, &'a GlobalTransform, Option<&'a PointLight>, Has<CastingLight>, Option<&'a ShadowSource>);

/// A fixture shape new this frame, or one whose bounds are (a shape has them only once its
/// mesh is loaded).
type NewFixtures = (With<crate::light_shadows::LightFixture>, Or<(Added<crate::light_shadows::LightFixture>, Added<bevy::camera::primitives::Aabb>)>);

/// The lamps' fixture shapes ([`crate::light_shadows::LightFixture`]).
type Fixtures<'a> = (&'a GlobalTransform, &'a bevy::camera::primitives::Aabb);

/// The lights these settings touch: the placed ones and the other sources.
type Lamps = Or<(With<SessionLight>, With<ShadowSource>)>;

type Cameras<'a> = (Entity, &'a GlobalTransform, Has<ContactShadows>, Has<TemporalJitter>);

/// The soft sizes of the casting point lights on Quality, and of those whose held maps still
/// fade out (a slot kept, `HoldLightShadowMaps`); none otherwise. Each lamp's own, from its
/// fixture ([`lamp_soft_size`]; found once and kept, looked for again when fixtures appear
/// while it has none). They go to the renderer in `HoldLightShadowMaps::soft_size`, apart
/// from the lights' `radius`: until 2026-11-24 the radius carried the size, and the lighting
/// read it too (a lamp's highlight grew and dimmed, and popped when the size was switched).
/// Spot lamps get none: they keep the camera's shadow filter.
fn soften(
    settings: Res<LightShadows>,
    mut hold: ResMut<HoldLightShadowMaps>,
    lights: Query<Lit, Lamps>,
    fixtures: Query<Fixtures, With<crate::light_shadows::LightFixture>>,
    new_fixtures: Query<(), NewFixtures>,
    mut known: Local<EntityHashMap<(f32, bool)>>,
    mut logged: Local<(usize, usize)>,
) {
    let _timed = crate::light_shadows::Timed::start(2);
    let mut sizes: EntityHashMap<f32> = EntityHashMap::default();
    if detailed(settings.choice) {
        // Fixtures that came (a cell loaded after its lights; a shape has its bounds only once
        // its mesh is loaded, a frame or more after it is spawned): the lamps without one look
        // again.
        if !new_fixtures.is_empty() {
            known.retain(|_, (_, fixture)| *fixture);
        }
        for (e, place, point, casting, source) in &lights {
            if point.is_none() {
                continue;
            }
            if !(casting || hold.slots.contains_key(&e) || hold.far_slots.contains_key(&e)) {
                continue;
            }
            let size = soft_aid().or_else(|| soft_radius(source.map(|s| s.0))).unwrap_or_else(|| {
                known
                    .entry(e)
                    .or_insert_with(|| {
                        let at = place.translation();
                        lamp_soft_size(fixtures.iter().map(|(t, aabb)| {
                            let (scale, _, _) = t.to_scale_rotation_translation();
                            (t.transform_point(Vec3::from(aabb.center)).distance(at), Vec3::from(aabb.half_extents) * scale.abs())
                        }))
                    })
                    .0
            });
            if size > 0.0 {
                sizes.insert(e, size);
            }
        }
        // Lamps gone: forgotten once there are many.
        if known.len() > 512 {
            known.retain(|e, _| sizes.contains_key(e));
        }
        let with_fixture = known.values().filter(|(_, fixture)| *fixture).count();
        if *logged != (known.len(), with_fixture) && !known.is_empty() {
            *logged = (known.len(), with_fixture);
            let (lo, hi) = known.values().fold((f32::MAX, 0.0f32), |(lo, hi), (s, _)| (lo.min(*s), hi.max(*s)));
            info!("light shadows: light sizes of {} lamps from {lo:.0} to {hi:.0} units, {with_fixture} by their fixture, the rest {SOFT_RADIUS}", known.len());
        }
    }
    if hold.soft_size != sizes {
        hold.soft_size = sizes;
    }
}
type Contacted<'a> = (&'a GlobalTransform, Option<&'a mut PointLight>, Option<&'a mut SpotLight>, Option<&'a ShadowSource>, Has<CastingLight>);

/// The camera's contact shadows on Quality, and the lights that take them.
#[allow(clippy::too_many_arguments)]
fn contact(
    mut commands: Commands,
    settings: Res<LightShadows>,
    sky: Option<Res<crate::sky::SkyState>>,
    cameras: Query<Cameras, crate::fly::WorldCamera>,
    mut lights: Query<Contacted, Lamps>,
    mut logged: Local<Option<(bool, usize)>>,
) {
    let _timed = crate::light_shadows::Timed::start(3);
    let mut on = false;
    let mut at = None;
    for (e, t, has, temporal) in &cameras {
        at = Some(t.translation());
        // Only with a temporal method (TAA, FSR, DLSS: the camera jitters), which smooths
        // the march's per-pixel noise; without one it speckles the walls (§4). Under FSR and
        // DLSS the vendored march reads the depth of the main pass's viewport only
        // (raymarch.wesl `depth_uv_scale`): before that it threw hard-edged false shadows
        // (playtest videos 2026-11-18, the Prospector Saloon's porch).
        on = detailed(settings.choice) && !contact_aid_off() && temporal;
        if on && !has {
            commands.entity(e).insert((contact_settings(), DepthPrepass));
        } else if !on && has {
            // The depth prepass stays: FSR and DLSS may hold it too, and it is harmless.
            commands.entity(e).remove::<ContactShadows>();
        }
    }
    let reach = sky.as_ref().map_or(0.0, |s| s.shadow_distance * s.shadow_reach.max(1.0));
    let mut count = 0;
    for (t, point, spot, source, casting) in &mut lights {
        // Not the transients: a tenth of a second, and their maps are drawn anyway. And only
        // the lights that cast: they carry the shadow's fade (its strength, its fade in and
        // out), which the march follows; on a light past the cap it stayed at full strength
        // whatever the row said, went out the moment the light took a slot and came back at
        // full when it left (2026-11-25).
        let kind_ok = casting && source.is_none_or(|s| !s.0.transient());
        let range = point.as_ref().map(|p| p.range).or(spot.as_ref().map(|s| s.range)).unwrap_or(0.0);
        let near = at.is_some_and(|a| (t.translation().distance(a) - range).max(0.0) <= reach);
        let want = on && kind_ok && near;
        count += usize::from(want);
        if let Some(mut p) = point {
            if p.contact_shadows_enabled != want {
                p.contact_shadows_enabled = want;
            }
        }
        if let Some(mut s) = spot {
            if s.contact_shadows_enabled != want {
                s.contact_shadows_enabled = want;
            }
        }
    }
    let soft = detailed(settings.choice);
    if *logged != Some((on, count)) && (soft || logged.is_some()) {
        *logged = Some((on, count));
        match (soft, on) {
            (true, true) => info!(
                "light shadows: soft edges (light size {SOFT_RADIUS}, explosions {BLAST_RADIUS}) and contact shadows on {count} lights ({} steps, thickness {}, length {})",
                CONTACT.linear_steps, CONTACT.thickness, CONTACT.length
            ),
            (true, false) => info!("light shadows: soft edges (light size {SOFT_RADIUS}, explosions {BLAST_RADIUS}); no contact shadows (only with TAA, FSR or DLSS)"),
            _ => info!("light shadows: no soft edges or contact shadows"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_quality_softens_and_explosions_are_larger() {
        assert!(detailed(2));
        assert!(!detailed(1) && !detailed(0));
        assert_eq!(soft_radius(None), None, "a placed light: its fixture's");
        assert_eq!(soft_radius(Some(SourceKind::Attached)), None);
        assert_eq!(soft_radius(Some(SourceKind::MuzzleFlash)), Some(FLASH_RADIUS));
        assert_eq!(soft_radius(Some(SourceKind::GrassFire)), Some(FIRE_RADIUS));
        assert_eq!(soft_radius(Some(SourceKind::Explosion)), Some(BLAST_RADIUS));
        const { assert!(FLASH_RADIUS < SOFT_RADIUS && SOFT_RADIUS < BLAST_RADIUS) };
        let c = contact_settings();
        assert!(c.length < 40.0 && c.thickness < 12.0, "shorter and thinner than the sun's dropped try");
    }

    #[test]
    fn a_lamps_light_size_follows_its_fixture() {
        let lamp = |fixtures: &[(f32, Vec3)]| lamp_soft_size(fixtures.iter().copied());
        // A candle (a few units across): the smallest, crisp shadows.
        assert_eq!(lamp(&[(20.0, Vec3::new(2.0, 2.0, 6.0))]), (SOFT_MIN, true));
        // A bare bulb, a shade, a chandelier: wider each.
        let (bulb, shade, chandelier) = (lamp(&[(60.0, Vec3::splat(12.0))]).0, lamp(&[(60.0, Vec3::new(20.0, 20.0, 12.0))]).0, lamp(&[(60.0, Vec3::new(60.0, 60.0, 40.0))]).0);
        assert!(bulb < shade && shade < chandelier && chandelier == SOFT_MAX, "{bulb} {shade} {chandelier}");
        assert!((shade - 4.0).abs() < 1e-4);
        // A strip light is long: the two larger half extents count; a cord is thin.
        assert!((lamp(&[(60.0, Vec3::new(40.0, 4.0, 4.0))]).0 - 4.4).abs() < 1e-4);
        // The nearest fixture within reach is the lamp's.
        assert_eq!(lamp(&[(120.0, Vec3::splat(40.0)), (50.0, Vec3::splat(15.0))]), (3.0, true));
        // None in reach (a fill light): the one size.
        assert_eq!(lamp(&[(FIXTURE_REACH + 1.0, Vec3::splat(10.0))]), (SOFT_RADIUS, false));
        assert_eq!(lamp(&[]), (SOFT_RADIUS, false));
        const { assert!(SOFT_MIN <= SOFT_RADIUS && SOFT_RADIUS <= SOFT_MAX) };
    }
}
