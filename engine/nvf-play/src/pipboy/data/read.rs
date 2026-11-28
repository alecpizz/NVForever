//! What the DATA page reads from the plugins: notes (`NOTE`), map markers (references
//! with `XMRK`), the worldspace's map (`WRLD ICON`, `MNAM`), regions' map names
//! (`REGN RDMP`) and the names of places (docs/spec-notes/pipboy.md §5.3).
//!
//! Subrecords are read from their raw bytes as xEdit lays them out [documented: xEdit
//! wbDefinitionsFNV.pas, as `schemas/fnv` records them] unless marked otherwise.

use super::model::{game_text, MapTransform, WorldMap, CELL};
use fnv_esm::{LoadOrder, Schema};
use fnv_sim::{FormId, RefTable};
use fnv_world::fields::{global, num, winner, Fields};
use fnv_world::WorldIndex;

/// The `STAT` every map marker reference places [observed: `NAME 0x00000010` on the
/// Goodsprings, Goodsprings Cemetery and Vault 22 markers].
pub const MAP_MARKER: u32 = 0x10;

fn fields<'a>(lo: &LoadOrder, schema: &'a Schema, form: u32) -> Option<Fields<'a>> {
    let (pi, r) = winner(lo, form)?;
    Fields::new(r, pi, schema).ok()
}

fn text(f: &Fields, sig: &[u8; 4]) -> Option<String> {
    f.raw(sig).map(|s| game_text(&s.data)).filter(|s| !s.trim().is_empty())
}

fn byte(f: &Fields, sig: &[u8; 4]) -> Option<u8> {
    f.raw(sig).and_then(|s| s.data.first().copied())
}

/// A note: its name, its type (`DATA`: 0 sound, 1 text, 2 image, 3 voice), what it shows
/// and the quest it belongs to (`ONAM`).
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub form: u32,
    pub name: String,
    pub kind: u8,
    /// A text note's text: `TNAM`, a NUL-ended string [observed: VMQ01CourierNote's
    /// `TNAM` is the text the owner's screenshot shows, "INSTRUCTIONS", a blank line,
    /// "Deliver the package …"; the schema leaves `TNAM` of `NOTE` unread].
    pub text: Option<String>,
    /// An image note's picture (`XNAM`).
    pub image: Option<String>,
    pub quest: Option<u32>,
}

pub fn note(lo: &LoadOrder, schema: &Schema, form: u32) -> Option<Note> {
    let f = fields(lo, schema, form)?;
    if &f.sig != b"NOTE" {
        return None;
    }
    let kind = byte(&f, b"DATA").unwrap_or(1);
    let quest = f.raw(b"ONAM").and_then(|s| s.data.get(..4)).and_then(|b| global(lo, f.plugin, u32::from_le_bytes([b[0], b[1], b[2], b[3]])));
    Some(Note {
        form,
        // `FULL` display name; fall back to `EDID` when a `NOTE` record carries no `FULL`
        // (for example terminal-only `NOTE` records referenced by `TERM`/`INAM`, matching
        // `Game::note_notice_text` and radio stations), and to raw `FormId` only when both
        // are absent or blank [design].
        name: text(&f, b"FULL")
            .or_else(|| text(&f, b"EDID"))
            .unwrap_or_else(|| format!("{}", FormId(form))),
        kind,
        text: if kind == 1 { f.raw(b"TNAM").map(|s| game_text(&s.data)) } else { None },
        image: if kind == 2 { text(&f, b"XNAM") } else { None },
        quest,
    })
}

/// A map marker: the reference, its worldspace and place, name, type (`TNAM`), flags
/// (`FNAM`: 1 visible, 2 can travel to), the radius the player finds it in (`XRDS`) and
/// its reputation (`WMI1`, a REPU: "the reputation that is shown when the player hovers
/// over the icon on the world map" [documented: GECK wiki Reference, Map Marker]).
#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    pub form: u32,
    pub world: u32,
    pub pos: [f32; 3],
    pub name: String,
    pub kind: u8,
    pub flags: u8,
    /// `XRDS`, 0 when the reference has none (found within the default distance then:
    /// `super::discovery_radius`, #2338).
    pub radius: f32,
    /// The reputation (REPU FormID) and its name (`FULL`).
    pub reputation: Option<(u32, String)>,
}

