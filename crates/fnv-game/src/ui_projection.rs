//! Narrow source-command state for projected UI, independent of unrelated plugin state.
//! [design: docs/spec-notes/held-ui-projection-state.md]
use fnv_sim::FormId;
use fnv_ui::{TileId, Ui};
use std::collections::BTreeMap;

const MAX_REQUESTS: usize = 16;
const MAX_PIXELS: u64 = 8 * 1024 * 1024;
const MAX_EDGE: f64 = 2048.0;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Target {
    pub reference: FormId,
    pub block: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Revision(u64);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub x: f64,
    pub y: f64,
    pub width: u32,
    pub height: u32,
}
impl Viewport {
    fn parse(rect: [f64; 4]) -> Option<Self> {
        let [x, y, width, height] = rect;
        if !rect.into_iter().all(f64::is_finite) || x.abs() > 10_000_000.0 || y.abs() > 10_000_000.0
            || width <= 0.0 || height <= 0.0 || width > MAX_EDGE || height > MAX_EDGE { return None; }
        Some(Self { x, y, width: width.ceil() as u32, height: height.ceil() as u32 })
    }
    fn pixels(self) -> u64 { u64::from(self.width) * u64::from(self.height) }
}

/// Command acceptance binds the source generation; it does not claim rendered pixels or
/// a matching live model target. Renderer admission and model ownership are separate.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub source: TileId,
    pub viewport: Viewport,
    pub revision: Revision,
    source_path: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejected { Target, TextBounds, Viewport, MissingSource, Limit, Exhausted }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Accepted { Registered(Revision), Withdrawn }

/// Transient UI state: default empty and cleared with the actual tree rebuild. Kept in
/// UiExtra so Game construction/snapshots need no new field or aggregate state import.
#[derive(Debug, Default)]
pub struct Requests {
    screen: Option<TileId>,
    last_revision: u64,
    entries: BTreeMap<Target, Request>,
}
impl Requests {
    pub fn clear(&mut self) { self.entries.clear(); self.screen = None; }

    /// Retire an observed stale source permanently. Restoring the path alone must not
    /// resurrect a request revision; a fresh explicit command is required. Called by
    /// the actual pre-draw consumer and before capacity admission [design].
    pub fn reconcile(&mut self, ui: &Ui) {
        if self.screen != Some(ui.screen()) {
            self.entries.clear(); self.screen = Some(ui.screen());
        }
        self.entries.retain(|_, request| ui.is_live(request.source)
            && ui.find_tile(&request.source_path) == Some(request.source));
    }

    pub fn set(&mut self, ui: &Ui, reference: FormId, source_path: &str, block: &str, rect: [f64; 4]) -> Result<Accepted, Rejected> {
        if reference.0 == 0 { return Err(Rejected::Target); }
        if source_path.trim().is_empty() || source_path.len() > 4096 || block.trim().is_empty() || block.len() > 512 { return Err(Rejected::TextBounds); }
        let viewport = Viewport::parse(rect).ok_or(Rejected::Viewport)?;
        let key = Target { reference, block: block.to_ascii_lowercase() };
        // Full UI identity prevents a new tree reusing a prior request's source slots.
        self.reconcile(ui);
        // This exact encoding is observed in the installed caller; other tiny rectangles
        // are not guessed to be cancellation. A missing source can still be withdrawn.
        if rect[2] == 1.0 && rect[3] == 1.0 { self.entries.remove(&key); return Ok(Accepted::Withdrawn); }
        let source = ui.find_tile(source_path).ok_or(Rejected::MissingSource)?;
        if !ui.is_live(source) { return Err(Rejected::MissingSource); }
        if let Some(old) = self.entries.get(&key) {
            if old.source == source && old.viewport == viewport && old.source_path.eq_ignore_ascii_case(source_path) {
                return Ok(Accepted::Registered(old.revision));
            }
        }
        if !self.entries.contains_key(&key) && self.entries.len() >= MAX_REQUESTS { return Err(Rejected::Limit); }
        let used = self.entries.iter().filter(|(target, _)| **target != key)
            .try_fold(0u64, |sum, (_, request)| sum.checked_add(request.viewport.pixels())).ok_or(Rejected::Limit)?;
        if used.checked_add(viewport.pixels()).is_none_or(|total| total > MAX_PIXELS) { return Err(Rejected::Limit); }
        let next = self.last_revision.checked_add(1).ok_or(Rejected::Exhausted)?;
        let revision = Revision(next);
        self.entries.insert(key, Request { source, viewport, revision, source_path: source_path.to_string() });
        self.last_revision = next;
        Ok(Accepted::Registered(revision))
    }

