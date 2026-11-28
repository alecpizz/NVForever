//! Clean-room runtime support for `Data\NVSE\Plugins\kNVSE.ini`
//! (`[Anim Fixes] sLegacyAnimTimePaths`, `docs/spec-notes/ext-knvse.md` §5).
//!
//! Sources: kNVSE Animation Plugin v37 (`https://www.nexusmods.com/newvegas/mods/71336`),
//! shipped `Data\NVSE\Plugins\kNVSE.ini` public comments and defaults. No kNVSE DLL,
//! PDB, disassembly, or source code was inspected or consulted.
//!
//! Only `[Anim Fixes] sLegacyAnimTimePaths` is stored and wired here so there are zero
//! dead fields. When `kNVSE.ini` is absent, `legacy_anim_time_paths` is empty and
//! vanilla/default sequence timing is completely unchanged.

/// Relative Data path for kNVSE's INI [documented: kNVSE v37 shipped `kNVSE.ini`].
pub const KNVSE_INI_PATH: &str = "NVSE\\Plugins\\kNVSE.ini";

/// Active runtime settings read from `Data\NVSE\Plugins\kNVSE.ini`.
#[derive(Debug, Clone, Default)]
pub struct KnvseIni {
    /// `[Anim Fixes] sLegacyAnimTimePaths`: normalized lower-case path segments/prefixes
    /// (backslash-separated, trimmed, non-empty) parsed from `Data\NVSE\Plugins\kNVSE.ini`
    /// [documented: kNVSE Animation Plugin v37 (`https://www.nexusmods.com/newvegas/mods/71336`),
    /// `kNVSE.ini` comment `"; use legacy anim time algorithm for these paths (these mods
    /// rely on bugged behavior from previous versions of kNVSE)"`, shipped default
    /// `B42Inject,B42Interact,B42Loot`; design (NVForever): absent-INI default is empty,
    /// and listed paths currently stay on NVForever's existing sequence clock until a
    /// committed original-game probe shows where kNVSE v37 differs; see
    /// `docs/spec-notes/ext-knvse.md` §5].
    pub legacy_anim_time_paths: Vec<String>,
    roots_generation: u64,
    loaded_legacy_anim_time_paths: Vec<String>,
    auto_sync: bool,
}

impl PartialEq for KnvseIni {
    fn eq(&self, other: &Self) -> bool {
        self.legacy_anim_time_paths == other.legacy_anim_time_paths
    }
}

impl Eq for KnvseIni {}

impl KnvseIni {
    /// Load `Data\NVSE\Plugins\kNVSE.ini` from the current `mod_files` read roots / VFS
    /// reader (case-insensitive). Missing files leave `legacy_anim_time_paths` empty.
    pub fn load() -> Self {
        let roots_generation = crate::mod_files::roots_generation();
        let legacy_anim_time_paths = crate::mod_files::read_data_file(KNVSE_INI_PATH)
            .map(|b| Self::parse(&b).legacy_anim_time_paths)
            .unwrap_or_default();
        let loaded_legacy_anim_time_paths = legacy_anim_time_paths.clone();
        Self {
            legacy_anim_time_paths,
            roots_generation,
            loaded_legacy_anim_time_paths,
            auto_sync: true,
        }
    }

    /// Reload if `mod_files` roots changed since this snapshot was loaded and no caller
    /// has manually overridden `legacy_anim_time_paths`. Returns `true` when a reload
    /// occurred.
    pub fn sync(&mut self) -> bool {
        if !self.auto_sync {
            return false;
        }
        let gen = crate::mod_files::roots_generation();
        if gen == self.roots_generation {
            return false;
        }
        if self.legacy_anim_time_paths != self.loaded_legacy_anim_time_paths {
            self.auto_sync = false;
            self.roots_generation = gen;
            return false;
        }
        *self = Self::load();
        true
    }

