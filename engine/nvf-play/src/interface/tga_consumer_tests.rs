//! Actual CPU Pictures -> GameData/VFS -> Assets<Image> controls; no renderer/app.
use super::*;
use bevy::render::render_resource::TextureFormat;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

fn tga(kind: u8, width: u16, height: u16, bits: u8, descriptor: u8, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0, 0, kind, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    bytes.extend(width.to_le_bytes());
    bytes.extend(height.to_le_bytes());
    bytes.extend([bits, descriptor]);
    bytes.extend(payload);
    bytes
}

// Same authored header layout as nvf-assets' own DDS texture tests.
fn dds(mips: u32, blocks: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0; 128];
    bytes[..4].copy_from_slice(b"DDS ");
    for (at, value) in [
        (4, 124u32),
        (8, 0x1007),
        (12, 4),
        (16, 4),
        (28, mips),
        (76, 32),
        (80, 4),
        (108, 0x1000),
    ] {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[84..88].copy_from_slice(b"DXT1");
    bytes.extend(blocks);
    bytes
}

struct OwnedFiles {
    root: PathBuf,
    files: Vec<PathBuf>,
}

impl OwnedFiles {
    fn new(entries: &[(&str, Vec<u8>)]) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "nvf-owned-ui-tga-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(root.join("textures")).unwrap();
        let mut files = Vec::new();
        for (name, bytes) in entries {
            assert!(!name.contains(['/', '\\']) && !name.is_empty());
            let path = root.join("textures").join(name);
            std::fs::write(&path, bytes).unwrap();
            files.push(path);
        }
        Self { root, files }
    }

    fn data(&self) -> GameData {
        let vfs = fnv_vfs::Vfs::open(&self.root, &[]).unwrap();
        let schema_root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/nif");
        let schema = fnv_nif::Schema::load_dir(&schema_root).unwrap();
        GameData::new(Arc::new(vfs), Arc::new(schema))
    }
}

impl Drop for OwnedFiles {
    fn drop(&mut self) {
        // Only individually tracked authored inputs and the two owned directories;
        // no recursive delete, path enumeration, game folder or external assets.
        for path in &self.files {
            let _ = std::fs::remove_file(path);
        }
        let _ = std::fs::remove_dir(self.root.join("textures"));
        let _ = std::fs::remove_dir(&self.root);
    }
}

#[test]
fn corrupt_dds_and_recognizable_invalid_tga_remain_failures_without_image_assets() {
    let bad_dds = b"DDS ".to_vec();
    let bad_tga = tga(10, 1, 1, 32, 32, &[129, 1, 2, 3, 4]);
    let unknown = vec![0; 18];
    let decoder = nvf_assets::texture::dds_image;
    let expected = decoder(&bad_dds, true, true).expect_err("corrupt DDS");
    assert_eq!(
        decode_texture(&bad_dds, true, decoder).expect_err("DDS stays failed"),
        expected
    );
    assert!(decode_texture(&bad_tga, true, decoder)
        .expect_err("TGA stays failed")
        .starts_with("TGA UI image:"));
    let original_unknown = decoder(&unknown, true, true).expect_err("unknown original error");
    assert_eq!(
        decode_texture(&unknown, true, decoder).expect_err("unknown stays failed"),
        original_unknown
    );
    let files = OwnedFiles::new(&[
        ("bad_dds.dds", bad_dds),
        ("bad_tga.dds", bad_tga),
        ("unknown.dds", unknown),
    ]);
    let data = files.data();
    let mut pictures = Pictures::load(Some(&data));
    let mut images = Assets::<Image>::default();
    for _ in 0..2 {
        for name in ["bad_dds.dds", "bad_tga.dds", "unknown.dds"] {
            assert!(pictures
                .own_sized(name, Some(&data), &mut images, true)
                .is_none());
            assert_eq!(images.len(), 0, "no opaque placeholder accepted");
        }
    }
}

#[test]
fn original_dds_compression_mips_bc_fallback_and_font_decoder_are_preserved() {
    let block = [0, 248, 0, 0, 0, 0, 0, 0];
    let bytes = dds(2, &[block, block].concat());
    let normal = decode_texture(&bytes, true, nvf_assets::texture::dds_image).unwrap();
    assert_eq!(
        normal.texture_descriptor.format,
        TextureFormat::Bc1RgbaUnormSrgb
    );
    assert_eq!(normal.texture_descriptor.mip_level_count, 2);
    assert_eq!(
        normal.data.as_deref(),
        Some([block, block].concat().as_slice())
    );
    let top = decode_texture(&bytes, true, nvf_assets::texture::dds_image_top).unwrap();
    assert_eq!(
        top.texture_descriptor.format,
        TextureFormat::Bc1RgbaUnormSrgb
    );
    assert_eq!(top.texture_descriptor.mip_level_count, 1);
    assert_eq!(top.data.as_deref(), Some(block.as_slice()));
    // Android's DDS decoder now supports devices without BC sampling. The TGA
    // admission layer must preserve that exact decoded mip chain and colour intent.
    let fallback = decode_texture(&bytes, false, nvf_assets::texture::dds_image)
        .expect("DDS CPU fallback is preserved");
    assert_eq!(
        fallback.texture_descriptor.format,
        TextureFormat::Rgba8UnormSrgb
    );
    assert_eq!(fallback.texture_descriptor.mip_level_count, 2);
    assert_eq!(
        fallback.data.as_deref(),
        Some([255, 0, 0, 255].repeat(4 * 4 + 2 * 2).as_slice())
    );
    let files = OwnedFiles::new(&[("owned_bc.dds", bytes)]);
    let data = files.data();
    let mut images = Assets::<Image>::default();
    let mut pictures = Pictures::load(Some(&data));
    let (handle, _, size) = pictures
        .own_sized("owned_bc.dds", Some(&data), &mut images, true)
        .unwrap();
    assert_eq!(size, Vec2::new(4.0, 4.0));
    assert_eq!(
        images.get(&handle).unwrap().texture_descriptor.format,
        TextureFormat::Bc1RgbaUnormSrgb
    );
}
