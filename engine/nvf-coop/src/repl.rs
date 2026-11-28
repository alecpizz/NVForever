//! The generic records the reliable stream ([`crate::rel`]) carries: changes to a
//! reference's components ([`Change`], each one's [`Value`]), what the other side holds
//! ([`Shadow`], so a change goes only when it differs), and digests of what a guest
//! holds ([`Digest`]), for the host to compare with its own and send again what differs.
//! The world's records ride it too: an object a guest's physics moves ([`Carry`]), a
//! player's dropped items ([`Loose`]) and the visible side effects of the host's scripts
//! ([`Effect`]). Beside the stream, the compact pose of each moving actor ([`ActorPose`],
//! unreliable, newest wins: `crate::net::Msg::Actors`).
//! Nothing here knows about the game: the values are what the game side reads and writes.
//!
//! An item on the reliable stream is a kind byte, then its records. Little-endian. Every
//! list is bounded (by its own bound, or by the item's [`crate::rel::ITEM_MAX`]), and the
//! world records' floats must be finite: an item that breaks either does not parse.

use std::collections::HashMap;

/// Reliable item kinds.
pub mod kind {
    /// Host to guest: changes ([`super::Change`]).
    pub const CHANGES: u8 = 1;
    /// Guest to host: what the guest holds of references ([`super::Digest`]).
    pub const DIGEST: u8 = 2;
    /// World sync's kinds are 0x10-0x2F (each co-op piece has its own range).
    /// Host to guest: script effects ([`super::Effect`]); guest to host, only the
    /// syncing screen's ask ([`super::Effect::Sync`]).
    pub const EFFECTS: u8 = 0x10;
    /// Guest to host: an object the guest's physics moves (held, or falling after a
    /// throw or a drop), with its pose ([`super::Carry`]).
    pub const CARRY: u8 = 0x11;
    /// Both ways: a player's dropped items ([`super::Loose`]).
    pub const LOOSE: u8 = 0x12;
}

/// The components of a reference.
pub mod comp {
    pub const ENABLED: u8 = 0;
    pub const PLACE: u8 = 1;
    pub const OPEN: u8 = 2;
    pub const LOCK: u8 = 3;
    pub const LIFE: u8 = 4;
    pub const LOOK: u8 = 5;
    pub const EXISTS: u8 = 6;
    pub const ANIM: u8 = 7;
    pub const ITEM: u8 = 8;
    pub const POSE: u8 = 9;
    pub const COUNT: usize = 10;
}

/// A component's value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// Enabled, and taken away (deleted).
    Enabled { enabled: bool, deleted: bool },
    /// Where it is: its cell (0: a worldspace's persistent reference with none), position,
    /// rotation (radians) and the scale a script set (0: none).
    Place { cell: u32, pos: [f32; 3], rot: [f32; 3], scale: f32 },
    Open(bool),
    /// Locked and the lock's level; `has` false: no lock at all.
    Lock { has: bool, locked: bool, level: u8 },
    /// Dead.
    Life { dead: bool },
    /// What decides how an actor is drawn: its base, the level and rolls its leveled
    /// picks are read with, and what it wears and holds (the host's equipped items).
    Look { base: u32, level: u16, all_levels: bool, rolls: u8, worn: Vec<u32> },
    /// A reference the host's game created (a dynamic FormID): its base and where it
    /// was made.
    Exists { base: u32, cell: u32, pos: [f32; 3], rot: [f32; 3] },
    /// The furniture an actor uses (0: none) and the weapon it holds (0: none), and
    /// whether it is hostile to the players in the host's game (a guest's compass).
    /// `space`: its interior cell, or the worldspace it stands in, in the host's world
    /// (`crate::net::NpcState::space`; 0: not told).
    Anim { furniture: u32, weapon: u32, hostile: bool, space: u32 },
    /// An item a player dropped, lying in the host's world: its base, how many, and
    /// where it was made. Every player sees it and any one can take it.
    Item { base: u32, count: i32, cell: u32, pos: [f32; 3], rot: [f32; 3] },
    /// A dead actor's body as it lies at rest in the host's world: per ragdoll body (in
    /// the ragdoll's body order), its bone's place and turn (None: a body without a
    /// pose). A guest lays its copy down in it, at rest, instead of dropping it from a
    /// death pose of its own [observed: buckallobill, in game, FNV 1.4.0.525: without it,
    /// bodies knelt upright, or fell from the sky one by one, on a guest that came by
    /// after the deaths].
    Pose { bones: Vec<Option<PoseBone>> },
}

/// One ragdoll body of a [`Value::Pose`]: its bone's place (game units, game space) and
/// turn (a unit quaternion x, y, z, w, each times [`PoseBone::Q`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PoseBone {
    pub at: [f32; 3],
    pub turn: [i16; 4],
}

impl PoseBone {
    pub const Q: f32 = 32767.0;

    /// From a unit quaternion (x, y, z, w).
    pub fn new(at: [f32; 3], q: [f32; 4]) -> PoseBone {
        PoseBone { at, turn: q.map(|v| (v.clamp(-1.0, 1.0) * Self::Q).round() as i16) }
    }

    /// The turn as a unit quaternion (x, y, z, w).
    pub fn quat(&self) -> [f32; 4] {
        let q = self.turn.map(|v| f32::from(v) / Self::Q);
        let n = q.iter().map(|v| v * v).sum::<f32>().sqrt();
        if n > 1e-6 {
            q.map(|v| v / n)
        } else {
            [0.0, 0.0, 0.0, 1.0]
        }
    }
}

