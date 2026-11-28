//! The DATA page as plain data: the heading's date, the order of the quest list, a
//! list's scrolling, the world map's projection and the map markers' pictures
//! (docs/spec-notes/pipboy.md §5.3). No Bevy here; tested below.

use fnv_sim::clock::Calendar;

/// Game units across a cell [documented: world.md, 4096 units a side].
pub const CELL: f32 = 4096.0;

/// The heading's date and time: month, day and the year's last two digits with dots,
/// then the time of day as the wait menu writes it ("10.25.81, 9:29") [observed: the
/// owner's screenshots show "10.25.81, 9:29" on 25 October 2281; hypothesis: no leading
/// zero on the month or the day, a 24-hour clock as the wait menu's, wait-menu.md §4].
/// The clock in 12 hours with the player's tweak ("9:29 PM") [design: docs/design/tweaks.md].
pub fn date_time_with(c: &Calendar, twelve: bool) -> String {
    let minute = crate::wait::model::minute_of_day(c);
    let clock = if twelve { clock_12(minute) } else { crate::wait::model::clock(minute) };
    let date = crate::wait::model::styled_date(c).unwrap_or_else(|| format!("{}.{}.{:02}", c.month as i64 + 1, c.day as i64, (c.year as i64).rem_euclid(100)));
    format!("{date}, {clock}")
}

/// Game units in a metre, for the map's distance line [hypothesis: the scale commonly
/// given for Gamebryo games, 1 unit about 1.43 cm].
pub const UNITS_PER_METRE: f32 = 70.0;

/// A distance in game units as the World Map's tweak line writes it: metres under a
/// kilometre, else kilometres to a tenth ("350 m away", "2.4 km away"; our words) [design: docs/design/tweaks.md].
pub fn distance_text(units: f32) -> String {
    let m = units / UNITS_PER_METRE;
    if m < 1000.0 {
        format!("{:.0} m away", (m / 10.0).round() * 10.0)
    } else {
        format!("{:.1} km away", m / 1000.0)
    }
}

/// `minute_of_day` as a 12-hour clock: 0:05 is "12:05 AM", 13:30 "1:30 PM".
pub fn clock_12(minute_of_day: u32) -> String {
    let h = minute_of_day / 60 % 24;
    let shown = if h.is_multiple_of(12) { 12 } else { h % 12 };
    format!("{shown}:{:02} {}", minute_of_day % 60, if h < 12 { "AM" } else { "PM" })
}

/// A quest in the list: when it was first seen listed (a running count; larger is newer),
/// its FormID, whether it is done (completed or failed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuestKey {
    pub seen: u64,
    pub form: u32,
    pub done: bool,
}

/// The quest list's order: quests still running before those done, each part newest
/// first, quests first seen together by FormID from the highest [hypothesis: fits the
/// owner's screenshot, where the add-ons' quests (FormIDs 01…–04…) come before "Back in
/// the Saddle" (0010A214), before "They Went That-a-Way" (000842DD), and the completed
/// "Ain't That a Kick in the Head" is last].
pub fn sort_quests(keys: &mut [QuestKey]) {
    keys.sort_by(|a, b| a.done.cmp(&b.done).then(b.seen.cmp(&a.seen)).then(b.form.cmp(&a.form)));
}

/// [`sort_quests`], then moves `top` (`PushUIQuestToTop` [documented: GECK wiki]) to the
/// first entry if present, preserving the relative order of the remaining quests.
pub fn sort_quests_with_top(keys: &mut [QuestKey], top: Option<u32>) {
    sort_quests(keys);
    if let Some(form) = top {
        if let Some(pos) = keys.iter().position(|q| q.form == form) {
            keys[..=pos].rotate_right(1);
        }
    }
}

/// An objective of the quest shown: its index in the quest, its text, done or not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectiveLine {
    pub index: i32,
    pub text: String,
    pub done: bool,
}

