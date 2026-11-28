# Wind on loose bodies: the tumbleweeds

How the weather's wind moves bodies in NVForever, and what the original was seen to do.
Issue #1401. Code: `engine/nvforever/src/tumble.rs` (the wind), `crates/fnv-phys`
`PhysWorld::wind_refs` / `PhysWorld::blow` (the bodies), `crates/fnv-collide`
`CollisionObject::wind` (the flag).

## 1. Which bodies

- **The flag.** A rigid body's Body Flags: "1 = respond to wind" [documented: niftools
  nif.xml, bhkRigidBody, field Body Flags]. NVForever reads bit 0 of the field
  (`schemas/nif/havok.json` `body_flags`).
- **Who sets it.** Three of the base archive's 12,941 collision objects:
  `clutter\tumbleweednv.nif`, `effects\tumbleweedpile.nif` and
  `clutter\nv_caesarthrone\nv_caesarthrone.nif` [observed: `fnvtool collide objects`
  over `Fallout - Meshes.bsa`, its `wind` column, 2026-11-06].
- **Placed tumbleweeds.** FalloutNV.esm places 52 references of the MSTT `TumbleweedNV`
  (all persistent, in WastelandNV's persistent cell, most around Goodsprings and along
  the road south) and 4 of the ACTI `TumbleweedRanch01` [observed: `fnvtool world refs
  --world WastelandNV`]. The three tumbleweed creatures (`NVCRTumbleweed`,
  `NVCRTumbleweedBaby`, `NVCRTumbleweedRad`) are placed nowhere in the masters: no ACRE
  and no leveled list names them [observed: FalloutNV.esm, the creatures' FormIDs appear
  only in their own records and one XNAM], so what the owner saw blowing are the MSTT
  clutter.
- **Scripted.** The Goodsprings intro pushes one tumbleweed: Doc Mitchell's trigger
  script enables `StartingTumbleweedREF`, forces `NVWastelandClearWindy` and 8 s later
  places `vSmallImpulseExplosion` at `VCG01TumbleweedImpulseREF`
  ("VCG01RemoveDocCarebearModeTriggerSCRIPT" [documented: its source in FalloutNV.esm]);
  that explosion's force moves it as any explosion does.

## 2. What the original does [observed: campaign o27_tumble, `nvfairport` save, 2026-11-06]

The observer tracked ten `TumbleweedNV` references around the `coc Goodsprings` spot
(-72151, 643) and one far away (0x00178A76 at (2760, -86718)) by FormID, a sample about
every 0.11 s, for 30 s in each of `NVWastelandClear` (WTHR DATA wind speed 50),
`HVDustStorm` (127) and `WastelandNorth` (25), forced in that order in one game.

- **After the load** (the first 30 s, NVWastelandClear) four of the ten were rolling:
  0x00178A64 went 701 units net (1,012 along its path) at 150–210 units a second towards
  326–352° (clockwise from north), climbed 85 units and stopped by 17 s; 0x00178A67 went
  643 units at 140–190 units a second towards 308–332° and stopped at 5 s; 0x00178A7F
  and 0x00178A61 moved 800 and 195 units along their paths in starts and stops. They
  spun as they went (their heading turned ±250° a second). The other six stayed put.
- **Then they stayed at rest.** No tumbleweed at rest started again in the next minute,
  in the dust storm or in the calm weather. Only 0x00178A64, stopped on its slope, kept
  rocking back and forth: 1,181 units of path for 84 net in the storm, 461 for 41 in the
  calm.
- **Far off** 0x00178A76 never moved (its cell was not loaded).

