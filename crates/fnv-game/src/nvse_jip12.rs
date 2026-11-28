//! JIP LN NVSE, batch 12 (#1006): the climate, wind, moons and lightning, the image
//! space modifiers on, the cells buffered and found by grid, the light at a point, fast
//! travel and the load order, the game settings as a map, purged references and internal
//! markers, the barter menu's trade, the item menus' filters and lists, the recipe menu,
//! AddRecipeCondition, the quantity and text input menus, and GetWheelDisabled
//! [documented: each page in the wiki export; docs/spec-notes/jip-world-12.md].
//!
//! The engine tells this batch what only it knows (the buffered cells, the lights, the
//! open trade, the item lists, the recipe menu) and takes back what the scripts asked for
//! (a climate, the wind, the moons' textures, a lightning flash, the price multipliers).

use crate::cond::{Cmp, Condition, Operand, RunOn};
use crate::game::{Game, RunCtx};
use fnv_script::vm::{ArrayKind, XKey, XVal};
use fnv_sim::FormId;
use std::collections::{BTreeMap, BTreeSet};

/// The wind's direction when no script set one, JIP's degrees (0 north, 90 east): where
/// the engine's fixed wind blows to, 330° clockwise from north (`nvforever::tree`, the
/// original's tumbleweeds [observed: o27_tumble 2026-11-06]) [hypothesis: the original's
/// GetWindDirection reads that direction].
pub const DEFAULT_WIND: f32 = -57.29578;

/// The moon phases' names in GetMoonTexture's order: full, three waning, half waning, one
/// waning, none, one waxing, half waxing, three waxing [documented: GetMoonTexture's
/// table], as the texture files name them (`fnv_world::sky::MOON_PHASES`).
const MOON_FILES: [&str; 8] = ["full", "three_wan", "half_wan", "one_wan", "new", "one_wax", "half_wax", "three_wax"];

/// The text input menu's character limit when none is given [documented:
/// SetTextInputExtendedProps, "defaults to 32,767"].
pub const TEXT_MAX: usize = 32767;

/// A light the engine sees, for GetLightAmountAtPoint: where (game units), its reach and
/// its strength (as the detection round counts it).
pub type PointLight = ([f32; 3], f32, f32);

/// What the engine measures light from (`nvforever::detection`): the base light outdoors
/// in the sun, outdoors in shade, indoors (the cell's ambient), 0 to 1 each, and the lights.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LightField {
    pub outdoors: Option<(f32, f32)>,
    pub interior: f32,
    pub lights: Vec<PointLight>,
}

/// The barter menu's trade, as the engine shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct BarterView {
    /// The caps the merchant has to trade with [documented: GetBarterGoldAlt, "the value
    /// displayed on the upper-right corner"].
    pub merchant_caps: i32,
    /// The merchant's container (whose items the player buys) and the player.
    pub stock: FormId,
    /// The items put up: bought from the merchant, sold by the player.
    pub bought: Vec<FormId>,
    pub sold: Vec<FormId>,
    /// The price multipliers shown: buying, selling.
    pub mult: [f32; 2],
}

/// An item menu's list as the engine shows it: each row's index in the menu's whole list,
/// the item, and whether the row is shown (the filter lets it through). No owner: an
/// empty list.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MenuList {
    pub owner: Option<FormId>,
    pub rows: Vec<(usize, FormId, bool)>,
}

/// An item menu's filters and lists, left (the player's) and right.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ItemMenuView {
    pub filter: [u8; 2],
    pub lists: [MenuList; 2],
}

/// ShowTextInputMenu's menu: what the engine draws and how the typing is held.
#[derive(Debug, Clone, PartialEq)]
pub struct TextInput {
    pub callback: XVal,
    pub title: String,
    pub size: (f32, f32),
    pub offset: (f32, f32),
    pub min: usize,
    pub max: usize,
    pub digits_only: bool,
    pub enter_closes: bool,
    /// Current live text of the input box (kept synchronized with `TextEditMenu`'s input
    /// tile while open; updated by `SetTextInputString` and engine typing).
    pub text: String,
    /// SetTextInputString's text, for the engine to take.
    pub set_text: Option<String>,
}

/// ShowQuantityMenu's menu.
#[derive(Debug, Clone, PartialEq)]
pub struct QuantityAsk {
    pub callback: XVal,
    pub max: i32,
    pub initial: i32,
}

