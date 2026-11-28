//! Authored speech and choices fed through the production painters and their measured targets.
use super::*;
use crate::dialogue_device::{DeviceKind, DeviceView, Request};
use crate::dialogue_menu::{ChoiceRow, MenuView};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    Choices,
    Speech,
    Last,
}

#[derive(Resource)]
struct Fonts {
    theme: String,
    reference: Vec<Handle<Image>>,
    classic_face: Option<crate::interface::Face>,
    matches: std::collections::HashMap<AssetId<Image>, bool>,
}

fn load_fonts(
    mut fonts: ResMut<Fonts>,
    data: Res<nvf_assets::GameData>,
    mut images: ResMut<Assets<Image>>,
    formats: Option<Res<bevy::image::CompressedImageFormatSupport>>,
) {
    let bc = crate::interface::bc_supported(formats.as_deref());
    let faces = match fonts.theme.as_str() {
        "Normal" => vec![crate::dialogue_menu::load_kit(Some(&data), &mut images, bc).body],
        "Retro Console" | "Classic" => {
            let faces = crate::terminal::screen::load_faces(Some(&data), &mut images, bc);
            vec![faces.text, faces.lit]
        }
        "Wrist Terminal" => vec![crate::interface::face(
            crate::pipboy::frame::FONTS[1],
            crate::interface::plain_font(31.0, 13.0),
            Some(&data),
            &mut images,
            bc,
        )],
        _ => return,
    };
    if fonts.theme == "Classic" {
        fonts.classic_face = Some(faces[0].clone());
    }
    fonts.reference = faces
        .into_iter()
        .map(|face| {
            face.image
                .expect("original dialogue font bitmap required")
                .0
        })
        .collect();
}

fn identify_fonts(mut fonts: ResMut<Fonts>, images: Res<Assets<Image>>, nodes: Query<&ImageNode>) {
    for node in &nodes {
        if node.rect.is_none() || fonts.matches.contains_key(&node.image.id()) {
            continue;
        }
        let Some(image) = images.get(&node.image) else {
            continue;
        };
        // Separately loaded atlas handles may differ. Compare the actual texture
        // payload once per observed handle, including softened terminal variants.
        let matched = fonts
            .reference
            .iter()
            .filter_map(|h| images.get(h))
            .any(|reference| {
                reference.texture_descriptor.size == image.texture_descriptor.size
                    && reference.texture_descriptor.format == image.texture_descriptor.format
                    && reference.data.is_some()
                    && reference.data == image.data
            });
        fonts.matches.insert(node.image.id(), matched);
    }
}

#[derive(Resource)]
struct ClassicScene(Scene);

fn sync_classic(
    script: Res<crate::script::ScriptState>,
    sim: Res<crate::sim::SimState>,
    scene: Res<ClassicScene>,
    mut menu: ResMut<DialogueMenu>,
) {
    let game = &script.rt.as_ref().expect("capture runtime").game;
    let view = crate::dialogue::view(game, &sim).expect("selected NPC must remain in conversation");
    let list = crate::dialogue::list_id(game);
    let selected = if scene.0 == Scene::Last {
        view.choices.len().saturating_sub(1)
    } else {
        0
    };
    if menu.view.as_ref() != Some(&view) {
        menu.view = Some(view);
    }
    if menu.list != list {
        menu.list = list;
    }
    if menu.selected != selected {
        menu.selected = selected;
    }
}

// Advance at most one spoken response per frame. The ordinary state consumer
// first sees that exact utterance. A choice frame cannot qualify during speech.
fn advance_classic(
    scene: Res<ClassicScene>,
    menu: Res<DialogueMenu>,
    mut script: ResMut<crate::script::ScriptState>,
    mut sim: ResMut<crate::sim::SimState>,
    state: Res<super::classic_state::State>,
) {
    if scene.0 == Scene::Speech {
        return;
    }
    let Some(line) = menu.view.as_ref().and_then(|v| v.line.as_deref()) else {
        return;
    };
    if state.speaking && state.spoken.as_deref() == Some(line) {
        assert!(
            crate::script::with_world(script.rt.as_mut().unwrap(), &mut sim, |rt| rt
                .game
                .skip_response()),
            "could not advance the displayed response"
        );
    }
}

