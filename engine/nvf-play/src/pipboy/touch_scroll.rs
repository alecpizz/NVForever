//! Projected mobile pointer/scroll frame, separate from physical wheel and buttons.
use super::{Frame, Page, PipBoy};
use bevy::prelude::*;
pub mod gesture;
/// The production projected pointer route, reusable by headless mobile composition tests.
pub struct TouchPointerPlugin;
impl Plugin for TouchPointerPlugin {
    fn build(&self, app: &mut App) {
        super::tab_swipe::install(app);
        app.init_resource::<WheelFrame>()
            .add_message::<super::PipBoyInput>()
            .init_resource::<PhysicalPriority>()
            .init_resource::<ButtonInput<MouseButton>>()
            .add_message::<bevy::input::mouse::MouseWheel>()
            .add_systems(
                PreUpdate,
                sample_physical_priority
                    .after(bevy::input::InputSystems)
                    .after(crate::pad::InputSwitchSystems::Switch)
                    .before(super::TouchPointerSet),
            )
            .init_resource::<bevy::picking::pointer::PointerMap>()
            .add_message::<bevy::picking::pointer::PointerInput>()
            .add_message::<bevy::window::WindowEvent>()
            .add_systems(
                PreUpdate,
                super::drive_pointer
                    .in_set(super::TouchPointerSet)
                    .after(crate::pad::device::automatic)
                    .after(crate::pad::pointer::follow)
                    .before(bevy::picking::PickingSystems::ProcessInput),
            );
    }
}

