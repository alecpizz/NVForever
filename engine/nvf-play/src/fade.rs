//! References fading in and out (docs/design/script-function-params.md §2): Enable fades
//! its reference in unless its FadeIn is 0, Disable fades it out when its FadeOut is 1
//! [documented: GECK wiki Enable, Disable], and enable children follow their own rule
//! (`script_events::effects`, which sends [`RefFade`] after the reference's
//! `RefChanged`).
//!
//! - **Look**: every mesh drawn for the reference gets a blended copy of its material
//!   whose alpha goes from 0 to 1 (in) or 1 to 0 (out) in a straight line over
//!   [`FADE_SECONDS`]; at the end the mesh gets its own material back [hypothesis: the
//!   shape and the length; capture job `tools/capture/jobs/enable_fade_timing.json`].
//! - **Single owner with `SetActorAlpha`** (#3105): one [`Fading`] component owns the
//!   mesh's single see-through copy whenever a presence fade, `SetActorAlpha`, or both
//!   are active. Its drawn opacity and cut-out threshold scale the mesh's own persistent
//!   material by `presence * actor_alpha`; either effect starting, changing, or ending
//!   recomputes the copy in place, and the last one to end removes the copy and puts the
//!   mesh's own material back.
//! - The copy is marked as a [`HeldCopy`]: an animated model whose rig binds the mesh's
//!   material while it is see-through makes its own copy from the material held and puts
//!   it there ([`follow_rig_materials`]); the fade then draws a see-through copy of the
//!   rig's own, as it moves, and gives that back (#3919: the Hidden Valley dust storm,
//!   enabled by its quest with a fade, stood still after it).
//! - **Out**: the simulation keeps the reference enabled while it fades
//!   (`fnv_game::functions::refs::FADE_OUT_HOLD`, GetDisabled 0 [observed]); its meshes
//!   fade to nothing and stay hidden until the hold ends and the simulation disables it
//!   (`ScriptLooks::disabled_after_fade`, a `RefChanged` from `script`), when collision
//!   goes too. A plain Disable meanwhile hides it at once.
//! - **In**: only a reference present after its change fades in (an enable child "set to
//!   the opposite of its parent" is disabled by its parent's Enable and pops out).
//! - The fade runs on the game's clock (`Time<Virtual>`): it stands still in menus.
//! - A reference whose presence turns back while it fades (Enable 0 during a fade out,
//!   Disable during a fade in) ends its fade at once and is drawn as it now is.
//! - References scripts draw (`script_draw`) are kept while they fade out
//!   ([`Fades::fading_out`]).
//! - **MoveToMarkerWithFade** (`fnv_game::functions::refs::move_to_marker_with_fade`):
//!   the reference fades out while the simulation holds its move
//!   (`ScriptLooks::fade_moves`), stays hidden ([`FadeHidden`]) until the move, then fades
//!   in where it went. It is never disabled.
//! - A reference its critical stage culls (`crate::activate::drawn`) is not shown by a
//!   fade.
//!
//! Log: `fade: <id> in|out over <s> s (<n> meshes)` when one starts (meshes drawn then;
//! a cell loaded later joins in) and `fade: <id> in|out done` at its end, with `; <n>
//! meshes given their own material anew while it ran, drawn after it` when a rig bound
//! the material of some while it ran (#3919).

use crate::script::ScriptState;
use crate::sim::SimState;
use crate::{PlaySet, RefVisual};
use bevy::prelude::*;
use fnv_sim::FormId;
use nvf_assets::material_anim::HeldCopy;
use nvf_assets::NifMaterial;
use std::collections::{HashMap, HashSet};

/// How long a fade lasts, seconds: a fade out as long as the original kept the reference
/// enabled while it faded (`fnv_game::functions::FADE_OUT_HOLD`, 2 s [observed: capture
/// `disable_fade_long`]), and a fade in the same [hypothesis: not measured; every
/// screenshot of the fade captures failed].
pub const FADE_SECONDS: f32 = 2.0;

/// Fade a reference in or out (sent by `script` after the reference's `RefChanged`).
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct RefFade {
    pub form_id: u32,
    pub fade_in: bool,
}

pub struct FadePlugin;

