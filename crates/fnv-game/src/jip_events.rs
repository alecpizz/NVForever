//! JIP LN's own event handlers (task J6; docs/spec-notes/jip-events.md): the
//! `SetOn…EventHandler` functions and SetGameMainLoopCallback. Beside the extender's
//! SetEventHandler (`Game::dispatch_event`), with JIP's own rules:
//! - several handlers for one event run **newest first** [observed: ev.order];
//! - a handler that declares fewer parameters than the event passes never runs
//!   [observed: ev.noparams, ev.oneparam]; NVForever logs why [decision];
//! - registrations are not saved: a load starts with none [documented for the ragdoll
//!   event; hypothesis for the others, jip-events.md §1].

use crate::game::{Game, RunCtx};
use fnv_script::vm::{Value, XVal};
use fnv_sim::FormId;
use std::sync::Arc;
use crate::jip_provenance::Registrar;

/// XMarker: the "no filter" form some events take, since 0 is refused [documented:
/// SetOnCriticalHitEventHandler].
const XMARKER: u32 = 0x0000_003B;

/// Analog controller trigger press threshold (`0.0..=1.0`): standard XInput
/// `XINPUT_GAMEPAD_TRIGGER_THRESHOLD` (30 / 255) [documented: Microsoft XInput
/// `XINPUT_GAMEPAD_TRIGGER_THRESHOLD`; hypothesis: used for JIP trigger down/up edges].
pub const TRIGGER_PRESS_THRESHOLD: f32 = 30.0 / 255.0;

/// Default `fAutoAimMaxDistance` when the load order lacks the GMST
/// [documented: GECK wiki Auto Aim Settings, 1800].
pub const DEFAULT_AUTO_AIM_MAX_DISTANCE: f64 = 1800.0;

/// The JIP registrations of a session.
#[derive(Debug, Clone, Default)]
pub struct JipEvents {
    pub handlers: Vec<JipHandler>,
    pub main_loop: Vec<MainLoop>,
    seq: u64,
    /// Diagnostic metadata only, bounded by active registrations; no callable retention.
    registrars: std::collections::HashMap<u64, Arc<Registrar>>,
    /// Each actor's hit in progress, or its last one (task J7, jip-hit-info.md §5).
    pub hits: std::collections::HashMap<FormId, JipHit>,
    /// The keys and mouse buttons held last frame, for the key-down and key-up events.
    pub keys_held: std::collections::BTreeSet<u16>,
    /// Last 28-control event snapshot; shared by both control-event interfaces.
    pub controls_held: u32,
    /// Last gamepad button mask snapshot (XInput bits), for OnButtonDown/OnButtonUp.
    pub buttons_held: u32,
    /// Last controller trigger bitmask (bit 0 = left trigger, bit 1 = right trigger),
    /// for SetOnTriggerDownEventHandler / SetOnTriggerUpEventHandler.
    pub triggers_held: u8,
    /// Last actor target reported to SetOnPCTargetChangeEventHandler.
    pub pc_target: Option<FormId>,
    /// Last `(crosshair_ref, crosshair_far, reticle_hit_bits, auto_aim_max_bits)`
    /// snapshot synced from `Senses`, so explicit `set_pc_target` calls are not
    /// overwritten on ticks when `Senses` has not changed.
    last_crosshair_senses: (Option<FormId>, Option<FormId>, Option<u32>, u64),
    /// ScriptWait: the waiting block of each script instance (one at a time
    /// [documented: ScriptWait]); not saved [hypothesis, as the registrations].
    pub waits: std::collections::HashMap<crate::game::InstKey, Waiting>,
    /// A ScriptWait call's cycles, for the block run that made it (`Exec::take_wait`).
    pub wait_request: Option<u32>,
    /// The script depth of the block run that may wait: ScriptWait elsewhere (a result
    /// script, a function, an effect) does not [documented: ScriptWait, "cannot be used
    /// in Effect-type scripts, UDF scripts, or result scripts"].
    pub wait_depth: Option<u32>,
    /// SetMusicState's requests for the engine: 0 stop, 1 pause, 2 resume [documented].
    pub music_states: Vec<u8>,
    /// What lasts the whole session, across loads (`Game::take_session`).
    pub kept: JipKept,
    /// `(frame, [(actor, weapon)])` pairs whose `fireweapon` event already fired this
    /// frame, deduplicating `weapon_fired` -> `projectile_fired` and multi-pellet bursts.
    pub(crate) fired_this_frame: (u64, Vec<(FormId, FormId)>),
}

impl JipEvents {
    /// Carry active main-loop registrar metadata across `Game::take_session`.
    pub(crate) fn take_main_loop_registrars(
        &self,
        kept: &[MainLoop],
    ) -> (std::collections::HashMap<u64, Arc<Registrar>>, u64) {
        let mut out = std::collections::HashMap::new();
        for m in kept {
            if let Some(r) = self.registrars.get(&m.seq) {
                out.insert(m.seq, Arc::clone(r));
            }
        }
        (out, self.seq)
    }

    /// Restore carried main-loop registrar metadata and advance `seq` so new
    /// registrations in the restored `Game` never collide with carried `MainLoop.seq`.
    pub(crate) fn restore_main_loop_registrars(
        &mut self,
        registrars: std::collections::HashMap<u64, Arc<Registrar>>,
        seq: u64,
    ) {
        let max_kept = self.main_loop.iter().map(|m| m.seq).max().unwrap_or(0);
        self.seq = self.seq.max(seq).max(max_kept);
        self.registrars.extend(registrars);
    }
}

/// JIP's settings that last until the game is quit (`crate::nvse_ttw`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct JipKept {
    /// MarkActivatorAshPile's activators [documented: "remains in effect for the duration
    /// of the current game session"].
    pub ash_pile_bases: std::collections::BTreeSet<FormId>,
    /// SetTerminalUIModel's menu art, by terminal form, a path under `meshes\`
    /// [documented]; kept for the session as MarkActivatorAshPile's marks [hypothesis:
    /// the page does not say].
    pub terminal_models: std::collections::BTreeMap<FormId, String>,
}

