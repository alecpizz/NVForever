//! Bounded documented ySI configuration -> tag/icon/filename (docs/spec-notes/ysi-filename.md).
//! Serves `nvse_small.rs` (`ySIGetTrait` / `ySISetTrait` `"tag"`, `"string"`, `"icon"`,
//! `"filename"`) and `ui_ysi` display lookups.
use crate::{game::Game, mod_files};
use fnv_script::vm::{form_type_code, XVal};
use fnv_sim::FormId;
use serde_json::{Map, Value as Json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::OnceLock;

/// Maximum ancestor traversal depth when resolving nested ySI subfolder chains [design].
const MAX_CATEGORY_DEPTH: usize = 16;

/// One clickable Pip-Boy category or nested subfolder resolved from `menus/ySI/*.json`
/// `"icons"` entries (`name`, `tab`, `count`, `filename`, `template`, `icon`, optional
/// parent `category`) [documented: ySI - Sorting Ycons v1.4b
/// (`https://www.nexusmods.com/newvegas/mods/74358`), `menus/ySI/ySI.json` `"icons"` entries
/// defining `(jdc0)` / `(jdd0)` / `(jde0)` with `"name"`, `"tab"`, `"count"`, `"filename"`,
/// `"template"`, `"icon"`, and child tags carrying `"category"`; no yUI/ySI DLL or source
/// consulted].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YsiCategoryDef {
    /// Sorting tag of the category folder row itself (e.g. `"(jdd0)"`, `"(kw00)"`).
    pub tag: String,
    /// Resolved display label (game-string entities like `"&-sKeyring;"` expanded).
    pub name: String,
    /// Pip-Boy Inventory tab index (`0` Weapons, `1` Apparel, `2` Aid, `3` Misc, `4` Ammo).
    pub tab: u8,
    /// Optional parent category index in [`YsiCategoryIndex::categories`] for nested subfolders.
    pub parent: Option<u32>,
    /// Minimum distinct matching rows required to collapse into a folder (clamped `>= 1`).
    pub min_count: usize,
    /// Row icon DDS path (`filename`) for `ySIImage`.
    pub row_icon: Option<String>,
    /// Row XML template name (`template`, defaulting to `ySIDefault`).
    pub template: String,
    /// Large Pip-Boy preview picture DDS path (`icon`) shown in `IM_ItemIcon`.
    pub preview_icon: Option<String>,
}

/// Resolved table of Pip-Boy ySI categories/subfolders and item-tag -> category memberships
/// built from the session's `menus/ySI/*.json` configuration [design].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct YsiCategoryIndex {
    /// Deterministic category list ordered by `(tab, tag)`.
    pub categories: Vec<YsiCategoryDef>,
    /// Maps an item sorting tag (e.g. `"(jdd1)"`, `"(fbb3)"`) to its direct category index.
    pub tag_to_category: BTreeMap<String, u32>,
}

// Mapped configuration resource bounds, not native limits.
const MAX_FILES: usize = 128;
const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_RULES: usize = 20_000;

#[derive(Default)]
pub(crate) struct State {
    // Original configuration documentation describes launch-time classification.
    // This session's first query freezes its input snapshot, including a load error.
    // Game restore creates a fresh cache; no result is attached to an item or save.
    book: OnceLock<Result<Book, String>>,
    // Pre-indexed tag rule lookup tables built once per session from `book` and
    // `g.data.load_order` so Pip-Boy / inventory refreshes avoid scanning thousands of
    // JSON rules and 255 plugin names per item [design].
    compiled: OnceLock<CompiledTags>,
    // Cached Pip-Boy category/subfolder index built once per session [design].
    categories: OnceLock<Option<YsiCategoryIndex>>,
    // Cached `Data\NVSE\Plugins\yUI.ini` (with `Data\menus\ySI\ySI.ini` overlay) snapshot.
    yui_ini: OnceLock<Option<crate::yui_ini::YuiConfig>>,
}
struct Rule {
    priority: i64,
    tag: String,
    fields: Map<String, Json>,
}
struct Book {
    tags: Vec<Rule>,
    icons: Vec<Rule>,
    /// Pre-resolved winning `(filename, template)` per tag for O(1) icon lookup [design].
    icons_by_tag: HashMap<String, Result<(String, Option<String>), String>>,
}
struct CompiledTags {
    /// Rules with `formlist == 0` and valid loaded `mod` + hex `form`(s), keyed by resolved FormId.
    by_explicit_form: HashMap<FormId, Vec<usize>>,
    /// Non-explicit rules constrained to concrete `formType` integer code(s).
    by_form_type: HashMap<u16, Vec<usize>>,
    /// Remaining generic rules (no `formType`, non-integer `formType`, or malformed explicit `form`).
    any_type: Vec<usize>,
    /// Lowercased plugin name -> load order index for O(1) mod lookup inside rules.
    mod_index: HashMap<String, usize>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Match {
    Yes,
    No,
    Unknown,
}

/// Returns the session's cached `yUI.ini` (+ `menus\ySI\ySI.ini` overlay) configuration,
/// loading it lazily once on first query (`None` when neither INI exists).
pub(crate) fn yui_config(g: &Game<'_>) -> Option<crate::yui_ini::YuiConfig> {
    *g.ysi_filename
        .yui_ini
        .get_or_init(crate::yui_ini::YuiConfig::load_from_data)
}

/// Replaces the session's cached `yUI.ini` configuration (primarily for isolated tests).
pub(crate) fn set_yui_config(g: &mut Game<'_>, cfg: Option<crate::yui_ini::YuiConfig>) {
    g.ysi_filename.yui_ini = OnceLock::from(cfg);
}
/// Resolves a form's certain JSON-configured ySI tag using the session's cached
/// configuration snapshot (returns `None` when configuration is unavailable,
/// unmatched, ambiguous, or blocked by an unsupported predicate).
pub(crate) fn json_tag(g: &Game<'_>, form: FormId) -> Option<String> {
    let book = g.ysi_filename.book.get_or_init(Book::load).as_ref().ok()?;
    let compiled = g
        .ysi_filename
        .compiled
        .get_or_init(|| CompiledTags::build(book, &g.data.load_order));
    compiled.resolve_tag(book, g, form).ok()
}

/// Resolves the certain winning JSON icon `filename` for a ySI sorting tag using
/// the session's cached configuration snapshot.
pub(crate) fn json_icon_filename(g: &Game<'_>, tag: &str) -> Option<String> {
    json_icon_spec(g, tag).map(|(filename, _)| filename)
}

/// Resolves the certain winning JSON icon `(filename, optional template)` for a ySI
/// sorting tag using the session's cached configuration snapshot.
pub(crate) fn json_icon_spec(g: &Game<'_>, tag: &str) -> Option<(String, Option<String>)> {
    let book = g.ysi_filename.book.get_or_init(Book::load).as_ref().ok()?;
    book.icon_spec(tag).ok()
}

/// Whether a valid `menus/ySI/*.json` configuration book is loaded in this session.
pub(crate) fn has_loaded_book(g: &Game<'_>) -> bool {
    g.ysi_filename.book.get_or_init(Book::load).is_ok()
}

/// Resolves all clickable Pip-Boy category/subfolder definitions and tag memberships from
/// the session's cached `menus/ySI/*.json` book (`None` when no valid book or no categories
/// are defined).
pub(crate) fn json_categories(g: &Game<'_>) -> Option<YsiCategoryIndex> {
    g.ysi_filename
        .categories
        .get_or_init(|| {
            let book = g.ysi_filename.book.get_or_init(Book::load).as_ref().ok()?;
            let index = book.build_categories(|raw| resolve_category_name(g, raw));
            (!index.categories.is_empty()).then_some(index)
        })
        .clone()
}

/// Expands game-string entities like `"&-sKeyring;"` on category names through
/// `ui_loading::game_string`, falling back to clean built-in labels when a test or install
/// omits the GMST [design].
fn resolve_category_name(g: &Game<'_>, raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(inner) = trimmed.strip_prefix("&-").and_then(|s| s.strip_suffix(';')) {
        let key = inner.trim();
        let resolved = crate::ui_loading::game_string(g, key);
        if !resolved.trim().is_empty() {
            return resolved;
        }
        return match key.to_ascii_lowercase().as_str() {
            "skeyring" => "Keyring".to_string(),
            "snotestoggle" => "Notes".to_string(),
            "scaravandecktext" => "Caravan Deck".to_string(),
            _ => key.to_string(),
        };
    }
    trimmed.to_string()
}

