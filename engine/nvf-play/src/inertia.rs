//! Movement inertia: the player character speeds up and slows down on the ground instead
//! of going from standing to full speed, and back, in one step [design: the owner's
//! request of 2026-11-13, "when player lets go of the movement key the character does not
//! stop instantly"; the original's player is at full speed from the first loop, physics.md
//! §6.3, so this departs from it on purpose, behind the "Movement inertia" switch].
//!
//! - **Where**: `crate::player`'s fixed step shapes the velocity the movement keys ask
//!   for before the character controller gets it. The controller (`fnv_phys`), the
//!   people's movement and the parity checks are untouched: only the player moves this way.
//! - **How**: each axis of the ground velocity rides a critically damped spring
//!   ([`crate::ease::Spring`]) towards the keys' velocity: about [`START_SECONDS`] to
//!   reach a new speed, walking or running, and [`STOP_SECONDS`] (half that) to glide down
//!   to a twentieth of it, after which the last of the glide eases out (a run's glide is
//!   about 36 units, half a metre); braking before a turn back takes the stopping time.
//!   From rest the spring starts gently, speeds up and settles without overshoot; solved
//!   exactly per step, it is the same at any physics tick rate. [design: the owner's
//!   playtests of 2026-11-13: first "slow the movement inertia down", then "stopping feels
//!   like … sliding on ice - make the stop time 2x speed of the start one".]
//! - **Switches** (the settings page's NVF Features › Advanced Movement): the Global Toggle
//!   over all of them ([`MovementInertia::advanced`]); under Gameplay, Movement Inertia (the
//!   ground inertia here, the eased crouch, the take-off and landing dip and a hard
//!   landing's weight), Weapon Handling (the slower draw, put-away and aim, the weapon's
//!   weight in them), Advanced Recoil, Sprint (its shared state and contract:
//!   `crate::stance`); under Visual Only, Animation Smoothing here and
//!   nvforever's Weapon Inertia and Camera Motion (`PlayerBody`) [design: the owner,
//!   2026-11-15, "simplify the Advanced Movement menu ... add 2 categories"].
//! - **Movement Weight** (a dropdown under the Global Toggle, [`MOVEMENT_WEIGHTS`]): 100 %
//!   is the times below; 25 % makes the body's inertia four times as quick (starting,
//!   stopping, crouching, the landing's stand-up and the speed it costs) and Weapon
//!   Handling's extra time over the original's a quarter as long [design: the owner,
//!   2026-11-15, after testers found it heavy: "Movement Weight under the global toggle ...
//!   25-100% while 100% being the current values"]. (An "Inertia weight" dropdown from 0.5
//!   to 2 was removed in the eleventh playtest.)
//! - **Attributes** ([`MovementInertia::equip_rate`], [`MovementInertia::aim_seconds`]):
//!   draws and put-aways play at the original's Agility rate, 0.5 + 0.1 × Agility
//!   [documented: Fallout Wiki, Agility (Fallout: New Vegas): the draw and holster times as
//!   the reload's, 10 ÷ (Agility + 5); docs/handoff/special-skills-audit-2026-10-23.md §2].
//!   With Weapon Handling, Agility counts most for light weapons and Strength most for heavy
//!   ones ([`attribute_rate`]), and the aim takes a smaller share of the same ([`aim_rate`])
//!   [design: the owner, 2026-11-15]. The ground speed itself is the original's: its run
//!   speed does not depend on Agility (docs/spec-notes/ext-last-core.md §4).
//! - **The air keeps its own rules** (`fnv_phys::air`, the game's measured air control):
//!   off the ground and swimming the keys' velocity goes to the controller as before, and
//!   the spring follows what the character does, so a landing carries the air speed into
//!   the ground speed. The game's half-second landing speed profile is left out while the
//!   switch is on: the landing lands into the inertia instead.
//! - **Clips**: while the body glides with no key held, or brakes against the keys, the
//!   movement clips follow the way it slides ([`MovementInertia::clip_intent`]). What the
//!   game's rules read (detection, power attacks, scripts) still reads the keys.

use crate::ease::{omega_for, Spring};
use bevy::prelude::*;

/// Seconds to reach a new, faster speed from rest (95%).
pub const START_SECONDS: f32 = 0.5;
/// Seconds to glide down to 5% of the speed, or to brake before a turn back (95%): half
/// the start [design: the owner's second playtest, 2026-11-13,
/// "stopping feels like the character is sliding on ice - make the stop time 2x speed of
/// the start one"; 0.9 s before].
pub const STOP_SECONDS: f32 = 0.25;
/// With Weapon Handling, drawing and putting away a weapon of `weight` (pounds) take this
/// many times as long as the original's at Agility 5, before the attributes
/// ([`attribute_rate`]) and Movement Weight: light weapons (2 lb and under) 1.5, a rifle of
/// about 8 lb 1.8, from 20 lb 2.4, smoothly between. Played along the settling curve (about
/// the clip's own speed at first, very slow at the end), so a switch has weight; the game's
/// readiness waits for it [design: the owner, 2026-11-15, testers: "the weapon draw speed is
/// kinda brutal ... like pulling out a concrete slab", "make lighter/medium items faster to
/// draw"; it was 2.2 times for a 10mm pistol, 2.4 for a rifle, 3.3 for heavy weapons, then
/// 1.2 / 1.5 / 2 for one build, then the owner's "Light 1.5, Medium 1.8, Heavy 2.4"].
pub fn draw_weight(weight: f32) -> f32 {
    use crate::ease::smoothstep;
    1.5 + 0.3 * smoothstep(2.0, 8.0, weight) + 0.6 * smoothstep(8.0, 20.0, weight)
}

/// How much a weapon of `weight` (pounds) is light, medium and heavy, summing to 1: light up
/// to 3 lb (pistols, knives), medium from 8 to 10 lb (rifles, shotguns), heavy from 18 lb
/// (heavy weapons, two-handed hammers), smoothly between [design].
pub fn weight_class(weight: f32) -> [f32; 3] {
    use crate::ease::smoothstep;
    let light = 1.0 - smoothstep(3.0, 8.0, weight);
    let heavy = smoothstep(10.0, 18.0, weight);
    [light, (1.0 - light - heavy).max(0.0), heavy]
}

/// How much Agility and Strength count for a light, medium and heavy weapon's draw: Agility
/// most for light ones, Strength most for heavy ones [design: the owner, 2026-11-15].
pub const AGILITY_SHARE: [f32; 3] = [1.0, 0.6, 0.3];
pub const STRENGTH_SHARE: [f32; 3] = [0.3, 0.6, 1.0];

