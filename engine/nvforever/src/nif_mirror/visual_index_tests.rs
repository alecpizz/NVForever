//! Unit tests for [`VisualRootCache`] and [`MirrorIndex::build_from_cache`]:
//! incremental spawn, despawn, respawn, reparenting, `RefVisual` mutation with child
//! promotion, `DrawOnly`/`PointLight`/`SpotLight`/`SessionLight` add/remove transitions,
//! `ScriptState.rt == None` frame gaps, and zero-stale-entity parity with [`root_of`].

use super::*;

#[derive(Resource, Default)]
struct TestCache {
    cache: VisualRootCache,
    rt_active: bool,
    wanted: HashSet<u32>,
    results: HashMap<u32, Option<Entity>>,
    oracle: HashMap<u32, Option<Entity>>,
    removed_visual_drained: bool,
}

fn sync_and_query(
    mut state: ResMut<TestCache>,
    rigs: Rigs,
    visuals: Visuals,
    mut changes: VisualChanges,
    dropped: Dropped,
) {
    // Mirrors `mirror()` / `mirror_game_impl()`: only sync while `rt` is active and
    // `wanted` is non-empty; otherwise drain `RemovedComponents` and clear `initialized`.
    if !state.rt_active || state.wanted.is_empty() {
        state.cache.suspend(&mut changes);
        state.removed_visual_drained = changes.removed_visual.is_empty();
        state.results.clear();
        state.oracle.clear();
        return;
    }
    state.cache.sync(&visuals, &mut changes);
    state.removed_visual_drained = changes.removed_visual.is_empty();
    let wanted = state.wanted.clone();
    let index = MirrorIndex::build_from_cache(&wanted, &rigs, &visuals, &dropped, &mut state.cache);
    state.results.clear();
    state.oracle.clear();
    for r in wanted {
        state.results.insert(r, index.root_of(r, ModelView::Reference));
        state.oracle.insert(r, root_of(r, ModelView::Reference, &rigs, &visuals));
    }
}

fn run_step(app: &mut App, refs: &[u32]) {
    {
        let mut state = app.world_mut().resource_mut::<TestCache>();
        state.rt_active = true;
        state.wanted = refs.iter().copied().collect();
    }
    app.update();
    let state = app.world().resource::<TestCache>();
    for &r in refs {
        assert_eq!(
            state.results.get(&r).copied().flatten(),
            state.oracle.get(&r).copied().flatten(),
            "cached root mismatch for 0x{r:08X}",
        );
    }
}

#[test]
fn visual_root_cache_tracks_add_remove_respawn_reparent_and_filter_changes() {
    const R_A: u32 = 0x5001;
    const R_B: u32 = 0x5002;
    const R_C: u32 = 0x5003;
    const R_D: u32 = 0x5004;
    const R_MISSING: u32 = 0x5099;
    let tracked = [R_A, R_B, R_C, R_D, R_MISSING];

    let mut app = App::new();
    app.init_resource::<TestCache>();
    app.add_systems(Update, sync_and_query);

    // Frame 1 (cold init): spawn hierarchy, light-only, DrawOnly.
    let a_root = app.world_mut().spawn((Transform::IDENTITY, RefVisual(R_A))).id();
    let a_child = app.world_mut().spawn((Transform::IDENTITY, RefVisual(R_A), ChildOf(a_root))).id();
    let b_draw_only = app.world_mut().spawn((Transform::IDENTITY, RefVisual(R_B), nvf_play::DrawOnly)).id();
    app.world_mut().spawn((PointLight::default(), RefVisual(R_C)));

    run_step(&mut app, &tracked);
    assert_eq!(app.world().resource::<TestCache>().results[&R_A], Some(a_root));
    assert_eq!(app.world().resource::<TestCache>().results[&R_B], None);
    assert_eq!(app.world().resource::<TestCache>().results[&R_C], None);
    assert_eq!(app.world().resource::<TestCache>().results[&R_MISSING], None);

    // Frame 2 (incremental): spawn R_C real visual, remove DrawOnly on R_B, remove RefVisual on a_root so a_child becomes root.
    let c_real = app.world_mut().spawn((Transform::IDENTITY, RefVisual(R_C))).id();
    app.world_mut().entity_mut(b_draw_only).remove::<nvf_play::DrawOnly>();
    app.world_mut().entity_mut(a_root).remove::<RefVisual>();

    run_step(&mut app, &tracked);
    assert_eq!(app.world().resource::<TestCache>().results[&R_A], Some(a_child));
    assert_eq!(app.world().resource::<TestCache>().results[&R_B], Some(b_draw_only));
    assert_eq!(app.world().resource::<TestCache>().results[&R_C], Some(c_real));

    // Frame 3 (despawn + respawn): despawn c_real -> None; then respawn new entity -> new entity (zero stale entity).
    app.world_mut().despawn(c_real);
    run_step(&mut app, &tracked);
    assert_eq!(app.world().resource::<TestCache>().results[&R_C], None);

    let c_respawn = app.world_mut().spawn((Transform::IDENTITY, RefVisual(R_C))).id();
    run_step(&mut app, &tracked);
    assert_eq!(app.world().resource::<TestCache>().results[&R_C], Some(c_respawn));
    assert_ne!(c_respawn, c_real);

    // Frame 4 (reparent + RefVisual mutation): reparent c_respawn under a new C parent, mutate R_B -> R_D.
    let c_new_parent = app.world_mut().spawn((Transform::IDENTITY, RefVisual(R_C))).id();
    app.world_mut().entity_mut(c_respawn).insert(ChildOf(c_new_parent));
    app.world_mut().entity_mut(b_draw_only).insert(RefVisual(R_D));

    run_step(&mut app, &tracked);
    assert_eq!(app.world().resource::<TestCache>().results[&R_C], Some(c_new_parent));
    assert_eq!(app.world().resource::<TestCache>().results[&R_B], None);
    assert_eq!(app.world().resource::<TestCache>().results[&R_D], Some(b_draw_only));

    // Frame 5 (filter addition): adding DrawOnly or PointLight invalidates cached entry with zero stale return.
    app.world_mut().entity_mut(b_draw_only).insert(nvf_play::DrawOnly);
    app.world_mut().entity_mut(a_child).insert(PointLight::default());

    run_step(&mut app, &tracked);
    assert_eq!(app.world().resource::<TestCache>().results[&R_D], None);
    assert_eq!(app.world().resource::<TestCache>().results[&R_A], None);
}

