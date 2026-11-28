//! Textures scripts swap on references' shapes (`SwapTexture`, playtest list 5 item 38;
//! `fnv_game::functions::misc::swap_texture`, docs/design/script-stubs.md §4): the
//! drawn shapes of the reference whose model shape has the name the call gave get a
//! copy of their material with the new diffuse texture. Done again when a reference is
//! drawn anew (a cell loaded again), as the swaps are kept per reference.
//!
//! The shape is found by its material: the drawn entities of a reference (`RefVisual`)
//! carry the material of the model's shape they draw [design: no shape names are kept
//! on drawn entities of placed objects; a shape whose material was copied (animated,
//! emittance) is not found]. The shapes of actors' parts (a Securitron's screen,
//! `SetSecuritronExpression`) carry their name (`nvf_assets::actor::ShapeName`) and are
//! found by it.
//!
//! A mesh drawn see-through wears a single copy owned by `nvf_play::fade::Fading`
//! (shared by presence fades and `SetActorAlpha`, #3105). The swap goes to the mesh's own
//! persistent material (`nvf_play::fade::persistent_material`), so the end of the alpha or
//! the fade does not bring the old texture back (the psych test's card fades in as Doc
//! Mitchell's line swaps its texture, smoke test `rorschach-cards`): `queue_retarget`
//! updates `Fading::original` and rebuilds its single see-through copy in place at the
//! current combined opacity.

use bevy::prelude::*;
use nvf_assets::actor::ShapeName;
use nvf_assets::model::ModelCtx;
use nvf_assets::NifMaterial;
use nvf_play::fade::Fading;
use nvf_play::script::ScriptState;
use nvf_play::sim::SimState;
use nvf_play::RefVisual;
use std::collections::HashMap;

use crate::world::WorldData;

pub struct TextureSwapPlugin;

impl Plugin for TextureSwapPlugin {
    fn build(&self, app: &mut App) {
        // Before the fades step, which then copies a swapped material the same frame.
        app.init_resource::<Swapped>().add_systems(Update, apply.before(nvf_play::PlaySet::Sync)).add_systems(Last, release_swapped);
    }
}

/// The materials made for swaps: (original material, texture) → copy, and each copy's
/// original.
#[derive(Resource, Default)]
pub(crate) struct Swapped {
    made: HashMap<(AssetId<NifMaterial>, String), Handle<NifMaterial>>,
    original: HashMap<AssetId<NifMaterial>, AssetId<NifMaterial>>,
    generation: u64,
}

impl Swapped {
    /// The swapped copies held, for the stress census.
    pub(crate) fn share(&self, materials: &Assets<NifMaterial>) -> nvf_assets::release::CacheShare {
        let mut s = nvf_assets::release::CacheShare::default();
        for h in self.made.values() {
            s.material(materials, h, 1);
        }
        s
    }
}

/// Every [`SWEEP_SECS`](nvf_assets::release::SWEEP_SECS), drop the swapped copies no mesh
/// wears (its reference unloaded or was swapped again), with their originals' entries, so
/// the textures go with their models (issue #904). A reference drawn again is swapped
/// again from the script's list.
fn release_swapped(time: Res<Time<Real>>, mut swapped: ResMut<Swapped>, mut since: Local<f32>) {
    *since += time.delta_secs();
    if *since < nvf_assets::release::SWEEP_SECS {
        return;
    }
    *since = 0.0;
    let s = &mut *swapped;
    if nvf_assets::release::retain_held(&mut s.made, 1) > 0 {
        let kept: std::collections::HashSet<AssetId<NifMaterial>> = s.made.values().map(Handle::id).collect();
        s.original.retain(|copy, _| kept.contains(copy));
    }
}

/// Whether a model's shape name is the shape a swap names: the call's name comes with its
/// leading `#`s dropped and in lower case (`fnv_game::functions::misc::texture_swap_names`),
/// the model's keeps them. The end slides swap `"##gs01:0"` on `gs01.nif`, whose shape is
/// `##GS01:0` [observed: fnvtool nif decode of meshes\interface\endgame\gs01.nif]; compared
/// with its `##` kept the swap never matched and the model's own first slide stayed up
/// (issue #2226).
fn same_shape(model_name: &str, swap: &str) -> bool {
    model_name.trim_start_matches('#').eq_ignore_ascii_case(swap)
}

