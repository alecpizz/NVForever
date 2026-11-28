//! SUP NVSE, batch 7 (#1008; docs/spec-notes/ext-sup7.md): the NIF block interpolations
//! and vertices, and the HUD bars' script-variable and UDF values. The interpolations run
//! here, frame by frame from the game time: each frame's translation, rotation or scale
//! goes to the engine as a `SceneEdit` (`crate::nif_blocks`), which holds it over the
//! animation as it holds JIP's NIF block setters'. The vertices come back from the
//! engine's drawn meshes (`SceneMirror::set_vertices`, `nvforever::nif_mirror`). The bars'
//! values are taken each fUpdateTimer seconds from the variable or the function
//! (`crate::ui_hudbar`; docs/spec-notes/hud-bars.md §1).

use crate::game::{Game, InstKey, RunCtx};
use crate::nif_blocks::SceneEdit;
use fnv_script::vm::{ArrayKind, Value, XKey, XVal};
use fnv_sim::FormId;

fn num(args: &[XVal], k: usize, dflt: f64) -> f64 {
    match args.get(k) {
        Some(XVal::Num(x)) => *x,
        _ => dflt,
    }
}

fn text(args: &[XVal], k: usize) -> Option<String> {
    match args.get(k)? {
        XVal::Str(s) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

fn yes(b: bool) -> XVal {
    XVal::Num(f64::from(u8::from(b)))
}

/// What an interpolation changes: 0 the scale, 1 the rotation, 2 the translation
/// [documented: NifBlockInterpolateStop, "type 1" stops the rotation interpolation;
/// observed: sup7 scale.start and event.read, a scale is type 0 (type 2 does not find
/// it, the finish event says 0); a rotation found by type 1, a translation not by 0;
/// hypothesis: the translation 2, the one left].
pub const SCALE: u8 = 0;
pub const ROTATION: u8 = 1;
pub const TRANSLATION: u8 = 2;

/// One running interpolation.
#[derive(Debug, Clone, PartialEq)]
pub struct Interp {
    pub r: FormId,
    pub node: String,
    pub kind: u8,
    pub start: [f32; 3],
    pub end: [f32; 3],
    /// Seconds one way.
    pub timer: f64,
    /// 0 start to end once; 1 start to end and back once; 2 there and back again and
    /// again; 3 start to end again and again [hypothesis: the modes' category page is
    /// not in the export; the pages' examples: mode 0 "from start to end in 2 seconds",
    /// 1 "from start to end and to start again", 2 "Infinite amount of times … and then
    /// back"; 3 the remaining case].
    pub mode: u8,
    /// The first-person model (the player only). NVForever's mirror has one tree a
    /// reference, the drawn one, so both change it [decision: kept apart for the
    /// searches and the events].
    pub first_person: bool,
    /// Game time it started.
    pub since: f64,
    /// Trait 0: the start value is set again when it ends [documented:
    /// NifBlockInterpolateSetTrait].
    pub reset_at_end: bool,
    /// The loops finished, for the loop events.
    pub loops: u64,
}

impl Interp {
    /// The value at `t` seconds since the start, and whether it has ended.
    pub fn at(&self, t: f64) -> ([f32; 3], bool) {
        let d = self.timer.max(1e-6);
        let lerp = |f: f64| -> [f32; 3] {
            let f = f.clamp(0.0, 1.0) as f32;
            std::array::from_fn(|i| self.start[i] + (self.end[i] - self.start[i]) * f)
        };
        match self.mode {
            1 => {
                if t >= 2.0 * d {
                    (self.start, true)
                } else if t >= d {
                    (lerp(2.0 - t / d), false)
                } else {
                    (lerp(t / d), false)
                }
            }
            2 => {
                let p = (t / (2.0 * d)).fract() * 2.0;
                (lerp(if p <= 1.0 { p } else { 2.0 - p }), false)
            }
            3 => (lerp((t / d).fract()), false),
            _ => (lerp(t / d), t >= d),
        }
    }

    /// How many whole loops `t` covers (modes 2 and 3).
    pub fn loops_at(&self, t: f64) -> u64 {
        let d = self.timer.max(1e-6) * if self.mode == 2 { 2.0 } else { 1.0 };
        (t / d).floor().max(0.0) as u64
    }

    fn edit(&self, v: [f32; 3]) -> SceneEdit {
        let (r, node) = (self.r, self.node.clone());
        match self.kind {
            TRANSLATION => SceneEdit::Translation { r, node, v },
            ROTATION => SceneEdit::Rotation { r, node, v },
            _ => SceneEdit::Scale { r, node, v: v[0] },
        }
    }
}

/// A variable a script handed over by place (HudBarSetValueScriptVar), read later.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VarPlace {
    Global(FormId),
    Var(InstKey, u16),
}

/// Where a HUD bar takes its value from.
#[derive(Debug, Clone, PartialEq)]
pub enum BarSource {
    Var(usize),
    Udf { func: XVal, caller: Option<FormId> },
}

/// A bar's value source: the bar's id, the source, iCalculateValueMax and the game time
/// it is next read.
#[derive(Debug, Clone, PartialEq)]
pub struct BarFeed {
    pub bar: u32,
    pub src: BarSource,
    pub of_max: bool,
    pub next: f64,
}

#[derive(Debug, Clone, Default)]
pub struct Sup7State {
    pub interps: Vec<Interp>,
    /// SetSUPOnInterpolateFinishEvent's handlers and whether each wants the loops' ends.
    pub handlers: Vec<(XVal, bool)>,
    /// The variables handed over by place (`Host::var_place`), by number.
    pub places: Vec<VarPlace>,
    pub feeds: Vec<BarFeed>,
}

impl Game<'_> {
    /// A variable's place for HudBarSetValueScriptVar: its number in [`Sup7State::places`].
    pub(crate) fn add_var_place(&mut self, p: VarPlace) -> u32 {
        if let Some(i) = self.sup7.places.iter().position(|q| *q == p) {
            return i as u32;
        }
        self.sup7.places.push(p);
        (self.sup7.places.len() - 1) as u32
    }

    fn place_value(&self, i: usize) -> Option<f64> {
        Some(match *self.sup7.places.get(i)? {
            VarPlace::Global(g) => f64::from(self.world.global(g).ok()?),
            VarPlace::Var(k, slot) => self.var(k, slot).num(),
        })
    }

    /// Each frame (`Game::tick`): the interpolations' values to the engine, their ends
    /// and loops told to the handlers; the bars' values from their sources.
    pub(crate) fn sup7_tick(&mut self) {
        if !self.sup7.interps.is_empty() {
            let now = self.time;
            let mut events = Vec::new();
            let mut kept = Vec::new();
            for mut it in std::mem::take(&mut self.sup7.interps) {
                let t = now - it.since;
                let (v, ended) = it.at(t);
                if ended {
                    let v = if it.reset_at_end { it.start } else { v };
                    self.scene.push_edit(it.edit(v));
                    events.push((it.r, it.node.clone(), it.kind, false, it.first_person));
                    continue;
                }
                self.scene.push_edit(it.edit(v));
                if it.mode >= 2 {
                    let n = it.loops_at(t);
                    if n > it.loops {
                        it.loops = n;
                        events.push((it.r, it.node.clone(), it.kind, true, it.first_person));
                    }
                }
                kept.push(it);
            }
            // Interpolations a handler starts come after the ones kept [documented:
            // SetSUPOnInterpolateFinishEvent, a new interpolation may be set in the
            // event].
            self.sup7.interps = kept;
            // The last argument, iPCNode, is 1 on the player's third-person model
            // [observed: sup7 event.read]; read as "the player's" [hypothesis: the name;
            // the first-person model and other references not probed].
            for (r, node, kind, from_loop, _pc) in events {
                let handlers: Vec<XVal> = self.sup7.handlers.iter().filter(|(_, loops)| !from_loop || *loops).map(|(h, _)| h.clone()).collect();
                for h in handlers {
                    let args = vec![XVal::Ref(r.0), XVal::Str(node.clone()), XVal::Num(f64::from(kind)), yes(from_loop), yes(r == FormId::PLAYER_REF)];
                    let ctx = RunCtx { self_ref: Some(r), ..RunCtx::default() };
                    let _ = self.run_function(&h, Some(Value::Ref(r.0)), args, None, ctx);
                }
            }
        }
        if !self.sup7.feeds.is_empty() {
            // A bar destroyed takes its source with it.
            let ids: Vec<u32> = self.ui_extra.bars.list.iter().map(|b| b.id).collect();
            self.sup7.feeds.retain(|s| ids.contains(&s.bar));
            let now = self.time;
            let due: Vec<(usize, BarFeed)> = self.sup7.feeds.iter().enumerate().filter(|(_, s)| now >= s.next).map(|(k, s)| (k, s.clone())).collect();
            for (k, feed) in due {
                let Some(i) = self.ui_extra.bars.list.iter().position(|b| b.id == feed.bar) else { continue };
                // A bar hidden by HudBarShowBar is not read [documented: HudBarShowBar,
                // "Bar value calculations will stop processing"]; shown again, it is read
                // in that frame (its time is past). A bar its conditions hide is still
                // read [hypothesis: the page names HudBarShowBar only].
                if !self.ui_extra.bars.list[i].shown {
                    continue;
                }
                let every = self.ui_extra.bars.list[i].bar_trait(2).max(0.0);
                if let Some(s) = self.sup7.feeds.get_mut(k) {
                    s.next = now + every;
                }
                let v = match feed.src {
                    BarSource::Var(p) => self.place_value(p),
                    BarSource::Udf { func, caller } => {
                        let ctx = RunCtx { self_ref: caller, ..RunCtx::default() };
                        match self.run_function(&func, caller.map(|c| Value::Ref(c.0)), Vec::new(), None, ctx) {
                            Ok(XVal::Num(x)) => Some(x),
                            _ => None,
                        }
                    }
                };
                // The function may have destroyed bars.
                let Some(i) = self.ui_extra.bars.list.iter().position(|b| b.id == feed.bar) else { continue };
                if let Some(v) = v.filter(|v| v.is_finite()) {
                    crate::ui_hudbar::set_value(self, i, v, feed.of_max);
                }
            }
        }
    }
}