/// What this batch keeps; all of it for the session [hypothesis: the pages say nothing
/// of saving, but the climate the page's ForceClimate locks is also lost to a load].
#[derive(Debug, Default)]
pub struct Jip12State {
    /// ForceClimate's climate.
    pub forced_climate: Option<FormId>,
    /// SetCurrentClimate's climate, and the worldspace it was set in.
    pub set_climate: Option<(FormId, FormId)>,
    /// SetWindDirection's angle.
    pub wind: Option<f32>,
    /// SetMoonTexture's paths, by phase.
    pub moons: BTreeMap<u8, String>,
    /// TriggerLightningFX asked for a flash.
    pub lightning: bool,
    /// The cells the engine keeps (exterior and interior).
    pub buffered: BTreeSet<FormId>,
    /// The engine has told the buffered cells.
    pub buffered_told: bool,
    pub light: Option<LightField>,
    /// Fast travels since the start, and each script's count at its last ask.
    pub fast_travels: u32,
    pub travels_seen: BTreeMap<(Option<FormId>, Option<FormId>), u32>,
    /// The loaded save's load order differs from this one.
    pub load_order_changed: bool,
    /// TogglePurgeOnUnload's references.
    pub purge: BTreeSet<FormId>,
    /// SetInternalMarker's forms.
    pub internal: BTreeMap<FormId, bool>,
    pub barter: Option<BarterView>,
    /// SetBarterPriceMult's multipliers, buying and selling, for the trade open.
    pub price_mults: [Option<f32>; 2],
    pub item_menus: BTreeMap<u16, ItemMenuView>,
    /// Each item menu's filters when it was last open, kept after it closes.
    pub item_filters: BTreeMap<u16, [u8; 2]>,
    /// The recipe menu: its category and the recipe highlighted.
    pub recipe_menu: Option<(Option<FormId>, Option<FormId>)>,
    /// AddRecipeCondition's conditions.
    pub recipe_conditions: BTreeMap<FormId, Vec<Condition>>,
    pub text_input: Option<TextInput>,
    pub quantity: Option<QuantityAsk>,
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

fn yes(b: bool) -> XVal {
    XVal::Num(f64::from(u8::from(b)))
}

fn reff(f: Option<FormId>) -> XVal {
    XVal::Ref(f.map_or(0, |f| f.0))
}

/// A list getter's array; none when there is nothing to give [observed: the jip10
/// probes for JIP's list getters; hypothesis: the same here].
fn list(g: &mut Game, items: Vec<XVal>) -> XVal {
    if items.is_empty() {
        return XVal::Array(0);
    }
    let entries = items.into_iter().enumerate().map(|(i, v)| (XKey::Num(i as f64), v)).collect();
    XVal::Array(g.ext.new_array(ArrayKind::List, entries))
}

/// A formatted-string argument: the script VM formats it with its values and passes the
/// text in its place (`fnv_script::vm`, `format_at`).
fn formatted(args: &[XVal], k: usize) -> String {
    text(args, k).unwrap_or_default()
}

/// The light at a point from the engine's field: the base light, plus each light within
/// its reach, `strength × (1 − (d / reach)²)`, as 0 to 100 (`nvforever::detection`'s
/// rule); a point outdoors counts as in the sun when the sun is up (see
/// docs/spec-notes/jip-world-12.md §2).
pub fn light_at(field: &LightField, at: [f32; 3]) -> f64 {
    let mut l = f64::from(match field.outdoors {
        Some((sun, _)) => sun,
        None => field.interior,
    });
    for &(p, reach, strength) in &field.lights {
        let d = (0..3).map(|k| (f64::from(p[k]) - f64::from(at[k])).powi(2)).sum::<f64>().sqrt();
        let reach = f64::from(reach);
        if d < reach {
            l += f64::from(strength) * (1.0 - (d / reach).powi(2));
        }
    }
    (l * 100.0).clamp(0.0, 100.0)
}

/// Whether a model path is a marker's, which the engine does not draw: a file whose name
/// begins with "marker", or one under a `markers` folder (`nvforever::world::is_marker`).
pub fn marker_model(path: &str) -> bool {
    let p = path.to_ascii_lowercase().replace('/', "\\");
    let file = p.rsplit('\\').next().unwrap_or("");
    file.starts_with("marker") || p.starts_with("markers\\") || p.contains("\\markers\\")
}

impl Game<'_> {
    fn worldspace_own_ref_override(&self, world: FormId, key: &str, expect_sig: &[u8; 4]) -> Option<FormId> {
        let orig = self.unclone(world);
        let v = self.field_overrides.get(&(world, key.to_string())).or_else(|| self.field_overrides.get(&(orig, key.to_string())))?;
        let FormId(id) = match v {
            XVal::Ref(r) if *r != 0 => FormId(*r),
            _ => return None,
        };
        (self.data.sig(self.unclone(FormId(id))) == Some(*expect_sig)).then_some(FormId(id))
    }

    fn worldspace_own_ref_record(&self, world: FormId, sub: &str) -> Option<FormId> {
        let s = self.data.record_field(self.unclone(world), sub, "")?.as_str()?;
        let id = u32::from_str_radix(s.trim_start_matches("0x"), 16).ok()?;
        (id != 0).then_some(FormId(id))
    }

    fn worldspace_parent_info(&self, world: FormId) -> (Option<FormId>, u16) {
        let parent = match crate::nvse::record_value(self, world, "WNAM", "") {
            Some(XVal::Ref(r)) if r != 0 && self.data.sig(self.unclone(FormId(r))) == Some(*b"WRLD") => Some(FormId(r)),
            _ => None,
        };
        let pnam = match crate::nvse::record_value(self, world, "PNAM", "") {
            Some(XVal::Num(n)) if n.is_finite() => (n.trunc() as i64 as u64 & 0xFFFF) as u16,
            _ => 0,
        };
        (parent, pnam)
    }

    #[allow(clippy::too_many_arguments)]
    fn resolve_worldspace_ref(
        &self,
        world: FormId,
        key: &str,
        sub: &str,
        expect_sig: &[u8; 4],
        parent_flag: u16,
        override_only: bool,
        depth: u8,
    ) -> Option<FormId> {
        if depth >= 4 {
            return None;
        }
        let orig = self.unclone(world);
        if self.data.sig(orig) != Some(*b"WRLD") {
            return None;
        }
        if let Some(ov) = self.worldspace_own_ref_override(world, key, expect_sig) {
            return Some(ov);
        }
        let (parent, pnam) = self.worldspace_parent_info(world);
        let parent = parent.filter(|&p| p != world && p != orig);
        let own_rec = self.worldspace_own_ref_record(world, sub);
        if pnam & parent_flag != 0 {
            let from_parent = parent.and_then(|p| self.resolve_worldspace_ref(p, key, sub, expect_sig, parent_flag, override_only, depth + 1));
            if override_only { from_parent } else { from_parent.or(own_rec) }
        } else if own_rec.is_some() {
            if override_only { None } else { own_rec }
        } else {
            parent.and_then(|p| self.resolve_worldspace_ref(p, key, sub, expect_sig, parent_flag, override_only, depth + 1))
        }
    }

    /// Session `SetWorldspaceClimate` (`WRLD CNAM/`) override applying to `world`
    /// (clone-first, original fallback, following `WNAM` + `PNAM` Use Climate Data
    /// or missing child `CNAM`). Returns `None` when no session override applies [design].
    pub fn worldspace_climate_override(&self, world: FormId) -> Option<FormId> {
        self.resolve_worldspace_ref(world, "CNAM/", "CNAM", b"CLMT", fnv_world::index::parent_use::CLIMATE, true, 0)
    }

    /// Effective climate (`CLMT`) for `world`: session `SetWorldspaceClimate` override
    /// first, else authored `WRLD CNAM` (following `WNAM` + `PNAM` Use Climate Data
    /// inheritance) [documented: xEdit WRLD; design].
    pub fn worldspace_climate(&self, world: FormId) -> Option<FormId> {
        self.resolve_worldspace_ref(world, "CNAM/", "CNAM", b"CLMT", fnv_world::index::parent_use::CLIMATE, false, 0)
    }

