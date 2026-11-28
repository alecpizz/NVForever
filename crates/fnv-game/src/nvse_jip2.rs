//! JIP LN / LU / JohnnyGuitar / ShowOff functions of the next batch (N7, N8, N9 of
//! docs/handoff/nvse-next-batch.md) [documented: their pages; observed: the peer's
//! player-state, jip-n7 and plugin-odds sets in the original game].

use crate::game::{Game, RunCtx};
use fnv_script::vm::{ArrayKind, XKey, XVal};
use fnv_sim::FormId;
use std::collections::{HashMap, HashSet};

/// The gamepad as the engine tells it each frame, and what scripts changed of it
/// [documented: the Controller Functions pages; observed: player-state, buttons are a
/// bit mask and Enable/DisableButton take several at once]. Button bits: 4 is DPad Left
/// [documented: IsButtonPressed]; the rest in the same pad's order, 1 up, 2 down, 8
/// right, 0x10 Start, 0x20 Back, 0x40/0x80 the sticks, 0x100/0x200 the shoulders,
/// 0x1000 A, 0x2000 B, 0x4000 X, 0x8000 Y [hypothesis: the probes used 4096 and 8192 as
/// buttons].
#[derive(Debug, Default, Clone)]
pub struct Pad {
    pub connected: bool,
    pub buttons: u32,
    /// Left, right, 0–1.
    pub triggers: [f32; 2],
    /// Left x, y; right x, y; −1–1.
    pub sticks: [f32; 4],
    pub disabled_buttons: u32,
    pub disabled_triggers: [bool; 2],
    pub disabled_sticks: [bool; 2],
    pub held_buttons: u32,
    pub held_triggers: [bool; 2],
    /// The dead zones SetDeadZoneLS/RS set (`None`: the engine's own).
    pub dead_zones: [Option<f32>; 2],
}

/// What the engine tells about the player each frame for these functions.
#[derive(Debug, Default, Clone)]
pub struct PlayerTold {
    pub auto_move: bool,
    /// Block held with a gun out (aiming down the sights).
    pub aiming: bool,
    /// The sights are up now: a gun with sights in hand, ready, Block held, and no reload
    /// or jam holding them down (the engine's `Combat::player_sights_up`); false while a
    /// melee weapon blocks. For SUP's HUD bars' aiming condition (`crate::ui_hudbar`).
    pub sights_up: bool,
    /// A kill camera (`nvf_play::kill_cam::KillCam::kill_playing`) currently has the view.
    pub kill_cam: bool,
    pub pad: Pad,
}

/// This batch's session state.
#[derive(Debug, Default)]
pub struct Jip2State {
    pub told: PlayerTold,
    /// AssignKeyword's keywords, lower case, by base form.
    pub keywords: HashMap<FormId, HashSet<String>>,
    /// SetHardcoreStageThreshold's thresholds.
    pub stage_thresholds: HashMap<FormId, u32>,
    pub hardcore_tracking_off: bool,
    /// SetNthPerkEntryValue1's values: (perk, entry index).
    pub perk_values: HashMap<(FormId, usize), f32>,
    /// SetLinkedReference's links (`None`: cleared back to the record's).
    pub links: HashMap<FormId, FormId>,
    /// Actors GetActorVelocity follows: last position and the velocity from it.
    pub velocity: HashMap<FormId, ([f32; 3], [f32; 3])>,
    pub velocity_time: f64,
    /// ShowOff's aux timers: (owner, name lower case) -> (seconds left, started at, paused).
    pub timers: HashMap<(FormId, String), (f64, f64, bool)>,
    /// JohnnyGuitar's extra misc stats.
    pub misc_stats: HashMap<String, f64>,
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

impl Game<'_> {
    /// Whether JIP LN's `IsInCharGen` reports the player as in character generation:
    /// `CharGenMenu` / TagSkills (`1048`), `LoveTesterMenu` (`1074`), or `TraitMenu` (`1084`)
    /// is open — NOT bare `SetInCharGen 1`, `ShowNameMenu` (`1051`), or
    /// `ShowRaceMenu`/`ShowBarberMenu`/`ShowPlasticSurgeonMenu` (`1036`)
    /// [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30,
    /// 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, probes
    /// `w4c_a02_set_in_chargen_menus`, `w4c_a02_chargen_all_menus`, `w4c_a02b_set_in_chargen_xp`
    /// (`wave4c_extenders.json`), and `w4d_01_tagskills_chargen` (`wave4d_extenders.json`)].
    pub fn is_in_chargen(&self) -> bool {
        use crate::game::menus;
        self.menu_open(menus::TAG_SKILLS)
            || self.menu_open(menus::LOVE_TESTER)
            || self.menu_open(menus::TRAITS)
    }

