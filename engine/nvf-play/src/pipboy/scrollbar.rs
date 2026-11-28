//! One late-checked list-scroll path for actual bars and projected device controls.
use super::{Frame, Page, PipBoy, PipBoyScreen};
use crate::menu::{model::{Category, Folder, Mode, Side}, ActiveMenu};
use bevy::{ecs::system::SystemState, prelude::*};
use bevy::picking::{events::{PointerClick, PointerPress}, pointer::{PointerButton, PointerId}};

#[derive(Clone, Copy, Debug)]
pub enum ScrollBy { Lines(i32), Pages(i32), ToFraction(f32) }
impl ScrollBy {
    pub(crate) fn value(self, now: f64, most: f64, page: f64) -> Option<f64> {
        let value = match self {
            Self::Lines(n) => now + f64::from(n),
            Self::Pages(n) => now + f64::from(n) * page.max(1.0),
            Self::ToFraction(p) if p.is_finite() => f64::from(p.clamp(0.0, 1.0)) * most,
            Self::ToFraction(_) => return None,
        };
        (now.is_finite() && most.is_finite() && most >= 0.0 && page.is_finite())
            .then(|| value.round().clamp(0.0, most))
    }
}

/// Capture at the owned contact; queue `apply` on Commands for a single authority.
#[derive(Clone, Copy)]
pub struct ListScrollRequest {
    page: Page, tab: usize, frame: Frame,
    image: bevy::asset::AssetId<Image>, on_model: bool, scale: u32,
    contact: Option<u64>,
    by: ScrollBy,
}
impl ListScrollRequest {
    pub fn new(pip: &PipBoy, screen: &PipBoyScreen, by: ScrollBy) -> Option<Self> {
        (pip.phase == super::Phase::Up && pip.open() && !matches!(by, ScrollBy::ToFraction(p) if !p.is_finite())).then(|| Self {
            page: pip.page, tab: pip.tab(), frame: pip.frame, image: screen.image.id(),
            on_model: screen.on_model, scale: screen.scale.to_bits(), contact: None, by,
        })
    }
    pub fn with_contact(mut self, id: u64) -> Self { self.contact = Some(id); self }
    fn matches(self, pip: &PipBoy, screen: &PipBoyScreen) -> bool {
        pip.phase == super::Phase::Up && pip.open() && pip.page == self.page && pip.tab() == self.tab && pip.frame == self.frame
            && screen.image.id() == self.image && screen.on_model == self.on_model && screen.scale.to_bits() == self.scale
    }
    pub fn apply(self, world: &mut World) -> bool {
        let mut admission = SystemState::<super::touch_items::Admission>::new(world);
        if !admission.get(world).is_ok_and(|a| a.allows_contact(self.contact)) || world.get_resource::<crate::pause::Paused>().is_some_and(|p| p.0) { return false; }
        let Some(pip) = world.get_resource::<PipBoy>() else { return false };
        let Some(_) = world.get_resource::<PipBoyScreen>().filter(|s| self.matches(pip,s)) else { return false };
        let native = native_list(world, pip);
        if native == NativeList::Blocked { return false; }
        match (self.page, native) {
            (Page::Items, NativeList::Native) => {
                let Some(mut tiles) = world.get_resource_mut::<crate::tiles::inventory_menu::InventoryTiles>() else { return false };
                let (now, most, page) = tiles.view.scroll_extent(Side::Player);
                self.by.value(now, most, page).is_some_and(|to| tiles.view.scroll_to(Side::Player, to))
            }
            (Page::Data, NativeList::Native) => world.get_resource_mut::<crate::tiles::map_menu::MapTiles>()
                .is_some_and(|mut tiles| tiles.scroll_list_by(self.by)),
            (Page::Stats, _) => world.get_resource_mut::<super::stats::StatsView>().is_some_and(|mut view| {
                let now = view.top.get(self.tab).copied().unwrap_or(0);
                let most = view.list_most(self.tab);
                self.by.value(now as f64, most as f64, view.list_page(self.tab) as f64).is_some_and(|to| {
                    view.scroll(self.tab, to as i32 - now as i32); to as usize != now
                })
            }),
            (Page::Items, NativeList::Handbuilt) => {
                let valid = world.get_resource::<ActiveMenu>().and_then(|m|m.0.as_ref()).is_some_and(|m| m.mode == Mode::Inventory && m.quantity.is_none()
                    && world.get_resource::<ItemsScroll>().is_some_and(|s| s.matches(m)));
                if !valid { return false; }
                world.get_resource_mut::<ItemsScroll>().is_some_and(|mut view| view.scroll(self.by))
            }
            (Page::Data, NativeList::Handbuilt) => world.get_resource_mut::<super::data::DataState>()
                .is_some_and(|mut view| view.scroll_list_by(self.tab, self.by)),
            _ => false,
        }
    }
}

