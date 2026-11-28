//! Part A (docs/design/layer4-play.md §4): crosshair, prompt, activation and keeping
//! render entities in line with the simulation.
//!
//! - `PlaySet::Activation` (while playing): find the target under the crosshair
//!   ([`pick`]: the collision ray, else references' render bounds), then E activates it
//!   through `fnv_sim::World::activate` and its outcome becomes messages
//!   ([`outcome`]): `RefChanged`, `TravelRequest` (travel behind a loading screen,
//!   `crate::loading`), `OpenContainer` or a placeholder line ([`HudMessage`]: logged,
//!   not drawn). The crosshair and the prompt under it are drawn by `crate::reticle`
//!   from [`Target`].
//! - A locked door or container without its key goes to the lockpicking menu
//!   (`crate::lockpick`), which opens or says why not.
//! - While a script has the player's controls off (`control::ControlLocks`,
//!   docs/spec-notes/controls.md §5): with movement off E does nothing.
//! - `PlaySet::Sync`: `RefVisual` entities are hidden while the simulation says their
//!   reference is not in the world, on `RefChanged` and when cells stream in.
//!
//! - E on something the player can hold (`crate::grab`): the activation waits for E to
//!   come up; held longer, the grab picks it up instead.
//!
//! Test aid: `NVF_PLAY_TEST_ACTIVATE=<seconds>[,<seconds>...]` presses E at those times
//! on the world aids' clock (`crate::pause::WorldAidClock`; unattended in-game checks).
//!
//! With Layer 5 scripts running, E goes through `script::activate_by_hand`: a target
//! whose script has an OnActivate block runs it instead of the default action (whose
//! effects then come from the script's own `Activate`, applied by `script.rs`).

pub mod outcome;
pub mod pick;
pub mod see_through;
pub mod touch;
#[cfg(test)]
pub(crate) mod tests;

use crate::phys::PhysState;
use crate::player::Player;
use crate::sim::SimState;
use crate::loading::TravelRequest;
use crate::{space, OpenContainer, PlaySet, PlayState, PlayWorld, RefChanged, RefVisual, Travel};
use bevy::prelude::*;
use fnv_phys::{layers, Filter, Owner};
use fnv_sim::forms::{BaseData, ItemKind};
use fnv_sim::world::Outcome;
use fnv_sim::FormId;
use outcome::{Action, Effect};
use pick::{Pick, RayHit, Source, REACH};
use std::collections::HashMap;

pub struct ActivatePlugin;

impl Plugin for ActivatePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Target>()
            .init_resource::<touch::TouchActivation>()
            .init_resource::<HudMessage>()
            .init_resource::<ActivateNext>()
            .insert_resource(AutoActivate::from_env())
            .add_systems(Update, (shift_talks, find_target, crate::quick_loot::follow, activate).chain().in_set(PlaySet::Activation).run_if(in_state(PlayState::Playing)))
            .add_systems(Update, sync_visuals.in_set(PlaySet::Sync));
    }
}

/// The reference under the crosshair, if it can be activated.
#[derive(Resource, Default, Debug)]
pub struct Target(pub Option<Targeted>);

#[derive(Debug, Clone, PartialEq)]
pub struct Targeted {
    pub form_id: u32,
    pub name: String,
    pub action: Action,
    pub distance: f32,
    pub source: Source,
    /// Someone else owns it: taking from it, or opening its lock, is a crime
    /// (`crate::crime`); the prompt is drawn in the crime colour.
    pub crime: bool,
    /// A container to open, or a body to search, with nothing in it the menu would list:
    /// the prompt says "[Empty]" under the name ([`holds_nothing`]).
    pub empty: bool,
    /// Picked through this reference's see-through collision (cell bars, a fence: #3233,
    /// [`see_through`]): E only opens the conversation.
    pub through: Option<u32>,
}

impl Targeted {
    /// Whether the crosshair shows it: its prompt is drawn and the crosshair takes its
    /// "something to use" picture. Not for a living hostile actor ([`Action::Hostile`]),
    /// which stays the target only so that E does nothing and the HUD knows who is aimed
    /// at (issue #2536).
    pub fn prompted(&self) -> bool {
        self.action != Action::Hostile
    }
}

/// A reference to activate as the player would, as soon as play resumes, whatever is
/// under the crosshair: a lock just picked or forced (`crate::lockpick`; playtest list 5
/// item 64).
#[derive(Resource, Default, Debug)]
pub struct ActivateNext(pub Option<u32>);

/// The last placeholder line: what the engine has no message of the game's for (an
/// item picked up, a door opened, a kill, a key that does nothing yet). It is logged
/// and kept here for the tests, never drawn: the owner wants no such white text on
/// screen (list 5, items 43 and 50). A line the game itself shows goes to the corner
/// messages (`crate::corner_messages::CornerMessages`).
#[derive(Resource, Default)]
pub struct HudMessage {
    text: String,
}

