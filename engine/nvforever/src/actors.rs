//! Placed NPCs drawn as one assembled entity each (workstream 5,
//! docs/design/static-actors.md): `fnv-actor` decides the parts, `nvf_assets::actor`
//! assembles them on one skeleton, playing its locomotion clips with `--actor-pose idle`
//! (the default; docs/spec-notes/actors.md §7–8, `locomotion.rs`) or frozen in the bind
//! pose; FaceGen shapes the faces (`facegen.rs`). Placed creatures (part D, actors.md
//! §10) are assembled the same way from their skeleton and NIFZ models and play their
//! skeleton's idle.
//!
//! References not enabled at the start of a game (initially disabled, or behind a
//! disabled enable parent) are spawned hidden ([`SpawnedDisabled`]), so a script's
//! Enable can show them through `RefVisual` sync (docs/design/layer5-play.md §4).

use bevy::ecs::lifecycle::HookContext;
use bevy::ecs::world::DeferredWorld;
use bevy::math::Affine3A;
use bevy::prelude::*;
use fnv_actor::locomotion::{Blend, Body, Move};
use fnv_actor::{ActorForms, Appearance, CreatureAppearance, Resolver};
use fnv_esm::{LoadOrder, Schema};
use fnv_sim::{FormId, FormStore};
use nvf_assets::actor::{blend_player, spawn_actor, ActorCtx, ClipSource, PartUse, PreparedSkeleton};
use nvf_play::RefVisual;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::facegen::FaceGenCache;
use crate::limbs::ActorRig;
use crate::locomotion::{simulate, walk_test_at, walk_test_period, ClipSet, Locomotion, WalkTest};
use crate::weapons::{Stance, WeaponHold, WeaponMode};
use fnv_actor::weapon::AnimType;

/// How placed actors are posed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum ActorPose {
    /// The skeleton's bind pose, frozen (`--actor-pose bind`).
    Bind,
    /// The skeleton's locomotion clips (`locomotion.rs`): the idle in a loop while an
    /// actor stands; the default since 2026-10-09 (owner decision,
    /// docs/design/static-actors.md).
    #[default]
    Idle,
}

/// `--actor-walk-test`: speed (units/s) and how far from the start position (game units;
/// 0 = any distance) actors take part.
#[derive(Debug, Clone, Copy)]
pub struct WalkTestArgs {
    pub speed: f32,
    pub radius: f32,
    pub center: Vec3,
}

/// Where in its clip an actor starts (0–1): spread by reference FormID so neighbours do
/// not move in step [engine choice, not a game rule].
pub fn clip_phase(reference: u32) -> f32 {
    (reference.wrapping_mul(2_654_435_761) >> 8) as f32 / (1u32 << 24) as f32
}

/// The records actors are resolved from (built once at start).
#[derive(Resource)]
pub struct ActorWorld {
    pub actors: ActorForms,
    pub forms: FormStore,
    /// FaceGen on/off (`--no-facegen`), and its file cache.
    pub facegen_enabled: bool,
    pub facegen: Mutex<FaceGenCache>,
    /// Skin tints and face shapes add the race's FaceGen values to the actor's own
    /// (`--no-race-facegen` turns it off; actors.md §4.2, §4.3).
    pub race_facegen: bool,
    /// Test aid (`--show-disabled-actors`): draw actors that are not enabled at the start
    /// instead of spawning them hidden.
    pub show_disabled: bool,
    pub pose: ActorPose,
    /// With [`ActorPose::Idle`]: hold every actor at this time instead of playing
    /// (screenshots): the idle's clip time, or with the walk test the script time.
    pub pose_time: Option<f32>,
    pub walk_test: Option<WalkTestArgs>,
    /// How carried weapons are shown (`--actor-weapons`), and the draw/holster test aid's
    /// period (`--actor-weapon-test`).
    pub weapon_mode: WeaponMode,
    pub weapon_test: Option<f32>,
    /// Clip sets and weapon stances, loaded once each.
    pub anims: Anims,
    /// How many times ResurrectActor rolled a reference's random choices again
    /// (`fnv_game::resurrect`, told by `resurrect.rs`): the seed of its leveled picks.
    pub rolls: Mutex<HashMap<u32, u32>>,
    /// The race and sex scripts gave a reference (AgeRace, SexChange;
    /// `fnv_game::appearance`, told by `appearance.rs`): drawn in place of the record's.
    pub changes: Mutex<HashMap<u32, fnv_actor::Change>>,
    /// A chosen look a reference is drawn with when none is passed: a co-op player's
    /// (`coop.rs`, keyed by its puppet's reference).
    pub looks: Mutex<HashMap<u32, fnv_actor::Look>>,
    /// The face morphs scripts gave a reference (MatchFaceGeometry; `fnv_game::
    /// appearance`, told by `appearance.rs`): applied to its face as it is drawn.
    pub faces: Mutex<HashMap<u32, Vec<FaceMorph>>>,
    /// NPC bases whose look comes from another base for the session (ShowOff's
    /// BaseCopyFaceGenFrom, `fnv_game::Game::face_from_base`, told by `appearance.rs`).
    pub face_bases: Mutex<HashMap<u32, u32>>,
    /// The level each placed actor reads its leveled lists at in the game
    /// (`fnv_game::Game::leveled_at`, told by `zones.rs`); absent: the display level 1
    /// (docs/spec-notes/encounter-zones.md §5).
    pub levels: Mutex<HashMap<u32, fnv_sim::zones::LevelAt>>,
}

/// A MatchFaceGeometry call as the resolver applies it ([`ActorWorld::faces`]): the
/// source's shape coefficients when the game kept them, else its base and reference to
/// draw them from.
#[derive(Clone, Debug, PartialEq)]
pub struct FaceMorph {
    pub source_base: u32,
    pub source_ref: u32,
    pub percent: f32,
    pub fggs: Vec<f32>,
    pub fgga: Vec<f32>,
}

