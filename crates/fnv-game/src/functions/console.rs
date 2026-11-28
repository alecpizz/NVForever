//! Console-only commands that print (docs/design/console.md): the catalogue lists them
//! without an opcode, so the function table numbers them from
//! `fnv_script::table::CONSOLE_OPCODE_BASE`; the console runs the ones registered here
//! by catalogue name ([`registry`], checked by a test) with the line's words. Vanilla
//! functions the catalogue gives no opcode and no console category (KillAllActors) are
//! found by name and alias in the table's unnumbered functions ([`lookup_word`]).
//!
//! A command takes the console line's words after its name and returns the lines to print.
//! What only the engine can do (travel, quit, menus, saving) is asked through
//! [`Game::console_requests`]. What each command prints is our design; the original's
//! wording is not copied.

use crate::game::{Game, InstKey};
use fnv_script::vm::Value;
use fnv_sim::settings::Setting;
use fnv_sim::FormId;

/// What a console command runs with.
#[derive(Debug, Clone, Default)]
pub struct ConsoleCtx {
    /// `reference.Command`, else the console's selected reference.
    pub on: Option<FormId>,
    /// The Pip-Boy's active quest, if the engine has one.
    pub active_quest: Option<FormId>,
}

/// A bounded first-level preview. Nested arrays are labeled rather than traversed.
pub(crate) fn expression_result(g: &Game, value: &fnv_script::vm::XVal) -> String {
    use fnv_script::vm::{number_text, XKey, XVal};
    use std::fmt::Write as _;

    fn atom(g: &Game, line: &mut String, value: &XVal) {
        match value {
            XVal::Num(n) => line.push_str(&number_text(*n)),
            XVal::Ref(0) => line.push_str("none"),
            XVal::Ref(r) => { let _ = write!(line, "{} {}", FormId(*r), g.data.name(FormId(*r))); }
            XVal::Str(s) => { let _ = write!(line, "{s:?}"); }
            XVal::Array(id) => { let _ = write!(line, "array #{id}"); }
        }
    }

    let mut line = String::new();
    if let XVal::Array(id) = value {
        let Some(array) = g.ext.array(*id) else { return "(no array)".into() };
        const PREVIEW: usize = 16;
        line.push('[');
        for (i, (key, value)) in array.entries.iter().take(PREVIEW).enumerate() {
            if i > 0 {
                line.push_str(", ");
            }
            match key {
                XKey::Num(n) => line.push_str(&number_text(*n)),
                XKey::Str(s) => { let _ = write!(line, "{s:?}"); }
            }
            line.push_str(": ");
            atom(g, &mut line, value);
        }
        if array.entries.len() > PREVIEW {
            let _ = write!(line, ", … ({} more)", array.entries.len() - PREVIEW);
        }
        line.push(']');
    } else {
        atom(g, &mut line, value);
    }
    line
}

/// Something only the engine can do, asked by a console command.
#[derive(Debug, Clone, PartialEq)]
pub enum ConsoleRequest {
    /// Put the player at `feet` (game units) in `interior`, or outdoors in `world`.
    Travel { world: Option<FormId>, interior: Option<FormId>, feet: [f32; 3], heading_deg: f32, label: String },
    /// Select a reference (`PickRefByID`).
    Select(FormId),
    Quit,
    CloseMenus,
    Save(String),
    /// A save by name; empty for the quick save.
    Load(String),
    /// The lockpicking menu on a practice lock of that difficulty, no reference's
    /// (`ShowLockPickMenu`).
    Lockpick(crate::lockpick::Level),
    /// A location marker at `pos` (game units) with its label and colour 0-9
    /// (`PlaceLocationMarker`).
    PlaceMarker { pos: [f32; 3], name: String, color: u8 },
    /// The location markers of the current place go (`ClearLocationMarkers`).
    ClearMarkers,
    /// The first-person model shown, or no longer, beside the third-person one
    /// (`Show1stPerson`).
    Show1stPerson,
    /// A reference's velocity along one axis set, in game units per second (`SetVel`).
    SetVel { on: FormId, axis: char, speed: f32 },
}

pub type ConsoleImpl = fn(&mut Game, &ConsoleCtx, &[String]) -> Result<Vec<String>, String>;

/// Every registered console-only command: catalogue name, implementation.
pub fn registry() -> &'static [(&'static str, ConsoleImpl)] {
    &[
        ("ShowQuestTargets", show_quest_targets),
        ("MoveToQuestTarget", move_to_quest_target),
        ("ShowQuestStages", show_quest_stages),
        ("ShowQuestVars", show_quest_vars),
        ("ShowVars", show_vars),
        ("ShowQuests", show_quests),
        ("CompleteAllQuestStages", complete_all_quest_stages),
        ("ShowInventory", show_inventory),
        ("GetActorValueInfo", get_actor_value_info),
        ("PickRefByID", pick_ref_by_id),
        ("SetGameSetting", set_game_setting),
        ("SetStringSetting", set_string_setting),
        ("GetINISetting", no_ini),
        ("SetINISetting", no_ini),
        ("QuitGame", quit_game),
        ("CloseAllMenus", close_all_menus),
        ("SaveGame", save_game),
        ("LoadGame", load_game),
        ("KillAllActors", kill_all_actors),
        ("StartAllQuests", start_all_quests),
        ("ResetDialogueFlags", reset_dialogue_flags),
        ("ResetAllTerminals", reset_all_terminals),
        ("DeleteReference", delete_reference),
        ("ToggleFogOfWar", toggle_fog_of_war),
        ("ShowLockPickMenu", show_lock_pick_menu),
        ("MFG", modify_face_gen),
        ("PlaceLocationMarker", place_location_marker),
        ("ClearLocationMarkers", clear_location_markers),
        ("Show1stPerson", show_1st_person),
        ("SetVel", set_vel),
        ("Show", show_variable),
        ("PersistentRefCount", persistent_ref_count),
        ("TestRaceSexTerimnal", test_race_sex_terminal),
        ("ClearScreenBlood", clear_screen_blood),
        ("SetDefaultOpen", set_default_open),
    ]
}

/// Vanilla functions the engine's console handles itself by name, outside both
/// registries (engine/nvf-play/src/console.rs): `coc`, `cow` and `coe` travel, `help`
/// searches records, `AdvLevel` levels up, `bat` runs a file's lines, `clr` empties the
/// scrollback, `rbat` records lines for `bat`, `scof` writes the console to a file.
pub const ENGINE_WORDS: &[&str] = &["CenterOnCell", "CenterOnWorld", "CenterOnExterior", "Help", "AdvancePCLevel", "bat", "clr", "rbat", "SetConsoleOuputFile"];

/// The vanilla functions of the catalogue that nothing handles and no script of the
/// masters calls (so the stub audit, `fnvtool script stubs`, never counts them): those
/// in the wiki's "Console Functions" category, then the others. Typed at the console
/// they print "unknown" or do nothing (docs/design/console.md, "What the console does not handle").
pub fn unhandled_unused(table_json: &serde_json::Value) -> (Vec<String>, Vec<String>) {
    let (mut console, mut other) = (Vec::new(), Vec::new());
    for f in table_json["functions"].as_array().into_iter().flatten() {
        let Some(name) = f["name"].as_str() else { continue };
        if f["family"] != "vanilla" || f["script_calls"].as_u64().unwrap_or(0) > 0 {
            continue;
        }
        if super::lookup_name(name).is_some() || lookup(name).is_some() || super::player_source::handles(name) || super::refs::handles_source(name) || crate::vats::handles_area_query(name) || ENGINE_WORDS.iter().any(|w| w.eq_ignore_ascii_case(name)) {
            continue;
        }
        let in_console = f["categories"].as_array().into_iter().flatten().any(|c| c == "Console Functions");
        if in_console { console.push(name.to_string()) } else { other.push(name.to_string()) }
    }
    console.sort();
    other.sort();
    (console, other)
}

