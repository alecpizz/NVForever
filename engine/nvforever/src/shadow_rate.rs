//! The sun shadows' modes: the settings page's Sun shadows row, Off, Performance or Quality
//! ([`SUN_SHADOW_CHOICES`]; `--sun-shadows[=quality|performance]`). Performance trades some
//! of the shadows' precision for frame time. [engineering choices throughout; the original
//! casts no sun shadows from the world at all, docs/spec-notes/world.md §6]
//!
//! Performance:
//! - **The sun's shadow maps are drawn every other frame**, the farthest cascade's every
//!   fourth ([`PERFORMANCE_SCHEDULE`], [`hold_cascades`]). On the frames between, the
//!   lights keep the cascades of the frame that drew them, so the maps and the matrices
//!   they are read with still belong together, and the renderer is told to leave the maps
//!   alone (`bevy::pbr::HoldDirectionalShadowMaps`, a patched bevy_pbr:
//!   engine/vendor/README.md). The shadow of a moving thing then moves at half the frame
//!   rate (a quarter in the farthest cascade), and on a fast turn the edge of the view can
//!   lie outside a held cascade for one frame (up to three in the farthest; unshadowed).
//! - **The sun's shadows are split over 3 cascades** (`sky::CascadeLayout::Performance`;
//!   2 until #2843) instead of Quality's 4 ([`set_performance`]): one
//!   shadow view less, and shadows near the camera coarser (the cascades end at 1,500 and
//!   4,500 of 8,000 units instead of 700, 2,000 and 5,000 with 2048² maps:
//!   `sky::shadow_cascade_layout`).
//! - **The first-person arms have no shade light of their own** (player_body.rs
//!   `shade_light`): another shadow view less; the arms take the sun's own shadows.
//! - **A tree casts a shadow only within [`TREE_SHADOW_NEAR`] of the camera**
//!   ([`far_trees_cast_no_shadow`]): a SpeedTree tree is thousands of leaf cards and a
//!   finely divided trunk in every cascade it falls in.
//!
//! Quality (the default) draws the sun's 4 cascades on a staggered schedule
//! ([`STAGGERED_SCHEDULE`], #2063; 3 cascades until #2843): the nearest every frame, the
//! second every other frame, the third and the fourth every fourth frame, on different
//! frames, each held cascade kept with the matrices of the frame that drew it, as above.
//! A moving thing's shadow in the far cascades moves at a half or a quarter of the frame
//! rate, and on a fast turn the edge of the view can lie outside a held cascade for up to
//! three frames. Test aid `NVF_PLAY_TEST_SHADOW_STAGGER=0` draws
//! every cascade every frame.
//!
//! A held cascade is kept as the sun's cascades stand after they are snapped to a grid
//! anchored near the camera and stretched toward the sun (shadow_anchor.rs
//! `cascade_edits`, which schedules [`hold_cascades`] after those two), so it is given back
//! exactly as its map was drawn. Without the anchoring the sun light's turn with the clock
//! made the shadows shimmer, strongest on the frames a held cascade is drawn again
//! (docs/design/frame-rate-audit.md §11).
//!
//! While multiple active 3D views consume directional shadows, all cascades draw
//! every frame: the renderer reuses view-local shadow array layers between cameras.
//! Holding the last camera's maps would pair them with another camera's matrices.
//! The water mirror (reflection.rs) is the exception: on a schedule it is given the
//! main camera's cascades every frame, so both views draw the same maps into the same
//! layers and a held layer is right for either (#2674). Its reflected shadows then
//! cover what the main camera's cascades cover; past that the reflection is sunlit.
//!
//! What it costs and saves: docs/design/frame-rate-audit.md §9 and §10.

use crate::fly::FlyCam;
use crate::sky::{CascadeLayout, SkyState};
use bevy::camera::visibility::RenderLayers;
use bevy::ecs::entity::EntityHashMap;
use bevy::light::{CascadeShadowConfig, Cascades, DirectionalLight, DirectionalLightShadowMap, NotShadowCaster};
use bevy::pbr::HoldDirectionalShadowMaps;
use bevy::prelude::*;
use bevy::render::extract_resource::ExtractResourcePlugin;

/// `--sun-shadows=<mode>`: the mode the sun's shadows start in.
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SunShadowMode {
    Performance,
    Quality,
}

/// Sun shadows in Performance mode (the Sun shadows row's middle choice).
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct PerformanceShadows(pub bool);

/// The Sun shadows row's choices, in the order the page and the INI keep them: an INI
/// written while the row was an on/off switch (`1`/`0`) reads back as Quality or Off.
pub const SUN_SHADOW_CHOICES: [&str; 3] = ["Off", "Performance", "Quality"];

/// The Sun shadows row's choice now (an index into [`SUN_SHADOW_CHOICES`]).
pub fn sun_shadow_choice(world: &World) -> usize {
    if !world.resource::<SkyState>().shadows {
        0
    } else if world.resource::<PerformanceShadows>().0 {
        1
    } else {
        2
    }
}

/// Set the Sun shadows row's choice: Off turns the sun's shadows off and leaves the mode
/// as it was; Performance and Quality set the mode, then turn the shadows on.
pub fn set_sun_shadow_choice(world: &mut World, choice: usize) {
    if choice > 0 {
        let performance = choice == 1;
        if world.resource::<PerformanceShadows>().0 != performance {
            set_performance(world, performance);
        }
    }
    if world.resource::<SkyState>().shadows != (choice > 0) {
        crate::sky::set_shadows(world, choice > 0);
    }
}

