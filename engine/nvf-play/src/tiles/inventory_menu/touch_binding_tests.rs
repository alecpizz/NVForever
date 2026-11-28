// Included inside the actual native inventory fixture module.
fn confirm_mobile_binding(app: &mut bevy::prelude::App) -> bool {
    use crate::pipboy::touch_items::TouchItems;
    app.world_mut().resource_scope(|world, mut touch: bevy::prelude::Mut<TouchItems>| {
        world.resource_scope(|world, mut hotkeys: bevy::prelude::Mut<Hotkeys>| {
            world.resource_scope(|world,mut sim:bevy::prelude::Mut<crate::sim::SimState>| {
                touch.confirm_binding(world.resource::<ActiveMenu>(), &mut hotkeys,&mut sim.world)
            })
        })
    })
}

#[test]
fn armed_native_item_tap_proposes_without_using_then_confirms_saved_identity() {
    use crate::pipboy::touch_items::TouchItems;
    use bevy::input::touch::TouchPhase;
    let NativeInventoryFixture { mut app, row, .. } = native_inventory_fixture();
    let expected = {
        let menu = app.world().resource::<ActiveMenu>().0.as_ref().unwrap();
        let item = &menu.list(Side::Player).rows[0];
        crate::hotkeys::Slot { item: item.item.0, mods: item.mods }
    };
    assert!(app.world_mut().resource_mut::<TouchItems>().arm_binding(0));
    native_guard_event(&mut app, 150, TouchPhase::Started, row);
    native_guard_event(&mut app, 150, TouchPhase::Ended, row);
    for _ in 0..5 { app.update(); }
    assert!(app.world().resource::<TouchItems>().binding_ready());
    assert!(app.world().resource::<Hotkeys>().0.iter().all(Option::is_none));
    assert_eq!(app.world().resource::<NativeGuardSeen>().used, 0);
    assert_eq!(app.world().resource::<NativeGuardSeen>().selects, 0);
    assert_eq!(app.world_mut().resource_mut::<crate::sim::SimState>().world.item_count(FormId::PLAYER_REF, FormId(expected.item)).unwrap(), 3);
    assert!(confirm_mobile_binding(&mut app));
    assert_eq!(app.world().resource::<Hotkeys>().0[0], Some(expected));
    assert_eq!(app.world().resource::<TouchItems>().binding_slot(), None);
    assert!(!confirm_mobile_binding(&mut app), "a confirmation cannot replay");
    let encoded = serde_json::to_vec(&app.world().resource::<Hotkeys>().0).unwrap();
    let saved: [Option<crate::hotkeys::Slot>; crate::hotkeys::SLOTS] = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(saved[0], Some(expected), "saved identity includes modifications");
    assert_eq!(app.world().resource::<NativeGuardSeen>().used, 0);
}

#[test]
fn armed_native_scroll_never_becomes_a_candidate_and_cancellation_preserves_hardware() {
    use crate::pipboy::touch_items::TouchItems;
    use bevy::{prelude::*, input::touch::TouchPhase};
    let NativeInventoryFixture { mut app, row, .. } = native_inventory_fixture();
    assert!(app.world_mut().resource_mut::<TouchItems>().arm_binding(2));
    native_guard_event(&mut app, 151, TouchPhase::Started, row); app.update(); app.update();
    native_guard_event(&mut app, 151, TouchPhase::Moved, row - Vec2::Y * 60.0);
    native_guard_event(&mut app, 151, TouchPhase::Ended, row - Vec2::Y * 60.0);
    for _ in 0..5 { app.update(); }
    assert_eq!(app.world().resource::<ActiveMenu>().0.as_ref().unwrap().selected[Side::Player as usize], 3);
    assert!(!app.world().resource::<TouchItems>().binding_ready());
    assert_eq!(app.world().resource::<NativeGuardSeen>().used, 0);
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyF);
    app.world_mut().resource_mut::<TouchItems>().cancel_binding();
    assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::KeyF));
    assert_eq!(app.world().resource::<TouchItems>().binding_slot(), None);
    assert!(app.world().resource::<Hotkeys>().0.iter().all(Option::is_none));
}

