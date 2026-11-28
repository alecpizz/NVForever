# Spec notes: image space modifiers (screen effects)

Clean-room description in plain language. Tags: **[documented]** cites a public source,
**[observed]** was measured on the owner's install (FNV 1.4.0.525), **[hypothesis]**
needs verification, **[design]** is our choice where the game's rule is not known.
Counts come from `crates/fnv-world/examples/imad_dump.rs` over `FalloutNV.esm` and from
`fnvtool script decompile` of the same plugin.

An image space modifier is an `IMAD` record played over the picture: a fade to black or
white, blurred or doubled vision, a tint, a change to brightness or contrast. The same
record type grades a weather by time of day (world.md §6). Scripts play one with
`ApplyImageSpaceModifier` (`imod`), end it with `RemoveImageSpaceModifier` (`rimod`) and
ask about it with `IsImageSpaceActive`.

Code: `fnv_world::image_modifier` (the record, its tracks, the effect at a moment, several
at once), `fnv_game` (`Game::image_modifiers`, the three script functions),
`engine/nvforever/src/imagespace.rs` with `imagespace.wesl` and `imagespace_blur.wesl`
(the picture).

## 1. The IMAD record

- **DNAM** [documented: xEdit wbDefinitionsFNV.pas IMAD DNAM], little-endian:

  | Bytes | Field |
  |---|---|
  | 0 | "Animatable", u32 used as yes/no |
  | 4 | "Duration", f32, seconds |
  | 8 | 21 pairs of u32 key counts (multiply, add): 14 HDR parameters, 3 bloom, 4 cinematic |
  | 176 | key counts, u32 each: tint colour, blur radius, double vision strength, radial blur strength, radial blur ramp up, radial blur start |
  | 200 | "Radial Blur - Use Target", u32 used as yes/no |
  | 204 | radial blur centre X, Y, f32 each |
  | 212 | key counts, u32 each: depth of field strength, distance, range |
  | 224 | "DoF - Use Target", u8 used as yes/no |
  | 225 | "DoF Flags", u8: bit 0 "Mode - Front", bit 1 "Mode - Back", bit 2 "No Sky" |
  | 226 | 2 unused bytes |
  | 228 | key counts, u32 each: radial blur ramp down, radial blur down start, fade colour, motion blur strength |

  244 bytes in 196 of the 215 records; 240 in 8, 236 in 10 and 188 in 1, the fields
  past the end missing [observed]. Every key count equals the number of keys in its
  subrecord: 11,825 counts checked, none differs [observed].
- **Key lists** [documented: xEdit IMAD wbTimeInterpolators, wbColorInterpolator]: each
  parameter is a subrecord of (time f32, value f32) keys; the tint colour (`TNAM`) and
  the fade colour (`NAM3`) are lists of (time f32, red, green, blue, alpha f32) keys.
  - Multiply/add pairs: `\x00IAD`..`\x14IAD` multiply and the same number + 0x40 add
    (`@IAD`..`TIAD`); the names are in schemas/fnv/sky.json. The cinematic four are
    0x11 saturation, 0x12, 0x13 and 0x14 brightness; 0x12 is read as contrast average
    luminance and 0x13 as contrast, the order world.md §6 argues for [hypothesis].
  - Single lists: `BNAM` blur radius, `VNAM` double vision strength, `RNAM` radial blur
    strength, `SNAM` its ramp up, `UNAM` its start, `NAM1` its ramp down, `NAM2` its down
    start, `WNAM` depth of field strength, `XNAM` its distance, `YNAM` its range, `NAM4`
    motion blur strength.

## 2. What the records hold [observed]

- 215 records: 94 animatable, 121 not. 24 of the latter are the weather adapters
  (names ending in `IS`).
- **Key times run from 0 to 1** in every record but one (the weather adapter
  NVWastelandNightIS has keys up to 2), while the durations of animatable records run up
  to 106 s. So a key's time is a part of the duration, not seconds [hypothesis from this].
  One animatable record has duration 0 (HVBunkerExplosionISFX).
- Records that use each effect: brightness 166, tint 127, 0x12 102, saturation 97, 0x13
  93, fade colour 64, blur 62, radial blur 39, double vision 26, motion blur 7, depth of
  field 6. Largest values: blur radius 5, double vision 1, radial blur strength 10, depth
  of field strength 2, motion blur 0.5.
- Radial blur "use target" is set in 6 records (2 of them with centre 1, 1; all others
  0.5, 0.5); depth of field "use target" in 3, its flags are 1 in 2 records.
