//! Touch dialogue view and release-only gestures [design: android-dialogue-accessibility.md].
#[cfg(test)]
#[path = "dialogue_touch/split_tests.rs"]
mod split_tests;
pub(crate) mod appearance;
mod native;
pub(crate) mod device_canvas;
mod classic;
pub(crate) mod classic_presenter;
pub(crate) mod classic_state;
use crate::dialogue::{ListId, ViewInput};
use crate::dialogue_menu::DialogueMenu;
use crate::ui_pointer::{Button, PointerState};
use bevy::input::touch::{TouchInput, TouchPhase};
use bevy::prelude::*;
use bevy::ui::{CalculatedClip, ComputedUiTargetCamera, UiGlobalTransform};
use bevy::window::{AppLifecycle, PrimaryWindow, WindowFocused};
use std::collections::HashSet;

#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct TouchDialogue {
    pub enabled: bool,
    pub choice_scale: f32,
    pub subtitle_scale: f32,
    pub confirm: bool,
    /// [design] Platform text size at 100%, in logical units: phones hold more
    /// choices at a smaller base. Not saved; the saved sliders stay percentages of it.
    pub text_base: f32,
}
impl Default for TouchDialogue {
    fn default() -> Self {
        Self::for_platform(cfg!(target_os = "android"))
    }
}
impl TouchDialogue {
    fn for_platform(android: bool) -> Self {
        Self { enabled: android, choice_scale: if android { 0.85 } else { 1.0 }, subtitle_scale: 1.0, confirm: !android,
            text_base: if android { 16.0 } else { 20.0 } }
    }
    /// The conversation owns raw touches; the underlying virtual menu keys must sleep.
    pub fn captures(&self, state: crate::PlayState) -> bool {
        self.enabled && state == crate::PlayState::Dialogue
    }
    /// Choice text size in logical units for default/normal dialogue themes.
    pub fn choice_size(&self) -> f32 { self.text_base * scale(self.choice_scale) }
    /// Speaking-line text size in logical units for default/normal dialogue themes.
    pub fn subtitle_size(&self) -> f32 { self.text_base * scale(self.subtitle_scale) }
    /// Per-theme text size multiplier: Wrist Terminal and Classic bezels start smaller
    /// so more choices and subtitles fit cleanly inside the hardware frame without
    /// changing Normal/Minimal or Split Panel defaults.
    pub(crate) fn theme_text_factor(theme: appearance::Theme) -> f32 {
        match theme {
            appearance::Theme::WristTerminal | appearance::Theme::ClassicGlass => 0.78,
            _ => 1.0,
        }
    }
    /// Choice text size adjusted for `theme`.
    pub(crate) fn choice_size_for(&self, theme: appearance::Theme) -> f32 {
        self.choice_size() * Self::theme_text_factor(theme)
    }
    /// Speaking-line text size adjusted for `theme`.
    pub(crate) fn subtitle_size_for(&self, theme: appearance::Theme) -> f32 {
        self.subtitle_size() * Self::theme_text_factor(theme)
    }
}
/// Platform layout policy is separate from persisted text sizes. Existing saved
/// scales are never interpreted as a request to reset density or accessibility.
#[derive(Resource, Clone, Copy, PartialEq)]
struct TouchLayout {
    height: f32,
    padding: f32,
    gap: f32,
    row_min: f32,
    row_padding: f32,
    confirm_padding: f32,
    confirm_text: f32,
}
impl Default for TouchLayout {
    fn default() -> Self { Self::for_platform(cfg!(target_os = "android")) }
}
impl TouchLayout {
    fn for_platform(android: bool) -> Self {
        if android {
            Self { height: 75.0, padding: 8.0, gap: 4.0, row_min: 48.0, row_padding: 6.0, confirm_padding: 8.0, confirm_text: 18.0 }
        } else {
            Self { height: 65.0, padding: 12.0, gap: 8.0, row_min: 56.0, row_padding: 10.0, confirm_padding: 12.0, confirm_text: 20.0 }
        }
    }
}
fn scale(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.75, 2.0)
    } else {
        1.0
    }
}
#[cfg(any(target_os = "android", test))]
fn part(v: f32) -> f32 {
    (scale(v) - 0.75) / 1.25
}
#[cfg(any(target_os = "android", test))]
fn from_part(v: f32) -> f32 {
    scale(
        0.75 + if v.is_finite() {
            v.clamp(0.0, 1.0)
        } else {
            0.2
        } * 1.25,
    )
}
#[cfg(any(target_os = "android", test))]
fn shown(v: f32) -> String {
    format!("{:.0}%", from_part(v) * 100.0)
}
#[cfg(any(target_os = "android", test))]
fn mode(w: &World) -> String {
    if w.resource::<TouchDialogue>().confirm {
        "Select then confirm"
    } else {
        "Direct tap"
    }
    .into()
}
#[cfg(any(target_os = "android", test))]
fn set_mode(w: &mut World, name: &str) -> bool {
    let confirm = match name {
        "Select then confirm" => true,
        "Direct tap" => false,
        _ => return false,
    };
    w.resource_mut::<TouchDialogue>().confirm = confirm;
    true
}

#[cfg(any(target_os = "android", test))]
fn set_enabled(world: &mut World, enabled: bool) {
    if world.resource::<TouchDialogue>().enabled == enabled { return; }
    world.resource_mut::<TouchDialogue>().enabled = enabled;
    let (window, ids) = if let Some(mut gestures) = world.get_resource_mut::<Gestures>() {
        let owned = (gestures.guard.as_ref().map(|g| g.window), gestures.contacts.iter().copied().take(33).collect::<Vec<_>>());
        gestures.gesture = None;
        gestures.armed = None;
        gestures.contacts.clear();
        owned
    } else { (None, Vec::new()) };
    if let Some(mut frame) = world.get_resource_mut::<TouchFrame>() { frame.active = false; frame.asked = ViewInput::default(); frame.barter = false; }
    crate::touch_ui_options::retire(world, crate::touch_ui_options::Feature::Dialogue, window, &ids);
}