/// The most ragdoll bodies a [`Value::Pose`] carries.
pub const MAX_POSE_BONES: usize = 64;

impl Value {
    pub fn comp(&self) -> u8 {
        match self {
            Value::Enabled { .. } => comp::ENABLED,
            Value::Place { .. } => comp::PLACE,
            Value::Open(_) => comp::OPEN,
            Value::Lock { .. } => comp::LOCK,
            Value::Life { .. } => comp::LIFE,
            Value::Look { .. } => comp::LOOK,
            Value::Exists { .. } => comp::EXISTS,
            Value::Anim { .. } => comp::ANIM,
            Value::Item { .. } => comp::ITEM,
            Value::Pose { .. } => comp::POSE,
        }
    }

    fn write(&self, b: &mut Vec<u8>) {
        match self {
            Value::Enabled { enabled, deleted } => b.push(u8::from(*enabled) | u8::from(*deleted) << 1),
            Value::Place { cell, pos, rot, scale } => {
                b.extend_from_slice(&cell.to_le_bytes());
                for v in pos.iter().chain(rot.iter()).chain(std::iter::once(scale)) {
                    b.extend_from_slice(&v.to_le_bytes());
                }
            }
            Value::Open(o) => b.push(u8::from(*o)),
            Value::Lock { has, locked, level } => {
                b.push(u8::from(*has) | u8::from(*locked) << 1);
                b.push(*level);
            }
            Value::Life { dead } => b.push(u8::from(*dead)),
            Value::Look { base, level, all_levels, rolls, worn } => {
                b.extend_from_slice(&base.to_le_bytes());
                b.extend_from_slice(&level.to_le_bytes());
                b.push(u8::from(*all_levels));
                b.push(*rolls);
                let n = worn.len().min(MAX_WORN);
                b.push(n as u8);
                for w in &worn[..n] {
                    b.extend_from_slice(&w.to_le_bytes());
                }
            }
            Value::Exists { base, cell, pos, rot } => {
                b.extend_from_slice(&base.to_le_bytes());
                b.extend_from_slice(&cell.to_le_bytes());
                for v in pos.iter().chain(rot.iter()) {
                    b.extend_from_slice(&v.to_le_bytes());
                }
            }
            Value::Anim { furniture, weapon, hostile, space } => {
                b.extend_from_slice(&furniture.to_le_bytes());
                b.extend_from_slice(&weapon.to_le_bytes());
                b.push(u8::from(*hostile));
                b.extend_from_slice(&space.to_le_bytes());
            }
            Value::Item { base, count, cell, pos, rot } => {
                b.extend_from_slice(&base.to_le_bytes());
                b.extend_from_slice(&count.to_le_bytes());
                b.extend_from_slice(&cell.to_le_bytes());
                for v in pos.iter().chain(rot.iter()) {
                    b.extend_from_slice(&v.to_le_bytes());
                }
            }
            Value::Pose { bones } => {
                let n = bones.len().min(MAX_POSE_BONES);
                b.push(n as u8);
                for bone in &bones[..n] {
                    match bone {
                        None => b.push(0),
                        Some(p) => {
                            b.push(1);
                            for v in p.at {
                                b.extend_from_slice(&v.to_le_bytes());
                            }
                            for v in p.turn {
                                b.extend_from_slice(&v.to_le_bytes());
                            }
                        }
                    }
                }
            }
        }
    }

    fn read(comp: u8, r: &mut Reader) -> Option<Value> {
        Some(match comp {
            comp::ENABLED => {
                let f = r.u8()?;
                Value::Enabled { enabled: f & 1 != 0, deleted: f & 2 != 0 }
            }
            comp::PLACE => {
                let cell = r.u32()?;
                let pos = [r.f32()?, r.f32()?, r.f32()?];
                let rot = [r.f32()?, r.f32()?, r.f32()?];
                Value::Place { cell, pos, rot, scale: r.f32()? }
            }
            comp::OPEN => Value::Open(r.u8()? != 0),
            comp::LOCK => {
                let f = r.u8()?;
                Value::Lock { has: f & 1 != 0, locked: f & 2 != 0, level: r.u8()? }
            }
            comp::LIFE => Value::Life { dead: r.u8()? != 0 },
            comp::LOOK => {
                let base = r.u32()?;
                let level = r.u16()?;
                let all_levels = r.u8()? != 0;
                let rolls = r.u8()?;
                let n = r.u8()? as usize;
                if n > MAX_WORN {
                    return None;
                }
                let worn = (0..n).map(|_| r.u32()).collect::<Option<Vec<_>>>()?;
                Value::Look { base, level, all_levels, rolls, worn }
            }
            comp::EXISTS => {
                let base = r.u32()?;
                let cell = r.u32()?;
                let pos = [r.f32()?, r.f32()?, r.f32()?];
                Value::Exists { base, cell, pos, rot: [r.f32()?, r.f32()?, r.f32()?] }
            }
            comp::ANIM => Value::Anim { furniture: r.u32()?, weapon: r.u32()?, hostile: r.u8()? != 0, space: r.u32()? },
            comp::ITEM => {
                let base = r.u32()?;
                let count = r.i32()?;
                let cell = r.u32()?;
                let pos = r.finite3()?;
                Value::Item { base, count, cell, pos, rot: r.finite3()? }
            }
            comp::POSE => {
                let n = r.u8()? as usize;
                if n > MAX_POSE_BONES {
                    return None;
                }
                let mut bones = Vec::with_capacity(n);
                for _ in 0..n {
                    bones.push(match r.u8()? {
                        0 => None,
                        1 => {
                            let at = r.finite3()?;
                            let turn = [r.u16()? as i16, r.u16()? as i16, r.u16()? as i16, r.u16()? as i16];
                            Some(PoseBone { at, turn })
                        }
                        _ => return None,
                    });
                }
                Value::Pose { bones }
            }
            _ => return None,
        })
    }

