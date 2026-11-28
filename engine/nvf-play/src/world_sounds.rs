//! Sounds of the world's objects (docs/spec-notes/audio.md §11): what a shot's impact
//! picks, loose objects knocking, items picked up and dropped, containers opened and
//! closed, activators used, and the sounds scripts ask for.
//!
//! - **Impacts** (played by `crate::combat`): [`log_impact`] tells what a hit picked,
//!   from the collision material to the IPCT and its sounds; counted by material.
//! - **Knocks** [hypothesis for which sound, design for when]: a loose (dynamic) body,
//!   placed clutter or a dropped item, that loses at least [`KNOCK_LIGHT`] units/s of
//!   its speed in a frame ([`knock_speed`]) plays the PHY sound of its collision
//!   material ([`phy_sound`]): the `…H` one from [`KNOCK_HARD`] up, else the `…L` one,
//!   at most once per [`KNOCK_GAP`] seconds per body and not in its first
//!   [`KNOCK_SETTLE`] seconds. No record links the PHY sounds to materials; the table
//!   goes by their names.
//! - **Items** [documented: xEdit, the `YNAM` Sound - Pick Up and `ZNAM` Sound - Drop of
//!   MISC, KEYM, WEAP, ARMO, AMMO, ALCH, BOOK, NOTE, IMOD, CHIP, CCRD, CMNY]: a
//!   reference taken out of the world (`RefState::deleted` on its `RefChanged`) plays
//!   its base's pick-up sound, a reference a drop creates plays its drop sound, both at
//!   the reference [hypothesis: where], and neither for what a load puts back (a
//!   `RefChanged` marked `restored`); an item moved between the player and an open
//!   container plays the pick-up sound when taken and the drop sound when put in, at the
//!   listener. An item whose record names none plays `UIItemGenericUp` / `…Down`
//!   [hypothesis: by the names].
//! - **Message boxes**: `UIPopUpMessageWindow` when one opens [design: by its name; the
//!   data names no sound for it], the buttons' `UIMenuOK` when one is picked and
//!   `UIMenuFocus` when the choice moves [observed: menus\prefabs\list_box_template.xml].
//! - **Containers** [documented: xEdit CONT `SNAM` Sound - Open, `QNAM` Sound - Close]:
//!   when the container menu opens on a container and when it closes again.
//! - **Activators** [documented: xEdit ACTI `VNAM` Sound - Activation]: when the player
//!   activates one (its script's OnActivate or its default action), at the activator.
//!   Activators with a radio station (`RNAM`) are left to the radio.
//! - **Scripts** (`fnv_game::sound`): `PlaySound` at the listener, `PlaySound3D` at the
//!   calling reference. Without its SystemSound flag a `PlaySound` asked for in a menu
//!   waits until the game is back [documented: GECK wiki PlaySound].
//! - **Objects' own sounds** [documented: xEdit ACTI `SNAM` Sound - Looping, CONT `RNAM`
//!   Sound - Random/Looping, DOOR `BNAM` Sound - Looping]: every placed activator,
//!   container and door whose base names one is a [`SoundSource`] where it stands, while
//!   it is enabled; `crate::ambient` plays it as a sound marker of that sound (a Loop
//!   sound goes round, a Play At Random one sounds now and then) [design: the records'
//!   sounds carry either flag, audio.md §14].
//! - **Load doors** [hypothesis]: activating a door that leads elsewhere plays its open
//!   sound (DOOR `SNAM`), at the listener, as the player goes through. An actor going
//!   through one ([`DoorPassed`], sent by nvforever) plays it at the door it went in by
//!   and at the one it came out of, where the listener is [design: owner's playtest list
//!   5 item 3, "like the player"].
//! - Not built: a knock between two loose objects told apart from one on the ground
//!   (both play the moving body's sound). The casings' sounds are nvforever's
//!   (`shot_fx.rs`).
//!
//! A `world sounds summary` line at the end.

use crate::menu::model::Mode;
use crate::menu::ActiveMenu;
use crate::phys::PhysState;
use crate::player::Player;
use crate::sfx::{ActorSound, PlaySound, Sfx, SoundSource};
use crate::sim::SimState;
use crate::{PlaySet, PlayState, PlayWorld, RefChanged};
use bevy::prelude::*;
use fnv_esm::{LoadOrder, Record};
use fnv_game::sound::SoundRequest;
use fnv_sim::FormId;
use std::collections::{BTreeMap, HashMap, HashSet};

/// A change of velocity (game units per second, gravity's share taken off) that knocks.
pub const KNOCK_LIGHT: f32 = 90.0;
/// … that knocks hard.
pub const KNOCK_HARD: f32 = 300.0;
/// Seconds between two knocks of one body.
pub const KNOCK_GAP: f64 = 0.12;
/// Seconds after a body's collision comes (its cell loads, it is dropped) in which it
/// settles without a sound; a dropped item makes its drop sound instead.
pub const KNOCK_SETTLE: f64 = 1.5;

/// The 32 Havok materials, by code mod 32 [documented: nif.xml `Fallout3HavokMaterial`,
/// `schemas/nif/havok.json`]; 32–127 are the same as platforms and stairs.
pub const HAVOK_MATERIALS: [&str; 32] = [
    "STONE",
    "CLOTH",
    "DIRT",
    "GLASS",
    "GRASS",
    "METAL",
    "ORGANIC",
    "SKIN",
    "WATER",
    "WOOD",
    "HEAVY_STONE",
    "HEAVY_METAL",
    "HEAVY_WOOD",
    "CHAIN",
    "BOTTLECAP",
    "ELEVATOR",
    "HOLLOW_METAL",
    "SHEET_METAL",
    "SAND",
    "BROKEN_CONCRETE",
    "VEHICLE_BODY",
    "VEHICLE_PART_SOLID",
    "VEHICLE_PART_HOLLOW",
    "BARREL",
    "BOTTLE",
    "SODA_CAN",
    "PISTOL",
    "RIFLE",
    "SHOPPING_CART",
    "LUNCHBOX",
    "BABY_RATTLE",
    "RUBBER_BALL",
];

/// The name of a Havok material code.
pub fn havok_name(code: u32) -> &'static str {
    HAVOK_MATERIALS[(code % 32) as usize]
}

/// The SOUN (Editor ID) a loose body of Havok material `code` plays when it knocks, hard
/// or light [hypothesis: by the names, `PHY<material>H` / `…L`, and `CWood…` whose files
/// are `fx\phy\wood\crate_*`; observed in FalloutNV.esm]. Where a family comes in sizes
/// rather than H and L, a hard knock takes the larger [design]. `None`: no sound for
/// that material (dirt, grass, water, sand).
pub fn phy_sound(code: u32, hard: bool) -> Option<&'static str> {
    let (h, l) = match code % 32 {
        0 | 10 => ("PHYGenericConcMediumH", "PHYGenericConcMediumL"),
        1 => ("PHYGenericClothH", "PHYGenericClothL"),
        3 | 24 => ("PHYBottleH", "PHYBottleL"),
        5 => ("PHYGenericMetalMediumH", "PHYGenericMetalMediumL"),
        6 | 7 => ("PHYSkinMediumH", "PHYSkinMediumL"),
        9 => ("CWoodMedium", "CWoodSmall"),
        11 | 15 => ("PHYGenericMetalLargeH", "PHYGenericMetalLargeL"),
        12 => ("CWoodLarge", "CWoodMedium"),
        13 => ("PHYChain", "PHYChain"),
        14 => ("PHYBottlecaps", "PHYBottlecaps"),
        16 => ("PHYGenericMetalHollowMedium", "PHYGenericMetalHollowSmall"),
        17 => ("PHYGenericMetalSheetMedium", "PHYGenericMetalSheetSmall"),
        19 => ("PHYGenericConcSmall", "PHYGenericConcSmall"),
        20 => ("PHYVehicleMetalBodyH", "PHYVehicleMetalBodyL"),
        21 => ("PHYVehicleMetalSolidH", "PHYVehicleMetalSolidL"),
        22 => ("PHYVehicleMetalHollowH", "PHYVehicleMetalHollowL"),
        23 => ("PHYBarrelH", "PHYBarrelL"),
        25 => ("PHYCanSodaH", "PHYCanSodaL"),
        26 => ("PHYWeaponPistolH", "PHYWeaponPistolL"),
        27 => ("PHYWeaponRifleH", "PHYWeaponRifleL"),
        28 => ("PHYShoppingCartH", "PHYShoppingCartL"),
        29 => ("PHYLunchboxH", "PHYLunchboxL"),
        30 => ("PHYBabyRattle", "PHYBabyRattle"),
        31 => ("PHYRubberBall", "PHYRubberBall"),
        _ => return None,
    };
    Some(if hard { h } else { l })
}

/// How hard a body was knocked between two velocities: the speed it lost along the way
/// it was going (all of it on landing, twice it on a bounce straight back) [design]. A
/// body gaining speed (falling, pushed by a shot) or at rest is not knocked.
pub fn knock_speed(before: [f32; 3], after: [f32; 3]) -> f32 {
    let speed = (before[0] * before[0] + before[1] * before[1] + before[2] * before[2]).sqrt();
    if speed < 1e-3 {
        return 0.0;
    }
    let along = (after[0] * before[0] + after[1] * before[1] + after[2] * before[2]) / speed;
    (speed - along).max(0.0)
}

/// The sound fields of the records that have them, by base FormID.
#[derive(Default)]
pub struct SoundForms {
    /// Every item record: (pick up, drop), as the record names them.
    pub items: HashMap<FormId, (Option<FormId>, Option<FormId>)>,
    /// Per item record type: records, those naming a pick-up sound, a drop sound.
    pub counts: BTreeMap<String, [u32; 3]>,
    /// Containers: (open, close).
    pub containers: HashMap<FormId, (Option<FormId>, Option<FormId>)>,
    /// Activators without a radio station: their activation sound.
    pub activators: HashMap<FormId, FormId>,
    /// The own sounds of the bases in [`OWN_SOUNDS`]: the sound and the kind (its index
    /// there).
    pub object_sounds: HashMap<FormId, (FormId, usize)>,
    /// … how many records of each kind name one.
    pub object_sound_counts: [u32; OWN_SOUNDS.len()],
    /// Doors' open sounds (SNAM), for load doors.
    pub door_open: HashMap<FormId, FormId>,
    /// Water types' sounds (WATR `SNAM` Sound [documented: xEdit]).
    pub waters: HashMap<FormId, FormId>,
}

/// The records whose placed references sound a sound of their own, the subrecord naming
/// it and what the log calls them [documented: xEdit wbDefinitionsFNV.pas ACTI `SNAM`
/// Sound - Looping, CONT `RNAM` Sound - Random/Looping, DOOR `BNAM` Sound - Looping, MSTT
/// `SNAM` Sound, STAT `RNAM` Sound - Looping/Random, LIGH `SNAM` Sound, TERM `SNAM` Sound -
/// Looping, MISC and KEYM `RNAM` Sound - Random/Looping] (audio.md §14.3, §15;
/// item-fields.md §2).
pub const OWN_SOUNDS: [(&[u8; 4], &[u8; 4], &str); 9] = [
    (b"ACTI", b"SNAM", "activators"),
    (b"CONT", b"RNAM", "containers"),
    (b"DOOR", b"BNAM", "doors"),
    (b"MSTT", b"SNAM", "moveable statics"),
    (b"STAT", b"RNAM", "statics"),
    (b"LIGH", b"SNAM", "lights"),
    (b"TERM", b"SNAM", "terminals"),
    (b"MISC", b"RNAM", "misc items"),
    (b"KEYM", b"RNAM", "keys"),
];

/// The kind in [`OWN_SOUNDS`] of statics: they never move, so their place is read once.
const STATIC_KIND: usize = 4;

const ITEM_SIGS: [&[u8; 4]; 12] =[b"MISC", b"KEYM", b"WEAP", b"ARMO", b"AMMO", b"ALCH", b"BOOK", b"NOTE", b"IMOD", b"CHIP", b"CCRD", b"CMNY"];

/// A FormID subrecord of a record (plugin `pi`) as a global FormID; null and unresolved:
/// `None`. The subrecords read here are all `formid` [documented: xEdit].
fn raw_fid(lo: &LoadOrder, pi: usize, r: &Record, sig: &[u8; 4]) -> Option<FormId> {
    let subs = r.subrecords().ok()?;
    let s = subs.iter().find(|s| &s.sig == sig)?;
    let local = u32::from_le_bytes(s.data.get(..4)?.try_into().ok()?);
    if local == 0 {
        return None;
    }
    fnv_sim::extract::global(lo, pi, local).ok().flatten()
}

impl SoundForms {
    pub fn build(lo: &LoadOrder) -> SoundForms {
        let mut out = SoundForms::default();
        for (&g, defs) in &lo.index {
            let Some(d) = defs.last() else { continue };
            let pi = d.plugin as usize;
            let r = lo.records[pi][d.ordinal as usize];
            if r.header.flags & 0x20 != 0 {
                continue;
            }
            let sig = &r.header.sig;
            let id = FormId(g);
            if let Some(kind) = OWN_SOUNDS.iter().position(|(s, _, _)| *s == sig) {
                if let Some(l) = raw_fid(lo, pi, r, OWN_SOUNDS[kind].1) {
                    out.object_sounds.insert(id, (l, kind));
                    out.object_sound_counts[kind] += 1;
                }
            }
            if ITEM_SIGS.contains(&sig) {
                let (up, down) = (raw_fid(lo, pi, r, b"YNAM"), raw_fid(lo, pi, r, b"ZNAM"));
                let c = out.counts.entry(String::from_utf8_lossy(sig).into_owned()).or_default();
                c[0] += 1;
                c[1] += up.is_some() as u32;
                c[2] += down.is_some() as u32;
                out.items.insert(id, (up, down));
            } else if sig == b"CONT" {
                let (open, close) = (raw_fid(lo, pi, r, b"SNAM"), raw_fid(lo, pi, r, b"QNAM"));
                if open.is_some() || close.is_some() {
                    out.containers.insert(id, (open, close));
                }
            } else if sig == b"ACTI" {
                if raw_fid(lo, pi, r, b"RNAM").is_some() {
                    continue;
                }
                if let Some(v) = raw_fid(lo, pi, r, b"VNAM") {
                    out.activators.insert(id, v);
                }
            } else if sig == b"WATR" {
                if let Some(l) = raw_fid(lo, pi, r, b"SNAM") {
                    out.waters.insert(id, l);
                }
            } else if sig == b"DOOR" {
                if let Some(o) = raw_fid(lo, pi, r, b"SNAM") {
                    out.door_open.insert(id, o);
                }
            }
        }
        out
    }
}

/// An actor other than the player went through the load door `door` (or came out of
/// it), which stands at `at` in the space being played: its open sound plays there.
#[derive(Message, Clone, Copy, Debug)]
pub struct DoorPassed {
    pub door: u32,
    pub at: [f32; 3],
}

/// The player activated a reference by hand (`crate::activate`): its activation sound.
#[derive(Message, Clone, Copy, Debug)]
pub struct Activated {
    pub reference: u32,
}

