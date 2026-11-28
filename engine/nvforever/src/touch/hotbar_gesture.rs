//! Input-independent release admission. These exact routines feed the touch adapter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Box2 {
    pub min: [f32; 2],
    pub max: [f32; 2],
}
impl Box2 {
    pub fn contains(self, p: [f32; 2]) -> bool {
        p.into_iter().all(f32::is_finite)
            && p[0] >= self.min[0]
            && p[1] >= self.min[1]
            && p[0] < self.max[0]
            && p[1] < self.max[1]
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Target {
    Slot(usize),
    Tray,
    Context(usize),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Tap(Target),
    Context(usize),
}
#[derive(Clone, Copy, Debug)]
struct Contact {
    id: u64,
    target: Target,
    bounds: Box2,
    origin: [f32; 2],
    at: [f32; 2],
    started: u64,
    size: [f32; 2],
    cancelled: bool,
    held: bool,
}
#[derive(Default, Debug)]
pub(super) struct Gesture {
    contact: Option<Contact>,
}
impl Gesture {
    pub fn cancel(&mut self) {
        self.contact = None;
    }
    pub fn owns(&self, id: u64) -> bool {
        self.contact.is_some_and(|c| c.id == id)
    }
    pub fn busy(&self) -> bool {
        self.contact.is_some()
    }
    pub fn owner(&self) -> Option<u64> { self.contact.map(|c|c.id) }
    /// Read only: ordinary Outcome/end semantics remain unchanged. Binding waits
    /// for this particular contact's valid release even after its hold tick fired.
    pub fn valid_release(&self,id:u64,p:[f32;2],size:[f32;2])->bool {
        self.contact.is_some_and(|c|c.id==id&&!c.cancelled&&c.size==size&&c.bounds.contains(p)
            &&(p[0]-c.origin[0]).hypot(p[1]-c.origin[1])<=12.0)
    }
    pub fn start(
        &mut self,
        id: u64,
        target: Target,
        bounds: Box2,
        p: [f32; 2],
        size: [f32; 2],
        now: u64,
    ) -> bool {
        if self.contact.is_some() || !bounds.contains(p) {
            return false;
        }
        self.contact = Some(Contact {
            id,
            target,
            bounds,
            origin: p,
            at: p,
            started: now,
            size,
            cancelled: false,
            held: false,
        });
        true
    }
    pub fn moved(&mut self, id: u64, p: [f32; 2], size: [f32; 2]) {
        let Some(c) = self.contact.as_mut().filter(|c| c.id == id) else {
            return;
        };
        c.at = p;
        let distance = (p[0] - c.origin[0]).hypot(p[1] - c.origin[1]);
        c.cancelled |= size != c.size || !p.into_iter().all(f32::is_finite) || distance > 12.0;
    }
    pub fn tick(&mut self, now: u64) -> Option<Outcome> {
        let c = self.contact.as_mut()?;
        if !c.cancelled && !c.held && now.saturating_sub(c.started) >= 450 {
            c.held = true;
            if let Target::Slot(slot) = c.target {
                return Some(Outcome::Context(slot));
            }
        }
        None
    }
    pub fn end(
        &mut self,
        id: u64,
        p: [f32; 2],
        size: [f32; 2],
        now: u64,
        cancelled: bool,
    ) -> Option<Outcome> {
        if !self.owns(id) {
            return None;
        }
        self.moved(id, p, size);
        let c = self.contact.take()?;
        if cancelled
            || c.cancelled
            || c.held
            || now.saturating_sub(c.started) >= 450
            || !c.bounds.contains(p)
        {
            return None;
        }
        Some(Outcome::Tap(c.target))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn area() -> Box2 {
        Box2 {
            min: [10., 10.],
            max: [70., 70.],
        }
    }
    #[test]
    fn hold_opens_context_once_and_release_cannot_use_or_choose_it() {
        let mut g = Gesture::default();
        assert!(g.start(7, Target::Slot(3), area(), [30., 30.], [800., 400.], 0));
        assert_eq!(g.tick(449), None);
        assert_eq!(g.tick(450), Some(Outcome::Context(3)));
        assert_eq!(g.tick(900), None);
        assert_eq!(g.end(7, [30., 30.], [800., 400.], 950, false), None);
    }
    #[test]
    fn only_original_finger_can_release_and_tap_requires_same_bounds() {
        let mut g = Gesture::default();
        g.start(7, Target::Slot(0), area(), [30., 30.], [800., 400.], 0);
        assert!(!g.start(8, Target::Slot(1), area(), [30., 30.], [800., 400.], 1));
        assert_eq!(g.end(8, [30., 30.], [800., 400.], 10, false), None);
        assert!(g.owns(7));
        assert_eq!(
            g.end(7, [31., 31.], [800., 400.], 100, false),
            Some(Outcome::Tap(Target::Slot(0)))
        );
        g.start(9, Target::Slot(0), area(), [30., 30.], [800., 400.], 101);
        g.moved(9, [100., 30.], [800., 400.]);
        g.moved(9, [30., 30.], [800., 400.]);
        assert_eq!(g.end(9, [30., 30.], [800., 400.], 200, false), None);
    }
    #[test]
    fn cancellation_resize_and_nonfinite_positions_never_activate() {
        for (size, point, cancelled) in [
            ([801., 400.], [30., 30.], false),
            ([800., 400.], [f32::NAN, 30.], false),
            ([800., 400.], [30., 30.], true),
        ] {
            let mut g = Gesture::default();
            g.start(1, Target::Tray, area(), [30., 30.], [800., 400.], 0);
            assert_eq!(g.end(1, point, size, 100, cancelled), None);
        }
        let mut g = Gesture::default();
        g.start(1, Target::Tray, area(), [30., 30.], [800., 400.], 0);
        g.cancel();
        assert_eq!(g.end(1, [30., 30.], [800., 400.], 100, false), None);
    }
    #[test]
    fn context_action_needs_a_new_short_tap_and_busy_tracks_ownership() {
        let mut g = Gesture::default();
        assert!(!g.busy());
        assert!(g.start(2, Target::Context(0), area(), [30., 30.], [800., 400.], 0));
        assert!(g.busy());
        assert_eq!(
            g.end(2, [30., 30.], [800., 400.], 100, false),
            Some(Outcome::Tap(Target::Context(0)))
        );
        assert!(!g.busy());
    }
}
