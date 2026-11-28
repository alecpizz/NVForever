//! Isolated teaching input: never synthesize gameplay input or menu requests.
//! NVForever design: docs/design/android-touch-practice.md.
use super::{TouchInput, TouchPhase};
use bevy::{ecs::system::SystemParam, input::keyboard::KeyboardInput, prelude::*};
use nvf_play::{
    pause::{FormOver, Page, Paused},
    settings::{self, ChoiceList, Setting},
};
use std::collections::HashMap;

const KEY: &str = "touch-practice-v1";
const OPEN: &str = "Open practice";
const CLOSED: &str = "Ready";
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum PageKind {
    #[default]
    Guide,
    GuideLook,
    GuideActions,
    Controls,
    Drawer,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Close,
    Reset,
    Begin,
    Move,
    Look,
    Fire,
    Aim,
    Jump,
    DrawerPage,
    Drawer,
}
#[derive(Clone, Copy)]
struct Contact {
    target: Target,
    start: Vec2,
    origin: Vec2,
    at: Vec2,
    canceled: bool,
}
#[derive(Resource, Default)]
pub(super) struct Practice {
    active: bool,
    opening: bool,
    closing: u8,
    page: PageKind,
    size: Option<Vec2>,
    viewport: Option<Vec2>,
    contacts: HashMap<u64, Contact>,
    move_delta: Vec2,
    look: Vec2,
    looked: bool,
    fire: bool,
    aim: bool,
    aimed: bool,
    jump: bool,
    drawer_done: bool,
    drawer_enabled: bool,
    drawer_distance: f32,
    toggle_aim: bool,
    dead_zone: f32,
    floating: bool,
    movement_right: bool,
    analog: bool,
    fire_drag: bool,
}
impl Practice {
    pub(super) fn blocking(&self) -> bool {
        self.active || self.closing > 0
    }
    fn clear_contacts(&mut self) {
        self.contacts.clear();
        self.move_delta = Vec2::ZERO;
        if !self.toggle_aim {
            self.aim = false;
        }
    }
    fn close(&mut self) {
        self.active = false;
        self.closing = 2;
        self.clear_contacts();
        self.aim = false;
    }
    fn reset(&mut self) {
        self.page = PageKind::Guide;
        self.clear_contacts();
        self.look = Vec2::ZERO;
        self.looked = false;
        self.fire = false;
        self.aim = false;
        self.aimed = false;
        self.jump = false;
        self.drawer_done = false;
    }
    fn stick_centre(&self, size: Vec2) -> Vec2 {
        self.contacts
            .values()
            .find(|c| c.target == Target::Move)
            .map_or(stick_on_side(size, self.movement_right).0, |c| c.origin)
    }
    fn speed_feedback(&self, radius: f32) -> String {
        if !self.analog { return "Legacy directional movement".into(); }
        let v = super::movement::axes(self.move_delta, radius, self.dead_zone);
        format!("Analog {:.0}%: {} with Always Run", v.length()*100.0,
            if v.length() < nvf_play::pad::STICK_WALK_BELOW { "walk" } else { "run" })
    }
    fn direction(&self, radius: f32) -> String {
        let d = if self.analog { super::movement::axes(self.move_delta, radius, self.dead_zone) * Vec2::new(1.0,-1.0) }
            else { self.move_delta / radius.max(1.0) };
        let threshold = if self.analog { 0.001 } else { self.dead_zone };
        let mut words = Vec::new();
        if d.y < -threshold {
            words.push("Forward");
        }
        if d.y > threshold {
            words.push("Back");
        }
        if d.x < -threshold {
            words.push("left");
        }
        if d.x > threshold {
            words.push("right");
        }
        if words.is_empty() {
            "In dead zone".into()
        } else {
            words.join(" ")
        }
    }
    // Returns true for a page change: the caller drops the remaining entry/closing batch.
    fn event(&mut self, t: &TouchInput, size: Vec2) -> bool {
        // Live toggle aim clears on every cancelled touch, even another role/id.
        if t.phase == TouchPhase::Canceled && self.toggle_aim {
            self.aim = false;
        }
        if !t.position.is_finite() {
            self.clear_contacts();
            return false;
        }
        match t.phase {
            TouchPhase::Started => {
                if self.contacts.len() >= 10 || self.contacts.contains_key(&t.id) {
                    return false;
                }
                if let Some(target) = target(
                    self.page,
                    size,
                    self.drawer_enabled,
                    self.floating,
                    self.movement_right,
                    t.position,
                ) {
                    if matches!(target, Target::Move | Target::Look | Target::Drawer)
                        && self.contacts.values().any(|c| c.target == target)
                    {
                        return false;
                    }
                    let origin = if target == Target::Move && !self.floating {
                        stick_on_side(size, self.movement_right).0
                    } else {
                        t.position
                    };
                    if target == Target::Fire && self.fire_drag { self.fire = true; }
                    if target == Target::Aim {
                        let already_down = self.contacts.values().any(|c| c.target == Target::Aim);
                        if self.toggle_aim {
                            if !already_down {
                                self.aim = !self.aim;
                            }
                        } else {
                            self.aim = true;
                        }
                        self.aimed = true;
                    }
                    self.contacts.insert(
                        t.id,
                        Contact {
                            target,
                            start: t.position,
                            origin,
                            at: t.position,
                            canceled: false,
                        },
                    );
                    if target == Target::Move {
                        self.move_delta = t.position - origin;
                    }
                }
            }
            TouchPhase::Moved => {
                if let Some(c) = self.contacts.get_mut(&t.id) {
                    if c.target == Target::Look || (c.target == Target::Fire && self.fire_drag) {
                        self.look = (self.look + t.position - c.at)
                            .clamp(Vec2::splat(-70.0), Vec2::splat(70.0));
                        self.looked |= (t.position - c.start).length() >= 8.0;
                    }
                    let delta = t.position - c.start;
                    if c.target == Target::Drawer {
                        c.canceled |= delta.x.abs() > 32.0 || delta.y > 24.0;
                    } else if !matches!(c.target, Target::Move | Target::Look) && !(c.target == Target::Fire && self.fire_drag) {
                        c.canceled |= delta.length() > 18.0;
                    }
                    c.at = t.position;
                    if c.target == Target::Move {
                        self.move_delta = t.position - c.origin;
                    }
                }
            }
            TouchPhase::Ended | TouchPhase::Canceled => {
                if let Some(c) = self.contacts.remove(&t.id) {
                    if c.target == Target::Move {
                        self.move_delta = Vec2::ZERO;
                    }
                    if c.target == Target::Aim && !self.toggle_aim {
                        self.aim = self.contacts.values().any(|c| c.target == Target::Aim);
                    }
                    if t.phase == TouchPhase::Canceled {
                        return false;
                    }
                    if c.target == Target::Drawer {
                        let d = t.position - c.start;
                        self.drawer_done |=
                            !c.canceled && d.y <= -self.drawer_distance && d.x.abs() <= 32.0;
                    } else if !c.canceled
                        && (t.position - c.start).length() <= 18.0
                        && target(
                            self.page,
                            size,
                            self.drawer_enabled,
                            self.floating,
                            self.movement_right,
                            t.position,
                        ) == Some(c.target)
                    {
                        match c.target {
                            Target::Close => {
                                self.close();
                                return true;
                            }
                            Target::Reset => {
                                self.reset();
                                return true;
                            }
                            Target::Begin => {
                                self.page = match self.page {
                                    PageKind::Guide => PageKind::GuideLook,
                                    PageKind::GuideLook => PageKind::GuideActions,
                                    _ => PageKind::Controls,
                                };
                                self.clear_contacts();
                                return true;
                            }
                            Target::DrawerPage => {
                                self.page = PageKind::Drawer;
                                self.clear_contacts();
                                return true;
                            }
                            Target::Fire => self.fire = true,
                            Target::Jump => self.jump = true,
                            _ => {}
                        }
                    }
                }
            }
        }
        false
    }
}
fn header(size: Vec2, close: bool) -> Rect {
    Rect::from_corners(
        Vec2::new(size.x - if close { 112.0 } else { 220.0 }, 12.0),
        Vec2::new(size.x - if close { 16.0 } else { 124.0 }, 68.0),
    )
}
fn area(size: Vec2, left: bool) -> Rect {
    Rect::from_corners(
        Vec2::new(if left { 16.0 } else { size.x * 0.52 }, 96.0),
        Vec2::new(
            if left { size.x * 0.48 } else { size.x - 16.0 },
            size.y - 88.0,
        ),
    )
}
fn look_centre(size: Vec2, movement_right: bool) -> Vec2 {
    let a = area(size, movement_right);
    Vec2::new(
        a.center().x,
        a.center().y.max(a.min.y + 84.0).min(a.max.y - 12.0),
    )
}
fn stick(size: Vec2) -> (Vec2, f32) {
    let a = area(size, true);
    let radius = ((a.height() - 60.0) * 0.5)
        .clamp(28.0, 80.0)
        .min(a.width() * 0.42);
    (Vec2::new(a.center().x, a.min.y + 60.0 + radius), radius)
}
fn stick_on_side(size: Vec2, movement_right: bool) -> (Vec2, f32) {
    let (mut centre, radius) = stick(size);
    if movement_right {
        centre.x = size.x - centre.x;
    }
    (centre, radius)
}
fn action(size: Vec2, i: usize, drawer: bool) -> Rect {
    let n = if drawer { 4.0 } else { 3.0 };
    let width = ((size.x - 32.0) / n - 8.0).min(140.0);
    let x = (size.x - n * (width + 8.0) + 8.0) * 0.5 + i as f32 * (width + 8.0);
    Rect::from_corners(
        Vec2::new(x, size.y - 80.0),
        Vec2::new(x + width, size.y - 24.0),
    )
}
fn begin(size: Vec2) -> Rect {
    Rect::from_center_size(
        Vec2::new(size.x * 0.5, size.y - 60.0),
        Vec2::new(200.0, 64.0),
    )
}
fn drawer_handle(size: Vec2) -> Rect {
    Rect::from_center_size(
        Vec2::new(size.x * 0.5, size.y - 64.0),
        Vec2::new(200.0, 64.0),
    )
}
fn target(page: PageKind, size: Vec2, drawer: bool, floating: bool, movement_right: bool, p: Vec2) -> Option<Target> {
    if header(size, true).contains(p) {
        return Some(Target::Close);
    }
    if header(size, false).contains(p) {
        return Some(Target::Reset);
    }
    match page {
        PageKind::Guide | PageKind::GuideLook | PageKind::GuideActions => {
            begin(size).contains(p).then_some(Target::Begin)
        }
        PageKind::Drawer => drawer_handle(size).contains(p).then_some(Target::Drawer),
        PageKind::Controls => {
            for (i, t) in [Target::Fire, Target::Aim, Target::Jump, Target::DrawerPage]
                .into_iter()
                .enumerate()
            {
                if i == 3 && !drawer {
                    break;
                }
                if action(size, i, drawer).contains(p) {
                    return Some(t);
                }
            }
            let (centre, r) = stick_on_side(size, movement_right);
            if (floating && area(size, !movement_right).contains(p)) || (!floating && p.distance(centre) <= r)
            {
                Some(Target::Move)
            } else {
                area(size, movement_right).contains(p).then_some(Target::Look)
            }
        }
    }
}