/// The command a console word names: a console-only command numbered from
/// `CONSOLE_OPCODE_BASE`, or a vanilla function without an opcode (by name or alias, the
/// table's unnumbered functions), when one is registered here. Its catalogue name too.
pub fn lookup_word(table: &fnv_script::table::FunctionTable, word: &str) -> Option<(String, ConsoleImpl)> {
    let numbered = table.by_name(word).filter(|&op| op >= fnv_script::table::CONSOLE_OPCODE_BASE).and_then(|op| table.get(op)).map(|f| f.name.clone());
    let name = numbered.or_else(|| table.unnumbered.get(&word.to_ascii_lowercase()).map(|u| u.name.clone()))?;
    lookup(&name).map(|run| (name, run))
}

/// The implementation registered under a catalogue name (case-insensitive).
pub fn lookup(name: &str) -> Option<ConsoleImpl> {
    registry().iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|r| r.1)
}

/// A word as a form: `player`, an Editor ID, a FormID with or without `0x`.
pub fn form(g: &Game, s: &str) -> Result<FormId, String> {
    let s = s.trim_matches('"');
    crate::runner::resolve(g, s).or_else(|e| {
        let h = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
        if !h.is_empty() && h.len() <= 8 && h.chars().all(|c| c.is_ascii_hexdigit()) {
            u32::from_str_radix(h, 16).map(FormId).map_err(|_| e)
        } else {
            Err(e)
        }
    })
}

fn quest_arg(g: &Game, a: &[String]) -> Result<FormId, String> {
    let q = form(g, a.first().ok_or("a quest's Editor ID or FormID")?)?;
    if g.data.is_quest(q) {
        Ok(q)
    } else {
        Err(format!("{} is not a quest", g.data.name(q)))
    }
}

/// A reference or record for printing: `FormID EditorID`.
fn label(g: &Game, r: FormId) -> String {
    if r == FormId::PLAYER_REF {
        return format!("{r} player");
    }
    let e = g.data.name(r);
    if e == r.to_string() {
        match g.world.placed.get(r).and_then(|p| p.edid.clone()) {
            Some(e) => format!("{r} {e}"),
            None => format!("{r} ({})", g.data.name(crate::functions::base_of(g, r).unwrap_or(r))),
        }
    } else {
        format!("{r} {e}")
    }
}

fn shown(v: Value, g: &Game) -> String {
    match v {
        Value::Num(n) if n.fract() == 0.0 => format!("{n:.0}"),
        Value::Num(n) => format!("{n:.2}"),
        Value::Ref(0) => "none".into(),
        Value::Ref(r) => label(g, FormId(r)),
    }
}

// ---- quests ------------------------------------------------------------------------------

/// One target of an objective shown and not done.
struct QuestTarget {
    quest: FormId,
    objective: i32,
    target: FormId,
    cell: Option<FormId>,
    pos: [f32; 3],
    rot_z: f32,
    ignores_locks: bool,
    /// Its conditions pass (`crate::quest_targets`): it is marked.
    shown: bool,
}

/// The quests whose targets the console shows: the active quest, else every running quest
/// with an objective shown and not done [hypothesis: the original's ShowQuestTargets
/// reports the active quest; with none picked we show them all rather than nothing].
fn target_quests(g: &Game, ctx: &ConsoleCtx) -> Vec<FormId> {
    if let Some(q) = ctx.active_quest.filter(|q| g.quests.get(q).is_some_and(|s| s.running)) {
        return vec![q];
    }
    g.quests.iter().filter(|(_, s)| s.running && s.objectives.values().any(|o| o.displayed && !o.completed)).map(|(&q, _)| q).collect()
}

/// Every target of the objectives shown and not done, where it is now, and whether its
/// conditions pass now (as the map markers, `pipboy::data::quest_targets`).
fn quest_targets(g: &mut Game, ctx: &ConsoleCtx) -> Vec<QuestTarget> {
    let mut out = Vec::new();
    for q in target_quests(g, ctx) {
        for c in g.check_quest_targets(q) {
            let r = c.target;
            let (cell, pos, rot) = match g.world.peek(r) {
                Some(s) => (s.cell, s.pos, s.rot),
                None => match g.world.placed.get(r) {
                    Some(p) => (p.cell, p.pos, p.rot),
                    None => continue,
                },
            };
            out.push(QuestTarget { quest: q, objective: c.objective, target: r, cell, pos, rot_z: rot[2], ignores_locks: c.ignores_locks, shown: c.shown });
        }
    }
    out
}

/// The door outdoors a marker for a target in an interior cell stands at, if a way leads
/// there: (its cell, position) (`quest_targets::outdoor_marker_door`: a door straight in,
/// else the first door outdoors of the way through other interiors, #2458).
fn door_into(g: &mut Game, cell: FormId, ignores_locks: bool) -> Option<(FormId, [f32; 3])> {
    let direct = g.doors_into(cell);
    let refs = g.world.placed;
    let graph = g.other.objective_doors.get_or_insert_with(|| crate::quest_targets::DoorGraph::build(refs));
    let door = crate::quest_targets::outdoor_marker_door(&g.world, graph, &direct, cell, ignores_locks)?;
    let d = g.world.placed.get(door)?;
    Some((d.cell?, d.pos))
}

fn cell_label(g: &Game, cell: Option<FormId>) -> String {
    match cell {
        None => "no cell".into(),
        Some(c) => match g.world.placed.cell_world.get(&c) {
            Some(w) => format!("{} {} (exterior)", c, g.data.name(*w)),
            None => format!("{} {}", c, g.data.name(c)),
        },
    }
}

/// `ShowQuestTargets` (`sqt`): each target of the active quest's objectives shown and not
/// done whose conditions pass: quest, objective, target reference, its cell and position,
/// and for a target indoors the door outdoors that leads to it; then each target whose
/// conditions fail, marked as hidden.
pub fn show_quest_targets(g: &mut Game, ctx: &ConsoleCtx, _a: &[String]) -> Result<Vec<String>, String> {
    let ts = quest_targets(g, ctx);
    if ts.is_empty() {
        return Ok(vec!["No quest targets (no objective shown and not done)".into()]);
    }
    let mut out = Vec::new();
    for t in ts.iter().filter(|t| t.shown) {
        out.push(format!(
            "{} objective {}: target {} in {} at {:.0} {:.0} {:.0}",
            g.data.name(t.quest),
            t.objective,
            label(g, t.target),
            cell_label(g, t.cell),
            t.pos[0],
            t.pos[1],
            t.pos[2]
        ));
        if let Some(c) = t.cell.filter(|c| !g.world.placed.cell_world.contains_key(c)) {
            if let Some((dc, p)) = door_into(g, c, t.ignores_locks) {
                out.push(format!("  load door outdoors in {} at {:.0} {:.0} {:.0}", cell_label(g, Some(dc)), p[0], p[1], p[2]));
            }
        }
    }
    for t in ts.iter().filter(|t| !t.shown) {
        out.push(format!("{} objective {}: target {} hidden (its conditions fail)", g.data.name(t.quest), t.objective, label(g, t.target)));
    }
    Ok(out)
}

