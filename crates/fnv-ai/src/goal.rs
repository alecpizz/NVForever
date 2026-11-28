//! Where a package sends its actor: the package's location (PLDT) turned into a place
//! on the navmesh. Which package types go to their location, and what each location
//! kind means: docs/spec-notes/packages.md §3 [documented: GECK wiki, Category:Packages
//! "Standard Location" and the page of each type]; what is not carried out yet:
//! docs/design/actor-behaviour.md §5. A Dialogue package has no place to stay at but
//! someone to talk to ([`Talk`], docs/design/actor-behaviour.md §5.6). Escort, Find, Use
//! Item At, Ambush and Flee Not Combat packages have goals of their own ([`Escort`],
//! [`Find`], [`UseAt`], [`Ambush`], [`Flee`]; packages.md §3.1), carried out in
//! `crate::errand`.

use fnv_game::game::Game;
use fnv_game::package::{furniture, Location, LocationKind, PackageDef, PackageType, TargetKind};
use fnv_nav::graph::NONE;
use fnv_nav::{NavGraph, Place, Space};
use fnv_sim::{FormId, RefTable};

/// What an actor has to do to be where its package wants it.
#[derive(Debug, Clone, PartialEq)]
pub enum Goal {
    /// Nothing to walk to: the package acts where the actor is ("near current
    /// location", or no location).
    Here,
    /// Be within `radius` of a place.
    Near {
        place: Place,
        radius: f32,
        /// The effective destination selected for this walk, retained for arrival
        /// facing without another object search [design, #2526; `stay_at`, `resolve`].
        reference: Option<FormId>,
    },
    /// Be anywhere inside an interior cell ("in cell": the radius is not used); `at` is
    /// a point of the cell's navmesh to walk towards.
    Inside { space: Space, at: Place },
    /// Walk a patrol route: the location's reference and the references linked from it.
    Route(Route),
    /// Talk to someone: a Dialogue package.
    Talk(Talk),
    /// Keep near someone, wherever it goes: a Follow or Accompany package.
    Follow(Follow),
    /// Lead someone to a place: an Escort package.
    Escort(Escort),
    /// Look for objects in an area and take, use or go up to them: a Find package.
    Find(Find),
    /// Go to an object and use it: a Use Item At package.
    UseAt(UseAt),
    /// Wait at a place for an enemy to come into another, then fight: an Ambush package.
    Ambush(Ambush),
    /// Get away from a threat, without fighting: a Flee Not Combat package.
    Flee(Flee),
    /// The package type or the location kind is not carried out yet: the actor stays
    /// where it is.
    NotCarriedOut(&'static str),
    /// The location names something that is not in the world: the package cannot be
    /// carried out at all.
    Missing(&'static str),
}

/// A patrol route.
#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    /// The patrol points in order (the reference, where it is).
    pub points: Vec<(FormId, Place)>,
    /// The chain of linked references comes back to this point after the last one.
    pub loops_to: Option<usize>,
    /// PKPT "repeatable".
    pub repeatable: bool,
}

/// What a Dialogue package has its actor do [documented: GECK wiki, Dialogue Package]:
/// go to the wait location, wait there for the target, go up to it and talk.
#[derive(Debug, Clone, PartialEq)]
pub struct Talk {
    /// Whom the actor talks to (the package's target).
    pub target: FormId,
    /// "Activate Distance": how near the target the actor has to be to start talking
    /// (the target's count / distance); [`STANDARD_TALK_DISTANCE`] when the record has 0.
    pub distance: f32,
    /// The "Start/Wait Location" (location 1) as a place to stay at: [`Goal::Near`],
    /// [`Goal::Inside`], or [`Goal::Here`] when the package has none.
    pub wait: Box<Goal>,
    /// The wait location's radius.
    pub wait_radius: f32,
    /// The "Trigger Location" (location 2).
    pub trigger: Trigger,
    /// The topic the conversation starts with, or the one line of a Say To package;
    /// none: the greeting.
    pub topic: Option<FormId>,
    /// "Say To": one line said to the target, no conversation.
    pub say_to: bool,
    /// PKDD "No Headtracking": speaker and target are not made to look at each other.
    pub no_headtracking: bool,
    /// PKDD "Do Not Control Target Movement" off: with a trigger location, the target
    /// is to be in it when the conversation starts (`fnv_ai::carry`, `talk_step`).
    pub control_target: bool,
}

/// What a Follow or Accompany package has its actor do: keep within `distance` of its
/// target, wherever the target goes (through doors too), and stand while the target
/// stands [documented: GECK wiki Category:Packages, "Follow/Escort/Accompany: Idles
/// play while not moving but waiting for target to move"; hypothesis: that the
/// target's count / distance is how near it keeps].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Follow {
    pub target: FormId,
    /// Game units; [`STANDARD_FOLLOW_DISTANCE`] when the record has 0.
    pub distance: f32,
}

/// How near a follower keeps to a target whose package gives no distance [our choice].
pub const STANDARD_FOLLOW_DISTANCE: f32 = 256.0;

/// What an Escort package has its actor do: go to `to` (the package's location) with
/// `target` (the package's target) and stop to wait whenever the target is more than
/// `distance` away (PKE2) [documented: Category:Packages, "escort leads it to a
/// destination", "Follow/Escort/Accompany: Idles play while not moving but waiting for
/// target to move"; hypothesis: that PKE2 is the distance at which it stops to wait;
/// packages.md §3.1].
#[derive(Debug, Clone, PartialEq)]
pub struct Escort {
    pub target: FormId,
    /// [`Goal::Near`] or [`Goal::Inside`].
    pub to: Box<Goal>,
    /// Game units; [`STANDARD_ESCORT_DISTANCE`] when the record gives none.
    pub distance: f32,
}

/// The escort distance of a package without one [our choice: the most common PKE2 of
/// the masters, 7 of the 13 Escort packages].
pub const STANDARD_ESCORT_DISTANCE: f32 = 200.0;