#[derive(Default, Debug)]
pub struct WorldSoundStats {
    /// Impacts by the material picked (`fnv_combat::fx::IMPACT_MATERIALS`).
    pub impacts: BTreeMap<&'static str, u32>,
    /// Impacts on a reference whose collision has no material (they count as dirt).
    pub impacts_no_material: u32,
    pub knocks: u32,
    pub knocks_hard: u32,
    /// Knocks of a material with no PHY sound.
    pub knocks_unsounded: u32,
    pub pickups: u32,
    pub drops: u32,
    /// Items moved in a container menu: taken by the player, put in.
    pub taken: u32,
    pub put: u32,
    /// Of those taken, by Take All: their sounds held back for `UIItemTakeAll`
    /// (`crate::ui_sounds`).
    pub taken_quietly: u32,
    pub boxes_opened: u32,
    pub boxes_closed: u32,
    pub container_opens: u32,
    pub container_closes: u32,
    pub activations: u32,
    /// Load doors gone through with an open sound.
    pub load_doors: u32,
    /// … by other actors (each side counted).
    pub actor_doors: u32,
    /// Placed objects with a sound of their own, and those in reach (sent to the ambient),
    /// with how many of each kind of [`OWN_SOUNDS`] were.
    pub object_sources: usize,
    pub object_sources_sent: HashSet<u32>,
    pub object_sources_by_kind: [u32; OWN_SOUNDS.len()],
    /// Water types whose sound was sent to the ambient.
    pub water_sources: HashSet<u32>,
    pub script_2d: u32,
    pub script_3d: u32,
    /// Script sounds held while a menu was open.
    pub script_held: u32,
}

type ObjectSource = (u32, FormId, usize, Option<[f32; 3]>);

/// A loose body being followed for knocks.
struct Tracked {
    vel: [f32; 3],
    last_knock: f64,
    /// Real time it was first seen.
    since: f64,
}

#[derive(Resource, Default)]
pub struct WorldSounds {
    forms: SoundForms,
    /// Per-form collision sound overrides loaded from `Data\NVSE\Plugins\SoundExtender\` (`*.json` / `*.txt`)
    /// (`docs/spec-notes/sound-extender.md`).
    sound_extender: fnv_game::sound_extender::SoundExtenderTable,
    /// SOUN FormIDs by Editor ID (from `Sfx`, the first frame it is there).
    by_edid: Option<HashMap<String, FormId>>,
    /// The player's inventory while a container menu is open, to hear what moves.
    carried: Option<BTreeMap<FormId, i32>>,
    /// The message box on screen (its text) and its chosen button.
    message: Option<(String, usize)>,
    tracked: HashMap<u32, Tracked>,
    steps: u64,
    picked: HashSet<u32>,
    dropped: HashSet<u32>,
    container: Option<u32>,
    held: Vec<SoundRequest>,
    /// Placed objects with a sound of their own, by the space they are in: (reference,
    /// sound, kind, the place of one that never moves); found the first frame.
    sources: Option<HashMap<crate::music::Space, Vec<ObjectSource>>>,
    /// The index over each space's `sources`, made the first frame the space is played
    /// ([`SourceIndex`]).
    index: HashMap<crate::music::Space, SourceIndex>,
    /// This frame's candidates and the sources in reach (reused).
    near: Vec<u32>,
    heard: Vec<(u32, [f32; 3])>,
    pub stats: WorldSoundStats,
}

/// The objects' own sounds of one space, indexed so that a frame visits only those that
/// can be in reach (`crate::sound_grid`) [design]. The answer is the walk over the whole
/// list's ([`scan_all`]): each candidate gets the same test ([`source_heard`]).
///
/// - A static's place is its placement and never changes (the list says so), and a
///   source of another kind is where its run-time state puts it, or at its placement
///   while it has none ([`ref_pos`]). So the grid holds every source at its placement,
///   and the sources other than statics that have a run-time state (`live`) are tested
///   every frame wherever they are, the grid's copy of them left out.
/// - Which of them have a state changes only when a state is made or dropped. A state
///   made adds to the world's count of states; one dropped (a cell reset) or made
///   with other flags moves [`fnv_sim::world::World::presence_serial`]; a world
///   restored starts from a serial no other world used. So `live` is made again when
///   that pair is not the one it was made at.
/// - The reaches are read from the sound forms once ([`Sfx`] gets its forms when it is
///   made; only tests add to them later, so a change in their number makes the index
///   again).
struct SourceIndex {
    /// Each entry's reach (`None`: its sound is not in the load order; never sent).
    reach: Vec<Option<f32>>,
    grid: crate::sound_grid::FixedGrid,
    /// The entries that are not statics, (reference, entry) in FormID order; those of
    /// them with a run-time state now (and the flag by entry), and the world's (presence
    /// serial, number of states) then.
    movable: Vec<(u32, u32)>,
    live: Vec<u32>,
    is_live: Vec<bool>,
    live_key: Option<(u64, usize)>,
    /// The number of sound forms the reaches were read from.
    sounds: usize,
}

/// The squared distance between two places, as the reach test measures it.
fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

/// Where a placed object's own sound is heard from when the listener at `eye` is within
/// its `reach` and the object is enabled.
fn source_heard(sim: &SimState, eye: [f32; 3], &(id, _, _, fixed): &ObjectSource, reach: f32) -> Option<[f32; 3]> {
    let at = fixed.or_else(|| ref_pos(sim, id))?;
    if dist2(at, eye) > reach * reach || !sim.world.is_enabled(FormId(id)).unwrap_or(false) {
        return None;
    }
    Some(at)
}

/// The reach of a sound: its maximum distance and a tenth more.
fn reach_of(sfx: &Sfx, sound: FormId) -> Option<f32> {
    sfx.forms.sounds.get(&sound).map(|s| s.max_distance * 1.1)
}

/// The world's number of reference states, when its iterator says it exactly (a
/// `BTreeMap`'s does); `None` otherwise, and the caller takes every frame as a change.
fn state_count(sim: &SimState) -> Option<usize> {
    let (lo, hi) = sim.world.changed().size_hint();
    (hi == Some(lo)).then_some(lo)
}

impl SourceIndex {
    fn build(list: &[ObjectSource], sim: &SimState, reach: impl Fn(FormId) -> Option<f32>, sounds: usize) -> SourceIndex {
        let reach: Vec<Option<f32>> = list.iter().map(|e| reach(e.1)).collect();
        let place = |&(id, _, _, fixed): &ObjectSource| fixed.or_else(|| sim.refs.get(FormId(id)).map(|p| p.pos));
        // The grid's reach is the one the squared test reads: a negative reach squares as
        // its size does.
        let grid = crate::sound_grid::FixedGrid::build(list.iter().zip(&reach).enumerate().filter_map(|(i, (e, r))| Some((i as u32, place(e)?, r.as_ref()?.abs()))));
        let mut movable: Vec<(u32, u32)> = list.iter().zip(&reach).enumerate().filter(|(_, (e, r))| e.3.is_none() && r.is_some()).map(|(i, (e, _))| (e.0, i as u32)).collect();
        movable.sort_unstable();
        SourceIndex { reach, grid, movable, live: Vec::new(), is_live: vec![false; list.len()], live_key: None, sounds }
    }

    /// The entries of `list` (the one the index was made over) heard at `eye`, and where
    /// from, in list order, into `out`; `near` is scratch.
    fn heard(&mut self, list: &[ObjectSource], sim: &SimState, eye: [f32; 3], near: &mut Vec<u32>, out: &mut Vec<(u32, [f32; 3])>) {
        let key = state_count(sim).map(|n| (sim.world.presence_serial(), n));
        if key.is_none() || key != self.live_key {
            // Both in FormID order (`World::changed` gives the states so): one walk down
            // the two lists.
            self.live.clear();
            let mut states = sim.world.changed().map(|(id, _)| id.0).peekable();
            for &(id, i) in &self.movable {
                while states.next_if(|&s| s < id).is_some() {}
                let live = states.peek() == Some(&id);
                self.is_live[i as usize] = live;
                if live {
                    self.live.push(i);
                }
            }
            self.live.sort_unstable();
            self.live_key = key;
        }
        self.grid.near(eye, |c, r| dist2(c, eye) > r * r, near);
        near.retain(|&i| !self.is_live[i as usize]);
        near.extend_from_slice(&self.live);
        near.sort_unstable();
        out.clear();
        for &i in near.iter() {
            let Some(reach) = self.reach[i as usize] else { continue };
            if let Some(at) = source_heard(sim, eye, &list[i as usize], reach) {
                out.push((i, at));
            }
        }
    }
}

/// The entries of `list` heard at `eye`, and where from, in list order, by testing every
/// one: what [`SourceIndex::heard`] must answer (the tests', and in debug builds a check
/// now and then).
#[cfg(any(test, debug_assertions))]
fn scan_all(list: &[ObjectSource], sim: &SimState, eye: [f32; 3], reach: impl Fn(FormId) -> Option<f32>) -> Vec<(u32, [f32; 3])> {
    let mut out = Vec::new();
    for (i, e) in list.iter().enumerate() {
        let Some(r) = reach(e.1) else { continue };
        if let Some(at) = source_heard(sim, eye, e, r) {
            out.push((i as u32, at));
        }
    }
    out
}

/// Log what a shot's impact picked (`crate::combat`), for the summary's count.
pub fn log_impact(s: &Sfx, hit: Option<u32>, actor: bool, code: Option<u32>, material: &'static str, impact: Option<FormId>) -> ImpactPicked {
    let i = impact.and_then(|i| s.forms.impacts.get(&i));
    let sounds: Vec<String> = i
        .map(|i| i.sound_1.into_iter().chain(i.sound_2).map(|x| s.forms.sounds.get(&x).and_then(|x| x.edid.clone()).unwrap_or_else(|| x.to_string())).collect())
        .unwrap_or_default();
    debug!(
        "impact: {} {}: collision material {}, impact material {material}, IPCT {}, sounds [{}]",
        if actor { "actor" } else { "object" },
        hit.map_or("terrain".to_string(), |h| format!("{h:08X}")),
        code.map_or(if actor { "-".to_string() } else { "none".to_string() }, |c| format!("{c} {}", havok_name(c))),
        i.and_then(|i| i.edid.as_deref()).unwrap_or("none"),
        sounds.join(", ")
    );
    ImpactPicked { material, no_material: !actor && hit.is_some() && code.is_none() }
}

/// What an impact picked, for the summary.
#[derive(Message, Clone, Copy, Debug)]
pub struct ImpactPicked {
    pub material: &'static str,
    /// A reference whose collision has no material.
    pub no_material: bool,
}

fn init(mut commands: Commands, d: Res<PlayWorld>, data: Option<Res<nvf_assets::GameData>>) {
    let t = std::time::Instant::now();
    let forms = SoundForms::build(d.lo);
    let sound_extender = match data.as_deref() {
        Some(gd) => fnv_game::sound_extender::SoundExtenderTable::load_from_vfs(d.lo, &gd.vfs),
        None => fnv_game::sound_extender::SoundExtenderTable::load_from_mod_files(d.lo),
    };
    let se_rep = sound_extender.report();
    if se_rep.files_loaded > 0 || se_rep.files_malformed > 0 {
        info!(
            "sound extender: {} file(s) ({} malformed), {} override(s) ({} rule(s) applied, {} skipped)",
            se_rep.files_loaded,
            se_rep.files_malformed,
            sound_extender.len(),
            se_rep.rules_applied,
            se_rep.total_skipped(),
        );
    }
    info!(
        "world sounds: items (records, with a pick-up sound, with a drop sound): {}; {} containers with open or close sounds, {} activators with an activation sound; sounds of their own: {}; read in {:.0} ms",
        forms.counts.iter().map(|(s, c)| format!("{s} {} {} {}", c[0], c[1], c[2])).collect::<Vec<_>>().join(", "),
        forms.containers.len(),
        forms.activators.len(),
        OWN_SOUNDS.iter().zip(forms.object_sound_counts).map(|((_, sub, what), n)| format!("{n} {what} ({})", String::from_utf8_lossy(*sub))).collect::<Vec<_>>().join(", "),
        t.elapsed().as_secs_f64() * 1000.0
    );
    commands.insert_resource(WorldSounds { forms, sound_extender, ..Default::default() });
}

/// SOUN FormIDs by Editor ID.
fn by_edid(sfx: &Sfx) -> HashMap<String, FormId> {
    sfx.forms.sounds.iter().filter_map(|(&id, s)| s.edid.clone().map(|e| (e, id))).collect()
}

/// The sound an item makes when picked up (`up`) or put down: its record's, else the
/// generic one [hypothesis: `UIItemGenericUp` / `UIItemGenericDown`, by their names;
/// no record links them].
fn item_sound(ws: &mut WorldSounds, sfx: Option<&Sfx>, item: FormId, up: bool) -> Option<FormId> {
    let &(u, d) = ws.forms.items.get(&item)?;
    let named = if up { u } else { d };
    if named.is_some() {
        return named;
    }
    let map = ws.by_edid.get_or_insert_with(|| sfx.map(by_edid).unwrap_or_default());
    map.get(if up { "UIItemGenericUp" } else { "UIItemGenericDown" }).copied()
}

impl WorldSounds {
    /// The sound an item makes when picked up (`up`) or put down, for the menus
    /// (`crate::ui_sounds`): as [`item_sound`] picks it.
    pub fn item_sound_of(&mut self, sfx: Option<&Sfx>, item: FormId, up: bool) -> Option<FormId> {
        item_sound(self, sfx, item, up)
    }
}

/// A SOUN's Editor ID for logs.
fn sound_name(sfx: Option<&Sfx>, sound: Option<FormId>) -> String {
    match sound {
        None => "none".to_string(),
        Some(s) => sfx.and_then(|x| x.forms.sounds.get(&s)).and_then(|x| x.edid.clone()).unwrap_or_else(|| s.to_string()),
    }
}

/// Where a reference is now (game space).
fn ref_pos(sim: &SimState, id: u32) -> Option<[f32; 3]> {
    sim.world.peek(FormId(id)).map(|s| s.pos).or_else(|| sim.refs.get(FormId(id)).map(|p| p.pos))
}

/// Resolve the collision knock sound for a dynamic body: a SoundExtender per-form override
/// on its base form takes precedence over Havok-material [`phy_sound`] lookup [hypothesis:
/// docs/spec-notes/sound-extender.md].
fn knock_sound(
    sound_extender: &fnv_game::sound_extender::SoundExtenderTable,
    phy: &HashMap<String, FormId>,
    base: Option<FormId>,
    material: Option<u32>,
    hard: bool,
) -> Option<FormId> {
    if let Some(sound) = base.and_then(|b| sound_extender.resolve(b)) {
        return Some(sound);
    }
    material.and_then(|c| phy_sound(c, hard)).and_then(|e| phy.get(e).copied())
}

/// Knocks of loose bodies (see the module notes).
fn knocks(
    mut ws: ResMut<WorldSounds>,
    sfx: Option<Res<Sfx>>,
    phys: Option<Res<PhysState>>,
    sim: Option<Res<SimState>>,
    time: Res<Time<Real>>,
    mut out: MessageWriter<PlaySound>,
) {
    let (Some(sfx), Some(phys)) = (sfx, phys) else { return };
    let ws = &mut *ws;
    if ws.by_edid.is_none() {
        // Once: what the actors' records say they are made of, for the log.
        let mut n: BTreeMap<&str, u32> = BTreeMap::new();
        for &k in sfx.forms.actor_impact.values() {
            *n.entry(fnv_combat::fx::IMPACT_MATERIALS[k]).or_default() += 1;
        }
        info!("world sounds: actors' impact materials (NPC_ and CREA NAM4): {}", n.iter().map(|(m, c)| format!("{m} {c}")).collect::<Vec<_>>().join(", "));
    }
    let phy = ws.by_edid.get_or_insert_with(|| by_edid(&sfx));
    let sound_extender = &ws.sound_extender;
    let now = time.elapsed_secs_f64();
    let steps = phys.world.steps();
    let stepped = steps != ws.steps;
    ws.steps = steps;
    let bodies = phys.placed_dynamic.keys().copied().chain(phys.created.iter().filter(|(_, c)| c.dynamic).map(|(&id, _)| id));
    let mut seen = HashSet::new();
    for id in bodies {
        let Some(b) = phys.world.ref_bodies(id).first().copied() else { continue };
        seen.insert(id);
        let vel = if b.sleeping { [0.0; 3] } else { b.linvel };
        let Some(t) = ws.tracked.get_mut(&id) else {
            ws.tracked.insert(id, Tracked { vel, last_knock: f64::NEG_INFINITY, since: now });
            continue;
        };
        let before = std::mem::replace(&mut t.vel, vel);
        if !stepped {
            continue;
        }
        let speed = knock_speed(before, vel);
        if speed < KNOCK_LIGHT || now - t.last_knock < KNOCK_GAP || now - t.since < KNOCK_SETTLE {
            continue;
        }
        t.last_knock = now;
        let hard = speed >= KNOCK_HARD;
        let code = phys.world.ref_material(id);
        let base = sim.as_deref().and_then(|s| s.base_of(id));
        let sound = knock_sound(sound_extender, phy, base, code, hard);
        ws.stats.knocks += 1;
        ws.stats.knocks_hard += hard as u32;
        match sound {
            Some(sound) => {
                out.write(PlaySound { sound, at: Some(b.translation) });
            }
            None => ws.stats.knocks_unsounded += 1,
        }
        debug!(
            "knock: {id:08X} at {speed:.0} units/s ({}), {:.1} s after its collision came, material {}, sound {}",
            if hard { "hard" } else { "light" },
            now - t.since,
            code.map_or("none".to_string(), |c| format!("{c} {}", havok_name(c))),
            sound_name(Some(&sfx), sound)
        );
    }
    ws.tracked.retain(|id, _| seen.contains(id));
}

