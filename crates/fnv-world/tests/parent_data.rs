//! A child worldspace's parent use flags (WRLD `WNAM`, `PNAM`; #2345, world.md §2.1) on a
//! synthetic plugin built in memory (no game data): what the world index takes from the
//! parent for land, default heights, water type, climate and distant LOD, and what it
//! leaves alone.

use fnv_esm::raw::{write_subrecords, Subrecord};
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_world::index::parent_use;
use fnv_world::WorldIndex;

const PARENT: u32 = 0x100;
const SHARES_LAND: u32 = 0x200;
const SHARES_ALL: u32 = 0x300;
const MAP_ONLY: u32 = 0x400;
const SHARES_LOD: u32 = 0x500;
const CLIMATE_P: u32 = 0x10;
const CLIMATE_C: u32 = 0x11;
const WATER_P: u32 = 0x12;
const WATER_C: u32 = 0x13;

fn sub(sig: &[u8; 4], data: &[u8]) -> Subrecord {
    Subrecord { sig: *sig, data: data.to_vec(), extended: false }
}

fn record(sig: &[u8; 4], fid: u32, subs: &[Subrecord]) -> Vec<u8> {
    let payload = write_subrecords(subs);
    [sig.as_slice(), &(payload.len() as u32).to_le_bytes(), &0u32.to_le_bytes(), &fid.to_le_bytes(), &[0; 4], &15u16.to_le_bytes(), &[0; 2], &payload].concat()
}

fn group(label: [u8; 4], gtype: i32, body: &[u8]) -> Vec<u8> {
    [b"GRUP".as_slice(), &((24 + body.len()) as u32).to_le_bytes(), &label, &gtype.to_le_bytes(), &[0; 8], body].concat()
}

/// A worldspace with exterior cells `(cell FormID, grid x, grid y, LAND FormID or 0)`.
fn world(fid: u32, edid: &str, extra: &[Subrecord], cells: &[(u32, i32, i32, u32)]) -> Vec<u8> {
    let mut subs = vec![sub(b"EDID", format!("{edid}\0").as_bytes())];
    subs.extend_from_slice(extra);
    let wrld = record(b"WRLD", fid, &subs);
    let mut body = Vec::new();
    for &(cell, x, y, land) in cells {
        let xclc = [x.to_le_bytes(), y.to_le_bytes(), 0u32.to_le_bytes()].concat();
        body.extend(record(b"CELL", cell, &[sub(b"DATA", &[0]), sub(b"XCLC", &xclc)]));
        if land != 0 {
            let land = record(b"LAND", land, &[sub(b"DATA", &[0; 4])]);
            body.extend(group(cell.to_le_bytes(), 6, &group(cell.to_le_bytes(), 9, &land)));
        }
    }
    let children = group(fid.to_le_bytes(), 1, &group([0; 4], 4, &group([0; 4], 5, &body)));
    [wrld, children].concat()
}

fn heights(land: f32, water: f32) -> Subrecord {
    sub(b"DNAM", &[land.to_le_bytes(), water.to_le_bytes()].concat())
}

fn child(parent_flags: u16) -> Vec<Subrecord> {
    vec![sub(b"WNAM", &PARENT.to_le_bytes()), sub(b"PNAM", &parent_flags.to_le_bytes())]
}