    /// Parse `kNVSE.ini` from raw bytes (UTF-8 BOM safe, `;`/`#` comment safe, ASCII
    /// case-insensitive section and key matching, no `&str` byte-index slicing) [design].
    pub fn parse(bytes: &[u8]) -> Self {
        let mut legacy_anim_time_paths = Vec::new();
        parse_ini_bytes(bytes, |section, key, value| {
            if section.eq_ignore_ascii_case(b"Anim Fixes")
                && key.eq_ignore_ascii_case(b"sLegacyAnimTimePaths")
            {
                legacy_anim_time_paths = parse_path_list_bytes(value);
            }
        });
        Self {
            legacy_anim_time_paths,
            roots_generation: crate::mod_files::roots_generation(),
            loaded_legacy_anim_time_paths: Vec::new(),
            auto_sync: false,
        }
    }

    /// Whether `path` matches any configured `sLegacyAnimTimePaths` entry (ASCII
    /// case-insensitive, `/` and `\` normalized, optional leading `meshes\` ignored,
    /// matched on whole path segments so `B42Inject` matches `B42Inject\stim.kf` and
    /// `Characters\_1stPerson\B42Inject\stim.kf` without matching `NotB42Inject\stim.kf`)
    /// [design].
    pub fn uses_legacy_anim_time(&self, path: &str) -> bool {
        if self.legacy_anim_time_paths.is_empty() {
            return false;
        }
        let norm = normalize_rel_path(path);
        if norm.is_empty() {
            return false;
        }
        let path_parts: Vec<&str> = norm.split('\\').filter(|p| !p.is_empty()).collect();
        if path_parts.is_empty() {
            return false;
        }
        self.legacy_anim_time_paths.iter().any(|entry| {
            let entry_parts: Vec<&str> = entry.split('\\').filter(|p| !p.is_empty()).collect();
            !entry_parts.is_empty()
                && entry_parts.len() <= path_parts.len()
                && path_parts.windows(entry_parts.len()).any(|w| w == entry_parts.as_slice())
        })
    }
}

impl crate::game::Game<'_> {
    /// Reload `Data\NVSE\Plugins\kNVSE.ini` from the current `mod_files` read roots / VFS
    /// reader.
    pub fn reload_knvse_ini(&mut self) {
        self.knvse_ini = KnvseIni::load();
    }

    /// Reload `kNVSE.ini` if `mod_files` roots changed since the last load.
    pub fn sync_knvse_ini(&mut self) -> bool {
        self.knvse_ini.sync()
    }
}

/// Trim leading and trailing ASCII whitespace from a byte slice without UTF-8 assumptions.
fn trim_ascii_bytes(mut b: &[u8]) -> &[u8] {
    while let Some((&first, rest)) = b.split_first() {
        if first.is_ascii_whitespace() {
            b = rest;
        } else {
            break;
        }
    }
    while let Some((&last, rest)) = b.split_last() {
        if last.is_ascii_whitespace() {
            b = rest;
        } else {
            break;
        }
    }
    b
}

/// Strip an inline `;` or `#` comment from a byte slice and trim ASCII whitespace.
fn strip_inline_comment(b: &[u8]) -> &[u8] {
    let end = b.iter().position(|&c| c == b';' || c == b'#').unwrap_or(b.len());
    trim_ascii_bytes(&b[..end])
}

/// Parse an INI buffer on raw `&[u8]`, invoking `on_entry(section, key, value)` for each
/// `key = value` line. Never slices `&str` by byte index and tolerates arbitrary non-ASCII
/// or invalid UTF-8 bytes in comments, keys, or values [design].
fn parse_ini_bytes(bytes: &[u8], mut on_entry: impl FnMut(&[u8], &[u8], &[u8])) {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let mut section: &[u8] = b"";
    for raw_line in bytes.split(|&c| c == b'\n') {
        let line = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
        let trimmed = trim_ascii_bytes(line);
        if trimmed.is_empty() || trimmed[0] == b';' || trimmed[0] == b'#' {
            continue;
        }
        if trimmed[0] == b'[' {
            if let Some(close) = trimmed[1..].iter().position(|&c| c == b']') {
                section = trim_ascii_bytes(&trimmed[1..1 + close]);
            }
            continue;
        }
        let Some(eq) = trimmed.iter().position(|&c| c == b'=') else { continue };
        let key = trim_ascii_bytes(&trimmed[..eq]);
        if key.is_empty() {
            continue;
        }
        let value = strip_inline_comment(&trimmed[eq + 1..]);
        on_entry(section, key, value);
    }
}

