//! JIP LN NVSE, batch 14 (#1006): the last of JIP's functions. Effects added to magic
//! items, primitives, the auxiliary-variable condition, IsSpellTargetList, DismemberLimb,
//! FireWeaponEx, PlayIdleEx, PlaySound3DNode, per-reference models, texture slots, VATS
//! targets, the gun wobble, the projectiles' fuses and weapons, GetSelfAsInventoryRef,
//! animation sequence frequencies, and the functions left as nothing values by decision
//! (the extra cameras, AttachLine, HasKeywordCond, FreePlayer, UpdatePlayer3D)
//! [documented: each page in the wiki export; docs/spec-notes/jip-last-14.md].

use crate::data::EffectItem;
use crate::game::{Game, RunCtx};
use crate::nif_blocks::node_place;
use fnv_script::vm::{ArrayKind, XKey, XVal};
use fnv_sim::FormId;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// Maximum simultaneously attached named extra cameras per session [design].
pub const EXTRA_CAMERA_MAX: usize = 8;
/// Default square render-target resolution when `pixelSize` is omitted or `<= 0`
/// [documented: GECK wiki ProjectExtraCamera].
pub const EXTRA_CAMERA_DEFAULT_PX: u32 = 256;
/// Minimum clamped square render-target resolution [design].
pub const EXTRA_CAMERA_MIN_PX: u32 = 16;
/// Maximum clamped square render-target resolution [documented: GECK wiki
/// ProjectExtraCamera discourages >1024; design].
pub const EXTRA_CAMERA_MAX_PX: u32 = 1024;
/// Minimum clamped vertical FoV in degrees [design].
pub const EXTRA_CAMERA_MIN_FOV_DEG: f32 = 0.5;
/// Maximum clamped vertical FoV in degrees [design].
pub const EXTRA_CAMERA_MAX_FOV_DEG: f32 = 170.0;

/// Projection target requested by `ProjectExtraCamera` [documented: ProjectExtraCamera; design].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtraCameraTarget {
    /// A named 3D geometry block (`NiTriShape`/`NiTriStrips`) on `reference`'s drawn model.
    Geometry { reference: FormId, node: String },
    /// A UI image tile path (originally prefixed with `*` in `targetGeometry`, stored
    /// without the leading `*`).
    UiTile { path: String },
}

/// Active projection parameters for a named extra camera (`ProjectExtraCamera`)
/// [documented: ProjectExtraCamera; design].
#[derive(Debug, Clone, PartialEq)]
pub struct ExtraCameraProjection {
    pub target: ExtraCameraTarget,
    /// Clamped field-of-view angle in degrees (`EXTRA_CAMERA_MIN_FOV_DEG..=EXTRA_CAMERA_MAX_FOV_DEG`).
    pub fov_deg: f32,
    /// Clamped square pixel resolution (`EXTRA_CAMERA_MIN_PX..=EXTRA_CAMERA_MAX_PX`).
    pub pixel_size: u32,
    /// Non-zero `grayscale` flag from `ProjectExtraCamera`.
    pub grayscale: bool,
    /// Monotonic update sequence for engine change detection.
    pub sequence: u64,
}

/// An attached named extra camera (`AttachExtraCamera`) [documented: AttachExtraCamera; design].
#[derive(Debug, Clone, PartialEq)]
pub struct ExtraCamera {
    /// Caller-supplied camera name (case-preserving; stored under its ASCII lowercase key).
    pub name: String,
    /// Calling reference that owns `parent_node`.
    pub reference: FormId,
    /// Optional parent node name on `reference`'s 3D model (`None` anchors at the root).
    pub parent_node: Option<String>,
    /// Most recent valid projection request for this camera, if any.
    pub projection: Option<ExtraCameraProjection>,
    /// Monotonic revision bumped whenever attach or projection parameters change.
    pub revision: u64,
}

/// A primitive a script gave or changed: sphere or box (or a collision plane), half
/// extents (a sphere's radius first).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScriptPrimitive {
    /// 1 box, 2 sphere, 3 plane [documented: AddPrimitive].
    pub shape: u8,
    pub bounds: [f32; 3],
}