- **The opening's records:**

  | Record | Animatable, duration | Keys (time: value) |
  |---|---|---|
  | WhiteISFX 0x000AC03C | no (1 s) | fade colour white, amount 0: 1, 1: 0 |
  | MQ08AllBlackedOutISFX 0x00026370 | no (1 s) | brightness multiply 0: 0, 1: 1 |
  | VCG01FadeInFromBlackISFX 0x00120E95 | yes, 7 s | fade colour **white**, amount 0: 1, 0.71: 0; blur radius 0: 5, 0.5: 3, 0.8: 4, 1: 0; double vision 0: 0.1, 0.5: 0, 0.8: 0.05, 1: 0 |
  | VCG01PlayerStrainISFX 0x00120E96 | yes, 10 s | 0x12 multiply 0: 1, 0.17: 2, 1: 1; brightness multiply 0: 1, 0.25: 2, 1: 1 and add 0: 0, 0.02: −0.75, 0.17: 1, 1: 0; blur radius 0: 0, 0.02: 2, 0.17: 3, 1: 0; double vision up to 0.01; radial blur strength 0.17: 0.3, ramp up 0.17: 0.5 |
  | FadeToBlackISFX 0x000269C4 | yes, 4 s | fade colour black, amount 0: 0, 0.33: 1, 0.67: 1, 1: 0 |

  The other records VCG00's graveyard stages play are FadeInFromBlack2sISFX,
  FadeToBlack2sISFX (both 1 s, despite the names) and Addiction01ISFX (6 s).

## 3. Playing one

- **ApplyImageSpaceModifier** plays the modifier, **RemoveImageSpaceModifier** removes
  it from the screen, **IsImageSpaceActive** returns 1 while it is active [documented:
  GECK wiki pages of the three functions, owner export].
- **Animatable** [documented: GECK wiki "ImageSpace Modifiers"]: "If checked, the
  modifier has a duration and is animated. If this is not checked, the duration box is
  ignored."
- **A record that is not animatable stays at its first keys until it is removed**
  [hypothesis]. Evidence [observed]: MQ08AllBlackedOutISFX's brightness multiplier is 0
  at time 0 and 1 at time 1, so only its first key blacks the screen out; WhiteISFX is
  white only at its first key; in the capture of the new-game save the screen stayed
  white for as long as the session was left alone (layer5-captures.md §10.2); and every
  one of the 9 such records a script applies is also removed by a script.
- **An animatable record plays its keys over its duration and then ends by itself**
  [hypothesis]. Evidence [observed]: 29 script lines wait until `IsImageSpaceActive`
  reads 0 on FadeToBlackISFX before going on, and no script removes it; its own keys
  bring the picture back by time 1. Of the 38 animatable records scripts apply, scripts remove 5.
- Between keys the value moves in a straight line; before the first and after the last
  key it holds [hypothesis: xEdit calls the lists interpolators, the curve is not
  documented].
- **Applying one that is already playing stacks another instance in application order**
  [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9, 2026-11-23, GSDocMitchellHouse
  (`0x00103DF9`), 10 official base masters, clean-room script probes
  `w1_3656_objectives_unholster_imod` (`imod` twice + `rimod` twice on `WhiteNonAnimated`
  `0x000C9AFA`; `imod GetHit` `0x00000162` 0.4 s + ~1.5 s wait) and
  `b3_core_ex_06_on_apply_imod_event` (`ApplyImageSpaceModifier`/`RemoveImageSpaceModifier`
  on `0x00144C41`) in #3679; and 2026-11-24 with JIP LN 57.30, `w4d_24_active_imods_order`
  (`tools/capture/nvse/catalogs/wave4d_extenders.json`, #3679): applying non-animatable
  `0x00054E13`, `0x001736A8`, `0x00054E13` yields `n=3,0=00054E13,1=001736A8,2=00054E13`
  from `GetActiveIMODs`; no DLL or source consulted: re-applying an active IMAD fires
  `OnApplyIMOD` with `IsImageSpaceActive == 1` and stacks another instance; a single
  `RemoveImageSpaceModifier` removes all matching instances and fires `OnRemoveIMOD` once
  per removed instance (twice for two stacked instances). VCG00 also applies
  Addiction01ISFX at stages 27 and 30, half a second apart]. Because the probes measured
  event counts and `GetActiveIMODs` order rather than whether the original blends duplicate
  instances visually, NVForever keeps each stacked instance (animatable and non-animatable)
  in `Game::image_modifiers` in exact application order, caps stacked instances per form at
  `Game::MAX_IMOD_STACK = 64` (evicting the oldest matching entry at capacity so repeated
  re-applies cannot grow unbounded), and draws **one instance per `IMAD` form** (the most
  recently applied entry) in `nvforever::imagespace::screen_effect` so unmodded visuals stay
  unchanged [design].
- **A modifier's time passes with the game outside menus and loading screens,
  conversations included** [hypothesis: `Game::tick` ages modifiers where it advances a
  conversation's lines]. Not known: whether an animated modifier goes on behind a
  message box or the pause menu in the game.
- A record with animatable set and duration 0 ends in the frame after it is applied
  [design].

Trace events (`fnv.script.trace/1`): `image_modifier_applied {modifier}` and
`image_modifier_removed {modifier, ended}` (`ended` when an animated one ran out).

## 4. Several at once

- **Tint** [documented: GECK wiki "ImageSpace Modifiers"]: the colour is the average of
  the active modifiers' tint colours weighted by their alpha; the alpha is the highest of
  them.
- **Blur radius and double vision** [documented: the same page]: the highest value among
  the active modifiers.
- **Multiply/add parameters** [documented: the same page]: the parameter × multiply +
  add; "to simply set a parameter to a target value, multiply by zero and add the target
  value". With several modifiers the multipliers are multiplied and the adds summed
  [hypothesis].
- Fade colour: as the tint. Radial blur and depth of field: those of the modifier with
  the greatest strength. Motion blur: the highest [hypothesis, all three].
- The image space's own tint (with the weather's adapter, world.md §6) counts as one more
  tint in the average [hypothesis].

## 5. On the picture

Parameter meanings [documented: GECK wiki "ImageSpace Modifiers"]: blur radius 0 none,
7 the most; double vision strength "what percentage of the screen to apply the effect
to", 0 to 1, "animates automatically", 0.15 "jarring but still readable", 0.35
"gibberish"; radial blur strength 2.5 moderate, 5 strong; ramp up 0 never blurs, 1 is at
full strength at once; start is the part of the screen around the centre left sharp (0
all blurred, 1 none); ramp down and down start are the same from the outside in, leaving
a sharp ring at the edge; depth of field distance and range in game units.

The renderer's formulas are ours [hypothesis, every constant; to be set against
screenshots of the game, §7]:

