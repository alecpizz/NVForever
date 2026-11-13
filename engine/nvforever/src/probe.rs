//! The remaster's local light probe (docs/design/graphics-remaster.md §2 "Light probe"):
//! six small cameras at the main camera's position, one per cube face, render the world
//! around it, off the screen too, one face every [`EVERY`] frames. gi.rs copies the faces into a 2D
//! array each frame and (1) lights surfaces by the probe's light from each direction
//! (the Strip's signs behind the camera still light the street) and (2) gives the faces
//! to the lighting as the reflection environment of environment-mapped shapes
//! (nif_material.wesl `env_reflection`). An engineering choice, not the original game's.
//!
//! Only while the main camera carries [`crate::gi::GlobalIllumination`].

use crate::fly::FlyCam;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::RenderTarget;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::pbr::DistanceFog;
use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::asset::RenderAssetUsages;
use bevy::transform::TransformSystems;

/// A face is drawn every this many frames (the whole probe every 6 x this).
const EVERY: usize = 4;

/// The side of a face, in texels.
pub const FACE_SIZE: u32 = 128;

/// The faces' looking direction and up, in the order of the array's layers (gi.wesl and
/// nif_material.wesl `probe_uv` use the same table).
pub const FACES: [(Vec3, Vec3); 6] = [
    (Vec3::X, Vec3::Y),
    (Vec3::NEG_X, Vec3::Y),
    (Vec3::Y, Vec3::NEG_Z),
    (Vec3::NEG_Y, Vec3::Z),
    (Vec3::Z, Vec3::Y),
    (Vec3::NEG_Z, Vec3::Y),
];

/// The faces' images (HDR, linear, after exposure).
#[derive(Resource, ExtractResource, Clone)]
#[extract_app(bevy::render::RenderApp)]
pub struct ProbeImages {
    pub faces: [Handle<Image>; 6],
}

#[derive(Component)]
struct ProbeFace(usize);

pub struct ProbePlugin;

impl Plugin for ProbePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ExtractResourcePlugin::<ProbeImages>::default())
            .add_systems(Startup, setup)
            .add_systems(
                PostUpdate,
                follow
                    .after(TransformSystems::Propagate)
                    .before(bevy::camera::visibility::VisibilitySystems::UpdateFrusta)
                    .before(bevy::light::SimulationLightSystems::UpdateDirectionalLightCascades),
            );
    }
}

fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let faces = std::array::from_fn(|i| {
        let mut image = Image::new_uninit(
            Extent3d { width: FACE_SIZE, height: FACE_SIZE, depth_or_array_layers: 1 },
            TextureDimension::D2,
            TextureFormat::Rgba16Float,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        );
        image.texture_descriptor.usage |= TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_SRC | TextureUsages::RENDER_ATTACHMENT;
        let image = images.add(image);
        commands.spawn((
            Name::new(format!("Light probe face {i}")),
            ProbeFace(i),
            Camera3d::default(),
            Camera { order: -2, is_active: false, ..default() },
            RenderTarget::Image(image.clone().into()),
            bevy::camera::Hdr,
            Tonemapping::Linear,
            Msaa::Off,
            Projection::Perspective(PerspectiveProjection { fov: std::f32::consts::FRAC_PI_2, aspect_ratio: 1.0, near: 8.0, ..default() }),
            crate::sky::initial_fog(),
            RenderLayers::from_layers(&[0]),
            Transform::default(),
        ));
        image
    });
    commands.insert_resource(ProbeImages { faces });
}

/// The main camera, for [`follow`].
type MainCamera<'w, 's> = Query<'w, 's, (&'static GlobalTransform, &'static DistanceFog, Has<crate::gi::GlobalIllumination>), (With<FlyCam>, Without<ProbeFace>)>;

/// One face every [`EVERY`] frames, at the main camera, while it has global illumination; the fog as the
/// main camera's.
fn follow(
    main: MainCamera,
    mut faces: Query<(&ProbeFace, &mut Camera, &mut Transform, &mut GlobalTransform, &mut DistanceFog), Without<FlyCam>>,
    mut frame: Local<usize>,
) {
    let Ok((at, fog, gi)) = main.single() else {
        for (_, mut cam, ..) in &mut faces {
            cam.is_active = false;
        }
        return;
    };
    *frame = frame.wrapping_add(1);
    let pos = at.translation();
    for (face, mut cam, mut t, mut gt, mut f) in &mut faces {
        // One face every EVERY-th frame: each probe view costs a view's culling, queueing
        // and its own sun cascades on the render thread (about 2 ms a view at Goodsprings).
        let on = gi && frame.is_multiple_of(EVERY) && face.0 == (*frame / EVERY) % 6;
        if cam.is_active != on {
            cam.is_active = on;
        }
        if on {
            let (dir, up) = FACES[face.0];
            *t = Transform::from_translation(pos).looking_to(dir, up);
            *gt = GlobalTransform::from(*t);
            *f = fog.clone();
        }
    }
}

