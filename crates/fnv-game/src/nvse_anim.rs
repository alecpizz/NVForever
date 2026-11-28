//! kNVSE's animation functions, first slice (N5 of docs/handoff/nvse-next-batch.md;
//! docs/design/knvse-anim.md) [documented: their pages; observed: the peer's knvse-anim
//! set, kNVSE 37]. The game keeps what scripts asked (replacement paths by actor and
//! weapon, sequences a script played) and answers the getters from the clip the engine
//! says each actor's body plays (`AnimState::clip`). Explicit player sequence
//! requests feed the engine's script-sequence layer; direct NPC sequence playback
//! and the remaining replacement paths stay outside that bounded bridge.

use crate::game::Game;
use fnv_script::vm::{ArrayKind, XKey, XVal};
use fnv_sim::FormId;
use std::collections::HashMap;

/// What scripts asked of actors' and weapons' animations this session.
#[derive(Debug, Default)]
pub struct AnimPaths {
    /// SetActorAnimationPath's replacements: (actor, first person) -> paths.
    pub actor: HashMap<(FormId, bool), Vec<String>>,
    /// SetWeaponAnimationPath's: (weapon, first person) -> paths.
    pub weapon: HashMap<(FormId, bool), Vec<String>>,
    /// SetActorAnimationPath's conditions: (actor, first person, path in lower case) ->
    /// the condition function; the path applies only while it returns true
    /// [documented: SetActorAnimationPath, `pollConditionFunction conditionFunction`].
    pub conditions: HashMap<(FormId, bool, String), XVal>,
    /// Sequences a script played on an actor (PlayAnimationPath, ActivateAnim), in the
    /// script's spelling [observed: "sequences loaded by a script keep the script's
    /// spelling"].
    pub played: HashMap<FormId, Vec<String>>,
    /// Explicit script sequences for the renderer, independently of locomotion.
    /// The boolean selects the player's first-person rig (NPCs always use false).
    pub sequences: HashMap<(FormId, bool, String), ScriptSequence>,
    /// Per-sequence integrated clock state tracking pre-first-tick `-f32::MAX`
    /// (`PlayAnimationPath`) and frequency-scaled `GetAnimCurrentTime` deltas while
    /// `ScriptSequence::clock_offset` stays fixed for unscaled `lastTime`
    /// [observed: `w4e_08_knvse_legacy_anim_time`, #3803].
    pub(crate) sequence_clocks: HashMap<(FormId, bool, String), SequenceClock>,
    /// Paused time and weight, retained with the session's existing sequence clocks.
    pub sequence_resume: HashMap<(FormId, bool, String), (f64, f32)>,
    pub next_sequence: u32,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SequenceClock {
    pub initialized: bool,
    pub current_time: f64,
    pub last_clock: f64,
}

#[derive(Debug, Clone)]
pub struct ScriptSequence {
    pub path: String,
    pub first: bool,
    pub serial: u32,
    pub priority: i32,
    pub weight: f32,
    pub ease_in: Option<f32>,
    pub ease_out: f32,
    pub stopped: Option<f64>,
    pub auto_finish: bool,
    pub clock_offset: f64,
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

fn form(args: &[XVal], k: usize) -> Option<FormId> {
    match args.get(k)? {
        XVal::Ref(0) => None,
        XVal::Ref(r) => Some(FormId(*r)),
        XVal::Num(x) if *x >= 1.0 && x.fract() == 0.0 => Some(FormId(*x as u32)),
        _ => None,
    }
}

/// Two sequence paths name the same file: without regard to case [decision: NVForever's
/// paths do not keep the original's spelling, which the game compares exactly
/// (observed: "mtidle.kf" in lower case did not match the game's "Characters\_Male\
/// Locomotion\mtidle.kf")].
pub(crate) fn same(a: &str, b: &str) -> bool {
    a.replace('/', "\\").eq_ignore_ascii_case(&b.replace('/', "\\"))
}

/// Whether a path under meshes is a file of the game (loose or in an archive).
pub(crate) fn file_exists(path: &str) -> bool {
    let path = path.replace('/', "\\");
    if path.contains(':') || path.starts_with('\\') || path.split('\\').any(|p|p=="..") { return false; }
    let path = if path.to_ascii_lowercase().starts_with("meshes\\") { path } else { format!("meshes\\{path}") };
    crate::mod_files::exists(&path)
}

/// The key a path's condition is kept under: backslashes, lower case.
pub(crate) fn path_key(path: &str) -> String {
    path.replace('/', "\\").to_ascii_lowercase()
}

/// The KF files a path under meshes names: itself when it is a file, the `.kf` files
/// directly in it when it is a folder [documented: SetActorAnimationPath, "Path may also
/// be a folder containing KF files"], none otherwise.
pub fn kf_files(path: &str) -> Vec<String> {
    let path = path.replace('/', "\\").trim_end_matches('\\').to_string();
    if file_exists(&path) {
        return vec![path];
    }
    let mut files: Vec<String> = crate::mod_files::files_in(&format!("meshes\\{path}"))
        .into_iter()
        .filter(|f| f.to_ascii_lowercase().ends_with(".kf"))
        .map(|f| format!("{path}\\{}", f.rsplit('\\').next().unwrap_or(&f)))
        .collect();
    files.sort_by_key(|f| f.to_ascii_lowercase());
    files
}

impl Game<'_> {
    /// Integrate active script sequence clocks (honoring `SetAnimSequenceFrequency` on
    /// both `sLegacyAnimTimePaths` and normal paths while keeping `clock_offset` fixed),
    /// initialize freshly started `PlayAnimationPath` sequences on their first tick, and
    /// stop one-shot sequences that reached their end [observed: `w4e_08_knvse_legacy_anim_time`, #3803].
    pub(crate) fn script_sequences_tick(&mut self) {
        let updates: Vec<_> = self
            .anim_paths
            .sequences
            .iter()
            .map(|(k, seq)| {
                let clock = self.script_sequence_clock(k.0, k.1);
                let freq = f64::from(self.anim_frequency(k.0, &seq.path).unwrap_or(1.0));
                (k.clone(), clock, freq)
            })
            .collect();
        for (k, clock, freq) in updates {
            let sc = self.anim_paths.sequence_clocks.entry(k.clone()).or_insert(SequenceClock {
                initialized: true,
                current_time: 0.0,
                last_clock: clock,
            });
            if !sc.initialized {
                sc.initialized = true;
                sc.current_time = 0.0;
                sc.last_clock = clock;
                if let Some(seq) = self.anim_paths.sequences.get_mut(&k) {
                    seq.clock_offset = -clock;
                }
            } else {
                sc.current_time += (clock - sc.last_clock) * freq;
                sc.last_clock = clock;
            }
        }
        let mut stop = Vec::new();
        for (key, sequence) in &self.anim_paths.sequences {
            if sequence.auto_finish && sequence.stopped.is_none() {
                if let Some(info) = self.knvse.sequences.get(&key.2) {
                    let t = self.script_sequence_time_for(key.0, &sequence.path, key.1);
                    if t.is_finite() && info.cycle == 2 && t * info.frequency >= info.end - info.begin {
                        stop.push(key.clone());
                    }
                }
            }
        }
        for key in stop {
            let at = self.script_sequence_time_for(key.0, &key.2, key.1);
            let stopped = self.script_sequence_now(key.0, key.1);
            let weight = self.anim_paths.sequences.get(&key).map_or(1.0, |s|s.weight);
            self.anim_paths.sequence_resume.insert(key.clone(), (at,weight));
            if let Some(sequence) = self.anim_paths.sequences.get_mut(&key) {
                sequence.stopped = Some(stopped);
            }
            if let Some(paths) = self.anim_paths.played.get_mut(&key.0) {
                paths.retain(|p| !same(p, &key.2));
            }
        }
        let world_now = self.time;
        let first_now = self.script_sequence_now(FormId::PLAYER_REF, true);
        self.anim_paths.sequences.retain(|&(actor, first, _), sequence| {
            let now = if actor == FormId::PLAYER_REF && first { first_now } else { world_now };
            sequence.stopped.is_none_or(|at| now - at < f64::from(sequence.ease_out))
        });
        let sequences = &self.anim_paths.sequences;
        self.anim_paths.sequence_clocks.retain(|k, _| sequences.contains_key(k));
    }

