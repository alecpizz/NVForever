//! Who may have a volume and when, without a GPU: the plan and the admission as tables, and what the
//! extraction does, without a request and with one, on wgpu's no-op device.

use super::admit::{admit, counts, view_key, Asked, Limits, MESH, MOST_BYTES};
use super::gpu::RtgiGpu;
use super::plan::{plan, Admission, Key, Latch, Now, Pipelines, Plan, Why};
use super::tests::{extracted, noop};
use super::*;
use crate::sky::cartographer_live_tests::test_streamer;
use crate::sun_visibility::{view_counts, StageCounts};
use crate::world::Streamer;
use bevy::app::SubApp;
use bevy::camera::{ComputedCameraValues, RenderTargetInfo};
use bevy::pbr::{check_views_need_specialization, init_mesh_pipeline_view_layouts, MeshPipelineKey, MeshPipelineViewLayoutKey, MeshPipelineViewLayouts, NvfRtgiVolumeBindings, ViewKeyCache};
use bevy::render::camera::DirtySpecializations;
use bevy::render::render_resource::CachedPipelineState;
use bevy::render::sync_world::{MainEntity, RenderEntity};
use bevy::render::view::RetainedViewEntity;
use bevy::render::{ExtractSchedule, MainWorld, RenderStartup};
use bevy::shader::ShaderCacheError;
use nvf_play::loading::Loading;

const HERE: Place = Place { cell: 0x0010_4C1A, travels: 3, game: Some(7) };

fn key(place: Place, revision: u64, solved: bool) -> Key {
    Key { place, revision, solved }
}

// ---------------------------------------------------------------------------------------
// The plan.

/// All there is to a request, one thing changed at a time.
#[test]
fn the_plan() {
    let now = Now { here: Some(HERE), hook: HookState::Installed, admission: Admission::Granted, open: true, views: true, ended: None };
    let shut = Now { open: false, ..now };
    let (empty, solved) = (key(HERE, 1, false), key(HERE, 1, true));
    let stock = Plan::Stock;
    let elsewhere = [Place { cell: 1, ..HERE }, Place { travels: 2, ..HERE }, Place { game: None, ..HERE }, Place { game: Some(6), ..HERE }];

    // The first binding, under the screen and with a view alone; the rest of a visit at any time.
    for want in [empty, solved] {
        assert_eq!(plan(Some(want), &now, None), Plan::Bind { new: true });
        assert_eq!(plan(Some(want), &shut, None), stock(Why::Late));
        assert_eq!(plan(Some(want), &shut, Some(want)), Plan::Bind { new: false });
        for now in [now, shut] {
            assert_eq!(plan(Some(want), &Now { views: false, ..now }, None), stock(Why::Waiting), "no view");
            assert_eq!(plan(Some(want), &Now { views: false, ..now }, Some(want)), Plan::Bind { new: false }, "its camera is being replaced");
        }
    }
    assert_eq!(plan(Some(solved), &shut, Some(empty)), Plan::Bind { new: true }, "empty to solved");
    assert_eq!(plan(Some(key(HERE, 2, true)), &shut, Some(solved)), Plan::Bind { new: true }, "the next revision");
    assert_eq!(plan(Some(solved), &shut, Some(key(HERE, 2, true))), Plan::Keep, "an older result is dropped");

    // No request, or one of another place: nothing stays, under a screen or not.
    for now in [now, shut] {
        assert_eq!(plan(None, &now, Some(solved)), stock(Why::Nothing));
        for other in elsewhere {
            assert_eq!(plan(Some(key(other, 9, true)), &now, None), stock(Why::Elsewhere), "a stale result");
            assert_eq!(plan(Some(key(other, 9, true)), &now, Some(key(other, 9, true))), stock(Why::Elsewhere), "a stale one that is bound");
        }
        assert_eq!(plan(Some(solved), &Now { here: None, ..now }, Some(solved)), stock(Why::Elsewhere), "outdoors");
    }
    // What was bound elsewhere gives no right here: so too where the game's token comes in the frame
    // the screen settles.
    for other in elsewhere {
        assert_eq!(plan(Some(solved), &shut, Some(key(other, 1, true))), stock(Why::Late));
        assert_eq!(plan(Some(solved), &now, Some(key(other, 1, true))), Plan::Bind { new: true });
    }
    // The hook, the device, the end of the visit: none is bound, and what is bound goes.
    for held in [None, Some(solved), Some(key(HERE, 2, true))] {
        for (why, now) in [
            (Why::Waiting, Now { hook: HookState::Pending, ..now }),
            (Why::Hook, Now { hook: HookState::Failed, ..now }),
            (Why::Waiting, Now { admission: Admission::Deferred, ..now }),
            (Why::Refused, Now { admission: Admission::Refused, ..now }),
            (Why::Failed, Now { ended: Some((HERE, Why::Failed)), ..now }),
            (Why::Late, Now { ended: Some((HERE, Why::Late)), ..now }),
        ] {
            assert_eq!(plan(Some(solved), &now, held), stock(why), "{held:?}");
        }
    }
    assert_eq!(plan(Some(solved), &Now { ended: Some((elsewhere[1], Why::Failed)), ..now }, None), Plan::Bind { new: true }, "another visit's end");
    let ends = [Why::Hook, Why::Failed, Why::Refused, Why::Late, Why::Nothing, Why::Elsewhere, Why::Waiting].map(Why::ends_visit);
    assert_eq!(ends, [true, true, true, true, false, false, false]);
}

