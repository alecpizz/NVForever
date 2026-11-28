//! Trigger volumes for scripts (layer5.md §3.4: OnTriggerEnter / OnTrigger /
//! OnTriggerLeave come from the engine, workstream 5D). A trigger is a scripted
//! reference in a loaded cell whose script has one of those blocks and which carries a
//! placed primitive (`XPRM`, box or sphere): FalloutNV's VCG01VigorTesterTrigger
//! 0x00104C16 is a 216 × 93 × 100 box (half extents) whose OnTriggerEnter sets VCG01
//! stage 60 [observed: `fnvtool esm decode`, `fnvtool script decompile`].
//!
//! A scripted reference whose *model* holds trigger volumes counts too: phantoms
//! (`bhkSimpleShapePhantom`) on the Trigger layer (12), as in the bear trap, the
//! tripwire and the pressure plate (`meshes\traps\beartrap01.nif` node "Trigger": a
//! box 32 × 32 × 8 units half extents) [observed: `fnvtool nif decode`], and phantoms on
//! the Gas Trap layer (24), the gas clouds (`meshes\traps\trapgasfire01.nif`: one
//! sphere, nothing solid [observed]; their scripts set them off when an actor walks in
//! with a flamer, an incinerator or a Shishkebab drawn, or burning; shots and blasts:
//! `crate::gas_traps`, #2687). Each phantom
//! is the box of its shape's bounds in the model, scaled, turned and moved by the
//! reference [hypothesis: the box shapes under a transform shape are what Havok tests;
//! docs/spec-notes/traps.md §1]. The model is the one the physics cell loader read
//! ([`crate::phys::PhysState::model_triggers`]); until it has, the reference is asked
//! again on the next frame. The volumes follow the reference: when a script moves, turns
//! or rescales it (MoveTo, SetPos, SetAngle, SetScale), they are built again at its new
//! pose [design, #1091].
//!
//! Physics does not hold these volumes (`fnv_phys` loads primitives of CollisionMarker
//! references only, as solid collision); the test here is geometric, each frame:
//! - **Who**: every actor in the loaded cells, not only the player [documented: any actor
//!   fires a trigger, a block parameter such as `Begin OnTriggerEnter player` limits it to
//!   that reference; GECK tutorials, docs/spec-notes/level-design.md §11.2 T1]. The
//!   actors are the player and the placed NPCs and creatures (ACHR, ACRE) of the loaded
//!   cells that are present and alive, at their physics body's feet (else the
//!   simulation's position, else their placed one). The runtime matches the
//!   block parameter against the subject, which is the actor; the action reference is
//!   the actor too. Non-actors (thrown items, projectiles) do not fire them here
//!   [hypothesis for the default; the ActorZone / Projectile flags are not read, T5].
//! - **How**: the actor's collision capsule against the volume ([`Capsule`],
//!   [`crate::script_events::TriggerShape::touches`]) [documented: a collision check of
//!   the object against the volume, T2; hypothesis: the capsule sizes, the player's
//!   physics capsule and `fnv_phys::CharacterParams::default()` × the reference's scale
//!   now (GetScale's, SetScale's included) for other actors].
//! - **When**: OnTriggerEnter once on the frame an actor first touches the volume, then
//!   OnTrigger every frame it touches it (a frame's OnTrigger runs are the script's
//!   per-frame run for GetSecondsPassed, `Game::trigger_frame`), **the entering frame included, after
//!   OnTriggerEnter** [documented: OnTrigger every frame inside, T3; hypothesis: the order
//!   on the entering frame, open in game, level-design.md §13 question 2]; OnTriggerLeave
//!   once on the first frame it no longer does. A trigger that is disabled is left
//!   without a Leave; an actor that is disabled, dies or leaves the loaded cells leaves
//!   its triggers with one [hypothesis]. A trigger whose cell unloads is left without a
//!   Leave by the actors still in it (they stay inside, the cell only went out of reach),
//!   but **the player** leaves it with one: the cell unloads because the player went
//!   away (fast travel, a door, a script's MoveTo) [design, #2449: the Boomers'
//!   NellisArtilleryScript counts the actors in its field in and out, and a player who
//!   fast-travelled out of the field stayed counted, so the barrage never stopped after
//!   the Boomers took the player in; [`leave_reported`]].

use crate::script::Runtime;
use crate::script_events::{trigger_changes, Capsule, TriggerKind, TriggerShape};
use crate::phys::PhysState;
use crate::PlayWorld;
use fnv_game::game::event;
use fnv_phys::CharacterParams;
use fnv_script::vm::Value;
use fnv_sim::FormId;
use fnv_world::transform::ref_rotation;
use std::collections::{BTreeSet, HashMap};

#[derive(Default)]
pub struct Triggers {
    /// Actors that entered a trigger so far (each OnTriggerEnter), for the game digest test
    /// aid (nvforever game_digest.rs).
    pub entered: u64,
    /// The volumes of each candidate, once read (empty: no usable primitive or model
    /// trigger), with the pose they were built at: a reference a script moves, turns
    /// or rescales gets them built again at its new pose (#1091).
    shapes: HashMap<FormId, (Pose, Vec<TriggerShape>)>,
    /// Candidates whose model had not been read yet.
    waiting: HashMap<FormId, Lookup>,
    /// When each candidate first waited for its model (game time): its script's
    /// GameMode waits too, for at most [`HOLD_SECS`] (`Game::scripts_held`).
    waiting_since: HashMap<FormId, f64>,
    /// Scripts with a trigger block, by SCPT.
    has_blocks: HashMap<FormId, bool>,
    /// Scripts with an OnHit / OnHitWith block, by SCPT (#3662).
    has_hit_blocks: HashMap<FormId, bool>,
    /// Actor references (ACHR, ACRE) placed in each cell, once listed.
    actors_by_cell: HashMap<FormId, Vec<FormId>>,
    /// (trigger, actor) pairs touching after the last frame.
    pub inside: BTreeSet<(FormId, FormId)>,
    /// Of the candidates' volumes, those from Gas Trap-layer phantoms: the gas clouds
    /// shots, blasts and guns fired inside set off (`crate::gas_traps`, #2687). Built
    /// and dropped with [`Triggers::shapes`].
    gas: HashMap<FormId, Vec<TriggerShape>>,
    /// Scripted non-actor references in loaded cells checked for a Projectile Zone primitive
    /// (`XPRM` with `XTRI 23`, `fnv_phys::layers::PROJECTILE_ZONE`) (#3662): `Zone` holds
    /// the decoded primitive kind and world-space volume at its last pose, `NotZone` records
    /// that the reference is not on layer 23 keyed by reference alone so pose changes never
    /// re-decode its record. Pruned when cells unload.
    projectile_zones: HashMap<FormId, ProjectileZoneEntry>,
    /// A co-op guest's game: its story triggers are held ([`Triggers::held_for_guest`]).
    guest: bool,
    /// This game has been a co-op guest's (`Game::coop_guest` set) since it joined.
    was_guest: bool,
    /// Candidates whose volumes come from their model's trigger shapes (traps: bear
    /// traps, tripwires, pressure plates), not from a placed primitive.
    from_model: std::collections::HashSet<FormId>,
    /// Number of `XPRM` decodes performed for `projectile_zones` (test aid).
    #[cfg(test)]
    projectile_zone_decodes: u64,
}