impl Book {
    fn load() -> Result<Self, String> {
        let mut names: Vec<String> = mod_files::data_files("menus\\ySI")
            .into_iter()
            .map(|(name, _)| name)
            .filter(|name| name.to_ascii_lowercase().ends_with(".json"))
            .collect();
        if !names.iter().any(|n| n.eq_ignore_ascii_case("ySI.json")) {
            names.push("ySI.json".into());
        }
        names.sort_by_key(|n| n.to_ascii_lowercase());
        if names.len() > MAX_FILES {
            return Err("ySI filename configuration exceeds the mapped file bound".into());
        }
        let mut book = Self {
            tags: Vec::new(),
            icons: Vec::new(),
            icons_by_tag: HashMap::new(),
        };
        let mut bytes_read = 0usize;
        for name in names {
            let bytes = mod_files::read_data_file(&format!("menus\\ySI\\{name}"))
                .ok_or_else(|| format!("ySI filename configuration unavailable: {name}"))?;
            bytes_read = bytes_read
                .checked_add(bytes.len())
                .ok_or("ySI configuration size overflow")?;
            if bytes_read > MAX_BYTES {
                return Err("ySI filename configuration exceeds the mapped byte bound".into());
            }
            let text = std::str::from_utf8(&bytes).map_err(|_| "ySI configuration is not UTF-8")?;
            let cleaned = commented_json(text)?;
            if cleaned.trim().is_empty() && !name.eq_ignore_ascii_case("ySI.json") {
                continue;
            }
            let value: Json = serde_json::from_str(&cleaned)
                .map_err(|_| "invalid ySI commented configuration")?;
            let object = value
                .as_object()
                .ok_or("ySI configuration must be an object")?;
            for (key, rules) in [("tags", &mut book.tags), ("icons", &mut book.icons)] {
                let Some(entries) = object.get(key) else {
                    continue;
                };
                for entry in entries
                    .as_array()
                    .ok_or("ySI rule section must be an array")?
                {
                    let fields = entry
                        .as_object()
                        .ok_or("ySI rule must be an object")?
                        .clone();
                    let priority = fields
                        .get("priority")
                        .and_then(Json::as_i64)
                        .ok_or("ySI rule priority must be an integer")?;
                    let tag = fields
                        .get("tag")
                        .and_then(Json::as_str)
                        .ok_or("ySI rule tag must be a string")?
                        .to_string();
                    rules.push(Rule {
                        priority,
                        tag,
                        fields,
                    });
                }
            }
            if book.tags.len() + book.icons.len() > MAX_RULES {
                return Err("ySI filename configuration exceeds the mapped rule bound".into());
            }
        }
        // Sort tag rules by descending priority so evaluation can stop as soon as a
        // strictly higher-priority certain match is found [design].
        book.tags.sort_by_key(|a| std::cmp::Reverse(a.priority));
        // Pre-index winning icon spec per tag for O(1) `icon_spec` lookups [design].
        let unique_tags: BTreeSet<&str> = book.icons.iter().map(|r| r.tag.as_str()).collect();
        let mut icons_by_tag = HashMap::with_capacity(unique_tags.len());
        for tag in unique_tags {
            icons_by_tag.insert(tag.to_string(), book.compute_icon_spec(tag));
        }
        book.icons_by_tag = icons_by_tag;
        Ok(book)
    }

    fn resolve_tag(&self, g: &Game<'_>, form: FormId) -> Result<String, String> {
        let mut best = None;
        let mut tags = BTreeSet::new();
        let mut unknown = None;
        for rule in &self.tags {
            if best.is_some_and(|p| rule.priority < p) {
                break;
            }
            match rule.matches(g, form, None) {
                Match::Yes => match best {
                    Some(priority) if priority > rule.priority => {}
                    Some(priority) if priority == rule.priority => {
                        tags.insert(rule.tag.as_str());
                    }
                    _ => {
                        best = Some(rule.priority);
                        tags.clear();
                        tags.insert(rule.tag.as_str());
                    }
                },
                Match::Unknown => {
                    unknown = Some(unknown.map_or(rule.priority, |p: i64| p.max(rule.priority)))
                }
                Match::No => {}
            }
        }
        finish_tag_resolution(best, &tags, unknown)
    }

