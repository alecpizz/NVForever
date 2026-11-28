//! Loading screens (LSCR): the picture and text shown while the game loads, and the
//! places a screen belongs to. Facts and tags: docs/spec-notes/loading-screens.md.

use crate::fields::{formid_of, global, num, winner, Fields};
use fnv_esm::{LoadOrder, Schema};
use std::collections::BTreeSet;

/// Record flag "deleted" [documented: xEdit record header flags].
const DELETED: u32 = 0x20;

/// The LSCR header flag that puts a screen behind the main menu, by its schema name
/// [documented: xEdit wbDefinitionsFNV.pas LSCR, bit 10; schemas/fnv/gameplay.json].
pub const MAIN_MENU_FLAG: &str = "Displays In Main Menu";

/// Where a screen belongs (LSCR LNAM) [documented: fopdoc FalloutNV/Records/LSCR].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenLocation {
    /// "Direct": a cell or a worldspace (FormID).
    Direct(u32),
    /// "Indirect": a grid square of a worldspace.
    Grid { world: u32, x: i32, y: i32 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadScreen {
    pub form_id: u32,
    pub edid: Option<String>,
    /// ICON as stored: a path under `textures\`.
    pub texture: Option<String>,
    /// DESC: the text shown with the picture.
    pub text: String,
    /// Empty: a screen for anywhere.
    pub locations: Vec<ScreenLocation>,
    /// Header flag "Displays In Main Menu" ([`MAIN_MENU_FLAG`]).
    pub main_menu: bool,
    /// How the text is set: the LSCT that WMI1 names, when it is one of the tip type.
    pub tip: Option<TipStyle>,
}

/// LSCT's type for a tip [documented: fopdoc FalloutNV/Records/LSCT].
const TIP_TYPE: u64 = 3;

/// How a screen's tip is set: the fields of an LSCT of the tip type
/// [documented: fopdoc FalloutNV/Records/LSCT; units: loading-screens.md §4].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TipStyle {
    /// The text box's left edge and width, pixels of a window 1280 wide.
    pub x: f32,
    pub width: f32,
    /// The text box's top and height, parts of 960 of the window's height.
    pub y: f32,
    pub height: f32,
    /// The font's number (`sFontFile_n`).
    pub font: u32,
    /// The text's colour, 0 to 255 per channel.
    pub colour: [f32; 3],
    /// 1 left, 2 centre, 4 right.
    pub alignment: u32,
}

impl TipStyle {
    /// For a screen with no LSCT of the tip type behind it: the values of
    /// `StandardLoadingTip` in FalloutNV.esm [observed: `fnvtool esm decode`,
    /// loading-screens.md §2].
    pub const FALLBACK: TipStyle = TipStyle { x: 210.0, width: 850.0, y: 100.0, height: 400.0, font: 6, colour: [255.0, 180.0, 53.0], alignment: 2 };
}

/// Where the player is being taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Destination {
    /// The interior cell, when going indoors.
    pub interior: Option<u32>,
    /// Outdoors: the worldspace and the grid square arrived in.
    pub exterior: Option<(u32, (i32, i32))>,
}

/// A grid location covers the squares within this many steps of its own: the squares
/// loaded around a player standing on it [hypothesis: the game's own rule is not known;
/// loading-screens.md §3].
pub const GRID_REACH: i32 = 2;

impl LoadScreen {
    /// Whether one of the screen's locations is the destination.
    pub fn belongs_to(&self, d: &Destination) -> bool {
        self.locations.iter().any(|l| match *l {
            ScreenLocation::Direct(id) => d.interior == Some(id) || d.exterior.is_some_and(|(w, _)| w == id),
            ScreenLocation::Grid { world, x, y } => {
                d.exterior.is_some_and(|(w, g)| w == world && (g.0 - x).abs() <= GRID_REACH && (g.1 - y).abs() <= GRID_REACH)
            }
        })
    }
}

/// Every loading screen of the load order (the winning definition of each), by FormID.
pub fn load_screens(lo: &LoadOrder, schema: &Schema) -> Vec<LoadScreen> {
    let mut ids = BTreeSet::new();
    for (pi, records) in lo.records.iter().enumerate() {
        for r in records.iter().filter(|r| &r.header.sig == b"LSCR") {
            ids.extend(lo.to_global(pi, r.header.form_id));
        }
    }
    ids.into_iter().filter_map(|id| load_screen(lo, schema, id)).collect()
}

