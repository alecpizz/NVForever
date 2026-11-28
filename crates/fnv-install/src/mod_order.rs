//! The launcher's mod lists in Mod Organizer 2's `modlist.txt` form, so MO2 can read them
//! (docs/spec-notes/mod-manager.md, "The list files"; the form itself is
//! docs/spec-notes/mod-organizer-profile.md, `modlist.txt`).
//!
//! One line a row, the **highest priority first**: `+name` on, `-name` off, and a separator
//! as `-<name>_separator`. Locks, which MO2 has no mark for, are kept in a side file, one
//! name a line ([`locks_text`]). Rows are handed around lowest priority first, the page's
//! order and the order of the game's layers.
//!
//! The launcher's earlier forms are still read:
//! - the home `mod-order.txt` of before: lowest priority first, `*name` on, a bare name off,
//!   `=` in front of a locked row, `% name` a separator, `;` a comment ([`read_mod_order`]);
//! - a profile's `modlist.txt` of before: MO2's order and prefixes, with `=` and `% name`.

use std::collections::HashSet;

/// What MO2 adds to a separator's folder name [documented: mod-organizer-profile.md, a
/// separator is a mod line whose name ends `_separator`; observed: the MO2 profiles in
/// `mo2.rs`'s tests].
pub const SEPARATOR_SUFFIX: &str = "_separator";

/// One row of a mod list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModLine {
    /// A mod folder's name, or a separator's name without [`SEPARATOR_SUFFIX`].
    pub name: String,
    pub on: bool,
    pub separator: bool,
    pub locked: bool,
}

impl ModLine {
    pub fn module(name: impl Into<String>, on: bool) -> Self {
        Self { name: name.into(), on, separator: false, locked: false }
    }

    pub fn divider(name: impl Into<String>) -> Self {
        Self { name: name.into(), on: false, separator: true, locked: false }
    }
}

/// A name a list line can hold: not empty, no folder separator, and not a real folder that
/// would read back as a separator.
pub fn is_listable(name: &str) -> bool {
    !name.is_empty() && !name.contains('/') && !name.contains('\\') && !has_separator_suffix(name)
}

fn has_separator_suffix(name: &str) -> bool {
    name.len() > SEPARATOR_SUFFIX.len() && name.to_ascii_lowercase().ends_with(SEPARATOR_SUFFIX)
}

fn clean(line: &str) -> &str {
    line.trim_start_matches('\u{feff}').trim()
}

/// A profile's `modlist.txt` (MO2's form, or the launcher's earlier profile form), lowest
/// priority first. `*` lines (MO2's foreign mods: the game's own files) and comments are
/// skipped.
pub fn read_modlist(text: &str) -> Vec<ModLine> {
    let mut rows = Vec::new();
    for line in text.lines() {
        let t = clean(line);
        if t.is_empty() || t.starts_with('#') || t.starts_with(';') {
            continue;
        }
        if let Some(name) = old_separator(t) {
            rows.push(ModLine::divider(name));
            continue;
        }
        let (locked, t) = match t.strip_prefix('=') {
            Some(rest) => (true, rest.trim_start()),
            None => (false, t),
        };
        let (on, name) = if let Some(n) = t.strip_prefix('+') {
            (true, n)
        } else if let Some(n) = t.strip_prefix('-') {
            (false, n)
        } else {
            continue;
        };
        if let Some(row) = row_of(name, on, locked) {
            rows.push(row);
        }
    }
    rows.reverse();
    rows
}

/// The launcher home's `mod-order.txt`, lowest priority first: MO2's form, or the
/// launcher's earlier form (told apart by its `;` comment, a `*` line or a bare name).
pub fn read_mod_order(text: &str) -> Vec<ModLine> {
    if is_old_mod_order(text) { read_old_mod_order(text) } else { read_modlist(text) }
}

fn is_old_mod_order(text: &str) -> bool {
    text.lines().map(clean).filter(|t| !t.is_empty() && !t.starts_with('#')).any(|t| {
        if t.starts_with(';') {
            return true;
        }
        let t = t.strip_prefix('=').map_or(t, str::trim_start);
        !(t.starts_with('+') || t.starts_with('-') || t.starts_with('%'))
    })
}

fn read_old_mod_order(text: &str) -> Vec<ModLine> {
    let mut rows = Vec::new();
    for line in text.lines() {
        let t = clean(line);
        if t.is_empty() || t.starts_with(';') || t.starts_with('#') {
            continue;
        }
        if let Some(name) = old_separator(t) {
            rows.push(ModLine::divider(name));
            continue;
        }
        let (locked, t) = match t.strip_prefix('=') {
            Some(rest) => (true, rest.trim_start()),
            None => (false, t),
        };
        let (on, name) = match t.strip_prefix('*') {
            Some(rest) => (true, rest.trim_start()),
            None => (false, t),
        };
        if is_listable(name) {
            rows.push(ModLine { name: name.to_string(), on, separator: false, locked });
        }
    }
    rows
}