/// `MoveToQuestTarget [index]` (`movetoqt`): the player travels to a marked target of the
/// active quest (`index` from 0 among `sqt`'s list, else the first), arriving at the
/// target's position [hypothesis: the original puts the player at the target; collision
/// then moves the player out of it].
pub fn move_to_quest_target(g: &mut Game, ctx: &ConsoleCtx, a: &[String]) -> Result<Vec<String>, String> {
    let ts: Vec<QuestTarget> = quest_targets(g, ctx).into_iter().filter(|t| t.shown).collect();
    let i = match a.first() {
        Some(w) => w.parse::<usize>().map_err(|_| format!("{w:?} is not an index"))?,
        None => 0,
    };
    let t = ts.get(i).ok_or_else(|| if ts.is_empty() { "No quest target to move to".to_string() } else { format!("Only {} quest target(s)", ts.len()) })?;
    let cell = t.cell.ok_or("the target is in no cell")?;
    let world = g.world.placed.cell_world.get(&cell).copied();
    let label = format!("{} objective {} ({})", g.data.name(t.quest), t.objective, label(g, t.target));
    g.console_requests.push(ConsoleRequest::Travel {
        world,
        interior: if world.is_some() { None } else { Some(cell) },
        feet: t.pos,
        heading_deg: t.rot_z.to_degrees(),
        label: label.clone(),
    });
    Ok(vec![format!("Moving to {label}")])
}

/// `ShowQuestStages <quest>` (`sqs`): every stage the quest defines, and whether it is done.
pub fn show_quest_stages(g: &mut Game, _ctx: &ConsoleCtx, a: &[String]) -> Result<Vec<String>, String> {
    let q = quest_arg(g, a)?;
    let def = &g.data.quests[&q];
    let state = g.quests.get(&q);
    let mut out = vec![format!(
        "{}: {}{}stage {}",
        g.data.name(q),
        if state.is_some_and(|s| s.running) { "running, " } else { "not running, " },
        if state.is_some_and(|s| s.completed) { "completed, " } else { "" },
        state.map(|s| s.stage()).unwrap_or(0)
    )];
    for st in def.stages.keys() {
        let done = state.is_some_and(|s| s.stages_done.contains(st));
        out.push(format!("  stage {st}: {}", if done { "done" } else { "-" }));
    }
    Ok(out)
}

/// The variables of an instance as lines: every variable its script declares, with its
/// value now (0 when never set).
fn var_lines(g: &Game, key: InstKey, script: FormId) -> Vec<String> {
    let Some(c) = g.data.script(script) else { return vec![format!("script {} is not compiled", g.data.name(script))] };
    let mut vars: Vec<(u16, String)> = c.unit.variables.iter().map(|v| (v.index as u16, v.name.clone())).collect();
    vars.sort();
    if vars.is_empty() {
        return vec![format!("script {} declares no variables", g.data.name(script))];
    }
    vars.into_iter().map(|(i, n)| format!("  {n} = {}", shown(g.var(key, i), g))).collect()
}

/// `ShowQuestVars <quest>` (`sqv`): the quest script's variables and values.
pub fn show_quest_vars(g: &mut Game, _ctx: &ConsoleCtx, a: &[String]) -> Result<Vec<String>, String> {
    let q = quest_arg(g, a)?;
    let (key, script) = g.instance_of(q).ok_or_else(|| format!("{} runs no script", g.data.name(q)))?;
    let mut out = vec![format!("{} (script {}):", g.data.name(q), g.data.name(script))];
    out.extend(var_lines(g, key, script));
    Ok(out)
}

/// `ShowVars` (`sv`): the selected reference's script variables (or a quest's, named).
pub fn show_vars(g: &mut Game, ctx: &ConsoleCtx, a: &[String]) -> Result<Vec<String>, String> {
    let r = match a.first() {
        Some(w) => form(g, w)?,
        None => ctx.on.ok_or("select a reference first (click it, or prid)")?,
    };
    let (key, script) = g.instance_of(r).ok_or_else(|| format!("{} runs no script", label(g, r)))?;
    let mut out = vec![format!("{} (script {}):", label(g, r), g.data.name(script))];
    out.extend(var_lines(g, key, script));
    Ok(out)
}

/// `ShowQuests` (`sq`): the running quests with their stage, and the completed ones.
pub fn show_quests(g: &mut Game, _ctx: &ConsoleCtx, _a: &[String]) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for (q, s) in g.quests.iter().filter(|(_, s)| s.running) {
        let name = g.data.quests.get(q).and_then(|d| d.name.clone()).map(|n| format!(" \"{n}\"")).unwrap_or_default();
        out.push(format!("{} {}{name}: stage {}{}", q, g.data.name(*q), s.stage(), if s.completed { ", completed" } else { "" }));
    }
    if out.is_empty() {
        out.push("No quest is running".into());
    }
    let done = g.quests.values().filter(|s| !s.running && (s.completed || s.failed)).count();
    let running = g.quests.values().filter(|s| s.running).count();
    out.push(format!("{running} running, {done} finished and stopped"));
    Ok(out)
}

/// `CompleteAllQuestStages` (`caqs`): every stage of every quest is marked done
/// [hypothesis: their result scripts are not run, so the world is not changed by
/// hundreds of stage scripts at once; quests are not marked completed].
pub fn complete_all_quest_stages(g: &mut Game, _ctx: &ConsoleCtx, _a: &[String]) -> Result<Vec<String>, String> {
    let data = g.data;
    let mut n = 0usize;
    for (q, def) in &data.quests {
        let s = g.quest(*q);
        for st in def.stages.keys() {
            n += usize::from(s.stages_done.insert(*st));
        }
    }
    Ok(vec![format!("{n} stage(s) of {} quests marked done (stage scripts not run)", data.quests.len())])
}

// ---- references and actors ---------------------------------------------------------------

/// `ShowInventory` (`inv`): the selected reference's items (the player without one): count,
/// FormID, Editor ID and name, equipped, and condition where it is kept.
pub fn show_inventory(g: &mut Game, ctx: &ConsoleCtx, _a: &[String]) -> Result<Vec<String>, String> {
    let r = ctx.on.unwrap_or(FormId::PLAYER_REF);
    let who = label(g, r);
    let groups: Vec<_> = g.world.inventory_mut(r).map_err(|e| format!("{who}: {e}"))?.groups().collect();
    let mut out = vec![format!("{who}: {} stack(s)", groups.len())];
    for (item, extra, n) in groups {
        let name = g.world.forms.get(item).and_then(|i| i.name.clone()).map(|n| format!(" \"{n}\"")).unwrap_or_default();
        let worn = if extra.equipped { " (equipped)" } else { "" };
        let cond = extra.condition.map(|c| format!(" condition {c:.0}")).unwrap_or_default();
        out.push(format!("  {n} x {} {}{name}{worn}{cond}", item, g.data.name(item)));
    }
    Ok(out)
}