1. **Blur**: a Gaussian blur of the tonemapped picture, in two passes (across, down).
   A radius of 1 reaches 1/270 of the screen height to each side (two standard
   deviations), so radius 5 is about 2% of the height. Reasoning: the wiki gives the HDR
   blur radius as whole numbers from 1 to 7, which reads like a reach in pixels of a
   reduced picture.
2. **Double vision**: two copies of the picture side by side, each at half strength,
   strength × screen width apart at most, swinging between 0.2 and 1 of that with a
   period of about 4 s.
3. **Radial blur**: 12 samples from each point towards the centre, reaching strength ×
   2% of the point's distance from the centre, scaled by the ramps: 0 inside `start`,
   rising by ramp up ÷ (1 − ramp up) per unit of distance (1 = the far corner of a
   centred screen), falling the same way from `down start` by ramp down.
4. **Grade**: the image space's cinematic values with the modifiers' multiply/add, then
   the grade's formula (world.md §6).
5. **Fade colour**: the graded picture mixed towards the colour by its alpha, on display
   values, last. That the fade covers the grade is [observed]: in the new-game save
   WhiteISFX and MQ08AllBlackedOutISFX (brightness × 0) are both active and the screen
   is white (layer5-captures.md §10.2).

Menus, message boxes, subtitles and the HUD are drawn over the result and are not
affected [observed for a message box: the same capture shows the pack box over the white
screen].

The world behind a menu drawn from its tiles is blurred with this same pass (behind
`[UI] TileMenus=`, tutorial-menu.md §5): radius 3, on and off at once, as behind the wait
menu [design: the original's shots show that the world is blurred, not its radius or its
timing]. Like every other request it is the highest value, not a sum: the modifiers, the
wait menu, the Pip-Boy, death and the weapon wheel keep theirs when the menu goes, and what
is nearer than the Pip-Boy's sharp depth stays sharp. It is applied after the frame's
`update_grade` (`imagespace::tile_menu_blur`), so the first frame drawn from tiles has it and
the frame the menu closes has not. Log, on changes only, of this contribution alone:
"tile menu blur: TutorialMenu radius 3", "tile menu blur: off", and with the off "tile menu
blur: the screen's own blur N stays" when something else still blurs the screen.

Depth of field is drawn as §8 says, motion blur as §9 says, the HDR and bloom tracks as
§11 says. Not drawn: radial blur "use target".

## 6. The opening

VCG00 stage 0 applies WhiteISFX and MQ08AllBlackedOutISFX; stage 90 applies the latter
again and stage 100 removes it; VCG01 stage 0 removes WhiteISFX and applies
VCG01FadeInFromBlackISFX; VCG01 stage 5 applies VCG01PlayerStrainISFX [observed in the
masters]. So the player sees white, then over 5 s the room comes out of the white,
blurred and doubled, sharp after 7 s; with Doc's first line over, a dark blink and a
bright, blurred flash that settle over 10 s.

`nvforever --play --new-game` (2026-10-10, 1280×720, boxes answered after 1 s): the log
line "image space modifiers on the screen:" gives WhiteISFX, MQ08AllBlackedOutISFX at
the first script frame; VCG01FadeInFromBlackISFX at VCG01 0 (3.05 s of game time);
VCG01PlayerStrainISFX joins at stage 5 (8.69 s); the fade-in ends at 10.12 s (7.06 s
after it began; the pack boxes held it meanwhile) and the strain at 18.67 s; then none.
Until the scripts have started the new game's screen is white [design: the script data
loads while the room is already drawn].

