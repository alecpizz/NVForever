//! CacheUI's functions (task T9, docs/spec-notes/menu-xml.md §9): tiles kept by number
//! (`CacheUI 20 "HUDMainMenu\SomeTile"`), then their traits read, set, changed over time
//! or revealed by that number, one at a time or a range; and its small queues. From the
//! GECK wiki pages of the CacheUI plugin, cited per function.
//!
//! Each mod has its own cache [documented: CacheUI, "Each mod use their own Cache and
//! can't be accessed by another mod"]: the plain functions use the calling script's
//! plugin's, the Alt ones a cache named by their first argument [documented: CacheUIAlt,
//! "use key rather then modindex"]. The page's exception for script runners is not
//! known further and not built. Entries are flushed when the tree is built again (a
//! load) [documented: "Cache will be flushed on gamereload"] and an entry whose tile was
//! unloaded is dropped when read [documented: "Menu specific entries will be flushed when
//! menu is not longer active"].

use crate::game::Game;
use crate::ui_functions::NOT_FOUND;
use crate::ui_gradients::Owner;
use crate::ui_more::{finite, finite_value, gradual, gradual_delay, num, opt, reveal, text, trait_name, yes};
use fnv_script::vm::XVal;
use fnv_ui::{TileId, Value};
use std::collections::{BTreeMap, HashMap};

/// The caches and queues.
#[derive(Default)]
pub struct Cache {
    /// (cache, index) to tile.
    entries: BTreeMap<(String, i64), TileId>,
    /// (owner, id) to values [hypothesis: each mod's queues are its own, as its cache].
    queues: HashMap<(String, i64), Vec<f64>>,
}

impl Cache {
    /// Forgets the cached tiles (the tree was built again); the queues hold no tiles and
    /// stay [hypothesis].
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// The tile cached at `index`, if it is still loaded.
    fn tile(&mut self, ui: &fnv_ui::Ui, key: &str, index: i64) -> Option<TileId> {
        let k = (key.to_string(), index);
        let t = *self.entries.get(&k)?;
        if ui.tile(t).live {
            Some(t)
        } else {
            self.entries.remove(&k);
            None
        }
    }

    /// The cached indices of a cache, in order.
    fn indices(&self, key: &str) -> Vec<i64> {
        self.entries.keys().filter(|(c, _)| c == key).map(|(_, i)| *i).collect()
    }

    /// The cached indices of a cache from `start` to `end` inclusive [documented:
    /// CacheUIRange, "Index 0 to 10"]: only the entries there, so a script's huge range
    /// costs no more than the cache holds [design].
    fn indices_in(&self, key: &str, start: f64, end: f64) -> Vec<i64> {
        let (s, e) = (start as i64, end as i64);
        self.indices(key).into_iter().filter(|i| (s..=e).contains(i)).collect()
    }
}

/// The calling script's own cache.
fn own(g: &Game) -> String {
    format!("mod:{}", g.other.plugin)
}

/// The tile at `index` and its trait `name` (a trait of it, or a path under it [design]),
/// when both are there.
fn target(g: &mut Game, key: &str, index: i64, name: &str) -> Option<(TileId, String)> {
    let tile = g.ui_extra.cache.tile(&g.ui_tree, key, index)?;
    let (t, k) = g.ui_tree.find_trait_in(tile, name)?;
    Some((t, trait_name(&g.ui_tree, t, k)))
}

