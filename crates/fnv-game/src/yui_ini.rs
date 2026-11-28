//! Clean-room loader for `Data\NVSE\Plugins\yUI.ini` (with `Data\menus\ySI\ySI.ini`
//! overlay) (`docs/spec-notes/yui-ini.md`).
//!
//! Only the public **yUI - User Ynterface** v1.4d Nexus page
//! (`https://www.nexusmods.com/newvegas/mods/74357`), shipped
//! `Data\NVSE\Plugins\yUI.ini` section/key names, defaults, and inline comments, and
//! **ySI - Sorting Ycons** 1.4b (`https://www.nexusmods.com/newvegas/mods/74358`) shipped
//! `Data\menus\ySI\ySI.ini` were consulted. No plugin DLL (`yUI.dll`) or C++ source code
//! was opened, disassembled, or consulted.
//!
//! - Lookup reads `NVSE\Plugins\yUI.ini` and overlays `menus\ySI\ySI.ini` when present
//!   through `mod_files::read_data_file` (or a caller-supplied VFS reader).
//! - When neither INI file exists, [`YuiConfig::load_with`] returns `None` so unmodded
//!   vanilla UI, sorting, hotkey icons, and cursor tint stay completely unchanged
//!   [design].
//! - Parsing operates directly on raw `&[u8]` slices (no `&str` byte-index slicing) so
//!   UTF-8 BOMs, CRLF endings, `;`/`#` comments, non-ASCII text, and invalid UTF-8 bytes
//!   never panic [design].
//! - Already-avoided original-engine bugfix keys (`bFixIndefiniteSorting`,
//!   `bFixDroppedItems`, `bFixTablineSelected`) are skipped cleanly rather than stored as
//!   dead struct fields.

/// Case-insensitive VFS path for `Data\NVSE\Plugins\yUI.ini`
/// [documented: yUI - User Ynterface v1.4d (`https://www.nexusmods.com/newvegas/mods/74357`),
/// shipped `Data\NVSE\Plugins\yUI.ini` install path].
pub const YUI_INI_PATH: &str = "NVSE\\Plugins\\yUI.ini";

pub use crate::ysi_filename::{YsiCategoryDef, YsiCategoryIndex};

/// Case-insensitive VFS path for `Data\menus\ySI\ySI.ini`
/// [documented: ySI - Sorting Ycons 1.4b (`https://www.nexusmods.com/newvegas/mods/74358`),
/// shipped `Data\menus\ySI\ySI.ini` containing `[General] bSortingIcons = 1`].
pub const YSI_INI_PATH: &str = "menus\\ySI\\ySI.ini";

/// Parsed `Data\NVSE\Plugins\yUI.ini` configuration (with optional `menus\ySI\ySI.ini`
/// overlay) [documented: yUI - User Ynterface v1.4d
/// (`https://www.nexusmods.com/newvegas/mods/74357`), shipped `Data\NVSE\Plugins\yUI.ini`;
/// ySI - Sorting Ycons 1.4b (`https://www.nexusmods.com/newvegas/mods/74358`), shipped
/// `Data\menus\ySI\ySI.ini`; plugin DLL and source code were never consulted].
///
/// Every field here has a live engine consumer:
/// - `sorting_icons` -> `ui_loading`, `Game::ysi_active`, and gates `[Sorting and Icons]`
/// - `matching_cursor` -> `nvf_play::cursor`
/// - `sort_inventory` -> `nvf_play::menu` / `MenuModel::apply_ysi_options`
/// - `add_inventory_icons` -> `nvf_play::menu`, `tiles/item_mod_menu`, `tiles/repair_menu`
/// - `replace_hotkey_icons` -> `Game::ysi_hotkey_icon_path` -> `nvf_play::hotkeys::icon_override`
///   (`pipboy/hotkey_panel` and `weapon_wheel/draw`)
/// - `enable_categories` -> `Game::ysi_categories_enabled` / `Game::ysi_categories` ->
///   `nvf_play::menu` / `MenuModel::apply_ysi_categories`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct YuiConfig {
    /// `[General] bSortingIcons`: master gate for `[Sorting and Icons]` and ySI XML
    /// injection (shipped `yUI.ini` default `0`; `menus\ySI\ySI.ini` sets `1`).
    pub sorting_icons: bool,
    /// `[General] bMatchingCursor`: tint the mouse cursor arrow to match the HUD colour
    /// (shipped `yUI.ini` default `0`).
    pub matching_cursor: bool,
    /// `[Sorting and Icons] bSortInventory`: sort inventory/container/barter lists by ySI
    /// tag names (shipped `yUI.ini` default `1`; gated by `sorting_icons`).
    pub sort_inventory: bool,
    /// `[Sorting and Icons] bAddInventoryIcons`: add ySI icons to inventory, container,
    /// barter, repair, and item-mod menus (shipped `yUI.ini` default `1`; gated by
    /// `sorting_icons`).
    pub add_inventory_icons: bool,
    /// `[Sorting and Icons] bReplaceHotkeyIcons`: replace hotkey icons with ySI icons
    /// when no explicit `SetItemHotkeyIconPath` override is set (shipped `yUI.ini`
    /// default `1`; gated by `sorting_icons`).
    pub replace_hotkey_icons: bool,
    /// `[Sorting and Icons] bEnableCategories`: enable JSON-defined clickable Pip-Boy
    /// inventory categories and subfolders (shipped `yUI.ini` default `1`; gated by
    /// `sorting_icons`).
    pub enable_categories: bool,
}