impl HudMessage {
    /// Note a placeholder line (replacing the last one); it is logged, not drawn.
    pub fn show(&mut self, text: String) {
        debug!("placeholder line, not drawn: {text}");
        self.text = text;
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

// ---- targeting -------------------------------------------------------------------

/// Name and action of a reference the player can activate now: present in the
/// simulation and of an activatable type. Activators and furniture need a display name
/// (FULL) [hypothesis: unnamed activators are trigger volumes and markers the player is
/// not meant to use; not captured], or an Activation Prompt (XATO) of the reference or
/// the base: then the prompt is offered with no name under it [hypothesis: #2160, Hoover
/// Dam's nameless control panel must be usable; cells-refs.md §3]. So does a door, unless its reference has a teleport
/// destination (XTEL) or an Activation Prompt (XATO): a nameless door with neither gives
/// no prompt [hypothesis: #1598, Mick & Ralph's OffDoorHidden01; a nameless door the
/// player may need to go through, a travel door or one given a prompt, stays offered so
/// no quest is blocked; cells-refs.md §2]. A primitive is never offered (level-design.md
/// §4.1). Placed leveled items and explosives with no weapon to give are not offered.
pub fn describe(sim: &SimState, index: &fnv_world::WorldIndex, form_id: u32) -> Option<(String, Action)> {
    if !sim.present(form_id) {
        return None;
    }
    let id = FormId(form_id);
    let base = sim.forms.get(sim.base_of(form_id)?)?;
    let name = sim.name(base.id)?.to_string();
    let state = sim.world.peek(id);
    let placed = sim.refs.get(id);
    // A primitive (XPRM) is a trigger volume or marker, not an object to use, whatever
    // name its activator has (docs/spec-notes/level-design.md §4.1).
    if placed.is_some_and(|p| p.primitive.is_some()) {
        return None;
    }
    // [design, #2538] Use the same applicable lock as activation, including a load
    // door's partner (`World::lock_of`, docs/spec-notes/cells-refs.md section 6).
    let lock = sim.world.lock_of(id).filter(|l| l.locked).map(|l| (l.level, l.key));
    let carried = |k: FormId| {
        sim.world.peek(FormId::PLAYER_REF).and_then(|p| p.inventory.as_ref()).is_some_and(|inv| inv.count(k) > 0)
    };
    let locked = |unlocked: Action| match lock {
        Some((_, Some(k))) if carried(k) => Action::Unlock,
        Some((level, _)) => Action::Locked { level },
        None => unlocked,
    };
    let action = match &base.data {
        // No prompt for a non-playable weapon; non-playable ammo shows one but cannot be
        // taken (`fnv_sim::World::activate`) [documented: GECK wiki "Unplayable Items"].
        BaseData::Item(it) if it.non_playable && it.kind == ItemKind::Weapon => return None,
        // Nor for a book that can't be taken (docs/spec-notes/skill-books.md §4).
        BaseData::Item(it) if it.cant_be_taken => return None,
        BaseData::Item(_) => Action::Take { count: state.map_or_else(|| sim.forms.placed_count(base.id, placed.and_then(|p| p.count)), |s| s.count) },
        BaseData::Container(_) => locked(Action::Open),
        // A door flagged Inaccessible (REFR header bit 8 [documented: GECK wiki
        // GetFormFlag]) leads nowhere the player may go (cells-refs.md §2).
        BaseData::Door(_) if placed.is_some_and(|p| p.flags & fnv_sim::refs::INACCESSIBLE != 0) => Action::Inaccessible,
        // A door without a name, leading nowhere and given no Activation Prompt, is not
        // for the player (above): no prompt.
        BaseData::Door(_)
            if base.name.as_deref().is_none_or(|n| n.trim().is_empty())
                && placed.is_none_or(|p| p.teleport.is_none() && p.prompt.is_none())
                && base.prompt.is_none() =>
        {
            return None
        }
        BaseData::Door(_) => locked(match placed.and_then(|p| p.teleport) {
            Some(tp) => Action::Travel { to: destination_name(sim, index, tp.door, tp.pos) },
            None if state.is_some_and(|s| s.open) => Action::Close,
            None => Action::Open,
        }),
        BaseData::Actor(_) => Action::Talk,
        BaseData::Activator if &base.sig == b"TERM" => Action::Access,
        // A nameless activator the level designer gave an Activation Prompt is for the
        // player: offered by its prompt alone, no name line (#2160: Hoover Dam's control
        // panel, VHDMainControlPanel1REF, a talking activator with no FULL and the XATO
        // "Activate", where the override chip goes).
        BaseData::Activator | BaseData::Furniture if base.name.is_none() => {
            placed.and_then(|p| p.prompt.as_ref()).or(base.prompt.as_ref())?;
            return Some((String::new(), Action::Activate));
        }
        BaseData::Activator | BaseData::Furniture => Action::Activate,
        BaseData::LeveledItem(_) | BaseData::Projectile { item: None, .. } => return None,
        BaseData::Projectile { item: Some(_), .. } => Action::Disarm,
    };
    Some((name, action))
}

/// Name of the cell a door leads into: the destination door's cell, or for a worldspace
/// the exterior cell holding the destination, by display name, else its worldspace's,
/// else its Editor ID.
fn destination_name(sim: &SimState, index: &fnv_world::WorldIndex, door: FormId, pos: [f32; 3]) -> Option<String> {
    let cell = sim.refs.get(door)?.cell?;
    let cell = sim.refs.exterior_cell_at(cell, pos).unwrap_or(cell);
    let c = index.cells.get(&cell.0)?;
    c.name
        .clone()
        .or_else(|| c.world.and_then(|w| index.worldspaces.get(&w)).and_then(|w| w.name.clone()))
        .or_else(|| c.edid.clone())
}

/// Resolve the body part location (`BodyLocation` enum, 0..=14) for an actor hit under
/// the crosshair (`GetReticleTargetLimb`). Returns `Some(0)` (Torso, default body
/// location [design]) for single-capsule fallback actors (`fnv_phys::actors::WHOLE`) so
/// scripts distinguish hitting an actor from a miss, and returns `None` (`-1` to scripts)
/// for non-actors, misses, and detached gore pieces (`fnv_phys::gore::GORE_PART`).
pub(crate) fn reticle_target_limb(
    owner: Owner,
    parts: Option<&crate::hitvol::ActorParts>,
    combat: Option<&crate::combat::Combat>,
) -> Option<i32> {
    let Owner::Actor { form_id, part, .. } = owner else {
        return None;
    };
    if part == crate::hitvol::WEAPON_PART {
        return Some(14);
    }
    if part == fnv_phys::actors::WHOLE {
        return Some(0);
    }
    if part == fnv_phys::gore::GORE_PART {
        return None;
    }
    let data = parts?.data_of(form_id)?;
    let loc = combat?.forms.body_parts.get(&data)?.parts.get(part as usize)?.location();
    (loc >= 0).then_some(loc)
}

/// The collision ray under the crosshair (the player's capsule and trigger volumes
/// ignored), or a door just behind what it hits (`pick::door_behind`), then the
/// render-bounds fallback (`pick`), then someone to talk to behind bars
/// (`see_through`, #3233). In first person it is the eye's
/// view ray; in third person the camera's, through the crosshair over the shoulder
/// (`control::Crosshair`; playtest list 4, item 11).
#[allow(clippy::too_many_arguments)]
pub(crate) fn find_target(
    player: Option<Res<Player>>,
    mut phys: Option<ResMut<PhysState>>,
    mut sim: ResMut<SimState>,
    play: Res<PlayWorld>,
    visuals: crate::PickVisuals,
    mut target: ResMut<Target>,
    mut script: Option<ResMut<crate::script::ScriptState>>,
    control: Option<Res<crate::control::PlayerControl>>,
    mut talkers: Local<HashMap<u32, (bool, f64)>>,
    (mut touch,cameras,windows,admission,input,time):touch::Context,
    parts: Option<Res<crate::hitvol::ActorParts>>,
    combat: Option<Res<crate::combat::Combat>>,
) {
    let Some(player) = player else { return };
    let crosshair = control.as_deref().and_then(|c| c.crosshair);
    let (eye, dir) = crate::control::Crosshair::ray(crosshair.as_ref(), player.eye(), player.look_dir());
    let mut picker=TargetPicker {phys:phys.as_deref_mut(),sim:&mut sim,play:&play,
        visuals:&visuals,script:script.as_deref_mut(),talkers:&mut talkers,parts:parts.as_deref(),combat:combat.as_deref()};
    let (new, note)=picker.at(eye,dir,true);
    if let Some(touch)=touch.as_deref_mut() {
        if let Some(point)=touch.point() {
            if admission.allows(point.id) && input.virtual_action_allowed(crate::bindings::Control::Activate) {
                let selected=touch::camera_ray(point,&cameras,&windows).and_then(|(camera,direction)|{
                    let view=crate::control::Crosshair {camera,dir:direction,pivot:player.eye(),shoulder:[0.;2]};
                    let (origin,dir)=crate::control::Crosshair::ray(Some(&view),player.eye(),player.look_dir());
                    let target=picker.at(origin,dir,false).0?;
                    let hit=Vec3::from_array(origin)+Vec3::from_array(dir)*target.distance;
                    let delta=hit-Vec3::from_array(player.eye());
                    let distance=delta.length();
                    if !(0.001..=REACH).contains(&distance){return None;}
                    // The camera can peek around cover; the player still needs LOS.
                    let legal=picker.at(player.eye(),(delta/distance).to_array(),false).0?;
                    (legal.form_id==target.form_id).then_some(legal)
                });
                touch.publish(selected,time.as_ref().map_or(0.,|t|t.delta_secs()));
            } else {touch.cancel();}
        }
    }
    let key = |t: &Option<Targeted>| t.as_ref().map(|t| (t.form_id, t.action.clone(), t.crime, t.empty, t.through));
    if key(&target.0) != key(&new) {
        if let Some(note) = note {
            info!("{note}");
        }
        match &new {
            Some(t) => info!(
                "target 0x{:08X} {} ({}{}{}) by {:?}{} at {:.0}",
                t.form_id,
                t.name,
                t.action.label(),
                if t.crime { ", owned" } else { "" },
                if t.empty { ", empty" } else { "" },
                t.source,
                t.through.map_or_else(String::new, |b| format!(" through 0x{b:08X}")),
                t.distance
            ),
            None => info!("target none"),
        }
    }
    if target.0 != new {
        target.0 = new;
    }
}

struct TargetPicker<'a,'w,'s>{
    phys:Option<&'a mut PhysState>,sim:&'a mut SimState,play:&'a PlayWorld,
    visuals:&'a crate::PickVisuals<'w,'s>,script:Option<&'a mut crate::script::ScriptState>,
    talkers:&'a mut HashMap<u32,(bool,f64)>,parts:Option<&'a crate::hitvol::ActorParts>,
    combat:Option<&'a crate::combat::Combat>,
}
impl TargetPicker<'_,'_,'_>{
    fn at(&mut self,eye:[f32;3],dir:[f32;3],publish_crosshair:bool)->(Option<Targeted>, Option<String>){
        let phys=self.phys.as_deref_mut();
        let sim=&mut *self.sim;
        let play=self.play;
        let visuals=self.visuals;
        let mut script=self.script.as_deref_mut();
        let talkers=&mut *self.talkers;
        let parts=self.parts;
        let combat=self.combat;
    // The same ray, far: where PlaceAtReticle puts things (`fnv_game` senses `Reticle`).
    let mut far = None;
    let mut far_ref = None;
    let mut far_limb = None;
    // The node of what the far ray meets (GetReticleNode, `fnv_game::nvse_jip13`).
    let mut far_node = None;
    let mut behind = None;
    // An actor behind bars or a fence the ray stops on (`see_through::actor_behind`).
    let mut through = None;
    let hit = phys.and_then(|p| {
        let mut filter = p.player.as_ref().map_or_else(Filter::default, |c| Filter::excluding(c.key()));
        // Trigger volumes do not block the view [hypothesis].
        filter.exclude_layers.insert(layers::TRIGGER);
        let far_hit = p.world.queries().ray(eye, dir, fnv_game::functions::senses::RETICLE_REACH, &filter);
        far = far_hit.as_ref().map(|h| h.distance);
        far_limb = far_hit.as_ref().and_then(|h| reticle_target_limb(h.owner, parts, combat));
        far_node = far_hit.as_ref().and_then(|h| match h.owner {
            Owner::Ref { form_id, object, .. } => p.world.object_node(form_id, object).map(str::to_string),
            _ => None,
        });
        far_ref = far_hit.and_then(|h| match h.owner {
            Owner::Ref { form_id, .. } | Owner::Actor { form_id, .. } => Some(form_id),
            _ => None,
        });
        // Invisible walls do not take the crosshair: what stands behind one is activated
        // (#1602: the Gun Runners' kiosk, nv_gunrun_kiosk.nif, is one body on layer 27
        // Invisible Wall, the Vendortron inside it) [documented: GECK wiki "Collision
        // layer": "the player can activate / loot objects through this layer"]. The far
        // ray above (PlaceAtReticle) keeps stopping at them.
        filter.exclude_layers.insert(layers::INVISIBLE_WALL);
        let h = p.world.queries().ray(eye, dir, REACH, &filter)?;
        // A door, closed, hides what is behind it whatever its collision layer.
        let s: &SimState = sim;
        let closed_door = |id: u32| s.base_of(id).and_then(|b| s.forms.get(b)).is_some_and(|f| matches!(f.data, BaseData::Door(_))) && s.world.peek(FormId(id)).is_none_or(|st| !st.open);
        through = see_through::actor_behind(&mut p.world, eye, dir, REACH, &h, &filter, closed_door);
        let owner = |o: Owner| match o {
            // An actor's hit volume (combat C3) targets the actor.
            Owner::Ref { form_id, .. } | Owner::Actor { form_id, .. } => Some(form_id),
            _ => None,
        };
        let first = owner(h.owner);
        // The first other reference within DOOR_SLACK behind it (a door set just
        // behind a building's face, `pick::door_behind`), past the first hit's other faces.
        let mut from = h.distance;
        while behind.is_none() && from < h.distance + pick::DOOR_SLACK {
            let start = [eye[0] + dir[0] * (from + NUDGE), eye[1] + dir[1] * (from + NUDGE), eye[2] + dir[2] * (from + NUDGE)];
            let Some(b) = p.world.queries().ray(start, dir, h.distance + pick::DOOR_SLACK - from - NUDGE, &filter) else { break };
            from += NUDGE + b.distance;
            behind = owner(b.owner).filter(|id| Some(*id) != first).map(|id| RayHit { form_id: Some(id), distance: from });
        }
        Some(RayHit { form_id: first, distance: h.distance })
    });
    // Only a door is looked for behind a face.
    let behind = behind.filter(|b| b.form_id.and_then(|id| sim.base_of(id)).and_then(|base| sim.forms.get(base)).is_some_and(|f| matches!(f.data, BaseData::Door(_))));
    // Render bounds are in Bevy space; distances are game units in both.
    let (o, d) = (space::to_bevy(eye), space::to_bevy(dir));
    let bounds = visuals.iter().filter(|(_, _, _, v)| v.get()).filter_map(|(r, aabb, gt, _)| {
        let t = gt.affine();
        let (c, h) = (Vec3::from(aabb.center), Vec3::from(aabb.half_extents));
        let (sc, sr) = pick::bounding_sphere(&t, c, h);
        if sc.distance(o) - sr > REACH {
            return None;
        }
        let (entry, exit) = pick::ray_box_span(o, d, &t, c, h)?;
        Some((r.0, entry, exit))
    });
    // An actor's box is rough (`pick::choose_precise_first`, #3125).
    let bounds: Vec<pick::BoxSpan> = bounds.map(|(id, entry, exit)| (id, entry, exit, sim.is_actor(id))).collect();
    if let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()).filter(|_| publish_crosshair) {
        rt.game.senses.reticle = Some(fnv_game::functions::senses::Reticle { start: eye, dir, hit: far, limb: far_limb });
        if rt.game.other.jip13.reticle_node != far_node {
            rt.game.set_reticle_node(far_node);
        }
        // GetCrosshairRef: what is in activation reach; GetCrosshairRefEx: at any range.
        let now = hit.as_ref().and_then(|h| h.form_id).map(FormId);
        rt.game.set_crosshair_ref(now);
        rt.game.senses.crosshair_far = far_ref.map(FormId);
    }
    // Beds an actor lies on among the candidates, and who (`pick::to_occupant`, #3231):
    // the bed markers are IDs 1 to 9 (`fnv_game::functions::furniture`). Looked up only
    // when furniture is under the crosshair.
    let is_furniture = |id: u32| sim.base_of(id).and_then(|b| sim.forms.get(b)).is_some_and(|b| matches!(b.data, BaseData::Furniture));
    let furniture_seen = hit.and_then(|h| h.form_id).into_iter().chain(bounds.iter().map(|b| b.0)).any(is_furniture);
    // Beds among the boxes (FURN marker flags with Sleep, bit 31, furniture.md §1) may
    // yield to what lies in them (`pick::set_aside_enclosing`, #3323).
    // The containers among the boxes give way to what lies inside them (#3351), activators
    // to an item lying in their box (the note on a radio, #3937), nothing else
    // (`pick::choose_inside`).
    let of_kind = |kind: fn(&BaseData) -> bool| -> std::collections::HashSet<u32> {
        bounds.iter().map(|b| b.0).chain(hit.and_then(|h| h.form_id)).filter(|&id| sim.base_of(id).and_then(|b| sim.forms.get(b)).is_some_and(|f| kind(&f.data))).collect()
    };
    let containers = of_kind(|d| matches!(d, BaseData::Container(_)));
    let activators = of_kind(|d| matches!(d, BaseData::Activator));
    let items = of_kind(|d| matches!(d, BaseData::Item(_)));
    let yields = |outer: u32, inner: u32| containers.contains(&outer) || (activators.contains(&outer) && items.contains(&inner));
    let beds: std::collections::HashSet<u32> = match script.as_deref().and_then(|s| s.rt.as_ref()) {
        Some(rt) if furniture_seen => {
            let bed = |id: u32| sim.base_of(id).and_then(|b| rt.game.data.furniture.get(&b)).is_some_and(|f| f & fnv_game::package::furniture::SLEEP != 0);
            bounds.iter().map(|b| b.0).filter(|&id| bed(id)).collect()
        }
        _ => Default::default(),
    };
    let sleepers: HashMap<u32, u32> = match script.as_deref().and_then(|s| s.rt.as_ref()) {
        Some(rt) if furniture_seen => rt
            .game
            .extra
            .iter()
            .filter(|(a, _)| **a != FormId::PLAYER_REF)
            .filter_map(|(a, e)| e.furniture.filter(|u| (1..=9).contains(&u.marker_id)).map(|u| (u.reference.0, a.0)))
            .collect(),
        _ => HashMap::new(),
    };
    let mut described: HashMap<u32, Option<(String, Action)>> = HashMap::new();
    // A dead actor is searched rather than talked to (combat C5); a living one is talked
    // to only when it may be ([`actor_action`]). Actors are named by their template's
    // name where the template supplies it (playtest list 4, item 20). A destroyed door
    // or activator gives no rollover text (`fnv_game::Game::activation_refused`).
    let mut rt = script.as_deref_mut().and_then(|s| s.rt.as_mut());
    let mut describe_once = |id: u32| {
        described
            .entry(id)
            .or_insert_with(|| {
                // A body culled by its critical stage (turned to ash or goo) is gone.
                // Nor a form type or reference scripts closed to the player
                // (SetInteractionDisabledType, SetInteractionDisabled).
                if rt.as_deref().is_some_and(|rt| rt.game.activation_refused(FormId(id)) || rt.game.culled(FormId(id)) || rt.game.player_interaction_disabled(FormId(id))) {
                    return None;
                }
                let pile = rt.as_deref().is_some_and(|rt| rt.game.ash_pile_source(FormId(id)).is_some());
                describe(sim, play.index, id).map(|(n, a)| match a {
                    // An ash or goo pile is searched as the body was (ash-piles.md §3).
                    Action::Activate if pile => (n, Action::Search),
                    Action::Activate if rt.as_deref().is_some_and(|rt| rt.game.drink_water(FormId(id)).is_some()) => {
                        // The player's tweak: the prompt says the H2O a drink takes off [design: docs/design/tweaks.md].
                        let h2o = rt.as_deref_mut().filter(|rt| rt.game.tweaks.water_shows_h2o).and_then(|rt| {
                            let spell = rt.game.drink_water(FormId(id)).and_then(|w| rt.game.water_effect(w))?;
                            crate::script::with_world(rt, sim, |rt| rt.game.aid_preview(FormId::PLAYER_REF, spell)).into_iter().find(|(c, _)| *c == fnv_game::aid::DEHYDRATION).map(|(_, a)| a)
                        });
                        (water_label(&n, h2o), Action::Drink)
                    }
                    Action::Talk => {
                        // An actor's shown name, never its base's Editor ID (#3445): one with no
                        // name in its template chain is prompted for without one.
                        let name = sim.ref_shown_name(rt.as_deref().map(|rt| &rt.game), id).unwrap_or_default().to_string();
                        // The player's tweak marks someone not yet spoken to [design: docs/design/tweaks.md].
                        let unspoken = rt.as_deref().is_some_and(|rt| rt.game.tweaks.mark_unspoken && !rt.game.extra.get(&FormId(id)).is_some_and(|e| e.talked_to_pc));
                        let name = if unspoken { format!("{name} *") } else { name };
                        // The player's tweak: no name for someone who has not spoken to the
                        // player (a body is named still, as Search) [design: docs/design/tweaks.md].
                        let unknown = rt.as_deref().is_some_and(|rt| rt.game.tweaks.hide_unknown_names && !rt.game.extra.get(&FormId(id)).is_some_and(|e| e.talked_to_pc));
                        let name = if unknown { String::new() } else { name };
                        (name, actor_action(rt.as_deref_mut(), sim, id, talkers))
                    }
                    Action::Access => (n, terminal_action(rt.as_deref_mut(), sim, id)),
                    // An interior a script renamed (Fallout 3's SetCellFullName,
                    // `fnv_game::functions::fo3`) is offered by its new name.
                    Action::Travel { to } => {
                        let cell = sim.refs.get(FormId(id)).and_then(|p| p.teleport).and_then(|tp| sim.refs.get(tp.door)?.cell);
                        let renamed = rt.as_deref().zip(cell).and_then(|(rt, c)| rt.game.script_name(c));
                        // The player's tweak marks a place never visited [design: docs/design/tweaks.md].
                        let unseen = rt.as_deref().zip(cell).is_some_and(|(rt, c)| rt.game.tweaks.mark_unvisited_doors && !rt.game.forms.visited_cells.contains(&c));
                        let to = renamed.map(str::to_string).or(to).map(|t| if unseen { format!("{t} *") } else { t });
                        (n, Action::Travel { to })
                    }
                    a => (n, a),
                })
            })
            .clone()
    };
    // A bed's box does not hide the suitcase lying in it (`pick::set_aside_enclosing`,
    // #3323), cut off at the raw first hit, not a door promoted from behind it.
    let kept = pick::set_aside_enclosing(hit, &bounds, REACH, |id| beds.contains(&id), |id| describe_once(id).is_some());
    // `NVF_PLAY_TEST_PICK_LOG`: what the pick was made from, logged with each new target.
    let note = pick_log().then(|| {
        let aside: Vec<String> = bounds.iter().filter(|b| !kept.iter().any(|k| k.0 == b.0)).map(|b| format!("0x{:08X}", b.0)).collect();
        let boxes: Vec<String> = bounds.iter().map(|b| format!("0x{:08X} {:.1}-{:.1}{}{}", b.0, b.1, b.2, if b.3 { " rough" } else { "" }, if beds.contains(&b.0) { " bed" } else { "" })).collect();
        format!("pick: raw hit {:?}; boxes [{}]; set aside [{}]", hit.map(|h| (h.form_id.map(|id| format!("0x{id:08X}")), h.distance)), boxes.join(", "), aside.join(", "))
    });
    let raw_hit = hit;
    let hit = pick::door_behind(hit, behind, |id| describe_once(id).is_some());
    // What is inside a container wins over it (an open locker's shelves, #3351), an item
    // over the activator it lies on (#3937), among the boxes the bed rule kept. When only
    // rough actor boxes remain, prefer the actor whose posed body segments are nearest the
    // crosshair ray (#3664).
    let kept: Vec<pick::BoxSpan> = bounds.iter().filter(|b| kept.iter().any(|k| k.0 == b.0)).copied().collect();
    let rough_limit = hit.map_or(REACH, |h| (h.distance + pick::SURFACE_SLACK).min(REACH));
    let ray_origin = Vec3::from_array(eye);
    let ray_dir = Vec3::from_array(dir);
    let body_ray_distance = |actor: u32| {
        let posed = parts.and_then(|p| p.by_ref.get(&actor)).filter(|p| !p.segments.is_empty())?;
        posed
            .segments
            .iter()
            .map(|s| pick::ray_segment_distance(ray_origin, ray_dir, rough_limit, Vec3::from_array(s.a), Vec3::from_array(s.b)))
            .reduce(f32::min)
    };
    let picked = pick::choose_inside_with_body(hit, raw_hit, kept, REACH, yields, body_ray_distance, |id| describe_once(id).is_some());
    // How far the sleeper's body is from the eye: its posed bones (`crate::hitvol::ActorParts`,
    // which follow the lying body), else its render boxes (rough: the bind pose).
    let body_distance = |actor: u32| {
        let posed = parts.and_then(|p| p.by_ref.get(&actor)).filter(|p| !p.segments.is_empty());
        if let Some(posed) = posed {
            let e = Vec3::from_array(eye);
            return posed.segments.iter().map(|s| pick::segment_distance(e, Vec3::from_array(s.a), Vec3::from_array(s.b))).reduce(f32::min);
        }
        visuals
            .iter()
            .filter(|(r, _, _, v)| r.0 == actor && v.get())
            .filter_map(|(_, aabb, gt, _)| pick::box_distance(o, &gt.affine(), Vec3::from(aabb.center), Vec3::from(aabb.half_extents)))
            .reduce(f32::min)
    };
    let picked = pick::to_occupant(picked, |id| sleepers.get(&id).and_then(|&a| Some((a, body_distance(a)?))), REACH, |id| describe_once(id).is_some());
    // Nothing offered and the ray stopped on bars: talk to whoever stands behind them
    // (#3233). Only Talk passes; GetCrosshairRef (above) stays the first collision hit.
    let (picked, through) = see_through::talk_through(picked, through, REACH, |id| matches!(describe_once(id), Some((_, Action::Talk))));
    let chosen = picked.and_then(|p| describe_once(p.form_id).map(|d| (p, d)));
    let game = script.as_deref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game);
    let chosen = chosen.map(|(Pick { form_id, distance, source }, (name, action))| {
        let crime = owned_by_other(game, sim, form_id, &action);
        let empty = holds_nothing(game, sim, form_id, &action);
        Targeted { form_id, name, action, distance, source, crime, empty, through }
    });
    (chosen, note)
    }
}

