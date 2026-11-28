//! JohnnyGuitar NVSE, batch 8 (#1007): the remaining event handlers (failed quests, lost
//! limbs, seen cells, processing levels, NPCs' actor values, reputations, NPC responses,
//! sleeping and waiting, general subtitles, the crosshair, the settings menu, keyboard
//! selection, radio sounds, taking items back), the player's custom map marker, file
//! hashes, loaded archives, DDS texture headers, BMP pixels, array shuffling and sorting
//! by editor ID, the system colours, the sleep/wait menu's state and radio tuning
//! [documented: each page in the wiki export; docs/spec-notes/ext-jg8.md].

use crate::game::{Game, RunCtx};
use fnv_script::vm::{ArrayKind, XKey, XVal};
use fnv_sim::FormId;
use rand_core::RngCore;
use std::collections::{BTreeSet, HashMap};

/// The events of this batch's handlers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ev {
    /// A quest is failed: (quest).
    FailQuest,
    /// An actor loses a limb: (actor, limb).
    LimbGone,
    /// A cell is marked as seen: (cell).
    SeenData,
    /// An actor's processing level changes: (actor, old, new).
    ProcessLevel,
    /// An NPC's actor value changes: (actor, code, old, new).
    NpcActorValue,
    /// A reputation changes: (reputation, axis, value).
    Reputation,
    /// An NPC says a response to the player in the dialogue menu: (text, voice path,
    /// emotion, emotion value, response number).
    NpcResponse,
    /// The player starts sleeping or waiting: (mode).
    SleepWait,
    /// A general subtitle is shown: (speaker, text, topic).
    GeneralSubtitle,
    /// The activation prompt comes up on a reference: (reference).
    Crosshair,
    /// The settings menu closes after a change: ().
    SettingsUpdate,
    /// Keyboard or controller navigation moves a menu's selection: (menu_id).
    KeyboardSelection,
    /// A radio reference gets a sound: (radio, mode).
    RadioPostSound,
    /// An item is taken back in the barter menu: (item, count).
    TakeBackItem,
}

/// A handler a script set.
#[derive(Debug, Clone, PartialEq)]
pub struct Handler {
    pub ev: Ev,
    pub func: XVal,
    pub flag: i64,
    /// The optional filters, in the page's order; `None` matches anything.
    pub filters: Vec<Option<XVal>>,
}

/// The session's state of this batch.
#[derive(Debug, Default)]
pub struct Jg8State {
    /// The handlers set, in the order they were set; flushed by a load (a new game
    /// starts without them) [documented: "flushed every game load"].
    pub handlers: Vec<Handler>,
    /// SetCustomMapMarker's place: the worldspace (or interior cell) and the point.
    pub custom_marker: Option<(FormId, [f64; 3])>,
    /// The wait menu was opened as a sleep (the engine sets it as it opens the menu).
    pub sleep_menu: bool,
    /// TuneRadioRef's stations, by radio reference.
    pub radio_tuned: HashMap<FormId, FormId>,
}