    fn icon_spec(&self, tag: &str) -> Result<(String, Option<String>), String> {
        match self.icons_by_tag.get(tag) {
            Some(res) => res.clone(),
            None => Err(
                "missing or ambiguous ySI icon filename; native defaults/ties remain unqualified"
                    .into(),
            ),
        }
    }

    fn compute_icon_spec(&self, tag: &str) -> Result<(String, Option<String>), String> {
        let priority = self
            .icons
            .iter()
            .filter(|r| r.tag == tag)
            .map(|r| r.priority)
            .max();
        let mut filenames = BTreeSet::new();
        let mut templates = BTreeSet::new();
        for rule in self
            .icons
            .iter()
            .filter(|r| r.tag == tag && Some(r.priority) == priority)
        {
            let filename = rule
                .fields
                .get("filename")
                .and_then(Json::as_str)
                .ok_or("selected ySI icon has no filename string")?;
            filenames.insert(filename);
            if let Some(tpl) = rule
                .fields
                .get("template")
                .and_then(Json::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                templates.insert(tpl);
            }
        }
        if filenames.len() != 1 {
            return Err(
                "missing or ambiguous ySI icon filename; native defaults/ties remain unqualified"
                    .into(),
            );
        }
        let template = if templates.len() == 1 {
            templates.first().map(|s| (*s).to_string())
        } else {
            None
        };
        Ok(((*filenames.first().expect("one filename")).to_string(), template))
    }

    /// Builds the deterministic category/subfolder index from winning highest-priority
    /// `"icons"` entries across all loaded `menus/ySI/*.json` files.
    fn build_categories(&self, mut resolve_name: impl FnMut(&str) -> String) -> YsiCategoryIndex {
        // Group winning (highest-priority) icon rules per tag.
        let mut max_prio: BTreeMap<&str, i64> = BTreeMap::new();
        for rule in &self.icons {
            if rule.tag.is_empty() {
                continue;
            }
            max_prio
                .entry(rule.tag.as_str())
                .and_modify(|p| *p = (*p).max(rule.priority))
                .or_insert(rule.priority);
        }

        struct RawCategory {
            tag: String,
            name: String,
            tab: Option<u8>,
            parent_tag: Option<String>,
            min_count: usize,
            row_icon: Option<String>,
            template: String,
            preview_icon: Option<String>,
        }

        let mut raw_cats: BTreeMap<String, RawCategory> = BTreeMap::new();
        let mut raw_membership: BTreeMap<String, String> = BTreeMap::new();

        for rule in &self.icons {
            if rule.tag.is_empty() || max_prio.get(rule.tag.as_str()) != Some(&rule.priority) {
                continue;
            }
            let parent_tag = rule
                .fields
                .get("category")
                .and_then(Json::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty() && *s != rule.tag.as_str())
                .map(str::to_string);

            let name_raw = rule
                .fields
                .get("name")
                .and_then(Json::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty());

            let tab = rule
                .fields
                .get("tab")
                .and_then(Json::as_i64)
                .filter(|t| (0..=4).contains(t))
                .map(|t| t as u8);

            if let Some(raw_name) = name_raw {
                if tab.is_some() || parent_tag.is_some() {
                    let min_count = rule
                        .fields
                        .get("count")
                        .and_then(Json::as_i64)
                        .map_or(1, |c| c.max(1) as usize);
                    let row_icon = rule
                        .fields
                        .get("filename")
                        .and_then(Json::as_str)
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string);
                    let tpl_opt = rule
                        .fields
                        .get("template")
                        .and_then(Json::as_str)
                        .map(str::trim)
                        .filter(|s| !s.is_empty());
                    let template =
                        crate::ui_ysi::row_template_or_default(tpl_opt, row_icon.is_some());
                    let preview_icon = rule
                        .fields
                        .get("icon")
                        .and_then(Json::as_str)
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string);
                    let resolved = resolve_name(raw_name);
                    if !resolved.is_empty() {
                        raw_cats.insert(
                            rule.tag.clone(),
                            RawCategory {
                                tag: rule.tag.clone(),
                                name: resolved,
                                tab,
                                parent_tag: parent_tag.clone(),
                                min_count,
                                row_icon,
                                template,
                                preview_icon,
                            },
                        );
                    }
                }
            } else if let Some(parent) = parent_tag {
                raw_membership.insert(rule.tag.clone(), parent);
            }
        }

        // Resolve effective tab for each category (own tab or inherited from parent chain
        // with cycle protection) and keep only well-formed categories.
        let mut resolved_tabs: BTreeMap<String, u8> = BTreeMap::new();
        for (tag, cat) in &raw_cats {
            let mut cur_tag = tag.as_str();
            let mut seen = BTreeSet::new();
            let mut found_tab = cat.tab;
            let mut cycle = false;
            seen.insert(cur_tag);
            while found_tab.is_none() && seen.len() <= MAX_CATEGORY_DEPTH {
                let Some(next) = raw_cats.get(cur_tag).and_then(|c| c.parent_tag.as_deref()) else {
                    break;
                };
                if !seen.insert(next) {
                    cycle = true;
                    break;
                }
                let Some(parent_cat) = raw_cats.get(next) else {
                    break;
                };
                found_tab = parent_cat.tab;
                cur_tag = next;
            }
            // Also verify explicit-tab child categories do not form a parent cycle.
            if !cycle {
                let mut walk = cat.parent_tag.as_deref();
                let mut check_seen = BTreeSet::new();
                check_seen.insert(tag.as_str());
                while let Some(next) = walk {
                    if !check_seen.insert(next) || check_seen.len() > MAX_CATEGORY_DEPTH {
                        cycle = true;
                        break;
                    }
                    walk = raw_cats.get(next).and_then(|c| c.parent_tag.as_deref());
                }
            }
            if !cycle {
                if let Some(t) = found_tab {
                    resolved_tabs.insert(tag.clone(), t);
                }
            }
        }

        let mut ordered: Vec<(u8, String)> = resolved_tabs
            .iter()
            .map(|(tag, &tab)| (tab, tag.clone()))
            .collect();
        ordered.sort();

        let tag_to_idx: BTreeMap<String, u32> = ordered
            .iter()
            .enumerate()
            .map(|(i, (_, tag))| (tag.clone(), i as u32))
            .collect();

        let mut categories = Vec::with_capacity(ordered.len());
        for (tab, tag) in &ordered {
            let cat = &raw_cats[tag];
            let parent = cat
                .parent_tag
                .as_deref()
                .and_then(|p| tag_to_idx.get(p).copied());
            categories.push(YsiCategoryDef {
                tag: cat.tag.clone(),
                name: cat.name.clone(),
                tab: *tab,
                parent,
                min_count: cat.min_count,
                row_icon: cat.row_icon.clone(),
                template: cat.template.clone(),
                preview_icon: cat.preview_icon.clone(),
            });
        }

        let mut tag_to_category = BTreeMap::new();
        for (item_tag, cat_tag) in raw_membership {
            if let Some(&idx) = tag_to_idx.get(&cat_tag) {
                tag_to_category.insert(item_tag, idx);
            }
        }

        YsiCategoryIndex {
            categories,
            tag_to_category,
        }
    }
}