type ClipSets = HashMap<(String, Body, &'static str), Option<Arc<ClipSet>>>;
type Stances = HashMap<(String, String), Option<Arc<Stance>>>;

/// Locomotion clip sets by (skeleton, body, stance; `None` when no idle was found) and
/// weapon stances by (skeleton, prefix; `None` when they could not be loaded), loaded
/// once each: when an actor is drawn, or ahead of it on a worker thread
/// (`readahead.rs`, [`Anims::warm`]). Loading does not hold the lock, so drawing an
/// actor never waits for a worker; both may load the same set, and the first kept wins.
#[derive(Clone, Default)]
pub struct Anims {
    sets: Arc<Mutex<ClipSets>>,
    stances: Arc<Mutex<Stances>>,
}

impl ActorWorld {
    pub fn build(lo: &LoadOrder, schema: &Schema) -> ActorWorld {
        ActorWorld {
            actors: ActorForms::build(lo, schema),
            forms: FormStore::build(lo, schema),
            facegen_enabled: true,
            facegen: Mutex::new(FaceGenCache::default()),
            race_facegen: true,
            show_disabled: false,
            pose: ActorPose::Bind,
            pose_time: None,
            walk_test: None,
            weapon_mode: WeaponMode::None,
            weapon_test: None,
            anims: Anims::default(),
            rolls: Mutex::new(HashMap::new()),
            changes: Mutex::new(HashMap::new()),
            looks: Mutex::new(HashMap::new()),
            faces: Mutex::new(HashMap::new()),
            face_bases: Mutex::new(HashMap::new()),
            levels: Mutex::new(HashMap::new()),
        }
    }

    /// The reference whose FormID seeds the leveled picks of `reference`: itself until a
    /// resurrection rolled them again (`fnv_sim::leveled::roll_seed`).
    fn seed(&self, reference: u32) -> FormId {
        let rolls = self.rolls.lock().map_or(0, |r| r.get(&reference).copied().unwrap_or(0));
        fnv_sim::leveled::roll_seed(FormId(reference), rolls)
    }

    /// The level `reference` reads leveled lists at (`Self::levels`).
    pub fn level(&self, reference: u32) -> fnv_sim::zones::LevelAt {
        let display = fnv_sim::zones::LevelAt::plain(fnv_actor::resolve::DISPLAY_LEVEL);
        self.levels.lock().map_or(display, |l| l.get(&reference).copied().unwrap_or(display))
    }

    pub fn resolve(&self, base: u32, reference: u32) -> Result<Appearance, String> {
        self.resolve_wearing(base, reference, None)
    }

    /// [`Self::resolve`] wearing the given armour instead of the chosen one
    /// (`Resolver::resolve_wearing`; the player's body).
    pub fn resolve_wearing(&self, base: u32, reference: u32, wearing: Option<&[FormId]>) -> Result<Appearance, String> {
        self.resolve_as(base, reference, wearing, None)
    }

    /// A script's race or sex change of `reference` (`Self::changes`).
    pub fn change(&self, reference: u32) -> fnv_actor::Change {
        self.changes.lock().map_or_else(|_| Default::default(), |c| c.get(&reference).copied().unwrap_or_default())
    }

    /// [`Self::resolve_wearing`] with a chosen look (the player's, from the character
    /// creator) in place of the record's.
    pub fn resolve_as(&self, base: u32, reference: u32, wearing: Option<&[FormId]>, look: Option<&fnv_actor::Look>) -> Result<Appearance, String> {
        let change = self.change(reference);
        let kept = if look.is_none() { self.looks.lock().ok().and_then(|l| l.get(&reference).cloned()) } else { None };
        let look = look.or(kept.as_ref());
        let resolver = Resolver { actors: &self.actors, forms: &self.forms };
        let (seed, level) = (self.seed(reference), self.level(reference));
        let mut app = resolver.resolve_changed_at(FormId(base), seed, wearing, look, change, level).map_err(|e| e.to_string())?;
        // BaseCopyFaceGenFrom: the source NPC's look (race, sex, face, skin, hair, eyes,
        // head parts) wearing this actor's own clothes, as this actor [documented:
        // CopyFaceGenFrom; hypothesis: height and outfit stay the actor's own].
        let source = self.face_bases.lock().ok().and_then(|m| m.get(&base).copied());
        if let (Some(src), None) = (source, look) {
            match resolver.resolve_changed_at(FormId(src), seed, Some(wearing.unwrap_or(&app.worn)), None, change, level) {
                Ok(mut copy) => {
                    copy.npc = app.npc;
                    copy.edid = app.edid.take();
                    copy.height = app.height;
                    app = copy;
                }
                Err(e) => warn!("appearance: BaseCopyFaceGenFrom source {src:08X} of {base:08X}: {e}"),
            }
        }
        let morphs = self.faces.lock().ok().and_then(|f| f.get(&reference).cloned());
        for m in morphs.unwrap_or_default() {
            self.morph_face(&mut app, &m);
        }
        Ok(app)
    }

    /// One MatchFaceGeometry morph on a drawn face (`fnv_game::appearance`): its shape
    /// coefficients move towards the source's, the ones kept with the call or else the
    /// source's own as drawn without its morphs [design]. A source with no face leaves
    /// it as it is.
    fn morph_face(&self, app: &mut Appearance, m: &FaceMorph) {
        let wide = |v: &[f32]| v.iter().map(|&x| f64::from(x)).collect::<Vec<f64>>();
        let source = if m.fggs.is_empty() && m.fgga.is_empty() {
            let r = FormId(m.source_ref);
            let at = Resolver { actors: &self.actors, forms: &self.forms }.resolve_changed_at(FormId(m.source_base), self.seed(m.source_ref), None, None, self.change(m.source_ref), self.level(m.source_ref));
            match at.ok().and_then(|a| a.facegen) {
                Some(f) => (f.fggs, f.fgga),
                None => {
                    warn!("appearance: MatchFaceGeometry source {r} has no face to match");
                    return;
                }
            }
        } else {
            (wide(&m.fggs), wide(&m.fgga))
        };
        let face = app.facegen.get_or_insert_with(Default::default);
        face.fggs = fnv_game::appearance::blend_coefficients(&face.fggs, &source.0, f64::from(m.percent));
        face.fgga = fnv_game::appearance::blend_coefficients(&face.fgga, &source.1, f64::from(m.percent));
    }

    pub fn resolve_creature(&self, base: u32, reference: u32) -> Result<CreatureAppearance, String> {
        Resolver { actors: &self.actors, forms: &self.forms }.resolve_creature_at(FormId(base), self.seed(reference), self.level(reference)).map_err(|e| e.to_string())
    }
}

impl Anims {
    /// The set at `key`, loaded with `load` (the lock not held) if it is not there yet.
    fn set_or_load(&self, key: (String, Body, &'static str), load: impl FnOnce() -> Option<Arc<ClipSet>>) -> Option<Arc<ClipSet>> {
        if let Some(set) = self.sets.lock().expect("clip sets").get(&key) {
            return set.clone();
        }
        let set = load();
        self.sets.lock().expect("clip sets").entry(key).or_insert(set).clone()
    }

    /// The locomotion clip set of a skeleton and body (loaded once), if it has an idle.
    pub fn clip_set(&self, ctx: &mut impl ClipSource, skeleton: &str, skel: &PreparedSkeleton, body: Body) -> Option<Arc<ClipSet>> {
        self.clip_set_stance(ctx, skeleton, skel, body, crate::locomotion::STANCE)
    }

    /// [`Self::clip_set`] of a stance prefix (`locomotion::SNEAK_STANCE`).
    pub fn clip_set_stance(&self, ctx: &mut impl ClipSource, skeleton: &str, skel: &PreparedSkeleton, body: Body, stance: &'static str) -> Option<Arc<ClipSet>> {
        self.set_or_load((fnv_vfs::normalize(skeleton), body, stance), || {
            let (set, line) = ClipSet::load_stance(ctx, skeleton, skel, body, stance);
            info!("actor locomotion: {line}");
            set.clips.contains_key(&Move::Idle).then(|| Arc::new(set))
        })
    }

    /// The limping set (`locomotion::ClipSet::load_hurt`) built on `base`, the standing
    /// set of the skeleton and body, or with `alert` on a creature's alert set with the
    /// alert stance's limping clips too (loaded once each).
    pub fn clip_set_hurt(&self, ctx: &mut impl ClipSource, skeleton: &str, skel: &PreparedSkeleton, body: Body, base: &ClipSet, alert: bool) -> Option<Arc<ClipSet>> {
        let stances: &[&str] = if alert { &[crate::locomotion::STANCE, crate::locomotion::ALERT_STANCE] } else { &[crate::locomotion::STANCE] };
        self.set_or_load((fnv_vfs::normalize(skeleton), body, if alert { "hurt-alert" } else { "hurt" }), || {
            let (set, line) = ClipSet::load_hurt(ctx, skeleton, skel, base, stances)?;
            info!("actor locomotion: {line}");
            Some(Arc::new(set))
        })
    }

    /// The swimming set of a skeleton and body (`ClipSet::swim_from` over
    /// `locomotion::SWIM_STANCES`, the standing set `calm` filling the moves it lacks),
    /// loaded once; `None` when the folder has no swimming movement clip.
    pub fn swim_set(&self, ctx: &mut impl ClipSource, skeleton: &str, skel: &PreparedSkeleton, body: Body, calm: &ClipSet) -> Option<Arc<ClipSet>> {
        self.set_or_load((fnv_vfs::normalize(skeleton), body, "swim+"), || {
            let mut lines = Vec::new();
            let stances = crate::locomotion::SWIM_STANCES
                .iter()
                .map(|st| {
                    let (set, line) = ClipSet::load_stance(ctx, skeleton, skel, body, st);
                    if !set.clips.is_empty() {
                        lines.push(line);
                    }
                    set
                })
                .collect();
            let set = ClipSet::swim_from(stances, calm);
            if set.is_some() {
                info!("actor locomotion (swimming): {}", lines.join(" | "));
            }
            set.map(Arc::new)
        })
    }

    /// A creature's alert-stance set (`ClipSet::alert_from`: its `h2h` movement clips over
    /// `calm`), loaded once per skeleton; none for a skeleton that keeps its calm clips
    /// (`creature_pace::has_alert_stance`).
    pub fn alert_set(&self, ctx: &mut impl ClipSource, skeleton: &str, skel: &PreparedSkeleton, calm: &ClipSet) -> Option<Arc<ClipSet>> {
        if !crate::creature_pace::has_alert_stance(skeleton) {
            return None;
        }
        self.set_or_load((fnv_vfs::normalize(skeleton), Body::Male, crate::locomotion::ALERT_STANCE), || {
            let (stance, line) = ClipSet::load_stance(ctx, skeleton, skel, Body::Male, crate::locomotion::ALERT_STANCE);
            let set = ClipSet::alert_from(stance, calm);
            if set.is_some() {
                info!("actor locomotion (alert stance): {line}");
            }
            set.map(Arc::new)
        })
    }

    /// The stance `prefix` bound to the skeleton at `skeleton`, loaded once.
    pub fn stance(&self, ctx: &mut impl ClipSource, skeleton: &str, skel: &PreparedSkeleton, prefix: &str) -> Option<Arc<Stance>> {
        let key = (fnv_vfs::normalize(skeleton), prefix.to_string());
        if let Some(s) = self.stances.lock().expect("stances").get(&key) {
            return s.clone();
        }
        let s = match Stance::load(ctx, skeleton, skel, prefix) {
            Ok((s, line)) => {
                info!("actor weapons: {line}");
                Some(Arc::new(s))
            }
            Err(e) => {
                warn!("actor weapons: {prefix}: {e}");
                None
            }
        };
        self.stances.lock().expect("stances").entry(key).or_insert(s).clone()
    }

    /// Whether the stance `prefix` of `skeleton` is loaded (or known not to load).
    pub fn stance_loaded(&self, skeleton: &str, prefix: &str) -> bool {
        self.stances.lock().expect("stances").contains_key(&(fnv_vfs::normalize(skeleton), prefix.to_string()))
    }

    /// Whether the standing set of `skeleton` and `body` and the stance `prefix` (if any)
    /// are loaded (or known not to load), and the stances of its body weapons `arms`.
    pub fn loaded(&self, skeleton: &str, body: Body, prefix: Option<&str>, arms: &[&str]) -> bool {
        let skeleton = fnv_vfs::normalize(skeleton);
        if !self.sets.lock().expect("clip sets").contains_key(&(skeleton.clone(), body, crate::locomotion::STANCE)) {
            return false;
        }
        let stances = self.stances.lock().expect("stances");
        let has = |p: &str| stances.contains_key(&(skeleton.clone(), p.to_string()));
        prefix.is_none_or(|p| has(p) && has(AnimType::HandToHand.prefix())) && arms.iter().all(|p| has(p))
    }

    /// Load what drawing an actor of `skeleton` and `body` takes (as [`spawn`] and
    /// [`spawn_creature`] ask for it), the stance `prefix` it holds its weapon in and the
    /// stances of a creature's body weapons `arms` (`body_weapons::attach`): run on a
    /// worker thread ahead of the actor (`readahead.rs`).
    #[allow(clippy::too_many_arguments)]
    pub fn warm(&self, ctx: &mut impl ClipSource, skeleton: &str, skel: &PreparedSkeleton, body: Body, creature: bool, prefix: Option<&str>, arms: &[&str]) {
        for arm in arms {
            self.stance(ctx, skeleton, skel, arm);
        }
        if let Some(set) = self.clip_set(ctx, skeleton, skel, body) {
            if creature {
                self.clip_set_hurt(ctx, skeleton, skel, body, &set, false);
                self.swim_set(ctx, skeleton, skel, body, &set);
                if let Some(alert) = self.alert_set(ctx, skeleton, skel, &set) {
                    self.clip_set_hurt(ctx, skeleton, skel, body, &alert, true);
                }
            } else {
                self.clip_set_stance(ctx, skeleton, skel, body, crate::locomotion::SNEAK_STANCE);
                self.swim_set(ctx, skeleton, skel, body, &set);
                self.clip_set_hurt(ctx, skeleton, skel, body, &set, false);
            }
        }
        if let Some(prefix) = prefix {
            self.stance(ctx, skeleton, skel, prefix);
            // `arm` falls back to fists when a weapon's model or stance is missing.
            if !creature && prefix != AnimType::HandToHand.prefix() {
                self.stance(ctx, skeleton, skel, AnimType::HandToHand.prefix());
            }
        }
    }
}

/// Marks the root of an actor, or the light source of a light, spawned hidden because its
/// reference is not enabled at the start of a game (world.rs `WorldData::enabled`).
/// `RefVisual` sync shows it when a script enables it; if the simulation already has it
/// enabled when its cell loads (enabled while the cell was away), it is shown at once
/// (component hook, so no system needs registering).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[component(on_add = show_if_present)]
pub struct SpawnedDisabled(pub u32);

fn show_if_present(mut world: DeferredWorld, ctx: HookContext) {
    let Some(r) = world.get::<SpawnedDisabled>(ctx.entity).map(|s| s.0) else { return };
    let present = world.get_resource::<nvf_play::sim::SimState>().is_some_and(|s| s.present(r));
    if present {
        if let Some(mut v) = world.get_mut::<Visibility>(ctx.entity) {
            *v = Visibility::Inherited;
        }
    }
}

/// Hide `root` (spawned for `reference`) until a script enables it ([`SpawnedDisabled`]).
pub fn spawn_hidden(commands: &mut Commands, root: Entity, reference: u32) {
    commands.entity(root).insert((Visibility::Hidden, SpawnedDisabled(reference)));
}

#[derive(Debug, Default, Clone)]
pub struct ActorStats {
    /// Successful NPC spawns by appearance source (profiled per system call).
    pub equipment_spawns: usize,
    pub record_spawns: usize,
    pub drawn: usize,
    pub unresolved: usize,
    pub parts_missing: usize,
    /// Skinned shapes whose bones the skeleton lacks (drawn unskinned).
    pub unbound_shapes: usize,
    /// Actors playing clips, those whose idle could not be loaded, and those under the
    /// walk test.
    pub animated: usize,
    pub clip_missing: usize,
    pub walking: usize,
    /// NPC bodies whose skin texture was tinted with the race's body `.egt`.
    pub skins_tinted: usize,
    /// NPCs whose hand skin texture was tinted with the hand `.egt`.
    pub hands_tinted: usize,
    /// Placed creatures drawn, unresolved, playing their idle, without an idle.
    pub creatures: usize,
    pub creatures_unresolved: usize,
    pub creatures_animated: usize,
    pub creature_clip_missing: usize,
    /// Actors and creatures spawned hidden (not enabled at the start).
    pub spawned_hidden: usize,
    /// Actors holding a weapon, and those whose weapon model or stance clips are missing.
    pub armed: usize,
    pub weapon_missing: usize,
    /// Actors holding their fists (no weapon, or the Fists): the `h2h` stance alone.
    pub fists: usize,
}

/// Take back what worker threads made for actors: parts read ahead
/// (`ActorParts::take_fetched`).
pub fn take_background_work(parts: Option<ResMut<nvf_assets::actor::ActorParts>>, mut models: nvf_assets::model::ModelCtx, time: Res<Time>) {
    if let Some(mut parts) = parts {
        parts.take_fetched(&mut models, time.elapsed_secs_f64());
    }
}

/// A race skin texture and the `.egt` that tints it; `offset` is made on first use.
struct SkinTint {
    hand: bool,
    texture: String,
    egt: String,
    offset: Option<Option<Handle<Image>>>,
}

/// Per shape of `part` (a part given texture `given`; `armor` for a worn model):
/// - the race skin texture its skin shape takes in place of the one it names (skin
///   showing inside an outfit or a glove, so a ghoul's or a raider's arms are not human
///   skin [hypothesis, actors.md §3.1]; `worn` is `Appearance::worn_skin_texture`);
/// - the texture (`fnv_vfs::texture_path`) its skin offset is looked up by: the worn skin,
///   else the part's given texture on the shapes that take it, else its own; none for caps.
///
/// `part_uses` draws with both; read-ahead (`readahead::Plan::offsets`) asks the second
/// which skin offsets a drawing will take.
pub fn skin_textures<'a>(part: &nvf_assets::actor::PreparedPart, given: Option<&'a str>, armor: bool, worn: impl Fn(Option<&str>) -> Option<&'a str>) -> (Vec<Option<&'a str>>, Vec<Option<String>>) {
    let takes_texture = part.texture_targets();
    let worn_skin: Vec<Option<&'a str>> = part.shapes.iter().map(|s| if armor && !s.cap && s.is_skin() { worn(s.base_texture()) } else { None }).collect();
    let lookup = part
        .shapes
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let given = worn_skin[i].or(given.filter(|_| takes_texture[i]));
            if s.cap { None } else { given.or_else(|| s.base_texture()).map(fnv_vfs::texture_path) }
        })
        .collect();
    (worn_skin, lookup)
}

