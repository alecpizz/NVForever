//! Settings uses the menu-sheet router's single raw contact stream, and hits the
//! same desktop Bevy buttons as the mouse. Only the pause consumer applies effects.
use super::*;
use crate::{menu::barter_touch::Frame, pipboy::touch_scroll::gesture::Intent};
use bevy::{input::touch::{TouchInput, TouchPhase}, ui::{CalculatedClip, ComputedUiTargetCamera, UiStack}};

#[derive(Resource)]
pub struct SettingsTouch {
    enabled: bool,
    frame: Option<Frame>,
    edges: Vec<TouchInput>,
    contact: Option<Contact>,
}

impl Default for SettingsTouch {
    fn default() -> Self {
        Self::for_platform(cfg!(target_os = "android"))
    }
}

impl SettingsTouch {
    fn for_platform(enabled: bool) -> Self { Self { enabled, frame: None, edges: Vec::new(), contact: None } }

    pub fn enabled(&self) -> bool { self.enabled }

    #[cfg(feature = "test-fixtures")]
    pub(super) fn enabled_fixture() -> Self { Self::for_platform(true) }

    pub fn publish(&mut self, frame: Option<Frame>, edges: &mut Vec<TouchInput>) {
        self.edges.clear();
        self.frame = frame.filter(|f| f.admitted && !f.fresh);
        if self.frame.is_some() {
            std::mem::swap(&mut self.edges, edges);
        } else {
            self.contact = None;
        }
    }
}

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(super) enum Surface { Rows, Choices }

#[derive(Clone, Copy, PartialEq, Eq)]
enum Modal { Page, Choices(usize), Keep(usize), Listening }

#[derive(Clone, Copy, PartialEq, Eq)]
enum Target { Button(PauseButton), Body, Choices, Outside }

#[derive(Clone, Copy)]
struct Hit {
    target: Target,
    in_rows: bool,
    meter: Option<(f32, f32)>,
    track: Option<(f32, f32)>,
}

struct Contact {
    id: u64,
    origin: Vec2,
    last_y: f32,
    hit: Hit,
    intent: Intent,
    first: f32,
    scope: SettingsScope,
    modal: Modal,
    size: Vec2,
    scale: f32,
    camera: Entity,
    density: f32,
    ui_scale: f32,
}

#[derive(Default)]
pub(super) struct Action {
    pub click: Option<PauseButton>,
    pub outside: bool,
    pub first: Option<usize>,
    pub meter: Option<(RowId, f32)>,
    pub track: Option<f32>,
}

#[derive(SystemParam)]
pub(super) struct Input<'w, 's> {
    state: Option<ResMut<'w, SettingsTouch>>,
    scale: Option<Res<'w, SettingsMenuScale>>,
    geometry: Geometry<'w, 's>,
}

impl Input<'_, '_> {
    pub(super) fn scale(&self) -> f32 {
        self.scale.as_ref().map_or(1.0, |s| s.scale())
    }

