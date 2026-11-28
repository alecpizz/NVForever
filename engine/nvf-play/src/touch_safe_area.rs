//! Shared logical-pixel safe area; platform snapshots never control game state.
use bevy::prelude::*;
/// Platform insets publish before consumers retire stale presentation/input.
#[derive(SystemSet,Debug,Clone,Copy,PartialEq,Eq,Hash)]
pub struct Publish;
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct SafeInsets { pub left: f32, pub top: f32, pub right: f32, pub bottom: f32 }
impl SafeInsets {
    pub fn from_array(values: [f32;4]) -> Option<Self> {
        values.iter().all(|x| x.is_finite() && (0.0..=512.0).contains(x)).then_some(Self { left:values[0], top:values[1], right:values[2], bottom:values[3] })
    }
    /// Snapshot: version, physical window width/height, physical L/T/R/B.
    /// A stale window size is rejected rather than interpreted in the new geometry.
    pub fn from_snapshot(text: &str, physical: UVec2, density: f32) -> Option<Self> {
        if text.len()>160 || !density.is_finite() || density<=0.0 || physical.min_element()==0 { return None }
        let values:Vec<f32>=text.split(',').map(str::parse).collect::<Result<_,_>>().ok()?;
        if values.len()!=7 || values[0]!=1.0 || values.iter().any(|v|!v.is_finite())
            || values[1]!=physical.x as f32 || values[2]!=physical.y as f32
            || values[3..].iter().any(|v|*v<0.0) { return None }
        let max=physical.as_vec2()/density*0.45;
        Self::from_array([
            (values[3]/density).min(max.x).min(512.0),
            (values[4]/density).min(max.y).min(512.0),
            (values[5]/density).min(max.x).min(512.0),
            (values[6]/density).min(max.y).min(512.0),
        ])
    }
}