impl Game<'_> {
    /// Whether a handler of `ev` is set (to skip the work of an event no one hears).
    pub fn jg8_listening(&self, ev: Ev) -> bool {
        self.other.jg8.handlers.iter().any(|h| h.ev == ev)
    }

    /// Whether filter `f` lets `v` through: a form matches itself, a reference its base,
    /// and a form list its members (and their references) [documented: "Formlists are
    /// also accepted as filters"]; a number matches an equal number.
    fn jg8_matches(&self, f: &XVal, v: &XVal) -> bool {
        match (f, v) {
            (XVal::Ref(f), XVal::Ref(v)) => {
                let (f, v) = (FormId(*f), FormId(*v));
                let base = crate::functions::base_of(self, v);
                if f == v || base == Some(f) {
                    return true;
                }
                let list = crate::functions::list_members(self, f);
                list.contains(&v) || base.is_some_and(|b| list.contains(&b))
            }
            (XVal::Num(f), XVal::Num(v)) => f == v,
            _ => false,
        }
    }

    /// Run the handlers of `ev` whose filters let `keys` through, with `args`; a handler
    /// may take fewer arguments than it is given.
    pub fn jg8_fire(&mut self, ev: Ev, keys: &[XVal], args: Vec<XVal>) {
        if !self.jg8_listening(ev) {
            return;
        }
        let hs: Vec<Handler> = self.other.jg8.handlers.iter().filter(|h| h.ev == ev).cloned().collect();
        for h in hs {
            let pass = h.filters.iter().enumerate().all(|(k, f)| match (f, keys.get(k)) {
                (None, _) => true,
                // Sleep/wait's filter 0 is "both" [documented: SetJohnnyOnSleepWaitEventHandler].
                (Some(XVal::Num(z)), _) if ev == Ev::SleepWait && *z == 0.0 => true,
                (Some(f), Some(v)) => self.jg8_matches(f, v),
                (Some(_), None) => false,
            });
            if !pass {
                continue;
            }
            // NPCs' actor values go as whole numbers unless flag bit 0 asks for floats
            // [documented: SetOnNPCActorValueChangeEventHandler].
            let args = if ev == Ev::NpcActorValue && h.flag & 1 == 0 {
                args.iter().map(|a| if let XVal::Num(n) = a { XVal::Num(n.round()) } else { a.clone() }).collect()
            } else {
                args.clone()
            };
            if let Err(error) = self.run_function(&h.func, None, args, None, RunCtx::default()) {
                self.handler_failed("JohnnyGuitar event handler".into(), error);
            }
        }
    }

    /// The events of the game's trace this batch hears (`Game::emit` calls it before the
    /// event is kept and fires what it returns after): failed quests and reputation
    /// changes.
    pub(crate) fn jg8_from_event(&self, kind: &crate::trace::EventKind) -> Option<(Ev, Vec<XVal>, Vec<XVal>)> {
        use crate::trace::EventKind as K;
        match *kind {
            K::QuestFailed { quest } if self.jg8_listening(Ev::FailQuest) => Some((Ev::FailQuest, vec![XVal::Ref(quest.0)], vec![XVal::Ref(quest.0)])),
            // [observed 2026-11-24: w6t2p3_12_rep_fame_infamy, FNV 1.4.0.525 + JG 528:
            // callback receives (reputation, fame, infamy)]
            K::ReputationChanged { reputation, .. } if self.jg8_listening(Ev::Reputation) => {
                let [infamy, fame] = self.player.reputation.get(&reputation).copied().unwrap_or([0.0; 2]);
                Some((Ev::Reputation, vec![XVal::Ref(reputation.0)], vec![XVal::Ref(reputation.0), XVal::Num(fame), XVal::Num(infamy)]))
            }
            _ => None,
        }
    }

    /// An NPC's actor value changed (`functions::actor`): never the player's [documented:
    /// "will not trigger for the Player"].
    pub(crate) fn jg8_actor_value(&mut self, actor: FormId, code: u16, old: f64, new: f64) {
        if actor == FormId::PLAYER_REF {
            return;
        }
        let keys = [XVal::Ref(actor.0), XVal::Num(f64::from(code))];
        self.jg8_fire(Ev::NpcActorValue, &keys, vec![XVal::Ref(actor.0), XVal::Num(f64::from(code)), XVal::Num(old), XVal::Num(new)]);
    }

    /// A response of the player's conversation is said (`crate::dialogue`): its text, its
    /// voice file (when the host can name it, `voice::VoiceLengths::response_path`),
    /// emotion, emotion value and number; filtered by the emotion [documented].
    pub(crate) fn jg8_npc_response(&mut self, speaker: FormId, info: FormId, index: usize, resp: &crate::data::Response) {
        if !self.jg8_listening(Ev::NpcResponse) {
            return;
        }
        let base = self.speaker_voice_base(speaker);
        let path = self.voice.as_ref().and_then(|v| match self.voice_override(base) {
            Some(vt) => v.response_path_in_voice(info, index, vt),
            None => v.response_path(info, index, base),
        });
        let path = path.unwrap_or_default();
        let args = vec![XVal::Str(resp.text.clone()), XVal::Str(path), XVal::Num(f64::from(resp.emotion)), XVal::Num(f64::from(resp.emotion_value)), XVal::Num(f64::from(resp.number))];
        self.jg8_fire(Ev::NpcResponse, &[XVal::Num(f64::from(resp.emotion))], args);
    }

    /// The crosshair came onto `r` (the engine's activation reach, `nvf-play`
    /// activate.rs): filters the form and the base's form type code (e.g. `ACTI = 21`)
    /// [documented: SetJohnnyOnCrosshairEventHandler, Form Type IDs].
    pub fn jg8_crosshair(&mut self, r: FormId) {
        if !self.jg8_listening(Ev::Crosshair) {
            return;
        }
        let base = crate::functions::base_of(self, r).unwrap_or(r);
        let ty = self.data.sig(base).and_then(|s| fnv_script::vm::form_type_code(&s)).map_or(-1.0, f64::from);
        self.jg8_fire(Ev::Crosshair, &[XVal::Ref(r.0), XVal::Num(ty)], vec![XVal::Ref(r.0)]);
    }

    /// The settings menu closed after a change (the engine tells it).
    pub fn jg8_settings_closed(&mut self) {
        self.jg8_fire(Ev::SettingsUpdate, &[], Vec::new());
    }

    /// Keyboard or controller navigation moved a menu's selection (`ui_input::step`):
    /// fires `SetJohnnyOnKeyboardControllerSelectionChangeEventHandler` /
    /// `SetOnKBCTrlUIDeltaHandler` handlers with `menu_id` and filters by optional
    /// `MenuID` (`0` or omitted matches any menu) [hypothesis: menu ID argument as
    /// expected by menu UI scripts alongside SetOnMouseoverChangeEventHandler;
    /// observer check].
    pub fn jg8_keyboard_selection(&mut self, menu_id: f64) {
        self.jg8_fire(Ev::KeyboardSelection, &[XVal::Num(menu_id)], vec![XVal::Num(menu_id)]);
    }

    /// The loaded cells changed (`Game::set_loaded`): the cells seen for the first time,
    /// and the actors whose processing level went between 0 (the loaded area) and 3
    /// (the rest), as GetActorsByProcessingLevel reads them.
    pub(crate) fn jg8_loaded(&mut self, first_seen: &[FormId], before: Option<BTreeSet<FormId>>) {
        for &c in first_seen {
            self.jg8_fire(Ev::SeenData, &[XVal::Ref(c.0)], vec![XVal::Ref(c.0)]);
        }
        let Some(before) = before else { return };
        let after = self.package_actors();
        let changes: Vec<(FormId, f64, f64)> =
            after.difference(&before).map(|&a| (a, 3.0, 0.0)).chain(before.difference(&after).map(|&a| (a, 0.0, 3.0))).collect();
        for (a, old, new) in changes {
            self.jg8_fire(Ev::ProcessLevel, &[XVal::Ref(a.0), XVal::Num(new)], vec![XVal::Ref(a.0), XVal::Num(old), XVal::Num(new)]);
        }
    }

    /// The player's custom map marker (SetCustomMapMarker), for the Pip-Boy's map.
    pub fn custom_map_marker(&self) -> Option<(FormId, [f64; 3])> {
        self.other.jg8.custom_marker
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

fn text(args: &[XVal], k: usize) -> Option<String> {
    match args.get(k)? {
        XVal::Str(s) => Some(s.clone()),
        _ => None,
    }
}

fn yes(b: bool) -> XVal {
    XVal::Num(f64::from(u8::from(b)))
}

fn list(g: &mut Game, items: Vec<XVal>) -> XVal {
    let entries = items.into_iter().enumerate().map(|(i, v)| (XKey::Num(i as f64), v)).collect();
    XVal::Array(g.ext.new_array(ArrayKind::List, entries))
}

/// The return value and the outputs, for the VM to write into the script variables.
fn outputs(g: &mut Game, ret: f64, vals: &[f64]) -> XVal {
    list(g, std::iter::once(ret).chain(vals.iter().copied()).map(XVal::Num).collect())
}

/// A path that stays inside the game's folder: no drive, no leading slash, no `..`.
fn safe(path: &str) -> bool {
    !path.is_empty() && !path.starts_with(['\\', '/']) && !path.contains(':') && !path.split(['\\', '/']).any(|s| s == "..")
}

/// A file "relative to the Fallout folder" [documented: MD5File, SHA1File,
/// GetPixelFromBMP]: under `Data\` through NVForever's mod files (loose files and
/// archives), else the game folder above Data, read only.
fn game_file(path: &str) -> Option<Vec<u8>> {
    if !safe(path) {
        return None;
    }
    let lower = path.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("data\\").or_else(|| lower.strip_prefix("data/")) {
        return crate::mod_files::read_data_file(&path[path.len() - rest.len()..]);
    }
    let dir = crate::mod_files::data_dir()?;
    let p = dir.parent()?.join(path.replace('\\', "/"));
    std::fs::read(p).ok()
}

/// MD5 of `data` [documented: RFC 1321].
pub fn md5(data: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15,
        21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    let k: Vec<u32> = (0..64).map(|i| ((i as f64 + 1.0).sin().abs() * 4_294_967_296.0) as u32).collect();
    let mut h: [u32; 4] = [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476];
    let mut msg = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_le_bytes());
    for chunk in msg.chunks(64) {
        let m: Vec<u32> = chunk.chunks(4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]])).collect();
        let [mut a, mut b, mut c, mut d] = h;
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let f = f.wrapping_add(a).wrapping_add(k[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(S[i]));
        }
        h = [h[0].wrapping_add(a), h[1].wrapping_add(b), h[2].wrapping_add(c), h[3].wrapping_add(d)];
    }
    let mut out = [0u8; 16];
    for (i, w) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
    }
    out
}