/// Pick-ups, drops, containers, activators and the sounds scripts ask for.
#[allow(clippy::too_many_arguments)]
fn objects(
    mut ws: ResMut<WorldSounds>,
    mut changed: MessageReader<RefChanged>,
    mut activated: MessageReader<Activated>,
    mut passed: MessageReader<DoorPassed>,
    mut impacts: MessageReader<ImpactPicked>,
    menu: Option<Res<ActiveMenu>>,
    state: Res<State<PlayState>>,
    sim: Res<SimState>,
    script: Option<ResMut<crate::script::ScriptState>>,
    player: Option<Res<Player>>,
    sfx: Option<Res<Sfx>>,
    panel: Option<Res<crate::message_menu::MessagePanel>>,
    mut ui: MessageReader<crate::ui_sounds::UiSound>,
    (mut out, mut by_actors): (MessageWriter<PlaySound>, MessageWriter<ActorSound>),
    mut script_sounds: ResMut<crate::sfx::ScriptSounds>,
) {
    let ws = &mut *ws;
    let sfx = sfx.as_deref();
    // Take All plays one sound of its own instead of every item's (`crate::ui_sounds`).
    let take_all = ui.read().any(|u| u.event == crate::ui_sounds::UiEvent::TakeAll);
    for i in impacts.read() {
        *ws.stats.impacts.entry(i.material).or_default() += 1;
        ws.stats.impacts_no_material += i.no_material as u32;
    }
    // Quick Loot's takes sound as the container menu's (`crate::quick_loot`): the player's
    // inventory is followed while its list shows, but only a frame it took something in
    // is sounded (the game runs on under it: a shot's ammo, a hotkeyed Stimpak are not takes).
    let menu_open = menu.as_ref().and_then(|m| m.0.as_ref()).is_some_and(|m| matches!(m.mode, Mode::Container(_)));
    let in_container = menu_open || crate::quick_loot::holds_input();
    // Read every frame, so a take flagged while the container menu is open is not left to
    // sound a later frame after it closes.
    let took = crate::quick_loot::took_this_frame() | menu_open;
    for &RefChanged { form_id: id, restored, .. } in changed.read() {
        let Some(s) = sim.world.peek(FormId(id)) else { continue };
        if !ws.forms.items.contains_key(&s.base) {
            continue;
        }
        // In the save as picked up or dropped: heard before it.
        if restored {
            if s.deleted() {
                ws.picked.insert(id);
            } else if id >= 0xFF00_0000 {
                ws.dropped.insert(id);
            }
            continue;
        }
        let (base, pos) = (s.base, s.pos);
        if s.deleted() {
            // Taken from a container menu (a corpse's dropped weapon, `menu::model::Row::
            // reference`): the menu's own take sound below plays instead.
            if took && s.dropped_by.is_some() {
                ws.picked.insert(id);
                continue;
            }
            if ws.picked.insert(id) {
                ws.stats.pickups += 1;
                let sound = item_sound(ws, sfx, base, true);
                info!("world sounds: {id:08X} ({base}) picked up, sound {}", sound_name(sfx, sound));
                if let Some(sound) = sound {
                    out.write(PlaySound { sound, at: Some(pos) });
                }
            }
        } else if id >= 0xFF00_0000 && ws.dropped.insert(id) {
            ws.stats.drops += 1;
            let sound = item_sound(ws, sfx, base, false);
            info!("world sounds: {id:08X} ({base}) dropped, sound {}", sound_name(sfx, sound));
            if let Some(sound) = sound {
                out.write(PlaySound { sound, at: Some(pos) });
            }
        }
    }

    // Items moved between the player and an open container: what the player gains is
    // taken (its pick-up sound), what the player loses is put in (its drop sound), heard
    // at the listener [hypothesis: the same sounds as in the world].
    let now_carried: Option<BTreeMap<FormId, i32>> =
        in_container.then(|| sim.world.peek(FormId::PLAYER_REF).and_then(|p| p.inventory.as_ref()).map(|i| i.iter().collect()).unwrap_or_default());
    if let (Some(before), Some(now)) = (ws.carried.as_ref().filter(|_| took), now_carried.as_ref()) {
        let mut moved: Vec<(FormId, bool)> = Vec::new();
        for (&item, &n) in now {
            if n > before.get(&item).copied().unwrap_or(0) {
                moved.push((item, true));
            }
        }
        for (&item, &n) in before {
            if n > now.get(&item).copied().unwrap_or(0) {
                moved.push((item, false));
            }
        }
        for (item, taken) in moved {
            let sound = item_sound(ws, sfx, item, taken);
            if taken {
                ws.stats.taken += 1;
            } else {
                ws.stats.put += 1;
            }
            let quiet = take_all && taken;
            ws.stats.taken_quietly += quiet as u32;
            info!(
                "world sounds: {item} {} the container, sound {}{}",
                if taken { "taken from" } else { "put in" },
                sound_name(sfx, sound),
                if quiet { " (held back: Take All)" } else { "" }
            );
            if let Some(sound) = sound.filter(|_| !quiet) {
                out.write(PlaySound { sound, at: None });
            }
        }
    }
    ws.carried = now_carried;

    // The message box: a sound when one opens [design: `UIPopUpMessageWindow`, by its
    // name; neither MESG nor message_menu.xml names one], the buttons' click when one is
    // picked (the box goes) and their mouse-over sound when the frame moves [observed:
    // `UIMenuOK` and `UIMenuFocus` in menus\prefabs\list_box_template.xml, which the
    // message box's buttons include].
    let shown = panel.as_ref().and_then(|p| p.view.as_ref().map(|v| (format!("{:?}|{}|{:?}", v.title, v.text, v.buttons), p.selected)));
    if shown != ws.message {
        let was = ws.message.clone();
        let (was, now) = (was.as_ref(), shown.as_ref());
        let mut ui = |edid: &str, what: &str, ws: &mut WorldSounds| {
            let map = ws.by_edid.get_or_insert_with(|| sfx.map(by_edid).unwrap_or_default());
            let sound = map.get(edid).copied();
            info!("world sounds: message box {what}, sound {edid}{}", if sound.is_some() { "" } else { " (not found)" });
            if let Some(sound) = sound {
                out.write(PlaySound { sound, at: None });
            }
        };
        let (closed, opened) = match (was, now) {
            (Some(a), Some(b)) => (a.0 != b.0, a.0 != b.0),
            (a, b) => (a.is_some(), b.is_some()),
        };
        if closed {
            ws.stats.boxes_closed += 1;
            ui("UIMenuOK", "button picked", ws);
        }
        if opened {
            ws.stats.boxes_opened += 1;
            ui("UIPopUpMessageWindow", "opened", ws);
        } else if let (Some(a), Some(b)) = (was, now) {
            if a.1 != b.1 {
                ui("UIMenuFocus", "choice moved", ws);
            }
        }
        ws.message = shown;
    }

    // The container menu.
    let open = menu.as_ref().and_then(|m| m.0.as_ref()).and_then(|m| match m.mode {
        Mode::Container(c) => Some(c.0),
        // A merchant is an actor, not a container: no container sound [design].
        Mode::Inventory | Mode::Barter(_) => None,
    });
    if open != ws.container {
        let sound_of = |c: u32, opening: bool| {
            let base = sim.base_of(c)?;
            let &(o, cl) = ws.forms.containers.get(&base)?;
            if opening {
                o
            } else {
                cl
            }
        };
        for (c, opening) in [(ws.container, false), (open, true)] {
            let Some(c) = c else { continue };
            let sound = sound_of(c, opening);
            if opening {
                ws.stats.container_opens += 1;
            } else {
                ws.stats.container_closes += 1;
            }
            info!("world sounds: container {c:08X} {}, sound {}", if opening { "opened" } else { "closed" }, sound_name(sfx, sound));
            if let Some(sound) = sound {
                out.write(PlaySound { sound, at: ref_pos(&sim, c) });
            }
        }
        ws.container = open;
    }

    for &DoorPassed { door, at } in passed.read() {
        let Some(&sound) = sim.base_of(door).and_then(|b| ws.forms.door_open.get(&b)) else { continue };
        ws.stats.actor_doors += 1;
        info!("world sounds: an actor passed load door {door:08X}, sound {}", sound_name(sfx, Some(sound)));
        by_actors.write(ActorSound { sound, at });
    }
    for &Activated { reference } in activated.read() {
        // A load door: its open sound as the player goes through [hypothesis].
        let leads_away = sim.refs.get(FormId(reference)).is_some_and(|p| p.teleport.is_some());
        if let Some(&sound) = sim.base_of(reference).and_then(|b| ws.forms.door_open.get(&b)).filter(|_| leads_away) {
            ws.stats.load_doors += 1;
            info!("world sounds: load door {reference:08X} used, sound {}", sound_name(sfx, Some(sound)));
            out.write(PlaySound { sound, at: None });
            continue;
        }
        let Some(&sound) = sim.base_of(reference).and_then(|b| ws.forms.activators.get(&b)) else { continue };
        ws.stats.activations += 1;
        info!("world sounds: activator {reference:08X} used, sound {}", sound_name(sfx, Some(sound)));
        out.write(PlaySound { sound, at: ref_pos(&sim, reference) });
    }

    // Items a script gave the player (caps, #3095): their pick-up sound at the listener.
    for item in std::mem::take(&mut script_sounds.items_added) {
        let sound = item_sound(ws, sfx, item, true);
        info!("world sounds: {item} given by a script, sound {}", sound_name(sfx, sound));
        if let Some(sound) = sound {
            out.write(PlaySound { sound, at: None });
        }
    }

    // Scripts: the engine's answers for the sounds already played, StopSound's stops,
    // and the new sounds (`fnv_game::nvse_sound`).
    if let Some(rt) = script.and_then(|s| s.into_inner().rt.as_mut()) {
        for (id, seconds) in script_sounds.heard.drain(..) {
            rt.game.gaps.sounds.heard(id, seconds);
        }
        script_sounds.stops.append(&mut rt.game.gaps.sounds.stops);
        // Sounds made for files and the fades (`fnv_game::nvse_jg9`), before the plays.
        let jg9 = &mut rt.game.other.jg9;
        script_sounds.paths.append(&mut jg9.new_paths);
        script_sounds.fades_in.append(&mut jg9.fades_in);
        script_sounds.fades_out.append(&mut jg9.fades_out);
        ws.held.append(&mut rt.game.sound_requests);
    }
    let in_game = *state.get() == PlayState::Playing;
    let eye = player.as_deref().map(Player::eye);
    let mut held = Vec::new();
    for r in std::mem::take(&mut ws.held) {
        if !in_game && !r.system {
            held.push(r);
            continue;
        }
        // PlaySound3DNode's node (`fnv_game::nvse_jip14`), else the reference.
        let at = match (r.point, r.at) {
            (Some(p), _) => Some(p),
            (None, None) => None,
            (None, Some(FormId::PLAYER_REF)) => eye,
            (None, Some(at)) => ref_pos(&sim, at.0),
        };
        if r.at.is_some() {
            ws.stats.script_3d += 1;
        } else {
            ws.stats.script_2d += 1;
        }
        info!("world sounds: a script plays {}{}", r.sound, r.at.map_or(String::new(), |a| format!(" at {a}")));
        script_sounds.plays.push((r.id, r.sound, at));
    }
    if !held.is_empty() && held.len() != ws.stats.script_held as usize {
        ws.stats.script_held = ws.stats.script_held.max(held.len() as u32);
    }
    ws.held = held;
}

/// Placed objects' own sounds near the listener, sent to the ambient every frame (see
/// the module notes).
fn object_sources(
    mut ws: ResMut<WorldSounds>,
    sim: Res<SimState>,
    loc: Res<crate::Location>,
    player: Option<Res<Player>>,
    sfx: Option<Res<Sfx>>,
    #[cfg(debug_assertions)] mut frames: Local<u32>,
    mut out: MessageWriter<SoundSource>,
) {
    let (Some(player), Some(sfx)) = (player, sfx) else { return };
    let ws = &mut *ws;
    if ws.sources.is_none() {
        let mut by: HashMap<crate::music::Space, Vec<ObjectSource>> = HashMap::new();
        for p in sim.refs.refs.values() {
            let (Some(&(sound, kind)), Some(cell)) = (ws.forms.object_sounds.get(&p.base), p.cell) else { continue };
            let space = sim.refs.cell_world.get(&cell).map_or(crate::music::Space::Interior(cell.0), |w| crate::music::Space::World(w.0));
            by.entry(space).or_default().push((p.id.0, sound, kind, (kind == STATIC_KIND).then_some(p.pos)));
        }
        ws.stats.object_sources = by.values().map(Vec::len).sum();
        info!("world sounds: {} placed objects with a sound of their own", ws.stats.object_sources);
        ws.sources = Some(by);
    }
    let eye = player.eye();
    let space = crate::music::Space::of(&loc);
    let Some(list) = ws.sources.as_ref().and_then(|s| s.get(&space)) else { return };
    let sounds = sfx.forms.sounds.len();
    if ws.index.get(&space).is_none_or(|x| x.sounds != sounds) {
        let built = SourceIndex::build(list, &sim, |s| reach_of(&sfx, s), sounds);
        debug!("world sounds: {} objects' own sounds in this space, {} indexed", list.len(), built.grid.len());
        ws.index.insert(space, built);
    }
    let index = ws.index.get_mut(&space).expect("made above");
    index.heard(list, &sim, eye, &mut ws.near, &mut ws.heard);
    #[cfg(debug_assertions)]
    {
        // The walk over the whole list, now and then, as the reference.
        *frames = frames.wrapping_add(1);
        if *frames % 64 == 1 {
            let bits = |v: &[(u32, [f32; 3])]| v.iter().map(|&(i, at)| (i, at.map(f32::to_bits))).collect::<Vec<_>>();
            debug_assert_eq!(bits(&ws.heard), bits(&scan_all(list, &sim, eye, |s| reach_of(&sfx, s))), "world sounds: the index lost or added a source");
        }
    }
    for &(i, at) in &ws.heard {
        let (id, sound, kind, _) = list[i as usize];
        if ws.stats.object_sources_sent.insert(id) {
            ws.stats.object_sources_by_kind[kind] += 1;
        }
        out.write(SoundSource { key: SoundSource::key(1, id), sound, at, looping: false });
    }
}