/// What a Find or Use Item At package looks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wanted {
    /// One placed reference.
    Reference(FormId),
    /// References of this base form, or of the bases in this form list, or actors of
    /// this faction or drawn from this leveled list.
    Base(FormId),
    /// References of an object type (PTDT object type code, `object_type_name`).
    Type(u32),
    /// Nothing: object type "None" [observed: 3 of the masters' Find packages], or no
    /// target at all.
    Nothing,
}

/// An area searched for objects: within `radius` of `centre` (the whole cell when the
/// location is "in cell": radius infinite); `centre` `None`: around where the actor is
/// when it searches.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    pub centre: Option<Place>,
    pub radius: f32,
}

impl Area {
    /// No area at all: what it looks for counts wherever it is, in any cell (a Find for
    /// one reference whose location gives no radius, packages.md §10).
    pub const ANYWHERE: Area = Area { centre: None, radius: f32::INFINITY };

    pub fn is_anywhere(&self) -> bool {
        self.centre.is_none() && self.radius == f32::INFINITY
    }
}

/// A search area of radius 0 is this wide [our choice: 16 of the masters' Find and Use
/// Item At packages have radius 0 and name an object to find around the location].
pub const STANDARD_SEARCH_RADIUS: f32 = 512.0;

/// What a Find package has its actor do [documented: Category:Packages, "Standard
/// Search Data" (search location, Allow Buying / Killing / Stealing), Must Complete "the
/// target must be found with a find", "Find: Idles play when 'activating' the target of
/// the find"; Combat Package Flags, "travel behavior on the way to the object"]: go to the
/// search location, then to each object it looks for in the area, nearest first, and
/// take it (an item), harvest it (flora), use it (furniture), or go up to it; `count`
/// of them (the target's count) [hypothesis for what is done with each kind;
/// packages.md §3.1].
#[derive(Debug, Clone, PartialEq)]
pub struct Find {
    /// The search location as a place to go first ([`Goal::Here`] without one).
    pub search: Box<Goal>,
    pub area: Area,
    pub wanted: Wanted,
    /// How many to find (at least 1).
    pub count: u32,
    /// For a specific reference: how near it to go (the target's distance; 0: up to it).
    pub distance: f32,
    /// Allow Killing: actors carrying what it looks for (and actors it looks for) are
    /// attacked; Allow Stealing: items owned by others are taken too; Allow Buying: a
    /// vendor whose merchant container holds what it looks for is gone to and it buys
    /// one (`crate::errand`, packages.md §3.1).
    pub kill: bool,
    pub steal: bool,
    pub buy: bool,
}

/// What a Use Item At package has its actor do [documented: Category:Packages "Use
/// Item: Idles play at idle location"; Combat Package Flags, "travel behavior on the way
/// to the fire location"]: go to the object the package names (or the nearest of its
/// kind in the area) and use it: sit on furniture, stand at a marker facing its way, or
/// use an item it carries [hypothesis for the kinds; packages.md §3.1].
#[derive(Debug, Clone, PartialEq)]
pub struct UseAt {
    pub at: Box<Goal>,
    pub area: Area,
    pub wanted: Wanted,
    /// "Sit Down": only furniture to sit on is chosen [hypothesis].
    pub sit_down: bool,
}

/// What an Ambush package has its actor do [documented: Category:Packages, "wait at a
/// location until enemies enter another"; Combat Package Flags, "travel behavior on
/// the way to the ambush location"; observed: GoodspringsSettler04Ref under
/// GSSettlerAmbushPackage went to its location (procedure TRAVEL) and waited there
/// (procedure WAIT_AMBUSH), not sneaking although the package says Hide While
/// Ambushing, packages.md §4]: go to `wait`, wait there, and fight the first enemy in
/// `trigger`.
#[derive(Debug, Clone, PartialEq)]
pub struct Ambush {
    pub wait: Box<Goal>,
    pub trigger: Trigger,
    /// The package's target: only it is ambushed (a reference), or only members of a
    /// faction or kind (a base form); [`Wanted::Nothing`]: any actor that the ambusher
    /// would fight.
    pub target: Wanted,
    /// Hide While Ambushing: only reported (it does not make the actor sneak [observed]).
    pub hide: bool,
}

/// What a Flee Not Combat package has its actor do [documented: Category:Packages,
/// "flee to or from somewhere", "Flee: Idles play when at flee destination"; IsFleeing,
/// "running a Flee-type AI package"]: run from its threat (the package's target, else an
/// actor, when it is an enemy) to the package's location, or, without one, until it is
/// `distance` from the threat; it does not fight [hypothesis for the threat and the
/// distance; packages.md §3.1].
#[derive(Debug, Clone, PartialEq)]
pub struct Flee {
    pub from: Option<FormId>,
    /// Game units; [`flee_distance`] when the target names none.
    pub distance: f32,
    pub to: Option<Box<Goal>>,
}

/// How far a fleeing actor without a destination gets from its threat when the package
/// names no distance and the game settings are missing [our choice: fCombatFleeNormalDistance's
/// default, 2048, Combat Flee]. With the masters loaded [`flee_distance`] reads
/// `fFleeDistanceExterior` / `fFleeDistanceInterior` instead.
pub const STANDARD_FLEE_DISTANCE: f32 = 2048.0;

/// The distance a Flee package without one of its own runs to: `fFleeDistanceInterior`
/// (3000 in FalloutNV.esm) indoors and `fFleeDistanceExterior` (5000) outdoors [observed:
/// the masters' values; hypothesis: the settings are for flee packages, by their names, as
/// the `fCombatFlee…` ones are for combat fleeing (`crate::alert`)].
pub fn flee_distance(game: &Game, actor: FormId) -> f32 {
    let indoors = place_of_ref(game, actor).is_some_and(|p| matches!(p.space, Space::Interior(_)));
    let name = if indoors { "fFleeDistanceInterior" } else { "fFleeDistanceExterior" };
    game.world.setting_f32(name).filter(|d| *d > 0.0).unwrap_or(STANDARD_FLEE_DISTANCE)
}