pub(crate) fn apply(script: Option<Res<ScriptState>>, sim: Option<Res<SimState>>, world: Option<Res<WorldData>>, mut drawing: Swapping) {
    let (Some(script), Some(sim), Some(world)) = (script, sim, world) else { return };
    let Some(rt) = script.rt.as_ref() else { return };
    drawing.sync(&rt.game.texture_swaps, &sim, &world);
}

/// [design, #2557] Production swap application, shared with synthetic ECS tests.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct Swapping<'w, 's> {
    commands: Commands<'w, 's>,
    models: ModelCtx<'w>,
    swapped: ResMut<'w, Swapped>,
    #[allow(clippy::type_complexity)]
    drawn: Query<'w, 's, (Entity, &'static RefVisual, Option<&'static MeshMaterial3d<NifMaterial>>, Option<&'static crate::mesh_terrain_blend::BlendShape>, Option<&'static ShapeName>, Option<&'static Fading>, Option<&'static nvf_assets::nif_material::MaterialOverride>)>,
    added: Query<'w, 's, (), NewSwapTargets>,
}

/// What makes the retained swaps apply again: a reference's mesh drawn anew, as on main,
/// and, for the mesh terrain blend's shapes only (test aid; no entity has `BlendShape`
/// without it), their marker or a restored ordinary material arriving [design, #2661].
type BlendShapeRef = (With<crate::mesh_terrain_blend::BlendShape>, With<RefVisual>);
type NewSwapTargets = Or<(Added<RefVisual>, (Added<MeshMaterial3d<NifMaterial>>, BlendShapeRef), (Added<crate::mesh_terrain_blend::BlendShape>, With<RefVisual>))>;

