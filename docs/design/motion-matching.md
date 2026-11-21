# Motion matching for the player's locomotion (experiment, fork only)

Status: built 2026-11-20, branch `feat/motion-matching`. Not a parity feature: the original
picks clips by state [observed: actors.md §8.3, player-body.md]; this replaces that choice for
the third-person player's walking, running and turning when **NVF Features › Visual Only ›
Motion Matching** (`motion-matching`, `--motion-matching`) is on. Off by default. Aim: grounded
movement (the fork's owner: "Last of Us Part 2 style").

## 1. Idea

Every frame of every locomotion motion in the player's current set is a row of a
**database**, with a **feature vector**: the feet's places and velocities, the hips' velocity,
and where the character will be over the next second if that motion plays on. Each moment the
game builds the same vector from **the pose playing now plus the trajectory the controls ask
for** and searches for the nearest row. A jump to another motion or moment is hidden by
**inertialization** [documented: Clavet, "Motion Matching and The Road to Next-Gen
Animation", GDC 2016; Holden, "Learned Motion Matching", SIGGRAPH 2020, §3; Holden's articles
on springs and inertialization, theorangeduck.com; Bollo, "Inertialization", GDC 2018].

Grounding takes three things besides the choice of frame (§§6-8): **the legs set the ground
speed** (the owner's choice, 2026-11-20: "animation shapes speed"), **each clip's speed is
read from its planted feet**, and **planted feet are locked** to the ground.

## 2. What stays as it is

- **Upper-body layers** (aim, attack, draw, holster, reload) stay above locomotion in the
  `Animator` by priority. The idle stays under everything, as without matching.
- **Jump and landing clips**, **swimming**, **special idles** and **first person**: the
  original choice (the matcher pauses and starts afresh after).
- **NPCs**: unchanged (`BlendPlayer`/`Locomotion`).
- **Direction** comes from the keys; only the **speed** on the ground follows the legs (§6).

## 3. The library

[observed, 2026-11-20, `fnvtool nif anim` over `Fallout - Meshes.bsa`, `_male\locomotion\`]:
every player locomotion clip is a **loop**: idle (4.0 s), walk 4 directions (1.13-1.27 s),
run 4 directions (0.73-0.80 s), turn left/right (1.0 s), per weapon prefix and stance. There
are no starts, stops, plants or pivots, and no New Vegas mod found adds them (searched
2026-11-20). The turning clips do not turn `Bip01` (0°/s): they step on the spot.

[observed: `_male\skeleton.nif`] `Bip01`'s bind turns a quarter about z at (0, 0.66, 67.8);
the animator holds it at the identity and the clips carry the turn on `Bip01 NonAccum`. The
bones pair `Bip01 L …`/`Bip01 R …`, `LPauldron`/`RPauldron`, `LUpArmTwistBone`/
`RUpArmTwistBone`; `R ForeTwistDriver` has no partner.

[observed: the matcher's log, 2026-11-20] **The clips' root travel is not their feet's.**
Measured from the planted feet (§5), the unarmed standing set: walk 66-80 units/s (roots
×0.88-0.97), run forward 303 (×0.85), run back 195 (×0.69), run left 307 (×0.87), **run right
121 (×0.34)**. Played at its root's speed, as the original plays it, the original's run
right slides its planted feet about 240 units/s, its run forward about 100 (the foot skate
trace, `motion-matching-inertia-off`).

## 4. The motions and the database

A **motion** (`nvf_assets::motion_match::Motion`) is what the animator plays as one: clips on
one shared phase (`Mode::Synced`) at fixed weights. Per clip set (posture × body × weapon
drawn × limping; `player_body/motion_matching.rs`):

- the idle;
- 32 movement motions: walk and run × 16 directions (every 22.5°), the forward/backward clip
  blended with the sideways one by `direction_weights` (the original's diagonal blend), their
  left footfalls lined up by their `Enum: Left` keys (`Layer::phase_offset`). No walk-run
  blends: a walk and a run plant their feet at different shares of their cycles, so the
  blend's foot is never still (planted feet slid 110-200 units/s) [observed];
- the two turning clips, standing for a turn of 90°/s (`Motion::implied_turn`) [design];
- **every one of those mirrored** (§5): left for right, the other foot leading;
- every `.kf` in the skeleton's `locomotion\motionmatching\` folder (§10), not mirrored.

Rows every 1/30 s: 2,268 rows from 70 motions for the unarmed standing set, built in 71 ms on
a thread of its own and kept for the session [observed]. The rows' poses come from a scratch
`Animator` playing the motion exactly as the body will.

## 5. Two corrections to the clips, at build

- **Speed from the feet** (`contact_speed`, `Motion::travel_scale`): each foot within 2 units
  of its lowest is on the ground; the median speed at which such feet move under the body is
  the motion's ground speed, and its root travel is scaled to it (0.3-3×). The playback rate,
  the legs' pace (§6) and the trajectory features all use it.
- **Mirroring** (`mirror_clip`): each frame the bones' model-space poses (the accumulation
  root held, as the animator holds it) are reflected x → −x onto their partners, put back into
  each bone's own axes through the bind pose (so the bind pose mirrors to itself), and baked
  to linear keys at 30 Hz; the root's own track is mirrored by itself (x and yaw negated);
  text keys `Left`/`Right` swap. Besides both leading feet, it stands in for a badly made
  clip: the mirrored run left runs right at 307 units/s where the original's run right plants
  at 121.

## 6. The legs set the ground speed (`nvf_play::player::AnimationPace`)

While the matcher plays, the player's ground speed is the matched motion's (the feet's) speed,
eased with a half-life of 0.08 s, the way the keys point (or, let go, the way the player was
going), times what the paces above the keys apply (perks, crippled legs, sprint, the tweaks).
Idle and turning: 0. It replaces Movement Inertia's speed on the ground (its springs started
slower than the legs and the feet slid; the owner's report). Guard: standing while the keys
ask to move for 0.3 s lets the keys' speed through. The original's speeds change accordingly
(run forward 303 for 344): a gameplay change of this experiment.

## 7. Features, query and search

| Group | Values | Count | Weight |
|---|---|---|---|
| Left/right foot place (model space) | x, y, z | 6 | 0.75 |
| Left/right foot velocity over the ground | x, y, z | 6 | 1.0 |
| Hip velocity over the ground (`Bip01 Pelvis`) | x, y, z | 3 | 1.0 |
| Trajectory places at +1/3, +2/3, +1 s | x, y | 6 | 2.0 |
| Trajectory facings at the same times | x, y (unit) | 6 | 1.5 |

Groups normalised by their spread (Holden 2020 §3.1); the facings by a fixed 0.5. Velocities
over the ground are the clip's motion about its root plus the root's own; in the query the
root's part is the velocity 0.1 s ahead on the controls' spring (the speed now is the legs'
own, and a walk asked to run kept walking for seconds) [observed].

Trajectory (`motion_match::predict`): a critically damped spring from the ground velocity to
the velocity the keys ask for, half-life 0.12 s; the turn rate (clamped to ±180°/s) easing off
with half-life 0.5 s.

Search: brute force every 0.1 s and at once on a sharp change of the asked velocity. Skipped:
rows within 0.25 s of the one playing in the same motion, and rows **heading away**: both the
row and the query go more than 40 units over the second and their ways part by more than 50°
(from standing every running pose is far, and the pose alone took a mirrored backward run for
a start forward) [observed; design: candidate filtering as shipped matchers do]. A jump needs
to beat carrying on by 0.05 and 10 %. It cuts the movement/turn layers, starts the new
motion's at the row's phase, and `Animator::smooth_jump` with the third-person rig's `Seams`
(95 % in 0.3 s, Bollo's overshoot clamp) eases the pose across. Movement motions play at the
measured speed over the motion's (0.5-2.0×); a turn at the turn rate over 90°/s.

## 8. Foot locking (`foot_ik.rs`)

With Motion Matching on, the player's feet are locked after the animation and the ground
placement: a foot within 4 units of its bind height moving under 250 units/s over the ground
is held where it came down, through the existing two-bone leg solve, and let go when it lifts
or the animation has left it 16 units behind; held and let go along an ease of 30/s [design].
Runs whether or not Foot IK (ground placement) is on.

## 9. Results (smoke, Movement Inertia on, the same moves; foot skate in units/s)

Planted feet's speed over the ground, as shown (`NVF_PLAY_TEST_FOOT_SKATE`), per phase
[observed 2026-11-20, `motion-matching-inertia` and `-off`]:

| | original clips + Movement Inertia | motion matching |
|---|---|---|
| steady run forward | ~100 | ~30 |
| starting | (slide under the inertia's ramp) | ~40 |
| stops | | 25-70 |
| strafes and diagonals | ~240 | 110-150 |

Feet jolt (`NVF_PLAY_TEST_FEET_JOLT`, frame-to-frame change of velocity, 99th percentile /
worst per 10 s): the original 15.8k/38k, matching 15.0k/47-54k; without inertialization
(`NVF_MM_NO_BLEND`) 19-22k/714-741k. The jumps are hidden as well as the original's crossfades
hide theirs.

Open: the strafes still slide (their feet do not step straight against the travel); the
original clips' quality is the ceiling.

## 10. More clips: the motion matching folder

Any `.kf` under `meshes\characters\_male\locomotion\motionmatching\` (loose or in an archive)
joins every clip set's database as a motion of its own (looping or not, by its cycle type),
its trajectory and pace from its `Bip01` root motion: starts, stops, plants, pivots. Clips
must be on the New Vegas skeleton (`Bip01` accumulation root, +y forward). A clip that does
not loop is matched only where a second of it is left and left 0.15 s before its end; its pace
is its root's speed where it is. The engine ships none (no game data). Sources to look at for
one's own use: UFSC's locomotion dataset (commercial use allowed, no resale), HDM05 (CC BY-SA
3.0), 100STYLE (licence unchecked), Mixamo; they need retargeting to `Bip01`.

## 11. Test aids and logs

- `NVF_PLAY_TEST_MM_WEIGHTS=feet_pos,feet_vel,hip_vel,traj_pos,traj_facing`: weights.
- `NVF_PLAY_TEST_FEET_JOLT`: the jolt report every 10 s and each spike over 60,000 units/s².
- `NVF_PLAY_TEST_FOOT_SKATE` (`=<file>.csv` for a line a frame and foot): the planted feet's
  speed over the ground, animated and shown, every 10 s.
- Diagnostics (not prefixed, so the smoke runner passes them through from the shell):
  `NVF_MM_NO_BLEND` (no inertialization), `NVF_MM_NO_PACE` (the keys' speed), `NVF_MM_NO_LOCK`.
- `RUST_LOG=nvforever::player_body::motion_matching=debug`: the skeleton's top nodes, each
  motion's length, feet speed, root scale and travel, and every jump with its costs group by
  group.

## 12. Code

- `engine/nvf-assets/src/motion_match.rs`: `Motion`, `Database` (build, the feet's speed,
  normalise, query, search and its gate), `mirror_clip`, `predict`, `Matcher`; unit tests on
  synthetic clips.
- `engine/nvf-assets/src/anim_blend.rs`: `Layer::phase_offset`.
- `engine/nvforever/src/player_body/motion_matching.rs`: the clip sets' motions and mirrors,
  the database cache and builds, the frame's choice, the legs' pace, the jolt watch.
- `engine/nvforever/src/player_body.rs`: `Frame::mm`, `pose_rig`, the rig's seams, the pace.
- `engine/nvforever/src/foot_ik.rs`: foot locking and the skate aid.
- `engine/nvf-play/src/player.rs`: `AnimationPace` and `paced` in the character's step.
- The switch: `nvf_play::inertia::MovementInertia::motion_matching` (under the Global
  Toggle), `nvf-settings-layout` (NVF Features › Visual Only), docs/settings-reference.md.
