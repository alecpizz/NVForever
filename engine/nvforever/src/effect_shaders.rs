//! Effect shaders on references (EFSH, docs/spec-notes/effect-shaders.md) and actors'
//! alpha (`SetActorAlpha`).
//!
//! - **What plays**: the shaders scripts play (`PlayMagicShaderVisuals`,
//!   `fnv_game::functions::misc::ScriptLooks`) and those of the magic effects active on
//!   actors (MGEF "effect shader", for as long as the effect lasts).
//! - **Drawing**: the membrane only (§4 of the notes). Every mesh drawn for the reference
//!   gets up to two overlays drawn on the same surface (its mesh, and its skin when it
//!   has one): the fill (texture × colour, panned by the texture speed) and the edge
//!   (the edge colour, faded in towards the outline by the NIF material's view-angle
//!   falloff; an "Edge Effect Inverse" over an additive fill darkens instead, #3017).
//!   Blended meshes (beards, lashes) get none: the overlays do not carry the
//!   mesh's texture alpha (but under a hologram colour, #3017: `hologram_cover`); nor do
//!   refracting ones (heat haze), which show no surface. Alpha-tested meshes (a robot's body, cut-out hair) get copies
//!   cut by their own texture's alpha at their own cut (the holes test turned round,
//!   `nvf_assets::nif_material` "Holes"; none when the shader has holes of its own),
//!   #1717. The player's are the meshes of its
//!   third-person body. Both are unlit, cast no shadow and sit a decal's depth step in
//!   front of the surface. Their alpha follows the record's cycle every frame; a mesh drawn later
//!   (a cell loaded again, armour put on) gets its overlays then.
//! - **Holes** (§7 of the notes): a shader with a holes texture gives every mesh it
//!   covers (and its own overlays) a copy of the mesh's material that the holes texture
//!   eats away as the record's value moves (`nvf_assets::nif_material`, "Holes"), the
//!   band next to the holes in the holes' edge colour; an opaque mesh's copy is drawn
//!   alpha-tested so the depth passes cut the same holes. When the play ends the mesh
//!   gets its own material back.
//! - **Add-on models** (§8): a shader naming a DEBR puts one of its models (by the
//!   models' percentages) on up to [`ADDON_PLACES`] bones of the reference's skinned
//!   meshes, spread over them (an unskinned reference: one, at its first mesh), each
//!   played as an effect model (`crate::shot_fx::spawn_attached`: billboards turn to the
//!   camera, its clip plays), grown and faded in from the start and shrunk and faded out
//!   after the end by the record's add-on timing.
//! - **Ending**: a script's play with a duration ends after max(duration, one cycle),
//!   without one when stopped; an effect's when the effect ends.
//! - **Actor alpha**: the meshes of an actor with an alpha below 1 get blended copies of
//!   their materials with the alpha multiplied in; 1 gives the originals back. Their
//!   effect shaders' overlays stay on (covered as the mesh's own material) and fade with
//!   the alpha drawn (#3017).
//! - **Hologram colours** (#3017): which colour plays is settled at the script call
//!   (`fnv_game::functions::misc::ScriptLooks::play`); here every script play is drawn.
//!
//! Log: `effect shader: <EDID> on <who> (…)` when one starts, `… ends (…)` when it ends,
//! `effect shader: <EDID> on <who> fades with its actor to <a> (<n> overlays on <m>
//! meshes)` when the alpha its actor is drawn with changes under it, `effect shaders by
//! script on <who>: <EDIDs, or none>` when a reference's set of script-played shaders
//! changes,
//! `effect shader: <EDID> holes on <n> meshes of <who> (…)`, `effect shader: <EDID> puts
//! <n> add-on models of DEBR <id> on <who> (…)`, `actor alpha: <who> <a> (<n> meshes)`,
//! and at exit `effect shaders summary: …`.

use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::mesh::skinning::SkinnedMesh;
use bevy::prelude::*;
use fnv_game::data::MagicEffectDef;
use fnv_sim::FormId;
use fnv_world::effect_shader::{blend, Addons, EffectShader, Holes, EDGE_INVERSE};
use nvf_assets::material_anim::RigMaterials;
use nvf_assets::model::ModelCtx;
use nvf_assets::nif_material::nif_material;
use nvf_assets::NifMaterial;
use nvf_play::script::ScriptState;
use nvf_play::RefVisual;
use std::collections::{HashMap, HashSet};

use crate::world::WorldData;

pub struct EffectShadersPlugin;

impl Plugin for EffectShadersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Shaders>()
            .init_resource::<crate::effect_particles::EffectParticles>()
            .add_systems(Update, (play.after(nvf_play::player::BodySet).after(actor_alpha), cover.after(play), sync_player_effects.after(cover), actor_alpha.after(crate::texture_swap::apply).after(nvf_play::PlaySet::Sync), crate::effect_particles::draw.after(sync_player_effects), addons_leaving.after(play)))
            .add_systems(PostUpdate, crate::effect_particles::build.in_set(crate::particles::ParticleDrawSet))
            .add_systems(Last, (report, crate::effect_particles::report));
        crate::particles::configure_draw_set(app);
    }
}

#[cfg(test)]
#[derive(Resource)]
pub(crate) struct PlaybackTestGame(pub(crate) fnv_game::Game<'static>);

#[cfg(test)]
pub(crate) fn holes_playback_for_test(app: &mut App, on: u32, game: fnv_game::Game<'static>) -> Source {
    let fill = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(nif_material(StandardMaterial::default()));
    let mut active = Active::membrane_for_test(on, fill);
    active.shader.holes = Some(Holes {
        texture: "synthetic-holes.dds".into(), start_time: 0.0, end_time: 10.0,
        start_value: 255.0, end_value: 0.0, edge_width: 0.0, edge_color: [0; 3],
    });
    active.holes_texture = Some(Handle::default());
    let source = Source::Script(1);
    let mut game = game;
    game.script_looks.shaders.push(fnv_game::functions::misc::ShaderPlay { on: FormId(on), shader: FormId(1), duration: None, serial: 1 });
    app.init_resource::<Shaders>()
        .init_resource::<Time>()
        .insert_resource(PlaybackTestGame(game))
        .add_systems(Update, (play, cover).chain());
    app.world_mut().resource_mut::<Shaders>().active.insert(source, active);
    source
}

#[cfg(test)]
impl Active {
    /// [design, #2557] Synthetic membrane for production coverage regressions.
    pub(crate) fn membrane_for_test(on: u32, fill: Handle<NifMaterial>) -> Active {
        let shader = EffectShader {
            form_id: 1, edid: "SyntheticUncloak".into(), flags: 0,
            fill_texture: None, particle_texture: None,
            source_blend: blend::SOURCE_ALPHA, blend_op: 1, z_test: 1,
            dest_blend: blend::SOURCE_INVERTED_ALPHA, fill_color: [255; 3],
            fill: Default::default(), fill_speed: [0.0; 2], edge_color: [0; 3],
            edge: Default::default(), edge_fall_off: 1.0,
            particles: None, holes: None, addons: None,
        };
        let mut a = Self::for_test(on, shader, 0.0, &[]);
        a.fill = Some(fill);
        a
    }

    /// A play of `shader` on reference `on` since `since` (persistent), covering the meshes
    /// `covered`, without overlays: for tests of the effect particles.
    pub(crate) fn for_test(on: u32, shader: EffectShader, since: f32, covered: &[Entity]) -> Active {
        Active {
            on,
            shader: Box::new(shader),
            since,
            until: None,
            persist: true,
            fill: None,
            edge: None,
            cut: HashMap::new(),
            coverage_state: HashMap::new(),
            covered: covered.iter().map(|e| (*e, Vec::new())).collect(),
            camera_sees: None,
            holes_texture: None,
            holed: HashMap::new(),
            addons: Vec::new(),
            faded_by: 1.0,
            logged_fade: 1.0,
            hologram: false,
        }
    }
}

/// What a play comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Source {
    /// A script's play (its serial).
    Script(u64),
    /// An active magic effect (its id).
    Effect(u32),
    /// A companion perk's mark while the player aims (`nvf_play::perk_marks`): the
    /// reference and the shader.
    Perk(u32, u32),
}

/// An overlay's fill and edge materials.
type Looks = [Option<Handle<NifMaterial>>; 2];

/// A shader playing on a reference.
pub(crate) struct Active {
    pub(crate) on: u32,
    pub(crate) shader: Box<EffectShader>,
    /// `Time::elapsed_secs` when it started.
    pub(crate) since: f32,
    /// Ends at this many seconds from `since`; `None`: until stopped.
    until: Option<f32>,
    /// The persistent part shows after the cycle.
    pub(crate) persist: bool,
    fill: Option<Handle<NifMaterial>>,
    edge: Option<Handle<NifMaterial>>,
    /// Copies of `fill` and `edge` cut by an alpha-tested mesh's own texture, by that
    /// texture and cut (in alpha units): see [`cut_out_overlay`].
    cut: HashMap<(AssetId<Image>, u8), Looks>,
    /// The meshes covered, and their overlays.
    pub(crate) covered: HashMap<Entity, Vec<Entity>>,
    /// [design: sync_player_effects] Last reported camera-visible and total meshes.
    camera_sees: Option<(usize, usize)>,
    /// [design, #2557] Coverage depends on the plain source and the derived cloak.
    coverage_state: HashMap<Entity, (AssetId<NifMaterial>, bool)>,
    /// The holes texture, when the shader has holes and it was found.
    holes_texture: Option<Handle<Image>>,
    /// Meshes given a material with holes: their own material and the copy.
    holed: HashMap<Entity, (Handle<NifMaterial>, Handle<NifMaterial>)>,
    /// The add-on models put on the reference: (entity above the model, rig root that
    /// fades it); empty until the reference is drawn.
    addons: Vec<(Entity, Option<Entity>)>,
    /// The actor alpha the overlays are faded by: that of the see-through copy its covered
    /// meshes wear (`Faded`), 1 with none (#3017).
    pub(crate) faded_by: f32,
    /// `faded_by` as last logged with the live overlays (`cover`).
    logged_fade: f32,
    /// One of the hologram colours (`fnv_game::functions::misc::hologram_colour`): its
    /// blended and cut-out meshes are covered too (`hologram_cover`, #3017).
    pub(crate) hologram: bool,
}

impl Active {
    /// The material copies the play writes: overlay fill and edge, their cut copies and
    /// the copies with holes (the profile aid's `profile change sources:` line).
    pub(crate) fn materials(&self) -> impl Iterator<Item = AssetId<NifMaterial>> + '_ {
        let looks = self.fill.iter().chain(&self.edge).chain(self.cut.values().flatten().flatten());
        looks.map(Handle::id).chain(self.holed.values().map(|(_, copy)| copy.id()))
    }
}

/// How many bones of a reference get an add-on model at most [design: the record does
/// not say how many; effect-shaders.md §8].
pub const ADDON_PLACES: usize = 8;

/// Add-on models shrinking and fading after their play ended.
struct Leaving {
    addons: Addons,
    models: Vec<(Entity, Option<Entity>)>,
    /// `Time::elapsed_secs` of the play's start and end.
    since: f32,
    ended: f32,
}