/// A block that waits (ScriptWait): which, the processing cycles still to wait and where
/// it goes on ([`fnv_script::vm::WaitPoint`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Waiting {
    pub script: FormId,
    pub block: usize,
    pub left: u32,
    pub at: fnv_script::vm::WaitPoint,
}

/// What JIP's GetHit… functions read [documented: jip-hit-info.md §1].
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct JipHit {
    pub attacker: Option<FormId>,
    pub weapon: Option<FormId>,
    pub projectile: Option<FormId>,
    pub health_damage: f64,
    pub limb_damage: f64,
    pub fatigue_damage: f64,
    pub base_weapon_damage: f64,
    pub location: i32,
    /// GetHitExtendedFlag's bits: 0 blocking, 1 critical, 2 fatal, 5 limb crippled,
    /// 7 sneak attack, 8 explosion hit (bit n = flag n).
    pub flags: u16,
    /// World-space 3D impact point (`GetHit3DData 0` and first 3 elements of type 2);
    /// `[0.0; 3]` for an explosion hit [observed: 2026-11-23, GSDocMitchellHouse
    /// (0x00103DF9), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 (10
    /// official base masters), #3678 / PR #3679 probe `w1_3606_hit_3d_dir_and_base_damage`:
    /// `GrenadeFragExplosion` placed via `PlaceAtMe`].
    pub hit_pos: Option<[f32; 3]>,
    /// Unit 3D incoming travel direction vector `(nx, ny, nz)` (`GetHit3DData 1` and last
    /// 3 elements of type 2); `[0.0; 3]` for an explosion hit [observed: 2026-11-23,
    /// GSDocMitchellHouse (0x00103DF9), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 +
    /// JIP LN 57.30 (10 official base masters), #3678 / PR #3679 probe
    /// `w1_3606_hit_3d_dir_and_base_damage`: South->North `+Y`, East->West `-X`, and
    /// `GrenadeFragExplosion`].
    pub hit_dir: Option<[f32; 3]>,
}

/// One `SetOn<Event>EventHandler handler 1 filters…`.
#[derive(Debug, Clone, PartialEq)]
pub struct JipHandler {
    /// The name between `SetOn` and `EventHandler`, lower case (`healthdamage`).
    pub event: String,
    pub func: XVal,
    pub filters: Vec<XVal>,
    pub seq: u64,
}

/// Snapshot diagnostics with the handler before any callback can remove its registrar.
struct JipAttempt {
    handler: JipHandler,
    registrar: Option<Arc<Registrar>>,
}

impl JipEvents {
    fn attempt(&self, handler: &JipHandler) -> JipAttempt {
        JipAttempt { handler: handler.clone(), registrar: self.registrars.get(&handler.seq).cloned() }
    }
}

/// Bounded values for diagnostics: no array dereference or unbounded string copy.
fn diagnostic_value(value: &XVal) -> String {
    match value {
        XVal::Num(n) => format!("number:{n:?}"),
        XVal::Ref(id) => format!("ref:0x{id:08X}"),
        XVal::Array(id) => format!("array:{id}"),
        XVal::Str(text) => {
            let mut out = String::from("string:\"");
            let mut chars = text.chars();
            for c in chars.by_ref().take(32) {
                out.extend(c.escape_debug());
            }
            if chars.next().is_some() {
                out.push_str("<truncated>");
            }
            out.push('"');
            out
        }
    }
}

impl JipHandler {
    fn failure_context(&self, on: Option<FormId>, args: usize) -> String {
        let filters = self.filters.iter().take(4).map(diagnostic_value).collect::<Vec<_>>().join(", ");
        let omitted = if self.filters.len() > 4 { ", <truncated>" } else { "" };
        let receiver = on.map_or_else(|| "none".into(), |r| format!("0x{:08X}", r.0));
        format!(
            "registration={} callable={} receiver={} args={} filters(total={})=[{}{}]",
            self.seq, diagnostic_value(&self.func), receiver, args, self.filters.len(), filters, omitted,
        )
    }
}

/// One SetGameMainLoopCallback registration.
#[derive(Debug, Clone, PartialEq)]
pub struct MainLoop {
    pub func: XVal,
    /// The reference it was called on; the player when none [documented].
    pub on: Option<FormId>,
    /// Every this many frames [documented: frames, not time].
    pub delay: u64,
    /// 1 game mode, 2 menu mode, 8 removed on load [documented].
    pub flags: u32,
    pub due: u64,
    pub(crate) seq: u64,
}

impl MainLoop {
    fn failure_context(&self) -> String {
        let receiver = self.on.map_or_else(|| "none".into(), |r| format!("0x{:08X}", r.0));
        format!(
            "registration={} callable={} receiver={} delay={} flags={}",
            self.seq, diagnostic_value(&self.func), receiver, self.delay, self.flags,
        )
    }
}

/// What a filter at each position tests: the calling reference or an argument
/// [documented: the table of jip-events.md §2].
#[derive(Clone, Copy)]
enum Slot {
    On,
    Arg(usize),
    /// A required animation code: zero is a value, not an actor wildcard.
    ExactArg(usize),
}

