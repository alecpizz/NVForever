//! JIP LN NVSE, batch 7 (#1006): ammunition, ammo effects, projectiles, explosions, impact
//! data, weathers and climates, armour [documented: each page in the wiki export;
//! docs/spec-notes/jip-records-7.md]. Setters change the form for the session, read by the
//! getters (this file's and the earlier batches').

use crate::game::{Game, RunCtx};
use crate::nvse::{field_number, F32, U8};
use fnv_script::vm::XVal;
use fnv_sim::FormId;
use serde_json::Value as Json;
use std::collections::HashMap;

/// The supported session-only edits needed when a new shot is created.
/// `original` supplies the typed record; the requested clone retains its own identity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectileLaunchTraits {
    pub original: FormId,
    pub speed: Option<f32>,
    pub impact_force: Option<f32>,
    pub flags: Option<u16>,
}

/// EXPL DATA flag bits [documented: schemas/fnv/combat.json `ExplosionData.flags`,
/// GECK wiki `GetExplosionFlag` / `SetExplosionFlag`].
pub const EXPL_FLAG_RADIUS_BS_UNITS: u32 = 1 << 0;
pub const EXPL_FLAG_ALWAYS_WORLD_ORIENTATION: u32 = 1 << 1;
pub const EXPL_FLAG_KNOCK_DOWN_ALWAYS: u32 = 1 << 2;
pub const EXPL_FLAG_KNOCK_DOWN_BY_FORMULA: u32 = 1 << 3;
pub const EXPL_FLAG_IGNORE_LOS_CHECK: u32 = 1 << 4;
pub const EXPL_FLAG_PUSH_SOURCE_REF_ONLY: u32 = 1 << 5;
pub const EXPL_FLAG_IGNORE_IMAGE_SPACE_SWAP: u32 = 1 << 6;

/// Canonical live session view of an `EXPL` record (or `TempCloneForm` clone of one),
/// combining `Game::field_overrides` (`SetExplosionTraitForm`, `SetExplosionFlag`,
/// `SetExplosionTraitNumeric`, `SetExplosionSound`, `SetModelPath`) over the authored
/// ESM record (`Game::explosion_traits`).
#[derive(Debug, Clone, PartialEq)]
pub struct ExplosionTraits {
    /// Authored ESM `EXPL` form (identical to the queried id when not a clone).
    pub original: FormId,
    pub force: f32,
    pub damage: f32,
    pub radius: f32,
    pub is_radius: f32,
    pub radiation_level: f32,
    pub radiation_dissipation_time: f32,
    pub radiation_radius: f32,
    pub sound_level: u8,
    pub flags: u32,
    pub light: Option<FormId>,
    pub sound_1: Option<FormId>,
    pub sound_2: Option<FormId>,
    pub impact_dataset: Option<FormId>,
    pub placed_impact: Option<FormId>,
    pub enchantment: Option<FormId>,
    pub image_space_modifier: Option<FormId>,
    pub model: Option<String>,
}

impl ExplosionTraits {
    /// Whether flag bit `bit` (0..32) is set in `DATA/flags`.
    pub fn has_flag(&self, bit: u8) -> bool {
        bit < 32 && (self.flags & (1u32 << bit)) != 0
    }

    pub fn radius_in_bs_units(&self) -> bool {
        self.flags & EXPL_FLAG_RADIUS_BS_UNITS != 0
    }

    pub fn always_world_orientation(&self) -> bool {
        self.flags & EXPL_FLAG_ALWAYS_WORLD_ORIENTATION != 0
    }

    pub fn knock_down_always(&self) -> bool {
        self.flags & EXPL_FLAG_KNOCK_DOWN_ALWAYS != 0
    }

    pub fn knock_down_by_formula(&self) -> bool {
        self.flags & EXPL_FLAG_KNOCK_DOWN_BY_FORMULA != 0
    }

    pub fn ignore_los(&self) -> bool {
        self.flags & EXPL_FLAG_IGNORE_LOS_CHECK != 0
    }

    pub fn push_source_only(&self) -> bool {
        self.flags & EXPL_FLAG_PUSH_SOURCE_REF_ONLY != 0
    }

    pub fn ignore_image_space_swap(&self) -> bool {
        self.flags & EXPL_FLAG_IGNORE_IMAGE_SPACE_SWAP != 0
    }
}

/// AMMO DATA flag bits [documented: schemas/fnv/objects.json `AMMO.DATA.flags`].
pub const AMMO_FLAG_IGNORES_NORMAL_WEAPON_RESISTANCE: u8 = 1 << 0;
pub const AMMO_FLAG_NON_PLAYABLE: u8 = 1 << 1;

/// Canonical live session view of an `AMMO` record (or `TempCloneForm` clone of one),
/// combining `Game::field_overrides`, `Jip7State::ammo_effects`, `Game::playable_overrides`,
/// and `Game::name_overrides` over the authored ESM record (`Game::ammo_traits`).
#[derive(Debug, Clone, PartialEq)]
pub struct AmmoTraits {
    /// Authored ESM `AMMO` form (identical to the queried id when not a clone).
    pub original: FormId,
    pub speed: f32,
    pub flags: u8,
    pub value: i32,
    pub clip_rounds: u8,
    pub projectiles_per_shot: i32,
    pub projectile: Option<FormId>,
    pub weight: f32,
    pub casing: Option<FormId>,
    pub consumed_percentage: f32,
    pub effects: Vec<FormId>,
    pub full_name: Option<String>,
    pub short_name: Option<String>,
    pub abbrev: Option<String>,
}

impl AmmoTraits {
    /// Whether flag bit `bit` (0..8) is set in `DATA/flags`.
    pub fn has_flag(&self, bit: u8) -> bool {
        bit < 8 && (self.flags & (1u8 << bit)) != 0
    }

    pub fn ignores_normal_weapon_resistance(&self) -> bool {
        self.flags & AMMO_FLAG_IGNORES_NORMAL_WEAPON_RESISTANCE != 0
    }

    pub fn non_playable(&self) -> bool {
        self.flags & AMMO_FLAG_NON_PLAYABLE != 0
    }

    pub fn is_playable(&self) -> bool {
        !self.non_playable()
    }
}

/// Canonical live session view of an `AMEF` record (or `TempCloneForm` clone of one),
/// combining `SetAmmoEffectTraitNumeric` / `SetAmmoEffectTraitNumericAlt` overrides over
/// the authored ESM record (`Game::ammo_effect_traits`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AmmoEffectTraits {
    /// Authored ESM `AMEF` form (identical to the queried id when not a clone).
    pub original: FormId,
    /// 0 Damage Mod, 1 DR Mod, 2 DT Mod, 3 Spread Mod, 4 Weapon Condition Mod,
    /// 5 Fatigue Mod (plus One111 6..=9).
    pub effect_type: u8,
    /// 0 Add, 1 Multiply, 2 Subtract.
    pub operation: u8,
    pub value: f32,
}

impl AmmoEffectTraits {
    /// Apply this ammo effect's operation to `base`.
    pub fn apply(&self, base: f64) -> f64 {
        let v = f64::from(self.value);
        match self.operation {
            0 => base + v,
            1 => base * v,
            2 => base - v,
            _ => base,
        }
    }

    /// Combat kind label matching `fnv_combat::data::AmmoEffect::kind` for vanilla types 0..=5.
    pub fn kind_label(&self) -> Option<&'static str> {
        Some(match self.effect_type {
            0 => "Damage Mod",
            1 => "DR Mod",
            2 => "DT Mod",
            3 => "Spread Mod",
            4 => "Weapon Condition Mod",
            5 => "Fatigue Mod",
            _ => return None,
        })
    }

    /// Combat operation label matching `fnv_combat::data::AmmoEffect::operation`.
    pub fn operation_label(&self) -> Option<&'static str> {
        Some(match self.operation {
            0 => "Add",
            1 => "Multiply",
            2 => "Subtract",
            _ => return None,
        })
    }
}

/// Effective non-flag `PROJ` record traits for a projectile (or `TempCloneForm` `PROJ`
/// alias), resolving active session overrides (`SetProjectileTraitNumeric`,
/// `SetProjectileExplosion`, `SetProjectileLight`, `SetProjectileMuzzleFlash`,
/// `SetProjectileSound`) before authored record fields ([`Game::projectile_traits`]).
///
/// `DATA/flags` is intentionally excluded (owned separately by projectile flag helpers).
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectileTraits {
    /// Authored `PROJ` record that backs this form (`unclone(projectile)`).
    pub original: FormId,
    /// `DATA/type` code (0 none, 1 Missile, 2 Lobber, 4 Beam, 8 Flame, 16 Continuous Beam).
    pub kind_code: u16,
    /// `DATA/type` label matching `fnv_combat::data::ProjectileStats::kind`.
    pub kind: Option<&'static str>,
    /// `DATA/gravity` (trait 1).
    pub gravity: f32,
    /// `DATA/speed` (trait 2).
    pub speed: f32,
    /// `DATA/range` (trait 3).
    pub range: f32,
    /// `DATA/tracer_chance` (trait 4).
    pub tracer_chance: f32,
    /// `DATA/explosion_alt_trigger_proximity` (trait 5).
    pub alt_trigger_proximity: f32,
    /// `DATA/explosion_alt_trigger_timer` (trait 6).
    pub alt_trigger_timer: f32,
    /// `DATA/muzzle_flash_duration` (trait 7).
    pub muzzle_flash_duration: f32,
    /// `DATA/fade_duration` (trait 8).
    pub fade_duration: f32,
    /// `DATA/impact_force` (trait 9).
    pub impact_force: f32,
    /// `DATA/rotation.{x,y,z}` (traits 10–12).
    pub rotation: [f32; 3],
    /// `DATA/bouncy_mult` (trait 13).
    pub bouncy_mult: f32,
    /// `VNAM` detection sound level code (0 Loud, 1 Normal, 2 Silent; trait 14).
    pub sound_level_code: u8,
    /// `VNAM` label matching `fnv_combat::data::ProjectileStats::sound_level`.
    pub sound_level: Option<&'static str>,
    /// `DATA/explosion` (`SetProjectileExplosion` / `GetProjectileExplosion`).
    pub explosion: Option<FormId>,
    /// `DATA/light` (`SetProjectileLight` / `GetProjectileLight`).
    pub light: Option<FormId>,
    /// `NAM1` muzzle-flash model path (`SetProjectileMuzzleFlash` / `GetProjectileMuzzleFlash`).
    pub muzzle_flash_model: Option<String>,
    /// `DATA/sound` (`SetProjectileSound proj 1 sound`).
    pub sound: Option<FormId>,
    /// `DATA/sound_countdown` (`SetProjectileSound proj 2 sound`).
    pub sound_countdown: Option<FormId>,
    /// `DATA/sound_disable` (`SetProjectileSound proj 3 sound`).
    pub sound_disable: Option<FormId>,
}