/// The activate distance of a Dialogue package that gives none [documented: GECK wiki,
/// Dialogue Package: "The standard distance is 150, as defined in the
/// iActivatePickLength game setting"; hypothesis: that a stored 0 means the standard
/// distance; 54 of FalloutNV.esm's 293 Dialogue packages aimed at the player store 0].
pub const STANDARD_TALK_DISTANCE: f32 = 150.0;

/// Where the target of a Dialogue package has to be for the actor to set off towards it
/// [documented: GECK wiki, Dialogue Package, Trigger Location].
#[derive(Debug, Clone, PartialEq)]
pub enum Trigger {
    /// The package has no trigger location: the actor goes up to the target at once.
    None,
    /// Within `radius` of a reference, or inside the reference's own volume when it
    /// carries one. [observed: 32 of the 33 trigger locations of FalloutNV.esm that name
    /// a reference with radius 0 name a box primitive, an activator placed for it such
    /// as SunnySmilesTrigger; of the 29 with a radius, 5 do; packages.md §2.
    /// hypothesis: that the game tests the volume; the documentation only speaks of
    /// the radius.]
    NearReference { reference: FormId, radius: f32 },
    /// Within `radius` of a fixed place (the actor's editor location).
    Near { place: Place, radius: f32 },
    /// Within `radius` of where the actor is now.
    NearActor { radius: f32 },
    /// In this cell.
    InCell(FormId),
    /// A trigger location that names nothing in the world, or is of a kind not
    /// understood: the target is never in it.
    Never,
}

/// How far inside a trigger box's sides a target is walked to ([`Trigger::lead_point`])
/// [our choice].
const LEAD_MARGIN: f32 = 48.0;
/// The share of a trigger radius a target is walked to within [our choice].
const LEAD_INSIDE_RADIUS: f32 = 0.75;

/// How much of an actor is tested against a trigger location's volume: its feet, its
/// middle and its head, this far apart [our choice; the game tests the actor's
/// collision object against trigger volumes, docs/spec-notes/level-design.md §11.2].
const BODY_STEP: f32 = 64.0;

/// Whether a point is inside the volume a reference carries (REFR XPRM), where the
/// reference is now.
fn in_volume(game: &Game, reference: FormId, p: [f32; 3]) -> bool {
    let Some(placed) = game.world.placed.get(reference) else { return false };
    let Some(prim) = placed.primitive else { return false };
    let (pos, rot) = match game.world.peek(reference) {
        Some(s) => (s.pos, s.rot),
        None => (placed.pos, placed.rot),
    };
    // world = pos + R·local·scale, R row-major (`fnv_world::transform::ref_rotation`).
    let r = fnv_world::transform::ref_rotation(rot);
    let scale = placed.scale.filter(|s| *s > 0.0).unwrap_or(1.0);
    let d = [p[0] - pos[0], p[1] - pos[1], p[2] - pos[2]];
    let local = [0, 1, 2].map(|i| (r[i] * d[0] + r[3 + i] * d[1] + r[6 + i] * d[2]) / scale);
    if prim.sphere {
        local.iter().map(|v| v * v).sum::<f32>() <= prim.bounds[0] * prim.bounds[0]
    } else {
        (0..3).all(|i| local[i].abs() <= prim.bounds[i])
    }
}

impl Trigger {
    /// Whether the target, standing at `target`, is in the trigger location of an
    /// actor standing at `feet`. Without a trigger location this always holds.
    pub fn holds(&self, game: &Game, feet: Place, target: Place) -> bool {
        let near = |place: Place, radius: f32| place.space == target.space && radius > 0.0 && dist(place.pos, target.pos) <= radius;
        match self {
            Trigger::None => true,
            Trigger::NearReference { reference, radius } => {
                let Some(place) = place_of_ref(game, *reference) else { return false };
                let body = |i: usize| [target.pos[0], target.pos[1], target.pos[2] + BODY_STEP * i as f32];
                near(place, *radius) || place.space == target.space && (0..3).any(|i| in_volume(game, *reference, body(i)))
            }
            Trigger::Near { place, radius } => near(*place, *radius),
            Trigger::NearActor { radius } => near(feet, *radius),
            Trigger::InCell(cell) => cell_of_place(game.world.placed, &target) == Some(*cell),
            Trigger::Never => false,
        }
    }

    /// Where to walk a target standing at `target` so that it is in the trigger
    /// location: the nearest point a little inside its volume, or inside its radius,
    /// at the target's height. `None` for a cell or no room.
    pub fn lead_point(&self, game: &Game, feet: Place, target: Place) -> Option<[f32; 3]> {
        let toward = |centre: Place, radius: f32| -> Option<[f32; 3]> {
            if centre.space != target.space || radius <= 0.0 {
                return None;
            }
            let (dx, dy) = (target.pos[0] - centre.pos[0], target.pos[1] - centre.pos[1]);
            let len = dx.hypot(dy);
            let keep = radius * LEAD_INSIDE_RADIUS;
            if len <= keep {
                return Some(target.pos);
            }
            Some([centre.pos[0] + dx / len * keep, centre.pos[1] + dy / len * keep, target.pos[2]])
        };
        match self {
            Trigger::NearReference { reference, radius } => {
                let place = place_of_ref(game, *reference)?;
                if place.space != target.space {
                    return None;
                }
                match game.world.placed.get(*reference).and_then(|p| p.primitive.map(|prim| (p, prim))) {
                    Some((placed, prim)) if !prim.sphere => {
                        let (pos, rot) = game.world.peek(*reference).map_or((placed.pos, placed.rot), |s| (s.pos, s.rot));
                        let r = fnv_world::transform::ref_rotation(rot);
                        let scale = placed.scale.filter(|s| *s > 0.0).unwrap_or(1.0);
                        let d = [target.pos[0] - pos[0], target.pos[1] - pos[1], target.pos[2] - pos[2]];
                        let mut local = [0, 1, 2].map(|i| (r[i] * d[0] + r[3 + i] * d[1] + r[6 + i] * d[2]) / scale);
                        // x and y into the box, a margin inside its sides; the height kept.
                        for (i, l) in local.iter_mut().enumerate().take(2) {
                            let half = prim.bounds[i];
                            let keep = (half - LEAD_MARGIN.min(half / 2.0)).max(0.0);
                            *l = l.clamp(-keep, keep);
                        }
                        let w = [0, 1, 2].map(|i| (r[3 * i] * local[0] + r[3 * i + 1] * local[1] + r[3 * i + 2] * local[2]) * scale);
                        Some([pos[0] + w[0], pos[1] + w[1], target.pos[2]])
                    }
                    Some((_, prim)) => toward(place, prim.bounds[0]),
                    None => toward(place, *radius),
                }
            }
            Trigger::Near { place, radius } => toward(*place, *radius),
            Trigger::NearActor { radius } => toward(feet, *radius),
            Trigger::None | Trigger::InCell(_) | Trigger::Never => None,
        }
    }