/// With Weapon Handling, how fast the player draws and puts away a weapon of `weight` for
/// their Agility and Strength, a multiple of the speed at 5 and 5: each point from 5 is
/// the original's tenth (its 0.5 + 0.1 × Agility) times the attribute's share for the
/// weapon's class ([`weight_class`]), so a light weapon at Agility 10 draws as the
/// original's does (1.5), a heavy one at Strength 10 as well. Attributes are read within
/// 1 to 10; never under [`ATTRIBUTE_RATE_MIN`].
pub fn attribute_rate(weight: f32, agility: f32, strength: f32) -> f32 {
    let class = weight_class(weight);
    let share = |s: [f32; 3]| class.iter().zip(s).map(|(c, s)| c * s).sum::<f32>();
    let (a, s) = (agility.clamp(1.0, 10.0) - 5.0, strength.clamp(1.0, 10.0) - 5.0);
    (1.0 + 0.1 * (a * share(AGILITY_SHARE) + s * share(STRENGTH_SHARE))).max(ATTRIBUTE_RATE_MIN)
}

/// [`attribute_rate`]'s floor: the original's rate at Agility 1, so two low attributes
/// together cost no more than one does in the original (Agility and Strength 1 with a
/// minigun drew at 4.2 times its clip's time; review, 2026-11-15) [design].
pub const ATTRIBUTE_RATE_MIN: f32 = 0.6;

/// The share of [`attribute_rate`]'s gain or loss the aim takes, so even at 10 and 10 aiming
/// a light weapon stays deliberate [design: the owner, 2026-11-15, "do not make the min-max
/// difference huge"].
pub const AIM_ATTRIBUTE_SHARE: f32 = 0.4;

/// How fast aiming comes up and goes down for the attributes, a multiple of the speed at
/// 5 and 5: 1.26 at most (light weapon, Agility and Strength 10), 0.84 at least.
pub fn aim_rate(weight: f32, agility: f32, strength: f32) -> f32 {
    1.0 + AIM_ATTRIBUTE_SHARE * (attribute_rate(weight, agility, strength) - 1.0)
}

/// How many times [`AIM_IN_SECONDS`] and [`AIM_OUT_SECONDS`] aiming a weapon of `weight`
/// takes at 5 and 5: 0.9 at 0.5 lb, 1 for a pistol of about 3 lb, 1.07 for an 8 lb rifle,
/// 1.35 from 20 lb (it was [`handling`], up to 1.5) [design: the owner, 2026-11-15].
pub fn aim_weight(weight: f32) -> f32 {
    use crate::ease::smoothstep;
    0.9 + 0.1 * smoothstep(0.5, 3.0, weight) + 0.35 * smoothstep(3.0, 20.0, weight)
}

/// The Movement Weight dropdown's choices: the share of the full weight, and the name the
/// page shows (module notes).
pub const MOVEMENT_WEIGHTS: [(f32, &str); 4] = [(0.25, "25%"), (0.5, "50%"), (0.75, "75%"), (1.0, "100%")];

/// The Movement Weight of a name or a number (`"75%"`, `"75"`, `"0.75"`).
pub fn movement_weight_by_name(s: &str) -> Option<f32> {
    let s = s.trim();
    let number = match s.strip_suffix('%') {
        Some(percent) => percent.trim().parse::<f32>().ok().map(|v| v / 100.0),
        None => s.parse::<f32>().ok().map(|v| if v > 1.0 { v / 100.0 } else { v }),
    };
    MOVEMENT_WEIGHTS.iter().find(|(w, name)| *name == s || number.is_some_and(|v| (v - w).abs() < 1e-3)).map(|(w, _)| *w)
}

/// The name the page shows for a Movement Weight (the nearest offered).
pub fn movement_weight_name(weight: f32) -> &'static str {
    MOVEMENT_WEIGHTS.iter().min_by(|a, b| (a.0 - weight).abs().total_cmp(&(b.0 - weight).abs())).map_or("100%", |(_, n)| n)
}
/// With smooth weapon animations, aiming takes this long to come up and to go down,
/// along the settling curve (`crate::ease::settle`: quick first, very slow at the
/// end): the aim and block clips' cross-over, the true iron sights' move to the eye and
/// the view's zoom all run on these, so they are one motion [design: the owner's seventh
/// playtest, 2026-11-14: "fast at the start but very slow and smooth at the end - make
/// the zoom in use the same curve … so it matches the animation"].
pub const AIM_IN_SECONDS: f32 = 0.6;
pub const AIM_OUT_SECONDS: f32 = 0.55;
/// Without Weapon Handling the aim keeps the game's own pace, the true iron sights' (nvforever
/// `iron_sights::SIGHTS_IN_SECONDS`): Animation Smoothing then only shapes the way, along
/// the settling curve, the clips' cross-over and the zoom with it.
pub const AIM_IN_PLAIN_SECONDS: f32 = 0.35;
pub const AIM_OUT_PLAIN_SECONDS: f32 = 0.3;
/// The first-person view and arms take this long into a crouch and out of one: the
/// standing and sneaking clips cross over that long along the
/// settling curve, so the view sinks and the arms change their hold together.
pub const CROUCH_DOWN_SECONDS: f32 = 1.0;
pub const CROUCH_UP_SECONDS: f32 = 1.1;
/// The first-person arms' standing, walking and running clips cross over this long
/// along the settling curve (their own times are about 0.2 s, straight).
pub const MOVE_FADE_SECONDS: f32 = 0.45;
/// The first-person landing clip (the view dips and stands up again) takes this many times
/// its own length, its place along [`land_curve`]: the dip comes at the clip's own pace
/// from the landing frame on, the standing up after it slowly and to a soft end. A
/// hop's landing (up to [`LANDING_SOFT_SPEED`]) takes the first, a hard one (from
/// [`LANDING_HARD_SPEED`]) the second, smoothly between ([`land_stretch`]) [design: the
/// owner's eighth playtest, 2026-11-14, "fall-scaled landing"].
pub const LAND_STRETCH: f32 = 1.8;
pub const HARD_LAND_STRETCH: f32 = 3.2;
/// After a reload, a jam or an attack the first-person arms go back to the hold over this
/// long (× the weapon's handling), along the settling curve; the clips
/// themselves start at once and the arms ease across the step to them [design: the owner's
/// eighth playtest, 2026-11-14: "try improving reloads with springs"].
pub const RELOAD_OUT_SECONDS: f32 = 0.4;
pub const ATTACK_OUT_SECONDS: f32 = 0.3;
/// With soft recoil the view's kick of a shot comes over this long (95 %):
/// most of it at once, the rest softly (`crate::combat`). The climb is the
/// same, only spread over a few frames [design: the owner's eighth playtest, 2026-11-14:
/// "fast start, slow end … the jump is not instant to the next recoil point"].
pub const KICK_EASE_SECONDS: f32 = 0.16;
/// With soft recoil this share of each shot's view kick settles back down again,
/// over this long (95 %) on a critically damped spring: the gun
/// rises and comes back down a little instead of the whole kick staying in the view
/// [design: the owner's ninth playtest, 2026-11-14: "make the gun smoothly return down a
/// bit so the recoil does not just force the whole camera up. Also make the end of the
/// recoil smoother - slower ending" (the rise was 0.1 s)].
pub const KICK_BACK_SHARE: f32 = 0.4;
pub const KICK_BACK_SECONDS: f32 = 0.6;
/// How many times its base times the first-person hands take to draw, put away and aim a
/// weapon of `weight` (pounds) and to come back from its reload: light ones (1 lb and under)
/// 0.85, a pistol of about 3 lb 1, from 20 lb 1.5, smoothly between [design: the owner's
/// eighth playtest, 2026-11-14, "weapon weight slows handling"; the 10mm pistol the earlier
/// times were tuned with weighs 3].
pub fn handling(weight: f32) -> f32 {
    use crate::ease::smoothstep;
    0.85 + 0.15 * smoothstep(0.5, 3.0, weight) + 0.5 * smoothstep(3.0, 20.0, weight)
}

