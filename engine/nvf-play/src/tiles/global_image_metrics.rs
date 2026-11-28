//! Own-image metrics needed by expressions outside an open menu.
//! [design: docs/spec-notes/global-image-metrics.md]
use super::draw::FontKit;
use crate::script::ScriptState;
use bevy::prelude::*;
use fnv_ui::{TileId, Ui, Value};
use std::collections::{HashMap, HashSet};

// Engine work bounds, not original-game constants. Admission completes before writes.
const MAX_NODES: usize = 4096;
const MAX_IMAGES: usize = 64;

// Original-game fallback widths for own-image tiles [observed:
// w4g_09_global_image_metrics_missing, tools/capture/nvse/catalogs/wave4g_f2_extenders.json]:
// - Missing DDS filenames resolve filewidth to 256.0 on both visible and hidden tiles
//   (neither 0 nor stale previous width).
// - Empty filenames ("") substitute "Interface\Shared\empty.dds" with filewidth 1024.0.
// [hypothesis]: square fileheight == filewidth (256.0 for missing DDS; 1024.0 for empty /
// Interface\Shared\empty.dds) and recovery to real decoded dimensions when restored after a
// missing path (the committed catalog probe prints filewidth only on those steps).
const MISSING_TEXTURE_SIZE: (f64, f64) = (256.0, 256.0);
const EMPTY_IMAGE_FILENAME: &str = "Interface\\Shared\\empty.dds";
const EMPTY_IMAGE_KEY: &str = "interface\\shared\\empty.dds";
const EMPTY_TEXTURE_SIZE: (f64, f64) = (1024.0, 1024.0);