    /// Flush active sequence clocks on `actor` using their current frequency before
    /// `SetAnimSequenceFrequency` changes the rate so earlier elapsed time is not
    /// retroactively rescaled [observed: `w4e_08_knvse_legacy_anim_time`, #3803].
    pub(crate) fn flush_script_sequence_clocks(&mut self, actor: FormId) {
        let updates: Vec<_> = self
            .anim_paths
            .sequences
            .iter()
            .filter(|((a, _, _), _)| *a == actor)
            .map(|(k, seq)| {
                let clock = self.script_sequence_clock(k.0, k.1);
                let freq = f64::from(self.anim_frequency(k.0, &seq.path).unwrap_or(1.0));
                (k.clone(), clock, freq)
            })
            .collect();
        for (k, clock, freq) in updates {
            if let Some(sc) = self.anim_paths.sequence_clocks.get_mut(&k) {
                if sc.initialized {
                    sc.current_time += (clock - sc.last_clock) * freq;
                    sc.last_clock = clock;
                }
            }
        }
    }

    /// Read the same established animation clock/offset as GetAnimCurrentTime,
    /// without changing it while the renderer samples the requested pose.
    pub fn script_sequence_time(&self, actor: FormId, path: &str) -> f64 {
        self.script_sequence_time_for(actor, path, actor == FormId::PLAYER_REF && self.senses.first_person)
    }