fn slots(event: &str) -> &'static [Slot] {
    match event {
        "hit" | "healthdamage" | "crippledlimb" | "fireweapon" | "projectileimpact" => &[Slot::On],
        "animaction" | "playgroup" => &[Slot::On, Slot::ExactArg(0)],
        "criticalhit" => &[Slot::On, Slot::Arg(0), Slot::Arg(1)],
        // Stage 0 matches only stage 0, not every stage [observed: 2026-11-24, Fallout:
        // New Vegas 1.4.0.525, xNVSE 6.4.9, JIP LN 57.30, GSDocMitchellHouse (0x00103DF9),
        // 10 official base masters,
        // `tools/capture/nvse/catalogs/wave6d1_jip_quest_stage_exact_and_dup.json` probe
        // `w6d1_23_jip_quest_stage_and_skip_script`:
        // `ret=1,0,1,0|n1=1,n2=0|s0=1,qOk=1,arg=10,in=10,post=10`; no DLL, PDB, or
        // third-party source consulted].
        "queststage" => &[Slot::Arg(0), Slot::ExactArg(1)],
        "controldown" | "controlup" | "triggerdown" | "triggerup" => &[Slot::ExactArg(0)],
        "useaiditem" | "dialogtopic" | "keydown" | "keyup" | "menuopen" | "menuclose" | "mouseoverchange" => &[Slot::Arg(0)],
        _ => &[],
    }
}