/// `% name`, the launcher's earlier separator line.
fn old_separator(line: &str) -> Option<String> {
    let name = line.strip_prefix('%')?.trim();
    (!name.is_empty() && !name.contains('/') && !name.contains('\\')).then(|| name.to_string())
}

fn row_of(name: &str, on: bool, locked: bool) -> Option<ModLine> {
    if has_separator_suffix(name) {
        let base = &name[..name.len() - SEPARATOR_SUFFIX.len()];
        return (!base.contains('/') && !base.contains('\\')).then(|| ModLine::divider(base));
    }
    is_listable(name).then(|| ModLine { name: name.to_string(), on, separator: false, locked })
}

/// `rows` (lowest priority first) in MO2's `modlist.txt` form: highest priority first, CRLF
/// line ends as MO2 writes them [documented: mod-organizer-profile.md], no header line.
/// Rows whose name a line cannot hold are left out.
pub fn modlist_text(rows: &[ModLine]) -> String {
    let mut body = String::new();
    for row in rows.iter().rev() {
        if row.separator {
            if row.name.is_empty() || row.name.contains('/') || row.name.contains('\\') {
                continue;
            }
            body.push('-');
            body.push_str(&row.name);
            body.push_str(SEPARATOR_SUFFIX);
        } else {
            if !is_listable(&row.name) {
                continue;
            }
            body.push(if row.on { '+' } else { '-' });
            body.push_str(&row.name);
        }
        body.push_str("\r\n");
    }
    body
}

/// The locked mods' names, one a line, for the side file beside a list.
pub fn locks_text(rows: &[ModLine]) -> String {
    rows.iter().filter(|r| r.locked && !r.separator && is_listable(&r.name)).map(|r| format!("{}\r\n", r.name)).collect()
}

/// The names a lock side file holds. A missing file is no locks.
pub fn read_locks(text: &str) -> HashSet<String> {
    text.lines().map(clean).filter(|t| !t.is_empty() && !t.starts_with('#')).map(str::to_ascii_lowercase).collect()
}

/// `rows` with the locks of a side file added (names without case). A lock an earlier form
/// kept in the list itself stays.
pub fn apply_locks(rows: &mut [ModLine], locks: &HashSet<String>) {
    for row in rows.iter_mut().filter(|r| !r.separator) {
        if locks.contains(&row.name.to_ascii_lowercase()) {
            row.locked = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(rows: &[ModLine]) -> Vec<String> {
        rows.iter().map(|r| if r.separator { format!("[{}]", r.name) } else { format!("{}{}{}", if r.locked { "=" } else { "" }, if r.on { "+" } else { "-" }, r.name) }).collect()
    }

    #[test]
    fn writes_mo2_form_highest_first_and_reads_it_back() {
        let rows = vec![ModLine::module("Base", true), ModLine::divider("Patches"), ModLine { name: "Late".into(), on: false, separator: false, locked: true }];
        let text = modlist_text(&rows);
        assert_eq!(text, "-Late\r\n-Patches_separator\r\n+Base\r\n");
        // No `=`, `%` or `*`: only lines MO2 writes itself.
        assert!(text.lines().all(|l| l.starts_with('+') || l.starts_with('-')));
        let mut back = read_modlist(&text);
        assert_eq!(names(&back), ["+Base", "[Patches]", "-Late"]);
        apply_locks(&mut back, &read_locks(&locks_text(&rows)));
        assert_eq!(back, rows);
        // The home file reads the same way.
        assert_eq!(read_mod_order(&text), read_modlist(&text));
    }

    #[test]
    fn reads_the_earlier_home_mod_order() {
        let old = "; A star means this folder is on. = locks the row. % is a separator.\n*Base\n% Patches\n=*Locked\nOff\n";
        assert_eq!(names(&read_mod_order(old)), ["+Base", "[Patches]", "=+Locked", "-Off"]);
        // Without the comment, a `*` line or a bare name still says it is the earlier form.
        assert_eq!(names(&read_mod_order("*Base\nOff\n")), ["+Base", "-Off"]);
    }

    #[test]
    fn reads_the_earlier_profile_form() {
        let old = "=+Late\n% Patches\n-Off\n+Base\n";
        assert_eq!(names(&read_modlist(old)), ["+Base", "-Off", "[Patches]", "=+Late"]);
    }

    #[test]
    fn reads_an_mo2_profile() {
        let mo2 = "# This file was automatically generated by Mod Organizer.\r\n+Late\r\n-Off\r\n+Top_separator\r\n*DLC: Dead Money\r\n+Base\r\n";
        assert_eq!(names(&read_modlist(mo2)), ["+Base", "[Top]", "-Off", "+Late"]);
    }

    #[test]
    fn a_folder_named_like_a_separator_is_not_written_as_a_mod() {
        let rows = vec![ModLine::module("Real_separator", true), ModLine::module("a/b", true), ModLine::module("Fine", false)];
        assert_eq!(modlist_text(&rows), "-Fine\r\n");
        assert!(!is_listable("Real_separator"));
        assert!(is_listable("separator"));
    }
}