    /// Whether the trigger location is a place a target can be in: a volume, a radius
    /// above 0 or a cell. A reference with radius 0 and no volume of its own is not.
    pub fn has_room(&self, game: &Game) -> bool {
        match self {
            Trigger::None | Trigger::Never => false,
            Trigger::NearReference { reference, radius } => *radius > 0.0 || game.world.placed.get(*reference).is_some_and(|p| p.primitive.is_some()),
            Trigger::Near { radius, .. } | Trigger::NearActor { radius } => *radius > 0.0,
            Trigger::InCell(_) => true,
        }
    }
}

/// Distance between two points of one space.
pub fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// The navmesh space of a cell.
pub fn space_of_cell(refs: &RefTable, cell: FormId) -> Space {
    match refs.world_of(cell) {
        Some(w) => Space::World(w.0),
        None => Space::Interior(cell.0),
    }
}

/// The cell of a place: the interior, or the exterior cell under the position.
pub fn cell_of_place(refs: &RefTable, place: &Place) -> Option<FormId> {
    match place.space {
        Space::Interior(c) => Some(FormId(c)),
        Space::World(w) => refs.world_grid.get(&FormId(w))?.get(&fnv_world::cell_of(place.pos[0], place.pos[1])).copied(),
    }
}

/// Where a reference is now (moved references included).
pub fn place_of_ref(game: &Game, r: FormId) -> Option<Place> {
    let refs = game.world.placed;
    let (cell, pos) = match game.world.peek(r) {
        Some(s) => (s.cell, s.pos),
        None => {
            let p = refs.get(r)?;
            (p.cell, p.pos)
        }
    };
    Some(Place { space: space_of_cell(refs, cell?), pos })
}

/// [hypothesis, #2652; docs/spec-notes/use-weapon.md section 6, observer check 207]
/// A Use Weapon target that is a non-actor placed reference in a sibling worldspace
/// shares the shooter's coordinates. Actors and interiors still require the same
/// space; worldspaces without a shared, nonempty immediate parent do not qualify.
pub fn weapon_target_place(game: &Game, target: FormId, space: Space) -> Option<Place> {
    let mut at = place_of_ref(game, target)?;
    if at.space == space {
        return Some(at);
    }
    let placed = game.world.placed.get(target)?;
    if placed.sig != *b"REFR" || fnv_game::ai::is_actor_base(game.world.forms, placed.base) {
        return None;
    }
    let (Space::World(shooter), Space::World(other)) = (space, at.space) else { return None };
    let parents = &game.world.placed.world_parents;
    let parent = parents.get(&FormId(shooter))?;
    if parents.get(&FormId(other)) != Some(parent) {
        return None;
    }
    at.space = space;
    Some(at)
}

/// Where a reference was placed in the editor.
pub fn editor_place(refs: &RefTable, r: FormId) -> Option<Place> {
    let p = refs.get(r)?;
    Some(Place { space: space_of_cell(refs, p.cell?), pos: p.pos })
}

/// A reference's editor location: where the plugins place it, else, for a reference
/// created at run time (PlaceAtMe, a leveled spawn), where it was created (#2278)
/// [hypothesis: not measured in the original; without it every package near the editor
/// location fails for a created actor, which then stands still].
/// The creation place is saved with the reference; a created actor of a save from before
/// that has the place it was saved at instead (`World::made_at_from_saved_place`, #3042).
pub fn editor_place_of(game: &Game, r: FormId) -> Option<Place> {
    let refs = game.world.placed;
    editor_place(refs, r).or_else(|| {
        let (cell, pos) = game.world.peek(r)?.made_at?;
        Some(Place { space: space_of_cell(refs, cell?), pos })
    })
}

/// A point of a cell's navmesh to walk towards: the centre of the triangle nearest the
/// middle of the cell's largest walkable area [our choice].
pub fn cell_anchor(nav: &NavGraph, refs: &RefTable, cell: FormId) -> Option<Place> {
    let space = space_of_cell(refs, cell);
    let tris: Vec<u32> = nav
        .meshes
        .iter()
        .filter(|m| m.enabled && m.cell == Some(cell.0))
        .flat_map(|m| m.first_tri..m.first_tri + m.tri_count)
        .filter(|&t| nav.area[t as usize] != NONE)
        .collect();
    // The area with the most triangles in this cell.
    let mut counts: std::collections::BTreeMap<u32, usize> = Default::default();
    for &t in &tris {
        *counts.entry(nav.area[t as usize]).or_default() += 1;
    }
    let (&area, _) = counts.iter().max_by_key(|(a, n)| (**n, std::cmp::Reverse(**a)))?;
    let of_area: Vec<u32> = tris.into_iter().filter(|&t| nav.area[t as usize] == area).collect();
    let n = of_area.len() as f32;
    let mean = of_area.iter().fold([0.0f32; 3], |m, &t| {
        let c = nav.centre(t);
        [m[0] + c[0] / n, m[1] + c[1] / n, m[2] + c[2] / n]
    });
    let d2 = |t: &u32| {
        let c = nav.centre(*t);
        (c[0] - mean[0]).powi(2) + (c[1] - mean[1]).powi(2) + (c[2] - mean[2]).powi(2)
    };
    let best = of_area.iter().min_by(|a, b| d2(a).total_cmp(&d2(b)))?;
    Some(Place { space, pos: nav.centre(*best) })
}

