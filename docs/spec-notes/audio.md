# Spec notes: music, sound markers and background loops

Clean-room description in plain language. Tags: **[documented]** cites a public source,
**[observed]** was measured on the owner's install (FNV 1.4.0.525), **[hypothesis]**
needs verification, **[design]** is our choice where the game's rule is not known.
Counts come from `fnvtool world audio` (fnv.world.audio/1, §8) over FalloutNV.esm alone.

Code: `fnv_world::audio` (records, markers, the choice of music at a place),
`nvf-play` `music.rs` (playback of music) and `ambient.rs` (placed sounds, background
loops). Related: combat-rules.md §16 (SOUN records and how a sound effect is played),
voice.md (dialogue voice).

## 1. The records

Layouts [documented: xEdit `wbDefinitionsFNV.pas`, branch dev-4.1.6; `schemas/fnv/audio.json`];
all four record types decode over FalloutNV.esm with no bytes left over [observed:
`fnvtool esm coverage`].

- **MUSC, music type** (20): `FNAM` a file or a folder, `ANAM` a float xEdit names
  "dB (positive = Loop)". 13 name a file, 6 a folder, `1NoMusic` nothing [observed].
- **MSET, media set** (140: 81 location, 32 battle, 23 dungeon, 4 incidental): `NAM1`
  the type; `NAM2`–`NAM7` six file names; `NAM8`, `NAM9`, `NAM0`, `ANAM`, `BNAM`, `CNAM`
  a decibel value for each; `JNAM`–`ONAM` a "boundary %" for each; `PNAM` one enable bit
  per file; `DNAM`, `ENAM`, `FNAM`, `GNAM` times; `HNAM`, `INAM` an intro and an outro
  sound (SOUN). For a location set the six files are day outer, middle, inner and night
  outer, middle, inner, and the times are "minimum time on", "looping/random crossfade
  overlap" and "layer crossfade time"; a battle set uses the first file as its loop; a
  dungeon set the first three as battle, explore and suspense [documented: the field
  names].
- **ALOC, media location controller** (89): `NAM5` and `NAM6` the day's and the night's
  start (the stored number over 256 is the fraction of a day [documented: xEdit
  `wbAlocTime`]; 64 and 255, i.e. 06:00 and 23:54, on the ones looked at [observed]);
  `NAM4` location delay, `NAM7` retrigger delay; lists of media sets: `HNAM` neutral,
  `ZNAM` ally, `XNAM` friend, `YNAM` enemy, `LNAM` location, `GNAM` battle; `RNAM` a
  "conditional faction". `NAM1` (flags), `NAM2`, `NAM3` and `FNAM` are not understood.
  83 have location sets (29 more than one), 8 a faction (4 of them location sets too),
  2 no set at all (one is `musCtrlAAAntiMusic`) [observed].