/// Sparse session-only non-flag `PROJ` overrides directly on a projectile (including
/// overrides snapshotted at `TempCloneForm` creation time), `None` for any field not
/// overridden ([`Game::projectile_trait_overrides`]).
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectileTraitOverrides {
    pub original: FormId,
    pub kind: Option<Option<&'static str>>,
    pub gravity: Option<f32>,
    pub speed: Option<f32>,
    pub range: Option<f32>,
    pub tracer_chance: Option<f32>,
    pub alt_trigger_proximity: Option<f32>,
    pub alt_trigger_timer: Option<f32>,
    pub muzzle_flash_duration: Option<f32>,
    pub fade_duration: Option<f32>,
    pub impact_force: Option<f32>,
    pub rotation: [Option<f32>; 3],
    pub bouncy_mult: Option<f32>,
    pub sound_level: Option<Option<&'static str>>,
    pub explosion: Option<Option<FormId>>,
    pub light: Option<Option<FormId>>,
    pub muzzle_flash_model: Option<Option<String>>,
    pub sound: Option<Option<FormId>>,
    pub sound_countdown: Option<Option<FormId>>,
    pub sound_disable: Option<Option<FormId>>,
}

/// Exact backup of the two supported launch fields for a bounded engine diagnostic.
/// It cannot expose or replace unrelated record edits, and cannot restore into a
/// different Game even if an allocator reuses that Game's former address.
#[derive(Debug)]
pub struct ProjectileLaunchFieldLease {
    owner: std::sync::Arc<()>,
    projectile: FormId,
    before: Option<[Option<XVal>; 2]>,
    effective: [f64; 2],
}

impl ProjectileLaunchFieldLease {
    pub fn belongs_to(&self, game: &Game<'_>) -> bool {
        std::sync::Arc::ptr_eq(&self.owner, &game.projectile_launch_restore_owner)
    }

    pub fn effective_values(&self) -> [f64; 2] { self.effective }

    /// Restore exact entry presence and values once. Wrong-owner rejection leaves
    /// the capsule available for cleanup by its actual owner.
    pub fn restore(&mut self, game: &mut Game<'_>) -> Result<(), String> {
        if !self.belongs_to(game) { return Err("projectile field lease belongs to another Game".into()); }
        let before = self.before.take().ok_or("projectile field lease was already restored")?;
        for (field, before) in ["DATA/speed", "DATA/impact_force"].into_iter().zip(before) {
            let key = (self.projectile, field.to_string());
            match before {
                Some(value) => { game.field_overrides.insert(key, value); }
                None => { game.field_overrides.remove(&key); }
            }
        }
        Ok(())
    }
}

/// One climate weather (WLST): the weather, its chance, the global that gates it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClimateWeather {
    pub weather: FormId,
    pub chance: i64,
    pub global: Option<FormId>,
}

/// What this batch keeps for the session (nothing is saved [hypothesis], except where a
/// page says so: SetDamageToArmorMaxPercent "is saved in the game" [documented], kept in
/// the session only for now).
#[derive(Debug, Default)]
pub struct Jip7State {
    /// AddAmmoEffect's and RemoveAmmoEffect's lists, by ammunition (`Game::ammo_effects`).
    pub ammo_effects: HashMap<FormId, Vec<FormId>>,
    /// ClimateAddWeatherType's and ClimateRemoveWeatherType's lists, by climate
    /// (`Game::climate_weathers`).
    pub climate_weathers: HashMap<FormId, Vec<ClimateWeather>>,
    /// SetWeatherTransitionTimeOverride's seconds (`None`: the weathers' own).
    pub weather_transition: Option<f64>,
    /// SetArmorConditionPenalty's multiplier (`None`: the default 1.0).
    pub armor_condition_penalty: Option<f64>,
    /// SetDamageToArmorMaxPercent's share (`None`: not set).
    pub damage_to_armor_max: Option<f64>,
    /// SetImpactDataTraitNumeric 13's flags as set, read back by GetImpactDataTrait 16.
    pub impact_flags: HashMap<FormId, u32>,
}

fn form(args: &[XVal], k: usize) -> Option<FormId> {
    match args.get(k)? {
        XVal::Ref(0) => None,
        XVal::Ref(r) => Some(FormId(*r)),
        XVal::Num(x) if *x >= 1.0 && x.fract() == 0.0 => Some(FormId(*x as u32)),
        _ => None,
    }
}

fn num(args: &[XVal], k: usize, dflt: f64) -> f64 {
    match args.get(k) {
        Some(XVal::Num(x)) => *x,
        _ => dflt,
    }
}

fn text(args: &[XVal], k: usize) -> Option<String> {
    match args.get(k) {
        Some(XVal::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

/// [`field_number`] for a field of this range, whole for a field that is not an f32 (an
/// integer or an enumeration).
fn fit(v: f64, range: (f64, f64)) -> Option<f64> {
    let v = field_number(v, range.0, range.1)?;
    Some(if range == F32 { v } else { v.trunc() })
}

/// How a trait setter takes a number the field cannot hold, as the original showed
/// [observed: jip7, jip7b, 2026-11-08].
#[derive(Clone, Copy, PartialEq)]
enum Take {
    /// clamped to the range (`fit`)
    Clamp((f64, f64)),
    /// cut to a whole number and kept as a byte: 300 reads 44, -1 reads 255
    Wrap8,
    /// cut to a whole number, sign kept, no range
    Int,
    /// an f32 that refuses a negative number
    NonNeg,
    /// only these values; any other sets nothing
    OneOf(&'static [f64]),
}

fn take(v: f64, how: Take) -> Option<f64> {
    if !v.is_finite() {
        return None;
    }
    match how {
        Take::Clamp(r) => fit(v, r),
        Take::Wrap8 => Some(f64::from(v.trunc() as i64 as u8)),
        Take::Int => Some(v.trunc()),
        Take::NonNeg => (v >= 0.0).then_some(v),
        Take::OneOf(ok) => ok.contains(&v).then_some(v),
    }
}

fn yes(b: bool) -> XVal {
    XVal::Num(f64::from(u8::from(b)))
}

fn json_form(v: Option<&Json>) -> Option<FormId> {
    v.and_then(Json::as_str).and_then(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).ok()).filter(|&f| f != 0).map(FormId)
}

fn amef_type_code(label: &str) -> Option<u8> {
    Some(match label.to_ascii_lowercase().as_str() {
        "damage mod" => 0,
        "dr mod" => 1,
        "dt mod" => 2,
        "spread mod" => 3,
        "weapon condition mod" => 4,
        "fatigue mod" => 5,
        _ => return None,
    })
}

fn amef_op_code(label: &str) -> Option<u8> {
    Some(match label.to_ascii_lowercase().as_str() {
        "add" => 0,
        "multiply" => 1,
        "subtract" => 2,
        _ => return None,
    })
}

/// AMMO numeric traits 0-3 [documented: SetAmmoTraitNumeric's table: 0 speed, 1
/// projectiles per shot, 2 ammo consumed percent, 3 clip rounds]; the fields: xEdit AMMO.
/// Each with its field's range (u32 projectiles, u8 clip rounds).
// Projectiles per shot keeps its sign (-5 reads -5, -2.5 reads -2) and the clip rounds wrap
// as a byte (300 reads 44) [observed: jip7 ammo.trait.*, jip7b ammo.clip.*, ammo.pps.*].
const AMMO_NUMERIC: [(&str, &str, Take); 4] =
    [("DATA", "speed", Take::Clamp(F32)), ("DAT2", "projectiles_per_shot", Take::Int), ("DAT2", "consumed_percentage", Take::Clamp(F32)), ("DATA", "clip_rounds", Take::Wrap8)];

/// AMEF numeric traits 0-2 [documented: SetAmmoEffectTraitNumeric's table] as
/// GetAmmoEffectTraitNumeric reads them (`nvse_gaps`).
/// The type is 0-5 and the operation 0-2 [documented: the page's tables].
// A type past 0-5 and a negative value set nothing [observed: jip7 amef.trait.0.9, jip7b amef.*].
const AMEF_NUMERIC: [(&str, Take); 3] =
    [("type", Take::OneOf(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0])), ("operation", Take::Clamp((0.0, 2.0))), ("value", Take::NonNeg)];

/// PROJ numeric traits 0-14 [documented: SetProjectileTraitNumeric's table]; 14 the
/// detection sound level is VNAM [the field: xEdit PROJ].
/// The type is one of 1-16 and the sound level 0-2 [documented: the page's tables]; the
/// rest are f32 fields.
// The type takes 0 or one of the type bits 1, 2, 4, 8, 16 (3 and 17 set nothing); the
// sound level only 1 or 2 (0, 3, 5 and -1 set nothing) [observed: jip7 proj.settrait.*,
// jip7b proj.*].
const PROJ_NUMERIC: [(&str, &str, Take); 15] = [
    ("DATA", "type", Take::OneOf(&[0.0, 1.0, 2.0, 4.0, 8.0, 16.0])),
    ("DATA", "gravity", Take::Clamp(F32)),
    ("DATA", "speed", Take::Clamp(F32)),
    ("DATA", "range", Take::Clamp(F32)),
    ("DATA", "tracer_chance", Take::Clamp(F32)),
    ("DATA", "explosion_alt_trigger_proximity", Take::Clamp(F32)),
    ("DATA", "explosion_alt_trigger_timer", Take::Clamp(F32)),
    ("DATA", "muzzle_flash_duration", Take::Clamp(F32)),
    ("DATA", "fade_duration", Take::Clamp(F32)),
    ("DATA", "impact_force", Take::Clamp(F32)),
    ("DATA", "rotation.x", Take::Clamp(F32)),
    ("DATA", "rotation.y", Take::Clamp(F32)),
    ("DATA", "rotation.z", Take::Clamp(F32)),
    ("DATA", "bouncy_mult", Take::Clamp(F32)),
    ("VNAM", "", Take::OneOf(&[1.0, 2.0])),
];

/// `PROJ DATA/type` enum label matching `schemas/fnv/combat.json` `ProjectileData.type`
/// and `fnv_combat::data::ProjectileStats::kind` [documented: xEdit PROJ; GECK wiki
/// GetProjectileTraitNumeric].
fn projectile_kind_label(code: u16) -> Option<&'static str> {
    match code {
        1 => Some("Missile"),
        2 => Some("Lobber"),
        4 => Some("Beam"),
        8 => Some("Flame"),
        16 => Some("Continuous Beam"),
        _ => None,
    }
}

/// `PROJ VNAM` sound level label matching `schemas/fnv/objects.json` `SoundLevel`
/// and `fnv_combat::data::ProjectileStats::sound_level` [documented: xEdit PROJ; GECK wiki
/// GetProjectileTraitNumeric].
fn projectile_sound_level_label(code: u8) -> Option<&'static str> {
    match code {
        0 => Some("Loud"),
        1 => Some("Normal"),
        2 => Some("Silent"),
        _ => None,
    }
}

