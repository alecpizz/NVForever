//! Accessible live terminal input; docs/design/android-terminal-wheel.md.
use super::*;
use bevy::input::touch::{TouchInput, TouchPhase};
use bevy::window::{AppLifecycle, WindowFocused};
use serde::{Deserialize, Serialize};
use std::io::Read;
const SETTINGS_LIMIT: usize = 4096;
fn may_write() -> bool {
    true
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scheme {
    #[default]
    Direct,
    Cursor,
}
#[derive(Resource, Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
    pub separate_confirm: bool,
    pub scheme: Scheme,
    pub zoom: u8,

    #[serde(skip, default = "may_write")]
    allow_write: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: cfg!(target_os = "android"),
            separate_confirm: true,
            scheme: Scheme::Direct,
            zoom: 0,

            allow_write: true,
        }
    }
}
impl Settings {
    fn normalize(&mut self) {
        self.zoom = self.zoom.min(2);
    }
    fn decode(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > SETTINGS_LIMIT {
            return Err("Touch settings exceed the v1 size limit".into());
        }
        let document: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        if document["version"].as_u64() != Some(1) {
            return Err("Unsupported touch settings version; file preserved".into());
        }
        if document
            .as_object()
            .is_none_or(|object| object.len() != 2 || !object.contains_key("settings"))
        {
            return Err("Unrecognized v1 settings fields; file preserved".into());
        }
        let mut value: Self =
            serde_json::from_value(document["settings"].clone()).map_err(|e| e.to_string())?;
        value.normalize();
        value.allow_write = true;
        Ok(value)
    }
    fn import(&mut self, bytes: &[u8]) -> Result<(), String> {
        match Self::decode(bytes) {
            Ok(value) => {
                *self = value;
                Ok(())
            }
            Err(error) => {
                self.allow_write = false;
                Err(error)
            }
        }
    }
    fn save_to(&self, path: &std::path::Path) -> Result<(), String> {
        if !self.allow_write {
            return Err("Unrecognized settings file is protected from rewriting".into());
        }
        let bytes = serde_json::to_vec_pretty(&serde_json::json!({"version":1,"settings":self}))
            .map_err(|e| e.to_string())?;
        if bytes.len() > SETTINGS_LIMIT {
            return Err("Touch settings exceed the v1 size limit".into());
        }
        let parent = path.parent().ok_or("Settings directory unavailable")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let temporary = path.with_file_name("terminal-touch.json.tmp");
        std::fs::write(&temporary, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(temporary, path).map_err(|e| e.to_string())
    }
    fn save(&self) -> Result<(), String> {
        let home = fnv_install::EngineHome::default_root().ok_or("Engine home unavailable")?;
        self.save_to(&home.join("terminal-touch.json"))
    }
}
pub(super) fn load(mut settings: ResMut<Settings>) {
    if !cfg!(target_os = "android") {
        return;
    }
    let Some(home) = fnv_install::EngineHome::default_root() else {
        return;
    };
    match std::fs::File::open(home.join("terminal-touch.json")) {
        Ok(file) => {
            let mut bytes = Vec::new();
            let result = file
                .take((SETTINGS_LIMIT + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())
                .and_then(|_| settings.import(&bytes));
            if let Err(error) = result {
                settings.allow_write = false;
                warn!("terminal touch settings: {error}; existing file will not be rewritten");
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            settings.allow_write = false;
            warn!("terminal touch settings: {error}; existing file will not be rewritten");
        }
    }
}
/// The HUD bridge uses this predicate to release/suppress its generic menu contacts.
pub fn captures(settings: &Settings, screen: &TerminalScreen) -> bool {
    settings.enabled && screen.0.is_some()
}

/// Register the saved presentation switch for full or independently hosted menus.
pub fn register(app: &mut App) {
    crate::touch_ui_options::install(app);
    app.init_resource::<Settings>();
    crate::settings::add_setting(app, crate::settings::Setting {
        key: "touch-terminal-v1", group: crate::pause::GROUP_MENUS,
        label: "Enhanced hacking and terminal", switches: "",
        note: "Large terminal targets, word selection and separate Guess. Off keeps standard menu touch arrows, Confirm, Back and Exit, plus mouse/controller input. Text and scheme preferences are kept.",
        get: |w| w.resource::<Settings>().enabled, set: set_enabled, slider: None,
    });
}
fn set_enabled(world: &mut World, enabled: bool) {
    if world.resource::<Settings>().enabled == enabled { return; }
    world.resource_mut::<Settings>().enabled = enabled;
    let (window, ids) = if let Some(mut state) = world.get_resource_mut::<TouchState>() {
        let owned = (state.viewport.map(|v| v.0), state.contacts.iter().copied().take(33).collect::<Vec<_>>());
        state.gesture = None;
        state.contacts.clear();
        state.close_options();
        owned
    } else { (None, Vec::new()) };
    crate::touch_ui_options::retire(world, crate::touch_ui_options::Feature::Terminal, window, &ids);
}
#[derive(Clone, Copy, Debug, PartialEq)]
enum Hit {
    Cell(usize),
    Action(usize),
    Column,
    Pan,
    Other,
}
struct Gesture {
    id: u64,
    origin: Vec2,
    hit: Hit,
    row: usize,
    pan: f32,
    dragged: bool,
}
type SessionKey = (FormId, (u8, u32, Option<FormId>));
#[derive(Resource, Default)]
pub(super) struct TouchState {
    session: Option<SessionKey>,
    gesture: Option<Gesture>,
    contacts: std::collections::HashSet<u64>,
    suspended: bool,
    focused: Option<bool>,
    viewport: Option<(Entity, Vec2)>,
    first_row: usize,
    column: usize,
    pan_x: f32,
    last_cursor: Option<usize>,
    options: bool,
    block_commits: bool,
    message: String,
}
impl TouchState {
    pub(super) fn configuring(&self) -> bool {
        self.options || self.block_commits
    }
    pub(super) fn close_options(&mut self) {
        self.options = false;
        self.block_commits = true;
    }
}
#[derive(Component)]
pub(super) struct TouchRoot;
struct Geometry {
    size: Vec2,
    row: f32,
    visible: usize,
    cell: f32,
    columns: usize,
    column: usize,
    action_columns: usize,
    toolbar: f32,
    grid_top: f32,
    pan: f32,
}
impl Geometry {
    fn new(size: Vec2, zoom: u8) -> Self {
        let row = 48.0 + f32::from(zoom.min(2)) * 8.0;
        let cell = 24.0 + f32::from(zoom.min(2)) * 4.0;
        let columns = if size.x >= 2.0 * ROW_CHARS as f32 * cell + 56.0 {
            2
        } else {
            1
        };
        let action_columns = (((size.x - 16.0) / 52.0).floor() as usize).clamp(1, 7);
        let toolbar = 7usize.div_ceil(action_columns) as f32 * 52.0 + 8.0;
        let grid_top = if size.y - toolbar - row >= 112.0 {
            104.0
        } else {
            56.0
        };
        Self {
            size,
            row,
            cell,
            columns,
            column: 0,
            action_columns,
            toolbar,
            grid_top,
            pan: 0.0,
            visible: ((size.y - toolbar - grid_top - 4.0).max(row) / row).floor() as usize,
        }
    }
    fn for_column(mut self, column: usize) -> Self {
        self.column = column.min(1);
        self
    }
    fn max_pan(&self) -> f32 {
        (ROW_CHARS as f32 * self.cell - (self.size.x - 32.0)).max(0.0)
    }
    fn for_pan(mut self, pan: f32) -> Self {
        self.pan = pan.clamp(0.0, self.max_pan());
        self
    }
    fn column_rect(&self) -> Option<Rect> {
        (self.columns == 1).then_some(Rect::from_corners(
            Vec2::new(self.size.x - 120.0, 4.0),
            Vec2::new(self.size.x - 8.0, 52.0),
        ))
    }
    fn cell_rect(&self, index: usize, first: usize) -> Option<Rect> {
        let column = index / (ROWS * ROW_CHARS);
        let row = (index / ROW_CHARS) % ROWS;
        if row < first
            || row >= first + self.visible
            || (self.columns == 1 && column != self.column)
        {
            return None;
        }
        let display_column = if self.columns == 1 { 0 } else { column };
        let grid_width =
            self.columns as f32 * ROW_CHARS as f32 * self.cell + (self.columns - 1) as f32 * 24.0;
        let x = ((self.size.x - grid_width) / 2.0).max(16.0) - self.pan
            + display_column as f32 * (ROW_CHARS as f32 * self.cell + 24.0)
            + (index % ROW_CHARS) as f32 * self.cell;
        let y = self.grid_top + (row - first) as f32 * self.row;
        Some(Rect::from_corners(
            Vec2::new(x, y),
            Vec2::new(x + self.cell, y + self.row),
        ))
    }
    fn action(&self, index: usize) -> Rect {
        let width = (self.size.x - 16.0) / self.action_columns as f32;
        let row = index / self.action_columns;
        let column = index % self.action_columns;
        Rect::from_center_size(
            Vec2::new(
                8.0 + width * (column as f32 + 0.5),
                self.size.y - self.toolbar + 28.0 + row as f32 * 52.0,
            ),
            Vec2::new((width - 4.0).max(48.0), 48.0),
        )
    }
    fn hit(&self, p: Vec2, first: usize, options: bool) -> Hit {
        if let Some(i) = (0..7).find(|&i| self.action(i).contains(p)) {
            return Hit::Action(i);
        }
        if !options {
            if self.column_rect().is_some_and(|r| r.contains(p)) {
                return Hit::Column;
            }
            if !Rect::new(
                16.0,
                self.grid_top,
                self.size.x - 16.0,
                self.size.y - self.toolbar - 4.0,
            )
            .contains(p)
            {
                return Hit::Other;
            }
            if let Some(i) = (0..2 * ROWS * ROW_CHARS)
                .find(|&i| self.cell_rect(i, first).is_some_and(|r| r.contains(p)))
            {
                return Hit::Cell(i);
            }
        }
        Hit::Other
    }
    /// Each usable word/bracket segment has a visible target of at least48x48.
    /// A partial word at a scroll boundary is padded inside its own column.
    fn tokens(&self, puzzle: &Puzzle, first: usize) -> Vec<(usize, Rect)> {
        let mut result = Vec::new();
        for index in 0..puzzle.chars.len() {
            let target = puzzle.target(index);
            if !matches!(target, Target::Word(_) | Target::Brackets { .. }) {
                continue;
            }
            let span = puzzle.span(target);
            let row_start = (index / ROW_CHARS) * ROW_CHARS;
            let segment_start = span.start.max(row_start);
            if index != segment_start {
                continue;
            }
            let segment_end = span.end.min(row_start + ROW_CHARS);
            let (Some(left), Some(right)) = (
                self.cell_rect(segment_start, first),
                self.cell_rect(segment_end - 1, first),
            ) else {
                continue;
            };
            let row_left = self.cell_rect(row_start, first).unwrap().min.x.max(16.0);
            let row_right = (self.cell_rect(row_start, first).unwrap().min.x
                + ROW_CHARS as f32 * self.cell)
                .min(self.size.x - 16.0);
            let shown_left = left.min.x.max(row_left);
            let shown_right = right.max.x.min(row_right);
            if shown_right <= shown_left {
                continue;
            }
            let width = (shown_right - shown_left)
                .max(48.0)
                .min(row_right - row_left);
            let center = ((shown_left + shown_right) / 2.0)
                .clamp(row_left + width / 2.0, row_right - width / 2.0);
            result.push((
                span.start,
                Rect::from_center_size(
                    Vec2::new(center, left.center().y),
                    Vec2::new(width, self.row),
                ),
            ));
        }
        result
    }
    fn token_hit(&self, puzzle: &Puzzle, p: Vec2, first: usize, options: bool) -> Hit {
        let raw = self.hit(p, first, options);
        if !matches!(raw, Hit::Cell(_)) {
            return raw;
        }
        if let Hit::Cell(index) = raw {
            let target = puzzle.target(index);
            if matches!(target, Target::Word(_) | Target::Brackets { .. }) {
                return Hit::Cell(puzzle.span(target).start);
            }
        }
        self.tokens(puzzle, first)
            .into_iter()
            .filter(|(_, r)| r.contains(p))
            .min_by(|(_, a), (_, b)| {
                a.center()
                    .distance_squared(p)
                    .total_cmp(&b.center().distance_squared(p))
            })
            .map_or(Hit::Pan, |(index, _)| Hit::Cell(index))
    }
}
fn page_key(page: &Page) -> (u8, u32, Option<FormId>) {
    match page {
        Page::Hacking(_) => (0, 0, None),
        Page::Menu(menu) => (1, menu.terminal.0, menu.note),
        Page::LockedOut(_) => (2, 0, None),
    }
}
#[allow(clippy::too_many_arguments)]
pub(super) fn input(
    screen: Res<TerminalScreen>,
    mut settings: ResMut<Settings>,
    mut state: ResMut<TouchState>,
    mut touches: MessageReader<TouchInput>,
    mut focus: MessageReader<WindowFocused>,
    mut lifecycle: MessageReader<AppLifecycle>,
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
    flat: Query<(&ComputedNode, &UiGlobalTransform), With<screen::FlatScreen>>,
    rows: Res<MenuRows>,
    bindings: Option<Res<crate::bindings::Bindings>>,
    touch_switch: Option<Res<crate::pad::TouchDeviceSwitch>>,
    platform: Option<Res<crate::pad::PlatformInputGate>>,
    ui_capture: Option<Res<crate::ui_pointer::TouchUiCapture>>,
    retirements: Option<Res<crate::touch_ui_options::Retirements>>,
    mut out: MessageWriter<TerminalInput>,
    script: Option<Res<ScriptState>>,
) {
    settings.normalize();
    state.block_commits = state.options;
    let mut interrupted = false;
    for event in lifecycle.read() {
        match event {
            AppLifecycle::Suspended => {
                state.suspended = true;
                interrupted = true;
            }
            AppLifecycle::Running => {
                state.suspended = false;
                interrupted = true;
            }
            _ => {}
        }
    }
    for event in focus.read() {
        if windows.iter().any(|(id, _)| id == event.window) {
            state.focused = Some(event.focused);
            interrupted = true;
        }
    }
    let session = screen.0.as_ref().map(|s| (s.reference, page_key(&s.page)));
    if state.session != session || interrupted {
        state.session = session;
        state.gesture = None;
        state.contacts.clear();
        state.first_row = 0;
        state.column = 0;
        state.pan_x = 0.0;
        state.last_cursor = None;
        state.options = false;
        touches.clear();
        return;
    }
    let Ok((window_id, window)) = windows.single() else {
        state.viewport = None;
        state.gesture = None;
        state.contacts.clear();
        touches.clear();
        return;
    };
    let viewport = (window_id, Vec2::new(window.width(), window.height()));
    if state.viewport != Some(viewport) {
        state.viewport = Some(viewport);
        state.gesture = None;
        state.contacts.clear();
        touches.clear();
        return;
    }
    if retirements.as_ref().is_some_and(|r| r.barrier(crate::touch_ui_options::Feature::Terminal)) {
        state.gesture = None;
        state.contacts.clear();
        state.options = false;
        // This reader owns no keyboard/controller state. Root handles persistent
        // per-window ID masking; this closes the post-publisher callback interval.
        touches.read().for_each(drop);
        return;
    }
    let device_allows_actions = touch_switch.as_deref().is_none_or(crate::pad::TouchDeviceSwitch::allows_actions);
    let controller = bindings.is_some_and(|b| b.device == crate::bindings::InputDevice::Controller);
    if controller && state.options { state.close_options(); }
    if !settings.enabled
        || state.suspended
        || !window.focused
        || state.focused == Some(false)
        || controller
        || !device_allows_actions
        || platform.as_ref().is_some_and(|gate| !gate.allows_actions())
    {
        state.gesture = None;
        state.contacts.clear();
        touches.clear();
        return;
    }
    let Some(session) = screen.0.as_ref() else {
        state.gesture = None;
        touches.clear();
        return;
    };
    let expected_menu = if matches!(session.page, Page::Menu(_)) {
        MENU_TERMINAL
    } else {
        MENU_HACKING
    };
    if script
        .as_ref()
        .and_then(|s| s.rt.as_ref())
        .is_some_and(|rt| rt.game.menu != Some(expected_menu))
    {
        state.gesture = None;
        state.contacts.clear();
        touches.clear();
        return;
    }
    if let Page::Hacking(h) = &session.page {
        let cursor = (!h.hidden).then_some(h.cursor);
        if cursor != state.last_cursor {
            if let Some(cursor) = cursor {
                state.column = cursor / (ROWS * ROW_CHARS);
            }
            state.last_cursor = cursor;
        }
    }
    let geometry = Geometry::new(Vec2::new(window.width(), window.height()), settings.zoom)
        .for_column(state.column)
        .for_pan(state.pan_x);
    state.first_row = state.first_row.min(ROWS.saturating_sub(geometry.visible));
    let mut drop_tail = false;
    for touch in touches.read().filter(|e| e.window == window_id
        && ui_capture.as_ref().is_none_or(|capture| capture.allows(e.id))
        && platform.as_ref().is_none_or(|gate| gate.allows_touch(e.id))) {
        if !touch.position.is_finite() {
            state.gesture = None;
            state.contacts.remove(&touch.id);
            continue;
        }
        match touch.phase {
            TouchPhase::Started => {
                if !state.contacts.insert(touch.id) || state.contacts.len() != 1 {
                    state.gesture = None;
                    continue;
                }
                let hit = if let Page::Hacking(h) = &session.page {
                    geometry.token_hit(&h.puzzle, touch.position, state.first_row, state.options)
                } else {
                    geometry.hit(touch.position, state.first_row, state.options)
                };
                state.gesture = Some(Gesture {
                    id: touch.id,
                    origin: touch.position,
                    hit,
                    row: state.first_row,
                    pan: state.pan_x,
                    dragged: false,
                });
            }
            TouchPhase::Moved => {
                if let Some(g) = state.gesture.as_mut().filter(|g| g.id == touch.id) {
                    let delta = touch.position - g.origin;
                    if delta.length() > 10.0 {
                        g.dragged = true;
                    }
                    let pan = (matches!(g.hit, Hit::Cell(_) | Hit::Pan) && g.dragged)
                        .then_some((g.row as f32 - delta.y / geometry.row).round());
                    let horizontal =
                        matches!(g.hit, Hit::Cell(_) | Hit::Pan).then_some(g.pan - delta.x);
                    if let Some(horizontal) = horizontal {
                        state.pan_x = horizontal.clamp(0.0, geometry.max_pan());
                    }
                    if let Some(row) = pan {
                        state.first_row =
                            row.clamp(0.0, ROWS.saturating_sub(geometry.visible) as f32) as usize;
                    }
                }
            }
            TouchPhase::Canceled => {
                // A pan changed the displayed viewport; queued gestures still refer
                // to the previous layout and must wait for its next rendered frame.
                drop_tail = state.gesture.as_ref().is_some_and(|g| g.dragged);
                state.contacts.remove(&touch.id);
                state.gesture = None;
            }
            TouchPhase::Ended => {
                state.contacts.remove(&touch.id);
                let Some(g) = state.gesture.take().filter(|g| g.id == touch.id) else {
                    continue;
                };
                if g.dragged || touch.position.distance(g.origin) > 10.0 {
                    drop_tail = true;
                    break;
                }
                if let Page::Hacking(h) = &session.page {
                    if h.ended.is_some() {
                        continue;
                    }
                    let hit = geometry.token_hit(
                        &h.puzzle,
                        touch.position,
                        state.first_row,
                        state.options,
                    );
                    if hit != g.hit {
                        continue;
                    }
                    match hit {
                        Hit::Column => {
                            state.column = 1 - state.column;
                            state.pan_x = 0.0;
                            drop_tail = true;
                        }
                        Hit::Cell(index) if settings.scheme == Scheme::Direct => {
                            let at = h.puzzle.span(h.puzzle.target(index)).start;
                            out.write(TerminalInput::Point(at));
                            if !settings.separate_confirm {
                                out.write(TerminalInput::Select);
                            }
                        }
                        Hit::Action(i) => {
                            if state.options {
                                state.block_commits = true;
                                drop_tail = true;
                                match i {
                                    0 => {
                                        settings.scheme = if settings.scheme == Scheme::Direct {
                                            Scheme::Cursor
                                        } else {
                                            Scheme::Direct
                                        }
                                    }
                                    1 => settings.separate_confirm = !settings.separate_confirm,
                                    2 => settings.zoom = settings.zoom.saturating_sub(1),
                                    3 => settings.zoom = (settings.zoom + 1).min(2),
                                    _ => state.options = false,
                                }
                                state.message = match settings.save() {
                                    Ok(()) => "Options saved".into(),
                                    Err(e) => format!("Options not saved: {e}"),
                                };
                            } else {
                                let command = match i {
                                    0 => Some(TerminalInput::Step(-1, 0)),
                                    1 => Some(TerminalInput::Step(0, -1)),
                                    2 => Some(TerminalInput::Step(0, 1)),
                                    3 => Some(TerminalInput::Step(1, 0)),
                                    4 => Some(TerminalInput::Select),
                                    5 => {
                                        state.options = true;
                                        state.block_commits = true;
                                        drop_tail = true;
                                        None
                                    }
                                    _ => Some(TerminalInput::Exit),
                                };
                                if let Some(command) = command {
                                    out.write(command);
                                }
                            }
                        }
                        _ => {}
                    }
                } else {
                    if g.hit == Hit::Action(5) && geometry.action(5).contains(touch.position) {
                        out.write(TerminalInput::Back);
                        continue;
                    }
                    if g.hit == Hit::Action(6) && geometry.action(6).contains(touch.position) {
                        out.write(TerminalInput::Exit);
                        continue;
                    }
                    // Flat menu canvas: direct row selection is distinct from opening,
                    // scrolling and the terminal monitor's stale mouse hover position.
                    let point = flat.iter().next().and_then(|(node, at)| {
                        let size = node.size();
                        if size.min_element() <= 0.0 {
                            return None;
                        }
                        let p = (touch.position * window.scale_factor()
                            - (at.translation - size / 2.0))
                            / size
                            * screen::CANVAS;
                        Rect::from_corners(Vec2::ZERO, screen::CANVAS)
                            .contains(p)
                            .then_some(p)
                    });
                    if let Some(p) = point {
                        match &session.page {
                            Page::Menu(_) => {
                                if let Some((row, _)) = rows.0.rows.iter().find(|(_, r)| r.contains(p)) {
                                    out.write(TerminalInput::Row(*row));
                                    out.write(TerminalInput::Select);
                                }
                            }
                            Page::LockedOut(_) => {
                                out.write(TerminalInput::Back);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        if drop_tail { break; }
    }
    if drop_tail {
        state.gesture = None;
        state.contacts.clear();
        touches.clear();
    }
}
fn label(commands: &mut ChildSpawnerCommands, text: &str, rect: Rect, size: f32, color: Color) {
    commands.spawn((
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(rect.min.x),
            top: Val::Px(rect.min.y),
            width: Val::Px(rect.width()),
            height: Val::Px(rect.height()),
            ..default()
        },
    ));
}
#[allow(clippy::too_many_arguments)]
pub(super) fn draw(
    mut commands: Commands,
    screen: Res<TerminalScreen>,
    settings: Res<Settings>,
    state: Res<TouchState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    roots: Query<Entity, With<TouchRoot>>,
    mut shown: Local<String>,
) {
    let Ok(window) = windows.single() else { return };
    let session = screen.0.as_ref().filter(|_| settings.enabled);
    let key = format!(
        "{:?}:{}:{}:{:?}:{}:{:?}:{}",
        session.map(super::view_key),
        window.width(),
        window.height(),
        (state.first_row, state.column, state.pan_x),
        state.options,
        settings,
        state.message
    );
    if *shown == key {
        return;
    }
    *shown = key;
    for e in &roots {
        commands.entity(e).despawn();
    }
    let Some(session) = session else { return };
    let g = Geometry::new(Vec2::new(window.width(), window.height()), settings.zoom)
        .for_column(state.column)
        .for_pan(state.pan_x);
    let color = Color::srgb(0.4, 1.0, 0.65);
    let Page::Hacking(h) = &session.page else {
        commands
            .spawn((
                TouchRoot,
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                GlobalZIndex(3500),
            ))
            .with_children(|root| {
                for (i, text) in [(5, "Back"), (6, "Exit")] {
                    let rect = g.action(i);
                    root.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(rect.min.x),
                            top: Val::Px(rect.min.y),
                            width: Val::Px(rect.width()),
                            height: Val::Px(rect.height()),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.05, 0.15, 0.1)),
                    ));
                    label(root, text, rect, 20.0, color);
                }
            });
        return;
    };
    commands
        .spawn((
            TouchRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.01, 0.025, 0.015)),
            GlobalZIndex(3500),
        ))
        .with_children(|root| {
            label(
                root,
                &format!(
                    "Terminal · {} attempts · {}",
                    h.puzzle.attempts,
                    h.puzzle.log.last().map_or("", String::as_str)
                ),
                Rect::new(
                    16.0,
                    4.0,
                    g.size.x - if g.columns == 1 { 128.0 } else { 16.0 },
                    if g.grid_top >= 104.0 { 32.0 } else { 4.0 },
                ),
                20.0,
                color,
            );
            let token = if h.hidden {
                "Select a word or bracket".to_string()
            } else {
                h.puzzle.text(h.puzzle.target(h.cursor))
            };
            label(
                root,
                &token,
                Rect::new(
                    16.0,
                    if g.grid_top >= 104.0 { 40.0 } else { 8.0 },
                    g.size.x - if g.columns == 1 { 128.0 } else { 16.0 },
                    g.grid_top - 8.0,
                ),
                32.0,
                color,
            );
            if !state.options {
                if let Some(rect) = g.column_rect() {
                    root.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(rect.min.x),
                            top: Val::Px(rect.min.y),
                            width: Val::Px(rect.width()),
                            height: Val::Px(rect.height()),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.05, 0.15, 0.1)),
                    ));
                    label(
                        root,
                        &format!("Column {}/2", state.column + 1),
                        rect,
                        18.0,
                        color,
                    );
                }
            }
            if state.options {
                label(
                    root,
                    &format!(
                        "Touch options\nScheme: {:?}\n{}\nText size: {}%\n{}",
                        settings.scheme,
                        if settings.separate_confirm {
                            "Select, then Guess"
                        } else {
                            "Tap enters immediately"
                        },
                        100 + settings.zoom as u32 * 25,
                        state.message
                    ),
                    Rect::new(
                        20.0,
                        g.grid_top,
                        g.size.x - 20.0,
                        g.size.y - g.toolbar - 4.0,
                    ),
                    22.0,
                    color,
                );
            } else {
                let selected = (!h.hidden).then(|| h.puzzle.span(h.puzzle.target(h.cursor)));
                let tokens = g.tokens(&h.puzzle, state.first_row);
                for (at, rect) in &tokens {
                    let lit = selected.as_ref().is_some_and(|span| span.contains(at));
                    root.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(rect.min.x),
                            top: Val::Px(rect.min.y),
                            width: Val::Px(rect.width()),
                            height: Val::Px(rect.height()),
                            ..default()
                        },
                        BackgroundColor(if lit {
                            color
                        } else {
                            Color::srgb(0.035, 0.09, 0.055)
                        }),
                    ));
                }
                for (index, character) in h.puzzle.chars.iter().enumerate() {
                    if let Some(rect) = g.cell_rect(index, state.first_row) {
                        let lit = selected.as_ref().is_some_and(|span| span.contains(&index));
                        if lit {
                            root.spawn((
                                Node {
                                    position_type: PositionType::Absolute,
                                    left: Val::Px(rect.min.x),
                                    top: Val::Px(rect.min.y),
                                    width: Val::Px(rect.width()),
                                    height: Val::Px(rect.height()),
                                    ..default()
                                },
                                BackgroundColor(color),
                            ));
                        }
                        label(
                            root,
                            &character.to_string(),
                            rect,
                            24.0 + settings.zoom as f32 * 4.0,
                            if lit { Color::BLACK } else { color },
                        );
                    }
                }
            }
            let labels = if state.options {
                [
                    "Scheme", "Tap mode", "Text -", "Text +", "Done", "Done", "Done",
                ]
            } else {
                ["←", "↑", "↓", "→", "Guess", "Options", "Exit"]
            };
            for (i, text) in labels.iter().enumerate() {
                let rect = g.action(i);
                root.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(rect.min.x),
                        top: Val::Px(rect.min.y),
                        width: Val::Px(rect.width()),
                        height: Val::Px(rect.height()),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.05, 0.15, 0.1)),
                ));
                label(
                    root,
                    text,
                    Rect::from_corners(rect.min + Vec2::splat(6.0), rect.max),
                    18.0,
                    color,
                );
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use fnv_game::hacking::{Word, ATTEMPTS, DUMP_CHARS};
    fn puzzle() -> Puzzle {
        let mut chars = vec!['.'; DUMP_CHARS];
        for (start, text) in [(0, "AAAAA"), (12, "BBBBB"), (30, "[!!]")] {
            for (i, c) in text.chars().enumerate() {
                chars[start + i] = c;
            }
        }
        Puzzle {
            chars,
            address: 0,
            words: vec![
                Word {
                    text: "AAAAA".into(),
                    len: 5,
                    start: 0,
                    removed: false,
                },
                Word {
                    text: "BBBBB".into(),
                    len: 5,
                    start: 12,
                    removed: false,
                },
            ],
            password: 1,
            attempts: ATTEMPTS,
            used: vec![],
            log: vec![],
            state: GameState::Guessing,
            rules: fnv_game::hacking::HackRules::default(),
            guessed: Vec::new(),
        }
    }
    fn scene() -> (App, Entity) {
        let mut script = crate::script::ScriptState::over_no_plugins();
        script.rt.as_mut().unwrap().game.open_menu(MENU_HACKING);
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::input::InputPlugin,
            bevy::state::app::StatesPlugin,
        ))
        .insert_state(PlayState::Menu)
        .init_resource::<TouchState>()
        .init_resource::<MenuRows>()
        .init_resource::<TerminalStats>()
        .init_resource::<super::super::ReadEntries>()
        .init_resource::<crate::pause::PlayClock>()
        .init_resource::<crate::bindings::Bindings>()
        .insert_resource(Settings {
            enabled: true,
            allow_write: false,
            ..default()
        })
        .insert_resource(TerminalScreen(Some(Session {
            reference: FormId(1),
            base: FormId(2),
            page: Page::Hacking(Hacking::new(puzzle(), 0)),
        })))
        .insert_resource(script)
        .insert_resource(crate::sim::test_world::sim(vec![], vec![]))
        .add_message::<TouchInput>()
        .add_message::<WindowFocused>()
        .add_message::<AppLifecycle>()
        .add_message::<TerminalInput>()
        .add_message::<UiSound>()
        .add_message::<crate::lockpick::LockOpened>()
        .add_systems(
            Update,
            (input, super::super::keys, super::super::apply).chain(),
        );
        let window = app
            .world_mut()
            .spawn((
                Window {
                    resolution: bevy::window::WindowResolution::new(792, 363),
                    ..default()
                },
                PrimaryWindow,
            ))
            .id();
        app.update();
        app.update();
        (app, window)
    }
    fn event(app: &mut App, w: Entity, id: u64, phase: TouchPhase, p: Vec2) {
        app.world_mut().write_message(TouchInput {
            window: w,
            id,
            phase,
            position: p,
            force: None,
        });
    }
    fn tap(app: &mut App, w: Entity, p: Vec2) {
        event(app, w, 1, TouchPhase::Started, p);
        event(app, w, 1, TouchPhase::Ended, p);
        app.update();
    }
    fn at(app: &App, index: usize) -> Vec2 {
        Geometry::new(
            Vec2::new(792.0, 363.0),
            app.world().resource::<Settings>().zoom,
        )
        .cell_rect(index, app.world().resource::<TouchState>().first_row)
        .unwrap()
        .center()
    }
    fn hacking(app: &App) -> &Hacking {
        let Page::Hacking(h) = &app
            .world()
            .resource::<TerminalScreen>()
            .0
            .as_ref()
            .unwrap()
            .page
        else {
            panic!()
        };
        h
    }
    #[test]
    fn loading_cover_discards_a_terminal_gesture_before_reveal() {
        let (mut app, w) = scene();
        app.init_resource::<crate::pad::PlatformInputGate>();
        let p = at(&app, 0);
        event(&mut app, w, 78, TouchPhase::Started, p);
        app.update();
        assert!(app.world().resource::<TouchState>().gesture.is_some());
        app.world_mut().resource_mut::<crate::pad::PlatformInputGate>().set_blocked(true);
        event(&mut app, w, 78, TouchPhase::Ended, p);
        app.update();
        assert!(app.world().resource::<TouchState>().gesture.is_none());
        assert!(app.world().resource::<TouchState>().contacts.is_empty());
        assert!(hacking(&app).hidden);
        app.world_mut().resource_mut::<crate::pad::PlatformInputGate>().set_blocked(false);
        event(&mut app, w, 78, TouchPhase::Ended, p);
        app.update();
        assert!(hacking(&app).hidden);
        tap(&mut app, w, p);
        assert!(!hacking(&app).hidden);
    }
    #[test]
    fn v1_settings_roundtrip_and_unknown_versions_never_rewrite_the_existing_file() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "nvf-terminal-touch-v1-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("terminal-touch.json");
        let settings = Settings {
            enabled: true,
            ..default()
        };
        settings.save_to(&path).unwrap();
        let stored = std::fs::read(&path).unwrap();
        let decoded = Settings::decode(&stored).unwrap();
        assert!(decoded.enabled);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&stored).unwrap()["version"],
            1
        );
        let future = br#"{"version":99,"settings":{"enabled":true},"future":"keep exactly"}"#;
        std::fs::write(&path, future).unwrap();
        let mut blocked = settings.clone();
        assert!(blocked.import(future).is_err());
        assert!(blocked.save_to(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), future);
        assert!(Settings::decode(&vec![b' '; SETTINGS_LIMIT + 1]).is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }
    #[test]
    fn raw_word_tap_selects_whole_token_and_only_explicit_guess_spends_one_attempt() {
        let (mut app, w) = scene();
        let p = at(&app, 3);
        tap(&mut app, w, p);
        assert_eq!(hacking(&app).cursor, 0);
        assert!(!hacking(&app).hidden);
        assert_eq!(hacking(&app).puzzle.attempts, ATTEMPTS);
        let guess = Geometry::new(Vec2::new(792.0, 363.0), 0).action(4).center();
        // Even multiple simultaneous confirmation producers commit once in apply.
        app.world_mut().write_message(TerminalInput::Select);
        tap(&mut app, w, guess);
        assert_eq!(hacking(&app).puzzle.attempts, ATTEMPTS - 1);
        assert_eq!(app.world().resource::<TerminalStats>().guesses, 1);
    }
    #[test]
    fn correct_password_keeps_existing_grant_and_hacked_access_outcome() {
        let (mut app, w) = scene();
        let p = at(&app, 14);
        tap(&mut app, w, p);
        let guess = Geometry::new(Vec2::new(792.0, 363.0), 0).action(4).center();
        tap(&mut app, w, guess);
        assert_eq!(hacking(&app).puzzle.state, GameState::Granted);
        assert!(hacking(&app).ended.is_some());
        assert_eq!(app.world().resource::<TerminalStats>().hacked, 1);
        assert_eq!(
            app.world()
                .resource::<crate::script::ScriptState>()
                .rt
                .as_ref()
                .unwrap()
                .game
                .terminals
                .access
                .get(&FormId(1)),
            Some(&fnv_game::terminal::Access::Hacked)
        );
    }
    #[test]
    fn bracket_selection_reaches_real_rules_only_after_confirmation() {
        let (mut app, w) = scene();
        let p = at(&app, 31);
        tap(&mut app, w, p);
        assert_eq!(hacking(&app).cursor, 30);
        assert!(hacking(&app).puzzle.used.is_empty());
        let guess = Geometry::new(Vec2::new(792.0, 363.0), 0).action(4).center();
        tap(&mut app, w, guess);
        assert_eq!(hacking(&app).puzzle.used, vec![30]);
        assert_eq!(app.world().resource::<TerminalStats>().pairs, 1);
        assert_eq!(app.world().resource::<TerminalStats>().guesses, 0);
    }
    #[test]
    fn opening_pan_focus_multitouch_and_cancel_do_not_burn_attempts() {
        for case in 0..5 {
            let (mut app, w) = scene();
            app.world_mut().resource_mut::<Settings>().separate_confirm = false;
            let p = at(&app, 0);
            event(&mut app, w, 1, TouchPhase::Started, p);
            match case {
                0 => {
                    app.world_mut()
                        .resource_mut::<TerminalScreen>()
                        .0
                        .as_mut()
                        .unwrap()
                        .reference = FormId(8)
                }
                1 => event(&mut app, w, 1, TouchPhase::Moved, p - Vec2::new(0.0, 72.0)),
                2 => {
                    app.world_mut().write_message(WindowFocused {
                        window: w,
                        focused: false,
                    });
                }
                3 => event(&mut app, w, 2, TouchPhase::Started, p),
                _ => event(&mut app, w, 1, TouchPhase::Canceled, p),
            }
            event(&mut app, w, 1, TouchPhase::Ended, p);
            app.update();
            assert_eq!(hacking(&app).puzzle.attempts, ATTEMPTS, "case {case}");
            assert_eq!(app.world().resource::<TerminalStats>().guesses, 0);
        }
    }
    #[test]
    fn resume_drops_the_old_contact_and_allows_a_fresh_selection() {
        let (mut app, w) = scene();
        app.world_mut().resource_mut::<Settings>().separate_confirm = false;
        let p = at(&app, 0);
        event(&mut app, w, 1, TouchPhase::Started, p);
        app.update();
        app.world_mut().write_message(AppLifecycle::Suspended);
        app.update();
        app.world_mut().write_message(AppLifecycle::Running);
        event(&mut app, w, 1, TouchPhase::Ended, p);
        app.update();
        assert_eq!(hacking(&app).puzzle.attempts, ATTEMPTS);
        tap(&mut app, w, p);
        assert_eq!(hacking(&app).puzzle.attempts, ATTEMPTS - 1);
    }
    #[test]
    fn controller_return_touch_is_consumed_and_fresh_cursor_pad_actions_work() {
        let (mut app, w) = scene();
        app.init_resource::<crate::pad::TouchDeviceSwitch>()
            .add_plugins(crate::pad::InputSwitchPlugin);
        app.world_mut()
            .resource_mut::<crate::bindings::Bindings>()
            .device = crate::bindings::InputDevice::Controller;
        let p = at(&app, 2);
        tap(&mut app, w, p);
        assert!(hacking(&app).hidden);
        assert_eq!(app.world().resource::<crate::bindings::Bindings>().device,
            crate::bindings::InputDevice::KeyboardMouse);
        assert!(!app.world().resource::<crate::pad::TouchDeviceSwitch>().allows_actions());
        app.world_mut().resource_mut::<Settings>().scheme = Scheme::Cursor;
        tap(&mut app, w, p);
        assert!(
            hacking(&app).hidden,
            "cursor scheme does not guess on dump taps"
        );
        let right = Geometry::new(Vec2::new(792.0, 363.0), 0).action(3).center();
        tap(&mut app, w, right);
        assert!(!hacking(&app).hidden);
        assert_eq!(hacking(&app).puzzle.attempts, ATTEMPTS);
    }
    #[test]
    fn options_block_keyboard_and_other_guess_producers_until_closed() {
        let (mut app, w) = scene();
        let p = at(&app, 0);
        tap(&mut app, w, p);
        app.world_mut().resource_mut::<TouchState>().options = true;
        app.world_mut().write_message(TerminalInput::Select);
        app.world_mut()
            .write_message(bevy::input::keyboard::KeyboardInput {
                key_code: KeyCode::Enter,
                logical_key: bevy::input::keyboard::Key::Enter,
                state: bevy::input::ButtonState::Pressed,
                text: None,
                repeat: false,
                window: w,
            });
        app.update();
        assert_eq!(hacking(&app).puzzle.attempts, ATTEMPTS);
        app.world_mut()
            .write_message(bevy::input::keyboard::KeyboardInput {
                key_code: KeyCode::Escape,
                logical_key: bevy::input::keyboard::Key::Escape,
                state: bevy::input::ButtonState::Pressed,
                text: None,
                repeat: false,
                window: w,
            });
        app.update();
        assert!(!app.world().resource::<TouchState>().options);
        let guess = Geometry::new(Vec2::new(792.0, 363.0), 0).action(4).center();
        tap(&mut app, w, guess);
        assert_eq!(hacking(&app).puzzle.attempts, ATTEMPTS - 1);
    }
    #[test]
    fn reopening_the_same_reference_discards_the_opening_touch() {
        let (mut app, w) = scene();
        app.world_mut().resource_mut::<Settings>().separate_confirm = false;
        app.world_mut().resource_mut::<TerminalScreen>().0 = None;
        app.update();
        app.world_mut().resource_mut::<TerminalScreen>().0 = Some(Session {
            reference: FormId(1),
            base: FormId(2),
            page: Page::Hacking(Hacking::new(puzzle(), 0)),
        });
        let p = at(&app, 0);
        tap(&mut app, w, p);
        assert_eq!(hacking(&app).puzzle.attempts, ATTEMPTS);
        tap(&mut app, w, p);
        assert_eq!(hacking(&app).puzzle.attempts, ATTEMPTS - 1);
    }
    #[test]
    fn small_landscape_wraps_actions_and_keeps_usable_token_targets_at_least48px() {
        let puzzle = puzzle();
        for size in [
            Vec2::new(320.0, 240.0),
            Vec2::new(360.0, 280.0),
            Vec2::new(592.0, 320.0),
            Vec2::new(792.0, 363.0),
        ] {
            for zoom in 0..=2 {
                let g = Geometry::new(size, zoom);
                assert!(g.row >= 48.0);
                for index in 0..7 {
                    let r = g.action(index);
                    assert!(r.width() >= 48.0 && r.height() >= 48.0);
                    assert!(r.min.x >= 0.0 && r.max.x <= size.x && r.max.y <= size.y);
                }
                for (_, r) in g.tokens(&puzzle, 0) {
                    assert!(r.width() >= 48.0 && r.height() >= 48.0);
                    assert!(r.min.x >= 16.0 && r.max.x <= size.x - 16.0);
                }
                if g.columns == 1 {
                    assert!(g.column_rect().unwrap().height() >= 48.0);
                }
            }
        }
    }
    #[test]
    fn zoomed_single_column_pans_without_guessing_and_column_switch_keeps_full_size_cells() {
        let (mut app, w) = scene();
        app.world_mut().resource_mut::<Settings>().zoom = 2;
        // A landscape surface narrower than two full-size columns gets a column toggle.
        app.world_mut()
            .get_mut::<Window>(w)
            .unwrap()
            .resolution
            .set(320.0, 240.0);
        app.update();
        let g = Geometry::new(Vec2::new(320.0, 240.0), 2);
        let p = g.cell_rect(3, 0).unwrap().center();
        event(&mut app, w, 1, TouchPhase::Started, p);
        event(&mut app, w, 1, TouchPhase::Moved, p - Vec2::new(90.0, 0.0));
        event(&mut app, w, 1, TouchPhase::Ended, p - Vec2::new(90.0, 0.0));
        app.update();
        assert!(app.world().resource::<TouchState>().pan_x > 0.0);
        assert_eq!(hacking(&app).puzzle.attempts, ATTEMPTS);
        tap(&mut app, w, g.column_rect().unwrap().center());
        assert_eq!(app.world().resource::<TouchState>().column, 1);
        assert_eq!(app.world().resource::<TouchState>().pan_x, 0.0);
        assert_eq!(hacking(&app).puzzle.attempts, ATTEMPTS);
        assert_eq!(g.cell, 32.0);
    }
    #[test]
    fn a_partial_word_at_the_scroll_boundary_keeps_its_padded48px_target() {
        let mut p = puzzle();
        p.words[0].start = 11;
        p.words[1].start = 36;
        p.chars.fill('.');
        for word in &p.words {
            for (k, c) in word.text.chars().enumerate() {
                p.chars[word.start + k] = c;
            }
        }
        let g = Geometry::new(Vec2::new(320.0, 240.0), 0);
        assert_eq!(g.visible, 1);
        let (_, rect) = g
            .tokens(&p, 0)
            .into_iter()
            .find(|(at, _)| *at == 11)
            .unwrap();
        assert!(rect.width() >= 48.0 && rect.height() >= 48.0);
        assert_eq!(
            g.token_hit(&p, Vec2::new(rect.min.x + 2.0, rect.center().y), 0, false),
            Hit::Cell(11)
        );
    }
    #[test]
    fn persisted_options_clamp_zoom_and_share_rendered_cell_targets() {
        let mut value: Settings = serde_json::from_str(
            r#"{"enabled":true,"zoom":255,"scheme":"cursor","separate_confirm":true}"#,
        )
        .unwrap();
        value.normalize();
        assert_eq!(value.zoom, 2);
        assert!(matches!(value.scheme, Scheme::Cursor));
        for zoom in 0..=2 {
            let g = Geometry::new(Vec2::new(792.0, 363.0), zoom);
            for i in [0, 12, 204, 216] {
                let chosen =
                    Geometry::new(Vec2::new(792.0, 363.0), zoom).for_column(i / (ROWS * ROW_CHARS));
                let r = chosen.cell_rect(i, 0).unwrap();
                assert_eq!(chosen.hit(r.center(), 0, false), Hit::Cell(i));
            }
            assert!(g.action(4).height() >= 44.0);
        }
    }
    #[test]
    fn closing_options_blocks_simultaneous_physical_and_queued_confirmations_until_a_fresh_gesture() {
        for close_with_escape in [false, true] {
            let (mut app, w) = scene();
            let p = at(&app, 0);
            tap(&mut app, w, p);
            app.world_mut().resource_mut::<TouchState>().options = true;
            app.world_mut().write_message(TerminalInput::Select);
            let key_code = if close_with_escape { KeyCode::Escape } else { KeyCode::Enter };
            app.world_mut().write_message(bevy::input::keyboard::KeyboardInput {
                window: w, key_code,
                logical_key: bevy::input::keyboard::Key::Character("terminal fixture".into()),
                state: bevy::input::ButtonState::Pressed, text: None, repeat: false,
            });
            if !close_with_escape {
                let done = Geometry::new(Vec2::new(792.0, 363.0), 0).action(4).center();
                event(&mut app, w, 7, TouchPhase::Started, done);
                event(&mut app, w, 7, TouchPhase::Ended, done);
            }
            app.update();
            assert!(!app.world().resource::<TouchState>().options);
            assert_eq!(hacking(&app).puzzle.attempts, ATTEMPTS);
            assert_eq!(app.world().resource::<TerminalStats>().guesses, 0);
            let guess = Geometry::new(Vec2::new(792.0, 363.0), 0).action(4).center();
            tap(&mut app, w, guess);
            assert_eq!(hacking(&app).puzzle.attempts, ATTEMPTS - 1);
            assert_eq!(app.world().resource::<TerminalStats>().guesses, 1);
        }
    }
    #[test]
    fn controller_handoff_closes_touch_options_and_does_not_block_later_commands() {
        let (mut app, w) = scene();
        let p = at(&app, 0);
        tap(&mut app, w, p);
        app.world_mut().resource_mut::<TouchState>().options = true;
        app.world_mut().resource_mut::<crate::bindings::Bindings>().device = crate::bindings::InputDevice::Controller;
        app.world_mut().write_message(TerminalInput::Select);
        app.update();
        assert!(!app.world().resource::<TouchState>().options);
        assert_eq!(hacking(&app).puzzle.attempts, ATTEMPTS);
        app.world_mut().write_message(TerminalInput::Select);
        app.update();
        assert_eq!(hacking(&app).puzzle.attempts, ATTEMPTS - 1);
    }

    #[test]
    fn column_switch_discards_same_batch_word_tap_without_replaying_a_wrong_guess() {
        let (mut app, w) = scene();
        let second_column = ROWS * ROW_CHARS;
        {
            let mut screen = app.world_mut().resource_mut::<TerminalScreen>();
            let Page::Hacking(h) = &mut screen.0.as_mut().unwrap().page else {
                panic!()
            };
            h.puzzle.words[1].start = second_column;
            h.puzzle.chars.fill('.');
            for word in &h.puzzle.words {
                for (i, c) in word.text.chars().enumerate() {
                    h.puzzle.chars[word.start + i] = c;
                }
            }
        }
        app.world_mut().resource_mut::<Settings>().separate_confirm = false;
        app.world_mut()
            .get_mut::<Window>(w)
            .unwrap()
            .resolution
            .set(592.0, 363.0);
        app.update();
        let g = Geometry::new(Vec2::new(592.0, 363.0), 0);
        let column = g.column_rect().unwrap().center();
        let word = g.for_column(1).cell_rect(second_column + 2, 0).unwrap().center();
        event(&mut app, w, 31, TouchPhase::Started, column);
        event(&mut app, w, 31, TouchPhase::Ended, column);
        event(&mut app, w, 32, TouchPhase::Started, word);
        event(&mut app, w, 32, TouchPhase::Ended, word);
        app.update();
        assert_eq!(app.world().resource::<TouchState>().column, 1);
        assert_eq!(hacking(&app).puzzle.attempts, ATTEMPTS);
        assert_eq!(app.world().resource::<TerminalStats>().guesses, 0);
        app.update();
        assert_eq!(hacking(&app).puzzle.attempts, ATTEMPTS);
        assert_eq!(app.world().resource::<TerminalStats>().guesses, 0);
        tap(&mut app, w, word);
        assert_eq!(hacking(&app).cursor, second_column);
        assert_eq!(hacking(&app).puzzle.state, GameState::Granted);
        assert_eq!(app.world().resource::<TerminalStats>().hacked, 1);
    }

    #[test]
    fn panning_discards_same_batch_word_tap_on_release_or_cancel_without_replay() {
        for finish in [TouchPhase::Ended, TouchPhase::Canceled] {
            let (mut app, w) = scene();
            {
                let mut screen = app.world_mut().resource_mut::<TerminalScreen>();
                let Page::Hacking(h) = &mut screen.0.as_mut().unwrap().page else {
                    panic!()
                };
                h.puzzle.words[1].start = 6;
                h.puzzle.chars.fill('.');
                for word in &h.puzzle.words {
                    for (i, c) in word.text.chars().enumerate() {
                        h.puzzle.chars[word.start + i] = c;
                    }
                }
            }
            {
                let mut settings = app.world_mut().resource_mut::<Settings>();
                settings.zoom = 2;
                settings.separate_confirm = false;
            }
            app.world_mut()
                .get_mut::<Window>(w)
                .unwrap()
                .resolution
                .set(320.0, 363.0);
            app.update();
            let g = Geometry::new(Vec2::new(320.0, 363.0), 2);
            let origin = g.cell_rect(3, 0).unwrap().center();
            let end = origin - Vec2::new(90.0, 0.0);
            let word = g.for_pan(90.0).cell_rect(7, 0).unwrap().center();
            event(&mut app, w, 41, TouchPhase::Started, origin);
            event(&mut app, w, 41, TouchPhase::Moved, end);
            event(&mut app, w, 41, finish, end);
            event(&mut app, w, 42, TouchPhase::Started, word);
            event(&mut app, w, 42, TouchPhase::Ended, word);
            app.update();
            assert_eq!(app.world().resource::<TouchState>().pan_x, 90.0);
            assert_eq!(hacking(&app).puzzle.attempts, ATTEMPTS);
            assert_eq!(app.world().resource::<TerminalStats>().guesses, 0);
            app.update();
            assert_eq!(hacking(&app).puzzle.attempts, ATTEMPTS);
            assert_eq!(app.world().resource::<TerminalStats>().guesses, 0);
            tap(&mut app, w, word);
            assert_eq!(hacking(&app).cursor, 6);
            assert_eq!(hacking(&app).puzzle.state, GameState::Granted);
            assert_eq!(app.world().resource::<TerminalStats>().hacked, 1);
        }
    }
    include!("touch_toggle_tests.rs");
}