pub(crate) fn install(app: &mut App) {
    crate::dialogue_split::install(app);
    crate::dialogue_device::install(app);
    crate::touch_ui_options::install(app);
    appearance::install(app);
    native::install(app);
    device_canvas::install(app);
    classic_presenter::install(app);
    classic_state::install(app);
    app.init_resource::<TouchDialogue>()
        .init_resource::<TouchLayout>()
        .init_resource::<TouchFrame>()
        .init_resource::<Gestures>()
        .init_resource::<RevealRequest>()
        .add_message::<TouchInput>()
        .add_message::<WindowFocused>()
        .add_message::<AppLifecycle>()
        .add_systems(
            Update,
            read.before(crate::dialogue::input)
                .in_set(crate::PlaySet::Scripts),
        )
        .add_systems(
            Update,
            (draw, reveal_selected)
                .chain()
                .in_set(crate::PlaySet::Ui)
                .after(crate::dialogue_menu::DrawMenu),
        );
    #[cfg(any(target_os = "android", test))]
    {
        use crate::settings::{add_choice_list, add_setting, ChoiceList, Setting, Slider};
        add_setting(app, Setting { key: "touch-dialogue", group: crate::pause::GROUP_MENUS, label: "Touch dialogue",
            note: "Wide wrapped dialogue choices, large targets and swipe scrolling. Uses the same offered choices and game actions. On by default on Android.",
            switches: "",
            get: |w| w.resource::<TouchDialogue>().enabled, set: set_enabled, slider: None });
        add_setting(app, Setting { key: "dialogue-choice-text-size", group: crate::pause::GROUP_MENUS, label: "Dialogue choice text",
            note: "Choice text size in every dialogue style (75-200%), independent of subtitles. Phones use a smaller 100% and start at 85% to fit more choices; your saved size is kept. Rows stay at least 48 units tall.",
            switches: "",
            get: |_| false, set: |_,_| {}, slider: Some(Slider { get: |w| part(w.resource::<TouchDialogue>().choice_scale),
                set: |w,v| w.resource_mut::<TouchDialogue>().choice_scale=from_part(v), min:0.0,step:0.04,shown:Some(shown) }) });
        add_setting(app, Setting { key: "dialogue-subtitle-text-size", group: crate::pause::GROUP_MENUS, label: "Dialogue subtitle text",
            note: "Speaking-line text size in every dialogue style (75-200%; phones use a smaller 100%). Does not change choices or world HUD subtitles.",
            switches: "",
            get: |_| false, set: |_,_| {}, slider: Some(Slider { get: |w| part(w.resource::<TouchDialogue>().subtitle_scale),
                set: |w,v| w.resource_mut::<TouchDialogue>().subtitle_scale=from_part(v), min:0.0,step:0.04,shown:Some(shown) }) });
        add_choice_list(app, Setting { key:"dialogue-touch-choice",group:crate::pause::GROUP_MENUS,label:"Touch dialogue choices",
            note:"Select then confirm: tap a choice, then Confirm selection. Direct tap: a released tap answers. A swipe never answers. Keyboard/controller and real mouse keep their usual actions.",
            switches: "",
            get: |_|false,set:|_,_|{},slider:None }, ChoiceList { names: |_|vec!["Select then confirm".into(),"Direct tap".into()],
                chosen:mode,pick:set_mode,save:mode,load:set_mode,ask_keep:false });
    }
}

#[derive(Resource, Default)]
pub(crate) struct TouchFrame {
    pub active: bool,
    pub asked: ViewInput,
    pub barter: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Choice(usize),
    Confirm,
    Skip,
    Barter,
    PowerExit,
}
#[derive(Component)]
struct Action {
    kind: Kind,
    list: Option<ListId>,
}
/// Actual hardware and screen owners paint their own palettes. Generic HUD
/// recolouring must not tint their casing or reinterpret Pip-Boy text as HUD ink.
#[derive(Component)]
pub(crate) struct PaletteRoot;

#[derive(Component)]
struct Root;
#[derive(Component)]
struct Viewport;
#[derive(Component)]
struct Row {
    index: usize,
    short: bool,
}
#[derive(Component)]
struct ConfirmCaption;

