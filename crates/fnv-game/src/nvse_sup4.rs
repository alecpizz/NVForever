//! SUP NVSE, batch 4 (#1008; docs/spec-notes/ext-sup4.md): the last 17 SUP functions.
//! Random array keys, invalid references in arrays, floats from an array, a UDF a number
//! of frames later, the compass targets, the last save's and load's names and sizes, the
//! cells and map markers near the player, scripts marked after a load, killing the
//! player's enemies, the caught-pickpocketing flag and Cyrillic to Latin letters
//! [documented: each page in the wiki export].

use crate::game::{Game, RunCtx};
use fnv_script::vm::{ArrayKind, XKey, XVal};
use fnv_sim::FormId;
use rand_core::RngCore;
use std::collections::BTreeSet;

fn form(args: &[XVal], k: usize) -> Option<FormId> {
    match args.get(k)? {
        XVal::Ref(0) => None,
        XVal::Ref(r) => Some(FormId(*r)),
        XVal::Num(x) if *x >= 1.0 && x.fract() == 0.0 => Some(FormId(*x as u32)),
        _ => None,
    }
}

fn num(args: &[XVal], k: usize, dflt: f64) -> f64 {
    match args.get(k) {
        Some(XVal::Num(x)) => *x,
        _ => dflt,
    }
}

fn list(g: &mut Game, items: Vec<XVal>) -> XVal {
    let entries = items.into_iter().enumerate().map(|(i, v)| (XKey::Num(i as f64), v)).collect();
    XVal::Array(g.ext.new_array(ArrayKind::List, entries))
}

/// A save file as GetSaveName and the size getters give it: its name without the
/// extension and its size in bytes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SaveFile {
    pub name: String,
    pub bytes: u64,
}

/// The last save written and the last one loaded this session; the engine sets them
/// (`nvf_play::save`), and they outlast a load (`Game::take_session`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SaveFiles {
    pub saved: Option<SaveFile>,
    pub loaded: Option<SaveFile>,
}

/// Batch 4's state that a load clears: the scripts MarkScriptOnLoad marked since it.
#[derive(Debug, Clone, Default)]
pub struct Sup4State {
    pub marked: BTreeSet<FormId>,
}

/// The array's entries, `None` for no array.
fn entries(g: &Game, v: Option<&XVal>) -> Option<Vec<(XKey, XVal)>> {
    let id = match v? {
        XVal::Array(id) => *id,
        XVal::Num(x) if *x >= 1.0 => *x as u32,
        _ => return None,
    };
    Some(g.ext.array(id)?.entries.clone())
}

/// A reference in an array that is no longer valid: none (0), or a form neither the
/// load order nor the world knows (a removed temporary reference) [documented:
/// Ar_HasInvalidRefs, "origin mod is disabled or temporary reference … removed"].
fn invalid_ref(g: &Game, r: u32) -> bool {
    let f = FormId(r);
    r == 0 || (g.data.sig(f).is_none() && g.world.peek(f).is_none() && g.world.placed.get(f).is_none() && f != FormId::PLAYER_REF)
}

/// Cyrillic letters to Latin ones [decision: NVForever's own table, a common Russian
/// romanisation with the Ukrainian and Belarusian letters added (the page names no
/// scheme); an upper-case letter starts its Latin letters in upper case]. Other
/// characters are kept.
pub fn translit(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let lower = c.to_lowercase().next().unwrap_or(c);
        let latin = match lower {
            'а' => "a",
            'б' => "b",
            'в' => "v",
            'г' | 'ґ' => "g",
            'д' => "d",
            'е' | 'э' => "e",
            'ё' => "yo",
            'є' => "ye",
            'ж' => "zh",
            'з' => "z",
            'и' | 'і' => "i",
            'ї' => "yi",
            'й' | 'ы' => "y",
            'к' => "k",
            'л' => "l",
            'м' => "m",
            'н' => "n",
            'о' => "o",
            'п' => "p",
            'р' => "r",
            'с' => "s",
            'т' => "t",
            'у' => "u",
            'ў' => "w",
            'ф' => "f",
            'х' => "kh",
            'ц' => "ts",
            'ч' => "ch",
            'ш' => "sh",
            'щ' => "shch",
            'ъ' | 'ь' => "",
            'ю' => "yu",
            'я' => "ya",
            _ => {
                out.push(c);
                continue;
            }
        };
        if c != lower {
            let mut it = latin.chars();
            if let Some(first) = it.next() {
                out.extend(first.to_uppercase());
                out.push_str(it.as_str());
            }
        } else {
            out.push_str(latin);
        }
    }
    out
}

