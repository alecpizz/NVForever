//! Window/canvas correspondence from the same posed glass and camera as the image.
use crate::pipboy_arm::Posed;
use bevy::math::{Affine2, DVec2, DVec3, DVec4};
use bevy::prelude::*;
use nvf_play::dialogue_device::ScreenMap;
use std::sync::Arc;

pub(crate) struct GlassMap {
    glass: Arc<Posed>,
    blockers: Vec<Arc<Posed>>,
    power_button: Option<Arc<Posed>>,
    power_rect: Option<Rect>,
    camera: Camera,
    transform: GlobalTransform,
    display: Rect,
    canvas: Rect,
    uv_to_canvas: Affine2,
    logical_view: Rect,
}

impl GlassMap {
    /// Camera values must have been published by the normal camera system.
    /// The returned scale bounds every admitted triangle over the usable canvas.
    pub(crate) fn new(
        glass: Arc<Posed>, blockers: Vec<Arc<Posed>>, camera: &Camera, transform: &GlobalTransform,
        display: Rect, canvas: Rect, uv_to_canvas: Affine2,
    ) -> Option<(Self, f32)> {
        let physical = camera.physical_viewport_rect()?;
        let logical_view = camera.logical_viewport_rect()?;
        if !camera.is_active || physical.min != UVec2::ZERO
            || !valid_rect(display) || !valid_rect(canvas) || !valid_rect(logical_view)
            || !uv_to_canvas.is_finite() || uv_to_canvas.matrix2.determinant().abs() < 1e-9 {
            return None;
        }
        let scale = projected_scale(&glass, camera, transform, canvas, uv_to_canvas, display.size())?;
        Some((Self { glass, blockers, power_button: None, power_rect: None, camera: camera.clone(),
            transform: *transform, display, canvas, uv_to_canvas, logical_view }, scale))
    }

    /// Attach a tappable power button (projected mesh and/or bezel fallback rect)
    /// for Wrist Terminal / Retro Console quick-exit dialogue.
    pub(crate) fn with_power_button(mut self, mesh: Option<Arc<Posed>>, fallback_rect: Option<Rect>, density: f32) -> Self {
        let min_size = (56.0 * density.max(1.0)).max(48.0);
        let projected = mesh.as_ref().and_then(|posed| {
            let mut bounds: Option<Rect> = None;
            for &world in &posed.points {
                let Ok(viewport) = self.camera.world_to_viewport(&self.transform, world) else {
                    continue;
                };
                let local = (viewport - self.logical_view.min) / self.logical_view.size();
                let point = self.display.min + local * self.display.size();
                if !point.is_finite() {
                    continue;
                }
                bounds = Some(bounds.map_or(Rect::from_corners(point, point), |r| {
                    Rect::from_corners(r.min.min(point), r.max.max(point))
                }));
            }
            bounds.map(|r| {
                let center = r.center();
                let size = r.size().max(Vec2::splat(min_size));
                Rect::from_center_size(center, size).intersect(self.display)
            }).filter(|r| valid_rect(*r))
        });
        self.power_button = mesh;
        self.power_rect = projected.or_else(|| fallback_rect.map(|r| r.intersect(self.display)).filter(|r| valid_rect(*r)));
        self
    }
}

impl ScreenMap for GlassMap {
    fn canvas_at(&self, point: Vec2) -> Option<Vec2> {
        if !point.is_finite() || !self.display.contains(point) { return None; }
        // Camera viewport APIs take logical target pixels, not physical window pixels.
        let local = (point - self.display.min) / self.display.size();
        let viewport = self.logical_view.min + local * self.logical_view.size();
        let ray = self.camera.viewport_to_world(&self.transform, viewport).ok()?;
        let (distance, uv) = self.glass.hit(ray.origin, Vec3::from(ray.direction))?;
        if self.blockers.iter().any(|shape| shape.hit(ray.origin, Vec3::from(ray.direction))
            .is_some_and(|(near,_)| near <= distance)) { return None; }
        let canvas = self.uv_to_canvas.transform_point2(uv);
        (canvas.is_finite() && self.canvas.contains(canvas)).then_some(canvas)
    }