    pub(super) fn read(&mut self, view: &PauseView, page: Page, rows: &[ViewEntry], keep: Option<usize>, listening: bool) -> Action {
        let mut result = Action::default();
        let settings_scale = self.scale();
        let Some(state) = self.state.as_deref_mut() else { return result };
        let Some((drawn, frame)) = view.drawn.as_ref().zip(state.frame.take()).filter(|(d, frame)| {
            if !d.touch || !matches!(page, Page::Settings { .. }) || !matches!(d.page, Page::Settings { .. })
                || d.scope != view.scope || (!listening && d.entries != rows) || d.scroll != view.scroll
                || d.dropdown != view.dropdown || d.keep.is_some() != keep.is_some()
                || d.listening.is_some() != listening || d.settings_scale != settings_scale.to_bits()
                || !frame.size.is_finite() || frame.size.y <= 0.0
            {
                return false;
            }
            let _units = settings_units(settings_scale);
            let expected_w = (960.0 * frame.size.x / frame.size.y * crate::interface::menu_room()).round() as u32;
            d.view_w == expected_w
        }) else {
            state.contact = None;
            state.edges.clear();
            return result;
        };
        let modal = if let Some(row) = keep { Modal::Keep(row) }
            else if listening { Modal::Listening }
            else if let Some(o) = view.dropdown { Modal::Choices(o.row) }
            else { Modal::Page };
        let first = view.dropdown.map_or(view.scroll, |o| o.first);
        let last = match modal {
            Modal::Choices(row) => drawn.list(row).map_or(0, |(n, _)| n.len().saturating_sub(DROP_ROWS)),
            Modal::Page => rows.len().saturating_sub(view.lines()),
            _ => 0,
        };
        let unit_scale = crate::interface::menu_scale() * settings_scale;
        let row_units = if matches!(modal, Modal::Choices(_)) { DROP_ITEM } else { OPTION_ROW };
        let step_px = (row_units * (frame.size.y / 960.0) * unit_scale).max(16.0);
        let bounds = Rect::from_corners(Vec2::ZERO, frame.size);
        if frame.neutral || state.contact.as_ref().is_some_and(|c| c.scope != view.scope || c.modal != modal
            || c.size != frame.size || c.scale != settings_scale || c.camera != frame.camera
            || c.density != frame.density || c.ui_scale != frame.ui_scale)
        {
            state.contact = None;
        }
        for e in state.edges.drain(..) {
            if e.window != frame.window { continue; }
            if e.phase == TouchPhase::Started {
                if state.contact.is_none() && bounds.contains(e.position) {
                    let hit = self.geometry.hit(frame, e.position, modal);
                    state.contact = Some(Contact {
                        id: e.id, origin: e.position, last_y: e.position.y, hit,
                        intent: Intent::Pending, first: first.min(last) as f32,
                        scope: view.scope, modal, size: frame.size, scale: settings_scale,
                        camera: frame.camera, density: frame.density, ui_scale: frame.ui_scale,
                    });
                } else if state.contact.as_ref().is_some_and(|c| c.id == e.id) {
                    state.contact = None;
                }
                continue;
            }
            let Some(c) = state.contact.as_mut().filter(|c| c.id == e.id) else { continue };
            if e.phase == TouchPhase::Canceled || !e.position.is_finite() {
                state.contact = None;
                continue;
            }
            c.intent.observe((e.position - c.origin).to_array(), 0);
            let ended = e.phase == TouchPhase::Ended;
            let matched = || bounds.contains(e.position) && self.geometry.hit(frame, e.position, modal).target == c.hit.target;
            if c.hit.target == Target::Button(PauseButton::ScrollTrack) {
                let origin = self.geometry.hit(frame, c.origin, modal);
                if origin.target == c.hit.target {
                    if let Some((offset_y, per_y)) = origin.track {
                        result.track = Some((offset_y + per_y * e.position.y).clamp(0.0, 1.0));
                    }
                }
            } else if c.intent == Intent::Scroll && c.hit.in_rows {
                c.first = (c.first + (c.last_y - e.position.y) / step_px).clamp(0.0, last as f32);
                c.last_y = e.position.y;
                let next = c.first.round() as usize;
                result.first = (next != first).then_some(next);
            } else if let Target::Button(PauseButton::Meter(row)) = c.hit.target {
                // Repaint can replace the node each frame; re-admit its semantic row
                // and the meter still painted at the same origin before writing.
                let origin = self.geometry.hit(frame, c.origin, modal);
                if origin.target == c.hit.target && (c.intent == Intent::Assign || (ended && c.intent == Intent::Pending && matched())) {
                    if let Some((offset, per_x)) = origin.meter {
                        result.meter = Some((row, (offset + per_x * e.position.x).clamp(0.0, 1.0)));
                    }
                }
            } else if ended && c.intent == Intent::Pending && matched() {
                match c.hit.target {
                    Target::Button(b) => result.click = Some(b),
                    Target::Outside => result.outside = true,
                    _ => {}
                }
            }
            if ended {
                state.contact = None;
                // A later Start in this batch has no new painted targets yet.
                break;
            }
        }
        result
    }
}

type HitNode = (&'static ComputedNode, &'static UiGlobalTransform, &'static ComputedUiTargetCamera,
    &'static InheritedVisibility, Option<&'static CalculatedClip>, Option<&'static PauseButton>, Option<&'static Surface>);
type HitFilter = Or<(With<PauseButton>, With<Surface>)>;

#[derive(SystemParam)]
struct Geometry<'w, 's> {
    stack: Option<Res<'w, UiStack>>,
    nodes: Query<'w, 's, HitNode, HitFilter>,
}

impl Geometry<'_, '_> {
    fn hit(&self, frame: Frame, point: Vec2, modal: Modal) -> Hit {
        let physical = point * frame.density;
        if let Some(stack) = self.stack.as_ref() {
            for &entity in stack.uinodes.iter().rev() {
                let Ok((node, transform, camera, visible, clip, button, surface)) = self.nodes.get(entity) else { continue };
                if camera.get() != Some(frame.camera) || !visible.get() || !node.contains_point(*transform, physical)
                    || clip.is_some_and(|c| !c.contains_point(physical)) { continue; }
                let (target, in_rows) = match (modal, button.copied(), surface) {
                    (Modal::Keep(_), Some(b @ PauseButton::Keep(_)), _) | (Modal::Listening, Some(b @ PauseButton::Back), _) => (Target::Button(b), false),
                    (Modal::Choices(row), Some(b @ PauseButton::Choice(r, _)), _) if row == r => (Target::Button(b), true),
                    (Modal::Choices(_), _, Some(Surface::Choices)) => (Target::Choices, true),
                    (Modal::Page, Some(b), _) if !matches!(b, PauseButton::Choice(..) | PauseButton::Keep(_)) => {
                        (Target::Button(b), matches!(b, PauseButton::Row(_) | PauseButton::Meter(_) | PauseButton::SliderStep(..)))
                    }
                    (Modal::Page, _, Some(Surface::Rows)) => (Target::Body, true),
                    _ => continue,
                };
                let meter = if matches!(target, Target::Button(PauseButton::Meter(_))) {
                    node.normalize_point(*transform, Vec2::new(0.0, physical.y)).zip(node.normalize_point(*transform, Vec2::new(frame.density, physical.y)))
                        .map(|(a, b)| (a.x + 0.5, b.x - a.x))
                } else { None };
                let track = if target == Target::Button(PauseButton::ScrollTrack) {
                    node.normalize_point(*transform, Vec2::new(physical.x, 0.0)).zip(node.normalize_point(*transform, Vec2::new(physical.x, frame.density)))
                        .map(|(a, b)| (a.y + 0.5, b.y - a.y))
                } else { None };
                return Hit { target, in_rows, meter, track };
            }
        }
        Hit { target: if matches!(modal, Modal::Choices(_)) { Target::Outside } else { Target::Body }, in_rows: false, meter: None, track: None }
    }
}

#[cfg(test)]
mod tests;