#[derive(PartialEq, Eq)]
enum NativeList { Native, Handbuilt, Blocked }
fn native_list(world: &World, pip: &PipBoy) -> NativeList {
    if world.get_resource::<super::repair::PipRepair>().is_some_and(|r| r.is_open())
        || world.get_resource::<super::item_mod::PipItemMod>().is_some_and(|r| r.is_open()) { return NativeList::Blocked; }
    if world.get_resource::<super::data::DataState>().is_some_and(|s| s.popup().is_some()) { return NativeList::Blocked; }
    let Some(game) = world.get_resource::<crate::script::ScriptState>().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game) else { return NativeList::Handbuilt };
    if crate::tiles::stats_menu::modal_over(game.menu) { return NativeList::Blocked; }
    let switch = world.get_resource::<crate::tiles::vanilla::TileMenus>().copied().unwrap_or_default();
    match pip.page {
        Page::Items => {
            let Some(tiles) = world.get_resource::<crate::tiles::inventory_menu::InventoryTiles>().filter(|t| switch.inventory && t.shown.is_some()) else { return NativeList::Handbuilt };
            let quantity = world.get_resource::<ActiveMenu>().and_then(|a| a.0.as_ref()).is_some_and(|m| m.quantity.is_some());
            if !tiles.others && crate::tiles::inventory_menu::takes(tiles, switch, pip, game, false, true, quantity).is_some() { NativeList::Native } else { NativeList::Blocked }
        }
        Page::Stats => {
            let Some(tiles) = world.get_resource::<crate::tiles::stats_menu::StatsTiles>().filter(|t| switch.stats && t.shown.is_some()) else { return NativeList::Handbuilt };
            if !tiles.others && crate::tiles::stats_menu::takes(tiles, switch, pip, game, false).is_some() { NativeList::Native } else { NativeList::Blocked }
        }
        Page::Data => {
            let Some(tiles) = world.get_resource::<crate::tiles::map_menu::MapTiles>().filter(|t| switch.map && t.shown.is_some()) else { return NativeList::Handbuilt };
            if !tiles.others && !tiles.popup_owns && crate::tiles::map_menu::takes(tiles, switch, pip, game, false).is_some() { NativeList::Native } else { NativeList::Blocked }
        }
    }
}