/// A CacheUI function by lower-case name; `None` for any other.
/// A float or range write of a non-finite number, or a sum that is one, writes 0
/// [design: `ui_more::finite`].
pub(crate) fn call(g: &mut Game, lname: &str, args: &[XVal]) -> Option<Result<XVal, String>> {
    if lname.starts_with("cui") {
        return queue(g, lname, args).map(Ok);
    }
    if !lname.starts_with("cacheui") {
        return None;
    }
    // The Alt forms take the cache's name first [hypothesis: a name without case, as the
    // tree's names and the functions' own; the page says only "key"].
    let (base, key, a) = match lname.strip_suffix("alt") {
        Some(base) => (base, format!("key:{}", text(args, 0).to_ascii_lowercase()), args.get(1..).unwrap_or(&[])),
        None => (lname, own(g), args),
    };
    let zero = XVal::Num(0.0);
    let idx = |k: usize| num(a, k) as i64;
    Some(Ok(match base {
        // `index path` [documented: CacheUI]; 1 when the tile is there [design].
        "cacheui" => match g.ui_tree.find_tile(&text(a, 1)) {
            Some(t) => {
                g.ui_extra.cache.entries.insert((key, idx(0)), t);
                yes(true)
            }
            None => zero,
        },
        // `index trait value`, on a trait that is there [documented: CacheUIFloat;
        // hypothesis: as SetUIFloat, none is made].
        "cacheuifloat" | "cacheuistring" | "cacheuimodfloat" => match target(g, &key, idx(0), &text(a, 1)) {
            Some((t, name)) => {
                let v = match base {
                    "cacheuistring" => Value::Str(text(a, 2)),
                    // [documented: CacheUIMod, "CUIM 0 "alpha" 10 Add 10 to alpha"]
                    "cacheuimodfloat" => Value::Num(finite_value(g.ui_tree.tile(t).get(&name).map_or(0.0, Value::num) + finite(a, 2))),
                    _ => Value::Num(finite(a, 2)),
                };
                g.ui_tree.set_trait(t, &name, v);
                yes(true)
            }
            None => zero,
        },
        // [documented: CacheUIGetFloat, CacheUIGetString]; a trait not there reads -999
        // and "" as GetUIFloat and GetUIString do [hypothesis].
        "cacheuigetfloat" => XVal::Num(target(g, &key, idx(0), &text(a, 1)).map_or(NOT_FOUND, |(t, n)| g.ui_tree.tile(t).get(&n).map_or(NOT_FOUND, Value::num))),
        "cacheuigetstring" => XVal::Str(target(g, &key, idx(0), &text(a, 1)).map(|(t, n)| g.ui_tree.tile(t).get(&n).map(Value::text).unwrap_or_default()).unwrap_or_default()),
        // `trait start end value` [documented: CacheUIRange]. CacheUIRangeMod's page lists
        // `start end trait value` but its example writes `"alpha" 0 10 2`: both orders are
        // taken, by whether the first argument is text [design].
        "cacheuirange" | "cacheuirangemod" => {
            let (name, s, e, v) = match a.first() {
                Some(XVal::Str(_)) => (text(a, 0), num(a, 1), num(a, 2), finite(a, 3)),
                _ => (text(a, 2), num(a, 0), num(a, 1), finite(a, 3)),
            };
            let mut n = 0;
            for i in g.ui_extra.cache.indices_in(&key, s, e) {
                if let Some((t, tr)) = target(g, &key, i, &name) {
                    let v = if base == "cacheuirangemod" { finite_value(g.ui_tree.tile(t).get(&tr).map_or(0.0, Value::num) + v) } else { v };
                    g.ui_tree.set_trait(t, &tr, Value::Num(v));
                    n += 1;
                }
            }
            XVal::Num(f64::from(n))
        }
        // `start [end]`, -1 the whole cache [documented: CacheUIErase].
        "cacheuierase" => {
            let s = idx(0);
            let e = opt(a, 1).map_or(s, |e| e as i64);
            g.ui_extra.cache.entries.retain(|(c, i), _| *c != key || !(s == -1 || (s..=e).contains(i)));
            zero
        }
        // `index`, -1 the whole cache, printed to the console [documented: CacheUIDump];
        // one line an entry, its index and tile path [design].
        "cacheuidump" => {
            let which = idx(0);
            let indices = g.ui_extra.cache.indices(&key);
            let mut lines = Vec::new();
            for i in indices.into_iter().filter(|&i| which == -1 || i == which) {
                if let Some(t) = g.ui_extra.cache.tile(&g.ui_tree, &key, i) {
                    lines.push(format!("CacheUI [{i}] {}", g.ui_tree.path_of(t)));
                }
            }
            g.console_out.extend(lines);
            zero
        }
        // `index trait [start [end [duration [mode]]]]` [documented: CacheUIGradual].
        "cacheuigradual" => match target(g, &key, idx(0), &text(a, 1)) {
            Some((t, name)) => yes(gradual(g, Owner::CacheUi, t, &name, a.get(2..).unwrap_or(&[]))),
            None => zero,
        },
        // `start end trait …` [documented: CacheUIRangeGradual].
        "cacheuirangegradual" | "cacheuirangegradualdelay" => {
            let name = text(a, 2);
            let rest = a.get(3..).unwrap_or(&[]);
            let mut n = 0;
            for i in g.ui_extra.cache.indices_in(&key, num(a, 0), num(a, 1)) {
                if let Some((t, tr)) = target(g, &key, i, &name) {
                    let ok = if base == "cacheuirangegradual" { gradual(g, Owner::CacheUi, t, &tr, rest) } else { gradual_delay(g, t, &tr, rest) };
                    n += i32::from(ok);
                }
            }
            XVal::Num(f64::from(n))
        }
        // `index trait delay [end [duration [flag [mode [match]]]]]` [documented:
        // CacheUIGradualDelay].
        "cacheuigradualdelay" => match target(g, &key, idx(0), &text(a, 1)) {
            Some((t, name)) => yes(gradual_delay(g, t, &name, a.get(2..).unwrap_or(&[]))),
            None => zero,
        },
        // `id trait [text [time [mode [invert]]]]` [documented: CacheUIStringReveal].
        "cacheuistringreveal" => match target(g, &key, idx(0), &text(a, 1)) {
            Some((t, name)) => yes(reveal(g, t, &name, a.get(2..).unwrap_or(&[]))),
            None => zero,
        },
        _ => return None,
    }))
}