/// The water near the listener sounds its type's sound (audio.md §15.2): per water type,
/// one source at the nearest point of the surfaces of that type, while it is in reach.
#[allow(clippy::too_many_arguments)]
fn water_sources(
    mut ws: ResMut<WorldSounds>,
    phys: Option<Res<PhysState>>,
    d: Res<PlayWorld>,
    sim: Res<SimState>,
    player: Option<Res<Player>>,
    sfx: Option<Res<Sfx>>,
    mut placed_types: Local<HashMap<u32, Option<u32>>>,
    mut out: MessageWriter<SoundSource>,
) {
    let (Some(phys), Some(player), Some(sfx)) = (phys, player, sfx) else { return };
    let ws = &mut *ws;
    if ws.forms.waters.is_empty() {
        return;
    }
    let eye = player.eye();
    let world = d.index.worldspaces.get(&phys.worldspace);
    // The nearest surface point of each water type and its distance².
    let mut nearest: HashMap<u32, ([f32; 3], f32)> = HashMap::new();
    let mut offer = |water: Option<u32>, at: [f32; 3]| {
        let Some(water) = water else { return };
        let d2 = (at[0] - eye[0]).powi(2) + (at[1] - eye[1]).powi(2) + (at[2] - eye[2]).powi(2);
        if nearest.get(&water).is_none_or(|n| d2 < n.1) {
            nearest.insert(water, (at, d2));
        }
    };
    for (area, surface) in phys.world.water() {
        let water = match (area, phys.space) {
            (fnv_phys::WaterArea::Exterior(gx, gy), _) => {
                let cell = world.and_then(|w| w.grid.get(&(gx, gy))).and_then(|id| d.index.cells.get(id));
                cell.and_then(|c| c.water_type).or(world.and_then(|w| w.water))
            }
            (fnv_phys::WaterArea::Interior, crate::phys::cells::Space::Interior(id)) => d.index.cells.get(&id).and_then(|c| c.water_type),
            _ => None,
        };
        // The point of the area's surface under or over the listener, else its nearest.
        let at = match area.bounds() {
            Some((sw, size)) => [eye[0].clamp(sw[0], sw[0] + size), eye[1].clamp(sw[1], sw[1] + size), surface],
            None => [eye[0], eye[1], surface],
        };
        offer(water, at);
    }
    for w in phys.world.placed_water() {
        let water = *placed_types.entry(w.form_id).or_insert_with(|| {
            let base = sim.refs.get(FormId(w.form_id))?.base;
            fnv_world::sky::placeable_water_type(d.lo, d.schema, base.0)
        });
        // Over the box: the surface above or below the listener; else its middle [design].
        let at = if w.covers(eye[0], eye[1]) { [eye[0], eye[1], w.surface] } else { [w.centre[0], w.centre[1], w.surface] };
        offer(water, at);
    }
    for (water, (at, d2)) in nearest {
        let Some(&sound) = ws.forms.waters.get(&FormId(water)) else { continue };
        let Some(reach) = sfx.forms.sounds.get(&sound).map(|s| s.max_distance * 1.1) else { continue };
        if d2 > reach * reach {
            continue;
        }
        ws.stats.water_sources.insert(water);
        out.write(SoundSource { key: SoundSource::key(WATER_KIND, water), sound, at, looping: false });
    }
}

/// The [`SoundSource::key`] kind of a water type's sound.
const WATER_KIND: u8 = 5;

fn report(mut exit: MessageReader<AppExit>, ws: Option<Res<WorldSounds>>) {
    let (Some(_), Some(ws)) = (exit.read().last(), ws) else { return };
    let s = &ws.stats;
    let impacts: Vec<String> = s.impacts.iter().map(|(m, n)| format!("{m} {n}")).collect();
    info!(
        "world sounds summary: impacts by material: {} ({} on collision without a material); {} knocks ({} hard, {} of a material without a sound); {} pick-ups, {} drops; {} items taken from and {} put in containers; containers opened {} and closed {} times; {} activator sounds; {} message boxes opened, {} closed; scripts played {} sounds and {} placed; {} load doors ({} sides passed by other actors); {} objects' own sounds in reach ({}); {} water sounds in reach",
        if impacts.is_empty() { "none".to_string() } else { impacts.join(", ") },
        s.impacts_no_material,
        s.knocks,
        s.knocks_hard,
        s.knocks_unsounded,
        s.pickups,
        s.drops,
        s.taken,
        s.put,
        s.container_opens,
        s.container_closes,
        s.activations,
        s.boxes_opened,
        s.boxes_closed,
        s.script_2d,
        s.script_3d,
        s.load_doors,
        s.actor_doors,
        s.object_sources_sent.len(),
        OWN_SOUNDS.iter().zip(s.object_sources_by_kind).filter(|(_, n)| *n > 0).map(|((_, _, what), n)| format!("{what} {n}")).collect::<Vec<_>>().join(", "),
        s.water_sources.len()
    );
}

/// Session state kept by [`script_sound_files`] so `SetSoundSourceFile`,
/// `SetSoundTraitNumeric`, and `TempCloneForm` SOUN aliases reach `Sfx` before
/// playback and reset cleanly on a new session or load.
#[derive(Default)]
struct ScriptSoundSync {
    seen_sources: HashMap<FormId, String>,
    seen_traits: HashMap<(FormId, i64), u64>,
    seen_clones: HashMap<FormId, FormId>,
    originals: HashMap<FormId, fnv_combat::fx::Sound>,
    active_clones: HashSet<FormId>,
}

/// Apply the direct `SetSoundSourceFile` and `SetSoundTraitNumeric` (`0..=11`) overrides
/// for `id` (including overrides snapshotted at `TempCloneForm` time) onto `sound`.
fn apply_sound_overrides(game: &fnv_game::Game, id: FormId, sound: &mut fnv_combat::fx::Sound) {
    if let Some(path) = game.sound_source_override(id) {
        let trimmed = path.trim();
        sound.file = (!trimmed.is_empty()).then(|| trimmed.to_string());
    }
    for slot in 0..=11 {
        let Some(v) = game.sound_trait_override(id, slot) else { continue };
        match slot {
            0 => sound.min_distance = v.max(0.0) as f32,
            1 => sound.max_distance = v.max(0.0) as f32,
            2 => sound.frequency_adjust_percent = v as f32,
            3 => sound.static_attenuation_db = (-v).max(0.0) as f32,
            4 => sound.stop_hour = (v.clamp(0.0, 255.0) * 24.0 / 256.0) as f32,
            5 => sound.start_hour = (v.clamp(0.0, 255.0) * 24.0 / 256.0) as f32,
            6..=10 => {
                let curve = sound.attenuation_curve.get_or_insert(fnv_combat::fx::DEFAULT_ATTENUATION_CURVE);
                curve[(slot - 6) as usize] = v.clamp(0.0, 100.0) as f32;
            }
            11 => sound.reverb_attenuation_percent = v.clamp(0.0, 100.0) as f32,
            _ => {}
        }
    }
}

/// Sync `SetSoundSourceFile`, `SetSoundTraitNumeric`, and `TempCloneForm` SOUN aliases
/// from `game` into `sounds`. Returns `true` when any entry changed.
fn sync_script_sounds(game: &fnv_game::Game, sounds: &mut HashMap<FormId, fnv_combat::fx::Sound>, state: &mut ScriptSoundSync) -> bool {
    let family = fnv_game::nvse_traits::SOUND_TRAIT_FAMILY;
    let sources_same = game.traits.sources.len() == state.seen_sources.len()
        && game.traits.sources.iter().all(|(k, v)| state.seen_sources.get(k) == Some(v));
    let mut sound_trait_count = 0usize;
    let mut traits_same = true;
    for (&(id, fam, slot), &val) in &game.traits.values {
        if fam != family || !(0..=11).contains(&slot) {
            continue;
        }
        sound_trait_count += 1;
        if state.seen_traits.get(&(id, slot)) != Some(&val.to_bits()) {
            traits_same = false;
        }
    }
    traits_same &= sound_trait_count == state.seen_traits.len();

    let mut sound_clone_count = 0usize;
    let mut clones_same = true;
    for (&clone_id, &parent) in game.clones() {
        let origin = game.unclone(clone_id);
        let is_sound = !game.clones().contains_key(&origin)
            && (state.originals.contains_key(&origin) || (sounds.contains_key(&origin) && !state.active_clones.contains(&origin)));
        if !is_sound {
            continue;
        }
        sound_clone_count += 1;
        if state.seen_clones.get(&clone_id) != Some(&parent) {
            clones_same = false;
        }
    }
    clones_same &= sound_clone_count == state.seen_clones.len();

    if sources_same && traits_same && clones_same {
        return false;
    }

    // Drain all previously materialized clone entries before rebuilding so disappeared
    // clones and FormIDs reused for non-SOUN clones in a fresh session never leak.
    for id in state.active_clones.drain() {
        sounds.remove(&id);
    }

    // Collect authored SOUN IDs that currently have a direct source or numeric trait override.
    let mut overridden_authored: HashSet<FormId> = HashSet::new();
    for &id in game.traits.sources.keys() {
        if !game.clones().contains_key(&id) && (state.originals.contains_key(&id) || sounds.contains_key(&id)) {
            overridden_authored.insert(id);
        }
    }
    for &(id, fam, slot) in game.traits.values.keys() {
        if fam == family && (0..=11).contains(&slot) && !game.clones().contains_key(&id) && (state.originals.contains_key(&id) || sounds.contains_key(&id)) {
            overridden_authored.insert(id);
        }
    }

    // Restore previously mutated authored SOUNs before re-applying active overrides.
    state.originals.retain(|id, orig| {
        if let Some(entry) = sounds.get_mut(id) {
            *entry = orig.clone();
        }
        overridden_authored.contains(id)
    });

    // Apply overrides to authored SOUN records, snapshotting each authored record once.
    for id in overridden_authored {
        let Some(entry) = sounds.get_mut(&id) else { continue };
        state.originals.entry(id).or_insert_with(|| entry.clone());
        apply_sound_overrides(game, id, entry);
        if let Some(v) = game.traits.sources.get(&id) {
            if state.seen_sources.get(&id) != Some(v) {
                let label = entry.edid.as_deref().unwrap_or("a sound");
                if v.trim().is_empty() {
                    info!("world sounds: {label} cleared sound file (SetSoundSourceFile)");
                } else {
                    info!("world sounds: {label} plays from {v} (SetSoundSourceFile)");
                }
            }
        }
    }

    // Materialize/update every TempCloneForm SOUN alias from its authored origin's base Sound,
    // clearing `edid` so EDID-based lookups never accidentally pick a clone over the original.
    let mut next_clones: HashMap<FormId, FormId> = HashMap::new();
    for (&clone_id, &parent) in game.clones() {
        let origin = game.unclone(clone_id);
        if game.clones().contains_key(&origin) || state.active_clones.contains(&origin) {
            continue;
        }
        let Some(mut cloned) = state.originals.get(&origin).or_else(|| sounds.get(&origin)).cloned() else {
            continue;
        };
        cloned.edid = None;
        apply_sound_overrides(game, clone_id, &mut cloned);
        if let Some(v) = game.traits.sources.get(&clone_id) {
            if state.seen_sources.get(&clone_id) != Some(v) {
                if v.trim().is_empty() {
                    info!("world sounds: {clone_id} (clone of {origin}) cleared sound file (SetSoundSourceFile)");
                } else {
                    info!("world sounds: {clone_id} (clone of {origin}) plays from {v} (SetSoundSourceFile)");
                }
            }
        }
        sounds.insert(clone_id, cloned);
        state.active_clones.insert(clone_id);
        next_clones.insert(clone_id, parent);
    }

    state.seen_sources = game.traits.sources.clone();
    state.seen_traits = game
        .traits
        .values
        .iter()
        .filter(|&(&(_, fam, slot), _)| fam == family && (0..=11).contains(&slot))
        .map(|(&(id, _, slot), &v)| ((id, slot), v.to_bits()))
        .collect();
    state.seen_clones = next_clones;
    true
}

/// SetSoundSourceFile, SetSoundTraitNumeric, and TempCloneForm SOUN sync
/// (`fnv_game::nvse_traits`): a SOUN (or temporary clone) whose file or numeric traits
/// a script changed plays with those values [documented: SetSoundSourceFile,
/// SetSoundTraitNumeric, TempCloneForm; observed: Titans of the New West points
/// FSTArmorPowerL/R at its silence folders (`fx\fst\TNW_SilenceL\`) (#2541)].
fn script_sound_files(script: Option<Res<crate::script::ScriptState>>, sfx: Option<ResMut<Sfx>>, mut state: Local<ScriptSoundSync>) {
    let (Some(script), Some(mut sfx)) = (script, sfx) else { return };
    let Some(rt) = script.rt.as_ref() else { return };
    sync_script_sounds(&rt.game, &mut sfx.forms.sounds, &mut state);
}

/// Session state kept for syncing `Game::weapon_sound_changes` (slots `0..=11`) and
/// `TempCloneForm` `WEAP` aliases into `Sfx::forms::weapons` (docs/spec-notes/weapon-sound-bridge.md).
#[derive(Default)]
struct WeaponFxSync {
    stamp: Option<(u64, u64)>,
    originals: HashMap<FormId, fnv_combat::fx::WeaponFx>,
    inserted: HashSet<FormId>,
    active_clones: HashSet<FormId>,
}

/// Session state kept for syncing `Game::weapon_sound_changes` (slots `12..=13`, `YNAM`
/// pickup and `ZNAM` putdown) and `TempCloneForm` `WEAP` aliases into `WorldSounds::forms::items`.
#[derive(Default)]
struct WeaponItemSoundSync {
    stamp: Option<(u64, u64)>,
    originals: HashMap<FormId, (Option<FormId>, Option<FormId>)>,
    inserted: HashSet<FormId>,
    active_clones: HashSet<FormId>,
}

/// Shared resource holding weapon-sound sync state across both frame sync points
/// (before `PlaySet::Activation` and after `PlaySet::Scripts` before `PlaySet::Ui`) so
/// combat, UI, world pickup/drop, and NPC idle audio all observe live `SetWeaponSound`
/// overrides without 1-frame lag or stale state across loads [design].
#[derive(Resource, Default)]
pub struct WeaponSoundSync {
    weapons: WeaponFxSync,
    items: WeaponItemSoundSync,
}

/// Order-independent digest of `game.clones()`, so adding, removing, or reassigning a
/// `TempCloneForm` entry invalidates weapon-sound sync even when no sound slot changed yet.
fn weapon_clones_stamp(game: &fnv_game::Game) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut n = 0u64;
    let mut sum = 0u64;
    for (&clone_id, &parent) in game.clones() {
        let origin = game.unclone(clone_id);
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (clone_id.0, parent.0, origin.0).hash(&mut h);
        sum = sum.wrapping_add(h.finish());
        n += 1;
    }
    sum ^ n.rotate_left(32)
}

/// Apply JIP `SetWeaponSound` slots `0..=11` onto `fx`, preserving untouched slots and
/// non-sound fields (`shell_casing_model`, `impact_dataset`) [documented: GECK wiki
/// SetWeaponSound; design: docs/spec-notes/weapon-sound-bridge.md].
fn apply_weapon_fx_change(fx: &mut fnv_combat::fx::WeaponFx, change: &fnv_game::nvse_gaps::WeaponSoundChange) {
    if let Some(s) = change.slots[0] { fx.shoot_3d = s; }
    if let Some(s) = change.slots[1] { fx.shoot_dist = s; }
    if let Some(s) = change.slots[2] { fx.shoot_2d = s; }
    if let Some(s) = change.slots[3] { fx.shoot_loop = s; }
    if let Some(s) = change.slots[4] { fx.no_ammo = s; }
    if let Some(s) = change.slots[5] { fx.block = s; }
    if let Some(s) = change.slots[6] { fx.idle = s; }
    if let Some(s) = change.slots[7] { fx.equip = s; }
    if let Some(s) = change.slots[8] { fx.unequip = s; }
    if let Some(s) = change.slots[9] { fx.mod_shoot_3d = s; }
    if let Some(s) = change.slots[10] { fx.mod_shoot_dist = s; }
    if let Some(s) = change.slots[11] { fx.mod_shoot_2d = s; }
}