    /// A short hash of the value, for digests.
    pub fn hash(&self) -> u32 {
        let mut b = Vec::with_capacity(48);
        b.push(self.comp());
        self.write(&mut b);
        fnv1a(&b)
    }
}

/// The most worn items a look carries.
pub const MAX_WORN: usize = 16;

/// One change: reference `reference`'s component now has `value`.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub reference: u32,
    pub value: Value,
}

/// Pack changes into reliable items of at most `max` bytes each.
pub fn pack_changes(changes: &[Change], max: usize) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut cur = vec![kind::CHANGES];
    let mut rec = Vec::with_capacity(64);
    for c in changes {
        rec.clear();
        rec.extend_from_slice(&c.reference.to_le_bytes());
        rec.push(c.value.comp());
        c.value.write(&mut rec);
        if cur.len() + rec.len() > max && cur.len() > 1 {
            out.push(std::mem::replace(&mut cur, vec![kind::CHANGES]));
        }
        cur.extend_from_slice(&rec);
    }
    if cur.len() > 1 {
        out.push(cur);
    }
    out
}

/// The changes of a [`kind::CHANGES`] item (None: it does not parse).
pub fn unpack_changes(item: &[u8]) -> Option<Vec<Change>> {
    let (&k, rest) = item.split_first()?;
    if k != kind::CHANGES {
        return None;
    }
    let mut r = Reader(rest);
    let mut out = Vec::new();
    while !r.0.is_empty() {
        let reference = r.u32()?;
        let comp = r.u8()?;
        out.push(Change { reference, value: Value::read(comp, &mut r)? });
    }
    Some(out)
}

/// A guest's digest: for each reference, a hash of the components it holds
/// ([`digest_of`]). The host compares with its own and sends again what differs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Digest {
    pub refs: Vec<(u32, u32)>,
}

/// One reference's digest from its components' values (in any order).
pub fn digest_of<'a>(values: impl IntoIterator<Item = &'a Value>) -> u32 {
    let mut hs: Vec<u32> = values.into_iter().map(Value::hash).collect();
    hs.sort_unstable();
    let mut b = Vec::with_capacity(hs.len() * 4);
    for h in hs {
        b.extend_from_slice(&h.to_le_bytes());
    }
    fnv1a(&b)
}

pub fn pack_digest(d: &Digest, max: usize) -> Vec<Vec<u8>> {
    let per = (max - 1) / 8;
    d.refs
        .chunks(per.max(1))
        .map(|c| {
            let mut b = vec![kind::DIGEST];
            for (r, h) in c {
                b.extend_from_slice(&r.to_le_bytes());
                b.extend_from_slice(&h.to_le_bytes());
            }
            b
        })
        .collect()
}

pub fn unpack_digest(item: &[u8]) -> Option<Digest> {
    let (&k, rest) = item.split_first()?;
    if k != kind::DIGEST || rest.len() % 8 != 0 {
        return None;
    }
    let mut r = Reader(rest);
    let mut refs = Vec::new();
    while !r.0.is_empty() {
        refs.push((r.u32()?, r.u32()?));
    }
    Some(Digest { refs })
}

/// An object a guest's physics moves (held with the grab key, or falling after it let
/// go): where its reference is now. The host puts its own copy there, so the host's game
/// and the other guests see the guest carry it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Carry {
    pub reference: u32,
    pub pos: [f32; 3],
    pub rot: [f32; 3],
    /// Let go and at rest: the last of this object's poses.
    pub done: bool,
}

pub fn pack_carry(c: &Carry) -> Vec<u8> {
    let mut b = vec![kind::CARRY];
    b.extend_from_slice(&c.reference.to_le_bytes());
    for v in c.pos.iter().chain(c.rot.iter()) {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.push(u8::from(c.done));
    b
}

/// A [`kind::CARRY`] item (None: it does not parse, or a float is not finite).
pub fn unpack_carry(item: &[u8]) -> Option<Carry> {
    let (&k, rest) = item.split_first()?;
    if k != kind::CARRY {
        return None;
    }
    let mut r = Reader(rest);
    let reference = r.u32()?;
    let (pos, rot) = (r.finite3()?, r.finite3()?);
    let done = r.u8()? != 0;
    r.0.is_empty().then_some(Carry { reference, pos, rot, done })
}

/// A player's dropped items: the host's world holds the one copy, so a drop shows for
/// everyone and a pickup takes it away for everyone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Loose {
    /// Guest to host: the guest dropped `count` of `base`; its own reference `local` has
    /// come to rest there. The host makes it in its world.
    Dropped { local: u32, base: u32, count: i32, cell: u32, pos: [f32; 3], rot: [f32; 3] },
    /// Host to the dropping guest: its `local` reference is `host` in the host's world.
    Made { local: u32, host: u32 },
    /// Guest to host: the guest took the host's `reference`.
    Took { reference: u32 },
    /// Host to guest: someone took `reference` first: the guest gives back the `count` of
    /// `base` its own game put in its inventory.
    Refused { reference: u32, base: u32, count: i32 },
}