    /// Session `SetWorldspaceImagespace` (`WRLD INAM/`) override applying to `world`
    /// (clone-first, original fallback, following `WNAM` + `PNAM` Use Image Space Data
    /// or missing child `INAM`). Returns `None` when no session override applies [design].
    pub fn worldspace_image_space_override(&self, world: FormId) -> Option<FormId> {
        self.resolve_worldspace_ref(world, "INAM/", "INAM", b"IMGS", fnv_world::index::parent_use::IMAGE_SPACE, true, 0)
    }

    /// Effective image space (`IMGS`) for `world`: session `SetWorldspaceImagespace`
    /// override first, else authored `WRLD INAM` (following `WNAM` + `PNAM` Use Image
    /// Space Data inheritance) [documented: xEdit WRLD; design].
    pub fn worldspace_image_space(&self, world: FormId) -> Option<FormId> {
        self.resolve_worldspace_ref(world, "INAM/", "INAM", b"IMGS", fnv_world::index::parent_use::IMAGE_SPACE, false, 0)
    }

    /// Session `SetWorldspaceImagespace` override for the player's current exterior
    /// worldspace, `None` indoors or when unedited [design].
    pub fn current_worldspace_image_space_override(&self) -> Option<FormId> {
        self.player_world().and_then(|w| self.worldspace_image_space_override(w))
    }

    /// Effective image space (`IMGS`) of the player's current exterior worldspace,
    /// `None` indoors [design].
    pub fn current_worldspace_image_space(&self) -> Option<FormId> {
        self.player_world().and_then(|w| self.worldspace_image_space(w))
    }

    /// Effective image space (`IMGS`) of a `CELL`: `SetCellImageSpace` (`FormChanges`)
    /// first, else authored `CELL XCIM` [documented: SetCellImageSpace, GetCellImageSpace].
    pub fn cell_image_space(&self, cell: FormId) -> Option<FormId> {
        let orig = self.unclone(cell);
        if self.data.sig(orig) != Some(*b"CELL") {
            return None;
        }
        if let Some(&space) = self.forms.cell_image_spaces.get(&cell).or_else(|| self.forms.cell_image_spaces.get(&orig)) {
            return Some(space);
        }
        self.worldspace_own_ref_record(cell, "XCIM")
    }

    /// Effective image space (`IMGS`) where the player currently is: the current cell's
    /// image space (`SetCellImageSpace` / `CELL XCIM`) first, else the exterior
    /// worldspace's (`SetWorldspaceImagespace` / `WRLD INAM`) [design].
    pub fn current_image_space(&self) -> Option<FormId> {
        let cell = self.cell_of(FormId::PLAYER_REF)?;
        self.cell_image_space(cell).or_else(|| self.world.placed.world_of(cell).and_then(|w| self.worldspace_image_space(w)))
    }

    /// The climate the scripts want drawn: ForceClimate's, else SetCurrentClimate's while
    /// the player is in the worldspace it was set in, else SetWorldspaceClimate's override
    /// for the player's current worldspace; `None` leaves the worldspace's authored record.
    pub fn script_climate(&self) -> Option<FormId> {
        let j = &self.other.jip12;
        j.forced_climate
            .or_else(|| {
                let (c, w) = j.set_climate?;
                (self.player_world() == Some(w)).then_some(c)
            })
            .or_else(|| self.player_world().and_then(|w| self.worldspace_climate_override(w)))
    }

    /// The climate the player's worldspace shows now: `script_climate()`, else the
    /// worldspace's authored climate (with parent inheritance); `None` indoors.
    pub fn current_climate(&self) -> Option<FormId> {
        let w = self.player_world()?;
        self.script_climate().or_else(|| self.worldspace_climate(w))
    }

    /// The weather table (`WLST`, with `ClimateAddWeatherType` / `ClimateRemoveWeatherType`
    /// edits) of the player's current climate, empty indoors or without a climate [design].
    pub fn current_climate_weathers(&self) -> Vec<crate::nvse_jip7::ClimateWeather> {
        self.current_climate().map(|c| self.climate_weathers(c)).unwrap_or_default()
    }

    /// The default weather of the player's current climate: the eligible `WLST` entry
    /// (no global, or its global is non-zero) with the highest chance, first entry on ties
    /// (matching `fnv_world::sky::default_weather`) [design].
    pub fn current_climate_default_weather(&self) -> Option<FormId> {
        let mut best: Option<(i64, FormId)> = None;
        for entry in self.current_climate_weathers() {
            if let Some(glob) = entry.global {
                if !self.world.global(glob).is_ok_and(|v| v != 0.0) {
                    continue;
                }
            }
            if best.is_none_or(|(c, _)| entry.chance > c) {
                best = Some((entry.chance, entry.weather));
            }
        }
        best.map(|(_, w)| w)
    }

    fn player_world(&self) -> Option<FormId> {
        self.cell_of(FormId::PLAYER_REF).and_then(|c| self.world.placed.world_of(c))
    }

    /// SetWindDirection's angle as the engine draws it: degrees from the game's +x axis
    /// toward +y (JIP's 0 is north, +y; 90 east, +x); `None` leaves the engine's own.
    pub fn script_wind(&self) -> Option<f32> {
        self.other.jip12.wind.map(|j| (90.0 - j).rem_euclid(360.0))
    }

    /// SetMoonTexture's paths, phase by phase (`None`: the phase's own), under `textures\`
    /// as the engine loads them.
    pub fn moon_textures(&self) -> [Option<String>; 8] {
        std::array::from_fn(|k| {
            self.other.jip12.moons.get(&(k as u8)).map(|p| {
                let p = p.replace('/', "\\").trim_start_matches('\\').to_string();
                let low = p.to_ascii_lowercase();
                match low.strip_prefix("textures\\") {
                    Some(_) => p[9..].to_string(),
                    None => p,
                }
            })
        })
    }

    /// TriggerLightningFX's flash, taken by the engine.
    pub fn take_lightning(&mut self) -> bool {
        std::mem::take(&mut self.other.jip12.lightning)
    }

    /// The engine's buffered cells, each frame they change.
    pub fn set_buffered_cells(&mut self, cells: BTreeSet<FormId>) {
        self.other.jip12.buffered = cells;
        self.other.jip12.buffered_told = true;
    }