To look at one modifier: `nvforever ... --imod <Editor ID or 0xFormID>[@seconds]`
(repeatable). An animated one plays again and again, a second apart; `@seconds` holds it
at that time.

## 7. Open

- Captures to set the constants of §5: game screenshots at known moments of
  VCG01FadeInFromBlackISFX and of a record applied from the console (`imod`), e.g.
  blur radius alone and double vision alone, against `--imod <record>@<seconds>` at the
  same window size.
- Whether a second apply restarts or stacks; whether animated modifiers go on behind
  menus (§3).
- Radial blur "use target". Motion blur's constants (§9.4) against game screenshots:
  `imod VATScamNoDOF` from the console while turning. Depth of field's constants and modes (§8.5)
  against game screenshots: FlashBangISfx from the console (`imod`), VATS with a target
  in front of a far background.
- Explosions play theirs (explosion-effects.md §5, 2026-10-19). Other things that play
  modifiers are not built: weapons (their records name an IMAD), the get-hit and night-eye blocks of the image space.
  (`TriggerScreenSplatter`: screen-splatter.md; `SetCellImageSpace`: bink-films.md §4.)
- `ApplyImageSpaceModifier` given a form that is not an IMAD fails the call here; the
  game's behaviour is not known.

## 8. Depth of field

Code: `fnv_world::image_modifier::dof_weight` (the rule below), `engine/nvforever/src/imagespace.rs`
(the sources and the log line) with `imagespace_blur.wesl` (the picture).

### 8.1 The fields

- An IMAD's depth of field is three key lists, **strength** (`WNAM`), **distance**
  (`XNAM`) and **range** (`YNAM`), and two settings in DNAM, **Use Target** and the
  flags **Mode - Front**, **Mode - Back** and **No Sky** [documented: xEdit IMAD, §1].
- The GECK's words [documented: GECK wiki "ImageSpace Modifiers", owner export]:
  strength is "how strong of a blur to apply", 0 to 1 "preserves image quality", more
  gives "an awesome contrast-like effect"; distance is "the distance in units that
  contains a clean, non-blurred view", and "setting the distance higher than the range
  will create a blurred foreground with a clear background"; range is "the range in
  units where the depth blur begins".
- An image space (IMGS) carries no depth of field in Fallout: New Vegas: its DNAM is the
  HDR, bloom, get-hit, night-eye and cinematic blocks [documented: xEdit IMGS DNAM,
  schemas/fnv/sky.json]. So the only sources are modifiers and the engine's own uses.

### 8.2 The records and who plays them [observed: `imad_dump`, a scan of FalloutNV.esm for references]

| Record | Strength, distance, range | Use target, flags | Played by |
|---|---|---|---|
| FlashBangISfx 0x0002E5D4 | 2 → 1 (at 0.48) → 0 over 10 s, 0, 0 | no, 0 | the explosions FlashBangExplosion and GrenadeStunExplosion (EXPL MNAM) |
| AntNectarISFX 0x00031032 | 0.25, 64, 1024 | no, 0 | the script AntNectarSCRIPTfx |
| vault106HallucinationISFX 0x000892C2 | 0.24, 64, 1024 | no, 0 | the package vault106FatherWalk3A's script (Fallout 3's) |
| PipBackgroundFX 0x00096389 | 0.7, 0, 0 | no, 0 | nothing in the masters |
| PauseBackgroundFX 0x0004EEE8 | 1, 0, 0 (with blur radius 3) | no, 0 | nothing in the masters |
| VATScamDOFCloseBlur 0x00053BCD | 0.75, 0, 512 | yes, Front | nothing in the masters |
| VATScamDOF50, TurboDOF50 | 0 | yes, 0 | camera shots (CAMS MNAM: FirstPersonCamAuto, FirstPersonMissileHit) |

Game settings [observed: FalloutNV.esm GMST]: `fVATSDOFRange` 500, `fVATSDOFStrengthCap`
0.6, `fIronSightsDOFDistance` 275, `fIronSightsDOFRange` 650, `fIronSightsDOFStrengthCap` 0.
No setting names a depth of field for dialogue or scopes [observed: no other GMST with DOF
or Focus in its name].

### 8.3 How much a point is blurred [hypothesis: our reading of the wiki's words]

The blur at a point is `strength × t`, where `t` (0 sharp … 1 fully blurred) comes from
the point's distance `z` from the eye along the view, in game units:

- **Without Use Target**: `t` runs from 0 at `distance` to 1 at `range`, in a straight
  line, held outside. With range above distance the near view is clean and the far view
  blurs; with distance above range it is the other way round, the wiki's "blurred
  foreground with a clear background". Distance equal to range: everything beyond it is
  blurred (PipBackgroundFX and PauseBackgroundFX, 0 and 0, blur the whole picture).
  The mode flags are not used without a target.