#[derive(Default)]
pub(super) struct Metrics {
    root: Option<TileId>,
    dimensions: HashMap<String, Option<(f64, f64)>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Report {
    images: usize,
    resolved: usize,
    unavailable: usize,
    limited: bool,
}

fn write(ui: &mut Ui, state: &mut Metrics, read: &mut impl FnMut(&str) -> Option<(f64, f64)>) -> Report {
    let root = ui.globals();
    if state.root != Some(root) {
        state.root = Some(root);
        state.dimensions.clear();
    }
    let mut stack = vec![root];
    let mut seen = HashSet::new();
    let mut images = Vec::new();
    while let Some(id) = stack.pop() {
        if seen.len() == MAX_NODES || !seen.insert(id) {
            return Report { limited: true, ..Default::default() };
        }
        let tile = ui.tile(id);
        if !tile.live { continue; }
        if tile.children.len() > MAX_NODES.saturating_sub(stack.len()) {
            return Report { limited: true, ..Default::default() };
        }
        stack.extend(tile.children.iter().copied());
        // filewidth/fileheight describe an own file, not an atlas piece. Do not
        // reinterpret an atlas child's existing metadata or visible state.
        if tile.kind != "image" || tile.get("texatlas").is_some_and(|atlas| !atlas.text().trim().is_empty()) { continue; }
        if images.len() == MAX_IMAGES {
            return Report { limited: true, ..Default::default() };
        }
        let raw_filename = tile.get("filename").map(Value::text).unwrap_or_default();
        let normalized = raw_filename.trim().replace('/', "\\").to_ascii_lowercase();
        let is_empty = normalized.is_empty();
        let key = if is_empty { EMPTY_IMAGE_KEY.to_string() } else { normalized };
        let zoom = match tile.get("zoom") {
            None => 100.0,
            Some(Value::Num(n)) if n.is_finite() && *n > 0.0 => *n,
            _ => 0.0,
        };
        images.push((id, is_empty, key, zoom));
    }
    let wanted: HashSet<_> = images.iter().map(|(_, _, key, _)| key.as_str()).collect();
    state.dimensions.retain(|name, _| wanted.contains(name.as_str()));
    let mut report = Report { images: images.len(), ..Default::default() };
    for (id, is_empty, key, zoom) in images {
        if is_empty {
            ui.set_trait(id, "filename", Value::Str(EMPTY_IMAGE_FILENAME.into()));
        }
        let dimensions = *state.dimensions.entry(key.clone()).or_insert_with(|| {
            read(&key).filter(|(w, h)| [*w, *h].into_iter().all(|n| n.is_finite() && n > 0.0))
        });
        let (base, is_resolved) = match dimensions {
            Some(dim) => (dim, true),
            None if key == EMPTY_IMAGE_KEY => (EMPTY_TEXTURE_SIZE, true),
            None => (MISSING_TEXTURE_SIZE, false),
        };
        let scaled = (base.0 * zoom / 100.0, base.1 * zoom / 100.0);
        let finite = [scaled.0, scaled.1].into_iter().all(f64::is_finite);
        if is_resolved && finite { report.resolved += 1; } else { report.unavailable += 1; }
        let (w, h) = if finite { scaled } else { (0.0, 0.0) };
        for (name, value) in [("filewidth", w), ("fileheight", h)] {
            if ui.tile(id).get(name).is_none_or(|old| !matches!(old, Value::Num(n) if *n == value)) {
                ui.set_trait(id, name, Value::Num(value));
            }
        }
    }
    report
}

/// After scripts and before the existing expression update/late page settle. The
/// globals subtree is independent of the open page, and hidden images need metrics.
pub(super) fn update_globals(mut script: ResMut<ScriptState>, data: Option<Res<nvf_assets::GameData>>, mut fonts: FontKit, mut state: Local<Metrics>, mut told: Local<Option<Report>>) {
    let Some(rt) = script.rt.as_mut() else { return };
    if rt.game.ui_files.is_none() { return; }
    // Missing renderer data is not a decoded missing asset. Do not cache that
    // pre-admission state; a later real resource must be able to resolve metrics.
    let Some(data)=data else { return };
    if data.is_changed() { state.dimensions.clear(); }
    let report = write(&mut rt.game.ui_tree,&mut state,&mut |filename| {
        fonts.picture_size(filename).map(|size| (f64::from(size.x),f64::from(size.y)))
    });
    if *told != Some(report) {
        if report.images > 0 || report.limited {
            info!("ui image metrics: globals images={} resolved={} unavailable={} limited={}",report.images,report.resolved,report.unavailable,report.limited);
        }
        *told = Some(report);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tiles::draw::{Fit, Look, Placed};
    use fnv_ui::MemSource;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64,Ordering};

    fn ui() -> Ui {
        let source = MemSource::default()
            .with("menus\\globals.xml",r#"<rect name="Globals"><image name="Calibration"><visible>0</visible><filename>calibration.dds</filename></image><_Density><copy>12800</copy><div src="child(Calibration)" trait="fileheight"/></_Density></rect>"#)
            .with("menu.xml",r#"<menu name="Owned"><image name="Body"><filename>body.dds</filename><width>185.6</width><height>185.6</height><zoom><copy src="globals()" trait="_Density"/><mul>1.45</mul></zoom></image><image name="Other"><filename>unrelated.dds</filename><fileheight>77</fileheight></image></menu>"#);
        let mut ui=Ui::new();assert!(ui.load_globals(&source));ui.load_menu(&source,"menu.xml").unwrap();ui.update();ui
    }
    fn calibration(ui:&Ui)->TileId {ui.tile(ui.globals()).children.iter().copied().find(|&id|ui.tile(id).name=="Calibration").unwrap()}
    fn number(ui:&Ui,id:TileId,name:&str)->f64 {ui.tile(id).get(name).map_or(0.0,Value::num)}

    #[test]
    fn hidden_global_metrics_recompute_the_real_dependent_child_piece_geometry() {
        // Original source behavior is observed before invoking the new writer: Div0
        // preserves its accumulator, yielding huge zoom rather than invented zero.
        let mut ui=ui();let body=ui.find_tile("Owned/Body").unwrap();assert_eq!(number(&ui,body,"zoom"),18560.0);
        let mut state=Metrics::default();let report=write(&mut ui,&mut state,&mut |_|Some((512.,512.)));ui.update();
        assert_eq!(report,Report{images:1,resolved:1,..Default::default()});
        assert_eq!(number(&ui,calibration(&ui),"fileheight"),512.);assert_eq!(number(&ui,body,"zoom"),36.25);
        let mut fonts=Fonts;let pieces=super::super::draw::whole_pieces(&ui,ui.menu("Owned").unwrap(),&mut fonts,true);
        let piece=pieces.iter().find(|piece|piece.tile==body).unwrap();assert_eq!(piece.rect.width,185.6);assert_eq!(piece.rect.height,185.6);
        let Look::Picture{fit,..}=&piece.look else {panic!("actual child picture")};assert_eq!(*fit,Fit::Zoom(36.25));
        let Placed::At{width,height}=super::super::draw::placed(*fit,false,(512.,512.))else{panic!("actual child placement")};
        assert!((width-185.6).abs()<1e-9 && (height-185.6).abs()<1e-9);
        assert_eq!(ui.get_float("Owned/Other/fileheight"),Some(77.),"non-global images are not rewritten");
    }
    struct Fonts;
    impl super::super::draw::MenuFonts for Fonts {
        fn loaded(&self,_:u8)->bool{true}fn load_font(&mut self,_:u8){}
        fn size(&self,_:f64,_:&str,_:Option<f64>)->(f64,f64){(0.,0.)}
    }

    #[test]
    fn filename_changes_use_missing_and_empty_fallbacks_and_recover_without_stale_size_or_reads() {
        let mut ui = ui();
        let id = calibration(&ui);
        let mut state = Metrics::default();
        let reads = std::cell::Cell::new(0);
        let mut read = |name: &str| {
            reads.set(reads.get() + 1);
            match name {
                "calibration.dds" => Some((512., 512.)),
                "other.dds" => Some((256., 128.)),
                _ => None,
            }
        };
        write(&mut ui, &mut state, &mut read);
        write(&mut ui, &mut state, &mut read);
        assert_eq!((number(&ui, id, "filewidth"), number(&ui, id, "fileheight")), (512., 512.));
        // Missing DDS resolves to 256x256 (neither 0 nor stale 512x512).
        ui.set_trait(id, "filename", Value::Str("missing.dds".into()));
        let report = write(&mut ui, &mut state, &mut read);
        assert_eq!(report.unavailable, 1);
        assert_eq!((number(&ui, id, "filewidth"), number(&ui, id, "fileheight")), (256., 256.));
        // Restoring a valid DDS updates back to its real dimensions.
        ui.set_trait(id, "filename", Value::Str("other.dds".into()));
        write(&mut ui, &mut state, &mut read);
        assert_eq!((number(&ui, id, "filewidth"), number(&ui, id, "fileheight")), (256., 128.));
        // Empty filename substitutes Interface\Shared\empty.dds at 1024x1024 and caches cleanly.
        ui.set_trait(id, "filename", Value::Str(String::new()));
        let report = write(&mut ui, &mut state, &mut read);
        assert_eq!(report.resolved, 1);
        assert_eq!(ui.tile(id).get("filename").map(Value::text).as_deref(), Some(EMPTY_IMAGE_FILENAME));
        assert_eq!((number(&ui, id, "filewidth"), number(&ui, id, "fileheight")), (1024., 1024.));
        assert_eq!(reads.get(), 4);
        // Subsequent pass with no filename change makes zero additional reads or trait writes.
        let counts = ui.trait_write_counts();
        write(&mut ui, &mut state, &mut read);
        assert_eq!(reads.get(), 4);
        assert_eq!(ui.trait_write_counts(), counts);
    }
    #[test]
    fn zoom_changes_are_live_finite_and_do_not_read_a_cached_file_again() {
        let mut ui=ui();let id=calibration(&ui);let mut state=Metrics::default();let reads=std::cell::Cell::new(0);
        let mut read=|_:&str|{reads.set(reads.get()+1);Some((512.,256.))};
        for (zoom,expected) in [(150.,384.),(50.,128.),(0.,0.),(-1.,0.),(f64::NAN,0.),(f64::INFINITY,0.),(f64::MAX,0.)] {
            ui.set_trait(id,"zoom",Value::Num(zoom));write(&mut ui,&mut state,&mut read);assert_eq!(number(&ui,id,"fileheight"),expected);
        }
        ui.set_trait(id,"zoom",Value::Sym("scale".into()));write(&mut ui,&mut state,&mut read);assert_eq!(number(&ui,id,"fileheight"),0.);assert_eq!(reads.get(),1);
    }
    #[test]
    fn replacing_the_ui_tree_rebinds_real_global_identity_and_discards_cached_dimensions() {
        let mut old=ui();let before=calibration(&old);let mut state=Metrics::default();write(&mut old,&mut state,&mut |_|Some((128.,128.)));
        let mut fresh=ui();assert_ne!(before,calibration(&fresh));write(&mut fresh,&mut state,&mut |_|Some((512.,512.)));
        assert_eq!(number(&fresh,calibration(&fresh),"fileheight"),512.);assert_eq!(number(&old,before,"fileheight"),128.);
        let counts=fresh.trait_write_counts();write(&mut fresh,&mut state,&mut |_|panic!("cached dimensions"));assert_eq!(fresh.trait_write_counts(),counts,"unchanged metadata makes no writes");
    }
    #[test]
    fn bounded_global_admission_is_atomic_and_atlas_or_ordinary_neighbors_are_untouched() {
        let mut ui=ui();let id=calibration(&ui);
        for empty in ["", "  ", "\t"] {
            ui.set_trait(id,"texatlas",Value::Str(empty.into()));
            let report=write(&mut ui,&mut Metrics::default(),&mut |_|Some((512.,512.)));
            assert_eq!(report.resolved,1);assert_eq!(number(&ui,id,"fileheight"),512.,"empty atlas is an own image in the actual painter");
        }
        ui.set_trait(id,"texatlas",Value::Str("shared.tai".into()));ui.set_trait(id,"fileheight",Value::Num(7.));
        let report=write(&mut ui,&mut Metrics::default(),&mut |_|panic!("atlas is not an own file"));assert_eq!(report.images,0);assert_eq!(number(&ui,id,"fileheight"),7.);
        let source=MemSource::default().with("menus\\globals.xml",&format!("<rect name=\"Globals\">{}</rect>",(0..MAX_IMAGES+1).map(|i|format!("<image name=\"I{i}\"><filename>x.dds</filename></image>")).collect::<String>()));
        let mut ui=Ui::new();assert!(ui.load_globals(&source));let counts=ui.trait_write_counts();let report=write(&mut ui,&mut Metrics::default(),&mut |_|panic!("reject before reads"));assert!(report.limited);assert_eq!(ui.trait_write_counts(),counts);
    }

    struct Files {root:PathBuf,file:PathBuf}
    impl Files {
        fn new()->Self {
            static NEXT:AtomicU64=AtomicU64::new(0);let root=std::env::temp_dir().join(format!("nvf-own-global-metrics-{}-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
            std::fs::create_dir(&root).unwrap();std::fs::create_dir(root.join("textures")).unwrap();let file=root.join("textures/calibration.dds");
            let mut bytes=vec![0;128];bytes[..4].copy_from_slice(b"DDS ");for(at,value)in[(4,124u32),(8,0x1007),(12,512),(16,512),(28,1),(76,32),(80,4),(108,0x1000)]{bytes[at..at+4].copy_from_slice(&value.to_le_bytes());}bytes[84..88].copy_from_slice(b"DXT1");
            for _ in 0..128*128 {bytes.extend([0,248,0,0,0,0,0,0]);}std::fs::write(&file,bytes).unwrap();Self{root,file}
        }
        fn data(&self)->nvf_assets::GameData {
            let vfs=fnv_vfs::Vfs::open(&self.root,&[]).unwrap();let schema=fnv_nif::Schema::load_dir(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/nif")).unwrap();
            nvf_assets::GameData::new(Arc::new(vfs),Arc::new(schema))
        }
    }
    impl Drop for Files {fn drop(&mut self){let _=std::fs::remove_file(&self.file);let _=std::fs::remove_dir(self.root.join("textures"));let _=std::fs::remove_dir(&self.root);}}
    #[test]
    fn actual_scheduled_fontkit_vfs_image_metrics_reach_the_ui_before_expression_settle() {
        let files=Files::new();let mut script=ScriptState::over_no_plugins();let game=&mut script.rt.as_mut().unwrap().game;game.ui_tree=ui();game.ui_files=Some(Box::new(MemSource::default()));
        let body=game.ui_tree.find_tile("Owned/Body").unwrap();assert_eq!(number(&game.ui_tree,body,"zoom"),18560.);
        fn settle(mut script:ResMut<ScriptState>){script.rt.as_mut().unwrap().game.ui_tree.update();}
        let mut app=App::new();app.init_resource::<Assets<Image>>().init_resource::<super::super::draw::TileFonts>().insert_resource(script).add_systems(Update,(update_globals,settle).chain());
        app.update();let ui=&app.world().resource::<ScriptState>().rt.as_ref().unwrap().game.ui_tree;
        assert!(ui.tile(calibration(ui)).get("fileheight").is_none(),"no data is not a cached missing texture");assert_eq!(number(ui,body,"zoom"),18560.);
        app.insert_resource(files.data());
        app.update();let ui=&app.world().resource::<ScriptState>().rt.as_ref().unwrap().game.ui_tree;
        assert_eq!(number(ui,calibration(ui),"fileheight"),512.);assert_eq!(number(ui,body,"zoom"),36.25);assert_eq!(ui.get_float("Owned/Other/fileheight"),Some(77.));
        let counts=ui.trait_write_counts();app.update();assert_eq!(app.world().resource::<ScriptState>().rt.as_ref().unwrap().game.ui_tree.trait_write_counts(),counts);
    }

    #[test]
    fn exact_global_node_budget_is_admitted_and_next_node_refuses_before_reads_or_writes() {
        fn bounded(total: usize) -> Ui {
            // The root and one hidden image each count as actual visited nodes.
            let mut xml = String::from("<rect name=\"Globals\"><image name=\"Metric\"><visible>0</visible><filename>x.dds</filename><fileheight>73</fileheight></image>");
            for i in 0..total - 2 { xml.push_str(&format!("<rect name=\"R{i}\"/>")); }
            xml.push_str("</rect>");
            let source = MemSource::default().with("menus\\globals.xml", &xml);
            let mut ui = Ui::new();
            assert!(ui.load_globals(&source));
            ui
        }
        let mut exact = bounded(MAX_NODES);
        let reads = std::cell::Cell::new(0);
        let report = write(&mut exact, &mut Metrics::default(), &mut |_| {
            reads.set(reads.get() + 1);
            Some((128.0, 64.0))
        });
        assert_eq!(report, Report { images: 1, resolved: 1, ..Default::default() });
        assert_eq!(reads.get(), 1);
        let id = exact.tile(exact.globals()).children[0];
        assert_eq!(number(&exact, id, "fileheight"), 64.0);
        let mut overflow = bounded(MAX_NODES + 1);
        let id = overflow.tile(overflow.globals()).children[0];
        assert_eq!(number(&overflow, id, "fileheight"), 73.0);
        let counts = overflow.trait_write_counts();
        let report = write(&mut overflow, &mut Metrics::default(), &mut |_| panic!("reject complete frontier before any decode"));
        assert!(report.limited);
        assert_eq!(overflow.trait_write_counts(), counts);
        assert_eq!(number(&overflow, id, "fileheight"), 73.0);
    }

    #[test]
    fn scheduled_runtime_handles_visible_hidden_missing_empty_and_setuistringex_transitions() {
        let files = Files::new();
        let source = MemSource::default().with(
            "menus\\globals.xml",
            r#"<rect name="Globals">
                <image name="Vis"><visible>1</visible><filename>calibration.dds</filename></image>
                <image name="Hid"><visible>0</visible><filename>calibration.dds</filename></image>
                <image name="InitMiss"><visible>1</visible><filename>interface\__w4g_missing__.dds</filename></image>
                <image name="InitEmp"><visible>1</visible><filename></filename></image>
            </rect>"#,
        );
        let mut ui = Ui::new();
        assert!(ui.load_globals(&source));
        ui.update();
        let child = |ui: &Ui, name: &str| ui.tile(ui.globals()).children.iter().copied().find(|&id| ui.tile(id).name == name).unwrap();

        let mut script = ScriptState::over_no_plugins();
        {
            let game = &mut script.rt.as_mut().unwrap().game;
            game.ui_tree = ui;
            game.ui_files = Some(Box::new(MemSource::default()));
        }
        fn settle(mut script: ResMut<ScriptState>) { script.rt.as_mut().unwrap().game.ui_tree.update(); }
        let mut app = App::new();
        app.init_resource::<Assets<Image>>()
            .init_resource::<super::super::draw::TileFonts>()
            .insert_resource(script)
            .insert_resource(files.data())
            .add_systems(Update, (update_globals, settle).chain());

        // Tick 1: valid visible & hidden populate 512x512 (M054), init missing -> 256x256,
        // init empty -> Interface\Shared\empty.dds at 1024x1024.
        app.update();
        {
            let ui = &app.world().resource::<ScriptState>().rt.as_ref().unwrap().game.ui_tree;
            for name in ["Vis", "Hid"] {
                let id = child(ui, name);
                assert_eq!((number(ui, id, "filewidth"), number(ui, id, "fileheight")), (512.0, 512.0), "{name}");
            }
            let init_miss = child(ui, "InitMiss");
            assert_eq!((number(ui, init_miss, "filewidth"), number(ui, init_miss, "fileheight")), (256.0, 256.0));
            let init_emp = child(ui, "InitEmp");
            assert_eq!(ui.tile(init_emp).get("filename").map(Value::text).as_deref(), Some(EMPTY_IMAGE_FILENAME));
            assert_eq!((number(ui, init_emp, "filewidth"), number(ui, init_emp, "fileheight")), (1024.0, 1024.0));
        }

        // Tick 2: mutate both Vis and Hid to missing DDS -> both update to 256x256 (not 0, not stale 512).
        {
            let mut script = app.world_mut().resource_mut::<ScriptState>();
            let ui = &mut script.rt.as_mut().unwrap().game.ui_tree;
            for name in ["Vis", "Hid"] {
                let id = child(ui, name);
                ui.set_trait(id, "filename", Value::Str("interface\\__w4g_missing__.dds".into()));
            }
        }
        app.update();
        {
            let ui = &app.world().resource::<ScriptState>().rt.as_ref().unwrap().game.ui_tree;
            for name in ["Vis", "Hid"] {
                let id = child(ui, name);
                assert_eq!((number(ui, id, "filewidth"), number(ui, id, "fileheight")), (256.0, 256.0), "{name} missing");
            }
        }

        // Tick 3: SetUIStringEx "" on Vis -> substitutes Interface\Shared\empty.dds at 1024x1024,
        // and restoring Hid to calibration.dds recovers 512x512.
        {
            let mut script = app.world_mut().resource_mut::<ScriptState>();
            let game = &mut script.rt.as_mut().unwrap().game;
            game.run_query(
                "Begin Function {}\nstring_var sEmp\nlet sEmp := \"\"\nSetUIStringEx \"globals()/Vis/filename\" \"%z\" sEmp\nSetUIString \"globals()/Hid/filename\" \"calibration.dds\"\nEnd\n",
                Default::default(),
            )
            .unwrap();
        }
        app.update();
        {
            let ui = &app.world().resource::<ScriptState>().rt.as_ref().unwrap().game.ui_tree;
            let vis = child(ui, "Vis");
            assert_eq!(ui.tile(vis).get("filename").map(Value::text).as_deref(), Some(EMPTY_IMAGE_FILENAME));
            assert_eq!((number(ui, vis, "filewidth"), number(ui, vis, "fileheight")), (1024.0, 1024.0));
            let hid = child(ui, "Hid");
            assert_eq!((number(ui, hid, "filewidth"), number(ui, hid, "fileheight")), (512.0, 512.0));
        }
    }
}
