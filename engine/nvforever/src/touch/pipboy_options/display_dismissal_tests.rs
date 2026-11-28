// Actual registered chooser/legacy callbacks -> existing Settings INI loader and
// writer. No alternate settings parser, persistence mock or process-global state.
const TEST_SIDE_KEY:&str="touch-pipboy-dismiss-side-v2";
const TEST_TRAVEL_KEY:&str="touch-pipboy-dismiss-travel-v1";
const TEST_LEFT_KEY:&str="touch-pipboy-dismiss-left";
const TEST_RIGHT_KEY:&str="touch-pipboy-dismiss-right";
fn stored_travel(app:&App)->f32 {
    let settings=app.world().resource::<settings::Settings>();
    let row=settings.index(TEST_TRAVEL_KEY).expect("actual registered travel setting");
    settings.list()[row].value(app.world())
}
struct SideFile(std::path::PathBuf);
impl SideFile {
    fn new(text:&str)->Self {
        use std::sync::atomic::{AtomicU64,Ordering};static NEXT:AtomicU64=AtomicU64::new(0);
        let file=std::env::temp_dir().join(format!("nvf-pipboy-sides-{}-{}.ini",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));
        assert!(!file.exists());std::fs::write(&file,text).unwrap();Self(file)
    }
}
impl Drop for SideFile {fn drop(&mut self){let _=std::fs::remove_file(&self.0);}}
fn side_app(file:&SideFile)->App {
    let mut app=App::new();install(&mut app);app.insert_resource(settings::SettingsFile(Some(file.0.clone())));app
}
fn chosen(app:&App)->String {
    let registry=app.world().resource::<settings::Settings>();let row=registry.index(TEST_SIDE_KEY).unwrap();
    (registry.choices(row).unwrap().chosen)(app.world())
}
fn save_choice(app:&mut App,name:&str) {
    let row=app.world().resource::<settings::Settings>().index(TEST_SIDE_KEY).unwrap();
    let other=if name=="Disabled" {"Both"} else {"Disabled"};
    assert!(settings::pick(app.world_mut(),row,other));assert!(settings::pick(app.world_mut(),row,name));
}
#[test]
fn explicit_disabled_beats_true_legacy_flags_even_when_the_default_choice_loader_is_skipped() {
    let input="[Settings]\ntouch-pipboy-dismiss-left=1\ntouch-pipboy-dismiss-right=1\ntouch-pipboy-dismiss-side-v2=Disabled\n[Unrelated]\nkeep=yes\n";
    let file=SideFile::new(input);let mut app=side_app(&file);
    assert_eq!(chosen(&app),"Disabled","exercise the equal-default list short circuit");
    settings::apply_stored(app.world_mut());
    assert_eq!(chosen(&app),"Disabled");assert_eq!(app.world().resource::<Options>().dismissal(),(false,false,false));
    assert_eq!(std::fs::read_to_string(&file.0).unwrap(),input,"virtual read migration never writes the profile");
    save_choice(&mut app,"Disabled");
    let stored=std::fs::read_to_string(&file.0).unwrap();
    assert_eq!(fnv_install::ini_get(&stored,settings::SECTION,TEST_LEFT_KEY).as_deref(),Some("0"));
    assert_eq!(fnv_install::ini_get(&stored,settings::SECTION,TEST_RIGHT_KEY).as_deref(),Some("0"));
    assert_eq!(fnv_install::ini_get(&stored,"Unrelated","keep").as_deref(),Some("yes"));
    let mut reload=side_app(&file);settings::apply_stored(reload.world_mut());assert_eq!(chosen(&reload),"Disabled");
}
#[test]
fn all_four_side_choices_load_save_reload_and_override_conflicting_legacy_flags() {
    for (name,pair) in [("Disabled",(false,false)),("Left only",(true,false)),("Right only",(false,true)),("Both",(true,true))] {
        for old in ["0","1"] {
            let input=format!("[Settings]\n{TEST_LEFT_KEY}={old}\n{TEST_RIGHT_KEY}={old}\n{TEST_SIDE_KEY}={}\n{TEST_TRAVEL_KEY}=0.5\n[Other]\nkeep=a=b\n",name);
            let file=SideFile::new(&input);let mut app=side_app(&file);settings::apply_stored(app.world_mut());
            assert_eq!((app.world().resource::<Options>().left,app.world().resource::<Options>().right),pair);
            assert_eq!(chosen(&app),name);assert!((stored_travel(&app)-0.5).abs()<1e-5);
            assert_eq!(std::fs::read_to_string(&file.0).unwrap(),input);
            save_choice(&mut app,name);let saved=std::fs::read_to_string(&file.0).unwrap();
            assert_eq!(fnv_install::ini_get(&saved,settings::SECTION,TEST_SIDE_KEY).as_deref(),Some(name));
            assert_eq!(fnv_install::ini_get(&saved,"Other","keep").as_deref(),Some("a=b"));
            let mut loaded=side_app(&file);settings::apply_stored(loaded.world_mut());
            assert_eq!(chosen(&loaded),name);assert!((stored_travel(&loaded)-0.5).abs()<1e-5);
        }
    }
}
#[test]
fn absent_new_side_key_migrates_both_legacy_switches_using_the_existing_boolean_grammar() {
    for (left,right,want) in [("1","YES","Both"),("off","true","Right only"),("ON","0","Left only"),("no","FALSE","Disabled")] {
        let input=format!("[Settings]\n{TEST_LEFT_KEY}={left}\n{TEST_RIGHT_KEY}={right}\n[Keep]\nvalue=untouched\n");
        let file=SideFile::new(&input);let mut app=side_app(&file);settings::apply_stored(app.world_mut());
        assert_eq!(chosen(&app),want);assert_eq!(std::fs::read_to_string(&file.0).unwrap(),input);
        save_choice(&mut app,want);let saved=std::fs::read_to_string(&file.0).unwrap();
        assert_eq!(fnv_install::ini_get(&saved,settings::SECTION,TEST_SIDE_KEY).as_deref(),Some(want));
        let mut loaded=side_app(&file);settings::apply_stored(loaded.world_mut());assert_eq!(chosen(&loaded),want);
    }
}
#[test]
fn malformed_or_absent_new_choice_preserves_ordinary_legacy_and_nondefault_current_skip_semantics() {
    for (rows,want) in [
        (format!("{TEST_SIDE_KEY}=unrecognized\n{TEST_LEFT_KEY}=1\n{TEST_RIGHT_KEY}=1\n"),"Both"),
        (format!("{TEST_SIDE_KEY}=unrecognized\n{TEST_LEFT_KEY}=0\n{TEST_RIGHT_KEY}=yes\n"),"Right only"),
        (format!("{TEST_SIDE_KEY}=unrecognized\n{TEST_LEFT_KEY}=broken\n{TEST_RIGHT_KEY}=broken\n"),"Left only"),
        (format!("{TEST_LEFT_KEY}=broken\n"),"Left only"),
        (String::new(),"Left only"),
    ] {
        let input=format!("[Settings]\n{rows}{TEST_TRAVEL_KEY}=broken\n[Other]\nkeep=1\n");
        let file=SideFile::new(&input);let mut app=App::new();install(&mut app);
        app.insert_resource(settings::SettingsFile(None));
        let row=app.world().resource::<settings::Settings>().index(TEST_SIDE_KEY).unwrap();
        assert!(settings::pick(app.world_mut(),row,"Left only"),"nondefault existing state makes invalid-value skip behavior discriminating");
        app.insert_resource(settings::SettingsFile(Some(file.0.clone())));
        settings::apply_stored(app.world_mut());
        assert_eq!(chosen(&app),want);assert!((stored_travel(&app)-0.375).abs()<1e-5);
        assert_eq!(std::fs::read_to_string(&file.0).unwrap(),input);
    }
}

