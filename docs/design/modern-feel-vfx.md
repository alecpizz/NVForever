# Modern Feel: shell casings and impact effects

Part of the fork owner's "modernize the game feel" experiment (2026-11-20, branch
`feat/modern-feel`). Two switches of NVF Features › Modern Feel:
`ModernFeel::shell_casings()` and `ModernFeel::impact_effects()` (both on by default, off
with the Global Toggle). Everything here is NVForever's own [design]; every constant is an
engineering choice tuned by eye. Code: `engine/nvforever/src/feel_casings.rs`,
`engine/nvforever/src/feel_impacts.rs`, `crates/fnv-phys/src/casings.rs`.

## 1. Shell casings

**What the original does** [documented: combat-rules.md §16]: a weapon with a WEAP casing
model throws it from the weapon model's `ShellCasingNode` along the node's +z at
`fGunShellEjectSpeed` (160), at the shot or at the attack clip's `Eject` key; NVForever
draws it falling and bouncing as a point (`shot_fx::casings`), the player's only. While
the switch is on that path throws nothing and `feel_casings` throws instead; switched off
at run time, every Modern Feel case goes at once and the original's come back.

**Which shots throw one** (`feel_casings::eject_rule`), for a gun: skill Guns (or
Explosives: the grenade launchers' shells), not thrown, placed, swung or an energy weapon:

| Weapon | Rule | Examples |
|---|---|---|
| has a WEAP casing model | one each shot (the player's at the `Eject` key where the clip has one) | 9mm, 10mm, service rifle, varmint, cowboy repeater, minigun, grenade launcher |
| break action: not automatic, clip of 1 or 2 | the spent ones fall out at the reload | single, caravan and sawed-off shotguns |
| pistol with no casing model, clip ≤ 6 | the spent ones fall out at the reload | .357, .44, Ranger Sequoia, hunting revolver |
| other gun with no casing model | the small pistol case each shot | silenced .22 pistol |
| energy weapon with a casing model | none | laser rifle, gatling laser (`AssaultRifleShellCasing.NIF` left from the earlier game [observed: `fnvtool combat fx`]) |

The reload drop's model is picked by the weapon's Editor ID (.357, .44, 45-70, shotgun hull,
else the 10mm/.45 case): no record links a revolver to a case [design]. Other actors drop
nothing at reloads (their reloads are not tracked here).

**From where.** The player's: the drawn `ShellCasingNode` (`PlayerBody::casing_entity`). In
first person the arms rig is drawn widened across the view about the eye
(`player_body::place_lens`, a narrower field of view without a second camera), so the
node's global transform is already where the gun is seen: the case starts inside the drawn
receiver and leaves it. Without the node: 9 (one hand) or 35 (two hands; the service rifle's node is 43 behind its `ProjectileNode` [observed: `fnvtool nif decode` of `servicerifle.nif`, `ProjectileNode` at (50.6, 5.2, 0), `ShellCasingNode` at (7.8, 5.2, 0.7)]) units behind the
drawn muzzle and 1.5 to the right. Other actors': their weapon model's `ShellCasingNode`,
read with the muzzle on the part's worker thread (`PreparedPart::casing`) and placed as a
`CasingPort` under the muzzle marker (so it goes with it); else behind the muzzle. Only
within 2,100 units of the player.

**Throw.** Along the node's +z (the original's way out) at the eject speed ±20 %, turned up
to 14° at random, plus a lift of 27–63 units/s and 17–52 back against the shot, plus the
player's own motion (smoothed from the feet); tumbling end over end at 14–28 rad/s and
spinning about its length up to 12 rad/s.

**Physics** (`fnv_phys::casings`): a dynamic rapier cylinder sized from the case model's
mesh bounds (long axis, radius a quarter of the other two), friction 0.55, restitution 0.42,
linear damping 0.05, angular damping 1.2 (rapier has no rolling resistance), CCD on, 10 g.
Its colliders' `user_data` is a tag `Owner::decode` does not know, so no query sees a case
(shots, crosshair, camera, the ground probe) and characters walk through them
(`character::solid_for`); the contact hook (`RagdollHooks`) lets a case meet terrain,
references (not the layers the character passes) and other cases, never actors, ragdolls or
gore. Drawn between steps like gore (`drawn_casing_state`).

**Sound.** A change of velocity of 40 units/s or more (beyond gravity's) is a knock: the
case's PHY sound (`PHYCasingPistol`, `PHYCasingRifle`, `PHYCasingShotgun` by model name,
`shot_fx::casing_sound`) at the point, gain = Δv / 160 within 0.25–1, at most 3 a case and
none within 50 ms of the last [observed: 8 cases, 24 knocks].

**Cap and fade.** 48 cases at most; past it the oldest fades quickly. A case lies 8–15 s
(random), then shrinks to nothing over 0.8 s.

## 2. Impact effects

Laid over the game's own impacts (IPCT models and their add-on particles, decals), not
replacing them.

- **Muzzle smoke.** A gun's shot (skill Guns/Explosives, not energy/thrown/placed) puffs
  2–3 soft smoke squares forward from the muzzle (50–130 units/s, slowed hard, 0.9–1.6 s,
  growing to 18–28 units, alpha 0.26), and heats its shooter's barrel by 0.006 × damage
  (0.08–0.45, × √projectiles), heat capped at 2.5, halving every 1.6 s. A hot barrel lets
  thin wisps rise from where its muzzle is now: 9 a second per unit of heat, rising
  12–24 units/s, drifting with a light breeze, 1.6–2.8 s, alpha 0.08–0.24 by heat. Sustained
  fire leaves a smoking barrel. The player's smoke is 0.7 the size (it is before the eye).
- **By surface.** A shot landing on the world (not an actor) casts one short ray down the
  surface normal for its collision material (`fnv_combat::fx::impact_material`; terrain is
  dirt):
  - dirt/grass: 3–5 sand-coloured dust puffs (to 42 units, 1.4–2.4 s), 4–7 dark clods;
  - stone: 2–3 grey puffs, 5–8 grey chips, 0–2 sparks;
  - wood: 1–2 brown puffs, 4–6 splinters (tan chips);
  - metal/hollow metal: 8–14 sparks (260–560 units/s, 0.18–0.5 s, falling at 0.55 g,
    stretched along their flight as seen), and a wisp of dark smoke;
  - glass, cloth, organic, water: nothing more.
- **Bits.** Chips and sparks fly ballistically (gravity, a little drag) and bounce off the
  world: one ray per moving bit per frame (world only), keeping 0.3 of the speed off the
  surface and 0.5 along it, resting below 30 units/s or after 4 bounces, then fading in the
  last fifth of their 1.6–2.6 s.
- **Drawing.** Four pools, each one mesh of camera-facing squares rebuilt every frame
  (one draw each), with the game's textures: `fxsmokesub.dds` (smoke), `fxdustsmoke01.dds`
  (2×2 frames, dust), `fxdebrisgen01.dds` (4×4 frames, chips), `fxtinysparkwht.dds`
  (sparks). Smoke, dust and chips are unlit, their colour scaled by the place's ambient
  light (luminance of `GlobalAmbientLight`'s display values over 0.35, held to 0.22–1; 0.46
  at noon in Goodsprings [observed: log]) so they do not glow at night; sparks are added
  (HDR orange). Caps: smoke 420,
  dust 220, chips 200, sparks 260.

## 3. Cost

- Casings: up to 48 small CCD bodies; a settled case sleeps. Per frame one state read and
  a transform write per live case. Each case is the model's shapes as entities (2–3
  meshes, no shadows).
- Impacts: one ray per impact, one ray per moving chip/spark per frame (dozens at most in
  a firefight), four meshes rebuilt (at most ~1,100 squares), four draws. Nothing runs
  while both switches are off and the pools are empty.

## 4. Rough edges

- Other actors throw their cases at the shot, also lever and bolt actions (their clips'
  `Eject` keys are not read for them), and drop none at reloads.
- The reload drop starts 0.35 s into the reload (scaled by the reload rate), not at a clip
  key; the cases fall from the drawn `ShellCasingNode` or the breech guess.
- Cases are sized as cylinders; a bottle-necked rifle case rolls straight, not in an arc.
- Smoke squares are soft round puffs, not shaped wisps; the breeze is one fixed drift, not
  the weather's wind.
- A lit path was tried first: the engine's lit material drew the squares black; unlit with
  the ambient scale stands in.
- Not ours, seen while testing: the game's own dirt impact (`BalisticDirtImpact`'s model and
  its particles) leaves a large dark cloud drifting from the hit in NVForever [observed:
  the same frames with Modern Feel's impact effects held off, Goodsprings at noon]; it
  hides the lighter dust here.
- Material by the collision under the hit: a surface whose collision material does not
  match its look (painted metal as stone) gets the collision's effect.

## 5. Verification

Smoke scenarios `feel-casings-impacts` (service rifle at the ground, frames of cases in
flight, on the ground and of the barrel smoke), `feel-casings-revolver-reload` (.357: three
shots and a reload) and `feel-casings-npc` (Violet's service rifle). Log lines:
`shell casings: …`, `impact effects: …` and their summaries at exit.
