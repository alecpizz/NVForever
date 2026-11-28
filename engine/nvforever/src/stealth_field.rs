//! [design, #2557] Loaded living actors also use this field at Chameleon >= 1,
//! following their ActorRig visual roots (actors.rs); the player path stays separate.
//!
//! The Stealth Boy's look on the player (docs/spec-notes/aid-items.md §9,
//! docs/spec-notes/refraction.md §5): while the player's Chameleon actor value is 1 or
//! more (`fnv_game::aid`, the Stealth Boy's "Stealth Field" effect), the player's body
//! and arms rig are drawn refracting: each mesh shows what lies behind it, bent by its own
//! normals and normal map, with the normal map scrolling so the bend shimmers
//! (`nvf_assets::nif_material`, "Refraction" and "Fire refraction").
//!
//! [hypothesis: the Stealth Field's MGEF names no effect shader (archetype and actor value
//! Chameleon only) [observed], so the look is the engine's; strength [`STRENGTH`] and
//! period [`PERIOD`] are chosen near the game's own refracting shapes (0.04 for Mister
//! Handy's jet, 0.056 for the Incinerator's burst; docs/spec-notes/refraction.md).] Every
//! mesh gets a refracting copy of its material; the copies are dropped and the materials
//! put back when the effect ends. Detection does not read this: it reads the Chameleon
//! value itself (`fnv_game::detection`).
//!
//! The Pip-Boy's screen and its glare are the one exception: while the menus are on it
//! (`pipboy_arm.rs`) they are drawn plain, so the menus can be read and used; the rest of
//! the arm and the Pip-Boy stay see-through, and both refract again once the Pip-Boy is
//! down (owner, 2026-10-23) [design].
//!
//! Log lines: `stealth field: on (Chameleon 75): <n> meshes see-through (refracting,
//! strength 0.030)` and `stealth field: off`.

use crate::limbs::ActorRig;
use crate::pipboy_arm::PipBoyPart;
use crate::player_body::PlayerBody;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use fnv_sim::FormId;
use nvf_assets::nif_material::{MaterialOverride, NifMaterial};
use nvf_play::pipboy::PipBoyScreen;
use nvf_play::script::ScriptState;
use nvf_play::RefVisual;
use std::collections::{HashMap, HashSet};

/// The Chameleon actor value [documented: xEdit ActorValue, `schemas/fnv/common.json`].
const CHAMELEON: u16 = 49;

/// The screen shift of the field: screen UV per unit of view-space normal [hypothesis,
/// docs/spec-notes/refraction.md §5]; the material's strength is this over
/// `nif_material::REFRACTION_SCREEN`.
pub const STRENGTH: f32 = 0.03;

/// Seconds for the normal maps to scroll once, the shimmer [hypothesis].
pub const PERIOD: f32 = 2.0;

/// The cut of a blended part (hair, eyelashes) drawn refracting [hypothesis].
const BLEND_CUT: f32 = 0.5;

/// The meshes made see-through and their own materials.
#[derive(Resource, Default)]
pub struct StealthField {
    pub on: bool,
    originals: HashMap<Entity, Handle<NifMaterial>>,
}

pub struct StealthFieldPlugin;

impl Plugin for StealthFieldPlugin {
    fn build(&self, app: &mut App) {
        // After the Pip-Boy has put the menus on its screen or taken them off, so the
        // screen is skipped exactly while the menus are on it.
        app.init_resource::<StealthField>()
            .init_resource::<ActorFields>()
            .add_systems(Update, apply.after(nvf_play::player::BodySet).after(crate::pipboy_arm::show_screen))
            .add_systems(PostUpdate, apply_actors);
    }
}

impl StealthField {
    /// Give `e` its own material back and stop making it see-through: the material it
    /// had before the field, or `None` when the field had not touched it. The
    /// see-through copy is dropped.
    pub fn take_original(&mut self, e: Entity, current: &Handle<NifMaterial>, materials: &mut Assets<NifMaterial>) -> Option<Handle<NifMaterial>> {
        let original = self.originals.remove(&e)?;
        materials.remove(current);
        Some(original)
    }

    /// As the field does when it makes `e` see-through, for a test of what takes it back.
    #[cfg(test)]
    pub(crate) fn keep_original(&mut self, e: Entity, original: Handle<NifMaterial>) {
        self.originals.insert(e, original);
    }
}

/// The see-through copy of a material: refracting, keeping the outline of its cut.
pub fn see_through(m: &NifMaterial) -> NifMaterial {
    let mut m = m.clone();
    let cut = match m.base.alpha_mode {
        AlphaMode::Mask(cut) => cut,
        AlphaMode::Opaque => m.extension.alpha_cut,
        _ => m.extension.alpha_cut.max(BLEND_CUT),
    };
    nvf_assets::nif_spawn::refraction_material(&mut m, STRENGTH / nvf_assets::nif_material::REFRACTION_SCREEN, PERIOD);
    m.extension.alpha_cut = cut;
    m
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn apply(
    script: Option<Res<ScriptState>>,
    body: Res<PlayerBody>,
    pip_screen: Option<Res<PipBoyScreen>>,
    parts: Query<&PipBoyPart>,
    mut field: ResMut<StealthField>,
    children: Query<&Children>,
    mut meshes: Query<&mut MeshMaterial3d<NifMaterial>>,
    mut materials: ResMut<Assets<NifMaterial>>,
) {
    let chameleon = script.as_ref().and_then(|s| s.rt.as_ref()).map_or(0.0, |rt| fnv_game::functions::actor_value_temporary(&rt.game, FormId::PLAYER_REF, CHAMELEON));
    // The player's tweak drops the shimmer while the Pip-Boy is up [design: docs/design/tweaks.md].
    let in_pipboy = pip_screen.as_ref().is_some_and(|s| s.on_model) && script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.no_stealth_shimmer_in_pipboy);
    let want = chameleon >= 1.0 && !in_pipboy;
    if want {
        // Every frame: a weapon drawn or an outfit changed brings new meshes.
        let mut fresh = 0;
        let menus_on_screen = pip_screen.is_some_and(|s| s.on_model);
        for root in body.roots() {
            for e in children.iter_descendants(root) {
                if field.originals.contains_key(&e) || (menus_on_screen && matches!(parts.get(e), Ok(PipBoyPart::Screen | PipBoyPart::Glare))) {
                    continue;
                }
                let Ok(mut handle) = meshes.get_mut(e) else { continue };
                let Some(copy) = materials.get(&handle.0).map(see_through) else { continue };
                let copy = materials.add(copy);
                field.originals.insert(e, std::mem::replace(&mut handle.0, copy));
                fresh += 1;
            }
        }
        if !field.on {
            info!("stealth field: on (Chameleon {chameleon:.0}): {fresh} meshes see-through (refracting, strength {STRENGTH:.3})");
            field.on = true;
        }
    } else if field.on {
        for (e, original) in field.originals.drain() {
            if let Ok(mut handle) = meshes.get_mut(e) {
                materials.remove(&handle.0);
                handle.0 = original;
            }
        }
        field.on = false;
        info!("stealth field: off");
    }
    // Meshes despawned meanwhile.
    field.originals.retain(|e, _| meshes.contains(*e));
}