/// Every placed map marker that has a name, by worldspace.
pub fn markers(lo: &LoadOrder, schema: &Schema, refs: &RefTable) -> Vec<Marker> {
    let mut out = Vec::new();
    for r in refs.refs.values().filter(|r| r.base == FormId(MAP_MARKER)) {
        let Some(world) = r.cell.and_then(|c| refs.cell_world.get(&c)) else { continue };
        let Some(f) = fields(lo, schema, r.id.0) else { continue };
        if f.raw(b"XMRK").is_none() {
            continue;
        }
        let Some(name) = text(&f, b"FULL") else { continue };
        let radius = f.raw(b"XRDS").and_then(|s| s.data.get(..4)).map_or(0.0, |b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        let reputation = f
            .raw(b"WMI1")
            .and_then(|s| s.data.get(..4))
            .and_then(|b| global(lo, f.plugin, u32::from_le_bytes([b[0], b[1], b[2], b[3]])))
            .filter(|&rep| rep != 0)
            .and_then(|rep| Some((rep, fields(lo, schema, rep).and_then(|rf| text(&rf, b"FULL"))?)));
        out.push(Marker { form: r.id.0, world: world.0, pos: r.pos, name, kind: byte(&f, b"TNAM").unwrap_or(0), flags: byte(&f, b"FNAM").unwrap_or(0), radius, reputation });
    }
    out.sort_by_key(|m| m.form);
    out
}

/// This worldspace's own map transform; absent ONAM is identity, absent bounds have
/// centre zero [observed: run08 captures of MapMenu traits, 2026-11-07]. Field layouts
/// [documented: xEdit wbWorldMapOffset, wbWorldObjectBounds; schemas/fnv/world.json].
fn map_transform(f: &Fields) -> MapTransform {
    let floats = |sig: &[u8; 4], count: usize| -> Option<Vec<f32>> {
        let data = &f.raw(sig)?.data;
        (data.len() >= count * 4).then(|| data.as_chunks::<4>().0.iter().take(count).map(|b| f32::from_le_bytes(*b)).collect())
    };
    let mut t = MapTransform::default();
    if let Some(o) = floats(b"ONAM", 3) {
        t.scale = o[0];
        t.offset = [o[1], o[2]];
    }
    if let (Some(min), Some(max)) = (floats(b"NAM0", 2), floats(b"NAM9", 2)) {
        t.centre = [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5];
    }
    t
}

/// Follow WNAM only while PNAM bit 2 (Use Map Data) is set [documented: xEdit,
/// schemas/fnv/world.json]. The root supplies ICON/MNAM; the original worldspace
/// supplies ONAM and NAM0/NAM9 [observed: run08 captures of MapMenu traits, 2026-11-07].
pub fn world_map(lo: &LoadOrder, schema: &Schema, world: u32) -> Option<WorldMap> {
    let mut root = world;
    let mut f = fields(lo, schema, root).filter(|f| &f.sig == b"WRLD")?;
    let transform = map_transform(&f);
    let mut visited = std::collections::HashSet::new();
    loop {
        // Broken mods' parent cycles have no map root [design: this reader].
        if !visited.insert(root) {
            return None;
        }
        let uses_map = f.raw(b"PNAM").and_then(|s| s.data.get(..2)).is_some_and(|b| u16::from_le_bytes([b[0], b[1]]) & 4 != 0);
        if !uses_map {
            break;
        }
        let Some(parent) = f.formid(b"WNAM", lo) else { break };
        root = parent;
        f = fields(lo, schema, root).filter(|f| &f.sig == b"WRLD")?;
    }
    let texture = text(&f, b"ICON")?;
    let m = &f.raw(b"MNAM")?.data;
    if m.len() < 16 {
        return None;
    }
    let i32_at = |o: usize| i32::from_le_bytes([m[o], m[o + 1], m[o + 2], m[o + 3]]);
    let i16_at = |o: usize| i16::from_le_bytes([m[o], m[o + 1]]) as i32;
    Some(WorldMap { root, transform, texture, usable: [i32_at(0) as f32, i32_at(4) as f32], nw: [i16_at(8), i16_at(10)], se: [i16_at(12), i16_at(14)] })
}

/// A worldspace's `DATA` flags (bit 1: Can't Fast Travel [documented: xEdit]); 0 when
/// it has none.
pub fn world_flags(lo: &LoadOrder, schema: &Schema, world: u32) -> u8 {
    fields(lo, schema, world).filter(|f| &f.sig == b"WRLD").and_then(|f| byte(&f, b"DATA")).unwrap_or(0)
}

/// A region's map name (`RDMP`) and the priority of the map entry it belongs to (the
/// `RDAT` before it).
fn region_name(lo: &LoadOrder, schema: &Schema, region: u32) -> Option<(String, f64)> {
    let f = fields(lo, schema, region)?;
    let mut priority = 0.0;
    for s in &f.subs {
        match &s.sig {
            b"RDAT" => priority = f.decode(s).and_then(|v| num(&v, "priority")).unwrap_or(0.0),
            b"RDMP" => return Some((game_text(&s.data), priority)).filter(|(n, _)| !n.trim().is_empty()),
            _ => {}
        }
    }
    None
}

/// The name of a place: an interior's own name (`FULL`, else its Editor ID); outdoors the
/// map name of the regions the cell is in, the highest priority first, else the
/// worldspace's name [hypothesis: the owner's screenshots, taken in Goodsprings, show
/// "Goodsprings", the map name of `VMapGoodspringsRegion` (observed); an interior's own
/// name is NVForever's reading].
pub fn place_name(lo: &LoadOrder, schema: &Schema, index: &WorldIndex, world: u32, interior: Option<u32>, at: [f32; 3]) -> String {
    if let Some(c) = interior.and_then(|i| index.cells.get(&i)) {
        return c.name.clone().or_else(|| c.edid.clone()).unwrap_or_default();
    }
    let grid = ((at[0] / CELL).floor() as i32, (at[1] / CELL).floor() as i32);
    let cell = index.worldspaces.get(&world).and_then(|w| w.grid.get(&grid)).copied();
    let best = cell
        .into_iter()
        .flat_map(|c| fnv_world::audio::cell_regions_merged(lo, schema, c, index.merge_region_lists))
        .filter_map(|r| region_name(lo, schema, r))
        .max_by(|a, b| a.1.total_cmp(&b.1));
    match best {
        Some((n, _)) => n,
        None => index.worldspaces.get(&world).and_then(|w| w.name.clone().or_else(|| w.edid.clone())).unwrap_or_default(),
    }
}

/// A door that leads elsewhere, near where the local map looks: where it stands and the
/// name of the place it leads to.
#[derive(Debug, Clone, PartialEq)]
pub struct Door {
    pub form: u32,
    pub pos: [f32; 3],
    pub name: String,
    /// It leads outside: its destination door stands in a worldspace's cell.
    pub outside: bool,
}

/// REFR header flag "Hidden From Local Map" (bit 6): "The reference will not be displayed
/// on the local map" [documented: GECK wiki Reference, Reference Data Flags; xEdit
/// wbDefinitionsFNV.pas REFR flags; schemas/fnv/world.json].
pub const HIDDEN_FROM_LOCAL_MAP: u32 = 0x40;

/// DOOR `FNAM` flag "Hidden" (bit 2) [documented: xEdit wbDefinitionsFNV.pas DOOR;
/// schemas/fnv/objects.json]. What it does is not documented; NVForever leaves such a
/// door off the local map [hypothesis: the three doors flagged in FalloutNV.esm are
/// secret ones, `CG04SecretDoor`, `NVLucky38RmLoWallNoShadowDoor` and
/// `SewerLidExit01NoMinUse`, observed; docs/owner-review.md].
pub const DOOR_HIDDEN: u8 = 0x04;

/// Whether a door is left off the local map: its reference is flagged Hidden From Local
/// Map, or its base door is flagged Hidden (`door_flags`, DOOR `FNAM`).
pub fn door_hidden(ref_flags: u32, door_flags: u8) -> bool {
    ref_flags & HIDDEN_FROM_LOCAL_MAP != 0 || door_flags & DOOR_HIDDEN != 0
}

/// The doors leading elsewhere among the references of `cells`, named by the place their
/// destination door stands in [observed: the owner's Local Map screenshot labels the door
/// icon "Goodsprings Gas Station", the name of the cell `GSGasStation`]. Doors left off
/// the local map ([`door_hidden`]) are not listed; whether a door is enabled now is the
/// caller's to ask (it changes while the list is kept).
pub fn doors(lo: &LoadOrder, schema: &Schema, index: &WorldIndex, refs: &RefTable, cells: &[u32]) -> Vec<Door> {
    let mut out = Vec::new();
    for c in cells {
        for id in refs.by_cell.get(&FormId(*c)).into_iter().flatten() {
            let Some(r) = refs.get(*id) else { continue };
            let Some(t) = r.teleport else { continue };
            let door_flags = fields(lo, schema, r.base.0).filter(|f| &f.sig == b"DOOR").and_then(|f| byte(&f, b"FNAM")).unwrap_or(0);
            if door_hidden(r.flags, door_flags) {
                continue;
            }
            let Some(dest) = refs.get(t.door) else { continue };
            let Some(dest_cell) = dest.cell else { continue };
            let outside = refs.cell_world.contains_key(&dest_cell);
            let name = match refs.cell_world.get(&dest_cell) {
                Some(w) => place_name(lo, schema, index, w.0, None, dest.pos),
                None => place_name(lo, schema, index, 0, Some(dest_cell.0), dest.pos),
            };
            if !name.trim().is_empty() {
                out.push(Door { form: id.0, pos: r.pos, name, outside });
            }
        }
    }
    out
}

#[cfg(test)]
mod note_display_name_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use fnv_esm::raw::{write_subrecords, Subrecord};
    use fnv_esm::Plugin;

    fn sub(sig: &[u8; 4], data: Vec<u8>) -> Subrecord {
        Subrecord { sig: *sig, data, extended: false }
    }

    fn floats(sig: &[u8; 4], values: &[f32]) -> Subrecord {
        sub(sig, values.iter().flat_map(|v| v.to_le_bytes()).collect())
    }

    fn record(sig: &[u8; 4], id: u32, subs: &[Subrecord]) -> Vec<u8> {
        let payload = write_subrecords(subs);
        [&sig[..], &(payload.len() as u32).to_le_bytes(), &[0; 4], &id.to_le_bytes(), &[0; 4], &15u16.to_le_bytes(), &[0; 2], &payload].concat()
    }

    fn picture(name: &str) -> Vec<Subrecord> {
        let span = [2048i32.to_le_bytes(), 1024i32.to_le_bytes()].concat();
        let cells: Vec<u8> = [-2i16, 3, 1, 2].into_iter().flat_map(|v| v.to_le_bytes()).collect();
        vec![sub(b"ICON", [name.as_bytes(), &[0]].concat()), sub(b"MNAM", [span, cells].concat())]
    }

    fn parent(id: u32, flags: u16) -> Vec<Subrecord> {
        vec![sub(b"WNAM", id.to_le_bytes().to_vec()), sub(b"PNAM", flags.to_le_bytes().to_vec())]
    }

    /// All worldspaces and pictures here are made for these tests.
    fn maps(records: Vec<Vec<u8>>, test: impl FnOnce(&LoadOrder, &Schema)) {
        let hedr = [1.34f32.to_le_bytes(), 0i32.to_le_bytes(), 0x800u32.to_le_bytes()].concat();
        let bytes = [record(b"TES4", 0, &[sub(b"HEDR", hedr)]), records.concat()].concat();
        let plugins = [Plugin::parse(&bytes, "Maps.esm".into()).unwrap()];
        let lo = LoadOrder::new(&plugins).unwrap();
        let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
        test(&lo, &schema);
    }

    #[test]
    fn roots_apply_their_own_offsets_and_default_to_identity() {
        let plain = picture("plain.dds");
        let mut identity = picture("identity.dds");
        identity.push(floats(b"ONAM", &[1.0, 0.0, 0.0]));
        let mut scaled = picture("scaled.dds");
        scaled.push(floats(b"ONAM", &[2.0, 100.0, -200.0]));
        maps(vec![record(b"WRLD", 1, &plain), record(b"WRLD", 2, &identity), record(b"WRLD", 3, &scaled)], |lo, schema| {
            let a = world_map(lo, schema, 1).unwrap();
            let b = world_map(lo, schema, 2).unwrap();
            let c = world_map(lo, schema, 3).unwrap();
            assert_eq!(a.root, 1);
            assert_eq!(a.transform, MapTransform::default());
            assert_eq!(a.uv(-100.0, 300.0), b.uv(-100.0, 300.0));
            assert_eq!(c.transform.apply(-100.0, 300.0), [-100.0, 400.0]);
            assert_eq!(c.uv(-100.0, 300.0), a.uv(-100.0, 400.0));
        });
    }

    #[test]
    fn a_roots_scale_is_about_its_bounds_centre() {
        let mut root = picture("bounded.dds");
        root.push(floats(b"ONAM", &[2.0, 100.0, -200.0]));
        root.push(floats(b"NAM0", &[-100.0, 200.0]));
        let incomplete = root.clone();
        root.push(floats(b"NAM9", &[300.0, 1000.0]));
        maps(vec![record(b"WRLD", 1, &root), record(b"WRLD", 2, &incomplete)], |lo, schema| {
            let a = world_map(lo, schema, 1).unwrap();
            assert_eq!(a.transform.centre, [100.0, 600.0]);
            assert_eq!(a.transform.apply(-100.0, 300.0), [-200.0, -200.0]);
            let b = world_map(lo, schema, 2).unwrap();
            assert_eq!(b.transform.centre, [0.0; 2]);
            assert_eq!(b.transform.apply(-100.0, 300.0), [-100.0, 400.0]);
        });
    }

    #[test]
    fn a_map_family_uses_each_members_transform_and_only_the_roots_picture() {
        let mut root = picture("root.dds");
        root.push(floats(b"ONAM", &[3.0, 99.0, 88.0]));
        let mut child = parent(1, 4);
        // Its own picture must be ignored while Use Map Data is set.
        child.extend(picture("ignored.dds"));
        child.push(floats(b"ONAM", &[0.5, 1000.0, -2000.0]));
        child.push(floats(b"NAM0", &[-100.0, 200.0]));
        child.push(floats(b"NAM9", &[300.0, 1000.0]));
        maps(vec![record(b"WRLD", 1, &root), record(b"WRLD", 2, &child), record(b"WRLD", 3, &parent(2, 4))], |lo, schema| {
            let a = world_map(lo, schema, 1).unwrap();
            let b = world_map(lo, schema, 2).unwrap();
            let c = world_map(lo, schema, 3).unwrap();
            assert_eq!((a.root, b.root, c.root), (1, 1, 1));
            assert_eq!(b.texture, "root.dds");
            assert_eq!((b.usable, b.nw, b.se), (a.usable, a.nw, a.se));
            assert_eq!(b.transform.centre, [100.0, 600.0]);
            assert_eq!(b.transform.apply(500.0, 1000.0), [1300.0, -1200.0]);
            // The grandchild has identity ONAM, not either ancestor's transform.
            assert_eq!(c.transform, MapTransform::default());
            assert_eq!(b.uv(500.0, 1000.0), c.uv(1300.0, -1200.0));
            assert_ne!(a.uv(1300.0, -1200.0), c.uv(1300.0, -1200.0));
        });
    }

    #[test]
    fn a_child_without_use_map_data_keeps_its_own_map_or_none() {
        let mut child = parent(1, 2); // Use LOD Data alone does not inherit a map.
        child.extend(picture("own.dds"));
        maps(vec![record(b"WRLD", 1, &picture("root.dds")), record(b"WRLD", 2, &child), record(b"WRLD", 3, &parent(1, 0)), record(b"WRLD", 4, &parent(2, 4))], |lo, schema| {
            let child = world_map(lo, schema, 2).unwrap();
            assert_eq!((child.root, child.texture.as_str()), (2, "own.dds"));
            assert!(world_map(lo, schema, 3).is_none());
            assert_eq!(world_map(lo, schema, 4).unwrap().root, 2);
        });
    }

    #[test]
    fn parent_cycles_and_missing_parent_records_have_no_map() {
        maps(vec![record(b"WRLD", 1, &parent(2, 4)), record(b"WRLD", 2, &parent(1, 4)), record(b"WRLD", 3, &parent(99, 4))], |lo, schema| {
            assert!(world_map(lo, schema, 1).is_none());
            assert!(world_map(lo, schema, 3).is_none());
        });
    }

    #[test]
    fn a_map_family_can_have_more_than_four_generations() {
        let mut records = vec![record(b"WRLD", 1, &picture("root.dds"))];
        records.extend((2..=8).map(|id| record(b"WRLD", id, &parent(id - 1, 4))));
        maps(records, |lo, schema| {
            assert_eq!(world_map(lo, schema, 8).unwrap().root, 1);
        });
    }

    #[test]
    fn doors_hidden_from_the_local_map() {
        assert!(!door_hidden(0, 0));
        // Hidden From Local Map on the reference (the Dead Money doors 0x0100BFB1 …).
        assert!(door_hidden(HIDDEN_FROM_LOCAL_MAP | 0x400, 0));
        // Hidden on the base door (`CG04SecretDoor`, FNAM 28: Hidden, Minimal Use,
        // Sliding Door).
        assert!(door_hidden(0, 28));
        // Automatic Door and Minimal Use alone keep it on the map.
        assert!(!door_hidden(0x400, 0x02 | 0x08));
    }

    #[test]
    fn place_name_uses_merged_regions_only_when_hnvse_merge_enabled() {
        fn z(s: &str) -> Vec<u8> {
            [s.as_bytes(), &[0]].concat()
        }
        fn group(label: [u8; 4], kind: i32, body: &[u8]) -> Vec<u8> {
            [&b"GRUP"[..], &((24 + body.len()) as u32).to_le_bytes(), &label[..], &kind.to_le_bytes(), &[0; 8], body].concat()
        }
        fn tes4(masters: &[&str]) -> Vec<u8> {
            let hedr = [1.34f32.to_le_bytes(), 0i32.to_le_bytes(), 0x800u32.to_le_bytes()].concat();
            let mut subs = vec![sub(b"HEDR", hedr)];
            for m in masters {
                subs.push(sub(b"MAST", z(m)));
                subs.push(sub(b"DATA", 0u64.to_le_bytes().to_vec()));
            }
            record(b"TES4", 0, &subs)
        }
        fn map_region(id: u32, name: &str, prio: u8) -> Vec<u8> {
            // RDAT: type 4 (Map), flags 0, priority, pad -> followed by RDMP.
            let rdat = [4u32.to_le_bytes().as_slice(), &[0, prio, 0, 0]].concat();
            record(b"REGN", id, &[sub(b"RDAT", rdat), sub(b"RDMP", z(name))])
        }

        const WORLD: u32 = 0x10;
        const CELL: u32 = 0x20;
        const REG_MAP: u32 = 0x30;
        const REG_EMPTY: u32 = 0x31;

        let cell_base = record(b"CELL", CELL, &[sub(b"DATA", vec![0]), sub(b"XCLC", [0i32.to_le_bytes(), 0i32.to_le_bytes()].concat()), sub(b"XCLR", REG_MAP.to_le_bytes().to_vec())]);
        let cell_override = record(b"CELL", CELL, &[sub(b"DATA", vec![0]), sub(b"XCLC", [0i32.to_le_bytes(), 0i32.to_le_bytes()].concat()), sub(b"XCLR", REG_EMPTY.to_le_bytes().to_vec())]);
        let wrld_base = [
            record(b"WRLD", WORLD, &[sub(b"EDID", z("TestWorld")), sub(b"FULL", z("Fallback World"))]),
            group(WORLD.to_le_bytes(), 1, &group(0i32.to_le_bytes(), 4, &group(0i32.to_le_bytes(), 5, &cell_base))),
        ]
        .concat();
        let wrld_override = [
            record(b"WRLD", WORLD, &[sub(b"EDID", z("TestWorld")), sub(b"FULL", z("Fallback World"))]),
            group(WORLD.to_le_bytes(), 1, &group(0i32.to_le_bytes(), 4, &group(0i32.to_le_bytes(), 5, &cell_override))),
        ]
        .concat();
        let p0 = [
            tes4(&[]),
            group(*b"REGN", 0, &[map_region(REG_MAP, "Master Region", 50), record(b"REGN", REG_EMPTY, &[])].concat()),
            group(*b"WRLD", 0, &wrld_base),
        ]
        .concat();
        let p1 = [tes4(&["Base.esm"]), group(*b"WRLD", 0, &wrld_override)].concat();

        let plugins = [Plugin::parse(&p0, "Base.esm".into()).unwrap(), Plugin::parse(&p1, "Override.esp".into()).unwrap()];
        let lo = LoadOrder::new(&plugins).unwrap();
        let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
        let mut index = WorldIndex::build(&lo, &schema);

        // Vanilla (merge_region_lists = false): Override.esp's empty region wins -> world fallback.
        assert_eq!(place_name(&lo, &schema, &index, WORLD, None, [100.0, 100.0, 0.0]), "Fallback World");
        // hNVSE bMergeRegionLists=1: Master.esm's map region survives across override.
        index.set_merge_region_lists(true);
        assert_eq!(place_name(&lo, &schema, &index, WORLD, None, [100.0, 100.0, 0.0]), "Master Region");
    }
}