pub fn install_scene(app: &mut App, theme: &str, scene: Scene) {
    app.init_resource::<crate::pad::PadGlyphs>()
        .insert_resource(Fonts {
            theme: theme.into(),
            reference: Vec::new(),
            classic_face: None,
            matches: Default::default(),
        })
        .add_systems(Startup, load_fonts)
        .add_systems(PostUpdate, identify_fonts);
    app.insert_state(crate::PlayState::Dialogue)
        .init_resource::<crate::bindings::Bindings>()
        .init_resource::<DialogueMenu>();
    install(app);
    app.insert_resource(TouchLayout::for_platform(true));
    app.insert_resource(TouchDialogue { enabled: true, ..TouchDialogue::for_platform(true) });
    app.world_mut().resource_mut::<crate::dialogue_split::Options>().side =
        crate::dialogue_split::Side::Standard;
    let settings = app.world().resource::<crate::settings::Settings>();
    let row = settings.index(appearance::THEME_KEY).expect("installed theme row");
    let select = settings.choices(row).expect("production theme choices").pick;
    assert!(select(app.world_mut(), theme), "unknown capture theme");
    if theme == "Classic" {
        app.insert_resource(ClassicScene(scene))
            .add_systems(PreUpdate, sync_classic)
            .add_systems(Last, advance_classic);
        return;
    }
    let list = app
        .world()
        .get_resource::<crate::script::ScriptState>()
        .and_then(|s| s.rt.as_ref())
        .and_then(|rt| crate::dialogue::list_id(&rt.game));
    let mut menu = app.world_mut().resource_mut::<DialogueMenu>();
    let mut choices: Vec<_> = [
        "What's beyond the relay?",
        "[Science 50] Fix the relay.",
        "I've heard that before.",
        "Show me your supplies.",
        "Who runs this station?",
        "Where is the checkpoint?",
        "Do you need any help?",
        "I'll return tomorrow.",
        "Goodbye.",
    ]
    .into_iter()
    .enumerate()
    .map(|(index, label)| ChoiceRow {
        label: label.into(),
        short: index == 1,
        darkened: index == 2,
    })
    .collect();
    let line = if scene == Scene::Speech {
        choices.clear();
        Some("The relay is quiet today.".into())
    } else {
        None
    };
    menu.list = list;
    menu.selected = if scene == Scene::Last {
        choices.len() - 1
    } else {
        0
    };
    menu.view = Some(MenuView {
        speaker: "Station keeper".into(),
        line,
        choices,
        ..default()
    });
}

fn round_trip(request: &Request, view: &DeviceView, point: Vec2) -> Option<Vec2> {
    let physical = view.window_at(request, point)?;
    let back = view.canvas_at(request, physical)?;
    (back.distance(point) <= 1.0).then_some(physical)
}