- **With Use Target**: the same ramp is measured from the target's distance `f`: the
  point is `|z − f|` from it; Mode - Front blurs only what is nearer than the target,
  Mode - Back only what is beyond it, neither (or both) both sides. The target is the
  engine's (VATS, §8.4); without one it is the point under the screen's centre.
- **No Sky**: the sky (where nothing was drawn) is left sharp; without it the sky counts
  as infinitely far.
- **Several at once**: the strongest, as before (§4) [hypothesis]; the engine's own uses
  (§8.4) count as one more.

### 8.4 The engine's own uses [hypothesis: from the settings' names; the records the engine plays itself are not known]

- **VATS**: while marking and firing, the view's depth of field is focused on the mark
  (the last marked while marking, the one being shot while firing): strength
  `fVATSDOFStrengthCap` (0.6), distance 0, range `fVATSDOFRange` (500), what is beyond
  the target only (Mode - Back), faded in and out with VATS mode's look
  (vats-mode.md §5). Without a mark nothing is blurred. Back only keeps the player's own
  weapon and what is between the player and the target clean [design].
- **Aiming down the sights**: strength `fIronSightsDOFStrengthCap` with the zoom's blend,
  distance `fIronSightsDOFDistance`, range `fIronSightsDOFRange`. The shipped cap is 0,
  so there is none in the shipped game [observed]; a plugin that raises it gets it.
- **In a conversation**: focused on the speaker's face, at its straight distance from the
  camera (so looking around does not move the focus), what is beyond it only (Mode -
  Back), faded with the close-up's zoom and following the `dialogue-zoom` setting
  (docs/design/dialogue-play.md §8.8). Strength 1, distance 32 (the speaker's own body
  stays clean), range 512 [design: no setting names it, §8.2; to tune against the
  original's shots, where the far background is strongly blurred and a wall close behind
  the speaker a little: observed, dialogue-play.md §8.5]. Whether the original's is a
  depth of field at all is a [hypothesis].
- PipBackgroundFX and PauseBackgroundFX are not used: the Pip-Boy keeps its blur from
  `fBlurRadiusPipboy` (§5) and the pause menu covers the screen [design].
- Camera shots' modifiers (VATS kill cameras, CAMS MNAM) are not played here.

### 8.5 On the picture [hypothesis, every constant]

- The blurred picture is the Gaussian blur of §5.1 at a fixed **radius 3**
  (`DOF_BLUR_RADIUS`); each point is mixed from its sharp value towards that blur by its
  weight `strength × t`. The weight is not clamped at 1: past 1 the mix goes beyond the
  blur, away from it, which sharpens edges and raises contrast, the wiki's
  "contrast-like effect" (FlashBangISfx starts at 2).
- The mix is made in each of the two passes (across, then down), so a weight of 1 is the
  full blur and weights between give a softer one.
- The weight is the point's own (its depth), so a sharp object in front of a blurred
  background keeps its edges.
- Applied with the modifiers' blur (§5.1) where both are on; what is nearer than the
  Pip-Boy's sharp depth stays sharp.

Log line (smoke `imagespace-dof`, `vats-mode-dof`, `dialogue-zoom`): "depth of field: strength
S, distance D, range R[, focus F][, front][, back][, no sky] (source)" when it changes (source:
the modifier's Editor ID, `VATS`, `iron sights` or `dialogue`), and "depth of field: off".

## 9. Motion blur

Code: `engine/nvforever/src/imagespace.rs` (`motion_blur`, `motion_frames`,
`update_motion`, the log line) with `imagespace_blur.wesl` (`motion_blur`, the picture).

### 9.1 The field

- An IMAD's motion blur is one key list, **strength** (`NAM4`), counted at DNAM byte 240
  [documented: xEdit IMAD, §1]; `GetImageSpaceModTrait` calls it trait 63 "Full-Screen
  Motion Blur: Strength" [documented: GECK wiki "GetImageSpaceModTrait", owner export].
- The GECK's words [documented: GECK wiki "ImageSpace Modifiers", "FullScreen Motion
  Blur"]: strength is "how much to blur the screen while moving", 0.0 no blur, 10.0
  maximum. Nothing else says how it is drawn. The console's `ToggleMotionBlur` and
  `SetMotionBlur` pages are empty [documented: the same export]. The reference flag
  "Motion Blur" (512) is "Unused" [documented: GECK wiki "Reference"].
- An image space (IMGS) has no motion blur field [documented: xEdit IMGS DNAM].

### 9.2 The records and who plays them [observed: a scan of the masters' IMAD NAM4 lists and of every subrecord for their form IDs, 2026-10-20]

Keys are (time: value); none of these is animatable except MS10DrinkSapISFX, and one that
is not stays at its first keys (§3), so the value at time 0 is what shows.