/// The chain of linked references (XLKR or runtime SetLinkedReference) from `start`, as
/// patrol points.
fn route_from(game: &Game, start: FormId, repeatable: bool) -> Option<Route> {
    let mut ids = vec![start];
    let mut loops_to = None;
    let mut at = start;
    while let Some(next) = game.linked_ref(at) {
        if let Some(i) = ids.iter().position(|x| *x == next) {
            loops_to = Some(i);
            break;
        }
        if ids.len() >= 256 {
            break;
        }
        ids.push(next);
        at = next;
    }
    let mut points = Vec::with_capacity(ids.len());
    for id in ids {
        match place_of_ref(game, id) {
            Some(p) => points.push((id, p)),
            // A point that is nowhere ends the route there.
            None => {
                loops_to = None;
                break;
            }
        }
    }
    (!points.is_empty()).then_some(Route { points, loops_to, repeatable })
}

/// The goal of `actor` under package `def`.
///
/// Carried out: Travel, Sandbox, Sleep, Eat, Guard and Wander go to their location and
/// stay within its radius [documented: the page of each type]; a Patrol located near a
/// reference, or near the actor's linked reference, walks the chain of references linked
/// from it [documented: Patrol Package; observed: GSVictorRef, packages.md §3]. A Patrol
/// with another location goes to that location [hypothesis].
pub fn resolve(game: &Game, nav: &NavGraph, actor: FormId, def: &PackageDef) -> Goal {
    match def.ty {
        // A Use Weapon package stands at its location to shoot (`fnv_game::use_weapon`).
        PackageType::Travel | PackageType::Sandbox | PackageType::Sleep | PackageType::Eat | PackageType::Guard | PackageType::Wander | PackageType::Patrol | PackageType::UseWeapon => {}
        PackageType::Follow | PackageType::Accompany => return follow(game, actor, def),
        PackageType::Escort => return escort(game, nav, actor, def),
        PackageType::Dialogue => return talk(game, nav, actor, def),
        PackageType::Find => return find(game, nav, actor, def),
        PackageType::UseItemAt => return use_at(game, nav, actor, def),
        PackageType::Ambush => return ambush(game, nav, actor, def),
        PackageType::FleeNotCombat => return flee(game, nav, actor, def),
        PackageType::Other(_) => return Goal::NotCarriedOut("an unknown package type"),
    }
    // Guard with Remain Near Reference to Guard: it stays near the guarded reference (the
    // package's target), within the location's radius, else the target's distance
    // [hypothesis: the flag's name; no page documents the Guard tab, packages.md §7.7;
    // 3 packages of the masters set it].
    if def.ty == PackageType::Guard && def.remain_near_reference() {
        if let Ok(guarded) = target_ref(game, actor, def, "") {
            if guarded != actor {
                let radius = def.location.map_or(0, |l| l.radius.max(0));
                let radius = if radius > 0 { radius } else { def.target.map_or(0, |t| t.count_distance.max(0)) };
                return match place_of_ref(game, guarded) {
                    Some(place) => Goal::Near { place, radius: radius as f32, reference: Some(guarded) },
                    None => Goal::Missing("the guarded reference is in no cell"),
                };
            }
        }
    }
    let Some(loc) = def.location else { return Goal::Here };
    let linked = game.linked_ref(actor);
    if def.ty == PackageType::Patrol {
        let start = match loc.kind {
            LocationKind::NearReference(r) => r,
            LocationKind::NearLinked => linked,
            _ => None,
        };
        if let Some(route) = start.and_then(|s| route_from(game, s, def.patrol_repeatable.unwrap_or(false))) {
            return Goal::Route(route);
        }
    }
    stay_at(game, nav, actor, &loc)
}

/// A location as a place to stay at ([`Goal::Here`], [`Goal::Near`], [`Goal::Inside`]),
/// or why it is none.
fn stay_at(game: &Game, nav: &NavGraph, actor: FormId, loc: &Location) -> Goal {
    let refs = game.world.placed;
    let radius = loc.radius.max(0) as f32;
    let reference = location_reference(game, actor, loc.kind);
    let near = |place: Option<Place>, why: &'static str| match place {
        Some(place) => Goal::Near { place, radius, reference },
        None => Goal::Missing(why),
    };
    match loc.kind {
        LocationKind::NearReference(Some(r)) if r == FormId::PLAYER_REF => near(reference.and_then(|r| place_of_ref(game, r)), "the location is the player, who is nowhere yet"),
        LocationKind::NearReference(Some(_)) => near(reference.and_then(|r| place_of_ref(game, r)), "the location's reference is in no cell"),
        LocationKind::NearReference(None) => Goal::Missing("the location's reference is not in the load order"),
        LocationKind::InCell(Some(c)) => match (cell_anchor(nav, refs, c), refs.world_of(c)) {
            (Some(at), None) => Goal::Inside { space: at.space, at },
            // An exterior cell: be near the middle of its navmesh [our choice].
            (Some(at), Some(_)) => Goal::Near { place: at, radius: 2048.0, reference: None },
            (None, _) => Goal::Missing("the location's cell has no navmesh"),
        },
        LocationKind::InCell(None) => Goal::Missing("the location's cell is not in the load order"),
        LocationKind::NearCurrent => Goal::Here,
        LocationKind::NearEditor => near(editor_place_of(game, actor), "the actor has no editor location"),
        LocationKind::NearLinked => near(reference.and_then(|r| place_of_ref(game, r)), "the actor has no linked reference"),
        LocationKind::ObjectId(Some(_)) => near(reference.and_then(|r| place_of_ref(game, r)), "no reference of the location's object is near the actor"),
        LocationKind::ObjectId(None) => Goal::Missing("the location's object is not in the load order"),
        LocationKind::ObjectType(0) => Goal::NotCarriedOut("a location of object type None"),
        LocationKind::ObjectType(_) => near(reference.and_then(|r| place_of_ref(game, r)), "no reference of the location's object type is near the actor"),
        // Location 1 is the package's location: as location 1 it names nothing else
        // (the masters use it as Eat's location 2 only, packages.md §7.7).
        LocationKind::AtPackageLocation => Goal::Here,
        LocationKind::Other(_) => Goal::NotCarriedOut("an unknown location type"),
    }
}