/// Apply JIP `SetWeaponSound` slots `12..=13` (`YNAM` pickup / `ZNAM` putdown, shared with
/// `SetPickupSound` / `SetPutdownSound`) onto `(pickup, putdown)` [documented: GECK wiki
/// SetWeaponSound; design: docs/spec-notes/weapon-sound-bridge.md].
fn apply_weapon_item_change(pair: &mut (Option<FormId>, Option<FormId>), change: &fnv_game::nvse_gaps::WeaponSoundChange) {
    if let Some(s) = change.slots[12] { pair.0 = s; }
    if let Some(s) = change.slots[13] { pair.1 = s; }
}

/// Sync live `Game::weapon_sound_changes` (slots `0..=11`) and `TempCloneForm` `WEAP`
/// aliases from `game` into `weapons`. Returns `true` when the sync ran (stamp changed).
fn sync_script_weapon_fx(
    game: &fnv_game::Game,
    weapons: &mut HashMap<FormId, fnv_combat::fx::WeaponFx>,
    state: &mut WeaponFxSync,
) -> bool {
    let key = (game.weapon_sound_stamp(), weapon_clones_stamp(game));
    if state.stamp == Some(key) {
        return false;
    }

    // Drain previously materialized clone and synthetic entries before rebuilding so
    // disappeared clones and FormIDs reused across sessions never leak.
    for id in state.active_clones.drain() {
        weapons.remove(&id);
    }
    for id in state.inserted.drain() {
        weapons.remove(&id);
    }

    let changes = game.weapon_sound_changes();
    let mut overridden_authored: HashSet<FormId> = HashSet::new();
    for (&id, change) in &changes {
        if !game.clones().contains_key(&id) && change.slots[..12].iter().any(Option::is_some) {
            overridden_authored.insert(id);
        }
    }

    // Restore previously mutated authored weapons before re-applying active overrides.
    state.originals.retain(|id, orig| {
        if let Some(entry) = weapons.get_mut(id) {
            *entry = orig.clone();
        }
        overridden_authored.contains(id)
    });

    for &id in &overridden_authored {
        let Some(change) = changes.get(&id) else { continue };
        if let Some(entry) = weapons.get_mut(&id) {
            state.originals.entry(id).or_insert_with(|| entry.clone());
            apply_weapon_fx_change(entry, change);
        } else {
            let mut fx = fnv_combat::fx::WeaponFx::default();
            apply_weapon_fx_change(&mut fx, change);
            weapons.insert(id, fx);
            state.inserted.insert(id);
        }
    }

    // Materialize every TempCloneForm WEAP alias from its authored origin's base WeaponFx
    // plus its own (or snapshotted) weapon-sound overrides.
    for &clone_id in game.clones().keys() {
        let origin = game.unclone(clone_id);
        if game.clones().contains_key(&origin) || state.active_clones.contains(&origin) || state.inserted.contains(&origin) {
            continue;
        }
        let Some(mut cloned) = state.originals.get(&origin).or_else(|| weapons.get(&origin)).cloned() else {
            continue;
        };
        if let Some(change) = changes.get(&clone_id) {
            apply_weapon_fx_change(&mut cloned, change);
        }
        weapons.insert(clone_id, cloned);
        state.active_clones.insert(clone_id);
    }

    state.stamp = Some(key);
    true
}

/// Sync live `Game::weapon_sound_changes` (slots `12..=13`, `YNAM` pickup / `ZNAM` putdown)
/// and `TempCloneForm` `WEAP` aliases from `game` into `items`. Returns `true` when synced.
fn sync_script_weapon_items(
    game: &fnv_game::Game,
    items: &mut HashMap<FormId, (Option<FormId>, Option<FormId>)>,
    state: &mut WeaponItemSoundSync,
) -> bool {
    let key = (game.weapon_sound_stamp(), weapon_clones_stamp(game));
    if state.stamp == Some(key) {
        return false;
    }

    for id in state.active_clones.drain() {
        items.remove(&id);
    }
    for id in state.inserted.drain() {
        items.remove(&id);
    }

    let changes = game.weapon_sound_changes();
    let mut overridden_authored: HashSet<FormId> = HashSet::new();
    for (&id, change) in &changes {
        if !game.clones().contains_key(&id) && (change.slots[12].is_some() || change.slots[13].is_some()) {
            overridden_authored.insert(id);
        }
    }

    state.originals.retain(|id, orig| {
        if let Some(entry) = items.get_mut(id) {
            *entry = *orig;
        }
        overridden_authored.contains(id)
    });

    for &id in &overridden_authored {
        let Some(change) = changes.get(&id) else { continue };
        if let Some(entry) = items.get_mut(&id) {
            state.originals.entry(id).or_insert(*entry);
            apply_weapon_item_change(entry, change);
        } else {
            let mut pair = (None, None);
            apply_weapon_item_change(&mut pair, change);
            items.insert(id, pair);
            state.inserted.insert(id);
        }
    }

    // Materialize TempCloneForm WEAP entries into items (checking WEAP signature so non-weapon
    // clones stay untouched).
    for &clone_id in game.clones().keys() {
        let origin = game.unclone(clone_id);
        if game.clones().contains_key(&origin)
            || state.active_clones.contains(&origin)
            || state.inserted.contains(&origin)
            || game.data.sig(origin) != Some(*b"WEAP")
        {
            continue;
        }
        let Some(mut cloned) = state.originals.get(&origin).or_else(|| items.get(&origin)).copied() else {
            continue;
        };
        if let Some(change) = changes.get(&clone_id) {
            apply_weapon_item_change(&mut cloned, change);
        }
        items.insert(clone_id, cloned);
        state.active_clones.insert(clone_id);
    }

    state.stamp = Some(key);
    true
}

/// Sync live `SetWeaponSound` / `SetPickupSound` / `SetPutdownSound` overrides and
/// `TempCloneForm` `WEAP` aliases from `ScriptState` into `Sfx::forms::weapons` (slots
/// `0..=11`) and `WorldSounds::forms::items` (slots `12..=13`) [design:
/// docs/spec-notes/weapon-sound-bridge.md].
pub fn script_weapon_sounds(
    script: Option<Res<crate::script::ScriptState>>,
    mut sfx: Option<ResMut<Sfx>>,
    mut ws: Option<ResMut<WorldSounds>>,
    mut sync: ResMut<WeaponSoundSync>,
) {
    let Some(rt) = script.as_deref().and_then(|s| s.rt.as_ref()) else { return };
    if let Some(sfx) = sfx.as_deref_mut() {
        sync_script_weapon_fx(&rt.game, &mut sfx.forms.weapons, &mut sync.weapons);
    }
    if let Some(ws) = ws.as_deref_mut() {
        sync_script_weapon_items(&rt.game, &mut ws.forms.items, &mut sync.items);
    }
}

pub struct WorldSoundsPlugin;