impl Plugin for FadePlugin {
    fn build(&self, app: &mut App) {
        // A model drawn while its reference fades (one a script enabled, drawn by
        // `script_draw::sync` the frame it fades) is given the fade's held copy before its rig
        // binds: `step` runs after the drawing and its copy goes on before
        // `bind_rig_materials`, so the rig binds under it ([`HeldCopy`]). Otherwise the rig's
        // own is put on over the copy and the fade gives back the model's material (#3919).
        app.add_message::<RefFade>()
            .init_resource::<Fades>()
            .add_systems(
                Update,
                (start, step)
                    .chain()
                    .after(crate::activate::sync_visuals)
                    .after(crate::script_draw::sync)
                    .in_set(PlaySet::Sync)
                    .before(nvf_assets::material_anim::bind_rig_materials),
            )
            .add_systems(
            Update,
            follow_rig_materials.after(nvf_assets::material_anim::bind_rig_materials).after(nvf_assets::material_anim::animate_rig_materials).after(step),
        );
    }
}

/// A fade under way.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Active {
    fade_in: bool,
    /// `Time<Virtual>` seconds when it started.
    since: f32,
}

#[derive(Resource, Default)]
pub struct Fades {
    active: HashMap<u32, Active>,
    /// The meshes of each reference given a new own material by a rig while they were
    /// see-through (#3919), counted in the fade's "done" line.
    renewed: HashMap<u32, HashSet<Entity>>,
}

impl Fades {
    /// The reference is fading out: it is still drawn though disabled.
    pub fn fading_out(&self, id: u32) -> bool {
        self.active.get(&id).is_some_and(|a| !a.fade_in)
    }
}

/// Which opacity effect writes a factor into a mesh's single [`Fading`] copy (#3105).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FadeChannel {
    /// A presence fade (`Enable` / `Disable` / `MoveToMarkerWithFade`).
    Presence,
    /// An actor's scripted alpha (`SetActorAlpha`).
    ActorAlpha,
}

/// On a mesh drawn see-through by a presence fade, `SetActorAlpha`, or both (#3105):
/// its own persistent material, the single see-through copy drawn, and the two
/// independent factors whose product (`presence * actor_alpha`) scales the copy's base
/// opacity and cut-out threshold.
///
/// The last active factor to end removes `copy` from `Assets<NifMaterial>`, puts
/// `original` back on the mesh, and removes this component. Systems that replace or edit
/// the mesh's own material while it is see-through (`SwapTexture`, scripted NIF material
/// edits) update `original` and rebuild `copy` in place via [`queue_retarget`].
#[derive(Component)]
pub struct Fading {
    original: Handle<NifMaterial>,
    copy: Handle<NifMaterial>,
    /// The original's alpha and cut-out, which the combined factor multiplies.
    alpha: f32,
    cut: Option<f32>,
    presence: Option<f32>,
    actor_alpha: Option<f32>,
    /// The original is an animated model's own (bound while the mesh was see-through,
    /// [`HeldCopy`]): the copy is made from it again every frame, so it moves as the
    /// original does (#3919).
    follows: bool,
}

impl Fading {
    /// The mesh's own persistent material, which it gets back when every fade ends.
    pub fn original(&self) -> &Handle<NifMaterial> {
        &self.original
    }

    /// The single see-through copy drawn while any fade or actor alpha is active.
    pub fn copy(&self) -> &Handle<NifMaterial> {
        &self.copy
    }

    /// The presence fade's factor while a presence fade is active on this mesh.
    pub fn presence(&self) -> Option<f32> {
        self.presence
    }

    /// The actor-alpha factor while `SetActorAlpha` (< 1) is active on this mesh.
    pub fn actor_alpha(&self) -> Option<f32> {
        self.actor_alpha
    }

    /// The combined opacity multiplier currently drawn (`presence * actor_alpha`).
    pub fn factor(&self) -> f32 {
        self.presence.unwrap_or(1.0) * self.actor_alpha.unwrap_or(1.0)
    }
}

/// Resolve a mesh's own persistent material beneath both a temporary cloak override
/// (`MaterialOverride`) and any active see-through copy (`Fading`) [design, #3105].
pub fn persistent_material<'a>(
    drawn: &'a Handle<NifMaterial>,
    drawing: Option<&'a nvf_assets::nif_material::MaterialOverride>,
    fading: Option<&'a Fading>,
) -> &'a Handle<NifMaterial> {
    fading.map_or_else(|| nvf_assets::nif_material::plain_material(drawn, drawing), Fading::original)
}

/// Derive the see-through copy of `source` at `factor`, preserving the cut-out rule,
/// disabling depth writes, converting opaque/mask materials to `Blend`, and dropping
/// authored height maps [design, #2981, #3105].
fn make_see_through(source: &NifMaterial, factor: f32) -> (NifMaterial, f32, Option<f32>) {
    let mut m = source.clone();
    let base = m.base.base_color.alpha();
    let cut = match m.base.alpha_mode {
        AlphaMode::Mask(c) => Some(c),
        _ => None,
    };
    // A cut-out keeps its cut while it blends.
    if let Some(c) = cut {
        m.extension.alpha_cut = c * factor;
    }
    // Fading hair lets what lies behind it show too.
    m.extension.depth_write = false;
    if matches!(m.base.alpha_mode, AlphaMode::Opaque | AlphaMode::Mask(_)) {
        m.base.alpha_mode = AlphaMode::Blend;
    }
    // #2981: a blended copy draws no authored height map.
    nvf_assets::authored::recheck_height(&mut m);
    m.base.base_color.set_alpha(base * factor);
    (m, base, cut)
}