/// [design, #2557] Separate from the player's field and Pip-Boy material ownership.
#[derive(Resource, Default)]
pub(crate) struct ActorFields(HashMap<Entity, ActorField>);

struct ActorField {
    reference: u32,
    name: String,
    originals: HashMap<Entity, (Handle<NifMaterial>, Handle<NifMaterial>)>,
    pending: bool,
}

/// [design, #2557] Inputs from the loaded visual root and the script runtime.
#[derive(Clone)]
struct ActorTarget {
    root: Entity,
    reference: u32,
    name: String,
    chameleon: f64,
    dead: bool,
}

#[derive(SystemParam)]
struct ActorDrawing<'w, 's> {
    commands: Commands<'w, 's>,
    fields: ResMut<'w, ActorFields>,
    fading: Query<'w, 's, (Entity, &'static nvf_play::fade::Fading)>,
    children: Query<'w, 's, &'static Children>,
    removed_children: RemovedComponents<'w, 's, Children>,
    parents: Query<'w, 's, &'static ChildOf>,
    #[allow(clippy::type_complexity)]
    meshes: ParamSet<'w, 's, (
        Query<'w, 's, Entity, Or<(Changed<Children>, Changed<ChildOf>, Changed<MeshMaterial3d<NifMaterial>>)>>,
        Query<'w, 's, &'static mut MeshMaterial3d<NifMaterial>>,
    )>,
    materials: ResMut<'w, Assets<NifMaterial>>,
}

impl ActorDrawing<'_, '_> {
    /// [design, #2557] Never remove a material supplied by another rendering system.
    fn restore(&mut self, e: Entity, original: Handle<NifMaterial>, copy: Handle<NifMaterial>) {
        let mut meshes = self.meshes.p1();
        if let Ok(mut current) = meshes.get_mut(e) {
            if current.0 == copy {
                current.0 = original;
            }
            self.commands.entity(e).remove::<MaterialOverride>();
        }
        self.materials.remove(&copy);
    }