fn finish_tag_resolution(
    best: Option<i64>,
    tags: &BTreeSet<&str>,
    unknown: Option<i64>,
) -> Result<String, String> {
    let Some(priority) = best else {
        return Err("no certain ySI tag; native missing-tag behavior remains unqualified".into());
    };
    if unknown.is_some_and(|p| p >= priority) {
        return Err("an unsupported ySI predicate could change the highest-priority tag".into());
    }
    if tags.len() != 1 {
        return Err("ambiguous ySI tag priority; native tie ordering remains unqualified".into());
    }
    Ok((*tags.first().expect("one tag")).to_string())
}

impl CompiledTags {
    fn build(book: &Book, load_order: &[String]) -> Self {
        let mut mod_index: HashMap<String, usize> = HashMap::with_capacity(load_order.len());
        for (i, plugin) in load_order.iter().enumerate() {
            mod_index.entry(plugin.to_ascii_lowercase()).or_insert(i);
        }
        let mut by_explicit_form: HashMap<FormId, Vec<usize>> = HashMap::new();
        let mut by_form_type: HashMap<u16, Vec<usize>> = HashMap::new();
        let mut any_type: Vec<usize> = Vec::new();

        for (rule_idx, rule) in book.tags.iter().enumerate() {
            let formlist_mode = match rule.fields.get("formlist") {
                None => Some(0i64),
                Some(v) => match v.as_i64() {
                    Some(n @ 0..=2) => Some(n),
                    _ => None,
                },
            };
            // Rules referencing an unloaded plugin with a valid `form` selector (modes 0..=2)
            // always evaluate to `Match::No`; drop them upfront.
            if let (Some(mode), Some(form_val)) = (formlist_mode, rule.fields.get("form")) {
                if let Some(mod_name) = rule.fields.get("mod").and_then(Json::as_str) {
                    let Some(&idx) = mod_index.get(&mod_name.to_ascii_lowercase()) else {
                        continue;
                    };
                    if mode == 0 && idx <= 255 {
                        let mut parsed = Vec::new();
                        let mut all_valid = true;
                        match form_val {
                            Json::Array(arr) => {
                                for v in arr {
                                    if let Some(local) = parse_local_form(v) {
                                        parsed.push(FormId(((idx as u32) << 24) | local));
                                    } else {
                                        all_valid = false;
                                        break;
                                    }
                                }
                            }
                            v => {
                                if let Some(local) = parse_local_form(v) {
                                    parsed.push(FormId(((idx as u32) << 24) | local));
                                } else {
                                    all_valid = false;
                                }
                            }
                        }
                        if all_valid {
                            for fid in parsed {
                                let bucket = by_explicit_form.entry(fid).or_default();
                                if bucket.last() != Some(&rule_idx) {
                                    bucket.push(rule_idx);
                                }
                            }
                            continue;
                        }
                    }
                }
            }

            // Bucket remaining generic rules by concrete `formType` when unambiguous.
            if let Some(ft_val) = rule.fields.get("formType") {
                let mut codes = Vec::new();
                let mut all_int = true;
                match ft_val {
                    Json::Array(arr) => {
                        for v in arr {
                            if let Some(n) = v.as_i64() {
                                if let Ok(code) = u16::try_from(n) {
                                    codes.push(code);
                                }
                            } else {
                                all_int = false;
                                break;
                            }
                        }
                    }
                    v => {
                        if let Some(n) = v.as_i64() {
                            if let Ok(code) = u16::try_from(n) {
                                codes.push(code);
                            }
                        } else {
                            all_int = false;
                        }
                    }
                }
                if all_int {
                    for code in codes {
                        let bucket = by_form_type.entry(code).or_default();
                        if bucket.last() != Some(&rule_idx) {
                            bucket.push(rule_idx);
                        }
                    }
                    continue;
                }
            }

            any_type.push(rule_idx);
        }

        Self {
            by_explicit_form,
            by_form_type,
            any_type,
            mod_index,
        }
    }

    fn resolve_tag(&self, book: &Book, g: &Game<'_>, form: FormId) -> Result<String, String> {
        let Some(code) = g.data.sig(form).and_then(|s| form_type_code(&s)) else {
            return book.resolve_tag(g, form);
        };
        let mut best: Option<i64> = None;
        let mut tags: BTreeSet<&str> = BTreeSet::new();
        let mut unknown: Option<i64> = None;

        let explicit = self
            .by_explicit_form
            .get(&form)
            .map_or(&[][..], Vec::as_slice);
        let typed = self.by_form_type.get(&code).map_or(&[][..], Vec::as_slice);
        for slice in [explicit, typed, self.any_type.as_slice()] {
            for &idx in slice {
                let rule = &book.tags[idx];
                if best.is_some_and(|p| rule.priority < p) {
                    break;
                }
                match rule.matches(g, form, Some(&self.mod_index)) {
                    Match::Yes => match best {
                        Some(priority) if priority > rule.priority => {}
                        Some(priority) if priority == rule.priority => {
                            tags.insert(rule.tag.as_str());
                        }
                        _ => {
                            best = Some(rule.priority);
                            tags.clear();
                            tags.insert(rule.tag.as_str());
                        }
                    },
                    Match::Unknown => {
                        unknown = Some(unknown.map_or(rule.priority, |p: i64| p.max(rule.priority)));
                    }
                    Match::No => {}
                }
            }
        }
        finish_tag_resolution(best, &tags, unknown)
    }
}

