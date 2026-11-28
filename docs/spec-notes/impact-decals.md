# Spec notes: impact decals (bullet holes, scorch marks, blood)

Clean-room description in plain language. Tags as in CLAUDE.md: **[documented]** cites a
public source, **[observed]** was measured on the owner's install, **[hypothesis]** needs
verification, **[design]** is our choice where the game's rule is not known.

A shot that lands shows its impact (IPCT) there: a model, sounds (explosion-effects.md,
audio.md §11) and a decal, a texture laid on the surface it hit. Code: the records in
`crates/fnv-combat/src/fx.rs` (`Impact`, `DecalData`, `TextureSet`, `FxForms::decal`);
drawing in `engine/nvforever/src/decals.rs`; the actor struck travels in
`nvf_play::combat::ImpactFx::actor`. `fnvtool combat fx` prints each impact's decal.

## 1. The records

- **IPCT** [documented: xEdit wbDefinitionsFNV IPCT; GECK wiki ImpactData]: `DATA` holds
  the effect's duration and orientation, the *angle threshold* ("Affects how far around
  in degrees a decal renders"), the *placement radius* ("Affects the distance between
  decals"), the sound level and a flag, "No Decal Data". `DODT` is decal data, `DNAM`
  the texture set ("TextureSet to be used for Decal").
