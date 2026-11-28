//! More of xNVSE's functions on the game (docs/handoff/xnvse-remaining.md): item and
//! form tests, game settings, doors, form-list editing by replacement, and a few
//! actor and quest lookups [documented: their pages].

use crate::game::Game;
use fnv_script::vm::{ArrayKind, XKey, XVal};
use fnv_sim::FormId;

/// Fonts 1-8 as Fallout_default.ini names them [documented: Fallout_default.ini, [Fonts]
/// sFontFile_1 … sFontFile_8].
const DEFAULT_FONTS: [&str; 8] = [
    "Textures\\Fonts\\Glow_Monofonto_Large.fnt",
    "Textures\\Fonts\\Monofonto_Large.fnt",
    "Textures\\Fonts\\Glow_Monofonto_Medium.fnt",
    "Textures\\Fonts\\Monofonto_VeryLarge02_Dialogs2.fnt",
    "Textures\\Fonts\\Fixedsys_Comp_uniform_width.fnt",
    "Textures\\Fonts\\Glow_Monofonto_VL_dialogs.fnt",
    "Textures\\Fonts\\Baked-in_Monofonto_Large.fnt",
    "Textures\\Fonts\\Glow_Futura_Caps_Large.fnt",
];

fn form(args: &[XVal], k: usize) -> Option<FormId> {
    match args.get(k)? {
        XVal::Ref(0) => None,
        XVal::Ref(r) => Some(FormId(*r)),
        XVal::Num(x) if *x >= 1.0 && x.fract() == 0.0 => Some(FormId(*x as u32)),
        _ => None,
    }
}

fn int(args: &[XVal], k: usize, dflt: i64) -> i64 {
    match args.get(k) {
        Some(XVal::Num(x)) => *x as i64,
        _ => dflt,
    }
}

fn text(args: &[XVal], k: usize) -> String {
    match args.get(k) {
        Some(XVal::Str(s)) => s.clone(),
        _ => String::new(),
    }
}

/// A game setting's name, as a setting function's first argument: the text, or, written
/// without quotes, the setting's own record, whose Editor ID is the name. The compiler
/// reads a bare word that is an Editor ID as that form, and every setting of the load
/// order is a GMST record named for itself [observed: JustAssortedMods.esp 4.6 writes
/// `GetNumericGameSetting fVATSShotLongBurstTime` and its like 17 times, and divides by
/// the result; `fnvtool ext queries` read 1.75 quoted and 0 unquoted until this].
pub(crate) fn setting_name(g: &Game, args: &[XVal], k: usize) -> String {
    match args.get(k) {
        Some(XVal::Str(s)) => s.clone(),
        Some(XVal::Ref(f)) if g.data.sig(FormId(*f)) == Some(*b"GMST") => g.data.edid_of.get(&FormId(*f)).cloned().unwrap_or_default(),
        _ => String::new(),
    }
}

fn yes(b: bool) -> XVal {
    XVal::Num(f64::from(u8::from(b)))
}

