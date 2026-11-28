//! Archive and FOMOD XML install for the Mods page. The format, its sources and the
//! launcher's own choices are in `docs/spec-notes/fomod.md`.
//!
//! Packaging: `.zip`, `.rar`, `.7z`, and a `.fomod` that is a zip, as the Nexus Mods
//! App's developer note "About FOMOD" says (`docs/developers/misc/AboutFomod.md` in
//! Nexus-Mods/NexusMods.App); `.tar` and `.gz` too. One nested archive (an outer
//! archive whose only file is another archive) is opened. `fomod/script.cs` is not
//! compiled or run.
//!
//! The XML installer is one step on screen at a time. A group is
//! `SelectAtLeastOne`, `SelectAtMostOne`, `SelectExactlyOne`, `SelectAll`, or
//! `SelectAny`. An option is `Required`, `Optional`, `Recommended`, `NotUsable`,
//! or `CouldBeUsable`. Flags set by a chosen option decide which later steps show
//! and which conditional files are copied. Where two files land on the same path,
//! the higher `priority` is copied last and is the one kept. Equal priority keeps
//! the later file in the plan's order. The schema leaves that open, and the note
//! above says the format is "fuzzy" on priorities.

use eframe::egui::{self, Align2, RichText, ScrollArea};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

const LOOSE: &[&str] = &[
    "meshes", "textures", "sound", "music", "video", "menus", "interface", "scripts", "facegen", "shaders", "lodsettings", "trees", "materials", "distantlod", "lsdata", "nvse", "config", "uio", "plugins",
];

#[allow(clippy::large_enum_variant)]
pub enum Staged {
    Tree { name: String, from: PathBuf, temp: Option<PathBuf> },
    Plan(Plan),
    Wizard(Wizard),
    /// The archive has more than one folder that could be the mod. The player picks one.
    ChooseRoot { name: String, roots: Vec<PathBuf>, temp: Option<PathBuf> },
}

pub struct Plan {
    pub name: String,
    pub root: PathBuf,
    pub items: Vec<Item>,
    pub temp: Option<PathBuf>,
}

pub struct Wizard {
    pub title: String,
    pub folder_name: String,
    pub note: String,
    pub error: Option<String>,
    root: PathBuf,
    temp: Option<PathBuf>,
    steps: Vec<Step>,
    required: Vec<Item>,
    patterns: Vec<When>,
    chosen: Vec<Vec<Vec<bool>>>,
    at: usize,
    facts: Facts,
    image: Option<PathBuf>,
}

struct Step {
    name: String,
    visible: Option<Dep>,
    groups: Vec<Group>,
}

struct Group {
    name: String,
    kind: GroupKind,
    plugins: Vec<Plug>,
}

struct Plug {
    name: String,
    description: String,
    image: Option<PathBuf>,
    files: Vec<Item>,
    flags: Vec<(String, String)>,
    typed: TypeSpec,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GroupKind {
    Any,
    All,
    ExactlyOne,
    AtMostOne,
    AtLeastOne,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Required,
    Optional,
    Recommended,
    NotUsable,
    CouldBeUsable,
}

enum TypeSpec {
    Fixed(Kind),
    Depends { default: Kind, patterns: Vec<(Dep, Kind)> },
}

enum Dep {
    Flag { flag: String, value: String },
    File { file: String, state: String },
    /// A game-version check. The launcher does not read the game version, so this passes.
    Game,
    All(Vec<Dep>),
    Any(Vec<Dep>),
}

#[derive(Clone)]
pub struct Item {
    source: String,
    destination: String,
    /// The installer's own `priority` (0 when it gives none). Copied lowest first.
    priority: i32,
    folder: bool,
    /// The item's place in the plan (`Wizard::plan`): between equal priorities, a later one
    /// is copied later and kept. Kept apart from `priority`, so no number of files moves an
    /// item past a higher priority.
    order: usize,
}

struct When {
    when: Dep,
    files: Vec<Item>,
}

struct Facts {
    active: HashSet<String>,
    present: HashSet<String>,
    paths: HashSet<String>,
}

pub enum Decision {
    Cancel,
    Install,
}

pub fn stage(src: &Path, facts: FactsInput) -> Result<Staged, String> {
    if src.as_os_str().is_empty() || !src.exists() {
        return Err("That path was not found.".into());
    }
    let facts = Facts::from_input(facts);
    if src.is_dir() {
        return stage_tree(src, None, src.file_name().and_then(|n| n.to_str()).unwrap_or("mod"), facts);
    }
    if !is_archive(src) {
        let file_name = src.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if is_game_file(file_name) {
            let temp = make_temp()?;
            if let Err(e) = std::fs::copy(src, temp.join(file_name)) {
                remove_temp(&temp);
                return Err(format!("{}: {e}", src.display()));
            }
            let stem = src.file_stem().and_then(|n| n.to_str()).unwrap_or("mod");
            return stage_tree(&temp, Some(temp.clone()), stem, facts);
        }
        return Err("Drop a folder or an archive (.zip, .7z, .rar, .fomod, .tar, .gz).".into());
    }
    let temp = make_temp()?;
    let limit = unpack_limit(src);
    if let Err(e) = extract_archive(src, &temp, limit) {
        remove_temp(&temp);
        return Err(e);
    }
    if let Err(e) = unwrap_nested(&temp, limit) {
        remove_temp(&temp);
        return Err(e);
    }
    let stem = archive_stem(src);
    stage_tree(&temp, Some(temp.clone()), &stem, facts)
}

pub struct FactsInput {
    pub active: HashSet<String>,
    pub present: HashSet<String>,
    pub paths: HashSet<String>,
}

impl Facts {
    fn from_input(input: FactsInput) -> Facts {
        Facts {
            active: input.active.into_iter().map(|s| s.to_ascii_lowercase()).collect(),
            present: input.present.into_iter().map(|s| s.to_ascii_lowercase()).collect(),
            paths: input.paths.into_iter().map(|s| s.replace('\\', "/").to_ascii_lowercase()).collect(),
        }
    }
}

fn stage_tree(tree: &Path, temp: Option<PathBuf>, fallback: &str, facts: Facts) -> Result<Staged, String> {
    if fomod_script(tree).is_some() && find_config(tree).is_none() {
        discard(&temp);
        return Err("This installer is a C# script (fomod/script.cs). It is not run.".into());
    }
    if let Some(xml) = find_config(tree) {
        let root = xml.parent().and_then(|p| p.parent()).unwrap_or(tree).to_path_buf();
        let text = match read_xml(&xml) {
            Ok(t) => t,
            Err(e) => {
                discard(&temp);
                return Err(e);
            }
        };
        let wizard = match parse_wizard(&text, &root, temp.clone(), fallback, facts) {
            Ok(w) => w,
            Err(e) => {
                discard(&temp);
                return Err(e);
            }
        };
        if !wizard.needs_choice() {
            let name = wizard.folder_name.clone();
            let items = wizard.plan();
            return Ok(Staged::Plan(Plan { name, root, items, temp }));
        }
        return Ok(Staged::Wizard(wizard));
    }
    let from = hoist(tree);
    let name = clean_name(fallback).unwrap_or_else(|_| "mod".into());
    // One nested folder is already unwrapped. Several folders that each look like a mod stay a choice.
    if !has_top_files(&from) {
        let roots: Vec<PathBuf> = subdirs(&from).into_iter().filter(|p| has_game_files(p)).collect();
        if roots.len() > 1 {
            return Ok(Staged::ChooseRoot { name, roots, temp });
        }
    }
    Ok(Staged::Tree { name, from, temp })
}

pub fn place_tree(mods: &Path, name: &str, from: &Path) -> Result<String, String> {
    let name = clean_name(name)?;
    let dest = mods.join(&name);
    if dest.exists() {
        return Err("That folder is already in the mods folder.".into());
    }
    if let Err(e) = copy_tree(from, &dest) {
        let _ = std::fs::remove_dir_all(&dest);
        return Err(e);
    }
    if let Err(e) = surface_hidden_plugins(&dest) {
        let _ = std::fs::remove_dir_all(&dest);
        return Err(e);
    }
    keep_if_game_files(&dest, &name)
}

pub fn place_plan(mods: &Path, plan: &Plan) -> Result<String, String> {
    let name = clean_name(&plan.name)?;
    let dest = mods.join(&name);
    if dest.exists() {
        return Err("That folder is already in the mods folder.".into());
    }
    std::fs::create_dir_all(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    if let Err(e) = copy_items(&plan.root, &dest, &plan.items) {
        let _ = std::fs::remove_dir_all(&dest);
        return Err(e);
    }
    if let Err(e) = surface_hidden_plugins(&dest) {
        let _ = std::fs::remove_dir_all(&dest);
        return Err(e);
    }
    keep_if_game_files(&dest, &name)
}

/// Installs over a mod of the same name. The old folder is moved into `recycle` first and put back if the new copy fails.
pub fn replace_tree(mods: &Path, name: &str, from: &Path, recycle: &Path) -> Result<String, String> {
    let name = clean_name(name)?;
    let dest = mods.join(&name);
    let aside = move_aside(&dest, recycle, &name)?;
    match place_tree(mods, &name, from) {
        Ok(done) => Ok(done),
        Err(e) => {
            restore_aside(aside, &dest);
            Err(e)
        }
    }
}

pub fn replace_plan(mods: &Path, plan: &Plan, recycle: &Path) -> Result<String, String> {
    let name = clean_name(&plan.name)?;
    let dest = mods.join(&name);
    let aside = move_aside(&dest, recycle, &name)?;
    let mut placed = plan.clone_for_place();
    placed.name = name;
    match place_plan(mods, &placed) {
        Ok(done) => Ok(done),
        Err(e) => {
            restore_aside(aside, &dest);
            Err(e)
        }
    }
}

fn move_aside(dest: &Path, recycle: &Path, name: &str) -> Result<Option<PathBuf>, String> {
    if !dest.exists() {
        return Ok(None);
    }
    std::fs::create_dir_all(recycle).map_err(|e| format!("{}: {e}", recycle.display()))?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let mut aside = recycle.join(format!("{name} {stamp}"));
    let mut n = 2u32;
    while aside.exists() {
        aside = recycle.join(format!("{name} {stamp}-{n}"));
        n += 1;
    }
    std::fs::rename(dest, &aside).map_err(|e| format!("{}: {e}", dest.display()))?;
    Ok(Some(aside))
}

fn restore_aside(aside: Option<PathBuf>, dest: &Path) {
    if let Some(aside) = aside {
        let _ = std::fs::rename(&aside, dest);
    }
}

/// The most paths `plan_loose_files` lists: an installer can name one folder many times, and the
/// list only feeds the clash warning.
const PLAN_LIST_MAX: usize = 100_000;

/// Loose-file paths a plan would write, using `/` and no drive; at most `PLAN_LIST_MAX` of them.
pub fn plan_loose_files(plan: &Plan) -> Vec<String> {
    let mut out = Vec::new();
    'items: for item in &plan.items {
        let Ok(rel) = rel_path(&item.source) else { continue };
        let src = plan.root.join(&rel);
        if item.folder {
            let mut stack = vec![(src, String::new())];
            while let Some((dir, prefix)) = stack.pop() {
                let Ok(rd) = std::fs::read_dir(&dir) else { continue };
                for entry in rd.flatten() {
                    let path = entry.path();
                    if path.is_symlink() {
                        continue;
                    }
                    let name = entry.file_name().to_string_lossy().to_string();
                    let next = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
                    if path.is_dir() {
                        stack.push((path, next));
                    } else {
                        if out.len() >= PLAN_LIST_MAX {
                            break 'items;
                        }
                        let key = if item.destination.trim().is_empty() { next } else { format!("{}/{}", item.destination.trim_end_matches(['/', '\\']), next) };
                        out.push(key.replace('\\', "/"));
                    }
                }
            }
        } else {
            if out.len() >= PLAN_LIST_MAX {
                break 'items;
            }
            let named = src.file_name().unwrap_or_default().to_string_lossy();
            let key = if item.destination.trim().is_empty() { named.to_string() } else { item.destination.replace('\\', "/") };
            out.push(key);
        }
    }
    out.sort();
    out.dedup();
    out
}

pub fn discard(temp: &Option<PathBuf>) {
    if let Some(t) = temp {
        remove_temp(t);
    }
}

fn keep_if_game_files(dest: &Path, name: &str) -> Result<String, String> {
    if !has_game_files(dest) {
        let _ = std::fs::remove_dir_all(dest);
        return Err("That archive had no plugin and no game files, so nothing was added. Use Manual to pick the files.".into());
    }
    Ok(name.to_string())
}

impl Plan {
    fn clone_for_place(&self) -> Plan {
        Plan { name: self.name.clone(), root: self.root.clone(), items: self.items.clone(), temp: None }
    }
}

impl Wizard {
    pub fn discard(&self) {
        discard(&self.temp);
    }

    #[allow(dead_code)]
    pub fn install(&mut self, mods: &Path) -> Result<String, String> {
        self.install_over(mods, None)
    }

    /// Same as [`install`], and a folder of this name already in `mods` is moved to `recycle` first.
    pub fn install_over(&mut self, mods: &Path, recycle: Option<&Path>) -> Result<String, String> {
        if let Some(e) = self.validate_all() {
            return Err(e);
        }
        let plan = Plan { name: self.folder_name.clone(), root: self.root.clone(), items: self.plan(), temp: None };
        match recycle {
            Some(recycle) => replace_plan(mods, &plan, recycle),
            None => place_plan(mods, &plan),
        }
    }

    /// The choices, so a later install of this mod can start from them.
    pub fn choices_text(&self) -> String {
        let mut out = format!("folder={}\n", self.folder_name);
        for (s, step) in self.chosen.iter().enumerate() {
            for (g, group) in step.iter().enumerate() {
                for (p, on) in group.iter().enumerate() {
                    if *on {
                        out.push_str(&format!("{s} {g} {p}\n"));
                    }
                }
            }
        }
        out
    }

    pub fn apply_choices(&mut self, text: &str) {
        for step in &mut self.chosen {
            for group in step.iter_mut() {
                for on in group.iter_mut() {
                    *on = false;
                }
            }
        }
        for line in text.lines() {
            let line = line.trim();
            if let Some(name) = line.strip_prefix("folder=") {
                if clean_name(name).is_ok() {
                    self.folder_name = name.to_string();
                }
                continue;
            }
            let mut parts = line.split_whitespace();
            let (Some(s), Some(g), Some(p)) = (parts.next(), parts.next(), parts.next()) else { continue };
            let (Ok(s), Ok(g), Ok(p)) = (s.parse::<usize>(), g.parse::<usize>(), p.parse::<usize>()) else { continue };
            if let Some(flag) = self.chosen.get_mut(s).and_then(|step| step.get_mut(g)).and_then(|group| group.get_mut(p)) {
                *flag = true;
            }
        }
        self.normalize();
    }

    fn needs_choice(&self) -> bool {
        let visible = self.visible_steps();
        let kinds = self.kinds();
        for (s, step) in self.steps.iter().enumerate() {
            if !visible[s] {
                continue;
            }
            for (g, group) in step.groups.iter().enumerate() {
                let usable: Vec<Kind> = group.plugins.iter().enumerate().map(|(p, _)| kinds[s][g][p]).filter(|k| *k != Kind::NotUsable).collect();
                match group.kind {
                    GroupKind::All => {}
                    GroupKind::ExactlyOne | GroupKind::AtLeastOne if usable.len() > 1 => return true,
                    GroupKind::AtMostOne | GroupKind::Any if !usable.is_empty() => return true,
                    _ => {}
                }
            }
        }
        false
    }

    fn plan(&self) -> Vec<Item> {
        let visible = self.visible_steps();
        let kinds = self.kinds();
        // In plan order: the required files, the chosen options' files by step, group and
        // option, then the conditional installs by pattern. Sorted by priority, then that order.
        let mut items: Vec<Item> = self.required.clone();
        for (s, step) in self.steps.iter().enumerate() {
            if !visible[s] {
                continue;
            }
            for (g, group) in step.groups.iter().enumerate() {
                for (p, plug) in group.plugins.iter().enumerate() {
                    if !self.chosen[s][g][p] || kinds[s][g][p] == Kind::NotUsable {
                        continue;
                    }
                    items.extend(plug.files.iter().cloned());
                }
            }
        }
        let flags = self.flags();
        for when in &self.patterns {
            if when.when.holds(&flags, &self.facts) {
                items.extend(when.files.iter().cloned());
            }
        }
        for (order, item) in items.iter_mut().enumerate() {
            item.order = order;
        }
        items.sort_by_key(|item| (item.priority, item.order));
        items
    }

    fn flags(&self) -> HashMap<String, String> {
        let mut flags = HashMap::new();
        let mut visible = vec![true; self.steps.len()];
        for _ in 0..8 {
            flags.clear();
            let kinds = self.kinds_with(&visible);
            for (s, step) in self.steps.iter().enumerate() {
                if !visible[s] {
                    continue;
                }
                for (g, group) in step.groups.iter().enumerate() {
                    for (p, plug) in group.plugins.iter().enumerate() {
                        if self.chosen[s][g][p] && kinds[s][g][p] != Kind::NotUsable {
                            for (name, value) in &plug.flags {
                                flags.insert(name.to_ascii_lowercase(), value.clone());
                            }
                        }
                    }
                }
            }
            let next: Vec<bool> = self.steps.iter().map(|step| step.visible.as_ref().map(|d| d.holds(&flags, &self.facts)).unwrap_or(true)).collect();
            if next == visible {
                break;
            }
            visible = next;
        }
        flags
    }

    fn visible_steps(&self) -> Vec<bool> {
        let flags = self.flags();
        self.steps.iter().map(|step| step.visible.as_ref().map(|d| d.holds(&flags, &self.facts)).unwrap_or(true)).collect()
    }

    fn kinds(&self) -> Vec<Vec<Vec<Kind>>> {
        self.kinds_with(&self.visible_steps())
    }

    fn kinds_with(&self, visible: &[bool]) -> Vec<Vec<Vec<Kind>>> {
        let flags = {
            let mut flags = HashMap::new();
            for (s, step) in self.steps.iter().enumerate() {
                if !visible.get(s).copied().unwrap_or(false) {
                    continue;
                }
                for (g, group) in step.groups.iter().enumerate() {
                    for (p, plug) in group.plugins.iter().enumerate() {
                        if self.chosen[s][g][p] {
                            for (name, value) in &plug.flags {
                                flags.insert(name.to_ascii_lowercase(), value.clone());
                            }
                        }
                    }
                }
            }
            flags
        };
        self.steps.iter().map(|step| step.groups.iter().map(|group| group.plugins.iter().map(|plug| plug.typed.resolve(&flags, &self.facts)).collect()).collect()).collect()
    }

