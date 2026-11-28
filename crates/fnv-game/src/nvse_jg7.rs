//! JohnnyGuitar NVSE, batch 7 (#1007): the switches and settings scripts give the game's
//! interface, music, saves, camera and HUD, the queued screen texts, the colour
//! conversions into script variables, JohnnyGuitar.ini's patches and nvse_config.ini's
//! heap size [documented: each page in the wiki export; docs/spec-notes/ext-jg7.md]. The
//! game keeps each value for the session; the engine parts that read them are listed in
//! the spec note.

use crate::game::{menus, Game, RunCtx};
use fnv_script::vm::{ArrayKind, XKey, XVal};
use fnv_sim::FormId;
use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};

/// ToggleCombatMusic 0: off "until the game is restarted" [documented: ToggleCombatMusic],
/// so for the process, not cleared by loading a save.
static COMBAT_MUSIC_OFF: AtomicBool = AtomicBool::new(false);

/// Whether a script turned combat music off (ToggleCombatMusic); the engine's music
/// (`nvf-play` music.rs) starts no battle music then.
pub fn combat_music_off() -> bool {
    COMBAT_MUSIC_OFF.load(Ordering::Relaxed)
}

/// The world map's icon names by map marker type (REFR XMRK TNAM, 0 None … 14 Vault), the
/// files `interface\icons\world map\icon_map_<name>.dds` [hypothesis: by the files'
/// names; type 0 takes the settlement's; observed: Goodsprings (type 3) reads
/// icon_map_encampment.dds, jg7.markericon]. The engine's world map draws the same
/// (`nvf-play` pipboy/data/model.rs).
pub const MAP_MARKER_ICONS: [&str; 15] =
    ["settlement", "city", "settlement", "encampment", "natural_landmark", "cave", "factory", "monument", "military", "office", "ruins_town", "ruins_urban", "ruins_sewer", "metro", "vault"];

/// A cinematic text QueueCinematicText asked for [documented: QueueCinematicText].
#[derive(Debug, Clone, PartialEq)]
pub struct CinematicText {
    pub title: String,
    pub subtitle: String,
    pub sound: Option<String>,
    /// 0 append, 1 prepend, 2 clear the queue and show at once.
    pub priority: i64,
    /// 0 left, 1 centre, 2 right.
    pub justification: i64,
    pub title_font: Option<i64>,
    pub subtitle_font: Option<i64>,
}

/// An objective text QueueObjectiveText asked for [documented: QueueObjectiveText].
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectiveText {
    pub text: String,
    pub completed: bool,
    pub multiple: bool,
}

/// What the engine is asked to do once (taken with [`Game::take_jg7_requests`]).
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// ExitGameAlt: back to the main menu without the credits.
    ExitToMainMenu,
    /// RollCredits.
    RollCredits,
    /// StopVATSCam.
    StopVatsCamera,
    /// SetCameraShake: multiplier, seconds.
    CameraShake(f64, f64),
    /// SetHUDShudderPower.
    HudShudder(f64),
}

/// The session's switches and settings of this batch.
#[derive(Debug, Default)]
pub struct Jg7State {
    /// ToggleLevelUpMenu 0 [documented: re-enabled when a save is loaded].
    pub level_up_menu_off: bool,
    /// DisableMenuArrowKeys.
    pub menu_arrow_keys_off: bool,
    /// SetYieldTimer: the seconds and the game time ([`Game::time`]) they were set at.
    pub yield_timer: Option<(f64, f64)>,
    /// SetViewmodelClipDistance (0: the default).
    pub viewmodel_clip: f64,
    /// SetNiLODLevel's global override; `None` reads -1, the default [observed:
    /// jg7 diagnostics, GetNiLODLevel -1 before any SetNiLODLevel].
    pub lod_level: Option<i64>,
    /// ToggleDisableSaves: the plugins (load-order index of the calling script's form)
    /// that disabled saving.
    pub saves_disabled_by: BTreeSet<u8>,
    /// SetUIUpdateSound: 1 quest failed, 2 quest added, 3 quest completed, 4 location
    /// discovered.
    pub ui_sounds: HashMap<i64, FormId>,
    /// SetCustomReputationChangeIcon: (reputation, tier 1-4) → icon path.
    pub reputation_icons: HashMap<(FormId, i64), String>,
    /// SetDisablePlayerControlsHUDVisibilityFlags' bitmask.
    pub dpc_hud_flags: Option<u32>,
    /// ToggleNthPipboyLight: the Stats, Items and Data button lights.
    pub pipboy_lights: [bool; 3],
    /// PushUIQuestToTop's quest.
    pub top_quest: Option<FormId>,
    /// HideItemBarterEx: (item, vendor) → the flag IsItemBarterHiddenEx reads (1 shown
    /// on the player's side, 2 on the vendor's, 3 hidden on both).
    pub barter_hidden: HashMap<(FormId, Option<FormId>), i64>,
    /// The texts queued for the HUD.
    pub cinematic: Vec<CinematicText>,
    pub objectives: Vec<ObjectiveText>,
    /// SetCustomMapMarkerIcon's paths, by map marker reference.
    pub marker_icons: HashMap<FormId, String>,
    pub requests: Vec<Request>,
}