/// SHA-1 of `data` [documented: RFC 3174].
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0];
    let mut msg = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for (i, c) in chunk.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i / 20 {
                0 => ((b & c) | (!b & d), 0x5A82_7999),
                1 => (b ^ c ^ d, 0x6ED9_EBA1),
                2 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        h = [h[0].wrapping_add(a), h[1].wrapping_add(b), h[2].wrapping_add(c), h[3].wrapping_add(d), h[4].wrapping_add(e)];
    }
    let mut out = [0u8; 20];
    for (i, w) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&w.to_be_bytes());
    }
    out
}

/// Upper-case hex, as the original prints MD5File and SHA1File [observed: probe set jg8,
/// 2026-11-09].
fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02X}")).collect()
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
}

/// A DDS file's header fields [documented: docs/spec-notes/dds.md §1, MS DDS]: height at
/// byte 12, width 16, mip-map count 28, the pixel format's FourCC at 84; `None` without
/// the magic.
pub fn dds_header(b: &[u8]) -> Option<(u32, u32, u32, [u8; 4])> {
    if b.get(..4)? != b"DDS " {
        return None;
    }
    let cc = b.get(84..88)?;
    Some((u32_at(b, 16)?, u32_at(b, 12)?, u32_at(b, 28)?, [cc[0], cc[1], cc[2], cc[3]]))
}

