# Modern Feel: grounded footsteps and NPC aim

Part of the fork owner's Modern Feel experiment (`nvf_play::modern_feel`, NVF Features ›
Modern Feel). Both parts are NVForever's own design, not the original's behaviour; every
constant is an engineering choice tuned by eye [design]. Code: `engine/nvforever/src/npc_feel.rs`
(one system after `locomotion::drive`, before head tracking and foot IK), with small hooks in
`locomotion.rs` (`Locomotion::feel_turn`), `fnv_actor::locomotion::Blend::stride_match` and
`npc_combat.rs` (`Fighters::aim_at`). Switches: `ModernFeel::grounded_footsteps()` and
`ModernFeel::npc_aim()`; both off (or the Global Toggle off) the bones ease back to the
animation and the clip choice is the original's again.

## Grounded footsteps (actors other than the player)

What slid before [observed in NVForever]:

- A fighter locked on its target (`residents.rs`, `LOCK_HOLD`) follows it exactly, so a
  target strafing past turns the whole standing body a few degrees a frame: under 15°/s the
  idle plays and the body spins on its feet; over it the turn clip plays at rate 1 whatever
  the speed (the turn clips carry no rotation, actors.md §8.9).
- A fighter strafing on a diagonal plays the nearest of its four direction clips, and its
  feet slide sideways by the rest of the angle (up to 45°).
- During a walk/run cross-fade both clips advance at the weight-blended cycle rate, so the
  blended stride covers more or less ground than the actor moves.

What it does now:

- **Planted turns.** A standing person's lower body keeps its heading while the root turns:
  the root's child (`Bip01`) is turned back by the lag about the root's vertical axis, so
  the feet stay put, and the three spine bones turn the upper body round to the root's
  heading (shared evenly, at most `UPPER_MAX` 50°). Once the lag passes `START_LAG` (25°),
  or the root has been still for `SETTLE_TIME` (0.35 s) with over `SETTLE_LAG` (8°) left,
  the feet step round: the lag closes at 4 × itself per second (100–270°/s) and the turn
  clip plays at the rate for the lower body's own turn speed (`turn_clip_rate`: the clips
  count as 120°/s at rate 1, kept within 0.7–1.8). A lag over `MAX_LAG` (60°) is dragged.
- **Feet along the way they travel.** Moving, the lower body is turned by the angle between
  the velocity and the playing clip's direction (at most 50°, eased at 10/s); the spine
  turns the upper body back.
- **Stride-matched fades** (`Blend::stride_match`): during a walk/run fade both clips
  advance at speed ÷ the weight-blended stride (root speed × length per cycle). Outside a
  fade the rate already matched the ground speed (`choose`, 0.4–2.5).
- Only people (`NPC_`/`LVLN` with thighs and a `Bip01 Spine` chain), standing on the ground,
  not on furniture, alive, within 3,000 units of the camera.

## NPC aim

A fighting gunman (`Fighters::aim_at`: its target's chest, where its shots go) with its gun
drawn, not reloading, blocking or striking, has its spine bent on top of the animation so
the barrel (the weapon model's +X through the `Muzzle`) points at that point. The aim
clips' up and down forms (`attack_anims.rs`) still do the coarse pitch; the bend takes out
what is left: the clip's own offset, the planted turn's twist, the distance between the
eyes' line and the muzzle, a target above or below, crouched or on a slope. Solved in actor
space: three passes of "measure barrel and muzzle by forward kinematics, add the yaw and
elevation still missing", the yaw about the vertical and the pitch about the level axis
across the way to the target, each shared evenly over `Bip01 Spine`, `Spine1`, `Spine2`
(each bone turns about its own pivot). Limits ±40° yaw and pitch; followed at 14/s,
faded in and out at 5/s. The weapon arm is not bent: the spine was enough in the runs below.

## Measured

Log lines `grounded footsteps:` and `npc aim:` (every 10 s and at exit) count the frames
worked on, steps, foot slip against the ground speed (old blend versus stride-matched) and
the barrel's angle from its target before and after the bend. Test aid
`NVF_PLAY_TEST_FEEL_NPC_OFF=1` turns both parts off for a run (the aim is still measured,
nothing bent), for screenshots side by side.

Run 2026-11-21 [observed in NVForever]: Goodsprings, Violet (FiendVioletRef, Hunting Rifle)
set on the player in god mode, the player 100 units in front of her and strafing, 32 s, once
with the parts on and once with the test aid (fights differ run to run):

| | on | test aid off |
|---|---|---|
| Barrel off its target, mean (worst) | 22.9° before → **5.7°** after (35°) | 17.7° (67°) |
| Frames within 2° | 39 % | 14 % |
| Frames at the bend's limit | 86 of 1,769 | – |
| Planted standing frames / steps | 1,896 / 5 (lag up to 58°) | – |
| Foot slip against ground speed, moving | 36.7 % → 25.7 % | – |
| … during walk/run fades (494 frames) | 83 % → 52 % | – |

The remaining aim error is mostly the bend following (14/s) while she moves and turns; held
still it settles within 1–2°. The fades' remaining slip comes from fades between clips of
different directions and from rates at the 0.4/2.5 clamp, which stride matching does not
change.

## Cost

Per actor and frame within 3,000 units: a walk up the skeleton to the weapon node (about a
dozen transforms) four times for an aiming gunman, once for a planted turn; nothing for an
actor at rest. Measured: 16–22 µs per frame for the whole system in the run above (one gunman, ~20 actors near).

## Rough or left out

- The arm is never bent; a gun held far off the chest's line (a pistol one-handed at the
  hip) relies on the spine alone.
- Turn clips have no turn angle of their own, so "planted" during a step is approximate:
  the feet step while the lower body turns at the clip's pace, as the original's turns do.
- Creatures keep the original turns (their spines and legs are not a person's).
- The barrel axis is the weapon model's +X, as the player's iron sights take it
  (`player_body.rs`); a model built otherwise would aim off.