impl Rule {
    fn matches(&self, g: &Game<'_>, item: FormId, mod_index: Option<&HashMap<String, usize>>) -> Match {
        let mut answer = Match::Yes;
        let formlist_mode = match self.fields.get("formlist") {
            None => Some(0i64),
            Some(v) => match v.as_i64() {
                Some(n @ 0..=2) => Some(n),
                _ => None,
            },
        };
        if formlist_mode.is_none() {
            answer = Match::Unknown;
        }
        // [hypothesis] When paired with an explicit `form` selector (`formlist: 0`),
        // yUI v1.4d ignores non-applicable record-type predicates on mismatched record
        // types instead of disqualifying as `Match::No`.
        let explicit_form = self.fields.contains_key("form") && formlist_mode == Some(0);
        for (name, value) in &self.fields {
            let part = match name.as_str() {
                "tag" | "priority" | "mod" | "formlist" => continue,
                "form" => match formlist_mode {
                    Some(0) => self.explicit_form(g, item, value, mod_index),
                    Some(1) => self.formlist_member(g, item, value, mod_index),
                    Some(2) => crate::ysi_predicates::formlist_mode2(g, item, &self.fields, value),
                    _ => Match::Unknown,
                },
                "formType" => match g.data.sig(item).and_then(|s| form_type_code(&s)) {
                    Some(code) => number_set(value, i64::from(code)),
                    None => Match::Unknown,
                },
                "questItem" => match value.as_i64() {
                    Some(n @ (0 | 1)) => {
                        if g.world.is_quest_item(item) == (n == 1) {
                            Match::Yes
                        } else {
                            Match::No
                        }
                    }
                    _ => Match::Unknown,
                },
                // WEAP / ALCH / ARMO record predicates: every key name below is
                // [documented: Sorting Icons 1.4b package, menus/ySI/ySI.json]; no ySI/yUI
                // DLL or source code was consulted. Uncited keys (including shorthand names
                // such as `weaponAutomatic`/`weaponScope`) fall through to `Match::Unknown`.
                //
                // [documented: Sorting Icons 1.4b menus/ySI/ySI.json; schemas/fnv/objects.json WEAP.DNAM.skill;
                //  observed: FNV 1.4.0.525 + xNVSE 6.4.9 + yUI 140 (v1.4d): scalar integer only, arrays abort JSON]
                "weaponSkill" => weapon_scalar_int_field(g, item, "DNAM", "skill", value, explicit_form),
                // [documented: Sorting Icons 1.4b menus/ySI/ySI.json; schemas/fnv/objects.json WEAP.DNAM.animation_type;
                //  observed: FNV 1.4.0.525 + xNVSE 6.4.9 + yUI 140 (v1.4d): scalar and array]
                "weaponType" => weapon_int_field(g, item, "DNAM", "animation_type", value, explicit_form),
                // [documented: Sorting Icons 1.4b menus/ySI/ySI.json; schemas/fnv/objects.json WEAP.DNAM.flags1 bit 1 ("Is Automatic");
                //  observed: FNV 1.4.0.525 + xNVSE 6.4.9 + yUI 140 (v1.4d)]
                "weaponIsAutomatic" => weapon_flag1(g, item, 1 << 1, value, explicit_form),
                // [documented: Sorting Icons 1.4b menus/ySI/ySI.json; schemas/fnv/objects.json WEAP.DNAM.flags1 bit 2 ("Has Scope");
                //  observed: FNV 1.4.0.525 + xNVSE 6.4.9 + yUI 140 (v1.4d): 1=true, 0=false, -1 does not match false]
                "weaponHasScope" => weapon_flag1(g, item, 1 << 2, value, explicit_form),
                // [documented: Sorting Icons 1.4b menus/ySI/ySI.json; schemas/fnv/objects.json WEAP.DNAM.flags1 bit 0 ("Ignores Normal Weapon Resistance");
                //  observed: FNV 1.4.0.525 + xNVSE 6.4.9 + yUI 140 (v1.4d)]
                "weaponIgnoresDTDR" => weapon_flag1(g, item, 1 << 0, value, explicit_form),
                // [documented: Sorting Icons 1.4b menus/ySI/ySI.json; schemas/fnv/objects.json ALCH.ENIT.flags bit 1 (0x02, "Food Item");
                //  observed: FNV 1.4.0.525 + xNVSE 6.4.9 + yUI 140 (v1.4d): 1=true, -1 does not match false]
                "aidIsFood" => aid_flag(g, item, crate::data::IngestibleDef::FOOD, value, explicit_form),
                // [documented: Sorting Icons 1.4b menus/ySI/ySI.json; schemas/fnv/objects.json ALCH.ENIT.flags bit 2 (0x04, "Medicine");
                //  observed: FNV 1.4.0.525 + xNVSE 6.4.9 + yUI 140 (v1.4d)]
                "aidIsMedicine" => aid_flag(g, item, crate::data::IngestibleDef::MEDICINE, value, explicit_form),
                // [documented: Sorting Icons 1.4b menus/ySI/ySI.json; schemas/fnv/objects.json ALCH.ENIT.withdrawal_effect; hypothesis: non-null withdrawal form]
                "aidIsAddictive" => aid_addictive(g, item, value, explicit_form),
                // [documented: Sorting Icons 1.4b menus/ySI/ySI.json; GECK wiki GetArmorClass (1 Light, 2 Medium, 3 Heavy);
                //  observed: FNV 1.4.0.525 + xNVSE 6.4.9 + yUI 140 (v1.4d)]
                "armorClass" => armor_class_match(g, item, value, explicit_form),
                // [documented: Sorting Icons 1.4b menus/ySI/ySI.json; schemas/fnv/objects.json ARMO.BMDT.general_flags bit 5 / GECK wiki IsPowerArmor;
                //  observed: FNV 1.4.0.525 + xNVSE 6.4.9 + yUI 140 (v1.4d): 1=true, -1 does not match false]
                "armorPower" => armor_power_match(g, item, value, explicit_form),
                // 11 ARMO biped-slot keys, each individually cited in `armor_biped_bit`.
                slot if armor_biped_bit(slot).is_some() => {
                    armor_biped_match(g, item, armor_biped_bit(slot).expect("checked"), value, explicit_form)
                }
                _ => crate::ysi_predicates::eval(g, item, &self.fields, name, value, explicit_form),
            };
            if part == Match::No {
                return Match::No;
            }
            if part == Match::Unknown {
                answer = Match::Unknown;
            }
        }
        // A mod or formlist selector without its explicit form has no established meaning here.
        if (self.fields.contains_key("mod") || self.fields.contains_key("formlist"))
            && !self.fields.contains_key("form")
        {
            return Match::Unknown;
        }
        if crate::ysi_predicates::has_dangling_companions(&self.fields) {
            return Match::Unknown;
        }
        answer
    }

