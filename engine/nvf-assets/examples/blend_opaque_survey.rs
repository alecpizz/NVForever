//! Survey (#3230): every shape in the named archives that asks to be blended, and how many
//! of them `nif_spawn::blend_is_opaque` draws opaque (blend on, nothing that can thin it:
//! an opaque texture, material alpha 1, no vertex alpha, ...). Prints the totals, the
//! split by alpha test, the shapes kept blended only by a window environment map, and the
//! models the rule takes, one line each.
//!
//! cargo run --release -p nvf-assets --example blend_opaque_survey -- <Data dir> <nif schema dir> <archive>...
//!
//! Name the archives: the base game's only for a parity survey (parity-lab/AGENT.md), the
//! textures archives too, or every texture reads as missing and nothing is taken.

use fnv_vfs::Vfs;
use nvf_assets::data::GameData;
use nvf_assets::nif_spawn::blend_opaque_survey;
use nvf_assets::texture::{alpha_all_opaque, resolved_path};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 4 {
        eprintln!("usage: blend_opaque_survey <Data dir> <nif schema dir> <archive>...");
        std::process::exit(2);
    }
    let vfs = Vfs::open(a[1].as_ref(), &a[3..]).expect("archives");
    let schema = fnv_nif::Schema::load_dir(a[2].as_ref()).expect("schema");
    let data = GameData::new(Arc::new(vfs), Arc::new(schema));
    let opaque_cache: Mutex<HashMap<String, bool>> = Mutex::default();
    let opaque = |raw: &str| {
        let path = resolved_path(raw, |p| data.vfs.exists(p));
        if let Some(&o) = opaque_cache.lock().unwrap().get(&path) {
            return o;
        }
        let o = data.read(&path).and_then(|b| alpha_all_opaque(&b)).unwrap_or(false);
        opaque_cache.lock().unwrap().insert(path, o);
        o
    };
    let models = data.vfs.paths(|k| k.ends_with(".nif"));
    let (mut files, mut blended, mut taken, mut taken_untested, mut window_only) = (0usize, 0usize, 0usize, 0usize, 0usize);
    let mut files_taken: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut textures_taken: BTreeMap<String, usize> = BTreeMap::new();
    for path in &models {
        let Some(bytes) = data.read(path) else { continue };
        files += 1;
        for s in blend_opaque_survey(&bytes, &data.schema, opaque) {
            blended += 1;
            if s.drawn_opaque {
                taken += 1;
                taken_untested += usize::from(!s.tested);
                *textures_taken.entry(s.texture.to_ascii_lowercase()).or_default() += 1;
                files_taken.entry(path.clone()).or_default().push(s.shape);
            }
            window_only += usize::from(s.only_window_env);
        }
    }
    println!("models read: {files} of {}", models.len());
    println!("shapes drawn blended by their NiAlphaProperty: {blended}");
    println!("of those drawn opaque by the rule: {taken} shapes in {} models ({taken_untested} with the alpha test off, {} with it on)", files_taken.len(), taken - taken_untested);
    println!("kept blended only by a window environment map: {window_only}");
    println!("distinct base textures taken: {}", textures_taken.len());
    for (file, shapes) in &files_taken {
        println!("  {file}: {}", shapes.join(", "));
    }
}