#[test]
fn visual_root_cache_promotes_children_on_parent_mutation_and_exclusion_and_tracks_session_light() {
    const R_MUT_OLD: u32 = 0x6001;
    const R_MUT_NEW: u32 = 0x6002;
    const R_PLAYER_MUT: u32 = 0x6003;
    const R_DRAW: u32 = 0x6004;
    const R_POINT: u32 = 0x6005;
    const R_SPOT: u32 = 0x6006;
    const R_SESSION: u32 = 0x6007;
    const R_SESSION_REMOVE: u32 = 0x6008;
    const R_CHAIN: u32 = 0x6009;
    let tracked_single_root = [
        R_MUT_OLD,
        R_MUT_NEW,
        R_PLAYER_MUT,
        R_DRAW,
        R_POINT,
        R_SPOT,
        R_SESSION,
        R_SESSION_REMOVE,
    ];
    let tracked_all = [
        R_MUT_OLD,
        R_MUT_NEW,
        R_PLAYER_MUT,
        R_DRAW,
        R_POINT,
        R_SPOT,
        R_SESSION,
        R_SESSION_REMOVE,
        R_CHAIN,
    ];

    let mut app = App::new();
    app.init_resource::<TestCache>();
    app.add_systems(Update, sync_and_query);

    // Parent/child pairs (include a bare parent without Transform on R_DRAW to prove Children query works without NodeQuery).
    let mut_parent = app.world_mut().spawn((Transform::IDENTITY, RefVisual(R_MUT_OLD))).id();
    let mut_child = app.world_mut().spawn((Transform::IDENTITY, RefVisual(R_MUT_OLD), ChildOf(mut_parent))).id();

    let player_mut_parent = app.world_mut().spawn((Transform::IDENTITY, RefVisual(R_PLAYER_MUT))).id();
    let player_mut_child = app.world_mut().spawn((Transform::IDENTITY, RefVisual(R_PLAYER_MUT), ChildOf(player_mut_parent))).id();

    let draw_parent = app.world_mut().spawn(RefVisual(R_DRAW)).id();
    let draw_child = app.world_mut().spawn((Transform::IDENTITY, RefVisual(R_DRAW), ChildOf(draw_parent))).id();

    let point_parent = app.world_mut().spawn((Transform::IDENTITY, RefVisual(R_POINT))).id();
    let point_child = app.world_mut().spawn((Transform::IDENTITY, RefVisual(R_POINT), ChildOf(point_parent))).id();

    let spot_parent = app.world_mut().spawn((Transform::IDENTITY, RefVisual(R_SPOT))).id();
    let spot_child = app.world_mut().spawn((Transform::IDENTITY, RefVisual(R_SPOT), ChildOf(spot_parent))).id();

    let session_parent = app.world_mut().spawn((Transform::IDENTITY, RefVisual(R_SESSION))).id();
    let session_child = app.world_mut().spawn((Transform::IDENTITY, RefVisual(R_SESSION), ChildOf(session_parent))).id();

    let session_only = app
        .world_mut()
        .spawn((Transform::IDENTITY, RefVisual(R_SESSION_REMOVE), crate::lights::SessionLight::test_stub()))
        .id();

    let chain_gp = app.world_mut().spawn((Transform::IDENTITY, RefVisual(R_CHAIN))).id();
    let chain_mid = app.world_mut().spawn((Transform::IDENTITY, RefVisual(R_CHAIN), ChildOf(chain_gp))).id();
    let chain_leaf = app.world_mut().spawn((Transform::IDENTITY, RefVisual(R_CHAIN), ChildOf(chain_mid))).id();

    run_step(&mut app, &tracked_all);
    let r = &app.world().resource::<TestCache>().results;
    assert_eq!(r[&R_MUT_OLD], Some(mut_parent));
    assert_eq!(r[&R_MUT_NEW], None);
    assert_eq!(r[&R_PLAYER_MUT], Some(player_mut_parent));
    assert_eq!(r[&R_DRAW], Some(draw_parent));
    assert_eq!(r[&R_POINT], Some(point_parent));
    assert_eq!(r[&R_SPOT], Some(spot_parent));
    assert_eq!(r[&R_SESSION], Some(session_parent));
    assert_eq!(r[&R_SESSION_REMOVE], None);
    assert_eq!(r[&R_CHAIN], Some(chain_gp));

    // Mutate parent RefVisuals, add all four exclusion components to parents, remove SessionLight on session_only,
    // and exclude chain_mid in the 3-level chain (which makes both chain_gp and chain_leaf valid roots simultaneously).
    app.world_mut().entity_mut(mut_parent).insert(RefVisual(R_MUT_NEW));
    app.world_mut().entity_mut(player_mut_parent).insert(RefVisual(FormId::PLAYER_REF.0));
    app.world_mut().entity_mut(draw_parent).insert(nvf_play::DrawOnly);
    app.world_mut().entity_mut(point_parent).insert(PointLight::default());
    app.world_mut().entity_mut(spot_parent).insert(SpotLight::default());
    app.world_mut().entity_mut(session_parent).insert(crate::lights::SessionLight::test_stub());
    app.world_mut().entity_mut(session_only).remove::<crate::lights::SessionLight>();
    app.world_mut().entity_mut(chain_mid).insert(crate::lights::SessionLight::test_stub());

    run_step(&mut app, &tracked_single_root);
    let r = &app.world().resource::<TestCache>().results;
    assert_eq!(r[&R_MUT_OLD], Some(mut_child));
    assert_eq!(r[&R_MUT_NEW], Some(mut_parent));
    assert_eq!(r[&R_PLAYER_MUT], Some(player_mut_child));
    assert_eq!(r[&R_DRAW], Some(draw_child));
    assert_eq!(r[&R_POINT], Some(point_child));
    assert_eq!(r[&R_SPOT], Some(spot_child));
    assert_eq!(r[&R_SESSION], Some(session_child));
    assert_eq!(r[&R_SESSION_REMOVE], Some(session_only));

    // Removing SessionLight from session_parent restores it as root and re-shadows session_child;
    // removing RefVisual from chain_gp while chain_mid is still excluded leaves chain_leaf as the unique root.
    app.world_mut().entity_mut(session_parent).remove::<crate::lights::SessionLight>();
    app.world_mut().entity_mut(chain_gp).remove::<RefVisual>();
    run_step(&mut app, &tracked_all);
    let r = &app.world().resource::<TestCache>().results;
    assert_eq!(r[&R_SESSION], Some(session_parent));
    assert_eq!(r[&R_CHAIN], Some(chain_leaf));

    // Removing SessionLight from chain_mid re-shadows chain_leaf and promotes chain_mid as sole root.
    app.world_mut().entity_mut(chain_mid).remove::<crate::lights::SessionLight>();
    run_step(&mut app, &tracked_all);
    assert_eq!(app.world().resource::<TestCache>().results[&R_CHAIN], Some(chain_mid));

    // Recursive despawn of chain_gp removes chain_mid and chain_leaf cleanly -> None.
    app.world_mut().despawn(chain_gp);
    run_step(&mut app, &tracked_all);
    assert_eq!(app.world().resource::<TestCache>().results[&R_CHAIN], None);
}