/// `GetActorValueInfo <actor value>` (`getavinfo`): the actor value's parts on the selected
/// actor (the player without one): base, modifier, damage and the value now.
pub fn get_actor_value_info(g: &mut Game, ctx: &ConsoleCtx, a: &[String]) -> Result<Vec<String>, String> {
    let r = ctx.on.unwrap_or(FormId::PLAYER_REF);
    let name = a.first().ok_or("an actor value: GetAVInfo Strength")?;
    let code = crate::functions::av_code(g, name)?;
    let [base, modifier, damage] = crate::functions::actor_value_parts(g, r, code)?;
    let now = crate::functions::actor_value(g, r, code)?;
    let av = g.data.table.enum_name("actor_value", code).unwrap_or(name).to_string();
    Ok(vec![format!("{av} ({code}) on {}: now {now:.2} = base {base:.2} + modifier {modifier:.2} - damage {damage:.2}", label(g, r))])
}

/// `PickRefByID <reference>` (`prid`): select it for the console.
pub fn pick_ref_by_id(g: &mut Game, _ctx: &ConsoleCtx, a: &[String]) -> Result<Vec<String>, String> {
    let r = form(g, a.first().ok_or("prid <reference>")?)?;
    g.console_requests.push(ConsoleRequest::Select(r));
    Ok(Vec::new())
}

// ---- settings ----------------------------------------------------------------------------

fn setting_text(s: &Setting) -> String {
    match s {
        Setting::Float(v) => format!("{v}"),
        Setting::Int(v) => format!("{v}"),
        Setting::Str(v) => format!("\"{v}\""),
        Setting::Raw(b) => format!("{} bytes", b.len()),
    }
}

/// `SetGameSetting <setting> <value>` (`setgs`): change a game setting for this run. The
/// value's kind follows the setting's (its Editor ID's first letter). Readers of the game
/// layer (`World::setting_f32`, `GetGameSetting`) see it; the change is not saved, and
/// values the engine read at start (from the records) do not change [hypothesis: the
/// original keeps it until it quits, not in the save].
pub fn set_game_setting(g: &mut Game, _ctx: &ConsoleCtx, a: &[String]) -> Result<Vec<String>, String> {
    let (Some(name), Some(v)) = (a.first(), a.get(1)) else { return Err("setgs <setting> <value>".into()) };
    let old = g.world.setting_value(name).cloned().ok_or_else(|| format!("No game setting {name}"))?;
    let new = match old {
        Setting::Float(_) => Setting::Float(v.parse().map_err(|_| format!("{name} takes a number"))?),
        Setting::Int(_) => Setting::Int(v.parse::<f64>().map_err(|_| format!("{name} takes a whole number"))? as i32),
        Setting::Str(_) => Setting::Str(a[1..].join(" ").trim_matches('"').to_string()),
        Setting::Raw(_) => return Err(format!("{name}: its value is not read by this engine")),
    };
    g.world.change_setting(name, new.clone()).map_err(|e| e.to_string())?;
    Ok(vec![format!("{name}: {} -> {}", setting_text(&old), setting_text(&new))])
}

/// `SetStringSetting <setting> <text>`: as `setgs` for a string setting.
pub fn set_string_setting(g: &mut Game, ctx: &ConsoleCtx, a: &[String]) -> Result<Vec<String>, String> {
    match a.first().and_then(|n| g.world.setting_value(n)) {
        Some(Setting::Str(_)) => set_game_setting(g, ctx, a),
        Some(_) => Err(format!("{} is not a string setting", a[0])),
        None => Err("SetStringSetting <setting> <text>".into()),
    }
}

/// `GetINISetting` / `SetINISetting`: the engine keeps no INI settings store (the values
/// it takes from the original's INIs are written into the code where they are used).
pub fn no_ini(_g: &mut Game, _ctx: &ConsoleCtx, _a: &[String]) -> Result<Vec<String>, String> {
    Err("INI settings are not built: the engine keeps no store of the original's INI values".into())
}

// ---- the game ----------------------------------------------------------------------------

/// `QuitGame` (`qqq`): the engine exits.
pub fn quit_game(g: &mut Game, _ctx: &ConsoleCtx, _a: &[String]) -> Result<Vec<String>, String> {
    g.console_requests.push(ConsoleRequest::Quit);
    Ok(vec!["Quitting".into()])
}

/// `StartAllQuests` (`saq`): every quest of the load order is started ("Starts every quest
/// in the game" [documented: GECK wiki StartAllQuests]), each as StartQuest starts one;
/// those running already are left as they are.
pub fn start_all_quests(g: &mut Game, _ctx: &ConsoleCtx, _a: &[String]) -> Result<Vec<String>, String> {
    let quests: Vec<FormId> = g.data.quests.keys().copied().collect();
    let started = quests.iter().filter(|&&q| !g.quest_state(q).running && g.start_quest(q)).count();
    Ok(vec![format!("{started} quests started ({} in all)", quests.len())])
}

/// `ResetDialogueFlags` (`rdf`): the Say Once and Say Once a Day lines may be said again
/// ("Currently resets the say-once and say-once-per-day flags" [documented: GECK wiki
/// ResetDialogueFlags]). The record of lines said to the player goes with them, so their
/// choices are no longer shaded [hypothesis: one set holds both here, `Game::said`].
pub fn reset_dialogue_flags(g: &mut Game, _ctx: &ConsoleCtx, _a: &[String]) -> Result<Vec<String>, String> {
    let (once, daily) = (g.said.len(), g.said_day.values().map(|m| m.len()).sum::<usize>());
    g.said.clear();
    g.said_day.clear();
    Ok(vec![format!("Dialogue flags reset: {once} lines said once, {daily} once a day")])
}

/// `ResetAllTerminals` (`rat`): every terminal is as if never hacked: hacked ones must be
/// hacked again, locked-out ones may be tried again ("Clears all variables changed during
/// hacking, resetting terminals as if never hacked" [documented: GECK wiki
/// ResetAllTerminals]). What scripts set with Lock and Unlock stays: no hacking set it.
pub fn reset_all_terminals(g: &mut Game, _ctx: &ConsoleCtx, _a: &[String]) -> Result<Vec<String>, String> {
    let n = g.terminals.access.len();
    g.terminals.access.clear();
    g.terminals.retried.clear();
    Ok(vec![format!("{n} terminals reset")])
}

/// `[ref.]DeleteReference` (`zap`): the reference (else the console's selection) is
/// removed from the world at once ("immediately removes a reference" [documented: GECK
/// wiki DeleteReference]); the save keeps it removed, as MarkForDelete's. Never the
/// player.
pub fn delete_reference(g: &mut Game, ctx: &ConsoleCtx, _a: &[String]) -> Result<Vec<String>, String> {
    let r = ctx.on.ok_or("select a reference first, or ref.zap")?;
    if r == FormId::PLAYER_REF {
        return Err("not the player".into());
    }
    let what = label(g, r);
    g.extra.entry(r).or_default().marked_for_delete = true;
    g.world.set_deleted(r, true).map_err(|e| e.to_string())?;
    Ok(vec![format!("{what} deleted")])
}

/// `ToggleFogOfWar` (`tfow`): "turns local map fog of war on or off" [documented: GECK
/// wiki ToggleFogOfWar]. NVForever's local map draws no fog of war (the Pip-Boy shows the
/// whole map, `nvf_play::pipboy`), so there is nothing to switch; the console says so.
pub fn toggle_fog_of_war(_g: &mut Game, _ctx: &ConsoleCtx, _a: &[String]) -> Result<Vec<String>, String> {
    Ok(vec!["The local map has no fog of war here: it always shows the whole map".into()])
}

