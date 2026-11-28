//! The terminal's monitor (`nvf_play::terminal`, docs/spec-notes/hacking.md §7): the
//! game's model `meshes\terminals\terminalinterface01.nif`, drawn by a camera of its own
//! into a picture shown over the darkened world ([`nvf_play::terminal::MonitorView`]),
//! with the terminal's screen on its glass and its power button leaving the terminal.
//!
//! - The parts, by shape name [observed: the file's nodes]: the case
//!   `TerminalInterface01:0`; the glass `screen:0` (textured `pipboy3000\pipboy.dds`, the
//!   rendered screen's stand-in) and over it `glare:0` (`TerminalScreenGlare.dds`, alpha
//!   blended); the power button `PowerButton:0` and its glow `Plane01:0`, under the screen
//!   on the right; three point lights (`Omni02`, `03`, `05`), not read.
//! - The screen's picture (`nvf_play::terminal::TerminalImage`) goes on `screen:0` unlit,
//!   as a lit display [hypothesis], spanning `screen::TEXTURE_UNITS` of the canvas.
//!   [design] Keep `glare:0`, added in linear light at a per-channel scale (§7), which
//!   gives the top-right streak [observed: owner's 2026-10-31 reference] without the
//!   floor the display-value add puts over the textured glass.
//! - The camera looks at the front (along +Y) with the field of view
//!   `fRenderedTerminalFOV` 0.15 [documented: Fallout_default.ini `[RenderedTerminal]`;
//!   hypothesis: radians, vertical], looking `fRenderedTerminalVPos` 0.38 below the
//!   model's origin [documented; hypothesis: model units; observed: the owner's
//!   screenshot puts the origin 0.37 above the middle of the view], from [`DISTANCE`] in
//!   front [observed: fitted to the same screenshot, where the glass is 0.735 of the view's
//!   height; `fRenderedTerminalZoom` 36 is not that distance as such]. Lights of its
//!   own on a render layer of its own [design], as the lockpicking view's
//!   (`lockpick_view.rs`).
//! - The window's pointer is followed onto the glass and the button on CPU copies of
//!   their shapes: where it lies on the glass goes to the screen
//!   (`nvf_play::terminal::TerminalPointer`), and a left click on the button leaves the
//!   terminal (the owner, 2026-10-16).

use crate::pipboy_arm::Posed;
pub(crate) mod projection;
mod dialogue;
pub(crate) use dialogue::DialogueTerminalPlugin;
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ImageRenderTarget, RenderTarget};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::AmbientLight;
use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::window::PrimaryWindow;
use nvf_assets::model::ModelCtx;
use nvf_assets::NifMaterial;
use nvf_play::terminal::screen::{CANVAS, TEXTURE_UNITS};
use nvf_play::terminal::{MonitorView, PowerAid, TerminalImage, TerminalInput, TerminalPointer, TerminalScreen, TARGET};

pub const MONITOR_MODEL: &str = "meshes\\terminals\\terminalinterface01.nif";
/// The render layer of the monitor, its lights and its camera.
pub const MONITOR_LAYER: usize = 8;
/// Where the monitor is put, far from the world and the lockpicking lock (Bevy space)
/// [design].
const PLACE: Vec3 = Vec3::new(0.0, -210_000.0, 0.0);
/// The camera's field of view, radians, and how far below the model's origin it looks
/// [documented: `[RenderedTerminal]`; hypothesis: their units, module docs].
const FOV: f32 = 0.15;
const DROP: f32 = 0.38;
/// The camera's distance in front of the model's origin, model units [observed, module
/// docs].
pub const DISTANCE: f32 = 38.5;

/// [design] The glare is added in linear light, as a plain unlit additive material on
/// the model's own texture, not through `nif_material.wesl`'s display-value add. The
/// shape is additive in the model (NiAlphaProperty flags 4109: blend on, SRC_ALPHA /
/// ONE; `TerminalScreenGlare.dds` DXT1, alpha 1 throughout, 4 to 15 in its dark parts
/// and (90, 110, 112) at the streak's peak [observed: fnvtool nif decode, PIL,
/// 2026-10-31]). [observed: in game, 1280 x 720, 2026-10-31, hacking.md §7] Through the
/// display-value add the dark texels put a floor of (13-26, 13-24, 7-22) over the whole
/// glass and (+30, 0, +25) on the attempt blocks, and the original shows no such floor
/// (corners (8, 11, 5), blocks (59-65, 255, 148-152) under a streak of +80); added in
/// linear light the same texels put 1 to 4 units on the glass and the streak where the
/// original has it. [hypothesis] A counter-example to nif.md §6's display-value add
/// for this one shape; the engine's rule for every other additive shape is untouched.
/// The linear scale on the texture, per channel: the original's peak (100, 125, 95)
/// less its glass (21, 44, 26), over the texture's peak, in linear light.
/// Recipe: on the hacking smoke shot measure the 10x10 streak peak; scale a channel
/// by (linear(target) - linear(glass beside it)) / (linear(measured) - linear(glass)).
const GLARE_LINEAR_SCALE: Vec3 = Vec3::new(1.19, 1.14, 0.64);