    /// The cells kept in memory: the engine's buffer and the loaded cells.
    pub fn buffered_cells(&self) -> BTreeSet<FormId> {
        let mut out = self.other.jip12.buffered.clone();
        out.extend(self.loaded_cells.iter().copied());
        out
    }

    /// The engine's light sources, at each detection round.
    pub fn set_light_field(&mut self, field: LightField) {
        self.other.jip12.light = Some(field);
    }

    /// Whether `marker` is a placed map marker that the player may fast-travel to:
    /// discovered (`set_found_markers`), or shown and enabled for travel by `ShowMap` /
    /// `ShowAllMapMarkers` (respecting `skip_hidden` via `map_marker_shown` and
    /// `MARKER_HIDDEN`, without treating authored `FNAM` bit 2 `Can Travel To` on
    /// undiscovered markers as discovered) [documented: GetMapMarkerVisible, ShowMap,
    /// ShowAllMapMarkers].
    fn map_marker_can_travel(&self, marker: FormId) -> bool {
        let Some(flags) = self.marker_flags(marker) else { return false };
        let p = &self.player;
        if p.found_markers.contains(&marker) {
            return true;
        }
        let hidden = flags & crate::nvse_lu2::MARKER_HIDDEN != 0;
        let shown = self.map_marker_shown(marker, hidden);
        shown && (p.travel_markers.contains(&marker) || p.all_markers.is_some_and(|a| a.shown && a.travel))
    }

    /// The arrival reference for a fast travel to `marker`: its linked reference
    /// (`XLKR` / `SetLinkedReference`) when present, else `marker` itself
    /// [documented: GECK wiki SetOnFastTravelEventHandler; hypothesis: marker itself
    /// when unlinked, docs/spec-notes/fast-travel.md §4].
    fn fast_travel_destination(&self, marker: FormId) -> Option<&fnv_sim::refs::PlacedRef> {
        let placed = self.world.placed.get(marker)?;
        self.linked_ref(marker).and_then(|l| self.world.placed.get(l)).or(Some(placed))
    }

    /// Find the fast-travel destination marker matching the player's current arrival
    /// position (set by `nvf_play::pipboy::data::travel::depart` immediately before
    /// calling [`Game::note_fast_travel`]).
    fn fast_travel_marker_at_player(&self) -> Option<FormId> {
        let p = self.world.peek(FormId::PLAYER_REF)?;
        let player_world = p.cell.and_then(|c| self.world.placed.world_of(c));
        let mut best: Option<(f32, f32, u32)> = None;
        for m in self.world.placed.refs.values().filter(|r| r.map_marker.is_some()) {
            let dest = self.fast_travel_destination(m.id).unwrap_or(m);
            let dest_world = dest.cell.and_then(|c| self.world.placed.world_of(c));
            let same_space = if player_world.is_some() || dest_world.is_some() {
                dest_world == player_world
            } else {
                dest.cell == p.cell
            };
            if !same_space {
                continue;
            }
            let d2 = (0..3).map(|i| (dest.pos[i] - p.pos[i]).powi(2)).sum::<f32>();
            if d2 > 1.0 {
                continue;
            }
            let yaw = (dest.rot[2] - p.rot[2]).abs();
            let cand = (d2, yaw, dest.id.0);
            if best.is_none_or(|b| cand.0 < b.0 || (cand.0 == b.0 && (cand.1 < b.1 || (cand.1 == b.1 && cand.2 < b.2)))) {
                best = Some(cand);
            }
        }
        best.map(|(_, _, id)| FormId(id))
    }

    /// Record a completed player fast travel to `marker` (the destination marker
    /// reference, which may be a map marker's linked arrival reference rather than
    /// the MapMarker-based reference itself) [documented: SetOnFastTravelEventHandler,
    /// GetPCFastTravelled]: increments `GetPCFastTravelled`'s counter and fires
    /// `SetOnFastTravelEventHandler` (`fasttravel`) with `marker`, returning how
    /// many handlers ran.
    pub fn note_fast_travel_to(&mut self, marker: FormId) -> usize {
        self.other.jip12.fast_travels += 1;
        self.jip_event("fasttravel", None, vec![XVal::Ref(marker.0)])
    }

    /// The player fast travelled (the Pip-Boy's map): counts the travel for
    /// `GetPCFastTravelled` and fires JIP's `SetOnFastTravelEventHandler`
    /// (`fasttravel`) with the arrival marker reference at the player's destination
    /// (or form 0 when none matches) [documented: SetOnFastTravelEventHandler,
    /// GetPCFastTravelled].
    pub fn note_fast_travel(&mut self) {
        let marker = self.fast_travel_marker_at_player().unwrap_or(FormId(0));
        self.note_fast_travel_to(marker);
    }

    /// Attempt to fast-travel the player to `marker`: checks `can_fast_travel`
    /// (`EnableFastTravel 0`, cell/world flags, air, combat, hostiles near,
    /// encumbrance without Long Haul) and verifies `marker` is a placed, travelable
    /// map marker. Refused travel changes nothing and fires no event; on success,
    /// moves the player to the marker's linked arrival reference (if any) or the
    /// marker itself, updates `player_entered` / `bring_teammates` /
    /// `GetPCFastTravelled`, and fires `SetOnFastTravelEventHandler` (`fasttravel`)
    /// with the destination marker reference [documented: SetOnFastTravelEventHandler,
    /// EnableFastTravel, GetPCCanFastTravel].
    pub fn fast_travel(&mut self, marker: FormId) -> bool {
        if !self.can_fast_travel(&RunCtx::default()) || !self.map_marker_can_travel(marker) {
            return false;
        }
        let Some((dest_id, dest_cell, dest_pos, dest_rot)) = self
            .fast_travel_destination(marker)
            .map(|d| (d.id, d.cell, d.pos, d.rot))
        else {
            return false;
        };
        let cell = dest_cell.map(|c| self.world.placed.exterior_cell_at(c, dest_pos).unwrap_or(c));
        let from = self.world.peek(FormId::PLAYER_REF).map(|p| (p.cell, p.pos));
        if let Ok(p) = self.world.get(FormId::PLAYER_REF) {
            (p.cell, p.pos, p.rot) = (cell, dest_pos, [0.0, 0.0, dest_rot[2]]);
        }
        self.player_entered(cell);
        self.note_fast_travel_to(dest_id);
        self.bring_teammates(from);
        true
    }

