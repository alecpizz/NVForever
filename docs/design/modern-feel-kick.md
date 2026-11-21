# Modern Feel: camera kick and damage direction

Part of the fork owner's Modern Feel experiment (2026-11-20, `nvf_play::modern_feel`,
NVF Features › Modern Feel). Everything here is NVForever's own [design]; every constant is
an engineering choice tuned by eye. None of it is the original's behaviour, and with the
switches off (`ModernFeel::camera_kick()`, `ModernFeel::damage_direction()`) nothing of it
acts.

## Camera kick (`engine/nvf-play/src/camera_kick.rs`)

**What it does.** Each of the player's shots (`combat::ShotFired` from the player, guns and
launchers; not thrown weapons or mines) punches the view: pitch up, a random turn aside and
a small roll, on critically damped springs (`ease::Spring`) set moving by an impulse, so the
view peaks about 50 ms after the shot and is back within about a quarter of a second. A
heavy shot also widens the field of view for a moment (out and back in about 0.15 s).

**Only the view.** Aim stays the game's rules: `combat`'s own kick, spread and bloom are not
touched, and no shot reads the camera's transform. The punch turns the camera after
`control::place_camera` (beside the explosion shake, `shake.rs`; both multiply small turns
onto the placed rotation, so their order does not matter). In first person nvforever's
player body turns the arms rig by the same turn about the eye
(`CameraKick::arms_rotation`, composed onto the view sway's pose in `player_body.rs`), so
the gun stays where it is on the screen and the world kicks; Advanced Recoil's kick of the
gun (`view_sway.rs` RECOIL_*) and the head bob's nudge back (`head_bob.rs`, a move of the
camera's place) lie on top. The 3D scope's lens hangs from the gun, so it turns with it.

**By the weapon** (`kick_for`, from WEAP DATA/DNAM and the projectile):

| | |
|---|---|
| strength | `sqrt(damage / 40)`, the whole shot's damage |
| weight | × up to 1.25 from 4 to 20 lb |
| launcher, or an exploding projectile | at least 1.4, × 1.3 |
| shotgun (more than one projectile) | × 1.35 |
| energy weapon | × 0.75 |
| two-handed | × 1.1 |
| automatic | × 0.6 × sqrt(4 / fire rate), stiffer springs (ω 27 against 19), no FOV punch |
| limits | 0.2 to 2.4 |

At strength 1: 1.3° up, up to 0.35° aside, up to 0.9° roll. The FOV punch starts at strength
1.15 and is full (4°) at 2. Examples (FalloutNV.esm, `fnvtool combat weapon`): 9mm pistol
0.63 (0.8° up), .44 Magnum 0.95, hunting rifle 1.25, hunting shotgun about 2 (2.6° up, 4°
FOV), anti-materiel rifle and missile launcher at the 2.4 cap, 10mm SMG about 0.3, minigun
0.2. Soft limits for sustained fire: 4.5° up, 2° aside, 3° roll.

**Calmer**: down the sights by the zoom's eased share (to 45 %), a scope up 30 %, sneaking
75 %, third person 50 % and no FOV punch. None in VATS, a kill camera, the free camera; the
springs rest when the switch goes off.

**Field of view.** Widened in `PostUpdate` before `CameraUpdateSystems` and put back in
`First` (only if nobody else wrote it since), so `aim_zoom`, `dialogue_zoom` and the console
never see the punch as a new base angle.

**Cost.** Per frame: four spring steps and one quaternion multiply; a `Projection` write
only during a punch. Unmeasurable against the frame.

## Damage direction (`engine/nvf-play/src/damage_direction.rs`)

**The mark.** A hit on the player with an attacker (`combat::hit_player` pushes who struck
and the share of health taken onto `HitFeedback::player_struck`, a new field; no
`ActorHit` is written for the player) puts an arc around the crosshair on the side the attacker is from the view
(ahead at the top, behind at the bottom), recomputed every frame from the attacker's
current place (`SimState`, its chest 90 units over its feet), so it turns as the player
turns. It comes in over 60 ms and fades over up to 1.5 s (shorter for weak hits); its
strength (alpha) is 0.7 + 0.3 × smoothstep(0, 0.25, share of health). A new hit by the
same attacker renews its arc; up to 6 at once (the oldest goes). The picture (a 256² ring
segment, ±24°, soft edges, brightest in the middle) is made in code; it is tinted with the
HUD colour (`interface::hud_rgb`), drawn with the HUD (hidden when `HudView::shown` is
false), at radius 118 interface units, following the grenade indicator's system (a 0×0
positioning node turned with `UiTransform`, its picture a child).

**The kill tick.** When `ActorHit` says the player's attack killed an actor, a short (0.14 s)
quiet "tock" plays: a sine falling from 880 to 620 Hz with a shorter overtone and a faint
click, synthesized at startup of the first kill (`kill_tick`, no shipped file), on the
effects volume (`SoundCategory::Effects`) at 0.35.

**Cost.** Six UI nodes, made at the first hit; per frame a transform inverse and an atan2
per live mark.

## Verified

Smoke scenario `modern-feel-kick` (two Goodsprings dogs, the hunting shotgun): the
`camera kick: weapon …` and `damage direction: hit by …` lines and screenshots of the arcs.

## Rough edges

- The arc is centred on the screen; in third person the crosshair can sit off the middle.
- Hits with no attacker (falls, traps, the environment) mark nothing.
- Kick constants are tuned by eye on a few weapons; weapon mods (a compensator) do not
  change the kick.
- The camera punch reads last frame's shots (combat fires after the camera is placed): one
  frame late.
