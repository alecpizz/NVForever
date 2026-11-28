//! Diagnostic survey of the shapes the baked shadow patch rule takes
//! (docs/spec-notes/nif.md §6 "Vertex colours", nvf-assets `nif_spawn::baked_shadow_patch`):
//! visible triangle shapes with a BSShaderNoLightingProperty that names no texture and
//! vertex colours black on every vertex with an alpha that varies. For each: whether a
//! NiAlphaProperty blends it SRC_ALPHA / INV_SRC_ALPHA (the rule's shapes; the others are
//! listed apart), how it lies in the model (the share of its triangles' area that faces up,
//! the model's z), its size and its alpha range, by folder.
//!
//! cargo run --release -p fnv-nif --example baked_shadow_survey -- <schema dir> <archive.bsa>...

use fnv_nif::scene::{mesh_data, scene_objects, Decoded};
use fnv_nif::{NifFile, Schema};
use std::collections::{BTreeMap, HashMap};

/// The largest colour channel a shadow patch's vertex may hold (nif_spawn `SHADOW_BLACK`).
const SHADOW_BLACK: f32 = 0.02;

fn clean(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        1.0
    }
}

fn rotate(r: &[f32; 9], v: [f32; 3]) -> [f32; 3] {
    [r[0] * v[0] + r[1] * v[1] + r[2] * v[2], r[3] * v[0] + r[4] * v[1] + r[5] * v[2], r[6] * v[0] + r[7] * v[1] + r[8] * v[2]]
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let schema = Schema::load_dir(std::path::Path::new(&a[1])).unwrap();
    // (taken by the rule, lies flat) -> folder -> lines.
    let mut rows: BTreeMap<(bool, bool), BTreeMap<String, Vec<String>>> = BTreeMap::new();
    for arc in &a[2..] {
        let mut bsa = fnv_bsa::Archive::open(arc).unwrap();
        let entries: Vec<(String, fnv_bsa::FileRecord)> =
            bsa.entries().map(|e| (e.path(), e.record.clone())).filter(|(p, _)| p.ends_with(".nif")).collect();
        for (path, rec) in entries {
            let Ok(data) = bsa.extract(&rec) else { continue };
            let Ok(file) = NifFile::parse(&data, Some(&schema)) else { continue };
            let dec = Decoded::new(&file, &data, &schema);
            let objects = scene_objects(&file, &dec);
            let by_block: HashMap<usize, _> = objects.iter().map(|o| (o.block, o)).collect();
            for o in objects.iter().filter(|o| !o.is_node) {
                let mut chain = vec![o.block];
                let mut cur = o.parent;
                while let Some(p) = cur {
                    if chain.len() > 512 {
                        break;
                    }
                    chain.push(p);
                    cur = by_block.get(&p).and_then(|x| x.parent);
                }
                let hidden = chain.iter().any(|&b| {
                    let s = dec.blocks[b].as_ref().ok();
                    let name = s.and_then(|s| s.text("name")).unwrap_or("");
                    s.and_then(|s| s.int("flags")).unwrap_or(0) & 1 != 0
                        || (name.len() >= 12 && name[..12].eq_ignore_ascii_case("editormarker"))
                });
                if hidden {
                    continue;
                }
                // Nearest alpha and shader properties (the shape's own win over ancestors').
                let (mut alpha, mut shader) = (None, None);
                for &b in chain.iter().rev() {
                    for &r in dec.blocks[b].as_ref().ok().and_then(|s| s.refs("properties")).unwrap_or_default() {
                        let Ok(r) = usize::try_from(r) else { continue };
                        let Ok(s) = dec.blocks[r].as_ref() else { continue };
                        let ty = file.blocks[r].type_name.as_str();
                        if ty == "NiAlphaProperty" {
                            alpha = Some(s.int("flags").unwrap_or(0));
                        } else if dec.is_a(r, "BSShaderProperty") {
                            let textured = ty != "BSShaderNoLightingProperty" || s.text("file_name").is_some_and(|t| !t.is_empty());
                            shader = Some((ty.to_string(), s.int("shader_flags").unwrap_or(0), textured));
                        }
                    }
                }
                let Some((ty, shader_flags, textured)) = shader else { continue };
                if ty != "BSShaderNoLightingProperty" || textured {
                    continue;
                }
                let Some(d) = dec.blocks[o.block].as_ref().ok().and_then(|s| s.block_ref("data")).and_then(|r| dec.get(r)) else {
                    continue;
                };
                let mesh = mesh_data(d);
                let colours: Vec<[f32; 4]> = mesh.colors.iter().map(|c| [clean(c[0]), clean(c[1]), clean(c[2]), clean(c[3])]).collect();
                let black = !colours.is_empty() && colours.iter().all(|c| c[..3].iter().all(|&v| v <= SHADOW_BLACK));
                if !black || !colours.iter().any(|c| c[3] < 1.0) || !colours.iter().any(|c| c[3] > 0.0) {
                    continue;
                }
                let blends = alpha.is_some_and(|f| f & 1 != 0 && (f >> 1) & 15 == 6 && (f >> 5) & 15 == 7);
                // The model's rotation down to the shape (parents' applied last).
                let turn = |mut v: [f32; 3]| {
                    for &b in &chain {
                        if let Some(x) = by_block.get(&b) {
                            v = rotate(&x.transform.rotation, v);
                        }
                    }
                    v
                };
                let (mut up, mut area) = (0.0f32, 0.0f32);
                for t in &mesh.triangles {
                    let p = |i: u32| mesh.positions.get(i as usize).copied().unwrap_or([0.0; 3]);
                    let (p0, p1, p2) = (p(t[0]), p(t[1]), p(t[2]));
                    let (u, v) = ([p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]], [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]]);
                    let n = turn([u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]);
                    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                    area += len;
                    up += n[2].abs();
                }
                let flat = if area > 0.0 { up / area } else { 0.0 };
                let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
                for p in &mesh.positions {
                    for k in 0..3 {
                        (lo[k], hi[k]) = (lo[k].min(p[k]), hi[k].max(p[k]));
                    }
                }
                let size = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
                let (amin, amax) = colours.iter().fold((1.0f32, 0.0f32), |(lo, hi), c| (lo.min(c[3]), hi.max(c[3])));
                let folder = path.rsplit_once('\\').map_or("", |(f, _)| f).to_string();
                rows.entry((blends, flat >= 0.9)).or_default().entry(folder).or_default().push(format!(
                    "{} :: {:?}: {} vertices, faces up {:.0}%, size {:.0} x {:.0} x {:.0}, alpha {amin:.2}..{amax:.2}, alpha flags {}, shader flags {:#x}{}",
                    path.rsplit_once('\\').map_or(path.as_str(), |(_, f)| f),
                    o.name.as_deref().unwrap_or(""),
                    colours.len(),
                    flat * 100.0,
                    size[0],
                    size[1],
                    size[2],
                    alpha.map_or("none".to_string(), |f| format!("{f:#x}")),
                    shader_flags,
                    if shader_flags & (1 << 3) != 0 { " (Vertex_Alpha)" } else { "" }
                ));
            }
        }
    }
    for ((blends, flat), folders) in &rows {
        let shapes: usize = folders.values().map(Vec::len).sum();
        println!(
            "\n== {} / {}: {shapes} shapes in {} folders",
            if *blends { "blends SRC_ALPHA / INV_SRC_ALPHA (the rule's)" } else { "no such blend (not taken)" },
            if *flat { "lies flat (90% or more of its area faces up or down)" } else { "does not lie flat" },
            folders.len()
        );
        for (folder, lines) in folders {
            println!("{folder}: {}", lines.len());
            for l in lines.iter().take(if std::env::var_os("SURVEY_ALL").is_some() { usize::MAX } else if *flat && *blends { 2 } else { 12 }) {
                println!("    {l}");
            }
        }
    }
}
