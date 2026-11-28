//! The ground pack test aid (#3322). Every file is generated here, in a folder of the test's own.

use super::*;
use crate::{GameData, TextureCache};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use std::fs;
use std::sync::Arc;

fn temp_home(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("nvf-ground-pack-{tag}-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()))
}

#[test]
fn the_generated_height_map_is_a_valid_bc4_chain_with_the_full_range() {
    let bytes = ground_height_dds();
    assert_eq!(bytes, ground_height_dds(), "deterministic");
    let dds = fnv_dds::Dds::parse(&bytes).expect("a DDS file the engine reads");
    assert_eq!(dds.format, fnv_dds::Format::Bc4U, "unsigned BC4, as the pack rules ask of a _p");
    assert_eq!((dds.header.width, dds.header.height), (GROUND_SIDE, GROUND_SIDE));
    assert_eq!(GROUND_SIDE % 4, 0, "sides on the 4-texel grid");
    assert_eq!(dds.header.levels(), GROUND_SIDE.ilog2() + 1, "a full mip chain down to 1x1");
    assert_eq!(dds.levels[0].len(), dds.header.levels() as usize);
    let top = fnv_dds::decode_bc4(dds.format, GROUND_SIDE, GROUND_SIDE, dds.levels[0][0]).expect("level 0 decodes");
    let (min, max) = top.iter().fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
    assert_eq!((min, max), (0.0, 1.0), "the full 0..1 range");
    let low = top.iter().filter(|&&v| v == 0.0).count() as f32 / top.len() as f32;
    // An eighth of each direction raised: (7/8)^2 of the map low.
    assert!((low - 49.0 / 64.0).abs() < 1e-6, "mostly low, so a layer sinks as one: {low}");
    // The pattern by texel: the ridge rows and columns, every 64 texels, 8 wide.
    let at = |x: usize, y: usize| top[y * GROUND_SIDE as usize + x];
    assert_eq!((at(0, 30), at(7, 30), at(8, 30), at(30, 64), at(30, 72)), (1.0, 1.0, 0.0, 1.0, 0.0));
    // The last level is one value, the map's mean (15/64 raised), rounded to a byte.
    let last = dds.levels[0].last().unwrap();
    assert_eq!(last[0], (255.0f32 * 15.0 / 64.0).round() as u8);
}

#[test]
fn every_landscape_texture_gets_a_height_only_set_where_the_index_looks() {
    let diffuses = ["Landscape\\Dirt01.dds", "landscape/dirt01.dds", "data\\textures\\landscape\\Rocks02.dds", "landscape\\xbox\\Sand.dds", ""];
    let files = ground_fixture(GroundPack::Heights, &diffuses);
    assert_eq!(files, ground_fixture(GroundPack::Heights, &diffuses), "deterministic");
    let names: Vec<&str> = files.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(
        names,
        [
            "textures/pbr/landscape/dirt01_p.dds",
            "textures/pbr/landscape/dirt01.toml",
            "textures/pbr/landscape/pc/sand_p.dds",
            "textures/pbr/landscape/pc/sand.toml",
            "textures/pbr/landscape/rocks02_p.dds",
            "textures/pbr/landscape/rocks02.toml",
            "textures/pbr/landscape/xbox/sand_p.dds",
            "textures/pbr/landscape/xbox/sand.toml",
        ],
        "one set per texture (two spellings share one), the \\pc\\ twin of a console folder, nothing for an empty path"
    );
    // The index finds each as a height-only set by the diffuse's path.
    let index = fnv_pbr::PbrIndex::build(names.iter().copied());
    for d in ["textures\\landscape\\dirt01.dds", "textures\\landscape\\rocks02.dds", "textures\\landscape\\pc\\sand.dds"] {
        let found = index.lookup(d).unwrap_or_else(|| panic!("{d}"));
        assert_eq!(found.state(), fnv_pbr::SetState::HeightOnly, "{d}");
    }
    assert_eq!(index.counts().unmatched, 0, "every file is a set member");
    for (path, bytes) in &files {
        if path.ends_with(".toml") {
            let parsed = fnv_pbr::parse_scalars(bytes);
            assert!(!parsed.unreadable && parsed.warnings.is_empty(), "{path}: {:?}", parsed.warnings);
            assert_eq!(parsed.scalars.parallax_depth, Some(GROUND_DEPTH), "{path}");
        } else {
            assert_eq!(bytes, &ground_height_dds(), "{path}");
        }
    }
}

#[test]
fn the_terrain_lookup_takes_the_generated_height_map_at_the_set_depth() {
    // A load order of one diffuse, with the pack mounted over it as the game mounts it.
    let home = temp_home("lookup");
    let data_dir = home.join("Data");
    fs::create_dir_all(data_dir.join("textures").join("landscape")).unwrap();
    fs::write(data_dir.join("textures").join("landscape").join("dirt01.dds"), crate::authored::tests::legacy(b"DXT1", 4, 4, &[0xFF, 0xFF, 0, 0, 0, 0, 0, 0])).unwrap();
    let (pack, sets, written) = seed_ground_pack_in(&home, 3, 1, GroundPack::Heights, &["Landscape\\Dirt01.dds"]).unwrap();
    assert_eq!((sets, written), (1, 2));
    let mut vfs = fnv_vfs::Vfs::open(&data_dir, &[]).unwrap();
    vfs.add_loose_root(&pack);
    let data = GameData::new(Arc::new(vfs), Arc::new(fnv_nif::Schema::default()));
    let mut cache = TextureCache::default();
    let mut images = Assets::<Image>::default();
    let (height, depth) = cache.authored_height_with(true, "Landscape\\Dirt01.dds", &data, &mut images, None).expect("the generated set's _p");
    assert_eq!(depth, GROUND_DEPTH);
    let image = images.get(&height).unwrap();
    assert_eq!(image.texture_descriptor.format, TextureFormat::Bc4RUnorm);
    assert!(image.texture_descriptor.mip_level_count > 1, "the mip chain is uploaded");
    // Another texture has no set; with the setting off nothing is asked.
    assert!(cache.authored_height_with(true, "landscape\\other.dds", &data, &mut images, None).is_none());
    assert!(cache.authored_height_with(false, "Landscape\\Dirt01.dds", &data, &mut images, None).is_none());
    let _ = fs::remove_dir_all(&home);
}

/// #3486: a diffuse's alpha is a height only where the authored index knows no set for it.
#[test]
fn a_diffuse_alpha_is_a_height_only_without_an_authored_set() {
    let home = temp_home("alpha-height");
    let data_dir = home.join("Data");
    let land = data_dir.join("textures").join("landscape");
    fs::create_dir_all(&land).unwrap();
    for name in ["dirt01.dds", "sand01.dds"] {
        fs::write(land.join(name), ground_diffuse_dds(0)).unwrap();
    }
    // A set for dirt01 only: without a `_p`, a colour set that gives no height.
    let pbr = data_dir.join("textures").join("pbr").join("landscape");
    fs::create_dir_all(&pbr).unwrap();
    fs::write(pbr.join("dirt01_orm.dds"), ground_diffuse_dds(1)).unwrap();
    let data = GameData::new(Arc::new(fnv_vfs::Vfs::open(&data_dir, &[]).unwrap()), Arc::new(fnv_nif::Schema::default()));
    let mut cache = TextureCache::default();
    let mut images = Assets::<Image>::default();
    assert!(cache.alpha_height_with(true, "Landscape\\Dirt01.dds", &data, &mut images, None).is_none(), "the set decides: no _p, no height");
    assert!(cache.alpha_height_with(false, "landscape\\sand01.dds", &data, &mut images, None).is_none(), "the setting off");
    let (height, depth) = cache.alpha_height_with(true, "landscape\\sand01.dds", &data, &mut images, None).expect("a varying alpha and no set");
    assert_eq!(depth, crate::texture::ALPHA_HEIGHT_DEPTH);
    let image = images.get(&height).unwrap();
    assert_eq!((image.texture_descriptor.format, image.texture_descriptor.mip_level_count), (TextureFormat::Bc4RUnorm, GROUND_SIDE.ilog2() + 1));
    assert_eq!(cache.alpha_height_with(true, "Landscape\\Sand01.dds", &data, &mut images, None).unwrap().0, height, "made once");
    assert!(cache.failures.is_empty() && cache.alpha_height_with(true, "landscape\\none.dds", &data, &mut images, None).is_none());
    assert!(cache.failures.is_empty(), "nothing to report: the diffuse's own load does");
    let _ = fs::remove_dir_all(&home);
}

/// The pack writes only into a fresh folder of its own: a second run gets another one, files
/// already in the home are never touched, and nothing is written without a texture or a home.
#[test]
fn the_ground_pack_writes_only_a_fresh_folder_of_its_own() {
    let home = temp_home("fresh");
    let mine = home.join("mods").join("textures").join("pbr").join("landscape");
    fs::create_dir_all(&mine).unwrap();
    fs::write(mine.join("dirt01_p.dds"), b"the player's own").unwrap();
    let (first, ..) = seed_ground_pack_in(&home, 7, 1, GroundPack::Heights, &["landscape\\dirt01.dds"]).unwrap();
    let (second, ..) = seed_ground_pack_in(&home, 7, 1, GroundPack::Heights, &["landscape\\dirt01.dds"]).unwrap();
    assert_ne!(first, second, "a new folder per run");
    assert!(first.starts_with(home.join("pbr-ground-pack")));
    for (rel, bytes) in ground_fixture(GroundPack::Heights, &["landscape\\dirt01.dds"]) {
        assert_eq!(fs::read(first.join(&rel)).unwrap(), bytes, "{rel}");
    }
    assert_eq!(fs::read(mine.join("dirt01_p.dds")).unwrap(), b"the player's own", "the home's files untouched");
    assert!(seed_ground_pack_in::<&str>(&home, 8, 1, GroundPack::Heights, &[]).is_err(), "no texture: nothing written");
    assert!(!home.join("pbr-ground-pack").join("run-8-1-0").exists());
    assert!(ground_pack_home(None).is_err() && ground_pack_home(Some("".into())).is_err());
    assert_eq!(ground_pack_home(Some("h".into())).unwrap(), PathBuf::from("h"));
    // A file in the way inside a fresh folder cannot happen, but write_files refuses one.
    let taken = home.join("taken");
    let files = ground_fixture(GroundPack::Heights, &["landscape\\dirt01.dds"]);
    fs::create_dir_all(taken.join("textures").join("pbr").join("landscape")).unwrap();
    fs::write(taken.join(&files[0].0), b"there before").unwrap();
    assert!(write_files(&taken, &files).is_err());
    assert_eq!(fs::read(taken.join(&files[0].0)).unwrap(), b"there before", "never truncated");
    let _ = fs::remove_dir_all(&home);
}

/// A plugin's texture path is data: one with parent steps, a dot component or a colon gets no
/// set, so nothing is written outside the run's own folder, and the writer refuses any such
/// path before its first write.
#[test]
fn no_texture_path_can_write_outside_the_pack_folder() {
    let hostile = [r"..\..\..\escape.dds", r"landscape\..\..\..\..\escape.dds", r"landscape\.\dot.dds", r"landscape\c:\stream.dds", r"landscape\xbox\..\..\..\escape.dds"];
    for d in hostile {
        assert!(ground_fixture(GroundPack::Heights, &[d]).is_empty(), "{d}: {:?}", ground_fixture(GroundPack::Heights, &[d]).iter().map(|f| &f.0).collect::<Vec<_>>());
    }
    // Mixed with a plain texture, only the plain one is written.
    let mixed = ground_fixture(GroundPack::Heights, &[r"..\..\escape.dds", r"landscape\dirt01.dds"]);
    assert_eq!(mixed.iter().map(|f| f.0.as_str()).collect::<Vec<_>>(), ["textures/pbr/landscape/dirt01_p.dds", "textures/pbr/landscape/dirt01.toml"]);
    let home = temp_home("contain");
    // The home exists, as the game's NVFOREVER_HOME does (the pack makes only its own folder).
    fs::create_dir_all(&home).unwrap();
    let (dir, sets, _) = seed_ground_pack_in(&home, 9, 1, GroundPack::Heights, &[r"..\..\..\..\..\escape.dds", r"landscape\dirt01.dds"]).unwrap();
    assert_eq!(sets, 1);
    let written: Vec<PathBuf> = walk(&home);
    assert!(written.iter().all(|p| p.starts_with(&dir)), "every file inside the run's folder: {written:?}");
    assert!(seed_ground_pack_in(&home, 10, 1, GroundPack::Heights, &[r"..\..\escape.dds"]).is_err(), "nothing safe to write: refused");
    // The writer's own check, before any write.
    let out = home.join("guarded");
    fs::create_dir_all(&out).unwrap();
    for bad in ["../escape_p.dds", "textures/../../escape_p.dds", "/abs_p.dds", "c:/drive_p.dds", "textures//empty_p.dds", "./dot_p.dds"] {
        let files = vec![("textures/pbr/ok_p.dds".to_string(), b"x".to_vec()), (bad.to_string(), b"x".to_vec())];
        assert!(write_files(&out, &files).is_err(), "{bad}");
        assert!(!out.join("textures").exists(), "{bad}: nothing written before the refusal");
    }
    assert!(!home.parent().unwrap().join("escape_p.dds").exists());
    let _ = fs::remove_dir_all(&home);
}

/// #3789: the alpha pack's diffuses: where the game's own lie, each of one flat colour of its
/// own with the checkerboard alpha, over the same height sets.
#[test]
fn the_alpha_pack_replaces_every_diffuse_with_a_flat_colour_and_a_checkerboard_alpha() {
    let diffuses = ["Landscape\\Dirt01.dds", "landscape\\Rocks02.dds"];
    let names: Vec<String> = ground_fixture(GroundPack::Alpha, &diffuses).into_iter().map(|f| f.0).collect();
    assert_eq!(
        names,
        [
            "textures/pbr/landscape/dirt01_p.dds",
            "textures/pbr/landscape/dirt01.toml",
            "textures/landscape/dirt01.dds",
            "textures/landscape/dirt01_n.dds",
            "textures/pbr/landscape/rocks02_p.dds",
            "textures/pbr/landscape/rocks02.toml",
            "textures/landscape/rocks02.dds",
            "textures/landscape/rocks02_n.dds",
        ]
    );
    let mut colours = Vec::new();
    for (_, bytes) in ground_fixture(GroundPack::Alpha, &diffuses).iter().filter(|f| f.0.starts_with("textures/landscape/") && !f.0.ends_with("_n.dds")) {
        let dds = fnv_dds::Dds::parse(bytes).expect("a DDS file the engine reads");
        assert_eq!((dds.header.width, dds.header.height, dds.header.levels()), (GROUND_SIDE, GROUND_SIDE, GROUND_SIDE.ilog2() + 1));
        let top = fnv_dds::decode_blocks(dds.format, GROUND_SIDE, GROUND_SIDE, dds.levels[0][0]).expect("level 0 decodes");
        let px: Vec<[u8; 4]> = top.as_chunks::<4>().0.to_vec();
        assert!(px.iter().all(|p| p[..3] == px[0][..3]), "one colour");
        let at = |x: usize, y: usize| px[y * GROUND_SIDE as usize + x][3];
        assert_eq!((at(0, 0), at(31, 31), at(32, 0), at(0, 32), at(32, 32)), (255, 255, 0, 0, 255));
        colours.push(px[0]);
    }
    assert_ne!(colours[0], colours[1], "a colour of its own each");
    let home = temp_home("alpha");
    fs::create_dir_all(&home).unwrap();
    let (_, sets, files) = seed_ground_pack_in(&home, 11, 1, GroundPack::Alpha, &diffuses).unwrap();
    assert_eq!((sets, files), (2, 8));
    let _ = fs::remove_dir_all(&home);
    // The alpha-height pack (#3486): the diffuses and normal maps alone, the height map in
    // the diffuse's alpha, the colour in two shades.
    let files = ground_fixture(GroundPack::AlphaHeight, &diffuses);
    assert_eq!(files.iter().map(|f| f.0.as_str()).collect::<Vec<_>>(), ["textures/landscape/dirt01.dds", "textures/landscape/dirt01_n.dds", "textures/landscape/rocks02.dds", "textures/landscape/rocks02_n.dds"]);
    let dds = fnv_dds::Dds::parse(&files[0].1).unwrap();
    let top = fnv_dds::decode_blocks(dds.format, GROUND_SIDE, GROUND_SIDE, dds.levels[0][0]).unwrap();
    let px = top.as_chunks::<4>().0;
    assert_eq!((px[0][3], px[30 * GROUND_SIDE as usize + 30][3]), (255, 0), "a ridge and the ground between");
    let shades: std::collections::HashSet<[u8; 3]> = px.iter().map(|p| [p[0], p[1], p[2]]).collect();
    assert_eq!(shades.len(), 2);
    let named = |v: &str| ground_pack_named(std::ffi::OsStr::new(v));
    assert_eq!((named("1"), named("alpha"), named("alpha-height")), (Some(GroundPack::Heights), Some(GroundPack::Alpha), Some(GroundPack::AlphaHeight)));
    assert_eq!((named("0"), named("")), (None, None));
}

/// Every file under `dir`.
fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}