pub(crate) fn native_choice(index: usize, list: Option<ListId>, short: bool) -> impl Bundle {
    (Action {kind:Kind::Choice(index),list}, Row {index,short})
}
pub(crate) fn native_skip(list: Option<ListId>) -> impl Bundle {Action {kind:Kind::Skip,list}}
pub(crate) fn native_confirm(list: Option<ListId>) -> impl Bundle {Action {kind:Kind::Confirm,list}}
pub(crate) fn native_barter(list: Option<ListId>) -> impl Bundle {Action {kind:Kind::Barter,list}}
pub(crate) fn native_viewport() -> impl Bundle {(Viewport,ScrollPosition::default())}
#[derive(Resource, Default)]
struct RevealRequest(bool);
#[derive(Clone, PartialEq)]
struct Guard {
    window: Entity,
    list: Option<ListId>,
    settings: TouchDialogue,
    size: Vec2,
    density: f32,
    device: crate::bindings::InputDevice,
    split: crate::dialogue_split::Frame,
    side: crate::dialogue_split::Side,
    safe: crate::touch_safe_area::SafeInsets,
    hints: bool,
    theme: appearance::Theme,
    device_publication: Option<crate::dialogue_device::Publication>,
    barter: Option<classic_state::BarterTarget>,
    normal: bool,
}
struct Gesture {
    id: u64,
    origin: Vec2,
    last: Vec2,
    target: Option<Kind>,
    scrolling: bool,
    viewport: Option<Entity>,
    selection: usize,
}
#[derive(Resource)]
struct Gestures {
    guard: Option<Guard>,
    gesture: Option<Gesture>,
    contacts: HashSet<u64>,
    armed: Option<usize>,
    focused: bool,
    running: bool,
}
impl Default for Gestures {
    fn default() -> Self {
        Self {
            guard: None,
            gesture: None,
            contacts: HashSet::new(),
            armed: None,
            focused: true,
            running: true,
        }
    }
}
pub(crate) fn available(
    state: Option<&State<crate::PlayState>>,
    paused: Option<&crate::pause::Paused>,
    script: Option<&crate::script::ScriptState>,
    creator: Option<&crate::creator::CreatorMenu>,
) -> bool {
    state.is_some_and(|s| *s.get() == crate::PlayState::Dialogue)
        && !paused.is_some_and(|p| p.0)
        && !creator.is_some_and(|c| c.is_open())
        && !script
            .and_then(|s| s.rt.as_ref())
            .is_some_and(|rt| crate::dialogue::held(&rt.game))
}
type TargetData = (
    &'static Action,
    &'static ComputedNode,
    &'static UiGlobalTransform,
    Option<&'static InheritedVisibility>,
    Option<&'static CalculatedClip>,
    Option<&'static ComputedUiTargetCamera>,
);
fn ui_point(
    point: Vec2,
    target: Option<&ComputedUiTargetCamera>,
    cameras: &Query<&Camera>,
) -> Vec2 {
    point
        - target
            .and_then(|t| t.get())
            .and_then(|c| cameras.get(c).ok())
            .and_then(Camera::physical_viewport_rect)
            .map_or(Vec2::ZERO, |r| r.min.as_vec2())
}
fn hit(
    node: &ComputedNode,
    transform: UiGlobalTransform,
    visible: Option<&InheritedVisibility>,
    clip: Option<&CalculatedClip>,
    point: Vec2,
) -> bool {
    visible.is_none_or(|v| v.get())
        && node.contains_point(transform, point)
        && clip.is_none_or(|c| c.contains_point(point))
}
fn clamp_scroll(node: &ComputedNode, position: &mut ScrollPosition) {
    position.y = position.y.clamp(
        0.0,
        ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.0),
    );
}
type InputGates<'w> = (
    Option<Res<'w, crate::bindings::Bindings>>,
    Option<Res<'w, crate::pad::TouchDeviceSwitch>>,
    Option<Res<'w, crate::pad::PlatformInputGate>>,
    Option<Res<'w, crate::ui_pointer::TouchUiCapture>>,
    Option<Res<'w, crate::touch_ui_options::Retirements>>,
);
type Availability<'w> = (Option<Res<'w,State<crate::PlayState>>>, Option<Res<'w,crate::pause::Paused>>, Option<Res<'w,crate::script::ScriptState>>, Option<Res<'w,crate::creator::CreatorMenu>>);
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn read(
    settings: Res<TouchDialogue>,
    menu: Res<DialogueMenu>,
    (bindings, touch_switch, platform, touch_owner, retirements): InputGates<'_>,
    (state, paused, script, creator): Availability<'_>,
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
    targets: Query<(Entity, TargetData)>,
    pointers: Query<(Entity, Ref<PointerState>)>,
    mut viewport: Query<
        (
            Entity,
            &ComputedNode,
            &UiGlobalTransform,
            &mut ScrollPosition,
            Option<&ComputedUiTargetCamera>,
        ),
        With<Viewport>,
    >,
    mut touches: MessageReader<TouchInput>,
    mut lifecycle: (MessageReader<WindowFocused>, MessageReader<AppLifecycle>),
    mut gestures: ResMut<Gestures>,
    mut frame: ResMut<TouchFrame>,
    cameras: Query<&Camera>,
    device: (Option<Res<crate::dialogue_device::Request>>, Option<Res<crate::dialogue_device::DeviceView>>,
        Option<Res<device_canvas::ArtCache>>),
    mouse: crate::pad::ActionButtons<MouseButton>,
    (split, split_options, safe, appearance, classic_surface, classic_state): (
        Res<crate::dialogue_split::Frame>, Res<crate::dialogue_split::Options>,
        Res<crate::touch_safe_area::SafeInsets>, Res<appearance::Appearance>,
        Option<Res<classic_presenter::Surface>>, Option<Res<classic_state::State>>),
) {
    *frame = TouchFrame::default();
    let Ok((window_id, window)) = windows.single() else {
        gestures.gesture = None;
        gestures.armed = None;
        gestures.contacts.clear();
        touches.clear();
        return;
    };
    let mut interrupted = false;
    for event in lifecycle.0.read().filter(|e| e.window == window_id) {
        gestures.focused = event.focused;
        interrupted = true;
    }
    for event in lifecycle.1.read() {
        match event {
            AppLifecycle::Suspended => {
                gestures.running = false;
                interrupted = true;
            }
            AppLifecycle::Running => {
                gestures.running = true;
                interrupted = true;
            }
            _ => {}
        }
    }
    let expected_device = match appearance.theme {
        appearance::Theme::RetroConsole=>Some(crate::dialogue_device::DeviceKind::Terminal),
        appearance::Theme::WristTerminal=>Some(crate::dialogue_device::DeviceKind::PipBoy),_=>None,
    };
    let requested_device = expected_device.is_some();
    let projection = device.0.as_deref().zip(device.1.as_deref())
        .filter(|(request, view)| request.stamp.window == window_id
            && Some(request.stamp.kind)==expected_device && view.matches(request)
            && device_canvas::usable(request,view,&menu,&settings,device.2.as_deref()));
    let on_device=requested_device && projection.is_some();
    let classic_ready=classic_presenter::ready_for(window_id,classic_surface.as_deref(),&split);
    let normal=native::uses_normal(appearance.theme,projection.is_some(),classic_ready);
    let enabled = settings.enabled
        && available(
            state.as_deref(),
            paused.as_deref(),
            script.as_deref(),
            creator.as_deref(),
        )
        && menu.view.is_some();
    frame.active = settings.enabled;
    let device = bindings
        .as_deref()
        .map_or(crate::bindings::InputDevice::KeyboardMouse, |b| b.device);
    let guard = Guard {
        window: window_id,
        list: menu.list.clone(),
        settings: *settings,
        size: Vec2::new(window.width(), window.height()),
        density: window.scale_factor(),
        device,
        split: *split,
        side: split_options.side,
        safe: *safe,
        hints: appearance.hints,
        theme: appearance.theme,
        device_publication: projection.map(|(_,view)|view.publication()),
        barter: classic_state.as_deref().filter(|_|appearance.theme==appearance::Theme::ClassicGlass)
            .and_then(|state|state.barter.clone()),
        normal,
    };
    let fresh = gestures.guard.as_ref() != Some(&guard);
    gestures.guard = Some(guard);
    let retiring = retirements.as_ref().is_some_and(|r| r.barrier(crate::touch_ui_options::Feature::Dialogue));
    if retiring {
        gestures.gesture = None;
        gestures.contacts.clear();
        gestures.armed = None;
    }
    let mut retired_raw = false;
    let events: Vec<_> = touches
        .read()
        .filter(|e| e.window == window_id)
        .filter(|e| touch_owner.as_ref().is_none_or(|owner| owner.allows(e.id)))
        .inspect(|_| retired_raw |= retiring)
        .filter(|_| !retiring)
        .cloned()
        .collect();
    if gestures.gesture.as_ref().is_some_and(|g| touch_owner.as_ref().is_some_and(|owner| !owner.allows(g.id))) {
        gestures.gesture = None;
        gestures.armed = None;
    }
    gestures.contacts.retain(|id| touch_owner.as_ref().is_none_or(|owner| owner.allows(*id)));
    let input_blocked = touch_switch.as_deref().is_some_and(|s| !s.allows_actions())
        || platform.as_deref().is_some_and(|p| {
            !p.allows_actions() || events.iter().any(|event| !p.allows_touch(event.id))
        });
    if !enabled
        || !window.focused
        || !gestures.focused
        || !gestures.running
        || interrupted
        || fresh
        || input_blocked
    {
        gestures.gesture = None;
        gestures.armed = None;
        if interrupted || !window.focused || !gestures.focused || !gestures.running || input_blocked
        {
            gestures.contacts.clear();
        }
        if input_blocked {
            return;
        }
        for event in events {
            if event.phase == TouchPhase::Started {
                gestures.contacts.insert(event.id);
            } else if matches!(event.phase, TouchPhase::Ended | TouchPhase::Canceled) {
                gestures.contacts.remove(&event.id);
            }
        }
        return;
    }
    frame.asked.list = menu.list.clone();
    let density = window.scale_factor();
    let projected_point = |p: Vec2, camera: Option<&ComputedUiTargetCamera>| {
        if on_device {
            let (request, view) = projection?;
            (camera.and_then(|c| c.get()) == Some(request.camera))
                .then(|| view.canvas_at(request, p * density)).flatten()
        } else {
            Some(ui_point(p * density, camera, &cameras))
        }
    };
    let can_power_exit = menu.view.as_ref().is_some_and(|v| v.goodbye && !v.choices.is_empty());
    let target_at = |p: Vec2| {
        if on_device && can_power_exit {
            if let Some((request, view)) = projection {
                if view.power_button_at(request, p * density) {
                    return Some(Kind::PowerExit);
                }
            }
        }
        if let Some(found) = targets.iter().find_map(|(_, (a, n, t, v, c, camera))| {
            (a.list == menu.list && projected_point(p, camera).is_some_and(|point| hit(n, *t, v, c, point)))
                .then_some(a.kind)
        }) {
            return Some(found);
        }
        if on_device {
            if let Some((request, view)) = projection {
                if view.power_button_at(request, p * density) {
                    return Some(Kind::PowerExit);
                }
            }
        }
        None
    };
    // A discarded raw touch must not re-enter via its aggregate UiPointer press.
    // With no admitted raw touch, the independent physical pointer path remains.
    let mut touched = retired_raw;
    let mut drop_tail = false;
    for event in &events {
        touched = true;
        if !event.position.is_finite() {
            gestures.gesture = None;
            gestures.contacts.remove(&event.id);
            continue;
        }
        if device == crate::bindings::InputDevice::Controller {
            gestures.gesture = None;
            gestures.armed = None;
            match event.phase {
                TouchPhase::Started => {
                    gestures.contacts.insert(event.id);
                }
                TouchPhase::Ended | TouchPhase::Canceled => {
                    gestures.contacts.remove(&event.id);
                }
                TouchPhase::Moved => {}
            }
            continue;
        }
        match event.phase {
            TouchPhase::Started => {
                let fresh_contact = gestures.contacts.insert(event.id);
                if !fresh_contact || gestures.contacts.len() > 1 {
                    gestures.gesture = None;
                    continue;
                }
                let scroll_owner = viewport.iter().find_map(|(entity,n,t,_,camera)| {
                    projected_point(event.position,camera)
                        .is_some_and(|point|n.contains_point(*t,point)).then_some(entity)
                });
                gestures.gesture = Some(Gesture {
                    id: event.id,
                    origin: event.position,
                    last: event.position,
                    target: target_at(event.position),
                    scrolling: false,
                    viewport: scroll_owner,
                    selection: menu.selected,
                });
            }
            TouchPhase::Moved => {
                let Some(g) = gestures.gesture.as_mut().filter(|g| g.id == event.id) else {
                    continue;
                };
                let scroll_threshold = if g.target == Some(Kind::PowerExit) { 14.0 } else { 8.0 };
                if event.position.distance(g.origin) > scroll_threshold {
                    g.scrolling = true;
                }
                if g.scrolling {
                    if let Some((_,node,_,mut position,camera)) = g.viewport.and_then(|owner|viewport.get_mut(owner).ok()) {
                        if let Some((previous, current)) = projected_point(g.last, camera)
                            .zip(projected_point(event.position, camera)) {
                            position.y += (previous.y - current.y) * node.inverse_scale_factor();
                        }
                        clamp_scroll(node, &mut position);
                    }
                }
                g.last = event.position;
            }
            TouchPhase::Canceled => {
                drop_tail = gestures.gesture.as_ref().is_some_and(|g| g.scrolling);
                gestures.contacts.remove(&event.id);
                gestures.gesture = None;
                if drop_tail {
                    break;
                }
            }
            TouchPhase::Ended => {
                gestures.contacts.remove(&event.id);
                let Some(g) = gestures.gesture.take().filter(|g| g.id == event.id) else {
                    continue;
                };
                if g.scrolling {
                    // Computed row transforms still describe the pre-scroll layout.
                    // All raw messages were collected above, so dropping this tail
                    // cannot replay a queued tap on the following frame.
                    drop_tail = true;
                    break;
                }
                let slop = if g.target == Some(Kind::PowerExit) { 14.0 } else { 8.0 };
                if event.position.distance(g.origin) <= slop && g.target == target_at(event.position)
                {
                    if let Some(kind) = g.target {
                        if kind != Kind::Confirm
                            || (gestures.armed == Some(menu.selected)
                                && g.selection == menu.selected)
                        {
                            activate(kind, true, &settings, &menu, &mut frame);
                            if let Kind::Choice(i) = kind {
                                gestures.armed = Some(i);
                            }
                        }
                    }
                }
            }
        }
    }
    if drop_tail {
        gestures.gesture = None;
        gestures.contacts.clear();
        gestures.armed = None;
    }
    // Mouse feedback stays on the original press edge; touches use only the guarded release route.
    if !touched && gestures.gesture.is_none() && gestures.contacts.is_empty() {
        if on_device {
            if let Some(position) = window.physical_cursor_position().map(|p| p / density) {
                if let Some(kind) = target_at(position) {
                    if mouse.physical_just_pressed(MouseButton::Left) {
                        activate(kind, false, &settings, &menu, &mut frame);
                    } else if let Kind::Choice(index) = kind {
                        frame.asked.hover = Some(index);
                    }
                }
            }
            return;
        }
        for (entity, pointer) in &pointers {
            if !pointer.is_changed() {
                continue;
            }
            let Ok((_, (action, _, _, _, _, _))) = targets.get(entity) else {
                continue;
            };
            if action.list != menu.list {
                continue;
            }
            match *pointer {
                PointerState::Pressed => {
                    activate(action.kind, false, &settings, &menu, &mut frame)
                }
                PointerState::Hovered => {
                    if let Kind::Choice(i) = action.kind {
                        frame.asked.hover = Some(i);
                    }
                }
                PointerState::None => {}
            }
        }
    }
}
fn activate(
    kind: Kind,
    touch: bool,
    settings: &TouchDialogue,
    menu: &DialogueMenu,
    frame: &mut TouchFrame,
) {
    let asked=&mut frame.asked;
    if asked.pick.is_some() || asked.skip || asked.leave || frame.barter {
        return;
    }
    match kind {
        Kind::Choice(i) if menu.view.as_ref().is_some_and(|v| i < v.choices.len()) => {
            asked.hover = Some(i);
            if !touch || !settings.confirm {
                asked.pick = Some(i);
            }
        }
        Kind::Confirm
            if menu
                .view
                .as_ref()
                .is_some_and(|v| menu.selected < v.choices.len()) =>
        {
            asked.pick = Some(menu.selected)
        }
        Kind::Skip => asked.skip = true,
        Kind::Barter => frame.barter = true,
        Kind::PowerExit
            if menu
                .view
                .as_ref()
                .is_some_and(|v| v.goodbye && !v.choices.is_empty()) =>
        {
            asked.leave = true;
        }
        _ => {}
    }
}