/// The matching interpolations: the reference, the block when named, the kind unless
/// -1 or none, the first-person model when given [documented: NifBlockInterpolateStop,
/// "if type -1 specified - it will be ignored"].
fn matches(it: &Interp, r: FormId, block: Option<&str>, kind: Option<f64>, pc: Option<f64>) -> bool {
    it.r == r
        && block.is_none_or(|b| it.node.eq_ignore_ascii_case(b))
        && kind.is_none_or(|k| k < 0.0 || f64::from(it.kind) == k)
        && pc.is_none_or(|p| it.first_person == (p != 0.0))
}

/// Whether the reference's model file names the block, for a reference whose drawn tree
/// is not known (not drawn yet, or no engine): an actor's skeleton is its base's model.
fn model_names(g: &Game, r: FormId, node: &str) -> bool {
    crate::nif_blocks::model_of(g, r).and_then(|m| crate::mod_files::read_data(&format!("meshes\\{m}"))).is_some_and(|b| crate::mod_files::nif_has_name(&b, node))
}

/// `[block] [type] [pc]` as the searches take them: a named block without pc is the
/// third-person model's [documented: the pages, "third person by default"]; no block,
/// every one [documented: "Will stop all interpolations on Player"].
fn search(args: &[XVal], at: usize) -> (Option<String>, Option<f64>, Option<f64>) {
    let block = text(args, at);
    let kind = args.get(at + 1).map(|_| num(args, at + 1, -1.0));
    let pc = match args.get(at + 2) {
        Some(_) => Some(num(args, at + 2, 0.0)),
        None if block.is_some() => Some(0.0),
        None => None,
    };
    (block, kind, pc)
}