    pub(crate) fn script_sequence_clock(&self, actor: FormId, first: bool) -> f64 {
        let now = self.script_sequence_now(actor, first);
        let menu = if actor == FormId::PLAYER_REF && first {
            self.knvse.first_person_menu_seconds - self.knvse.first_person_menu_start.unwrap_or(self.knvse.first_person_menu_seconds)
        } else { 0.0 };
        self.knvse.global.get(&(actor, first)).map_or_else(
            || self.time - self.knvse.clock_start.get(&actor).copied().unwrap_or(self.time) + menu + self.cfg.frame_seconds,
            |&(value, at)| value + now - at)
    }

    /// Active sequence time for `GetAnimCurrentTime`: returns `-f32::MAX` (`-3.40282e+38`)
    /// on a freshly started `PlayAnimationPath` before its first tick, and thereafter
    /// integrates frequency-scaled deltas (`t + dg * freq`, honoring `SetAnimSequenceFrequency`)
    /// identically on both `kNVSE.ini` `[Anim Fixes] sLegacyAnimTimePaths` and normal paths
    /// while `ScriptSequence::clock_offset` stays fixed and `lastTime` tracks unscaled
    /// `clock + offset` [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + kNVSE 37,
    /// 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, isolated
    /// clean-room staging copy, `w4e_08_knvse_legacy_anim_time`
    /// (`tools/capture/nvse/catalogs/wave4e_mods.json`, #3803); `docs/spec-notes/ext-knvse.md` §5].
    pub fn script_sequence_time_for(&self, actor: FormId, path: &str, first: bool) -> f64 {
        let key = path.replace('/', "\\").to_ascii_lowercase();
        let seq_key = (actor, first, key.clone());
        if let Some(sequence) = self.anim_paths.sequences.get(&seq_key) {
            if let Some(sc) = self.anim_paths.sequence_clocks.get(&seq_key) {
                if !sc.initialized {
                    return f64::from(-f32::MAX);
                }
                let clock = self.script_sequence_clock(actor, first);
                let dg = clock - sc.last_clock;
                let freq = f64::from(self.anim_frequency(actor, &sequence.path).unwrap_or(1.0));
                sc.current_time + dg * freq
            } else {
                self.script_sequence_clock(actor, first) + sequence.clock_offset
            }
        } else {
            self.script_sequence_clock(actor, false) + self.knvse.offsets.get(&(actor, key)).copied().unwrap_or(0.0)
        }
    }

    /// The sequences an actor plays as far as this side knows: its body's clip, then
    /// what scripts played.
    pub(crate) fn playing_sequences(&self, a: FormId) -> Vec<String> {
        self.playing_sequences_for(a, a == FormId::PLAYER_REF && self.senses.first_person)
    }