    fn resolve_mod_index(
        g: &Game<'_>,
        name: &str,
        mod_index: Option<&HashMap<String, usize>>,
    ) -> Option<usize> {
        if let Some(map) = mod_index {
            map.get(&name.to_ascii_lowercase()).copied()
        } else {
            g.data
                .load_order
                .iter()
                .position(|p| p.eq_ignore_ascii_case(name))
        }
    }

    fn explicit_form(
        &self,
        g: &Game<'_>,
        item: FormId,
        value: &Json,
        mod_index: Option<&HashMap<String, usize>>,
    ) -> Match {
        let Some(name) = self.fields.get("mod").and_then(Json::as_str) else {
            return Match::Unknown;
        };
        let Some(index) = Self::resolve_mod_index(g, name, mod_index) else {
            return Match::No;
        };
        let matches = |v: &Json| {
            let Some(local) = parse_local_form(v) else {
                return Match::Unknown;
            };
            if index > 255 {
                return Match::Unknown;
            }
            if item.0 == (((index as u32) << 24) | local) {
                Match::Yes
            } else {
                Match::No
            }
        };
        match value {
            Json::Array(values) => any(values.iter().map(matches)),
            _ => matches(value),
        }
    }

    fn formlist_member(
        &self,
        g: &Game<'_>,
        item: FormId,
        value: &Json,
        mod_index: Option<&HashMap<String, usize>>,
    ) -> Match {
        let Some(name) = self.fields.get("mod").and_then(Json::as_str) else {
            return Match::Unknown;
        };
        let Some(index) = Self::resolve_mod_index(g, name, mod_index) else {
            return Match::No;
        };
        let matches = |v: &Json| {
            let Some(local) = parse_local_form(v) else {
                return Match::Unknown;
            };
            if index > 255 {
                return Match::Unknown;
            }
            let list_id = FormId(((index as u32) << 24) | local);
            match g.data.sig(list_id) {
                Some(sig) if &sig == b"FLST" => {
                    if crate::functions::refs::list_members(g, list_id).contains(&item) {
                        Match::Yes
                    } else {
                        Match::No
                    }
                }
                _ => Match::Unknown,
            }
        };
        match value {
            Json::Array(values) => any(values.iter().map(matches)),
            _ => matches(value),
        }
    }
}

pub(crate) fn parse_local_form(v: &Json) -> Option<u32> {
    let text = v.as_str()?;
    u32::from_str_radix(
        text.strip_prefix("0x")
            .or_else(|| text.strip_prefix("0X"))
            .unwrap_or(text),
        16,
    )
    .ok()
    .filter(|n| *n <= 0x00ff_ffff)
}

pub(crate) fn any(values: impl Iterator<Item = Match>) -> Match {
    let mut answer = Match::No;
    for v in values {
        if v == Match::Yes {
            return Match::Yes;
        }
        if v == Match::Unknown {
            answer = Match::Unknown;
        }
    }
    answer
}
pub(crate) fn number_set(value: &Json, number: i64) -> Match {
    let one = |v: &Json| match v.as_i64() {
        Some(n) if n == number => Match::Yes,
        Some(_) => Match::No,
        None => Match::Unknown,
    };
    match value {
        Json::Array(values) => any(values.iter().map(one)),
        _ => one(value),
    }
}

/// Match a boolean ySI predicate: `1` = required true, `0` = required false [observed:
/// FNV 1.4.0.525 + xNVSE 6.4.9 + yUI 140 (v1.4d) on `weaponHasScope`], JSON `true`/`false`
/// [hypothesis], and `-1` = required false only when `allow_neg_one` is true (armor
/// biped-slot rules [documented: Sorting Icons 1.4b menus/ySI/ySI.json uses `1` and `-1`
/// on armor slot rules such as `"armorHat": 1, "armorMask": -1`]; on non-slot flags `-1`
/// was observed not to match false in `yUI 1.4d` and stays `Match::Unknown`). Any other
/// JSON value remains `Match::Unknown`.
pub(crate) fn bool_pred(value: &Json, actual: bool, allow_neg_one: bool) -> Match {
    if let Some(b) = value.as_bool() {
        return if actual == b { Match::Yes } else { Match::No };
    }
    match value.as_i64() {
        Some(1) => if actual { Match::Yes } else { Match::No },
        Some(0) => if !actual { Match::Yes } else { Match::No },
        Some(-1) if allow_neg_one => if !actual { Match::Yes } else { Match::No },
        _ => Match::Unknown,
    }
}

fn mismatched_type(explicit_form: bool) -> Match {
    if explicit_form {
        Match::Yes
    } else {
        Match::No
    }
}

fn weapon_scalar_int_field(g: &Game<'_>, item: FormId, sub: &str, path: &str, value: &Json, explicit_form: bool) -> Match {
    if g.data.sig(item) != Some(*b"WEAP") {
        return mismatched_type(explicit_form);
    }
    let Some(expected) = value.as_i64() else {
        return Match::Unknown;
    };
    match crate::nvse::record_value(g, item, sub, path) {
        Some(XVal::Num(n)) if n.is_finite() => {
            if n.trunc() as i64 == expected {
                Match::Yes
            } else {
                Match::No
            }
        }
        _ => Match::Unknown,
    }
}

fn weapon_int_field(g: &Game<'_>, item: FormId, sub: &str, path: &str, value: &Json, explicit_form: bool) -> Match {
    if g.data.sig(item) != Some(*b"WEAP") {
        return mismatched_type(explicit_form);
    }
    match crate::nvse::record_value(g, item, sub, path) {
        Some(XVal::Num(n)) if n.is_finite() => number_set(value, n.trunc() as i64),
        _ => Match::Unknown,
    }
}