pub(crate) fn call(g: &mut Game, ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let zero = XVal::Num(0.0);
    let on = subject.or(ctx.self_ref);
    let v = match name {
        // `block start… end… timer mode [pc] [calcStart]` (the translation: `… mode
        // world pc calcStart`): from start to end in timer seconds by mode [documented:
        // the pages]; calcStart takes the block's value now as the start [documented:
        // "Scale N will be calculated automatically"]. 1 when it runs, 0 without a
        // reference or a block name (see docs/spec-notes/ext-sup7.md §1). The translation's world flag is not
        // used [decision: the mirror holds and sets values in the parent's space].
        "nifblockinterpolatetranslation" | "nifblockinterpolaterotation" | "nifblockinterpolatescale" => {
            let (Some(r), Some(node)) = (on, text(args, 0)) else { return Some(Ok(zero)) };
            let (kind, n) = match name {
                "nifblockinterpolatetranslation" => (TRANSLATION, 3),
                "nifblockinterpolaterotation" => (ROTATION, 3),
                _ => (SCALE, 1),
            };
            let at = |k: usize| num(args, k, 0.0) as f32;
            let triple = |o: usize| -> [f32; 3] { if n == 3 { [at(o), at(o + 1), at(o + 2)] } else { [at(o); 3] } };
            let mut start = triple(1);
            let end = triple(1 + n);
            let rest = 1 + 2 * n;
            let timer = num(args, rest, 0.0).max(0.0);
            let mode = num(args, rest + 1, 0.0).clamp(0.0, 3.0) as u8;
            let extra = usize::from(kind == TRANSLATION);
            let pc = num(args, rest + 2 + extra, 0.0) != 0.0;
            if num(args, rest + 3 + extra, 0.0) != 0.0 {
                if let Some(nd) = g.scene.node_now(r, &node) {
                    start = match kind {
                        TRANSLATION => nd.translation,
                        ROTATION => nd.rotation,
                        _ => [nd.scale; 3],
                    };
                }
            }
            // A new one of the same type on the block replaces the old [hypothesis: the
            // pages allow "many types of interpolations at the same time", read as one
            // of each type].
            g.sup7.interps.retain(|it| !(it.r == r && it.kind == kind && it.first_person == pc && it.node.eq_ignore_ascii_case(&node)));
            g.sup7.interps.push(Interp { r, node, kind, start, end, timer, mode, first_person: pc, since: g.time, reset_at_end: false, loops: 0 });
            yes(true)
        }
        // `[block] [type] [pc]` [documented: NifBlockInterpolateStop, IsNifBlockInterpolated].
        // Is: 1 when one matches; without a block 0 [observed: sup7 scale.start and
        // event.wait, 0 with a scale running, against the page's example]. Stop: 0 when
        // no arguments or when the reference has zero active interpolations [observed:
        // sup7 types.translation; Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + SUP NVSE
        // 855, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters,
        // `w4d_36_sup_random_keymap_and_dialog` (`tools/capture/nvse/catalogs/wave4d_extenders.json`,
        // #3679): `nifStop(0arg=0,miss=0)`; no DLL or source consulted]; 1 when one
        // matched [observed: `sup_23_nif_interp_stop_and_type`], -1 when the reference
        // has active interpolations but none match [observed: sup7 scale.next;
        // `sup_23_nif_interp_stop_and_type`]. A stopped block keeps the value it had
        // (see docs/spec-notes/ext-sup7.md §1).
        "nifblockinterpolatestop" | "isnifblockinterpolated" => {
            let Some(r) = on else { return Some(Ok(zero)) };
            let (block, kind, pc) = search(args, 0);
            let stop = name == "nifblockinterpolatestop";
            if block.is_none() {
                return Some(Ok(zero));
            }
            let hit = |it: &Interp| matches(it, r, block.as_deref(), kind, pc);
            let any = g.sup7.interps.iter().any(hit);
            if !stop {
                return Some(Ok(yes(any)));
            }
            if !g.sup7.interps.iter().any(|it| it.r == r) {
                return Some(Ok(zero));
            }
            g.sup7.interps.retain(|it| !hit(it));
            XVal::Num(if any { 1.0 } else { -1.0 })
        }
        // `trait value [block] [type] [pc]`: trait 0 sets the start value again when it
        // ends [documented: NifBlockInterpolateSetTrait]. 0 [observed: sup7 trait, 0 on a
        // running one named by its block, with none and for trait 5; hypothesis: the
        // trait set all the same].
        "nifblockinterpolatesettrait" => {
            let Some(r) = on else { return Some(Ok(zero)) };
            let (t, v) = (num(args, 0, -1.0), num(args, 1, 0.0));
            if t != 0.0 {
                return Some(Ok(zero));
            }
            let (block, kind, pc) = search(args, 2);
            for it in g.sup7.interps.iter_mut().filter(|it| matches(it, r, block.as_deref(), kind, pc)) {
                it.reset_at_end = v != 0.0;
            }
            zero
        }
        // `udf [loops]` [documented: SetSUPOnInterpolateFinishEvent]; the same handler
        // again changes its loops flag (see docs/spec-notes/ext-sup7.md §1). Every reference's interpolations
        // [documented: "when an interpolation on object will end", the handler given the
        // object].
        "setsuponinterpolatefinishevent" => {
            let Some(h) = args.first().filter(|h| matches!(h, XVal::Ref(f) if *f != 0)).cloned() else { return Some(Ok(zero)) };
            let loops = num(args, 1, 0.0) != 0.0;
            match g.sup7.handlers.iter_mut().find(|(x, _)| *x == h) {
                Some(e) => e.1 = loops,
                None => g.sup7.handlers.push((h, loops)),
            }
            zero
        }
        "removesuponinterpolatefinishevent" => {
            if let Some(h) = args.first() {
                g.sup7.handlers.retain(|(x, _)| x != h);
            }
            zero
        }
        // `ref.… block [pc]`: the block's vertices as the engine draws them, in the
        // block's own space [documented: NifBlockGetVerticles, "3 arrays, each containing
        // X,Y,Z values"; GetNifBlockNumVerticles]. Asked once, they come with the next
        // frame, as the JIP block functions' trees do. A block with no vertices: -2, a
        // missing block -1, both an empty array [observed: sup7 verts, "Bip01 Head" and
        // a made-up name]; told none yet: -2 for a block in the tree, -1 otherwise
        // [hypothesis]. pc is not read: the mirror has the drawn model only.
        "getnifblocknumverticles" | "nifblockgetverticles" => {
            let (Some(r), Some(node)) = (on, text(args, 0)) else {
                return Some(Ok(if name == "getnifblocknumverticles" { XVal::Num(-1.0) } else { XVal::Array(g.ext.new_array(ArrayKind::List, Vec::new())) }));
            };
            let verts = g.scene.vertices(r, &node);
            if name == "getnifblocknumverticles" {
                match verts.filter(|v| !v.is_empty()) {
                    Some(v) => XVal::Num(v.len() as f64),
                    None if g.scene.node_now(r, &node).is_some() || model_names(g, r, &node) => XVal::Num(-2.0),
                    None => XVal::Num(-1.0),
                }
            } else {
                let verts = verts.unwrap_or_default();
                let mut entries = Vec::new();
                if !verts.is_empty() {
                    for a in 0..3 {
                        let axis = verts.iter().enumerate().map(|(i, v)| (XKey::Num(i as f64), XVal::Num(f64::from(v[a])))).collect();
                        entries.push((XKey::Num(a as f64), XVal::Array(g.ext.new_array(ArrayKind::List, axis))));
                    }
                }
                XVal::Array(g.ext.new_array(ArrayKind::List, entries))
            }
        }
        // `bar variable calcMax` [documented: HudBarSetValueScriptVar]: the variable as
        // the VM hands it over, by place (`Host::var_place`, -1 - its number); anything
        // else is refused, -1 as the other value setters [hypothesis]. `bar udf [calcMax]
        // [caller]` [documented: HudBarSetValueUDF], the UDF called on the caller when
        // given. Read each fUpdateTimer seconds (bar trait 2) [documented: "Value will be
        // updated each instance of Bar update"], the first time in this frame's tick; a
        // new source replaces the bar's old one [hypothesis]. 1 when set.
        "hudbarsetvaluescriptvar" | "hudbarsetvalueudf" => {
            let fail = XVal::Num(-1.0);
            let Some(bar) = text(args, 0).and_then(|n| crate::ui_hudbar::bar_id(g, &n)) else { return Some(Ok(fail)) };
            let of_max = num(args, 2, 0.0) != 0.0;
            let src = if name == "hudbarsetvaluescriptvar" {
                let p = num(args, 1, 0.0);
                if p > -1.0 || p.fract() != 0.0 || (-(p + 1.0)) as usize >= g.sup7.places.len() {
                    return Some(Ok(fail));
                }
                BarSource::Var((-(p + 1.0)) as usize)
            } else {
                let Some(func) = args.get(1).filter(|f| matches!(f, XVal::Ref(r) if *r != 0)).cloned() else { return Some(Ok(fail)) };
                let caller = match args.get(3) {
                    Some(XVal::Ref(c)) if *c != 0 => Some(FormId(*c)),
                    _ => None,
                };
                BarSource::Udf { func, caller }
            };
            g.sup7.feeds.retain(|s| s.bar != bar);
            g.sup7.feeds.push(BarFeed { bar, src, of_max, next: g.time });
            XVal::Num(1.0)
        }
        _ => return None,
    };
    Some(Ok(v))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn it(mode: u8) -> Interp {
        Interp { r: FormId(1), node: "n".into(), kind: SCALE, start: [1.0; 3], end: [3.0; 3], timer: 2.0, mode, first_person: false, since: 0.0, reset_at_end: false, loops: 0 }
    }

    #[test]
    fn modes() {
        assert_eq!(it(0).at(1.0), ([2.0; 3], false));
        assert_eq!(it(0).at(2.5), ([3.0; 3], true));
        assert_eq!(it(1).at(3.0), ([2.0; 3], false));
        assert_eq!(it(1).at(4.0), ([1.0; 3], true));
        assert_eq!(it(2).at(3.0), ([2.0; 3], false));
        assert_eq!(it(2).at(6.0), ([3.0; 3], false));
        assert_eq!(it(3).at(3.0), ([2.0; 3], false));
        assert_eq!(it(2).loops_at(8.5), 2);
        assert_eq!(it(3).loops_at(8.5), 4);
    }
}