#[test]
fn visual_root_cache_does_not_drop_deltas_while_script_runtime_is_none() {
    const R_BEFORE: u32 = 0x7001;
    const R_DURING: u32 = 0x7002;
    const R_SESSION: u32 = 0x7003;
    const R_PROMOTE: u32 = 0x7004;
    let tracked = [R_BEFORE, R_DURING, R_SESSION, R_PROMOTE];

    let mut app = App::new();
    app.init_resource::<TestCache>();
    app.add_systems(Update, sync_and_query);

    // Frame 1: rt active, cache initialized.
    let before = app.world_mut().spawn((Transform::IDENTITY, RefVisual(R_BEFORE))).id();
    let sess = app
        .world_mut()
        .spawn((Transform::IDENTITY, RefVisual(R_SESSION), crate::lights::SessionLight::test_stub()))
        .id();
    let p_parent = app.world_mut().spawn((Transform::IDENTITY, RefVisual(R_PROMOTE))).id();
    let p_child = app.world_mut().spawn((Transform::IDENTITY, RefVisual(R_PROMOTE), ChildOf(p_parent))).id();
    run_step(&mut app, &tracked);
    assert_eq!(app.world().resource::<TestCache>().results[&R_BEFORE], Some(before));
    assert_eq!(app.world().resource::<TestCache>().results[&R_DURING], None);
    assert_eq!(app.world().resource::<TestCache>().results[&R_SESSION], None);
    assert_eq!(app.world().resource::<TestCache>().results[&R_PROMOTE], Some(p_parent));

    // Frames 2-3: rt becomes None (e.g. menu / load transition); perform spawns, despawns,
    // SessionLight removal, and parent exclusion while rt is None.
    app.world_mut().resource_mut::<TestCache>().rt_active = false;
    app.world_mut().despawn(before);
    let during = app.world_mut().spawn((Transform::IDENTITY, RefVisual(R_DURING))).id();
    app.update();
    assert!(!app.world().resource::<TestCache>().cache.initialized);
    assert!(app.world().resource::<TestCache>().removed_visual_drained);

    app.world_mut().entity_mut(sess).remove::<crate::lights::SessionLight>();
    app.world_mut().entity_mut(p_parent).insert(nvf_play::DrawOnly);
    app.update();
    assert!(!app.world().resource::<TestCache>().cache.initialized);

    // Frame 4: rt returns; cache must rebuild once and reflect every delta that occurred while rt was None.
    run_step(&mut app, &tracked);
    assert!(app.world().resource::<TestCache>().cache.initialized);
    let r = &app.world().resource::<TestCache>().results;
    assert_eq!(r[&R_BEFORE], None);
    assert_eq!(r[&R_DURING], Some(during));
    assert_eq!(r[&R_SESSION], Some(sess));
    assert_eq!(r[&R_PROMOTE], Some(p_child));

    // Also verify the real `mirror` system runs cleanly with no `ScriptState` resource across spawns/removals.
    let mut mirror_app = App::new();
    mirror_app.add_systems(PostUpdate, mirror);
    let e = mirror_app.world_mut().spawn((Transform::IDENTITY, RefVisual(R_DURING))).id();
    mirror_app.update();
    mirror_app.world_mut().despawn(e);
    mirror_app.update();
}