#[derive(Clone, PartialEq)]
struct DrawKey {
    view: crate::dialogue_menu::MenuView,
    list: Option<ListId>,
    settings: TouchDialogue,
    layout: TouchLayout,
    split: crate::dialogue_split::Frame,
    ui_scale: f32,
}
fn row_colors(short: bool, selected: bool, appearance: &appearance::Appearance) -> (Color, Color) {
    appearance.response_colors(short,selected)
}
fn text(p: &mut ChildSpawnerCommands, value: impl Into<String>, size: f32, paint: appearance::Paint, appearance: &appearance::Appearance) {
    p.spawn((
        paint,
        Text::new(value.into()),
        TextShadow {offset:Vec2::splat(1.5),color:Color::srgba(0.,0.,0.,0.85)},
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(crate::interface::tint(crate::interface::hud_rgb(),255.)),
        TextLayout::linebreak(bevy::text::LineBreak::WordOrCharacter),
        Node {
            width: percent(100.0),
            display: if matches!(paint,appearance::Paint::Hint) && !appearance.hints {Display::None}else{Display::Flex},
            ..default()
        },
    ));
}
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn draw(
    mut commands: Commands,
    menu: Res<DialogueMenu>,
    settings: Res<TouchDialogue>,
    layout: Res<TouchLayout>,
    roots: Query<Entity, With<Root>>,
    mut rows: Query<(&Row, &mut BorderColor, &mut BackgroundColor), With<appearance::Paint>>,
    mut shown: Local<Option<DrawKey>>,
    state: Option<Res<State<crate::PlayState>>>,
    paused: Option<Res<crate::pause::Paused>>,
    script: Option<Res<crate::script::ScriptState>>,
    creator: Option<Res<crate::creator::CreatorMenu>>,
    gestures: Res<Gestures>,
    mut captions: Query<&mut Text, With<ConfirmCaption>>,
    mut reveal: ResMut<RevealRequest>,
    (split, ui_scale, appearance): (Res<crate::dialogue_split::Frame>, Option<Res<UiScale>>, Res<appearance::Appearance>),
) {
    let confirm_label = if gestures.armed == Some(menu.selected) {
        "Confirm selection"
    } else {
        "Select a choice first"
    };
    let key = menu
        .view
        .as_ref()
        .filter(|_| {
            settings.enabled
                && appearance.theme == appearance::Theme::SplitPanel
                && available(
                    state.as_deref(),
                    paused.as_deref(),
                    script.as_deref(),
                    creator.as_deref(),
                )
        })
        .map(|v| DrawKey {
            view: v.clone(),
            list: menu.list.clone(),
            settings: *settings,
            layout: *layout,
            split: *split,
            ui_scale: ui_scale.as_deref().map_or(1.0,|s|s.0),
        });
    if key != *shown {
        reveal.0 = true;
        for root in &roots {
            commands.entity(root).despawn();
        }
        if let Some(key) = &key {
            let v = &key.view;
            let mut panel = Node {
                position_type: PositionType::Absolute,
                left: percent(2.0),
                width: percent(96.0),
                bottom: px(12.0),
                height: percent(layout.height),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(layout.padding)),
                row_gap: px(layout.gap),
                ..default()
            };
            if let Some(rect) = key.split.panel {
                let scale = if key.ui_scale.is_finite() && key.ui_scale>0.0 {key.ui_scale}else{1.0};
                panel.left=px(rect.min.x/scale);
                panel.top=px(rect.min.y/scale);
                panel.bottom=Val::Auto;
                panel.width=px(rect.width()/scale);
                panel.height=px(rect.height()/scale);
            }
            let mut root = commands.spawn((
                    Root,
                    appearance::Paint::Panel,
                    panel,
                    GlobalZIndex(2400),
                    BackgroundColor(Color::NONE),
                    BorderColor::all(Color::NONE),
                ));
            if let Some(camera)=key.split.ui_camera { root.insert(UiTargetCamera(camera)); }
            root.with_children(|p| {
                    appearance::decoration(p);
                    text(p, &v.speaker, 18.0, appearance::Paint::Speaker, &appearance);
                    text(
                        p,
                        if v.choices.is_empty() {
                            "Swipe to read. Tap and release to continue."
                        } else if settings.confirm {
                            "Swipe to scroll. Tap a choice, then Confirm selection."
                        } else {
                            "Swipe to scroll. Tap and release a choice to answer."
                        },
                        14.0,
                        appearance::Paint::Hint,
                        &appearance,
                    );
                    p.spawn((
                        Viewport,
                        Node {
                            width: percent(100.0),
                            flex_direction: FlexDirection::Column,
                            flex_grow: 1.0,
                            min_height: px(0.0),
                            overflow: Overflow::scroll_y(),
                            ..default()
                        },
                        ScrollPosition::default(),
                    ))
                    .with_children(|p| {
                        p.spawn(Node {
                            width: percent(100.0),
                            flex_direction: FlexDirection::Column,
                            flex_shrink: 0.0,
                            row_gap: px(layout.gap),
                            ..default()
                        })
                        .with_children(|p| {
                            if let Some(line) = &v.line {
                                p.spawn((
                                    Button,
                                    Action {
                                        kind: Kind::Skip,
                                        list: menu.list.clone(),
                                    },
                                    Node {
                                        width: percent(100.0),
                                        min_height: px(56.0),
                                        padding: UiRect::all(px(10.0)),
                                        flex_shrink: 0.0,
                                        ..default()
                                    },
                                ))
                                .with_children(|p| {
                                    text(
                                        p,
                                        line,
                                        settings.subtitle_size(),
                                        appearance::Paint::Speech,
                                        &appearance,
                                    )
                                });
                            }
                            for (index, row) in v.choices.iter().enumerate() {
                                p.spawn((
                                    Button,
                                    Action {
                                        kind: Kind::Choice(index),
                                        list: menu.list.clone(),
                                    },
                                    appearance::Paint::Choice {index},
                                    Row {
                                        index,
                                        short: row.short,
                                    },
                                    Node {
                                        width: percent(100.0),
                                        min_height: px(layout.row_min),
                                        padding: UiRect {left:px(30.),right:px(layout.row_padding),top:px(layout.row_padding),bottom:px(layout.row_padding)},
                                        border: UiRect::ZERO,
                                        flex_shrink: 0.0,
                                        ..default()
                                    },
                                    BorderColor::all(
                                        row_colors(row.short, index == menu.selected, &appearance).0,
                                    ),
                                    BackgroundColor(
                                        row_colors(row.short, index == menu.selected, &appearance).1,
                                    ),
                                ))
                                .with_children(|p| {
                                    appearance::response_mark(p,index);
                                    p.spawn((appearance::Paint::Focus{index,short:row.short},bevy::picking::Pickable::IGNORE,
                                        Node{position_type:PositionType::Absolute,left:px(3.),top:percent(50.),width:px(2.),height:px(8.),..default()},BackgroundColor(Color::NONE)));
                                    text(
                                        p,
                                        &row.label,
                                        settings.choice_size(),
                                        appearance::Paint::ChoiceText {darkened:row.darkened},
                                        &appearance,
                                    );
                                });
                            }
                        });
                    });
                    if settings.confirm && !v.choices.is_empty() {
                        p.spawn((
                            Button,
                            Action {
                                kind: Kind::Confirm,
                                list: menu.list.clone(),
                            },
                            Node {
                                width: percent(100.0),
                                min_height: px(layout.row_min),
                                padding: UiRect::all(px(layout.confirm_padding)),
                                border: UiRect::ZERO,
                                flex_shrink: 0.0,
                                ..default()
                            },
                            appearance::Paint::Confirm,
                            BorderColor::all(Color::NONE),
                            BackgroundColor(Color::NONE),
                        ))
                        .with_children(|p| {
                            p.spawn((
                                ConfirmCaption,
                                appearance::Paint::Caption,
                                Text::new(confirm_label),
                                TextShadow {offset:Vec2::splat(1.5),color:Color::srgba(0.,0.,0.,0.85)},
                                TextFont {
                                    font_size: FontSize::Px(layout.confirm_text),
                                    ..default()
                                },
                                TextColor(crate::interface::tint(crate::interface::hud_rgb(),255.)),
                            ));
                        });
                    }
                });
        }
        *shown = key;
    }
    for (row, mut border, mut background) in &mut rows {
        let (line, fill) = row_colors(row.short, row.index == menu.selected, &appearance);
        *border = BorderColor::all(line);
        background.0 = fill;
    }
    for mut caption in &mut captions {
        if caption.0 != confirm_label {
            caption.0 = confirm_label.into();
        }
    }
}
fn reveal_selected(
    menu: Res<DialogueMenu>,
    mut previous: Local<Option<(Option<ListId>, usize)>>,
    mut reveal: ResMut<RevealRequest>,
    rows: Query<(Entity, &Row, &ComputedNode, &UiGlobalTransform)>,
    parents: Query<&ChildOf>,
    mut viewport: Query<(&ComputedNode, &UiGlobalTransform, &mut ScrollPosition), With<Viewport>>,
) {
    let key = (menu.list.clone(), menu.selected);
    if !reveal.0 && previous.as_ref() == Some(&key) {
        return;
    }
    let Some((entity, _, row, transform)) = rows.iter()
        .find(|(_, row, node, _)| row.index == menu.selected && !node.is_empty())
    else {
        return;
    };
    let mut ancestor = entity;
    // The owned tree is shallow; never walk an unbounded foreign hierarchy.
    for _ in 0..256 {
        let Ok(parent) = parents.get(ancestor) else { break; };
        ancestor = parent.parent();
        let Ok((view, t, mut scroll)) = viewport.get_mut(ancestor) else { continue; };
        let top = t.transform_point2(Vec2::ZERO).y - view.size().y * 0.5;
        let bottom = top + view.size().y;
        let row_top = transform.transform_point2(Vec2::ZERO).y - row.size().y * 0.5;
        let delta = if row_top < top {
            row_top - top
        } else {
            (row_top + row.size().y - bottom).max(0.0)
        };
        scroll.y += delta * view.inverse_scale_factor();
        clamp_scroll(view, &mut scroll);
        *previous = Some(key);
        reveal.0 = false;
        return;
    }
}