#[test]
fn legacy_setters_remain_operational_but_real_settings_navigation_uses_the_single_side_chooser() {
    use nvf_play::settings_view::{parent_of,RowId,Visibility};
    let mut app=App::new();install(&mut app);app.insert_resource(settings::SettingsFile(None));
    let registry=app.world().resource::<settings::Settings>();let chooser=registry.index(TEST_SIDE_KEY).unwrap();let left=registry.index(TEST_LEFT_KEY).unwrap();let right=registry.index(TEST_RIGHT_KEY).unwrap();
    let visibility=Visibility::of_world(app.world());
    assert!(!visibility.shows(left,&registry.list()[left]) && !visibility.shows(right,&registry.list()[right]));
    assert!(visibility.shows(chooser,&registry.list()[chooser]));
    assert_eq!(parent_of(registry.list(),left,&visibility),Some(RowId::Setting(chooser)));
    assert_eq!(parent_of(registry.list(),right,&visibility),Some(RowId::Setting(chooser)));
    assert_eq!(settings::flip(app.world_mut(),left),Some(true));assert_eq!(chosen(&app),"Left only");
    assert_eq!(settings::flip(app.world_mut(),right),Some(true));assert_eq!(chosen(&app),"Both");
    assert!(settings::pick(app.world_mut(),chooser,"Disabled"));
    assert_eq!(app.world().resource::<Options>().dismissal(),(false,false,false));
}