/// Put on an overlay mesh (not covered itself, not faded by actor alpha).
#[derive(Component)]
pub(crate) struct Overlay;

/// [design: sync_player_effects] Only player overlays need to follow view changes.
#[derive(Component)]
pub(crate) struct PlayerOverlay(Entity);

/// [design: player_body::Seen] A player's add-on holder follows this body part.
#[derive(Component)]
pub(crate) struct PlayerAddon(crate::player_body::EffectPart);

/// [design: particles::emit] Check the part directly, even before visibility propagates.
#[derive(Component)]
pub(crate) struct PlayerEffectEmitter(pub(crate) crate::player_body::EffectPart);

fn overlay_layers(mesh: Option<&RenderLayers>) -> RenderLayers {
    mesh.cloned().unwrap_or_default()
}

pub(crate) fn player_mesh_seen(mesh: Option<&RenderLayers>) -> bool {
    use crate::sky::{ARMS_LAYER, BODY_LAYER};
    overlay_layers(mesh).intersects(&RenderLayers::from_layers(&[0, ARMS_LAYER, BODY_LAYER]))
}

/// [design: player_body::show_rigs] Render layers do not propagate to children.
pub(crate) fn sync_player_effects(
    mut st: ResMut<Shaders>,
    body: Option<Res<crate::player_body::PlayerBody>>,
    meshes: Query<&RenderLayers, Without<Overlay>>,
    shown: Query<&InheritedVisibility>,
    mut overlays: Query<(&PlayerOverlay, &mut RenderLayers), With<Overlay>>,
    mut addons: Query<(&PlayerAddon, &mut Visibility)>,
) {
    for (parent, mut layer) in &mut overlays {
        let target = overlay_layers(meshes.get(parent.0).ok());
        if *layer != target {
            *layer = target;
        }
    }
    for (part, mut visibility) in &mut addons {
        let target = if body.as_deref().is_some_and(|b| b.sees_effect(part.0)) { Visibility::Inherited } else { Visibility::Hidden };
        if *visibility != target {
            *visibility = target;
        }
    }
    for a in st.active.values_mut().filter(|a| a.on == FormId::PLAYER_REF.0) {
        let seen = a.covered.keys().filter(|e| player_mesh_seen(meshes.get(**e).ok()) && !shown.get(**e).is_ok_and(|v| !v.get())).count();
        let counts = (seen, a.covered.len());
        if a.camera_sees != Some(counts) {
            info!("effect shader: {} on the player: the camera sees {} of {} covered meshes", a.shader.edid, counts.0, counts.1);
            a.camera_sees = Some(counts);
        }
    }
}

/// [design: shot_fx::spawn_attached] Mark only this player's add-on emitters once,
/// after the commands that create the model's node tree have been applied.
fn player_addon(commands: &mut Commands, holder: Entity, part: crate::player_body::EffectPart, seen: bool) {
    commands.entity(holder).insert((PlayerAddon(part), if seen { Visibility::Inherited } else { Visibility::Hidden }));
    commands.queue(move |world: &mut World| {
        let mut pending = vec![holder];
        while let Some(e) = pending.pop() {
            if let Some(children) = world.get::<Children>(e) {
                pending.extend(children.iter());
            }
            if world.get::<crate::particles::AddonEmitter>(e).is_some() || world.get::<crate::particles::ModelEmitter>(e).is_some() {
                world.entity_mut(e).insert(PlayerEffectEmitter(part));
            }
        }
    });
}

#[derive(Resource, Default)]
pub(crate) struct Shaders {
    records: HashMap<u32, Option<Box<EffectShader>>>,
    pub(crate) active: HashMap<Source, Active>,
    leaving: Vec<Leaving>,
    /// The script-played shaders on each reference as last logged (#3017).
    script_sets: HashMap<u32, Vec<String>>,
    played: u32,
    holed: u32,
    addons: u32,
    ended: u32,
    overlays: u32,
    alpha_calls: u32,
}

fn linear(c: [u8; 3]) -> LinearRgba {
    Color::srgb_u8(c[0], c[1], c[2]).to_linear()
}

/// How a membrane blends onto what lies behind it, from its destination blend (notes §4).
fn alpha_mode(s: &EffectShader) -> AlphaMode {
    match s.dest_blend {
        blend::SOURCE_INVERTED_ALPHA => AlphaMode::Blend,
        blend::DEST_COLOR => AlphaMode::Multiply,
        blend::ONE | blend::SOURCE_ALPHA => AlphaMode::Add,
        _ => AlphaMode::Add,
    }
}

/// The view-angle falloff of the edge: none facing the camera, half at cos θ =
/// 1 − 0.5^(1/fall off), a linear ramp through it (notes §4, design).
fn edge_falloff(fall_off: f32) -> Vec4 {
    let f = if fall_off.is_finite() && fall_off > 0.01 { fall_off } else { 1.0 };
    let half = 1.0 - 0.5f32.powf(1.0 / f);
    Vec4::new(1.0, 2.0 * half - 1.0, 0.0, 1.0)
}

/// Whether the edge darkens the mesh rather than lighting it: "Edge Effect Inverse" makes
/// the edge's blend "inverted relative to the fill effect", so with an additive fill the
/// edge darkens "rather than lighten[s] the mesh" [documented: GECK wiki EffectShader,
/// Edge Effect]. Only that case, the wiki's example, is drawn inverted; with any other
/// fill blend the edge stays as it was [design: the wiki does not say what the inverse of
/// an alpha-blended or multiplied fill is] (effect-shaders.md §4, #3017).
fn edge_darkens(s: &EffectShader) -> bool {
    s.flags & EDGE_INVERSE != 0 && matches!(alpha_mode(s), AlphaMode::Add)
}

/// The edge overlay: the edge colour at the edge alpha, faded in towards the outline. An
/// inverse edge over an additive fill is drawn multiplied by 1 - colour, so where it
/// shows the mesh behind loses the edge colour times the alpha: destination × (1 - alpha
/// × colour), the nearest the renderer's blends come to subtracting the colour [design:
/// Bevy has no subtractive blend; on a bright surface the two agree, on a dark one
/// subtracting darkens more].
fn edge_material(s: &EffectShader) -> NifMaterial {
    let mut m = overlay_material(s, s.edge_color, None, Some(edge_falloff(s.edge_fall_off)));
    if edge_darkens(s) {
        let c = linear(s.edge_color);
        m.base.base_color = Color::linear_rgba(1.0 - c.red, 1.0 - c.green, 1.0 - c.blue, 0.0);
        m.base.alpha_mode = AlphaMode::Multiply;
        m.base.depth_bias = DARK_EDGE_AFTER_FILL;
    }
    m
}

/// The sort bias that draws a darkening edge after the additive fill on the same mesh:
/// both overlays share the mesh's centre, so without it their order in the transparent
/// pass is a tie. The fill's light is added first and the edge then takes the colour off
/// the mesh and the fill together, so the rim ends darker, as "darken rather than lighten
/// the mesh" reads [design, #3017]. Bevy adds a material's depth bias to its transparent
/// sort distance (a positive one sorts nearer, so later) and casts it to an integer for
/// the depth test's bias, where a value below 1 is 0: the edge is ordered, not moved.
const DARK_EDGE_AFTER_FILL: f32 = 0.5;

/// An overlay's alpha: the record's cycle at this moment times the alpha its actor is
/// drawn with (the see-through copy its meshes wear this frame; 1 with none), so a
/// membrane fades out with the actor it covers [hypothesis: effect-shaders.md §5, #3017].
fn overlay_alpha(cycle: f32, drawn_alpha: f32) -> f32 {
    cycle * drawn_alpha.clamp(0.0, 1.0)
}