/// The loaded exterior cells within `depth` of the player's cell, each axis apart; a cell
/// south of the origin (y < 0) is counted at x + 1, the player's own cell as it is; -1
/// every loaded cell; indoors the player's cell alone [documented: GetNearCells, "-1 …
/// uGridsToLoad"; observed: ext-sup4.md §5, #2039].
fn near_cells(g: &Game, depth: f64) -> Vec<FormId> {
    let Some(here) = g.cell_of(FormId::PLAYER_REF) else { return Vec::new() };
    let placed = &g.world.placed;
    let Some(grid) = placed.world_of(here).and_then(|w| placed.world_grid.get(&w)) else { return vec![here] };
    let Some(&at) = grid.iter().find(|(_, &c)| c == here).map(|(xy, _)| xy) else { return vec![here] };
    near_in_grid(grid.iter().filter(|(_, c)| g.loaded_cells.contains(c)).map(|(&xy, &c)| (xy, c)), at, depth)
}

/// [`near_cells`]'s rule on the loaded cells `cells` (grid x, y) around the player's cell
/// at `at`, in row order.
pub fn near_in_grid(cells: impl Iterator<Item = ((i32, i32), FormId)>, at: (i32, i32), depth: f64) -> Vec<FormId> {
    let d = (depth >= 0.0).then_some(depth as i64);
    let mut out: Vec<((i32, i32), FormId)> = cells
        .filter(|&((x, y), _)| {
            let x = if y < 0 { x + 1 } else { x };
            d.is_none_or(|d| (i64::from(x) - i64::from(at.0)).abs() <= d && (i64::from(y) - i64::from(at.1)).abs() <= d)
        })
        .collect();
    out.sort_by_key(|&((x, y), _)| (y, x));
    out.into_iter().map(|(_, c)| c).collect()
}