/// The queues [documented: CUICreateQueue, CUIGetQueue, CUIGetQueueSize, CUIQueueCycle,
/// CUIQueueFloat].
fn queue(g: &mut Game, lname: &str, args: &[XVal]) -> Option<XVal> {
    let key = (format!("mod:{}", g.other.plugin), num(args, 0) as i64);
    let queues = &mut g.ui_extra.cache.queues;
    Some(match lname {
        // An empty queue at the id [hypothesis: a queue already there is emptied].
        "cuicreatequeue" => {
            queues.insert(key, Vec::new());
            XVal::Num(0.0)
        }
        // -1 when the index is empty [documented: CUIGetQueue].
        "cuigetqueue" => {
            let k = num(args, 1);
            XVal::Num(queues.get(&key).filter(|_| k >= 0.0).and_then(|q| q.get(k as usize).copied()).unwrap_or(-1.0))
        }
        "cuigetqueuesize" => XVal::Num(queues.get(&key).map_or(0.0, |q| q.len() as f64)),
        // Direction 1: the first goes last, `[0, 25, 20, 9]` to `[25, 20, 9, 0]`
        // [documented: CUIQueueCycle's example]; 0 the last goes first (see docs/spec-notes/menu-xml.md §9).
        "cuiqueuecycle" => {
            if let Some(q) = queues.get_mut(&key).filter(|q| !q.is_empty()) {
                if num(args, 1) != 0.0 {
                    q.rotate_left(1);
                } else {
                    q.rotate_right(1);
                }
            }
            XVal::Num(0.0)
        }
        // `id value [duplicates [flag]]`: by default a value not there is added and one
        // there removed ("When called again, it will be removed"); flag 0 forces a removal
        // and 1 an add; with duplicates 1 a value "can not be added again" [documented:
        // CUIQueueFloat]. [hypothesis] A forced add without duplicates 1 may add a value
        // twice; removing takes its first copy; a queue not created takes nothing.
        "cuiqueuefloat" => {
            let v = num(args, 1);
            let no_dup = num(args, 2) != 0.0;
            let flag = opt(args, 3).map_or(-1, |f| f as i64);
            if let Some(q) = queues.get_mut(&key) {
                let at = q.iter().position(|x| *x == v);
                match (flag, at) {
                    (0, Some(i)) => {
                        q.remove(i);
                    }
                    (1, Some(_)) if no_dup => {}
                    (1, _) => q.push(v),
                    (-1, Some(i)) => {
                        q.remove(i);
                    }
                    (-1, None) => q.push(v),
                    _ => {}
                }
            }
            XVal::Num(0.0)
        }
        _ => return None,
    })
}
