//! Seamless doors (docs/design/seamless-doors.md): load doors without a loading screen.
//! NVForever's own feature, not the original's behaviour: the original always shows a
//! loading screen through a load door [observed: loading-screens.md]. Off unless asked
//! for (`--seamless-doors`).
//!
//! Its four parts, each on its own switch ([`Seamless`]):
//!
//! 1. **ahead**: near a load door, what its travel would load is prepared before it is
//!    opened (physics: `phys::warm`; nvforever's `door_ahead.rs`: models, landscape,
//!    actors).
//! 2. **fade**: the travel's screen is a short fade through black; the loading picture
//!    shows only if the destination is still loading after
//!    `loading::FADE_PICTURE_AFTER_SECS` (`crate::loading`).
//! 3. **keep**: the place left stays loaded, hidden and out of the way, while the
//!    destination's area does not overlap it ([`keeps`]): physics keeps its cells
//!    (`PhysState::kept`), the renderer its cells' entities (nvforever `world.rs`).
//!    Going back through the door finds everything in.
//! 4. **portal**: an open load door shows the place it leads to, drawn by a second
//!    camera (nvforever `door_portal.rs`).
//!
//! The doors are listed once from the simulation's references ([`LoadDoors`]).

use crate::{Location, PlayData, Travel};
use bevy::prelude::*;
use fnv_sim::refs::RefTable;
use fnv_sim::FormId;
use fnv_world::{cell_of, WorldIndex, CELL_SIZE};

/// Which parts of seamless doors are on (module notes).
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Seamless {
    pub ahead: bool,
    pub fade: bool,
    pub keep: bool,
    pub portal: bool,
}

impl Seamless {
    pub const ALL: Seamless = Seamless { ahead: true, fade: true, keep: true, portal: true };

    pub fn any(&self) -> bool {
        self.ahead || self.fade || self.keep || self.portal
    }

    /// `all`, `off`, or a comma-separated list of `ahead`, `fade`, `keep`, `portal`.
    pub fn parse(s: &str) -> Result<Seamless, String> {
        let s = s.trim();
        match s {
            "" | "all" | "on" => return Ok(Seamless::ALL),
            "off" | "none" => return Ok(Seamless::default()),
            _ => {}
        }
        let mut out = Seamless::default();
        for part in s.split(',').map(str::trim) {
            match part {
                "ahead" => out.ahead = true,
                "fade" => out.fade = true,
                "keep" => out.keep = true,
                "portal" => out.portal = true,
                other => return Err(format!("unknown part {other:?}: expected all, off, or ahead, fade, keep, portal")),
            }
        }
        Ok(out)
    }

    pub fn describe(&self) -> String {
        let parts: Vec<&str> = [(self.ahead, "ahead"), (self.fade, "fade"), (self.keep, "keep"), (self.portal, "portal")]
            .into_iter()
            .filter_map(|(on, n)| on.then_some(n))
            .collect();
        if parts.is_empty() { "off".into() } else { parts.join(", ") }
    }
}

/// A load door: a placed door with a teleport, where it stands and where it leads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoadDoor {
    pub door: u32,
    /// The door at the other end.
    pub to_door: u32,
    /// Where it stands (game space), and the place it stands in.
    pub pos: [f32; 3],
    pub rot: [f32; 3],
    pub from: Place,
    /// The travel its activation sends (`activate::outcome::effects`).
    pub travel: Travel,
}

/// A place the player can be in: an interior cell, or a worldspace's exterior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Place {
    Interior(u32),
    Exterior(u32),
}

impl Place {
    pub fn of(l: Location) -> Place {
        match l.interior {
            Some(c) => Place::Interior(c),
            None => Place::Exterior(l.world),
        }
    }

    /// Where `t` arrives, leaving `from`.
    pub fn arrival(t: &Travel, from: Location) -> Place {
        Place::of(t.destination(from))
    }
}

/// Every load door of the plugins (module notes).
#[derive(Resource, Default)]
pub struct LoadDoors(pub Vec<LoadDoor>);

impl LoadDoors {
    pub fn build(refs: &RefTable, index: &WorldIndex) -> LoadDoors {
        let mut out = Vec::new();
        for (id, r) in &refs.refs {
            if r.teleport.is_none() {
                continue;
            }
            let Some(travel) = door_travel(refs, index, *id) else { continue };
            let Some(cell) = r.cell.and_then(|c| index.cells.get(&c.0)) else { continue };
            let from = match cell.world {
                Some(w) => Place::Exterior(w),
                None => Place::Interior(cell.form_id),
            };
            let to_door = r.teleport.map_or(0, |t| t.door.0);
            out.push(LoadDoor { door: id.0, to_door, pos: r.pos, rot: r.rot, from, travel });
        }
        LoadDoors(out)
    }