/// The objectives' order: those still to do first, then those done, each part from the
/// latest objective down [hypothesis].
pub fn sort_objectives(lines: &mut [ObjectiveLine]) {
    lines.sort_by(|a, b| a.done.cmp(&b.done).then(b.index.cmp(&a.index)));
}

/// The first row shown so that row `selected` is among the `fit` rows shown from it.
pub fn keep_in_view(selected: usize, first: usize, fit: usize) -> usize {
    let fit = fit.max(1);
    if selected < first {
        selected
    } else if selected >= first + fit {
        selected + 1 - fit
    } else {
        first
    }
}

/// A row index moved by `by`, held within `len` rows.
pub fn step(selected: usize, by: i32, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    (selected as i64 + by as i64).clamp(0, len as i64 - 1) as usize
}

/// A worldspace's map: its picture and the cells it shows (WRLD `ICON`, `MNAM`)
/// [documented: xEdit, schema `world.json`].
#[derive(Debug, Clone, PartialEq)]
pub struct WorldMap {
    /// The last WNAM parent reached through PNAM Use Map Data [documented: xEdit].
    pub root: u32,
    /// This worldspace's own ONAM and bounds, even when the picture is inherited.
    pub transform: MapTransform,
    /// The picture, as the interface names it (under `textures\`).
    pub texture: String,
    /// `MNAM` usable width and height: the map's size on the canvas at a magnification
    /// of 1 [hypothesis: the menu file draws FO3's map 2048 wide (`filewidth`) and FO3's
    /// and this game's worldspaces both give 2048].
    pub usable: [f32; 2],
    /// The north-west and south-east cells the picture runs between.
    pub nw: [i32; 2],
    pub se: [i32; 2],
}

/// The cell span's share of the picture on both axes
/// [observed: run08 captures of MapMenu traits, 2026-11-07].
pub const MAP_SPAN: f32 = 51.0 / 64.0;

/// WRLD ONAM scale and offsets about the centre of NAM0/NAM9 [documented: xEdit,
/// schemas/fnv/world.json; observed: run08 captures of MapMenu traits, 2026-11-07].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapTransform {
    pub scale: f32,
    pub offset: [f32; 2],
    pub centre: [f32; 2],
}

impl Default for MapTransform {
    fn default() -> Self {
        Self { scale: 1.0, offset: [0.0; 2], centre: [0.0; 2] }
    }
}

impl MapTransform {
    pub fn apply(&self, x: f32, y: f32) -> [f32; 2] {
        let axis = |p: f32, i: usize| self.scale * (p - self.centre[i]) + self.centre[i] + self.offset[i];
        [axis(x, 0), axis(y, 1)]
    }

    /// The position whose map position is `p` ([`MapTransform::apply`] undone); a scale
    /// of 0 (a broken record) is taken as 1.
    pub fn undo(&self, p: [f32; 2]) -> [f32; 2] {
        let s = if self.scale.is_finite() && self.scale != 0.0 { self.scale } else { 1.0 };
        let axis = |m: f32, i: usize| (m - self.centre[i] - self.offset[i]) / s + self.centre[i];
        [axis(p[0], 0), axis(p[1], 1)]
    }
}

impl WorldMap {
    /// Where a position in this worldspace falls on its root's picture, in texture
    /// coordinates [observed: run08 captures of MapMenu traits, 2026-11-07].
    pub fn uv(&self, x: f32, y: f32) -> [f32; 2] {
        let [x, y] = self.transform.apply(x, y);
        let across = (self.se[0] - self.nw[0] + 1).max(1) as f32;
        let down = (self.nw[1] - self.se[1] + 1).max(1) as f32;
        let cells = [(x / CELL - self.nw[0] as f32) / across, ((self.nw[1] + 1) as f32 - y / CELL) / down];
        [0.5 + MAP_SPAN * (cells[0] - 0.5), 0.5 + MAP_SPAN * (cells[1] - 0.5)]
    }

