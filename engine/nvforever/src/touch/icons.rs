//! Owned SVG geometry rasterized after layout at physical image-box dimensions.
//! Artwork is authoritative in assets/touch; tools/android/svg_icons.py validates the
//! SVG profile and regenerates icons_generated.rs by hand so Cargo builds need no Python.
//! No position, binding or theme state lives here.
use bevy::{
    asset::{AssetEventSystems, RenderAssetUsages},
    camera::visibility::VisibilitySystems,
    image::ImageSampler,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    ui::UiSystems,
};
use std::collections::HashMap;
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};

const MAX_ENTRIES: usize = 64;
const MAX_DIMENSION: u32 = 512;
const MAX_BYTES: usize = 8 * 1024 * 1024;
pub(super) const FAMILY_KEY: &str = "touch-icon-family";

#[derive(Clone, Copy)]
enum Verb {
    M(f32, f32),
    L(f32, f32),
    Q(f32, f32, f32, f32),
    C(f32, f32, f32, f32, f32, f32),
    Z,
}
enum Geometry {
    Path(&'static [Verb]),
    Circle(f32, f32, f32),
}
struct Primitive {
    geometry: Geometry,
    fill: bool,
    stroke: bool,
}
struct IconFamily {
    id: &'static str,
    label: &'static str,
    stroke_width: f32,
    line_cap: LineCap,
    line_join: LineJoin,
    icons: &'static [IconSource],
}
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
struct FamilyChoice(usize);

struct IconSource {
    id: &'static str,
    label: &'static str,
    primitives: &'static [Primitive],
}
include!("icons_generated.rs");

/// Manifest index, deliberately independent of positions, bindings and appearance.
#[derive(Component)]
pub(super) struct TouchIcon(pub usize);

/// Stable artwork identity lookup for presentation adapters; action/layout IDs stay put.
pub(super) fn semantic_index(id: &str) -> Option<usize> {
    FAMILIES
        .first()?
        .icons
        .iter()
        .position(|source| source.id == id)
}

pub(super) fn install(app: &mut App) {
    debug!("Owned touch SVG source {SVG_SOURCE_SHA256}");
    app.init_resource::<FamilyChoice>();
    if cfg!(target_os = "android") {
        register_family(app);
    }
    app.init_resource::<IconCache>().add_systems(
        PostUpdate,
        update
            .after(UiSystems::Layout)
            .after(VisibilitySystems::VisibilityPropagate)
            .before(AssetEventSystems),
    );
}

pub(super) fn next_family(current: Option<&str>) -> Option<&'static str> {
    let index = current.and_then(|name| FAMILIES.iter().position(|f| f.id == name));
    match index {
        None => Some(FAMILIES[0].id),
        Some(i) => FAMILIES.get(i+1).map(|family|family.id),
    }
}
fn pick_family(world: &mut World, name: &str) -> bool {
    let Some(index) = FAMILIES
        .iter()
        .position(|f| f.id.eq_ignore_ascii_case(name.trim()))
    else {
        return false;
    };
    world.resource_mut::<FamilyChoice>().0 = index;
    true
}
fn family_chosen(world: &World) -> String {
    FAMILIES[world.resource::<FamilyChoice>().0].id.to_string()
}
fn register_family(app: &mut App) {
    use nvf_play::settings::{self, ChoiceList, Setting};
    if app
        .world()
        .get_resource::<settings::Settings>()
        .is_some_and(|s| s.index(FAMILY_KEY).is_some())
    {
        return;
    }
    settings::add_choice_list(
        app,
        Setting {
            key: FAMILY_KEY,
            group: "Touch",
            label: "Touch icon family",
            note: "Choose Precise line, Bold filled, Rounded or Industrial action icons.",
            switches: "",
            get: |_| false,
            set: |_, _| {},
            slider: None,
        },
        ChoiceList {
            names: |_| FAMILIES.iter().map(|f| f.id.to_string()).collect(),
            chosen: family_chosen,
            pick: pick_family,
            save: family_chosen,
            load: pick_family,
            ask_keep: false,
        },
    );
    settings::set_choice_labels(app, FAMILY_KEY, |_, id| {
        FAMILIES
            .iter()
            .find(|f| f.id == id)
            .map_or_else(|| id.to_string(), |f| f.label.to_string())
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RasterKey {
    family: usize,
    index: usize,
    size: UVec2,
}
impl RasterKey {
    fn bytes(self) -> usize {
        self.size.x as usize * self.size.y as usize * 4
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct Request {
    family: usize,
    index: usize,
    x: u32,
    y: u32,
}
impl Request {
    fn new(family: usize, index: usize, size: Vec2) -> Self {
        Self {
            family,
            index,
            x: size.x.to_bits(),
            y: size.y.to_bits(),
        }
    }
}
#[derive(Default)]
struct Entry {
    key: Option<RasterKey>,
    handle: Option<Handle<Image>>,
    rejected: Option<(Request, usize)>,
    bytes: usize,
}
#[derive(Resource, Default)]
struct IconCache {
    entries: HashMap<Entity, Entry>,
    bytes: usize,
    warned_capacity: bool,
}

fn physical_size(size: Vec2) -> Result<UVec2, &'static str> {
    // ComputedNode already includes DPI and UiScale. Never multiply density again.
    if !size.is_finite() || size.min_element() <= 0.0 {
        return Err("invalid physical dimensions");
    }
    let rounded = size.ceil();
    if rounded.max_element() > MAX_DIMENSION as f32 {
        return Err("physical dimensions exceed the icon budget");
    }
    Ok(rounded.as_uvec2())
}
fn fits_bytes(current: usize, replaced: usize, next: usize) -> bool {
    current
        .checked_sub(replaced)
        .and_then(|n| n.checked_add(next))
        .is_some_and(|n| n <= MAX_BYTES)
}
fn release(entry: &mut Entry, images: &mut Assets<Image>, bytes: &mut usize) {
    if let Some(handle) = entry.handle.take() {
        images.remove(handle.id());
    }
    *bytes -= entry.bytes;
    entry.bytes = 0;
    entry.key = None;
}

fn cache_visible(node: &ComputedNode, visibility: &InheritedVisibility) -> bool {
    // content_box constructs a Rect, which panics for NaN or negative raw sizes.
    // Reject invalid layout before that construction, not only in physical_size.
    visibility.get()
        && node.size().is_finite()
        && node.size().cmpgt(Vec2::ZERO).all()
        && !node.content_box().size().cmple(Vec2::ZERO).any()
}

fn update(
    family: Res<FamilyChoice>,
    layout: Option<Res<super::Layout>>,
    mut cache: ResMut<IconCache>,
    mut images: ResMut<Assets<Image>>,
    icons: Query<(Entity, &TouchIcon, &ComputedNode, &InheritedVisibility)>,
    mut nodes: Query<&mut ImageNode>,
) {
    let family = FamilyChoice(
        layout
            .as_ref()
            .and_then(|l| l.config.appearance.family.as_deref())
            .and_then(|name| FAMILIES.iter().position(|f| f.id == name))
            .unwrap_or(family.0),
    );

    // Retire hidden and removed nodes before any new allocation, independent of
    // query order. Visibility::Hidden keeps nonzero layout geometry; the propagated
    // hierarchy visibility, not that geometry alone, decides whether a glyph draws.
    let mut reclaimed = 0;
    cache.entries.retain(|entity, entry| {
        let live = icons
            .get(*entity)
            .is_ok_and(|(_, _, computed, visibility)| cache_visible(computed, visibility))
            && nodes.contains(*entity);
        if !live {
            if let Some(handle) = &entry.handle {
                // A surviving node must drop our strong handle as well. Bevy's
                // renderer retires GPU assets on Unused, not merely Removed.
                if let Ok(mut node) = nodes.get_mut(*entity) {
                    if node.image == *handle {
                        node.image = ImageNode::default().image;
                    }
                }
                images.remove(handle.id());
            }
            reclaimed += entry.bytes;
        }
        live
    });
    cache.bytes -= reclaimed;
    for (entity, icon, computed, visibility) in &icons {
        if !cache_visible(computed, visibility) {
            continue;
        }
        let Ok(mut node) = nodes.get_mut(entity) else {
            continue;
        };
        // Raster already includes uniform letterboxing to this exact content box.
        // Auto would use ImageNodeSize from before this post-layout resize.
        node.image_mode = NodeImageMode::Stretch;
        node.visual_box = VisualBox::ContentBox;
        if cache.entries.len() >= MAX_ENTRIES && !cache.entries.contains_key(&entity) {
            node.image = ImageNode::default().image;
            if !cache.warned_capacity {
                warn!("Touch SVG cache is full ({MAX_ENTRIES} live nodes); extra icons are transparent");
                cache.warned_capacity = true;
            }
            continue;
        }
        let mut entry = cache.entries.remove(&entity).unwrap_or_default();
        // ImageNode uses ContentBox. Insets can change independently of node size.
        let size = computed.content_box().size();
        let request = Request::new(family.0, icon.0, size);
        if entry.rejected == Some((request, cache.bytes)) {
            node.image = ImageNode::default().image;
            cache.entries.insert(entity, entry);
            continue;
        }
        let result = physical_size(size).and_then(|size| {
            let key = RasterKey {
                family: family.0,
                index: icon.0,
                size,
            };
            if FAMILIES
                .get(key.family)
                .and_then(|f| f.icons.get(key.index))
                .is_none()
            {
                return Err("unknown owned SVG identity");
            }
            if !fits_bytes(cache.bytes, entry.bytes, key.bytes()) {
                return Err("live icon texture bytes exceed the budget");
            }
            Ok(key)
        });
        let result = result.and_then(|key| {
            if entry.key == Some(key)
                && entry
                    .handle
                    .as_ref()
                    .is_some_and(|h| images.contains(h.id()))
            {
                return Ok(key);
            }
            let image = rasterize(key).ok_or("owned SVG rasterization failed")?;
            let replacement = match entry.handle.as_ref().and_then(|h| images.get_mut(h.id())) {
                Some(existing) => {
                    *existing.into_inner() = image;
                    None
                }
                None => Some(image),
            };
            if let Some(image) = replacement {
                entry.handle = Some(images.add(image));
            }
            cache.bytes = cache.bytes - entry.bytes + key.bytes();
            entry.bytes = key.bytes();
            entry.key = Some(key);
            Ok(key)
        });
        match result {
            Ok(_) => {
                entry.rejected = None;
                node.image = entry
                    .handle
                    .clone()
                    .expect("successful raster has a texture");
            }
            Err(reason) => {
                let (id, label) = FAMILIES
                    .get(family.0)
                    .and_then(|f| f.icons.get(icon.0))
                    .map_or(("unknown", "unknown"), |s| (s.id, s.label));
                warn!("Touch SVG {id} ({label}) refused: {reason}");
                release(&mut entry, &mut images, &mut cache.bytes);
                entry.rejected = Some((request, cache.bytes));
                node.image = ImageNode::default().image;
            }
        }
        // Do not touch node.color: appearance owns tint and idle/pressed opacity.
        cache.entries.insert(entity, entry);
    }
}

fn rasterize(key: RasterKey) -> Option<Image> {
    let family = FAMILIES.get(key.family)?;
    let source = family.icons.get(key.index)?;
    debug_assert_eq!(semantic_index(source.id), Some(key.index));
    let mut pixmap = Pixmap::new(key.size.x, key.size.y)?;
    let scale = key.size.min_element() as f32 / 32.0;
    let transform = Transform::from_row(
        scale,
        0.0,
        0.0,
        scale,
        (key.size.x as f32 - 32.0 * scale) / 2.0,
        (key.size.y as f32 - 32.0 * scale) / 2.0,
    );
    let mut paint = Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    paint.anti_alias = true;
    let stroke = Stroke {
        width: family.stroke_width,
        line_cap: family.line_cap,
        line_join: family.line_join,
        ..Stroke::default()
    };
    for primitive in source.primitives {
        let mut builder = PathBuilder::new();
        match &primitive.geometry {
            Geometry::Circle(x, y, r) => builder.push_circle(*x, *y, *r),
            Geometry::Path(commands) => {
                for command in *commands {
                    match *command {
                        Verb::M(x, y) => builder.move_to(x, y),
                        Verb::L(x, y) => builder.line_to(x, y),
                        Verb::Q(x1, y1, x, y) => builder.quad_to(x1, y1, x, y),
                        Verb::C(x1, y1, x2, y2, x, y) => builder.cubic_to(x1, y1, x2, y2, x, y),
                        Verb::Z => builder.close(),
                    }
                }
            }
        }
        let path = builder.finish()?;
        if primitive.fill {
            pixmap.fill_path(&path, &paint, FillRule::EvenOdd, transform, None);
        }
        if primitive.stroke {
            pixmap.stroke_path(&path, &paint, &stroke, transform, None);
        }
    }
    // tiny-skia is premultiplied RGBA; Bevy images here use straight alpha. For
    // white coverage masks, RGB must be white even at antialiased/transparent pixels.
    let mut data = pixmap.take();
    for pixel in data.as_chunks_mut::<4>().0 {
        pixel[..3].fill(255);
    }
    let mut image = Image::new(
        Extent3d {
            width: key.size.x,
            height: key.size.y,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::linear();
    Some(image)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::asset::{AssetApp, AssetEvent, AssetPlugin};
    #[derive(Resource)]
    struct LayoutSize(Vec2);
    fn test_layout(size: Res<LayoutSize>, mut nodes: Query<&mut ComputedNode, With<TouchIcon>>) {
        for mut node in &mut nodes {
            node.size = size.0;
        }
    }
    fn events(app: &mut App) -> Vec<AssetEvent<Image>> {
        app.world_mut()
            .resource_mut::<Messages<AssetEvent<Image>>>()
            .drain()
            .collect()
    }
    fn test_app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>();
        install(&mut app);
        app
    }
    fn spawn(app: &mut App, index: usize, size: Vec2) -> Entity {
        app.world_mut()
            .spawn((
                TouchIcon(index),
                ComputedNode { size, ..default() },
                // Legacy headless raster fixtures publish computed geometry/visibility.
                // The visibility-budget regression below uses real propagation instead.
                InheritedVisibility::VISIBLE,
                ImageNode::default(),
            ))
            .id()
    }
    #[test]
    fn physical_sizes_already_include_dpi_and_round_up() {
        for (logical, dpi) in [(24.0, 1.0), (24.0, 2.625), (48.0, 3.0)] {
            let physical = Vec2::splat(logical * dpi);
            assert_eq!(
                physical_size(physical),
                Ok(UVec2::splat((logical * dpi).ceil() as u32))
            );
        }
        assert_eq!(physical_size(Vec2::new(63.1, 31.2)), Ok(UVec2::new(64, 32)));
        for size in [
            Vec2::ZERO,
            Vec2::splat(-1.0),
            Vec2::splat(f32::NAN),
            Vec2::splat(f32::INFINITY),
            Vec2::splat(512.01),
        ] {
            assert!(physical_size(size).is_err());
        }
    }
    #[test]
    fn invalid_layout_is_rejected_before_constructing_its_content_rect() {
        let mut app = test_app();
        let entity = spawn(&mut app, 0, Vec2::splat(40.0));
        for size in [
            Vec2::splat(f32::NAN),
            Vec2::new(f32::NAN, 40.0),
            Vec2::new(40.0, f32::NAN),
            Vec2::splat(f32::INFINITY),
            Vec2::splat(f32::NEG_INFINITY),
            Vec2::ZERO,
            Vec2::new(-1.0, 40.0),
            Vec2::new(40.0, -1.0),
        ] {
            let node = ComputedNode { size, ..default() };
            assert!(
                !cache_visible(&node, &InheritedVisibility::VISIBLE),
                "invalid raw layout {size:?} must be rejected without constructing a Rect"
            );
            app.world_mut()
                .get_mut::<ComputedNode>(entity)
                .unwrap()
                .size = Vec2::splat(40.0);
            app.update();
            let retired = app.world().get::<ImageNode>(entity).unwrap().image.id();
            assert_eq!(app.world().resource::<Assets<Image>>().len(), 1);
            assert_eq!(app.world().resource::<IconCache>().bytes, 40 * 40 * 4);
            events(&mut app);
            app.world_mut()
                .get_mut::<ComputedNode>(entity)
                .unwrap()
                .size = size;
            app.update();
            assert_eq!(
                app.world().get::<ImageNode>(entity).unwrap().image,
                ImageNode::default().image,
                "invalid raw layout {size:?} must clear the old node image"
            );
            assert!(app.world().resource::<Assets<Image>>().is_empty());
            let cache = app.world().resource::<IconCache>();
            assert!(cache.entries.is_empty());
            assert_eq!(cache.bytes, 0);
            app.update();
            assert!(
                events(&mut app).iter().any(|e| e.is_unused(retired)),
                "invalid raw layout {size:?} must release the final strong handle"
            );
        }
        let mut node = ComputedNode {
            size: Vec2::splat(40.0),
            ..default()
        };
        assert!(cache_visible(&node, &InheritedVisibility::VISIBLE));
        assert!(!cache_visible(&node, &InheritedVisibility::HIDDEN));
        node.padding = BorderRect {
            min_inset: Vec2::splat(20.0),
            max_inset: Vec2::splat(20.0),
        };
        assert!(
            !cache_visible(&node, &InheritedVisibility::VISIBLE),
            "a collapsed content box remains ineligible even with a valid raw size"
        );
    }
    #[test]
    fn resize_replaces_asset_and_tint_changes_reuse_it() {
        let mut app = test_app();
        app.insert_resource(LayoutSize(Vec2::splat(37.0)))
            .add_systems(PostUpdate, test_layout.in_set(UiSystems::Layout));
        let entity = spawn(&mut app, 7, Vec2::ZERO);
        app.update();
        let handle = app.world().get::<ImageNode>(entity).unwrap().image.clone();
        assert!(
            events(&mut app).iter().any(|e| e.is_added(handle.id())),
            "first-create asset event must reach extraction this frame"
        );
        assert_eq!(
            app.world()
                .resource::<Assets<Image>>()
                .get(handle.id())
                .unwrap()
                .size(),
            UVec2::splat(37)
        );
        let tint = Color::srgba(0.2, 0.9, 0.6, 0.3);
        app.world_mut().resource_mut::<LayoutSize>().0 = Vec2::new(112.0, 73.0);
        app.world_mut().get_mut::<ImageNode>(entity).unwrap().color = tint;
        app.update();
        let node = app.world().get::<ImageNode>(entity).unwrap();
        assert_eq!(node.image, handle);
        assert_eq!(node.color, tint);
        assert_eq!(
            node.image_mode,
            NodeImageMode::Stretch,
            "same-frame extraction must ignore previous square ImageNodeSize"
        );
        assert_eq!(node.visual_box, VisualBox::ContentBox);
        assert!(
            events(&mut app).iter().any(|e| e.is_modified(handle.id())),
            "same-handle resize asset event must reach extraction this frame"
        );
        let images = app.world().resource::<Assets<Image>>();
        assert_eq!(images.len(), 1);
        assert_eq!(images.get(handle.id()).unwrap().size(), UVec2::new(112, 73));
        let pixels = images
            .get(handle.id())
            .unwrap()
            .data
            .as_ref()
            .unwrap()
            .as_ptr();
        app.world_mut().get_mut::<ImageNode>(entity).unwrap().color = Color::WHITE;
        app.update();
        assert_eq!(app.world().get::<ImageNode>(entity).unwrap().image, handle);
        let images = app.world().resource::<Assets<Image>>();
        assert_eq!(
            images
                .get(handle.id())
                .unwrap()
                .data
                .as_ref()
                .unwrap()
                .as_ptr(),
            pixels
        );
        assert_eq!(app.world().resource::<IconCache>().bytes, 112 * 73 * 4);
        assert!(
            !events(&mut app).iter().any(|e| e.is_modified(handle.id())),
            "tint-only updates must reuse raster pixels"
        );
        app.world_mut()
            .get_mut::<ComputedNode>(entity)
            .unwrap()
            .padding = BorderRect {
            min_inset: Vec2::splat(2.0),
            max_inset: Vec2::splat(3.0),
        };
        app.update();
        assert_eq!(app.world().get::<ImageNode>(entity).unwrap().image, handle);
        assert_eq!(
            app.world()
                .resource::<Assets<Image>>()
                .get(handle.id())
                .unwrap()
                .size(),
            UVec2::new(107, 68)
        );
        app.world_mut().despawn(entity);
        app.update();
        assert_eq!(app.world().resource::<Assets<Image>>().len(), 0);
        assert_eq!(app.world().resource::<IconCache>().bytes, 0);
    }
    #[test]
    fn invalid_request_is_transparent_releases_old_image_and_can_recover() {
        let mut app = test_app();
        let entity = spawn(&mut app, 0, Vec2::splat(40.0));
        app.update();
        app.world_mut()
            .get_mut::<ComputedNode>(entity)
            .unwrap()
            .size = Vec2::splat(f32::NAN);
        app.update();
        assert_eq!(app.world().resource::<Assets<Image>>().len(), 0);
        assert_eq!(
            app.world().get::<ImageNode>(entity).unwrap().image,
            ImageNode::default().image
        );
        app.world_mut()
            .get_mut::<ComputedNode>(entity)
            .unwrap()
            .size = Vec2::splat(80.0);
        app.update();
        assert_eq!(app.world().resource::<Assets<Image>>().len(), 1);
        app.world_mut().get_mut::<TouchIcon>(entity).unwrap().0 = usize::MAX;
        app.update();
        assert_eq!(app.world().resource::<IconCache>().bytes, 0);
        app.world_mut().get_mut::<TouchIcon>(entity).unwrap().0 = 0;
        app.update();
        let retired = app.world().get::<ImageNode>(entity).unwrap().image.id();
        events(&mut app);
        app.world_mut().entity_mut(entity).remove::<TouchIcon>();
        app.update();
        assert_eq!(
            app.world().get::<ImageNode>(entity).unwrap().image,
            ImageNode::default().image
        );
        assert_eq!(app.world().resource::<Assets<Image>>().len(), 0);
        assert!(events(&mut app).iter().any(|e| e.is_removed(retired)));
        app.update();
        assert!(
            events(&mut app).iter().any(|e| e.is_unused(retired)),
            "final strong handle must drop so renderer can retire GPU image"
        );
        app.world_mut().entity_mut(entity).insert(TouchIcon(0));
        app.update();
        let retired = app.world().get::<ImageNode>(entity).unwrap().image.id();
        events(&mut app);
        app.world_mut().entity_mut(entity).remove::<ComputedNode>();
        app.update();
        assert_eq!(
            app.world().get::<ImageNode>(entity).unwrap().image,
            ImageNode::default().image
        );
        assert_eq!(app.world().resource::<IconCache>().bytes, 0);
        app.update();
        assert!(events(&mut app).iter().any(|e| e.is_unused(retired)));
        // Retiring our cache must not clear an image another system has installed.
        app.world_mut().entity_mut(entity).insert(ComputedNode {
            size: Vec2::splat(24.0),
            ..default()
        });
        app.update();
        let external = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::default());
        app.world_mut().get_mut::<ImageNode>(entity).unwrap().image = external.clone();
        app.world_mut().entity_mut(entity).remove::<TouchIcon>();
        app.update();
        assert_eq!(
            app.world().get::<ImageNode>(entity).unwrap().image,
            external
        );
    }
    #[test]
    fn cache_is_bounded_by_nodes_and_live_bytes_including_resize() {
        assert!(fits_bytes(MAX_BYTES, 1024, 1024));
        assert!(!fits_bytes(MAX_BYTES, 1024, 1028));
        assert!(!fits_bytes(0, 1, 1));
        assert!(!fits_bytes(usize::MAX, 0, 4));
        let mut app = test_app();
        for _ in 0..MAX_ENTRIES + 2 {
            spawn(&mut app, 0, Vec2::splat(8.0));
        }
        app.update();
        assert_eq!(
            app.world().resource::<IconCache>().entries.len(),
            MAX_ENTRIES
        );
        assert_eq!(app.world().resource::<Assets<Image>>().len(), MAX_ENTRIES);
        let mut app = test_app();
        let nodes: Vec<_> = (0..9)
            .map(|_| spawn(&mut app, 0, Vec2::splat(512.0)))
            .collect();
        app.update();
        assert_eq!(app.world().resource::<IconCache>().bytes, MAX_BYTES);
        assert_eq!(app.world().resource::<Assets<Image>>().len(), 8);
        let allocated = *nodes
            .iter()
            .find(|entity| {
                app.world().get::<ImageNode>(**entity).unwrap().image != ImageNode::default().image
            })
            .unwrap();
        app.world_mut().entity_mut(allocated).remove::<TouchIcon>();
        app.update();
        // Removing a live marker frees one slot; the refused unchanged request retries.
        assert_eq!(app.world().resource::<Assets<Image>>().len(), 8);
        assert_eq!(app.world().resource::<IconCache>().bytes, MAX_BYTES);
        assert_eq!(app.world().resource::<IconCache>().entries.len(), 8);
    }
    #[test]
    fn actual_rasters_are_straight_alpha_distinct_and_letterboxed() {
        let mut signatures = std::collections::HashSet::new();
        assert_eq!(FAMILIES.len(), 4);
        assert_eq!(semantic_index("aim"), Some(6));
        assert_eq!(semantic_index("block"), Some(25));
        assert_eq!(semantic_index("gyro"), Some(26));
        assert_eq!(semantic_index("settings"), Some(18));
        assert_eq!(semantic_index("help"), Some(19));
        assert_eq!(semantic_index("unknown"), None);
        assert_eq!(SVG_SOURCE_SHA256.len(), 64);
        for (family, art) in FAMILIES.iter().enumerate() {
            assert_eq!(art.icons.len(), 27);
            for (index, source) in art.icons.iter().enumerate() {
                assert!(!source.id.is_empty() && !source.label.is_empty());
                for size in [UVec2::splat(24), UVec2::splat(32), UVec2::splat(48)] {
                    let image = rasterize(RasterKey {
                        family,
                        index,
                        size,
                    })
                    .unwrap();
                    let data = image.data.as_ref().unwrap();
                    assert_eq!(data.len(), (size.x * size.y * 4) as usize);
                    let (pixels, remainder) = data.as_chunks::<4>();
                    assert!(remainder.is_empty());
                    assert!(pixels.iter().all(|p| p[..3] == [255, 255, 255]));
                    assert!(pixels.iter().any(|p| p[3] > 0));
                    assert!(pixels.iter().any(|p| p[3] == 0));
                    if size.x == 24 {
                        assert!(
                            signatures.insert((family, data.clone())),
                            "duplicate {}/{}",
                            art.id,
                            source.id
                        );
                    }
                }
            }
        }
        let image = rasterize(RasterKey {
            family: 0,
            index: 6,
            size: UVec2::new(112, 56),
        })
        .unwrap();
        let data = image.data.unwrap();
        let (pixels, remainder) = data.as_chunks::<4>();
        assert!(remainder.is_empty());
        assert!(pixels
            .iter()
            .enumerate()
            .filter(|(i, _)| i % 112 < 28 || i % 112 >= 84)
            .all(|(_, p)| p[3] == 0));
    }
    #[test]
    fn family_choice_uses_real_settings_ini_and_unknown_values_preserve_choice() {
        use nvf_play::settings::{self, Settings, SettingsFile};
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "nvf-icon-family-{}-{}.ini",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let mut app = test_app();
        register_family(&mut app);
        app.insert_resource(SettingsFile(Some(path.clone())));
        let row = app
            .world()
            .resource::<Settings>()
            .index(FAMILY_KEY)
            .unwrap();
        assert_eq!(
            app.world()
                .resource::<Settings>()
                .choice_label(app.world(), row, "bold-filled"),
            "Bold filled"
        );
        assert!(settings::pick(app.world_mut(), row, "bold-filled"));
        let saved = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            fnv_install::ini_get(&saved, "Settings", FAMILY_KEY).as_deref(),
            Some("bold-filled")
        );
        let mut loaded = test_app();
        register_family(&mut loaded);
        loaded.insert_resource(SettingsFile(Some(path.clone())));
        settings::apply_stored(loaded.world_mut());
        assert_eq!(family_chosen(loaded.world()), "bold-filled");
        assert!(!settings::pick(loaded.world_mut(), row, "unknown-family"));
        assert_eq!(family_chosen(loaded.world()), "bold-filled");
        std::fs::write(&path, "[Settings]\ntouch-icon-family=unknown-family\n").unwrap();
        settings::apply_stored(loaded.world_mut());
        assert_eq!(family_chosen(loaded.world()), "bold-filled");
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn family_switch_replaces_same_handle_at_same_size_and_preserves_tint_and_node() {
        use nvf_play::settings::{self, Settings};
        let mut app = test_app();
        register_family(&mut app);
        let entity = spawn(&mut app, 7, Vec2::splat(32.0));
        let tint = Color::srgba(0.3, 0.6, 0.9, 0.4);
        app.world_mut().get_mut::<ImageNode>(entity).unwrap().color = tint;
        app.world_mut().get_mut::<Node>(entity).unwrap().left = px(41.0);
        app.update();
        let handle = app.world().get::<ImageNode>(entity).unwrap().image.clone();
        let row = app
            .world()
            .resource::<Settings>()
            .index(FAMILY_KEY)
            .unwrap();
        let mut before = app
            .world()
            .resource::<Assets<Image>>()
            .get(handle.id())
            .unwrap()
            .data
            .clone();
        events(&mut app);
        for (family, art) in FAMILIES.iter().enumerate().skip(1) {
            assert!(settings::pick(app.world_mut(), row, art.id));
            app.update();
            let node = app.world().get::<ImageNode>(entity).unwrap();
            assert_eq!(node.image, handle);
            assert_eq!(node.color, tint);
            assert_eq!(app.world().get::<Node>(entity).unwrap().left, px(41.0));
            let images = app.world().resource::<Assets<Image>>();
            let image = images.get(handle.id()).unwrap();
            assert_eq!(image.size(), UVec2::splat(32));
            assert_ne!(image.data, before);
            before = image.data.clone();
            assert_eq!(images.len(), 1);
            assert_eq!(app.world().resource::<IconCache>().bytes, 32 * 32 * 4);
            assert_eq!(
                app.world().resource::<IconCache>().entries[&entity]
                    .key
                    .unwrap()
                    .family,
                family
            );
            assert!(events(&mut app).iter().any(|e| e.is_modified(handle.id())));
        }
        let image = rasterize(RasterKey {
            family: 1,
            index: 6,
            size: UVec2::splat(32),
        })
        .unwrap();
        let pixels = image.data.unwrap();
        assert_eq!(
            pixels[(20 * 32 + 16) * 4 + 3],
            0,
            "EvenOdd aim ring has an open counter"
        );
        assert!(pixels[(24 * 32 + 16) * 4 + 3] > 0);
        let aid = rasterize(RasterKey {
            family: 1,
            index: 24,
            size: UVec2::splat(32),
        })
        .unwrap()
        .data
        .unwrap();
        assert_eq!(
            aid[(19 * 32 + 16) * 4 + 3],
            0,
            "medical cross is negative space"
        );
    }
    #[test]
    fn hidden_hierarchies_release_before_visible_allocations_and_controller_hiding_drops_assets() {
        use bevy::camera::visibility::VisibilityPlugin;
        let mut app = test_app();
        app.init_asset::<Mesh>()
            .init_asset::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>()
            .add_plugins((bevy::transform::TransformPlugin, VisibilityPlugin));
        let old_parent = app
            .world_mut()
            .spawn((Node::default(), Visibility::Visible))
            .id();
        let hidden_parent = app
            .world_mut()
            .spawn((Node::default(), Visibility::Hidden))
            .id();
        let mut old = Vec::new();
        for _ in 0..8 {
            let entity = spawn(&mut app, 7, Vec2::splat(512.0));
            app.world_mut()
                .entity_mut(entity)
                .insert(ChildOf(old_parent));
            old.push(entity);
        }
        let mut hidden = Vec::new();
        for _ in 0..MAX_ENTRIES + 20 {
            let entity = spawn(&mut app, 7, Vec2::splat(512.0));
            app.world_mut()
                .entity_mut(entity)
                .insert(ChildOf(hidden_parent));
            hidden.push(entity);
        }
        app.update();
        assert_eq!(app.world().resource::<IconCache>().bytes, MAX_BYTES);
        assert_eq!(app.world().resource::<IconCache>().entries.len(), 8);
        assert!(
            hidden
                .iter()
                .all(|e| app.world().get::<ImageNode>(*e).unwrap().image
                    == ImageNode::default().image)
        );
        let retired: Vec<_> = old
            .iter()
            .map(|e| app.world().get::<ImageNode>(*e).unwrap().image.id())
            .collect();
        events(&mut app);
        // New visible nodes are spawned before the old hierarchy is hidden. Retirement
        // must free its complete budget before whichever node the query happens to visit.
        let visible: Vec<_> = (0..8)
            .map(|_| spawn(&mut app, 7, Vec2::splat(512.0)))
            .collect();
        app.world_mut()
            .get_mut::<Visibility>(old_parent)
            .unwrap()
            .set_if_neq(Visibility::Hidden);
        app.update();
        assert_eq!(app.world().resource::<IconCache>().bytes, MAX_BYTES);
        assert_eq!(app.world().resource::<IconCache>().entries.len(), 8);
        assert!(
            old.iter()
                .chain(&hidden)
                .all(|e| app.world().get::<ImageNode>(*e).unwrap().image
                    == ImageNode::default().image)
        );
        assert!(
            visible
                .iter()
                .all(|e| app.world().get::<ImageNode>(*e).unwrap().image
                    != ImageNode::default().image)
        );
        assert!(
            old.iter()
                .all(|e| app.world().get::<ComputedNode>(*e).unwrap().size() == Vec2::splat(512.0)),
            "Visibility hiding must not require layout to collapse"
        );
        app.update();
        let retired_events = events(&mut app);
        assert!(retired
            .iter()
            .all(|id| retired_events.iter().any(|e| e.is_unused(*id))));
        // Controller/UI mode hiding changes Visibility alone, just like show_labels.
        for entity in &visible {
            *app.world_mut().get_mut::<Visibility>(*entity).unwrap() = Visibility::Hidden;
        }
        app.update();
        assert_eq!(app.world().resource::<IconCache>().bytes, 0);
        assert!(app.world().resource::<IconCache>().entries.is_empty());
        assert!(app.world().resource::<Assets<Image>>().is_empty());
        for entity in visible.iter().take(3) {
            *app.world_mut().get_mut::<Visibility>(*entity).unwrap() = Visibility::Inherited;
        }
        app.update();
        assert_eq!(app.world().resource::<IconCache>().entries.len(), 3);
        assert_eq!(app.world().resource::<IconCache>().bytes, 3 * 512 * 512 * 4);
        assert!(
            visible
                .iter()
                .take(3)
                .all(|e| app.world().get::<ImageNode>(*e).unwrap().image
                    != ImageNode::default().image)
        );
        app.world_mut()
            .get_mut::<ComputedNode>(visible[0])
            .unwrap()
            .size = Vec2::ZERO;
        app.update();
        assert_eq!(app.world().resource::<IconCache>().bytes, 2 * 512 * 512 * 4);
        assert_eq!(
            app.world().get::<ImageNode>(visible[0]).unwrap().image,
            ImageNode::default().image
        );
    }
}