/// The parts of a resolved actor ready to assemble, each with its index in `a.parts`:
/// models loaded, textures, FaceGen, body skin and hair colour applied, the hair's hat
/// variant chosen. Parts whose model cannot be loaded are left out (counted in `stats`).
/// Placed NPCs and the player's body (`player_body.rs`) build their parts here.
pub fn part_uses(a: &Appearance, world: &ActorWorld, ctx: &mut ActorCtx, stats: &mut ActorStats) -> Vec<(usize, PartUse)> {
    let mut uses = Vec::with_capacity(a.parts.len());
    // The skin textures, each tinted once per actor: the race's upper body texture with
    // the body `.egt`, its hand texture with the hand `.egt` (the left one for both
    // hands) [hypothesis, actors.md §4.1]. Face, body and hands take the actor's texture
    // coefficients plus its race's [hypothesis, actors.md §4.2].
    let fgts = a.skin_fgts(world.race_facegen).filter(|_| world.facegen_enabled);
    let fgts = fgts.as_deref();
    // The shape of the face and of what is fitted to it (hair, brows, eyes, hats): the
    // actor's geometry coefficients plus its race's [hypothesis, actors.md §4.3].
    let shape = a.shape_facegen(world.race_facegen).filter(|_| world.facegen_enabled);
    let mut skins: Vec<SkinTint> = Vec::new();
    if let Some(body_egt) = a.body_egt.as_deref().filter(|_| fgts.is_some()) {
        let hand_egt = fnv_actor::resolve::hand_egt(body_egt, true);
        for (hand, texture, egt) in [(false, &a.body_texture, Some(body_egt.to_string())), (true, &a.hand_texture, hand_egt)] {
            if let (Some(texture), Some(egt)) = (texture.as_deref().map(fnv_vfs::texture_path), egt) {
                skins.push(SkinTint { hand, texture, egt, offset: None });
            }
        }
    }
    let pipboy = a.parts.iter().any(|p| world.actors.armors.get(&p.form).is_some_and(|ar| ar.slots & fnv_actor::resolve::slot::PIPBOY != 0));
    for (pi, p) in a.parts.iter().enumerate() {
        // A worn item's or head part's alternate textures (texture-swaps.md §4) are in
        // the part's own shapes and materials from here on.
        let Some(part) = info_span!("actor::part_swapped").in_scope(|| ctx.part_swapped(&p.model, world.actors.part_alt_textures(p, a.female))) else {
            stats.parts_missing += 1;
            continue;
        };
        let mut materials: Vec<_> = part.shapes.iter().map(|s| s.material.clone()).collect();
        // The part's texture goes on its skin shapes when it has any: the race bodies
        // also hold the underwear, which keeps its own texture (actors.md §3.1).
        let takes_texture = part.texture_targets();
        if let Some(t) = &p.texture {
            for (i, _) in takes_texture.iter().enumerate().filter(|(_, &takes)| takes) {
                materials[i] = ctx.material_with_texture(&p.model, &part, i, t);
            }
        }
        let (worn_skin, skin_lookup) = skin_textures(&part, p.texture.as_deref(), p.role == "armor", |t| a.worn_skin_texture(t));
        for (i, t) in worn_skin.iter().enumerate() {
            if let Some(t) = t.filter(|t| part.shapes[i].base_texture().map(fnv_vfs::texture_path) != Some(fnv_vfs::texture_path(t))) {
                materials[i] = ctx.material_with_texture(&p.model, &part, i, t);
            }
        }
        // #2981: a shape whose role makes it skin (one of the race's skin textures) or the
        // face (the head's texture targets), or the hair model's, takes no authored set, for
        // good, from the moment the role is known here, whether or not a FaceGen tint follows
        // (the actor may have no FaceGen data, or its offset may be missing). Skipped when no
        // shape can take a set (the setting off, or no active set, authored or height only,
        // in the mount): nothing to exclude, and no copies made.
        if nvf_assets::authored::mount_has_sets(&ctx.models.data) {
            let mut cache = world.facegen.lock().expect("facegen cache");
            for (i, s) in part.shapes.iter().enumerate().filter(|(_, s)| !s.cap) {
                let given = worn_skin[i].or(p.texture.as_deref().filter(|_| takes_texture[i]));
                let race_skin = given.or_else(|| s.base_texture()).map(fnv_vfs::texture_path).is_some_and(|t| {
                    [&a.body_texture, &a.hand_texture].into_iter().flatten().any(|k| fnv_vfs::texture_path(k) == t)
                });
                if race_skin || p.role == "hair" || (p.role == "head:head" && takes_texture[i]) {
                    materials[i] = cache.role_legacy(&mut ctx.models.materials, &materials[i]);
                }
            }
        }
        let mut meshes = vec![None; part.shapes.len()];
        if let Some(shape) = &shape {
            let _span = info_span!("actor::facegen_morph").entered();
            let mut cache = world.facegen.lock().expect("facegen cache");
            meshes = cache.morph(ctx, &part, &p.model, shape);
            // The face: the head's texture (the race's, on its skin shapes) takes the
            // offset of the `.egt` beside the head model.
            if p.role == "head:head" {
                let _face_span = info_span!("actor::face_tint").entered();
            if let Some(offset) = fgts.and_then(|fgts| cache.face_offset(ctx, &p.model, fgts)) {
                    for (i, _) in takes_texture.iter().enumerate().filter(|(_, &takes)| takes) {
                        if let Some(m) = cache.tinted(&mut ctx.models.materials, &materials[i], &offset) {
                            materials[i] = m;
                        }
                    }
                }
            }
        }
        // Skin: every shape whose diffuse is one of the race's skin textures (the race
        // body and hands, and skin inside outfits and gloves) takes the skin's offset
        // [hypothesis, actors.md §4.1].
        let skin_span = info_span!("actor::skin_tint").entered();
        if let Some(fg) = fgts {
            for (i, tex) in skin_lookup.iter().enumerate() {
                let Some(tex) = tex else { continue };
                let Some(skin) = skins.iter_mut().find(|k| &k.texture == tex) else { continue };
                let mut cache = world.facegen.lock().expect("facegen cache");
                let offset = skin
                    .offset
                    .get_or_insert_with(|| {
                        let offset = cache.skin_offset(ctx, &skin.egt, fg);
                        *(if skin.hand { &mut stats.hands_tinted } else { &mut stats.skins_tinted }) += usize::from(offset.is_some());
                        offset
                    })
                    .clone();
                if let Some(m) = offset.and_then(|offset| cache.tinted(&mut ctx.models.materials, &materials[i], &offset)) {
                    materials[i] = m;
                }
            }
        }
        // Hair colour: the texture times HCLR, doubled, in gamma space [hypothesis,
        // actors.md §3: the hair textures are close to grey; a plain multiply leaves
        // Sunny Smiles' hair dark brown where the owner's screenshot shows auburn]. The
        // hair model's shapes, and the head parts' hair-shader shapes (beards), which
        // were drawn untinted, near white (playtest list 5 item 26,
        // actors.md §3).
        drop(skin_span);
        if p.role == "hair" || p.role == "head_part" {
            if let Some([r, g, b]) = a.hair_color {
                let tint = Color::srgb(2.0 * f32::from(r) / 255.0, 2.0 * f32::from(g) / 255.0, 2.0 * f32::from(b) / 255.0).to_linear();
                for (i, s) in part.shapes.iter().enumerate() {
                    if s.cap || (p.role == "head_part" && !s.is_hair()) {
                        continue;
                    }
                    if let Some(mut m) = ctx.models.materials.get(&materials[i]).cloned() {
                        let base = m.base.base_color.to_linear();
                        m.base.base_color =
                            Color::LinearRgba(LinearRgba::new(base.red * tint.red, base.green * tint.green, base.blue * tint.blue, base.alpha));
                        materials[i] = ctx.models.materials.add(m);
                    }
                }
            }
        }
        // The hair model is drawn as a solid cut-out core (`nif_spawn::hair_model_cut_out`,
        // #1454) with its fainter texels blended over it from a second draw
        // (`nif_spawn::hair_model_layer`, #1576): shaved sides stay see-through.
        let mut layers = Vec::new();
        if p.role == "hair" {
            layers = vec![None; part.shapes.len()];
            for (i, _) in part.shapes.iter().enumerate().filter(|(_, s)| !s.cap) {
                let Some(mut m) = ctx.models.materials.get(&materials[i]).cloned() else { continue };
                if matches!(m.base.alpha_mode, AlphaMode::Blend) {
                    layers[i] = nvf_assets::nif_spawn::hair_model_layer(&m).map(|l| ctx.models.materials.add(l));
                    nvf_assets::nif_spawn::hair_model_cut_out(&mut m);
                    materials[i] = ctx.models.materials.add(m);
                }
            }
        }
        // The hair model holds a `NoHat` and a `Hat` shape (actors.md §2.4).
        let mut hidden_shapes = if p.role == "hair" { vec![if a.hat { "NoHat" } else { "Hat" }.to_string()] } else { Vec::new() };
        // The Pip-Boy light's glow shows only with the light on, which is not built, and
        // outfits hold a left sleeve for wearing a Pip-Boy and one for not (actors.md §1.2).
        hidden_shapes.extend(part.shapes.iter().filter_map(|s| s.name.clone().filter(|n| fnv_actor::mesh::hidden_for_pipboy(n, pipboy))));
        uses.push((pi, PartUse { meshes, part, materials, layers, hidden_shapes, by_ancestor: false }));
    }
    uses
}