/// The reference selected by a reference-valued location [design, #2526]: `stay_at`
/// retains it in the goal for `carry::marker_heading`. Locations that describe a
/// position or cell return none.
fn location_reference(game: &Game, actor: FormId, kind: LocationKind) -> Option<FormId> {
    match kind {
        LocationKind::NearReference(r) => r,
        LocationKind::NearLinked => game.linked_ref(actor),
        LocationKind::ObjectId(Some(b)) => nearest_object(game, actor, Wanted::Base(b)),
        LocationKind::ObjectType(c) if c != 0 => nearest_object(game, actor, Wanted::Type(c)),
        _ => None,
    }
}

/// Stay-at locations given by an object (Near Object ID, Near Object Type) are looked for
/// this far round the actor, game units [design: a cell; packages.md §7.7].
pub const OBJECT_LOCATION_SEARCH: f32 = 4096.0;

/// The nearest reference round `actor` that `wanted` names (a base, the
/// bases of a form list, or an object type), present and enabled, within
/// [`OBJECT_LOCATION_SEARCH`]; ties by FormID [hypothesis: no source says how the game
/// picks the object, issue #961].
fn nearest_object(game: &Game, actor: FormId, wanted: Wanted) -> Option<FormId> {
    let at = place_of_ref(game, actor)?;
    let data = game.data;
    crate::errand::cells_around(game, at, OBJECT_LOCATION_SEARCH)
        .into_iter()
        .filter(|&r| r != actor && game.present(r))
        .filter_map(|r| {
            let p = place_of_ref(game, r).filter(|p| p.space == at.space)?;
            let d = dist(p.pos, at.pos);
            if d > OBJECT_LOCATION_SEARCH {
                return None;
            }
            let base = fnv_game::functions::base_of(game, r)?;
            let fits = match wanted {
                Wanted::Base(x) => base == x || fnv_game::functions::list_members(game, x).contains(&base),
                Wanted::Type(code) => crate::errand::of_type(game, r, base, &data.sig(base).unwrap_or(*b"    "), code),
                _ => false,
            };
            fits.then_some((d, r))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
        .map(|(_, r)| r)
}

/// The goal of a Follow or Accompany package: its target, a reference or the actor's
/// linked reference ([`Follow`]).
fn follow(game: &Game, actor: FormId, def: &PackageDef) -> Goal {
    let linked = game.linked_ref(actor);
    let target = match def.target.map(|t| t.kind) {
        Some(TargetKind::Reference(Some(r))) => r,
        Some(TargetKind::LinkedReference) => match linked {
            Some(l) => l,
            None => return Goal::Missing("the actor has no linked reference to follow"),
        },
        Some(TargetKind::Reference(None)) => return Goal::Missing("the reference to follow is not in the load order"),
        _ => return Goal::NotCarriedOut("a follow package without a reference to follow"),
    };
    if target == actor {
        return Goal::Missing("the actor would follow itself");
    }
    let distance = def.target.map_or(0, |t| t.count_distance);
    Goal::Follow(Follow { target, distance: if distance > 0 { distance as f32 } else { STANDARD_FOLLOW_DISTANCE } })
}

/// The goal of a Dialogue package [documented: GECK wiki, Dialogue Package].
///
/// Carried out: a conversation with the player (the actor "will force-greet the
/// player"), a Say To line to anyone, and a conversation with another actor or with a
/// talking activator, which speaks for its talking actor (`Game::start_actor_conversation`;
/// Veronica's FollowersVeronicaForceGreetIntercom talks to Hidden Valley's intercom,
/// issue #2451) [documented: the package engages the actor "in a specific conversation
/// with the player or another NPC"].
fn talk(game: &Game, nav: &NavGraph, actor: FormId, def: &PackageDef) -> Goal {
    let linked = game.linked_ref(actor);
    let target = match def.target.map(|t| t.kind) {
        Some(TargetKind::Reference(Some(r))) => r,
        Some(TargetKind::LinkedReference) => match linked {
            Some(l) => l,
            None => return Goal::Missing("the actor has no linked reference to talk to"),
        },
        Some(TargetKind::Reference(None)) => return Goal::Missing("the reference to talk to is not in the load order"),
        _ => return Goal::NotCarriedOut("a dialogue package without a reference to talk to"),
    };
    let data = def.dialogue.unwrap_or_default();
    if data.say_to && data.topic.is_none() {
        return Goal::NotCarriedOut("a Say To package without a topic");
    }
    // A wait location that cannot be gone to leaves the actor where it is [our choice].
    let wait = match def.location.map(|l| stay_at(game, nav, actor, &l)) {
        Some(g @ (Goal::Near { .. } | Goal::Inside { .. })) => g,
        _ => Goal::Here,
    };
    let trigger = def.location2.map_or(Trigger::None, |l| trigger_of(game, actor, &l));
    let distance = def.target.map_or(0, |t| t.count_distance);
    Goal::Talk(Talk {
        target,
        distance: if distance > 0 { distance as f32 } else { STANDARD_TALK_DISTANCE },
        wait: Box::new(wait),
        wait_radius: def.location.map_or(0.0, |l| l.radius.max(0) as f32),
        trigger,
        topic: data.topic,
        say_to: data.say_to,
        no_headtracking: data.no_headtracking,
        control_target: !data.dont_control_target_movement,
    })
}

/// A location 2 as a trigger location (Dialogue and Ambush packages).
fn trigger_of(game: &Game, actor: FormId, l: &Location) -> Trigger {
    let linked = game.linked_ref(actor);
    let radius = l.radius.max(0) as f32;
    match l.kind {
        LocationKind::NearReference(Some(reference)) => Trigger::NearReference { reference, radius },
        LocationKind::NearEditor => editor_place_of(game, actor).map_or(Trigger::Never, |place| Trigger::Near { place, radius }),
        LocationKind::NearLinked => linked.map_or(Trigger::Never, |reference| Trigger::NearReference { reference, radius }),
        LocationKind::NearCurrent => Trigger::NearActor { radius },
        LocationKind::InCell(Some(c)) => Trigger::InCell(c),
        _ => Trigger::Never,
    }
}

/// A package's target as one reference: a specific reference, or the actor's linked
/// reference. `Err`: the goal to give the package instead (`why_none` when it has
/// neither).
fn target_ref(game: &Game, actor: FormId, def: &PackageDef, why_none: &'static str) -> Result<FormId, Goal> {
    match def.target.map(|t| t.kind) {
        Some(TargetKind::Reference(Some(r))) => Ok(r),
        Some(TargetKind::LinkedReference) => game.linked_ref(actor).ok_or(Goal::Missing("the actor has no linked reference")),
        Some(TargetKind::Reference(None)) => Err(Goal::Missing("the package's target is not in the load order")),
        _ => Err(Goal::NotCarriedOut(why_none)),
    }
}

/// What a package's target asks for ([`Wanted`]).
fn wanted(game: &Game, actor: FormId, def: &PackageDef) -> Result<Wanted, Goal> {
    Ok(match def.target.map(|t| t.kind) {
        None | Some(TargetKind::Other(_)) | Some(TargetKind::ObjectType(0)) => Wanted::Nothing,
        Some(TargetKind::ObjectType(c)) => Wanted::Type(c),
        Some(TargetKind::ObjectId(Some(b))) => Wanted::Base(b),
        Some(TargetKind::ObjectId(None)) => return Err(Goal::Missing("the package's target is not in the load order")),
        Some(_) => Wanted::Reference(target_ref(game, actor, def, "")?),
    })
}

/// A location as a place to go first and the area around it: `Err` when the location
/// names something that is not in the world.
fn place_and_area(game: &Game, nav: &NavGraph, actor: FormId, loc: Option<&Location>) -> Result<(Goal, Area), Goal> {
    let radius = loc.map_or(0.0, |l| l.radius.max(0) as f32);
    let radius = if radius > 0.0 { radius } else { STANDARD_SEARCH_RADIUS };
    let Some(loc) = loc else { return Ok((Goal::Here, Area { centre: None, radius })) };
    Ok(match stay_at(game, nav, actor, loc) {
        g @ Goal::Near { place, .. } => (g, Area { centre: Some(place), radius }),
        g @ Goal::Inside { at, .. } => (g, Area { centre: Some(at), radius: f32::INFINITY }),
        g @ Goal::Missing(_) => return Err(g),
        // Near the current location, or a location given by an object: around the actor.
        _ => (Goal::Here, Area { centre: None, radius }),
    })
}

/// The goal of an Escort package ([`Escort`]): its target is the one it leads, its
/// location where to.
fn escort(game: &Game, nav: &NavGraph, actor: FormId, def: &PackageDef) -> Goal {
    let target = match target_ref(game, actor, def, "an escort package without someone to lead") {
        Ok(t) if t == actor => return Goal::Missing("the actor would escort itself"),
        Ok(t) => t,
        Err(g) => return g,
    };
    // Without a destination it stays with its target where it is [our choice].
    let to = match def.location.map(|l| stay_at(game, nav, actor, &l)) {
        Some(g @ Goal::Missing(_)) => return g,
        Some(g @ (Goal::Near { .. } | Goal::Inside { .. })) => g,
        _ => Goal::Here,
    };
    let distance = def.escort_distance.filter(|d| *d > 0).map_or(STANDARD_ESCORT_DISTANCE, |d| d as f32);
    Goal::Escort(Escort { target, to: Box::new(to), distance })
}

/// The goal of a Find package ([`Find`]).
fn find(game: &Game, nav: &NavGraph, actor: FormId, def: &PackageDef) -> Goal {
    let (search, area) = match place_and_area(game, nav, actor, def.location.as_ref()) {
        Ok(x) => x,
        Err(g) => return g,
    };
    let wanted = match wanted(game, actor, def) {
        Ok(Wanted::Reference(r)) if r == actor => return Goal::Missing("the actor would find itself"),
        Ok(w) => w,
        Err(g) => return g,
    };
    // One reference to find, with no radius to its location (0, or no location): it is
    // gone after wherever it is, from where the actor stands [hypothesis, issue #2473:
    // Cachino's vGomorrahCachinoDialoguePackage finds the player anywhere in Gomorrah's
    // four cells from "near editor location" radius 0; packages.md §10]. Before, the
    // 512 of a radius 0 search held for it too, and he stood still.
    let (search, area) = if matches!(wanted, Wanted::Reference(_)) && def.location.as_ref().is_none_or(|l| l.radius <= 0) {
        (Goal::Here, Area::ANYWHERE)
    } else {
        (search, area)
    };
    let n = def.target.map_or(0, |t| t.count_distance.max(0));
    let (count, distance) = match wanted {
        Wanted::Reference(_) => (1, n as f32),
        _ => (n.max(1) as u32, 0.0),
    };
    use fnv_game::package::type_flags as t;
    Goal::Find(Find { search: Box::new(search), area, wanted, count, distance, kill: def.search_flag(t::ALLOW_KILLING), steal: def.search_flag(t::ALLOW_STEALING), buy: def.search_flag(t::ALLOW_BUYING) })
}

/// The goal of a Use Item At package ([`UseAt`]).
fn use_at(game: &Game, nav: &NavGraph, actor: FormId, def: &PackageDef) -> Goal {
    let (at, area) = match place_and_area(game, nav, actor, def.location.as_ref()) {
        Ok(x) => x,
        Err(g) => return g,
    };
    match wanted(game, actor, def) {
        Ok(wanted) => Goal::UseAt(UseAt { at: Box::new(at), area, wanted, sit_down: def.sit_down() }),
        Err(g) => g,
    }
}

/// The trigger location of an Ambush package without a location 2 [our choice; every
/// Ambush package of the masters has one, packages.md §2].
const STANDARD_AMBUSH_TRIGGER: f32 = 1024.0;

/// The goal of an Ambush package ([`Ambush`]): location 1 is where it waits, location
/// 2 where enemies set it off.
fn ambush(game: &Game, nav: &NavGraph, actor: FormId, def: &PackageDef) -> Goal {
    let wait = match def.location.map(|l| stay_at(game, nav, actor, &l)) {
        Some(g @ Goal::Missing(_)) => return g,
        Some(g @ (Goal::Near { .. } | Goal::Inside { .. })) => g,
        _ => Goal::Here,
    };
    let trigger = def.location2.map_or(Trigger::NearActor { radius: STANDARD_AMBUSH_TRIGGER }, |l| trigger_of(game, actor, &l));
    match wanted(game, actor, def) {
        Ok(target) => Goal::Ambush(Ambush { wait: Box::new(wait), trigger, target, hide: def.hide_while_ambushing() }),
        Err(g) => g,
    }
}

/// The goal of a Flee Not Combat package ([`Flee`]).
fn flee(game: &Game, nav: &NavGraph, actor: FormId, def: &PackageDef) -> Goal {
    let from = match target_ref(game, actor, def, "") {
        Ok(t) if t != actor => Some(t),
        Err(g @ Goal::Missing(_)) => return g,
        _ => None,
    };
    let to = match def.location.map(|l| stay_at(game, nav, actor, &l)) {
        Some(g @ Goal::Missing(_)) => return g,
        Some(g @ (Goal::Near { .. } | Goal::Inside { .. })) => Some(Box::new(g)),
        _ => None,
    };
    let n = def.target.map_or(0, |t| t.count_distance);
    Goal::Flee(Flee { from, distance: if n > 0 { n as f32 } else { flee_distance(game, actor) }, to })
}

/// The furniture a package gives an actor that has arrived at `at`, if any:
/// - the package's location, when that reference (or the linked reference it names) is
///   furniture [observed: under Travel
///   packages to a chair EasyPeteRef and a settler were sitting, GetSitting 3, in 7,395
///   samples; packages.md §4];
/// - for a Sleep package, the nearest bed of the actor's cell within the location's
///   radius, or anywhere in the cell for an "in cell" location [documented: GECK wiki,
///   Sleep Package];
/// - for an Eat package, the nearest seat within 256 units [documented: "sitting down
///   if furniture is available", Eat Package; the distance is our choice].
///
/// Furniture given to another actor (`taken`: furniture to its user) is left alone.
/// Sandbox and Wander packages choose for themselves (`crate::sandbox`). What the actor
/// has no clips for is not given; what is searched for is not a cross either, what is
/// named or linked may be ([`crate::furniture`]).
pub fn package_furniture(game: &Game, taken: &std::collections::BTreeMap<FormId, FormId>, actor: FormId, def: &PackageDef, at: Place, eligibility: &mut crate::furniture::Eligibility) -> Option<FormId> {
    let refs = game.world.placed;
    let flags = |r: FormId| refs.get(r).and_then(|p| game.data.furniture.get(&p.base)).copied();
    let free = |r: &FormId| taken.get(r).is_none_or(|a| *a == actor);
    let loc = def.location?;
    // The package's place as one reference: a specific one, or the actor's linked
    // reference [observed: the crucified Powder Gangers of Nipton (vNiptonSurvivorCrucifix
    // and its kin) have a Travel package to "Near linked reference", linked to their
    // crosses (NVCrucifixFurniture), and their script restrains them once GetSitting is
    // 3 (NiptonCrucifiedVictimScript); owner's playtest list 7 item 34: they got off the
    // crosses].
    let place_ref = match loc.kind {
        LocationKind::NearReference(Some(r)) => Some(r),
        LocationKind::NearLinked => game.linked_ref(actor),
        _ => None,
    };
    if let Some(r) = place_ref {
        if flags(r).is_some() {
            return (free(&r) && eligibility.fits_assigned(game, actor, r)).then_some(r);
        }
    }
    let (want, reach) = match (def.ty, loc.kind) {
        (PackageType::Sleep, LocationKind::InCell(_)) => (furniture::SLEEP, f32::INFINITY),
        (PackageType::Sleep, _) => (furniture::SLEEP, (loc.radius as f32).max(256.0)),
        (PackageType::Eat, _) => (furniture::SIT, 256.0),
        _ => return None,
    };
    let cell = cell_of_place(refs, &at)?;
    let near: Vec<(f32, FormId)> = refs
        .by_cell
        .get(&cell)?
        .iter()
        .filter(|r| free(r) && flags(**r).is_some_and(|f| f & want != 0))
        .filter_map(|r| {
            let p = refs.get(*r)?;
            let d = (p.pos[0] - at.pos[0]).hypot(p.pos[1] - at.pos[1]);
            (d <= reach && (p.pos[2] - at.pos[2]).abs() < 200.0).then_some((d, *r))
        })
        .collect();
    let excluded = eligibility.excluded_casual(game, actor, near.iter().map(|x| x.1));
    near.into_iter().filter(|x| !excluded.contains(&x.1)).min_by(|a, b| a.0.total_cmp(&b.0)).map(|x| x.1)
}