/// One frame's real mouse intent. Observing it never consumes or clears hardware state.
#[derive(Resource, Default)]
pub struct PhysicalPriority {
    pub active: bool,
}
fn sample_physical_priority(
    mouse: Res<ButtonInput<MouseButton>>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    mut priority: ResMut<PhysicalPriority>,
) {
    let buttons = mouse.get_pressed().next().is_some()
        || mouse.get_just_pressed().next().is_some()
        || mouse.get_just_released().next().is_some();
    let wheels = wheel
        .read()
        .fold(false, |used, w| used || w.x != 0.0 || w.y != 0.0);
    priority.active = buttons || wheels;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeRow {
    Inventory {
        guard: crate::menu::model::RowGuard,
        folder: Option<crate::menu::model::Folder>,
        category: [crate::menu::model::Category; 2],
    },
    InventorySurface {
        mode: crate::menu::model::Mode,
        folder: Option<crate::menu::model::Folder>,
        category: [crate::menu::model::Category; 2],
        needs_selection: bool,
    },
    Stats(super::stats::model::RowKey),
    StatsHint { hint: usize, status: usize, limb: Option<usize> },
    DataButton { kind: u8, list: usize, group: u32, key: Option<u32>, context: DataActionContext },
    Data {
        list: usize,
        key: u32,
    },
}
/// Semantic DATA button context; float coordinates use exact bit identities, never approximate hits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DataActionContext {
    pub selection: usize,
    pub key: Option<u32>,
    pub challenges: bool,
    pub filtered: bool,
    pub quest: Option<u32>,
    pub travel: Option<u32>,
    pub custom: Option<(u32, [u32; 3])>,
    pub spot: Option<(u32, [u32; 3])>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NativeTarget {
    menu: fnv_ui::TileId,
    tile: fnv_ui::TileId,
    region: Option<fnv_ui::input::Area>,
    row: Option<NativeRow>,
    pub selected: Option<bool>,
    pub valid: bool,
    control: bool,
}
impl NativeTarget {
    pub fn at(
        ui: &fnv_ui::Ui,
        menu: fnv_ui::TileId,
        point: Vec2,
        row: impl FnOnce(fnv_ui::TileId) -> Option<NativeRow>,
        selected: impl FnOnce(Option<NativeRow>) -> Option<bool>,
        valid: impl FnOnce(Option<NativeRow>) -> bool,
    ) -> Option<Self> {
        let hits = fnv_ui::input::hit_candidates(ui, menu, &fnv_ui::input::no_measure);
        let hit = fnv_ui::input::top_hit(ui, &hits, (f64::from(point.x), f64::from(point.y)))?;
        let region = hits
            .iter()
            .find(|h| h.under == hit.tile)
            .or_else(|| hits.iter().find(|h| h.under == hit.under))
            .map(|h| h.region());
        let row = row(hit.tile);
        Some(Self {
            menu,
            tile: hit.tile,
            region,
            row,
            selected: selected(row),
            valid: valid(row),
            control: false,
        })
    }
    /// The menu's real tile classifier marks its actual draggable marker only.
    pub fn with_control(mut self, is_marker: impl FnOnce(fnv_ui::TileId) -> bool) -> Self {
        self.control = is_marker(self.tile);
        self
    }
    pub fn control(&self) -> bool { self.control }
    pub fn same_target(&self, other: &Self) -> bool {
        self.valid
            && other.valid
            && self.menu == other.menu
            && self.tile == other.tile
            && self.region == other.region
            && self.row == other.row
            && self.control == other.control
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct PointerFrame {
    pub at: Option<Vec2>,
    pub started: bool,
    pub tap: bool,
    pub held: bool,
    pub scrolling: bool,
    pub native: bool,
    pub initially_selected: Option<bool>,
    pub control: bool,
}
impl PointerFrame {
    pub fn press(&self) -> bool { self.tap || (self.control && self.started && self.held) }
}
/// Native tile input has one pointer; a real mouse button keeps its own position/action.
pub fn native_frame(
    touch: Option<&super::touch_items::TouchItems>,
    mouse: &crate::pad::ActionButtons<'_, MouseButton>,
) -> PointerFrame {
    let hardware = [MouseButton::Left, MouseButton::Right, MouseButton::Middle]
        .into_iter()
        .any(|b| mouse.pressed(b) || mouse.just_pressed(b) || mouse.just_released(b));
    if hardware {
        PointerFrame::default()
    } else {
        touch
            .map(|t| t.pointer_frame())
            .filter(|f| f.native)
            .unwrap_or_default()
    }
}
#[derive(Clone, Copy, Debug)]
struct Stamp {
    page: Page,
    tab: usize,
    frame: Frame,
}
#[derive(Resource, Default)]
pub struct WheelFrame {
    stamp: Option<Stamp>,
    pub at: Option<Vec2>,
    pixels: f32,
}
impl WheelFrame {
    pub fn clear(&mut self) {
        *self = Self::default();
    }
    pub fn set(&mut self, pip: &PipBoy, at: Vec2, pixels: f32) {
        self.clear();
        if pip.open() && pixels.is_finite() && at.is_finite() {
            self.stamp = Some(Stamp {
                page: pip.page,
                tab: pip.tab(),
                frame: pip.frame,
            });
            self.at = Some(at);
            self.pixels = pixels.clamp(-256.0, 256.0);
        }
    }
    pub fn lines(&self, pip: &PipBoy) -> f32 {
        if self.stamp.is_some_and(|s| {
            pip.open() && s.page == pip.page && s.tab == pip.tab() && s.frame == pip.frame
        }) {
            self.pixels / 20.0
        } else {
            0.0
        }
    }
}

/// Select one real action owner: painted native tile target or hand-built Bevy node.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Routing<'w> {
    script: Option<Res<'w, crate::script::ScriptState>>,
    switch: Option<Res<'w, crate::tiles::vanilla::TileMenus>>,
    inventory: Option<Res<'w, crate::tiles::inventory_menu::InventoryTiles>>,
    stats: Option<Res<'w, crate::tiles::stats_menu::StatsTiles>>,
    data: Option<Res<'w, crate::tiles::map_menu::MapTiles>>,
    active: Option<Res<'w, crate::menu::ActiveMenu>>,
    repair: Option<Res<'w, super::repair::PipRepair>>,
    modding: Option<Res<'w, super::item_mod::PipItemMod>>,
    paused: Option<Res<'w, crate::pause::Paused>>,
    items_scroll: Option<Res<'w, super::scrollbar::ItemsScroll>>,
    stats_view: Option<Res<'w, super::stats::StatsView>>,
    data_state: Option<Res<'w, super::data::DataState>>,
}
impl Routing<'_> {
    pub fn inventory_model(&self)->Option<&crate::menu::model::MenuModel> {self.active.as_ref()?.0.as_ref()}
    /// Read the last painted hand-built view only when it still describes this
    /// actual model. A pending shared scroll request may precede the next paint.
    pub fn handbuilt_inventory_scroll(&self,pip:&PipBoy)->Option<usize> {
        if pip.page!=Page::Items || self.native_menu(pip).is_some() {return None}
        let model=self.inventory_model()?;
        self.items_scroll.as_ref().filter(|view|view.matches(model))?.position()
    }
    pub fn native_inventory_scroll(&self,pip:&PipBoy)->Option<f64> {
        if pip.page!=Page::Items {return None}
        self.native_menu(pip)?;
        self.inventory.as_ref().map(|t|t.view.value(crate::menu::model::Side::Player))
    }
    pub fn allows_navigation(&self,pip:&PipBoy)->bool {
        pip.phase==super::Phase::Up
            && !self.paused.as_ref().is_some_and(|p|p.0)
            && !self.active.as_ref().and_then(|a|a.0.as_ref()).is_some_and(|m|m.quantity.is_some())
            && !self.repair.as_ref().is_some_and(|r|r.is_open()) && !self.modding.as_ref().is_some_and(|m|m.is_open())
            && !self.inventory.as_ref().is_some_and(|t|t.modal || t.others)
            && !self.stats.as_ref().is_some_and(|t|t.modal || t.others)
            && !self.data.as_ref().is_some_and(|t|t.modal || t.others || t.popup_owns)
            && self.script.as_ref().and_then(|s|s.rt.as_ref()).is_none_or(|rt|rt.game.menu.is_none_or(|menu|menu==super::menu_code(pip.page,false,false)))
    }
    fn native_menu(&self, pip: &PipBoy) -> Option<(&fnv_ui::Ui,fnv_ui::TileId)> {
        let (game, switch) = self
            .script
            .as_ref()
            .and_then(|s| s.rt.as_ref())
            .map(|rt| &rt.game)
            .zip(self.switch.as_deref())
            .map(|(g,s)|(g,*s))?;
        let paused = self.paused.as_ref().is_some_and(|p| p.0);
        let menu = match pip.page {
            Page::Items => self.inventory.as_ref().filter(|t| !t.others).and_then(|t| {
                crate::tiles::inventory_menu::takes(
                    t,
                    switch,
                    pip,
                    game,
                    paused,
                    crate::tiles::inventory_menu::lists_closed(
                        self.repair.as_deref(),
                        self.modding.as_deref(),
                    ),
                    self.active
                        .as_ref()
                        .and_then(|a| a.0.as_ref())
                        .is_some_and(|m| m.quantity.is_some()),
                )
            }),
            Page::Stats => self
                .stats
                .as_ref()
                .filter(|t| !t.others)
                .and_then(|t| crate::tiles::stats_menu::takes(t, switch, pip, game, paused)),
            Page::Data => self
                .data
                .as_ref()
                .filter(|t| !t.others && !t.popup_owns)
                .and_then(|t| crate::tiles::map_menu::takes(t, switch, pip, game, paused)),
        };
        let menu = menu?;
        Some((&game.ui_tree,menu))
    }
    pub fn navigation_regions(&self,pip:&PipBoy)->Vec<super::tab_swipe::Region> {
        let Some((ui,_))=self.native_menu(pip) else {return Vec::new()};
        match pip.page {
            Page::Items=>self.inventory.as_ref().map(|t|crate::tiles::inventory_menu::touch_navigation(t,ui)),
            Page::Stats=>self.stats.as_ref().map(|t|crate::tiles::stats_menu::touch_navigation(t,ui)),
            Page::Data=>self.data.as_ref().map(|t|crate::tiles::map_menu::touch_navigation(t,ui)),
        }.unwrap_or_default()
    }
    pub fn native_at(&self, pip: &PipBoy, at: Option<Vec2>) -> Option<NativeTarget> {
        let at=at?;
        let (ui,menu)=self.native_menu(pip)?;
        match pip.page {
            Page::Items => crate::tiles::inventory_menu::touch_target(
                self.inventory.as_deref()?,
                ui,
                menu,
                at,
                self.active.as_ref()?.0.as_ref()?,
            ),
            Page::Stats => crate::tiles::stats_menu::touch_target(
                self.stats.as_deref()?,
                ui,
                menu,
                at,
                self.stats_view.as_deref()?,
            ),
            Page::Data => {
                crate::tiles::map_menu::touch_target(self.data.as_deref()?, ui, menu, at, self.data_state.as_deref()?)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::{
        model::{Category, Command, MenuModel, Side},
        ActiveMenu, MenuInput,
    };
    use crate::pipboy::{self, touch_items::TouchItems};
    use bevy::{
        input::{
            touch::{TouchInput, TouchPhase},
            InputPlugin,
        },
        picking::{
            backend::{HitData, PointerHits},
            pointer::{PointerAction, PointerId, PointerInput, PointerLocation},
            InteractionPlugin, PickingPlugin, PickingSystems,
        },
    };
    #[derive(Component)]
    struct HitRect(Rect);
    #[derive(Resource, Default)]
    struct Seen {
        scrolls: usize,
        releases: usize,
        cancels: usize,
        tabs: usize,
    }
    fn backend(
        pointers: Query<(&PointerId, &PointerLocation)>,
        targets: Query<(Entity, &HitRect)>,
        mut hits: MessageWriter<PointerHits>,
    ) {
        for (id, location) in &pointers {
            if *id != pipboy::touch_items::POINTER {
                continue;
            }
            let Some(at) = location.location() else {
                continue;
            };
            let picks = targets
                .iter()
                .filter(|(_, r)| r.0.contains(at.position))
                .map(|(e, _)| (e, HitData::new(Entity::PLACEHOLDER, 0.0, None, None)))
                .collect();
            hits.write(PointerHits::new(*id, picks, 0.0));
        }
    }
    fn project(mut touch: ResMut<TouchItems>) {
        touch.project(Some);
    }
    fn apply(
        mut input: MessageReader<MenuInput>,
        mut active: ResMut<ActiveMenu>,
        mut sim: ResMut<crate::sim::SimState>,
    ) {
        for cmd in input.read() {
            if let Some(m) = active.0.as_mut() {
                m.apply(&mut sim.world, cmd.0);
            }
        }
    }
    fn observe(
        mut pointer: MessageReader<PointerInput>,
        mut tabs: MessageReader<pipboy::PipBoyInput>,
        mut seen: ResMut<Seen>,
    ) {
        for p in pointer
            .read()
            .filter(|p| p.pointer_id == pipboy::touch_items::POINTER)
        {
            match p.action {
                PointerAction::Scroll { y, .. } if y != 0.0 => seen.scrolls += 1,
                PointerAction::Release(_) => seen.releases += 1,
                PointerAction::Cancel => seen.cancels += 1,
                _ => {}
            }
        }
        seen.tabs += tabs.read().count();
    }
    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            InputPlugin,
            PickingPlugin,
            InteractionPlugin,
        ));
        app.init_resource::<TouchItems>()
            .init_resource::<WheelFrame>()
            .init_resource::<crate::bindings::Bindings>()
            .init_resource::<pipboy::PipBoyAid>()
            .init_resource::<pipboy::PointerMoved>()
            .init_resource::<pipboy::repair::PipRepair>()
            .init_resource::<pipboy::item_mod::PipItemMod>()
            .init_resource::<pipboy::KeysTaken>()
            .add_systems(First, pipboy::clear_keys_taken)
            .init_resource::<pipboy::hotkey_panel::HotkeyHold>()
            .init_resource::<crate::hotkeys::Hotkeys>()
            .init_resource::<Seen>()
            .add_message::<bevy::window::WindowEvent>()
            .add_message::<MenuInput>()
            .add_message::<pipboy::PipBoyInput>();
        let mut sim = crate::sim::test_world::sim(
            (0..12)
                .map(|i| {
                    let mut b = crate::sim::test_world::item_base(0xF100 + i);
                    b.name = Some(format!("Authored{i:02}"));
                    b
                })
                .collect(),
            vec![],
        );
        for i in 0..12 {
            sim.world
                .add_item(fnv_sim::FormId::PLAYER_REF, fnv_sim::FormId(0xF100 + i), 3)
                .unwrap();
        }
        let mut menu = MenuModel::open_inventory(&mut sim.world).unwrap();
        menu.apply(
            &mut sim.world,
            Command::SetCategory(Side::Player, Category::Aid),
        );
        app.insert_resource(sim)
            .insert_resource(ActiveMenu(Some(menu)))
            .insert_resource(pipboy::PipBoy {
                phase: pipboy::Phase::Up,
                page: Page::Items,
                tabs: [0, 2, 0],
                ..default()
            });
        let camera = app.world_mut().spawn_empty().id();
        app.insert_resource(pipboy::PipBoyScreen {
            image: Handle::default(),
            camera,
            on_model: false,
            pointer: None,
            scale: 1.0,
        });
        app.world_mut().spawn(pipboy::touch_items::POINTER);
        app.add_plugins(TouchPointerPlugin)
            .add_systems(PreUpdate, backend.in_set(PickingSystems::Backend))
            .add_systems(
                Update,
                (pipboy::apply_input, pipboy::items::keys, apply, observe).chain(),
            )
            .add_systems(Last, project);
        app.update();
        app
    }
    fn event(app: &mut App, phase: TouchPhase, p: Vec2, now: u64) {
        assert!(app.world_mut().resource_mut::<TouchItems>().event(
            &TouchInput {
                phase,
                position: p,
                window: Entity::PLACEHOLDER,
                id: 7,
                force: None
            },
            now
        ));
    }
    fn flush(app: &mut App) {
        for _ in 0..5 {
            app.update();
        }
    }
    #[test]
    fn actual_projected_vertical_swipe_emits_scroll_and_changes_native_inventory_selection_without_assignment_or_consumption(
    ) {
        let mut a = app();
        let p = Vec2::new(150., 200.);
        event(&mut a, TouchPhase::Started, p, 0);
        event(&mut a, TouchPhase::Moved, p - Vec2::Y * 60., 50);
        event(&mut a, TouchPhase::Ended, p - Vec2::Y * 60., 100);
        flush(&mut a);
        let menu = a.world().resource::<ActiveMenu>().0.as_ref().unwrap();
        assert_eq!(menu.selected[Side::Player as usize], 3);
        assert!(menu.navigated > 0);
        assert!(a.world().resource::<Seen>().scrolls > 0);
        assert_eq!(a.world().resource::<Seen>().releases, 0);
        assert!(a
            .world()
            .resource::<crate::hotkeys::Hotkeys>()
            .0
            .iter()
            .all(Option::is_none));
        for i in 0..12 {
            assert_eq!(
                a.world_mut()
                    .resource_mut::<crate::sim::SimState>()
                    .world
                    .item_count(fnv_sim::FormId::PLAYER_REF, fnv_sim::FormId(0xF100 + i))
                    .unwrap(),
                3
            );
        }
    }
    #[test]
    fn queued_touch_tap_reaches_real_bevy_tab_click_once_but_vertical_out_and_back_never_changes_tabs(
    ) {
        for scroll in [false, true] {
            let mut a = app();
            a.world_mut()
                .spawn((
                    HitRect(Rect::from_corners(
                        Vec2::new(100., 100.),
                        Vec2::new(300., 160.),
                    )),
                    pipboy::Clickable(pipboy::PipBoyInput::Tab(4)),
                ))
                .observe(pipboy::on_click);
            let p = Vec2::new(150., 130.);
            event(&mut a, TouchPhase::Started, p, 0);
            if scroll {
                event(&mut a, TouchPhase::Moved, p - Vec2::Y * 40., 30);
            }
            event(&mut a, TouchPhase::Ended, p, 80);
            flush(&mut a);
            assert_eq!(
                a.world().resource::<pipboy::PipBoy>().tab(),
                if scroll { 2 } else { 4 }
            );
            assert_eq!(a.world().resource::<Seen>().tabs, usize::from(!scroll));
        }
    }
    #[test]
    fn capture_and_session_changes_drop_queued_swipe_and_release_without_touching_physical_keys() {
        for cause in 0..4 {
            let mut a = app();
            let p = Vec2::new(150., 200.);
            event(&mut a, TouchPhase::Started, p, 0);
            a.update();
            a.update();
            event(&mut a, TouchPhase::Moved, p - Vec2::Y * 60., 50);
            event(&mut a, TouchPhase::Ended, p - Vec2::Y * 60., 80);
            a.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::KeyF);
            match cause {
                0 => {
                    a.insert_resource(pipboy::touch_items::OverlayCapture(true));
                }
                1 => {
                    a.world_mut().resource_mut::<pipboy::PipBoy>().frame = Frame::Screen;
                }
                2 => {
                    a.world_mut().resource_mut::<pipboy::PipBoy>().phase = pipboy::Phase::Lowering;
                }
                _ => {
                    a.init_resource::<crate::pad::PlatformInputGate>();
                    a.world_mut()
                        .resource_mut::<crate::pad::PlatformInputGate>()
                        .set_blocked(true);
                }
            }
            flush(&mut a);
            assert_eq!(
                a.world()
                    .resource::<ActiveMenu>()
                    .0
                    .as_ref()
                    .unwrap()
                    .selected[Side::Player as usize],
                0
            );
            assert_eq!(a.world().resource::<Seen>().scrolls, 0);
            assert!(a
                .world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::KeyF));
            assert_eq!(
                a.world()
                    .resource::<WheelFrame>()
                    .lines(a.world().resource::<pipboy::PipBoy>()),
                0.0
            );
        }
    }
    #[test]
    fn touch_wheel_is_page_tab_and_frame_stamped_and_clears_without_replay() {
        let mut pip = pipboy::PipBoy {
            phase: pipboy::Phase::Up,
            page: Page::Items,
            ..default()
        };
        let mut w = WheelFrame::default();
        w.set(&pip, Vec2::ONE, -60.);
        assert_eq!(w.lines(&pip), -3.0);
        pip.apply(pipboy::PipBoyInput::StepTab(1));
        assert_eq!(w.lines(&pip), 0.0);
        pip.apply(pipboy::PipBoyInput::StepTab(-1));
        pip.frame = Frame::Screen;
        assert_eq!(w.lines(&pip), 0.0);
        w.clear();
        assert_eq!(w.lines(&pip), 0.0);
    }

    #[test]
    fn armed_handbuilt_row_uses_real_press_and_click_observers_without_consumption() {
        fn propose(mut touch: ResMut<TouchItems>, active: Res<ActiveMenu>, mut keys: ResMut<crate::hotkeys::Hotkeys>, mut sim: ResMut<crate::sim::SimState>) {
            touch.assign_pending(&active, &mut keys, &mut sim.world);
        }
        let mut a = app();
        a.add_systems(PostUpdate, propose);
        let entity = pipboy::items::binding_fixture_row(a.world_mut(), 0);
        a.world_mut().entity_mut(entity).insert(HitRect(Rect::new(100.0, 100.0, 300.0, 180.0)));
        assert!(a.world_mut().resource_mut::<TouchItems>().arm_binding(0));
        event(&mut a, TouchPhase::Started, Vec2::new(150.0, 130.0), 0);
        event(&mut a, TouchPhase::Ended, Vec2::new(150.0, 130.0), 80);
        flush(&mut a);
        assert!(a.world().resource::<TouchItems>().binding_ready());
        assert!(a.world().resource::<crate::hotkeys::Hotkeys>().0.iter().all(Option::is_none));
        assert_eq!(a.world_mut().resource_mut::<crate::sim::SimState>().world.item_count(fnv_sim::FormId::PLAYER_REF, fnv_sim::FormId(0xF100)).unwrap(), 3);
        let confirmed = a.world_mut().resource_scope(|world, mut touch: Mut<TouchItems>| {
            world.resource_scope(|world, mut keys: Mut<crate::hotkeys::Hotkeys>| {
                world.resource_scope(|world,mut sim:Mut<crate::sim::SimState>| {
                    touch.confirm_binding(world.resource::<ActiveMenu>(), &mut keys,&mut sim.world)
                })
            })
        });
        assert!(confirmed);
        assert_eq!(a.world().resource::<crate::hotkeys::Hotkeys>().0[0].unwrap().item, 0xF100);
        assert_eq!(a.world_mut().resource_mut::<crate::sim::SimState>().world.item_count(fnv_sim::FormId::PLAYER_REF, fnv_sim::FormId(0xF100)).unwrap(), 3);
    }
}