impl Game<'_> {
    /// The engine's one-time requests of this batch since the last call.
    pub fn take_jg7_requests(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.other.jg7.requests)
    }

    /// HideItemBarterEx's flag for `item` at `vendor`: 0 shown, 1 shown on the player's
    /// side only, 2 on the vendor's only, 3 hidden on both [documented]; the vendor
    /// reference's own entry first, then its base form's, then the one for every vendor
    /// [hypothesis].
    pub fn barter_hide_flag(&self, item: FormId, vendor: Option<FormId>) -> i64 {
        let h = &self.other.jg7.barter_hidden;
        let vendor_base = vendor.and_then(|v| self.world.placed.get(v).map(|p| p.base));
        h.get(&(item, vendor))
            .or_else(|| vendor_base.and_then(|b| h.get(&(item, Some(b)))))
            .or_else(|| h.get(&(item, None)))
            .copied()
            .unwrap_or(0)
    }

    /// Active non-zero `HideItemBarterEx` flags for a barter session with `vendor`.
    pub fn barter_hidden_for(&self, vendor: FormId) -> std::collections::BTreeMap<FormId, i64> {
        let mut out = std::collections::BTreeMap::new();
        for &(item, _) in self.other.jg7.barter_hidden.keys() {
            if out.contains_key(&item) {
                continue;
            }
            let flag = self.barter_hide_flag(item, Some(vendor));
            if flag != 0 {
                out.insert(item, flag);
            }
        }
        out
    }

    /// Whether a script has disabled saving (ToggleDisableSaves) [documented].
    pub fn saves_disabled(&self) -> bool {
        !self.other.jg7.saves_disabled_by.is_empty()
    }

    /// Whether the level-up menu is enabled (`ToggleLevelUpMenu`, on by default) [documented].
    pub fn level_up_menu_enabled(&self) -> bool {
        !self.other.jg7.level_up_menu_off
    }

    /// The `SOUN` FormID set by `SetUIUpdateSound` for notification `kind` (`1` quest
    /// failed, `2` quest added, `3` quest completed, `4` location discovered) [documented].
    pub fn ui_update_sound(&self, kind: i64) -> Option<FormId> {
        self.other.jg7.ui_sounds.get(&kind).copied()
    }

    /// The quest pushed to the top of the Pip-Boy quest list (`PushUIQuestToTop`) [documented].
    pub fn top_ui_quest(&self) -> Option<FormId> {
        self.other.jg7.top_quest
    }

    /// The custom map marker texture path set for `r` by `SetCustomMapMarkerIcon`,
    /// if any (non-empty) [documented: SetCustomMapMarkerIcon].
    pub fn custom_map_marker_icon(&self, r: FormId) -> Option<&str> {
        self.other.jg7.marker_icons.get(&r).map(String::as_str).filter(|p| !p.trim().is_empty())
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

fn int(args: &[XVal], k: usize) -> Option<i64> {
    match args.get(k) {
        Some(XVal::Num(x)) if x.fract() == 0.0 => Some(*x as i64),
        _ => None,
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

/// The return value and the outputs, for the VM to write into the script variables
/// (`fnv_script::vm::out_params`).
fn outputs(g: &mut Game, ret: f64, vals: &[f64]) -> XVal {
    let entries = std::iter::once(ret).chain(vals.iter().copied()).enumerate().map(|(i, v)| (XKey::Num(i as f64), XVal::Num(v))).collect();
    XVal::Array(g.ext.new_array(ArrayKind::List, entries))
}

/// Hue (degrees), saturation and value (0-1) from 0-255 channels without input clamping
/// [documented: RGBtoHSV; observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 +
/// JohnnyGuitarNVSE 528, 2026-11-24, `w6_t1_07_jg_hsv_rgb`
/// (`tools/capture/nvse/catalogs/wave6_t1_jg_colors.json`, `ext-jg7.md`):
/// `(300, -10, 128) -> (333.290, 1.033, 1.176)`].
pub fn rgb_to_hsv(r: f64, g: f64, b: f64) -> [f64; 3] {
    let (r, g, b) = (r / 255.0, g / 255.0, b / 255.0);
    let max = r.max(g).max(b);
    let d = max - r.min(g).min(b);
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    [h, if max == 0.0 { 0.0 } else { d / max }, max]
}

/// 0-255 channels from hue, saturation and value, rounded to whole channels [documented:
/// HSVtoRGB; observed: same `w6_t1_07_jg_hsv_rgb` probe — `(30, 0.5, 0.5) -> (128, 96, 64)`].
pub fn hsv_to_rgb(h: f64, s: f64, v: f64) -> [f64; 3] {
    let (h, s, v) = (h.rem_euclid(360.0), s.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [((r + m) * 255.0).round(), ((g + m) * 255.0).round(), ((b + m) * 255.0).round()]
}

/// A `key = value` of an INI file's section, case-insensitively.
/// A Data text file through the merged Data: NVForever's layer, then the game's Data and
/// the host's index (a Mod Organizer 2 merge's mod folders).
fn data_text(rel: &str) -> Option<String> {
    crate::mod_files::read_data_file(rel).map(|b| String::from_utf8_lossy(&b).into_owned())
}

fn ini_text_value(text: &str, section: &str, key: &str) -> Option<String> {
    let mut here = false;
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            here = l.trim_matches(|c| c == '[' || c == ']').eq_ignore_ascii_case(section);
            continue;
        }
        if !here || l.starts_with(';') || l.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = l.split_once('=') {
            if k.trim().eq_ignore_ascii_case(key) {
                return Some(v.split(';').next().unwrap_or("").trim().to_string());
            }
        }
    }
    None
}

/// The JohnnyGuitar batch-7 functions; `None` for any other name.
pub(crate) fn call(g: &mut Game, ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let zero = XVal::Num(0.0);

    let v = match name {
        // ---- switches ----
        "togglecombatmusic" => {
            COMBAT_MUSIC_OFF.store(num(args, 0, 1.0) == 0.0, Ordering::Relaxed);
            zero
        }
        "iscombatmusicenabled" => yes(!combat_music_off()),
        "togglelevelupmenu" => {
            g.other.jg7.level_up_menu_off = num(args, 0, 1.0) == 0.0;
            zero
        }
        "islevelupmenuenabled" => yes(!g.other.jg7.level_up_menu_off),
        "disablemenuarrowkeys" | "enablemenuarrowkeys" => {
            g.other.jg7.menu_arrow_keys_off = name == "disablemenuarrowkeys";
            zero
        }
        // `seconds`: counts down in game seconds from when it was set; 0 when run out or
        // never set [documented: Get/SetYieldTimer; hypothesis: game time].
        "setyieldtimer" => {
            let now = g.time;
            g.other.jg7.yield_timer = Some((num(args, 0, 0.0), now));
            zero
        }
        "getyieldtimer" => XVal::Num(g.other.jg7.yield_timer.map_or(0.0, |(v, at)| (v - (g.time - at)).max(0.0))),
        // 0 is the default [documented: Get/SetViewmodelClipDistance]; stores raw finite
        // values including negatives without clamping to 0 [observed: Fallout: New Vegas
        // 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528, 2026-11-24, `GSDocMitchellHouse`
        // (`0x00103DF9`), 10 official base masters, `jg_25_yield_timer_and_viewmodel_clip_neg`
        // (`-5.00`) and `w4c_b3_05_jg7_bounds_heap` (`vc(0=0.0,neg=-5.0)`)].
        "setviewmodelclipdistance" => {
            let v = num(args, 0, 0.0);
            if v.is_finite() {
                g.other.jg7.viewmodel_clip = v;
            }
            zero
        }
        "getviewmodelclipdistance" => XVal::Num(g.other.jg7.viewmodel_clip),
        "setnilodlevel" => {
            g.other.jg7.lod_level = int(args, 0);
            zero
        }
        "getnilodlevel" => XVal::Num(g.other.jg7.lod_level.unwrap_or(-1) as f64),
        // `on`: per plugin, the plugin of the calling script's form (its reference or
        // quest) [documented: ToggleDisableSaves, "stored on a per-mod basis"; hypothesis:
        // the form's load-order index names the mod]. Loading a save clears it.
        "toggledisablesaves" => {
            let owner = ctx.quest.or(ctx.self_ref).map_or(0xFF, |f| (f.0 >> 24) as u8);
            if num(args, 0, 0.0) != 0.0 {
                g.other.jg7.saves_disabled_by.insert(owner);
            } else {
                g.other.jg7.saves_disabled_by.remove(&owner);
            }
            zero
        }
        // ---- the interface ----
        // `sound type`: 1 quest failed, 2 quest added, 3 quest completed, 4 location
        // discovered [documented: SetUIUpdateSound]; another type or a form that is no
        // SOUN changes nothing [hypothesis].
        "setuiupdatesound" => {
            let t = int(args, 1).filter(|t| (1..=4).contains(t));
            let snd = form(args, 0).filter(|&f| g.data.sig(f) == Some(*b"SOUN"));
            if let (Some(t), Some(snd)) = (t, snd) {
                g.other.jg7.ui_sounds.insert(t, snd);
            }
            zero
        }
        // `reputation tier path`: 1 when set for tiers 1 and 4 on a `REPU`, and 0 for tiers
        // 0 and 5 or a non-`REPU` form [documented: SetCustomReputationChangeIcon; observed:
        // `b3_jg7_01_ui_sound_and_rep_icon_bounds` (`wave4b_extenders.json`) and
        // `w4c_b3_05_jg7_bounds_heap` (`wave4c_extenders.json`), see
        // docs/spec-notes/ext-jg7.md §1]; tiers 2 and 3 (and other out-of-range tiers beyond
        // 0 and 5) remain [hypothesis].
        "setcustomreputationchangeicon" => {
            let (r, t, p) = (form(args, 0).filter(|&f| g.data.sig(f) == Some(*b"REPU")), int(args, 1).filter(|t| (1..=4).contains(t)), text(args, 2));
            match (r, t, p) {
                (Some(r), Some(t), Some(p)) => {
                    g.other.jg7.reputation_icons.insert((r, t), p);
                    XVal::Num(1.0)
                }
                _ => zero,
            }
        }
        "setdisableplayercontrolshudvisibilityflags" => {
            g.other.jg7.dpc_hud_flags = Some(num(args, 0, 0.0).max(0.0) as u32);
            zero
        }
        // `button on`: 0 Stats, 1 Items, 2 Data [documented: ToggleNthPipboyLight].
        "togglenthpipboylight" => {
            if let Some(b) = int(args, 0).filter(|b| (0..3).contains(b)) {
                g.other.jg7.pipboy_lights[b as usize] = num(args, 1, 0.0) != 0.0;
            }
            zero
        }
        "pushuiquesttotop" => {
            let q = form(args, 0).filter(|&f| g.data.sig(f) == Some(*b"QUST"));
            if q.is_some() {
                g.other.jg7.top_quest = q;
            }
            zero
        }
        // `item hide [side] [vendor]`: hide 0 shows it again; side 1 keeps it on the
        // player's side, 2 on the vendor's, none hides it on both [documented:
        // HideItemBarterEx]. IsItemBarterHiddenEx reads 0 shown, 1 shown on the player's
        // side, 2 on the vendor's, 3 hidden on both [documented: its notes]; a vendor's
        // own entry first, then the one for every vendor [hypothesis]; the barter menu is
        // to read it through [`Game::barter_hide_flag`]. But IsItemBarterHiddenEx read 0
        // after every HideItemBarterEx in the original, on the caps and on the 10mm pistol,
        // with and without a vendor [observed 2026-11-08: jg7.barter, jg7.barter.pistol,
        // outside the barter menu], so the script function reads 0.
        "hideitembarterex" => {
            let Some(item) = form(args, 0) else { return Some(Ok(zero)) };
            let vendor = form(args, 3);
            if num(args, 1, 0.0) == 0.0 {
                g.other.jg7.barter_hidden.remove(&(item, vendor));
            } else {
                let flag = match int(args, 2) {
                    Some(1) => 1,
                    Some(2) => 2,
                    _ => 3,
                };
                g.other.jg7.barter_hidden.insert((item, vendor), flag);
            }
            zero
        }
        "isitembarterhiddenex" => zero,
        // `title subtitle [sound] [priority] [justification] [title font] [subtitle font]`
        // [documented: QueueCinematicText]; 2 clears what waits.
        "queuecinematictext" => {
            let t = CinematicText {
                title: text(args, 0).unwrap_or_default(),
                subtitle: text(args, 1).unwrap_or_default(),
                sound: text(args, 2).filter(|x| !x.is_empty()),
                priority: int(args, 3).unwrap_or(0),
                justification: int(args, 4).unwrap_or(0),
                title_font: int(args, 5),
                subtitle_font: int(args, 6),
            };
            match t.priority {
                1 => g.other.jg7.cinematic.insert(0, t),
                2 => {
                    g.other.jg7.cinematic.clear();
                    g.other.jg7.cinematic.push(t);
                }
                _ => g.other.jg7.cinematic.push(t),
            }
            zero
        }
        "queueobjectivetext" => {
            g.other.jg7.objectives.push(ObjectiveText { text: text(args, 0).unwrap_or_default(), completed: num(args, 1, 0.0) != 0.0, multiple: num(args, 2, 0.0) != 0.0 });
            zero
        }
        // `[tab]`: opens the Pip-Boy on 1002 Inventory, 1003 Stats, 1023 Data, else where
        // it closed (the inventory here), or closes it when it is open [documented:
        // TogglePipBoy; hypothesis: the tab it last closed on is not kept, the inventory
        // stands for it].
        "togglepipboy" => {
            if g.menu_open(menus::PIPBOY) {
                g.close_menu();
            } else {
                let tab = match int(args, 0) {
                    Some(1003) => menus::STATS,
                    Some(1023) => menus::MAP,
                    _ => menus::INVENTORY,
                };
                g.open_menu(tab);
            }
            zero
        }
        "exitgamealt" => {
            g.other.jg7.requests.push(Request::ExitToMainMenu);
            zero
        }
        "rollcredits" => {
            g.other.jg7.requests.push(Request::RollCredits);
            zero
        }
        "stopvatscam" => {
            g.other.jg7.requests.push(Request::StopVatsCamera);
            zero
        }
        "setcamerashake" => {
            g.other.jg7.requests.push(Request::CameraShake(num(args, 0, 1.0), num(args, 1, 0.0).max(0.0)));
            zero
        }
        "sethudshudderpower" => {
            g.other.jg7.requests.push(Request::HudShudder(num(args, 0, 0.0)));
            zero
        }
        // `marker|list path`: a map marker reference, or each one of a form list
        // [documented: SetCustomMapMarkerIcon], for the session. An empty path clears the
        // override back to the marker's type icon [hypothesis].
        "setcustommapmarkericon" => {
            let (Some(f), Some(p)) = (form(args, 0), text(args, 1)) else { return Some(Ok(zero)) };
            let list = crate::functions::list_members(g, f);
            let markers = if list.is_empty() { vec![f] } else { list };
            for m in markers {
                if p.trim().is_empty() {
                    g.other.jg7.marker_icons.remove(&m);
                } else {
                    g.other.jg7.marker_icons.insert(m, p.clone());
                }
            }
            zero
        }
        // The path SetCustomMapMarkerIcon gave the calling marker, else its type's icon
        // file [documented: GetCustomMapMarkerIcon, "or the default texture path";
        // observed 2026-11-08: the Goodsprings marker read
        // `Interface\Icons\World Map\icon_map_encampment.dds`, spelled so]; "" for a
        // reference that is no map marker [hypothesis].
        "getcustommapmarkericon" => {
            let Some(r) = subject else { return Some(Ok(XVal::Str(String::new()))) };
            let p = g.custom_map_marker_icon(r).map(str::to_string).or_else(|| {
                let kind = g.marker_kind(r)?;
                let name = MAP_MARKER_ICONS.get(kind as usize).copied().unwrap_or("settlement");
                Some(format!("Interface\\Icons\\World Map\\icon_map_{name}.dds"))
            });
            XVal::Str(p.unwrap_or_default())
        }
        // [documented: GetLocationSpecificLoadScreensOnly, the switch
        // SetLocationSpecificLoadScreensOnly sets].
        "getlocationspecificloadscreensonly" => yes(g.player.location_load_screens_only),
        // NVForever has no Real Time Menus, so every menu is paused: 1 for a menu code,
        // and without one 1 while in menu mode [documented: IsMenuPaused, "if Real Time
        // Menus is not installed"].
        "ismenupaused" => match int(args, 0) {
            Some(c) if c != 0 => XVal::Num(1.0),
            _ => yes(g.in_menu() || g.loading),
        },
        // ---- colours into script variables ----
        // `h s v rOut gOut bOut` [documented: HSVtoRGB]; `r g b hOut sOut vOut`
        // [documented: RGBtoHSV].
        "hsvtorgb" => {
            let c = hsv_to_rgb(num(args, 0, 0.0), num(args, 1, 0.0), num(args, 2, 0.0));
            return Some(Ok(outputs(g, 0.0, &c)));
        }
        "rgbtohsv" => {
            let c = rgb_to_hsv(num(args, 0, 0.0), num(args, 1, 0.0), num(args, 2, 0.0));
            return Some(Ok(outputs(g, 0.0, &c)));
        }
        // ---- files of the extender's own ----
        // `patch`: 1 when JohnnyGuitar.ini's patch is on: 1 bLoadEditorIDs (always 1), 3
        // bFixFleeing, 4 bFixItemStackCount, 5 bFixNPCShootingAngle, 6
        // bNoMuzzleFlashCooldown, 7 bReset3rdPersonCamera, 8 bJIPFixes [documented:
        // GetJohnnyPatch]; read from Data\NVSE\Plugins\JohnnyGuitar.ini, section MAIN; 0
        // without the file or for another ID [hypothesis].
        "getjohnnypatch" => {
            let key = match int(args, 0) {
                Some(1) => return Some(Ok(XVal::Num(1.0))),
                Some(3) => "bFixFleeing",
                Some(4) => "bFixItemStackCount",
                Some(5) => "bFixNPCShootingAngle",
                Some(6) => "bNoMuzzleFlashCooldown",
                Some(7) => "bReset3rdPersonCamera",
                Some(8) => "bJIPFixes",
                _ => return Some(Ok(zero)),
            };
            // Through the merged Data (a Mod Organizer 2 mod folder holds it) [observed:
            // Nuclear Sunset's JohnnyGuitar.ini, `bJIPFixes = 1`, sits only in its mod
            // folders; read from the stock Data, MCM Extender showed "Please enable the
            // following JohnnyGuitar patches: > JIP Fixes: 0 -> 1", log
            // nvforever-20261125-154352-15152.log].
            let text = data_text("NVSE\\Plugins\\JohnnyGuitar.ini");
            let on = text.and_then(|t| ini_text_value(&t, "MAIN", key)).and_then(|v| v.parse::<f64>().ok()).is_some_and(|v| v != 0.0);
            yes(on)
        }
        // DefaultHeapInitialAllocMB of Data\NVSE\nvse_config.ini [documented:
        // GetDefaultHeapSize], any section; 200 (xNVSE default) when absent [observed:
        // Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528, 2026-11-24,
        // `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters,
        // `w4c_b3_05_jg7_bounds_heap`: `heap=200`].
        "getdefaultheapsize" => {
            let text = data_text("NVSE\\nvse_config.ini").unwrap_or_default();
            let v = text.lines().filter_map(|l| l.split_once('=')).find(|(k, _)| k.trim().eq_ignore_ascii_case("DefaultHeapInitialAllocMB")).and_then(|(_, v)| v.trim().parse::<f64>().ok());
            XVal::Num(v.unwrap_or(200.0))
        }
        // ---- nothing values ----
        // A debugger breakpoint [documented: AsmBreak]: nothing [decision: NVForever
        // runs no native debugger hooks for scripts].
        "asmbreak" => zero,
        // A memory address [documented: RefAddr]: 0 [decision: NVForever's forms have no
        // addresses a script could use].
        "refaddr" => zero,
        _ => return None,
    };
    Some(Ok(v))
}