/// How far past a surface the ray behind it starts, game units.
const NUDGE: f32 = 0.5;

/// Whether `NVF_PLAY_TEST_PICK_LOG` is on: each new target logs the raw first collision
/// hit, every render box the view ray enters (reference, entry and exit, rough, bed) and
/// the boxes set aside (`pick::set_aside_enclosing`, #3323).
fn pick_log() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("NVF_PLAY_TEST_PICK_LOG").is_ok_and(|v| !matches!(v.trim(), "" | "0" | "off" | "false")))
}

/// Seconds a creature's "has something to say" answer is kept before it is asked again.
const TALKER_SECONDS: f64 = 2.0;
/// The corner line when the player has no Stimpak to revive a companion with (the
/// Revive Unconscious Companions tweak) [design: our own words].
const REVIVE_NEEDS_STIMPAK: &str = "You need a Stimpak to revive them.";

/// What E does to a living or dead actor (playtest list 4, item 19): a dead one is
/// searched (combat C5); one hostile to the player (it would attack the player, or is
/// fighting the player) shows no prompt at all ([`Action::Hostile`], issue #2536: players
/// saw the name come up each time the crosshair crossed an enemy in a fight, which the
/// original does not do [observed: the player's report; not captured]); a creature is talked to only when it has a
/// greeting it can say now ([`fnv_game::Game::has_greeting`]), else shows only its name;
/// any other NPC is talked to [hypothesis: the game offers Talk on creatures that have
/// dialogue, such as Rex and ED-E, and not on animals; an NPC without a line keeps the
/// prompt and says nothing, as before]. Without the scripts: Talk.
fn actor_action(rt: Option<&mut crate::script::Runtime>, sim: &mut SimState, id: u32, talkers: &mut HashMap<u32, (bool, f64)>) -> Action {
    let Some(rt) = rt else { return Action::Talk };
    // By the base for a reference made at run time, which has no placement (#3124).
    let creature = sim.is_creature(id);
    crate::script::with_world(rt, sim, |rt| actor_action_in(&mut rt.game, creature, id, talkers))
}