/// Cached Projectile Zone (`XTRI 23`) lookup state for a scripted non-actor reference (#3662).
enum ProjectileZoneEntry {
    /// Reference is not on layer 23 (no `XPRM`, other layer, or unsupported primitive kind);
    /// keyed by reference so pose changes never re-decode its record.
    NotZone,
    /// Reference has a layer-23 primitive; keeps the decoded record primitive kind so pose
    /// or script-bound updates rebuild the world-space volume without re-decoding `XPRM`.
    Zone {
        record_kind: TriggerKind,
        pose: Pose,
        shape: TriggerShape,
    },
}

impl ProjectileZoneEntry {
    fn shape(&self) -> Option<&TriggerShape> {
        match self {
            Self::NotZone => None,
            Self::Zone { shape, .. } => Some(shape),
        }
    }
}

impl Triggers {
    /// Whether this frame's story triggers are held because this game is a co-op guest's
    /// (`fnv_game::Game::coop_guest`). The host's world runs the world's scripts: its
    /// stand-in for the guest is in the host's world there. Held are the placed volumes
    /// (XPRM primitives, the level designers' story triggers); a trap's trigger from its
    /// model still fires here for the guest's own player only, so a guest who steps on a
    /// bear trap is hit [design]; the guest's NPCs follow the host's and fire nothing. A
    /// guest's own copy of a trigger running for its own player locked every control at
    /// Old World Blues' slide show (`DisablePlayerControls` with no quest check, and the
    /// EnablePlayerControls that ends it only comes from the quest the guest's game never
    /// runs) and killed the guest's own Jed at Honest Hearts' ambush, which then showed
    /// "Honest Hearts can't be accessed" [observed: buckallobill, in game, NVForever
    /// co-op]. Nothing is entered, run or left while held, and what was inside is
    /// forgotten, so a game that stops being a guest enters what it stands in afresh (as
    /// after a load).
    ///
    /// `joining`: the host welcomed this game as a guest (`crate::save::KeepSolo`). The
    /// game is flagged a guest only once its own game was kept (`crate::save::SoloBase`),
    /// a moment after the host's world moved its player: before, its player entered its
    /// own copy of a story trigger on that first frame [observed: buckallobill, in game,
    /// NVForever co-op, two windows]. So it is held from the welcome until the flag first
    /// comes, and after that by the flag alone (a game that leaves the host's world runs
    /// its triggers again). Outside co-op both are false and nothing is held.
    pub fn held_for_guest(&mut self, guest: bool, joining: bool) -> bool {
        self.was_guest |= guest;
        let guest = guest || (joining && !self.was_guest);
        if guest != self.guest {
            self.guest = guest;
            if guest {
                bevy::log::info!("script: triggers held: a co-op guest's world is the host's, whose game runs them ({} inside forgotten)", self.inside.len());
            } else {
                bevy::log::info!("script: triggers run again: no longer a co-op guest");
            }
            self.inside.clear();
        }
        guest
    }

    /// Every trigger volume built so far, at the pose it was built at (the collision
    /// view draws them, `crate::collision_view`).
    pub fn volumes(&self) -> impl Iterator<Item = &TriggerShape> {
        self.shapes.values().flat_map(|(_, v)| v.iter()).chain(self.projectile_zones.values().filter_map(ProjectileZoneEntry::shape))
    }

    /// A trigger reference's volumes, once built.
    pub fn shapes_of(&self, r: FormId) -> Option<&[TriggerShape]> {
        self.shapes.get(&r).map(|(_, v)| v.as_slice())
    }

    /// The gas clouds' volumes built so far, by reference (`crate::gas_traps`); a
    /// reference disabled since is still listed: the caller asks whether it is present.
    pub fn gas_volumes(&self) -> impl Iterator<Item = (FormId, &TriggerShape)> {
        self.gas.iter().flat_map(|(r, v)| v.iter().map(move |s| (*r, s)))
    }

    /// The Projectile Zone (`XTRI 23`) primitive volumes built so far, by reference
    /// (#3662); a reference disabled since is still listed: the caller checks
    /// `Game::present` and `loaded_cells`.
    pub fn projectile_zone_volumes(&self) -> impl Iterator<Item = (FormId, &TriggerShape)> {
        self.projectile_zones.iter().filter_map(|(r, e)| e.shape().map(|s| (*r, s)))
    }

    /// Drop cached Projectile Zone entries whose references are no longer in any loaded
    /// cell (when cells unload or a reference moves to an unloaded cell) (#3662).
    pub(crate) fn prune_unloaded_projectile_zones(&mut self, game: &fnv_game::Game) {
        self.projectile_zones.retain(|r, _| game.cell_of(*r).is_some_and(|c| game.loaded_cells.contains(&c)));
    }
}