#[test]
fn visual_root_cache_suspends_when_idle_drains_removals_and_rebuilds_on_resume() {
    const R_KEPT: u32 = 0x8001;
    const R_DESPAWNED: u32 = 0x8002;
    const R_SPAWNED_IDLE: u32 = 0x8003;
    const R_PROMOTED_IDLE: u32 = 0x8004;
    let tracked = [R_KEPT, R_DESPAWNED, R_SPAWNED_IDLE, R_PROMOTED_IDLE];

    let mut app = App::new();
    app.init_resource::<TestCache>();
    app.add_systems(Update, sync_and_query);

    let kept = app.world_mut().spawn((Transform::IDENTITY, RefVisual(R_KEPT))).id();
    let despawned = app.world_mut().spawn((Transform::IDENTITY, RefVisual(R_DESPAWNED))).id();
    let parent = app.world_mut().spawn((Transform::IDENTITY, RefVisual(R_PROMOTED_IDLE))).id();
    let child = app.world_mut().spawn((Transform::IDENTITY, RefVisual(R_PROMOTED_IDLE), ChildOf(parent))).id();

    // Frame 1: active watching initializes the cache.
    run_step(&mut app, &tracked);
    {
        let state = app.world().resource::<TestCache>();
        assert!(state.cache.initialized);
        assert_eq!(state.results[&R_KEPT], Some(kept));
        assert_eq!(state.results[&R_DESPAWNED], Some(despawned));
        assert_eq!(state.results[&R_SPAWNED_IDLE], None);
        assert_eq!(state.results[&R_PROMOTED_IDLE], Some(parent));
    }

    // Frame 2: watching stops (`wanted` is empty) while a visual is despawned; cache must
    // suspend (`initialized = false`), clear cached maps, and drain `RemovedComponents`.
    app.world_mut().despawn(despawned);
    {
        let mut state = app.world_mut().resource_mut::<TestCache>();
        state.rt_active = true;
        state.wanted.clear();
    }
    app.update();
    {
        let state = app.world().resource::<TestCache>();
        assert!(!state.cache.initialized, "idle frame must reset initialized so Added/Changed queries are skipped");
        assert!(state.cache.roots.is_empty());
        assert!(state.cache.entity_ref.is_empty());
        assert!(state.removed_visual_drained, "idle frame must drain RemovedComponents readers");
    }

    // Frame 3: another idle frame with new spawn + parent exclusion stays suspended.
    let spawned_idle = app.world_mut().spawn((Transform::IDENTITY, RefVisual(R_SPAWNED_IDLE))).id();
    app.world_mut().entity_mut(parent).insert(nvf_play::DrawOnly);
    app.update();
    assert!(!app.world().resource::<TestCache>().cache.initialized);

    // Frame 4: watching resumes -> cache rebuilds once and matches the live world.
    run_step(&mut app, &tracked);
    {
        let state = app.world().resource::<TestCache>();
        assert!(state.cache.initialized, "resuming watch must rebuild and re-arm the cache");
        assert_eq!(state.results[&R_KEPT], Some(kept));
        assert_eq!(state.results[&R_DESPAWNED], None);
        assert_eq!(state.results[&R_SPAWNED_IDLE], Some(spawned_idle));
        assert_eq!(state.results[&R_PROMOTED_IDLE], Some(child));
    }

    // Frame 5: incremental updates work normally after rebuild.
    let late = app.world_mut().spawn((Transform::IDENTITY, RefVisual(R_DESPAWNED))).id();
    run_step(&mut app, &tracked);
    assert!(app.world().resource::<TestCache>().cache.initialized);
    assert_eq!(app.world().resource::<TestCache>().results[&R_DESPAWNED], Some(late));
}