/// Parses an integer trait index at `args[k]`, rejecting non-finite, fractional, or
/// negative values.
fn parse_trait_index(args: &[XVal], k: usize) -> Option<usize> {
    match args.get(k)? {
        XVal::Num(x) if x.is_finite() && x.fract() == 0.0 && *x >= 0.0 => usize::try_from(*x as i64).ok(),
        _ => None,
    }
}

/// Parses an optional form argument (`SetProjectileLight`):
/// - `Some(None)` when omitted or explicitly null (`0`) -> clear.
/// - `Some(Some(id))` when a non-zero form/integer ID is supplied -> validate signature.
/// - `None` when malformed (string, negative/fractional number, etc.) -> reject cleanly.
fn parse_optional_form_arg(args: &[XVal], k: usize) -> Option<Option<FormId>> {
    match args.get(k) {
        None | Some(XVal::Ref(0)) => Some(None),
        Some(XVal::Num(x)) if *x == 0.0 => Some(None),
        Some(XVal::Ref(r)) => Some(Some(FormId(*r))),
        Some(XVal::Num(x)) if x.is_finite() && *x >= 1.0 && *x <= f64::from(u32::MAX) && x.fract() == 0.0 => Some(Some(FormId(*x as u32))),
        _ => None,
    }
}

fn xval_finite_f32(v: &XVal) -> Option<f32> {
    match v {
        XVal::Num(n) if n.is_finite() && (*n as f32).is_finite() => Some(*n as f32),
        _ => None,
    }
}

fn xval_opt_form(v: &XVal) -> Option<Option<FormId>> {
    match v {
        XVal::Ref(id) => Some((*id != 0).then_some(FormId(*id))),
        XVal::Num(n) if n.is_finite() && *n >= 0.0 && *n <= f64::from(u32::MAX) && n.fract() == 0.0 => Some((*n != 0.0).then_some(FormId(*n as u32))),
        _ => None,
    }
}

/// EXPL numeric traits 0-7 [documented: SetExplosionTraitNumeric's table].
/// f32 fields but the sound level, 0-2 [documented: the sound levels of the projectile
/// page].
// The sound level wraps as a byte with no range (9 reads 9, 300 reads 44) [observed: jip7
// expl.settrait.7.9, jip7b expl.7.*].
const EXPL_NUMERIC: [(&str, Take); 8] = [
    ("force", Take::Clamp(F32)),
    ("damage", Take::Clamp(F32)),
    ("radius", Take::Clamp(F32)),
    ("is_radius", Take::Clamp(F32)),
    ("radiation_level", Take::Clamp(F32)),
    ("radiation_dissipation_time", Take::Clamp(F32)),
    ("radiation_radius", Take::Clamp(F32)),
    ("sound_level", Take::Wrap8),
];

/// IPCT numeric traits of SetImpactDataTraitNumeric, 0-14 [documented: its table]: the
/// impact data (DATA) 0-4, the decal data (DODT) 5-12; 13 the flags and 14 the colour are
/// worked out apart. GetImpactDataTrait numbers the same with the three forms (texture
/// set, sounds 1 and 2) at 5-7, so its 8-17 are these 5-14 [documented: its table].
/// The orientation and sound level are 0-2 [documented: the page's table], the passes a
/// u8; the rest f32 fields.
const IPCT_NUMERIC: [(&str, &str, (f64, f64)); 13] = [
    ("DATA", "effect_duration", F32),
    ("DATA", "effect_orientation", (0.0, 2.0)),
    ("DATA", "angle_threshold", F32),
    ("DATA", "placement_radius", F32),
    ("DATA", "sound_level", (0.0, 2.0)),
    ("DODT", "min_width", F32),
    ("DODT", "max_width", F32),
    ("DODT", "min_height", F32),
    ("DODT", "max_height", F32),
    ("DODT", "depth", F32),
    ("DODT", "shininess", F32),
    ("DODT", "parallax_scale", F32),
    ("DODT", "parallax_passes", U8),
];

/// IPCT form traits 0-2 [documented: SetImpactDataTraitForm's table].
const IPCT_FORMS: [&str; 3] = ["DNAM", "SNAM", "NAM1"];

/// WTHR cloud texture layers 0-3 [documented: Get/SetWeatherTexture, layers 0-3; the
/// fields: xEdit WTHR DNAM, CNAM, ANAM, BNAM].
const WEATHER_LAYERS: [&str; 4] = ["DNAM", "CNAM", "ANAM", "BNAM"];

/// ARMO BMDT general flags: bit 3 Medium, bit 7 Heavy [the bits: xEdit ARMO]; neither
/// is Light.
const MEDIUM: u64 = 1 << 3;
const HEAVY: u64 = 1 << 7;