/// `ShowLockPickMenu [difficulty]` (`slpm`): the lockpicking menu on a practice lock,
/// difficulty 1 Very Easy to 5 Very Hard, 3 Average without one [documented: GECK wiki
/// ShowLockPickMenu]. The console closes and the menu opens with the player's Lockpick
/// skill and bobby pins, which wear and break as on any lock; whatever the skill, the
/// lock may be tried, and opening it opens nothing, gives no XP and is no crime
/// [hypothesis: a designer's test of the minigame].
pub fn show_lock_pick_menu(g: &mut Game, _ctx: &ConsoleCtx, a: &[String]) -> Result<Vec<String>, String> {
    use crate::lockpick::Level;
    let d = match a.first() {
        Some(w) => w.parse::<u8>().ok().filter(|d| (1..=5).contains(d)).ok_or("difficulty 1 (Very Easy) to 5 (Very Hard)")?,
        None => 3,
    };
    let level = [Level::VeryEasy, Level::Easy, Level::Average, Level::Hard, Level::VeryHard][usize::from(d - 1)];
    g.console_requests.push(ConsoleRequest::Lockpick(level));
    Ok(vec![format!("Practice lock ({}): the lockpicking menu opens", level.name())])
}

/// The expressions of `MFG Expression <id>`, by ID, as the heads' `.tri` morph labels
/// [documented: GECK wiki ModifyFaceGen, Expression IDs; the wiki's `MoodCocky` is the
/// label `Cocky` of the heads, lips.rs `MOODS`].
pub const MFG_EXPRESSIONS: [&str; 15] =
    ["Anger", "Fear", "Happy", "Sad", "Surprise", "MoodNeutral", "MoodAfraid", "MoodAnnoyed", "Cocky", "MoodDrugged", "MoodPleasant", "MoodAngry", "MoodSad", "Pained", "CombatAnger"];
/// Phoneme IDs 0 to 15 and modifier IDs 0 to 16 [documented: GECK wiki ModifyFaceGen],
/// the `.lip` channels of the same numbers (fnv_voice::lip::CHANNEL_NAMES).
pub const MFG_PHONEMES: u8 = 16;
pub const MFG_MODIFIERS: u8 = 17;
/// Modifiers 14 to 16 turn the head (HeadPitch, HeadRoll, HeadYaw): they may be negative.
pub const MFG_HEAD_MODIFIERS: std::ops::RangeInclusive<u8> = 14..=16;

/// What `MFG` set on one actor's face, intensities 0 to 100 (head turns −100 to 100).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FaceMfg {
    /// The expression (an index of [`MFG_EXPRESSIONS`]) and its intensity: one at a time.
    pub expression: Option<(u8, f32)>,
    pub phonemes: std::collections::BTreeMap<u8, f32>,
    pub modifiers: std::collections::BTreeMap<u8, f32>,
}

/// `[ref.]MFG <Expression|Phoneme|Modifier> <id> <intensity>`, `MFG Reset`: morphs on the
/// actor's face (else the selection's) [documented: GECK wiki ModifyFaceGen, "An actor may
/// only have one Expression at a time"; reset "clears all MFG emotions"]. Intensity 0
/// takes one off. They stay until reset, on top of what the face does otherwise (lips,
/// blinks, a line's emotion). The original takes "a second or two" to show them
/// [documented]; ours show at once [design].
pub fn modify_face_gen(g: &mut Game, ctx: &ConsoleCtx, a: &[String]) -> Result<Vec<String>, String> {
    const USE: &str = "MFG Expression|Phoneme|Modifier <id> <intensity 0-100>, or MFG Reset";
    let r = ctx.on.ok_or("select an actor first, or ref.MFG")?;
    let who = label(g, r);
    let kind = a.first().map(|k| k.to_ascii_lowercase()).ok_or(USE)?;
    if kind == "reset" {
        g.mfg.remove(&r);
        return Ok(vec![format!("{who}: MFG reset")]);
    }
    let id: u8 = a.get(1).and_then(|w| w.parse().ok()).ok_or(USE)?;
    let asked: f32 = a.get(2).and_then(|w| w.parse().ok()).ok_or(USE)?;
    let head = kind == "modifier" && MFG_HEAD_MODIFIERS.contains(&id);
    let value = asked.clamp(if head { -100.0 } else { 0.0 }, 100.0);
    let face = g.mfg.entry(r).or_default();
    let (what, name) = match kind.as_str() {
        "expression" => {
            let name = *MFG_EXPRESSIONS.get(usize::from(id)).ok_or("expression IDs are 0 to 14")?;
            face.expression = (value > 0.0).then_some((id, value));
            ("expression", name.to_string())
        }
        "phoneme" if id < MFG_PHONEMES => {
            set_mfg(&mut face.phonemes, id, value);
            ("phoneme", id.to_string())
        }
        "modifier" if id < MFG_MODIFIERS => {
            set_mfg(&mut face.modifiers, id, value);
            ("modifier", id.to_string())
        }
        "phoneme" => return Err("phoneme IDs are 0 to 15".into()),
        "modifier" => return Err("modifier IDs are 0 to 16".into()),
        _ => return Err(USE.into()),
    };
    if *face == FaceMfg::default() {
        g.mfg.remove(&r);
    }
    Ok(vec![format!("{who}: MFG {what} {name} at {value:.0}")])
}

fn set_mfg(m: &mut std::collections::BTreeMap<u8, f32>, id: u8, v: f32) {
    if v == 0.0 {
        m.remove(&id);
    } else {
        m.insert(id, v);
    }
}

/// `KillAllActors [killer]` (`killall`): every actor of the loaded area dies (the cells
/// around the player outdoors, the player's cell indoors), the player never; an essential
/// one is knocked out instead; the killer given (a reference) takes the kill as its own
/// [documented: GECK wiki KillAllActors, the catalogue's summary and notes: "Kills every
/// actor in the loaded area", "Essential actors are put into their usual temporary
/// unconscious state rather than being killed"; hypothesis: the dead and the disabled are
/// left alone]. Each death is `Game::kill`'s, as KillActor's.
/// Shared loaded-area kill pass used by both console `killall` and source-compiled
/// `KillAllActors` / `KillAll` calls [documented: GECK wiki KillAllActors].
pub(crate) fn kill_all_actors_in_loaded(g: &mut Game, killer: Option<FormId>) -> (usize, usize) {
    let placed = g.world.placed;
    let is_actor = |r: &FormId| placed.get(*r).is_some_and(|p| matches!(&p.sig, b"ACHR" | b"ACRE"));
    let mut actors: std::collections::BTreeSet<FormId> = std::collections::BTreeSet::new();
    for c in &g.loaded_cells {
        actors.extend(placed.by_cell.get(c).into_iter().flatten().filter(|r| is_actor(r)));
    }
    actors.extend(g.moved.iter().filter(|r| is_actor(r)));
    actors.extend(g.created_actors());
    let loaded = g.loaded_cells.clone();
    actors.retain(|&r| r != FormId::PLAYER_REF && g.cell_of(r).is_some_and(|c| loaded.contains(&c)) && g.present(r) && !g.is_dead(r));
    let (mut killed, mut knocked) = (0, 0);
    for r in actors {
        if super::actor::essential(g, r) {
            if g.knock_out(r, crate::unconscious::Cause::Essential, killer) {
                knocked += 1;
            }
        } else if g.kill(r, killer) {
            killed += 1;
        }
    }
    (killed, knocked)
}

