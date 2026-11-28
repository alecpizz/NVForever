//! A dialogue-owned screen rendered on an existing game device model.
//! The renderer supplies the real glass projection; it never opens a device menu.
use bevy::prelude::*;
use std::sync::Arc;
mod presentation;
pub(crate) use presentation::install;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceKind {
    Terminal,
    PipBoy,
}

/// Identity and geometry of one published dialogue surface. Display coordinates
/// are physical window pixels; canvas coordinates are pixels in its UI texture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stamp {
    pub owner: Entity,
    pub generation: u64,
    pub window: Entity,
    pub kind: DeviceKind,
    pub display: Rect,
    pub density: f32,
}

#[derive(Resource, Clone)]
pub struct Request {
    pub stamp: Stamp,
    pub image: Handle<Image>,
    pub camera: Entity,
    pub target: UVec2,
    pub canvas: Rect,
}

fn positive_rect(rect: Rect) -> bool {
    rect.min.is_finite() && rect.max.is_finite()
        && rect.width() > 0.0 && rect.height() > 0.0
}

impl Request {
    pub fn valid(&self) -> bool {
        self.stamp.density.is_finite() && self.stamp.density > 0.0
            && positive_rect(self.stamp.display) && positive_rect(self.canvas)
            && self.target.x > 0 && self.target.y > 0
            && self.canvas.min.cmpge(Vec2::ZERO).all()
            && self.canvas.max.cmple(self.target.as_vec2()).all()
    }
}

/// Both directions use the same posed screen mesh and camera as the image.
/// Housing, switches and areas outside the glass return None.
pub trait ScreenMap: Send + Sync {
    fn canvas_at(&self, window_physical: Vec2) -> Option<Vec2>;
    fn window_at(&self, canvas: Vec2) -> Option<Vec2>;
    /// Whether `window_physical` hits the device's power button (outside the glass).
    fn power_button_at(&self, _window_physical: Vec2) -> bool { false }
    /// Screen-space bounds (physical window pixels) of the device's power button, if any.
    fn power_button_rect(&self) -> Option<Rect> { None }
}

#[derive(Resource)]
pub struct DeviceView {
    pub stamp: Stamp,
    pub image: Handle<Image>,
    pub camera: Entity,
    /// A conservative lower bound on physical screen pixels per canvas pixel,
    /// measured over the usable glass. Content uses it for text and hit targets.
    pub scale: f32,
    pub map: Arc<dyn ScreenMap>,
}

/// A held gesture and its painted image belong to this exact projection. Keeping
/// the Arc also prevents a replacement mapper from reusing the old identity.
#[derive(Clone)]
pub(crate) struct Publication {
    stamp: Stamp,
    image: bevy::asset::AssetId<Image>,
    camera: Entity,
    scale: f32,
    map: Arc<dyn ScreenMap>,
}
impl PartialEq for Publication {
    fn eq(&self, other: &Self) -> bool {
        self.stamp==other.stamp && self.image==other.image && self.camera==other.camera
            && self.scale==other.scale && Arc::ptr_eq(&self.map,&other.map)
    }
}

impl DeviceView {
    pub(crate) fn publication(&self) -> Publication {
        Publication {stamp:self.stamp,image:self.image.id(),camera:self.camera,
            scale:self.scale,map:self.map.clone()}
    }
    pub fn matches(&self, request: &Request) -> bool {
        request.valid() && self.stamp == request.stamp
            && self.scale.is_finite() && self.scale > 0.0
    }

    pub fn canvas_at(&self, request: &Request, window_physical: Vec2) -> Option<Vec2> {
        if !self.matches(request) || !window_physical.is_finite()
            || !request.stamp.display.contains(window_physical) {
            return None;
        }
        self.map.canvas_at(window_physical)
            .filter(|p| p.is_finite() && request.canvas.contains(*p))
    }

    pub fn window_at(&self, request: &Request, canvas: Vec2) -> Option<Vec2> {
        if !self.matches(request) || !canvas.is_finite()
            || !request.canvas.contains(canvas) {
            return None;
        }
        self.map.window_at(canvas)
            .filter(|p| p.is_finite() && request.stamp.display.contains(*p))
    }

