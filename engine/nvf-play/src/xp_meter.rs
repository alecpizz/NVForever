//! The HUD's XP meter (`XPMeter` of `menus\main\hud_main_menu.xml`;
//! docs/spec-notes/experience.md §4): when the player earns experience, a bar on the
//! right just above the action points with the level the player has at its left end, the next
//! level at its right end, a pointer over it at how far the player has come between the
//! two, and "+n XP" under its right end; "LEVEL UP" when the next level is reached. It is
//! not shown at the level cap.
//!
//! - [`Meter`] is the model (pure, tested): it watches the player's experience and level
//!   ([`fnv_game::experience::Standing`]) and says what is on the screen.
//! - Any rise of the experience shows the meter, whatever gave it (kills, locks, places
//!   found, `RewardXP`): the model compares with what it saw last. A fall (`ResetXP`) or
//!   a new level only resets what it saw.
//! - When the next level is earned, the bar's "LEVEL UP" plays `UILevelUpText` and the
//!   level-up menu opens [`LEVEL_UP_DELAY`] seconds later, once the player is also in the
//!   world, out of every menu and hunted by nobody ([`fnv_game::experience::level_up`]);
//!   one level at a time, the next (sound and wait again) after the menu closes.
//! - Logged: `xp meter: +<n> XP (<xp> in all): level <l>, <p> % to <l+1>`,
//!   `xp meter: LEVEL UP (<l> -> <l+1>)`, `xp meter: +<n> XP at the level cap <max>: not
//!   shown`, `xp: level up taken: level <l> (cap <max>)`, and the cap once:
//!   `xp: level cap <max> (<k> story add-ons)`.

use crate::hud::HudView;
use crate::interface::{amber, at, bc_supported, face, plain_font, set_text, texture, u, Face, Set};
use crate::script::{with_world, ScriptState};
use crate::sfx::{PlaySound, Sfx};
use crate::sim::SimState;
use crate::tiles::vanilla::MeterRoot;
use crate::{PlaySet, PlayState};
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use fnv_font::Font;
use fnv_game::experience::{self, Standing};
use fnv_sim::FormId;
use nvf_assets::GameData;
use std::collections::HashMap;

// ---- the menu file's numbers [documented: hud_main_menu.xml `XPMeter`] -----------------

/// The bracket picture `glow_XP_bar_over.dds`, 512 x 256.
const BRACKET: Vec2 = Vec2::new(512.0, 256.0);
const BRACKET_PATH: &str = "textures\\interface\\hud\\glow_xp_bar_over.dds";
/// The pointer `glow_XP_bar_pointer.dds`, 64 square, its x from `_x_min` (no experience
/// towards the next level) to `_x_max` (all of it).
const POINTER: f32 = 64.0;
const POINTER_PATH: &str = "textures\\interface\\hud\\glow_xp_bar_pointer.dds";
const POINTER_X: (f32, f32) = (55.0, 270.0);
/// `XPLastLevel` ends at x 70 (right-justified), `XPNextLevel` starts at 315, both in the
/// menus' default font 1; `XPAmount` ends at 315 in font 8, `XPLabel` ("XP") at 350 in
/// font 7, `XPLevelUp` ("LEVEL UP") at 360 in font 8.
const LAST_LEVEL_END: f32 = 70.0;
const NEXT_LEVEL_X: f32 = 315.0;
const AMOUNT_END: f32 = 315.0;
const LABEL_END: f32 = 350.0;
const LEVEL_UP_END: f32 = 360.0;
const LEVEL_FONT: &str = "textures\\fonts\\glow_monofonto_large.fnt";
const LABEL_FONT: &str = "textures\\fonts\\baked-in_monofonto_large.fnt";
const AMOUNT_FONT: &str = "textures\\fonts\\glow_futura_caps_large.fnt";
/// The words: `sStatsXP` and `sLevelUp`, which the masters do not set [observed: not
/// among FalloutNV.esm's GMST; the words: hud.md §14.2, XPLabel and XPLevelUp].
pub(crate) const XP_WORD: &str = "XP";
pub(crate) const LEVEL_UP_WORD: &str = "LEVEL UP";

// ---- what the pictures and the owner's screenshot show [observed] ----------------------

/// The bracket's drawn part: the long line (rows 64–72) from x 7, the nine ticks centred
/// 86.5 … 302.5, the upright at 364–367 from row 7 to 129 [observed: the texture].
const PIECE: Vec2 = Vec2::new(368.0, 130.0);
const LINE_MIDDLE: f32 = 68.0;
/// The pointer's tip stands on the ticks' tops (row 47), not among them; the picture's
/// triangle has its tip at row 39 [owner, 2026-10-20: "at the top of each notch"; observed:
/// the texture].
const POINTER_Y: f32 = 46.0 - 39.0;
/// The lines the texts stand on [observed: the owner's screenshot, as above]: the levels
/// 49, "+54" 118, "XP" 103.
const LEVEL_BASE: f32 = 49.0;
const AMOUNT_BASE: f32 = 118.0;
const LABEL_BASE: f32 = 103.0;
/// "LEVEL UP" stands 16 above the amount's line [observed: hud_compare xp_levelup_menu at
/// 1280 x 720, the original's LEVEL UP from y 535 pixels, this HUD's from 547 on the
/// amount's line (xp_level_up); documented: hud_main_menu.xml puts `XPLevelUp` at y 70,
/// 20 above `XPAmount`'s 90]. Until T12e on the amount's line [design].
const LEVEL_UP_BASE: f32 = AMOUNT_BASE - 16.0;

/// The meter's place: from the view's right edge to the upright, and from the view's
/// bottom edge to the bracket's bottom (25 units over the action points' piece, the line
/// at 244) [observed: the owner's full-screen screenshot at 1920 x 1080, 1.125 pixels to
/// the unit: the upright at x 1862, the line at y 805].
const MARGIN_X: f32 = 52.0;
const BOTTOM: f32 = 182.0;

// ---- timing [observed: private job hud_xp_limbs2, x9_level_up_long] ---------------------