fn overlay_material(s: &EffectShader, color: [u8; 3], texture: Option<Handle<Image>>, falloff: Option<Vec4>) -> NifMaterial {
    let mut m = nif_material(StandardMaterial {
        base_color: Color::from(linear(color)).with_alpha(0.0),
        base_color_texture: texture,
        unlit: true,
        alpha_mode: alpha_mode(s),
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    m.extension.decal = true;
    m.extension.hard_edges = true;
    if let Some(f) = falloff {
        m.extension.falloff = f;
    }
    m
}

/// A reference's drawn meshes an effect shader covers: not the overlays themselves, nor
/// the full-detail ring's draw-only copies (`nvf_play::DrawOnly`, full_detail.rs, #2701):
/// a scripted effect, its add-on models and its particles go on the loaded reference only.
type Coverable<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static RefVisual, &'static Mesh3d, Option<&'static SkinnedMesh>, Option<&'static MeshMaterial3d<NifMaterial>>, Option<&'static crate::mesh_terrain_blend::BlendShape>),
    (Without<Overlay>, Without<nvf_play::DrawOnly>),
>;

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn play(
    mut commands: Commands,
    time: Res<Time>,
    mut script: Option<ResMut<ScriptState>>,
    world: Option<Res<WorldData>>,
    mut models: ModelCtx,
    mut st: ResMut<Shaders>,
    drawn: Coverable,
    drawings: Query<&nvf_assets::nif_material::MaterialOverride>,
    (body, children, body_meshes): (
        Option<Res<crate::player_body::PlayerBody>>,
        Query<&Children>,
        Query<(&Mesh3d, Option<&SkinnedMesh>, &MeshMaterial3d<NifMaterial>), (Without<Overlay>, Without<RefVisual>)>,
    ),
    (combat, mut fades, mut transforms, perk_marks): (Option<Res<nvf_play::combat::Combat>>, Query<&mut RigMaterials>, Query<&mut Transform>, Option<Res<nvf_play::perk_marks::PerkMarks>>),
    see_through: Query<&nvf_play::fade::Fading>,
    #[cfg(test)] mut test_game: Option<ResMut<PlaybackTestGame>>,
) {
    let game = script.as_deref_mut().and_then(|s| s.rt.as_mut()).map(|rt| &mut rt.game);
    // [design, #2557] Tests supply a synthetic game at the runtime acquisition boundary.
    #[cfg(test)]
    let game = game.or_else(|| test_game.as_deref_mut().map(|g| &mut g.0));
    let (Some(game), Some(world)) = (game, world) else { return };
    let st = &mut *st;
    let now = time.elapsed_secs();

    // What should play: (source, reference, shader, duration, persist).
    let mut want: Vec<(Source, u32, u32, Option<f64>, bool)> =
        game.script_looks.shaders.iter().map(|p| (Source::Script(p.serial), p.on.0, p.shader.0, p.duration, true)).collect();
    for e in &game.effects {
        let Some(m) = game.data.magic_effects.get(&e.effect) else { continue };
        if let Some(sh) = m.shader {
            want.push((Source::Effect(e.id), e.target.0, sh.0, None, m.has(MagicEffectDef::FX_PERSIST)));
        }
    }
    if let Some(marks) = perk_marks.as_deref() {
        want.extend(marks.0.iter().map(|(r, sh, _)| (Source::Perk(r.0, sh.0), r.0, sh.0, None, true)));
    }
    let wanted: HashSet<Source> = want.iter().map(|w| w.0).collect();

    // Ends: stopped, the effect over, or its time up.
    let mut ended: Vec<(Source, &'static str)> = st.active.keys().filter(|k| !wanted.contains(k)).map(|k| (*k, "stopped")).collect();
    for (k, a) in &st.active {
        if a.until.is_some_and(|u| now - a.since >= u) {
            ended.push((*k, "its time is up"));
        }
    }
    let mut gone: HashSet<Source> = HashSet::new();
    for (k, why) in ended {
        let Some(a) = st.active.remove(&k) else { continue };
        gone.insert(k);
        for x in a.covered.values().flatten() {
            commands.entity(*x).try_despawn();
        }
        for h in [a.fill, a.edge].into_iter().chain(a.cut.into_values().flatten()).flatten() {
            models.materials.remove(&h);
        }
        // The meshes get their own materials back, unless something else changed them since.
        for (e, (original, copy)) in a.holed {
            let current = drawn.get(e).ok().and_then(|d| nvf_assets::nif_surface_blend::source_material(d.4, d.5))
                .or_else(|| body_meshes.get(e).ok().map(|d| &d.2 .0));
            if current.is_some_and(|m| nvf_assets::nif_material::plain_material(m, drawings.get(e).ok()) == &copy) {
                commands.entity(e).try_insert(MeshMaterial3d(original));
            }
            models.materials.remove(&copy);
        }
        if let Some(addons) = a.shader.addons.filter(|x| x.linger() > 0.0 && !a.addons.is_empty()) {
            st.leaving.push(Leaving { addons, models: a.addons, since: a.since, ended: now });
        } else {
            for (holder, _) in a.addons {
                commands.entity(holder).try_despawn();
            }
        }
        if let Source::Script(serial) = k {
            game.script_looks.end(serial);
        }
        st.ended += 1;
        info!("effect shader: {} on {} ends ({why})", a.shader.edid, game.data.name(FormId(a.on)));
    }

    // Starts.
    let played_before = st.played;
    for (k, on, shader, duration, persist) in want {
        if st.active.contains_key(&k) || gone.contains(&k) {
            continue;
        }
        let record = st.records.entry(shader).or_insert_with(|| fnv_world::effect_shader::effect_shader(world.lo, world.schema, shader).map(Box::new)).clone();
        let Some(s) = record else {
            // Not an EFSH: nothing to draw; a script's play is let go.
            if let Source::Script(serial) = k {
                warn!("effect shader: {shader:08X} is not an effect shader");
                game.script_looks.end(serial);
            }
            continue;
        };
        let mut load = |what: &str, t: &str| {
            let path = texture_path(t);
            let a = &mut models;
            let image = a.textures.get(&path, true, &a.data, &mut a.images, a.formats.as_deref());
            if image.is_none() {
                warn!("effect shader: {}: {what} texture {path} not found", s.edid);
            }
            image
        };
        let texture = s.fill_texture.as_deref().and_then(|t| load("fill", t));
        let holes_texture = s.holes.as_ref().and_then(|h| load("holes", &h.texture));
        let shows = |c: &fnv_world::effect_shader::AlphaCycle| c.full > 0.0 || (persist && c.persistent > 0.0);
        let with_holes = |mut m: NifMaterial| {
            if let (Some(h), Some(tex)) = (&s.holes, &holes_texture) {
                set_holes(&mut m, h, tex.clone(), h.value(0.0));
            }
            m
        };
        let fill = (s.has_membrane() && shows(&s.fill)).then(|| models.materials.add(with_holes(overlay_material(&s, s.fill_color, texture, None))));
        let edge = (s.has_membrane() && shows(&s.edge)).then(|| models.materials.add(with_holes(edge_material(&s))));
        let until = duration.map(|d| (d as f32).max(s.cycle()));
        let hologram = fnv_game::functions::misc::hologram_colour(&game.data.load_order, Some(&s.edid), FormId(shader)).is_some();
        st.played += 1;
        info!(
            "effect shader: {} on {} ({}{}, {})",
            s.edid,
            game.data.name(FormId(on)),
            if fill.is_some() { "fill" } else { "no fill" },
            match (edge.is_some(), edge_darkens(&s)) {
                (true, true) => " and darkening edge",
                (true, false) => " and edge",
                (false, _) => "",
            },
            match (until, k) {
                (Some(u), _) => format!("for {u:.1} s"),
                (None, Source::Script(_)) => "until stopped".to_string(),
                (None, Source::Effect(_)) => "while the effect lasts".to_string(),
                (None, Source::Perk(..)) => "while the player aims (a companion perk)".to_string(),
            }
        );
        st.active.insert(k, Active { on, shader: s, since: now, until, persist, fill, edge, cut: HashMap::new(), covered: HashMap::new(), coverage_state: HashMap::new(), camera_sees: None, holes_texture, holed: HashMap::new(), addons: Vec::new(), faded_by: 1.0, logged_fade: 1.0, hologram });
    }

    // #3017: the script-played shaders on each reference, logged when the set changes
    // (looked at only in a frame where a play started or ended).
    if !gone.is_empty() || st.played != played_before {
        let mut sets: HashMap<u32, Vec<String>> = HashMap::new();
        for (k, a) in &st.active {
            if matches!(k, Source::Script(_)) {
                sets.entry(a.on).or_default().push(a.shader.edid.clone());
            }
        }
        let gone_refs: Vec<u32> = st.script_sets.keys().filter(|r| !sets.contains_key(r)).copied().collect();
        for r in gone_refs {
            st.script_sets.remove(&r);
            info!("effect shaders by script on {}: none", game.data.name(FormId(r)));
        }
        for (r, mut set) in sets {
            set.sort();
            if st.script_sets.get(&r) != Some(&set) {
                info!("effect shaders by script on {}: {}", game.data.name(FormId(r)), set.join(", "));
                st.script_sets.insert(r, set);
            }
        }
    }

    // [documented: #1323] Ends and starts must run even when no shader remains to draw.
    if st.active.is_empty() {
        return;
    }

    // Cover only the references targeted by active shaders; animate the materials.
    type Drawn<'a> = (Entity, &'a Mesh3d, Option<&'a SkinnedMesh>, &'a Handle<NifMaterial>);
    let mut by_ref: HashMap<u32, Vec<Drawn<'_>>> = st.active.values().map(|a| (a.on, Vec::new())).collect();
    for (e, rv, mesh, skin, mat, blend) in &drawn {
        let Some(mat) = nvf_assets::nif_surface_blend::source_material(mat, blend) else { continue };
        if let Some(list) = by_ref.get_mut(&rv.0) {
            list.push((e, mesh, skin, mat));
        }
    }
    // [design: player_body::show_rigs] The third-person body has no RefVisual. Its
    // overlays copy its layers; attached add-on models are not the body's own meshes.
    if let Some((list, root)) = by_ref.get_mut(&FormId::PLAYER_REF.0).zip(body.as_deref().and_then(|b| b.roots().first().copied())) {
        let addon_parts: HashSet<Entity> = st.active.values().flat_map(|a| a.addons.iter().map(|x| x.0)).chain(st.leaving.iter().flat_map(|l| l.models.iter().map(|x| x.0))).flat_map(|h| std::iter::once(h).chain(children.iter_descendants(h))).collect();
        for e in children.iter_descendants(root) {
            if addon_parts.contains(&e) {
                continue;
            }
            if let Ok((mesh, skin, mat)) = body_meshes.get(e) {
                list.push((e, mesh, skin, &mat.0));
            }
        }
    }
    let (holed_count, addon_count) = (&mut st.holed, &mut st.addons);
    for a in st.active.values_mut() {
        let meshes = by_ref.get(&a.on).map(Vec::as_slice).unwrap_or(&[]);
        a.covered.retain(|e, _| meshes.iter().any(|(m, ..)| m == e));
        a.holed.retain(|e, _| meshes.iter().any(|(m, ..)| m == e));
        let t = now - a.since;
        // Holes: each mesh drawn with a copy of its material that the holes eat away.
        let holes = if let (Some(h), Some(tex)) = (a.shader.holes.as_ref(), a.holes_texture.as_ref()) {
            let value = h.value(t);
            let first = a.holed.is_empty();
            for &(e, _, _, mat) in meshes {
                let current = nvf_assets::nif_material::plain_material(mat, drawings.get(e).ok());
                if a.holed.get(&e).is_some_and(|(_, copy)| copy == current) {
                    continue;
                }
                // New, or something else gave it another material since: copy that one.
                let Some(own) = models.materials.get(current).cloned() else { continue };
                let mut m = own;
                set_holes(&mut m, h, tex.clone(), value);
                if matches!(m.base.alpha_mode, AlphaMode::Opaque) {
                    m.base.alpha_mode = AlphaMode::Mask(0.0);
                    m.extension.hard_edges = true;
                }
                // #2981: a holed copy is alpha-tested, so an authored height map comes off.
                nvf_assets::authored::recheck_height(&mut m);
                let copy = models.materials.add(m);
                commands.entity(e).try_insert(MeshMaterial3d(copy.clone()));
                if let Some((_, old)) = a.holed.insert(e, (current.clone(), copy)) {
                    models.materials.remove(&old);
                }
                *holed_count += 1;
            }
            if first && !a.holed.is_empty() {
                info!(
                    "effect shader: {} holes on {} meshes of {} ({}, {:.0} to {:.0} from {:.2} to {:.2} s, edge {:.1} units of ({}, {}, {}))",
                    a.shader.edid,
                    a.holed.len(),
                    game.data.name(FormId(a.on)),
                    texture_path(&h.texture),
                    h.start_value,
                    h.end_value,
                    h.start_time,
                    h.end_time,
                    h.edge_width,
                    h.edge_color[0],
                    h.edge_color[1],
                    h.edge_color[2]
                );
            }
            for (e, (_, copy)) in &a.holed {
                animate_material(&mut models.materials, copy, Some(value), None, None);
                // [design, #2557] Publish the updated plain source for cloak derivation.
                commands.entity(*e).try_insert(MeshMaterial3d(copy.clone()));
            }
            Some(value)
        } else {
            None
        };
        // Add-on models: put on once the reference is drawn, then grown and faded in.
        if let Some(ad) = a.shader.addons {
            if a.addons.is_empty() && !meshes.is_empty() {
                let mut bones: Vec<Entity> = meshes.iter().filter_map(|m| m.2).flat_map(|s| s.joints.iter().copied()).collect();
                bones.sort();
                bones.dedup();
                let places: Vec<Entity> = if bones.is_empty() { vec![meshes[0].0] } else { spread(&bones, ADDON_PLACES) };
                let choices: Vec<(u8, String)> = combat.as_deref().and_then(|c| c.forms.debris.get(&FormId(ad.debris))).map(|m| m.iter().map(|m| (m.percent, m.model.clone())).collect()).unwrap_or_default();
                let total: u32 = choices.iter().map(|c| c.0 as u32).sum();
                let mut first_model = String::new();
                for (i, &place) in places.iter().enumerate() {
                    if total == 0 {
                        break;
                    }
                    // A fixed pick per place and play [design].
                    let mut pick = (mix(a.on as u64 ^ ((a.since.to_bits() as u64) << 20) ^ i as u64) % total as u64) as i64;
                    let Some((_, model)) = choices.iter().find(|c| {
                        pick -= c.0 as i64;
                        pick < 0
                    }) else {
                        continue;
                    };
                    let path = fnv_actor::resolve::mesh_path(model);
                    let Some(pm) = models.model(&path) else { continue };
                    let (scale, _) = ad.look(t, None);
                    let made = crate::shot_fx::spawn_attached(&mut commands, place, &pm, Transform::from_scale(Vec3::splat(scale)), &path);
                    if a.on == FormId::PLAYER_REF.0 {
                        let part = body.as_deref().map_or(crate::player_body::EffectPart::Body, |b| b.effect_part(place));
                        let seen = body.as_deref().is_some_and(|b| b.sees_effect(part));
                        player_addon(&mut commands, made.0, part, seen);
                    }
                    if first_model.is_empty() {
                        first_model = path;
                    }
                    a.addons.push(made);
                }
                if a.addons.is_empty() {
                    // Nothing to put on (no DEBR, or none of its models loads): do not try again.
                    a.shader.addons = None;
                    warn!("effect shader: {}: no add-on model of DEBR {:08X} could be drawn", a.shader.edid, ad.debris);
                } else {
                    *addon_count += a.addons.len() as u32;
                    info!(
                        "effect shader: {} puts {} add-on models of DEBR {:08X} on {} ({} first; fade in {:.2} s, out {:.2} s, scale {:.2} to 1 over {:.2} s, to {:.2} over {:.2} s at the end)",
                        a.shader.edid,
                        a.addons.len(),
                        ad.debris,
                        game.data.name(FormId(a.on)),
                        first_model,
                        ad.fade_in,
                        ad.fade_out,
                        ad.scale_start,
                        ad.scale_in,
                        ad.scale_end,
                        ad.scale_out
                    );
                }
            }
            let (scale, alpha) = ad.look(t, None);
            addon_look(&a.addons, scale, alpha, &mut transforms, &mut fades);
        }
        // #3017: the overlays fade with the alpha their actor is drawn with (Dead Money's
        // holograms flicker theirs between 0, 0.7 and 1 while their membrane plays): that of
        // the see-through copy its meshes wear this frame (`actor_alpha` runs first), so a
        // play started on an actor already unseen starts unseen. A mesh no copy is made for
        // (the player's third-person body, which `AlphaDrawing` does not reach) keeps 1.
        a.faded_by = meshes.iter().find_map(|(e, ..)| see_through.get(*e).ok().map(nvf_play::fade::Fading::factor)).unwrap_or(1.0);
        let drawn_alpha = a.faded_by;
        // The cut copies (made only for shaders without holes) follow their originals.
        let fills = a.fill.iter().chain(a.cut.values().filter_map(|c| c[0].as_ref()));
        let [u, v] = a.shader.fill_speed;
        let uv = bevy::math::Affine2::from_translation(Vec2::new((u * t).fract(), (v * t).fract()));
        for fill in fills {
            animate_material(&mut models.materials, fill, holes, Some(overlay_alpha(a.shader.fill.alpha(t, a.persist), drawn_alpha)), Some(uv));
        }
        for edge in a.edge.iter().chain(a.cut.values().filter_map(|c| c[1].as_ref())) {
            animate_material(&mut models.materials, edge, holes, Some(overlay_alpha(a.shader.edge.alpha(t, a.persist), drawn_alpha)), None);
        }
    }
}

/// How a mesh is covered, from its own material.
#[derive(Debug, PartialEq)]
enum Cover {
    /// With the shader's overlays as they are (an opaque mesh).
    Plain,
    /// With copies cut where the mesh's own texture's alpha is below the cut (an
    /// alpha-tested mesh: a robot's body, #1717; hair drawn as a cut-out).
    CutBy(Handle<Image>, f32),
    /// Not at all: a blended mesh (beards, lashes), a refracting one (heat haze), or an
    /// alpha-tested one when the shader has holes of its own (one holes texture per
    /// material).
    None,
}

/// [design, #2557] Reconsider coverage when the plain material or cloak changes.
/// PostUpdate restoration is therefore visible to overlays on the next Update.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn cover(
    mut commands: Commands,
    mut st: ResMut<Shaders>,
    mut materials: ResMut<Assets<NifMaterial>>,
    script: Option<Res<ScriptState>>,
    drawn: Coverable,
    body: Option<Res<crate::player_body::PlayerBody>>,
    children: Query<&Children>,
    body_meshes: Query<(&Mesh3d, Option<&SkinnedMesh>, &MeshMaterial3d<NifMaterial>), (Without<Overlay>, Without<RefVisual>)>,
    layers: Query<&RenderLayers>,
    drawings: Query<&nvf_assets::nif_material::MaterialOverride>,
    see_through: Query<&nvf_play::fade::Fading>,
) {
    let st = &mut *st;
    type Drawn<'a> = (Entity, &'a Mesh3d, Option<&'a SkinnedMesh>, &'a Handle<NifMaterial>);
    let mut by_ref: HashMap<u32, Vec<Drawn<'_>>> = st.active.values().map(|a| (a.on, Vec::new())).collect();
    for (e, rv, mesh, skin, mat, blend) in &drawn {
        let Some(mat) = nvf_assets::nif_surface_blend::source_material(mat, blend) else { continue };
        if let Some(list) = by_ref.get_mut(&rv.0) {
            list.push((e, mesh, skin, mat));
        }
    }
    if let Some((list, root)) = by_ref.get_mut(&FormId::PLAYER_REF.0).zip(body.as_deref().and_then(|b| b.roots().first().copied())) {
        let addon_parts: HashSet<Entity> = st.active.values().flat_map(|a| a.addons.iter().map(|x| x.0))
            .chain(st.leaving.iter().flat_map(|l| l.models.iter().map(|x| x.0)))
            .flat_map(|h| std::iter::once(h).chain(children.iter_descendants(h))).collect();
        for e in children.iter_descendants(root) {
            if addon_parts.contains(&e) {
                continue;
            }
            if let Ok((mesh, skin, mat)) = body_meshes.get(e) {
                list.push((e, mesh, skin, &mat.0));
            }
        }
    }
    for a in st.active.values_mut() {
        let meshes = by_ref.get(&a.on).map(Vec::as_slice).unwrap_or(&[]);
        a.covered.retain(|e, _| meshes.iter().any(|(m, ..)| m == e));
        a.coverage_state.retain(|e, _| meshes.iter().any(|(m, ..)| m == e));
        let before = a.covered.len();
        // Meshes looked at this frame left without overlays: (blended or cut out, cloaked).
        let mut bare = (0usize, 0usize);
        for &(e, mesh, skin, mat) in meshes {
            let drawing = drawings.get(e).ok();
            let plain = nvf_assets::nif_material::plain_material(mat, drawing);
            // [design, #2557] Producers can publish a plain handle before PostUpdate
            // re-derives the cloak; ownership still suppresses overlays in that gap.
            let cloaked = drawing.is_some();
            // #3017: a mesh drawn see-through for its actor's alpha (`Faded`) is covered as
            // its own material: its blended copy counted as a blended mesh, and every time
            // the alpha dropped below 1 the overlays came off (a Dead Money hologram's
            // membrane went out whenever it flickered, and stayed out at 0.7). The
            // overlays fade with the alpha instead (`play`).
            let own = match (a.holed.get(&e), see_through.get(e).ok()) {
                (Some((original, _)), _) => original,
                (None, Some(f)) => f.original(),
                (None, None) => plain,
            };
            let state = (own.id(), cloaked);
            if a.coverage_state.insert(e, state) == Some(state) && a.covered.contains_key(&e) {
                continue;
            }
            if let Some(overlays) = a.covered.remove(&e) {
                for overlay in overlays {
                    commands.entity(overlay).try_despawn();
                }
            }
            if cloaked {
                bare.1 += 1;
                a.covered.insert(e, Vec::new());
                continue;
            }
            // A blended mesh (beards, lashes) gets no overlay: drawn without the mesh's
            // texture alpha, it covered every card of the hair in the membrane's colour
            // and the head showed bald (owner's playtest, 2026-10-24). An alpha-tested
            // one (a robot's body, #1717) gets overlays cut by its own texture. A hologram
            // colour covers blended and cut-out meshes too (#3017, `hologram_cover`).
            let holes = a.shader.holes.is_some();
            let look = match materials.get(own).map(|m| if a.hologram { hologram_cover(m, holes) } else { cut_out_overlay(m, holes) }) {
                Some(Cover::Plain) | None => [a.fill.clone(), a.edge.clone()],
                Some(Cover::CutBy(tex, cut)) => {
                    let key = (tex.id(), (cut * 255.0).round() as u8);
                    a.cut
                        .entry(key)
                        .or_insert_with(|| {
                            [&a.fill, &a.edge].map(|m| {
                                let copy = materials.get(m.as_ref()?)?.clone();
                                Some(materials.add(cut_by(copy, tex.clone(), cut)))
                            })
                        })
                        .clone()
                }
                Some(Cover::None) => {
                    bare.0 += 1;
                    a.covered.insert(e, Vec::new());
                    continue;
                }
            };
            let mut overlays = Vec::new();
            for mat in look.iter().flatten() {
                let mut c = commands.spawn((Mesh3d(mesh.0.clone()), MeshMaterial3d(mat.clone()), Transform::IDENTITY, Overlay, overlay_layers(layers.get(e).ok()), NoFrustumCulling, bevy::light::NotShadowCaster));
                if a.on == FormId::PLAYER_REF.0 {
                    c.insert(PlayerOverlay(e));
                }
                if let Some(skin) = skin {
                    c.insert(skin.clone());
                }
                let x = c.id();
                commands.entity(e).add_child(x);
                overlays.push(x);
            }
            st.overlays += overlays.len() as u32;
            a.covered.insert(e, overlays);
        }
        let who = |on: u32| script.as_deref().and_then(|s| s.rt.as_ref()).map_or_else(|| format!("0x{on:08X}"), |rt| rt.game.data.name(FormId(on)));
        if a.covered.len() != before && before == 0 && !a.covered.is_empty() {
            let with = a.covered.values().filter(|o| !o.is_empty()).count();
            info!("effect shader: {} covers {} meshes of {} (overlays on {with}; none on {} blended or cut out, {} cloaked)", a.shader.edid, a.covered.len(), who(a.on), bare.0, bare.1);
        }
        // #3017: after this frame's coverage, so the count says whether the see-through
        // copy left the overlays on.
        if a.faded_by != a.logged_fade {
            let live: usize = a.covered.values().map(Vec::len).sum();
            info!("effect shader: {} on {} fades with its actor to {:.2} ({live} overlays on {} meshes)", a.shader.edid, who(a.on), a.faded_by, a.covered.len());
            a.logged_fade = a.faded_by;
        }
    }
}

