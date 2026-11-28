# Spec note: experience, levels and the XP meter

Tags as in `nif.md`: `[documented]` cites a page or file, `[observed]` says what was
looked at, `[hypothesis]` is a reading that nothing has confirmed yet, `[design]` marks
what is NVForever's own, `[owner]` what the owner asked for. Rules:
`crates/fnv-game/src/experience.rs`; the meter: `engine/nvf-play/src/xp_meter.rs`; the
menu it opens: levelup.md.

## 1. Sources

- **The owner's screenshot** of the original's XP meter (2026-10-16, kept privately):
  the bar with "2" at its left, "3" at its right, the pointer at about nine tenths and
  "+54 XP" under its right end; and the owner's words: shown on the middle right side
  when the player earns experience, not shown at the level cap; the cap 30, and 5 more
  for each add-on, 50 in all; a sound for every gain, one for a level up, and a
  music-like one on the level-up screen.
- **The menu file** `menus\main\hud_main_menu.xml` (`Fallout - Misc.bsa`), its rectangle
  `XPMeter`; and the textures `textures\interface\hud\glow_xp_bar_over.dds` and
  `glow_xp_bar_pointer.dds` (`Fallout - Textures2.bsa`). Described in plain words; nothing
  is shipped.
- **The masters' game settings** [observed: `fnvtool esm decode`, FalloutNV.esm].
- **GECK wiki** Experience Settings, Entry Point, Projectile (the owner's export).
- **The masters' perks** [observed: their PERK records read by `GameData` from
  FalloutNV.esm and the five add-on masters, entry point effects listed by code].

## 2. What an enemy is worth

[documented: GECK wiki Experience Settings; observed: the values in FalloutNV.esm]
No actor record carries an experience value. A kill's reward comes from the target's
**level** (`crate::combat::Game::kill_xp`): the level picks a difficulty class, the first
whose `iXPLevelKill<NPC|Creature><Class>` it does not pass, and the class gives the
reward `iXPRewardKill<NPC|Opponent><Class>`.

| Class | NPC up to level | NPC reward | Creature up to level | Creature reward |
| --- | --- | --- | --- | --- |
| VeryEasy | 0 | 0 (default) | 1 | 1 |
| Easy | 1 | 10 (default) | 3 | 5 |
| Average | 7 | 20 (default) | 6 | 10 |
| Hard | 10 | 30 (default) | 9 | 25 |
| VeryHard | 13, and above | 50 (default) | 12, and above | 50 |

The level thresholds and the creature rewards are in FalloutNV.esm; the NPC rewards are
not, so the wiki's defaults hold. There is none when others did more than
`100 − iXPDeathRewardHealthThreshold` (40) % of the damage. An actor's level is its
record's, or the player's times its multiplier for a levelled (PC Level Mult) actor,
within its minimum and maximum. The difficulty multipliers `fDiffMultXP…` are all 1 in the
master.

A hired companion's (a teammate's) kill earns the player the same XP as the player's own,
and the player's and the companions' damage count together towards the share above
[owner, 2026-10-27; design: the shared share] (`Game::kill_earns_xp`). The player's own
death earns nothing, by the player's or a companion's hand (`Game::kill_xp`) [design: what the
original does there is not known]. The other rewards:

| Source | Settings (FalloutNV.esm; default) | Where (code) |
| --- | --- | --- |
| Picked lock | `iXPRewardPickLock<class>` 20, 30, 40, 50, 60 by the lock's level [observed; documented: wiki] | `lockpick::opened` |
| Hacked terminal | `iXPRewardHackComputer<class>` 20 to 60 [observed; documented: wiki]; the class is the first whose `iXPLevelHackComputer<class>` (0 to 4 by the wiki, not in the master) the terminal's difficulty code does not pass [documented: wiki] | `terminal::hacked`, on success only |
| Disarmed mine | `iMineDisarmExperience` 5 [observed]; given "on a successful deactivation" of a projectile that Can be Disabled [documented: wiki Projectile] | `Game::default_activation`, the player's Disarmed outcome, and the engine's hand activation (`nvf_play::script`, which skipped it until playtest list 7 item 23); trap scripts give theirs by `RewardXP` (traps.md §3) |
| Speech challenge | `iXPRewardSpeechChallenge<class>` by the line's DNAM (1 Very Easy … 5 Very Hard): only VeryEasy (5) is in any master, and the wiki lists none [observed]; a class without a setting gives nothing [design] | `Game::choose`, a line with the Speech Challenge flag |
| Place found | `iXPRewardDiscoverMapMarker` 10 [observed; documented: wiki] | the engine's compass |
| Scripts | `RewardXP <n>` (quests, challenges, dialogue result scripts) | `functions::player::reward_xp` |

