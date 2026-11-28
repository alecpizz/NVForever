//! JIP / LU / JohnnyGuitar functions from the owner's mod lists (jip-fifth set)
//! [documented: their pages; observed: the peer's jip-fifth rows in the original game].

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

/// The game calendar's day number of a date: 365-day years, no leap days, as the game's
/// clock counts them (fnv_sim::clock::days_in_month).
fn day_number(year: i64, month0: i64, day: i64) -> i64 {
    let months: i64 = (0..month0.clamp(0, 12)).map(|m| fnv_sim::clock::days_in_month(year, m)).sum();
    year * 365 + months + (day - 1)
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let base = |g: &Game, f: FormId| crate::functions::base_of(g, f).unwrap_or(f);
    let v = match name {
        // JohnnyGuitar [documented: EditorIDToFormID; GetEditorID; Clamp; observed: the
        // editor ID is matched without regard to case, though the page says
        // case-sensitive; the player reference's is "PlayerRef"].
        "editoridtoformid" => {
            let id = match args.first() {
                Some(XVal::Str(s)) => s.to_ascii_lowercase(),
                _ => String::new(),
            };
            // SetEditorID's names first [hypothesis: SetEditorID precedence over
            // JohnnyGuitar camera overlay refs is unprobed], then loaded plugin records,
            // then built-in JohnnyGuitar camera overlay references
            // (`JG_CameraOverlayRef_PreIS`, `JG_CameraOverlayRef_PreUI`) [observed:
            // Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528,
            // 2026-11-24, GSDocMitchellHouse (0x00103DF9), 10 official base masters,
            // `w4d_35_jg_camera_overlay_nif_bound`
            // (`tools/capture/nvse/catalogs/wave4d_extenders.json`, calling
            // `EditorIDToFormID` on both overlay names):
            // `camRef(preIS=FF001314,preUI=FF001318)`; no DLL or source consulted].
            let set = g.other.edids.iter().find(|(_, e)| e.eq_ignore_ascii_case(&id)).map(|(f, _)| *f);
            let found = if id == "playerref" {
                Some(FormId::PLAYER_REF)
            } else {
                set.or_else(|| g.data.edids.get(&id).copied())
                    .or_else(|| crate::nvse_jg_nif::camera_overlay_by_edid(&id))
            };
            XVal::Ref(found.map_or(0, |f| f.0))
        }
        "geteditorid" => {
            let f = form(args, 0).or(subject).unwrap_or(FormId(0));
            XVal::Str(if f == FormId::PLAYER_REF {
                "PlayerRef".into()
            } else {
                g.other
                    .edids
                    .get(&f)
                    .or_else(|| g.data.edid_of.get(&f))
                    .cloned()
                    .or_else(|| crate::nvse_jg_nif::camera_overlay_edid(f).map(str::to_string))
                    .unwrap_or_default()
            })
        }
        "clamp" => XVal::Num(num(args, 0, 0.0).clamp(num(args, 1, 0.0), num(args, 2, 0.0).max(num(args, 1, 0.0)))),
        // [documented: GetDistance3D (LU); observed: Doc's door to his dresser 828.363]
        // the straight-line distance from the calling reference to another; FLT_MAX when
        // either is missing [documented].
        "getdistance3d" => {
            let pos = |g: &mut Game, r: Option<FormId>| r.and_then(|r| g.ref_pos_rot(r)).map(|(p, _)| p);
            match (pos(g, subject), pos(g, form(args, 0))) {
                (Some(p), Some(q)) => XVal::Num(f64::from(((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt())),
                _ => XVal::Num(f64::from(f32::MAX)),
            }
        }
        // [documented: GetGameDaysPassed (JIP)] the game days since a date (by default
        // 13 October 2281), the hour's fraction included.
        "getgamedayspassed" => {
            let Ok(c) = g.world.calendar() else { return Some(Ok(XVal::Num(0.0))) };
            let now = day_number(c.year as i64, c.month as i64, c.day as i64) as f64 + f64::from(c.hour) / 24.0;
            let (y, m, d) = if args.is_empty() { (2281, 10, 13) } else { (num(args, 0, 2281.0) as i64, num(args, 1, 1.0) as i64, num(args, 2, 1.0) as i64) };
            XVal::Num(now - day_number(y, m - 1, d) as f64)
        }
        // [documented: SetGameHour] the clock moves forward to that hour (the next day's
        // when it has passed today), the other time globals with it.
        "setgamehour" => {
            let Ok(c) = g.world.calendar() else { return Some(Ok(XVal::Num(0.0))) };
            let to = num(args, 0, f64::from(c.hour));
            if let Some(hours) = g.world.advance_to_hour(to).ok().filter(|h| *h > 0.0) {
                g.pass_hours(hours, false);
                // A jump of the clock, for the engine's actors (#2353).
                if let Ok(now) = g.world.calendar() {
                    g.note_hour_jump(c.hour, now.hour);
                }
            }
            XVal::Num(0.0)
        }
        // [documented: AddItemAlt] `item quantity [condition 0–100, negative: default]
        // [equip] [noMessage]` [observed: condition 0.5 gives 0.4 of a pistol's 80, the
        // value a percent; equip 1 equips it].
        "additemalt" => {
            let (Some(to), Some(item)) = (subject, form(args, 0)) else { return Some(Ok(XVal::Num(0.0))) };
            let n = num(args, 1, 1.0).max(1.0) as i32;
            let pct = num(args, 2, -1.0);
            let cond = if pct >= 0.0 { Some((pct / 100.0) as f32) } else { None };
            if let Err(e) = g.world.add_item_at_condition(to, item, n, cond) {
                return Some(Err(e.to_string()));
            }
            if num(args, 3, 0.0) != 0.0 {
                let _ = crate::functions::console::call_words(g, Some(to), "EquipItem", &[format!("{:08X}", item.0), "0".into(), "1".into()], "AddItemAlt");
            }
            XVal::Num(0.0)
        }
        // [documented: RemoveItemTarget] `item target [quantity, ≤0 all]`: the items move
        // to the target with their state [observed: 2 stimpaks moved, Doc 2 to 4].
        "removeitemtarget" => {
            let (Some(from), Some(item), Some(to)) = (subject, form(args, 0), form(args, 1)) else { return Some(Ok(XVal::Num(0.0))) };
            let have = g.world.inventory_mut(from).map(|inv| inv.count(item)).unwrap_or(0);
            let q = num(args, 2, 0.0) as i32;
            let n = if q <= 0 { have } else { q.min(have) };
            if n > 0 {
                let moved = g.world.remove_item(from, item, n).unwrap_or(0);
                if moved > 0 {
                    let _ = g.world.add_item(to, item, moved);
                }
            }
            XVal::Num(0.0)
        }
        // [documented: EquipItemAlt] EquipItem with OnEquip run.
        "equipitemalt" => {
            let (Some(a), Some(item)) = (subject, form(args, 0)) else { return Some(Ok(XVal::Num(0.0))) };
            let words: Vec<String> = std::iter::once(format!("{:08X}", base(g, item).0)).chain(args.iter().skip(1).map(|v| format!("{}", v.num().unwrap_or(0.0) as i64))).collect();
            if let Err(e) = crate::functions::console::call_words(g, Some(a), "EquipItem", &words, "EquipItemAlt") {
                return Some(Err(e));
            }
            XVal::Num(0.0)
        }
        // [documented: GetWeaponRefModFlags; GetEquippedWeaponModFlags] a weapon's mods:
        // 1, 2, 4 for mods 1–3.
        "getweaponrefmodflags" => match subject.and_then(|r| g.temp_item(r)) {
            Some((c, it)) => XVal::Num(f64::from(g.world.inventory_mut(c).map(|inv| inv.groups().find(|(i, _, _)| *i == it).map_or(0, |(_, e, _)| e.mods)).unwrap_or(0))),
            None => XVal::Num(0.0),
        },
        "getequippedweaponmodflags" => {
            let a = subject.unwrap_or(FormId(0));
            let w = crate::functions::items::equipped_weapon(g, a);
            XVal::Num(f64::from(w.map_or(0, |w| g.world.inventory_mut(a).map(|inv| inv.equipped_mods(w)).unwrap_or(0))))
        }
        _ => return None,
    };
    Some(Ok(v))
}
