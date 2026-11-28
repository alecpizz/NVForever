//! Detection: how well one actor (the detector) perceives another (the target), as a
//! number, and whether that number makes the target hidden, noticed or detected
//! (docs/spec-notes/actor-behaviour.md §2, docs/design/detection.md).
//!
//! The value is the editor documentation's formula [documented: GECK wiki
//! Category:Detection, owner export 2026-10-09]:
//!
//! ```text
//! D = fSneakBaseValue + Sound + Visual + fSneakSkillMult · (DetectorSkill − TargetSkill)
//! ```
//!
//! - every term but the target's skill is multiplied by the distance falloff
//!   `((R − d) / R) ^ fSneakDistanceAttenuationExponent`, `R` = fSneakMaxDistance (times
//!   fSneakExteriorDistanceMult outdoors);
//! - **Sound** = falloff · fSneakSoundsMult · (movement + action), the falloff further ×
//!   fSneakSoundLosMult without line of sight; movement is 0 standing still, else
//!   (fSneakBootWeightBase + fSneakBootWeightMult · body armour weight) × fSneakRunningMult
//!   when running; action = the loudest action sound × fSneakActionMult; none at all when
//!   that noise is under the loudest sound of the world heard where the target is
//!   (`Situation::sound_cover`, [`masked`]; #2688, NVForever's, not the original's);
//! - **Visual** = falloff · seen · light · movement multiplier · blindness · stealth boy:
//!   light = (fDetectionSneakLightMod + light level × night eye) · fSneakLightMult, night
//!   eye = fDetectionNightEyeBonus (the product capped at 100) when the detector has Night
//!   Eye; movement multiplier = 1 + moving · fSneakLightMoveMult + running ·
//!   fSneakLightRunMult; blindness is always 1 in New Vegas; stealth boy =
//!   fSneakStealthboyMult when the target's Invisibility or Chameleon is at least 1;
//! - **Detector skill** = falloff · Perception skill · (1 + alert · fSneakAlertMod + asleep
//!   · fSneakSleepBonus + in combat with someone else · fSneakCombatMod), Perception skill =
//!   fSneakPerceptionSkillMin + (Max − Min) · Perception / 10. A fighter perceives its own
//!   combat targets at 1.2 and anyone else at 0.8 [observed: captures run15b_t1 and
//!   run15b_t2, 2026-11-15; docs/spec-notes/actor-behaviour.md §2.1a]. This follows the
//!   GECK wiki Category:Detection description of fSneakCombatMod; its IsCombatTarget
//!   formula has the direction reversed (docs/design/detection.md §9);
//! - **Target skill** (New Vegas) = sneaking · (Sneak + iSneakLevelBonus · (target level −
//!   detector level) + max(iSneakStartBonus − iSneakStartBonusLevelPenatly · target level,
//!   0) − armour penalty), the penalty iMediumArmorStealthPenalty or
//!   iHeavyArmorStealthPenalty for medium or heavy armour in the torso slot.
//!
//! Thresholds [documented: same page]: above 0 the target is detected; above
//! fSneakNoticedMin (−20) it is noticed (the alert threshold); else it is hidden.
//!
//! The model is split so it can be tested without a world: [`Settings`] (the game
//! settings, read once), [`Pair`] (what the two actors' records and actor values say,
//! [`Game::detection_pair`]) and [`Situation`] (what the world shows this moment: distance,
//! sight, light, movement; the engine's to measure). [`Detections`] holds the latest
//! value of each pair for the script functions.

use crate::actor_values::{capped, PERCEPTION, SNEAK};
use crate::combat::setting;
use crate::functions::actor::actor_value;
use crate::functions::{Args, Ctx};
use crate::game::Game;
use fnv_script::vm::Value;
use fnv_sim::forms::{ArmorClass, ItemKind};
use fnv_sim::FormId;
use std::collections::{BTreeMap, HashMap};

pub const INVISIBILITY: u16 = 48;
pub const CHAMELEON: u16 = 49;
pub const NIGHT_EYE: u16 = 50;

/// The biped slot of body armour, "Upper Body" [documented: xEdit wbBMDT, the torso slot
/// the Target Skill's armour penalty reads].
pub const UPPER_BODY: u32 = 1 << 2;

/// The detection game settings, with the defaults the editor documentation gives
/// [documented: GECK wiki Category:Detection]. The masters may set other values; the
/// game's are read where they exist.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    pub base_value: f64,
    pub skill_mult: f64,
    pub max_distance: f64,
    pub exterior_distance_mult: f64,
    pub attenuation_exponent: f64,
    pub sound_los_mult: f64,
    pub boot_weight_base: f64,
    pub boot_weight_mult: f64,
    pub running_mult: f64,
    pub action_mult: f64,
    pub sounds_mult: f64,
    pub light_move_mult: f64,
    pub light_run_mult: f64,
    pub light_mod: f64,
    pub light_mult: f64,
    pub stealthboy_mult: f64,
    pub night_eye_bonus: f64,
    pub perception_skill_min: f64,
    pub perception_skill_max: f64,
    pub alert_mod: f64,
    pub sleep_bonus: f64,
    pub combat_mod: f64,
    pub level_bonus: f64,
    pub start_bonus: f64,
    pub start_bonus_level_penalty: f64,
    pub medium_armor_penalty: f64,
    pub heavy_armor_penalty: f64,
    /// The alert threshold, fSneakNoticedMin.
    pub noticed_min: f64,
    /// The firing sound levels, iSoundLevelLoud, Normal and Silent.
    pub sound_loud: f64,
    pub sound_normal: f64,
    pub sound_silent: f64,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            base_value: -35.0,
            skill_mult: 0.5,
            max_distance: 2500.0,
            exterior_distance_mult: 2.0,
            attenuation_exponent: 2.0,
            sound_los_mult: 0.1,
            boot_weight_base: 12.0,
            boot_weight_mult: 0.5,
            running_mult: 1.5,
            action_mult: 2.0,
            sounds_mult: 1.6,
            light_move_mult: 0.01,
            light_run_mult: 0.2,
            light_mod: 0.0,
            light_mult: 1.4,
            stealthboy_mult: 0.0,
            night_eye_bonus: 3.0,
            perception_skill_min: 10.0,
            perception_skill_max: 90.0,
            alert_mod: 0.2,
            sleep_bonus: -0.2,
            combat_mod: -0.4,
            level_bonus: 5.0,
            start_bonus: 50.0,
            start_bonus_level_penalty: 10.0,
            medium_armor_penalty: 10.0,
            heavy_armor_penalty: 20.0,
            noticed_min: -20.0,
            sound_loud: 100.0,
            sound_normal: 50.0,
            sound_silent: 10.0,
        }
    }
}