/// How a mesh drawn with `m` is covered (`shader_has_holes`: the shader eats its
/// overlays away with a holes texture).
fn cut_out_overlay(m: &NifMaterial, shader_has_holes: bool) -> Cover {
    // A refracting shape shows no surface, only the shimmer behind it: a Mister Gutsy's
    // jet haze (`##RefractCloudMesh`) was covered as a cluster of solid blobs under it
    // (#1717) [design].
    if m.extension.refract {
        return Cover::None;
    }
    match (m.base.alpha_mode, &m.base.base_color_texture) {
        (AlphaMode::Opaque, _) if m.extension.alpha_cut <= 0.0 => Cover::Plain,
        (AlphaMode::Mask(cut), Some(tex)) if m.extension.alpha_cut <= 0.0 && !shader_has_holes => Cover::CutBy(tex.clone(), cut),
        _ => Cover::None,
    }
}

/// How a hologram colour covers a mesh drawn with `m` (#3017): Dead Money's hologram
/// bodies are drawn see-through by their own materials, which [`cut_out_overlay`] leaves
/// bare (the membrane never showed on them: "0 overlays drawn" in smoke runs of
/// 2026-11-20 and 2026-11-21 with their shaders playing [observed]). The membrane is
/// their look, so a blended or cut-out mesh with a texture gets overlays cut where that
/// texture's alpha is below its own cut or [`HOLOGRAM_BODY_CUT`], one without a texture
/// gets them whole [design]. Refracting meshes and shaders with holes of their own keep
/// the general rule.
fn hologram_cover(m: &NifMaterial, shader_has_holes: bool) -> Cover {
    let general = cut_out_overlay(m, shader_has_holes);
    if general != Cover::None || m.extension.refract || shader_has_holes {
        return general;
    }
    let own_cut = match m.base.alpha_mode {
        AlphaMode::Mask(cut) => cut,
        _ => 0.0,
    };
    match &m.base.base_color_texture {
        Some(tex) => Cover::CutBy(tex.clone(), own_cut.max(m.extension.alpha_cut).max(HOLOGRAM_BODY_CUT)),
        None => Cover::Plain,
    }
}