/// Meter: about 2 s in, 2 s full (pointer moving), 2 s out. Previously the general
/// HUD fade 0.25 s, hold 3 s, immediate eased slide 0.6 s [hypothesis].
const FADE: f32 = 2.0;
const HOLD: f32 = 2.0;
const SLIDE: f32 = 2.0;
const METER_SECONDS: f32 = FADE + HOLD + FADE;
/// LEVEL UP alone starts about 7.2 s after the gain, after the meter is gone
/// [observed: private job hud_xp_limbs2, x9_level_up_long]; 1.2 s gap fitted to that
/// sampling [hypothesis]. Previously replaced the amount during the meter [hypothesis].
const LEVEL_UP_START: f32 = METER_SECONDS + 1.2;
/// Up within 0.34 s, full about 1.3 s, then down [observed: private job
/// hud_xp_limbs2, x9_level_up_long]; symmetric 0.34 s down [hypothesis].
const LEVEL_UP_FADE: f32 = 0.34;
const LEVEL_UP_HOLD: f32 = 1.3;
const LEVEL_UP_SECONDS: f32 = 2.0 * LEVEL_UP_FADE + LEVEL_UP_HOLD;
/// From the filled-bar level-up sound (`FADE + SLIDE = 4.0 s`) to the level-up menu,
/// counted while the HUD is up [observed: private job hud_xp_limbs2, x9_level_up_long:
/// original menu opened at 9.35 s after the gain; design: #3438 brings `4.0 + 5.35 =
/// 9.35 s` in line with that observation while keeping the owner's 2026-10-24 filled-bar
/// sound trigger and letting the 9.18 s LEVEL UP fade finish cleanly].
pub const LEVEL_UP_DELAY: f32 = 5.35;
/// The level-up sound, when the bar is filled [owner, 2026-10-24: "when the xp bar is
/// filled it should play the ui_leveluptext.wav"; observed: SOUN `UILevelUpText` names it].
const LEVEL_UP_SOUND: &str = "UILevelUpText";

/// What the meter shows.
#[derive(Debug, Clone, PartialEq)]
pub struct Show {
    pub level: i32,
    /// The experience gained since the meter came up.
    pub amount: f64,
    /// The pointer moves from `from` to `to` (0 to 1 between the two levels).
    pub from: f32,
    pub to: f32,
    /// The next level is earned: "LEVEL UP" after the meter fades away.
    pub level_up: bool,
    /// Seconds since it came up, and since the last gain.
    pub age: f32,
    pub since_gain: f32,
}

/// What [`Meter::observe`] saw happen.
#[derive(Debug, Clone, PartialEq)]
pub enum Seen {
    /// Experience gained below the cap: the meter shows it.
    Gained { amount: f64, level_up: bool },
    /// Experience gained at the cap: not shown.
    AtCap { amount: f64 },
}

#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct Meter {
    /// The experience and level seen last.
    seen: Option<(f64, i32)>,
    pub show: Option<Show>,
    /// "LEVEL UP" has been put up for this show (its sound plays once).
    level_up_shown: bool,
}

impl Meter {
    /// Compare the player's standing with what was seen last.
    pub fn observe(&mut self, now: &Standing) -> Option<Seen> {
        let before = self.seen.replace((now.xp, now.level));
        let (xp, level) = before?;
        if now.xp <= xp {
            if now.level != level {
                self.show = None;
            }
            return None;
        }
        let amount = now.xp - xp;
        if now.at_max() {
            self.show = None;
            return Some(Seen::AtCap { amount });
        }
        let level_up = now.due();
        let pointer = self.pointer();
        match &mut self.show {
            Some(s) if s.level == now.level => {
                // On from where the pointer is.
                s.from = pointer;
                s.amount += amount;
                s.to = now.fraction();
                s.level_up = level_up;
                s.since_gain = 0.0;
                // Back to full strength if it was fading out.
                s.age = s.age.min(FADE);
            }
            _ => {
                let from = if level != now.level { 0.0 } else { now.fraction_of(xp) };
                self.show = Some(Show { level: now.level, amount, from, to: now.fraction(), level_up, age: 0.0, since_gain: 0.0 });
                self.level_up_shown = false;
            }
        }
        Some(Seen::Gained { amount, level_up })
    }

    /// Time passes while the meter is on the screen. True when "LEVEL UP" comes up.
    pub fn step(&mut self, dt: f32) -> bool {
        let Some(s) = &mut self.show else { return false };
        s.age += dt;
        s.since_gain += dt;
        let end = if s.level_up { LEVEL_UP_START + LEVEL_UP_SECONDS } else { METER_SECONDS };
        if s.since_gain >= end {
            self.show = None;
            return false;
        }
        if self.level_up_word() && !self.level_up_shown {
            self.level_up_shown = true;
            return true;
        }
        false
    }

    /// The pointer's place now, 0 to 1.
    pub fn pointer(&self) -> f32 {
        self.show.as_ref().map_or(0.0, |s| {
            // Glide only at full alpha [observed: private job hud_xp_limbs2,
            // x9_level_up_long]; linear fit to pointer samples [hypothesis].
            let t = ((s.since_gain - FADE) / SLIDE).clamp(0.0, 1.0);
            s.from + (s.to - s.from) * t
        })
    }

    /// How strong the meter is drawn, 0 to 1: fading in, full, fading out.
    pub fn strength(&self) -> f32 {
        self.show.as_ref().map_or(0.0, |s| {
            if self.level_up_word() {
                let age = s.since_gain - LEVEL_UP_START;
                (age / LEVEL_UP_FADE).min((LEVEL_UP_SECONDS - age) / LEVEL_UP_FADE).clamp(0.0, 1.0)
            } else {
                let left = METER_SECONDS - s.since_gain;
                (s.age / FADE).min(left / FADE).clamp(0.0, 1.0)
            }
        })
    }

    /// LEVEL UP alone after the meter [observed: private job hud_xp_limbs2,
    /// x9_level_up_long].
    pub fn level_up_word(&self) -> bool {
        self.show.as_ref().is_some_and(|s| s.level_up && s.since_gain >= LEVEL_UP_START)
    }

    /// Keep the owner's sound trigger at the filled pointer, separately from the later
    /// visual notice [owner 2026-10-24, LEVEL_UP_SOUND; design: separate phases].
    fn level_up_filled(&self) -> bool {
        self.show.as_ref().is_some_and(|s| s.level_up && s.since_gain >= FADE + SLIDE)
    }

    /// The meter is gone (the level-up menu waits for this).
    pub fn idle(&self) -> bool {
        self.show.is_none()
    }

    /// What the texts read: the two levels, and the amount or "LEVEL UP".
    pub fn texts(&self) -> Option<(String, String, String)> {
        let s = self.show.as_ref()?;
        let third = if self.level_up_word() { LEVEL_UP_WORD.to_string() } else { format!("+{}", s.amount.round() as i64) };
        Some((s.level.to_string(), (s.level + 1).to_string(), third))
    }
}

/// Where the pointer picture stands for a place 0 to 1 [documented: `_x_min`, `_x_max`].
pub fn pointer_x(fraction: f32) -> f32 {
    POINTER_X.0 + (POINTER_X.1 - POINTER_X.0) * fraction.clamp(0.0, 1.0)
}

pub struct XpMeterPlugin;

