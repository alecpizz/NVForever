//! JohnnyGuitar NVSE, batch 3 (#1007): effect shaders' traits and textures, media sets'
//! traits, casinos' chips and winnings, terminals' menu items and quests' script delay
//! [documented: each page in the wiki export; docs/spec-notes/ext-jg3.md]. Setters
//! change the record for the session, read by the getters.

use crate::game::{Game, RunCtx};
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

fn text(args: &[XVal], k: usize) -> String {
    match args.get(k) {
        Some(XVal::Str(s)) => s.clone(),
        _ => String::new(),
    }
}

/// A whole number at `k`, else none.
fn index(args: &[XVal], k: usize) -> Option<i64> {
    match args.get(k) {
        Some(XVal::Num(x)) if x.fract() == 0.0 => Some(*x as i64),
        _ => None,
    }
}

/// What an effect shader trait reads.
#[derive(Clone, Copy, PartialEq)]
enum Trait {
    /// An EFSH DATA number; true for the blend and Z test enums.
    Field(&'static str, bool),
    /// An EFSH DATA colour, as one number: red × 65536 + green × 256 + blue [observed: jg3
    /// efsh.trait.nuke.4 (75, 222, 39) read 4.97207e+06, efsh.trait.nuke.14 (154, 222,
    /// 131) 1.01495e+07, efsh.trait.cateye.14 (4, 255, 4) 327428].
    Colour(&'static str),
}

/// An effect shader trait 0-60 [documented: GetEffectShaderTraitNumeric's table, 0-46]:
/// the EFSH DATA field. Traits 47-52 read the next DATA fields: colour keys 1-3 and
/// colour keys 1-3 alpha [observed: jg3 efsh.trait.nuke.47 read 1.43523e+07; Fallout:
/// New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528, clean-room script probe
/// jg_03_efsh_trait_beyond_47: (47, 48, 50, -1) -> 1.43523e+07, 8.84712e+06, 1, 0;
/// observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528,
/// 2026-11-24, GSDocMitchellHouse (0x00103DF9), 10 official base masters,
/// w4d_30_jg_efsh_47_to_52 (tools/capture/nvse/catalogs/wave4d_extenders.json):
/// get(47=1.43523e+07,48=8.84712e+06,49=9.07549e+06,50=1,51=1,52=1)|set(49:1->1.12233e+06,51:1->0.5,52:1->0.25);
/// no DLL or source consulted]. Traits 53-60 map to colour key 1-3 times, initial speed
/// along normal range, initial rotation/range, and rotation speed/range (see
/// docs/spec-notes/ext-jg3.md). Traits `< 0` and `> 60` return 0 [design: safe out-of-range
/// guard; -1 also observed returning 0 on getter in jg3 and jg_03].
fn efsh_trait(id: i64) -> Option<Trait> {
    match id {
        4 => return Some(Trait::Colour("fill_color")),
        14 => return Some(Trait::Colour("edge_color")),
        47 => return Some(Trait::Colour("color_key_1")),
        48 => return Some(Trait::Colour("color_key_2")),
        49 => return Some(Trait::Colour("color_key_3")),
        50 => return Some(Trait::Field("color_key_1_alpha", false)),
        51 => return Some(Trait::Field("color_key_2_alpha", false)),
        52 => return Some(Trait::Field("color_key_3_alpha", false)),
        53 => return Some(Trait::Field("color_key_1_time", false)),
        54 => return Some(Trait::Field("color_key_2_time", false)),
        55 => return Some(Trait::Field("color_key_3_time", false)),
        56 => return Some(Trait::Field("initial_speed_along_normal_range", false)),
        57 => return Some(Trait::Field("initial_rotation_deg", false)),
        58 => return Some(Trait::Field("initial_rotation_deg_range", false)),
        59 => return Some(Trait::Field("rotation_speed_deg", false)),
        60 => return Some(Trait::Field("rotation_speed_deg_range", false)),
        _ => {}
    }
    const NAMES: [&str; 47] = [
        "flags",
        "membrane_source_blend",
        "membrane_blend_op",
        "membrane_z_test",
        "",
        "fill_alpha_fade_in_time",
        "fill_full_alpha_time",
        "fill_alpha_fade_out_time",
        "fill_persistent_alpha_ratio",
        "fill_alpha_pulse_amplitude",
        "fill_alpha_pulse_frequency",
        "fill_texture_speed_u",
        "fill_texture_speed_v",
        "edge_fall_off",
        "",
        "edge_alpha_fade_in_time",
        "edge_full_alpha_time",
        "edge_alpha_fade_out_time",
        "edge_persistent_alpha_ratio",
        "edge_alpha_pulse_amplitude",
        "edge_alpha_pulse_frequency",
        "fill_full_alpha_ratio",
        "edge_full_alpha_ratio",
        "membrane_dest_blend",
        "particle_source_blend",
        "particle_blend_op",
        "particle_z_test",
        "particle_dest_blend",
        "particle_birth_ramp_up_time",
        "full_particle_birth_time",
        "particle_birth_ramp_down_time",
        "full_particle_birth_ratio",
        "persistent_particle_birth_ratio",
        "particle_lifetime",
        "particle_lifetime_range",
        "initial_speed_along_normal",
        "acceleration_along_normal",
        "initial_velocity_x",
        "initial_velocity_y",
        "initial_velocity_z",
        "acceleration_x",
        "acceleration_y",
        "acceleration_z",
        "scale_key_1",
        "scale_key_2",
        "scale_key_1_time",
        "scale_key_2_time",
    ];
    let name = *NAMES.get(usize::try_from(id).ok()?)?;
    (!name.is_empty()).then_some(Trait::Field(name, matches!(id, 1..=3 | 23..=27)))
}

/// The game's "script call failed": worded as the VM's NoReference, which stops the
/// script (as `nvse_jg5` does).
fn failed(name: &str) -> Option<Result<XVal, String>> {
    Some(Err(fnv_script::vm::VmError::NoReference(name.to_string()).to_string()))
}

/// An effect shader texture 0-2: fill (ICON), particle shader (ICO2), holes (NAM7)
/// [documented: GetEffectShaderTexturePath's table; the subrecords: xEdit EFSH].
fn efsh_texture(id: i64) -> Option<&'static str> {
    ["ICON", "ICO2", "NAM7"].get(usize::try_from(id).ok()?).copied()
}

/// A media set's numeric trait 0-17 [documented: GetMediaSetTraitNumeric's table, each
/// with its subrecord].
fn mset_numeric(id: i64) -> Option<&'static str> {
    ["NAM1", "NAM8", "NAM9", "NAM0", "ANAM", "BNAM", "CNAM", "JNAM", "KNAM", "LNAM", "MNAM", "NNAM", "ONAM", "PNAM", "DNAM", "ENAM", "FNAM", "GNAM"]
        .get(usize::try_from(id).ok()?)
        .copied()
}

