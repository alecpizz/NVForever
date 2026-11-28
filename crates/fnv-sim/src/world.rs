//! Save-agnostic runtime state: references as they are now, stored as a delta over the
//! authored placement data.
//!
//! A reference that nothing has touched has no entry: its state is derived from its
//! `PlacedRef` when first needed ("materialised"). Only materialised references are
//! written to a [`Snapshot`], so a snapshot is the set of changes against the plugins,
//! independent of the original game's savegame format (spec Layer 4 "save-agnostic
//! game state"; savegame compatibility is Layer 7).
//!
//! Everything is deterministic: ordered maps, and one seeded RNG whose position is part
//! of the snapshot, so a run continued from a snapshot equals an uninterrupted run.

use rand_chacha::ChaCha8Rng;
use rand_core::SeedableRng;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::clock::{self, Calendar};
use crate::form::FormId;
use crate::forms::{BaseData, FormStore, ItemKind, LeveledEntry, SPECIAL};
use crate::inventory::{unequipped, Extra, Inventory, Pick};
use crate::leveled::{self, LeveledError};
use crate::refs::{PlacedRef, RefTable};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SimConfig {
    pub seed: u64,
    /// The configured level: what item rolls read until [`World::set_player_level`], and
    /// what a leveled lock with no fixed level reads ([`World::lock_read_level`]); 1 in
    /// play [design]. [observed in game: `AddItem` of a list resolves against the player's
    /// level (`L4.sim.leveled.basic`); that containers do too is hypothesis]
    pub player_level: u16,
    /// Hardcore mode: ammunition has weight. (Absent in older snapshots.)
    #[serde(default)]
    pub hardcore: bool,
    /// Game hours a cell must stay unloaded before its respawning containers refill.
    /// `None`: the `iHoursToRespawnCell` game setting when a plugin defines it, else
    /// [`DEFAULT_RESPAWN_HOURS`] (see [`World::respawn_hours`]).
    #[serde(default)]
    pub respawn_hours: Option<f32>,
}

/// Respawn delay when neither the config nor a plugin sets one: the engine's built-in
/// value of `iHoursToRespawnCell` [observed in game: `GetGameSetting
/// iHoursToRespawnCell` = 72.0; no master defines the setting]. Merchant containers
/// restock on a schedule of their own ([`is_merchant_restock_day`]).
pub const DEFAULT_RESPAWN_HOURS: f32 = 72.0;

/// The game setting that holds the cell-respawn delay [observed in game: the running
/// game answers `GetGameSetting iHoursToRespawnCell`].
pub const RESPAWN_SETTING: &str = "iHoursToRespawnCell";

/// Whether merchant containers restock at the midnight that starts GameDaysPassed `day`:
/// days that are 0 or 3 modulo 7, i.e. twice a week, 3 and 4 days apart.
/// [observed in game, Ringo's cash box 0x001629C0 in two saves
/// (docs/spec-notes/simulation.md §3): refills only ever came just after midnights 7, 10,
/// 14, 17, 21, 24 and 28, and an emptied box was passed over at midnights 8, 9, 13, 15,
/// 16, 18, 19, 20, 22, 23, 25, 26 and 27 — every other residue — whenever it had been
/// emptied and whatever its last restock. That the schedule is global (GameDaysPassed
/// modulo 7, the same for every merchant) rather than per container is hypothesis: both
/// saves agree, other merchants are not captured]
pub fn is_merchant_restock_day(day: i64) -> bool {
    matches!(day.rem_euclid(7), 0 | 3)
}

impl Default for SimConfig {
    fn default() -> Self {
        SimConfig { seed: 0, player_level: 1, hardcore: false, respawn_hours: None }
    }
}

/// How far in front of the holder, along its heading, a dropped item is placed (game
/// units). [observed: one capture, the player dropping stimpaks]
pub const DROP_AHEAD: f32 = 50.0;
/// How far above the holder's position (its feet) a dropped item is placed. [observed:
/// same capture]
pub const DROP_ABOVE: f32 = 30.0;
/// How far above the player's feet an item the player drops by hand (the inventory menu)
/// is placed: about chest height (the eye is 112 units up, nvf-play `EYE_HEIGHT`), from
/// where it falls [design: owner 2026-10-27, "spawn it at chest level not ground level";
/// the game's own drop is [`DROP_ABOVE`]].
pub const PLAYER_DROP_ABOVE: f32 = 85.0;
/// How many places a ring around the player holds for hand-dropped items: the first is
/// [`DROP_AHEAD`] in front, the next ones step around both sides to the back [design:
/// owner 2026-10-27, "dropping multiple items at once should ring around the player"].
pub const PLAYER_DROP_RING: usize = 8;
/// How much farther out each further ring is, once the nearer ones are full (game units).
pub const PLAYER_DROP_RING_STEP: f32 = 30.0;
/// How many rings are tried before a drop goes in the first place regardless.
pub const PLAYER_DROP_RINGS: usize = 3;
/// A place in a ring is taken while a dropped item stands within this distance of it,
/// measured across the ground (the item may have fallen since).
pub const PLAYER_DROP_CLEAR: f32 = 20.0;

/// Where [`World::drop_groups`] puts the new references.
#[derive(Debug, Clone, Copy)]
enum Placing {
    /// All of them in front of and above the holder, as the game's drop does.
    Ahead,
    /// Each in the next free place around the player, at chest height.
    Ring,
}

/// An inventory `new` rebuilt in place of `old`, with `old`'s live units (planted
/// explosives, `Extra::live`) kept: a rebuild never takes a planted explosive away from its
/// plant [design, #1640]. `None` stays `None` only when `old` had no live unit.
fn with_live(old: Option<&Inventory>, new: Option<Inventory>) -> Option<Inventory> {
    match old.filter(|o| o.has_live()) {
        None => new,
        Some(o) => {
            let mut inv = new.unwrap_or_default();
            inv.keep_live_of(o);
            Some(inv)
        }
    }
}

/// Session-only runtime overrides for an item base form (or `TempCloneForm` item alias),
/// synchronized from script setters such as `SetEquipmentSlotsMask`, `SetEquipmentBipedMask`,
/// `SetArmorClass`, `SetIsPowerArmor`, `SetIsPlayable`, `SetArmorDT`, `SetArmorArmorRating`,
/// `SetWeight`, and `SetObjectHealth`. Not serialized in [`Snapshot`].
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ItemOverride {
    pub slots: Option<u32>,
    pub armor_class: Option<crate::forms::ArmorClass>,
    pub power_armor: Option<bool>,
    pub non_playable: Option<bool>,
    pub armor_dt: Option<f32>,
    pub armor_dr: Option<f32>,
    pub weight: Option<f32>,
    pub health: Option<i32>,
}