pub fn pack_loose(l: &Loose) -> Vec<u8> {
    let mut b = vec![kind::LOOSE];
    // A tag, then whole words.
    let mut u = |v: u32| b.extend_from_slice(&v.to_le_bytes());
    match *l {
        Loose::Dropped { local, base, count, cell, pos, rot } => {
            u(0);
            u(local);
            u(base);
            u(count as u32);
            u(cell);
            for v in pos.iter().chain(rot.iter()) {
                u(v.to_bits());
            }
        }
        Loose::Made { local, host } => {
            u(1);
            u(local);
            u(host);
        }
        Loose::Took { reference } => {
            u(2);
            u(reference);
        }
        Loose::Refused { reference, base, count } => {
            u(3);
            u(reference);
            u(base);
            u(count as u32);
        }
    }
    b
}

/// A [`kind::LOOSE`] item (None: it does not parse, or a float is not finite).
pub fn unpack_loose(item: &[u8]) -> Option<Loose> {
    let (&k, rest) = item.split_first()?;
    if k != kind::LOOSE {
        return None;
    }
    let mut r = Reader(rest);
    let l = match r.u32()? {
        0 => {
            let (local, base, count, cell) = (r.u32()?, r.u32()?, r.i32()?, r.u32()?);
            let pos = r.finite3()?;
            Loose::Dropped { local, base, count, cell, pos, rot: r.finite3()? }
        }
        1 => Loose::Made { local: r.u32()?, host: r.u32()? },
        2 => Loose::Took { reference: r.u32()? },
        3 => Loose::Refused { reference: r.u32()?, base: r.u32()?, count: r.i32()? },
        _ => return None,
    };
    r.0.is_empty().then_some(l)
}

/// A visible side effect of a host script, played by each guest; and, the other way,
/// the syncing screen's ask ([`Effect::Sync`]).
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// A sound form, at a reference (0: none) or a place, as the host played it.
    Sound { sound: u32, at_ref: u32, pos: [f32; 3] },
    /// A music type asked for (MUSC FormID), 0: back to the location's.
    Music { music: u32 },
    /// A script moved the host's player (MoveTo, a DLC's start): every guest goes with
    /// it, to this cell (an interior's, or 0 with `world` for an exterior) and place.
    /// Doors and fast travel move only the one who used them.
    Summon { interior: u32, world: u32, pos: [f32; 3], yaw: f32 },
    /// The session's rules the host set in the lobby: whether players can hurt each
    /// other. Sent when they change and when a guest's stream starts.
    Rules { friendly_fire: bool },
    /// A script played animation group `group` (the bytecode's code) on reference `on`
    /// with start flag `flag`. `settled`: what the host's scripts last asked of it before
    /// this guest's stream started, shown as a draw shows it, not played anew.
    Anim { on: u32, group: u16, flag: u8, settled: bool },
    /// A script reset reference `on`'s animation state (`Reset3DState`).
    AnimReset { on: u32 },
    /// A checkpoint took the host's player's items of form type `code` (40: weapons) into
    /// `container`, all but those in form list `except` (0: none) (a Strip casino's
    /// weapon check): each guest hands over its own the same; `back`: the container gave
    /// them back to the host's player, and each guest gets its own back.
    Checkpoint { container: u32, code: i32, except: u32, back: bool },
    /// A body part of `actor` went on the host (severed, or `exploded`): part `part` of
    /// body part data `data`, GetHitLocation's `location`, hit at `point` going `dir`;
    /// each guest takes it off and throws the same gore.
    Dismember { actor: u32, data: u32, part: u16, location: i32, exploded: bool, point: [f32; 3], dir: [f32; 3] },
    /// The syncing screen's hand-shake: a guest that has just joined or come back after
    /// going down asks (`done` false) once it stands in its place; the host sends that
    /// guest its whole area again and then answers (`done` true) with the same
    /// `serial`, after those changes on the same ordered stream.
    Sync { serial: u32, done: bool },
}

pub fn pack_effects(effects: &[Effect], max: usize) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut cur = vec![kind::EFFECTS];
    for e in effects {
        let mut rec = Vec::with_capacity(24);
        match e {
            Effect::Sound { sound, at_ref, pos } => {
                rec.push(0);
                rec.extend_from_slice(&sound.to_le_bytes());
                rec.extend_from_slice(&at_ref.to_le_bytes());
                for v in pos {
                    rec.extend_from_slice(&v.to_le_bytes());
                }
            }
            Effect::Music { music } => {
                rec.push(1);
                rec.extend_from_slice(&music.to_le_bytes());
            }
            Effect::Summon { interior, world, pos, yaw } => {
                rec.push(2);
                rec.extend_from_slice(&interior.to_le_bytes());
                rec.extend_from_slice(&world.to_le_bytes());
                for v in pos.iter().chain([yaw]) {
                    rec.extend_from_slice(&v.to_le_bytes());
                }
            }
            Effect::Rules { friendly_fire } => {
                rec.push(3);
                rec.push(u8::from(*friendly_fire));
            }
            Effect::Anim { on, group, flag, settled } => {
                rec.push(4);
                rec.extend_from_slice(&on.to_le_bytes());
                rec.extend_from_slice(&group.to_le_bytes());
                rec.push(*flag);
                rec.push(u8::from(*settled));
            }
            Effect::AnimReset { on } => {
                rec.push(5);
                rec.extend_from_slice(&on.to_le_bytes());
            }
            Effect::Checkpoint { container, code, except, back } => {
                rec.push(6);
                rec.extend_from_slice(&container.to_le_bytes());
                rec.extend_from_slice(&code.to_le_bytes());
                rec.extend_from_slice(&except.to_le_bytes());
                rec.push(u8::from(*back));
            }
            Effect::Dismember { actor, data, part, location, exploded, point, dir } => {
                rec.push(7);
                rec.extend_from_slice(&actor.to_le_bytes());
                rec.extend_from_slice(&data.to_le_bytes());
                rec.extend_from_slice(&part.to_le_bytes());
                rec.extend_from_slice(&location.to_le_bytes());
                rec.push(u8::from(*exploded));
                for v in point.iter().chain(dir) {
                    rec.extend_from_slice(&v.to_le_bytes());
                }
            }
            Effect::Sync { serial, done } => {
                rec.push(8);
                rec.extend_from_slice(&serial.to_le_bytes());
                rec.push(u8::from(*done));
            }
        }
        if cur.len() + rec.len() > max && cur.len() > 1 {
            out.push(std::mem::replace(&mut cur, vec![kind::EFFECTS]));
        }
        cur.extend_from_slice(&rec);
    }
    if cur.len() > 1 {
        out.push(cur);
    }
    out
}