/// With the switch on a tree casts a shadow within this distance of the camera (game
/// units: about 43 m), and again once it is back within [`TREE_SHADOW_BACK`] of it.
pub const TREE_SHADOW_NEAR: f32 = 3000.0;
pub const TREE_SHADOW_BACK: f32 = 2700.0;

/// Seconds between two looks at the trees.
const TREES_EVERY: f32 = 0.5;

/// The sun's cascade layout Performance replaced when it was chosen (Quality's, or the
/// count `--graphics-cycle cascades=` asked for), given back with Quality.
#[derive(Resource, Default)]
struct CascadesBeforePerformance(Option<CascadeLayout>);

/// Performance mode on or off now (the Sun shadows row): the cascade layout changes with
/// it. Only the layout is set here; graphics.rs `sun_shadow_quality` puts the cascades on
/// the sun as the hour wants them (the moon's single cascade by night).
pub fn set_performance(world: &mut World, on: bool) {
    let was = std::mem::replace(&mut world.resource_mut::<PerformanceShadows>().0, on);
    let current = world.resource::<SkyState>().shadow_layout;
    let mut before = world.get_resource_or_init::<CascadesBeforePerformance>();
    let layout = if on {
        if !was {
            before.0 = Some(current);
        }
        CascadeLayout::Performance
    } else if was {
        before.0.take().unwrap_or_default()
    } else {
        current
    };
    world.resource_mut::<SkyState>().shadow_layout = layout;
    let cascades = layout.count();
    let how = if on {
        "performance (maps drawn every other frame, the farthest every fourth, trees cast near the camera only, no arms shade light)"
    } else if staggered() {
        "cascades on a staggered schedule (the nearest every frame, the farther ones less often)"
    } else {
        "drawn every frame"
    };
    info!("sun shadows: {how}, {cascades} cascades");
}

/// A part of a SpeedTree tree (tree.rs `spawn`).
#[derive(Component)]
pub struct TreePart;

/// Marks the tree parts whose `NotShadowCaster` [`far_trees_cast_no_shadow`] put there.
#[derive(Component)]
pub struct FarTreeNoShadow;

/// Whether a tree part `distance` from the camera casts a shadow with the switch on,
/// given whether it casts now.
pub fn tree_casts(distance: f32, casting_now: bool) -> bool {
    distance <= if casting_now { TREE_SHADOW_NEAR } else { TREE_SHADOW_BACK }
}

/// With the switch on, trees far from the camera cast no shadow (module notes); with it
/// off every tree casts again.
fn far_trees_cast_no_shadow(
    mut commands: Commands,
    on: Res<PerformanceShadows>,
    time: Res<Time<Real>>,
    cams: Query<&GlobalTransform, With<FlyCam>>,
    trees: Query<(Entity, &GlobalTransform, Has<FarTreeNoShadow>), With<TreePart>>,
    ours: Query<Entity, With<FarTreeNoShadow>>,
    mut since: Local<f32>,
) {
    if !on.0 {
        for e in &ours {
            commands.entity(e).try_remove::<(NotShadowCaster, FarTreeNoShadow)>();
        }
        return;
    }
    *since += time.delta_secs();
    if *since < TREES_EVERY && !on.is_changed() {
        return;
    }
    *since = 0.0;
    let Some(cam) = cams.iter().next().map(|t| t.translation()) else { return };
    for (e, at, held) in &trees {
        let casts = tree_casts(at.translation().distance(cam), !held);
        if casts && held {
            commands.entity(e).try_remove::<(NotShadowCaster, FarTreeNoShadow)>();
        } else if !casts && !held {
            commands.entity(e).try_insert((NotShadowCaster, FarTreeNoShadow));
        }
    }
}

/// How often each cascade's map is drawn, nearest cascade first: `(n, phase)` draws it on
/// the frames where (frame + phase) is a multiple of n; a cascade past the list is drawn
/// every frame. With the switch on, the first two cascades every other frame, together,
/// and the third every fourth frame, on a frame the other two are not drawn (#2843): never
/// more than two drawn in a frame, 1.25 of 3 on average against 1 of the 2 of before. The
/// third holds the farthest and coarsest shadows, where a moving thing's shadow moving at a
/// quarter of the frame rate shows least.
pub const PERFORMANCE_SCHEDULE: [(u32, u32); 3] = [(2, 0), (2, 0), (4, 1)];
/// With the switch off: the nearest cascade every frame, the second every other frame,
/// the third and the fourth every fourth frame, all on different frames, so exactly two
/// are drawn in each frame (2 of 4 on average; 1.75 of 3 with the 3 cascades of before
/// #2843, which drew the farthest every fourth frame: `--graphics-cycle cascades=3` still
/// does). The far cascades hold most of the casters (with 3 cascades the farthest held
/// about 1,150 of 1,400 at Goodsprings, 1,570 of 2,140 at Novac), and their shadows are
/// the coarsest, so a moving thing's shadow there moving at a quarter of the frame rate
/// shows least.
pub const STAGGERED_SCHEDULE: [(u32, u32); 4] = [(1, 0), (2, 1), (4, 2), (4, 0)];