impl Game<'_> {
    /// An ammunition's effects (RCIL), AddAmmoEffect's and RemoveAmmoEffect's changes
    /// first. Follows `TempCloneForm` originals when the clone has no list override yet.
    pub fn ammo_effects(&self, ammo: FormId) -> Vec<FormId> {
        if let Some(v) = self.other.jip7.ammo_effects.get(&ammo) {
            return v.clone();
        }
        self.data.record_all(self.unclone(ammo), "RCIL").into_iter().filter_map(|v| json_form(Some(v))).collect()
    }

    /// Resolve an `AMEF` form's (or `TempCloneForm` clone's) live numeric traits
    /// (`SetAmmoEffectTraitNumeric` / `SetAmmoEffectTraitNumericAlt` over authored `DATA`).
    pub fn ammo_effect_traits(&self, effect: FormId) -> Option<AmmoEffectTraits> {
        let original = self.unclone(effect);
        if self.data.sig(original) != Some(*b"AMEF") {
            return None;
        }
        let effect_type = match crate::nvse::record_value(self, effect, "DATA", "type") {
            Some(XVal::Num(n)) if n.is_finite() && n >= 0.0 => n.trunc().min(255.0) as u8,
            Some(XVal::Str(s)) => amef_type_code(&s).unwrap_or(0),
            _ => 0,
        };
        let operation = match crate::nvse::record_value(self, effect, "DATA", "operation") {
            Some(XVal::Num(n)) if n.is_finite() && n >= 0.0 => n.trunc().min(255.0) as u8,
            Some(XVal::Str(s)) => amef_op_code(&s).unwrap_or(0),
            _ => 0,
        };
        let value = match crate::nvse::record_value(self, effect, "DATA", "value") {
            Some(XVal::Num(n)) if n.is_finite() && (n as f32).is_finite() => n as f32,
            _ => 0.0,
        };
        Some(AmmoEffectTraits { original, effect_type, operation, value })
    }

    /// Resolve every active `AMEF` attached to `ammo` (respecting `AddAmmoEffect`,
    /// `RemoveAmmoEffect`, and live `AMEF` trait overrides).
    pub fn ammo_effect_defs(&self, ammo: FormId) -> Vec<AmmoEffectTraits> {
        self.ammo_effects(ammo).into_iter().filter_map(|e| self.ammo_effect_traits(e)).collect()
    }

    /// Apply all live ammo effects of `ammo` matching `effect_type` to `base`, in order.
    pub fn apply_ammo_effects_value(&self, ammo: FormId, effect_type: u8, mut base: f64) -> f64 {
        for eff in self.ammo_effect_defs(ammo) {
            if eff.effect_type == effect_type {
                base = eff.apply(base);
            }
        }
        base
    }

    /// Resolve the effective session traits, flags, projectile/casing, names, and effects
    /// of an `AMMO` form (or `TempCloneForm` clone of one), reading `Game::field_overrides`,
    /// `Jip7State::ammo_effects`, `Game::playable_overrides`, and `Game::name_overrides`
    /// over the authored ESM record. Returns `None` for non-`AMMO` forms.
    pub fn ammo_traits(&self, ammo: FormId) -> Option<AmmoTraits> {
        let original = self.unclone(ammo);
        if self.data.sig(original) != Some(*b"AMMO") {
            return None;
        }
        let f32_field = |sub: &str, path: &str| match crate::nvse::record_value(self, ammo, sub, path) {
            Some(XVal::Num(n)) if n.is_finite() && (n as f32).is_finite() => n as f32,
            _ => 0.0,
        };
        let form_field = |sub: &str, path: &str| match crate::nvse::record_value(self, ammo, sub, path) {
            Some(XVal::Ref(id)) if id != 0 => Some(FormId(id)),
            _ => None,
        };
        let mut flags = match crate::nvse::record_value(self, ammo, "DATA", "flags") {
            Some(XVal::Num(n)) if n.is_finite() && n >= 0.0 => (n as u64 & 0xFF) as u8,
            _ => 0,
        };
        if let Some(&playable) = self.playable_overrides.get(&ammo) {
            if playable {
                flags &= !AMMO_FLAG_NON_PLAYABLE;
            } else {
                flags |= AMMO_FLAG_NON_PLAYABLE;
            }
        }
        let value = match crate::nvse::record_value(self, ammo, "DATA", "value") {
            Some(XVal::Num(n)) if n.is_finite() => n.trunc().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32,
            _ => 0,
        };
        let clip_rounds = match crate::nvse::record_value(self, ammo, "DATA", "clip_rounds") {
            Some(XVal::Num(n)) if n.is_finite() => (n.trunc() as i64).rem_euclid(256) as u8,
            _ => 0,
        };
        let projectiles_per_shot = match crate::nvse::record_value(self, ammo, "DAT2", "projectiles_per_shot") {
            Some(XVal::Num(n)) if n.is_finite() => n.trunc().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32,
            _ => 0,
        };
        let name_sub = |sub: &str| -> Option<String> {
            if let Some(XVal::Str(s)) = self.field_overrides.get(&(ammo, format!("{sub}/"))) {
                return Some(s.clone());
            }
            if sub == "FULL" {
                if let Some(s) = self.name_overrides.get(&ammo) {
                    return Some(s.clone());
                }
            }
            self.data
                .record_field(original, sub, "")
                .and_then(Json::as_str)
                .map(str::to_string)
                .or_else(|| (sub == "FULL").then(|| self.data.full_names.get(&original).cloned()).flatten())
        };
        Some(AmmoTraits {
            original,
            speed: f32_field("DATA", "speed"),
            flags,
            value,
            clip_rounds,
            projectiles_per_shot,
            projectile: form_field("DAT2", "projectile"),
            weight: f32_field("DAT2", "weight"),
            casing: form_field("DAT2", "consumed_ammo"),
            consumed_percentage: f32_field("DAT2", "consumed_percentage"),
            effects: self.ammo_effects(ammo),
            full_name: name_sub("FULL"),
            short_name: name_sub("ONAM"),
            abbrev: name_sub("QNAM"),
        })
    }

    /// Read a single `AMMO` flag bit (`0..8`: bit 0 Ignores Normal Weapon Resistance,
    /// bit 1 Non-Playable) with session overrides applied; `None` for non-`AMMO` forms
    /// or bits `>= 8`.
    pub fn ammo_flag(&self, ammo: FormId, bit: u8) -> Option<bool> {
        if bit >= 8 {
            return None;
        }
        Some(self.ammo_traits(ammo)?.has_flag(bit))
    }

    /// Snapshot all canonical `AMMO` or `AMEF` fields from `source` onto `clone` at
    /// `TempCloneForm` creation time so pre-clone edits on `source` carry onto `clone`
    /// while post-clone edits on `source` stay isolated from already-created clones.
    pub(crate) fn snapshot_ammo_traits(&mut self, source: FormId, clone: FormId) {
        let original = self.unclone(source);
        match self.data.sig(original) {
            Some(sig) if &sig == b"AMMO" => {
                let Some(t) = self.ammo_traits(source) else { return };
                self.field_overrides.insert((clone, "DATA/speed".into()), XVal::Num(f64::from(t.speed)));
                self.field_overrides.insert((clone, "DATA/flags".into()), XVal::Num(f64::from(t.flags)));
                self.field_overrides.insert((clone, "DATA/value".into()), XVal::Num(f64::from(t.value)));
                self.field_overrides.insert((clone, "DATA/clip_rounds".into()), XVal::Num(f64::from(t.clip_rounds)));
                self.field_overrides.insert((clone, "DAT2/projectiles_per_shot".into()), XVal::Num(f64::from(t.projectiles_per_shot)));
                self.field_overrides.insert((clone, "DAT2/projectile".into()), XVal::Ref(t.projectile.map_or(0, |f| f.0)));
                self.field_overrides.insert((clone, "DAT2/weight".into()), XVal::Num(f64::from(t.weight)));
                self.field_overrides.insert((clone, "DAT2/consumed_ammo".into()), XVal::Ref(t.casing.map_or(0, |f| f.0)));
                self.field_overrides.insert((clone, "DAT2/consumed_percentage".into()), XVal::Num(f64::from(t.consumed_percentage)));
                for sub in ["FULL", "ONAM", "QNAM"] {
                    if let Some(v) = self.field_overrides.get(&(source, format!("{sub}/"))).cloned() {
                        self.field_overrides.insert((clone, format!("{sub}/")), v);
                    }
                }
                if let Some(name) = self.name_overrides.get(&source).cloned() {
                    self.name_overrides.insert(clone, name);
                }
                self.set_item_playable(clone, t.is_playable());
                if let Some(w) = self.world.effective_weight(source) {
                    self.world.mutate_item_override(clone, |ov| {
                        ov.weight = Some(w);
                    });
                }
                self.other.jip7.ammo_effects.insert(clone, t.effects);
            }
            Some(sig) if &sig == b"AMEF" => {
                let Some(e) = self.ammo_effect_traits(source) else { return };
                self.field_overrides.insert((clone, "DATA/type".into()), XVal::Num(f64::from(e.effect_type)));
                self.field_overrides.insert((clone, "DATA/operation".into()), XVal::Num(f64::from(e.operation)));
                self.field_overrides.insert((clone, "DATA/value".into()), XVal::Num(f64::from(e.value)));
            }
            _ => {}
        }
    }

    /// A climate's weathers (WLST), ClimateAddWeatherType's and ClimateRemoveWeatherType's
    /// changes first (clone-first, original fallback).
    pub fn climate_weathers(&self, climate: FormId) -> Vec<ClimateWeather> {
        let orig = self.unclone(climate);
        if let Some(v) = self.other.jip7.climate_weathers.get(&climate).or_else(|| self.other.jip7.climate_weathers.get(&orig)) {
            return v.clone();
        }
        let list = self.data.record_field(orig, "WLST", "").and_then(Json::as_array);
        list.into_iter()
            .flatten()
            .filter_map(|e| {
                let weather = json_form(e.get("weather"))?;
                Some(ClimateWeather { weather, chance: e.get("chance").and_then(Json::as_i64).unwrap_or(0), global: json_form(e.get("global")) })
            })
            .collect()
    }

    /// An armour's class: 1 light, 2 medium, 3 heavy (BMDT's general flags), SetArmorClass'
    /// change first [documented: SetArmorClass's table].
    pub fn armor_class(&self, armor: FormId) -> u8 {
        let flags = self.num_field(armor, "BMDT", "general_flags") as u64;
        if flags & HEAVY != 0 {
            3
        } else if flags & MEDIUM != 0 {
            2
        } else {
            1
        }
    }

    /// SetArmorConditionPenalty's multiplier, else Stewie's Custom Armor Condition Penalty
    /// tweak (`Tweaks::armor_condition_penalty`, default 1.0) [documented: "1.0, by default";
    /// design: docs/design/tweaks.md].
    pub fn armor_condition_penalty(&self) -> f64 {
        self.other.jip7.armor_condition_penalty.unwrap_or(self.tweaks.armor_condition_penalty)
    }

    /// Effective armour DR/DT condition share: full at or above 50 %, falling by
    /// `1 - (0.5 - condition) * penalty.max(0.0)` clamped at 0.0 (matching
    /// `fnv_combat::condition::armor_mult_with`), reading [`Self::armor_condition_penalty`].
    pub fn armor_condition_mult(&self, condition: f32) -> f64 {
        (1.0 - (0.5 - f64::from(condition).clamp(0.0, 1.0)).max(0.0) * self.armor_condition_penalty().max(0.0)).max(0.0)
    }

    /// SetWeatherTransitionTimeOverride's seconds, `None` without one [documented: "no
    /// arguments (or with 0) removes the override"].
    pub fn weather_transition_override(&self) -> Option<f64> {
        self.other.jip7.weather_transition
    }

    /// A record's number (subrecord `sub`, then `path`), with the session's changes; 0
    /// when it has none.
    pub fn num_field(&self, f: FormId, sub: &str, path: &str) -> f64 {
        crate::nvse::record_value(self, f, sub, path).and_then(|v| v.num().ok()).unwrap_or(0.0)
    }

    /// Preserve only the two existing projectile launch fields before a diagnostic
    /// runs ordinary script setters. No field is changed by taking the capsule.
    pub fn preserve_projectile_launch_fields(&self, projectile: FormId) -> Option<ProjectileLaunchFieldLease> {
        if self.data.sig(projectile) != Some(*b"PROJ") { return None; }
        Some(ProjectileLaunchFieldLease {
            owner: self.projectile_launch_restore_owner.clone(), projectile,
            before: Some(["DATA/speed", "DATA/impact_force"].map(|path| self.field_overrides.get(&(projectile, path.to_string())).cloned())),
            effective: [self.num_field(projectile, "DATA", "speed"), self.num_field(projectile, "DATA", "impact_force")],
        })
    }

    /// The selected projectile at launch, keeping the typed defaults when no session
    /// field was edited. Clearing an ammo projectile falls back to the weapon's.
    /// This does not change other ammo/weapon traits or mutate their typed records.
    pub fn projectile_for_launch(&self, weapon: FormId, ammo: Option<FormId>, weapon_default: Option<FormId>, ammo_default: Option<FormId>) -> (Option<FormId>, Option<&'static str>) {
        let changed = |form, path: &str| {
            let key = path.to_string();
            let v = self.field_overrides.get(&(form, key.clone())).or_else(|| self.field_overrides.get(&(self.unclone(form), key)))?;
            match v {
                XVal::Ref(id) => Some((*id != 0).then_some(FormId(*id))),
                XVal::Num(n) if n.is_finite() && *n >= 0.0 && *n <= f64::from(u32::MAX) && n.fract() == 0.0 => Some((*n != 0.0).then_some(FormId(*n as u32))),
                _ => None,
            }
        };
        let ammo_projectile = ammo.and_then(|a| changed(a, "DAT2/projectile").unwrap_or(ammo_default));
        let weapon_projectile = changed(weapon, "DNAM/projectile").unwrap_or(weapon_default);
        match ammo_projectile {
            Some(p) => (Some(p), Some("ammo")),
            None => (weapon_projectile, weapon_projectile.map(|_| "weapon")),
        }
    }

    /// Look up a `PROJ` `field_overrides` entry directly on `form`.
    /// Cloned `PROJ` records snapshot all active overrides at `TempCloneForm` creation
    /// time [observed: w4g_02_tempcloneform_world_proj_csty], so an absent entry uses
    /// the authored record rather than inheriting post-clone edits from a parent or original.
    pub(crate) fn projectile_field_override(&self, form: FormId, key: &str) -> Option<&XVal> {
        self.field_overrides.get(&(form, key.to_string()))
    }

    /// Snapshot all active session `PROJ` overrides (numeric traits, flags, form slots,
    /// muzzle-flash/model paths) from `source` onto `clone` at `TempCloneForm` creation
    /// time so pre-clone edits on `source` carry onto `clone` and clone-of-clone chains,
    /// while post-clone edits on `source` never bleed into already-created clones
    /// [observed: w4g_02_tempcloneform_world_proj_csty].
    pub(crate) fn snapshot_projectile_traits(&mut self, source: FormId, clone: FormId) {
        if self.data.sig(self.unclone(source)) != Some(*b"PROJ") {
            return;
        }
        let entries: Vec<(String, XVal)> = self
            .field_overrides
            .iter()
            .filter(|((form, _), _)| *form == source)
            .map(|((_, key), val)| (key.clone(), val.clone()))
            .collect();
        for (key, val) in entries {
            self.field_overrides.insert((clone, key), val);
        }
    }

    /// Session `DATA/flags` override directly on a `PROJ` (including overrides snapshotted
    /// at `TempCloneForm` creation time), from `SetProjectileFlag`. Returns `None` for
    /// non-projectiles, absent edits, or non-finite values [design].
    pub fn projectile_flags_override(&self, projectile: FormId) -> Option<u16> {
        let original = self.unclone(projectile);
        if self.data.sig(original) != Some(*b"PROJ") {
            return None;
        }
        match self.field_overrides.get(&(projectile, "DATA/flags".to_string())) {
            Some(XVal::Num(n)) if n.is_finite() => Some((n.trunc() as i64 as u64 & 0xFFFF) as u16),
            _ => None,
        }
    }

    /// Effective `DATA/flags` bitmask for a `PROJ` (session override first, else
    /// authored record; `0` when the projectile has no `DATA`), or `None` for
    /// non-projectiles.
    pub fn projectile_flags(&self, projectile: FormId) -> Option<u16> {
        let original = self.unclone(projectile);
        if self.data.sig(original) != Some(*b"PROJ") {
            return None;
        }
        match crate::nvse::record_value(self, projectile, "DATA", "flags") {
            Some(XVal::Num(n)) if n.is_finite() => Some((n.trunc() as i64 as u64 & 0xFFFF) as u16),
            _ => Some(0),
        }
    }

    /// Effective projectile `Can Be Disabled` state (`DATA/flags` bit 5), honoring
    /// live `SetProjectileFlag` overrides before `FormStore`.
    pub fn projectile_can_be_disabled(&self, projectile: FormId) -> bool {
        if let Some(flags) = self.projectile_flags_override(projectile) {
            return flags & (1 << 5) != 0;
        }
        matches!(
            self.world.forms.get(self.unclone(projectile)).map(|b| &b.data),
            Some(fnv_sim::forms::BaseData::Projectile { can_be_disabled: true, .. })
        )
    }

    /// Speed, impact-force, and `DATA/flags` overrides directly on `projectile` (including
    /// overrides snapshotted at `TempCloneForm` creation time). Absent edits leave the
    /// typed consumer's values alone. The finite conversion guard is an engineering boundary
    /// for physical values.
    pub fn projectile_launch_traits(&self, projectile: FormId) -> Option<ProjectileLaunchTraits> {
        let original = self.unclone(projectile);
        if self.data.sig(original) != Some(*b"PROJ") {
            return None;
        }
        let number = |path: &str| self.projectile_field_override(projectile, &format!("DATA/{path}")).and_then(xval_finite_f32);
        Some(ProjectileLaunchTraits {
            original,
            speed: number("speed"),
            impact_force: number("impact_force"),
            flags: self.projectile_flags_override(projectile),
        })
    }

    /// Sparse session-only non-flag `PROJ` overrides directly on `projectile` (including
    /// overrides snapshotted at `TempCloneForm` creation time). Absent or non-finite entries
    /// stay `None` so typed combat/audio consumers preserve their authored values.
    pub fn projectile_trait_overrides(&self, projectile: FormId) -> Option<ProjectileTraitOverrides> {
        let original = self.unclone(projectile);
        if self.data.sig(original) != Some(*b"PROJ") {
            return None;
        }
        let f32_ov = |path: &str| self.projectile_field_override(projectile, &format!("DATA/{path}")).and_then(xval_finite_f32);
        let form_ov = |path: &str| self.projectile_field_override(projectile, &format!("DATA/{path}")).and_then(xval_opt_form);
        let kind = self.projectile_field_override(projectile, "DATA/type").and_then(|v| match v {
            XVal::Num(n) if n.is_finite() && *n >= 0.0 && *n <= f64::from(u16::MAX) && n.fract() == 0.0 => Some(projectile_kind_label(*n as u16)),
            _ => None,
        });
        let sound_level = self.projectile_field_override(projectile, "VNAM/").and_then(|v| match v {
            XVal::Num(n) if n.is_finite() && *n >= 0.0 && *n <= f64::from(u8::MAX) && n.fract() == 0.0 => Some(projectile_sound_level_label(*n as u8)),
            _ => None,
        });
        let muzzle_flash_model = self.projectile_field_override(projectile, "NAM1/").and_then(|v| match v {
            XVal::Str(s) => Some((!s.trim().is_empty()).then(|| s.clone())),
            _ => None,
        });
        Some(ProjectileTraitOverrides {
            original,
            kind,
            gravity: f32_ov("gravity"),
            speed: f32_ov("speed"),
            range: f32_ov("range"),
            tracer_chance: f32_ov("tracer_chance"),
            alt_trigger_proximity: f32_ov("explosion_alt_trigger_proximity"),
            alt_trigger_timer: f32_ov("explosion_alt_trigger_timer"),
            muzzle_flash_duration: f32_ov("muzzle_flash_duration"),
            fade_duration: f32_ov("fade_duration"),
            impact_force: f32_ov("impact_force"),
            rotation: [f32_ov("rotation.x"), f32_ov("rotation.y"), f32_ov("rotation.z")],
            bouncy_mult: f32_ov("bouncy_mult"),
            sound_level,
            explosion: form_ov("explosion"),
            light: form_ov("light"),
            muzzle_flash_model,
            sound: form_ov("sound"),
            sound_countdown: form_ov("sound_countdown"),
            sound_disable: form_ov("sound_disable"),
        })
    }

    /// Canonical resolved non-flag `PROJ` traits for `projectile` (authored `PROJ` or
    /// `TempCloneForm` `PROJ`), applying live session overrides before authored fields.
    pub fn projectile_traits(&self, projectile: FormId) -> Option<ProjectileTraits> {
        let original = self.unclone(projectile);
        if self.data.sig(original) != Some(*b"PROJ") {
            return None;
        }
        let f32_val = |path: &str| {
            self.projectile_field_override(projectile, &format!("DATA/{path}"))
                .and_then(xval_finite_f32)
                .unwrap_or_else(|| self.data.record_field(original, "DATA", path).and_then(Json::as_f64).unwrap_or(0.0) as f32)
        };
        let form_val = |path: &str| {
            self.projectile_field_override(projectile, &format!("DATA/{path}"))
                .and_then(xval_opt_form)
                .unwrap_or_else(|| json_form(self.data.record_field(original, "DATA", path)))
        };
        let kind_code = self
            .projectile_field_override(projectile, "DATA/type")
            .and_then(|v| match v {
                XVal::Num(n) if n.is_finite() && *n >= 0.0 && *n <= f64::from(u16::MAX) && n.fract() == 0.0 => Some(*n as u16),
                _ => None,
            })
            .unwrap_or_else(|| self.data.record_field(original, "DATA", "type").and_then(Json::as_u64).unwrap_or(0) as u16);
        let sound_level_raw = self
            .projectile_field_override(projectile, "VNAM/")
            .and_then(|v| match v {
                XVal::Num(n) if n.is_finite() && *n >= 0.0 && *n <= f64::from(u8::MAX) && n.fract() == 0.0 => Some(*n as u8),
                _ => None,
            })
            .or_else(|| self.data.record_field(original, "VNAM", "").and_then(Json::as_u64).map(|n| n as u8));
        let muzzle_flash_model = self
            .projectile_field_override(projectile, "NAM1/")
            .and_then(|v| match v {
                XVal::Str(s) => Some((!s.trim().is_empty()).then(|| s.clone())),
                _ => None,
            })
            .unwrap_or_else(|| self.data.record_field(original, "NAM1", "").and_then(Json::as_str).filter(|s| !s.trim().is_empty()).map(str::to_string));
        Some(ProjectileTraits {
            original,
            kind_code,
            kind: projectile_kind_label(kind_code),
            gravity: f32_val("gravity"),
            speed: f32_val("speed"),
            range: f32_val("range"),
            tracer_chance: f32_val("tracer_chance"),
            alt_trigger_proximity: f32_val("explosion_alt_trigger_proximity"),
            alt_trigger_timer: f32_val("explosion_alt_trigger_timer"),
            muzzle_flash_duration: f32_val("muzzle_flash_duration"),
            fade_duration: f32_val("fade_duration"),
            impact_force: f32_val("impact_force"),
            rotation: [f32_val("rotation.x"), f32_val("rotation.y"), f32_val("rotation.z")],
            bouncy_mult: f32_val("bouncy_mult"),
            sound_level_code: sound_level_raw.unwrap_or(0),
            sound_level: sound_level_raw.and_then(projectile_sound_level_label),
            explosion: form_val("explosion"),
            light: form_val("light"),
            muzzle_flash_model,
            sound: form_val("sound"),
            sound_countdown: form_val("sound_countdown"),
            sound_disable: form_val("sound_disable"),
        })
    }

    /// Effective `PROJ` sound slot matching JohnnyGuitar's `SetProjectileSound`:
    /// `1` sound, `2` countdown sound, `3` disable sound.
    pub fn projectile_sound(&self, projectile: FormId, which: u8) -> Option<FormId> {
        let t = self.projectile_traits(projectile)?;
        match which {
            1 => t.sound,
            2 => t.sound_countdown,
            3 => t.sound_disable,
            _ => None,
        }
    }

    fn set_field(&mut self, f: FormId, sub: &str, path: &str, v: XVal) {
        self.field_overrides.insert((f, format!("{sub}/{path}")), v);
    }

    /// Snapshot all canonical `EXPL` fields from `source` onto `clone` at
    /// `TempCloneForm` creation time so pre-clone edits on `source` carry onto `clone`
    /// while post-clone edits on `source` stay isolated from already-created clones.
    pub(crate) fn snapshot_explosion_traits(&mut self, source: FormId, clone: FormId) {
        let original = self.unclone(source);
        if self.data.sig(original) != Some(*b"EXPL") {
            return;
        }
        const NUM_FIELDS: [(&str, &str); 9] = [
            ("DATA", "force"),
            ("DATA", "damage"),
            ("DATA", "radius"),
            ("DATA", "is_radius"),
            ("DATA", "radiation_level"),
            ("DATA", "radiation_dissipation_time"),
            ("DATA", "radiation_radius"),
            ("DATA", "sound_level"),
            ("DATA", "flags"),
        ];
        const FORM_FIELDS: [(&str, &str); 7] = [
            ("DATA", "light"),
            ("DATA", "sound_1"),
            ("DATA", "sound_2"),
            ("DATA", "impact_dataset"),
            ("INAM", ""),
            ("EITM", ""),
            ("MNAM", ""),
        ];
        let mut snap: Vec<(String, XVal)> = Vec::with_capacity(NUM_FIELDS.len() + FORM_FIELDS.len() + 1);
        for &(sub, path) in &NUM_FIELDS {
            let v = match crate::nvse::record_value(self, source, sub, path) {
                Some(XVal::Num(n)) => XVal::Num(n),
                Some(XVal::Str(s)) if path == "sound_level" => XVal::Num(f64::from(sound_level_code(&s))),
                _ => XVal::Num(0.0),
            };
            snap.push((format!("{sub}/{path}"), v));
        }
        for &(sub, path) in &FORM_FIELDS {
            let v = match crate::nvse::record_value(self, source, sub, path) {
                Some(XVal::Ref(r)) => XVal::Ref(r),
                _ => XVal::Ref(0),
            };
            snap.push((format!("{sub}/{path}"), v));
        }
        let model = match crate::nvse::record_value(self, source, "MODL", "") {
            Some(XVal::Str(s)) => XVal::Str(s),
            _ => XVal::Str(String::new()),
        };
        snap.push(("MODL/".into(), model));
        for (key, val) in snap {
            self.field_overrides.insert((clone, key), val);
        }
    }

    /// Resolve the effective session traits, form slots, flags, and model of an `EXPL`
    /// form (or `TempCloneForm` of one), reading `Game::field_overrides` first and
    /// falling back to the authored ESM record. Returns `None` for non-`EXPL` forms.
    pub fn explosion_traits(&self, explosion: FormId) -> Option<ExplosionTraits> {
        let original = self.unclone(explosion);
        if self.data.sig(original) != Some(*b"EXPL") {
            return None;
        }
        let f32_field = |path: &str| match crate::nvse::record_value(self, explosion, "DATA", path) {
            Some(XVal::Num(n)) if n.is_finite() && (n as f32).is_finite() => n as f32,
            _ => 0.0,
        };
        let sound_level = match crate::nvse::record_value(self, explosion, "DATA", "sound_level") {
            Some(XVal::Num(n)) if n.is_finite() => (n.trunc() as i64).rem_euclid(256) as u8,
            Some(XVal::Str(s)) => sound_level_code(&s),
            _ => 0,
        };
        let flags = match crate::nvse::record_value(self, explosion, "DATA", "flags") {
            Some(XVal::Num(n)) if n.is_finite() && n >= 0.0 => n as u64 as u32,
            _ => 0,
        };
        let form_field = |sub: &str, path: &str| match crate::nvse::record_value(self, explosion, sub, path) {
            Some(XVal::Ref(id)) if id != 0 => Some(FormId(id)),
            _ => None,
        };
        let model = match crate::nvse::record_value(self, explosion, "MODL", "") {
            Some(XVal::Str(s)) if !s.trim().is_empty() => Some(s),
            _ => None,
        };
        Some(ExplosionTraits {
            original,
            force: f32_field("force"),
            damage: f32_field("damage"),
            radius: f32_field("radius"),
            is_radius: f32_field("is_radius"),
            radiation_level: f32_field("radiation_level"),
            radiation_dissipation_time: f32_field("radiation_dissipation_time"),
            radiation_radius: f32_field("radiation_radius"),
            sound_level,
            flags,
            light: form_field("DATA", "light"),
            sound_1: form_field("DATA", "sound_1"),
            sound_2: form_field("DATA", "sound_2"),
            impact_dataset: form_field("DATA", "impact_dataset"),
            placed_impact: form_field("INAM", ""),
            enchantment: form_field("EITM", ""),
            image_space_modifier: form_field("MNAM", ""),
            model,
        })
    }

    /// Read a single `EXPL` flag bit (`1..32`, matching `GetExplosionFlag`) with session
    /// overrides applied; `None` for non-`EXPL` forms or out-of-range flag IDs.
    pub fn explosion_flag(&self, explosion: FormId, bit: u8) -> Option<bool> {
        if bit == 0 || bit >= 32 {
            return None;
        }
        Some(self.explosion_traits(explosion)?.has_flag(bit))
    }

    /// Session `SetExplosionTraitNumeric` radius override (`DATA/radius`) directly
    /// present for `explosion` (or its original when not a snapshotted clone), if any.
    pub(crate) fn explosion_radius_override(&self, explosion: FormId) -> Option<f32> {
        let original = self.unclone(explosion);
        if self.data.sig(original) != Some(*b"EXPL") {
            return None;
        }
        let key = "DATA/radius".to_string();
        match self.field_overrides.get(&(explosion, key.clone())).or_else(|| self.field_overrides.get(&(original, key))) {
            Some(XVal::Num(n)) if n.is_finite() && (*n as f32).is_finite() => Some(*n as f32),
            _ => None,
        }
    }
}