pub(crate) fn call(g: &mut Game, ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let zero = XVal::Num(0.0);
    let v = match name {
        // A joke page ("Two. Can play at that game. (no, they can't)") [documented];
        // there is one player [decision: nothing is done].
        "allocateplayer" => zero,
        // A random key of the array: a number for Ar_GetRandomKey, a string for the map
        // form (numeric map keys — including negative floats — and `Ar_List` indices
        // formatted with 6 decimal places, e.g. `"7.000000"`, `"-3.500000"`, `"2.000000"`)
        // [documented; observed: 2026-11-23 & 2026-11-24, `GSDocMitchellHouse`
        // (`0x00103DF9`), Fallout: New Vegas 1.4.0.525, xNVSE 6.4.9, SUP NVSE 855 with
        // the 10 official base masters, `sup_26_ar_getrandomkey_num_map` (`"7.000000"`)
        // and `w4d_36_sup_random_keymap_and_dialog`
        // (`tools/capture/nvse/catalogs/wave4d_extenders.json`, #3679):
        // `rnd(str=[0.000000],neg=[-3.500000],arr=[2.000000],empLen=8)`; no DLL or source
        // consulted]; an empty array returns 0 for Ar_GetRandomKey [observed: sup4
        // randomkey; 2026-11-23, `sup_08_ar_getrandomkey_empty`].
        // On stringmaps, `sup_08_ar_getrandomkey_empty` and `w4d_36_sup_random_keymap_and_dialog`
        // observed `"0.000000"` and buffer garbage on empty maps; NVForever keeps
        // returning the string key on stringmaps [documented] and `""` on empty arrays
        // [design].
        "ar_getrandomkey" | "ar_getrandomkeymap" => {
            let keys: Vec<XKey> = entries(g, args.first()).unwrap_or_default().into_iter().map(|(k, _)| k).collect();
            let pick = (!keys.is_empty()).then(|| keys[g.rng.next_u32() as usize % keys.len()].clone());
            match (name, pick) {
                ("ar_getrandomkey", Some(XKey::Num(k))) => XVal::Num(k),
                ("ar_getrandomkey", _) => zero,
                (_, Some(XKey::Str(s))) => XVal::Str(s),
                (_, Some(XKey::Num(k))) => XVal::Str(format!("{k:.6}")),
                _ => XVal::Str(String::new()),
            }
        }
        // `array [dimension]`: 1 when the array holds an invalid reference; with a
        // dimension the inner array at that key is searched [documented: the page's
        // second example, `Ar_HasInvalidRefs ar_myArr 1` searching ar_myArr[1]].
        "ar_hasinvalidrefs" => {
            let mut es = entries(g, args.first()).unwrap_or_default();
            if let Some(k) = args.get(1).map(|_| num(args, 1, 0.0)) {
                let inner = es.into_iter().find(|(key, _)| *key == XKey::Num(k)).map(|(_, v)| v);
                es = entries(g, inner.as_ref()).unwrap_or_default();
            }
            let bad = es.iter().any(|(_, v)| matches!(v, XVal::Ref(r) if invalid_ref(g, *r)));
            XVal::Num(f64::from(u8::from(bad)))
        }
        // `array f1 … f10`: the VM writes the variables (`fnv_script::vm` `out_params`):
        // key 0 is the return value 0, and key `i + 1` is written only when the `i`-th
        // element (in key order) is a number; non-numeric elements and positions past
        // the array's length leave their output variables unchanged [documented: the
        // example, f1 3, f2 8, f3 5; observed: 2026-11-24, `w6_t1_13_sup_ar_setfloats`:
        // `[3.5, "abc", 8.25]` into `(11, 22, 33, 44)` yields `(3.50, 22.00, 8.25, 44.00)`].
        "ar_setfloatsfrom" => {
            let mut out = vec![(XKey::Num(0.0), zero)];
            for (i, (_, v)) in entries(g, args.first()).unwrap_or_default().into_iter().take(10).enumerate() {
                if let XVal::Num(x) = v {
                    out.push((XKey::Num((i + 1) as f64), XVal::Num(x)));
                }
            }
            XVal::Array(g.ext.new_array(ArrayKind::Map, out))
        }
        // `udf [frames] [removeOnLoad]`: the UDF `frames` frames later (1 to 1000, 1
        // without), on the calling reference; with removeOnLoad 0 it outlasts a load
        // [documented: the page's notes and examples]. A lambda does not outlast a load
        // (see docs/spec-notes/ext-sup4.md §1; `Game::take_session`).
        "callfunctionnextframe" => {
            let Some(f) = args.first().filter(|f| matches!(f, XVal::Ref(r) if *r != 0)).cloned() else { return Some(Ok(zero)) };
            let frames = num(args, 1, 1.0).clamp(1.0, 1000.0) as u64;
            let keep = num(args, 2, 1.0) == 0.0;
            crate::delayed::call_in_frames(g, f, subject.or(ctx.self_ref), frames, keep);
            zero
        }
        // SUP returns a flat list of the current compass-marker references, unlike
        // xNVSE's nested complete paths.
        "getcurrentquestobjectiveteleportlinksalt" => crate::nvse_other::objective_teleport_links(g, true),
        // `type`: 0 the save file's bytes; 1 the .nvse co-save's, 0 [decision: NVForever
        // writes the extender's state into its one save file]; -1 before any save or
        // load this session [documented].
        "getloadedsavesize" | "getsavedsavesize" => {
            let f = if name == "getloadedsavesize" { &g.save_files.loaded } else { &g.save_files.saved };
            match (f, num(args, 0, 0.0) as i64) {
                (None, _) => XVal::Num(-1.0),
                (Some(f), 0) => XVal::Num(f.bytes as f64),
                (Some(_), _) => zero,
            }
        }
        // `type`: 0 the last save's name, 1 the last load's, without the extension; ""
        // before any [documented].
        "getsavename" => {
            let f = if num(args, 0, 0.0) as i64 == 1 { &g.save_files.loaded } else { &g.save_files.saved };
            XVal::Str(f.as_ref().map(|f| f.name.clone()).unwrap_or_default())
        }
        // `depth`: the cells near the player [documented: "references of near cells"].
        "getnearcells" => {
            let cells = near_cells(g, num(args, 0, 0.0));
            list(g, cells.into_iter().map(|c| XVal::Ref(c.0)).collect())
        }
        // `depth`: the map marker references in those cells [documented].
        "getnearmapmarkers" => {
            let mut found = Vec::new();
            for c in near_cells(g, num(args, 0, 0.0)) {
                for r in g.refs_in_cell(c, false) {
                    if g.world.placed.get(r).is_some_and(|p| p.map_marker.is_some()) {
                        found.push(XVal::Ref(r.0));
                    }
                }
            }
            list(g, found)
        }
        // The calling script, marked until the next load; the VM hands it over as the
        // argument [documented: MarkScriptOnLoad, "Marks calling script"]. 1, from a query
        // with no script form too [observed: sup4 markonload].
        "markscriptonload" => {
            if let Some(s) = form(args, 0) {
                g.sup4.marked.insert(s);
            }
            XVal::Num(1.0)
        }
        "isscriptmarkedonload" => XVal::Num(f64::from(u8::from(form(args, 0).is_some_and(|s| g.sup4.marked.contains(&s))))),
        // Kills the actors fighting the player, not the player's teammates [documented:
        // KillAll2, "kills all current player combat enemies leaving companions and
        // friendly NPCs intact"; deprecated].
        "killall2" | "killallalt" => {
            let all: Vec<FormId> = g.package_actors().into_iter().collect();
            for a in all {
                if a != FormId::PLAYER_REF && !g.is_dead(a) && g.present(a) && !g.is_teammate(a) && g.combat_target_of(a) == Some(FormId::PLAYER_REF) {
                    if let Err(e) = crate::functions::console::call_words(g, Some(a), "KillActor", &[], "KillAll2") {
                        return Some(Err(e));
                    }
                }
            }
            zero
        }
        // `[caught]` on an NPC: the flag a failed pickpocket sets (`crate::pickpocket`);
        // with an argument it is set; the flag as it is now either way [documented].
        "setcaughtpcpickpocketting" => {
            let Some(r) = subject.filter(|&r| r != FormId::PLAYER_REF) else { return Some(Ok(zero)) };
            if !args.is_empty() {
                g.extra.entry(r).or_default().caught_pickpocket = num(args, 0, 0.0) != 0.0;
            }
            XVal::Num(f64::from(u8::from(g.extra.get(&r).is_some_and(|e| e.caught_pickpocket))))
        }
        // The text with its Cyrillic letters in Latin ones (`translit`) [documented:
        // TranslitText; hypothesis: it returns the new text].
        "translittext" => XVal::Str(match args.first() {
            Some(XVal::Str(s)) => translit(s),
            _ => String::new(),
        }),
        _ => return None,
    };
    Some(Ok(v))
}

#[cfg(test)]
mod tests {
    use super::translit;

    #[test]
    fn cyrillic_to_latin() {
        assert_eq!(translit("Привет, мир!"), "Privet, mir!");
        assert_eq!(translit("Щука Жук Юля"), "Shchuka Zhuk Yulya");
        assert_eq!(translit("NV 2281"), "NV 2281");
    }
}
