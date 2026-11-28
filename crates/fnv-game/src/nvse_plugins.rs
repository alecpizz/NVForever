//! The most called LU, JohnnyGuitar, ShowOff and kNVSE functions the mod lists use past
//! JIP (docs/handoff/modlist-function-counts.json) [documented: their pages; observed:
//! the peer's plugins sets in the original game]. Session state lives in
//! `crate::nvse_gaps::GapsState` where a setter needs it.

use crate::game::{Game, RunCtx};
use fnv_script::vm::{ArrayKind, XKey, XVal};
use fnv_sim::FormId;

impl Game<'_> {
    /// Whether `item` has been marked unique via `ToggleItemUnique` (and therefore
    /// excluded as an optional repair-part source in the item repair menu)
    /// [documented: GECK wiki ToggleItemUnique / IsItemUnique].
    pub fn is_item_unique(&self, item: FormId) -> bool {
        self.gaps.unique_items.contains(&item)
    }
}

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

fn text(args: &[XVal], k: usize) -> String {
    match args.get(k) {
        Some(XVal::Str(s)) => s.clone(),
        _ => String::new(),
    }
}

fn yes(b: bool) -> XVal {
    XVal::Num(f64::from(u8::from(b)))
}

fn list(g: &mut Game, items: Vec<XVal>) -> XVal {
    let entries = items.into_iter().enumerate().map(|(i, v)| (XKey::Num(i as f64), v)).collect();
    XVal::Array(g.ext.new_array(ArrayKind::List, entries))
}

/// The script extender files NVForever stands in for, by name without `.dll`, without
/// regard to case; a name with `.dll` is not found [observed: plugins1 and plugins2, the
/// five plugins of the owner's Data\NVSE\Plugins and the extender's own nvse_1_4 and
/// nvse_steam_loader give 1, "jip_nvse.dll" 0].
const DLLS: [&str; 7] = ["jip_nvse", "johnnyguitar", "showoffnvse", "knvse", "mcm", "nvse_1_4", "nvse_steam_loader"];

/// ShowOff's easing of `t` (0–1) by name and mode, 0 in, 1 out, 2 in-out, the Penner
/// curves; an unknown name gives -1 [observed: plugins1 and plugins2, quad, cubic, quart,
/// quint, sine, expo, circ, back, elastic and bounce at 0.25 in all three modes;
/// "linear" -1].
pub(crate) fn ease(t: f64, name: &str, mode: i64) -> f64 {
    ease_unclamped(t.clamp(0.0, 1.0), name, mode)
}

/// [`ease`] without holding `t` to 0-1: ApplyEasingAlt eases 1.5 by quad-in to 2.25
/// [observed: showoff3 so3.ease.outside].
pub(crate) fn ease_unclamped(t: f64, name: &str, mode: i64) -> f64 {
    let f: fn(f64) -> f64 = match name.to_ascii_lowercase().as_str() {
        "quad" => |t| t * t,
        "cubic" => |t| t * t * t,
        "quart" => |t| t.powi(4),
        "quint" => |t| t.powi(5),
        "sine" => |t| 1.0 - (t * std::f64::consts::FRAC_PI_2).cos(),
        "expo" => |t| if t <= 0.0 { 0.0 } else { 2f64.powf(10.0 * t - 10.0) },
        "circ" => |t| 1.0 - (1.0 - t * t).max(0.0).sqrt(),
        "back" => |t| {
            let c1 = 1.70158;
            (c1 + 1.0) * t * t * t - c1 * t * t
        },
        "elastic" => |t| {
            if t <= 0.0 || t >= 1.0 {
                return t;
            }
            let c4 = 2.0 * std::f64::consts::PI / 3.0;
            -(2f64.powf(10.0 * t - 10.0)) * ((t * 10.0 - 10.75) * c4).sin()
        },
        "bounce" => |t| 1.0 - bounce_out(1.0 - t),
        _ => return -1.0,
    };
    // Penner's in-out back and elastic have constants of their own.
    let lower = name.to_ascii_lowercase();
    if mode == 2 && lower == "back" {
        let c2 = 1.70158 * 1.525;
        return if t < 0.5 {
            (2.0 * t).powi(2) * ((c2 + 1.0) * 2.0 * t - c2) / 2.0
        } else {
            ((2.0 * t - 2.0).powi(2) * ((c2 + 1.0) * (t * 2.0 - 2.0) + c2) + 2.0) / 2.0
        };
    }
    if mode == 2 && lower == "elastic" {
        let c5 = 2.0 * std::f64::consts::PI / 4.5;
        return if t <= 0.0 || t >= 1.0 {
            t
        } else if t < 0.5 {
            -(2f64.powf(20.0 * t - 10.0) * ((20.0 * t - 11.125) * c5).sin()) / 2.0
        } else {
            2f64.powf(-20.0 * t + 10.0) * ((20.0 * t - 11.125) * c5).sin() / 2.0 + 1.0
        };
    }
    match mode {
        0 => f(t),
        1 => 1.0 - f(1.0 - t),
        _ if t < 0.5 => f(2.0 * t) / 2.0,
        _ => 1.0 - f(2.0 - 2.0 * t) / 2.0,
    }
}