/// The effects of a [`kind::EFFECTS`] item (None: it does not parse, or a float is not
/// finite). Codes 9 and 10 are kept for the grass fire's effects, which this build does
/// not read.
pub fn unpack_effects(item: &[u8]) -> Option<Vec<Effect>> {
    let (&k, rest) = item.split_first()?;
    if k != kind::EFFECTS {
        return None;
    }
    let mut r = Reader(rest);
    let mut out = Vec::new();
    while !r.0.is_empty() {
        out.push(match r.u8()? {
            0 => Effect::Sound { sound: r.u32()?, at_ref: r.u32()?, pos: r.finite3()? },
            1 => Effect::Music { music: r.u32()? },
            2 => Effect::Summon { interior: r.u32()?, world: r.u32()?, pos: r.finite3()?, yaw: r.finite()? },
            3 => Effect::Rules { friendly_fire: r.u8()? != 0 },
            4 => Effect::Anim { on: r.u32()?, group: r.u16()?, flag: r.u8()?, settled: r.u8()? != 0 },
            5 => Effect::AnimReset { on: r.u32()? },
            6 => Effect::Checkpoint { container: r.u32()?, code: r.i32()?, except: r.u32()?, back: r.u8()? != 0 },
            7 => Effect::Dismember { actor: r.u32()?, data: r.u32()?, part: r.u16()?, location: r.i32()?, exploded: r.u8()? != 0, point: r.finite3()?, dir: r.finite3()? },
            8 => Effect::Sync { serial: r.u32()?, done: r.u8()? != 0 },
            _ => return None,
        });
    }
    Some(out)
}

/// What the host knows each guest holds, by reference and component: a change goes
/// only when the value differs from it (a reference that changed five times in a
/// tick goes once). The reliable stream delivers every change, so what was sent is what
/// the guest has; when the stream starts again ([`Shadow::clear`]) everything goes again.
#[derive(Default)]
pub struct Shadow {
    held: HashMap<(u32, u8), Value>,
}

impl Shadow {
    /// Whether `value` must go (and note it as sent).
    pub fn take(&mut self, reference: u32, value: &Value) -> bool {
        let key = (reference, value.comp());
        if self.held.get(&key) == Some(value) {
            return false;
        }
        self.held.insert(key, value.clone());
        true
    }

    /// The guest no longer has `reference` as sent (a digest said otherwise): every
    /// component goes again.
    pub fn forget(&mut self, reference: u32) {
        self.held.retain(|(r, _), _| *r != reference);
    }

    /// What the guest holds of `reference`, as sent.
    pub fn of(&self, reference: u32) -> impl Iterator<Item = &Value> {
        self.held.iter().filter(move |((r, _), _)| *r == reference).map(|(_, v)| v)
    }

    pub fn clear(&mut self) {
        self.held.clear();
    }

    pub fn len(&self) -> usize {
        self.held.len()
    }

    pub fn is_empty(&self) -> bool {
        self.held.is_empty()
    }
}

// ---- the compact actor stream ---------------------------------------------------------

/// One moving actor's pose, [`ACTOR_BYTES`] on the wire (against 45 for
/// `crate::net::NpcState`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActorPose {
    pub reference: u32,
    /// Feet, game units (quantised to a quarter unit, ±2 million units).
    pub feet: [f32; 3],
    /// Heading, radians (quantised to 1/65536 of a turn).
    pub heading: f32,
    /// Aim up or down, radians (to about 0.7 degree).
    pub pitch: f32,
    /// `crate::net::npc_flag` bits.
    pub flags: u8,
    /// Attacks begun (wrapping).
    pub attacks: u8,
}

pub const ACTOR_BYTES: usize = 18;
/// Actor poses per packet (fits the 1200-byte budget with the header).
pub const ACTORS_PER_PACKET: usize = 60;
const QUARTER: f32 = 4.0;
const I24_MAX: i32 = (1 << 23) - 1;

fn put_i24(b: &mut Vec<u8>, v: f32) {
    // A float that is not finite goes as 0 (`as` takes NaN to 0).
    let q = (v * QUARTER).round().clamp(-(I24_MAX as f32), I24_MAX as f32) as i32;
    b.extend_from_slice(&q.to_le_bytes()[..3]);
}

fn get_i24(r: &mut Reader) -> Option<f32> {
    let b = r.take(3)?;
    let v = i32::from_le_bytes([b[0], b[1], b[2], if b[2] & 0x80 != 0 { 0xFF } else { 0 }]);
    Some(v as f32 / QUARTER)
}