    fn normalize(&mut self) {
        for _ in 0..8 {
            let visible = self.visible_steps();
            let kinds = self.kinds_with(&visible);
            let mut changed = false;
            for (s, step) in self.steps.iter().enumerate() {
                for (g, group) in step.groups.iter().enumerate() {
                    for (p, _) in group.plugins.iter().enumerate() {
                        let kind = kinds[s][g][p];
                        let want = match (group.kind, kind) {
                            (_, Kind::NotUsable) => Some(false),
                            (_, Kind::Required) => Some(true),
                            (GroupKind::All, _) => Some(true),
                            _ => None,
                        };
                        if let Some(on) = want {
                            if self.chosen[s][g][p] != on {
                                self.chosen[s][g][p] = on;
                                changed = true;
                            }
                        }
                    }
                    if matches!(group.kind, GroupKind::ExactlyOne | GroupKind::AtLeastOne) {
                        let usable: Vec<usize> = group.plugins.iter().enumerate().filter(|(p, _)| kinds[s][g][*p] != Kind::NotUsable).map(|(p, _)| p).collect();
                        let picked = usable.iter().filter(|p| self.chosen[s][g][**p]).count();
                        if picked == 0 {
                            if let Some(p) = default_pick(&group.plugins, &kinds[s][g]) {
                                self.chosen[s][g][p] = true;
                                changed = true;
                            }
                        } else if group.kind == GroupKind::ExactlyOne && picked > 1 {
                            let keep = usable.into_iter().find(|p| self.chosen[s][g][*p]);
                            for (p, slot) in self.chosen[s][g].iter_mut().enumerate().take(group.plugins.len()) {
                                let on = Some(p) == keep;
                                if *slot != on {
                                    *slot = on;
                                    changed = true;
                                }
                            }
                        }
                    }
                }
            }
            if !changed {
                break;
            }
        }
    }

    fn validate_all(&mut self) -> Option<String> {
        self.normalize();
        let visible = self.visible_steps();
        let kinds = self.kinds();
        for (s, step) in self.steps.iter().enumerate() {
            if !visible[s] {
                continue;
            }
            for (g, group) in step.groups.iter().enumerate() {
                if let Some(e) = group_error(group, &self.chosen[s][g], &kinds[s][g]) {
                    return Some(e);
                }
            }
        }
        clean_name(&self.folder_name).err()
    }
}

fn default_pick(plugins: &[Plug], kinds: &[Kind]) -> Option<usize> {
    plugins.iter().enumerate().find(|(p, _)| kinds[*p] == Kind::Recommended).map(|(p, _)| p).or_else(|| plugins.iter().enumerate().find(|(p, _)| kinds[*p] == Kind::Required).map(|(p, _)| p)).or_else(|| plugins.iter().enumerate().find(|(p, _)| kinds[*p] != Kind::NotUsable).map(|(p, _)| p))
}

fn group_error(group: &Group, chosen: &[bool], kinds: &[Kind]) -> Option<String> {
    let n = group.plugins.iter().enumerate().filter(|(p, _)| chosen[*p] && kinds[*p] != Kind::NotUsable).count();
    let name = &group.name;
    match group.kind {
        GroupKind::ExactlyOne if n != 1 => Some(format!("Choose one in {name}.")),
        GroupKind::AtLeastOne if n < 1 => Some(format!("Choose at least one in {name}.")),
        GroupKind::AtMostOne if n > 1 => Some(format!("Choose at most one in {name}.")),
        _ => None,
    }
}

fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Required => "required",
        Kind::Optional => "optional",
        Kind::Recommended => "recommended",
        Kind::NotUsable => "not usable",
        Kind::CouldBeUsable => "could be usable",
    }
}

fn group_label(kind: GroupKind) -> &'static str {
    match kind {
        GroupKind::Any => "Any number",
        GroupKind::All => "All of these",
        GroupKind::ExactlyOne => "Choose one",
        GroupKind::AtMostOne => "Choose at most one",
        GroupKind::AtLeastOne => "Choose at least one",
    }
}

pub fn draw(ctx: &egui::Context, w: &mut Wizard) -> Option<Decision> {
    w.normalize();
    let visible = w.visible_steps();
    let ids: Vec<usize> = visible.iter().enumerate().filter(|(_, on)| **on).map(|(i, _)| i).collect();
    if w.at >= ids.len() && !ids.is_empty() {
        w.at = ids.len() - 1;
    }
    let kinds = w.kinds();
    let mut decision = None;
    let mut open = true;
    let title = w.title.clone();
    let screen = ctx.viewport_rect().size();
    egui::Window::new(title)
        .collapsible(false)
        .resizable(true)
        .constrain(true)
        .default_width(540.0)
        .max_size(egui::vec2((screen.x - 32.0).max(280.0), (screen.y - 32.0).max(200.0)))
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .open(&mut open)
        .show(ctx, |ui| {
        if !w.note.is_empty() {
            ui.label(RichText::new(&w.note).small().color(ui.visuals().weak_text_color()));
        }
        if let Some(uri) = w.image.as_deref().and_then(file_uri) {
            ui.add(egui::Image::from_uri(uri).max_height(120.0));
        }
        if ids.is_empty() {
            ui.label("Nothing to choose. The files that always install can be copied.");
        } else {
            let s = ids[w.at];
            ui.label(RichText::new(format!("Step {} of {}: {}", w.at + 1, ids.len(), w.steps[s].name)).strong());
            ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
                for (g, group) in w.steps[s].groups.iter().enumerate() {
                    ui.add_space(8.0);
                    ui.label(RichText::new(format!("{}  ({})", group.name, group_label(group.kind))).strong());
                    match group.kind {
                        GroupKind::ExactlyOne | GroupKind::AtMostOne => {
                            if group.kind == GroupKind::AtMostOne && ui.radio(group.plugins.iter().enumerate().all(|(p, _)| !w.chosen[s][g][p] || kinds[s][g][p] == Kind::NotUsable), "None").clicked() {
                                w.chosen[s][g].fill(false);
                            }
                            for (p, plug) in group.plugins.iter().enumerate() {
                                let kind = kinds[s][g][p];
                                let on = w.chosen[s][g][p] && kind != Kind::NotUsable;
                                let text = format!("{} ({})", plug.name, kind_label(kind));
                                if kind == Kind::NotUsable {
                                    ui.add_enabled(false, egui::RadioButton::new(false, text));
                                } else if ui.radio(on, text).clicked() {
                                    for (i, flag) in w.chosen[s][g].iter_mut().enumerate() {
                                        *flag = i == p;
                                    }
                                }
                                option_body(ui, plug);
                            }
                        }
                        _ => {
                            for (p, plug) in group.plugins.iter().enumerate() {
                                let kind = kinds[s][g][p];
                                let locked = kind == Kind::NotUsable || kind == Kind::Required || group.kind == GroupKind::All;
                                let mut on = w.chosen[s][g][p] && kind != Kind::NotUsable;
                                let text = format!("{} ({})", plug.name, kind_label(kind));
                                if ui.add_enabled(!locked, egui::Checkbox::new(&mut on, text)).changed() {
                                    w.chosen[s][g][p] = on;
                                }
                                option_body(ui, plug);
                            }
                        }
                    }
                }
            });
        }
        if let Some(e) = &w.error {
            ui.label(RichText::new(e).color(ui.visuals().error_fg_color));
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label("Folder name");
            ui.add(egui::TextEdit::singleline(&mut w.folder_name).desired_width(220.0));
        });
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                decision = Some(Decision::Cancel);
            }
            if w.at > 0 && ui.button("Back").clicked() {
                w.at -= 1;
                w.error = None;
            }
            let last = ids.is_empty() || w.at + 1 >= ids.len();
            if !last && ui.button("Next").clicked() {
                w.normalize();
                let s = ids[w.at];
                let kinds = w.kinds();
                let mut bad = None;
                for (g, group) in w.steps[s].groups.iter().enumerate() {
                    if let Some(e) = group_error(group, &w.chosen[s][g], &kinds[s][g]) {
                        bad = Some(e);
                        break;
                    }
                }
                if let Some(e) = bad {
                    w.error = Some(e);
                } else {
                    w.error = None;
                    w.at += 1;
                }
            }
            if last && ui.button("Install").clicked() {
                if let Some(e) = w.validate_all() {
                    w.error = Some(e);
                } else {
                    w.error = None;
                    decision = Some(Decision::Install);
                }
            }
        });
    });
    if !open {
        return Some(Decision::Cancel);
    }
    decision
}

fn option_body(ui: &mut egui::Ui, plug: &Plug) {
    if !plug.description.is_empty() {
        ui.label(RichText::new(&plug.description).small().color(ui.visuals().weak_text_color()));
    }
    if let Some(uri) = plug.image.as_deref().and_then(file_uri) {
        ui.add(egui::Image::from_uri(uri).max_height(100.0));
    }
}

/// The URI the window loads the picture at `path` from: `path` itself, unescaped. The window's
/// file loader reads that exact path first and only falls back to a %-decoded form when it is
/// missing, so an escaped name could load another file than the one checked. A path that is not
/// UTF-8, or holds `%`, `#` or `?`, gets no URI and its picture is not shown. On Windows the loader
/// drops one `/` after `file://` and reads anything else as a network path (`\\…`), so the path
/// there follows `file:///`.
fn file_uri(path: &Path) -> Option<String> {
    let s = path.to_str()?;
    if s.contains(['%', '#', '?']) {
        return None;
    }
    Some(if cfg!(windows) { format!("file:///{s}") } else { format!("file://{s}") })
}

impl TypeSpec {
    fn resolve(&self, flags: &HashMap<String, String>, facts: &Facts) -> Kind {
        match self {
            TypeSpec::Fixed(kind) => *kind,
            TypeSpec::Depends { default, patterns } => {
                for (dep, kind) in patterns {
                    if dep.holds(flags, facts) {
                        return *kind;
                    }
                }
                *default
            }
        }
    }
}

impl Dep {
    fn holds(&self, flags: &HashMap<String, String>, facts: &Facts) -> bool {
        match self {
            Dep::Flag { flag, value } => flags.get(&flag.to_ascii_lowercase()).is_some_and(|v| v.eq_ignore_ascii_case(value)),
            Dep::File { file, state } => file_state(file, state, facts),
            Dep::Game => true,
            Dep::All(parts) => parts.iter().all(|d| d.holds(flags, facts)),
            Dep::Any(parts) => parts.iter().any(|d| d.holds(flags, facts)),
        }
    }
}

fn file_state(file: &str, state: &str, facts: &Facts) -> bool {
    let name = file.rsplit(['/', '\\']).next().unwrap_or(file).to_ascii_lowercase();
    let path = file.replace('\\', "/").trim_matches('/').to_ascii_lowercase();
    let plugin = name.ends_with(".esm") || name.ends_with(".esp");
    let present = if plugin { facts.present.contains(&name) || facts.active.contains(&name) } else { facts.paths.iter().any(|p| p == &path || p.ends_with(&format!("/{path}"))) };
    let active = if plugin { facts.active.contains(&name) } else { present };
    match state.to_ascii_lowercase().as_str() {
        "active" => active,
        "inactive" => present && !active,
        "missing" => !present,
        _ => false,
    }
}

fn parse_wizard(text: &str, root: &Path, temp: Option<PathBuf>, fallback: &str, facts: Facts) -> Result<Wizard, String> {
    let doc = parse_xml(text).map_err(|e| format!("The installer could not be read: {e}"))?;
    let config = find_el(&doc, "config").ok_or("The installer has no config.")?;
    paths_fit(config)?;
    let title = el_text(config, "moduleName").filter(|s| !s.trim().is_empty()).unwrap_or_else(|| fallback.to_string());
    let folder_name = clean_name(&title).unwrap_or_else(|_| clean_name(fallback).unwrap_or_else(|_| "mod".into()));
    let image = el_child(config, "moduleImage").and_then(|el| attr(el, "path")).and_then(|p| resolve_rel(root, p));
    let required = el_child(config, "requiredInstallFiles").map(parse_items).unwrap_or_default();
    let mut steps = Vec::new();
    if let Some(list) = el_child(config, "installSteps") {
        let order = attr(list, "order").unwrap_or("");
        for step_el in children(list, "installStep") {
            let name = attr(step_el, "name").unwrap_or("Options").to_string();
            let visible = el_child(step_el, "visible").map(parse_dep);
            let mut groups = Vec::new();
            if let Some(glist) = el_child(step_el, "optionalFileGroups") {
                let gorder = attr(glist, "order").unwrap_or("");
                for group_el in children(glist, "group") {
                    let gname = attr(group_el, "name").unwrap_or("Options").to_string();
                    let kind = group_kind(attr(group_el, "type").unwrap_or("SelectAny"));
                    let mut plugins = Vec::new();
                    if let Some(plist) = el_child(group_el, "plugins") {
                        let porder = attr(plist, "order").unwrap_or("");
                        for plug_el in children(plist, "plugin") {
                            plugins.push(parse_plug(plug_el, root));
                        }
                        sort_by(&mut plugins, porder, |p| p.name.as_str());
                    }
                    groups.push(Group { name: gname, kind, plugins });
                }
                sort_by(&mut groups, gorder, |g| g.name.as_str());
            }
            steps.push(Step { name, visible, groups });
        }
        sort_by(&mut steps, order, |s| s.name.as_str());
    }
    let mut patterns = Vec::new();
    if let Some(cond) = el_child(config, "conditionalFileInstalls") {
        if let Some(list) = el_child(cond, "patterns") {
            for pattern in children(list, "pattern") {
                let when = el_child(pattern, "dependencies").map(parse_dep).unwrap_or(Dep::Any(Vec::new()));
                let files = el_child(pattern, "files").map(parse_items).unwrap_or_default();
                patterns.push(When { when, files });
            }
        }
    }
    let note = if fomod_script(root).is_some() { "A script.cs was in the archive and was not run.".to_string() } else { String::new() };
    let mut chosen = Vec::new();
    for step in &steps {
        let mut groups = Vec::new();
        for group in &step.groups {
            let row: Vec<bool> = group.plugins.iter().map(|plug| matches!(plug.typed.resolve(&HashMap::new(), &facts), Kind::Required | Kind::Recommended) || group.kind == GroupKind::All).collect();
            groups.push(row);
        }
        chosen.push(groups);
    }
    let mut wizard = Wizard { title, folder_name, note, error: None, root: root.to_path_buf(), temp, steps, required, patterns, chosen, at: 0, facts, image };
    wizard.keep_images();
    wizard.normalize();
    Ok(wizard)
}

/// The largest installer picture file read, in bytes.
const IMAGE_MAX_BYTES: u64 = 8 * 1024 * 1024;

/// The widest or tallest installer picture shown, in pixels: under the texture size every
/// graphics card takes, which the window's drawing asserts.
const IMAGE_MAX_SIDE: u32 = 2048;

/// The memory all of one installer's pictures may take once decoded (four bytes a pixel).
const IMAGE_MAX_DECODED: u64 = 256 * 1024 * 1024;

/// Whether the picture at `path` is shown: a file of at most `IMAGE_MAX_BYTES`, at most
/// `IMAGE_MAX_SIDE` each way by its header, and room left in `left` for its decoded size.
/// Only the header is read here; a picture that fails is not shown.
fn image_fits(path: &Path, left: &mut u64) -> bool {
    let Ok(meta) = std::fs::metadata(path) else { return false };
    if meta.len() > IMAGE_MAX_BYTES {
        return false;
    }
    let Ok((w, h)) = image::image_dimensions(path) else { return false };
    if w > IMAGE_MAX_SIDE || h > IMAGE_MAX_SIDE {
        return false;
    }
    let decoded = u64::from(w) * u64::from(h) * 4;
    if decoded > *left {
        return false;
    }
    *left -= decoded;
    true
}

impl Wizard {
    /// Drop the pictures that are too large, or past what all of them may take together. A
    /// picture used twice is counted once: the window loads it once.
    fn keep_images(&mut self) {
        let mut left = IMAGE_MAX_DECODED;
        let mut kept: HashSet<PathBuf> = HashSet::new();
        let mut keep = |image: &mut Option<PathBuf>| {
            if let Some(path) = image.clone() {
                if !kept.contains(&path) {
                    if file_uri(&path).is_some() && image_fits(&path, &mut left) {
                        kept.insert(path);
                    } else {
                        *image = None;
                    }
                }
            }
        };
        keep(&mut self.image);
        for step in &mut self.steps {
            for group in &mut step.groups {
                for plug in &mut group.plugins {
                    keep(&mut plug.image);
                }
            }
        }
    }

    /// Let the window free the installer's decoded pictures; call when the installer closes.
    pub fn forget_images(&self, ctx: &egui::Context) {
        let plugs = self.steps.iter().flat_map(|s| &s.groups).flat_map(|g| &g.plugins);
        for path in self.image.iter().chain(plugs.filter_map(|p| p.image.as_ref())) {
            if let Some(uri) = file_uri(path) {
                ctx.forget_image(&uri);
            }
        }
    }
}

fn parse_plug(el: &Elem, root: &Path) -> Plug {
    let name = attr(el, "name").unwrap_or("Option").to_string();
    let description = el_text(el, "description").unwrap_or_default().trim().to_string();
    let image = el_child(el, "image").and_then(|image| attr(image, "path")).and_then(|p| resolve_rel(root, p));
    let files = el_child(el, "files").map(parse_items).unwrap_or_default();
    let mut flags = Vec::new();
    if let Some(list) = el_child(el, "conditionFlags") {
        for flag in children(list, "flag") {
            if let Some(name) = attr(flag, "name") {
                flags.push((name.to_string(), flag.text.trim().to_string()));
            }
        }
    }
    let typed = el_child(el, "typeDescriptor").map(parse_type).unwrap_or(TypeSpec::Fixed(Kind::Optional));
    Plug { name, description, image, files, flags, typed }
}

fn parse_type(el: &Elem) -> TypeSpec {
    if let Some(fixed) = el_child(el, "type") {
        return TypeSpec::Fixed(kind_named(attr(fixed, "name").unwrap_or("Optional")));
    }
    if let Some(dep) = el_child(el, "dependencyType") {
        let default = el_child(dep, "defaultType").and_then(|d| attr(d, "name")).map(kind_named).unwrap_or(Kind::Optional);
        let mut patterns = Vec::new();
        if let Some(list) = el_child(dep, "patterns") {
            for pattern in children(list, "pattern") {
                let when = el_child(pattern, "dependencies").map(parse_dep).unwrap_or(Dep::Any(Vec::new()));
                let kind = el_child(pattern, "type").and_then(|t| attr(t, "name")).map(kind_named).unwrap_or(default);
                patterns.push((when, kind));
            }
        }
        return TypeSpec::Depends { default, patterns };
    }
    TypeSpec::Fixed(Kind::Optional)
}

/// The longest `source` or `destination` a file or folder item may name, in bytes. A longer one
/// cannot be installed on Windows anyway, and each listed path (`plan_loose_files`) repeats it.
const ITEM_PATH_MAX: usize = 1024;

/// Refuses an installer whose file or folder items name a path longer than `ITEM_PATH_MAX`.
fn paths_fit(config: &Elem) -> Result<(), String> {
    let mut stack = vec![config];
    while let Some(el) = stack.pop() {
        if el.name.eq_ignore_ascii_case("file") || el.name.eq_ignore_ascii_case("folder") {
            for key in ["source", "destination"] {
                if attr(el, key).is_some_and(|v| v.len() > ITEM_PATH_MAX) {
                    return Err(format!("The installer names a path longer than {ITEM_PATH_MAX} bytes, so it was not opened."));
                }
            }
        }
        stack.extend(el.children.iter());
    }
    Ok(())
}

