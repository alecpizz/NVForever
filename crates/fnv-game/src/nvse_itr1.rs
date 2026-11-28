//! ITR NVSE, batch 1 (#1011): actor switches (fall damage, crouching, weapon search and
//! switching, contact watch, alert), the camera and dialogue camera settings, keys, race
//! eyes and eye meshes, hair colour, creature combat skill, casino bans, challenges,
//! worldspace map offsets, dialogue info flags, the radio and its injected queue, UI tile
//! watches and textures, item mod flags and the weapon's emissive colour [documented: each
//! page in the wiki export; docs/spec-notes/ext-itr.md]. ITR is not installed in the probe
//! game, so nothing here is observed. State lasts for the session; what a page says
//! survives a load is carried across it (`ItrKept`, `Game::take_session`).

use crate::game::{Game, RunCtx};
use fnv_script::vm::XVal;
use fnv_sim::FormId;
use fnv_ui::TileId;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

/// What lasts across a load: the plugin keeps it "in static memory" until the game is
/// restarted [documented: SetFallDamageMult, SetNoWeaponSearch, SetPreventWeaponSwitch].
#[derive(Debug, Clone, Default)]
pub struct ItrKept {
    /// SetFallDamageMult's global multiplier (`None`: 1) and per-actor overrides.
    pub fall_global: Option<f64>,
    pub fall_actors: BTreeMap<FormId, f64>,
    pub no_weapon_search: BTreeSet<FormId>,
    pub prevent_weapon_switch: BTreeSet<FormId>,
}

/// The dialogue camera's settings (SetDialogueCamera…). Read by the engine's dialogue
/// camera when it has one (ext-itr.md §3, follow-up).
#[derive(Debug, Clone, PartialEq)]
pub struct DialogueCamera {
    /// `None`: never set, the INI's `bDialogueCamera` [documented: SetDialogueCameraEnabled];
    /// NVForever reads no ITR INI, so off [decision].
    pub enabled: Option<bool>,
    /// 0 cycle, 1 fixed, 2 random, 3 manual [documented: SetDialogueCameraMode].
    pub mode: u8,
    pub fixed_angle: u8,
    /// The angle SetDialogueCameraAngle switched to, for the engine to take.
    pub angle: Option<u8>,
    /// Angles 0-11 allowed in the cycle and random choice [documented:
    /// SetDialogueCameraAngleEnabled]; all at the start [hypothesis].
    pub allowed: [bool; 12],
    /// Dolly speed (units a second), the most it moves, once a conversation.
    pub dolly: (f64, f64, bool),
    /// Shake amplitude; 3 by default [documented: SetDialogueCameraShake, "-1 to restore
    /// the default amplitude (3.0)"].
    pub shake: f64,
}

impl Default for DialogueCamera {
    fn default() -> Self {
        DialogueCamera { enabled: None, mode: 0, fixed_angle: 0, angle: None, allowed: [true; 12], dolly: (0.0, 0.0, true), shake: 3.0 }
    }
}

/// A watched tile trait (WatchTileValue): its id, the trait's path and the value last seen.
#[derive(Debug, Clone)]
pub struct TileWatch {
    pub id: u32,
    pub path: String,
    /// `None` while the trait is not there (its menu still is) [observed: itr probes].
    pub last: Option<TileValue>,
}

/// A trait's value as a watch compares it: a number, or text.
#[derive(Debug, Clone, PartialEq)]
pub enum TileValue {
    Num(f64),
    Str(String),
}

impl From<&fnv_ui::Value> for TileValue {
    fn from(v: &fnv_ui::Value) -> TileValue {
        match v {
            fnv_ui::Value::Num(x) => TileValue::Num(*x),
            other => TileValue::Str(other.text()),
        }
    }
}

/// What the radio plays now, as far as the game knows: a station's line
/// (`Game::radio_line_begins`), or what the engine reports (`Game::set_radio_playing`).
#[derive(Debug, Clone, PartialEq)]
pub struct RadioPlaying {
    /// The song's sound form or the line's topic info.
    pub track: Option<FormId>,
    /// The file the engine plays ("" when not told).
    pub file: String,
    pub station: Option<FormId>,
}

/// ITR's session state (cleared by a load: a new game state).
#[derive(Debug, Clone, Default)]
pub struct ItrState {
    pub kept: ItrKept,
    /// DisableCrouching's actors.
    pub no_crouch: BTreeSet<FormId>,
    /// ForceCrouch's crouch (true) or stand (false), over DisableCrouching.
    pub crouch: BTreeMap<FormId, bool>,
    /// SetOnContactWatch's references.
    pub contact_watch: BTreeSet<FormId>,
    /// SetAlertNS's alerts: the game time it ends, `None` for no timer.
    pub alerts: BTreeMap<FormId, Option<f64>>,
    /// SetAimZoomFirstPersonOnly; `None` never set (the INI's value: off, NVForever reads
    /// no ITR INI [decision]).
    pub aim_zoom_first_person_only: Option<bool>,
    /// SetCameraAngle's override: X, Y, Z in degrees.
    pub camera_angle: Option<[f64; 3]>,
    pub dialogue_camera: DialogueCamera,
    /// DisableKeyEx's mask groups by key: a key stays disabled while one is left.
    pub key_masks: BTreeMap<u16, BTreeSet<i64>>,
    /// SetEyeMesh's meshes by eyes form: left, right.
    pub eye_meshes: HashMap<FormId, (String, String)>,
    /// The injected radio queue (paths under Data\Sound\), at most 8, and its loop switch.
    pub radio_queue: VecDeque<String>,
    pub radio_loop: bool,
    /// PlayRadioFile's file, for the engine to start; ChangeRadioTrack's skips.
    pub radio_play_now: Option<String>,
    pub radio_skips: u32,
    pub radio_playing: Option<RadioPlaying>,
    pub tile_watches: Vec<TileWatch>,
    pub next_watch: u32,
    /// SetUIAlphaMap's textures (as the script names them) and SetUITexOffset's offsets
    /// (wrapped into 0-1), by tile, for the engine's tile renderer (`nvf-play`'s
    /// `tiles::texmod`). Kept by the tile's id, not its path, so they follow the tile: one
    /// unloaded, or replaced by another of the same path, drops them
    /// ([`Game::itr_drop_gone_tile_textures`]) [design: #1833].
    pub alpha_maps: BTreeMap<TileId, String>,
    pub tex_offsets: BTreeMap<TileId, (f64, f64)>,
    /// SetWeaponEmissiveColor's red, green, blue and multiplier while it holds.
    pub emissive: Option<[f64; 4]>,
}