    /// The load doors of `place` within `radius` of `at` (game space), nearest first.
    pub fn near(&self, place: Place, at: [f32; 3], radius: f32) -> Vec<&LoadDoor> {
        let mut v: Vec<(&LoadDoor, f32)> = self
            .0
            .iter()
            .filter(|d| d.from == place)
            .map(|d| (d, Vec3::from_array(d.pos).distance(Vec3::from_array(at))))
            .filter(|(_, dist)| *dist <= radius)
            .collect();
        v.sort_by(|a, b| a.1.total_cmp(&b.1));
        v.into_iter().map(|(d, _)| d).collect()
    }

    pub fn get(&self, door: u32) -> Option<&LoadDoor> {
        self.0.iter().find(|d| d.door == door)
    }
}

/// The travel a load door's activation sends, as the simulation resolves it
/// (`fnv_sim::World::activate`, `activate::outcome::effects`): into the destination
/// door's interior, or out into the exterior cell holding the teleport position.
pub fn door_travel(refs: &RefTable, index: &WorldIndex, door: FormId) -> Option<Travel> {
    let tp = refs.get(door)?.teleport?;
    let door_cell = refs.get(tp.door)?.cell?;
    let cell = refs.exterior_cell_at(door_cell, tp.pos).unwrap_or(door_cell);
    let c = index.cells.get(&cell.0)?;
    let heading_deg = crate::activate::outcome::heading_deg(tp.rot);
    Some(match c.world {
        None => Travel { world: None, interior: Some(c.form_id), feet: tp.pos, heading_deg },
        Some(w) => Travel { world: Some(w), interior: None, feet: tp.pos, heading_deg },
    })
}

/// An axis-aligned rectangle in game space (x0, y0, x1, y1).
pub type Rect = [f32; 4];

/// Grid squares around an exterior place kept loaded, for the overlap test: the renderer's
/// loaded grid and the read-ahead ring around it [design: `--radius 2` plus its ring; a
/// larger radius is covered by [`MARGIN`]].
pub const EXTERIOR_REACH: i32 = 3;

/// Distance two places must keep apart (game units) to be loaded at once
/// [engineering choice: a cell, so lights, sounds and long models of one cannot reach
/// the other].
pub const MARGIN: f32 = CELL_SIZE;

/// The area of an exterior place around `feet`: the squares within [`EXTERIOR_REACH`].
pub fn exterior_rect(feet: [f32; 3]) -> Rect {
    let (x, y) = cell_of(feet[0], feet[1]);
    let r = EXTERIOR_REACH;
    [(x - r) as f32 * CELL_SIZE, (y - r) as f32 * CELL_SIZE, (x + r + 1) as f32 * CELL_SIZE, (y + r + 1) as f32 * CELL_SIZE]
}

/// The area of an interior: the box of its references' positions. `None` for one with
/// no references the index knows.
pub fn interior_rect(d: &PlayData, cell: u32) -> Option<Rect> {
    let c = d.index.cells.get(&cell)?;
    let mut r: Option<Rect> = None;
    for &id in &c.refs {
        let Some(p) = fnv_world::placed::placement(d.lo, d.schema, id) else { continue };
        let [x, y, _] = p.position;
        r = Some(match r {
            None => [x, y, x, y],
            Some([x0, y0, x1, y1]) => [x0.min(x), y0.min(y), x1.max(x), y1.max(y)],
        });
    }
    r
}

/// Whether two areas stay [`MARGIN`] apart.
pub fn apart(a: Rect, b: Rect) -> bool {
    a[2] + MARGIN <= b[0] || b[2] + MARGIN <= a[0] || a[3] + MARGIN <= b[1] || b[3] + MARGIN <= a[1]
}

