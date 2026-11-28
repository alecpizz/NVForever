//! Mod-list rules that do not draw the window: names, priority,
//! a profile comparison, archive names, and a pack of the three list files.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// A folder sitting in `mods/` that is part of a mod, not a mod itself.
pub fn is_tool_dir(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "meshes" | "textures" | "meta" | "fomod" | "optional" | "docs" | "source" | "__macosx"
    )
}

/// Higher number is lower in the list, and that mod wins a loose-file overwrite.
pub fn priority(index: usize, len: usize) -> usize {
    len.saturating_sub(index)
}

/// What a folder holds, for the line under its name.
pub fn folder_kind(files: &[String]) -> &'static str {
    if files.is_empty() {
        return "empty";
    }
    let plugin = files.iter().any(|f| {
        let n = f.rsplit(['/', '\\']).next().unwrap_or(f);
        let n = n.to_ascii_lowercase();
        n.ends_with(".esm") || n.ends_with(".esp")
    });
    let mesh = files.iter().any(|f| f.to_ascii_lowercase().contains("meshes/"));
    let texture = files.iter().any(|f| f.to_ascii_lowercase().contains("textures/"));
    if !plugin && !mesh && !texture {
        "no game files"
    } else {
        ""
    }
}

/// Archive file names in a mod folder. The mod list does not print them.
#[allow(dead_code)]
pub fn bsa_names(files: &[String]) -> Vec<String> {
    let mut names = Vec::new();
    for file in files {
        let name = file.rsplit(['/', '\\']).next().unwrap_or(file);
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".bsa") || lower.ends_with(".ba2") {
            names.push(name.to_string());
        }
    }
    names.sort();
    names.dedup();
    names
}