/// A hard landing pushes the view and the hands this many times as hard as its speed alone
/// would (from [`LANDING_HARD_SPEED`]; a hop's push, below [`LANDING_SOFT_SPEED`], is as
/// it was), so a long drop lands heavily [design: the owner's eighth playtest, 2026-11-14].
pub const HARD_LANDING_GAIN: f32 = 1.6;

/// A take-off's or a landing's push (a change of vertical speed, units/s) as the view's
/// and the hands' springs take it ([`HARD_LANDING_GAIN`]).
pub fn hard_landing(push: f32) -> f32 {
    push * (1.0 + (HARD_LANDING_GAIN - 1.0) * crate::ease::smoothstep(LANDING_SOFT_SPEED, LANDING_HARD_SPEED, push.abs()))
}

/// How many times its own length the first-person landing clip takes for a landing at
/// `fall_speed` (units/s).
pub fn land_stretch(fall_speed: f32) -> f32 {
    LAND_STRETCH + (HARD_LAND_STRETCH - LAND_STRETCH) * crate::ease::smoothstep(LANDING_SOFT_SPEED, LANDING_HARD_SPEED, fall_speed)
}

/// Where the first-person landing clip is (0–1 of its length) at `u`, 0–1 of the `stretch`
/// times longer time it takes: `1 − (1 − u)^stretch`, which leaves its start at the clip's
/// own pace and slows all the way to rest at its end. Along the settling curve, which
/// leaves 0 at no speed, the clip's first frames (the impact: the view going down) played
/// in slow motion and the dip came about 0.06 s late, quickest 0.2 s after the feet were
/// down [design: the owner, 2026-11-17, "when holding W - moving forward and jumping, the
/// landing animation has a lag/hitch"; the trace `movement-inertia-trace-run-jump`].
pub fn land_curve(u: f32, stretch: f32) -> f32 {
    1.0 - (1.0 - u.clamp(0.0, 1.0)).powf(stretch.max(1.0))
}

/// The `u` at which [`land_curve`] is `y`.
pub fn land_curve_inv(y: f32, stretch: f32) -> f32 {
    1.0 - (1.0 - y.clamp(0.0, 1.0)).powf(1.0 / stretch.max(1.0))
}

/// Landing compression: landings slower than the first fall speed (units/s) keep their
/// speed, from the second on they lose [`LANDING_MOST_LOST`] of it, smoothly between.
pub const LANDING_SOFT_SPEED: f32 = 350.0;
pub const LANDING_HARD_SPEED: f32 = 900.0;
pub const LANDING_MOST_LOST: f32 = 0.6;
/// Ground speed below which a glide no longer plays the movement clips, units/s.
pub const GLIDE_CLIP_SPEED: f32 = 30.0;
/// A move in one frame this far across the ground (units) is a placement, not motion:
/// the spring starts again from rest.
const PLACED_DISTANCE: f32 = 500.0;
/// Within this of the keys' velocity (units/s) and slower to change than [`SNAP_RATE`]
/// (units/s²), the spring is the keys' velocity exactly.
const SNAP_SPEED: f32 = 0.3;
const SNAP_RATE: f32 = 3.0;

/// What a start or a stop took, for the log (a smoke check).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Report {
    /// From standing to 95% of `speed` (units/s) in `seconds`.
    Started { seconds: f32, speed: f32 },
    /// From `speed` (units/s) when the keys were let go to a standstill: `distance`
    /// units in `seconds`.
    Stopped { seconds: f32, distance: f32, speed: f32 },
}

/// The player's ground velocity under inertia, and the switches (by default as shipped: the
/// Global Toggle and Advanced Recoil on, every other part off [design: the owner, 2026-11-16]). Each
/// switch's own field is what the settings page shows; whether it acts is its method, under
/// the main switch.
#[derive(Resource, Debug, Clone)]
pub struct MovementInertia {
    /// The Global Toggle: off, none of the switches here acts, nor Weapon Inertia and Camera
    /// Motion (nvforever `PlayerBody`).
    pub advanced: bool,
    /// Movement Inertia ([`Self::ground`]): the ground speed's springs, the eased crouch, the
    /// take-off and landing dip and a hard landing's weight.
    pub on: bool,
    /// Animation Smoothing ([`Self::smooth_hands`]): the first-person clips along the settling
    /// curve, the aim's and the zoom's way along it, at Weapon Handling's pace or the game's.
    pub smooth: bool,
    /// Advanced Recoil ([`Self::soft_recoil`]): the view's kick eased in and partly settling
    /// back, the view's nudge back.
    pub recoil: bool,
    /// Weapon Handling ([`Self::handles`]): the draw, put-away and aim by the weapon's weight
    /// and the attributes, and the weapon in hand's [`handling`] for the hand-back after a
    /// reload or an attack (1 with it off), kept by [`follow_weapon`].
    pub weapon_handling: bool,
    /// Sprint ([`Self::sprint_on`]): the Sprint control runs faster for AP (`crate::stance`).
    pub sprint: bool,
    /// Motion Matching ([`Self::matches_motion`]): the third-person body's walking, running
    /// and turning picked frame by frame from the locomotion clips (nvforever
    /// `player_body.rs`, docs/design/motion-matching.md).
    pub motion_matching: bool,
    pub handling: f32,
    /// Movement Weight, 0.25 to 1 ([`MOVEMENT_WEIGHTS`]): the share of the inertia's times.
    pub weight: f32,
    /// The weapon in hand's weight (pounds; none: fists or nothing known) and the player's
    /// Agility and Strength now, kept by [`follow_weapon`] and `crate::combat`.
    pub weapon_weight: Option<f32>,
    pub agility: f32,
    pub strength: f32,
    /// Horizontal velocity (game x, y), units/s, and its rate of change.
    axes: [Spring; 2],
    /// The keys' velocity at the last step.
    last_key: [f32; 2],
    /// A start being timed (seconds so far), a glide being measured (seconds, distance,
    /// the speed it began at).
    starting: Option<f32>,
    gliding: Option<(f32, f32, f32)>,
    /// Starts and stops finished since the log last took them.
    pub reports: Vec<Report>,
    /// Whether the last movement asked for ran (not walked): the glide's clips run too.
    pub ran: bool,
    /// On the ground (and not swimming) after the last step.
    pub grounded: bool,
    /// Feet at the end of the last step, to tell a placement from motion.
    last_feet: Option<[f32; 3]>,
    /// Where the game last put the player (`crate::player::Player::placed`).
    last_placed: Option<[f32; 3]>,
}

