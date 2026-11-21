# Modern Feel: hit reactions

Part of the fork's Modern Feel experiment (`nvf_play::modern_feel::ModernFeel::hit_reactions`,
NVF Features › Modern Feel). Everything here is NVForever's own [design]; every constant is an
engineering choice tuned by eye. The original plays a whole hit reaction clip or nothing; that
clip still plays (`deaths.rs`), this lays a bone-level jolt over it and over everything else.

Code: `engine/nvforever/src/hit_react.rs` (one plugin, `HitReactPlugin`, added in `main.rs`
with the other play plugins). Smoke scenario: `modern-feel-hit-reactions`.

## What it does

- A living actor struck by a shot or a blow has the **struck bone** turned on a damped
  spring, and with less of it (shares 1, 0.55, 0.3) up to two bones above it. The turn is laid
  over the frame's pose after the animation, foot IK, head tracking and aim wrote it, and
  before transforms propagate (so skinning, hit volumes and the ragdoll all see it).
- The turn is the one a push at the hit point gives each bone about its joint: angular rate
  `r × v / |r|²` (r from the joint to the hit point, v the push), so where the hit lands
  decides the bend and the twist: a shot to the side of the chest twists the spine, one to
  the face snaps the head back, one low on the belly bends it forward.
- **Head** hits have a gain of 1.5 and a stiffer spring (snap); **legs** also buckle the knee
  (thigh forward, shin back twice as much, about the knee's hinge from the leg's own bend);
  **arms** 1.2; **torso** 1.3.
- **Strength**: the hit's push (`ActorPush`, 140 units/s a shot or pellet, 220 a blow, ×1.6
  a power blow) times `0.6 + 2.5 × share` of the actor's health the hit took, within 0.6–1.6.
- **Stacking**: a hit adds to the bone's spring rate; turns saturate at 0.6 rad (34°); one
  hit's rate at most 16 rad/s. Shotgun pellets stack into one big jolt.
- **Settling**: ζ = 0.55, ω₀ 16–26 rad/s by limb (torso 17): one overshoot, settled in
  about 0.35–0.45 s. Stepped at 240 Hz at least, so frame rate does not change it; stops while
  the game is paused (virtual time).
- **Killing hit**: the reaction's pose is in the skeleton when the ragdoll starts (it reads
  the bones' global transforms after propagation), and an extra `ActorPush` lands at the
  struck point: 9 units/s a point of the weapon's base damage, within 60–900, plus 30 % of
  it upwards. A pistol (base damage 20) adds 180, a Caravan shotgun's killing pellet (50) 450 on top of
  every pellet's own 140, an anti-materiel rifle 900.

## How the bone is found (any skeleton)

No bone names. The hit's location (`ActorHit::location`) gives the kind of limb; the actor's
body part chains (`limbs::LimbRig`, built from its body part data, creatures included) give
the candidate bones; the bone whose segment (to the next node of its chain, or the skull tip,
or itself) passes nearest the hit point is struck. Bones that carry the legs (the pelvis and
everything up to the root, found from the leg chains) are never turned, so feet stay where
foot IK planted them; a creature without leg chains keeps its root and the root's child still.

## Layering over the animation

`apply` (PostUpdate, before propagation) multiplies the bone's local rotation by the spring's
turn and remembers (found, written). `restore` (PreUpdate) puts the found rotation back where
the bone still holds the written one, before anything of the next frame looks. Without it a
bone the animation does not rewrite each frame would collect the turn, and foot IK (which
tells its own writes from the animation's the same way) would take it for the animation.
A corpse whose ragdoll moves its bones is left alone. Switching the part off clears the
springs; the next `restore` puts the pose back, so the original behaviour returns at once.

## Cost

Nothing while no one reacts. Per hit: a scan of the actor's chain nodes (tens). Per frame
while any actor reacts: a few springs an actor, 240 Hz substeps (4 at 60 fps), one quaternion
multiply a bone. The exit line `hit reactions: … ms a frame while any reacts` measures it.

## Rough edges

- The spring turns bones in their parent's frame as it was at the hit; an actor turning
  quickly during the 0.4 s sees the jolt turn with it (wanted, mostly).
- A leg hit moves the foot a little off where foot IK planted it during the buckle.
- Only hits that come with a push react: the player's and actors' shots and blows. Explosions
  (no `ActorPush` on the living) and the `NVF_PLAY_TEST_HIT` aid do not.
- The kill shove's weapon scale reads the weapon's base damage (no mods, no ammo multiplier).

## Verified

Smoke `modern-feel-hit-reactions` (GSGeneralStore, Chet 130 units off, 9mm pistol then a
Caravan shotgun): 6 pistol hits reacted on Bip01 L Thigh, Spine, Neck1 (3 bones) and R Calf
(thigh + calf with the knee buckle), 8.6–15 rad/s at the hit; the shotgun's killing pellet
shoved the ragdoll 450 units/s more at the left thigh and the body was thrown about 85 units
back and came to rest (screenshots `hit_react_kill_*.png`). Cost: 0.0023 ms a frame while
any actor reacts.
