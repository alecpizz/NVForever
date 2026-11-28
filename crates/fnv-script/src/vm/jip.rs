//! The JIP LN plugin's functions, as NVForever's own module (tasks J4, J5 of
//! docs/handoff/jip-tasks.md; docs/spec-notes/jip-*.md). Catalogue origins `JIP` and `LU`
//! (lutana's functions, merged into JIP LN). Written from the wiki export's pages and the
//! peer session's runs in the original game with JIP LN 57.30 [observed rows]; no plugin
//! code is read.

use super::{ArrayKind, ExtStore, Vm, VmError, XKey, XVal};
use crate::ir::{XCall, XExpr};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// JIP LN's auxiliary variables and RefMap arrays [documented: jip-variable-stores.md].
/// Names are kept lower-cased: they ignore case [observed: aux.case].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct JipStores {
    /// Owner form -> name -> elements (each a number, a form or a string).
    #[serde(default)]
    pub aux: BTreeMap<u32, BTreeMap<String, Vec<XVal>>>,
    /// Name -> (key form, value), in the order the keys were first set [decision:
    /// jip-variable-stores.md §3].
    #[serde(default)]
    pub refmaps: BTreeMap<String, Vec<(u32, XVal)>>,
    /// Where RefMapArrayGetNext goes on from, per map.
    #[serde(skip)]
    cursor: BTreeMap<String, usize>,
    /// ShowOff's string maps (AuxStringMapArray…): name -> lower-case key -> (the key as
    /// first set, value), one store for every reference [observed: rest1.sm.other.ref,
    /// sm.noref.*; docs/spec-notes/ext-unlisted-rest.md §3].
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub strmaps: BTreeMap<String, BTreeMap<String, (String, XVal)>>,
    /// Where AuxStringMapArrayGetNext goes on from, per map.
    #[serde(skip)]
    strmap_cursor: BTreeMap<String, usize>,
}

impl JipStores {
    pub fn is_empty(&self) -> bool {
        self.aux.is_empty() && self.refmaps.is_empty() && self.strmaps.is_empty()
    }
}

/// Between a private name and its plugin in a store's key.
const PRIVATE: char = '\u{1}';

/// The store a name lives in: names starting `*` are temporary, not saved [observed:
/// aux.class.*; documented by the class names].
fn stores<'s>(x: &'s mut ExtStore, name: &str) -> &'s mut JipStores {
    if name.starts_with('*') { &mut x.jip_temp } else { &mut x.jip }
}

/// The element type codes of AuxiliaryVariableGetType and RefMapArrayGetType
/// [documented]: 0 none, 1 number, 2 form, 4 string.
fn type_code(v: Option<&XVal>) -> f64 {
    match v {
        Some(XVal::Num(_)) => 1.0,
        Some(XVal::Ref(_)) => 2.0,
        Some(XVal::Str(_)) => 4.0,
        _ => 0.0,
    }
}

/// What a typed getter gives for an element: the element when its type matches, else
/// 0, the empty string or no form [observed: aux.float.of.string].
fn typed(v: Option<&XVal>, want: char) -> XVal {
    match (want, v) {
        ('f', Some(x @ XVal::Num(_))) | ('r', Some(x @ XVal::Ref(_))) | ('s', Some(x @ XVal::Str(_))) => x.clone(),
        ('f', _) => XVal::Num(0.0),
        ('r', _) => XVal::Ref(0),
        _ => XVal::Str(String::new()),
    }
}

/// The type letter of a setter or getter name ("setfloat" -> 'f', "getref" -> 'r').
fn kind_of(op: &str) -> char {
    op.as_bytes().get(3).map_or('f', |b| *b as char)
}

/// Drop an emptied variable, and an owner left with none.
fn prune(s: &mut JipStores, owner: u32, name: &str) {
    if let Some(m) = s.aux.get_mut(&owner) {
        if m.get(name).is_some_and(Vec::is_empty) {
            m.remove(name);
        }
        if m.is_empty() {
            s.aux.remove(&owner);
        }
    }
}

/// The version NVForever reports for JIP LN: the one these tasks follow [observed:
/// GetPluginVersion "JIP NVSE Plugin" and GetLNVersion give 57.3 on the owner's install].
pub const JIP_VERSION: f64 = 57.3;

/// The plugin names JIP LN registers [documented: ext-version.md §2a].
pub const JIP_NAMES: [&str; 1] = ["JIP NVSE Plugin"];

/// The extender plugins NVForever reports as installed, by the name each registers, at
/// the version the owner's game reports (the one whose behaviour NVForever's functions
/// follow, extender-compat.md §5.7) [observed: the peer's lu-leveled and mcm sets,
/// nvse.log and GetPluginVersion, 2026-10-26; names are matched without regard to case].
///
/// Two more are built into the engine but are not on the owner's install, so no version
/// could be read; Tale of Two Wastelands' start-up checks (`TTWGenericHandlerScript`:
/// "lStewieAl's Tweaks" at least 941, `IsPluginInstalled ttw_nvse`) showed their "plugin
/// missing" messages although NVForever runs the functions TTW calls from them
/// (`TTW_GetPlugins` and the other `TTW_*`, StewieGetValue; `fnv_game::nvse_small`)
/// [design, owner 2026-11-08: "technically the dll is present, just in engine"]:
/// Stewie's Tweaks started at 941 (TTW 3.4's minimum), was promoted to 945 once the
/// 9.42–9.45 changelog features and Casino Rewards Fix 1.1 consumers were verified
/// (#3179), and was promoted to 960 once the 9.50–9.60 changelog features, live
/// `StewVal`/`StewieGetValue` settings, live `GetPipBoyMode`, and menu search-bar tile
/// publication were verified (#3183, ext-version.md §5.1); ttw_nvse stays at 1, as TTW
/// only asks whether it is there. The higher gate (970, #3184) remains unpromoted until
/// its own step is verified.
///
/// The same holds for every other plugin whose functions the engine builds: a mod that
/// asks `IsPluginInstalled "SUP NVSE Plugin"` before calling SUP's functions got 0 and
/// stopped, although every SUP function of the catalogue runs here [observed: `fnvtool
/// ext queries`, 2026-11-12, 0 and -1 for the six names below]. They are reported under
/// the names the wiki's list gives [documented: ext-version.md §2a]:
///
/// - CacheUI (`CUI`) and hNVSE at the highest version their function pages name, times
///   100 [documented: the pages' "added in" versions, the catalogue's `origin_version`,
///   1.2 for both; GetPluginVersion, "usually the version times 100"]. The numbers read
///   on the owner's install for the plugins above are at or over the same figure for
///   theirs (JIP 57.30 = 57.30, JohnnyGuitar 528 = 5.28, ShowOff 184 ≥ 1.83, kNVSE 37 ≥ 30).
/// - SUP, JIP PP LN, AnhNVSE, NX and UIO at [`UNOBSERVED_VERSION`]: their pages name no
///   version and none was read in the original.
/// - Improved Console (`"Improved Console"`) at `3.0` [documented: ext-version.md §2a
///   registered name `"Improved Console"`; observed: Fallout: New Vegas 1.4.0.525 +
///   xNVSE 6.4.9 + Improved Console v3 (10 official masters), `nvse.log` version `3`,
///   `GetPluginVersion "Improved Console" == 3`]. It has no script functions of its own:
///   NVForever provides persistent console locals (`int`/`float`/`ref`/`string_var`/
///   `array_var`) and transparent expression/statement result printing directly in the
///   interactive console (`docs/design/console.md`, `docs/spec-notes/ext-version.md` §5).
///
/// UIO has no script functions: the engine reads its `uio\public` lists itself
/// (`fnv_ui`), which is all the plugin does for a mod that asks whether it is there.
/// Not reported, though the catalogue has functions of theirs: yUI (its two ySI trait
/// functions are built, its menus are not, and a mod that finds it goes on to read its
/// tiles), and the plugins whose registered name no page gives (ITR, One111, ppNVSE,
/// vheyes-nvse and the smaller ones): ext-version.md §6.
pub const PLUGINS: [(&str, f64); 18] = [
    ("JIP NVSE Plugin", JIP_VERSION),
    ("JIP LN NVSE", JIP_VERSION),
    ("ShowOffNVSE Plugin", 184.0),
    ("kNVSE", 37.0),
    ("JohnnyGuitarNVSE", 528.0),
    ("MCM Extensions", 1.0),
    ("lStewieAl's Tweaks", 960.0),
    ("ttw_nvse", 1.0),
    ("CUI", 120.0),
    ("hNVSE", 120.0),
    ("SUP NVSE Plugin", UNOBSERVED_VERSION),
    ("JIP PP LN", UNOBSERVED_VERSION),
    ("AnhNVSE", UNOBSERVED_VERSION),
    ("Extensions for NVSE", UNOBSERVED_VERSION),
    ("UI Organizer Plugin", UNOBSERVED_VERSION),
    ("Improved Console", 3.0),
    // No script functions of their own; their work (engine speed-ups, a sharper local map)
    // is the engine's own here, as UIO's lists are (ext-version.md §4). Nuclear Sunset's
    // start-up Script Runner check shows "Outdated xNVSE plugins detected!" without them
    // [observed in FadedLight's Nuclear Sunset fork: an installed copy's nvse.log,
    // "Engine Optimizations; version 100", "HighResLocalMaps; version 12" (#4491)].
    ("Engine Optimizations", 100.0),
    ("HighResLocalMaps", 12.0),
];