/// Open from a screen's first frame until it settles, and not again under the same screen.
#[test]
fn the_latch() {
    let mut latch = Latch::default();
    let frames = [(false, false), (true, false), (true, false), (true, true), (true, false), (false, false), (true, false), (true, true), (false, false)];
    assert_eq!(frames.map(|(screen, settling)| latch.step(screen, settling)), [false, true, true, false, false, false, true, false, false]);
}

/// An error that is tried again is no failure; any other is.
#[test]
fn a_reader_fails_for_good_only() {
    let mut n = Pipelines::default();
    for e in [ShaderCacheError::ShaderImportNotYetAvailable, ShaderCacheError::ShaderNotLoaded(AssetId::default())] {
        n.count(&CachedPipelineState::Err(e));
    }
    n.count(&CachedPipelineState::Queued);
    assert_eq!(n, Pipelines { waiting: 3, ..default() });
    for e in [ShaderCacheError::CreateShaderModule("made".into()), ShaderCacheError::ProcessShaderError("made".into())] {
        n.count(&CachedPipelineState::Err(e));
    }
    assert_eq!(n, Pipelines { waiting: 3, failed: 2, ..default() });
}

// ---------------------------------------------------------------------------------------
// The admission.

/// Each limit, at what is needed and one below it. Bindless or not and with the clouds or without is
/// a matter of the counts (the next test).
#[test]
fn the_admission() {
    let needed = StageCounts { textures: 27, samplers: 15, uniforms: 16 };
    let fits = Limits { stage: needed, side: 256, format: true };
    // 32 MiB, to the byte.
    let atlas = [256, 256, 64];
    assert_eq!(8 * 256 * 256 * 64, MOST_BYTES);
    assert_eq!(admit(&fits, needed, atlas), Ok(()));
    for (why, limits, atlas) in [
        ("sampled textures", Limits { stage: StageCounts { textures: 26, ..needed }, ..fits }, atlas),
        ("samplers", Limits { stage: StageCounts { samplers: 14, ..needed }, ..fits }, atlas),
        ("uniform buffers", Limits { stage: StageCounts { uniforms: 15, ..needed }, ..fits }, atlas),
        ("side", Limits { side: 255, ..fits }, atlas),
        ("filtered", Limits { format: false, ..fits }, atlas),
        ("bytes", fits, [256, 256, 65]),
        ("bytes", Limits { side: u32::MAX, ..fits }, [u32::MAX; 3]),
    ] {
        assert!(admit(&limits, needed, atlas).unwrap_err().contains(why), "{why}");
    }
}