fn weapon_flag1(g: &Game<'_>, item: FormId, bit: u8, value: &Json, explicit_form: bool) -> Match {
    if g.data.sig(item) != Some(*b"WEAP") {
        return mismatched_type(explicit_form);
    }
    let Some(flags) = g.weapon_flags1(item) else {
        return Match::No;
    };
    bool_pred(value, flags & bit != 0, false)
}

fn aid_flag(g: &Game<'_>, item: FormId, bit: u8, value: &Json, explicit_form: bool) -> Match {
    if g.data.sig(item) != Some(*b"ALCH") {
        return mismatched_type(explicit_form);
    }
    let Some(def) = g.ingestible_def(item) else {
        return Match::Unknown;
    };
    bool_pred(value, def.flags & bit != 0, false)
}

fn aid_addictive(g: &Game<'_>, item: FormId, value: &Json, explicit_form: bool) -> Match {
    if g.data.sig(item) != Some(*b"ALCH") {
        return mismatched_type(explicit_form);
    }
    let Some(def) = g.ingestible_def(item) else {
        return Match::Unknown;
    };
    bool_pred(value, def.withdrawal.is_some(), false)
}

fn armor_class_match(g: &Game<'_>, item: FormId, value: &Json, explicit_form: bool) -> Match {
    if g.data.sig(item) != Some(*b"ARMO") {
        return mismatched_type(explicit_form);
    }
    number_set(value, i64::from(g.armor_class(item)))
}

fn armor_power_match(g: &Game<'_>, item: FormId, value: &Json, explicit_form: bool) -> Match {
    if g.data.sig(item) != Some(*b"ARMO") {
        return mismatched_type(explicit_form);
    }
    let is_power = g
        .other
        .power_armor
        .get(&item)
        .copied()
        .unwrap_or_else(|| g.world.effective_power_armor(item));
    bool_pred(value, is_power, false)
}

/// ARMO `BMDT/biped_flags` bit index for each public ySI armor-slot predicate key.
/// Every key below is individually [documented: Sorting Icons 1.4b package,
/// `menus/ySI/ySI.json`]; bit indices are [documented: `schemas/fnv/objects.json`
/// `ARMO.BMDT.biped_flags`]. No ySI/yUI DLL or source code was consulted.
fn armor_biped_bit(name: &str) -> Option<u32> {
    Some(match name {
        // [documented: Sorting Icons 1.4b menus/ySI/ySI.json; ARMO.BMDT.biped_flags bit 0 "Head"]
        "armorHead" => 0,
        // [documented: Sorting Icons 1.4b menus/ySI/ySI.json; ARMO.BMDT.biped_flags bit 1 "Hair"]
        "armorHair" => 1,
        // [documented: Sorting Icons 1.4b menus/ySI/ySI.json; ARMO.BMDT.biped_flags bit 2 "Upper Body"]
        "armorUpperBody" => 2,
        // [documented: Sorting Icons 1.4b menus/ySI/ySI.json; ARMO.BMDT.biped_flags bit 6 "PipBoy"]
        "armorPipBoy" => 6,
        // [documented: Sorting Icons 1.4b menus/ySI/ySI.json; ARMO.BMDT.biped_flags bit 7 "Backpack"]
        "armorBackpack" => 7,
        // [documented: Sorting Icons 1.4b menus/ySI/ySI.json; ARMO.BMDT.biped_flags bit 9 "Headband"]
        "armorHeadband" => 9,
        // [documented: Sorting Icons 1.4b menus/ySI/ySI.json; ARMO.BMDT.biped_flags bit 10 "Hat"]
        "armorHat" => 10,
        // [documented: Sorting Icons 1.4b menus/ySI/ySI.json; ARMO.BMDT.biped_flags bit 11 "Eye Glasses"]
        "armorEyeglasses" => 11,
        // [documented: Sorting Icons 1.4b menus/ySI/ySI.json; ARMO.BMDT.biped_flags bit 14 "Mask"]
        "armorMask" => 14,
        // [documented: Sorting Icons 1.4b menus/ySI/ySI.json; ARMO.BMDT.biped_flags bit 15 "Choker"]
        "armorChoker" => 15,
        // [documented: Sorting Icons 1.4b menus/ySI/ySI.json; ARMO.BMDT.biped_flags bit 16 "Mouth Object"]
        "armorMouthObject" => 16,
        _ => return None,
    })
}

fn armor_biped_match(g: &Game<'_>, item: FormId, bit: u32, value: &Json, explicit_form: bool) -> Match {
    if g.data.sig(item) != Some(*b"ARMO") {
        return mismatched_type(explicit_form);
    }
    let mask = match crate::nvse::record_value(g, item, "BMDT", "biped_flags") {
        Some(XVal::Num(n)) if n.is_finite() => n.trunc() as i64 as u64,
        _ => 0,
    };
    bool_pred(value, (mask >> bit) & 1 != 0, true)
}

/// ySI-local commented configuration, leaving generic strict JSON APIs unchanged.
fn commented_json(text: &str) -> Result<String, String> {
    let bytes = text.trim_start_matches('\u{feff}').as_bytes();
    let mut out = bytes.to_vec();
    let (mut i, mut string, mut escape) = (0, false, false);
    while i < bytes.len() {
        if string {
            if escape {
                escape = false;
            } else if bytes[i] == b'\\' {
                escape = true;
            } else if bytes[i] == b'"' {
                string = false;
            }
            i += 1;
            continue;
        }
        if bytes[i] == b'"' {
            string = true;
            i += 1;
            continue;
        }
        if bytes[i..].starts_with(b"//") {
            while i < bytes.len() && bytes[i] != b'\n' {
                out[i] = b' ';
                i += 1;
            }
        } else if bytes[i..].starts_with(b"/*") {
            out[i] = b' ';
            out[i + 1] = b' ';
            i += 2;
            while i < bytes.len() && !bytes[i..].starts_with(b"*/") {
                if !matches!(bytes[i], b'\r' | b'\n') {
                    out[i] = b' ';
                }
                i += 1;
            }
            if i == bytes.len() {
                return Err("unterminated ySI configuration comment".into());
            }
            out[i] = b' ';
            out[i + 1] = b' ';
            i += 2;
        } else {
            i += 1;
        }
    }
    // Trailing commas are a mapped configuration extension, outside quoted strings.
    let (mut string, mut escape) = (false, false);
    for i in 0..out.len() {
        if string {
            if escape {
                escape = false;
            } else if out[i] == b'\\' {
                escape = true;
            } else if out[i] == b'"' {
                string = false;
            }
            continue;
        }
        if out[i] == b'"' {
            string = true;
            continue;
        }
        if out[i] == b','
            && out[i + 1..]
                .iter()
                .find(|b| !b.is_ascii_whitespace())
                .is_some_and(|b| matches!(*b, b'}' | b']'))
        {
            out[i] = b' ';
        }
    }
    String::from_utf8(out).map_err(|_| "invalid UTF-8 in ySI configuration".into())
}

