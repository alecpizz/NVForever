//! Optional touch analog sample. Raw hardware, bindings and script permissions stay owned elsewhere.
use crate::bindings::{Control, ControlInput};
use bevy::prelude::*;

#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct TouchMovementInput {
    /// Right/forward, radially clamped to one after the touch dead zone.
    pub axes: Vec2,
    /// Prior touch sample, for source-owned directional action edges.
    pub previous_axes: Vec2,
}
impl TouchMovementInput {
    pub fn admitted(
        &self,
        input: &ControlInput,
        touch: Option<&crate::pad::TouchDeviceSwitch>,
    ) -> Vec2 {
        if input.controller()
            || touch.is_some_and(|t| !t.allows_actions())
            || !self.axes.is_finite()
        {
            return Vec2::ZERO;
        }
        let mut v = self.axes.clamp_length_max(1.0);
        if !input.virtual_action_allowed(if v.x < 0.0 {
            Control::SlideLeft
        } else {
            Control::SlideRight
        }) {
            v.x = 0.0;
        }
        if !input.virtual_action_allowed(if v.y < 0.0 {
            Control::Back
        } else {
            Control::Forward
        }) {
            v.y = 0.0;
        }
        v
    }
}