#[derive(Debug, Default)]
pub struct Jip14State {
    /// AddNewEffect's entries, by magic item: those put at the top (newest first) and
    /// those appended.
    pub added_effects: BTreeMap<FormId, (Vec<EffectItem>, Vec<EffectItem>)>,
    /// Signed script-level `[magnitude, area, duration]` trait overrides for `AddNewEffect`
    /// slots, keyed by `(item, back, at)` so `SetNthEffectTraitNumeric` preserves negative
    /// values while `EffectItem` keeps unsigned fields [observed:
    /// `w6d2_05_nth_effect_traits_and_base_edges`; design].
    pub added_effect_traits: BTreeMap<(FormId, bool, usize), [f64; 3]>,
    /// AddPrimitive's primitives and SetPrimitiveBound's changes, by reference.
    pub primitives: BTreeMap<FormId, ScriptPrimitive>,
    /// SetRefrModelPath's models (`None`: the base's again), by reference.
    pub ref_models: BTreeMap<FormId, Option<String>>,
    /// SetVATSTargetable 0's actors, for the session [documented: "targetable again once
    /// a saved game is loaded"].
    pub vats_off: BTreeSet<FormId>,
    /// The engine's VATS targets while VATS is up.
    pub vats_targets: Vec<FormId>,
    /// SetNoGunWobble's actors.
    pub no_wobble: BTreeSet<FormId>,
    /// The projectiles' fuses (seconds left) as the engine tells them, and the scripts'
    /// new fuses for it.
    pub fuses: BTreeMap<FormId, f32>,
    pub fuse_sets: Vec<(FormId, f32)>,
    /// SetAnimSequenceFrequency's frequencies, by reference and lower-case sequence ("*":
    /// every sequence).
    pub anim_freq: BTreeMap<(FormId, String), f32>,
    /// The item whose script runs now (its holder and base), for GetSelfAsInventoryRef.
    pub item_running: Option<(FormId, FormId)>,
    /// The drawn objects' sequence names (lower case), as the engine tells them.
    pub sequences: BTreeMap<FormId, Vec<String>>,
    /// Transient one-shot `UpdatePlayer3D` request for the engine's player body rebuild
    /// (not saved) [design].
    pub update_player_3d: bool,
    /// Attached named extra cameras (`AttachExtraCamera` / `ProjectExtraCamera`), keyed
    /// by ASCII lowercase camera name; session-transient (cleared on restore/new game)
    /// [documented: AttachExtraCamera, ProjectExtraCamera; design].
    pub extra_cameras: BTreeMap<String, ExtraCamera>,
    /// Monotonic counter incremented on any extra-camera attach, detach, projection
    /// update, or clear [design].
    pub extra_camera_revision: u64,
    /// Unique lifetime token for this `Game` instance so engine consumers can detect
    /// game/save replacement even when camera names match across sessions [design].
    pub extra_camera_session: Arc<()>,
}

impl Game<'_> {
    /// Whether `UpdatePlayer3D` has a pending full player 3D rebuild request.
    pub fn update_player_3d_pending(&self) -> bool {
        self.other.jip14.update_player_3d
    }

    /// Consume the pending `UpdatePlayer3D` request, if any.
    pub fn take_update_player_3d(&mut self) -> bool {
        std::mem::take(&mut self.other.jip14.update_player_3d)
    }

    /// Currently attached named extra cameras (`AttachExtraCamera` / `ProjectExtraCamera`),
    /// keyed by ASCII lowercase camera name.
    pub fn extra_cameras(&self) -> &BTreeMap<String, ExtraCamera> {
        &self.other.jip14.extra_cameras
    }

    /// Monotonic revision of [`Game::extra_cameras`].
    pub fn extra_camera_revision(&self) -> u64 {
        self.other.jip14.extra_camera_revision
    }

    /// Session token for this `Game` instance's extra-camera table.
    pub fn extra_camera_session(&self) -> Arc<()> {
        Arc::clone(&self.other.jip14.extra_camera_session)
    }

    /// Detach all extra cameras and bump the revision (used on cell/session teardown).
    pub fn clear_extra_cameras(&mut self) {
        if !self.other.jip14.extra_cameras.is_empty() {
            self.other.jip14.extra_cameras.clear();
            self.other.jip14.extra_camera_revision = self.other.jip14.extra_camera_revision.wrapping_add(1);
        }
    }
}

/// Sanitize an animation sequence frequency override: non-finite values (`NaN`,
/// `±inf`) are rejected (`None`), and negative values clamp to `0.0` (paused) [design].
pub fn sanitize_anim_frequency(f: f32) -> Option<f32> {
    if !f.is_finite() {
        return None;
    }
    Some(if f <= 0.0 { 0.0 } else { f })
}

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

fn list(g: &mut Game, items: Vec<XVal>) -> XVal {
    if items.is_empty() {
        return XVal::Array(0);
    }
    let entries = items.into_iter().enumerate().map(|(i, v)| (XKey::Num(i as f64), v)).collect();
    XVal::Array(g.ext.new_array(ArrayKind::List, entries))
}

const MAGIC_ITEMS: [&[u8; 4]; 4] = [b"ALCH", b"ENCH", b"SPEL", b"INGR"];

/// An entry of the scripts' list of a magic item's effects ([`Game::effect_slot`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectSlot {
    /// The record's entry at this record index.
    Own(usize),
    /// AddNewEffect's entry: appended (`back`) or at the top, `at` in the order added.
    Added { back: bool, at: usize },
}