impl Default for MovementInertia {
    /// What a new game gets: the Global Toggle and Advanced Recoil on, the rest off, so an app
    /// that only adds the resource (a test app, `crate::player::PlayerPlugin`) plays as shipped.
    fn default() -> Self {
        MovementInertia { advanced: true, on: false, smooth: false, recoil: true, weapon_handling: false, sprint: false, motion_matching: false, handling: 1.0, weight: 1.0, weapon_weight: None, agility: 5.0, strength: 5.0, axes: [Spring::REST; 2], last_key: [0.0; 2], starting: None, gliding: None, reports: Vec::new(), ran: true, grounded: true, last_feet: None, last_placed: None }
    }
}

impl MovementInertia {
    /// At rest, Movement Inertia switched on or off, the other parts at their defaults.
    pub fn switched(on: bool) -> MovementInertia {
        MovementInertia { on, ..default() }
    }

    /// At rest with every part on (the tests of the parts).
    pub fn all_on() -> MovementInertia {
        MovementInertia { on: true, smooth: true, weapon_handling: true, ..default() }
    }

    /// The ground speed rides its springs (Movement Inertia under the Global Toggle).
    pub fn ground(&self) -> bool {
        self.advanced && self.on
    }

    /// The first-person view eases into a crouch and out, and a crouched jump stays low
    /// (Movement Inertia).
    pub fn eased_crouch(&self) -> bool {
        self.ground()
    }

    /// The view dips at a take-off and a landing, deeper and slower to stand up after a
    /// higher fall, and a hard landing takes speed (Movement Inertia).
    pub fn landing_weight(&self) -> bool {
        self.ground()
    }

    /// The first-person weapon clips follow the settling curve (Animation Smoothing).
    pub fn smooth_hands(&self) -> bool {
        self.advanced && self.smooth
    }

    /// The third-person body walks, runs and turns by motion matching (Motion Matching).
    pub fn matches_motion(&self) -> bool {
        self.advanced && self.motion_matching
    }

    /// Drawing, putting away and aiming go by the weapon's weight and the attributes, never
    /// quicker than the original's (Weapon Handling).
    pub fn handles(&self) -> bool {
        self.advanced && self.weapon_handling
    }

    /// The view's kick eases in and partly settles back (Advanced Recoil).
    pub fn soft_recoil(&self) -> bool {
        self.advanced && self.recoil
    }

    /// The Sprint control acts: its key (the player's; none by default) runs faster for AP
    /// (Sprint under the Global Toggle; `crate::stance`).
    pub fn sprint_on(&self) -> bool {
        self.advanced && self.sprint
    }

    /// Movement Weight as a share, 0.25 to 1.
    pub fn weight(&self) -> f32 {
        self.weight.clamp(0.25, 1.0)
    }

    /// `base` seconds of the body's inertia at the Movement Weight.
    pub fn weighted(&self, base: f32) -> f32 {
        base * self.weight()
    }

    /// `slower` times the original's time with only the part over it at the Movement Weight:
    /// Weapon Handling's extra time shrinks, the original's stays; quicker than it stays as is.
    fn weighted_extra(&self, slower: f32) -> f32 {
        if slower > 1.0 { 1.0 + (slower - 1.0) * self.weight() } else { slower }
    }

    /// The weapon in hand's weight for the handling's formulas: a fist or an unknown weapon
    /// as a light one (1 lb).
    fn held_weight(&self) -> f32 {
        self.weapon_weight.unwrap_or(1.0)
    }

    /// How fast the player draws and puts away the weapon in hand, a multiple of its clips'
    /// own speed, before the Equip Speed perks: the original's 0.5 + 0.1 × Agility (module
    /// notes); with Weapon Handling the weapon's weight ([`draw_weight`]) over the attributes
    /// by its class ([`attribute_rate`]), never quicker than the original's at the same
    /// Agility, its time over that at the Movement Weight.
    pub fn equip_rate(&self) -> f32 {
        let original = 1.0 / fnv_combat::attack::reload_rate(self.agility);
        if !self.handles() {
            return 1.0 / original;
        }
        let w = self.held_weight();
        let handled = draw_weight(w) / attribute_rate(w, self.agility, self.strength);
        1.0 / (original + (handled - original).max(0.0) * self.weight())
    }

    /// Seconds aiming takes to come up (`up`) or go down: with Weapon Handling the slower
    /// pace by the weapon's weight ([`aim_weight`]) and the attributes ([`aim_rate`]), its
    /// time over the game's own at the Movement Weight; else the game's own.
    pub fn aim_seconds(&self, up: bool) -> f32 {
        let plain = if up { AIM_IN_PLAIN_SECONDS } else { AIM_OUT_PLAIN_SECONDS };
        if !self.handles() {
            return plain;
        }
        let w = self.held_weight();
        let handled = if up { AIM_IN_SECONDS } else { AIM_OUT_SECONDS } * aim_weight(w) / aim_rate(w, self.agility, self.strength);
        plain + (handled - plain).max(0.0) * self.weight()
    }

    /// How many times their base times the first-person hands take to come back from a
    /// reload or an attack (the weapon's handling, its part over 1 at the Movement Weight).
    pub fn hands(&self) -> f32 {
        self.weighted_extra(self.handling).max(0.1)
    }

    /// Seconds the first-person view takes into a crouch (`down`) or out of one, at the
    /// Movement Weight.
    pub fn crouch_seconds(&self, down: bool) -> f32 {
        self.weighted(if down { CROUCH_DOWN_SECONDS } else { CROUCH_UP_SECONDS })
    }

    /// How many times its own length the first-person landing clip takes after a landing at
    /// `fall_speed` ([`land_stretch`], its part over 1 at the Movement Weight).
    pub fn land_stretch(&self, fall_speed: f32) -> f32 {
        self.weighted_extra(land_stretch(fall_speed))
    }

    /// The ground velocity now (game x, y), units/s.
    pub fn velocity(&self) -> [f32; 2] {
        [self.axes[0].value, self.axes[1].value]
    }

    /// Back to rest (placed somewhere, held by furniture, flying with `tcl`).
    pub fn reset(&mut self) {
        self.axes = [Spring::REST; 2];
        self.last_feet = None;
        self.forget_timing();
    }

    fn forget_timing(&mut self) {
        self.last_key = [0.0; 2];
        self.starting = None;
        self.gliding = None;
    }

    /// Before a frame's steps: the game put the player somewhere new (`placed`: a door, the
    /// start, a load), however near, and the body starts from rest there. A door between
    /// two interiors whose coordinates lie close together is under [`PLACED_DISTANCE`]
    /// apart, and the run's speed came along into the new room (review, 2026-11-15).
    pub fn check_game_placed(&mut self, placed: Option<[f32; 3]>) {
        if placed.is_some() && placed != self.last_placed {
            self.reset();
        }
        self.last_placed = placed;
    }