/// The offset across the ground, from the player's position, of place `slot` around a
/// player facing `heading` (z rotation): ring `slot / PLAYER_DROP_RING`, in it straight
/// ahead first, then alternately right and left of it a step further round, the last one
/// behind.
pub fn player_drop_offset(heading: f32, slot: usize) -> [f32; 2] {
    let (ring, k) = (slot / PLAYER_DROP_RING, slot % PLAYER_DROP_RING);
    let step = std::f32::consts::TAU / PLAYER_DROP_RING as f32;
    // 0, +1, -1, +2, -2, ...: odd places to the right (clockwise), even to the left.
    let turn = if k % 2 == 1 { k.div_ceil(2) as f32 } else { -((k / 2) as f32) };
    let r = DROP_AHEAD + ring as f32 * PLAYER_DROP_RING_STEP;
    let (sin, cos) = (heading + turn * step).sin_cos();
    [r * sin, r * cos]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockState {
    pub level: u8,
    pub key: Option<FormId>,
    pub locked: bool,
}

/// A reference's run-time state.
///
/// `enabled` and `deleted` are private to this module, so that [`World::presence_serial`]
/// moves with them: read them with [`RefState::enabled`] and [`RefState::deleted`], change
/// them with [`World::set_enabled`] and [`World::set_deleted`].
///
/// ```
/// fn present(state: &fnv_sim::world::RefState) -> bool {
///     state.enabled() && !state.deleted()
/// }
/// ```
///
/// Writing either field from outside does not compile:
///
/// ```compile_fail,E0616
/// fn write(state: &mut fnv_sim::world::RefState) {
///     state.enabled = false;
/// }
/// ```
///
/// ```compile_fail,E0616
/// fn write(state: &mut fnv_sim::world::RefState) {
///     state.deleted = true;
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefState {
    pub base: FormId,
    pub cell: Option<FormId>,
    pub pos: [f32; 3],
    pub rot: [f32; 3],
    enabled: bool,
    /// Removed from the world (picked up, or a created reference deleted).
    deleted: bool,
    /// Stack size for item references; 1 otherwise.
    pub count: i32,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub inventory: Option<Inventory>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub lock: Option<LockState>,
    pub open: bool,
    /// Item references: condition (XHLP on placed weapons and armour) and owner.
    #[serde(skip_serializing_if = "Extra::is_plain", default)]
    pub extra: Extra,
    /// Actor values changed at run time, by lower-case name (S.P.E.C.I.A.L. so far).
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub actor_values: BTreeMap<String, f32>,
    /// Merchant containers: GameDaysPassed of the last restock, a restock day (`None`:
    /// not restocked since the simulation started).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub last_restock: Option<f32>,
    /// An actor other than the player has put on its outfit and taken up its weapon
    /// ([`World::outfit`]): done the first time its cell is loaded.
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub outfitted: bool,
    /// The scale a script set (SetScale), drawn and collided in place of the
    /// placement's (XSCL).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub scale: Option<f32>,
    /// Items a script locked on or off this actor (EquipItem's NoUnequipFlag,
    /// UnequipItem's NoEquipFlag), by base form ([`World::lock_equip`]).
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub equip_locks: BTreeMap<FormId, EquipLock>,
    /// A created item reference that an actor dropped as it died (its weapon): that
    /// actor. The corpse's loot menu lists it beside the body's inventory, and taking it
    /// there picks this reference up ([`World::dropped_beside`]).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub dropped_by: Option<FormId>,
    /// A created item reference the player dropped (by hand, a script's drop, a weapon
    /// lost from the player's hand): in co-op every player sees it and any one can take
    /// it [design]. Saved with the reference.
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub player_drop: bool,
    /// A reference created at run time ([`World::place`]): the cell and position it was
    /// created at, its editor location for packages and `GetStartingPos`-like questions
    /// that a placed reference answers from the plugins (#2278) [hypothesis]. Saved with
    /// the reference; a created actor of an older save without it takes its saved place
    /// on a load (`World::made_at_from_saved_place`, #3042).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub made_at: Option<(Option<FormId>, [f32; 3])>,
    /// A reference created at run time ([`World::place`]): the Euler angles (radians)
    /// it was created with, answered by `GetStartingAngle` [observed: Fallout: New Vegas
    /// 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-23, `GSDocMitchellHouse`
    /// (`0x00103DF9`), 10 official base masters, `b3_jip4_02_session_time_and_start_angle`
    /// (`tools/capture/nvse/catalogs/wave4b_extenders.json`, #3679):
    /// `PROBE|b3_jip4_02_session_time_and_start_angle|sess=5.31,docZ=22.92,dynZ=270.00`;
    /// no DLL or source consulted; hypothesis: X/Y axes and save persistence].
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub made_rot: Option<[f32; 3]>,
    /// A shot's temporary reference for scripts (`fnv_game::nvse_proj`): the engine draws
    /// the shot's flying model along its flight, so this reference is neither drawn nor
    /// given collision as a dropped item (#2186).
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub shot: bool,
}

impl RefState {
    /// The reference's own enabled flag; [`World::is_enabled`] adds its enable parents.
    #[inline]
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Removed from the world (picked up, or a created reference deleted).
    #[inline]
    pub fn deleted(&self) -> bool {
        self.deleted
    }
}

/// A script's lock on an item of an actor [documented: GECK wiki EquipItem, "If
/// NoUnequipFlag = 1, actor (including the player) will be unable to unequip the item.
/// Once the item is unequipped with the UnequipItem function, the item can be equipped
/// and unequipped as normal"; UnequipItem, the same for NoEquipFlag and EquipItem].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EquipLock {
    /// Put on with NoUnequipFlag: it stays on, and nothing else takes its slot.
    NoUnequip,
    /// Taken off with NoEquipFlag: it cannot be put on.
    NoEquip,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SimError {
    #[error("unknown reference {0}")]
    UnknownRef(FormId),
    #[error("unknown base form {0}")]
    UnknownForm(FormId),
    #[error("{0} is not a supported temporary scene base")]
    UnsupportedTemporaryBase(FormId),
    #[error("{0} has no inventory")]
    NoInventory(FormId),
    #[error("{0} is not an inventory item")]
    NotAnItem(FormId),
    #[error("count must be positive, got {0}")]
    BadCount(i32),
    #[error("{0} is not in the world (deleted)")]
    Deleted(FormId),
    #[error("{0} cannot be equipped")]
    NotEquippable(FormId),
    #[error("{actor} has no unequipped {item}")]
    NotCarried { actor: FormId, item: FormId },
    #[error("unknown actor value {0:?}")]
    UnknownActorValue(String),
    #[error("{0} has no actor values")]
    NoActorValues(FormId),
    #[error("the game setting {0} is not defined by any plugin")]
    MissingSetting(String),
    #[error("the game setting {0} holds another kind of value")]
    SettingKind(String),
    #[error("the global {0} is not defined by any plugin")]
    MissingGlobal(String),
    /// The player tried to drop a quest item from the inventory menu
    /// ([`World::player_drop_item`]) [documented: GECK wiki Category:Items and
    /// SetQuestObject: the player cannot drop or sell one].
    #[error("{0} is a quest item: the player cannot drop it")]
    QuestItem(FormId),
    /// [`World::place_as`]: the FormID is not a dynamic one, or a reference has it.
    #[error("{0} is taken, or not a dynamic FormID")]
    Taken(FormId),
    /// Every unequipped unit of the weapon is at zero condition
    /// ([`World::equip`]) [design: the owner's rule, 2026-10-11].
    #[error("{0} is broken: it cannot be equipped until it is repaired")]
    Broken(FormId),
    /// A script locked the item on or off ([`EquipLock`]): `item` is the locked one
    /// (the item itself, or the one holding its slot).
    #[error("{item} was locked by a script and cannot be {}", if *on { "unequipped" } else { "equipped" })]
    EquipLocked { item: FormId, on: bool },
    /// The player has no Power Armor Training for power armour ([`World::equip`],
    /// `World::set_power_armor_training`).
    #[error("{0} is power armour: the player lacks the training to wear it")]
    NoPowerArmorTraining(FormId),
    #[error(transparent)]
    Leveled(#[from] LeveledError),
}

/// What activating a reference did. [hypothesis for the dispatch by base type:
/// follows the GECK wiki's description of default activation per object type;
/// scripted OnActivate blocks are Layer 5 and reported in `script` only]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum Outcome {
    PickedUp { item: FormId, count: i32, owner: Option<FormId> },
    OpenedContainer { container: FormId },
    Locked { level: u8 },
    Teleported { to_door: FormId, cell: Option<FormId>, pos: [f32; 3], rot: [f32; 3] },
    /// A placed explosive made safe: `item` is the weapon it is, left where the mine lay
    /// as a loose item reference (`placed`, to be taken like any other).
    Disarmed { item: FormId, placed: Option<FormId> },
    DoorToggled { open: bool },
    Talk { actor: FormId },
    Activated,
    Disabled,
    NoDefault { sig: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Activation {
    #[serde(flatten)]
    pub outcome: Outcome,
    /// The lock was opened with this key from the actor's inventory first.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unlocked_with: Option<FormId>,
    /// The base form's script, which would receive OnActivate (Layer 5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<FormId>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Equip {
    /// Aid items and skill books are used up instead of worn.
    pub consumed: bool,
    /// Items unequipped to make room.
    pub unequipped: Vec<FormId>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Encumbrance {
    pub weight: f64,
    pub capacity: f64,
    pub over: bool,
    /// Carried items whose weight is unknown (not counted).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unknown_weight: Vec<FormId>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub format: String,
    pub config: SimConfig,
    /// ChaCha8 word position, as a decimal string (u128 does not fit JSON numbers).
    pub rng_word_pos: String,
    pub next_dynamic: u32,
    pub refs: BTreeMap<FormId, RefState>,
    /// Globals whose value differs from the plugins (absent in older snapshots).
    #[serde(default)]
    pub globals: BTreeMap<FormId, f32>,
    /// GameHour and GameDaysPassed at the clock's own precision, as decimal strings like
    /// `rng_word_pos`; absent in older snapshots, which go on from the globals
    /// ([`clock::FineClock`]). An editor that changes one of those globals in a snapshot
    /// must clear its field here: a same-value write cannot be seen from the values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clock_hour: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clock_days_passed: Option<String>,
    /// Unloaded cells: GameDaysPassed when each was detached (absent in older snapshots).
    #[serde(default)]
    pub detached_cells: BTreeMap<FormId, f32>,
    /// Quest-item flags set by scripts (`SetQuestObject`), by base form; absent when
    /// none (and in older snapshots).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub quest_objects: BTreeMap<FormId, bool>,
    /// Entries scripts added to leveled item lists (AddItemToLeveledList), by list;
    /// absent when none (and in older snapshots).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub leveled_added: BTreeMap<FormId, Vec<LeveledEntry>>,
    /// Encounter zones entered and their levels (`crate::zones`); absent when none.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub zone_levels: BTreeMap<FormId, u16>,
    /// Encounter zones whose respawning a script turned on or off (SetZoneRespawns,
    /// `crate::zones`); absent when none.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub zone_respawns: BTreeMap<FormId, bool>,
    /// The level fixed for each reference whose cell was loaded (`crate::zones`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ref_levels: BTreeMap<FormId, crate::zones::LevelAt>,
    /// Death items owed to dead actors whose inventory was not read yet
    /// ([`World::add_death_item`]); absent when none.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub death_items: BTreeMap<FormId, FormId>,
    /// References created at run time and since deleted (picked up, a shot that is over),
    /// by FormID, with their base form: the save keeps only this of them, not their whole
    /// state ([`World::deleted_base`]); absent when none (and in older snapshots, which
    /// keep the whole entries in `refs`). It still grows by one small entry (about 26
    /// bytes of JSON against about 148 for a whole entry) per created reference deleted
    /// over a playthrough: the FormIDs are never given again, and a script variable may
    /// still hold one [decision: kept rather than forgotten, #1002].
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub deleted_created: BTreeMap<FormId, FormId>,
    /// The inventory sources of references created at run time ([`World::give_items_of`]:
    /// a created leveled actor's pick, a template's Inventory source), which the game
    /// layer cannot work out again from the plugins on a load, as it does the placed
    /// actors' (`set_inventory_sources`, a resurrection's or a zone's reroll); deleted
    /// ones left out; absent when none (and in older snapshots).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub created_inventory_sources: BTreeMap<FormId, FormId>,
    /// The player may wear power armour ([`World::set_power_armor_training`]); absent
    /// when not (and in older snapshots).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub power_armor_training: bool,
    /// The game layer's carry bonuses ([`World::set_carry_bonus`]); absent when none.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub carry_bonus: BTreeMap<FormId, f64>,
    /// The game layer's weight rules ([`World::set_weight_rules`]); absent when none.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub weight_rules: BTreeMap<FormId, crate::inventory::WeightRules>,
    /// Default-open overrides set at run time ([`World::set_default_open`]); absent when none.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub default_open: BTreeMap<FormId, bool>,
}

/// What a world keeps for the session and not in a save: the edits of functions
/// documented as not save baked but lasting for the game session, which a load in the
/// same run keeps and a new run starts without ([`World::take_session`],
/// [`World::put_session`]). Game settings changed at run time [documented:
/// SetNumericGameSetting, "not save baked but rather is session based"; hypothesis: the
/// console's SetGameSetting, which changes the same setting, alike]; leveled lists edited
/// by script [documented: LU's leveled-list functions, "not savebaked"]; base containers
/// edited by script [documented: BaseAddItem, "will only persist for the duration of the
/// game session"; hypothesis: BaseRemoveItem, whose page says nothing, alike]; container
/// Respawns flag overrides (`SetContainerRespawns`) [hypothesis: session-only, like
/// `base_edits`].
///
/// "The session" is one run of the program. The runner's SaveLoad and any restore within
/// a run keep them; the engine's load while playing starts NVForever again with `--load`
/// [design, owner 2026-10-16: nothing of the run before carries over], so there they
/// start empty, as the scripts' session does (GetGameRestarted is true again in the new
/// run, so a mod that makes such edits on GetGameRestarted makes them again; saving the
/// edits as well would make it add its leveled-list entries twice) [decision].
/// Session-only encounter-zone level/flag overrides (`SetZoneMinLevel`, `SetZoneFlag`
/// bit 2 "Match PC Below Minimum Level") and zone assignment overrides
/// (`SetRefEncounterZone`, `SetCellEncounterZone`, `SetWorldspaceEncounterZone`).
/// Carried across same-run loads in [`WorldSession`], not saved in [`Snapshot`].
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ZoneSession {
    pub(crate) min_levels: BTreeMap<FormId, i8>,
    pub(crate) match_levels: BTreeMap<FormId, bool>,
    pub(crate) ref_zones: BTreeMap<FormId, Option<FormId>>,
    pub(crate) cell_zones: BTreeMap<FormId, Option<FormId>>,
    pub(crate) world_zones: BTreeMap<FormId, Option<FormId>>,
}

impl ZoneSession {
    pub(crate) fn is_empty(&self) -> bool {
        self.min_levels.is_empty()
            && self.match_levels.is_empty()
            && self.ref_zones.is_empty()
            && self.cell_zones.is_empty()
            && self.world_zones.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorldSession {
    setting_changes: BTreeMap<String, crate::settings::Setting>,
    leveled_edits: BTreeMap<FormId, crate::forms::LeveledList>,
    base_edits: BTreeMap<FormId, Vec<crate::forms::Entry>>,
    container_respawns: BTreeMap<FormId, bool>,
    zone_session: ZoneSession,
}

impl WorldSession {
    pub fn is_empty(&self) -> bool {
        self.setting_changes.is_empty()
            && self.leveled_edits.is_empty()
            && self.base_edits.is_empty()
            && self.container_respawns.is_empty()
            && self.zone_session.is_empty()
    }
}

/// A death item rolled into a body ([`World::add_death_item`]): the level the list was
/// read at and the items added.
pub type DeathItemRoll = (u16, Vec<(FormId, i32)>);

pub const SNAPSHOT_FORMAT: &str = "fnv.sim.state/1";

impl Snapshot {
    /// Whether [`World::restore`] can take this snapshot: a load asks before it starts
    /// anything from the save (docs/design/save-load.md §6).
    pub fn check(&self) -> Result<(), String> {
        if self.format != SNAPSHOT_FORMAT {
            return Err(format!("unsupported snapshot format {:?}", self.format));
        }
        self.word_pos().map(|_| ())
    }

    fn word_pos(&self) -> Result<u128, String> {
        self.rng_word_pos.parse().map_err(|_| format!("bad rng_word_pos {:?}", self.rng_word_pos))
    }
}

/// A starting [`World::lock_serial`] no other world of this process used.
fn next_world_serial() -> u64 {
    static WORLDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    WORLDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) << 32
}

/// The time globals [`World::set_global`] has to recognise, as the load order names them.
struct TimeGlobalIds {
    hour: Option<FormId>,
    days_passed: Option<FormId>,
}

/// Session-only descriptor for a directly cloned non-actor scene base. It is not
/// serialized; the Game layer refuses unsupported snapshots containing its refs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemporarySceneBase {
    pub original: FormId,
    pub model_override: Option<String>,
}

pub struct World<'a> {
    pub forms: &'a FormStore,
    pub placed: &'a RefTable,
    cfg: SimConfig,
    /// The player's level as item rolls read it ([`World::set_player_level`]); `None`:
    /// [`SimConfig::player_level`]. Not saved: the game sets it on a load and on each tick.
    roll_level: Option<u16>,
    refs: BTreeMap<FormId, RefState>,
    temporary_scene_bases: BTreeMap<FormId, TemporarySceneBase>,
    /// Globals set at run time (scripts, the clock); others keep their authored value.
    globals: BTreeMap<FormId, f32>,
    /// The digits of the time globals their f32 cannot hold.
    fine_clock: clock::FineClock,
    /// Resolved once as `set_global` is on the scripts' hot path; `forms` is not replaced after construction [design: nothing assigns `World::forms`].
    time_global_ids: TimeGlobalIds,
    /// Unloaded cells and the GameDaysPassed at which they were unloaded.
    detached_cells: BTreeMap<FormId, f32>,
    /// Quest-item flags set at run time (`set_quest_object`); they replace the record's.
    quest_objects: BTreeMap<FormId, bool>,
    /// Default-open flags set at run time (`set_default_open`); they replace the placement's ONAM flag.
    default_open: BTreeMap<FormId, bool>,
    /// Entries added to leveled item lists at run time ([`World::add_to_leveled_list`]).
    leveled_added: BTreeMap<FormId, Vec<LeveledEntry>>,
    /// Leveled item lists edited by script for the session (LeveledListAddForm and its
    /// family): not saved, as "this change is not savebaked" for LU's functions
    /// [documented: RemoveFormFromLeveledList, "Unlike for LeveledListRemoveForm, this
    /// change is savebaked"], but kept across a load in the same run ([`WorldSession`]).
    leveled_edits: BTreeMap<FormId, crate::forms::LeveledList>,
    /// Container and actor bases whose carried items scripts edited (BaseAddItem,
    /// BaseRemoveItem): the entries a fresh inventory rolls from, for the session.
    base_edits: BTreeMap<FormId, Vec<crate::forms::Entry>>,
    /// What the game layer adds to an actor's carry capacity beyond the formula on its
    /// permanent Strength: temporary Strength (Buffout) and Carry Weight modifiers
    /// ([`World::set_carry_bonus`]).
    carry_bonus: BTreeMap<FormId, f64>,
    /// The game layer's weight rules of a carrier (its perks, [`World::set_weight_rules`]).
    weight_rules: BTreeMap<FormId, crate::inventory::WeightRules>,
    /// Placed actors whose carried items are another base's than their own (an actor
    /// template's, [`World::set_inventory_sources`]).
    inventory_sources: BTreeMap<FormId, FormId>,
    /// The inventory sources of references created at run time ([`World::give_items_of`]);
    /// saved.
    created_inventory_sources: BTreeMap<FormId, FormId>,
    /// Items made at run time (TempCloneForm of an item), each with the item it copies:
    /// a container takes them by their own ID ([`World::add_clone_item`]). Not saved.
    clone_items: BTreeMap<FormId, FormId>,
    /// Session-only item overrides (`SetEquipmentSlotsMask`, `SetEquipmentBipedMask`,
    /// `SetArmorClass`, `SetIsPowerArmor`, `SetIsPlayable`, `SetArmorDT`,
    /// `SetArmorArmorRating`, `SetWeight`, `SetObjectHealth`). Not saved.
    item_overrides: BTreeMap<FormId, ItemOverride>,
    /// Game settings changed at run time (the console's `SetGameSetting`), by lower-case
    /// Editor ID; they replace the record's value for [`World::setting_value`] readers.
    /// Kept for the session, not saved ([`WorldSession`]).
    setting_changes: BTreeMap<String, crate::settings::Setting>,
    /// The player may wear power armour (SetPCCanUsePowerArmor; the game layer keeps the
    /// flag and sets it here, [`World::set_power_armor_training`]).
    power_armor_training: bool,
    /// The player may equip a broken weapon (the player's Equip Broken Items tweak, set by
    /// the game layer, [`World::set_player_equips_broken`]) [design: docs/design/tweaks.md].
    player_equips_broken: bool,
    /// The player's key is not used on a lock: the engine sets it while the player's
    /// Pick Locks Even With Key tweak and Shift are on (NVForever's tweaks,
    /// docs/design/tweaks.md); not saved.
    player_ignores_keys: bool,
    /// Encounter zones do not freeze their level on first visit (`bNoLockEncounterZoneLevels`,
    /// set by the game layer, [`World::set_no_lock_encounter_zone_levels`]) [design:
    /// docs/design/tweaks.md]; not saved.
    pub(crate) no_lock_encounter_zone_levels: bool,
    /// Encounter zones entered and their levels (`crate::zones`).
    pub(crate) zone_levels: BTreeMap<FormId, u16>,
    /// SetZoneRespawns: zone → whether it respawns, over its "Never Resets" flag.
    pub(crate) zone_respawns: BTreeMap<FormId, bool>,
    /// The leveled-list level fixed per reference (`crate::zones`).
    pub(crate) ref_levels: BTreeMap<FormId, crate::zones::LevelAt>,
    /// Death items owed to dead actors not read yet, rolled into the inventory when it is
    /// first made ([`World::add_death_item`]).
    death_items: BTreeMap<FormId, FormId>,
    /// Created references deleted before the last load, with their base forms: their
    /// state was left out of the save (`Snapshot::deleted_created`). A lookup of one
    /// answers [`SimError::Deleted`]; their FormIDs are never given again
    /// (`next_dynamic` only goes up).
    deleted_created: BTreeMap<FormId, FormId>,
    /// Counts lock changes ([`World::lock`], [`World::unlock`]) so the AI can bring its
    /// nav graph's door locks in line when one changed ([`World::lock_serial`]). Not
    /// saved: every world made (a new or a restored one) starts from a number no other
    /// world of this process used, so a reader sees the change of world as a change.
    lock_serial: u64,
    /// Counts the changes that can change [`World::is_enabled`] or a reference's
    /// `deleted` flag ([`World::presence_serial`]): a reference's state made with flags
    /// other than its placement's, put in or taken out of `refs`, and every write of
    /// `enabled` or `deleted`. Not saved; every
    /// world starts from a number no other world of this process used, as
    /// `lock_serial`.
    presence_serial: u64,
    /// Session-only `SetContainerRespawns` overrides: container base → whether it respawns,
    /// over its `CONT` record's `Respawns` flag (`World::container_respawns`,
    /// `World::set_container_respawns`). Kept for the session, not saved ([`WorldSession`])
    /// [hypothesis: session-only, like `base_edits`].
    container_respawns: BTreeMap<FormId, bool>,
    /// Session-only encounter-zone level/flag and assignment overrides ([`WorldSession`]).
    /// Not saved in [`Snapshot`] [hypothesis: session-only, like other record setters].
    pub(crate) zone_session: ZoneSession,
    /// ShowOff's SetEnableParent: a reference's enable parent for the session in place of
    /// its XESP (`None`: removed) [documented: SetEnableParent]. Not saved [hypothesis: the
    /// page does not say the change is savebaked].
    enable_parent_edits: BTreeMap<FormId, Option<crate::refs::EnableParent>>,
    /// References DisableAlt or EnableAlt set: their own enable flag counts, whatever their
    /// enable parent says [documented: DisableAlt, EnableAlt, "Ignores Disable's limitation
    /// of being unable to disable references that have an Enable Parent"; hypothesis: until
    /// SetEnableParent changes their parent]. Not saved.
    own_enable: std::collections::BTreeSet<FormId>,
    /// Monotonic identity for this `World` instance, used by caches that track incremental
    /// reference mutations against `World::refs`.
    instance_id: u64,
    /// References inserted into, removed from, or mutably borrowed via `World::refs` since
    /// the last [`World::take_touched_refs`].
    touched_refs: std::collections::BTreeSet<FormId>,
    next_dynamic: u32,
    rng: ChaCha8Rng,
    /// Co-op replication: the references whose state was reached for writing
    /// ([`World::get`], a reference made or taken away) since the host last took them
    /// ([`World::take_touched`]). `None`, the default and always in single player:
    /// nothing is kept [design]. Not saved.
    touched: Option<std::collections::BTreeSet<FormId>>,
}

impl<'a> World<'a> {
    pub fn new(forms: &'a FormStore, placed: &'a RefTable, cfg: SimConfig) -> World<'a> {
        let serial = next_world_serial();
        World {
            forms,
            placed,
            cfg,
            roll_level: None,
            refs: BTreeMap::new(),
            temporary_scene_bases: BTreeMap::new(),
            globals: BTreeMap::new(),
            fine_clock: clock::FineClock::default(),
            time_global_ids: TimeGlobalIds { hour: forms.by_edid(clock::GAME_HOUR), days_passed: forms.by_edid(clock::GAME_DAYS_PASSED) },
            detached_cells: BTreeMap::new(),
            quest_objects: BTreeMap::new(),
            default_open: BTreeMap::new(),
            leveled_added: BTreeMap::new(),
            leveled_edits: BTreeMap::new(),
            base_edits: BTreeMap::new(),
            carry_bonus: BTreeMap::new(),
            weight_rules: BTreeMap::new(),
            inventory_sources: BTreeMap::new(),
            created_inventory_sources: BTreeMap::new(),
            clone_items: BTreeMap::new(),
            item_overrides: BTreeMap::new(),
            setting_changes: BTreeMap::new(),
            power_armor_training: false,
            player_equips_broken: false,
            player_ignores_keys: false,
            no_lock_encounter_zone_levels: false,
            zone_levels: BTreeMap::new(),
            zone_respawns: BTreeMap::new(),
            ref_levels: BTreeMap::new(),
            death_items: BTreeMap::new(),
            deleted_created: BTreeMap::new(),
            lock_serial: serial,
            presence_serial: serial,
            container_respawns: BTreeMap::new(),
            zone_session: ZoneSession::default(),
            enable_parent_edits: BTreeMap::new(),
            own_enable: std::collections::BTreeSet::new(),
            instance_id: serial,
            touched_refs: std::collections::BTreeSet::new(),
            next_dynamic: FormId::FIRST_DYNAMIC.0,
            rng: ChaCha8Rng::seed_from_u64(cfg.seed),
            touched: None,
        }
    }

    /// Unique monotonic ID of this `World` instance.
    pub fn instance_id(&self) -> u64 {
        self.instance_id
    }

    /// Drain references inserted into, removed from, or mutably borrowed in `World::refs`
    /// since the last call.
    pub fn take_touched_refs(&mut self) -> std::collections::BTreeSet<FormId> {
        std::mem::take(&mut self.touched_refs)
    }

    /// Keep (or stop keeping) the references whose state is reached for writing, for a
    /// co-op host's change log ([`World::take_touched`]). Off in single player.
    pub fn set_tracking(&mut self, on: bool) {
        match (on, self.touched.is_some()) {
            (true, false) => self.touched = Some(Default::default()),
            (false, true) => self.touched = None,
            _ => {}
        }
    }

    pub fn tracking(&self) -> bool {
        self.touched.is_some()
    }

    /// The references reached for writing since the last call (empty when not tracking):
    /// every write of a reference's state goes through [`World::get`] or makes or takes
    /// away a whole state here, so nothing that changes one is missed.
    pub fn take_touched(&mut self) -> std::collections::BTreeSet<FormId> {
        self.touched.as_mut().map(std::mem::take).unwrap_or_default()
    }

    fn touch(&mut self, id: FormId) {
        if let Some(t) = self.touched.as_mut() {
            t.insert(id);
        }
    }

    /// Dress actor `id` in exactly `items` (a co-op guest drawing an NPC as the host's
    /// game has it dressed): each item it lacks is added once, each is equipped, and
    /// every other equipped item is taken off (kept in the inventory). The actor counts
    /// as outfitted, so its own outfitting does not dress it again. Returns whether
    /// anything changed. The player is never dressed so; nor is a reference with no
    /// inventory.
    pub fn wear_as(&mut self, id: FormId, items: &[FormId]) -> Result<bool, SimError> {
        if id == FormId::PLAYER_REF {
            return Ok(false);
        }
        let known: Vec<FormId> = items.iter().copied().filter(|&i| self.forms.item(i).is_some()).collect();
        let s = self.get(id)?;
        let Some(inv) = s.inventory.as_mut() else { return Ok(false) };
        let mut changed = false;
        for i in inv.equipped() {
            if !known.contains(&i) {
                changed |= inv.clear_equipped(i);
            }
        }
        for &i in &known {
            if inv.count(i) <= 0 {
                inv.add(i, 1);
                changed = true;
            }
            if !inv.is_equipped(i) {
                changed |= inv.set_equipped(i);
            }
        }
        if !s.outfitted {
            s.outfitted = true;
            changed = true;
        }
        Ok(changed)
    }

    pub fn config(&self) -> SimConfig {
        self.cfg
    }

    /// Hardcore mode turned on or off while the game runs (`fnv_game::Game::set_hardcore`,
    /// SetHardcore): ammunition weighs from then on, or no longer.
    pub fn set_hardcore(&mut self, on: bool) {
        self.cfg.hardcore = on;
    }

    /// The player's level changed (`fnv_game::Game::sync_player_level`): AddItem, PlaceAtMe
    /// and references with neither a fixed level nor a zone roll their items at it from now
    /// on. Kept here because inventories are made on any read, where no caller can pass it
    /// in. Locks do not read it ([`World::lock_read_level`]).
    pub fn set_player_level(&mut self, level: u16) {
        self.roll_level = Some(level.max(1));
    }

    /// The level item rolls read as the player's.
    pub fn roll_level(&self) -> u16 {
        self.roll_level.unwrap_or(self.cfg.player_level)
    }

    /// Whether the player may wear power armour (ARMO BMDT "Power Armor"): without it
    /// [`World::equip`] refuses such armour on the player with
    /// [`SimError::NoPowerArmorTraining`] [documented: GECK wiki SetPCCanUsePowerArmor,
    /// "sets the flag that lets the player use power armor"; hypothesis: that the flag
    /// gates the BMDT flag's armour]. Other actors wear it freely. Off at the start
    /// [hypothesis: the masters set it only from a stage of the Generic quest, which the
    /// Power Armor Training perk runs, observed].
    pub fn set_power_armor_training(&mut self, on: bool) {
        self.power_armor_training = on;
    }

    /// Whether the player may equip a broken weapon (else [`SimError::Broken`]).
    pub fn set_player_equips_broken(&mut self, on: bool) {
        self.player_equips_broken = on;
    }

    /// Whether the player's carried key is left unused on a lock ([`World::activate`]).
    pub fn set_player_ignores_keys(&mut self, on: bool) {
        self.player_ignores_keys = on;
    }

    /// Whether encounter zones avoid freezing their level on first visit
    /// (`bNoLockEncounterZoneLevels`; not saved) [design: docs/design/tweaks.md].
    pub fn set_no_lock_encounter_zone_levels(&mut self, on: bool) {
        self.no_lock_encounter_zone_levels = on;
    }

    pub fn no_lock_encounter_zone_levels(&self) -> bool {
        self.no_lock_encounter_zone_levels
    }

    pub fn power_armor_training(&self) -> bool {
        self.power_armor_training
    }

    /// State of a reference that has already been materialised.
    pub fn peek(&self, id: FormId) -> Option<&RefState> {
        self.refs.get(&id)
    }

    /// Materialised references in FormID order.
    pub fn changed(&self) -> impl Iterator<Item = (FormId, &RefState)> {
        self.refs.iter().map(|(&k, v)| (k, v))
    }

    /// [design, #2538] The applicable lock (`World::lock_source`), from runtime state
    /// or its placed XLOC at `World::lock_read_level`, without materialising it.
    pub fn lock_of(&self, id: FormId) -> Option<LockState> {
        self.own_lock_of(self.lock_source(id))
    }

    /// The reference holding the applicable lock. A load door without its own lock
    /// reads its XTEL partner's [observed: capture run15b_a, 2026-11-15; #2538,
    /// docs/spec-notes/cells-refs.md section 6]. Own locks win; only one link is followed
    /// [hypothesis: independent locks and malformed links were not captured].
    /// [design] No state is materialised or duplicated (`own_lock_of`).
    pub fn lock_source(&self, id: FormId) -> FormId {
        if self.own_lock_of(id).is_some() {
            return id;
        }
        let placed = self.placed.get(id);
        let base = self.peek(id).map(|s| s.base).or_else(|| placed.map(|p| p.base));
        if base.and_then(|b| self.forms.sig(b)).as_ref() != Some(b"DOOR") {
            return id;
        }
        placed.and_then(|p| p.teleport).map(|t| t.door).filter(|&d| self.own_lock_of(d).is_some()).unwrap_or(id)
    }

    fn own_lock_of(&self, id: FormId) -> Option<LockState> {
        match self.peek(id) {
            Some(s) => s.lock,
            None => self.placed.get(id).and_then(|p| p.lock).map(|l| LockState { level: l.level_at(self.lock_read_level(id)), key: l.key, locked: true }),
        }
    }

    /// The current state of `id`, creating it from the placement data on first use.
    pub fn get(&mut self, id: FormId) -> Result<&mut RefState, SimError> {
        if let Some(t) = self.touched.as_mut() {
            t.insert(id);
        }
        if !self.refs.contains_key(&id) {
            if self.deleted_created.contains_key(&id) {
                return Err(SimError::Deleted(id));
            }
            let s = self.initial(id)?;
            // Until it has a state, a placed reference's presence is read from its
            // placement (`is_enabled`: enabled unless Initially Disabled; not deleted). A
            // state that starts with those values changes nothing a reader of presence
            // sees, so it leaves the serial; any other (the player, which is not placed)
            // moves it.
            let as_placed = self.placed.get(id).is_some_and(|p| s.enabled() == !p.initially_disabled() && !s.deleted());
            self.refs.insert(id, s);
            if !as_placed {
                self.presence_serial += 1;
            }
        }
        self.touched_refs.insert(id);
        Ok(self.refs.get_mut(&id).expect("inserted above"))
    }

    /// The base form a reference not yet materialised starts from, as [`World::initial`]
    /// picks it.
    fn initial_base(&self, id: FormId) -> Result<FormId, SimError> {
        match self.placed.get(id) {
            Some(p) => Ok(p.base),
            None if id == FormId::PLAYER_REF => Ok(FormId::PLAYER_BASE),
            None => Err(SimError::UnknownRef(id)),
        }
    }

    fn initial(&mut self, id: FormId) -> Result<RefState, SimError> {
        let (base, placed): (FormId, Option<&PlacedRef>) = match self.placed.get(id) {
            Some(p) => (p.base, Some(p)),
            // The player is not placed by any shipped plugin. [hypothesis; reported
            // by `fnvtool sim forms`] It starts from its base form.
            None if id == FormId::PLAYER_REF => (FormId::PLAYER_BASE, None),
            None => return Err(SimError::UnknownRef(id)),
        };
        let mut inventory = self.roll_inventory(self.inventory_base(id, base), self.item_level(id))?;
        let lock_level = self.lock_read_level(id);
        if let Some(list) = self.death_items.remove(&id) {
            let level = self.item_level(id);
            let inv = inventory.get_or_insert_with(Inventory::new);
            for e in self.roll_death_item(list, level)? {
                inv.add_extra(e.item, e.count, e.extra);
            }
        }
        if id == FormId::PLAYER_REF {
            // The player wears the non-playable armour it carries: the Pip-Boy Glove and
            // the Pip-Boy 3000 that the base form NPC_ 0x7 lists. [observed in game,
            // docs/spec-notes/simulation.md §2: `GetEquipped` 1 for both in the saves
            // nvftest and nvfdoc (`cl_a_pipboy_*`); the owner never saw the glove
            // unequipped. A new game before the intro (nvfnew) holds neither: handing them
            // over is the intro's script, Layer 5]
            if let Some(inv) = inventory.as_mut() {
                for item in inv.iter().map(|(i, _)| i).collect::<Vec<_>>() {
                    if self.effective_non_playable_biped(item) {
                        inv.set_equipped(item);
                    }
                }
            }
        }
        Ok(RefState {
            base,
            cell: placed.and_then(|p| p.cell),
            pos: placed.map(|p| p.pos).unwrap_or_default(),
            rot: placed.map(|p| p.rot).unwrap_or_default(),
            enabled: !placed.is_some_and(|p| p.initially_disabled()),
            deleted: false,
            count: self.forms.placed_count(base, placed.and_then(|p| p.count)),
            inventory,
            // A reference with lock data starts locked. [hypothesis] A leveled lock's
            // level is set now (`World::lock_read_level`, `Lock::level_at`).
            lock: placed.and_then(|p| p.lock).map(|l| LockState { level: l.level_at(lock_level), key: l.key, locked: true }),
            // A door flagged Open by Default (or overridden by SetDefaultOpen) starts open.
            // [hypothesis from the flag's name; a gate so flagged let the player drop through it, physics.md §4]
            open: self.default_open(id),
            extra: Extra {
                condition: placed.and_then(|p| p.health),
                owner: placed.and_then(|p| p.owner),
                equipped: false,
                mods: 0,
                live: false,
                magazine: None,
            },
            actor_values: BTreeMap::new(),
            last_restock: None,
            scale: None,
            equip_locks: BTreeMap::new(),
            dropped_by: None,
            player_drop: false,
            made_at: None,
            made_rot: None,
            shot: false,
            outfitted: false,
        })
    }

    /// The placed actors whose carried items come from another actor base than their
    /// own: the game layer's actor templates (template flag "Inventory") and placed
    /// leveled actor lists, which this layer does not follow (`fnv_game`
    /// `GameData::inventory_sources`). Set before the references are first read, and
    /// again on a load (worked out from the plugins, so not saved).
    pub fn set_inventory_sources(&mut self, sources: BTreeMap<FormId, FormId>) {
        self.inventory_sources = sources;
    }

    /// The base whose carried items `id` starts with, in place of its own base's
    /// (`None`: its own base's), after its leveled choices were rolled again. Created
    /// references save their source; placed references reconstruct it on a load.
    pub fn set_inventory_source(&mut self, id: FormId, source: Option<FormId>) {
        let sources = if id.is_dynamic() {
            // A created reference's saved source takes precedence in inventory_base.
            // Retire an earlier reconstructed entry too, including when clearing it.
            self.inventory_sources.remove(&id);
            &mut self.created_inventory_sources
        } else {
            &mut self.inventory_sources
        };
        match source {
            Some(s) => sources.insert(id, s),
            None => sources.remove(&id),
        };
    }

    /// The base whose carried items `id` (placed with `base`) starts with.
    fn inventory_base(&self, id: FormId, base: FormId) -> FormId {
        self.created_inventory_sources.get(&id).or_else(|| self.inventory_sources.get(&id)).copied().unwrap_or(base)
    }

    /// A fresh inventory for a container or actor base: its carried items with leveled
    /// lists resolved at `level` (the reference's, `crate::zones`). `None` for bases
    /// without carried items.
    fn roll_inventory(&mut self, base: FormId, level: u16) -> Result<Option<Inventory>, SimError> {
        let Some(entries) = self.base_entries(base) else { return Ok(None) };
        let mut inv = Inventory::new();
        let mut out = Vec::new();
        for e in entries {
            let from = out.len();
            // LVLG globals at their current values [observed: leveled_b8 captures].
            leveled::resolve_with(self.forms, &self.globals, &self.leveled_added, &self.leveled_edits, e.item, e.count, level, &mut self.rng, &mut out)?;
            // COED on an entry applies to whatever it resolves to [hypothesis]; without
            // one an item keeps the condition a script's leveled entry gave it.
            if !e.extra.is_plain() {
                for r in &mut out[from..] {
                    r.extra = e.extra;
                }
            }
        }
        for e in out {
            if e.count > 0 {
                inv.add_extra(e.item, e.count, e.extra);
            }
        }
        Ok(Some(inv))
    }

    /// An actor other than the player starts with the armour it wears and the weapon it
    /// draws equipped, chosen from its carried items: armour by DT, then DR, then carried
    /// order, each piece worn unless one already chosen shares a body slot (pieces with no
    /// slot are not worn); a drawable weapon held in the hands before one thrown or laid
    /// (`Item::thrown`), then the one with the highest damage as the weapon choice counts it (`forms::weapon_choice_damage`: a gun's explosion counts), the
    /// first on ties. These are the rules the engine dresses and arms NPCs by (`fnv_actor::resolve`,
    /// docs/spec-notes/actors.md §2.3 and §9.3) [observed rule for armour: the owner's
    /// statement of 2026-10-08; the weapon rule is the engine's choice standing in for the
    /// original's]. `inventory_source` is the form whose carried items were rolled;
    /// nothing happens for other bases (returns false).
    fn equip_default(&self, inventory_source: FormId, inventory: &mut Option<Inventory>) -> bool {
        let Some(inv) = inventory.as_mut() else { return false };
        let Some(chosen) = self.default_choice(inventory_source, inv) else { return false };
        for id in chosen {
            inv.set_equipped(id);
        }
        true
    }

    /// The items [`World::equip_default`] equips from `inv` (armour, then the weapon);
    /// `None` when `inventory_source` is no actor base.
    fn default_choice(&self, inventory_source: FormId, inv: &Inventory) -> Option<Vec<FormId>> {
        if !matches!(self.forms.get(inventory_source).map(|b| &b.data), Some(BaseData::Actor(_))) {
            return None;
        }
        // Carried order is the inventory's order (FormID order here; the engine uses the
        // record's entry order, so ties between equal pieces may fall the other way).
        let carried: Vec<FormId> = inv.iter().filter(|&(_, n)| n > 0).map(|(i, _)| i).collect();
        let mut armor: Vec<(usize, FormId, f32, f32, u32)> = carried
            .iter()
            .enumerate()
            .filter_map(|(o, &i)| {
                let it = self.resolve_item(i)?;
                (it.kind == ItemKind::Armor).then(|| {
                    (
                        o,
                        i,
                        self.effective_armor_dt(i).unwrap_or(0.0),
                        self.effective_armor_dr(i).unwrap_or(0.0),
                        self.effective_slots(i).unwrap_or(0),
                    )
                })
            })
            .collect();
        armor.sort_by(|a, b| b.2.total_cmp(&a.2).then(b.3.total_cmp(&a.3)).then(a.0.cmp(&b.0)));
        let mut used = 0u32;
        let mut wear = Vec::new();
        for (_, id, _, _, slots) in armor {
            if slots != 0 && slots & used == 0 {
                used |= slots;
                wear.push(id);
            }
        }
        // A weapon held in the hands before one thrown or laid, then the highest damage.
        let mut weapon: Option<((bool, f32), FormId)> = None;
        for &i in &carried {
            let Some(it) = self.forms.item(i).filter(|it| it.kind == ItemKind::Weapon && it.drawable && !it.not_for_npcs) else { continue };
            let key = (!it.thrown, it.weapon_damage.unwrap_or(0.0));
            if weapon.is_none_or(|(best, _)| (key.0 && !best.0) || (key.0 == best.0 && key.1 > best.1)) {
                weapon = Some((key, i));
            }
        }
        Some(wear.into_iter().chain(weapon.map(|w| w.1)).collect())
    }

    /// What actor `id` wears and holds by the rules: its equipped items once outfitted
    /// ([`World::outfit`]), before that the ones its outfitting will equip, so what is
    /// drawn does not change when it is. The player, never outfitted, has its equipped
    /// items (issue #1364: before, the armour it carried counted as worn). `None` while
    /// the simulation holds no inventory for it, or for an inventory not rolled from an
    /// actor base. For the engine, which dresses and arms NPCs by it [design].
    pub fn equipment(&self, id: FormId) -> Option<Vec<FormId>> {
        let s = self.refs.get(&id)?;
        let inv = s.inventory.as_ref()?;
        if s.outfitted || id == FormId::PLAYER_REF {
            return Some(inv.equipped());
        }
        self.default_choice(self.inventory_base(id, s.base), inv)
    }

    /// An actor other than the player that was given something reviews its inventory and
    /// equips the best armour and weapon it has: "Executing AddItem function on NPCs will
    /// cause them to review their inventory and equip the best armor and weapon they
    /// have" [documented: GECK wiki AddItem notes]. Best is the outfitting's own choice
    /// ([`World::equip_default`]); each chosen item not yet equipped is equipped, taking
    /// off what shares its slot (a weapon replaces the weapon held). A broken weapon or a
    /// locked slot keeps what is there. Before the actor's first outfitting nothing happens
    /// (that outfitting chooses from everything it then carries). Returns each item put
    /// on with what came off for it (the caller runs OnEquip and OnUnequip).
    pub fn review_equipment(&mut self, id: FormId) -> Vec<(FormId, Vec<FormId>)> {
        if id == FormId::PLAYER_REF {
            return Vec::new();
        }
        let Some(s) = self.refs.get(&id).filter(|s| s.outfitted) else { return Vec::new() };
        let source = self.inventory_base(id, s.base);
        let Some(inv) = s.inventory.as_ref() else { return Vec::new() };
        let Some(chosen) = self.default_choice(source, inv) else { return Vec::new() };
        let wanted: Vec<FormId> = chosen.into_iter().filter(|&i| !inv.is_equipped(i)).collect();
        let mut out = Vec::new();
        for item in wanted {
            if let Ok(e) = self.equip(id, item) {
                out.push((item, e.unequipped));
            }
        }
        out
    }

    /// The first time an actor other than the player is loaded, it puts on its outfit
    /// and takes up its weapon ([`World::equip_default`]); later calls do nothing.
    /// Returns whether it was outfitted now. [observed in game (capture
    /// `fa_batch2_loaded_npcs`, 2026-10-16, docs/design/script-function-audit.md §3):
    /// Sunny Smiles read `GetEquipped` 0 for her leather armour and varmint rifle while
    /// her cell had never been loaded (`GetItemCount` 1 each), 1 for both once the player
    /// stood next to her, and still 1 after the player left her cell.] Called by the
    /// rules when a cell loads (`fnv_game::Game::set_loaded`).
    pub fn outfit(&mut self, id: FormId) -> Result<bool, SimError> {
        if id == FormId::PLAYER_REF || self.get(id)?.outfitted {
            return Ok(false);
        }
        let base = self.get(id)?.base;
        let source = self.inventory_base(id, base);
        let mut inv = self.get(id)?.inventory.take();
        let done = self.equip_default(source, &mut inv);
        let s = self.get(id)?;
        s.inventory = inv;
        s.outfitted = done;
        Ok(done)
    }

    /// Dress actor `id` in `items`, the armour and weapon another game has it wear and
    /// hold (a co-op guest's copy of the host's NPC: each game rolls carried items from
    /// its own random numbers, so the same NPC can come out dressed apart [design]). Its
    /// armour and weapon not in `items` come off, each of `items` it lacks is given (one)
    /// and each is put on; nothing else it carries changes. Afterwards
    /// [`World::equipment`] is `items` (but for one a script locked on or off). Returns
    /// whether anything changed. Not for the player.
    pub fn dress_as(&mut self, id: FormId, items: &[FormId]) -> Result<bool, SimError> {
        if id == FormId::PLAYER_REF {
            return Ok(false);
        }
        if self.get(id)?.inventory.is_none() {
            self.get(id)?.inventory = Some(Inventory::new());
        }
        self.outfit(id)?;
        let mut changed = !std::mem::replace(&mut self.get(id)?.outfitted, true);
        let forms = self.forms;
        let drawn = |i: FormId| forms.item(i).is_some_and(|it| matches!(it.kind, ItemKind::Armor | ItemKind::Weapon));
        let off: Vec<FormId> = self.inventory(id)?.equipped().into_iter().filter(|&i| drawn(i) && !items.contains(&i)).collect();
        for i in off {
            changed |= self.unequip(id, i).unwrap_or(false);
        }
        for &i in items.iter().filter(|&&i| drawn(i)) {
            if self.inventory(id)?.is_equipped(i) {
                continue;
            }
            if self.inventory(id)?.count(i) <= 0 {
                self.add_item(id, i, 1)?;
            }
            changed |= self.equip(id, i).is_ok();
        }
        Ok(changed)
    }

    /// Whether container base form `base` currently has the Respawns flag, reading
    /// session `SetContainerRespawns` overrides before the authored `CONT` record; `false`
    /// for non-containers.
    pub fn container_respawns(&self, base: FormId) -> bool {
        let Some(BaseData::Container(c)) = self.forms.get(base).map(|b| &b.data) else {
            return false;
        };
        self.container_respawns.get(&base).copied().unwrap_or(c.respawns)
    }

    /// `SetContainerRespawns flag [container]`: override `base`'s Respawns flag for the
    /// current session when `base` is a `CONT` record; ignored for non-containers.
    pub fn set_container_respawns(&mut self, base: FormId, respawns: bool) {
        if matches!(self.forms.get(base).map(|b| &b.data), Some(BaseData::Container(_))) {
            self.container_respawns.insert(base, respawns);
        }
    }

    /// Whether `id` is a merchant container that restocks: an actor names it in XMRC and
    /// its base has the Respawns flag (including session `SetContainerRespawns` overrides).
    /// [observed in game: Primm's merchant box 0x0017409C, whose base lacks the flag, never
    /// refilled in 7 days of daily reads (`bl_primm_restock`); Ringo's 0x001629C0, with the
    /// flag, did; docs/spec-notes/simulation.md §3]
    fn restocks(&self, id: FormId, base: FormId) -> bool {
        self.placed.is_merchant_container(id) && self.container_respawns(base)
    }

    /// Merchant restocks at the midnights in (`from`, `to`] (GameDaysPassed crossing a
    /// whole number): on a restock day ([`is_merchant_restock_day`]) every restocking
    /// merchant container ([`World::restocks`]) gets a fresh roll of its base's carried
    /// items (anything sold into it is lost). Contents do not change while time passes, so
    /// several restock days in one advance restock once, at the last of them. Containers
    /// never materialised are still as authored and need no visit.
    /// [observed in game, docs/spec-notes/simulation.md §3: refills land just after
    /// midnights, need no cell event, menu or access, and replace the contents (a stimpak
    /// put in was gone). Hypothesis: untouched containers are restocked too (invisible
    /// with Ringo's fixed 200 caps; a merchant with leveled stock would tell), and the
    /// day is the one GameDaysPassed starts — the authored clock's GameDaysPassed (5.0)
    /// and GameHour (12) disagree by half a day, while saves keep them together]
    fn restock_merchants(&mut self, from: f64, to: f64) -> Result<(), SimError> {
        let (first, last) = (from.floor() as i64 + 1, to.floor() as i64);
        let Some(day) = (first..=last).rev().take(7).find(|&d| is_merchant_restock_day(d)) else {
            return Ok(());
        };
        let due: Vec<(FormId, FormId)> = self
            .refs
            .iter()
            .filter(|&(&id, s)| !s.deleted() && self.restocks(id, s.base))
            .map(|(&id, s)| (id, s.base))
            .collect();
        for (id, base) in due {
            let inv = self.roll_inventory(base, self.item_level(id))?;
            let s = self.get(id)?;
            s.inventory = with_live(s.inventory.as_ref(), inv);
            s.last_restock = Some(day as f32);
        }
        Ok(())
    }

    fn inventory(&mut self, id: FormId) -> Result<&mut Inventory, SimError> {
        let s = self.get(id)?;
        if s.deleted() {
            return Err(SimError::Deleted(id));
        }
        s.inventory.as_mut().ok_or(SimError::NoInventory(id))
    }

    /// `clone`, made at run time from `item`, is an item a container takes by its own ID
    /// [documented: TempCloneForm; observed: MCM marks the mods it listed by adding them
    /// to a container, MCM Extender's menus by temporary clones of Caps001, MCM's script
    /// source and MCM Extender's OnPauseMenuOpen.gek]. False when `item` is no item.
    pub fn add_clone_item(&mut self, clone: FormId, item: FormId) -> bool {
        let item = self.clone_items.get(&item).copied().unwrap_or(item);
        let ok = self.forms.item(item).is_some();
        if ok {
            self.clone_items.insert(clone, item);
        }
        ok
    }

    /// Resolve `item` to its authored [`crate::forms::Item`] record, following
    /// `TempCloneForm` item aliases registered with [`World::add_clone_item`].
    pub fn resolve_item(&self, item: FormId) -> Option<&crate::forms::Item> {
        self.forms.item(item).or_else(|| self.clone_items.get(&item).and_then(|o| self.forms.item(*o)))
    }

    /// Session-only [`ItemOverride`] for `item` (or its uncloned original when `item` has
    /// no own entry). O(1) fast-path when no overrides are active.
    pub fn item_override(&self, item: FormId) -> Option<&ItemOverride> {
        if self.item_overrides.is_empty() {
            return None;
        }
        self.item_overrides
            .get(&item)
            .or_else(|| self.clone_items.get(&item).and_then(|o| self.item_overrides.get(o)))
    }

    /// Replace `item`'s session-only [`ItemOverride`].
    pub fn set_item_override(&mut self, item: FormId, ov: ItemOverride) {
        self.item_overrides.insert(item, ov);
    }

    /// Mutate `item`'s session-only [`ItemOverride`], creating a default entry if needed.
    pub fn mutate_item_override(&mut self, item: FormId, f: impl FnOnce(&mut ItemOverride)) {
        f(self.item_overrides.entry(item).or_default());
    }

    /// Effective biped slot mask (`BMDT/biped_flags`) for `item`, honoring session overrides.
    pub fn effective_slots(&self, item: FormId) -> Option<u32> {
        self.item_override(item).and_then(|o| o.slots).or_else(|| self.resolve_item(item)?.slots)
    }

    /// Effective armour class (`BMDT/general_flags` Medium/Heavy) for `item`.
    pub fn effective_armor_class(&self, item: FormId) -> Option<crate::forms::ArmorClass> {
        let it = self.resolve_item(item)?;
        (it.kind == ItemKind::Armor).then(|| self.item_override(item).and_then(|o| o.armor_class).unwrap_or(it.armor_class))
    }

    /// Effective Power Armor flag (`BMDT/general_flags` bit 5 / `SetIsPowerArmor`) for `item`.
    pub fn effective_power_armor(&self, item: FormId) -> bool {
        if let Some(pa) = self.item_override(item).and_then(|o| o.power_armor) {
            return pa;
        }
        self.resolve_item(item).is_some_and(|it| it.power_armor)
    }

    /// Effective Non-Playable flag (`BMDT/general_flags` bit 6 / `SetIsPlayable`) for `item`.
    pub fn effective_non_playable(&self, item: FormId) -> bool {
        if let Some(np) = self.item_override(item).and_then(|o| o.non_playable) {
            return np;
        }
        self.resolve_item(item).is_some_and(|it| it.non_playable)
    }

    /// Effective `Item::non_playable_biped` (non-playable armour) honoring session overrides.
    pub fn effective_non_playable_biped(&self, item: FormId) -> bool {
        self.resolve_item(item).is_some_and(|it| it.kind == ItemKind::Armor) && self.effective_non_playable(item)
    }

    /// Effective armour DT (`DNAM/dt`) for `item`.
    pub fn effective_armor_dt(&self, item: FormId) -> Option<f32> {
        self.item_override(item).and_then(|o| o.armor_dt).or_else(|| self.resolve_item(item)?.armor_dt)
    }

    /// Effective armour DR in percent (`DNAM/dr` / 100) for `item`.
    pub fn effective_armor_dr(&self, item: FormId) -> Option<f32> {
        self.item_override(item).and_then(|o| o.armor_dr).or_else(|| self.resolve_item(item)?.armor_dr)
    }

    /// Effective base weight (`DATA/weight`) for `item`.
    pub fn effective_weight(&self, item: FormId) -> Option<f32> {
        self.item_override(item).and_then(|o| o.weight).or_else(|| self.resolve_item(item)?.weight)
    }

    /// Effective full health (`DATA/health`) for `item`.
    pub fn effective_health(&self, item: FormId) -> Option<i32> {
        self.item_override(item).and_then(|o| o.health).or_else(|| self.resolve_item(item)?.health)
    }

    /// Whether putting on `item` takes `other` off ([`World::equip_at`]): a weapon replaces
    /// the equipped weapon, armour replaces equipped armour sharing an effective biped slot.
    fn equip_clashes_item(&self, item: FormId, other: FormId) -> bool {
        let (Some(it), Some(o)) = (self.resolve_item(item), self.resolve_item(other)) else { return false };
        match (it.kind, o.kind) {
            (ItemKind::Weapon, ItemKind::Weapon) => true,
            (ItemKind::Armor, ItemKind::Armor) => self.effective_slots(item).unwrap_or(0) & self.effective_slots(other).unwrap_or(0) != 0,
            _ => false,
        }
    }

    fn check_item(&self, item: FormId) -> Result<(), SimError> {
        if self.clone_items.contains_key(&item) {
            return Ok(());
        }
        match self.forms.item(item) {
            Some(_) => Ok(()),
            None if self.forms.sig(item).is_some() => Err(SimError::NotAnItem(item)),
            None => Err(SimError::UnknownForm(item)),
        }
    }

    pub fn item_count(&mut self, of: FormId, item: FormId) -> Result<i32, SimError> {
        Ok(self.inventory(of)?.count(item))
    }

    /// Add `count` of an item or leveled list; returns the items actually added.
    pub fn add_item(&mut self, to: FormId, item: FormId, count: i32) -> Result<Vec<(FormId, i32)>, SimError> {
        self.add_item_with(to, item, count, None)
    }

    /// As [`World::add_item`], every item given at `condition` (a fraction; `None` full),
    /// a leveled list's own condition included: AddItemHealthPercent [documented: GECK
    /// wiki AddItemHealthPercent, "the specified health percent overrides the health
    /// percentage set by the leveled list"].
    pub fn add_item_at_condition(&mut self, to: FormId, item: FormId, count: i32, condition: Option<f32>) -> Result<Vec<(FormId, i32)>, SimError> {
        self.add_item_with(to, item, count, Some(condition))
    }

    fn add_item_with(&mut self, to: FormId, item: FormId, count: i32, condition: Option<Option<f32>>) -> Result<Vec<(FormId, i32)>, SimError> {
        if count <= 0 {
            return Err(SimError::BadCount(count));
        }
        if self.forms.leveled(item).is_none() {
            self.check_item(item)?;
        }
        self.inventory(to)?;
        let mut out = Vec::new();
        leveled::resolve_with(self.forms, &self.globals, &self.leveled_added, &self.leveled_edits, item, count, self.roll_level(), &mut self.rng, &mut out)?;
        let inv = self.inventory(to)?;
        Ok(out
            .into_iter()
            .filter(|e| e.count > 0)
            .map(|e| {
                let extra = match condition {
                    Some(c) => Extra { condition: c, ..e.extra },
                    None => e.extra,
                };
                inv.add_extra(e.item, e.count, extra);
                (e.item, e.count)
            })
            .collect())
    }

    /// The dead actor `id` gets its death item `list` (a leveled item list, or an item),
    /// resolved at the level its carried items are rolled at (`crate::zones`), without
    /// OnAdd (docs/spec-notes/death-items.md). An actor whose inventory was not made yet
    /// (a placed corpse never read) is owed it until then, so that the list is read at
    /// the level fixed when its cell loads; `Ok(None)` then. Otherwise the level read at and
    /// the items added.
    pub fn add_death_item(&mut self, id: FormId, list: FormId) -> Result<Option<DeathItemRoll>, SimError> {
        if self.forms.leveled(list).is_none() {
            self.check_item(list)?;
        }
        if !self.refs.contains_key(&id) {
            if self.placed.get(id).is_none() {
                return Err(SimError::UnknownRef(id));
            }
            self.death_items.insert(id, list);
            return Ok(None);
        }
        let level = self.item_level(id);
        let rolled = self.roll_death_item(list, level)?;
        let inv = self.inventory(id)?;
        let added = rolled
            .into_iter()
            .map(|e| {
                inv.add_extra(e.item, e.count, e.extra);
                (e.item, e.count)
            })
            .collect();
        Ok(Some((level, added)))
    }

    /// The death items owed to actors whose inventory is not made yet.
    pub fn owed_death_items(&self) -> &BTreeMap<FormId, FormId> {
        &self.death_items
    }

    fn roll_death_item(&mut self, list: FormId, level: u16) -> Result<Vec<crate::forms::Entry>, SimError> {
        let mut out = Vec::new();
        leveled::resolve_with(self.forms, &self.globals, &self.leveled_added, &self.leveled_edits, list, 1, level, &mut self.rng, &mut out)?;
        out.retain(|e| e.count > 0);
        Ok(out)
    }

    /// Remove up to `count`; returns how many were removed. A live unit (a planted
    /// explosive, `Extra::live`) is not removed here, nor moved or dropped by the methods
    /// below: no `Pick` names it [design, #1640].
    pub fn remove_item(&mut self, from: FormId, item: FormId, count: i32) -> Result<i32, SimError> {
        if count <= 0 {
            return Err(SimError::BadCount(count));
        }
        Ok(self.inventory(from)?.remove(item, count))
    }

    /// Move up to `count` from one inventory to another; returns how many moved. Quest
    /// items move like any other: the pages that forbid dropping and selling them say
    /// nothing about containers, so storing one is allowed [hypothesis; not captured].
    pub fn transfer(&mut self, from: FormId, to: FormId, item: FormId, count: i32) -> Result<i32, SimError> {
        self.transfer_at(from, to, item, count, Pick::ANY)
    }

    /// As [`World::transfer`], moving only the units `at` names (`Extra::picked`: one
    /// condition and mods, or any): a menu row's units.
    pub fn transfer_at(&mut self, from: FormId, to: FormId, item: FormId, count: i32, at: Pick) -> Result<i32, SimError> {
        if count <= 0 {
            return Err(SimError::BadCount(count));
        }
        self.inventory(to)?;
        let groups = unequipped(self.inventory(from)?.take_at(item, count, at));
        let dest = self.inventory(to)?;
        let mut n = 0;
        for (e, c) in groups {
            dest.add_extra(item, c, e);
            n += c;
        }
        Ok(n)
    }

    /// Whether RemoveAllItems leaves `item` with `from`: non-playable biped items
    /// ([`Item::non_playable_biped`]), and the player's quest items [documented: GECK
    /// wiki RemoveAllItems, the exceptions are unplayable biped items and, "If the calling
    /// reference is the player, Quest Items"].
    fn stays(&self, from: FormId, item: FormId) -> bool {
        self.effective_non_playable_biped(item) || (from == FormId::PLAYER_REF && self.is_quest_item(item))
    }

    /// Move everything but non-playable items and the player's quest items (as [`World::remove_all_items`], the same
    /// script function with a target); returns the stacks moved. Extra data (condition,
    /// owner) moves with the items; equipped items are unequipped.
    pub fn transfer_all(&mut self, from: FormId, to: FormId) -> Result<Vec<(FormId, i32)>, SimError> {
        self.transfer_all_owned(from, to, true)
    }

    /// As [`World::transfer_all`]; with `keep_owner` false the moved items lose their
    /// owner [documented: GECK wiki RemoveAllItems, "If the retain ownership flag is set
    /// to a non-zero value, the items' original ownership is retained; otherwise the
    /// ownership is cleared"].
    pub fn transfer_all_owned(&mut self, from: FormId, to: FormId, keep_owner: bool) -> Result<Vec<(FormId, i32)>, SimError> {
        self.inventory(to)?;
        // A live unit (a planted explosive, `Extra::live`) stays with its holder.
        let all: Vec<_> = self.inventory(from)?.free_iter().collect();
        let stacks: Vec<_> = all.into_iter().filter(|&(i, _)| !self.stays(from, i)).collect();
        for &(item, n) in &stacks {
            let groups = unequipped(self.inventory(from)?.take_at(item, n, Pick::ANY));
            let dest = self.inventory(to)?;
            for (mut e, c) in groups {
                if !keep_owner {
                    e.owner = None;
                }
                dest.add_extra(item, c, e);
            }
        }
        Ok(stacks)
    }

    /// Empty an inventory except for non-playable items (and the player's quest items,
    /// [`World::stays`]), which stay as they are
    /// (equipped ones stay equipped); returns the stacks removed. [observed in game,
    /// docs/spec-notes/simulation.md §3: `player.RemoveAllItems` left the Pip-Boy Glove
    /// and the Pip-Boy 3000, both still equipped, and nothing else (`cl_a_pipboy_nvftest`);
    /// that other non-playable items and other holders behave alike is hypothesis]
    pub fn remove_all_items(&mut self, from: FormId) -> Result<Vec<(FormId, i32)>, SimError> {
        let groups: Vec<_> = self.inventory(from)?.groups().collect();
        // A live unit (a planted explosive, `Extra::live`) stays too [design, #1640].
        let keep: Vec<_> = groups.into_iter().filter(|&(i, e, _)| e.live || self.stays(from, i)).collect();
        let inv = self.inventory(from)?;
        let mut kept = Inventory::new();
        for &(item, extra, n) in &keep {
            kept.add_extra(item, n, extra);
        }
        let stacks: Vec<_> = inv.iter().map(|(i, n)| (i, n - kept.count(i))).filter(|&(_, n)| n > 0).collect();
        *inv = kept;
        Ok(stacks)
    }

    /// Whether `item` is a quest item now: the value a script last set with
    /// [`World::set_quest_object`], else the record's "Quest Item" header flag
    /// (`Item::quest_item`). One flag per base form: SetQuestObject sets "the" quest-item
    /// flag of the form [documented: GECK wiki SetQuestObject], the one GetFormFlag reads
    /// as bit 10 [documented: GECK wiki GetFormFlag]; that a script's value replaces the
    /// authored one for every reference is [hypothesis: the change is to the base form].
    pub fn is_quest_item(&self, item: FormId) -> bool {
        self.quest_objects.get(&item).copied().unwrap_or_else(|| self.forms.is_quest_item(item))
    }

    /// `SetQuestObject item flag`: set or clear the quest-item flag of a base form. It is
    /// kept in snapshots.
    pub fn set_quest_object(&mut self, item: FormId, quest: bool) {
        self.quest_objects.insert(item, quest);
    }

    /// `AddItemToLeveledList list form level count`: from now on `form` (an item or
    /// another leveled list) is an entry of the leveled item list `list` at `level`,
    /// `count` at a time; kept in snapshots, never taken back [documented: GECK wiki
    /// AddItemToLeveledList, "it will persist in the save game data. It can not be
    /// undone"]. Adding the same entry twice gives two entries [hypothesis].
    pub fn add_to_leveled_list(&mut self, list: FormId, entry: LeveledEntry) -> Result<(), SimError> {
        if self.forms.leveled(list).is_none() {
            return Err(if self.forms.sig(list).is_some() { SimError::NotAnItem(list) } else { SimError::UnknownForm(list) });
        }
        if self.forms.leveled(entry.item).is_none() {
            self.check_item(entry.item)?;
        }
        self.leveled_added.entry(list).or_default().push(entry);
        Ok(())
    }

    /// A leveled item list as the session has it: a script's edit, else the record's
    /// with AddItemToLeveledList's entries after it.
    pub fn leveled_list(&self, id: FormId) -> Option<crate::forms::LeveledList> {
        if let Some(l) = self.leveled_edits.get(&id) {
            return Some(l.clone());
        }
        let mut l = self.forms.leveled(id)?.clone();
        l.entries.extend(self.leveled_added(id).iter().copied());
        Some(l)
    }

    /// Edit a leveled item list for the session (the resolver then reads the edit);
    /// false when `id` is no leveled item list.
    pub fn edit_leveled(&mut self, id: FormId, f: impl FnOnce(&mut crate::forms::LeveledList)) -> bool {
        let Some(mut l) = self.leveled_list(id) else { return false };
        f(&mut l);
        self.leveled_edits.insert(id, l);
        true
    }

    /// A container or actor base's carried items (CNTO), with the session's edits.
    pub fn base_entries(&self, base: FormId) -> Option<Vec<crate::forms::Entry>> {
        match self.base_edits.get(&base) {
            Some(e) => Some(e.clone()),
            None => self.forms.entries(base).map(<[_]>::to_vec),
        }
    }

    /// Edit a container or actor base's carried items for the session; false when
    /// `base` carries none.
    pub fn edit_base_entries(&mut self, base: FormId, f: impl FnOnce(&mut Vec<crate::forms::Entry>)) -> bool {
        let Some(mut e) = self.base_entries(base) else { return false };
        f(&mut e);
        self.base_edits.insert(base, e);
        true
    }

    /// The entries scripts added to a leveled list.
    pub fn leveled_added(&self, list: FormId) -> &[LeveledEntry] {
        self.leveled_added.get(&list).map(Vec::as_slice).unwrap_or_default()
    }

    /// Drop items from an inventory into the world, one new reference per group of
    /// identical extra data, in the holder's cell, [`DROP_AHEAD`] units in front of the
    /// holder along its heading and [`DROP_ABOVE`] units above its position. Returns the
    /// new references. [observed in game, docs/spec-notes/simulation.md §5: the player
    /// dropping 1, 2 and 3 stimpaks made one reference each (count 1, 2, 3) at exactly
    /// that offset, which did not move for 10 s; hypothesis for other actors and items]
    ///
    /// This is the mechanism scripts use (`Drop`, `DropMe`, the action scripts'
    /// `drop_item`): it ignores the quest-item flag, as the game's scripted `player.Drop`
    /// does [observed 2026-10-09, `qi_drop_quest_item`: a quest key and a stimpak marked
    /// with SetQuestObject both left the player's inventory]. The player's own drop is
    /// [`World::player_drop_item`].
    pub fn drop_item(&mut self, from: FormId, item: FormId, count: i32) -> Result<Vec<FormId>, SimError> {
        self.drop_item_at(from, item, count, Pick::ANY)
    }

    /// As [`World::drop_item`], the dropped references keeping only their owner: DropMe
    /// "does not preserve the health, charges, or script variables" [documented: GECK
    /// wiki DropMe].
    pub fn drop_item_fresh(&mut self, from: FormId, item: FormId, count: i32) -> Result<Vec<FormId>, SimError> {
        if count <= 0 {
            return Err(SimError::BadCount(count));
        }
        let groups = unequipped(self.inventory(from)?.take_at(item, count, Pick::ANY));
        let groups = groups.into_iter().map(|(e, n)| (Extra { owner: e.owner, ..Extra::default() }, n)).collect();
        self.drop_groups(from, item, groups, Placing::Ahead)
    }

    /// As [`World::drop_item`], dropping only the units `at` names.
    pub fn drop_item_at(&mut self, from: FormId, item: FormId, count: i32, at: Pick) -> Result<Vec<FormId>, SimError> {
        if count <= 0 {
            return Err(SimError::BadCount(count));
        }
        let groups = unequipped(self.inventory(from)?.take_at(item, count, at));
        self.drop_groups(from, item, groups, Placing::Ahead)
    }

    /// Drop the equipped unit of `item` (a weapon that broke in an actor's hand), as
    /// [`World::drop_item`]: the new reference keeps its condition. Nothing is dropped
    /// when none is equipped.
    pub fn drop_equipped(&mut self, from: FormId, item: FormId) -> Result<Vec<FormId>, SimError> {
        let groups = unequipped(self.inventory(from)?.take_equipped(item));
        self.drop_groups(from, item, groups, Placing::Ahead)
    }

    fn drop_groups(&mut self, from: FormId, item: FormId, groups: Vec<(Extra, i32)>, placing: Placing) -> Result<Vec<FormId>, SimError> {
        let holder = self.get(from)?.clone();
        // A z rotation turns clockwise seen from above and 0 faces +Y (world.md §4), so
        // the heading's direction is (sin z, cos z).
        let (sin, cos) = holder.rot[2].sin_cos();
        let ahead = [holder.pos[0] + DROP_AHEAD * sin, holder.pos[1] + DROP_AHEAD * cos, holder.pos[2] + DROP_ABOVE];
        let mut out = Vec::new();
        for (extra, n) in groups {
            let pos = match placing {
                Placing::Ahead => ahead,
                Placing::Ring => self.player_drop_place(&holder),
            };
            let id = FormId(self.next_dynamic);
            self.next_dynamic += 1;
            self.presence_serial += 1;
            self.touch(id);
            self.touched_refs.insert(id);
            self.refs.insert(
                id,
                RefState {
                    base: item,
                    cell: holder.cell,
                    pos,
                    rot: [0.0; 3],
                    enabled: true,
                    deleted: false,
                    count: n,
                    inventory: None,
                    lock: None,
                    open: false,
                    extra,
                    actor_values: BTreeMap::new(),
                    last_restock: None,
                    outfitted: false,
                    scale: None,
                    equip_locks: BTreeMap::new(),
                    dropped_by: None,
                    player_drop: from == FormId::PLAYER_REF,
                    made_at: None,
                    made_rot: None,
                    shot: false,
                },
            );
            out.push(id);
        }
        Ok(out)
    }

    /// The player drops items by hand (the inventory menu): as [`World::drop_item`], but a
    /// quest item ([`World::is_quest_item`]) is refused with [`SimError::QuestItem`] and
    /// nothing moves [documented: GECK wiki Category:Items and SetQuestObject: the player
    /// cannot drop one]. The count is checked first.
    pub fn player_drop_item(&mut self, item: FormId, count: i32) -> Result<Vec<FormId>, SimError> {
        self.player_drop_item_at(item, count, Pick::ANY)
    }

    /// As [`World::player_drop_item`], dropping only the units `at` names. Unlike a
    /// scripted drop, each new reference is placed at chest height ([`PLAYER_DROP_ABOVE`])
    /// in the first free place of a ring around the player ([`player_drop_offset`]), so
    /// items dropped one after another lie side by side instead of in one heap [design:
    /// owner 2026-10-27].
    pub fn player_drop_item_at(&mut self, item: FormId, count: i32, at: Pick) -> Result<Vec<FormId>, SimError> {
        if count > 0 && self.is_quest_item(item) {
            return Err(SimError::QuestItem(item));
        }
        if count <= 0 {
            return Err(SimError::BadCount(count));
        }
        let from = FormId::PLAYER_REF;
        let groups = unequipped(self.inventory(from)?.take_at(item, count, at));
        self.drop_groups(from, item, groups, Placing::Ring)
    }

    /// Where the player's next hand-dropped item goes: the first place in the rings around
    /// `holder` with no dropped item standing in the holder's cell within
    /// [`PLAYER_DROP_CLEAR`] of it, or the first place when all are taken.
    fn player_drop_place(&self, holder: &RefState) -> [f32; 3] {
        let at = |slot| {
            let [dx, dy] = player_drop_offset(holder.rot[2], slot);
            [holder.pos[0] + dx, holder.pos[1] + dy, holder.pos[2] + PLAYER_DROP_ABOVE]
        };
        let taken = |p: [f32; 3]| {
            self.created().any(|(_, s)| s.cell == holder.cell && !s.deleted() && s.enabled() && (s.pos[0] - p[0]).hypot(s.pos[1] - p[1]) < PLAYER_DROP_CLEAR)
        };
        (0..PLAYER_DROP_RING * PLAYER_DROP_RINGS).map(at).find(|&p| !taken(p)).unwrap_or_else(|| at(0))
    }

    /// Equip one carried `item` on `actor`. Weapons replace the equipped weapon; armour
    /// replaces equipped armour that shares a body slot (BMDT biped flags); aid items
    /// are consumed (their effects are Layer 6). [hypothesis: in-game equip rules as the
    /// GECK wiki describes `EquipItem`; to be captured] A weapon at zero condition is
    /// broken and is refused with [`SimError::Broken`] until it is repaired [design: the
    /// owner's rule, 2026-10-11]; a unit that is not broken is taken before a broken one.
    pub fn equip(&mut self, actor: FormId, item: FormId) -> Result<Equip, SimError> {
        self.equip_at(actor, item, Pick::ANY)
    }

    /// As [`World::equip`], equipping (or using) one of the units `at` names: a menu
    /// row's. Its checks are [`World::equip_check`]'s.
    pub fn equip_at(&mut self, actor: FormId, item: FormId, at: Pick) -> Result<Equip, SimError> {
        let _ = self.resolve_item(item).ok_or(SimError::NotAnItem(item))?;
        // Materialise the actor first; the checks read its state.
        self.inventory(actor)?;
        let consumed = self.equip_check(actor, item, at)?;
        if consumed {
            let inv = self.inventory(actor)?;
            return match inv.take_at(item, 1, at).iter().map(|g| g.1).sum::<i32>() {
                1 => Ok(Equip { consumed: true, unequipped: Vec::new() }),
                _ => Err(SimError::NotCarried { actor, item }),
            };
        }
        let clashing: Vec<FormId> = self
            .refs
            .get(&actor)
            .and_then(|s| s.inventory.as_ref())
            .map(|inv| inv.equipped().into_iter().filter(|&other| self.equip_clashes_item(item, other)).collect())
            .unwrap_or_default();
        let inv = self.inventory(actor)?;
        let mut unequipped = Vec::new();
        for other in clashing {
            if inv.clear_equipped(other) {
                unequipped.push(other);
            }
        }
        let equipped = inv.set_equipped_at(item, at);
        debug_assert!(equipped, "can_equip_at said a unit was free");
        Ok(Equip { consumed: false, unequipped })
    }

    /// Whether [`World::equip_at`] takes `item` (one of the units `at` names) on `actor`,
    /// changing nothing: `Ok(true)` when it is used up (an aid item, a skill book),
    /// `Ok(false)` when it is put on, else the error `equip_at` returns. `equip_at` runs
    /// these checks itself, so a menu's hint reading them (#1896) agrees with the action.
    /// Only a materialised actor is read ([`World::peek`]); `equip_at` materialises it.
    pub fn equip_check(&self, actor: FormId, item: FormId, at: Pick) -> Result<bool, SimError> {
        let it = self.resolve_item(item).ok_or(SimError::NotAnItem(item))?;
        let state = self.refs.get(&actor).ok_or(SimError::UnknownRef(actor))?;
        if state.deleted() {
            return Err(SimError::Deleted(actor));
        }
        let inv = state.inventory.as_ref().ok_or(SimError::NoInventory(actor))?;
        // A skill book is read, and so used up like an aid item (`fnv_game::books`,
        // docs/spec-notes/skill-books.md §2); a book that teaches nothing is not used.
        if it.kind == ItemKind::Aid || (it.kind == ItemKind::Book && it.teaches.is_some()) {
            // Any unit `at` names is taken (`Inventory::take_at`).
            return if inv.can_equip_at(item, at, true) { Ok(true) } else { Err(SimError::NotCarried { actor, item }) };
        }
        if !matches!(it.kind, ItemKind::Weapon | ItemKind::Armor) {
            return Err(SimError::NotEquippable(item));
        }
        // A live unit (a planted explosive, `Extra::live`) is not the actor's to equip.
        if inv.free_count(item) == 0 {
            return Err(SimError::NotCarried { actor, item });
        }
        if it.kind == ItemKind::Weapon && !inv.has_sound_at(item, at) && !inv.is_equipped(item) && !(actor == FormId::PLAYER_REF && self.player_equips_broken) {
            return Err(SimError::Broken(item));
        }
        if self.effective_power_armor(item) && actor == FormId::PLAYER_REF && !self.power_armor_training && !inv.is_equipped(item) {
            return Err(SimError::NoPowerArmorTraining(item));
        }
        let locks = &state.equip_locks;
        if locks.get(&item) == Some(&EquipLock::NoEquip) {
            return Err(SimError::EquipLocked { item, on: false });
        }
        // "EquipItem will not work if there is already an item in that slot that has
        // been equipped with the NoUnequipFlag set" [documented: GECK wiki EquipItem].
        if let Some(held) = inv.equipped().into_iter().find(|&o| o != item && self.equip_clashes_item(item, o) && locks.get(&o) == Some(&EquipLock::NoUnequip)) {
            return Err(SimError::EquipLocked { item: held, on: true });
        }
        // Check a unit `at` names can be put on before taking anything off, so a failed
        // equip leaves the actor dressed as it was (#1083). `equip_at` frees the equipped
        // units of `item` itself when it clashes with itself.
        if !inv.can_equip_at(item, at, self.equip_clashes_item(item, item)) {
            return Err(SimError::NotCarried { actor, item });
        }
        Ok(false)
    }

    /// Unequip `item`; returns whether it was equipped. An item a script locked on
    /// ([`EquipLock::NoUnequip`]) stays on ([`World::unequip_check`]).
    pub fn unequip(&mut self, actor: FormId, item: FormId) -> Result<bool, SimError> {
        self.inventory(actor)?;
        self.unequip_check(actor, item)?;
        Ok(self.inventory(actor)?.clear_equipped(item))
    }

    /// Whether [`World::unequip`] takes `item` off `actor` (or finds it off), changing
    /// nothing: the error it returns for an item a script locked on. Only a materialised
    /// actor is read, as [`World::equip_check`].
    pub fn unequip_check(&self, actor: FormId, item: FormId) -> Result<(), SimError> {
        let worn = self.refs.get(&actor).and_then(|s| s.inventory.as_ref()).is_some_and(|i| i.is_equipped(item));
        if worn && self.equip_lock(actor, item) == Some(EquipLock::NoUnequip) {
            return Err(SimError::EquipLocked { item, on: true });
        }
        Ok(())
    }

    /// The script lock on an actor's item, if any.
    pub fn equip_lock(&self, actor: FormId, item: FormId) -> Option<EquipLock> {
        self.refs.get(&actor).and_then(|s| s.equip_locks.get(&item)).copied()
    }

    /// Set (or with `None` take away) a script's lock on an actor's item: EquipItem and
    /// UnequipItem take away the lock they undo and set the one their flag asks for
    /// [documented: GECK wiki EquipItem, UnequipItem].
    pub fn lock_equip(&mut self, actor: FormId, item: FormId, lock: Option<EquipLock>) -> Result<(), SimError> {
        let locks = &mut self.get(actor)?.equip_locks;
        match lock {
            Some(l) => locks.insert(item, l),
            None => locks.remove(&item),
        };
        Ok(())
    }

    /// An actor value: S.P.E.C.I.A.L. from the base NPC_ record unless changed at run
    /// time. (Template NPCs take stats from their template: not followed yet.)
    /// Reading never materialises the reference, so it rolls no leveled list and leaves
    /// the random number stream where it was (#1083).
    pub fn actor_value(&self, actor: FormId, name: &str) -> Result<f32, SimError> {
        let key = name.to_ascii_lowercase();
        let idx = SPECIAL.iter().position(|n| *n == key).ok_or_else(|| SimError::UnknownActorValue(name.into()))?;
        let base = match self.peek(actor) {
            Some(s) => {
                if let Some(v) = s.actor_values.get(&key) {
                    return Ok(*v);
                }
                s.base
            }
            None => self.initial_base(actor)?,
        };
        match self.forms.get(base).map(|b| &b.data) {
            Some(BaseData::Actor(a)) => a.attributes.map(|a| a[idx] as f32).ok_or(SimError::NoActorValues(actor)),
            _ => Err(SimError::NoActorValues(actor)),
        }
    }

    pub fn set_actor_value(&mut self, actor: FormId, name: &str, value: f32) -> Result<(), SimError> {
        let key = name.to_ascii_lowercase();
        self.actor_value(actor, &key)?;
        self.get(actor)?.actor_values.insert(key, value);
        Ok(())
    }

    fn setting(&self, edid: &str) -> Result<f32, SimError> {
        self.setting_f32(edid).ok_or_else(|| SimError::MissingSetting(edid.into()))
    }

    /// A game setting's number: a run-time change ([`World::change_setting`]), else the
    /// record's, else a seeded built-in engine default ([`crate::settings::builtin_default`]).
    pub fn setting_f32(&self, edid: &str) -> Option<f32> {
        self.setting_value(edid)?.as_f32()
    }

    /// A game setting as it is now: a run-time change, else the record's, else a seeded
    /// built-in engine default.
    pub fn setting_value(&self, edid: &str) -> Option<&crate::settings::Setting> {
        let k = edid.to_ascii_lowercase();
        self.setting_changes
            .get(&k)
            .or_else(|| self.forms.settings.get(&k))
            .or_else(|| crate::settings::builtin_default(&k))
    }

    /// Change a game setting for this run. It must exist (in the load order or in the
    /// seeded built-in engine defaults), and the value must be of its kind (its Editor
    /// ID's first letter: `f`, `i`, `s`).
    pub fn change_setting(&mut self, edid: &str, value: crate::settings::Setting) -> Result<(), SimError> {
        use crate::settings::Setting;
        let k = edid.to_ascii_lowercase();
        let old = self
            .forms
            .settings
            .get(&k)
            .or_else(|| crate::settings::builtin_default(&k))
            .ok_or_else(|| SimError::MissingSetting(edid.into()))?;
        let fits = matches!((old, &value), (Setting::Float(_), Setting::Float(_)) | (Setting::Int(_), Setting::Int(_)) | (Setting::Str(_), Setting::Str(_)));
        if !fits {
            return Err(SimError::SettingKind(edid.into()));
        }
        self.setting_changes.insert(k, value);
        Ok(())
    }

    /// Carried weight against capacity. Capacity = fAVDCarryWeightsBase +
    /// fAVDCarryWeightMult × Strength (150 + 10 × Strength in the masters); the actor is
    /// over-encumbered when the whole part of the weight exceeds it. [observed in game:
    /// capacity 200 at Strength 5 and 160 at Strength 1, equipped items counted — xNVSE
    /// capture, `L4.sim.trace.goodsprings_weight_and_time`; with capacity 200 the player
    /// still ran (343 units/s) carrying 199.5, 200.0 and 200.5 and walked (85 units/s,
    /// `IsRunning` 0) carrying 201.0, 201.5, 202, 203, 205 and 210.5 —
    /// `cl_b_encumbrance_speed`, `cl_b2_encumbrance_speed`, docs/spec-notes/simulation.md
    /// §3. Whole part vs. weight ≥ capacity + 1 differ only for fractional capacities:
    /// hypothesis]
    ///
    /// The capacity also holds the game layer's bonus ([`World::set_carry_bonus`]).
    fn inventory_totals(&mut self, actor: FormId) -> Result<crate::inventory::Totals, SimError> {
        self.inventory(actor)?;
        let (forms, hardcore) = (self.forms, self.cfg.hardcore);
        let rules = self.weight_rules(actor);
        let inv = self.refs.get(&actor).and_then(|s| s.inventory.as_ref()).ok_or(SimError::NoInventory(actor))?;
        Ok(inv.totals_with_overrides(forms, hardcore, &rules, |id| {
            let it = self.resolve_item(id)?;
            Some((it, self.item_override(id).and_then(|o| o.weight), self.effective_power_armor(id)))
        }))
    }

    pub fn encumbrance(&mut self, actor: FormId) -> Result<Encumbrance, SimError> {
        let capacity = self.formula_capacity(actor)? + self.carry_bonus.get(&actor).copied().unwrap_or(0.0) as f32;
        let t = self.inventory_totals(actor)?;
        Ok(Encumbrance {
            weight: t.weight,
            capacity: capacity as f64,
            over: t.weight.floor() > capacity as f64,
            unknown_weight: t.unknown_weight,
        })
    }

    /// The carry capacity from the permanent Strength alone: fAVDCarryWeightsBase +
    /// fAVDCarryWeightMult × Strength.
    pub fn formula_capacity(&mut self, actor: FormId) -> Result<f32, SimError> {
        let strength = self.actor_value(actor, "strength")?;
        Ok(self.setting("fAVDCarryWeightsBase")? + self.setting("fAVDCarryWeightMult")? * strength)
    }

    /// What the game layer adds to the capacity of [`World::formula_capacity`] (the
    /// current Carry Weight with temporary modifiers less that formula); 0 removes it.
    pub fn set_carry_bonus(&mut self, actor: FormId, bonus: f64) {
        if bonus.abs() < 1e-6 {
            self.carry_bonus.remove(&actor);
        } else {
            self.carry_bonus.insert(actor, bonus);
        }
    }

    /// The weight a reference carries, as [`World::encumbrance`] counts it.
    pub fn carried_weight(&mut self, actor: FormId) -> Result<f64, SimError> {
        Ok(self.inventory_totals(actor)?.weight)
    }

    /// What a carrier's perks do to the weight it carries (none: every item its own weight).
    pub fn set_weight_rules(&mut self, actor: FormId, rules: crate::inventory::WeightRules) {
        if rules.is_none() {
            self.weight_rules.remove(&actor);
        } else {
            self.weight_rules.insert(actor, rules);
        }
    }

    /// A carrier's weight rules ([`World::set_weight_rules`]).
    pub fn weight_rules(&self, actor: FormId) -> crate::inventory::WeightRules {
        self.weight_rules.get(&actor).copied().unwrap_or_default()
    }

    /// The worldspace a reference is in now: that of its cell (`None` in an interior).
    /// The cell is the truth: a teleport into another worldspace moves the actor into an
    /// exterior cell of that worldspace, so position, cell and worldspace change
    /// together and a snapshot keeps them (it stores the cell). Does not materialise
    /// anything.
    pub fn worldspace_of(&self, id: FormId) -> Option<FormId> {
        let cell = match self.peek(id) {
            Some(s) => s.cell,
            None => self.placed.get(id)?.cell,
        };
        self.placed.world_of(cell?)
    }

    /// Whether a reference is enabled, following its enable-parent chain. A reference
    /// with an enable parent takes the parent's effective state (inverted when the
    /// "opposite" flag is set) and its own flag is ignored; a chain that ends at a
    /// reference that does not exist, or loops, falls back to the reference's own flag.
    /// [hypothesis: GECK wiki "Enable Parent"; to be captured] Does not materialise
    /// anything, so it never consumes randomness.
    pub fn is_enabled(&self, id: FormId) -> Result<bool, SimError> {
        let own = |w: &World, id: FormId| -> Option<bool> {
            match w.refs.get(&id) {
                Some(s) => Some(s.enabled()),
                None => w.placed.get(id).map(|p| !p.initially_disabled()),
            }
        };
        let start = own(self, id).ok_or(SimError::UnknownRef(id))?;
        let mut invert = false;
        let mut cur = id;
        let mut seen = vec![id];
        loop {
            let parent = if self.own_enable.contains(&cur) { None } else { self.enable_parent(cur) };
            let Some(ep) = parent else {
                return Ok(own(self, cur).unwrap_or(start) ^ invert);
            };
            if seen.contains(&ep.reference) || own(self, ep.reference).is_none() {
                return Ok(start);
            }
            invert ^= ep.opposite;
            seen.push(ep.reference);
            cur = ep.reference;
        }
    }

    /// A reference's enable parent: SetEnableParent's for the session, else its XESP.
    pub fn enable_parent(&self, id: FormId) -> Option<crate::refs::EnableParent> {
        match self.enable_parent_edits.get(&id) {
            Some(e) => *e,
            None => self.placed.get(id).and_then(|p| p.enable_parent),
        }
    }

    /// SetEnableParent: `parent` (none: no parent) in place of the record's, not
    /// opposite, for the session; the reference follows it again even after DisableAlt
    /// or EnableAlt [hypothesis].
    pub fn set_enable_parent(&mut self, id: FormId, parent: Option<FormId>) {
        let pop_in = self.enable_parent(id).is_some_and(|e| e.pop_in);
        self.enable_parent_edits.insert(id, parent.map(|reference| crate::refs::EnableParent { reference, opposite: false, pop_in }));
        self.own_enable.remove(&id);
        self.presence_serial += 1;
    }

    /// DisableAlt / EnableAlt: from now on the reference's own flag counts, not its enable
    /// parent's state (`own_enable`).
    pub fn keep_own_enable(&mut self, id: FormId) {
        if self.own_enable.insert(id) {
            self.presence_serial += 1;
        }
    }

    /// Whether the reference's own flag counts over its enable parent
    /// ([`World::keep_own_enable`]).
    pub fn keeps_own_enable(&self, id: FormId) -> bool {
        self.own_enable.contains(&id)
    }

    /// Current value of a global.
    pub fn global(&self, id: FormId) -> Result<f32, SimError> {
        match self.globals.get(&id) {
            Some(v) => Ok(*v),
            None => self.forms.globals.get(&id).copied().ok_or(SimError::UnknownForm(id)),
        }
    }

    pub fn set_global(&mut self, id: FormId, value: f32) -> Result<(), SimError> {
        if !self.forms.globals.contains_key(&id) {
            return Err(SimError::UnknownForm(id));
        }
        self.globals.insert(id, value);
        // A write is the time now, even when it equals what the clock last stored.
        if self.time_global_ids.hour == Some(id) {
            self.fine_clock.forget_hour();
        } else if self.time_global_ids.days_passed == Some(id) {
            self.fine_clock.forget_days_passed();
        }
        Ok(())
    }

    fn time_global(&self, edid: &str) -> Result<(FormId, f32), SimError> {
        let (id, _) = self.forms.global_by_edid(edid).ok_or_else(|| SimError::MissingGlobal(edid.to_string()))?;
        Ok((id, self.global(id)?))
    }

    /// The clock as stored in the time globals.
    pub fn calendar(&self) -> Result<Calendar, SimError> {
        Ok(Calendar {
            year: self.time_global(clock::GAME_YEAR)?.1,
            month: self.time_global(clock::GAME_MONTH)?.1,
            day: self.time_global(clock::GAME_DAY)?.1,
            hour: self.time_global(clock::GAME_HOUR)?.1,
            days_passed: self.time_global(clock::GAME_DAYS_PASSED)?.1,
        })
    }

    /// Advance game time by `hours` (waiting, sleeping, fast travel). Merchant
    /// containers restock at the midnights passed ([`World::restock_merchants`]).
    pub fn advance_hours(&mut self, hours: f64) -> Result<Calendar, SimError> {
        let before = self.calendar()?;
        let c = self.fine_clock.advance(before, hours);
        self.store_calendar(before, c)?;
        Ok(c)
    }

    /// Move game time forward to `hour` (the next day's when it has passed today), landing
    /// on it; the game hours that passed, 0 when the clock shows that hour already.
    pub fn advance_to_hour(&mut self, hour: f64) -> Result<f64, SimError> {
        let before = self.calendar()?;
        let (c, hours) = self.fine_clock.advance_to_hour(before, hour);
        // A GameHour out of range can come to the hour with no time passing.
        if c != before {
            self.store_calendar(before, c)?;
        }
        Ok(hours)
    }

    fn store_calendar(&mut self, before: Calendar, c: Calendar) -> Result<(), SimError> {
        for (edid, v) in [
            (clock::GAME_YEAR, c.year),
            (clock::GAME_MONTH, c.month),
            (clock::GAME_DAY, c.day),
            (clock::GAME_HOUR, c.hour),
            (clock::GAME_DAYS_PASSED, c.days_passed),
        ] {
            let (id, _) = self.time_global(edid)?;
            self.globals.insert(id, v);
        }
        self.restock_merchants(before.days_passed as f64, c.days_passed as f64)
    }

    /// Set the clock to `c` as it stands, backwards too (a co-op guest takes the host's
    /// clock). Nothing restocks: the time between is not lived through here.
    pub fn set_calendar(&mut self, c: Calendar) -> Result<(), SimError> {
        for (edid, v) in [
            (clock::GAME_YEAR, c.year),
            (clock::GAME_MONTH, c.month),
            (clock::GAME_DAY, c.day),
            (clock::GAME_HOUR, c.hour),
            (clock::GAME_DAYS_PASSED, c.days_passed),
        ] {
            let (id, _) = self.time_global(edid)?;
            self.globals.insert(id, v);
        }
        Ok(())
    }

    /// The TimeScale global (game seconds per real second), if the plugins define it.
    pub fn time_scale(&self) -> Option<f64> {
        self.time_global(clock::TIME_SCALE).ok().map(|(_, v)| v as f64)
    }

    /// Advance game time by `seconds` of real time at the current TimeScale.
    pub fn advance_real_seconds(&mut self, seconds: f64) -> Result<Calendar, SimError> {
        let scale = self.time_global(clock::TIME_SCALE)?.1 as f64;
        self.advance_hours(seconds * scale / 3600.0)
    }

    /// The respawn delay in game hours: the config's, else the `iHoursToRespawnCell`
    /// setting from the plugins, else [`DEFAULT_RESPAWN_HOURS`].
    pub fn respawn_hours(&self) -> f32 {
        self.cfg
            .respawn_hours
            .or_else(|| self.setting_f32(RESPAWN_SETTING))
            .unwrap_or(DEFAULT_RESPAWN_HOURS)
    }

    /// The engine unloads `cell` (the player left it).
    pub fn detach_cell(&mut self, cell: FormId) -> Result<(), SimError> {
        let now = self.calendar()?.days_passed;
        self.detached_cells.insert(cell, now);
        Ok(())
    }

    /// Record a saved cell detach timestamp (`GameDaysPassed`, offline save import).
    pub fn set_cell_detached_at(&mut self, cell: FormId, days_passed: f32) {
        if days_passed.is_finite() && days_passed >= 0.0 {
            self.detached_cells.insert(cell, days_passed);
        }
    }

    /// GameDaysPassed when `cell` was last unloaded ([`World::detach_cell`]), while it
    /// is still unloaded.
    pub fn detached_since(&self, cell: FormId) -> Option<f32> {
        self.detached_cells.get(&cell).copied()
    }

    /// The engine loads `cell` again. If it stayed unloaded for at least
    /// `respawn_hours`, the containers in it whose base has the Respawns flag go back
    /// to their authored state: their next use rolls new contents, and anything left in
    /// them is lost. [hypothesis: the cell-reset behaviour commonly described for this
    /// engine generation; other references are not reset yet] Returns the containers
    /// reset.
    pub fn attach_cell(&mut self, cell: FormId) -> Result<Vec<FormId>, SimError> {
        let never_resets = self.cell_never_resets(cell);
        self.attach_cell_with_never_resets(cell, never_resets)
    }

    /// As [`World::attach_cell`], using an effective `never_resets` decision supplied by the
    /// caller (so `fnv-game` can include session `SetZoneFlag` and `SetCellEncounterZone`
    /// overrides alongside saved `SetZoneRespawns`).
    pub fn attach_cell_with_never_resets(&mut self, cell: FormId, never_resets: bool) -> Result<Vec<FormId>, SimError> {
        let Some(since) = self.detached_cells.remove(&cell) else { return Ok(Vec::new()) };
        let now = self.calendar()?.days_passed;
        // GameDaysPassed is an f32 global, so an exact delay can come out a hair short
        // (72 h measured as 71.99999 h): compare with a thousandth of an hour of slack.
        let elapsed_hours = (now as f64 - since as f64) * 24.0;
        if elapsed_hours + 1e-3 < self.respawn_hours() as f64 {
            return Ok(Vec::new());
        }
        // An encounter zone with "Never Resets" keeps its containers as they are
        // (docs/spec-notes/encounter-zones.md §4).
        if never_resets {
            return Ok(Vec::new());
        }
        Ok(self.reset_containers(cell))
    }

    /// The containers of `cell` whose base has the Respawns flag go back to their
    /// authored state now, as [`World::attach_cell`] does after the respawn delay (a
    /// script's ResetInterior). Returns the containers reset.
    pub fn reset_containers(&mut self, cell: FormId) -> Vec<FormId> {
        let reset: Vec<FormId> = self
            .placed
            .by_cell
            .get(&cell)
            .into_iter()
            .flatten()
            .copied()
            .filter(|id| self.refs.get(id).is_some_and(|s| self.container_respawns(s.base)))
            .collect();
        for id in &reset {
            if let Some(t) = self.touched.as_mut() {
                t.insert(*id);
            }
            self.refs.remove(id);
            self.touched_refs.insert(*id);
            self.presence_serial += 1;
            // Made anew: its level is fixed again when the cell is entered.
            self.ref_levels.remove(id);
        }
        reset
    }

    /// Whether `id`'s default open state is open: a runtime override ([`World::set_default_open`]),
    /// else its placement's ONAM (`open_by_default`) flag.
    pub fn default_open(&self, id: FormId) -> bool {
        self.default_open
            .get(&id)
            .copied()
            .unwrap_or_else(|| self.placed.get(id).is_some_and(|p| p.open_by_default))
    }

    /// Validated default-open query (`GetDefaultOpen`, `0x10D7`): checks that `id` is a live
    /// reference without materializing an untouched placement.
    pub fn check_default_open(&self, id: FormId) -> Result<bool, SimError> {
        if self.deleted_created.contains_key(&id) || self.refs.get(&id).is_some_and(RefState::deleted) {
            return Err(SimError::Deleted(id));
        }
        let authored = if let Some(p) = self.placed.get(id) {
            p.open_by_default
        } else if self.refs.contains_key(&id) || id == FormId::PLAYER_REF {
            false
        } else {
            return Err(SimError::UnknownRef(id));
        };
        Ok(self.default_open.get(&id).copied().unwrap_or(authored))
    }

    /// Set `id`'s default open state (`SetDefaultOpen`). Rejects unknown or deleted references
    /// without mutating state. Leaves an already-materialized reference's current `open` flag
    /// untouched (`SetOpenState` owns live open state), while unmaterialized or reset
    /// references inherit this default in [`World::initial`].
    pub fn set_default_open(&mut self, id: FormId, open: bool) -> Result<(), SimError> {
        if self.deleted_created.contains_key(&id) || self.refs.get(&id).is_some_and(RefState::deleted) {
            return Err(SimError::Deleted(id));
        }
        let authored = if let Some(p) = self.placed.get(id) {
            p.open_by_default
        } else if self.refs.contains_key(&id) || id == FormId::PLAYER_REF {
            false
        } else {
            return Err(SimError::UnknownRef(id));
        };
        if open == authored {
            self.default_open.remove(&id);
        } else {
            self.default_open.insert(id, open);
        }
        Ok(())
    }

    pub fn set_enabled(&mut self, id: FormId, enabled: bool) -> Result<(), SimError> {
        let s = self.get(id)?;
        if s.enabled != enabled {
            s.enabled = enabled;
            self.presence_serial += 1;
        }
        Ok(())
    }

    /// Mark a reference removed from the world (`deleted`: picked up, MarkForDelete, a
    /// shot that is over), or not. The field is private to this module, so a direct write
    /// from outside it does not compile and [`World::presence_serial`] sees the change.
    /// Replacing a whole state through [`World::get`] is not stopped
    /// ([`World::presence_serial`] says what that leaves open).
    pub fn set_deleted(&mut self, id: FormId, deleted: bool) -> Result<(), SimError> {
        let s = self.get(id)?;
        if s.deleted != deleted {
            s.deleted = deleted;
            self.presence_serial += 1;
        }
        Ok(())
    }

    pub fn lock(&mut self, id: FormId, level: Option<u8>) -> Result<(), SimError> {
        self.lock_serial += 1;
        let s = self.get(id)?;
        match &mut s.lock {
            Some(l) => {
                l.locked = true;
                if let Some(v) = level {
                    l.level = v;
                }
            }
            None => s.lock = Some(LockState { level: level.unwrap_or(0), key: None, locked: true }),
        }
        Ok(())
    }

    pub fn unlock(&mut self, id: FormId) -> Result<(), SimError> {
        // Unlocking through either side changes the one source lock [hypothesis,
        // #2538: capture run15b_a checked refusal and reads, not unlocking].
        let id = self.lock_source(id);
        if let Some(l) = &mut self.get(id)?.lock {
            l.locked = false;
            self.lock_serial += 1;
        }
        Ok(())
    }

    /// [hypothesis, #2538] Change the applicable source lock's key without changing
    /// its level or access state, as `unlock` shares the source. A reference with no
    /// applicable lock gets an open level-zero lock [hypothesis: existing fallback].
    pub fn set_open_key(&mut self, id: FormId, key: FormId) -> Result<(), SimError> {
        let id = self.lock_source(id);
        let s = self.get(id)?;
        s.lock.get_or_insert(LockState { level: 0, key: None, locked: false }).key = Some(key);
        self.lock_serial += 1;
        Ok(())
    }

    /// [design] Goes up with every lock change ([`World::lock`], [`World::unlock`],
    /// [`World::set_open_key`]): a reader that
    /// saw the same number has nothing new to learn about locks.
    pub fn lock_serial(&self) -> u64 {
        self.lock_serial
    }

    /// Goes up with every change that can change whether a reference is present
    /// (`fnv_game::Game::present`: not `deleted`, and [`World::is_enabled`] with its
    /// enable parents): a state made ([`World::get`]) whose flags differ from what the
    /// placement gave before (the player's; a placed reference's state starts as placed
    /// and leaves it), created ([`World::place`], drops), reset ([`World::attach_cell`]),
    /// and every write of `enabled` ([`World::set_enabled`]) or `deleted`
    /// ([`World::set_deleted`], activation's pick-ups). A reader that saw the same number
    /// of the same world reads the same presence for every reference. A new or restored
    /// world starts from a number no other world of this process used.
    ///
    /// `enabled` and `deleted` are private to this module, so a direct write of either
    /// field from outside it does not compile (issue #1464; the writers are listed in
    /// docs/spec-notes/navmesh.md §4c). That is not all of the rule: [`World::get`] still
    /// hands out `&mut RefState`, so `*world.get(id)? = another_state` compiles and would
    /// change the flags without moving this number. A search of `crates/` and `engine/`
    /// found none [design]; closing that is a separate question, left open.
    pub fn presence_serial(&self) -> u64 {
        self.presence_serial
    }

    /// The access state of a door or container lock, as the in-game `GetLocked` reports
    /// it: 0 not locked, 1 locked. [documented: GECK wiki `GetLocked`, which also lists 2
    /// "locked out" for an impossible level or a broken lock; observed in game,
    /// docs/spec-notes/simulation.md §5: a level-255 lock reads 1, both an authored one
    /// and one set with `Lock 255`. Broken locks (failed lockpicking) are Layer 6.]
    /// Read without materialising the reference ([`World::lock_of`]), as
    /// [`World::actor_value`].
    pub fn lock_status(&self, id: FormId) -> Result<u8, SimError> {
        if self.peek(id).is_none() {
            self.initial_base(id)?;
        }
        Ok(match self.lock_of(id) {
            Some(l) if l.locked => 1,
            _ => 0,
        })
    }

    /// Create a reference of any base form at run time (the script function PlaceAtMe,
    /// Layer 5): enabled, at `pos`/`rot` in `cell`, with a fresh inventory for a
    /// container or actor base. Returns its FormID (the next dynamic one, as drops).
    /// Where the game puts it exactly is the caller's choice [hypothesis].
    pub fn place(&mut self, base: FormId, cell: Option<FormId>, pos: [f32; 3], rot: [f32; 3], count: i32) -> Result<FormId, SimError> {
        if self.forms.sig(base).is_none() {
            return Err(SimError::UnknownForm(base));
        }
        let id = FormId(self.next_dynamic);
        self.next_dynamic += 1;
        self.put_new(id, base, cell, pos, rot, count)
    }

    /// [`World::place`] under a dynamic FormID the caller chooses, which no reference
    /// has: a co-op guest's copy of a reference the host's game created, under the host's
    /// number, so the two games name it alike [design]. The dynamic counter is left as it
    /// is.
    pub fn place_as(&mut self, id: FormId, base: FormId, cell: Option<FormId>, pos: [f32; 3], rot: [f32; 3]) -> Result<FormId, SimError> {
        if self.forms.sig(base).is_none() {
            return Err(SimError::UnknownForm(base));
        }
        if !id.is_dynamic() || self.refs.contains_key(&id) || self.placed.get(id).is_some() {
            return Err(SimError::Taken(id));
        }
        self.put_new(id, base, cell, pos, rot, 1)
    }

    /// Move the dynamic counter to at least `next`: references created from now on are
    /// numbered from there (a co-op guest keeps its own above the host's,
    /// [`World::place_as`]).
    pub fn number_created_from(&mut self, next: u32) {
        self.next_dynamic = self.next_dynamic.max(next);
    }

    fn put_new(&mut self, id: FormId, base: FormId, cell: Option<FormId>, pos: [f32; 3], rot: [f32; 3], count: i32) -> Result<FormId, SimError> {
        let mut inventory = self.roll_inventory(base, self.roll_level())?;
        // A created actor appears where the player is: it is outfitted at once.
        let outfitted = self.equip_default(base, &mut inventory);
        self.presence_serial += 1;
        self.touch(id);
        self.touched_refs.insert(id);
        self.refs.insert(
            id,
            RefState {
                base,
                cell,
                pos,
                rot,
                enabled: true,
                deleted: false,
                count: count.max(1),
                inventory,
                lock: None,
                open: false,
                extra: Extra::default(),
                actor_values: BTreeMap::new(),
                last_restock: None,
                outfitted,
                scale: None,
                equip_locks: BTreeMap::new(),
                dropped_by: None,
                player_drop: false,
                made_at: Some((cell, pos)),
                made_rot: Some(rot),
                shot: false,
            },
        );
        Ok(id)
    }

    /// Admit a known original's direct temporary alias without losing its identity.
    /// Inventory/actor/clone chains and changed descriptors are outside this preview.
    pub fn place_temporary_scene_base(&mut self, alias: FormId, descriptor: TemporarySceneBase, cell: Option<FormId>, pos: [f32; 3], rot: [f32; 3], count: i32) -> Result<FormId, SimError> {
        // Inert model admission only. Doors, furniture, activators and actors need
        // additional interaction/script contracts beyond a selected model path.
        let supported = self.forms.sig(descriptor.original).is_some_and(|sig| matches!(&sig, b"STAT" | b"MSTT" | b"PROJ"));
        if !supported || self.forms.sig(alias).is_some() || self.temporary_scene_bases.get(&alias).is_some_and(|old| old != &descriptor) {
            return Err(SimError::UnsupportedTemporaryBase(alias));
        }
        let reference = self.place(descriptor.original, cell, pos, rot, count)?;
        self.refs.get_mut(&reference).expect("place inserted this reference").base = alias;
        self.temporary_scene_bases.insert(alias, descriptor);
        Ok(reference)
    }

    pub fn temporary_scene_base(&self, base: FormId) -> Option<&TemporarySceneBase> {
        self.temporary_scene_bases.get(&base)
    }

    /// The preview freezes both a placed alias and the original it inherits from.
    /// Descriptors remain for the session, including deleted-reference tombstones.
    pub fn has_placed_temporary_base(&self, form: FormId) -> bool {
        self.temporary_scene_bases.contains_key(&form) || self.temporary_scene_bases.values().any(|d| d.original == form)
    }

    /// The inventory of a reference, for callers that change extra data in place
    /// (Layer 5 functions such as ModWeaponHealthPerc).
    pub fn inventory_mut(&mut self, id: FormId) -> Result<&mut Inventory, SimError> {
        self.inventory(id)
    }

    /// Add `count` of an item (not a leveled list) with the given extra data.
    pub fn add_item_extra(&mut self, to: FormId, item: FormId, count: i32, extra: Extra) -> Result<i32, SimError> {
        if count <= 0 {
            return Err(SimError::BadCount(count));
        }
        self.check_item(item)?;
        Ok(self.inventory(to)?.add_extra(item, count, extra))
    }

    /// A reference created at run time whose base stands for another actor base's carried
    /// items (a leveled actor list's pick, a template's Inventory source: the game layer
    /// finds `source`, as it does for placed actors, `set_inventory_sources`): those items,
    /// rolled now, with the armour and weapon an actor starts with equipped, as [`World::place`]
    /// outfits an actor base of its own. Without this a created leveled actor carried
    /// nothing and was drawn unarmed (the stress tests' battle, 2026-10-18: ten
    /// LvlNCRTrooper fought with their fists).
    pub fn give_items_of(&mut self, id: FormId, source: FormId) -> Result<(), SimError> {
        self.get(id)?;
        self.set_inventory_source(id, Some(source));
        let mut inv = self.roll_inventory(source, self.item_level(id))?;
        let outfitted = self.equip_default(source, &mut inv);
        let s = self.get(id)?;
        s.inventory = with_live(s.inventory.as_ref(), inv);
        s.outfitted = outfitted;
        Ok(())
    }

    /// A fresh roll of the base's carried items replaces the reference's inventory
    /// (ResetInventory, Layer 5; respawns and resurrections call it too). `false` when the
    /// base carries no items. A death item still owed is dropped: the actor starts again.
    /// A live unit (a planted explosive, `Extra::live`) stays: its plant still goes off
    /// [design, #1640].
    pub fn reset_inventory(&mut self, id: FormId) -> Result<bool, SimError> {
        self.death_items.remove(&id);
        let base = self.get(id)?.base;
        let source = self.inventory_base(id, base);
        let mut inv = self.roll_inventory(source, self.item_level(id))?;
        if self.get(id)?.outfitted {
            self.equip_default(source, &mut inv);
        }
        let s = self.get(id)?;
        let had = inv.is_some();
        if had {
            s.inventory = with_live(s.inventory.as_ref(), inv);
        }
        Ok(had)
    }

    /// The carried items of `id`'s own item list made up again: the list is rolled anew
    /// (its leveled lists with it) and of each item the roll gives, as many are added as
    /// the reference carries fewer. What it carries otherwise stays: items given to it,
    /// and items of its list it still has are not doubled. An outfitted actor puts on
    /// what was added where nothing it wears is in the way (a weapon when it holds none,
    /// armour on free body slots). The items added, with their counts.
    ///
    /// A resurrected actor that keeps its inventory (`fnv_game`'s resurrect, issue
    /// #2683): its weapon lay beside the body and its pack was looted, and the squads
    /// the shipped scripts resurrect for each new visit would come back with nothing.
    pub fn restock_inventory(&mut self, id: FormId) -> Result<Vec<(FormId, i32)>, SimError> {
        let base = self.get(id)?.base;
        let source = self.inventory_base(id, base);
        let Some(rolled) = self.roll_inventory(source, self.item_level(id))? else { return Ok(Vec::new()) };
        let outfitted = self.get(id)?.outfitted;
        let mut inv = self.get(id)?.inventory.clone().unwrap_or_default();
        let mut added: Vec<(FormId, i32)> = Vec::new();
        for (item, extra, n) in rolled.groups() {
            let short = (rolled.count(item) - inv.count(item)).min(n);
            if short > 0 {
                inv.add_extra(item, short, extra);
                match added.iter_mut().find(|(i, _)| *i == item) {
                    Some(a) => a.1 += short,
                    None => added.push((item, short)),
                }
            }
        }
        if added.is_empty() {
            return Ok(added);
        }
        if outfitted {
            for chosen in self.default_choice(source, &inv).unwrap_or_default() {
                if self.resolve_item(chosen).is_none() {
                    continue;
                }
                let free = !inv.is_equipped(chosen) && !inv.equipped().into_iter().any(|other| self.equip_clashes_item(chosen, other));
                if free && added.iter().any(|(i, _)| *i == chosen) {
                    inv.set_equipped(chosen);
                }
            }
        }
        self.get(id)?.inventory = Some(inv);
        Ok(added)
    }

    /// Empty `id`'s inventory (no inventory at all), but for its live units (planted
    /// explosives, `Extra::live`), which stay [design, #1640]: a resurrected actor whose
    /// new pick carries nothing.
    pub fn clear_inventory_but_live(&mut self, id: FormId) -> Result<(), SimError> {
        let s = self.get(id)?;
        s.inventory = with_live(s.inventory.as_ref(), None);
        Ok(())
    }

    /// References created at run time (drops), in FormID order, taken ones included
    /// (`RefState::deleted`).
    pub fn created(&self) -> impl Iterator<Item = (FormId, &RefState)> {
        self.refs.range(FormId::FIRST_DYNAMIC..).map(|(&k, v)| (k, v))
    }

    /// The created references `holder` dropped as it died ([`RefState::dropped_by`]) that
    /// are still present (not taken, enabled), in FormID order.
    pub fn dropped_beside(&self, holder: FormId) -> Vec<FormId> {
        self.created().filter(|(_, s)| s.dropped_by == Some(holder) && !s.deleted() && s.enabled()).map(|(id, _)| id).collect()
    }

    /// The newest reference created at run time (a drop) whose base is `base` and that
    /// is still present (not taken, enabled). Scenarios name it `created:<item>`
    /// because the FormID the game gives a dropped item is not known in advance.
    pub fn newest_created(&self, base: FormId) -> Option<FormId> {
        self.refs
            .range(FormId::FIRST_DYNAMIC..)
            .rev()
            .find(|(_, s)| s.base == base && !s.deleted() && s.enabled())
            .map(|(&id, _)| id)
    }

    /// `actor` activates `target` itself (the player's own activation): an item
    /// reference gives its whole stack [hypothesis; not captured]. Interactive
    /// activation can only reach what the player can see, so keeping taken or disabled
    /// references out of reach is the engine's job, not this function's. Otherwise the
    /// same as [`World::activate_scripted`].
    pub fn activate(&mut self, actor: FormId, target: FormId) -> Result<Activation, SimError> {
        self.activate_as(actor, target, false)
    }

    /// `target` is activated by the scripted `Activate` function with `actor` as the
    /// activator (`<ref>.Activate player`, how the parity traces are captured). An item
    /// reference gives **one** item, whatever its count, and keeps a count of 1 (the
    /// rest of the stack is lost). [observed in game, docs/spec-notes/simulation.md §5:
    /// a placed stack of 12 BB rounds gave 1 and then counted 1 (`GetRefCount`), a second
    /// activation gave 1 more; dropped stacks of 2 and 3 stimpaks gave 1 each;
    /// documented: GECK wiki Activate notes the loss of a stack]
    pub fn activate_scripted(&mut self, actor: FormId, target: FormId) -> Result<Activation, SimError> {
        self.activate_as(actor, target, true)
    }

    fn activate_as(&mut self, actor: FormId, target: FormId, scripted: bool) -> Result<Activation, SimError> {
        self.get(actor)?;
        let t = self.get(target)?.clone();
        let base = self.forms.get(t.base);
        let script = base.and_then(|b| b.script);
        let done = |outcome, unlocked_with| Activation { outcome, unlocked_with, script };
        // Scripted activation of an item gives its stack on every call, whether the
        // reference was already taken or is disabled, and leaves the reference's enabled
        // state alone. [observed in game, docs/spec-notes/simulation.md §5: one stimpak
        // reference activated three times gave 1, 2, 3 stimpaks; a hand-taken one gave
        // another; a never-taken cap disabled with `Disable` still gave a cap and stayed
        // disabled; GetDisabled and GetParentCell do not change after a pick-up]
        if let Some(BaseData::Item(it)) = base.map(|b| &b.data) {
            // A non-playable weapon or ammo lying in the world cannot be picked up
            // [documented: GECK wiki "Unplayable Items"; scripted activation is not
            // covered there and still gives it, hypothesis].
            if !scripted && self.effective_non_playable(t.base) && matches!(it.kind, ItemKind::Weapon | ItemKind::Ammo) {
                return Ok(done(Outcome::NoDefault { sig: base.map(|b| String::from_utf8_lossy(&b.sig).into_owned()).unwrap_or_default() }, None));
            }
            // A placed book flagged "Can't be Taken" stays where it is (skill-books.md
            // §4) [hypothesis: by the flag's name; the GECK wiki Book page calls the flag
            // unused; scripted activation still gives it, as for non-playable items].
            if !scripted && it.cant_be_taken {
                return Ok(done(Outcome::NoDefault { sig: base.map(|b| String::from_utf8_lossy(&b.sig).into_owned()).unwrap_or_default() }, None));
            }
            let count = if scripted { t.count.min(1) } else { t.count };
            // A placed weapon's own ammo (XAMT, XAMC) comes with it the first time it is
            // taken by hand [hypothesis: the rounds in it; world-fields.md §1].
            let ammo = (!scripted && !t.deleted()).then(|| self.placed.get(target).and_then(|p| p.ammo)).flatten();
            self.inventory(actor)?.add_extra(t.base, count, t.extra);
            if let Some((a, n)) = ammo {
                self.inventory(actor)?.add(a, n);
            }
            self.set_deleted(target, true)?;
            if scripted {
                self.get(target)?.count = count;
            }
            return Ok(done(Outcome::PickedUp { item: t.base, count, owner: t.extra.owner }, None));
        }
        // Other types: a taken or disabled reference does nothing. [observed by the
        // owner in game, 2026-10-08: disabled references cannot be interacted with; not
        // captured by script for doors, containers and activators]
        if t.deleted() || !self.is_enabled(target)? {
            return Ok(done(Outcome::Disabled, None));
        }
        let Some(base) = base else {
            let sig = self.forms.sig(t.base).ok_or(SimError::UnknownForm(t.base))?;
            return Ok(done(Outcome::NoDefault { sig: String::from_utf8_lossy(&sig).into_owned() }, None));
        };
        // Locks apply to doors and containers. A locked target opens if the actor
        // carries its key, without a prompt. [observed by the owner in game,
        // 2026-10-08, for a container; doors per the GECK wiki's description of keys;
        // lockpicking is Layer 6]
        let mut unlocked_with = None;
        if matches!(base.data, BaseData::Door(_) | BaseData::Container(_)) {
            if let Some(l) = self.lock_of(target).filter(|l| l.locked) {
                let skip_key = actor == FormId::PLAYER_REF && self.player_ignores_keys;
                match l.key.filter(|&k| !skip_key && self.inventory(actor).is_ok_and(|inv| inv.count(k) > 0)) {
                    Some(k) => {
                        self.unlock(target)?;
                        unlocked_with = Some(k);
                    }
                    None => return Ok(done(Outcome::Locked { level: l.level }, None)),
                }
            }
        }
        let outcome = match &base.data {
            BaseData::Item(_) => unreachable!("items are handled above"),
            BaseData::Container(_) => Outcome::OpenedContainer { container: target },
            BaseData::Door(_) => match self.placed.get(target).and_then(|p| p.teleport) {
                Some(tp) => {
                    let door_cell = match self.placed.get(tp.door) {
                        Some(d) => d.cell,
                        None => self.peek(tp.door).and_then(|d| d.cell),
                    };
                    // Into a worldspace, the actor ends up in the exterior cell that
                    // contains the destination, not necessarily the destination door's.
                    // [observed in game: leaving Doc Mitchell's house puts the player in
                    // Goodsprings, WastelandNV grid -18,0]
                    let cell = door_cell.map(|c| self.placed.exterior_cell_at(c, tp.pos).unwrap_or(c));
                    let a = self.get(actor)?;
                    a.cell = cell;
                    a.pos = tp.pos;
                    a.rot = tp.rot;
                    Outcome::Teleported { to_door: tp.door, cell, pos: tp.pos, rot: tp.rot }
                }
                None => {
                    let s = self.get(target)?;
                    s.open = !s.open;
                    Outcome::DoorToggled { open: s.open }
                }
            },
            BaseData::Actor(_) => Outcome::Talk { actor: target },
            // A talking activator (an intercom, a radio with lines) is talked to, as an
            // actor is [hypothesis: the masters' intercom scripts start the talk with the
            // default action, `fnv_game::talking_activators`].
            BaseData::Activator if &base.sig == b"TACT" => Outcome::Talk { actor: target },
            BaseData::Activator | BaseData::Furniture => Outcome::Activated,
            BaseData::LeveledItem(_) => Outcome::NoDefault { sig: "LVLI".into() },
            // A placed explosive activated by script: nothing happens. [observed in
            // game, docs/spec-notes/simulation.md §1: `<mine>.Activate player` on two
            // placed frag mines in Nipton left them enabled and in place and gave no Frag
            // Mine (`bl_placed_mines`, `L4.sim.trace.nipton_placed_mines`)]
            BaseData::Projectile { .. } if scripted => Outcome::NoDefault { sig: "PROJ".into() },
            // The player's own activation disarms it, even while its fuse burns: the mine
            // becomes the weapon that fires it, a loose item where it lay, taken with the
            // next activation [owner 2026-10-19: "one activate on a live mine will disarm
            // it you can then pick it up like any other loose item"; every mine needs
            // Explosives 1, so no skill check]. Its owner, unless the player, owns the
            // item [hypothesis].
            BaseData::Projectile { item: Some(item), .. } => {
                let owner = self.placed.get(target).and_then(|p| p.owner);
                self.set_deleted(target, true)?;
                let s = self.get(target)?;
                let (cell, pos, rot, owner) = (s.cell, s.pos, s.rot, s.extra.owner.or(owner).filter(|&o| o != FormId::PLAYER_REF));
                let placed = self.place(*item, cell, pos, rot, 1)?;
                self.get(placed)?.extra.owner = owner;
                Outcome::Disarmed { item: *item, placed: Some(placed) }
            }
            BaseData::Projectile { item: None, .. } => Outcome::NoDefault { sig: "PROJ".into() },
        };
        Ok(done(outcome, unlocked_with))
    }

    /// The base form of a created reference deleted before the last load, whose state
    /// is gone (`Snapshot::deleted_created`); `None` for any other FormID.
    pub fn deleted_base(&self, id: FormId) -> Option<FormId> {
        self.deleted_created.get(&id).copied()
    }

    /// The world's state for a save. A created reference that was deleted (picked up, a
    /// shot that is over) is kept as its FormID and base form only, not its whole state,
    /// so saves do not grow with every item dropped and picked up (#1002) [hypothesis: the
    /// original drops them too; its save M held 574 created forms while its next created
    /// FormID was 0xFF00155E, save-files.md §6.3].
    pub fn snapshot(&self) -> Snapshot {
        let gone = |id: &FormId, s: &RefState| id.is_dynamic() && s.deleted();
        let mut deleted_created = self.deleted_created.clone();
        deleted_created.extend(self.created().filter(|(id, s)| gone(id, s)).map(|(id, s)| (id, s.base)));
        let kept = |id: &FormId| !deleted_created.contains_key(id);
        let (clock_hour, clock_days_passed) = self.fine_clock.saved();
        Snapshot {
            format: SNAPSHOT_FORMAT.into(),
            config: self.cfg,
            rng_word_pos: self.rng.get_word_pos().to_string(),
            next_dynamic: self.next_dynamic,
            refs: self.refs.iter().filter(|(id, s)| !gone(id, s)).map(|(&k, v)| (k, v.clone())).collect(),
            globals: self.globals.clone(),
            clock_hour,
            clock_days_passed,
            detached_cells: self.detached_cells.clone(),
            quest_objects: self.quest_objects.clone(),
            leveled_added: self.leveled_added.clone(),
            zone_levels: self.zone_levels.clone(),
            zone_respawns: self.zone_respawns.clone(),
            ref_levels: self.ref_levels.iter().filter(|(id, _)| kept(id)).map(|(&k, &v)| (k, v)).collect(),
            death_items: self.death_items.iter().filter(|(id, _)| kept(id)).map(|(&k, &v)| (k, v)).collect(),
            // The same `kept` set as the references: a pruned one keeps no source.
            created_inventory_sources: self.created_inventory_sources.iter().filter(|(id, _)| kept(id) && self.refs.contains_key(id)).map(|(&k, &v)| (k, v)).collect(),
            power_armor_training: self.power_armor_training,
            carry_bonus: self.carry_bonus.clone(),
            weight_rules: self.weight_rules.clone(),
            default_open: self.default_open.iter().filter(|(id, _)| kept(id)).map(|(&k, &v)| (k, v)).collect(),
            deleted_created,
        }
    }

    /// Take what lasts for the session and not in a save ([`WorldSession`]), to hand it
    /// to the world a load in the same run makes ([`World::put_session`]).
    pub fn take_session(&mut self) -> WorldSession {
        WorldSession {
            setting_changes: std::mem::take(&mut self.setting_changes),
            leveled_edits: std::mem::take(&mut self.leveled_edits),
            base_edits: std::mem::take(&mut self.base_edits),
            container_respawns: std::mem::take(&mut self.container_respawns),
            zone_session: std::mem::take(&mut self.zone_session),
        }
    }

    /// Take over what lasts for the session ([`World::take_session`]).
    pub fn put_session(&mut self, s: WorldSession) {
        self.setting_changes = s.setting_changes;
        self.leveled_edits = s.leveled_edits;
        self.base_edits = s.base_edits;
        self.container_respawns = s.container_respawns;
        self.zone_session = s.zone_session;
    }

    pub fn restore(forms: &'a FormStore, placed: &'a RefTable, s: &Snapshot) -> Result<World<'a>, String> {
        s.check()?;
        let pos = s.word_pos()?;
        let mut w = World::new(forms, placed, s.config);
        w.rng.set_word_pos(pos);
        w.next_dynamic = s.next_dynamic;
        w.refs = s.refs.clone();
        w.globals = s.globals.clone();
        if let Ok(stored) = w.calendar() {
            w.fine_clock = clock::FineClock::restored(s.clock_hour.as_deref(), s.clock_days_passed.as_deref(), &stored);
        }
        w.detached_cells = s.detached_cells.clone();
        w.quest_objects = s.quest_objects.clone();
        w.leveled_added = s.leveled_added.clone();
        w.zone_levels = s.zone_levels.clone();
        w.zone_respawns = s.zone_respawns.clone();
        w.ref_levels = s.ref_levels.clone();
        w.death_items = s.death_items.clone();
        w.deleted_created = s.deleted_created.clone();
        w.created_inventory_sources = s.created_inventory_sources.clone();
        w.power_armor_training = s.power_armor_training;
        w.carry_bonus = s.carry_bonus.clone();
        w.weight_rules = s.weight_rules.clone();
        w.default_open = s.default_open.clone();
        w.made_at_from_saved_place();
        Ok(w)
    }

    /// A created actor of a save written before [`RefState::made_at`] existed (#2278
    /// came with 0.8.0) has no creation place, and kept none in every save after: its
    /// packages near the editor location failed after each load (#3042). It takes the
    /// place it was saved at, the nearest fact the save holds [decision]. That is only an
    /// approximation of where it was made: combat, following someone or a scripted MoveTo
    /// may have moved it before the save, and then it is anchored where it stood.
    /// Only actors (NPC_, CREA and the leveled actor lists placed as themselves), because
    /// a dropped item has no creation place by design ([`World::drop_item`]); every actor
    /// [`World::place`] makes has one, so `None` on a present created actor marks the
    /// older save, and the snapshot format stays `fnv.sim.state/1` (save-load.md §4).
    fn made_at_from_saved_place(&mut self) {
        let forms = self.forms;
        let actor = |base: FormId| matches!(forms.sig(base).as_ref(), Some(b"NPC_" | b"CREA" | b"LVLN" | b"LVLC"));
        for (_, s) in self.refs.range_mut(FormId::FIRST_DYNAMIC..) {
            if s.made_at.is_none() && !s.deleted && actor(s.base) {
                s.made_at = Some((s.cell, s.pos));
            }
        }
    }
}