/// Apply one channel's factor (`Some(a)` or `None` to release) directly to `entity` on
/// `world`. Running inside queued commands ensures presence fades and `SetActorAlpha`
/// compose multiplicatively and hand back `original` in any start/end/system order,
/// including both starting or ending in one update [design, #3105].
pub fn apply_factor(world: &mut World, entity: Entity, channel: FadeChannel, value: Option<f32>) {
    if world.get_entity(entity).is_err() {
        return;
    }
    if let Some(existing) = world.get::<Fading>(entity) {
        let original = existing.original.clone();
        let copy = existing.copy.clone();
        let base_alpha = existing.alpha;
        let cut = existing.cut;
        let (old_presence, old_actor) = (existing.presence, existing.actor_alpha);
        let (presence, actor_alpha) = match channel {
            FadeChannel::Presence => (value, old_actor),
            FadeChannel::ActorAlpha => (old_presence, value),
        };
        if (presence, actor_alpha) == (old_presence, old_actor) {
            return;
        }
        if presence.is_none() && actor_alpha.is_none() {
            if let Some(mut materials) = world.get_resource_mut::<Assets<NifMaterial>>() {
                materials.remove(&copy);
            }
            // A rig bound since [`follow_rig_materials`] last ran: its own is given back.
            let original = world.get::<HeldCopy>(entity).filter(|h| h.copy == copy).map_or(original, |h| h.source.clone());
            if let Ok(mut e) = world.get_entity_mut(entity) {
                e.insert(MeshMaterial3d(original)).remove::<(Fading, HeldCopy)>();
            }
            return;
        }
        let factor = presence.unwrap_or(1.0) * actor_alpha.unwrap_or(1.0);
        if let Some(mut f) = world.get_mut::<Fading>(entity) {
            f.presence = presence;
            f.actor_alpha = actor_alpha;
        }
        if let Some(mut materials) = world.get_resource_mut::<Assets<NifMaterial>>() {
            if let Some(mut m) = materials.get_mut(&copy) {
                m.base.base_color.set_alpha(base_alpha * factor);
                if let Some(c) = cut {
                    m.extension.alpha_cut = c * factor;
                }
            }
        }
        return;
    }
    let Some(factor) = value else { return };
    let original = {
        let mat = world.get::<MeshMaterial3d<NifMaterial>>(entity);
        let blend = world.get::<nvf_assets::nif_surface_blend::BlendShape>(entity);
        let drawing = world.get::<nvf_assets::nif_material::MaterialOverride>(entity);
        let Some(mat) = nvf_assets::nif_surface_blend::source_material(mat, blend) else { return };
        // [design, #2557] Both the see-through copy and its restoration handle start from
        // the mesh's own plain material, never a temporary cloak override.
        nvf_assets::nif_material::plain_material(mat, drawing).clone()
    };
    let Some(source) = world.get_resource::<Assets<NifMaterial>>().and_then(|a| a.get(&original).cloned()) else {
        return;
    };
    let (m, base_alpha, cut) = make_see_through(&source, factor);
    let copy = world.resource_mut::<Assets<NifMaterial>>().add(m);
    let (presence, actor_alpha) = match channel {
        FadeChannel::Presence => (Some(factor), None),
        FadeChannel::ActorAlpha => (None, Some(factor)),
    };
    if let Ok(mut e) = world.get_entity_mut(entity) {
        e.insert((
            MeshMaterial3d(copy.clone()),
            HeldCopy { source: original.clone(), copy: copy.clone() },
            Fading { original, copy, alpha: base_alpha, cut, presence, actor_alpha, follows: false },
        ));
    }
}

