//! JIP LN NVSE, batch 13 (#1006): physics and movement. The collision objects' velocities,
//! layers and surface and mass properties, contacts, the ground under an actor, phantoms,
//! materials, grabbing, collision on and off, the local gravity, pushes, script-forced
//! travel, moves to a node, the reticle and a fade, PlaceAtCell, rotations, turning, and
//! the reticle's node and position [documented: each page in the wiki export;
//! docs/spec-notes/jip-physics-13.md].
//!
//! The engine's physics answers through a view of the references the scripts asked about
//! ([`Game::phys_watched`], [`Game::set_phys_view`]): a reference's first ask finds
//! nothing and the view follows from the next frame [hypothesis: as the node functions'
//! scene mirror, `crate::nif_blocks`]. What scripts change goes to the engine as requests
//! ([`Game::take_phys_requests`]).

use crate::game::{Game, RunCtx};
use crate::nif_blocks::{apply, extrinsic_angles, mul, node_place, rotation, transpose, Mat};
use crate::package::{Location, LocationKind, PackageDef, PackageType};
use fnv_script::vm::{ArrayKind, XKey, XVal};
use fnv_sim::FormId;
use std::collections::{BTreeMap, BTreeSet};

/// Game units in JIP's linear velocity unit: "0.1 Meter/sec, or 7 game units/sec"
/// [documented: GetLinearVelocityAlt].
pub const LINEAR_UNIT: f32 = 7.0;

/// Radians a second in JIP's angular unit, "2π rad/s" [documented: GetAngularVelocity].
pub const ANGULAR_UNIT: f32 = std::f32::consts::TAU;

/// The radius of a script-forced travel's destination, game units [hypothesis: near
/// enough to count as arrived, as a Travel package's 0 radius is in practice].
const TRAVEL_RADIUS: i32 = 64;

/// One body of a reference as the engine tells it (`fnv_phys::script::ScriptBody`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PhysBody {
    pub node: Option<String>,
    pub layer: u8,
    pub dynamic: bool,
    /// Game units a second, world axes.
    pub linvel: [f32; 3],
    /// Radians a second, world axes.
    pub angvel: [f32; 3],
    /// Row-major body to world.
    pub rotation: [f32; 9],
    pub mass: f32,
    pub friction: f32,
    pub restitution: f32,
    pub linear_damping: f32,
    pub angular_damping: f32,
    pub inertia: [f32; 3],
}

/// A watched reference's physics as the engine tells it each frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PhysView {
    pub bodies: Vec<PhysBody>,
    /// The references its colliders touch.
    pub contacts: Vec<FormId>,
    /// The player could take it with the grab.
    pub grabbable: bool,
    /// An actor's ground: what it stands on (`None`: the terrain) and the ground's
    /// collision material; `None` in the air.
    pub ground: Option<(Option<FormId>, Option<u32>)>,
    /// Its model has a phantom (non-solid) collision shape.
    pub phantom: bool,
}

/// What the scripts ask of the engine's physics and movement.
#[derive(Debug, Clone, PartialEq)]
pub enum PhysRequest {
    /// A body's velocities, game units and radians a second, world axes.
    Velocity { r: FormId, body: usize, linvel: Option<[f32; 3]>, angvel: Option<[f32; 3]> },
    /// A body's property, JIP's index 0-7.
    Property { r: FormId, body: usize, property: i32, value: f32 },
    Layer { r: FormId, body: usize, layer: u8 },
    Collision { r: FormId, on: bool },
    /// The moving bodies' gravity, game units a second squared; `None` the world's own.
    Gravity(Option<[f32; 3]>),
    /// An actor is shoved along `dir` (unit, game x y) at `speed` game units a second
    /// without falling.
    Shove { r: FormId, dir: [f32; 2], speed: f32 },
}

/// SynchronizePosition's follow: the reference kept at the target's node.
#[derive(Debug, Clone, PartialEq)]
pub struct Sync {
    pub target: FormId,
    pub node: String,
    pub rotation: bool,
    pub z: f32,
}

#[derive(Debug, Default)]
pub struct Jip13State {
    pub watched: BTreeSet<FormId>,
    pub views: BTreeMap<FormId, PhysView>,
    pub requests: Vec<PhysRequest>,
    /// The collision switched off (ToggleObjectCollision 0).
    pub collision_off: BTreeSet<FormId>,
    /// SetLocalGravityVector's gravity, and the interior cell it was set in (`None`: the
    /// exteriors) [documented: an interior's lasts until it is loaded again].
    pub gravity: Option<([f32; 3], Option<FormId>)>,
    pub syncs: BTreeMap<FormId, Sync>,
    /// MoveToFadeDelay's moves: when, the reference, the marker.
    pub fade_delays: Vec<(f64, FormId, FormId)>,
    /// The reticle's node as the engine tells it.
    pub reticle_node: Option<String>,
    /// FaceObject's and Turn's turns: the actor and the heading (radians, clockwise from
    /// north) it turns on the spot to, playing its turn (`nvforever::locomotion`).
    pub turns: Vec<(FormId, f32)>,
    /// Travel packages made for TravelToRef and MoveAwayFrom.
    pub made_packages: u32,
}