fn index() -> WorldIndex {
    let hedr = [1.34f32.to_le_bytes(), 0i32.to_le_bytes(), 0x800u32.to_le_bytes()].concat();
    let tes4 = record(b"TES4", 0, &[sub(b"HEDR", &hedr)]);
    let own = |f: &[Subrecord]| [f, &[sub(b"CNAM", &CLIMATE_C.to_le_bytes()), sub(b"NAM2", &WATER_C.to_le_bytes())]].concat();
    let worlds = [
        world(
            PARENT,
            "Parent",
            &[sub(b"CNAM", &CLIMATE_P.to_le_bytes()), sub(b"NAM2", &WATER_P.to_le_bytes()), heights(-2048.0, 10500.0)],
            &[(0x101, 0, 0, 0x111), (0x102, 1, 0, 0x112), (0x103, 2, 0, 0)],
        ),
        // Use Land Data and Use Map Data, no DNAM, its own climate and water (as Megaton
        // and Oasis): a square without LAND, one with its own, one where the parent has
        // none either, and one the parent has no cell for.
        world(SHARES_LAND, "SharesLand", &own(&child(parent_use::LAND | parent_use::MAP)), &[(0x201, 0, 0, 0), (0x202, 1, 0, 0x212), (0x203, 2, 0, 0), (0x204, 9, 9, 0)]),
        // Land, map, water, climate and image space, none of its own (as Paradise Falls).
        world(SHARES_ALL, "SharesAll", &child(0x3D), &[(0x301, 0, 0, 0)]),
        // Use Map Data only, with its own records (as every New Vegas child): untouched.
        world(MAP_ONLY, "MapOnly", &own(&[child(parent_use::MAP), vec![heights(1000.0, 0.0)]].concat()), &[(0x401, 0, 0, 0)]),
        // Use LOD Data.
        world(SHARES_LOD, "SharesLod", &own(&[child(parent_use::LOD | parent_use::MAP), vec![heights(12288.0, 0.0)]].concat()), &[(0x501, 0, 0, 0)]),
    ]
    .concat();
    let bytes = [tes4, group(*b"WRLD", 0, &worlds)].concat();
    let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let plugins = [Plugin::parse(&bytes, "Parents.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    WorldIndex::build(&lo, &schema)
}

#[test]
fn a_cell_without_land_takes_the_parents_on_the_same_square() {
    let ix = index();
    assert!(ix.errors.is_empty(), "{:?}", ix.errors);
    let cell = |id: u32| (ix.cells[&id].land, ix.cells[&id].land_of_parent);
    // The parent's own cells are its own.
    assert_eq!((cell(0x101), cell(0x102), cell(0x103)), ((Some(0x111), false), (Some(0x112), false), (None, false)));
    // Use Land Data: the square without LAND has the parent's; so does the one with its
    // own LAND (#3755); where the parent has no LAND, or no cell, there is none.
    assert_eq!(cell(0x201), (Some(0x111), true));
    assert_eq!(cell(0x202), (Some(0x112), true));
    assert_eq!(cell(0x203), (None, false));
    assert_eq!(cell(0x204), (None, false));
    assert_eq!(cell(0x301), (Some(0x111), true));
    // Without the flag nothing is taken, though the square matches.
    assert_eq!(cell(0x401), (None, false));
    assert_eq!(cell(0x501), (None, false));
}

#[test]
fn defaults_water_and_climate_follow_the_flags() {
    let ix = index();
    let w = |id: u32| &ix.worldspaces[&id];
    let heights = |id: u32| (w(id).default_land_height, w(id).default_water_height);
    // No DNAM with Use Land Data: the parent's heights. Its own water and climate stay.
    assert_eq!(heights(SHARES_LAND), (Some(-2048.0), Some(10500.0)));
    assert_eq!((w(SHARES_LAND).water, w(SHARES_LAND).climate), (Some(WATER_C), Some(CLIMATE_C)));
    // Water and climate flags, none of its own: the parent's.
    assert_eq!(heights(SHARES_ALL), (Some(-2048.0), Some(10500.0)));
    assert_eq!((w(SHARES_ALL).water, w(SHARES_ALL).climate), (Some(WATER_P), Some(CLIMATE_P)));
    // Map only, and LOD only: their own.
    assert_eq!(heights(MAP_ONLY), (Some(1000.0), Some(0.0)));
    assert_eq!((w(MAP_ONLY).water, w(MAP_ONLY).climate), (Some(WATER_C), Some(CLIMATE_C)));
    assert_eq!(heights(SHARES_LOD), (Some(12288.0), Some(0.0)));
    assert_eq!(heights(PARENT), (Some(-2048.0), Some(10500.0)));
}

#[test]
fn distant_lod_is_the_parents_with_use_lod_data() {
    let ix = index();
    assert_eq!(ix.lod_world(SHARES_LOD), PARENT);
    for own in [PARENT, SHARES_LAND, SHARES_ALL, MAP_ONLY, 0xDEAD] {
        assert_eq!(ix.lod_world(own), own);
    }
}