pub fn kill_all_actors(g: &mut Game, _ctx: &ConsoleCtx, a: &[String]) -> Result<Vec<String>, String> {
    let killer = a.first().map(|w| form(g, w)).transpose()?;
    let (killed, knocked) = kill_all_actors_in_loaded(g, killer);
    Ok(vec![format!("KillAllActors >> {killed} killed, {knocked} essential knocked out")])
}

/// `PlaceLocationMarker x y z [name] [color]` (`plm`): a small debug box at (x, y, z)
/// with its name above it, drawn through walls; without a name the coordinates are the
/// name; colour 0 is white, 1 to 9 others [documented: GECK wiki PlaceLocationMarker].
/// The engine draws it (nvf-play `console::LocationMarkers`); which colours 1-9 are is
/// ours [design].
pub fn place_location_marker(g: &mut Game, _ctx: &ConsoleCtx, a: &[String]) -> Result<Vec<String>, String> {
    const USE: &str = "PlaceLocationMarker <x> <y> <z> [name] [colour 0-9]";
    let num = |i: usize| a.get(i).and_then(|w| w.parse::<f32>().ok()).ok_or(USE);
    let pos = [num(0)?, num(1)?, num(2)?];
    let name = match a.get(3).map(|w| w.trim_matches('"')) {
        Some(n) if !n.is_empty() => n.to_string(),
        _ => format!("{:.0} {:.0} {:.0}", pos[0], pos[1], pos[2]),
    };
    let color = match a.get(4) {
        Some(w) => w.parse::<u8>().ok().filter(|c| *c <= 9).ok_or("the colour is 0 (white) to 9")?,
        None => 0,
    };
    g.console_requests.push(ConsoleRequest::PlaceMarker { pos, name: name.clone(), color });
    Ok(vec![format!("Location marker {name} placed")])
}

/// `ClearLocationMarkers` (`clm`): the markers placed in the current cell go
/// [documented: GECK wiki ClearLocationMarkers]. Outdoors, the current cell is the
/// worldspace: every marker placed outdoors in it goes [hypothesis].
pub fn clear_location_markers(g: &mut Game, _ctx: &ConsoleCtx, _a: &[String]) -> Result<Vec<String>, String> {
    g.console_requests.push(ConsoleRequest::ClearMarkers);
    Ok(Vec::new())
}

/// `Show1stPerson` (`s1st`): the first-person model shown from the third-person camera
/// as well, so in third person both are seen [documented: GECK wiki Show1stPerson];
/// again: hidden as before [hypothesis: a switch, as the other debug views].
pub fn show_1st_person(g: &mut Game, _ctx: &ConsoleCtx, _a: &[String]) -> Result<Vec<String>, String> {
    g.console_requests.push(ConsoleRequest::Show1stPerson);
    Ok(Vec::new())
}

/// `[ref.]SetVel <X|Y|Z> <speed>`: the reference's velocity along the axis
/// [documented: GECK wiki SetVel, "Sets Velocity on a reference", axis X, Y or Z]; in game
/// units per second along the world's axes, Z up [hypothesis: the units are not given].
/// The engine sets it on the player's movement, or on a loose object's physics body; a
/// reference with neither says so.
pub fn set_vel(g: &mut Game, ctx: &ConsoleCtx, a: &[String]) -> Result<Vec<String>, String> {
    const USE: &str = "[ref.]SetVel <X|Y|Z> <units per second>";
    let on = ctx.on.ok_or("select a reference first, or ref.SetVel")?;
    let axis = match a.first().map(|w| w.trim_matches('"').to_ascii_uppercase()) {
        Some(w) if matches!(w.as_str(), "X" | "Y" | "Z") => w.chars().next().unwrap_or('Z'),
        _ => return Err(USE.into()),
    };
    let speed = a.get(1).and_then(|w| w.parse::<f32>().ok()).ok_or(USE)?;
    g.console_requests.push(ConsoleRequest::SetVel { on, axis, speed });
    Ok(Vec::new())
}

/// `Show <variable>` (`tst`): a global's value, by Editor ID ("prints the value of a
/// special or global variable" [documented: the catalogue's summary]). The special ones
/// the catalogue speaks of (GameHour, GameDaysPassed, TimeScale, …) are globals of the
/// masters [observed: FalloutNV.esm GLOB records].
pub fn show_variable(g: &mut Game, _ctx: &ConsoleCtx, a: &[String]) -> Result<Vec<String>, String> {
    let name = a.first().map(|w| w.trim_matches('"')).ok_or("Show <global>")?;
    let (id, _) = g.world.forms.global_by_edid(name).ok_or_else(|| format!("No global {name}"))?;
    let v = g.world.global(id).map_err(|e| e.to_string())?;
    Ok(vec![format!("{name} = {}", shown(Value::Num(f64::from(v)), g))])
}

/// `PersistentRefCount` (`prc`): how many persistent references there are [documented:
/// the catalogue's summary]: placed references flagged persistent or stored in a cell's
/// persistent group, and the player, those removed for good left out; references made
/// while playing (PlaceAtMe) are not persistent [hypothesis].
pub fn persistent_ref_count(g: &mut Game, _ctx: &ConsoleCtx, _a: &[String]) -> Result<Vec<String>, String> {
    let placed = g.world.placed;
    let n = placed.refs.keys().filter(|&&r| g.persistent(r) && !g.extra.get(&r).is_some_and(|e| e.marked_for_delete)).count();
    Ok(vec![format!("Persistent references: {}", n + 1)])
}

/// `TestRaceSexTerimnal` (the catalogue's spelling): the race menu, as ShowRaceMenu
/// [documented: the catalogue's summary, "equivalent to ShowRaceMenu"].
pub fn test_race_sex_terminal(g: &mut Game, _ctx: &ConsoleCtx, _a: &[String]) -> Result<Vec<String>, String> {
    g.race_menu = crate::game::RaceMenu::Creator;
    g.race_menu_studio = false;
    g.open_menu(crate::game::menus::RACE);
    Ok(vec!["The race menu opens".into()])
}

/// `ClearScreenBlood`: the blood on the screen goes [documented: the catalogue's
/// summary], as ClearScreenSplatter: the engine draws both kinds of splat, the scripts'
/// and the player's hurts, in one layer (nvf-play `screen_splatter`).
pub fn clear_screen_blood(g: &mut Game, _ctx: &ConsoleCtx, _a: &[String]) -> Result<Vec<String>, String> {
    g.ui.splatter.triggers.clear();
    g.ui.splatter.clears += 1;
    Ok(vec!["Screen blood cleared".into()])
}

/// `CloseAllMenus` (`cam`): every open menu closes, the console too.
pub fn close_all_menus(g: &mut Game, _ctx: &ConsoleCtx, _a: &[String]) -> Result<Vec<String>, String> {
    g.console_requests.push(ConsoleRequest::CloseMenus);
    Ok(Vec::new())
}

/// `SaveGame <name>` (`save`): a manual save under that name.
pub fn save_game(g: &mut Game, _ctx: &ConsoleCtx, a: &[String]) -> Result<Vec<String>, String> {
    let name = a.first().map(|s| s.trim_matches('"').to_string()).ok_or("save <name>")?;
    g.console_requests.push(ConsoleRequest::Save(name));
    Ok(Vec::new())
}