/// Spawn the actor placed by `reference` under `parent` with the reference transform
/// `rt` (parent space); `at` is its position in game space. Returns whether anything was
/// drawn ([`spawn_body`] returns its root entity).
#[allow(clippy::too_many_arguments)]
pub fn spawn(
    commands: &mut Commands,
    parent: Entity,
    rt: Affine3A,
    at: Vec3,
    reference: u32,
    base: u32,
    world: &ActorWorld,
    sim: Option<&fnv_sim::World<'_>>,
    ctx: &mut ActorCtx,
    stats: &mut ActorStats,
    hidden: bool,
) -> bool {
    spawn_body(commands, parent, rt, at, reference, base, world, sim, ctx, stats, hidden).is_some()
}

/// Spawn the actor placed by `reference` under `parent` with the reference transform
/// `rt` (parent space); `at` is its position in game space. Returns its root entity (the
/// one under `parent` carrying its `RefVisual` and `ActorRig`), `None` if nothing was drawn.
#[allow(clippy::too_many_arguments)]
pub fn spawn_body(
    commands: &mut Commands,
    parent: Entity,
    rt: Affine3A,
    at: Vec3,
    reference: u32,
    base: u32,
    world: &ActorWorld,
    sim: Option<&fnv_sim::World<'_>>,
    ctx: &mut ActorCtx,
    stats: &mut ActorStats,
    hidden: bool,
) -> Option<Entity> {
    let appearance_span = info_span!("actor::appearance").entered();
    let equipment = crate::outfits::Equipment::of(world, sim, reference);
    let a = match crate::outfits::appearance(world, base, reference, equipment.as_ref()) {
        Ok(a) => a,
        Err(e) => {
            stats.unresolved += 1;
            debug!("actor {reference:08X}: {e}");
            return None;
        }
    };
    let Some(skel) = ctx.skeleton(&a.skeleton) else {
        stats.unresolved += 1;
        return None;
    };
    drop(appearance_span);
    let uses_span = info_span!("actor::part_uses").entered();
    let (part_index, uses): (Vec<usize>, Vec<PartUse>) = part_uses(&a, world, ctx, stats).into_iter().unzip();
    drop(uses_span);
    // The race's height scales the actor [hypothesis, actors.md §3], times its own (NAM6,
    // docs/spec-notes/actor-flags.md §5) [hypothesis].
    let rt = rt * Affine3A::from_scale(Vec3::splat((a.race_height * a.height) as f32));
    let spawned = info_span!("actor::spawn_entities").in_scope(|| spawn_actor(commands, parent, Transform::from_matrix(Mat4::from(rt)), &skel, &uses));
    let lips_span = info_span!("actor::lips_rig").entered();
    // The shapes a `.tri` can move when the actor speaks (lips.rs).
    if let Some(rig) = crate::lips::rig(&a, &part_index, &uses, &spawned, &ctx.models.data, a.shape_facegen(world.race_facegen).filter(|_| world.facegen_enabled)) {
        commands.entity(spawned.root).insert(rig);
    }
    drop(lips_span);
    commands.entity(spawned.root).insert((
        Name::new(format!("Actor {reference:08X} {}", a.edid.as_deref().unwrap_or(""))),
        RefVisual(reference),
        ActorRig { reference, base, body: base, skeleton: a.skeleton.clone(), skel: skel.clone(), nodes: spawned.nodes.clone(), base_scale: 1.0 },
        // What it wears and holds follows its inventory (`outfits.rs`).
        crate::outfits::Dressed::new(world, &a, &part_index, &spawned),
    ));
    for e in &spawned.drawn {
        commands.entity(*e).insert(RefVisual(reference));
    }
    if hidden && !world.show_disabled {
        spawn_hidden(commands, spawned.root, reference);
        stats.spawned_hidden += 1;
    }
    if world.pose == ActorPose::Idle {
        let _anims_span = info_span!("actor::anims_and_arm").entered();
        let body = if a.female { Body::Female } else { Body::Male };
        match world.anims.clip_set(ctx, &a.skeleton, &skel, body) {
            Some(set) => {
                let mut lo = Locomotion::new(set.clone(), clip_phase(reference));
                // People crouch to sneak (`sneaking.rs`).
                lo.sneak = world.anims.clip_set_stance(ctx, &a.skeleton, &skel, body, crate::locomotion::SNEAK_STANCE);
                // In water they swim (`swimming.rs`).
                lo.swim = world.anims.swim_set(ctx, &a.skeleton, &skel, body, &set);
                // A crippled leg limps (`limping.rs`).
                lo.hurt = world.anims.clip_set_hurt(ctx, &a.skeleton, &skel, body, &set, false);
                let walk = world.walk_test.filter(|w| w.radius <= 0.0 || at.distance(w.center) <= w.radius);
                let mut player = blend_player(&spawned, &skel);
                if let Some(w) = walk {
                    let base = Transform::from_matrix(Mat4::from(rt));
                    let mut wt = WalkTest { base, speed: w.speed, t: clip_phase(reference) * walk_test_period(w.speed) };
                    let tr = match world.pose_time {
                        Some(t) => {
                            // A repeatable pose: the script and blend run from the start to `t`.
                            wt.t = 0.0;
                            lo.hold = true;
                            simulate(&mut wt, &mut lo, t)
                        }
                        None => wt.transform(&walk_test_at(wt.t, w.speed)),
                    };
                    commands.entity(spawned.root).insert((tr, wt));
                    stats.walking += 1;
                } else if let Some(t) = world.pose_time {
                    let len = set.info(Move::Idle).map_or(1.0, |c| c.duration).max(f32::EPSILON);
                    lo.blend = Blend::new(Move::Idle, t / len);
                    lo.hold = true;
                }
                lo.layers(&mut player);
                // Dead-at-spawn actors keep their own weapon (`nvf_play::combat::death_drops`
                // documents this presentation). Arm from the simulated appearance too:
                // changing equipment here must not leave a record-selected weapon on the
                // corpse, or rely on `follow_inventory` to rearm it (it never arms corpses).
                let weapon = info_span!("actor::arm").in_scope(|| arm(commands, &spawned, &skel, &a, reference, world, ctx, &mut player, stats));
                let mut ec = commands.entity(spawned.root);
                ec.insert((lo, player));
                if let Some(w) = weapon {
                    ec.insert(w);
                }
                stats.animated += 1;
            }
            None => stats.clip_missing += 1,
        }
    }
    stats.unbound_shapes += spawned.unbound;
    stats.drawn += 1;
    if equipment.is_some() {
        stats.equipment_spawns += 1;
    } else {
        stats.record_spawns += 1;
    }
    Some(spawned.root)
}