    /// The position in this worldspace at texture coordinates `uv` of its root's picture:
    /// [`WorldMap::uv`] undone (Place Marker on the World Map, #1954) [design].
    pub fn position(&self, uv: [f32; 2]) -> [f32; 2] {
        let across = (self.se[0] - self.nw[0] + 1).max(1) as f32;
        let down = (self.nw[1] - self.se[1] + 1).max(1) as f32;
        let cells = [(uv[0] - 0.5) / MAP_SPAN + 0.5, (uv[1] - 0.5) / MAP_SPAN + 0.5];
        let map = [(cells[0] * across + self.nw[0] as f32) * CELL, ((self.nw[1] + 1) as f32 - cells[1] * down) * CELL];
        self.transform.undo(map)
    }
}

/// Where a map picture `size` long lies in its window along one axis, its point at `t`
/// (0 to 1 of it) wanted at the window's middle `mid`: the menu file keeps the picture's
/// near edge no further in than the middle and its far edge no further out
/// [documented: map_menu.xml `MM_WorldMap_ParentImage` x between 427.5 − width and
/// 427.5, y between 250 − height and 250].
pub fn pan(t: f32, size: f32, mid: f32) -> f32 {
    // A worldspace's map size comes from its record: a broken mod's negative or NaN size
    // keeps the picture at the middle (`clamp` would panic).
    if !(size.is_finite() && size > 0.0 && t.is_finite()) {
        return mid;
    }
    (mid - t * size).clamp(mid - size, mid)
}

/// Map marker types (REFR `TNAM` of a map marker) [documented: xEdit wbDefinitionsFNV.pas,
/// "None", "City", "Settlement", "Encampment", "Natural Landmark", "Cave", "Factory",
/// "Monument", "Military", "Office", "Town Ruins", "Urban Ruins", "Sewer Ruins", "Metro",
/// "Vault"; observed: Vault 22 and Vault 3 are 14, Hidden Supply Cave 5, Camp McCarran 8,
/// Hoover Dam 7].
/// The table is the game's (GetCustomMapMarkerIcon reads it too).
const ICONS: [&str; 15] = fnv_game::nvse_jg7::MAP_MARKER_ICONS;

/// The picture of a map marker of type `kind`: its type's icon once it counts as
/// discovered, the "undiscovered" icon while it is only known [hypothesis: by the files'
/// names in `interface\icons\world map\`; type 0 "None" takes the settlement's picture,
/// design]. The highlighted marker keeps its picture and is drawn larger and brighter
/// (owner 2026-10-23: not the inverted files, which read as the undiscovered icon).
pub fn marker_icon(kind: u8, discovered: bool) -> String {
    let name = if discovered { ICONS.get(kind as usize).copied().unwrap_or("settlement") } else { "undiscovered" };
    format!("interface\\icons\\world map\\icon_map_{name}.dds")
}

/// The line a World Map marker's reputation (REFR `WMI1`) shows under its name: the
/// reputation's name and the player's standing title with it ("Goodsprings: Neutral"),
/// or the name alone without a standing (no scripts' game) [documented: GECK wiki
/// Reference, Map Marker, "Reputation: The reputation that is shown when the player
/// hovers over the icon on the world map"; hypothesis: the wording, a second line, and
/// the title (`crate::reputation::title`); docs/owner-review.md].
pub fn reputation_line(name: &str, title: Option<&str>) -> String {
    match title {
        Some(t) => format!("{name}: {t}"),
        None => name.to_string(),
    }
}