/// Normalize a relative meshes/animation path to lower-case backslashes without leading
/// `meshes\` or redundant separators [design].
fn normalize_rel_path(path: &str) -> String {
    let mut parts: Vec<String> = path
        .split(['/', '\\'])
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| p.to_ascii_lowercase())
        .collect();
    if parts.first().is_some_and(|p| p == "meshes") {
        parts.remove(0);
    }
    parts.join("\\")
}

/// Parse a comma-separated list of path prefixes from raw bytes (trimmed, optional
/// surrounding quotes stripped, empty entries ignored, deduplicated preserving order)
/// [design].
fn parse_path_list_bytes(value: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    for part in value.split(|&c| c == b',') {
        let mut p = trim_ascii_bytes(part);
        if p.len() >= 2 && p[0] == b'"' && p[p.len() - 1] == b'"' {
            p = trim_ascii_bytes(&p[1..p.len() - 1]);
        }
        if p.is_empty() {
            continue;
        }
        let decoded = String::from_utf8_lossy(p);
        let norm = normalize_rel_path(&decoded);
        if !norm.is_empty() && !out.contains(&norm) {
            out.push(norm);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_ini_leaves_legacy_paths_empty() {
        let def = KnvseIni::default();
        assert!(def.legacy_anim_time_paths.is_empty());
        assert!(!def.uses_legacy_anim_time("B42Inject\\stim.kf"));
        assert!(!def.uses_legacy_anim_time("Characters\\_Male\\mtidle.kf"));
    }

    #[test]
    fn parses_shipped_v37_default_and_handles_bom_comments_case_quotes_and_non_utf8() {
        // Exact kNVSE v37 shipped default entry.
        let shipped = KnvseIni::parse(
            b"[Anim Fixes]\r\n; use legacy anim time algorithm for these paths\r\nsLegacyAnimTimePaths = B42Inject,B42Interact,B42Loot\r\n",
        );
        assert_eq!(shipped.legacy_anim_time_paths, vec!["b42inject", "b42interact", "b42loot"]);

        // BOM, non-ASCII and invalid UTF-8 in comments, mixed case, quotes, duplicates, inline # comment.
        let mut raw = Vec::new();
        raw.extend_from_slice(b"\xEF\xBB\xBF; UTF-8 comment: \xC3\xA9 \xE2\x80\x94\r\n");
        raw.extend_from_slice(b"# invalid utf8 comment: \xFF\xFE\r\n");
        raw.extend_from_slice(b"[ aNiM fIxEs ] ; trailing\r\n");
        raw.extend_from_slice(b"slegacyanimtimepaths =  B42Inject , \"meshes/B42Interact/\" , , B42Loot , b42inject # inline\r\n");
        let parsed = KnvseIni::parse(&raw);
        assert_eq!(parsed.legacy_anim_time_paths, vec!["b42inject", "b42interact", "b42loot"]);
        assert!(parsed.uses_legacy_anim_time("B42Inject\\1stperson\\stim.kf"));
        assert!(parsed.uses_legacy_anim_time("meshes/Characters/_1stPerson/b42interact/open.kf"));
        assert!(parsed.uses_legacy_anim_time("B42Loot/grab.kf"));
        assert!(!parsed.uses_legacy_anim_time("NotB42Inject\\stim.kf"));
        assert!(!parsed.uses_legacy_anim_time("B42InjectExtra\\stim.kf"));
        assert!(!parsed.uses_legacy_anim_time("Characters\\_Male\\mtidle.kf"));

        // Explicit empty value clears an earlier entry in the same INI.
        let cleared = KnvseIni::parse(b"[Anim Fixes]\nsLegacyAnimTimePaths = B42Inject\nsLegacyAnimTimePaths = ; cleared\n");
        assert!(cleared.legacy_anim_time_paths.is_empty());
        assert!(!cleared.uses_legacy_anim_time("B42Inject\\stim.kf"));
    }
}