fn parse_items(el: &Elem) -> Vec<Item> {
    let mut items = Vec::new();
    for child in &el.children {
        if child.name.eq_ignore_ascii_case("file") || child.name.eq_ignore_ascii_case("folder") {
            let source = attr(child, "source").unwrap_or("").to_string();
            if source.is_empty() {
                continue;
            }
            let destination = attr(child, "destination").unwrap_or("").to_string();
            let priority = attr(child, "priority").and_then(|s| s.parse().ok()).unwrap_or(0);
            items.push(Item { source, destination, priority, folder: child.name.eq_ignore_ascii_case("folder"), order: 0 });
        }
    }
    items
}

fn parse_dep(el: &Elem) -> Dep {
    if el.name.eq_ignore_ascii_case("flagDependency") {
        return Dep::Flag { flag: attr(el, "flag").unwrap_or("").to_string(), value: attr(el, "value").unwrap_or("").to_string() };
    }
    if el.name.eq_ignore_ascii_case("fileDependency") {
        return Dep::File { file: attr(el, "file").unwrap_or("").to_string(), state: attr(el, "state").unwrap_or("Active").to_string() };
    }
    if el.name.eq_ignore_ascii_case("gameDependency") {
        return Dep::Game;
    }
    let parts: Vec<Dep> = el.children.iter().filter(|c| is_dep_name(&c.name)).map(parse_dep).collect();
    let or = attr(el, "operator").is_some_and(|s| s.eq_ignore_ascii_case("or"));
    if or { Dep::Any(parts) } else { Dep::All(parts) }
}

fn is_dep_name(name: &str) -> bool {
    matches!(name.to_ascii_lowercase().as_str(), "flagdependency" | "filedependency" | "gamedependency" | "dependencies" | "compositedependency")
}

fn group_kind(name: &str) -> GroupKind {
    match name.to_ascii_lowercase().as_str() {
        "selectall" => GroupKind::All,
        "selectexactlyone" => GroupKind::ExactlyOne,
        "selectatmostone" => GroupKind::AtMostOne,
        "selectatleastone" => GroupKind::AtLeastOne,
        _ => GroupKind::Any,
    }
}

fn kind_named(name: &str) -> Kind {
    match name.to_ascii_lowercase().as_str() {
        "required" => Kind::Required,
        "recommended" => Kind::Recommended,
        "notusable" => Kind::NotUsable,
        "couldbeusable" => Kind::CouldBeUsable,
        _ => Kind::Optional,
    }
}

fn sort_by<T>(items: &mut [T], order: &str, name: impl Fn(&T) -> &str) {
    match order.to_ascii_lowercase().as_str() {
        "ascending" => items.sort_by_key(|a| name(a).to_ascii_lowercase()),
        "descending" => items.sort_by_key(|a| std::cmp::Reverse(name(a).to_ascii_lowercase())),
        _ => {}
    }
}

/// How many times the size of the unpacked files an installer may copy in all. A file may be
/// listed for more than one place, but not so often that the copies fill the disk.
const COPY_MAX_RATIO: u64 = 4;

/// The least an installer may copy, so one with many small files listed often still installs.
const COPY_MIN_BYTES: u64 = 16 * 1024 * 1024;

/// What one file or folder counts as, at least, against the size limits: about a disk block. An
/// empty file still takes a block and an inode, so a flood of empty entries is not free.
const ENTRY_COST: u64 = 4096;

/// The size a file of `len` bytes counts as against the size limits.
fn entry_cost(len: u64) -> u64 {
    len.max(ENTRY_COST)
}

/// Take `cost` from `left`; past it the installer is stopped.
fn charge(left: &mut u64, cost: u64) -> Result<(), String> {
    if cost > *left {
        return Err(format!("The installer copies more than {COPY_MAX_RATIO} times the size of its files, so it was stopped."));
    }
    *left -= cost;
    Ok(())
}

/// Take `ENTRY_COST` from `left` for each folder from `dir` up that does not exist yet: the
/// folders an item's destination is about to make.
fn charge_new_folders(dir: &Path, left: &mut u64) -> Result<(), String> {
    let missing = dir.ancestors().take_while(|a| !a.exists()).count();
    charge(left, u64::try_from(missing).unwrap_or(u64::MAX).saturating_mul(ENTRY_COST))
}

/// The size of the files and folders under `root` as the limits count them, without following
/// links.
fn tree_size(root: &Path) -> u64 {
    let mut total: u64 = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for entry in rd.flatten() {
            let Ok(meta) = entry.path().symlink_metadata() else { continue };
            if meta.is_dir() {
                total = total.saturating_add(ENTRY_COST);
                stack.push(entry.path());
            } else if meta.is_file() {
                total = total.saturating_add(entry_cost(meta.len()));
            }
        }
    }
    total
}

fn copy_items(root: &Path, dest: &Path, items: &[Item]) -> Result<(), String> {
    let mut items = items.to_vec();
    items.sort_by_key(|item| (item.priority, item.order));
    let mut left = tree_size(root).saturating_mul(COPY_MAX_RATIO).max(COPY_MIN_BYTES);
    for item in items {
        let rel = rel_path(&item.source)?;
        let src = root.join(&rel);
        if !src.exists() {
            return Err(format!("The installer asked for {}, and that is not in the archive.", item.source));
        }
        bounded(root, &src)?;
        if item.folder {
            let into = if item.destination.trim().is_empty() {
                dest.to_path_buf()
            } else {
                let rel = rel_path(&item.destination)?;
                // `copy_children` charges `into` itself.
                charge_new_folders(dest.join(&rel).parent().unwrap_or(dest), &mut left)?;
                let into = place_in(dest, &rel)?;
                std::fs::create_dir_all(&into).map_err(|e| format!("{}: {e}", into.display()))?;
                into
            };
            copy_children(&src, &into, &mut left)?;
        } else {
            let named = src.file_name().unwrap_or_default();
            let rel = if item.destination.trim().is_empty() || item.destination.ends_with('/') || item.destination.ends_with('\\') {
                rel_path(&item.destination)?.join(named)
            } else {
                rel_path(&item.destination)?
            };
            charge_new_folders(dest.join(&rel).parent().unwrap_or(dest), &mut left)?;
            let to = place_in(dest, &rel)?;
            copy_counted(&src, &to, &mut left)?;
        }
    }
    Ok(())
}

/// Copy one file, taking what it counts as (`entry_cost`) from `left`; a copy that would go past
/// it is refused.
fn copy_counted(src: &Path, to: &Path, left: &mut u64) -> Result<(), String> {
    let size = std::fs::metadata(src).map_err(|e| format!("{}: {e}", src.display()))?.len();
    charge(left, entry_cost(size))?;
    let copied = std::fs::copy(src, to).map_err(|e| format!("{}: {e}", src.display()))?;
    *left = left.saturating_sub(copied.saturating_sub(size));
    Ok(())
}

fn copy_tree(src: &Path, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    let mut left = u64::MAX;
    copy_children(src, dest, &mut left)
}

/// Copy what is inside `src` into `dest`, folder by folder from a list rather than by calling
/// itself, so a deeply nested archive cannot use up the stack.
fn copy_children(src: &Path, dest: &Path, left: &mut u64) -> Result<(), String> {
    let mut stack = vec![(src.to_path_buf(), dest.to_path_buf())];
    while let Some((from, into)) = stack.pop() {
        charge(left, ENTRY_COST)?;
        std::fs::create_dir_all(&into).map_err(|e| format!("{}: {e}", into.display()))?;
        for entry in std::fs::read_dir(&from).map_err(|e| format!("{}: {e}", from.display()))?.flatten() {
            let path = entry.path();
            if path.is_symlink() {
                return Err(format!("{} is a link", path.display()));
            }
            let name = entry.file_name();
            let lower = name.to_string_lossy().to_ascii_lowercase();
            if matches!(lower.as_str(), "fomod" | "__macosx" | "meta") {
                continue;
            }
            let to = into.join(&name);
            if path.is_dir() {
                stack.push((path, to));
            } else if path.is_file() {
                copy_counted(&path, &to, left)?;
            }
        }
    }
    Ok(())
}

fn has_game_files(root: &Path) -> bool {
    fn walk(dir: &Path, depth: u8) -> bool {
        if depth == 0 || dir.is_symlink() {
            return false;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return false };
        for entry in rd.flatten() {
            let path = entry.path();
            if path.is_symlink() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
            if name.eq_ignore_ascii_case("fomod") || name.eq_ignore_ascii_case("__macosx") {
                continue;
            }
            if path.is_dir() {
                if loose_name(name) || walk(&path, depth - 1) {
                    return true;
                }
            } else if path.is_file() && is_game_file(name) {
                return true;
            }
        }
        false
    }
    walk(root, 6)
}

fn loose_name(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    LOOSE.iter().any(|n| *n == l)
}

fn is_game_file(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    l.ends_with(".esm") || l.ends_with(".esp") || l.ends_with(".bsa") || l.ends_with(".ba2")
}

fn hoist(dir: &Path) -> PathBuf {
    let mut cur = dir.to_path_buf();
    for _ in 0..6 {
        if has_top_files(&cur) {
            break;
        }
        let kids = subdirs(&cur);
        if kids.len() == 1 {
            cur = kids[0].clone();
            continue;
        }
        // A readme and a pictures folder beside one installer folder (`optional`,
        // or a single option). The plugins in that one folder are the mod.
        let with_plugins: Vec<PathBuf> = kids.iter().filter(|k| dir_has_plugin(k)).cloned().collect();
        if with_plugins.len() == 1 {
            cur = with_plugins[0].clone();
            continue;
        }
        break;
    }
    let data = ci_child(&cur, "Data");
    if let Some(data) = data {
        if data.is_dir() && has_game_files(&data) && !has_top_files(&cur) {
            return data;
        }
    }
    cur
}

/// A plugin the scanner skips (`optional`, and the same names) is copied to the
/// mod folder itself. Otherwise the folder shows in Mod Order and the plugin list stays empty.
fn surface_hidden_plugins(dest: &Path) -> Result<(), String> {
    if !fnv_install::plugins::plugins_in_tree(dest).is_empty() {
        return Ok(());
    }
    let mut found = Vec::new();
    walk_every_plugin(dest, 8, &mut found);
    for path in found {
        let Some(name) = path.file_name() else { continue };
        let to = dest.join(name);
        if to == path || to.exists() {
            continue;
        }
        std::fs::copy(&path, &to).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

fn dir_has_plugin(dir: &Path) -> bool {
    let mut found = Vec::new();
    walk_every_plugin(dir, 6, &mut found);
    !found.is_empty()
}

fn walk_every_plugin(dir: &Path, depth: u8, out: &mut Vec<PathBuf>) {
    if depth == 0 || dir.is_symlink() {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_symlink() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
        if name.starts_with('.') || name.eq_ignore_ascii_case("fomod") || name.eq_ignore_ascii_case("__macosx") {
            continue;
        }
        if path.is_dir() {
            walk_every_plugin(&path, depth - 1, out);
        } else if path.is_file() && is_game_file(name) && (name.to_ascii_lowercase().ends_with(".esp") || name.to_ascii_lowercase().ends_with(".esm")) {
            out.push(path);
        }
    }
}

fn has_top_files(dir: &Path) -> bool {
    let Ok(rd) = std::fs::read_dir(dir) else { return false };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_symlink() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
        if name.starts_with('.') || name.eq_ignore_ascii_case("fomod") || name.eq_ignore_ascii_case("__macosx") {
            continue;
        }
        if path.is_file() && is_game_file(name) {
            return true;
        }
        if path.is_dir() && loose_name(name) {
            return true;
        }
    }
    false
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    rd.flatten().map(|e| e.path()).filter(|p| p.is_dir() && !p.is_symlink()).filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| !n.starts_with('.') && !n.eq_ignore_ascii_case("fomod") && !n.eq_ignore_ascii_case("__macosx"))).collect()
}

fn fomod_script(dir: &Path) -> Option<PathBuf> {
    let mut roots = vec![dir.to_path_buf()];
    roots.extend(subdirs(dir));
    for root in roots {
        let Some(fomod) = ci_child(&root, "fomod") else { continue };
        if let Some(script) = ci_child(&fomod, "script.cs") {
            if script.is_file() {
                return Some(script);
            }
        }
    }
    None
}

fn find_config(dir: &Path) -> Option<PathBuf> {
    let mut roots = vec![dir.to_path_buf()];
    roots.extend(subdirs(dir));
    for root in roots {
        if let Some(fomod) = ci_child(&root, "fomod") {
            if let Some(xml) = ci_child(&fomod, "ModuleConfig.xml") {
                if xml.is_file() {
                    return Some(xml);
                }
            }
        }
    }
    None
}

fn ci_child(dir: &Path, name: &str) -> Option<PathBuf> {
    let rd = std::fs::read_dir(dir).ok()?;
    for entry in rd.flatten() {
        if entry.file_name().to_str().is_some_and(|n| n.eq_ignore_ascii_case(name)) {
            return Some(entry.path());
        }
    }
    None
}

fn resolve_rel(root: &Path, raw: &str) -> Option<PathBuf> {
    let path = root.join(rel_path(raw).ok()?);
    path.is_file().then_some(path)
}

/// A path from an archive or an installer, as plain names under the folder it is joined to.
/// Refused: `..`, a leading `/` or `\` (a root), and any `:` (a drive such as `C:`, or a
/// Windows stream name), so joining it can never leave that folder on any system.
fn rel_path(raw: &str) -> Result<PathBuf, String> {
    const LEAVES: &str = "The installer path would leave the archive.";
    if raw.starts_with(['/', '\\']) || raw.contains(':') {
        return Err(LEAVES.into());
    }
    let mut path = PathBuf::new();
    for part in raw.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(LEAVES.into());
        }
        path.push(part);
    }
    if !path.components().all(|c| matches!(c, std::path::Component::Normal(_))) {
        return Err(LEAVES.into());
    }
    Ok(path)
}

/// `dest` joined with `rel` (from [`rel_path`]), its folders made, and checked to be inside
/// `dest` once the folders exist (a link already in `dest` cannot lead it out).
fn place_in(dest: &Path, rel: &Path) -> Result<PathBuf, String> {
    const LEAVES: &str = "The installer path would leave the mod folder.";
    if !rel.components().all(|c| matches!(c, std::path::Component::Normal(_))) {
        return Err(LEAVES.into());
    }
    let to = dest.join(rel);
    let parent = to.parent().unwrap_or(dest);
    std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    let dest_c = dest.canonicalize().map_err(|e| format!("{}: {e}", dest.display()))?;
    let parent_c = parent.canonicalize().map_err(|e| format!("{}: {e}", parent.display()))?;
    if !parent_c.starts_with(&dest_c) {
        return Err(LEAVES.into());
    }
    Ok(to)
}

fn bounded(root: &Path, path: &Path) -> Result<(), String> {
    let root_c = root.canonicalize().map_err(|e| format!("{}: {e}", root.display()))?;
    let path_c = path.canonicalize().map_err(|e| format!("{}: {e}", path.display()))?;
    if path_c != root_c && !path_c.starts_with(&root_c) {
        return Err("The installer path would leave the archive.".into());
    }
    Ok(())
}

pub fn clean_name(name: &str) -> Result<String, String> {
    let last = name.split(['/', '\\']).rev().find(|s| !s.is_empty()).unwrap_or("");
    // Control characters go too: a line break in a folder name would split its line in the
    // mod list (`mod-order.txt`, a profile's `modlist.txt`).
    let cleaned: String = last.chars().filter(|c| !c.is_control() && !matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*')).collect();
    let cleaned = cleaned.trim().to_string();
    if cleaned.is_empty() || cleaned.starts_with('.') || cleaned == ".." {
        return Err("That name cannot be used.".into());
    }
    Ok(cleaned)
}

fn is_archive(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
    const SUFFIXES: &[&str] = &[
        ".zip", ".7z", ".rar", ".fomod", ".tar", ".gz", ".tgz", ".bz2", ".tbz2", ".xz", ".txz", ".cab", ".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst", ".taz",
    ];
    if SUFFIXES.iter().any(|s| name.ends_with(s)) {
        return true;
    }
    archive_kind(path).is_some()
}

fn archive_stem(src: &Path) -> String {
    let name = src.file_name().and_then(|n| n.to_str()).unwrap_or("mod");
    let lower = name.to_ascii_lowercase();
    for ext in [".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst"] {
        if let Some(stripped) = lower.strip_suffix(ext) {
            let stem = &name[..stripped.len()];
            if !stem.is_empty() {
                return stem.to_string();
            }
        }
    }
    src.file_stem().and_then(|n| n.to_str()).filter(|s| !s.is_empty()).unwrap_or("mod").to_string()
}

fn archive_kind(path: &Path) -> Option<&'static str> {
    let mut buf = [0u8; 8];
    let mut f = std::fs::File::open(path).ok()?;
    use std::io::Read;
    let n = f.read(&mut buf).ok()?;
    let b = &buf[..n];
    if b.starts_with(b"PK\x03\x04") || b.starts_with(b"PK\x05\x06") || b.starts_with(b"PK\x07\x08") {
        Some("zip")
    } else if b.starts_with(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]) {
        Some("7z")
    } else if b.starts_with(b"Rar!") {
        Some("rar")
    } else if b.starts_with(&[0x1F, 0x8B]) {
        Some("gz")
    } else if b.starts_with(b"BZh") {
        Some("bz2")
    } else if b.starts_with(&[0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00]) {
        Some("xz")
    } else {
        None
    }
}

/// Why a built-in archive reader stopped.
#[derive(Debug, PartialEq)]
enum ReadFail {
    /// The file could not be read as that format at all, before any entry: an outside tool
    /// may still read it.
    Unreadable(String),
    /// The archive was read and an entry was refused (it would land outside the folder, is a
    /// link, or is not a file or folder), an entry was broken, or a write failed. Final: no
    /// outside tool gets the archive after this.
    Refused(String),
}

/// How many times its own size an archive may unpack to, in the built-in readers. Mods
/// compress far less than this; an archive bomb compresses far more.
const UNPACK_MAX_RATIO: u64 = 100;

/// The most bytes the built-in readers unpack from the archive `src` installs, the archive
/// inside it included.
fn unpack_limit(src: &Path) -> u64 {
    std::fs::metadata(src).map(|m| m.len()).unwrap_or(0).saturating_mul(UNPACK_MAX_RATIO)
}

fn too_big() -> String {
    format!("it unpacks to more than {UNPACK_MAX_RATIO} times the archive's size, so it is not unpacked")
}