- **ASPC, acoustic space** (113): five `SNAM` in a row, by position the looping sound of
  dawn (or the default loop), afternoon, dusk and night, and a "walla" sound; `WNAM`
  walla trigger count; `RDAT` "use sound from region (interiors only)"; `ANAM` a reverb
  environment by name; `INAM` is interior. No shipped master names a walla sound: the
  fifth `SNAM` is null and `WNAM` 0 on all 145 acoustic spaces of FalloutNV.esm (113),
  Dead Money (18), Honest Hearts (2), Old World Blues (3) and Lonesome Road (9); the other
  add-ons have none [observed 2026-11-06, `fnvtool esm decode` of every ASPC, issue #990].
  The casinos' crowd sounds are sound markers instead (e.g. `AMBTopsCasinoAWallaOS`).
- **On cells and worldspaces**: CELL `XCMO` a music type (297 cells), CELL `XCAS` an
  acoustic space (17,485 cells), WRLD `ZNAM` a music type (3 worldspaces).
- **On references**: the audio marker data `MMRK` (empty), `FULL` a location name,
  `CNAM` the controller, `BNAM` bit 0 "use controller values", `MNAM` and `NNAM` "layer
  2 / layer 3 trigger %"; `XRDS` the radius.

## 2. The files

- **Music** is 199 loose MP3 files under `Data\Music` (`LOC`, `BTTL`, `DNGN`, `OLD`,
  `SCR`, `Special`, `Fallout1and2`) [observed]. Record paths are relative to `music\`.
- The three files of a location set's day (and of its night) have the same size to the
  byte in the folders looked at (`LOC\CaesarsLegion`, `LOC\CorporateRuins`) and are
  named `…_1Low`, `…_2Mid`, `…_3High` [observed]: three stems of one piece.
- The folders the default music types name (`explore\`, `public\`, `dungeon\`, `base\`,
  `battle\`) are not in the install [observed].

## 3. Audio markers

- 572 references carry audio marker data, 363 of them in interiors. Every one has a
  controller and a radius (1,000 to 18,000 units); none is initially disabled [observed].
- **Every marker stores 0.33 and 0.67 as its layer 2 and layer 3 triggers**, whether it
  uses its controller's values (426) or not (146) [observed], so the markers' own
  values cannot be told from a default.
- Goodsprings: one marker (00152433) at −70849, 3430, 9501 with radius 14,000 and the
  controller `musCtrlGoodsprings`, whose neutral, ally and friend lists all hold
  `musSetLOCDesExplBothABC` and whose enemy list holds a dungeon set [observed].
- **Cell music types do not silence markers** [observed, indirectly]: 264 of the 372
  cells that hold audio markers also name a music type, 159 cells name `1NoMusic`, and
  the other types' folders are not shipped. Reading: the music type is the older system
  and the markers are what New Vegas plays [hypothesis].

## 4. What plays where (as built)

1. **The marker** [design]: of the enabled markers whose radius reaches the player, the
   one the player is deepest inside (smallest distance over radius); the lower FormID on
   a tie. How the game ranks overlapping markers is not known.
2. **The set** [design]: a controller with a conditional faction plays the list for the
   faction's standing towards the player (§13.2), any other its location sets. One of
   the list is picked at random on entering the marker and kept while inside (picked
   again when the standing changes). A controller without a set is silence. A dungeon
   set in the list plays as §13.2 says.
3. **Day or night** by the controller's two hours.
4. **Layers** [hypothesis]: the outer file sounds within its boundary percentage of the
   radius (100 on the sets looked at), the middle and inner ones are added inside
   theirs (60 and 20 for the Goodsprings set; 66.6 and 33.3 on others). A marker that
   does not use its controller's values gives the two inner boundaries itself, read as
   the fraction of the way in from the edge (0.33 and 0.67, i.e. 67 and 33 percent of
   the radius). That the layers add up, rather than replace one another, rests on the
   files being stems of equal length (§2).
5. **Levels and fades**: each file at 10^(dB ÷ 20). A layer fades in or out over the
   set's layer crossfade time (9 s on the Goodsprings set) [hypothesis: the unit is
   seconds]; a change of set, or between day and night, fades the old out and the new in
   over the same time [design].
6. **No marker**: the cell's music type, else the worldspace's, if its file exists; as
   one track at 10^(−|ANAM| ÷ 20), looping when `ANAM` is positive [hypothesis].

Battle, dungeon and incidental sets, the faction lists, the minimum time on, intro and
outro sounds and `PlayMusic`: §13.

## 5. Sound markers

- A reference whose base is a SOUN. 1,178 of them (706 in interiors) place 311 sounds
  [observed]. 607 have the Loop flag, 550 Play At Random, 21 neither; 151 Random
  Location; 54 are initially disabled and 91 have an enable parent.
- **Play At Random sounds name a folder** and have a Random Chance (`RNAM`) of 1 to 60
  percent, 4 on 220 of them [observed]. As built: once a second a marker in reach that
  is not already sounding is rolled against its chance [hypothesis: the chance is per
  second].
- **Hours**: SOUN `SNDD` start and stop time, the stored byte over 256 as the fraction
  of a day. 1,150 placed sounds store 0 and 0; 21 store about 20:00 or 21:00 to 06:00,
  one 08:15 to 17:20 [observed]. As built: a sound with different start and stop sounds
  only between them.
- **Reach and loudness** [design]: a marker sounds within its sound's maximum
  attenuation distance, at the loudness combat-rules.md §16 gives a sound at that
  distance.
- **Ogg under a `.wav` name** [observed]: the base sound archive holds 4,233 `.wav` and
  2,232 `.ogg` files; 2,184 of the Ogg files are effects (`sound\fx`), ambient ones
  above all. A record that names `x.wav` is served by `x.ogg` when only that exists
  (`AMBDesertBedDay` names `…\amb_desertdaybed_lp.wav`; the archive has the `.ogg`),
  and folders hold `.ogg` takes.

Not built: markers that neither loop nor play at random, Random Location, Start at
Random Position, Mute When Submerged. Panning: §14.5.

## 6. Background loops

The acoustic space of the cell the player is in names a looping sound per time of day
(the Goodsprings cell: `ExtDesertDefault`, with `AMBDesertBedDawn`, `…Day`, `…Dusk`,
`…Night`, all flagged 2D) [observed]. As built [design]: dawn 5–9, afternoon 9–17, dusk
17–21, night otherwise; a space with only its first sound plays it all day; one loop
gives way to the next over 2 seconds. The hours the game uses are not known.

The region an acoustic space points to plays its sounds (REGN `RDSD`) now and then:
default-objects.md §2 (claude/em9-records). Not built: walla. The space's reverb
environment: §14.6.

## 7. Playback (`nvforever --play`)

- Music and loops are decoded while they play (rodio with symphonia: MP3, Ogg Vorbis,
  WAV); the three files of a set are mixed in one source so they cannot drift apart.
- Sound effects are decoded whole, on a worker thread the first time a file is asked
  for; the request starts once the file is ready (`nvf-play/src/sfx.rs`, #1428)
  [engineering choice; what the original does with a sound not yet in memory is not
  known]. A game event's one-shot (a shot, a footstep, an impact) whose file comes in
  more than 150 ms after it joined the waiting list is dropped, not started, so it is
  not heard long after its moment; loops and scripts' sounds start however late
  (`MAX_ONE_SHOT_WAIT`) [engineering choice]. A folder's file is picked from an index
  of the folders listed on a worker at the start; a request before it is in waits for
  it. A Play At Random marker whose file is still being read skips that roll: the
  sound is not played when the file arrives, the marker plays on a later roll. The
  exit line `sfx files: …` counts the
  sounds that began in the frame they were asked for, those that waited, those decided
  silent and the rolls skipped; at its end, the one-shots dropped as too late. The
  frames are counted from the frame a sound was asked in to the frame it began,
  whatever it waited for: its file, a voice slot's sound fading out (below), or both; a
  one-shot waiting for a slot is counted when it starts there or is silenced, one
  dropped while it waits for the slot is not in this line (`sfx voices: …` counts it).
  At the line's end, apart, how soon the files were ready: `N requests found their file
  ready in the frame they were asked for, M later (at most F frames)`, counted as each
  file is ready, whatever became of the request.
- A second chance late in each frame (`late_start`, `PostUpdate`, before the audio
  output takes up new players) starts the requests whose file finished decoding since
  the frame's requests were played, as they would have started then, so a file decoded
  a few milliseconds late can still start in the frame it was asked for [engineering
  choice]. The exit line adds how many it started, how many of those in their own frame
  and, for those only, the longest time in milliseconds from joining the waiting list to
  the start: not the delay from the request until the sound is heard.
- Log: `music: in marker …`, `music: <set> day|night started, layers …`,
  `music: <set> layers …`, `music: <set> fades out`, `music: battle begins …`,
  `music: battle over …`, `music: intro|outro of …`, `music: a script plays …`,
  `music: no music here; <set> plays a phrase in N s`; at the end `music summary: …`
  (sets started, changes of layers, files not played, battles, battle loops, intro and
  outro sounds, script requests, incidental phrases) and `ambient summary: …`.
- Switches: `--no-music`, `--music-volume`; placed sounds and background loops go with
  `--no-sound`. The settings page has a Music switch, and its Sound tab the volumes by
  category (`nvf-play/src/audio_out.rs`: Master, Music, Footstep, Voice, Effects,
  Radio, as the original's audio menu [observed: the owner's photo, 2026-10-17]; Footstep
  is the `FST` records, Effects every other effect [hypothesis]).
- Smoke test (CLAUDE.md): standing in Goodsprings, expect the desert exploration set
  with three layers, two placed loops (the air conditioners) and one background loop.
- **Voice limit** (`nvf-play/src/sfx.rs` `MAX_VOICES` 64, `MAX_SAME` 8; interface sounds
  are not limited) [engineering choice: the original's limits are not known]. A one-shot
  beyond 8 of its own SOUN stops that SOUN's oldest voice and plays in its place; one
  beyond 64 in all is not played. Before #1098 the 9th of a SOUN was not played either:
  the 10mm SMG (9 shots a second) fired for 3 s gave 28 shots of `WPN10mmSMGFire2D` and
  35 sounds refused, gaps in the burst [observed: smoke `auto-fire-burst-sound` on main
  2026-10-31]; now 28 started, 20 of them over the oldest, 0 refused [observed]. Whether
  the original cuts a previous shot's sound is not known (observer check). The exit log
  names every SOUN that met a limit: `sfx: <EDID>: N started, S of them over its oldest
  voice, R refused at the voice limit`.
- **Voice slots** (#2217, since stage 2) [design]: the limits count what is heard. Every
  one-shot holds a slot until its sound has ended, also while it fades out after a stop
  (below), so 8 of a SOUN and 64 in all are never passed, not even for a fade. A
  one-shot beyond 8 of its SOUN has one slot's sound fade out (one fading already,
  else the oldest voice) and waits for it: it starts once that sound has ended, about
  5 ms and a frame or two after it was asked for, with the file and the pitch chosen
  when it was asked for (a Random Frequency Shift is rolled once) and the listener as
  it is then (its gain, pan and place worked out again). A
  slot has one waiting replacement at most, so with every slot of the SOUN promised
  one more is refused; nothing else (a placed sound, a late start) can take a promised
  slot. A game event's one-shot whose wait for its file and its slot comes to more
  than 150 ms is dropped; a script's sound is answered (its seconds) when it starts, a
  StopSound drops the ones asked for before it. One-shots started in the same frame
  have made no sound yet: one taken over among them goes at once and the new one starts
  in its place. `S of them over its oldest voice` counts the one-shots started in a
  slot taken over.
- **Measurements of the mix and of stopped sounds** (#2217: crackle with many sounds)
  [design]. Every sound reaches the device (or the drain without one) through one mix
  bus (`nvf-play/src/audio_out.rs` `MixBus`), which counts, for the whole run, the
  loudest sample and the samples over full scale (`abs(sample) > 1`, each channel apart;
  exactly ±1 is not over), before the output's protection (a limiter, §18, since
  stage 4) and after it, and the frames the limiter turned down; the raw overloads stay
  measured, after the limiter none should be over. Every stop of a playing sound
  before its end goes through `audio_out::stop_sound`, which counts it by why: a one-shot
  taken over at the voice limit, a LoopSound stopped, a script's StopSound without a
  fade, an ambient loop, a radio or a voice line no longer wanted, a holotape stopped
  (by a script, or another note picked), a film skipped. Since stage 2 every
  such stop fades the sound out over 5 ms (`audio_out::DECLICK_FADE`, a straight fall of
  the amplitude, timed by the rate the player plays the sound at: its rate times the
  sink's speed, as rodio's speed stage reports it, whatever that speed, slow motion's
  and a script's time multiplier included) and the sound goes once the fade has
  played (a skipped film's sound fades on what it has queued, silence after); a film
  played to its end lets its sound play out; removing a playing sound at
  once, which cut its waveform, is counted as a hard stop and should not happen any
  more. One asked for but not attached to the output yet, or already over, is not
  counted. The fades of their own (music's layers, a background loop giving way, a
  script's fade) and natural ends are not stops. Whether the original limits its mix,
  cuts or fades voices it stops, or what its limits are, is not known. Exit lines:
  `audio mix: pre peak P, N samples over full scale; post peak Q, M samples over full
  scale; K frames limited` (peaks to nine decimals), `sfx hard stops: S steals, L loop
  stops, T script stops, A ambient stops, R radio stops, V voice stops, H holotape
  stops, F film stops`, `sfx declicks: S steals, … V voice stops; C completed, W
  pending; H holotape stops, F film stops` (the same reasons, for stops faded out; C
  the fades played out, W those of every reason not over at the end; the holotape and
  film reasons came later and close each line) and `sfx voices: at most V total, S of
  one SOUN; Q replacements waiting, R replacements started, C replacements cancelled,
  E replacements expired, M replacements silenced at start, P replacements still
  waiting; I one-shots started at once over one not heard yet` (the most slots taken,
  fading ones counted; Q the one-shots that waited for a slot, and what became of
  them, Q = R + C + E + M + P: R started in their slot, C dropped by a StopSound, E
  dropped past the 150 ms bound, M decided silent when their turn came, P waiting at
  the end; I the one-shots that took over one of their SOUN started in the same frame,
  with no wait; `S of them over its oldest voice` counts both kinds).

## 8. Tools

`fnvtool world audio <Data> <plugins> --schema schemas/fnv` (fnv.world.audio/1): the
counts above. With `--cell C`: the cell's music type and acoustic space, and every
audio marker, controller and sound marker in earshot (the whole worldspace for an
exterior cell). With `--at x,y,z [--hour H]`: the marker, set, layers and files chosen
there, the background loop and the sound markers in reach.

## 9. Open

- How the game ranks overlapping markers, and picks among several sets.
- Whether layers add up or replace one another, and what "trigger %" on a marker means.
- What a controller's faction lists need (reputation, disguise?) and when battle and
  incidental sets play.
- The per-second reading of Random Chance, and the hours of the four background loops.

## 10. Sounds animations and doors name

Code: `fnv_actor::anim_sounds` (reading the keys, footstep and armour sounds),
nvforever `anim_sounds.rs` (hearing the clips that play, the ground underfoot),
`nvf_play::doors` (door sequences' keys). Survey tools: the examples
`fnv-nif kf_sound_keys` (text keys of every sequence in the archives named) and
`fnv-world footstep_survey` (LTEX materials, ARMO animation sounds, `FST` records,
which key names are SOUN editor IDs). Counts are over `Fallout - Meshes.bsa` (5,171
files with sequences, 6,229 sequences) and FalloutNV.esm alone; the add-on archives
were not read.

### 10.1 What the data says

- **Text keys naming sounds** [observed]: 3,530 keys read `Sound: <name>` (also
  `sound:`), sometimes with a node after the name (`Sound: OBJVertibirdBladesALP
  SoundNodeProps`; 586 of them on doors, doors.md). 842 distinct names: 663 are editor
  IDs of SOUN records in FalloutNV.esm; the 179 others are not records there (83 with
  `DLC` in the name, the rest Fallout 3 content such as `QSTBirthEKGCritical`,
  `NPCTrogFootLeft`) [observed; hypothesis: animations left over from Fallout 3].
- **First-person and third-person weapon clips** name their sounds: each reload clip
  its weapon family's parts, e.g. `1hpreloada.kf` `WPNPistol10mmReloadOut` (0.20 s),
  `WPNPistol10mmReloadN` (0.43 s), `WPNPistol10mmReloadChamber` (0.67 s);
  `2hrreloada.kf` `WPNRifleHuntingReloadPt1`–`Pt4`; `2hareloadb.kf`
  `WPNRiflePlasmaReloadPt1`–`Pt4`; jams, grenade throws (`WPNGrenadeThrowA`/`B`), melee
  swings (`FXSwingKnife`, `FXSwing2hm`, `WPNArmSwing`) and swimming (`NPCHumanSwim`,
  `NPCHumanSwimSurface`) likewise [observed]. Equip clips carry `Enum: Equip` and no
  sound (the weapon's own draw sound plays, combat-rules.md §16) [observed].
- **Footfalls** [observed]: `Enum: Left` (1,038 keys) and `Enum: Right` (966) on the
  movement clips of people and creatures, first and third person (`_male\locomotion\
  male\mtforward.kf`: Left 0.37 s, Right 1.03 s; `_1stperson\locomotion\mtforward.kf`:
  0.33 s, 0.93 s), also on landing clips. Beside them, `Enum: ArmorWalk` (90),
  `ArmorRun` (106), `ArmorSneak` (45), `FoleyWalk` (84), `FoleyRun` (126) and
  `FoleySneak` (48): an `Armor…` key at each footfall, a `Foley…` key between them.
  Creature clips also carry `Enum: BackLeft`/`BackRight`, `Auxiliary1`–`8`, `Idle`,
  `Aware`, `Attack` (the CREA sound types, CSDT) and `m:L`/`m:R` (actors.md §8.3).
- **Footstep records** [observed]: SOUN editor IDs `FST<material><kind>`: the families
  ConcBroken, ConcSolid, Dirt, Grass, MetalHollow, MetalSheet, MetalSolid, Sand, Water
  and Wood have `WalkL`/`WalkR`, `RunL`/`RunR`, mostly `SneakL`/`SneakR`, `Land` and
  `Jump`; Wood, MetalSheet, MetalSolid and ConcSolid also `StairsL`/`StairsR`; Snow has
  `Walk`, `Sneak` and `Land`; there are `FSTArmorL`/`R`, `FSTArmorPowerWalkL`/`R`,
  `FSTWaterSwim` and Fallout 3's baby set. No record links a material to them: the
  link is by name [hypothesis].
- **Ground** [documented: xEdit LTEX HNAM; observed: counts]: each landscape texture
  names a Havok material, numbered like nif.xml's `Fallout3HavokMaterial` (0 Stone, 2
  Dirt, 4 Grass, 18 Sand, 19 Broken Concrete …). Of 89 LTEX: Dirt 47, Grass 12, Broken
  Concrete 10, Stone 10 (`Asphalt02`, `Pavement`), Sand 7 (`GravelRoadWasteland01`),
  Heavy Stone 3. Placed objects' collision triangles carry the same codes, plus 32 for
  platforms and 64/96 for stairs (physics.md, `fnv_combat::fx::impact_material`).
- **Armour rustle** [documented: xEdit ARMO SNAM "Animation Sound" (sound, chance,
  type 17–22: Walk, Sneak, Run, Sneak (Armor), Run (Armor), Walk (Armor)) and TNAM
  "Animation Sounds Template"; observed: the rest]: of 389 ARMO, 5 have SNAM, all
  `…AudioTemplate` records (Light, Medium, Heavy, Plastic, Power), three sounds each,
  at 100%; 228 name one of them as template (Light 134, Medium 63, Heavy 24, Plastic
  6, Power 1); none sets BNAM. Light and Plastic use the plain types (FOLCloth…,
  FOLPlastic…), Medium, Heavy and Power the `(Armor)` types (FOLArmorMedium…, …). The
  type names and the sounds' names disagree consistently: type Walk holds
  `FOLClothSneak`, Sneak `FOLClothWalk`, Run (Armor) `FOLArmorHeavyWalk`, Walk (Armor)
  `FOLArmorHeavyRun` [observed; either xEdit's enum order or the records are off].

### 10.2 What NVForever plays (`--play`)

- **Which clips are heard** [design]: the player's rig in view (the first-person arms
  rig while it is drawn, else the third-person body) and every drawn actor within
  3,000 units of the player. Of a rig's layers, those with at least half their layer's
  weight; a clip first seen part-way is heard from its next key, one just started (0.1
  s) from its start; no actor steps or rustles twice within 0.2 s (crossfading clips).
- `Sound:` keys play the SOUN of that editor ID; the node after the name is not used.
  The player's sounds play at the listener, an actor's at its feet.
- `Enum: Left`/`Right`: a ray down from the feet; on the landscape, the texture showing
  most there (the default texture where none shows on more than half) gives the
  material, on a placed object the triangle's code; the family by
  `fnv_actor::anim_sounds::footstep_family` [hypothesis, from the names: stone, glass
  and heavy stone are ConcSolid; dirt, cloth, organic and skin Dirt; hollow metal,
  vehicle bodies, barrels and carts MetalHollow; sheet metal, cans and lunchboxes
  MetalSheet; other metal MetalSolid]; shapes without per-triangle codes count as
  ConcSolid, nothing underfoot as Dirt, swimming as Water [design]. The record is
  `FST<family>Stairs<side>` on stairs, else `<gait><side>`, falling back to the walk,
  the side-less sound and Dirt's [design]. The gait is the clip's `Armor…`/`Foley…`
  keys' (else from its name: `fast` runs, `sneak` sneaks) [hypothesis].
- **Armour's footfall** (#2541): beside the ground's step, an actor or the player in
  armour with the Power Armor flag (ARMO BMDT general flags) plays `FSTArmorPowerL`/`R`,
  in armour with the Heavy flag and not Power Armor `FSTArmorL`/`R`, not while swimming
  [observed for walking NPCs: observer check 140, the in-game probe of 2026-11-14 (T-45d
  FSTArmorPower beside FSTConcSolidWalk; NCR salvaged power armour, Heavy only,
  FSTArmor; FSTArmorPowerWalkL/R never); hypothesis: the same for the player, in first
  person, running and on other ground]. Each footstep NVForever plays also goes on the
  game's list of playing sounds on its actor for half a second, so `IsSoundPlaying`
  reads it as the original's does [observed: the same probe; engineering choice: the
  half second] (Titans of the New West watches `FSTArmorPowerL`/`R` for its own
  footsteps).
- `Enum: Armor<gait>` plays the worn armour's `(Armor)` sound for that gait,
  `Foley<gait>` its plain one [hypothesis]; the gait is read from the sound's name, not
  the type (§10.1) [design]; the chance is applied. Of several pieces worn, the first
  with armour sounds counts, else the first with any [design]. No armour, no rustle.
- **Doors**: the keys of the "Open"/"Close" sequence as the door passes them, at the
  door; a sequence without sound keys plays the DOOR record's open (SNAM) or close
  (ANAM) sound as it starts [design: whether the game plays the record's sound as well
  as the keys' is not known].
- Log: `anim sounds summary: N footsteps (… by the player, … by actors; ground: …), N
  armour rustles, N reload keys, N other sound keys, N door sounds (N from door
  records), N keys without a SOUN`; per sound with
  `RUST_LOG=info,nvforever::anim_sounds=debug` (`trace` adds what is underfoot).
- Built since (§14): creature sound types, the player's landing and jumping, wading, a
  door record's looping sound (BNAM), load doors' sounds. Stairs on the landscape: the
  data has none (§14.2).

## 11. World and script sounds

Owner request 2026-10-11: "the rest of the sounds in game", hits on walls and bottles
first, and shot glass that sounds like glass. Code: `nvf_play::world_sounds` (knocks,
items, containers, activators, message boxes, script sounds, the per-impact log),
`nvf_play::combat` (impacts), `fnv_combat::fx`, `fnv_game::sound`. Counts are of the
owner's load order (FalloutNV.esm and the DLC) [observed, `nvforever --play` start-up
log] unless they say FalloutNV.esm.

### 11.1 Shot impacts: surface → sound

A shot's impact takes the IPCT its weapon's impact data set (IPDS) gives for the
surface's impact material, and plays that IPCT's sounds (combat-rules.md §16).

- **Finding** [observed 2026-10-11, the bottles of combat-rules.md §14]: shooting
  Sunny's bottles played `FXBulletImpactEarth`, the dirt sound. A bottle's collision is
  one convex shape, and only triangle meshes kept their Havok material; a hit without a
  material counts as dirt. Every box, sphere, capsule and convex-vertices shape stores
  one material [documented: nif.xml `bhkSphereRepShape`]; it is now read
  (`CollisionObject::material`, the first such shape's when a body has several
  [design]) and given to the body's convex colliders. The same bottles now play
  `FXBulletImpactGlass` ("impacts by material: glass 3").
- Walls were not silent: buildings are triangle meshes whose triangles carry materials
  [observed: Goodsprings building 001055E0, triangles of 0 STONE and 17 SHEET_METAL].
- **Material names.** `fnv_combat::fx::impact_material` maps the 32 Havok materials
  (code mod 32) [documented: nif.xml `Fallout3HavokMaterial`] to the twelve IPDS
  materials by their names [hypothesis]. Checked against the names: stone, heavy stone
  and broken concrete → stone; cloth → cloth; dirt and sand → dirt; glass and bottle →
  glass; grass → grass; organic and skin → organic; water → water; wood and heavy wood
  → wood; hollow metal, sheet metal, vehicle body and hollow parts, barrel, soda can,
  shopping cart, lunchbox → hollow metal; metal, heavy metal, chain, bottle cap,
  elevator, solid vehicle parts, pistol, rifle, baby rattle and rubber ball → metal
  (the last two by default, not by name). Terrain has no code and counts as dirt
  [design].
- **Actors** take their base's Impact Material Type (NPC_ and CREA `NAM4`, the same
  twelve in the same order [documented: xEdit]), organic without one [design]. Read
  [observed, load order]: organic 5,565, metal 487, organic bug 318, hollow metal 40,
  organic glow 34, stone 8, grass 3. Before this every actor counted as organic.
- Destructible and loose objects go through the same path as walls: the impact is
  picked before the hit's damage or push [observed: the code and the bottles' log].

The table: the ballistic impact data set 0x00019083, which the 10mm pistol, the Varmint
rifle and the service rifle name (WEAP `INAM`) [observed: `fnvtool combat fx`]. Sound
folders are under `sound\fx\fx\bullet\impact\`; dB is the SOUN's static attenuation.

| Surface (example) | Havok material | Impact material | IPCT | Sound (folder, dB) |
|---|---|---|---|---|
| Sunset Sarsaparilla bottle (loose) | 24 BOTTLE, convex [observed] | glass | `BallisticGlassImpact` | `FXBulletImpactGlass` (`glass\`, 0) |
| window glass | 3 GLASS | glass | `BallisticGlassImpact` | `FXBulletImpactGlass` |
| concrete or stone wall | 0 STONE [observed: 001055E0] | stone | `BallisticStoneImpact` | `FXBulletImpactConc` (`concrete\`, 15.4) |
| sheet metal wall, car body, barrel, can | 17, 20, 23, 25 [17 observed: 001055E0] | hollow metal | `BallisticMetalHollowImpact` | `FXBulletImpactMetalH` (`metalhollow\`, 7.9) |
| solid metal, guns | 5, 11, 26, 27 | metal | `BallisticMetalImpact` | `FXBulletImpactMetalS` (`metalsolid\`, 10.1) |
| wood | 9, 12 | wood | `BallisticWoodImpact` | `FXBulletImpactWood` (`newood\`, 12.8) |
| terrain, dirt, sand | none, 2, 18 [observed: terrain] | dirt | `BalisticDirtImpact` | `FXBulletImpactEarth` (`earth\`, 8.4) |
| grass | 4 | grass | `BalisticDirtImpact` | `FXBulletImpactEarth` |
| water | 8 | water | `ImpactWaterSplashSm` | `FXBulletImpactWater` (`water\`, 0) |
| cloth | 1 | cloth | none: the set names no IPCT [observed] | none |
| human NPC | `NAM4` Organic [observed: a hit on 00109A39] | organic | `BallisticOrganicImpact` | `FXBulletImpactFlesh` (`flesh\`, 10.1) |
| robot | `NAM4` Metal or Hollow Metal | metal, hollow metal | as metal, hollow metal above | `FXBulletImpactMetalS`, `…MetalH` |
| insect (radscorpion, ant, …) | `NAM4` Organic Bug | organic bug | `BallisticOrganicBugImpact` | `FXBulletImpactFlesh` |
| glowing one | `NAM4` Organic Glow | organic glow | `BallisticOrganicGlowImpact` | `FXBulletImpactFlesh` |

Read [observed, load order]: 147 IPCT in 73 IPDS. The three organic kinds share the
flesh sound; their effects differ.

### 11.2 Loose objects knocking

No record names what a loose body sounds like when it hits something. FalloutNV.esm
has 51 SOUN named `PHY…` (files under `sound\fx\phy\<material>\`), most in pairs `…H`
and `…L`, named after the Havok materials (`PHYBottleH`, `PHYBarrelL`, `PHYCanSodaH`,
`PHYGenericMetalSheetSmall`, `PHYVehicleMetalBodyH`, `PHYChain`, `PHYRubberBall`, …),
and `CWoodSmall`, `CWoodMedium`, `CWoodLarge` whose files are `fx\phy\wood\crate_*`
[observed]. `world_sounds::phy_sound` takes the one named after the body's material
[hypothesis]; H for a hard knock, L for a light one [hypothesis: hard and light];
where a family comes in sizes, hard takes the larger [design]. Dirt, grass, water and
sand have none.

When [design]: a dynamic body (placed clutter, a dropped item) that loses at least 90
units/s of its speed in a frame (`knock_speed`: the speed lost along the way it was
going) knocks; from 300 units/s hard; at most every 0.12 s per body, and not in the
first 1.5 s after its collision came (bodies settling when a cell loads). Gaining speed
(falling, pushed by a shot) is no knock. A knock plays at the body.

Seen [observed 2026-10-11, Goodsprings]: a shot bottle falling off the fence,
`PHYBottleL` at 122 units/s and `PHYBottleH` at 322; tumbleweeds (`TumbleweedNV`,
collision material 30 BABY_RATTLE) rolling, `PHYBabyRattle`. `PHYTumbleweed` exists but
nothing links it to the tumbleweed (its MSTT has no `SNAM`) [observed].

Mods may supply per-form collision sound overrides via
`Data\NVSE\Plugins\SoundExtender\` (`*.json` / `*.txt`, `sound-extender.md`); when no
overrides are present, knocks take the material table above unchanged.

### 11.3 Items, containers, activators

| Record | Fields [documented: xEdit] | Records | Naming a pick-up / drop sound [observed] |
|---|---|---|---|
| WEAP | `YNAM` Sound - Pick Up, `ZNAM` Sound - Drop | 496 | 221 / 221 |
| ARMO | same | 489 | 427 / 425 |
| AMMO | same | 145 | 144 / 144 |
| MISC | same | 603 | 53 / 42 |
| KEYM | same | 313 | 283 / 283 |
| ALCH | same | 226 | 24 / 24 |
| CHIP | same | 6 | 1 / 1 |
| BOOK | same | 27 | 0 / 0 |
| NOTE | same | 1,225 | 0 / 0 |
| IMOD | same | 112 | 0 / 0 |
| CCRD | same | 324 | 0 / 0 |
| CMNY | same | 6 | 0 / 0 |

- Played: the pick-up sound when an item leaves the world for an inventory (activated
  by hand or by a script), the drop sound when a drop creates a reference, both at the
  reference [hypothesis: where]; in a container menu, the pick-up sound for what the
  player takes and the drop sound for what the player puts in, at the listener
  [hypothesis]. A record without one plays `UIItemGenericUp` or `UIItemGenericDown`
  [hypothesis: by the names; no record links them].
- Seen [observed 2026-10-11, GSHouseInterior03]: the BB gun `ITMPistolUp`, an empty
  bottle (no field) `UIItemGenericUp`, five kinds of food taken from the fridge
  `UIItemGenericUp`, 10mm rounds put in `ITMAmmunitionDown`, the jumpsuit dropped
  `UIItemsClothingDown`.
- CONT `SNAM` Sound - Open and `QNAM` Sound - Close [documented: xEdit]: 1,249
  containers name one or both [observed]; played when the container menu opens on the
  container and when it closes, at the container. The fridge 00105658 names neither.
- ACTI `VNAM` Sound - Activation [documented: xEdit]: 116 activators without a radio
  station (`RNAM`) name one [observed]; played when the player activates one, at it.
  Not seen in a run yet.

### 11.4 Message boxes and other interface sounds

- MESG has no sound field [documented: xEdit MESG], and `menus\message_menu.xml` names
  none for opening [observed]. Its buttons include
  `menus\prefabs\list_box_template.xml`, whose `clicksound` is `UIMenuOK` and
  `mouseoversound` `UIMenuFocus` [observed].
- Built: `UIPopUpMessageWindow` when a box opens [design: by its name;
  `UIPopUpMessageGeneral` names the same file, `fx\ui\popup\ui_popup_messagewindow.wav`
  [observed]], `UIMenuOK` when a button is picked (the box goes), `UIMenuFocus` when the
  choice moves.
- What the interface files name for the other menus, and what NVForever plays in them: §12.

### 11.5 Scripts

`PlaySound sound [SystemSound]` plays a SOUN not placed; `ref.PlaySound3D sound` plays
it at the calling reference [documented: GECK wiki PlaySound, PlaySound3D, owner
export]. The rules queue them (`Game::sound_requests`, traced `sound_asked`); the
engine plays them. Without SystemSound a sound asked for in a menu waits for the game
[documented: it "may be queued and not played until returning to GameMode"]. Seen
[observed]: a DLC start-up script's `PlaySound` (0x03016173). `PlayMusic` queues a music
type for music.rs (`Game::music_requests`, traced `music_asked`; §13).

Since a sound's file is read on a worker (§7, #1428), a script's sound can still be
waiting for its file when the script acts on it again. What the original does in these
cases is not known; NVForever does this [engineering choice; hypothesis for the
original] (`fnv-game/src/nvse_sound.rs` `SoundState::stop`, `nvf-play/src/sfx.rs`
`play` and `begin`):

- `PlaySound X; StopSound X` in one frame plays nothing: the stop covers the sounds of X
  asked for before it, also in the same frame. `StopSound X; PlaySound X` plays X.
  Observer check 82.
- A looping script sound (a SOUN with the Loop flag) whose file cannot be played is over
  at once: IsSoundPlaying reads 0 for it, as for a one-shot with no file.
- `LoopSound` on then off for one SOUN in one frame leaves nothing playing (the off drops
  the start still waiting for its file).
- A sound stopped while it plays (StopSound without a fade, LoopSound off or on again)
  fades out over 5 ms instead of being cut (#2217, §7) [design]; it is no longer the
  script's or the loop at once (a second StopSound or LoopSound off finds nothing of
  it). A one-shot started earlier in the same frame has
  made no sound and goes at once. StopSound with a fade time keeps its own fade. A
  script's one-shot that waits for a voice slot (§7) is answered when it starts; a
  StopSound drops it as one waiting for its file, so it never starts after the stop.

### 11.6 Checks and what is left

Log: `impact: …` (debug: collision material, impact material, IPCT, sounds), `knock: …`
(debug), `world sounds: …` for items, containers, activators, message boxes and
scripts; at the end `world sounds summary: …`. Smoke tests in CLAUDE.md (the bottles,
the house).

Not built: two loose bodies knocking each other told apart from one on the ground,
loudness by how hard a knock is. Objects' own sounds (ACTI `SNAM`, CONT `RNAM`), the
casings' sounds and panning: §14.

## 12. Menu sounds

Owner request 2026-10-11: build the sounds still missing; this part is the menus'. Code:
`nvf_play::ui_sounds` (the table `sound_for`, the menus watched, the summary), with
small hooks in `menu.rs` (a command's outcome), `pause.rs` (a switch flipped) and
`world_sounds.rs` (Take All). Message boxes (§11.4) and the quest lines
(quest-messages.md) play their own.

### 12.1 What the interface files say

A control of an interface file names a sound to play when it is clicked
(`<clicksound>`) and when the pointer comes onto it (`<mouseoversound>`): the Editor ID of
a SOUN [documented: GECK wiki Mouseoversound, owner export]; leaving it out is silence
like `&nosound;` unless a parent gives one [documented: same page]. The wiki's
interface guide calls `clicksound` "a numeric element" [documented: GECK wiki, Misc.xml
export], and some files give numbers.

Counted [observed 2026-10-11] over the 121 `.xml` files of `Fallout - Misc.bsa` (all of
`menus\`; `Update.bsa` holds none): 89 `clicksound` and 19 `mouseoversound` in 28 files,
and `globals.xml` sets `_sound_button_click` to `UIMenuOK`.

| Value | clicksound | mouseoversound | Where |
|---|---|---|---|
| `UIMenuPrevNext` | 20 | | prev/next arrows (level up, character, tutorial, map), the settings' meters and toggles (start_menu.xml), the race menu's sliders |
| `3` | 18 (2 more only off Xbox) | | every scroll bar prefab's arrows, sides and marker (vertical_scroll, vert_floating_scroll, horizontal_scroll, horiz_floating_scroll) |
| `&nosound;` (`nosound` once) | 15 | 13 | the inventory, map, stats, repair, item mod and reputation lists; level-up and character items; the dialogue topics (click only) |
| `UIMenuOK` | 9 | | list_box_template.xml rows; Continue, Done and the settings' buttons |
| `UIMenuFocus` | | 6 | list_box_template.xml rows, text_box, super_text_box, tab_box, the race menu |
| `UIPipboySelect` | 6 | | the stats page's CND, RAD, EFF, H2O, FOD and SLP buttons |
| `1` | 6 | | the main menu's six entries |
| `2` | 4 | | return buttons (select menu, save/load), the quest-added buttons |
| `UIPipboyScroll`, `UIPipBoyMode` | 2, 2 | | the race menu's scroll arrows; its back and next buttons |
| `UIMenuCancel` | 1 | | start_menu.xml `main_back_button` |
| `9` | 1 | | the QuestAddedMenu itself |
| copied | 3 | | book_menu.xml's pages from `BookMenu` `user8`; button_basic.xml from `gloabls()` `_sound_button_click` (spelt so) |

- What the numbers stand for is not known; no file or wiki page maps them. They are not
  played [design].
- The files write `UIPipboySelect` and `UIPipboyScroll` where the records are
  `UIPipBoySelect` and `UIPipBoyScroll` [observed]; Editor IDs are taken here as the
  records spell them.
- The menus NVForever draws, by their files: the container and barter rows and the
  dialogue topics include `list_box_template.xml` (click `UIMenuOK`, pointer
  `UIMenuFocus`), and the dialogue topics silence the click (`DM_Topic` `&nosound;`); the
  inventory's rows silence both (`IM_InventoryListTemplateRect`); the container and
  barter buttons (`text_box.xml`) name only `UIMenuFocus`; `sleep_wait_menu.xml`'s
  scroll bar (`scrollbar_horiz.xml`) and buttons name no click; `message_menu.xml`
  names none for opening; no menu file names a sound for opening or closing
  [all observed].

Files of the UI SOUN used [observed: FalloutNV.esm `FNAM`]: `UIMenuOK`
`fx\ui\menu\ui_menu_ok.wav`, `UIMenuFocus` and `UIPipBoyScroll` both
`fx\ui\menu\ui_menu_focus.wav`, `UIMenuPrevNext` `…\ui_menu_prevnext.wav`,
`UIMenuCancel` `…\ui_menu_cancel.wav` (as `UIActivateNothing`), `UIMenuMode`
`fx\ui\pipboy\ui_pipboy_mode.wav` (as `UIPipBoyMode` and `UICheckBox`), `UIMenuTab`
`fx\xxx_placeholder_silence.wav` (a silent placeholder), `UIPipBoyAccessUp` / `…Down`
`fx\ui\pipboy\ui_pipboy_access_up.wav` / `…_down.wav`, `UIItemTakeAll`
`fx\ui\items\ui_items_takeall.wav`. 123 SOUN of FalloutNV.esm start with `UI`.

**Aid items** name the sound of being used: ALCH `ENIT` bytes 16–19, Sound - Consume
[documented: xEdit, schemas/fnv/objects.json]. FalloutNV.esm: 186 of 189 ALCH name one
(`NPCHumanEatingFoodChewy` 93, `…Crunchy` 20, `NPCHumanDrinkingSoda` 19,
`NPCHumanUsingComic` 15, the Stimpak `NPCHumanUsingStimpak`, …) [observed]; the owner's
load order: 219 of 226 [observed, `nvforever --play` start-up].

### 12.2 What plays

| Menu | Event | Sound | Source |
|---|---|---|---|
| container, barter, dialogue, settings | the selection moves (keys, wheel, pointer; to the other list) | `UIMenuFocus` | [observed] the rows' mouse-over; [design] for the keys |
| barter | an item put up or taken back | `UIMenuOK` | [observed] the rows' click |
| dialogue | a choice answered | none | [observed] `DM_Topic` `&nosound;` |
| settings page | chosen; a switch flipped; back | `UIMenuOK`; `UIMenuPrevNext`; `UIMenuCancel` | [observed] start_menu.xml rows, toggles, back button |
| inventory | opened, closed (Tab) | `UIPipBoyAccessUp`, `…Down` | [design] Tab is the Pip-Boy's control; by name |
| inventory | the selection moves | `UIPipBoyScroll` | [hypothesis] the file silences its rows; the Pip-Boy's scroll by name (the same file as `UIMenuFocus`) |
| inventory | an item put on, taken off | the item's pick-up, drop sound (§11.3) | [design] |
| inventory | an aid item used | its ALCH consume sound | [documented] the field; [hypothesis] that the menu plays it |
| inventory | refused ("Cannot equip or use") | `UIMenuCancel` | [design] |
| container | opened, closed | the CONT's own (§11.3) | [design] nothing more |
| container | an item taken or put in | the item's own (§11.3) | |
| container | Take All | `UIItemTakeAll`; the items' own held back | [hypothesis] by name |
| container, barter | another category | `UIMenuPrevNext` | [design] as other menus' prev/next arrows |
| barter | the trade made | the caps' pick-up sound when paid, their drop sound when paying; `UIMenuOK` when no caps move | [design] |
| barter | refused (not enough caps, nothing to trade) | `UIMenuCancel` | [design] |
| barter, Wait/Sleep, pause | opened | `UIMenuMode` | [design] by name |
| barter, Wait/Sleep, pause | exit, cancel, resume | `UIMenuCancel` | [design] as the settings' back button |
| Wait/Sleep | the slider moved | `UIMenuPrevNext` | [hypothesis] as the settings' meters; the menu's own scroll bar names none |
| Wait/Sleep | the hours accepted | `UIMenuOK` | [design] the basic button's click (`_sound_button_click`) |
| Wait/Sleep | closed after the last hour | none | [design] |

Added for the owner's list 5 (items 39, 41, 42, 47; their sources in each menu's note):

| Menu | Event | Sound | Source |
|---|---|---|---|
| Vigor Tester | page turned; attribute raised, lowered; review row chosen; Done; refused | `OBJBookSpecialPageTurn`; `OBJBookSpecialNumber`, `…NumberDown`; `OBJBookSpecialFocus`; `OBJBookSpecialNumberFinished`; `UIMenuCancel` | [hypothesis] by the records' names (vigor-tester.md) |
| tag skills, traits | row chosen (keys, pointer), pointer onto Reset or Done; a row taken or given back; refused; Reset; Done | `UIMenuFocus`; `UIMenuPrevNext`; `UIMenuCancel`; `UIMenuCancel`; `UIMenuOK` | [observed] Done's click, the text boxes' pointer sound; [design] the rest (chargen.md §8) |
| terminal | cursor or selection moved; entered; password right; wrong | `UIHackingCharScroll`; `UIHackingCharEnter`; `UIHackingPassGood`; `UIHackingPassBad` | [hypothesis] by the records' names (hacking.md) |
| container | Take All | `UIItemTakeAll`, and the menu closes (owner, list 5 item 60) | |

All at the listener. The caps (MISC 0x0000000F `Caps001`) name no sound [observed in the
run: a trade's caps fall back to `UIItemGenericUp` / `…Down`, §11.3]; `ITMBottlecapsUp`
and `…Down` exist but nothing links them [observed].

### 12.3 Checks and what is left

Log: `ui sounds: <menu> <event>: <sound>` (moves of the selection at debug), at the end
`ui sounds summary: …` (sounds played by Editor ID, events by menu, events without a
sound and why, sounds not found, items held back for Take All). Smoke test in CLAUDE.md
(Chet's barter, the cash register, the inventory, a stimpak, the Wait menu, the pause
menu's settings) [observed 2026-10-11]: 24 played, 0 not found.

Not built: the menus that do not exist yet (the Pip-Boy's pages, tabs, light, hum and
radiation sounds; lockpicking, hacking, VATS, level up, perks, repair, surgery, the main
menu, loading screens), the popups not drawn (`UIKarmaUp` / `…Down`, `UIRepGood` / `…Bad`, `UIPopUpMapMarkerAdded`), the numbered
sounds 1, 2, 3 and 9, the quantity menu.

## 13. Battle, dungeon, incidental and script music

Owner request 2026-10-11: build the music that was "not built". Code:
`fnv_world::audio` (`RegionMusic`, `cell_regions`, `MediaController::sets_for`,
`dungeon_tracks`), `nvf-play` `music.rs` (`MusicCues`, battles, dungeon layers,
incidental phrases, script music), `fnv_game::sound::play_music`. Survey:
`cargo run --release -p fnv-world --example music_survey -- schemas/fnv <FalloutNV.esm>`.
Counts are of FalloutNV.esm alone [observed] unless they say otherwise.

### 13.1 What the records say

- **Field meanings by set type** [documented: xEdit `wbDefinitionsFNV.pas` field names,
  `schemas/fnv/audio.json`; GECK wiki Media Set, owner export, a page marked incomplete
  whose times are partly "unverified"]: a battle set's `NAM2` is its loop, `DNAM` the
  wait before the intro, `ENAM` the loop's fade-out time, `FNAM` the recovery time
  ("time before battle state completely ends and another music can start"),
  `HNAM`/`INAM` an intro and an outro SOUN. A dungeon set's `NAM2` is battle, `NAM3`
  explore, `NAM4` suspense ("while enemies are on alert"), `DNAM` minimum time on,
  `ENAM` crossfade overlap, `FNAM` layer crossfade time, `HNAM`/`INAM` intro ("while
  player starts combat") and outro ("when combat stops"). An incidental set's `HNAM`
  and `INAM` are a day and a night SOUN, `DNAM`/`FNAM` the day's minimum and maximum
  time, `ENAM`/`GNAM` the night's (the wiki: "Minimum time in seconds").
- **Battle sets** (32): all store wait 1, fade-out 3, recovery 10 and −10 dB; 16 named
  `…Perc` and 16 `…Full`, files `music\bttl\{good,evil}N\mus_bttl_lp_…`. Rural sets
  name `musBTTLAllRuralIntro`/`…Outro`, city sets `musBTTLAllCityIntro`/`…Outro`: 2D
  SOUN at 9 dB attenuation whose folders (`sound\fx\mus\bttl\allruralintro\` and the
  like) hold 5 to 7 takes each. `PNAM` holds values above 63 on some (11111100 binary
  on `musSetBTTLCityEvil1Perc`): the enable flags are a location set's.
- **Dungeon sets** (23): `musSetDNGN1ABC`–`8ABC` put `…_3High` in battle, `…_1Low` in
  explore and `…_2Mid` in suspense (`music\dngn\dungeonN\`), at −2.5, −4, −4 dB, with
  the city intro and outro; the three files have one length to the byte (Dungeon1 and
  2 looked at), as a location set's stems do. Two Hoover Dam sets do the same with
  `loc\hooverdam\` files; 13 `musSetOldFO1…`/`FO2…` sets name one Fallout 1 or 2 piece
  in all three. Times 6, 10, 6 (DNGN1–7) or 6, 15, 6.
- **Location sets' times** (81): 6, 15, 9 on 66 and 6, 15, 6 on 15: a minimum time on
  of 6 everywhere.
- **Controllers' lists**: battle lists hold battle sets only (265 entries); location
  lists 92 dungeon and 74 location entries; enemy lists 10 dungeon and 1 location
  entries; neutral, ally and friend lists location sets but for one dungeon set each.
  65 controllers have a battle list; their markers are 486 of the 572 (307 of them in
  interiors). `musCtrlGoodsprings` (faction `GoodspringsFaction`): neutral, ally and
  friend `musSetLOCDesExplBothABC`, enemy `musSetDNGN6ABC`, battle
  `musSetBTTLRuralEvil2Full` and `…Evil1Full`.
- **Delays and `NAM1`**: the location delay is 0 on all 89 controllers. The retrigger
  delay is 0 on 17 and 805.0667 on 39; the rest store values such as 7.8e−44, 1.3e−18,
  3.5e18, −10085 [observed], which look like bytes nobody set. `NAM1` (xEdit: "Flags and
  Enums, messily combined") is four bytes, the last three 0, the first 0x40 (2), 0x44
  (48), 0x45 (1), 0x54 (37) or 0x55 (1). 26 of the 34 controllers with more than one
  location set have 0x10 set [observed; hypothesis: the "Random" looping option of the
  GECK's dialog]. The GECK wiki's "Faction Not Found" and "Looping Options" choices are
  presumably in it; not decoded.
- **Regions** (REGN `RDMO` "Music", `RDSI` "Incidental MediaSet", `RDSB` "Battle
  MediaSets" [documented: xEdit wbDefinitionsFNV.pas, now in `schemas/fnv/sky.json`]):
  11 regions name music, none a music type; 10 an incidental set (`musSetINCDesert`,
  `…Hostile`, `…Mountain`, `…Freeside`) and 10 four to eight battle sets. Cells reach
  them two ways: through their acoustic space (ASPC `RDAT`, which xEdit calls "Use
  Sound From Region (Interiors Only)"), as 15,728 exterior cells and 1 interior reach
  `AudioExtDesertDefault` (incidental `musSetINCDesert`, eight rural battle sets), and
  other `AudioExt…` regions the same way; and through their own region list (`XCLR`:
  `AudioINCPrimm` 72 cells, `AudioExtNorthVegas` 278, `AudioINCMountain` 54). The
  Goodsprings cell lists only `GSWeatherRegion` and `VMapGoodspringsRegion`; its
  acoustic space gives it the desert region.
- **Incidental sets** (4, named by regions only, never by a controller): times
  (DNAM, ENAM, FNAM, GNAM) Desert 4, 6, 16, 12; Hostile and Mountain 6, 6, 10, 12;
  Freeside 6, 3, 12, 8. Their sounds are folders of short phrases:
  `musINCdesertday` (`fx\mus\inc\day\`, 121 Ogg files of 29 to 330 KB),
  `musINCdesertNight` (`…\night\`, 134), `musINCCreepy` (86), `musINCPeacful` (54).
- **Game settings**: `fCombatMusicNearCombatInnerRadius` 500,
  `fCombatMusicNearCombatOuterRadius` 2000, `fCombatMusicPlayerTargetedThreatRatio` 100,
  `fCombatMusicGroupThreatRatioMax` 4, `fCombatMusicStopTime` 3 [observed: the names
  and values; their meanings are hypotheses from the names].
- **Main menu** [observed: the install's `Fallout_default.ini`]:
  `SMainMenuMusicTrack=special\maintitle.mp3`, `sTitleMusic=MainTitle`,
  `fMainMenuMusicVolume=0.6`, `fDefaultMusicVolume=0.6`; `Data\Music\Special\MainTitle.mp3`
  is shipped.
- **Death**: the music type `DefaultDeath` names `special\death.mp3`, which is not
  shipped; a SOUN `MUSDeath` exists, and `sound\fx\mus\mus_death.ogg` is in the sound
  archive; `Data\Music\SCR\mus_SCR_DeathStinger.mp3` is shipped and no music type names
  it [observed]. Which of these the game plays when the player dies is not known.
- **`PlayMusic`** [observed: `fnvtool script decompile`]: 9 calls in FalloutNV.esm (7
  scripts, a dialogue line's result script, a quest stage) naming 7 music types:
  `musSCRGoodspringsStinger` (`VCG01RemoveDocCarebearModeTriggerSCRIPT`, the trigger
  outside Doc Mitchell's door), `1NoMusic` (the opening quest's stage 0 and the
  Vault 11 slideshow script), `musSCRVegasStinger`, `MUSKimbleSpeechHail` (a trigger
  and a dialogue line), `EndSlideShow`, `EndTransitionStinger` and `Endgame01`.
  The GECK wiki: "Will not stop the currently playing music of a music marker. NV
  Only." [documented].

### 13.2 What plays (as built)

1. **A battle** [design: NVForever's actors do not fight on their own yet]: a living,
   drawn, hostile actor (`fnv_game::rest::hostile_to_player`: aggression with the
   factions' relation) within `fCombatMusicNearCombatOuterRadius` that has the player
   detected (`nvforever` `detection.rs`) makes one; it ends `fCombatMusicStopTime`
   seconds after the last such look [hypothesis: the settings' meanings]. Aggressive
   actors neutral to the player (the coyotes east of Goodsprings) do not count, Very
   Aggressive ones (Fiends) do.
2. **Battle sets**: the marker's controller's battle list, else the region's (§13.1),
   one picked at random per battle [design]. Its intro plays after the wait time and
   its loop with it; the place's music fades out over the loop's fade-out time and the
   loop fades in over it; at the end the outro plays, the loop fades out, and the
   place's music starts again after the recovery time [documented meanings, unverified;
   design: intro and loop together].
3. **Dungeon sets** in a controller's list play as three stems in step: explore always,
   suspense added while a hostile actor near has noticed (or detected) the player,
   battle added in a battle [hypothesis]; in a battle at a dungeon set no battle set
   plays, the set's intro and outro do [design]. The enable flags are not read.
4. **Minimum time on**: a layer of a location or dungeon set that came in stays at
   least the set's minimum time on (6 s) [hypothesis: "Minimum Time on Layer"].
5. **Faction lists**: with a conditional faction, GetFactionRelation of that faction
   towards the player picks enemy, ally, friend or neutral sets; an empty list falls
   back to neutral, then location [design; GECK wiki: the lists' meanings]. Standing as
   GetFactionRelation reads it (faction reactions, SetEnemy/SetAlly, crime enemies);
   reputation is not consulted [hypothesis].
6. **Incidental phrases**: where no music plays at all (no marker, no cell or
   worldspace type, no battle), the region's incidental set plays its day (06:00 to
   23:54, the hours every controller stores) or night sound after a random wait between
   its two times, again and again [design]. **Unit: minutes** [hypothesis]: the GECK wiki
   says seconds, which with the desert's 4 to 16 would leave gaps shorter than the
   phrases themselves; `INCIDENTAL_UNIT` in music.rs is the one place to change it.
7. **Script music** (`PlayMusic`): the music type plays beside the place's music, at
   its `ANAM` level, once or looping by `ANAM`'s sign (§4.6), coming in at once; a new
   request replaces the last; a type without a file (`1NoMusic`) fades it out
   [hypothesis].
8. Intro, outro and incidental sounds go through `crate::sfx` at the listener (the
   effects volume) and are not played with music off [design].
9. **The Pip-Boy radio** (#3311): while it plays its station the place's music (its
   marker's set, else the cell's or worldspace's type, and its outgoing crossfades) is
   turned down to silence over 3 s; it plays on unheard, so when the radio stops it
   fades back in over 3 s where it has got to. "Plays its station"
   (`radio::pipboy_holds_music`, `radio::pipboy_line_seen`): on, tuned to a station
   received here, sound on and the player's Radio volume above 0, not at the poker
   table, the radios not held (the Esc menu, a loading screen), and one of that
   station's lines or songs heard now or within the last 8 s (the gaps between lines, a
   file still being read). Heard means played at a level above 0 after the volumes, the
   Radio While Talking tweak and reception's static; a line at no level, or the
   programme muted outright even between lines (a volume or that tweak at 0, the station
   all static), lets the music back at once, and retuning or switching off forgets the
   last station's line, which only counts for the station tuned. Static
   alone (a station saying nothing, one whose lines have no file, a station all static
   at the edge of its range) and a station out of reception leave the music alone. Only the level the mixer
   is given changes: the player's Music volume, SetMusicState's pause and the layers
   stay as they were. A world receiver (a placed radio, a jukebox, an actor made a
   radio) does not turn the music down. Battle music keeps its level over the radio: a
   battle set's loop, and a dungeon set's battle stem, which comes in and goes by its
   own fade while the explore and suspense stems stay down; so do script music (item 7)
   and the death and level-up music. Incidental phrases (item 6) are not changed
   [hypothesis: a player's report says the music should stop while the radio is on;
   battle music, how fast it goes, whether it resumes or starts again, and static or
   out of range are observer check 266; design: the 3 s fades, the 8 s hold, resumed
   rather than started again].

Seen [observed 2026-10-11, `nvforever --play`, the owner's load order]:
- Standing next to Violet (`--cell Goodsprings --pos=-46800,65000,4600 --hour 12`):
  `music: battle begins (0x000F570B has the player detected): musSetBTTLRuralGood3Perc
  after 1.0 s, from 8 battle sets` (the desert region's list), the intro `0x0014E8DA` a
  second later and the loop, then `battle over, 6.9 s after it began` when she lost
  sight of the player, and the outro `0x0014E8DB`. Outside every marker there: `no
  music here; musSetINCDesert plays a phrase in 650 s`.
- Walking onto Doc Mitchell's door trigger (the smoke test in CLAUDE.md):
  `music: a script plays musSCRGoodspringsStinger (once)` over the Goodsprings set.

### 13.3 Not built

- The main menu's music: NVForever has no main menu yet (the INI's track is above).
- The death music: the player does not die yet, and which record it is is open.
- The retrigger delay (its values look unset) and `NAM1`; the inner radius and threat
  ratios of the combat music settings; the crossfade overlap time (sets loop by
  starting their files again).
- The other NVSE music functions. IsMusicPlaying and SetMusicState are built
  (`nvse_gaps.rs`, [jip-ttw-functions.md](jip-ttw-functions.md) §2).

### 13.4 Open

- The incidental times' unit (§13.2 item 6), and whether phrases also play inside a
  marker's reach.
- Whether battle music needs the player to be the target, how the threat ratios weigh
  a group, and whether the original plays the intro before the loop or over it.
- What a dungeon set's "suspense" answers to, and how a controller with both dungeon
  sets and a battle list chooses.
- What the Pip-Boy radio does to the music (§13.2 item 9): whether battle music cuts in
  over it, whether the place's music fades or stops, whether it comes back when the
  radio is turned off, and what static at the edge of a station's radius or a station
  out of reception does (observer check 266).

### 13.5 Fallout 3 music (owner, list 5 item 69)

The owner hears Fallout 3's ambient music and asked whether a condition on the tracks is
missing. What the records say [observed 2026-10-16, FalloutNV.esm]:

- Eleven location sets play Fallout 3 pieces (`musSetOldFO3Base2`, `…Base3`, `…Base5`,
  `…Dungeon1`–`3`, `…Explore1`, `…3`–`6`, files `music\old\fo3\mus_fo3_*.mp3`,
  shipped loose in `Data\Music\OLD\FO3`). Their fields are those of the New Vegas
  single-piece location sets (`musSetLOCDesExplBothA` and the like): type 1, enable flags
  0x09, the day outer and night outer layers naming the same file. Nothing in a set marks
  it apart.
- 18 of the 89 controllers list them, beside New Vegas, Fallout 1 and Fallout 2 sets, in
  their location (and for two camps their neutral) lists: the generic
  `musCtrlGnrcPeaceful`, `…Mystical`, `…Dangerous`, `…Military`, `…NCRCamp`,
  `…LegionCamp` and `musCtrlGnrcFallout3Military`, `musCtrlNovac`,
  `musCtrlBoulderCityExternal`, `…Memorial`, `musCtrlNorthVegas`, the Gomorrah, Tops and
  NCR embassy controllers and `musCtrlCrashedB29`. 171 of the 572 audio markers name one
  of them, 64 out in the Mojave (`WastelandNV`); round Goodsprings the nearest are a
  `musCtrlGnrcMystical` marker (radius 3000) about 15,300 units from the town's marker,
  whose own controller `musCtrlGoodsprings` lists none.
- So the masters themselves ask for these pieces at those places: no condition on a set
  or a list keeps them out. The one field not understood is the controllers' `NAM1`
  (§13.1; the GECK's looping options and "Faction Not Found" are presumably in it): if
  the original plays a list in order ("Loop") rather than at random, the Fallout 3 pieces
  would come up in a fixed turn instead of NVForever's random picks [hypothesis]. Nothing
  changed in NVForever for this item; docs/owner-review.md asks the owner where it was
  heard.

## 14. Creatures, movement, objects' own sounds, placement

Owner request 2026-10-11: the remaining "not built" sounds. Code:
`fnv_actor::anim_sounds` (the creature records, key → type), nvforever `anim_sounds.rs`
(creatures, jumps, landings, wading) and `shot_fx.rs` (casings), `nvf_play::world_sounds`
(objects' own sounds, load doors), `nvf_play::ambient` (sources played like sound
markers), `nvf_play::sfx` (panning). Survey: the example `fnv-actor
creature_sound_survey` (FalloutNV.esm alone; `--show <CREA edid>`, `--near x,y,r`,
`--soun <prefix>`).

### 14.1 Creatures

- **The records** [documented: xEdit wbDefinitionsFNV.pas CREA, wbCSDT,
  wbSoundTypeSounds; `schemas/fnv/gameplay.json`]: `CSDT` a sound type, then pairs of
  `CSDI` a sound and `CSDC` its chance; `CSCR` "Inherits Sounds from" a creature. The
  types: Left Foot, Right Foot, Left Back Foot, Right Back Foot, Idle, Aware, Attack, Hit,
  Death, Weapon, Movement Loop, Conscious Loop, Auxiliary 1–8, then (xEdit lists
  "Auxiliary 8" twice, numbering the rest one higher) Jump and PlayRandom/Loop.
- **Counts** [observed]: 1,578 CREA. 86 list sounds themselves (most named
  `AudioTemplate…`), 999 only inherit them (`CSCR`), none do both, 468 only have a
  template (`TPLT`: 443 a leveled list, 25 a creature), 25 neither; 1,085 have sounds
  after following `CSCR`. Every type a creature lists holds one sound (704 type lists).
  Types listed (creatures): Left/Right Foot 72, Back feet 34, Idle 46, Aware 15, Attack
  40, Hit 32, Death 66, Movement Loop 12, Conscious Loop 37, Auxiliary 1–8 45 to 16,
  type 21 3; Weapon none, type 20 none. Chances: 100 on 541 of the 704, the rest 5–90.
- **Type 21** holds `NPCSecuritronSiren` on the three Securitron templates [observed]:
  a siren, not a jump; read with the observation that type 20 is never used, the game's
  numbering is likely Auxiliary 8 = 19, Jump = 20, PlayRandom/Loop = 21 [hypothesis].
  Not played.
- **Examples** [observed]: `AudioTemplateGecko` Left/Right Foot `NPCGeckoFootLeft`/
  `Right`, back feet `NPCGeckoFootRun…`, Attack `NPCGeckoSnarl` 50%, Hit
  `NPCFeralGhoulInjured`, Death `NPCGeckoDeath`, Auxiliary 1–7 voices, swing, chomp,
  jump; `AudioTemplateBigHorner` (Goodsprings' bighorners) feet `NPCBigHorner…FootL/R`,
  Idle `NPCBrahminIdleBreathing` 50%, Hit and Aware `NPCBigHornerInjured`, Death
  `NPCBighornerDeath`, Conscious Loop `NPCBigHornerConsciousLP` 50% (a folder, flagged
  Play At Random), Auxiliary 1–4; `NVCrRaven` only Auxiliary 1 and 3 (take-off and fly
  away); `AudioTemplateSecuritron` Movement Loop and Conscious Loop (files `…_lp.wav`).
- **The keys** [observed, `kf_sound_keys` over `Fallout - Meshes.bsa`]: creature clips
  carry `Enum: BackLeft` (136), `BackRight` (77), `Idle` (22), `Aware` (9), `Attack` (17,
  and `attack` 11), `Hit` (3, the cazador's hit clips), `Auxiliary1`–`8` (54, 261, 51,
  39, 39, 28, 23, 28; once `Auxiliary 3`), beside `Left`/`Right`. The bighorner's idle
  clip has `BackLeft` at 4.5, 9.1, 11.1 and 14.0 s; its walk `Left`/`Right`; its run the
  back feet and `Auxiliary2`; the raven's fly-away `Auxiliary1` at 0 s and `Auxiliary3`
  at 2.4 s.
- **As built**: the sounds are the creature's own list, else `CSCR`'s (followed up to
  five times) [hypothesis: an own list wins; none has both], of the creature a leveled
  list picked, else of the creature its Model/Animation template group comes from (a
  template-only creature, §16), else of the one placed. An `Enum:` key plays one of its type's sounds at
  its chance (`fnv_actor::anim_sounds::creature_key_type`, `pick_creature_sound`)
  [hypothesis: the keys name the types, one to one by name], at the creature, at most
  once per 0.2 s per type [design]. Hit and Death play when combat reports a hit or a
  death (`ActorHit`) [hypothesis]; `Enum: Hit` keys are not played again [design].
  A living creature within 3,000 units sounds its Conscious Loop (the chance rolled once
  per creature [design]) and while it moves (over 20 units/s) its Movement Loop, as
  sources the ambient plays like sound markers (§14.3). A creature without sounds steps
  like a person (§10.2). Nothing else plays a type: Aware, Idle and Attack without a key
  and the type-21 siren stay silent, as in the original (§16.1). Weapon is not built (no
  list names one).

### 14.2 The player's and actors' movement

- **Jumps and landings** [hypothesis: by the names]: `FST<family>Jump` when the player
  leaves the ground going up faster than 100 units/s, `FST<family>Land` on touching
  down after at least 0.4 s in the air [design: the thresholds], of the ground under the
  feet (§10.2), else `FSTDirtJump`/`Land`; at the listener. Not while swimming.
  **Actors** that step like a person (people, and creatures without a sound list, §10.2)
  play theirs by the same thresholds where they are, within 3,000 units of the player
  [design: the player's rule]: `actor_jumps.rs` tells each one leaving the ground (its
  character's vertical speed, or its feet's for a navmesh walker) and coming down
  (`ActorAir`). A creature with a sound list plays none: none lists a Jump (CSDT 20,
  §14.1) [observed], and how the original sounds a creature's jump is not documented.
  An actor stepping off a ledge (actor-jumps.md §3) leaves at vertical speed 0, so only
  its landing sounds.
  `FSTDirtJump` names `fx\xxx_placeholder_silence.wav` [observed].
- **Hard landings** (issue #1104): FalloutNV.esm has `FSTLandHardLight` (0x0005A724) and
  `FSTLandHardHeavy` (0x0005A725), both the folder `fx\fst\landhard\` (one file in
  `Fallout - Sound.bsa`, `fst_landhard_01.wav`), Random Frequency Shift, static
  attenuation 1762 and 1410 (hundredths of a dB); no record of FalloutNV.esm names either
  [observed: a scan of every record's fields for the two form ids]. Built: a landing that
  does fall damage (physics.md §6.12) plays the Heavy one for an actor in heavy body
  armour, else the Light one [hypothesis: the exe picks them, by their names, the way the
  data names armour footsteps `FSTArmorLightSneak` / `FSTArmorHeavySneak`], the player's
  at the listener, an NPC's at its feet, besides the ground's `FST…Land`; in god mode too
  [design]. Falls without damage play only `FST…Land`. The player's hurt voice is the
  combat lines' (combat-dialogue.md §3: `GenericPlayer` Hit lines), which every hit, a
  fall's too, may say by `fCombatSpeakHitThreshold` / `…Chance`; no SOUN of FalloutNV.esm
  is named for a player's pain [observed: no `Pain`/`Hurt` edid outside creatures and
  `UISurgeryPain01`–`03`], so no separate issue. Log: `falls: <actor> hard landing,
  sound FSTLandHard…`.
- **Wading** [design]: feet more than 2 units under a water surface (the cell's water or
  a placed water volume) step on Water (`FSTWaterWalkL`…, files `fx\fst\water\…`).
- **Stairs on the landscape** do not exist in the data: the 89 landscape textures' Havok
  materials are 0 (10), 2 (47), 4 (12), 10 (3), 18 (7) and 19 (10), none a stairs code
  (64–127) [observed].
- Smoke: `actor-jump-sounds` (Easy Pete drops off the Goodsprings ledge in play).

### 14.3 Objects' own sounds

- **The fields** [documented: xEdit]: ACTI `SNAM` Sound - Looping, CONT `RNAM` Sound -
  Random/Looping, DOOR `BNAM` Sound - Looping. In FalloutNV.esm [observed]: 216 of 1,143
  activators name one (Play At Random 164, Loop 49, neither 3), 144 of 2,478 containers
  (Loop 86, Play At Random 39, neither 19), 3 of 320 doors (`EMTWoodFence`, Play At
  Random); placed: 1,518 activators, 909 containers, 15 doors. Around Goodsprings: the
  mailboxes and dumpsters (`EMTSandOnMetal`), the picket-fence gates (`EMTWoodFence`),
  the saloon's exit triggers (`EMTWaterPressure`).
- **As built** [design]: each such reference, while enabled, is a source at its place;
  the ambient plays it as it plays a sound marker of that sound (§5: Loop goes round
  within the sound's reach, Play At Random rolls its chance once a second, hours apply).
  Sounds with neither flag are not played.
- **Load doors** [hypothesis]: activating a door that leads elsewhere plays its open sound
  (DOOR `SNAM`) at the listener as the player goes through.

### 14.4 Shell casings

FalloutNV.esm has `PHYCasingPistol`, `PHYCasingRifle` and `PHYCasingShotgun` (folders
`fx\phy\weapon\casing_pistol\` …) and no field links them to a weapon [observed]. 78
weapons name a casing model (WEAP `MOD2`): `assaultrifleshellcasing` 23, `grenadefrag`
13, `45calcasing` 11 (the 10mm pistol's), `762mmcasing` 10, `shotgunshellcasing` 6,
`eccellcasing` 6, others 1 or 2 [observed]. As built: the sound by the model's name
(shotgun → Shotgun; .45, .357, .44, 10mm, 9mm, pistol and energy cells → Pistol; the rest
Rifle) [hypothesis]; a bounce from 60 units/s plays it where the casing lands, up to
three times a casing [design].

### 14.5 Panning

How the game places sounds in stereo is not known. As built [design]
(`nvf_play::sfx::pan_gains`): a sound with a place is mixed to one channel and played in
stereo; the ear facing it hears it whole, the other loses up to 0.7 of it by how far to
the side it is (ahead, behind, above, below: both ears alike). The listener is the
player's eye and heading, followed while the sound plays. Panned: effects at a place
(impacts, knocks, actors' and creatures' sounds, doors, items, `PlaySound3D`), placed
loops and random markers, objects' and creatures' sources, radios. Not panned: sounds at
the listener (the player's own, interface), background loops, music, voices (a
recorded decision: voice plays flat, voice.md).

**Pan easing** (#2217, since stage 3) [design: how the original moves a sound's pan is
not known]. The listener's turn or a sound passing changes a playing sound's left and
right gains once a game frame; set at once, the gains jump between two samples, which
can click on a loud sound. Each panned sound's decoder (`PcmDecoder`, and
`PanDecoder` for the other modules' panned streams: placed loops, radios, voices) keeps
the gains it plays at, its own (the shared targets are the main thread's): its first
frame plays at the targets as they are, then each gain moves toward its target by at
most `1 / (rate × PAN_EASE)` a frame, `PAN_EASE` = 20 ms for a whole unit (the scale of
the volume sliders' easing), `rate` the sound's own sample rate, so slowed playing
stretches it with the sound. Both ears of a frame play one pair of gains. A sound
swapping sides (a gain moving 0.7) gets there in 14 ms; a sound that moves follows
slightly behind. A target that is not a number leaves the gain where it is.

### 14.6 Reverb

**The records.**
- ASPC `ANAM` is an environment type, stored as a number 0–30 [documented: xEdit
  `wbDefinitionsFNV.pas`, schemas/fnv/audio.json]: None, Default, Generic, Padded Cell,
  Room, Bathroom, Livingroom, Stone Room, Auditorium, Concerthall, Cave, Arena, Hangar,
  Carpeted Hallway, Hallway, Stone Corridor, Alley, Forest, City, Mountains, Quarry,
  Plain, Parkinglot, Sewerpipe, Underwater, Small Room, Medium Room, Large Room, Medium
  Hall, Large Hall, Plate. The GECK wiki calls the field a "drop-down list of DirectSound
  reverb presets" [documented: GECK wiki "Acoustic Space"], and the names after None are
  DirectSound's I3DL2 environment presets in the order of its
  `DSFX_I3DL2_ENVIRONMENT_PRESET_*` list [documented: Windows SDK `um/dsound.h`]. New
  Vegas has no REVB records (those are later games') [observed: no REVB group in
  FalloutNV.esm].
- The preset's parameters are DirectSound's `I3DL2_ENVIRONMENT_PRESET_*` values, the
  twelve fields of `DSFXI3DL2Reverb`: Room and Room HF (mB), Room Rolloff Factor, Decay
  Time (s), Decay HF Ratio, Reflections (mB) and Reflections Delay (s), Reverb (mB) and
  Reverb Delay (s), Diffusion and Density (%), HF Reference (Hz) [documented: dsound.h,
  Windows SDK 10.0.26100.0]. `nvf_play::reverb::PRESETS` holds them.
- SOUN `SNDD` flag 2 is Environment Ignored, flag 5 Menu Sound [documented: xEdit; GECK
  wiki GetSoundFlag]; SOUN `GNAM` is "Reverb Attenuation Control" (i16) [documented:
  xEdit; GECK wiki GetSoundTraitNumeric, trait 11]; its unit is not documented and it is
  not read. The desert's background loops carry Environment Ignored [observed:
  `AMBDesertBedDawn`].
- The game applies the environment only when the INI setting `bEnableEnviroEffectsOnPC`
  is 1 [documented: GECK wiki "Acoustic Space" notes], and the shipped default INI sets
  it to 0 [observed: `Fallout_default.ini`]: as shipped, the original plays no reverb on
  PC. The wiki also says acoustic spaces placed in cells (the placed boxes) do not work
  in New Vegas [documented: GECK wiki].
- In FalloutNV.esm [observed, `fnvtool world audio`, `acoustic_spaces`]: 113 spaces,
  99 interior, 14 exterior. Interior: Small Room 26, Stone Corridor 25, Auditorium 23,
  Large Room 10, Cave 5, Medium Room 3, Medium Hall 3, Livingroom, Sewerpipe, Stone Room
  and None 1 each. Exterior: Medium Room 12 (`ExtDesertDefault`, the lake shore and the
  lake body, the airport, Hoover Dam, …), Mountains 2 (`EXTTheStrip`, `ExtSlum`).
  Examples: the Prospector Saloon and Doc Mitchell's house are Small Room, Vault 19
  Stone Corridor, the Goodsprings cave Cave.

**As built** (`nvf_play::reverb`, NVForever plays it on by default; the settings page's
Sound tab switch `reverb`, or `--no-reverb`, turns it off) [design: the owner asked for
reverb indoors, owner-review.md].
- **The environment** is that of the acoustic space of the cell the listener is in (CELL
  `XCAS`, as for the background loops, §6): inside, as stored; outside, only an open-air
  preset (Alley, Forest, City, Mountains, Quarry, Plain, Parkinglot, Underwater), a
  room's preset playing dry [hypothesis: the Medium Room of 12 exterior spaces, the open
  desert among them, reads as a value left unset]. The log says each change:
  `reverb: interior 00153159: acoustic space INTCave (001445CA) environment Cave -> Cave`.
- **The sounds** [hypothesis]: every sound effect (§10–§14: shots, impacts, footsteps,
  doors, creatures, the random placed sounds) and the placed loops, as they start; not
  the records with Environment Ignored or Menu Sound, the interface sounds (`UI…`), the
  background loops (the room's own tone), music, radios, or the loops a weapon
  holds while it fires (they repeat their file, and the tail would break at each repeat).
- **Voices** (`reverb::for_voice`, played by `voice.rs`; built 2026-11-01, #987): a line
  heard where it is said (SayTo, Say, hellos, barks, lines between actors; voice.md §9)
  goes into the listener's environment after the voice filter, and its reverb stops
  with the line, no tail: "during dialogue the reverberation if any is abruptly cut off
  at the end of each spoken line" [documented: GECK wiki "Special Effect" notes, owner
  export]. A line cut early stops with its reverb. The send is 0.85: the
  [Audio] `fDialogReverbAttenuation=0.15` of `Fallout_default.ini` [observed] read as a
  SOUN's Reverb Attenuation Control is (0 all, 1 nothing) [hypothesis: its unit is not
  documented]. The conversation with the player and 2D lines stay dry [design: the wiki
  note can be read as the conversation being reverbed in the original; NVForever keeps
  it whole and unplaced, as voice.md has it]. `bEnableEnviroEffectsOnPC=0` of the
  shipped INI (above) is not followed for voices, as it is not for world sounds: the
  owner asked for reverb indoors, and voices follow the same Reverb switch [decision].
  The Sound tab's Voice reverb switch (`voice-reverb`, kept in the INI) keeps every
  voice dry. Log: `voice: … in the room's reverb: <environment> (send 0.85, cut at its end)`; the
  `reverb summary` and `voice summary` count the lines through it.
- **Per sound** [hypothesis: DirectSound's effects run on each sound buffer, so a sound's
  reverb falls with its distance as the sound does]: each sound has its own reverb and
  plays in stereo, and goes on for the preset's delays plus its decay time (at most 6 s)
  after its file ends.
- **The processor** [hypothesis: how DirectSound renders the parameters is not
  documented]: the sound mixed to one channel; Room HF a one-pole low-pass at the HF
  reference; the early reflections one tap at Reflections Delay at Room + Reflections;
  the late reverb Reverb Delay later at Room + Reverb, through eight parallel feedback
  combs per ear (Freeverb's lengths, the right ear 23 samples longer) and four series
  all-passes. Each comb falls 60 dB in Decay Time; a low-pass in its loop makes the HF
  reference fall in Decay Time × Decay HF Ratio (a ratio above 1 taken as 1). The combs'
  inputs are scaled so an impulse's late tail carries its level's energy. Density scales
  the comb lengths (half to whole), Diffusion the all-passes' gain (up to 0.5). Room
  Rolloff Factor is 0 in every preset.
- **A sound stopped before its end** (#2217, §7) [design]: its fade (5 ms) takes the
  whole sound, reverb tail included, so a forced stop shortens the tail and frees the
  voice slot promptly; a sound that ends by itself keeps its whole tail.
- Not built: a reverb that changes under a sound already playing. (The `GNAM` reverb
  attenuation was built 2026-10-24, §15.5; voices in rooms 2026-11-01, above.)
- Smoke: `reverb-cave-exit` (the Goodsprings cave, a shot, out through its door).

### 14.7 Checks

Log: the `anim sounds summary` ends `creatures: N sounds (<type> n, …), N hits, N deaths,
N keys of a type not listed; the player jumped N and landed N times; actors jumped N and
landed N times`; the `ambient
summary` ends `things that sound (activators, containers, doors, creatures): N heard, N
loops started, N random sounds played`; the `world sounds summary` ends `N load doors; N
objects' own sounds in reach`; the `shot fx` line ends `N casing knocks heard`. Per
sound with `RUST_LOG=info,nvforever::anim_sounds=debug,nvf_play::ambient=debug`. Seen
[observed 2026-10-11, Goodsprings by the bighorner pen]: the bighorners' idle `Left Back
Foot` (12 in 25 s), their conscious sound now and then, 4 hits and a death of
00109A39 with its Hit and Death sounds, the player's jump and landing on dirt, 10
casing knocks of 5 10mm casings. Smoke test in CLAUDE.md.

## 15. Sounds the records name that were not played (ESM audit group 5)

From the ESM gap audit (docs/handoff/esm-gap-audit-2026-10-21.md, "Sound"). Counts are
FalloutNV.esm alone, from a private survey script (not committed) [observed 2026-10-22].
"Loop", "Random" and "neither" are the named SOUN's Loop / Play At Random flags.

### 15.1 What the records say

| Field | Records naming one | Placed | Kinds | Most named |
|---|---|---|---|---|
| MSTT `SNAM` Sound | 45 of 241 | 1,033 refs | Loop 40, Random 3, neither 2 | `EMTFlies`, `AMBWaterSpoutLP`, `OBJConsole03LP` |
| STAT `RNAM` Sound - Looping/Random | 740 of 6,785 | 30,959 refs | Random 526, Loop 175, both 9, neither 30 | `EMTFlies`, `EMTUtilityPoleMetal`, `EMTMetalStairsCreak` |
| LIGH `SNAM` Sound | 13 of 501 | 276 refs | Loop 13 | `FXFireBarrelLP` (all) |
| TERM `SNAM` Sound - Looping | 268 of 344 | 184 refs | Loop 268 | `OBJComputerTerminalLP` 267 |
| ADDN `SNAM` Sound | 5 of 37 | (in models) | Loop 3, neither 2 | `FXFireLP`; the bullet-impact add-ons name impact sounds |
| WATR `SNAM` Sound | 32 of 78 | — | Loop 32 | `AMBWaterUtilityLP`, `AMBWaterMurkySewerLP`, `AMBWaterLakeLP` |
| PROJ DATA Sound | 37 of 95 | — | neither 32, Loop 4, 2D 1 | `FXBulletWhiz` 25, `FXMissileFlyBy` 4 (Loop) |
| WEAP `NAM6` Sound - Block | 49 of 261 | — | neither 49 | `WPNBlockKnife`, `WPNBlockWood`, `WPNBlockHand` |
| WEAP `UNAM` Sound - Idle | 16 of 261 | — | neither 12, Loop 4 | `WPNRiflePlasmaIdleLPM`, `WPNFlamerIdleLPM`, `WPNChainSawIdleLP` |
| MGEF DATA Effect / Bolt / Hit / Area Sound | 0 / 1 / 19 / 0 of 289 | — | Hit: neither 13, Loop 4 | `FXFireImmolationLP`, `FXMeleePowerFistMetal`, `FXDisintegration` |

[documented: xEdit wbDefinitionsFNV.pas for every field name; `schemas/fnv/*.json`.]
The SOUN flags over the 3,190 sounds [observed]: 2D 440, 2D Radius 102, Mute When
Submerged 2,846, Start at Random Position 110. 2,423 keep the attenuation curve
(100, 50, 20, 5, 0); 32 set loop points. STAT `BRUS` Passthrough Sound: 255 (none) on
3,384 statics, 0 on 14, 11 on 5, 1 on 1. ARMO `DNAM` bit 0 Modulates Voice: 38 armours.

### 15.2 As built

- **Placed objects' own sounds** (`nvf_play::world_sounds`): MSTT `SNAM`, STAT `RNAM`,
  LIGH `SNAM` and TERM `SNAM` join ACTI `SNAM`, CONT `RNAM` and DOOR `BNAM` (§14.3): each
  placed reference of such a base, while it is enabled, is a source where it stands,
  played as a sound marker of that sound (a Loop sound goes round within its reach, a
  Play At Random one rolls its chance once a second) [hypothesis: the fields mean the
  same as the ACTI and CONT ones; the names say so]. Sounds with neither flag are not
  played, as in §14.3. A static never moves, so its place is read once [design].
- **Add-on nodes** (nvforever `own_sounds.rs`): an add-on node in the world whose ADDN
  names a sound is a source at the node while its emitter shows [hypothesis: the fire
  add-ons' `FXFireLP` is the fire's crackle]. The two bullet-impact add-ons' sounds have
  neither flag and are not played (the impact already plays its IPCT sound, §11.1).
- **Projectiles in flight** (nvforever `own_sounds.rs`): a projectile whose PROJ sound has
  the Loop flag sounds it where it flies, as a source that follows it [hypothesis: a
  missile's fly-by loop]. One whose sound has neither flag (the bullet whiz) plays it
  once, where it passes the listener: the frame it starts to draw away after coming
  closer, when its nearest was within the sound's maximum distance [hypothesis: a
  fly-by; design: when]. A shot that only draws away (the player's own) never whizzes,
  nor one that ends on the listener (its impact sounds instead). A first rule (on
  coming within reach from farther) never fired in 67 smoke runs: the shooters stood
  within the whiz's reach.
- **Block** (`nvf_play::combat`): a blocked blow plays the blocker's weapon's `NAM6`, at
  the blocker [hypothesis: whose weapon; `WPNBlockHand` is on the unarmed weapons].
- **Idle** (`nvf_play::combat`, nvforever `own_sounds.rs`): while a weapon whose `UNAM`
  names a sound is out, that sound goes round: the player's at the listener, an
  actor's at the actor [hypothesis: an idle hum. 12 of the 16 have neither flag, but
  their names end `LP`/`LPM`, so it is played as a loop whatever its flags].
- **Magic effects** (`nvf_play::effect_sounds`): an effect starting on an actor plays
  its MGEF Hit Sound at the actor once; a Hit Sound with the Loop flag goes round
  instead, at the actor, while the effect lasts [hypothesis]. The Effect Sound (none in
  the master) goes round at the actor while the effect lasts [hypothesis: by the name].
  The Bolt Sound (one MGEF, no spell projectile drawn) and Area Sound (none) are not
  played.
- **Water** (`nvf_play::world_sounds`): the water types of the water near the listener
  sound their `SNAM` at the nearest point of its surface: an exterior cell's water over
  its grid square, a placed water volume over its top [hypothesis: the water's own
  lapping]. The type is the cell's `XCWT`, else the worldspace's, as for radiation
  (`radiation.rs`); a placed water volume's is its PWAT's.
- **SOUN 2D** (`nvf_play::sfx`, `nvf_play::ambient`): a sound with the 2D flag played at
  a place is not panned. Nor does its loudness fall with distance, unless it also has 2D
  Radius: §15.6 (until #3444 it did).
- **Start at Random Position**: a loop with the flag starts at a random point of its
  file instead of its start [documented: the flag's name].
- **Mute When Submerged**: while the listener (the player's eye) is under the surface
  of physics' water (cells' and placed volumes'; an interior's own `XCLW` is not water,
  physics.md §6.6), sounds with the flag are silent: one-shots are not started, loops
  are held at no gain [documented: the flag's name; hypothesis: the listener is the
  eye]. A swimmer floating at the surface keeps its eye above it (feet 102.5 under); one
  that went below the floating height stays there with its eye under [observed:
  `own-sounds-lake`, feet about 200 under, eye 81 under: 13–21 sounds muted]. The
  under-water fog's rule was tried and dropped: it reads an interior's `XCLW` as a
  surface, which muted the Goodsprings cave (`reverb-cave-exit`).

### 15.3 Not built, and why

- **BRUS** Passthrough Sound: an index (0–11, `BushA`…) with no record linking it to a
  SOUN, and the 20 statics that set it are too few to infer the table from names.
- **Loop points**: in what unit they count is not documented (32 sounds set them).
  The attenuation curve and the reverb control are built (§15.5).
- **ASPC walla** (a crowd loop with a trigger count) and **ALOC location / retrigger
  delays**: how the count and the delays act is not documented; music.md keeps the
  delays "not built". The walla is used by no shipped master (§1; issue #990), so the
  original cannot show what the count counts without a plugin that sets one, and there
  is nothing to play until a mod does [observed 2026-11-06].
- **ARMO Modulates Voice**: built (§15.5); what the original's helmet filter sounds
  like is still a capture question. **TACT No Voice Filter**: read, and nothing is
  filtered for a talking activator (§15.5, observed).
- **The Bolt and Area Sounds** of MGEF: one bolt sound, no area sound in the master,
  and no spell projectile is drawn.

### 15.4 Checks

- `world sounds summary`: objects' own sounds in reach, by kind, and the water types
  heard.
- `own sounds summary` (nvforever): add-on nodes with a sound, projectiles with a loop,
  whizzes, actors' weapons idling.
- `effect sounds summary`: hit sounds played, effects with a looping hit or effect sound.
- `ambient summary`: loops started at a random position; `sfx:` sounds silenced under
  water and 2D sounds not panned.
- Block sounds per blow with `RUST_LOG=nvf_play::combat=debug` ("blocks: WPNBlock…").

Smoke scenario `own-sounds-goodsprings`.

### 15.5 The attenuation curve, the reverb send, whizzes and the voice filter (2026-10-24)

From the ESM audit (docs/handoff/esm-full-audit-2026-10-24.md, gaps 7, 15, 17). Code:
`nvf_play::sfx::distance_gain`, `nvf_play::reverb::send_of`, nvforever `own_sounds.rs`,
`nvf_play::voice_filter`.

- **The records** [documented: xEdit wbDefinitionsFNV.pas SOUN `SNDD`;
  `schemas/fnv/objects.json`]: after the start and stop times, `attenuation_curve` (five
  `i16`), `reverb_attenuation_control` (`i16`), `priority` (`i32`), the loop points.
  Over FalloutNV.esm's 3,190 sounds [observed]: the curve (100, 50, 20, 5, 0) on 2,423,
  then (100, 32, 20, 5, 0) 112, (100, 60, 30, 10, 0) 84, (100, 50, 25, 10, 0) 67 …, flat
  (100 at every point) 9; the reverb control 80 on 1,687, 0 on 1,032, 100 on 107.
- **What they mean** [documented: GECK wiki Sound, the owner's export
  (`C:nv-private\geckwiki`, MiscWithmessage.xml)]: the Attenuation tab "has five
  sliders to modify the attenuation curve. The topmost slider is the sound's volume
  percentage at its inner radius. The bottommost slider is the sound's volume percentage
  at its outer radius. The values in between ... describe the volume falloff curve
  between the inner and outer radius of the sound. The curve defaults to a logarithmic
  falloff." Reverb Attenuation Control: "When set to 100, the sound's reverb send is
  100% attenuated -- no sound is sent into the current reverb preset. When set to 0 ...
  all of the sound is allowed into the current reverb preset." The minimum and maximum
  attenuation distances are the inner and outer radius (same page).
- **As built**: within the minimum distance a placed sound plays at the curve's first
  point; between the two distances it follows the curve, the middle three points
  evenly spaced and straight lines between the points [hypothesis: the wiki gives no
  distances for the middle points]; at and beyond the maximum distance it is silent. A
  percentage is a share of the amplitude [hypothesis]. With the default curve a sound
  is at half its level a quarter of the way out (was: minimum ÷ distance times a linear
  fade). A sound whose record has no curve, and the radio's fallback template, play the
  default curve. Dialogue keeps its own rule (`voice::voice_gain`).
- **Reverb send**: a sound sends 1 − control ÷ 100 of its amplitude into the reverb of
  the listener's space [hypothesis: linear]; 100 sends nothing (the sound plays dry and
  does not count in `reverb summary`). The dry sound is unchanged. Most sounds now reverb
  at a fifth of the level they did (control 80).
- **Whizzes** (PROJ `DATA` flag Supersonic, 48 PROJ of the masters): "If checked, the
  sound emanates from the point of the projectile's path closest to the listener ...
  If unchecked, the sound emitter follows the projectile around until it is destroyed"
  [documented: GECK wiki Projectile]. A supersonic projectile's sound plays once, at the
  point of its path nearest the listener, the frame it starts to draw away [design:
  when]; any other's follows it, going round while it flies [hypothesis: the plasma,
  pulse and sonic projectiles' sounds are named `…ProjectileLP` but lack the Loop flag].
  This replaces §15.2's rule by the sound's Loop flag.
- **The voice filter**: a speaker wearing an armour with ARMO `DNAM` "Modulates Voice"
  (38 armours, all helmets and masks: the power armour helmets, the Ranger combat
  helmet, the Legion helmets, gas and surgical masks [observed]; "Checked if the item modulates the actor's voice", ArmorAddon
  "distorts the character's voice (like Power Armor helmets do)" [documented: GECK wiki
  Armor, ArmorAddon]) and a talking activator whose record header lacks bit 13 "No
  Voice Filter" [documented: GECK wiki GetFormFlag] is heard through a filter. 85 of
  FalloutNV.esm's 87 TACT lack the bit (intercoms, speakers, the Lucky 38 mainframe), the
  two with it are radios (`TopsBruceIsaacRadio`, `TopsLonesomeDrifterRadio`); Dead
  Money, Honest Hearts and Old World Blues set it on none of their 45 [observed]. The
  filter's sound is NVForever's [hypothesis]: a 350–3,200 Hz band, a mild saturation
  and a 3.5 ms ring, at about the same level (`voice_filter.rs`).
- **Talking activators are not filtered** (issue #992, observer batch o28, 2026-11-06)
  [observed]:
  - *The files are processed.* The lines said only through an activator (condition
    `IsActorTalkingThroughActivator == 1`: Benny 11 files, Chris Haversam 7, Ramos 21,
    Tabitha 11, Captain Graham 2) against the same speakers' lines said face to face
    (12 to 26 files each), by the share of the file's energy per band: below 300 Hz
    0.02–0.05 of it through the activator against 0.30–0.43 face to face, above 6 kHz
    0.0001–0.0009 against 0.007–0.037; Benny's through the intercom lie 25–30 dB under
    his own below 200 Hz and 10–20 dB under above 8 kHz, Tabitha's radio lines are a band
    of about 400–2,500 Hz, Ramos's carry added noise above 3 kHz. Only the NCR radio
    operator's two lines look like plain speech. (Private script over the Voices
    archives, `fnvtool voice line` and `bsa extract`, decoded with ffmpeg.)
  - *The original adds nothing.* Observer campaign `o28_intercom` (save `nvfstrip`, the
    Tops presidential suite, 249 units from VTopsPresidentialIntercomREF 0x00129556):
    `SetTalkingActivatorActor BennyREF` and `Activate player` on the intercom (as its
    trigger's script does, observer-checks 47) opened the dialogue and Benny's GREETING
    0x00147865 played, all three responses, while the game's output was recorded on the
    WASAPI loopback. Each response's file is found in the recording with a normalised
    correlation of 0.80–0.83, and the transfer from the file to the recording stays
    within +4.5 and −4.1 dB of its 600–1,500 Hz level from 200 Hz to 6 kHz (coherence
    0.8–1.0): +4 dB at 300–400 Hz, −4 dB at 2–3.5 kHz, +0.7 dB at 4–5 kHz. A 350 Hz
    high-pass and a 3,200 Hz low-pass would take 3 dB at 350 Hz and about 8 dB at 5 kHz.
    The tilt is unexplained (the room, the game's mix) [hypothesis]; no band filter is
    applied to the line. Above 6 kHz the file holds too little to measure.
    2026-11-27 (#4486): with a face-to-face control in the same room, which played flat, the
    same tilt came back through the REPCONN intercom. So it comes with the talking
    activator [observed], a mild EQ rather than a band; the dry file 0x000CECF7 is still to
    record (observer check 4486-talking-activator-voice-eq).
  - *Built*: a talking activator's lines play as their files; the voice filter is only for
    armour that modulates the voice. The TACT "No Voice Filter" flag is read but changes
    nothing heard [hypothesis: it may name the pre-processing done to the files, or
    another use; nothing shows it]. Whether the original filters a helmet wearer's voice
    is not captured (observer-checks).

### 15.6 2D sounds have no radii (#3444, 2026-11-23)

- **The flags** [documented: GECK wiki Sound]: 2D "forces playback in 2D with no 3D
  positioning or inner/outer radii settings in the game world"; 2D radius "forces playback
  in 2D but with 3D positioning and inner/outer radii settings in the game world. Use with
  stereo files only."
- **The records** [observed 2026-11-23: a one-off script over the plugins' SOUN records,
  `SNDD` flag bits 6 and 11 and the maximum distance]: of FalloutNV.esm's 440 sounds with 2D, 232 store 0
  for the maximum distance and 1 has 2D Radius as well; of Lonesome Road's 30 with 2D, 11
  store 0 and 10 have 2D Radius as well, all 10 with distances. A record with no distances
  heard by distance is silent beyond a few units.
- **The missile launch** at Lonesome Road's second silo [observed, LonesomeRoad.esm and
  its archives]: the launch quest's script (SCPT xx00868E) plays Forward (13.33 s) on
  `NVDLC04MissileLaunchREF` (xx007CEE, `nvdlc04_silo-launch.nif`) and 13 s later Backward
  (53.30 s, in which the node `Rocket_Root` climbs and flies 13,300 units off). Forward's
  sound key at 0.1 s names `NVDLC04QSTMissileLaunchPt1` (2D and 2D Radius, 1,275 to
  25,500 units, a flat curve, 9.25 dB off; 18.7 s, quiet for its first 10), Backward's at
  0.067 s `NVDLC04QSTMissileLaunchPt2` (2D, both distances 0, nothing off; 21.3 s, loud
  from its start). Both files are stereo 16-bit PCM at 24 kHz. The script that starts the
  quest moves the player to `NVDLC04MissileLaunchViewMarkerREF`, 2,900 units from the
  reference: there Pt2's gain by distance was 0.0003, under the 0.001 a sound needs to be
  started, the silent flight of the report. The other sounds of the launch are
  `PlaySound`s, at the listener: `musNVDLC04WarheardExplode` (xx00D0BA, a musical sting)
  with Forward, `NVDLC04QSTMissileLaunchPt3` with the explosion 33 s later. No sound is
  moved with the flying node: Pt2 is 2D, so it has no place [documented, above].
- **As built** (`sfx::no_radii`, `sfx::placed_gain`): a one-shot at a place (object keys,
  PlaySound3D, doors, explosions, random markers in reach; not other actors', below) with
  2D and without 2D Radius is as loud at any distance [documented, above]. With both flags the radii hold [hypothesis:
  the radius flag wins; most such records are placed loops with radii]; 2D Radius alone is
  as before (panned, by its radii).
- **Kept** [design]: a placed loop (a sound marker, an object's own sound) still sounds
  within its maximum distance and falls toward it whatever its flags: heard from anywhere,
  one marker would sound over its whole worldspace. A Play At Random marker is still only
  rolled within its maximum distance.
- **Other actors** [design] (`sfx::ActorSound`): the sounds of another actor's or a
  creature's clips (sound keys, footfalls, armour footfalls, rustles, creature sound
  types), its jumps and landings, a creature's hit and death, a load door it passes
  (`DRSVegasDoorOpen`, the Freeside gates'), an effect hitting it and the sound a placed
  line plays instead of a voice (107 responses name a 2D one, 106 of them files under
  `songs\` [observed 2026-11-23: the same script over the INFO `TRDT` sounds of FalloutNV.esm
  and five add-ons, load-order winners]) go by the record's distances whatever its flags, gain and reach both,
  as before #3444; the player's own keys and placed objects' keys do not. The generic
  `chair_leftexit.kf` (IDLE `ChairLeftStand`) keys `NPCDocMStandUp` (2D, 265 to 1,600
  units): whole, every actor rising from a seat within 3,000 units (`ACTOR_RANGE`) would be
  heard. The falling
  gain is kept, not a whole one cut at the reach: it changes nothing from before, and has
  no edge to step over. 12 sounds are such clip keys [observed 2026-11-23: `fnv-nif`'s `kf_sound_keys` example over the game's mesh archives, its
  keys matched to the 2D SOUN records]: nine
  of Doc Mitchell's and the opening's clips, all with distances, and Mr House's
  `NPCMrHouseWakeUp`, `NPCMrHouseElectro` and `NPCMrHouseDeath`, with none. Those three stay
  as before: 1 ÷ distance, all but silent (0.01 at 100 units); his terminals' scripts also
  play them with `PlaySound`, at the listener. An effect's `FXPoisonStinger`, with none
  too, is as silent on another actor as it was.
- **Log**: each sound key an object's group passes says what became of it: `sfx: sound key
  <EDID> starts: <file>, <s> s, gain <g>[, 2D], asked at (x, y, z), <n> units away`, or
  `not heard (too quiet | muted under water | voice limit | sounds off): …`, `not heard: no
  file to play`, `not heard: <file> cannot be played`, `dropped: ready <n> ms after it was
  asked for` (§7's 150 ms).
- What the original does with 2D alone, 2D Radius and both: observer check
  3444-2d-sounds-and-distance. Smoke
  scenario `missile-launch-sounds`.

## 16. Creature sound audit (owner's playtest list 7 item 46, 2026-10-23)

The owner heard nothing from feral ghouls. Survey: `fnv-actor creature_sound_survey`
(FalloutNV.esm alone) and `fnv-nif kf_sound_keys` (`KEYS_FILTER=enum`, base meshes
archive) [observed 2026-10-23].

- **Why the ghouls were silent** [observed: smoke `creature-sounds-ghoul`, before the
  fix "creature 000CE9E7 lists 0 sounds"]: the placed ghoul's base `LvlFeralGhoul`
  (0x0003A6FA) is a template-only creature: no sounds, no `CSCR`, `TPLT` a leveled list
  with every template flag set (Model/Animation among them). Its leveled list is not
  "a leveled list picked" (the base is a CREA, not an LVLC), so the sounds were read
  from `LvlFeralGhoul` itself. They are now read from the creature the Model/Animation
  group comes from (`fnv_actor::resolve::CreatureAppearance::model_source`, the same
  pick the drawn model used) [hypothesis: the sounds go with the model and its
  animations, whose keys name them; xEdit does not say which template group holds
  `CSDT`/`CSCR`]. The ghoul then sounds its footsteps (`Left`/`Right` walking,
  `BackLeft`/`BackRight` running), its attack voices and swings (`Auxiliary2`–`5` in
  every `h2hattack…` clip), its chase voice (`Auxiliary8` in `mtfastforward.kf`), its
  aware voice (`Auxiliary6` in `h2hequip.kf`, as it raises its hands), Hit and Death.
- **How many it touched**: 468 CREA have only a template. Following it (to a creature,
  or to every creature a leveled list can pick, down the lists): 283 get sounds from
  every pick, 159 from some (their lists hold Fallout 3 creatures without sounds as
  well), 26 from none (the ceiling turrets `LvlTurretCeiling…`, `LvlCrTrogg`,
  `BethesdaGhoul`, a few FFER/quest creatures, `NVCRTumbleweedBaby`).
- **Aware**: only an `Enum: Aware` key plays it (the mole rat's `mtforward.kf`, the
  mirelurk king's `specialidle_alert.kf`, 9 clips in all). From 2026-10-23 to 2026-11-06 a
  creature becoming aware (`fnv_game::awareness` Normal or Lost to Alert or Combat) played
  it as well [hypothesis then]; the original does not (§16.1), so that was dropped.
  `creature-sounds-aware` now checks that radscorpions noticing the player play none.
- **Attack** is still only played by the clips' keys (`Enum: Attack`, in the radroach,
  radscorpion, giant rat, ant, queen ant, spore carrier, brahmin and the gecko's power
  attacks). Not at every attack's start [design]: the ghoul's list has Attack
  `NPCFeralGhoulAttack` 100 % and its attack clips already play the same sound through
  `Auxiliary3` at 69 %, so an engine-played Attack would double it. Lists with Attack
  and no key in their clips (the lakelurk king, the ghoul) stay quiet on that type.

What each sound template lists (parentheses: only some of the row; F front feet, B back
feet, MoveLP/ConsLP the movement and conscious loops, AuxN how many Auxiliary types)
[observed]:

| Template | Types |
|---|---|
| FeralGhoul, FeralGlowingOne | F B Attack Hit Death Aux7 |
| Gecko, FireGecko | F B Attack Hit Death Aux7–8 |
| BigHorner | F B Idle Aware Attack Hit Death ConsLP Aux4 |
| RadScorpion, BarkScorpion | F Idle Aware (Attack) Hit Death ConsLP Aux6 |
| Brahmin | F B Idle Aware Attack Hit Death ConsLP |
| MoleRat, Mirelurk, MirelurkKing | F (B) Idle Aware Hit Death (ConsLP) Aux2–8 |
| SporeCarrier, LakelurkKing | F (B) Idle Aware Attack Hit Death ConsLP (Aux8) |
| Ant, RadRoach | F (B) Idle Attack (Hit) Death Aux1–3 |
| Dog, DogRex, GiantRat, YaoGuai | F B (Idle) Hit Death (MoveLP/ConsLP) Aux6–8 |
| Cazador, Centaur, NightStalker, Deathclaw, Mantis, AntQueen | F (B) (Idle) (Hit) Death (ConsLP) Aux3–7 |
| BlowFly (bloatfly) | Hit Death MoveLP ConsLP |
| SporePlant | Aware Hit Death MoveLP |
| SuperMutant | F B Aux2 (no Hit or Death: their voices are dialogue, the VTYP's combat topics) |
| SuperMutantBehemoth, RobotProtectron | F ConsLP |
| RobotMrHandy, RobotSentrybot, RobotRobobrain, RobotEyebot, TurretCeiling, Securitron | MoveLP ConsLP (Securitron F Aux1) |
| NVCrRaven | Aux2 (take-off, fly away) |

Every type these list now plays where something calls for it: feet and Auxiliary and
Idle, Aware and Attack types by the clips' keys, Hit and Death on hits (`ActorHit`), the
loops while the creature lives (and moves). Not played, as in the original (§16.1): the
type-21 siren, Idle without an `Enum: Idle` key (the mole rat, yao guai, giant rat), Aware
and Attack without a key (the radscorpion's and lakelurk king's Aware, the lakelurk king's
Attack). Left: Weapon (no list names one), super mutants' and robots' combat voices
(dialogue topics, not creature sounds), and the 26 creatures whose templates give none.
Checks: the `anim sounds summary` (types counted, "Aware n"), smoke
`creature-sounds-ghoul`, `creature-sounds-aware`, `bighorner`.

### 16.1 What the original plays without a key (issue #989, 2026-11-06)

Observer campaigns `o28_creature_sounds` and `o28_creature_sounds2` (plugin version 10,
save `nvfairport`, the Searchlight Airport runway, god mode): one creature spawned with
`PlaceAtMe` per session, and every sample (5 game loops) JIP's `IsSoundPlaying <sound>
<creature>` [documented: GECK wiki IsSoundPlaying, JIP: "Returns 1 if the specified Sound
is currently being played by the passed objectRef"] for three of its sounds, plus the
first one from any source, read back through the player's `Variable01`-`09`. The probe
works: it read the Securitrons' Conscious Loop the whole time, `NPCSecuritronForward`
as they set off, the mole rat's Aware `NPCMoleRatMtFwdVox` twice while it ran (its
`mtforward.kf` has the `Enum: Aware` key) and the radscorpion's Attack
`NPCRadScorpionClawAtk` five times in its fight (its clips' `Enum: Attack` keys)
[observed]. What was never read [observed 2026-11-06]:

| Creature (sound template) | Situation | Sound, never playing |
|---|---|---|
| VStreetSecuritron (Cops), VStreetSecuritronMk2 (Army) | 21–24 s standing, then `StartCombat player`: 26 s (36 hits) and 12 s (8 hits) of fight | siren `NPCSecuritronSiren` (type 21) |
| VStreetSecuritron | `player.SendAssaultAlarm` on it (it fought, 37 s, 70 hits), `SendTrespassAlarm player` (it did not fight; 36 s), in combat and hurt with `DamageAV Health` until it died | the siren |
| CrMoleRatDomesticated (MoleRat) | 83 s idling and wandering (procedures 0 and 7), then 25 s in combat | Idle `NPCMoleRatIdle` (50 %) |
| NVCrGiantRat (GiantRat) | 3 s idle, then it attacked by itself: 93 s, 39 hits | Idle `NPCMoleRatIdle` (50 %) |
| CrLakelurkKingCG (LakelurkKing) | 25 s out of combat, then `StartCombat player`: 70 s, 46 hits on the player; again finding the player by itself (combat at once), 72 s, 51 hits | Aware `NPCMirelurkKingEquip` (100 %), Attack `NPCMirelurkKingAttack` (50 %), Idle `NPCMirelurkKingIdleFeeding` (50 %) |
| CrRadscorpion1Small (RadScorpion) | found the player by itself: 60 s of fight, 15 hits | Aware `NPCRadScorpionAware` (75 %), Idle `NPCRadScorpionIdle` (100 %) |

No script of FalloutNV.esm names the siren or these Idle, Aware and Attack sounds
(`fnvtool script decompile`, all units) [observed]. So in what was tried the original
plays a creature's Idle, Aware and Attack only from its clips' keys, and the siren not at
all; when, if ever, the siren plays is open (observer-checks 96). NVForever plays none of
them otherwise (nvforever `anim_sounds.rs`); the Aware on becoming aware it played
before is dropped. The siren's folder is `fx\npc\robotsecuritron\futz\`.


## 17. Volume sliders (issue #1450, 2026-11-03)

- **Defaults** [observed: `Fallout_default.ini` `[Audio]`]: `fDefaultMasterVolume=1.0`,
  `fDefaultFootVolume=0.5`, `fDefaultMusicVolume=0.6`, `fDefaultRadioVolume=0.5`,
  `fDefaultEffectsVolume=1.0`, `fDefaultVoiceVolume=0.6`. An install left at its
  defaults reads back the same with JIP's `GetGameVolume` 0-5 (master, footsteps, voice,
  effects, music, radio): 100, 50, 60, 100, 60, 50 [observed: capture job, the owner's
  install]. NVForever starts its sliders there (`audio_out::DEFAULT_VOLUMES`); before,
  all were 1, and music, voices and radio played louder than the original's.
- **Main menu theme** [observed: WASAPI loopback of the original at its main menu,
  `MainTitle.mp3` decoded and fitted to the recording per channel, offset by
  cross-correlation; four runs, the fit explaining all of the recording]: the theme
  plays at 0.61 of the file (−4.3 dB) with the Music slider at 60, master 100 and
  `fMainMenuMusicVolume=0.6`. One 0.6, not the two multiplied (0.36): either the theme
  takes the slider and not the INI value, or the INI value and not the slider.
  NVForever takes the slider alone [hypothesis], so the default menu matches the
  original and the slider still turns the theme down.
- **`SetGameVolume` from the console** (JIP) changed nothing heard, at the main menu
  (the theme's level was the same after `SetGameVolume 4` 80, 60, 40 and 20) or in game
  (no step in the level when the music slider was set to 0 and back, and the other
  categories kept playing after being set to 0) [observed: loopback, same runs]; the
  sliders read back at their defaults afterwards. It does not reach the playing mix, so
  it cannot measure the slider's curve.
- **The curve** stays linear [hypothesis]; the menu result fits it (0.6 → 0.6) but does
  not tell it from the other hypothesis above. The reporter of #1450 found NVForever's
  music at 0.6 still louder than the original's in game; the in-game music level (music
  types' `ANAM` decibels, §13) was not measured. Owner checks:
  docs/handoff/observer-checks.md.

## 18. Output protection (#2217, 2026-11-12)

Whether the original game limits, compresses or clips its summed mix is not known; this
is NVForever's own [design]. Many sounds at once (a minigun's shots, a pack of mantises)
can add up past full scale, and what the device's conversion then does with them is not
measured here [hypothesis: it clips them]. So the summed 48 kHz stereo mix, after the
categories' and the master's volumes, goes through a look-ahead limiter on its way to
the device (or to the drain without one) (`nvf-play/src/audio_out.rs` `Limiter`, in the
run-long `MixBus`):

- **Threshold** 0.98 (`LIMIT_THRESHOLD`): no sample leaves the bus louder; a little
  below full scale, so nothing over it reaches the device, and ordinary sound below it is
  not touched.
- **Look-ahead** 144 frames, 3 ms (`LIMIT_LOOKAHEAD`): each frame needs a gain
  `a = 0.98 / max(|left|, |right|)` when its louder sample is over 0.98, else 1. A frame
  goes out 144 frames later at `min(ceiling, the last gain + release step)`, the ceiling
  being the lowest of `a[n + k] + k / 144` over the next 144 frames and itself (at most
  1): the gain starts down in a straight line within the look-ahead before a peak, from
  the farthest frame whose `a + k / 144` is below 1 (the deeper the cut, the earlier: a
  peak needing 0.49 is met 73 frames, 1.5 ms, ahead), so even a single frame over cannot
  pass, and is never above what the frame itself needs. A frame turned down also has
  its samples held to 0.98, so the product's rounding (one f32 step) cannot pass it.
- **Release** 50 ms from no gain to unity at most (`LIMIT_RELEASE`, a step of 1/2400 a
  frame), so the gain does not pump between peaks close together.
- **One gain for both channels** of a frame: a sound's place in stereo is kept.
- **Unity below the threshold**: a mix that stays at or below 0.98 comes out exactly as
  it went in, 144 frames later. Around an overload the look-ahead and the release turn
  some quieter samples down with it (less than 3 ms before a peak and up to 50 ms
  after).
- **Latency**: all sound reaches the device 3 ms later than it did.
- A frame with a sample that is not a number (or infinite) goes out as silence (each
  sound's own guard, `audio_out.rs` `GuardedSource`, turns such samples to silence
  already).
- The limiter's delay and gain belong to the bus, so a device reopened (`audio_out.rs`'s
  watcher, on a change of device) goes on with them. Per frame it costs a few compares and two multiplies; its
  buffers are allocated at the start; the audio thread allocates, locks and logs nothing
  more for it.
- **Measured** in the exit line `audio mix: pre peak P, N samples over full scale; post
  peak Q, M samples over full scale; K frames limited`: `pre` before the limiter (the raw
  mix's overloads stay counted), `post` after it (M should be 0 and Q at most 0.98), K
  the frames it played below unity. It works on the 48 kHz bus; what the device's own
  conversion does after it is not measured here.

## 19. BulletSnap.ini pass-by snap audio

Clean-room native support for the `BulletSnap.ini` configuration schema (`fnv_combat::bullet_snap`, `nvf_play::bullet_snap`, wired from `nvf_play::combat::fly` and `nvforever::own_sounds`).

**Sources and provenance.** Only the public Nexus Mods page, the shipped INI file comments, and the loose sound-folder hierarchy of **Bullet Snap - ReRedux v1.2** (by LowbeeBob, `https://www.nexusmods.com/newvegas/mods/89474`) were consulted; the third-party plugin's DLL/binary and source code were never consulted.

- **Vanilla unchanged when absent** [design]: when `Data\NVSE\Plugins\BulletSnapConfig\BulletSnap.ini` is absent from the VFS (or when no BulletSnap audio folders exist under `Data\Sound\fx\`), `BulletSnap` remains inactive: no folders are scanned, no synthetic SOUN records are registered, no snap audio is emitted, and vanilla `PROJ` `Supersonic` whizzes (`own_sounds.rs`, §15.5) behave identically to unmodded vanilla.
- **INI location, sections, keys and defaults** [documented: Bullet Snap - ReRedux v1.2 (`https://www.nexusmods.com/newvegas/mods/89474`), shipped `Data\NVSE\Plugins\BulletSnapConfig\BulletSnap.ini` and its inline `;` comments]:
  - Path: `Data\NVSE\Plugins\BulletSnapConfig\BulletSnap.ini` (case-insensitive VFS lookup).
  - `[General]`:
    - `ImpactedPlayerSnap = 0`: whether to play a snap when a projectile hits the player (`; Snap when hit player`).
    - `DistancedBasedSnap = 1`: whether to classify pass-by snaps into the four `[DistanceBased]` bands (`; Will check distance between projectile and player when bullet pass by player when enable`).
    - `Subsonic = 1`: whether projectiles slower than `SubsonicThresholdSpeed` play subsonic pass-by audio (`; enable subsonic bullet pass sound`).
    - `SubsonicThresholdSpeed = 21000`: projectile speed threshold (game units/s) dividing subsonic from supersonic snaps (`; Will do subsonic 'snap' when projectiles' speed lower than this value`).
    - `SnapAngle = 75`: threshold angle in degrees between the projectile's flight direction and the vector from the projectile to the player camera (`; Check the angle between projectiles and player camera / ; Greater than this angle will do snap. / ; 90 Means that the projs' direction is perpendicular to the vector of proj to player's camera`).
  - `[DistanceBased]` (active when `DistancedBasedSnap = 1`; `; Setting the max distance for each snap sounds.`):
    - `VeryClose = 768`, `Close = 1536`, `Distance = 4092`, `Far = 8192` (maximum distance in game units for each band).
  - `[DamageBased]`:
    - Key names and defaults `middledmg = 32`, `highdmg = 46` [documented: Bullet Snap - ReRedux v1.2 shipped `BulletSnap.ini`]. Because the shipped INI and public mod page do not state which damage value `middledmg`/`highdmg` measure (base weapon damage vs. ammo-modified or effective damage), comparing them against weapon `base_damage` to select the medium/high-caliber (`HCCloseMid`) and very-high-caliber (`VeryHighCaliber`) supersonic audio tiers is `[hypothesis]`.
- **Parsing and clamping** [design]: parsed directly from raw bytes (`&[u8]`) with optional UTF-8 BOM stripping, `;`/`#` comment stripping, ASCII case-insensitive section/key matching, and finite monotonic clamping (`SnapAngle` in `0..=180`, non-negative speed/damage thresholds, `VeryClose <= Close <= Distance <= Far`, `middledmg <= highdmg`), so non-ASCII or legacy Windows-1252 bytes in comments or unknown keys never panic.
- **Pass-by detection and eligibility** [hypothesis/design]:
  - Non-player-owned ballistic projectiles (`Motion::Missile`, excluding mines, remote-detonated charges, beams, flames, lobbers, and non-supersonic explosive rockets) are tracked across swept movement segments (including hitscan `Ghost` flights).
  - A projectile must approach the listener (`dot(dir, eye - from) > 0`, angle `< 90°`) before crossing `angle > SnapAngle`; it snaps at most once per pass-by at the closest point on that segment to the player's eye (`closest_on_segment`).
  - When a projectile hits the player, `ImpactedPlayerSnap = 0` suppresses snapping for that projectile; `ImpactedPlayerSnap = 1` triggers one snap at the impact point.
  - When active, `BulletSnap` suppresses the duplicate vanilla `PROJ` `Supersonic` whiz (`FXBulletWhiz`) in `own_sounds::projectiles` while leaving non-supersonic looping projectile sounds unchanged [design].
- **Sound folder layout and playback**:
  - Loose `.wav` folder hierarchy under `Data\Sound\fx\` [observed: Bullet Snap - ReRedux v1.2 (`https://www.nexusmods.com/newvegas/mods/89474`) release archive loose directory layout — 10 folders: `SubSonic\Close` (10 files), `SubSonic\Mid` (18), `GeneralCloseMid\Close` (10), `GeneralCloseMid\Mid` (12), `GeneralFar` (26), `HCCloseMid\Close` (23), `HCCloseMid\Mid` (15), `VeryHighCaliber\Close` (12), `VeryHighCaliber\Mid` (12), `VeryHighCaliber\Far` (8)]:
  - Tier/band folder selection [hypothesis/design]:
    - Subsonic (`speed < SubsonicThresholdSpeed` with `Subsonic = 1`): `SubSonic\Close` (`VeryClose`/`Close`), `SubSonic\Mid` (`Distance`/`Far`).
    - Supersonic General (`damage < middledmg`): `GeneralCloseMid\Close` (`VeryClose`/`Close`), `GeneralCloseMid\Mid` (`Distance`), `GeneralFar` (`Far`).
    - Supersonic High-Caliber (`middledmg <= damage < highdmg`): `HCCloseMid\Close` (`VeryClose`/`Close`), `HCCloseMid\Mid` (`Distance`), falling back to `GeneralFar` (`Far`).
    - Supersonic Very-High-Caliber (`damage >= highdmg`): `VeryHighCaliber\Close` (`VeryClose`/`Close`), `VeryHighCaliber\Mid` (`Distance`), `VeryHighCaliber\Far` (`Far`).
  - Synthetic `SOUN` FormID allocation [design]: each non-empty folder is registered once on `Sfx` inside the reserved audio-only subrange `0xFE7E_0000..0xFE7F_0000` (`fnv_game::game::BULLET_SNAP_SOUND_BASE..fnv_game::game::CAMERA_OVERLAY_BASE`), recorded in `docs/spec-notes/callable-identity.md` and strictly disjoint from `TEMP_REF_BASE..BULLET_SNAP_SOUND_BASE` (`0xFE40_0000..0xFE7E_0000`, enforced in `Game::temp_ref` and `Game::temp_item`), `CAMERA_OVERLAY_BASE` (`0xFE7F_0000..0xFE80_0000`), `LAMBDA_BASE`/`MADE_PACKAGE_BASE` (`0xFE80_0000..0xFF00_0000`), `FormId::FIRST_DYNAMIC` (`0xFF00_0000..`), `CLONE_BASE`/`COMPILED_BASE` (`0xFE00_0000..0xFE40_0000`), and normal plugin indices (`0x00..=0xFD`). Registration never overwrites an existing entry in `Sfx::forms.sounds`: if a loaded `SOUN` record already occupies a preferred slot, the loaded record wins untouched and BulletSnap selects the next free ID in `0xFE7E_000A..0xFE7F_0000`. Registered sounds use reach `max(Far, 8192) * 1.25` and attenuation curve `[100, 80, 60, 40, 15]`, played via `ScaledSound` with per-band gain (`VeryClose` 1.0, `Close` 0.85, `Distance` 0.65, `Far` 0.45; `1.0` with `Close` folder within `8192` units when `DistancedBasedSnap = 0`) so snaps get 3D panning, reverb, volume sliders, and voice limits through the existing `Sfx` pipeline.