/// The first FormID of the travel packages made for scripts [hypothesis: past any
/// reference the world creates, as NVForever's own temporary forms are].
const MADE_PACKAGE_BASE: u32 = 0xFE80_0000;

fn num(args: &[XVal], k: usize) -> Option<f64> {
    match args.get(k) {
        Some(XVal::Num(x)) if x.is_finite() => Some(*x),
        _ => None,
    }
}

fn text(args: &[XVal], k: usize) -> Option<String> {
    match args.get(k) {
        Some(XVal::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn form(args: &[XVal], k: usize) -> Option<FormId> {
    match args.get(k)? {
        XVal::Ref(0) => None,
        XVal::Ref(r) => Some(FormId(*r)),
        XVal::Num(x) if *x >= 1.0 && x.fract() == 0.0 => Some(FormId(*x as u32)),
        _ => None,
    }
}

/// An axis argument: X, Y, Z as letters or 0-2.
fn axis(args: &[XVal], k: usize) -> Option<usize> {
    match args.get(k)? {
        XVal::Str(s) => match s.trim().to_ascii_lowercase().as_str() {
            "x" => Some(0),
            "y" => Some(1),
            "z" => Some(2),
            _ => None,
        },
        XVal::Num(x) if (0.0..3.0).contains(x) => Some(*x as usize),
        XVal::Num(x) if [88.0, 89.0, 90.0, 120.0, 121.0, 122.0].contains(x) => Some(((*x as u32 - 88) % 32) as usize),
        _ => None,
    }
}

fn yes(b: bool) -> XVal {
    XVal::Num(f64::from(u8::from(b)))
}

fn reff(f: Option<FormId>) -> XVal {
    XVal::Ref(f.map_or(0, |f| f.0))
}

/// A list getter's array; none when there is nothing to give [observed: the jip10
/// probes for JIP's list getters].
fn list(g: &mut Game, items: Vec<XVal>) -> XVal {
    if items.is_empty() {
        return XVal::Array(0);
    }
    let entries = items.into_iter().enumerate().map(|(i, v)| (XKey::Num(i as f64), v)).collect();
    XVal::Array(g.ext.new_array(ArrayKind::List, entries))
}

fn is_actor(g: &Game, r: FormId) -> bool {
    crate::functions::base_of(g, r).is_some_and(|b| crate::ai::is_actor_base(g.world.forms, b))
}

/// A form list's members, or the form itself.
fn list_or_one(g: &mut Game, f: FormId) -> Vec<FormId> {
    if g.data.sig(f) == Some(*b"FLST") {
        crate::functions::list_members(g, f)
    } else {
        vec![f]
    }
}

/// A function with script output variables: the return value and the outputs.
fn outputs(g: &mut Game, ret: XVal, outs: &[f64]) -> XVal {
    let entries = std::iter::once(ret).chain(outs.iter().map(|v| XVal::Num(*v))).enumerate().map(|(i, v)| (XKey::Num(i as f64), v)).collect();
    XVal::Array(g.ext.new_array(ArrayKind::List, entries))
}

fn mat9(m: &[f32; 9]) -> Mat {
    [[m[0], m[1], m[2]], [m[3], m[4], m[5]], [m[6], m[7], m[8]]]
}

/// The rotation of angles (radians) taken about the turning axes X, then Y, then Z
/// (Rx·Ry·Rz).
fn intrinsic(a: [f32; 3]) -> Mat {
    mul(&rotation([a[0], 0.0, 0.0]), &mul(&rotation([0.0, a[1], 0.0]), &rotation([0.0, 0.0, a[2]])))
}

/// The angles a reference keeps (radians) for a rotation.
fn angles_of(m: &Mat) -> [f32; 3] {
    extrinsic_angles(m).map(f32::to_radians)
}

/// The material code's place in JIP's table, 0 stone, 1 dirt, 2 grass, 3 glass, 4 metal,
/// 5 wood, 6 organic, 7 cloth, 8 water, 9 hollow metal [documented: GetGroundMaterial],
/// by the impact material the collision code (nif.xml's `Fallout3HavokMaterial`, mod 32)
/// stands for: the table of `fnv_combat::fx::impact_material` (see
/// docs/spec-notes/jip-physics-13.md §2); no code (terrain) dirt.
pub fn material_id(code: Option<u32>) -> i32 {
    let Some(code) = code else { return 1 };
    match code % 32 {
        0 | 10 | 19 => 0,
        1 => 7,
        2 | 18 => 1,
        3 | 24 => 3,
        4 => 2,
        6 | 7 => 6,
        8 => 8,
        9 | 12 => 5,
        16 | 17 | 20 | 22 | 23 | 25 | 28 | 29 => 9,
        _ => 4,
    }
}

impl Game<'_> {
    /// The references whose physics the scripts asked about, for the engine to tell.
    pub fn phys_watched(&self) -> impl Iterator<Item = FormId> + '_ {
        self.other.jip13.watched.iter().copied()
    }

    /// A watched reference's physics this frame (`None`: not in the physics world).
    pub fn set_phys_view(&mut self, r: FormId, v: Option<PhysView>) {
        match v {
            Some(v) => self.other.jip13.views.insert(r, v),
            None => self.other.jip13.views.remove(&r),
        };
    }

    /// The scripts' requests since the last frame.
    pub fn take_phys_requests(&mut self) -> Vec<PhysRequest> {
        std::mem::take(&mut self.other.jip13.requests)
    }

    /// FaceObject's and Turn's turns since the last frame.
    pub fn take_turns(&mut self) -> Vec<(FormId, f32)> {
        std::mem::take(&mut self.other.jip13.turns)
    }

    /// The reticle's node (GetReticleNode), as the engine tells it.
    pub fn set_reticle_node(&mut self, node: Option<String>) {
        self.other.jip13.reticle_node = node;
    }

    /// Whether ToggleObjectCollision switched a reference's collision off.
    pub fn collision_off(&self, r: FormId) -> bool {
        self.other.jip13.collision_off.contains(&r)
    }

    fn view(&mut self, r: FormId) -> Option<&PhysView> {
        self.other.jip13.watched.insert(r);
        self.other.jip13.views.get(&r)
    }

    /// The places of a reference's bodies on `node` ("*" every one).
    fn bodies_at(&mut self, r: FormId, node: &str) -> Vec<(usize, PhysBody)> {
        let Some(v) = self.view(r) else { return Vec::new() };
        v.bodies.iter().cloned().enumerate().filter(|(_, b)| node == "*" || b.node.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(node))).collect()
    }

    /// The first moving body on `node`.
    fn moving_body(&mut self, r: FormId, node: &str) -> Option<(usize, PhysBody)> {
        self.bodies_at(r, node).into_iter().find(|(_, b)| b.dynamic)
    }

    /// Each frame (`Game::tick`): MoveToFadeDelay's moves come due, SynchronizePosition's
    /// references follow their nodes, the gravity of an interior left goes back.
    pub(crate) fn jip13_tick(&mut self) {
        let now = self.time;
        let due: Vec<(FormId, FormId)> = self.other.jip13.fade_delays.iter().filter(|(t, _, _)| *t <= now).map(|&(_, r, m)| (r, m)).collect();
        self.other.jip13.fade_delays.retain(|(t, _, _)| *t > now);
        for (r, marker) in due {
            crate::functions::refs::start_fade_move(self, r, marker, [0.0; 3]);
        }
        let syncs: Vec<(FormId, Sync)> = self.other.jip13.syncs.iter().map(|(r, s)| (*r, s.clone())).collect();
        for (r, s) in syncs {
            let Some((pos, rot)) = node_place(self, s.target, &s.node, true) else { continue };
            let cell = self.cell_of(s.target);
            let pos = [pos[0], pos[1], pos[2] + s.z];
            let angles = if s.rotation { Some(angles_of(&rot)) } else { None };
            crate::functions::refs::put_ref(self, r, cell, pos, angles);
        }
        if let Some((_, Some(cell))) = self.other.jip13.gravity {
            if self.cell_of(FormId::PLAYER_REF) != Some(cell) {
                self.other.jip13.gravity = None;
                self.other.jip13.requests.push(PhysRequest::Gravity(None));
            }
        }
    }

    fn travel_handle(&self) -> Result<FormId, String> {
        let start = MADE_PACKAGE_BASE.checked_add(self.other.jip13.made_packages).ok_or("runtime travel package handles exhausted")?;
        self.free_callable_handle(start, FormId::FIRST_DYNAMIC.0).map(FormId).ok_or_else(|| "runtime travel package handles exhausted".into())
    }

    /// A Travel package to near `target`, made for a script, run on `actor` as its script
    /// package (AddScriptPackage's place).
    fn force_travel(&mut self, actor: FormId, target: FormId, flags: f64) -> Result<(), String> {
        let flags = flags as u32;
        let mut f = 0;
        if flags & 1 != 0 {
            f |= crate::package::flags::ALWAYS_RUN;
        }
        if flags & 2 != 0 {
            f |= crate::package::flags::ALWAYS_SNEAK;
        }
        let def = PackageDef { ty: PackageType::Travel, flags: f, location: Some(Location { kind: LocationKind::NearReference(Some(target)), radius: TRAVEL_RADIUS }), ..PackageDef::default() };
        // The same journey asked again (a script calling every frame) keeps the package it
        // already made for the actor: each one made lives for the session (#3150).
        let had = self.extra.get(&actor).and_then(|x| x.script_package).filter(|id| id.0 >= MADE_PACKAGE_BASE && self.package_overrides.get(id).is_some_and(|d| **d == def));
        let id = match had {
            Some(id) => id,
            None => {
                let id = self.travel_handle()?;
                self.other.jip13.made_packages = id.0 - MADE_PACKAGE_BASE + 1;
                let leaked: &PackageDef = Box::leak(Box::new(def));
                self.package_overrides.insert(id, leaked);
                id
            }
        };
        self.extra.entry(actor).or_default().script_package = Some(id);
        if self.runs_packages() {
            self.set_package(actor, Some(id));
        } else {
            self.extra.entry(actor).or_default().current_package = Some(id);
        }
        Ok(())
    }
}