/// Replace or refresh the persistent material underneath an active [`Fading`] copy (for
/// `SwapTexture` or scripted NIF material edits while a fade or actor alpha runs),
/// rebuilding `copy` in place at the current combined factor [design, #3105].
/// If `Fading` ended before this command ran, the entity gets `new_original` directly.
pub fn apply_retarget(world: &mut World, entity: Entity, new_original: Handle<NifMaterial>) {
    if world.get_entity(entity).is_err() {
        return;
    }
    if world.get::<Fading>(entity).is_none() {
        if let Ok(mut e) = world.get_entity_mut(entity) {
            e.insert(MeshMaterial3d(new_original));
        }
        return;
    }
    let renewed = world.resource_scope(|world, mut materials: Mut<Assets<NifMaterial>>| {
        let mut f = world.get_mut::<Fading>(entity)?;
        let old_copy = f.copy.clone();
        f.original = new_original;
        rebuild_copy(&mut materials, &mut f).then(|| (old_copy, f.original.clone(), f.copy.clone()))
    });
    let Some((old_copy, original, copy)) = renewed else { return };
    if let Ok(mut e) = world.get_entity_mut(entity) {
        e.insert(HeldCopy { source: original, copy: copy.clone() });
        if copy != old_copy {
            e.insert(MeshMaterial3d(copy));
        }
    }
}

/// Make `f`'s see-through copy again from its original at its current factor: in place,
/// or as a new asset when the old copy is gone (then `f.copy` is the new one). False when
/// the original is not loaded, and nothing changed [design, #3105, #3919].
fn rebuild_copy(materials: &mut Assets<NifMaterial>, f: &mut Fading) -> bool {
    let Some(source) = materials.get(&f.original) else { return false };
    let (rebuilt, base_alpha, cut) = make_see_through(source, f.factor());
    if materials.contains(f.copy.id()) {
        if let Some(mut dst) = materials.get_mut(&f.copy) {
            *dst = rebuilt;
        }
    } else {
        f.copy = materials.add(rebuilt);
    }
    f.alpha = base_alpha;
    f.cut = cut;
    true
}

/// #3919: a mesh whose animated model's rig bound its material while it was see-through
/// ([`HeldCopy`], `nvf_assets::material_anim::bind_rig_materials`) wears the fade's copy
/// still, the rig's own copy held beneath it. That own copy becomes the original, which
/// the see-through copy is made from again every frame, after the rig's pose (one
/// material clone per such mesh a frame), so it moves; it is given back at the end.
#[allow(clippy::type_complexity)]
pub fn follow_rig_materials(
    mut st: ResMut<Fades>,
    mut materials: ResMut<Assets<NifMaterial>>,
    mut meshes: Query<(Entity, &mut HeldCopy, &mut Fading, Option<&mut MeshMaterial3d<NifMaterial>>, Option<&RefVisual>)>,
) {
    for (e, mut h, mut f, worn, rv) in &mut meshes {
        if h.copy == f.copy && h.source != f.original {
            f.original = h.source.clone();
            f.follows = true;
            // Counted for a presence fade's log line (an actor alpha alone has none).
            if let Some(rv) = rv.filter(|rv| st.active.contains_key(&rv.0)) {
                st.renewed.entry(rv.0).or_default().insert(e);
            }
        }
        if f.follows {
            let old = f.copy.clone();
            if rebuild_copy(&mut materials, &mut f) && f.copy != old {
                h.copy = f.copy.clone();
                if let Some(mut worn) = worn {
                    worn.0 = f.copy.clone();
                }
            }
        }
    }
}

/// Queue a presence-fade factor update (`Some(a)` or `None` when the fade ends) for `entity`.
pub fn queue_presence(commands: &mut Commands, entity: Entity, value: Option<f32>) {
    commands.queue(move |world: &mut World| apply_factor(world, entity, FadeChannel::Presence, value));
}

/// Queue a `SetActorAlpha` factor update (`Some(a)` or `None` when released) for `entity`.
pub fn queue_actor_alpha(commands: &mut Commands, entity: Entity, value: Option<f32>) {
    commands.queue(move |world: &mut World| apply_factor(world, entity, FadeChannel::ActorAlpha, value));
}

/// Queue replacing or refreshing the persistent material underneath `entity`'s [`Fading`].
pub fn queue_retarget(commands: &mut Commands, entity: Entity, new_original: Handle<NifMaterial>) {
    commands.queue(move |world: &mut World| apply_retarget(world, entity, new_original));
}

/// The alpha of a fade `t` seconds in (1 at the end of a fade in, 0 at the end of one out).
pub fn fade_alpha(fade_in: bool, t: f32) -> f32 {
    let f = (t / FADE_SECONDS).clamp(0.0, 1.0);
    if fade_in {
        f
    } else {
        1.0 - f
    }
}

/// Whether a reference is (still) on its way out: disabled, held enabled by a
/// `Disable 1` (`ScriptLooks::fading_out`), or waiting for a MoveToMarkerWithFade move
/// (`ScriptLooks::fade_moves`).
fn going_out(sim: &SimState, script: Option<&ScriptState>, id: u32) -> bool {
    !sim.present(id)
        || script.and_then(|s| s.rt.as_ref()).is_some_and(|rt| {
            let looks = &rt.game.script_looks;
            looks.fading_out.contains_key(&FormId(id)) || looks.fade_moves.contains_key(&FormId(id))
        })
}