    pub(crate) fn playing_sequences_for(&self, a: FormId, first: bool) -> Vec<String> {
        let current = a != FormId::PLAYER_REF || first == self.senses.first_person;
        let mut v: Vec<String> = self.senses.anim.get(&a).filter(|_|current).and_then(|s| s.clip.clone()).into_iter().collect();
        let mut active:Vec<_> = self.anim_paths.sequences.iter().filter(|((actor,view,_),s)|*actor==a && *view==first && s.stopped.is_none()).map(|(_,s)|s).collect();
        active.sort_by_key(|s|s.serial);
        v.extend(active.into_iter().map(|s|s.path.clone()));
        v
    }
}

/// Deactivate `path` on `(a, first)` with explicit ease-out duration `ease`.
pub(crate) fn deactivate_anim_for(g: &mut Game, a: FormId, first: bool, path: &str, ease: f64) -> bool {
    if !ease.is_finite() || ease < 0.0 || ease > f64::from(f32::MAX) {
        return false;
    }
    let norm = path.replace('/', "\\").to_ascii_lowercase();
    let key = (a, first, norm.clone());
    let raw_resume = g.script_sequence_time_for(a, path, first);
    let resume = if g.anim_paths.sequence_clocks.get(&key).is_some_and(|sc| !sc.initialized) {
        0.0
    } else {
        raw_resume
    };
    let stopped = g.script_sequence_now(a, first);
    let weight = g.anim_paths.sequences.get(&key).map_or(1.0, |s| s.weight);
    g.anim_paths.sequence_resume.insert(key.clone(), (resume, weight));
    let mut changed = false;
    if let Some(sequence) = g.anim_paths.sequences.get_mut(&key).filter(|s| s.stopped.is_none()) {
        sequence.ease_out = ease as f32;
        sequence.stopped = Some(stopped);
        changed = true;
    }
    let other_active = a == FormId::PLAYER_REF
        && g.anim_paths
            .sequences
            .get(&(a, !first, norm))
            .is_some_and(|s| s.stopped.is_none());
    if !other_active {
        if let Some(p) = g.anim_paths.played.get_mut(&a) {
            p.retain(|x| !same(x, path));
        }
    }
    changed
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let zero = XVal::Num(0.0);
    let list = |g: &mut Game, items: Vec<XVal>| {
        let entries = items.into_iter().enumerate().map(|(i, v)| (XKey::Num(i as f64), v)).collect();
        XVal::Array(g.ext.new_array(ArrayKind::List, entries))
    };
    let a = subject.unwrap_or(FormId::PLAYER_REF);
    let v = match name {
        // `firstPerson enable path …`: 1 when the file is there (or when disabling)
        // [observed: a missing file gives 0, disabling 1].
        "setactoranimationpath" | "setweaponanimationpath" => {
            let weapon = name == "setweaponanimationpath";
            let at = usize::from(weapon);
            let owner = if weapon { form(args, 0).unwrap_or(FormId(0)) } else { a };
            let first = num(args, at, 0.0) != 0.0;
            let enable = num(args, at + 1, 1.0) != 0.0;
            let path = text(args, at + 2);
            // `poll condition` (actors only): the condition is kept with the path.
            let cond = if weapon { None } else { args.get(at + 4).filter(|c| !matches!(c, XVal::Num(_))).cloned() };
            let key = (owner, first, path_key(&path));
            let map = if weapon { &mut g.anim_paths.weapon } else { &mut g.anim_paths.actor };
            let paths = map.entry((owner, first)).or_default();
            if !enable {
                paths.retain(|p| !same(p, &path));
                g.anim_paths.conditions.remove(&key);
                return Some(Ok(XVal::Num(1.0)));
            }
            if kf_files(&path).is_empty() {
                return Some(Ok(zero));
            }
            // Enabling again puts it on top of the stack [documented: "the previous
            // override will be restored once the other mod disables" its own].
            paths.retain(|p| !same(p, &path));
            paths.push(path);
            match cond {
                Some(c) => g.anim_paths.conditions.insert(key, c),
                None => g.anim_paths.conditions.remove(&key),
            };
            XVal::Num(1.0)
        }
        "playanimationpath" | "activateanim" => {
            let path = text(args, 0);
            if file_exists(&path) {
                let first = a == FormId::PLAYER_REF && num(args, 1, f64::from(u8::from(g.senses.first_person))) != 0.0;
                let key = (a, first, path.replace('/', "\\").to_ascii_lowercase());
                // ActivateAnim requires an inactive sequence. Time synchronization
                // needs matching morph keys and is not inferred from a path alone.
                if (name == "activateanim" && g.anim_paths.sequences.get(&key).is_some_and(|s| s.stopped.is_none())) || !text(args, 6).is_empty() {
                    return Some(Ok(zero));
                }
                let priority = num(args, 2, 1.0);
                let weight = num(args, 4, g.anim_paths.sequence_resume.get(&key).map_or(1.0, |s|f64::from(s.1)));
                let ease = args.get(5).map(|_| num(args, 5, 0.0));
                if !priority.is_finite() || !weight.is_finite() || ease.is_some_and(|v| !v.is_finite() || v < 0.0 || v > f64::from(f32::MAX)) {
                    return Some(Ok(zero));
                }
                let Some(serial) = g.anim_paths.next_sequence.checked_add(1).filter(|n| *n < 0x4000_0000) else { return Some(Ok(zero)); };
                g.anim_paths.next_sequence = serial;
                let resume = if name != "playanimationpath" && num(args, 3, 1.0) == 0.0 {
                    g.anim_paths.sequence_resume.get(&key).map_or(0.0, |s|s.0)
                } else { 0.0 };
                g.knvse_started(a, &path);
                if let Some(offset) = g.knvse.offsets.get_mut(&(a, key.2.clone())) { *offset += resume; }
                g.knvse.weights.insert((a,key.2.clone()), weight.clamp(0.0,1.0));
                let start_clock = g.script_sequence_clock(a, first);
                let clock_offset = resume - start_clock;
                g.anim_paths.sequence_clocks.insert(
                    key.clone(),
                    SequenceClock {
                        initialized: name != "playanimationpath",
                        current_time: resume,
                        last_clock: start_clock,
                    },
                );
                g.anim_paths.sequences.insert(key, ScriptSequence {
                    path:path.clone(), first, serial, priority:priority as i32,
                    weight:weight.clamp(0.0, 1.0) as f32, ease_in:ease.map(|v| v as f32),
                    ease_out:0.2, stopped:None, auto_finish:name == "playanimationpath", clock_offset,
                });
                let p = g.anim_paths.played.entry(a).or_default();
                if !p.iter().any(|x| same(x, &path)) {
                    p.push(path.clone());
                }
                return Some(Ok(XVal::Num(1.0)));
            }
            zero
        }
        "deactivateanim" => {
            let path = text(args, 0);
            let current_first = a == FormId::PLAYER_REF && g.senses.first_person;
            let norm = path.replace('/', "\\").to_ascii_lowercase();
            let first = if g.anim_paths.sequences.get(&(a, current_first, norm.clone())).is_some_and(|s| s.stopped.is_none()) {
                current_first
            } else if a == FormId::PLAYER_REF && g.anim_paths.sequences.get(&(a, !current_first, norm)).is_some_and(|s| s.stopped.is_none()) {
                !current_first
            } else {
                current_first
            };
            let ease = num(args, 1, 0.2);
            let changed = deactivate_anim_for(g, a, first, &path, ease);
            XVal::Num(f64::from(u8::from(changed)))
        }
        "isanimsequenceplaying" => {
            let path = text(args, 0);
            XVal::Num(f64::from(u8::from(g.playing_sequences(a).iter().any(|p| same(p, &path)))))
        }
        // Eight slots, the body's clip first [observed: an array of 8, [0] filled].
        "getplayinganimsequencepaths" => {
            let mut v: Vec<XVal> = g.senses.anim.get(&a).and_then(|s| s.clip.clone()).into_iter().map(XVal::Str).collect();
            v.resize(8, XVal::Str(String::new()));
            list(g, v)
        }
        "getactiveanims" => {
            let first = a == FormId::PLAYER_REF && num(args,0,f64::from(u8::from(g.senses.first_person))) != 0.0;
            let v = g.playing_sequences_for(a,first).into_iter().map(XVal::Str).collect();
            list(g, v)
        }
        // [observed: Idle reads 1 on a standing actor]
        "isanimplayingalt" => {
            let want = text(args, 0);
            XVal::Num(f64::from(u8::from(g.senses.anim.get(&a).and_then(|s| s.group).is_some_and(|gr| gr.eq_ignore_ascii_case(&want)))))
        }
        "getanimationsbyanimgroup" => {
            let want = text(args, 0);
            let st = g.senses.anim.get(&a);
            let v = st.filter(|s| s.group.is_some_and(|gr| gr.eq_ignore_ascii_case(&want))).and_then(|s| s.clip.clone()).into_iter().map(XVal::Str).collect();
            list(g, v)
        }
        _ => return None,
    };
    Some(Ok(v))
}