pub struct TerminalViewPlugin;

impl Plugin for TerminalViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (follow_pointer.before(nvf_play::PlaySet::Ui), show.after(nvf_play::PlaySet::Ui).before(nvf_play::PlaySet::Sync)));
    }
}

/// The monitor as drawn: its entities, its camera, and the glass and the button posed
/// where they are drawn.
#[derive(Default)]
struct Drawn {
    entities: Vec<Entity>,
    warned: bool,
    failed: bool,
}

#[derive(Resource)]
struct Monitor {
    camera: Entity,
    glass: Posed,
    button: Option<Posed>,
}

/// A shape's name as one of the monitor's parts.
fn is_glass(name: &str) -> bool {
    name.eq_ignore_ascii_case("screen:0")
}

fn is_button(name: &str) -> bool {
    name.eq_ignore_ascii_case("PowerButton:0")
}

#[allow(clippy::too_many_arguments)]
fn show(
    mut commands: Commands,
    screen: Res<TerminalScreen>,
    picture: Option<Res<TerminalImage>>,
    view: Option<Res<MonitorView>>,
    mut models: ModelCtx,
    mut plain: ResMut<Assets<StandardMaterial>>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut drawn: Local<Drawn>,
) {
    let Some(picture) = picture.filter(|_| screen.0.is_some()) else {
        if !drawn.entities.is_empty() || view.is_some() || drawn.failed {
            for e in drawn.entities.drain(..) {
                commands.entity(e).despawn();
            }
            *drawn = Drawn { warned: drawn.warned, ..default() };
            commands.remove_resource::<MonitorView>();
            commands.remove_resource::<Monitor>();
        }
        return;
    };
    if !drawn.entities.is_empty() || drawn.failed {
        return;
    }
    let Some(model) = models.model_with_cpu(MONITOR_MODEL) else {
        if !drawn.warned {
            warn!("terminal view: {MONITOR_MODEL} not loaded; the screen is drawn flat");
            drawn.warned = true;
        }
        return;
    };
    let size = window.single().map(|w| Extent3d { width: w.physical_width().max(1), height: w.physical_height().max(1), depth_or_array_layers: 1 }).unwrap_or(Extent3d { width: 1280, height: 720, depth_or_array_layers: 1 });
    let Some(view) = spawn_monitor(&mut commands, &mut models, &mut plain, &model,
        &picture.image, TARGET, size, MONITOR_LAYER, "Terminal", false, -4, false, true, PLACE) else {
        if !drawn.warned {
            warn!("terminal view: {MONITOR_MODEL} has no usable screen:0 shape; the screen is drawn flat");
            drawn.warned = true;
        }
        drawn.failed = true;
        return;
    };
    info!("terminal view: the monitor drawn ({} shapes, power button {}), the camera {DISTANCE} in front", view.shapes, if view.button.is_some() { "found" } else { "missing" });
    commands.insert_resource(Monitor { camera: view.camera, glass: view.glass, button: view.button });
    commands.insert_resource(MonitorView { image: view.image });
    drawn.entities = view.entities;
}

/// Fit every actual model-bound corner, including the housing, into a requested
/// aspect. The original terminal caller retains its measured camera unchanged.
fn fitted_camera((min,max): (Vec3,Vec3), aspect: f32) -> Option<(Vec3,Vec3,f32)> {
    if !min.is_finite() || !max.is_finite() || !min.cmplt(max).all()
        || !aspect.is_finite() || aspect<=0.0 { return None; }
    let center = (min+max)*0.5;
    let half = (max-min)*0.5;
    let tangent = (FOV*0.5).tan();
    let distance = half.y + (half.z/tangent).max(half.x/(tangent*aspect)).max(1.1)/0.95;
    let eye = center-Vec3::Y*distance;
    let far = distance+half.y+1.0;
    (eye.is_finite() && far.is_finite()).then_some((eye,center,far))
}