/// Whether a travel `t` leaving `from` (the player's feet at `feet`) can keep the place
/// left loaded (step 3): the same worldspace, and the two areas apart. Interiors are
/// drawn at their own coordinates, which overlap one another and the worldspace's middle
/// [observed: `fnvtool world refs --interiors`, the 376 interiors of FalloutNV.esm span
/// x -7,040..18,378 and y -22,592..16,000; Goodsprings is around (-71,600, 2,300)].
pub fn keeps(d: &PlayData, from: Location, feet: [f32; 3], t: &Travel) -> bool {
    let to = t.destination(from);
    if to.world != from.world {
        return false;
    }
    let area = |place: Place, at: [f32; 3]| match place {
        Place::Interior(c) => interior_rect(d, c),
        Place::Exterior(_) => Some(exterior_rect(at)),
    };
    match (area(Place::of(from), feet), area(Place::of(to), t.feet)) {
        (Some(a), Some(b)) => Place::of(from) != Place::of(to) && apart(a, b),
        _ => false,
    }
}

/// The last travel that went out and whether it keeps the place left (step 3), decided
/// once when it went out (`loading::send`) so that physics and the renderer, which apply
/// it in different sets of the frame, agree.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct TravelKeeps(pub Option<(Travel, bool)>);

impl TravelKeeps {
    /// Whether `t` is the travel decided last and keeps the place left.
    pub fn keeps(&self, t: &Travel) -> bool {
        self.0.is_some_and(|(k, keep)| keep && k == *t)
    }
}

/// Load doors this near the player (game units) have their destination prepared (step 1)
/// [engineering choice: about four seconds' walk, three of a run; an interior's cells
/// take a few hundred milliseconds to prepare, a worldspace's nine squares about a
/// second (docs/design/seamless-doors.md §2)].
pub const AHEAD_RADIUS: f32 = 1200.0;

/// Real seconds between looks for doors near the player [engineering choice].
pub const AHEAD_EVERY_SECS: f64 = 0.25;

/// Doors whose destination is prepared at most at once [engineering choice: a town
/// square's front doors; more would prepare places the player does not go].
pub const AHEAD_MAX_DOORS: usize = 4;

/// Real seconds since the program started (the warm caches' clock).
pub fn now_secs() -> f64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64()
}

/// The load doors near the player whose destinations are prepared now (step 1), nearest
/// first. Physics prepares their cells ([`warm_physics`]); nvforever reads it for the
/// renderer (`door_ahead.rs`).
#[derive(Resource, Default, Debug, Clone)]
pub struct DoorsAhead {
    pub doors: Vec<LoadDoor>,
    /// Where the player is (the place the doors are in).
    pub place: Option<Place>,
    last: f64,
    /// Doors whose destination was prepared so far (for the exit line).
    pub listed: usize,
}

/// The physics cells a travel loads in its frame (`phys::travel`): the interior, or the
/// squares around the arrival in the worldspace being played (`phys_world`). None for
/// another worldspace.
pub fn phys_cells(t: &Travel, from: Location, phys_world: u32) -> Vec<crate::phys::cells::PhysCell> {
    use crate::phys::cells::{wanted, PhysCell};
    let to = t.destination(from);
    match to.interior {
        Some(c) => vec![PhysCell::Interior(c)],
        None if to.world == phys_world => wanted(cell_of(t.feet[0], t.feet[1])).into_iter().map(|(x, y)| PhysCell::Exterior(x, y)).collect(),
        None => Vec::new(),
    }
}

fn pick_doors(
    seamless: Option<Res<Seamless>>,
    doors: Option<Res<LoadDoors>>,
    location: Res<Location>,
    player: Res<crate::player::Player>,
    loading: Res<crate::loading::Loading>,
    mut ahead: ResMut<DoorsAhead>,
) {
    let (Some(s), Some(doors)) = (seamless, doors) else { return };
    if !(s.ahead || s.portal) || loading.active() {
        return;
    }
    let now = now_secs();
    if now - ahead.last < AHEAD_EVERY_SECS {
        return;
    }
    ahead.last = now;
    let place = Place::of(*location);
    let near: Vec<LoadDoor> = doors.near(place, player.feet, AHEAD_RADIUS).into_iter().take(AHEAD_MAX_DOORS).copied().collect();
    let new = near.iter().filter(|d| !ahead.doors.iter().any(|o| o.door == d.door)).count();
    for d in near.iter().filter(|d| !ahead.doors.iter().any(|o| o.door == d.door)) {
        info!("seamless doors: door {:08X} is near: its destination ({}) is prepared ahead", d.door, match Place::arrival(&d.travel, *location) {
            Place::Interior(c) => format!("interior {c:08X}"),
            Place::Exterior(w) => format!("worldspace {w:08X} around {:?}", cell_of(d.travel.feet[0], d.travel.feet[1])),
        });
    }
    ahead.listed += new;
    ahead.doors = near;
    ahead.place = Some(place);
}