/// The functions of this file; `None` for any other name. `subject` is the reference
/// called on, else the calling reference.
pub(crate) fn call(g: &mut Game, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let base = |g: &Game, f: FormId| crate::functions::base_of(g, f).unwrap_or(f);
    // A form argument at `k`, else the calling reference's base form.
    let item = |g: &Game, k: usize| form(args, k).or(subject).map(|f| base(g, f)).unwrap_or(FormId(0));
    let v = match name {
        // [documented: GetBaseForm] the base form of a (leveled) reference.
        "getbaseform" => XVal::Ref(subject.map_or(0, |r| base(g, r).0)),
        // [documented: IsQuestItem; IsPlayable; IsPowerArmor] the item's flags, scripts'
        // SetQuestObject included.
        "isquestitem" => yes(g.world.is_quest_item(item(g, 0))),
        "isplayable" => {
            let it = item(g, 0);
            yes(g.is_item_playable(it))
        }
        "ispowerarmor" => {
            let f = item(g, 0);
            // SetIsPowerArmor and SetEquipmentBipedMask (BMDT bit 5) stay synchronized.
            yes(g.other.power_armor.get(&f).copied().unwrap_or_else(|| g.world.effective_power_armor(f)))
        }
        // [documented: IsModLoaded] a plugin of the load order, by file name.
        "ismodloaded" => {
            let n = text(args, 0);
            yes(g.data.load_order.iter().any(|p| p.eq_ignore_ascii_case(&n)))
        }
        // [documented: GetPluginHeaderVersion] "the header version number of the specified
        // plugin as a float": the TES4 HEDR version (1.34 for FalloutNV.esm [observed:
        // jip-getters headerversion.fnv]); 0 for a plugin not loaded [hypothesis].
        "getpluginheaderversion" => {
            let n = text(args, 0);
            let i = g.data.load_order.iter().position(|p| p.eq_ignore_ascii_case(&n));
            XVal::Num(i.and_then(|i| g.data.plugin_versions.get(i)).map_or(0.0, |&v| f64::from(v)))
        }
        // `fontType`: the font's .fnt path, for fonts 1-8 and 10-89 [documented:
        // GetFontFile], the INI's `sFontFile_N` (category Fonts), Fallout_default.ini's
        // when no INI has it [documented: Fallout_default.ini; hypothesis: the game's
        // defaults are the file's]; 9 and fonts no INI names give "" [observed: jip-forms,
        // fontfile.2 "Textures\Fonts\Monofonto_Large.fnt", fontfile.9 ""].
        "getfontfile" => {
            let n = int(args, 0, 0);
            // SetFontFile's file first (`crate::nvse_jip9`).
            if let Some(f) = g.font_file(n) {
                return Some(Ok(XVal::Str(f.to_string())));
            }
            let from_ini = match crate::nvse_settings::call(g, "getstringinisetting", &[XVal::Str(format!("sFontFile_{n}:Fonts"))]) {
                Some(Ok(XVal::Str(s))) if !s.is_empty() => Some(s),
                _ => None,
            };
            let path = match n {
                1..=8 => from_ini.unwrap_or_else(|| DEFAULT_FONTS[(n - 1) as usize].to_string()),
                10..=89 => from_ini.unwrap_or_default(),
                _ => String::new(),
            };
            XVal::Str(path)
        }
        // [documented: GetRawFormIDString] eight hex digits, valid form or not.
        "getrawformidstring" => XVal::Str(format!(
            "{:08X}",
            match args.first() {
                Some(XVal::Ref(r)) => *r,
                Some(XVal::Num(x)) => *x as u32,
                _ => 0,
            }
        )),
        // [documented: GetNumericGameSetting; GetStringGameSetting; SetNumericGameSetting].
        "getnumericgamesetting" => XVal::Num(g.world.setting_f32(&setting_name(g, args, 0)).map_or(0.0, f64::from)),
        "getstringgamesetting" => XVal::Str(match g.world.setting_value(&setting_name(g, args, 0)) {
            Some(fnv_sim::settings::Setting::Str(s)) => s.clone(),
            _ => String::new(),
        }),
        "setnumericgamesetting" => {
            use fnv_sim::settings::Setting;
            let n = setting_name(g, args, 0);
            let x = match args.get(1) {
                Some(XVal::Num(x)) => *x,
                _ => 0.0,
            };
            let v = if n.to_ascii_lowercase().starts_with('i') { Setting::Int(x as i32) } else { Setting::Float(x as f32) };
            yes(g.world.change_setting(&n, v).is_ok())
        }
        // [documented: GetClass] the actor's class (`Game::traits_class`).
        "getclass" => XVal::Ref(form(args, 0).or(subject).and_then(|f| g.traits_class(f)).map_or(0, |c| c.0)),
        // `actor.SetClass class` (a base-game function scripts reach by name: the
        // catalogue has no opcode for it): the actor's base takes the class, kept in the
        // save (`FormChanges::classes`) [documented: SetClass, "Changes the class of the
        // actor to the parameter"; observed: w4f_04_set_class]. The autocalc the page
        // names for actors with autocalc on is not run: NVForever reads the record's
        // stats. Non-actor and null receivers continue without mutation; a non-CLAS or
        // null class argument aborts the script at runtime leaving the actor's class
        // unchanged [observed: w4g_05_isactordetected_and_setclass_invalid].
        "setclass" => {
            let Some(class) = form(args, 0).filter(|&c| g.data.sig(c) == Some(*b"CLAS")) else {
                return Some(Err("setclass requires a CLAS form".into()));
            };
            let Some(r) = subject else { return Some(Ok(XVal::Num(0.0))) };
            let b = crate::functions::base_of(g, r).unwrap_or(r);
            // Class changes belong to actors, including the actor selected for a
            // placed leveled list; a carrier's signature alone does not classify it.
            if g.template_source(b, r, |_| false).is_some() {
                g.forms.classes.insert(b, class);
            }
            XVal::Num(0.0)
        }
        // [documented: GetRefCount] the count of a stacked reference (1 without one).
        "getrefcount" => {
            let r = subject.unwrap_or(FormId(0));
            // An inventory reference: its container's stack (1 for CreateTempRef's).
            if let Some((c, it)) = g.temp_item(r) {
                let n = if c.0 == 0 { 1 } else { g.world.inventory_mut(c).map(|inv| inv.count(it)).unwrap_or(0) };
                return Some(Ok(XVal::Num(f64::from(n))));
            }
            let n = g.world.peek(r).map(|s| s.count).or_else(|| g.world.placed.get(r).map(|p| p.count.unwrap_or(1))).unwrap_or(0);
            XVal::Num(f64::from(n))
        }
        // Doors [documented: GetLinkedDoor; GetTeleportCell; IsLoadDoor]: the reference's
        // teleport (XTEL).
        "getlinkeddoor" | "getteleportcell" | "isloaddoor" => {
            // SetDoorTeleport's target first [observed: nvse-fifth, GetLinkedDoor follows it].
            let t = subject.and_then(|r| g.door_teleport(r)).map(|t| t.0);
            match name {
                "getlinkeddoor" => XVal::Ref(t.map_or(0, |d| d.0)),
                // An exterior destination gives no cell [observed: nvse-actors, Doc's
                // house door].
                "getteleportcell" => XVal::Ref(t.filter(|&d| g.world.worldspace_of(d).is_none()).and_then(|d| g.cell_of(d)).map_or(0, |c| c.0)),
                _ => yes(t.is_some()),
            }
        }
        // [documented: GetCurrentQuest] the player's active quest.
        "getcurrentquest" => XVal::Ref(g.ui.active_quest.map_or(0, |q| q.0)),
        // [documented: IsRefInList] the exact form's index, else its base form's; -1.
        "isrefinlist" => {
            let m = crate::functions::list_members(g, form(args, 0).unwrap_or(FormId(0)));
            let f = form(args, 1).unwrap_or(FormId(0));
            let b = base(g, f);
            XVal::Num(m.iter().position(|&x| x == f).or_else(|| m.iter().position(|&x| x == b)).map_or(-1.0, |i| i as f64))
        }
        // [documented: GetListForms] the list as an array.
        "getlistforms" => {
            let m = crate::functions::list_members(g, form(args, 0).unwrap_or(FormId(0)));
            let entries = m.into_iter().enumerate().map(|(i, f)| (XKey::Num(i as f64), XVal::Ref(f.0))).collect();
            XVal::Array(g.ext.new_array(ArrayKind::List, entries))
        }
        // [documented: ListReplaceForm] the index replaced, -1 when absent [hypothesis].
        "listreplaceform" => {
            let (l, with, old) = (form(args, 0).unwrap_or(FormId(0)), form(args, 1), form(args, 2));
            let list = g.edited_list(l);
            match (with, old.and_then(|o| list.iter().position(|&x| x == o))) {
                (Some(w), Some(i)) => {
                    list[i] = w;
                    XVal::Num(i as f64)
                }
                _ => XVal::Num(-1.0),
            }
        }
        // [documented: ListReplaceNthForm] the form that was there; without an index the
        // last [hypothesis, as ListRemoveNthForm].
        "listreplacenthform" => {
            let (l, with) = (form(args, 0).unwrap_or(FormId(0)), form(args, 1));
            let list = g.edited_list(l);
            let n = int(args, 2, list.len() as i64 - 1);
            match (with, usize::try_from(n).ok().filter(|&n| n < list.len())) {
                (Some(w), Some(n)) => XVal::Ref(std::mem::replace(&mut list[n], w).0),
                _ => XVal::Ref(0),
            }
        }
        // [documented: ListAddReference] ListAddForm of the calling reference.
        "listaddreference" => {
            let l = form(args, 0).unwrap_or(FormId(0));
            let Some(r) = subject else { return Some(Ok(XVal::Num(-1.0))) };
            let at = int(args, 1, -2);
            let dupes = args.get(2).is_some_and(XVal::truthy);
            let list = g.edited_list(l);
            if dupes && list.contains(&r) {
                return Some(Ok(XVal::Num(-1.0)));
            }
            let i = usize::try_from(at).ok().filter(|&i| i <= list.len()).unwrap_or(list.len());
            list.insert(i, r);
            XVal::Num(i as f64)
        }
        // [documented: PickOneOf] a random item both in the inventory and the list; 0
        // when none.
        "pickoneof" => {
            let m = crate::functions::list_members(g, form(args, 0).unwrap_or(FormId(0)));
            let held: Vec<FormId> = subject
                .and_then(|r| g.world.inventory_mut(r).ok().map(|inv| inv.iter().filter(|(i, n)| *n > 0 && m.contains(i)).map(|(i, _)| i).collect()))
                .unwrap_or_default();
            if held.is_empty() {
                XVal::Ref(0)
            } else {
                use rand_core::RngCore;
                let k = g.rng.next_u32() as usize % held.len();
                XVal::Ref(held[k].0)
            }
        }
        // [documented: ActorValueToString] the actor value's display name: the FULL of its
        // AVIF record, found by Editor ID "AV" + the value's name [observed: AVIF Editor
        // IDs, functions/actor.rs]; "" when none matches.
        "actorvaluetostring" | "actorvaluetostringc" => {
            let code = match args.first() {
                Some(XVal::Num(x)) => usize::try_from(*x as i64).ok(),
                Some(XVal::Str(s)) => crate::functions::av_code(g, s).ok().map(usize::from),
                _ => None,
            };
            // Guns is AVSmallGuns and Survival AVThrowing [observed: functions/actor.rs].
            let edid = code.and_then(|c| crate::functions::actor::AV_NAMES.get(c)).map(|n| match *n {
                "Guns" => "SmallGuns".to_string(),
                "Survival" => "Throwing".to_string(),
                n => n.replace(' ', ""),
            });
            let avif = edid.and_then(|n| g.data.edids.get(&format!("av{}", n.to_ascii_lowercase())));
            XVal::Str(avif.and_then(|f| g.data.full_names.get(f).cloned()).unwrap_or_default())
        }
        // [documented: CompareNames] the two full names compared without regard to case:
        // -1, 0 or 1 [hypothesis: the sign convention].
        "comparenames" => {
            let a = form(args, 1).or(subject).map(|f| base(g, f));
            let b = form(args, 0).map(|f| base(g, f));
            let name_of = |f: Option<FormId>| f.and_then(|f| g.name_overrides.get(&f).or_else(|| g.forms.saved_names.get(&f)).or_else(|| g.data.full_names.get(&f)).cloned()).unwrap_or_default().to_lowercase();
            XVal::Num(match name_of(a).cmp(&name_of(b)) {
                std::cmp::Ordering::Less => -1.0,
                std::cmp::Ordering::Equal => 0.0,
                std::cmp::Ordering::Greater => 1.0,
            })
        }
        _ => return None,
    };
    Some(Ok(v))
}