/// The pixel at (`x`, `y`) of a 24-bit BMP, counted from its top-left corner, as R, G, B
/// [documented: the BMP file header (pixel offset at byte 10) and BITMAPINFOHEADER
/// (width 18, height 22, bits per pixel 28; rows bottom-up for a positive height, each
/// padded to 4 bytes, blue first); hypothesis: the page's "width and height" are x and y
/// from the top-left].
pub fn bmp_pixel(b: &[u8], x: i64, y: i64) -> Option<[u8; 3]> {
    if b.get(..2)? != b"BM" || b.get(28..30)? != [24, 0] {
        return None;
    }
    let off = u32_at(b, 10)? as usize;
    let w = u32_at(b, 18)? as i32 as i64;
    let h = u32_at(b, 22)? as i32 as i64;
    if x < 0 || y < 0 || x >= w || y >= h.abs() {
        return None;
    }
    let row = (w * 3 + 3) / 4 * 4;
    let line = if h > 0 { h - 1 - y } else { y };
    let at = off + (line * row + x * 3) as usize;
    let p = b.get(at..at + 3)?;
    Some([p[2], p[1], p[0]])
}

/// The interface colours before a script sets one: amber for the HUD, Pip-Boy and main
/// menu, FF432A for the HUD's alternate colour, 21E779 for the terminal [observed: probe
/// set jg8, 2026-11-09, the original's answers on the owner's install; hypothesis: they
/// are the owner's interface colour settings, which NVForever does not read].
fn default_colour(id: i64) -> [u8; 3] {
    match id {
        2 => [0xFF, 0x43, 0x2A],
        3 => [0x21, 0xE7, 0x79],
        _ => [255, 182, 66],
    }
}

fn system_colour(g: &Game, id: i64) -> Option<[u8; 3]> {
    (1..=5).contains(&id).then(|| g.other.jip9.colours.get(&(id as u8)).copied().unwrap_or_else(|| default_colour(id)))
}

