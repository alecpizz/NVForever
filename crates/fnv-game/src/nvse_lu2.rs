//! Lutana NVSE (LU, merged into JIP LN), batch 2 (#1009): map markers, radio stations,
//! hotkeys with their unit's health and mods, the INI writers, skill magazines and
//! ingestible flags, the quest items switch, form counts by type and the plugins that
//! hold a form [documented: each page in the wiki export; docs/spec-notes/ext-lutana.md
//! §9]. The INI writers are in `crate::mod_files`. Setters change the reference or the
//! record for the session.

use crate::game::{Game, RunCtx};
use fnv_script::vm::{ArrayKind, XKey, XVal};
use fnv_sim::FormId;
use std::collections::HashMap;

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

fn yes(b: bool) -> XVal {
    XVal::Num(f64::from(u8::from(b)))
}

fn list(g: &mut Game, items: Vec<XVal>) -> XVal {
    let entries = items.into_iter().enumerate().map(|(i, v)| (XKey::Num(i as f64), v)).collect();
    XVal::Array(g.ext.new_array(ArrayKind::List, entries))
}

/// The map marker flags [documented: xEdit REFR FNAM; GetMapMarkerHidden,
/// GetMapMarkerTravel, SetMapMarkerVisible].
pub const MARKER_VISIBLE: u8 = 1;
pub const MARKER_TRAVEL: u8 = 2;
pub const MARKER_HIDDEN: u8 = 4;

/// This batch's session state (not saved).
#[derive(Debug, Default)]
pub struct State {
    /// SetMapMarkerHidden / Travel / Visible's flags and SetMapMarkerType's types, by
    /// marker reference (`Game::marker_flags`, read by the Pip-Boy's map too).
    pub marker_flags: HashMap<FormId, u8>,
    /// Mask of `MARKER_*` flag bits explicitly set or cleared by a script
    /// (`SetMapMarkerVisible`, `SetMapMarkerTravel`, `SetMapMarkerHidden`), distinct
    /// from the authored `FNAM` bits.
    pub marker_flag_overrides: HashMap<FormId, u8>,
    pub marker_kind: HashMap<FormId, u8>,
    /// SetRadio…'s radio data, by station reference.
    pub radio: HashMap<FormId, fnv_sim::refs::RadioData>,
    /// SetHotkey's health and mod flags of each hotkey's unit (slot 0 is key 1).
    pub hotkey_data: [Option<(f32, u8)>; 8],
}