    /// Whether `form` (its base form, or `TempCloneForm` origin) carries JIP keyword
    /// `keyword` (case-insensitive, shared by `HasKeyword` and `HasKeywordCond`)
    /// [documented: GECK wiki HasKeyword, HasKeywordCond; observed: jip-n7].
    pub fn has_keyword(&self, form: FormId, keyword: &str) -> bool {
        if keyword.is_empty() {
            return false;
        }
        let k = keyword.to_ascii_lowercase();
        let b = crate::functions::base_of(self, form).unwrap_or(form);
        if self.jip2.keywords.get(&b).is_some_and(|s| s.contains(&k)) {
            return true;
        }
        let orig = self.unclone(b);
        orig != b && self.jip2.keywords.get(&orig).is_some_and(|s| s.contains(&k))
    }

    /// GetActorVelocity's followed actors move on (`Game::tick`).
    pub(crate) fn track_velocity(&mut self) {
        let dt = self.time - self.jip2.velocity_time;
        self.jip2.velocity_time = self.time;
        if self.jip2.velocity.is_empty() || dt <= 0.0 {
            return;
        }
        let actors: Vec<FormId> = self.jip2.velocity.keys().copied().collect();
        for a in actors {
            let Some((p, _)) = self.ref_pos_rot(a) else { continue };
            if let Some((last, v)) = self.jip2.velocity.get_mut(&a) {
                *v = [0, 1, 2].map(|i| ((p[i] - last[i]) as f64 / dt) as f32);
                *last = p;
            }
        }
    }

    /// Put a reference at a place, as MoveTo does (`functions::refs::move_ref`).
    pub(crate) fn place_ref(&mut self, r: FormId, cell: Option<FormId>, pos: [f32; 3], rot: [f32; 3]) -> bool {
        let cell = cell.map(|c| self.world.placed.exterior_cell_at(c, pos).unwrap_or(c));
        let Ok(s) = self.world.get(r) else { return false };
        s.cell = cell;
        s.pos = pos;
        if r != FormId::PLAYER_REF {
            s.rot = rot;
        }
        if self.ref_script(r).is_some() {
            self.moved.insert(r);
        }
        if r != FormId::PLAYER_REF {
            self.teleported.insert(r);
            if !crate::ai::is_actor_base(self.world.forms, crate::functions::base_of(self, r).unwrap_or(FormId(0))) {
                self.turned.insert(r);
            }
        }
        true
    }

    fn aux_timer_now(&self) -> f64 {
        self.time
    }