/// [`spawn_creature_body`], returning whether anything was drawn.
#[allow(clippy::too_many_arguments)]
pub fn spawn_creature(
    commands: &mut Commands,
    parent: Entity,
    rt: Affine3A,
    reference: u32,
    base: u32,
    world: &ActorWorld,
    ctx: &mut ActorCtx,
    stats: &mut ActorStats,
    hidden: bool,
) -> bool {
    spawn_creature_body(commands, parent, rt, reference, base, world, ctx, stats, hidden).is_some()
}

/// Spawn the creature placed by `reference` (ACRE) under `parent` (actors.md §10): its
/// skeleton with the NIFZ models, scaled by the reference's and the creature's base
/// scale, playing the skeleton's idle with `--actor-pose idle`. `rt` is the reference
/// transform (parent space). Returns its root entity, `None` if nothing was drawn.
#[allow(clippy::too_many_arguments)]
pub fn spawn_creature_body(
    commands: &mut Commands,
    parent: Entity,
    rt: Affine3A,
    reference: u32,
    base: u32,
    world: &ActorWorld,
    ctx: &mut ActorCtx,
    stats: &mut ActorStats,
    hidden: bool,
) -> Option<Entity> {
    let c = match world.resolve_creature(base, reference) {
        Ok(c) => c,
        Err(e) => {
            stats.creatures_unresolved += 1;
            debug!("creature {reference:08X}: {e}");
            return None;
        }
    };
    let Some(skel) = ctx.skeleton(&c.skeleton) else {
        stats.creatures_unresolved += 1;
        return None;
    };
    let parts_span = info_span!("creature::parts").entered();
    let mut uses = Vec::with_capacity(c.parts.len());
    // Parts that move by themselves, drawn as playing models on their bone below.
    let mut playing: Vec<(String, Arc<nvf_assets::actor::PreparedPart>)> = Vec::new();
    // The creature's idle, read only when a part would play by itself (#2282).
    let mut idle: Option<Option<Arc<nvf_assets::anim_play::Clip>>> = None;
    for p in &c.parts {
        let Some(part) = ctx.part(&p.model) else {
            stats.parts_missing += 1;
            continue;
        };
        if plays_on_bone(&part, &skel) && !idle.get_or_insert_with(|| ctx.clip(&idle_path(&c.skeleton))).as_deref().is_some_and(|clip| drives_part(clip, &part)) {
            playing.push((p.model.clone(), part));
            continue;
        }
        let materials = part.shapes.iter().map(|s| s.material.clone()).collect();
        let meshes = vec![None; part.shapes.len()];
        uses.push(PartUse { meshes, part, materials, layers: Vec::new(), hidden_shapes: Vec::new(), by_ancestor: true });
    }
    // The creature's base scale (BNAM) on top of the reference's [hypothesis, §10.1].
    drop(parts_span);
    let rt = rt * Affine3A::from_scale(Vec3::splat(c.base_scale as f32));
    let spawned = info_span!("creature::spawn_entities").in_scope(|| spawn_actor(commands, parent, Transform::from_matrix(Mat4::from(rt)), &skel, &uses));
    // Where its body weapons fire from, when a body part holds the node (#1535).
    let nodes_span = info_span!("creature::projectile_nodes").entered();
    for p in &c.parts {
        if let Some(part) = ctx.part(&p.model) {
            crate::body_weapons::spawn_part_projectile_nodes(commands, &p.model, &part, &skel, &spawned.nodes);
        }
    }
    drop(nodes_span);
    for (path, part) in &playing {
        let Some(pm) = ctx.models.model(path) else {
            stats.parts_missing += 1;
            continue;
        };
        let (bone, place) = match part.prn.as_ref().and_then(|n| skel.by_name.get(n)) {
            Some(&b) => (b, Transform::from_matrix(Mat4::from(part.root))),
            None => (0, Transform::IDENTITY),
        };
        let (_, drawn) = crate::shot_fx::spawn_playing_itself(commands, spawned.nodes[bone], &pm, place, path);
        for e in drawn {
            commands.entity(e).insert(RefVisual(reference));
        }
        if first_creature_report(base, path) {
            let on = skel.names.get(bone).cloned().flatten().unwrap_or_default();
            info!("creature parts: {} ({base:08X}): {path} plays its own controllers on {on}", c.edid.as_deref().unwrap_or(""));
        }
    }
    if !spawned.part_nodes.is_empty() && first_creature_report(base, "") {
        let names: Vec<&str> = spawned.part_nodes.iter().map(|(n, ..)| n.as_str()).collect();
        info!("creature parts: {} ({base:08X}): part node(s) {} for its clips to drive", c.edid.as_deref().unwrap_or(""), names.join(", "));
    }
    commands.entity(spawned.root).insert((
        Name::new(format!("Creature {reference:08X} {}", c.edid.as_deref().unwrap_or(""))),
        RefVisual(reference),
        ActorRig { reference, base, body: c.body.0, skeleton: c.skeleton.clone(), skel: skel.clone(), nodes: spawned.nodes.clone(), base_scale: c.base_scale as f32 },
    ));
    if let Some(half_width) = skel.bbx_half_width {
        commands.entity(spawned.root).insert(nvf_play::hitvol::CreatureBbx { reference, half_width });
    }
    for e in &spawned.drawn {
        commands.entity(*e).insert(RefVisual(reference));
    }
    if hidden && !world.show_disabled {
        spawn_hidden(commands, spawned.root, reference);
        stats.spawned_hidden += 1;
    }
    if world.pose == ActorPose::Idle {
        let _anims_span = info_span!("creature::anims").entered();
        match world.anims.clip_set(ctx, &c.skeleton, &skel, Body::Male) {
            Some(set) => {
                let mut lo = Locomotion::new(set.clone(), clip_phase(reference));
                lo.idle_under = true;
                // A creature sets off at once from a standing fade (actors.md §8.10), as
                // every actor does (`Locomotion::new`, #3670).
                lo.blend.set_off_at_once = true;
                lo.set_off_across_sets = true;
                lo.hurt = world.anims.clip_set_hurt(ctx, &c.skeleton, &skel, Body::Male, &set, false);
                lo.swim = world.anims.swim_set(ctx, &c.skeleton, &skel, Body::Male, &set);
                // Some creatures move otherwise when alert (`creature_pace.rs`), and limp
                // otherwise when alert too (`limping.rs`).
                lo.alert = world.anims.alert_set(ctx, &c.skeleton, &skel, &set);
                if let Some(alert) = lo.alert.clone() {
                    lo.hurt_alert = world.anims.clip_set_hurt(ctx, &c.skeleton, &skel, Body::Male, &alert, true);
                }
                if let Some(t) = world.pose_time {
                    let len = set.info(Move::Idle).map_or(1.0, |c| c.duration).max(f32::EPSILON);
                    lo.blend = Blend::new(Move::Idle, t / len);
                    lo.hold = true;
                }
                let mut player = blend_player(&spawned, &skel);
                lo.layers(&mut player);
                commands.entity(spawned.root).insert((lo, player));
                stats.creatures_animated += 1;
            }
            None => stats.creature_clip_missing += 1,
        }
    }
    stats.unbound_shapes += spawned.unbound;
    stats.creatures += 1;
    Some(spawned.root)
}