impl Game<'_> {
    /// A map marker's flags as they stand: a script's, else the record's; `None` for a
    /// reference that is no map marker.
    pub fn marker_flags(&self, r: FormId) -> Option<u8> {
        let m = self.world.placed.get(r).and_then(|p| p.map_marker)?;
        Some(self.other.lutana.lu2.marker_flags.get(&r).copied().unwrap_or(m.flags))
    }

    /// Whether a script explicitly overrode `bit` (`MARKER_VISIBLE`, `MARKER_TRAVEL`, or
    /// `MARKER_HIDDEN`) on map marker `r`: `Some(true)` when set on, `Some(false)` when
    /// cleared, `None` when the bit still comes from the authored `FNAM` record (or `r`
    /// is no map marker).
    pub fn marker_flag_override(&self, r: FormId, bit: u8) -> Option<bool> {
        let mask = self.other.lutana.lu2.marker_flag_overrides.get(&r).copied()?;
        if mask & bit == 0 {
            return None;
        }
        Some(self.marker_flags(r)? & bit != 0)
    }

    /// Clear a script override bit on `r` (used when `ShowMap` re-enables travel after a
    /// prior `SetMapMarkerTravel 0`).
    pub(crate) fn clear_marker_flag_override(&mut self, r: FormId, bit: u8) {
        if let Some(mask) = self.other.lutana.lu2.marker_flag_overrides.get_mut(&r) {
            *mask &= !bit;
            if *mask == 0 {
                self.other.lutana.lu2.marker_flag_overrides.remove(&r);
            }
        }
    }

    /// Effective `MARKER_TRAVEL` bit for map marker `r` (`GetMapMarkerTravel`,
    /// `GetMapMarkerVisible`, and Pip-Boy fast-travel):
    /// - Explicit `SetMapMarkerTravel` override wins (`Some(on)`), cleared if a subsequent
    ///   `ShowMap r 1` re-enables travel.
    /// - Otherwise `true` when the player discovered `r` (`found_markers`), `ShowMap r 1`
    ///   or `ShowAllMapMarkers` with travel enabled it on a shown marker, or the authored
    ///   `FNAM` record sets BOTH `MARKER_VISIBLE` and `MARKER_TRAVEL` (`FNAM & 3 == 3`)
    ///   [observed: observer check 252 (#3468, `o31_mapmarker_visible` control + probe
    ///   plugin: authored `FNAM=3` reads Vis=2/Trv=1 while authored `FNAM=2` alone reads
    ///   Vis=0/Trv=0; `ShowMap 1` and discovery set Vis=2/Trv=1) and the 2026-11-22 live
    ///   map-marker probe documented in `docs/spec-notes/ext-lutana.md` §9
    ///   (`SetMapMarkerTravel 1` alone reads Vis=0/Trv=1; `SetMapMarkerTravel 0` on
    ///   `FNAM=3` reads Vis=1/Trv=0)].
    pub fn marker_travel(&self, r: FormId) -> bool {
        let Some(m) = self.world.placed.get(r).and_then(|p| p.map_marker) else {
            return false;
        };
        if let Some(on) = self.marker_flag_override(r, MARKER_TRAVEL) {
            return on;
        }
        if self.player.found_markers.contains(&r) {
            return true;
        }
        if m.flags & (MARKER_VISIBLE | MARKER_TRAVEL) == (MARKER_VISIBLE | MARKER_TRAVEL) {
            return true;
        }
        let flags = self.marker_flags(r).unwrap_or(m.flags);
        let hidden = flags & MARKER_HIDDEN != 0;
        let shown = self.map_marker_shown(r, hidden);
        shown && (self.player.travel_markers.contains(&r) || self.player.all_markers.is_some_and(|a| a.shown && a.travel))
    }

    /// A map marker's type as it stands (the icon it shows).
    pub fn marker_kind(&self, r: FormId) -> Option<u8> {
        let m = self.world.placed.get(r).and_then(|p| p.map_marker)?;
        Some(self.other.lutana.lu2.marker_kind.get(&r).copied().unwrap_or(m.kind))
    }

    /// A station reference's radio data as it stands: a script's, else XRDO's.
    pub fn radio_data(&self, r: FormId) -> Option<fnv_sim::refs::RadioData> {
        self.other.lutana.lu2.radio.get(&r).copied().or_else(|| self.world.placed.get(r).and_then(|p| p.radio))
    }

    /// Change a station's radio data, and the copy GetAvailableRadios reads
    /// (`nvse_gaps::RadioSite`).
    fn set_radio(&mut self, r: FormId, f: impl FnOnce(&mut fnv_sim::refs::RadioData)) -> bool {
        let Some(mut d) = self.radio_data(r) else { return false };
        f(&mut d);
        self.other.lutana.lu2.radio.insert(r, d);
        if let Some(s) = self.gaps.radio_sites.iter_mut().flatten().find(|s| s.station == r) {
            s.broadcast.radius = d.radius;
            s.broadcast.static_percent = d.static_percent;
            s.broadcast.range = fnv_world::radio::BroadcastRange::of_code(d.range);
        }
        true
    }

    fn set_marker_flag(&mut self, r: FormId, bit: u8, on: bool) {
        if let Some(f) = self.marker_flags(r) {
            self.other.lutana.lu2.marker_flags.insert(r, if on { f | bit } else { f & !bit });
            *self.other.lutana.lu2.marker_flag_overrides.entry(r).or_insert(0) |= bit;
            if !on && bit == MARKER_VISIBLE {
                self.player.map_markers.remove(&r);
            }
            if !on && bit == MARKER_TRAVEL {
                self.player.travel_markers.remove(&r);
            }
        }
    }
}