/// The texture alpha below which a hologram's see-through body gets no membrane: its
/// empty texels, and with them faint ones below 8/255 [design, #3017].
const HOLOGRAM_BODY_CUT: f32 = 8.0 / 255.0;

/// A cut (0-1) in the holes test's units: the shader takes 255 and above for "no holes",
/// so a cut of 1 (only fully opaque texels) is drawn as 254.5, which keeps 255 and drops
/// 254 the same way (#3017).
fn holes_cut(cut: f32) -> f32 {
    (cut * 255.0).clamp(0.0, 254.5)
}

/// An overlay material cut where `texture`'s alpha, at the mesh's own texture
/// coordinates, is below `cut` (0-1): the holes test turned round (nif_material.rs
/// "Holes", `holes.z`), with no edge band.
fn cut_by(mut m: NifMaterial, texture: Handle<Image>, cut: f32) -> NifMaterial {
    m.extension.holes = Vec4::new(holes_cut(cut), 0.0, 1.0, 0.0);
    m.extension.holes_texture = Some(texture);
    m
}

/// Apply only differing values in one mutable access [documented: Bevy 0.20.1
/// `AssetMut::deref_mut` / `AssetMutChangeNotifier::drop` marks the asset modified].
fn animate_material(materials: &mut Assets<NifMaterial>, handle: &Handle<NifMaterial>, holes: Option<f32>, alpha: Option<f32>, uv: Option<bevy::math::Affine2>) {
    let Some(m) = materials.get(handle) else { return };
    let holes = holes.filter(|v| m.extension.holes.x != *v);
    let alpha = alpha.filter(|v| m.base.base_color.alpha() != *v);
    let uv = uv.filter(|v| m.base.uv_transform != *v);
    if holes.is_none() && alpha.is_none() && uv.is_none() {
        return;
    }
    let Some(mut m) = materials.get_mut(handle) else { return };
    if let Some(value) = holes {
        m.extension.holes.x = value;
    }
    if let Some(alpha) = alpha {
        m.base.base_color.set_alpha(alpha);
    }
    if let Some(uv) = uv {
        m.base.uv_transform = uv;
    }
}

/// The path of a texture as the records store it, under `textures\`.
fn texture_path(t: &str) -> String {
    let path = t.replace('/', "\\").to_ascii_lowercase();
    if path.starts_with("textures\\") {
        path
    } else {
        format!("textures\\{path}")
    }
}

/// Give a material the shader's holes at `value` (nif_material.rs, "Holes").
fn set_holes(m: &mut NifMaterial, h: &Holes, texture: Handle<Image>, value: f32) {
    m.extension.holes = Vec4::new(value, h.edge_width, 0.0, 0.0);
    m.extension.holes_edge = Vec4::from_array(linear(h.edge_color).to_f32_array());
    m.extension.holes_texture = Some(texture);
}

/// Up to `n` of `items`, spread evenly over them.
fn spread<T: Copy>(items: &[T], n: usize) -> Vec<T> {
    if items.len() <= n {
        return items.to_vec();
    }
    (0..n).map(|i| items[i * items.len() / n]).collect()
}

/// A 64-bit mix (splitmix64's finaliser), for the add-on models' picks.
fn mix(mut x: u64) -> u64 {
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

/// Scale and fade the add-on models.
fn addon_look(models: &[(Entity, Option<Entity>)], scale: f32, alpha: f32, transforms: &mut Query<&mut Transform>, fades: &mut Query<&mut RigMaterials>) {
    for &(holder, root) in models {
        if let Ok(mut tr) = transforms.get_mut(holder) {
            if tr.scale.x != scale {
                tr.scale = Vec3::splat(scale);
            }
        }
        if let Some(mut m) = root.and_then(|r| fades.get_mut(r).ok()) {
            if m.fade != alpha {
                m.fade = alpha;
            }
        }
    }
}

/// Shrink and fade the add-on models of ended plays; take them away when done.
fn addons_leaving(mut commands: Commands, time: Res<Time>, mut st: ResMut<Shaders>, mut transforms: Query<&mut Transform>, mut fades: Query<&mut RigMaterials>) {
    let now = time.elapsed_secs();
    st.leaving.retain(|l| {
        let since_end = now - l.ended;
        if since_end >= l.addons.linger() {
            for (holder, _) in &l.models {
                commands.entity(*holder).try_despawn();
            }
            return false;
        }
        let (scale, alpha) = l.addons.look(l.ended - l.since, Some(since_end));
        addon_look(&l.models, scale, alpha, &mut transforms, &mut fades);
        true
    });
}

/// Draw actors with an alpha below 1 see-through; give the others their materials back.
fn actor_alpha(script: Option<Res<ScriptState>>, mut drawing: AlphaDrawing) {
    let Some(rt) = script.as_deref().and_then(|s| s.rt.as_ref()) else { return };
    for (r, (a, n)) in drawing.sync(&rt.game.script_looks.alpha) {
        info!("actor alpha: {} {a:.2} ({n} meshes)", rt.game.data.name(FormId(r)));
    }
}

/// [design, #2557, #3105] Production actor-alpha application, also used by synthetic ECS
/// tests. Delegates the single see-through copy per mesh to `nvf_play::fade::Fading` so
/// `SetActorAlpha` and presence fades compose multiplicatively and always restore the
/// mesh's own persistent material when the last effect ends.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct AlphaDrawing<'w, 's> {
    commands: Commands<'w, 's>,
    st: ResMut<'w, Shaders>,
    #[allow(clippy::type_complexity)]
    drawn: Query<'w, 's, (Entity, &'static RefVisual, Option<&'static MeshMaterial3d<NifMaterial>>, Option<&'static crate::mesh_terrain_blend::BlendShape>, Option<&'static nvf_play::fade::Fading>), Without<Overlay>>,
    /// The meshes of `drawn` still wearing a see-through copy.
    #[allow(clippy::type_complexity)]
    still_faded: Query<'w, 's, &'static nvf_play::fade::Fading, (With<RefVisual>, Without<Overlay>)>,
}

impl AlphaDrawing<'_, '_> {
    pub(crate) fn sync(&mut self, alpha: &std::collections::BTreeMap<FormId, f32>) -> HashMap<u32, (f32, u32)> {
        let Self { commands, st, drawn, still_faded } = self;
        let mut changed: HashMap<u32, (f32, u32)> = HashMap::new();
        // No actor see-through and no mesh still carrying an actor-alpha factor: the walk
        // over every drawn mesh would change nothing.
        if alpha.is_empty() && still_faded.iter().all(|f| f.actor_alpha().is_none()) {
            return changed;
        }
        let st = &mut **st;
        for (e, rv, mat, blend, fading) in &*drawn {
            if nvf_assets::nif_surface_blend::source_material(mat, blend).is_none() {
                continue;
            }
            let want = alpha.get(&FormId(rv.0)).copied();
            let had = fading.and_then(nvf_play::fade::Fading::actor_alpha);
            if want == had {
                continue;
            }
            nvf_play::fade::queue_actor_alpha(commands, e, want);
            changed.entry(rv.0).or_insert((want.unwrap_or(1.0), 0)).1 += 1;
        }
        st.alpha_calls += changed.len() as u32;
        changed
    }
}

impl Shaders {
    /// See-through copies are owned per mesh by `nvf_play::fade::Fading` and removed on
    /// release (#3105); no retained cache remains here.
    pub(crate) fn share(&self, _materials: &Assets<NifMaterial>) -> nvf_assets::release::CacheShare {
        nvf_assets::release::CacheShare::default()
    }
}

fn report(mut exit: MessageReader<AppExit>, st: Res<Shaders>) {
    if exit.read().next().is_none() {
        return;
    }
    info!(
        "effect shaders summary: {} played, {} ended, {} overlays drawn, {} actor alpha changes, {} meshes given holes, {} add-on models",
        st.played, st.ended, st.overlays, st.alpha_calls, st.holed, st.addons
    );
}

#[cfg(test)]
mod tests {
    /// #2981 / #3105: a swap retargets the unified fade owner's stored original, rebuilding
    /// its see-through copy in place and giving back the swapped material on release.
    #[test]
    fn a_swap_retargets_the_alpha_fades_original() {
        let mut world = World::new();
        world.init_resource::<Assets<NifMaterial>>();
        let (old, new) = {
            let mut materials = world.resource_mut::<Assets<NifMaterial>>();
            (materials.add(nvf_assets::nif_material(Default::default())), materials.add(nvf_assets::nif_material(Default::default())))
        };
        let e = world.spawn(MeshMaterial3d(old.clone())).id();
        nvf_play::fade::apply_factor(&mut world, e, nvf_play::fade::FadeChannel::ActorAlpha, Some(0.5));
        assert_eq!(world.get::<nvf_play::fade::Fading>(e).unwrap().original(), &old);
        nvf_play::fade::apply_retarget(&mut world, e, new.clone());
        assert_eq!(world.get::<nvf_play::fade::Fading>(e).unwrap().original(), &new);
        nvf_play::fade::apply_factor(&mut world, e, nvf_play::fade::FadeChannel::ActorAlpha, None);
        assert!(world.get::<nvf_play::fade::Fading>(e).is_none());
        assert_eq!(world.get::<MeshMaterial3d<NifMaterial>>(e).unwrap().0, new);
    }
    use super::*;
    use crate::sky::{BODY_LAYER, SHADOW_LAYER};
    use bevy::asset::{AssetApp, AssetPlugin};
    use bevy::ecs::message::Messages;
    use bevy::math::Affine2;
    use nvf_assets::nif_material::{Falloff, NO_HOLES};