/// A record's number: a number or a flags value (the record's enums decode as their
/// numbers, `GameData::record_fields`).
fn number(v: Option<XVal>) -> f64 {
    match v {
        Some(XVal::Num(x)) => x,
        Some(XVal::Ref(r)) => f64::from(r),
        _ => 0.0,
    }
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let field = |g: &Game, f: FormId, sub: &str, path: &str| crate::nvse::record_value(g, f, sub, path);
    let of_sig = |g: &Game, f: Option<FormId>, sig: &[u8; 4]| f.filter(|&f| g.data.sig(f) == Some(*sig));
    let zero = XVal::Num(0.0);
    let empty = XVal::Str(String::new());
    let v = match name {
        // `shader trait [value]`: the EFSH DATA field [documented: Get/
        // SetEffectShaderTraitNumeric]; the setter returns 1 when done [documented:
        // "returning 1 on success"] and takes every trait the getter reads, not only the
        // 0-42 its page lists [observed: jg3 efsh.set.nuke.43 and efsh.set.nuke.47 returned
        // 1 and read the new value back; w4d_30_jg_efsh_47_to_52 confirmed setter round-trips
        // for 49, 51, 52].
        "geteffectshadertraitnumeric" | "seteffectshadertraitnumeric" => {
            let set = name.starts_with("set");
            let (Some(s), Some(t)) = (of_sig(g, form(args, 0), b"EFSH"), index(args, 1).and_then(efsh_trait)) else { return Some(Ok(zero)) };
            let v = num(args, 2, 0.0);
            match (t, set) {
                (Trait::Field(path, by_name), true) => {
                    // A flag byte and the enums keep whole numbers [observed: jg3
                    // efsh.set.nuke.0 (2.7 read 2) and efsh.set.nuke.1 (5.5 read 5)].
                    let v = if path == "flags" || by_name { v.trunc() } else { v };
                    g.field_overrides.insert((s, format!("DATA/{path}")), XVal::Num(v));
                    XVal::Num(1.0)
                }
                (Trait::Field(path, _), false) => XVal::Num(number(field(g, s, "DATA", path))),
                // A colour set as one number reads back as given [observed: jg3
                // efsh.set.nuke.4, 255 read 255]; kept to the three channels' 24 bits
                // (see docs/spec-notes/ext-jg3.md).
                (Trait::Colour(path), true) => {
                    g.field_overrides.insert((s, format!("DATA/{path}")), XVal::Num(v.trunc().clamp(0.0, f64::from(0xFF_FFFF))));
                    XVal::Num(1.0)
                }
                (Trait::Colour(path), false) => match g.field_overrides.get(&(s, format!("DATA/{path}"))) {
                    Some(v) => v.clone(),
                    None => {
                        let ch = |c: &str| number(field(g, s, "DATA", &format!("{path}.{c}")));
                        XVal::Num(ch("red") * 65536.0 + ch("green") * 256.0 + ch("blue"))
                    }
                },
            }
        }
        // `shader id [path]`: 0 fill, 1 particle, 2 holes [documented: Get/
        // SetEffectShaderTexturePath].
        "geteffectshadertexturepath" | "seteffectshadertexturepath" => {
            let sub = index(args, 1).and_then(efsh_texture);
            // An id past the table fails the call [observed: jg3 efsh.tex.nuke.3 and
            // efsh.tex.nuke.-1, "script call failed"].
            if name.starts_with("get") && sub.is_none() && of_sig(g, form(args, 0), b"EFSH").is_some() {
                return failed(name);
            }
            let (Some(s), Some(sub)) = (of_sig(g, form(args, 0), b"EFSH"), sub) else {
                return Some(Ok(if name.starts_with("get") { empty } else { zero }));
            };
            if name.starts_with("set") {
                g.field_overrides.insert((s, format!("{sub}/")), XVal::Str(text(args, 2)));
                zero
            } else {
                XVal::Str(match field(g, s, sub, "") {
                    Some(XVal::Str(p)) => p,
                    _ => String::new(),
                })
            }
        }
        // `set trait [value]` [documented: Get/SetMediaSetTraitNumeric, traits 0-17].
        "getmediasettraitnumeric" | "setmediasettraitnumeric" => {
            let (Some(m), Some(sub)) = (of_sig(g, form(args, 0), b"MSET"), index(args, 1).and_then(mset_numeric)) else { return Some(Ok(zero)) };
            if name.starts_with("set") {
                let v = num(args, 2, 0.0);
                let v = if matches!(sub, "NAM1" | "PNAM") { v.trunc() } else { v };
                g.field_overrides.insert((m, format!("{sub}/")), XVal::Num(v));
                zero
            } else {
                XVal::Num(number(field(g, m, sub, "")))
            }
        }
        // `set trait [path]`: NAM2-NAM7 [documented: Get/SetMediaSetTraitString, 0-5].
        "getmediasettraitstring" | "setmediasettraitstring" => {
            let sub = index(args, 1).filter(|t| (0..=5).contains(t)).map(|t| format!("NAM{}", t + 2));
            // A trait past the table fails the call [observed: jg3 mset.str.corp.6 and
            // mset.str.corp.-1, "script call failed"].
            if name.starts_with("get") && sub.is_none() && of_sig(g, form(args, 0), b"MSET").is_some() {
                return failed(name);
            }
            let (Some(m), Some(sub)) = (of_sig(g, form(args, 0), b"MSET"), sub) else {
                return Some(Ok(if name.starts_with("get") { empty } else { zero }));
            };
            if name.starts_with("set") {
                g.field_overrides.insert((m, format!("{sub}/")), XVal::Str(text(args, 2)));
                zero
            } else if let Some(v) = g.field_overrides.get(&(m, format!("{sub}/"))) {
                // A path a script set reads back as given [observed: jg3 mset.setstr.corp.0].
                v.clone()
            } else {
                // The record's path reads under `Data\Music\`, the folder the game plays
                // it from; none reads "" [observed: jg3 mset.str.corp.0-5 and
                // mset.str.dung.0/3 read `Data\Music\` and the record's path,
                // mset.str.dung.1 ""].
                XVal::Str(match field(g, m, &sub, "") {
                    Some(XVal::Str(p)) if !p.is_empty() => format!("Data\\Music\\{p}"),
                    _ => String::new(),
                })
            }
        }
        // `set trait [sound]`: 0 HNAM, 1 INAM [documented: Get/SetMediaSetTraitSound].
        "getmediasettraitsound" | "setmediasettraitsound" => {
            let sub = match index(args, 1) {
                Some(0) => "HNAM",
                Some(1) => "INAM",
                _ => return Some(Ok(if name.starts_with("get") { XVal::Ref(0) } else { zero })),
            };
            let Some(m) = of_sig(g, form(args, 0), b"MSET") else {
                return Some(Ok(if name.starts_with("get") { XVal::Ref(0) } else { zero }));
            };
            if name.starts_with("set") {
                // A sound form only: none and other forms change nothing [observed: jg3
                // mset.setsound.bttl.none and mset.setsound.bttl.notsound].
                if let Some(s) = of_sig(g, form(args, 2), b"SOUN") {
                    g.field_overrides.insert((m, format!("{sub}/")), XVal::Ref(s.0));
                }
                zero
            } else {
                XVal::Ref(match field(g, m, sub, "") {
                    Some(XVal::Ref(r)) => r,
                    _ => 0,
                })
            }
        }
        // `casino [chip]`: the casino's chip, DATA's currency [documented: Get/
        // SetCasinoChip; the field: xEdit CSNO]; the setter takes a casino chip form.
        "getcasinochip" => XVal::Ref(form(args, 0).and_then(|c| crate::casino::chip(g, c)).map_or(0, |c| c.0)),
        "setcasinochip" => {
            if let (Some(c), Some(chip)) = (form(args, 0).filter(|c| g.data.casinos.contains_key(c)), of_sig(g, form(args, 1), b"CHIP")) {
                g.other.casino_chips.insert(c, chip);
            }
            zero
        }
        // `casino [chips]`: the chips the player has won at the casino, the net of its
        // games (`crate::casino::add_earnings`) [documented: Get/SetCasinoWinnings]; the
        // setter also sets the winnings level the casino's scripts read.
        "getcasinowinnings" => XVal::Num(form(args, 0).and_then(|c| g.player.casino_earnings.get(&c)).map_or(0.0, |&e| e as f64)),
        "setcasinowinnings" => {
            if let Some(c) = form(args, 0).filter(|c| g.data.casinos.contains_key(c)) {
                let max = g.data.casinos[&c].max_winnings;
                // The earnings set outright (`as` saturates a huge number), then the level
                // worked out as after a round, so no difference can overflow (#1328 review).
                g.player.casino_earnings.insert(c, num(args, 1, 0.0).trunc() as i64);
                crate::casino::add_earnings(g, c, 0, max);
            }
            zero
        }
        // `terminal`: the number of its menu items [documented: GetTerminalMenuItemCount].
        "getterminalmenuitemcount" => XVal::Num(form(args, 0).filter(|t| g.data.terminals.contains_key(t)).map_or(0.0, |t| crate::terminal::item_count(g, t) as f64)),
        // `terminal textID item [text]`: 1 the item's line (ITXT), 2 its result (RNAM)
        // [documented: Get/SetTerminalMenuItemText].
        "getterminalmenuitemtext" | "setterminalmenuitemtext" => {
            let set = name.starts_with("set");
            // textID 0 fails the call; 3 reads "" here as for an item without text
            // [observed: jg3 term.text.v34.0.0 "script call failed", term.text.v34.3.0 ""].
            if !set && index(args, 1) == Some(0) {
                return failed(name);
            }
            let (Some(t), Some(which), Some(i)) = (form(args, 0), index(args, 1).filter(|w| matches!(w, 1 | 2)), index(args, 2)) else {
                return Some(Ok(if set { zero } else { empty }));
            };
            let Some(item) = usize::try_from(i).ok().and_then(|i| crate::terminal::item(g, t, i).map(|it| (i, it))) else {
                return Some(Ok(if set { zero } else { empty }));
            };
            if set {
                let edit = g.other.terminal_items.entry((t, item.0)).or_default();
                if which == 1 {
                    edit.text = Some(text(args, 3));
                } else {
                    edit.result = Some(text(args, 3));
                }
                zero
            } else {
                XVal::Str(if which == 1 { item.1.text } else { item.1.result })
            }
        }
        // `terminal item [form]`: the note it shows (INAM), the terminal it opens (TNAM),
        // its flags (ANAM: 1 Add Note, 2 Force Redraw) [documented: Get/
        // SetTerminalMenuItemNote, …Submenu, …Flags].
        "getterminalmenuitemnote" | "getterminalmenuitemsubmenu" | "getterminalmenuitemflags" => {
            let it = form(args, 0).zip(index(args, 1).and_then(|i| usize::try_from(i).ok())).and_then(|(t, i)| crate::terminal::item(g, t, i));
            match name {
                "getterminalmenuitemnote" => XVal::Ref(it.and_then(|it| it.note).map_or(0, |n| n.0)),
                "getterminalmenuitemsubmenu" => XVal::Ref(it.and_then(|it| it.submenu).map_or(0, |n| n.0)),
                _ => XVal::Num(it.map_or(0.0, |it| f64::from(it.flags))),
            }
        }
        "setterminalmenuitemnote" | "setterminalmenuitemsubmenu" | "setterminalmenuitemflags" => {
            let Some((t, i)) = form(args, 0).zip(index(args, 1).and_then(|i| usize::try_from(i).ok())).filter(|&(t, i)| crate::terminal::item(g, t, i).is_some()) else {
                return Some(Ok(zero));
            };
            match name {
                "setterminalmenuitemnote" => {
                    // A note, or none [hypothesis: anything else is refused].
                    let n = form(args, 2);
                    if n.is_none() || of_sig(g, n, b"NOTE").is_some() {
                        g.other.terminal_items.entry((t, i)).or_default().note = Some(n);
                    }
                }
                "setterminalmenuitemsubmenu" => {
                    let s = form(args, 2);
                    if s.is_none() || of_sig(g, s, b"TERM").is_some() {
                        g.other.terminal_items.entry((t, i)).or_default().submenu = Some(s);
                    }
                }
                _ => {
                    // The flags are kept whole, not only the page's two bits [observed: jg3
                    // term.setflags.east.0.7 read 7, term.setflags.v34.4.6 read 6]; a byte, as
                    // ANAM is [hypothesis].
                    g.other.terminal_items.entry((t, i)).or_default().flags = Some((num(args, 2, 0.0) as i64).clamp(0, 255) as u8);
                }
            }
            zero
        }
        // `quest`: the quest script's delay, SetQuestDelay's first, else the record's as
        // it stands: 0 stays 0, not the 5 s the quest then runs at [documented:
        // GetQuestDelay; observed: jg3 quest.delay.vcg02 read 0]; a DATA without a delay
        // reads 0 [hypothesis].
        "getquestdelay" => {
            let Some(q) = form(args, 0).filter(|q| g.data.quests.contains_key(q)) else { return Some(Ok(zero)) };
            XVal::Num(g.quests.get(&q).and_then(|s| s.delay).unwrap_or_else(|| g.data.quests[&q].delay.map_or(0.0, f64::from)))
        }
        _ => return None,
    };
    let _ = (ctx, subject);
    Some(Ok(v))
}
