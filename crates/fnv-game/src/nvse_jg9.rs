//! JohnnyGuitar NVSE, batch 9 (#1007): sounds played from a file's path, PlaySoundFade,
//! StopSoundLooping and the scripted holotapes [documented: each page in the wiki export;
//! docs/spec-notes/ext-jg9.md].
//!
//! A sound file a script names gets a sound of its own for the session: a form ID in
//! [`PATH_SOUND_BASE`]'s range, one per file, source and looping, which the engine plays
//! as it plays a SOUN (`nvf-play` sfx.rs reads [`Jg9State::new_paths`]), so IsSoundPlaying,
//! StopSound and the engine's answers work for it as for any script sound.

use crate::game::Game;
use crate::sound::SoundRequest;
use fnv_script::vm::XVal;
use fnv_sim::FormId;
use std::collections::HashMap;

/// The first form ID of the sounds made for files [engineering choice: high in the 0xFF
/// range, past any form a session creates].
pub const PATH_SOUND_BASE: u32 = 0xFFF0_0000;

/// Sounds made for files kept at most [design].
pub const MAX_PATH_SOUNDS: usize = 4096;

/// UIPipBoyHolotapeStart and UIPipBoyHolotapeStop [observed: FalloutNV.esm SOUN
/// 0007DFF4 and 0007DFF5], PlayHolotape's and StopHolotape's start/stop sound.
pub const HOLOTAPE_START: FormId = FormId(0x0007_DFF4);
pub const HOLOTAPE_STOP: FormId = FormId(0x0007_DFF5);

/// A sound made for a file.
#[derive(Debug, Clone, PartialEq)]
pub struct PathSound {
    pub id: FormId,
    /// The Data path as the archives spell it, lower case, `\`.
    pub path: String,
    /// The reference it plays at (PlaySound3DFromPath); `None` not placed.
    pub at: Option<FormId>,
    pub looping: bool,
    /// Played by PlaySoundFile (StopSoundFile stops those).
    pub file: bool,
}

/// What a script asks of the holotape player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HolotapeRequest {
    Play(FormId),
    Stop,
}

/// An audio marker's values (REFR MMRK, XRDS, CNAM, MNAM, NNAM; `fnv_world::audio`):
/// its media location controller, radius and layer 2 and 3 trigger fractions.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MarkerValues {
    pub controller: Option<FormId>,
    pub radius: f32,
    pub layer2: f32,
    pub layer3: f32,
}

/// This batch's state.
#[derive(Debug, Clone, Default)]
pub struct Jg9State {
    /// Every sound made for a file this session, by its id less [`PATH_SOUND_BASE`].
    pub path_sounds: Vec<PathSound>,
    /// Made since the engine last looked: id, Data path, looping.
    pub new_paths: Vec<(FormId, String, bool)>,
    /// Script sounds (request ids) to fade in, and over how many seconds.
    pub fades_in: Vec<(u32, f32)>,
    /// Sounds stopped with a fade: over how many seconds they fade out.
    pub fades_out: Vec<(FormId, f32)>,
    /// For the holotape player, oldest first.
    pub holotape: Vec<HolotapeRequest>,
    /// Terminals whose menu items a script added or removed: what stands at each
    /// position (`crate::terminal::slots`).
    pub terminal_slots: HashMap<FormId, Vec<Option<usize>>>,
    /// RefreshTerminalMenu: the engine reads the open menu's list again.
    pub refresh_terminal: bool,
    /// The audio markers' values as the engine read them from the plugins (`nvf-play`
    /// music.rs tells them once), and the marker whose music plays now.
    pub markers: HashMap<FormId, MarkerValues>,
    pub current_marker: Option<FormId>,
    /// The markers a script changed this session (AudioMarkerSet…), their values now;
    /// `markers_changed` until the engine's music takes them up.
    pub marker_edits: HashMap<FormId, MarkerValues>,
    pub markers_changed: bool,
    /// Actors a script made jump (Jump), for the engine.
    pub jumps: Vec<FormId>,
    /// SetAutoMove's word, for the engine's controls.
    pub auto_move: Option<bool>,
    /// SetPlayerMovementFlags' flags, for the engine's next frame of the player's moves.
    pub move_flags: Option<u32>,
    /// SetVelEx's velocities (game units per second), for the engine.
    pub velocities: Vec<(FormId, [f32; 3])>,
    /// What the engine tells: actors' run speeds as their pace gave them (units per
    /// second; the player's too), actors' spreads as their last shot or the crosshair
    /// had them, how long the player held a thrown weapon's trigger, the nearest hostile
    /// compass mark's direction (0-7; of all, and of those not invisible), the sky's
    /// colours now (0-255, in the weather colour types' order) and whether the player
    /// looks through the third-person camera (false, first person, until the engine
    /// tells: the original answered the first person's FOV in the probe's save
    /// [observed 2026-11-10]).
    pub run_speeds: HashMap<FormId, f32>,
    pub spreads: HashMap<FormId, f32>,
    pub grenade_hold: f32,
    pub compass_hostile: [Option<u8>; 2],
    pub sky_colors: Option<[[f32; 3]; 10]>,
    pub third_person: bool,
}