/// The version reported for a plugin the engine builds whose own number is neither
/// documented nor read in the original: higher than any a mod asks for, since mods
/// compare with "at least" and a number too low shows their "update the plugin" message
/// [design; ext-version.md §4, "reporting a version that is too high is safer than too
/// low"]. To be replaced, plugin by plugin, by the number read in the original
/// (docs/handoff/observer-checks.md).
pub const UNOBSERVED_VERSION: f64 = 9999.0;

impl Vm<'_, '_> {
    /// A JIP LN function by catalogue name; `None` when it is not built yet.
    pub(super) fn jip_function(&mut self, n: &str, c: &XCall) -> Result<Option<XVal>, VmError> {
        let val = |vm: &mut Self, k: usize| -> Result<XVal, VmError> {
            match c.args.get(k) {
                Some(a) => vm.xeval(a),
                None => Err(VmError::Expression(format!("{n} without argument {}", k + 1))),
            }
        };
        let opt = |vm: &mut Self, k: usize| -> Result<Option<XVal>, VmError> { c.args.get(k).map(|a| vm.xeval(a)).transpose() };
        let text = |v: XVal| -> Option<String> { if let XVal::Str(s) = v { Some(s) } else { None } };
        let num = |x: f64| Ok(Some(XVal::Num(x)));
        match n {
            // `base toAdd [override]`: the second array's elements join the first in place,
            // a list's at its end, a map's by key (an old key kept unless override)
            // [documented: Ar_Cat; observed: data2, (1 2) + (3 4 5) reads size 5, last 5].
            "ar_cat" => {
                let (a, b) = (val(self, 0)?, val(self, 1)?);
                let replace = opt(self, 2)?.is_some_and(|v| v.truthy());
                let id = |v: &XVal| if let XVal::Array(i) = v { Some(*i).filter(|&i| i != 0) } else { None };
                let (Some(ia), Some(ib)) = (id(&a), id(&b)) else { return num(0.0) };
                let store = self.store()?;
                let list = store.array(ia).is_some_and(|x| x.kind == ArrayKind::List);
                let mut len = store.array(ia).map_or(0, |x| x.entries.len());
                let add: Vec<(XKey, XVal)> = store.array(ib).map(|x| x.entries.clone()).unwrap_or_default();
                let have: Vec<XKey> = store.array(ia).map(|x| x.entries.iter().map(|(k, _)| k.clone()).collect()).unwrap_or_default();
                for (k, v) in add {
                    if list {
                        self.set_element(&a, &XVal::Num(len as f64), v)?;
                        len += 1;
                    } else if replace || !have.contains(&k) {
                        self.set_element(&a, &k.value(), v)?;
                    }
                }
                num(0.0)
            }
            // [documented: Fsin; Fcos; Ftan; observed: fsin 90 = 1] degrees.
            "fsin" | "fcos" | "ftan" => {
                let d = val(self, 0)?.num()?.to_radians();
                num(match n {
                    "fsin" => d.sin(),
                    "fcos" => d.cos(),
                    _ => d.tan(),
                })
            }
            // [documented: Fasin; Facos; Fatan; observed: fasin 2 = 90] degrees, the input
            // held to ±1 [hypothesis from the clamp observed].
            "fasin" | "facos" => {
                let x = val(self, 0)?.num()?;
                num(match n {
                    "fasin" => x.clamp(-1.0, 1.0).asin().to_degrees(),
                    _ => x.clamp(-1.0, 1.0).acos().to_degrees(),
                })
            }
            // [documented: Fatan; Fatan2; observed: (1, -1) = 135, (-1, -1) = -135] degrees
            // by the game's own approximation (`fast_atan2`), not the exact angle.
            "fatan" => num(fast_atan2(val(self, 0)?.num()?, 1.0)),
            "fatan2" => {
                let (y, x) = (val(self, 0)?.num()?, val(self, 1)?.num()?);
                num(fast_atan2(y, x))
            }
            // [documented: Fsqrt; observed: fsqrt -1 = 0].
            "fsqrt" => {
                let x = val(self, 0)?.num()?;
                num(if x < 0.0 { 0.0 } else { x.sqrt() })
            }
            // [documented: GetMaxOf; GetMinOf] 2 to 5 values.
            "getmaxof" | "getminof" => {
                let mut best: Option<f64> = None;
                for k in 0..c.args.len().min(5) {
                    let x = val(self, k)?.num()?;
                    best = Some(match best {
                        None => x,
                        Some(b) if n == "getmaxof" => b.max(x),
                        Some(b) => b.min(x),
                    });
                }
                num(best.unwrap_or(0.0))
            }
            // [documented: GetRandomInRange; observed: (5, 6) = 5] min ≤ x < max.
            "getrandominrange" => {
                let (a, b) = (val(self, 0)?.num()?.trunc(), val(self, 1)?.num()?.trunc());
                let r = self.host.random();
                num(if b <= a { a } else { (a + ((b - a) * r).floor()).min(b - 1.0) })
            }
            // [documented: StringToActorValue; observed: case ignored, -1 for no name].
            "stringtoactorvalue" => {
                let s = text(val(self, 0)?).unwrap_or_default();
                num(self.host.actor_value_code(&s).map_or(-1.0, f64::from))
            }
            // [documented: GetModName; observed: 0 → "FalloutNV", (0, 1) → "FalloutNV.esm",
            // 99 → ""].
            "getmodname" => {
                let i = val(self, 0)?.num()?;
                let keep = opt(self, 1)?.is_some_and(|v| v.truthy());
                let name = if i < 0.0 { None } else { self.host.mod_name(i as u32) };
                let name = name.map(|n| if keep { n } else { n.rsplit_once('.').map_or(n.clone(), |(stem, _)| stem.to_string()) });
                Ok(Some(XVal::Str(name.unwrap_or_default())))
            }
            // [documented: ValidateModIndex; observed: 0 → 0, 200 → -1].
            "validatemodindex" => {
                let i = val(self, 0)?.num()?;
                let ok = i >= 0.0 && self.host.mod_name(i as u32).is_some();
                num(if ok { i.trunc() } else { -1.0 })
            }
            // [documented: GetFormFromMod; observed: "14" and "000014" give the player].
            "getformfrommod" | "gffm" => {
                let (p, h) = (text(val(self, 0)?).unwrap_or_default(), text(val(self, 1)?).unwrap_or_default());
                Ok(Some(XVal::Ref(self.plugin_form(&p, &h).unwrap_or(0))))
            }
            // [documented: StringToRef; observed: "00000014" gives the player] RefToString's
            // "Plugin:hex", or a whole FormID in hex.
            "stringtoref" => {
                let s = text(val(self, 0)?).unwrap_or_default();
                let f = match s.split_once(':') {
                    Some((p, h)) => self.plugin_form(p, h),
                    None => u32::from_str_radix(s.trim(), 16).ok().filter(|&f| self.host.form_sig(f).is_some()),
                };
                Ok(Some(XVal::Ref(f.unwrap_or(0))))
            }
            // [documented: RefToString; observed: "FalloutNV.esm:104C0C"] upper-case hex
            // without the mod index or leading zeros.
            "reftostring" => {
                let f = match c.args.first() {
                    Some(a) => self.xeval(a)?.value()?.as_form(),
                    None => match &c.on {
                        Some(e) => self.xeval(e)?.value()?.as_form(),
                        None => None,
                    },
                };
                let Some(f) = f else { return Ok(Some(XVal::Str(String::new()))) };
                let plugin = self.host.mod_name(f >> 24).unwrap_or_default();
                Ok(Some(XVal::Str(format!("{plugin}:{:X}", f & 0x00FF_FFFF))))
            }
            // What NVForever reports for JIP LN (task J4) [observed: GetLNVersion 57.3 on the
            // owner's install, though its page says 0; GetNVSEVersionFull 6.49 on xNVSE
            // 6.4.9].
            "getlnversion" | "getjiplnversion" => num(JIP_VERSION),
            "getnvseversionfull" => num(6.49),
            // [documented: GetLocalGravity; observed: X 0, Z -98.1] SetLocalGravityVector is
            // not built, so the cell's gravity is the default.
            "getlocalgravity" => {
                let axis = match c.args.first() {
                    Some(XExpr::Axis { c }) => *c,
                    Some(a) => match self.xeval(a)? {
                        XVal::Str(s) => s.chars().next().unwrap_or('Z').to_ascii_uppercase(),
                        _ => 'Z',
                    },
                    None => 'Z',
                };
                num(if axis == 'Z' { -98.1 } else { 0.0 })
            }
            // Record traits and flags (jip-records.md): each number is the decoded field
            // the original's value equalled [observed: jip-records rows]. A number outside
            // the table gives 0 [hypothesis].
            "getprojectiletraitnumeric" | "getexplosiontraitnumeric" | "getammotraitnumeric" | "getexplosiontraitform" | "getprojectileflag" | "getexplosionflag" => {
                let f = val(self, 0)?.value()?.as_form().unwrap_or(0);
                let k = val(self, 1)?.num()? as i64;
                let field = match (n, k) {
                    ("getprojectiletraitnumeric", 0) => Some(("DATA", "type")),
                    ("getprojectiletraitnumeric", 1) => Some(("DATA", "gravity")),
                    ("getprojectiletraitnumeric", 2) => Some(("DATA", "speed")),
                    ("getprojectiletraitnumeric", 3) => Some(("DATA", "range")),
                    ("getprojectiletraitnumeric", 4) => Some(("DATA", "tracer_chance")),
                    ("getprojectiletraitnumeric", 5) => Some(("DATA", "explosion_alt_trigger_proximity")),
                    ("getprojectiletraitnumeric", 6) => Some(("DATA", "explosion_alt_trigger_timer")),
                    ("getprojectiletraitnumeric", 7) => Some(("DATA", "muzzle_flash_duration")),
                    ("getprojectiletraitnumeric", 8) => Some(("DATA", "fade_duration")),
                    ("getprojectiletraitnumeric", 9) => Some(("DATA", "impact_force")),
                    ("getprojectiletraitnumeric", 10) => Some(("DATA", "rotation.x")),
                    ("getprojectiletraitnumeric", 11) => Some(("DATA", "rotation.y")),
                    ("getprojectiletraitnumeric", 12) => Some(("DATA", "rotation.z")),
                    ("getprojectiletraitnumeric", 13) => Some(("DATA", "bouncy_mult")),
                    ("getprojectiletraitnumeric", 14) => Some(("VNAM", "")),
                    ("getexplosiontraitnumeric", 0) => Some(("DATA", "force")),
                    ("getexplosiontraitnumeric", 1) => Some(("DATA", "damage")),
                    ("getexplosiontraitnumeric", 2) => Some(("DATA", "radius")),
                    ("getexplosiontraitnumeric", 3) => Some(("DATA", "is_radius")),
                    ("getexplosiontraitnumeric", 4) => Some(("DATA", "radiation_level")),
                    ("getexplosiontraitnumeric", 5) => Some(("DATA", "radiation_dissipation_time")),
                    ("getexplosiontraitnumeric", 6) => Some(("DATA", "radiation_radius")),
                    ("getexplosiontraitnumeric", 7) => Some(("DATA", "sound_level")),
                    ("getexplosiontraitform", 0) => Some(("DATA", "light")),
                    ("getexplosiontraitform", 1) => Some(("DATA", "sound_1")),
                    ("getexplosiontraitform", 2) => Some(("DATA", "sound_2")),
                    ("getexplosiontraitform", 3) => Some(("DATA", "impact_dataset")),
                    ("getexplosiontraitform", 4) => Some(("INAM", "")),
                    ("getexplosiontraitform", 5) => Some(("EITM", "")),
                    ("getexplosiontraitform", 6) => Some(("MNAM", "")),
                    ("getammotraitnumeric", 0) => Some(("DATA", "speed")),
                    ("getammotraitnumeric", 1) => Some(("DAT2", "projectiles_per_shot")),
                    ("getammotraitnumeric", 2) => Some(("DAT2", "consumed_percentage")),
                    ("getammotraitnumeric", 3) => Some(("DATA", "clip_rounds")),
                    ("getprojectileflag" | "getexplosionflag", _) => Some(("DATA", "flags")),
                    _ => None,
                };
                let v = field.and_then(|(sub, path)| self.host.record_field(f, sub, path));
                Ok(Some(match n {
                    // Bit n of the flags [observed: projflag.*, explflag.*]; explosion IDs
                    // start at 1.
                    "getprojectileflag" | "getexplosionflag" => {
                        let bits = v.map_or(Ok(0.0), |v| v.num())? as u64;
                        let on = (0..64).contains(&k) && !(n == "getexplosionflag" && k == 0) && bits & (1 << k) != 0;
                        XVal::Num(f64::from(u8::from(on)))
                    }
                    "getexplosiontraitform" => match v {
                        Some(XVal::Ref(r)) => XVal::Ref(r),
                        _ => XVal::Ref(0),
                    },
                    _ => match v {
                        Some(XVal::Num(x)) => XVal::Num(x),
                        _ => XVal::Num(0.0),
                    },
                }))
            }
            // JIP's own event registrations (task J6; jip-events.md).
            _ if n.starts_with("seton") && n.ends_with("eventhandler") && n.len() > 17 => {
                let event = n["seton".len()..n.len() - "eventhandler".len()].to_string();
                let func = val(self, 0)?;
                let add = val(self, 1)?.truthy();
                let mut filters = Vec::new();
                for a in c.args.iter().skip(2) {
                    filters.push(self.xeval(a)?);
                }
                let r = self.host.event_call(super::EventCall::JipSet { event, func, add, filters }).map_err(VmError::Host)?;
                Ok(Some(r))
            }
            "setgamemainloopcallback" => {
                let func = val(self, 0)?;
                let add = val(self, 1)?.truthy();
                let delay = opt(self, 2)?.map(|v| v.num()).transpose()?.unwrap_or(1.0).max(1.0) as u64;
                let flags = opt(self, 3)?.map(|v| v.num()).transpose()?.unwrap_or(3.0) as u32;
                let on = match &c.on {
                    Some(e) => Some(self.xeval(e)?.value()?),
                    None => None,
                };
                let r = self.host.event_call(super::EventCall::MainLoop { func, add, delay, flags, on }).map_err(VmError::Host)?;
                Ok(Some(r))
            }
            // Weapon and armor fields (jip-records.md §4–5, #829) [observed: jip-records
            // weapon and armor rows equal the decoded fields].
            "getweaponkillimpulse" | "getweaponimpulsedistance" | "getweaponcritflags" | "getweapononhitsetting" | "getweaponsemiautofiredelay" => {
                let v0 = val(self, 0)?;
                let w = self.jip_base(v0)?;
                let (sub, path) = match n {
                    "getweaponkillimpulse" => ("DNAM", "kill_impulse"),
                    "getweaponimpulsedistance" => ("DNAM", "impulse_dist"),
                    "getweaponcritflags" => ("CRDT", "flags"),
                    "getweapononhitsetting" => ("DNAM", "on_hit"),
                    _ if opt(self, 1)?.map(|v| v.num()).transpose()?.unwrap_or(0.0) != 0.0 => ("DNAM", "semi_automatic_fire_delay_max"),
                    _ => ("DNAM", "semi_automatic_fire_delay_min"),
                };
                Ok(Some(match self.host.record_field(w, sub, path) {
                    Some(XVal::Num(x)) => XVal::Num(x),
                    _ => XVal::Num(0.0),
                }))
            }
            "getweaponvatseffect" | "getimpactdataset" => {
                let v0 = val(self, 0)?;
                let w = self.jip_base(v0)?;
                let (sub, path) = if n == "getimpactdataset" { ("INAM", "") } else { ("VATS", "effect") };
                Ok(Some(match self.host.record_field(w, sub, path) {
                    Some(XVal::Ref(r)) => XVal::Ref(r),
                    _ => XVal::Ref(0),
                }))
            }
            // Mod mask 0 the plain model, 1–7 the MWD1–MWD7 models [documented:
            // GetWeaponModel; observed].
            "getweaponmodel" => {
                let v0 = val(self, 0)?;
                let w = self.jip_base(v0)?;
                let mask = opt(self, 1)?.map(|v| v.num()).transpose()?.unwrap_or(0.0) as i64;
                let sub = match mask {
                    1..=7 => format!("MWD{mask}"),
                    _ => "MODL".to_string(),
                };
                let v = self.host.record_field(w, &sub, "").or_else(|| self.host.record_field(w, &sub, "model"));
                Ok(Some(match v {
                    Some(XVal::Str(s)) => XVal::Str(s),
                    _ => XVal::Str(String::new()),
                }))
            }
            // [documented: GetArmorClass; observed: metal 3, leather 1] BMDT general flags:
            // Heavy (bit 7) 3, Medium (bit 3) 2 [hypothesis: none probed], else 1; 0 when
            // the form is no armor.
            "getarmorclass" => {
                let v0 = val(self, 0)?;
                let a = self.jip_base(v0)?;
                Ok(Some(XVal::Num(match self.host.record_field(a, "BMDT", "general_flags") {
                    Some(XVal::Num(bits)) => {
                        let bits = bits as u64;
                        if bits & 0x80 != 0 {
                            3.0
                        } else if bits & 0x08 != 0 {
                            2.0
                        } else {
                            1.0
                        }
                    }
                    _ => 0.0,
                })))
            }
            // The current or last hit of the actor it is called on (task J7,
            // jip-hit-info.md §1): 0, no form, or ar_Null (Ar_Size -1) without a hit or on
            // invalid GetHit3DData dataType [observed: 2026-11-23, GSDocMitchellHouse
            // (0x00103DF9), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 (10
            // official base masters), #3678 / PR #3679 probe
            // `w1_3606_hit_3d_dir_and_base_damage`: `Ar_Size (rDoc.GetHit3DData 0/1/2/9)`
            // pre-hit returns `-1`].
            "gethitattacker" | "gethitweapon" | "gethitprojectile" | "gethithealthdamage" | "gethitlimbdamage" | "gethitfatiguedamage" | "gethitbaseweapondamage" | "gethitextendedflag" | "gethit3ddata" => {
                let who = match &c.on {
                    Some(e) => self.xeval(e)?.value()?.as_form(),
                    None => self.host.calling_ref(),
                }
                .unwrap_or(0);
                let k = opt(self, 0)?.map(|v| v.num()).transpose()?.unwrap_or(0.0) as i64;
                let what = match n {
                    "gethitattacker" => "attacker",
                    "gethitweapon" => "weapon",
                    "gethitprojectile" => "projectile",
                    "gethithealthdamage" => "health",
                    "gethitlimbdamage" => "limb",
                    "gethitfatiguedamage" => "fatigue",
                    "gethitbaseweapondamage" => "base_weapon",
                    "gethitextendedflag" => "flag",
                    "gethit3ddata" => "3d",
                    _ => unreachable!("routed by name"),
                };
                if let Some(v) = self.host.hit_value(who, what, k) {
                    return Ok(Some(v));
                }
                Ok(Some(match what {
                    "attacker" | "weapon" | "projectile" => XVal::Ref(0),
                    "3d" => XVal::Array(0),
                    _ => XVal::Num(0.0),
                }))
            }
            // Names and text fields (jip-strings.md §3): a missing entry is "" [observed].
            "lngetname" => {
                let f = match (c.args.first(), &c.on) {
                    (Some(a), _) => self.xeval(a)?.value()?.as_form(),
                    (None, Some(e)) => self.xeval(e)?.value()?.as_form(),
                    (None, None) => self.host.calling_ref(),
                };
                Ok(Some(XVal::Str(f.and_then(|f| self.host.form_text(f)).unwrap_or_default())))
            }
            // GetArmorRCT: the ragdoll constraint template, ARMO BMCT [observed: #830,
            // ArmorPowerT45d `RagdollConstraint\default.rdt`, ArmorMetal ""].
            "getformdescription" | "getloadscreentexture" | "getprojectilemuzzleflash" | "getarmorrct" => {
                let v0 = val(self, 0)?;
                let f = if n == "getarmorrct" { self.jip_base(v0)? } else { v0.value()?.as_form().unwrap_or(0) };
                let sub = match n {
                    "getformdescription" => "DESC",
                    "getloadscreentexture" => "ICON",
                    "getarmorrct" => "BMCT",
                    _ => "NAM1",
                };
                Ok(Some(match self.host.record_field(f, sub, "") {
                    Some(XVal::Str(s)) => XVal::Str(s),
                    _ => XVal::Str(String::new()),
                }))
            }
            // [documented: GetBodyPartName; observed: NVGecko part 0 "Torso", 14 ""] by part
            // type: 0 torso, 1–2 head … 14 weapon.
            "getbodypartname" => {
                let f = val(self, 0)?.value()?.as_form().unwrap_or(0);
                let part = val(self, 1)?.num()? as i64;
                Ok(Some(XVal::Str(self.host.body_part_name(f, part).unwrap_or_default())))
            }
            // [documented: GetObjectiveText; observed: objective 9999 gives ""].
            "getobjectivetext" => {
                let q = val(self, 0)?.value()?.as_form().unwrap_or(0);
                let i = val(self, 1)?.num()? as i32;
                Ok(Some(XVal::Str(self.host.objective_text(q, i).unwrap_or_default())))
            }
            // [documented: GetStringSetting; observed: `%d` left in].
            "getstringsetting" => {
                let s = text(val(self, 0)?).unwrap_or_default();
                Ok(Some(match self.host.game_setting(&s) {
                    Some(XVal::Str(t)) => XVal::Str(t),
                    _ => XVal::Str(String::new()),
                }))
            }
            // [observed: projexpl.*, projlight.slug, ammoproj.*] form fields.
            "getprojectileexplosion" | "getprojectilelight" | "getammoprojectile" => {
                let f = val(self, 0)?.value()?.as_form().unwrap_or(0);
                let (sub, path) = match n {
                    "getprojectileexplosion" => ("DATA", "explosion"),
                    "getprojectilelight" => ("DATA", "light"),
                    _ => ("DAT2", "projectile"),
                };
                Ok(Some(match self.host.record_field(f, sub, path) {
                    Some(XVal::Ref(r)) => XVal::Ref(r),
                    _ => XVal::Ref(0),
                }))
            }
            "sv_regexmatch" | "sv_regexsearch" | "sv_regexreplace" | "validateregex" => self.regex_function(n, c).map(Some),
            // [documented: MessageBoxExAlt] `callback "^title^text|button|…" values…`: up to
            // 255 buttons; the UDF gets the pressed button's index. The title is not shown
            // [hypothesis: NVForever's box has none]. Without a button the box gets "Ok", as
            // MessageBoxEx [hypothesis].
            "messageboxexalt" => {
                let func = val(self, 0)?;
                let (t, _) = self.format_arg(c, 1)?;
                let body = match t.strip_prefix('^').and_then(|r| r.split_once('^')) {
                    Some((_, rest)) => rest.to_string(),
                    None => t,
                };
                let mut parts = body.split('|').map(str::to_string);
                let text = parts.next().unwrap_or_default();
                let mut buttons: Vec<String> = parts.take(255).collect();
                if buttons.is_empty() {
                    buttons.push("Ok".into());
                }
                self.host.box_callback(func);
                self.host.show_text(&text, buttons);
                Ok(Some(XVal::Num(0.0)))
            }
            _ => self.jip_store_function(n, c),
        }
    }

