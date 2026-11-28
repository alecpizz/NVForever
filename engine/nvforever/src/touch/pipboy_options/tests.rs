use super::*;
#[test]
fn all_five_preferences_round_trip_independently_through_the_real_settings_ini() {
    for bits in 0..32 {
        let nonce=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let path=std::env::temp_dir().join(format!("nvf-pipboy-options-{}-{nonce}.ini",std::process::id()));
        let mut app=App::new(); install(&mut app);
        app.insert_resource(settings::SettingsFile(Some(path.clone())));
        let keys=["touch-pipboy-dismiss-left","touch-pipboy-dismiss-right","touch-pipboy-close-button","touch-pipboy-button-graphics","touch-pipboy-model-wheel"];
        for (bit,key) in keys.into_iter().enumerate() {
            let row=app.world().resource::<settings::Settings>().index(key).unwrap();
            let desired=bits&(1<<bit)!=0;
            let get=app.world().resource::<settings::Settings>().list()[row].get;
            if get(app.world())!=desired {assert_eq!(settings::flip(app.world_mut(),row),Some(desired));}
        }
        // Even the all-default combination takes the real store path.
        let row=app.world().resource::<settings::Settings>().index(keys[0]).unwrap();
        settings::flip(app.world_mut(),row); settings::flip(app.world_mut(),row);
        let stored=std::fs::read_to_string(&path).unwrap();
        let mut loaded=App::new(); install(&mut loaded);
        loaded.insert_resource(settings::SettingsFile(Some(path.clone())));settings::apply_stored(loaded.world_mut());
        for (bit,key) in keys.into_iter().enumerate() {
            let expected=bits&(1<<bit)!=0;
            assert!(stored.contains(&format!("{key}={}",if expected {1} else {0})));
            let row=loaded.world().resource::<settings::Settings>().index(key).unwrap();
            assert_eq!((loaded.world().resource::<settings::Settings>().list()[row].get)(loaded.world()),expected);
        }
        std::fs::remove_file(path).unwrap();
    }
}
#[test]
fn configured_regions_stay_inside_all_system_edges_and_reject_invalid_geometry() {
    let options=Options {left:true,right:true,close:true,..default()};
    for size in [Vec2::new(792.,363.),Vec2::new(363.,792.),Vec2::splat(1000.)] {
        let safe=SafeInsets {left:25.,top:13.,right:31.,bottom:27.};
        let regions=options.regions(size,safe); assert_eq!(regions.len(),3);
        for (_,r) in regions {
            assert!(r.min.x>=safe.left+8. && r.max.x<=size.x-safe.right-8.);
            assert!(r.min.y>=safe.top+8. && r.max.y<=size.y-safe.bottom-8.);
            assert!(r.width()>=48. && r.height()>=48.);
        }
    }
    assert!(options.regions(Vec2::new(f32::NAN,500.),SafeInsets::default()).is_empty());
    assert!(options.regions(Vec2::splat(1000.),SafeInsets {left:f32::INFINITY,..default()}).is_empty());
    assert_eq!(Options::default().dismissal(),(false,false,false));
    assert!(Options::default().graphics && Options::default().wheel);
}

include!("display_dismissal_tests.rs");