/// A path "relative to the New Vegas folder" [documented: PlaySoundFile] as a Data path:
/// `Data\` taken off, lower case, `\`; `None` for one outside Data, with a drive, a
/// leading slash or `..` (only Data's files are the game's to play) [engineering choice].
pub fn data_path(path: &str) -> Option<String> {
    let p = path.trim().replace('/', "\\").to_ascii_lowercase();
    let rest = p.strip_prefix("data\\")?;
    if rest.is_empty() || rest.starts_with('\\') || rest.contains(':') || rest.split('\\').any(|s| s == "..") {
        return None;
    }
    Some(rest.to_string())
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

fn text(args: &[XVal], k: usize) -> Option<String> {
    match args.get(k)? {
        XVal::Str(s) => Some(s.clone()),
        _ => None,
    }
}

/// The return value and the outputs, for the VM to write into the script variables
/// (`fnv_script::vm::out_params`).
fn outputs(g: &mut Game, ret: f64, vals: &[f64]) -> XVal {
    use fnv_script::vm::{ArrayKind, XKey};
    let entries = std::iter::once(ret).chain(vals.iter().copied()).enumerate().map(|(i, v)| (XKey::Num(i as f64), XVal::Num(v))).collect();
    XVal::Array(g.ext.new_array(ArrayKind::List, entries))
}

fn int_arg(args: &[XVal], k: usize) -> Option<i64> {
    match args.get(k) {
        Some(XVal::Num(x)) if x.fract() == 0.0 => Some(*x as i64),
        _ => None,
    }
}

fn num_index(args: &[XVal], k: usize) -> Option<usize> {
    match args.get(k) {
        Some(XVal::Num(x)) if *x >= 0.0 && x.fract() == 0.0 => Some(*x as usize),
        _ => None,
    }
}

fn yes(b: bool) -> XVal {
    XVal::Num(f64::from(u8::from(b)))
}

impl Game<'_> {
    /// The sound made for a file, source and looping, made now if there is none.
    pub fn path_sound(&mut self, path: &str, at: Option<FormId>, looping: bool, file: bool) -> Option<FormId> {
        let st = &mut self.other.jg9;
        if let Some(p) = st.path_sounds.iter().find(|p| p.path == path && p.at == at && p.looping == looping && p.file == file) {
            return Some(p.id);
        }
        if st.path_sounds.len() >= MAX_PATH_SOUNDS {
            return None;
        }
        let id = FormId(PATH_SOUND_BASE + st.path_sounds.len() as u32);
        st.path_sounds.push(PathSound { id, path: path.to_string(), at, looping, file });
        st.new_paths.push((id, path.to_string(), looping));
        Some(id)
    }

    /// Play a sound for a script and fade it in over `fade` seconds; the request's id.
    fn play_faded(&mut self, sound: FormId, at: Option<FormId>, system: bool, fade: f64) -> u32 {
        self.ask_sound(SoundRequest { sound, at, system, id: 0, point: None });
        let id = self.sound_requests.last().map_or(0, |r| r.id);
        if fade > 0.0 && fade.is_finite() {
            self.other.jg9.fades_in.push((id, fade as f32));
        }
        id
    }

    /// Stop what a script plays of `sound`, fading it out over `fade` seconds (0 at once).
    fn stop_faded(&mut self, sound: FormId, fade: f64) {
        if fade > 0.0 && fade.is_finite() {
            self.other.jg9.fades_out.push((sound, fade as f32));
        }
        let frame = self.frame;
        self.gaps.sounds.stop(sound, frame);
    }

    /// The sounds made for files that `keep` keeps, stopped (fading over `fade`); whether
    /// any of them had a playing not over or stopped (also one asked for this frame)
    /// [hypothesis: isSuccess is whether something was stopped].
    fn stop_paths(&mut self, fade: f64, keep: impl Fn(&PathSound) -> bool) -> bool {
        let ids: Vec<FormId> = self.other.jg9.path_sounds.iter().filter(|p| keep(p)).map(|p| p.id).collect();
        let mut any = false;
        for id in ids {
            any |= self.gaps.sounds.played.iter().any(|p| p.sound == id && p.stopped.is_none());
            self.stop_faded(id, fade);
        }
        any
    }

    /// An audio marker's values now: a script's, else the plugins'.
    pub fn marker_values(&self, r: FormId) -> Option<MarkerValues> {
        let st = &self.other.jg9;
        st.marker_edits.get(&r).or_else(|| st.markers.get(&r)).copied()
    }

    /// What the holotape player is asked, oldest first, since the last call.
    pub fn take_holotape_requests(&mut self) -> Vec<HolotapeRequest> {
        std::mem::take(&mut self.other.jg9.holotape)
    }
}