/// The height field's side, in texels, and the square it covers (game units): about 23
/// units a texel (graphics-remaster.md §2 "Height field").
pub const HEIGHT_SIZE: u32 = 512;
pub const HEIGHT_SPAN: f32 = 12_000.0;
/// The top-down camera stands this far above the player and sees this far down.
const HEIGHT_ABOVE: f32 = 20_000.0;
pub const HEIGHT_FAR: f32 = 60_000.0;
/// The height field is drawn every this many frames, never on a probe frame.
const HEIGHT_EVERY: usize = 8;

/// The top-down camera of the height field (main world; extracted so gi.rs finds its
/// depth prepass).
#[derive(Component, bevy::render::extract_component::ExtractComponent, Clone, Copy, Default)]
#[extract_app(bevy::render::RenderApp)]
pub struct HeightCamera;

/// The height field: the world seen from straight above the player, outdoors. `image`
/// holds its lit colour; gi.rs turns the camera's depth into heights the frame it is drawn
/// (`fresh`). `origin`: x, z the square's centre, y the camera's height; w 1 once drawn.
#[derive(Resource, ExtractResource, Clone)]
#[extract_app(bevy::render::RenderApp)]
pub struct HeightField {
    pub image: Handle<Image>,
    pub origin: Vec4,
    pub fresh: bool,
}

pub struct HeightFieldPlugin;

impl Plugin for HeightFieldPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            ExtractResourcePlugin::<HeightField>::default(),
            bevy::render::extract_component::ExtractComponentPlugin::<HeightCamera>::default(),
        ))
        .add_systems(Startup, setup_height)
        .add_systems(
            PostUpdate,
            follow_height
                .after(TransformSystems::Propagate)
                .before(bevy::camera::visibility::VisibilitySystems::UpdateFrusta)
                .before(bevy::light::SimulationLightSystems::UpdateDirectionalLightCascades),
        );
    }
}

fn setup_height(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let mut image = Image::new_uninit(
        Extent3d { width: HEIGHT_SIZE, height: HEIGHT_SIZE, depth_or_array_layers: 1 },
        TextureDimension::D2,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage |= TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT;
    let image = images.add(image);
    commands.spawn((
        Name::new("Height field camera"),
        HeightCamera,
        Camera3d::default(),
        Camera { order: -3, is_active: false, ..default() },
        RenderTarget::Image(image.clone().into()),
        bevy::camera::Hdr,
        Tonemapping::Linear,
        Msaa::Off,
        bevy::core_pipeline::prepass::DepthPrepass,
        Projection::Orthographic(OrthographicProjection {
            near: 0.0,
            far: HEIGHT_FAR,
            scaling_mode: bevy::camera::ScalingMode::Fixed { width: HEIGHT_SPAN, height: HEIGHT_SPAN },
            ..OrthographicProjection::default_3d()
        }),
        RenderLayers::from_layers(&[0]),
        Transform::default(),
    ));
    commands.insert_resource(HeightField { image, origin: Vec4::ZERO, fresh: false });
}

/// Every [`HEIGHT_EVERY`] frames, outdoors with global illumination: the camera over the
/// player, its centre held to the texel grid so the field does not shimmer as he walks.
#[allow(clippy::type_complexity)]
fn follow_height(
    sky: Option<Res<crate::sky::SkyState>>,
    main: Query<(&GlobalTransform, Has<crate::gi::GlobalIllumination>), (With<FlyCam>, Without<HeightCamera>)>,
    mut cam: Query<(&mut Camera, &mut Transform, &mut GlobalTransform), (With<HeightCamera>, Without<FlyCam>)>,
    mut field: ResMut<HeightField>,
    mut frame: Local<usize>,
) {
    *frame = frame.wrapping_add(1);
    let indoors = sky.is_some_and(|s| s.interior.is_some());
    let Ok((mut camera, mut t, mut gt)) = cam.single_mut() else { return };
    let at = main.single().ok().filter(|(_, gi)| *gi && !indoors).map(|(g, _)| g.translation());
    // Never on the probe's frames (every 4th): one extra view a frame at most.
    let on = at.is_some() && *frame % HEIGHT_EVERY == 2;
    if camera.is_active != on {
        camera.is_active = on;
    }
    if field.fresh != on {
        field.fresh = on;
    }
    if !on {
        if at.is_none() && field.origin.w != 0.0 {
            field.origin.w = 0.0;
        }
        return;
    }
    let Some(p) = at else { return };
    let texel = HEIGHT_SPAN / HEIGHT_SIZE as f32;
    let c = Vec2::new((p.x / texel).round() * texel, (p.z / texel).round() * texel);
    let y = p.y + HEIGHT_ABOVE;
    *t = Transform::from_xyz(c.x, y, c.y).looking_to(Vec3::NEG_Y, Vec3::NEG_Z);
    *gt = GlobalTransform::from(*t);
    field.origin = Vec4::new(c.x, y, c.y, 1.0);
}