/// The existing monitor renderer shared by the terminal and an independent dialogue.
/// Neither construction nor projection opens a terminal or sends terminal input.
struct RenderedMonitor {
    entities: Vec<Entity>,
    camera: Entity,
    image: Handle<Image>,
    glass: Posed,
    button: Option<Posed>,
    blockers: Vec<std::sync::Arc<Posed>>,
    shapes: usize,
}

/// [design: docs/design/android.md] Fit the Retro Console dialogue camera straight-on
/// and tightly around the screen glass + power button (ignoring rear CRT tube depth)
/// so the console fills almost the full height and side width of the screen.
fn fitted_dialogue_camera(
    glass: &Posed,
    button: Option<&Posed>,
    model_bounds: (Vec3, Vec3),
    place_inv: Affine3A,
    aspect: f32,
) -> Option<(Vec3, Vec3, f32)> {
    if !aspect.is_finite() || aspect <= 0.0 { return None; }
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for &p in &glass.points {
        let local = place_inv.transform_point3(p);
        min = min.min(local);
        max = max.max(local);
    }
    if let Some(btn) = button {
        for &p in &btn.points {
            let local = place_inv.transform_point3(p);
            min = min.min(local);
            max = max.max(local);
        }
    }
    if !min.is_finite() || !max.is_finite() {
        return fitted_camera(model_bounds, aspect);
    }
    let center = (min + max) * 0.5;
    let half = (max - min) * 0.5;
    let tangent = (FOV * 0.5).tan();
    let frame_x = half.x * 1.12;
    let frame_z = half.z * 1.08;
    let front_y = (center.y - min.y).max(0.0);
    let distance = front_y + (frame_z / tangent).max(frame_x / (tangent * aspect)).max(1.1) * 1.01;
    let eye = center - Vec3::Y * distance;
    let back_y = (model_bounds.1.y - center.y).max(half.y).max(1.0);
    let far = distance + back_y + 2.0;
    (eye.is_finite() && far.is_finite()).then_some((eye, center, far))
}

