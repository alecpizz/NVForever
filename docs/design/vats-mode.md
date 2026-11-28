# VATS mode: slow-time mark and fire

Status 2026-10-17: **design for review; built on `claude/vats-mode`** (the engine's
`nvf-play` `vats_mode.rs`, `fnv_combat::vats`, `fnv_game::vats`; seen in game runs since through the
`vats-mode-*` smoke scenarios). Owner request (todo
6): "VATS mode", NVForever's own take on VATS, modelled on Red Dead Redemption's Dead Eye.
Combat is not a parity target (docs/design/combat.md §1: only the quest script hooks must
behave as in the original), so this is deliberately **not** the original's VATS (no
paused target menu, no hit chances, no kill camera; kill cameras came later, §19). The original's VATS stays "not
started" in docs/status.md; the script functions that ask about VATS answer for this
mode (§12).

2026-10-24: **the VATS helpers and the AP perks** (§18): Mysterious Stranger and Miss
Fortune come beside the player and shoot the mark, Friendly Help doubles their odds,
Grim Reaper's Sprint gives AP back after a kill, Nerves of Steel speeds the AP back.

2026-10-23: **hit chances** (§17, owner: "lets build hit chance into our vats … show a
percentage hit chance against the X"): each mark shows its chance to hit beside its "x"
and every shot is rolled on it; a miss goes wide. Built, unit-tested, and seen since in the
smoke scenario `vats-mode-hit-chance`.

2026-10-20: **beyond single-shot guns** (§16): automatic weapons fire a burst per mark,
melee and unarmed weapons teleport beside each mark and swing, grenades and thrown weapons
are lobbed at each mark; melee marks are within 5 m and the player teleports beside each
to swing (owner 2026-10-20). Built, unit-tested, and seen since in the smoke scenarios
`vats-mode-burst`, `-melee` and `-thrown`.

Tags: `[documented]` cites a page or file, `[observed]` is something we looked at in the
game data or the references, `[hypothesis]` is unconfirmed, and **`[design]` is our own
choice where the owner's design leaves something open**. The owner's design itself is
quoted as "owner".

## 1. The owner's design

- **Enter** (owner): pressing V slows time almost to a stop ("very, very slow") and a
  different colour swipes across the screen as it enters.
- **Mark** (owner): the player clicks valid actor targets (creatures, NPCs, any actor).
  Left click (Attack) marks the one under the crosshair with an "x" and reserves the
  shot's AP; the AP cost comes from the records. The player can mark body parts.
- **Unmark and exit** (owner): right click removes the last mark; right click with no
  marks left ends VATS mode.
- **Limits** (owner): no more marks than the AP and the loaded ammunition allow (a .357
  with 6 rounds loaded takes at most 6 marks). Single-shot pistols and rifles first.
- **Accept** (owner): E (Activate) fires every marked shot in real time (since 2026-10-18 at half speed, §4), auto-aimed at
  each mark in order, faster than the weapon's normal rate.
- **Accuracy** (owner): 100 %; no shot misses. Replaced 2026-10-23 by a hit chance per
  mark (owner, §17).
- **Sound** (owner): the base game's VATS sounds for entering, marking, accepting and
  leaving.
- **Look** (owner's references): two Dead Eye clips in `C:\fnv-private\VATS` (private,
  never committed). Frames were taken from them with Windows' own MediaComposition
  thumbnails (no ffmpeg on this machine) into `C:\fnv-private\VATS\frames`; §5 says what
  they show.

## 2. States

```
        V (a gun, AP for a shot)            E (one mark or more)
  Off ───────────────────────────▶ Marking ───────────────────▶ Firing ──(last shot + 0.3 s)──▶ Off
   ▲                                │  ▲ │                                 │
   │  right click with no marks,    │  │ └ left click: mark / right click: unmark
   │  V again, a menu, death        │  └────────────────────────────────────┘ (never back)
   └────────────────────────────────┘
```

- **Off**: the game as usual.
- **Marking**: the clock runs at [`MARK_SCALE`](#4-time-scale); the view still turns at
  full speed (the mouse is read per event, not per game second); left click marks, a
  right-click tap unmarks, E accepts. The weapon's own trigger and activation are held;
  a held right click aims down the sights (§11.5, since 2026-10-23).
- **Firing**: the clock runs at half speed (`FIRE_SCALE`, §4); the marked shots leave one after another
  (§9); all other input is held until the mode is off [design: the sequence is the
  player's committed action, like the original's VATS playback], except aiming down
  the sights (§11.5, since 2026-10-23).
- Leaving takes the grade off over [`LEAVE_FADE`](#5-the-look) (§5); the state is Off
  at once, only the look lingers.

Nothing of the mode is saved (§11.7).

## 3. Entering

V (`Control::Vats`, the game's `Vats` binding 0x002FFF0E, already read by
`engine/nvf-play/src/bindings.rs`; until now it said "not available yet",
`control.rs` `NOT_BUILT`) enters Marking when all of these hold, else it plays
`UIVATSEnterFail` and logs why [design: the masters hold no VATS message strings, and the
owner wants no placeholder text on screen (`activate::HudMessage` keeps the line, not
drawn); the refusal sound tells the player]:

1. The game is playing (`PlayState::Playing`), the player is alive, and no script has
   disabled the fighting controls (`DisablePlayerControls`, `ControlLocks::fighting`).
2. The weapon in use is a gun with a firing cycle that fires a projectile (the detonator
   and the binoculars do not), a melee or unarmed weapon (the Fists too), or a grenade or
   thrown weapon (§16); not a mine. Message: "VATS mode needs a weapon to aim."
   (Until 2026-10-20 guns only: "VATS mode needs a gun.")
3. It is not reloading or jammed. Message: "Not while reloading."
4. It has a round loaded. Message: "Out of ammo."
5. The player has the AP for one shot (§7). Message: "Not enough action points."

With the weapon away, V draws it and enters as soon as it is out [design: a press that
needs no second press; until the weapon is out the mode waits, and a second V cancels].

## 4. Time scale

- `MARK_SCALE = 0.05` [design]: one game second takes 20 real seconds while marking. The
  owner asked for "very, very slow"; the references show people still moving a little
  [observed: Sample 1 and 2, the actors move a step or two over the ~10 s of marking].
- The scale eases from 1 to 0.05 over the swipe (`ENTER_SWIPE = 0.35 s` of real time,
  smoothstep) [design], so the world is seen to slow down.
- Firing runs at `FIRE_SCALE = 0.5`, half speed [design: owner 2026-10-18, "the firing
  is too fast"; first built at 1 after the owner's "in real time"]. The clock jumps from
  0.05 to 0.5 on E and back to 1 when the mode ends after the last shot.
- Mechanism: the engine's own slow motion multiplies SetGlobalTimeMultiplier
  (`script.rs` `time_multiplier`): `ClockScale` (the death camera's) × the VATS mode's
  scale. Everything on the virtual clock slows together: animations, physics, NPC AI,
  projectiles in flight, scripts' frame time and the game clock. Sounds slow too,
  longer and lower (owner 2026-10-18: "more drawn out and lower pitch like the base
  game"), as for the death camera, but less than the clock: `audio_out::slowed_sound`,
  the square root of the engine's scale, no lower than 0.4 [design], so 0.71 while
  firing, and while marking as while firing (`VatsMode::sound_scale`: no slower than
  firing's clock while targets are chosen) [design, #3784: at the 0.4 floor a voice
  spoken while the player took 17 s to mark played as a drone a player read as broken
  sound; owner-review.md]. Sounds already playing follow at once. Effects,
  footsteps and voices slow; music, radio and interface sounds (`UI…`, the UIVATS ones
  among them) keep their speed [hypothesis]. A script's SetGlobalTimeMultiplier still
  slows only the clock. The test aids' clock is the virtual one, so aids
  timed during marking stretch 20 times in real time (§13).

## 5. The look

What the references show [observed: `C:\fnv-private\VATS\frames`, Sample 2 at
0.9–2.5 s and Sample 1 at 0.6–2.2 s, 24 frames each]:

- Entering: in about 0.1 s the picture first goes pale sepia, then a near-white, creamy
  yellow wash covers it (≈ 0.15 s), which fades over ≈ 0.8 s into a lasting **amber
  grade**: mid tones ≈ (228, 141, 62), shadows a dark orange-brown ≈ (91, 43, 5), the
  corners darker (a vignette). The grade stays for the whole mode and drops back to the
  normal colours when the shots are done.
- Marks: a small dark-red "x", about 2 % of the screen width across, on the spot
  clicked; the crosshair is a small dot.

Ours [design, after the references and the owner's "swipes a different colour across
the screen"]:

- **Swipe** (`ENTER_SWIPE = 0.35 s` real): a soft vertical edge crosses the screen from
  left to right. Behind it (to its left) the picture is in the amber grade; on the edge
  rides a band of the creamy wash ((1.0, 0.95, 0.78), 22 % of the screen wide, strongest
  at the edge). When it has crossed, the whole screen carries a wash of 0.5 that fades
  to nothing over 0.6 s, as the references' flash does.
- **Grade** while the mode is on: saturation drops, the picture goes towards its
  luminance × amber (1.45, 0.92, 0.42) by 0.85 (amber mids from mid grey, as measured),
  and the corners darken by up to 35 %.
- **Leaving** (`LEAVE_FADE = 0.3 s` real): the grade fades out evenly.
- It is drawn by the image-space pass (`engine/nvforever/src/imagespace.rs`, its
  shader's last steps, after the scripts' modifiers and before the fade colour), from
  four numbers the play side publishes each frame (`VatsMode::look`: edge, band, wash,
  grade).

## 6. Marking: targets and body parts

- **What is under the crosshair**: the same line the shots use: from the eye along the
  view in first person, the camera's line through the crosshair in third person
  (`control::Crosshair`). Its reach is the projectile's range (PROJ `DATA` Range) when
  it has one, else 8,192 units [design].
- **A valid target** is a living actor other than the player with a hit volume
  (`crate::hitvol`: creatures, NPCs, anything with an actor reference). The ray meets
  actor hit volumes and the world (walls, rocks stop it); a click on nothing, the world,
  a dead body or an object plays nothing and marks nothing [design]. A friendly or
  neutral actor can be marked: shooting one is a crime as any other shot is (combat,
  crime) [design].
- **Body parts**: an actor whose hit volume has body parts (BPTD, combat step C8,
  `hitvol::ActorParts`) is struck by the ray on one part capsule; the mark is on that
  part (its index in the BPTD, its name from the BPTD's part name). An actor with a single
  capsule is marked whole, aimed at its centre. The weapon an NPC holds
  (`hitvol::WEAPON_PART`) can be marked: the shot then wears that weapon (combat §17)
  [design].
- **The same actor** can be marked more than once, on the same or other parts; each mark
  is one shot (as Dead Eye) [design].
- Each mark keeps the actor, the part, and where on the screen to draw it: the part's
  capsule centre now, followed every frame while the actor moves (`PhysWorld::
  actor_part_center`, the whole capsule's aim point without parts). An actor that loses
  its volume (it died, its ragdoll took over, it unloaded) keeps the last point seen.

## 7. AP and ammunition

### 7.1 The cost of a shot

The weapon's AP cost [observed: FalloutNV.esm WEAP `DNAM`; the "Override - Action
Points" flag (Flags 2) and the `override_action_points` field, `fnv_combat::data::
WeaponStats::action_points`]:

| Weapon | Override AP | Weapon | Override AP |
|---|---|---|---|
| WeapNV357Revolver | 26 | WeapNVVarmintRifle | 35 |
| Weap10mmPistol | 17 | WeapNVCowboyRepeater | 27 |
| WeapNVServiceRifle | 15 | WeapSniperRifle | 27 |
| WeapNVHuntingShotgun | 28 | WeapNVAntiMaterielRifle | 55 |

A weapon without the flag costs the attack setting of its animation type [hypothesis:
the setting each type uses, from the names; values observed in FalloutNV.esm GMST]:

| Animation type (WEAP DNAM) | Setting | Value |
|---|---|---|
| Pistol - Ballistic / Energy (1 Hand) | `fActionPointsAttackPistol` | 20 |
| Rifle - Ballistic / Automatic / Energy (2 Hand) | `fActionPointsAttackRifle` | 25 |
| Handle (2 Hand) | `fActionPointsAttackHandle` | 20 |
| Launcher (2 Hand) | `fActionPointsAttackLauncher` | 40 |
| Melee (1 Hand) | `fActionPointsAttackOneHandMelee` | 15 |
| Melee (2 Hand) | `fActionPointsAttackTwoHandMelee` | 30 |
| Hand to Hand | `fActionPointsAttackUnarmed` | 22 |
| Grenade Throw / Thrown (1 Hand) | `fActionPointsAttackGrenade` | 35 |
| Land Mine / Mine Drop (1 Hand) | `fActionPointsAttackMine` | 10 |

Mines do not enter VATS mode (§3), so the last row is not used. Since 2026-10-20 melee,
unarmed and thrown weapons use their rows, and an automatic weapon's cost is one burst's
(§16.1). Not applied [design]: `fActionPointsSustainedFireMult` (2; its role is not
known [hypothesis: the original's sustained automatic fire]; a burst costs one attack,
§16.1), `fActionPointsVATSMeleeMult` (value not read), perk entry points that change the cost (the "Action Point
Cost" ingestible effect, Nerves of Steel's regeneration [documented: GECK wiki Misc
"Action Point Cost … Modifies Action Point Cost of Weapons in VATS"]). Weapon mods do not
change the AP cost [hypothesis].

### 7.2 Reserving and spending

- The player's AP is the script runtime's Action Points actor value (code 12: base 65 +
  3 × Agility [documented: `fAVDActionPointsBase`, `fAVDActionPointsMult`, observed 65
  and 3], modifiers, damage), as the HUD's AP bar shows it (`hud.rs`).
- **A mark reserves** the cost; nothing is spent while marking. A mark is refused
  (`UIVATSInsufficientAP`, logged "Not enough action points") when `reserved + cost`
  would exceed the AP left now.
- **Ammunition**: a mark is refused (the same sound, logged "No more rounds loaded") when
  the marks would need more rounds than the magazine holds now: `marks × ammo_use ≤
  magazine` [design: the rounds loaded, not those carried; Dead Eye does not reload
  mid-sequence either]. God mode (a full magazine always) keeps the magazine's size as
  the limit.
- **Unmarking** frees that mark's reservation.
- **Each shot fired spends** its cost (damage on Action Points), so a sequence cut short
  (§9.3) spends only what it fired. Leaving without firing spends nothing.
- **Regeneration** [hypothesis, the setting's name; rate unconfirmed]: outside VATS mode
  the player's spent AP comes back at `fActionPointsRestoreRate` (0.06, FalloutNV.esm) ×
  the AP maximum per game second: a full bar of 92 AP in ≈ 17 s. Nothing regenerated AP
  before this (nothing spent it). Not while marking or firing.

### 7.3 The pure rules

`fnv_combat::vats` holds the headless parts, unit-tested: `shot_ap_cost(weapon,
settings)`, and `MarkQueue` (push with the AP and round limits, pop the last, reserved
AP, rounds needed, the shots in order). The engine module only feeds it.

## 8. Mark visuals and the HUD

- **"x" marks**: each mark draws an "x" of two crossed bars in the interface's amber
  (`interface::AMBER`, the HUD's colour) lifted 25 % of the way to white so it stands out
  on the amber grade, 1.3 % of the screen width across with bars 10 % of that thick, at its point projected on the
  screen (§6) [design, after the references; amber instead of their dark red at the
  owner's wishes of 2026-10-24, to match the HUD and then to be brighter; smaller and thinner, owner 2026-10-25]. Marks behind
  the camera are not drawn. A mark that has fired goes [design]. The "x" and its
  chance are kept at the same size on the screen at any range, so a far mark stays
  readable [design]. Marks on the same actor and part share a point, so they draw one
  "x" and one chance, the newest mark's: with Concentrated Fire it climbs as marks are
  added (§17.2) [design: the owner, 2026-10-24, no stacked marks].
- **AP bar**: while marking, the HUD's AP bar shows the AP that is left after the
  reservations (`(AP − reserved) / max`), so it drains mark by mark [design]; firing
  spends it for real, so the bar stays where it was.
- The crosshair stays as it is. The HUD is not hidden [design].
- A refused press is heard (§10) and logged, not written on the screen (§3).
- **Chances before marking**: the actor under or nearest the crosshair shows each body
  part's chance on the part, the one under the crosshair in brackets and brighter (§17.5).

## 9. Firing

### 9.1 The sequence

On E with at least one mark (`UIVATSReady`):

1. The clock goes to `FIRE_SCALE` (0.5, half speed) at once; the grade stays.
2. Every `SHOT_INTERVAL = 0.2 s` of game time, 0.4 s of real time at half speed [design:
   owner 2026-10-18, the first 0.12 s at full speed was too fast; still faster than a
   single-shot weapon's own rate, so the owner's "very quickly"] the next mark in the
   order it was made:
   - the view turns to it: heading and pitch from the eye to the mark's point (the
     part's capsule centre now), at once [design: a snap, as Dead Eye's camera does];
   - one shot leaves through combat's own path (`combat.rs` `fire`): the cycle is made to
     fire now whatever its cooldown or wind-up (the fire-rate override), it takes its
     rounds from the magazine as any shot, plays the weapon's sounds, muzzle flash,
     casing and OnFire blocks, wears the weapon, and counts in the stats;
   - the hit is rolled on the mark's chance (§17.4); a miss leaves through a point beside
     the mark instead of the mark's own;
   - the shot leaves along the line from the eye to the point with **no spread, bloom or
     recoil** (owner: 100 % accuracy) and never jams [design];
   - the shot's AP is spent (§7.2).
3. After the last shot, `AFTER_LAST = 0.3 s` of game time (0.6 s real) [design], the
   mode is Off, the clock back at 1
   (`UIVATSExit`) and the grade fades out.

### 9.2 What a hit is

The projectile still flies through the physics world (`fnv_phys` rays) and hits what it
meets: 100 % accuracy means the shot leaves exactly at the mark, not that it passes
walls. Something that moved between the eye and the mark after marking (another actor,
a door) takes the shot instead [design]. A hit is an ordinary hit (combat C5/C8):
damage, limb damage, crippling, death, OnHit, crime, XP. The hit counts as a VATS
attack for the critical chance: `Attacker::in_vats` adds `fVATSCriticalChanceBonus`
(15, FalloutNV.esm) [documented: GECK wiki Critical Hit, the VATS bonus;
docs/spec-notes/combat-rules.md §2] [design: kept, as the original's VATS has it].

### 9.3 Cut short

The sequence stops (Off, `UIVATSExit`, the rest of the marks dropped and their AP not
spent) when the player dies, a menu opens, a script disables the fighting controls, or
the weapon leaves the hand (it broke on a shot, a script unequipped it). A mark whose
actor died before its turn is still shot at its last point [design: the body is a
target, as a shot at a corpse is].

## 10. Sounds

The base game's VATS SOUN records [observed: FalloutNV.esm SOUN Editor IDs; which
moment each belongs to is a hypothesis from its name unless said otherwise]:

| Moment | SOUN | FormID |
|---|---|---|
| Entering | `UIVATSEnter` | 0x00015630 |
| While marking (a loop) | `UIVATSLoop` | 0x0015511B |
| A mark on an actor without parts | `UIVATSSelectTarget` | 0x0002764D |
| A mark on a body part | `UIVATSSelectTargetPart` | 0x00014C35 |
| Unmarking [design: a camera move in the original] | `UIVATSMove` | 0x00015633 |
| A refused mark (AP or rounds) | `UIVATSInsufficientAP` | 0x00022E16 |
| Accepting (E) | `UIVATSReady` | 0x00015632 |
| Leaving (after firing or unmarked) | `UIVATSExit` | 0x00015631 |
| Entering refused (§3) | `UIVATSEnterFail` | 0x000039F7 |
| Not used | `UIVATSCameraIn`, `UIVATSCameraOut` | 0x000158D3, 0x000158D4 |

They play at the listener (`PlaySound { at: None }`, the UI category), found by Editor ID
in `Sfx::forms` (no FormID is written in the code). The loop starts on entering and
stops on accepting or leaving.

## 11. How it meets the rest

### 11.1 Combat

- `combat.rs` gets the mode's request each frame (a shot due now, and its point) and
  fires it as §9.1 says; the player's own trigger is not pulled while the mode is on.
- `By` (who fired a projectile) carries `vats: true` for these shots, which `attacker()`
  passes as `in_vats` when the hit is resolved (§9.2). The `aiming_vats` parameter of
  `attacker()` existed for this and was always false.
- Recoil is not added for these shots; bloom is not grown [design].
- Weapon wear (§17 of combat-rules.md) applies; `fVATSDamageToWeaponMult` (1.0) is not
  applied [design: it is 1].
- Other actors' attacks are not changed: during marking they are slowed with the world;
  during firing they act as usual. `fVATSPlayerDamageMult` (the original's damage the
  player takes in VATS) is not applied [design].

### 11.2 Detection and AI

Nothing special: every shot is a gunshot with its noise and its hit, as any of the
player's (docs/design/detection.md). Entering and marking make no noise [design]. The AI
runs on the slowed clock (§4), so actors keep doing what they were doing, slowly.

### 11.3 The HUD

§8: the AP bar shows AP minus reservations; the marks are drawn by `vats_mode`'s own
overlay, under the menus.

### 11.4 The time scale and the death camera

§4: the two scales multiply. Dying ends the mode first (§9.3), so in practice they do
not overlap.

### 11.5 Controls

- `Control::Vats` (V) leaves `NOT_BUILT`; it enters, and in Marking leaves without
  firing [design: a second V is "never mind", as a right click with no marks].
- In Marking and Firing the mode takes Use (left mouse: mark), Block (right mouse:
  unmark) and Activate (E: accept) for itself: `PlayerControl::trigger` is cleared and
  activation is held (`activate.rs`) [design].
- **Aiming down the sights** (owner 2026-10-23: "vats mode to also work whilst the
  player is aiming down sight … so that snipers scopes are still usable in vats … let
  the player aim or not whilst vats slow time is running"):
  - Marking: Block is shared. A **tap** (let go within `UNMARK_TAP` = 0.25 real
    seconds of the press) unmarks, or with no marks ends the mode; a **hold** aims down
    the sights (or blocks, with a melee weapon), the zoom and a scope included. The aim
    waits out the tap, so a tap does not twitch the zoom; the unmark comes on the
    release, not the press [design]. Aiming held from before V goes on aiming; its
    release is no tap.
  - Firing, first person: the player's own aim stays as it is: a scoped rifle keeps
    its scope up through the marked shots, or the player lets go and fires unaimed
    [design]. Melee and thrown weapons do not aim while firing.
  - Firing, third person: the body follows the NPCs' 20 m rule (owner 2026-10-23: "the
    player will play his aiming animation if the target is far away greater than 20
    metres but he hip fires if its close by"; docs/design/combat-ai.md §10.6): a gun
    with sights (Sight FOV above 0) aims down them for a mark beyond 1,400 units of the
    eye and fires from the hip nearer, held up to 1,350 once raised
    (`nvf_play::combat::aims_down_sights`, shared with `npc_combat`). The mark looked at
    is the one being fired until its burst is out, then the next. The aim clips, the
    third-person zoom and the walk while aiming follow as for a held Block. Log:
    `vats mode: third person, mark k N units away: aims down the sights (beyond 20 m)`
    or `… fires from the hip (within 20 m)`.
  - The zoom runs on real time (`aim_zoom.rs`), so the sights come up at their usual
    pace on the slowed clock; the arms' own aim clips run on the game clock and are
    slowed with the world [observed in code; with true iron sights on, the gun is put
    on the eye by the zoom's share, so the sight picture arrives at the usual pace].
- Movement, looking and the camera (Toggle POV too) keep working, slowed with
  the world; sneaking ends the mode since 2026-10-25 (§11.8). Ready Item's reload and Ammo Swap are held while the mode is on [design: no
  reload or ammunition change mid-mode].
- Controller: the same controls on their pad bindings (`Bindings::pad`), nothing extra.
- `DisablePlayerControls` with the fighting group off refuses entering and ends the mode
  (§3, §9.3).

### 11.6 Menus and pause

Any menu (`PlayState` other than Playing: the pause menu, the Pip-Boy, dialogue,
loading) ends the mode at once without firing; marks are dropped and nothing is spent
[design: simplest, and the slowed world would otherwise sit under the menu].

### 11.7 Saving

The mode is never saved and is Off after a load. A save made while marking (only
possible through the pause menu, which ends the mode first, §11.6) holds the AP as it is,
reservations being nothing spent. Spent AP is the Action Points damage the save already
keeps. The VATS clock scale is not part of the save.

### 11.8 Never stuck in the mode

Owner 2026-10-25: "kick the player out of vats if he activates something, gets forced
dialogued, tries to wait (basically any action besides aiming and marking or accepting
marks) or if he dies. i dont want the player to get stuck in vats" [design].

- **Actions** (`vats_mode::LEAVING`, `leaving_press`): in Drawing, Marking or Firing a
  press of Activate, Ready Item, Sneak, Jump, the Pip-Boy, Wait, a hotkey (Ammo Swap
  too), Quick Save, Quick Load or Grab ends the mode at once and the press then does
  what it does outside it. The one exception is Activate while marking with a mark
  made: that accepts (§11.5). So E with nothing marked activates what is under the
  crosshair, and Wait opens the Wait menu (or is refused as usual, the mode ended all the
  same). The other exception is the Pip-Boy (Menu Mode), whose own action comes on the
  release (a tap opens the Pip-Boy) or after a hold (the flashlight, `flashlight.rs`): a
  Menu Mode press made while the mode is on (Drawing, Marking or Firing) is the mode's
  and is used up (`flashlight::MenuKeyHold::consume`), so it only leaves, or on the
  controller's classic screen, where B is also the screen's Return, only returns; its
  release opens no Pip-Boy and its hold switches no flashlight, and the next press out
  of the mode opens the Pip-Boy as usual [design, #2311: B, a controller's back button,
  left Dead Eye and opened the Pip-Boy on its release; Tab the same]. The
  check runs in `read_keys`, before activation, so the same frame's E
  reaches the target. Use, Block, V, movement, looking and Toggle POV stay the mode's
  or go on as before. Log: `vats mode: off (…; Wait pressed)`, the control's name from
  `bindings::label`.
- **A conversation** (E on an actor, StartConversation, a Dialogue package's forced
  greeting) or a script's menu: `Game::in_menu` ends the mode the frame it starts, before
  `PlayState::Dialogue` follows (§11.6 would catch it a frame later). Log:
  `… a conversation or a script's menu began`.
- **Death** ends the mode (§9.3), as before.
- **Drawing** (V with the weapon away) gives up when the weapon is put away again or is
  not out within `DRAW_LIMIT` = 3 real seconds [design]. Log: `vats mode: entering
  cancelled (the weapon did not come out)`.

## 12. Script functions

| Function | Opcode | Conditions in the masters | Returns with VATS mode |
|---|---|---|---|
| GetVATSMode | 0x10E0 | — | 0 Off; **1** (`VATS_TARGET_SELECT`) while marking; **4** (`VATS_PLAYBACK`) while firing [documented: GECK wiki GetVATSMode return values 0–4] [design: our states mapped to the nearest; 2 menu and 3 scan are never returned] |
| GetVATSValue | 0x1198 | 250 | While firing, about the shot leaving now; 0 otherwise (below) |
| GetVATSTargetHeight | 0x10EB | — | stays a stub (0) [design] |
| SetVatsTarget | 0x11EB | — | stays a stub (no effect) [design] |

GetVATSValue's queries [documented: GECK wiki GetVATSValue lists the queries; the
query codes are the page's order from 0 with Weapon Type Is moved last [hypothesis;
observed: the parameter kinds the masters' conditions give each code, and Center of
Mass's and Sniper's Targeted Part conditions on code 5, perk-entry-points.md §4]]. The
player's current VATS shot is the one firing now (set before the shot leaves, cleared
when the mode is off), so the conditions of camera paths and perk entry points evaluated
while a shot leaves see it:

| Code | Query | Returns |
|---|---|---|
| 0 | WeaponIs (a WEAP) | 1 if the shot's weapon is it |
| 1 | Weapon In List (a FLST) | 1 if the weapon is in the list |
| 2 | Target Is (an NPC_/CREA) | 1 if the target's base (or the reference itself) is it |
| 3 | Target In List (a FLST) | 1 if the target's base or reference is in the list |
| 4 | Target Distance | the distance from the player to the target, units |
| 5 | Targeted Part (a body part's condition actor value) | 1 if the mark's part names that condition: the BPTD part's own Actor Value, as combat's limb hits read it (`fnv_game::combat::limb_condition`, 25–31) |
| 6 | VATS Action | 1 for the code of the attack action (0) [hypothesis: Attack is the first] |
| 7 | Is Success | 1 if the shot's hit roll succeeded (§17.4); 0 while a mark's chance is worked out |
| 8–14 | Is Critical, Crit Effect Is, Crit Effect In List, Is Fatal, Explode Part, Dismember Part, Cripple Part | 0: none of them is known before the shot lands [design]; while a kill camera is chosen 8 and 11–14 answer about the landed shot (§19.3) |
| 15 | Weapon Type Is (an animation type) | 1 if the weapon's DNAM animation type is it |

The parameter of codes 0, 1, 2 and 3 arrives as a raw FormID (`ptVATSValueParam` is
read as an integer, `fnv_game::cond`): it is taken as a load-order FormID, right for
records of FalloutNV.esm, whose local and load-order FormIDs agree [hypothesis: the
masters' conditions name FalloutNV.esm records].

The engine tells the script runtime: `Game::vats` (mode and shot) is set by `vats_mode`
each frame. The `in_vats` flag in `fnv_combat::damage::Attacker` is the hit side (§9.2).

## 13. Test aids and smoke tests

- `NVF_PLAY_TEST_VATS_HELPER=stranger|fortune|none` (§18.5).
- `NVF_PLAY_TEST_VATS_ROLL=hit` (or `miss`) makes every hit roll come out so (§17.4), for
  checks about something other than the chance; unset, the rolls are rolled.
- `NVF_PLAY_TEST_KEYS` presses V, the left mouse button and E through the bindings
  (`v`, `mouse-left`, `mouse-right`, `e`), so the real key path is tested.
- New `NVF_PLAY_TEST_CONTROL` press **`aimvats`**: turn the view (and the third-person
  crosshair) to the nearest living actor with a hit volume that has no VATS mark yet, at
  its aim point (as `aimactor`), so a click marks it.
- Aid times are on the virtual clock (§4): an aid 0.05 s after another while marking
  comes about 1 s later in real time.
- Log lines (smoke tests read them):
  - `vats mode: on (<weapon EDID>, <n> AP a shot, <m> rounds loaded, <ap> AP; at most <k> marks)`
  - `vats mode: refused: <why>` (entering)
  - `vats mode: marked <name> <ref> (<part>), mark <k>; <r> AP reserved, <left> AP left`
  - `vats mode: mark refused: <why>`
  - `vats mode: unmarked <ref> (<part>), <n> marks left`
  - `vats mode: firing <n> marked shots`
  - `vats mode: shot <k> of <n> at <name> <ref> (<part>), <cost> AP spent`
  - `vats mode: off (<n> shots fired, <ap> AP spent; <why>)`
  - at exit: `vats mode summary: entered <e> times, <m> marks, <s> shots, <ap> AP spent, <r> refused, <h> hit, <x> missed`
  - the hit chance's lines: §17.6
- Scenarios in `tools/smoke/scenarios.toml`: `vats-mode-two-marks` (the Goodsprings dogs,
  a .357: enter, mark both, accept, two shots, 52 AP), `vats-mode-unmark` (mark, unmark,
  right click again: off with nothing fired or spent) and `vats-mode-ammo-limit` (one
  round loaded: the second mark is refused); for bursts, melee and throws §16.4.

## 14. Not built, for later

- Bursts, melee and thrown weapons: built 2026-10-20 (§16). Left: power attacks and
  chained swings in VATS mode, a short last burst, mines.
- The "Action Point Cost" effect. Hit chances: built 2026-10-23 (§17).
- The original's VATS menu, GetVATSTargetHeight, SetVatsTarget. (Kill cameras: built
  2026-10-24, §19.)
- Tuning by eye: `MARK_SCALE`, the swipe and grade colours, `SHOT_INTERVAL`, the mark's
  size, all [design] and owner-checkable in one run.

## 15. Evidence

| Fact | Source |
|---|---|
| Weapon AP overrides (.357 26, 10mm 17, …) | [observed] `fnvtool combat form` on FalloutNV.esm, DNAM `override_action_points` with the Flags 2 bit |
| `fActionPointsAttack*` 20/25/20/40/15/30/22/35/10, `fActionPointsSustainedFireMult` 2, `fActionPointsRestoreRate` 0.06, `fVATSCriticalChanceBonus` 15, `fVATSPlayerTimeUpdateMult` 6 | [observed] FalloutNV.esm GMST (`fnvtool esm decode`) |
| VATS SOUN Editor IDs | [observed] FalloutNV.esm SOUN records |
| GetVATSMode return values, GetVATSValue queries | [documented] GECK wiki pages, local export `C:\fnv-private\geckwiki\functions.xml` |
| The look of Dead Eye | [observed] the owner's two clips, frames in `C:\fnv-private\VATS\frames` (private) |

## 16. Beyond single-shot guns (2026-10-20)

Owner request (todo 6, follow-up): automatic and burst weapons, melee weapons and thrown
weapons in VATS mode. Everything in this section is **[design]** unless tagged: the
original's VATS has bursts, melee attacks and throws, but this mode is our own (§1), so
nothing here is matched to it. The weapon's kind is decided on entering
(`fnv_combat::vats::AttackStyle`) and fixes how each mark is spent:

| Kind | Which weapons | A mark is | Rounds limit | Mark reach |
|---|---|---|---|---|
| Single | guns without the WEAP "Is Automatic" flag (shotguns: one shot of all their pellets) | one shot | rounds loaded (§7.2) | projectile range, else 8,192 |
| Burst | guns with "Is Automatic" | one burst (§16.1) | rounds loaded | as Single |
| Melee | melee and unarmed weapons, the Fists | a teleport beside the mark and one swing (§16.2) | none | `MELEE_MARK_REACH` 350 (5 m) |
| Thrown | weapons that spend themselves (Grenade Throw, Thrown; not mines) | one throw (§16.3) | the count carried | projectile range, no farther than the throw reaches (§16.3) |

The mark's AP is the weapon's attack cost (§7.1: the WEAP override, else the animation
type's `fActionPointsAttack…`), reserved on marking and spent when the mark's attack
starts (the first round of a burst, the swing, the throw); a sequence cut short spends
only the marks it started (§9.3). Observed costs [observed: `fnvtool combat form` on
FalloutNV.esm, WEAP DNAM, all with the override flag]:

| Weapon | Kind | AP | Fire rate (shots/s) | Burst |
|---|---|---|---|---|
| Weap10mmSubmachineGun | Burst | 22 | 9 | 4 shots |
| WeapNVAssaultCarbine | Burst | 20 | 12 | 5 shots |
| WeapMinigun | Burst | 30 | 20 | 8 shots |
| WeapNVMachete | Melee | 20 | | |
| WeapKnifeCombat | Melee | 17 | | |
| Fists | Melee (unarmed) | 18 | | |
| WeapNVBallisticFist | Melee (unarmed) | 28 | | |
| WeapGrenadeFrag, WeapNVDynamite | Thrown | 24 | | |
| WeapNVThrowingSpear | Thrown | 24 | | |
| WeapNVThrowingKnife | Thrown | 20 | | |

The log's `on` line names the kind for the new ones:
`vats mode: on (<EDID>, burst of <s>, <n> AP a mark, …)`, `… melee, …`, `… thrown, …`;
a single-shot weapon's line is unchanged (`<n> AP a shot`).

### 16.1 Bursts

- **Size**: `burst_shots(fire_rate) = round(fire_rate × BURST_TIME)`, `BURST_TIME` 0.4 s
  of game time, no fewer than 3 and no more than 8 shots (`BURST_MIN`, `BURST_MAX`). A
  record without a fire rate fires 3. So a burst lasts about 0.4 s of game time
  (0.8 s real at `FIRE_SCALE`) whatever the weapon.
- **Rounds**: a burst takes `shots × Ammo Use` rounds, and only whole bursts are marked:
  the marks stop when the rounds loaded do not cover another full burst (a carbine with
  24 loaded takes 4 bursts of 5 and leaves 4) [design: simple and predictable; a short
  last burst would need its own AP rule].
- **AP**: one burst costs the weapon's attack cost once (the carbine's 20), not per round
  [design: the AP override is an attack's cost and a mark is one attack; with the
  observed costs a full bar of 92 AP buys 4 carbine bursts, about the magazine].
  `fActionPointsSustainedFireMult` is not applied.
- **Firing**: the burst's rounds leave `1 / fire_rate` game seconds apart (no faster than
  one a frame), each straight at the mark's point (followed every frame), with no
  spread, bloom, recoil or jam, as a single shot (§9.1). The next mark's burst starts
  `SHOT_INTERVAL` after the last round of the previous one. The weapon's looping fire
  sound (the minigun's) plays through the burst as usual (combat).
- Log: `vats mode: burst <k> of <n> at <name> <ref> (<part>), <s> shots, <cost> AP spent`
  at the burst's first round; `off (…)` and the summary count rounds fired as shots.

### 16.2 Melee and unarmed

Owner 2026-10-20: "for melee swap it to a teleport to target and attack but set it to a
has to be in range within 5 meters or so for it to be allowed". (First written as a run
at each mark; replaced before it was built into a run.)

- **Marking** works as for guns, but only within `MELEE_MARK_REACH`, 350 units of the
  eye (5 m at 70 units a metre, `fnv-phys`): a click on an actor farther away marks
  nothing (`vats mode: nothing to mark under the crosshair`, the crosshair's ray is that
  long). No rounds.
- **Firing**, each mark in order (`fnv_combat::vats::MeleeRun`):
  1. **Teleport**: the player is put on the ground beside the mark
     (`PhysState::place_player`, the ground looked for from 150 units above to 600
     below), on the line from where they stand to the mark's point, `stand_off` from it
     across the ground: half the weapon's reach (`damage::melee_reach`: `fCombatDistance`
     107 × the WEAP Reach, combat-rules.md §10, plus the player's capsule radius 22: the reach
     from the eye) plus 32 units [design]; the view turns to the mark. Nothing
     checks the spot is free beyond the character's own placing [design: the mark was
     within 5 m and in sight, so the spot is too in practice].
  2. **Swing**: the next frame (the mark is within the weapon's reach plus `CLOSE_SLACK`,
     64 units, across the ground; `APPROACH_LIMIT`, 2.5 game s, is the fallback should it
     not be), one tap of Use for combat's own swing (`fnv_combat::melee`: its clip,
     timing, sounds, Attack line, impacts, wear), the view kept on the mark. The mark's
     AP is spent now. At the swing's strike the blow lands on the mark (100 % accuracy)
     when the mark is still within reach plus `CLOSE_SLACK` across the ground and the
     first thing on the line from the eye to its point is the marked actor; else the
     swing strikes as an ordinary one would (its fan of rays). The hit counts as a VATS
     attack (the critical bonus, §9.2). Blocking applies as usual.
  3. **Next**: once the swing has ended (or `SWING_LIMIT`, 2 game s, has passed), the next
     mark. No power attacks and no chained swings [design: one plain swing a mark].
- `AFTER_LAST` after the last swing the mode ends, as for shots. The player stays where
  the last teleport put them.
- Log: `vats mode: teleported to <name> <ref> (<part>), <d> units away, now <s>`, then
  `vats mode: swing <k> of <n> at <name> <ref> (<part>), <d> units across, <cost> AP
  spent`; at the strike `combat: vats mode blow on <ref> (<part>): <d> units across,
  struck` before combat's own hit line.

### 16.3 Grenades and thrown weapons

- **Marking**: up to the count of the weapon carried (each throw spends one; god mode
  spends none and still limits to the count), within the projectile's range and no
  farther than the throw reaches on flat ground (speed² / gravity: the frag grenade's
  1,200 units/s under 1 × the world's 686.7 units/s² reach 2,097 units) [observed:
  GrenadeFragProjectile PROJ speed 1200, gravity 1.0, range 10000, Lobber].
- **Firing**: every `THROW_INTERVAL` (0.35 game s, 0.7 s real) the next mark: the view
  turns to it, one throw leaves through combat's own path (the wind-up skipped as for a
  gun's shot, the count spent, the Attack/FireExplosive line), from the eye, with no
  spread, along the **low arc that lands on the mark's point** for the projectile's
  speed and gravity (`fnv_combat::vats::lob_dir`); a point out of reach is thrown at 45°
  towards it. A projectile without gravity flies straight. Grenades' fuses and impacts
  are their own (combat C7): a frag grenade's 2.5 s fuse may go off after the mode ends.
- The last throw spends the last one carried and the weapon leaves the hand; the mode
  then ends as "all shots fired", not "the weapon left the hand".
- Log: `vats mode: throw <k> of <n> at <name> <ref> (<part>), <cost> AP spent`.

### 16.4 The rest

- GetVATSValue answers as before; the shot's weapon is the one in hand. A burst is one
  VATS shot for the scripts while its rounds leave.
- Smoke scenarios (§13): `vats-mode-burst` (a 10mm SMG, two marks: two bursts of 4,
  44 AP), `vats-mode-melee` (a machete, one mark on a dog: teleport, swing, 20 AP),
  `vats-mode-thrown` (frag grenades, one mark: one throw, 24 AP). Written 2026-10-20 by a
  worker session that does not run the game: not yet run.

## 17. Hit chance (2026-10-23)

Owner 2026-10-23: "lets build hit chance into our vats we'll need to extend the UI when
marking targets to show a percentage hit chance against the X too". Each mark has a
chance to hit, drawn beside its "x" and rolled for every shot. The original's formula is
not documented (the GECK wiki names its settings, not how they combine), so the
formula is ours [design], built from what is documented and observed.

### 17.1 What is documented and observed

- Each BPTD part has a "To Hit Chance", "the base percentage chance to hit this body part
  target in VATS" [documented: GECK wiki Body Part, BodyPartData]. FalloutNV.esm
  [observed: `fnvtool combat form`]: DefaultBodyPartData (people) Head 30, Torso 60, arms
  45, legs 50; DogBodyPartData Torso 75, Head 30, legs 40.
- A WEAP's DNAM "Base VATS To-Hit Chance": the chance to hit the weapon in another actor's
  hands, "factored before distance, gun skill and screen percentage" [documented: GECK
  wiki Weapons]. So the original's chance reads the part, the distance, the skill and how
  big the target is on the screen.
- The settings [documented: GECK wiki VATS Settings, the defaults; observed:
  FalloutNV.esm sets `fVATSDistanceFactor` 2 (the wiki says it does not exist in New
  Vegas; the game data has it, so it is read) and `fVATSSpreadMult` 1.1, and none of the
  others, so their wiki defaults are used]: `fVATSMaxChance` 95 ("the maximum chance to
  hit a target in VATS"), `fVATSHitChanceMult` 1, `fVATSSkillFactor` 1,
  `fVATSDistanceFactor` ("how rapidly the chance to hit decreases as the range of the
  target increases … non linear"), `fVATSSpreadMult` ("gun spread … multiplied by this
  value while in VATS"), `iVATSConcentratedFireBonus` 5 ("+ [bonus/100] × [current shot −
  1]": 63 % on a limb becomes 68 on the second shot, 73 on the third).
- The perks: entry point 8 Calculate To Hit Chance (Commando × 1.25 with rifles,
  Gunslinger × 1.25 with pistols, Sniper × 1.25 on the head, Hobbler × 1.25 on the legs,
  Wired Reflexes × 1.1, Fast Shot × 0.8 and Trigger Discipline × 1.2 with guns and energy
  weapons, Sneering Imperialist × 1.25 against some factions, Camarader-E, Lonesome Road)
  and 33 Has Concentrated Fire (Concentrated Fire, Set 1) [observed: the PERK records;
  documented: GECK wiki GetNthPerkEntryFunction, the codes; docs/handoff/
  perk-audit-2026-10-23.md §3.9].

### 17.2 The chance (fitted to the original, 2026-11-01)

The original's formula is not documented, so it was measured (issue #1361): its VATS
percentages, read from the VATS menu's tiles (`VATSMenu/GlowBranch/*:N`, traits
`_LimbName` and `string`, through JIP's GetUIString), against an NCR trooper held still
at 350 to 4,200 units, with a 9mm pistol, a varmint rifle, a sawed-off shotgun and a
hunting rifle, at Guns 17, 32, 67 and 117, standing and sneaking, Strength 5 (save
`nvfVATS`, 276 readings) [observed]. The structure below is ours [design], each constant
fitted to those readings [observed: fitted]:

```
chance = clamp( perks( base × CLOSE_MULT × range × fVATSHitChanceMult )
                + concentrated, 0, fVATSMaxChance )
range  = 1 / (1 + (open / (FOCUS × size))^FALLOFF)          (1 for a melee mark)
open   = distance × tan(cone × fVATSSpreadMult)
cone   = the VATS actor spread (below) and the weapon's Min Spread, in degrees, halved
```

- What the readings showed [observed]: close by, nearly every part is at the 95 % cap,
  even at Guns 17 (a torso 95 %, a head 53–66 % at 350 units). The chance falls steeply
  with distance, and faster for a wide gun (sawed-off torso: 52 % at 350 units, 10 % at
  1,050; varmint rifle: 95 %, 79 %). Skill helps a rifle a lot and a pistol little
  (at 1,050 units, the hunting rifle's torso is 43 % at Guns 32 and 85 % at 117; the
  pistol's is 38 % at 17 and 55 % at 117), so the skill acts through the spread and not
  as a separate multiplier. Sneaking changed nothing (within a point). The held weapon
  reads about a fifth of the torso. `GetGameSetting` gives `fVATSDistanceFactor` 0 in
  the running game.
- **The VATS actor spread** (`spread::vats_actor_spread`): not aiming and standing
  (`fUnaimedSpreadPenalty` + `fStandingSpreadPenalty`, 0.3) whatever the stance, plus an
  unmet Strength requirement and crippled limbs, times 1 − `fWobbleToSkillConversion` ×
  skill / 100. The fit found 0.32 units and a 51 % drop at skill 100, matching these
  documented settings (0.3, 0.5) [observed: fitted]; the hunting rifle's extra matches its
  Strength requirement 6 against Strength 5. A skill below the weapon's requirement adds
  nothing: with that penalty the fit got worse [observed]. Moving and jumping are left
  out as sneaking is [hypothesis]; crippled arms stay in [design].
- **CLOSE_MULT** 7.58, **FOCUS** 3.3 units, **FALLOFF** 1.29 [observed: fitted]. The base from the
  BPTD part, as before.
- **size**: torso 1, head 0.89, arms 0.84, legs 0.91 (by the part's condition actor
  value), held weapon 0.32 [observed: fitted]. A held weapon's base is its WEAP's Base VATS To-Hit
  Chance [documented; the Service Rifle's 42 fits within 1.3 points]; ours still uses
  `HELD_WEAPON_TO_HIT` 30 until the NPC's weapon is looked up. An actor without parts:
  `WHOLE_TO_HIT` 75 at size 1 [design, not measured].
- How close: over all 276 readings, 2.8 points RMS, 214 within 3 points. The pistol,
  the hunting rifle and the heads come within 2 points at the test's sample points. The
  varmint rifle and the sawed-off up close come out 5–8 points low, unexplained. Arms
  vary with the pose: one arm read 76 % or 95 % in the same setup.
- `fVATSSkillFactor` and `fVATSDistanceFactor` are read and not used.
- **Melee** (no cone: the player is put beside the mark) keeps the old skill share in
  place of CLOSE_MULT: base × (0.5 + 1.5 × skill / 100), so skill 0 halves the base and 100
  doubles it (`MELEE_SKILL_BASE`, `MELEE_SKILL_SPAN`) [design: melee was not measured;
  CLOSE_MULT alone would put every human part at the cap whatever the skill].
- **An object** marked with a gun (`WHOLE_TO_HIT` 75, size 1) takes the gun's cone like an
  actor's part, from the first frame after the mark; with a melee weapon it takes the
  melee share.
- The distance is from the eye to the mark's point now. The chance is worked out again
  every frame while marking and firing, so it changes as the actor and the player move.
- **perks**: the player's Calculate To Hit Chance entries with the tabs Weapon and
  Target (the conditions of each tab as for the other entry points,
  perk-entry-points.md §3), read when the mark is made, with GetVATSValue answering for
  that mark (so Sniper's head and Hobbler's legs see the marked part) and GetVATSMode for
  the mode as it is (`fnv_game::Game::vats_hit_perks`). Not read again while the mark
  stays [design: the conditions are about the weapon, the target and the part, which do
  not change]. Not measured.
- **concentrated**: with Concentrated Fire, `iVATSConcentratedFireBonus` for each mark
  made before it on the same actor and part [documented].
- Not measured yet: Energy Weapons, melee and thrown marks, creatures' parts, perks.

### 17.3 Where it is worked out

- `fnv_combat::vats`: `HitTuning` (the settings, `Rules::vats_hit`), `raw_chance`,
  `final_chance`, `rolled_hit`, `miss_point`, `throw_miss_point`; unit-tested.
- `fnv_game::vats`: `HAS_CONCENTRATED_FIRE`, `HitPerks`, `Game::vats_hit_perks`;
  `tests/perk_combat.rs` `vats_to_hit_perks_see_the_mark`.
- `nvf-play` `vats_mode.rs`: each mark's base, perks and earlier marks when it is made,
  its chance every frame (`chance_of`, `rechance`), the rolls, the labels.

### 17.4 Rolling and missing [design]

- Every round is rolled on its mark's chance at the moment it leaves: a single shot once,
  each round of a burst on its own, each throw, each swing. The rolls come from their
  own seeded generator, so a run with the same inputs repeats.
- **A gun's miss** leaves from the eye through a point beside the mark, across the line
  of fire, 45–80 units from it (`MISS_MIN`, `MISS_SPAN`), to a side or above, never more
  than 30° below the side (where the legs are, which would turn a miss into a hit). The
  shot then flies as any other and hits what it meets: another actor behind, a wall.
- **A throw's miss** is thrown on the low arc onto a point 120–240 units from the mark
  across the ground (`THROW_MISS_*`); its explosion may still reach the mark.
- **A swing's miss** strikes no one: the blow is not dealt, the swing plays.
- The AP is spent either way. GetVATSValue Is Success answers the roll while the shot
  leaves.

### 17.5 On the screen

- Beside each mark's "x" (to its right, centred on it) its chance in whole percent,
  "62%", in the HUD's font (`textures\fonts\baked-in_monofonto_large.fnt`), in the
  marks' brightened amber (§8) [design: the owner's wishes of 2026-10-24; it was a
  creamy white]. It follows the mark and changes as the
  chance does; it goes with the "x" once the mark is fired.
- **Before marking** (issue #1384, 2026-11-06): while marking, one actor shows every
  body part's chance before anything is marked: the living actor under the crosshair,
  else the nearest valid target (§6) whose aim point lies within 10° of the crosshair's
  line (`PREVIEW_CONE`), in the weapon's reach and in sight [design: one actor, not
  every one in view, so the labels stay readable; the one the next click would land on
  or near]. Each part its hit volume has a capsule for (the weapon it holds too) draws
  "Head 41%" (the BPTD part name as the data spells it, "Weapon" for the held weapon) centred
  on the part's capsule centre (`PhysWorld::actor_part_center`); an actor with a single
  capsule draws its chance alone at its aim point. The part the crosshair is on is drawn
  in brackets, "[Head 41%]", and brighter (the amber lifted 75 % to white against the
  marks' 25 %), so the player sees which part a click marks [design: NVForever's own
  wording and look]. Same font as the marks' labels, the marks' amber, the same size on
  the screen at any range; a label that would overlap one drawn before it moves down a
  line [design].
- The figure is the one a mark on that part shows: both come from the same work
  (`vats_mode::spot_on`: the part's BPTD To Hit Chance and size, the to-hit perks seeing
  it as the shot, then `chance_of` every frame: skill, the spread at range, the 95 %
  cap). The parts, their names and their perks are worked out again when the actor
  changes and four times a second (`PREVIEW_REBUILD`), their points and chances every
  frame [design: the perks may read the distance].
- A part already marked shows its mark's "x" and label only. A part behind the camera,
  or whose centre the world hides from the crosshair's line, is not drawn; an actor in
  the way does not hide it. A dead actor, a cloaked one without Enhanced Sensors and a
  No VATS Melee one with a melee weapon (§6) show nothing, as they cannot be marked.
  Nothing is previewed while firing.

### 17.6 Log lines

- `vats mode: marked <name> <ref> (<part>), mark <k>; <r> AP reserved, <left> AP left;
  <c>% to hit (base <b>[, perks <names>][, concentrated fire +<n>]); <d> units, skill <s>,
  cone <deg> deg|none[; previewed <c>%]` (the distance, skill and cone the chance was
  worked from; the preview's figure for the part when it showed one, §17.5)
- `vats mode: shot <k> of <n> at … , <cost> AP spent; <c>% to hit: hit|missed (rolled
  <r>)` (`burst …; round 1 …`, `throw …`, `swing …` alike; later rounds of a burst at
  debug)
- `combat: vats mode blow on <ref>: missed (the hit roll)` for a swing's miss
- `vats mode: rolls: <h> hit, <x> missed` after `off (…)`
- `vats mode: preview <name> <ref>: <part> <c>%, …` when the previewed actor changes
  (§17.5), each part's chance as its label shows it
- the summary's `<h> hit, <x> missed`

### 17.7 Checks

- Unit: `fnv-combat` `vats` (`the_hit_chance_falls_with_range_and_part_size`,
  `the_chance_matches_the_originals_samples`,
  `perks_concentrated_fire_and_the_cap`, `a_miss_passes_wide_and_never_below`),
  `nvf-play` `vats_mode` (`a_marks_chance_follows_range_and_perks`,
  `a_chance_reads_as_whole_percent`, `a_preview_figure_is_the_marks_chance`,
  `a_preview_label_names_the_part`), `fnv-game` `perk_combat`.
- Smoke: `vats-mode-hit-chance` (two marks on the Goodsprings dogs show their chances and
  are rolled), `vats-mode-miss` (every roll forced to miss: the shot goes wide and the
  dog is not hit by it), `vats-mode-preview` (the dog under the crosshair shows its
  parts' chances before marking, and the mark's chance is one of them; frame
  `vats_preview.png`).

## 18. VATS helpers and the AP perks (2026-10-24)

Owner 2026-10-24: "Build the VATS perk batch next: Mysterious Stranger, Miss Fortune,
Friendly Help, Grim Reaper's Sprint and Nerves of Steel" (docs/handoff/
perk-audit-2026-10-23.md §3.1). The original runs these in its own VATS; VATS mode is
ours (§1), so where they fit in it is [design]. What they do comes from the records.

### 18.1 What the records and the wiki say

- The perks' entries [observed: FalloutNV.esm PERK]: Mysterious Stranger (level 10, Luck
  6) entry point 28 Mysterious Stranger Set 1; Miss Fortune (level 10, Luck 6) 41 Miss
  Fortune Set 1; Friendly Help (the challenge reward) 67 VATS HelperChance Multiply 2;
  Grim Reaper's Sprint (level 20) 35 Player Kill AP Reward Add 20; Nerves of Steel (level
  26, Agility 7) 39 Action Point Regen Multiply 1.2. What each entry point does
  [documented: GECK wiki Entry Point]: 28 and 41 "if non-zero, the VATS helper … may
  appear"; 67 "modifies the chance of a VATS helper"; 35 "run when VATS finishes if the
  player killed at least one enemy; modifies the AP reward from 0".
- The settings [observed: FalloutNV.esm GMST; documented: GECK wiki VATS Settings]:
  `fVATSStrangerOdds` 0.1, `fVATSFortuneOdds` 0.1, `iVATSStrangerMaxHP` 150 ("the maximum
  health of an NPC that the stranger … will act upon"; the wiki's condition: "attacking
  in VATS, enemy not dead, but less than 150 hitpoints"), `fVATSStrangerDistance` 164.
- The helpers [observed: FalloutNV.esm]: the NPC_ `Stranger` ("Mysterious Stranger",
  essential, MysteriousStrangerFaction) carries `MysteriousStranger44Magnum`, a .44 of
  9,000 damage; `MissFortune` (the same faction) carries `WeapNVMissFortuneGun`, 10
  damage, whose object effect `VMissFortuneGunObjectEffect` casts `MissFortuneGunSpell`,
  whose script (NVMissFortuneGunSpellScript) rolls GetRandomPercent and knocks the target
  down (MissFortuneKnockdownSpell: PushActorAway), sets off an explosion where it carries
  grenades or a flamer, ruins its weapon arm and weapon, or paralyses it. The music
  `MUSMysteriousStrangerA01`, `A02`. The challenge "He Moves in Mysterious Ways"
  (NVChallengeMysterious) counts misc statistic 25, Mysterious Stranger Visits, to 15; its
  script adds Friendly Help.
- Not used here: the camera path `MysteriousStrangerCameras` and its 13 shots (the
  original's kill camera; VATS mode has none, §14).

### 18.2 The helpers in VATS mode [design]

- **When**: as each mark's attack starts (its AP spent: a shot's first round, a burst's,
  a throw, a swing), if no helper is in the world already. One roll a mark
  (`fnv_game::vats::pick_helper`): the Stranger on the first `odds.stranger` of the line,
  Miss Fortune on the next `odds.fortune`, none after.
- **Odds** (`Game::vats_helper_odds`): with Mysterious Stranger `fVATSStrangerOdds` when
  the mark's actor lives with at most `iVATSStrangerMaxHP` health; with Miss Fortune
  `fVATSFortuneOdds` when it lives [hypothesis: no limit is given for her]; each times the
  VATS HelperChance entries (Friendly Help: × 2). The player's own roll (§17.4) is not
  looked at: the helper may come for a mark the player hits, as the wiki's condition is
  about the target, not the shot.
- **Where**: `fVATSStrangerDistance` (164) units from the player's feet across the line to
  the mark, on its right, else its left, else behind the player: the first spot from whose
  eye (110 units up) nothing but the mark's actor stands in the way (`helper_spot`). All
  blocked: no helper. The helper is a new reference of their NPC_ in the player's cell,
  turned to the mark (`Game::summon_vats_helper`, made as PlaceAtMe makes them). The
  Stranger's music plays as he comes (A01 or A02).
- **The shot**: `HELPER_DRAW` (0.3 game s) after appearing, one shot of their weapon from
  their eye straight at the mark's point (an `ActorFire`, no spread), through combat's own
  path: its sounds, projectile, hit, damage and the weapon's object effect, all the
  records'. The Stranger's 9,000 kills; Miss Fortune's 10 and her object effect do what
  its script rolls. They are the shooter, so the kill is not the player's (no XP, no
  Grim Reaper's Sprint) [design].
- **Leaving**: `HELPER_STAY` (1.2 game s) after appearing they fade out (Disable 1) and
  are deleted (MarkForDelete) (`Game::dismiss_vats_helper`), whether the mode is still on
  or not.
- **Counting**: each Stranger visit adds 1 to Mysterious Stranger Visits, so 15 visits
  complete "He Moves in Mysterious Ways" and give Friendly Help through its script. Miss
  Fortune's visits are not counted [observed: the statistic and the challenge name only
  the Stranger; the fandom wiki's Friendly Help says "15 Mysterious Stranger or Miss
  Fortune visits", which the records do not bear out].
- Not built: the helpers' own attack clips (the body stands while the shot leaves), the
  original's kill camera, a helper's line.

### 18.3 Grim Reaper's Sprint [design where the wiki is silent]

When the mode ends after firing, if the player's kills (People Killed + Creatures
Killed) rose since E, the Player Kill AP Reward entries give that much AP back (from 0;
Grim Reaper's Sprint + 20), no more than was spent. Kills by a helper are theirs, not the
player's. Log `vats mode: <n> killed: the Player Kill AP Reward perks give <r> AP back
(<b> restored)`.

### 18.4 Nerves of Steel

The AP's regeneration outside the mode (§7.2) is `fActionPointsRestoreRate` × the maximum
× the Action Point Regen entries (Nerves of Steel × 1.2; Atomic! × 1.1 … 1.5 by its rads;
Ain't Like That Now × 1.25, their own conditions deciding). Log `vats mode: AP
regenerates at <m> x (the Action Point Regen perks)` when the factor changes.

### 18.5 Test aids, logs and checks

- `NVF_PLAY_TEST_VATS_HELPER=stranger|fortune|none`: that helper comes for every mark
  whose odds for them are above 0 (the player still needs the perk), or none.
- Logs: `vats mode: <helper> comes for mark <k> (<ref>), <d> units from the player (<o>%
  odds, rolled <r>)`, `vats mode: <helper> appears (<ref>)`, `… fires at <ref> from <d>
  units`, `… leaves`; the summary's `<h> helpers, <a> AP rewarded`.
- Unit: `fnv-game` `vats` `one_helper_a_roll_the_stranger_first`, `perk_combat`
  `vats_helpers_and_ap_perks`. Smoke: `vats-mysterious-stranger`, `vats-miss-fortune`,
  `vats-grim-reaper-nerves`.

## 19. Kill cameras (2026-10-24)

ESM audit gap 10 (docs/handoff/esm-full-audit-2026-10-24.md): the masters' camera shots
(CAMS, 276) and camera paths (CPTH, 418) were never loaded. They are now, and VATS mode
plays them as kill cameras. Built and unit-tested; the path choice is also checked
against FalloutNV.esm's own tree (`fnv-game` test `camera_paths`, with game data).

### 19.1 What the records say

- **CAMS** [documented: xEdit wbDefinitionsFNV.pas CAMS; GECK wiki CameraShot]: a model
  (`meshes\vatscameras\…`), the action that starts the shot (Shoot when the command
  starts, Fly when the projectile starts to fly, Hit when the target takes the hit),
  where the camera is held and what it looks at (attacker, projectile or target), flags
  (Position Follows Location, Rotation Follows Target, Don't Follow Bone, First Person
  Camera, which ignores the model and uses the first-person view, No Tracer, Start At
  Time Zero), time multipliers (Global slows everything but the camera shot itself; the
  player's and the target's are relative to it), Max and Min Time (how long before the
  next camera), Target % Between Actors, and an image space modifier.
- **CPTH** [documented: xEdit]: conditions, the parent path and the previous sibling,
  the camera zoom and the shots. The paths form a tree [observed: FalloutNV.esm]:
  `PlayerDeath`, `MrHouseKC`, `MysteriousStrangerCameras`, `MustDoCam`, `DEMOCams`,
  `MeleeWeaponAttackKC`, `MeleeWeaponAttack`, weapon roots (`SpecMiniGun`,
  `MissileLauncherCams`, `FatManCams`, `FlamerAttacks`, `ThrownCams`, `GrenadeCams`),
  `CloseCameras`, `DefaultMissCams`, `DefaultHitsKC`, `DefaultHits`. Under them the
  conditions are GetVATSValue (Is Success, Is Fatal, Target Distance, Weapon Type Is,
  Explode/Dismember Part, VATS Action 15 or 18), GetRandomPercent (to vary the shot),
  GetDistance with no reference, and eight functions only xEdit names: 518–521
  GetVATS{Right,Left,Back,Front}AreaFree and 525–528 GetVATS{…}TargetVisible, compared
  with 40 to 320 [documented: xEdit wbCTDAFunctions, the names and a reference parameter;
  observed: the comparisons].
- **The models** [observed: `simplefronthitl03.nif`, `snipercamhit01.nif`]: a root node
  moved by its own transform controller for about 17 s (Euler keys of a few degrees, a
  hand-held sway; a few translation keys) over an `NiCamera` 75° across.

### 19.2 When a kill camera plays [design]

VATS mode fires its marks in one run (§9), so a kill camera follows only the run's last
mark: from the frame every mark's attack has started until 2 game seconds after the mode
is off (a slow projectile), the last mark's actor is watched, and if it dies the kill
camera starts. With the masters' data a kill camera plays on every such kill: the
original's chance of one is not in our notes (the tree's GetRandomPercent conditions
only choose between shots).

Since 2026-11-10 (the fork owner's playtests, docs/design/vats-classic.md §8.1), in both
styles:

- A mark still to play whose actor has died is dropped, and with none left the mode ends
  2 real seconds later. Such a kill, by an earlier mark, shows no kill camera.
- The last mark's actor is watched while a classic attack's camera still plays, and the
  kill camera takes over from it. A near target died inside that camera, was dead before
  it was watched, and got no kill camera.
- A corpse has no hit volume once its ragdoll is in the world (`hitvol.rs`). The camera
  then holds to the ragdoll's centre, followed smoothly (0.1 s), and to the last place
  seen when there is none. Before, it lost the target a frame after the kill and stood
  about the attacker, with the room about the two measured as none.
- The last shot is held 3 real seconds longer.

The settings page's Kill camera row (`kill-camera` in NVForever.ini, Gameplay; owner's
playtest requests #1035 and #1037, 2026-10-31) narrows that [design: combat is not a
parity target; the original is thought to play one on any killing shot, hypothesis]:

- **Last enemy** (the default): only when the kill leaves no other living actor in
  combat with the player, counted by the player's IsInCombat rule (an actor in Alert,
  Combat or Lost with the player on its target or alert list,
  `fnv_game::awareness::is_in_combat`). Otherwise none plays and the view stays the
  player's.
- **Every kill**: on every such kill, as before.
- **Off**: never.

While one plays (#1034, #1036) [design] the HUD, the crosshair and the corner and quest
notices are hidden (the notices wait, as under a message box), and the gameplay controls
are held: every key and button but Menu Mode, Wait, Quick Save and Quick Load (a menu or
the pause menu ends the camera, §19.4), the mouse and stick look, the weapon wheel, and
the frame's moving, jumping, trigger, aim, reload and ammo swap requests, including the
test aids that stand in for the keys. A fire button held as it starts needs a fresh
press after it.

### 19.3 Choosing the path [hypothesis]

`fnv_game::camera_paths`, `Game::pick_camera_path`: the tree is walked from the roots in
order (siblings by their previous-sibling links); the first path whose conditions pass is
entered; its first child chosen so wins, else the path itself when it has shots; a
passing path that gives nothing is passed over for its next sibling. The conditions run
on the player with the target as target, while:

- GetVATSValue answers about the last shot, with what is known once it landed: Is
  Success 1, Is Fatal 1, Is Critical from the killing hit, Explode Part and Dismember
  Part when a part of the target is gone (`Game::kill_outcome`), Cripple Part 0 [design];
  VATS Action stays 1 only for action 0 (§12), so the `…KC` paths (action 18) and
  `PlayerDeath` (15) are not taken [hypothesis: 18 is the original's kill camera outside
  VATS, `fKillCameraCooldown`; 15 the player's death].
- A GetDistance naming no reference measures to the target [hypothesis].
- The area functions read the room measured by rays at the moment of the kill
  (`nvf_play::kill_cam::measure_room`): AreaFree, how far the way is free from the
  attacker's middle to its right, left, back and front of the level line from the
  attacker to the target; TargetVisible, the same from the target's middle (up to 1024
  units) [hypothesis: from the names; the paths pick a side camera only where these
  pass, observed].

From the chosen path the shots started by the hit play in order; a path with none plays
its last shot (`camera_paths::hit_shots`) [design: the choice is made once the shot has
landed, so its Shoot and Fly shots have passed].

### 19.4 Playing a shot

`nvf_play::kill_cam`:

- It plays for its Max Time in real seconds (0.5 to 10) [hypothesis: with nothing to cut
  to sooner, the most the page allows]; the world's clock runs at its Global time
  multiplier (0.25 for most hit shots [observed]) in place of VATS mode's own scale, with
  the sounds slowed as for VATS mode (§4); the player's and the target's multipliers are
  not applied [design: one clock for the world]; its image space modifier plays.
- The camera [hypothesis: the models carry no frame; this one puts their cameras where
  the shot names say, a front shot's at (−59, −226, 18) between the target and the
  attacker, observed]: the model's animated root in a frame centred on the actor the shot
  is held by (its Location; a projectile counts as the attacker) at its middle, +Y level
  from the attacker to the target, +Z up; the frame follows the actor with Position
  Follows Location, else it stays where the shot began. It looks at its Target actor's
  middle moved Target % Between Actors of the way to the other actor (followed with
  Rotation Follows Target, else as at the start), plus the model's own turn since its
  first key as a sway. Don't Follow Bone is not told apart: the middle is used for every
  shot [design]. It keeps in front of walls between the look point and it.
- A First Person Camera shot leaves the view where it is (the slow motion and the
  modifier still play).
- While a model's camera has the view, it counts as third person
  (`PlayerControl::cinematic`): the body is drawn, the first-person arms are not.
- The player's death or any menu ends it.
- Not built: the No Tracer flag; the shots' fields of view
  (the player's stays); the death cameras (§19.5).

### 19.5 Death cameras

The `PlayerDeath` paths (`PlayerDeathDefault`, `…Back`, `…Above`; VATS Action 15) are
loaded but not played: the death camera is the owner's own design (`death_cam.rs`, owner
2026-10-16), so the masters' one waits for the owner's word (docs/owner-review.md).

### 19.6 Log lines and checks

- `kill camera: <name> <ref> killed by the last mark: path <EDID> (<FormID>), shots
  <EDID>, …; room right/left/back/front …, target seen …`, or `… no camera path chosen`
- `kill camera: shot <k> of <n> <EDID>: <model>, held by the <Location>, looking at the
  <Target>, <s> s at world speed <x>[, image space modifier <EDID>]`
- `kill camera: skipped, <n> enemies still in combat (<name> <ref>, …)` (Last enemy) or
  `kill camera: off in the settings` for a kill that shows none
- `hud: hidden: the kill camera has the view` and `hud: back after the kill camera`;
  `test aid: look … ignored: the kill camera has the view`
- `kill camera: done (<s> s[; <why cut short>]), input held <n> frames` (frames in which
  something asked of the character was struck); at exit `kill camera summary: <n> shown,
  <n> skipped, <n> off`
- Unit: `fnv-game` `camera_paths` (tree order, shot data, hit shots) and the
  `camera_paths` test file (the choice on a synthetic tree; the masters' tree with
  `FNVLAB_DATA_DIR`), `nvf-assets` `camera_shot`, `nvf-play` `kill_cam` (frame, look
  point, length, the setting's names, the held requests).
- Smoke: `vats-kill-cam` (one mark on a Goodsprings dog left with 1 health, the Fiends
  about it killed first: the shot kills it, a path and its shots play, the HUD goes and
  comes back, a held W and a look are held, the camera ends),
  `vats-kill-cam-others-fighting` (another dog set on the player: skipped) and
  `vats-kill-cam-off` (the row set to Off: none).

## 20. Placed objects as targets (2026-10-25)

- A click whose ray first meets a placed object marks it when `Game::vats_targetable`
  allows: a script's SetVatsTarget for that reference (kept on it, saved), else its
  base's DEST flag "VATS Targetable" [documented: GECK wiki SetVatsTarget, "turns VATS
  targeting on or off for a destructible object, overriding the object's base setting";
  xEdit wbDEST]; never once destroyed. In the masters the flag is on 8 activators
  (generators, fuse boxes, a fire extinguisher, the Lonesome Road maintenance bots), 16
  movable statics (the trucks, the motorcycle, the vertibirds, a munition shell) and the
  thrown weapons' projectiles; the traps' scripts turn it on when they are armed
  [observed: FalloutNV.esm and the DLC masters].
- The whole object is marked at the hit point, at the whole-actor base chance
  (`WHOLE_TO_HIT`) [design]; the point does not move. Shots, bursts, throws and melee
  swings go at it as at an actor's mark; its damage is the object's destruction
  (destruction-stages.md).
- Log `vats mode: marked object <name> <ref>, mark <k>; …`; a click on an object VATS
  may not target logs `nothing to mark under the crosshair (<ref> is not a VATS target)`.
- Not built: thrown grenades and placed mines in flight or on the ground (projectiles,
  not placed objects).

## 21. Projectiles as targets (the Tweaks tab, 2026-11-13, #1891)

[design: Stewie's Tweaks' `bTargetProjectilesInVATS` and its options, our own way] With
the Tweaks tab's Target Projectiles in V.A.T.S. on, a click marks a missile or lobbed
projectile in flight (not a hitscan shot, beam or flame) whose path passes within 24
units of the crosshair's line, or within 0.03 rad of it far off, nearer than what the
line meets; the one nearest the line by angle. Not the player's own (unless V.A.T.S.
Targets Your Own Projectiles), not thrown mines (unless Mines Too), only explosive ones
under V.A.T.S. Ignores Non-Explosive Projectiles. The mark's chance is the usual work
(§17) on a held weapon's size with no body part: the distance and the player's skill.
The mark follows the projectile and is dropped when it no longer flies. A landed shot
ends it where it is, its explosion going off there as the player's (`Combat::shoot_down`).
With Mines Too, an armed mine lying in the world is marked as a placed object (§20) and
the shot sets it off as any destructible. The classic style's target list still holds
actors only.