The master also has `iXPRewardHackComputer` (20) without a class [observed]; nothing
known reads it and it is not used. **Skill checks in dialogue** (New Vegas's "[Speech
50]" lines: KNAM, no Speech Challenge flag) give no XP of their own: 638 of the 1,499 such
lines in the masters carry `RewardXP` in their result scripts [observed], so the data
gives that XP, and an engine award on top would count it twice [hypothesis that the
original gives none of its own]. Only three lines have the flag and a difficulty (all
Hard, a Fallout 3 left-over topic, dialogue.md §3.3), so the speech-challenge award, with
no Hard setting, gives nothing in the shipped data; the roll that could fail it is not
modelled.

### Perks and traits

Every gain goes through one function, `fnv_game::experience::award`, which applies the
player's perks' **Adjust Experience Points** entry point (code 9) [documented: GECK wiki
Entry Point: "Run when the player gains any experience; Modifies the experience gained",
example Swift Learner], so to **every source** above, scripts included. The masters'
entries of code 9, all Multiply [observed]:

| Perk | Factor | Conditions |
| --- | --- | --- |
| Swift Learner (3 ranks) | 1.1, 1.2, 1.3 (each rank's entry holds the whole bonus) | none |
| Skilled (trait, OldWorldBlues.esm) | 0.9 | none |
| Well Rested | 1.1 | one, on the player |
| Lessons Learned (LonesomeRoad.esm) | 1.25 to 1.5, one entry per level | one each, on the player |
| Logan's Loophole (trait, OldWorldBlues.esm) | 0 | one, on the player |

The rules of `perks::entry_point` hold (aid-items.md §10): only the highest rank owned
applies, the perk owner's conditions are evaluated on the player, entries apply in
priority order and multiply together (Swift Learner 3 with Skilled: × 1.17). The result
drops its fraction after rounding to thousandths (the factors are 32-bit: 1.3 is
1.2999999523, and 100 × 1.3 must be 130) [hypothesis for the fraction; design for the
rounding]. A loss (`RewardXP` with a negative number) is not changed [hypothesis]. Logged
`xp: +<n> XP from <source>[ (<before> before <perk EDIDs>)]`.

## 3. Levels

- **What a level needs** in all: (n − 1)((n − 2)/2 · `iXPBumpBase` + `iXPBase`)
  [documented: GECK wiki]; `iXPBumpBase` 150 [observed], `iXPBase` not in the master, 200
  by the wiki. Level 2 needs 200, 3 550, 4 1,050, 30 66,700, 50 186,200.
- **The cap**: `iMaxCharacterLevel`, 30 in FalloutNV.esm [observed], plus 5 for each of
  the four story add-ons in the load order (Dead Money, Honest Hearts, Old World Blues,
  Lonesome Road): 50 with all four [owner]. None of the add-ons overrides the setting or
  names it in a script [observed: the bytes of each master], so the raise is the
  program's; Gun Runners' Arsenal and the four packs add none [design: as the owner's
  count of 50].
- **Experience at the cap** still adds up (the Pip-Boy shows it) [hypothesis]; the meter
  does not show it and no level is earned.
- **Earning a level**: when the experience reaches the next level's total below the cap,
  the meter says LEVEL UP and plays its sound; the level-up menu opens 5 s later (the
  wait counted while the HUD is up) [owner, 2026-10-24: "play this sound then wait 5
  seconds before showing the level up screen"], once the player is also in the world, out of every menu and conversation, and no living actor is fighting
  the player (Combat) or searching for them (Alert, Lost: the sneak word's [CAUTION])
  with the player among its targets or an alarm the player raised [owner, 2026-10-16:
  not until the player is out of combat and not in a cautious state]
  (`fnv_game::experience::hunters`). Logged `xp: level up waits: <n> actors fighting or
  searching for the player` while it waits. Its opening raises the level by one (as
  `AdvancePCLevel`). With experience for more than one level, the next opens after the
  menu closes.

## 4. The meter: what the file says

[documented: hud_main_menu.xml] `XPMeter`, in the HUD's main colour, has no place or size
(set in code):
- `XPBracket`: the picture `glow_XP_bar_over.dds`, 512 × 256.
- `XPPointer`: `glow_XP_bar_pointer.dds`, 64 square, its x between `_x_min` 55 and
  `_x_max` 270 (start at 65).
- `XPLastLevel` at 70, 30, right-justified; `XPNextLevel` at 315, 30 (both the default
  font 1).
- `XPAmount` at 315, 90, right-justified, font 8 (`Glow_Futura_Caps_Large`).
- `XPLabel` at 350, 80, right-justified, font 7, the string `sStatsXP`.
- `XPLevelUp` at 360, 70, right-justified, font 8, the string `sLevelUp`.
- Neither string is in the masters [observed]: "XP" (the screenshot) and "LEVEL UP"
  [hypothesis] are the program's.

## 5. What the pictures and the screenshot show

[observed]
- **Bracket**: a long double line (rows 64–72) from x 7 to the upright at 364–367
  (rows 7–129); nine ticks on the line, 4 wide, centred 86.5 to 302.5, 27 apart, from about
  row 47 up to the line.
- **Pointer**: a triangle pointing down, its solid part x 21–41, rows 29–39, middle at 31.
  So `_x_min` + 31 = 86 is the first tick and `_x_max` + 31 = 301 the last: the pointer
  runs from this level (first tick) to the next (last tick).
- **Screenshot**, fitted to the bracket's ticks (1.7 pixels to the unit): the levels
  stand on y 49, "+54" on 118, "XP" on 103; the right ends at 70, 315 and 350 and the
  next level's start at 315 match the file. The pointer's tip is at y 55 (the picture at
  y 16).
- **Full-screen screenshot** (owner, 2026-10-20, 1920 × 1080, 1.125 pixels to the unit of
  a 960-unit-high view): the bracket's line at y 805 and its upright at x 1862, so the
  upright 52 units from the right edge and the bracket's bottom 182 units over the view's
  bottom, 25 over the action points' piece.

## 6. How NVForever draws it

- Just above the action points: the upright 52 units from the right edge, the bracket's
  bottom 182 over the view's bottom [observed: the full-screen screenshot, §5].
- The pointer's tip on the ticks' tops (row 46, the ticks from 47), not among them
  [owner, 2026-10-20].
- [observed: private jobs hud_xp_limbs, x1_reward10, x1b_later; hud_xp_limbs2,
  x9_level_up_long] On a gain the meter fades in about 2 s, holds about 2 s while the
  pointer glides, fades out about 2 s (about 6.4 s across the captured sequence).
  The pointer starts moving once alpha is full. [hypothesis] Fit with linear glide
  over 2 s; keep the confirmed pointer range 55-270. History: fade 0.25 s, hold 3 s,
  immediate eased slide 0.6 s [hypothesis], replaced 2026-10-29.
- [observed: private job hud_xp_limbs2, x9_level_up_long] After a level-reaching gain,
  LEVEL UP appears alone after the meter has faded away, about 7.2-7.4 s after the gain;
  it rises within 0.34 s, stays full about 1.3 s, then fades. [hypothesis] Fit with a
  1.2 s gap after the six-second meter and symmetric 0.34 s notice fades. History:
  LEVEL UP replaced +n XP while the meter showed [hypothesis].
- [design] Gains while shown retain the existing accumulation/restart behaviour.
  [observed: private job hud_xp_limbs, x2_reward25, x2b_reward5_on_top] A second +5 kept
  the first +25 and did not restart the original's fade. [hypothesis: open] Not repeated
  in round 2; capture again before changing the concurrent-gain rule.
- Not at the level cap [owner]. Hidden, its time held, while the HUD is (menus,
  conversations).
- **Sounds** [owner: a sound for each gain, one for a level up, a music-like one on the
  level-up screen; observed: the records and their files' lengths]: each gain
  `UIPopUpExperienceUp` (`ui_popup_experienceup.wav`, 3.6 s); LEVEL UP (the bar filled,
  `FADE + SLIDE` = 4.0 s after the gain) `UILevelUpText` (`ui_leveluptext.wav`, 4.9 s);
  the level-up menu opening `MUSSuccess` (`fx\mus\mus_success.wav`, the archives' `.ogg`;
  SOUN 0x00078594) [owner, 2026-10-24: "when the xp bar is filled it should play the
  ui_leveluptext.wav", and mus_success on the level-up screen; observed: the records].
  With a second level earned at once (or when no bar is shown, e.g. loading a save already
  due), its sound plays and the wait starts once the meter is idle (after the first menu
  closes) [design]. When `No XP Meter in Combat` defers showing gains in combat, the
  level-up sound waits for combat to end with the meter [design]. The short `UILevelUp`
  (`ui_levelup.wav`, 0.9 s) is not played.
- [owner, 2026-10-24; observed: private job hud_xp_limbs2, x9_level_up_long; design: #3438]
  Keep `UILevelUpText` at the filled pointer (4.0 s into the meter) and set
  `LEVEL_UP_DELAY = 5.35 s` after the sound (previously 5.0 s), so the level-up menu opens
  at `9.35 s` after the gain — matching the original's measured `9.35 s` menu opening
  (`x9_level_up_long`, about 2 s after `LEVEL UP` appears; the original's audio moment was
  not read) and letting both `ui_leveluptext.wav` (ending at 8.9 s) and the `LEVEL UP`
  notice fade (ending at 9.18 s) finish cleanly without interruption.
- Without the game's pictures the bracket and pointer are plain blocks.
- Log: `xp: level cap <n> (<k> story add-ons)`; `xp meter: +<n> XP (<xp> in all): level
  <l>, <p> % to <l+1>[, next level earned]`; `xp meter: sound <SOUN>`; `xp meter: LEVEL UP
  (<l> -> <l+1>)`; `xp meter: +<n> XP at the level cap <max>: not shown`; `xp: level up
  taken: level <l> (cap <max>)`.

## 7. Checks

- Unit tests: the totals and the cap (`fnv_game::experience`), the meter's model (gains,
  adding up, LEVEL UP, the cap, resets, the pointer's bounds) (`nvf_play::xp_meter`).
- Smoke test `xp-meter` (tools/smoke/scenarios.toml): `rewardxp 54` then `rewardxp 150`
  at the console in Doc Mitchell's house; the meter, LEVEL UP, the three sounds, and the
  level-up menu opening by itself at level 2; screenshots `xp_meter.png`,
  `xp_level_up.png`, `xp_levelup_menu.png`.
- Synthetic tests (`crates/fnv-game/tests/experience.rs`): Swift Learner by rank, with
  Skilled, the fraction, a loss; RewardXP through the perks; a hacked terminal's XP by
  class and none for a locked-out one; a disarmed mine's 5. In
  `crates/fnv-game/tests/dialogue.rs`: a skill line gives none, a Very Easy speech
  challenge 5, a Hard one nothing.
- Smoke test `xp-perks`: `addperk SwiftLearner` then `rewardxp 100` gives 110, and the
  meter shows it.

## 8. Not built, or open

- Speech challenges of the classes Easy to Very Hard give nothing: no master or wiki page
  gives their settings (§2). Whether the original awards anything of its own for New
  Vegas skill checks is [hypothesis: no, as their scripts carry RewardXP].
- The mine's Explosives check and the chance to set it off are not built (disarming always
  succeeds), and the projectile's Can be Disabled flag is not read.
- [observed: private job hud_xp_limbs2, x9_level_up_long] The meter's timing is now
  measured (§6); exact interpolation and concurrent gains remain open [hypothesis].

## 9. What would settle the meter's numbers (historical request to the owner)

[observed: private job hud_xp_limbs2, x9_level_up_long] The 2026-10-29 capture settles
the single-gain timing in §6; the historical hypotheses and suggested checks below
remain for provenance, not the current drawing rule [design].

The place is settled by the full-screen screenshot (§5). The rest of this section was
written before it. `engine/nvf-play/src/xp_meter.rs` used five numbers that no file gives [hypothesis]:
`HOLD` 3 s (how long the meter stays after the last gain), `SLIDE` 0.6 s (the pointer's
move), `FADE` 0.25 s (in and out; the HUD's fade, documented for other HUD parts), the
bar's line at the middle of the screen's height, and the upright 40 units from the right
edge. The owner's screenshot (§1) is cropped to the meter, so it fixes the meter's own
layout but not where it sits on the screen, and a still cannot time anything. They stay
as they are until one of these comes:

1. **A full-screen screenshot** of the original with the meter showing, at a known
   resolution (say 1920 × 1080, HUD scale and safe-zone settings untouched): it gives the
   bar's height on the screen and its distance from the right edge.
2. **A short video** (30 or 60 fps, full screen, game sound on) of: picking a Very Easy
   lock or killing a weak creature, waiting until the meter is gone, then two gains a
   second or two apart (two kills, or `rewardxp 20` twice at the console), and a gain that
   earns a level. From it: the fade in and out (frames from first to full and back), the
   pointer's slide and its easing, how long the meter stays after the last gain and
   whether a second gain restarts that time, what LEVEL UP looks like and when the
   UILevelUp sound plays against it, and how long after the meter goes the level-up menu
   opens.
3. Any number from the original's own files the owner comes across (an INI setting or
   menu file naming the meter's time).

The same video, with an enemy still searching, would also show whether the level-up menu
waits out [CAUTION] as the owner described (§3).