- **DODT** Decal Data [documented: xEdit wbDODT; GECK wiki ImpactData]: min and max
  width, min and max height, depth ("How far from the point of impact the decal
  renders"), shininess ("Sets the specular exponent"), parallax scale and passes, flags
  (Parallax, Alpha - Blending, Alpha - Testing) and a colour ("Multiplies the texture by
  the selected color").
- **Whose decal data holds**: the GECK's IPCT dialog has "Has Own Decal Data: Overrides
  the Texture Set decal data" [documented: GECK wiki ImpactData], and a TXST may carry a
  DODT of its own ("Decal Data: Gives the texture set additional properties when used as
  a dynamically-generated decal", [documented: GECK wiki Texture Set]). Read here: the
  IPCT's flag "No Decal Data" is the dialog's "Has Own Decal Data" left off, and then the
  texture set's DODT is used [hypothesis]. It fits the data [observed, below]: the one
  IPCT with the flag and a texture set (`BallisticOrganicBBImpact`) names the one texture
  set with a DODT (`DecalOrganicBallisticBB`, 0x0001A32C), and no IPCT with the flag has
  a DODT.
- **TXST** [documented: xEdit TXST; GECK wiki Texture Set]: `TX00` base image /
  transparency, `TX01` normal map / specular, `TX04` parallax (height) map; `DNAM` flag
  "No Specular Map".

### What FalloutNV.esm holds [observed: `fnvtool esm decode` of every IPCT and of the TXSTs they name, 2026-10-20]

- 125 IPCTs: 90 with a texture set and their own DODT, 1 with a texture set and "No
  Decal Data" (above), 29 with a DODT but no texture set (nothing to draw), 5 with
  neither. 14 texture sets are named, all with a base image under `textures\decals\`.
- The flamer's and the Incinerator's impacts: `FlamerScorchBigImpact` (dirt and grass)
  and `FlamerScorchStoneImpact` (every other material), both on `DecalDirtScorch`
  (0x00019073, `ImpactDecalScorch01.dds`, normal map `DecalFlatNormal.dds`). Big: width
  150–250, depth 96; Stone: width stored 120 then 80 (min above max), depth 16.
  `ExplosionDirtImpact`: width 256–425, depth 96. Bullet holes: `BallisticStoneImpact`
  10–28 (depth 16), `BallisticMetalImpact` 8–15, `BallisticWoodImpact` 8–32,
  `BalisticDirtImpact` 23–31; blood `BallisticOrganicImpact` 12–19 (depth 50,
  shininess 100).
- **Heights**: 71 of the 90 own DODTs of IPCTs with a texture set (100 of all 119
  DODTs) store height 8–32 whatever their width (the scorches' 150–250 and 256–425
  among them); of the rest, 2 store the width range again (`AntVomitImpact` 60–100 ×
  60–100), 2 a band 16–64 (`BloodSpoutRedData` 70–90 × 16–64), and 15 other narrow
  ranges (8–24, 8–16, 6, 5, 2, 1).
- **The decal textures** hold four marks each in a 2 × 2 grid: four holes of different
  size in `ImpactDecalConcrete01.dds`, four scorches in `ImpactDecalScorch01.dds`, and so
  for the metal, wood, flesh and dirt ones (256 × 256; dirt 128 × 128) [observed: the
  textures from `Fallout - Textures.bsa`, looked at].

## 2. Reading of the shape [hypothesis and design]

- **One quarter of the texture** is laid down, picked at random [hypothesis: from the 2 ×
  2 grids; drawn whole, a bullet would leave four holes of four sizes at once].
- **Square, its side from the width range** [design]: a scorch 150–250 wide and 8–32 high
  would be a strip, which neither the round scorch marks of the texture nor the use of
  the range (one mark per impact) fits; the height pair 8–32 looks like a default left in
  most records (§1). The height range is not used. A record whose min is above its max
  (`FlamerScorchStoneImpact`) is read with the two swapped.
- **Depth**: the decal shows on surfaces within its depth of the plane through the point,
  along the normal, fading out towards that depth (Bevy's forward decal fades
  linearly) [hypothesis: the GECK's "How far from the point of impact the decal
  renders"]; at least 4 units [design].
- **Turned about the normal at random** [design].
- **Colour**: the base image × the DODT colour (as sRGB), alpha from the base image.
  Normal map `TX01` read as the NIF shapes' (green down). Specular exponent = shininess,
  as a NIF material's glossiness (nif.md §6); with "No Specular Map" no specular,
  otherwise masked by the normal map's alpha as a NIF shape's [hypothesis].
- **On actors**: Fallout_default.ini has `bDecalsOnSkinnedGeometry=1` [observed], read as
  decals on actors' (skinned) bodies being on. The decal hangs from the actor's
  skeleton bone (`Bip01…`) nearest the point, so it moves with that part [design]. The
  player in first person has no skeleton drawn and gets none.

## 3. How many and how long

The game's `Fallout_default.ini` [observed, 2026-10-20]: `[Display] fDecalLifetime=10.0000`,
`[Display] iMaxDecalsPerFrame=10` (2 in `low.ini`, 5 in `medium.ini`, 10 in `high.ini` and
`VeryHigh.ini`), `[Display] bDecalsOnSkinnedGeometry=1`, `[Decals] uMaxDecalCount=100`,
`bProfileDecals=0`, `bDecalOcclusionQuery=1`; the quality INIs also set `fDecalLOD1=500`,
`fDecalLOD2=1000`. Their meanings are read from the names [hypothesis].

**Decals are not kept** [design: the owner, 2026-10-20, "make sure the scorch marks and
bullet holes arent persistent i dont want to jam up the engine remembering all these"]:
a decal is only a drawn entity. It is never written to a save, nor remembered by a cell
or an actor; it hangs from the drawn cell it lies in (or the actor's bone), so it goes
when that cell unloads (or the actor is removed). What else limits them, all
[design] constants in `decals.rs`:

- `MAX_DECALS` = 64 alive at once, a fixed global cap below the game's
  `uMaxDecalCount=100` (the owner asked for a small one); a new one removes the oldest;
- `DECAL_LIFETIME` = 10 s, as `fDecalLifetime`; then it fades out over 1 s
  (`DECAL_FADE`; the game's way of removing one is not known);
- `MAX_DECALS_PER_FRAME` = 10 new decals a frame, as `iMaxDecalsPerFrame` (the high
  setting); more in one frame are not placed;
- no new decal within the IPCT's placement radius of a live decal of the same texture
  set [hypothesis, from the GECK's "Affects the distance between decals"]: a flamer's
  stream lands every frame and leaves one scorch per 16 units, not one per frame;
- none outside the drawn cells (nothing would hold it);
- `fDecalLOD1/2` (perhaps distances at which decals thin out) are not used.

The log's `decals summary` line gives how many were placed, the most alive at once and
how many went at the cap, by fading and with their cell or actor (smoke
`impact-decals-cap`).

## 4. Blood on actors (playtest list 7 item 27, 2026-10-23)

The owner: blood decals "appear floating off the mesh, at the wrong rotations and look
sideways", and too many. Cause [observed in code]: a shot meets the actor's hit volume
(capsules round the bones, hitvol.rs), not its drawn skin, and the decal (a flat quad facing
the hit normal) was put there, at the capsule's point and normal. The forward decal draws
on whatever lies within its depth of its plane, so off the skin it showed in the air or
turned with the capsule.

- **Onto the body** [design]: the shot's line (from 48 units before the hit along the shot
  to 48 past it) is cast against the actor's drawn shapes as they are posed this frame:
  each shape's vertices skinned on the CPU (the joints' world matrices times their inverse
  bind poses), shapes drawn with no triangles (exploded parts) and hidden ones left out
  (`decals::ray_shape`). The decal goes where the line first meets a triangle, facing that
  triangle's normal, and hangs from the bone nearest that point. A line that meets no
  drawn shape (a hit on the capsule beside an arm) leaves no decal.
- **Fewer and thinner** [design: "tone down"]: at most `MAX_PER_ACTOR` = 3 alive on one
  actor, at least `ACTOR_GAP` = 0.3 s between two on it (a shotgun's pellets leave one),
  no wider than `ACTOR_MAX_SIDE` = 16 units and shown within `ACTOR_DEPTH` = 4 units of
  their plane, so the flat mark does not show past a limb's curve.
- Log at exit: `decals: on actors, <n> hits whose line met no drawn shape left none, <m>
  were left out by the actor limits`. Smoke `impact-decals-blood`.

## 4a. Blood on the world behind a shot actor (#3318)

A player's report (v0.8.1): shooting someone left blood on the body but none on the walls
or floor near them. Cause: the shot's impact went to the actor's body only (§4;
`engine/nvf-play/src/combat.rs`, `fly`, the one `show_impact` of an actor hit, before
#3318); nothing put blood on the world.

- **What the original does** [documented: GECK wiki ITR:OnWoundSpray, an extender's event
  page]: after a hit on an actor, when the game setting `fCombatEnvironmentBloodChance`
  rolls in favour, the engine casts from the impact point in the projectile's direction
  and paints one blood splatter, the hit body part's impact, on the first valid wall,
  floor or other static surface within 512 units; at most one a hit. It does not when
  the actor cannot have blood, wears power armour (torso; helmet for head and torso
  hits), is too far off or out of view, or over a cap of combat actors. Distinct from
  the decals of a limb severed or blown apart (dismemberment.md; §5).
- **Ours** (`nvf_play::combat::world_blood` and `splatter_impact`, sent from the shot's
  impact in `fly`, whether or not the weapon's impact data set names an impact for the
  actor): a shot (not a melee blow [design]) on an actor rolls its own random numbers
  against the chance unless:
  - the shot is harmless (a scripted Do No Damage shot, #2433);
  - the actor's own blood decal is suppressed for this hit: flagged No Blood Decal
    (actor-flags.md §2) or stopped by the Armor Prevents Blood Decals tweak (read from the actor
    and the tweak, not from the weapon's impact). The limits of §4 (three alive, one each 0.3 s) are not suppression: a
    follow-up torso shot within the body decal's 0.3 s remains eligible to roll;
  - the hit is on the held weapon or a gore piece, whatever the tweaks;
  - the actor's Impact Material Type is not organic (organic, organic bug, organic glow)
    [design]: a robot's oil is not built;
  - the struck body part (its BPTD part, from the actor's published posed parts; a hit on
    the whole volume of an actor with published parts reads the first part [design]; an
    actor without published parts, one whose skeleton gives no limb chains, gets none)
    has no blood spurt / decal impact data set (the
    severable one, else the explodable [hypothesis: observer check 263]) [documented:
    GECK wiki Body Part]. Its impact for the actor's material is the splatter, not the
    weapon's impact data set's, whose organic slot can hold another effect (a flamer's
    scorch).

  On a win, a line is cast from the hit along the shot, within 512 units, against the
  world only (walls, rocks, the ground, wrecks: what shields from a blast, not bodies,
  clutter, small or see-through things or trees); if it meets nothing, a second line
  straight down from the hit [hypothesis: the wiki names only the projectile's direction
  and a loop of casts; observer check 263]. The first surface met gets the splatter as a
  decal only (`ImpactFx::splatter`): no model, no sound, no noise. It is placed as any
  world decal (§2, §3): its cell, the placement radius, the frame cap, the life and the
  global cap.
- **The chance**: `fCombatEnvironmentBloodChance`, a fraction held to 0..1; FalloutNV.esm
  does not set it, so its default 0.75 holds [documented: GECK wiki
  FCombatEnvironmentBloodChance, "Default = 0.7500", "the chance blood sprays from a actor
  will lay blood on the environment"]. The log says the chance and the setting's value at
  the first roll (`combat: blood on the world behind shot actors: chance ...`).
- Not built: the power-armour, distance, view and combat-actor-cap exceptions; melee;
  robots; actors without published body parts (their BPTD is not looked up apart from
  the posed colliders).
- Log: `decals: <IPCT> splatters blood (<TXST>) on the world behind a shot actor at ...`
  for the first; at exit `combat summary: blood on the world: <n> shots on actors rolled,
  <w> won the roll, <s> met a wall or floor`, one line per actor struck (`combat summary: blood
  on the world behind <ref>: <s> splatters from <n> rolled hits`) and `decals: blood on the world behind shot
  actors: <s> splatters sent, <p> placed`. Smoke `impact-decals-blood`.

[design] A world decal on a solid receiver below water blends with its receiver before
the scene colour is copied for refraction. A shot's water entry plays the weapon's water
impact with decals disabled. A subsequent hit on the bed can leave its own material's mark.

## 5. Not built, open

- **Angle threshold** ("how far around in degrees a decal renders"): read as the largest
  angle between the decal's normal and a surface it is drawn on (glass 180, flesh 90,
  the big scorches 65, bullet holes in stone 15) [hypothesis]; the forward decal knows
  only depth, not the surface's normal, so a decal drawn on a corner bends round it within
  its depth whatever the threshold.
- Parallax (`TX04`, the Parallax flag, scale and passes), alpha testing (every decal is
  blended), the height range (§2), decals on water, `bDecalOcclusionQuery`.
- Blood decals of dismemberment (BPTD "Decal Count") and `AddDecal`.
- To compare with the game: bullet holes in a wall and a flamer's scorch on the ground at
  a known hour, their size and how long they stay (compare-frames rules in
  owner-review.md).