#[allow(clippy::too_many_arguments)]
fn spawn_monitor(
    commands: &mut Commands,
    models: &mut ModelCtx,
    plain: &mut Assets<StandardMaterial>,
    model: &nvf_assets::PreparedModel,
    picture: &Handle<Image>,
    target: UVec2,
    size: Extent3d,
    layer_index: usize,
    label: &str,
    block_housing: bool,
    camera_order: isize,
    fit_model: bool,
    with_glare: bool,
    placement: Vec3,
) -> Option<RenderedMonitor> {
    let model_bounds = if fit_model { Some(model.bounds?) } else { None };
    // Game space (Z up) into Bevy's (Y up), as the world root does.
    let place = Affine3A::from_translation(placement) * Affine3A::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    // Validate glass, power button, blockers, and camera fit BEFORE allocating any
    // render-target image or spawning shape entities, so failures leak nothing.
    let mut glass = None;
    let mut button = None;
    let mut blockers = Vec::new();
    for s in &model.shapes {
        let name = s.name.as_deref().unwrap_or("");
        if !with_glare && name.eq_ignore_ascii_case("glare:0") {
            continue;
        }
        let world = place * model.root * s.transform;
        if is_glass(name) {
            glass = s.cpu.as_deref().and_then(|m| Posed::new(m, None, Mat4::from(world)));
        } else if is_button(name) {
            button = s.cpu.as_deref().and_then(|m| Posed::new(m, None, Mat4::from(world)));
        }
        if block_housing && !is_glass(name) && !name.eq_ignore_ascii_case("glare:0") {
            if let Some(posed) = s.cpu.as_deref().and_then(|m| Posed::new(m, None, Mat4::from(world))) {
                blockers.push(std::sync::Arc::new(posed));
            }
        }
    }
    let glass = glass?;
    let (eye, aim, far) = if let Some(bounds) = model_bounds {
        fitted_dialogue_camera(&glass, button.as_ref(), bounds, place.inverse(), size.width as f32 / size.height as f32)?
    } else {
        (Vec3::new(0.0, -DISTANCE, -DROP), Vec3::new(0.0, 0.0, -DROP), 200.0)
    };

    let mut image = Image::new_fill(size, TextureDimension::D2, &[0, 0, 0, 0], TextureFormat::Bgra8UnormSrgb, RenderAssetUsages::default());
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    let image = models.images.add(image);
    let layer = RenderLayers::layer(layer_index);
    let mut entities = Vec::new();
    for s in &model.shapes {
        let name = s.name.as_deref().unwrap_or("");
        if !with_glare && name.eq_ignore_ascii_case("glare:0") {
            continue;
        }
        let world = place * model.root * s.transform;
        let mut material = s.material.clone();
        let mut glare = None;
        if is_glass(name) {
            let mut m = models.materials.get(&s.material).cloned().unwrap_or_default();
            m.base.base_color = Color::WHITE;
            m.base.base_color_texture = Some(picture.clone());
            // [design] Preserve the measured texels instead of applying the model's
            // emissive colour to an already lit display (nif_material.wesl).
            m.base.emissive_texture = None;
            m.base.emissive = LinearRgba::BLACK;
            m.base.normal_map_texture = None;
            m.base.unlit = true;
            m.base.alpha_mode = AlphaMode::Opaque;
            m.base.uv_transform = bevy::math::Affine2::from_scale(TEXTURE_UNITS / target.as_vec2());
            // An unlit copy carries nothing of an authored set (#2981), as the Pip-Boy's.
            nvf_assets::authored::clear_for_unlit_copy(&mut m);
            material = models.materials.add(m);
        } else if name.eq_ignore_ascii_case("glare:0") {
            // [design] The model's texture on Bevy's own additive material: its colour,
            // texel × GLARE_LINEAR_SCALE, is added in linear light (GLARE_LINEAR_SCALE).
            let m = models.materials.get(&s.material).map(glare_material).unwrap_or_else(|| glare_material(&NifMaterial::default()));
            glare = Some(plain.add(m));
        }
        let mut shape = commands.spawn((Name::new(format!("{label} monitor {name}")), Mesh3d(s.mesh.clone()), Transform::from_matrix(Mat4::from(world)), layer.clone()));
        match glare {
            Some(plain) => shape.insert(MeshMaterial3d(plain)),
            None => shape.insert(MeshMaterial3d(material)),
        };
        entities.push(shape.id());
    }
    let shapes = entities.len();
    let to = |p: Vec3| place.transform_point3(p);
    let camera = commands
        .spawn((
            Name::new(format!("{label} monitor camera")),
            Camera3d::default(),
            Camera { order: camera_order, clear_color: ClearColorConfig::Custom(Color::NONE), ..default() },
            RenderTarget::Image(ImageRenderTarget::from(image.clone())),
            Projection::Perspective(PerspectiveProjection { fov: FOV, near: 1.0, far, ..default() }),
            Tonemapping::Linear,
            Msaa::Sample4,
            AmbientLight { color: Color::WHITE, brightness: 150.0, ..default() },
            Transform::from_translation(to(eye)).looking_at(to(aim), Vec3::Y),
            layer.clone(),
        ))
        .id();
    // A key light from the upper left in front and a fill from the right [design: as
    // bright as makes the case about the owner's screenshot's, (29, 28, 18)].
    let key = commands
        .spawn((DirectionalLight { illuminance: 2400.0, ..default() }, Transform::from_translation(to(Vec3::new(-20.0, -30.0, 25.0))).looking_at(to(Vec3::ZERO), Vec3::Y), layer.clone()))
        .id();
    let fill = commands
        .spawn((DirectionalLight { illuminance: 600.0, ..default() }, Transform::from_translation(to(Vec3::new(30.0, -20.0, -5.0))).looking_at(to(Vec3::ZERO), Vec3::Y), layer.clone()))
        .id();
    entities.extend([camera, key, fill]);
    Some(RenderedMonitor { entities, camera, image, glass, button, blockers, shapes })
}

/// Where a spot of the glass's texture lies on the canvas.
pub fn canvas_point(uv: Vec2) -> Option<Vec2> {
    let p = uv * TEXTURE_UNITS;
    (p.x >= 0.0 && p.y >= 0.0 && p.x < CANVAS.x && p.y < CANVAS.y).then_some(p)
}

type MonitorTouchContext<'w> = (
    Option<Res<'w, Monitor>>,
    Option<Res<'w, nvf_play::terminal::touch::Settings>>,
    Option<Res<'w, TerminalScreen>>,
);