impl ActorPose {
    pub fn write(&self, b: &mut Vec<u8>) {
        b.extend_from_slice(&self.reference.to_le_bytes());
        for v in self.feet {
            put_i24(b, v);
        }
        let turn = self.heading.rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU;
        b.extend_from_slice(&((turn * 65536.0).round() as u32 as u16).to_le_bytes());
        b.push(((self.pitch / std::f32::consts::FRAC_PI_2).clamp(-1.0, 1.0) * 127.0).round() as i8 as u8);
        b.push(self.flags);
        b.push(self.attacks);
    }

    /// A pose as [`ActorPose::write`] wrote it; every value read is finite.
    pub fn read(r: &mut Reader) -> Option<ActorPose> {
        let reference = r.u32()?;
        let feet = [get_i24(r)?, get_i24(r)?, get_i24(r)?];
        let heading = f32::from(r.u16()?) / 65536.0 * std::f32::consts::TAU;
        let pitch = f32::from(r.u8()? as i8) / 127.0 * std::f32::consts::FRAC_PI_2;
        Some(ActorPose { reference, feet, heading, pitch, flags: r.u8()?, attacks: r.u8()? })
    }

    /// The same pose as the guest will draw it (quantised), to compare with the last sent.
    pub fn quantised(&self) -> ActorPose {
        let mut b = Vec::with_capacity(ACTOR_BYTES);
        self.write(&mut b);
        ActorPose::read(&mut Reader(&b)).expect("own encoding")
    }
}

/// How often an actor's pose goes to a guest: at `distance` game units from the guest,
/// `in_view` when it is in front of the guest. Hz [design].
pub fn pose_hz(distance: f32, in_view: bool) -> f32 {
    // 20 m and 60 m, at 70 units a metre (about 1.43 cm a unit).
    const NEAR: f32 = 20.0 * 70.0;
    const FAR: f32 = 60.0 * 70.0;
    match (distance, in_view) {
        (d, true) if d <= NEAR => 20.0,
        (d, _) if d <= NEAR => 10.0,
        (d, true) if d <= FAR => 10.0,
        _ => 3.0,
    }
}

/// Whether `at` is in front of a player at `eye` looking along `yaw` (compass radians),
/// within a generous 120 degree cone [design].
pub fn in_view(eye: [f32; 3], yaw: f32, at: [f32; 3]) -> bool {
    let (dx, dy) = (at[0] - eye[0], at[1] - eye[1]);
    let d = dx.hypot(dy);
    if d < 1.0 {
        return true;
    }
    // Compass heading: 0 north (+Y), π/2 east (+X).
    let (fx, fy) = (yaw.sin(), yaw.cos());
    (dx * fx + dy * fy) / d >= (60f32).to_radians().cos()
}

// ---- reading ------------------------------------------------------------------------

pub struct Reader<'a>(pub &'a [u8]);

impl<'a> Reader<'a> {
    pub fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.0.len() < n {
            return None;
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Some(a)
    }

    pub fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }

    pub fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_le_bytes([b[0], b[1]]))
    }

    pub fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn i32(&mut self) -> Option<i32> {
        self.u32().map(|v| v as i32)
    }

    pub fn f32(&mut self) -> Option<f32> {
        self.u32().map(f32::from_bits)
    }

    /// A float, None unless finite.
    pub fn finite(&mut self) -> Option<f32> {
        self.f32().filter(|v| v.is_finite())
    }

    /// Three finite floats.
    pub fn finite3(&mut self) -> Option<[f32; 3]> {
        Some([self.finite()?, self.finite()?, self.finite()?])
    }
}

