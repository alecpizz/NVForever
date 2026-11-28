//! kNVSE's animation functions, second slice (#1018): the animation clock (Get/
//! SetGlobalAnimTime, Get/SetAnimCurrentTime), weights, additive poses, text keys, the
//! blends, the attributes of a sequence, and AllowAttack / ForceFireWeapon /
//! CopyAnimationsToForm [documented: each page in the wiki export; observed: the probe
//! sets knvse-anim and knvse2, docs/spec-notes/ext-knvse.md]. As the first slice
//! (`crate::nvse_anim`), the game keeps what scripts asked and answers from it and from
//! the clip headers the engine publishes; the engine consumes weights, additive poses,
//! blend transitions, and sequence clocks during player playback (ext-knvse.md §4).

use crate::game::{Game, RunCtx};
use crate::nvse_anim::{file_exists, same};
use fnv_script::vm::{ArrayKind, XKey, XVal};
use fnv_sim::FormId;
use std::collections::HashMap;

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

// Transition helpers predate the explicit renderer requests. Keep their immediate
// readbacks and the rendered request on the same state, rather than writing only
// the legacy map now hidden by ScriptSequence's targeted clock and weight.
fn transition_weight(g: &mut Game, actor: FormId, first: bool, path: &str, weight: f64) {
    let clamped = weight.clamp(0.0, 1.0);
    g.knvse.weights.insert((actor, key(path)), clamped);
    if let Some(sequence) = g.anim_paths.sequences.get_mut(&(actor, first, key(path))) {
        sequence.weight = clamped as f32;
    }
}

/// Align `dst`'s clock to a currently playing `timeSync` sequence on `(actor, first)`
/// [design: scales by normalized phase `(src_t - src_begin) / src_span` when both
/// `SequenceInfo` headers have positive finite spans, else copies `src`'s current clock
/// time; returns `false` when `sync` is empty or not playing so callers still run the
/// blend transition with `destFrame` or the default clock rather than failing].
fn apply_time_sync(g: &mut Game, actor: FormId, first: bool, sync: &str, dst: &str) -> bool {
    if sync.is_empty() || !g.plays_for(actor, first, sync) {
        return false;
    }
    let src_time = g.script_sequence_time_for(actor, sync, first);
    if !src_time.is_finite() {
        return false;
    }
    let synced = match (g.knvse.sequences.get(&key(sync)), g.knvse.sequences.get(&key(dst))) {
        (Some(si), Some(di)) => {
            let src_span = si.end - si.begin;
            let dst_span = di.end - di.begin;
            if src_span.is_finite() && src_span > 1e-6 && dst_span.is_finite() && dst_span > 1e-6 {
                let phase = ((src_time - si.begin) / src_span).rem_euclid(1.0);
                di.begin + phase * dst_span
            } else {
                src_time
            }
        }
        _ => src_time,
    };
    if !synced.is_finite() {
        return false;
    }
    transition_offset(g, actor, first, dst, synced);
    true
}

