//! Source-owned touch intent; no OS/device or game-state dependencies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Intent {
    #[default]
    Pending,
    Scroll,
    Assign,
    /// An actual painted scrollbar marker owns this contact, not body scrolling.
    Control,
}
impl Intent {
    pub fn observe(&mut self, delta: [f32; 2], age: u64) {
        if *self != Self::Pending || !delta.into_iter().all(f32::is_finite) {
            return;
        }
        let [x, y] = delta.map(f32::abs);
        if x.hypot(y) > 12.0 {
            *self = if y > x { Self::Scroll } else { Self::Assign };
        } else if age >= 450 {
            *self = Self::Assign;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vertical_browsing_never_becomes_assignment_after_a_hold_or_turn() {
        let mut i = Intent::default();
        i.observe([2., -20.], 20);
        assert_eq!(i, Intent::Scroll);
        i.observe([100., -20.], 1000);
        assert_eq!(i, Intent::Scroll);
    }
    #[test]
    fn deliberate_horizontal_drag_and_stationary_hold_keep_existing_assignment() {
        for (d, t) in [([20., 2.], 20), ([2., 2.], 450), ([400., 400.], 100)] {
            let mut i = Intent::default();
            i.observe(d, t);
            assert_eq!(i, Intent::Assign);
        }
    }
    #[test]
    fn jitter_does_not_scroll_or_assign_and_nonfinite_samples_do_not_arm() {
        let mut i = Intent::default();
        i.observe([3., 4.], 449);
        assert_eq!(i, Intent::Pending);
        i.observe([f32::NAN, 20.], 500);
        assert_eq!(i, Intent::Pending);
    }
}