    /// #2701: a scripted effect never covers the full-detail ring's draw-only copy of a
    /// reference, only the loaded reference's meshes.
    #[test]
    fn effects_cover_no_draw_only_meshes() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        world.spawn((RefVisual(5), Mesh3d(Handle::default())));
        world.spawn((RefVisual(5), Mesh3d(Handle::default()), nvf_play::DrawOnly));
        world.spawn((RefVisual(5), Mesh3d(Handle::default()), Overlay));
        let n = world.run_system_once(|q: Coverable| q.iter().count()).unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn overlays_take_the_mesh_layer_or_the_default_world_layer() {
        assert_eq!(overlay_layers(None), RenderLayers::default());
        for layers in [RenderLayers::layer(BODY_LAYER), RenderLayers::layer(SHADOW_LAYER), RenderLayers::from_layers(&[0, 7]), RenderLayers::none()] {
            assert_eq!(overlay_layers(Some(&layers)), layers);
        }
        assert!(player_mesh_seen(None));
        assert!(player_mesh_seen(Some(&RenderLayers::layer(BODY_LAYER))));
        assert!(!player_mesh_seen(Some(&RenderLayers::layer(SHADOW_LAYER))));
        assert!(!player_mesh_seen(Some(&RenderLayers::none())));
    }

    #[test]
    fn player_overlays_follow_layers_in_both_directions() {
        let mut app = App::new();
        app.init_resource::<Shaders>().add_systems(Update, sync_player_effects);
        let parent = app.world_mut().spawn(RenderLayers::layer(SHADOW_LAYER)).id();
        let overlay = app.world_mut().spawn((Overlay, PlayerOverlay(parent), RenderLayers::default(), bevy::light::NotShadowCaster)).id();
        let other = app.world_mut().spawn((Overlay, RenderLayers::default())).id();
        for layer in [SHADOW_LAYER, BODY_LAYER, SHADOW_LAYER, BODY_LAYER] {
            app.world_mut().entity_mut(parent).insert(RenderLayers::layer(layer));
            app.update();
            assert_eq!(app.world().get::<RenderLayers>(overlay), Some(&RenderLayers::layer(layer)));
            assert_eq!(app.world().get::<RenderLayers>(other), Some(&RenderLayers::default()));
            assert!(app.world().get::<bevy::light::NotShadowCaster>(overlay).is_some());
        }
    }

    #[test]
    fn player_addon_emitters_are_marked_and_hidden_at_spawn() {
        let mut world = World::new();
        let holder = world.spawn(Visibility::Inherited).id();
        let emitter = world.spawn((crate::particles::AddonEmitter::new(42), ChildOf(holder))).id();
        let other = world.spawn(crate::particles::AddonEmitter::new(43)).id();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let mut commands = Commands::new(&mut queue, &world);
        player_addon(&mut commands, holder, crate::player_body::EffectPart::Head, false);
        queue.apply(&mut world);
        assert_eq!(world.get::<Visibility>(holder), Some(&Visibility::Hidden));
        assert_eq!(world.get::<PlayerEffectEmitter>(emitter).map(|p| p.0), Some(crate::player_body::EffectPart::Head));
        assert!(world.get::<PlayerEffectEmitter>(other).is_none());
    }

    #[derive(Resource, Default)]
    struct Alphas(std::collections::BTreeMap<FormId, f32>);

    #[derive(Resource, Default)]
    struct Writes(usize);

    #[test]
    fn actor_alpha_restores_the_frame_after_it_clears_and_writes_nothing_when_idle() {
        let mut app = App::new();
        app.init_resource::<Assets<NifMaterial>>()
            .init_resource::<Shaders>()
            .init_resource::<Alphas>()
            .init_resource::<Writes>()
            .add_systems(Update, (
                |alpha: Res<Alphas>, mut drawing: AlphaDrawing| {
                    drawing.sync(&alpha.0);
                },
                |changed: Query<Entity, Changed<MeshMaterial3d<NifMaterial>>>, mut writes: ResMut<Writes>| writes.0 = changed.iter().count(),
            ).chain());
        let original = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(nif_material(StandardMaterial::default()));
        let reference = 0xFF00_0001;
        let mesh = app.world_mut().spawn((RefVisual(reference), MeshMaterial3d(original.clone()))).id();
        let worn = |app: &App| app.world().get::<MeshMaterial3d<NifMaterial>>(mesh).unwrap().0.clone();
        app.update();
        // Nothing see-through: no write, no copy.
        for _ in 0..2 {
            app.update();
            assert_eq!(app.world().resource::<Writes>().0, 0);
            assert_eq!(worn(&app), original);
            assert!(app.world().get::<nvf_play::fade::Fading>(mesh).is_none());
        }
        app.world_mut().resource_mut::<Alphas>().0.insert(FormId(reference), 0.5);
        app.update();
        assert_eq!(app.world().resource::<Writes>().0, 1);
        assert_ne!(worn(&app), original);
        assert!(app.world().get::<nvf_play::fade::Fading>(mesh).is_some());
        // The alpha goes: the next frame gives the material back, then nothing more.
        app.world_mut().resource_mut::<Alphas>().0.clear();
        app.update();
        assert_eq!(app.world().resource::<Writes>().0, 1);
        assert_eq!(worn(&app), original);
        assert!(app.world().get::<nvf_play::fade::Fading>(mesh).is_none());
        for _ in 0..2 {
            app.update();
            assert_eq!(app.world().resource::<Writes>().0, 0);
            assert_eq!(worn(&app), original);
        }
    }

