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
use crate::sfx::{PlaySound, Sfx, SoundSource};
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

/// A space's placed objects with a sound of their own, found the first frame, with each
/// sound's reach.
#[derive(Default)]
struct SpaceSources {
    /// Statics, which never move: (reference, sound, kind, place, reach).
    fixed: Vec<(u32, FormId, usize, [f32; 3], f32)>,
    /// The others, whose place is read each frame: (reference, sound, kind, reach).
    moving: Vec<(u32, FormId, usize, f32)>,
    /// The statics (indices into `fixed`) within their reach plus [`NEAR_SLACK`] of
    /// `near_at`: while the listener stays within [`NEAR_SLACK`] of that point, no other
    /// static can be in reach, so only these are measured.
    near: Vec<u32>,
    near_at: Option<[f32; 3]>,
}

/// How far the listener moves before the statics near it are picked again (game units).
const NEAR_SLACK: f32 = 2048.0;

fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

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
    /// Placed objects with a sound of their own, by the space they are in.
    sources: Option<HashMap<crate::music::Space, SpaceSources>>,
    pub stats: WorldSoundStats,
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

fn init(mut commands: Commands, d: Res<PlayWorld>) {
    let t = std::time::Instant::now();
    let forms = SoundForms::build(d.lo);
    info!(
        "world sounds: items (records, with a pick-up sound, with a drop sound): {}; {} containers with open or close sounds, {} activators with an activation sound; sounds of their own: {}; read in {:.0} ms",
        forms.counts.iter().map(|(s, c)| format!("{s} {} {} {}", c[0], c[1], c[2])).collect::<Vec<_>>().join(", "),
        forms.containers.len(),
        forms.activators.len(),
        OWN_SOUNDS.iter().zip(forms.object_sound_counts).map(|((_, sub, what), n)| format!("{n} {what} ({})", String::from_utf8_lossy(*sub))).collect::<Vec<_>>().join(", "),
        t.elapsed().as_secs_f64() * 1000.0
    );
    commands.insert_resource(WorldSounds { forms, ..Default::default() });
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

/// Knocks of loose bodies (see the module notes).
fn knocks(mut ws: ResMut<WorldSounds>, sfx: Option<Res<Sfx>>, phys: Option<Res<PhysState>>, time: Res<Time<Real>>, mut out: MessageWriter<PlaySound>) {
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
        let sound = code.and_then(|c| phy_sound(c, hard)).and_then(|e| phy.get(e).copied());
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
            code.and_then(|c| phy_sound(c, hard)).unwrap_or("none")
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
    mut out: MessageWriter<PlaySound>,
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
    let in_container = menu.as_ref().and_then(|m| m.0.as_ref()).is_some_and(|m| matches!(m.mode, Mode::Container(_)));
    for &RefChanged { form_id: id, restored } in changed.read() {
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
            if in_container && s.dropped_by.is_some() {
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
    if let (Some(before), Some(now)) = (ws.carried.as_ref(), now_carried.as_ref()) {
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
        out.write(PlaySound { sound, at: Some(at) });
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

    // Scripts: the engine's answers for the sounds already played, StopSound's stops,
    // and the new sounds (`fnv_game::nvse_sound`).
    if let Some(rt) = script.and_then(|s| s.into_inner().rt.as_mut()) {
        for (id, seconds) in script_sounds.heard.drain(..) {
            rt.game.gaps.sounds.heard(id, seconds);
        }
        script_sounds.stops.append(&mut rt.game.gaps.sounds.stops);
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
        let at = match r.at {
            None => None,
            Some(FormId::PLAYER_REF) => eye,
            Some(at) => ref_pos(&sim, at.0),
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
fn object_sources(mut ws: ResMut<WorldSounds>, sim: Res<SimState>, loc: Res<crate::Location>, player: Option<Res<Player>>, sfx: Option<Res<Sfx>>, mut out: MessageWriter<SoundSource>) {
    let (Some(player), Some(sfx)) = (player, sfx) else { return };
    let WorldSounds { forms, sources, stats, .. } = &mut *ws;
    let sources = sources.get_or_insert_with(|| {
        let mut by: HashMap<crate::music::Space, SpaceSources> = HashMap::new();
        let mut n = 0;
        for p in sim.refs.refs.values() {
            let (Some(&(sound, kind)), Some(cell)) = (forms.object_sounds.get(&p.base), p.cell) else { continue };
            n += 1;
            // A sound with no record is never heard.
            let Some(reach) = sfx.forms.sounds.get(&sound).map(|s| s.max_distance * 1.1) else { continue };
            let space = sim.refs.cell_world.get(&cell).map_or(crate::music::Space::Interior(cell.0), |w| crate::music::Space::World(w.0));
            let e = by.entry(space).or_default();
            if kind == STATIC_KIND {
                e.fixed.push((p.id.0, sound, kind, p.pos, reach));
            } else {
                e.moving.push((p.id.0, sound, kind, reach));
            }
        }
        stats.object_sources = n;
        let fixed: usize = by.values().map(|s| s.fixed.len()).sum();
        info!("world sounds: {n} placed objects with a sound of their own ({fixed} statics)");
        by
    });
    let eye = player.eye();
    let space = crate::music::Space::of(&loc);
    let Some(list) = sources.get_mut(&space) else { return };
    if list.near_at.is_none_or(|a| dist2(a, eye) > NEAR_SLACK * NEAR_SLACK) {
        list.near = (0..list.fixed.len() as u32).filter(|&i| {
            let (.., at, reach) = list.fixed[i as usize];
            dist2(at, eye) <= (reach + NEAR_SLACK).powi(2)
        }).collect();
        list.near_at = Some(eye);
    }
    let fixed = list.near.iter().map(|&i| list.fixed[i as usize]);
    let moving = list.moving.iter().filter_map(|&(id, sound, kind, reach)| Some((id, sound, kind, ref_pos(&sim, id)?, reach)));
    for (id, sound, kind, at, reach) in fixed.chain(moving) {
        if dist2(at, eye) > reach * reach || !sim.world.is_enabled(FormId(id)).unwrap_or(false) {
            continue;
        }
        if stats.object_sources_sent.insert(id) {
            stats.object_sources_by_kind[kind] += 1;
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

pub struct WorldSoundsPlugin;

impl Plugin for WorldSoundsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, init)
            .add_systems(Update, (knocks, objects, object_sources, water_sources).in_set(PlaySet::Sync))
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
        let restored = |form_id| RefChanged { form_id, restored: true };
        // Picked up before the save, and picked up now, in the same frame.
        assert_eq!(hear(&mut app, &[restored(0x301), RefChanged::new(0x302)]), vec![UP]);
        // Dropped before the save, and dropped now.
        assert_eq!(hear(&mut app, &[restored(dropped_before), RefChanged::new(dropped_now)]), vec![DOWN]);
        let stats = &app.world().resource::<WorldSounds>().stats;
        assert_eq!((stats.pickups, stats.drops), (1, 1));
        // Both count as heard: a later notice of them makes no sound.
        assert!(hear(&mut app, &[RefChanged::new(0x301), RefChanged::new(dropped_before)]).is_empty());
    }
}