pub fn fnv1a(b: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &x in b {
        h ^= u32::from(x);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Piece 1's kinds are 0x00-0x0F and the world's 0x10-0x2F; every one is a kind the
    /// guard lets through, and none shares a number.
    #[test]
    fn the_kinds_are_ones_the_guard_lets_through() {
        let all = [kind::CHANGES, kind::DIGEST, kind::EFFECTS, kind::CARRY, kind::LOOSE];
        for k in all {
            assert!(k <= crate::guard::ITEM_KIND_MAX);
        }
        assert!(all.iter().enumerate().all(|(i, a)| all[i + 1..].iter().all(|b| a != b)));
        assert!(all[..2].iter().all(|k| *k <= 0x0F) && all[2..].iter().all(|k| (0x10..=0x2F).contains(k)));
    }

    fn every_kind() -> Vec<Change> {
        vec![
            Change { reference: 0x00AB_C001, value: Value::Enabled { enabled: true, deleted: false } },
            Change { reference: 0x5, value: Value::Place { cell: 0x1234, pos: [1.5, -2.0, 300.25], rot: [0.0, 0.1, 3.0], scale: 0.0 } },
            Change { reference: 0x6, value: Value::Open(true) },
            Change { reference: 0x7, value: Value::Lock { has: true, locked: false, level: 50 } },
            Change { reference: 0x8, value: Value::Life { dead: true } },
            Change { reference: 0x9, value: Value::Look { base: 0x1F, level: 12, all_levels: true, rolls: 2, worn: vec![0x10, 0x11, 0x12] } },
            Change { reference: 0xFF00_0001, value: Value::Exists { base: 0x22, cell: 0x33, pos: [1.0; 3], rot: [0.5; 3] } },
            Change { reference: 0xA, value: Value::Anim { furniture: 0x44, weapon: 0x55, hostile: true, space: 0x0010_4C00 } },
            Change { reference: 0xFF00_0002, value: Value::Item { base: 0x15169, count: 3, cell: 0x33, pos: [1.0, 2.0, 3.0], rot: [0.0, 0.0, 1.5] } },
            Change { reference: 0xFF00_0003, value: Value::Pose { bones: vec![Some(PoseBone::new([10.0, -20.5, 300.0], [0.0, 0.0, 0.70710677, 0.70710677])), None, Some(PoseBone { at: [1.0, 2.0, 3.0], turn: [-32767, 0, 0, 1] })] } },
        ]
    }

    #[test]
    fn every_change_round_trips() {
        let all = every_kind();
        let items = pack_changes(&all, 1100);
        assert_eq!(items.len(), 1);
        assert_eq!(unpack_changes(&items[0]).unwrap(), all);
        // Many are cut into items under the budget, nothing lost.
        let many: Vec<Change> = (0..500).flat_map(|_| every_kind()).collect();
        let items = pack_changes(&many, 1100);
        assert!(items.len() > 1 && items.iter().all(|i| i.len() <= 1100));
        let back: Vec<Change> = items.iter().flat_map(|i| unpack_changes(i).unwrap()).collect();
        assert_eq!(back, many);
        // A cut item does not parse.
        assert!(unpack_changes(&items[0][..items[0].len() - 1]).is_none());
    }

    #[test]
    fn a_change_goes_once_until_it_differs() {
        let mut s = Shadow::default();
        let v = Value::Open(true);
        assert!(s.take(1, &v));
        assert!(!s.take(1, &v));
        assert!(s.take(1, &Value::Open(false)));
        assert!(s.take(1, &Value::Enabled { enabled: true, deleted: false }));
        s.forget(1);
        assert!(s.take(1, &Value::Open(false)));
        s.clear();
        assert!(s.is_empty());
    }

    #[test]
    fn a_digest_is_the_same_whatever_the_order() {
        let a = [Value::Open(true), Value::Enabled { enabled: false, deleted: false }];
        let b = [a[1].clone(), a[0].clone()];
        assert_eq!(digest_of(&a), digest_of(&b));
        assert_ne!(digest_of(&a), digest_of(&[Value::Open(true)]));
        let d = Digest { refs: (0..300).map(|i| (i, i * 7)).collect() };
        let items = pack_digest(&d, 1100);
        assert!(items.len() == 3);
        let back: Vec<(u32, u32)> = items.iter().flat_map(|i| unpack_digest(i).unwrap().refs).collect();
        assert_eq!(back, d.refs);
    }

    /// A component this build does not know does not parse: the whole item is dropped.
    #[test]
    fn an_unknown_component_does_not_parse() {
        let mut item = pack_changes(&[Change { reference: 1, value: Value::Open(true) }], 1100).remove(0);
        item[5] = comp::COUNT as u8;
        assert!(unpack_changes(&item).is_none());
    }

    /// The world components' lists keep to their bounds, and their floats are finite.
    #[test]
    fn world_components_out_of_bounds_do_not_parse() {
        let pose = |n: usize| Change { reference: 1, value: Value::Pose { bones: vec![None; n] } };
        let mut item = pack_changes(&[pose(MAX_POSE_BONES)], 1100).remove(0);
        assert!(unpack_changes(&item).is_some());
        // Its count byte, one past the bound (the extra body is there too).
        item[6] = MAX_POSE_BONES as u8 + 1;
        item.push(0);
        assert!(unpack_changes(&item).is_none());
        for bad in [f32::NAN, f32::INFINITY] {
            let item = pack_changes(&[Change { reference: 1, value: Value::Item { base: 1, count: 1, cell: 2, pos: [0.0, bad, 0.0], rot: [0.0; 3] } }], 1100).remove(0);
            assert!(unpack_changes(&item).is_none());
            let item = pack_changes(&[Change { reference: 1, value: Value::Pose { bones: vec![Some(PoseBone { at: [bad, 0.0, 0.0], turn: [0; 4] })] } }], 1100).remove(0);
            assert!(unpack_changes(&item).is_none());
        }
    }

    #[test]
    fn a_pose_bone_keeps_its_turn() {
        let q = [0.1f32, -0.2, 0.3, 0.9];
        let n = q.iter().map(|v| v * v).sum::<f32>().sqrt();
        let unit = q.map(|v| v / n);
        let back = PoseBone::new([0.0; 3], unit).quat();
        assert!(back.iter().zip(&unit).all(|(a, b)| (a - b).abs() < 1e-4), "{back:?}");
    }

    #[test]
    fn every_loose_message_round_trips() {
        let all = [
            Loose::Dropped { local: 0xFF80_0001, base: 0x15169, count: 3, cell: 0x33, pos: [1.5, -2.0, 300.0], rot: [0.0, 0.1, 3.0] },
            Loose::Dropped { local: 1, base: 2, count: -5, cell: 0, pos: [0.0; 3], rot: [0.0; 3] },
            Loose::Made { local: 0xFF80_0001, host: 0xFF00_0042 },
            Loose::Took { reference: 0xFF00_0042 },
            Loose::Refused { reference: 0xFF00_0042, base: 0x15169, count: 3 },
        ];
        for l in all {
            let b = pack_loose(&l);
            assert_eq!(unpack_loose(&b), Some(l));
            assert_eq!(unpack_loose(&b[..b.len() - 1]), None);
            let mut long = b.clone();
            long.push(0);
            assert_eq!(unpack_loose(&long), None, "trailing bytes");
            assert!(unpack_carry(&b).is_none() && unpack_changes(&b).is_none());
        }
        for bad in [f32::NAN, f32::NEG_INFINITY] {
            let l = Loose::Dropped { local: 1, base: 2, count: 1, cell: 0, pos: [0.0, 0.0, bad], rot: [0.0; 3] };
            assert_eq!(unpack_loose(&pack_loose(&l)), None);
            let l = Loose::Dropped { local: 1, base: 2, count: 1, cell: 0, pos: [0.0; 3], rot: [bad, 0.0, 0.0] };
            assert_eq!(unpack_loose(&pack_loose(&l)), None);
        }
    }

    #[test]
    fn a_carried_object_reaches_the_host_as_sent() {
        let c = Carry { reference: 0x0010_4C10, pos: [1.5, -2.25, 900.0], rot: [0.1, 0.0, -3.0], done: true };
        assert_eq!(unpack_carry(&pack_carry(&c)), Some(c));
        assert_eq!(unpack_carry(&pack_carry(&c)[..10]), None);
        assert!(unpack_digest(&pack_carry(&c)).is_none());
        let mut long = pack_carry(&c);
        long.push(1);
        assert_eq!(unpack_carry(&long), None, "trailing bytes");
        for bad in [f32::NAN, f32::INFINITY] {
            assert_eq!(unpack_carry(&pack_carry(&Carry { pos: [bad, 0.0, 0.0], ..c })), None);
            assert_eq!(unpack_carry(&pack_carry(&Carry { rot: [0.0, 0.0, bad], ..c })), None);
        }
    }

    #[test]
    fn effects_round_trip() {
        let e = vec![
            Effect::Sound { sound: 0x1, at_ref: 0x2, pos: [3.0, 4.0, 5.0] },
            Effect::Music { music: 0x77 },
            Effect::Summon { interior: 0, world: 0xDA726, pos: [-1.5, 2.25, 300.0], yaw: 1.25 },
            Effect::Summon { interior: 0x1234, world: 0, pos: [0.0; 3], yaw: 0.0 },
            Effect::Rules { friendly_fire: true },
            Effect::Rules { friendly_fire: false },
            Effect::Anim { on: 0x00AB_C002, group: 0x10, flag: 1, settled: false },
            Effect::Anim { on: 0xFF00_0001, group: 0x2A, flag: 0, settled: true },
            Effect::AnimReset { on: 0x0010_4C16 },
            Effect::Checkpoint { container: 0x0012_795C, code: 40, except: 0x0012_9876, back: false },
            Effect::Checkpoint { container: 0x0012_795C, code: -1, except: 0, back: true },
            Effect::Dismember { actor: 0x0008_D74C, data: 0x0001_3F0B, part: 1, location: 1, exploded: true, point: [26574.0, -33489.0, 7043.0], dir: [0.0, -1.0, 0.0] },
            Effect::Sync { serial: 7, done: false },
            Effect::Sync { serial: 7, done: true },
        ];
        let items = pack_effects(&e, 1100);
        assert_eq!(unpack_effects(&items[0]).unwrap(), e);
        // A float that is not finite, or an effect this build does not read (the grass
        // fire's codes), does not parse.
        let bad = pack_effects(&[Effect::Summon { interior: 0, world: 1, pos: [0.0; 3], yaw: f32::NAN }], 1100).remove(0);
        assert!(unpack_effects(&bad).is_none());
        assert!(unpack_effects(&[kind::EFFECTS, 10, 0, 0, 0, 0]).is_none());
    }

    #[test]
    fn a_pose_is_18_bytes_and_close_enough() {
        let p = ActorPose { reference: 0x0010_2030, feet: [-123_456.3, 98_765.6, -1_234.9], heading: 5.0, pitch: -0.4, flags: 0x15, attacks: 200 };
        let mut b = Vec::new();
        p.write(&mut b);
        assert_eq!(b.len(), ACTOR_BYTES);
        let q = ActorPose::read(&mut Reader(&b)).unwrap();
        assert_eq!(q.reference, p.reference);
        for k in 0..3 {
            assert!((q.feet[k] - p.feet[k]).abs() <= 0.125 + 1e-3, "{k}: {} vs {}", q.feet[k], p.feet[k]);
        }
        assert!((q.heading - p.heading).abs() < 1e-3);
        assert!((q.pitch - p.pitch).abs() < 0.013);
        assert_eq!((q.flags, q.attacks), (p.flags, p.attacks));
        // Quantising twice changes nothing (what is compared with the last sent).
        assert_eq!(q.quantised(), q);
        // Not finite on the way in: finite on the way out.
        let nan = ActorPose { feet: [f32::NAN; 3], heading: f32::NAN, pitch: f32::INFINITY, ..p }.quantised();
        assert!(nan.feet.iter().chain([&nan.heading, &nan.pitch]).all(|v| v.is_finite()));
        // The packet budget holds.
        const _: () = assert!(ACTORS_PER_PACKET * ACTOR_BYTES + 16 <= 1200);
    }

    #[test]
    fn near_actors_in_view_go_most_often() {
        assert_eq!(pose_hz(500.0, true), 20.0);
        assert_eq!(pose_hz(500.0, false), 10.0);
        assert_eq!(pose_hz(3000.0, true), 10.0);
        assert_eq!(pose_hz(3000.0, false), 3.0);
        assert_eq!(pose_hz(9000.0, true), 3.0);
        // Facing north (+Y): ahead is in view, behind is not.
        assert!(in_view([0.0; 3], 0.0, [0.0, 100.0, 0.0]));
        assert!(!in_view([0.0; 3], 0.0, [0.0, -100.0, 0.0]));
        assert!(in_view([0.0; 3], std::f32::consts::FRAC_PI_2, [100.0, 10.0, 0.0]));
    }
}