impl Default for YuiConfig {
    /// Exact defaults from shipped `Data\NVSE\Plugins\yUI.ini`
    /// [documented: yUI - User Ynterface v1.4d (`https://www.nexusmods.com/newvegas/mods/74357`)].
    fn default() -> Self {
        Self {
            sorting_icons: false,
            matching_cursor: false,
            sort_inventory: true,
            add_inventory_icons: true,
            replace_hotkey_icons: true,
            enable_categories: true,
        }
    }
}

impl YuiConfig {
    /// Whether inventory/container/barter tag sorting is enabled (`bSortingIcons && bSortInventory`).
    pub const fn effective_sort_inventory(self) -> bool {
        self.sorting_icons && self.sort_inventory
    }

    /// Whether inventory/container/barter/repair/item-mod row icons are enabled
    /// (`bSortingIcons && bAddInventoryIcons`).
    pub const fn effective_add_inventory_icons(self) -> bool {
        self.sorting_icons && self.add_inventory_icons
    }

    /// Whether hotkey icon replacement with ySI icons is enabled
    /// (`bSortingIcons && bReplaceHotkeyIcons`).
    pub const fn effective_replace_hotkey_icons(self) -> bool {
        self.sorting_icons && self.replace_hotkey_icons
    }

    /// Whether JSON-defined Pip-Boy inventory categories and subfolders are enabled
    /// (`bSortingIcons && bEnableCategories`).
    pub const fn effective_enable_categories(self) -> bool {
        self.sorting_icons && self.enable_categories
    }

    /// Parse raw `yUI.ini` bytes from defaults strictly on `&[u8]` slices [design].
    pub fn parse(bytes: &[u8]) -> Self {
        let mut cfg = Self::default();
        cfg.apply_overlay(bytes);
        cfg
    }

    /// Overlay keys from raw INI bytes onto `self` (used both for primary `yUI.ini` and
    /// for layering `menus\ySI\ySI.ini` on top) [design].
    pub fn apply_overlay(&mut self, bytes: &[u8]) {
        let body = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
        let mut section = Section::None;

        for raw_line in body.split(|&b| b == b'\n') {
            let line = strip_comment(raw_line.strip_suffix(b"\r").unwrap_or(raw_line));
            let line = trim_ascii(line);
            if line.is_empty() {
                continue;
            }
            if line[0] == b'[' {
                if let Some(end) = line.iter().position(|&b| b == b']') {
                    section = Section::from_bytes(trim_ascii(&line[1..end]));
                }
                continue;
            }
            let Some(eq) = line.iter().position(|&b| b == b'=') else {
                continue;
            };
            let key = trim_ascii(&line[..eq]);
            let val = trim_ascii(&line[eq + 1..]);
            if key.is_empty() || val.is_empty() {
                continue;
            }

            match section {
                Section::General => {
                    if key.eq_ignore_ascii_case(b"bSortingIcons") {
                        if let Some(b) = parse_bool(val) {
                            self.sorting_icons = b;
                        }
                    } else if key.eq_ignore_ascii_case(b"bMatchingCursor") {
                        if let Some(b) = parse_bool(val) {
                            self.matching_cursor = b;
                        }
                    }
                }
                Section::SortingAndIcons => {
                    if key.eq_ignore_ascii_case(b"bSortInventory") {
                        if let Some(b) = parse_bool(val) {
                            self.sort_inventory = b;
                        }
                    } else if key.eq_ignore_ascii_case(b"bAddInventoryIcons") {
                        if let Some(b) = parse_bool(val) {
                            self.add_inventory_icons = b;
                        }
                    } else if key.eq_ignore_ascii_case(b"bReplaceHotkeyIcons") {
                        if let Some(b) = parse_bool(val) {
                            self.replace_hotkey_icons = b;
                        }
                    } else if key.eq_ignore_ascii_case(b"bEnableCategories") {
                        if let Some(b) = parse_bool(val) {
                            self.enable_categories = b;
                        }
                    }
                }
                Section::None | Section::Other => {}
            }
        }
    }