/// Follow the window's pointer onto the monitor: on the glass it goes to the screen, a
/// left click on the power button leaves the terminal. The test aid `power` clicks the
/// button's middle.
fn follow_pointer(
    (monitor, touch, screen): MonitorTouchContext<'_>,
    cams: Query<(&Camera, &GlobalTransform)>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mouse: nvf_play::pad::ActionButtons<MouseButton>,
    mut aid: ResMut<PowerAid>,
    mut pointer: ResMut<TerminalPointer>,
    mut out: MessageWriter<TerminalInput>,
) {
    if touch.as_deref().zip(screen.as_deref())
        .is_some_and(|(settings, screen)| nvf_play::terminal::touch::captures(settings, screen)) {
        pointer.canvas = None;
        aid.0 = false;
        return;
    }
    let mouse = mouse.snapshot();
    let Some(m) = monitor else { return };
    let Ok((cam, at)) = cams.get(m.camera) else { return };
    let Ok(window) = windows.single() else { return };
    let aimed = aid.0.then(|| m.button.as_ref().and_then(Posed::middle).and_then(|p| cam.world_to_viewport(at, p).ok())).flatten();
    if aid.0 && aimed.is_none() {
        warn!("test aid: the terminal's power button is not in view");
        aid.0 = false;
    }
    // The camera draws a picture of the window's size, one pixel to a pixel.
    let cursor = aimed.or_else(|| window.physical_cursor_position());
    let ray = cursor.and_then(|c| cam.viewport_to_world(at, c).ok());
    pointer.canvas = None;
    let Some(ray) = ray else { return };
    let (origin, dir) = (ray.origin, Vec3::from(ray.direction));
    let glass = m.glass.hit(origin, dir);
    let button = m.button.as_ref().and_then(|b| b.hit(origin, dir));
    match (glass, button) {
        (Some((t, uv)), b) if b.is_none_or(|(bt, _)| t < bt) => pointer.canvas = canvas_point(uv),
        (_, Some(_)) if mouse.just_pressed(MouseButton::Left) || aid.0 => {
            info!("terminal: the power button pressed");
            out.write(TerminalInput::Exit);
            aid.0 = false;
        }
        _ => {}
    }
}