/// The player's Talk While Sneaking tweak: Shift held while sneaking talks instead of
/// picking a pocket [design: docs/design/tweaks.md]. Set each frame by [`shift_talks`].
static SHIFT_TALKS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn shift_talks(keys: Res<ButtonInput<KeyCode>>, tweaks: Option<Res<crate::tweaks::TweakSettings>>, mut sim: ResMut<SimState>) {
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let on = tweaks.as_ref().is_some_and(|t| t.shift_talks_sneaking) && shift;
    SHIFT_TALKS.store(on, std::sync::atomic::Ordering::Relaxed);
    // The player's tweak: with Shift the lock is picked though the player has its key [design: docs/design/tweaks.md].
    sim.world.set_player_ignores_keys(tweaks.is_some_and(|t| t.pick_locks_with_key) && shift);
}

fn actor_action_in(g: &mut fnv_game::Game, creature: bool, id: u32, talkers: &mut HashMap<u32, (bool, f64)>) -> Action {
    let r = FormId(id);
    // Another player of a co-op game: only its name (there is no one to talk to).
    if g.is_coop_actor(r) {
        return Action::Name;
    }
    // The player's tweak: a knocked-out companion is revived [design: docs/design/tweaks.md].
    if g.can_revive(r) {
        return Action::ReviveCompanion;
    }
    // The player's tweak lets an unconscious actor be searched as a dead one is [design: docs/design/tweaks.md].
    if g.is_dead(r) || (g.tweaks.loot_unconscious && g.knockout(r).is_some()) {
        return Action::Search;
    }
    let fighting_player = g.awareness.get(r).is_some_and(|a| a.state.in_combat() && a.targets.contains_key(&FormId::PLAYER_REF));
    // On a co-op guest, the host's word too (`Game::coop_hostile`, as the compass reads
    // it): the host's quest stages turn people hostile (VMS16 stage 70's SetEnemy) in its
    // world, which the guest's own factions don't know [observed: buckallobill, in game,
    // NVForever co-op: the Powder Gangers fighting in Goodsprings offered Talk on the
    // guest]. Empty outside co-op.
    if fighting_player || g.coop_hostile.contains(&r) || g.would_attack(r, FormId::PLAYER_REF) {
        return Action::Hostile;
    }
    // A co-op guest talks only to trade, to play and to change chips at a cashier
    // (`fnv_game::Game::guest_lines`); the host's world runs the rest [design].
    if g.coop_guest {
        let now = g.time;
        let talks = match talkers.get(&id) {
            Some(&(t, at)) if (now - at).abs() < TALKER_SECONDS => t,
            _ => {
                let t = g.guest_can_talk(r);
                talkers.insert(id, (t, now));
                t
            }
        };
        return if talks { Action::Talk } else { Action::Name };
    }
    // Sneaking, the player picks pockets instead (`crate::pickpocket`); one that caught
    // the player before still shows Steal and says so on E.
    let shift = SHIFT_TALKS.load(std::sync::atomic::Ordering::Relaxed);
    if !shift && g.is_sneaking(FormId::PLAYER_REF) && g.pickpocket_refusal(r) != Some(fnv_game::pickpocket::Refusal::NotAllowed) {
        return Action::Steal;
    }
    if !creature {
        return Action::Talk;
    }
    let now = g.time;
    let talks = match talkers.get(&id) {
        Some(&(t, at)) if (now - at).abs() < TALKER_SECONDS => t,
        _ => {
            // A creature is talked to when it is flagged Allow PC Dialogue and has a
            // greeting to say (docs/spec-notes/actor-flags.md §4) [hypothesis: both].
            let t = g.allows_pc_dialogue(r) && g.has_greeting(r);
            talkers.insert(id, (t, now));
            t
        }
    };
    if talks {
        Action::Talk
    } else {
        Action::Name
    }
}