impl Plugin for XpMeterPlugin {
    fn build(&self, app: &mut App) {
        // The standing is read before the compass's finds of the frame: on the first frame
        // the scripts run, the start's place found would otherwise be taken as where the
        // meter began, and its +10 XP never shown.
        app.init_resource::<Meter>().add_systems(Update, (watch.before(crate::compass::discover), level_up, draw).chain().in_set(PlaySet::Ui).after(crate::hud::HudSet));
    }
}

/// Read the player's standing, show gains, move time on while the HUD is up.
#[allow(clippy::too_many_arguments)]
fn watch(
    time: Res<Time>,
    view: Res<HudView>,
    mut script: Option<ResMut<ScriptState>>,
    mut meter: ResMut<Meter>,
    sfx: Option<Res<Sfx>>,
    mut sounds: MessageWriter<PlaySound>,
    mut sound_forms: Local<HashMap<&'static str, Option<FormId>>>,
    mut cap_logged: Local<bool>,
    tweaks: Option<Res<crate::tweaks::TweakSettings>>,
) {
    let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else {
        meter.set_if_neq(Meter::default());
        return;
    };
    let now = experience::standing(&rt.game);
    if !*cap_logged {
        *cap_logged = true;
        let dlcs = experience::STORY_DLCS.iter().filter(|d| rt.game.data.load_order.iter().any(|p| p.eq_ignore_ascii_case(d))).count();
        info!("xp: level cap {} ({dlcs} story add-ons)", now.max);
    }
    // The player's tweak: gains wait for the fight to end, then show together [design: docs/design/tweaks.md].
    if tweaks.as_ref().is_some_and(|t| t.no_xp_meter_in_combat) && crate::hud::sneak_now(&rt.game) == crate::hud::Sneak::Danger {
        return;
    }
    let mut play = |name: &'static str| play_sound(sfx.as_deref(), &mut sound_forms, &mut sounds, name);
    match meter.observe(&now) {
        // The player's Quiet Experience tweak: a gain that earns no level is not shown
        // [design: docs/design/tweaks.md]; one that does still brings up LEVEL UP.
        Some(Seen::Gained { amount, level_up: false }) if tweaks.as_ref().is_some_and(|t| t.no_xp_messages) => {
            meter.show = None;
            info!("xp meter: +{amount} XP ({} in all), not shown (Quiet Experience)", now.xp);
        }
        Some(Seen::Gained { amount, level_up }) => {
            info!("xp meter: +{amount} XP ({} in all): level {}, {:.0} % to {}{}", now.xp, now.level, now.fraction() * 100.0, now.level + 1, if level_up { ", next level earned" } else { "" });
            // Every gain [owner, 2026-10-16: "every time you get experience there is also a
            // sound effect that plays"; hypothesis: this record, by its name].
            play("UIPopUpExperienceUp");
        }
        Some(Seen::AtCap { amount }) => {
            info!("xp meter: +{amount} XP at the level cap {}: not shown", now.max);
            // The player's tweak still plays the sound [design: docs/design/tweaks.md].
            if tweaks.as_ref().is_some_and(|t| t.xp_sound_at_cap) {
                play("UIPopUpExperienceUp");
            }
        }
        None => {}
    }
    // Held, its time standing still, while the HUD is not up (menus, conversations).
    if view.shown && meter.bypass_change_detection().step(time.delta_secs()) {
        // The sound and the wait for the menu are `level_up`'s.
        info!("xp meter: LEVEL UP ({} -> {})", now.level, now.level + 1);
    }
}