/// The JohnnyGuitar batch-8 functions; `None` for any other name.
pub(crate) fn call(g: &mut Game, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let zero = XVal::Num(0.0);
    let v = match name {
        // ---- event handlers: `set|remove function flag [filters…]` [documented: each
        // page]. Setting one again with the same function and filters replaces it.
        "setjohnnyonfailquesteventhandler"
        | "setonfailquesteventhandler"
        | "setjohnnyonlimbgoneeventhandler"
        | "setonlimbgoneeventhandler"
        | "setjohnnyseendataeventhandler"
        | "setseendataeventhandler"
        | "setonprocesslevelchangeeventhandler"
        | "setonnpcactorvaluechangeeventhandler"
        | "setonreputationchangeeventhandler"
        | "setonnpcresponseeventhandler"
        | "setjohnnyonsleepwaiteventhandler"
        | "setongeneralsubtitleeventhandler"
        | "setjohnnyoncrosshaireventhandler"
        | "setjohnnyonsettingsupdateeventhandler"
        | "setonsettingsupdateeventhandler"
        | "setjohnnyonkeyboardcontrollerselectionchangeeventhandler"
        | "setonkbctrluideltahandler"
        | "setjohnnyonradiopostsoundattacheventhandler"
        | "setontakebackitemeventhandler" => {
            let ev = match name.trim_start_matches("set").trim_start_matches("johnny").trim_start_matches("on") {
                "failquesteventhandler" => Ev::FailQuest,
                "limbgoneeventhandler" => Ev::LimbGone,
                "seendataeventhandler" => Ev::SeenData,
                "processlevelchangeeventhandler" => Ev::ProcessLevel,
                "npcactorvaluechangeeventhandler" => Ev::NpcActorValue,
                "reputationchangeeventhandler" => Ev::Reputation,
                "npcresponseeventhandler" => Ev::NpcResponse,
                "sleepwaiteventhandler" => Ev::SleepWait,
                "generalsubtitleeventhandler" => Ev::GeneralSubtitle,
                "crosshaireventhandler" => Ev::Crosshair,
                "settingsupdateeventhandler" => Ev::SettingsUpdate,
                "keyboardcontrollerselectionchangeeventhandler" | "kbctrluideltahandler" => Ev::KeyboardSelection,
                "radiopostsoundattacheventhandler" => Ev::RadioPostSound,
                _ => Ev::TakeBackItem,
            };
            let Some(func) = args.get(1).cloned().filter(|f| !matches!(f, XVal::Num(n) if *n == 0.0)) else { return Some(Ok(zero)) };
            // The filters that are numbers (a limb, a level, an actor value code, an
            // emotion, a form type, the sleep/wait mode) keep 0 as a value; a 0 or
            // XMarker (0x3B) where a form goes (or a menu ID <= 0 for KeyboardSelection)
            // is no filter. Live-observed ONLY for XMarker wildcard dispatch on
            // SetOnReputationChangeEventHandler (`xm=1`) and XMarker + `21`
            // registration/removal returning `0` on SetJohnnyOnCrosshairEventHandler
            // (`xh(0,0)`) [observed 2026-11-24: Fallout: New Vegas 1.4.0.525 + xNVSE
            // 6.4.9 + JohnnyGuitarNVSE 528 + JIP LN 57.30, 10 official base masters,
            // GSDocMitchellHouse (`0x00103DF9`), isolated clean-room staging copy,
            // committed probe `w6t2p2_12_jg_reputation_and_crosshair_xmarker`
            // (`tools/capture/nvse/catalogs/wave6c_jg8_xmarker_wildcard.json`):
            // `PROBE|w6t2p2_12_jg_reputation_and_crosshair_xmarker|xh(0,0)|fame(n=0,rOk=0,ax=0,v=0.0)|inf(n=1,ax=1.08423e+09,v=3.0)|xm=1`;
            // no DLL or source consulted]; applying XMarker wildcard to the remaining
            // unprobed handlers (FailQuest, LimbGone, SeenData, ProcessLevel,
            // NpcActorValue, GeneralSubtitle, RadioPostSound, TakeBackItem) and numeric 0
            // in a form slot are [hypothesis].
            const XMARKER: u32 = 0x0000_003B;
            let numeric: &[usize] = match ev {
                Ev::LimbGone | Ev::ProcessLevel | Ev::NpcActorValue | Ev::Crosshair => &[1],
                Ev::NpcResponse | Ev::SleepWait | Ev::KeyboardSelection => &[0],
                _ => &[],
            };
            let filters: Vec<Option<XVal>> = args
                .iter()
                .skip(3)
                .enumerate()
                .map(|(k, a)| match a {
                    XVal::Ref(0) => None,
                    XVal::Ref(XMARKER) if !numeric.contains(&k) => None,
                    XVal::Num(n) if !numeric.contains(&k) && *n == 0.0 => None,
                    XVal::Num(n) if ev == Ev::KeyboardSelection && *n <= 0.0 => None,
                    XVal::Num(n) if numeric.contains(&k) && ev != Ev::SleepWait && *n < 0.0 => None,
                    x => Some(x.clone()),
                })
                .collect();
            let same = |h: &Handler| h.ev == ev && h.func == func && h.filters == filters;
            g.other.jg8.handlers.retain(|h| !same(h));
            if num(args, 0, 1.0) != 0.0 {
                g.other.jg8.handlers.push(Handler { ev, func, flag: num(args, 2, 0.0) as i64, filters });
            }
            zero
        }
        // ---- the player's custom map marker: `x y z` in the player's worldspace (or
        // interior cell) [documented]; 1 when set [hypothesis: it fails only without a
        // place for the player]. ClearCustomMapMarker returns 1 even when no custom marker
        // was set [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE
        // 528, clean-room script probe jg_29_custom_map_marker_set_clear_ret: 1,1,1,1;
        // no DLL or source consulted].
        "setcustommapmarker" => {
            let space = g.cell_of(FormId::PLAYER_REF).map(|c| g.world.placed.world_of(c).unwrap_or(c));
            match space {
                Some(s) => {
                    g.other.jg8.custom_marker = Some((s, [num(args, 0, 0.0), num(args, 1, 0.0), num(args, 2, 0.0)]));
                    XVal::Num(1.0)
                }
                None => zero,
            }
        }
        "clearcustommapmarker" => {
            g.other.jg8.custom_marker = None;
            XVal::Num(1.0)
        }
        // ---- files [documented: MD5File, SHA1File; observed: upper-case hex (probe set
        // jg8); hypothesis: "" for a file that cannot be read].
        "md5file" => XVal::Str(text(args, 0).and_then(|p| game_file(&p)).map(|b| hex(&md5(&b))).unwrap_or_default()),
        "sha1file" => XVal::Str(text(args, 0).and_then(|p| game_file(&p)).map(|b| hex(&sha1(&b))).unwrap_or_default()),
        // `name`: an archive the game loaded, by its file name in Data, any case
        // [documented: IsBSALoaded; the host lists them, `mod_files::Roots::archives`;
        // observed: a name with `Data\` before it reads 0 (probe set jg8)].
        "isbsaloaded" => {
            let n = text(args, 0).unwrap_or_default();
            yes(!n.is_empty() && crate::mod_files::archives().iter().any(|a| a.eq_ignore_ascii_case(&n)))
        }
        // `path`, under Data\Textures [documented: GetTexture…]; 0 without the file
        // [observed 2026-11-24: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 +
        // JohnnyGuitarNVSE 528, 10 official base masters, `GSDocMitchellHouse`, committed
        // probe `w6t2_23_jg_dds_texture_format_and_bounds`
        // (`tools/capture/nvse/catalogs/wave6_t2_observed_matches.json`):
        // `dds(w=64,h=64,m=0,fmt=5,miss=0)` confirming DXT5 = 5 and missing file = 0].
        // GetTextureFormat: 1-5 for DXT1-DXT5 [documented], 0 for another format
        // [hypothesis].
        "gettexturewidth" | "gettextureheight" | "gettexturemipmapcount" | "gettextureformat" => {
            let p = text(args, 0).unwrap_or_default();
            let path = if p.to_ascii_lowercase().starts_with("textures\\") { p } else { format!("textures\\{p}") };
            let h = if safe(&path) { crate::mod_files::read_data_file(&path).as_deref().and_then(dds_header) } else { None };
            XVal::Num(h.map_or(0.0, |(w, ht, mips, cc)| match name {
                "gettexturewidth" => f64::from(w),
                "gettextureheight" => f64::from(ht),
                "gettexturemipmapcount" => f64::from(mips),
                _ => match &cc {
                    b"DXT1" => 1.0,
                    b"DXT2" => 2.0,
                    b"DXT3" => 3.0,
                    b"DXT4" => 4.0,
                    b"DXT5" => 5.0,
                    _ => 0.0,
                },
            }))
        }
        // `path rOut gOut bOut x y` (the VM passes path, x, y) [documented]; 0 0 0 when
        // the file or the pixel is not there [hypothesis].
        "getpixelfrombmp" => {
            let px = text(args, 0).and_then(|p| game_file(&p)).and_then(|b| bmp_pixel(&b, num(args, 1, 0.0) as i64, num(args, 2, 0.0) as i64));
            let c = px.unwrap_or([0; 3]);
            return Some(Ok(outputs(g, 0.0, &[f64::from(c[0]), f64::from(c[1]), f64::from(c[2])])));
        }
        // ---- arrays [documented: Ar_Shuffle, a new shuffled array; Ar_SortEditor, a new
        // array of the forms by editor ID, `reverse` the other way; observed: IDs compared
        // without case in `jg_30_ar_sort_editor_case_order` (`wave4a_extenders.json`),
        // see docs/spec-notes/ext-jg8.md §2; hypothesis: forms without an editor ID first].
        "ar_shuffle" => {
            let Some(XVal::Array(id)) = args.first() else { return Some(Ok(zero)) };
            let mut vals: Vec<XVal> = g.ext.array(*id).map(|a| a.entries.iter().map(|(_, v)| v.clone()).collect()).unwrap_or_default();
            for i in (1..vals.len()).rev() {
                let j = (g.rng.next_u32() as usize) % (i + 1);
                vals.swap(i, j);
            }
            list(g, vals)
        }
        "ar_sorteditor" => {
            let Some(XVal::Array(id)) = args.first() else { return Some(Ok(zero)) };
            let vals: Vec<XVal> = g.ext.array(*id).map(|a| a.entries.iter().map(|(_, v)| v.clone()).collect()).unwrap_or_default();
            let key = |v: &XVal| match v {
                XVal::Ref(r) => g.other.edids.get(&FormId(*r)).or_else(|| g.data.edid_of.get(&FormId(*r))).map(|s| s.to_ascii_lowercase()).unwrap_or_default(),
                _ => String::new(),
            };
            let mut keyed: Vec<(String, XVal)> = vals.into_iter().map(|v| (key(&v), v)).collect();
            keyed.sort_by(|a, b| a.0.cmp(&b.0));
            if num(args, 1, 0.0) != 0.0 {
                keyed.reverse();
            }
            list(g, keyed.into_iter().map(|(_, v)| v).collect())
        }
        // ---- the system colours: 1 HUD, 2 HUD alt, 3 terminal, 4 Pip-Boy, 5 main menu
        // [documented], SetSystemColor's (`nvse_jip9`) else [`default_colour`].
        // GetSystemColor packs them as 0xRRGGBB and returns 0 for another id
        // [observed: `jg_31_get_system_color_packed` (`wave4a_extenders.json`),
        // see docs/spec-notes/ext-jg8.md §2]; GetSystemColorAlt writes the three
        // variables for types 1..=5 and leaves them unchanged for other ids [observed:
        // Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528, 2026-11-24,
        // `w6_t1_08_jg_system_color_alt` (`tools/capture/nvse/catalogs/wave6_t1_jg_colors.json`,
        // `ext-jg8.md` §2)].
        "getsystemcolor" => XVal::Num(system_colour(g, num(args, 0, 0.0) as i64).map_or(0.0, |c| f64::from((u32::from(c[0]) << 16) | (u32::from(c[1]) << 8) | u32::from(c[2])))),
        "getsystemcoloralt" => {
            return Some(Ok(match system_colour(g, num(args, 0, 0.0) as i64) {
                Some(c) => outputs(g, 0.0, &[f64::from(c[0]), f64::from(c[1]), f64::from(c[2])]),
                None => outputs(g, 0.0, &[]),
            }));
        }
        // 0 closed, 1 wait, 2 sleep [documented: GetSleepWaitMenuState].
        "getsleepwaitmenustate" => XVal::Num(if !g.menu_open(crate::game::menus::SLEEP_WAIT) {
            0.0
        } else if g.other.jg8.sleep_menu {
            2.0
        } else {
            1.0
        }),
        // ---- radios: TuneRadioRef `[station]` kept for the reference [documented]; the
        // engine's radios do not read it yet. IsRadioRefPlaying: 0 [decision: the page
        // says it "is currently broken and will crash the game", so no script relies on
        // an answer].
        "tuneradioref" => {
            if let (Some(r), Some(s)) = (subject, form(args, 0)) {
                g.other.jg8.radio_tuned.insert(r, s);
            }
            zero
        }
        "isradiorefplaying" => zero,
        _ => return None,
    };
    Some(Ok(v))
}
