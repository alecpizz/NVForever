use super::*;
use crate::dialogue_device::ScreenMap;
use crate::dialogue_touch::compact_tests::{layout_scene, rows, send, settle};
use std::sync::Arc;

struct Glass { window: Rect, canvas: Rect, power: Option<Rect> }
impl ScreenMap for Glass {
    fn canvas_at(&self, point: Vec2) -> Option<Vec2> {
        self.window.contains(point).then(|| self.canvas.min
            + (point - self.window.min) * self.canvas.size() / self.window.size())
    }
    fn window_at(&self, point: Vec2) -> Option<Vec2> {
        self.canvas.contains(point).then(|| self.window.min
            + (point - self.canvas.min) * self.window.size() / self.canvas.size())
    }
    fn power_button_at(&self, point: Vec2) -> bool {
        self.power.is_some_and(|r| r.contains(point))
    }
    fn power_button_rect(&self) -> Option<Rect> { self.power }
}

#[test]
fn wrist_terminal_uses_published_glass_for_actual_painted_choice_taps_without_an_atlas() {
    let (mut app, window) = layout_scene(true, Vec2::new(792.0, 363.0), 1.0);
    app.init_resource::<Assets<Image>>().init_resource::<crate::pad::PadGlyphs>();
    app.world_mut().resource_mut::<TouchDialogue>().confirm = false;
    let i = app.world().resource::<crate::settings::Settings>().index(appearance::THEME_KEY).unwrap();
    assert!(crate::settings::pick(app.world_mut(), i, "Wrist Terminal"));
    settle(&mut app);
    let request = app.world().resource::<Request>().clone();
    let glass = Rect::from_center_size(request.stamp.display.center(), Vec2::new(320.0, 240.0));
    let camera = app.world_mut().spawn(Camera3d::default()).id();
    let image = app.world_mut().resource_mut::<Assets<Image>>().add(Image::transparent());
    app.insert_resource(DeviceView { stamp: request.stamp, image, camera,
        scale: 240.0 / request.canvas.height(), map: Arc::new(Glass { window: glass, canvas: request.canvas, power: None }) });
    settle(&mut app);
    let world = app.world_mut();
    let mut output = world.query_filtered::<(&ImageNode, &ComputedNode), With<Display>>();
    assert!(output.single(world).unwrap().1.size().min_element() > 0.0);
    let canvas_point = rows(&mut app)[0].1.center();
    let point = app.world().resource::<DeviceView>().window_at(&request, canvas_point).unwrap();
    send(&mut app, window, 41, TouchPhase::Started, point);
    send(&mut app, window, 41, TouchPhase::Ended, point);
    app.update();
    assert_eq!(app.world().resource::<TouchFrame>().asked.pick, Some(0));
}

#[test]
fn wrist_terminal_power_button_exits_only_when_goodbye_exists_and_uses_compact_default_text() {
    let touch = TouchDialogue::default();
    assert!(touch.choice_size_for(appearance::Theme::WristTerminal) < touch.choice_size_for(appearance::Theme::Minimal));
    assert!(touch.choice_size_for(appearance::Theme::ClassicGlass) < touch.choice_size_for(appearance::Theme::Minimal));
    assert!(touch.subtitle_size_for(appearance::Theme::WristTerminal) < touch.subtitle_size_for(appearance::Theme::SplitPanel));
    assert!(touch.subtitle_size_for(appearance::Theme::ClassicGlass) < touch.subtitle_size_for(appearance::Theme::SplitPanel));

    let (mut app, window) = layout_scene(true, Vec2::new(792.0, 363.0), 1.0);
    app.init_resource::<Assets<Image>>().init_resource::<crate::pad::PadGlyphs>();
    let i = app.world().resource::<crate::settings::Settings>().index(appearance::THEME_KEY).unwrap();
    assert!(crate::settings::pick(app.world_mut(), i, "Wrist Terminal"));
    settle(&mut app);
    let request = app.world().resource::<Request>().clone();
    let glass = Rect::from_center_size(request.stamp.display.center(), Vec2::new(320.0, 240.0));
    let power = Rect::from_corners(Vec2::new(glass.max.x + 8.0, glass.max.y - 48.0), Vec2::new(glass.max.x + 56.0, glass.max.y));
    let camera = app.world_mut().spawn(Camera3d::default()).id();
    let image = app.world_mut().resource_mut::<Assets<Image>>().add(Image::transparent());
    app.insert_resource(DeviceView { stamp: request.stamp, image, camera,
        scale: 240.0 / request.canvas.height(), map: Arc::new(Glass { window: glass, canvas: request.canvas, power: Some(power) }) });
    // Forced dialogue (goodbye == false): power button must NOT trigger exit.
    app.world_mut().resource_mut::<DialogueMenu>().view.as_mut().unwrap().goodbye = false;
    settle(&mut app);
    let p = power.center();
    send(&mut app, window, 51, TouchPhase::Started, p);
    send(&mut app, window, 51, TouchPhase::Ended, p);
    app.update();
    assert!(!app.world().resource::<TouchFrame>().asked.leave, "power button refused during forced dialogue");
    // Normal dialogue with goodbye option: power button requests clean leave.
    app.world_mut().resource_mut::<DialogueMenu>().view.as_mut().unwrap().goodbye = true;
    settle(&mut app);
    send(&mut app, window, 52, TouchPhase::Started, p);
    send(&mut app, window, 52, TouchPhase::Ended, p);
    app.update();
    assert!(app.world().resource::<TouchFrame>().asked.leave, "power button quick-exits when goodbye exists");
}