/// Whether `what` (a part's path, or "" for its part nodes) of creature `base` is
/// reported for the first time: the line is written once per creature and part, not for
/// every reference.
fn first_creature_report(base: u32, what: &str) -> bool {
    static SEEN: std::sync::OnceLock<Mutex<std::collections::HashSet<(u32, String)>>> = std::sync::OnceLock::new();
    SEEN.get_or_init(Default::default).lock().is_ok_and(|mut s| s.insert((base, what.to_string())))
}

/// Whether a creature draws `part` as a model playing its own controllers on its `Prn`
/// bone (`shot_fx::spawn_playing_itself`) rather than as still shapes: the part moves by
/// itself ([`nvf_assets::actor::PreparedPart::plays_itself`]) and no shape hangs from a
/// node of the part named as one of the skeleton's (actors.md §10.2), which a model on
/// one bone could not follow [design, #2228: Mister Handy's jet].
fn plays_on_bone(part: &nvf_assets::actor::PreparedPart, skel: &PreparedSkeleton) -> bool {
    part.plays_itself && part.shapes.iter().all(|s| s.ancestors.iter().all(|(n, _)| n.as_ref().is_none_or(|n| !skel.by_name.contains_key(n))))
}

/// The creature's idle clip: `mtidle.kf` beside its skeleton (`locomotion::STANCE`).
fn idle_path(skeleton: &str) -> String {
    let folder = skeleton.rfind(['\\', '/']).map_or("", |i| &skeleton[..i]);
    format!("{folder}\\{}idle.kf", crate::locomotion::STANCE)
}