/// How long a trigger's script waits for its model's volumes before its GameMode runs
/// without them (a model the cell loader never reads would otherwise hold it for good)
/// [engineering choice; the loader reads a cell's models within a few frames].
const HOLD_SECS: f64 = 2.0;

const TRIGGER_BLOCKS: [u16; 3] = [event::ON_TRIGGER_ENTER, event::ON_TRIGGER, event::ON_TRIGGER_LEAVE];
const HIT_BLOCKS: [u16; 2] = [event::ON_HIT, event::ON_HIT_WITH];

fn is_actor(sig: &[u8; 4]) -> bool {
    sig == b"ACHR" || sig == b"ACRE"
}

/// A reference's pose as its trigger volumes take it: position, rotation (the reference's
/// angles) and scale.
type Pose = ([f32; 3], [f32; 3], f32);

/// `r`'s pose now: its state's position and rotation (else its placement's) and its
/// scale as GetScale reads it (SetScale's, else the placement's).
fn pose_of(rt: &Runtime, r: FormId) -> Option<Pose> {
    let placed = rt.game.world.placed.get(r)?;
    let (pos, rot) = rt.game.world.peek(r).map_or((placed.pos, placed.rot), |s| (s.pos, s.rot));
    Some((pos, rot, fnv_game::functions::scale_of(&rt.game, r) as f32))
}

/// Where a candidate's volumes come from, once looked up.
enum Lookup {
    /// Its volumes, at its pose when read.
    Shapes(Vec<TriggerShape>),
    /// Its model's trigger volumes, at this pose (position, rotation, scale), once the
    /// model has been read.
    Model(String, [f32; 3], [f32; 9], f32, Pose),
}

/// The trigger volumes of a placed reference at its current pose: its primitive (XPRM),
/// else the Trigger-layer phantoms of its model.
fn lookup(play: &PlayWorld, rt: &Runtime, r: FormId, pose: Pose) -> Lookup {
    let Some(placed) = rt.game.world.placed.get(r) else { return Lookup::Shapes(Vec::new()) };
    let (pos, rot, scale) = pose;
    let rot = ref_rotation(rot);
    // AddPrimitive's and SetPrimitiveBound's (`fnv_game::nvse_jip14`) before the record's.
    if let Some((sphere, b)) = rt.game.script_primitive(r) {
        let kind = if sphere { TriggerKind::Sphere { radius: b[0].abs() } } else { TriggerKind::Box { half_extents: b.map(f32::abs) } };
        return Lookup::Shapes(vec![TriggerShape { pos, rot, scale, kind }]);
    }
    if let Some(p) = fnv_phys::primitive::primitive(play.lo, play.schema, r.0) {
        let kind = match p.kind {
            fnv_phys::primitive::PrimitiveKind::Box => TriggerKind::Box { half_extents: p.bounds.map(f32::abs) },
            fnv_phys::primitive::PrimitiveKind::Sphere => TriggerKind::Sphere { radius: p.bounds[0].abs() },
            _ => return Lookup::Shapes(Vec::new()),
        };
        return Lookup::Shapes(vec![TriggerShape { pos, rot, scale, kind }]);
    }
    match fnv_world::placed::base_object(play.lo, play.schema, placed.base.0).and_then(|b| b.world_model_path()) {
        Some(path) => Lookup::Model(path, pos, rot, scale, pose),
        None => Lookup::Shapes(Vec::new()),
    }
}

/// Each trigger object of a model at the reference's pose: the bounds of its shape in
/// the model, centred where they are, turned and scaled with the reference; a box, or a
/// sphere for a sphere shape (the gas clouds', #2687).
pub fn model_shapes(objects: &[fnv_collide::CollisionObject], pos: [f32; 3], rot: [f32; 9], scale: f32) -> Vec<TriggerShape> {
    objects
        .iter()
        .filter_map(|o| {
            let b = o.root_bounds();
            if !b.is_valid() {
                return None;
            }
            let c = b.center().map(|v| v * scale);
            let rc = [rot[0] * c[0] + rot[1] * c[1] + rot[2] * c[2], rot[3] * c[0] + rot[4] * c[1] + rot[5] * c[2], rot[6] * c[0] + rot[7] * c[1] + rot[8] * c[2]];
            let half = b.extent().map(|v| v / 2.0);
            let kind = match *o.shape {
                fnv_collide::Shape::Sphere { .. } => TriggerKind::Sphere { radius: half[0].min(half[1]).min(half[2]) },
                _ => TriggerKind::Box { half_extents: half },
            };
            Some(TriggerShape { pos: [pos[0] + rc[0], pos[1] + rc[1], pos[2] + rc[2]], rot, scale, kind })
        })
        .collect()
}

/// The actors that can fire triggers now, with their capsules: the player (its physics
/// capsule, `player`) and the present, living NPCs and creatures of the loaded cells.
fn actors(rt: &mut Runtime, phys: &PhysState, player: Capsule) -> Vec<(FormId, Capsule)> {
    let placed = rt.game.world.placed;
    let mut ids: BTreeSet<FormId> = BTreeSet::new();
    for c in &rt.game.loaded_cells {
        let list = rt.triggers.actors_by_cell.entry(*c).or_insert_with(|| {
            placed.by_cell.get(c).map_or_else(Vec::new, |v| v.iter().copied().filter(|r| placed.get(*r).is_some_and(|p| is_actor(&p.sig))).collect())
        });
        ids.extend(list.iter().copied());
    }
    let g = &rt.game;
    // Actors a script moved in from elsewhere.
    ids.extend(g.moved.iter().copied().filter(|r| placed.get(*r).is_some_and(|p| is_actor(&p.sig))));
    let npc = CharacterParams::default();
    let mut out = vec![(FormId::PLAYER_REF, player)];
    for r in ids {
        if !g.present(r) || g.extra.get(&r).is_some_and(|e| e.dead) || !g.cell_of(r).is_some_and(|c| g.loaded_cells.contains(&c)) {
            continue;
        }
        let Some(p) = placed.get(r) else { continue };
        // The live feet of its physics body, as the player's capsule; else its state's
        // position. Sized by its scale now, SetScale's included (#1091).
        let feet = phys.npcs.get(&r.0).map(|b| b.ch.feet).unwrap_or_else(|| g.world.peek(r).map_or(p.pos, |s| s.pos));
        let k = (fnv_game::functions::scale_of(g, r) as f32).max(0.0);
        out.push((r, Capsule { feet, radius: npc.radius * k, height: npc.height * k }));
    }
    out
}