/// Match the ordered atlas cuts of a production line to characters from the
/// actual face, on the designated camera and inside the appropriate aperture.
fn painted(
    world: &World,
    anchors: &mut QueryState<&Children>,
    text: &str,
    area: Rect,
    camera: Entity,
) -> bool {
    let fonts = world.resource::<Fonts>();
    let Some(face) = &fonts.classic_face else {
        return false;
    };
    let Some((_, size)) = &face.image else {
        return false;
    };
    let mut expected = Vec::new();
    for line in face.font.lines(text, None) {
        for placed in line.glyphs {
            let g = face.font.glyph(placed.code);
            if g.drawn() {
                expected.push(Rect::new(
                    g.uv[0] * size.x,
                    g.uv[1] * size.y,
                    g.uv[2] * size.x,
                    g.uv[3] * size.y,
                ));
            }
        }
    }
    if expected.is_empty() {
        return false;
    }
    // A label owns its wrapped line anchors in reading order. Compare every
    // native glyph, requiring the full glyph box to survive both clip regions.
    anchors.iter(world).any(|lines| {
        let mut observed = Vec::new();
        for &line in lines {
            let Some(children) = world.get::<Children>(line) else {
                continue;
            };
            for &child in children {
                let Some(glyph) = world.get::<ImageNode>(child) else {
                    continue;
                };
                if fonts.matches.get(&glyph.image.id()) != Some(&true) {
                    continue;
                }
                let Some(rect) = glyph.rect else {
                    return false;
                };
                let Some(node) = world.get::<ComputedNode>(child) else {
                    return false;
                };
                let Some(t) = world.get::<UiGlobalTransform>(child) else {
                    return false;
                };
                let Some(c) = world.get::<ComputedUiTargetCamera>(child) else {
                    return false;
                };
                if c.get() != Some(camera)
                    || node.is_empty()
                    || world
                        .get::<InheritedVisibility>(child)
                        .is_some_and(|v| !v.get())
                    || !fully_visible(node, t, world.get::<CalculatedClip>(child), area)
                {
                    return false;
                }
                observed.push(rect);
            }
        }
        observed == expected
    })
}

fn fully_visible(
    node: &ComputedNode,
    t: &UiGlobalTransform,
    clip: Option<&CalculatedClip>,
    area: Rect,
) -> bool {
    let half = (node.size() * 0.5 - Vec2::splat(0.5)).max(Vec2::ZERO);
    [-half.x, half.x].into_iter().all(|x| {
        [-half.y, half.y].into_iter().all(|y| {
            let p = t.transform_point2(Vec2::new(x, y));
            area.contains(p) && clip.is_none_or(|c| c.contains_point(p))
        })
    })
}