    /// Convert a logical window length to texture pixels without shrinking the
    /// requested physical target or type size when the model is fitted smaller.
    pub fn canvas_length(&self, request: &Request, logical: f32) -> Option<f32> {
        if !self.matches(request) || !logical.is_finite() || logical < 0.0 {
            return None;
        }
        let value = logical * request.stamp.density / self.scale;
        value.is_finite().then_some(value)
    }

    /// Whether `window_physical` hits the device's power button for this request.
    pub fn power_button_at(&self, request: &Request, window_physical: Vec2) -> bool {
        self.matches(request)
            && window_physical.is_finite()
            && request.stamp.display.contains(window_physical)
            && self.map.power_button_at(window_physical)
    }

    /// Screen-space bounds (physical window pixels) of the device's power button.
    pub fn power_button_rect(&self, request: &Request) -> Option<Rect> {
        self.matches(request)
            .then(|| self.map.power_button_rect())
            .flatten()
            .filter(|r| positive_rect(*r))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Glass;
    impl ScreenMap for Glass {
        fn canvas_at(&self, point: Vec2) -> Option<Vec2> {
            // The real renderer supplies its mesh; this synthetic mapping makes
            // the housing and stale-publication refusals independently visible.
            (point.x >= 20.0).then_some((point - Vec2::new(20.0, 10.0)) * 4.0)
        }
        fn window_at(&self, point: Vec2) -> Option<Vec2> {
            Some(point * 0.25 + Vec2::new(20.0, 10.0))
        }
    }

    fn fixture() -> (Request, DeviceView) {
        let stamp = Stamp { owner: Entity::PLACEHOLDER, generation: 7,
            window: Entity::PLACEHOLDER, kind: DeviceKind::Terminal,
            display: Rect::new(0.0, 0.0, 320.0, 240.0), density: 2.0 };
        (Request { stamp, image: Handle::default(), camera: Entity::PLACEHOLDER,
            target: UVec2::new(1280, 960), canvas: Rect::new(0.0, 0.0, 960.0, 720.0) },
         DeviceView { stamp, image: Handle::default(), camera: Entity::PLACEHOLDER,
            scale: 0.25, map: Arc::new(Glass) })
    }

    #[test]
    fn glass_mapping_rejects_housing_outside_canvas_and_stale_publication() {
        let (mut request, view) = fixture();
        assert_eq!(view.canvas_at(&request, Vec2::new(10.0, 50.0)), None);
        let point = Vec2::new(100.0, 80.0);
        let canvas = view.canvas_at(&request, point).unwrap();
        assert_eq!(view.window_at(&request, canvas), Some(point));
        assert_eq!(view.canvas_at(&request, Vec2::new(300.0, 220.0)), None);
        request.stamp.generation += 1;
        assert_eq!(view.canvas_at(&request, point), None);
        assert_eq!(view.canvas_length(&request, 48.0), None);
    }

    #[test]
    fn replacement_mapper_retires_the_same_request_publication() {
        let (request, mut view) = fixture();
        let held = view.publication();
        assert!(held == view.publication());
        view.map = Arc::new(Glass);
        assert!(view.matches(&request));
        assert!(held != view.publication());
        let rebound = view.publication();
        view.scale = 0.5;
        assert!(rebound != view.publication());
    }

    #[test]
    fn projected_size_preserves_logical_targets_and_refuses_invalid_geometry() {
        let (mut request, mut view) = fixture();
        assert_eq!(view.canvas_length(&request, 48.0), Some(384.0));
        assert_eq!(view.canvas_length(&request, 17.0), Some(136.0));
        view.scale = f32::NAN;
        assert!(!view.matches(&request));
        view.scale = 0.25;
        request.canvas.max.x = 2000.0;
        assert!(!request.valid());
        assert_eq!(view.window_at(&request, Vec2::ONE), None);
    }
}