fn normalize_click_filter(p: &str) -> String {
    if let Some((m, n)) = p.split_once('#') {
        return format!("{}#{}", m.trim().to_ascii_lowercase(), n.trim());
    }
    p.split(['/', '\\'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>()
        .join("/")
}

fn click_segment_matches(want: &str, have: &str) -> bool {
    fn split_idx(s: &str) -> (&str, Option<&str>) {
        match s.rsplit_once(':') {
            Some((base, idx)) if !idx.is_empty() && idx.bytes().all(|b| b.is_ascii_digit()) => (base, Some(idx)),
            _ => (s, None),
        }
    }
    let (w_name, w_idx) = split_idx(want);
    let (h_name, h_idx) = split_idx(have);
    (w_name == "*" || w_name == h_name) && (w_idx.is_none() || w_idx == h_idx)
}

/// Whether a SetOnMenuClickEventHandler tile path names this tile (see
/// [`Game::jip_menu_click`]).
pub fn click_path_matches(filter: &str, menu: &str, path: &str, id: f64) -> bool {
    if let Some((m, n)) = filter.split_once('#') {
        return m.trim().eq_ignore_ascii_case(menu) && n.trim().parse::<f64>().is_ok_and(|n| n == id && id >= 0.0);
    }
    let segs = |p: &str| p.split(['/', '\\']).map(str::trim).filter(|s| !s.is_empty()).map(str::to_ascii_lowercase).collect::<Vec<_>>();
    let (want, have) = (segs(filter), segs(path));
    !want.is_empty() && want.len() <= have.len() && want.iter().zip(&have).all(|(a, b)| click_segment_matches(a, b))
}

impl Game<'_> {
    /// `SetOn<event>EventHandler func add filters…`; a second identical registration is
    /// kept once [decision, jip-events.md §2]. A removal takes the registrations whose
    /// filters begin with the ones given: the same filters, or with fewer (none: every
    /// registration of that handler for the event) [documented: the same filters remove
    /// the registration, jip-events.md §1; hypothesis: fewer remove all that match them,
    /// as Just Assorted Mods' sprint relies on: it registers its key handler per key and
    /// removes it with no key before registering again or when the sprint is turned off,
    /// observed in its script source; issue #2952]. Returns whether anything changed.
    pub(crate) fn jip_set(&mut self, event: String, func: XVal, add: bool, mut filters: Vec<XVal>, registrar: Option<Arc<Registrar>>) -> bool {
        if event == "playgroup" {
            if let Some(XVal::Str(name)) = filters.get(1) {
                if let Some(code) = self.anim_group_code(name) {
                    filters[1] = XVal::Num(f64::from(code));
                }
            }
        }
        let filters_eq = |a: &XVal, b: &XVal| match (event.as_str(), a, b) {
            ("menuclick", XVal::Str(sa), XVal::Str(sb)) => normalize_click_filter(sa) == normalize_click_filter(sb),
            _ => a == b,
        };
        let same = |h: &JipHandler| {
            h.event == event && h.func == func && h.filters.len() == filters.len() && h.filters.iter().zip(&filters).all(|(a, b)| filters_eq(a, b))
        };
        let removed = |h: &JipHandler| {
            h.event == event && h.func == func && h.filters.len() >= filters.len() && h.filters.iter().zip(&filters).all(|(a, b)| filters_eq(a, b))
        };
        if add {
            if self.jip.handlers.iter().any(same) {
                // `SetOnQuestStageEventHandler` returns 0 on duplicate registration
                // [observed: 2026-11-24, Fallout: New Vegas 1.4.0.525, xNVSE 6.4.9,
                // JIP LN 57.30, GSDocMitchellHouse (0x00103DF9), 10 official base masters,
                // `tools/capture/nvse/catalogs/wave6d1_jip_quest_stage_exact_and_dup.json`
                // probe `w6d1_23_jip_quest_stage_and_skip_script`:
                // `ret=1,0,1,0|n1=1,n2=0|s0=1,qOk=1,arg=10,in=10,post=10`; no DLL, PDB, or
                // third-party source consulted]. Other JIP event handlers keep returning 1
                // on duplicate registration unless probed individually.
                return event != "queststage";
            }
            self.jip.seq += 1;
            let seq = self.jip.seq;
            self.jip.handlers.push(JipHandler { event, func, filters, seq });
            if let Some(registrar) = registrar { self.jip.registrars.insert(seq, registrar); }
            true
        } else {
            let before = self.jip.handlers.len();
            let registrars = &mut self.jip.registrars;
            self.jip.handlers.retain(|h| {
                if removed(h) { registrars.remove(&h.seq); false } else { true }
            });
            self.jip.handlers.len() < before
        }
    }

    /// SetGameMainLoopCallback func add [delay] [flags].
    pub(crate) fn jip_main_loop(
        &mut self,
        func: XVal,
        add: bool,
        delay: u64,
        flags: u32,
        on: Option<FormId>,
        registrar: Option<Arc<Registrar>>,
    ) {
        let registrars = &mut self.jip.registrars;
        self.jip.main_loop.retain(|m| {
            if m.func == func && m.on == on {
                registrars.remove(&m.seq);
                false
            } else {
                true
            }
        });
        if !add {
            return;
        }
        // Refuse null/zero callable tokens — including any numeric value that
        // `run_function_here` (`*x as u32`) would coerce to form 0 (0.0, negative
        // numbers, fractions below 1.0, NaN) — at registration time with registrar
        // attribution rather than enqueuing a form-0 callback that fails every frame
        // (#2603) [design].
        let is_zero = matches!(func, XVal::Ref(0)) || matches!(func, XVal::Num(n) if (n as u32) == 0);
        if is_zero {
            let receiver = on.map_or_else(|| "none".into(), |r| format!("0x{:08X}", r.0));
            let callable = diagnostic_value(&func);
            self.handler_failed_with_registrar(
                "JIP main-loop callback".into(),
                format!("cannot register zero callable token {callable}"),
                || Some(format!("registration=0 callable={callable} receiver={receiver} delay={} flags={flags}", delay.max(1))),
                registrar.as_deref(),
            );
            return;
        }
        let delay = delay.max(1);
        self.jip.seq += 1;
        let seq = self.jip.seq;
        self.jip.main_loop.push(MainLoop {
            func,
            on,
            delay,
            flags,
            due: self.frame + delay,
            seq,
        });
        if let Some(registrar) = registrar {
            self.jip.registrars.insert(seq, registrar);
        }
    }

    /// Run the handlers of a JIP event, newest first; returns how many ran.
    pub fn jip_event(&mut self, event: &str, on: Option<FormId>, args: Vec<XVal>) -> usize {
        if self.jip.handlers.is_empty() {
            return 0;
        }
        let hs = self.jip.handlers.iter().filter(|h| h.event == event && self.jip_matches(h, on, &args)).map(|h| self.jip.attempt(h)).collect();
        self.jip_run(event, hs, on, args)
    }

    /// Fire JIP's `SetOnAnimActionEventHandler` (`animaction`) for `actor` starting
    /// `action` (0–13, `GetAnimAction` code) [documented: SetOnAnimActionEventHandler].
    pub fn fire_anim_action(&mut self, actor: FormId, action: i32) -> usize {
        if action < 0 {
            return 0;
        }
        self.jip_event("animaction", Some(actor), vec![XVal::Num(f64::from(action))])
    }

    /// Fire JIP's `SetOnPlayGroupEventHandler` (`playgroup`) for `actor` starting
    /// animation group `group` [documented: SetOnPlayGroupEventHandler]. Ignores
    /// internal sentinel codes (`SEQUENCE`, `OPEN_GROUP`, `CLOSE_GROUP`).
    pub fn fire_play_group(&mut self, actor: FormId, group: u16) -> usize {
        if matches!(group, crate::anims::SEQUENCE | crate::anims::OPEN_GROUP | crate::anims::CLOSE_GROUP) {
            return 0;
        }
        self.jip_event("playgroup", Some(actor), vec![XVal::Num(f64::from(group))])
    }

    /// SetOnMenuClickEventHandler's event: a tile was clicked (task T8). `menu` is the
    /// menu's name, `path` the tile's full path, `id` its `<id>` (-1 without one), `name`
    /// the name passed. A handler's tile path matches when it is `Menu#<id>` of this tile,
    /// or the tile's path or a path above it [documented: SetOnMenuClickEventHandler,
    /// "a partial path … ANY tile beginning with that path"; hypothesis: compared by
    /// whole segments, without case, `/` and `\` alike]. The handler gets the menu's ID,
    /// the tile's id and its name in lower case [documented].
    pub fn jip_menu_click(&mut self, menu: &str, path: &str, menu_id: f64, id: f64, name: &str) -> usize {
        if self.jip.handlers.is_empty() {
            return 0;
        }
        let matches = |f: &XVal| match f {
            XVal::Str(f) => click_path_matches(f, menu, path, id),
            _ => false,
        };
        let hs = self.jip.handlers.iter().filter(|h| h.event == "menuclick" && h.filters.first().is_some_and(matches)).map(|h| self.jip.attempt(h)).collect();
        self.jip_run("menuclick", hs, None, vec![XVal::Num(menu_id), XVal::Num(id), XVal::Str(name.to_ascii_lowercase())])
    }

    /// SetOnDialogTopicEventHandler's event: `speaker` spoke `info` under `topic`
    /// (jip-events.md §2, §5). Each matching handler runs newest-first with
    /// `this = speaker`: a non-wildcard filter matching `info` receives `info`;
    /// a non-wildcard filter matching `topic` (or an unfiltered / wildcard
    /// registration) receives `topic` [documented: topic or info; hypothesis:
    /// unfiltered/wildcard handlers receive the topic].
    pub(crate) fn jip_dialog_topic(&mut self, speaker: FormId, topic: FormId, info: FormId) -> usize {
        if self.jip.handlers.is_empty() {
            return 0;
        }
        let info_val = XVal::Ref(info.0);
        let topic_val = XVal::Ref(topic.0);
        let mut hs: Vec<(JipAttempt, XVal)> = self
            .jip
            .handlers
            .iter()
            .filter(|h| h.event == "dialogtopic")
            .filter_map(|h| {
                let matched = match h.filters.first() {
                    None => Some(topic_val.clone()),
                    Some(f) if self.jip_wildcard("dialogtopic", f) => Some(topic_val.clone()),
                    Some(f) if self.filter_matches(f, &info_val, false) => Some(info_val.clone()),
                    Some(f) if self.filter_matches(f, &topic_val, false) => Some(topic_val.clone()),
                    Some(_) => None,
                };
                matched.map(|arg| (self.jip.attempt(h), arg))
            })
            .collect();
        if hs.is_empty() {
            return 0;
        }
        hs.sort_by_key(|(a, _)| std::cmp::Reverse(a.handler.seq));
        let ctx = RunCtx { self_ref: Some(speaker), ..RunCtx::default() };
        let mut ran = 0;
        for (attempt, arg) in hs {
            let h = &attempt.handler;
            if self.function_arity(&h.func).is_some_and(|n| n < 1) {
                tracing::info!("jip: a dialogtopic handler takes fewer than the event's 1 values and does not run");
                continue;
            }
            match self.run_function(&h.func, Some(Value::Ref(speaker.0)), vec![arg], None, ctx) {
                Ok(_) => ran += 1,
                Err(error) => self.handler_failed_with_registrar(
                    "JIP dialogtopic handler".into(),
                    error,
                    || Some(h.failure_context(Some(speaker), 1)),
                    attempt.registrar.as_deref(),
                ),
            }
        }
        ran
    }

    fn jip_run(&mut self, event: &str, mut hs: Vec<JipAttempt>, on: Option<FormId>, args: Vec<XVal>) -> usize {
        hs.sort_by_key(|h| std::cmp::Reverse(h.handler.seq));
        let ctx = RunCtx { self_ref: on, ..RunCtx::default() };
        let mut ran = 0;
        for attempt in &hs {
            let h = &attempt.handler;
            if self.function_arity(&h.func).is_some_and(|n| n < args.len()) {
                tracing::info!("jip: a {event} handler takes fewer than the event's {} values and does not run", args.len());
                continue;
            }
            match self.run_function(&h.func, on.map(|f| Value::Ref(f.0)), args.clone(), None, ctx) {
                Ok(_) => ran += 1,
                Err(error) => self.handler_failed_with_registrar(format!("JIP {event} handler"), error,
                    || Some(h.failure_context(on, args.len())), attempt.registrar.as_deref()),
            }
        }
        ran
    }

    fn jip_matches(&self, h: &JipHandler, on: Option<FormId>, args: &[XVal]) -> bool {
        slots(&h.event).iter().zip(&h.filters).all(|(s, f)| {
            if !matches!(s, Slot::ExactArg(_)) && self.jip_wildcard(&h.event, f) {
                return true;
            }
            let v = match s {
                Slot::On => on.map(|r| XVal::Ref(r.0)),
                Slot::Arg(i) | Slot::ExactArg(i) => args.get(*i).cloned(),
            };
            v.is_some_and(|v| self.filter_matches(f, &v, false))
        })
    }

    /// A filter that lets everything through: none (0), an empty form list
    /// [documented: SetOnAnimActionEventHandler, an empty list means any actor],
    /// or `XMarker` (`0x3B`) strictly for `SetOnCriticalHitEventHandler` where 0 is
    /// refused [documented: SetOnCriticalHitEventHandler; observed: 2026-11-24,
    /// Fallout: New Vegas 1.4.0.525, xNVSE 6.4.9, JIP LN 57.30, GSDocMitchellHouse
    /// (0x00103DF9), 10 official base masters,
    /// `tools/capture/nvse/catalogs/wave6d1_jip_xmarker_wildcard_scope.json` probes
    /// `w6d1_21_jip_health_damage_filters_dups_rem` (`xm=0`),
    /// `w6d1_22_jip_use_aid_item_filters` (`xm=0`),
    /// `w6d1_24_jip_anim_action_and_play_group` (`draw(1,0,0)|jam(1,1,0)`), and
    /// `w6d1_25_jip_crippled_limb_and_note_added` (`xm=0`); no DLL, PDB, or
    /// third-party source consulted; design: other unprobed JIP events also exclude
    /// `XMarker` wildcard].
    fn jip_wildcard(&self, event: &str, f: &XVal) -> bool {
        match f {
            XVal::Num(x) => *x == 0.0,
            XVal::Ref(0) => true,
            XVal::Ref(XMARKER) => event == "criticalhit",
            XVal::Ref(r) => self.jip_empty_form_list(FormId(*r)),
            _ => false,
        }
    }

    /// Resolve a materialized clone's declared type, keeping its own list edits.
    /// Unmaterialized clones cannot use an absent copied list as a wildcard
    /// (docs/spec-notes/jip-cloned-list-wildcard.md).
    fn jip_empty_form_list(&self, list: FormId) -> bool {
        if self.clones.contains_key(&list) && !self.list_edits.contains_key(&list) && !self.form_lists.contains_key(&list) {
            return false;
        }
        let mut original = list;
        // At most one step per alias plus its terminal authored record. Public
        // TempCloneForm makes acyclic chains; malformed cycles fail closed here.
        for _ in 0..=self.clones.len() {
            match self.clones.get(&original) {
                Some(&next) => original = next,
                None => return self.data.sig(original) == Some(*b"FLST") && crate::functions::list_members(self, list).is_empty(),
            }
        }
        false
    }

    /// Whether a quest-stage registration that matches asked to skip the stage's result
    /// script (skipScript, the third filter) [documented: SetOnQuestStageEventHandler].
    pub(crate) fn jip_skips_stage(&self, quest: FormId, stage: i32) -> bool {
        let args = [XVal::Ref(quest.0), XVal::Num(f64::from(stage))];
        self.jip.handlers.iter().any(|h| h.event == "queststage" && h.filters.get(2).is_some_and(XVal::truthy) && self.jip_matches(h, None, &args))
    }

    /// A GetHit… value of `actor`'s current or last hit; `None` when it had none.
    pub(crate) fn hit_value(&mut self, actor: FormId, what: &str, k: i64) -> Option<XVal> {
        let h = *self.jip.hits.get(&actor)?;
        Some(match what {
            "attacker" => XVal::Ref(h.attacker.map_or(0, |a| a.0)),
            "weapon" => XVal::Ref(h.weapon.map_or(0, |w| w.0)),
            "projectile" => XVal::Ref(h.projectile.map_or(0, |p| p.0)),
            "health" => XVal::Num(h.health_damage),
            "limb" => XVal::Num(h.limb_damage),
            "fatigue" => XVal::Num(h.fatigue_damage),
            "base_weapon" => XVal::Num(h.base_weapon_damage),
            "flag" => XVal::Num(f64::from(u8::from((0..16).contains(&k) && h.flags & (1 << k) != 0))),
            "3d" => {
                let coords: Vec<f32> = match k {
                    0 => h.hit_pos?.to_vec(),
                    1 => h.hit_dir?.to_vec(),
                    2 => {
                        let p = h.hit_pos?;
                        let d = h.hit_dir?;
                        vec![p[0], p[1], p[2], d[0], d[1], d[2]]
                    }
                    _ => return None,
                };
                let entries = coords
                    .into_iter()
                    .enumerate()
                    .map(|(i, c)| (fnv_script::vm::XKey::Num(i as f64), XVal::Num(f64::from(c))))
                    .collect();
                XVal::Array(self.ext.new_array(fnv_script::vm::ArrayKind::List, entries))
            }
            _ => return None,
        })
    }

    /// Each frame (`Game::tick` and `Game::page_tick`): key-down/key-up events by
    /// DirectX scancode (256 + n for the mouse) for both JIP (`SetOnKeyDownEventHandler`
    /// / `SetOnKeyUpEventHandler`) and named `SetEventHandler` (`OnKeyDown` / `OnKeyUp`,
    /// plain or `:code`) [documented: GECK wiki SetOnKeyDownEventHandler, OnKeyDown,
    /// OnKeyUp]. The keys held are the player's whatever DisableKey says, as JIP's
    /// events "still run even if the key has been disabled" [documented:
    /// SetOnKeyDownEventHandler; hypothesis for SetEventHandler OnKeyDown/OnKeyUp].
    /// Both control-event interfaces and gamepad button events (`OnButtonDown` /
    /// `OnButtonUp`, plain or `:bitmask`) follow the same frame entry point against
    /// snapshots taken before callbacks (control-input-events.md, ext-events.md).
    pub(crate) fn jip_keys(&mut self) {
        let controls = self.input.controls_for_events();
        let buttons = self.jip2.told.pad.buttons | self.jip2.told.pad.held_buttons;
        let triggers = self.trigger_mask();
        self.jip_key_edges();
        self.control_edges(controls);
        self.button_edges(buttons);
        self.trigger_edges(triggers);
        self.sync_pc_target();
    }

    fn sanitize_trigger(value: f32) -> f32 {
        if value.is_finite() { value.clamp(0.0, 1.0) } else { 0.0 }
    }

    fn trigger_mask(&self) -> u8 {
        let p = &self.jip2.told.pad;
        let left = p.held_triggers[0] || p.triggers[0] >= TRIGGER_PRESS_THRESHOLD;
        let right = p.held_triggers[1] || p.triggers[1] >= TRIGGER_PRESS_THRESHOLD;
        u8::from(left) | (u8::from(right) << 1)
    }

    /// Publish sanitized controller trigger values (`0.0..=1.0`, left then right) for
    /// `GetLeftTrigger` / `GetRightTrigger` and the frame's `Game::jip_keys` trigger edge
    /// dispatch (`Game::tick` / `Game::page_tick`) without running script callbacks
    /// immediately [design: engine input systems run before `PlaySet::Scripts` and outside
    /// `with_world` / pause / menu-page guards].
    pub fn publish_pad_triggers(&mut self, left: f32, right: f32) {
        self.jip2.told.pad.triggers = [Self::sanitize_trigger(left), Self::sanitize_trigger(right)];
    }

    /// Publish controller trigger values (`0.0..=1.0`, left then right) and immediately
    /// dispatch any `SetOnTriggerDownEventHandler` / `SetOnTriggerUpEventHandler` edge
    /// transitions (`0` left, `1` right) [documented: SetOnTriggerDownEventHandler,
    /// SetOnTriggerUpEventHandler].
    pub fn set_pad_triggers(&mut self, left: f32, right: f32) {
        self.publish_pad_triggers(left, right);
        let now = self.trigger_mask();
        self.trigger_edges(now);
    }

    fn trigger_edges(&mut self, now: u8) {
        let before = std::mem::replace(&mut self.jip.triggers_held, now);
        let changed = now ^ before;
        if changed == 0 {
            return;
        }
        let jip = self.jip.handlers.iter().any(|h| h.event == "triggerdown" || h.event == "triggerup");
        if !jip {
            return;
        }
        for code in 0u8..2 {
            let bit = 1u8 << code;
            if changed & bit == 0 {
                continue;
            }
            let down = now & bit != 0;
            self.jip_event(if down { "triggerdown" } else { "triggerup" }, None, vec![XVal::Num(f64::from(code))]);
        }
    }

    fn is_pc_target_actor(&self, r: FormId) -> bool {
        r != FormId::PLAYER_REF
            && matches!(
                crate::functions::base_of(self, r).and_then(|b| self.data.sig(b)).as_ref(),
                Some(b"NPC_") | Some(b"CREA")
            )
    }

    fn pc_target_from_senses(&self, max_dist: f64) -> Option<FormId> {
        let from_far = self.senses.crosshair_far.is_some();
        let candidate = self.senses.crosshair_far.or(self.senses.crosshair_ref)?;
        if !self.is_pc_target_actor(candidate) {
            return None;
        }
        if from_far && max_dist > 0.0 {
            let dist = self
                .senses
                .reticle
                .and_then(|r| r.hit)
                .map(f64::from)
                .or_else(|| crate::nvse_showoff5::distance_between(self, FormId::PLAYER_REF, candidate));
            if dist.is_some_and(|d| d > max_dist) {
                return None;
            }
        }
        Some(candidate)
    }

    /// Synchronize `SetOnPCTargetChangeEventHandler` (`pctargetchange`) against `Senses`
    /// (`crosshair_far`, `crosshair_ref`, `reticle`, and `fAutoAimMaxDistance`) whenever
    /// any of those inputs changed.
    pub(crate) fn sync_pc_target(&mut self) {
        let max_dist = crate::combat::setting(self, "fAutoAimMaxDistance", DEFAULT_AUTO_AIM_MAX_DISTANCE);
        let snap = (
            self.senses.crosshair_ref,
            self.senses.crosshair_far,
            self.senses.reticle.and_then(|r| r.hit).map(f32::to_bits),
            max_dist.to_bits(),
        );
        if snap == self.jip.last_crosshair_senses {
            return;
        }
        self.jip.last_crosshair_senses = snap;
        let target = self.pc_target_from_senses(max_dist);
        self.set_pc_target(target);
    }

    /// Update the actor under the player's reticle and fire JIP's
    /// `SetOnPCTargetChangeEventHandler` (`pctargetchange`) if it changed: no calling
    /// reference, 1 argument (`newTargetRef` or `0` for none). Non-actor references
    /// normalize to `None` [documented: SetOnPCTargetChangeEventHandler,
    /// docs/spec-notes/jip-events.md §2]. Returns how many handlers ran.
    pub fn set_pc_target(&mut self, target: Option<FormId>) -> usize {
        let next = target.filter(|&r| self.is_pc_target_actor(r));
        if next == self.jip.pc_target {
            return 0;
        }
        self.jip.pc_target = next;
        self.jip_event("pctargetchange", None, vec![XVal::Ref(next.map_or(0, |r| r.0))])
    }

    fn control_edges(&mut self, now: u32) {
        let before = std::mem::replace(&mut self.jip.controls_held, now);
        let changed = now ^ before;
        if changed == 0 { return; }
        let jip = self.jip.handlers.iter().any(|h| h.event == "controldown" || h.event == "controlup");
        let named = self.handlers.iter().any(|h| h.event.starts_with("oncontroldown:") || h.event.starts_with("oncontrolup:"));
        if !jip && !named { return; }
        // Snapshot all edges before callbacks can rebind or disable controls. The
        // existing dispatchers own filters, callable admission and source context.
        for c in 0..crate::script_input::CONTROLS {
            if changed & (1 << c) == 0 { continue; }
            let down = now & (1 << c) != 0;
            if jip {
                self.jip_event(if down { "controldown" } else { "controlup" }, None, vec![XVal::Num(c as f64)]);
            }
            if named {
                self.dispatch_event(&format!("OnControl{}:{c}", if down { "Down" } else { "Up" }), vec![XVal::Num(c as f64)], None);
            }
        }
    }

    fn jip_key_edges(&mut self) {
        if self.jip.keys_held == self.input.held {
            return;
        }
        let now = self.input.held.clone();
        let before = std::mem::replace(&mut self.jip.keys_held, now.clone());
        let jip = self.jip.handlers.iter().any(|h| h.event == "keydown" || h.event == "keyup");
        let named = self.handlers.iter().any(|h| {
            h.event == "onkeydown"
                || h.event == "onkeyup"
                || h.event.starts_with("onkeydown:")
                || h.event.starts_with("onkeyup:")
        });
        if !jip && !named {
            return;
        }
        for &k in now.difference(&before) {
            let code = XVal::Num(f64::from(k));
            if jip {
                self.jip_event("keydown", None, vec![code.clone()]);
            }
            if named {
                self.dispatch_event("OnKeyDown", vec![code.clone()], None);
                self.dispatch_event(&format!("OnKeyDown:{k}"), vec![code], None);
            }
        }
        for &k in before.difference(&now) {
            let code = XVal::Num(f64::from(k));
            if jip {
                self.jip_event("keyup", None, vec![code.clone()]);
            }
            if named {
                self.dispatch_event("OnKeyUp", vec![code.clone()], None);
                self.dispatch_event(&format!("OnKeyUp:{k}"), vec![code], None);
            }
        }
    }

    fn button_edges(&mut self, now: u32) {
        let before = std::mem::replace(&mut self.jip.buttons_held, now);
        let changed = now ^ before;
        if changed == 0 {
            return;
        }
        let named = self.handlers.iter().any(|h| {
            h.event == "onbuttondown"
                || h.event == "onbuttonup"
                || h.event.starts_with("onbuttondown:")
                || h.event.starts_with("onbuttonup:")
        });
        if !named {
            return;
        }
        for bit in 0..u32::BITS {
            let mask = 1u32 << bit;
            if changed & mask == 0 {
                continue;
            }
            let down = now & mask != 0;
            let args = vec![XVal::Num(f64::from(mask))];
            self.dispatch_event(if down { "OnButtonDown" } else { "OnButtonUp" }, args.clone(), None);
            let prefix = if down { "onbuttondown:" } else { "onbuttonup:" };
            self.dispatch_event_matching(
                |ev| ev.strip_prefix(prefix).and_then(|s| s.parse::<u32>().ok()).is_some_and(|m| m & mask != 0),
                args,
                None,
            );
        }
    }

    /// An item was made in the recipe menu: SetOnCraftingEventHandler's event, with the
    /// item's base form and the quantity [documented: SetOnCraftingEventHandler]. Called
    /// once per output of the recipe, with all of that output the making gave
    /// [hypothesis: the page does not say how a recipe made several times, or one with
    /// two outputs, is reported].
    pub fn item_crafted(&mut self, item: FormId, quantity: i64) {
        self.jip_event("crafting", None, vec![XVal::Ref(item.0), XVal::Num(quantity as f64)]);
    }

    /// Fire JIP's `SetOnFireWeaponEventHandler` event (`fireweapon`) when an actor fires
    /// a ranged weapon: `this = actor`, `arg0 = weapon` base form
    /// [documented: SetOnFireWeaponEventHandler, docs/spec-notes/jip-events.md §2].
    ///
    /// Excluded:
    /// - non-actors (e.g. trap activators firing via `FireWeapon`);
    /// - melee/hand-to-hand weapons (WEAP DNAM `animation_type` 0..=2);
    /// - the player while god mode (`ToggleGodMode`) is active (demigod /
    ///   `ToggleImmortalMode` still fires) [documented: SetOnFireWeaponEventHandler].
    ///
    /// When `dedup_same_frame` is true (`Game::projectile_fired`), skip if
    /// `(self.frame, actor, weapon)` already fired this frame so a player shot
    /// (`weapon_fired` followed by `projectile_fired`) or a multi-pellet shotgun burst
    /// fires once per round [hypothesis]. Player normal firing (`so=1,jp=1`) and player
    /// god-mode suppression (`so=1,jp=0`) are [observed 2026-11-24: Fallout: New Vegas
    /// 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 + ShowOffNVSE 184, 10 official base masters,
    /// `GSDocMitchellHouse`, committed probe `w6t2p3_04_fire_god_melee`
    /// (`tools/capture/nvse/catalogs/wave6_t2_observed_matches.json`):
    /// `norm(so=1,jp=1)|god(so=1,jp=0)|melee(so=1,jp=1)`].
    pub(crate) fn fire_weapon_jip_event(&mut self, actor: FormId, weapon: FormId, dedup_same_frame: bool) -> usize {
        let is_actor = actor == FormId::PLAYER_REF
            || matches!(
                crate::functions::base_of(self, actor).and_then(|b| self.data.sig(b)).as_ref(),
                Some(b"NPC_") | Some(b"CREA")
            );
        if !is_actor {
            return 0;
        }
        if matches!(self.data.weapon_anim_types.get(&weapon), Some(0..=2)) {
            return 0;
        }
        if actor == FormId::PLAYER_REF && self.toggles.god_mode {
            return 0;
        }
        if self.jip.fired_this_frame.0 != self.frame {
            self.jip.fired_this_frame = (self.frame, Vec::new());
        }
        let key = (actor, weapon);
        if dedup_same_frame && self.jip.fired_this_frame.1.contains(&key) {
            return 0;
        }
        if !self.jip.fired_this_frame.1.contains(&key) {
            self.jip.fired_this_frame.1.push(key);
        }
        self.jip_event("fireweapon", Some(actor), vec![XVal::Ref(weapon.0)])
    }

    /// Fire JIP's `SetOnReloadWeaponEventHandler` event (`reloadweapon`) when an actor
    /// starts reloading `weapon`: `this = actor`, `arg0 = weapon` base form
    /// [documented: SetOnReloadWeaponEventHandler, docs/spec-notes/jip-events.md §2].
    pub fn weapon_reloaded(&mut self, actor: FormId, weapon: FormId) -> usize {
        self.jip_event("reloadweapon", Some(actor), vec![XVal::Ref(weapon.0)])
    }

    /// The main-loop callbacks due this frame (`Game::tick`).
    pub(crate) fn run_main_loop(&mut self) {
        if self.jip.main_loop.is_empty() {
            return;
        }
        let menu = self.menu.is_some() || self.dialogue.is_some();
        let want = if menu { 2 } else { 1 };
        let frame = self.frame;
        let due: Vec<(MainLoop, Option<Arc<Registrar>>)> = self
            .jip
            .main_loop
            .iter()
            .filter(|m| m.due <= frame && m.flags & want != 0)
            .map(|m| (m.clone(), self.jip.registrars.get(&m.seq).cloned()))
            .collect();
        // Main-loop callbacks observe the single-frame delta, not accumulated time across
        // skipped callDelay frames [observed: w4g_03_jip_callback_getsecondspassed].
        let seconds = self.stateless_udf_seconds();
        for (m, registrar) in due {
            for x in self.jip.main_loop.iter_mut().filter(|x| x.func == m.func && x.on == m.on) {
                x.due = frame + x.delay;
            }
            let on = m.on.unwrap_or(FormId::PLAYER_REF);
            let ctx = RunCtx { self_ref: Some(on), ..RunCtx::default() };
            if let Err(error) = self.run_function_with_seconds(&m.func, Some(Value::Ref(on.0)), Vec::new(), None, (ctx, Some(seconds))) {
                self.handler_failed_with_registrar(
                    "JIP main-loop callback".into(),
                    error,
                    || Some(m.failure_context()),
                    registrar.as_deref(),
                );
            }
        }
    }
}

#[cfg(test)]
mod click_path_tests {
    use super::*;

    #[test]
    fn click_path_matches_wildcards_and_indices() {
        let path = "BarterMenu/BarterCategories/BC_Category:1";
        assert!(click_path_matches("BarterMenu/BarterCategories", "BarterMenu", path, -1.0));
        assert!(click_path_matches("BarterMenu\\BarterCategories\\BC_Category", "BarterMenu", path, -1.0));
        assert!(click_path_matches("BarterMenu/*/BC_Category", "BarterMenu", path, -1.0));
        assert!(click_path_matches("BarterMenu/*/BC_Category:1", "BarterMenu", path, -1.0));
        assert!(click_path_matches("BarterMenu/*/*:1", "BarterMenu", path, -1.0));
        assert!(!click_path_matches("BarterMenu/*/BC_Category:0", "BarterMenu", path, -1.0));
        assert!(!click_path_matches("BarterMenu/*/*:2", "BarterMenu", path, -1.0));
        assert!(!click_path_matches("ContainerMenu/BarterCategories", "BarterMenu", path, -1.0));
        assert!(click_path_matches("bartermenu#12", "BarterMenu", path, 12.0));
        assert!(!click_path_matches("bartermenu#12", "BarterMenu", path, -1.0));
    }
}