/// Unpack `src` into `dest`. The built-in zip and tar readers come first, and unpack at most
/// `limit` bytes. An outside tool (`extract_external`) is used only for a format they do not
/// read (`.7z`, `.rar`) or a file they cannot read as that format at all; an entry they
/// refused stays refused.
fn extract_archive(src: &Path, dest: &Path, limit: u64) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    let name = src.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
    let kind = archive_kind(src);
    let zip_like = kind == Some("zip") || name.ends_with(".zip") || name.ends_with(".fomod");
    if zip_like {
        return match extract_zip(src, dest, limit) {
            Ok(()) => reject_links(dest),
            Err(ReadFail::Refused(e)) => Err(e),
            Err(ReadFail::Unreadable(e)) => match extract_external(src, dest) {
                Ok(()) => reject_links(dest),
                Err(_) => Err(e),
            },
        };
    }
    let gzip = kind == Some("gz") || name.ends_with(".tar.gz") || name.ends_with(".tgz") || name.ends_with(".taz");
    if gzip || name.ends_with(".tar") {
        match extract_tar(src, dest, gzip, limit) {
            Ok(()) => return reject_links(dest),
            Err(ReadFail::Refused(e)) => return Err(e),
            Err(ReadFail::Unreadable(_)) => {}
        }
    }
    extract_external(src, dest)?;
    reject_links(dest)
}

fn extract_zip(archive: &Path, dest: &Path, limit: u64) -> Result<(), ReadFail> {
    let bad = |e: &dyn std::fmt::Display| format!("{}: {e}", archive.display());
    let refused = |e: &dyn std::fmt::Display| ReadFail::Refused(bad(e));
    let f = std::fs::File::open(archive).map_err(|e| ReadFail::Unreadable(bad(&e)))?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| ReadFail::Unreadable(bad(&e)))?;
    // First every entry's name, without decompressing anything: a name or link refused here is
    // final. Only when every name passes may an entry this build cannot decompress (it decodes
    // Stored and Deflated only) or an encrypted one hand the archive to an outside tool.
    let mut decodable = true;
    let mut declared: u64 = 0;
    for i in 0..z.len() {
        let entry = z.by_index_raw(i).map_err(|e| refused(&e))?;
        let name = entry.name().map(|n| n.into_owned()).unwrap_or_default();
        if entry.enclosed_name().is_none() {
            return Err(refused(&format!("entry {name} would land outside the folder")));
        }
        if entry.is_symlink() {
            return Err(refused(&format!("entry {name} is a link")));
        }
        // The same path rule as tar entries and outside tools' names: no drive, root, `..` or
        // `:` (on Windows a `:` would write a hidden stream of another file).
        if let Err(e) = rel_path(&name) {
            return Err(refused(&format!("entry {name}: {e}")));
        }
        if entry.encrypted() || !matches!(entry.compression(), zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated) {
            decodable = false;
        }
        declared = declared.saturating_add(entry_cost(entry.size()));
    }
    // The sizes the entries declare, before any is unpacked.
    if declared > limit {
        return Err(refused(&too_big()));
    }
    if !decodable {
        return Err(ReadFail::Unreadable(bad(&"an entry is encrypted or packed in a way this reader does not decode")));
    }
    std::fs::create_dir_all(dest).map_err(|e| ReadFail::Refused(format!("{}: {e}", dest.display())))?;
    for i in 0..z.len() {
        let mut entry = z.by_index(i).map_err(|e| refused(&e))?;
        let name = entry.name().map(|n| n.into_owned()).unwrap_or_default();
        let Some(rel) = entry.enclosed_name() else {
            return Err(refused(&format!("entry {name} would land outside the folder")));
        };
        if entry.is_symlink() {
            return Err(refused(&format!("entry {name} is a link")));
        }
        let to = place_in(dest, &rel).map_err(|e| refused(&format!("entry {name}: {e}")))?;
        if entry.is_dir() {
            std::fs::create_dir_all(&to).map_err(|e| ReadFail::Refused(format!("{}: {e}", to.display())))?;
            continue;
        }
        let mut out = std::fs::File::create(&to).map_err(|e| ReadFail::Refused(format!("{}: {e}", to.display())))?;
        // The zip reader stops an entry that unpacks to more than it declared, so with the
        // check above the total stays within `limit`.
        std::io::copy(&mut entry, &mut out).map_err(|e| refused(&e))?;
    }
    Ok(())
}

fn extract_tar(src: &Path, dest: &Path, gzip: bool, limit: u64) -> Result<(), ReadFail> {
    let bad = |e: &dyn std::fmt::Display| format!("{}: {e}", src.display());
    let f = std::fs::File::open(src).map_err(|e| ReadFail::Unreadable(bad(&e)))?;
    if gzip {
        unpack_tar(flate2::read::GzDecoder::new(f), dest, limit, &bad)
    } else {
        unpack_tar(f, dest, limit, &bad)
    }
}

/// A reader that gives at most `left` bytes. Past that it fails and sets `hit`.
struct Capped<R> {
    inner: R,
    left: u64,
    hit: std::rc::Rc<std::cell::Cell<bool>>,
}

impl<R: std::io::Read> std::io::Read for Capped<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.left == 0 {
            // At the cap: the stream may end here, or it is longer than allowed.
            let mut one = [0u8; 1];
            if self.inner.read(&mut one)? == 0 {
                return Ok(0);
            }
            self.hit.set(true);
            return Err(std::io::Error::other("the archive is longer than its limit"));
        }
        let most = buf.len().min(usize::try_from(self.left).unwrap_or(usize::MAX));
        let n = self.inner.read(&mut buf[..most])?;
        self.left = self.left.saturating_sub(n as u64);
        Ok(n)
    }
}

/// A failure before the first entry was read means the stream is not a tar this reader
/// reads (`Unreadable`); every failure after that is `Refused`. At most `limit` bytes of
/// entries are unpacked: a tar entry is exactly as long as its header says. The stream itself
/// is read to twice that, plus 1 MiB for headers, and no further: the tar reader keeps a long
/// name or extended header whole in memory, so this caps what one header can take.
fn unpack_tar<R: std::io::Read>(reader: R, dest: &Path, limit: u64, bad: &impl Fn(&dyn std::fmt::Display) -> String) -> Result<(), ReadFail> {
    let hit = std::rc::Rc::new(std::cell::Cell::new(false));
    let capped = Capped { inner: reader, left: limit.saturating_mul(2).saturating_add(1 << 20), hit: hit.clone() };
    let mut archive = tar::Archive::new(capped);
    let mut read_one = false;
    let mut total: u64 = 0;
    // Past the cap the archive is refused, whatever the tar reader made of the cut-off stream.
    let fail = |read_one: bool, e: String| {
        if hit.get() {
            ReadFail::Refused(bad(&too_big()))
        } else if read_one {
            ReadFail::Refused(e)
        } else {
            ReadFail::Unreadable(e)
        }
    };
    for entry in archive.entries().map_err(|e| fail(false, bad(&e)))? {
        let mut entry = entry.map_err(|e| fail(read_one, bad(&e)))?;
        read_one = true;
        let path = entry.path().map_err(|e| ReadFail::Refused(bad(&e)))?.into_owned();
        let rel = rel_path(&path.to_string_lossy()).map_err(|e| ReadFail::Refused(bad(&e)))?;
        let kind = entry.header().entry_type();
        if !(kind.is_file() || kind.is_dir()) {
            return Err(ReadFail::Refused(bad(&format!("entry {} is not a file or folder", path.display()))));
        }
        // `./`, as `tar -C folder -cf mod.tar .` writes it, is the archive's top folder: the
        // mod's folder itself, already there.
        if rel.as_os_str().is_empty() {
            if kind.is_dir() {
                continue;
            }
            return Err(ReadFail::Refused(bad(&format!("entry {} has no name", path.display()))));
        }
        total = total.saturating_add(entry_cost(entry.size()));
        if total > limit {
            return Err(ReadFail::Refused(bad(&too_big())));
        }
        let to = place_in(dest, &rel).map_err(|e| ReadFail::Refused(bad(&e)))?;
        // The archive's modes are not used: its folders and files get this account's defaults,
        // so none is left writable by other users, or read-only so that it cannot be removed.
        let wrote = |e: std::io::Error| fail(true, format!("{}: {e}", to.display()));
        if kind.is_dir() {
            std::fs::create_dir_all(&to).map_err(wrote)?;
        } else {
            let mut out = std::fs::File::create(&to).map_err(wrote)?;
            std::io::copy(&mut entry, &mut out).map_err(wrote)?;
        }
    }
    Ok(())
}

/// The refusal for an entry that is a link, device or anything else but a file or a folder.
fn not_file_or_folder(name: &str) -> String {
    format!("entry {name} is not a file or folder")
}

/// A Unix mode as listings print it (`-rw-r--r--`, `lrwxrwxrwx`): its first letter is the
/// entry's type, `-` a file and `d` a folder.
fn unix_mode(token: &str) -> Option<char> {
    let mut chars = token.chars();
    let kind = chars.next()?;
    let rest: Vec<char> = chars.collect();
    (rest.len() == 9 && rest.iter().all(|c| "rwxsStT-".contains(*c))).then_some(kind)
}

/// The entry names in a `7z l -slt -ba` listing, one block of `Key = value` lines an entry,
/// each starting with `Path = <name>`. Refused before anything is unpacked: an entry whose
/// `Symbolic Link`, `Hard Link` or `Copy Link` is set; whose Unix mode (in `Attributes`, or
/// `Mode` for a tar) is not a file's or folder's; or whose Windows attributes have `L`
/// (a reparse point: a link or junction) or `d` (a device). [observed: 7-Zip 26.06 listings of
/// .7z, .rar and .tar archives holding a link, made on this machine]
fn names_7z(listings: &[String]) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    let mut current: Option<String> = None;
    for line in listings.first().map(String::as_str).unwrap_or("").lines().map(|l| l.trim_end_matches('\r')) {
        if let Some(name) = line.strip_prefix("Path = ") {
            names.push(name.to_string());
            current = Some(name.to_string());
            continue;
        }
        let Some(name) = current.as_deref() else { continue };
        let Some((key, value)) = line.split_once(" = ").or_else(|| line.strip_suffix(" =").map(|k| (k, ""))) else { continue };
        let value = value.trim();
        let refused = match key {
            "Symbolic Link" | "Hard Link" | "Copy Link" => !value.is_empty(),
            "Mode" => unix_mode(value).is_some_and(|k| k != '-' && k != 'd'),
            "Attributes" => value.split_whitespace().any(|t| match unix_mode(t) {
                Some(k) => k != '-' && k != 'd',
                None => t.contains(['L', 'd']),
            }),
            _ => false,
        };
        if refused {
            return Err(not_file_or_folder(name));
        }
    }
    Ok(names)
}

/// The entry names in `bsdtar -tf`, checked against `bsdtar -tvf` line by line: each verbose
/// line starts with the entry's mode, `l` for a link and `h` for a hard link, so only `-` and
/// `d` pass. Two listings that do not line up are refused. [observed: bsdtar 3.8 on this
/// machine, tar, .7z and .rar archives holding a link and a hard link]
fn names_bsdtar(listings: &[String]) -> Result<Vec<String>, String> {
    let lines = |s: Option<&String>| -> Vec<String> {
        s.map(String::as_str).unwrap_or("").lines().map(|l| l.trim_end_matches('\r')).filter(|l| !l.is_empty()).map(str::to_string).collect()
    };
    let names = lines(listings.first());
    let verbose = lines(listings.get(1));
    if names.len() != verbose.len() {
        return Err("the archive's two listings do not match, so its entries could not be checked".into());
    }
    for (name, line) in names.iter().zip(&verbose) {
        if !matches!(line.chars().next(), Some('-' | 'd')) {
            return Err(not_file_or_folder(name));
        }
    }
    Ok(names)
}

/// The entry names in an `unrar lt` listing: each entry has a `Name: ` line and a `Type: `
/// line, and only `File` and `Directory` pass (a link is `Unix symbolic link`, `Windows
/// symbolic link`, `Junction` or `Hard link`). [observed: UNRAR 7.23 on this machine]
fn names_unrar(listings: &[String]) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    let mut typed = 0;
    for line in listings.first().map(String::as_str).unwrap_or("").lines().map(|l| l.trim_end_matches('\r').trim_start()) {
        if let Some(name) = line.strip_prefix("Name: ") {
            names.push(name.to_string());
        } else if let Some(kind) = line.strip_prefix("Type: ") {
            typed += 1;
            if names.len() != typed || !matches!(kind.trim(), "File" | "Directory") {
                return Err(not_file_or_folder(names.last().map_or("", String::as_str)));
            }
        }
    }
    if names.len() != typed {
        return Err("the archive's listing gives no type for an entry, so it could not be checked".into());
    }
    Ok(names)
}

/// The entry names in an `lsar -j` listing (JSON, `lsarContents` one object an entry, the
/// name in `XADFileName`). Refused: an entry with `XADIsLink` or `XADIsHardLink` true, an
/// `XADLinkDestination`, a Unix type that is not a file's or folder's in
/// `XADPosixPermissions`, or the reparse-point bit (0x400) in `XADWindowsAttributes`.
/// [hypothesis: the key names of The Unarchiver's XADArchiveParser; lsar was not installed
/// on the machine this was written on]
fn names_lsar(listings: &[String]) -> Result<Vec<String>, String> {
    let json: serde_json::Value = serde_json::from_str(listings.first().map(String::as_str).unwrap_or(""))
        .map_err(|e| format!("lsar's listing could not be read, so its entries could not be checked: {e}"))?;
    let entries = json.get("lsarContents").and_then(|c| c.as_array()).ok_or("lsar's listing has no entries, so none could be checked")?;
    let mut names = Vec::new();
    for entry in entries {
        let name = entry.get("XADFileName").and_then(|n| n.as_str()).ok_or("lsar listed an entry without a name")?;
        let flag = |key: &str| entry.get(key).is_some_and(|v| v.as_bool() == Some(true) || v.as_i64().is_some_and(|n| n != 0));
        let posix = entry.get("XADPosixPermissions").and_then(|v| v.as_u64()).map(|m| m & 0o170_000);
        let windows = entry.get("XADWindowsAttributes").and_then(|v| v.as_u64()).unwrap_or(0);
        if flag("XADIsLink") || flag("XADIsHardLink") || entry.get("XADLinkDestination").is_some()
            || posix.is_some_and(|t| t != 0 && t != 0o100_000 && t != 0o040_000)
            || windows & 0x400 != 0
        {
            return Err(not_file_or_folder(name));
        }
        names.push(name.to_string());
    }
    Ok(names)
}

/// Unpack with an outside tool, for a format the built-in readers do not read. Each tool first
/// lists the archive's entries, and every entry must be a file or a folder and every name go
/// through `rel_path` before anything is unpacked: a link written first could carry a later
/// entry out of the mod folder before `reject_links` looks. A refused entry, or a listing with
/// no names, ends the install and no other tool gets the archive. A tool that is not installed,
/// or cannot open the archive, gives way to the next.
fn extract_external(src: &Path, dest: &Path) -> Result<(), String> {
    let src_s = src.display().to_string();
    let dest_s = dest.display().to_string();
    /// An outside tool: the program that lists the archive, the arguments of each listing it
    /// makes and how to read them, then the program and arguments that unpack.
    struct Tool {
        lister: &'static str,
        listings: Vec<Vec<String>>,
        names: fn(&[String]) -> Result<Vec<String>, String>,
        bin: &'static str,
        args: Vec<String>,
    }
    let list_7z = vec![vec!["l".to_string(), "-slt".into(), "-ba".into(), src_s.clone()]];
    let unpack_7z = vec!["x".to_string(), "-y".into(), format!("-o{dest_s}"), src_s.clone()];
    let attempts = [
        Tool { lister: "7z", listings: list_7z.clone(), names: names_7z, bin: "7z", args: unpack_7z.clone() },
        Tool { lister: "7zz", listings: list_7z, names: names_7z, bin: "7zz", args: unpack_7z },
        Tool { lister: "bsdtar", listings: vec![vec!["-tf".into(), src_s.clone()], vec!["-tvf".into(), src_s.clone()]], names: names_bsdtar, bin: "bsdtar", args: vec!["-xf".into(), src_s.clone(), "-C".into(), dest_s.clone()] },
        Tool { lister: "lsar", listings: vec![vec!["-j".into(), src_s.clone()]], names: names_lsar, bin: "unar", args: vec!["-f".into(), "-o".into(), dest_s.clone(), src_s.clone()] },
        Tool { lister: "unrar", listings: vec![vec!["lt".into(), src_s.clone()]], names: names_unrar, bin: "unrar", args: vec!["x".into(), "-o+".into(), "-y".into(), src_s.clone(), format!("{dest_s}/")] },
    ];
    let mut last = String::new();
    let mut missing = true;
    'tools: for Tool { lister, listings, names, bin, args } in &attempts {
        let mut outputs = Vec::new();
        for list_args in listings {
            match Command::new(lister).args(list_args).output() {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue 'tools,
                Err(e) => return Err(format!("{lister}: {e}")),
                Ok(out) if !out.status.success() => {
                    missing = false;
                    let err = String::from_utf8_lossy(&out.stderr);
                    last = format!("{lister}: {}", err.lines().next().unwrap_or("could not open the archive"));
                    continue 'tools;
                }
                Ok(out) => outputs.push(String::from_utf8_lossy(&out.stdout).into_owned()),
            }
        }
        let listed = names(&outputs).map_err(|e| format!("{src_s}: {e}"))?;
        if listed.is_empty() {
            return Err(format!("{src_s}: {lister} listed no entries, so none could be checked"));
        }
        for name in &listed {
            rel_path(name).map_err(|e| format!("{src_s}: entry {name}: {e}"))?;
        }
        match Command::new(bin).args(args).output() {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("{bin}: {e}")),
            Ok(out) if out.status.success() => return Ok(()),
            Ok(out) => {
                missing = false;
                let err = String::from_utf8_lossy(&out.stderr);
                last = format!("{bin}: {}", err.lines().next().unwrap_or("could not open the archive"));
            }
        }
    }
    if missing {
        Err("Could not open that archive. Install 7z or unar, then try again.".into())
    } else {
        Err(last)
    }
}

fn reject_links(root: &Path) -> Result<(), String> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for entry in rd.flatten() {
            let path = entry.path();
            if path.is_symlink() {
                return Err(format!("{} is a link", path.display()));
            }
            if path.is_dir() {
                stack.push(path);
            }
        }
    }
    Ok(())
}

/// Open the one archive inside `dir`, if it is the only thing there. `limit` is the limit of
/// the archive that held it, so the two together unpack at most twice that.
fn unwrap_nested(dir: &Path, limit: u64) -> Result<(), String> {
    let mut archives = Vec::new();
    let mut others = 0;
    let Ok(rd) = std::fs::read_dir(dir) else { return Ok(()) };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_symlink() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if name == "__macosx" || name.starts_with('.') {
            continue;
        }
        if path.is_file() && is_archive(&path) {
            archives.push(path);
        } else {
            others += 1;
        }
    }
    if archives.len() == 1 && others == 0 {
        let inner = &archives[0];
        let sub = dir.join("_nvf_inner");
        extract_archive(inner, &sub, limit)?;
        std::fs::remove_file(inner).map_err(|e| format!("{}: {e}", inner.display()))?;
        for entry in std::fs::read_dir(&sub).map_err(|e| format!("{}: {e}", sub.display()))?.flatten() {
            let to = dir.join(entry.file_name());
            std::fs::rename(entry.path(), &to).map_err(|e| format!("{}: {e}", to.display()))?;
        }
        remove_temp(&sub);
    }
    Ok(())
}