/// Hand-built ITEMS keeps a view scroll independent of its selected gameplay row.
#[derive(Resource, Default)]
pub struct ItemsScroll {
    context: Option<(Category, Option<Folder>, usize)>,
    selected: usize, first: usize, most: usize, page: usize, manual: bool,
}
impl ItemsScroll {
    pub fn position(&self) -> Option<usize> { self.context.map(|_| self.first) }
    pub fn matches(&self, model: &crate::menu::model::MenuModel) -> bool {
        self.context == Some((model.category[Side::Player as usize], model.folder, model.list(Side::Player).rows.len()))
    }
    pub(crate) fn resolve(&mut self, model: &crate::menu::model::MenuModel, heights: &[f32], room: f32) -> usize {
        let next = Some((model.category[Side::Player as usize], model.folder, heights.len()));
        let selected = model.selected[Side::Player as usize];
        if self.context != next { self.manual = false; }
        if self.selected != selected && !model.pointer_pick { self.manual = false; }
        let fallback = model.scroll[Side::Player as usize];
        self.most = heights.len();
        let mut used = 0.0;
        while self.most > 0 && used + heights[self.most-1] <= room { self.most -= 1; used += heights[self.most]; }
        self.most = self.most.min(heights.len().saturating_sub(1));
        if !self.manual {
            self.first = if model.pointer_pick { fallback } else { super::items::first_shown(heights, fallback, selected, room) };
        }
        self.first = self.first.min(self.most);
        self.context = next;
        self.selected = selected;
        self.page = 0;
        used = 0.0;
        for height in &heights[self.first..] {
            if used + height > room { break; }
            self.page += 1; used += height;
        }
        self.first
    }
    fn scroll(&mut self, by: ScrollBy) -> bool {
        let Some(to) = by.value(self.first as f64, self.most as f64, self.page as f64).filter(|_| self.context.is_some()) else { return false };
        self.manual = true;
        let moved = self.first != to as usize;
        self.first = to as usize;
        moved
    }
}

pub(crate) struct ScrollbarPlugin;
impl Plugin for ScrollbarPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DragState>().add_systems(Update, advance_drag.after(super::PointerSet).before(crate::PlaySet::Activation));
    }
}
#[derive(Clone, Copy)]
enum Piece { Lines(i32), Pages(i32), Marker }
#[derive(Component, Clone, Copy)]
struct Hit { page: Page, tab: usize, piece: Piece, track: Rect, marker: Rect }
#[derive(Clone, Copy)]
struct Drag { request: ListScrollRequest, pointer: PointerId, track: Rect, marker_height: f32, grab: f32 }
#[derive(Resource, Default)]
struct DragState(Option<Drag>);