/// What E does to a terminal: one that must still be hacked (or refuses, or is locked
/// out) is prompted as a lock of its difficulty, "Locked (Easy)" (the owner,
/// 2026-10-22; `fnv_game::terminal::still_locked`); one that opens at its menu is
/// accessed. Without the scripts: Access.
fn terminal_action(rt: Option<&mut crate::script::Runtime>, sim: &mut SimState, id: u32) -> Action {
    let (Some(rt), Some(base)) = (rt, sim.base_of(id)) else { return Action::Access };
    // The player's tweak: a locked terminal the note opens says so [design: docs/design/tweaks.md].
    if crate::script::with_world(rt, sim, |rt| fnv_game::terminal::opens_by_password(&mut rt.game, FormId(id), base)) {
        return Action::UsePassword;
    }
    let lock = crate::script::with_world(rt, sim, |rt| fnv_game::terminal::still_locked(&mut rt.game, FormId(id), base));
    lock.map_or(Action::Access, |d| Action::Locked { level: outcome::terminal_lock_level(d.code()) })
}

/// Whether acting on the target is a crime: an item to take, a container to open or a
/// lock to pick (a terminal to hack among them) that someone other than the player owns
/// (`fnv_game::crime`). A dead actor's inventory is not [hypothesis].
pub(crate) fn owned_by_other(game: Option<&fnv_game::Game>, sim: &SimState, form_id: u32, action: &Action) -> bool {
    // Picking a pocket is theft: the prompt shows as a crime's does [design: the
    // original's steal prompt is red].
    if *action == Action::Steal {
        return true;
    }
    let Some(g) = game else { return false };
    let container = sim.base_of(form_id).and_then(|b| sim.forms.get(b)).is_some_and(|b| matches!(b.data, BaseData::Container(_)));
    let judged = match action {
        Action::Take { .. } | Action::Locked { .. } => true,
        Action::Open | Action::Unlock => container,
        _ => false,
    };
    judged && g.ownership_in(&sim.world, FormId(form_id)).is_some_and(|o| !g.counts_as_owner(FormId::PLAYER_REF, &o))
}

/// Whether the target, a container to open or a body (or its ash pile) to search, holds
/// nothing the container menu would list (`menu::model::listing`: non-playable items are
/// not) [owner 2026-10-27: such a prompt says "[Empty]"]. A container's contents are
/// rolled here if it has not been touched yet, as opening it would roll them; a locked
/// one is not looked into. A body's weapon lying beside it (`World::dropped_beside`) is
/// listed in its loot menu, so it counts too (#1032).
pub(crate) fn holds_nothing(game: Option<&fnv_game::Game>, sim: &mut SimState, form_id: u32, action: &Action) -> bool {
    let holder = match action {
        Action::Search => game.and_then(|g| g.ash_pile_source(FormId(form_id))).unwrap_or(FormId(form_id)),
        Action::Open if sim.base_of(form_id).and_then(|b| sim.forms.get(b)).is_some_and(|b| matches!(b.data, BaseData::Container(_))) => FormId(form_id),
        _ => return false,
    };
    let forms = sim.forms;
    let unlisted = |item: FormId| !crate::menu::model::shown(forms, item, false);
    let Ok(s) = sim.world.get(holder) else { return false };
    if !s.inventory.as_ref().is_none_or(|inv| inv.iter().all(|(item, _)| unlisted(item))) {
        return false;
    }
    sim.world.dropped_beside(holder).into_iter().all(|r| sim.world.peek(r).is_none_or(|d| unlisted(d.base)))
}

/// Test aid: times (seconds on the world aids' clock, `crate::pause::WorldAidClock`) at
/// which E counts as pressed.
#[derive(Resource, Default)]
pub(crate) struct AutoActivate {
    times: Vec<f32>,
}

impl AutoActivate {
    /// Schedule an E press after a diagnostic replays its successful view [design: #2528,
    /// test_aid/activation_sweep.rs]. This still activates the current Target.
    pub(crate) fn test_press_at(&mut self, at: f32) {
        self.times.push(at);
    }

    fn from_env() -> AutoActivate {
        let times = std::env::var("NVF_PLAY_TEST_ACTIVATE")
            .map(|v| v.split(',').filter_map(|t| t.trim().parse().ok()).collect())
            .unwrap_or_default();
        AutoActivate { times }
    }

    /// Whether a time has come since the last call (each fires once).
    fn fire(&mut self, now: f32) -> bool {
        let before = self.times.len();
        self.times.retain(|&t| t > now);
        self.times.len() < before
    }
}

/// A water source's prompt name with the H2O a drink takes off (the player's Water Sources
/// Show H2O tweak): "Faucet (H2O -15)" [design: our own words, as the Pip-Boy's figures].
pub fn water_label(name: &str, h2o: Option<f64>) -> String {
    match h2o.filter(|a| a.abs() >= 0.5) {
        Some(a) => format!("{name} (H2O {:+.0})", a),
        None => name.to_string(),
    }
}

/// How long E is held to act under the player's Hold to Activate tweak, seconds [design].
pub const HOLD_ACTIVATE: f32 = 0.5;

/// The kinds of target the player's Hold to Activate tweak asks E to be held on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HoldKinds {
    pub containers: bool,
    pub furniture: bool,
    pub water: bool,
}