impl<'a> Game<'a> {
    /// A magic item's effects as scripts list them: AddNewEffect's at the top, newest
    /// first, the record's RemoveNthEffect left, AddNewEffect's appended. Every
    /// Nth-effect function indexes this list (`EffectSlot`, #2016).
    pub fn effect_list(&self, item: FormId) -> std::borrow::Cow<'a, [EffectItem]> {
        let data: &'a crate::data::GameData = self.data;
        let own = data.effect_items.get(&self.unclone(item)).map(Vec::as_slice).unwrap_or(&[]);
        let added = self.other.jip14.added_effects.get(&item);
        if added.is_none() && !self.other.jip11.removed_effects.contains_key(&item) {
            return std::borrow::Cow::Borrowed(own);
        }
        let kept = self.effect_indices(item).into_iter().map(|i| own[i].clone());
        match added {
            None => std::borrow::Cow::Owned(kept.collect()),
            Some((front, back)) => std::borrow::Cow::Owned(front.iter().rev().cloned().chain(kept).chain(back.iter().cloned()).collect()),
        }
    }

    /// A magic item's effects as they are applied: the record's at their own places (the
    /// active effects keep a record index), then AddNewEffect's [decision: the order the
    /// effects take hold in is not the list's].
    pub fn applied_effects(&self, item: FormId) -> std::borrow::Cow<'a, [EffectItem]> {
        let data: &'a crate::data::GameData = self.data;
        let own = data.effect_items.get(&self.unclone(item)).map(Vec::as_slice).unwrap_or(&[]);
        match self.other.jip14.added_effects.get(&item) {
            None => std::borrow::Cow::Borrowed(own),
            Some((front, back)) => std::borrow::Cow::Owned(own.iter().chain(front.iter().rev()).chain(back).cloned().collect()),
        }
    }

    /// Snapshot `source`'s session magic-item effect list edits (`AddNewEffect`,
    /// `RemoveNthEffect`, `SetNthEffectBase`, `SetNthEffectTraitNumeric`) onto its
    /// `TempCloneForm` `clone` so pre-clone edits carry over while post-clone edits stay
    /// isolated [observed: `w6d2_06_magic_effects_tempclone_isolation`; design].
    pub(crate) fn snapshot_magic_effects(&mut self, source: FormId, clone: FormId) {
        if let Some(added) = self.other.jip14.added_effects.get(&source).cloned() {
            self.other.jip14.added_effects.insert(clone, added);
        }
        if let Some(removed) = self.other.jip11.removed_effects.get(&source).cloned() {
            self.other.jip11.removed_effects.insert(clone, removed);
        }
        let bases: Vec<(usize, FormId)> = self.gaps.effect_bases.iter().filter(|(&(f, _), _)| f == source).map(|(&(_, r), &b)| (r, b)).collect();
        for (r, b) in bases {
            self.gaps.effect_bases.insert((clone, r), b);
        }
        let traits: Vec<(usize, usize, f64)> = self.effect_overrides.iter().filter(|(&(f, _, _), _)| f == source).map(|(&(_, r, t), &v)| (r, t, v)).collect();
        for (r, t, v) in traits {
            self.effect_overrides.insert((clone, r, t), v);
        }
    }

    /// The number of entries in the scripts' list of a magic item's effects
    /// ([`Game::effect_list`]): GetNumEffects, and AddNewEffect's and RemoveNthEffect's
    /// returned counts.
    pub fn effect_count(&self, item: FormId) -> usize {
        let (front, back) = self.other.jip14.added_effects.get(&item).map_or((0, 0), |(f, b)| (f.len(), b.len()));
        front + self.effect_indices(item).len() + back
    }

    /// Where the scripts' entry `i` of a magic item's list ([`Game::effect_list`]) lives;
    /// `None` past the end. One index space for GetNumEffects, Get/SetNthEffectBase,
    /// Get/SetNthEffectTraitNumeric, RemoveNthEffect and AddNewEffect's count
    /// [decision: #2016].
    pub fn effect_slot(&self, item: FormId, i: usize) -> Option<EffectSlot> {
        let own = self.effect_indices(item);
        let (front, back) = self.other.jip14.added_effects.get(&item).map_or((0, 0), |(f, b)| (f.len(), b.len()));
        if i < front {
            // Kept oldest first: the newest is at the top.
            Some(EffectSlot::Added { back: false, at: front - 1 - i })
        } else if i < front + own.len() {
            Some(EffectSlot::Own(own[i - front]))
        } else {
            (i - front - own.len() < back).then(|| EffectSlot::Added { back: true, at: i - front - own.len() })
        }
    }

    /// AddNewEffect's entry at a slot, to read or change.
    pub fn added_effect_mut(&mut self, item: FormId, back: bool, at: usize) -> Option<&mut EffectItem> {
        self.other.jip14.added_effects.get_mut(&item).and_then(|(f, b)| if back { b.get_mut(at) } else { f.get_mut(at) })
    }

    /// AddNewEffect's entry at a slot.
    pub fn added_effect(&self, item: FormId, back: bool, at: usize) -> Option<&EffectItem> {
        self.other.jip14.added_effects.get(&item).and_then(|(f, b)| if back { b.get(at) } else { f.get(at) })
    }

    /// Script-visible `[magnitude, area, duration]` trait for an `AddNewEffect` slot,
    /// reading signed `SetNthEffectTraitNumeric` overrides before falling back to `EffectItem`.
    pub fn added_effect_trait(&self, item: FormId, back: bool, at: usize, trait_idx: usize) -> Option<f64> {
        let e = self.added_effect(item, back, at)?;
        if let Some(ov) = self.other.jip14.added_effect_traits.get(&(item, back, at)) {
            return ov.get(trait_idx).copied();
        }
        match trait_idx {
            0 => Some(f64::from(e.magnitude)),
            1 => Some(f64::from(e.area)),
            2 => Some(f64::from(e.duration)),
            _ => None,
        }
    }

    /// Store a `SetNthEffectTraitNumeric` value on an `AddNewEffect` slot, keeping signed
    /// script-visible storage alongside the clamped `u32` `EffectItem` field.
    pub fn set_added_effect_trait(&mut self, item: FormId, back: bool, at: usize, trait_idx: usize, v: f64) {
        if trait_idx >= 3 {
            return;
        }
        let Some(e) = self.added_effect_mut(item, back, at) else { return };
        let base = [f64::from(e.magnitude), f64::from(e.area), f64::from(e.duration)];
        match trait_idx {
            0 => e.magnitude = v.max(0.0) as u32,
            1 => e.area = v.max(0.0) as u32,
            _ => e.duration = v.max(0.0) as u32,
        }
        let entry = self.other.jip14.added_effect_traits.entry((item, back, at)).or_insert(base);
        entry[trait_idx] = v;
    }

    /// RemoveNthEffect on one of AddNewEffect's entries: it leaves the list.
    pub fn remove_added_effect(&mut self, item: FormId, back: bool, at: usize) {
        if let Some((f, b)) = self.other.jip14.added_effects.get_mut(&item) {
            let v = if back { b } else { f };
            if at < v.len() {
                v.remove(at);
                self.other.jip14.added_effect_traits.remove(&(item, back, at));
                let shifted: Vec<(usize, [f64; 3])> = self
                    .other
                    .jip14
                    .added_effect_traits
                    .iter()
                    .filter(|(&(i, bk, idx), _)| i == item && bk == back && idx > at)
                    .map(|(&(_, _, idx), &vals)| (idx, vals))
                    .collect();
                for (idx, _) in &shifted {
                    self.other.jip14.added_effect_traits.remove(&(item, back, *idx));
                }
                for (idx, vals) in shifted {
                    self.other.jip14.added_effect_traits.insert((item, back, idx - 1), vals);
                }
            }
        }
    }

    /// A reference's primitive as the scripts left it, if they gave or changed one:
    /// whether a sphere and its half extents.
    pub fn script_primitive(&self, r: FormId) -> Option<(bool, [f32; 3])> {
        self.other.jip14.primitives.get(&r).filter(|p| p.shape != 3).map(|p| (p.shape == 2, p.bounds))
    }

    /// SetRefrModelPath's model of a reference (under meshes): `Some(None)` its base's.
    pub fn ref_model(&self, r: FormId) -> Option<Option<&str>> {
        self.other.jip14.ref_models.get(&r).map(|m| m.as_deref())
    }

    /// The references SetRefrModelPath changed.
    pub fn ref_models(&self) -> &BTreeMap<FormId, Option<String>> {
        &self.other.jip14.ref_models
    }

    /// Whether VATS may mark the actor (SetVATSTargetable).
    pub fn actor_vats_targetable(&self, r: FormId) -> bool {
        !self.other.jip14.vats_off.contains(&r)
    }

    /// The engine's VATS targets while VATS is up (empty when it is not).
    pub fn set_vats_targets(&mut self, targets: Vec<FormId>) {
        self.other.jip14.vats_targets = targets;
    }

    /// Whether SetNoGunWobble switched the actor's gun wobble off.
    pub fn gun_wobble_off(&self, r: FormId) -> bool {
        self.other.jip14.no_wobble.contains(&r)
    }

    /// A projectile's fuse as the engine has it (seconds left; `None`: no fuse).
    pub fn set_fuse(&mut self, r: FormId, left: Option<f32>) {
        match left {
            Some(s) => self.other.jip14.fuses.insert(r, s),
            None => self.other.jip14.fuses.remove(&r),
        };
    }

    /// SetDetonationTimer's fuses, for the engine.
    pub fn take_fuse_sets(&mut self) -> Vec<(FormId, f32)> {
        std::mem::take(&mut self.other.jip14.fuse_sets)
    }

    /// The engine's word on a drawn object's sequences (`None`: no longer drawn).
    pub fn set_sequences(&mut self, r: FormId, names: Option<Vec<String>>) {
        match names {
            Some(n) => {
                self.other.jip14.sequences.insert(r, n.iter().map(|s| s.to_ascii_lowercase()).collect());
            }
            None => {
                self.other.jip14.sequences.remove(&r);
            }
        }
    }

    /// Whether any `SetAnimSequenceFrequency` overrides are active (zero-allocation
    /// fast path for engine consumers).
    pub fn has_anim_frequencies(&self) -> bool {
        !self.other.jip14.anim_freq.is_empty()
    }

    /// `SetAnimSequenceFrequency`'s sanitized frequency for a reference's sequence
    /// (case-insensitive sequence name or `.kf` path), falling back to `"*"` for the
    /// same reference; `None` when unset or non-finite. Negative values clamp to `0.0`
    /// [documented: GECK wiki SetAnimSequenceFrequency for `"*"`; hypothesis: exact
    /// sequence takes precedence over `"*"`; design: non-finite rejection and `< 0`
    /// clamp to `0.0`].
    pub fn anim_frequency(&self, r: FormId, sequence: &str) -> Option<f32> {
        let j = &self.other.jip14.anim_freq;
        if j.is_empty() {
            return None;
        }
        let mut wildcard = None;
        for ((id, key), &freq) in j.range((r, String::new())..) {
            if *id != r {
                break;
            }
            if crate::nvse_anim::same(key, sequence) {
                if let Some(clean) = sanitize_anim_frequency(freq) {
                    return Some(clean);
                }
            } else if key == "*" {
                wildcard = sanitize_anim_frequency(freq);
            }
        }
        wildcard
    }
}