impl Settings {
    /// The settings as the loaded masters have them, the documented default for each one
    /// they lack.
    pub fn read(g: &Game) -> Settings {
        let d = Settings::default();
        let s = |name: &str, default: f64| setting(g, name, default);
        Settings {
            base_value: s("fSneakBaseValue", d.base_value),
            skill_mult: s("fSneakSkillMult", d.skill_mult),
            max_distance: s("fSneakMaxDistance", d.max_distance),
            exterior_distance_mult: s("fSneakExteriorDistanceMult", d.exterior_distance_mult),
            attenuation_exponent: s("fSneakDistanceAttenuationExponent", d.attenuation_exponent),
            sound_los_mult: s("fSneakSoundLosMult", d.sound_los_mult),
            boot_weight_base: s("fSneakBootWeightBase", d.boot_weight_base),
            boot_weight_mult: s("fSneakBootWeightMult", d.boot_weight_mult),
            running_mult: s("fSneakRunningMult", d.running_mult),
            action_mult: s("fSneakActionMult", d.action_mult),
            sounds_mult: s("fSneakSoundsMult", d.sounds_mult),
            light_move_mult: s("fSneakLightMoveMult", d.light_move_mult),
            light_run_mult: s("fSneakLightRunMult", d.light_run_mult),
            light_mod: s("fDetectionSneakLightMod", d.light_mod),
            light_mult: s("fSneakLightMult", d.light_mult),
            stealthboy_mult: s("fSneakStealthboyMult", d.stealthboy_mult),
            night_eye_bonus: s("fDetectionNightEyeBonus", d.night_eye_bonus),
            perception_skill_min: s("fSneakPerceptionSkillMin", d.perception_skill_min),
            perception_skill_max: s("fSneakPerceptionSkillMax", d.perception_skill_max),
            alert_mod: s("fSneakAlertMod", d.alert_mod),
            sleep_bonus: s("fSneakSleepBonus", d.sleep_bonus),
            combat_mod: s("fSneakCombatMod", d.combat_mod),
            level_bonus: s("iSneakLevelBonus", d.level_bonus),
            start_bonus: s("iSneakStartBonus", d.start_bonus),
            // The setting's name is spelled so [documented: GECK wiki Category:Detection].
            start_bonus_level_penalty: s("iSneakStartBonusLevelPenatly", d.start_bonus_level_penalty),
            medium_armor_penalty: s("iMediumArmorStealthPenalty", d.medium_armor_penalty),
            heavy_armor_penalty: s("iHeavyArmorStealthPenalty", d.heavy_armor_penalty),
            noticed_min: s("fSneakNoticedMin", d.noticed_min),
            sound_loud: s("iSoundLevelLoud", d.sound_loud),
            sound_normal: s("iSoundLevelNormal", d.sound_normal),
            sound_silent: s("iSoundLevelSilent", d.sound_silent),
        }
    }

    /// The farthest a detector perceives anything (fSneakMaxDistance, × the exterior
    /// multiplier outdoors).
    pub fn max_distance(&self, exterior: bool) -> f64 {
        self.max_distance * if exterior { self.exterior_distance_mult } else { 1.0 }
    }

    /// The distance falloff every term but the target's skill takes, 0 at and beyond the
    /// maximum distance.
    pub fn attenuation(&self, distance: f64, exterior: bool) -> f64 {
        let r = self.max_distance(exterior);
        if r <= 0.0 || distance >= r {
            return 0.0;
        }
        ((r - distance.max(0.0)) / r).powf(self.attenuation_exponent)
    }

    /// The action sound of firing a weapon of this WEAP sound level ("Loud", "Normal",
    /// "Silent"); 0 for anything else.
    pub fn firing_sound(&self, level: &str) -> f64 {
        SoundLevel::parse(level).map_or(0.0, |l| self.level_sound(l))
    }

    /// What a sound level is worth (iSoundLevelLoud, Normal, Silent).
    pub fn level_sound(&self, level: SoundLevel) -> f64 {
        match level {
            SoundLevel::Loud => self.sound_loud,
            SoundLevel::Normal => self.sound_normal,
            SoundLevel::Silent => self.sound_silent,
        }
    }

    /// The band a detection value falls in.
    pub fn band(&self, value: f64) -> Band {
        if value > 0.0 {
            Band::Detected
        } else if value > self.noticed_min {
            Band::Noticed
        } else {
            Band::Hidden
        }
    }

    /// The sound the target makes, as the sound term counts it before the distance
    /// falloff: fSneakSoundsMult · (movement + action), movement 0 standing still, else
    /// (fSneakBootWeightBase + fSneakBootWeightMult · body armour weight) × fSneakRunningMult
    /// when running (not with Ignore Running During Detection), a creature's by its sound
    /// level; action = the loudest action sound × fSneakActionMult.
    pub fn noise(&self, p: &Pair, moving: bool, running: bool, action_sound: f64) -> f64 {
        let movement = if moving || running {
            let run = if running && !p.ignore_running { self.running_mult } else { 1.0 };
            // A creature moves as loud as its NAM5 level against Normal (§8 of the design).
            let level = p.target_sound_level.filter(|_| self.sound_normal > 0.0).map_or(1.0, |l| self.level_sound(l) / self.sound_normal);
            (self.boot_weight_base + self.boot_weight_mult * p.target_armor_weight) * run * level
        } else {
            0.0
        };
        self.sounds_mult * (movement + action_sound * self.action_mult)
    }