impl HoldKinds {
    pub fn of(t: &fnv_game::tweaks::Tweaks) -> HoldKinds {
        HoldKinds { containers: t.hold_activate && t.hold_containers, furniture: t.hold_activate && t.hold_furniture, water: t.hold_activate && t.hold_water }
    }

    /// Whether E must be held on `t`: a container or a body to search, furniture, water.
    pub fn needs(&self, sim: &SimState, t: &Targeted) -> bool {
        let base = sim.base_of(t.form_id).and_then(|b| sim.forms.get(b)).map(|b| &b.data);
        match &t.action {
            Action::Search => self.containers,
            Action::Open => self.containers && matches!(base, Some(BaseData::Container(_))),
            Action::Activate => self.furniture && matches!(base, Some(BaseData::Furniture)),
            Action::Drink => self.water,
            _ => false,
        }
    }
}

/// The clocks of the activate aid: time, the stopped-clock time and the world aids' clock
/// (each absent in tests that build without them).
pub(crate) type AidClocks<'w> = (Option<Res<'w, Time>>, Option<Res<'w, crate::pause::PlayClock>>, Option<Res<'w, crate::pause::WorldAidClock>>);

/// What E acts on: the target, a queued activation, VATS mode or the weapon wheel holding
/// the keys, the grab.
type ActivateFrom<'w> = (Res<'w, Target>, Option<ResMut<'w, ActivateNext>>, (Option<Res<'w, crate::vats_mode::VatsMode>>, Option<Res<'w, crate::weapon_wheel::WeaponWheel>>), Option<ResMut<'w, crate::grab::GrabState>>, Option<ResMut<'w, touch::TouchActivation>>);