    /// Current source generations only. No hidden-ancestor exception is granted by merely
    /// inspecting the registration; the renderer must successfully admit its own target.
    pub fn source_requests<'a>(&'a self, ui: &'a Ui) -> impl Iterator<Item = (&'a Target, &'a Request)> + 'a {
        self.entries.iter().filter(move |(_, request)| self.screen == Some(ui.screen())
            && ui.is_live(request.source) && ui.find_tile(&request.source_path) == Some(request.source))
    }
}

/// Freshly source-compiled command routing only. No native opcode/parameter encoding is
/// claimed by this module. The calling reference is supplied by normal nvse::call.
pub(crate) fn call(game: &mut crate::Game<'_>, name: &str, subject: Option<FormId>, args: &[fnv_script::vm::XVal]) -> Option<Result<fnv_script::vm::XVal, String>> {
    use fnv_script::vm::XVal;
    if name != "projectuitile" { return None; }
    let [XVal::Str(source), XVal::Str(block), XVal::Num(x), XVal::Num(y), XVal::Num(width), XVal::Num(height)] = args else {
        return Some(Err("ProjectUITile requires two strings and four numbers".to_string()));
    };
    if let Some(reference) = subject {
        let _ = game.ui_extra.projections.set(&game.ui_tree, reference, source, block, [*x, *y, *width, *height]);
    }
    // JIP LN ProjectUITile returns 0.0 and continues script execution on valid,
    // withdrawal, missing-tile, invalid-viewport, and missing/null-reference calls
    // without enqueueing invalid requests [observed: w4g_10_project_ui_tile_errors].
    Some(Ok(XVal::Num(0.0)))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tree() -> Ui {
        let source = fnv_ui::MemSource::default().with("owned.xml", r#"<menu name="Owned"><template name="NewPanel"><rect name="Panel"/></template><rect name="Panel"><image name="Feature"><filename>nvf:feature</filename><width>80</width><height>40</height></image></rect></menu>"#);
        let mut ui = Ui::new();ui.load_menu(&source,"owned.xml").unwrap();ui.update();ui
    }
    fn rect() -> [f64; 4] { [-100.,-50.,128.,64.] }
    #[test]
    fn source_generation_and_other_reference_keys_are_independent() {
        let ui=tree();let mut requests=Requests::default();
        requests.set(&ui,FormId(10),"Owned/Panel","Screen",rect()).unwrap();
        requests.set(&ui,FormId(11),"Owned/Panel","Screen",rect()).unwrap();
        assert_eq!(requests.source_requests(&ui).count(),2);
        assert!(requests.source_requests(&ui).all(|(_,request)|request.source==ui.find_tile("Owned/Panel").unwrap()));
        assert_eq!(ui.get_float("Owned/Panel/Feature/width"),Some(80.));
    }
    #[test]
    fn repeated_request_reuses_revision_but_changed_viewport_gets_a_new_one() {
        let ui=tree();let mut requests=Requests::default();let a=requests.set(&ui,FormId(10),"Owned/Panel","Screen",rect()).unwrap();
        assert_eq!(requests.set(&ui,FormId(10),"Owned/Panel","screen",rect()).unwrap(),a);
        let b=requests.set(&ui,FormId(10),"Owned/Panel","screen",[0.,0.,128.,64.]).unwrap();assert_ne!(a,b);assert_eq!(requests.source_requests(&ui).count(),1);
    }
    #[test]
    fn exact_one_by_one_withdraws_without_requiring_a_current_source() {
        let ui=tree();let mut requests=Requests::default();requests.set(&ui,FormId(10),"Owned/Panel","Screen",rect()).unwrap();
        assert_eq!(requests.set(&ui,FormId(10),"missing","screen",[0.,0.,1.,1.]),Ok(Accepted::Withdrawn));assert_eq!(requests.source_requests(&ui).count(),0);
        requests.set(&ui,FormId(10),"Owned/Panel","screen",[0.,0.,0.5,0.5]).unwrap();assert_eq!(requests.source_requests(&ui).count(),1,"other small sizes are not cancellation");
    }
    #[test]
    fn reloaded_source_is_not_silently_retargeted_before_explicit_registration() {
        let mut ui=tree();let mut requests=Requests::default();requests.set(&ui,FormId(10),"Owned/Panel","Screen",rect()).unwrap();
        let old=requests.source_requests(&ui).next().unwrap().1.source;
        let src=fnv_ui::MemSource::default().with("owned.xml",r#"<menu name="Owned"><rect name="Panel"/></menu>"#);ui.load_menu(&src,"owned.xml").unwrap();ui.update();
        assert_ne!(old,ui.find_tile("Owned/Panel").unwrap());assert_eq!(requests.source_requests(&ui).count(),0);
        requests.set(&ui,FormId(10),"Owned/Panel","Screen",rect()).unwrap();assert_eq!(requests.source_requests(&ui).count(),1);
    }
    #[test]
    fn tree_identity_and_clear_remove_admission_without_resetting_revision_clock() {
        let ui=tree();let mut requests=Requests::default();let first=requests.set(&ui,FormId(10),"Owned/Panel","Screen",rect()).unwrap();let other=tree();
        assert_eq!(requests.source_requests(&other).count(),0);requests.clear();assert_eq!(requests.source_requests(&ui).count(),0);
        let second=requests.set(&other,FormId(10),"Owned/Panel","Screen",rect()).unwrap();assert_ne!(first,second);
    }
    #[test]
    fn invalid_inputs_leave_the_existing_request_unchanged() {
        let ui=tree();let mut requests=Requests::default();requests.set(&ui,FormId(10),"Owned/Panel","Screen",rect()).unwrap();let before=requests.entries.clone();
        for bad in [[f64::NAN,0.,32.,32.],[0.,f64::INFINITY,32.,32.],[0.,0.,0.,32.],[0.,0.,2049.,32.]] {assert_eq!(requests.set(&ui,FormId(10),"Owned/Panel","Screen",bad),Err(Rejected::Viewport));assert_eq!(requests.entries,before);}
        assert_eq!(requests.set(&ui,FormId(0),"Owned/Panel","Screen",rect()),Err(Rejected::Target));assert_eq!(requests.entries,before);
    }
    #[test]
    fn aggregate_pixels_and_request_count_are_checked_before_replacement() {
        let ui=tree();let mut requests=Requests::default();
        for id in 1..=16 {requests.set(&ui,FormId(id),"Owned/Panel","screen",[0.,0.,8.,8.]).unwrap();}
        assert_eq!(requests.set(&ui,FormId(17),"Owned/Panel","screen",[0.,0.,8.,8.]),Err(Rejected::Limit));
        requests.clear();for id in 1..=2 {requests.set(&ui,FormId(id),"Owned/Panel","screen",[0.,0.,2048.,2048.]).unwrap();}
        assert_eq!(requests.set(&ui,FormId(3),"Owned/Panel","screen",rect()),Err(Rejected::Limit));assert_eq!(requests.source_requests(&ui).count(),2);
    }
    #[test]
    fn exhausted_revision_never_reuses_an_old_token_or_changes_an_existing_request() {
        let ui=tree();let mut requests=Requests::default();let first=requests.set(&ui,FormId(10),"Owned/Panel","Screen",rect()).unwrap();let before=requests.entries.clone();requests.last_revision=u64::MAX;
        assert_eq!(requests.set(&ui,FormId(10),"Owned/Panel","screen",rect()).unwrap(),first);
        assert_eq!(requests.set(&ui,FormId(10),"Owned/Panel","screen",[0.,0.,64.,64.]),Err(Rejected::Exhausted));assert_eq!(requests.entries,before);
    }
    #[test]
    fn observed_indexed_path_shadow_is_sticky_until_explicit_registration() {
        let mut ui=tree();let mut requests=Requests::default();
        let original=ui.find_tile("Owned/Panel:0").unwrap();
        let first=requests.set(&ui,FormId(10),"Owned/Panel:0","screen",rect()).unwrap();
        let shadow=ui.add_from_template(&fnv_ui::MemSource::default(),"Owned","NewPanel",Some("Panel")).unwrap();ui.update();
        assert_ne!(original,shadow);assert_eq!(ui.find_tile("Owned/Panel:0"),Some(shadow));
        requests.reconcile(&ui);assert!(requests.entries.is_empty());
        ui.unload_tile(shadow);ui.update();assert_eq!(ui.find_tile("Owned/Panel:0"),Some(original));
        requests.reconcile(&ui);assert_eq!(requests.source_requests(&ui).count(),0);
        let second=requests.set(&ui,FormId(10),"Owned/Panel:0","screen",rect()).unwrap();
        assert_ne!(first,second);assert_eq!(requests.source_requests(&ui).count(),1);
    }
    #[test]
    fn admission_prunes_dead_sources_before_count_and_pixel_quotas() {
        let mut ui=tree();let mut requests=Requests::default();
        for id in 1..=16 {requests.set(&ui,FormId(id),"Owned/Panel","screen",[0.,0.,8.,8.]).unwrap();}
        let source=ui.find_tile("Owned/Panel").unwrap();ui.unload_tile(source);ui.update();
        let replacement=ui.add_from_template(&fnv_ui::MemSource::default(),"Owned","NewPanel",Some("Panel")).unwrap();ui.update();
        assert_ne!(source,replacement);
        requests.set(&ui,FormId(17),"Owned/Panel","screen",[0.,0.,2048.,2048.]).unwrap();
        assert_eq!(requests.entries.len(),1);
        requests.set(&ui,FormId(18),"Owned/Panel","screen",[0.,0.,2048.,2048.]).unwrap();
        assert_eq!(requests.entries.len(),2);
    }

}