| Record | Motion blur | Played by |
|---|---|---|
| VATScamNoDOF 0x00028FA1 | 0: 0.15, 1: 0 | most VATS camera shots (CAMS MNAM: CamDefaultShoot, CamDefaultHit, FirstPersonCam, PlayerDeathDefault, ...) |
| VATScamNoDOFDeSat 0x0007DC93 | 0: 0.2, 1: 0 | camera shots (DocuCam..., SlowMoSideFaceOff..., SniperCam...Desat) |
| VATScamNoDOFDeSatGreen 0x0002A139 | 0: 0.2, 1: 0 | camera shots (NinjaCam..., MeleeActionCam...b, SkyCam01) |
| VATScamNoDOFBlur 0x000A9E9C | 0: 0.2, 1: 0 | camera shots (the ...01b DocuCams) |
| NightScopeISFX 0x0008CD8C | 0: 0.2, 1: 0 | nothing in the masters |
| MS10DrinkSapISFX 0x000AC039 | animated 5 s: 0: 0, 0.06: 0.1, 1: 0 | nothing (Fallout 3's Oasis quest) |
| MS10DrinkSap02ISFX 0x000CA8B2 | 0: 0.5, 0.06: 0.1, 1: 0 | nothing |

In the add-ons: NVDLC01CloudPoisonISFX (Dead Money, 0.05; not referenced in its plugin),
NVDLC02SacredDaturaPoisonIS (Honest Hearts, 0.75; the quest NVDLC02MS01's script) and
NVDLC02WeapBindISFX (Honest Hearts, 0.2; the script NVDLC02WeapBindRitISFXscript).

Game settings [observed: FalloutNV.esm GMST]: `fIronSightsMotionBlur` 0 and
`fIronSightsGunMotionBlur` 0. No other setting names a motion blur.

So in the shipped game a motion blur shows in VATS's kill cameras (not built here, §8.4)
and when a script or the console applies one of these records.

### 9.3 Sources

- **Modifiers**: the largest strength among those on the screen (§4) [hypothesis].
- **Down the sights**: `fIronSightsMotionBlur` × the zoom's blend, when larger than the
  modifiers'; 0 in the shipped game, so none [hypothesis: from the setting's name, as for
  the depth of field, §8.4]. `fIronSightsGunMotionBlur` (also 0) would be the weapon's own
  smear; the player's own body is never smeared here (§9.4), so it is not used.
- The VATS camera shots' records are not played (§8.4); NightScopeISFX is played by
  nothing in the data, so a night-vision scope would be the engine's own use: not built.

### 9.4 On the picture [hypothesis, every constant]

- "While moving": each point is smeared along its own motion on the screen, found from
  its depth: the point is put back through the last frame's camera, which gives where it
  was on the screen a frame ago. So turning smears the whole picture, walking smears the
  near ground more than the far hills, and a still view is sharp. The motion of other
  actors is not counted (a still camera does not smear a running dog) [design: the
  renderer has no motion vectors].
- **Exposure**: the smear is the screen motion over the last `strength × 1/30 s`
  (`MOTION_EXPOSURE`), at most 8 % of the screen height long (`MOTION_MAX_REACH`); 10 then
  covers a third of a second, at the limit on any brisk turn [hypothesis]. With VATScamNoDOF's
  0.15 a turn of 125° a second smears plainly (smoke `imagespace-motion-blur`,
  motion_turning.png, 2026-10-20) [observed in ours].
- The smear is 12 samples (`MOTION_TAPS`) back along the motion, averaged; it is drawn
  before the blur and depth of field passes.
- The player's own body (what is nearer than 40 units, `MOTION_SHARP`) moves with the view:
  it is neither smeared nor smeared into what is behind it.
- A camera that jumps more than 300 units or turns more than 45° in one frame was cut, not
  moved: no smear that frame [design]. A paused game has no motion, so no smear.

Log line (smoke `imagespace-motion-blur`): "motion blur: strength S (source)" when it
changes, S in steps of 0.05 ("under 0.05" for less; source the modifier's Editor ID or
`iron sights`), and "motion blur: off".

## 10. Sounds

### 10.1 The fields [documented: xEdit wbDefinitionsFNV.pas IMAD]

`RDSD` "Sound - Intro" and `RDSI` "Sound - Outro", each a SOUN form ID (schema
`sky.json`).

### 10.2 The records [observed: a scan of the ten masters, 2026-10-24]

20 records set RDSD, one (NVDLC03BattleBrewDrinkISFX) RDSI too:

- Scripted ones: BarkScorpionPoisonISFX, NVDLC01CloudPoisonISFX and the Sacred Datura
  pair (FXPoisonStinger); CGCameraFlashISFX (FXExplosionThermite); NVCaveEnteringISFX and
  NVCaveExitISFX (AMBRCaveWind01, the cave scripts' `imod` / `rimod`); the stealth suit's
  NVDLC03StealthSuitONISFX (UIRadioSignalFound) and OFFISFX (UIRadioSignalLost);
  NVDLC03BattleBrewDrinkISFX (NVDLC03BattleBrewHeartbeat, outro NVDLC03RedMistWearOff);
  NVDLC03RedMistISFX, the X-8, rez-in and radar-fence ones of Old World Blues.
- Weather adapters: NVColoradoRiverIS, NVJacobstownIS, NVBlackMountainIS and the three
  NVWasteland…ISUPDATE (thunder and birds).

### 10.3 Played [hypothesis]

When a modifier comes on the screen (a script's apply, an explosion's, `--imod`, a held
one) its intro sound plays at the listener; when it leaves (removed, or an animated one
at its end) its outro sound. The names say so, and the data fits: the stealth suit's ON
and OFF records each have a "signal found" or "signal lost" intro, and Battle Brew's
starts a heartbeat and ends with the chems' wear-off sound. Not played: the weather
adapters' (when the game plays a weather adapter's sounds is not known). Log line
(smoke `imagespace-sounds`): `image space modifier sound: <Editor ID> intro|outro
0x<SOUN>`.

## 11. HDR and bloom tracks

### 11.1 The fields [documented: xEdit IMAD]

`\x00IAD`..`\x10IAD` multiply and `\x40IAD`..`\x50IAD` add tracks, the 14 HDR values, the
skin dimmer and bloom's three, in the order `fnv_world::imagespace::Hdr` keeps (world.md
§6); every key read and played as the cinematic tracks are (§3). Several modifiers:
multipliers multiplied, adds summed (as §4's pairs) [hypothesis].

### 11.2 The records [observed: the masters, 2026-10-24]

Scripted modifiers with HDR keys: the poison stings (BarkScorpionPoisonISFX,
NVDLC01CloudPoisonISFX, the Sacred Datura pair: Target LUM and Bright Scale ×2 and ×3 a
quarter of the way through, back to ×1); CGCameraFlashISFX (Bright Scale ×5 and Bright
Clamp ×0.1 for half its 8 s); NVDLC03PlayerRezInISFX (Bright Scale up to ×10);
NVDLC03RadarConcussionISFX (Emissive Mult and Bright Scale ×2, Bright Clamp ×0.25);
NVCaveEnteringISFX (Eye Adapt Speed ×1 to ×0.5 over its keys); NVDLC03X8ExperimentFX
(sets every value outright). The flash-bang (FlashBangISfx) has no HDR keys: its
whiteout is its fade and cinematic tracks (§5).

### 11.3 On the picture [hypothesis, every rule]

The values with the modifiers on them, over the image space's own (with the weather's
adapters), give a factor per value (`ScreenModifiers::hdr_ratio`):