/// Activate (E) activates the target; the outcome's effects go out as messages.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn activate(
    input: crate::bindings::ControlInput,
    (time, clock, world): AidClocks,
    auto: Option<ResMut<AutoActivate>>,
    (target, next, (vats, wheel), mut grab, mut touched): ActivateFrom,
    mut sim: ResMut<SimState>,
    play: Res<PlayWorld>,
    mut hud: ResMut<HudMessage>,
    mut travel: MessageWriter<TravelRequest>,
    mut open: MessageWriter<OpenContainer>,
    mut changed: MessageWriter<RefChanged>,
    (mut sit, mut sounds, mut terminals, mut lockpicks): (
        MessageWriter<crate::furniture::UseFurniture>,
        MessageWriter<crate::world_sounds::Activated>,
        MessageWriter<crate::terminal::OpenTerminal>,
        MessageWriter<crate::lockpick::OpenLockpick>,
    ),
    (mut seat, mut steal, mut ui, mut screen): (ResMut<crate::furniture::PlayerSeat>, MessageWriter<crate::pickpocket::OpenPickpocket>, MessageWriter<crate::ui_sounds::UiSound>, Option<ResMut<crate::loading::Loading>>),
    script: Option<ResMut<crate::script::ScriptState>>,
    locks: Option<Res<crate::control::ControlLocks>>,
    stats: Option<ResMut<crate::control::LockStats>>,
    (mut mines, mut disarm_rolls, mut hold, mut used): (Option<ResMut<crate::mines::Mines>>, Local<u64>, Local<Option<(u32, f32, bool)>>, Option<ResMut<Messages<crate::ItemUsed>>>),
) {
    // A reference queued by a lock just opened, or by the perks' choice list answered
    // with the default action: activated as if E were pressed on it.
    let mut script = script;
    let touched=touched.as_deref_mut().and_then(|t|t.take_ready(&input))
        .filter(|_|!input.just_pressed(crate::bindings::Control::Activate));
    let again = script.as_mut().and_then(|s| s.rt.as_mut()).and_then(|rt| rt.activate_again.take()).map(|f| (f.0, "the perks' choice list chose its default action"));
    let queued = next.and_then(|mut n| n.0.take()).map(|id| (id, "its lock was just opened")).or(again).and_then(|(id, why)| {
        let (name, action) = describe(&sim, play.index, id)?;
        info!("activate 0x{id:08X} {name}: {why}");
        Some(Targeted { form_id: id, name, action, distance: 0.0, source: Source::Bounds, crime: false, empty: false, through: None })
    });
    // A press held off because the target can be held (`crate::grab`): activated now
    // that E came up before the hold picked it up.
    let tapped = grab.as_mut().and_then(|g| g.released_tap(input.pressed(crate::bindings::Control::Activate)));
    let queued = queued.or(tapped);
    let hold_dt = time.as_ref().map_or(0.0, |t| t.delta_secs());
    let fired = match (auto, time) {
        // The aids' clock goes on while a menu holds the picture (`crate::pause`); a world
        // aid, it waits for the scripts with NVF_PLAY_AIDS_FROM_SCRIPTS (`WorldAidClock`).
        (Some(mut a), Some(t)) => {
            let now = t.elapsed_secs() + clock.map_or(0.0, |c| c.stopped_secs());
            world.map_or(Some(now), |w| w.at(now)).is_some_and(|now| a.fire(now))
        }
        _ => false,
    };
    // The player's Hold to Activate tweak: on the kinds it names E must be held
    // [`HOLD_ACTIVATE`] before it acts [design: docs/design/tweaks.md].
    let kinds = script.as_ref().and_then(|s| s.rt.as_ref()).map(|rt| HoldKinds::of(&rt.game.tweaks)).unwrap_or_default();
    let needs_hold = target.0.as_ref().filter(|t| kinds.needs(&sim, t));
    let mut pressed_now = input.just_pressed(crate::bindings::Control::Activate);
    if let Some(t) = needs_hold {
        let dt = hold_dt;
        let down = input.pressed(crate::bindings::Control::Activate);
        pressed_now = false;
        match hold.as_mut() {
            Some((id, secs, done)) if *id == t.form_id && down => {
                *secs += dt;
                if !*done && *secs >= HOLD_ACTIVATE {
                    *done = true;
                    pressed_now = true;
                    info!("activate 0x{:08X} {}: held {:.2} s (tweak)", t.form_id, t.name, *secs);
                }
            }
            _ if input.just_pressed(crate::bindings::Control::Activate) => *hold = Some((t.form_id, 0.0, false)),
            _ => *hold = None,
        }
    } else {
        *hold = None;
    }
    // Quick Loot has Activate while its list shows (it takes); its Wait asks for the
    // full menu, activated here as Activate would (`crate::quick_loot`) [design,
    // #2933].
    let full = crate::quick_loot::take_full_request();
    if full || crate::quick_loot::holds_input() {
        pressed_now = full;
        *hold = None;
    }
    if !pressed_now && !fired && queued.is_none() && touched.is_none() {
        return;
    }
    // VATS mode has Activate for itself (it fires the marks, docs/design/vats-mode.md §11.5).
    if vats.is_some_and(|v| v.holds_input()) {
        return;
    }
    // Nor while the weapon wheel is open (docs/design/weapon-wheel.md §5).
    if wheel.is_some_and(|w| w.holds_input()) {
        return;
    }
    // Scripts turned the player's movement off: world objects cannot be activated
    // [documented: GECK wiki DisablePlayerControls, MovementFlag].
    if locks.is_some_and(|l| l.movement) {
        info!("activate: ignored, a script has disabled the player's controls (movement)");
        if let Some(mut s) = stats {
            s.held[crate::control::group::MOVEMENT] += 1;
        }
        return;
    }
    let Some(t) = queued.as_ref().or(touched.as_ref()).or(target.0.as_ref()) else {
        // On furniture with nothing under the crosshair (lying on a bed, the bed is out
        // of view): Activate gets up [design].
        // (Not under the player's Activate Doesn't Stand Up tweak, docs/design/tweaks.md.)
        seat.leave |= seat.hold.is_some() && !crate::tweaks::SEAT_KEEPS_ON_ACTIVATE.load(std::sync::atomic::Ordering::Relaxed);
        return;
    };
    // E pressed on something the player can hold: a tap activates it when E comes up,
    // a hold picks it up (`crate::grab`, docs/spec-notes/grab.md §2).
    if queued.is_none() && touched.is_none() && !fired && !full {
        if let Some(g) = grab.as_mut().filter(|g| g.holdable(t.form_id) && g.tap.is_none()) {
            let candidate = g.candidate.expect("holdable has a candidate");
            g.tap = Some(crate::grab::Tap { target: Some(t.clone()), candidate, held: 0.0 });
            return;
        }
    }
    // A downed co-op player: the revive takes E (`nvforever`'s `coop.rs`).
    if matches!(t.action, Action::Revive { .. }) {
        return;
    }
    // An actor shown by name only (hostile, or a creature with nothing to say): E does
    // nothing (playtest list 4, item 19) except:
    // - open the perks' choices, if they offer any on it now (Robotics Expert's, #3479);
    // - for a non-hostile creature shown by name only (`Action::Name`) whose script has
    //   an OnActivate block (e.g. CRBrahminScript brahmin pushing, #3669), run its
    //   OnActivate block while keeping the default activation behind it "nothing".
    // Hostile actors keep #2536's rule.
    if matches!(t.action, Action::Name | Action::Hostile) {
        if crate::script::choices_by_hand(script.as_deref_mut(), &mut sim, t.form_id) {
            return;
        }
        if t.action == Action::Name {
            if let Some(done) = crate::script::script_activate_by_hand(script.as_deref_mut(), &mut sim, t.form_id) {
                match done {
                    Ok(crate::script::ByHand::Script) => {
                        sounds.write(crate::world_sounds::Activated { reference: t.form_id });
                        info!("activate 0x{:08X} {} ({}): its script's OnActivate ran", t.form_id, t.name, t.action.label());
                    }
                    Ok(crate::script::ByHand::Refused) => {
                        info!("activate 0x{:08X} {}: destroyed or prevented, it cannot be activated", t.form_id, t.name);
                    }
                    Ok(crate::script::ByHand::Talk) => {
                        info!("activate 0x{:08X} {} ({}): a conversation started", t.form_id, t.name, t.action.label());
                    }
                    Ok(crate::script::ByHand::Greeted) => {
                        info!("activate 0x{:08X} {} ({}): it said its greeting, no conversation", t.form_id, t.name, t.action.label());
                    }
                    Ok(crate::script::ByHand::Woken) => {
                        info!("activate 0x{:08X} {} ({}): asleep, it wakes and gets up before talking", t.form_id, t.name, t.action.label());
                    }
                    Ok(crate::script::ByHand::Wheel) => {
                        info!("activate 0x{:08X} {} ({}): the companion wheel opened", t.form_id, t.name, t.action.label());
                    }
                    Ok(_) => {}
                    Err(e) => {
                        warn!("activate 0x{:08X} {}: {e}", t.form_id, t.name);
                        hud.show(format!("{}: {e}", t.name));
                    }
                }
                return;
            }
        }
        info!("activate 0x{:08X} {}: nothing to do ({})", t.form_id, t.name, if t.action == Action::Hostile { "hostile" } else { "name only" });
        return;
    }
    if t.action == Action::Steal {
        let refusal = script.as_deref_mut().and_then(|s| s.rt.as_mut()).map(|rt| crate::script::with_world(rt, &mut sim, |rt| rt.game.pickpocket_refusal(FormId(t.form_id))));
        match refusal {
            Some(None) => {
                info!("activate 0x{:08X} {} (Steal): its pocket opens", t.form_id, t.name);
                steal.write(crate::pickpocket::OpenPickpocket { victim: t.form_id });
            }
            Some(Some(fnv_game::pickpocket::Refusal::CaughtBefore)) => {
                let line = crate::pickpocket::caught_before_line(&sim, &t.name);
                info!("activate 0x{:08X} {} (Steal): refused, {line}", t.form_id, t.name);
                hud.show(line);
            }
            _ => info!("activate 0x{:08X} {} (Steal): it cannot be pickpocketed now", t.form_id, t.name),
        }
        return;
    }
    if t.action == Action::Inaccessible {
        info!("activate 0x{:08X} {}: nothing to do (an inaccessible door)", t.form_id, t.name);
        return;
    }
    // The player's tweak: a Stimpak gets a knocked-out companion up [design: docs/design/tweaks.md].
    if t.action == Action::ReviveCompanion {
        let done = script.as_deref_mut().and_then(|s| s.rt.as_mut()).map(|rt| crate::script::with_world(rt, &mut sim, |rt| rt.game.revive(FormId(t.form_id))));
        match done {
            Some(Ok(())) => info!("activate 0x{:08X} {} (Revive): a Stimpak used, they get up (tweak)", t.form_id, t.name),
            Some(Err(fnv_game::unconscious::ReviveRefusal::NoStimpak)) => {
                info!("activate 0x{:08X} {} (Revive): no Stimpak (tweak)", t.form_id, t.name);
                hud.show(REVIVE_NEEDS_STIMPAK.to_string());
            }
            _ => info!("activate 0x{:08X} {} (Revive): they cannot be revived now", t.form_id, t.name),
        }
        return;
    }
    // The player's tweak: a mine's disarm is a roll on the player's skill against the
    // mine's skill requirement, and a failed one may set it off [design: docs/design/tweaks.md].
    if t.action == Action::Disarm {
        let item = sim.base_of(t.form_id).and_then(|b| sim.forms.get(b)).and_then(|b| match b.data {
            BaseData::Projectile { item, .. } => item,
            _ => None,
        });
        let chances = item.zip(script.as_deref_mut().and_then(|s| s.rt.as_mut())).and_then(|(item, rt)| crate::script::with_world(rt, &mut sim, |rt| rt.game.disarm_chances(item)));
        if let Some((disarm, explode)) = chances {
            let mut roll = || {
                *disarm_rolls = disarm_rolls.wrapping_add(0x9E37_79B9_7F4A_7C15 ^ u64::from(t.form_id));
                let mut z = *disarm_rolls;
                z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
                z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
                ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64 * 100.0
            };
            if roll() >= disarm {
                if roll() < explode {
                    info!("activate 0x{:08X} {}: the disarm fails and sets it off ({disarm:.0} % to disarm, {explode:.0} % to go off; tweak)", t.form_id, t.name);
                    if let Some(m) = mines.as_mut() {
                        m.set_off.push(FormId(t.form_id));
                    }
                } else {
                    info!("activate 0x{:08X} {}: the disarm fails ({disarm:.0} % to disarm; tweak)", t.form_id, t.name);
                    hud.show(crate::tweaks::DISARM_FAILED_LINE.to_string());
                }
                return;
            }
        }
    }
    // The actor's place before, to put back if a teleport cannot be followed.
    let before = sim.world.peek(FormId::PLAYER_REF).map(|p| (p.cell, p.pos, p.rot));
    // The player's Quick Use tweak: Shift held as an item is taken [design: docs/design/tweaks.md].
    let quick = script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.quick_use) && input.keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    // Picked through bars or a fence (#3233, [`see_through`]): E only opens the
    // conversation; no OnActivate block, perks' choices, companion wheel or waking.
    if let Some(bars) = t.through {
        match crate::script::talk_by_hand(script.map(|s| s.into_inner()), &mut sim, t.form_id) {
            Ok(crate::script::ByHand::Talk) => info!("activate 0x{:08X} {} (Talk through 0x{bars:08X}): a conversation started", t.form_id, t.name),
            Ok(crate::script::ByHand::Greeted) => info!("activate 0x{:08X} {} (Talk through 0x{bars:08X}): it said its greeting, no conversation", t.form_id, t.name),
            Ok(_) => info!("activate 0x{:08X} {} (Talk through 0x{bars:08X}): no conversation", t.form_id, t.name),
            Err(e) => {
                info!("activate 0x{:08X} {} (Talk through 0x{bars:08X}): no conversation: {e}", t.form_id, t.name);
                hud.show(format!("{} has nothing to say", t.name));
            }
        }
        return;
    }
    let mut script = script.map(|s| s.into_inner());
    let a = match crate::script::activate_by_hand(script.as_deref_mut(), &mut sim, t.form_id) {
        Ok(crate::script::ByHand::Default(a)) => {
            sounds.write(crate::world_sounds::Activated { reference: t.form_id });
            if t.action == Action::Drink {
                info!("activate 0x{:08X} {}: the player drinks from it", t.form_id, t.name);
            }
            a
        }
        Ok(crate::script::ByHand::Script) => {
            sounds.write(crate::world_sounds::Activated { reference: t.form_id });
            info!("activate 0x{:08X} {} ({}): its script's OnActivate ran", t.form_id, t.name, t.action.label());
            return;
        }
        Ok(crate::script::ByHand::Talk) => {
            // The dialogue screen opens (`crate::dialogue`).
            info!("activate 0x{:08X} {} ({}): a conversation started", t.form_id, t.name, t.action.label());
            return;
        }
        Ok(crate::script::ByHand::Woken) => {
            info!("activate 0x{:08X} {} ({}): asleep, it wakes and gets up before talking", t.form_id, t.name, t.action.label());
            return;
        }
        Ok(crate::script::ByHand::Wheel) => {
            info!("activate 0x{:08X} {} ({}): the companion wheel opened", t.form_id, t.name, t.action.label());
            return;
        }
        Ok(crate::script::ByHand::Choices) => {
            info!("activate 0x{:08X} {} ({}): the perks' choice list opened", t.form_id, t.name, t.action.label());
            return;
        }
        Ok(crate::script::ByHand::Refused) => {
            info!("activate 0x{:08X} {}: destroyed, it cannot be activated", t.form_id, t.name);
            return;
        }
        Ok(crate::script::ByHand::Greeted) => {
            // Heard and shown like a SayTo line; the player stays free.
            info!("activate 0x{:08X} {} ({}): it said its greeting, no conversation", t.form_id, t.name, t.action.label());
            return;
        }
        Err(e) => {
            warn!("activate 0x{:08X} {}: {e}", t.form_id, t.name);
            hud.show(format!("{}: {e}", t.name));
            return;
        }
    };
    info!("activate 0x{:08X} {} ({}): {:?}", t.form_id, t.name, t.action.label(), a);
    // Quick Use: a weapon taken goes in hand, an aid item is used, apparel but armour goes
    // on, as its hotkey would (`crate::hotkeys::use_slot`) [design: docs/design/tweaks.md].
    if let (true, Outcome::PickedUp { item, .. }) = (quick, &a.outcome) {
        let armour = sim.forms.item(*item).is_some_and(|i| i.kind == ItemKind::Armor && i.armor_dt.is_some_and(|dt| dt > 0.0));
        if !armour {
            let allowed = match script.and_then(|s| s.rt.as_mut()).filter(|rt| rt.game.has_pre_activate_inventory_handlers()) {
                Some(rt) => crate::script::with_world(rt, &mut sim, |rt| rt.game.pre_activate_inventory_item(FormId::PLAYER_REF, *item, 0, true)),
                None => true,
            };
            if allowed {
                let r = crate::hotkeys::use_slot(&mut sim.world, Some(crate::hotkeys::Slot { item: item.0, mods: 0 }));
                if let crate::hotkeys::Used::UsedUp(u) = r {
                    if let Some(m) = used.as_mut() {
                        m.write(crate::ItemUsed { item: u.0, limb: None });
                    }
                }
                info!("activate 0x{:08X} {}: quick use: {} (tweak)", t.form_id, t.name, r.describe(&t.name));
            } else {
                info!("activate 0x{:08X} {}: quick use prevented by script", t.form_id, t.name);
            }
        }
    }
    // Opened with the player's key: the lock's opening sound, as a picked lock's
    // (`crate::lockpick`) [owner 2026-10-27; hypothesis: the original's sound for a key].
    if a.unlocked_with.is_some() {
        info!("activate 0x{:08X} {}: unlocked with the key, the unlock sound plays", t.form_id, t.name);
        ui.write(crate::ui_sounds::UiSound { menu: crate::ui_sounds::UiMenu::Lockpick, event: crate::ui_sounds::UiEvent::Unlocked });
    }
    // Furniture's default activation: the player uses it (`crate::furniture`).
    let furniture = sim.base_of(t.form_id).and_then(|b| sim.forms.get(b)).is_some_and(|b| matches!(b.data, BaseData::Furniture));
    if furniture && matches!(a.outcome, Outcome::Activated) {
        sit.write(crate::furniture::UseFurniture { reference: t.form_id });
        return;
    }
    // A terminal's default activation: its screen opens (`crate::terminal`).
    let terminal = sim.base_of(t.form_id).and_then(|b| sim.forms.get(b)).is_some_and(|b| &b.sig == b"TERM");
    if terminal && matches!(a.outcome, Outcome::Activated) {
        terminals.write(crate::terminal::OpenTerminal { reference: t.form_id });
        return;
    }
    // Locked, without the key: the lockpicking menu, or a line saying why not
    // (`crate::lockpick`, docs/spec-notes/lockpicking.md §4).
    if matches!(a.outcome, Outcome::Locked { .. }) {
        lockpicks.write(crate::lockpick::OpenLockpick { reference: t.form_id });
        return;
    }
    let name_of = |id: FormId| sim.name(id).map_or_else(|| id.to_string(), str::to_string);
    let effects = outcome::effects(&a, t.form_id, &t.name, play.index, name_of);
    let travels = effects.iter().any(|e| matches!(e, Effect::Travel { .. }));
    if matches!(a.outcome, Outcome::Teleported { .. }) && !travels {
        if let (Some((cell, pos, rot)), Ok(p)) = (before, sim.world.get(FormId::PLAYER_REF)) {
            (p.cell, p.pos, p.rot) = (cell, pos, rot);
        }
    }
    for e in effects {
        match e {
            Effect::RefChanged(form_id) => {
                changed.write(RefChanged::new(form_id));
            }
            Effect::Travel { world, interior, feet, heading_deg } => {
                let to = match (interior, world) {
                    (Some(c), _) => format!("interior 0x{c:08X}"),
                    (None, Some(w)) => format!("worldspace 0x{w:08X}"),
                    (None, None) => "the worldspace".to_string(),
                };
                info!("travel to {to} at {feet:?} heading {heading_deg:.1}");
                // The player's own trip through a load door (`Loading::arriving`).
                if let Some(s) = screen.as_deref_mut() {
                    s.arriving = true;
                }
                travel.write(TravelRequest(Travel { world, interior, feet, heading_deg, pitch_deg: None }));
            }
            Effect::OpenContainer(container) => {
                open.write(OpenContainer { container });
            }
            Effect::Message(m) => {
                info!("message: {m}");
                hud.show(m);
            }
        }
    }
}

