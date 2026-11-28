//! The menus' three-state pointer feedback (none, hovered, pressed), kept by NVForever.
//!
//! Every menu reads a [`PointerState`] on its clickable nodes, mostly through
//! `Changed<PointerState>`. Bevy 0.20 deprecated its own `Interaction` and
//! `widget::Button` (gone in 0.21); their replacements (`Hovered`, the `Pressed` marker,
//! the headless `ui_widgets::Button`) activate a button on release and carry no
//! three-state value, which would change when every menu's clicks fire. So the state is
//! ours, and [`update_pointer_state`] keeps it with the timing the menus were built on:
//!
//! - a node turns `Pressed` on the frame the left mouse button (or a touch) goes down over
//!   it, so a click fires on press;
//! - it stays `Pressed` while the button is held, even when the pointer leaves it;
//! - on release it turns `Hovered` if the pointer is still over it, `None` otherwise;
//! - a node that is hovered and not pressed is `Hovered`, any other `None`;
//! - the topmost node under the pointer whose [`FocusPolicy`] is `Block` stops the nodes
//!   beneath it from being hovered or pressed; a hidden node is always `None`;
//! - the mouse's pointer is over nothing while it sleeps under the controller
//!   (`crate::pad::pointer`) [design, #2310]; touches still count.
//!
//! These are the rules Bevy 0.20's `ui_focus_system` applies to `Interaction`, written out
//! again for our component (which Bevy's system no longer sees). `RelativeCursorPosition`
//! is not deprecated and Bevy still keeps it.

use bevy::camera::{NormalizedRenderTarget, RenderTarget};
use bevy::ecs::entity::{ContainsEntity, EntityHashMap};
use bevy::input::touch::Touches;
use bevy::input::InputSystems;
use bevy::prelude::*;
use bevy::ui::{CalculatedClip, ComputedUiTargetCamera, FocusPolicy, UiGlobalTransform, UiStack, UiSystems};
use bevy::window::PrimaryWindow;

/// The pointer state of a UI node (none, hovered, pressed); see the module notes for when
/// each holds.
#[derive(Component, Copy, Clone, Eq, PartialEq, Debug, Default)]
pub enum PointerState {
    /// The left button (or a touch) went down over the node and is still held.
    Pressed,
    /// The pointer is over the node and nothing above it blocks it.
    Hovered,
    /// Neither.
    #[default]
    None,
}

/// A clickable UI node: blocks the pointer and carries a [`PointerState`], as Bevy's own
/// `Button` did before 0.20.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq, Eq)]
#[require(Node, FocusPolicy::Block, PointerState)]
pub struct Button;

/// Keeps [`PointerState`] up to date, in `PreUpdate` beside Bevy's own UI focus pass (the
/// test aids that feed presses run before [`UiSystems::Focus`]).
pub struct UiPointerPlugin;

/// One frame's alternate-sheet touch ownership, published by the sole wrapper
/// reader before UI focus. Hardware input is never filtered by this resource.
/// Overflow suppresses touch until the publisher's next fresh frame; the sheet
/// remains responsible for draining its contacts to actual neutral.
#[derive(Resource, Default)]
pub struct TouchUiCapture {
    ids: [Option<u64>; 32],
    overflow: bool,
    pointer_suppressed: bool,
}
impl TouchUiCapture {
    pub fn begin_frame(&mut self) { self.ids.fill(None); self.overflow = false; self.pointer_suppressed = false; }
    pub fn block(&mut self, id: u64) {
        if self.ids.contains(&Some(id)) { return; }
        if let Some(slot) = self.ids.iter_mut().find(|slot| slot.is_none()) { *slot = Some(id); }
        else { self.overflow = true; }
    }
    pub fn allows(&self, id: u64) -> bool { !self.overflow && !self.ids.contains(&Some(id)) }
    pub fn quarantine_overflow(&mut self) { self.overflow = true; }
    /// Hide the stale pointer under a visible sheet. Actual hardware intent yields
    /// the sheet first, so physical input resumes without changing Window state.
    pub fn suppress_pointer(&mut self, suppressed: bool) { self.pointer_suppressed = suppressed; }
    pub fn pointer_suppressed(&self) -> bool { self.pointer_suppressed }
}