/// A new folder for one install, in the system's temporary folder. It must not exist yet, and
/// on Unix only this account can open it, so no other user can swap a folder in it for a link
/// while an archive is unpacked there. (Windows gives each account its own temporary folder.)
pub(crate) fn make_temp() -> Result<PathBuf, String> {
    // The count keeps two installs started in the same clock tick apart.
    static MADE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let k = MADE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("nvforever-import-{}-{n}-{k}", std::process::id()));
    #[cfg(unix)]
    let builder = {
        use std::os::unix::fs::DirBuilderExt;
        let mut b = std::fs::DirBuilder::new();
        b.mode(0o700);
        b
    };
    #[cfg(not(unix))]
    let builder = std::fs::DirBuilder::new();
    builder.create(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

/// Remove an install's temporary folder. An outside tool may have kept an archive's read-only
/// modes, which stop the removal: then every folder and file in it (never a link, which could
/// point outside it) is made writable by this account and the removal is tried again. If that
/// fails too, a line says what was left.
fn remove_temp(dir: &Path) {
    if std::fs::remove_dir_all(dir).is_ok() || !dir.exists() {
        return;
    }
    let mut stack = vec![dir.to_path_buf()];
    while let Some(folder) = stack.pop() {
        if let Ok(meta) = folder.symlink_metadata() {
            if meta.is_dir() {
                make_writable(&folder, meta.permissions(), true);
            }
        }
        let Ok(rd) = std::fs::read_dir(&folder) else { continue };
        for entry in rd.flatten() {
            let path = entry.path();
            let Ok(meta) = path.symlink_metadata() else { continue };
            if meta.is_dir() {
                stack.push(path);
            } else if meta.is_file() {
                make_writable(&path, meta.permissions(), false);
            }
        }
    }
    if let Err(e) = std::fs::remove_dir_all(dir) {
        eprintln!("warning: the temporary folder {} could not be removed: {e}", dir.display());
    }
}

/// Make `path`, a folder or a file that is not a link, writable by this account.
fn make_writable(path: &Path, mut perms: std::fs::Permissions, folder: bool) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(perms.mode() | if folder { 0o700 } else { 0o600 });
    }
    #[cfg(not(unix))]
    {
        let _ = folder;
        // Windows: a read-only file cannot be deleted; this only clears that mark.
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
    }
    let _ = std::fs::set_permissions(path, perms);
}

/// The largest `ModuleConfig.xml` read, in bytes.
const XML_MAX_BYTES: u64 = 16 * 1024 * 1024;

/// How deep the installer's elements may nest. The tree is read, searched and freed one call
/// level an element deeper, so a deeper one is refused before it can use up the stack.
const XML_MAX_DEPTH: usize = 64;

fn read_xml(path: &Path) -> Result<String, String> {
    use std::io::Read as _;
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(XML_MAX_BYTES + 1).read_to_end(&mut bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() as u64 > XML_MAX_BYTES {
        return Err(format!("{}: the installer is larger than {} MiB, so it is not read.", path.display(), XML_MAX_BYTES / (1024 * 1024)));
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let (chunks, _) = bytes[2..].as_chunks::<2>();
        let units: Vec<u16> = chunks.iter().map(|c| u16::from_le_bytes(*c)).collect();
        return Ok(String::from_utf16_lossy(&units));
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let (chunks, _) = bytes[2..].as_chunks::<2>();
        let units: Vec<u16> = chunks.iter().map(|c| u16::from_be_bytes(*c)).collect();
        return Ok(String::from_utf16_lossy(&units));
    }
    Ok(String::from_utf8_lossy(&bytes).trim_start_matches('\u{feff}').to_string())
}

struct Elem {
    name: String,
    attrs: Vec<(String, String)>,
    text: String,
    children: Vec<Elem>,
}

fn parse_xml(text: &str) -> Result<Elem, String> {
    let mut stack = vec![Elem { name: String::new(), attrs: Vec::new(), text: String::new(), children: Vec::new() }];
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'<' {
            let start = i;
            while i < b.len() && b[i] != b'<' {
                i += 1;
            }
            let chunk = decode(&text[start..i]);
            if let Some(top) = stack.last_mut() {
                top.text.push_str(&chunk);
            }
            continue;
        }
        if text[i..].starts_with("<!--") {
            i = text[i + 4..].find("-->").map(|n| i + 4 + n + 3).ok_or("The installer XML is broken.")?;
            continue;
        }
        if text[i..].starts_with("<?") {
            i = text[i + 2..].find("?>").map(|n| i + 2 + n + 2).ok_or("The installer XML is broken.")?;
            continue;
        }
        if text[i..].starts_with("<!") {
            i = text[i..].find('>').map(|n| i + n + 1).ok_or("The installer XML is broken.")?;
            continue;
        }
        let end = text[i..].find('>').map(|n| i + n).ok_or("The installer XML is broken.")?;
        let body = &text[i + 1..end];
        let close = body.starts_with('/');
        let empty = body.ends_with('/');
        let body = body.trim().trim_start_matches('/').trim_end_matches('/').trim();
        if close {
            if stack.len() <= 1 {
                return Err("The installer XML is broken.".into());
            }
            let done = stack.pop().unwrap();
            stack.last_mut().unwrap().children.push(done);
        } else {
            // `stack` holds the document root and every open element, so the new element is
            // `stack.len()` deep.
            if stack.len() > XML_MAX_DEPTH {
                return Err(format!("its elements nest more than {XML_MAX_DEPTH} deep."));
            }
            let (name, attrs) = split_tag(body)?;
            let el = Elem { name, attrs, text: String::new(), children: Vec::new() };
            if empty {
                stack.last_mut().unwrap().children.push(el);
            } else {
                stack.push(el);
            }
        }
        i = end + 1;
    }
    if stack.len() != 1 {
        return Err("The installer XML is broken.".into());
    }
    Ok(stack.pop().unwrap())
}

fn split_tag(body: &str) -> Result<(String, Vec<(String, String)>), String> {
    let mut parts = body.split_whitespace();
    let raw = parts.next().unwrap_or("").to_string();
    let name = raw.split(':').next_back().unwrap_or(&raw).to_string();
    let rest = body[raw.len()..].trim();
    let mut attrs = Vec::new();
    let b = rest.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= b.len() {
            break;
        }
        let start = i;
        while i < b.len() && b[i] != b'=' && !b[i].is_ascii_whitespace() {
            i += 1;
        }
        let key_raw = &rest[start..i];
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= b.len() || b[i] != b'=' {
            return Err("The installer XML is broken.".into());
        }
        i += 1;
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= b.len() {
            return Err("The installer XML is broken.".into());
        }
        let quote = b[i];
        if quote != b'"' && quote != b'\'' {
            return Err("The installer XML is broken.".into());
        }
        i += 1;
        let vstart = i;
        while i < b.len() && b[i] != quote {
            i += 1;
        }
        if i >= b.len() {
            return Err("The installer XML is broken.".into());
        }
        let key = key_raw.split(':').next_back().unwrap_or(key_raw).to_string();
        attrs.push((key, decode(&rest[vstart..i])));
        i += 1;
    }
    Ok((name, attrs))
}

fn decode(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at + 1..];
        let Some(end) = rest.find(';') else {
            out.push('&');
            break;
        };
        let token = &rest[..end];
        rest = &rest[end + 1..];
        match token {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            t if t.starts_with("#x") || t.starts_with("#X") => {
                if let Some(c) = u32::from_str_radix(&t[2..], 16).ok().and_then(char::from_u32) {
                    out.push(c);
                }
            }
            t if let Some(num) = t.strip_prefix('#') => {
                if let Some(c) = num.parse::<u32>().ok().and_then(char::from_u32) {
                    out.push(c);
                }
            }
            _ => {}
        }
    }
    out.push_str(rest);
    out
}

fn attr<'a>(el: &'a Elem, name: &str) -> Option<&'a str> {
    el.attrs.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}

fn children<'a>(el: &'a Elem, name: &str) -> Vec<&'a Elem> {
    el.children.iter().filter(|c| c.name.eq_ignore_ascii_case(name)).collect()
}

fn el_child<'a>(el: &'a Elem, name: &str) -> Option<&'a Elem> {
    el.children.iter().find(|c| c.name.eq_ignore_ascii_case(name))
}

fn el_text(el: &Elem, name: &str) -> Option<String> {
    el_child(el, name).map(|c| c.text.clone())
}

fn find_el<'a>(el: &'a Elem, name: &str) -> Option<&'a Elem> {
    if el.name.eq_ignore_ascii_case(name) {
        return Some(el);
    }
    for child in &el.children {
        if let Some(found) = find_el(child, name) {
            return Some(found);
        }
    }
    None
}

/// One archive entry in the manual installer. `<data>` is the folder that becomes the mod.
#[derive(Clone)]
pub struct ManFile {
    pub name: String,
    pub dir: bool,
    pub checked: bool,
    pub children: Vec<ManFile>,
}

/// The files under `dir`, for the manual installer. Nothing is copied.
pub fn scan_manual(dir: &Path) -> ManFile {
    let mut left = 8000usize;
    ManFile { name: "<data>".into(), dir: true, checked: true, children: read_manual(dir, 8, &mut left) }
}

fn read_manual(dir: &Path, depth: u8, left: &mut usize) -> Vec<ManFile> {
    if depth == 0 || *left == 0 || dir.is_symlink() {
        return Vec::new();
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name().to_string_lossy().to_ascii_lowercase());
    let mut out = Vec::new();
    for entry in entries {
        if *left == 0 {
            break;
        }
        let path = entry.path();
        if path.is_symlink() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name.eq_ignore_ascii_case("fomod") || name.eq_ignore_ascii_case("__macosx") {
            continue;
        }
        *left -= 1;
        if path.is_dir() {
            let children = read_manual(&path, depth - 1, left);
            out.push(ManFile { name, dir: true, checked: true, children });
        } else if path.is_file() {
            out.push(ManFile { name, dir: false, checked: true, children: Vec::new() });
        }
    }
    out
}

pub fn set_branch(node: &mut ManFile, on: bool) {
    node.checked = on;
    for child in &mut node.children {
        set_branch(child, on);
    }
}

/// True when every ticked entry under `<data>` is a plugin, an archive, or a known folder.
pub fn data_valid(node: &ManFile) -> bool {
    let checked: Vec<&ManFile> = node.children.iter().filter(|c| c.checked).collect();
    !checked.is_empty() && checked.iter().all(|c| manual_ok(c))
}

fn manual_ok(node: &ManFile) -> bool {
    if !node.checked {
        return true;
    }
    if node.dir {
        if loose_name(&node.name) {
            return true;
        }
        let checked: Vec<&ManFile> = node.children.iter().filter(|c| c.checked).collect();
        return !checked.is_empty() && checked.iter().all(|c| manual_ok(c));
    }
    manual_file_ok(&node.name)
}

fn manual_file_ok(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    is_game_file(name) || lower.ends_with(".ini") || lower.ends_with(".xml") || lower.ends_with(".dll") || lower.ends_with(".json")
}

/// Copies ticked files from `data` into `dest`. Unticked files are left out. The game folder is not used.
pub fn copy_checked(data: &Path, node: &ManFile, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    copy_checked_into(data, node, dest)
}

fn copy_checked_into(dir: &Path, node: &ManFile, dest: &Path) -> Result<(), String> {
    for child in &node.children {
        if !child.checked {
            continue;
        }
        if child.name.is_empty() || child.name.contains(['/', '\\', ':']) || child.name == "." || child.name == ".." {
            return Err("That file name cannot be used.".into());
        }
        let from = dir.join(&child.name);
        let to = dest.join(&child.name);
        if from.is_symlink() {
            return Err(format!("{} is a link", from.display()));
        }
        if child.dir {
            std::fs::create_dir_all(&to).map_err(|e| format!("{}: {e}", to.display()))?;
            copy_checked_into(&from, child, &to)?;
        } else if from.is_file() {
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            std::fs::copy(&from, &to).map_err(|e| format!("{}: {e}", from.display()))?;
        }
    }
    Ok(())
}

/// Extracts an archive, or uses a folder, so Manual can list it. Nothing is installed.
pub fn open_manual(src: &Path) -> Result<(Option<PathBuf>, PathBuf, ManFile), String> {
    if src.as_os_str().is_empty() || !src.exists() {
        return Err("That path was not found.".into());
    }
    if src.is_dir() {
        return Ok((None, src.to_path_buf(), scan_manual(src)));
    }
    let temp = make_temp()?;
    if is_archive(src) {
        let limit = unpack_limit(src);
        if let Err(e) = extract_archive(src, &temp, limit) {
            remove_temp(&temp);
            return Err(e);
        }
        if let Err(e) = unwrap_nested(&temp, limit) {
            remove_temp(&temp);
            return Err(e);
        }
    } else {
        let file_name = src.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !is_game_file(file_name) && !manual_file_ok(file_name) {
            remove_temp(&temp);
            return Err("Drop a folder or an archive (.zip, .7z, .rar, .fomod, .tar, .gz).".into());
        }
        if let Err(e) = std::fs::copy(src, temp.join(file_name)) {
            remove_temp(&temp);
            return Err(format!("{}: {e}", src.display()));
        }
    }
    let tree = scan_manual(&temp);
    Ok((Some(temp.clone()), temp, tree))
}

#[cfg(test)]
mod tests {
    #[test]
    fn installer_paths_never_leave_the_folder() {
        use super::rel_path;
        for bad in [r"C:\Windows\x", "C:/Windows/x", r"\Windows\x", "/etc/x", "a/../../x", "..", "file.esp:stream", r"\\?\C:\x"] {
            assert!(rel_path(bad).is_err(), "{bad} should be refused");
        }
        assert_eq!(rel_path(r"textures\armor\a.dds").unwrap(), std::path::Path::new("textures").join("armor").join("a.dds"));
        assert_eq!(rel_path("./Data//b.esp").unwrap(), std::path::Path::new("Data").join("b.esp"));
        assert_eq!(rel_path("").unwrap(), std::path::PathBuf::new());
    }