    /// The detection value of a pair in a situation, with its parts.
    pub fn evaluate(&self, p: &Pair, s: &Situation) -> Detection {
        let a = self.attenuation(s.distance, s.exterior);
        let moving = s.moving || s.running;

        // Sound: none when the world around the target is louder than it ([`masked`]).
        let sound_falloff = if s.line_of_sight { a } else { a * self.sound_los_mult };
        let noise = self.noise(p, moving, s.running, s.action_sound);
        let sound = if masked(noise, s.sound_cover) { 0.0 } else { sound_falloff * noise };

        // Visual.
        let seen = s.line_of_sight && s.in_view;
        let light_level = if p.detector_night_eye { (s.light * self.night_eye_bonus).min(100.0) } else { s.light };
        let light = (self.light_mod + light_level) * self.light_mult;
        let motion = 1.0 + if moving { self.light_move_mult } else { 0.0 } + if s.running { self.light_run_mult } else { 0.0 };
        let stealth_boy = if p.target_invisible { self.stealthboy_mult } else { 1.0 };
        let visual = if seen { a * light * motion * stealth_boy } else { 0.0 };

        // Skills.
        let perception_skill =
            self.perception_skill_min + (self.perception_skill_max - self.perception_skill_min) * p.detector_perception / 10.0;
        let state = 1.0
            + if s.detector_alert { self.alert_mod } else { 0.0 }
            + if s.detector_asleep { self.sleep_bonus } else { 0.0 }
            + if s.in_combat_with_someone_else { self.combat_mod } else { 0.0 };
        let detector_skill = a * perception_skill * state;
        // The target's Sneak counts only against an actor that sees it, or hears it: an
        // unseen, silent target reads the same sneaking or standing, by distance alone
        // [observed: jip_detection, observer-jip-audit-2026-10-25.md §3.2: -12 to -14
        // within 500, -32 to -33 at 3,000-4,000, standing or sneaking; #969]. Kept when
        // there is sound [hypothesis: no unseen, noisy case was captured].
        let target_skill = if s.sneaking && (seen || sound > 0.0) {
            let penalty = match p.target_armor_class {
                ArmorClass::Heavy => self.heavy_armor_penalty,
                ArmorClass::Medium => self.medium_armor_penalty,
                ArmorClass::Light => 0.0,
            };
            p.target_sneak
                + self.level_bonus * (p.target_level - p.detector_level)
                + (self.start_bonus - self.start_bonus_level_penalty * p.target_level).max(0.0)
                - penalty
        } else {
            0.0
        };

        let value = self.base_value + sound + visual + self.skill_mult * (detector_skill - target_skill);
        Detection { value, band: self.band(value), attenuation: a, sound, visual, detector_skill, target_skill, seen: s.camera_sight.unwrap_or(seen), clear: s.line_of_sight }
    }
}

/// Whether the world's sound covers a target's own: its noise ([`Settings::noise`]) under
/// the loudest sound heard where it is, `cover`, in the same units (#2688) [design: not the
/// original's; the owner's request 2026-11-15]. A silent target is not "masked": it makes
/// no sound to hide.
pub fn masked(noise: f64, cover: f64) -> bool {
    noise > 0.0 && noise < cover
}

/// What a detection round keeps ([`Game::begin_detection_round`]).
#[derive(Debug, Default)]
pub struct Round {
    sides: HashMap<FormId, Side>,
    /// `hostile_to(actor, other)` by (actor, other), until [`Game::end_detection_hostility`].
    pub(crate) hostile: Option<HashMap<(FormId, FormId), bool>>,
    /// Each actor's factions and how it stands towards others by them, for the whole
    /// round: they change only by script (AddToFaction and the like) and crimes, which
    /// forget them (`functions::actor::FactionKeep`).
    pub(crate) kept: crate::functions::actor::FactionKeep,
    /// Each actor's kinship facts ([`crate::awareness::KinFacts`]) and Aggression, kept
    /// while `hostile` is: asked again for every pair, `Game::kin` was half the hostility
    /// of a 50-a-side battle's measurement (5.6-9.4 ms of 14.5-18.4 for ~11,000 pairs),
    /// the Aggression a fifth [observed 2026-11-07, #1799]. Nothing the pairs do changes
    /// them; they go with the hostility at [`Game::end_detection_hostility`].
    pub(crate) kin: HashMap<FormId, crate::awareness::KinFacts>,
    pub(crate) aggression: HashMap<FormId, i64>,
}

/// One actor's detection inputs ([`Pair`]'s detector and target fields).
#[derive(Debug, Clone, Copy)]
struct Side {
    perception: f64,
    night_eye: bool,
    sneak: f64,
    invisible: bool,
    armor_weight: f64,
    armor_class: ArmorClass,
    level: f64,
    sound_level: Option<SoundLevel>,
}

/// What the two actors' records and actor values say, for one detector and one target.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pair {
    /// The detector's Perception, 1–10.
    pub detector_perception: f64,
    pub detector_level: f64,
    /// The detector's Night Eye actor value is not 0.
    pub detector_night_eye: bool,
    /// The target's Sneak, 0–100.
    pub target_sneak: f64,
    pub target_level: f64,
    /// The target's Invisibility or Chameleon is at least 1.
    pub target_invisible: bool,
    /// The weight and class of the armour in the target's torso slot (0 and light with
    /// none).
    pub target_armor_weight: f64,
    pub target_armor_class: ArmorClass,
    /// The "Ignore Running During Detection" perk entry point applies to the target.
    /// Perk entry points are not modelled yet, so this is always false.
    pub ignore_running: bool,
    /// A creature target's sound level (CREA NAM5), which scales its movement sound by
    /// its worth ÷ Normal's [hypothesis, docs/design/detection.md §8]; `None` for people.
    pub target_sound_level: Option<SoundLevel>,
}

/// A record's sound level [documented: xEdit `SoundLevel`: Loud, Normal, Silent].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundLevel {
    Loud,
    Normal,
    Silent,
}

impl SoundLevel {
    /// The level a record's label names ("Loud", "Normal", "Silent", any case).
    pub fn parse(label: &str) -> Option<SoundLevel> {
        match label.to_ascii_lowercase().as_str() {
            "loud" => Some(SoundLevel::Loud),
            "normal" => Some(SoundLevel::Normal),
            "silent" => Some(SoundLevel::Silent),
            _ => None,
        }
    }
}

impl Default for Pair {
    fn default() -> Pair {
        Pair {
            detector_perception: 5.0,
            detector_level: 1.0,
            detector_night_eye: false,
            target_sneak: 0.0,
            target_level: 1.0,
            target_invisible: false,
            target_armor_weight: 0.0,
            target_armor_class: ArmorClass::Light,
            ignore_running: false,
            target_sound_level: None,
        }
    }
}