/// Whether an ingestible is a skill magazine: one of its effects has a condition HasPerk
/// Retention or HasPerk Comprehension [documented: IsSkillMagazine]. The perks are found
/// by their Editor IDs [hypothesis: the page names them so].
fn skill_magazine(g: &Game, item: FormId) -> bool {
    const HAS_PERK: u16 = 449;
    let perks: Vec<FormId> = ["Retention", "Comprehension"].iter().filter_map(|e| g.data.by_edid(e)).collect();
    g.data.effect_items.get(&item).into_iter().flatten().flat_map(|e| &e.conditions).any(|c| {
        c.function == HAS_PERK && matches!(c.params.first().and_then(|p| p.value()), Some(fnv_script::vm::Value::Ref(r)) if perks.contains(&FormId(r)))
    })
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, _ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let zero = XVal::Num(0.0);
    let v = match name {
        // ---- map markers (REFR XMRK: FNAM flags, TNAM type) ----
        // [documented: IsMapMarker, GetMapMarkerHidden, GetMapMarkerTravel,
        // GetMapMarkerType and their setters]. A reference that is no map marker reads 0
        // and is not changed.
        "ismapmarker" => yes(subject.and_then(|r| g.marker_flags(r)).is_some()),
        "getmapmarkerhidden" => yes(subject.and_then(|r| g.marker_flags(r)).is_some_and(|f| f & MARKER_HIDDEN != 0)),
        "getmapmarkertravel" => yes(subject.is_some_and(|r| g.marker_travel(r))),
        "getmapmarkertype" => XVal::Num(subject.and_then(|r| g.marker_kind(r)).map_or(0.0, f64::from)),
        "setmapmarkerhidden" | "setmapmarkertravel" | "setmapmarkervisible" => {
            let bit = match name {
                "setmapmarkerhidden" => MARKER_HIDDEN,
                "setmapmarkertravel" => MARKER_TRAVEL,
                _ => MARKER_VISIBLE,
            };
            if let Some(r) = subject {
                g.set_marker_flag(r, bit, num(args, 0, 0.0) != 0.0);
            }
            zero
        }
        // `typeID`, 0 None … 14 Vault [documented: the page's table]; another number is
        // kept as given [hypothesis].
        "setmapmarkertype" => {
            if let Some(r) = subject.filter(|&r| g.marker_kind(r).is_some()) {
                g.other.lutana.lu2.marker_kind.insert(r, num(args, 0, 0.0).clamp(0.0, 255.0) as u8);
            }
            zero
        }
        // ---- radio stations (REFR XRDO) ----
        // [documented: IsRadioRef, "the Radio Station checkbox on a TalkingActivator";
        // GetRadioBroadcastType 0 Radius … 4 Current Cell, GetRadioRadius,
        // GetRadioStatic, GetRadioPosRef, and the setters]. The engine's radio plays from
        // the record's data [not built: it reading the session's changes].
        "isradioref" => yes(subject.and_then(|r| g.radio_data(r)).is_some()),
        "getradiobroadcasttype" => XVal::Num(subject.and_then(|r| g.radio_data(r)).map_or(0.0, |d| f64::from(d.range))),
        "getradioradius" => XVal::Num(subject.and_then(|r| g.radio_data(r)).map_or(0.0, |d| f64::from(d.radius))),
        "getradiostatic" => XVal::Num(subject.and_then(|r| g.radio_data(r)).map_or(0.0, |d| f64::from(d.static_percent))),
        "getradioposref" => XVal::Ref(subject.and_then(|r| g.radio_data(r)).and_then(|d| d.position_ref).map_or(0, |p| p.0)),
        "setradiobroadcasttype" | "setradioradius" | "setradiostatic" | "setradioposref" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            let x = num(args, 0, 0.0);
            let pos = form(args, 0);
            g.set_radio(r, |d| match name {
                "setradiobroadcasttype" => {
                    let v = x.trunc();
                    if v.is_finite() && (0.0..=4.0).contains(&v) {
                        d.range = v as u32;
                    }
                }
                "setradioradius" => d.radius = x.max(0.0) as f32,
                "setradiostatic" => {
                    if x.is_finite() && (0.0..=100.0).contains(&x) {
                        d.static_percent = x as f32;
                    }
                }
                _ => d.position_ref = pos,
            });
            zero
        }
        // ---- hotkeys ----
        // Every hotkey cleared [documented: ClearAllHotkeys].
        "clearallhotkeys" => {
            g.other.hotkeys = [None; 8];
            g.other.lutana.lu2.hotkey_data = [None; 8];
            zero
        }
        // `index item health modFlags`: SetHotkeyItem with the unit's health (-1 any) and
        // weapon mod flags [documented: SetHotkey]; keys 1-8 as SetHotkeyItem, an item the
        // player carries (see docs/spec-notes/ext-lutana.md §9).
        "sethotkey" => {
            let Some(k) = usize::try_from(num(args, 0, 0.0) as i64 - 1).ok().filter(|&k| k < 8) else { return Some(Ok(zero)) };
            let Some(item) = form(args, 1) else { return Some(Ok(zero)) };
            let carried = g.world.inventory_mut(FormId::PLAYER_REF).is_ok_and(|inv| inv.count(item) > 0);
            if carried {
                g.other.hotkeys[k] = Some(item);
                g.other.lutana.lu2.hotkey_data[k] = Some((num(args, 2, -1.0) as f32, num(args, 3, 0.0).clamp(0.0, 7.0) as u8));
            }
            zero
        }
        // The hotkeys as an array of eight [item, health, mods] entries (0 for an empty
        // key), for RestoreHotkeys [documented: SaveHotkeys, deprecated; hypothesis: the
        // entries' layout].
        "savehotkeys" => {
            let rows: Vec<(FormId, (f32, u8))> = (0..8).map(|k| (g.other.hotkeys[k].unwrap_or(FormId(0)), g.other.lutana.lu2.hotkey_data[k].unwrap_or((-1.0, 0)))).collect();
            let mut out = Vec::new();
            for (item, (h, m)) in rows {
                out.push(list(g, vec![XVal::Ref(item.0), XVal::Num(f64::from(h)), XVal::Num(f64::from(m))]));
            }
            list(g, out)
        }
        // `array`: the hotkeys SaveHotkeys gave back [documented: RestoreHotkeys].
        "restorehotkeys" => {
            let Some(XVal::Array(id)) = args.first() else { return Some(Ok(zero)) };
            let rows: Vec<XVal> = g.ext.array(*id).map(|a| a.entries.iter().map(|(_, v)| v.clone()).collect()).unwrap_or_default();
            for (k, row) in rows.into_iter().take(8).enumerate() {
                let XVal::Array(r) = row else { continue };
                let cells: Vec<XVal> = g.ext.array(r).map(|a| a.entries.iter().map(|(_, v)| v.clone()).collect()).unwrap_or_default();
                let item = match cells.first() {
                    Some(XVal::Ref(i)) if *i != 0 => Some(FormId(*i)),
                    _ => None,
                };
                g.other.hotkeys[k] = item;
                g.other.lutana.lu2.hotkey_data[k] = item.map(|_| (num(&cells, 1, -1.0) as f32, num(&cells, 2, 0.0) as u8));
            }
            zero
        }
        // ---- ingestibles ----
        "isskillmagazine" => yes(form(args, 0).is_some_and(|i| skill_magazine(g, i))),
        // `ingestible flag value`: 2 Food, 4 Medicine [documented: SetIngestibleFlag];
        // GetIngestibleFlag and the Medicine and Survival boosts of using it read it
        // (`Game::ingest_on`).
        "setingestibleflag" => {
            let (Some(i), bit) = (form(args, 0).filter(|&i| g.data.sig(g.unclone(i)) == Some(*b"ALCH")), num(args, 1, 0.0) as i64) else { return Some(Ok(zero)) };
            if bit == 2 || bit == 4 {
                let now = u32::from(g.effect_flags(i));
                let new = if num(args, 2, 0.0) != 0.0 { now | bit as u32 } else { now & !(bit as u32) };
                g.field_overrides.insert((i, "ENIT/flags".into()), XVal::Num(f64::from(new)));
            }
            zero
        }
        // SetShowQuestItems' switch, on by default [documented: GetShowQuestItems;
        // SetShowQuestItems, "Shows (1, default)"].
        "getshowquestitems" => yes(g.player.show_quest_items.unwrap_or(true)),
        // A script's SetShowQuestItems by name (source run as text; the catalogue row has
        // no opcode), as `functions::items::set_show_quest_items`.
        "setshowquestitems" => {
            g.player.show_quest_items = Some(num(args, 0, 1.0) != 0.0);
            zero
        }
        // `array type`: how many forms of the array have that form type (GetType's codes)
        // [documented: GetFormCountType]. Since JIP v40 the first parameter is the array's
        // ID as an integer [documented: the page]: an array variable passes it, an array
        // expression in its place did not compile in the original [observed 2026-11-08:
        // lu2.formcount.alch with `(ar_List …)`], so the ID may come as a number.
        "getformcounttype" => {
            let id = match args.first() {
                Some(XVal::Array(id)) => *id,
                Some(XVal::Num(x)) if *x >= 0.0 => *x as u32,
                _ => return Some(Ok(zero)),
            };
            let want = num(args, 1, -1.0) as i32;
            let forms: Vec<FormId> = g.ext.array(id).map(|a| a.entries.iter().filter_map(|(_, v)| if let XVal::Ref(r) = v { Some(FormId(*r)) } else { None }).collect()).unwrap_or_default();
            let n = forms.into_iter().filter(|&f| g.data.sig(f).as_ref().and_then(crate::functions::items::form_type_code) == Some(want));
            XVal::Num(n.count() as f64)
        }
        // `form`: the plugins that hold it, defining or overriding, in load order
        // [documented: GetFormMods' example].
        "getformmods" => {
            let Some(f) = form(args, 0) else { return Some(Ok(list(g, Vec::new()))) };
            let names: Vec<XVal> = g.data.form_plugins(f).into_iter().map(XVal::Str).collect();
            list(g, names)
        }
        _ => return None,
    };
    Some(Ok(v))
}
