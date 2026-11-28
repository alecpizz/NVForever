//! xNVSE's animation lists and generic form lists [documented: AddAnimation;
//! DelAnimation; DelAnimations; GetNthAnimation; GenericAddForm; GenericDeleteForm;
//! GenericGetForm; GenericReplaceForm]. Changes last for the session.

use crate::game::Game;
use fnv_script::vm::XVal;
use fnv_sim::FormId;

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

fn ids(v: Option<&serde_json::Value>) -> Vec<FormId> {
    let one = |x: &serde_json::Value| x.as_str().and_then(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).ok()).map(FormId);
    match v {
        Some(serde_json::Value::Array(a)) => a.iter().filter_map(one).collect(),
        Some(x) => one(x).into_iter().collect(),
        None => Vec::new(),
    }
}

impl Game<'_> {
    /// An actor base's animations (KFFZ), AddAnimation's and DelAnimation's changes first.
    pub fn base_animations(&self, b: FormId) -> Vec<String> {
        if let Some(v) = self.other.animations.get(&b) {
            return v.clone();
        }
        match self.data.record_field(b, "KFFZ", "") {
            Some(serde_json::Value::Array(a)) => a.iter().filter_map(|x| x.as_str()).filter(|s| !s.is_empty()).map(str::to_string).collect(),
            Some(serde_json::Value::String(s)) if !s.is_empty() => vec![s.clone()],
            _ => Vec::new(),
        }
    }

    /// A generic form list: 0 race hairs (HNAM), 1 race eyes (ENAM), 5 actor base
    /// packages, 6 actor base spells, 7 leveled spells (none read), 10 NPC head parts,
    /// 13 a form list [documented: GenericGetForm's list codes]; a script's change first.
    /// Returns `None` when `code` is unknown or `target`'s record type does not match the
    /// list code [observed: 2026-11-23 & 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`),
    /// Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 with the 10 official base masters,
    /// isolated clean-room staging copy (read-only `PrintToConsole` probe; zero writes to
    /// game install or Saves), `core_13_generic_add_form_split` (`NPC_` + code 1, `RACE` +
    /// code 5, `FLST` + code 1) and `w4d_04_generic_form_neg_idx`
    /// (`tools/capture/nvse/catalogs/wave4d_extenders.json`, #3802 / PR #3803:
    /// `flstC1(g=0,a=-1,r=0,d=0,n=36)` confirming FLST + code 1 rejects Get, Add, Replace,
    /// and Delete without mutating); zero DLL, PDB, or third-party source consulted].
    pub fn generic_list(&self, code: i64, target: FormId) -> Option<Vec<FormId>> {
        let sig = self.data.sig(self.unclone(target)).or_else(|| {
            (target == FormId::PLAYER_BASE || target == FormId::PLAYER_REF).then_some(*b"NPC_")
        })?;
        let valid_target = match code {
            0 | 1 => sig == *b"RACE",
            5..=7 => matches!(&sig, b"NPC_" | b"CREA"),
            10 => sig == *b"NPC_",
            13 => sig == *b"FLST",
            _ => false,
        };
        if !valid_target {
            return None;
        }
        if let Some(v) = self.other.generic.get(&(code, target)) {
            return Some(v.clone());
        }
        Some(match code {
            0 => ids(self.data.record_field(target, "HNAM", "")),
            1 => ids(self.data.record_field(target, "ENAM", "")),
            // From the template's list where GetBasePackages and GetBaseSpellListSpells
            // read it (actors.md §2.2).
            5 => match self.other.package_lists.get(&target) {
                Some(l) => l.clone(),
                None => self.data.actors.get(&self.data.base_template_source(target, |d| d.template_packages)).map(|d| d.packages.clone()).unwrap_or_default(),
            },
            6 => self.data.actors.get(&self.data.base_template_source(target, |d| d.template_spells)).map(|d| d.spells.clone()).unwrap_or_default(),
            7 => Vec::new(),
            // In GetHeadParts' order [observed: nvse-fourth, the record's order reversed].
            10 => {
                let mut v: Vec<FormId> = self
                    .data
                    .record_fields
                    .get(&target)
                    .into_iter()
                    .flatten()
                    .filter(|(s, _)| s == b"PNAM")
                    .filter_map(|(_, v)| v.as_str().and_then(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).ok()).map(FormId))
                    .collect();
                v.reverse();
                v
            }
            13 => crate::functions::list_members(self, target),
            _ => return None,
        })
    }

    /// Store a changed generic list where its readers look (packages: the package list;
    /// a form list: the list's edits; the rest: `OtherState::generic`).
    fn set_generic_list(&mut self, code: i64, target: FormId, list: Vec<FormId>) {
        match code {
            5 => {
                self.other.package_lists.insert(target, list);
            }
            13 => {
                *self.edited_list(target) = list;
            }
            _ => {
                self.other.generic.insert((code, target), list);
            }
        }
    }
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let base = |g: &Game, f: FormId| crate::functions::base_of(g, f).unwrap_or(f);
    let zero = XVal::Num(0.0);
    let v = match name {
        // `animName [baseActor]` (DelAnimations: `[baseActor]`); the calling reference's
        // base when none is given [documented].
        "addanimation" | "delanimation" | "delanimations" | "getnthanimation" => {
            let at = usize::from(name != "delanimations");
            let Some(b) = form(args, at).or(subject).map(|f| base(g, f)) else { return Some(Ok(zero)) };
            let mut list = g.base_animations(b);
            let out = match name {
                "getnthanimation" => {
                    let i = num(args, 0, -1.0) as i64;
                    return Some(Ok(XVal::Str(usize::try_from(i).ok().and_then(|i| list.get(i).cloned()).unwrap_or_default())));
                }
                "addanimation" => {
                    if let Some(XVal::Str(a)) = args.first() {
                        if !list.iter().any(|x| x.eq_ignore_ascii_case(a)) {
                            list.push(a.clone());
                        }
                    }
                    zero
                }
                "delanimation" => {
                    if let Some(XVal::Str(a)) = args.first() {
                        list.retain(|x| !x.eq_ignore_ascii_case(a));
                    }
                    zero
                }
                _ => {
                    list.clear();
                    zero
                }
            };
            g.other.animations.insert(b, list);
            out
        }
        // `code target [new] index`: -2 adds at the end (GenericAddForm); the delete and
        // replace give the form taken out [documented].
        "genericgetform" | "genericaddform" | "genericdeleteform" | "genericreplaceform" => {
            let code = num(args, 0, -1.0) as i64;
            let miss = || if name == "genericaddform" { XVal::Num(-1.0) } else { XVal::Ref(0) };
            let Some(t) = form(args, 1) else { return Some(Ok(miss())) };
            let t = if matches!(code, 0 | 1 | 13) { t } else { base(g, t) };
            let Some(mut list) = g.generic_list(code, t) else { return Some(Ok(miss())) };
            let idx_at = if matches!(name, "genericaddform" | "genericreplaceform") { 3 } else { 2 };
            let i = num(args, idx_at, -1.0) as i64;
            let n = list.len();
            let at = usize::try_from(i).ok().filter(|&i| i < n);
            let out = match name {
                "genericgetform" => return Some(Ok(XVal::Ref(at.map_or(0, |i| list[i].0)))),
                "genericaddform" => {
                    let Some(f) = form(args, 2) else { return Some(Ok(zero)) };
                    // `-2` appends: on an empty list (`n == 0`) it appends and returns `0`
                    // (`n: 0 -> 1`), and on a non-empty list it appends and returns the
                    // previous last index (`n - 1`); `0` succeeds on an empty list (`n == 0`,
                    // inserting at `0` and returning `0`), while on a non-empty list only
                    // `0..n` inserts at `i` and returns `i`; exact `i == n` when `n > 0`,
                    // past-end (`> n`), and any negative other than `-2` (`-1`, `-3`, `-10`)
                    // return `-1` without modifying the list [observed: nvse-eighth;
                    // 2026-11-23 & 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`),
                    // Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 with the 10
                    // official base masters, isolated clean-room staging copy (read-only
                    // `PrintToConsole` probes; zero writes to game install or Saves),
                    // `core_13_generic_add_form_split`, `w4d_04_generic_form_neg_idx`
                    // (`tools/capture/nvse/catalogs/wave4d_extenders.json`, #3802 / PR #3803:
                    // `PROBE|w4d_04_generic_form_neg_idx|emp(n0=0,d=0/0/0/0:0,r=0:0/0:1/0/0:1,a=-1/0:1/-1/-1/-1:1)|ne(n0=38,a=-1/37:39/-1/-1/-1:39,r=0/1/0/0,d=1:38/1:37/1:36/0:36)|flstC1(g=0,a=-1,r=0,d=0,n=36)`),
                    // and `w6t2p3_25_generic_form_real_flst`
                    // (`tools/capture/nvse/catalogs/wave6_t2_generic_form.json`:
                    // `PROBE|w6t2p3_25_generic_form_real_flst|add(0,-1,-1,n=1,past=-1)|delM1(h=1,t=0)|delM2(b=0,c=0)|nEnd=0`,
                    // confirming `i == 0` succeeds on `n == 0` while `i == 1` on `n == 1`
                    // returns `-1` without appending); zero DLL, PDB, or third-party source
                    // consulted].
                    let (at, out) = if i == -2 {
                        (n, n.saturating_sub(1) as f64)
                    } else if let Some(at) = usize::try_from(i).ok().filter(|&i| i < n.max(1)) {
                        (at, at as f64)
                    } else {
                        return Some(Ok(XVal::Num(-1.0)));
                    };
                    list.insert(at, f);
                    XVal::Num(out)
                }
                "genericdeleteform" => {
                    // On a non-empty list, negative indices (`-1`, `-2`, `-3`) each remove
                    // an entry (returning a non-zero form and decrementing count
                    // `39 -> 38 -> 37 -> 36`), and return `0` on an empty list; `0..n`
                    // removes `i`; `>= n` returns `0` [observed: `w4d_04_generic_form_neg_idx`,
                    // #3802 / PR #3803: `emp(...,d=0/0/0/0:0,...)` and
                    // `ne(...,d=1:38/1:37/1:36/0:36)`]. Removing index `0` (head of the list)
                    // for negative indices is [hypothesis] (`w4d_04` checks non-zero return
                    // and decremented counts, not which position was removed).
                    let pos = if i < 0 { (n > 0).then_some(0) } else { at };
                    match pos {
                        Some(pos) => XVal::Ref(list.remove(pos).0),
                        None => return Some(Ok(XVal::Ref(0))),
                    }
                }
                _ => {
                    // `GenericReplaceForm`: `-2` replaces the last entry when non-empty
                    // (returning the previous last form), and on an empty list returns `0`
                    // while appending the replacement form (`n: 0 -> 1`); `0..n` replaces
                    // `i`; other negatives (`-1`, `-3`) and `>= n` return `0` without
                    // mutating [observed: `w4d_04_generic_form_neg_idx`, #3802 / PR #3803].
                    let Some(f) = form(args, 2) else { return Some(Ok(XVal::Ref(0))) };
                    if i == -2 {
                        if let Some(last) = list.last_mut() {
                            XVal::Ref(std::mem::replace(last, f).0)
                        } else {
                            list.push(f);
                            XVal::Ref(0)
                        }
                    } else if let Some(pos) = at {
                        XVal::Ref(std::mem::replace(&mut list[pos], f).0)
                    } else {
                        return Some(Ok(XVal::Ref(0)));
                    }
                }
            };
            g.set_generic_list(code, t, list);
            out
        }
        _ => return None,
    };
    Some(Ok(v))
}