/// The cascades (a bit each, nearest first) whose maps frame `frame` keeps from an earlier
/// frame under `schedule`, of `cascades` cascades.
pub fn held_mask(schedule: &[(u32, u32)], frame: u64, cascades: usize) -> u32 {
    (0..cascades.min(32)).fold(0, |mask, i| {
        let (n, phase) = schedule.get(i).copied().unwrap_or((1, 0));
        if n > 1 && !(frame + u64::from(phase)).is_multiple_of(u64::from(n)) { mask | 1 << i } else { mask }
    })
}

/// A light's cascades kept from the frame that drew them, with what its
/// `CascadeShadowConfig` was then (far bounds, overlap, minimum distance; `None` without
/// one).
#[derive(Clone)]
pub(crate) struct Kept {
    cascades: Cascades,
    config: Option<(Vec<f32>, f32, f32)>,
}

impl Kept {
    fn new(cascades: &Cascades, config: Option<&CascadeShadowConfig>) -> Self {
        Kept { cascades: cascades.clone(), config: config.map(|c| (c.bounds.clone(), c.overlap_proportion, c.minimum_distance)) }
    }

    /// Whether `config` is the one these cascades were kept under.
    fn same_config(&self, config: Option<&CascadeShadowConfig>) -> bool {
        match (&self.config, config) {
            (None, None) => true,
            (Some((bounds, overlap, minimum)), Some(c)) => *bounds == c.bounds && *overlap == c.overlap_proportion && *minimum == c.minimum_distance,
            _ => false,
        }
    }
}

/// A light casting shadows this frame: its cascades as Bevy built them and the config they
/// were built from.
type Lit<'a> = (Entity, &'a Cascades, Option<&'a CascadeShadowConfig>);

/// Whether the cascades kept from earlier frames can stand in for this frame's: every
/// light has kept cascades for the same views, as many of them, under the same config.
/// The vendored renderer reads each cascade's far bound from the light's config, not from
/// the kept cascade, so a held cascade under changed bounds (the Shadow resolution row,
/// the Shadow distance slider, Longer shadows, the mode) would be read against the wrong
/// slice: a change of the config draws every cascade that frame (#2843 review).
fn same_layout(kept: &EntityHashMap<Kept>, now: &[Lit]) -> bool {
    !now.is_empty()
        && now.iter().all(|(light, cascades, config)| {
            kept.get(light).is_some_and(|k| {
                k.same_config(*config)
                    && k.cascades.cascades.len() == cascades.cascades.len()
                    && cascades.cascades.iter().all(|(view, c)| k.cascades.cascades.get(view).is_some_and(|kc| kc.len() == c.len()))
            })
        })
}

/// Whether the cascades are drawn on a staggered schedule with the switch off (test aid
/// `NVF_PLAY_TEST_SHADOW_STAGGER=0` draws every cascade every frame).
fn staggered() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("NVF_PLAY_TEST_SHADOW_STAGGER").map_or(true, |v| v != "0"))
}

/// The cascades not drawn this frame ([`PERFORMANCE_SCHEDULE`], [`STAGGERED_SCHEDULE`]): give
/// the directional lights back those cascades as the frame that drew them had them, and
/// have the renderer keep their maps (module notes). Runs after Bevy has built this
/// frame's cascades, after they are anchored and stretched toward the sun, and before Bevy
/// fits the lights' frusta to them (shadow_anchor.rs `cascade_edits` schedules it).
///
/// Only lights casting shadows count. Bevy does not rebuild the cascades of a light casting
/// none, and draws no maps for it: nothing of it is kept or given back, so the first frame
/// it casts again (Sun shadows switched back on, the sun or moon past the horizon's fade)
/// finds nothing kept for it and draws every cascade. (Before, a light casting no shadows
/// was kept and given back like the others; when it cast again with the same layout a held
/// frame gave it the cascades of the frame its shadows went off, read against maps drawn
/// before then, or, with a new shadow texture, drawn with those stale matrices.)
/// The main camera and the water mirror while the mirror shares the main camera's shadow
/// maps this frame ([`hold_cascades`]), for [`share_mirror_casters`].
#[derive(Resource, Default)]
pub(crate) struct MirrorShadowShare(pub Option<(Entity, Entity)>);