impl Plugin for WorldSoundsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WeaponSoundSync>()
            .add_systems(Startup, init)
            // Before Activation so combat (weapon_sounds, fire, melee, actor_attacks) sees
            // overrides from console, load, or previous frame; after Scripts before Ui so
            // UI sounds, world pickup/drop, and NPC weapon idles see same-frame script edits.
            .add_systems(Update, script_weapon_sounds.before(PlaySet::Activation))
            .add_systems(Update, script_weapon_sounds.after(PlaySet::Scripts).before(PlaySet::Ui))
            .add_systems(Update, (knocks, objects, object_sources, water_sources, script_sound_files).in_set(PlaySet::Sync))
            .add_systems(Last, report);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn havok_materials_to_knock_sounds() {
        // A bottle (24) and glass (3) clink like bottles; platform and stairs codes are
        // their material's.
        assert_eq!(phy_sound(24, true), Some("PHYBottleH"));
        assert_eq!(phy_sound(3 + 32, false), Some("PHYBottleL"));
        assert_eq!(havok_name(24 + 64), "BOTTLE");
        assert_eq!(phy_sound(9, true), Some("CWoodMedium"));
        // Dirt, grass, water and sand make no knock.
        for c in [2, 4, 8, 18] {
            assert_eq!(phy_sound(c, true), None);
        }
    }

    #[test]
    fn own_sounds_one_kind_per_record() {
        let sigs: HashSet<&[u8; 4]> = OWN_SOUNDS.iter().map(|k| k.0).collect();
        assert_eq!(sigs.len(), OWN_SOUNDS.len());
        assert_eq!(OWN_SOUNDS[STATIC_KIND].0, b"STAT");
    }

    #[test]
    fn knocks_are_speed_lost() {
        // Falling faster, or pushed from rest: no knock; at rest: none.
        assert_eq!(knock_speed([10.0, 0.0, -100.0], [10.0, 0.0, -111.0]), 0.0);
        assert_eq!(knock_speed([0.0; 3], [0.0, 210.0, 0.0]), 0.0);
        assert_eq!(knock_speed([0.0; 3], [0.0; 3]), 0.0);
        // Landing from 300 units/s: a hard knock; bouncing straight back: twice the speed.
        assert!((knock_speed([0.0, 0.0, -300.0], [0.0; 3]) - 300.0).abs() < 1e-3);
        assert!((knock_speed([0.0, 0.0, -100.0], [0.0, 0.0, 100.0]) - 200.0).abs() < 1e-3);
    }

    const CARTON: u32 = 0x100;
    const UP: FormId = FormId(0x900);
    const DOWN: FormId = FormId(0x901);

    /// The world's sounds over two item references picked up (0x301, 0x302) and two
    /// dropped (returned).
    fn objects_app() -> (App, u32, u32) {
        use crate::sim::test_world::{item_base, placed, sim};
        let mut world = sim(vec![item_base(CARTON)], vec![placed(0x301, CARTON), placed(0x302, CARTON)]);
        for id in [0x301, 0x302] {
            world.world.activate(FormId::PLAYER_REF, FormId(id)).unwrap();
        }
        let mut drop = || world.world.place(FormId(CARTON), None, [0.0; 3], [0.0; 3], 1).unwrap().0;
        let (a, b) = (drop(), drop());
        let mut ws = WorldSounds::default();
        ws.forms.items.insert(FormId(CARTON), (Some(UP), Some(DOWN)));
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<PlayState>()
            .insert_resource(world)
            .insert_resource(ws)
            .init_resource::<crate::sfx::ScriptSounds>()
            .add_message::<RefChanged>()
            .add_message::<Activated>()
            .add_message::<DoorPassed>()
            .add_message::<ImpactPicked>()
            .add_message::<crate::ui_sounds::UiSound>()
            .add_message::<PlaySound>()
            .add_message::<ActorSound>()
            .add_systems(Update, objects);
        (app, a, b)
    }

    /// Tell these changes and run a frame: the sounds asked for.
    fn hear(app: &mut App, changes: &[RefChanged]) -> Vec<FormId> {
        let mut m = app.world_mut().resource_mut::<bevy::ecs::message::Messages<RefChanged>>();
        for &c in changes {
            m.write(c);
        }
        app.update();
        app.world_mut().resource_mut::<bevy::ecs::message::Messages<PlaySound>>().drain().map(|s| s.sound).collect()
    }

    #[test]
    fn what_the_load_restores_is_silent_and_a_real_change_beside_it_is_heard() {
        let (mut app, dropped_before, dropped_now) = objects_app();
        let restored = RefChanged::restored;
        // Picked up before the save, and picked up now, in the same frame.
        assert_eq!(hear(&mut app, &[restored(0x301), RefChanged::new(0x302)]), vec![UP]);
        // Dropped before the save, and dropped now.
        assert_eq!(hear(&mut app, &[restored(dropped_before), RefChanged::new(dropped_now)]), vec![DOWN]);
        let stats = &app.world().resource::<WorldSounds>().stats;
        assert_eq!((stats.pickups, stats.drops), (1, 1));
        // Both count as heard: a later notice of them makes no sound.
        assert!(hear(&mut app, &[RefChanged::new(0x301), RefChanged::new(dropped_before)]).is_empty());
    }

    /// The index's answer is the walk over the whole list's ([`scan_all`]): sources across
    /// cell borders, a reach that ends on a cell border, a negative reach, a sound not in
    /// the load order, initially disabled and disabled references, statics whose state
    /// moves (they stay at their placement), others moved by their state into reach,
    /// moved again with nothing else changing, one state dropped (a cell reset) and
    /// another made in the same frame (the number of states unchanged), a restored world
    /// with as many states as the last, and their states gone with a new world.
    #[test]
    fn indexed_object_sources_are_the_whole_lists() {
        use crate::sim::test_world::{item_base, placed, sim};
        use crate::sound_grid::tests::Lcg;
        use fnv_sim::forms::{BaseData, BaseForm, Container};
        use fnv_sim::refs::FLAG_INITIALLY_DISABLED;
        fn reach(s: FormId) -> Option<f32> {
            match s.0 - 0x900 {
                0 => Some(4096.0),
                1 => Some(300.0),
                2 => Some(2200.0),
                3 => Some(6000.0),
                4 => Some(-500.0),
                _ => None,
            }
        }
        fn bits(v: &[(u32, [f32; 3])]) -> Vec<(u32, [u32; 3])> {
            v.iter().map(|&(i, at)| (i, at.map(f32::to_bits))).collect()
        }
        /// Every listener: the index's answer and the whole list's; how many heard.
        fn check(index: &mut SourceIndex, list: &[ObjectSource], sim: &SimState, eyes: &[[f32; 3]]) -> usize {
            let (mut near, mut out) = (Vec::new(), Vec::new());
            let mut heard = 0;
            for &eye in eyes {
                index.heard(list, sim, eye, &mut near, &mut out);
                assert_eq!(bits(&out), bits(&scan_all(list, sim, eye, reach)), "listener at {eye:?}");
                if eye[0].is_finite() {
                    assert!(near.len() < list.len() / 4, "the index narrows the candidates at {eye:?}");
                }
                heard += out.len();
            }
            heard
        }
        const BASE: u32 = 0x100;
        const FIRST: u32 = 0x1000;
        let mut rng = Lcg(11);
        let mut refs = Vec::new();
        let mut list: Vec<ObjectSource> = Vec::new();
        for i in 0..900u32 {
            let mut pos = [rng.range(-30000.0, 30000.0), rng.range(-30000.0, 30000.0), rng.range(-500.0, 2500.0)];
            if i % 5 == 0 {
                pos[0] = 4096.0 * (rng.next_u32() % 13) as f32 - 24576.0;
            }
            if i % 7 == 0 {
                pos[1] = -4096.0 * (rng.next_u32() % 6) as f32;
            }
            let mut p = placed(FIRST + i, BASE);
            p.pos = pos;
            if i % 13 == 0 {
                p.flags |= FLAG_INITIALLY_DISABLED;
            }
            refs.push(p);
            let kind = if i % 3 == 0 { STATIC_KIND } else { 0 };
            list.push((FIRST + i, FormId(0x900 + i % 6), kind, (kind == STATIC_KIND).then_some(pos)));
        }
        // A respawning container in a cell, beside the listener at the origin.
        const CRATE_BASE: u32 = 0x101;
        const CELL: FormId = FormId(0x200);
        let crate_ref = FormId(FIRST + 900);
        let crate_base = BaseForm {
            id: FormId(CRATE_BASE),
            sig: *b"CONT",
            edid: None,
            name: None,
            script: None,
            prompt: None,
            voice_filter: false,
            data: BaseData::Container(Container { respawns: true, entries: vec![] }),
        };
        let mut p = placed(crate_ref.0, CRATE_BASE);
        (p.cell, p.pos) = (Some(CELL), [100.0, 0.0, 0.0]);
        refs.push(p);
        list.push((crate_ref.0, FormId(0x900), 0, None));
        let mut sim = sim(vec![item_base(BASE), crate_base], refs);
        let mut index = SourceIndex::build(&list, &sim, reach, 6);
        let mut eyes: Vec<[f32; 3]> = (0..120).map(|_| [rng.range(-32000.0, 32000.0), rng.range(-32000.0, 32000.0), rng.range(0.0, 2000.0)]).collect();
        eyes.extend([[0.0; 3], [4096.0, -4096.0, 0.0], [f32::NAN, 0.0, 0.0]]);
        // On the edge of a source's reach.
        for &(_, sound, _, fixed) in list.iter().filter(|e| e.3.is_some()).take(30) {
            let (at, r) = (fixed.unwrap(), reach(sound).unwrap_or(0.0).abs());
            eyes.push([at[0] + r, at[1], at[2]]);
        }
        assert!(check(&mut index, &list, &sim, &eyes) > 0, "some sources are heard");

        // Disabled now, enabled now; a static's state moved (it stays at its placement);
        // others moved by their state, one beside a listener.
        let at = |i: u32| FormId(FIRST + i);
        let eye = eyes[0];
        sim.world.set_enabled(at(1), false).unwrap();
        sim.world.set_enabled(at(13), true).unwrap();
        sim.world.get(at(3)).unwrap().pos = eye;
        sim.world.get(at(4)).unwrap().pos = [eye[0] + 100.0, eye[1], eye[2]];
        sim.world.get(at(8)).unwrap().pos = [1e7, 1e7, 0.0];
        let (mut near, mut out) = (Vec::new(), Vec::new());
        let mine = |out: &[(u32, [f32; 3])]| out.iter().any(|&(i, _)| i == 4);
        assert!(check(&mut index, &list, &sim, &eyes) > 0);
        index.heard(&list, &sim, eye, &mut near, &mut out);
        assert!(mine(&out), "a source moved beside the listener is heard there");
        // Moved again, nothing else changing (no new state, no serial).
        sim.world.get(at(4)).unwrap().pos = [eye[0] + 1e6, eye[1], eye[2]];
        check(&mut index, &list, &sim, &eyes);
        index.heard(&list, &sim, eye, &mut near, &mut out);
        assert!(!mine(&out), "a source moved away is no longer heard");
        sim.world.get(at(4)).unwrap().pos = [eye[0], eye[1] + 50.0, eye[2]];
        check(&mut index, &list, &sim, &eyes);
        let heard = |out: &[(u32, [f32; 3])], id: FormId| out.iter().any(|&(i, _)| list[i as usize].0 == id.0);
        // The container gets a state where it stands, and the index sees it.
        sim.world.get(crate_ref).unwrap();
        check(&mut index, &list, &sim, &eyes);
        index.heard(&list, &sim, [0.0; 3], &mut near, &mut out);
        assert!(heard(&out, crate_ref), "the container is heard beside its listener");
        // Between two frames, one state dropped and another made, the number of states the
        // same: the container's state goes with its cell's reset, and a source with no
        // state so far gets one and is moved beside a listener. A state made as placed
        // leaves the presence serial, so only the reset's drop tells the index that the
        // states changed.
        let fresh = at(2);
        assert!(sim.world.changed().all(|(id, _)| id != fresh), "a source with no state so far");
        let states = sim.world.changed().count();
        assert_eq!(sim.world.reset_containers(CELL), vec![crate_ref]);
        sim.world.get(fresh).unwrap().pos = [eye[0], eye[1] + 100.0, eye[2]];
        assert_eq!(sim.world.changed().count(), states, "as many states as before");
        check(&mut index, &list, &sim, &eyes);
        index.heard(&list, &sim, eye, &mut near, &mut out);
        assert!(heard(&out, fresh), "the source with a new state is heard beside the listener");
        // A restored world with as many states as the last (it starts from a serial no
        // other world used): other sources have them, one beside the listener.
        let before: Vec<FormId> = sim.world.changed().map(|(id, _)| id).collect();
        sim.world = fnv_sim::World::new(sim.forms, sim.refs, fnv_sim::SimConfig::default());
        let fresh: Vec<FormId> = list
            .iter()
            .filter(|e| e.3.is_none() && reach(e.1).is_some_and(|r| r > 0.0) && !before.contains(&FormId(e.0)) && sim.world.is_enabled(FormId(e.0)).unwrap_or(false))
            .map(|e| FormId(e.0))
            .collect();
        sim.world.get(fresh[0]).unwrap().pos = [eye[0] - 100.0, eye[1], eye[2]];
        for &id in &fresh[1..] {
            if sim.world.changed().count() >= before.len() {
                break;
            }
            sim.world.get(id).unwrap();
        }
        assert_eq!(sim.world.changed().count(), before.len(), "as many states as the last world");
        check(&mut index, &list, &sim, &eyes);
        index.heard(&list, &sim, eye, &mut near, &mut out);
        assert!(heard(&out, fresh[0]), "a source the restored world moved is heard beside the listener");
        // A new world: every state gone, every source at its placement again.
        sim.world = fnv_sim::World::new(sim.forms, sim.refs, fnv_sim::SimConfig::default());
        check(&mut index, &list, &sim, &eyes);
    }

    fn test_sound_game() -> (fnv_game::GameData, fnv_sim::FormStore, fnv_sim::RefTable) {
        fn sub(sig: &[u8; 4], data: &[u8]) -> Vec<u8> {
            let mut out = Vec::with_capacity(6 + data.len());
            out.extend_from_slice(sig);
            out.extend_from_slice(&(data.len() as u16).to_le_bytes());
            out.extend_from_slice(data);
            out
        }
        fn rec(sig: &[u8; 4], id: u32, subs: &[Vec<u8>]) -> Vec<u8> {
            let body: Vec<u8> = subs.concat();
            let mut out = Vec::with_capacity(24 + body.len());
            out.extend_from_slice(sig);
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&id.to_le_bytes());
            out.extend_from_slice(&[0u8; 8]);
            out.extend_from_slice(&body);
            out
        }
        fn top(sig: &[u8; 4], body: &[u8]) -> Vec<u8> {
            let mut out = Vec::with_capacity(24 + body.len());
            out.extend_from_slice(b"GRUP");
            out.extend_from_slice(&((24 + body.len()) as u32).to_le_bytes());
            out.extend_from_slice(sig);
            out.extend_from_slice(&[0u8; 12]);
            out.extend_from_slice(body);
            out
        }
        let mut bytes = rec(b"TES4", 0, &[sub(b"HEDR", &[0, 0, 0x80, 0x3F, 0, 0, 0, 0, 0, 0, 0, 0])]);
        bytes.extend(top(
            b"SOUN",
            &rec(b"SOUN", 0x0000_0D60, &[sub(b"EDID", b"OwnedMenuFocus\0"), sub(b"FNAM", b"fx\\ui\\owned_focus.wav\0")]),
        ));
        bytes.extend(top(b"STAT", &rec(b"STAT", 0x0000_0D61, &[sub(b"EDID", b"OwnedStatic\0")])));
        let plugins = vec![fnv_esm::Plugin::parse(&bytes, "Owned.esm".into()).unwrap()];
        let lo = LoadOrder::new(&plugins).unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
        let schema = fnv_esm::Schema::load_dir(root.join("fnv")).unwrap();
        let functions = root.join("script/functions.json");
        let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&functions).unwrap()).unwrap();
        let forms = fnv_sim::FormStore::build(&lo, &schema);
        let refs = fnv_sim::RefTable::build(&lo, &schema);
        let data = fnv_game::GameData::build(&lo, &schema, fnv_script::FunctionTable::load(&functions).unwrap(), &json, &refs);
        (data, forms, refs)
    }

    #[test]
    fn sync_script_sounds_admits_cloned_sounds_applies_traits_and_resets_across_sessions() {
        use fnv_combat::fx::Sound;
        use fnv_script::vm::XVal;

        let (data, forms, refs) = test_sound_game();
        let mut game = fnv_game::Game::new(&data, fnv_sim::World::new(&forms, &refs, Default::default()), Default::default());
        let orig = FormId(0x0000_0D60);
        let non_sound = FormId(0x0000_0D61);

        let mut sounds: HashMap<FormId, Sound> = HashMap::from([(
            orig,
            Sound {
                edid: Some("OwnedMenuFocus".into()),
                file: Some("fx\\ui\\owned_focus.wav".into()),
                min_distance: 100.0,
                max_distance: 1500.0,
                static_attenuation_db: 6.0,
                frequency_adjust_percent: 0.0,
                ..Sound::default()
            },
        )]);
        let mut sync = ScriptSoundSync::default();

        // Create an unmodified SOUN clone BEFORE mutating orig, plus a non-SOUN clone.
        let XVal::Ref(c_unmod_raw) = fnv_game::nvse::call(&mut game, &Default::default(), "tempcloneform", None, &[XVal::Ref(orig.0), XVal::Num(1.0)]).unwrap().unwrap() else {
            panic!("expected unmodified sound clone");
        };
        let c_unmod = FormId(c_unmod_raw);
        let XVal::Ref(bad_raw) = fnv_game::nvse::call(&mut game, &Default::default(), "tempcloneform", None, &[XVal::Ref(non_sound.0), XVal::Num(1.0)]).unwrap().unwrap() else {
            panic!("expected non-sound clone");
        };
        let bad = FormId(bad_raw);

        // Silence/redirect orig AFTER c_unmod exists (B42 Inject silence-original pattern):
        // c_unmod must keep orig's authored file and attenuation, while c1 cloned AFTER
        // this edit snapshots orig's active overrides.
        fnv_game::nvse::call(&mut game, &Default::default(), "setsoundsourcefile", None, &[XVal::Ref(orig.0), XVal::Str("fx\\owned\\silence.wav".into())]).unwrap().unwrap();
        fnv_game::nvse::call(&mut game, &Default::default(), "setsoundtraitnumeric", None, &[XVal::Ref(orig.0), XVal::Num(3.0), XVal::Num(-99.0)]).unwrap().unwrap();

        let XVal::Ref(c1_raw) = fnv_game::nvse::call(&mut game, &Default::default(), "tempcloneform", None, &[XVal::Ref(orig.0), XVal::Num(1.0)]).unwrap().unwrap() else {
            panic!("expected sound clone 1");
        };
        let c1 = FormId(c1_raw);

        assert!(sync_script_sounds(&game, &mut sounds, &mut sync));
        // Second call with unchanged state is a zero-work no-op.
        assert!(!sync_script_sounds(&game, &mut sounds, &mut sync));

        // Non-SOUN clone is never admitted into sounds.
        assert!(!sounds.contains_key(&bad));

        // c_unmod was created before orig was silenced, so it keeps orig's authored Sound.
        let su = sounds.get(&c_unmod).expect("unmodified clone materialized");
        assert_eq!(su.edid, None);
        assert_eq!(su.file.as_deref(), Some("fx\\ui\\owned_focus.wav"));
        assert_eq!(su.static_attenuation_db, 6.0);

        // c1 was created after orig was mutated, so it snapshotted orig's session overrides.
        let s1_initial = sounds.get(&c1).expect("clone 1 materialized");
        assert_eq!(s1_initial.edid, None);
        assert_eq!(s1_initial.file.as_deref(), Some("fx\\owned\\silence.wav"));
        assert_eq!(s1_initial.static_attenuation_db, 99.0);

        // Apply SetSoundSourceFile and SetSoundTraitNumeric to clone 1, then clone c2 from c1,
        // then mutate c1 again; c2 must stay isolated at the snapshot taken from c1.
        fnv_game::nvse::call(&mut game, &Default::default(), "setsoundsourcefile", None, &[XVal::Ref(c1.0), XVal::Str("fx\\owned\\scope_click.wav".into())]).unwrap().unwrap();
        fnv_game::nvse::call(&mut game, &Default::default(), "setsoundtraitnumeric", None, &[XVal::Ref(c1.0), XVal::Num(0.0), XVal::Num(250.0)]).unwrap().unwrap();
        fnv_game::nvse::call(&mut game, &Default::default(), "setsoundtraitnumeric", None, &[XVal::Ref(c1.0), XVal::Num(1.0), XVal::Num(3200.0)]).unwrap().unwrap();
        fnv_game::nvse::call(&mut game, &Default::default(), "setsoundtraitnumeric", None, &[XVal::Ref(c1.0), XVal::Num(2.0), XVal::Num(12.0)]).unwrap().unwrap();
        fnv_game::nvse::call(&mut game, &Default::default(), "setsoundtraitnumeric", None, &[XVal::Ref(c1.0), XVal::Num(3.0), XVal::Num(-25.0)]).unwrap().unwrap();
        fnv_game::nvse::call(&mut game, &Default::default(), "setsoundtraitnumeric", None, &[XVal::Ref(c1.0), XVal::Num(6.0), XVal::Num(80.0)]).unwrap().unwrap();
        fnv_game::nvse::call(&mut game, &Default::default(), "setsoundtraitnumeric", None, &[XVal::Ref(c1.0), XVal::Num(11.0), XVal::Num(55.0)]).unwrap().unwrap();

        let XVal::Ref(c2_raw) = fnv_game::nvse::call(&mut game, &Default::default(), "tempcloneform", None, &[XVal::Ref(c1.0), XVal::Num(1.0)]).unwrap().unwrap() else {
            panic!("expected sound clone 2");
        };
        let c2 = FormId(c2_raw);

        // Post-clone edits on c1 do not leak into c2; empty SetSoundSourceFile clears file to None.
        fnv_game::nvse::call(&mut game, &Default::default(), "setsoundsourcefile", None, &[XVal::Ref(c1.0), XVal::Str("fx\\owned\\later_c1.wav".into())]).unwrap().unwrap();
        fnv_game::nvse::call(&mut game, &Default::default(), "setsoundtraitnumeric", None, &[XVal::Ref(c1.0), XVal::Num(3.0), XVal::Num(-40.0)]).unwrap().unwrap();
        fnv_game::nvse::call(&mut game, &Default::default(), "setsoundsourcefile", None, &[XVal::Ref(c_unmod.0), XVal::Str(String::new())]).unwrap().unwrap();

        assert!(sync_script_sounds(&game, &mut sounds, &mut sync));
        assert!(!sync_script_sounds(&game, &mut sounds, &mut sync));

        let su_empty = sounds.get(&c_unmod).expect("c_unmod present");
        assert_eq!(su_empty.file, None);

        let s1 = sounds.get(&c1).expect("clone 1 present");
        assert_eq!(s1.edid, None);
        assert_eq!(s1.file.as_deref(), Some("fx\\owned\\later_c1.wav"));
        assert_eq!((s1.min_distance, s1.max_distance, s1.frequency_adjust_percent, s1.static_attenuation_db), (250.0, 3200.0, 12.0, 40.0));
        assert_eq!(s1.attenuation_curve.unwrap()[0], 80.0);
        assert_eq!(s1.reverb_attenuation_percent, 55.0);

        let s2 = sounds.get(&c2).expect("clone 2 present");
        assert_eq!(s2.edid, None);
        assert_eq!(s2.file.as_deref(), Some("fx\\owned\\scope_click.wav"));
        assert_eq!(s2.static_attenuation_db, 25.0);

        // Original received its own overrides while keeping its authored EDID.
        let so = sounds.get(&orig).expect("original present");
        assert_eq!(so.edid.as_deref(), Some("OwnedMenuFocus"));
        assert_eq!(so.file.as_deref(), Some("fx\\owned\\silence.wav"));
        assert_eq!(so.static_attenuation_db, 99.0);

        // Fresh session (new game / load): cloned SOUNs are pruned and authored original is restored.
        let fresh = fnv_game::Game::new(&data, fnv_sim::World::new(&forms, &refs, Default::default()), Default::default());
        assert!(sync_script_sounds(&fresh, &mut sounds, &mut sync));
        assert!(!sounds.contains_key(&c_unmod));
        assert!(!sounds.contains_key(&c1));
        assert!(!sounds.contains_key(&c2));
        let restored = sounds.get(&orig).expect("original restored");
        assert_eq!(restored.file.as_deref(), Some("fx\\ui\\owned_focus.wav"));
        assert_eq!(restored.static_attenuation_db, 6.0);
    }

    #[test]
    fn sync_script_sounds_prunes_reused_clone_form_id_when_new_session_clones_non_sound() {
        use fnv_combat::fx::Sound;
        use fnv_script::vm::XVal;

        let (data, forms, refs) = test_sound_game();
        let orig = FormId(0x0000_0D60);
        let non_sound = FormId(0x0000_0D61);

        let mut sounds: HashMap<FormId, Sound> = HashMap::from([(
            orig,
            Sound {
                edid: Some("OwnedMenuFocus".into()),
                file: Some("fx\\ui\\owned_focus.wav".into()),
                ..Sound::default()
            },
        )]);
        let mut sync = ScriptSoundSync::default();

        // Session 1: first local clone is a SOUN at deterministic FormId 0x00FE_0000.
        let mut s1 = fnv_game::Game::new(&data, fnv_sim::World::new(&forms, &refs, Default::default()), Default::default());
        let XVal::Ref(c1_raw) = fnv_game::nvse::call(&mut s1, &Default::default(), "tempcloneform", None, &[XVal::Ref(orig.0), XVal::Num(1.0)]).unwrap().unwrap() else {
            panic!("expected session 1 SOUN clone");
        };
        let c1 = FormId(c1_raw);
        assert!(sync_script_sounds(&s1, &mut sounds, &mut sync));
        assert!(sounds.contains_key(&c1));
        assert!(sync.active_clones.contains(&c1));

        // Session 2: before sync runs, a non-SOUN (STAT) clone reuses the exact same FormId.
        let mut s2 = fnv_game::Game::new(&data, fnv_sim::World::new(&forms, &refs, Default::default()), Default::default());
        let XVal::Ref(reused_raw) = fnv_game::nvse::call(&mut s2, &Default::default(), "tempcloneform", None, &[XVal::Ref(non_sound.0), XVal::Num(1.0)]).unwrap().unwrap() else {
            panic!("expected session 2 STAT clone");
        };
        let reused_id = FormId(reused_raw);
        assert_eq!(reused_id, c1, "fresh session deterministically reuses first local clone FormId");

        assert!(sync_script_sounds(&s2, &mut sounds, &mut sync));
        assert!(!sounds.contains_key(&reused_id), "stale Session 1 SOUN must be removed when FormId is reused by a non-SOUN");
        assert!(!sync.active_clones.contains(&reused_id), "active_clones must prune reused non-SOUN FormId");
    }

    fn test_weapon_sound_game() -> (fnv_game::GameData, fnv_sim::FormStore, fnv_sim::RefTable, Vec<fnv_esm::Plugin>) {
        fn sub(sig: &[u8; 4], data: &[u8]) -> Vec<u8> {
            let mut out = Vec::with_capacity(6 + data.len());
            out.extend_from_slice(sig);
            out.extend_from_slice(&(data.len() as u16).to_le_bytes());
            out.extend_from_slice(data);
            out
        }
        fn rec(sig: &[u8; 4], id: u32, subs: &[Vec<u8>]) -> Vec<u8> {
            let body: Vec<u8> = subs.concat();
            let mut out = Vec::with_capacity(24 + body.len());
            out.extend_from_slice(sig);
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&id.to_le_bytes());
            out.extend_from_slice(&[0u8; 8]);
            out.extend_from_slice(&body);
            out
        }
        fn top(sig: &[u8; 4], body: &[u8]) -> Vec<u8> {
            let mut out = Vec::with_capacity(24 + body.len());
            out.extend_from_slice(b"GRUP");
            out.extend_from_slice(&((24 + body.len()) as u32).to_le_bytes());
            out.extend_from_slice(sig);
            out.extend_from_slice(&[0u8; 12]);
            out.extend_from_slice(body);
            out
        }
        let mut sounds = Vec::new();
        for (id, edid) in [
            (0x0000_0E00u32, "AuthS0"),
            (0x0000_0E01, "AuthS1"),
            (0x0000_0E02, "AuthS2"),
            (0x0000_0E03, "AuthS3"),
            (0x0000_0E04, "AuthS4"),
            (0x0000_0E05, "AuthS5"),
            (0x0000_0E06, "AuthS6"),
            (0x0000_0E07, "AuthS7"),
            (0x0000_0E08, "AuthS8"),
            (0x0000_0E09, "AuthS9"),
            (0x0000_0E0A, "AuthS10"),
            (0x0000_0E0B, "AuthS11"),
            (0x0000_0E0C, "AuthUp"),
            (0x0000_0E0D, "AuthDown"),
            (0x0000_0E20, "NewS0"),
            (0x0000_0E21, "NewS1"),
            (0x0000_0E22, "NewS2"),
            (0x0000_0E23, "NewS3"),
            (0x0000_0E24, "NewS4"),
            (0x0000_0E25, "NewS5"),
            (0x0000_0E26, "NewS6"),
            (0x0000_0E27, "NewS7"),
            (0x0000_0E28, "NewS8"),
            (0x0000_0E29, "NewS9"),
            (0x0000_0E2A, "NewS10"),
            (0x0000_0E2B, "NewS11"),
            (0x0000_0E2C, "NewUp"),
            (0x0000_0E2D, "NewDown"),
            (0x0000_0E2E, "UIItemGenericUp"),
            (0x0000_0E2F, "UIItemGenericDown"),
        ] {
            let mut z = edid.as_bytes().to_vec();
            z.push(0);
            sounds.extend(rec(b"SOUN", id, &[sub(b"EDID", &z), sub(b"FNAM", b"fx\\owned\\w.wav\0")]));
        }
        let weap = rec(
            b"WEAP",
            0x0000_0E50,
            &[
                sub(b"EDID", b"OwnedWeap\0"),
                sub(b"SNAM", &0x0000_0E00u32.to_le_bytes()),
                sub(b"SNAM", &0x0000_0E01u32.to_le_bytes()),
                sub(b"XNAM", &0x0000_0E02u32.to_le_bytes()),
                sub(b"NAM7", &0x0000_0E03u32.to_le_bytes()),
                sub(b"TNAM", &0x0000_0E04u32.to_le_bytes()),
                sub(b"NAM6", &0x0000_0E05u32.to_le_bytes()),
                sub(b"UNAM", &0x0000_0E06u32.to_le_bytes()),
                sub(b"NAM9", &0x0000_0E07u32.to_le_bytes()),
                sub(b"NAM8", &0x0000_0E08u32.to_le_bytes()),
                sub(b"WMS1", &0x0000_0E09u32.to_le_bytes()),
                sub(b"WMS1", &0x0000_0E0Au32.to_le_bytes()),
                sub(b"WMS2", &0x0000_0E0Bu32.to_le_bytes()),
                sub(b"YNAM", &0x0000_0E0Cu32.to_le_bytes()),
                sub(b"ZNAM", &0x0000_0E0Du32.to_le_bytes()),
            ],
        );
        let stat = rec(b"STAT", 0x0000_0E51, &[sub(b"EDID", b"OwnedNonWeap\0")]);
        let mut bytes = rec(b"TES4", 0, &[sub(b"HEDR", &[0, 0, 0x80, 0x3F, 0, 0, 0, 0, 0, 0, 0, 0])]);
        bytes.extend(top(b"SOUN", &sounds));
        bytes.extend(top(b"WEAP", &weap));
        bytes.extend(top(b"STAT", &stat));
        let plugins = vec![fnv_esm::Plugin::parse(&bytes, "OwnedWeapSounds.esm".into()).unwrap()];
        let lo = LoadOrder::new(&plugins).unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
        let schema = fnv_esm::Schema::load_dir(root.join("fnv")).unwrap();
        let functions = root.join("script/functions.json");
        let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&functions).unwrap()).unwrap();
        let forms = fnv_sim::FormStore::build(&lo, &schema);
        let refs = fnv_sim::RefTable::build(&lo, &schema);
        let data = fnv_game::GameData::build(&lo, &schema, fnv_script::FunctionTable::load(&functions).unwrap(), &json, &refs);
        (data, forms, refs, plugins)
    }

    #[test]
    fn sync_script_weapon_sounds_covers_all_slots_clones_clears_and_session_reset() {
        use fnv_combat::fx::{Sound, WeaponFx};
        use fnv_game::game::RunCtx;
        use fnv_script::vm::XVal;

        let (data, forms, refs, _) = test_weapon_sound_game();
        let mut game = fnv_game::Game::new(&data, fnv_sim::World::new(&forms, &refs, Default::default()), Default::default());
        let weap = FormId(0x0000_0E50);
        let non_weap = FormId(0x0000_0E51);
        let generic_up = FormId(0x0000_0E2E);
        let generic_down = FormId(0x0000_0E2F);

        let authored_fx = WeaponFx {
            shoot_3d: Some(FormId(0x0E00)),
            shoot_dist: Some(FormId(0x0E01)),
            shoot_2d: Some(FormId(0x0E02)),
            shoot_loop: Some(FormId(0x0E03)),
            no_ammo: Some(FormId(0x0E04)),
            block: Some(FormId(0x0E05)),
            idle: Some(FormId(0x0E06)),
            equip: Some(FormId(0x0E07)),
            unequip: Some(FormId(0x0E08)),
            mod_shoot_3d: Some(FormId(0x0E09)),
            mod_shoot_dist: Some(FormId(0x0E0A)),
            mod_shoot_2d: Some(FormId(0x0E0B)),
            shell_casing_model: Some("weapons\\casing.nif".into()),
            impact_dataset: Some(FormId(0x0F00)),
        };

        let mut fx_forms = fnv_combat::fx::FxForms::default();
        fx_forms.weapons.insert(weap, authored_fx.clone());
        fx_forms.sounds.insert(generic_up, Sound { edid: Some("UIItemGenericUp".into()), ..Sound::default() });
        fx_forms.sounds.insert(generic_down, Sound { edid: Some("UIItemGenericDown".into()), ..Sound::default() });
        let mut sfx = Sfx::new(fx_forms);
        let mut ws = WorldSounds::default();
        ws.forms.items.insert(weap, (Some(FormId(0x0E0C)), Some(FormId(0x0E0D))));
        let mut sync = WeaponSoundSync::default();

        // Clone BEFORE mutating `weap` + clone a non-WEAP (STAT).
        let XVal::Ref(c_before_raw) = fnv_game::nvse::call(&mut game, &RunCtx::default(), "tempcloneform", None, &[XVal::Ref(weap.0), XVal::Num(1.0)]).unwrap().unwrap() else {
            panic!("expected weapon clone");
        };
        let c_before = FormId(c_before_raw);
        let XVal::Ref(stat_clone_raw) = fnv_game::nvse::call(&mut game, &RunCtx::default(), "tempcloneform", None, &[XVal::Ref(non_weap.0), XVal::Num(1.0)]).unwrap().unwrap() else {
            panic!("expected stat clone");
        };
        let stat_clone = FormId(stat_clone_raw);

        // Compiled NVSE script overrides all 14 slots on `OwnedWeap` (0..=11 via SetWeaponSound,
        // 12 via SetPickupSound, 13 via SetPutdownSound).
        game.run_query(
            "Begin Function {}\n\
             SetWeaponSound OwnedWeap 0 NewS0\n\
             SetWeaponSound OwnedWeap 1 NewS1\n\
             SetWeaponSound OwnedWeap 2 NewS2\n\
             SetWeaponSound OwnedWeap 3 NewS3\n\
             SetWeaponSound OwnedWeap 4 NewS4\n\
             SetWeaponSound OwnedWeap 5 NewS5\n\
             SetWeaponSound OwnedWeap 6 NewS6\n\
             SetWeaponSound OwnedWeap 7 NewS7\n\
             SetWeaponSound OwnedWeap 8 NewS8\n\
             SetWeaponSound OwnedWeap 9 NewS9\n\
             SetWeaponSound OwnedWeap 10 NewS10\n\
             SetWeaponSound OwnedWeap 11 NewS11\n\
             SetPickupSound OwnedWeap NewUp\n\
             SetPutdownSound OwnedWeap NewDown\n\
             End",
            RunCtx::default(),
        )
        .unwrap();

        // Clone AFTER mutating `weap` snapshots the active overrides onto `c_after`.
        let XVal::Ref(c_after_raw) = fnv_game::nvse::call(&mut game, &RunCtx::default(), "tempcloneform", None, &[XVal::Ref(weap.0), XVal::Num(1.0)]).unwrap().unwrap() else {
            panic!("expected post-edit weapon clone");
        };
        let c_after = FormId(c_after_raw);

        assert!(sync_script_weapon_fx(&game, &mut sfx.forms.weapons, &mut sync.weapons));
        assert!(!sync_script_weapon_fx(&game, &mut sfx.forms.weapons, &mut sync.weapons), "unchanged sync is a no-op");
        assert!(sync_script_weapon_items(&game, &mut ws.forms.items, &mut sync.items));
        assert!(!sync_script_weapon_items(&game, &mut ws.forms.items, &mut sync.items), "unchanged item sync is a no-op");

        // Non-WEAP clone never enters weapons or items.
        assert!(!sfx.forms.weapons.contains_key(&stat_clone));
        assert!(!ws.forms.items.contains_key(&stat_clone));

        // Original received all 12 WeaponFx overrides while keeping casing model and impact dataset.
        let fx = &sfx.forms.weapons[&weap];
        assert_eq!(fx.shoot_3d, Some(FormId(0x0E20)));
        assert_eq!(fx.shoot_dist, Some(FormId(0x0E21)));
        assert_eq!(fx.shoot_2d, Some(FormId(0x0E22)));
        assert_eq!(fx.shoot_loop, Some(FormId(0x0E23)));
        assert_eq!(fx.no_ammo, Some(FormId(0x0E24)));
        assert_eq!(fx.block, Some(FormId(0x0E25)));
        assert_eq!(fx.idle, Some(FormId(0x0E26)));
        assert_eq!(fx.equip, Some(FormId(0x0E27)));
        assert_eq!(fx.unequip, Some(FormId(0x0E28)));
        assert_eq!(fx.mod_shoot_3d, Some(FormId(0x0E29)));
        assert_eq!(fx.mod_shoot_dist, Some(FormId(0x0E2A)));
        assert_eq!(fx.mod_shoot_2d, Some(FormId(0x0E2B)));
        assert_eq!(fx.shell_casing_model.as_deref(), Some("weapons\\casing.nif"));
        assert_eq!(fx.impact_dataset, Some(FormId(0x0F00)));

        // Pickup/putdown via item_sound_of (used by world_sounds & ui_sounds) reflect slots 12/13.
        assert_eq!(ws.item_sound_of(Some(&sfx), weap, true), Some(FormId(0x0E2C)));
        assert_eq!(ws.item_sound_of(Some(&sfx), weap, false), Some(FormId(0x0E2D)));

        // Pre-edit clone keeps authored sounds; post-edit clone keeps snapshotted overrides.
        assert_eq!(sfx.forms.weapons[&c_before].shoot_2d, Some(FormId(0x0E02)));
        assert_eq!(ws.item_sound_of(Some(&sfx), c_before, true), Some(FormId(0x0E0C)));
        assert_eq!(sfx.forms.weapons[&c_after].shoot_2d, Some(FormId(0x0E22)));
        assert_eq!(ws.item_sound_of(Some(&sfx), c_after, true), Some(FormId(0x0E2C)));

        // Clearing slots 12 and 13 via SetWeaponSound falls back to UIItemGenericUp / UIItemGenericDown.
        game.run_query(
            "Begin Function {}\nSetWeaponSound OwnedWeap 12 0\nSetWeaponSound OwnedWeap 13 0\nEnd",
            RunCtx::default(),
        )
        .unwrap();
        assert!(sync_script_weapon_items(&game, &mut ws.forms.items, &mut sync.items));
        assert_eq!(ws.item_sound_of(Some(&sfx), weap, true), Some(generic_up));
        assert_eq!(ws.item_sound_of(Some(&sfx), weap, false), Some(generic_down));
        // Post-edit clone remains isolated from later clear on original.
        assert_eq!(ws.item_sound_of(Some(&sfx), c_after, true), Some(FormId(0x0E2C)));

        // Fresh session (new game / load): clones are pruned (even if FormID is reused by a STAT)
        // and authored WEAP sounds are restored in full.
        let mut fresh = fnv_game::Game::new(&data, fnv_sim::World::new(&forms, &refs, Default::default()), Default::default());
        let XVal::Ref(reused_raw) = fnv_game::nvse::call(&mut fresh, &RunCtx::default(), "tempcloneform", None, &[XVal::Ref(non_weap.0), XVal::Num(1.0)]).unwrap().unwrap() else {
            panic!("expected reused clone id");
        };
        assert_eq!(FormId(reused_raw), c_before, "fresh session deterministically reuses first clone FormId");
        assert!(sync_script_weapon_fx(&fresh, &mut sfx.forms.weapons, &mut sync.weapons));
        assert!(sync_script_weapon_items(&fresh, &mut ws.forms.items, &mut sync.items));
        assert!(!sfx.forms.weapons.contains_key(&c_before));
        assert!(!sfx.forms.weapons.contains_key(&c_after));
        assert!(!ws.forms.items.contains_key(&c_before));
        assert!(!ws.forms.items.contains_key(&c_after));
        assert_eq!(sfx.forms.weapons[&weap].shoot_2d, authored_fx.shoot_2d);
        assert_eq!(sfx.forms.weapons[&weap].equip, authored_fx.equip);
        assert_eq!(ws.item_sound_of(Some(&sfx), weap, true), Some(FormId(0x0E0C)));
        assert_eq!(ws.item_sound_of(Some(&sfx), weap, false), Some(FormId(0x0E0D)));
    }

    #[test]
    fn world_pickup_and_drop_play_set_weapon_sound_slots_12_and_13() {
        use crate::sim::test_world::{item_base, placed, sim};
        use fnv_game::game::RunCtx;

        let (_, _, _, plugins) = test_weapon_sound_game();
        let leaked = Box::leak(plugins.into_boxed_slice());
        let script = crate::script::ScriptState::over_plugins(leaked);
        let weap_u32 = 0x0000_0E50u32;
        let weap = FormId(weap_u32);
        let auth_up = FormId(0x0000_0E0C);
        let auth_down = FormId(0x0000_0E0D);
        let new_up = FormId(0x0000_0E2C);
        let new_down = FormId(0x0000_0E2D);

        let mut world = sim(vec![item_base(weap_u32)], vec![placed(0x310, weap_u32), placed(0x311, weap_u32)]);
        world.world.activate(FormId::PLAYER_REF, FormId(0x310)).unwrap();
        world.world.activate(FormId::PLAYER_REF, FormId(0x311)).unwrap();
        let drop_a = world.world.place(weap, None, [0.0; 3], [0.0; 3], 1).unwrap().0;
        let drop_b = world.world.place(weap, None, [0.0; 3], [0.0; 3], 1).unwrap().0;

        let mut ws = WorldSounds::default();
        ws.forms.items.insert(weap, (Some(auth_up), Some(auth_down)));

        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<PlayState>()
            .insert_resource(script)
            .insert_resource(world)
            .insert_resource(ws)
            .init_resource::<WeaponSoundSync>()
            .init_resource::<crate::sfx::ScriptSounds>()
            .add_message::<RefChanged>()
            .add_message::<Activated>()
            .add_message::<DoorPassed>()
            .add_message::<ImpactPicked>()
            .add_message::<crate::ui_sounds::UiSound>()
            .add_message::<PlaySound>()
            .add_message::<ActorSound>()
            .add_systems(Update, (script_weapon_sounds, objects).chain());

        // Before override: world pickup & drop play static authored YNAM / ZNAM.
        assert_eq!(hear(&mut app, &[RefChanged::new(0x310), RefChanged::new(drop_a)]), vec![auth_up, auth_down]);

        // Compiled NVSE SetWeaponSound overrides slots 12 & 13: next pickup & drop play NewUp / NewDown.
        app.world_mut()
            .resource_mut::<crate::script::ScriptState>()
            .rt
            .as_mut()
            .unwrap()
            .game
            .run_query(
                "Begin Function {}\nSetWeaponSound OwnedWeap 12 NewUp\nSetWeaponSound OwnedWeap 13 NewDown\nEnd",
                RunCtx::default(),
            )
            .unwrap();
        assert_eq!(hear(&mut app, &[RefChanged::new(0x311), RefChanged::new(drop_b)]), vec![new_up, new_down]);
    }

    #[test]
    fn sound_extender_overrides_loose_body_knock_sounds_end_to_end() {
        use crate::phys::{CreatedBody, PlacedDynamic};
        use crate::sim::test_world::{item_base, placed, sim};
        use fnv_collide::{CollisionObject, Iso, Motion, Shape};
        use fnv_combat::fx::{FxForms, Sound};
        use fnv_game::sound_extender::SoundExtenderTable;
        use fnv_phys::RefPose;
        use std::sync::Arc;
        use std::time::Duration;

        fn sub(sig: &[u8; 4], data: &[u8]) -> Vec<u8> {
            let mut out = Vec::with_capacity(6 + data.len());
            out.extend_from_slice(sig);
            out.extend_from_slice(&(data.len() as u16).to_le_bytes());
            out.extend_from_slice(data);
            out
        }
        fn rec(sig: &[u8; 4], id: u32, subs: &[Vec<u8>]) -> Vec<u8> {
            let body: Vec<u8> = subs.concat();
            let mut out = Vec::with_capacity(24 + body.len());
            out.extend_from_slice(sig);
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&id.to_le_bytes());
            out.extend_from_slice(&[0u8; 8]);
            out.extend_from_slice(&body);
            out
        }
        fn top(sig: &[u8; 4], body: &[u8]) -> Vec<u8> {
            let mut out = Vec::with_capacity(24 + body.len());
            out.extend_from_slice(b"GRUP");
            out.extend_from_slice(&((24 + body.len()) as u32).to_le_bytes());
            out.extend_from_slice(sig);
            out.extend_from_slice(&[0u8; 12]);
            out.extend_from_slice(body);
            out
        }

        const BASE_MAPPED: u32 = 0x0000_0E4B;
        const BASE_DROPPED: u32 = 0x0000_0E4C;
        const BASE_VANILLA: u32 = 0x0000_0E4D;
        const SND_BOTTLE_H: FormId = FormId(0x0000_0901);
        const SND_BOTTLE_L: FormId = FormId(0x0000_0902);
        const SND_FIRST: FormId = FormId(0x0000_0910);
        const SND_OVERRIDE: FormId = FormId(0x0000_0911);
        const SND_DROPPED: FormId = FormId(0x0000_0912);

        let mut esm = rec(b"TES4", 0, &[sub(b"HEDR", &[0, 0, 0x80, 0x3F, 0, 0, 0, 0, 0, 0, 0, 0])]);
        let mut sounds_bin = Vec::new();
        for (id, edid) in [
            (SND_BOTTLE_H.0, b"PHYBottleH\0".as_slice()),
            (SND_BOTTLE_L.0, b"PHYBottleL\0".as_slice()),
            (SND_FIRST.0, b"SynthFirstKnock\0".as_slice()),
            (SND_OVERRIDE.0, b"SynthDrumKnock\0".as_slice()),
            (SND_DROPPED.0, b"SynthDropKnock\0".as_slice()),
        ] {
            sounds_bin.extend(rec(b"SOUN", id, &[sub(b"EDID", edid)]));
        }
        esm.extend(top(b"SOUN", &sounds_bin));
        let mut misc_bin = Vec::new();
        misc_bin.extend(rec(b"MISC", BASE_MAPPED, &[sub(b"EDID", b"SynthDrum\0")]));
        misc_bin.extend(rec(b"MISC", BASE_DROPPED, &[sub(b"EDID", b"SynthRelic\0")]));
        misc_bin.extend(rec(b"MISC", BASE_VANILLA, &[sub(b"EDID", b"SynthBottle\0")]));
        esm.extend(top(b"MISC", &misc_bin));
        let plugins = vec![fnv_esm::Plugin::parse(&esm, "SynthAudio.esm".into()).unwrap()];
        let lo = LoadOrder::new(&plugins).unwrap();

        let dir = std::env::temp_dir().join(format!("nvf-play-sound-extender-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // 1) Absent SoundExtender folder -> vanilla material lookup unchanged.
        let empty_vfs = fnv_vfs::Vfs::open(&dir, &[]).unwrap();
        let empty_se = SoundExtenderTable::load_from_vfs(&lo, &empty_vfs);
        assert!(empty_se.is_empty());

        let phy = HashMap::from([
            ("PHYBottleH".to_string(), SND_BOTTLE_H),
            ("PHYBottleL".to_string(), SND_BOTTLE_L),
        ]);
        assert_eq!(knock_sound(&empty_se, &phy, Some(FormId(BASE_MAPPED)), Some(24), true), Some(SND_BOTTLE_H));
        assert_eq!(knock_sound(&empty_se, &phy, Some(FormId(BASE_MAPPED)), Some(24), false), Some(SND_BOTTLE_L));
        assert_eq!(knock_sound(&empty_se, &phy, Some(FormId(BASE_DROPPED)), None, true), None);

        // 2) Populate temp VFS with sorted SoundExtender *.txt / *.json configs + one malformed file.
        let se_dir = dir.join("NVSE").join("Plugins").join("SoundExtender");
        std::fs::create_dir_all(&se_dir).unwrap();
        std::fs::write(
            se_dir.join("01_base.txt"),
            r#"[
                {"mod": "SynthAudio.esm", "form": "E4B", "sound": "SynthFirstKnock"},
                {"mod": "SynthAudio.esm", "form": "E4C", "sound": "SynthDropKnock"},
                {"mod": "MissingPlugin.esp", "form": "1234", "sound": "SynthFirstKnock"}
            ]"#,
        )
        .unwrap();
        std::fs::write(
            se_dir.join("02_override.JSON"),
            r#"[{"mod": "SynthAudio.esm", "form": "0xE4B", "sound": "SynthDrumKnock"}]"#,
        )
        .unwrap();
        std::fs::write(se_dir.join("03_bad.txt"), "not valid json").unwrap();

        let vfs = fnv_vfs::Vfs::open(&dir, &[]).unwrap();
        let se = SoundExtenderTable::load_from_vfs(&lo, &vfs);
        assert_eq!(se.report().files_loaded, 2);
        assert_eq!(se.report().files_malformed, 1);
        assert_eq!(se.report().skipped_missing_plugin, 1);

        // Build a real knocks Bevy App with three dynamic bodies:
        // - placed_mapped (material 24 BOTTLE, base BASE_MAPPED -> overridden to SND_OVERRIDE)
        // - placed_vanilla (material 24 BOTTLE, base BASE_VANILLA -> vanilla PHYBottleH)
        // - dropped_mapped (material None, base BASE_DROPPED -> overridden to SND_DROPPED)
        let placed_mapped_id = 0x0000_5001u32;
        let placed_vanilla_id = 0x0000_5002u32;
        let mut world = sim(
            vec![item_base(BASE_MAPPED), item_base(BASE_DROPPED), item_base(BASE_VANILLA)],
            vec![placed(placed_mapped_id, BASE_MAPPED), placed(placed_vanilla_id, BASE_VANILLA)],
        );
        let dropped_mapped_id = world.world.place(FormId(BASE_DROPPED), None, [200.0, 0.0, 30.0], [0.0; 3], 1).unwrap().0;

        let mut phys = PhysState::default();
        // Flat floor at z = 0 so falling dynamic boxes land and trigger knocks.
        let floor_obj = CollisionObject {
            node: None,
            node_block: 0,
            node_transform: Iso::IDENTITY,
            body_transform: Iso::IDENTITY,
            motion: Motion::Static,
            layer: 1,
            mass: 0.0,
            friction: 0.5,
            restitution: 0.0,
            shape: Arc::new(Shape::Box { half_extents: [500.0, 500.0, 10.0] }),
            shell: 0.0,
            material: Some(0),
            wind: false,
        };
        phys.world.add_ref(
            &RefPose { form_id: 0x0000_5000, position: [0.0, 0.0, -10.0], rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0], scale: 1.0 },
            &[floor_obj],
        );

        let dynamic_box = |material: Option<u32>| CollisionObject {
            node: None,
            node_block: 0,
            node_transform: Iso::IDENTITY,
            body_transform: Iso::IDENTITY,
            motion: Motion::Dynamic,
            layer: 4,
            mass: 1.0,
            friction: 0.5,
            restitution: 0.0,
            shape: Arc::new(Shape::Box { half_extents: [8.0, 8.0, 8.0] }),
            shell: 0.0,
            material,
            wind: false,
        };
        let identity = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        for (id, x, mat) in [
            (placed_mapped_id, 0.0, Some(24)),
            (placed_vanilla_id, 100.0, Some(24)),
            (dropped_mapped_id, 200.0, None),
        ] {
            let pos = [x, 0.0, 120.0];
            phys.world.add_ref(&RefPose { form_id: id, position: pos, rotation: identity, scale: 1.0 }, &[dynamic_box(mat)]);
            if id == dropped_mapped_id {
                phys.created.insert(
                    id,
                    CreatedBody {
                        local: Some(Iso::IDENTITY),
                        dynamic: true,
                        start: pos,
                        added_at: 0.0,
                        settled: false,
                        marks: 0,
                        rested_at: None,
                        sank: false,
                    },
                );
            } else {
                phys.placed_dynamic.insert(
                    id,
                    PlacedDynamic { local: Iso::IDENTITY, scale: 1.0, added: (pos, identity), moved: false, rested: false },
                );
            }
        }

        let mut fx_forms = FxForms::default();
        for (id, edid) in [
            (SND_BOTTLE_H, "PHYBottleH"),
            (SND_BOTTLE_L, "PHYBottleL"),
            (SND_FIRST, "SynthFirstKnock"),
            (SND_OVERRIDE, "SynthDrumKnock"),
            (SND_DROPPED, "SynthDropKnock"),
        ] {
            fx_forms.sounds.insert(id, Sound { edid: Some(edid.into()), ..Sound::default() });
        }

        let mut app = App::new();
        app.insert_resource(world)
            .insert_resource(phys)
            .insert_resource(Sfx::new(fx_forms))
            .insert_resource(WorldSounds { sound_extender: se, ..Default::default() })
            .insert_resource(Time::<Real>::default())
            .add_message::<PlaySound>()
            .add_systems(Update, knocks);

        // Initial registration frame at t = 0, then advance real time past KNOCK_SETTLE (1.5 s)
        // and step physics until all three falling bodies land on the floor and knock.
        app.update();
        app.world_mut().resource_mut::<Time<Real>>().advance_by(Duration::from_secs(2));
        let mut played = Vec::new();
        for _ in 0..90 {
            app.world_mut().resource_mut::<Time<Real>>().advance_by(Duration::from_millis(16));
            app.world_mut().resource_mut::<PhysState>().world.step();
            app.update();
            played.extend(
                app.world_mut()
                    .resource_mut::<bevy::ecs::message::Messages<PlaySound>>()
                    .drain()
                    .map(|m| m.sound),
            );
        }

        assert!(played.contains(&SND_OVERRIDE), "mapped placed clutter plays later-file SoundExtender override: {played:?}");
        assert!(!played.contains(&SND_FIRST), "earlier-file SoundExtender rule was deterministically superseded: {played:?}");
        assert!(played.contains(&SND_DROPPED), "mapped dropped item with no Havok material still plays SoundExtender override: {played:?}");
        assert!(
            played.contains(&SND_BOTTLE_H) || played.contains(&SND_BOTTLE_L),
            "unmapped sibling body retains vanilla PHYBottle knock sound: {played:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