impl Plugin for UiPointerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreUpdate, update_pointer_state.in_set(UiSystems::Focus).after(InputSystems));
    }
}

/// Where a node stands relative to the pointer this frame.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
enum Place {
    /// Not looked at this frame (not in the UI stack, or its root has no camera).
    Untouched,
    /// Not drawn (inherited visibility off).
    Hidden,
    /// Drawn, pointer not over it. `known` is false when the pointer's place relative to
    /// the node is unknown (no cursor in the window, or a node of zero size).
    Out { known: bool },
    /// Under the pointer, at or above the topmost blocking node.
    Top,
    /// Under the pointer, beneath a blocking node.
    Under,
}

/// The frame's input for one node.
#[derive(Copy, Clone, Debug)]
struct Frame {
    place: Place,
    /// The left button or a touch went down this frame.
    clicked: bool,
    /// The left button or a touch came up this frame.
    released: bool,
    /// The node was pressed and released in the same frame last frame: it goes back to
    /// `None` now.
    reset: bool,
}

/// One node's next state. Returns the state, whether it was written (a write marks the
/// component changed even when the value ends where it started, as a press and release
/// in one frame does), and whether it must be reset next frame.
fn next_state(was: PointerState, f: Frame) -> (PointerState, bool, bool) {
    let mut v = was;
    let mut wrote = false;
    let mut reset_next = false;
    if f.reset {
        v = PointerState::None;
        wrote = true;
    }
    if f.released && v == PointerState::Pressed {
        v = PointerState::None;
        wrote = true;
    }
    let mut set_if_neq = |v: &mut PointerState, to: PointerState| {
        if *v != to {
            *v = to;
            wrote = true;
        }
    };
    match f.place {
        Place::Untouched => {}
        Place::Hidden => set_if_neq(&mut v, PointerState::None),
        Place::Out { known } => {
            if v == PointerState::Hovered || !known {
                set_if_neq(&mut v, PointerState::None);
            }
        }
        Place::Top => {
            if f.clicked {
                if v != PointerState::Pressed {
                    set_if_neq(&mut v, PointerState::Pressed);
                    reset_next = f.released;
                }
            } else if v == PointerState::None {
                set_if_neq(&mut v, PointerState::Hovered);
            }
        }
        Place::Under => {
            if v != PointerState::Pressed {
                set_if_neq(&mut v, PointerState::None);
            }
        }
    }
    (v, wrote, reset_next)
}

type NodeData = (
    &'static ComputedNode,
    &'static UiGlobalTransform,
    &'static ComputedUiTargetCamera,
    Option<&'static InheritedVisibility>,
    Option<&'static CalculatedClip>,
    Option<&'static FocusPolicy>,
);