    /// Before a frame's steps: a character moved further than it can walk in a frame was
    /// placed (a door, a script's move) and starts from rest.
    pub fn check_placed(&mut self, feet: [f32; 3]) {
        if self.last_feet.is_some_and(|l| (feet[0] - l[0]).hypot(feet[1] - l[1]) > PLACED_DISTANCE) {
            self.axes = [Spring::REST; 2];
            self.forget_timing();
        }
    }

    /// Times starts from standing and measures glides to a stop, after a step from the
    /// speed `before` to `v`.
    fn time(&mut self, key: [f32; 2], before: f32, v: [f32; 2], dt: f32) {
        let moving = |k: [f32; 2]| k != [0.0, 0.0];
        let (want, speed) = (key[0].hypot(key[1]), v[0].hypot(v[1]));
        if moving(key) && !moving(self.last_key) && speed <= want * 0.2 {
            self.starting = Some(0.0);
        }
        if !moving(key) && moving(self.last_key) && before > GLIDE_CLIP_SPEED {
            self.gliding = Some((0.0, 0.0, before));
        }
        if !moving(key) {
            self.starting = None;
        } else {
            self.gliding = None;
        }
        if let Some(t) = self.starting.as_mut() {
            *t += dt;
            if speed >= 0.95 * want {
                self.reports.push(Report::Started { seconds: *t, speed: want });
                self.starting = None;
            }
        }
        if let Some((t, d, from)) = self.gliding.as_mut() {
            *t += dt;
            *d += (before + speed) / 2.0 * dt;
            if !moving(v) {
                self.reports.push(Report::Stopped { seconds: *t, distance: *d, speed: *from });
                self.gliding = None;
            }
        }
        self.last_key = key;
    }

    /// One fixed step of `dt` seconds on the ground: the velocity towards the keys'
    /// velocity `key` (units/s), starting or braking; the velocity to move at.
    pub fn step(&mut self, key: [f32; 2], walk: bool, dt: f32) -> [f32; 2] {
        if key != [0.0, 0.0] {
            self.ran = !walk;
        }
        let v = self.velocity();
        let speed = v[0].hypot(v[1]);
        let want = key[0].hypot(key[1]);
        // Faster the same way: starting; slower, or against the way it goes: braking.
        let starting = want > speed && key[0] * v[0] + key[1] * v[1] >= 0.0;
        let omega = omega_for(self.weighted(if starting { START_SECONDS } else { STOP_SECONDS }));
        for (s, k) in self.axes.iter_mut().zip(key) {
            s.step(k, omega, dt);
        }
        // Never faster than both the keys ask and it already went (the spring cannot
        // overshoot from rest; this holds when a change of rate meets a moving spring).
        let now = self.velocity();
        let limit = want.max(speed);
        let len = now[0].hypot(now[1]);
        if len > limit && len > 0.0 {
            for s in &mut self.axes {
                s.value *= limit / len;
            }
        }
        for (s, k) in self.axes.iter_mut().zip(key) {
            s.snap(k, SNAP_SPEED, SNAP_RATE);
        }
        let v = self.velocity();
        self.time(key, speed, v, dt);
        v
    }

    /// After a step off the ground or in the water (or with the switch off): the spring
    /// takes the velocity the character moved at, so the ground picks up from there.
    pub fn follow(&mut self, velocity: [f32; 2]) {
        self.axes = [Spring { value: velocity[0], rate: 0.0 }, Spring { value: velocity[1], rate: 0.0 }];
        self.forget_timing();
    }

    /// A landing at `fall_speed` (units/s, down): a hard one takes speed out of the
    /// ground velocity, which the spring then builds up again [design: the owner,
    /// 2026-11-14, "landing compression"]. A hop (about 290 units/s) takes none.
    pub fn land(&mut self, fall_speed: f32) {
        let hard = crate::ease::smoothstep(LANDING_SOFT_SPEED, LANDING_HARD_SPEED, fall_speed);
        let keep = 1.0 - LANDING_MOST_LOST * hard * self.weight();
        for s in &mut self.axes {
            s.value *= keep;
            s.rate = 0.0;
        }
    }

    /// After a frame's steps.
    pub fn end_frame(&mut self, feet: [f32; 3], grounded: bool) {
        self.last_feet = Some(feet);
        self.grounded = grounded;
    }

    /// The movement the clips play for the keys held (`forward`, `right` as in
    /// `crate::player::MoveIntent`, `walk`) at the compass `heading`: the keys', but while
    /// the body glides on the ground with no key held or brakes against the keys, the way
    /// it slides, walking or running as it last moved.
    pub fn clip_intent(&self, forward: f32, right: f32, walk: bool, heading: f32) -> (f32, f32, bool) {
        let v = self.velocity();
        let speed = v[0].hypot(v[1]);
        if !self.ground() || !self.grounded || speed < GLIDE_CLIP_SPEED {
            return (forward, right, walk);
        }
        let (s, c) = heading.sin_cos();
        // Forward is (sin h, cos h), right (cos h, −sin h) (`crate::player::velocity`).
        let along = (v[0] * s + v[1] * c) / speed;
        let across = (v[0] * c - v[1] * s) / speed;
        let keys = forward != 0.0 || right != 0.0;
        let against = along * forward + across * right < 0.0;
        if keys && !against {
            return (forward, right, walk);
        }
        (along, across, !self.ran)
    }
}


/// Log Advanced Movement's switches kept here at the start and whenever one changes (a smoke
/// check of the defaults: the owner, 2026-11-16, every part off but Advanced Recoil).
pub fn log_switches(inertia: Res<MovementInertia>, mut last: Local<Option<[bool; 7]>>) {
    let now = [inertia.advanced, inertia.on, inertia.weapon_handling, inertia.recoil, inertia.smooth, inertia.sprint, inertia.motion_matching];
    if *last != Some(now) {
        *last = Some(now);
        let s = |b: bool| if b { "on" } else { "off" };
        info!(
            "advanced movement: Global Toggle {}, Movement Inertia {}, Weapon Handling {}, Advanced Recoil {}, Animation Smoothing {}, Sprint {}, Motion Matching {}",
            s(now[0]),
            s(now[1]),
            s(now[2]),
            s(now[3]),
            s(now[4]),
            s(now[5]),
            s(now[6])
        );
    }
}