/// The glare's material on Bevy's own additive material, from the model's: its colour,
/// texel x GLARE_LINEAR_SCALE, added in linear light. The source's authored set comes off
/// first (#2981, `clear_for_unlit_copy`, as for the glass): no ORM, no depth map, and a height
/// overlay's depth, method and layers back to the legacy look's.
fn glare_material(source: &NifMaterial) -> StandardMaterial {
    let mut own = source.clone();
    nvf_assets::authored::clear_for_unlit_copy(&mut own);
    let mut m = own.base;
    m.base_color = Color::linear_rgb(GLARE_LINEAR_SCALE.x, GLARE_LINEAR_SCALE.y, GLARE_LINEAR_SCALE.z);
    m.emissive = LinearRgba::BLACK;
    m.emissive_texture = None;
    m.normal_map_texture = None;
    m.metallic_roughness_texture = None;
    m.occlusion_texture = None;
    m.depth_map = None;
    m.unlit = true;
    m.alpha_mode = AlphaMode::Add;
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #2981 review H3: the glare copy of a material wearing a height overlay keeps nothing of
    /// it: no depth map, and the legacy look's depth, method and layer count.
    #[test]
    fn the_glare_copy_drops_a_height_overlay_whole() {
        use nvf_assets::authored::{LegacyLook, Parallax};
        let mut images = Assets::<Image>::default();
        let mut source = nvf_assets::nif_material(StandardMaterial::default());
        let baseline = Parallax::default();
        source.extension.legacy = Some(LegacyLook { normal: None, specular: None, roughness: 0.7, reflectance: 0.3, metallic: 0.0, tangents: true, specular_tint: Color::WHITE, parallax: baseline.clone(), scripted_tint: None });
        let mut overlay = Parallax::height(images.add(Image::default()), 0.07);
        overlay.method = bevy::pbr::ParallaxMappingMethod::DEFAULT_RELIEF_MAPPING;
        overlay.put(&mut source.base);
        source.extension.height_overlay = true;
        let glare = glare_material(&source);
        assert_eq!(Parallax::of(&glare), baseline, "depth map, depth, method and layers as without the overlay");
        assert!(glare.unlit && matches!(glare.alpha_mode, AlphaMode::Add));
        assert!(source.base.depth_map.is_some(), "the source left alone");
    }

    #[test]
    fn the_canvas_starts_at_the_glass_corner() {
        assert_eq!(canvas_point(Vec2::ZERO), Some(Vec2::ZERO));
        // The glass's far corner (0.767, 0.755) is past the canvas's right edge.
        assert_eq!(canvas_point(Vec2::new(0.767, 0.755)), None);
        assert!(canvas_point(Vec2::new(0.7, 0.7)).is_some());
    }
    #[test]
    fn flat_terminal_capture_clears_stale_monitor_pointer_and_power_aid_without_exit() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::input::InputPlugin))
            .init_resource::<nvf_play::bindings::Bindings>()
            .init_resource::<nvf_play::terminal::touch::Settings>()
            .insert_resource(PowerAid(true))
            .insert_resource(TerminalPointer { canvas: Some(Vec2::ONE) })
            .insert_resource(TerminalScreen(Some(nvf_play::terminal::Session {
                reference: fnv_sim::FormId(1), base: fnv_sim::FormId(2),
                page: nvf_play::terminal::Page::LockedOut(["Synthetic".into(), "Fixture".into()]),
            })))
            .add_message::<TerminalInput>()
            .add_systems(Update, follow_pointer);
        app.world_mut().resource_mut::<nvf_play::terminal::touch::Settings>().enabled = true;
        app.update();
        assert!(app.world().resource::<TerminalPointer>().canvas.is_none());
        assert!(!app.world().resource::<PowerAid>().0);
        assert_eq!(app.world().resource::<Messages<TerminalInput>>().len(), 0);
    }

    #[test]
    fn spawn_monitor_missing_bounds_or_glass_leaks_zero_entities_and_images() {
        use nvf_assets::model::{ModelCtx, PreparedModel, PreparedShape};
        use nvf_assets::NifMaterial;
        let (mut app, _files) = crate::outfits::tests::asset_fixture();
        app.init_resource::<Assets<StandardMaterial>>();
        let mesh_handle = app.world_mut().resource_mut::<Assets<Mesh>>().add(Mesh::new(
            bevy::mesh::PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        ));
        let mat_handle = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial::default());
        let picture = app.world_mut().resource_mut::<Assets<Image>>().add(Image::default());
        let initial_images = app.world().resource::<Assets<Image>>().len();
        let initial_entities = app.world().entities().len();

        let model_without_bounds = PreparedModel {
            root: Affine3A::IDENTITY,
            shapes: vec![PreparedShape {
                name: Some("screen:0".into()),
                mesh: mesh_handle,
                material: mat_handle,
                transform: Affine3A::IDENTITY,
                ancestors: Vec::new(),
                node: None,
                own: Affine3A::IDENTITY,
                cpu: None,
                segments: Vec::new(),
                external_emittance: false,
                look: nvf_assets::material_anim::Look::stored([0.0; 3], 1.0, 1.0),
                missing_diffuse: None,
                block: 0,
            }],
            bounds: None,
            ..Default::default()
        };

        let mut system_state = bevy::ecs::system::SystemState::<(
            Commands,
            ModelCtx,
            ResMut<Assets<StandardMaterial>>,
        )>::new(app.world_mut());
        let (mut commands, mut models, mut plain) = system_state.get_mut(app.world_mut()).unwrap();
        // fit_model=true with bounds=None must bail out BEFORE spawning any shape or allocating any image.
        let res = spawn_monitor(
            &mut commands,
            &mut models,
            &mut plain,
            &model_without_bounds,
            &picture,
            TARGET,
            Extent3d { width: 640, height: 480, depth_or_array_layers: 1 },
            MONITOR_LAYER,
            "Dialogue terminal",
            true,
            -3,
            true,
            false,
            Vec3::ZERO,
        );
        assert!(res.is_none());
        // fit_model=false with missing cpu glass must also bail out with zero leaks.
        let res_no_glass = spawn_monitor(
            &mut commands,
            &mut models,
            &mut plain,
            &model_without_bounds,
            &picture,
            TARGET,
            Extent3d { width: 640, height: 480, depth_or_array_layers: 1 },
            MONITOR_LAYER,
            "Terminal",
            false,
            -4,
            false,
            true,
            PLACE,
        );
        assert!(res_no_glass.is_none());
        system_state.apply(app.world_mut());

        assert_eq!(app.world().entities().len(), initial_entities, "no leaked monitor entities");
        assert_eq!(app.world().resource::<Assets<Image>>().len(), initial_images, "no leaked full-display images");
    }

}