    /// Load `NVSE\Plugins\yUI.ini` and `menus\ySI\ySI.ini` (when present) using a VFS
    /// reader. Returns `None` when neither file is present so vanilla sessions stay
    /// untouched [design: absent-INI vanilla gate; hypothesis: effective `bSortingIcons`
    /// combines both INIs with logical OR (`yUI.bSortingIcons || ySI.bSortingIcons`) and
    /// `ySIGetTrait` JSON queries remain active when UI injection is disabled; see
    /// `docs/spec-notes/yui-ini.md`].
    pub fn load_with(mut read: impl FnMut(&str) -> Option<Vec<u8>>) -> Option<Self> {
        let yui = read(YUI_INI_PATH);
        let ysi = read(YSI_INI_PATH);
        if yui.is_none() && ysi.is_none() {
            return None;
        }
        let mut cfg = Self::default();
        if let Some(bytes) = yui {
            cfg.apply_overlay(&bytes);
        }
        let yui_sorting_icons = cfg.sorting_icons;
        if let Some(bytes) = ysi {
            cfg.apply_overlay(&bytes);
        }
        // [hypothesis] ySI.ini bSortingIcons=0 does not turn off an explicit yUI.ini
        // bSortingIcons=1 (and vice versa).
        cfg.sorting_icons = yui_sorting_icons || cfg.sorting_icons;
        Some(cfg)
    }