/// What the world shows of one detector and one target at one moment; the engine
/// measures it (distance, sight, light) and knows the actors' movement and state.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Situation {
    /// Distance between the two, in game units.
    pub distance: f64,
    /// Both are in an exterior (the longer maximum distance).
    pub exterior: bool,
    /// Nothing solid between the detector's eyes and the target.
    pub line_of_sight: bool,
    /// The target is within the detector's field of view.
    pub in_view: bool,
    /// The light level at the target, 0–100.
    pub light: f64,
    pub moving: bool,
    pub running: bool,
    pub sneaking: bool,
    /// The loudest action sound the target made this moment (a shot: 100, 50 or 10).
    pub action_sound: f64,
    /// The loudest sound of the world heard where the target is, in the units of its noise
    /// ([`Settings::noise`]); a noise under it is not heard ([`masked`], #2688). 0: none.
    pub sound_cover: f64,
    /// The detector is in a combat state (Alert, Combat or Lost).
    pub detector_alert: bool,
    pub detector_asleep: bool,
    /// The detector is in Combat and the target is absent from its combat target list:
    /// fSneakCombatMod applies [observed: captures run15b_t1 and run15b_t2, 2026-11-15;
    /// docs/spec-notes/actor-behaviour.md §2.1a]. Lost does not qualify [design: §9 of
    /// docs/design/detection.md].
    pub in_combat_with_someone_else: bool,
    /// What GetLineOfSight reads when it differs from the sight above: the player's is the
    /// camera's, its detection the body's [observed: capture detection-2 det4, 2026-10-17,
    /// third person: player.GetDetected Ringo 1 to 90° off the facing on both sides, 0
    /// from 100°; player.GetLineOfSight 0 at 90° left but 1 to 110° right].
    pub camera_sight: Option<bool>,
}

/// Where a detection value falls against the two thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, serde::Serialize, serde::Deserialize)]
pub enum Band {
    /// At or below fSneakNoticedMin (−20).
    #[default]
    Hidden,
    /// Above fSneakNoticedMin, at or below 0: the alert band.
    Noticed,
    /// Above 0.
    Detected,
}

impl Band {
    pub fn name(self) -> &'static str {
        match self {
            Band::Hidden => "hidden",
            Band::Noticed => "noticed",
            Band::Detected => "detected",
        }
    }

    /// The word of the original's sneak meter for this band: HIDDEN, CAUTION, DANGER
    /// [hypothesis: the meter shows the highest band any actor has the player in, with
    /// these bands; not checked against a capture].
    pub fn meter(self) -> &'static str {
        match self {
            Band::Hidden => "HIDDEN",
            Band::Noticed => "CAUTION",
            Band::Detected => "DANGER",
        }
    }
}

/// A detection value and its parts.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Detection {
    pub value: f64,
    pub band: Band,
    /// The distance falloff.
    pub attenuation: f64,
    pub sound: f64,
    pub visual: f64,
    pub detector_skill: f64,
    pub target_skill: f64,
    /// The detector could see the target (line of sight and field of view; for the player
    /// the camera's, `Situation::camera_sight`): what GetLineOfSight reads.
    pub seen: bool,
    /// Nothing solid between the detector's eyes and the target (walls, closed doors),
    /// whichever way the detector faces: `Situation::line_of_sight`. A force greet needs
    /// it ([`Game::greet_sight`]).
    pub clear: bool,
}

impl Detection {
    /// Not detected: the value held at the top of the Hidden band, sight as measured.
    fn hide(&mut self, settings: &Settings) {
        self.value = self.value.min(settings.noticed_min);
        self.band = Band::Hidden;
    }
}

/// Seconds of game time an actor the player woke stays up ([`Game::wake`]) [our choice].
pub const WAKE_SECONDS: f64 = 30.0;

/// The latest detection of each (detector, target) pair the engine measured, and when.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Detections {
    pub pairs: BTreeMap<(FormId, FormId), (Detection, f64)>,
    /// The engine measures the actors' sight of the player (set by the engine at each
    /// measurement; off without one, and under the `tdetect` toggle or the test aid
    /// that leaves the player unseen, where no pair to the player is measured).
    pub measures_player: bool,
}

impl Detections {
    /// Record a pair's detection at game time `time` (seconds); returns the band it had
    /// before, `None` when the pair is new.
    pub fn record(&mut self, detector: FormId, target: FormId, d: Detection, time: f64) -> Option<Band> {
        self.pairs.insert((detector, target), (d, time)).map(|(old, _)| old.band)
    }

    pub fn get(&self, detector: FormId, target: FormId) -> Option<&Detection> {
        self.pairs.get(&(detector, target)).map(|(d, _)| d)
    }

    /// The highest band any detector has the target in (the sneak meter's band for the
    /// player), with that detector; `None` when no one measured it.
    pub fn highest(&self, target: FormId) -> Option<(FormId, Detection)> {
        self.highest_among(target, |_| true)
    }

    /// The detectors `keep` accepts that have `target` in the detected band, the highest
    /// detection value first.
    pub fn detected_by(&self, target: FormId, keep: impl Fn(FormId) -> bool) -> Vec<FormId> {
        let mut v: Vec<(FormId, f32)> = self.pairs.iter().filter(|((d, t), (det, _))| *t == target && det.band == Band::Detected && keep(*d)).map(|((d, _), (det, _))| (*d, det.value as f32)).collect();
        v.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        v.into_iter().map(|(d, _)| d).collect()
    }

    /// [`Detections::highest`] among the detectors `keep` accepts.
    pub fn highest_among(&self, target: FormId, keep: impl Fn(FormId) -> bool) -> Option<(FormId, Detection)> {
        self.pairs
            .iter()
            .filter(|((d, t), _)| *t == target && keep(*d))
            .max_by(|a, b| a.1 .0.value.total_cmp(&b.1 .0.value))
            .map(|((d, _), (det, _))| (*d, *det))
    }

    /// Forget every pair involving `r` after unload or disable.
    pub fn forget(&mut self, r: FormId) {
        self.pairs.retain(|(d, t), _| *d != r && *t != r);
    }

    /// Forget detections made by `r`, preserving other actors' detections of it.
    pub fn forget_detector(&mut self, r: FormId) {
        self.pairs.retain(|(d, _), _| *d != r);
    }

    /// Forget pairs not measured since `time`.
    pub fn forget_older_than(&mut self, time: f64) {
        self.pairs.retain(|_, (_, t)| *t >= time);
    }
}