/// Keep [`MovementInertia::handling`] and the weapon's weight on the weapon the player holds
/// (or is drawing). Logs the draw's and the aim's pace when they change (a smoke check).
pub fn follow_weapon(mut inertia: ResMut<MovementInertia>, combat: Option<Res<crate::combat::Combat>>, mut logged: Local<Option<(f32, f32)>>) {
    let weight = combat.as_deref().and_then(|c| c.forms.weapons.get(&c.player_weapon()?).map(|w| w.weight));
    let handling = match (inertia.handles(), weight) {
        (true, Some(w)) => handling(w),
        _ => 1.0,
    };
    if (inertia.handling - handling).abs() > 1e-4 {
        inertia.handling = handling;
    }
    if inertia.weapon_weight != weight {
        inertia.weapon_weight = weight;
    }
    let now = (inertia.equip_rate(), inertia.aim_seconds(true));
    if logged.is_none_or(|l| (l.0 - now.0).abs() > 1e-3 || (l.1 - now.1).abs() > 1e-3) {
        *logged = Some(now);
        info!(
            "weapon handling: draws at x{:.2} before perks, aims in {:.2}s ({} lb, Agility {:.0}, Strength {:.0}, Movement Weight {}, Weapon Handling {})",
            now.0,
            now.1,
            weight.map_or("-".to_string(), |w| format!("{w:.1}")),
            inertia.agility,
            inertia.strength,
            movement_weight_name(inertia.weight()),
            if inertia.handles() { "on" } else { "off" }
        );
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;
    const RUN: f32 = 344.0;

    fn steps(m: &mut MovementInertia, key: [f32; 2], seconds: f32, dt: f32) -> [f32; 2] {
        let n = (seconds / dt).round() as usize;
        let mut v = m.velocity();
        for _ in 0..n {
            v = m.step(key, false, dt);
        }
        v
    }

    #[test]
    fn starts_gently_and_reaches_speed_in_time() {
        let mut m = MovementInertia::all_on();
        let first = m.step([0.0, RUN], false, DT);
        assert!(first[1] > 0.0 && first[1] < 0.2 * RUN, "a gentle first step: {first:?}");
        let v = steps(&mut m, [0.0, RUN], START_SECONDS - DT, DT);
        assert!(v[1] > 0.93 * RUN && v[1] <= RUN, "{v:?}");
        let v = steps(&mut m, [0.0, RUN], 1.5, DT);
        assert_eq!(v, [0.0, RUN], "then exactly the keys' speed");
    }

    #[test]
    fn walking_has_inertia_too() {
        let walk = 86.8;
        let mut m = MovementInertia::all_on();
        let v = steps(&mut m, [0.0, walk], 0.1, DT);
        assert!(v[1] < 0.3 * walk, "not at walking speed at once: {v:?}");
        steps(&mut m, [0.0, walk], 2.0, DT);
        let v = steps(&mut m, [0.0, 0.0], 0.1, DT);
        assert!(v[1] > 0.3 * walk, "still walking on after letting go: {v:?}");
    }

    #[test]
    fn glides_to_a_stop_creeping_at_the_end_and_ends_at_rest() {
        let mut m = MovementInertia::all_on();
        steps(&mut m, [RUN, 0.0], 2.0, DT);
        let v = m.step([0.0, 0.0], false, DT);
        assert!(v[0] > 0.95 * RUN, "still moving just after the key is let go: {v:?}");
        let v = steps(&mut m, [0.0, 0.0], STOP_SECONDS - DT, DT);
        assert!(v[0] > 0.0 && v[0] < 0.07 * RUN, "{v:?}");
        // The last of it eases out: still moving a moment later, slowly.
        let v = steps(&mut m, [0.0, 0.0], 0.15, DT);
        assert!(v[0] > 0.3 && v[0] < 0.02 * RUN, "{v:?}");
        let v = steps(&mut m, [0.0, 0.0], 2.0, DT);
        assert_eq!(v, [0.0, 0.0], "at rest exactly");
    }

    #[test]
    fn the_start_takes_its_time_and_the_main_switch_rules_every_switch() {
        let mut m = MovementInertia::all_on();
        let mut t = 0.0;
        while m.step([0.0, RUN], false, DT / 4.0)[1] < 0.95 * RUN {
            t += DT / 4.0;
        }
        assert!((t - START_SECONDS).abs() < 0.01, "{t}");
        let m = MovementInertia::all_on();
        assert!(m.ground() && m.eased_crouch() && m.landing_weight() && m.smooth_hands() && m.handles() && m.soft_recoil(), "all on by default");
        let off = MovementInertia { advanced: false, ..MovementInertia::all_on() };
        assert!(!off.ground() && !off.eased_crouch() && !off.smooth_hands() && !off.handles() && !off.soft_recoil() && !off.landing_weight());
        assert!(off.on && off.smooth && off.recoil && off.weapon_handling, "each keeps its own choice under it");
        // Movement Inertia carries the crouch and the landings with it.
        let still = MovementInertia { on: false, ..MovementInertia::all_on() };
        assert!(!still.ground() && !still.eased_crouch() && !still.landing_weight() && still.smooth_hands());
        // Without Weapon Handling the aim keeps the game's pace, the weapon's weight none.
        let plain = MovementInertia { weapon_handling: false, weapon_weight: Some(20.0), agility: 10.0, ..MovementInertia::all_on() };
        assert_eq!((plain.aim_seconds(true), plain.aim_seconds(false)), (AIM_IN_PLAIN_SECONDS, AIM_OUT_PLAIN_SECONDS));
        let heavy = MovementInertia { weapon_weight: Some(20.0), ..MovementInertia::all_on() };
        assert!((heavy.aim_seconds(true) - AIM_IN_SECONDS * 1.35).abs() < 1e-5);
        let pistol = MovementInertia { weapon_weight: Some(3.0), ..MovementInertia::all_on() };
        assert!((pistol.aim_seconds(true) - AIM_IN_SECONDS).abs() < 1e-5 && (pistol.aim_seconds(false) - AIM_OUT_SECONDS).abs() < 1e-5, "a pistol keeps the tuned aim");
    }

    #[test]
    fn sprint_is_off_by_default_and_needs_the_global_toggle() {
        let m = MovementInertia::default();
        assert!(!m.sprint && !m.sprint_on(), "off by default");
        let on = MovementInertia { sprint: true, ..MovementInertia::default() };
        assert!(on.sprint_on());
        let off = MovementInertia { advanced: false, ..on };
        assert!(!off.sprint_on() && off.sprint, "it keeps its own choice under the Global Toggle");
    }

    #[test]
    fn draws_follow_the_attributes_by_the_weapons_class() {
        let at = |weight: f32, agility: f32, strength: f32| MovementInertia { weapon_weight: Some(weight), agility, strength, ..MovementInertia::all_on() };
        // The original's Agility rate without Weapon Handling, whatever the weapon.
        for (agility, rate) in [(1.0, 0.6), (5.0, 1.0), (10.0, 1.5)] {
            let off = MovementInertia { weapon_handling: false, ..at(25.0, agility, 1.0) };
            assert!((off.equip_rate() - rate).abs() < 1e-5, "{agility}: {}", off.equip_rate());
            let all_off = MovementInertia { advanced: false, ..at(2.0, agility, 10.0) };
            assert!((all_off.equip_rate() - rate).abs() < 1e-5);
        }
        // At 5 and 5 only the weight: light 1.5 times the original's time, a rifle 1.8, heavy 2.4.
        assert!((at(1.0, 5.0, 5.0).equip_rate() - 1.0 / 1.5).abs() < 1e-5);
        assert!((at(8.0, 5.0, 5.0).equip_rate() - 1.0 / 1.8).abs() < 1e-5);
        assert!((at(25.0, 5.0, 5.0).equip_rate() - 1.0 / 2.4).abs() < 1e-5);
        assert_eq!(weight_class(2.0), [1.0, 0.0, 0.0]);
        assert_eq!(weight_class(9.0), [0.0, 1.0, 0.0]);
        assert_eq!(weight_class(30.0), [0.0, 0.0, 1.0]);
        // Agility counts most for light weapons, Strength most for heavy ones.
        let gain = |w: f32, a: f32, s: f32| attribute_rate(w, a, s) - 1.0;
        assert!((gain(2.0, 10.0, 5.0) - 0.5).abs() < 1e-5, "a light weapon at Agility 10 as the original's");
        assert!(gain(2.0, 10.0, 5.0) > gain(9.0, 10.0, 5.0) && gain(9.0, 10.0, 5.0) > gain(30.0, 10.0, 5.0));
        assert!(gain(30.0, 5.0, 10.0) > gain(9.0, 5.0, 10.0) && gain(9.0, 5.0, 10.0) > gain(2.0, 5.0, 10.0));
        assert!((gain(30.0, 5.0, 10.0) - 0.5).abs() < 1e-5);
        assert_eq!(attribute_rate(30.0, 1.0, 1.0), ATTRIBUTE_RATE_MIN, "two low attributes cost no more than the original's Agility 1");
        assert!(attribute_rate(9.0, 3.0, 4.0) > ATTRIBUTE_RATE_MIN && attribute_rate(9.0, 3.0, 4.0) < 1.0);
        // Never quicker than the original's at the same Agility, whatever the Strength and
        // the Movement Weight.
        for weight in [0.5, 2.0, 8.0, 15.0, 30.0] {
            for (agility, strength) in [(1.0, 10.0), (5.0, 10.0), (10.0, 10.0), (1.0, 1.0)] {
                for share in [0.25, 1.0] {
                    let m = MovementInertia { weight: share, ..at(weight, agility, strength) };
                    let original = fnv_combat::attack::reload_rate(agility);
                    assert!(m.equip_rate() <= original + 1e-5, "{weight} lb, {agility}/{strength}, {share}: {} over {original}", m.equip_rate());
                    assert!(m.equip_rate().is_finite() && m.equip_rate() > 0.0);
                }
            }
        }
        assert_eq!(attribute_rate(2.0, 0.0, 99.0), attribute_rate(2.0, 1.0, 10.0), "read within 1 to 10");
        // A light weapon at Agility 10 draws in the original's time at 5 (1.5 over 1.5).
        assert!((at(2.0, 10.0, 5.0).equip_rate() - 1.0).abs() < 1e-5);
        // The aim takes a small share: never ultra quick, never very slow.
        let quick = at(1.0, 10.0, 10.0).aim_seconds(true);
        assert!(quick > AIM_IN_PLAIN_SECONDS && quick < AIM_IN_SECONDS, "{quick}");
        assert!((aim_rate(1.0, 10.0, 10.0) - 1.26).abs() < 1e-4);
        let slow = at(30.0, 1.0, 1.0).aim_seconds(true);
        assert!(slow < 1.2, "{slow}");
    }

    #[test]
    fn movement_weight_scales_the_inertia_and_the_extra_handling_only() {
        let light = MovementInertia { weight: 0.25, ..MovementInertia::all_on() };
        // The start four times as quick.
        let mut m = light.clone();
        let mut t = 0.0;
        while m.step([0.0, RUN], false, DT / 4.0)[1] < 0.95 * RUN {
            t += DT / 4.0;
        }
        assert!((t - START_SECONDS / 4.0).abs() < 0.01, "{t}");
        assert_eq!(light.crouch_seconds(true), CROUCH_DOWN_SECONDS / 4.0);
        assert!((light.land_stretch(2000.0) - (1.0 + (HARD_LAND_STRETCH - 1.0) / 4.0)).abs() < 1e-5);
        let mut m = MovementInertia { weight: 0.25, ..MovementInertia::all_on() };
        m.follow([0.0, RUN]);
        m.land(LANDING_HARD_SPEED);
        assert!((m.velocity()[1] - RUN * (1.0 - LANDING_MOST_LOST / 4.0)).abs() < 1e-3);
        // Weapon Handling's time over the original's shrinks; the original's stays.
        let rifle = |weight: f32| MovementInertia { weight, weapon_weight: Some(8.0), ..MovementInertia::all_on() };
        assert!((1.0 / rifle(0.25).equip_rate() - (1.0 + 0.8 / 4.0)).abs() < 1e-4);
        assert!(rifle(0.25).equip_rate() <= 1.0, "never quicker than the original's at 5 and 5");
        let aim = rifle(0.25).aim_seconds(true);
        assert!(aim > AIM_IN_PLAIN_SECONDS && aim < rifle(1.0).aim_seconds(true));
        // The dropdown's names and numbers.
        assert_eq!(movement_weight_by_name("25%"), Some(0.25));
        assert_eq!(movement_weight_by_name("75"), Some(0.75));
        assert_eq!(movement_weight_by_name("0.5"), Some(0.5));
        assert_eq!(movement_weight_by_name("60%"), None);
        assert_eq!(movement_weight_by_name("1%"), None, "a percent sign means a percent");
        assert_eq!(movement_weight_by_name("1"), Some(1.0));
        assert_eq!(movement_weight_name(1.0), "100%");
        assert_eq!(movement_weight_name(0.3), "25%");
        assert_eq!(MovementInertia { weight: 0.0, ..MovementInertia::all_on() }.weight(), 0.25, "held within the choices");
    }

    #[test]
    fn turning_back_brakes_first_and_never_overshoots() {
        let mut m = MovementInertia::all_on();
        steps(&mut m, [0.0, RUN], 2.0, DT);
        let mut last = RUN;
        let mut crossed = false;
        for _ in 0..300 {
            let v = m.step([0.0, -RUN], false, DT);
            assert!(v[1] <= last + 1e-3, "braking and then speeding up the other way only: {last} then {}", v[1]);
            assert!(v[1] >= -RUN - 1e-3, "never faster than asked: {v:?}");
            crossed |= v[1] < 0.0;
            last = v[1];
        }
        assert!(crossed);
        assert_eq!(m.velocity(), [0.0, -RUN]);
    }

    #[test]
    fn the_same_at_any_tick_rate() {
        // 1/12 s is a whole number of steps at each rate.
        for hz in [120.0, 144.0, 240.0] {
            let mut a = MovementInertia::all_on();
            let mut b = MovementInertia::all_on();
            let x = steps(&mut a, [RUN, 0.0], 1.0 / 12.0, DT)[0];
            let y = steps(&mut b, [RUN, 0.0], 1.0 / 12.0, 1.0 / hz)[0];
            assert!((x - y).abs() < 0.05, "{hz} Hz: {y} against {x}");
        }
    }

    #[test]
    fn the_air_hands_its_speed_to_the_ground_and_a_placement_starts_from_rest() {
        let mut m = MovementInertia::all_on();
        m.follow([0.0, 300.0]);
        let v = m.step([0.0, 0.0], false, DT);
        assert!(v[1] > 0.9 * 300.0, "lands gliding: {v:?}");
        m.end_frame([0.0, 0.0, 0.0], true);
        m.check_placed([10.0, 0.0, 0.0]);
        assert!(m.velocity()[1] > 0.0, "walking distance keeps it");
        m.check_placed([5000.0, 0.0, 0.0]);
        assert_eq!(m.velocity(), [0.0, 0.0]);
        // Put somewhere by the game, however near: from rest; the same place again keeps on.
        m.follow([0.0, 300.0]);
        m.check_game_placed(Some([10.0, 0.0, 0.0]));
        assert_eq!(m.velocity(), [0.0, 0.0]);
        m.follow([0.0, 300.0]);
        m.check_game_placed(Some([10.0, 0.0, 0.0]));
        m.check_game_placed(None);
        assert_eq!(m.velocity(), [0.0, 300.0], "walking on after it");
    }

    #[test]
    fn starts_and_glides_are_timed_for_the_log() {
        let mut m = MovementInertia::all_on();
        steps(&mut m, [0.0, RUN], 2.0, DT);
        steps(&mut m, [0.0, 0.0], 4.0, DT);
        let [Report::Started { seconds, speed }, Report::Stopped { seconds: stop, distance, speed: from }] = m.reports[..] else {
            panic!("{:?}", m.reports);
        };
        assert!((seconds - START_SECONDS).abs() < 0.03 && speed == RUN, "{seconds} {speed}");
        // A critically damped glide from v covers 2v/ω: about 36 units from a run.
        let glide = 2.0 * RUN / omega_for(STOP_SECONDS);
        assert!((distance - glide).abs() < 1.0 && (30.0..45.0).contains(&distance), "{distance} against {glide}");
        assert!(stop > 1.5 * STOP_SECONDS && stop < 1.0 && from == RUN, "{stop} {from}");
        assert!((STOP_SECONDS * 2.0 - START_SECONDS).abs() < 1e-6, "stopping takes half the start (the owner)");
        // Let go before full speed: no start reported, the glide still is.
        let mut m = MovementInertia::all_on();
        steps(&mut m, [0.0, RUN], 0.2, DT);
        steps(&mut m, [0.0, 0.0], 4.0, DT);
        assert!(matches!(m.reports[..], [Report::Stopped { .. }]), "{:?}", m.reports);
    }

    #[test]
    fn heavy_weapons_handle_slower_and_hard_landings_take_longer() {
        assert!((handling(0.5) - 0.85).abs() < 1e-4);
        assert!((handling(3.0) - 1.0).abs() < 1e-4, "the 10mm pistol keeps the tuned times");
        assert!((handling(20.0) - 1.5).abs() < 1e-4);
        assert!(handling(7.0) > 1.0 && handling(7.0) < handling(15.0));
        assert_eq!(land_stretch(290.0), LAND_STRETCH, "a hop's landing as before");
        assert_eq!(land_stretch(2000.0), HARD_LAND_STRETCH);
        // The landing clip starts at its own pace and comes to rest at its end.
        for s in [1.2, LAND_STRETCH, HARD_LAND_STRETCH] {
            let du = 1e-4;
            let start = land_curve(du, s) / du / s;
            assert!((start - 1.0).abs() < 1e-2, "stretch {s}: the clip starts at x{start} its own pace");
            let end = (1.0 - land_curve(1.0 - du, s)) / du / s;
            assert!(end < if s >= LAND_STRETCH { 0.01 } else { 0.2 }, "stretch {s}: it ends at x{end} its pace");
            for k in 0..=20 {
                let y = k as f32 / 20.0;
                assert!((land_curve(land_curve_inv(y, s), s) - y).abs() < 1e-4);
            }
        }
        assert_eq!(hard_landing(291.0), 291.0, "a hop pushes as before");
        assert!((hard_landing(-1200.0) + 1200.0 * HARD_LANDING_GAIN).abs() < 1e-2);
        let m = MovementInertia { handling: 1.5, ..MovementInertia::all_on() };
        assert_eq!(m.hands(), 1.5);
        let m = MovementInertia { handling: 1.5, weight: 0.5, ..MovementInertia::all_on() };
        assert_eq!(m.hands(), 1.25, "the part over 1 at the Movement Weight");
        let m = MovementInertia { handling: 0.85, weight: 0.25, ..MovementInertia::all_on() };
        assert_eq!(m.hands(), 0.85, "a light weapon's quicker hand-back stays");
    }

    #[test]
    fn a_hard_landing_takes_speed_and_it_builds_up_again() {
        let mut m = MovementInertia::all_on();
        m.follow([0.0, RUN]);
        m.land(290.0);
        assert_eq!(m.velocity(), [0.0, RUN], "a hop keeps its speed");
        m.land(LANDING_HARD_SPEED);
        assert!((m.velocity()[1] - RUN * (1.0 - LANDING_MOST_LOST)).abs() < 1e-3);
        let v = steps(&mut m, [0.0, RUN], 2.0, DT);
        assert_eq!(v, [0.0, RUN], "back up to speed");
    }

    #[test]
    fn the_clips_follow_a_glide_and_the_keys_otherwise() {
        let mut m = MovementInertia::all_on();
        // Heading north; running north.
        steps(&mut m, [0.0, RUN], 2.0, DT);
        assert_eq!(m.clip_intent(1.0, 0.0, false, 0.0), (1.0, 0.0, false), "the keys while they agree");
        m.step([0.0, 0.0], false, DT);
        let (f, r, walk) = m.clip_intent(0.0, 0.0, true, 0.0);
        assert!((f - 1.0).abs() < 1e-4 && r.abs() < 1e-4 && !walk, "gliding forward, running: {f} {r} {walk}");
        // Facing east (π/2), the northward glide is to the left.
        let (f, r, _) = m.clip_intent(0.0, 0.0, false, std::f32::consts::FRAC_PI_2);
        assert!(f.abs() < 1e-4 && (r + 1.0).abs() < 1e-4, "{f} {r}");
        // Braking against Back: the clips still go forward.
        let (f, _, _) = m.clip_intent(-1.0, 0.0, false, 0.0);
        assert!(f > 0.9);
        // Stopped, in the air, or switched off: the keys.
        m.grounded = false;
        assert_eq!(m.clip_intent(0.0, 0.0, false, 0.0), (0.0, 0.0, false));
        m.grounded = true;
        m.on = false;
        assert_eq!(m.clip_intent(0.0, 0.0, false, 0.0), (0.0, 0.0, false));
        m.on = true;
        m.advanced = false;
        assert_eq!(m.clip_intent(0.0, 0.0, false, 0.0), (0.0, 0.0, false), "the main switch off");
        m.advanced = true;
        steps(&mut m, [0.0, 0.0], 3.0, DT);
        assert_eq!(m.clip_intent(0.0, 0.0, false, 0.0), (0.0, 0.0, false));
    }
}