    /// The regular-expression functions (jip-strings.md §1). The dialect is ECMAScript's
    /// [observed: `w4c_b1_24_jip_regex` (`wave4c_extenders.json`), see
    /// docs/spec-notes/jip-strings.md §1]; `fancy-regex` has its groups,
    /// classes and back-references. An invalid pattern is a run-time error [decision: the
    /// original crashes, documented].
    pub(super) fn regex_function(&mut self, n: &str, c: &XCall) -> Result<XVal, VmError> {
        let target = |vm: &mut Self| -> Result<String, VmError> {
            match c.args.first() {
                Some(a) => Ok(match vm.xeval(a)? {
                    XVal::Str(s) => s,
                    other => vm.text_of(&other)?,
                }),
                None => Err(VmError::Expression(format!("{n} without a string"))),
            }
        };
        // An invalid pattern crashes the original [documented: the three pages]: here the
        // default value (0, an empty string or no array) and a script error on the
        // console [decision: the owner, 2026-10-26].
        let fallback = match n {
            "sv_regexsearch" => XVal::Array(0),
            "sv_regexreplace" => XVal::Str(String::new()),
            _ => XVal::Num(0.0),
        };
        let build = |p: &str| fancy_regex::Regex::new(p).map_err(|e| VmError::Type(format!("{n}: the pattern {p:?} is not valid ({e})")));
        let result = (|| -> Result<XVal, VmError> { Ok(match n {
            // [documented: ValidateRegex; observed: "[" gives 5] 0 when valid. Which
            // fancy-regex error is which code is our reading of the error names
            // [hypothesis].
            "validateregex" => {
                let p = match c.args.first() {
                    Some(a) => match self.xeval(a)? {
                        XVal::Str(s) => s,
                        _ => return Ok(XVal::Num(-1.0)),
                    },
                    None => return Ok(XVal::Num(-1.0)),
                };
                match fancy_regex::Regex::new(&p) {
                    Ok(_) => XVal::Num(0.0),
                    Err(e) => {
                        let msg = e.to_string();
                        if let Some(XExpr::Var { var, .. }) = c.args.get(1) {
                            self.write_var(var, XVal::Str(msg.clone()))?;
                        }
                        XVal::Num(regex_error_code(&msg))
                    }
                }
            }
            // [observed: regex.match, regex.partial] the whole target must match.
            "sv_regexmatch" => {
                let t = target(self)?;
                let (p, _) = self.format_arg(c, 1)?;
                let re = build(&format!("^(?:{p})$"))?;
                XVal::Num(f64::from(u8::from(re.is_match(&t).map_err(|e| VmError::Type(e.to_string()))?)))
            }
            // [documented: Sv RegexSearch; observed: regex.search] every match, in order.
            "sv_regexsearch" => {
                let t = target(self)?;
                let (p, _) = self.format_arg(c, 1)?;
                let re = build(&p)?;
                let mut found = Vec::new();
                for m in re.find_iter(&t) {
                    found.push(XVal::Str(m.map_err(|e| VmError::Type(e.to_string()))?.as_str().to_string()));
                }
                self.jlist(found)?
            }
            // [documented: Sv RegexReplace; observed: regex.replace] "replacement|pattern"
            // as one string; split at the first `|` [hypothesis].
            "sv_regexreplace" => {
                let t = target(self)?;
                let (both, _) = self.format_arg(c, 1)?;
                let Some((with, p)) = both.split_once('|') else {
                    return Err(VmError::Type(format!("{n} needs \"replacement|pattern\"")));
                };
                let re = build(p)?;
                XVal::Str(re.replace_all(&t, with).into_owned())
            }
            _ => unreachable!("routed by name"),
        }) })();
        match result {
            Err(VmError::Type(e)) if n != "validateregex" => {
                self.host.script_error(&e);
                Ok(fallback)
            }
            r => r,
        }
    }