    /// Load `yUI.ini` / `ySI.ini` from NVForever's mounted Data files (`mod_files`).
    pub fn load_from_data() -> Option<Self> {
        Self::load_with(crate::mod_files::read_data_file)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    None,
    General,
    SortingAndIcons,
    Other,
}

impl Section {
    fn from_bytes(name: &[u8]) -> Self {
        if name.eq_ignore_ascii_case(b"General") {
            Self::General
        } else if name.eq_ignore_ascii_case(b"Sorting and Icons") {
            Self::SortingAndIcons
        } else {
            Self::Other
        }
    }
}

fn strip_comment(line: &[u8]) -> &[u8] {
    match line.iter().position(|&b| b == b';' || b == b'#') {
        Some(pos) => &line[..pos],
        None => line,
    }
}

fn trim_ascii(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|&b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
        .unwrap_or(bytes.len());
    let rest = &bytes[start..];
    let end = rest
        .iter()
        .rposition(|&b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
        .map_or(0, |i| i + 1);
    &rest[..end]
}

fn parse_bool(bytes: &[u8]) -> Option<bool> {
    if bytes.eq_ignore_ascii_case(b"true")
        || bytes.eq_ignore_ascii_case(b"yes")
        || bytes.eq_ignore_ascii_case(b"on")
    {
        return Some(true);
    }
    if bytes.eq_ignore_ascii_case(b"false")
        || bytes.eq_ignore_ascii_case(b"no")
        || bytes.eq_ignore_ascii_case(b"off")
    {
        return Some(false);
    }
    std::str::from_utf8(bytes)
        .ok()?
        .parse::<f32>()
        .ok()
        .filter(|n| n.is_finite())
        .map(|n| n != 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_shipped_yui_ini_defaults_and_gates_sorting_and_icons() {
        let ini = concat!(
            "[General]\r\n",
            "; fix the issue where items with different conditions would 'jump around' on update\r\n",
            "bFixIndefiniteSorting = 1\r\n",
            "bFixDroppedItems = 1\r\n",
            "bFixTablineSelected = 1\r\n",
            "bSortingIcons = 0\r\n",
            "bMatchingCursor = 0\r\n",
            "\r\n",
            "[Sorting and Icons]\r\n",
            "bSortInventory = 1\r\n",
            "bAddInventoryIcons = 1\r\n",
            "bReplaceHotkeyIcons = 1\r\n",
            "bEnableCategories = 1\r\n",
        );
        let cfg = YuiConfig::parse(ini.as_bytes());
        assert_eq!(cfg, YuiConfig::default());
        assert!(!cfg.sorting_icons);
        assert!(!cfg.matching_cursor);
        assert!(cfg.sort_inventory);
        assert!(cfg.add_inventory_icons);
        assert!(cfg.replace_hotkey_icons);
        assert!(cfg.enable_categories);
        // Master gate bSortingIcons = 0 disables every [Sorting and Icons] feature.
        assert!(!cfg.effective_sort_inventory());
        assert!(!cfg.effective_add_inventory_icons());
        assert!(!cfg.effective_replace_hotkey_icons());
        assert!(!cfg.effective_enable_categories());
    }

    #[test]
    fn ysi_ini_overlays_sorting_icons_while_preserving_yui_subtoggles() {
        let yui = b"[General]\nbSortingIcons = 0\nbMatchingCursor = 1\n[Sorting and Icons]\nbSortInventory = 0\nbAddInventoryIcons = 1\nbReplaceHotkeyIcons = 0\n";
        let ysi = b"[General]\nbSortingIcons = 1\n";
        let cfg = YuiConfig::load_with(|path| {
            if path.eq_ignore_ascii_case(YUI_INI_PATH) {
                Some(yui.to_vec())
            } else if path.eq_ignore_ascii_case(YSI_INI_PATH) {
                Some(ysi.to_vec())
            } else {
                None
            }
        })
        .expect("layered config");

        assert!(cfg.sorting_icons);
        assert!(cfg.matching_cursor);
        assert!(!cfg.effective_sort_inventory());
        assert!(cfg.effective_add_inventory_icons());
        assert!(!cfg.effective_replace_hotkey_icons());
    }

    #[test]
    fn sorting_icons_combines_yui_and_ysi_inis_with_logical_or() {
        let load = |yui: Option<&[u8]>, ysi: Option<&[u8]>| {
            YuiConfig::load_with(|path| {
                if path.eq_ignore_ascii_case(YUI_INI_PATH) {
                    yui.map(<[u8]>::to_vec)
                } else if path.eq_ignore_ascii_case(YSI_INI_PATH) {
                    ysi.map(<[u8]>::to_vec)
                } else {
                    None
                }
            })
            .expect("config present")
        };

        // yUI.ini bSortingIcons=1 + ySI.ini bSortingIcons=0 stays enabled (see `YuiConfig` docs above).
        let yui1_ysi0 = load(
            Some(b"[General]\nbSortingIcons = 1\n"),
            Some(b"[General]\nbSortingIcons = 0\n"),
        );
        assert!(yui1_ysi0.sorting_icons);
        assert!(yui1_ysi0.effective_sort_inventory());
        assert!(yui1_ysi0.effective_add_inventory_icons());

        // yUI.ini bSortingIcons=0 + ySI.ini bSortingIcons=1 is enabled (see `YuiConfig` docs above).
        let yui0_ysi1 = load(
            Some(b"[General]\nbSortingIcons = 0\n"),
            Some(b"[General]\nbSortingIcons = 1\n"),
        );
        assert!(yui0_ysi1.sorting_icons);

        // yUI.ini bSortingIcons=0 + no ySI.ini disables UI gating.
        let yui0_none = load(Some(b"[General]\nbSortingIcons = 0\n"), None);
        assert!(!yui0_none.sorting_icons);

        // Both explicitly 0 disables UI gating.
        let yui0_ysi0 = load(
            Some(b"[General]\nbSortingIcons = 0\n"),
            Some(b"[General]\nbSortingIcons = 0\n"),
        );
        assert!(!yui0_ysi0.sorting_icons);
    }

    #[test]
    fn absent_inis_return_none() {
        assert_eq!(YuiConfig::load_with(|_| None), None);
    }

    #[test]
    fn handles_bom_comments_case_non_ascii_and_invalid_utf8_cleanly() {
        let mut raw = Vec::new();
        raw.extend_from_slice(b"\xEF\xBB\xBF");
        raw.extend_from_slice(b"# utf8 comment \xC3\xA9\r\n");
        raw.extend_from_slice(b"; invalid utf8 \xFF\xFE\x80\r\n");
        raw.extend_from_slice(b"  [gEnErAl] ; inline comment\r\n");
        raw.extend_from_slice(b"  bsortingicons = TRUE # hash comment\r\n");
        raw.extend_from_slice(b"  bmatchingcursor = yes\r\n");
        raw.extend_from_slice(b"[\xC3\xA9Unknown\xFF]\r\n");
        raw.extend_from_slice(b"bSortingIcons = 0\r\n");
        raw.extend_from_slice(b"[SORTING AND ICONS]\r\n");
        raw.extend_from_slice(b"bSortInventory = off\r\n");
        raw.extend_from_slice(b"bAddInventoryIcons = \xFF\xFE\r\n");
        raw.extend_from_slice(b"bReplaceHotkeyIcons = 0\r\n");
        raw.extend_from_slice(b"bEnableCategories = 0\r\n");

        let cfg = YuiConfig::parse(&raw);
        assert!(cfg.sorting_icons);
        assert!(cfg.matching_cursor);
        assert!(!cfg.effective_sort_inventory());
        // Invalid value leaves default `true` intact.
        assert!(cfg.effective_add_inventory_icons());
        assert!(!cfg.effective_replace_hotkey_icons());
        assert!(!cfg.effective_enable_categories());
    }
}