fn sound_level_code(label: &str) -> u8 {
    match label.to_ascii_lowercase().as_str() {
        "normal" => 1,
        "silent" => 2,
        _ => 0,
    }
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, _ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let base = |g: &Game, f: FormId| crate::functions::base_of(g, f).unwrap_or(f);
    let of_sig = |g: &Game, f: Option<FormId>, sig: &[u8; 4]| f.filter(|&f| g.data.sig(f) == Some(*sig));
    // Validate the inherited type, but write to the exact session clone. Replacing
    // the clone ID with its original here would edit every user of that original.
    let projectile = |g: &Game, f: Option<FormId>| f.filter(|&f| g.data.sig(g.unclone(f)) == Some(*b"PROJ"));
    let explosion = |g: &Game, f: Option<FormId>| f.filter(|&f| g.data.sig(g.unclone(f)) == Some(*b"EXPL"));
    let ammo = |g: &Game, f: Option<FormId>| f.filter(|&f| g.data.sig(g.unclone(f)) == Some(*b"AMMO"));
    let amef = |g: &Game, f: Option<FormId>| f.filter(|&f| g.data.sig(g.unclone(f)) == Some(*b"AMEF"));
    let zero = XVal::Num(0.0);
    // The setters of a trait return 0 [observed: jip7 and `b3_jip_ex_04_set_ammo_trait_ret`
    // (`wave4b_extenders.json`), see docs/spec-notes/jip-records-7.md].
    let v = match name {
        // ---- ammunition
        "setammotraitnumeric" => {
            let t = usize::try_from(num(args, 1, -1.0) as i64).ok().and_then(|i| AMMO_NUMERIC.get(i));
            if let (Some(a), Some(&(sub, path, how))) = (ammo(g, form(args, 0)), t) {
                if let Some(v) = take(num(args, 2, f64::NAN), how) {
                    g.set_field(a, sub, path, XVal::Num(v));
                }
            }
            zero
        }
        // `ammo [projectile]`: omitting the 2nd argument clears to 0; passing an explicit
        // null ref (0) is a no-op leaving the current projectile unchanged [observed:
        // w4g_11_ammo_clear_and_proj_delete, tools/capture/nvse/catalogs/wave4g_f2_extenders.json].
        // A non-null 2nd argument must be a PROJ (following TempCloneForm clones) [hypothesis].
        "setammoprojectile" => {
            if let Some(a) = ammo(g, form(args, 0)) {
                if args.len() < 2 {
                    g.set_field(a, "DAT2", "projectile", XVal::Ref(0));
                } else if let Some(p) = form(args, 1) {
                    if g.data.sig(g.unclone(p)) == Some(*b"PROJ") {
                        g.set_field(a, "DAT2", "projectile", XVal::Ref(p.0));
                    }
                }
            }
            zero
        }
        // `ammo [form]`: left out, none [documented: SetAmmoCasing].
        "setammocasing" => {
            if let Some(a) = ammo(g, form(args, 0)) {
                let target = form(args, 1);
                g.set_field(a, "DAT2", "consumed_ammo", XVal::Ref(target.map_or(0, |f| f.0)));
            }
            zero
        }
        // `ammo effect`: 1 when the list changed, 0 for an effect already there or not
        // there to remove [documented: `successful`; observed: `jip_16_ammo_effect_add_remove_ret`
        // (`wave4a_extenders.json`), see docs/spec-notes/jip-records-7.md §1].
        "addammoeffect" | "removeammoeffect" => {
            let (Some(a), Some(e)) = (ammo(g, form(args, 0)), amef(g, form(args, 1))) else { return Some(Ok(zero)) };
            let mut list = g.ammo_effects(a);
            let had = list.contains(&e);
            let changed = if name == "addammoeffect" { !had } else { had };
            if name == "addammoeffect" {
                if !had {
                    list.push(e);
                }
            } else {
                list.retain(|&x| x != e);
            }
            g.other.jip7.ammo_effects.insert(a, list);
            yes(changed)
        }
        "setammoeffecttraitnumeric" => {
            let t = usize::try_from(num(args, 1, -1.0) as i64).ok().and_then(|i| AMEF_NUMERIC.get(i));
            if let (Some(e), Some(&(path, how))) = (amef(g, form(args, 0)), t) {
                if let Some(v) = take(num(args, 2, f64::NAN), how) {
                    g.set_field(e, "DATA", path, XVal::Num(v));
                }
            }
            zero
        }
        // ---- projectiles and explosions
        "setprojectiletraitnumeric" => {
            let t = parse_trait_index(args, 1).and_then(|i| PROJ_NUMERIC.get(i));
            if let (Some(p), Some(&(sub, path, how))) = (projectile(g, form(args, 0)), t) {
                if let Some(v) = take(num(args, 2, f64::NAN), how) {
                    g.set_field(p, sub, path, XVal::Num(v));
                }
            }
            zero
        }
        // `projectile [light]`: left out or null clears; a non-null form must be a LIGH
        // (including TempCloneForm LIGH) [documented: SetProjectileLight].
        "setprojectilelight" => {
            if let (Some(p), Some(arg)) = (projectile(g, form(args, 0)), parse_optional_form_arg(args, 1)) {
                match arg {
                    None => g.set_field(p, "DATA", "light", XVal::Ref(0)),
                    Some(l) if g.data.sig(g.unclone(l)) == Some(*b"LIGH") => g.set_field(p, "DATA", "light", XVal::Ref(l.0)),
                    Some(_) => {}
                }
            }
            zero
        }
        // `projectile path`: the muzzle flash model (NAM1) [documented:
        // SetProjectileMuzzleFlash; the field: xEdit PROJ].
        "setprojectilemuzzleflash" => {
            if let (Some(p), Some(path)) = (projectile(g, form(args, 0)), text(args, 1)) {
                g.set_field(p, "NAM1", "", XVal::Str(path));
            }
            zero
        }
        "setexplosiontraitnumeric" => {
            let t = usize::try_from(num(args, 1, -1.0) as i64).ok().and_then(|i| EXPL_NUMERIC.get(i));
            if let (Some(e), Some(&(path, how))) = (explosion(g, form(args, 0)), t) {
                if let Some(v) = take(num(args, 2, f64::NAN), how) {
                    g.set_field(e, "DATA", path, XVal::Num(v));
                }
            }
            zero
        }
        // ---- impact data
        // `ipct trait`: 0-4 the impact data, 5-7 the texture set and sounds, 8-15 the
        // decal data, 16 the flags (1 no decal data, 2 parallax, 4 alpha blending, 8 alpha
        // testing), 17 the colour as R × 1000000 + G × 1000 + B [documented:
        // GetImpactDataTrait's tables]; past them 0.
        "getimpactdatatrait" => {
            let Some(i) = of_sig(g, form(args, 0), b"IPCT") else { return Some(Ok(zero)) };
            match num(args, 1, -1.0) as i64 {
                t @ 0..=4 => XVal::Num(g.num_field(i, IPCT_NUMERIC[t as usize].0, IPCT_NUMERIC[t as usize].1)),
                t @ 5..=7 => XVal::Ref(match crate::nvse::record_value(g, i, IPCT_FORMS[(t - 5) as usize], "") {
                    Some(XVal::Ref(r)) => r,
                    _ => 0,
                }),
                t @ 8..=15 => {
                    let (sub, path, _) = IPCT_NUMERIC[(t - 3) as usize];
                    if crate::nvse::record_value(g, i, "DODT", path).is_none() {
                        // An impact without decal data reads these: widths and heights 8 to
                        // 32, depth 32, shininess 4, parallax scale 1 and 4 passes [observed:
                        // jip7 ipct.trait.8-15 on BaseBallBatMetalImpact, which has no DODT].
                        XVal::Num([8.0, 32.0, 8.0, 32.0, 32.0, 4.0, 1.0, 4.0][(t - 8) as usize])
                    } else {
                        XVal::Num(g.num_field(i, sub, path))
                    }
                }
                16 => XVal::Num(g.other.jip7.impact_flags.get(&i).map_or(impact_flags(g, i) as f64, |&f| f64::from(f))),
                17 => XVal::Num(impact_colour(g, i)),
                _ => zero,
            }
        }
        "setimpactdatatraitnumeric" => {
            let Some(i) = of_sig(g, form(args, 0), b"IPCT") else { return Some(Ok(zero)) };
            let v = num(args, 2, f64::NAN);
            match num(args, 1, -1.0) as i64 {
                t @ 0..=12 => {
                    let (sub, path, range) = IPCT_NUMERIC[t as usize];
                    if let Some(v) = fit(v, range) {
                        g.set_field(i, sub, path, XVal::Num(v));
                    }
                }
                // The four flag bits.
                // Kept whole, to nine bits (99 reads 99, 256 reads 256, -1 reads 511), and read
                // back as set [observed: jip7 ipct.settrait.13.99, jip7b ipct.flags.*]; the four
                // known bits reach the records.
                13 => {
                    let Some(f) = take(v, Take::Int) else { return Some(Ok(zero)) };
                    let f = (f as i64 as u64) & 0x1FF;
                    g.other.jip7.impact_flags.insert(i, f as u32);
                    let f = f & 15;
                    let data = g.num_field(i, "DATA", "flags") as u64;
                    g.set_field(i, "DATA", "flags", XVal::Num(((data & !1) | (f & 1)) as f64));
                    let decal = g.num_field(i, "DODT", "flags") as u64;
                    g.set_field(i, "DODT", "flags", XVal::Num(((decal & !7) | ((f >> 1) & 7)) as f64));
                }
                14 => {
                    // past 0-255255255 nothing is set [observed: jip7b ipct.setcolour.255255256, .m1]
                    let Some(c) = take(v, Take::Int).filter(|c| (0.0..=255_255_255.0).contains(c)) else { return Some(Ok(zero)) };
                    let c = c as u32;
                    // RRRGGGBBB packed as R + G × 256 + B × 65536 into the colour's four
                    // bytes, a channel past 255 carrying into the next (16777215 reads back
                    // 16009218) [observed: jip-records-7.md, the colour's packing; #2039].
                    let word = (c / 1_000_000) + (c / 1000 % 1000) * 256 + (c % 1000) * 65536;
                    for (k, p) in COLOUR_BYTES.iter().enumerate() {
                        g.set_field(i, "DODT", p, XVal::Num(f64::from((word >> (8 * k)) & 0xFF)));
                    }
                }
                _ => {}
            }
            zero
        }
        "setimpactdatatraitform" => {
            let t = usize::try_from(num(args, 1, -1.0) as i64).ok().and_then(|i| IPCT_FORMS.get(i));
            // A form of the wrong type (a creature for a sound) sets none [observed: jip7
            // ipct.setform.sound1; a sound is taken, jip7b ipct.setform.sound].
            if let (Some(i), Some(sub)) = (of_sig(g, form(args, 0), b"IPCT"), t) {
                let want: &[u8; 4] = if *sub == "DNAM" { b"TXST" } else { b"SOUN" };
                g.set_field(i, sub, "", XVal::Ref(of_sig(g, form(args, 2), want).map_or(0, |f| f.0)));
            }
            zero
        }
        // `weapon dataset`: WEAP INAM [documented: SetImpactDataSet; the field: xEdit WEAP].
        "setimpactdataset" => {
            if let Some(w) = of_sig(g, form(args, 0), b"WEAP") {
                g.set_field(w, "INAM", "", XVal::Ref(of_sig(g, form(args, 1), b"IPDS").map_or(0, |f| f.0)));
            }
            zero
        }
        // `[baseActor]`, else the calling reference's base: NAM4 0-11 [documented:
        // Get/SetImpactMaterialType's table; the field: xEdit NPC_/CREA]; not an actor 0.
        "getimpactmaterialtype" => {
            let a = form(args, 0).or(subject).map(|f| base(g, f)).filter(|&a| matches!(g.data.sig(a).as_ref(), Some(b"NPC_" | b"CREA")));
            XVal::Num(a.map_or(0.0, |a| g.num_field(a, "NAM4", "")))
        }
        "setimpactmaterialtype" => {
            let a = form(args, 1).or(subject).map(|f| base(g, f)).filter(|&a| matches!(g.data.sig(a).as_ref(), Some(b"NPC_" | b"CREA")));
            let m = num(args, 0, -1.0);
            if let Some(a) = a.filter(|_| (0.0..=11.0).contains(&m)) {
                g.set_field(a, "NAM4", "", XVal::Num(m.trunc()));
            }
            zero
        }
        // ---- weathers and climates
        // The precipitation model (MODL) and the cloud layers 0-3 [documented: the pages].
        "getweatherprecipitationmodel" | "getweathertexture" => {
            let w = of_sig(g, form(args, 0), b"WTHR");
            let sub = if name == "getweathertexture" { usize::try_from(num(args, 1, -1.0) as i64).ok().and_then(|i| WEATHER_LAYERS.get(i)).copied() } else { Some("MODL") };
            XVal::Str(match w.zip(sub).and_then(|(w, sub)| crate::nvse::record_value(g, w, sub, "")) {
                Some(XVal::Str(s)) => s,
                _ => String::new(),
            })
        }
        "setweatherprecipitationmodel" | "setweathertexture" => {
            let w = of_sig(g, form(args, 0), b"WTHR");
            let (sub, k) = if name == "setweathertexture" { (usize::try_from(num(args, 1, -1.0) as i64).ok().and_then(|i| WEATHER_LAYERS.get(i)).copied(), 2) } else { (Some("MODL"), 1) };
            if let (Some(w), Some(sub), Some(path)) = (w, sub, text(args, k)) {
                g.set_field(w, sub, "", XVal::Str(path));
            }
            zero
        }
        // `[seconds]`: 0 or none removes the override [documented].
        "setweathertransitiontimeoverride" => {
            // NaN or infinity changes nothing.
            if let Some(s) = field_number(num(args, 0, 0.0), 0.0, F32.1) {
                let next = (s > 0.0).then_some(s);
                if g.other.jip7.weather_transition != next {
                    g.other.jip7.weather_transition = next;
                    g.traits.note_weather_visual_change();
                }
            }
            zero
        }
        // `climate weather chance [global]`: a weather already listed takes the chance and
        // global [documented: ClimateAddWeatherType]; 1 when done [documented:
        // `successful`].
        "climateaddweathertype" => {
            let (Some(c), Some(w)) = (of_sig(g, form(args, 0), b"CLMT"), of_sig(g, form(args, 1), b"WTHR")) else { return Some(Ok(zero)) };
            // The chance is 0-100 [documented: the page's `0-100`].
            let Some(chance) = fit(num(args, 2, f64::NAN), (0.0, 100.0)) else { return Some(Ok(zero)) };
            let entry = ClimateWeather { weather: w, chance: chance as i64, global: of_sig(g, form(args, 3), b"GLOB") };
            let mut list = g.climate_weathers(c);
            match list.iter_mut().find(|e| e.weather == w) {
                Some(e) => *e = entry,
                None => list.push(entry),
            }
            g.other.jip7.climate_weathers.insert(c, list);
            yes(true)
        }
        "climateremoveweathertype" => {
            let (Some(c), Some(w)) = (of_sig(g, form(args, 0), b"CLMT"), form(args, 1)) else { return Some(Ok(zero)) };
            let mut list = g.climate_weathers(c);
            let had = list.iter().any(|e| e.weather == w);
            list.retain(|e| e.weather != w);
            g.other.jip7.climate_weathers.insert(c, list);
            yes(had)
        }
        // ---- armour
        // `armor class`: 1 light, 2 medium, 3 heavy [documented: SetArmorClass's table].
        "setarmorclass" => {
            let class = num(args, 1, 0.0) as i64;
            if let Some(a) = form(args, 0).filter(|&a| g.is_armo(a) && (1..=3).contains(&class)) {
                g.set_armo_class(a, class as u8);
            }
            zero
        }
        // `[armor]`: 1 light, 2 medium, 3 heavy; with no armor given, the calling actor's
        // UpperBody armour's; 0 without one, or for a form that is no armour [documented:
        // GECK wiki GetArmorClass, alias GetAC]. A class SetArmorClass gave is read back.
        "getarmorclass" | "getac" => {
            use fnv_sim::forms::ArmorClass;
            let class = match form(args, 0) {
                Some(a) if g.is_armo(a) => Some(crate::armor_overrides::armor_class_from_flags(g.armo_general_flags(a))),
                Some(_) => None,
                None => subject.and_then(|r| g.body_armor(r)).map(|(_, c)| c),
            };
            XVal::Num(match class {
                Some(ArmorClass::Light) => 1.0,
                Some(ArmorClass::Medium) => 2.0,
                Some(ArmorClass::Heavy) => 3.0,
                None => 0.0,
            })
        }
        // `armor path`: the ragdoll constraint template (BMCT) [documented: SetArmorRCT].
        "setarmorrct" => {
            if let (Some(a), Some(path)) = (of_sig(g, form(args, 0), b"ARMO"), text(args, 1)) {
                g.set_field(a, "BMCT", "", XVal::Str(path));
            }
            zero
        }
        // `[multiplier]`: sets it and returns the value before the call; left out, returns
        // it unchanged [documented: SetArmorConditionPenalty]. Any finite float (e.g. 5.0,
        // 3.0, -1.0) is stored verbatim without clamping [observed: jip7 armor.condpenalty,
        // jip7b armor.condpenalty.neg; Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN
        // 57.30, probe jip_19_armor_cond_penalty_clamp: 1.000, 1.000, 5.000, 5.000, -1.000];
        // the share (`SetDamageToArmorMaxPercent`) takes only 0-1 (2 and -1 set nothing)
        // [observed: jip7 armor.maxpercent, jip7b armor.maxpercent.range].
        "setarmorconditionpenalty" => {
            let was = g.armor_condition_penalty();
            if let Some(m) = args.first().and_then(|v| v.num().ok()).filter(|m| m.is_finite()) {
                g.other.jip7.armor_condition_penalty = Some(m);
            }
            XVal::Num(was)
        }
        // `[maxPercent]` 0-1, the same way [documented: SetDamageToArmorMaxPercent]; 1
        // (no cap) until a script sets it [observed: jip7 armor.maxpercent and
        // `b3_jip7_01_dmg_to_armor_max_clamp` (`wave4b_extenders.json`), see
        // docs/spec-notes/jip-records-7.md §5].
        "setdamagetoarmormaxpercent" => {
            let was = g.other.jip7.damage_to_armor_max.unwrap_or(1.0);
            if let Some(m) = args.first().and_then(|v| v.num().ok()).filter(|m| (0.0..=1.0).contains(m)) {
                g.other.jip7.damage_to_armor_max = Some(m);
            }
            XVal::Num(was)
        }
        _ => return None,
    };
    Some(Ok(v))
}