#[derive(SystemParam)]
pub(super) struct Bridge<'w, 's> {
    state: ResMut<'w, Practice>,
    paused: Option<Res<'w, Paused>>,
    page: Option<Res<'w, Page>>,
    over: ResMut<'w, FormOver>,
    keys: MessageReader<'w, 's, KeyboardInput>,
}
impl Bridge<'_, '_> {
    /// A form (practice, report or editor) is over the pause page.
    pub fn form_over(&self) -> bool { self.over.0 }
    /// Practice is open or draining its closing frames.
    pub fn blocking(&self) -> bool { self.state.blocking() }
    // Called only from the live touch reader, before it can create gameplay contacts.
    pub fn capture(
        &mut self,
        touches: &mut MessageReader<TouchInput>,
        window: Entity,
        size: Vec2,
        interactive: bool,
        in_menu: bool,
    ) -> bool {
        self.state.viewport = Some(size);
        let escape = self.keys.read().any(|k| {
            k.key_code == KeyCode::Escape
                && k.state == bevy::input::ButtonState::Pressed
                && !k.repeat
        });
        if !self.state.blocking() {
            return false;
        }
        if !self.state.active {
            self.state.closing -= 1;
            if self.state.closing == 0 {
                self.over.0 = false;
            }
            touches.clear();
            return true;
        }
        let valid = interactive
            && in_menu
            && self.paused.as_ref().is_some_and(|p| p.0)
            && self
                .page
                .as_ref()
                .is_some_and(|p| matches!(**p, Page::Settings { .. }))
            && size.x >= 400.0
            && size.y >= 300.0;
        if !valid || escape || self.state.size.is_some_and(|old| old != size) {
            self.state.close();
            touches.clear();
            return true;
        }
        self.state.size = Some(size);
        if self.state.opening {
            self.state.opening = false;
            touches.clear();
            return true;
        }
        let mut changed = false;
        for t in touches.read() {
            if t.window == window && self.state.event(t, size) {
                changed = true;
                break;
            }
        }
        if changed {
            touches.clear();
        }
        true
    }
    pub fn cancel(&mut self) {
        self.state.viewport = None;
        if self.state.active {
            self.state.close();
        }
    }
}
fn unavailable(world: &World) -> Option<&'static str> {
    if world
        .resource::<Practice>()
        .viewport
        .is_some_and(|s| s.x >= 400.0 && s.y >= 300.0)
    {
        None
    } else {
        Some("Rotate the device or enlarge the window. Practice needs at least 400 x 300 screen units.")
    }
}
fn open(world: &mut World, name: &str) -> bool {
    if unavailable(world).is_some()
        || name != OPEN
        || !world
            .get_resource::<State<nvf_play::PlayState>>()
            .is_some_and(|s| *s.get() == nvf_play::PlayState::Menu)
        || world.resource::<Practice>().blocking()
        || !world.get_resource::<Paused>().is_some_and(|p| p.0)
        || !world
            .get_resource::<Page>()
            .is_some_and(|p| matches!(p, Page::Settings { .. }))
        || world.resource::<FormOver>().0
    {
        return false;
    }
    let (dead_zone, floating, toggle_aim, movement_right) = {
        let layout = world.resource::<super::Layout>();
        (
            layout.config.aiming.dead_zone,
            layout.config.floating,
            layout.config.aiming.toggle_aim,
            layout.config.stick.center.x > 0.5,
        )
    };
    let movement = world.get_resource::<super::movement::Options>().copied().unwrap_or_default();
    let viewport = world.resource::<Practice>().viewport;
    let drawer = *world.resource::<super::drawer::Options>();
    *world.resource_mut::<Practice>() = Practice {
        active: true,
        opening: true,
        viewport,
        drawer_enabled: drawer.enabled,
        drawer_distance: drawer.threshold,
        toggle_aim,
        dead_zone,
        floating,
        movement_right,
        analog: movement.analog, fire_drag: movement.fire_drag,
        ..default()
    };
    world.resource_mut::<FormOver>().0 = true;
    true
}
pub(super) fn install(app: &mut App) {
    app.init_resource::<Practice>()
        .init_resource::<FormOver>()
        .add_systems(Startup, spawn)
        .add_systems(Update, show);
    settings::add_choice_list(app, Setting {
        key: KEY, group: "Controller", label: "Controls help / gesture practice", note: "Learn movement, aiming and gestures safely. Close returns here. Use Customize layout to move or resize controls and change styles or profiles.",
        switches: "",
        get: |_| false, set: |_, _| {}, slider: None,
    }, ChoiceList {
        names: |_| vec![CLOSED.into(), OPEN.into()], chosen: |_| CLOSED.into(),
        pick: open, save: |_| CLOSED.into(), load: |_, name| name == CLOSED, ask_keep: false,
    });
    settings::set_choice_action(app, KEY, |name| name == OPEN);
    settings::set_disabled_status(app, KEY, unavailable);
}