/// Whether `clip` drives a shape of `part` (a material track naming the shape) or moves
/// one of the part's named nodes (a transform track naming it). Such a part stays a
/// part of the actor, posed and lit by the actor's clips, rather than playing its own
/// controllers on its bone ([`plays_on_bone`]): the street Securitron's screen
/// (`nvsecuritronpolicescreen01.nif`, `Screen01:0`) has looping controllers of its own,
/// and its `mtidle.kf` keys `Screen01` and the shape's glow and texture too; drawn as a
/// playing model it was not reached by the idle any more (#2282, after #2228)
/// [observed: smoke securitron-screen; hypothesis: the idle's tracks are what the
/// original shows on the screen, docs/spec-notes/effects-animation-audit.md §2].
/// Visibility tracks do not count: Mister Handy's `mtidle.kf` shows and hides its jet's
/// `##JetFlames01`/`02` (NiVisController) [observed: `fnvtool nif decode`], and the jet
/// is the part #2228 plays by itself.
fn drives_part(clip: &nvf_assets::anim_play::Clip, part: &nvf_assets::actor::PreparedPart) -> bool {
    let shapes: Vec<&str> = part.shapes.iter().filter_map(|s| s.name.as_deref()).collect();
    let nodes: Vec<&str> = part.shapes.iter().flat_map(|s| s.ancestors.iter().filter_map(|(n, _)| n.as_deref())).collect();
    let material_shapes: Vec<&str> = clip.materials.iter().map(|m| m.shape.as_str()).collect();
    let keyed_nodes: Vec<&str> = clip.tracks.iter().map(|(t, _)| t.as_str()).collect();
    names_driven(&shapes, &nodes, &material_shapes, &keyed_nodes)
}

/// [`drives_part`] by names (case does not matter): a part's shape names and node names
/// against the shapes a clip's material tracks name and the nodes its transform tracks
/// name.
fn names_driven(shapes: &[&str], nodes: &[&str], material_shapes: &[&str], keyed_nodes: &[&str]) -> bool {
    shapes.iter().any(|s| material_shapes.iter().any(|m| m.eq_ignore_ascii_case(s))) || nodes.iter().any(|n| keyed_nodes.iter().any(|k| k.eq_ignore_ascii_case(n)))
}