fn load_screen(lo: &LoadOrder, schema: &Schema, form_id: u32) -> Option<LoadScreen> {
    let (pi, r) = winner(lo, form_id)?;
    if &r.header.sig != b"LSCR" || r.header.flags & DELETED != 0 {
        return None;
    }
    let f = Fields::new(r, pi, schema).ok()?;
    let locations = f
        .subs
        .iter()
        .filter(|s| &s.sig == b"LNAM")
        .filter_map(|s| {
            let v = f.decode(s)?;
            let id = |k: &str| v.get(k).and_then(formid_of).and_then(|l| global(lo, pi, l));
            match (id("direct"), id("indirect_world")) {
                (Some(direct), _) => Some(ScreenLocation::Direct(direct)),
                (None, Some(world)) => {
                    Some(ScreenLocation::Grid { world, x: num(&v, "indirect_grid_x")? as i32, y: num(&v, "indirect_grid_y")? as i32 })
                }
                (None, None) => None,
            }
        })
        .collect();
    Some(LoadScreen {
        form_id,
        edid: f.string(b"EDID"),
        texture: f.string(b"ICON").filter(|t| !t.is_empty()),
        text: f.string(b"DESC").unwrap_or_default(),
        locations,
        main_menu: schema.header_flag(b"LSCR", MAIN_MENU_FLAG).is_some_and(|m| r.header.flags & m != 0),
        tip: f.formid(b"WMI1", lo).and_then(|id| tip_style(lo, schema, id)),
    })
}

/// The style of the LSCT `form_id`; `None` for anything but a whole one of the tip type.
fn tip_style(lo: &LoadOrder, schema: &Schema, form_id: u32) -> Option<TipStyle> {
    let (pi, r) = winner(lo, form_id)?;
    if &r.header.sig != b"LSCT" || r.header.flags & DELETED != 0 {
        return None;
    }
    let f = Fields::new(r, pi, schema).ok()?;
    let v = f.get(b"DATA")?;
    let int = |k: &str| num(&v, k).map(|n| n as u32);
    let float = |k: &str| num(&v, k).map(|n| n as f32);
    if num(&v, "type")? as u64 != TIP_TYPE {
        return None;
    }
    Some(TipStyle {
        x: float("x")?,
        width: float("width")?,
        y: float("y")?,
        height: float("height")?,
        font: int("font_1")?,
        colour: [float("font_1_red")?, float("font_1_green")?, float("font_1_blue")?],
        alignment: int("font_1_alignment")?,
    })
}

/// The pictures shown behind the main menu: those of the screens flagged Displays In Main
/// Menu [documented: the flag's name]; when no screen has the flag (a load order whose
/// plugins never set it), the screens for anywhere [design: as before the flag was read].
/// Sorted, each once.
pub fn main_menu_pictures(screens: &[LoadScreen]) -> Vec<String> {
    let flagged = screens.iter().any(|s| s.main_menu);
    let mut out: Vec<String> = screens.iter().filter(|s| if flagged { s.main_menu } else { s.locations.is_empty() }).filter_map(|s| s.texture.clone()).collect();
    out.sort();
    out.dedup();
    out
}

/// The screen for a trip to `to`. Screens that belong to the destination and screens for
/// anywhere are both allowed [documented: GECK wiki SetLocationSpecificLoadScreensOnly,
/// which exists to leave only the former]; when the destination has its own, one of them
/// is taken every other time [design: the game's odds are not known]. `roll` is any
/// number, e.g. random: the same roll gives the same screen.
pub fn choose<'a>(screens: &'a [LoadScreen], to: &Destination, roll: u64) -> Option<&'a LoadScreen> {
    choose_with(screens, to, roll, false)
}