/// The mirror draws the main camera's casters too. The same matrices are not enough for
/// the same maps: after the light's visibility pass each view's caster lists are cut down
/// by that view's own outline (`sky::sun_casters_over_slice`) and eye
/// (`player_body::near_shade_casters`), so the mirror drew other casters into the shared
/// layers than the main camera draws over them, and a held frame would have shown the
/// reflection the main camera's set where a drawn frame showed the mirror's. Runs after
/// both cuts.
pub(crate) fn share_mirror_casters(share: Res<MirrorShadowShare>, mut lights: Query<&mut bevy::camera::visibility::CascadesVisibleEntities, With<DirectionalLight>>) {
    let Some((main, mirror)) = share.0 else { return };
    for mut visible in &mut lights {
        if !visible.entities.contains_key(&mirror) {
            continue;
        }
        if let Some(lists) = visible.entities.get(&main).cloned() {
            visible.entities.insert(mirror, lists);
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn hold_cascades(
    on: Res<PerformanceShadows>,
    map: Option<Res<DirectionalLightShadowMap>>,
    mut hold: ResMut<HoldDirectionalShadowMaps>,
    mut share: Option<ResMut<MirrorShadowShare>>,
    mut all_lights: Query<(Entity, &DirectionalLight, &mut Cascades, Option<&CascadeShadowConfig>, Option<&RenderLayers>)>,
    views: Query<(Entity, &Camera, Option<&RenderLayers>, Has<FlyCam>, Has<crate::reflection::ReflectionCamera>), With<Camera3d>>,
    mut kept: Local<EntityHashMap<Kept>>,
    mut frame: Local<u64>,
    mut said: Local<Option<bool>>,
) {
    *frame = frame.wrapping_add(1);
    if let Some(share) = share.as_mut().filter(|s| s.0.is_some()) {
        share.0 = None;
    }
    let schedule: &[(u32, u32)] = if on.0 {
        &PERFORMANCE_SCHEDULE
    } else if staggered() {
        &STAGGERED_SCHEDULE
    } else {
        &[]
    };
    // The water mirror shares the main camera's maps (module notes) when it has the same
    // shadowed lights: then the renderer gives each light the same layers in both views.
    let mut world_views = views.iter().filter(|(_, c, _, fly, mirror)| c.is_active && *fly && !*mirror);
    let main = world_views.next().filter(|_| world_views.next().is_none());
    let mirror = views.iter().find(|(_, c, _, _, mirror)| c.is_active && *mirror);
    let shared = match (main, mirror) {
        (Some((main, _, main_layers, _, _)), Some((mirror, _, mirror_layers, _, _))) if !schedule.is_empty() => {
            let same = all_lights.iter().filter(|(_, l, _, _, _)| l.shadow_maps_enabled).all(|(_, _, cascades, _, layers)| {
                let layers = layers.unwrap_or_default();
                let lit = layers.intersects(mirror_layers.unwrap_or_default());
                lit == layers.intersects(main_layers.unwrap_or_default()) && (!lit || cascades.cascades.contains_key(&main))
            });
            same.then_some((main, mirror))
        }
        _ => None,
    };
    // Directional shadow array layers are view-local: each camera redraws the
    // same layers before consuming them. With multiple shadowed views, the last
    // camera's maps cannot stand in for another camera next frame. Count the
    // union across lights, excluding inactive/disjoint cameras, empty lists and a
    // mirror that shares the main camera's maps.
    let mut shadowed_views = views.iter().filter(|(entity, camera, layers, _, _)| {
        camera.is_active && shared.is_none_or(|(_, mirror)| mirror != *entity) && all_lights.iter().any(|(_, light, cascades, _, light_layers)| {
            light.shadow_maps_enabled && light_layers.unwrap_or_default().intersects(layers.unwrap_or_default())
                && cascades.cascades.get(entity).is_some_and(|c| !c.is_empty())
        })
    });
    if shadowed_views.nth(1).is_some() {
        // Forget matrices as well as the hold mask. On return to one view, draw
        // fresh maps first, even if its layout still matches the old cache.
        kept.clear();
        if hold.0 != 0 { hold.0 = 0; }
        return;
    }
    let lights: Vec<Lit> = all_lights.iter().filter(|(_, l, _, _, _)| l.shadow_maps_enabled).map(|(e, _, c, config, _)| (e, c, config)).collect();
    let count = lights.iter().flat_map(|(_, c, _)| c.cascades.values().map(Vec::len)).max().unwrap_or(0);
    // A new map size makes new maps: nothing to keep.
    let same_maps = !map.is_some_and(|m| m.is_changed());
    let mut mask = held_mask(schedule, *frame, count);
    if mask != 0 && !(same_maps && same_layout(&kept, &lights)) {
        mask = 0;
    }
    if mask == 0 {
        kept.clear();
        if !schedule.is_empty() {
            kept.extend(lights.iter().map(|(e, c, config)| (*e, Kept::new(c, *config))));
        }
    } else {
        // A light that stopped casting while another casts on: forget what it kept.
        if kept.len() != lights.len() {
            kept.retain(|e, _| lights.iter().any(|(l, _, _)| l == e));
        }
        for (light, directional, mut cascades, _, _) in &mut all_lights {
            if !directional.shadow_maps_enabled {
                continue;
            }
            let Some(k) = kept.get_mut(&light) else { continue };
            for (view, list) in cascades.cascades.iter_mut() {
                let Some(kl) = k.cascades.cascades.get_mut(view) else { continue };
                for (i, (c, kc)) in list.iter_mut().zip(kl.iter_mut()).enumerate() {
                    if mask & (1 << i) != 0 {
                        *c = kc.clone();
                    } else {
                        *kc = c.clone();
                    }
                }
            }
        }
    }
    // Every frame, drawn or held, so the reflection's shadows do not change fit with the
    // schedule: the mirror draws (on drawn frames) and reads the main camera's maps.
    if let Some((main, mirror)) = shared {
        if let Some(share) = share.as_mut() {
            share.0 = shared;
        }
        for (_, directional, mut cascades, _, _) in &mut all_lights {
            if directional.shadow_maps_enabled && cascades.cascades.contains_key(&mirror) {
                if let Some(list) = cascades.cascades.get(&main).cloned() {
                    cascades.cascades.insert(mirror, list);
                }
            }
        }
    }
    // Once for each schedule that comes into use (the switch on, or the staggered one with
    // it off): the first frame that kept maps.
    if schedule.is_empty() {
        *said = None;
    } else if mask != 0 && *said != Some(on.0) {
        if on.0 {
            info!("sun shadows: a frame kept the shadow maps of the frame before ({} light(s))", kept.len());
        } else {
            // As many bits as cascades (`0b0110` of 4).
            info!("sun shadows: a frame kept its farther cascades' maps (staggered schedule, held cascades {mask:#0width$b}, {} light(s))", kept.len(), width = count + 2);
        }
        *said = Some(on.0);
    }
    if hold.0 != mask {
        hold.0 = mask;
    }
}

/// `performance`: start in Performance mode (`--sun-shadows=performance`; the program sets
/// `SkyState`'s cascade layout to match).
pub struct ShadowRatePlugin {
    pub performance: bool,
}

impl Plugin for ShadowRatePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(PerformanceShadows(self.performance))
            .insert_resource(CascadesBeforePerformance(self.performance.then_some(CascadeLayout::Quality)))
            .init_resource::<HoldDirectionalShadowMaps>()
            .init_resource::<MirrorShadowShare>()
            .add_plugins(ExtractResourcePlugin::<HoldDirectionalShadowMaps>::default())
            // `hold_cascades` runs with the other edits of the cascades, in their order
            // (shadow_anchor.rs `cascade_edits`).
            .add_systems(Update, far_trees_cast_no_shadow);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::light::cascade::Cascade;

    fn cascades(view: Entity, n: usize) -> Cascades {
        let mut c = Cascades::default();
        c.cascades.insert(view, vec![Cascade::default(); n]);
        c
    }

    #[test]
    fn maps_are_held_only_for_the_same_lights_and_cascades() {
        let (sun, arms, view) = (Entity::from_raw_u32(1).unwrap(), Entity::from_raw_u32(2).unwrap(), Entity::from_raw_u32(3).unwrap());
        let (three, two) = (cascades(view, 3), cascades(view, 2));
        let mut kept = EntityHashMap::default();
        kept.insert(sun, Kept::new(&three, None));
        assert!(same_layout(&kept, &[(sun, &three, None)]));
        // Never without lights.
        assert!(!same_layout(&kept, &[]));
        // A light that was not there (the arms' shade light switching on), a different
        // number of cascades, or another view: this frame draws.
        assert!(!same_layout(&kept, &[(sun, &three, None), (arms, &three, None)]));
        assert!(!same_layout(&kept, &[(sun, &two, None)]));
        assert!(!same_layout(&kept, &[(sun, &cascades(arms, 3), None)]));
    }

    /// The same number of cascades under other bounds or another overlap (the Shadow
    /// resolution row, the Shadow distance slider, Longer shadows): this frame draws.
    #[test]
    fn maps_are_held_only_under_the_same_config() {
        let (sun, view) = (Entity::from_raw_u32(1).unwrap(), Entity::from_raw_u32(3).unwrap());
        let four = cascades(view, 4);
        let config = |bounds: [f32; 4], overlap: f32| CascadeShadowConfig { bounds: bounds.to_vec(), overlap_proportion: overlap, minimum_distance: 5.0 };
        let near = config([700.0, 2000.0, 5000.0, 8000.0], 0.2);
        let mut kept = EntityHashMap::default();
        kept.insert(sun, Kept::new(&four, Some(&near)));
        assert!(same_layout(&kept, &[(sun, &four, Some(&near))]));
        assert!(!same_layout(&kept, &[(sun, &four, Some(&config([1500.0, 2400.0, 4800.0, 8000.0], 0.4)))]), "other bounds");
        assert!(!same_layout(&kept, &[(sun, &four, Some(&config([700.0, 2000.0, 5000.0, 8000.0], 0.4)))]), "another overlap");
        assert!(!same_layout(&kept, &[(sun, &four, None)]), "the config gone");
        // In the system: a frame that would hold draws every cascade when the bounds changed.
        let mut world = World::new();
        world.insert_resource(PerformanceShadows(true));
        world.init_resource::<HoldDirectionalShadowMaps>();
        let mut schedule = Schedule::default();
        schedule.add_systems(hold_cascades);
        let light = world.spawn((DirectionalLight { shadow_maps_enabled: true, ..default() }, near.clone(), cascades(view, 2))).id();
        let mut run = |world: &mut World| {
            schedule.run(world);
            world.resource::<HoldDirectionalShadowMaps>().0
        };
        // Performance with 2 cascades: frames 1 and 2 draw, 3 holds both.
        assert_eq!((run(&mut world), run(&mut world), run(&mut world)), (0, 0, 0b11));
        // Frame 4 draws, frame 5 would hold: under new bounds it draws.
        assert_eq!(run(&mut world), 0);
        world.entity_mut(light).insert(config([1500.0, 2400.0, 4800.0, 8000.0], 0.4));
        assert_eq!(run(&mut world), 0, "new bounds, same count: every cascade drawn");
        // Then held as usual under the new config.
        assert_eq!((run(&mut world), run(&mut world)), (0, 0b11));
    }

    /// The switch gives back the cascade layout it replaced (#2182 review: it reset a
    /// `--graphics-cycle cascades=` value to the default), and only sets the layout. Since
    /// #2843 Performance has as many cascades as `cascades=3`, but not their spacing.
    #[test]
    fn the_switch_gives_back_the_cascades_it_replaced() {
        let mut world = World::new();
        world.init_resource::<PerformanceShadows>();
        let mut sky = SkyState::default();
        // `--graphics-cycle cascades=3`: the old spacing, as many cascades as Performance.
        let given = CascadeLayout::Spaced(3);
        assert_eq!(given.count(), CascadeLayout::Performance.count());
        sky.shadow_layout = given;
        world.insert_resource(sky);
        set_performance(&mut world, true);
        assert_eq!(world.resource::<SkyState>().shadow_layout, CascadeLayout::Performance);
        set_performance(&mut world, true);
        set_performance(&mut world, false);
        assert_eq!(world.resource::<SkyState>().shadow_layout, given);
        // Off when it already is: the layout stays.
        set_performance(&mut world, false);
        assert_eq!(world.resource::<SkyState>().shadow_layout, given);
        // Started in Performance (`--sun-shadows=performance`): Quality's layout back.
        let mut world = World::new();
        world.insert_resource(PerformanceShadows(true));
        world.insert_resource(CascadesBeforePerformance(Some(CascadeLayout::Quality)));
        let mut sky = SkyState::default();
        sky.shadow_layout = CascadeLayout::Performance;
        world.insert_resource(sky);
        set_performance(&mut world, false);
        assert_eq!(world.resource::<SkyState>().shadow_layout, CascadeLayout::Quality);
    }

    #[test]
    fn the_schedules_hold_the_cascades_they_should() {
        // Performance: the first two cascades every other frame, together, the third every
        // fourth frame, on a frame the first two are held.
        let performance: Vec<u32> = (0..4).map(|f| held_mask(&PERFORMANCE_SCHEDULE, f, 3)).collect();
        assert_eq!(performance, [0b100, 0b111, 0b100, 0b011]);
        // Over four frames each cascade is drawn at least once.
        assert_eq!(performance.iter().fold(0b111, |all, m| all & m), 0);
        // With 2 cascades (still a valid input): both every other frame, together.
        let two: Vec<u32> = (0..4).map(|f| held_mask(&PERFORMANCE_SCHEDULE, f, 2)).collect();
        assert_eq!(two, [0, 0b11, 0, 0b11]);
        // Staggered: the nearest every frame, the second every other, the third and the
        // fourth every fourth on different frames: exactly two drawn in each frame.
        let staggered: Vec<u32> = (0..4).map(|f| held_mask(&STAGGERED_SCHEDULE, f, 4)).collect();
        assert_eq!(staggered, [0b0110, 0b1100, 0b1010, 0b1100]);
        assert!(staggered.iter().all(|m| m.count_ones() == 2));
        // With 3 cascades (`--graphics-cycle cascades=3`) the schedule of before: the
        // farthest every fourth frame, never both of the far two drawn in one frame.
        let three: Vec<u32> = (0..4).map(|f| held_mask(&STAGGERED_SCHEDULE, f, 3)).collect();
        assert_eq!(three, [0b110, 0b100, 0b010, 0b100]);
        // More cascades than the schedule: those are drawn every frame.
        assert_eq!(held_mask(&PERFORMANCE_SCHEDULE, 1, 4), 0b0111);
        assert_eq!(held_mask(&[], 1, 3), 0);
    }

    /// A light casting no shadows is neither kept nor given back, and the first frame it
    /// casts again draws every cascade, whatever the frame's place in the schedule.
    #[test]
    fn a_light_casting_again_draws_every_cascade_first() {
        let mut world = World::new();
        world.insert_resource(PerformanceShadows(true));
        world.init_resource::<HoldDirectionalShadowMaps>();
        let mut schedule = Schedule::default();
        schedule.add_systems(hold_cascades);
        let view = world.spawn_empty().id();
        let sun = world.spawn(DirectionalLight { shadow_maps_enabled: true, ..default() }).id();
        // Frame `k`'s cascades, told apart by their texel size.
        let built = |k: f32| {
            let mut c = Cascades::default();
            c.cascades.insert(view, vec![Cascade { texel_size: k, ..default() }; 2]);
            c
        };
        let mut run = |world: &mut World, k: u32, casting: bool| {
            world.get_mut::<DirectionalLight>(sun).unwrap().shadow_maps_enabled = casting;
            world.entity_mut(sun).insert(built(k as f32));
            schedule.run(world);
            let texels: Vec<f32> = world.get::<Cascades>(sun).unwrap().cascades[&view].iter().map(|c| c.texel_size).collect();
            (world.resource::<HoldDirectionalShadowMaps>().0, texels)
        };
        // Performance: frame 1 draws (nothing kept yet), 2 draws, 3 holds both.
        assert_eq!(run(&mut world, 1, true), (0, vec![1.0, 1.0]));
        assert_eq!(run(&mut world, 2, true), (0, vec![2.0, 2.0]));
        assert_eq!(run(&mut world, 3, true), (0b11, vec![2.0, 2.0]));
        // Shadows off: its cascades left alone, nothing held, on held frames too.
        for k in 4..=6 {
            assert_eq!(run(&mut world, k, false), (0, vec![k as f32; 2]), "frame {k}");
        }
        // Casting again on a frame that would hold: drawn, then held as usual.
        assert_eq!(run(&mut world, 7, true), (0, vec![7.0, 7.0]));
        assert_eq!(run(&mut world, 8, true), (0, vec![8.0, 8.0]));
        assert_eq!(run(&mut world, 9, true), (0b11, vec![8.0, 8.0]));
    }

    #[test]
    fn multiple_shadowed_views_draw_fresh_and_single_view_cache_recovers() {
        let mut world = World::new();
        world.insert_resource(PerformanceShadows(true));
        world.init_resource::<HoldDirectionalShadowMaps>();
        let mut schedule = Schedule::default();
        schedule.add_systems(hold_cascades);
        let main = world.spawn((Camera3d::default(), Camera::default())).id();
        let second = world.spawn((Camera3d::default(), Camera::default(), RenderLayers::layer(2))).id();
        let ui = world.spawn(Camera::default()).id(); // A UI-only view has no Camera3d.
        let empty = world.spawn((Camera3d::default(), Camera::default())).id();
        let light = world.spawn(DirectionalLight { shadow_maps_enabled: true, ..default() }).id();
        let mut run = |world: &mut World, k: u32| {
            let mut built = Cascades::default();
            for view in [main, second, ui] {
                built.cascades.insert(view, vec![Cascade { texel_size: k as f32, ..default() }; 2]);
            }
            built.cascades.insert(empty, Vec::new());
            world.entity_mut(light).insert(built);
            schedule.run(world);
            let texels: Vec<f32> = world.get::<Cascades>(light).unwrap().cascades[&main].iter().map(|c| c.texel_size).collect();
            (world.resource::<HoldDirectionalShadowMaps>().0, texels)
        };
        // Disjoint layers, a 2D/UI camera, and an empty cascade list do not disable holding.
        assert_eq!(run(&mut world, 1), (0, vec![1.0; 2]));
        assert_eq!(run(&mut world, 2), (0, vec![2.0; 2]));
        assert_eq!(run(&mut world, 3), (0b11, vec![2.0; 2]));
        // A second shadowed 3D view: no old matrices restored, even on held frames.
        world.entity_mut(second).insert(RenderLayers::default());
        for k in 4..=6 {
            assert_eq!(run(&mut world, k), (0, vec![k as f32; 2]));
        }
        // Its old cascades can still be present when the camera switches off.
        world.get_mut::<Camera>(second).unwrap().is_active = false;
        assert_eq!(run(&mut world, 7), (0, vec![7.0; 2]), "return on held frame must draw fresh");
        assert_eq!(run(&mut world, 8), (0, vec![8.0; 2]));
        assert_eq!(run(&mut world, 9), (0b11, vec![8.0; 2]));
    }

    #[test]
    fn shadowed_view_count_is_union_across_directional_lights() {
        let mut world = World::new();
        world.insert_resource(PerformanceShadows(true));
        world.init_resource::<HoldDirectionalShadowMaps>();
        let mut schedule = Schedule::default();
        schedule.add_systems(hold_cascades);
        let main = world.spawn((Camera3d::default(), Camera::default())).id();
        let other = world.spawn((Camera3d::default(), Camera::default(), RenderLayers::layer(2))).id();
        world.spawn((DirectionalLight { shadow_maps_enabled: true, ..default() }, cascades(main, 2)));
        let separate = world.spawn((DirectionalLight { shadow_maps_enabled: true, ..default() }, cascades(other, 2), RenderLayers::layer(2))).id();
        // Neither light has two views, but their view-local layers still share storage.
        for _ in 0..2 {
            schedule.run(&mut world);
            assert_eq!(world.resource::<HoldDirectionalShadowMaps>().0, 0);
        }
        world.get_mut::<DirectionalLight>(separate).unwrap().shadow_maps_enabled = false;
        schedule.run(&mut world);
        assert_eq!(world.resource::<HoldDirectionalShadowMaps>().0, 0, "fresh draw before resuming single-view holds");
        schedule.run(&mut world);
        schedule.run(&mut world);
        assert_eq!(world.resource::<HoldDirectionalShadowMaps>().0, 0b11);
    }

    /// The mirror takes the main camera's final caster lists while it shares its maps.
    #[test]
    fn the_sharing_mirror_draws_the_main_cameras_casters() {
        use bevy::camera::visibility::{CascadesVisibleEntities, VisibleMeshEntities};
        let mut world = World::new();
        let (main, mirror, other) = (world.spawn_empty().id(), world.spawn_empty().id(), world.spawn_empty().id());
        let (a, b) = (world.spawn_empty().id(), world.spawn_empty().id());
        let list = |e: &[Entity]| vec![VisibleMeshEntities { entities: e.to_vec() }];
        let mut visible = CascadesVisibleEntities::default();
        visible.entities.insert(main, list(&[a]));
        visible.entities.insert(mirror, list(&[a, b]));
        visible.entities.insert(other, list(&[b]));
        let light = world.spawn((DirectionalLight::default(), visible)).id();
        world.insert_resource(MirrorShadowShare(None));
        let mut schedule = Schedule::default();
        schedule.add_systems(share_mirror_casters);
        let lists = |world: &World, view: Entity| world.get::<CascadesVisibleEntities>(light).unwrap().entities[&view][0].entities.clone();
        schedule.run(&mut world);
        assert_eq!(lists(&world, mirror), vec![a, b], "not sharing: the mirror's own list");
        world.insert_resource(MirrorShadowShare(Some((main, mirror))));
        schedule.run(&mut world);
        assert_eq!((lists(&world, mirror), lists(&world, main), lists(&world, other)), (vec![a], vec![a], vec![b]));
    }

    /// The water mirror is given the main camera's cascades, held ones included, every
    /// frame: both draw the same maps into the same layers, so the schedule keeps holding.
    #[test]
    fn the_water_mirror_shares_the_main_cameras_cascades_and_holds() {
        let mut world = World::new();
        world.insert_resource(PerformanceShadows(true));
        world.init_resource::<HoldDirectionalShadowMaps>();
        let mut schedule = Schedule::default();
        schedule.add_systems(hold_cascades);
        let fly = FlyCam { yaw: 0.0, pitch: 0.0, speed: 0.0 };
        let main = world.spawn((Camera3d::default(), Camera::default(), fly, RenderLayers::from_layers(&[0, 1]))).id();
        let mirror = world.spawn((Camera3d::default(), Camera::default(), crate::reflection::ReflectionCamera, RenderLayers::layer(0))).id();
        let light = world.spawn(DirectionalLight { shadow_maps_enabled: true, ..default() }).id();
        let mut run = |world: &mut World, k: u32| {
            let mut built = Cascades::default();
            built.cascades.insert(main, vec![Cascade { texel_size: k as f32, ..default() }; 2]);
            built.cascades.insert(mirror, vec![Cascade { texel_size: 100.0 + k as f32, ..default() }; 2]);
            world.entity_mut(light).insert(built);
            schedule.run(world);
            let c = world.get::<Cascades>(light).unwrap();
            let texels = |view: Entity| c.cascades[&view].iter().map(|c| c.texel_size).collect::<Vec<f32>>();
            assert_eq!(texels(mirror), texels(main), "frame {k}: the mirror reads the main camera's maps");
            (world.resource::<HoldDirectionalShadowMaps>().0, texels(main))
        };
        assert_eq!(run(&mut world, 1), (0, vec![1.0; 2]));
        assert_eq!(run(&mut world, 2), (0, vec![2.0; 2]));
        assert_eq!(run(&mut world, 3), (0b11, vec![2.0; 2]), "held with the mirror on");
        assert_eq!(run(&mut world, 4), (0, vec![4.0; 2]));
        // A shadowed light the main camera sees and the mirror does not: the layers differ
        // between the views, so nothing is held.
        world.spawn((DirectionalLight { shadow_maps_enabled: true, ..default() }, cascades(main, 2), RenderLayers::layer(1)));
        for _ in 0..3 {
            schedule.run(&mut world);
            assert_eq!(world.resource::<HoldDirectionalShadowMaps>().0, 0);
        }
    }

    #[test]
    fn a_far_tree_stops_casting_and_starts_again_nearer() {
        assert!(tree_casts(TREE_SHADOW_NEAR, true) && !tree_casts(TREE_SHADOW_NEAR + 1.0, true));
        // Between the two distances it stays as it is.
        let between = (TREE_SHADOW_NEAR + TREE_SHADOW_BACK) / 2.0;
        assert!(tree_casts(between, true) && !tree_casts(between, false));
        assert!(tree_casts(TREE_SHADOW_BACK, false));
    }

    /// #3779: switching between third person (4), first person with ArmsShade (5) and
    /// flashlight (6) grows the directional shadow texture's layer count once per peak and
    /// reuses it on every later transition instead of reallocating on shrink/grow.
    #[test]
    fn directional_shadow_layer_high_water_reuses_texture_across_view_and_flashlight_toggles() {
        let mut hw = bevy::pbr::NvfDirectionalShadowLayerHighWater::default();
        // Staying in third person keeps exactly 4 layers (no extra memory for 3rd-person-only players).
        assert_eq!(hw.layers(4096, 4, 256), 4);
        assert_eq!(hw.layers(4096, 4, 256), 4);
        // Entering first person (4 sun + 1 ArmsShade) grows once to 5; leaving stays at 5.
        assert_eq!(hw.layers(4096, 5, 256), 5);
        assert_eq!(hw.layers(4096, 4, 256), 5);
        // Toggling flashlight on (5 + 1 spot) grows once to 6.
        assert_eq!(hw.layers(4096, 6, 256), 6);
        // Repeatedly toggling 1st/3rd person and flashlight never changes the allocated layer count.
        let mut changes = 0usize;
        let mut last = 6usize;
        for needed in [5, 4, 5, 6, 4, 5, 4, 6, 5, 4] {
            let allocated = hw.layers(4096, needed, 256);
            if allocated != last {
                changes += 1;
                last = allocated;
            }
            assert_eq!(allocated, 6);
        }
        assert_eq!(changes, 0, "no texture reallocation across 4 <-> 5 <-> 6 transitions");
        // Changing shadow resolution resets the high-water mark to the newly needed layer count.
        assert_eq!(hw.layers(2048, 4, 256), 4);
        assert_eq!(hw.layers(2048, 5, 256), 5);
        assert_eq!(hw.layers(2048, 4, 256), 5);
        assert_eq!(hw.layers(4096, 4, 256), 4);
        // Respects max_layers cap and >= 1 floor, and reset() clears remembered state.
        assert_eq!(hw.layers(4096, 0, 256), 4);
        assert_eq!(hw.layers(4096, 300, 8), 8);
        hw.reset();
        assert_eq!(hw.layers(4096, 0, 256), 1);
    }
}