/// Penner's bounce, out.
fn bounce_out(t: f64) -> f64 {
    let (n1, d1) = (7.5625, 2.75);
    if t < 1.0 / d1 {
        n1 * t * t
    } else if t < 2.0 / d1 {
        let t = t - 1.5 / d1;
        n1 * t * t + 0.75
    } else if t < 2.5 / d1 {
        let t = t - 2.25 / d1;
        n1 * t * t + 0.9375
    } else {
        let t = t - 2.625 / d1;
        n1 * t * t + 0.984375
    }
}

impl Game<'_> {
    /// The session display name set for map marker `r` by `SetMapMarkerName`, if any.
    pub fn map_marker_name_override(&self, r: FormId) -> Option<&str> {
        self.gaps.marker_names.get(&r).map(String::as_str)
    }

    /// The session reputation override set for map marker `r` by `SetMapMarkerRep`:
    /// `Some(Some(rep))` replaces the marker's reputation, `Some(None)` clears it, and
    /// `None` leaves the authored `WMI1` record intact.
    pub fn map_marker_reputation_override(&self, r: FormId) -> Option<Option<FormId>> {
        self.gaps.marker_reputations.get(&r).copied()
    }
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, _ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let base = |g: &Game, f: FormId| crate::functions::base_of(g, f).unwrap_or(f);
    let zero = XVal::Num(0.0);
    let v = match name {
        // ---- LU ----
        // IDLE MODL; the setter for the session [observed: plugins1].
        "getidleanimpath" => match form(args, 0) {
            Some(f) if g.idle_def(f).is_some() => XVal::Str(g.idle_animation_path(f).unwrap_or_default().to_string()),
            Some(f) => match crate::nvse::record_value(g, f, "MODL", "") {
                Some(XVal::Str(s)) => XVal::Str(s),
                _ => XVal::Str(String::new()),
            },
            None => XVal::Str(String::new()),
        },
        "setidleanimpath" => {
            if let Some(f) = form(args, 0) {
                g.field_overrides.insert((f, "MODL/".into()), XVal::Str(text(args, 1)));
            }
            zero
        }
        // An entry point's second value; none reads -1 and the setter leaves it
        // [observed: plugins1, Toughness's first entry].
        "getnthperkentryvalue2" | "setnthperkentryvalue2" => XVal::Num(-1.0),
        // [observed: plugins1, Recipe127mm 4 inputs, the first 00140AAA; category
        // 00153621, subcategory 0013E44E]
        "getrecipeinputforms" => {
            let ins: Vec<XVal> = form(args, 0).and_then(|f| crate::nvse_lutana::recipe(g, f)).map(|r| r.ingredients.iter().map(|(f, _)| XVal::Ref(f.0)).collect()).unwrap_or_default();
            list(g, ins)
        }
        "getrecipecategory" | "getrecipesubcategory" => {
            let f = form(args, 0).unwrap_or(FormId(0));
            let sub = name == "getrecipesubcategory";
            let r = g.data.recipes.get(&f);
            let rec = r.and_then(|r| if sub { r.subcategory } else { r.category });
            XVal::Ref(g.gaps.recipe_categories.get(&(f, sub)).copied().or(rec).map_or(0, |c| c.0))
        }
        // `item [base]`: the count in the base's item list, leveled entries not rolled
        // [observed: plugins1, a stimpak on Doc's base and on Doc 1].
        "basegetitemcount" => {
            let item = form(args, 0).unwrap_or(FormId(0));
            let b = form(args, 1).or(subject).map(|f| base(g, f)).unwrap_or(FormId(0));
            XVal::Num(f64::from(g.world.base_entries(b).unwrap_or_default().iter().filter(|e| e.item == item).map(|e| e.count).sum::<i32>()))
        }
        // "the mod index of the script that calls the function" [documented:
        // GetSelfModIndex]: its plugin's place in the load order (`Exec::ext_call` sets
        // `other.plugin`); a runtime script has none [observed: plugins1, 0]. MCM mods
        // compare it with MCM's `_ActiveMod` (menu-xml.md §10).
        "getselfmodindex" => {
            let p = &g.other.plugin;
            XVal::Num(if p.is_empty() { 0.0 } else { g.data.load_order.iter().position(|l| l.eq_ignore_ascii_case(p)).map_or(0.0, |i| i as f64) })
        }
        // The rank the actor has, 0 without [observed: plugins1, the player without
        // Toughness 0].
        "hasperkrank" => {
            let p = form(args, 0).unwrap_or(FormId(0));
            let a = subject.unwrap_or(FormId::PLAYER_REF);
            XVal::Num(if a == FormId::PLAYER_REF { f64::from(g.player.perks.get(&p).copied().unwrap_or(0)) } else { 0.0 })
        }
        // `ingestible flag`: bitmask test `(ENIT.flags & flag) != 0` (1 No Auto-Calc, 2
        // Food Item, 4 Medicine) [observed: plugins1 and plugins2; Fallout: New Vegas
        // 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-25, GSDocMitchellHouse
        // (0x00103DF9), 10 official base masters,
        // `w6d2p2_25_ingestible_flag_and_skill_magazine`
        // (`tools/capture/nvse/catalogs/wave6d2_get_ingestible_flag_bitmask.json`):
        // `stim=1,0,1,1,1,1|cram=1,0|set4=0->1(ef2=1)->0|mag=1,0`].
        "getingestibleflag" => {
            let flags = form(args, 0).and_then(|f| g.ingestible_def(f)).map_or(0, |d| u32::from(d.flags));
            let k = num(args, 1, 0.0);
            let mask = if k.is_finite() && k > 0.0 && k <= f64::from(u32::MAX) && k.fract() == 0.0 { k as u32 } else { 0 };
            yes(mask != 0 && (flags & mask) != 0)
        }
        // [observed: plugins1, 2]
        "getdefaultmessagetime" => XVal::Num(crate::nvse_lutana::message_time(g)),
        // ---- JohnnyGuitar ----
        // The current value by code [observed: plugins1, Strength 5, Health 200, Barter
        // and Guns 17].
        "getactorvaluealt" => {
            let a = subject.unwrap_or(FormId::PLAYER_REF);
            XVal::Num(crate::functions::actor::actor_value(g, a, num(args, 0, 0.0) as u16).unwrap_or(0.0))
        }
        // The value within the range the game counts [observed: plugins1, Health 200,
        // Strength 5].
        "getthresholdedactorvalue" => {
            let a = subject.unwrap_or(FormId::PLAYER_REF);
            let code = match args.first() {
                Some(XVal::Num(c)) => *c as u16,
                Some(XVal::Str(n)) => match crate::functions::av_code(g, n) {
                    Ok(c) => c,
                    Err(_) => return Some(Ok(zero)),
                },
                _ => return Some(Ok(zero)),
            };
            XVal::Num(crate::functions::actor::actor_value_capped(g, a, code))
        }
        "getquestfailed" => yes(form(args, 0).is_some_and(|q| g.quest_state(q).failed)),
        // RACE DATA flags bit by bit [observed: plugins1, CaucasianOld 0 0 0]: the bit's
        // value, not 1 [observed: jg4 race.get.child.2 read 4, race.set.testqa.1.1 2,
        // race.set.testqa.3.1 8; race.get.caucasian.0 1].
        "getraceflag" => {
            let f = form(args, 0).unwrap_or(FormId(0));
            let flags = crate::nvse::record_value(g, f, "DATA", "flags").and_then(|v| v.num().ok()).unwrap_or(0.0) as u64;
            let k = num(args, 1, -1.0);
            XVal::Num(if (0.0..32.0).contains(&k) { (flags & 1 << (k as u64)) as f64 } else { 0.0 })
        }
        "isdllloaded" => {
            let n = text(args, 0).to_ascii_lowercase();
            yes(DLLS.contains(&n.as_str()))
        }
        // `on [ask]`: returns 1; GetAlwaysRun reads the engine's switch, which takes the
        // ask at its next frame [observed: plugins1].
        "setalwaysrun" => {
            let on = num(args, 0, 1.0) != 0.0;
            g.gaps.always_run_request = Some(on);
            g.gaps.always_run = on;
            XVal::Num(1.0)
        }
        "getalwaysrun" => yes(g.gaps.always_run),
        // `value inMin inMax outMin outMax`, not clamped [observed: plugins1, 5 of 0–10
        // onto 100–200 is 150, 15 is 250].
        "remap" => {
            let (v, a, b, c, d) = (num(args, 0, 0.0), num(args, 1, 0.0), num(args, 2, 1.0), num(args, 3, 0.0), num(args, 4, 1.0));
            XVal::Num(if b == a { c } else { c + (v - a) * (d - c) / (b - a) })
        }
        "getvector3ddistance" => {
            let p: Vec<f64> = (0..6).map(|k| num(args, k, 0.0)).collect();
            XVal::Num(((p[3] - p[0]).powi(2) + (p[4] - p[1]).powi(2) + (p[5] - p[2]).powi(2)).sqrt())
        }
        // Load-order override queries backed by `GameData::form_overrides`
        // [documented: GECK wiki GetFormOverrideIndex, IsFormOverridden ("overridden by a
        // plugin lower in the load order than the calling script");
        // observed: plugins1, non-overridden Stimpak returns 0 and 0;
        // observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528,
        // clean-room script probe jg_01_form_override_index_edge (runtime CompileScript
        // caller): overridden pistol GetFormOverrideIndex=5, null=0, TempCloneForm=255,
        // IsFormOverridden(pistol)=0, IsFormOverridden(clone)=0; no DLL or source consulted].
        "isformoverridden" => yes(form(args, 0).or(subject).is_some_and(|f| g.data.is_form_overridden_after(f, &g.other.plugin))),
        "getformoverrideindex" => {
            let Some(f) = form(args, 0).or(subject) else { return Some(Ok(XVal::Num(0.0))) };
            let idx = if let Some(i) = g.data.form_override_index(f) {
                f64::from(i)
            } else if f.0 >> 24 >= 0xFE {
                255.0
            } else if g.world.placed.get(f).is_some() {
                f64::from(f.0 >> 24)
            } else {
                0.0
            };
            XVal::Num(idx)
        }
        // ZNAM; Set… for the session, none leaving the sound [observed: plugins2, the
        // beep reads back, a null set does not clear it]. WEAP shares live slots 12/13
        // with GetWeaponSound / SetWeaponSound.
        "getputdownsound" => match form(args, 0) {
            Some(f) if g.is_weapon_form(f) => XVal::Ref(g.weapon_sound(f, 13).map_or(0, |s| s.0)),
            Some(f) => match crate::nvse::record_value(g, f, "ZNAM", "") {
                Some(XVal::Ref(r)) => XVal::Ref(r),
                _ => XVal::Ref(0),
            },
            None => XVal::Ref(0),
        },
        "setpickupsound" | "setputdownsound" => {
            if let (Some(f), Some(s)) = (form(args, 0), form(args, 1)) {
                if g.is_weapon_form(f) {
                    let slot = if name == "setpickupsound" { 12 } else { 13 };
                    g.set_weapon_sound(f, slot, Some(s));
                } else if g.is_sound_form(s) {
                    let sub = if name == "setpickupsound" { "YNAM/" } else { "ZNAM/" };
                    g.field_overrides.insert((f, sub.into()), XVal::Ref(s.0));
                }
            }
            zero
        }
        // [observed: plugins2, the 127mm recipe's subcategory set and back]
        "setrecipecategory" | "setrecipesubcategory" => {
            if let (Some(f), Some(c)) = (form(args, 0), form(args, 1)) {
                g.gaps.recipe_categories.insert((f, name == "setrecipesubcategory"), c);
            }
            zero
        }
        // ACTI SNAM and WNAM [observed: plugins2, the electric box none and none].
        "getactivatorsoundactivate" | "getactivatorwatertype" | "getactivatorsoundlooping" => {
            let sub = match name {
                "getactivatorsoundactivate" => "VNAM",
                "getactivatorwatertype" => "WNAM",
                _ => "SNAM",
            };
            match form(args, 0).and_then(|f| crate::nvse::record_value(g, f, sub, "")) {
                Some(XVal::Ref(r)) => XVal::Ref(r),
                _ => XVal::Ref(0),
            }
        }
        // `list form`: the index it was at, -1 when absent [observed: plugins2, the added
        // stimpak came off from index 0 and a second try gave -1].
        "removeformfromformlist" => {
            let (Some(l), Some(f)) = (form(args, 0), form(args, 1)) else { return Some(Ok(XVal::Num(-1.0))) };
            let mut members = crate::functions::list_members(g, l);
            let Some(i) = members.iter().position(|&m| m == f) else { return Some(Ok(XVal::Num(-1.0))) };
            members.remove(i);
            if let Some(added) = g.form_lists.get_mut(&l) {
                added.retain(|&m| m != f);
            }
            g.list_edits.insert(l, members);
            XVal::Num(i as f64)
        }
        // A map marker's WMI1; the setter for the session [observed: plugins2, the
        // Goodsprings marker's RepNVGoodsprings, set to Primm's and back].
        "getmapmarkerrep" => {
            let r = subject.unwrap_or(FormId(0));
            let rec = g.world.placed.get(r).and_then(|p| p.marker_reputation);
            XVal::Ref(g.gaps.marker_reputations.get(&r).copied().unwrap_or(rec).map_or(0, |f| f.0))
        }
        "setmapmarkerrep" => {
            if let Some(r) = subject {
                g.gaps.marker_reputations.insert(r, form(args, 0));
            }
            zero
        }
        // ---- record setters the getters read (`crate::nvse::field`), for the session ----
        // `list item` [documented: SetRepairList].
        "setrepairlist" => {
            if let (Some(l), Some(f)) = (form(args, 0), form(args, 1).or_else(|| subject.map(|r| base(g, r)))) {
                g.field_overrides.insert((f, "REPL/".into()), XVal::Ref(l.0));
            }
            zero
        }
        // `mask item`: BMDT biped flags and general flags, as GetEquipmentSlotsMask and
        // GetEquipmentBipedMask read them [documented].
        "setequipmentslotsmask" | "setequipmentbipedmask" => {
            if let Some(f) = form(args, 1).or_else(|| subject.map(|r| base(g, r))) {
                let mask = num(args, 0, 0.0).max(0.0).trunc();
                if g.is_armo(f) {
                    if name == "setequipmentslotsmask" {
                        g.set_armo_slots(f, mask as u32);
                    } else {
                        g.set_armo_general_flags(f, (mask as u32 & 0xFF) as u8);
                    }
                } else {
                    let path = if name == "setequipmentslotsmask" { "BMDT/biped_flags" } else { "BMDT/general_flags" };
                    g.field_overrides.insert((f, path.into()), XVal::Num(mask));
                }
            }
            zero
        }
        // ARMO BIPL [documented: GetBipedModelList; SetBipedModelList `armor list`].
        "getbipedmodellist" => match form(args, 0).and_then(|f| crate::nvse::record_value(g, f, "BIPL", "")) {
            Some(XVal::Ref(r)) => XVal::Ref(r),
            _ => XVal::Ref(0),
        },
        "setbipedmodellist" => {
            if let Some(f) = form(args, 0).filter(|&f| g.data.sig(g.unclone(f)) == Some(*b"ARMO")) {
                match args.get(1) {
                    None => {
                        g.field_overrides.insert((f, "BIPL/".into()), XVal::Ref(0));
                    }
                    Some(_) => {
                        if let Some(l) = form(args, 1).filter(|&l| g.data.sig(g.unclone(l)) == Some(*b"FLST")) {
                            g.field_overrides.insert((f, "BIPL/".into()), XVal::Ref(l.0));
                        }
                    }
                }
            }
            zero
        }
        // `flag actor`: a bit of the base's ACBS flags [documented: ActorHasBaseFlag], from
        // the template when the base takes its base data from one [observed: plugin-odds
        // so.baseflags.acre, VSpawnTier2RadscorpionMed's own flags are 576 (bits 6, 9),
        // with Use Base Data and a template the game reads bits 3 6 9 22 24 25].
        "actorhasbaseflag" => {
            let r = form(args, 1).or(subject).unwrap_or(FormId(0));
            let b = base(g, r);
            let b = g.template_source(b, r, |d| d.template_base).map_or(b, |(s, _)| s);
            let flags = u64::from(g.actor_base_flags_raw(b));
            let k = num(args, 0, -1.0);
            yes((0.0..32.0).contains(&k) && flags >> (k as u64) & 1 != 0)
        }
        // `weapon trait [model]`: the first-person model, 0 the plain one (WNAM), 1–7 the
        // mod combinations (WNM1–WNM7) [documented: SetWeapon1stPersonModel; observed:
        // data1, the 10mm pistol's 0–3 read WNAM, WNM1, WNM2, WNM3; trait 0 set and back].
        "getweapon1stpersonmodel" => {
            let w = form(args, 0).unwrap_or(FormId(0));
            let sub = match num(args, 1, -1.0) as i64 {
                0 => "WNAM".to_string(),
                k @ 1..=7 => format!("WNM{k}"),
                _ => return Some(Ok(XVal::Ref(0))),
            };
            match crate::nvse::record_value(g, w, &sub, "") {
                Some(XVal::Ref(r)) => XVal::Ref(r),
                _ => XVal::Ref(0),
            }
        }
        // No model argument clears the combination's form, so first person draws the
        // third-person model the weapon has for it (weapon-mods.md §2, §8) [hypothesis: WAP's
        // script files set MODL and MWD1–MWD7 with SetWeaponModel and then call this with no
        // model for each of 0–7, for new first-person models; observed: WAP Year One's
        // gr_10mmP.txt]. A model argument that is no form (an empty reference, a missing
        // master's form) changes nothing, as before [hypothesis: the original's no-op].
        "setweapon1stpersonmodel" => {
            let Some(w) = form(args, 0) else { return Some(Ok(zero)) };
            let sub = match num(args, 1, -1.0) as i64 {
                0 => "WNAM/".to_string(),
                k @ 1..=7 => format!("WNM{k}/"),
                _ => return Some(Ok(zero)),
            };
            let model = match args.get(2) {
                None => 0,
                Some(_) => match form(args, 2) {
                    Some(m) => m.0,
                    None => return Some(Ok(zero)),
                },
            };
            g.field_overrides.insert((w, sub), XVal::Ref(model));
            zero
        }
        // `armor modelID(1-4) index3D`, success(0/1) [documented: ClearArmorAltTexture].
        // NVForever keeps no alternate textures on armour models [hypothesis], so there is
        // nothing at any index to clear (0), while clearing them all (-1) succeeds on
        // modelID 1..=4 and fails (0) on modelID 0 or 5 or index 0 or 99 [observed:
        // plugin-odds, armour 00020423 model 1: index -1 gave 1, index 0 gave 0; observed
        // 2026-11-24: `w6t2_30_jg5_get_weapons_for_mod_and_armor_alt_tex`
        // (`tools/capture/nvse/catalogs/wave6_t2_observed_matches.json`):
        // `clr(1m1=1,10=0,4m1=1,0m1=0,5m1=0,199=0)`; zero DLL, PDB, or third-party source
        // consulted]; no form gives 0 [hypothesis].
        "cleararmoralttexture" => {
            yes(form(args, 0).is_some() && (1.0..=4.0).contains(&num(args, 1, 0.0)) && num(args, 2, 0.0) == -1.0)
        }
        // `set|remove function flag [quest]` [documented: SetJohnnyOnStartQuestEventHandler].
        "setjohnnyonstartquesteventhandler" | "setjohnnyonstopquesteventhandler" => {
            let start = name == "setjohnnyonstartquesteventhandler";
            let Some(f) = args.get(1).cloned() else { return Some(Ok(zero)) };
            let quest = form(args, 3).filter(|&id| id != FormId(0x0000_003B));
            g.gaps.quest_handlers.retain(|(s, h, q)| !(*s == start && *h == f && *q == quest));
            if num(args, 0, 1.0) != 0.0 {
                g.gaps.quest_handlers.push((start, f, quest));
            }
            zero
        }
        // ACBS flags, the low 16 bits; the setter for the session [observed: data2,
        // Sunny 17, Boone 144, Doc and the player 0; Sunny + 8 reads 25 and back].
        "getactorbaseflagslow" => {
            let b = form(args, 0).or(subject).map(|f| g.actor_flags_target(f)).unwrap_or(FormId(0));
            XVal::Num(f64::from(g.actor_base_flags_raw(b) & 0xFFFF))
        }
        "setactorbaseflagslow" => {
            if let Some(b) = form(args, 1).or(subject).map(|f| g.actor_flags_target(f)) {
                g.set_actor_base_flags(b, false, (num(args, 0, 0.0).max(0.0) as u64 & 0xFFFF) as u16);
            }
            zero
        }
        // NPC_ NAM6, 0 standing for 1; SetNPCHeight returns the old height [observed:
        // data2, five NPCs 1.000 (their NAM6 0), 1.25 set and back].
        "getnpcheight" | "setnpcheight" => {
            let set = name == "setnpcheight";
            let b = form(args, usize::from(set)).or(subject).map(|f| base(g, f)).unwrap_or(FormId(0));
            let h = crate::nvse::record_value(g, b, "NAM6", "").and_then(|v| v.num().ok()).filter(|&h| h > 0.0).unwrap_or(1.0);
            if set {
                g.field_overrides.insert((b, "NAM6/".into()), XVal::Num(num(args, 0, 1.0)));
            }
            XVal::Num(h)
        }
        // `slot`: [item, health, mod flags] of what is worn there, no array for none
        // [observed: data2, the 10mm pistol in slot 5 [0000434F, 79.92, 0]; slot 9 empty].
        "getequippeddata" => {
            let a = subject.unwrap_or(FormId::PLAYER_REF);
            let item = match crate::nvse::call(g, _ctx, "getequippedobject", Some(a), args) {
                Some(Ok(XVal::Ref(i))) if i != 0 => FormId(i),
                _ => return Some(Ok(zero)),
            };
            let full = crate::nvse::record_value(g, item, "DATA", "health").and_then(|v| v.num().ok()).unwrap_or(0.0);
            let (cond, mods) = g
                .world
                .inventory_mut(a)
                .ok()
                .and_then(|inv| inv.groups().find(|(i, e, _)| *i == item && e.equipped).map(|(_, e, _)| (f64::from(e.condition_or_whole()), e.mods)))
                .unwrap_or((1.0, 0));
            let vals = vec![XVal::Ref(item.0), XVal::Num(full * cond), XVal::Num(f64::from(mods))];
            list(g, vals)
        }
        // On an inventory reference of a currently equipped item: 1 when the world holds a
        // NoUnequip lock (EquipItem / EquipItemAlt NoUnequipFlag or SetNoUnequip 1), else 0
        // [documented: GECK wiki GetNoUnequip; observed: data2, unlocked pistol reads 0].
        "getnounequip" => {
            let Some((actor, item)) = subject.and_then(|r| g.temp_item(r)) else { return Some(Ok(zero)) };
            let equipped = g.world.peek(actor).and_then(|s| s.inventory.as_ref()).is_some_and(|inv| inv.is_equipped(item));
            yes(equipped && g.world.equip_lock(actor, item) == Some(fnv_sim::world::EquipLock::NoUnequip))
        }
        // ACTI WNAM; none leaves it [observed: data2].
        "setactivatorwatertype" => {
            if let (Some(w), Some(f)) = (form(args, 0), form(args, 1)) {
                g.field_overrides.insert((f, "WNAM/".into()), XVal::Ref(w.0));
            }
            zero
        }
        // A map marker's own name (its FULL), which GetName does not read; the setter for
        // the session [observed: data3, Goodsprings, then NVF Springs].
        "getmapmarkername" => {
            let r = subject.unwrap_or(FormId(0));
            XVal::Str(g.gaps.marker_names.get(&r).cloned().or_else(|| g.data.full_names.get(&r).cloned()).unwrap_or_default())
        }
        "setmapmarkername" => {
            if let Some(r) = subject {
                g.gaps.marker_names.insert(r, text(args, 0));
            }
            zero
        }
        // `forms types`: the types in the types array's order, each type's forms last
        // first, the forms of other types after them in their order [observed: data3,
        // knife, pistol, Nuka-Cola, stimpak, then the leather armour].
        "sortformsbytype" => {
            let ids = |v: Option<&XVal>| match v {
                Some(XVal::Array(i)) => Some(*i),
                _ => None,
            };
            let (Some(fa), Some(ta)) = (ids(args.first()), ids(args.get(1))) else { return Some(Ok(zero)) };
            let forms: Vec<FormId> = g.ext.array(fa).map(|a| a.entries.iter().filter_map(|(_, v)| match v { XVal::Ref(r) if *r != 0 => Some(FormId(*r)), _ => None }).collect()).unwrap_or_default();
            let types: Vec<i32> = g.ext.array(ta).map(|a| a.entries.iter().filter_map(|(_, v)| v.num().ok()).map(|x| x as i32).collect()).unwrap_or_default();
            let code = |g: &Game, f: FormId| g.data.sig(base(g, f)).and_then(|s| crate::functions::items::form_type_code(&s));
            let mut out: Vec<XVal> = Vec::new();
            for t in &types {
                out.extend(forms.iter().rev().filter(|&&f| code(g, f) == Some(*t)).map(|f| XVal::Ref(f.0)));
            }
            out.extend(forms.iter().filter(|&&f| code(g, f).is_none_or(|c| !types.contains(&c))).map(|f| XVal::Ref(f.0)));
            list(g, out)
        }
        // `list source index`: the source's forms (an array's, a list's) go in at the
        // index in their order; returns 1, 0 past the end [observed: data3 and plugins2].
        "listaddarray" | "listaddlist" => {
            let Some(l) = form(args, 0) else { return Some(Ok(zero)) };
            let add: Vec<FormId> = if name == "listaddarray" {
                match args.get(1) {
                    Some(XVal::Array(i)) => g.ext.array(*i).map(|a| a.entries.iter().filter_map(|(_, v)| match v { XVal::Ref(r) if *r != 0 => Some(FormId(*r)), _ => None }).collect()).unwrap_or_default(),
                    _ => Vec::new(),
                }
            } else {
                form(args, 1).map(|s| crate::functions::list_members(g, s)).unwrap_or_default()
            };
            let mut members = crate::functions::list_members(g, l);
            let at = num(args, 2, 0.0);
            if at < 0.0 || at as usize > members.len() {
                return Some(Ok(zero));
            }
            for (k, f) in add.into_iter().enumerate() {
                members.insert(at as usize + k, f);
            }
            g.list_edits.insert(l, members);
            XVal::Num(1.0)
        }
        // `IsItemUnique item` / `ToggleItemUnique item/list 1/0`: tracks session unique
        // status on repairable equipment forms (WEAP/ARMO, or FLST members of those types)
        // so unique items are excluded as repair-part sources (`Game::player_repair_list`)
        // [documented: GECK wiki ToggleItemUnique, IsItemUnique; observed: data3, ALCH
        // stimpak ignores toggle and remains 0].
        "isitemunique" => yes(form(args, 0).is_some_and(|f| g.is_item_unique(base(g, f)))),
        "toggleitemunique" => {
            if let Some(target) = form(args, 0) {
                let on = num(args, 1, 0.0) != 0.0;
                let tb = base(g, target);
                let items: Vec<FormId> = if g.data.sig(tb) == Some(*b"FLST") {
                    crate::functions::list_members(g, tb).into_iter().map(|m| base(g, m)).collect()
                } else {
                    vec![tb]
                };
                for item in items {
                    if matches!(g.data.sig(g.unclone(item)).as_ref(), Some(b"WEAP" | b"ARMO")) {
                        if on {
                            g.gaps.unique_items.insert(item);
                        } else {
                            g.gaps.unique_items.remove(&item);
                        }
                    }
                }
            }
            zero
        }
        // The XP as given, no perk adjusting it [documented: RewardXPExact; observed:
        // data3, 10 + 5 reads 15 in the same frame].
        "rewardxpexact" => {
            g.player.xp += num(args, 0, 0.0).trunc();
            zero
        }
        // `set|remove function flag [code]` [documented: SetOnActorValueChangeEventHandler;
        // observed 2026-11-24: -1 is a wildcard like omitted].
        "setonactorvaluechangeeventhandler" => {
            let Some(f) = args.get(1).cloned() else { return Some(Ok(zero)) };
            let code = args.get(3).and_then(|v| v.num().ok()).filter(|&c| c >= 0.0).map(|c| c as u16);
            let exact = (num(args, 2, 0.0) as i64) & 1 != 0;
            g.gaps.av_handlers.retain(|(h, c, _)| !(*h == f && *c == code));
            if num(args, 0, 1.0) != 0.0 {
                g.gaps.av_handlers.push((f, code, exact));
            }
            zero
        }
        // ---- idles and animations ----
        // The last special idle the actor played, kept after it stops [observed: anim1,
        // 00171DBB from the frame after ForcePlayIdle and still 4 frames after
        // ForceStopIdle or StopIdle; hypothesis: kept until another plays].
        "getplayedidle" => XVal::Ref(subject.and_then(|a| g.extra.get(&a)).and_then(|e| e.last_idle).map_or(0, |i| i.0)),
        // kNVSE: `idle [playerPOV]`, played at once as PlayIdle's are; optional
        // `playerPOV` 1 targets third person and 2 targets first person
        // [documented: GECK wiki ForcePlayIdle].
        "forceplayidle" => {
            if let (Some(a), Some(i)) = (subject, form(args, 0)) {
                let pov = match num(args, 1, 0.0) as i64 {
                    1 => Some(crate::idles::IdlePov::Third),
                    2 => Some(crate::idles::IdlePov::First),
                    _ => None,
                };
                if g.ask_idle_pov(a, i, crate::idles::IdleSource::Script, pov) {
                    g.extra.entry(a).or_default().last_idle = Some(i);
                }
            }
            zero
        }
        // The special idle stops (the engine takes it at its next frame); optional
        // `playerPOV` 1/2 scopes `ForceStopIdle` to one player rig [documented: ForceStopIdle].
        "stopidle" | "forcestopidle" => {
            if let Some(a) = subject {
                let pov = if name == "forcestopidle" {
                    match num(args, 0, 0.0) as i64 {
                        1 => Some(crate::idles::IdlePov::Third),
                        2 => Some(crate::idles::IdlePov::First),
                        _ => None,
                    }
                } else {
                    None
                };
                match pov {
                    Some(p) => g.gaps.idle_stop_pov.push((a, p)),
                    None => g.gaps.idle_stops.push(a),
                }
            }
            zero
        }
        // `category subtype [flags]`: 1 any (1 0) or the standing idle (1 1), 2 moving,
        // 3 attacking, 4 reloading, from what the engine tells the body does; PlayGroup's
        // attack and reload groups do not count [observed: anim1 and anim2, standing 1 1
        // 0 0 0, an idle kept 30 frames after a stop; NPCs fighting the player read 2 and
        // 3; PlayGroup AttackRight/ReloadA read 0; reload1, 4 0 read 1 during the player's and an NPC's reload].
        "isanimplayingex" => {
            use crate::anim_state::{AnimAction, MoveDir};
            let a = subject.unwrap_or(FormId::PLAYER_REF);
            let Some(st) = g.senses.anim.get(&a) else { return Some(Ok(zero)) };
            let moving = st.moving != MoveDir::None;
            // Movement: sub-type 0 any, 1–4 forward, backward, left, right, 5 jump;
            // variation flags 1 fast, 2 dodge, 4 turn [documented: IsAnimPlayingEx], read
            // from the movement group the body plays (`Jump…`, `Fast…`, `Turn…`). Before,
            // every sub-type answered "moving", so Enhanced Movement's sprint, which
            // stays off while `IsAnimPlayingEx 2 5` (a jump) holds, never started.
            let group = st.group.unwrap_or("");
            let jump = group.starts_with("Jump");
            let flags = num(args, 2, 0.0) as i64;
            let variation = (flags & 1 == 0 || group.starts_with("Fast")) && flags & 2 == 0 && (flags & 4 == 0 || group.starts_with("Turn"));
            yes(match (num(args, 0, -1.0) as i64, num(args, 1, 0.0) as i64) {
                (1, 0) => true,
                (1, _) => !moving,
                (2, 0) => (moving || jump) && variation,
                (2, 5) => jump,
                (2, s) => variation && st.moving == [MoveDir::None, MoveDir::Forward, MoveDir::Backward, MoveDir::Left, MoveDir::Right].get(s.clamp(0, 4) as usize).copied().unwrap_or(MoveDir::None) && moving,
                (3, _) => st.action.is_some_and(|x| matches!(x, AnimAction::Attack | AnimAction::FollowThrough | AnimAction::AttackReturn)),
                (4, _) => st.action.is_some_and(|x| matches!(x, AnimAction::Reload | AnimAction::ReloadOpen)),
                _ => false,
            })
        }
        // ---- ShowOff ----
        "applyeasing" => XVal::Num(ease(num(args, 0, 0.0), &text(args, 1), num(args, 2, 0.0) as i64)),
        // DNAM animation type, unarmed 0 [observed: plugins1, the unarmed player 0].
        "getequippedweapontype" => {
            let a = subject.unwrap_or(FormId::PLAYER_REF);
            let w = crate::functions::items::equipped_weapon(g, a);
            XVal::Num(w.and_then(|w| crate::nvse::record_value(g, w, "DNAM", "animation_type")).and_then(|v| v.num().ok()).unwrap_or(0.0))
        }
        "isaiming" | "isblockingoraiming" => yes(subject.is_none_or(|a| a == FormId::PLAYER_REF) && g.jip2.told.aiming),
        // Carrying more than the capacity [observed: plugins1, 0].
        "getisplayeroverencumbered" => {
            let w = g.world.carried_weight(FormId::PLAYER_REF).unwrap_or(0.0);
            let cap = crate::functions::actor::actor_value(g, FormId::PLAYER_REF, crate::actor_values::CARRY_WEIGHT).unwrap_or(0.0);
            yes(w > cap)
        }
        // A weapon of a gun skill [observed: plugins1, the pistol 1, the knife 0].
        "isweaponranged" => {
            let w = form(args, 0).or_else(|| subject.and_then(|a| crate::functions::items::equipped_weapon(g, a))).unwrap_or(FormId(0));
            let s = g.data.weapon_skills.get(&w).map(|&s| s as u16);
            yes(s.is_some_and(|s| s != crate::actor_values::MELEE_WEAPONS && s != crate::actor_values::UNARMED))
        }
        _ => return None,
    };
    Some(Ok(v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easings_match_the_observed_rows() {
        let f = |n: &str, m: i64| (ease(0.25, n, m) * 10000.0).floor();
        assert_eq!([f("quad", 0), f("quad", 1), f("quad", 2)], [625.0, 4375.0, 1250.0]);
        assert_eq!([f("cubic", 0), f("cubic", 1), f("cubic", 2)], [156.0, 5781.0, 625.0]);
        assert_eq!([f("sine", 0), f("sine", 1), f("sine", 2)], [761.0, 3826.0, 1464.0]);
        assert_eq!(ease(0.25, "linear", 0), -1.0);
        // [observed: plugins2]
        assert_eq!([f("quart", 0), f("quart", 1), f("quart", 2)], [39.0, 6835.0, 312.0]);
        assert_eq!([f("quint", 0), f("quint", 1), f("quint", 2)], [9.0, 7626.0, 156.0]);
        assert_eq!([f("expo", 0), f("expo", 1), f("expo", 2)], [55.0, 8232.0, 156.0]);
        assert_eq!([f("circ", 0), f("circ", 1), f("circ", 2)], [317.0, 6614.0, 669.0]);
        assert_eq!([f("back", 0), f("back", 1), f("back", 2)], [-642.0, 8174.0, -997.0]);
        assert_eq!([f("elastic", 0), f("elastic", 1), f("elastic", 2)], [-56.0, 9116.0, 119.0]);
        assert_eq!([f("bounce", 0), f("bounce", 1), f("bounce", 2)], [273.0, 4726.0, 1171.0]);
    }
}