impl Swapping<'_, '_> {
    pub(crate) fn sync(&mut self, swaps: &fnv_game::functions::misc::TextureSwaps, sim: &SimState, world: &WorldData) {
        let Self { commands, models, swapped, drawn, added } = self;
        if swaps.by_ref.is_empty() || (swaps.generation == swapped.generation && added.is_empty()) {
            return;
        }
        swapped.generation = swaps.generation;
        for (&r, list) in &swaps.by_ref {
            let model = sim
                .base_of(r.0)
                .and_then(|b| fnv_world::placed::base_object(world.lo, world.schema, b.0))
                .and_then(|b| b.model)
                .map(|m| fnv_actor::resolve::mesh_path(&m))
                .and_then(|path| models.model(&path));
            for (shape, texture) in list {
                let targets: Vec<AssetId<NifMaterial>> = model
                    .iter()
                    .flat_map(|m| m.shapes.iter())
                    .filter(|s| s.name.as_deref().is_some_and(|n| same_shape(n, shape)))
                    .map(|s| s.material.id())
                    .collect();
                for (e, rv, mat, blend, name, fading, drawing) in &*drawn {
                    let Some(mat) = nvf_assets::nif_surface_blend::source_material(mat, blend) else { continue };
                    if rv.0 != r.0 {
                        continue;
                    }
                    // A mesh drawn see-through wears `Fading`'s single copy: the swap goes
                    // to its own persistent material beneath both `Fading` and any cloak
                    // override (#2981, #3105).
                    let own = nvf_play::fade::persistent_material(mat, drawing, fading).id();
                    let original = swapped.original.get(&own).copied().unwrap_or(own);
                    if !targets.contains(&original) && name.is_none_or(|n| !same_shape(&n.0, shape)) {
                        continue;
                    }
                    let key = (original, texture.clone());
                    let copy = match swapped.made.get(&key) {
                        Some(h) => h.clone(),
                        None => {
                            let a = &mut *models;
                            let Some(image) = a.textures.get(texture, true, &a.data, &mut a.images, a.formats.as_deref()) else {
                                warn!("SwapTexture: {texture} not found (reference {:08X}, shape {shape})", r.0);
                                continue;
                            };
                            let Some(mut m) = models.materials.get(original).cloned() else { continue };
                            m.base.base_color_texture = Some(image);
                            // The new diffuse's authored set, or the legacy look (#2981).
                            let a = &mut *models;
                            nvf_assets::authored::retexture(&mut m, texture, &mut a.textures, &a.data, &mut a.images, a.formats.as_deref());
                            let h = models.materials.add(m);
                            swapped.original.insert(h.id(), original);
                            swapped.made.insert(key, h.clone());
                            info!("SwapTexture: reference {:08X} shape {shape}: {texture}", r.0);
                            h
                        }
                    };
                    match fading {
                        Some(f) if f.original().id() != copy.id() => {
                            nvf_play::fade::queue_retarget(commands, e, copy);
                        }
                        Some(_) => {}
                        None if copy.id() != mat.id() => {
                            commands.entity(e).insert(MeshMaterial3d(copy));
                        }
                        None => {}
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::same_shape;
    use bevy::prelude::*;
    use fnv_sim::FormId;
    use nvf_assets::NifMaterial;

    #[derive(Resource, Default)]
    struct Swaps(fnv_game::functions::misc::TextureSwaps);

    #[derive(Resource, Default)]
    struct Alphas(std::collections::BTreeMap<FormId, f32>);

    const TEXTURE: &str = "textures\\synthetic-fade-swap.dds";
    /// An unknown reference is present, so a fade in runs for it (nvf_play fade.rs `start`).
    const REFERENCE: u32 = 0xFF00_0001;

    /// The production systems in the fixture's order (the swap before the fades' step, the
    /// actor's alpha after it; the game declares no order between the last two), a synthetic
    /// texture and one named shape wearing its own material.
    fn overlap_app() -> (App, impl Sized, Entity, Handle<NifMaterial>) {
        overlap_app_ordered(AlphaOrder::AfterFade)
    }

    /// Where the actor alpha's sync runs against the fade's systems (`PlaySet::Sync`).
    #[derive(Clone, Copy, Debug)]
    enum AlphaOrder {
        /// After them, their commands applied first: the production order (#3017).
        AfterFade,
        /// After them in the same frame without their commands applied: the fade's copy is
        /// already removed while the mesh still wears the alpha's copy.
        AfterFadeUnapplied,
    }

    fn alpha_sync(alpha: Res<Alphas>, mut drawing: crate::effect_shaders::AlphaDrawing) {
        drawing.sync(&alpha.0);
    }

    fn overlap_app_ordered(order: AlphaOrder) -> (App, impl Sized, Entity, Handle<NifMaterial>) {
        let (mut app, _files) = crate::outfits::tests::asset_fixture();
        let forms: &'static fnv_sim::FormStore = Box::leak(Box::default());
        let refs: &'static fnv_sim::RefTable = Box::leak(Box::default());
        app.insert_resource(nvf_play::sim::SimState { forms, refs, world: fnv_sim::World::new(forms, refs, Default::default()) })
            .insert_resource(crate::outfits::tests::empty_world_data())
            .init_resource::<Time<Virtual>>()
            .init_resource::<super::Swapped>()
            .init_resource::<Swaps>()
            .init_resource::<crate::effect_shaders::Shaders>()
            .init_resource::<Alphas>()
            .add_plugins(nvf_play::fade::FadePlugin)
            .add_systems(Update, (
                (|swaps: Res<Swaps>, sim: Res<nvf_play::sim::SimState>, world: Res<crate::world::WorldData>, mut drawing: super::Swapping| drawing.sync(&swaps.0, &sim, &world)).before(nvf_play::PlaySet::Sync),
            ));
        match order {
            AlphaOrder::AfterFade => app.add_systems(Update, alpha_sync.after(nvf_play::PlaySet::Sync)),
            AlphaOrder::AfterFadeUnapplied => app.add_systems(Update, alpha_sync.after_ignore_deferred(nvf_play::PlaySet::Sync)),
        };
        // [documented: engine/nvf-assets/src/texture.rs tests::dds] A synthetic
        // one-block DDS offered to the normal texture loader, no game files.
        let texture = TEXTURE;
        let mut dds = vec![0u8; 136];
        dds[..4].copy_from_slice(b"DDS ");
        for (at, value) in [(4, 124u32), (8, 0x1007), (12, 4), (16, 4), (28, 1), (76, 32), (80, 4), (108, 0x1000)] {
            dds[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        dds[84..88].copy_from_slice(b"DXT1");
        app.world_mut().resource_mut::<nvf_assets::TextureCache>().offer(texture.into(), dds, 0.0);
        // An unknown reference is present, so a fade in runs for it (nvf_play fade.rs `start`).
        let reference = REFERENCE;
        let own = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(nvf_assets::nif_material::nif_material(StandardMaterial::default()));
        let e = app.world_mut().spawn((MeshMaterial3d(own.clone()), nvf_play::RefVisual(reference), Visibility::Inherited, nvf_assets::actor::ShapeName("testshape".into()))).id();
        app.update();
        (app, _files, e, own)
    }

    /// #2981 / #3105: a mesh under `SetActorAlpha` and a presence fade at once shares one
    /// `Fading` owner, with or without a texture swap, across all eight combinations of
    /// `(swap, alpha_first, alpha_released_first)`. Both effects compose multiplicatively
    /// (`base * actor_alpha * fade`), `Fading::original()` always stays the mesh's own (or
    /// swapped) material, changing actor alpha mid-fade updates the product and survives
    /// the fade's end, and every order restores the persistent material opaque with `Fading`
    /// gone.
    #[test]
    fn a_swap_under_both_fades_copies_the_mesh_own_material_in_either_start_order() {
        const HELD: &[&str] = &[
            "swap false, alpha first true, alpha released first true",
            "swap false, alpha first true, alpha released first false",
            "swap false, alpha first false, alpha released first true",
            "swap false, alpha first false, alpha released first false",
            "swap true, alpha first true, alpha released first true",
            "swap true, alpha first true, alpha released first false",
            "swap true, alpha first false, alpha released first true",
            "swap true, alpha first false, alpha released first false",
        ];
        let mut held = Vec::new();
        for swap in [false, true] {
            for alpha_first in [true, false] {
                for alpha_released_first in [true, false] {
                    let case = format!("swap {swap}, alpha first {alpha_first}, alpha released first {alpha_released_first}");
                    let (mut app, _files, e, own) = overlap_app();
                    let (reference, texture) = (REFERENCE, TEXTURE);
                    let (own_mode, own_depth) = {
                        let m = app.world().resource::<Assets<NifMaterial>>().get(&own).unwrap().clone();
                        (m.base.alpha_mode, m.extension.depth_write)
                    };
                    let start_alpha = |app: &mut App| {
                        app.world_mut().resource_mut::<Alphas>().0.insert(FormId(reference), 0.5);
                        app.update();
                    };
                    let start_fade = |app: &mut App| {
                        app.world_mut().write_message(nvf_play::fade::RefFade { form_id: reference, fade_in: true });
                        app.update();
                    };
                    if alpha_first {
                        start_alpha(&mut app);
                        start_fade(&mut app);
                    } else {
                        start_fade(&mut app);
                        start_alpha(&mut app);
                    }
                    let fading = app.world().get::<nvf_play::fade::Fading>(e).unwrap_or_else(|| panic!("{case}: single owner active"));
                    assert_eq!(fading.original(), &own, "{case}: original is always the mesh's own material");
                    assert_eq!(fading.actor_alpha(), Some(0.5), "{case}");
                    assert!(fading.presence().is_some(), "{case}");

                    // Partway through fade in (0.5 s / 2.0 s = 0.25): drawn alpha = 1.0 * 0.5 * 0.25.
                    app.world_mut().resource_mut::<Time<Virtual>>().advance_by(std::time::Duration::from_secs_f32(0.5));
                    app.update();
                    let worn_mid = app.world().get::<MeshMaterial3d<NifMaterial>>(e).unwrap().0.clone();
                    let mid_alpha = app.world().resource::<Assets<NifMaterial>>().get(&worn_mid).unwrap().base.base_color.alpha();
                    assert!((mid_alpha - 0.125).abs() < 1e-5, "{case}: mid fade-in alpha {mid_alpha}");

                    // Change actor alpha while the fade runs (0.5 -> 0.8): drawn alpha = 0.8 * 0.25 = 0.2.
                    app.world_mut().resource_mut::<Alphas>().0.insert(FormId(reference), 0.8);
                    app.update();
                    let changed_alpha = app.world().resource::<Assets<NifMaterial>>().get(&worn_mid).unwrap().base.base_color.alpha();
                    assert!((changed_alpha - 0.2).abs() < 1e-5, "{case}: updated actor alpha * fade {changed_alpha}");

                    let expected = if swap {
                        {
                            let mut swaps = app.world_mut().resource_mut::<Swaps>();
                            swaps.0.by_ref.insert(FormId(reference), vec![("testshape".into(), texture.into())]);
                            swaps.0.generation += 1;
                        }
                        app.update();
                        let swapped = app.world().resource::<super::Swapped>();
                        assert_eq!(swapped.made.len(), 1, "{case}");
                        let copy = swapped.made.values().next().unwrap().clone();
                        assert_eq!(swapped.original.get(&copy.id()), Some(&own.id()), "{case}: the swap copied the model's material");
                        let materials = app.world().resource::<Assets<NifMaterial>>();
                        let m = materials.get(&copy).unwrap();
                        assert!(m.base.base_color_texture.is_some(), "{case}: the new diffuse");
                        assert!(materials.get(&own).unwrap().base.base_color_texture.is_none(), "{case}");
                        assert_eq!((m.base.base_color.alpha(), m.base.alpha_mode, m.extension.depth_write), (1.0, own_mode, own_depth), "{case}: no fade's alpha baked into the swapped material");
                        // The live see-through copy keeps the combined opacity (0.2) with the swapped texture.
                        let drawn_mat = materials.get(&worn_mid).unwrap();
                        assert!(drawn_mat.base.base_color_texture.is_some(), "{case}: live copy rebuilt with swapped diffuse");
                        assert!((drawn_mat.base.base_color.alpha() - 0.2).abs() < 1e-5, "{case}: live copy keeps combined opacity");
                        copy
                    } else {
                        own.clone()
                    };
                    let release_alpha = |app: &mut App| {
                        app.world_mut().resource_mut::<Alphas>().0.clear();
                        app.update();
                    };
                    let end_fade = |app: &mut App| {
                        app.world_mut().resource_mut::<Time<Virtual>>().advance_by(std::time::Duration::from_secs_f32(3.0));
                        app.update();
                    };
                    if alpha_released_first {
                        release_alpha(&mut app);
                        let after_first = app.world().resource::<Assets<NifMaterial>>().get(&worn_mid).unwrap().base.base_color.alpha();
                        assert!((after_first - 0.25).abs() < 1e-5, "{case}: fade remains at 0.25 after alpha ends, got {after_first}");
                        end_fade(&mut app);
                    } else {
                        end_fade(&mut app);
                        let after_first = app.world().resource::<Assets<NifMaterial>>().get(&worn_mid).unwrap().base.base_color.alpha();
                        assert!((after_first - 0.8).abs() < 1e-5, "{case}: updated actor alpha 0.8 remains after fade ends, got {after_first}");
                        release_alpha(&mut app);
                    }
                    app.update();
                    app.update();
                    let worn = app.world().get::<MeshMaterial3d<NifMaterial>>(e).unwrap().0.clone();
                    let opaque = app.world().resource::<Assets<NifMaterial>>().get(&worn).is_some_and(|m| (m.base.base_color.alpha(), m.base.alpha_mode, m.extension.depth_write) == (1.0, own_mode, own_depth));
                    let unmarked = app.world().get::<nvf_play::fade::Fading>(e).is_none();
                    if worn == expected && opaque && unmarked {
                        held.push(case);
                    }
                }
            }
        }
        assert_eq!(held, HELD, "all eight orders give the mesh back whole");
    }

    /// #3105: fade-out composes multiplicatively with `SetActorAlpha` and base opacity, and
    /// both ending in the same update always restores the mesh's own material regardless of
    /// deferred-command flush boundaries (`AfterFade` and `AfterFadeUnapplied`).
    #[test]
    fn fade_out_composes_and_two_ends_in_one_update_restore_own_material() {
        for order in [AlphaOrder::AfterFade, AlphaOrder::AfterFadeUnapplied] {
            let (mut app, _files, e, own) = overlap_app_ordered(order);
            // Set base opacity 0.8 and mark the reference disabled so a fade-out runs.
            app.world_mut().resource_mut::<Assets<NifMaterial>>().get_mut(&own).unwrap().base.base_color.set_alpha(0.8);
            let mut placed = crate::outfits::tests::placed_ref(REFERENCE, *b"REFR");
            placed.flags |= fnv_sim::refs::FLAG_INITIALLY_DISABLED;
            let mut refs = fnv_sim::RefTable::default();
            refs.refs.insert(FormId(REFERENCE), placed);
            let forms: &'static fnv_sim::FormStore = Box::leak(Box::default());
            let refs: &'static fnv_sim::RefTable = Box::leak(Box::new(refs));
            app.insert_resource(nvf_play::sim::SimState { forms, refs, world: fnv_sim::World::new(forms, refs, Default::default()) });
            app.world_mut().resource_mut::<Alphas>().0.insert(FormId(REFERENCE), 0.5);
            app.world_mut().write_message(nvf_play::fade::RefFade { form_id: REFERENCE, fade_in: false });
            app.update();
            // Partway through fade out (0.5 s / 2.0 s -> presence 0.75): 0.8 * 0.5 * 0.75 = 0.3.
            app.world_mut().resource_mut::<Time<Virtual>>().advance_by(std::time::Duration::from_secs_f32(0.5));
            app.update();
            let copy = app.world().get::<nvf_play::fade::Fading>(e).expect("active").copy().clone();
            let alpha = app.world().resource::<Assets<NifMaterial>>().get(&copy).unwrap().base.base_color.alpha();
            assert!((alpha - 0.3).abs() < 1e-5, "{order:?}: fade-out * actor_alpha * base = {alpha}");
            // End BOTH in the same update.
            app.world_mut().resource_mut::<Time<Virtual>>().advance_by(std::time::Duration::from_secs_f32(3.0));
            app.world_mut().resource_mut::<Alphas>().0.clear();
            app.update();
            assert!(app.world().get::<nvf_play::fade::Fading>(e).is_none(), "{order:?}: Fading removed");
            assert!(!app.world().resource::<Assets<NifMaterial>>().contains(copy.id()), "{order:?}: copy removed");
            assert_eq!(app.world().get::<MeshMaterial3d<NifMaterial>>(e).unwrap().0, own, "{order:?}: own restored");
            assert_eq!(app.world().resource::<Assets<NifMaterial>>().get(&own).unwrap().base.base_color.alpha(), 0.8);
        }
    }

    /// #3105: a model-backed unnamed shape (matched via its model's material handle, with
    /// no `ShapeName` component) swaps cleanly under overlapping fades and restores the
    /// swapped material in all four start/end orders.
    #[test]
    fn model_backed_unnamed_shape_swaps_and_restores_across_overlapping_fades() {
        use crate::authored_test_records::*;
        const BASE: u32 = 0x901;
        const REF_ID: u32 = 0x902;
        const CELL: u32 = 0x903;
        let raw = [
            tes4(),
            top(b"STAT", &record(b"STAT", 0, BASE, &[sub(b"EDID", z("SwapStat")), sub(b"MODL", z("bare.nif"))])),
            top(b"CELL", &group([0; 4], 2, &group([0; 4], 3, &[
                record(b"CELL", 0, CELL, &[]),
                group(CELL.to_le_bytes(), 6, &group(CELL.to_le_bytes(), 9, &record(b"REFR", 0, REF_ID, &[
                    sub(b"NAME", u32s(&[BASE])),
                    sub(b"DATA", vec![0; 24]),
                ]))),
            ].concat()))),
        ].concat();
        let plugins: &'static Vec<fnv_esm::Plugin> = Box::leak(Box::new(vec![fnv_esm::Plugin::parse(&raw, "SwapTest.esm".into()).unwrap()]));
        let lo: &'static fnv_esm::LoadOrder = Box::leak(Box::new(fnv_esm::LoadOrder::new(plugins).unwrap()));
        let esm_schema: &'static fnv_esm::Schema = Box::leak(Box::new(fnv_esm::Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap()));
        let forms: &'static fnv_sim::FormStore = Box::leak(Box::new(fnv_sim::FormStore::build(lo, esm_schema)));
        let refs: &'static fnv_sim::RefTable = Box::leak(Box::new(fnv_sim::RefTable::build(lo, esm_schema)));

        for alpha_first in [true, false] {
            for alpha_released_first in [true, false] {
                let (mut app, _files) = crate::outfits::tests::asset_fixture();
                app.insert_resource(nvf_play::sim::SimState { forms, refs, world: fnv_sim::World::new(forms, refs, Default::default()) })
                    .insert_resource(crate::world::WorldData { lo, schema: esm_schema, ..crate::outfits::tests::empty_world_data() })
                    .init_resource::<Time<Virtual>>()
                    .init_resource::<super::Swapped>()
                    .init_resource::<Swaps>()
                    .init_resource::<crate::effect_shaders::Shaders>()
                    .init_resource::<Alphas>()
                    .add_plugins(nvf_play::fade::FadePlugin)
                    .add_systems(Update, (
                        (|swaps: Res<Swaps>, sim: Res<nvf_play::sim::SimState>, world: Res<crate::world::WorldData>, mut drawing: super::Swapping| drawing.sync(&swaps.0, &sim, &world)).before(nvf_play::PlaySet::Sync),
                        alpha_sync.after(nvf_play::PlaySet::Sync),
                    ));
                let mut dds = vec![0u8; 136];
                dds[..4].copy_from_slice(b"DDS ");
                for (at, value) in [(4, 124u32), (8, 0x1007), (12, 4), (16, 4), (28, 1), (76, 32), (80, 4), (108, 0x1000)] {
                    dds[at..at + 4].copy_from_slice(&value.to_le_bytes());
                }
                dds[84..88].copy_from_slice(b"DXT1");
                app.world_mut().resource_mut::<nvf_assets::TextureCache>().offer(TEXTURE.into(), dds, 0.0);
                // Load the real model ("meshes\\bare.nif") and spawn an unnamed mesh wearing its shape's material.
                let own = {
                    use bevy::ecs::system::RunSystemOnce;
                    app.world_mut().run_system_once(|mut models: nvf_assets::model::ModelCtx| {
                        models.model("meshes\\bare.nif").expect("fixture model").shapes[0].material.clone()
                    }).unwrap()
                };
                // Intentionally NO `ShapeName` component: matched only via `model.shapes` handle!
                let e = app.world_mut().spawn((MeshMaterial3d(own.clone()), nvf_play::RefVisual(REF_ID), Visibility::Inherited)).id();
                app.update();

                if alpha_first {
                    app.world_mut().resource_mut::<Alphas>().0.insert(FormId(REF_ID), 0.5);
                    app.update();
                    app.world_mut().write_message(nvf_play::fade::RefFade { form_id: REF_ID, fade_in: true });
                    app.update();
                } else {
                    app.world_mut().write_message(nvf_play::fade::RefFade { form_id: REF_ID, fade_in: true });
                    app.update();
                    app.world_mut().resource_mut::<Alphas>().0.insert(FormId(REF_ID), 0.5);
                    app.update();
                }
                app.world_mut().resource_mut::<Time<Virtual>>().advance_by(std::time::Duration::from_secs_f32(1.0));
                app.update();
                // Swap shape "bare" while both effects overlap.
                {
                    let mut swaps = app.world_mut().resource_mut::<Swaps>();
                    swaps.0.by_ref.insert(FormId(REF_ID), vec![("bare".into(), TEXTURE.into())]);
                    swaps.0.generation += 1;
                }
                app.update();
                let swapped_copy = app.world().resource::<super::Swapped>().made.values().next().expect("unnamed shape matched model material").clone();
                assert_eq!(app.world().get::<nvf_play::fade::Fading>(e).unwrap().original(), &swapped_copy);
                // Mid fade-in (1.0 / 2.0 = 0.5) * actor alpha 0.5 = 0.25.
                let copy = app.world().get::<nvf_play::fade::Fading>(e).unwrap().copy().clone();
                let mid = app.world().resource::<Assets<NifMaterial>>().get(&copy).unwrap().base.base_color.alpha();
                assert!((mid - 0.25).abs() < 1e-5, "unnamed shape combined alpha {mid}");

                if alpha_released_first {
                    app.world_mut().resource_mut::<Alphas>().0.clear();
                    app.update();
                    app.world_mut().resource_mut::<Time<Virtual>>().advance_by(std::time::Duration::from_secs_f32(3.0));
                    app.update();
                } else {
                    app.world_mut().resource_mut::<Time<Virtual>>().advance_by(std::time::Duration::from_secs_f32(3.0));
                    app.update();
                    app.world_mut().resource_mut::<Alphas>().0.clear();
                    app.update();
                }
                app.update();
                assert!(app.world().get::<nvf_play::fade::Fading>(e).is_none());
                assert_eq!(app.world().get::<MeshMaterial3d<NifMaterial>>(e).unwrap().0, swapped_copy);
                assert_eq!(app.world().resource::<Assets<NifMaterial>>().get(&swapped_copy).unwrap().base.base_color.alpha(), 1.0);
            }
        }
    }

    /// #2981 / #3105: the actor's alpha spans two presence fades. Both share one `Fading`
    /// whose original stays `own` across the first fade's end and the second fade, so a
    /// swap during the second fade copies `own` with no fade alpha baked in.
    #[test]
    fn a_swap_during_a_second_fade_under_one_actor_alpha_copies_the_mesh_own_material() {
        let (mut app, _files, e, own) = overlap_app();
        let fade = |app: &mut App| {
            app.world_mut().write_message(nvf_play::fade::RefFade { form_id: REFERENCE, fade_in: true });
            app.update();
        };
        fade(&mut app);
        app.world_mut().resource_mut::<Alphas>().0.insert(FormId(REFERENCE), 0.5);
        app.update();
        app.world_mut().resource_mut::<Time<Virtual>>().advance_by(std::time::Duration::from_secs_f32(3.0));
        app.update();
        app.update();
        let f = app.world().get::<nvf_play::fade::Fading>(e).expect("alpha still on");
        assert!(f.presence().is_none(), "the first fade is over");
        assert_eq!(f.actor_alpha(), Some(0.5));
        assert_eq!(f.original(), &own, "the single owner kept the mesh's own material");
        fade(&mut app);
        assert!(app.world().get::<nvf_play::fade::Fading>(e).unwrap().presence().is_some(), "the second fade runs");
        {
            let mut swaps = app.world_mut().resource_mut::<Swaps>();
            swaps.0.by_ref.insert(FormId(REFERENCE), vec![("testshape".into(), TEXTURE.into())]);
            swaps.0.generation += 1;
        }
        app.update();
        let swapped = app.world().resource::<super::Swapped>();
        assert_eq!(swapped.made.len(), 1, "the swap was made");
        let copy = swapped.made.values().next().unwrap().clone();
        assert_eq!(swapped.original.get(&copy.id()), Some(&own.id()), "the swap copied the model's material");
        let m = app.world().resource::<Assets<NifMaterial>>().get(&copy).unwrap();
        assert!(m.base.base_color_texture.is_some(), "the new diffuse");
        assert_eq!(m.base.base_color.alpha(), 1.0, "no fade's alpha baked in");
    }

    /// #3017 / #3105: an actor alpha started while a presence fade draws the mesh shares
    /// the single `Fading` copy (`original == own`). Both in `AfterFade` and in
    /// `AfterFadeUnapplied`, ending the fade leaves the mesh at actor alpha 0.7 and
    /// releasing actor alpha restores `own`, opaque.
    #[test]
    fn an_alpha_copy_made_during_a_fade_takes_the_material_the_fade_gives_back() {
        for order in [AlphaOrder::AfterFade, AlphaOrder::AfterFadeUnapplied] {
            let (mut app, _files, e, own) = overlap_app_ordered(order);
            app.world_mut().write_message(nvf_play::fade::RefFade { form_id: REFERENCE, fade_in: true });
            app.update();
            app.world_mut().resource_mut::<Alphas>().0.insert(FormId(REFERENCE), 0.7);
            app.update();
            assert_eq!(app.world().get::<nvf_play::fade::Fading>(e).unwrap().original(), &own, "{order:?}: single owner holds own");
            app.world_mut().resource_mut::<Time<Virtual>>().advance_by(std::time::Duration::from_secs_f32(3.0));
            app.update();
            app.update();
            let f = app.world().get::<nvf_play::fade::Fading>(e).expect("alpha still on");
            assert!(f.presence().is_none(), "{order:?}: the fade is over");
            assert_eq!(f.original(), &own, "{order:?}: the own material is the original");
            let worn = app.world().get::<MeshMaterial3d<NifMaterial>>(e).unwrap().0.clone();
            let materials = app.world().resource::<Assets<NifMaterial>>();
            let m = materials.get(&worn).unwrap_or_else(|| panic!("{order:?}: the worn material exists"));
            assert!((m.base.base_color.alpha() - 0.7).abs() < 1e-6, "{order:?}: one 0.7 copy, not a copy of a copy");
            app.world_mut().resource_mut::<Alphas>().0.clear();
            app.update();
            app.update();
            assert!(app.world().get::<nvf_play::fade::Fading>(e).is_none());
            assert_eq!(app.world().get::<MeshMaterial3d<NifMaterial>>(e).unwrap().0, own, "{order:?}: the release gives back the own material");
            assert_eq!(app.world().resource::<Assets<NifMaterial>>().get(&own).unwrap().base.base_color.alpha(), 1.0);
        }
    }

    #[test]
    fn a_swap_finds_shapes_named_with_or_without_hashes() {
        // The end slides' shape keeps its `##` in the model; the call's name has none.
        assert!(same_shape("##GS01:0", "gs01:0"));
        assert!(same_shape("PipBoyCard01:0", "pipboycard01:0"));
        assert!(!same_shape("##GS01:1", "gs01:0"));
        assert!(!same_shape("GS01", "gs01:0"));
    }
}