#[derive(Component, Clone, Copy)]
enum Visual {
    Header,
    Close,
    Reset,
    Guide,
    Begin,
    Left,
    Right,
    Ring,
    DeadZone,
    Knob,
    LookMarker,
    Fire,
    Aim,
    Jump,
    DrawerPage,
    DrawerText,
    DrawerHandle,
}
fn spawn(mut commands: Commands) {
    commands
        .spawn((
            PracticeRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
            BackgroundColor(Color::srgb(0.015, 0.025, 0.035)),
            GlobalZIndex(nvf_play::pause::FORM_Z),
            Visibility::Hidden,
        ))
        .with_children(|c| {
            for visual in [
                Visual::Header,
                Visual::Close,
                Visual::Reset,
                Visual::Guide,
                Visual::Begin,
                Visual::Left,
                Visual::Right,
                Visual::Ring,
                Visual::DeadZone,
                Visual::Knob,
                Visual::LookMarker,
                Visual::Fire,
                Visual::Aim,
                Visual::Jump,
                Visual::DrawerPage,
                Visual::DrawerText,
                Visual::DrawerHandle,
            ] {
                let mut entity = c.spawn((
                    visual,
                    Node {
                        position_type: PositionType::Absolute,
                        padding: UiRect::all(px(6)),
                        border: UiRect::all(px(1)),
                        ..default()
                    },
                    Text::new(""),
                    TextFont {
                        font_size: FontSize::Px(18.0),
                        ..default()
                    },
                    TextColor(Color::WHITE),
                    BackgroundColor(if matches!(visual, Visual::Knob) {
                        Color::NONE
                    } else {
                        Color::srgb(0.04, 0.09, 0.12)
                    }),
                    BorderColor::all(Color::srgb(0.25, 0.6, 0.65)),
                    Visibility::Hidden,
                ));
                if !matches!(
                    visual,
                    Visual::Ring | Visual::DeadZone | Visual::Knob | Visual::LookMarker
                ) {
                    entity.insert(Label);
                }
            }
        });
}
#[allow(clippy::type_complexity)]
fn show(
    state: Res<Practice>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    scale: Option<Res<UiScale>>,
    mut nodes: Query<
        (
            &Visual,
            &mut Node,
            &mut Text,
            &mut TextFont,
            &mut Visibility,
            Option<&mut Label>,
        ),
        Without<Children>,
    >,
    mut roots: Query<&mut Visibility, (With<Children>, With<PracticeRoot>)>,
) {
    // The root is updated separately below; child rectangles use logical screen units.
    for mut root in &mut roots {
        root.set_if_neq(if state.blocking() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    if !state.blocking() {
        for (_, _, _, _, mut visible, _) in &mut nodes {
            visible.set_if_neq(Visibility::Hidden);
        }
        return;
    }
    let Ok(w) = windows.single() else {
        return;
    };
    let size = Vec2::new(w.width(), w.height());
    let ui = scale.map_or(1.0, |s| s.0).max(0.1);
    let (_, r) = stick(size);
    let centre = state.stick_centre(size);
    for (v, mut node, mut text, mut font, mut visible, label) in &mut nodes {
        let control_page = state.page == PageKind::Controls;
        let (shown, rect, words) = match v {
            Visual::Header => (true, Rect::from_corners(Vec2::new(16.0, 8.0), Vec2::new(size.x-232.0, 88.0)), "Touch guide\nGame paused.".into()),
            Visual::Close => (true, header(size,true), "Close".into()), Visual::Reset => (true,header(size,false),"Reset".into()),
            Visual::Guide => (!control_page && state.page != PageKind::Drawer, Rect::from_corners(Vec2::new(20.0,84.0),Vec2::new(size.x-20.0,size.y-100.0)), match state.page {
                PageKind::Guide => format!("MOVEMENT THUMB ({})\n{} = no movement.\n{}\n{}", if state.movement_right { "right" } else { "left" }, if state.analog { "Inner circle" } else { "Inner box" },
                    if state.analog { "Push gently to walk; farther to run with Always Run. Lift to stop." } else { "Drag out for directional movement; lift to stop." },
                    if state.floating { "Floating: starts at your thumb." } else { "Fixed: start on the ring." }),
                PageKind::GuideLook => format!("LOOKING ({} thumb)\nDrag empty look space to turn.\n{}\nUse both thumbs together.", if state.movement_right { "left" } else { "right" }, if state.fire_drag { "Fire also looks while dragged; try it in practice." } else { "Lift to stop looking." }),
                _ => "SAFE PRACTICE\nTry Fire, Aim and Jump next.\nOnly this guide changes.\nNo ammo/AP used. Close exits.".into(),
            }),
            Visual::Begin => (!control_page && state.page != PageKind::Drawer,begin(size),match state.page { PageKind::Guide => "Next: looking", PageKind::GuideLook => "Next: actions", _ => "Begin practice" }.into()),
            Visual::Left => (control_page,area(size,!state.movement_right),format!("{}\n{}\n{}",if state.floating {if state.movement_right {"Touch right"} else {"Touch left"}} else {"Drag ring"},state.direction(r),state.speed_feedback(r))),
            Visual::Right => (control_page,area(size,state.movement_right),format!("LOOK: drag\n{}",if state.looked {"Look practised"} else {"Drag the dot"})),
            Visual::Ring => (control_page,Rect::from_center_size(centre,Vec2::splat(r*2.0)),String::new()),
            Visual::DeadZone => (control_page,Rect::from_center_size(centre,Vec2::splat((r*2.0*state.dead_zone).max(2.0))),String::new()),
            Visual::Knob => (control_page,Rect::from_center_size(centre+state.move_delta.clamp_length_max(r),Vec2::splat(28.0)),String::new()),
            Visual::LookMarker => (control_page,Rect::from_center_size((look_centre(size,state.movement_right)+state.look).clamp(area(size,state.movement_right).min+Vec2::splat(12.0),area(size,state.movement_right).max-Vec2::splat(12.0)),Vec2::splat(20.0)),String::new()),
            Visual::Fire => (control_page,action(size,0,state.drawer_enabled),if state.fire {"Fire: done"} else {"Fire"}.into()),
            Visual::Aim => (control_page,action(size,1,state.drawer_enabled),if state.aim && state.toggle_aim {"Aim: ON"} else if state.aim {"Aim: held"} else if state.aimed {"Aim: done"} else if state.toggle_aim {"Aim (tap)"} else {"Aim (hold)"}.into()),
            Visual::Jump => (control_page,action(size,2,state.drawer_enabled),if state.jump {"Jump: done"} else {"Jump"}.into()),
            Visual::DrawerPage => (control_page&&state.drawer_enabled,action(size,3,true),"Try drawer".into()),
            Visual::DrawerText => (state.page==PageKind::Drawer,Rect::from_corners(Vec2::new(24.0,84.0),Vec2::new(size.x-24.0,size.y-108.0)),if state.drawer_done {"DRAWER PRACTISED\nNo inventory command was sent.\nReset returns to the guide.".to_string()} else {format!("DRAG HANDLE UP; RELEASE\nMove up at least {:.0} units.\nPractice only - no inventory.\nAndroid navigation stays on.", state.drawer_distance)}),
            Visual::DrawerHandle => (state.page==PageKind::Drawer,drawer_handle(size),"Drag up; release".into()),
        };
        *visible = if state.blocking() && shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        node.left = px(rect.min.x / ui);
        node.top = px(rect.min.y / ui);
        node.width = px(rect.width() / ui);
        node.height = px(rect.height() / ui);
        node.border_radius = if matches!(v, Visual::DeadZone) {
            if state.analog { BorderRadius::MAX } else { BorderRadius::default() }
        } else { BorderRadius::all(px(
            if matches!(v, Visual::Ring | Visual::Knob | Visual::LookMarker) {
                999.0
            } else {
                4.0
            },
        )) };
        node.padding = if matches!(
            v,
            Visual::Ring | Visual::DeadZone | Visual::Knob | Visual::LookMarker
        ) {
            UiRect::ZERO
        } else {
            UiRect::all(px(6.0 / ui))
        };
        font.font_size = FontSize::Px(18.0 / ui);
        if text.0 != words {
            text.0 = words;
            if let Some(mut label) = label {
                label.set_changed();
            }
        }
    }
}
#[derive(Component)]
struct PracticeRoot;

#[cfg(test)]
mod tests {
    use super::super::{
        sync_touch, Contact as LiveContact, Role, TouchControlsPlugin, TouchState, BUTTONS,
    };
    use super::*;
    use bevy::{
        input::{
            keyboard::Key,
            mouse::{MouseButtonInput, MouseMotion},
            ButtonState, InputPlugin,
        },
        state::app::StatesPlugin,
        window::{AppLifecycle, PrimaryWindow, WindowFocused},
    };
    use nvf_play::{
        bindings::{Bindings, Control},
        control::{HeldSneakInput, PlayerControl},
        PlayState,
    };
    fn scene() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, InputPlugin, StatesPlugin));
        app.insert_resource(super::super::movement::Options::legacy());
        app.init_state::<PlayState>()
            .insert_resource(Bindings::default())
            .insert_resource(Paused(true))
            .insert_resource(Page::Settings { selected: None })
            .init_resource::<PlayerControl>()
            .add_message::<AppLifecycle>()
            .add_message::<WindowFocused>()
            .add_plugins(TouchControlsPlugin)
            .add_systems(Update, nvf_play::control::read_controls);
        let w = app
            .world_mut()
            .spawn((
                Window {
                    resolution: bevy::window::WindowResolution::new(900, 450),
                    ..default()
                },
                PrimaryWindow,
            ))
            .id();
        app.world_mut()
            .resource_mut::<NextState<PlayState>>()
            .set(PlayState::Menu);
        app.update();
        app.world_mut()
            .resource_mut::<super::super::Layout>()
            .config = super::super::layout::Config::default();
        (app, w)
    }
    fn launch(app: &mut App) {
        let i = app
            .world()
            .resource::<settings::Settings>()
            .index(KEY)
            .unwrap();
        assert!(settings::pick(app.world_mut(), i, OPEN));
        app.update(); // Drop the gesture that opened Settings' action.
    }
    fn send(app: &mut App, w: Entity, id: u64, phase: TouchPhase, p: Vec2) {
        app.world_mut().write_message(TouchInput {
            window: w,
            id,
            phase,
            position: p,
            force: None,
        });
    }
    fn tap(app: &mut App, w: Entity, id: u64, p: Vec2) {
        send(app, w, id, TouchPhase::Started, p);
        send(app, w, id, TouchPhase::Ended, p);
        app.update();
    }
    fn controls(app: &mut App, w: Entity) {
        for id in 1..=3 {
            tap(app, w, id, begin(Vec2::new(900.0, 450.0)).center());
        }
        assert!(app.world().resource::<Practice>().page == PageKind::Controls);
    }
    fn raw_key(app: &mut App, w: Entity, key_code: KeyCode) {
        app.world_mut().write_message(KeyboardInput {
            window: w,
            key_code,
            logical_key: Key::Character("practice fixture".into()),
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
        });
    }
    fn isolated(app: &App) {
        let state = app.world().resource::<TouchState>();
        assert!(
            state.contacts.is_empty()
                && !state.aim_latched
                && state.keys.is_empty()
                && state.mouse.is_empty()
        );
        assert!(!app.world().resource::<HeldSneakInput>().active);
        assert!(
            !app.world().resource::<PlayerControl>().trigger
                && !app.world().resource::<PlayerControl>().aiming
        );
        assert_eq!(
            app.world()
                .resource::<Messages<nvf_play::menu::OpenInventory>>()
                .len(),
            0
        );
        assert_eq!(app.world().resource::<Messages<MouseMotion>>().len(), 0);
        assert_eq!(
            *app.world().resource::<State<PlayState>>().get(),
            PlayState::Menu
        );
        assert!(app.world().resource::<Paused>().0);
    }
    #[test]
    fn settings_action_is_not_persisted_or_replayed_from_ini_and_refuses_other_forms() {
        let (mut app, _) = scene();
        let path = std::env::temp_dir().join(format!("nvf-practice-{}.ini", uuid::Uuid::new_v4()));
        let original = "[Settings]\nvoice=0\n";
        std::fs::write(&path, original).unwrap();
        app.insert_resource(settings::SettingsFile(Some(path.clone())));
        let i = app
            .world()
            .resource::<settings::Settings>()
            .index(KEY)
            .unwrap();
        let c = app
            .world()
            .resource::<settings::Settings>()
            .choices(i)
            .unwrap();
        assert!(!(c.load)(app.world_mut(), OPEN));
        assert!((c.load)(app.world_mut(), CLOSED));
        assert!(!app.world().resource::<Practice>().blocking());
        app.world_mut().resource_mut::<FormOver>().0 = true;
        assert!(!settings::pick(app.world_mut(), i, OPEN));
        app.world_mut().resource_mut::<FormOver>().0 = false;
        assert!(settings::pick(app.world_mut(), i, OPEN));
        assert_eq!((c.chosen)(app.world()), CLOSED);
        assert_eq!((c.save)(app.world()), CLOSED);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn guide_precedes_controls_and_entry_batch_cannot_skip_the_explanation() {
        let (mut app, w) = scene();
        let i = app
            .world()
            .resource::<settings::Settings>()
            .index(KEY)
            .unwrap();
        assert!(settings::pick(app.world_mut(), i, OPEN));
        let at = begin(Vec2::new(900.0, 450.0)).center();
        send(&mut app, w, 1, TouchPhase::Started, at);
        send(&mut app, w, 1, TouchPhase::Ended, at);
        app.update();
        assert!(app.world().resource::<Practice>().page == PageKind::Guide);
        controls(&mut app, w);
        isolated(&app);
    }
    #[test]
    fn actual_touch_route_changes_only_synthetic_move_look_and_actions() {
        let (mut app, w) = scene();
        launch(&mut app);
        controls(&mut app, w);
        let size = Vec2::new(900.0, 450.0);
        let (centre, r) = stick(size);
        send(&mut app, w, 10, TouchPhase::Started, centre);
        send(
            &mut app,
            w,
            10,
            TouchPhase::Moved,
            centre + Vec2::new(r, -r),
        );
        let at = area(size, false).center();
        send(&mut app, w, 11, TouchPhase::Started, at);
        send(
            &mut app,
            w,
            11,
            TouchPhase::Moved,
            at + Vec2::new(30.0, 10.0),
        );
        for (id, i) in [(20, 0), (21, 1), (22, 2)] {
            let p = action(size, i, false).center();
            send(&mut app, w, id, TouchPhase::Started, p);
            send(&mut app, w, id, TouchPhase::Ended, p);
        }
        app.update();
        let p = app.world().resource::<Practice>();
        assert!(p.fire && !p.aim && p.aimed && p.jump && p.looked);
        assert_eq!(p.direction(r), "Forward right");
        assert_ne!(p.look, Vec2::ZERO);
        isolated(&app);
        send(&mut app, w, 10, TouchPhase::Ended, centre);
        send(&mut app, w, 11, TouchPhase::Canceled, at);
        app.update();
        assert_eq!(
            app.world().resource::<Practice>().direction(r),
            "In dead zone"
        );
        isolated(&app);
    }
    #[test]
    fn cancelled_or_dragged_action_does_not_complete_and_reset_restores_guide() {
        let (mut app, w) = scene();
        launch(&mut app);
        controls(&mut app, w);
        let size = Vec2::new(900.0, 450.0);
        let at = action(size, 0, false).center();
        send(&mut app, w, 1, TouchPhase::Started, at);
        send(&mut app, w, 1, TouchPhase::Canceled, at);
        app.update();
        send(&mut app, w, 2, TouchPhase::Started, at);
        send(&mut app, w, 2, TouchPhase::Moved, at + Vec2::new(40.0, 0.0));
        send(&mut app, w, 2, TouchPhase::Ended, at);
        app.update();
        assert!(!app.world().resource::<Practice>().fire);
        tap(&mut app, w, 3, at);
        assert!(app.world().resource::<Practice>().fire);
        tap(&mut app, w, 4, header(size, false).center());
        let p = app.world().resource::<Practice>();
        assert!(p.page == PageKind::Guide && !p.fire && p.contacts.is_empty());
        isolated(&app);
    }
    #[test]
    fn optional_drawer_practice_never_sends_a_real_inventory_request() {
        let (mut app, w) = scene();
        app.world_mut()
            .resource_mut::<super::super::drawer::Options>()
            .enabled = true;
        launch(&mut app);
        controls(&mut app, w);
        let size = Vec2::new(900.0, 450.0);
        tap(&mut app, w, 1, action(size, 3, true).center());
        let at = drawer_handle(size).center();
        send(&mut app, w, 2, TouchPhase::Started, at);
        send(
            &mut app,
            w,
            2,
            TouchPhase::Moved,
            at - Vec2::new(0.0, 140.0),
        );
        send(
            &mut app,
            w,
            2,
            TouchPhase::Ended,
            at - Vec2::new(0.0, 140.0),
        );
        app.update();
        assert!(app.world().resource::<Practice>().drawer_done);
        isolated(&app);
    }
    #[test]
    fn entry_and_escape_close_release_touch_owners_but_preserve_hardware_and_drain_tail() {
        let (mut app, w) = scene();
        raw_key(&mut app, w, KeyCode::KeyW);
        app.world_mut().write_message(MouseButtonInput {
            window: w,
            button: MouseButton::Middle,
            state: ButtonState::Pressed,
        });
        app.update();
        app.world_mut()
            .resource_mut::<super::super::Layout>()
            .config
            .aiming
            .toggle_aim = true;
        let bindings = app.world().resource::<Bindings>().clone();
        let index = BUTTONS
            .iter()
            .position(|b| b.control == Some(Control::Block))
            .unwrap();
        {
            let mut state = app.world_mut().resource_mut::<TouchState>();
            state.toggle_aim = true;
            state.aim_latched = true;
            state.contacts.insert(
                50,
                LiveContact {
                    origin: Vec2::ZERO,
                    position: Vec2::ZERO,
                    role: Role::Button(index),
                    started: 0.0,
                    dragged: false,
                },
            );
            sync_touch(&mut state, Some(&bindings), false);
        }
        app.world_mut().resource_mut::<HeldSneakInput>().active = true;
        launch(&mut app);
        isolated(&app);
        controls(&mut app, w);
        raw_key(&mut app, w, KeyCode::Escape);
        let at = action(Vec2::new(900.0, 450.0), 0, false).center();
        send(&mut app, w, 99, TouchPhase::Started, at);
        app.update();
        assert!(!app.world().resource::<Practice>().active && app.world().resource::<FormOver>().0);
        for _ in 0..2 {
            app.update();
            isolated(&app);
            let state = app.world().resource::<TouchState>();
            assert!(
                state.physical_keys.contains(&KeyCode::KeyW)
                    && state.physical_mouse.contains(&MouseButton::Middle)
            );
            assert!(app
                .world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::KeyW));
            assert!(app
                .world()
                .resource::<ButtonInput<MouseButton>>()
                .pressed(MouseButton::Middle));
        }
        assert!(
            !app.world().resource::<Practice>().blocking() && !app.world().resource::<FormOver>().0
        );
    }
    #[test]
    fn close_touch_consumes_fresh_gameplay_starts_in_the_same_batch() {
        let (mut app, w) = scene();
        launch(&mut app);
        controls(&mut app, w);
        let size = Vec2::new(900.0, 450.0);
        let close = header(size, true).center();
        send(&mut app, w, 1, TouchPhase::Started, close);
        send(&mut app, w, 1, TouchPhase::Ended, close);
        send(
            &mut app,
            w,
            2,
            TouchPhase::Started,
            action(size, 1, false).center(),
        );
        app.update();
        isolated(&app);
        assert!(!app.world().resource::<Practice>().aimed);
        app.update();
        app.update();
        isolated(&app);
    }
    #[test]
    fn lifecycle_resize_and_leaving_settings_cancel_the_session() {
        for reason in 0..4 {
            let (mut app, w) = scene();
            launch(&mut app);
            controls(&mut app, w);
            match reason {
                0 => {
                    app.world_mut().write_message(WindowFocused {
                        window: w,
                        focused: false,
                    });
                }
                1 => {
                    app.world_mut().write_message(AppLifecycle::Suspended);
                }
                2 => app
                    .world_mut()
                    .get_mut::<Window>(w)
                    .unwrap()
                    .resolution
                    .set(800.0, 450.0),
                _ => {
                    *app.world_mut().resource_mut::<Page>() = Page::Main;
                }
            }
            app.update();
            assert!(!app.world().resource::<Practice>().active);
            isolated(&app);
        }
    }
    #[test]
    fn modal_practice_blocks_automatic_controller_takeover_without_changing_hardware_buttons() {
        use bevy::input::gamepad::{RawGamepadButtonChangedEvent, RawGamepadEvent};
        use nvf_play::bindings::InputDevice;
        let (mut app, w) = scene();
        app.add_plugins(nvf_play::pad::InputSwitchPlugin);
        let pad = app.world_mut().spawn(Gamepad::default()).id();
        launch(&mut app);
        app.world_mut()
            .write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
                pad,
                GamepadButton::South,
                1.0,
            )));
        app.update();
        assert_eq!(
            app.world().resource::<Bindings>().device,
            InputDevice::KeyboardMouse
        );
        assert!(app
            .world()
            .get::<Gamepad>(pad)
            .unwrap()
            .pressed(GamepadButton::South));
        // Explicit device selection also cannot make practice input become live input.
        nvf_play::bindings::set_device(app.world_mut(), InputDevice::Controller);
        controls(&mut app, w);
        tap(
            &mut app,
            w,
            42,
            action(Vec2::new(900.0, 450.0), 0, false).center(),
        );
        assert!(app.world().resource::<Practice>().fire);
        isolated(&app);
    }
    #[test]
    fn transition_back_to_gameplay_consumes_the_closing_frame_and_old_practice_finger() {
        let (mut app, w) = scene();
        launch(&mut app);
        controls(&mut app, w);
        let i = BUTTONS
            .iter()
            .position(|b| b.control == Some(Control::Use))
            .unwrap();
        let fire = app
            .world()
            .resource::<super::super::Layout>()
            .config
            .center(super::super::Target::Button(i), Vec2::new(900.0, 450.0));
        app.world_mut().resource_mut::<Paused>().0 = false;
        app.world_mut()
            .resource_mut::<NextState<PlayState>>()
            .set(PlayState::Playing);
        send(&mut app, w, 77, TouchPhase::Started, fire);
        app.update();
        assert!(!app.world().resource::<Practice>().active);
        assert_eq!(
            *app.world().resource::<State<PlayState>>().get(),
            PlayState::Playing
        );
        for _ in 0..3 {
            assert!(app.world().resource::<TouchState>().contacts.is_empty());
            assert!(!app.world().resource::<PlayerControl>().trigger);
            assert!(!app
                .world()
                .resource::<ButtonInput<MouseButton>>()
                .pressed(MouseButton::Left));
            assert_eq!(
                app.world()
                    .resource::<Messages<nvf_play::menu::OpenInventory>>()
                    .len(),
                0
            );
            app.update();
        }
        send(&mut app, w, 77, TouchPhase::Moved, fire);
        send(&mut app, w, 77, TouchPhase::Ended, fire);
        app.update();
        assert!(!app.world().resource::<PlayerControl>().trigger);
        assert!(app.world().resource::<TouchState>().contacts.is_empty());
    }
    #[test]
    fn practice_aim_uses_the_current_hold_or_toggle_mode_without_synthesizing_aim() {
        for toggle in [false, true] {
            let (mut app, w) = scene();
            app.world_mut()
                .resource_mut::<super::super::Layout>()
                .config
                .aiming
                .toggle_aim = toggle;
            launch(&mut app);
            controls(&mut app, w);
            let at = action(Vec2::new(900.0, 450.0), 1, false).center();
            send(&mut app, w, 60, TouchPhase::Started, at);
            app.update();
            assert!(
                app.world().resource::<Practice>().aim,
                "both modes act on Started"
            );
            isolated(&app);
            send(&mut app, w, 61, TouchPhase::Started, at);
            app.update();
            assert!(
                app.world().resource::<Practice>().aim,
                "overlapping Aim contact must not toggle again"
            );
            isolated(&app);
            send(
                &mut app,
                w,
                60,
                TouchPhase::Moved,
                at + Vec2::new(60.0, 0.0),
            );
            app.update();
            assert!(
                app.world().resource::<Practice>().aim,
                "drag retains the owned Aim action"
            );
            isolated(&app);
            send(
                &mut app,
                w,
                60,
                TouchPhase::Ended,
                at + Vec2::new(60.0, 0.0),
            );
            app.update();
            assert!(
                app.world().resource::<Practice>().aim,
                "other held contact still owns Hold; toggle latch survives Ended"
            );
            isolated(&app);
            send(&mut app, w, 61, TouchPhase::Ended, at);
            app.update();
            assert_eq!(app.world().resource::<Practice>().aim, toggle);
            assert!(app.world().resource::<Practice>().aimed);
            isolated(&app);
            send(&mut app, w, 62, TouchPhase::Started, at);
            app.update();
            assert_eq!(
                app.world().resource::<Practice>().aim,
                !toggle,
                "a new gesture toggles at Started"
            );
            isolated(&app);
            send(&mut app, w, 62, TouchPhase::Ended, at);
            app.update();
            assert!(
                !app.world().resource::<Practice>().aim,
                "Ended never flips a latch"
            );
            isolated(&app);
            send(&mut app, w, 63, TouchPhase::Started, at);
            send(&mut app, w, 64, TouchPhase::Started, at);
            app.update();
            assert!(app.world().resource::<Practice>().aim);
            send(&mut app, w, 63, TouchPhase::Canceled, at);
            app.update();
            assert_eq!(
                app.world().resource::<Practice>().aim,
                !toggle,
                "cancel clears toggle, but another held owner remains"
            );
            isolated(&app);
            send(&mut app, w, 64, TouchPhase::Ended, at);
            app.update();
            assert!(!app.world().resource::<Practice>().aim);
            isolated(&app);
            send(&mut app, w, 65, TouchPhase::Started, at);
            app.update();
            send(&mut app, w, 99, TouchPhase::Canceled, Vec2::ZERO);
            app.update();
            assert_eq!(
                app.world().resource::<Practice>().aim,
                !toggle,
                "even an unowned cancelled touch clears the live-style toggle latch"
            );
            isolated(&app);
            send(&mut app, w, 65, TouchPhase::Ended, at);
            app.update();
            assert!(!app.world().resource::<Practice>().aim);
            isolated(&app);
        }
    }
    fn displayed_ring_centre(app: &mut App) -> Vec2 {
        let world = app.world_mut();
        let mut query = world.query::<(&Visual, &Node)>();
        let (_, node) = query
            .iter(world)
            .find(|(v, _)| matches!(v, Visual::Ring))
            .unwrap();
        let (Val::Px(x), Val::Px(y), Val::Px(w), Val::Px(h)) =
            (node.left, node.top, node.width, node.height)
        else {
            panic!("practice ring must use logical pixel coordinates");
        };
        Vec2::new(x + w * 0.5, y + h * 0.5)
    }
    #[test]
    fn movement_modes_use_contact_origins_and_ignore_a_second_finger() {
        for floating in [false, true] {
            let (mut app, w) = scene();
            app.world_mut()
                .resource_mut::<super::super::Layout>()
                .config
                .floating = floating;
            launch(&mut app);
            controls(&mut app, w);
            let size = Vec2::new(900.0, 450.0);
            let (fixed, r) = stick(size);
            let start = fixed + Vec2::new(r * 0.4, 0.0);
            let origin = if floating { start } else { fixed };
            let zone = app.world().resource::<Practice>().dead_zone * r;
            send(&mut app, w, 40, TouchPhase::Started, start);
            app.update();
            {
                let p = app.world().resource::<Practice>();
                assert_eq!(p.contacts[&40].origin, origin);
                assert_eq!(p.move_delta, start - origin);
                assert_eq!(
                    p.direction(r),
                    if floating { "In dead zone" } else { "right" }
                );
            }
            assert_eq!(displayed_ring_centre(&mut app), origin);
            isolated(&app);
            send(
                &mut app,
                w,
                40,
                TouchPhase::Moved,
                origin + Vec2::new(zone * 0.5, 0.0),
            );
            app.update();
            assert_eq!(
                app.world().resource::<Practice>().direction(r),
                "In dead zone"
            );
            isolated(&app);
            send(
                &mut app,
                w,
                40,
                TouchPhase::Moved,
                origin + Vec2::new(zone * 1.5, 0.0),
            );
            app.update();
            assert_eq!(app.world().resource::<Practice>().direction(r), "right");
            send(&mut app, w, 41, TouchPhase::Started, fixed);
            send(&mut app, w, 41, TouchPhase::Moved, fixed - Vec2::new(r, r));
            app.update();
            {
                let p = app.world().resource::<Practice>();
                assert!(
                    !p.contacts.contains_key(&41),
                    "another thumb cannot steal movement"
                );
                assert_eq!(p.contacts[&40].origin, origin);
                assert_eq!(p.direction(r), "right");
            }
            assert_eq!(displayed_ring_centre(&mut app), origin);
            isolated(&app);
            send(&mut app, w, 41, TouchPhase::Ended, fixed);
            app.update();
            assert_eq!(app.world().resource::<Practice>().direction(r), "right");
            send(
                &mut app,
                w,
                40,
                TouchPhase::Moved,
                origin + Vec2::new(-zone * 1.5, zone * 1.5),
            );
            app.update();
            assert_eq!(app.world().resource::<Practice>().direction(r), "Back left");
            assert_eq!(displayed_ring_centre(&mut app), origin);
            isolated(&app);
            send(&mut app, w, 40, TouchPhase::Ended, start);
            app.update();
            assert_eq!(app.world().resource::<Practice>().move_delta, Vec2::ZERO);
            let outside = area(size, true).min + Vec2::new(16.0, 32.0);
            assert!(outside.distance(fixed) > r);
            send(&mut app, w, 42, TouchPhase::Started, outside);
            app.update();
            assert_eq!(
                app.world()
                    .resource::<Practice>()
                    .contacts
                    .contains_key(&42),
                floating,
                "only floating mode admits the whole left panel"
            );
            if floating {
                let p = app.world().resource::<Practice>();
                assert_eq!(p.move_delta, Vec2::ZERO);
                assert_eq!(p.contacts[&42].origin, outside);
                assert_eq!(displayed_ring_centre(&mut app), outside);
                send(
                    &mut app,
                    w,
                    42,
                    TouchPhase::Moved,
                    outside + Vec2::new(zone * 1.5, 0.0),
                );
                app.update();
                assert_eq!(app.world().resource::<Practice>().direction(r), "right");
            }
            isolated(&app);
            send(&mut app, w, 42, TouchPhase::Canceled, outside);
            app.update();
            assert_eq!(app.world().resource::<Practice>().move_delta, Vec2::ZERO);
            assert_eq!(displayed_ring_centre(&mut app), fixed);
            isolated(&app);
        }
    }
    #[test]
    fn too_small_a_viewport_refuses_the_settings_action_with_an_explanation() {
        let (mut app, w) = scene();
        app.world_mut()
            .get_mut::<Window>(w)
            .unwrap()
            .resolution
            .set(320.0, 450.0);
        app.update();
        let i = app
            .world()
            .resource::<settings::Settings>()
            .index(KEY)
            .unwrap();
        assert!(unavailable(app.world()).unwrap().contains("Rotate"));
        assert!(!settings::pick(app.world_mut(), i, OPEN));
        assert!(
            !app.world().resource::<Practice>().blocking() && !app.world().resource::<FormOver>().0
        );
    }
    #[test]
    fn geometry_and_dead_zone_feedback_have_large_distinct_targets() {
        for size in [
            Vec2::new(480.0, 320.0),
            Vec2::new(900.0, 450.0),
            Vec2::new(450.0, 900.0),
        ] {
            for i in 0..4 {
                let r = action(size, i, true);
                assert!(r.width() >= 80.0 && r.height() >= 56.0);
                assert!(!r.contains(stick(size).0));
            }
            assert!(header(size, true).height() >= 56.0 && begin(size).height() >= 56.0);
            let mut p = Practice {
                dead_zone: 0.2,
                ..default()
            };
            let r = stick(size).1;
            p.move_delta = Vec2::splat(r * 0.19);
            assert_eq!(p.direction(r), "In dead zone");
            p.move_delta = Vec2::new(-r * 0.3, r * 0.4);
            assert_eq!(p.direction(r), "Back left");
        }
    }
    #[test]
    fn practice_uses_current_analog_radial_feedback_and_fire_drag_without_live_actions() {
        let (mut app,w)=scene();
        app.world_mut().resource_mut::<super::super::movement::Options>().analog=true;
        app.world_mut().resource_mut::<super::super::movement::Options>().fire_drag=true;
        launch(&mut app);controls(&mut app,w);
        let size=Vec2::new(900.0,450.0);let (origin,r)=stick(size);
        send(&mut app,w,10,TouchPhase::Started,origin);
        send(&mut app,w,10,TouchPhase::Moved,origin+Vec2::new(0.15,-0.15)*r);app.update();
        assert_eq!(app.world().resource::<Practice>().direction(r),"Forward right","radial diagonal can leave dead zone before either axis alone");
        assert!(app.world().resource::<Practice>().speed_feedback(r).starts_with("Analog "));
        let fire=action(size,0,app.world().resource::<Practice>().drawer_enabled).center();
        send(&mut app,w,11,TouchPhase::Started,fire);app.update();
        send(&mut app,w,11,TouchPhase::Moved,fire+Vec2::new(25.0,0.0));app.update();
        assert!(app.world().resource::<Practice>().fire && app.world().resource::<Practice>().looked);
        assert!(app.world().resource::<Practice>().look.x>0.0);
        isolated(&app);
    }

    #[test]
    fn actual_practice_dead_zone_preserves_the_analog_circle_and_legacy_square() {
        for analog in [false, true] {
            let (mut app, w) = scene();
            app.world_mut().resource_mut::<super::super::movement::Options>().analog = analog;
            launch(&mut app);
            {
                let world = app.world_mut();
                let mut query = world.query::<(&Visual, &Text)>();
                let (_, text) = query.iter(world).find(|(v, _)| matches!(v, Visual::Guide)).unwrap();
                assert!(text.0.contains(if analog { "Inner circle" } else { "Inner box" }));
            }
            controls(&mut app, w);
            {
                let world = app.world_mut();
                let mut query = world.query::<(&Visual, &Node, &Visibility)>();
                let (_, node, visibility) = query.iter(world).find(|(v, _, _)| matches!(v, Visual::DeadZone)).unwrap();
                assert_eq!(*visibility, Visibility::Inherited);
                assert_eq!(node.border_radius, if analog { BorderRadius::MAX } else { BorderRadius::default() });
                assert_eq!(node.width, node.height);
            }
            isolated(&app);
            assert_eq!(app.world().resource::<nvf_play::touch_movement::TouchMovementInput>().axes, Vec2::ZERO);
        }
    }

    #[test]
    fn actual_practice_mirrors_current_right_side_for_fixed_and_floating_owners() {
        for floating in [false, true] {
            let (mut app, w) = scene();
            {
                let mut layout = app.world_mut().resource_mut::<super::super::Layout>();
                layout.config.apply_preset("left");
                layout.config.floating = floating;
                layout.config.aiming.dead_zone = 0.23;
                layout.config.aiming.look = 1.7;
                layout.config.aiming.ads = 0.6;
                if !floating {
                    // A custom right-side stick without the preset's southpaw bit
                    // must use the same side as live movement/instructions.
                    layout.config.stick.center.x = 0.71;
                    layout.config.southpaw = false;
                    layout.config.preset_name = "classic".into();
                }
            }
            // The real tray provider migrates an implicit layout on its next
            // normal frame. Settle it before starting the isolated lesson, so
            // the full unchanged-config oracle measures practice, not setup.
            assert!(!app.world().resource::<Practice>().blocking());
            app.update();
            let saved = app.world().resource::<super::super::Layout>().config.clone();
            app.update();
            assert_eq!(app.world().resource::<super::super::Layout>().config, saved,
                "the ordinary tray-migration baseline must be stable before practice opens");
            if !floating {
                assert_eq!(saved.preset_name, "classic", "Custom movement side must not come from the left preset ID");
            }
            launch(&mut app);
            assert!(app.world().resource::<Practice>().movement_right);
            assert_eq!(app.world().resource::<Practice>().dead_zone, 0.23);
            let size = Vec2::new(900.0, 450.0);
            {
                let world = app.world_mut();
                let mut query = world.query::<(&Visual, &Text)>();
                assert!(query.iter(world).find(|(v, _)| matches!(v, Visual::Guide)).unwrap().1.0.contains("MOVEMENT THUMB (right)"));
            }
            tap(&mut app, w, 70, begin(size).center());
            {
                let world = app.world_mut();
                let mut query = world.query::<(&Visual, &Text)>();
                assert!(query.iter(world).find(|(v, _)| matches!(v, Visual::Guide)).unwrap().1.0.contains("LOOKING (left thumb)"));
            }
            tap(&mut app, w, 71, begin(size).center());
            tap(&mut app, w, 72, begin(size).center());
            assert!(app.world().resource::<Practice>().page == PageKind::Controls);
            let (fixed, radius) = stick_on_side(size, true);
            assert_eq!(displayed_ring_centre(&mut app), fixed);
            let start = fixed + Vec2::new(radius * 0.25, 0.0);
            send(&mut app, w, 40, TouchPhase::Started, start);
            app.update();
            let origin = if floating { start } else { fixed };
            assert_eq!(app.world().resource::<Practice>().contacts[&40].origin, origin);
            assert_eq!(displayed_ring_centre(&mut app), origin);
            let look = area(size, true).center();
            send(&mut app, w, 41, TouchPhase::Started, look);
            send(&mut app, w, 41, TouchPhase::Moved, look + Vec2::new(20.0, 0.0));
            send(&mut app, w, 42, TouchPhase::Started, fixed);
            send(&mut app, w, 40, TouchPhase::Moved, start + Vec2::new(radius * 0.4, 0.0));
            app.update();
            {
                let state = app.world().resource::<Practice>();
                assert!(state.contacts[&40].target == Target::Move);
                assert!(state.contacts[&41].target == Target::Look);
                assert!(!state.contacts.contains_key(&42), "another thumb cannot steal movement");
                assert!(state.move_delta.x > 0.0 && state.look.x > 0.0 && state.looked);
            }
            {
                let world = app.world_mut();
                let mut query = world.query::<(&Visual, &Node, &Text)>();
                let (_, node, text) = query.iter(world).find(|(v, _, _)| matches!(v, Visual::Left)).unwrap();
                assert_eq!(node.left, px(area(size, false).min.x));
                assert!(text.0.starts_with(if floating { "Touch right" } else { "Drag ring" }));
                let (_, node, _) = query.iter(world).find(|(v, _, _)| matches!(v, Visual::LookMarker)).unwrap();
                let Val::Px(x) = node.left else { panic!("look marker must use logical pixels") };
                assert!(x < size.x * 0.5, "look marker must stay in the opposite panel");
            }
            send(&mut app, w, 42, TouchPhase::Ended, fixed);
            app.update();
            assert!(app.world().resource::<Practice>().contacts.contains_key(&40));
            send(&mut app, w, 40, TouchPhase::Ended, start);
            send(&mut app, w, 41, TouchPhase::Ended, look);
            app.update();
            assert!(app.world().resource::<Practice>().contacts.is_empty());
            assert_eq!(app.world().resource::<Practice>().move_delta, Vec2::ZERO);
            let drawer = app.world().resource::<Practice>().drawer_enabled;
            tap(&mut app, w, 60, action(size, 0, drawer).center());
            tap(&mut app, w, 61, action(size, 2, drawer).center());
            assert!(app.world().resource::<Practice>().fire && app.world().resource::<Practice>().jump,
                    "Mirroring teaching panels retains real practice action hit targets");
            assert_eq!(app.world().resource::<super::super::Layout>().config, saved);
            assert_eq!(app.world().resource::<nvf_play::touch_movement::TouchMovementInput>().axes, Vec2::ZERO);
            isolated(&app);
            tap(&mut app, w, 80, header(size, false).center());
            assert!(app.world().resource::<Practice>().page == PageKind::Guide);
            assert!(app.world().resource::<Practice>().movement_right, "Reset retains the selected movement side");
            controls(&mut app, w);
            send(&mut app, w, 43, TouchPhase::Started, fixed);
            send(&mut app, w, 43, TouchPhase::Moved, fixed + Vec2::new(24.0, 0.0));
            app.update();
            assert!(app.world().resource::<Practice>().move_delta.x > 0.0);
            send(&mut app, w, 43, TouchPhase::Canceled, fixed);
            app.update();
            assert_eq!(app.world().resource::<Practice>().move_delta, Vec2::ZERO);
            assert!(app.world().resource::<Practice>().movement_right);
            tap(&mut app, w, 81, header(size, true).center());
            for _ in 0..3 { app.update(); }
            assert!(!app.world().resource::<Practice>().blocking());
            app.world_mut().resource_mut::<super::super::Layout>().config.stick.center.x = 0.2;
            launch(&mut app);
            assert!(!app.world().resource::<Practice>().movement_right, "Reopening reads the current provider again");
            controls(&mut app, w);
            let (left, _) = stick(size);
            assert_eq!(displayed_ring_centre(&mut app), left);
            send(&mut app, w, 44, TouchPhase::Started, left);
            send(&mut app, w, 44, TouchPhase::Moved, left + Vec2::new(24.0, 0.0));
            app.update();
            assert!(app.world().resource::<Practice>().contacts[&44].target == Target::Move);
            assert!(app.world().resource::<Practice>().move_delta.x > 0.0, "Mirroring positions never reverses directional signs");
            isolated(&app);
        }
    }

}