pub(crate) fn call(g: &mut Game, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let v = match name {
        // PlaySoundFile path [forcePlay] [shouldLoop] [playInMainMenu] [documented].
        // forcePlay is read as playing in menu mode too, as playInMainMenu (see docs/spec-notes/ext-jg9.md §1).
        // Returns 1 even for a nonexistent path [observed: Fallout: New Vegas 1.4.0.525 +
        // xNVSE 6.4.9 + JohnnyGuitarNVSE 528 + JIP LN 57.30, 2026-11-23, `GSDocMitchellHouse`,
        // `b3_jg9_01_sound_runspeed_compass_pipboy` (`wave4b_extenders.json`, #3679):
        // `PROBE|b3_jg9_01_sound_runspeed_compass_pipboy|ps=1,ss=0,rsP=338.8,rsD=338.8,hostDir=-1,pb=0`].
        // Paths escaping Data\ play nothing and still return 1 [design].
        "playsoundfile" => {
            let Some(p) = text(args, 0).as_deref().and_then(data_path).filter(|p| crate::mod_files::exists(p)) else { return Some(Ok(yes(true))) };
            let looping = num(args, 2, 0.0) != 0.0;
            let system = num(args, 1, 0.0) != 0.0 || num(args, 3, 0.0) != 0.0;
            let Some(id) = g.path_sound(&p, None, looping, true) else { return Some(Ok(yes(true))) };
            g.play_faded(id, None, system, 0.0);
            yes(true)
        }
        // StopSoundFile: what PlaySoundFile plays [documented].
        "stopsoundfile" => yes(g.stop_paths(0.0, |p| p.file)),
        // PlaySoundFromPath path [fadeTime] [isVoice] [systemSound] [shouldLoop]
        // [documented]. isVoice plays it as any script sound (see docs/spec-notes/ext-jg9.md §1).
        // 1 for any path, a missing file too [observed: the original, 2026-11-10].
        "playsoundfrompath" => {
            let Some(p) = text(args, 0).as_deref().and_then(data_path).filter(|p| crate::mod_files::exists(p)) else { return Some(Ok(yes(true))) };
            let looping = num(args, 4, 0.0) != 0.0;
            let Some(id) = g.path_sound(&p, None, looping, false) else { return Some(Ok(yes(true))) };
            g.play_faded(id, None, num(args, 3, 0.0) != 0.0, num(args, 1, 0.0));
            yes(true)
        }
        // ref.PlaySound3DFromPath path [fadeTime] [isVoice] [shouldLoop] [documented]; 1
        // as PlaySoundFromPath [hypothesis].
        "playsound3dfrompath" => {
            let Some(at) = subject else { return Some(Ok(yes(false))) };
            let Some(p) = text(args, 0).as_deref().and_then(data_path).filter(|p| crate::mod_files::exists(p)) else { return Some(Ok(yes(true))) };
            let looping = num(args, 3, 0.0) != 0.0;
            let Some(id) = g.path_sound(&p, Some(at), looping, false) else { return Some(Ok(yes(true))) };
            g.play_faded(id, Some(at), false, num(args, 1, 0.0));
            yes(true)
        }
        // StopSoundFromPath path [fadeTime]: "all instances" of the file [documented].
        // 0, with the file just played [observed: the original, 2026-11-10; hypothesis:
        // it returns nothing]; the 3D one alike [hypothesis].
        "stopsoundfrompath" => {
            if let Some(p) = text(args, 0).as_deref().and_then(data_path) {
                g.stop_paths(num(args, 1, 0.0), |s| s.path == p);
            }
            yes(false)
        }
        // ref.StopSound3DFromPath path [fadeTime]: those from that source [documented].
        "stopsound3dfrompath" => {
            if let (Some(at), Some(p)) = (subject, text(args, 0).as_deref().and_then(data_path)) {
                g.stop_paths(num(args, 1, 0.0), |s| s.path == p && s.at == Some(at));
            }
            yes(false)
        }
        // ref.PlaySoundFade sound fadeTime: at the calling reference, the player without
        // one [documented].
        "playsoundfade" => {
            let Some(sound) = form(args, 0) else { return Some(Ok(yes(false))) };
            let at = subject.unwrap_or(FormId::PLAYER_REF);
            g.play_faded(sound, Some(at), false, num(args, 1, 0.0));
            yes(true)
        }
        // StopSoundLooping sound: "gradually" by its Loop End point [documented]. The
        // engine plays no loop points: what a script plays of it stops (see docs/spec-notes/ext-jg9.md §1).
        "stopsoundlooping" => {
            let Some(sound) = form(args, 0) else { return Some(Ok(yes(false))) };
            let was = g.gaps.sounds.is_playing(sound, g.frame);
            g.stop_faded(sound, 0.0);
            yes(was)
        }
        // PlayHolotape note [playStartStopSound] [documented]: a note with something to
        // play, played by the engine's holotape player as one picked in the Pip-Boy.
        "playholotape" => {
            let Some(note) = form(args, 0).filter(|n| g.note_tape(*n).is_some()) else { return Some(Ok(yes(false))) };
            if num(args, 1, 0.0) != 0.0 {
                g.ask_sound(SoundRequest { sound: HOLOTAPE_START, at: None, system: true, id: 0, point: None });
            }
            g.other.jg9.holotape.push(HolotapeRequest::Play(note));
            yes(true)
        }
        // StopHolotape [playStartStopSound] (1 by default) [documented].
        "stopholotape" => {
            if num(args, 0, 1.0) != 0.0 {
                g.ask_sound(SoundRequest { sound: HOLOTAPE_STOP, at: None, system: true, id: 0, point: None });
            }
            g.other.jg9.holotape.push(HolotapeRequest::Stop);
            yes(true)
        }
        // AddTerminalMenuItem terminal: a new, empty item at the end [documented; hypothesis:
        // at the end]. Its text and the rest are set with Set…TerminalMenuItem….
        "addterminalmenuitem" => {
            let Some(t) = form(args, 0).filter(|t| g.data.terminals.contains_key(t)) else { return Some(Ok(yes(false))) };
            let mut s = crate::terminal::slots(g, t);
            s.push(None);
            g.other.jg9.terminal_slots.insert(t, s);
            yes(true)
        }
        // RemoveTerminalMenuItem terminal index [documented]: the items after it move up,
        // with a script's changes to them.
        "removeterminalmenuitem" => {
            let Some(t) = form(args, 0).filter(|t| g.data.terminals.contains_key(t)) else { return Some(Ok(yes(false))) };
            let mut s = crate::terminal::slots(g, t);
            let Some(i) = num_index(args, 1).filter(|&i| i < s.len()) else { return Some(Ok(yes(false))) };
            s.remove(i);
            g.other.jg9.terminal_slots.insert(t, s);
            let edits = std::mem::take(&mut g.other.terminal_items);
            g.other.terminal_items = edits
                .into_iter()
                .filter(|((b, k), _)| !(*b == t && *k == i))
                .map(|((b, k), e)| ((b, if b == t && k > i { k - 1 } else { k }), e))
                .collect();
            // An open menu lists positions, which have just moved: it is read again at
            // once, so picking what it shows runs that item, not the next one (#2018).
            g.other.jg9.refresh_terminal = true;
            yes(true)
        }
        // ref.AudioMarkerGetController (AMKGetCtrl): the marker's media location
        // controller [documented]; a null reference for a reference that is no audio
        // marker or has none.
        "audiomarkergetcontroller" | "amkgetctrl" => XVal::Ref(subject.and_then(|r| g.marker_values(r)).and_then(|m| m.controller).map_or(0, |c| c.0)),
        // AudioMarkerGetCurrent (AMKGetCur): the marker whose music plays now [documented],
        // as the engine's music picked it.
        "audiomarkergetcurrent" | "amkgetcur" => XVal::Ref(g.other.jg9.current_marker.map_or(0, |r| r.0)),
        // ref.AudioMarkerGetProperty (AMKGetProp) n: 0 radius, 1 layer 2 trigger %, 2
        // layer 3 trigger % [documented]; the triggers as stored, fractions (see docs/spec-notes/ext-jg9.md §4).
        "audiomarkergetproperty" | "amkgetprop" => {
            let m = subject.and_then(|r| g.marker_values(r));
            XVal::Num(match (m, int_arg(args, 0)) {
                (Some(m), Some(0)) => f64::from(m.radius),
                (Some(m), Some(1)) => f64::from(m.layer2),
                (Some(m), Some(2)) => f64::from(m.layer3),
                _ => 0.0,
            })
        }
        // ref.AudioMarkerSetController (AMKSetCtrl) controller [documented]: an ALOC, for
        // the session; the engine's music picks from it.
        "audiomarkersetcontroller" | "amksetctrl" => {
            let (Some(r), Some(c)) = (subject, form(args, 0)) else { return Some(Ok(yes(false))) };
            let Some(mut m) = g.marker_values(r) else { return Some(Ok(yes(false))) };
            m.controller = Some(c);
            g.other.jg9.marker_edits.insert(r, m);
            g.other.jg9.markers_changed = true;
            yes(true)
        }
        // ref.AudioMarkerSetProperty (AMKSetProp) n value [documented].
        "audiomarkersetproperty" | "amksetprop" => {
            let Some(r) = subject else { return Some(Ok(yes(false))) };
            let Some(mut m) = g.marker_values(r) else { return Some(Ok(yes(false))) };
            let v = num(args, 1, 0.0) as f32;
            match int_arg(args, 0) {
                Some(0) => m.radius = v.max(0.0),
                Some(1) => m.layer2 = v,
                Some(2) => m.layer3 = v,
                _ => return Some(Ok(yes(false))),
            }
            g.other.jg9.marker_edits.insert(r, m);
            g.other.jg9.markers_changed = true;
            yes(true)
        }
        // actor.Jump: the actor jumps [documented], the engine's jump: the player's
        // Jump key, a person's ledge hop (taken standing on the ground).
        "jump" => {
            let a = subject.unwrap_or(FormId::PLAYER_REF);
            if g.other.jg9.jumps.len() < 256 {
                g.other.jg9.jumps.push(a);
            }
            XVal::Num(0.0)
        }
        // SetAutoMove 0/1: the player's Auto Move [documented].
        "setautomove" => {
            let Some(on) = int_arg(args, 0) else { return Some(Ok(yes(false))) };
            g.other.jg9.auto_move = Some(on != 0);
            yes(true)
        }
        // SetPlayerMovementFlags flags: the player moves as the flags say in the next
        // frame (1 forward, 2 back, 4 left, 8 right, 256 walking, 512 running, 1024
        // sneaking, 4096 jumping) [documented: the flags; hypothesis: for one frame, as
        // the movement keys would; the turn, swim, fly, fall and slide flags are the
        // engine's own].
        "setplayermovementflags" => {
            let Some(f) = int_arg(args, 0).filter(|f| *f >= 0) else { return Some(Ok(XVal::Num(0.0))) };
            g.other.jg9.move_flags = Some(f as u32);
            XVal::Num(0.0)
        }
        // ref.SetVelEx x y z: the three axes of SetVel at once [documented], units/s.
        "setvelex" => {
            let Some(r) = subject else { return Some(Ok(XVal::Num(0.0))) };
            let v = [num(args, 0, 0.0) as f32, num(args, 1, 0.0) as f32, num(args, 2, 0.0) as f32];
            if v.iter().all(|x| x.is_finite()) && g.other.jg9.velocities.len() < 256 {
                g.other.jg9.velocities.push((r, v));
            }
            XVal::Num(0.0)
        }
        // ref.GetLinearVelocity node xOut yOut zOut: "in units of 0.1 Meter/sec, or 7
        // game Units/sec" [documented]: the reference's velocity as GetActorVelocity
        // follows it (`crate::nvse_jip2`), over 7; the node's own body is not told apart
        // (see docs/spec-notes/ext-jg9.md §5).
        "getlinearvelocity" => {
            let a = subject.unwrap_or(FormId::PLAYER_REF);
            let here = g.ref_pos_rot(a).map(|p| p.0).unwrap_or([0.0; 3]);
            let v = g.jip2.velocity.entry(a).or_insert((here, [0.0; 3])).1;
            outputs(g, 0.0, &v.map(|x| f64::from(x) / 7.0))
        }
        // actor.GetRunSpeed: its run speed [documented], units per second, as the
        // engine's pace gives it (people's and creatures' paces; the player's run with
        // its Speed Mult and perks); 0 for one not drawn (see docs/spec-notes/ext-jg9.md §5).
        "getrunspeed" => XVal::Num(f64::from(g.other.jg9.run_speeds.get(&subject.unwrap_or(FormId::PLAYER_REF)).copied().unwrap_or(0.0))),
        // GetPlayerKarmaTitle 0/1: the title (by level and karma) or the tier
        // [documented], as the Pip-Boy's stats show them (`crate::karma_titles`).
        "getplayerkarmatitle" => {
            let tier = num(args, 0, 0.0) != 0.0;
            XVal::Str(g.player_karma().map_or_else(String::new, |(word, title)| if tier { word.to_string() } else { title }))
        }
        // [item].GetItemEffectString item: the item card's effect line for the player
        // [documented], as the Pip-Boy writes it (`crate::item_text`).
        "getitemeffectstring" => {
            let item = form(args, 0).or_else(|| subject.map(|r| crate::functions::base_of(g, r).unwrap_or(r)));
            XVal::Str(item.map_or_else(String::new, |i| g.item_effect_text(FormId::PLAYER_REF, i)))
        }
        // actor.GetCalculatedSpread: the actor's spread with its weapon now (spread
        // units: movement, perks, ammo and mods) [documented], as the engine's crosshair
        // (the player) or its last shot (others) had it; 0 before any.
        "getcalculatedspread" => XVal::Num(f64::from(g.other.jg9.spreads.get(&subject.unwrap_or(FormId::PLAYER_REF)).copied().unwrap_or(0.0))),
        // GetGrenadeHoldTime: seconds the player has held a thrown weapon's trigger,
        // kept after the throw until the next hold [documented].
        "getgrenadeholdtime" => XVal::Num(f64::from(g.other.jg9.grenade_hold)),
        // GetNearestCompassHostileDirection [skipInvisible]: 0 front … 7 front left
        // [documented]; -1 with no hostile mark [observed: the original, 2026-11-10].
        "getnearestcompasshostiledirection" => {
            let k = usize::from(num(args, 0, 0.0) != 0.0);
            XVal::Num(g.other.jg9.compass_hostile[k].map_or(-1.0, f64::from))
        }
        // GetCurrentSkyColor type rOut gOut bOut: the sky's colour of that type now
        // [documented: 0 sky upper … 9 clouds upper], as normalized 0.0..=1.0 floats (in
        // interiors too); out-of-range types (or no sky state) return 0 and leave the
        // output variables unchanged [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9
        // + JohnnyGuitarNVSE 528, 2026-11-24, `w6_t1_09_jg_sky_color_interior` and
        // `w6_t1_15b_raycast_ref_mat_reticle_sky`
        // (`tools/capture/nvse/catalogs/wave6_t1_jg_sky_color.json`, `ext-jg9.md` §6)].
        "getcurrentskycolor" => {
            let c = int_arg(args, 0).filter(|t| (0..10).contains(t)).zip(g.other.jg9.sky_colors).map(|(t, c)| c[t as usize]);
            match c {
                Some(c) => outputs(g, 1.0, &c.map(|x| f64::from(x) / 255.0)),
                None => outputs(g, 0.0, &[]),
            }
        }
        // ref.GetPlayerCamFOV 0/1/2: the world FOV, the first person's, or the one the
        // camera uses now [documented], as GetCameraFOV reads them.
        "getplayercamfov" => {
            let which = match int_arg(args, 0) {
                Some(1) => 1.0,
                Some(2) => f64::from(u8::from(!g.other.jg9.third_person)),
                _ => 0.0,
            };
            match crate::nvse_small::call(g, &crate::game::RunCtx::default(), "getcamerafov", None, &[XVal::Num(which)]) {
                Some(Ok(v)) => v,
                _ => XVal::Num(0.0),
            }
        }
        // RefreshTerminalMenu: the open terminal menu drawn again [documented]; nothing
        // with none open. No return value is given [hypothesis: 0].
        "refreshterminalmenu" | "rtm" => {
            g.other.jg9.refresh_terminal = true;
            XVal::Num(0.0)
        }
        _ => return None,
    };
    Some(Ok(v))
}