/// The injected queue's size [documented: QueueRadioTrack, "up to 8 tracks"].
const RADIO_QUEUE: usize = 8;
/// WatchTileValue's limit [documented: "Up to 48 tile values"].
const TILE_WATCHES: usize = 48;
/// SetAlertNS's longest timer [documented: "limited to 86400 seconds"].
const ALERT_MAX: f64 = 86_400.0;

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

/// The menu a tile path starts with.
fn menu_of(path: &str) -> &str {
    path.split(['/', '\\']).next().unwrap_or_default()
}

/// An offset wrapped into 0-1 [documented: SetUITexOffset]: `rem_euclid` of a tiny
/// negative number rounds up to 1 itself, which wraps to 0 [design].
pub fn wrap(x: f64) -> f64 {
    let w = x.rem_euclid(1.0);
    if w >= 1.0 {
        0.0
    } else {
        w
    }
}

/// The Data path of an alpha map a script named (SetUIAlphaMap): under `textures\`
/// [documented: SetUIAlphaMap, "relative to Data\Textures\"], `/` read as `\`, in small
/// letters, a leading separator dropped. A path that already starts with `textures\` is
/// kept as it is [decision: it would otherwise name `textures\textures\…`, which no mod
/// ships; how the original treats such a path is not documented]. The script's check
/// (`mod_files::exists`) and the engine's renderer (`tiles::texmod`) both read it so.
pub fn alpha_map_path(name: &str) -> String {
    let p = name.trim().trim_start_matches(['\\', '/']).replace('/', "\\").to_ascii_lowercase();
    if p.starts_with("textures\\") {
        p
    } else {
        format!("textures\\{p}")
    }
}

fn yes(b: bool) -> XVal {
    XVal::Num(f64::from(u8::from(b)))
}

/// A path the radio functions take: relative, ending in .wav or .ogg [documented:
/// PlayRadioFile, QueueRadioTrack "Only relative .wav and .ogg paths are accepted"],
/// or .mp3 [observed: itr probes, QueueRadioTrack "nvf\a.mp3" returns 1]; an absolute
/// path is refused [observed].
fn radio_path_ok(p: &str) -> bool {
    let l = p.trim().to_ascii_lowercase();
    !l.is_empty() && (l.ends_with(".wav") || l.ends_with(".ogg") || l.ends_with(".mp3")) && !l.starts_with(['\\', '/']) && !l.contains(':') && !l.split(['\\', '/']).any(|s| s == "..")
}