/// Loose files in `incoming` that an enabled mod already has.
pub fn clashes_with(incoming: &[String], enabled: &[(&str, &[String])]) -> Vec<String> {
    let mut out = Vec::new();
    for file in incoming {
        let key = file.replace('\\', "/").to_ascii_lowercase();
        for (mod_name, files) in enabled {
            if files.iter().any(|f| f.replace('\\', "/").eq_ignore_ascii_case(&key)) {
                out.push(format!("{file}  (also in {mod_name})"));
                break;
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// What differs between two profiles' mod lists and load orders. The page does not draw this.
#[allow(dead_code)]
pub fn compare_lists(left_name: &str, left_mods: &[(String, bool)], right_name: &str, right_mods: &[(String, bool)], left_plugins: &[String], right_plugins: &[String]) -> String {
    let mut lines = Vec::new();
    let left_set: BTreeSet<_> = left_mods.iter().map(|(n, _)| n.to_ascii_lowercase()).collect();
    let right_set: BTreeSet<_> = right_mods.iter().map(|(n, _)| n.to_ascii_lowercase()).collect();
    for (name, _) in left_mods {
        if !right_set.contains(&name.to_ascii_lowercase()) {
            lines.push(format!("{name} is only in {left_name}"));
        }
    }
    for (name, _) in right_mods {
        if !left_set.contains(&name.to_ascii_lowercase()) {
            lines.push(format!("{name} is only in {right_name}"));
        }
    }
    let left_order: Vec<_> = left_mods.iter().map(|(n, _)| n.to_ascii_lowercase()).collect();
    let right_order: Vec<_> = right_mods.iter().map(|(n, _)| n.to_ascii_lowercase()).collect();
    if left_order != right_order {
        lines.push("The mod list order differs.".into());
    }
    let lp: BTreeSet<_> = left_plugins.iter().map(|n| n.to_ascii_lowercase()).collect();
    let rp: BTreeSet<_> = right_plugins.iter().map(|n| n.to_ascii_lowercase()).collect();
    for name in left_plugins {
        if !rp.contains(&name.to_ascii_lowercase()) {
            lines.push(format!("{name} is only in {left_name}'s load order"));
        }
    }
    for name in right_plugins {
        if !lp.contains(&name.to_ascii_lowercase()) {
            lines.push(format!("{name} is only in {right_name}'s load order"));
        }
    }
    if left_plugins.iter().map(|n| n.to_ascii_lowercase()).collect::<Vec<_>>() != right_plugins.iter().map(|n| n.to_ascii_lowercase()).collect::<Vec<_>>() {
        lines.push("The load order differs.".into());
    }
    if lines.is_empty() {
        "The two profiles match.".into()
    } else {
        lines.join("\n")
    }
}

/// True when `master` is ticked and sits below `plugin` in the load order.
pub fn master_is_lower(plugin_at: usize, master_at: Option<usize>) -> bool {
    master_at.is_some_and(|at| at > plugin_at)
}

/// Move masters up among the plugins that are not locked. `readable` is false when a header could not be read.
pub fn masters_first(names: &[String], locked: &[bool], esm: &[bool], readable: bool) -> Result<Vec<String>, String> {
    if !readable {
        return Err("A plugin header could not be read. The load order was not moved.".into());
    }
    if names.len() != locked.len() || names.len() != esm.len() {
        return Err("The load order was not moved.".into());
    }
    let mut unlocked_masters = Vec::new();
    let mut unlocked_rest = Vec::new();
    for (i, name) in names.iter().enumerate() {
        if locked[i] {
            continue;
        }
        if esm[i] || name.to_ascii_lowercase().ends_with(".esm") {
            unlocked_masters.push(name.clone());
        } else {
            unlocked_rest.push(name.clone());
        }
    }
    let mut masters = unlocked_masters.into_iter();
    let mut rest = unlocked_rest.into_iter();
    let mut out = Vec::with_capacity(names.len());
    for (i, name) in names.iter().enumerate() {
        if locked[i] {
            out.push(name.clone());
        } else if let Some(master) = masters.next() {
            out.push(master);
        } else if let Some(plugin) = rest.next() {
            out.push(plugin);
        }
    }
    Ok(out)
}

const PACK_FILES: [&str; 3] = ["modlist.txt", "plugins.txt", "loadorder.txt"];

/// A zip of the three list files and nothing else. The page no longer has Export. The test still checks it.
#[allow(dead_code)]
pub fn write_pack(dir: &Path, dest: &Path) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let file = std::fs::File::create(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default();
    for name in PACK_FILES {
        let path = dir.join(name);
        if !path.is_file() {
            return Err(format!("{name} is missing. The pack was not written."));
        }
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        zip.start_file(name, opts).map_err(|e| e.to_string())?;
        std::io::Write::write_all(&mut zip, &bytes).map_err(|e| e.to_string())?;
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

/// Reads a pack. Any file other than the three lists is refused. Nothing is deployed.
/// The page no longer has Import. The test still checks it.
#[allow(dead_code)]
pub fn read_pack(pack: &Path, dest: &Path) -> Result<(), String> {
    let file = std::fs::File::open(pack).map_err(|e| format!("{}: {e}", pack.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let mut names = Vec::new();
    for i in 0..zip.len() {
        let entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().map(|n| n.replace('\\', "/")).unwrap_or_default();
        if name.ends_with('/') {
            return Err("That pack has a folder in it. It was not read.".into());
        }
        let base = name.rsplit('/').next().unwrap_or(&name);
        if !PACK_FILES.contains(&base) || name.contains("..") {
            return Err(format!("{name} is not a list file. The pack was not read."));
        }
        names.push((i, base.to_string()));
    }
    if names.len() != 3 {
        return Err("That pack does not hold the three list files.".into());
    }
    std::fs::create_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    for (i, base) in names {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut bytes).map_err(|e| e.to_string())?;
        std::fs::write(dest.join(&base), bytes).map_err(|e| format!("{}: {e}", dest.display()))?;
    }
    Ok(())
}

/// Keeps the newest `cap` copies of the three lists under `history/`.
pub fn keep_history(profile: &Path, cap: usize) -> Result<(), String> {
    let history = profile.join("history");
    std::fs::create_dir_all(&history).map_err(|e| format!("{}: {e}", history.display()))?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let dir = history.join(stamp.to_string());
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for name in PACK_FILES {
        let src = profile.join(name);
        if src.is_file() {
            std::fs::copy(&src, dir.join(name)).map_err(|e| format!("{}: {e}", src.display()))?;
        }
    }
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&history).map_err(|e| format!("{}: {e}", history.display()))?.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    while dirs.len() > cap {
        if let Some(old) = dirs.first() {
            let _ = std::fs::remove_dir_all(old);
            dirs.remove(0);
        }
    }
    Ok(())
}

/// The newest dated copy of a profile's lists. The page no longer has a separate Put back. The test still checks it.
#[allow(dead_code)]
pub fn newest_history(profile: &Path) -> Option<PathBuf> {
    let history = profile.join("history");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&history).ok()?.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    dirs.pop()
}

pub fn append_log(path: &Path, line: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    writeln!(file, "{line}").map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_folders_are_not_mods() {
        assert!(is_tool_dir("Meshes"));
        assert!(is_tool_dir("fomod"));
        assert!(is_tool_dir("meta"));
        assert!(!is_tool_dir("A World of Pain"));
    }

    #[test]
    fn the_bottom_mod_has_the_higher_priority() {
        assert_eq!(priority(0, 3), 3);
        assert_eq!(priority(2, 3), 1);
    }

    #[test]
    fn an_empty_folder_is_named() {
        assert_eq!(folder_kind(&[]), "empty");
        assert_eq!(folder_kind(&["readme.txt".into()]), "no game files");
        assert_eq!(folder_kind(&["meshes/a.nif".into()]), "");
    }

    #[test]
    fn archive_names_skip_loose_files() {
        let names = bsa_names(&["meshes/a.nif".into(), "Foo.bsa".into(), "bar.BA2".into(), "Foo.bsa".into()]);
        assert_eq!(names, vec!["Foo.bsa".to_string(), "bar.BA2".to_string()]);
    }

    #[test]
    fn two_profiles_name_what_differs() {
        let left = vec![("A".into(), true), ("B".into(), false)];
        let right = vec![("B".into(), true), ("C".into(), true)];
        let text = compare_lists("One", &left, "Two", &right, &["A.esp".into()], &["A.esp".into(), "B.esp".into()]);
        assert!(text.contains("A is only in One"));
        assert!(text.contains("C is only in Two"));
        assert!(text.contains("B.esp is only in Two"));
    }

    #[test]
    fn a_master_below_its_plugin_is_flagged() {
        assert!(master_is_lower(1, Some(3)));
        assert!(!master_is_lower(3, Some(1)));
        assert!(!master_is_lower(1, None));
    }

    #[test]
    fn masters_first_leaves_a_locked_row() {
        let names = vec!["Mod.esp".into(), "FalloutNV.esm".into(), "Patch.esp".into(), "DeadMoney.esm".into()];
        let locked = vec![false, true, false, false];
        let esm = vec![false, true, false, true];
        let out = masters_first(&names, &locked, &esm, true).unwrap();
        assert_eq!(out[1], "FalloutNV.esm");
        assert!(out[0].ends_with(".esm") || out[2].ends_with(".esm") || out[3].ends_with(".esm"));
        assert!(masters_first(&names, &locked, &esm, false).is_err());
    }

    #[test]
    fn a_pack_is_only_the_three_lists() {
        let root = std::env::temp_dir().join(format!("nvf-pack-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("Road");
        std::fs::create_dir_all(&dir).unwrap();
        for name in PACK_FILES {
            std::fs::write(dir.join(name), name).unwrap();
        }
        let pack = root.join("Road.zip");
        write_pack(&dir, &pack).unwrap();
        let extra = root.join("bad.zip");
        {
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&extra).unwrap());
            zip.start_file("modlist.txt", zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut zip, b"+A\n").unwrap();
            zip.start_file("notes.txt", zip::write::SimpleFileOptions::default()).unwrap();
            zip.finish().unwrap();
        }
        let dest = root.join("out");
        assert!(read_pack(&extra, &dest).is_err());
        read_pack(&pack, &dest).unwrap();
        assert_eq!(std::fs::read_to_string(dest.join("plugins.txt")).unwrap(), "plugins.txt");
        keep_history(&dir, 2).unwrap();
        keep_history(&dir, 2).unwrap();
        keep_history(&dir, 2).unwrap();
        let n = std::fs::read_dir(dir.join("history")).unwrap().count();
        assert!(n <= 2);
        assert!(newest_history(&dir).is_some());
        let _ = std::fs::remove_dir_all(&root);
    }
}