    /// Each frame (`Game::tick`): a running aux timer whose time is up stops, is erased and
    /// fires its stop handlers [observed: showoff1, a 0.1 s timer reads time left -1 and
    /// its handler had run 30 frames later].
    pub(crate) fn expire_aux_timers(&mut self) {
        self.aux_timer_updates();
        if self.jip2.timers.is_empty() {
            return;
        }
        let now = self.aux_timer_now();
        let mut done: Vec<(FormId, String)> = self.jip2.timers.iter().filter(|(_, t)| !t.2 && now - t.1 >= t.0).map(|(k, _)| k.clone()).collect();
        done.sort();
        for k in done {
            self.jip2.timers.remove(&k);
            self.showoff.countdowns.remove(&k);
            self.aux_timer_stopped(k.0, &k.1);
        }
    }
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let base = |g: &Game, f: FormId| crate::functions::base_of(g, f).unwrap_or(f);
    let zero = XVal::Num(0.0);
    let pad = g.jip2.told.pad.clone();
    let v = match name {
        // ---- the gamepad (N8) ----
        "getcontroller" => yes(pad.connected),
        "getpressedbuttons" => XVal::Num(f64::from(pad.buttons | pad.held_buttons)),
        "isbuttonpressed" => {
            let m = num(args, 0, 0.0) as i64 as u32;
            yes(m != 0 && (pad.buttons | pad.held_buttons) & m != 0)
        }
        "getlefttrigger" => XVal::Num(f64::from(if pad.held_triggers[0] { 1.0 } else { pad.triggers[0] })),
        "getrighttrigger" => XVal::Num(f64::from(if pad.held_triggers[1] { 1.0 } else { pad.triggers[1] })),
        "getleftstickx" => XVal::Num(f64::from(pad.sticks[0])),
        "getleftsticky" => XVal::Num(f64::from(pad.sticks[1])),
        "getrightstickx" => XVal::Num(f64::from(pad.sticks[2])),
        "getrightsticky" => XVal::Num(f64::from(pad.sticks[3])),
        "disablebutton" | "enablebutton" => {
            let m = num(args, 0, 0.0) as i64 as u32;
            let d = &mut g.jip2.told.pad.disabled_buttons;
            if name == "disablebutton" { *d |= m } else { *d &= !m }
            zero
        }
        "isbuttondisabled" => yes(pad.disabled_buttons & (num(args, 0, 0.0) as i64 as u32) != 0),
        "holdbutton" | "releasebutton" | "tapbutton" => {
            let m = num(args, 0, 0.0) as i64 as u32;
            let h = &mut g.jip2.told.pad.held_buttons;
            if name == "releasebutton" { *h &= !m } else { *h |= m }
            zero
        }
        "isbuttonheld" => yes(pad.held_buttons & (num(args, 0, 0.0) as i64 as u32) != 0),
        "disabletrigger" | "enabletrigger" | "istriggerdisabled" | "holdtrigger" | "releasetrigger" | "taptrigger" | "istriggerheld" => {
            let k = usize::from(num(args, 0, 0.0) != 0.0);
            let p = &mut g.jip2.told.pad;
            match name {
                "disabletrigger" => p.disabled_triggers[k] = true,
                "enabletrigger" => p.disabled_triggers[k] = false,
                "holdtrigger" | "taptrigger" => p.held_triggers[k] = true,
                "releasetrigger" => p.held_triggers[k] = false,
                "istriggerdisabled" => return Some(Ok(yes(p.disabled_triggers[k]))),
                _ => return Some(Ok(yes(p.held_triggers[k]))),
            }
            zero
        }
        "setstickdisabled" => {
            let k = usize::from(num(args, 0, 0.0) != 0.0);
            g.jip2.told.pad.disabled_sticks[k] = num(args, 1, 0.0) != 0.0;
            zero
        }
        "isstickdisabled" => yes(pad.disabled_sticks[usize::from(num(args, 0, 0.0) != 0.0)]),
        "getdeadzonels" | "getdeadzoners" => XVal::Num(f64::from(pad.dead_zones[usize::from(name.ends_with("rs"))].unwrap_or(0.0))),
        "setdeadzonels" | "setdeadzoners" => {
            g.jip2.told.pad.dead_zones[usize::from(name.ends_with("rs"))] = Some(num(args, 0, 0.0) as f32);
            zero
        }
        // ---- the player's state (sweep ranking) ----
        "getautomove" => yes(g.jip2.told.auto_move),
        // Aiming with a gun whose record has a scope model (MOD3) is the scope, without
        // one the iron sights [hypothesis].
        "getpcusingscope" | "getpcusingironsights" => {
            let w = crate::functions::items::equipped_weapon(g, FormId::PLAYER_REF);
            let scoped = w.is_some_and(|w| matches!(crate::nvse::record_value(g, w, "MOD3", ""), Some(XVal::Str(s)) if !s.is_empty()));
            let aiming = g.jip2.told.aiming && w.is_some();
            yes(aiming && (scoped == (name == "getpcusingscope")))
        }
        // Live kill-camera state reported each frame by the engine (`KillCam::kill_playing`)
        // [documented: GECK wiki IsInKillCam; design: classic VATS attack cameras are excluded].
        "isinkillcam" => yes(g.jip2.told.kill_cam),
        // Character-generation state
        // [observed: JIP LN 57.30 `w4c_a02_chargen_all_menus` + `w4d_01_tagskills_chargen`:
        // CharGenMenu/TagSkills (1048), LoveTesterMenu (1074), and TraitMenu (1084) return 1].
        "isincharGen" | "isinchargen" => yes(g.is_in_chargen()),
        // In water: the physics' swimming state [hypothesis: wading is not told];
        // immersion 1 when swimming, else 0 [hypothesis].
        "isinwater" | "getwaterimmersionperc" => {
            let a = subject.unwrap_or(FormId::PLAYER_REF);
            yes(g.senses.motion.get(&a).is_some_and(|m| m.swimming))
        }
        // In the air: the physics' airborne state (neither grounded nor swimming)
        // [documented: GECK wiki IsInAir; hypothesis: swimming counts as not in air].
        "isinair" => {
            let a = subject.unwrap_or(FormId::PLAYER_REF);
            yes(g.senses.motion.get(&a).is_some_and(|m| m.in_air))
        }
        // Units per second on the axis (none given: the speed), measured frame to frame
        // from the first call on [hypothesis: the first call gives 0].
        "getactorvelocity" => {
            let a = subject.unwrap_or(FormId::PLAYER_REF);
            let here = g.ref_pos_rot(a).map(|p| p.0).unwrap_or([0.0; 3]);
            let v = g.jip2.velocity.entry(a).or_insert((here, [0.0; 3])).1;
            let axis = match args.first() {
                Some(XVal::Str(s)) => s.to_ascii_uppercase(),
                Some(XVal::Num(n)) => match *n as i64 { 88 => "X".into(), 89 => "Y".into(), 90 => "Z".into(), _ => String::new() },
                _ => String::new(),
            };
            XVal::Num(f64::from(match axis.as_str() {
                "X" => v[0],
                "Y" => v[1],
                "Z" => v[2],
                _ => (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt(),
            }))
        }
        "isidleplayingex" => {
            let a = subject.unwrap_or(FormId(0));
            yes(form(args, 0).is_some_and(|i| g.idle_playing(a) == Some(i)))
        }
        "getcurrentweather" => XVal::Ref(g.weather.request.map(|r| r.weather).or(g.weather.shown).map_or(0, |w| w.0)),
        // ---- N7: actors and the game ----
        // Keywords: any text, without regard to case, on the base form; a form list
        // assigns to its members [observed: jip-n7].
        "assignkeyword" | "haskeyword" | "removekeyword" => {
            let Some(f) = form(args, 0) else { return Some(Ok(zero)) };
            let k = text(args, 1).to_ascii_lowercase();
            let targets: Vec<FormId> = if g.data.sig(f) == Some(*b"FLST") { crate::functions::list_members(g, f) } else { vec![base(g, f)] };
            match name {
                "haskeyword" => return Some(Ok(yes(g.has_keyword(f, &k)))),
                "assignkeyword" => {
                    for t in targets {
                        g.jip2.keywords.entry(t).or_default().insert(k.clone());
                    }
                }
                _ => {
                    for t in targets {
                        if let Some(s) = g.jip2.keywords.get_mut(&t) {
                            s.remove(&k);
                        }
                    }
                }
            }
            zero
        }
        // [observed: Water2 400, Rad1 200, a form that is no stage 0]
        "gethardcorestagethreshold" => {
            let Some(f) = form(args, 0) else { return Some(Ok(zero)) };
            let t = g.jip2.stage_thresholds.get(&f).copied().or_else(|| g.data.need_stages.values().flatten().find(|s| s.stage == f).map(|s| s.threshold));
            XVal::Num(f64::from(t.unwrap_or(0)))
        }
        "sethardcorestagethreshold" => {
            if let Some(f) = form(args, 0) {
                g.jip2.stage_thresholds.insert(f, num(args, 1, 0.0).max(0.0) as u32);
            }
            zero
        }
        // Gives 0 either way [observed]; the switch is kept [hypothesis: needs read it].
        "togglehardcoretracking" => {
            g.jip2.hardcore_tracking_off = num(args, 0, 1.0) == 0.0;
            zero
        }
        // The base's VTCK; the setter changes the base [observed: read back on the ref and
        // the base].
        "getactorvoicetype" | "setactorvoicetype" => {
            let set = name == "setactorvoicetype";
            let b = form(args, usize::from(set)).or(subject).map(|f| base(g, f)).unwrap_or(FormId(0));
            if set {
                if let Some(v) = form(args, 0) {
                    g.field_overrides.insert((b, "VTCK/".into()), XVal::Ref(v.0));
                }
                zero
            } else {
                match crate::nvse::record_value(g, b, "VTCK", "") {
                    Some(XVal::Ref(r)) => XVal::Ref(r),
                    Some(XVal::Str(s)) => XVal::Ref(u32::from_str_radix(s.trim_start_matches("0x"), 16).unwrap_or(0)),
                    _ => XVal::Ref(0),
                }
            }
        }
        // ACBS karma [observed: Doc 500]; SetBaseKarma `value [base]` works with the base
        // given or on a reference, not called on a base [observed].
        "getbasekarma" => {
            let b = form(args, 0).or(subject).map(|f| base(g, f)).unwrap_or(FormId(0));
            match crate::nvse::record_value(g, b, "ACBS", "karma") {
                Some(XVal::Num(x)) => XVal::Num(x),
                _ => zero,
            }
        }
        "setbasekarma" => {
            let target = form(args, 1).or(subject.filter(|s| g.world.placed.get(*s).is_some() || g.world.peek(*s).is_some()));
            if let Some(b) = target.map(|f| base(g, f)) {
                g.field_overrides.insert((b, "ACBS/karma".into()), XVal::Num(num(args, 0, 0.0)));
            }
            zero
        }
        // The Nth entry's number [observed: Toughness 0 gives 3, past the end -1].
        "getnthperkentryvalue1" | "setnthperkentryvalue1" => {
            let Some(p) = form(args, 0) else { return Some(Ok(XVal::Num(-1.0))) };
            let Ok(i) = usize::try_from(num(args, 1, -1.0) as i64) else { return Some(Ok(XVal::Num(-1.0))) };
            let rec = crate::nvse_lu3::nth_entry_of(g, p, i as f64).map(|e| match &e.effect {
                crate::data::PerkEffect::EntryPoint(ep) => ep.value,
                _ => None,
            });
            match (name, rec) {
                (_, None) => XVal::Num(-1.0),
                ("getnthperkentryvalue1", Some(v)) => XVal::Num(f64::from(v.unwrap_or(0.0))),
                // Into the perk's session copy that the entry points read (the one LU's
                // perk functions edit, `crate::nvse_lu3`), so the new value takes effect
                // [documented: SetNthPerkEntryValue1, "sets the value"; Enhanced Movement
                // sets its sprint speed perk's Modify Run Speed factor so, #2275].
                _ => {
                    let v = num(args, 2, 0.0) as f32;
                    if let Some(e) = crate::nvse_lu3::perk_entry_mut(g, p, i as f64) {
                        if let crate::data::PerkEffect::EntryPoint(ep) = &mut e.effect {
                            ep.value = Some(v);
                        }
                    }
                    zero
                }
            }
        }
        // `entryPoint base [form1] [form2]`: the base through the actor's perks at that
        // entry point [observed: 100 stays 100 for the player with no such perk].
        "getperkmodifier" => {
            let a = subject.unwrap_or(FormId::PLAYER_REF);
            let point = num(args, 0, -1.0);
            let v = num(args, 1, 0.0);
            match u8::try_from(point as i64) {
                Ok(pt) => {
                    let tabs = [form(args, 2), form(args, 3)];
                    XVal::Num(g.entry_point_with(a, pt, &tabs, v).0)
                }
                Err(_) => XVal::Num(v),
            }
        }
        // `name [form]`: the base's name [observed: on the base and on its references].
        "lnsetname" => {
            let Some(f) = form(args, 1).or(subject).map(|f| base(g, f)) else { return Some(Ok(zero)) };
            g.name_overrides.insert(f, text(args, 0));
            zero
        }
        "getkiller" => XVal::Ref(subject.and_then(|a| g.extra.get(&a)).and_then(|e| e.killer).map_or(0, |k| k.0)),
        // Without a value the current one is applied again [documented: SetSpeedMult]; the
        // engine reads Speed Mult every frame, so there is nothing more to do.
        "setspeedmult" => {
            let a = subject.unwrap_or(FormId(0));
            if args.is_empty() {
                return Some(Ok(zero));
            }
            let v = num(args, 0, 100.0);
            if let Err(e) = crate::functions::console::call_words(g, Some(a), "SetActorValue", &["SpeedMult".into(), format!("{v}")], "SetSpeedMult") {
                return Some(Err(e));
            }
            zero
        }
        // No target: back to the record's [observed].
        "setlinkedreference" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            match form(args, 0) {
                Some(t) => g.jip2.links.insert(r, t),
                None => g.jip2.links.remove(&r),
            };
            zero
        }
        "unequipitemalt" => {
            let (Some(a), Some(i)) = (subject, form(args, 0)) else { return Some(Ok(zero)) };
            let words = vec![format!("{:08X}", base(g, i).0), format!("{}", num(args, 1, 0.0) as i64), format!("{}", num(args, 2, 0.0) as i64)];
            if let Err(e) = crate::functions::console::call_words(g, Some(a), "UnequipItem", &words, "UnequipItemAlt") {
                return Some(Err(e));
            }
            zero
        }
        // The reference's record flags (GetFlagsLow's) [observed: Doc 64 and 1024 set].
        "getreferenceflag" | "setreferenceflag" => {
            let r = subject.unwrap_or(FormId(0));
            let bit = num(args, 0, 0.0) as i64 as u32;
            let v = crate::nvse_other::record_flags_of(g, r);
            if name == "getreferenceflag" {
                return Some(Ok(yes(v & bit != 0)));
            }
            let new = if num(args, 1, 0.0) != 0.0 { v | bit } else { v & !bit };
            g.other.flags.insert(r, new);
            zero
        }
        // The effect is cast on the reference at once [observed: health back the next
        // frame].
        "castimmediate" => {
            let (Some(r), Some(s)) = (subject, form(args, 0)) else { return Some(Ok(zero)) };
            let caster = form(args, 1).or(Some(r));
            g.apply_magic_item(r, s, caster, false);
            zero
        }
        // Back to where the record placed it [observed: Chet back in his cell; the player
        // gives 0].
        "movetoeditorposition" => {
            let Some(r) = subject.filter(|&r| r != FormId::PLAYER_REF) else { return Some(Ok(zero)) };
            let Some(p) = g.world.placed.get(r).cloned() else { return Some(Ok(zero)) };
            let rot = if num(args, 0, 0.0) != 0.0 { p.rot } else { g.ref_pos_rot(r).map_or(p.rot, |x| x.1) };
            yes(g.place_ref(r, p.cell, p.pos, rot))
        }
        // `cell x y z` [observed: GetParentCell is the cell the next frame].
        "movetocell" => {
            let (Some(r), Some(c)) = (subject, form(args, 0)) else { return Some(Ok(zero)) };
            let rot = g.ref_pos_rot(r).map_or([0.0; 3], |x| x.1);
            let pos = [num(args, 1, 0.0) as f32, num(args, 2, 0.0) as f32, num(args, 3, 0.0) as f32];
            yes(g.place_ref(r, Some(c), pos, rot))
        }
        // ---- N9: JohnnyGuitar, ShowOff ----
        // Not clamped [observed: t 1.5 gives 15 between 0 and 10].
        "lerp" => {
            let (a, b, t) = (num(args, 0, 0.0), num(args, 1, 0.0), num(args, 2, 0.0));
            XVal::Num(a + (b - a) * t)
        }
        // The Pip-Boy's mode: 0 closed, 1/2 opening, 3 opened, 4/5 closing [documented:
        // GECK wiki GetPipBoyMode; observed: 2026-11-24, `GSDocMitchellHouse`
        // (`0x00103DF9`), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE
        // 528 + JIP LN 57.30 with the 10 official base masters, clean-room script probe
        // `w4c_b3_06_toggle_pipboy_tab` (`tools/capture/nvse/catalogs/wave4c_extenders.json`,
        // #3678 / PR #3679): `init(pb=0)|open1003(mm=1003,pb=2)|close(mm=1003,pb=3)|reopenNoArg(mm=0,pb=0)`;
        // no DLL or source consulted]. When the engine drives a Pip-Boy transition
        // (`Game::ui.pipboy_mode`), reports 2 while raising, 4 while lowering, or 0 while
        // covered during startup warming; resting phases and headless runs leave it `None`
        // so `GetPipBoyMode` evaluates live `Game::menu_open(menus::PIPBOY)` (3 when open,
        // including under a tutorial overlay, else 0) [design].
        "getpipboymode" => XVal::Num(f64::from(
            g.ui.pipboy_mode.unwrap_or(if g.menu_open(crate::game::menus::PIPBOY) { 3 } else { 0 }),
        )),
        // [observed: an unknown stat is -1, and ModExtraMiscStat did not create one]
        "getextramiscstat" => XVal::Num(g.jip2.misc_stats.get(&text(args, 0).to_ascii_lowercase()).copied().unwrap_or(-1.0)),
        // ModExtraMiscStat and InitExtraMiscStat: `crate::nvse_ttw`.
        "setcamerashakenohudshudder" => zero,
        // Aux timers: per owner (the player when none), counting down in game seconds
        // [observed: plugin-odds].
        "auxtimerstart" | "auxtimerstop" | "auxtimertimeleft" | "auxtimertimeelapsed" | "auxtimerpaused" => {
            let owner = subject.unwrap_or(FormId::PLAYER_REF);
            let key = (owner, text(args, 0).to_ascii_lowercase());
            let now = g.aux_timer_now();
            let left = |t: &(f64, f64, bool)| if t.2 { t.0 } else { (t.0 - (now - t.1)).max(0.0) };
            match name {
                "auxtimerstart" => {
                    g.jip2.timers.insert(key.clone(), (num(args, 1, 0.0), now, false));
                    // The countdown and flags (default 1 + 2 + 4) for
                    // AuxTimerTimeToCountdown and the update handlers, then the start
                    // handlers [documented: AuxTimerStart; SetOnAuxTimerStartHandler].
                    g.showoff.countdowns.insert(key.clone(), (num(args, 1, 0.0), num(args, 2, 7.0) as i64 as u32));
                    g.showoff.timer_mods.insert(key.clone(), g.other.plugin.to_ascii_lowercase());
                    g.aux_timer_started(key.0, &key.1);
                    XVal::Num(1.0)
                }
                // [documented: AuxTimerStop fires the stop event only with fireEvent non-0]
                "auxtimerstop" => {
                    let had = g.jip2.timers.remove(&key).is_some();
                    if had && num(args, 1, 0.0) != 0.0 {
                        g.aux_timer_stopped(key.0, &key.1);
                    }
                    yes(had)
                }
                "auxtimertimeleft" => XVal::Num(g.jip2.timers.get(&key).map_or(-1.0, left)),
                "auxtimertimeelapsed" => XVal::Num(g.jip2.timers.get(&key).map_or(-1.0, |t| t.0 - left(t))),
                _ => {
                    // Gives whether it was paused before; a value pauses or resumes.
                    let Some(t) = g.jip2.timers.get_mut(&key) else { return Some(Ok(XVal::Num(-1.0))) };
                    let before = t.2;
                    if let Some(XVal::Num(p)) = args.get(1) {
                        let want = *p != 0.0;
                        if want && !t.2 {
                            t.0 = (t.0 - (now - t.1)).max(0.0);
                        } else if !want && t.2 {
                            t.1 = now;
                        }
                        t.2 = want;
                    }
                    yes(before)
                }
            }
        }
        _ => {
            let _ = ctx;
            return None;
        }
    };
    let _ = (ArrayKind::List, XKey::Num(0.0));
    Some(Ok(v))
}