/// An impact data's flags as GetImpactDataTrait 16 gives them: 1 No Decal Data (DATA's
/// flag bit 0), 2 Parallax, 4 Alpha Blending, 8 Alpha Testing (DODT's flag bits 0-2)
/// [documented: the page's table; the bits: xEdit IPCT DATA and the decal data].
fn impact_flags(g: &Game, i: FormId) -> u64 {
    let data = g.num_field(i, "DATA", "flags") as u64 & 1;
    let decal = g.num_field(i, "DODT", "flags") as u64 & 7;
    data | (decal << 1)
}

/// The decal colour's four bytes, the last ByteColor's unused one.
const COLOUR_BYTES: [&str; 4] = ["color.red", "color.green", "color.blue", "color.unused"];

/// A record's decal colour as one 32-bit word; `None` without decal data.
fn colour_word(g: &Game, f: FormId) -> Option<u32> {
    crate::nvse::record_value(g, f, "DODT", "color.red")?;
    Some(COLOUR_BYTES.iter().enumerate().fold(0, |w, (k, p)| w | ((g.num_field(f, "DODT", p) as u32 & 0xFF) << (8 * k))))
}

/// The decal colour as R × 1000000 + G × 1000 + B [documented: "Color (RRRGGGBBB)"]: R
/// the first byte, G the second, B the upper 16 bits unmasked. An impact without decal
/// data reads its texture set's; with no texture set all four bytes are FF, 255320535
/// [observed: jip-records-7.md, the colour's packing; #2039; hypothesis: a texture set
/// without decal data reads the same].
fn impact_colour(g: &Game, i: FormId) -> f64 {
    let set = || match crate::nvse::record_value(g, i, "DNAM", "") {
        Some(XVal::Ref(t)) if t != 0 => colour_word(g, FormId(t)),
        _ => None,
    };
    let w = colour_word(g, i).or_else(set).unwrap_or(u32::MAX);
    f64::from(w & 0xFF) * 1_000_000.0 + f64::from((w >> 8) & 0xFF) * 1000.0 + f64::from(w >> 16)
}

#[cfg(test)]
#[path = "nvse_jip7_launch_probe_tests.rs"]
mod launch_probe_tests;