    /// The engine loaded a save made with another load order.
    pub fn set_load_order_changed(&mut self, changed: bool) {
        self.other.jip12.load_order_changed = changed;
    }

    /// The references TogglePurgeOnUnload marked in cells that leave the loaded area are
    /// deleted [documented: "deleted once its 3D is unloaded"].
    pub(crate) fn purge_left(&mut self, left: &[FormId]) {
        if self.other.jip12.purge.is_empty() {
            return;
        }
        let gone: Vec<FormId> = self.other.jip12.purge.iter().copied().filter(|&r| self.cell_of(r).is_some_and(|c| left.contains(&c))).collect();
        for r in gone {
            self.other.jip12.purge.remove(&r);
            let _ = self.world.set_deleted(r, true);
        }
    }

    /// Whether a base form is not drawn: SetInternalMarker's, else a marker's model; a
    /// form whose record fields the game does not keep (statics) by its Editor ID, one
    /// naming a marker (see docs/spec-notes/jip-world-12.md §2).
    pub fn internal_marker(&self, base: FormId) -> bool {
        if let Some(&b) = self.other.jip12.internal.get(&base) {
            return b;
        }
        match crate::nif_blocks::model_of(self, base) {
            Some(m) => marker_model(&m),
            None => self.data.edid_of.get(&base).is_some_and(|e| e.to_ascii_lowercase().contains("marker")),
        }
    }

    /// The forms SetInternalMarker changed, for the engine's drawing.
    pub fn internal_marker_changes(&self) -> &BTreeMap<FormId, bool> {
        &self.other.jip12.internal
    }

    /// The barter menu's trade, each frame it is open (`None` when it closes; the
    /// multipliers set go with it).
    pub fn set_barter_view(&mut self, v: Option<BarterView>) {
        if v.is_none() {
            self.other.jip12.price_mults = [None; 2];
        }
        self.other.jip12.barter = v;
    }

    /// SetBarterPriceMult's multipliers, buying and selling.
    pub fn barter_price_mults(&self) -> [Option<f32>; 2] {
        self.other.jip12.price_mults
    }

    /// An item menu's filters and lists, each frame it is open (`None`: closed).
    pub fn set_item_menu_view(&mut self, code: u16, v: Option<ItemMenuView>) {
        match v {
            Some(v) => {
                self.other.jip12.item_filters.insert(code, v.filter);
                self.other.jip12.item_menus.insert(code, v)
            }
            None => self.other.jip12.item_menus.remove(&code),
        };
    }

    /// The recipe menu's category and highlighted recipe (`None`: closed).
    pub fn set_recipe_menu(&mut self, v: Option<(Option<FormId>, Option<FormId>)>) {
        self.other.jip12.recipe_menu = v;
    }

    /// Whether AddRecipeCondition's conditions of `recipe` pass for the player.
    pub fn recipe_conditions_pass(&mut self, recipe: FormId) -> bool {
        let Some(c) = self.other.jip12.recipe_conditions.get(&recipe).cloned() else { return true };
        self.conditions_pass(&c, Some(FormId::PLAYER_REF), None, "recipe")
    }

    /// ShowTextInputMenu's menu while open.
    pub fn text_input(&self) -> Option<&TextInput> {
        self.other.jip12.text_input.as_ref()
    }

    /// SetTextInputString's text, taken by the engine.
    pub fn take_text_input_string(&mut self) -> Option<String> {
        self.other.jip12.text_input.as_mut()?.set_text.take()
    }

    /// Update the open `ShowTextInputMenu`'s live typed text from the engine UI and
    /// mirror it into `TextEditMenu`'s input text tile without overwriting script edits
    /// to other `TextEditMenu` traits.
    pub fn set_text_input_live_text(&mut self, typed: &str) {
        let Some(t) = self.other.jip12.text_input.as_mut() else { return };
        let clipped: String = typed.chars().take(t.max).collect();
        t.text = clipped.clone();
        crate::ui_text_input::sync_live_text(self, &clipped);
    }

    /// Ok in the text input menu: the callback gets the text (next frame), the menu closes.
    pub fn finish_text_input(&mut self, typed: String) {
        let Some(t) = self.other.jip12.text_input.take() else { return };
        if self.menu == Some(crate::game::menus::NAME) {
            self.close_menu();
        }
        crate::delayed::call_next_frame(self, t.callback, None, vec![XVal::Str(typed)]);
    }

    /// ShowQuantityMenu's menu while open.
    pub fn quantity_ask(&self) -> Option<&QuantityAsk> {
        self.other.jip12.quantity.as_ref()
    }

    /// The quantity menu closed: the callback gets the amount, -1 for Cancel
    /// [documented: "If Cancel was selected, -1 is passed"].
    pub fn finish_quantity(&mut self, amount: Option<i32>) {
        let Some(q) = self.other.jip12.quantity.take() else { return };
        if self.menu == Some(crate::game::menus::QUANTITY) {
            self.close_menu();
        }
        crate::delayed::call_next_frame(self, q.callback, None, vec![XVal::Num(f64::from(amount.unwrap_or(-1)))]);
    }

    /// Whether SetWheelDisabled switched the companion wheel off for an actor.
    pub fn wheel_disabled(&self, actor: FormId) -> bool {
        self.jip_flags.contains(&(actor, "no_wheel"))
    }
}

/// The climate the player's worldspace shows now: the scripts', else the record's.
pub fn current_climate(g: &Game) -> Option<FormId> {
    g.current_climate()
}

