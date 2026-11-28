# HUD reads, held controls and scripted weapon draws

Plain-language notes for `crates/fnv-game/src/nvse_hud.rs` and the control functions in
`crates/fnv-game/src/script_input.rs`. Sources: the GECK wiki pages of each function
[documented], and the hud1 and controls1 probe sets run in the original game on
2026-10-27 [observed] (`tools/capture/jip_probes.py`; goldens stay private).

## 1. HUD visibility and shudder

- `GetHUDVisibilityOverride` (JohnnyGuitar) returns a bit mask, one bit per HUD element,
  bits 0-16 from Action Points to Hardcore Mode [documented: the page's flag list]. At rest
  it reads 131071, all 17 bits [observed: hud1].
- `SetHUDVisibilityOverride n` stores the mask; it reads back in the same frame and the next
  [observed: hud1, 3]. Whether the engine's HUD hides the elements is not built yet.
- `GetHUDShudderPower` reads 0 at rest [observed: hud1]. Nothing in NVForever shakes the HUD,
  so it always reads 0 [hypothesis].

## 2. The reticle

- `GetReticlePos` gives an array of X, Y, Z where the crosshair points, or ar_Null when the
  spot is not valid [documented]. In game, aimed at the ground or sky, it had 3 entries
  [observed: hud1]. NVForever takes the point from the engine's crosshair ray; a headless run
  has no ray and gives ar_Null.
- `GetReticleTargetLimb [maxRange]` reads -1 with no actor in the crosshair [observed:
  hud1; observed: 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas
  1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 10 official base masters, `#3678` / PR `#3679`
  probe `w1_3606_hit_3d_dir_and_base_damage`: placing Player 150u South of restrained Doc
  Mitchell (`0x00104C0F`, `SetAngle Z 180`) and sweeping `Player.SetAngle X` across `-7`,
  `0`, `15`, and `-80` returned `1` (Head), `0` (Torso), `10` (Right Leg), and `-1` on
  empty space, non-actor statics, and above-head misses; no DLL or source consulted]. On an actor hit by the
  crosshair's far ray, NVForever resolves the hit capsule's BPTD body part to its
  `BodyLocation` code (0 torso, 1–2 head, 3–4 left arm, 5–6 right arm, 7–9 left leg,
  10–12 right leg, 13 brain, 14 held weapon) [documented: GetReticleTargetLimb,
  GetHitLocation], honouring the optional `maxRange` cap (0 or omitted uses the full
  reticle reach). Actors without posed body parts (single-capsule fallback,
  `fnv_phys::actors::WHOLE`) read 0 (Torso, default body location) while respecting
  `maxRange`, so scripts distinguish hitting an actor from a miss; non-actors, misses,
  and detached gore pieces (`fnv_phys::gore::GORE_PART`) read -1 [design].

## 3. Corner messages

- `GetNumQueuedCornerMessages` (ShowOff) counts the corner messages waiting, the one on
  screen included [documented]. Three `MessageEx` calls raise it by 3 in their own frame, and
  it stays there the next frame [observed: hud1, 2 → 5 → 5].
- `ClearMessageQueue` (JIP) empties the queue: 0 in its frame and the next [observed: hud1].
- Messages shown without a box (MessageEx, ShowMessage of a non-box message) join the
  queue. `MessageExAlt` also increments the corner-message count for
  `GetNumQueuedCornerMessages` without enqueueing ShowOff `corner_pending` handlers
  [design]. In headless `fnv-game`, the one on screen leaves after 2 seconds [hypothesis:
  the pace was not measured].
- The 2 messages already queued when hud1 started came from loading the save. A headless run
  starts with none.
- In `nvf-play` (`engine/nvf-play/src/corner_messages.rs` and `script.rs`), the live
  `CornerMessages` queue is synced into `Game::hud` so `GetNumQueuedCornerMessages` reflects
  both script-queued and engine-generated corner notices (showing + waiting, paused while
  held behind menus) without running the headless 2-second expiry timer alongside it [design].
- `ClearMessageQueue` clears both waiting and currently displayed messages in
  `CornerMessages` (despawning any active HUD area immediately, including while held behind
  a menu), suppresses same-frame notifications emitted before the call while keeping
  messages queued after it, and clears pending note notices [documented: GECK wiki
  ClearMessageQueue; observed: hud1, 0 in its frame and the next; design: engine queue sync].

## 4. The compass

- `GetNumCompassHostiles [maxRange] [flags]`, `GetCompassHostiles` and
  `GetNearestCompassHostile` count, list and pick the hostile compass targets [documented].
  A placed NPC that is calm counts 0; 60 frames into a fight with the player it counts 1, is
  the one listed and is the nearest [observed: hud1]. With maxRange 100 and the NPC about
  500 units off, the count is 0 [observed]. With the NPC removed, all read 0 again.
- NVForever counts every loaded, enabled, living actor in a combat state whose targets hold
  the player, at any distance [hypothesis: the compass's own range is not measured], nearest
  first. Flag 1 (no invisible actors) changes nothing, as nothing is invisible to the compass
  yet [hypothesis].
- `GetCompassTargets 0/1/2` (ShowOff) read an empty array in every case, in the fight as well
  [observed: hud1; the page lists bugs]. NVForever gives an empty array.

## 5. Controls

- `SetControl control code [type]` rebinds a control's key (type 0) or mouse button (type 1)
  [documented]; `GetControl` reads the new code in the same frame and the next [observed:
  controls1, 48]. The original writes it to its INI on exit [documented]; NVForever's engine
  changes its own bindings and writes nothing of the game's. For type 1 the code is taken as
  GetControl gives it (256 + n), or as the bare n when under 256 [hypothesis].
- `HoldControl c` holds the control's key (else its mouse button) down until
  `ReleaseControl c` [documented]. Both take effect from the next frame: `IsControlPressed`
  reads 0 in HoldControl's frame and 1 two frames on, with or without IgnoreControlStatus,
  and 1 in ReleaseControl's frame, 0 ten frames on [observed: controls1]. A held Forward
  walks the player; after the release the player stops within 10 frames [observed].

## 6. Scripted weapon draws

- `SetWeaponOut 1` draws the equipped weapon and `SetWeaponOut 0` holsters it; `HolsterWeapon`
  holsters a drawn one [documented].
- IsWeaponOut after `SetWeaponOut 1`: 0 in its frame and at 10 frames, 1 from 30 frames on.
  After `SetWeaponOut 0`: 1 at 10 frames, 0 from 30 on [observed: controls1].
- After `SetWeaponOut 1`, `HolsterWeapon` makes IsWeaponOut 0 in its own frame and at 5 and
  10 frames, then 1 again from 15 to 120 frames: the weapon comes back out [observed:
  controls1]. NVForever reads this as SetWeaponOut 1 holding the weapon drawn until a
  SetWeaponOut 0 [hypothesis]; the hold also counts in the rule NPCs draw by.
- NVForever changes IsWeaponOut 12 frames after a scripted draw or holster, a figure that fits
  all three traces [hypothesis: the real length is the animation's]. With the engine running,
  the engine's animation tells the state instead; the engine takes the player's asks as it
  takes ToggleFirstPerson's.