fn transition_offset(g: &mut Game, actor: FormId, first: bool, path: &str, time: f64) {
    if !time.is_finite() {
        return;
    }
    let now = g.anim_clock(actor, false);
    g.knvse.offsets.insert((actor, key(path)), time - now);
    let clock = g.script_sequence_clock(actor, first);
    let seq_key = (actor, first, key(path));
    if let Some(sequence) = g.anim_paths.sequences.get_mut(&seq_key) {
        sequence.clock_offset = time - clock;
        g.anim_paths.sequence_clocks.insert(
            seq_key,
            crate::nvse_anim::SequenceClock {
                initialized: true,
                current_time: time,
                last_clock: clock,
            },
        );
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

/// A path as a map key: lower case, backslashes.
fn key(p: &str) -> String {
    p.replace('/', "\\").to_ascii_lowercase()
}

/// A sequence's text keys: time and text, in time order.
pub type TextKeys = Vec<(f64, String)>;

/// What a sequence file's controller sequence says, as the engine read it
/// ([`Game::set_sequence_info`]).
#[derive(Debug, Clone, PartialEq)]
pub struct SequenceInfo {
    pub name: String,
    pub accum_root: String,
    pub frequency: f64,
    pub begin: f64,
    pub end: f64,
    /// nif.xml CycleType: 0 loop, 1 reverse, 2 clamp.
    pub cycle: i64,
    pub text_keys: Vec<(f64, String)>,
}

/// What scripts asked of the actors' animations this session (second slice).
#[derive(Debug, Default)]
pub struct KnvseState {
    /// When the game first knew each actor's animations (game seconds).
    pub clock_start: HashMap<FormId, f64>,
    /// Menu seconds owned by the player's first-person device animations (pipboy.md §2).
    pub(crate) first_person_menu_seconds: f64,
    pub(crate) first_person_menu_start: Option<f64>,
    /// SetGlobalAnimTime: (actor, first person) -> (the value, that view's clock at the set).
    pub global: HashMap<(FormId, bool), (f64, f64)>,
    /// SetAnimCurrentTime's offsets and SetAnimWeight's weights, by (actor, path).
    pub offsets: HashMap<(FormId, String), f64>,
    pub weights: HashMap<(FormId, String), f64>,
    /// SetAnimAdditive: (actor, path) -> (first person, reference pose, its time).
    pub additive: HashMap<(FormId, String), (bool, String, f64)>,
    /// Per-view additive pose config so first- and third-person sequences of the same
    /// path do not overwrite each other [design].
    pub additive_views: HashMap<(FormId, bool, String), (String, f64)>,
    /// Text keys scripts gave a sequence: for every actor (`None`: AddAnimTextKey, or
    /// SetAnimTextKeys before a game is loaded) or one actor's.
    pub text_keys: HashMap<(Option<FormId>, String), TextKeys>,
    /// The engine's reading of the sequence files ([`Game::set_sequence_info`]).
    pub sequences: HashMap<String, SequenceInfo>,
    /// AllowAttack calls, for the engine to take.
    pub allow_attack: Vec<FormId>,
    /// SetAnimCurrentTime calls waiting for the next frame: (actor, path, time).
    pub pending_times: Vec<(FormId, String, f64)>,
    pub pending_sequence_times: Vec<(FormId, bool, String, f64)>,
}

impl Game<'_> {
    /// The device animates while its menu holds the world. Other actors/views keep
    /// game time; neither loading nor unrelated menus adds time here (pipboy.md §2).
    pub(crate) fn advance_first_person_menu_clock(&mut self) {
        let dt = self.cfg.frame_seconds;
        if !self.loading && self.in_menu() && self.menu_open(crate::game::menus::PIPBOY) && dt.is_finite() && dt > 0.0 {
            self.knvse.first_person_menu_seconds += dt;
        }
    }

    pub(crate) fn script_sequence_now(&self, actor: FormId, first: bool) -> f64 {
        self.time + if actor == FormId::PLAYER_REF && first { self.knvse.first_person_menu_seconds } else { 0.0 }
    }

    /// Menu time actually admitted by game ticks, for the renderer's first-person
    /// pose clock. Retained frame configuration is not a new animation frame.
    pub fn first_person_menu_animation_time(&self) -> f64 {
        self.knvse.first_person_menu_seconds
    }

    /// The engine tells what a sequence file's controller sequence holds (its name,
    /// accumulation root, frequency, key times, cycle and text keys), for
    /// GetAnimationAttributes.
    pub fn set_sequence_info(&mut self, path: &str, info: SequenceInfo) {
        self.knvse.sequences.insert(key(path), info);
    }

    /// The engine takes the actors AllowAttack let attack again.
    pub fn take_allow_attack(&mut self) -> Vec<FormId> {
        std::mem::take(&mut self.knvse.allow_attack)
    }

    /// A sequence started on an actor (PlayAnimationPath, ActivateAnim, the blends): its
    /// current time counts from 0 [observed: knvse2, GetAnimCurrentTime under 0.1 the
    /// frame after PlayAnimationPath].
    pub(crate) fn knvse_started(&mut self, a: FormId, path: &str) {
        let now = self.anim_clock(a, false);
        self.knvse.offsets.insert((a, key(path)), -now);
    }

    /// SetAnimCurrentTime's calls take effect at the start of the next frame [observed:
    /// knvse2, the same frame reads the time from before]. Called from [`Game::tick`].
    pub(crate) fn knvse_tick(&mut self) {
        for (a, k, t) in std::mem::take(&mut self.knvse.pending_times) {
            let now = self.anim_clock(a, false);
            self.knvse.offsets.insert((a, k), t - now);
        }
        for (actor, first, path, time) in std::mem::take(&mut self.knvse.pending_sequence_times) {
            let clock = self.script_sequence_clock(actor, first);
            let offset = time - clock;
            let seq_key = (actor, first, path.clone());
            if let Some(sequence) = self.anim_paths.sequences.get_mut(&seq_key) {
                sequence.clock_offset = offset;
                self.anim_paths.sequence_clocks.insert(
                    seq_key,
                    crate::nvse_anim::SequenceClock {
                        initialized: true,
                        current_time: time,
                        last_clock: clock,
                    },
                );
            }
            if let Some(paused) = self.anim_paths.sequence_resume.get_mut(&(actor,first,path)) { paused.0 = time; }
        }
        self.script_sequences_tick();
    }

    /// GetGlobalAnimTime: what SetGlobalAnimTime gave plus the game seconds since, else
    /// the game seconds since the game first knew the actor's animations, that first
    /// frame counted [observed: knvse-anim, Ceil of it reads 1 a few frames after
    /// placing an actor] (clock start: see docs/spec-notes/ext-knvse.md §1).
    fn anim_clock(&mut self, a: FormId, first: bool) -> f64 {
        let now = self.script_sequence_now(a, first);
        // Admit the default origins even when this view has an explicit value:
        // starting the other view must not require a prior getter (ext-knvse.md §2).
        let start = *self.knvse.clock_start.entry(a).or_insert(self.time);
        let menu = if a == FormId::PLAYER_REF {
            let began = *self.knvse.first_person_menu_start.get_or_insert(self.knvse.first_person_menu_seconds);
            if first { self.knvse.first_person_menu_seconds - began } else { 0.0 }
        } else { 0.0 };
        if let Some(&(v, at)) = self.knvse.global.get(&(a, first)) {
            return v + (now - at);
        }
        self.time - start + menu + self.cfg.frame_seconds
    }

    /// The text keys of a sequence: the actor's own, else every actor's, else the
    /// file's.
    fn text_keys_of(&self, a: Option<FormId>, path: &str) -> Vec<(f64, String)> {
        let k = key(path);
        a.and_then(|a| self.knvse.text_keys.get(&(Some(a), k.clone())))
            .or_else(|| self.knvse.text_keys.get(&(None, k.clone())))
            .cloned()
            .or_else(|| self.knvse.sequences.get(&k).map(|s| s.text_keys.clone()))
            .unwrap_or_default()
    }

    /// Additive pose configuration for `(actor, first, path)` set by `SetAnimAdditive`:
    /// `(reference_pose_path, reference_time)`. Empty reference path means bind/rest pose
    /// delta [documented: SetAnimAdditive; design].
    pub fn sequence_additive(&self, actor: FormId, first: bool, path: &str) -> Option<(&str, f32)> {
        let k = key(path);
        if let Some((pose, time)) = self.knvse.additive_views.get(&(actor, first, k.clone())) {
            return Some((pose.as_str(), *time as f32));
        }
        self.knvse
            .additive
            .get(&(actor, k))
            .filter(|(f, _, _)| *f == first)
            .map(|(_, pose, time)| (pose.as_str(), *time as f32))
    }

    fn plays(&self, a: FormId, path: &str) -> bool {
        self.playing_sequences(a).iter().any(|p| same(p, path))
    }

    fn plays_for(&self, a: FormId, first: bool, path: &str) -> bool {
        self.playing_sequences_for(a, first).iter().any(|p| same(p, path))
    }
}

/// The sequence type of a movement group: Idle 0, any other locomotion group Movement 1
/// [documented: GetAnimationAttributes' Sequence Type list; hypothesis: the body's clip is
/// one of the two]. A sequence that is not the body's: -1, None.
fn sequence_type(g: &Game, a: Option<FormId>, path: &str) -> i64 {
    let Some(st) = a.and_then(|a| g.senses.anim.get(&a)) else { return -1 };
    if !st.clip.as_deref().is_some_and(|c| same(c, path)) {
        return -1;
    }
    match st.group {
        Some(gr) if gr.eq_ignore_ascii_case("idle") => 0,
        Some(_) => 1,
        None => -1,
    }
}

/// GetAnimationAttributes' string map [documented: its 13 keys, and 9 more "when called
/// on an actor" that plays it; observed: knvse-anim, 22 keys for a sequence playing on an
/// actor]. What the engine has not told of the file reads as a default:
/// the file's stem as its name, frequency 1, keys 0 to 0, a loop, root "Bip01"
/// [hypothesis: defaults until the engine tells, ext-knvse.md §2].
fn attributes(g: &mut Game, a: Option<FormId>, path: &str) -> XVal {
    let k = key(path);
    let info = g.knvse.sequences.get(&k).cloned().unwrap_or_else(|| SequenceInfo {
        name: std::path::Path::new(&k.replace('\\', "/")).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
        accum_root: "Bip01".into(),
        frequency: 1.0,
        begin: 0.0,
        end: 0.0,
        cycle: 0,
        text_keys: Vec::new(),
    });
    let group = a.and_then(|a| g.senses.anim.get(&a)).filter(|s| s.clip.as_deref().is_some_and(|c| same(c, path))).and_then(|s| s.group).unwrap_or("");
    let keys = g.text_keys_of(a, path);
    let weight = a.and_then(|a| {
        let first = a == FormId::PLAYER_REF && g.senses.first_person;
        g.anim_paths.sequences.get(&(a,first,k.clone())).map(|s|f64::from(s.weight))
            .or_else(||g.knvse.weights.get(&(a,k.clone())).copied())
    }).unwrap_or(1.0);
    let times = keys.iter().enumerate().map(|(i, (t, _))| (XKey::Num(i as f64), XVal::Num(*t))).collect();
    let values = keys.iter().enumerate().map(|(i, (_, v))| (XKey::Num(i as f64), XVal::Str(v.clone()))).collect();
    let times = XVal::Array(g.ext.new_array(ArrayKind::List, times));
    let values = XVal::Array(g.ext.new_array(ArrayKind::List, values));
    let mut m: Vec<(&str, XVal)> = vec![
        ("accumRootName", XVal::Str(info.accum_root.clone())),
        // [hypothesis: the group's number is not kept; -1 as the Sequence Type list's None]
        ("animGroupId", XVal::Num(-1.0)),
        ("sequenceType", XVal::Num(sequence_type(g, a, path) as f64)),
        // Clamp 0, looping 1 [documented: Key Type list; hypothesis: from the cycle].
        ("keyType", XVal::Num(if info.cycle == 2 { 0.0 } else { 1.0 })),
        ("animGroupName", XVal::Str(group.to_string())),
        ("sequenceName", XVal::Str(info.name.clone())),
        ("seqWeight", XVal::Num(weight)),
        ("cycleType", XVal::Num(info.cycle as f64)),
        ("frequency", XVal::Num(info.frequency)),
        ("beginKeyTime", XVal::Num(info.begin)),
        ("endKeyTime", XVal::Num(info.end)),
        ("textKeyTimes", times),
        ("textKeyValues", values),
    ];
    if let Some(a) = a.filter(|&a| g.plays(a, path)) {
        let first = a == FormId::PLAYER_REF && g.senses.first_person;
        let seq_key = (a, first, k.clone());
        let (offset, last_time, calc_time) = if let Some(sequence) = g.anim_paths.sequences.get(&seq_key) {
            if g.anim_paths.sequence_clocks.get(&seq_key).is_some_and(|sc| !sc.initialized) {
                // `calculatedTime` / `GetAnimCurrentTime` reads `-f32::MAX` before the
                // first tick [observed: `w4e_08_knvse_legacy_anim_time`, #3803];
                // returning `-f32::MAX` for pre-first-tick `offset` and `lastTime` is
                // [hypothesis] (`w4e_08` on `main` reads only `t0L` pre-first-tick).
                let invalid = f64::from(-f32::MAX);
                (invalid, invalid, invalid)
            } else {
                // `offset` stays fixed and `lastTime` stays unscaled (`global + offset`),
                // while `calculatedTime` / `GetAnimCurrentTime` integrates frequency-scaled
                // deltas [observed: `w4e_08_knvse_legacy_anim_time`, #3803].
                let unscaled = g.script_sequence_clock(a, first) + sequence.clock_offset;
                let scaled = g.script_sequence_time(a, path);
                (sequence.clock_offset, unscaled, scaled)
            }
        } else {
            let global = g.anim_clock(a, false);
            let offset = g.knvse.offsets.get(&(a, k.clone())).copied().unwrap_or(0.0);
            let now = global + offset;
            (offset, now, now)
        };
        m.extend([
            ("lastTime", XVal::Num(last_time)),
            ("weightedLastTime", XVal::Num(last_time)),
            ("lastScaledTime", XVal::Num(last_time * info.frequency)),
            // kState_Animating [documented: the State list].
            ("state", XVal::Num(1.0)),
            ("offset", XVal::Num(offset)),
            ("startTime", XVal::Num(info.begin)),
            ("endTime", XVal::Num(info.end)),
            ("calculatedTime", XVal::Num(calc_time)),
            ("multiplier", XVal::Num(1.0)),
        ]);
    }
    let entries = m.into_iter().map(|(k, v)| (XKey::Str(k.to_string()), v)).collect();
    XVal::Array(g.ext.new_array(ArrayKind::StringMap, entries))
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, _ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let zero = XVal::Num(0.0);
    let a = subject.unwrap_or(FormId::PLAYER_REF);
    let v = match name {
        "getglobalanimtime" => XVal::Num(g.anim_clock(a, num(args, 0, 0.0) != 0.0)),
        // `time [firstPerson]` [documented: SetGlobalAnimTime].
        "setglobalanimtime" => {
            let first = num(args, 1, 0.0) != 0.0;
            let now = g.script_sequence_now(a, first);
            g.knvse.global.insert((a, first), (num(args, 0, 0.0), now));
            zero
        }
        // The sequence's offset plus the actor's clock [documented: GetAnimCurrentTime,
        // "the sum of the offset field … and … GetGlobalAnimTime"]; the offset starts at 0
        // [hypothesis].
        "getanimcurrenttime" => {
            let path = text(args, 0);
            if !g.knvse.clock_start.contains_key(&a) { g.anim_clock(a,false); }
            XVal::Num(g.script_sequence_time(a,&path))
        }
        // `path time`: from the next frame, the offset that makes the current time `time`
        // [documented; observed: knvse2, the same frame reads the time from before].
        "setanimcurrenttime" => {
            let (path, t) = (text(args, 0), num(args, 1, 0.0));
            g.knvse.pending_times.push((a, key(&path), t));
            g.knvse.pending_sequence_times.push((a, a == FormId::PLAYER_REF && g.senses.first_person, key(&path), t));
            zero
        }
        // `path weight` [documented: SetAnimWeight]; read back as seqWeight.
        "setanimweight" => {
            let path = key(&text(args, 0));
            let weight = num(args, 1, 1.0);
            if !weight.is_finite() {
                return Some(Ok(zero));
            }
            let current_first = a == FormId::PLAYER_REF && g.senses.first_person;
            let first = if g.anim_paths.sequences.contains_key(&(a, current_first, path.clone())) {
                current_first
            } else if a == FormId::PLAYER_REF && g.anim_paths.sequences.contains_key(&(a, !current_first, path.clone())) {
                !current_first
            } else {
                current_first
            };
            let key = (a, first, path.clone());
            let weight = weight.clamp(0.0, 1.0) as f32;
            g.anim_paths.sequence_resume.entry(key.clone()).or_insert((0.0, 1.0)).1 = weight;
            if let Some(sequence) = g.anim_paths.sequences.get_mut(&key) {
                sequence.weight = weight;
            }
            g.knvse.weights.insert((a, path), f64::from(weight));
            zero
        }
        // `path [firstPerson] [refPose] [refTime]`: 1 when it can be applied [documented:
        // SetAnimAdditive; hypothesis: when the files exist].
        "setanimadditive" => {
            let (path, pose) = (text(args, 0), text(args, 2));
            let ok = file_exists(&path) && (pose.is_empty() || file_exists(&pose));
            if ok {
                let first = a == FormId::PLAYER_REF && num(args, 1, 0.0) != 0.0;
                let ref_time = num(args, 3, 0.0);
                let k = key(&path);
                g.knvse.additive.insert((a, k.clone()), (first, pose.clone(), ref_time));
                g.knvse.additive_views.insert((a, first, k), (pose, ref_time));
            }
            XVal::Num(f64::from(u8::from(ok)))
        }
        // `path time key`: one text key more, for every actor [documented: AddAnimTextKey;
        // hypothesis: it returns 1 when the file exists, else adds nothing and gives 0].
        "addanimtextkey" => {
            let path = text(args, 0);
            if !file_exists(&path) {
                return Some(Ok(zero));
            }
            let mut keys = g.text_keys_of(None, &path);
            keys.push((num(args, 1, 0.0), text(args, 2)));
            keys.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
            g.knvse.text_keys.insert((None, key(&path)), keys);
            XVal::Num(1.0)
        }
        // `path times values`: the sequence's text keys, the actor's own; nothing while the
        // actor plays it, or when the lists differ in length [documented: SetAnimTextKeys;
        // hypothesis: the lengths]. No reference: every actor's (the main menu's case).
        "setanimtextkeys" => {
            let path = text(args, 0);
            let read = |g: &Game, k: usize| -> Vec<XVal> {
                match args.get(k) {
                    Some(XVal::Array(id)) => g.ext.array(*id).map(|ar| ar.entries.iter().map(|(_, v)| v.clone()).collect()).unwrap_or_default(),
                    _ => Vec::new(),
                }
            };
            let (times, values) = (read(g, 1), read(g, 2));
            let owner = subject;
            if times.len() != values.len() || owner.is_some_and(|o| g.plays(o, &path)) {
                return Some(Ok(zero));
            }
            let keys = times
                .iter()
                .zip(&values)
                .map(|(t, v)| {
                    let t = if let XVal::Num(x) = t { *x } else { 0.0 };
                    let v = if let XVal::Str(s) = v { s.clone() } else { String::new() };
                    (t, v)
                })
                .collect();
            g.knvse.text_keys.insert((owner, key(&path)), keys);
            zero
        }
        // `from to`: the forms' animation overrides (SetActorAnimationPath's and
        // SetWeaponAnimationPath's) copied, both views [documented: CopyAnimationsToForm;
        // hypothesis: added to the target's own]; the AnimGroupOverride folders' entries
        // too, with their conditions (`crate::nvse_anim_folders`).
        "copyanimationstoform" => {
            let (Some(from), Some(to)) = (form(args, 0), form(args, 1)) else { return Some(Ok(zero)) };
            g.copy_folder_entries(from, to);
            // A copied path keeps its condition (SetActorAnimationPath's).
            let conds: Vec<_> = g.anim_paths.conditions.iter().filter(|((a, _, _), _)| *a == from).map(|((_, first, p), c)| ((to, *first, p.clone()), c.clone())).collect();
            g.anim_paths.conditions.extend(conds);
            for map in [&mut g.anim_paths.actor, &mut g.anim_paths.weapon] {
                for first in [false, true] {
                    let Some(src) = map.get(&(from, first)).cloned() else { continue };
                    let dst = map.entry((to, first)).or_default();
                    for p in src {
                        if !dst.iter().any(|d| same(d, &p)) {
                            dst.push(p);
                        }
                    }
                }
            }
            zero
        }
        // `source dest [first] [ease] [priority] [startOver] [weight] [timeSync]`: the
        // source must play and the destination not; then the source stops (fading over
        // `ease`) and the destination plays at `weight` (default 1) with `ease`,
        // `priority`, and optional `timeSync` phase alignment; 1 / 0 [documented:
        // CrossFadeAnims, "simply a convenience function that calls DeactivateAnim on the
        // source sequence and ActivateAnim on the destination"].
        "crossfadeanims" => {
            let (src, dst) = (text(args, 0), text(args, 1));
            let first = a == FormId::PLAYER_REF && num(args, 2, f64::from(u8::from(g.senses.first_person))) != 0.0;
            if !g.plays_for(a, first, &src) || g.plays_for(a, first, &dst) || !file_exists(&dst) {
                return Some(Ok(zero));
            }
            let ease = args.get(3).map(|_| num(args, 3, 0.2));
            let priority = num(args, 4, 1.0);
            let start_over = num(args, 5, 1.0);
            let weight = num(args, 6, 1.0);
            let sync = text(args, 7);
            if !priority.is_finite()
                || !weight.is_finite()
                || ease.is_some_and(|v| !v.is_finite() || v < 0.0 || v > f64::from(f32::MAX))
            {
                return Some(Ok(zero));
            }
            let mut activate_args = vec![
                XVal::Str(dst.clone()),
                XVal::Num(f64::from(u8::from(first))),
                XVal::Num(priority),
                XVal::Num(start_over),
                XVal::Num(weight),
            ];
            if let Some(e) = ease {
                activate_args.push(XVal::Num(e));
            }
            let Some(Ok(XVal::Num(started))) = crate::nvse_anim::call(g, "activateanim", Some(a), &activate_args) else {
                return Some(Ok(zero));
            };
            if started == 0.0 {
                return Some(Ok(zero));
            }
            apply_time_sync(g, a, first, &sync, &dst);
            crate::nvse_anim::deactivate_anim_for(g, a, first, &src, ease.unwrap_or(0.2));
            transition_weight(g, a, first, &dst, weight);
            XVal::Num(1.0)
        }
        // `source dest [first] [duration] [destFrame] [sourceWeight] [destWeight] [priority] [timeSync]`:
        // the destination plays at `destWeight` (default 1) over `duration` and a playing
        // source keeps `sourceWeight`, stopping at 0 (fading over `duration`)
        // [documented: BlendToAnim; hypothesis: the default source weight is 0]. 1 when
        // the destination exists, also when the source does not play [observed: knvse2,
        // 1 with a source not playing; the page says "The source sequence must be an
        // actively playing animation"].
        "blendtoanim" => {
            let (src, dst) = (text(args, 0), text(args, 1));
            if !file_exists(&dst) {
                return Some(Ok(zero));
            }
            let first = a == FormId::PLAYER_REF && num(args, 2, f64::from(u8::from(g.senses.first_person))) != 0.0;
            let duration = args.get(3).map(|_| num(args, 3, 0.2));
            let dest_frame = match args.get(4) {
                Some(XVal::Num(f)) => Some(*f),
                _ => None,
            };
            let sw = num(args, 5, 0.0);
            let dw = num(args, 6, 1.0);
            let priority = num(args, 7, 1.0);
            let sync = text(args, 8);
            if !priority.is_finite()
                || !sw.is_finite()
                || !dw.is_finite()
                || duration.is_some_and(|v| !v.is_finite() || v < 0.0 || v > f64::from(f32::MAX))
                || dest_frame.is_some_and(|v| !v.is_finite())
            {
                return Some(Ok(zero));
            }
            let mut activate_args = vec![
                XVal::Str(dst.clone()),
                XVal::Num(f64::from(u8::from(first))),
                XVal::Num(priority),
                XVal::Num(1.0),
                XVal::Num(dw),
            ];
            if let Some(d) = duration {
                activate_args.push(XVal::Num(d));
            }
            crate::nvse_anim::call(g, "activateanim", Some(a), &activate_args);
            if let Some(seq) = g.anim_paths.sequences.get_mut(&(a, first, key(&dst))) {
                seq.priority = priority as i32;
                if let Some(d) = duration {
                    seq.ease_in = Some(d as f32);
                }
            }
            transition_weight(g, a, first, &dst, dw);
            let synced = apply_time_sync(g, a, first, &sync, &dst);
            if sw <= 0.0 {
                crate::nvse_anim::deactivate_anim_for(g, a, first, &src, duration.unwrap_or(0.2));
            } else {
                transition_weight(g, a, first, &src, sw);
            }
            if !synced {
                if let Some(f) = dest_frame {
                    transition_offset(g, a, first, &dst, f);
                }
            }
            XVal::Num(1.0)
        }
        // `path [first] [duration] [destFrame] [priority] [timeSync]`: the sequence plays
        // from the pose over `duration` [documented: BlendToAnimFromPose; hypothesis: 1
        // when the file exists].
        "blendtoanimfrompose" => {
            let path = text(args, 0);
            if !file_exists(&path) {
                return Some(Ok(zero));
            }
            let first = a == FormId::PLAYER_REF && num(args, 1, f64::from(u8::from(g.senses.first_person))) != 0.0;
            let duration = args.get(2).map(|_| num(args, 2, 0.2));
            let dest_frame = match args.get(3) {
                Some(XVal::Num(f)) => Some(*f),
                _ => None,
            };
            let priority = num(args, 4, 1.0);
            let sync = text(args, 5);
            if !priority.is_finite()
                || duration.is_some_and(|v| !v.is_finite() || v < 0.0 || v > f64::from(f32::MAX))
                || dest_frame.is_some_and(|v| !v.is_finite())
            {
                return Some(Ok(zero));
            }
            let mut activate_args = vec![
                XVal::Str(path.clone()),
                XVal::Num(f64::from(u8::from(first))),
                XVal::Num(priority),
                XVal::Num(1.0),
                XVal::Num(1.0),
            ];
            if let Some(d) = duration {
                activate_args.push(XVal::Num(d));
            }
            crate::nvse_anim::call(g, "activateanim", Some(a), &activate_args);
            if let Some(seq) = g.anim_paths.sequences.get_mut(&(a, first, key(&path))) {
                seq.priority = priority as i32;
                if let Some(d) = duration {
                    seq.ease_in = Some(d as f32);
                }
            }
            if !apply_time_sync(g, a, first, &sync, &path) {
                if let Some(f) = dest_frame {
                    transition_offset(g, a, first, &path, f);
                }
            }
            XVal::Num(1.0)
        }
        // On an actor that does not play the sequence: ar_Null [observed: knvse2, a
        // sequence the actor never played reads ar_Size -1].
        "getanimationattributes" | "getanimattrs" => {
            let path = text(args, 0);
            match subject {
                Some(s) if !g.plays(s, &path) => XVal::Array(0),
                _ => attributes(g, subject, &path),
            }
        }
        // `type`: the attributes of what the actor plays of that sequence type, else
        // ar_Null [documented: GetPlayingAnimBySequenceType].
        "getplayinganimbysequencetype" | "getanimbytype" => {
            let want = num(args, 0, -2.0) as i64;
            let found = g.playing_sequences(a).into_iter().find(|p| sequence_type(g, Some(a), p) == want);
            match found {
                Some(p) => attributes(g, Some(a), &p),
                None => XVal::Array(0),
            }
        }
        // The anim action set to 0 [documented: AllowAttack, "Sets the current Anim
        // Action of the calling reference to 0"]; the engine takes the call.
        "allowattack" => {
            if let Some(s) = g.senses.anim.get_mut(&a) {
                s.action = Some(crate::anim_state::AnimAction::Equip);
            }
            crate::nvse_vheyes::queue_once(&mut g.knvse.allow_attack, a);
            zero
        }
        // The equipped weapon fires, as FireWeapon (`CombatRequest::Fire`) [documented:
        // ForceFireWeapon]; nothing without one.
        "forcefireweapon" => {
            if let Some(w) = crate::functions::items::equipped_weapon(g, a) {
                g.combat_requests.push(crate::combat::CombatRequest::Fire { from: a, weapon: w, muzzle: None });
            }
            zero
        }
        _ => return None,
    };
    Some(Ok(v))
}