#[test]
fn binding_confirmation_revalidates_actual_inventory_row_and_reserved_slot() {
    use crate::pipboy::touch_items::TouchItems;
    use bevy::input::touch::TouchPhase;
    let NativeInventoryFixture { mut app, row, .. } = native_inventory_fixture();
    assert!(!app.world_mut().resource_mut::<TouchItems>().arm_binding(crate::hotkeys::AMMO_SWAP));
    assert!(app.world_mut().resource_mut::<TouchItems>().arm_binding(3));
    native_guard_event(&mut app, 152, TouchPhase::Started, row);
    native_guard_event(&mut app, 152, TouchPhase::Ended, row);
    for _ in 0..5 { app.update(); }
    assert!(app.world().resource::<TouchItems>().binding_ready());
    app.world_mut().resource_mut::<ActiveMenu>().0.as_mut().unwrap().lists[Side::Player as usize].rows.swap(0, 1);
    assert!(!confirm_mobile_binding(&mut app));
    assert!(!app.world().resource::<TouchItems>().binding_ready());
    assert_eq!(app.world().resource::<TouchItems>().binding_slot(), Some(3));
    assert!(app.world().resource::<Hotkeys>().0.iter().all(Option::is_none));
    assert_eq!(app.world().resource::<NativeGuardSeen>().used, 0);
}


#[test]
fn binding_confirm_rechecks_actual_carried_identity_after_removal_and_different_mod_replacement(){
    use crate::pipboy::touch_items::TouchItems;
    use bevy::input::touch::TouchPhase;
    for replacement in ["removed","different_mods","live_only"] {
        let NativeInventoryFixture{mut app,row,..}=native_inventory_fixture();
        assert!(app.world_mut().resource_mut::<TouchItems>().arm_binding(0));
        native_guard_event(&mut app,153,TouchPhase::Started,row);
        native_guard_event(&mut app,153,TouchPhase::Ended,row);
        for _ in 0..5{app.update();}
        assert!(app.world().resource::<TouchItems>().binding_ready());
        let candidate={let menu=app.world().resource::<ActiveMenu>().0.as_ref().unwrap();let row=&menu.list(Side::Player).rows[0];
            crate::hotkeys::Slot{item:row.item.0,mods:row.mods}};
        {let mut sim=app.world_mut().resource_mut::<crate::sim::SimState>();
            assert_eq!(sim.world.remove_item(FormId::PLAYER_REF,FormId(candidate.item),3).unwrap(),3);
            if replacement!="removed" {
                let mods=if replacement=="different_mods"{candidate.mods^1}else{candidate.mods};
                sim.world.add_item_extra(FormId::PLAYER_REF,FormId(candidate.item),3,
                    fnv_sim::inventory::Extra{mods,live:replacement=="live_only",..Default::default()}).unwrap();
                let (count,pick)=crate::hotkeys::carried(&mut sim.world,candidate);
                assert_eq!(count,3,"existing gameplay fallback/reserved-group lookup is unchanged");
                assert_eq!(pick.mods,if replacement=="different_mods"{None}else{Some(candidate.mods)},
                    "confirmation must reject either fallback or a now-reserved live group");
            }
        }
        assert_eq!(app.world().resource::<ActiveMenu>().0.as_ref().unwrap().list(Side::Player).rows[0].count,3,
            "intentionally retain the old drawn row: validation must consult the simulation");
        assert!(!confirm_mobile_binding(&mut app));
        assert!(!app.world().resource::<TouchItems>().binding_ready());
        assert_eq!(app.world().resource::<TouchItems>().binding_slot(),Some(0));
        assert!(app.world().resource::<Hotkeys>().0.iter().all(Option::is_none));
        assert_eq!(app.world().resource::<NativeGuardSeen>().used,0);
        assert_eq!(app.world().resource::<NativeGuardSeen>().selects,0);
    }
}