/// Give an actor its weapon (part H, actors.md §9): the model under the skeleton's
/// `Weapon` node and a [`WeaponHold`] with the stance's clips. An actor that fights with
/// its fists (no weapon, the Fists or another weapon without a model, a weapon that
/// cannot be drawn, or weapons off: the combat code takes an actor with no weapon in its
/// hand as unarmed) holds them: the `h2h` stance without a model (`h2hequip.kf`,
/// `h2haim.kf`, `h2hunequip.kf` in `characters\_male` [observed: the base meshes archive
/// listing]), drawn and holstered as any weapon is [design: as the game's fists look in
/// third person]. Nothing when the skeleton has no such stance.
#[allow(clippy::too_many_arguments)]
pub(crate) fn arm(
    commands: &mut Commands,
    spawned: &nvf_assets::actor::SpawnedActor,
    skel: &nvf_assets::actor::PreparedSkeleton,
    a: &Appearance,
    reference: u32,
    world: &ActorWorld,
    ctx: &mut ActorCtx,
    player: &mut nvf_assets::anim_play::BlendPlayer,
    stats: &mut ActorStats,
) -> Option<WeaponHold> {
    let held = if world.weapon_mode == WeaponMode::None { None } else { arm_weapon(commands, spawned, skel, &a.skeleton, a.weapon, a.weapon_mods, reference, world, ctx, player, stats) };
    held.or_else(|| {
        let stance = world.anims.stance(ctx, &a.skeleton, skel, AnimType::HandToHand.prefix()).filter(|s| !s.no_weapon_node)?;
        let skel_parent = skel.parents[stance.weapon_node].unwrap_or(0);
        let mut hold = WeaponHold::new(stance, world.weapon_mode == WeaponMode::Drawn, skel_parent);
        if let Some(period) = world.weapon_test {
            hold.test = Some((period, clip_phase(reference) * 2.0 * period));
        }
        hold.layers(player);
        stats.fists += 1;
        Some(hold)
    })
}

/// [`arm`] with the actor's weapon model, or nothing when it has none to draw: `weapon`
/// with its mod flags `weapon_mods`, on the skeleton at `skeleton` (archive path; a
/// creature's too, `outfits::arm_creatures`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn arm_weapon(
    commands: &mut Commands,
    spawned: &nvf_assets::actor::SpawnedActor,
    skel: &nvf_assets::actor::PreparedSkeleton,
    skeleton: &str,
    weapon: Option<fnv_sim::FormId>,
    weapon_mods: u8,
    reference: u32,
    world: &ActorWorld,
    ctx: &mut ActorCtx,
    player: &mut nvf_assets::anim_play::BlendPlayer,
    stats: &mut ActorStats,
) -> Option<WeaponHold> {
    let w = weapon.and_then(|id| world.actors.weapons.get(&id))?;
    let prefix = w.anim_type.as_deref().and_then(AnimType::from_name)?.prefix();
    // A modded weapon's own model (weapon-mods.md §3 "In the world"); the player's tweak
    // gives NPCs the first-person (finer) model (docs/design/tweaks.md).
    let fine = nvf_play::tweaks::HI_RES_NPC_WEAPONS.load(std::sync::atomic::Ordering::Relaxed);
    let model = w.model_for(fine, weapon_mods)?;
    // A stance without a `Weapon` node is for body weapons only (`Stance::no_weapon_node`).
    let stance = world.anims.stance(ctx, skeleton, skel, prefix).filter(|s| !s.no_weapon_node);
    // With the weapon's alternate textures (texture-swaps.md §4).
    let swaps = weapon.map_or(&[][..], |id| world.actors.weapon_alt_textures(id, false, weapon_mods));
    let (Some(stance), Some(part)) = (stance, ctx.part_swapped(&fnv_actor::resolve::mesh_path(model), swaps)) else {
        stats.weapon_missing += 1;
        return None;
    };
    let path = fnv_actor::resolve::mesh_path(model);
    // The bones its parts hang from, read with the part on its worker thread (#3057).
    let bones = part.upb.clone();
    let held: Vec<&nvf_assets::anim_play::Clip> = [fnv_actor::weapon::EquipClip::Aim, fnv_actor::weapon::EquipClip::Equip].iter().filter_map(|c| stance.clips.get(c)).map(|c| &*c.clip).collect();
    let shapes = crate::weapons::spawn_model(commands, spawned, skel, stance.weapon_node, &part, &bones, &stance.holstered_off, &held);
    for &e in &shapes.all {
        commands.entity(e).insert(RefVisual(reference));
        if let Some(weapon) = weapon {
            commands.entity(e).insert(crate::weapons::HeldWeapon { reference, weapon });
        }
    }
    // Where its shots leave (`npc_combat`): the part's `ProjectileNode`.
    crate::weapons::spawn_muzzle(commands, spawned, stance.weapon_node, &part, reference);
    let skel_parent = skel.parents[stance.weapon_node].unwrap_or(0);
    let mut hold = WeaponHold::new(stance, world.weapon_mode == WeaponMode::Drawn, skel_parent);
    if !shapes.back.is_empty() || !shapes.gun.is_empty() {
        info!("actor weapons: {reference:08X} {path}: {} shape(s) on the back, {} gun shape(s) the clips show and hide", shapes.back.len(), shapes.gun.len());
    }
    // The weapon's own hand models (WEAP BIPL: the power fist's metal hand), drawn as
    // worn parts in place of the bare hand (`fnv_actor::Resolver::resolve_wearing`).
    if !w.addons.is_empty() {
        let models: Vec<&str> = w.addons.iter().filter_map(|a| world.actors.armors.get(a)).filter_map(|a| a.worn[0].as_deref()).collect();
        info!("actor weapons: {reference:08X} {path}: worn with its hand model(s) {models:?} in place of the bare hand");
    }
    hold.visibility = shapes.visibility;
    hold.back = shapes.back;
    if !shapes.posed.is_empty() {
        info!("actor weapons: {reference:08X} {path}: {} shape(s) under parts the aim or draw clip moves", shapes.posed.len());
    }
    hold.parts = shapes.posed;
    if let Some(period) = world.weapon_test {
        hold.test = Some((period, clip_phase(reference) * 2.0 * period));
    }
    if let Some(t) = world.pose_time {
        // A repeatable pose: the test aid runs from its start to `t`; otherwise the stage's
        // clip is held at `t`.
        match hold.test.as_mut() {
            Some(test) => {
                test.1 = 0.0;
                hold.simulate(t);
            }
            None => hold.anim.time = t,
        }
        hold.hold = true;
    }
    hold.layers(player);
    stats.armed += 1;
    Some(hold)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_part_the_idle_drives_stays_a_part() {
        assert_eq!(idle_path(r"meshes\creatures\nvsecuritron\skeleton.nif"), r"meshes\creatures\nvsecuritron\mtidle.kf");
        // The street Securitron's screen: mtidle.kf keys the screen's glow and its node.
        assert!(names_driven(&["Screen01:0"], &["Screen01"], &["screen01:0", "ScreenStatic:0"], &["Bip01 Wheel"]));
        assert!(names_driven(&["Screen01:0"], &["Screen01"], &[], &["screen01"]));
        // Mister Handy's jet: its idle moves none of the jet's nodes and lights none of
        // its shapes (it only shows and hides `##JetFlames01`, not counted; #2228).
        assert!(!names_driven(&["JetFlame01"], &["##JetFlames01"], &[], &["Bip01", "Bip01 Spine 01"]));
    }

    #[test]
    fn phases_spread_within_the_clip() {
        let p: Vec<f32> = (0..64u32).map(|r| clip_phase(0x0010_4C0C + r)).collect();
        assert!(p.iter().all(|x| (0.0..1.0).contains(x)));
        assert!(p.iter().any(|&x| x < 0.25) && p.iter().any(|&x| x > 0.75));
    }
}