pub(crate) fn call(g: &mut Game, ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let zero = XVal::Num(0.0);
    let v = match name {
        // ---- the sky ----
        // A climate locked until ForceClimate without one or SetCurrentClimate
        // [documented]; NVForever's sky changes to it (`nvforever::worldspace`).
        "forceclimate" => {
            g.other.jip12.forced_climate = form(args, 0).filter(|&c| g.data.sig(g.unclone(c)) == Some(*b"CLMT"));
            if g.other.jip12.forced_climate.is_some() {
                g.other.jip12.set_climate = None;
            }
            zero
        }
        // The climate for now, unlocked; does not return the climate set (`retSet != rCl2`,
        // `setRet=0`) [observed: `w6d2_18_force_set_current_climate_and_isnight`]; returns 0
        // [hypothesis]. It holds while the player stays in the worldspace [decision: the
        // page's "Bugs" note, the climate going back after about 7 seconds, is not followed].
        "setcurrentclimate" => {
            let c = form(args, 0).filter(|&c| g.data.sig(g.unclone(c)) == Some(*b"CLMT"));
            g.other.jip12.forced_climate = None;
            g.other.jip12.set_climate = c.zip(g.player_world());
            zero
        }
        // Degrees, 0 north, 90 east, -90 west, ±180 south [documented]; -1 radian in
        // degrees when no script set one [observed: jip12 wind]; the set angle read back
        // as given [observed: jip12 setwind, 400].
        "getwinddirection" => XVal::Num(f64::from(g.other.jip12.wind.unwrap_or(DEFAULT_WIND))),
        "setwinddirection" => {
            if let Some(a) = num(args, 0) {
                g.other.jip12.wind = Some(a as f32);
            }
            zero
        }
        // The phase's texture [documented]; for a phase no script set, the file the
        // engine draws written `\Textures\Sky\Masser_<phase>.dds`, and "" for the new
        // moon [observed: jip12 moon.0, moon.3, moon.4].
        "getmoontexture" => {
            let Some(k) = num(args, 0).filter(|k| (0.0..8.0).contains(k)).map(|k| k as u8) else { return Some(Ok(XVal::Str(String::new()))) };
            XVal::Str(g.other.jip12.moons.get(&k).cloned().unwrap_or_else(|| match k {
                4 => String::new(),
                _ => format!("\\Textures\\Sky\\Masser_{}.dds", MOON_FILES[k as usize]),
            }))
        }
        "setmoontexture" => {
            if let (Some(k), Some(p)) = (num(args, 0).filter(|k| (0.0..8.0).contains(k)), text(args, 1)) {
                g.other.jip12.moons.insert(k as u8, p);
            }
            zero
        }
        // Outdoors in the rain only [documented]; the engine draws the flash and the
        // thunder (`nvforever::weather_fx`).
        "triggerlightningfx" => {
            let outdoors = g.player_world().is_some();
            if outdoors && g.weather.raining {
                g.other.jip12.lightning = true;
            }
            zero
        }
        // ---- the world ----
        // The active modifiers on the screen in application order without deduplication
        // [documented; observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN
        // 57.30, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base
        // masters, `w4d_24_active_imods_order` (`tools/capture/nvse/catalogs/wave4d_extenders.json`,
        // #3679): `n=3,0=00054E13,1=001736A8,2=00054E13`; no DLL or source consulted;
        // hypothesis: next-tick retention of duplicates].
        "getactiveimods" => {
            let mods: Vec<XVal> = g.image_modifiers.iter().map(|m| XVal::Ref(m.modifier.0)).collect();
            list(g, mods)
        }
        // Interiors (1) or exteriors (0) [documented].
        // An array even when empty [observed: jip12 buffered.interiors, size 0].
        "getbufferedcells" => {
            let want_interiors = num(args, 0).unwrap_or(0.0) != 0.0;
            let cells = g.buffered_cells().into_iter().filter(|c| g.world.placed.world_of(*c).is_none() == want_interiors);
            let entries = cells.enumerate().map(|(i, c)| (XKey::Num(i as f64), XVal::Ref(c.0))).collect();
            XVal::Array(g.ext.new_array(ArrayKind::List, entries))
        }
        "getcellbuffered" => {
            let c = form(args, 0).or_else(|| subject.and_then(|r| g.cell_of(r)));
            yes(c.is_some_and(|c| g.buffered_cells().contains(&c)))
        }
        // The north marker's rotation: a NorthMarker reference in the cell, its Z angle
        // in degrees; 0 without one [documented] (see docs/spec-notes/jip-world-12.md §2).
        "getcellnorthrotation" => {
            let Some(c) = form(args, 0) else { return Some(Ok(zero)) };
            let refs = g.world.placed.by_cell.get(&c).cloned().unwrap_or_default();
            let north = refs.into_iter().find(|&r| crate::functions::base_of(g, r).and_then(|b| g.data.edid_of.get(&b)).is_some_and(|e| e.eq_ignore_ascii_case("NorthMarker")));
            match north.and_then(|r| g.world.get(r).ok().map(|s| s.rot[2])) {
                Some(z) => XVal::Num(f64::from(z).to_degrees()),
                None => zero,
            }
        }
        // Only a cell in memory: 0 for one far from the player [observed: jip12 extcell,
        // WastelandNV -18 0 from the probes' start cell 0x000DEB16; hypothesis: "in memory" is the
        // buffered and loaded cells].
        "getexteriorcell" => {
            let (Some(w), Some(x), Some(y)) = (form(args, 0), num(args, 1), num(args, 2)) else { return Some(Ok(XVal::Ref(0))) };
            let c = g.world.placed.world_grid.get(&w).and_then(|grid| grid.get(&(x as i32, y as i32))).copied();
            reff(c.filter(|c| g.buffered_cells().contains(c)))
        }
        "getworldspacepersistentcell" => reff(form(args, 0).and_then(|w| g.world.placed.world_persistent.get(&w).copied())),
        // 0 dark to 100 lit [documented], from the lights the engine last measured with;
        // 0 before it has (see docs/spec-notes/jip-world-12.md §2).
        "getlightamountatpoint" => {
            let (Some(x), Some(y), Some(z)) = (num(args, 0), num(args, 1), num(args, 2)) else { return Some(Ok(zero)) };
            match &g.other.jip12.light {
                Some(f) => XVal::Num(light_at(f, [x as f32, y as f32, z as f32])),
                None => zero,
            }
        }
        // Since the calling script last asked [documented]; a script's first ask counts
        // the travels since the game started (see docs/spec-notes/jip-world-12.md §2).
        "getpcfasttravelled" => {
            let key = (ctx.self_ref, ctx.quest);
            let now = g.other.jip12.fast_travels;
            let seen = g.other.jip12.travels_seen.insert(key, now).unwrap_or(0);
            yes(now > seen)
        }
        "getloadorderchanged" => yes(g.other.jip12.load_order_changed),
        // The settings by name: 1 the numbers (f, i), 2 the texts (s), 3 both by
        // default [documented]; their values now. [decision: the names as the records
        // spell them; a setting's value as SetNumericGameSetting left it.]
        "getgamesettings" => {
            let mask = num(args, 0).map_or(3, |m| m as u32);
            let mut names: Vec<String> = g.world.forms.settings.keys().cloned().collect();
            names.sort();
            let mut entries = Vec::new();
            for low in names {
                let shown = g.world.forms.setting_names.get(&low).cloned().unwrap_or_else(|| low.clone());
                let v = match g.world.setting_value(&low) {
                    Some(fnv_sim::settings::Setting::Float(f)) if mask & 1 != 0 => XVal::Num(f64::from(*f)),
                    Some(fnv_sim::settings::Setting::Int(i)) if mask & 1 != 0 => XVal::Num(f64::from(*i)),
                    Some(fnv_sim::settings::Setting::Str(s)) if mask & 2 != 0 => XVal::Str(s.clone()),
                    _ => continue,
                };
                entries.push((XKey::Str(shown), v));
            }
            if entries.is_empty() {
                XVal::Array(0)
            } else {
                XVal::Array(g.ext.new_array(ArrayKind::StringMap, entries))
            }
        }
        // [decision: the page is empty (marked Incomplete); without its hash, 0. Follow-up
        // in the spec note.] It takes a string: `GetStringHash "NVForever"` compiles in the
        // original and gives -439019037 [observed: jip12 hash], no common 32-bit string
        // hash we tried (FNV-1/1a, djb2, sdbm, ×31, ×131, CRC-32, Adler-32) on the text as written, lower or upper case.
        "getstringhash" => zero,
        // Created references (FF) only [documented]; returns the previous toggle state
        // before applying any supplied argument [observed: `w6d2_23_world_state_internal_marker_and_light`].
        "togglepurgeonunload" => {
            let Some(r) = subject.filter(|r| r.is_dynamic()) else { return Some(Ok(zero)) };
            let was = g.other.jip12.purge.contains(&r);
            if let Some(t) = num(args, 0) {
                if t != 0.0 {
                    g.other.jip12.purge.insert(r);
                } else {
                    g.other.jip12.purge.remove(&r);
                }
            }
            yes(was)
        }
        // The form's marker state: 1-arg returns current state, 2-arg updates the state
        // and returns 0 [documented; observed: `w6d2p4_53_micro_precision_bounds`]; the
        // engine draws the change for references placed from then on (`nvforever::world`).
        "setinternalmarker" => {
            let Some(f) = form(args, 0) else { return Some(Ok(zero)) };
            if let Some(t) = num(args, 1) {
                g.other.jip12.internal.insert(f, t != 0.0);
                zero
            } else {
                yes(g.internal_marker(f))
            }
        }
        // ---- the barter menu ----
        "getbartergoldalt" => XVal::Num(g.other.jip12.barter.as_ref().map_or(0.0, |b| f64::from(b.merchant_caps))),
        // Inventory references to the items put up: bought (0) or sold (1); none when
        // there are none [documented: "Returns 0 if there are no items"].
        "getbarteritems" => {
            let Some(b) = g.other.jip12.barter.clone() else { return Some(Ok(XVal::Array(0))) };
            let sold = num(args, 0).unwrap_or(0.0) != 0.0;
            let (owner, items) = if sold { (FormId::PLAYER_REF, b.sold) } else { (b.stock, b.bought) };
            let refs: Vec<XVal> = items.into_iter().map(|i| XVal::Ref(g.temp_ref(owner, i).0)).collect();
            list(g, refs)
        }
        // Bought (0) or sold (1); 0 without a trade (see docs/spec-notes/jip-world-12.md §3).
        "getbarterpricemult" => {
            let k = usize::from(num(args, 0).unwrap_or(0.0) != 0.0);
            match &g.other.jip12.barter {
                Some(b) => XVal::Num(f64::from(g.other.jip12.price_mults[k].unwrap_or(b.mult[k]))),
                None => zero,
            }
        }
        // Bought at least 1, sold at most 1 [documented]; for the trade open
        // (see docs/spec-notes/jip-world-12.md §3).
        "setbarterpricemult" => {
            if let (Some(b), Some(m)) = (num(args, 0), num(args, 1)) {
                if g.other.jip12.barter.is_some() {
                    let k = usize::from(b != 0.0);
                    let m = m as f32;
                    g.other.jip12.price_mults[k] = Some(if k == 0 { m.max(1.0) } else { m.clamp(0.0, 1.0) });
                }
            }
            zero
        }
        // ---- the item menus ----
        // 0 none (all), 1 weapons, 2 apparel, 3 aid, 4 misc, 5 ammo [documented]; the
        // inventory, container and barter menus only.
        "getmenuitemfilter" => {
            let code = num(args, 0).unwrap_or(0.0) as u16;
            let side = usize::from(num(args, 1).unwrap_or(0.0) != 0.0);
            // The menu's filter, kept after it closes (see docs/spec-notes/jip-world-12.md §4); before it was ever
            // open, 1 (weapons) for the inventory (1002) and -1 for container (1008) /
            // barter (1053) / other menus [observed: jip12 filter.inventory; Fallout:
            // New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, `jip_40_barter_and_filter_defaults`:
            // 1002 -> 1, 1008 -> -1, 1053 -> -1; `w4d_19_menu_item_filter_persist`:
            // `f0=1|open1(mm=1002,f=1)`].
            let first: i16 = if code == 1002 { 1 } else { -1 };
            let f = g.other.jip12.item_menus.get(&code).map(|m| m.filter).or_else(|| g.other.jip12.item_filters.get(&code).copied());
            XVal::Num(f.map_or(f64::from(first), |f| f64::from(f[side])))
        }
        // The open menu's rows as inventory references, keyed by their index in the
        // unfiltered list [documented]; the shown ones, or all (1); the right panel (1).
        "getmenuitemlistrefs" => {
            let all = num(args, 0).unwrap_or(0.0) != 0.0;
            let side = usize::from(num(args, 1).unwrap_or(0.0) != 0.0);
            let Some(code) = g.menu_top_item_menu() else { return Some(Ok(XVal::Array(0))) };
            let Some(m) = g.other.jip12.item_menus.get(&code).cloned() else { return Some(Ok(XVal::Array(0))) };
            let l = &m.lists[side];
            let mut entries = Vec::new();
            let Some(owner) = l.owner else { return Some(Ok(XVal::Array(0))) };
            for &(i, item, shown) in &l.rows {
                if all || shown {
                    entries.push((XKey::Num(i as f64), XVal::Ref(g.temp_ref(owner, item).0)));
                }
            }
            if entries.is_empty() {
                XVal::Array(0)
            } else {
                XVal::Array(g.ext.new_array(ArrayKind::Map, entries))
            }
        }
        // ---- the recipe menu ----
        "getrecipemenucategory" => reff(g.other.jip12.recipe_menu.and_then(|m| m.0)),
        "getrecipemenuselection" => reff(g.other.jip12.recipe_menu.and_then(|m| m.1)),
        // A condition on the recipe for the session: function `func` (its index, an
        // opcode less 0x1000), parameter `param`, comparison `comp` against `value`
        // (`comp` read as the CTDA operator code, 0 == 1 != 2 > 3 >= 4 < 5 <=; see
        // docs/spec-notes/jip-world-12.md §4), run on the player and joined
        // with AND to the record's.
        "addrecipecondition" => {
            let (Some(r), Some(f), Some(c), Some(v)) = (form(args, 0), num(args, 1), num(args, 3), num(args, 4)) else { return Some(Ok(zero)) };
            if g.data.sigs.get(&r) != Some(b"RCPE") {
                return Some(Ok(zero));
            }
            let f = f as u32;
            let function = if f >= 0x1000 { f - 0x1000 } else { f } as u16;
            let cmp = match c as i32 {
                0 => Cmp::Eq,
                1 => Cmp::Ne,
                2 => Cmp::Gt,
                3 => Cmp::Ge,
                4 => Cmp::Lt,
                5 => Cmp::Le,
                _ => return Some(Ok(zero)),
            };
            // The parameter typed by the function's first parameter type, as a CTDA's
            // (`crate::cond::param`): a number stays a number, a form a form.
            let raw = match args.get(2) {
                Some(XVal::Ref(r)) => Some(*r),
                Some(XVal::Num(x)) if x.fract() == 0.0 => Some(*x as i32 as u32),
                _ => None,
            };
            let ty = g.data.param_types.0.get(&function).and_then(|t| t.first()).map(String::as_str);
            let params = raw.map(|raw| vec![crate::cond::param(ty, raw, &|r| Some(r))]).unwrap_or_default();
            g.other.jip12.recipe_conditions.entry(r).or_default().push(Condition { cmp, or: false, run_on: RunOn::Subject, function, params, rhs: Operand::Value(v) });
            zero
        }
        // ---- the quantity and text input menus ----
        // The callback gets the amount, -1 for Cancel [documented]; drawn as the item
        // menus' "How many?" prompt (`nvf_play::script_quantity`). Not while another
        // menu is open, its own included, as ShowTextInputMenu (see docs/spec-notes/jip-world-12.md §5; #2018).
        "showquantitymenu" => {
            let (Some(cb), Some(max)) = (args.first().cloned().filter(|v| !matches!(v, XVal::Num(_))), num(args, 1)) else { return Some(Ok(zero)) };
            if g.menu.is_some() {
                return Some(Ok(zero));
            }
            let max = (max as i32).max(1);
            let initial = num(args, 2).map_or(max, |i| (i as i32).clamp(1, max));
            g.ext.retain(&cb);
            g.other.jip12.quantity = Some(QuantityAsk { callback: cb, max, initial });
            g.open_menu(crate::game::menus::QUANTITY);
            zero
        }
        // The text box opens with the formatted title; Ok gives the callback the text
        // [documented]; drawn as the name menu's box (`nvf_play::chargen::name`).
        // 1 when it opened [documented: "success:0/1"; hypothesis: 0 while another menu
        // is open].
        "showtextinputmenu" => {
            let Some(cb) = args.first().cloned().filter(|v| !matches!(v, XVal::Num(_))) else { return Some(Ok(zero)) };
            if g.menu.is_some() {
                return Some(Ok(zero));
            }
            let title = formatted(args, 3);
            let size = (num(args, 1).unwrap_or(0.0) as f32, num(args, 2).unwrap_or(0.0) as f32);
            g.ext.retain(&cb);
            g.other.jip12.text_input = Some(TextInput {
                callback: cb,
                title,
                size,
                offset: (0.0, 0.0),
                min: 1,
                max: TEXT_MAX,
                digits_only: false,
                enter_closes: false,
                text: String::new(),
                set_text: None,
            });
            g.open_menu(crate::game::menus::NAME);
            yes(true)
        }
        // Origin, the length limits (1 and 32,767 by default, 0 the latter too), digits
        // only (1), Enter closes (2) [documented].
        "settextinputextendedprops" => {
            let mut updated = false;
            if let Some(t) = g.other.jip12.text_input.as_mut() {
                t.offset = (num(args, 0).unwrap_or(0.0) as f32, num(args, 1).unwrap_or(0.0) as f32);
                t.min = num(args, 2).map_or(1, |m| m.max(0.0) as usize);
                t.max = num(args, 3).filter(|&m| m > 0.0).map_or(TEXT_MAX, |m| m as usize);
                let switches = num(args, 4).unwrap_or(0.0) as u32;
                t.digits_only = switches & 1 != 0;
                t.enter_closes = switches & 2 != 0;
                t.text = t.text.chars().take(t.max).collect();
                updated = true;
            }
            if updated {
                crate::ui_text_input::sync_open(g);
            }
            zero
        }
        "settextinputstring" => {
            let s = formatted(args, 0);
            let mut live = None;
            if let Some(t) = g.other.jip12.text_input.as_mut() {
                let clipped: String = s.chars().take(t.max).collect();
                t.text = clipped.clone();
                t.set_text = Some(s);
                live = Some(clipped);
            }
            if let Some(live) = live {
                crate::ui_text_input::sync_live_text(g, &live);
            }
            zero
        }
        // SetWheelDisabled's flag; 0 for a non-actor [documented].
        "getwheeldisabled" => yes(subject.is_some_and(|a| g.wheel_disabled(a))),
        _ => return None,
    };
    Some(Ok(v))
}

impl Game<'_> {
    /// The item menu on top: the barter, container or inventory menu open.
    fn menu_top_item_menu(&self) -> Option<u16> {
        [1053u16, 1008, 1002].into_iter().find(|c| self.other.jip12.item_menus.contains_key(c))
    }
}