pub(crate) fn call(g: &mut Game, _ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let zero = XVal::Num(0.0);
    let v = match name {
        // ---- the collision objects' velocities ----
        // On the node's moving body, in revolutions a second [documented]; world axes
        // for the one-axis getter (see docs/spec-notes/jip-physics-13.md §1).
        "getangularvelocity" | "getangvel" => {
            let (Some(r), Some(node), Some(a)) = (subject, text(args, 0), axis(args, 1)) else { return Some(Ok(zero)) };
            match g.moving_body(r, &node) {
                Some((_, b)) => XVal::Num(f64::from(b.angvel[a] / ANGULAR_UNIT)),
                None => zero,
            }
        }
        // Local axes, or global (1) [documented]; 0 and the variables unchanged without a
        // moving body [documented].
        "getangularvelocityalt" | "getlinearvelocityalt" => {
            let (Some(r), Some(node)) = (subject, text(args, 0)) else { return Some(Ok(zero)) };
            // The output variables are not passed (`fnv_script::vm`, `out_params`): the
            // flag is the second argument here.
            let flag = num(args, 1).unwrap_or(0.0) != 0.0;
            let Some((_, b)) = g.moving_body(r, &node) else { return Some(Ok(zero)) };
            let angular = name == "getangularvelocityalt";
            // Angular: local unless global; linear: global unless local [documented].
            let local = if angular { !flag } else { flag };
            let v = if angular { b.angvel.map(|x| x / ANGULAR_UNIT) } else { b.linvel.map(|x| x / LINEAR_UNIT) };
            let v = if local { apply(&transpose(&mat9(&b.rotation)), v) } else { v };
            outputs(g, XVal::Num(1.0), &v.map(f64::from))
        }
        "setangularvelocity" | "setangvel" => {
            let (Some(r), Some(node), Some(a), Some(x)) = (subject, text(args, 0), axis(args, 1), num(args, 2)) else { return Some(Ok(zero)) };
            if let Some((i, b)) = g.moving_body(r, &node) {
                let mut w = b.angvel;
                w[a] = x as f32 * ANGULAR_UNIT;
                g.other.jip13.requests.push(PhysRequest::Velocity { r, body: i, linvel: None, angvel: Some(w) });
            }
            zero
        }
        "setangularvelocityex" | "setlinearvelocity" => {
            let (Some(r), Some(node), Some(x), Some(y), Some(z)) = (subject, text(args, 0), num(args, 1), num(args, 2), num(args, 3)) else { return Some(Ok(zero)) };
            let flag = num(args, 4).unwrap_or(0.0) != 0.0;
            let Some((i, b)) = g.moving_body(r, &node) else { return Some(Ok(zero)) };
            let angular = name == "setangularvelocityex";
            let local = if angular { !flag } else { flag };
            let v = [x as f32, y as f32, z as f32].map(|c| c * if angular { ANGULAR_UNIT } else { LINEAR_UNIT });
            let v = if local { apply(&mat9(&b.rotation), v) } else { v };
            let (linvel, angvel) = if angular { (None, Some(v)) } else { (Some(v), None) };
            g.other.jip13.requests.push(PhysRequest::Velocity { r, body: i, linvel, angvel });
            yes(true)
        }
        // The reference's velocity, game units a second, on an axis or in all (the
        // length) [documented; hypothesis: the units are GetActorVelocity's]: its moving
        // body's, else how fast it moved since the last frame (GetActorVelocity's measure).
        "getobjectvelocity" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            let v = match g.moving_body(r, "*") {
                Some((_, b)) => b.linvel,
                None => g.jip2.velocity.get(&r).map_or([0.0; 3], |(_, v)| *v),
            };
            match axis(args, 0) {
                Some(a) => XVal::Num(f64::from(v[a])),
                None => XVal::Num(f64::from((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt())),
            }
        }
        // For a frame [documented]: the console's SetVel path (`nvf_play::console`).
        "setactorvelocity" => {
            let (Some(r), Some(a), Some(v)) = (subject, axis(args, 0), num(args, 1)) else { return Some(Ok(zero)) };
            g.console_requests.push(crate::functions::console::ConsoleRequest::SetVel { on: r, axis: ['X', 'Y', 'Z'][a], speed: v as f32 });
            zero
        }
        // 0 for an actor [documented]; a moving body's mass up to 100, and 0 for static
        // (non-dynamic) bodies or without collision [observed: Fallout: New Vegas 1.4.0.525 +
        // xNVSE 6.4.9 + JIP LN 57.30, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`),
        // 10 official base masters, `w4c_b4_06_jip13_physics_queries`: static door `0x103E61`
        // returned `0.0`].
        "getrigidbodymass" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            if is_actor(g, r) {
                return Some(Ok(zero));
            }
            let bodies = g.bodies_at(r, "*");
            match bodies.iter().filter(|(_, b)| b.dynamic).map(|(_, b)| b.mass).reduce(f32::max) {
                Some(m) => XVal::Num(f64::from(m.min(100.0))),
                None => zero,
            }
        }
        // ---- layers and properties ----
        "getcollisionobjlayertype" => {
            let (Some(r), Some(node)) = (subject, text(args, 0)) else { return Some(Ok(zero)) };
            XVal::Num(g.bodies_at(r, &node).first().map_or(0.0, |(_, b)| f64::from(b.layer)))
        }
        // 1-42 [documented]; "*" every body [documented].
        "setcollisionobjlayertype" => {
            let (Some(r), Some(node), Some(l)) = (subject, text(args, 0), num(args, 1)) else { return Some(Ok(zero)) };
            if (1.0..=42.0).contains(&l) {
                for (i, _) in g.bodies_at(r, &node) {
                    g.other.jip13.requests.push(PhysRequest::Layer { r, body: i, layer: l as u8 });
                }
            }
            zero
        }
        // 0 friction, 1 restitution, 2 linear and 3 angular damping, 4-6 inertia, 7 mass
        // in kilograms [documented].
        "getcollisionobjproperty" => {
            let (Some(r), Some(node), Some(p)) = (subject, text(args, 0), num(args, 1)) else { return Some(Ok(zero)) };
            let Some((_, b)) = g.bodies_at(r, &node).into_iter().next() else { return Some(Ok(zero)) };
            let v = match p as i32 {
                0 => b.friction,
                1 => b.restitution,
                2 => b.linear_damping,
                3 => b.angular_damping,
                4..=6 => b.inertia[p as usize - 4],
                7 => b.mass,
                _ => 0.0,
            };
            XVal::Num(f64::from(v))
        }
        "setcollisionobjproperty" => {
            let (Some(r), Some(node), Some(p), Some(v)) = (subject, text(args, 0), num(args, 1), num(args, 2)) else { return Some(Ok(zero)) };
            if (0.0..8.0).contains(&p) {
                for (i, _) in g.bodies_at(r, &node) {
                    g.other.jip13.requests.push(PhysRequest::Property { r, body: i, property: p as i32, value: v as f32 });
                }
            }
            zero
        }
        // ---- contacts, phantoms, the ground ----
        "getcontactrefs" => {
            let Some(r) = subject else { return Some(Ok(XVal::Array(0))) };
            let refs: Vec<XVal> = g.view(r).map(|v| v.contacts.iter().map(|c| XVal::Ref(c.0)).collect()).unwrap_or_default();
            list(g, refs)
        }
        "gethascontact" | "gethascontactbase" | "gethascontacttype" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            let contacts = g.view(r).map(|v| v.contacts.clone()).unwrap_or_default();
            let hit = match name {
                "gethascontact" => {
                    let Some(f) = form(args, 0) else { return Some(Ok(zero)) };
                    let set = list_or_one(g, f);
                    contacts.iter().any(|c| set.contains(c))
                }
                "gethascontactbase" => {
                    let Some(f) = form(args, 0) else { return Some(Ok(zero)) };
                    let set = list_or_one(g, f);
                    contacts.iter().any(|&c| crate::functions::base_of(g, c).is_some_and(|b| set.contains(&b)))
                }
                _ => {
                    let Some(t) = num(args, 0) else { return Some(Ok(zero)) };
                    contacts.iter().any(|&c| crate::functions::base_of(g, c).and_then(|b| g.data.sig(b)).and_then(|s| crate::nvse_jip9::type_code(&s)) == Some(t as i32))
                }
            };
            yes(hit)
        }
        // A phantom shape in the model; 0 for Doc Mitchell [observed: the jip13 probe,
        // 2026-11-09, against the page's "always return 1 when called on an actor"].
        "gethasphantom" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            yes(g.view(r).is_some_and(|v| v.phantom))
        }
        // -1 for a non-actor or in the air [documented].
        "getgroundmaterial" => {
            let Some(r) = subject.filter(|&r| is_actor(g, r)) else { return Some(Ok(XVal::Num(-1.0))) };
            match g.view(r).and_then(|v| v.ground) {
                Some((_, code)) => XVal::Num(f64::from(material_id(code))),
                None => XVal::Num(-1.0),
            }
        }
        "getobjectunderfeet" => {
            let Some(r) = subject else { return Some(Ok(XVal::Ref(0))) };
            reff(g.view(r).and_then(|v| v.ground).and_then(|(on, _)| on))
        }
        // The first thing a ray from the reference along its facing meets, -1 for none
        // [documented]; from 64 units above an actor's feet, as GetRayCastPos [hypothesis];
        // the collision flag and the node are not used [decision: NVForever's casts meet
        // everything but the player].
        "getraycastmaterial" => {
            let Some(r) = subject else { return Some(Ok(XVal::Num(-1.0))) };
            let Some((pos, rot)) = g.ref_pos_rot(r) else { return Some(Ok(XVal::Num(-1.0))) };
            let from = if is_actor(g, r) { [pos[0], pos[1], pos[2] + 64.0] } else { pos };
            let dir = [rot[2].sin() * rot[0].cos(), rot[2].cos() * rot[0].cos(), -rot[0].sin()];
            let to = [0, 1, 2].map(|k| from[k] + dir[k] * 50_000.0);
            match crate::ray::cast_hit(from, to, crate::ray::CastKind::World) {
                Some(h) => XVal::Num(f64::from(material_id(h.material))),
                None => XVal::Num(-1.0),
            }
        }
        "isgrabbable" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            yes(g.view(r).is_some_and(|v| v.grabbable))
        }
        "toggleobjectcollision" | "toc" => {
            let (Some(r), Some(t)) = (subject, num(args, 0)) else { return Some(Ok(zero)) };
            let on = t != 0.0;
            if on {
                g.other.jip13.collision_off.remove(&r);
            } else {
                g.other.jip13.collision_off.insert(r);
            }
            g.other.jip13.requests.push(PhysRequest::Collision { r, on });
            zero
        }
        // For the exteriors, or for the interior the player is in until it loads again
        // [documented]; the moving bodies take it [decision: the characters keep their own
        // fall].
        "setlocalgravityvector" => {
            let (Some(x), Some(y), Some(z)) = (num(args, 0), num(args, 1), num(args, 2)) else { return Some(Ok(zero)) };
            let v = [x as f32, y as f32, z as f32];
            let cell = g.cell_of(FormId::PLAYER_REF).filter(|&c| g.world.placed.world_of(c).is_none());
            g.other.jip13.gravity = Some((v, cell));
            g.other.jip13.requests.push(PhysRequest::Gravity(Some(v)));
            zero
        }
        // ---- pushes ----
        // Knocked down and thrown away from (or, negative, toward) the actor's place moved
        // by the offsets [documented]; not the dead [documented]. The speed is
        // PushActorAway's for the force (see docs/spec-notes/jip-physics-13.md §3).
        "pushactorawayalt" => {
            let (Some(r), Some(force)) = (subject, num(args, 0)) else { return Some(Ok(zero)) };
            if g.is_dead(r) {
                return Some(Ok(zero));
            }
            let Some((pos, _)) = g.ref_pos_rot(r) else { return Some(Ok(zero)) };
            let origin = [pos[0] + num(args, 1).unwrap_or(0.0) as f32, pos[1] + num(args, 2).unwrap_or(0.0) as f32, pos[2] + num(args, 3).unwrap_or(0.0) as f32];
            let d = [pos[0] - origin[0], pos[1] - origin[1], pos[2] - origin[2]];
            let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            let speed = force as f32 * crate::unconscious::PUSH_SPEED;
            let shove = if len > 1e-3 { d.map(|c| c / len * speed) } else { [0.0, 0.0, speed.abs()] };
            g.knock_down(r, None, shove);
            zero
        }
        // Along a Z angle, or away from a reference [documented], without a fall; the
        // speed is PushActorAway's for the force (see docs/spec-notes/jip-physics-13.md §3).
        "pushactornoragdoll" => {
            let (Some(r), Some(force)) = (subject, num(args, 0)) else { return Some(Ok(zero)) };
            let dir = match form(args, 2).and_then(|o| g.ref_pos_rot(o)).zip(g.ref_pos_rot(r)) {
                Some(((o, _), (p, _))) => {
                    let (dx, dy) = (p[0] - o[0], p[1] - o[1]);
                    let len = (dx * dx + dy * dy).sqrt();
                    if len > 1e-3 {
                        [dx / len, dy / len]
                    } else {
                        return Some(Ok(zero));
                    }
                }
                None => {
                    let a = (num(args, 1).unwrap_or(0.0) as f32).to_radians();
                    [a.sin(), a.cos()]
                }
            };
            g.other.jip13.requests.push(PhysRequest::Shove { r, dir, speed: force as f32 * crate::unconscious::PUSH_SPEED });
            zero
        }
        // ---- script-forced travel ----
        // A Travel package to the reference [documented: "as in a Travel AI package"];
        // run (1), sneak (2) [documented].
        "traveltoref" => {
            let (Some(r), Some(t)) = (subject, form(args, 0)) else { return Some(Ok(zero)) };
            if let Err(e) = g.force_travel(r, t, num(args, 1).unwrap_or(0.0)) { return Some(Err(e)); }
            zero
        }
        // To a place within the distance, directly away from the reference [documented;
        // hypothesis: straight away, the farthest the distance allows], by a Travel
        // package to a marker made there.
        "moveawayfrom" => {
            let (Some(r), Some(t), Some(d)) = (subject, form(args, 0), num(args, 1)) else { return Some(Ok(zero)) };
            let (Some((p, _)), Some((o, _))) = (g.ref_pos_rot(r), g.ref_pos_rot(t)) else { return Some(Ok(zero)) };
            let (dx, dy) = (p[0] - o[0], p[1] - o[1]);
            let len = (dx * dx + dy * dy).sqrt().max(1e-3);
            let to = [p[0] + dx / len * d as f32, p[1] + dy / len * d as f32, p[2]];
            let Some(marker) = g.data.by_edid("XMarker") else { return Some(Ok(zero)) };
            if let Err(e) = g.travel_handle() { return Some(Err(e)); }
            let cell = g.cell_of(r);
            let Ok(m) = g.create_at(marker, cell, to, [0.0; 3]) else { return Some(Ok(zero)) };
            if let Err(e) = g.force_travel(r, m, num(args, 2).unwrap_or(0.0)) { return Some(Err(e)); }
            zero
        }
        // ---- moves ----
        // To the node's place in the target's 3D, plus the offsets [documented]; 0 while
        // the target's 3D is not known yet (its first ask; see docs/spec-notes/jip-physics-13.md §4).
        "movetonode" => {
            let (Some(r), Some(t), Some(node)) = (subject, form(args, 0), text(args, 1)) else { return Some(Ok(zero)) };
            let Some((pos, _)) = node_place(g, t, &node, true) else { return Some(Ok(zero)) };
            let pos = [pos[0] + num(args, 2).unwrap_or(0.0) as f32, pos[1] + num(args, 3).unwrap_or(0.0) as f32, pos[2] + num(args, 4).unwrap_or(0.0) as f32];
            let cell = g.cell_of(t);
            crate::functions::refs::put_ref(g, r, cell, pos, None);
            yes(true)
        }
        // To the reticle's point within the range (10,000 by default) [documented]; 0
        // when it points at nothing within it (see docs/spec-notes/jip-physics-13.md §4).
        "movetoreticle" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            let range = num(args, 0).filter(|&x| x > 0.0).unwrap_or(10_000.0) as f32;
            let Some(ret) = g.senses.reticle else { return Some(Ok(zero)) };
            let Some(d) = ret.hit.filter(|&d| d <= range) else { return Some(Ok(zero)) };
            let p = [0, 1, 2].map(|k| ret.start[k] + ret.dir[k] * d);
            let pos = [p[0] + num(args, 1).unwrap_or(0.0) as f32, p[1] + num(args, 2).unwrap_or(0.0) as f32, p[2] + num(args, 3).unwrap_or(0.0) as f32];
            let cell = g.cell_of(FormId::PLAYER_REF);
            crate::functions::refs::put_ref(g, r, cell, pos, None);
            yes(true)
        }
        // MoveToMarkerWithFade after the delay [documented].
        "movetofadedelay" => {
            let (Some(r), Some(m), Some(t)) = (subject, form(args, 0), num(args, 1)) else { return Some(Ok(zero)) };
            let at = g.time + t.max(0.0);
            g.other.jip13.fade_delays.push((at, r, m));
            zero
        }
        // Like PlaceAtMe, in a cell or a worldspace at the position [documented]: `count`
        // references [hypothesis: as PlaceAtMe makes of a form that is not an item]; the
        // last made.
        "placeatcell" => {
            let (Some(base), Some(c), Some(x), Some(y), Some(z)) = (form(args, 0), form(args, 2), num(args, 3), num(args, 4), num(args, 5)) else { return Some(Ok(XVal::Ref(0))) };
            let pos = [x as f32, y as f32, z as f32];
            let cell = match g.data.sig(c).as_ref().map(|s| &s[..]) {
                Some(b"WRLD") => g.world.placed.world_grid.get(&c).and_then(|grid| grid.get(&((pos[0] / 4096.0).floor() as i32, (pos[1] / 4096.0).floor() as i32))).copied(),
                Some(b"CELL") => Some(g.world.placed.exterior_cell_at(c, pos).unwrap_or(c)),
                _ => None,
            };
            let Some(cell) = cell else { return Some(Ok(XVal::Ref(0))) };
            let mut last = None;
            for _ in 0..num(args, 1).unwrap_or(1.0).max(1.0) as u32 {
                match g.create_at(base, Some(cell), pos, [0.0; 3]) {
                    Ok(r) => last = Some(r),
                    Err(_) => break,
                }
            }
            reff(last)
        }
        // About the point by the angles (degrees, taken about the world axes), the
        // reference's place and angles both [documented; hypothesis: the axes' order as
        // its angles are kept].
        "rotatearoundpoint" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            let v: Vec<f32> = (0..6).map(|k| num(args, k).unwrap_or(0.0) as f32).collect();
            let Some((pos, rot)) = g.ref_pos_rot(r) else { return Some(Ok(zero)) };
            let turn = rotation([v[3], v[4], v[5]].map(f32::to_radians));
            let p = [v[0], v[1], v[2]];
            let d = apply(&turn, [pos[0] - p[0], pos[1] - p[1], pos[2] - p[2]]);
            let angles = angles_of(&mul(&turn, &rotation(rot)));
            let cell = g.cell_of(r);
            crate::functions::refs::put_ref(g, r, cell, [p[0] + d[0], p[1] + d[1], p[2] + d[2]], Some(angles));
            zero
        }
        // 0 set the global angles, 1 the local ones, 2 turn by local, 3 by global, 4 face
        // the point given [documented].
        "setangleex" => {
            let (Some(r), Some(x), Some(y), Some(z)) = (subject, num(args, 0), num(args, 1), num(args, 2)) else { return Some(Ok(zero)) };
            let Some((pos, rot)) = g.ref_pos_rot(r) else { return Some(Ok(zero)) };
            let a = [x as f32, y as f32, z as f32];
            let rad = a.map(f32::to_radians);
            let angles = match num(args, 3).unwrap_or(0.0) as i32 {
                1 => angles_of(&intrinsic(rad)),
                2 => angles_of(&mul(&rotation(rot), &intrinsic(rad))),
                3 => angles_of(&mul(&rotation(rad), &rotation(rot))),
                4 => {
                    let d = [a[0] - pos[0], a[1] - pos[1], a[2] - pos[2]];
                    let flat = (d[0] * d[0] + d[1] * d[1]).sqrt();
                    [-d[2].atan2(flat), 0.0, d[0].atan2(d[1])]
                }
                _ => rad,
            };
            let cell = g.cell_of(r);
            crate::functions::refs::put_ref(g, r, cell, pos, Some(angles));
            zero
        }
        // ---- turning ----
        // The actor turns on the spot, playing its turn [documented]: to face the object,
        // or by the degrees, right positive [documented].
        "faceobject" | "turn" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            let Some((pos, rot)) = g.ref_pos_rot(r) else { return Some(Ok(zero)) };
            let heading = if name == "turn" {
                let Some(d) = num(args, 0) else { return Some(Ok(zero)) };
                rot[2] + (d as f32).to_radians()
            } else {
                let Some((o, _)) = form(args, 0).and_then(|o| g.ref_pos_rot(o)) else { return Some(Ok(zero)) };
                (o[0] - pos[0]).atan2(o[1] - pos[1])
            };
            g.other.jip13.turns.push((r, heading.rem_euclid(std::f32::consts::TAU)));
            zero
        }
        // The reference kept at the target's node (its root without one), its angles too
        // (1), raised by the offset [documented: the arguments]; without a target it is let
        // go (see docs/spec-notes/jip-physics-13.md §4).
        "synchronizeposition" | "syncpos" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            match form(args, 0) {
                Some(t) => {
                    let node = text(args, 3).filter(|n| !n.is_empty()).unwrap_or_else(|| "Scene Root".into());
                            g.other.jip13.syncs.insert(r, Sync { target: t, node, rotation: num(args, 1).unwrap_or(0.0) != 0.0, z: num(args, 2).unwrap_or(0.0) as f32 });
                }
                None => {
                    g.other.jip13.syncs.remove(&r);
                }
            }
            zero
        }
        // ---- the reticle ----
        // The node of what the crosshair points at within the range [documented], as the
        // engine tells it; empty for none.
        "getreticlenode" => {
            let range = num(args, 0).filter(|&x| x > 0.0);
            let within = g.senses.reticle.and_then(|r| r.hit).is_some_and(|d| range.is_none_or(|m| f64::from(d) <= m));
            XVal::Str(if within { g.other.jip13.reticle_node.clone().unwrap_or_default() } else { String::new() })
        }
        // 1 and the point, else 0 [documented].
        "getreticleposalt" => {
            let Some(ret) = g.senses.reticle else { return Some(Ok(outputs(g, zero, &[]))) };
            match ret.hit {
                Some(d) => {
                    let p = [0, 1, 2].map(|k| f64::from(ret.start[k] + ret.dir[k] * d));
                    outputs(g, XVal::Num(1.0), &p)
                }
                None => outputs(g, zero, &[]),
            }
        }
        _ => return None,
    };
    Some(Ok(v))
}