/// Decode a reference's Projectile Zone primitive (`XPRM` with `XTRI == PROJECTILE_ZONE`
/// (23)) from the load order (#3662) [documented: xEdit names layer 23 Projectile Zone;
/// GECK wiki Creating Primitives: the Projectile flag is "Used for triggers which can be
/// activated by a projectile"; hypothesis: XTRI 23 stores that flag]. Non-layer-23 or
/// unsupported primitives return `None`.
fn projectile_zone_record_kind(play: &PlayWorld, r: FormId) -> Option<TriggerKind> {
    let p = fnv_phys::primitive::primitive(play.lo, play.schema, r.0)?;
    if p.layer != Some(u32::from(fnv_phys::layers::PROJECTILE_ZONE)) {
        return None;
    }
    match p.kind {
        fnv_phys::primitive::PrimitiveKind::Box => Some(TriggerKind::Box { half_extents: p.bounds.map(f32::abs) }),
        fnv_phys::primitive::PrimitiveKind::Sphere => Some(TriggerKind::Sphere { radius: p.bounds[0].abs() }),
        _ => None,
    }
}

/// Script-overridden primitive bounds (`AddPrimitive` / `SetPrimitiveBound`), if any.
fn script_primitive_kind(game: &fnv_game::Game, r: FormId) -> Option<TriggerKind> {
    game.script_primitive(r).map(|(sphere, b)| {
        if sphere {
            TriggerKind::Sphere { radius: b[0].abs() }
        } else {
            TriggerKind::Box { half_extents: b.map(f32::abs) }
        }
    })
}

/// The Projectile Zone (`XTRI 23`) volumes that shots can cross now: present and in a
/// loaded cell (#3662). Call with the world in the runtime (`crate::script::with_world`).
pub fn projectile_zones(rt: &Runtime) -> Vec<(FormId, TriggerShape)> {
    let g = &rt.game;
    rt.triggers
        .projectile_zone_volumes()
        .filter(|(r, _)| g.present(*r) && g.cell_of(*r).is_some_and(|c| g.loaded_cells.contains(&c)))
        .map(|(r, s)| (r, *s))
        .collect()
}

/// The Projectile Zone volumes crossed by shot segment `from`–`to`, ordered nearest to
/// `from` along the shot direction (ties broken by `FormId`), each at most once (#3662).
pub fn crossed_projectile_zones(vols: &[(FormId, TriggerShape)], from: [f32; 3], to: [f32; 3]) -> Vec<FormId> {
    let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let mut hits: Vec<(FormId, f32)> = vols
        .iter()
        .filter(|(_, s)| s.crossed_by(from, to))
        .map(|(r, s)| {
            let v = [s.pos[0] - from[0], s.pos[1] - from[1], s.pos[2] - from[2]];
            (*r, v[0] * d[0] + v[1] * d[1] + v[2] * d[2])
        })
        .collect();
    hits.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.0.cmp(&b.0.0)));
    let mut out = Vec::with_capacity(hits.len());
    for (r, _) in hits {
        if !out.contains(&r) {
            out.push(r);
        }
    }
    out
}