impl Game<'_> {
    /// Whether an actor not with the player ([`Game::with_player`]) has the player in the
    /// detected band at its latest measurement: the sneak word's `[DETECTED]`
    /// (docs/spec-notes/hud.md §9). A companion's own detection is kept for its AI (#1159).
    /// The actors behind [`Game::player_detected_for_sneak`]: those not with the player
    /// and alive that have the player in the detected band, the most sure first (the
    /// player's Detected by Whom tweak, `nvf_play::hud`).
    pub fn player_detectors_for_sneak(&self) -> Vec<FormId> {
        self.detection.detected_by(FormId::PLAYER_REF, |d| !self.with_player(d) && !self.is_dead(d))
    }

    /// Living, present actors currently in `Band::Detected` against actor `target`,
    /// excluding self-pairs, sorted by FormID (`GetDetectingActors`). Non-actor targets
    /// return an empty list.
    pub fn detecting_actors(&self, target: FormId) -> Vec<FormId> {
        let is_actor = target == FormId::PLAYER_REF
            || crate::functions::base_of(self, target).is_some_and(|b| crate::ai::is_actor_base(self.world.forms, b));
        if !is_actor {
            return Vec::new();
        }
        let mut out: Vec<FormId> = self
            .detection
            .pairs
            .iter()
            .filter_map(|(&(detector, seen), (det, _))| {
                (seen == target
                    && detector != target
                    && det.band == Band::Detected
                    && !self.is_dead(detector)
                    && (detector == FormId::PLAYER_REF || self.present(detector)))
                    .then_some(detector)
            })
            .collect();
        out.sort_unstable();
        out
    }

    pub fn player_detected_for_sneak(&self) -> bool {
        self.detection.highest_among(FormId::PLAYER_REF, |d| !self.with_player(d) && !self.is_dead(d)).is_some_and(|(_, d)| d.band == Band::Detected)
    }

    /// Whether `actor` may force-greet `target` as far as sight goes: nothing solid
    /// (a wall, a closed door) between them at the engine's last measurement of the pair
    /// [owner statement 2026-10-23, playtest list 7 items 41/42: Davison and Harland must
    /// not start a conversation through a closed door or a wall; hypothesis: the
    /// original asks for a clear line, not for the speaker's field of view, the line
    /// being what the owner names]. A pair the engine has not measured blocks the greet
    /// while the engine measures the player at all (the pair comes within a fraction of
    /// a second); with no measuring (headless runs, `tdetect`, the unseen test aid) sight
    /// does not hold anyone back [engineering choice].
    pub fn greet_sight(&self, actor: FormId, target: FormId) -> bool {
        match self.detection.get(actor, target) {
            Some(d) => d.clear,
            None => !self.detection.measures_player,
        }
    }

    /// What the records and actor values say for `detector` perceiving `target`.
    pub fn detection_pair(&mut self, detector: FormId, target: FormId) -> Pair {
        let (d, t) = (self.detection_side(detector), self.detection_side(target));
        // Silent Running: the player's running does not count against it
        // (perk-entry-points.md §7).
        let ignore_running = target == FormId::PLAYER_REF && self.player_has_entry(crate::perks::IGNORE_RUNNING_DURING_DETECTION);
        Pair {
            detector_perception: d.perception,
            detector_level: d.level,
            detector_night_eye: d.night_eye,
            target_sneak: t.sneak,
            target_level: t.level,
            target_invisible: t.invisible,
            target_armor_weight: t.armor_weight,
            target_armor_class: t.armor_class,
            ignore_running,
            target_sound_level: t.sound_level,
        }
    }

    /// One actor's part of a [`Pair`], as detector and as target: worked out once per
    /// detection round ([`Game::begin_detection_round`]), else each time.
    fn detection_side(&mut self, r: FormId) -> Side {
        if let Some(s) = self.detection_round.as_ref().and_then(|round| round.sides.get(&r)) {
            return *s;
        }
        let av = |g: &mut Game, code: u16| actor_value(g, r, code).unwrap_or(0.0);
        let (armor_weight, armor_class) = self.body_armor(r).unwrap_or((0.0, ArmorClass::Light));
        let s = Side {
            perception: capped(PERCEPTION, av(self, PERCEPTION)),
            night_eye: av(self, NIGHT_EYE) != 0.0,
            sneak: capped(SNEAK, av(self, SNEAK)),
            invisible: av(self, INVISIBILITY) >= 1.0 || av(self, CHAMELEON) >= 1.0,
            armor_weight,
            armor_class,
            level: self.actor_level(r),
            sound_level: self.stats_def(r).filter(|d| &d.sig == b"CREA").and_then(|d| d.sound_level.as_deref()).and_then(SoundLevel::parse),
        };
        if let Some(round) = self.detection_round.as_mut() {
            round.sides.insert(r, s);
        }
        s
    }

    /// Start a detection round: until [`Game::end_detection_round`], each actor's
    /// detection inputs (its actor values, level and armour) and who would attack whom are
    /// worked out once and kept. The engine measures every pair of the loaded actors four
    /// times a second; worked out per pair, these were most of a 50-a-side battle's frame
    /// time, and each actor's factions, read for every hostility check, more [observed: CPU
    /// profiles, 2026-10-23, docs/design/streaming-hitches.md §4]. Within a round nothing
    /// they read changes: the pairs only record detections and awareness.
    pub fn begin_detection_round(&mut self) {
        // Crimes that expired since the last round end first (`crate::crime`, #2467),
        // read against the clock of the game's world (the engine's round runs on the
        // played world, `with_world`).
        self.expire_crimes();
        self.detection_round = Some(Round { hostile: Some(HashMap::new()), ..Default::default() });
    }

    /// Who would attack whom is worked out afresh from here on in the round: hits that
    /// provoke and allies taking up a fight change it. Each actor's factions stay kept.
    pub fn end_detection_hostility(&mut self) {
        if let Some(r) = self.detection_round.as_mut() {
            r.hostile = None;
            r.kin.clear();
            r.aggression.clear();
        }
    }

    /// End the detection round: what it kept is dropped.
    pub fn end_detection_round(&mut self) {
        self.detection_round = None;
    }

    /// Start a frame of the combat AI: until [`Game::end_faction_frame`], each actor's
    /// factions and faction relations are worked out once and kept, as in a detection
    /// round (`functions::actor::FactionKeep`). Its comrade and hostility checks asked
    /// them afresh for every pair [observed: CPU profile of a 50-a-side battle,
    /// 2026-10-23, `Game::comrades` → `factions_of` in `npc_combat::fight`]. Scripts and
    /// crimes that change them forget them, so what is read is always what is current.
    ///
    /// Crimes that expired end first, read against the clock of the game's world: the
    /// engine begins the frame on the played world (`with_world`).
    pub fn begin_faction_frame(&mut self) {
        self.expire_crimes();
        self.faction_frame = Some(Default::default());
    }

    /// End the combat AI's frame: the factions it kept are dropped.
    pub fn end_faction_frame(&mut self) {
        self.faction_frame = None;
    }

    /// The weight and class of the armour an actor wears in the torso slot.
    pub fn body_armor(&mut self, actor: FormId) -> Option<(f64, ArmorClass)> {
        let equipped = self.world.get(actor).ok()?.inventory.as_ref()?.equipped();
        equipped.into_iter().find_map(|i| {
            let it = self.world.resolve_item(i)?;
            (it.kind == ItemKind::Armor && self.world.effective_slots(i).is_some_and(|s| s & UPPER_BODY != 0)).then(|| {
                let weight = self.world.effective_weight(i).unwrap_or(0.0) as f64;
                let class = self.world.effective_armor_class(i).unwrap_or(it.armor_class);
                (weight, class)
            })
        })
    }

    /// The player talks to `actor`, asleep in its bed: it wakes and gets up first, and
    /// stays up for [`WAKE_SECONDS`] of game time (the conversation's own time not
    /// counted: the game is held then), after which its package takes it back to bed
    /// (`fnv_ai`) [owner's playtest list 7 item 38: "talking to a sleeping npc should get
    /// them out of their bed/sleep animation", as the original does; the time awake is
    /// our choice].
    pub fn wake(&mut self, actor: FormId) {
        let until = self.time + WAKE_SECONDS;
        self.extra.entry(actor).or_default().woken_until = Some(until);
    }

    /// Whether `actor` was woken by the player lately ([`Game::wake`]) and stays up.
    pub fn woken(&self, actor: FormId) -> bool {
        self.extra.get(&actor).and_then(|e| e.woken_until).is_some_and(|t| self.time < t)
    }

    /// The actor lies on a bed (furniture marker 1–10, on it, not getting on or off): it
    /// detects as a sleeper [hypothesis: lying on a bed is what "sleeping" means to the
    /// detector skill].
    pub fn asleep(&self, actor: FormId) -> bool {
        self.furniture_use(actor).is_some_and(|u| crate::functions::furniture::bed(&u) && u.state == 3)
    }

    /// The detection of `target` by `detector` in situation `s`, recorded for the script
    /// functions at the current game time. An enemy of a ghost does not detect it
    /// ([`Game::ghost_hidden_from`]): the value is held at the top of the Hidden band
    /// [engineering choice: the note says only that it is not detected], sight
    /// (GetLineOfSight) as measured. So is an unconscious detector's (SetUnconscious),
    /// which detects no one: "Actors that are unconscious will not use AI behavior"
    /// [documented: GECK wiki SetUnconscious; hypothesis: detecting is part of it;
    /// observer check 3479-robotics-expert-deactivate]. Old World Blues' X-13 test needs
    /// it: its script watches GetDetected on the robots the player has shut down too
    /// [observed: SCTX of OldWorldBlues.esm SCPT 0x01006DA6] (#3479).
    pub fn detect(&mut self, settings: &Settings, detector: FormId, target: FormId, s: &Situation) -> (Detection, Option<Band>) {
        let p = self.detection_pair(detector, target);
        let mut d = settings.evaluate(&p, s);
        // ForceActorDetectionValue, and the light GetActorLightAmount reads
        // (`crate::nvse_jip11`).
        self.other.jip11.light.insert(target, s.light);
        if self.other.jip11.forced_detection.contains_key(&target) {
            d.value = self.forced_detection(target, d.value);
            d.band = if d.value > 0.0 { Band::Detected } else if d.value > settings.noticed_min { Band::Noticed } else { Band::Hidden };
        }
        if self.extra.get(&detector).is_some_and(|e| e.unconscious) || self.ghost_hidden_from(detector, target) {
            d.hide(settings);
        }
        let before = self.detection.record(detector, target, d, self.time);
        (d, before)
    }

    /// `actor` was set unconscious: what it last detected reads Hidden at once, as its
    /// measurements do from then on ([`Game::detect`]). Awake again, its next measurement
    /// decides.
    pub fn forget_detected_by(&mut self, actor: FormId) {
        let settings = Settings::read(self);
        for (_, (d, _)) in self.detection.pairs.iter_mut().filter(|((by, _), _)| *by == actor) {
            d.hide(&settings);
        }
    }
}