    fn material_app() -> (App, Handle<NifMaterial>) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default())).init_asset::<NifMaterial>();
        let handle = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(nif_material(StandardMaterial::default()));
        app.update();
        app.world_mut().resource_mut::<Messages<AssetEvent<NifMaterial>>>().clear();
        (app, handle)
    }

    fn material_events(app: &mut App) -> Vec<AssetEvent<NifMaterial>> {
        app.update();
        app.world_mut().resource_mut::<Messages<AssetEvent<NifMaterial>>>().drain().collect()
    }

    #[test]
    fn equal_material_values_emit_no_modification() {
        let (mut app, handle) = material_app();
        for _ in 0..3 {
            animate_material(&mut app.world_mut().resource_mut::<Assets<NifMaterial>>(), &handle, Some(NO_HOLES.x), Some(1.0), Some(Affine2::IDENTITY));
            assert!(material_events(&mut app).is_empty());
        }
    }

    #[test]
    fn holes_alpha_and_uv_change_in_one_modification() {
        let (mut app, handle) = material_app();
        let uv = Affine2::from_translation(Vec2::new(-0.25, 0.5));
        animate_material(&mut app.world_mut().resource_mut::<Assets<NifMaterial>>(), &handle, Some(128.0), Some(0.5), Some(uv));
        assert_eq!(material_events(&mut app), vec![AssetEvent::Modified { id: handle.id() }]);
        let materials = app.world().resource::<Assets<NifMaterial>>();
        let m = materials.get(&handle).unwrap();
        assert_eq!(m.extension.holes, Vec4::new(128.0, 0.0, 0.0, 0.0));
        assert_eq!(m.base.base_color.alpha(), 0.5);
        assert_eq!(m.base.uv_transform, uv);

        // A persistent plateau with no further scroll produces no asset event.
        animate_material(&mut app.world_mut().resource_mut::<Assets<NifMaterial>>(), &handle, Some(128.0), Some(0.5), Some(uv));
        assert!(material_events(&mut app).is_empty());
    }

    #[test]
    fn material_channels_animate_independently_without_an_epsilon() {
        let (mut app, handle) = material_app();
        let alpha = f32::from_bits(1.0f32.to_bits() - 1);
        let uv = Affine2::from_translation(Vec2::new(0.001, -0.001));
        for (holes, alpha, uv) in [(Some(255.0), None, None), (None, Some(alpha), None), (None, None, Some(uv))] {
            animate_material(&mut app.world_mut().resource_mut::<Assets<NifMaterial>>(), &handle, holes, alpha, uv);
            assert_eq!(material_events(&mut app), vec![AssetEvent::Modified { id: handle.id() }]);
        }
        let materials = app.world().resource::<Assets<NifMaterial>>();
        let m = materials.get(&handle).unwrap();
        assert_eq!(m.extension.holes.x, 255.0);
        assert_eq!(m.base.base_color.alpha(), alpha);
        assert_eq!(m.base.uv_transform, uv);
    }

    #[test]
    fn absent_material_or_values_emit_no_modification() {
        let (mut app, handle) = material_app();
        animate_material(&mut app.world_mut().resource_mut::<Assets<NifMaterial>>(), &handle, None, None, None);
        assert!(material_events(&mut app).is_empty());
        app.world_mut().resource_mut::<Assets<NifMaterial>>().remove(&handle);
        material_events(&mut app);
        animate_material(&mut app.world_mut().resource_mut::<Assets<NifMaterial>>(), &handle, Some(0.0), Some(0.0), Some(Affine2::IDENTITY));
        assert!(material_events(&mut app).is_empty());
    }

    #[test]
    fn add_on_places_spread_over_the_bones() {
        assert_eq!(spread(&[1, 2, 3], 8), vec![1, 2, 3]);
        assert_eq!(spread(&(0..16).collect::<Vec<_>>(), 4), vec![0, 4, 8, 12]);
        assert_ne!(mix(1), mix(2));
        assert_eq!(NO_HOLES.x, -1.0);
    }

    #[test]
    fn edge_is_half_strong_where_the_fall_off_says() {
        for f in [0.5f32, 0.95, 1.0, 2.5] {
            let p = Falloff { params: edge_falloff(f) };
            let half = 1.0 - 0.5f32.powf(1.0 / f);
            assert!((p.opacity(half) - 0.5).abs() < 1e-4, "fall off {f}");
            assert_eq!(p.opacity(1.0), 0.0);
        }
    }

    #[test]
    fn alpha_tested_meshes_get_overlays_cut_by_their_own_texture() {
        let tex = Handle::<Image>::default();
        let with = |mode: AlphaMode, cut: f32| {
            let mut m = nif_material(StandardMaterial { alpha_mode: mode, base_color_texture: Some(tex.clone()), ..default() });
            m.extension.alpha_cut = cut;
            m
        };
        assert_eq!(cut_out_overlay(&with(AlphaMode::Opaque, 0.0), false), Cover::Plain);
        // A Mister Gutsy's body: NiAlphaProperty test at 120 (#1717).
        assert_eq!(cut_out_overlay(&with(AlphaMode::Mask(120.0 / 255.0), 0.0), false), Cover::CutBy(tex.clone(), 120.0 / 255.0));
        // One holes texture per material: the shader's own holes win.
        assert_eq!(cut_out_overlay(&with(AlphaMode::Mask(0.5), 0.0), true), Cover::None);
        assert_eq!(cut_out_overlay(&with(AlphaMode::Blend, 0.0), false), Cover::None);
        assert_eq!(cut_out_overlay(&with(AlphaMode::Blend, 0.3), false), Cover::None);
        // A Mister Gutsy's jet haze.
        let mut haze = with(AlphaMode::Opaque, 0.0);
        haze.extension.refract = true;
        assert_eq!(cut_out_overlay(&haze, false), Cover::None);
        let m = cut_by(with(AlphaMode::Add, 0.0), tex.clone(), 0.5);
        assert_eq!(m.extension.holes, Vec4::new(127.5, 0.0, 1.0, 0.0));
        // #3017: a cut of 1 stays a cut (the shader reads 255 as no holes): 255 kept, 254 dropped.
        let full = cut_by(with(AlphaMode::Add, 0.0), tex.clone(), 1.0).extension.holes.x;
        assert!(full < 255.0 && full > 254.0, "{full}");
        assert_eq!(holes_cut(0.0), 0.0);
        assert_eq!(holes_cut(2.0), 254.5);
        assert_eq!(m.extension.holes_texture, Some(tex));
    }

    /// A synthetic hologram membrane (#3017, the shape of Dead Money's three hologram
    /// shaders without their textures): additive fill (Source Alpha / Add / Source Alpha),
    /// "Edge Effect Inverse", fill and edge showing at full alpha from the start.
    fn hologram_membrane(edge_color: [u8; 3]) -> EffectShader {
        let full = fnv_world::effect_shader::AlphaCycle { full: 1.0, persistent: 1.0, ..Default::default() };
        EffectShader {
            form_id: 1, edid: "SyntheticHologram".into(), flags: fnv_world::effect_shader::NO_PARTICLES | EDGE_INVERSE,
            fill_texture: None, particle_texture: None,
            source_blend: blend::SOURCE_ALPHA, blend_op: 1, z_test: 3,
            dest_blend: blend::SOURCE_ALPHA, fill_color: [119, 218, 255],
            fill: full, fill_speed: [-0.5, 0.0], edge_color,
            edge: full, edge_fall_off: 0.95,
            particles: None, holes: None, addons: None,
        }
    }

    /// #3017: "Edge Effect Inverse" over an additive fill darkens the mesh by the edge colour
    /// (multiplied by 1 - colour) instead of adding a pale rim; without the flag, or over an
    /// alpha-blended fill, the edge is added as before.
    #[test]
    fn an_inverse_edge_over_an_additive_fill_darkens() {
        let red = hologram_membrane([168, 130, 131]);
        assert!(edge_darkens(&red));
        let m = edge_material(&red);
        assert_eq!(m.base.alpha_mode, AlphaMode::Multiply);
        let c = linear([168, 130, 131]);
        let got = m.base.base_color.to_linear();
        for (g, want) in [(got.red, 1.0 - c.red), (got.green, 1.0 - c.green), (got.blue, 1.0 - c.blue)] {
            assert!((g - want).abs() < 1e-5, "{g} != {want}");
        }
        assert_eq!(got.alpha, 0.0, "the cycle sets the alpha");
        assert_eq!(m.extension.falloff, edge_falloff(0.95), "it still shows towards the outline");
        // Drawn after the fill on the same mesh, and only reordered: the depth test's bias
        // (the integer part) stays 0.
        let fill = overlay_material(&red, red.fill_color, None, None);
        assert!(m.base.depth_bias > fill.base.depth_bias, "the darkening edge sorts after the fill");
        assert_eq!(m.base.depth_bias as i32, 0);
        // Destination Source Alpha (the holograms) and One (Dead Money's turret shield) are
        // both additive fills.
        assert_eq!(alpha_mode(&red), AlphaMode::Add);
        let mut shield = red.clone();
        shield.dest_blend = blend::ONE;
        assert!(edge_darkens(&shield));
        // No inverse flag: the edge colour is added.
        let mut plain = red.clone();
        plain.flags &= !EDGE_INVERSE;
        assert!(!edge_darkens(&plain));
        assert_eq!(edge_material(&plain).base.alpha_mode, AlphaMode::Add);
        assert_eq!(edge_material(&plain).base.depth_bias, 0.0, "an added edge is not reordered");
        assert_eq!(edge_material(&plain).base.base_color.to_linear().red, c.red);
        // An alpha-blended fill (Source Inverted Alpha) keeps its edge as it was.
        let mut blended = red.clone();
        blended.dest_blend = blend::SOURCE_INVERTED_ALPHA;
        assert!(!edge_darkens(&blended));
        assert_eq!(edge_material(&blended).base.alpha_mode, AlphaMode::Blend);
        // The fill is never touched by the flag.
        assert_eq!(overlay_material(&red, red.fill_color, None, None).base.alpha_mode, AlphaMode::Add);
    }

    #[test]
    fn overlay_alpha_is_the_cycle_times_the_drawn_alpha() {
        assert_eq!(overlay_alpha(0.9, 1.0), 0.9);
        assert!((overlay_alpha(0.9, 0.7) - 0.63).abs() < 1e-6);
        assert_eq!(overlay_alpha(0.9, 0.0), 0.0);
        assert_eq!(overlay_alpha(0.5, 2.0), 0.5);
    }

    /// #3017: a Dead Money hologram flickers its actor alpha (SetActorAlpha 0, 0.7, back to 1)
    /// while its membrane plays. The see-through copy must not count as a blended mesh: the
    /// overlays stay on the mesh (the same entities) through every alpha and after it.
    #[test]
    fn a_see_through_actor_keeps_its_membrane_overlays() {
        let mut app = App::new();
        app.init_resource::<Assets<NifMaterial>>()
            .init_resource::<Shaders>()
            .init_resource::<Alphas>()
            .add_systems(Update, (|alpha: Res<Alphas>, mut drawing: AlphaDrawing| {
                drawing.sync(&alpha.0);
            }, cover).chain());
        let original = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(nif_material(StandardMaterial::default()));
        let fill = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(nif_material(StandardMaterial::default()));
        let reference = 0xFF00_0003;
        let mesh = app.world_mut().spawn((RefVisual(reference), Mesh3d(Handle::default()), MeshMaterial3d(original.clone()))).id();
        let source = Source::Script(1);
        app.world_mut().resource_mut::<Shaders>().active.insert(source, Active::membrane_for_test(reference, fill));
        app.update();
        let overlays = app.world().resource::<Shaders>().active[&source].covered[&mesh].clone();
        assert_eq!(overlays.len(), 1, "the fill covers the opaque mesh");
        for alpha in [0.0, 0.7, 0.0, 0.7] {
            app.world_mut().resource_mut::<Alphas>().0.insert(FormId(reference), alpha);
            app.update();
            assert!(app.world().get::<nvf_play::fade::Fading>(mesh).is_some(), "the mesh is drawn see-through at {alpha}");
            assert_eq!(app.world().resource::<Shaders>().active[&source].covered[&mesh], overlays, "alpha {alpha}");
            assert!(app.world().get_entity(overlays[0]).is_ok());
        }
        app.world_mut().resource_mut::<Alphas>().0.clear();
        app.update();
        app.update();
        assert!(app.world().get::<nvf_play::fade::Fading>(mesh).is_none());
        assert_eq!(app.world().get::<MeshMaterial3d<NifMaterial>>(mesh).unwrap().0, original);
        assert_eq!(app.world().resource::<Shaders>().active[&source].covered[&mesh], overlays);
        // A blended mesh of its own (hair) still gets none.
        let hair = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(nif_material(StandardMaterial { alpha_mode: AlphaMode::Blend, ..default() }));
        let strands = app.world_mut().spawn((RefVisual(reference), Mesh3d(Handle::default()), MeshMaterial3d(hair))).id();
        app.world_mut().resource_mut::<Alphas>().0.insert(FormId(reference), 0.7);
        app.update();
        assert!(app.world().resource::<Shaders>().active[&source].covered[&strands].is_empty());
        assert_eq!(app.world().resource::<Shaders>().active[&source].covered[&mesh], overlays);
    }

    /// The play and coverage systems over a synthetic game (no records but those given), with
    /// the actor alpha drawn from the game's script alpha as the engine draws it.
    fn playback_app(game: fnv_game::Game<'static>, records: &[(u32, EffectShader)]) -> (App, crate::outfits::tests::TestFiles) {
        let (mut app, files) = crate::outfits::tests::asset_fixture();
        app.insert_resource(crate::outfits::tests::empty_world_data());
        let mut shaders = Shaders::default();
        for (id, r) in records {
            shaders.records.insert(*id, Some(Box::new(r.clone())));
        }
        app.insert_resource(shaders)
            .init_resource::<Time>()
            .insert_resource(PlaybackTestGame(game))
            .add_systems(Update, (|g: Res<PlaybackTestGame>, mut drawing: AlphaDrawing| {
                drawing.sync(&g.0.script_looks.alpha);
            }, play, cover).chain());
        (app, files)
    }

    fn test_game() -> fnv_game::Game<'static> {
        let forms = Box::leak(Box::default());
        let refs = Box::leak(Box::default());
        crate::outfits::tests::game(fnv_sim::World::new(forms, refs, Default::default()))
    }

    /// As `PlayMagicShaderVisuals` does: the same shader on the same reference starts over.
    fn script_play(app: &mut App, on: u32, shader: u32, serial: u64) {
        let looks = &mut app.world_mut().resource_mut::<PlaybackTestGame>().0.script_looks;
        looks.shaders.retain(|p| !(p.on == FormId(on) && p.shader == FormId(shader)));
        looks.shaders.push(fnv_game::functions::misc::ShaderPlay { on: FormId(on), shader: FormId(shader), duration: None, serial });
    }

    fn script_shaders(app: &App) -> Vec<(u32, String)> {
        let mut v: Vec<(u32, String)> = app.world().resource::<Shaders>().active.iter().filter(|(k, _)| matches!(k, Source::Script(_))).map(|(_, a)| (a.on, a.shader.edid.clone())).collect();
        v.sort();
        v
    }

    /// #3017: the renderer draws every script play (which hologram colour plays is settled
    /// at the script call), a hologram colour and a fire on one reference together, and logs
    /// each reference's set when it changes.
    #[test]
    fn script_plays_are_all_drawn_and_their_set_kept() {
        let (neutral, aggressive, fire) = (0x0100_1118, 0x0100_307C, 0x0005_F9B6);
        let mut n = hologram_membrane([164, 206, 240]);
        n.edid = "Neutral".into();
        let mut a = hologram_membrane([168, 130, 131]);
        a.edid = "Aggressive".into();
        let mut f = hologram_membrane([255, 128, 0]);
        f.edid = "Fire".into();
        f.flags = 0;
        f.dest_blend = blend::ONE;
        let (mut app, _files) = playback_app(test_game(), &[(neutral, n), (aggressive, a), (fire, f)]);
        let (holo, other) = (0xFF00_0005, 0xFF00_0006);
        script_play(&mut app, holo, neutral, 1);
        script_play(&mut app, other, neutral, 2);
        script_play(&mut app, holo, fire, 3);
        app.update();
        assert_eq!(script_shaders(&app), vec![(holo, "Fire".to_string()), (holo, "Neutral".into()), (other, "Neutral".into())]);
        assert_eq!(app.world().resource::<Shaders>().script_sets[&holo], vec!["Fire".to_string(), "Neutral".into()]);
        // The script side's colour rule: Aggressive ended Neutral there.
        app.world_mut().resource_mut::<PlaybackTestGame>().0.script_looks.shaders.retain(|p| p.serial != 1);
        script_play(&mut app, holo, aggressive, 5);
        app.update();
        assert_eq!(script_shaders(&app), vec![(holo, "Aggressive".to_string()), (holo, "Fire".into()), (other, "Neutral".into())]);
        assert_eq!(app.world().resource::<Shaders>().script_sets[&holo], vec!["Aggressive".to_string(), "Fire".into()]);
        // All gone: the set is let go.
        app.world_mut().resource_mut::<PlaybackTestGame>().0.script_looks.shaders.clear();
        app.update();
        assert!(app.world().resource::<Shaders>().script_sets.is_empty());
    }

    /// #3017: a persistent membrane started on an actor already unseen (alpha 0) starts
    /// unseen: no frame at full strength before its coverage.
    #[test]
    fn a_membrane_started_on_an_unseen_actor_starts_unseen() {
        let shader = 0x0100_307D;
        let (mut app, _files) = playback_app(test_game(), &[(shader, hologram_membrane([213, 210, 191]))]);
        let on = 0xFF00_0007;
        let own = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(nif_material(StandardMaterial::default()));
        let mesh = app.world_mut().spawn((RefVisual(on), Mesh3d(Handle::default()), MeshMaterial3d(own))).id();
        app.world_mut().resource_mut::<PlaybackTestGame>().0.script_looks.alpha.insert(FormId(on), 0.0);
        app.update();
        assert!(app.world().get::<nvf_play::fade::Fading>(mesh).is_some());
        script_play(&mut app, on, shader, 1);
        app.update();
        let shaders = app.world().resource::<Shaders>();
        let active = &shaders.active[&Source::Script(1)];
        assert_eq!(active.faded_by, 0.0);
        let materials = app.world().resource::<Assets<NifMaterial>>();
        for h in [active.fill.as_ref().unwrap(), active.edge.as_ref().unwrap()] {
            assert_eq!(materials.get(h).unwrap().base.base_color.alpha(), 0.0, "no first frame at full strength");
        }
        assert_eq!(active.covered[&mesh].len(), 2, "covered all the same");
    }

    /// #3017: the overlays' alpha is the record's cycle times the alpha the actor is drawn
    /// with, and comes back with it; a reference whose meshes get no see-through copy (the
    /// player's third-person body) keeps 1 whatever its script alpha.
    #[test]
    fn membrane_overlays_fade_with_the_alpha_their_actor_is_drawn_with() {
        let (mut app, _files) = playback_app(test_game(), &[]);
        let on = 0xFF00_0004;
        let fill = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(nif_material(StandardMaterial::default()));
        let edge = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(nif_material(StandardMaterial::default()));
        let own = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(nif_material(StandardMaterial::default()));
        app.world_mut().spawn((RefVisual(on), Mesh3d(Handle::default()), MeshMaterial3d(own)));
        let mut active = Active::for_test(on, hologram_membrane([164, 206, 240]), 0.0, &[]);
        active.fill = Some(fill.clone());
        active.edge = Some(edge.clone());
        let player_fill = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(nif_material(StandardMaterial::default()));
        let mut on_player = Active::for_test(FormId::PLAYER_REF.0, hologram_membrane([164, 206, 240]), 0.0, &[]);
        on_player.fill = Some(player_fill.clone());
        {
            let looks = &mut app.world_mut().resource_mut::<PlaybackTestGame>().0.script_looks;
            looks.shaders.push(fnv_game::functions::misc::ShaderPlay { on: FormId(on), shader: FormId(1), duration: None, serial: 1 });
            looks.shaders.push(fnv_game::functions::misc::ShaderPlay { on: FormId::PLAYER_REF, shader: FormId(1), duration: None, serial: 2 });
            looks.alpha.insert(FormId::PLAYER_REF, 0.5);
        }
        app.world_mut().resource_mut::<Shaders>().active.insert(Source::Script(1), active);
        app.world_mut().resource_mut::<Shaders>().active.insert(Source::Script(2), on_player);
        let alpha_of = |app: &App, h: &Handle<NifMaterial>| app.world().resource::<Assets<NifMaterial>>().get(h).unwrap().base.base_color.alpha();
        let alphas = |app: &App| [alpha_of(app, &fill), alpha_of(app, &edge)];
        app.update();
        app.update();
        assert_eq!(alphas(&app), [1.0, 1.0]);
        for a in [0.7, 0.0] {
            app.world_mut().resource_mut::<PlaybackTestGame>().0.script_looks.alpha.insert(FormId(on), a);
            app.update();
            for got in alphas(&app) {
                assert!((got - a).abs() < 1e-6, "actor alpha {a}: overlay alpha {got}");
            }
            let shaders = app.world().resource::<Shaders>();
            assert_eq!(shaders.active[&Source::Script(1)].faded_by, a);
            assert_eq!(shaders.active[&Source::Script(1)].logged_fade, a);
            assert!(shaders.active[&Source::Script(1)].covered.values().all(|o| o.len() == 2), "both overlays live at {a}");
        }
        app.world_mut().resource_mut::<PlaybackTestGame>().0.script_looks.alpha.remove(&FormId(on));
        app.update();
        app.update();
        assert_eq!(alphas(&app), [1.0, 1.0]);
        assert_eq!(alpha_of(&app, &player_fill), 1.0, "no see-through copy on the player's body: no fade");
        assert!(app.world().resource::<Shaders>().active.contains_key(&Source::Script(1)), "the play goes on");
    }

    /// #3017: a hologram colour covers its body's blended and cut-out meshes (cut by their
    /// own texture), which the general rule leaves bare; refracting meshes stay bare.
    #[test]
    fn a_hologram_colour_covers_blended_and_cut_out_meshes() {
        let tex = Handle::<Image>::default();
        let with = |mode: AlphaMode, texture: bool, cut: f32| {
            let mut m = nif_material(StandardMaterial { alpha_mode: mode, base_color_texture: texture.then(|| tex.clone()), ..default() });
            m.extension.alpha_cut = cut;
            m
        };
        let blended = with(AlphaMode::Blend, true, 0.0);
        assert_eq!(cut_out_overlay(&blended, false), Cover::None, "hair and lashes keep the general rule");
        assert_eq!(hologram_cover(&blended, false), Cover::CutBy(tex.clone(), HOLOGRAM_BODY_CUT));
        assert_eq!(hologram_cover(&with(AlphaMode::Blend, true, 0.3), false), Cover::CutBy(tex.clone(), 0.3));
        assert_eq!(hologram_cover(&with(AlphaMode::Blend, false, 0.0), false), Cover::Plain);
        assert_eq!(hologram_cover(&with(AlphaMode::Opaque, true, 0.0), false), Cover::Plain);
        assert_eq!(hologram_cover(&with(AlphaMode::Mask(0.5), true, 0.0), false), Cover::CutBy(tex.clone(), 0.5));
        let mut haze = with(AlphaMode::Blend, true, 0.0);
        haze.extension.refract = true;
        assert_eq!(hologram_cover(&haze, false), Cover::None);
        assert_eq!(hologram_cover(&blended, true), Cover::None, "a shader with holes of its own");
    }

    /// #3017: the hologram's see-through body (a blended mesh) wears the membrane, cut by its
    /// texture; the same mesh under any other shader stays bare.
    #[test]
    fn a_hologram_body_wears_the_membrane() {
        for hologram in [true, false] {
            let mut app = App::new();
            app.init_resource::<Assets<NifMaterial>>().init_resource::<Shaders>().add_systems(Update, cover);
            let body = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(nif_material(StandardMaterial {
                alpha_mode: AlphaMode::Blend,
                base_color_texture: Some(Handle::default()),
                ..default()
            }));
            let fill = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(nif_material(StandardMaterial::default()));
            let reference = 0xFF00_0008;
            let mesh = app.world_mut().spawn((RefVisual(reference), Mesh3d(Handle::default()), MeshMaterial3d(body))).id();
            let mut active = Active::membrane_for_test(reference, fill);
            active.hologram = hologram;
            app.world_mut().resource_mut::<Shaders>().active.insert(Source::Script(1), active);
            app.update();
            let shaders = app.world().resource::<Shaders>();
            let a = &shaders.active[&Source::Script(1)];
            assert_eq!(a.covered[&mesh].len(), usize::from(hologram), "hologram {hologram}");
            assert_eq!(a.cut.len(), usize::from(hologram), "the overlay is a copy cut by the body's texture");
        }
    }

    /// #3017 / #3105: an actor alpha started while a presence fade draws the mesh shares the
    /// single `Fading` owner whose original is always the mesh's own opaque material, so
    /// membrane coverage stays on throughout the fade, after the fade ends, and after alpha
    /// release, and the release always restores `own`.
    #[test]
    fn an_alpha_copy_made_during_a_fade_takes_the_material_the_fade_gives_back() {
        let mut app = App::new();
        app.init_resource::<Assets<NifMaterial>>()
            .init_resource::<Shaders>()
            .init_resource::<Alphas>()
            .add_systems(Update, (|alpha: Res<Alphas>, mut drawing: AlphaDrawing| {
                drawing.sync(&alpha.0);
            }, cover).chain());
        let own = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(nif_material(StandardMaterial::default()));
        let fill = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(nif_material(StandardMaterial::default()));
        let reference = 0xFF00_0009;
        let mesh = app.world_mut().spawn((RefVisual(reference), Mesh3d(Handle::default()), MeshMaterial3d(own.clone()))).id();
        app.world_mut().resource_mut::<Shaders>().active.insert(Source::Script(1), Active::membrane_for_test(reference, fill));
        // Presence fade starts first.
        nvf_play::fade::apply_factor(app.world_mut(), mesh, nvf_play::fade::FadeChannel::Presence, Some(0.5));
        app.world_mut().resource_mut::<Alphas>().0.insert(FormId(reference), 0.0);
        app.update();
        assert_eq!(app.world().get::<nvf_play::fade::Fading>(mesh).unwrap().original(), &own);
        assert_eq!(app.world().resource::<Shaders>().active[&Source::Script(1)].covered[&mesh].len(), 1, "covered as its own material even while fading");
        // Presence fade ends while actor alpha remains active.
        nvf_play::fade::apply_factor(app.world_mut(), mesh, nvf_play::fade::FadeChannel::Presence, None);
        app.update();
        assert_eq!(app.world().get::<nvf_play::fade::Fading>(mesh).unwrap().original(), &own, "the own material stays the original");
        assert_ne!(app.world().get::<MeshMaterial3d<NifMaterial>>(mesh).unwrap().0, own, "still drawn see-through for actor alpha");
        assert_eq!(app.world().resource::<Shaders>().active[&Source::Script(1)].covered[&mesh].len(), 1, "covered as its own material");
        app.world_mut().resource_mut::<Alphas>().0.clear();
        app.update();
        assert!(app.world().get::<nvf_play::fade::Fading>(mesh).is_none());
        assert_eq!(app.world().get::<MeshMaterial3d<NifMaterial>>(mesh).unwrap().0, own, "the release gives back a material that exists");
        assert_eq!(app.world().resource::<Shaders>().active[&Source::Script(1)].covered[&mesh].len(), 1);
    }
}