/// Test the actors against the triggers of the loaded cells and run the blocks. `player`
/// is the player's capsule; `None` when the player has no position yet: the volumes are
/// looked up (and their scripts held while they wait) but nobody enters or leaves.
pub fn update(rt: &mut Runtime, play: &PlayWorld, phys: &PhysState, player: Option<Capsule>, joining: bool) {
    let guest = rt.triggers.held_for_guest(rt.game.coop_guest, joining);
    let data = rt.game.data;
    let mut candidates: Vec<FormId> = Vec::new();
    let mut hit_candidates: Vec<FormId> = Vec::new();
    for c in &rt.game.loaded_cells {
        for &r in data.scripted_by_cell.get(c).map_or(&[][..], |v| v.as_slice()) {
            let Some(placed) = rt.game.world.placed.get(r) else { continue };
            let Some(script) = data.script_of_base(placed.base) else { continue };
            let has = *rt.triggers.has_blocks.entry(script).or_insert_with(|| data.script(script).is_some_and(|u| TRIGGER_BLOCKS.iter().any(|&b| u.has_block(b))));
            if has {
                candidates.push(r);
            }
            // Only non-actor references (`REFR`) can carry `XPRM` primitives; skip
            // `ACHR`/`ACRE` up front so moving scripted actors are never probed (#3662).
            if !is_actor(&placed.sig) {
                let has_hit = *rt.triggers.has_hit_blocks.entry(script).or_insert_with(|| data.script(script).is_some_and(|u| HIT_BLOCKS.iter().any(|&b| u.has_block(b))));
                if has_hit {
                    hit_candidates.push(r);
                }
            }
        }
    }
    let mut shapes = Vec::new();
    for r in candidates {
        if !rt.game.present(r) {
            continue;
        }
        let Some(pose) = pose_of(rt, r) else { continue };
        // Moved, turned or rescaled since its volumes were built: build them again.
        if rt.triggers.shapes.get(&r).is_some_and(|(at, _)| *at != pose) {
            rt.triggers.shapes.remove(&r);
            rt.triggers.gas.remove(&r);
            bevy::log::info!("script: trigger {} moved: its volumes follow", rt.game.data.name(r));
        }
        if rt.triggers.waiting.get(&r).is_some_and(|l| matches!(l, Lookup::Model(.., at) if *at != pose)) {
            rt.triggers.waiting.remove(&r);
        }
        if guest && !rt.triggers.from_model.contains(&r) && rt.triggers.shapes.contains_key(&r) {
            continue;
        }
        if !rt.triggers.shapes.contains_key(&r) {
            let l = match rt.triggers.waiting.remove(&r) {
                Some(l) => l,
                None => lookup(play, rt, r, pose),
            };
            match l {
                Lookup::Shapes(s) => {
                    rt.triggers.from_model.remove(&r);
                    rt.triggers.shapes.insert(r, (pose, s));
                    if guest {
                        continue;
                    }
                }
                Lookup::Model(path, pos, rot, scale, at) => match phys.model_triggers(&path) {
                    Some(objects) => {
                        let s = model_shapes(&objects, pos, rot, scale);
                        if !s.is_empty() {
                            bevy::log::info!("script: trigger {} has {} trigger volume(s) in its model", rt.game.data.name(r), s.len());
                        }
                        rt.triggers.from_model.insert(r);
                        let gas: Vec<_> = objects.iter().filter(|o| o.layer == fnv_phys::layers::GAS_TRAP).cloned().collect();
                        let gas = model_shapes(&gas, pos, rot, scale);
                        if !gas.is_empty() {
                            bevy::log::info!("script: {} has {} gas volume(s) in its model", rt.game.data.name(r), gas.len());
                            rt.triggers.gas.insert(r, gas);
                        }
                        rt.triggers.shapes.insert(r, (at, s));
                    }
                    // The model is not read yet: ask again on the next frame.
                    None => {
                        rt.triggers.waiting.insert(r, Lookup::Model(path, pos, rot, scale, at));
                        let now = rt.game.time;
                        rt.triggers.waiting_since.entry(r).or_insert_with(|| {
                            bevy::log::info!("script: trigger {} waits for its model: its script waits too", rt.game.data.name(r));
                            now
                        });
                        continue;
                    }
                },
            }
            rt.triggers.waiting_since.remove(&r);
        }
        shapes.extend(rt.triggers.shapes[&r].1.iter().map(|s| (r, *s)));
    }
    rt.triggers.prune_unloaded_projectile_zones(&rt.game);
    for r in hit_candidates {
        if !rt.game.present(r) {
            continue;
        }
        let Some(pose) = pose_of(rt, r) else { continue };
        match rt.triggers.projectile_zones.get_mut(&r) {
            Some(ProjectileZoneEntry::NotZone) => {}
            Some(ProjectileZoneEntry::Zone { record_kind, pose: at, shape }) => {
                let kind = script_primitive_kind(&rt.game, r).unwrap_or(*record_kind);
                if *at != pose || shape.kind != kind {
                    *at = pose;
                    *shape = TriggerShape { pos: pose.0, rot: ref_rotation(pose.1), scale: pose.2, kind };
                }
            }
            None => {
                #[cfg(test)]
                {
                    rt.triggers.projectile_zone_decodes += 1;
                }
                let entry = match projectile_zone_record_kind(play, r) {
                    Some(record_kind) => {
                        bevy::log::info!("script: {} has a Projectile Zone primitive (XTRI 23)", rt.game.data.name(r));
                        let kind = script_primitive_kind(&rt.game, r).unwrap_or(record_kind);
                        let shape = TriggerShape { pos: pose.0, rot: ref_rotation(pose.1), scale: pose.2, kind };
                        ProjectileZoneEntry::Zone { record_kind, pose, shape }
                    }
                    None => ProjectileZoneEntry::NotZone,
                };
                rt.triggers.projectile_zones.insert(r, entry);
            }
        }
    }
    // The triggers still waiting for their volumes hold their scripts' GameMode, so an
    // actor standing in one at the start enters it first (`Game::scripts_held`, #2571).
    let time = rt.game.time;
    rt.triggers.waiting_since.retain(|r, _| rt.triggers.waiting.contains_key(r));
    rt.game.scripts_held = held(&rt.triggers.waiting_since, time);
    let Some(player) = player else { return };
    let mut now = BTreeSet::new();
    if !shapes.is_empty() {
        // A co-op guest's game: only its own player, in the traps' volumes (above).
        let who = if guest { vec![(FormId::PLAYER_REF, player)] } else { actors(rt, phys, player) };
        for (a, cap) in who {
            for (t, s) in &shapes {
                if s.touches(&cap) {
                    now.insert((*t, a));
                }
            }
        }
    }
    let ch = trigger_changes(&rt.triggers.inside, &now);
    let g = &mut rt.game;
    let who = |g: &fnv_game::Game, a: FormId| if a == FormId::PLAYER_REF { "player".to_string() } else { g.data.name(a) };
    rt.triggers.entered += ch.entered.len() as u64;
    for &(t, a) in &ch.entered {
        bevy::log::info!("script: {} entered trigger {}", who(g, a), g.data.name(t));
        g.ref_event(t, event::ON_TRIGGER_ENTER, Some(Value::Ref(a.0)), Some(a));
    }
    // Each trigger's OnTrigger is its script's run for this frame, once however many
    // actors are inside (`Game::trigger_frame`: GetSecondsPassed counts from it).
    let running: BTreeSet<FormId> = ch.inside.iter().map(|&(t, _)| t).collect();
    for &t in &running {
        g.trigger_frame(t);
    }
    // The runs for the second and later actors inside one trigger come 0 s after its
    // first (`Game::trigger_again`, issue #2455).
    let mut ran = BTreeSet::new();
    for &(t, a) in &ch.inside {
        if !ran.insert(t) {
            g.trigger_again(t);
        }
        g.ref_event(t, event::ON_TRIGGER, Some(Value::Ref(a.0)), Some(a));
    }
    for &(t, a) in &ch.left {
        let loaded = g.cell_of(t).is_some_and(|c| g.loaded_cells.contains(&c));
        if leave_reported(a, g.present(t), loaded) {
            bevy::log::info!("script: {} left trigger {}", who(g, a), g.data.name(t));
            g.ref_event(t, event::ON_TRIGGER_LEAVE, Some(Value::Ref(a.0)), Some(a));
        }
    }
    rt.triggers.inside = now;
}

/// The triggers whose scripts wait for their volumes at game time `now`: those that
/// began waiting less than [`HOLD_SECS`] ago.
fn held(since: &HashMap<FormId, f64>, now: f64) -> BTreeSet<FormId> {
    since.iter().filter(|(_, &t)| now - t < HOLD_SECS).map(|(r, _)| *r).collect()
}