#[cfg(feature = "test-fixtures")]
pub mod model_capture;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogue_menu::{ChoiceRow, MenuView};
    fn scene(confirm: bool) -> (App, Entity, Entity) {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::input::InputPlugin,
            bevy::state::app::StatesPlugin,
        ))
        .insert_state(crate::PlayState::Dialogue)
        .init_resource::<crate::bindings::Bindings>()
        .insert_resource(TouchDialogue {
            enabled: true,
            confirm,
            ..default()
        })
        .init_resource::<DialogueMenu>();
        install(&mut app);
        let window = app
            .world_mut()
            .spawn((
                Window {
                    resolution: bevy::window::WindowResolution::new(600, 600),
                    ..default()
                },
                PrimaryWindow,
            ))
            .id();
        let list = Some(ListId::for_test(1, &[10, 20, 30]));
        {
            let mut menu = app.world_mut().resource_mut::<DialogueMenu>();
            menu.view = Some(MenuView {
                speaker: "Speaker".into(),
                choices: vec![
                    ChoiceRow {
                        label: "First".into(),
                        ..default()
                    },
                    ChoiceRow {
                        label: "Second".into(),
                        short: true,
                        ..default()
                    },
                    ChoiceRow {
                        label: "Third".into(),
                        darkened: true,
                        ..default()
                    },
                ],
                ..default()
            });
            menu.list = list.clone();
        }
        for (kind, y) in [
            (Kind::Choice(2), 100.0),
            (Kind::Confirm, 200.0),
            (Kind::Skip, 300.0),
        ] {
            app.world_mut().spawn((
                Action {
                    kind,
                    list: list.clone(),
                },
                ComputedNode {
                    size: Vec2::new(300.0, 56.0),
                    ..default()
                },
                UiGlobalTransform::from_xy(200.0, y),
                PointerState::None,
            ));
        }
        let viewport = app
            .world_mut()
            .spawn((
                Viewport,
                ComputedNode {
                    size: Vec2::splat(400.0),
                    content_size: Vec2::new(400.0, 1000.0),
                    ..default()
                },
                UiGlobalTransform::from_xy(200.0, 200.0),
                ScrollPosition::default(),
            ))
            .id();
        app.update();
        app.update();
        (app, window, viewport)
    }
    fn event(app: &mut App, window: Entity, id: u64, phase: TouchPhase, x: f32, y: f32) {
        app.world_mut().write_message(TouchInput {
            window,
            id,
            phase,
            position: Vec2::new(x, y),
            force: None,
        });
    }
    fn tap(app: &mut App, window: Entity, id: u64, y: f32) {
        event(app, window, id, TouchPhase::Started, 200.0, y);
        event(app, window, id, TouchPhase::Ended, 200.0, y);
        app.update();
    }
    fn asked(app: &App) -> ViewInput {
        app.world().resource::<TouchFrame>().asked.clone()
    }
    #[test]
    fn direct_touch_chooses_on_release_and_keeps_the_offered_index() {
        let (mut app, w, _) = scene(false);
        event(&mut app, w, 1, TouchPhase::Started, 200.0, 100.0);
        app.update();
        assert_eq!(asked(&app).pick, None);
        event(&mut app, w, 1, TouchPhase::Ended, 200.0, 100.0);
        app.update();
        assert_eq!((asked(&app).hover, asked(&app).pick), (Some(2), Some(2)));
        let list = app.world().resource::<DialogueMenu>().list.clone();
        assert_eq!(asked(&app).for_list(list.as_ref()).pick, Some(2));
        assert_eq!(
            asked(&app)
                .for_list(Some(&ListId::for_test(2, &[10, 20, 30])))
                .pick,
            None,
            "runtime stamp gate still rejects replacement lists"
        );
    }
    #[test]
    fn select_then_confirm_needs_a_separate_released_confirmation() {
        let (mut app, w, _) = scene(true);
        tap(&mut app, w, 9, 200.0);
        assert_eq!(asked(&app).pick, None, "touch confirmation starts unarmed");
        tap(&mut app, w, 1, 100.0);
        assert_eq!((asked(&app).hover, asked(&app).pick), (Some(2), None));
        app.world_mut().resource_mut::<DialogueMenu>().selected = 2;
        tap(&mut app, w, 2, 200.0);
        assert_eq!(asked(&app).pick, Some(2));
    }
    #[test]
    fn confirmation_cannot_follow_a_keyboard_selection_change_while_held() {
        let (mut app, w, _) = scene(true);
        tap(&mut app, w, 1, 100.0);
        app.world_mut().resource_mut::<DialogueMenu>().selected = 2;
        event(&mut app, w, 2, TouchPhase::Started, 200.0, 200.0);
        app.update();
        app.world_mut().resource_mut::<DialogueMenu>().selected = 0;
        event(&mut app, w, 2, TouchPhase::Ended, 200.0, 200.0);
        app.update();
        assert_eq!(asked(&app).pick, None);
    }
    #[test]
    fn pause_and_dialogue_entry_do_not_reuse_a_held_contact() {
        let (mut app, w, _) = scene(false);
        event(&mut app, w, 1, TouchPhase::Started, 200.0, 100.0);
        app.update();
        app.insert_resource(crate::pause::Paused(true));
        app.update();
        app.world_mut().resource_mut::<crate::pause::Paused>().0 = false;
        event(&mut app, w, 1, TouchPhase::Ended, 200.0, 100.0);
        app.update();
        assert_eq!(asked(&app).pick, None);
        app.insert_state(crate::PlayState::Playing);
        event(&mut app, w, 2, TouchPhase::Started, 200.0, 100.0);
        app.update();
        app.insert_state(crate::PlayState::Dialogue);
        event(&mut app, w, 2, TouchPhase::Ended, 200.0, 100.0);
        app.update();
        assert_eq!(asked(&app).pick, None);
        app.update(); // Resolve publishes the reopened overlay before admitting a fresh touch.
        tap(&mut app, w, 3, 100.0);
        assert_eq!(asked(&app).pick, Some(2));
    }
    #[test]
    fn speaking_line_skips_only_with_a_fresh_released_tap() {
        let (mut app, w, _) = scene(false);
        {
            let mut menu = app.world_mut().resource_mut::<DialogueMenu>();
            let v = menu.view.as_mut().unwrap();
            v.choices.clear();
            v.line = Some("An offered response".into());
        }
        app.update();
        tap(&mut app, w, 1, 300.0);
        assert!(asked(&app).skip);
        event(&mut app, w, 2, TouchPhase::Started, 200.0, 300.0);
        app.update();
        app.world_mut().resource_mut::<DialogueMenu>().list = Some(ListId::for_test(9, &[]));
        event(&mut app, w, 2, TouchPhase::Ended, 200.0, 300.0);
        app.update();
        assert!(!asked(&app).skip);
    }
    #[test]
    fn keyboard_selection_reveals_a_row_once_without_fighting_manual_scroll() {
        let (mut app, _, viewport) = scene(false);
        app.world_mut().spawn((
            Row {
                index: 2,
                short: false,
            },
            ComputedNode {
                size: Vec2::new(300.0, 56.0),
                ..default()
            },
            UiGlobalTransform::from_xy(200.0, 650.0),
            ChildOf(viewport),
        ));
        app.world_mut().resource_mut::<DialogueMenu>().selected = 2;
        app.update();
        assert!(app.world().get::<ScrollPosition>(viewport).unwrap().y > 0.0);
        app.world_mut()
            .get_mut::<ScrollPosition>(viewport)
            .unwrap()
            .y = 40.0;
        app.update();
        assert_eq!(app.world().get::<ScrollPosition>(viewport).unwrap().y, 40.0);
        assert_eq!(app.world().resource::<DialogueMenu>().selected, 2);
    }
    #[test]
    fn swiping_scrolls_without_answering_even_after_returning_to_the_row() {
        let (mut app, w, viewport) = scene(false);
        event(&mut app, w, 1, TouchPhase::Started, 200.0, 100.0);
        event(&mut app, w, 1, TouchPhase::Moved, 200.0, 30.0);
        app.update();
        assert!(app.world().get::<ScrollPosition>(viewport).unwrap().y > 0.0);
        event(&mut app, w, 1, TouchPhase::Moved, 200.0, 100.0);
        event(&mut app, w, 1, TouchPhase::Ended, 200.0, 100.0);
        app.update();
        assert_eq!((asked(&app).hover, asked(&app).pick), (None, None));
    }
    #[test]
    fn canceled_and_multifinger_contacts_never_answer() {
        let (mut app, w, _) = scene(false);
        event(&mut app, w, 1, TouchPhase::Started, 200.0, 100.0);
        event(&mut app, w, 2, TouchPhase::Started, 200.0, 100.0);
        event(&mut app, w, 1, TouchPhase::Ended, 200.0, 100.0);
        event(&mut app, w, 3, TouchPhase::Started, 200.0, 100.0);
        event(&mut app, w, 2, TouchPhase::Ended, 200.0, 100.0);
        event(&mut app, w, 3, TouchPhase::Ended, 200.0, 100.0);
        app.update();
        assert_eq!(asked(&app).pick, None);
        event(&mut app, w, 4, TouchPhase::Started, 200.0, 100.0);
        event(&mut app, w, 4, TouchPhase::Canceled, 200.0, 100.0);
        app.update();
        assert_eq!(asked(&app).pick, None);
        tap(&mut app, w, 5, 100.0);
        assert_eq!(asked(&app).pick, Some(2));
    }
    #[test]
    fn entry_and_new_response_require_a_fresh_contact() {
        let (mut app, w, _) = scene(false);
        event(&mut app, w, 1, TouchPhase::Started, 200.0, 100.0);
        app.update();
        app.world_mut().resource_mut::<DialogueMenu>().list =
            Some(ListId::for_test(2, &[10, 20, 30]));
        event(&mut app, w, 1, TouchPhase::Ended, 200.0, 100.0);
        app.update();
        assert_eq!(asked(&app).pick, None);
        // The old view's targets remain stamped for the old list until drawing replaces them.
        tap(&mut app, w, 2, 100.0);
        assert_eq!(asked(&app).pick, None);
        let list = app.world().resource::<DialogueMenu>().list.clone();
        let mut q = app.world_mut().query::<&mut Action>();
        for mut a in q.iter_mut(app.world_mut()) {
            a.list = list.clone();
        }
        tap(&mut app, w, 3, 100.0);
        assert_eq!(asked(&app).pick, Some(2));
    }
    #[test]
    fn a_release_on_another_target_or_outside_does_not_answer() {
        let (mut app, w, _) = scene(false);
        for (id, x, y) in [(1, 200.0, 200.0), (2, 590.0, 100.0)] {
            event(&mut app, w, id, TouchPhase::Started, 200.0, 100.0);
            event(&mut app, w, id, TouchPhase::Ended, x, y);
            app.update();
            assert_eq!(asked(&app).pick, None);
        }
    }
    #[test]
    fn focus_suspend_resize_and_mode_changes_cancel_pending_releases() {
        for cause in 0..4 {
            let (mut app, w, _) = scene(false);
            event(&mut app, w, 1, TouchPhase::Started, 200.0, 100.0);
            app.update();
            match cause {
                0 => {
                    app.world_mut().write_message(WindowFocused {
                        window: w,
                        focused: false,
                    });
                    app.world_mut().write_message(WindowFocused {
                        window: w,
                        focused: true,
                    });
                }
                1 => {
                    app.world_mut().write_message(AppLifecycle::Suspended);
                    app.world_mut().write_message(AppLifecycle::Running);
                }
                2 => app
                    .world_mut()
                    .get_mut::<Window>(w)
                    .unwrap()
                    .resolution
                    .set(700.0, 600.0),
                _ => app.world_mut().resource_mut::<TouchDialogue>().choice_scale = 1.5,
            }
            event(&mut app, w, 1, TouchPhase::Ended, 200.0, 100.0);
            app.update();
            assert_eq!(asked(&app).pick, None, "cause {cause}");
            tap(&mut app, w, 2, 100.0);
            assert_eq!(asked(&app).pick, Some(2), "fresh contact after {cause}");
        }
    }
    #[test]
    fn controller_ownership_and_first_returning_touch_cannot_answer() {
        let (mut app, w, _) = scene(false);
        app.world_mut()
            .resource_mut::<crate::bindings::Bindings>()
            .device = crate::bindings::InputDevice::Controller;
        app.update();
        tap(&mut app, w, 1, 100.0);
        assert_eq!(asked(&app).pick, None);
        app.world_mut()
            .resource_mut::<crate::bindings::Bindings>()
            .device = crate::bindings::InputDevice::KeyboardMouse;
        tap(&mut app, w, 2, 100.0);
        assert_eq!(asked(&app).pick, None);
        tap(&mut app, w, 3, 100.0);
        assert_eq!(asked(&app).pick, Some(2));
    }
    #[test]
    fn clipping_and_physical_density_control_actual_hit_testing() {
        let (mut app, w, _) = scene(false);
        app.world_mut()
            .get_mut::<Window>(w)
            .unwrap()
            .resolution
            .set_scale_factor_override(Some(2.0));
        app.update();
        event(&mut app, w, 1, TouchPhase::Started, 100.0, 50.0);
        event(&mut app, w, 1, TouchPhase::Ended, 100.0, 50.0);
        app.update();
        assert_eq!(
            asked(&app).pick,
            Some(2),
            "logical point becomes physical node coordinates once"
        );
        let mut q = app.world_mut().query::<Entity>();
        let ids: Vec<_> = q
            .iter(app.world())
            .filter(|e| {
                app.world()
                    .get::<Action>(*e)
                    .is_some_and(|a| a.kind == Kind::Choice(2))
            })
            .collect();
        for e in ids {
            app.world_mut()
                .entity_mut(e)
                .insert(CalculatedClip::FullyClipped);
        }
        event(&mut app, w, 2, TouchPhase::Started, 100.0, 50.0);
        event(&mut app, w, 2, TouchPhase::Ended, 100.0, 50.0);
        app.update();
        assert_eq!(asked(&app).pick, None);
    }
    #[test]
    fn real_mouse_press_keeps_direct_choice_even_in_confirm_touch_mode() {
        let (mut app, _, _) = scene(true);
        let mut q = app.world_mut().query::<(&Action, &mut PointerState)>();
        for (a, mut p) in q.iter_mut(app.world_mut()) {
            if a.kind == Kind::Choice(2) {
                *p = PointerState::Pressed;
            }
        }
        app.update();
        assert_eq!(asked(&app).pick, Some(2));
    }
    #[test]
    fn controller_pointer_feedback_keeps_the_existing_choice_route() {
        let (mut app, _, _) = scene(true);
        app.world_mut()
            .resource_mut::<crate::bindings::Bindings>()
            .device = crate::bindings::InputDevice::Controller;
        app.update();
        let mut q = app.world_mut().query::<(&Action, &mut PointerState)>();
        for (a, mut pointer) in q.iter_mut(app.world_mut()) {
            if a.kind == Kind::Choice(2) {
                *pointer = PointerState::Pressed;
            }
        }
        app.update();
        assert_eq!(asked(&app).pick, Some(2));
    }
    #[test]
    fn renderer_keeps_full_choice_text_order_and_independent_sizes() {
        let (mut app, _, _) = scene(true);
        app.world_mut().resource_mut::<appearance::Appearance>().theme = appearance::Theme::SplitPanel;
        let long =
            "A long offered choice that must remain complete and wrap within the full-width row. "
                .repeat(6);
        app.world_mut()
            .resource_mut::<DialogueMenu>()
            .view
            .as_mut()
            .unwrap()
            .choices[1]
            .label = long.clone();
        app.world_mut().resource_mut::<TouchDialogue>().choice_scale = 1.5;
        app.update();
        let mut q = app.world_mut().query::<(&Text, &TextFont, &TextLayout)>();
        assert!(q.iter(app.world()).any(|(t, f, l)| t.0 == long
            && f.font_size == FontSize::Px(30.0)
            && l.linebreak == bevy::text::LineBreak::WordOrCharacter));
        let mut q = app.world_mut().query::<(&Row, &Node)>();
        let mut rows: Vec<_> = q
            .iter(app.world())
            .map(|(r, n)| (r.index, n.width, n.min_height))
            .collect();
        rows.sort_by_key(|r| r.0);
        assert_eq!(rows.len(), 3);
        assert!(rows
            .iter()
            .enumerate()
            .all(|(i, r)| r.0 == i && r.1 == percent(100.0) && r.2 == px(if cfg!(target_os = "android") { 48.0 } else { 56.0 })));
        {
            let mut menu = app.world_mut().resource_mut::<DialogueMenu>();
            let v = menu.view.as_mut().unwrap();
            v.choices.clear();
            v.line = Some("A speaking subtitle".into());
        }
        app.world_mut()
            .resource_mut::<TouchDialogue>()
            .subtitle_scale = 0.75;
        app.update();
        let mut q = app.world_mut().query::<(&Text, &TextFont)>();
        assert!(q
            .iter(app.world())
            .any(|(t, f)| t.0 == "A speaking subtitle" && f.font_size == FontSize::Px(15.0)));
    }
    #[test]
    fn settings_roundtrip_through_the_existing_ini_flow() {
        let path = std::env::temp_dir().join(format!(
            "nvf-dialogue-touch-{}-{}.ini",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut app = App::new();
        install(&mut app);
        app.insert_resource(crate::settings::SettingsFile(Some(path.clone())));
        crate::settings::apply_stored(app.world_mut());
        let index = |app: &App, key: &str| {
            app.world()
                .resource::<crate::settings::Settings>()
                .index(key)
                .unwrap()
        };
        let enabled = index(&app, "touch-dialogue");
        if !app.world().resource::<TouchDialogue>().enabled {
            assert_eq!(crate::settings::flip(app.world_mut(), enabled), Some(true));
        }
        let i = index(&app, "dialogue-choice-text-size");
        crate::settings::set_slider(app.world_mut(), i, 1.0);
        let i = index(&app, "dialogue-subtitle-text-size");
        crate::settings::set_slider(app.world_mut(), i, 0.0);
        let i = index(&app, "dialogue-touch-choice");
        assert!(crate::settings::pick(app.world_mut(), i, "Direct tap"));
        let before = *app.world().resource::<TouchDialogue>();
        let mut loaded = App::new();
        install(&mut loaded);
        loaded.insert_resource(crate::settings::SettingsFile(Some(path.clone())));
        crate::settings::apply_stored(loaded.world_mut());
        assert_eq!(*loaded.world().resource::<TouchDialogue>(), before);
        std::fs::remove_file(path).unwrap();
        for value in [f32::NAN, f32::INFINITY, -100.0, 100.0] {
            assert!((0.75..=2.0).contains(&from_part(value)));
        }
    }
    #[test]
    fn actual_device_selector_consumes_the_return_touch_before_dialogue_can_answer() {
        let (mut app, w, _) = scene(false);
        app.init_resource::<crate::pad::TouchDeviceSwitch>()
            .add_plugins(crate::pad::InputSwitchPlugin);
        app.world_mut()
            .resource_mut::<crate::bindings::Bindings>()
            .device = crate::bindings::InputDevice::Controller;
        app.update();
        tap(&mut app, w, 91, 100.0);
        assert_eq!(
            app.world().resource::<crate::bindings::Bindings>().device,
            crate::bindings::InputDevice::KeyboardMouse
        );
        assert!(!app
            .world()
            .resource::<crate::pad::TouchDeviceSwitch>()
            .allows_actions());
        assert_eq!(asked(&app).pick, None);
        tap(&mut app, w, 92, 100.0);
        assert_eq!(asked(&app).pick, Some(2));
    }
    #[test]
    fn platform_touch_quarantine_rejects_a_duplicate_start_and_release_after_loading() {
        let (mut app, w, _) = scene(false);
        app.init_resource::<crate::pad::PlatformInputGate>()
            .add_plugins(crate::pad::InputSwitchPlugin);
        app.world_mut()
            .resource_mut::<crate::pad::PlatformInputGate>()
            .set_blocked(true);
        event(&mut app, w, 81, TouchPhase::Started, 200.0, 100.0);
        app.update();
        assert_eq!(asked(&app).pick, None);
        app.world_mut()
            .resource_mut::<crate::pad::PlatformInputGate>()
            .set_blocked(false);
        event(&mut app, w, 81, TouchPhase::Started, 200.0, 100.0);
        event(&mut app, w, 81, TouchPhase::Ended, 200.0, 100.0);
        app.update();
        assert!(app
            .world()
            .resource::<crate::pad::PlatformInputGate>()
            .allows_actions());
        assert!(!app
            .world()
            .resource::<crate::pad::PlatformInputGate>()
            .allows_touch(81));
        assert_eq!(asked(&app).pick, None);
        app.update();
        assert_eq!(asked(&app).pick, None);
        tap(&mut app, w, 82, 100.0);
        assert_eq!(asked(&app).pick, Some(2));
    }
    #[test]
    fn a_resume_frame_cannot_answer_with_an_entire_new_queued_tap() {
        let (mut app, w, _) = scene(false);
        app.world_mut().write_message(AppLifecycle::Suspended);
        app.update();
        app.world_mut().write_message(AppLifecycle::Running);
        event(&mut app, w, 61, TouchPhase::Started, 200.0, 100.0);
        event(&mut app, w, 61, TouchPhase::Ended, 200.0, 100.0);
        app.update();
        assert_eq!(asked(&app).pick, None);
        tap(&mut app, w, 62, 100.0);
        assert_eq!(asked(&app).pick, Some(2));
    }
    #[test]
    fn scrolling_drains_a_same_batch_following_tap_and_never_replays_a_stale_row_pick() {
        for finish in [TouchPhase::Ended, TouchPhase::Canceled] {
            let (mut app, w, viewport) = scene(false);
            event(&mut app, w, 51, TouchPhase::Started, 200.0, 100.0);
            event(&mut app, w, 51, TouchPhase::Moved, 200.0, 40.0);
            event(&mut app, w, 51, finish, 200.0, 40.0);
            event(&mut app, w, 52, TouchPhase::Started, 200.0, 100.0);
            event(&mut app, w, 52, TouchPhase::Ended, 200.0, 100.0);
            app.update();
            assert!(app.world().get::<ScrollPosition>(viewport).unwrap().y > 0.0);
            assert_eq!(asked(&app).pick, None);
            app.update();
            assert_eq!(asked(&app).pick, None);
            // Publish the next layout's row under the same pixel without changing the list.
            let mut q = app.world_mut().query::<&mut Action>();
            for mut action in q.iter_mut(app.world_mut()) {
                if action.kind == Kind::Choice(2) {
                    action.kind = Kind::Choice(1);
                }
            }
            tap(&mut app, w, 53, 100.0);
            assert_eq!(asked(&app).pick, Some(1));
        }
    }
    include!("dialogue_touch_optional_tests.rs");
    include!("dialogue_touch/availability_tests.rs");
}

#[cfg(test)]
#[path = "dialogue_touch/compact_tests.rs"]
mod compact_tests;