// ---- visual sync -----------------------------------------------------------------

type NewVisuals<'w, 's> = Query<'w, 's, (&'static RefVisual, &'static mut Visibility), Added<RefVisual>>;
type AllVisuals<'w, 's> = Query<'w, 's, (&'static RefVisual, &'static mut Visibility)>;

/// Whether a reference is drawn: present in the simulation ([`SimState::present`]) and not
/// culled by its critical stage (GooEnd, DisintegrateEnd: `fnv_game::Game::culled`). A
/// hidden actor has no hit volume (`crate::hitvol`) and its corpse leaves the physics world
/// (nvforever `ragdoll.rs`): "cull its 3D, and remove its Havok data" [documented: GECK
/// wiki SetCriticalStage].
pub(crate) fn drawn(sim: &SimState, script: Option<&crate::script::ScriptState>, id: u32) -> bool {
    sim.present(id) && !script.and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.culled(FormId(id)))
}

/// Whether a `RefChanged` entry should emit its own `info!` line: always for live
/// changes (`!all_restored`) and for any restored reference that actually updated drawn
/// render entities (`updated > 0`). Restored references in unloaded cells (`updated == 0`)
/// are counted into a single summary `info!` line so loading a mature save does not flood
/// the log with tens of thousands of no-op lines.
fn log_ref_change_individually(all_restored: bool, updated: usize) -> bool {
    !all_restored || updated > 0
}

/// Hide or show `RefVisual` entities from the simulation's state ([`drawn`]): every entity
/// of a reference named by `RefChanged`, and new entities as cells stream in (a taken item
/// must not come back when its cell reloads).
pub fn sync_visuals(
    sim: Res<SimState>,
    script: Option<Res<crate::script::ScriptState>>,
    mut changes: MessageReader<RefChanged>,
    mut visuals: ParamSet<(NewVisuals, AllVisuals)>,
) {
    let script = script.as_deref();
    let mut present: HashMap<u32, bool> = HashMap::new();
    for (r, mut v) in &mut visuals.p0() {
        if !*present.entry(r.0).or_insert_with(|| drawn(&sim, script, r.0)) {
            v.set_if_neq(Visibility::Hidden);
        }
    }
    let mut changed: HashMap<u32, (usize, bool)> = HashMap::new();
    for m in changes.read() {
        changed
            .entry(m.form_id)
            .and_modify(|(_, all_restored)| *all_restored &= m.restored)
            .or_insert((0, m.restored));
    }
    if changed.is_empty() {
        return;
    }
    for (r, mut v) in &mut visuals.p1() {
        if let Some((n, _)) = changed.get_mut(&r.0) {
            v.set_if_neq(if drawn(&sim, script, r.0) { Visibility::Inherited } else { Visibility::Hidden });
            *n += 1;
        }
    }
    let mut restored_unloaded = 0usize;
    for (id, (n, all_restored)) in changed {
        let how = if !sim.present(id) {
            "gone"
        } else if drawn(&sim, script, id) {
            "present"
        } else {
            "culled (critical stage)"
        };
        if log_ref_change_individually(all_restored, n) {
            info!("reference 0x{id:08X} {how}: {n} render entities updated");
        } else {
            restored_unloaded += 1;
            debug!("reference 0x{id:08X} {how}: 0 render entities updated");
        }
    }
    if restored_unloaded > 0 {
        info!("references restored from save: {restored_unloaded} not drawn (0 render entities updated)");
    }
}