    /// Argument `k`, or `None` when left out.
    fn jarg(&mut self, c: &XCall, k: usize) -> Result<Option<XVal>, VmError> {
        c.args.get(k).map(|a| self.xeval(a)).transpose()
    }

    /// The owner (aux) or key (RefMap) given as argument `k`, else the reference the
    /// function is called on, else the calling reference.
    fn jowner(&mut self, n: &str, c: &XCall, k: usize) -> Result<u32, VmError> {
        if let Some(f) = match self.jarg(c, k)? {
            Some(v) => v.value()?.as_form(),
            None => None,
        } {
            return Ok(f);
        }
        if let Some(e) = &c.on {
            if let Some(f) = self.xeval(e)?.value()?.as_form() {
                return Ok(f);
            }
        }
        self.host.calling_ref().ok_or_else(|| VmError::Type(format!("{n} without a reference or a form")))
    }

    /// As [`Self::jowner`], `None` when there is no owner: a form argument that is empty
    /// (a ref variable holding nothing) on a script with no calling reference. The
    /// function then gives what a variable that is not set gives [documented:
    /// AuxiliaryVariableGetFloat, "will (presumably) return 0 if the variable is not
    /// defined"; hypothesis: the same with no owner, JIP's functions failing to 0 rather
    /// than stopping the script].
    fn jowner_opt(&mut self, c: &XCall, k: usize) -> Result<Option<u32>, VmError> {
        match self.jowner("", c, k) {
            Ok(o) => Ok(Some(o)),
            Err(VmError::Type(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Argument `k` as an index (`dflt` when left out).
    fn jindex(&mut self, c: &XCall, k: usize, dflt: i64) -> Result<i64, VmError> {
        Ok(match self.jarg(c, k)? {
            Some(v) => v.num()? as i64,
            None => dflt,
        })
    }

    /// A setter's value (argument 1) as the type its name gives.
    fn jvalue(&mut self, n: &str, c: &XCall, kind: char) -> Result<XVal, VmError> {
        let v = self.jarg(c, 1)?.ok_or_else(|| VmError::Expression(format!("{n} without a value")))?;
        Ok(match kind {
            'f' => XVal::Num(v.num()?),
            'r' => XVal::Ref(v.value()?.as_form().unwrap_or(0)),
            _ => match v {
                XVal::Str(s) => XVal::Str(s),
                other => XVal::Str(self.text_of(&other)?),
            },
        })
    }

    /// A list array of `items`.
    fn jlist(&mut self, items: Vec<XVal>) -> Result<XVal, VmError> {
        let entries = items.into_iter().enumerate().map(|(i, v)| (XKey::Num(i as f64), v)).collect();
        Ok(XVal::Array(self.store()?.new_array(ArrayKind::List, entries)))
    }

    /// ShowOff's AuxStringMapArray functions (docs/spec-notes/ext-unlisted-rest.md §3;
    /// observed: set rest1 of tools/capture/jip_probes.py), by their full names.
    pub(super) fn strmap_function(&mut self, n: &str, c: &XCall) -> Result<Option<XVal>, VmError> {
        let Some(op) = n.strip_prefix("auxstringmaparray") else { return Ok(None) };
        // `0|1`: every temporary (1) or saved (0) map, by name, each as a string map;
        // no array when there is none [observed: sm.getall.0, sm.getall.1, and
        // `so_09_auxstrmap_getall_0_vs_1` (`wave4a_extenders.json`), see
        // docs/spec-notes/ext-unlisted-rest.md §3].
        if op == "getall" {
            let temp = match self.jarg(c, 0)? {
                Some(v) => v.num()? != 0.0,
                None => false,
            };
            let maps = if temp { self.store()?.jip_temp.strmaps.clone() } else { self.store()?.jip.strmaps.clone() };
            if maps.is_empty() {
                return Ok(Some(XVal::Array(0)));
            }
            let mut entries = Vec::new();
            for (name, m) in maps {
                let inner = m.into_values().map(|(k, v)| (XKey::Str(k), v)).collect();
                entries.push((XKey::Str(name), XVal::Array(self.store()?.new_array(ArrayKind::StringMap, inner))));
            }
            return Ok(Some(XVal::Array(self.store()?.new_array(ArrayKind::StringMap, entries))));
        }
        let name = match self.jarg(c, 0)? {
            Some(XVal::Str(s)) => s.to_lowercase(),
            _ => return Err(VmError::Type(format!("{n} without a name"))),
        };
        let v = match op {
            // The setters answer 0 [observed: sm.setfloat, setstring, setref].
            "setfloat" | "setref" | "setstring" => {
                let k = self.strmap_key(n, c)?;
                let v = match self.jarg(c, 2)? {
                    Some(v) => match kind_of(op) {
                        'f' => XVal::Num(v.num()?),
                        'r' => XVal::Ref(v.value()?.as_form().unwrap_or(0)),
                        _ => XVal::Str(match v {
                            XVal::Str(s) => s,
                            other => self.text_of(&other)?,
                        }),
                    },
                    None => return Err(VmError::Expression(format!("{n} without a value"))),
                };
                let m = stores(self.store()?, &name).strmaps.entry(name.clone()).or_default();
                m.entry(k.to_lowercase()).and_modify(|e| e.1 = v.clone()).or_insert((k, v));
                XVal::Num(0.0)
            }
            // Keys ignore case; a getter of another type gives 0, "" or no form
            // [observed: sm.getfloat.case, getfloat.string, getstring.float, getref.float].
            "getfloat" | "getref" | "getstring" | "getvaluetype" => {
                let k = self.strmap_key(n, c)?.to_lowercase();
                let v = stores(self.store()?, &name).strmaps.get(&name).and_then(|m| m.get(&k)).map(|e| e.1.clone());
                if op == "getvaluetype" { XVal::Num(type_code(v.as_ref())) } else { typed(v.as_ref(), kind_of(op)) }
            }
            "getsize" => XVal::Num(stores(self.store()?, &name).strmaps.get(&name).map_or(0, BTreeMap::len) as f64),
            "getkeys" => {
                let keys = stores(self.store()?, &name).strmaps.get(&name).map(|m| m.values().map(|e| XVal::Str(e.0.clone())).collect()).unwrap_or_default();
                self.jlist(keys)?
            }
            // A string map of the keys and values [observed: sm.asarray "3 2"].
            "getasarray" => {
                let m = stores(self.store()?, &name).strmaps.get(&name).cloned().unwrap_or_default();
                let entries = m.into_values().map(|(k, v)| (XKey::Str(k), v)).collect();
                XVal::Array(self.store()?.new_array(ArrayKind::StringMap, entries))
            }
            // A [key, value] list; none after the last [observed: sm.first, next1-3,
            // first.again].
            "getfirst" | "getnext" => {
                let s = stores(self.store()?, &name);
                let at = if op == "getfirst" { 0 } else { s.strmap_cursor.get(&name).copied().unwrap_or(0) };
                let pair = s.strmaps.get(&name).and_then(|m| m.values().nth(at)).cloned();
                s.strmap_cursor.insert(name.clone(), at + 1);
                match pair {
                    Some((k, v)) => self.jlist(vec![XVal::Str(k), v])?,
                    None => XVal::Array(0),
                }
            }
            // `name array [keep]`: the map becomes the array's string keys and values, or
            // (keep non-zero) gains them; answers 0 [observed: sm.fromarray 2 keys,
            // fromarray.keep 3, fromarray.replace 1].
            "setfromarray" => {
                let id = match self.jarg(c, 1)? {
                    Some(XVal::Array(id)) => id,
                    _ => return Ok(Some(XVal::Num(0.0))),
                };
                let keep = match self.jarg(c, 2)? {
                    Some(v) => v.num()? != 0.0,
                    None => false,
                };
                let pairs: Vec<(String, XVal)> = match self.store()?.array(id) {
                    // An array value is left out: the maps are saved, and an array id would
                    // point at nothing after a reload [decision, as RefMapArraySetValue].
                    Some(a) => a.entries.iter().filter_map(|(k, v)| match (k, v) {
                        (_, XVal::Array(_)) => None,
                        (XKey::Str(s), _) => Some((s.clone(), v.clone())),
                        _ => None,
                    }).collect(),
                    None => Vec::new(),
                };
                let s = stores(self.store()?, &name);
                let m = s.strmaps.entry(name.clone()).or_default();
                if !keep {
                    m.clear();
                }
                for (k, v) in pairs {
                    m.insert(k.to_lowercase(), (k, v));
                }
                if m.is_empty() {
                    s.strmaps.remove(&name);
                }
                XVal::Num(0.0)
            }
            // The pairs left; 0 when the key was not there [observed: sm.erase 2 of 3,
            // erase.missing 0].
            "erasekey" => {
                let k = self.strmap_key(n, c)?.to_lowercase();
                let s = stores(self.store()?, &name);
                let mut left = 0;
                if let Some(m) = s.strmaps.get_mut(&name) {
                    if m.remove(&k).is_some() {
                        left = m.len();
                    }
                    if m.is_empty() {
                        s.strmaps.remove(&name);
                    }
                }
                XVal::Num(left as f64)
            }
            // Pairs whose form is gone are dropped; the pairs left [observed: sm.validate
            // 3 with every value alive; hypothesis: dead forms go, as RefMapArrayValidate].
            "validatevalues" => {
                let forms: Vec<(String, u32)> = match stores(self.store()?, &name).strmaps.get(&name) {
                    Some(m) => m.iter().filter_map(|(k, e)| if let XVal::Ref(f) = e.1 { (f != 0).then(|| (k.clone(), f)) } else { None }).collect(),
                    None => Vec::new(),
                };
                let mut dead = Vec::new();
                for (k, f) in forms {
                    if self.host.form_sig(f).is_none() {
                        dead.push(k);
                    }
                }
                let s = stores(self.store()?, &name);
                if let Some(m) = s.strmaps.get_mut(&name) {
                    m.retain(|k, _| !dead.contains(k));
                }
                XVal::Num(s.strmaps.get(&name).map_or(0, BTreeMap::len) as f64)
            }
            // Answers 0 either way [observed: sm.destroy, destroy.again].
            "destroy" => {
                let s = stores(self.store()?, &name);
                s.strmap_cursor.remove(&name);
                s.strmaps.remove(&name);
                XVal::Num(0.0)
            }
            _ => return Ok(None),
        };
        Ok(Some(v))
    }

    /// A string map's key, argument 1, as text.
    fn strmap_key(&mut self, n: &str, c: &XCall) -> Result<String, VmError> {
        match self.jarg(c, 1)? {
            Some(XVal::Str(s)) => Ok(s),
            Some(other) => self.text_of(&other),
            None => Err(VmError::Expression(format!("{n} without a key"))),
        }
    }

    /// The auxiliary-variable and RefMap functions (jip-variable-stores.md), by their
    /// full names; aliases reach here through the catalogue.
    fn jip_store_function(&mut self, n: &str, c: &XCall) -> Result<Option<XVal>, VmError> {
        let (aux, op) = match (n.strip_prefix("auxiliaryvariable"), n.strip_prefix("refmaparray")) {
            (Some(op), _) => (true, op),
            (_, Some(op)) => (false, op),
            _ => return Ok(None),
        };
        if aux && op == "getall" {
            return self.aux_get_all(c).map(Some);
        }
        let name = match self.jarg(c, 0)? {
            Some(XVal::Str(s)) => s.to_lowercase(),
            _ => return Err(VmError::Type(format!("{n} without a name"))),
        };
        let name = self.store_key(&name);
        if aux {
            return self.aux_function(n, op, &name, c);
        }
        let v = match op {
            "setfloat" | "setref" | "setstring" => {
                let v = self.jvalue(n, c, kind_of(op))?;
                let Some(key) = self.jowner_opt(c, 2)? else { return Ok(Some(XVal::Num(0.0))) };
                let m = stores(self.store()?, &name).refmaps.entry(name.clone()).or_default();
                // Kept in FormID order [documented: RefMap Array Functions (JIP), "the
                // elements in a RefMap are ordered by FormID (ascending)"].
                match m.binary_search_by_key(&key, |(k, _)| *k) {
                    Ok(i) => m[i].1 = v,
                    Err(i) => m.insert(i, (key, v)),
                }
                XVal::Num(1.0)
            }
            // `name value key`: the key's element set to a value of any type; answers 0
            // [observed: rest5.refmap.set, refmap.getsize 1; docs/spec-notes/
            // ext-unlisted-rest.md §9].
            "setvalue" => {
                let v = self.jarg(c, 1)?.ok_or_else(|| VmError::Expression(format!("{n} without a value")))?;
                let v = match v {
                    XVal::Array(_) => return Ok(Some(XVal::Num(0.0))),
                    other => other,
                };
                let Some(key) = self.jowner_opt(c, 2)? else { return Ok(Some(XVal::Num(0.0))) };
                let m = stores(self.store()?, &name).refmaps.entry(name.clone()).or_default();
                match m.binary_search_by_key(&key, |(k, _)| *k) {
                    Ok(i) => m[i].1 = v,
                    Err(i) => m.insert(i, (key, v)),
                }
                XVal::Num(0.0)
            }
            // `name key`: a one-element list of the key's value, no array without one
            // [observed: rest5.refmap.get, a list of one].
            "getvalue" => {
                let Some(key) = self.jowner_opt(c, 1)? else { return Ok(Some(XVal::Array(0))) };
                let v = stores(self.store()?, &name).refmaps.get(&name).and_then(|m| m.iter().find(|(k, _)| *k == key)).map(|(_, v)| v.clone());
                match v {
                    Some(v) => self.jlist(vec![v])?,
                    None => XVal::Array(0),
                }
            }
            "getfloat" | "getref" | "getstring" | "gettype" => {
                let Some(key) = self.jowner_opt(c, 1)? else { return Ok(Some(if op == "gettype" { XVal::Num(type_code(None)) } else { typed(None, kind_of(op)) })) };
                let m = stores(self.store()?, &name).refmaps.get(&name);
                let v = m.and_then(|m| m.iter().find(|(k, _)| *k == key)).map(|(_, v)| v);
                if op == "gettype" { XVal::Num(type_code(v)) } else { typed(v, kind_of(op)) }
            }
            "getsize" => XVal::Num(stores(self.store()?, &name).refmaps.get(&name).map_or(0, Vec::len) as f64),
            "getkeys" => {
                let keys = stores(self.store()?, &name).refmaps.get(&name).map(|m| m.iter().map(|(k, _)| XVal::Ref(*k)).collect()).unwrap_or_default();
                self.jlist(keys)?
            }
            "getall" => {
                let pairs = stores(self.store()?, &name).refmaps.get(&name).cloned().unwrap_or_default();
                let entries = pairs.into_iter().map(|(k, v)| (XKey::Num(f64::from(k)), v)).collect();
                XVal::Array(self.store()?.new_array(ArrayKind::Map, entries))
            }
            // [documented: RefMapArrayGetNext; observed: Next after the last pair is
            // ar_Null] a [key, value] list.
            "getfirst" | "getnext" => {
                let s = stores(self.store()?, &name);
                let at = if op == "getfirst" { 0 } else { s.cursor.get(&name).copied().unwrap_or(0) };
                let pair = s.refmaps.get(&name).and_then(|m| m.get(at)).cloned();
                s.cursor.insert(name.clone(), at + 1);
                match pair {
                    Some((k, v)) => self.jlist(vec![XVal::Ref(k), v])?,
                    None => XVal::Array(0),
                }
            }
            // The pairs left [observed: gaps6, erasing the only key gives 0].
            "erase" => {
                let Some(key) = self.jowner_opt(c, 1)? else { return Ok(Some(XVal::Num(0.0))) };
                let s = stores(self.store()?, &name);
                let mut left = 0;
                if let Some(m) = s.refmaps.get_mut(&name) {
                    m.retain(|(k, _)| *k != key);
                    left = m.len();
                    if m.is_empty() {
                        s.refmaps.remove(&name);
                    }
                }
                XVal::Num(left as f64)
            }
            "destroy" => {
                let s = stores(self.store()?, &name);
                s.cursor.remove(&name);
                XVal::Num(f64::from(u8::from(s.refmaps.remove(&name).is_some())))
            }
            // [documented: RefMapArrayValidate; observed: a live key gives 1] the keys left.
            "validate" => {
                let keys: Vec<u32> = stores(self.store()?, &name).refmaps.get(&name).map(|m| m.iter().map(|(k, _)| *k).collect()).unwrap_or_default();
                let mut dead = Vec::new();
                for k in keys {
                    if self.host.form_sig(k).is_none() {
                        dead.push(k);
                    }
                }
                let s = stores(self.store()?, &name);
                if let Some(m) = s.refmaps.get_mut(&name) {
                    m.retain(|(k, _)| !dead.contains(k));
                }
                XVal::Num(s.refmaps.get(&name).map_or(0, Vec::len) as f64)
            }
            _ => return Ok(None),
        };
        Ok(Some(v))
    }

    /// AuxiliaryVariableSet…/Get…/GetType/GetSize/Erase/GetAsArray.
    fn aux_function(&mut self, n: &str, op: &str, name: &str, c: &XCall) -> Result<Option<XVal>, VmError> {
        let v = match op {
            // [documented: AuxiliaryVariableSetFloat; observed: -1 appends, an index past
            // the end does nothing, a new type replaces the element].
            "setfloat" | "setref" | "setstring" => {
                let v = self.jvalue(n, c, kind_of(op))?;
                // Setting no form leaves no element [observed: jip-events ev.source.type,
                // the null source stored with AuxVarSetRef].
                if v == XVal::Ref(0) {
                    return Ok(Some(XVal::Num(0.0)));
                }
                let i = self.jindex(c, 2, 0)?;
                let Some(o) = self.jowner_opt(c, 3)? else { return Ok(Some(XVal::Num(0.0))) };
                let s = stores(self.store()?, name);
                let list = s.aux.entry(o).or_default().entry(name.to_string()).or_default();
                let ok = match usize::try_from(i) {
                    _ if i == -1 => {
                        list.push(v);
                        true
                    }
                    Ok(at) if at < list.len() => {
                        list[at] = v;
                        true
                    }
                    Ok(at) if at == list.len() => {
                        list.push(v);
                        true
                    }
                    _ => false,
                };
                prune(s, o, name);
                XVal::Num(f64::from(u8::from(ok)))
            }
            "getfloat" | "getref" | "getstring" | "gettype" => {
                let i = self.jindex(c, 1, 0)?;
                let Some(o) = self.jowner_opt(c, 2)? else { return Ok(Some(if op == "gettype" { XVal::Num(type_code(None)) } else { typed(None, kind_of(op)) })) };
                let list = stores(self.store()?, name).aux.get(&o).and_then(|m| m.get(name));
                let v = list.and_then(|l| if i == -1 { l.last() } else { usize::try_from(i).ok().and_then(|i| l.get(i)) });
                if op == "gettype" { XVal::Num(type_code(v)) } else { typed(v, kind_of(op)) }
            }
            "getsize" => {
                let Some(o) = self.jowner_opt(c, 1)? else { return Ok(Some(XVal::Num(0.0))) };
                XVal::Num(stores(self.store()?, name).aux.get(&o).and_then(|m| m.get(name)).map_or(0, Vec::len) as f64)
            }
            // [documented: AuxiliaryVariableErase; observed: the new count, later elements
            // move down; the whole variable gives -1].
            "erase" => {
                let i = self.jindex(c, 1, -1)?;
                let Some(o) = self.jowner_opt(c, 2)? else { return Ok(Some(XVal::Num(if i == -1 { -1.0 } else { 0.0 }))) };
                let s = stores(self.store()?, name);
                let left = match s.aux.get_mut(&o) {
                    Some(m) if i == -1 => {
                        m.remove(name);
                        -1.0
                    }
                    Some(m) => match m.get_mut(name) {
                        Some(l) => {
                            if let Some(at) = usize::try_from(i).ok().filter(|&at| at < l.len()) {
                                l.remove(at);
                            }
                            l.len() as f64
                        }
                        None => 0.0,
                    },
                    None if i == -1 => -1.0,
                    None => 0.0,
                };
                prune(s, o, name);
                XVal::Num(left)
            }
            // `name array [owner]`: the variable becomes the array's values; returns how
            // many [observed: gaps6, ar_List 5 6 7 gives 3, size 3, element 2 is 7].
            "setfromarray" => {
                let Some(o) = self.jowner_opt(c, 2)? else { return Ok(Some(XVal::Num(0.0))) };
                let id = match self.jarg(c, 1)? {
                    Some(XVal::Array(id)) => id,
                    _ => return Ok(Some(XVal::Num(0.0))),
                };
                let vals: Vec<XVal> = self.store()?.array(id).map(|a| a.entries.iter().map(|(_, v)| v.clone()).collect()).unwrap_or_default();
                let count = vals.len();
                let s = stores(self.store()?, name);
                if count == 0 {
                    if let Some(m) = s.aux.get_mut(&o) {
                        m.remove(name);
                    }
                } else {
                    s.aux.entry(o).or_default().insert(name.to_string(), vals);
                }
                prune(s, o, name);
                XVal::Num(count as f64)
            }
            "getasarray" => {
                let Some(o) = self.jowner_opt(c, 1)? else { return self.jlist(Vec::new()).map(Some) };
                let l = stores(self.store()?, name).aux.get(&o).and_then(|m| m.get(name)).cloned().unwrap_or_default();
                self.jlist(l)?
            }
            _ => return Ok(None),
        };
        Ok(Some(v))
    }

    /// The key a store keeps a name under: the name cut to 79 characters, and a private
    /// name (no `_` after an optional `*`) tied to the calling plugin, so that each mod
    /// has its own [documented: Auxiliary-Variable Functions (JIP), JIP.xml]. A runtime
    /// script belongs to no plugin: its names are shared [observed: aux.class.*, each
    /// variable showed in both its private and its public class].
    fn store_key(&mut self, name: &str) -> String {
        let name: String = name.chars().take(79).collect();
        let public = name.trim_start_matches('*').starts_with('_');
        match self.host.calling_plugin() {
            Some(p) if !public => format!("{name}{PRIVATE}{p}"),
            _ => name,
        }
    }

    /// AuxiliaryVariableGetAll class [base]: a string map of name -> list of values.
    /// Classes 0 and 2 hold the saved variables, 1 and 3 the temporary ones; 0 and 1 the
    /// calling plugin's private ones, 2 and 3 the public ones [documented: AuxVar classes,
    /// JIP.xml]. From a runtime script every variable shows in both [observed: aux.class.*].
    fn aux_get_all(&mut self, c: &XCall) -> Result<XVal, VmError> {
        let class = self.jindex(c, 0, 0)?;
        let o = self.jowner("AuxiliaryVariableGetAll", c, 1).ok();
        let plugin = self.host.calling_plugin();
        let x = self.store()?;
        let s = if class % 2 == 1 { &x.jip_temp } else { &x.jip };
        let all: Vec<(String, Vec<XVal>)> = o.and_then(|o| s.aux.get(&o)).map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default();
        let private_class = class < 2;
        let vars: Vec<(String, Vec<XVal>)> = all
            .into_iter()
            .filter_map(|(k, v)| {
                let (bare, owner) = match k.split_once(PRIVATE) {
                    Some((b, p)) => (b.to_string(), Some(p.to_string())),
                    None => (k.clone(), None),
                };
                let public = bare.trim_start_matches('*').starts_with('_');
                let shown = match &plugin {
                    None => owner.is_none(),
                    Some(p) if private_class => !public && owner.as_deref() == Some(p.as_str()),
                    Some(_) => public,
                };
                shown.then_some((bare, v))
            })
            .collect();
        let mut entries = Vec::new();
        for (k, l) in vars {
            let inner = l.into_iter().enumerate().map(|(i, v)| (XKey::Num(i as f64), v)).collect();
            entries.push((XKey::Str(k), XVal::Array(x.new_array(ArrayKind::List, inner))));
        }
        Ok(XVal::Array(x.new_array(ArrayKind::StringMap, entries)))
    }

    /// A form argument as a base form: a reference gives its base.
    fn jip_base(&mut self, v: XVal) -> Result<u32, VmError> {
        let f = v.value()?.as_form().unwrap_or(0);
        Ok(self.host.base_form(f).unwrap_or(f))
    }

    /// The form `hex` (without the mod index) of plugin `name`.
    fn plugin_form(&mut self, name: &str, hex: &str) -> Option<u32> {
        let i = self.host.mod_index(name.trim())?;
        let low = u32::from_str_radix(hex.trim(), 16).ok()? & 0x00FF_FFFF;
        let f = (i << 24) | low;
        self.host.form_sig(f).map(|_| f)
    }
}

/// ValidateRegex's code for a pattern error [documented: ValidateRegex lists the codes;
/// which message is which is our reading, hypothesis]: 5 brackets, 6 parentheses,
/// 7 braces, 3 escape, 4 back-reference, 9 range, 11 repeat, else 14 (unknown).
fn regex_error_code(msg: &str) -> f64 {
    let m = msg.to_ascii_lowercase();
    let has = |w: &[&str]| w.iter().any(|w| m.contains(w));
    if has(&["class", "bracket"]) {
        5.0
    } else if has(&["paren", "group"]) {
        6.0
    } else if has(&["brace", "counted", "repetition quantifier"]) {
        7.0
    } else if has(&["backref"]) {
        4.0
    } else if has(&["escape"]) {
        3.0
    } else if has(&["range"]) {
        9.0
    } else if has(&["repeat", "repetition"]) {
        11.0
    } else {
        14.0
    }
}

/// FATan and FATan2 as the game computes them, in degrees: an odd polynomial of degree 11
/// in 32-bit floats over the smaller over the larger of |y| and |x|, then π/2 less it when
/// |y| is the larger, π less it when x is negative, negated when y is [observed: the
/// jip-probes sets rules1 and rules2 (#1025), 29 readings to eight and ten decimals; the
/// coefficients are a least-squares fit of the 21 readings in (0, 1], residual 4.6e-8
/// radians, the 32-bit noise there; every reading is reproduced within 0.000014°]. The
/// quadrant constants are π and π/2 as 32-bit floats [observed: FATan2 1 0 = 90.0000025,
/// FATan2 1 -1 = 135.0002]. FATan x is FATan2 x 1.
fn fast_atan2(y: f64, x: f64) -> f64 {
    const C: [f32; 6] = [0.999_995_6, -0.332_994_4, 0.195_635, -0.121_236_9, 0.057_475_3, -0.013_479_8];
    let (ay, ax) = ((y as f32).abs(), (x as f32).abs());
    if ay == 0.0 && ax == 0.0 {
        return 0.0;
    }
    let poly = |r: f32| {
        let r2 = r * r;
        let mut s = C[5];
        for c in C[..5].iter().rev() {
            s = s * r2 + c;
        }
        s * r
    };
    let mut a = if ay > ax { std::f32::consts::FRAC_PI_2 - poly(ax / ay) } else { poly(ay / ax) };
    if x < 0.0 {
        a = std::f32::consts::PI - a;
    }
    if y < 0.0 {
        a = -a;
    }
    f64::from(a).to_degrees()
}

#[cfg(test)]
mod hit_info_tests {
    use crate::compile::{compile, Env, FormInfo};
    use crate::vm::{ArgValue, ArrayKind, ExtStore, Host, RunOpts, Value, VarKind, Vm, XKey, XVal};
    use crate::{decode, FunctionTable, RefIndex, VarDecl, VarRef};
    use std::collections::HashMap;

    struct NoForms;
    impl Env for NoForms {
        fn form(&self, _: &str) -> Option<FormInfo> {
            None
        }
        fn info(&self, _: u32) -> Option<FormInfo> {
            None
        }
        fn editor_id(&self, _: u32) -> Option<String> {
            None
        }
        fn local_form(&self, local: u32) -> Option<u32> {
            Some(local)
        }
        fn script_vars(&self, _: u32) -> Option<Vec<VarDecl>> {
            None
        }
    }

    struct HitHost {
        ext: ExtStore,
        vars: HashMap<u16, Value>,
        calling: Option<u32>,
        populated: bool,
    }

    impl Host for HitHost {
        fn var_kind(&mut self, _: &VarRef) -> VarKind {
            VarKind::Value
        }
        fn ext(&mut self) -> Option<&mut ExtStore> {
            Some(&mut self.ext)
        }
        fn get_var(&mut self, var: &VarRef) -> Result<Value, String> {
            let crate::ir::VarSlot::Local { index, .. } = var.var else { return Ok(Value::Num(0.0)) };
            Ok(self.vars.get(&index).copied().unwrap_or(Value::Num(0.0)))
        }
        fn set_var(&mut self, var: &VarRef, value: Value) -> Result<(), String> {
            if let crate::ir::VarSlot::Local { index, .. } = var.var {
                self.vars.insert(index, value);
            }
            Ok(())
        }
        fn ref_value(&mut self, _: RefIndex) -> Result<Value, String> {
            Ok(Value::Ref(0))
        }
        fn call(&mut self, _: u16, _: Option<Value>, _: &[ArgValue]) -> Result<Value, String> {
            Ok(Value::Num(0.0))
        }
        fn calling_ref(&mut self) -> Option<u32> {
            self.calling
        }
        fn hit_value(&mut self, actor: u32, what: &str, k: i64) -> Option<XVal> {
            if !self.populated || actor != 0x0000_0014 {
                return None;
            }
            Some(match what {
                "attacker" => XVal::Ref(0x0000_00AA),
                "weapon" => XVal::Ref(0x0000_00BB),
                "projectile" => XVal::Ref(0xFF00_00CC),
                "health" => XVal::Num(42.0),
                "limb" => XVal::Num(12.5),
                "fatigue" => XVal::Num(250.0),
                "base_weapon" => XVal::Num(35.0),
                "flag" => XVal::Num(if k == 8 { 1.0 } else { 0.0 }),
                "3d" => {
                    let vals: &[f64] = match k {
                        0 => &[10.0, 20.0, 30.0],
                        1 => &[0.0, 1.0, 0.0],
                        2 => &[10.0, 20.0, 30.0, 0.0, 1.0, 0.0],
                        _ => return None,
                    };
                    let entries = vals.iter().enumerate().map(|(i, &v)| (XKey::Num(i as f64), XVal::Num(v))).collect();
                    XVal::Array(self.ext.new_array(ArrayKind::List, entries))
                }
                _ => return None,
            })
        }
    }

    fn run_block(host: &mut HitHost, decls: &str, body: &str) {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
        let table = FunctionTable::load(&p).expect("functions.json loads");
        let src = format!("{decls}\nbegin GameMode\n{body}\nend\n");
        let compiled = compile(&src, &table, &NoForms, None);
        let errors: Vec<_> = compiled.errors().collect();
        assert!(errors.is_empty(), "compile errors: {errors:?}");
        let dec = decode(&compiled.bytes, &table);
        assert!(dec.report.issues.is_empty(), "decode issues: {:?}", dec.report.issues);
        host.vars.clear();
        Vm::new(&table, host, RunOpts { strict: true, ..RunOpts::default() })
            .run(&dec.program.blocks[0].body)
            .expect("runs");
    }

    #[test]
    fn hit_info_queries_route_to_host_and_fall_back_when_absent() {
        let mut host = HitHost { ext: ExtStore::default(), vars: HashMap::new(), calling: Some(0x0000_0014), populated: false };
        let decls = "ref rProj\nfloat fFat\nfloat fBase\nfloat fFlag8\nfloat fSz0\nfloat fSz1\nfloat fSz2\nfloat fSzBad\nfloat fP1\nfloat fA2\nfloat fB5";
        let body = "\
let rProj := GetHitProjectile
let fFat := GetHitFatigueDamage
let fBase := GetHitBaseWeaponDamage
let fFlag8 := GetHitExtendedFlag 8
let fSz0 := Ar_Size (GetHit3DData 0)
let fSz1 := Ar_Size (GetHit3DData 1)
let fSz2 := Ar_Size (GetHit3DData 2)
let fSzBad := Ar_Size (GetHit3DData 9)";

        // Absent hit -> null ref, 0.0, ar_Null (Ar_Size == -1).
        run_block(&mut host, decls, body);
        assert_eq!(host.vars[&1], Value::Ref(0));
        assert_eq!(host.vars[&2], Value::Num(0.0));
        assert_eq!(host.vars[&3], Value::Num(0.0));
        assert_eq!(host.vars[&4], Value::Num(0.0));
        assert_eq!(host.vars[&5], Value::Num(-1.0));
        assert_eq!(host.vars[&6], Value::Num(-1.0));
        assert_eq!(host.vars[&7], Value::Num(-1.0));
        assert_eq!(host.vars[&8], Value::Num(-1.0));

        // Populated hit -> routed values from Host::hit_value.
        host.populated = true;
        let body_full = format!(
            "{body}\nlet fP1 := (GetHit3DData 0)[1]\nlet fA2 := (GetHit3DData 1)[1]\nlet fB5 := (GetHit3DData 2)[4]"
        );
        run_block(&mut host, decls, &body_full);
        assert_eq!(host.vars[&1], Value::Ref(0xFF00_00CC));
        assert_eq!(host.vars[&2], Value::Num(250.0));
        assert_eq!(host.vars[&3], Value::Num(35.0));
        assert_eq!(host.vars[&4], Value::Num(1.0));
        assert_eq!(host.vars[&5], Value::Num(3.0));
        assert_eq!(host.vars[&6], Value::Num(3.0));
        assert_eq!(host.vars[&7], Value::Num(6.0));
        assert_eq!(host.vars[&8], Value::Num(-1.0));
        assert_eq!(host.vars[&9], Value::Num(20.0));
        assert_eq!(host.vars[&10], Value::Num(1.0));
        assert_eq!(host.vars[&11], Value::Num(1.0));

        // Different calling ref receives absent defaults.
        host.calling = Some(0x0000_0099);
        run_block(&mut host, decls, body);
        assert_eq!(host.vars[&1], Value::Ref(0));
        assert_eq!(host.vars[&2], Value::Num(0.0));
        assert_eq!(host.vars[&5], Value::Num(-1.0));
    }
}