/// What is counted is what bevy_pbr lays out: the volume's three bindings, the clouds' two arrays,
/// the tonemapping's table, the contact shadows' uniform (which the clouds' key leaves out); the
/// mesh's and a material's beside it.
#[test]
fn the_counts_are_of_the_layouts() {
    let (device, adapter, _) = noop();
    let mut world = World::new();
    world.insert_resource(device.clone());
    world.insert_resource(adapter);
    world.run_system_cached(init_mesh_pipeline_view_layouts).expect("the layouts");
    let layouts = world.resource::<MeshPipelineViewLayouts>();
    let on = |clouds| view_counts(layouts, view_key(clouds));
    let more = |c: StageCounts, textures, samplers, uniforms| c.plus(StageCounts { textures, samplers, uniforms });
    assert_eq!(on(true), more(on(false), 2, 0, 0), "the light volume's arrays");
    assert!(!crate::sun_visibility::fullest_view_key().contains(MeshPipelineViewLayoutKey::CONTACT_SHADOWS));
    for clouds in [false, true] {
        let without = |bit: MeshPipelineViewLayoutKey| view_counts(layouts, view_key(clouds).difference(bit));
        assert_eq!(on(clouds), more(without(MeshPipelineViewLayoutKey::NVF_RTGI_VOLUME), 1, 1, 1));
        assert_eq!(on(clouds), more(without(MeshPipelineViewLayoutKey::TONEMAP_IN_SHADER), 1, 1, 0));
        assert_eq!(on(clouds), more(without(MeshPipelineViewLayoutKey::CONTACT_SHADOWS), 0, 0, 1));
        let (all, rest) = (counts(&device, layouts, clouds), on(clouds).plus(MESH));
        assert!(all.textures > rest.textures && all.samplers > rest.samplers && all.uniforms >= rest.uniforms, "{all:?}, {rest:?} before the material");
    }
}

// ---------------------------------------------------------------------------------------
// The extraction.

/// A volume that is not what it says is refused before anything is done with it.
#[test]
fn a_broken_volume_is_no_request() {
    let mut v = made::made([0; 3]);
    v.rgba16f_le.pop();
    assert!(RtgiRequest::solved(HERE, 1, Arc::new(v), made::FIT, 1.0).is_err());
}

/// One extraction, as Bevy's own: the main world lent to the render world.
fn extraction(app: &mut App) {
    let mut render = app.remove_sub_app(RenderApp).unwrap();
    let mut main = MainWorld::default();
    std::mem::swap(&mut *main, app.world_mut());
    render.world_mut().insert_resource(main);
    render.world_mut().run_schedule(ExtractSchedule);
    let mut main = render.world_mut().remove_resource::<MainWorld>().unwrap();
    std::mem::swap(&mut *main, app.world_mut());
    app.insert_sub_app(RenderApp, render);
}

type View = (Entity, RetainedViewEntity);

fn views(world: &mut World) -> [View; 2] {
    [Msaa::Off, Msaa::Sample4].map(|msaa| {
        let view = RetainedViewEntity::new(MainEntity::from(world.spawn_empty().id()), None, 0);
        (world.spawn((extracted(view), msaa)).id(), view)
    })
}

fn keys(world: &mut World, views: &[View; 2]) -> [MeshPipelineKey; 2] {
    world.init_resource::<ViewKeyCache>();
    world.init_resource::<DirtySpecializations>();
    world.run_system_cached(check_views_need_specialization).unwrap();
    views.map(|(_, view)| world.resource::<ViewKeyCache>()[&view])
}

/// A frame's extraction: the counters, what the two views have bound, whether their keys are those
/// with the volume's bit where they have, and what is held.
fn frame(app: &mut App, ours: &[View; 2], plain: &[MeshPipelineKey; 2]) -> ([u64; 4], [Option<[String; 3]>; 2], Option<Key>) {
    extraction(app);
    let render = app.sub_app_mut(RenderApp).world_mut();
    let bound = ours.map(|(e, _)| render.get::<NvfRtgiVolumeBindings>(e).map(|b| [format!("{:?}", b.uniform.id()), format!("{:?}", b.texture.id()), format!("{:?}", b.sampler.id())]));
    let want: [MeshPipelineKey; 2] = std::array::from_fn(|i| if bound[i].is_some() { plain[i] | MeshPipelineKey::NVF_RTGI_VOLUME } else { plain[i] });
    assert_eq!(keys(render, ours), want);
    (render.resource::<RtgiCounts>().all(), bound, render.resource::<RtgiGpu>().held.as_ref().map(|h| h.key))
}