/// Last runs after layout, camera projection and the renderer's stamped publication.
pub fn ready(world: &mut World, theme: &str, scene: Scene, density: f32) -> bool {
    let mut line_anchors = world.query::<&Children>();
    let mut actions = world.query::<(
        &Action,
        &ComputedNode,
        &UiGlobalTransform,
        Option<&InheritedVisibility>,
        Option<&CalculatedClip>,
        &ComputedUiTargetCamera,
    )>();
    let mut pictures = world.query::<(
        &ImageNode,
        &ComputedNode,
        &UiGlobalTransform,
        Option<&CalculatedClip>,
        &ComputedUiTargetCamera,
        Option<&InheritedVisibility>,
    )>();
    let mut text = world.query::<(
        &Text,
        &bevy::text::TextLayoutInfo,
        &ComputedNode,
        &UiGlobalTransform,
        Option<&CalculatedClip>,
        &ComputedUiTargetCamera,
        Option<&InheritedVisibility>,
    )>();
    let mut viewports =
        world.query_filtered::<(&ComputedNode, &UiGlobalTransform), With<Viewport>>();
    let expected = match theme {
        "Retro Console" => Some(DeviceKind::Terminal),
        "Wrist Terminal" => Some(DeviceKind::PipBoy),
        _ => None,
    };
    let device = match (
        expected,
        world.get_resource::<Request>(),
        world.get_resource::<DeviceView>(),
    ) {
        (Some(kind), Some(r), Some(v))
            if r.stamp.kind == kind
                && v.matches(r)
                && r.stamp.density == density
                && world.get::<Camera3d>(v.camera).is_some()
                && world.get::<Camera>(v.camera).is_some_and(|c| c.is_active)
                && world.resource::<Assets<Image>>().contains(v.image.id())
                && v.image != r.image =>
        {
            Some((r, v))
        }
        (None, None, None) => None,
        _ => return false,
    };
    if let Some((_, v)) = device {
        if !pictures.iter(world).any(|(p, n, _, _, _, visible)| {
            p.image == v.image && !n.is_empty() && visible.is_none_or(|v| v.get())
        }) {
            return false;
        }
    }
    let target = match scene {
        Scene::Choices => Kind::Choice(0),
        Scene::Last => {
            let Some(index) = world
                .resource::<DialogueMenu>()
                .view
                .as_ref()
                .and_then(|v| v.choices.len().checked_sub(1))
            else {
                return false;
            };
            Kind::Choice(index)
        }
        Scene::Speech => Kind::Skip,
    };
    let classic = world
        .get_resource::<super::classic_presenter::Surface>()
        .and_then(|s| s.0.as_ref())
        .filter(|_| theme == "Classic");
    let on_window = |r: Rect| {
        let origin = classic.map_or(Vec2::ZERO, |s| s.origin);
        Rect::from_corners((r.min + origin) * density, (r.max + origin) * density)
    };
    let Some((_, node, transform, vis, clip, camera)) =
        actions.iter(world).find(|(a, _, t, ..)| {
            a.kind == target
                && classic.is_none_or(|s| {
                    on_window(s.layout.responses).contains(t.transform_point2(Vec2::ZERO))
                })
        })
    else {
        return false;
    };
    if device.is_some_and(|(r, _)| camera.get() != Some(r.camera)) {
        return false;
    }
    let center = transform.transform_point2(Vec2::ZERO);
    let half = node.size() * 0.5;
    if half.min_element() <= 1.0 {
        return false;
    }
    // Check both ends and both sides of the actual measured button after scrolling.
    let inset = (half - Vec2::splat(0.51)).max(Vec2::ZERO);
    let physical = |point| match device {
        Some((r, v)) => round_trip(r, v, point),
        None => Some(point),
    };
    for x in [-inset.x, inset.x] {
        for y in [-inset.y, inset.y] {
            let p = center + Vec2::new(x, y);
            if !hit(node, *transform, vis, clip, p) || physical(p).is_none() {
                return false;
            }
        }
    }
    if scene != Scene::Speech
        && !viewports.iter(world).any(|(n, t)| {
            [-inset.y, inset.y]
                .into_iter()
                .all(|y| n.contains_point(*t, center + Vec2::Y * y))
        })
    {
        return false;
    }
    for extent in [Vec2::X * inset.x, Vec2::Y * inset.y] {
        let Some(top) = physical(center - extent) else {
            return false;
        };
        let Some(bottom) = physical(center + extent) else {
            return false;
        };
        if (bottom.distance(top) + 1.1) / density < 48.0 {
            return false;
        }
    }
    if theme == "Field Notes" {
        return text.iter(world).any(|(s, l, n, t, clip, c, vis)| {
            let p = t.transform_point2(Vec2::ZERO);
            !s.0.is_empty()
                && !l.glyphs.is_empty()
                && !n.is_empty()
                && node.contains_point(*transform, p)
                && clip.is_none_or(|v| v.contains_point(p))
                && camera.get() == c.get()
                && vis.is_none_or(|v| v.get())
        });
    }
    if theme == "Classic" {
        if !world
            .get_resource::<super::classic::Cache>()
            .is_some_and(|c| c.loaded().is_some())
        {
            return false;
        }
        let state = world.resource::<super::classic_state::State>();
        if state.spoken.as_deref().is_none_or(|s| s.trim().is_empty())
            || state.caps.is_none()
            || state.speaking != (scene == Scene::Speech)
            || (scene != Scene::Speech && state.barter.is_none())
        {
            return false;
        }
        let frame = world.resource::<crate::dialogue_split::Frame>();
        let Some(ui_camera) = frame.ui_camera else {
            return false;
        };
        let Some(surface) = classic else { return false };
        if !painted(
            world,
            &mut line_anchors,
            &state.caps.unwrap().to_string(),
            on_window(surface.layout.caps),
            ui_camera,
        ) || !painted(
            world,
            &mut line_anchors,
            state.spoken.as_deref().unwrap(),
            on_window(surface.layout.speech),
            ui_camera,
        ) {
            return false;
        }
        if let Some(barter) = &state.barter {
            if !actions.iter(world).any(|(a, n, t, v, clip, c)| {
                let center = t.transform_point2(Vec2::ZERO);
                a.kind == Kind::Barter
                    && a.list.as_ref() == Some(&barter.list)
                    && c.get() == Some(ui_camera)
                    && on_window(surface.layout.trade).contains(center)
                    && n.size().min_element() + 0.5 >= 48.0 * density
                    && hit(n, *t, v, clip, center)
            }) {
                return false;
            }
        }
        let Some(viewport) = frame.classic else {
            return false;
        };
        let Some(world_cam) = world.get::<Camera>(viewport.camera) else {
            return false;
        };
        let Some(actual) = world_cam.viewport.as_ref() else {
            return false;
        };
        let Some(transform) = world.get::<GlobalTransform>(viewport.camera) else {
            return false;
        };
        let Some((_, head)) = world
            .get_resource::<crate::dialogue_look::SpeakerFace>()
            .and_then(|f| f.0)
        else {
            return false;
        };
        if !world_cam.is_active
            || actual.physical_position != viewport.physical_position
            || actual.physical_size != viewport.physical_size
            || camera.get() != Some(ui_camera)
            || !world_cam
                .world_to_ndc(transform, crate::space::to_bevy(head))
                .is_some_and(|ndc| {
                    ndc.truncate().abs().max_element() < 1.0 && (0.0..=1.0).contains(&ndc.z)
                })
        {
            return false;
        }
        let atlas = surface.casing;
        if !world
            .resource::<Assets<Image>>()
            .get(atlas)
            .is_some_and(|i| i.size() == surface.layout.template.size())
        {
            return false;
        }
        // Every banded crop and its safe-area bleed is painted once, at its place,
        // and the casing reaches every window edge.
        let parts: Vec<_> = surface.layout.parts().collect();
        let mut seen = vec![false; parts.len()];
        let window_rect = on_window(Rect::from_corners(-surface.origin, surface.size - surface.origin));
        for (picture, node, t, clip, camera, visible) in pictures
            .iter(world)
            .filter(|(p, _, _, _, c, _)| p.image.id() == atlas && c.get() == Some(ui_camera))
        {
            let a = t.transform_point2(-node.size() * 0.5);
            let b = t.transform_point2(node.size() * 0.5);
            let Some(index) = parts.iter().enumerate().position(|(i, part)| {
                let expected = on_window(part.destination);
                !seen[i] && Some(part.source) == picture.rect
                    && a.abs_diff_eq(expected.min, 1.0) && b.abs_diff_eq(expected.max, 1.0)
            }) else {
                return false;
            };
            if visible.is_some_and(|v| !v.get())
                || camera.get() != Some(ui_camera)
                || node.is_empty()
                || picture.color.alpha() != 1.0
                || !fully_visible(node, t, clip, window_rect)
            {
                return false;
            }
            seen[index] = true;
        }
        if seen.iter().any(|&found| !found) {
            return false;
        }
    }
    // Bitmap glyphs use separate atlas crops; a panel or the final scene image
    // alone cannot qualify the selected line. Native speech overlays its line.
    let mut crops = Vec::new();
    for (image, n, t, clip, c, visible) in pictures.iter(world) {
        let point = t.transform_point2(Vec2::ZERO);
        if c.get() != camera.get()
            || n.is_empty()
            || visible.is_some_and(|v| !v.get())
            || !node.contains_point(*transform, point)
            || clip.is_some_and(|c| !c.contains_point(point))
        {
            continue;
        }
        if let Some(rect) = image.rect {
            if world.resource::<Fonts>().matches.get(&image.image.id()) == Some(&true)
                && !crops.contains(&(image.image.id(), rect))
            {
                crops.push((image.image.id(), rect));
                if crops.len() >= 4 {
                    return true;
                }
            }
        }
    }
    false
}