/// On a mesh a fade out hid although its reference is still present (a
/// MoveToMarkerWithFade waiting for its move): the fade in after the move shows it again.
#[derive(Component)]
pub struct FadeHidden;

fn start(mut fades: MessageReader<RefFade>, sim: Res<SimState>, script: Option<Res<ScriptState>>, time: Res<Time<Virtual>>, mut st: ResMut<Fades>, visuals: Query<&RefVisual>) {
    for f in fades.read() {
        if going_out(&sim, script.as_deref(), f.form_id) == f.fade_in {
            continue;
        }
        st.active.insert(f.form_id, Active { fade_in: f.fade_in, since: time.elapsed_secs() });
        let n = visuals.iter().filter(|r| r.0 == f.form_id).count();
        info!("fade: 0x{:08X} {} over {FADE_SECONDS} s ({n} meshes)", f.form_id, if f.fade_in { "in" } else { "out" });
    }
}

#[allow(clippy::type_complexity)]
fn step(
    mut commands: Commands,
    sim: Res<SimState>,
    script: Option<Res<ScriptState>>,
    time: Res<Time<Virtual>>,
    mut st: ResMut<Fades>,
    mut meshes: Query<(Entity, &RefVisual, Option<&MeshMaterial3d<NifMaterial>>, Option<&nvf_assets::nif_surface_blend::BlendShape>, Option<&Fading>, &mut Visibility, Has<FadeHidden>)>,
) {
    if st.active.is_empty() {
        return;
    }
    let now = time.elapsed_secs();
    // A presence that turned back ends the fade (module notes).
    let out = |id: u32| going_out(&sim, script.as_deref(), id);
    let over = |id: u32, a: &Active| now - a.since >= FADE_SECONDS || out(id) == a.fade_in;
    let culled = |id: u32| sim.present(id) && !crate::activate::drawn(&sim, script.as_deref(), id);
    for (e, rv, mat, blend, fading, mut vis, fade_hidden) in &mut meshes {
        if nvf_assets::nif_surface_blend::source_material(mat, blend).is_none() {
            continue;
        }
        let Some(a) = st.active.get(&rv.0) else { continue };
        let t = now - a.since;
        if over(rv.0, a) {
            if fading.is_some_and(|f| f.presence.is_some()) {
                // The entity may go with its cell before this is applied (#2757).
                queue_presence(&mut commands, e, None);
            }
            let hide = out(rv.0) || culled(rv.0);
            vis.set_if_neq(if hide { Visibility::Hidden } else { Visibility::Inherited });
            if hide && sim.present(rv.0) && !fade_hidden {
                commands.entity(e).insert(FadeHidden);
            } else if !hide && fade_hidden {
                commands.entity(e).remove::<FadeHidden>();
            }
            continue;
        }
        if culled(rv.0) {
            continue;
        }
        if !a.fade_in && !sim.present(rv.0) {
            // Hidden by `sync_visuals`; drawn while it fades out.
            vis.set_if_neq(Visibility::Inherited);
        }
        if a.fade_in && fade_hidden {
            // Hidden by the end of the fade out before a move; drawn while it fades in.
            vis.set_if_neq(Visibility::Inherited);
            commands.entity(e).remove::<FadeHidden>();
        }
        let alpha = fade_alpha(a.fade_in, t);
        if fading.and_then(Fading::presence) != Some(alpha) {
            queue_presence(&mut commands, e, Some(alpha));
        }
    }
    let done: Vec<(u32, bool)> = st.active.iter().filter(|(&id, a)| over(id, a)).map(|(&id, a)| (id, a.fade_in)).collect();
    for (id, fade_in) in done {
        st.active.remove(&id);
        let renewed = st.renewed.remove(&id).map_or(0, |m| m.len());
        let way = if fade_in { "in" } else { "out" };
        if renewed > 0 {
            info!("fade: 0x{id:08X} {way} done; {renewed} meshes given their own material anew while it ran, drawn after it");
        } else {
            info!("fade: 0x{id:08X} {way} done");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fade_goes_in_a_straight_line_over_its_length() {
        assert_eq!(fade_alpha(true, 0.0), 0.0);
        assert_eq!(fade_alpha(true, FADE_SECONDS / 2.0), 0.5);
        assert_eq!(fade_alpha(true, FADE_SECONDS * 2.0), 1.0);
        assert_eq!(fade_alpha(false, 0.0), 1.0);
        assert_eq!(fade_alpha(false, FADE_SECONDS / 4.0), 0.75);
        assert_eq!(fade_alpha(false, FADE_SECONDS), 0.0);
        assert_eq!(fade_alpha(true, -1.0), 0.0, "before its start: not yet drawn");
    }

    #[test]
    fn presence_and_actor_alpha_compose_multiplicatively_and_restore_in_all_orders() {
        for actor_first in [false, true] {
            for actor_ends_first in [false, true] {
                let mut world = World::new();
                world.init_resource::<Assets<NifMaterial>>();
                let own = world.resource_mut::<Assets<NifMaterial>>().add(nvf_assets::nif_material(StandardMaterial {
                    base_color: Color::linear_rgba(1.0, 1.0, 1.0, 0.8),
                    alpha_mode: AlphaMode::Mask(0.5),
                    ..default()
                }));
                let e = world.spawn(MeshMaterial3d(own.clone())).id();

                if actor_first {
                    apply_factor(&mut world, e, FadeChannel::ActorAlpha, Some(0.5));
                    apply_factor(&mut world, e, FadeChannel::Presence, Some(0.25));
                } else {
                    apply_factor(&mut world, e, FadeChannel::Presence, Some(0.25));
                    apply_factor(&mut world, e, FadeChannel::ActorAlpha, Some(0.5));
                }

                let (copy, orig) = {
                    let f = world.get::<Fading>(e).expect("single owner active");
                    assert_eq!(f.original(), &own);
                    assert!((f.factor() - 0.125).abs() < 1e-6);
                    (f.copy().clone(), f.original().clone())
                };
                assert_eq!(orig, own);
                assert_eq!(world.get::<MeshMaterial3d<NifMaterial>>(e).unwrap().0, copy);
                {
                    let m = world.resource::<Assets<NifMaterial>>().get(&copy).unwrap();
                    assert!((m.base.base_color.alpha() - 0.8 * 0.125).abs() < 1e-6);
                    assert!((m.extension.alpha_cut - 0.5 * 0.125).abs() < 1e-6);
                    assert_eq!(m.base.alpha_mode, AlphaMode::Blend);
                    assert!(!m.extension.depth_write);
                }

                // Changing actor alpha while presence runs updates the product in place.
                apply_factor(&mut world, e, FadeChannel::ActorAlpha, Some(0.75));
                {
                    let m = world.resource::<Assets<NifMaterial>>().get(&copy).unwrap();
                    assert!((m.base.base_color.alpha() - 0.8 * 0.75 * 0.25).abs() < 1e-6);
                    assert!((m.extension.alpha_cut - 0.5 * 0.75 * 0.25).abs() < 1e-6);
                }

                if actor_ends_first {
                    apply_factor(&mut world, e, FadeChannel::ActorAlpha, None);
                    assert!(world.get::<Fading>(e).is_some());
                    let m = world.resource::<Assets<NifMaterial>>().get(&copy).unwrap();
                    assert!((m.base.base_color.alpha() - 0.8 * 0.25).abs() < 1e-6);
                    apply_factor(&mut world, e, FadeChannel::Presence, None);
                } else {
                    apply_factor(&mut world, e, FadeChannel::Presence, None);
                    assert!(world.get::<Fading>(e).is_some());
                    let m = world.resource::<Assets<NifMaterial>>().get(&copy).unwrap();
                    assert!((m.base.base_color.alpha() - 0.8 * 0.75).abs() < 1e-6);
                    apply_factor(&mut world, e, FadeChannel::ActorAlpha, None);
                }

                assert!(world.get::<Fading>(e).is_none());
                assert!(!world.resource::<Assets<NifMaterial>>().contains(copy.id()), "copy removed on final release");
                assert_eq!(world.get::<MeshMaterial3d<NifMaterial>>(e).unwrap().0, own);
                let restored = world.resource::<Assets<NifMaterial>>().get(&own).unwrap();
                assert_eq!(restored.base.base_color.alpha(), 0.8);
                assert_eq!(restored.base.alpha_mode, AlphaMode::Mask(0.5));
            }
        }
    }

    #[test]
    fn retarget_updates_original_and_rebuilds_copy_in_place() {
        let mut world = World::new();
        world.init_resource::<Assets<NifMaterial>>();
        let own = world.resource_mut::<Assets<NifMaterial>>().add(nvf_assets::nif_material(StandardMaterial::default()));
        let edited = world.resource_mut::<Assets<NifMaterial>>().add(nvf_assets::nif_material(StandardMaterial {
            base_color: Color::linear_rgba(0.2, 0.4, 0.6, 0.5),
            emissive: LinearRgba::rgb(0.9, 0.1, 0.2),
            ..default()
        }));
        let e = world.spawn(MeshMaterial3d(own.clone())).id();

        apply_factor(&mut world, e, FadeChannel::Presence, Some(0.4));
        apply_factor(&mut world, e, FadeChannel::ActorAlpha, Some(0.5));
        let copy = world.get::<Fading>(e).unwrap().copy().clone();

        apply_retarget(&mut world, e, edited.clone());
        assert_eq!(world.get::<Fading>(e).unwrap().original(), &edited);
        assert_eq!(world.get::<Fading>(e).unwrap().copy(), &copy);
        {
            let m = world.resource::<Assets<NifMaterial>>().get(&copy).unwrap();
            assert!((m.base.base_color.alpha() - 0.5 * 0.4 * 0.5).abs() < 1e-6);
            assert_eq!(m.base.emissive, LinearRgba::rgb(0.9, 0.1, 0.2));
        }

        apply_factor(&mut world, e, FadeChannel::Presence, None);
        apply_factor(&mut world, e, FadeChannel::ActorAlpha, None);
        assert!(world.get::<Fading>(e).is_none());
        assert_eq!(world.get::<MeshMaterial3d<NifMaterial>>(e).unwrap().0, edited);
    }

    /// #3919: a reference enabled with a fade whose model scrolls its texture (the Hidden
    /// Valley storm's sheets). The fade's copy goes on before the model's rig binds its
    /// material; the rig's own is made from the mesh's own, fades in as it moves, and is
    /// what is drawn after the fade.
    #[test]
        fn an_animated_model_bound_while_it_fades_in_keeps_its_own_material_moving() {
        use fnv_nif::anim::{Channel, Key, KeyKind, Keys, ValueTrack};
        use nvf_assets::anim_play::{Clip, MaterialTarget, MaterialTrack, Playing};
        use nvf_assets::material_anim::{animate_rig_materials, bind_rig_materials, MaterialSlot, RigMaterials};
        use std::sync::Arc;
        use std::time::Duration;

        const ID: u32 = 0x0000_0D01;
        let mut app = App::new();
        app.insert_resource(SimState::over_placed([crate::sim::placed_ref(ID)]))
            .init_resource::<Assets<NifMaterial>>()
            .init_resource::<Time<Virtual>>()
            .add_plugins(FadePlugin)
            .add_systems(Update, (bind_rig_materials, animate_rig_materials).chain().after(step));
        let original = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(nvf_assets::nif_material::nif_material(StandardMaterial::default()));
        let mesh = app.world_mut().spawn((RefVisual(ID), MeshMaterial3d(original.clone()), Visibility::Inherited)).id();
        let worn = |app: &App| app.world().get::<MeshMaterial3d<NifMaterial>>(mesh).unwrap().0.clone();
        let look = |app: &App, h: &Handle<NifMaterial>| {
            let m = app.world().resource::<Assets<NifMaterial>>().get(h).unwrap();
            (m.base.uv_transform.translation.x, m.base.base_color.alpha())
        };

        app.world_mut().write_message(RefFade { form_id: ID, fade_in: true });
        app.update();
        let copy = app.world().get::<Fading>(mesh).unwrap().copy().clone();
        assert_eq!(worn(&app), copy);

        // The model's scroll: half a repeat a second, on a 4 s loop.
        let track = MaterialTrack {
            shape: "sheet".into(),
            target: MaterialTarget::UvOffset(0),
            cycle: None,
            priority: 0,
            values: Arc::new(ValueTrack::Float(Channel::Keys(Keys {
                kind: KeyKind::Linear,
                keys: [(0.0, 0.0), (4.0, 2.0)].into_iter().map(|(time, value)| Key { time, value: [value], forward: [0.0], backward: [0.0], tbc: [0.0; 3] }).collect(),
            }))),
        };
        let mut clip = Clip::from_controllers(vec![track.clone()], Vec::new(), Vec::new());
        clip.stop = 4.0;
        clip.cycle_type = 0;
        let rig = app
            .world_mut()
            .spawn((Playing::new("sheet", &clip, |_| None), RigMaterials { slots: vec![MaterialSlot::new("sheet".into(), mesh, original.clone(), [0.0; 3], 1.0, 1.0)], idle: Arc::new(vec![track]), dark: false, fade: 1.0 }))
            .id();
        let play = |app: &mut App, played: f32, faded: f32| {
            app.world_mut().get_mut::<Playing>(rig).unwrap().time = played;
            app.world_mut().resource_mut::<Time<Virtual>>().advance_by(Duration::from_secs_f32(faded));
            app.update();
            app.world().get::<RigMaterials>(rig).unwrap().slots[0].handle.clone()
        };

        // Bound while it fades: the fade's copy stays on, its source the rig's own.
        let own = play(&mut app, 0.5, 0.0);
        assert_ne!(own, original, "the rig has its own copy");
        assert_ne!(own, copy, "made from the mesh's own, not the fade's copy");
        assert_eq!(worn(&app), copy);
        assert_eq!(app.world().get::<HeldCopy>(mesh).unwrap().source, own);
        // The fade follows it: the copy drawn moves as the rig's own does, see-through.
        for (played, alpha) in [(1.0, 0.25), (2.0, 0.5)] {
            assert_eq!(play(&mut app, played, 0.5), own);
            assert_eq!(worn(&app), copy);
            assert_eq!(app.world().get::<Fading>(mesh).unwrap().original(), &own);
            assert_eq!(look(&app, &own), (played * 0.5, 1.0), "its own stays opaque");
            // The copy is made after this frame's rig pose (`follow_rig_materials`).
            assert_eq!(look(&app, &copy), (played * 0.5, alpha), "at {played} s the copy moves with it");
        }

        // At its end the rig's own is drawn, and it goes on moving.
        play(&mut app, 3.0, 2.0);
        assert!(app.world().get::<Fading>(mesh).is_none());
        assert!(app.world().get::<HeldCopy>(mesh).is_none());
        assert!(!app.world().resource::<Assets<NifMaterial>>().contains(copy.id()));
        assert_eq!(worn(&app), own);
        assert!(app.world().resource::<Fades>().renewed.is_empty(), "counted in the done line, then dropped");
        play(&mut app, 3.5, 0.5);
        assert_eq!(worn(&app), own);
        assert_eq!(look(&app, &own), (1.75, 1.0));
    }

    /// #3919 with #3105: a retarget (SwapTexture) while a rig's own material is followed
    /// under the see-through copy makes the swapped material the one held, followed and
    /// given back.
    #[test]
    fn retarget_while_a_rigs_own_material_is_followed_moves_the_held_copy_too() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        world.init_resource::<Assets<NifMaterial>>();
        world.init_resource::<Fades>();
        let add = |world: &mut World, alpha: f32, emissive: LinearRgba| {
            world.resource_mut::<Assets<NifMaterial>>().add(nvf_assets::nif_material(StandardMaterial { base_color: Color::linear_rgba(1.0, 1.0, 1.0, alpha), emissive, ..default() }))
        };
        let own = add(&mut world, 1.0, LinearRgba::BLACK);
        let rig_own = add(&mut world, 1.0, LinearRgba::rgb(0.0, 1.0, 0.0));
        let edited = add(&mut world, 0.5, LinearRgba::rgb(0.9, 0.1, 0.2));
        let e = world.spawn(MeshMaterial3d(own.clone())).id();

        apply_factor(&mut world, e, FadeChannel::Presence, Some(0.4));
        let copy = world.get::<Fading>(e).unwrap().copy().clone();
        {
            let h = world.get::<HeldCopy>(e).expect("the new copy is held");
            assert_eq!((&h.source, &h.copy), (&own, &copy));
        }
        // A rig binds: its own copy is held beneath the fade's (`bind_rig_materials`).
        world.get_mut::<HeldCopy>(e).unwrap().source = rig_own.clone();
        world.run_system_once(follow_rig_materials).unwrap();
        assert_eq!(world.get::<Fading>(e).unwrap().original(), &rig_own);
        assert_eq!(world.resource::<Assets<NifMaterial>>().get(&copy).unwrap().base.emissive, LinearRgba::rgb(0.0, 1.0, 0.0));

        apply_retarget(&mut world, e, edited.clone());
        {
            let h = world.get::<HeldCopy>(e).unwrap();
            assert_eq!((&h.source, &h.copy), (&edited, &copy));
        }
        // Followed from the swapped material now, not taken back to the rig's.
        world.run_system_once(follow_rig_materials).unwrap();
        assert_eq!(world.get::<Fading>(e).unwrap().original(), &edited);
        assert_eq!(world.get::<MeshMaterial3d<NifMaterial>>(e).unwrap().0, copy);
        {
            let m = world.resource::<Assets<NifMaterial>>().get(&copy).unwrap();
            assert_eq!(m.base.emissive, LinearRgba::rgb(0.9, 0.1, 0.2));
            assert!((m.base.base_color.alpha() - 0.5 * 0.4).abs() < 1e-6);
        }

        apply_factor(&mut world, e, FadeChannel::Presence, None);
        assert!(world.get::<Fading>(e).is_none());
        assert!(world.get::<HeldCopy>(e).is_none());
        assert!(!world.resource::<Assets<NifMaterial>>().contains(copy.id()));
        assert_eq!(world.get::<MeshMaterial3d<NifMaterial>>(e).unwrap().0, edited);
    }
}