    fn sync(&mut self, targets: impl IntoIterator<Item = ActorTarget>) {
        // Fast path: when no actor currently has an active stealth field, check whether
        // any target requests one before scanning world-wide Changed<Children/ChildOf/Material>
        // queries or walking ancestor chains [design, #2557].
        if self.fields.0.is_empty() {
            let mut active_targets: Vec<ActorTarget> = Vec::new();
            for target in targets {
                if !target.dead && target.chameleon >= 1.0 {
                    active_targets.push(target);
                }
            }
            if active_targets.is_empty() {
                // Drain removed-children events so they do not accumulate across idle frames.
                for _ in self.removed_children.read() {}
                return;
            }
            // Fall through with only the actors requesting a fresh stealth field; no existing
            // fields need dirty-ancestor tracking yet.
            for _ in self.removed_children.read() {}
            for target in active_targets {
                let ActorTarget { root, reference, name, chameleon, .. } = target;
                let mut field = ActorField { reference, name, originals: HashMap::new(), pending: false };
                let descendants: HashSet<_> = std::iter::once(root).chain(self.children.iter_descendants(root)).collect();
                for e in descendants {
                    let mut meshes = self.meshes.p1();
                    let Ok(mut current) = meshes.get_mut(e) else {
                        continue;
                    };
                    let Some(copy) = self.materials.get(&current.0).map(see_through) else {
                        field.pending = true;
                        continue;
                    };
                    let copy = self.materials.add(copy);
                    let original = std::mem::replace(&mut current.0, copy.clone());
                    self.commands.entity(e).insert(MaterialOverride { source: original.clone(), copy: copy.clone() });
                    field.originals.insert(e, (original, copy));
                }
                info!("stealth field: {} 0x{reference:08X} on (Chameleon {chameleon:.0})", field.name);
                self.fields.0.insert(root, field);
            }
            return;
        }
        // [design, #2557] Hierarchy edits and new/replaced material handles dirty only
        // their ancestors. Idle actors never walk descendants or access materials.
        let mut dirty = HashSet::new();
        for e in &self.meshes.p0() {
            dirty.insert(e);
            dirty.extend(self.parents.iter_ancestors(e));
        }
        // [documented: Bevy 0.20.1, bevy_ecs/src/relationship/mod.rs] Losing the last
        // child removes Children. [design, #2557] Dirty that former owner too:
        // Changed<Children> misses it during a transfer.
        for e in self.removed_children.read() {
            dirty.insert(e);
            dirty.extend(self.parents.iter_ancestors(e));
        }
        // [design, #2557; nvf_play::fade::step] Fades animate their underlying copy
        // in place. Refresh only these active meshes, keeping idle actors untouched.
        for (e, _) in &self.fading {
            dirty.insert(e);
            dirty.extend(self.parents.iter_ancestors(e));
        }
        let targets: Vec<_> = targets.into_iter().collect();
        let loaded: HashSet<_> = targets.iter().map(|t| t.root).collect();
        // [design, #2557] Restore detached parts before another actor can adopt them.
        let roots: Vec<_> = self.fields.0.keys().copied().collect();
        for root in roots {
            if !loaded.contains(&root) {
                let field = self.fields.0.remove(&root).unwrap();
                for (e, (original, copy)) in field.originals {
                    self.restore(e, original, copy);
                }
                info!("stealth field: {} 0x{:08X} off", field.name, field.reference);
            } else if dirty.contains(&root) {
                let descendants: HashSet<_> = std::iter::once(root).chain(self.children.iter_descendants(root)).collect();
                let mut field = self.fields.0.remove(&root).unwrap();
                let detached: Vec<_> = field.originals.keys().copied().filter(|e| !descendants.contains(e)).collect();
                for e in detached {
                    let (original, copy) = field.originals.remove(&e).unwrap();
                    self.restore(e, original, copy);
                }
                self.fields.0.insert(root, field);
            }
        }
        for target in targets {
            let ActorTarget { root, reference, name, chameleon, dead } = target;
            let want = !dead && chameleon >= 1.0;
            if !want {
                if let Some(field) = self.fields.0.remove(&root) {
                    for (e, (original, copy)) in field.originals {
                        self.restore(e, original, copy);
                    }
                    info!("stealth field: {} 0x{:08X} off", field.name, field.reference);
                }
                continue;
            }
            let fresh = !self.fields.0.contains_key(&root);
            if !fresh && !dirty.contains(&root) && !self.fields.0[&root].pending {
                continue;
            }
            let mut field = self.fields.0.remove(&root).unwrap_or_else(|| ActorField { reference, name, originals: HashMap::new(), pending: false });
            let descendants: HashSet<_> = std::iter::once(root).chain(self.children.iter_descendants(root)).collect();
            field.pending = false;
            for e in descendants {
                let mut meshes = self.meshes.p1();
                let Ok(mut current) = meshes.get_mut(e) else {
                    if let Some((_, copy)) = field.originals.remove(&e) {
                        self.materials.remove(&copy);
                    }
                    continue;
                };
                if let Some((source, copy)) = field.originals.get(&e).filter(|(source, copy)| current.0 == *copy || current.0 == *source) {
                    if current.0 == *source || self.fading.get(e).is_ok_and(|(_, f)| f.copy() == source) {
                        if let Some(material) = self.materials.get(source).map(see_through) {
                            if let Some(mut drawn) = self.materials.get_mut(copy) {
                                *drawn = material;
                            }
                        }
                        current.0 = copy.clone();
                    }
                    continue;
                }
                // [design, #2557] A replacement becomes the new original, so outfit or
                // texture changes survive uncloaking; dispose only of our old copy.
                if let Some((_, copy)) = field.originals.remove(&e) {
                    self.materials.remove(&copy);
                }
                let Some(copy) = self.materials.get(&current.0).map(see_through) else {
                    field.pending = true;
                    continue;
                };
                let copy = self.materials.add(copy);
                let original = std::mem::replace(&mut current.0, copy.clone());
                self.commands.entity(e).insert(MaterialOverride { source: original.clone(), copy: copy.clone() });
                field.originals.insert(e, (original, copy));
            }
            if fresh {
                info!("stealth field: {} 0x{reference:08X} on (Chameleon {chameleon:.0})", field.name);
            }
            self.fields.0.insert(root, field);
        }
    }
}

/// [design, #2557; actors.rs] ActorRig + RefVisual identifies loaded actor visual roots,
/// including bind-pose actors. PostUpdate sees scripts and newly attached outfits/weapons.
fn apply_actors(script: Option<Res<ScriptState>>, actors: Query<(Entity, &ActorRig), With<RefVisual>>, mut drawing: ActorDrawing) {
    let game = script.as_ref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game);
    drawing.sync(actors.iter().filter(|(_, rig)| rig.reference != FormId::PLAYER_REF.0).map(|(root, rig)| {
        let reference = FormId(rig.reference);
        let dead = game.is_some_and(|g| g.is_dead(reference));
        let chameleon = if dead { 0.0 } else { game.map_or(0.0, |g| fnv_game::functions::actor_value_temporary(g, reference, CHAMELEON)) };
        let name = if !dead && chameleon >= 1.0 {
            game.map_or_else(|| format!("0x{:08X}", rig.base), |g| g.data.name(FormId(rig.base)))
        } else {
            String::new()
        };
        ActorTarget { root, reference: rig.reference, name, chameleon, dead }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Resource, Default)]
    struct Targets(Vec<ActorTarget>);

    #[derive(Resource, Default)]
    struct Writes(usize);

    fn app() -> App {
        with_drawing(App::new())
    }

    fn with_drawing(mut app: App) -> App {
        app.init_resource::<ActorFields>()
            .init_resource::<Assets<NifMaterial>>()
            .init_resource::<Targets>()
            .init_resource::<Writes>()
            .add_systems(PostUpdate, (
                |targets: Res<Targets>, mut drawing: ActorDrawing| drawing.sync(targets.0.iter().cloned()),
                |changed: Query<Entity, Changed<MeshMaterial3d<NifMaterial>>>, mut writes: ResMut<Writes>| writes.0 = changed.iter().count(),
            ).chain());
        app
    }

    fn with_fades(app: &mut App) {
        let forms: &'static fnv_sim::FormStore = Box::leak(Box::default());
        let refs: &'static fnv_sim::RefTable = Box::leak(Box::default());
        app.insert_resource(nvf_play::sim::SimState { forms, refs, world: fnv_sim::World::new(forms, refs, Default::default()) })
            .init_resource::<Time<Virtual>>()
            .add_plugins(nvf_play::fade::FadePlugin);
    }

    fn start_fade(app: &mut App, body: Entity) {
        let reference = app.world().resource::<Targets>().0[0].reference;
        app.world_mut().entity_mut(body).insert((RefVisual(reference), Visibility::Inherited));
        app.world_mut().write_message(nvf_play::fade::RefFade { form_id: reference, fade_in: true });
        app.update();
    }

    fn advance_fade(app: &mut App, seconds: f32) {
        app.world_mut().resource_mut::<Time<Virtual>>().advance_by(std::time::Duration::from_secs_f32(seconds));
        app.update();
    }

    fn actor(app: &mut App, chameleon: f64) -> Entity {
        let root = app.world_mut().spawn_empty().id();
        let reference = 0xFF000000 + app.world().resource::<Targets>().0.len() as u32;
        app.world_mut().resource_mut::<Targets>().0.push(ActorTarget { root, reference, name: "TestActor".into(), chameleon, dead: false });
        root
    }

    fn mesh(app: &mut App, parent: Entity) -> (Entity, Handle<NifMaterial>) {
        let material = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(nvf_assets::nif_material::nif_material(StandardMaterial::default()));
        let e = app.world_mut().spawn((ChildOf(parent), MeshMaterial3d(material.clone()))).id();
        (e, material)
    }

    fn worn(app: &App, e: Entity) -> Handle<NifMaterial> {
        app.world().get::<MeshMaterial3d<NifMaterial>>(e).unwrap().0.clone()
    }

    fn refracts(app: &App, e: Entity) -> bool {
        app.world().resource::<Assets<NifMaterial>>().get(&worn(app, e)).unwrap().extension.refract
    }

    #[test]
    fn actors_cloak_independently_at_one_and_restore_on_drop_death_and_recloak() {
        let mut app = app();
        let a = actor(&mut app, 95.0);
        let b = actor(&mut app, 0.5);
        let (body, original) = mesh(&mut app, a);
        let (plain, plain_original) = mesh(&mut app, b);
        app.update();
        assert!(refracts(&app, body));
        assert_eq!(worn(&app, plain), plain_original);
        let copy = worn(&app, body);
        {
            let mut targets = app.world_mut().resource_mut::<Targets>();
            targets.0[0].chameleon = 0.0;
            targets.0[1].chameleon = 1.0;
        }
        app.update();
        assert_eq!(worn(&app, body), original);
        assert!(!app.world().resource::<Assets<NifMaterial>>().contains(copy.id()));
        assert!(refracts(&app, plain));
        app.world_mut().resource_mut::<Targets>().0[0].chameleon = 95.0;
        app.update();
        assert!(refracts(&app, body));
        app.world_mut().resource_mut::<Targets>().0[0].dead = true;
        app.update();
        assert_eq!(worn(&app, body), original);
        assert!(refracts(&app, plain));
        app.world_mut().resource_mut::<Targets>().0[0].dead = false;
        app.update();
        assert!(refracts(&app, body));
    }

    #[test]
    fn unchanged_actors_do_not_write_or_allocate_materials() {
        let mut app = app();
        let a = actor(&mut app, 95.0);
        let (body, _) = mesh(&mut app, a);
        app.update();
        let copy = worn(&app, body);
        let count = app.world().resource::<Assets<NifMaterial>>().len();
        for _ in 0..3 {
            app.update();
            assert_eq!(app.world().resource::<Writes>().0, 0);
            assert_eq!(app.world().resource::<Assets<NifMaterial>>().len(), count);
            assert_eq!(worn(&app, body), copy);
        }
    }

    #[test]
    fn nested_outfit_and_weapon_arrivals_and_replacements_follow_the_field() {
        let mut app = app();
        let a = actor(&mut app, 95.0);
        let (body, _) = mesh(&mut app, a);
        app.update();
        let outfit = app.world_mut().spawn(ChildOf(a)).id();
        let (part, part_original) = mesh(&mut app, outfit);
        let (weapon, weapon_original) = mesh(&mut app, outfit);
        app.update();
        assert!(refracts(&app, part));
        assert!(refracts(&app, weapon));
        let old_copy = worn(&app, body);
        let replacement = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(nvf_assets::nif_material::nif_material(StandardMaterial { base_color: Color::BLACK, ..default() }));
        app.world_mut().entity_mut(body).insert(MeshMaterial3d(replacement.clone()));
        app.update();
        assert!(refracts(&app, body));
        assert!(!app.world().resource::<Assets<NifMaterial>>().contains(old_copy.id()));
        app.world_mut().resource_mut::<Targets>().0[0].chameleon = 0.0;
        app.update();
        assert_eq!(worn(&app, body), replacement);
        assert_eq!(worn(&app, part), part_original);
        assert_eq!(worn(&app, weapon), weapon_original);
    }

    #[test]
    fn material_animation_writes_plain_source_and_preserves_cached_values() {
        animation_transition(false);
    }

    #[test]
    fn animation_started_before_cloak_and_stopped_during_cloak_restores_stored_values() {
        animation_transition(true);
    }

    fn animation_transition(stop_while_cloaked: bool) {
        use fnv_nif::anim::{Channel, Sequence, ValueTrack};
        use nvf_assets::anim_play::{BlendPlayer, BoundClip, Clip, Layer, MaterialTarget, MaterialTrack};
        use nvf_assets::material_anim::{Look, StoredLook};
        use std::sync::Arc;

        let mut app = app();
        let a = actor(&mut app, if stop_while_cloaked { 0.0 } else { 95.0 });
        let (body, original) = mesh(&mut app, a);
        app.world_mut().entity_mut(body).insert((nvf_assets::actor::ShapeName("testshape".into()), StoredLook(Look::stored([1.0; 3], 1.0, 1.0))));
        app.update();
        let mut clip = Clip::from_sequence(&Sequence {
            block: 0, name: Some("synthetic glow".into()), start_time: 0.0, stop_time: 1.0,
            frequency: 1.0, cycle_type: 0, accum_root_name: None, text_keys: Vec::new(), tracks: Vec::new(),
        });
        clip.materials.push(MaterialTrack {
            shape: "testshape".into(), target: MaterialTarget::EmissiveMult,
            values: Arc::new(ValueTrack::Float(Channel::Static(Some([2.0])))), cycle: None, priority: 0,
        });
        clip.materials.push(MaterialTrack {
            shape: "testshape".into(), target: MaterialTarget::UvOffset(0),
            values: Arc::new(ValueTrack::Float(Channel::Static(Some([0.25])))), cycle: None, priority: 0,
        });
        let mut player = BlendPlayer::new(Vec::new(), Arc::new(Vec::new()));
        player.layers[0] = Some(Layer { clip: Arc::new(BoundClip::bind(Arc::new(clip), |_| None)), phase: 0.0, weight: 1.0 });
        app.world_mut().entity_mut(a).insert(player);
        app.add_systems(Update, crate::actor_materials::pose);
        app.update();
        if stop_while_cloaked {
            assert!(!refracts(&app, body));
            app.world_mut().resource_mut::<Targets>().0[0].chameleon = 95.0;
            app.update();
        }
        let copy = worn(&app, body);
        let source = app.world().get::<MaterialOverride>(body).unwrap().source.clone();
        let materials = app.world().resource::<Assets<NifMaterial>>();
        assert_eq!(materials.get(&copy).unwrap().base.emissive.red, 2.0);
        assert_eq!(materials.get(&source).unwrap().base.emissive.red, 2.0);
        assert!(!materials.get(&source).unwrap().extension.refract);
        if stop_while_cloaked {
            app.world_mut().get_mut::<BlendPlayer>(a).unwrap().layers[0] = None;
            app.update();
            let materials = app.world().resource::<Assets<NifMaterial>>();
            for handle in [&source, &worn(&app, body)] {
                let m = materials.get(handle).unwrap();
                assert_eq!(m.base.emissive.red, 1.0);
                assert_eq!(m.base.uv_transform.translation.x, 0.0);
            }
        }
        app.world_mut().resource_mut::<Targets>().0[0].chameleon = 0.0;
        app.update();
        let restored = worn(&app, body);
        assert_ne!(restored, original, "animation must not mutate the shared source");
        for _ in 0..2 {
            let materials = app.world().resource::<Assets<NifMaterial>>();
            let plain = materials.get(&worn(&app, body)).unwrap();
            assert!(!plain.extension.refract);
            assert_eq!(plain.base.emissive.red, if stop_while_cloaked { 1.0 } else { 2.0 });
            assert_eq!(plain.base.uv_transform.translation.x, if stop_while_cloaked { 0.0 } else { 0.25 });
            assert_eq!(materials.get(&original).unwrap().base.emissive.red, 0.0);
            assert_eq!(materials.get(&original).unwrap().base.uv_transform.translation.x, 0.0);
            app.update();
            assert_eq!(worn(&app, body), restored);
        }
    }

    #[test]
    fn production_rig_animation_refreshes_plain_source_and_cloak_without_idle_writes() {
        use fnv_nif::anim::{Channel, Key, KeyKind, Keys, ValueTrack};
        use nvf_assets::anim_play::{Clip, MaterialTarget, MaterialTrack, Playing};
        use nvf_assets::material_anim::{animate_rig_materials, bind_rig_materials, MaterialSlot, RigMaterials};
        use std::sync::Arc;

        for bind_while_cloaked in [false, true] {
            let mut app = app();
            let a = actor(&mut app, if bind_while_cloaked { 95.0 } else { 0.0 });
            let rig = app.world_mut().spawn(ChildOf(a)).id();
            let (body, original) = mesh(&mut app, rig);
            app.update();
            let track = |target, start, end| MaterialTrack {
                shape: "synthetic-part".into(), target, cycle: None, priority: 0,
                values: Arc::new(ValueTrack::Float(Channel::Keys(Keys {
                    kind: KeyKind::Linear,
                    keys: [(0.0, start), (1.0, end)].into_iter().map(|(time, value)| Key {
                        time, value: [value], forward: [0.0], backward: [0.0], tbc: [0.0; 3],
                    }).collect(),
                }))),
            };
            let tracks = vec![track(MaterialTarget::UvOffset(0), 0.0, 0.75), track(MaterialTarget::Alpha, 1.0, 0.25)];
            let mut clip = Clip::from_controllers(tracks.clone(), Vec::new(), Vec::new());
            clip.stop = 1.0;
            clip.cycle_type = 2;
            let mut playing = Playing::new("synthetic-part", &clip, |_| None);
            playing.time = 0.25;
            app.world_mut().entity_mut(rig).insert((playing, RigMaterials {
                slots: vec![MaterialSlot::new("synthetic-part".into(), body, original.clone(), [0.0; 3], 1.0, 1.0)],
                idle: Arc::new(tracks), dark: false, fade: 1.0,
            }));
            app.add_systems(Update, (bind_rig_materials, animate_rig_materials).chain());
            app.update();
            if !bind_while_cloaked {
                app.world_mut().resource_mut::<Targets>().0[0].chameleon = 95.0;
                app.update();
            }
            let source = app.world().get::<MaterialOverride>(body).unwrap().source.clone();
            let copy = worn(&app, body);
            assert_ne!(source, original);
            for time in [0.5, 0.75] {
                app.world_mut().get_mut::<Playing>(rig).unwrap().time = time;
                app.update();
                assert_eq!(worn(&app, body), copy);
                assert_eq!(app.world().get::<RigMaterials>(rig).unwrap().slots[0].handle, source);
                let materials = app.world().resource::<Assets<NifMaterial>>();
                for handle in [&source, &copy] {
                    let m = materials.get(handle).unwrap();
                    assert_eq!(m.base.uv_transform.translation.x, time * 0.75);
                    assert_eq!(m.base.base_color.alpha(), 1.0 - time * 0.75);
                }
                assert!(!materials.get(&source).unwrap().extension.refract);
                assert!(materials.get(&copy).unwrap().extension.refract);
            }
            let count = app.world().resource::<Assets<NifMaterial>>().len();
            for _ in 0..2 {
                app.update();
                assert_eq!(app.world().resource::<Writes>().0, 0);
                assert_eq!(app.world().resource::<Assets<NifMaterial>>().len(), count);
            }
            let replacement = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(nvf_assets::nif_material::nif_material(StandardMaterial {
                base_color: Color::linear_rgb(0.25, 0.5, 0.75), ..default()
            }));
            app.world_mut().entity_mut(body).insert(MeshMaterial3d(replacement.clone()));
            app.world_mut().get_mut::<Playing>(rig).unwrap().time = 0.875;
            app.update();
            assert_eq!(app.world().get::<MaterialOverride>(body).unwrap().source, replacement);
            assert_eq!(app.world().get::<RigMaterials>(rig).unwrap().slots[0].handle, source);
            let materials = app.world().resource::<Assets<NifMaterial>>();
            assert_eq!(materials.get(&worn(&app, body)).unwrap().base.uv_transform.translation.x, 0.0);
            assert_eq!(materials.get(&source).unwrap().base.uv_transform.translation.x, 0.875 * 0.75);
            assert!(!materials.get(&source).unwrap().extension.refract);
            assert!(!materials.contains(copy.id()));
            // [design, #2557] A producer restoring the rig's source gets its latest
            // animation, without rig publication taking over that producer's material.
            app.world_mut().entity_mut(body).insert(MeshMaterial3d(source.clone()));
            app.update();
            assert_eq!(app.world().resource::<Assets<NifMaterial>>().get(&worn(&app, body)).unwrap().base.uv_transform.translation.x, 0.875 * 0.75);
            app.world_mut().resource_mut::<Targets>().0[0].chameleon = 0.0;
            for time in [0.875, 1.0] {
                app.world_mut().get_mut::<Playing>(rig).unwrap().time = time;
                app.update();
                assert_eq!(worn(&app, body), source);
                let materials = app.world().resource::<Assets<NifMaterial>>();
                let m = materials.get(&source).unwrap();
                assert!(!m.extension.refract);
                assert_eq!(m.base.uv_transform.translation.x, time * 0.75);
                assert_eq!(m.base.base_color.alpha(), 1.0 - time * 0.75);
                for handle in [&original, &replacement] {
                    let shared = materials.get(handle).unwrap();
                    assert_eq!(shared.base.uv_transform.translation.x, 0.0);
                    assert_eq!(shared.base.base_color.alpha(), 1.0);
                    assert!(!shared.extension.refract);
                }
            }
        }
    }

    #[derive(Resource, Default)]
    struct Alphas(std::collections::BTreeMap<FormId, f32>);

    fn with_alpha(app: &mut App) {
        app.init_resource::<crate::effect_shaders::Shaders>()
            .init_resource::<Alphas>()
            .add_systems(Update, |alpha: Res<Alphas>, mut drawing: crate::effect_shaders::AlphaDrawing| {
                drawing.sync(&alpha.0);
            });
    }

    #[test]
    fn production_actor_alpha_restores_live_plain_material_in_both_orders_and_together() {
        for order in 0..3 {
            let mut app = app();
            with_alpha(&mut app);
            let a = actor(&mut app, 95.0);
            let reference = app.world().resource::<Targets>().0[0].reference;
            let (body, original) = mesh(&mut app, a);
            app.world_mut().entity_mut(body).insert(RefVisual(reference));
            app.update();
            let old_cloak = worn(&app, body);
            app.world_mut().resource_mut::<Alphas>().0.insert(FormId(reference), 0.5);
            app.update();
            let faded = app.world().get::<nvf_play::fade::Fading>(body).unwrap();
            assert_eq!(faded.original(), &original);
            let plain = app.world().get::<MaterialOverride>(body).unwrap().source.clone();
            let materials = app.world().resource::<Assets<NifMaterial>>();
            assert!(!materials.contains(old_cloak.id()));
            assert!(!materials.get(&plain).unwrap().extension.refract);
            assert_eq!(materials.get(&plain).unwrap().base.base_color.alpha(), 0.5);
            assert_eq!(materials.get(&worn(&app, body)).unwrap().base.base_color.alpha(), 0.5);
            if order == 0 {
                app.world_mut().resource_mut::<Alphas>().0.clear();
                app.update();
                assert!(refracts(&app, body));
                assert_eq!(app.world().get::<MaterialOverride>(body).unwrap().source, original);
            }
            app.world_mut().resource_mut::<Targets>().0[0].chameleon = 0.0;
            if order == 2 {
                app.world_mut().resource_mut::<Alphas>().0.clear();
            }
            app.update();
            if order == 1 {
                assert_eq!(worn(&app, body), plain);
                assert!(!refracts(&app, body));
                app.world_mut().resource_mut::<Alphas>().0.clear();
                app.update();
            }
            for _ in 0..2 {
                assert_eq!(worn(&app, body), original);
                assert!(!refracts(&app, body));
                assert_eq!(app.world().resource::<Assets<NifMaterial>>().get(&original).unwrap().base.base_color.alpha(), 1.0);
                assert!(app.world().get::<nvf_play::fade::Fading>(body).is_none());
                app.update();
            }
        }
    }

    #[test]
    fn production_shader_play_and_cover_suppress_holes_overlays_while_cloaked() {
        use crate::effect_shaders::{holes_playback_for_test, Overlay, PlaybackTestGame, Shaders};
        let (app, _files) = crate::outfits::tests::asset_fixture();
        let mut app = with_drawing(app);
        let a = actor(&mut app, 95.0);
        let reference = app.world().resource::<Targets>().0[0].reference;
        let (body, original) = mesh(&mut app, a);
        app.world_mut().entity_mut(body).insert((RefVisual(reference), Mesh3d(Handle::default())));
        app.update();
        let forms = Box::leak(Box::default());
        let refs = Box::leak(Box::default());
        let game = crate::outfits::tests::game(fnv_sim::World::new(forms, refs, Default::default()));
        app.insert_resource(crate::outfits::tests::empty_world_data());
        let source = holes_playback_for_test(&mut app, reference, game);
        for seconds in [1.0, 2.0, 3.0] {
            app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(seconds));
            app.update();
            assert!(app.world().resource::<Shaders>().active[&source].covered[&body].is_empty());
            assert_eq!(app.world_mut().query::<&Overlay>().iter(app.world()).count(), 0);
            assert_eq!(app.world_mut().query::<&MaterialOverride>().iter(app.world()).count(), 1);
            let materials = app.world().resource::<Assets<NifMaterial>>();
            let cloak = materials.get(&worn(&app, body)).unwrap();
            assert!(cloak.extension.refract);
            assert!(cloak.extension.holes.x < 255.0 && cloak.extension.holes.x > 0.0);
        }
        app.world_mut().resource_mut::<Targets>().0[0].chameleon = 0.0;
        app.update();
        assert!(!refracts(&app, body));
        app.update();
        assert_eq!(app.world().resource::<Shaders>().active[&source].covered[&body].len(), 1);
        app.world_mut().resource_mut::<Targets>().0[0].chameleon = 95.0;
        app.update();
        app.update();
        assert!(app.world().resource::<Shaders>().active[&source].covered[&body].is_empty());
        assert_eq!(app.world_mut().query::<&Overlay>().iter(app.world()).count(), 0);
        app.world_mut().resource_mut::<PlaybackTestGame>().0.script_looks.shaders.clear();
        app.world_mut().resource_mut::<Targets>().0[0].chameleon = 0.0;
        app.update();
        assert_eq!(worn(&app, body), original);
        assert!(!refracts(&app, body));
    }