/// [`choose`] with optional location-specific preference (`prefer_location`, setting
/// `location-load-screens`, #3844): when `true` and the destination has matching
/// location-specific screens, always pick from them instead of alternating with generic
/// screens [documented: JIP LN NVSE v57.30 / Settings INI v56.24 `jip_nvse.ini`
/// `bLoadScreenFix`; no DLL or source consulted; deterministic `roll % own.len()` tiebreak
/// and generic fallback when none match are `[design]` / `[hypothesis]` per
/// `docs/spec-notes/location-load-screens.md`].
pub fn choose_with<'a>(screens: &'a [LoadScreen], to: &Destination, roll: u64, prefer_location: bool) -> Option<&'a LoadScreen> {
    let own: Vec<&LoadScreen> = screens.iter().filter(|s| s.belongs_to(to)).collect();
    let anywhere: Vec<&LoadScreen> = screens.iter().filter(|s| s.locations.is_empty()).collect();
    if prefer_location && !own.is_empty() {
        return Some(own[(roll % own.len() as u64) as usize]);
    }
    let pool = match (own.is_empty(), anywhere.is_empty()) {
        (true, true) => return None,
        (false, true) => own,
        (true, false) => anywhere,
        (false, false) => {
            if roll & 1 == 0 {
                own
            } else {
                anywhere
            }
        }
    };
    Some(pool[((roll >> 1) % pool.len() as u64) as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOJAVE: u32 = 0xDA726;
    const SALOON: u32 = 0x106FE5;

    fn screen(form_id: u32, locations: Vec<ScreenLocation>) -> LoadScreen {
        LoadScreen { form_id, edid: None, texture: None, text: String::new(), locations, main_menu: false, tip: None }
    }

    fn outdoors(x: i32, y: i32) -> Destination {
        Destination { interior: None, exterior: Some((MOJAVE, (x, y))) }
    }

    #[test]
    fn locations() {
        let town = screen(1, vec![ScreenLocation::Grid { world: MOJAVE, x: -17, y: -2 }]);
        assert!(town.belongs_to(&outdoors(-17, -2)));
        assert!(town.belongs_to(&outdoors(-18, 0)));
        assert!(!town.belongs_to(&outdoors(-17, 1)));
        assert!(!town.belongs_to(&Destination { interior: None, exterior: Some((0x100, (-17, -2))) }));
        assert!(!town.belongs_to(&Destination { interior: Some(SALOON), exterior: None }));

        let bar = screen(2, vec![ScreenLocation::Direct(0x200), ScreenLocation::Direct(SALOON)]);
        assert!(bar.belongs_to(&Destination { interior: Some(SALOON), exterior: None }));
        assert!(!bar.belongs_to(&outdoors(0, 0)));
        // A worldspace named directly covers all of it.
        assert!(screen(3, vec![ScreenLocation::Direct(MOJAVE)]).belongs_to(&outdoors(40, 40)));
        assert!(!screen(4, vec![]).belongs_to(&outdoors(0, 0)));
    }

    #[test]
    fn choice() {
        let screens = vec![
            screen(1, vec![]),
            screen(2, vec![ScreenLocation::Direct(SALOON)]),
            screen(3, vec![]),
            screen(4, vec![ScreenLocation::Direct(0x999)]),
        ];
        let saloon = Destination { interior: Some(SALOON), exterior: None };
        let picked = |to: &Destination| -> BTreeSet<u32> { (0..64).filter_map(|r| choose(&screens, to, r)).map(|s| s.form_id).collect() };
        // The saloon's own screen and the ones for anywhere; never another place's.
        assert_eq!(picked(&saloon), BTreeSet::from([1, 2, 3]));
        assert_eq!(picked(&outdoors(0, 0)), BTreeSet::from([1, 3]));
        // Its own screen every other time.
        assert_eq!((0..64).filter(|&r| choose(&screens, &saloon, r).unwrap().form_id == 2).count(), 32);
        assert_eq!(choose(&screens, &saloon, 5), choose(&screens, &saloon, 5));
        assert_eq!(choose(&[], &saloon, 0), None);
        // Only screens of other places: nothing fits.
        assert_eq!(choose(&screens[3..], &saloon, 0), None);
        // Off (prefer_location = false) matches vanilla choose identically.
        assert!((0..64).all(|r| choose_with(&screens, &saloon, r, false) == choose(&screens, &saloon, r)));
        // On (prefer_location = true): always picks matching location screens when present,
        // falling back to generic screens only when no location screen matches.
        assert!((0..64).all(|r| choose_with(&screens, &saloon, r, true).unwrap().form_id == 2));
        assert_eq!(
            (0..64).filter_map(|r| choose_with(&screens, &outdoors(0, 0), r, true)).map(|s| s.form_id).collect::<BTreeSet<_>>(),
            BTreeSet::from([1, 3])
        );
        // Multiple location-specific screens cycle deterministically without picking generic ones.
        let mut multi = screens.clone();
        multi.push(screen(5, vec![ScreenLocation::Direct(SALOON)]));
        assert_eq!(
            (0..64).filter_map(|r| choose_with(&multi, &saloon, r, true)).map(|s| s.form_id).collect::<BTreeSet<_>>(),
            BTreeSet::from([2, 5])
        );
    }

    #[test]
    fn main_menu_pictures_are_the_flagged_screens() {
        let pic = |id: u32, t: &str, locations: Vec<ScreenLocation>, main_menu: bool| LoadScreen { texture: Some(t.into()), main_menu, ..screen(id, locations) };
        let mut screens = vec![
            pic(1, "b.dds", vec![], false),
            pic(2, "a.dds", vec![ScreenLocation::Direct(SALOON)], true),
            pic(3, "c.dds", vec![], true),
            pic(4, "c.dds", vec![], true),
        ];
        // Flagged screens only, a location or not; each picture once.
        assert_eq!(main_menu_pictures(&screens), vec!["a.dds".to_string(), "c.dds".to_string()]);
        // None flagged: the screens for anywhere.
        for s in &mut screens {
            s.main_menu = false;
        }
        assert_eq!(main_menu_pictures(&screens), vec!["b.dds".to_string(), "c.dds".to_string()]);
    }
}