/// `LoadGame [name]` (`load`): the named save, else the quick save.
pub fn load_game(g: &mut Game, _ctx: &ConsoleCtx, a: &[String]) -> Result<Vec<String>, String> {
    g.console_requests.push(ConsoleRequest::Load(a.first().map(|s| s.trim_matches('"').to_string()).unwrap_or_default()));
    Ok(Vec::new())
}

// ---- `set <variable> to <value>` -----------------------------------------------------------

/// The console's `set <variable> to <value>` statement:
///
/// - `set quest.var to 5` / `set ref.var to 5`: a quest's or a reference's script variable;
/// - `set Global to 5`: a global by Editor ID;
/// - `set var to 5`: the selected reference's script variable.
///
/// The value is a number or a form (`player`, an Editor ID, a FormID). It is stored as a
/// script's `set` stores it (an integer variable truncates).
pub fn set_statement(g: &mut Game, ctx: &ConsoleCtx, words: &[String]) -> Result<Vec<String>, String> {
    let usage = "set <variable> to <value>";
    let (target, value) = match words {
        [t, to, v] if to.eq_ignore_ascii_case("to") => (t.as_str(), v.as_str()),
        _ => return Err(usage.into()),
    };
    let v = match value.parse::<f64>() {
        Ok(n) => Value::Num(n),
        Err(_) => Value::Ref(form(g, value)?.0),
    };
    let (owner, var) = match target.rsplit_once('.') {
        Some((o, v)) if !o.is_empty() && !v.is_empty() => (Some(form(g, o)?), v),
        _ => (None, target),
    };
    if owner.is_none() {
        if let Some((id, _)) = g.world.forms.global_by_edid(var) {
            // As a script's write: a jump of GameHour is noted for the engine (#2353).
            g.set_global_by_script(id, v.num() as f32).map_err(|e| e.to_string())?;
            return Ok(vec![format!("{var} = {}", shown(Value::Num(g.world.global(id).map_err(|e| e.to_string())? as f64), g))]);
        }
    }
    let owner = owner.or(ctx.on).ok_or_else(|| format!("No global {var}, and no reference selected"))?;
    let (key, script) = g.instance_of(owner).ok_or_else(|| format!("{} runs no script", label(g, owner)))?;
    let index = g.data.script(script).and_then(|c| c.var_index(var)).ok_or_else(|| format!("{} has no variable {var}", g.data.name(script)))?;
    g.store(key, Some(script), index, v, "console", false);
    Ok(vec![format!("{}.{var} = {}", label(g, owner), shown(g.var(key, index), g))])
}

/// What [`call_words`] gives back.
#[derive(Debug, Clone, PartialEq)]
pub struct Called {
    pub opcode: u16,
    /// The catalogue's spelling of the function.
    pub name: String,
    pub value: Value,
    /// The function is a stub (a system not built): why. Its value is then 0.
    pub stub: Option<&'static str>,
}

/// A FormID as the console prints it: eight hex digits, or hex after `0x`.
pub fn hex_form_id(s: &str) -> Option<FormId> {
    let h = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    (!h.is_empty() && h.len() <= 8 && h.chars().all(|c| c.is_ascii_hexdigit())).then(|| u32::from_str_radix(h, 16).ok().map(FormId)).flatten()
}

/// An enumeration code by name or number (`actor_value` by the game's own names too).
pub fn enum_code(g: &Game, e: &str, a: &str) -> Result<u16, String> {
    if let Ok(n) = a.parse::<u16>() {
        return Ok(n);
    }
    if e == "actor_value" {
        return crate::functions::av_code(g, a);
    }
    let squash = |s: &str| s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
    let want = squash(a);
    g.data.table.enums.get(e).and_then(|m| m.iter().find(|(_, n)| squash(n) == want).map(|(&c, _)| c)).ok_or_else(|| format!("no {e} named {a:?}"))
}

/// Call a script function by name (a catalogue name or alias) on `on` with console words
/// as its arguments, as the console runs a line: each word is read by the parameter's
/// encoding (an enumeration name or code, an axis letter, a string), else as a FormID
/// (eight hex digits), a number where the parameter is numeric, or a form by name
/// (`player`, an Editor ID, a FormID). `unit` names the caller in the trace.
///
/// Errors: an unknown function, a word that is no form or number, and the call's own
/// failure (`CallFailed`). Console-only commands that print are not run here
/// ([`lookup`]). Used by the engine's console and by the headless runner's `call`
/// action (docs/design/script-function-coverage.md, the probes).
pub fn call_words(g: &mut Game, on: Option<FormId>, func: &str, args: &[String], unit: &str) -> Result<Called, String> {
    use fnv_script::vm::ArgValue;
    use fnv_script::ParamEnc;
    let opcode = g.data.table.by_name(func).ok_or_else(|| format!("Script command \"{func}\" not found."))?;
    let def = g.data.table.get(opcode).cloned();
    let fname = def.as_ref().map_or(func.to_string(), |d| d.name.clone());
    let mut values = Vec::new();
    for (i, a) in args.iter().enumerate() {
        let kind = def.as_ref().and_then(|d| d.params.get(i)).map(|p| p.kind.to_ascii_lowercase()).unwrap_or_default();
        let v = match g.data.table.param_enc(opcode, i) {
            ParamEnc::Enum(e) => ArgValue::Enum(enum_code(g, &e, a.trim_matches('"'))?),
            ParamEnc::Axis => ArgValue::Axis(a.chars().next().unwrap_or('Z').to_ascii_uppercase()),
            ParamEnc::String => ArgValue::Str(a.trim_matches('"').to_string()),
            ParamEnc::Value => {
                let numeric = matches!(kind.as_str(), "int" | "integer" | "float" | "number" | "short" | "long");
                // Eight hex digits are a FormID as the console prints them (`00015169`), never
                // a number, whatever the parameter.
                let full_id = a.len() == 8 && a.chars().all(|c| c.is_ascii_hexdigit());
                if a.starts_with('"') {
                    ArgValue::Str(a.trim_matches('"').to_string())
                } else if let (true, Some(f)) = (full_id, hex_form_id(a)) {
                    ArgValue::Value(Value::Ref(f.0))
                } else if let (true, Ok(n)) = (numeric || kind.is_empty(), a.parse::<f64>()) {
                    ArgValue::Value(Value::Num(n))
                } else {
                    match crate::runner::resolve(g, a).ok().or_else(|| hex_form_id(a)) {
                        Some(f) => ArgValue::Value(Value::Ref(f.0)),
                        None => match a.parse::<f64>() {
                            Ok(n) => ArgValue::Value(Value::Num(n)),
                            Err(_) => return Err(format!("{fname}: no form or number {a:?}")),
                        },
                    }
                }
            }
        };
        values.push(v);
    }
    let ctx = crate::functions::Ctx { unit: unit.into(), opcode, on, self_ref: on, action_ref: None, quest: None, inst: None, in_condition: false };
    let from = g.events.len();
    let value = g.call_function(&ctx, &values);
    if let Some(crate::trace::EventKind::CallFailed { error, .. }) =
        g.events[from..].iter().map(|e| &e.kind).find(|k| matches!(k, crate::trace::EventKind::CallFailed { .. }))
    {
        return Err(format!("{fname}: {error}"));
    }
    let stub = match crate::functions::lookup(opcode).or_else(|| crate::functions::lookup_console(&fname)) {
        Some(crate::functions::Kind::Stub(why)) => Some(*why),
        _ => None,
    };
    Ok(Called { opcode, name: fname, value, stub })
}