fn warm_physics(
    seamless: Option<Res<Seamless>>,
    ahead: Res<DoorsAhead>,
    location: Res<Location>,
    d: Res<crate::PlayWorld>,
    sim: Res<crate::sim::SimState>,
    data: Option<Res<nvf_assets::GameData>>,
    mut phys: ResMut<crate::phys::PhysState>,
) {
    if !seamless.is_some_and(|s| s.ahead) || ahead.doors.is_empty() || ahead.place != Some(Place::of(*location)) {
        return;
    }
    let now = now_secs();
    let world = phys.worldspace;
    let cells: Vec<_> = ahead.doors.iter().flat_map(|door| phys_cells(&door.travel, *location, world)).collect();
    let ctx = crate::phys::LoadCtx { d: &d, sim: &sim, data: data.as_deref() };
    let asked = phys.warm_cells(&ctx, &cells, now);
    if asked > 0 {
        debug!("seamless doors: {asked} physics cell(s) asked for ahead");
    }
}

fn report(mut exit: MessageReader<AppExit>, seamless: Option<Res<Seamless>>, ahead: Res<DoorsAhead>, phys: Res<crate::phys::PhysState>) {
    if exit.read().next().is_none() || !seamless.is_some_and(|s| s.any()) {
        return;
    }
    let (ready, pending) = phys.warm.counts();
    info!(
        "seamless doors summary: {} door(s) prepared ahead; physics cells taken from the read-ahead {}, dropped {}, left {ready} ready and {pending} being prepared",
        ahead.listed, phys.warm.taken, phys.warm.dropped
    );
}

/// Load doors whose other side can be shown through them now (step 4): its place is
/// loaded (kept, step 3), set every frame by nvforever (`door_portal.rs`). Activating
/// one opens it ([`OpenPortal`]) instead of travelling.
#[derive(Resource, Default, Debug, Clone)]
pub struct PortalDoors(pub std::collections::HashSet<u32>);

/// Open a load door onto the place it leads to (step 4): its other side is drawn in the
/// doorway, and walking into it is the travel (nvforever `door_portal.rs`).
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct OpenPortal {
    pub door: u32,
}

pub struct SeamlessPlugin;

impl Plugin for SeamlessPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TravelKeeps>()
            .init_resource::<DoorsAhead>()
            .init_resource::<PortalDoors>()
            .add_message::<OpenPortal>()
            .add_systems(Startup, list_doors)
            .add_systems(Update, (pick_doors, warm_physics).chain().in_set(crate::PlaySet::Sync))
            .add_systems(Last, report);
    }
}

fn list_doors(mut commands: Commands, sim: Option<Res<crate::sim::SimState>>, d: Option<Res<crate::PlayWorld>>, seamless: Option<Res<Seamless>>) {
    let (Some(sim), Some(d)) = (sim, d) else { return };
    if !seamless.is_some_and(|s| s.any()) {
        return;
    }
    let doors = LoadDoors::build(sim.refs, d.index);
    info!("seamless doors: {} load doors listed", doors.0.len());
    commands.insert_resource(doors);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_parse() {
        assert_eq!(Seamless::parse("all"), Ok(Seamless::ALL));
        assert_eq!(Seamless::parse("off"), Ok(Seamless::default()));
        assert_eq!(Seamless::parse("fade, keep"), Ok(Seamless { fade: true, keep: true, ..Default::default() }));
        assert!(Seamless::parse("fast").is_err());
        assert_eq!(Seamless { ahead: true, portal: true, ..Default::default() }.describe(), "ahead, portal");
    }

    #[test]
    fn goodsprings_and_an_interior_are_apart() {
        // The Prospector Saloon's references [observed: fnvtool world refs] and the
        // Goodsprings exterior around its door.
        let saloon = [-592.0, -1186.0, 536.0, 637.0];
        let town = exterior_rect([-71_600.0, 2_300.0, 8_400.0]);
        assert!(apart(saloon, town));
        assert!(apart(town, saloon));
        // Two Goodsprings houses share their coordinates.
        assert!(!apart([-186.0, -906.0, 1082.0, -56.0], [256.0, -884.0, 1492.0, -40.0]));
        // The worldspace's middle is where interiors are.
        assert!(!apart(saloon, exterior_rect([0.0, 0.0, 0.0])));
    }
}