impl Game<'_> {
    /// Whether `r` is a reference to an actor (its base an NPC or a creature).
    pub(crate) fn itr_is_actor(&self, r: FormId) -> bool {
        let b = crate::functions::base_of(self, r).unwrap_or(r);
        r == FormId::PLAYER_REF || (r != b && matches!(self.data.sig(b).as_ref(), Some(b"NPC_" | b"CREA")))
    }

    /// The fall damage multiplier of an actor: its override, else the global one, 1 when
    /// neither was set [documented: GetFallDamageMult]. The engine's falls apply it.
    pub fn fall_damage_mult(&self, actor: FormId) -> f64 {
        let k = &self.other.itr.kept;
        k.fall_actors.get(&actor).copied().or(k.fall_global).unwrap_or(1.0)
    }

    /// Crouching as ITR rules it: `Some(true/false)` ForceCrouch's word, `Some(false)`
    /// DisableCrouching, `None` the game's own rule ([`Game::sneaks`]).
    pub(crate) fn itr_crouch(&self, actor: FormId) -> Option<bool> {
        let s = &self.other.itr;
        s.crouch.get(&actor).copied().or_else(|| s.no_crouch.contains(&actor).then_some(false))
    }

    /// Whether SetAlertNS holds the actor alert.
    pub(crate) fn itr_alert(&self, actor: FormId) -> bool {
        self.other.itr.alerts.contains_key(&actor)
    }

    /// A topic info's DATA flags as the dialogue reads them: a script's change first
    /// (`crate::dialogue_edits`).
    pub fn info_flags(&self, info: FormId) -> Option<u16> {
        self.dialogue_view().info(&info).map(|d| d.flags)
    }

    /// The engine tells what the radio plays (a song's sound or a line's info and its
    /// file), `None` when it stops.
    pub fn set_radio_playing(&mut self, p: Option<RadioPlaying>) {
        self.other.itr.radio_playing = p;
    }

    /// A station's line starts or ends (`crate::radio`): what GetPlayingRadioTrack reads
    /// while the engine has said nothing else.
    pub(crate) fn itr_radio_line(&mut self, station: FormId, info: FormId, begins: bool) {
        let s = &mut self.other.itr;
        if begins {
            s.radio_playing = Some(RadioPlaying { track: Some(info), file: String::new(), station: Some(station) });
        } else if s.radio_playing.as_ref().is_some_and(|p| p.track == Some(info)) {
            s.radio_playing = None;
        }
    }

    /// The alpha maps and texture offsets of tiles no longer loaded: an unloaded tile's
    /// id names no tile again, nor does it name a later tile in its slot or at its path
    /// (`fnv_ui::TileId`) [design: #1833].
    pub fn itr_drop_gone_tile_textures(&mut self) {
        let s = &mut self.other.itr;
        if s.alpha_maps.is_empty() && s.tex_offsets.is_empty() {
            return;
        }
        let ui = &self.ui_tree;
        s.alpha_maps.retain(|t, _| ui.is_live(*t));
        s.tex_offsets.retain(|t, _| ui.is_live(*t));
    }

    /// Once a frame: SetAlertNS timers that ran out (kept while the actor still fights
    /// [documented: SetAlertNS]) and the watched tile traits that changed, each firing
    /// ITR:OnTileValueChange (numbers) or ITR:OnTileStrValueChange (text) with the watch
    /// id, the path and the new value (see docs/spec-notes/ext-itr.md §6).
    pub(crate) fn itr_tick(&mut self) {
        self.itr_drop_gone_tile_textures();
        if !self.other.itr.alerts.is_empty() {
            let now = self.time;
            let done: Vec<FormId> = self.other.itr.alerts.iter().filter(|(a, end)| end.is_some_and(|e| now >= e) && !self.awareness.state(**a).in_combat()).map(|(a, _)| *a).collect();
            for a in done {
                self.other.itr.alerts.remove(&a);
            }
        }
        if self.other.itr.tile_watches.is_empty() {
            return;
        }
        let mut fired = Vec::new();
        let mut gone = Vec::new();
        for w in &mut self.other.itr.tile_watches {
            match self.ui_tree.get(&w.path).map(TileValue::from) {
                // [documented: WatchTileValue, "Watches are removed when their menu closes"].
                None if self.ui_tree.menu(menu_of(&w.path)).is_none() => gone.push(w.id),
                None => w.last = None,
                Some(v) => {
                    if w.last.as_ref() != Some(&v) {
                        fired.push((w.id, w.path.clone(), v.clone()));
                    }
                    w.last = Some(v);
                }
            }
        }
        self.other.itr.tile_watches.retain(|w| !gone.contains(&w.id));
        for (id, path, v) in fired {
            let (event, value) = match v {
                TileValue::Num(x) => ("ITR:OnTileValueChange", XVal::Num(x)),
                TileValue::Str(s) => ("ITR:OnTileStrValueChange", XVal::Str(s)),
            };
            self.dispatch_event(event, vec![XVal::Num(f64::from(id)), XVal::Str(path), value], None);
        }
    }
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, _ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let base = |g: &Game, f: FormId| crate::functions::base_of(g, f).unwrap_or(f);
    let field = |g: &Game, f: FormId, sub: &str, path: &str| crate::nvse::record_value(g, f, sub, path).and_then(|v| v.num().ok());
    let one = XVal::Num(1.0);
    let zero = XVal::Num(0.0);
    let actor = subject.filter(|&a| g.itr_is_actor(a));
    let v = match name {
        // ---- fall damage ----
        // `[actor]`: the actor's override, else the global multiplier, 1 when unset
        // [documented: GetFallDamageMult].
        "getfalldamagemult" => XVal::Num(match form(args, 0).or(subject) {
            Some(a) => g.fall_damage_mult(a),
            None => g.other.itr.kept.fall_global.unwrap_or(1.0),
        }),
        // `mult [actor]`: below 0 taken as 0; an actor's 1 clears its override; with no
        // actor the global one [documented: SetFallDamageMult]. A value that is not a
        // finite number changes nothing and gives 0, and one past f32's range is kept at
        // its largest, so the engine's fall damage stays a number [decision].
        "setfalldamagemult" => {
            let m = num(args, 0, 1.0);
            if !m.is_finite() {
                return Some(Ok(zero));
            }
            let m = m.clamp(0.0, f64::from(f32::MAX));
            let k = &mut g.other.itr.kept;
            match form(args, 1).or(subject) {
                Some(a) if m == 1.0 => {
                    k.fall_actors.remove(&a);
                }
                Some(a) => {
                    k.fall_actors.insert(a, m);
                }
                None => k.fall_global = Some(m),
            }
            one
        }
        // `[actor]`: the actor's override; with no actor at all every override and the
        // global multiplier [documented: ClearFallDamageMult].
        "clearfalldamagemult" => {
            let k = &mut g.other.itr.kept;
            match form(args, 0).or(subject) {
                Some(a) => {
                    k.fall_actors.remove(&a);
                }
                None => {
                    k.fall_actors.clear();
                    k.fall_global = None;
                }
            }
            one
        }
        // ---- crouching, weapon search and switching, contacts ----
        // `disable` on an actor; ForceCrouch `crouch` over it [documented: DisableCrouching;
        // ForceCrouch, "bypasses DisableCrouching"]. Both read by `Game::sneaks`. 0 for a
        // reference that is not an actor [documented: "Only works on actors"].
        "disablecrouching" | "forcecrouch" => {
            let Some(a) = actor else { return Some(Ok(zero)) };
            let on = num(args, 0, 0.0) != 0.0;
            let s = &mut g.other.itr;
            if name == "disablecrouching" {
                if on {
                    s.no_crouch.insert(a);
                } else {
                    s.no_crouch.remove(&a);
                }
                // A forced stance gives way to the new rule (see docs/spec-notes/ext-itr.md §2).
                s.crouch.remove(&a);
            } else {
                s.crouch.insert(a, on);
            }
            one
        }
        // [documented: SetNoWeaponSearch; GetNoWeaponSearch; SetPreventWeaponSwitch;
        // GetPreventWeaponSwitch]: kept across loads, read by the combat AI when it has
        // those choices (ext-itr.md §2, follow-up).
        "setnoweaponsearch" | "setpreventweaponswitch" => {
            let Some(a) = actor else { return Some(Ok(zero)) };
            let k = &mut g.other.itr.kept;
            let set = if name == "setnoweaponsearch" { &mut k.no_weapon_search } else { &mut k.prevent_weapon_switch };
            if num(args, 0, 0.0) != 0.0 {
                set.insert(a);
            } else {
                set.remove(&a);
            }
            one
        }
        "getnoweaponsearch" => yes(actor.is_some_and(|a| g.other.itr.kept.no_weapon_search.contains(&a))),
        "getpreventweaponswitch" => yes(actor.is_some_and(|a| g.other.itr.kept.prevent_weapon_switch.contains(&a))),
        // [documented: SetOnContactWatch; GetOnContactWatch]: the contact events are the
        // engine's physics' (follow-up); the switch is kept.
        "setoncontactwatch" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            if num(args, 0, 0.0) != 0.0 {
                g.other.itr.contact_watch.insert(r);
            } else {
                g.other.itr.contact_watch.remove(&r);
            }
            one
        }
        "getoncontactwatch" => yes(subject.is_some_and(|r| g.other.itr.contact_watch.contains(&r))),
        // ---- alert ----
        // `toggle [seconds]`: alert, for the seconds when above 0 (at most 86400), until
        // cleared otherwise; 0 clears it and its timer; 0 for a non-actor [documented:
        // SetAlertNS]. GetIsAlerted reads it (`Game::is_alerted`).
        "setalertns" => {
            let Some(a) = actor else { return Some(Ok(zero)) };
            if num(args, 0, 0.0) != 0.0 {
                let secs = num(args, 1, 0.0);
                let end = (secs > 0.0).then(|| g.time + secs.min(ALERT_MAX));
                g.other.itr.alerts.insert(a, end);
            } else {
                g.other.itr.alerts.remove(&a);
            }
            one
        }
        // The seconds left, -1 with no timer, 0 untracked [documented: GetAlertNS]; a
        // timer run out while fighting reads 0 [hypothesis].
        "getalertns" => XVal::Num(match actor.and_then(|a| g.other.itr.alerts.get(&a).copied()) {
            None => 0.0,
            Some(None) => -1.0,
            Some(Some(end)) => (end - g.time).max(0.0),
        }),
        // ---- camera ----
        // [documented: SetAimZoomFirstPersonOnly, "Returns the new setting"; GetAimZoom…].
        "setaimzoomfirstpersononly" => {
            let on = num(args, 0, 0.0) != 0.0;
            g.other.itr.aim_zoom_first_person_only = Some(on);
            yes(on)
        }
        "getaimzoomfirstpersononly" => yes(g.other.itr.aim_zoom_first_person_only.unwrap_or(false)),
        // `enable x y z` in degrees; 0 releases it [documented: SetCameraAngle]. The
        // page's return type is None: 0 (see docs/spec-notes/ext-itr.md §3). An angle that is not a finite
        // number changes nothing [decision].
        "setcameraangle" => {
            let angle = [num(args, 1, 0.0), num(args, 2, 0.0), num(args, 3, 0.0)];
            if angle.iter().all(|a| a.is_finite()) {
                g.other.itr.camera_angle = (num(args, 0, 0.0) != 0.0).then_some(angle);
            }
            zero
        }
        // ---- the dialogue camera ----
        "setdialoguecameraenabled" => {
            g.other.itr.dialogue_camera.enabled = Some(num(args, 0, 0.0) != 0.0);
            one
        }
        // `mode` 0-3; 0 for another [documented: SetDialogueCameraMode].
        "setdialoguecameramode" => match u8::try_from(num(args, 0, -1.0) as i64).ok().filter(|m| *m <= 3) {
            Some(m) => {
                g.other.itr.dialogue_camera.mode = m;
                one
            }
            None => zero,
        },
        // `angle` 0-11: returns the fixed angle now, 0 out of range [documented:
        // SetDialogueCameraFixedAngle].
        "setdialoguecamerafixedangle" => match u8::try_from(num(args, 0, -1.0) as i64).ok().filter(|a| *a <= 11) {
            Some(a) => {
                g.other.itr.dialogue_camera.fixed_angle = a;
                XVal::Num(f64::from(a))
            }
            None => zero,
        },
        // `angle`: 0-11, or -1 for one at random among the allowed angles; returns the
        // angle chosen, 0 out of range [documented: SetDialogueCameraAngle]; with no angle
        // allowed, any [hypothesis].
        "setdialoguecameraangle" => {
            let a = num(args, 0, -2.0) as i64;
            let chosen = match a {
                -1 => {
                    let allowed: Vec<u8> = (0..12u8).filter(|&i| g.other.itr.dialogue_camera.allowed[usize::from(i)]).collect();
                    let pool: Vec<u8> = if allowed.is_empty() { (0..12).collect() } else { allowed };
                    let r = rand_core::RngCore::next_u32(&mut g.rng) as usize;
                    Some(pool[r % pool.len()])
                }
                0..=11 => Some(a as u8),
                _ => None,
            };
            match chosen {
                Some(c) => {
                    g.other.itr.dialogue_camera.angle = Some(c);
                    XVal::Num(f64::from(c))
                }
                None => zero,
            }
        }
        // `angle enable` [documented: SetDialogueCameraAngleEnabled, "Allow or block an
        // angle (0-11)"]; 1 done, 0 out of range (see docs/spec-notes/ext-itr.md §3).
        "setdialoguecameraangleenabled" => match usize::try_from(num(args, 0, -1.0) as i64).ok().filter(|a| *a <= 11) {
            Some(a) => {
                g.other.itr.dialogue_camera.allowed[a] = num(args, 1, 0.0) != 0.0;
                one
            }
            None => zero,
        },
        // `speed maxDist [runOnce]`, runOnce 1 by default [documented: SetDialogueCameraDolly].
        // A speed or distance that is not a finite number is refused: 0 [decision].
        "setdialoguecameradolly" => {
            let (speed, dist) = (num(args, 0, 0.0), num(args, 1, 0.0));
            if !speed.is_finite() || !dist.is_finite() {
                return Some(Ok(zero));
            }
            g.other.itr.dialogue_camera.dolly = (speed.max(0.0), dist.max(0.0), num(args, 2, 1.0) != 0.0);
            one
        }
        // `amplitude`: 0 off, -1 the default 3 [documented: SetDialogueCameraShake]; below 0
        // otherwise taken as 0 [hypothesis]; one that is not a finite number refused, 0
        // [decision].
        "setdialoguecamerashake" => {
            let a = num(args, 0, 0.0);
            if !a.is_finite() {
                return Some(Ok(zero));
            }
            g.other.itr.dialogue_camera.shake = if a == -1.0 { 3.0 } else { a.max(0.0) };
            one
        }
        // ---- keys ----
        // `key [mask]`: DisableKey, kept under the mask group (0 when none), and
        // ITR:OnKeyDisabled with the key and mask [documented: DisableKeyEx] (event params:
        // see docs/spec-notes/ext-itr.md §3). Any code is taken [observed: itr probes,
        // DisableKeyEx 300 returns 1]; 0 only for a code outside 1-65535 [hypothesis].
        "disablekeyex" | "enablekeyex" => {
            let code = num(args, 0, -1.0) as i64;
            let Some(k) = u16::try_from(code).ok().filter(|&k| k > 0) else { return Some(Ok(zero)) };
            let mask = num(args, 1, 0.0) as i64;
            let masks = g.other.itr.key_masks.entry(k).or_default();
            if name == "disablekeyex" {
                masks.insert(mask);
                g.input.disabled_keys.insert(k);
            } else {
                // No mask releases the key; a mask releases that group's hold, the key
                // coming free once no group holds it (see docs/spec-notes/ext-itr.md §3).
                if args.len() < 2 {
                    masks.clear();
                } else {
                    masks.remove(&mask);
                }
                if masks.is_empty() {
                    g.other.itr.key_masks.remove(&k);
                    g.input.disabled_keys.remove(&k);
                }
            }
            let event = if name == "disablekeyex" { "ITR:OnKeyDisabled" } else { "ITR:OnKeyEnabled" };
            g.dispatch_event(event, vec![XVal::Num(f64::from(k)), XVal::Num(mask as f64)], None);
            one
        }
        // ---- race eyes and eye meshes ----
        // `race eyes`: the eyes join the race's list (GetRaceEyes, the race menu) if they
        // are playable [documented: AddRaceEye]; 1 when added (see docs/spec-notes/ext-itr.md §3).
        // RemoveRaceEye takes them off, ClearRaceEyes empties the list.
        // The list is the generic list 1 (`Game::generic_list`).
        "addraceeye" | "removeraceeye" | "clearraceeyes" => {
            let Some(race) = form(args, 0).filter(|&r| g.data.sig(r) == Some(*b"RACE")) else { return Some(Ok(zero)) };
            let mut list = g.generic_list(1, race).unwrap_or_default();
            let eyes = form(args, 1).filter(|&e| g.data.sig(e) == Some(*b"EYES"));
            let done = match (name, eyes) {
                ("clearraceeyes", _) => {
                    list.clear();
                    true
                }
                // EYES DATA bit 0 Playable [documented: xEdit EYES].
                ("addraceeye", Some(e)) if field(g, e, "DATA", "").is_some_and(|f| f as u32 & 1 != 0) && !list.contains(&e) => {
                    list.push(e);
                    true
                }
                ("removeraceeye", Some(e)) if list.contains(&e) => {
                    list.retain(|&x| x != e);
                    true
                }
                _ => false,
            };
            if done {
                g.other.generic.insert((1, race), list);
            }
            yes(done)
        }
        // `eyes left [right]`: right is left when not given; paths under Meshes\ for the
        // engine's head model on its next Update3D (follow-up) [documented: SetEyeMesh;
        // ClearEyeMesh]; 1 for an eyes form (see docs/spec-notes/ext-itr.md §3).
        "seteyemesh" => {
            let Some(e) = form(args, 0).filter(|&e| g.data.sig(e) == Some(*b"EYES")) else { return Some(Ok(zero)) };
            let left = text(args, 1);
            let right = Some(text(args, 2)).filter(|r| !r.is_empty()).unwrap_or_else(|| left.clone());
            g.other.itr.eye_meshes.insert(e, (left, right));
            one
        }
        // ClearEyeMesh: 1 for an eyes form, set or not [observed: itr probes, a second
        // ClearEyeMesh returns 1 too].
        "cleareyemesh" => match form(args, 0).filter(|&e| g.data.sig(e) == Some(*b"EYES")) {
            Some(e) => {
                g.other.itr.eye_meshes.remove(&e);
                one
            }
            None => zero,
        },
        // ---- hair colour ----
        // `[channel] [npc]` on the NPC base: 1 red, 2 green, 3 blue, else the packed
        // red + green·256 + blue·65536 SetHairColor takes; 0 for no NPC [documented:
        // GetHairColorAlt; the packing: GetHairColor, observed nvse-actors].
        "gethaircoloralt" => {
            let Some(npc) = form(args, 1).or(subject).map(|f| base(g, f)).filter(|&n| g.data.sig(n) == Some(*b"NPC_")) else { return Some(Ok(zero)) };
            let c = |k: &str| field(g, npc, "HCLR", k).unwrap_or(0.0);
            XVal::Num(match num(args, 0, 0.0) as i64 {
                1 => c("red"),
                2 => c("green"),
                3 => c("blue"),
                _ => c("red") + c("green") * 256.0 + c("blue") * 65536.0,
            })
        }
        // `red green blue [npc]`, each kept to its low 8 bits [documented: SetHairColorAlt].
        "sethaircoloralt" => {
            let Some(npc) = form(args, 3).or(subject).map(|f| base(g, f)).filter(|&n| g.data.sig(n) == Some(*b"NPC_")) else { return Some(Ok(zero)) };
            for (k, i) in [("red", 0), ("green", 1), ("blue", 2)] {
                let c = (num(args, i, 0.0) as i64 as u32) & 0xFF;
                g.field_overrides.insert((npc, format!("HCLR/{k}")), XVal::Num(f64::from(c)));
            }
            one
        }
        // `value [creature]`: the creature base's DATA combat skill, above 255 kept at
        // 255 (below 0 at 0, see docs/spec-notes/ext-itr.md §2; not a finite number, 0 [decision]); the calling
        // reference's base when no creature
        // is given; 1 done [documented: SetCreatureCombatSkill]. GetCreatureCombatSkill
        // reads it.
        "setcreaturecombatskill" => {
            let Some(c) = form(args, 1).or(subject).map(|f| base(g, f)).filter(|&c| g.data.sig(c) == Some(*b"CREA")) else { return Some(Ok(zero)) };
            g.field_overrides.insert((c, "DATA/combat_skill".into()), XVal::Num(Some(num(args, 0, 0.0)).filter(|v| v.is_finite()).map_or(0.0, |v| v.trunc().clamp(0.0, 255.0))));
            one
        }
        // ---- casinos ----
        // `casino`: 1 when the player's earnings there reached the max winnings
        // [documented: GetCasinoBan]; 0 never played there.
        "getcasinoban" => {
            let Some(c) = form(args, 0).filter(|c| g.data.casinos.contains_key(c)) else { return Some(Ok(zero)) };
            let max = g.data.casinos.get(&c).map_or(0, |d| d.max_winnings);
            yes(g.player.casino_earnings.get(&c).is_some_and(|&e| max > 0 && e >= i64::from(max)))
        }
        // `casino banned`: 1 the earnings set to the max winnings (an entry made when
        // missing), 0 to one below it where there is an entry, with the winnings level
        // then 3 [documented: SetCasinoBan].
        "setcasinoban" => {
            let Some(c) = form(args, 0).filter(|c| g.data.casinos.contains_key(c)) else { return Some(Ok(zero)) };
            let max = i64::from(g.data.casinos.get(&c).map_or(0, |d| d.max_winnings));
            if num(args, 1, 0.0) != 0.0 {
                g.player.casino_earnings.insert(c, max);
                g.player.casino_levels.insert(c, 4);
            } else if let Some(e) = g.player.casino_earnings.get_mut(&c) {
                *e = max - 1;
                let l = g.player.casino_levels.entry(c).or_insert(0);
                *l = (*l).min(3);
            }
            one
        }
        // ---- challenges ----
        // `challenge amount`: forwards, as many steps as the challenge takes, so one call
        // can cross a recurring challenge's threshold more than once [documented:
        // ModChallenge, "Handles overflow"]; backwards, the progress less the amount, not
        // below 0 and not undoing a completion (see docs/spec-notes/ext-itr.md §4). 1 for a challenge.
        "modchallenge" => {
            let Some(c) = form(args, 0).filter(|c| g.data.challenge_defs.contains_key(c)) else { return Some(Ok(zero)) };
            let mut amount = num(args, 1, 0.0).trunc();
            if amount < 0.0 {
                let s = g.player.challenges.entry(c).or_default();
                s.progress = s.progress.saturating_sub((-amount).min(f64::from(u32::MAX)) as u32);
            } else {
                let goal = f64::from(g.data.challenge_defs.get(&c).map_or(1, |d| d.goal()).max(1));
                let mut guard = 0;
                while amount > 0.0 && guard < 10_000 {
                    let have = f64::from(g.player.challenges.get(&c).map_or(0, |s| s.progress));
                    let step = amount.min((goal - have).max(1.0));
                    let before = g.player.challenges.get(&c).map(|s| (s.progress, s.times));
                    g.challenge_step(c, step);
                    amount -= step;
                    guard += 1;
                    if g.player.challenges.get(&c).map(|s| (s.progress, s.times)) == before {
                        break;
                    }
                }
            }
            one
        }
        // ---- worldspaces ----
        // `worldspace`: WRLD ONAM's world map scale and cell X and Y offsets; 0 for no
        // worldspace [documented: GetWorldspaceOffsetScale, …X, …Y; the fields: xEdit
        // wbWorldMapOffset].
        "getworldspaceoffsetscale" | "getworldspaceoffsetx" | "getworldspaceoffsety" => {
            let path = match name {
                "getworldspaceoffsetscale" => "world_map_scale",
                "getworldspaceoffsetx" => "cell_x_offset",
                _ => "cell_y_offset",
            };
            XVal::Num(form(args, 0).filter(|&w| g.data.sig(w) == Some(*b"WRLD")).and_then(|w| field(g, w, "ONAM", path)).unwrap_or(0.0))
        }
        // ---- dialogue info flags ----
        // `info`: DATA's two flag bytes, the second from bit 8; 0 for anything else
        // [documented: GetDialogueInfoFlags]. SetDialogueInfoFlags `info flags` writes
        // all 16 bits [documented] for the session (`crate::dialogue_edits`): the dialogue's
        // Say Once, Goodbye and Random rules read them too (ext-sup2.md §1).
        "getdialogueinfoflags" => XVal::Num(form(args, 0).and_then(|i| g.info_flags(i)).map_or(0.0, f64::from)),
        "setdialogueinfoflags" => {
            let Some(i) = form(args, 0).filter(|i| g.data.infos.contains_key(i)) else { return Some(Ok(zero)) };
            g.set_info_flags(i, (num(args, 1, 0.0) as i64 as u32 & 0xFFFF) as u16);
            one
        }
        // ---- the radio ----
        // [documented: IsRadioPlaying; GetPlayingRadioTrack, the song's sound or the
        // line's info, 0 when none; GetPlayingRadioTrackFileName, "" when none;
        // GetPlayingRadioText, the line's first response, "" for a song or none].
        "isradioplaying" => yes(g.other.itr.radio_playing.is_some()),
        "getplayingradiotrack" => XVal::Ref(g.other.itr.radio_playing.as_ref().and_then(|p| p.track).map_or(0, |f| f.0)),
        "getplayingradiotrackfilename" => XVal::Str(g.other.itr.radio_playing.as_ref().map(|p| p.file.clone()).unwrap_or_default()),
        "getplayingradiotext" => {
            let info = g.other.itr.radio_playing.as_ref().and_then(|p| p.track);
            XVal::Str(info.and_then(|i| g.data.infos.get(&i)).and_then(|d| d.responses.first()).map(|r| r.text.clone()).unwrap_or_default())
        }
        // The engine skips to the next track; 1, or 0 with nothing playing [documented:
        // ChangeRadioTrack].
        "changeradiotrack" => {
            if g.other.itr.radio_playing.is_none() {
                return Some(Ok(zero));
            }
            g.other.itr.radio_skips += 1;
            one
        }
        // `path`: to the front of the queue (its last dropped when full) and played now
        // on the Pip-Boy's station; 0 with no station on or a bad path [documented:
        // PlayRadioFile].
        "playradiofile" => {
            let p = text(args, 0);
            if !radio_path_ok(&p) || !g.radio.pipboy.on || g.radio.pipboy.station.is_none() {
                return Some(Ok(zero));
            }
            let q = &mut g.other.itr.radio_queue;
            if q.len() >= RADIO_QUEUE {
                q.pop_back();
            }
            q.push_front(p.clone());
            g.other.itr.radio_play_now = Some(p);
            one
        }
        // `path`: to the back of the queue, played at the next track change; 0 for a bad
        // path or a full queue [documented: QueueRadioTrack].
        "queueradiotrack" => {
            let p = text(args, 0);
            if !radio_path_ok(&p) || g.other.itr.radio_queue.len() >= RADIO_QUEUE {
                return Some(Ok(zero));
            }
            g.other.itr.radio_queue.push_back(p);
            one
        }
        // The tracks taken off [documented: ClearRadioQueue].
        "clearradioqueue" => {
            let n = g.other.itr.radio_queue.len();
            g.other.itr.radio_queue.clear();
            XVal::Num(n as f64)
        }
        "getradioqueuesize" => XVal::Num(g.other.itr.radio_queue.len() as f64),
        "setradioqueueloop" => {
            g.other.itr.radio_loop = num(args, 0, 0.0) != 0.0;
            one
        }
        // ---- UI tiles ----
        // `path trait`: a positive id, the same one for a trait already watched, 0 when
        // 48 are watched [documented: WatchTileValue]; a trait that is not there is
        // watched too while its menu is [observed: itr probes, "HUDMainMenu"
        // "_nvfnone" returns an id; the page says 0], 0 without the menu (see docs/spec-notes/ext-itr.md §6).
        // The trait's path is the tile's path and the trait's name.
        "watchtilevalue" => {
            let path = format!("{}/{}", text(args, 0).trim_end_matches(['/', '\\']), text(args, 1));
            let v = g.ui_tree.get(&path).map(TileValue::from);
            if v.is_none() && g.ui_tree.menu(menu_of(&path)).is_none() {
                return Some(Ok(zero));
            }
            let norm = |p: &str| p.replace('\\', "/").to_ascii_lowercase();
            if let Some(w) = g.other.itr.tile_watches.iter().find(|w| norm(&w.path) == norm(&path)) {
                return Some(Ok(XVal::Num(f64::from(w.id))));
            }
            if g.other.itr.tile_watches.len() >= TILE_WATCHES {
                return Some(Ok(zero));
            }
            let s = &mut g.other.itr;
            s.next_watch += 1;
            let id = s.next_watch;
            s.tile_watches.push(TileWatch { id, path, last: v });
            XVal::Num(f64::from(id))
        }
        "unwatchtilevalue" => {
            let id = num(args, 0, 0.0) as i64;
            let before = g.other.itr.tile_watches.len();
            g.other.itr.tile_watches.retain(|w| i64::from(w.id) != id);
            yes(g.other.itr.tile_watches.len() < before)
        }
        // `tile texture`: the alpha map for the tile's renderer; 1 on success, 0 when the
        // tile is not found or the texture fails to load [documented: SetUIAlphaMap]. The
        // texture is not decoded here: the engine's tile renderer reads it when it draws
        // the tile (`nvf-play`'s `tiles::texmod`), so a texture not in the data (loose or
        // in an archive, under `Data\Textures\`) is the failure this side sees
        // (existence as the test for "texture load failure", see docs/spec-notes/ext-itr.md §6;
        // a file there that does not decode is refused by the renderer only, said in its log). While the host
        // gives no answer for the archives (no Data open, or a game made without the
        // engine) the file is taken as there [decision]. A refused call leaves the tile as
        // it was [hypothesis].
        "setuialphamap" => {
            let path = text(args, 0);
            let tex = text(args, 1);
            let Some(t) = g.ui_tree.find_tile(&path).filter(|_| !tex.trim().is_empty()) else { return Some(Ok(zero)) };
            if crate::mod_files::knows_archives() && !crate::mod_files::exists(&alpha_map_path(&tex)) {
                return Some(Ok(zero));
            }
            g.other.itr.alpha_maps.insert(t, tex);
            one
        }
        // `tile x y`: wrapped into 0-1; image and radial tiles only [documented:
        // SetUITexOffset]. An offset that is not a finite number is refused, as it wraps
        // to no number [decision].
        "setuitexoffset" => {
            let path = text(args, 0);
            let (x, y) = (num(args, 1, 0.0), num(args, 2, 0.0));
            if !x.is_finite() || !y.is_finite() {
                return Some(Ok(zero));
            }
            let Some(t) = g.ui_tree.find_tile(&path) else { return Some(Ok(zero)) };
            if !matches!(g.ui_tree.tile(t).kind.as_str(), "image" | "radial") {
                return Some(Ok(zero));
            }
            g.other.itr.tex_offsets.insert(t, (wrap(x), wrap(y)));
            one
        }
        // ---- item mods ----
        // On an inventory reference: the weapon's mod bits (1, 2, 4) [documented:
        // GetItemModFlags]; SetItemModFlags `[flags]`, 1 by default, 0 takes them off
        // [documented]; 1 done, 0 when the item could not be changed (see docs/spec-notes/ext-itr.md §6).
        "getitemmodflags" => match subject.and_then(|r| g.temp_item(r)) {
            Some((c, it)) => XVal::Num(f64::from(g.world.inventory_mut(c).map(|inv| inv.groups().find(|(i, _, _)| *i == it).map_or(0, |(_, e, _)| e.mods)).unwrap_or(0))),
            None => zero,
        },
        "setitemmodflags" => {
            let Some((c, it)) = subject.and_then(|r| g.temp_item(r)) else { return Some(Ok(zero)) };
            let m = (num(args, 0, 1.0) as i64 as u32 & 7) as u8;
            let Ok(inv) = g.world.inventory_mut(c) else { return Some(Ok(zero)) };
            let Some(e) = inv.groups().find(|(i, _, _)| *i == it).map(|(_, e, _)| e) else { return Some(Ok(zero)) };
            yes(inv.set_mods_one(it, e, m))
        }
        // ---- the weapon's emissive colour ----
        // `r g b mult` on the player's first-person weapon, for the engine's materials
        // (follow-up); 0 with no weapon in hand [documented: SetWeaponEmissiveColor;
        // hypothesis: the weapon in hand stands for its first-person node], or for a
        // value that is not a finite number [decision].
        "setweaponemissivecolor" => {
            let colour = [num(args, 0, 0.0), num(args, 1, 0.0), num(args, 2, 0.0), num(args, 3, 1.0)];
            if crate::functions::items::equipped_weapon(g, FormId::PLAYER_REF).is_none() || !colour.iter().all(|c| c.is_finite()) {
                return Some(Ok(zero));
            }
            g.other.itr.emissive = Some(colour);
            one
        }
        // 0 when nothing was set [documented: ClearWeaponEmissiveColor].
        "clearweaponemissivecolor" => yes(g.other.itr.emissive.take().is_some()),
        _ => return None,
    };
    Some(Ok(v))
}