    #[test]
    fn production_shader_coverage_reconsiders_uncloak_and_shader_start_in_the_same_frame() {
        use crate::effect_shaders::{Active, Shaders, Source};
        let mut app = app();
        app.init_resource::<Shaders>().add_systems(Update, crate::effect_shaders::cover);
        let a = actor(&mut app, 95.0);
        let reference = app.world().resource::<Targets>().0[0].reference;
        let (body, _) = mesh(&mut app, a);
        app.world_mut().entity_mut(body).insert((RefVisual(reference), Mesh3d(Handle::default())));
        app.update();
        let fill = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(nvf_assets::nif_material::nif_material(StandardMaterial::default()));
        let source = Source::Script(1);
        app.world_mut().resource_mut::<Shaders>().active.insert(source, Active::membrane_for_test(reference, fill));
        app.world_mut().resource_mut::<Targets>().0[0].chameleon = 0.0;
        app.update();
        assert!(!refracts(&app, body));
        assert!(app.world().resource::<Shaders>().active[&source].covered[&body].is_empty());
        app.update();
        let overlays = app.world().resource::<Shaders>().active[&source].covered[&body].clone();
        assert_eq!(overlays.len(), 1);
        assert!(app.world().get_entity(overlays[0]).is_ok());
        app.world_mut().resource_mut::<Targets>().0[0].chameleon = 95.0;
        app.update();
        app.update();
        assert!(app.world().resource::<Shaders>().active[&source].covered[&body].is_empty());
        assert!(app.world().get_entity(overlays[0]).is_err());
        app.world_mut().resource_mut::<Targets>().0[0].chameleon = 0.0;
        app.update();
        app.update();
        assert_eq!(app.world().resource::<Shaders>().active[&source].covered[&body].len(), 1);
    }