    fn window_at(&self, canvas: Vec2) -> Option<Vec2> {
        if !canvas.is_finite() || !self.canvas.contains(canvas) { return None; }
        let uv = self.uv_to_canvas.inverse().transform_point2(canvas);
        let world = self.glass.at_uv(uv)?;
        let viewport = self.camera.world_to_viewport(&self.transform, world).ok()?;
        let local = (viewport - self.logical_view.min) / self.logical_view.size();
        let point = self.display.min + local * self.display.size();
        let nearest=self.canvas_at(point)?;
        // A folded screen can project a back triangle through a nearer part of
        // the same glass. Accept the reverse map only for the same canvas texel.
        ((nearest-canvas).abs().max_element()<=0.25).then_some(point)
    }

    fn power_button_at(&self, point: Vec2) -> bool {
        if !point.is_finite() || !self.display.contains(point) {
            return false;
        }
        if let Some(button) = self.power_button.as_ref() {
            let local = (point - self.display.min) / self.display.size();
            let viewport = self.logical_view.min + local * self.logical_view.size();
            if let Ok(ray) = self.camera.viewport_to_world(&self.transform, viewport) {
                if button.hit(ray.origin, Vec3::from(ray.direction)).is_some() {
                    return true;
                }
            }
        }
        self.power_rect.is_some_and(|r| r.contains(point))
    }

    fn power_button_rect(&self) -> Option<Rect> {
        self.power_rect
    }
}

fn valid_rect(r: Rect) -> bool {
    r.min.is_finite() && r.max.is_finite() && r.width() > 0.0 && r.height() > 0.0
}

fn area(poly: &[DVec2]) -> f64 {
    if poly.len() < 3 { return 0.0; }
    poly.iter().zip(poly.iter().cycle().skip(1)).take(poly.len())
        .map(|(a,b)| a.perp_dot(*b)).sum::<f64>() * 0.5
}

/// Clip a convex polygon to one oriented edge; used for actual UV triangles,
/// their overlap checks, and the requested canvas, never authored hit rectangles.
fn clip(poly: &[DVec2], a: DVec2, b: DVec2) -> Vec<DVec2> {
    let mut out = Vec::new();
    let Some(mut previous) = poly.last().copied() else { return out; };
    let mut before = (b-a).perp_dot(previous-a);
    for &point in poly {
        let after = (b-a).perp_dot(point-a);
        if (before >= 0.0) != (after >= 0.0) {
            out.push(previous + (point-previous) * (before/(before-after)));
        }
        if after >= 0.0 { out.push(point); }
        previous = point; before = after;
    }
    out
}

fn intersect(mut poly: Vec<DVec2>, boundary: &[DVec2]) -> Vec<DVec2> {
    for (&a,&b) in boundary.iter().zip(boundary.iter().cycle().skip(1)).take(boundary.len()) {
        poly = clip(&poly, a, b);
        if poly.is_empty() { break; }
    }
    poly
}