/// Play a SOUN by Editor ID at the listener, its FormID looked up once.
/// The level-up sound the player's Alternate Level-Up Sounds tweak gives level `level`:
/// A's every `a.0` levels, B's every `b.0`, the larger interval where both fall; none
/// when neither does or its sound is the game's own (empty).
pub fn alternate_level_sound<'a>(level: u32, a: (u32, &'a str), b: (u32, &'a str)) -> Option<&'a str> {
    let hits = |(every, name): (u32, &'a str)| (every > 0 && !name.is_empty() && level.is_multiple_of(every)).then_some((every, name));
    match (hits(a), hits(b)) {
        (Some(x), Some(y)) => Some(if y.0 > x.0 { y.1 } else { x.1 }),
        (Some(x), None) | (None, Some(x)) => Some(x.1),
        (None, None) => None,
    }
}

fn play_sound(sfx: Option<&Sfx>, cache: &mut HashMap<&'static str, Option<FormId>>, sounds: &mut MessageWriter<PlaySound>, name: &'static str) {
    let Some(sfx) = sfx else { return };
    let found = *cache.entry(name).or_insert_with(|| sfx.forms.sounds.iter().find(|(_, s)| s.edid.as_deref().is_some_and(|e| e.eq_ignore_ascii_case(name))).map(|(f, _)| *f));
    match found {
        Some(sound) => {
            info!("xp meter: sound {name}");
            sounds.write(PlaySound { sound, at: None });
        }
        None => warn!("xp meter: sound {name} not found"),
    }
}

/// For an earned level: the level-up sound once the pointer fills (or at once when
/// no bar is up, as for the second of two levels earned together), then the level-up menu
/// [`LEVEL_UP_DELAY`] seconds later once the player can take it: in the world, out of
/// every menu, nobody drawn fighting or searching for them (the owner's rule; counted over
/// the drawn actors as `crate::after_combat` counts them, since an actor left in an
/// unloaded cell keeps its combat state, #3030), and the last fight over for a while
/// (`crate::after_combat`, #1050). The wait counts only while the HUD is up. Each new
/// reason it waits for is logged once (`xp: level up waits: …`).
#[allow(clippy::too_many_arguments)]
fn level_up(
    time: Res<Time>,
    state: Res<State<PlayState>>,
    view: Res<HudView>,
    meter: Res<Meter>,
    mut script: Option<ResMut<ScriptState>>,
    mut sim: ResMut<SimState>,
    sfx: Option<Res<Sfx>>,
    mut sounds: MessageWriter<PlaySound>,
    mut music: Option<ResMut<crate::music::MusicCues>>,
    mut sound_forms: Local<HashMap<&'static str, Option<FormId>>>,
    after: Option<Res<crate::after_combat::AfterCombat>>,
    actors: Option<Res<crate::hitvol::HitVolumes>>,
    mut since_sound: Local<Option<f32>>,
    mut waiting: Local<Option<String>>,
    tweaks: Option<Res<crate::tweaks::TweakSettings>>,
) {
    let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else {
        *since_sound = None;
        return;
    };
    if !experience::level_up_due(&rt.game) {
        *since_sound = None;
        // Earned, but held by SetInCharGen 1 or ToggleLevelUpMenu 0: say so once.
        let due = experience::standing(&rt.game).due();
        let reason = if due && rt.game.player.in_chargen {
            Some("the game is in character generation (SetInCharGen 1)".to_string())
        } else if due && !rt.game.level_up_menu_enabled() {
            Some("a script disabled the level-up menu (ToggleLevelUpMenu 0)".to_string())
        } else {
            None
        };
        note_wait(&mut waiting, reason);
        return;
    }
    // Each hold says once why it holds (#3882: a level-up that never came, with none of
    // the other reasons logged).
    if let Some(why) = held_before_sound(*state.get(), &view, &rt.game) {
        note_wait(&mut waiting, Some(why));
        return;
    }
    let Some(since) = since_sound.as_mut() else {
        // When No XP Meter in Combat defers showing the gain in `watch`, hold the
        // level-up sound too so it waits for the deferred XP bar to fill [design].
        if tweaks.as_ref().is_some_and(|t| t.no_xp_meter_in_combat) && crate::hud::sneak_now(&rt.game) == crate::hud::Sneak::Danger {
            note_wait(&mut waiting, Some("the player is in a fight (the No XP Meter in Combat tweak)".to_string()));
            return;
        }
        if !(meter.level_up_filled() || meter.idle()) {
            note_wait(&mut waiting, Some("the XP meter is still filling".to_string()));
        } else {
            info!("xp: level up sound; the menu follows after {LEVEL_UP_DELAY:.1} s of play once nothing holds it");
            // The player's tweak: every so many levels another sound [design: docs/design/tweaks.md].
            let next = rt.game.player_level() as u32 + 1;
            let own = tweaks.as_deref().and_then(|t| alternate_level_sound(next, (t.level_sound_every_a as u32, &t.level_sound_a), (t.level_sound_every_b as u32, &t.level_sound_b)));
            match (own, sfx.as_deref()) {
                (Some(name), Some(s)) if s.forms.sounds.values().any(|x| x.edid.as_deref().is_some_and(|e| e.eq_ignore_ascii_case(name))) => {
                    let sound = s.forms.sounds.iter().find(|(_, x)| x.edid.as_deref().is_some_and(|e| e.eq_ignore_ascii_case(name))).map(|(f, _)| *f);
                    if let Some(sound) = sound {
                        info!("xp meter: sound {name} for level {next} (tweak)");
                        sounds.write(PlaySound { sound, at: None });
                    }
                }
                _ => play_sound(sfx.as_deref(), &mut sound_forms, &mut sounds, LEVEL_UP_SOUND),
            }
            // And the default objects' Level Up Music, when it has a file (#991).
            if let Some(m) = music.as_deref_mut() {
                m.level_up = true;
            }
            *since_sound = Some(0.0);
        }
        return;
    };
    *since += time.delta_secs();
    if *since < LEVEL_UP_DELAY {
        return;
    }
    let g = &rt.game;
    let hunters = match actors.as_deref() {
        Some(a) => crate::after_combat::drawn_hunters(g, &a.living),
        None => experience::hunters(g),
    };
    if let Some(why) = level_up_wait(hunters, after.as_deref()) {
        note_wait(&mut waiting, Some(why));
        return;
    }
    note_wait(&mut waiting, None);
    *since_sound = None;
    let (level, max) = with_world(rt, &mut sim, |rt| (experience::level_up(&mut rt.game), experience::max_level(&rt.game)));
    info!("xp: level up taken: level {level} (cap {max})");
}

/// What keeps an earned level-up from going on at all this frame: not in play, the HUD
/// hidden (and why), or a menu or a conversation open (which one).
fn held_before_sound(state: PlayState, view: &HudView, g: &fnv_game::Game) -> Option<String> {
    if state != PlayState::Playing {
        return Some(format!("the game is not in play ({state:?})"));
    }
    if !view.shown {
        return Some(format!("the HUD is hidden: {}", view.hidden.unwrap_or("for no reason given")));
    }
    if let Some(c) = &g.dialogue {
        return Some(format!("a conversation is open (with {})", c.actor));
    }
    if let Some(m) = g.menu {
        return Some(format!("menu {m} is open"));
    }
    None
}

/// What still keeps an earned level-up's menu shut once its delay has run, with nothing
/// open: drawn actors hunting the player, or the last fight not over for long enough.
fn level_up_wait(hunters: usize, after: Option<&crate::after_combat::AfterCombat>) -> Option<String> {
    if hunters > 0 {
        return Some(format!("{hunters} actors fighting or searching for the player"));
    }
    if after.is_some_and(|a| !a.settled()) {
        return Some(format!("the last fight is not over for {:.1} s of play yet", crate::after_combat::AFTER_COMBAT_DELAY));
    }
    None
}

/// Log the reason a level-up waits when it changes (`None`: nothing waits now).
fn note_wait(last: &mut Option<String>, why: Option<String>) {
    if *last != why {
        if let Some(w) = &why {
            info!("xp: level up waits: {w}");
        }
        *last = why;
    }
}

/// The meter's pointer.
#[derive(Component)]
struct Pointer;
/// The bracket and pointer disappear before the separate LEVEL UP notice [observed:
/// private job hud_xp_limbs2, x9_level_up_long].
#[derive(Component)]
struct MeterVisual;
#[derive(Component)]
struct Texts;
/// Drawn at the meter's strength: pictures and letters, and the plain blocks that stand
/// in for the pictures without the game's files (every node has a background colour, so
/// the two are kept apart).
#[derive(Component, Clone)]
struct Faded;
#[derive(Component)]
struct FadedFill;

struct Kit {
    bracket: Option<Handle<Image>>,
    pointer: Option<Handle<Image>>,
    level: Face,
    label: Face,
    amount: Face,
}

/// Where the amount, or "LEVEL UP" in its place, ends and the line it stands on.
fn third_line(level_up: bool) -> (f32, f32) {
    if level_up {
        (LEVEL_UP_END, LEVEL_UP_BASE)
    } else {
        (AMOUNT_END, AMOUNT_BASE)
    }
}

#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    view: Res<HudView>,
    meter: Res<Meter>,
    data: Option<Res<GameData>>,
    mut images: ResMut<Assets<Image>>,
    formats: Option<Res<CompressedImageFormatSupport>>,
    mut kit: Local<Option<Kit>>,
    mut shown_texts: Local<Option<(String, String, String)>>,
    mut root: Query<&mut Visibility, (With<MeterRoot>, Without<MeterVisual>)>,
    mut visuals: Query<&mut Visibility, (With<MeterVisual>, Without<MeterRoot>)>,
    texts: Query<Entity, With<Texts>>,
    mut pointer: Query<&mut Node, With<Pointer>>,
    mut pictures: Query<&mut ImageNode, With<Faded>>,
    mut words: Query<&mut TextColor, With<Faded>>,
    mut fills: Query<&mut BackgroundColor, With<FadedFill>>,
) {
    // The HUD's lengths at the UI scale (`crate::interface::hud_units`, #1631).
    let _hud = crate::interface::hud_units();
    let on = view.shown && meter.show.is_some();
    if kit.is_none() {
        if !on {
            return;
        }
        let bc = bc_supported(formats.as_deref());
        let d = data.as_deref();
        let mut read = |path: &str| d.and_then(|d| texture(path, d, &mut images, bc)).map(|t| t.0);
        let (bracket, pointer) = (read(BRACKET_PATH), read(POINTER_PATH));
        let loaded = Kit {
            bracket,
            pointer,
            level: face(LEVEL_FONT, plain_font(36.0, 15.0), d, &mut images, bc),
            label: face(LABEL_FONT, plain_font(36.0, 15.0), d, &mut images, bc),
            amount: face(AMOUNT_FONT, plain_font(52.0, 22.0), d, &mut images, bc),
        };
        spawn(&mut commands, &loaded);
        *kit = Some(loaded);
        *shown_texts = None;
        return;
    }
    for mut v in &mut root {
        v.set_if_neq(if on { Visibility::Inherited } else { Visibility::Hidden });
    }
    if !on {
        return;
    }
    let Some(kit) = kit.as_ref() else { return };
    for mut v in &mut visuals {
        v.set_if_neq(if meter.level_up_word() { Visibility::Hidden } else { Visibility::Inherited });
    }
    let colour = amber(255.0).with_alpha(meter.strength());
    let now = meter.texts();
    if *shown_texts != now {
        if let (Ok(e), Some((last, next, third))) = (texts.single(), &now) {
            commands.entity(e).despawn_related::<Children>().with_children(|p| {
                let line = |f: &Font, t: &str| f.lines(t, None).into_iter().next();
                if let Some(l) = line(&kit.level.font, last).filter(|_| third != LEVEL_UP_WORD) {
                    let x = LAST_LEVEL_END - l.width;
                    set_text(p, &kit.level, &Set { line: l, x, base: LEVEL_BASE }, Vec2::ZERO, colour, Faded);
                }
                if let Some(l) = line(&kit.level.font, next).filter(|_| third != LEVEL_UP_WORD) {
                    set_text(p, &kit.level, &Set { line: l, x: NEXT_LEVEL_X, base: LEVEL_BASE }, Vec2::ZERO, colour, Faded);
                }
                if third == LEVEL_UP_WORD {
                    if let Some(l) = line(&kit.amount.font, third) {
                        let (end, base) = third_line(true);
                        set_text(p, &kit.amount, &Set { x: end - l.width, line: l, base }, Vec2::ZERO, colour, Faded);
                    }
                } else {
                    if let Some(l) = line(&kit.amount.font, third) {
                        let (end, base) = third_line(false);
                        set_text(p, &kit.amount, &Set { x: end - l.width, line: l, base }, Vec2::ZERO, colour, Faded);
                    }
                    if let Some(l) = line(&kit.label.font, XP_WORD) {
                        let x = LABEL_END - l.width;
                        set_text(p, &kit.label, &Set { line: l, x, base: LABEL_BASE }, Vec2::ZERO, colour, Faded);
                    }
                }
            });
        }
        *shown_texts = now;
    }
    let left = u(pointer_x(meter.pointer()));
    for mut n in &mut pointer {
        if n.left != left {
            n.left = left;
        }
    }
    for mut p in &mut pictures {
        if p.color != colour {
            p.color = colour;
        }
    }
    for mut t in &mut words {
        t.set_if_neq(TextColor(colour));
    }
    for mut f in &mut fills {
        f.set_if_neq(BackgroundColor(colour));
    }
}

/// The meter: the bracket, the pointer and a holder for the texts, its upright
/// [`MARGIN_X`] from the right edge and its bottom [`BOTTOM`] over the view's bottom.
fn spawn(commands: &mut Commands, kit: &Kit) {
    let colour = amber(255.0).with_alpha(0.0);
    let piece = Node {
        position_type: PositionType::Absolute,
        right: u(MARGIN_X),
        bottom: u(BOTTOM),
        width: u(PIECE.x),
        height: u(PIECE.y),
        ..default()
    };
    commands.spawn((MeterRoot, piece, Visibility::Hidden, Pickable::IGNORE)).with_children(|p| {
        match &kit.bracket {
            Some(image) => {
                p.spawn((MeterVisual, at(0.0, 0.0, BRACKET.x, BRACKET.y), ImageNode { color: colour, ..ImageNode::new(image.clone()) }, Faded));
            }
            None => {
                // Without the picture: the line, the ticks and the upright as plain blocks.
                p.spawn((MeterVisual, at(7.0, LINE_MIDDLE - 2.0, 360.0, 4.0), BackgroundColor(colour), FadedFill));
                for i in 0..9 {
                    p.spawn((MeterVisual, at(85.0 + 27.0 * i as f32, 48.0, 4.0, 16.0), BackgroundColor(colour), FadedFill));
                }
                p.spawn((MeterVisual, at(364.0, 7.0, 4.0, 122.0), BackgroundColor(colour), FadedFill));
            }
        }
        let mut place = at(pointer_x(0.0), POINTER_Y, POINTER, POINTER);
        match &kit.pointer {
            Some(image) => {
                p.spawn((Pointer, MeterVisual, place, ImageNode { color: colour, ..ImageNode::new(image.clone()) }, Faded));
            }
            None => {
                // A small block where the triangle would be.
                place.width = u(POINTER);
                p.spawn((Pointer, MeterVisual, place)).with_children(|b| {
                    b.spawn((at(24.0, 29.0, 16.0, 10.0), BackgroundColor(colour), FadedFill));
                });
            }
        }
        p.spawn((Texts, at(0.0, 0.0, PIECE.x, PIECE.y)));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #3030: the menu waits for drawn hunters and the after-combat delay only; once both
    /// are clear nothing holds it, and each new reason is logged once.
    #[test]
    fn the_level_up_waits_for_drawn_hunters_and_the_after_combat_delay_only() {
        use crate::after_combat::AfterCombat;
        let settled = AfterCombat::default();
        let fresh = AfterCombat { fighting: false, quiet: 0.5 };
        assert_eq!(level_up_wait(0, Some(&settled)), None);
        assert_eq!(level_up_wait(0, None), None);
        assert!(level_up_wait(2, Some(&settled)).is_some_and(|w| w.starts_with("2 actors")));
        assert!(level_up_wait(0, Some(&fresh)).is_some_and(|w| w.contains("not over")));
        let mut last = None;
        note_wait(&mut last, Some("a".into()));
        assert_eq!(last.as_deref(), Some("a"));
        note_wait(&mut last, None);
        assert_eq!(last, None, "nothing waits: the next wait is logged again");
    }

    /// #3882: a level-up held before its sound says why: out of play, the HUD hidden (and
    /// why), or a menu open (which).
    #[test]
    fn a_hold_before_the_level_up_sound_names_itself() {
        let mut script = ScriptState::over_no_plugins();
        let g = &mut script.rt.as_mut().unwrap().game;
        let shown = HudView { shown: true, ..default() };
        assert_eq!(held_before_sound(PlayState::Playing, &shown, g), None);
        assert!(held_before_sound(PlayState::Dialogue, &shown, g).is_some_and(|w| w.contains("not in play")));
        let hidden = HudView { shown: false, hidden: Some("the weapon wheel is open"), ..default() };
        assert_eq!(held_before_sound(PlayState::Playing, &hidden, g).as_deref(), Some("the HUD is hidden: the weapon wheel is open"));
        g.open_menu(1002);
        assert_eq!(held_before_sound(PlayState::Playing, &shown, g).as_deref(), Some("menu 1002 is open"));
        g.close_menu();
        assert_eq!(held_before_sound(PlayState::Playing, &shown, g), None);
    }

    #[test]
    fn alternate_level_sounds_fall_on_their_levels() {
        let (a, b) = ((5, "UIA"), (10, "UIB"));
        assert_eq!(alternate_level_sound(5, a, b), Some("UIA"));
        assert_eq!(alternate_level_sound(10, a, b), Some("UIB"));
        assert_eq!(alternate_level_sound(7, a, b), None);
        assert_eq!(alternate_level_sound(10, (5, ""), (0, "UIB")), None);
    }

    /// T12e: "LEVEL UP" stands 16 above the amount's line (the original's 12 pixels higher
    /// at 720 lines), ending at its own x.
    #[test]
    fn level_up_stands_above_the_amounts_line() {
        assert_eq!(third_line(false), (AMOUNT_END, AMOUNT_BASE));
        assert_eq!(third_line(true), (LEVEL_UP_END, AMOUNT_BASE - 16.0));
    }

    #[test]
    fn the_level_notice_draws_alone_including_without_game_textures() {
        let mut m = Meter::default();
        m.observe(&at(1, 10.0));
        m.observe(&at(1, 200.0));
        m.step(3.0);
        let mut app = App::new();
        app.init_resource::<Assets<Image>>()
            .insert_resource(HudView { shown: true, ..default() })
            .insert_resource(m)
            .add_systems(Update, draw);
        app.update(); // Spawn the fallback drawing.
        app.update();
        let texts = |world: &mut World| world.query::<&Text>().iter(world).map(|t| t.0.clone()).collect::<Vec<_>>();
        let before = texts(app.world_mut());
        assert!(before.contains(&"+190".into()) && before.contains(&"1".into()) && before.contains(&"2".into()));
        app.world_mut().resource_mut::<Meter>().step(4.6);
        app.update();
        // [documented: interface.rs, set_marked; fnv-font, Line::text_spaced] The fallback
        // Text puts back the spaces between the placed glyphs (#2298).
        assert_eq!(texts(app.world_mut()), ["LEVEL UP"]);
        let mut visuals = app.world_mut().query_filtered::<&Visibility, With<MeterVisual>>();
        assert!(visuals.iter(app.world()).all(|v| *v == Visibility::Hidden));
    }

    /// A player at `level` with `xp`, between the default totals (200 for level 2, 550
    /// for 3, 1050 for 4), under a cap of 30.
    fn at(level: i32, xp: f64) -> Standing {
        let t = |n| experience::total_for(n, experience::DEFAULT_XP_BASE, experience::DEFAULT_XP_BUMP);
        Standing { xp, level, max: 30, this: t(level), next: t(level + 1) }
    }

    fn run(m: &mut Meter, seconds: f32) -> bool {
        let mut up = false;
        for _ in 0..(seconds * 60.0) as usize {
            up |= m.step(1.0 / 60.0);
        }
        up
    }

    #[test]
    fn the_first_look_and_no_change_show_nothing() {
        let mut m = Meter::default();
        assert_eq!(m.observe(&at(2, 300.0)), None);
        assert_eq!(m.observe(&at(2, 300.0)), None);
        assert!(m.idle());
    }

    /// The owner's screenshot: level 2 to 3, the pointer at about nine tenths, "+54".
    #[test]
    fn a_gain_shows_the_levels_the_pointer_and_the_amount() {
        let mut m = Meter::default();
        m.observe(&at(2, 450.0));
        assert_eq!(m.observe(&at(2, 504.0)), Some(Seen::Gained { amount: 54.0, level_up: false }));
        let s = m.show.clone().unwrap();
        assert!((s.from - 250.0 / 350.0).abs() < 1e-6 && (s.to - 304.0 / 350.0).abs() < 1e-6);
        assert_eq!(m.texts(), Some(("2".into(), "3".into(), "+54".into())));
        // The pointer starts where the player was and settles where the player is.
        assert_eq!(m.pointer(), s.from);
        assert!(!run(&mut m, 1.0));
        assert!((m.pointer() - s.from).abs() < 1e-6);
        assert!((m.strength() - 0.5).abs() < 1e-5);
        run(&mut m, 2.0);
        assert!((m.pointer() - (s.from + s.to) / 2.0).abs() < 1e-5);
        assert_eq!(m.strength(), 1.0);
        run(&mut m, 1.1);
        assert!((m.pointer() - s.to).abs() < 1e-6);
        assert!(m.strength() < 1.0);
        run(&mut m, 2.0);
        assert!(m.idle() && m.strength() == 0.0);
    }

    #[test]
    fn gains_while_shown_add_up_and_keep_it_up() {
        let mut m = Meter::default();
        m.observe(&at(1, 0.0));
        m.observe(&at(1, 20.0));
        run(&mut m, 3.0);
        m.observe(&at(1, 30.0));
        assert_eq!(m.texts().unwrap().2, "+30");
        run(&mut m, 3.0);
        assert!(!m.idle());
    }

    #[test]
    fn reaching_the_next_level_shows_a_separate_notice_after_the_meter() {
        let mut m = Meter::default();
        m.observe(&at(1, 150.0));
        assert_eq!(m.observe(&at(1, 210.0)), Some(Seen::Gained { amount: 60.0, level_up: true }));
        assert_eq!(m.show.as_ref().unwrap().to, 1.0);
        assert_eq!(m.texts().unwrap().2, "+60");
        assert!(!run(&mut m, 4.1));
        assert!(m.level_up_filled());
        assert!(!m.level_up_word());
        assert_eq!(m.texts().unwrap().2, "+60");
        assert!(!run(&mut m, 2.1));
        assert!(!m.level_up_word());
        assert_eq!(m.strength(), 0.0);
        assert!(!m.idle());
        assert!(run(&mut m, 1.4));
        assert!(m.level_up_word());
        assert_eq!(m.texts().unwrap().2, "LEVEL UP");
        assert_eq!(m.strength(), 1.0);
        // The word's moment comes once.
        assert!(!run(&mut m, 1.0));
        run(&mut m, 1.0);
        assert!(m.idle());
    }

    const XP_SOUND_ID: FormId = FormId(0x9001);
    const LEVEL_UP_SOUND_ID: FormId = FormId(0x9002);

    fn sync_app(start_xp: f64) -> App {
        let mut forms = fnv_combat::fx::FxForms::default();
        forms.sounds.insert(XP_SOUND_ID, fnv_combat::fx::Sound { edid: Some("UIPopUpExperienceUp".into()), ..default() });
        forms.sounds.insert(LEVEL_UP_SOUND_ID, fnv_combat::fx::Sound { edid: Some(LEVEL_UP_SOUND.into()), ..default() });

        let mut script = ScriptState::over_no_plugins();
        script.rt.as_mut().unwrap().game.player.xp = start_xp;
        let sim = SimState::new(&fnv_esm::LoadOrder::new(&[]).unwrap(), &fnv_esm::Schema::default(), &fnv_world::WorldIndex::default());

        let mut app = App::new();
        app.insert_resource(Time::<()>::default())
            .insert_resource(State::new(PlayState::Playing))
            .insert_resource(HudView { shown: true, ..default() })
            .init_resource::<Assets<Image>>()
            .init_resource::<Meter>()
            .init_resource::<crate::music::MusicCues>()
            .insert_resource(script)
            .insert_resource(sim)
            .insert_resource(Sfx::new(forms))
            .add_message::<PlaySound>()
            .add_systems(Update, (watch, level_up, draw).chain());
        // First tick establishes the meter baseline without showing anything.
        app.update();
        let _ = drain_sounds(&mut app);
        app
    }

    fn drain_sounds(app: &mut App) -> Vec<FormId> {
        app.world_mut().resource_mut::<bevy::ecs::message::Messages<PlaySound>>().drain().map(|m| m.sound).collect()
    }

    fn step_app(app: &mut App, dt: f32) -> Vec<FormId> {
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(dt));
        app.update();
        drain_sounds(app)
    }

    fn step_many(app: &mut App, seconds: f32) -> Vec<FormId> {
        let mut played = Vec::new();
        let steps = (seconds * 60.0).round() as usize;
        for _ in 0..steps {
            played.extend(step_app(app, 1.0 / 60.0));
        }
        played
    }

    fn hud_texts(app: &mut App) -> Vec<String> {
        app.world_mut().query::<&Text>().iter(app.world()).map(|t| t.0.clone()).collect()
    }

    /// #3438: `UILevelUpText` (and level-up music) plays when the XP bar fills (4.0 s,
    /// owner's 2026-10-24 rule), and the level-up menu opens `LEVEL_UP_DELAY` (5.35 s)
    /// later at 9.35 s — matching the observed 9.35 s menu opening without interrupting
    /// the LEVEL UP notice fade (which ends at 9.18 s).
    #[test]
    fn level_up_sound_plays_at_bar_fill_and_menu_opens_at_observed_9_35s() {
        let mut app = sync_app(150.0);
        app.world_mut().resource_mut::<ScriptState>().rt.as_mut().unwrap().game.player.xp = 210.0;
        let on_gain = step_app(&mut app, 1.0 / 60.0);
        assert_eq!(on_gain, vec![XP_SOUND_ID]);
        assert!(!app.world().resource::<crate::music::MusicCues>().level_up);

        // Before the XP pointer reaches full at 4.0 s: neither sound nor LEVEL UP has fired.
        assert!(step_many(&mut app, 3.8).is_empty(), "sound must not fire before the pointer fills");
        assert!(!app.world().resource::<Meter>().level_up_filled());
        assert!(!app.world().resource::<crate::music::MusicCues>().level_up);
        assert!(hud_texts(&mut app).contains(&"+60".into()));

        // Crossing 4.0 s: UILevelUpText and music cue fire on the exact frame the bar fills.
        let mut trigger_frame_sounds = Vec::new();
        for _ in 0..30 {
            let sounds = step_app(&mut app, 1.0 / 60.0);
            let filled = app.world().resource::<Meter>().level_up_filled();
            if !sounds.is_empty() || filled {
                assert!(filled, "sound fired before the XP bar filled");
                assert_eq!(sounds, vec![LEVEL_UP_SOUND_ID], "bar filled without UILevelUpText on the same frame");
                assert!(app.world().resource::<crate::music::MusicCues>().level_up);
                trigger_frame_sounds = sounds;
                break;
            }
        }
        assert_eq!(trigger_frame_sounds, vec![LEVEL_UP_SOUND_ID]);

        // Through the LEVEL UP notice (7.2 s .. 9.18 s): no duplicate sound, LEVEL UP
        // displays and finishes fading out without being interrupted at 9.0 s.
        assert!(step_many(&mut app, 3.5).is_empty(), "no duplicate level-up sound");
        assert!(app.world().resource::<Meter>().level_up_word());
        assert_eq!(hud_texts(&mut app), vec!["LEVEL UP"]);
        assert!(!app.world().resource::<ScriptState>().rt.as_ref().unwrap().game.in_menu());

        // At ~9.25 s after gain (5.25 s after sound): LEVEL UP has finished fading out
        // (idle), and the menu is still waiting for 9.35 s.
        assert!(step_many(&mut app, 1.75).is_empty());
        assert!(app.world().resource::<Meter>().idle());
        assert!(!app.world().resource::<ScriptState>().rt.as_ref().unwrap().game.in_menu());

        // Crossing 5.35 s after the filled-bar sound (~9.35 s after gain) opens the menu.
        assert!(step_many(&mut app, 0.2).is_empty());
        let g = &app.world().resource::<ScriptState>().rt.as_ref().unwrap().game;
        assert!(g.in_menu());
        assert_eq!(g.player_level(), 2.0);
    }

    #[test]
    fn repeated_xp_gains_do_not_play_level_up_sound_early_or_twice() {
        let mut app = sync_app(150.0);
        app.world_mut().resource_mut::<ScriptState>().rt.as_mut().unwrap().game.player.xp = 210.0;
        assert_eq!(step_app(&mut app, 1.0 / 60.0), vec![XP_SOUND_ID]);

        // Before the 4.0 s filled-bar mark, award another XP gain: timer resets, no early sound.
        assert!(step_many(&mut app, 3.0).is_empty());
        app.world_mut().resource_mut::<ScriptState>().rt.as_mut().unwrap().game.player.xp = 230.0;
        assert_eq!(step_app(&mut app, 1.0 / 60.0), vec![XP_SOUND_ID]);

        // Still no level-up sound 3.0 s after the second gain; plays once when the bar fills at 4.0 s.
        assert!(step_many(&mut app, 3.0).is_empty());
        let played = step_many(&mut app, 1.5);
        assert_eq!(played, vec![LEVEL_UP_SOUND_ID]);

        // Another XP gain after the sound played does not replay UILevelUpText.
        app.world_mut().resource_mut::<ScriptState>().rt.as_mut().unwrap().game.player.xp = 250.0;
        assert_eq!(step_app(&mut app, 1.0 / 60.0), vec![XP_SOUND_ID]);
        assert!(step_many(&mut app, 4.5).is_empty());
    }

    #[test]
    fn large_frame_step_past_level_up_window_still_plays_sound_once() {
        let mut app = sync_app(150.0);
        app.world_mut().resource_mut::<ScriptState>().rt.as_mut().unwrap().game.player.xp = 210.0;
        assert_eq!(step_app(&mut app, 1.0 / 60.0), vec![XP_SOUND_ID]);

        // A single frame hitch jumps past the entire 9.18 s meter + LEVEL UP window:
        // meter becomes idle, and UILevelUpText still plays once rather than being lost.
        assert_eq!(step_app(&mut app, 10.0), vec![LEVEL_UP_SOUND_ID]);
        assert!(app.world().resource::<Meter>().idle());
        assert!(step_app(&mut app, 1.0).is_empty(), "no duplicate sound on subsequent frames");
    }

    #[test]
    fn hidden_hud_pauses_both_text_and_sound_and_second_earned_level_plays_when_idle() {
        // Enough XP for both level 2 (200) and level 3 (550).
        let mut app = sync_app(0.0);
        app.world_mut().resource_mut::<HudView>().shown = false;
        app.world_mut().resource_mut::<ScriptState>().rt.as_mut().unwrap().game.player.xp = 600.0;
        assert_eq!(step_app(&mut app, 1.0 / 60.0), vec![XP_SOUND_ID]);

        // While the HUD is hidden, neither filled-bar sound nor LEVEL UP triggers.
        assert!(step_many(&mut app, 10.0).is_empty());
        assert!(!app.world().resource::<Meter>().level_up_filled());

        // Restore HUD: sound fires when the XP bar fills at 4.0 s.
        app.world_mut().resource_mut::<HudView>().shown = true;
        assert!(step_many(&mut app, 3.5).is_empty());
        assert_eq!(step_many(&mut app, 0.7), vec![LEVEL_UP_SOUND_ID]);
        assert!(app.world().resource::<Meter>().level_up_filled());

        // 5.35 s later (~9.35 s after HUD returned), level 2's menu opens.
        assert!(step_many(&mut app, 5.5).is_empty());
        {
            let mut script = app.world_mut().resource_mut::<ScriptState>();
            let g = &mut script.rt.as_mut().unwrap().game;
            assert_eq!(g.player_level(), 2.0);
            assert!(g.in_menu());
            // Player finishes level 2's menu; level 3 is still earned with no bar up.
            g.menu = None;
        }
        assert_eq!(step_app(&mut app, 1.0 / 60.0), vec![LEVEL_UP_SOUND_ID], "second earned level plays when meter is idle");
    }

    #[test]
    fn no_xp_meter_in_combat_defers_level_up_sound_until_combat_ends() {
        let mut app = sync_app(150.0);
        app.insert_resource(crate::tweaks::TweakSettings { no_xp_meter_in_combat: true, ..default() });
        // Put a hostile actor in combat with the player so sneak_now reports Danger.
        {
            let mut script = app.world_mut().resource_mut::<ScriptState>();
            let g = &mut script.rt.as_mut().unwrap().game;
            let foe = FormId(0x5000);
            let a = g.awareness.actors.entry(foe).or_default();
            a.state = fnv_game::awareness::State::Combat;
            a.targets.insert(FormId::PLAYER_REF, fnv_game::awareness::Target {
                band: fnv_game::detection::Band::Detected,
                value: 100.0,
                at: [0.0; 3],
                detected: Some(0.0),
                hidden_since: None,
            });
            assert_eq!(crate::hud::sneak_now(g), crate::hud::Sneak::Danger);
            g.player.xp = 210.0;
        }

        // While in Danger with No XP Meter in Combat enabled, both meter and sound wait.
        assert!(step_many(&mut app, 5.0).is_empty());
        assert!(app.world().resource::<Meter>().idle());

        // Once combat ends, watch shows the deferred gain and sound waits for bar fill at 4.0 s.
        app.world_mut().resource_mut::<ScriptState>().rt.as_mut().unwrap().game.awareness.actors.clear();
        assert_eq!(step_app(&mut app, 1.0 / 60.0), vec![XP_SOUND_ID]);
        assert!(!app.world().resource::<Meter>().idle());
        assert!(step_many(&mut app, 3.5).is_empty());
        assert_eq!(step_many(&mut app, 0.7), vec![LEVEL_UP_SOUND_ID]);
        assert!(app.world().resource::<Meter>().level_up_filled());
    }

    #[test]
    fn nothing_is_shown_at_the_cap() {
        let mut m = Meter::default();
        m.observe(&Standing { max: 30, ..at(30, 70_000.0) });
        assert_eq!(m.observe(&Standing { max: 30, ..at(30, 70_100.0) }), Some(Seen::AtCap { amount: 100.0 }));
        assert!(m.idle());
        // The cap with the four story add-ons: level 30 is shown again.
        m.observe(&Standing { max: 50, ..at(30, 70_200.0) });
        assert!(!m.idle());
    }

    #[test]
    fn a_reset_or_a_new_level_only_moves_what_was_seen() {
        let mut m = Meter::default();
        m.observe(&at(3, 600.0));
        assert_eq!(m.observe(&at(3, 0.0)), None);
        assert_eq!(m.observe(&at(4, 0.0)), None);
        assert!(m.idle());
        // A gain on a new level starts the pointer at the level's start.
        m.observe(&at(4, 1100.0));
        assert!(m.show.as_ref().unwrap().from == 0.0);
    }

    #[test]
    fn the_pointer_runs_between_the_files_bounds() {
        assert_eq!(pointer_x(0.0), 55.0);
        assert_eq!(pointer_x(1.0), 270.0);
        assert_eq!(pointer_x(2.0), 270.0);
        // Its triangle's middle (31 of 64) over the first and last ticks (86.5, 302.5).
        assert!((pointer_x(0.0) + 31.0 - 86.5).abs() <= 1.0 && (pointer_x(1.0) + 31.0 - 302.5).abs() <= 1.5);
    }
}