#[test]
fn console_and_wrist_terminal_power_button_exits_at_high_dpi_even_when_corner_overlaps_glass() {
    for (theme_name, density) in [("Wrist Terminal", 3.0_f32), ("Retro Console", 3.0_f32)] {
        let (mut app, window) = layout_scene(true, Vec2::new(792.0, 363.0), density);
        app.init_resource::<Assets<Image>>().init_resource::<crate::pad::PadGlyphs>();
        let i = app.world().resource::<crate::settings::Settings>().index(appearance::THEME_KEY).unwrap();
        assert!(crate::settings::pick(app.world_mut(), i, theme_name));
        settle(&mut app);
        let request = app.world().resource::<Request>().clone();
        // Enlarged glass fills almost the entire display; power button sits in bottom-right corner.
        let glass = request.stamp.display.inflate(-4.0);
        let btn = 56.0 * density;
        let power = Rect::from_corners(
            Vec2::new(request.stamp.display.max.x - 8.0 * density - btn, request.stamp.display.max.y - 8.0 * density - btn),
            Vec2::new(request.stamp.display.max.x - 8.0 * density, request.stamp.display.max.y - 8.0 * density),
        );
        let camera = app.world_mut().spawn(Camera3d::default()).id();
        let image = app.world_mut().resource_mut::<Assets<Image>>().add(Image::transparent());
        app.insert_resource(DeviceView {
            stamp: request.stamp,
            image,
            camera,
            scale: (glass.width() / request.canvas.width()).min(glass.height() / request.canvas.height()),
            map: Arc::new(Glass { window: glass, canvas: request.canvas, power: Some(power) }),
        });
        // Forced dialogue: tapping power button must NOT exit.
        app.world_mut().resource_mut::<DialogueMenu>().view.as_mut().unwrap().goodbye = false;
        settle(&mut app);
        let logical_tap = power.center() / density;
        send(&mut app, window, 61, TouchPhase::Started, logical_tap);
        send(&mut app, window, 61, TouchPhase::Ended, logical_tap);
        app.update();
        assert!(!app.world().resource::<TouchFrame>().asked.leave, "{theme_name}: forced dialogue stays open");

        // Normal dialogue with goodbye: tapping rendered power button (with small thumb jitter) exits.
        app.world_mut().resource_mut::<DialogueMenu>().view.as_mut().unwrap().goodbye = true;
        settle(&mut app);
        // Power button hit target is invisible: Display must spawn no visible ring/glyph children.
        {
            let world = app.world_mut();
            let mut displays = world.query_filtered::<Option<&Children>, With<Display>>();
            assert!(
                displays.single(world).unwrap().is_none_or(|c| c.is_empty()),
                "{theme_name}: power button overlay must stay invisible (no ring/glyph children on Display)"
            );
            let mut contents = world.query_filtered::<&BackgroundColor, With<Content>>();
            let bg = contents.single(world).unwrap().0.to_srgba();
            assert!((bg.alpha - 1.0).abs() < 1e-4, "{theme_name}: CRT backdrop must be fully opaque");
            assert!(bg.red < 0.12 && bg.green < 0.12 && bg.blue < 0.12, "{theme_name}: CRT backdrop must be dark");
        }
        send(&mut app, window, 62, TouchPhase::Started, logical_tap);
        send(&mut app, window, 62, TouchPhase::Moved, logical_tap + Vec2::new(5.0, -4.0));
        send(&mut app, window, 62, TouchPhase::Ended, logical_tap + Vec2::new(5.0, -4.0));
        app.update();
        assert!(app.world().resource::<TouchFrame>().asked.leave, "{theme_name}: tapping rendered power button exits dialogue");
    }
}

#[test]
fn advancing_dialogue_lines_keeps_device_request_generation_stable() {
    let (mut app, _window) = layout_scene(true, Vec2::new(792.0, 363.0), 1.0);
    app.init_resource::<Assets<Image>>().init_resource::<crate::pad::PadGlyphs>();
    let i = app.world().resource::<crate::settings::Settings>().index(appearance::THEME_KEY).unwrap();
    assert!(crate::settings::pick(app.world_mut(), i, "Wrist Terminal"));
    settle(&mut app);
    let initial = app.world().resource::<Request>().clone();
    // Transition from choices to spoken goodbye line (same as live-device choices -> "Take it easy now.").
    {
        let mut menu = app.world_mut().resource_mut::<DialogueMenu>();
        menu.list = None;
        let view = menu.view.as_mut().unwrap();
        view.choices.clear();
        view.line = Some("Take it easy now.".into());
    }
    settle(&mut app);
    let after = app.world().resource::<Request>().clone();
    assert_eq!(
        after.stamp, initial.stamp,
        "advancing dialogue lines must not bump Request generation or tear down the 3D Pip-Boy scene"
    );
    assert_eq!(after.camera, initial.camera);
    assert_eq!(after.image, initial.image);
}
