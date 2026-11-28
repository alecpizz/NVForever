//! Typography within the ordinary menu's fixed viewport. This changes measured
//! wrapping and glyph dimensions together, never the whole canvas/hit transform.
use bevy::prelude::*;
use crate::interface::{self, Face};
use crate::menu_readability::{MenuKind, MenuReadability};

/// A shared atlas/metrics face plus this menu's current typography size. The font
/// and atlas are not cloned or reloaded when the saved size changes.
pub(crate) struct Typeface {
    face:Face,
    pub scale:f32,
}
impl From<Face> for Typeface {
    fn from(face:Face)->Self {Self {face,scale:1.0}}
}
impl std::ops::Deref for Typeface {
    type Target=Face;
    fn deref(&self)->&Face {&self.face}
}
impl Typeface {
    pub fn width(&self,text:&str)->f32 {self.font.width(text)*self.scale}
}

pub(crate) fn size(options: Option<&MenuReadability>, kind: MenuKind) -> f32 {
    options.copied().unwrap_or_default().size(kind)
}

/// The same metric conversion used by native layout and its actual renderer.
/// Keep the wrapping box fixed: measure a narrower unscaled line, then scale its
/// result. XML zoom retains its existing independent meaning.
pub(crate) fn measured(
    measure: &crate::tiles::menus::Size<'_>,
    scale: f32,
    font: f64,
    text: &str,
    wrap: Option<f64>,
) -> (f64, f64) {
    let scale=f64::from(scale);
    let (width,height)=measure(font,text,wrap.map(|w|w/scale));
    (width*scale,height*scale)
}

/// Draw the existing font glyphs at the measured scale, anchored in the unchanged
/// menu coordinate system. The local scope does not alter global settings.
pub(crate) fn line(
    parent:&mut ChildSpawnerCommands,
    face:&Face,
    line:&fnv_font::Line,
    anchor:Vec2,
    scale:f32,
    color:Color,
) {
    let _units=interface::units_at(interface::unit_scale()*scale);
    interface::set_line(parent,face,line,anchor.x/scale,anchor.y/scale,color);
}