    #[test]
    fn production_fade_keeps_plain_restoration_handles_and_uncloaks_during_or_after_fading() {
        for uncloak_during in [false, true] {
            let mut app = app();
            with_fades(&mut app);
            let a = actor(&mut app, 95.0);
            let (body, original) = mesh(&mut app, a);
            app.update();
            let old_cloak = worn(&app, body);
            start_fade(&mut app, body);
            let fading = app.world().get::<nvf_play::fade::Fading>(body).unwrap();
            assert_eq!(fading.original(), &original);
            let fade_copy = fading.copy().clone();
            let materials = app.world().resource::<Assets<NifMaterial>>();
            assert!(materials.contains(original.id()));
            assert!(!materials.get(&fade_copy).unwrap().extension.refract);
            assert!(!materials.contains(old_cloak.id()));
            assert!(refracts(&app, body));
            advance_fade(&mut app, 0.5);
            assert_eq!(app.world().resource::<Assets<NifMaterial>>().get(&worn(&app, body)).unwrap().base.base_color.alpha(), 0.25);
            if uncloak_during {
                app.world_mut().resource_mut::<Targets>().0[0].chameleon = 0.0;
                app.update();
                assert_eq!(worn(&app, body), fade_copy);
                assert!(!refracts(&app, body));
            }
            advance_fade(&mut app, 2.0);
            assert!(app.world().get::<nvf_play::fade::Fading>(body).is_none());
            assert!(!app.world().resource::<Assets<NifMaterial>>().contains(fade_copy.id()));
            if !uncloak_during {
                assert!(refracts(&app, body));
                app.world_mut().resource_mut::<Targets>().0[0].chameleon = 0.0;
                app.update();
            }
            assert_eq!(worn(&app, body), original);
            assert!(!refracts(&app, body));
            app.update();
            assert_eq!(worn(&app, body), original);
        }
    }