/// `detector.GetDetected target`: 1 when the calling actor detects the target
/// [documented: catalogue notes], that is when its latest detection value is above 0; 0
/// for a pair the engine has not measured (an actor out of reach or not loaded)
/// [engineering choice]. The player is a detector too, measured from the camera with the
/// same formula (its Perception against the target's Sneak) [observed: player.GetDetected
/// Ringo 1 and Chet 1, function probes fp1/fp2 2026-10-17, also with `tdetect` on;
/// hypothesis: the same formula].
pub fn get_detected(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let detector = ctx.subject()?;
    let target = a.form(0)?;
    let d = !g.is_dead(detector) && g.detection.get(detector, target).is_some_and(|d| d.band == Band::Detected);
    Ok(Value::Num(if d { 1.0 } else { 0.0 }))
}

/// `detector.GetDetectionLevel target`: 0 lost, 1 unseen, 2 noticed, 3 seen
/// [documented: catalogue notes]. [hypothesis: unseen, noticed and seen are the hidden,
/// noticed and detected bands; "lost" is a pair not measured. The catalogue notes that
/// the original returns only 0 for the player as the target; NVForever does not copy
/// that.]
pub fn get_detection_level(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let detector = ctx.subject()?;
    let target = a.form(0)?;
    let level = match g.detection.get(detector, target).map(|d| d.band) {
        None => 0.0,
        Some(Band::Hidden) => 1.0,
        Some(Band::Noticed) => 2.0,
        Some(Band::Detected) => 3.0,
    };
    Ok(Value::Num(level))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    /// An NPC of Perception 5, level 1, 500 units from a level 1 player outdoors.
    fn outdoors(light: f64) -> Situation {
        Situation { distance: 500.0, exterior: true, line_of_sight: true, in_view: true, light, ..Default::default() }
    }

    #[test]
    fn detected_by_lists_the_detected_band_most_sure_first() {
        let mut d = Detections::default();
        let me = FormId::PLAYER_REF;
        let pair = |value: f64, band: Band| (Detection { value, band, ..Default::default() }, 0.0);
        d.pairs.insert((FormId(10), me), pair(40.0, Band::Detected));
        d.pairs.insert((FormId(11), me), pair(90.0, Band::Detected));
        d.pairs.insert((FormId(12), me), pair(60.0, Band::Noticed));
        d.pairs.insert((FormId(13), FormId(99)), pair(99.0, Band::Detected));
        assert_eq!(d.detected_by(me, |_| true), vec![FormId(11), FormId(10)]);
        assert_eq!(d.detected_by(me, |x| x != FormId(11)), vec![FormId(10)]);
    }

    #[test]
    fn falloff_reaches_zero_at_the_maximum_distance() {
        let s = Settings::default();
        assert!(near(s.attenuation(0.0, false), 1.0));
        assert!(near(s.attenuation(1250.0, false), 0.25));
        assert!(near(s.attenuation(2500.0, true), 0.25));
        assert_eq!(s.attenuation(2500.0, false), 0.0);
        assert_eq!(s.attenuation(9000.0, true), 0.0);
    }

    #[test]
    fn a_player_standing_in_the_light_is_detected() {
        let s = Settings::default();
        let d = s.evaluate(&Pair::default(), &outdoors(50.0));
        // a = (4500/5000)² = 0.81; visual 0.81 · 50 · 1.4 = 56.7; skill 0.81 · 50 = 40.5.
        assert!(near(d.attenuation, 0.81));
        assert!(near(d.visual, 56.7));
        assert!(near(d.detector_skill, 40.5));
        assert!(near(d.value, -35.0 + 56.7 + 0.5 * 40.5));
        assert_eq!(d.band, Band::Detected);
    }

    #[test]
    fn sneaking_in_the_dark_hides_and_the_dark_alone_is_noticed() {
        let s = Settings::default();
        let p = Pair { target_sneak: 15.0, ..Pair::default() };
        let dark = outdoors(5.0);
        let standing = s.evaluate(&p, &dark);
        assert!(near(standing.value, -35.0 + 0.81 * 7.0 + 0.5 * 40.5));
        assert_eq!(standing.band, Band::Noticed);
        let sneaking = s.evaluate(&p, &Situation { sneaking: true, ..dark });
        // Target skill: 15 + 5 · 0 + max(50 − 10, 0) = 55.
        assert!(near(sneaking.target_skill, 55.0));
        assert_eq!(sneaking.band, Band::Hidden);
    }

    /// An unseen, silent target reads the same sneaking or standing: its Sneak counts only
    /// against an actor that sees or hears it (#969; observer-jip-audit-2026-10-25.md §3.2).
    #[test]
    fn an_unseen_silent_target_reads_the_same_sneaking_or_standing() {
        let s = Settings::default();
        let p = Pair { target_sneak: 15.0, ..Pair::default() };
        for distance in [500.0, 3000.0, 5000.0] {
            let unseen = Situation { distance, in_view: false, ..outdoors(50.0) };
            let standing = s.evaluate(&p, &unseen);
            let sneaking = s.evaluate(&p, &Situation { sneaking: true, ..unseen });
            assert!(near(standing.value, sneaking.value), "{distance}: {} vs {}", standing.value, sneaking.value);
            assert_eq!(sneaking.target_skill, 0.0);
        }
        // Seen, sneaking still lowers it; heard while unseen too (see production `evaluate` above).
        let seen = outdoors(50.0);
        assert!(s.evaluate(&p, &Situation { sneaking: true, ..seen }).value < s.evaluate(&p, &seen).value);
        let heard = Situation { in_view: false, moving: true, ..outdoors(50.0) };
        assert!(s.evaluate(&p, &Situation { sneaking: true, ..heard }).target_skill > 0.0);
    }

    #[test]
    fn a_creature_moves_as_loud_as_its_sound_level() {
        let s = Settings::default();
        let walking = Situation { moving: true, ..outdoors(0.0) };
        let sound = |l: Option<SoundLevel>| s.evaluate(&Pair { target_sound_level: l, ..Pair::default() }, &walking).sound;
        let person = sound(None);
        assert!(person > 0.0);
        assert!(near(sound(Some(SoundLevel::Normal)), person));
        assert!(near(sound(Some(SoundLevel::Silent)), person * 0.2));
        assert!(near(sound(Some(SoundLevel::Loud)), person * 2.0));
    }

    #[test]
    fn running_is_louder_and_more_visible_than_walking() {
        let s = Settings::default();
        let p = Pair { target_armor_weight: 20.0, ..Pair::default() };
        let walk = s.evaluate(&p, &Situation { moving: true, ..outdoors(50.0) });
        let run = s.evaluate(&p, &Situation { running: true, ..outdoors(50.0) });
        // Movement sound (12 + 0.5 · 20) = 22, × 1.5 running; × 0.81 · 1.6.
        assert!(near(walk.sound, 0.81 * 1.6 * 22.0));
        assert!(near(run.sound, 0.81 * 1.6 * 33.0));
        assert!(near(walk.visual, 56.7 * 1.01));
        assert!(near(run.visual, 56.7 * 1.21));
    }

    #[test]
    fn a_louder_world_covers_the_targets_noise() {
        let s = Settings::default();
        let p = Pair { target_armor_weight: 20.0, ..Pair::default() };
        // Walking noise 1.6 · 22 = 35.2; running 1.6 · 33 = 52.8.
        let walk = Situation { moving: true, in_view: false, sneaking: true, ..outdoors(50.0) };
        let heard = s.evaluate(&p, &walk);
        assert!(heard.sound > 0.0 && heard.target_skill > 0.0);
        let covered = s.evaluate(&p, &Situation { sound_cover: 40.0, ..walk });
        assert_eq!(covered.sound, 0.0, "a walk under a 40 cover is not heard");
        assert_eq!(covered.target_skill, 0.0, "unseen and unheard: as a silent target");
        assert!(near(covered.value, heard.value - heard.sound - s.skill_mult * (0.0 - heard.target_skill)));
        let run = s.evaluate(&p, &Situation { running: true, sound_cover: 40.0, ..walk });
        assert!(near(run.sound, 0.81 * 1.6 * 33.0), "a run over the cover is heard whole");
        // Equal is not under; a shot over the cover is heard with the footsteps.
        assert!(!masked(35.2, 35.2) && masked(35.1, 35.2) && !masked(0.0, 10.0));
        let shot = s.evaluate(&p, &Situation { action_sound: s.firing_sound("Normal"), sound_cover: 40.0, ..walk });
        assert!(near(shot.sound, 0.81 * 1.6 * (22.0 + 50.0 * s.action_mult)));
    }

    #[test]
    fn walls_stop_sight_and_muffle_sound() {
        let s = Settings::default();
        let open = Situation { moving: true, ..outdoors(50.0) };
        let wall = Situation { line_of_sight: false, ..open };
        let a = s.evaluate(&Pair::default(), &open);
        let b = s.evaluate(&Pair::default(), &wall);
        assert_eq!(b.visual, 0.0);
        assert!(!b.seen);
        assert!(near(b.sound, a.sound * 0.1));
        let behind = s.evaluate(&Pair::default(), &Situation { in_view: false, ..open });
        assert_eq!(behind.visual, 0.0);
        assert!(near(behind.sound, a.sound));
    }

    #[test]
    fn a_shot_is_heard_through_walls_nearby() {
        let s = Settings::default();
        let p = Pair::default();
        // Sneaking at Sneak 0 behind a wall indoors, unseen and silent: −35 + 0.5 · 0.7744
        // · 50, the Sneak skill left out (#969): noticed, as the original's unseen targets
        // within 500 units (−12 to −14, observer-jip-audit-2026-10-25.md §3.2).
        let quiet = Situation { distance: 300.0, sneaking: true, ..Default::default() };
        let q = s.evaluate(&p, &quiet);
        assert!(near(q.value, -35.0 + 0.5 * 0.7744 * 50.0));
        assert_eq!(q.band, Band::Noticed);
        let shot = Situation { action_sound: s.firing_sound("Loud"), ..quiet };
        // a = (2200/2500)² = 0.7744; sound 0.7744 · 0.1 · 1.6 · 200 ≈ 24.8; heard, the
        // Sneak skill counts again (see production `evaluate` above).
        let d = s.evaluate(&p, &shot);
        assert!(near(d.sound, 0.7744 * 0.1 * 1.6 * 200.0));
        assert!(near(d.value, -35.0 + d.sound + 0.5 * (0.7744 * 50.0 - 40.0)));
        assert_eq!(d.band, Band::Noticed);
    }

    #[test]
    fn levels_armour_and_the_start_bonus_change_the_target_skill() {
        let s = Settings::default();
        let sneak = Situation { sneaking: true, ..outdoors(50.0) };
        let low = Pair { target_sneak: 30.0, target_level: 3.0, detector_level: 1.0, ..Pair::default() };
        // 30 + 5 · 2 + max(50 − 30, 0) = 60.
        assert!(near(s.evaluate(&low, &sneak).target_skill, 60.0));
        let high = Pair { target_level: 8.0, detector_level: 10.0, ..low };
        // 30 + 5 · (−2) + 0 = 20.
        assert!(near(s.evaluate(&high, &sneak).target_skill, 20.0));
        let heavy = Pair { target_armor_class: ArmorClass::Heavy, ..high };
        assert!(near(s.evaluate(&heavy, &sneak).target_skill, 0.0));
        let medium = Pair { target_armor_class: ArmorClass::Medium, ..high };
        assert!(near(s.evaluate(&medium, &sneak).target_skill, 10.0));
        // Not sneaking: no target skill at all.
        assert_eq!(s.evaluate(&low, &outdoors(50.0)).target_skill, 0.0);
    }

    #[test]
    fn perception_night_eye_stealth_boys_and_the_detectors_state() {
        let s = Settings::default();
        let sit = outdoors(40.0);
        let sharp = Pair { detector_perception: 10.0, ..Pair::default() };
        assert!(near(s.evaluate(&sharp, &sit).detector_skill, 0.81 * 90.0));
        let dull = Pair { detector_perception: 1.0, ..Pair::default() };
        assert!(near(s.evaluate(&dull, &sit).detector_skill, 0.81 * 18.0));
        let eye = Pair { detector_night_eye: true, ..Pair::default() };
        // 40 · 3 capped at 100.
        assert!(near(s.evaluate(&eye, &sit).visual, 0.81 * 100.0 * 1.4));
        let boy = Pair { target_invisible: true, ..Pair::default() };
        assert_eq!(s.evaluate(&boy, &sit).visual, 0.0);
        let asleep = s.evaluate(&Pair::default(), &Situation { detector_asleep: true, ..sit });
        assert!(near(asleep.detector_skill, 0.81 * 50.0 * 0.8));
        // Own foe at 1.2, anyone else at 0.8 [observed: captures run15b_t1 and run15b_t2,
        // 2026-11-15; docs/spec-notes/actor-behaviour.md §2.1a].
        let foe = s.evaluate(&Pair::default(), &Situation { detector_alert: true, ..sit });
        assert!(near(foe.detector_skill, 0.81 * 50.0 * 1.2));
        let other = s.evaluate(&Pair::default(), &Situation { detector_alert: true, in_combat_with_someone_else: true, ..sit });
        assert!(near(other.detector_skill, 0.81 * 50.0 * 0.8));
        assert!(foe.value > s.evaluate(&Pair::default(), &sit).value);
        assert!(other.value < s.evaluate(&Pair::default(), &sit).value);
    }

    #[test]
    fn beyond_the_maximum_distance_only_the_target_skill_counts() {
        let s = Settings::default();
        let far = Situation { distance: 6000.0, ..outdoors(100.0) };
        let d = s.evaluate(&Pair::default(), &far);
        assert_eq!(d.value, -35.0);
        assert_eq!(d.band, Band::Hidden);
    }

    #[test]
    fn bands_follow_the_thresholds() {
        let s = Settings::default();
        assert_eq!(s.band(0.5), Band::Detected);
        assert_eq!(s.band(0.0), Band::Noticed);
        assert_eq!(s.band(-19.9), Band::Noticed);
        assert_eq!(s.band(-20.0), Band::Hidden);
        assert_eq!(Band::Noticed.meter(), "CAUTION");
    }

    #[test]
    fn the_table_keeps_the_latest_value_and_the_highest_band() {
        let mut t = Detections::default();
        let (a, b, p) = (FormId(0x100), FormId(0x200), FormId::PLAYER_REF);
        let hid = Detection { value: -30.0, band: Band::Hidden, ..Default::default() };
        let seen = Detection { value: 12.0, band: Band::Detected, ..Default::default() };
        assert_eq!(t.record(a, p, hid, 1.0), None);
        assert_eq!(t.record(b, p, seen, 1.0), None);
        assert_eq!(t.highest(p).map(|(d, x)| (d, x.band)), Some((b, Band::Detected)));
        assert_eq!(t.record(b, p, hid, 2.0), Some(Band::Detected));
        t.forget_older_than(1.5);
        assert!(t.get(a, p).is_none() && t.get(b, p).is_some());
        t.forget(b);
        assert!(t.pairs.is_empty());
    }
}