#[cfg(test)]
#[path = "ysi_categories_tests.rs"]
mod ysi_categories_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_local_form_accepts_hex_and_rejects_out_of_range() {
        assert_eq!(parse_local_form(&Json::String("00000820".into())), Some(0x820));
        assert_eq!(parse_local_form(&Json::String("0x12AB".into())), Some(0x12AB));
        assert_eq!(parse_local_form(&Json::String("0X00FFFFFF".into())), Some(0x00FF_FFFF));
        assert_eq!(parse_local_form(&Json::String("01000000".into())), None);
        assert_eq!(parse_local_form(&Json::String("not-hex".into())), None);
        assert_eq!(parse_local_form(&Json::from(0x820)), None);
    }

    #[test]
    fn commented_json_strips_comments_trailing_commas_and_bom() {
        let input = "\u{feff}// lead\n{\"a\": \"http://x/*y*/\", /* mid */ \"b\": [1, 2,],}\n";
        let cleaned = commented_json(input).unwrap();
        let parsed: Json = serde_json::from_str(&cleaned).unwrap();
        assert_eq!(parsed["a"], "http://x/*y*/");
        assert_eq!(parsed["b"], serde_json::json!([1, 2]));
        assert!(commented_json("/* unclosed").is_err());
        assert_eq!(commented_json("// comment only\n/* block */").unwrap().trim(), "");
    }

    #[test]
    fn number_set_handles_scalars_arrays_and_unknowns() {
        assert_eq!(number_set(&Json::from(40), 40), Match::Yes);
        assert_eq!(number_set(&Json::from(40), 31), Match::No);
        assert_eq!(number_set(&serde_json::json!([31, 40]), 40), Match::Yes);
        assert_eq!(number_set(&serde_json::json!([31, "bad"]), 40), Match::Unknown);
        assert_eq!(number_set(&serde_json::json!([31, "bad"]), 31), Match::Yes);
    }

    #[test]
    fn bool_pred_and_biped_bits_handle_documented_values_and_reject_unknowns() {
        assert_eq!(bool_pred(&Json::from(1), true, true), Match::Yes);
        assert_eq!(bool_pred(&Json::from(1), false, true), Match::No);
        assert_eq!(bool_pred(&Json::from(true), true, false), Match::Yes);
        assert_eq!(bool_pred(&Json::from(true), false, false), Match::No);
        assert_eq!(bool_pred(&Json::from(false), false, false), Match::Yes);
        assert_eq!(bool_pred(&Json::from(false), true, false), Match::No);
        assert_eq!(bool_pred(&Json::from(0), false, true), Match::Yes);
        assert_eq!(bool_pred(&Json::from(-1), false, true), Match::Yes);
        assert_eq!(bool_pred(&Json::from(-1), true, true), Match::No);
        assert_eq!(bool_pred(&Json::from(-1), false, false), Match::Unknown);
        assert_eq!(bool_pred(&Json::from(-1), true, false), Match::Unknown);
        assert_eq!(bool_pred(&Json::from(2), false, true), Match::Unknown);
        assert_eq!(bool_pred(&Json::from("1"), true, true), Match::Unknown);
        assert_eq!(armor_biped_bit("armorHead"), Some(0));
        assert_eq!(armor_biped_bit("armorHat"), Some(10));
        assert_eq!(armor_biped_bit("armorMask"), Some(14));
        assert_eq!(armor_biped_bit("armorMouthObject"), Some(16));
        assert_eq!(armor_biped_bit("armorUnknown"), None);
    }

    #[test]
    fn compiled_tags_buckets_explicit_forms_form_types_and_drops_unloaded_mods() {
        let make_rule = |priority: i64, tag: &str, obj: Json| -> Rule {
            let mut fields = obj.as_object().cloned().unwrap();
            fields.insert("priority".into(), Json::from(priority));
            fields.insert("tag".into(), Json::from(tag));
            Rule { priority, tag: tag.to_string(), fields }
        };
        let mut book = Book {
            tags: vec![
                make_rule(500, "(explicit)", serde_json::json!({ "mod": "FalloutNV.esm", "form": ["00001000", "00002000"] })),
                make_rule(400, "(unloaded)", serde_json::json!({ "mod": "Missing.esp", "form": "00001000" })),
                make_rule(200, "(weap)", serde_json::json!({ "formType": 40 })),
                make_rule(100, "(generic)", serde_json::json!({ "name": "Fallback" })),
            ],
            icons: vec![
                make_rule(100, "(weap)", serde_json::json!({ "filename": "low.dds" })),
                make_rule(300, "(weap)", serde_json::json!({ "filename": "high.dds", "template": "ySIDefault" })),
            ],
            icons_by_tag: HashMap::new(),
        };
        book.tags.sort_by_key(|a| std::cmp::Reverse(a.priority));
        book.icons_by_tag.insert("(weap)".into(), book.compute_icon_spec("(weap)"));

        let compiled = CompiledTags::build(&book, &["FalloutNV.esm".into(), "DeadMoney.esm".into()]);
        assert_eq!(compiled.by_explicit_form.get(&FormId(0x0000_1000)).map(Vec::as_slice), Some(&[0][..]));
        assert_eq!(compiled.by_explicit_form.get(&FormId(0x0000_2000)).map(Vec::as_slice), Some(&[0][..]));
        assert_eq!(compiled.by_form_type.get(&40).map(Vec::as_slice), Some(&[2][..]));
        assert_eq!(compiled.any_type, vec![3]);
        assert_eq!(compiled.mod_index.get("falloutnv.esm"), Some(&0));
        assert_eq!(compiled.mod_index.get("deadmoney.esm"), Some(&1));
        assert_eq!(book.icon_spec("(weap)"), Ok(("high.dds".into(), Some("ySIDefault".into()))));
        assert!(book.icon_spec("(unknown)").is_err());
    }
}