- **Target LUM**: the picture's exposure moves by log2 of the factor (×2: one EV
  brighter), at most 4 EV either way, through the eye adaptation, so it follows at the
  adaptation's speed (`eye_adapt::modifier_ev`). The image space's own Target LUM keeps
  its fitted role in the eye adaptation (world.md §6).
- **Bright Scale and Bright Clamp**: the light glow's strength times the Bright Scale
  factor, up to a full-strength glow (`post::modified_glow`); the bright pass (what is
  brighter than its threshold, world.md §6) from the image space's Bright Scale times
  that factor over the Bright Clamp factor ("Bright Clamp: the clamp for which lum
  levels bloom" [documented: GECK wiki ImageSpace]: a lower clamp lets more glow)
  (`post::modified_bright_pass`). Until 2026-11-02 the clamp factor raised the light
  glow too; the light glow scatters every pixel, so the birth scene of Tale of Two
  Wastelands (CG00BirthBaseISFX: Bright Scale ×2, Bright Clamp ×0.2) came out blurred
  all over, where the original keeps the people and the frame sharp under blooming
  lamps [observed: owner's TTW run and a Fallout 3 frame, 2026-11-02].
- **Eye Adapt Speed**: the adaptation speed, from the value with the modifiers on it
  (`eye_adapt::adapt_speed`): 3 EV/s at the common 0.9, in proportion to it ("higher
  numbers give faster chasing"), at once at 0 [documented: GECK wiki ImageSpace]. The
  wiki's other line, "1.0 causes the adapted lum to never change", does not fit the
  first; the masters store values up to 8. This applies to the image spaces' own values
  too (interiors store 0.3 to 8).
- **Sunlight Dimmer** and **Emissive Mult**: the sun's light and the emissive light
  times their factors, on top of the image space's own use of them (world.md §6).
- Not used: Upper LUM Clamp, LUM Ramp Min and Max ("Not used" [documented: GECK wiki
  ImageSpace]); Blur Radius and Blur Passes of the HDR pass and bloom's (the light glow
  has its own blur); the skin dimmer; LUM Ramp No Tex (the sky), Grass Dimmer and Tree
  Dimmer ("scale the grass brightness" and so on [documented]): the weather adapters
  change these by up to three times against the base image space (NVWastelandNoonISUPDATE
  sets the grass dimmer to 0.5, the base has 1.5), and the grass, trees and sky were
  matched to the owner's frames without them, so applying them waits for frames that
  show what they do.

Log lines: `eye adaptation: … modifiers +1.00 EV` when a Target LUM change starts and
ends; `eye adaptation: speed S EV/s (Eye Adapt Speed E)` when the speed changes.


## Live nonanimated trait edits

[documented: GECK wiki SetImageSpaceModTrait,
https://geckwiki.com/index.php/SetImageSpaceModTrait] Traits 1–2 edit the radial blur
centre X/Y; 3–36 edit the 17 HDR/bloom multiply/add pairs (`\x00IAD`..`\x10IAD` /
`@IAD`..`PIAD`); 37–44 edit the cinematic multiply/add pairs; 45–46 edit blur and double
vision; 47–50 edit tint RGBA; 51–54 edit fade RGBA (`NAM3`); 55–59 edit radial blur
strength/ramps (`RNAM`, `SNAM`, `UNAM`, `NAM1`, `NAM2`); 60–62 edit depth of field
strength/distance/range (`WNAM`, `XNAM`, `YNAM`); 63 edits full-screen motion blur
strength (`NAM4`).

[design, mapped; engine design choice] For a nonanimatable modifier actually selected
for the screen or lens, evaluate its ordinary record and then apply the current session's
assigned values across traits 1–63 (with a fast exit when no trait overrides exist in the
session). Follow the existing IMAD getter and decoder: 3–36 map directly to the 17
`Hdr` multiply/add pairs (`ScreenModifiers::hdr`, feeding eye adaptation, light glow,
sunlight dimmer, and emissive fog scale), 37–38 are saturation,
39–40 are contrast average luminance (`0x12`/`0x52` IAD), 41–42 are contrast
(`0x13`/`0x53` IAD), and 43–44 are brightness [hypothesis: this trait-to-field order
is the existing getter's, not a capture of the original setter]. Tint RGBA (47–50) are
all 0–255: divide each assigned value by 255; the getter reads TNAM alpha ×255 like its
RGB [observed: original FNV 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-20 —
`GetImageSpaceModTrait` on unchanged non-animated `FalloutNV.esm` IMADs returned
traits 47–50 as `[106.0, 141.0, 149.0, 120.0]` for `WastelandMegatonNightISFX`
(`0x00054E13`, authored TNAM `106/255, 141/255, 149/255, 120/255`),
`[101.0, 154.0, 150.0, 160.0]` for `WastelandNightISFX` (`0x00091890`, authored TNAM
`101/255, 154/255, 150/255, 160/255`), and `[102.0, 149.0, 153.0, 100.0]` for
`WastelandNorthNightISFX` (`0x0003920C`, authored TNAM `102/255, 149/255, 153/255, 100/255`);
`SetImageSpaceModTrait` round-trips on the same records read back `0.35000002` for trait 50 = `0.35`,
`128.0` for trait 50 = `128.0`, and `200.000015` for trait 47 = `200.0`].
Separately, in an installed mod (Radiation Visuals), the controller writes 47–49 as 0–255
and 50 as its effect strength times its tint strength (default 0.3) times 255, and a
contributor playtest in NVForever with that mod (2026-11-18) visually confirmed on three
builds in irradiated water that before this PR the cyan effect persisted, with trait 50
read as 0–1 alpha the whole screen turned cyan, and with 47–50 all read as 0–255 the tint
looked correct.
Fade RGB (51–53) divide by 255 like the getter; NAM3 alpha (54) keeps its normalized
getter units (unobserved).
Leave every unassigned component, other effect, sound, duration and track alone.
[engine design choice] Read only bounded fields of active modifiers; do not clone the
record cache, mutate the load order or scan unrelated forms each frame. Nonfinite values
(and finite `f64` values that overflow `f32` to infinity) which cannot be represented as
finite renderer floats are left off the visual override; the existing core setter/getter
state and admission policy are unchanged.

[hypothesis] This maps the installed radiation controller's edit/apply/update/
remove cycle (and other nonanimated trait edits) onto the existing grade and HDR
consumers. Native first-key versus whole-track editing for animatable records remains
open; this bridge leaves animated evaluation unchanged.
Existing session-only trait lifetime is preserved, and removal must restore the
current base/other effects rather than replay a cached grade. Authored fixtures
exercise compiled setter calls and getter-to-setter round trips through
actual ImageSpaceGrade and ScreenModifiers::hdr output, warm-cache updates, nonfinite
and f32-overflow guards, removal, re-entry, lens isolation, missing runtime and fresh
restore.
No native, GPU or full installed-mod acceptance follows from CPU controls.