    #[test]
    fn a_tar_entry_with_a_drive_is_refused() {
        let dir = std::env::temp_dir().join(format!("nvf-fomod-tar-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let dest = dir.join("out");
        std::fs::create_dir_all(&dest).unwrap();
        let mut bytes = Vec::new();
        {
            let mut b = tar::Builder::new(&mut bytes);
            let mut h = tar::Header::new_gnu();
            // The name goes into the header as raw bytes: on Windows the tar crate's
            // `append_data` refuses an absolute path, so the test would build no archive there.
            let name = r"C:\Windows\x".as_bytes();
            h.as_gnu_mut().unwrap().name[..name.len()].copy_from_slice(name);
            h.set_size(1);
            h.set_mode(0o644);
            h.set_entry_type(tar::EntryType::Regular);
            h.set_cksum();
            b.append(&h, &b"x"[..]).unwrap();
            b.finish().unwrap();
        }
        let bad = |e: &dyn std::fmt::Display| e.to_string();
        // Refused by the path check itself (`rel_path`), not by a broken header.
        assert_eq!(super::unpack_tar(&bytes[..], &dest, u64::MAX, &bad), Err(super::ReadFail::Refused(super::rel_path(r"C:\Windows\x").unwrap_err())));
        assert_eq!(std::fs::read_dir(&dest).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `stage` on an archive the built-in reader refuses: the refusal is the result, and no
    /// outside tool (7z, bsdtar, unar, unrar) gets the archive after it.
    fn stage_error(archive: &Path) -> String {
        let facts = FactsInput { active: HashSet::new(), present: HashSet::new(), paths: HashSet::new() };
        match stage(archive, facts) {
            Err(e) => e,
            Ok(_) => panic!("{} was installed although an entry was refused", archive.display()),
        }
    }

    #[test]
    fn a_refused_zip_entry_stops_the_install() {
        let t = std::env::temp_dir().join(format!("nvf-fomod-refused-zip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(t.join("inside")).unwrap();
        let zip_path = t.join("inside").join("Evil.zip");
        {
            let mut z = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
            let o = zip::write::SimpleFileOptions::default();
            z.start_file("Mod/ok.esp", o).unwrap();
            z.write_all(b"TES4").unwrap();
            z.start_file("../evil.esp", o).unwrap();
            z.write_all(b"TES4").unwrap();
            z.finish().unwrap();
        }
        let e = stage_error(&zip_path);
        assert!(e.contains("would land outside the folder"), "{e}");
        assert!(!t.join("evil.esp").exists());
        assert_eq!(std::fs::read_dir(t.join("inside")).unwrap().count(), 1, "only the archive");
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn a_refused_tar_entry_stops_the_install() {
        let t = std::env::temp_dir().join(format!("nvf-fomod-refused-tar-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(&t).unwrap();
        let tar_path = t.join("Evil.tar");
        {
            let mut b = tar::Builder::new(std::fs::File::create(&tar_path).unwrap());
            let mut h = tar::Header::new_gnu();
            let name = r"C:\Windows\x".as_bytes();
            h.as_gnu_mut().unwrap().name[..name.len()].copy_from_slice(name);
            h.set_size(1);
            h.set_mode(0o644);
            h.set_entry_type(tar::EntryType::Regular);
            h.set_cksum();
            b.append(&h, &b"x"[..]).unwrap();
            b.finish().unwrap();
        }
        let e = stage_error(&tar_path);
        assert!(e.ends_with(&super::rel_path(r"C:\Windows\x").unwrap_err()), "{e}");
        assert_eq!(std::fs::read_dir(&t).unwrap().count(), 1, "only the archive");
        let _ = std::fs::remove_dir_all(&t);
    }

    /// A zip of `entries` (name, bytes), stored, then marked as packed with method `method` in
    /// every local and central header, so the entries claim a method this build cannot decode.
    fn zip_marked(path: &Path, entries: &[(&str, &[u8])], method: u16) {
        {
            let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
            let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            for (name, body) in entries {
                z.start_file(*name, o).unwrap();
                z.write_all(body).unwrap();
            }
            z.finish().unwrap();
        }
        let mut b = std::fs::read(path).unwrap();
        let set = |b: &mut Vec<u8>, sig: &[u8], at: usize| {
            let mut i = 0;
            while let Some(k) = b[i..].windows(4).position(|w| w == sig) {
                let p = i + k + at;
                b[p..p + 2].copy_from_slice(&method.to_le_bytes());
                i += k + 4;
            }
        };
        set(&mut b, b"PK\x03\x04", 8);
        set(&mut b, b"PK\x01\x02", 10);
        std::fs::write(path, b).unwrap();
    }

    #[test]
    fn an_entry_this_build_cannot_decode_leaves_the_archive_to_an_outside_tool_only_after_its_names_pass() {
        let t = std::env::temp_dir().join(format!("nvf-fomod-method-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(&t).unwrap();
        // 12 is bzip2, which this build of the zip reader does not decode.
        let packed = t.join("Packed.zip");
        zip_marked(&packed, &[("Mod/Plugin.esp", b"TES4")], 12);
        assert!(matches!(super::extract_zip(&packed, &t.join("out"), u64::MAX), Err(super::ReadFail::Unreadable(_))));
        assert!(!t.join("out").exists(), "nothing unpacked before the outside tool");
        // A refused name stays final, whatever the method: through `stage`, no outside tool
        // gets the archive after it.
        let evil = t.join("Evil.zip");
        zip_marked(&evil, &[("Mod/Plugin.esp", b"TES4"), ("../evil.esp", b"TES4")], 12);
        let e = stage_error(&evil);
        assert!(e.contains("would land outside the folder"), "{e}");
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn outside_tools_listings_give_every_entry_name() {
        let one = |s: &str| vec![s.to_string()];
        let seven = "Path = Mod/Plugin.esp\r\nFolder = -\r\nSize = 4\r\nAttributes = A -rw-r--r--\r\nSymbolic Link = \r\n\r\nPath = ../evil.esp\r\nFolder = -\r\n\r\nPath = Mod\r\nAttributes = D drwxr-xr-x\r\n";
        assert_eq!(super::names_7z(&one(seven)).unwrap(), ["Mod/Plugin.esp", "../evil.esp", "Mod"]);
        let tf = "Mod/\nMod/Plugin.esp\n../evil.esp\n\n".to_string();
        let tvf = "drwxr-xr-x  0 a a 0 Nov 27 14:46 Mod/\n-rw-r--r--  0 a a 4 Nov 27 14:46 Mod/Plugin.esp\n-rw-r--r--  0 a a 4 Nov 27 14:46 ../evil.esp\n".to_string();
        assert_eq!(super::names_bsdtar(&[tf, tvf]).unwrap(), ["Mod/", "Mod/Plugin.esp", "../evil.esp"]);
        let unrar = "\nUNRAR 7.23 freeware\n\nArchive: x.rar\nDetails: RAR 5\n\n        Name: Mod/Plugin.esp\n        Type: File\n        Size: 4\n\n        Name: Mod\n        Type: Directory\n";
        assert_eq!(super::names_unrar(&one(unrar)).unwrap(), ["Mod/Plugin.esp", "Mod"]);
        let lsar = r#"{"lsarFormatName":"7-Zip","lsarContents":[{"XADFileName":"Mod/Plugin.esp","XADPosixPermissions":33188},{"XADFileName":"..\\evil.esp"},{"XADFileName":"Mod","XADIsDirectory":true,"XADPosixPermissions":16877}]}"#;
        assert_eq!(super::names_lsar(&one(lsar)).unwrap(), ["Mod/Plugin.esp", "..\\evil.esp", "Mod"]);
    }

    /// Listings, as each tool printed them on this machine, of archives holding a link
    /// (`Mod/link`, to `/etc`) or a hard link (`Mod/a.esp`): every one is refused.
    #[test]
    fn outside_tools_listings_refuse_link_entries() {
        let one = |s: &str| vec![s.to_string()];
        let refused = |r: Result<Vec<String>, String>, name: &str| {
            let e = r.expect_err("a link entry was let through");
            assert_eq!(e, super::not_file_or_folder(name));
        };
        // 7z: a .7z (Unix mode in Attributes), a .rar (Symbolic Link), a tar (Mode, Hard Link).
        refused(super::names_7z(&one("Path = Mod/a.esp\nAttributes = A -rw-r--r--\n\nPath = Mod/link\nSize = 4\nAttributes = A lrwxrwxrwx\nCRC = 08E1E5AF\n")), "Mod/link");
        refused(super::names_7z(&one("Path = Mod/link\nFolder = -\nAttributes =  lrwxrwxrwx\nSymbolic Link = /etc\nHard Link = \n")), "Mod/link");
        refused(super::names_7z(&one("Path = Mod/link\nFolder = -\nSymbolic Link = /etc\n")), "Mod/link");
        refused(super::names_7z(&one("Path = Mod/link\nFolder = -\nMode = lrwxrwxrwx\n")), "Mod/link");
        refused(super::names_7z(&one("Path = Mod/a.esp\nMode = -rw-r--r--\nSymbolic Link = \nHard Link = Mod/hard.esp\n")), "Mod/a.esp");
        refused(super::names_7z(&one("Path = Mod/a.esp\nMode = hrw-r--r--\n")), "Mod/a.esp");
        // Windows attributes: L is a reparse point (link or junction), d a device.
        refused(super::names_7z(&one("Path = Mod\\link\r\nAttributes = DL\r\n")), "Mod\\link");
        refused(super::names_7z(&one("Path = Mod\\dev\r\nAttributes = Ad\r\n")), "Mod\\dev");
        // bsdtar: a link `l`, a hard link `h`; listings that do not line up are refused too.
        let tf = "Mod/\nMod/link\n".to_string();
        refused(super::names_bsdtar(&[tf.clone(), "drwxr-xr-x  0 a a 0 Nov 27 14:46 Mod/\nlrwxrwxrwx  0 a a 0 Nov 27 14:46 Mod/link -> /etc\n".into()]), "Mod/link");
        let tf_hard = "Mod/hard.esp\nMod/a.esp\n".to_string();
        refused(super::names_bsdtar(&[tf_hard, "-rw-r--r--  0 a a 3 Nov 27 14:46 Mod/hard.esp\nhrw-r--r--  0 a a 0 Nov 27 14:46 Mod/a.esp link to Mod/hard.esp\n".into()]), "Mod/a.esp");
        assert!(super::names_bsdtar(&[tf, "drwxr-xr-x  0 a a 0 Nov 27 14:46 Mod/\n".into()]).is_err());
        // unrar lt: Type other than File or Directory, or an entry with no Type.
        refused(super::names_unrar(&one("        Name: Mod/a.esp\n        Type: File\n\n        Name: Mod/link\n        Type: Unix symbolic link\n      Target: /etc\n")), "Mod/link");
        refused(super::names_unrar(&one("        Name: Mod\\link\r\n        Type: Junction\r\n")), "Mod\\link");
        assert!(super::names_unrar(&one("        Name: Mod/a.esp\n\n        Name: Mod/b.esp\n        Type: File\n")).is_err());
        assert!(super::names_unrar(&one("        Name: Mod/a.esp\n")).is_err());
        // lsar -j: link flags, a link target, a Unix type or the Windows reparse-point bit.
        refused(super::names_lsar(&one(r#"{"lsarContents":[{"XADFileName":"Mod/link","XADIsLink":true}]}"#)), "Mod/link");
        refused(super::names_lsar(&one(r#"{"lsarContents":[{"XADFileName":"Mod/a.esp","XADIsHardLink":1}]}"#)), "Mod/a.esp");
        refused(super::names_lsar(&one(r#"{"lsarContents":[{"XADFileName":"Mod/link","XADLinkDestination":"/etc"}]}"#)), "Mod/link");
        refused(super::names_lsar(&one(r#"{"lsarContents":[{"XADFileName":"Mod/link","XADPosixPermissions":41471}]}"#)), "Mod/link");
        refused(super::names_lsar(&one(r#"{"lsarContents":[{"XADFileName":"Mod/link","XADWindowsAttributes":1040}]}"#)), "Mod/link");
        assert!(super::names_lsar(&one("Evil.7z: 7-Zip\nMod/a.esp\n")).is_err());
    }

    /// A tar written under a .7z name, so no built-in reader takes it and it goes to the
    /// outside tools, holding a link entry and then a file written through that link.
    #[cfg(unix)]
    #[test]
    fn an_outside_tools_archive_with_a_link_entry_is_not_unpacked() {
        let installed = ["7z", "7zz", "bsdtar", "lsar", "unrar"].iter().any(|b| Command::new(b).arg("--help").output().is_ok());
        for (case, hard) in [("sym", false), ("hard", true)] {
            let t = std::env::temp_dir().join(format!("nvf-fomod-outside-link-{case}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&t);
            std::fs::create_dir_all(t.join("inside")).unwrap();
            std::fs::create_dir_all(t.join("outside")).unwrap();
            std::fs::write(t.join("outside").join("victim.esp"), b"KEEP").unwrap();
            let archive = t.join("inside").join("Evil.7z");
            {
                let mut b = tar::Builder::new(std::fs::File::create(&archive).unwrap());
                let mut h = tar::Header::new_gnu();
                h.set_size(0);
                h.set_mode(0o777);
                if hard {
                    // A hard link to a file outside, then a file of that name that overwrites it.
                    h.set_entry_type(tar::EntryType::Link);
                    b.append_link(&mut h, "Mod/victim.esp", t.join("outside").join("victim.esp")).unwrap();
                    let mut f = tar::Header::new_gnu();
                    f.set_size(4);
                    f.set_mode(0o644);
                    f.set_entry_type(tar::EntryType::Regular);
                    b.append_data(&mut f, "Mod/victim.esp", &b"EVIL"[..]).unwrap();
                } else {
                    // A link to the folder outside, then a file written through it.
                    h.set_entry_type(tar::EntryType::Symlink);
                    b.append_link(&mut h, "Mod/link", t.join("outside")).unwrap();
                    let mut f = tar::Header::new_gnu();
                    f.set_size(4);
                    f.set_mode(0o644);
                    f.set_entry_type(tar::EntryType::Regular);
                    b.append_data(&mut f, "Mod/link/evil.esp", &b"EVIL"[..]).unwrap();
                }
                b.finish().unwrap();
            }
            let e = stage_error(&archive);
            if installed {
                assert!(e.contains("is not a file or folder"), "{case}: {e}");
            }
            assert!(!t.join("outside").join("evil.esp").exists(), "{case}: written through the link");
            assert_eq!(std::fs::read(t.join("outside").join("victim.esp")).unwrap(), b"KEEP", "{case}: written through the hard link");
            assert_eq!(std::fs::read_dir(t.join("inside")).unwrap().count(), 1, "{case}: only the archive");
            let _ = std::fs::remove_dir_all(&t);
        }
    }

    /// A real .7z holding a link, made by 7z itself (`-snl` stores links as links): 7z's own
    /// listing refuses it, before 7z or any other tool unpacks anything.
    #[cfg(unix)]
    #[test]
    fn a_7z_archive_with_a_link_entry_is_refused_by_its_listing() {
        if Command::new("7z").arg("--help").output().is_err() {
            return;
        }
        let t = std::env::temp_dir().join(format!("nvf-fomod-7z-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(t.join("src").join("Mod")).unwrap();
        std::fs::create_dir_all(t.join("inside")).unwrap();
        std::fs::write(t.join("src").join("Mod").join("Plugin.esp"), b"TES4").unwrap();
        std::os::unix::fs::symlink(t.join("outside"), t.join("src").join("Mod").join("link")).unwrap();
        let archive = t.join("inside").join("Evil.7z");
        let made = Command::new("7z").args(["a", "-snl"]).arg(&archive).arg("Mod").current_dir(t.join("src")).output().unwrap();
        assert!(made.status.success(), "{}", String::from_utf8_lossy(&made.stderr));
        let e = stage_error(&archive);
        assert!(e.ends_with(&super::not_file_or_folder("Mod/link")), "{e}");
        assert_eq!(std::fs::read_dir(t.join("inside")).unwrap().count(), 1, "only the archive");
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn an_outside_tools_archive_with_a_refused_name_is_not_unpacked() {
        let t = std::env::temp_dir().join(format!("nvf-fomod-outside-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(t.join("inside")).unwrap();
        // A tar under a .7z name: no built-in reader takes it, so it goes to an outside tool.
        let archive = t.join("inside").join("Evil.7z");
        {
            let mut b = tar::Builder::new(std::fs::File::create(&archive).unwrap());
            for (name, body) in [("Mod/Plugin.esp", &b"TES4"[..]), ("../evil.esp", &b"TES4"[..])] {
                let mut h = tar::Header::new_gnu();
                h.as_gnu_mut().unwrap().name[..name.len()].copy_from_slice(name.as_bytes());
                h.set_size(body.len() as u64);
                h.set_mode(0o644);
                h.set_entry_type(tar::EntryType::Regular);
                h.set_cksum();
                b.append(&h, body).unwrap();
            }
            b.finish().unwrap();
        }
        let e = stage_error(&archive);
        let installed = ["7z", "7zz", "bsdtar", "lsar", "unrar"].iter().any(|b| Command::new(b).arg("--help").output().is_ok());
        if installed {
            assert!(e.contains(&super::rel_path("../evil.esp").unwrap_err()), "{e}");
        }
        assert!(!t.join("evil.esp").exists() && !t.join("inside").join("evil.esp").exists());
        assert_eq!(std::fs::read_dir(t.join("inside")).unwrap().count(), 1, "only the archive");
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn a_file_that_is_no_zip_is_unreadable_not_refused() {
        let t = std::env::temp_dir().join(format!("nvf-fomod-not-zip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(&t).unwrap();
        let fake = t.join("Fake.zip");
        std::fs::write(&fake, b"this is not a zip archive at all").unwrap();
        assert!(matches!(super::extract_zip(&fake, &t.join("out"), u64::MAX), Err(super::ReadFail::Unreadable(_))));
        let fake_tar = t.join("Fake.tar");
        std::fs::write(&fake_tar, vec![b'x'; 1024]).unwrap();
        assert!(matches!(super::extract_tar(&fake_tar, &t.join("out"), false, u64::MAX), Err(super::ReadFail::Unreadable(_))));
        let _ = std::fs::remove_dir_all(&t);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_inside_the_folder_cannot_lead_out() {
        let dir = std::env::temp_dir().join(format!("nvf-fomod-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let dest = dir.join("mod");
        let outside = dir.join("outside");
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, dest.join("link")).unwrap();
        assert!(super::place_in(&dest, std::path::Path::new("link/x.esp")).is_err());
        let ok = super::place_in(&dest, std::path::Path::new("textures/a.dds")).unwrap();
        assert_eq!(ok, dest.join("textures/a.dds"));
        assert!(dest.join("textures").is_dir());
        let _ = std::fs::remove_dir_all(&dir);
    }

    use super::*;
    use std::io::Write;

    #[test]
    fn an_option_folder_is_a_choice() {
        let xml = r#"
        <config>
          <moduleName>Example Mod</moduleName>
          <requiredInstallFiles><file source="readme.txt"/></requiredInstallFiles>
          <installSteps order="Explicit">
            <installStep name="Choose Option">
              <optionalFileGroups>
                <group name="Select an option:" type="SelectExactlyOne">
                  <plugins>
                    <plugin name="Option A">
                      <description>A</description>
                      <files><folder source="option_a"/></files>
                      <conditionFlags><flag name="option_a">selected</flag></conditionFlags>
                      <typeDescriptor><type name="Recommended"/></typeDescriptor>
                    </plugin>
                    <plugin name="Option B">
                      <files><folder source="option_b"/></files>
                      <typeDescriptor><type name="Optional"/></typeDescriptor>
                    </plugin>
                  </plugins>
                </group>
              </optionalFileGroups>
            </installStep>
          </installSteps>
        </config>"#;
        let facts = Facts { active: HashSet::new(), present: HashSet::new(), paths: HashSet::new() };
        let w = parse_wizard(xml, Path::new("."), None, "Example", facts).unwrap();
        assert!(w.needs_choice());
        assert_eq!(w.steps.len(), 1);
        assert!(w.chosen[0][0][0]);
        assert!(!w.chosen[0][0][1]);
        let flags = w.flags();
        assert_eq!(flags.get("option_a").map(String::as_str), Some("selected"));
    }

    #[test]
    fn a_plugin_hidden_beside_a_readme_is_the_mod() {
        let t = std::env::temp_dir().join(format!("nvf-fomod-hoist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let src = t.join("src");
        std::fs::create_dir_all(src.join("optional/meshes")).unwrap();
        std::fs::create_dir_all(src.join("images")).unwrap();
        std::fs::write(src.join("readme.txt"), b"hi").unwrap();
        std::fs::write(src.join("images/preview.png"), b"png").unwrap();
        std::fs::write(src.join("optional/Foo.esp"), b"TES4").unwrap();
        std::fs::write(src.join("optional/meshes/a.nif"), b"nif").unwrap();
        let from = hoist(&src);
        assert!(from.ends_with("optional"), "{}", from.display());
        let mods = t.join("mods");
        std::fs::create_dir_all(&mods).unwrap();
        let name = place_tree(&mods, "Example", &from).unwrap();
        assert_eq!(name, "Example");
        assert!(mods.join("Example/Foo.esp").is_file());
        assert!(mods.join("Example/meshes/a.nif").is_file());
        let listed = fnv_install::plugins::plugins_in_tree(&mods.join("Example"));
        assert_eq!(listed.len(), 1);
        assert!(listed[0].0.eq_ignore_ascii_case("Foo.esp"));
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn a_zip_installs_its_plugin_not_an_empty_folder() {
        let t = std::env::temp_dir().join(format!("nvf-fomod-zip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(&t).unwrap();
        let zip_path = t.join("Sample Mod.zip");
        {
            let mut z = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
            let o = zip::write::SimpleFileOptions::default();
            z.start_file("Sample Mod/readme.txt", o).unwrap();
            z.write_all(b"hi").unwrap();
            z.add_directory("Sample Mod/optional/", o).unwrap();
            z.start_file("Sample Mod/optional/Bar.esp", o).unwrap();
            z.write_all(b"TES4").unwrap();
            z.finish().unwrap();
        }
        let facts = FactsInput { active: HashSet::new(), present: HashSet::new(), paths: HashSet::new() };
        let staged = stage(&zip_path, facts).unwrap();
        let Staged::Tree { name, from, temp } = staged else { panic!("a zip with one plugin is not a choice") };
        let mods = t.join("mods");
        std::fs::create_dir_all(&mods).unwrap();
        let name = place_tree(&mods, &name, &from).unwrap();
        discard(&temp);
        assert!(mods.join(&name).join("Bar.esp").is_file(), "plugin was not copied out of the archive");
        assert!(!fnv_install::plugins::plugins_in_tree(&mods.join(&name)).is_empty());
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn a_fomod_choice_copies_only_the_chosen_plugin() {
        let t = std::env::temp_dir().join(format!("nvf-fomod-choice-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let root = t.join("root");
        std::fs::create_dir_all(root.join("option_a")).unwrap();
        std::fs::create_dir_all(root.join("option_b")).unwrap();
        std::fs::write(root.join("option_a/A.esp"), b"TES4").unwrap();
        std::fs::write(root.join("option_b/B.esp"), b"TES4").unwrap();
        std::fs::write(root.join("readme.txt"), b"r").unwrap();
        let xml = r#"
        <config>
          <moduleName>Example Mod</moduleName>
          <requiredInstallFiles><file source="readme.txt"/></requiredInstallFiles>
          <installSteps order="Explicit">
            <installStep name="Choose Option">
              <optionalFileGroups>
                <group name="Select an option:" type="SelectExactlyOne">
                  <plugins>
                    <plugin name="Option A">
                      <files><folder source="option_a"/></files>
                      <typeDescriptor><type name="Recommended"/></typeDescriptor>
                    </plugin>
                    <plugin name="Option B">
                      <files><folder source="option_b"/></files>
                      <typeDescriptor><type name="Optional"/></typeDescriptor>
                    </plugin>
                  </plugins>
                </group>
              </optionalFileGroups>
            </installStep>
          </installSteps>
        </config>"#;
        let facts = Facts { active: HashSet::new(), present: HashSet::new(), paths: HashSet::new() };
        let mut w = parse_wizard(xml, &root, None, "Example", facts).unwrap();
        assert!(w.needs_choice());
        let mods = t.join("mods");
        std::fs::create_dir_all(&mods).unwrap();
        let name = w.install(&mods).unwrap();
        assert!(mods.join(&name).join("A.esp").is_file());
        assert!(!mods.join(&name).join("B.esp").exists());
        assert!(mods.join(&name).join("readme.txt").is_file());
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn a_higher_priority_wins_however_many_files_come_between() {
        let t = std::env::temp_dir().join(format!("nvf-fomod-priority-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(&t).unwrap();
        for (name, body) in [("high.esp", "high"), ("low.esp", "low"), ("first.esp", "first"), ("second.esp", "second"), ("filler.txt", "x")] {
            std::fs::write(t.join(name), body).unwrap();
        }
        // The higher priority comes first; more than a thousand files come between it and the
        // lower one, which once moved the lower one past it.
        let mut files = String::from(r#"<file source="high.esp" destination="Mod.esp" priority="1"/>"#);
        for n in 0..1001 {
            files.push_str(&format!(r#"<file source="filler.txt" destination="filler/{n}.txt"/>"#));
        }
        files.push_str(r#"<file source="low.esp" destination="Mod.esp" priority="0"/>"#);
        // Equal priority: the later file is kept.
        files.push_str(r#"<file source="first.esp" destination="Tie.esp"/><file source="second.esp" destination="Tie.esp"/>"#);
        let xml = format!("<config><moduleName>Priority</moduleName><requiredInstallFiles>{files}</requiredInstallFiles></config>");
        let facts = Facts { active: HashSet::new(), present: HashSet::new(), paths: HashSet::new() };
        let mut w = parse_wizard(&xml, &t, None, "Priority", facts).unwrap();
        let mods = t.join("mods");
        std::fs::create_dir_all(&mods).unwrap();
        let name = w.install(&mods).unwrap();
        assert_eq!(std::fs::read_to_string(mods.join(&name).join("Mod.esp")).unwrap(), "high");
        assert_eq!(std::fs::read_to_string(mods.join(&name).join("Tie.esp")).unwrap(), "second");
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn an_exactly_one_group_with_two_picks_keeps_the_first() {
        let t = std::env::temp_dir().join(format!("nvf-fomod-one-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        for o in ["a", "b", "c"] {
            std::fs::create_dir_all(t.join(o)).unwrap();
            std::fs::write(t.join(o).join(format!("{o}.esp")), b"TES4").unwrap();
        }
        let option = |o: &str| format!(r#"<plugin name="{o}"><files><folder source="{o}"/></files><typeDescriptor><type name="Optional"/></typeDescriptor></plugin>"#);
        let xml = format!(
            r#"<config><moduleName>One</moduleName><installSteps order="Explicit"><installStep name="Step"><optionalFileGroups><group name="Pick" type="SelectExactlyOne"><plugins>{}{}{}</plugins></group></optionalFileGroups></installStep></installSteps></config>"#,
            option("a"),
            option("b"),
            option("c")
        );
        let facts = Facts { active: HashSet::new(), present: HashSet::new(), paths: HashSet::new() };
        let mut w = parse_wizard(&xml, &t, None, "One", facts).unwrap();
        w.chosen[0][0] = vec![false, true, true];
        w.normalize();
        assert_eq!(w.chosen[0][0], vec![false, true, false]);
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn two_roots_are_a_choice_and_a_reinstall_keeps_the_old_folder_until_the_new_one_is_in() {
        let t = std::env::temp_dir().join(format!("nvf-fomod-roots-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let src = t.join("src");
        std::fs::create_dir_all(src.join("One")).unwrap();
        std::fs::create_dir_all(src.join("Two")).unwrap();
        std::fs::write(src.join("One/A.esp"), b"TES4").unwrap();
        std::fs::write(src.join("Two/B.esp"), b"TES4").unwrap();
        let facts = FactsInput { active: HashSet::new(), present: HashSet::new(), paths: HashSet::new() };
        let staged = stage(&src, facts).unwrap();
        let Staged::ChooseRoot { roots, temp, .. } = staged else { panic!("two plugin folders should be a choice") };
        assert_eq!(roots.len(), 2);
        discard(&temp);
        let mods = t.join("mods");
        let recycle = t.join("recycle");
        std::fs::create_dir_all(mods.join("Example")).unwrap();
        std::fs::write(mods.join("Example/old.txt"), b"old").unwrap();
        let from = t.join("from");
        std::fs::create_dir_all(from.join("meshes")).unwrap();
        std::fs::write(from.join("meshes/a.nif"), b"nif").unwrap();
        replace_tree(&mods, "Example", &from, &recycle).unwrap();
        assert!(mods.join("Example/meshes/a.nif").is_file());
        assert!(!mods.join("Example/old.txt").exists());
        let kept = std::fs::read_dir(&recycle).unwrap().flatten().any(|e| e.path().join("old.txt").is_file());
        assert!(kept);
        let missing = t.join("missing");
        std::fs::create_dir_all(mods.join("Example")).ok();
        let err = replace_tree(&mods, "Example", &missing, &recycle);
        assert!(err.is_err());
        assert!(mods.join("Example").exists());
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn manual_data_accepts_nvse_and_refuses_docs() {
        let t = std::env::temp_dir().join(format!("nvf-fomod-manual-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let src = t.join("src");
        std::fs::create_dir_all(src.join("Data/NVSE")).unwrap();
        std::fs::write(src.join("Data/NVSE/nvse_loader.exe"), b"exe").unwrap();
        std::fs::write(src.join("README.md"), b"docs").unwrap();
        std::fs::write(src.join("LICENSE.txt"), b"lic").unwrap();
        let whole = scan_manual(&src);
        assert!(!data_valid(&whole));
        let just = scan_manual(&src.join("Data"));
        assert!(data_valid(&just));
        let mut skipped = just.clone();
        skipped.children[0].checked = false;
        set_branch(&mut skipped.children[0], false);
        assert!(!data_valid(&skipped));
        let dest = t.join("out");
        copy_checked(&src.join("Data"), &just, &dest).unwrap();
        assert!(dest.join("NVSE/nvse_loader.exe").is_file());
        assert!(!dest.join("README.md").exists());
        let mcm = t.join("mcm");
        std::fs::create_dir_all(&mcm).unwrap();
        std::fs::write(mcm.join("menu.xml"), b"<x/>").unwrap();
        assert!(data_valid(&scan_manual(&mcm)));
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn installer_elements_may_nest_64_deep_and_no_deeper() {
        let deep = |n: usize| format!("{}{}", "<a>".repeat(n), "</a>".repeat(n));
        assert!(parse_xml(&deep(64)).is_ok());
        assert_eq!(parse_xml(&deep(65)).err(), Some("its elements nest more than 64 deep.".to_string()));
        // An empty element is a level too.
        assert!(parse_xml(&format!("{}<b/>{}", "<a>".repeat(63), "</a>".repeat(63))).is_ok());
        assert!(parse_xml(&format!("{}<b/>{}", "<a>".repeat(64), "</a>".repeat(64))).is_err());
    }

    #[test]
    fn an_installer_nested_too_deep_is_refused_not_a_crash() {
        let t = std::env::temp_dir().join(format!("nvf-fomod-deep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(&t).unwrap();
        // Half a million levels: without the cap, reading, searching or freeing this tree runs
        // out of stack and ends the whole process.
        let n = 500_000;
        let xml = format!("<config><moduleName>Deep</moduleName>{}{}</config>", "<a>".repeat(n), "</a>".repeat(n));
        let zip_path = t.join("Deep.zip");
        {
            let mut z = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
            let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            z.start_file("fomod/ModuleConfig.xml", o).unwrap();
            z.write_all(xml.as_bytes()).unwrap();
            z.start_file("Deep.esp", o).unwrap();
            z.write_all(b"TES4").unwrap();
            z.finish().unwrap();
        }
        assert_eq!(stage_error(&zip_path), "The installer could not be read: its elements nest more than 64 deep.");
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn an_installer_over_16_mib_is_not_read() {
        let t = std::env::temp_dir().join(format!("nvf-fomod-big-xml-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let fomod = t.join("Big").join("fomod");
        std::fs::create_dir_all(&fomod).unwrap();
        let xml = fomod.join("ModuleConfig.xml");
        std::fs::File::create(&xml).unwrap().set_len(XML_MAX_BYTES + 1).unwrap();
        let e = stage_error(&t.join("Big"));
        assert!(e.ends_with("the installer is larger than 16 MiB, so it is not read."), "{e}");
        // One byte less is read (and then judged as an installer, which these zeros are not).
        std::fs::File::create(&xml).unwrap().set_len(XML_MAX_BYTES).unwrap();
        let e = stage_error(&t.join("Big"));
        assert!(!e.contains("larger than"), "{e}");
        let _ = std::fs::remove_dir_all(&t);
    }

    /// `n` bytes that do not compress.
    fn noise(n: usize) -> Vec<u8> {
        let mut x: u32 = 0x9E37_79B9;
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                x as u8
            })
            .collect()
    }

    #[test]
    fn a_zip_that_unpacks_to_over_100_times_its_size_is_refused() {
        let t = std::env::temp_dir().join(format!("nvf-fomod-zip-bomb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(&t).unwrap();
        let zip_with = |path: &Path, body: &[u8]| {
            let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
            let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            z.start_file("Mod/Plugin.esp", o).unwrap();
            z.write_all(body).unwrap();
            z.finish().unwrap();
        };
        // A mebibyte of zeros packs into about a kibibyte.
        let bomb = t.join("Bomb.zip");
        zip_with(&bomb, &vec![0; 1 << 20]);
        let e = stage_error(&bomb);
        assert!(e.ends_with(&too_big()), "{e}");
        // One that packs as mods do installs.
        let fine = t.join("Fine.zip");
        zip_with(&fine, &noise(64 * 1024));
        let facts = FactsInput { active: HashSet::new(), present: HashSet::new(), paths: HashSet::new() };
        let Staged::Tree { temp, .. } = stage(&fine, facts).unwrap() else { panic!("a zip with one plugin is not a choice") };
        discard(&temp);
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn a_zip_entry_that_unpacks_to_more_than_its_header_says_is_refused() {
        let t = std::env::temp_dir().join(format!("nvf-fomod-zip-liar-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(&t).unwrap();
        let liar = t.join("Liar.zip");
        {
            let mut z = zip::ZipWriter::new(std::fs::File::create(&liar).unwrap());
            let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            z.start_file("Mod/Plugin.esp", o).unwrap();
            z.write_all(&noise(4000)).unwrap();
            z.finish().unwrap();
        }
        // The headers then say the entry unpacks to 4 bytes: the unpacked size sits at offset
        // 22 of the local header and 24 of the central one.
        let mut b = std::fs::read(&liar).unwrap();
        for (sig, at) in [(b"PK\x03\x04", 22), (b"PK\x01\x02", 24)] {
            let p = b.windows(4).position(|w| w == sig).unwrap() + at;
            b[p..p + 4].copy_from_slice(&4u32.to_le_bytes());
        }
        std::fs::write(&liar, b).unwrap();
        // The zip reader's own refusal: the declared sizes are what the limit is checked against.
        let e = stage_error(&liar);
        assert!(e.contains("larger than its declared uncompressed size"), "{e}");
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn a_tar_gz_that_unpacks_to_over_100_times_its_size_is_refused() {
        let t = std::env::temp_dir().join(format!("nvf-fomod-tar-bomb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(&t).unwrap();
        let mut tar_bytes = Vec::new();
        {
            let mut b = tar::Builder::new(&mut tar_bytes);
            let mut h = tar::Header::new_gnu();
            h.set_path("Mod/Plugin.esp").unwrap();
            h.set_size(1 << 20);
            h.set_mode(0o644);
            h.set_entry_type(tar::EntryType::Regular);
            h.set_cksum();
            b.append(&h, &vec![0u8; 1 << 20][..]).unwrap();
            b.finish().unwrap();
        }
        let bomb = t.join("Bomb.tar.gz");
        let mut gz = flate2::write::GzEncoder::new(std::fs::File::create(&bomb).unwrap(), flate2::Compression::best());
        gz.write_all(&tar_bytes).unwrap();
        gz.finish().unwrap();
        let e = stage_error(&bomb);
        assert!(e.ends_with(&too_big()), "{e}");
        let _ = std::fs::remove_dir_all(&t);
    }

    #[cfg(unix)]
    #[test]
    fn an_archive_unpacks_into_a_private_folder_and_no_entry_is_left_open_to_others() {
        use std::os::unix::fs::PermissionsExt;
        let t = std::env::temp_dir().join(format!("nvf-fomod-private-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(&t).unwrap();
        // A folder and a file whose modes in the archive let anyone write to them.
        let mut tar_bytes = Vec::new();
        {
            let mut b = tar::Builder::new(&mut tar_bytes);
            let mut dir = tar::Header::new_gnu();
            dir.set_path("Mod/textures/").unwrap();
            dir.set_size(0);
            dir.set_mode(0o777);
            dir.set_entry_type(tar::EntryType::Directory);
            dir.set_cksum();
            b.append(&dir, &[][..]).unwrap();
            let mut file = tar::Header::new_gnu();
            file.set_path("Mod/textures/a.dds").unwrap();
            file.set_size(4);
            file.set_mode(0o666);
            file.set_entry_type(tar::EntryType::Regular);
            file.set_cksum();
            b.append(&file, &b"DDS "[..]).unwrap();
            b.finish().unwrap();
        }
        let archive = t.join("Open.tar");
        std::fs::write(&archive, &tar_bytes).unwrap();
        let facts = FactsInput { active: HashSet::new(), present: HashSet::new(), paths: HashSet::new() };
        let Staged::Tree { temp, .. } = stage(&archive, facts).unwrap() else { panic!("one folder of textures is not a choice") };
        let root = temp.clone().unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&root), 0o700, "the install folder is this account's only");
        let mut stack = vec![root];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let p = entry.path();
                assert_eq!(mode(&p) & 0o002, 0, "{} is writable by anyone", p.display());
                if p.is_dir() {
                    stack.push(p);
                }
            }
        }
        discard(&temp);
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn a_zip_entry_with_a_colon_is_refused_as_a_tar_entry_is() {
        let t = std::env::temp_dir().join(format!("nvf-fomod-zip-colon-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(&t).unwrap();
        let zip_path = t.join("Colon.zip");
        {
            let mut z = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
            let o = zip::write::SimpleFileOptions::default();
            z.start_file("Mod/Plugin.esp", o).unwrap();
            z.write_all(b"TES4").unwrap();
            // On Windows this name writes a hidden stream of `Plugin.esp`.
            z.start_file("Mod/Plugin.esp:x", o).unwrap();
            z.write_all(b"TES4").unwrap();
            z.finish().unwrap();
        }
        // Refused either way: on Windows the zip reader's own check (`enclosed_name`) already
        // refuses the name, elsewhere the path rule does.
        let e = stage_error(&zip_path);
        let by_rule = e.ends_with(&format!("entry Mod/Plugin.esp:x: {}", rel_path("a:b").unwrap_err()));
        let by_reader = e.ends_with("entry Mod/Plugin.esp:x would land outside the folder");
        assert!(by_rule || by_reader, "{e}");
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn a_link_entry_stops_the_install() {
        let t = std::env::temp_dir().join(format!("nvf-fomod-link-entry-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(&t).unwrap();
        let zip_path = t.join("Link.zip");
        {
            let mut z = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
            let o = zip::write::SimpleFileOptions::default();
            z.start_file("Mod/Plugin.esp", o).unwrap();
            z.write_all(b"TES4").unwrap();
            z.add_symlink("Mod/textures", "/etc", o).unwrap();
            z.finish().unwrap();
        }
        let e = stage_error(&zip_path);
        assert!(e.ends_with("entry Mod/textures is a link"), "{e}");
        let mut tar_bytes = Vec::new();
        {
            let mut b = tar::Builder::new(&mut tar_bytes);
            let mut h = tar::Header::new_gnu();
            h.set_path("Mod/textures").unwrap();
            h.set_link_name("/etc").unwrap();
            h.set_size(0);
            h.set_mode(0o777);
            h.set_entry_type(tar::EntryType::Symlink);
            h.set_cksum();
            b.append(&h, &[][..]).unwrap();
            b.finish().unwrap();
        }
        let tar_path = t.join("Link.tar");
        std::fs::write(&tar_path, &tar_bytes).unwrap();
        let e = stage_error(&tar_path);
        assert!(e.ends_with("entry Mod/textures is not a file or folder"), "{e}");
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn a_deep_tree_is_copied_without_using_up_the_stack() {
        let t = std::env::temp_dir().join(format!("nvf-fomod-deep-tree-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let levels = 1000;
        let mut deep = t.join("src");
        for _ in 0..levels {
            deep.push("a");
        }
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("x.dds"), b"DDS ").unwrap();
        let (src, dest) = (t.join("src"), t.join("dest"));
        // A small stack: a copy that called itself once a folder would run out of it.
        let copied = std::thread::Builder::new().stack_size(256 * 1024).spawn(move || copy_tree(&src, &dest)).unwrap().join().unwrap();
        assert!(copied.is_ok(), "{copied:?}");
        let mut got = t.join("dest");
        for _ in 0..levels {
            got.push("a");
        }
        assert!(got.join("x.dds").is_file());
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn an_installer_that_copies_one_file_over_and_over_is_stopped() {
        let t = std::env::temp_dir().join(format!("nvf-fomod-copies-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let root = t.join("Copies");
        std::fs::create_dir_all(root.join("fomod")).unwrap();
        // 1 MiB: twenty copies pass both four times the files' size and the 16 MiB floor.
        std::fs::write(root.join("big.bsa"), noise(1024 * 1024)).unwrap();
        let installer = |copies: usize| {
            let files: String = (0..copies).map(|i| format!(r#"<file source="big.bsa" destination="copy{i}.bsa"/>"#)).collect();
            format!("<config><moduleName>Copies</moduleName><requiredInstallFiles>{files}</requiredInstallFiles></config>")
        };
        let facts = || FactsInput { active: HashSet::new(), present: HashSet::new(), paths: HashSet::new() };
        // Twenty copies of one file are past four times the files' size and the floor.
        std::fs::write(root.join("fomod").join("ModuleConfig.xml"), installer(20)).unwrap();
        let Staged::Plan(plan) = stage(&root, facts()).unwrap() else { panic!("an installer with no choice is a plan") };
        let mods = t.join("mods");
        let e = place_plan(&mods, &plan).unwrap_err();
        assert!(e.contains("copies more than 4 times the size of its files"), "{e}");
        assert!(!mods.join("Copies").exists(), "a stopped install leaves no folder");
        // Three copies fit.
        std::fs::write(root.join("fomod").join("ModuleConfig.xml"), installer(3)).unwrap();
        let Staged::Plan(plan) = stage(&root, facts()).unwrap() else { panic!("an installer with no choice is a plan") };
        place_plan(&mods, &plan).unwrap();
        assert!(mods.join("Copies").join("copy2.bsa").is_file());
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn a_tar_long_name_header_cannot_take_more_than_the_archive_allows() {
        use std::io::Read as _;
        let t = std::env::temp_dir().join(format!("nvf-fomod-long-name-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(&t).unwrap();
        // A GNU long-name header whose name is 32 MiB of one letter, then an empty file: the
        // tar reader would hold the whole name in memory. It packs into a few KiB.
        let name_len: u64 = 32 * 1024 * 1024;
        let mut tar_bytes = Vec::new();
        {
            let mut b = tar::Builder::new(&mut tar_bytes);
            let mut long = tar::Header::new_gnu();
            long.as_gnu_mut().unwrap().name[..13].copy_from_slice(b"././@LongLink");
            long.set_size(name_len);
            long.set_mode(0o644);
            long.set_entry_type(tar::EntryType::GNULongName);
            long.set_cksum();
            b.append(&long, std::io::repeat(b'a').take(name_len)).unwrap();
            let mut file = tar::Header::new_gnu();
            file.set_path("x").unwrap();
            file.set_size(0);
            file.set_mode(0o644);
            file.set_entry_type(tar::EntryType::Regular);
            file.set_cksum();
            b.append(&file, &[][..]).unwrap();
            b.finish().unwrap();
        }
        let bomb = t.join("LongName.tar.gz");
        let mut gz = flate2::write::GzEncoder::new(std::fs::File::create(&bomb).unwrap(), flate2::Compression::best());
        gz.write_all(&tar_bytes).unwrap();
        gz.finish().unwrap();
        assert!(std::fs::metadata(&bomb).unwrap().len() < 256 * 1024);
        let e = stage_error(&bomb);
        assert!(e.ends_with(&too_big()), "{e}");
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn installer_pictures_too_large_to_draw_or_too_many_to_hold_are_not_shown() {
        let t = std::env::temp_dir().join(format!("nvf-fomod-pictures-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let root = t.join("Pictures");
        std::fs::create_dir_all(root.join("fomod")).unwrap();
        std::fs::create_dir_all(root.join("img")).unwrap();
        let png = |name: &str, w: u32, h: u32| image::RgbaImage::new(w, h).save_with_format(root.join("img").join(name), image::ImageFormat::Png).unwrap();
        // One pixel wider than any picture shown, and a small one.
        png("wide.png", IMAGE_MAX_SIDE + 1, 1);
        png("small.png", 2, 2);
        // Seventeen of the largest size: with the small one, fifteen fit in the memory all
        // pictures may take.
        let full = 17;
        for i in 0..full {
            png(&format!("full{i}.png"), IMAGE_MAX_SIDE, IMAGE_MAX_SIDE);
        }
        let option = |name: &str, picture: &str| format!(r#"<plugin name="{name}"><description/><image path="img/{picture}"/><typeDescriptor><type name="Optional"/></typeDescriptor></plugin>"#);
        let mut plugins = option("Wide", "wide.png") + &option("Small", "small.png");
        for i in 0..full {
            plugins += &option(&format!("Full{i}"), &format!("full{i}.png"));
        }
        let xml = format!(
            r#"<config><moduleName>Pictures</moduleName><moduleImage path="img/wide.png"/><installSteps><installStep name="One"><optionalFileGroups><group name="Pick" type="SelectAny"><plugins>{plugins}</plugins></group></optionalFileGroups></installStep></installSteps></config>"#
        );
        std::fs::write(root.join("fomod").join("ModuleConfig.xml"), xml).unwrap();
        let facts = FactsInput { active: HashSet::new(), present: HashSet::new(), paths: HashSet::new() };
        let Staged::Wizard(w) = stage(&root, facts).unwrap() else { panic!("an installer with a choice opens its window") };
        let shown: Vec<bool> = w.steps[0].groups[0].plugins.iter().map(|p| p.image.is_some()).collect();
        assert!(w.image.is_none(), "the module picture is too wide");
        assert!(!shown[0], "too wide");
        assert!(shown[1], "small");
        assert_eq!(shown[2..].iter().filter(|s| **s).count(), 15, "the small one and fifteen of the largest fill the memory");
        assert!(!shown[full + 1], "the last of the largest is past it");
        let _ = std::fs::remove_dir_all(&t);
    }

    /// RESDISC-003: a folder entry read-only in the archive, after a file inside it, once made
    /// the install's temporary folder impossible to remove.
    #[cfg(unix)]
    #[test]
    fn a_tar_folder_mode_is_not_kept_so_the_folder_can_be_removed() {
        let t = std::env::temp_dir().join(format!("nvf-fomod-tar-modes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let dest = t.join("out");
        std::fs::create_dir_all(&dest).unwrap();
        let mut bytes = Vec::new();
        {
            let mut b = tar::Builder::new(&mut bytes);
            let mut file = tar::Header::new_gnu();
            file.set_path("d/x.esp").unwrap();
            file.set_size(4);
            file.set_mode(0o444);
            file.set_entry_type(tar::EntryType::Regular);
            file.set_cksum();
            b.append(&file, &b"TES4"[..]).unwrap();
            let mut dir = tar::Header::new_gnu();
            dir.set_path("d/").unwrap();
            dir.set_size(0);
            dir.set_mode(0o555);
            dir.set_entry_type(tar::EntryType::Directory);
            dir.set_cksum();
            b.append(&dir, &[][..]).unwrap();
            b.finish().unwrap();
        }
        let bad = |e: &dyn std::fmt::Display| e.to_string();
        unpack_tar(&bytes[..], &dest, u64::MAX, &bad).unwrap();
        assert_eq!(std::fs::read(dest.join("d/x.esp")).unwrap(), b"TES4");
        // A plain removal works: no mode from the archive was kept.
        std::fs::remove_dir_all(&dest).unwrap();
        let _ = std::fs::remove_dir_all(&t);
    }

    /// RESDISC-004/005: an outside tool may keep read-only modes; the temporary folder is still
    /// removed, and a link inside it is not followed.
    #[cfg(unix)]
    #[test]
    fn a_read_only_temporary_folder_is_still_removed_and_its_links_are_not_followed() {
        use std::os::unix::fs::PermissionsExt;
        let t = std::env::temp_dir().join(format!("nvf-fomod-read-only-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let temp = t.join("temp");
        let sub = temp.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("x.esp"), b"TES4").unwrap();
        let outside = t.join("outside.txt");
        std::fs::write(&outside, b"keep").unwrap();
        std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o444)).unwrap();
        std::os::unix::fs::symlink(&outside, temp.join("link")).unwrap();
        std::fs::set_permissions(sub.join("x.esp"), std::fs::Permissions::from_mode(0o444)).unwrap();
        std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o555)).unwrap();
        remove_temp(&temp);
        assert!(!temp.exists(), "the temporary folder is gone");
        assert_eq!(std::fs::metadata(&outside).unwrap().permissions().mode() & 0o777, 0o444, "the link's target was not touched");
        assert_eq!(std::fs::read(&outside).unwrap(), b"keep");
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn an_installer_name_with_a_line_break_becomes_one_folder_name() {
        let t = std::env::temp_dir().join(format!("nvf-fomod-name-break-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let root = t.join("Broken");
        std::fs::create_dir_all(root.join("fomod")).unwrap();
        std::fs::write(root.join("a.esp"), b"TES4").unwrap();
        let xml = r#"<config><moduleName>Line&#10;-Two&#9;</moduleName><requiredInstallFiles><file source="a.esp" destination="a.esp"/></requiredInstallFiles></config>"#;
        std::fs::write(root.join("fomod").join("ModuleConfig.xml"), xml).unwrap();
        let facts = FactsInput { active: HashSet::new(), present: HashSet::new(), paths: HashSet::new() };
        let Staged::Plan(plan) = stage(&root, facts).unwrap() else { panic!("an installer with no choice is a plan") };
        // A line break would have made `-Two` a second, turned-off row of the mod list.
        assert_eq!(plan.name, "Line-Two");
        assert_eq!(clean_name("\n\t"), Err("That name cannot be used.".to_string()));
        let _ = std::fs::remove_dir_all(&t);
    }

    /// LOSSYSTR-001: the window loads a picture from the exact path that was checked, never
    /// from an escaped name another file in the mod could carry.
    #[test]
    fn a_picture_is_loaded_from_the_path_that_was_checked() {
        let t = std::env::temp_dir().join(format!("nvf-fomod-picture-uri-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let root = t.join("Uri");
        std::fs::create_dir_all(root.join("fomod")).unwrap();
        std::fs::create_dir_all(root.join("img")).unwrap();
        let png = |name: &str| image::RgbaImage::new(2, 2).save_with_format(root.join("img").join(name), image::ImageFormat::Png).unwrap();
        png("a b.png");
        png("a%20b.png");
        let option = |name: &str, picture: &str| format!(r#"<plugin name="{name}"><description/><image path="img/{picture}"/><typeDescriptor><type name="Optional"/></typeDescriptor></plugin>"#);
        let plugins = option("Space", "a b.png") + &option("Percent", "a%20b.png");
        let xml = format!(r#"<config><moduleName>Uri</moduleName><installSteps><installStep name="One"><optionalFileGroups><group name="Pick" type="SelectAny"><plugins>{plugins}</plugins></group></optionalFileGroups></installStep></installSteps></config>"#);
        std::fs::write(root.join("fomod").join("ModuleConfig.xml"), xml).unwrap();
        let facts = FactsInput { active: HashSet::new(), present: HashSet::new(), paths: HashSet::new() };
        let Staged::Wizard(w) = stage(&root, facts).unwrap() else { panic!("an installer with a choice opens its window") };
        let plugs = &w.steps[0].groups[0].plugins;
        let space = plugs[0].image.as_deref().unwrap();
        let uri = file_uri(space).unwrap();
        assert!(uri.ends_with("a b.png") && !uri.contains("%20"), "{uri}");
        assert!(plugs[1].image.is_none(), "a name with % could stand for another file once decoded");
        // The window's own loader reads exactly that file from the URI, on Windows too.
        let ctx = egui::Context::default();
        egui_extras::install_image_loaders(&ctx);
        let mut loaded = None;
        for _ in 0..500 {
            match ctx.try_load_bytes(&uri) {
                Ok(egui::load::BytesPoll::Ready { bytes, .. }) => {
                    loaded = Some(bytes.to_vec());
                    break;
                }
                Ok(egui::load::BytesPoll::Pending { .. }) => std::thread::sleep(std::time::Duration::from_millis(10)),
                Err(e) => panic!("{uri}: {e}"),
            }
        }
        assert_eq!(loaded.as_deref(), Some(std::fs::read(space).unwrap().as_slice()), "{uri}");
        let _ = std::fs::remove_dir_all(&t);
    }

    /// RESEXHAUST-001: an installer naming one folder thousands of times does not make the
    /// clash check list every path it would write.
    #[test]
    fn the_paths_listed_for_the_clash_check_are_capped() {
        let t = std::env::temp_dir().join(format!("nvf-fomod-plan-list-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let root = t.join("Many");
        std::fs::create_dir_all(root.join("fomod")).unwrap();
        std::fs::create_dir_all(root.join("f")).unwrap();
        for i in 0..100 {
            std::fs::write(root.join("f").join(format!("{i}.txt")), b"").unwrap();
        }
        // 2000 destinations of 100 files: 200 000 paths.
        let folders: String = (0..2000).map(|i| format!(r#"<folder source="f" destination="d{i}"/>"#)).collect();
        let xml = format!("<config><moduleName>Many</moduleName><requiredInstallFiles>{folders}</requiredInstallFiles></config>");
        std::fs::write(root.join("fomod").join("ModuleConfig.xml"), xml).unwrap();
        let facts = FactsInput { active: HashSet::new(), present: HashSet::new(), paths: HashSet::new() };
        let Staged::Plan(plan) = stage(&root, facts).unwrap() else { panic!("an installer with no choice is a plan") };
        assert_eq!(plan_loose_files(&plan).len(), PLAN_LIST_MAX);
        let _ = std::fs::remove_dir_all(&t);
    }

    /// RESEXHAUST-001 of the fourth review: a long destination, repeated in every listed path,
    /// is refused when the installer is opened.
    #[test]
    fn an_installer_naming_a_very_long_path_is_refused() {
        let t = std::env::temp_dir().join(format!("nvf-fomod-long-path-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let root = t.join("Long");
        std::fs::create_dir_all(root.join("fomod")).unwrap();
        std::fs::create_dir_all(root.join("f")).unwrap();
        std::fs::write(root.join("f").join("a.txt"), b"a").unwrap();
        let long = "d".repeat(ITEM_PATH_MAX + 1);
        let xml = format!(r#"<config><moduleName>Long</moduleName><requiredInstallFiles><folder source="f" destination="{long}"/></requiredInstallFiles></config>"#);
        std::fs::write(root.join("fomod").join("ModuleConfig.xml"), xml).unwrap();
        let facts = FactsInput { active: HashSet::new(), present: HashSet::new(), paths: HashSet::new() };
        let Err(e) = stage(&root, facts) else { panic!("a path past the limit opens no installer") };
        assert!(e.contains("longer than 1024 bytes"), "{e}");
        let _ = std::fs::remove_dir_all(&t);
    }

    /// RESEXHAUST-003: a small tar.gz of empty entries cannot flood the temporary folder: each
    /// entry counts as a block against the unpack limit.
    #[test]
    fn a_tar_gz_of_many_empty_entries_is_refused() {
        let t = std::env::temp_dir().join(format!("nvf-fomod-empty-entries-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(&t).unwrap();
        let mut tar_bytes = Vec::new();
        {
            let mut b = tar::Builder::new(&mut tar_bytes);
            for i in 0..50_000 {
                let mut h = tar::Header::new_gnu();
                h.set_path(format!("Mod/{i}.txt")).unwrap();
                h.set_size(0);
                h.set_mode(0o644);
                h.set_entry_type(tar::EntryType::Regular);
                h.set_cksum();
                b.append(&h, &[][..]).unwrap();
            }
            b.finish().unwrap();
        }
        let flood = t.join("Flood.tar.gz");
        let mut gz = flate2::write::GzEncoder::new(std::fs::File::create(&flood).unwrap(), flate2::Compression::best());
        gz.write_all(&tar_bytes).unwrap();
        gz.finish().unwrap();
        let e = stage_error(&flood);
        assert!(e.ends_with(&too_big()), "{e}");
        let _ = std::fs::remove_dir_all(&t);
    }

    /// RESEXHAUST-002: copies of empty files and folders count too, so an installer cannot fill
    /// the disk's file table with them.
    #[test]
    fn an_installer_copying_empty_files_over_and_over_is_stopped() {
        let t = std::env::temp_dir().join(format!("nvf-fomod-empty-copies-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let root = t.join("Empty");
        std::fs::create_dir_all(root.join("fomod")).unwrap();
        std::fs::create_dir_all(root.join("f")).unwrap();
        for i in 0..100 {
            std::fs::write(root.join("f").join(format!("{i}.txt")), b"").unwrap();
        }
        let folders: String = (0..100).map(|i| format!(r#"<folder source="f" destination="d{i}"/>"#)).collect();
        let xml = format!("<config><moduleName>Empty</moduleName><requiredInstallFiles>{folders}</requiredInstallFiles></config>");
        std::fs::write(root.join("fomod").join("ModuleConfig.xml"), xml).unwrap();
        let facts = FactsInput { active: HashSet::new(), present: HashSet::new(), paths: HashSet::new() };
        let Staged::Plan(plan) = stage(&root, facts).unwrap() else { panic!("an installer with no choice is a plan") };
        let mods = t.join("mods");
        let e = place_plan(&mods, &plan).unwrap_err();
        assert!(e.contains("copies more than 4 times the size of its files"), "{e}");
        assert!(!mods.join("Empty").exists(), "a stopped install leaves no folder");
        let _ = std::fs::remove_dir_all(&t);
    }

    /// RESEXHAUST-002 of the fourth review: the folders a deep destination makes count against the
    /// copy limit, so an installer cannot make millions of them from one small file.
    #[test]
    fn an_installer_making_many_deep_folders_is_stopped() {
        let t = std::env::temp_dir().join(format!("nvf-fomod-deep-dest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let root = t.join("Deep");
        std::fs::create_dir_all(root.join("fomod")).unwrap();
        std::fs::write(root.join("a.txt"), b"a").unwrap();
        // 500 files, each 11 new folders deep: about 6000 entries of 4 KiB, past the 16 MiB floor.
        // Under `textures`, so without the limit the install would be kept.
        let files: String = (0..500).map(|i| format!(r#"<file source="a.txt" destination="textures/d{i}/a/b/c/d/e/f/g/h/i/j/x.dds"/>"#)).collect();
        let xml = format!("<config><moduleName>Deep</moduleName><requiredInstallFiles>{files}</requiredInstallFiles></config>");
        std::fs::write(root.join("fomod").join("ModuleConfig.xml"), xml).unwrap();
        let facts = FactsInput { active: HashSet::new(), present: HashSet::new(), paths: HashSet::new() };
        let Staged::Plan(plan) = stage(&root, facts).unwrap() else { panic!("an installer with no choice is a plan") };
        let mods = t.join("mods");
        let e = place_plan(&mods, &plan).unwrap_err();
        assert!(e.contains("copies more than 4 times the size of its files"), "{e}");
        assert!(!mods.join("Deep").exists(), "a stopped install leaves no folder");
        let _ = std::fs::remove_dir_all(&t);
    }

    /// A tar made with `tar -C folder -cf mod.tar .` starts with a `./` entry: it installs.
    #[test]
    fn a_tar_with_a_dot_slash_entry_installs() {
        let t = std::env::temp_dir().join(format!("nvf-fomod-dot-slash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(&t).unwrap();
        let mut bytes = Vec::new();
        {
            let mut b = tar::Builder::new(&mut bytes);
            // The names go into the headers as raw bytes, as `tar` writes them.
            let header = |name: &[u8], size: u64, kind: tar::EntryType| {
                let mut h = tar::Header::new_gnu();
                h.as_gnu_mut().unwrap().name[..name.len()].copy_from_slice(name);
                h.set_size(size);
                h.set_mode(0o755);
                h.set_entry_type(kind);
                h.set_cksum();
                h
            };
            b.append(&header(b"./", 0, tar::EntryType::Directory), &[][..]).unwrap();
            b.append(&header(b"./Mod/", 0, tar::EntryType::Directory), &[][..]).unwrap();
            b.append(&header(b"./Mod/Plugin.esp", 4, tar::EntryType::Regular), &b"TES4"[..]).unwrap();
            b.finish().unwrap();
        }
        let archive = t.join("Dot.tar");
        std::fs::write(&archive, &bytes).unwrap();
        let facts = FactsInput { active: HashSet::new(), present: HashSet::new(), paths: HashSet::new() };
        let Staged::Tree { from, temp, .. } = stage(&archive, facts).unwrap() else { panic!("one plugin is not a choice") };
        assert!(from.join("Plugin.esp").is_file() || from.join("Mod").join("Plugin.esp").is_file(), "{}", from.display());
        discard(&temp);
        let _ = std::fs::remove_dir_all(&t);
    }
}