/// Text from the plugins' bytes, up to the first NUL, in this run's code page
/// (`fnv_util::text`, Windows-1252 unless set otherwise).
pub fn game_text(bytes: &[u8]) -> String {
    fnv_util::text::decode_z(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_heading_writes_the_date_month_first() {
        let c = Calendar { year: 2281.0, month: 9.0, day: 25.0, hour: 9.0 + 29.0 / 60.0, days_passed: 12.0 };
        assert_eq!(date_time_with(&c, false), "10.25.81, 9:29");
        let c = Calendar { year: 2282.0, month: 0.0, day: 3.0, hour: 21.05, days_passed: 0.0 };
        assert_eq!(date_time_with(&c, false), "1.3.82, 21:03");
        assert_eq!(date_time_with(&c, true), "1.3.82, 9:03 PM", "the 12-hour tweak");
        assert_eq!(distance_text(70.0 * 347.0), "350 m away");
        assert_eq!(distance_text(70.0 * 2400.0), "2.4 km away");
        assert_eq!(clock_12(5), "12:05 AM");
    }

    #[test]
    fn quests_running_come_first_newest_on_top() {
        let k = |seen, form, done| QuestKey { seen, form, done };
        let mut v = vec![k(0, 0x104C1C, true), k(0, 0x0842DD, false), k(0, 0x10A214, false), k(0, 0x01005229, false), k(3, 0x01, false)];
        sort_quests(&mut v);
        let forms: Vec<u32> = v.iter().map(|q| q.form).collect();
        assert_eq!(forms, vec![0x01, 0x01005229, 0x10A214, 0x0842DD, 0x104C1C]);
        // PushUIQuestToTop promotes a listed quest to index 0 without disturbing the rest,
        // and ignores an unlisted FormID.
        sort_quests_with_top(&mut v, Some(0x0842DD));
        assert_eq!(v.iter().map(|q| q.form).collect::<Vec<_>>(), vec![0x0842DD, 0x01, 0x01005229, 0x10A214, 0x104C1C]);
        sort_quests_with_top(&mut v, Some(0x999999));
        assert_eq!(v.iter().map(|q| q.form).collect::<Vec<_>>(), vec![0x01, 0x01005229, 0x10A214, 0x0842DD, 0x104C1C]);
    }

    #[test]
    fn objectives_to_do_come_before_those_done() {
        let o = |index, done| ObjectiveLine { index, text: String::new(), done };
        let mut v = vec![o(10, true), o(20, false), o(30, true), o(5, false)];
        sort_objectives(&mut v);
        assert_eq!(v.iter().map(|o| o.index).collect::<Vec<_>>(), vec![20, 5, 30, 10]);
    }

    #[test]
    fn the_selected_row_stays_in_view() {
        assert_eq!(keep_in_view(0, 3, 5), 0);
        assert_eq!(keep_in_view(9, 3, 5), 5);
        assert_eq!(keep_in_view(5, 3, 5), 3);
        assert_eq!(step(0, -1, 4), 0);
        assert_eq!(step(2, 5, 4), 3);
        assert_eq!(step(2, 1, 0), 0);
    }

    #[test]
    fn map_texture_coordinates_use_the_exact_span_on_both_axes() {
        let m = WorldMap { root: 1, transform: MapTransform::default(), texture: String::new(), usable: [512.0, 1024.0], nw: [-2, 3], se: [1, 2] };
        // Synthetic four-by-two-cell map: NW corner, centre and SE corner.
        assert_eq!(m.uv(-2.0 * CELL, 4.0 * CELL), [13.0 / 128.0; 2]);
        assert_eq!(m.uv(0.0, 3.0 * CELL), [0.5; 2]);
        assert_eq!(m.uv(2.0 * CELL, 2.0 * CELL), [115.0 / 128.0; 2]);
        // Texture coordinates do not depend on the picture's usable pixel size.
        let resized = WorldMap { usable: [2048.0; 2], ..m.clone() };
        assert_eq!(m.uv(-CELL, 2.5 * CELL), resized.uv(-CELL, 2.5 * CELL));
        assert_eq!(m.uv(-CELL, 2.5 * CELL), [0.5 - 51.0 / 256.0, 0.5 + 51.0 / 256.0]);
    }

    #[test]
    fn the_wasteland_map_puts_markers_where_the_original_reads_them() {
        // WastelandNV: MNAM usable 2048 × 2048, cells (-35, 35) to (25, -25), ONAM scale 1
        // and offsets 0, so the transform is the identity [FalloutNV.esm's WRLD, read with fnvtool].
        let m = WorldMap { root: 1, transform: MapTransform::default(), texture: String::new(), usable: [2048.0; 2], nw: [-35, 35], se: [25, -25] };
        // Each marker reference's world X/Y and the `_x`/`_y` the original's MapMenu
        // reported for its tile (pipboy.md §5.3, World Map) [observed: run08, 2026-11-07].
        // The rule fits all 184 markers placed in WastelandNV to under 0.0001 texel of
        // the 1024 picture; 0.01 texel leaves room for 32-bit rounding, while the old
        // screenshot fit (origin 0.108/0.102, span 0.7876) missed these by 1.5 to 5.3 texels.
        let places = [
            ("Goodsprings", -70992.0, 2560.0, 0.33236843, 0.5636847),
            ("Hoover Dam", 74712.0, 24848.0, 0.79706764, 0.49260074),
            ("Jacobstown", -108427.35, 96469.77, 0.21297449, 0.26417476),
        ];
        for (name, x, y, u, v) in places {
            let uv = m.uv(x, y);
            let off = [(uv[0] - u) * 1024.0, (uv[1] - v) * 1024.0];
            assert!(off[0].abs() < 0.01 && off[1].abs() < 0.01, "{name}: {uv:?} is {off:?} texels from ({u}, {v})");
        }
    }

    #[test]
    fn a_point_on_the_picture_goes_back_to_its_position() {
        // The child worldspace's own scale and offsets, as `family_markers…` uses them.
        let t = MapTransform { scale: 0.5, offset: [1000.0, -3.0 * CELL], centre: [200.0, -400.0] };
        let m = WorldMap { root: 1, transform: t, texture: String::new(), usable: [2048.0; 2], nw: [-35, 35], se: [25, -25] };
        for (x, y) in [(-70992.0, 2560.0), (74712.0, 24848.0), (0.0, 0.0)] {
            let back = m.position(m.uv(x, y));
            assert!((back[0] - x).abs() < 0.5 && (back[1] - y).abs() < 0.5, "({x}, {y}) came back as {back:?}");
        }
        // A broken record's scale of 0 is taken as 1 both ways round the undo.
        let flat = MapTransform { scale: 0.0, ..MapTransform::default() };
        assert_eq!(flat.undo([5.0, 6.0]), [5.0, 6.0]);
    }

    #[test]
    fn a_map_stays_over_its_window_middle() {
        // A 2048 map centred on its middle point; at its edges it stops at the middle.
        assert_eq!(pan(0.5, 2048.0, 427.5), 427.5 - 1024.0);
        assert_eq!(pan(-0.2, 2048.0, 427.5), 427.5);
        assert_eq!(pan(1.3, 2048.0, 427.5), 427.5 - 2048.0);
    }

    #[test]
    fn marker_pictures_follow_the_type() {
        assert_eq!(marker_icon(14, true), "interface\\icons\\world map\\icon_map_vault.dds");
        assert_eq!(marker_icon(3, false), "interface\\icons\\world map\\icon_map_undiscovered.dds");
        assert_eq!(marker_icon(5, true), "interface\\icons\\world map\\icon_map_cave.dds");
        assert_eq!(marker_icon(99, true), "interface\\icons\\world map\\icon_map_settlement.dds");
    }

    #[test]
    fn a_markers_reputation_line() {
        assert_eq!(reputation_line("Goodsprings", Some("Neutral")), "Goodsprings: Neutral");
        assert_eq!(reputation_line("NCR", Some("Trusted")), "NCR: Trusted");
        assert_eq!(reputation_line("NCR", None), "NCR");
    }

    #[test]
    fn plugin_text_reads_windows_1252() {
        assert_eq!(game_text(b"Ain\x92t That\0junk"), "Ain’t That");
        assert_eq!(game_text(b"caf\xe9"), "café");
    }
}