/// Hit geometry is the same track/marker that the view paints; offset maps its
/// local parent coordinates to the Pip-Boy texture's pointer coordinates.
pub(crate) fn hits(p: &mut ChildSpawnerCommands, page: Page, tab: usize, outer: Rect, track: Rect, marker: Rect, offset: Vec2) {
    let mut pieces = vec![
        (Rect::new(outer.min.x, outer.min.y, outer.max.x, track.min.y), Piece::Lines(-1)),
        (Rect::new(outer.min.x, track.max.y, outer.max.x, outer.max.y), Piece::Lines(1)),
        (Rect::new(outer.min.x, track.min.y, outer.max.x, marker.min.y), Piece::Pages(-1)),
        (Rect::new(outer.min.x, marker.max.y, outer.max.x, track.max.y), Piece::Pages(1)),
    ];
    pieces.push((Rect::new(outer.min.x, marker.min.y, outer.max.x, marker.max.y), Piece::Marker));
    for (rect, piece) in pieces.into_iter().filter(|(r, _)| r.width() > 0.0 && r.height() > 0.0) {
        let hit = Hit { page, tab, piece, track: Rect::from_corners(track.min+offset, track.max+offset), marker: Rect::from_corners(marker.min+offset, marker.max+offset) };
        p.spawn((crate::interface::at(rect.min.x, rect.min.y, rect.width(), rect.height()), hit, Pickable::default()))
            .observe(on_press).observe(on_click);
    }
}
fn on_press(e: On<PointerPress>, hits: Query<&Hit>, (pip, screen): (Res<PipBoy>, Option<Res<PipBoyScreen>>),
    (mut drag, mut touch): (ResMut<DragState>, Option<ResMut<super::touch_items::TouchItems>>), admission: super::touch_items::Admission) {
    if e.button != PointerButton::Primary || !admission.allows() { return; }
    let (Ok(hit), Some(screen)) = (hits.get(e.entity), screen) else { return };
    if !e.pointer.position.is_finite() || bevy::camera::RenderTarget::Image(screen.target()).normalize(None).as_ref() != Some(&e.pointer.target) { return; }
    if hit.page != pip.page || hit.tab != pip.tab() || !matches!(hit.piece, Piece::Marker) { return; }
    if e.pointer.id != super::PIPBOY_POINTER && e.pointer.id != super::touch_items::POINTER { return; }
    let Some(mut request) = ListScrollRequest::new(&pip, &screen, ScrollBy::Lines(0)) else { return };
    if e.pointer.id == super::touch_items::POINTER {
        let Some(touch) = touch.as_deref_mut().filter(|t| admission.allows_contact(t.contact_id())) else { return };
        if let Some(id) = touch.contact_id() { request = request.with_contact(id); }
        touch.claim_control();
    }
    drag.0 = Some(Drag { request, pointer: e.pointer.id, track: hit.track, marker_height: hit.marker.height(), grab: e.pointer.position.y - hit.marker.min.y });
}
fn on_click(e: On<PointerClick>, hits: Query<&Hit>, (pip, screen): (Res<PipBoy>, Option<Res<PipBoyScreen>>),
    touch: Option<Res<super::touch_items::TouchItems>>, admission: super::touch_items::Admission, mut commands: Commands) {
    if e.button != PointerButton::Primary || !admission.allows() { return; }
    if e.pointer.id == super::touch_items::POINTER && touch.as_ref().is_none_or(|t| !admission.allows_contact(t.contact_id()) || t.suppress_click()) { return; }
    let (Ok(hit), Some(screen)) = (hits.get(e.entity), screen) else { return };
    if !e.pointer.position.is_finite() || bevy::camera::RenderTarget::Image(screen.target()).normalize(None).as_ref() != Some(&e.pointer.target) { return; }
    if hit.page != pip.page || hit.tab != pip.tab() { return; }
    let by = match hit.piece { Piece::Lines(n) => ScrollBy::Lines(n), Piece::Pages(n) => ScrollBy::Pages(n), Piece::Marker => return };
    if let Some(mut request) = ListScrollRequest::new(&pip, &screen, by) {
        if e.pointer.id == super::touch_items::POINTER {
            if let Some(id) = touch.as_ref().and_then(|t|t.contact_id()) { request = request.with_contact(id); }
        }
        commands.queue(move |world: &mut World| { request.apply(world); });
    }
}
fn advance_drag(mut state: ResMut<DragState>, touch: Option<Res<super::touch_items::TouchItems>>, (pip,screen): (Res<PipBoy>,Option<Res<PipBoyScreen>>),
    mouse: Option<Res<ButtonInput<MouseButton>>>, admission: super::touch_items::Admission, mut commands: Commands) {
    let Some(mut drag) = state.0 else { return };
    if screen.as_ref().is_none_or(|s| !drag.request.matches(&pip,s)) { state.0 = None; return; }
    let (held, at) = if drag.pointer == super::touch_items::POINTER {
        let frame = touch.as_ref().map(|t| t.pointer_frame()).unwrap_or_default();
        (frame.held && frame.control && admission.allows_contact(touch.as_ref().and_then(|t| t.contact_id())), frame.at)
    } else { (mouse.as_ref().is_some_and(|m|m.pressed(MouseButton::Left)), screen.as_ref().and_then(|s|s.pointer)) };
    let Some(at) = at.filter(|p| p.is_finite() && held && admission.allows()) else { state.0 = None; return };
    let travel = drag.track.height() - drag.marker_height;
    if travel <= 0.0 { state.0 = None; return; }
    drag.request.by = ScrollBy::ToFraction(((at.y - drag.grab - drag.track.min.y) / travel).clamp(0.0, 1.0));
    commands.queue(move |world: &mut World| { drag.request.apply(world); });
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::picking::{backend::HitData, events::Pointer, pointer::Location};
    use fnv_sim::FormId;

    fn app() -> App {
        let mut sim=crate::sim::test_world::sim((1..=12).map(|i| {
            let mut base=crate::sim::test_world::item_base(0xF300+i);
            base.name=Some(format!("Item {i:02}"));base
        }).collect(),vec![]);
        for i in 1..=12 {sim.world.add_item(FormId::PLAYER_REF,FormId(0xF300+i),3).unwrap();}
        let mut model=crate::menu::model::MenuModel::open_inventory(&mut sim.world).unwrap();
        model.apply(&mut sim.world,crate::menu::model::Command::SetCategory(Side::Player,Category::Aid));
        assert_eq!(model.list(Side::Player).rows.len(),12,"the real menu must actually list the twelve authored fixture items");
        let mut scroll=ItemsScroll::default();assert_eq!(scroll.resolve(&model,&[40.0;12],200.0),0);
        let pip=PipBoy{phase:super::super::Phase::Up,page:Page::Items,tabs:[0,2,0],..default()};
        let mut app=App::new();app.add_plugins((MinimalPlugins,ScrollbarPlugin)).insert_resource(pip)
            .insert_resource(ActiveMenu(Some(model))).insert_resource(scroll).insert_resource(sim)
            .init_resource::<ButtonInput<MouseButton>>().init_resource::<ButtonInput<KeyCode>>();
        let camera=app.world_mut().spawn_empty().id();
        app.insert_resource(PipBoyScreen{image:Handle::default(),camera,on_model:false,pointer:Some(Vec2::new(16.0,40.0)),scale:1.0});
        fn paint(mut commands:Commands) {
            commands.spawn_empty().with_children(|p| {
                let (top,h)=super::super::data::bar_marker(200.0,0.0,5.0/12.0);
                hits(p,Page::Items,2,Rect::new(0.0,0.0,32.0,200.0),Rect::new(0.0,20.0,32.0,180.0),Rect::new(0.0,top,32.0,top+h),Vec2::ZERO);
            });
        }
        app.add_systems(Startup,paint);app.update();app
    }
    fn capture(app:&App,by:ScrollBy)->Option<ListScrollRequest>{
        ListScrollRequest::new(app.world().resource::<PipBoy>(),app.world().resource::<PipBoyScreen>(),by)
    }

    #[test]
    fn actual_handbuilt_marker_press_drag_moves_same_visible_list_without_changing_selected_item() {
        let mut app=app();
        let (entity,hit)={let world=app.world_mut();let mut q=world.query::<(Entity,&Hit)>();q.iter(world).find(|(_,h)|matches!(h.piece,Piece::Marker)).map(|(e,h)|(e,*h)).unwrap()};
        let screen=app.world().resource::<PipBoyScreen>();
        let target=bevy::camera::RenderTarget::Image(screen.target()).normalize(None).unwrap();
        app.world_mut().trigger(PointerPress{entity,pointer:Pointer::new(super::super::PIPBOY_POINTER,Location{target,position:hit.marker.center()}),button:PointerButton::Primary,hit:HitData::new(Entity::PLACEHOLDER,0.0,None,None),count:1});
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.world_mut().resource_mut::<PipBoyScreen>().pointer=Some(Vec2::new(16.0,180.0-hit.marker.height()*0.5));
        app.update();
        assert_eq!(app.world().resource::<ItemsScroll>().position(),Some(7));
        let active=app.world_mut().remove_resource::<ActiveMenu>().unwrap();
        let model=active.0.as_ref().unwrap();
        assert_eq!(model.selected[Side::Player as usize],0,"thumb drag is view movement, not item selection/use");
        let first=app.world_mut().resource_mut::<ItemsScroll>().resolve(model,&[40.0;12],200.0);
        assert_eq!(first,7,"actual handbuilt draw resolver retains the scroll instead of jumping back to selected row0");
        let (top,height)=super::super::data::bar_marker(200.0,first as f32/12.0,5.0/12.0);
        assert!((top+height-180.0).abs()<0.001,"the same painter places the thumb at the actual list's end");
        app.insert_resource(active);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);app.update();
        assert!(app.world().resource::<DragState>().0.is_none());
        assert_eq!(app.world_mut().resource_mut::<crate::sim::SimState>().world.item_count(FormId::PLAYER_REF,FormId(0xF301)).unwrap(),3);
    }

    #[test]
    fn actual_late_list_request_clamps_extent_pages_and_rejects_modal_controller_image_and_platform_changes() {
        let mut app=app();
        assert!(capture(&app,ScrollBy::ToFraction(f32::NAN)).is_none());
        let request=capture(&app,ScrollBy::Pages(1)).unwrap();assert!(request.apply(app.world_mut()));
        assert_eq!(app.world().resource::<ItemsScroll>().position(),Some(5));
        assert!(capture(&app,ScrollBy::ToFraction(2.0)).unwrap().apply(app.world_mut()));
        assert_eq!(app.world().resource::<ItemsScroll>().position(),Some(7));
        let request=capture(&app,ScrollBy::Lines(-1)).unwrap();
        app.insert_resource(crate::pause::Paused(true));assert!(!request.apply(app.world_mut()));
        app.insert_resource(crate::pause::Paused(false));
        let mut bindings=crate::bindings::Bindings::default();bindings.device=crate::bindings::InputDevice::Controller;
        app.insert_resource(bindings);assert!(!request.apply(app.world_mut()));
        app.world_mut().resource_mut::<crate::bindings::Bindings>().device=crate::bindings::InputDevice::KeyboardMouse;
        app.world_mut().resource_mut::<PipBoyScreen>().scale=2.0;assert!(!request.apply(app.world_mut()));
        app.world_mut().resource_mut::<PipBoyScreen>().scale=1.0;
        let image=app.world().resource::<PipBoyScreen>().image.clone();
        let replacement=Assets::<Image>::default().add(Image::default());
        app.world_mut().resource_mut::<PipBoyScreen>().image=replacement;assert!(!request.apply(app.world_mut()));
        app.world_mut().resource_mut::<PipBoyScreen>().image=image;
        app.world_mut().resource_mut::<PipBoy>().phase=super::super::Phase::Down;assert!(!request.apply(app.world_mut()));
        app.world_mut().resource_mut::<PipBoy>().phase=super::super::Phase::Up;
        let mut gate=crate::pad::PlatformInputGate::default();gate.set_blocked(true);app.insert_resource(gate);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
        assert!(!request.with_contact(99).apply(app.world_mut()));
        assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::KeyW),"late rejection never clears unrelated hardware");
        assert_eq!(app.world().resource::<ItemsScroll>().position(),Some(7));
        assert_eq!(app.world().resource::<ActiveMenu>().0.as_ref().unwrap().selected[Side::Player as usize],0);
        app.world_mut().resource_mut::<crate::pad::PlatformInputGate>().set_blocked(false);
        assert!(request.with_contact(100).apply(app.world_mut()),"a fresh eligible control resumes after gates clear");
        assert_eq!(app.world().resource::<ItemsScroll>().position(),Some(6));
    }

    #[test]
    fn actual_stats_list_scroll_request_pages_and_drags_view_without_moving_selection() {
        let mut app=app();
        {let mut pip=app.world_mut().resource_mut::<PipBoy>();pip.page=Page::Stats;pip.tabs[0]=2;}
        let mut stats=super::super::stats::StatsView::with_lens([12;5]);stats.set_rows_shown(2,5);
        app.insert_resource(stats);
        assert!(capture(&app,ScrollBy::Pages(1)).unwrap().apply(app.world_mut()));
        assert_eq!(app.world().resource::<super::super::stats::StatsView>().top[2],5);
        assert!(capture(&app,ScrollBy::ToFraction(1.0)).unwrap().apply(app.world_mut()));
        let stats=app.world().resource::<super::super::stats::StatsView>();
        assert_eq!((stats.top[2],stats.selected[2]),(7,0));
    }
}