/// A lower singular-value bound on the projective UV-to-window derivative.
/// On a triangle, homogeneous screen coordinates are affine in canvas x/y.
/// det(J)*w^3 is constant; each derivative numerator is affine. Vertex extrema
/// therefore bound the entire clipped triangle, not just a set of sample points.
fn projected_scale(
    glass: &Posed, camera: &Camera, transform: &GlobalTransform,
    canvas: Rect, uv_to_canvas: Affine2, physical: Vec2,
) -> Option<f32> {
    let view = transform.affine().inverse();
    let projection = camera.clip_from_view();
    if !projection.is_finite() || projection.determinant().abs() < 1e-12 { return None; }
    let lo = canvas.min.as_dvec2(); let hi = canvas.max.as_dvec2();
    let boundary = [lo, DVec2::new(hi.x,lo.y), hi, DVec2::new(lo.x,hi.y)];
    let expected = (hi.x-lo.x)*(hi.y-lo.y);
    let tolerance = expected * 1e-6;
    let mut covered = 0.0;
    let mut polygons: Vec<Vec<DVec2>> = Vec::new();
    let mut lower = f64::INFINITY;
    for &[a,b,c] in &glass.triangles {
        let indices = [a,b,c];
        if indices.iter().any(|&i| i>=glass.points.len() || i>=glass.uvs.len()) { return None; }
        let points = indices.map(|i| uv_to_canvas.transform_point2(glass.uvs[i]).as_dvec2());
        if !points.iter().all(|p| p.is_finite()) { return None; }
        let determinant = (points[1]-points[0]).perp_dot(points[2]-points[0]);
        if determinant.abs() < 1e-12 { continue; }
        let mut triangle = points.to_vec();
        if determinant < 0.0 { triangle.reverse(); }
        let polygon = intersect(triangle, &boundary);
        let size = area(&polygon).abs();
        if size <= tolerance * 1e-6 { continue; }
        // Overlapping UV faces would make reverse correspondence ambiguous.
        if polygons.iter().any(|old| area(&intersect(polygon.clone(),old)).abs() > tolerance) { return None; }
        let h: [DVec4;3] = indices.map(|i| {
            let p = view.transform_point3(glass.points[i]);
            (projection * p.extend(1.0)).as_dvec4()
        });
        if !h.iter().all(|v| v.is_finite()) { return None; }
        let e1 = points[1]-points[0]; let e2 = points[2]-points[0];
        let du = ((h[1]-h[0])*e2.y - (h[2]-h[0])*e1.y)/determinant;
        let dv = ((h[2]-h[0])*e1.x - (h[1]-h[0])*e2.x)/determinant;
        let mut w_min = f64::INFINITY; let mut w_max: f64 = 0.0;
        let mut numerators = [0.0_f64;4];
        for &point in &polygon {
            let q = h[0]+du*(point.x-points[0].x)+dv*(point.y-points[0].y);
            if !q.is_finite() || q.w<=0.0 || q.z<0.0 || q.z>q.w
                || q.x.abs()>q.w || q.y.abs()>q.w { return None; }
            w_min = w_min.min(q.w); w_max = w_max.max(q.w);
            for (bound,value) in numerators.iter_mut().zip([
                du.x*q.w-du.w*q.x, dv.x*q.w-dv.w*q.x,
                du.y*q.w-du.w*q.y, dv.y*q.w-dv.w*q.y,
            ]) { *bound = bound.max(value.abs()); }
        }
        let sx = f64::from(physical.x)*0.5; let sy = f64::from(physical.y)*0.5;
        let xyz = |v: DVec4| DVec3::new(v.x,v.y,v.w);
        let det_lower = xyz(du).dot(xyz(dv).cross(xyz(h[0]))).abs()*sx*sy/w_max.powi(3);
        let frobenius_upper = (
            (numerators[0]*sx).powi(2)+(numerators[1]*sx).powi(2)
            +(numerators[2]*sy).powi(2)+(numerators[3]*sy).powi(2)
        ).sqrt()/w_min.powi(2);
        let squared = frobenius_upper.powi(2);
        let maximum_singular = ((squared + (squared.powi(2)-4.0*det_lower.powi(2)).max(0.0).sqrt())*0.5).sqrt();
        let bound = det_lower/maximum_singular;
        if !bound.is_finite() || bound<=0.0 { return None; }
        lower = lower.min(bound);
        covered += size; polygons.push(polygon);
    }
    if (covered-expected).abs()>tolerance || !lower.is_finite() { return None; }
    // Leave room for floating-point rounding when publishing an f32 lower bound.
    let result = (lower * 0.9999) as f32;
    (result.is_finite() && result>0.0).then_some(result)
}