/// Sets every [`PointerState`] from the pointer and the left button (or touches).
#[allow(clippy::too_many_arguments)]
fn update_pointer_state(
    mut to_reset: Local<Vec<Entity>>,
    mut places: Local<EntityHashMap<Place>>,
    cameras: Query<(Entity, &Camera, &RenderTarget)>,
    primary_window: Query<Entity, With<PrimaryWindow>>,
    windows: Query<&Window>,
    mouse: crate::pad::ActionButtons<MouseButton>,
    touches: Res<Touches>,
    ui_stack: Res<UiStack>,
    nodes: Query<NodeData>,
    mut states: Query<(Entity, &mut PointerState)>,
    (pad_pointer, platform): (Option<Res<crate::pad::pointer::PadPointer>>, Option<Res<crate::pad::PlatformInputGate>>),
    touch_owner: Option<Res<TouchUiCapture>>,
) {
    if platform.as_ref().is_some_and(|g| !g.allows_actions()) {
        to_reset.clear(); places.clear();
        for (_, mut state) in &mut states { state.set_if_neq(PointerState::None); }
        return;
    }
    let mouse = mouse.snapshot();
    // A pointer asleep under the controller is on nothing (`crate::pad::pointer`, #2310).
    let sleeping = crate::pad::pointer::asleep(pad_pointer.as_deref())
        || touch_owner.as_ref().is_some_and(|owner| owner.pointer_suppressed());
    let primary_window = primary_window.iter().next();
    let touch_allowed = |id| platform.as_ref().is_none_or(|g| g.allows_touch(id))
        && touch_owner.as_ref().is_none_or(|owner| owner.allows(id));
    let released = mouse.just_released(MouseButton::Left) || touches.iter_just_released().any(|t| touch_allowed(t.id()));
    let clicked = mouse.just_pressed(MouseButton::Left) || touches.iter_just_pressed().any(|t| touch_allowed(t.id()));

    // The pointer in each window camera's viewport, in physical pixels.
    let cursor: EntityHashMap<Vec2> = cameras
        .iter()
        .filter_map(|(entity, camera, target)| {
            let Some(NormalizedRenderTarget::Window(window)) = target.normalize(primary_window) else {
                return None;
            };
            let window = windows.get(window.entity()).ok()?;
            let origin = camera.physical_viewport_rect().map(|r| r.min.as_vec2()).unwrap_or_default();
            let mouse_position = window.physical_cursor_position().filter(|_| !sleeping);
            let touch_position = || touches.iter().chain(touches.iter_just_pressed())
                .chain(touches.iter_just_released()).find(|t| touch_allowed(t.id()))
                .map(|t| t.position() * window.scale_factor());
            // A passive retained mouse coordinate is not a fresh touch's position.
            // An admitted mouse hold/press/release keeps its existing priority.
            let mouse_active = mouse.pressed(MouseButton::Left)
                || mouse.just_pressed(MouseButton::Left) || mouse.just_released(MouseButton::Left);
            let position = if mouse_active { mouse_position.or_else(touch_position) }
                else { touch_position().or(mouse_position) };
            position.map(|p| (entity, p - origin))
        })
        .collect();

    // Each drawn node's place, top of the stack first.
    places.clear();
    let mut under_pointer: Vec<Entity> = Vec::new();
    for slice in ui_stack.partition.iter().rev().map(|r| &ui_stack.uinodes[r.clone()]) {
        let Some(&root) = slice.first() else { continue };
        let Ok((_, _, camera, ..)) = nodes.get(root) else { continue };
        let Some(camera) = camera.get() else { continue };
        let point = cursor.get(&camera).copied();
        for &entity in slice.iter().rev() {
            let Ok((node, transform, _, visibility, clip, _)) = nodes.get(entity) else { continue };
            let Some(visibility) = visibility else { continue };
            if !visibility.get() {
                places.insert(entity, Place::Hidden);
                continue;
            }
            let over = point.is_some_and(|p| node.contains_point(*transform, p) && clip.is_none_or(|c| c.contains_point(p)));
            if over {
                under_pointer.push(entity);
            } else {
                let known = point.and_then(|p| node.normalize_point(*transform, p)).is_some();
                places.insert(entity, Place::Out { known });
            }
        }
    }
    // The nodes under the pointer down to the first that blocks are on top; the rest are
    // beneath it. A node without a focus policy blocks.
    let mut blocked = false;
    for entity in under_pointer {
        places.insert(entity, if blocked { Place::Under } else { Place::Top });
        if !blocked {
            let policy = nodes.get(entity).ok().and_then(|n| n.5).copied().unwrap_or(FocusPolicy::Block);
            blocked = policy == FocusPolicy::Block;
        }
    }

    let resets = std::mem::take(&mut *to_reset);
    for (entity, mut state) in &mut states {
        let place = places.get(&entity).copied().unwrap_or(Place::Untouched);
        let reset = resets.contains(&entity);
        let (v, wrote, reset_next) = next_state(*state, Frame { place, clicked, released, reset });
        if wrote {
            *state = v;
        }
        if reset_next {
            to_reset.push(entity);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PointerState::{Hovered, None, Pressed};
    use super::*;

    fn touch_scene() -> (App, Entity, Entity) {
        use bevy::{app::{HierarchyPropagatePlugin, PropagateSet}, camera::{ComputedCameraValues,RenderTargetInfo,visibility::VisibilityPlugin}, ui::{ComputedUiRenderTargetInfo,UiScale,ui_surface::UiSurface,update::propagate_ui_target_cameras}};
        let mut app = App::new();
        app.add_plugins((MinimalPlugins,bevy::input::InputPlugin,bevy::asset::AssetPlugin::default(),bevy::transform::TransformPlugin,bevy::text::TextPlugin));
        app.init_resource::<Assets<Mesh>>().init_resource::<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>()
            .add_plugins((VisibilityPlugin,HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(PostUpdate),HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(PostUpdate),UiPointerPlugin))
            .init_resource::<UiScale>().init_resource::<UiSurface>().init_resource::<UiStack>();
        app.add_systems(PostUpdate,(ApplyDeferred,propagate_ui_target_cameras,bevy::ui::ui_layout_system).chain());
        app.configure_sets(PostUpdate,PropagateSet::<ComputedUiTargetCamera>::default().after(propagate_ui_target_cameras).before(bevy::ui::ui_layout_system));
        app.configure_sets(PostUpdate,PropagateSet::<ComputedUiRenderTargetInfo>::default().after(propagate_ui_target_cameras).before(bevy::ui::ui_layout_system));
        let camera=app.world_mut().spawn((Camera2d,Camera {computed:ComputedCameraValues {target_info:Some(RenderTargetInfo {physical_size:UVec2::splat(400),scale_factor:1.0}),..default()},..default()})).id();
        let window=app.world_mut().spawn((Window {resolution:bevy::window::WindowResolution::new(400,400),..default()},PrimaryWindow)).id();
        let button=app.world_mut().spawn((Button,Node {position_type:PositionType::Absolute,left:px(20.0),top:px(20.0),width:px(100.0),height:px(50.0),..default()},UiTargetCamera(camera))).id();
        for _ in 0..4 {app.update();}
        // This is real Taffy layout, not authored ComputedNode data. Its system
        // requires TextPlugin's FontCx and RemSize even for a text-free Button.
        let node=app.world().get::<ComputedNode>(button).unwrap();
        assert!(node.size().abs_diff_eq(Vec2::new(100.0,50.0),0.01),"actual laid-out button: {:?}",node.size());
        assert!(app.world().get::<UiGlobalTransform>(button).unwrap().translation.abs_diff_eq(Vec2::new(70.0,45.0),0.01));
        assert!(app.world().get::<InheritedVisibility>(button).unwrap().get());
        assert_eq!(app.world().get::<ComputedUiTargetCamera>(button).unwrap().get(),Some(camera));
        let mut stack=app.world_mut().resource_mut::<UiStack>();
        stack.uinodes=vec![button];stack.partition.clear();stack.partition.push(0..1);
        (app,window,button)
    }

    #[test]
    fn alternate_sheet_owns_touch_at_real_ui_focus_without_consuming_mouse_or_raw_touches() {
        let (mut app,window,button)=touch_scene();
        app.init_resource::<TouchUiCapture>();
        app.world_mut().resource_mut::<TouchUiCapture>().block(7);
        app.world_mut().write_message(bevy::input::touch::TouchInput {window,id:7,phase:bevy::input::touch::TouchPhase::Started,position:Vec2::splat(40.0),force:std::option::Option::None});
        app.update();
        assert_eq!(*app.world().get::<PointerState>(button).unwrap(),PointerState::None);
        assert!(app.world().resource::<Touches>().get_pressed(7).is_some());
        app.world_mut().get_mut::<Window>(window).unwrap().set_cursor_position(Some(Vec2::splat(40.0)));
        app.world_mut().write_message(bevy::input::mouse::MouseButtonInput {window,button:MouseButton::Left,state:bevy::input::ButtonState::Pressed});
        app.update();
        assert_eq!(*app.world().get::<PointerState>(button).unwrap(),PointerState::Pressed,"physical mouse still reaches the real UI focus consumer");
        assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
    }

    fn f(place: Place) -> Frame {
        Frame { place, clicked: false, released: false, reset: false }
    }

    #[test]
    fn a_press_fires_on_the_frame_the_button_goes_down() {
        assert_eq!(next_state(None, f(Place::Top)), (Hovered, true, false));
        assert_eq!(next_state(Hovered, Frame { clicked: true, ..f(Place::Top) }), (Pressed, true, false));
        // Held: no further writes, so `Changed` does not fire again.
        assert_eq!(next_state(Pressed, f(Place::Top)), (Pressed, false, false));
    }

    #[test]
    fn a_held_press_survives_leaving_the_node_and_ends_on_release() {
        assert_eq!(next_state(Pressed, f(Place::Out { known: true })), (Pressed, false, false));
        assert_eq!(next_state(Pressed, f(Place::Under)), (Pressed, false, false));
        assert_eq!(next_state(Pressed, Frame { released: true, ..f(Place::Out { known: true }) }), (None, true, false));
        assert_eq!(next_state(Pressed, Frame { released: true, ..f(Place::Top) }), (Hovered, true, false));
    }

    #[test]
    fn hover_ends_when_the_pointer_leaves_or_a_node_above_blocks() {
        assert_eq!(next_state(Hovered, f(Place::Out { known: true })), (None, true, false));
        assert_eq!(next_state(Hovered, f(Place::Under)), (None, true, false));
        assert_eq!(next_state(None, f(Place::Out { known: true })), (None, false, false));
        // The pointer left the window: even a held press lets go.
        assert_eq!(next_state(Pressed, f(Place::Out { known: false })), (None, true, false));
    }

    #[test]
    fn hidden_nodes_are_never_hovered_or_pressed() {
        assert_eq!(next_state(Pressed, f(Place::Hidden)), (None, true, false));
        assert_eq!(next_state(None, f(Place::Hidden)), (None, false, false));
        assert_eq!(next_state(Hovered, f(Place::Untouched)), (Hovered, false, false));
    }

    #[test]
    fn a_press_and_release_in_one_frame_still_reads_as_a_press() {
        let (v, wrote, reset) = next_state(Hovered, Frame { clicked: true, released: true, ..f(Place::Top) });
        assert_eq!((v, wrote, reset), (Pressed, true, true));
        // Next frame it is put back to None, then hovered again in the same pass.
        assert_eq!(next_state(Pressed, Frame { reset: true, ..f(Place::Top) }), (Hovered, true, false));
        // Already pressed (a second click within one frame of a release): written again so
        // `Changed` fires.
        assert_eq!(next_state(Pressed, Frame { clicked: true, released: true, ..f(Place::Top) }), (Pressed, true, true));
    }
    #[test]
    fn platform_cover_blocks_real_touch_pointer_admission_without_clearing_touch_hardware() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::input::InputPlugin, UiPointerPlugin))
            .init_resource::<UiStack>().init_resource::<crate::pad::PlatformInputGate>();
        app.world_mut().resource_mut::<crate::pad::PlatformInputGate>().set_blocked(true);
        let button = app.world_mut().spawn((Button, PointerState::Pressed)).id();
        app.world_mut().write_message(bevy::input::touch::TouchInput {
            window: Entity::PLACEHOLDER, id: 7, phase: bevy::input::touch::TouchPhase::Started,
            position: Vec2::ONE, force: std::option::Option::None,
        });
        app.update();
        assert_eq!(*app.world().get::<PointerState>(button).unwrap(), PointerState::None);
        assert!(app.world().resource::<Touches>().any_just_pressed(), "hardware touch ownership is preserved");
    }

    include!("ui_pointer_retained_cursor_tests.rs");

}