    #[test]
    fn production_texture_swap_uses_plain_source_with_or_without_a_fade() {
        use fnv_game::functions::misc::TextureSwaps;

        #[derive(Resource, Default)]
        struct Swaps(TextureSwaps);

        for during_fade in [false, true] {
            let (app, _files) = crate::outfits::tests::asset_fixture();
            let mut app = with_drawing(app);
            with_fades(&mut app);
            app.insert_resource(crate::outfits::tests::empty_world_data())
                .init_resource::<crate::texture_swap::Swapped>()
                .init_resource::<Swaps>()
                .add_systems(Update, (|swaps: Res<Swaps>, sim: Res<nvf_play::sim::SimState>, world: Res<crate::world::WorldData>, mut drawing: crate::texture_swap::Swapping| {
                    drawing.sync(&swaps.0, &sim, &world);
                }).before(nvf_play::PlaySet::Sync));
            // [documented: engine/nvf-assets/src/texture.rs tests::dds] Synthetic
            // one-block DDS, offered to the normal texture loader without game files.
            let texture = "textures\\synthetic-cloak-swap.dds";
            let mut dds = vec![0u8; 136];
            dds[..4].copy_from_slice(b"DDS ");
            for (at, value) in [(4, 124u32), (8, 0x1007), (12, 4), (16, 4), (28, 1), (76, 32), (80, 4), (108, 0x1000)] {
                dds[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
            dds[84..88].copy_from_slice(b"DXT1");
            app.world_mut().resource_mut::<nvf_assets::TextureCache>().offer(texture.into(), dds, 0.0);
            let a = actor(&mut app, 95.0);
            let reference = app.world().resource::<Targets>().0[0].reference;
            let (body, original) = mesh(&mut app, a);
            app.world_mut().entity_mut(body).insert((RefVisual(reference), Visibility::Inherited, nvf_assets::actor::ShapeName("testshape".into())));
            app.update();
            if during_fade {
                start_fade(&mut app, body);
            }
            {
                let mut swaps = app.world_mut().resource_mut::<Swaps>();
                swaps.0.by_ref.insert(FormId(reference), vec![("testshape".into(), texture.into())]);
                swaps.0.generation += 1;
            }
            app.update();
            assert!(refracts(&app, body));
            let plain = app.world().get::<MaterialOverride>(body).unwrap().source.clone();
            let plain = app.world().get::<nvf_play::fade::Fading>(body).map_or(plain, |f| f.original().clone());
            let image = {
                let materials = app.world().resource::<Assets<NifMaterial>>();
                let swapped = materials.get(&plain).unwrap();
                assert!(!swapped.extension.refract);
                assert!(materials.get(&original).unwrap().base.base_color_texture.is_none());
                swapped.base.base_color_texture.clone().expect("production swap loaded the synthetic texture")
            };
            if during_fade {
                advance_fade(&mut app, 3.0);
            }
            app.world_mut().resource_mut::<Targets>().0[0].chameleon = 0.0;
            app.update();
            assert_eq!(worn(&app, body), plain);
            assert!(!refracts(&app, body));
            assert_eq!(app.world().resource::<Assets<NifMaterial>>().get(&plain).unwrap().base.base_color_texture.as_ref(), Some(&image));
            app.update();
            assert_eq!(worn(&app, body), plain);
        }
    }

    #[test]
    fn detached_and_transferred_parts_restore_before_the_new_owner_cloaks() {
        let mut app = app();
        let a = actor(&mut app, 95.0);
        let b = actor(&mut app, 95.0);
        let holder = app.world_mut().spawn(ChildOf(a)).id();
        let (part, original) = mesh(&mut app, holder);
        app.update();
        let copy = worn(&app, part);
        app.world_mut().entity_mut(holder).insert(ChildOf(b));
        app.update();
        assert!(refracts(&app, part));
        assert!(app.world().resource::<ActorFields>().0[&a].originals.is_empty());
        assert_eq!(app.world().get::<MaterialOverride>(part).unwrap().source, original);
        let transferred_copy = worn(&app, part);
        assert_ne!(transferred_copy, copy);
        assert!(!app.world().resource::<Assets<NifMaterial>>().contains(copy.id()));
        app.world_mut().entity_mut(holder).remove::<ChildOf>();
        app.update();
        assert_eq!(worn(&app, part), original);
        assert!(!app.world().resource::<Assets<NifMaterial>>().contains(transferred_copy.id()));
        assert!(app.world().get::<MaterialOverride>(part).is_none());
        assert!(app.world().resource::<ActorFields>().0.values().all(|f| f.originals.is_empty()));
    }

    #[test]
    fn unloaded_actor_restores_surviving_meshes_and_releases_despawned_copies() {
        let mut app = app();
        let a = actor(&mut app, 95.0);
        let (body, original) = mesh(&mut app, a);
        let (weapon, _) = mesh(&mut app, a);
        app.update();
        let weapon_copy = worn(&app, weapon);
        app.world_mut().despawn(weapon);
        app.update();
        assert!(!app.world().resource::<Assets<NifMaterial>>().contains(weapon_copy.id()));
        let body_copy = worn(&app, body);
        app.world_mut().resource_mut::<Targets>().0.clear();
        app.update();
        assert_eq!(worn(&app, body), original);
        assert!(!app.world().resource::<Assets<NifMaterial>>().contains(body_copy.id()));
        assert!(app.world().resource::<ActorFields>().0.is_empty());
    }

    #[test]
    fn the_copy_refracts_and_keeps_its_cut() {
        let m = nvf_assets::nif_material::nif_material(StandardMaterial { base_color: Color::WHITE, alpha_mode: AlphaMode::Mask(0.4), ..default() });
        let c = see_through(&m);
        assert!(c.extension.refract);
        assert!(matches!(c.base.alpha_mode, AlphaMode::Opaque));
        assert!((c.extension.refraction * nvf_assets::nif_material::REFRACTION_SCREEN - STRENGTH).abs() < 1e-6);
        assert!((c.extension.fire_period - PERIOD).abs() < 1e-6);
        assert!((c.extension.alpha_cut - 0.4).abs() < 1e-6);
        let hair = nvf_assets::nif_material::nif_material(StandardMaterial { alpha_mode: AlphaMode::Blend, ..default() });
        assert!((see_through(&hair).extension.alpha_cut - BLEND_CUT).abs() < 1e-6);
    }

    #[test]
    fn idle_fast_path_skips_writes_and_transitions_cleanly_on_chameleon_activation() {
        let mut app = app();
        let a = actor(&mut app, 0.0);
        let (body, original) = mesh(&mut app, a);
        app.update();
        assert!(app.world().resource::<ActorFields>().0.is_empty());
        assert_eq!(worn(&app, body), original);
        // Second idle frame must perform zero material writes.
        app.update();
        assert_eq!(app.world().resource::<Writes>().0, 0);
        // Enabling Chameleon from the idle fast path cloaks the actor and records originals.
        app.world_mut().resource_mut::<Targets>().0[0].chameleon = 80.0;
        app.update();
        assert_eq!(app.world().resource::<ActorFields>().0.len(), 1);
        assert_ne!(worn(&app, body), original);
        // Turning Chameleon back off restores original material and returns to idle state.
        app.world_mut().resource_mut::<Targets>().0[0].chameleon = 0.0;
        app.update();
        assert!(app.world().resource::<ActorFields>().0.is_empty());
        assert_eq!(worn(&app, body), original);
    }
}