So [hypothesis]: the wind pushes a tumbleweed that is awake, along one direction
(north-north-west here, about 330°), and does not wake one that sleeps; a tumbleweed is
awake when its cell loads, rolls until a slope or an obstacle stops it, and then sleeps.
How fast it goes with the wind's speed is not shown (the storm found them asleep); the
direction may be a fixed world wind or the weather's (WTHR DATA stores no direction
[observed: the record's other DATA bytes are 45, 45 or 0, 0 in every weather]).

### 2.1 Speed against the wind speed, and waking again [observed: campaign o32_tumble, 2026-11-25, observer check 94]

The same ten references and the far one, about every 0.18 s for one game hour (120 s at
TimeScale 30), with `ForceWeather HVDustStorm 1` (DATA wind speed 127) before
`coc Goodsprings`, so the cells loaded under the storm.

- **No faster in the storm.** 0x00178A67 rolled 654 units net in its first 6 s at a median
  126 units a second (90th percentile 182) towards about 319°, as under 50 in o27
  (643 units at 140–190 towards 308–332°). 0x00178A64 went out to 398 units by 20 s and
  back to 63 by 30 s on its slope; 0x00178A7F rocked in place (1,467 units of path for 13
  net); the other six moved under 15 units; the far one not at all.
- **They wake again, rarely.** All had settled by about 60 s. At about 106 s three at rest
  moved at the same moment (0x00178A64 about 117 units at up to 126 a second, 0x00178A7F
  12, 0x00178A7E, which had never moved, 5), with no actor within 3,000 units and the
  player 5,000–9,000 units off, standing still. One such wake in the hour, three at once,
  points to something global (a gust, a weather or cell update) rather than a nudge
  [hypothesis]. Not modelled: one event is too few to say how often or why.
- Not checked: that the forced weather was still HVDustStorm when the cells loaded.

## 3. What NVForever does

- A body whose collision object has the wind flag and is dynamic is put in the world
  awake, even with the clutter that starts asleep (physics.md §4.3.1).
- Each frame outdoors, for its first 3 s awake (`tumble::GUST`) such a body has its
  horizontal velocity drawn towards the wind's by at most 300 units a second squared
  (`tumble::WIND_ACCEL`, about the pull of gravity on a 25° slope); after that it is
  slowed by 50 units a second squared (`tumble::SETTLE`) until it rests and sleeps; a
  body asleep is left so, and one woken again (a shot, a push) has a new gust
  [hypothesis fitted to §2: a 200 units a second roller goes about 1,000 units, as the
  farthest one did]. Without the end of the gust, the first build's tumbleweeds rolled
  3,500–6,700 units in 30 s on Goodsprings' ground (smoke, 2026-11-06): a sphere rolls
  on for ever on rapier's terrain, which has no rolling resistance and none of the
  original's stones and scrub to lodge in.
- The wind: the trees' and grass's direction, now 120° from +x towards +y (330°
  clockwise from north, §2; it was 30° [chosen]), at 200 units a second in any weather
  with wind, whatever its DATA wind speed (`tumble::ROLL_SPEED`; §2.1: a dust storm's 127
  rolls them no faster than the usual 50) [hypothesis: 200 from the fastest under 50, and
  the same in every windy weather], eased in over the first unit of wind speed as a
  weather fades in from a still one (`tree::wind_vector`); none indoors and with a still
  weather. Until #1401's second part it was 4 units a second per unit of wind speed,
  508 under 127.
- The bodies are those in the loaded physics cells (3 × 3 around the player), so the cost
  stays bounded; the exit line `wind summary` gives how many responded, the most blown in
  a frame, the farthest one went and the most time a frame's blowing took.

## 4. Open

- What wakes tumbleweeds at rest without a load (§2.1: once in a game hour, three at
  once), and how often: a longer watch over several hours and weathers.
- Whether a still weather (DATA wind speed 0) rolls them at all.
- Caesar's throne carries the flag; its bodies are not dynamic, so nothing blows it.
- The tumbleweed pile effect (`effects\tumbleweedpile.nif`) is not placed in the masters.

## 5. Checks

- `cargo test -p fnv-phys the_wind_blows` (a flagged ball placed asleep starts awake and
  rolls downwind at about the wind's speed; an unflagged one is not blown).
- Smoke `wind-tumbleweeds`: Goodsprings at noon; the `wind summary` line has the
  tumbleweeds and one of them going far towards the north-west.