/// Without a request: two cameras, an interior, a travel and the outdoors, in worlds without a device
/// or a queue (what would make something panics): nothing is counted, held or bound, and the views'
/// keys are those without the plugin. With one, through the same extraction, the whole of a visit.
/// In a process of its own, as it sets the hook's state.
#[test]
fn the_extraction_without_a_request_and_with_one() {
    const CHILD: &str = "NVF_TEST_RTGI_EXTRACTION_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["rtgi_volume::owner_tests::the_extraction_without_a_request_and_with_one", "--exact", "--nocapture", "--test-threads=1"])
            .env(CHILD, "1")
            .output()
            .expect("start the test alone");
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success() && text.contains("1 passed"), "{text}\n{}", String::from_utf8_lossy(&output.stderr));
        return;
    }
    let mut world = World::new();
    let of_plain = views(&mut world);
    let plain = keys(&mut world, &of_plain);

    let mut app = App::new();
    app.init_resource::<Assets<Shader>>().insert_sub_app(RenderApp, SubApp::new());
    app.add_plugins(RtgiVolumePlugin);
    let render = app.sub_app_mut(RenderApp).world_mut();
    render.run_schedule(RenderStartup);
    let ours = views(render);
    // Drawn on a target of 64 x 64.
    let camera = |app: &mut App, view: usize| {
        let target_info = Some(RenderTargetInfo { physical_size: UVec2::splat(64), scale_factor: 1.0 });
        let drawn = Camera { computed: ComputedCameraValues { target_info, ..default() }, ..default() };
        app.world_mut().spawn((Camera3d::default(), drawn, crate::fly::FlyCam { yaw: 0.0, pitch: 0.0, speed: 0.0 }, RenderEntity::from(ours[view].0))).id()
    };
    let cameras = [0, 1].map(|view| camera(&mut app, view));
    // The player's place after `travels` travels, and a screen: none, up, or settling.
    let go = |app: &mut App, interior, travels| {
        app.insert_resource(Streamer { travels, ..test_streamer(interior) });
        place(app.world().get_resource(), None)
    };
    let screen = |app: &mut App, settling: Option<bool>| {
        app.insert_resource(settling.map_or_else(Loading::default, Loading::starting_for_test));
    };
    let frame = |app: &mut App| frame(app, &ours, &plain);
    let nothing = |n| (n, [None, None], None);

    // Nothing is asked for.
    for (interior, travels) in [(Some(5), 0), (Some(5), 1), (None, 2)] {
        go(&mut app, interior, travels);
        screen(&mut app, (travels == 1).then_some(false));
        assert_eq!(frame(&mut app), nothing([0; 4]));
    }
    assert!(!app.world().contains_resource::<RtgiRequest>() && app.sub_app(RenderApp).world().resource::<RtgiGpu>().asked.is_none());

    // Asked for before there is a device, then with one and no view (no camera, one that is not
    // active or has no target): not yet.
    let here = go(&mut app, Some(5), 3).unwrap();
    assert_eq!(here, Place { cell: 5, travels: 3, game: None });
    set_hook_state(HookState::Installed);
    screen(&mut app, Some(false));
    app.insert_resource(RtgiRequest::empty(here, 1));
    assert_eq!(frame(&mut app), nothing([0; 4]));
    let (device, adapter, queue) = noop();
    let render = app.sub_app_mut(RenderApp).world_mut();
    render.insert_resource(device);
    render.insert_resource(adapter);
    render.insert_resource(queue);
    render.run_system_cached(init_mesh_pipeline_view_layouts).expect("the layouts");
    // The no-op adapter says nothing of formats, so the device's answer is given here.
    let grant = |app: &mut App, atlas| app.sub_app_mut(RenderApp).world_mut().resource_mut::<RtgiGpu>().asked = Some(Asked { clouds: false, atlas, granted: Ok(()) });
    grant(&mut app, EMPTY);
    for c in cameras {
        app.world_mut().despawn(c);
    }
    assert_eq!(frame(&mut app), nothing([0; 4]));
    let unseen = camera(&mut app, 0);
    for (active, target) in [(false, Some(UVec2::splat(64))), (true, None), (true, Some(UVec2::new(64, 0)))] {
        let mut c = app.world_mut().get_mut::<Camera>(unseen).unwrap();
        c.is_active = active;
        c.computed.target_info = target.map(|physical_size| RenderTargetInfo { physical_size, scale_factor: 1.0 });
        assert_eq!(frame(&mut app), nothing([0; 4]));
    }
    app.world_mut().despawn(unseen);

    // The empty texture, once; a solved one in its place (buffer and sampler stay) when the screen
    // is down; another share.
    let first = camera(&mut app, 0);
    let (n, one, held) = frame(&mut app);
    assert_eq!((n, one[0].is_some(), &one[1], held), ([3, 2, 1, 0], true, &None, Some(key(here, 1, false))));
    assert_eq!(frame(&mut app), ([3, 2, 1, 0], one.clone(), held), "nothing again");
    screen(&mut app, None);
    let solved = |revision, share| RtgiRequest::solved(here, revision, Arc::new(made::made([0; 3])), made::FIT, share).unwrap();
    grant(&mut app, [5, 4, 24]);
    app.insert_resource(solved(2, 0.0));
    let (n, two, held) = frame(&mut app);
    let [before, after] = [&one, &two].map(|b| b[0].clone().unwrap());
    assert_eq!((n, held), ([4, 4, 2, 0], Some(key(here, 2, true))));
    assert!(before[0] == after[0] && before[1] != after[1] && before[2] == after[2]);
    app.insert_resource(solved(2, 0.5));
    assert_eq!(frame(&mut app), ([4, 5, 2, 0], two.clone(), held));
    // The camera replaced.
    app.world_mut().despawn(first);
    camera(&mut app, 1);
    let moved = [None, two[0].clone()];
    assert_eq!(frame(&mut app), ([4, 5, 3, 0], moved.clone(), held));
    // An older result, however large, changes nothing: what is held is what is admitted.
    let large = |revision| {
        let volume = Arc::new(SolvedVolume { atlas: [256, 256, 65], ..made::made([0; 3]) });
        RtgiRequest { place: here, revision, solved: Some(Solved { volume, fit: made::FIT, share: 1.0 }) }
    };
    app.insert_resource(large(1));
    assert_eq!(frame(&mut app), ([4, 5, 3, 0], moved, held));
    // A newer one that is refused ends the visit: what would be granted is not bound any more, under
    // a screen either.
    app.insert_resource(large(3));
    assert_eq!(frame(&mut app), nothing([4, 5, 3, 0]));
    grant(&mut app, EMPTY);
    app.insert_resource(RtgiRequest::empty(here, 4));
    screen(&mut app, Some(false));
    assert_eq!(frame(&mut app), nothing([4, 5, 3, 0]));

    // The next visit, whose first frame is the one the screen settles in: late, and so for good.
    let there = go(&mut app, Some(5), 4).unwrap();
    app.insert_resource(RtgiRequest::empty(there, 1));
    screen(&mut app, Some(true));
    assert_eq!(frame(&mut app), nothing([4, 5, 3, 0]));
    screen(&mut app, None);
    assert_eq!(frame(&mut app), nothing([4, 5, 3, 0]));
    screen(&mut app, Some(false));
    assert_eq!(frame(&mut app), nothing([4, 5, 3, 0]));
    // The one after it has no view under its screen, and one when that has settled: late too.
    let unseen = go(&mut app, Some(5), 5).unwrap();
    let activate = |app: &mut App, on| app.world_mut().query::<&mut Camera>().single_mut(app.world_mut()).unwrap().is_active = on;
    app.insert_resource(RtgiRequest::empty(unseen, 1));
    activate(&mut app, false);
    assert_eq!(frame(&mut app), nothing([4, 5, 3, 0]));
    screen(&mut app, Some(true));
    activate(&mut app, true);
    assert_eq!(frame(&mut app), nothing([4, 5, 3, 0]));
    assert_eq!(app.sub_app(RenderApp).world().resource::<RtgiGpu>().ended, Some((unseen, Why::Late)));
    screen(&mut app, None);
    assert_eq!(frame(&mut app), nothing([4, 5, 3, 0]));
    // The last has a screen of its own; no request, and it is gone.
    let last = go(&mut app, Some(5), 6).unwrap();
    screen(&mut app, Some(false));
    app.insert_resource(RtgiRequest::empty(last, 1));
    assert_eq!(frame(&mut app).2, Some(key(last, 1, false)));
    app.world_mut().remove_resource::<RtgiRequest>();
    assert_eq!(frame(&mut app), nothing([7, 7, 4, 0]));
}