/// Whether `actor` leaving trigger `t` runs its OnTriggerLeave (module notes): not when
/// the trigger was disabled or deleted (`present` false); when the trigger's cell is no
/// longer loaded, only for the player, who is the one that went away (#2449).
fn leave_reported(actor: FormId, present: bool, loaded: bool) -> bool {
    present && (loaded || actor == FormId::PLAYER_REF)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A co-op guest's game runs no trigger blocks: held while a guest, with
    /// what was inside forgotten, and run again once it is not.
    #[test]
    fn a_guests_triggers_are_held() {
        let mut t = Triggers::default();
        t.inside.insert((FormId(0x0300_326F), FormId::PLAYER_REF));
        assert!(!t.held_for_guest(false, false));
        assert_eq!(t.inside.len(), 1, "solo: as before");
        // Welcomed, not flagged a guest yet: held already.
        assert!(t.held_for_guest(false, true));
        assert!(t.inside.is_empty(), "forgotten: no Leave runs for them");
        t.inside.insert((FormId(0x0200_A7CC), FormId::PLAYER_REF));
        assert!(t.held_for_guest(true, true));
        // Left the host's world: the welcome stays, the flag goes, triggers run again.
        assert!(!t.held_for_guest(false, true));
        assert!(t.inside.is_empty(), "entered afresh after");
    }

    /// #2449: the player fast-travelling out of the Boomers' artillery field (its cell
    /// unloads) leaves the trigger with an OnTriggerLeave, so the script counts the
    /// player out; an NPC left behind in an unloaded cell is still inside; a disabled
    /// trigger reports no leave.
    /// #2571: a trigger whose model is not read holds its script's GameMode, for at
    /// most HOLD_SECS of game time.
    #[test]
    fn a_trigger_waiting_for_its_model_holds_its_script_for_a_while() {
        let (a, b) = (FormId(0x000B_AF39), FormId(0x0010_0000));
        let since = HashMap::from([(a, 10.0), (b, 11.5)]);
        assert_eq!(held(&since, 10.0), BTreeSet::from([a, b]));
        assert_eq!(held(&since, 12.5), BTreeSet::from([b]), "a waited 2.5 s: its script runs");
        assert!(held(&since, 14.0).is_empty());
    }

    #[test]
    fn the_player_leaves_a_trigger_whose_cell_unloads() {
        let npc = FormId(0x0010_0000);
        assert!(leave_reported(FormId::PLAYER_REF, true, true));
        assert!(leave_reported(npc, true, true));
        assert!(leave_reported(FormId::PLAYER_REF, true, false), "the player went away: the leave runs");
        assert!(!leave_reported(npc, true, false), "an NPC in an unloaded cell stays inside");
        assert!(!leave_reported(FormId::PLAYER_REF, false, true), "a disabled trigger reports none");
        assert!(!leave_reported(FormId::PLAYER_REF, false, false));
    }

    mod projectile_zone_cache_tests {
        use super::*;
        use crate::script::{with_world, ScriptState};
        use crate::sim::SimState;
        use fnv_esm::raw::{write_subrecords, Subrecord};
        use fnv_sim::{SimConfig, World};

        const CELL_A: FormId = FormId(0xA00);
        const CELL_B: FormId = FormId(0xA01);
        const SCPT_HIT: FormId = FormId(0xA10);
        const SCPT_HIT_WITH: FormId = FormId(0xA11);
        const ACTI_BASE: FormId = FormId(0xA20);
        const NPC_BASE: FormId = FormId(0xA21);
        const CREA_BASE: FormId = FormId(0xA22);
        const ZONE_REF: FormId = FormId(0xA30);
        const OTHER_LAYER_REF: FormId = FormId(0xA31);
        const PLAIN_REFR: FormId = FormId(0xA32);
        const NPC_REF: FormId = FormId(0xA33);
        const CREA_REF: FormId = FormId(0xA34);

        fn sub(sig: &[u8; 4], data: Vec<u8>) -> Subrecord {
            Subrecord { sig: *sig, data, extended: false }
        }
        fn z(s: &str) -> Vec<u8> {
            [s.as_bytes(), &[0]].concat()
        }
        fn record(sig: &[u8; 4], flags: u32, fid: u32, subs: &[Subrecord]) -> Vec<u8> {
            let payload = write_subrecords(subs);
            [&sig[..], &(payload.len() as u32).to_le_bytes(), &flags.to_le_bytes(), &fid.to_le_bytes(), &[0; 4], &15u16.to_le_bytes(), &[0; 2], &payload].concat()
        }
        fn grup(label: [u8; 4], gtype: i32, body: &[u8]) -> Vec<u8> {
            [&b"GRUP"[..], &((24 + body.len()) as u32).to_le_bytes(), &label, &gtype.to_le_bytes(), &[0; 8], body].concat()
        }
        fn scpt_event(id: FormId, edid: &str, ev: u16) -> Vec<u8> {
            let st = |op: u16, body: &[u8]| [op.to_le_bytes().to_vec(), (body.len() as u16).to_le_bytes().to_vec(), body.to_vec()].concat();
            let end = st(0x11, &[]);
            let mut scda = st(0x1D, &[]);
            scda.extend(st(0x10, &[ev.to_le_bytes().to_vec(), (end.len() as u32).to_le_bytes().to_vec()].concat()));
            scda.extend(end);
            let mut schr = vec![0u8; 4];
            schr.extend(0u32.to_le_bytes());
            schr.extend((scda.len() as u32).to_le_bytes());
            schr.extend(0u32.to_le_bytes());
            schr.extend(0u16.to_le_bytes());
            schr.extend(1u16.to_le_bytes());
            record(b"SCPT", 0, id.0, &[sub(b"EDID", z(edid)), sub(b"SCHR", schr), sub(b"SCDA", scda)])
        }
        fn xprm_box(half: [f32; 3]) -> Subrecord {
            let mut d: Vec<u8> = half.into_iter().flat_map(f32::to_le_bytes).collect();
            d.extend([0.0f32, 1.0, 0.0, 0.35].into_iter().flat_map(f32::to_le_bytes));
            d.extend(1u32.to_le_bytes());
            sub(b"XPRM", d)
        }
        fn placed_rec(sig: &[u8; 4], id: FormId, edid: &str, base: FormId, pos: [f32; 3], half: Option<[f32; 3]>, xtri: Option<u32>) -> Vec<u8> {
            let mut subs = vec![sub(b"EDID", z(edid)), sub(b"NAME", base.0.to_le_bytes().to_vec())];
            if let Some(h) = half {
                subs.push(xprm_box(h));
            }
            if let Some(layer) = xtri {
                subs.push(sub(b"XTRI", layer.to_le_bytes().to_vec()));
            }
            let data: Vec<u8> = pos.into_iter().chain([0.0; 3]).flat_map(f32::to_le_bytes).collect();
            subs.push(sub(b"DATA", data));
            record(sig, 0, id.0, &subs)
        }
        fn cell_entry(id: FormId, edid: &str, refs: &[u8]) -> Vec<u8> {
            let rec = record(b"CELL", 0, id.0, &[sub(b"EDID", z(edid)), sub(b"DATA", vec![1])]);
            if refs.is_empty() {
                return rec;
            }
            let temp = grup(id.0.to_le_bytes(), 9, refs);
            let children = grup(id.0.to_le_bytes(), 6, &temp);
            [rec, children].concat()
        }

        fn build_harness() -> (ScriptState, SimState, PlayWorld, PhysState) {
            let header = record(b"TES4", 1, 0, &[sub(b"HEDR", [1.34f32.to_le_bytes(), 0u32.to_le_bytes(), 0x1000u32.to_le_bytes()].concat())]);
            let player = record(b"NPC_", 0, FormId::PLAYER_BASE.0, &[sub(b"EDID", z("Player")), sub(b"DATA", [100i32.to_le_bytes().to_vec(), vec![5; 7]].concat())]);
            let npc = record(
                b"NPC_",
                0,
                NPC_BASE.0,
                &[sub(b"EDID", z("ScriptedNpc")), sub(b"SCRI", SCPT_HIT.0.to_le_bytes().to_vec()), sub(b"DATA", [100i32.to_le_bytes().to_vec(), vec![5; 7]].concat())],
            );
            let crea = record(b"CREA", 0, CREA_BASE.0, &[sub(b"EDID", z("ScriptedCrea")), sub(b"SCRI", SCPT_HIT_WITH.0.to_le_bytes().to_vec())]);
            let acti = record(b"ACTI", 0, ACTI_BASE.0, &[sub(b"EDID", z("ScriptedActi")), sub(b"SCRI", SCPT_HIT.0.to_le_bytes().to_vec())]);
            let scpts = [scpt_event(SCPT_HIT, "HitScript", event::ON_HIT), scpt_event(SCPT_HIT_WITH, "HitWithScript", event::ON_HIT_WITH)].concat();
            let cell_a_refs = [
                placed_rec(b"REFR", ZONE_REF, "ZoneRef", ACTI_BASE, [0.0, 200.0, 64.0], Some([50.0, 50.0, 50.0]), Some(23)),
                placed_rec(b"REFR", OTHER_LAYER_REF, "OtherLayerRef", ACTI_BASE, [100.0, 200.0, 64.0], Some([50.0, 50.0, 50.0]), Some(22)),
                placed_rec(b"REFR", PLAIN_REFR, "PlainRefr", ACTI_BASE, [200.0, 200.0, 64.0], None, None),
                placed_rec(b"ACHR", NPC_REF, "NpcRef", NPC_BASE, [300.0, 200.0, 64.0], None, None),
                placed_rec(b"ACRE", CREA_REF, "CreaRef", CREA_BASE, [400.0, 200.0, 64.0], None, None),
            ]
            .concat();
            let cells_inner = [cell_entry(CELL_A, "CacheCellA", &cell_a_refs), cell_entry(CELL_B, "CacheCellB", &[])].concat();
            let cell_top = grup(*b"CELL", 0, &grup(0i32.to_le_bytes(), 2, &grup(0i32.to_le_bytes(), 3, &cells_inner)));
            let raw = [
                header,
                grup(*b"NPC_", 0, &[player, npc].concat()),
                grup(*b"CREA", 0, &crea),
                grup(*b"SCPT", 0, &scpts),
                grup(*b"ACTI", 0, &acti),
                cell_top,
            ]
            .concat();
            let plugin = fnv_esm::Plugin::parse(&raw, "ProjZoneCacheTest.esm".into()).unwrap();
            let plugins = Box::leak(vec![plugin].into_boxed_slice());
            let mut state = ScriptState::over_plugins(plugins);
            let (forms, refs) = {
                let game = &state.rt.as_ref().unwrap().game;
                (game.world.forms, game.world.placed)
            };
            let lo = Box::leak(Box::new(fnv_esm::LoadOrder::new(plugins).unwrap()));
            let schema = Box::leak(Box::new(fnv_esm::Schema::load_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/fnv")).unwrap()));
            let mut sim = SimState { forms, refs, world: World::new(forms, refs, SimConfig::default()) };
            let play = PlayWorld(crate::PlayData {
                lo,
                schema,
                index: Box::leak(Box::default()),
                world: 0,
                interior: Some(CELL_A.0),
                eye: [0.0, 0.0, 64.0],
                heading_deg: 0.0,
                pitch_deg: 0.0,
                auto_walk: 0.0,
                start_at_cell_spot: false,
                packages: false,
            });
            let phys = PhysState::default();
            with_world(state.rt.as_mut().unwrap(), &mut sim, |rt| {
                rt.game.world.get(FormId::PLAYER_REF).unwrap().cell = Some(CELL_A);
                rt.game.player_entered(Some(CELL_A));
            });
            (state, sim, play, phys)
        }

        /// #3662 / PR #4101: moving scripted actors (`ACHR`/`ACRE`) and moving non-layer-23
        /// `REFR`s must not re-decode `XPRM` on pose change.
        #[test]
        fn moving_actors_and_non_layer23_refs_do_not_redecode_projectile_zones() {
            let (mut state, mut sim, play, phys) = build_harness();
            let rt = state.rt.as_mut().unwrap();
            // Control: `scripted_by_cell` genuinely includes both actors and non-actor REFRs.
            let cell_scripted = &rt.game.data.scripted_by_cell[&CELL_A];
            assert!(cell_scripted.contains(&NPC_REF), "control: scripted ACHR is in scripted_by_cell");
            assert!(cell_scripted.contains(&CREA_REF), "control: scripted ACRE is in scripted_by_cell");
            assert!(cell_scripted.contains(&OTHER_LAYER_REF));
            assert!(cell_scripted.contains(&PLAIN_REFR));

            with_world(rt, &mut sim, |rt| update(rt, &play, &phys, None, false));
            assert_eq!(rt.triggers.projectile_zone_decodes, 3, "only the 3 non-actor REFRs decode XPRM once; ACHR/ACRE are skipped");
            assert!(!rt.triggers.projectile_zones.contains_key(&NPC_REF), "ACHR is skipped");
            assert!(!rt.triggers.projectile_zones.contains_key(&CREA_REF), "ACRE is skipped");
            assert!(matches!(rt.triggers.projectile_zones.get(&OTHER_LAYER_REF), Some(ProjectileZoneEntry::NotZone)));
            assert!(matches!(rt.triggers.projectile_zones.get(&PLAIN_REFR), Some(ProjectileZoneEntry::NotZone)));

            // Move every actor and non-layer-23 REFR across several frames: zero extra decodes.
            for step in 1..=4 {
                let dx = step as f32 * 25.0;
                for (r, base_x) in [(NPC_REF, 300.0), (CREA_REF, 400.0), (OTHER_LAYER_REF, 100.0), (PLAIN_REFR, 200.0)] {
                    let s = sim.world.get(r).unwrap();
                    s.pos = [base_x + dx, 200.0 + dx, 64.0];
                    s.rot = [0.0, 0.0, step as f32 * 0.25];
                }
                with_world(rt, &mut sim, |rt| update(rt, &play, &phys, None, false));
            }
            assert_eq!(rt.triggers.projectile_zone_decodes, 3, "pose changes on actors and non-layer-23 REFRs must not re-decode XPRM");
        }

        /// #3662 / PR #4101: moving a layer-23 Projectile Zone updates its world-space
        /// volume without re-decoding `XPRM`.
        #[test]
        fn moving_layer23_projectile_zone_updates_pose_without_redecoding() {
            let (mut state, mut sim, play, phys) = build_harness();
            let rt = state.rt.as_mut().unwrap();
            with_world(rt, &mut sim, |rt| update(rt, &play, &phys, None, false));
            assert_eq!(rt.triggers.projectile_zone_decodes, 3);

            let vols = with_world(rt, &mut sim, |rt| projectile_zones(rt));
            assert_eq!(crossed_projectile_zones(&vols, [0.0, 0.0, 64.0], [0.0, 400.0, 64.0]), vec![ZONE_REF]);
            assert!(crossed_projectile_zones(&vols, [600.0, 0.0, 64.0], [600.0, 400.0, 64.0]).is_empty());

            // Move the layer-23 zone to X = 600 and run update again.
            sim.world.get(ZONE_REF).unwrap().pos = [600.0, 200.0, 64.0];
            with_world(rt, &mut sim, |rt| update(rt, &play, &phys, None, false));
            assert_eq!(rt.triggers.projectile_zone_decodes, 3, "moving a cached layer-23 zone updates pose in place without re-decoding XPRM");

            let vols = with_world(rt, &mut sim, |rt| projectile_zones(rt));
            assert!(crossed_projectile_zones(&vols, [0.0, 0.0, 64.0], [0.0, 400.0, 64.0]).is_empty(), "old position no longer crossed");
            assert_eq!(crossed_projectile_zones(&vols, [600.0, 0.0, 64.0], [600.0, 400.0, 64.0]), vec![ZONE_REF], "new position is crossed");
        }

        /// #3662 / PR #4101: unloading a cell prunes its `Triggers::projectile_zones`
        /// entries (both `Zone` and `NotZone`), and re-entering re-discovers them cleanly.
        #[test]
        fn unloading_cell_prunes_projectile_zones_cache() {
            let (mut state, mut sim, play, phys) = build_harness();
            let rt = state.rt.as_mut().unwrap();
            with_world(rt, &mut sim, |rt| update(rt, &play, &phys, None, false));
            assert_eq!(rt.triggers.projectile_zones.len(), 3, "Zone + 2 NotZone entries cached for loaded CELL_A");

            // Unload CELL_A by moving the player into CELL_B; pruning removes all CELL_A entries.
            with_world(rt, &mut sim, |rt| {
                rt.game.world.get(FormId::PLAYER_REF).unwrap().cell = Some(CELL_B);
                rt.game.player_entered(Some(CELL_B));
                rt.triggers.prune_unloaded_projectile_zones(&rt.game);
            });
            assert!(rt.triggers.projectile_zones.is_empty(), "unloading CELL_A must prune all cached projectile_zones entries");

            // Also verify `update()` prunes if `loaded_cells` changed without a direct prune call.
            with_world(rt, &mut sim, |rt| {
                rt.game.world.get(FormId::PLAYER_REF).unwrap().cell = Some(CELL_A);
                rt.game.player_entered(Some(CELL_A));
                update(rt, &play, &phys, None, false);
            });
            assert_eq!(rt.triggers.projectile_zones.len(), 3, "re-entering CELL_A re-populates cache");

            with_world(rt, &mut sim, |rt| {
                rt.game.world.get(FormId::PLAYER_REF).unwrap().cell = Some(CELL_B);
                rt.game.player_entered(Some(CELL_B));
                update(rt, &play, &phys, None, false);
            });
            assert!(rt.triggers.projectile_zones.is_empty(), "update() prunes unloaded cell entries");
        }
    }
}