#[test]
fn native_drag_assignment_rechecks_exact_carried_group_at_release_without_using_it() {
    use crate::pipboy::touch_items::TouchItems;
    use bevy::{prelude::*, input::touch::TouchPhase};
    for replacement in ["present", "removed", "different_mods", "live_only"] {
        let NativeInventoryFixture { mut app, row, .. } = native_inventory_fixture();
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::F12);
        let candidate = {
            let menu = app.world().resource::<ActiveMenu>().0.as_ref().unwrap();
            let item = &menu.list(Side::Player).rows[0];
            crate::hotkeys::Slot { item: item.item.0, mods: item.mods }
        };
        native_guard_event(&mut app, 159, TouchPhase::Started, row);
        app.update(); app.update();
        if replacement != "present" {
            let mut sim = app.world_mut().resource_mut::<crate::sim::SimState>();
            assert_eq!(sim.world.remove_item(FormId::PLAYER_REF, FormId(candidate.item), 3).unwrap(), 3);
            if replacement != "removed" {
                sim.world.add_item_extra(FormId::PLAYER_REF, FormId(candidate.item), 3,
                    fnv_sim::inventory::Extra {
                        mods: if replacement == "different_mods" { candidate.mods ^ 1 } else { candidate.mods },
                        live: replacement == "live_only", ..Default::default()
                    }).unwrap();
                let (count, pick) = crate::hotkeys::carried(&mut sim.world, candidate);
                assert_eq!(count, 3, "gameplay fallback remains available independently of assignment");
                assert_eq!(pick.mods, if replacement == "different_mods" { None } else { Some(candidate.mods) });
            }
        }
        // The drawn row intentionally remains valid. Its cached availability
        // alone cannot authorize an assignment after a script inventory change.
        assert_eq!(app.world().resource::<ActiveMenu>().0.as_ref().unwrap().list(Side::Player).rows[0].count, 3);
        native_guard_event(&mut app, 159, TouchPhase::Moved, row + Vec2::X * 80.0);
        native_guard_event(&mut app, 159, TouchPhase::Ended, row + Vec2::X * 80.0);
        app.world_mut().resource_mut::<TouchItems>().release_over(Some(2));
        for _ in 0..5 { app.update(); }
        assert_eq!(app.world().resource::<Hotkeys>().0[2], (replacement == "present").then_some(candidate), "{replacement}");
        assert_eq!(app.world().resource::<ActiveMenu>().0.as_ref().unwrap().list(Side::Player).rows[0].count, 3,
            "the cached model did not hide the actual-world authority check");
        assert!(app.world().resource::<TouchItems>().suppress_click());
        assert_eq!(app.world().resource::<NativeGuardSeen>().used, 0);
        assert_eq!(app.world().resource::<NativeGuardSeen>().selects, 0);
        assert_eq!(app.world_mut().resource_mut::<crate::sim::SimState>().world.item_count(
            FormId::PLAYER_REF, FormId(candidate.item)).unwrap(), if replacement == "removed" { 0 } else { 3 });
        assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::F12));
        // Retiring the pending operation prevents replay after inventory returns.
        if replacement == "removed" {
            app.world_mut().resource_mut::<crate::sim::SimState>().world
                .add_item(FormId::PLAYER_REF, FormId(candidate.item), 3).unwrap();
        }
        for _ in 0..3 { app.update(); }
        assert_eq!(app.world().resource::<Hotkeys>().0[2], (replacement == "present").then_some(candidate));
        assert_eq!(app.world().resource::<NativeGuardSeen>().used, 0);
        assert_eq!(app.world().resource::<NativeGuardSeen>().selects, 0);
        assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::F12));
    }
}