/// Split a console line into words; `"…"` keeps spaces (and its quotes), commas separate
/// as spaces do.
pub fn words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for ch in line.chars() {
        match ch {
            '"' => {
                if quoted {
                    out.push(format!("\"{cur}\""));
                    cur.clear();
                }
                quoted = !quoted;
            }
            c if quoted => cur.push(c),
            c if c.is_whitespace() || c == ',' => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(if quoted { format!("\"{cur}\"") } else { cur });
    }
    out
}

/// `reference.Function arg …` (or `Function arg …`): the reference word, the function
/// word and the argument words. None for an empty line.
pub fn split_call(line: &str) -> Option<(Option<String>, String, Vec<String>)> {
    let w = words(line);
    let first = w.first()?;
    let (on, func) = match first.rsplit_once('.') {
        Some((r, f)) if !r.is_empty() && !f.is_empty() => (Some(r.to_string()), f.to_string()),
        _ => (None, first.clone()),
    };
    Some((on, func, w[1..].to_vec()))
}

/// An expression operator or parenthesis outside quoted command arguments. A word
/// reader can reject these lines even though the statement compiler accepts them.
pub fn has_expression_operator(line: &str) -> bool {
    let mut quoted = false;
    line.chars().any(|c| {
        if c == '"' { quoted = !quoted; }
        !quoted && "+-*/%()<>=&|".contains(c)
    })
}

/// `ref.SetDefaultOpen defaultOpen`: set the calling reference's default open state
/// [documented: GECK wiki SetDefaultOpen].
pub fn set_default_open(g: &mut Game, ctx: &ConsoleCtx, a: &[String]) -> Result<Vec<String>, String> {
    let r = ctx.on.ok_or("SetDefaultOpen requires a calling reference")?;
    let [flag] = a else { return Err("SetDefaultOpen expects 1 argument (0 or 1)".into()) };
    let v: i32 = flag.parse().map_err(|_| format!("SetDefaultOpen: {flag:?} is not an integer"))?;
    let open = v != 0;
    g.world.set_default_open(r, open).map_err(|e| e.to_string())?;
    Ok(vec![format!("SetDefaultOpen >> {r} default_open = {}", usize::from(open))])
}

/// Decide before calling a host function. Numeric words are literals, and a
/// string parameter retains the word reader's path/name syntax even when unquoted.
pub fn call_has_expression(g: &Game, line: &str, func: &str, args: &[String]) -> bool {
    let opcode = g.data.table.by_name(func);
    // Compact assignments can contain dots in both the variable and numeric RHS;
    // split_call's reference/function split is not their expression grammar.
    if has_expression_operator(func) || (opcode.is_none() && words(line).first().is_some_and(|word| has_expression_operator(word))) {
        return true;
    }
    args.iter().enumerate().any(|(i, word)| {
        if word.starts_with('"') || word.parse::<f64>().is_ok() {
            return false;
        }
        if opcode.is_some_and(|op| matches!(g.data.table.param_enc(op, i), fnv_script::ParamEnc::String)) {
            return false;
        }
        if crate::runner::resolve(g, word).is_ok() {
            return false;
        }
        has_expression_operator(word)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calls_split_into_reference_function_and_words() {
        assert_eq!(split_call("0x0010A1C5.GetDisposition player"), Some((Some("0x0010A1C5".into()), "GetDisposition".into(), vec!["player".into()])));
        assert_eq!(split_call("GetPCMiscStat \"Locations Discovered\""), Some((None, "GetPCMiscStat".into(), vec!["\"Locations Discovered\"".into()])));
        assert_eq!(split_call("  "), None);
        assert_eq!(hex_form_id("0x00104C0C"), Some(FormId(0x0010_4C0C)));
        assert_eq!(hex_form_id("0000000F"), Some(FormId(0xF)));
        assert_eq!(hex_form_id("player"), None);
    }

    #[test]
    fn expression_fallback_ignores_operators_in_quoted_command_arguments() {
        assert!(has_expression_operator("OwnQuest.value = 2"));
        assert!(has_expression_operator("player.SetAV Luck (5)"));
        assert!(!has_expression_operator("Print \"x+=1\""));
        assert!(!has_expression_operator("ShowMessage \"a-b\""));
        assert!(!has_expression_operator("player.additem f 100"));
    }

    /// Every registered name is a console-only command of the catalogue (one numbered
    /// from `CONSOLE_OPCODE_BASE`, without a catalogue opcode), spelled as the catalogue
    /// spells it.
    #[test]
    fn registered_names_are_catalogue_console_commands() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/script/functions.json");
        let t = fnv_script::FunctionTable::load(std::path::Path::new(path)).expect("functions.json");
        for (name, _) in registry() {
            // A vanilla function without an opcode outside the console category.
            if t.by_name(name).is_none() {
                let u = t.unnumbered.get(&name.to_ascii_lowercase()).unwrap_or_else(|| panic!("{name} is not in the catalogue"));
                assert_eq!(&u.name, name);
                continue;
            }
            let op = t.by_name(name).unwrap_or_else(|| panic!("{name} is not in the catalogue"));
            assert!(op >= fnv_script::table::CONSOLE_OPCODE_BASE, "{name} has a catalogue opcode: register it in functions::registry");
            assert_eq!(&t.get(op).unwrap().name, name);
        }
        // The functions nothing handles (docs/design/console.md): a number to keep
        // going down. Implementing one lowers it here; a catalogue update that adds one
        // raises it.
        let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let (console, other) = unhandled_unused(&j);
        eprintln!("unhandled console functions: {console:?}
unhandled other vanilla functions: {other:?}");
        assert_eq!((console.len(), other.len()), (33, 76), "docs/design/console.md, What the console does not handle, sorts them");
        // `killall` reaches KillAllActors by its alias.
        assert_eq!(lookup_word(&t, "killall").map(|r| r.0).as_deref(), Some("KillAllActors"));
        assert_eq!(lookup_word(&t, "cam").map(|r| r.0).as_deref(), Some("CloseAllMenus"));
        // Aliases find them too.
        for alias in ["sqt", "MoveToQT", "sqs", "sqv", "sv", "caqs", "inv", "getavinfo", "prid", "setgs", "qqq", "cam", "plm", "clm", "s1st", "tst", "prc"] {
            let f = t.by_name(alias).and_then(|op| t.get(op)).unwrap_or_else(|| panic!("{alias}"));
            assert!(lookup(&f.name).is_some(), "{alias}");
        }
    }

    #[test]
    fn named_area_source_queries_are_handled_but_unknown_vanilla_names_are_not() {
        let rows = ["GetVATSLeftAreaFree", "getvatsrightareafree", "OwnedUnknownVanillaQuery"]
            .map(|name| serde_json::json!({"name": name, "family": "vanilla", "script_calls": 0}));
        let (console, other) = unhandled_unused(&serde_json::json!({"functions": rows}));
        assert!(console.is_empty());
        assert_eq!(other, ["OwnedUnknownVanillaQuery"]);
    }
}