pub(crate) fn call(g: &mut Game, _ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let zero = XVal::Num(0.0);
    let v = match name {
        // ---- magic items ----
        // A new entry at the top, or appended (1), of an ingestible's, object effect's or
        // actor effect's list; returns the 0-based inserted index (`n0` = `n1 - 1`) when
        // appended, the new effect count (`n1`) when prepended (the entry itself is still
        // placed at index 0), or -1 when not added [documented; observed: Fallout: New
        // Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-24, GSDocMitchellHouse
        // (0x00103DF9), 10 official base masters, `jip_48_addneweffect_duplicate_return`
        // and `w4d_16_add_new_effect_edges` (`tools/capture/nvse/catalogs/wave4d_extenders.json`,
        // #3679): n0=4 -> prepend ret=5, n=5, e0=00066EB8, e1=0000014E; duplicate append
        // ret=5, n=6, last=00066EB8; non-MGEF ret=-1, n=6; no DLL or source consulted];
        // for the session (see docs/spec-notes/jip-last-14.md §1).
        "addneweffect" => {
            let (Some(item), Some(mgef), Some(dur), Some(mag)) = (form(args, 0), form(args, 1), num(args, 2), num(args, 3)) else { return Some(Ok(XVal::Num(-1.0))) };
            let kind_ok = g.data.sig(g.unclone(item)).is_some_and(|s| MAGIC_ITEMS.contains(&&s));
            if !kind_ok || g.data.sig(g.unclone(mgef)) != Some(*b"MGEF") {
                return Some(Ok(XVal::Num(-1.0)));
            }
            // [decision: the range is not kept, as NVForever's effect entries have none]
            let e = EffectItem { effect: mgef, magnitude: mag.max(0.0) as u32, area: num(args, 5).unwrap_or(0.0).max(0.0) as u32, duration: dur.max(0.0) as u32, actor_value: None, conditions: Vec::new() };
            let append = num(args, 6).unwrap_or(0.0) != 0.0;
            let entry = g.other.jip14.added_effects.entry(item).or_default();
            if append {
                entry.1.push(e);
                XVal::Num(g.effect_count(item).saturating_sub(1) as f64)
            } else {
                entry.0.push(e);
                XVal::Num(g.effect_count(item) as f64)
            }
        }
        // Any effect of the list's items on the actor [documented], as IsSpellTarget.
        "isspelltargetlist" => {
            let (Some(r), Some(l)) = (subject, form(args, 0)) else { return Some(Ok(zero)) };
            let items = crate::functions::list_members(g, l);
            g.sync_actor_effects(r);
            yes(g.effects.iter().any(|e| e.target == r && items.contains(&e.source)))
        }
        // ---- primitives ----
        // On an activator, a box or sphere trigger; on a CollisionMarker, a collision box,
        // sphere or plane [documented]. NVForever's triggers take it (`nvf_play::
        // script_trigger`); a CollisionMarker's is kept [decision: its collider is not
        // built, a follow-up].
        "addprimitive" => {
            let (Some(r), Some(t), Some(x), Some(y), Some(z)) = (subject, num(args, 0), num(args, 1), num(args, 2), num(args, 3)) else { return Some(Ok(zero)) };
            let shape = t as u8;
            let base = crate::functions::base_of(g, r);
            let acti = base.and_then(|b| g.data.sig(b)) == Some(*b"ACTI");
            let marker = base.and_then(|b| g.data.edid_of.get(&b)).is_some_and(|e| e.eq_ignore_ascii_case("CollisionMarker"));
            if !(1..=3).contains(&shape) || !(acti && shape != 3 || marker) {
                return Some(Ok(zero));
            }
            g.other.jip14.primitives.insert(r, ScriptPrimitive { shape, bounds: [x as f32, y as f32, z as f32] });
            yes(true)
        }
        "hasprimitive" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            yes(g.other.jip14.primitives.contains_key(&r) || g.world.placed.get(r).is_some_and(|p| p.primitive.is_some()))
        }
        // The bound on the axis [documented]; half extents as the record keeps them
        // [hypothesis: the page does not say half or whole].
        // -1 for a reference with no primitive [observed: jip14 primitivebound.doc].
        "getprimitivebound" | "setprimitivebound" => {
            let (Some(r), Some(a)) = (subject, axis(args, 0)) else { return Some(Ok(zero)) };
            let own = g.world.placed.get(r).and_then(|p| p.primitive).map(|p| ScriptPrimitive { shape: if p.sphere { 2 } else { 1 }, bounds: p.bounds });
            let none = if name == "getprimitivebound" { -1.0 } else { 0.0 };
            let Some(mut p) = g.other.jip14.primitives.get(&r).copied().or(own) else { return Some(Ok(XVal::Num(none))) };
            if name == "setprimitivebound" {
                let Some(v) = num(args, 1) else { return Some(Ok(zero)) };
                p.bounds[a] = v as f32;
                g.other.jip14.primitives.insert(r, p);
            }
            XVal::Num(f64::from(p.bounds[a]))
        }
        // ---- the auxiliary-variable condition ----
        // The float at index 0 of the calling reference's auxiliary variable named by the
        // quest's string variable at `index` [documented]; 0 otherwise [documented]. A
        // name with no owning mod's prefix [decision: a condition has no calling script
        // to take a private name's mod from].
        "auxvargetfltcond" => {
            let (Some(r), Some(q), Some(i)) = (subject, form(args, 0), num(args, 1)) else { return Some(Ok(zero)) };
            let v = g.var(crate::game::InstKey::Quest { quest: q }, i as u16);
            let Some(aux) = g.ext.text(v.num() as u32).map(str::to_string) else { return Some(Ok(zero)) };
            let store = if aux.starts_with('*') { &g.ext.jip_temp } else { &g.ext.jip };
            match store.aux.get(&r.0).and_then(|m| m.get(&aux.to_ascii_lowercase())).and_then(|v| v.first()) {
                Some(XVal::Num(x)) => XVal::Num(*x),
                _ => zero,
            }
        }
        // ---- actors ----
        // Severed, or exploded (1), a limb 0-13 [documented]; the engine's dismemberment
        // (`nvf_play::dismember`).
        "dismemberlimb" => {
            let (Some(r), Some(part)) = (subject, num(args, 0)) else { return Some(Ok(zero)) };
            if (0.0..14.0).contains(&part) {
                g.combat_requests.push(crate::combat::CombatRequest::Dismember { actor: r, location: part as i32, by: None, explode: Some(num(args, 1).unwrap_or(0.0) != 0.0) });
            }
            zero
        }
        // FireWeapon's shot, from the node when its place is known [documented].
        "fireweaponex" => {
            let (Some(r), Some(w)) = (subject, form(args, 0)) else { return Some(Ok(zero)) };
            let muzzle = text(args, 1).filter(|n| !n.is_empty()).and_then(|n| node_place(g, r, &n, true)).map(|(p, _)| p);
            g.combat_requests.push(crate::combat::CombatRequest::Fire { from: r, weapon: w, muzzle });
            zero
        }
        // A parent idle: the first child whose conditions pass; a child: played whatever
        // its conditions; none: PickIdle [documented].
        "playidleex" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            match form(args, 0) {
                None => {
                    if !g.idle_picks.contains(&r) {
                        g.idle_picks.push(r);
                    }
                }
                Some(idle) => {
                    let pick = if g.data.idle_children.contains_key(&Some(idle)) { g.first_idle_leaf(r, idle) } else { Some(idle) };
                    if let Some(i) = pick {
                        g.ask_idle(r, i, crate::idles::IdleSource::Script);
                    }
                }
            }
            zero
        }
        // At the node's place in the reference's 3D [documented]; at the reference while
        // its 3D is not known yet (see docs/spec-notes/jip-last-14.md §3).
        "playsound3dnode" => {
            let (Some(r), Some(s), Some(node)) = (subject, form(args, 0), text(args, 1)) else { return Some(Ok(zero)) };
            let point = node_place(g, r, &node, true).map(|(p, _)| p);
            g.ask_sound(crate::sound::SoundRequest { sound: s, at: Some(r), system: false, id: 0, point });
            zero
        }
        // Whether the actor wobbles its gun [documented: ToggleNoGunWobble, alias
        // SetNoGunWobble, "0/1"]: 1, or no argument as the page's example, no wobble; 0
        // the wobble again (see docs/spec-notes/jip-last-14.md §3). The catalogue's
        // parameter has no name and does not reach here yet (a follow-up): every call
        // switches the wobble off.
        "setnogunwobble" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            if num(args, 0).unwrap_or(1.0) != 0.0 {
                g.other.jip14.no_wobble.insert(r);
            } else {
                g.other.jip14.no_wobble.remove(&r);
            }
            zero
        }
        // ---- models and textures ----
        // The reference's own model (under meshes), the base's without one [documented];
        // drawn when its 3D is made again [documented: "followed by a call to Update3D";
        // NVForever's engine draws it at the next drawing of the reference].
        "setrefrmodelpath" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            let m = text(args, 0).filter(|m| !m.is_empty()).map(|m| {
                let m = m.replace('/', "\\");
                match m.to_ascii_lowercase().strip_prefix("meshes\\") {
                    Some(_) => m[7..].to_string(),
                    None => m,
                }
            });
            g.other.jip14.ref_models.insert(r, m);
            zero
        }
        // SwapTexture with a slot [documented]: slot 0, the diffuse map, is SwapTexture's;
        // the others are not drawn [decision: NVForever's texture swaps change the diffuse
        // map only].
        "swaptextureex" => {
            let (Some(on), Some(block), Some(path), Some(slot)) = (form(args, 0), text(args, 1), text(args, 2), num(args, 3)) else { return Some(Ok(zero)) };
            if slot == 0.0 {
                let (shape, texture) = crate::functions::misc::texture_swap_names(&block, &path);
                g.texture_swaps.swap(on, shape, texture);
            }
            zero
        }
        // [decision: NVForever's scene mirror keeps no texture sets: no array, the page's
        // answer for a block without one]
        "getblocktextureset" => XVal::Array(0),
        // ---- VATS ----
        "getvatstargets" => {
            let t: Vec<XVal> = g.other.jip14.vats_targets.iter().map(|r| XVal::Ref(r.0)).collect();
            list(g, t)
        }
        "setvatstargetable" => {
            let (Some(r), Some(t)) = (subject, num(args, 0)) else { return Some(Ok(zero)) };
            if t != 0.0 {
                g.other.jip14.vats_off.remove(&r);
            } else {
                g.other.jip14.vats_off.insert(r);
            }
            zero
        }
        // ---- projectiles ----
        // Seconds to the detonation; -1 for a projectile without a fuse [documented]; a
        // mine not triggered yet FLOAT_MAX [documented].
        "getdetonationtimer" => {
            let Some(r) = subject.filter(|r| g.gaps.projs.shots.contains_key(r)) else { return Some(Ok(XVal::Num(-1.0))) };
            XVal::Num(g.other.jip14.fuses.get(&r).map_or(-1.0, |&s| f64::from(s)))
        }
        "setdetonationtimer" => {
            let (Some(r), Some(t)) = (subject, num(args, 0)) else { return Some(Ok(zero)) };
            if g.gaps.projs.shots.contains_key(&r) {
                g.other.jip14.fuse_sets.push((r, t.max(0.0) as f32));
            }
            zero
        }
        // The weapon that fired the live projectile [documented]; the shot's record, read
        // by the projectile getters and at the impact (see docs/spec-notes/jip-last-14.md §5).
        // Returns the weapon.
        "setprojectilerefweapon" => {
            let (Some(r), Some(w)) = (subject, form(args, 0)) else { return Some(Ok(XVal::Ref(0))) };
            match g.gaps.projs.shots.get_mut(&r) {
                Some(s) if g.data.sig(w) == Some(*b"WEAP") => {
                    s.weapon = w;
                    XVal::Ref(w.0)
                }
                _ => XVal::Ref(0),
            }
        }
        // ---- inventory references ----
        // From the script of an item in a container: the item as an inventory reference
        // [documented]; 0 elsewhere.
        // Outside an item's script the call fails [observed: jip14 selfinvref, "script
        // call failed"].
        "getselfasinventoryref" => match g.other.jip14.item_running {
            Some((holder, item)) => XVal::Ref(g.temp_ref(holder, item).0),
            // Worded as the VM's NoReference, which stops the script (as `nvse_jg5`).
            None => return Some(Err(fnv_script::vm::VmError::NoReference(name.to_string()).to_string())),
        },
        // ---- animation sequences ----
        // The frequency a script set; else 1 for a sequence of the drawn model (see
        // docs/spec-notes/jip-last-14.md §4); -1 for a sequence the
        // model lacks [observed: jip14 seqfreq.doc]. Drawn object sequences are played at
        // this frequency by `nvforever::object_anims` (actor `.kf` paths remain stored
        // only [design]).
        // [decision: only drawn objects' sequences are told by the engine; an actor's
        // animation files are not, so an actor's unset sequence is -1]
        // "*" is a setter-only wildcard and is not a queryable sequence name: querying "*"
        // returns -1.0 even after `SetAnimSequenceFrequency "*" 1.5` [observed: Fallout: New
        // Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-24, `GSDocMitchellHouse`
        // (`0x00103DF9`), 10 official base masters, `w4c_b1_22_sound3d_anim_freq`].
        "getanimsequencefrequency" | "getanimseqfreq" => {
            let (Some(r), Some(s)) = (subject, text(args, 0)) else { return Some(Ok(zero)) };
            if s == "*" {
                return Some(Ok(XVal::Num(-1.0)));
            }
            let lower = s.to_ascii_lowercase();
            let has = g.other.jip14.sequences.get(&r).is_some_and(|n| n.contains(&lower));
            XVal::Num(match g.anim_frequency(r, &s) {
                Some(f) => f64::from(f),
                None if has => 1.0,
                None => -1.0,
            })
        }
        // "*" every sequence [documented: GECK wiki SetAnimSequenceFrequency]; non-finite
        // values are ignored and negative values clamp to 0.0 (paused) [design].
        "setanimsequencefrequency" | "setanimseqfreq" => {
            let (Some(r), Some(s), Some(f)) = (subject, text(args, 0), num(args, 1)) else { return Some(Ok(zero)) };
            let Some(freq) = sanitize_anim_frequency(f.max(0.0) as f32) else { return Some(Ok(zero)) };
            g.flush_script_sequence_clocks(r);
            g.other.jip14.anim_freq.insert((r, s.to_ascii_lowercase()), freq);
            zero
        }
        // ---- extra cameras (AttachExtraCamera / ProjectExtraCamera) ----
        // [documented: GECK wiki AttachExtraCamera, ProjectExtraCamera; design]
        "attachextracamera" => {
            let (Some(raw_name), Some(mode)) = (text(args, 0), num(args, 1)) else { return Some(Ok(zero)) };
            let cam_name = raw_name.trim();
            if cam_name.is_empty() {
                return Some(Ok(zero));
            }
            let key = cam_name.to_ascii_lowercase();
            if mode == 0.0 {
                let removed = g.other.jip14.extra_cameras.remove(&key).is_some();
                if removed {
                    g.other.jip14.extra_camera_revision = g.other.jip14.extra_camera_revision.wrapping_add(1);
                }
                return Some(Ok(yes(removed)));
            }
            let Some(r) = subject.filter(|&r| valid_camera_ref(g, r)) else { return Some(Ok(zero)) };
            let exists = g.other.jip14.extra_cameras.contains_key(&key);
            if !exists && g.other.jip14.extra_cameras.len() >= EXTRA_CAMERA_MAX {
                return Some(Ok(zero));
            }
            let parent_node = text(args, 2).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
            g.other.jip14.extra_camera_revision = g.other.jip14.extra_camera_revision.wrapping_add(1);
            let rev = g.other.jip14.extra_camera_revision;
            match g.other.jip14.extra_cameras.get_mut(&key) {
                Some(cam) => {
                    cam.name = cam_name.to_string();
                    cam.reference = r;
                    cam.parent_node = parent_node;
                    cam.revision = rev;
                }
                None => {
                    g.other.jip14.extra_cameras.insert(
                        key,
                        ExtraCamera {
                            name: cam_name.to_string(),
                            reference: r,
                            parent_node,
                            projection: None,
                            revision: rev,
                        },
                    );
                }
            }
            yes(true)
        }
        "projectextracamera" => {
            let (Some(raw_name), Some(raw_target), Some(raw_fov)) = (text(args, 0), text(args, 1), num(args, 2)) else {
                return Some(Ok(zero));
            };
            let cam_name = raw_name.trim();
            let target_str = raw_target.trim();
            if cam_name.is_empty() || target_str.is_empty() || raw_fov <= 0.0 {
                return Some(Ok(zero));
            }
            let key = cam_name.to_ascii_lowercase();
            let Some(cam_ref) = g.other.jip14.extra_cameras.get(&key).map(|c| c.reference) else {
                return Some(Ok(zero));
            };
            if !camera_in_player_area(g, cam_ref) {
                return Some(Ok(zero));
            }
            let target = if let Some(tile) = target_str.strip_prefix('*') {
                let tile = tile.trim();
                if tile.is_empty() {
                    return Some(Ok(zero));
                }
                ExtraCameraTarget::UiTile { path: tile.to_string() }
            } else {
                let Some(r) = subject.filter(|&r| valid_camera_ref(g, r)) else { return Some(Ok(zero)) };
                ExtraCameraTarget::Geometry { reference: r, node: target_str.to_string() }
            };
            let fov_deg = (raw_fov as f32).clamp(EXTRA_CAMERA_MIN_FOV_DEG, EXTRA_CAMERA_MAX_FOV_DEG);
            let pixel_size = match num(args, 3) {
                Some(px) if px > 0.0 => (px.round() as u32).clamp(EXTRA_CAMERA_MIN_PX, EXTRA_CAMERA_MAX_PX),
                _ => EXTRA_CAMERA_DEFAULT_PX,
            };
            let grayscale = num(args, 4).is_some_and(|v| v != 0.0);
            g.other.jip14.extra_camera_revision = g.other.jip14.extra_camera_revision.wrapping_add(1);
            let rev = g.other.jip14.extra_camera_revision;
            if let Some(cam) = g.other.jip14.extra_cameras.get_mut(&key) {
                cam.projection = Some(ExtraCameraProjection {
                    target,
                    fov_deg,
                    pixel_size,
                    grayscale,
                    sequence: rev,
                });
                cam.revision = rev;
            }
            yes(true)
        }
        // ---- nothing values by decision ----
        // [decision: line meshes are not built; 0, the page's failure]
        "attachline" => zero,
        // Condition-compatible variant of HasKeyword: accepts `ref.HasKeywordCond
        // "keyword"` or `HasKeywordCond form "keyword"` over `Game::has_keyword`
        // [documented: GECK wiki HasKeywordCond, HasKeyword; hypothesis: both calling
        // conventions share the session keyword table].
        "haskeywordcond" => {
            let (target, keyword) = match (args.first(), args.get(1)) {
                (Some(XVal::Str(k)), None) => (subject, Some(k.as_str())),
                (_, Some(XVal::Str(k))) => (form(args, 0).or(subject), Some(k.as_str())),
                _ => (None, None),
            };
            yes(target.zip(keyword).is_some_and(|(f, k)| g.has_keyword(f, k)))
        }
        // [decision: NVForever does not crash the game on purpose; the page: "don't use
        // this"]
        "freeplayer" | "goodbyeworldspace" => zero,
        // Queues a full player 3D rig, equipped-model and animation-override reload for
        // the next engine frame [documented: UpdatePlayer3D; design].
        "updateplayer3d" => {
            g.other.jip14.update_player_3d = true;
            g.refresh_anim_folders_for(FormId::PLAYER_REF);
            zero
        }
        _ => return None,
    };
    Some(Ok(v))
}

fn valid_camera_ref(g: &Game, r: FormId) -> bool {
    r == FormId::PLAYER_REF || g.world.peek(r).is_some() || g.world.placed.get(r).is_some()
}

/// Whether `cam_ref` resides in the currently loaded cell grid or shares the player's
/// interior cell [documented: GECK wiki ProjectExtraCamera].
fn camera_in_player_area(g: &Game, cam_ref: FormId) -> bool {
    if cam_ref == FormId::PLAYER_REF {
        return true;
    }
    let cell_of = |id: FormId| {
        g.world
            .peek(id)
            .and_then(|s| s.cell)
            .or_else(|| g.world.placed.get(id).and_then(|p| p.cell))
    };
    let Some(cam_cell) = cell_of(cam_ref) else {
        return true;
    };
    if !g.loaded_cells.is_empty() {
        return g.loaded_cells.contains(&cam_cell);
    }
    let Some(player_cell) = cell_of(FormId::PLAYER_REF) else {
        return true;
    };
    if player_cell == cam_cell {
        return true;
    }
    match (g.world.placed.world_of(player_cell), g.world.placed.world_of(cam_cell)) {
        (Some(pw), Some(cw)) => pw == cw,
        _ => false,
    }
}