#[test]
fn visual_root_cache_handles_thousands_of_undrawn_watched_refs_incrementally() {
    let mut app = App::new();
    app.init_resource::<TestCache>();
    app.add_systems(Update, sync_and_query);

    let mut target = None;
    for i in 0..2_000u32 {
        let r = 0x0020_0000 + i;
        let root = app.world_mut().spawn((Transform::IDENTITY, RefVisual(r))).id();
        app.world_mut().spawn((Transform::IDENTITY, RefVisual(r), ChildOf(root)));
        if i == 1_234 {
            target = Some((r, root));
        }
    }
    let (drawn_ref, drawn_root) = target.unwrap();
    let mut watched: Vec<u32> = (0..2_500u32).map(|i| 0x4200_0000 + i).collect();
    watched.push(drawn_ref);

    // Frame 1: cold init.
    run_step(&mut app, &watched);
    assert_eq!(app.world().resource::<TestCache>().results[&drawn_ref], Some(drawn_root));

    // Frame 2: steady-state (zero entity changes) still resolves drawn_ref and None for all 2,500 undrawn refs.
    run_step(&mut app, &watched);
    assert_eq!(app.world().resource::<TestCache>().results[&drawn_ref], Some(drawn_root));
    for i in 0..2_500u32 {
        assert_eq!(app.world().resource::<TestCache>().results[&(0x4200_0000 + i)], None);
    }

    // Frame 3: despawn drawn_root and spawn a fresh one for a previously undrawn ref.
    let late_ref = 0x4200_0777u32;
    app.world_mut().despawn(drawn_root);
    let late_entity = app.world_mut().spawn((Transform::IDENTITY, RefVisual(late_ref))).id();
    run_step(&mut app, &watched);
    assert_eq!(app.world().resource::<TestCache>().results[&drawn_ref], None);
    assert_eq!(app.world().resource::<TestCache>().results[&late_ref], Some(late_entity));
}
