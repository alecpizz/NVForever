# SUP NVSE, batch 1 (2026-11-01, #1008)

Rounding, points and angles, numbers in words, the Cyrillic test, the actor finders and
the topic getters, in `crates/fnv-game/src/nvse_sup1.rs`. Source: the GECK wiki export
[documented: the page named]. Checked in the original on 2026-11-08 with the `sup1`
probe set (`tools/capture/jip_probes.py`, supNVSE in the probe game, the airport save;
lab scenario L5.ext.sup1): a settled value is tagged `[observed: sup1 <row>]`; where a
page leaves a value open and no row settled it the choice is tagged `[hypothesis]`. Tests: the unit tests in `nvse_sup1.rs` (the pages' examples) and
`crates/fnv-game/tests/sup1.rs` (called from scripts on a synthetic plugin).

| Function | NVForever |
|---|---|
| Round `x` | to the nearest whole number [documented: 37.4 is 37, 37.8 is 38], a half away from zero [observed: sup1 round.halves, 2.5 is 3, -2.5 is -3]; -0.4 gives -0, which the game's `%g` writes "-0" and NVForever's formatter "0" [observed: sup1 round.other; the formatter is shared, `fnv_script::vm::printf_number`, and not changed here] |
| RoundAlt `x places round` | rounded when `round` is set, else cut [documented: every example on the page]. Computed as [observed: sup1 roundalt.page, .places, .negative, .halves]: the scale 10^places as a 32-bit integer, not held to 0-4 (5 places keep 37.12346; 9 places overflow the cut integer to INT_MIN, -2.14748359; a negative count makes the scale 0 and the result 0/0, a NaN, which the game writes "-nan(ind)" and NVForever's formatter "nan"); the product as a 32-bit float, 0.5 added when rounding, cut toward zero (-2.5 rounds to -2, -37.66 to -37); the result a 32-bit float (37.6 reads 37.599998). A fractional place count or round flag (2.7, 2) did not compile in the game [observed: sup1 roundalt.flag2] |
| IsNumberNAN `[x]` | 1 for a NaN; with no number it returns a NaN [documented: both forms] |
| GetDistanceBetweenPoints `x1 y1 z1 x2 y2 z2` | the straight distance [documented] |
| GetHeadingAngleBetweenPoints `x y angleZ tx ty`, GetHeadingAngleAlt `x y angleZ ref` | degrees from the facing to the point (or the reference's position), clockwise, as GetHeadingAngle [documented: "equivalent to GetHeadingAngle"; the angle in degrees as GetAngle Z gives it; observed: sup1 heading.points.rad, a facing of 1.5708 is degrees]. One turn is taken off a difference of 180 or more, or added below -180, once: due behind is -180, a facing of 765 leaves -405 [observed: sup1 heading.points.s, .n.765, .n.m270]. **Eight of these calls on one script line crashed the original** (the game closed); one call a line runs [observed: the first sup1 run, 2026-11-08] |
| NumberToWords `n` | English words without "and" or hyphens [documented: the three examples]; "zero", "negative" before a negative number [observed: sup1 words.0, words.m15 "negative fifteen"]; up to 2147483647 [observed: sup1 words.2147483647]. A fractional literal (7.9) does not compile [observed: sup1 words.7p9] |
| StringContainsCyrillic `text` | 1 for Cyrillic text [documented]: "Привет" in a script is 1, "café" 0 [observed: sup1 cyr.utf8, cyr.accent]. A character made with `%a` from 207, 233 or 192 (Cyrillic letters in Windows-1251) is 0, so the original reads UTF-8, as NVForever's strings are [observed: sup1 cyr.code.*]. `%a 1055` is 0 in the game and 1 here: NVForever's `%a` makes U+041F where the game makes one byte; that is the shared formatter, not this function, and is left [observed: sup1 cyr.code.1055] |
| GetSUPVersion | -1 [documented: "will always return -1"] |
| IsPlayerOverencumbered | carrying more than the carry weight, as ShowOff's GetIsPlayerOverencumbered [documented: the page points there] |
| FindClosestActor, FindRandomActor `x y z [dead] [teammates] [distance]` | among the actors in the loaded cells, not the player: the closest to the point, or a random one; the dead only with `dead`, the player's teammates only with `teammates`, within `distance` when one above 0 is given; none (0) when there is none [documented: the pages and their notes on the loaded cells and the player] |
| FindClosestActorFromRef, FindRandomActorFromRef `[dead] [teammates] [distance]` | the same around the calling reference, which is not its own find [documented; hypothesis: not itself]. The sup1 finder rows differ from the headless run by setup only: the airport save's creatures are dead (killall) and the player stands elsewhere, so the game finds no living actor and a corpse 1121 units off [observed: sup1 closest.*, fromref.*, random.*] |
| TopicGetType `topic` | the topic's type, 0 Topic to 7 Radio (DIAL DATA) [documented: the page's table; observed: sup1 type.*]; 0 for a form that is not a topic [observed: sup1 type.stimpak] |
| TopicGetAllTopicInfos `topic [quest]` | the topic's lines in topic order [documented]. With a quest, the original gives the lines of the topic's first quest (its first QSTI) whichever quest is named: GREETING with VFreeformGoodsprings gives the 25 lines of 139B8C, GOODBYE the one line of 133046, Sunny's topic with VCG02 still its 104C66 line; a form that is not a quest gives none [observed: sup1 infos.greeting.quest, infos.goodbye.quest, infos.creatures.otherquest, infos.creatures.notquest] |
| TopicInfoGetAllChoices, TopicInfoGetChoiceCount `info`, TopicInfoHasChoice `info topic` | the line's choices (TCLT) [documented; observed: sup1 choices.greet, has.*]; an empty array for a line without choices or a form that is not a line, but -1 from the count and from HasChoice for them [observed: sup1 choices.bye, count.bye, count.notinfo, has.bye] |

Not in this batch: the topic and line setters (TopicSetPriority, TopicSetFlags,
TopicInfoAddChoice, …ReplaceChoice, …SetFlagsHigh/Low, …SetPrompt, …SetSaidOnce): built in
batch 2 (ext-sup2.md), with the dialogue reading the edits.
GetCalculatedPos and GetCalculatedPosAlt write into script variables. KillAll2 is
deprecated on its page as not working. The file, INI, Excel, JSON, clipboard, screenshot,
web and window functions touch the machine outside the game and need a policy first
(NVForever never writes into the game folder or `Documents\My Games\FalloutNV`).
