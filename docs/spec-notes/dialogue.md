# Spec note: dialogue topics, lines and conversations

Clean-room notes for Layer 6 dialogue, steps D1 and D2 of docs/design/dialogue.md
(2026-10-10). Code: `crates/fnv-game/src/data.rs` (`TopicDef`, `InfoDef`),
`crates/fnv-game/src/dialogue.rs` (conversations), `fnvtool dialogue`.

Sources and tags:
- **[documented: xEdit …]**: the record definitions of xEdit's `wbDefinitionsFNV.pas`
  (DIAL and INFO records, re-read 2026-10-10), as carried by `schemas/fnv/gameplay.json`.
- **[documented: page, owner export]**: a page of the owner's private GECK wiki export
  (`C:\fnv-private\geckwiki\`, never committed). Everything here is our own summary.
- **[owner statement 2026-10-10]**: the owner's rulings from long GECK use and play
  (which line is picked; then, the same day, that a conversation pauses the game, who
  can say a line, follow-ups and Random pools); not yet captured in game.
- **[observed: …]**: read from the ten masters (FalloutNV, the four add-ons,
  GunRunnersArsenal and the four packs), with the command that shows it. "The summary"
  means the `summary` of `fnvtool dialogue topics <Data> <ten masters> --schema
  schemas/fnv` or of `fnvtool dialogue lines …`. "Scratch parse" means a throw-away
  script over the DIAL groups of the same files (2026-10-10), not a committed tool.
- **[hypothesis]**: none of the above; each one is a capture question in §7.

Earlier notes this builds on: quests-dialogue.md §3–4 (the documented rules),
scripts.md §7.7 (SayTo and Say lines).

## 1. The topic record (DIAL)

A topic is a DIAL record. Its lines are the INFO records of the "topic children" group
(group type 7, labelled with the topic's FormID) that follows it
[documented: xEdit group types; observed: every INFO of the masters sits in such a
group, the summary's `with_topic` = `lines`].

### 1.1 Subrecords

| Sub | xEdit name | Layout | Meaning | Read into |
|---|---|---|---|---|
| EDID | Editor ID | string | e.g. `GREETING` | `TopicDef::edid` |
| QSTI | Added Quest → Quest | FormID (QUST), repeated | a quest that has lines in this topic [documented: xEdit DIAL "Added Quests"] | `quests` |
| INFC | Shared Info → Info Connection | FormID (INFO), after a QSTI | see §1.2 | `shared_infos`, `GameData::shared_lines` |
| INFX | Shared Info → Info Index | signed 32-bit, after its INFC | see §1.2 | `shared_infos`, `GameData::shared_lines` |
| QSTR | Removed Quest | FormID (QUST) | xEdit notes that New Vegas has none; none in the masters [observed: the summary's `subrecords`] | `removed_quests` |
| FULL | Name | string | the text the player sees for the topic when the line has no prompt [documented: Category:Dialogue, "Prompt"] | `name` |
| PNAM | Priority | float, xEdit default 50 | the topic's place in the list offered to the player; higher is nearer the top [documented: Category:Dialogue, "Priority"] | `priority` |
| TDUM | Dumb Response | string | the player's line as a dim character says it ("Peace." is "Pizza.") [observed: the texts]; shown for a player at or below `iDialogueDummySpeakThisIntOrBelow` (3) Intelligence since 2026-10-25 [hypothesis: the setting's name] (`Game::player_dim`) | `dumb_response` |
| DATA | Data | type (u8), flags (u8, optional) | type: 0 Topic, 1 Conversation, 2 Combat, 3 Persuasion, 4 Detection, 5 Service, 6 Miscellaneous, 7 Radio (the quest window's tabs [documented: Category:Dialogue]); flags: bit 0 Rumors ("not used" [documented: Category:Dialogue]), bit 1 Top-level | `topic_type`, `flags` |
| INOM, INOA | INFO Order | FormID arrays | xEdit's own bookkeeping; absent in the masters | ignored |

[documented: xEdit wbDefinitionsFNV.pas DIAL record, for every row's name and layout.]
A topic defined by several plugins is read from its winning record (the last plugin's);
115 topics are defined more than once [observed: the summary's
`defined_in_several_plugins`]. `TopicDef::order` is where the topic is first defined
(plugin, record ordinal).

### 1.2 INFC and INFX ("Shared Infos")

xEdit lists INFC/INFX pairs under each added quest as "Shared Infos" ("Info
Connection", "Info Index") and marks them as not mattering for conflicts; it also notes
a few records that carry such pairs with no quest and absent FormIDs, "probably error in
GECK" [documented: xEdit DIAL record]. What the game does with them is **not
documented** in our sources. What the data shows:

- 2,435 topics carry 3,224 pairs; 18 pairs follow no QSTI; 3,206 name an existing line,
  and 3,031 of those name a line **stored under another topic** [observed: the
  summary's `shared_infos`].
- The line a pair names belongs to the quest the pair is listed under in all but one
  case; it is not explained by the line's choices, link-from or follow-up links (63
  of 3,906 pairs coincide with a choice or link-from link, 7 with a follow-up)
  [observed: scratch parse].
- Example: GREETING lists four lines that are stored under the topic `VMS07Greeting`,
  with indices 11–14 [observed: scratch parse].

- Topics with no line of their own are the common case: their pairs' indices run 0,
  1, 2 … (0 for 2,086 pairs, 1 for 433, 2 for 80 …); 332 pairs belong to topics that
  have lines of their own. The topic the line is stored under has the same name (FULL)
  in 756 of the 3,206 pairs [observed: scratch survey over the ten masters, 2026-10-11].
- Example: Red Lucy's choice "What's this place?" (`VMS38VMS38RedLucyTopic047`) has no
  line; its one pair names the line of `VMS38VMS38RedLucyTopic004`, a topic of the same
  name, whose responses answer that question [observed: `fnvtool dialogue topics
  --topic …`].
- Of the 7,325 player lines (type Topic) with choices, 2,499 link to at least one
  topic without a line of its own, and in 257 every choice is such a topic. Counting
  shared lines, 59 and 7 remain [observed: the scratch survey; the real-data test
  asserts 7,325 / 257 / 7].

[hypothesis] Info Index is a position among the **listing quest's** lines for this
topic, rather than among all the topic's lines. xEdit's per-added-quest grouping is
documented above; the behavioral interpretation is not established vanilla behavior.
The supplied data survey in [issue #1895](https://github.com/LootJunkieAzza/NVForever/issues/1895)
reports HELLO sharing 223 pairs from 14 quests, with indices restarting per quest;
38 indices are used by multiple quests. Its Powder Ganger indices 5–24 follow their
GREETING order: the eligible Random lines `00151A7B`, `00151A7A`, `0015694C`,
`0015694D` precede the plain `00151A7C`. Topic-wide insertion instead placed the plain
line first [observed: fnvtool dialogue topics/lines over FalloutNV.esm, issue #1895].

[design: `crates/fnv-game/src/data.rs`, `shared_lines` / `GameData::lines_tried`]
The topic's own lines keep their stored order (§3.1); no line of them moves. Each
listing QSTI quest's list is its own lines of this topic, in stored order, with its
shared lines inserted at their Info Index in index order (a negative, missing or
out-of-range index puts the line last in that list). In the topic's list a shared line
goes just before the quest's own line that follows it in the quest's list, or right
after the quest's last own line when none follows; shared lines that meet at one place
keep index order, then QSTI order. A quest with no own line in the topic (Powder
Gangers in HELLO) has its shared lines appended after all own lines, quests in QSTI
order, each quest's lines in index order, so the Powder Ganger Random pool still comes
before their plain line. The list then gets the stable priority sort of §4, using each
INFO's **owning** quest, even when that differs from the listing quest. Since the own
lines never move, own lines of equal priority are tried in the same order as in a
topic without shared lines (`GameData::lines_by_priority`, "within one priority the
stored order", §4), also when two quests' own lines are interleaved. Hash-map
iteration never decides the order. Missing INFOs are skipped; an INFO already present
is not inserted again (the first pair in stable index order wins). If a valid pair
lacks QSTI, the entire topic retains the previous topic-wide merge as a malformed-data
fallback, including equal-index insertion order. This avoids guessing a listing quest
from INFO ownership or changing the malformed topic's interleaving.

[design: `crates/fnv-game/src/dialogue.rs`, `Game::select_line`, `Game::say_line`]
A shared line counts as a line of the listing topic and is said under that topic
(the event's `topic`); it retains its own conditions, Choices, Add Topics and result
scripts. GREETING's four `VMS07Greeting` shared lines at indices 11–14 therefore remain
available through GREETING. Synthetic and optional real-data regressions live in
`crates/fnv-game/tests/dialogue.rs` and `dialogue_data.rs`. Capture question 9 stays
open for the in-game check.

### 1.3 Counts [observed: the summary]

- 21,298 topics. By type: Topic 19,285, Conversation 1,724, Radio 219, Combat 43,
  Detection 12, Service 9, Miscellaneous 3, Persuasion 3.
- Top-level: 2,538 (2,536 of type Topic, 2 of type Conversation); the runtime offers
  the 2,536. Rumors: 1.
- With a name: 21,261. With a Dumb Response: 135.
- Priority: 20,073 at 50; the rest between 5 and 100 (190 at 100, 127 at 90, 97 at 80,
  95 at 60, 86 at 95).
- 14,608 topics list quests (15,853 entries). 9,304 topics have no line at all; 9,679
  have one, 1,460 two, 36 more than fifty. GREETING has 5,579 lines.
- Subrecords: of 21,560 DIAL records in the ten files every subrecord signature is read
  (`subrecords.DIAL.unread` is empty).

## 2. The line record (INFO)

### 2.1 Subrecords, in the order they appear in a record

| Sub | xEdit name | Layout | Meaning | Read into |
|---|---|---|---|---|
| DATA | Data | type (u8), next speaker (u8), flags 1 (u8), flags 2 (u8, optional) | type as DIAL's; next speaker 0 Target, 1 Self, 2 Either; flags below | `info_type`, `next_speaker`, `flags` |
| QSTI | Quest | FormID (QUST) | the quest the line belongs to | `quest` |
| TPIC | Previous Topic | FormID (DIAL) | xEdit: ignored by the editor for master files; absent in the masters | `previous_topic` |
| PNAM | Previous INFO | FormID (INFO) or 0 | the line above this one in the editor's list [documented: Category:Dialogue, "Edit Response Order"]; see §3.1 | `has_previous`, `previous` |
| NAME | Add Topics → Topic | FormID (DIAL), repeated | topics the line adds | `add_topics` |
| TRDT | Response Data | emotion type (u32), emotion value (i32), 4 unused, response number (u8), 3 unused, sound (FormID SOUN, optional), use emotion animation (u8, optional), 3 unused | one per response; emotion 0 Neutral, 1 Anger, 2 Disgust, 3 Fear, 4 Sad, 5 Happy, 6 Surprise, 7 Pained; the sound plays instead of the voice [documented: Category:Dialogue, "Sound"] | `Response` |
| NAM1 | Response Text | string | the subtitle; text in curly brackets is an acting note and is not shown [documented: Category:Dialogue, Notes] | `Response::text`, `response_chars` |
| NAM2 | Script Notes | string | acting notes | `Response::notes` |
| NAM3 | Edits | string | an editor comment; empty in all 37,447 cases | ignored |
| SNAM, LNAM | Speaker / Listener Animation | FormID (IDLE) | idles played during the response | `speaker_idle`, `listener_idle` |
| CTDA | Conditions | common condition | who can say the line and when (quests-dialogue.md §3) | `conditions` |
| TCLT | Link To → Topic | FormID (DIAL), repeated | the editor's "Choices": topics offered after this line | `choices` |
| TCLF | Link From → Topic | FormID (DIAL), repeated | see §3.5 | `link_from` |
| TCFU | Follow Up → Info | FormID (INFO), repeated | see §3.4 | `follow_ups` |
| SCHR … | Script (Begin) | embedded script | the begin result script | `begin` |
| NEXT | Marker | empty | separates the two scripts | – |
| SCHR … | Script (End) | embedded script | the end result script | `end` |
| SNDD | Unused | FormID (SOUN) | absent in the masters | ignored |
| RNAM | Prompt | string | shown to the player instead of the topic's name [documented: Category:Dialogue] | `prompt` |
| ANAM | Speaker | FormID (NPC_, CREA) | only this actor says the line; works like a GetIsID condition [documented: Category:Dialogue] | `speaker` |
| KNAM | ActorValue/Perk | FormID (AVIF, PERK) | the skill or perk shown with the choice; see §3.3 | `challenge_form` |
| DNAM | Speech Challenge | u32: 0 none, 1 Very Easy … 5 Very Hard | the difficulty of a Fallout 3 style speech challenge | `challenge_difficulty` |

[documented: xEdit wbDefinitionsFNV.pas INFO record, for names, layouts and order;
observed: the masters' records follow that order, scratch parse.] A line defined by
several plugins (37 lines) is read from its winning record and keeps the place of its
first definition.

Flags (`InfoDef::flags`: flags 1 in bits 0–7, flags 2 in bits 8–15, the combined value
the wiki's GetDialogueInfoFlags page lists) [documented: xEdit for the names;
Category:Dialogue for the meanings]:

| Bit | Name | Meaning | Lines |
|---|---|---|---|
| 0x0001 | Goodbye | the actor leaves dialogue after the line | 9,828 |
| 0x0002 | Random | §4 rule 3 | 8,001 |
| 0x0004 | Say Once | the line is said only once; in the one case checked another actor sharing it did not say it again (question 13) | 4,434 |
| 0x0008 | Run Immediately | the results run when the conversation is put together, not when the line is spoken | 765 |
| 0x0010 | Info Refusal | the player's prompt is not shaded after use | 80 |
| 0x0020 | Random End | closes a Random stack | 1,143 |
| 0x0040 | Run for Rumors | "not used" | 0 |
| 0x0080 | Speech Challenge | a speech challenge (§3.3) | 3 |
| 0x0100 | Say Once a Day | once per actor per `fSayOncePerDayInfoTimer` (12 game hours) | 194 |
| 0x0200 | Always Darken | the prompt is always shaded | 6 |
| 0x0400, 0x0800 | Unknown 2, 3 | not documented | 0 |
| 0x1000 | Low Intelligence | said to a dim player only (Intelligence at or below `iDialogueDummySpeakThisIntOrBelow`, 3 [observed]) since 2026-10-25 [hypothesis: the lines' acting notes say "{Dummy}", "realizing the player is not very smart", observed] | 33 |
| 0x2000 | High Intelligence | said only to a player who is not dim, as above [hypothesis: their acting notes say "{Non-Dummy}", observed] | 22 |

### 2.2 Counts [observed: the summary of `fnvtool dialogue lines`]

- 28,896 lines (28,933 INFO records in the ten files; 37 are overridden). By the plugin
  that first defines them: FalloutNV 23,247, DeadMoney 2,126, OldWorldBlues 1,620,
  HonestHearts 1,236, LonesomeRoad 667.
- By type: Topic 21,831, Conversation 2,603, Combat 2,300, Detection 1,349, Radio 769,
  Miscellaneous 39, Service 5. 954 lines have another type than their topic (613 Topic
  lines in Conversation topics, 167 Combat lines in Detection topics, 74 Conversation
  lines in Topic topics, 42 Topic and 40 Conversation lines in Radio topics, 16
  Detection lines in Combat topics, 2 Radio lines in Topic topics).
- Every line names a quest. 15,683 have a fixed speaker; 27,813 have conditions
  (69,277 in all); 3,336 have a begin script with code and 5,303 an end script.
- Responses: 37,403. Per line: none 9, one 23,531, two 3,205, three 1,578, four 375,
  five 127, six to nine 54, ten or more 17. 228 responses name a sound, 1,684 a speaker
  idle, 1 a listener idle, 1,747 have acting notes, 40 an empty text. Emotions: Neutral
  28,608, Anger 2,660, Happy 2,158, Disgust 973, Surprise 947, Sad 859, Fear 773,
  Pained 425.
- Response numbers: 874 lines are not numbered 1…n, and 583 store their responses out
  of ascending number order. The runtime says responses in number order
  [documented: xEdit "Response number" for the field; hypothesis that the number
  decides where it differs from the stored order].
- 1,707 lines have a prompt. 8,114 have choices (28,215 links); 924 have link-from
  topics (1,293 links); 1,759 have follow-ups (2,477 links); 472 add topics (827
  links). 15 lines have both follow-ups and choices.
- 1,499 lines name an actor value or perk (1,391 AVIF, 108 PERK); 3 have a difficulty
  (all "Hard"), the same 3 that have the Speech Challenge flag.
- Next speaker: Target 28,685, Self 211.
- Record header flag 0x2000 is set on 7,631 lines and nothing else is; what it means on
  an INFO is not documented, and it is not used.
- Of 28,933 INFO records every subrecord signature is read or deliberately ignored
  (NAM3); `subrecords.INFO.unread` is empty.

## 3. What the data shows about structure

### 3.1 The stored order of a topic's lines

The editor keeps a topic's lines in a list and evaluates them from the top; moving a
line writes its "Previous INFO" (PNAM) [documented: Category:Dialogue, "Edit Response
Order"].

- FalloutNV.esm's 23,247 lines have **no PNAM**; every line of the add-ons has one
  [observed: the summary's `previous_info`: `absent` 23,210 winning lines; scratch
  parse per plugin].
- Where PNAM exists it describes the order **within one quest** inside the topic: 3,039
  lines have PNAM 0 and are the first line of their quest in the topic; 2,638 name the
  line stored just before them among their quest's lines; 9 differ (6 have 0 without
  being first, 2 name another line of their quest, 1 names a line elsewhere)
  [observed: the summary's `previous_info`].
- Two add-on lines that override FalloutNV lines name as previous the line stored before
  them in FalloutNV.esm [observed: scratch parse, lines 0x00120F26 and 0x0014F170].

So file order agrees with the previous-info links in 5,677 of 5,686 cases, and is the
only order FalloutNV.esm has. **The runtime's stored order of a topic** is: the lines in
the file order of the plugin that first defines each, plugins in load order, then each
line a later plugin adds put straight behind the line its winning PNAM names, when that
line comes from an earlier plugin or was itself moved so (`GameData::topics`,
`data::follow_previous`) [observed as above for one quest's lines; hypothesis for the 9
exceptions, for how lines of different quests interleave in the editor's list, which §4
makes mostly irrelevant, and for added lines with PNAM 0 (left at the end), two added
lines naming one line (the later in file order first) and broken chains (file order)].
In the ten masters this moves no line [observed: `lines_moved_by_previous` is 0,
crates/fnv-game/tests/dialogue_data.rs]; it matters for mods that add lines to a
master's topic.

### 3.2 Lines of several quests in one topic

Only 102 topics have lines of more than one quest. In 99 of them one actor could in
principle say lines of quests with different priorities (same fixed speaker, or no
fixed speaker on one side), and in 76 of quests with the same priority (74 both)
[observed: the summary's `quest_mix`]. GREETING alone has lines of 228 quests; the
others are mostly the player-action topics (PLAYERFIREWEAPON and the like) and shared
service topics. So rule 2 of §4 (equal priority) matters for 76 topics, GREETING
foremost.

### 3.3 Skill and perk lines (KNAM)

- The Speech Challenge flag with a difficulty appears on 3 lines (one Fallout 3
  left-over topic). New Vegas's skill checks are the 1,499 lines with KNAM and no flag
  [observed: the summary].
- They come as pairs in one topic: of the topics that have such lines, 488 have exactly
  two lines, both with KNAM, and 380 have one line; the two lines of a pair differ in
  one condition, `GetActorValue <skill> >= N` against `< N`, run on the target (the
  player) [observed: scratch parse; e.g. topic 0x00100A60].
- The wiki says the field controls only what is shown; whether the line is available
  is up to its conditions; New Vegas shows the needed level, or "have/need" when the
  player falls short and a second line exists for that [documented: Category:Dialogue,
  "AV/Perk"].

Runtime: the pass or fail line is simply the line of the topic the actor can say
(§4); the choice carries the tag (`Challenge`: the form, the difficulty, and the needed
and current level taken from the line's first GetActorValue-family condition run on
the target) [hypothesis for where the game takes the two numbers from]. Choosing a
Speech check's line counts Speech Successes when the player meets the level and Speech
Failures otherwise; other skills and perks count nothing [hypothesis; misc-stats.md §3,
observer-checks.md item 70].

**Fallout 3 style percentage speech challenges (`SPEECH_CHALLENGE` flag + DNAM 1–5, #3406):**
A line carrying `info_flag::SPEECH_CHALLENGE` (0x0080) and a DNAM difficulty code 1–5
(`Very Easy` … `Very Hard`), with or without `KNAM`, is rolled against the player's
Speech skill, Charisma, Karma, weapon state, and the speaker's disposition and
responsibility [documented: GECK wiki Speech Formula (Fallout 3) and Complete List of
GameSettings; observed: `iSpeechChallengeDifficultyVeryEasy` 10, `Easy` 40, `Average`
55, `Hard` 70, `VeryHard` 80, `fSpeechChallengeDispositionBase` 30.0,
`fSpeechChallengeDispositionMultiplier` 1.0, `fSpeechChallengeMultiplier` 2.0,
`fSpeechAutoSuccessThreshold` 0.9, `fDispTargetCharMult` 10.0 in FalloutNV.esm;
`fSpeechChallengeSpeechBase` 50.0, `fDispTargetCharOffset` 5.0, `fDispTargetCharBase`
0.0, `fDispActorRespMult` 1.0, `fDispActorRespBase` −50.0, `fDispTargetKarmaMult` 0.002,
`fDispTargetWeaponBase` −10.0 from GECK wiki]:

- `Pass Chance = Disposition Value × Speech Value × Difficulty Value`, clamped to `0.0..=1.0`,
  with each factor floored at `0.0` before multiplication [design: avoids negative × negative].
- When `SetDisposition`/`ModDisposition` overrode the actor's disposition toward the
  player, that value replaces `Actor Disposition + Charisma Value` so Charisma is not
  counted twice [design, matching `functions::actor::get_disposition`].
- When `Pass Chance > fSpeechAutoSuccessThreshold` (default 0.9), the check always passes
  [documented: GECK wiki Speech Formula (Fallout 3)] and reports `1.0` (`100%`) [hypothesis].
- Choice display: the choice carries `Challenge::chance` even when `KNAM` is absent
  (resolving `AVSpeech` 0x000004BB when present, else `"Speech"` [design]) and `nvf-play`
  shows `[Speech NN%]` [documented: GECK wiki Category:Dialogue "Speech Challenge" and
  Fallout Wiki "Speech (Fallout 3)"; hypothesis: nearest-percent rounding and exact
  FNV/TTW label format not yet captured in-game, §7 item 19].
- On pass: counts `Speech Successes`, awards `iXPRewardSpeechChallenge<class>` XP, and
  speaks the challenge line.
- On failure: counts `Speech Failures` (no XP), marks the challenge line said without
  speaking it or running its result scripts (so a `Say Once` challenge cannot be retried
  [documented: Fallout Wiki "Speech (Fallout 3)"; hypothesis: via the line's said state]),
  and speaks the speaker's line of `SpeechChallengeFailure` (DIAL 0x000000FD) whose Link
  From names the challenge topic, falling back to an unlinked `SpeechChallengeFailure`
  line [documented: GECK wiki Topics Tab, SpeechChallengeFailure; observed: Fallout3.esm
  has 74 linked and 113 unlinked failure lines, #3024; hypothesis: unlinked lines serve as
  fallback]. If no failure line is sayable, emits `NoLine` and updates/removes the failed
  choice without speaking the pass line [design]. New Vegas deterministic checks (`KNAM`
  without `SPEECH_CHALLENGE`) are unchanged.

### 3.4 Follow-up lines (TCFU)

Not described in our sources beyond xEdit's name "Follow Up" and its target type INFO.
The data: a line with follow-ups has no choices (15 exceptions in 1,759); follow-up
targets have no prompt (25 exceptions in 2,477 links) and are stored under the same
topic (1,029) or another (1,448) [observed: the summary; scratch parse]. Doc Mitchell's
farewell is a chain of them: the greeting → the Pip-Boy line → one of two suit lines
(they differ by a condition) → choices → answer → the Goodbye line [observed:
`fnvtool dialogue lines … --topic VCG01DocMitchellTopic076`].

Runtime: when a line with follow-ups ends, the first follow-up the actor can say is
said next, without a choice by the player [owner statement 2026-10-10: follow-ups have
no player response, as read here from Doc Mitchell's lines; not yet captured;
hypothesis: the first sayable one when a line lists several].

The 15 lines of FalloutNV.esm with both follow-ups and choices (§3) fall in two kinds
[observed: `fnvtool dialogue lines --all`, 2026-10-31]. In 11 the follow-up is Say Once
(so the choices come once it has been said) or has choices of its own, mostly the
same or overlapping ones (the Fort's greeting, Arcade's Topic065, Papa Khan, Markland,
Vicky Vance, Caesar's Topic095/146/150, the followers' distance line). In 4 the
follow-up is a Goodbye line, so saying it leaves the choices unreachable: Yes Man's
Topic056 (INFO 0x00158A3D, whose choice Topic068 was never offered [observed:
mq-walkthrough-2026-10-27 §2 item 13]) and Topic064, both leading to Topic067's Goodbye
line, which is also one of their choices; Jeannie May's Repcon line (to GOODBYE); the
King's north bodyguard's Topic002. So a line with choices passes over its Goodbye
follow-ups and offers its choices; other follow-ups are said first, as above [design,
issue #964; what the original does is observer check 65].

When the player chooses a topic from a list, its lines that some line names as a
follow-up are passed over while another line of the topic can be said; only when none
can is a follow-up line taken [hypothesis, 2026-10-30; observer check 37]. Greetings and
SayTo keep taking the first sayable line: the rule tried there too changed the Legion's,
House's, Yes Man's and Dead Money's walkthroughs (greetings whose first line is some
line's follow-up) [observed: `script cover`, five chains with step errors]. Why: six
topics of the ten masters store a follow-up line first and a line that is no follow-up
after it [observed: scratch survey of `fnvtool dialogue lines --all`]. One of them is
the Think Tank's answer to "Topic001" in Old World Blues: its first line 0x01013555
starts the five-line chain that Topic000's line 0x0101432E leads into, and the line
after the chain, 0x01013559, is the only one offering Topic002, whose last line gives
the Sink's Central Intelligence chip (the start of All My Friends Have Off Switches).
Taking the first line would make the Sink unreachable.

### 3.5 Link From (TCLF)

The wiki: "This can be used to allow many topics to link to this topic" [documented:
Category:Dialogue, "Edit Topic Links"]; for conversations between NPCs it chains HELLO
→ … → GOODBYE, and the starting HELLO must have it blank [documented: Conversation Tab].

What the data shows (the ten masters) [observed: scratch survey 2026-10-11]:

- 1,293 links on 924 lines. Most are radio (792) and NPC conversation chains; 431 name
  the Conversation topic `ANY`.
- Between player topics (a type-Topic line of a type-Topic topic naming a type-Topic
  topic): 185 links under 58 named topics. In 166 of them every line of the named
  topic already lists this one as a choice; 19 add something (the real-data test
  asserts 58 / 185 / 19).
- Michael Angelo (VMS22) uses it most. His "Bye" line (`VMS22MichaelAngeloTopic004`)
  names all his topics and GREETING: an exit from anywhere. Topics 031, 032 and 034
  name each other: a group of answers that each offer the others. GREETING is named by
  his 003 and 004 only.
- Between actors (Conversation topics): 83 links under 65 named topics, 32 adding
  something. HELLO is named by the Fiends' and Cass's Freeside replies, Wrangler's,
  Boone's and Jack's (Nellis), and by seven GenericKids HELLO lines with the Goodbye flag
  ("Hi.", "What?"): replies to a greeting. Their openers have Link From blank or `ANY`.
  Several speeches name their own topic (the NCR deputies' speech, the Primm sheriff's
  order); the one line that does not is the last, with the Goodbye flag.
- Radio: 54 links under 21 named topics, 50 adding something. Black Mountain Radio's
  segments name RadioHello and themselves, and RadioBMMusic names the 13 segments
  (its lines are gated by a quest variable). RadioBMSegment12 is reached only this way
  (no RadioHello line lists it). Radio New Vegas's RNVNewsStory3 follows RNVNewsStory2
  only this way.
- `ANY` (a Conversation topic with no lines) is named by 431 links of 26 topics, all of
  which are also reached by ordinary Choices links. It reads as a marker ("a reply, not
  an opener"; HELLO lines naming it cannot start a conversation) rather than a topic
  that is said; taken literally it adds nothing, since no line is a line of `ANY`.
- A per-line reading (the line with the Link From can be said only after the topics
  it names) would contradict Choices links: 54 type-Topic lines (many of them Dead
  Money's radio acts, which name `ANY`) are listed as a choice by a topic their Link From
  leaves out, e.g. `VMS22MichaelAngeloTopic024`, a choice of Topic006, whose only line
  names just Topic025. So the per-line reading is used only for topics that a line
  reaches through Link From alone (below).

Runtime (`GameData::linked_from`, `Game::links_after`, `Game::link_line`), the same in
player dialogue (`Game::build_choices`), conversations between actors
(`Game::next_conversation_line`, §5.1) and on the radio (`Game::radio_follow`,
radio.md §5):

- After a line said under topic X, the topics with a line naming X in its Link From
  join the topics the line leads to, after its own choices. In player dialogue they join
  its choices if it has any; otherwise they join the topic list by priority; they do
  not turn a line without choices into one with choices. Between actors and on the
  radio they join the Link To topics one of which is picked at random.
- A topic reached only this way is said with one of its lines that names X (the first
  the speaker can say, by the rule of §4). A topic that is also a choice (or in the
  topic list) is said with any of its lines, as before. Examples: a child answers a
  hello between actors with "Hi." (Link From HELLO, Goodbye), not with its own opener;
  Michael Angelo's 031 after his 034 is his line naming 034.
- Which lines count follows who is talking: in player dialogue only player lines
  (type Topic) of topics of type Topic, as before the three shared the links; between
  actors only Conversation lines (§5.1, the owner's 2026-10-11 ruling); on the radio
  any line [hypothesis].
- [documented that Link From adds links; hypothesis for how they combine and for which
  line answers; capture question 10.] Radio and conversation topics name the topic of
  the stored line (the radio's line is never a shared one).

### 3.6 Run Immediately

[documented] The [GECK Dialogue page](https://geckwiki.com/index.php?title=Dialogue)
places Run Immediately results at conversation generation. It does not define the precise
selection point used here.

[design] NVForever runs begin, then the flagged end, when the selected line starts:
player conversation (`crates/fnv-game/src/dialogue.rs`, `speak`), actor conversation,
Say/SayTo and hello (`crates/fnv-game/src/game.rs`, `say_line`), and radio
(`crates/fnv-game/src/radio.rs`, `radio_line_begins`). Line/state publication and response
idles keep their existing order before those results. `say_line` sets `Speech::end_ran`
before either callback, so a result that finishes or replaces its own speech cannot mark
the replacement. Completion, interruption and snapshot/load do not repeat that end.
Radio completion still dispatches its line-end notification, but skips the flagged end
script; interruption has no completion call (`engine/nvf-play/src/radio.rs`). These are
shared runtime timing choices, not a claim of an observed original selection point.

[design] Synthetic ordering, completion, interruption, replacement and saved speech
coverage is in `crates/fnv-game/tests/dialogue.rs`; existing Goodbye coverage remains there.
Radio has an unflagged control as well.

All 765 lines with the flag are Radio (673) or Conversation (92) lines; no Topic line
has it [observed: scratch parse; the real-data test asserts it].

## 4. Which line an actor says

One rule for greetings, chosen topics and the script functions SayTo and Say (`Game::select_line`, `Game::can_say`):

1. **Quest priority decides between quests.** The topic's lines are taken with the lines
   of higher-priority quests first [documented: Quest Data Tab, owner export: priority
   decides whose dialogue wins, higher wins; owner statement 2026-10-10].
2. **Equal priority: stored order.** Lines of quests with the same priority keep the
   stored order of §3.1, "the first in the array" [owner statement 2026-10-10, from GECK
   use; not yet captured].
3. **Inside one quest: first eligible line from the top**, unless lines carry the
   Random flag [owner statement 2026-10-10, from GECK use; not yet captured; observed
   for SayTo: Doc Mitchell's VCG01Intro lines, layer5-captures.md §10.3].
4. **Random pools.** Random lines that sit together in the list form a pool. A line
   without the flag between two groups of Random lines splits them into two pools,
   whether or not that line can be said [owner statement 2026-10-10: "random pools have
   to be together; a non random line between 2 groups of randoms will split the pool
   into 2 separates"; not yet captured]. A Random End line closes its pool at that line
   [documented: Category:Dialogue, flag Random End; hypothesis that it also does when
   the actor cannot say that line]. Going down the list, a pool is taken if the actor
   can say any of its lines, and one of those is then picked at random [documented:
   Category:Dialogue, flag Random, for the pick]. This settles the wiki's contradiction
   (its flag description lets a qualifying plain line end the pool, its in-depth
   example 3 lets the pool run across a plain line) in favour of neither: the pool is a
   matter of where the lines sit, not of which qualify. Before this ruling the runtime
   skipped a plain line the actor could not say and let the pool run on across it.

Implementation: a stable sort of the stored order by quest priority, highest first;
then down the list over items, an item being a plain line or a pool; the first item
the speaker can say. A pool is a run in that sorted order, so lines of two quests of
equal priority that sit together can share a pool [hypothesis]. The Random pick uses
the game's seeded generator, and only when more than one line of the pool can be
said.

A speaker **can say** a line when:
- no actor has said it, for Say Once; it has not said it within the last
  `fSayOncePerDayInfoTimer` game hours (12 when the setting is absent), for Say Once a
  Day. The wiki has both per actor [documented: Category:Dialogue], but Say Once was seen
  to hold across actors: the Yes Man mainframe skipped the greeting 0x001164F1 the Tops
  Yes Man had said [observed 2026-10-31, observer check 31; one case, and the two may
  count as one speaker]. Ours holds a Say Once line back from every actor once any has
  said it (#1113); Say Once a Day stays per actor. The clock is GameDaysPassed × 24
  [design: `Game::game_hours`], which moves one per game hour as GameDaysPassed already
  grows by the fraction of the day (simulation.md §5b); that the timer runs on game time
  at all is [hypothesis];
- it is the line's fixed speaker, if the line has one [documented: Category:Dialogue] —
  unless the line has its own GetIsID condition on the speaker, which then decides
  [observed 2026-10-31: the Lucky 38 elevator Victor said 0x00165102, whose fixed
  speaker is the entrance Victor and whose condition is GetIsID the elevator Victor;
  observer check 29]. A condition on the speaker naming a group (GetInFaction,
  GetIsClass, GetIsRace, GetIsVoiceType, IsInList) decides as well [hypothesis, #2548,
  observer check 162]: Cottonwood Cove's "Halt!" greeting 0x0013AC97 names the AAM
  explorer and tests VCaesarsLegionCCoveExplorerFaction, which all four explorers are
  in, and the other three have GREETING force-greet packages of their own
  (0x001789B6-B8) that would end on the generic "Move along." otherwise. 19 lines of
  FalloutNV.esm have a fixed speaker and such a condition without GetIsID [observed]:
  the Cottonwood Cove explorer lines (7), the Tops' random bartender (3) and the
  Strip's Securitron messenger (9 with VVictorFaction, said by the messenger itself).
  Such a line said by another actor plays the fixed speaker's voice file when the
  actor's own voice has none (voice.rs, as for a GetIsID line);
  - **a script's Say or SayTo** whose topic has no line passing this check, judged from
    the data alone (every line names another fixed speaker and has no own-ID condition;
    a line the speaker passes the check for but cannot say now, because it was said
    once already or a condition is false, still counts, so that call says nothing as
    before), tries the topic's lines without the check, so the speaker the script names says a fixed
    speaker's line in that speaker's stead (`Game::say_by_script`, `Says::InStead`;
    issue #965) [hypothesis]. The data behind it [observed 2026-11-06, `fnvtool dialogue
    lines --all` and `script decompile` over FalloutNV.esm and the five add-ons, the
    Say/SayTo calls whose speaker is a placed reference named in the call]: 30 calls
    (29 topics, 52 lines, 10 speakers) find no line passing the check, and in every
    one the speaker is a narrator or an intercom saying another character's words: Dead
    Money's narrator (NVDLC01NarratorRef, base NVDLC01Narrator) saying INFO 0x01013C60
    (topic NVDLC01IntroSlide02) and 0x01011DE9 (NVDLC01EndingIntroElijah), fixed speaker
    Elijah, and 0x01011DE8 (NVDLC01EndingIntroVera), fixed speaker Vera, none with a
    condition, while the topics' other slide lines carry GetIsID NVDLC01Narrator; Dead
    Money's suite and restaurant intercoms (Christine's, Dog's and God's lines); Old World
    Blues' X-8 and satellite intercoms (Mobius's, Borous's and the other Think Tank
    doctors' lines, most with GetIsReference on the intercom, three with no condition).
    The other 20 calls whose topic has refused lines also have lines the speaker passes
    (GREETING, Hit, Death, Dog's kitchen barks), so they pick as before. Only the two
    script functions do this: combat barks, package and patrol topics keep the check,
    so an actor with no Hit line does not say Mobius's. Calls whose speaker is the
    script's own reference (no reference before the dot) were not surveyed.
- the line's quest is running **and the quest's own conditions pass**
  [owner statement 2026-10-10: "every actor has the line as long as the quest is active
  and the conditions are met on both the topic and the quest itself"; not yet captured;
  documented: Quest Data Tab: a quest's conditions apply to all of its dialogue and are
  checked before the lines' own]. A line of a stopped quest cannot be said. The
  runtime evaluated the quest's conditions (the QUST's CTDA list before its stages)
  from the start; they run like the line's, speaker as subject and listener as target
  [hypothesis: the page does not say on whom]. 113 of the 640 quests carry conditions
  (289 in all); 101 of them have lines, 8,151 lines in all [observed: the lines
  summary's `quest_conditions`];
- the line's conditions pass, with the speaker as subject and the listener as target
  [documented: Category:Conditions]. "Conditions on the topic" are these: the editor
  lists a line's conditions under its topic, and a DIAL record has no condition
  subrecords of its own [observed: no CTDA among the DIAL subrecords, the topics
  summary's `subrecords`].
- **A null form parameter** (FormID 0 in a CTDA form parameter: GetQuestVariable on Papa
  Khan's goodbyes INFO 0x0014B595-7, GetIsID on Cass's greeting 0x000FF5BA) reads 0
  [observed 2026-10-30, capture `o4_00_probe` (save nvfairport; issue #907): ShowOff's
  SayTo_GetTopicInfo after a placed copy of VRRCPapaKhan was told to SayTo the player:
  GOODBYE chose 0x0014B595 (`GetQuestVariable <null> 1 == 0`, then GetIsID VRRCPapaKhan),
  and GREETING chose 0x0014B5A9 (position 1613), passing over 0x000FF5BA (position 1305,
  only `GetIsID <null> == 1`; both quests priority 55 and running)]. So Cass's line
  0x000FF5BA is never said and Papa Khan's 0x0014B596/7 (`== 1`) never either. Ours gives
  0 for these two functions in a condition without a failed call (`Args::is_null_form`),
  and since #966 any condition with a null form parameter reads 0 without calling its
  function (`Game::condition_value`), so no `call_failed` is logged for it [hypothesis
  that the original gives 0 for the other functions too].

Pointer for the AI package work: the owner states that an actor's package is chosen the
same way, the first eligible one from the top of its list [owner statement 2026-10-10];
`Game::select_package` already does that (scripts.md §7.5).

## 5. Conversations (`crate::dialogue`)

State: `Game::dialogue` (`Conversation`: the actor, the line being said with the
response and its remaining time, or the choices offered), `Game::said` (lines said, per
actor), `Game::said_day`, `PlayerState::topics` (known topics), `RefExtra::talked_to_pc`.
All of it is in `GameSnapshot`.

- **Start** (`Game::start_dialogue(actor, topic)`). The actor says the line of GREETING
  it can say (§4), or of the named topic [documented: Topics Tab: GREETING is what an
  actor says when spoken to; StartConversation: a named topic replaces the greeting].
  An actor with no such line cannot be talked to [hypothesis]; nor a dead, disabled or
  deleted one. One conversation at a time [hypothesis].
- **A greeting with the Goodbye flag starts no conversation.** When the line picked from
  GREETING carries the flag, the actor says it at the player and that is all: no
  dialogue menu, no pause, nothing to choose or to leave, and the player is not held
  [owner statement 2026-10-10: a GREETING line marked Goodbye "does not draw the player
  into a dialogue exchange, it just is spoken at the player without locking them in";
  not yet captured]. 3,023 of GREETING's 5,579 lines carry the flag (2,972 of them
  first defined in FalloutNV), so this is the usual answer of an actor with nothing
  particular to say; 73 of them have a begin result, 150 an end result, 3 an Add Topics
  list, 99 Say Once, none a follow-up [observed: `fnvtool dialogue lines … --topic
  GREETING`, counted over `lines`]. In the runtime the line is a `Speech`
  (`Speech::greeting`), said like a SayTo line to the player (`Game::say_line`):
  `Game::dialogue` stays empty, the begin result runs at once and the end result when
  the line is over, in game time, and the trace has `line_started` / `line_ended` but
  no `dialogue_started`. What a conversation of that one line did is kept: the Add
  Topics list is applied when the line ends and the actor counts as having talked to
  the player (GetTalkedToPC); no SayToDone block runs [hypothesis for the three].
  The player talking to the actor again while it speaks does nothing: the line plays
  to its end and no second line starts [player report #4683, 2026-11-26: "New phrase
  should trigger only after previous phrase has ended"; the runtime's activation,
  `Runtime::talk_after`, checks `Game::greets_player`]. Before #4683 it cut the line
  and started the greeting afresh [hypothesis then, as for a second SayTo];
  `Game::start_dialogue` called directly still does that. A Goodbye line of a topic
  named by StartConversation still opens a conversation that ends with it [hypothesis:
  the statement is about GREETING]. Through a talking activator the greeting opens a
  conversation that the line ends, as below [owner statement 2026-10-22, of the REPCONN
  intercom; talking-activators.md §2].
- **StartConversation from afar walks up first** (issue #1544, 2026-11-05). "The actor
  will walk to the target" before the conversation [documented: GECK wiki
  StartConversation]. With the AI running, a speaker more than 300 units from the
  player, or in another space, records a walk-up (`Game::walk_up_greet`). The AI runs it
  to the player, planning the way again as the player moves, and opens the conversation
  within 150 units (`Ai::walk_up_step`). With no way there, or at the end of its way, it
  opens it where it is. Its package waits meanwhile, and a change of package plans the
  walk-up again. A speaker the AI does not walk (its cell not loaded) talks once it is
  within 300 units of the player, never from afar; out of the loaded area (the low
  process) the walk-up waits. Without the AI (headless runners) the
  conversation opens at once, as before [design: the distances and the run].
  [design, #2724] Any successful opening of a player conversation consumes that
  speaker's pending walk-up, before the first line's result scripts run
  (`Game::open_dialogue`). A repeated StartConversation that opens within 300 units,
  another force greet, or the player's own opener therefore cannot leave an old
  walk-up to greet again after the conversation. Refused opens and greetings that
  do not open a conversation leave it pending; other speakers' walks are unchanged.
  [observed 2026-11-05] The Legion explorers about Nelson (nvNelsonExplorerBarkSCRIPT
  calls StartConversation Player once its GameMode runs) greeted from 10,099 units off.
  Now explorer 01 runs over and greets from 147 units (smoke
  `start-conversation-walks-up`).
- **The rule above is for the player talking to the actor. When the actor starts the
  conversation** (a force greet: StartConversation with the player, or the actor's
  Dialogue package that is not Say To), a greeting with the Goodbye flag **opens a
  conversation too** (`Game::force_greet`): the dialogue menu holds the player, the game
  is paused behind it as in any conversation, and when the line is over the Goodbye flag
  ends it [owner statement 2026-10-11, of Sunny Smiles's "Hear that up on the ridge
  behind me there?" (INFO 0x0010A1ED, GREETING, Goodbye and Say Once, three responses,
  no choices; said by her Dialogue package VCG02SunnySmilesDialogueSneakStart at VCG02
  stage 30): the line "needs to lock the player into dialogue even though there are no
  responses from the player … until she is finished talking"; documented:
  StartConversation's allowMovement, "Enter 1 to enable movement during conversation",
  so by default the player does not move (GECK wiki StartConversation, owner export);
  hypothesis: a Dialogue package's force greet is the same, the wiki says the actor
  "will force-greet the player"]. Before 2026-10-11 StartConversation without a topic
  and the Dialogue packages said such a greeting without a conversation.
- **A conversation pauses the game** behind it; lines spoken outside one (SayTo, Say)
  do not [owner statement 2026-10-10: Doc Mitchell's Rorschach test, "a back and forth
  dialogue with chosen options from the player", is what makes "the game … pause in
  the background", unlike his earlier spoken lines; not yet captured]. §6 says what
  that means for scripts. The conversation's own lines go on in real time.
- **A line.** The begin result script runs when the line starts, the end result script
  when its last response is over [documented: Category:Dialogue]. Responses follow each
  other in number order; each lasts `Game::response_seconds` of real time (the one place a response
  is timed: the length of its voice file in the actor's voice, or the fitted 1.1 s +
  1/16 s per character when it has none or the files are not given,
  docs/spec-notes/voice.md §8). Result scripts run on the actor with the line's quest. With Run
  Immediately both scripts run when the line is picked [documented flag; hypothesis
  that for a line the player asked for "when the conversation is generated" is that
  moment; no Topic line of the masters has the flag].
- **When a line ends**, in this order: end result; the line's Add Topics join the known
  topics [documented: xEdit "Add Topics"; hypothesis on the moment and that they stay
  known afterwards]; a Goodbye line ends the conversation [documented]; else the first
  follow-up the actor can say is said (§3.4) [owner statement]; else choices are offered;
  with nothing to offer the conversation ends [observed: observer check 72, 2026-11-26,
  Sergeant Kilborn's vNiptonVulpes Topic003 (INFO 0x00138E1A), whose end script stops
  the quest: the menu showed no topics and closed by itself within about 4 s of the line's
  end, with no input; #1113]. A `StartConversation player`
  in the end result of the line that ends the conversation (Goodbye, or nothing to offer)
  is held and starts right after the conversation closes; when the conversation goes on
  it is refused, as any StartConversation while one runs (`Game::hold_force_greet`)
  [design, issue #1107: 8 end scripts in FalloutNV.esm do this, 4 of them of Goodbye
  lines, e.g. Anders' thanks in VMS46, INFO 0x00142A9C; the other 4 lines have neither
  the flag nor choices [observed: `fnvtool dialogue lines --all`]; observer check 39]. A
  conversation the end script ends some other way does not start it [design].
- **A chosen line with the Goodbye flag holds the player until it is said**, like the
  lines before it, and ends the conversation when it is over (`Game::finish_line`)
  [owner statement 2026-11-11, issue #2157: the last line of a conversation must hold
  the player like the rest; said in the world, a quest's line can be interrupted in
  play]. From 2026-11-05 (#1606) until then every chosen Goodbye line closed its
  conversation at once, a topic's last line with its quest stage as much as "Goodbye.".
  One rule for all of them: the data does not tell a plain goodbye from a quest's last
  line, since actors' own "Goodbye." lines are ordinary topics (Chet's is
  VFreeformGoodspringsGoodspringsChetTopic001, not the GOODBYE topic [observed: the
  `goodbye-choice-holds` smoke log]).
- **Kept behind a switch, off by default** (`GameConfig::goodbye_at_once`; nothing in
  the engine sets it): a chosen Goodbye line ends the conversation at once
  (`Game::choose`, `Game::say_farewell`, issue #1606). The dialogue menu closes as the
  line starts, the game is no longer
  paused, and the actor finishes the line in the world, said at the player
  [observed: owner's playtest 2026-11-05, against the original: choosing a Goodbye
  closes the menu at once and the actor finishes the line as the player walks away;
  which lines the original does this for is open, §7 item 18].
  In the runtime the line moves to `Game::speech` as a farewell (`Speech::farewell`)
  with the time it has left (the rest of the response being said and the responses
  after it), and `dialogue_ended` (`goodbye`, the line counted) comes before its
  `line_ended` (not cut). Its begin result, `line_started` and Say Once came as it was
  chosen and do not come again; with Run Immediately its end result ran then and does
  not run again. When the line ends it ends as above: end result, Add Topics, and a
  `StartConversation player` from that end result starts after them
  (`Game::hold_force_greet`) [design: the order of a Goodbye line that ends in its
  conversation]; no SayToDone block runs [hypothesis, as for a Goodbye greeting]. It is
  heard and subtitled where the actor stands, like a SayTo line, and it does not hold
  the actor in place: the actor goes about its package while it speaks [design, from the
  statement: the player walks away]. It is never cut: another line of the actor, the
  actor disabled, dead or deleted, or a fight (`State::Combat`, #883) ends it as if said to the end, as a
  Goodbye greeting at the player [hypothesis: a Goodbye line's end result sets quest
  stages and must not be lost; the owner's catch-all statement of 2026-10-27 was about
  the greeting]. Searching states (`State::Alert` and `State::Lost`, #3663) do not end a
  Goodbye greeting or farewell prematurely, so a Goodbye greeting started while its
  speaker is searching plays out instead of being discarded on the same frame
  [hypothesis: an NPC that only searches can be talked to]. Game time and GameMode scripts run while it is said, as for any line
  said in the world. Not moved, as they are not chosen: a Goodbye greeting that opens
  a conversation (a force greet, above), a Goodbye line of a topic named by
  StartConversation, and a Goodbye follow-up; each ends its conversation when it is
  over. Nor does a chosen Goodbye line whose begin result made the actor say another
  line (Say) leave its conversation: it ends it when it is over, and the other line is
  not cut [design: a begin script that makes the actor say another line keeps today's
  behaviour]; a begin result that ended or replaced the conversation leaves nothing to
  move. **Open:** a conversation through a talking activator keeps a chosen Goodbye line
  in the conversation until it is over, as before [hypothesis: the statement was face to
  face; a farewell is heard from its speaker, and the activator it went through is not
  carried by `Speech`]; whether the original closes that menu at once too is a question
  for the observer (§7 item 18).
- **Choices** (`Game::build_choices`). After a line with choices: those topics and
  nothing else [documented; owner statement 2026-10-11: a line linked by choices offers
  only the topics given as its choices]. Otherwise the top-level topics plus the known
  topics [documented: Category:Dialogue, "Top-level"; Quest and Dialogue Tutorial]. A
  topic is offered only if the actor has a line of it to say now, its own or a shared
  one (§1.2), and that line is the one said if it is chosen [hypothesis; the editor's
  filtered dialogue window lists only what an actor qualifies to say]. If none of a
  line's choices can be said, nothing is offered and the conversation ends
  [hypothesis]. (Until 2026-10-11 the topic list stood in there; with shared lines
  ignored, that showed the top-level topics after 257 lines whose choices only had
  shared lines, e.g. Red Lucy's greeting.) Topics whose lines name the line's topic in
  their Link From join either list (§3.5) [documented; hypothesis on how]. Order: topic priority (DIAL PNAM), highest first
  [documented]; then, for a line's choices, their order in the record [documented:
  "Sorting order of links"], and for the topic list the known topics newest first, one
  line's Add Topics keeping their record order and a line's topic already known moving
  to the front again (`AddTopic` on a known topic leaves it in place: scripts may call
  it every frame), then the others in record order, the top-level topics among them
  even when the player also learned one [owner's choice 2026-10-31 and design;
  `KnownTopics`, `dialogue::ListKey`]. The original follows no rule we can copy [observed
  2026-10-31: Boone's list after learning C8, CA, then CC and CB from one line read C8,
  CC, CB, CA; Manny Vargas with topics given by `AddTopic` (captures `dlg_topic_order_*`
  and owner screenshots): learned CEDEE, B15FC, 137A62, B15FB, B15FA, then 96BCF from
  the greeting gave 96BCF, B15FB, 137A62, B15FC, top-level 15F503, CEDEE, B15FA; CEDEE,
  B15FC, 96BCF gave 96BCF, B15FC, 15F503, CEDEE; 96BCF, B15FC, CEDEE, then 96BCF again
  gave 96BCF, 15F503, B15FC, CEDEE. The same set in two orders rules out record order
  or any fixed index; each run gave the same list twice. Hypothesis: equal priorities
  come out of a sort that does not keep their order.] Quest priority plays no part in the order of the list.
- **The text of a choice** is the line's prompt, else the topic's name [documented].
  It is shaded when the actor has said that line before, unless the line has Info
  Refusal; always with Always Darken [documented: Category:Dialogue flags].
- **GOODBYE.** When the topic list holds no Goodbye line, the actor's line of the
  GOODBYE topic is offered last, if it has one [documented: Quest and Dialogue Tutorial;
  Conversation Tab]. **Its line ends the conversation when it is over, with or without
  the Goodbye flag, unless it leads on: the actor can say a line of one of its own links
  now, a follow-up, a choice, or a player topic whose line names GOODBYE in its Link
  From (§3.5)** (`Game::ends_conversation`, issue #3477). The topic list is no link of
  the line. Only links the actor can say something for count [design]: a Link From names
  the topic, so it is a link of every actor's GOODBYE, and one actor's reply in a plugin
  would otherwise bring the topic list back after everyone else's [documented: Quest and Dialogue Tutorial:
  without a topic flagged Goodbye "the dialog will end with one of those GOODBYEs";
  Conversation Tab: NPCs use the topic when the player exits the dialogue menu;
  hypothesis: that this holds for its lines without the flag, observer check 283]. Such
  an entry counts as a Goodbye (`Choice::goodbye`): Tab picks it. The data [observed:
  `fnvtool dialogue lines --topic GOODBYE`, 2026-11-23, FalloutNV.esm with its add-ons
  and Fallout 3's six masters]: 2 of FalloutNV.esm's 274 lines have no flag (Keith's
  0x0014E019, Jerry the Punk's 0x0014C483), 31 of Fallout3.esm's 861 (17 of quest
  DialogueParadiseFalls, Pronto's 0x000384CF among them) and 1 of PointLookout.esm's 36;
  no New Vegas add-on has one. All 34 are Conversation lines with no choices, follow-ups,
  result scripts or Add Topics, and no line names GOODBYE in its Link From: they lead
  nowhere, so ending cuts nothing off, and the exception is for plugins. Two of them are between NPCs (Mr. Brotch and
  Freddie), whose exchange ends for want of links (§5.1). With the flag, 5 lines of
  Fallout3.esm have choices as well (4 of them between NPCs); the flag ends the player's
  conversation there, as before. With `GameConfig::goodbye_at_once` an unflagged line is
  still held until it is over [design: the switch is about the flag].
- **Known topics.** `AddTopic` and lines' Add Topics lists fill one list, newest first
  (above), kept for the whole game; every actor that can say a line of a known topic offers it [owner
  statement 2026-10-10: every actor has the line while its quest is active and the
  conditions are met; not yet captured]. When an Add Topics list is applied (here: when
  the line ends) stays a [hypothesis].
- **Leaving** (`Game::end_dialogue`). The line being said is cut without its end result
  [hypothesis]. When the game lets the player leave is the screen's business (D4:
  docs/design/dialogue-play.md §2).
- **Skipping** (`Game::skip_response`, added for the screen). The response being said
  ends at once: the next response of the line starts, and after the last one the line
  ends exactly as a line said to the end (end result, Add Topics, Goodbye, follow-ups,
  choices) [hypothesis: skipping only shortens a line; the other reading, a skipped
  line running no end script as a cut line does, would stop every quest whose stages
  are set by end scripts whenever the player clicks through; capture question §7.8].
  Nothing is skipped while a menu on top of the conversation holds it. No trace event
  of its own.
- **Not modelled:** IdleChatter (hellos and random conversations are §5.2, scripted
  conversations between actors §5.1), radio, combat and detection barks, the reaction
  topics (PLAYERFIREWEAPON and the rest), the quest flag "Allow Repeated Conversation
  Topics", lip sync and emotions on faces. (Low/High Intelligence lines and Dumb Response
  were added 2026-10-25; the Fallout 3 speech-challenge roll and `SpeechChallengeFailure`
  routing are in §3.3, #3406.)

### 5.1 Conversations between two actors (`Game::start_actor_conversation`)

`speaker.StartConversation target [topic]` with an actor as the target makes the two
talk to each other [documented: GECK wiki StartConversation: it "forces a conversation
between two NPCs, optionally starting with a specified topic"]. The masters call it 104
times: 83 with the player as the target, 21 (in 16 script units) with another actor
[observed: `fnvtool script decompile` over FalloutNV.esm]. One of them is Goodsprings'
Joe Cobb and Trudy (below).

- **What a conversation is**: "a series of infos that are played between two actors";
  each line's Link To topics (TCLT, the choices of player dialogue) give the next one,
  and with several Link To topics "the game [selects] one … at random"; a line with the
  Goodbye flag ends it; "infos in conversations cannot have more than one response.
  Extra responses are ignored" [documented: Conversation Tab].
- **Only Conversation lines** (`fnv_game::dialogue::between_actors`): every line said
  between two actors (the first, follow-ups, Link To lines) has the INFO type
  Conversation; Topic lines, the Topics tab's lines for the player, are passed over as
  if they could not be said [owner ruling 2026-10-11: "the dialogue between npcs should
  ONLY use the conversation options not the Topics", after a test run in which Sunny
  Smiles greeted another actor with the player's lines]. Why it mattered: HELLO holds
  569 Topic lines and 505 Conversation lines, and the topics HELLO links to 1,823 Topic
  and 2,325 Conversation lines [observed: `fnvtool dialogue lines --all`]; Sunny has no
  HELLO line of her own, and for each of 11 Goodsprings residents her first sayable
  HELLO line was a Topic line, while each now opens with a Conversation line [observed:
  `dialogue_data.rs`, `sunny_opens_conversations_with_conversation_lines`]. Of the 20
  scripted StartConversation calls with an actor and a named topic, 19 name topics of
  Conversation lines only; NellisArgyllPatientHealedDumb (SCPT 001009F2) holds one Topic
  line, so that exchange is now silent [owner decision 2026-10-11: left as it is for
  now; whether the rule should spare a topic a script names stays open].
- **Start.** The speaker says the line of `topic` it can say with the target as the
  conditions' target (§4); HELLO without a topic [hypothesis, from the Conversation
  Tab: random conversations start with HELLO]. Nothing starts when either side is
  disabled [documented: StartConversation], dead or deleted, already in a conversation
  (with the player or another actor), or when the speaker has no line [hypothesis for
  these]. In the game the speaker first goes to the target ("no matter where the two
  NPCs are currently"); the runtime starts at once [stand-in, as for
  `StartConversation player`]. For Joe Cobb and Trudy no walk is needed: Joe Cobb
  starts next to Trudy in all base-game footage the owner has seen [owner statement
  2026-10-11]. The location references, head-track and allow-movement parameters are
  not read.
- **The lines** are said in the world as SayTo lines are (`Game::speech`,
  `Speech::conversation`): no menu, no pause, the player keeps control and overhears
  them [hypothesis]; the listener is the other actor (its listener idle plays); begin
  result at the start, end result at the end, in game time; only the first response in
  response order is said and timed [documented, above]; Say Once is kept as for SayTo.
  No SayToDone block runs [hypothesis].
- **No topic twice** (2026-10-25): "If this box is unchecked, conversations will never use
  the same topic twice (in the same conversation)" [documented: GECK wiki Quest Data Tab,
  Allow Repeated Conversation Topics]. The next line's linked topics leave out one the
  conversation has used already unless the candidate line's quest has the flag (QUST DATA
  bit 2; 118 quests in the masters [observed]). Choices that name an INFO directly
  (follow-ups) are not filtered, but they carry the conversation's used topics on to the
  lines after them, their own topic added [design: `Game::next_conversation_line`, #1592].
  Radio programmes are not filtered either: their quests
  set the flag, as the wiki asks for radio stations [documented: Creating Radio
  Stations]. Tests: `dialogue.rs`
  `a_conversation_uses_a_topic_once_unless_its_quest_allows_repeats`,
  `a_follow_up_between_actors_keeps_the_topics_of_the_conversation`.
- **A talking activator** can take part (issue #2451): it says its talking actor's
  lines, chosen with that actor as the speaker and IsActorTalkingThroughActivator 1
  (`Game::talker`), and the line is said by that actor through it (`Speech::through`),
  so the other side's next line is said to the activator. Veronica's package at Hidden
  Valley talks to HVIntercomREF, which speaks for Ramos.
- **The next line** (`Game::next_conversation_line`), when a line has ended and its end
  result has run: a Goodbye line ends the conversation [documented] unless it links on
  (Link To or follow-ups), when it goes on [hypothesis from the data: 19 Conversation
  lines of FalloutNV.esm are Goodbye with links; Jason Bright's speech is started at its
  first part only (SCPT 0008D41A) and its Goodbye part 4 links to parts 5-9; Veronica's
  exchange with Ramos (HVVeronicaEnter, four Goodbye lines) opens the bunker only at its
  third line; observer check 139]; so does either
  side being gone [hypothesis]. Otherwise the line's next speaker says the next line:
  Target is the other actor, Self the same one, Either the other and else the same one
  [documented: xEdit wbNextSpeaker, the names; hypothesis: what they mean]. It says the
  first follow-up (TCFU) it can say to the other [hypothesis, as in player dialogue],
  else a line of one of the Link To topics, at random among the topics it has a line of
  [documented: the random pick; hypothesis: topics with no line are passed over]. With
  nothing to say the conversation ends; no GOODBYE line is added [owner statement
  2026-10-11: no goodbye is heard after Trudy's line, which links nowhere and has no
  Goodbye flag, in the footage of Joe Cobb and Trudy]. The Conversation Tab says
  "all random conversations will end with a GOODBYE unless they are ended by another
  info with the 'Goodbye' flag"; its own example reaches GOODBYE through a Link To, and
  the scene above is scripted, not random. In FalloutNV.esm 9 Conversation lines link
  to GOODBYE, and of the 1,965 Conversation lines that link nowhere (no Link To, no
  follow-up) 959 carry the Goodbye flag [observed: `fnvtool dialogue lines --all`]. So
  whether random conversations (not modelled) get a GOODBYE added stays open.
- **Cut.** A line cut off (its speaker starts another line, is disabled or dies) ends
  the conversation without its end result [hypothesis, as for SayTo].
- **Trace**: `actor_conversation_started` {speaker, target, topic}, then `line_started`
  (with `target`) / `line_ended` per line, and `actor_conversation_ended` {speaker,
  target (of the last line), reason `goodbye` | `no_links` | `cut` | `actor_gone`,
  lines}.

**Joe Cobb and Trudy** [observed: FalloutNV.esm, `fnvtool script decompile`, `fnvtool
dialogue lines --topic GSJoeCobbIntro`, `fnvtool esm decode`]. The end result of
INFO 0015F8B6 enables TrudyRef (00104C6D), GSJoeCobbRef (00104C68) and the trigger
volume 0010521E in GSProspectorSaloonInterior (three trigger scripts do the same in
other orders of events). The trigger's script GSJoeCobbTriggerScript (SCPT 0010521F),
on the player's OnTriggerEnter while Joe Cobb should not attack the player, calls
`GSJoeCobbRef.StartConversation TrudyRef, GSJoeCobbIntro GSJoeCobbRef TrudyRef 0 0`, sets
VMS16.bMetJoeCobb and disables and deletes itself. GSJoeCobbIntro (DIAL 00104DE7, type
Conversation, quest VFreeformGoodsprings) has one line (INFO 00105219: Joe Cobb to
Trudy, one response, next speaker Target, Link To GSTrudyToJoe); GSTrudyToJoe has one
line (INFO 00105218: Trudy, one response, no links) whose end result sets
VFreeformGoodsprings.bArgumentOver and re-evaluates Joe Cobb's packages; the quest's
script then gives him script package 0010663E. Headless (`fnvtool script run`: the
three references enabled, the trigger entered) the runtime plays this through: Joe
Cobb's line (8.80 s, its voice file's length), Trudy's answer (3.85 s), bArgumentOver 1,
package 0010663E chosen, the conversation ended `no_links` after 2 lines. Before this
section StartConversation with an actor was a stub, so the argument was never heard.

**A Dialogue package's Say To line to another actor leads on** (issue #2229,
`Game::say_package_line`): when the line is a Conversation line, once it ends and its
SayToDone blocks ran the two go on as above (`Game::next_conversation_line`: its next
speaker, follow-ups, Link To and choice topics, until a Goodbye line or none). A
scripted SayTo stays one line: scripts chain those themselves through SayToDone
[hypothesis, from the data: TTW's CG04MaryHoldenTalkToTom (Say To, topic
CG04AtriumConv1) says "We should just go back to our quarters" to Tom (INFO 0A0574FC:
Conversation, next speaker Target, its choice CG04AtriumConv1, begin script atrium
stage +1), and only Tom's answer from that topic (INFO 0A0574FE, Goodbye, at atrium
stage 1) sets the stage 3 his CG04TomHoldenDeathRun needs; before, Tom never answered
and never ran].

### 5.2 Actors who talk on their own (`fnv_ai::social`)

Two behaviours, each allowed by the actor's current package ("Enable Fallout Behavior":
"Hellos to player: Allow the actor to say hello to the player"; "Random conversations:
Allow the actor to participate in random conversations. (The packages that can initiate
random conversations are Sandbox, Guard, Eat, and Patrol.)" [documented:
Category:Packages]). Which packages allow them: packages.md §1, general bit 12 — without
it the default allows both [hypothesis, from the data and the owner's Vault 3
statement].

**The topic is HELLO, not GREETING.** GREETING is "the default greeting an actor will use
when spoken to" [documented: Topics Tab]; HELLO is the Conversation-tab topic that
starts conversations between NPCs [documented: Conversation Tab; Quest and Dialogue
Tutorial]. FalloutNV.esm's HELLO holds lines for both listeners [observed: `fnvtool
dialogue lines --all`]: 1,074 lines (569 of type Topic, 505 Conversation; 1,014 Random,
659 Goodbye; conditions mostly GetIsVoiceType, GetInFaction, GetGlobalValue, GetIsID; on
the target GetInFaction 89, GetIsID 108, GetIsRace 32, GetIsSex 26). Some are for the
player (Victor's "Howdy" line in VFreeformGoodsprings; Raul's lines with `target.GetIsID
Player`), some for another actor. 1,047 are one line; 27 link on (Link To) into
exchanges of 2 to 6 lines, e.g. GenericFiend's (two to four lines between Fiends) and
a five-line exchange at Nellis. 1,065 have Link From blank, so can start one
[documented: "The starting HELLO must have 'Link From' left blank"].

- **A hello to the player** (`Game::say_hello`): the actor's HELLO line for the player as
  the conditions' target, Link From blank, said in the world (`LineKind::Hello`): no
  menu, no SayToDone, no Add Topics [hypothesis for these]. Lines that open an exchange
  between actors are said to the player too, with no reply: "these lines can be spoken
  at the player in base game just with no reply if it doesn't hook into a back and
  forth; the fiend just goes back to what he was doing" [owner statement 2026-10-11].
  In the engine in Vault 3 (Vault3a, 200 s, the player put by a Fiend) 6 hellos were
  said, among them GenericFiend's openers 00175369 and 0017536A, and one random
  conversation started (a one-line remark, 00175374, which links nowhere). When: the player within 200 units
  (300 until 2026-11-11) with nothing solid between the two (the engine's measurement
  that a force greet uses, `Game::greet_sight`); again only after the player has gone
  beyond 600 [stand-ins: no distance or timer for hellos is documented, and none is set
  in the masters]. Since 2026-11-11 idle lines (hellos and idle chatter) keep apart
  [design: owner's playtest, #2156: NPCs "way too talkative", everyone in the Legion's
  Fort saying the same line, idle lines through walls, "the proximity cut down 33%"]:
  after one, nobody in the loaded area says another for 20 s and the same actor none
  for 60 s; an actor the player comes up to during the area's wait has missed its hello;
  and a line said within the last 180 s is not said again by anyone
  (`Game::say_hello_fresh`): a Random pool picks among its other lines, and an actor
  whose plain line, or whose whole pool, was said of late says nothing rather than a
  line stored further down (`crates/fnv-ai/src/social.rs`).
- **Which lines**: a hello to the player may be a Topic or a Conversation line (the
  Fiends' openers are Conversation lines, owner statement above); a random
  conversation uses Conversation lines only (§5.1). In the engine in Vault 3 (200 s)
  the 12 lines of 4 random conversations were all Conversation lines [observed: the
  script trace, `NVF_PLAY_SCRIPT_TRACE`].
- **A random conversation** (`Game::start_actor_conversation` with HELLO, then §5.1):
  an actor whose package may start one (Sandbox, Guard, Eat, Patrol; the behaviour
  allowed; not "No Conversation") may start one. The loaded area tries one after a time
  drawn between `fAISocialTimerForConversationsMin` and `…Max` (30 and 60 s of play),
  with a chance of `fAISocialchanceForConversation` per cent (`…Interior` indoors; 25
  both), the starter drawn among those that may [observed: the values in FalloutNV.esm;
  the original's rate, npc-behaviour-compare.md §5: Westside's 0.66 conversations per
  game hour is what one such timer gives, a timer per actor five times more there;
  hypothesis: one timer for the area, in real seconds]. The partner is the nearest actor whose
  package lets it take part, to whom the starter has a HELLO line
  (`Game::conversation_opener`) and a way over the navmesh: within
  `fAISocialRadiusToTriggerConversationInterior` (1000) indoors,
  `fAISocialRadiusToTriggerConversation` outdoors (not in the masters: the indoor value
  stands in).
- **The walk up to the partner** (`fnv_ai::Chat`): the starter walks to the partner,
  who waits where it stands and turns to it; the conversation starts when the starter
  is within 150 units, or where the way ends if it ends further off [documented:
  Sandbox Package, "Conversation -- no duration, the behavior is done when the
  conversation is finished"; StartConversation: "The calling actor will seek out the
  target actor in order to have a conversation"; 150: the standard Activate Distance of
  a Dialogue package, `iActivatePickLength` (Dialogue Package); hypothesis: the same
  distance for random conversations, and that the partner waits]. A seated starter
  does not get up for it (it talks only to someone within 150). The walk is given up
  when it takes over 30 s, when either is gone, or when the partner is busy (talking,
  or its package changed) [stand-in]. While they walk, wait and talk, neither does what
  its package has it do (no sandbox choice, no patrol), and the engine keeps whoever
  waits or talks where it stands; afterwards each goes back to its package's place, a
  sandboxing actor going on from where it is, as after a push [hypothesis]. Both
  actors rest a time drawn the same way when a conversation ends before either starts
  another [hypothesis: without it, in the engine in Vault 3 the same two Fiends started a
  second conversation the moment the first ended].
- **Nobody talks** while asleep (a Sleep package), on its way, already talking, carrying
  out a Dialogue package, dead or disabled, or while a menu or a conversation with the
  player holds the game [hypothesis].
- **Turning**: the one saying a hello turns to the player, and both of a conversation
  between actors to each other, unless on furniture or on their way [hypothesis].
- **Log**: `ai [hh:mm] <actor>: says hello to the player from N units, line …`,
  `… walks up to <partner> to talk: N units away, M units to go`, `… starts a
  conversation with <partner> from N units, line …`, `… gives up talking to <partner>:
  <why>`, `… has talked: back to the place of <package>, N units off`, and at exit
  `social summary: N hellos said to the player, M random conversations started, W walks
  up to a partner (G given up)`.

## 6. What scripts see

| Function | During a conversation | Source |
|---|---|---|
| `IsTalking` | 1 while the actor says a response; 0 while the player chooses | [documented: IsTalking for "currently talking"; hypothesis for the choosing phase] |
| `MenuMode`, `MenuMode 1009` | 1 from the greeting to the end (0 throughout for a greeting with the Goodbye flag the player asked for: no conversation, §5; 1 for one the actor force-greets with) | [owner statement 2026-10-10: the conversation pauses the game; documented: GetMenuTargetRef names menu 1009's target as the actor spoken to] |
| GameMode blocks, game time (`Game::time`, GetSecondsPassed), GameMode quest delays | stopped: **the conversation is menu mode** (`Game::in_menu`), by the rules observed for the other menus (scripts.md §7.2, §7.7) | [owner statement for the pause; the details are the existing menu rules] |
| `Begin MenuMode` and `Begin MenuMode 1009` blocks | run, with the MenuMode quest delay counter | [the existing menu rules; hypothesis for this menu] |
| Magic effects, SayTo and Say lines of other actors | stand still, as in any menu | [the existing menu rules; hypothesis] |
| A GameMode script that starts the conversation (StartConversation) | the rest of that frame's GameMode scripts wait until it is over | [the existing rule for a menu opened during GameMode; hypothesis] |
| `GetTalkedToPC` | 1 once a conversation with the player has started; SayTo lines do not count | [documented: GetTalkedToPC; hypothesis on SayTo] |
| `GetDialogueEmotion` | the emotion type of the response being said, else 0 | [documented: GetDialogueEmotion for the codes] |
| `StartConversation player [topic]` | starts the conversation at once; ignored while one runs or when the actor has no line; the walk to the player needs AI packages | [documented: StartConversation; stand-in for the walk] |
| `StartConversation <npc> [topic]` | the two actors talk to each other in the world (§5.1): no menu, the player overhears; `IsTalking` is 1 for the one saying a line | [documented: StartConversation, Conversation Tab; hypotheses in §5.1] |
| `AddTopic` | the topic joins the known topics (trace `topic_added`) | [documented: catalogue; hypothesis, §5] |
| `GetIsCurrentPackage` | unchanged. Dialogue packages are carried out by `fnv_ai` for actors handed to it (docs/design/actor-behaviour.md §5.6): the actor goes up to the player and the conversation starts as with `StartConversation player` | [documented: GECK wiki, Dialogue Package] |

`Game::menu` holds the other menus and stays empty for a conversation: the conversation
itself is the menu (`Game::in_menu()`, `Game::open_menu_code()` 1009,
`Game::menu_open(code)`). A menu opened during a conversation (a message box from a
result script) opens on top of it at once and holds the conversation's lines until it
is closed; `MenuMode 1009` stays 1 under it [hypothesis].

**The Lucky 38 elevator Securitron (SCPT 0x0015725D)**, which the first version of this
note read as evidence against a pause, fits a pausing menu better. Its OnActivate adds
a dialogue package and sets a counter to 1; a `Begin MenuMode 1009` block moves the
counter from 1 to 2; its GameMode block, when the counter is 2 and `MenuMode 1009` is
not 1, shows the floor menu and resets the counter [observed: the script's compiled
blocks, `fnvtool script decompile --form 0015725D`]. So the script marks "the
conversation is on" from menu mode and acts in GameMode once it is over, which is what
a script does when GameMode stands still during dialogue. With a pause the extra
`MenuMode 1009` test inside GameMode can never be false; why it is there remains
unexplained (a guard for the frame in which the menu closes, or plain caution). In the
runtime both blocks now run as that reading expects.
`IsActorTalkingThroughActivator` (talking-activators.md), `GetDisposition`,
`SetDisposition` and `Exists` are built (script-function-coverage.md); none of the 74
functions the masters' dialogue conditions use is a stub [observed: `fnvtool dialogue
lines --all` against `fnvtool script coverage`, 2026-10-24].

## 7. What needs in-game captures

**To confirm** (owner statements of 2026-10-10, the rules the runtime follows):

- A. Greeting choice when lines of several quests qualify: rules 1–3 of §4, including
  equal priorities and the stored order of §3.1. Two counter-examples to "equal priority:
  stored order" are in the data, where the shipped quests need the line stored later
  [observed; observer check 248]: Hoover Dam's control panel in All or Nothing (House's
  generic greeting 0x00125EBD, VDialogueMrHouse, stored before the panel's 0x001554D8,
  VHDHouseBattle, the only line that sets its stage 20; #3199) and Doctor Zimmer in TTW
  (MS08Fin's goodbye 0x0A0C84FF stored before MS08's hire line 0x0A02026A; #3061). Both
  pairs are of priority-50/55 quests with no other condition between them. One rule fits
  both and FormID or start order does not: the line that wins is the Say Once one
  [hypothesis]. Applied to every line it would reorder lines inside 112 of the 228
  quests with GREETING lines in the New Vegas masters (a plain line stored before one of
  the quest's own Say Once lines) [observed: `fnvtool dialogue lines --topic GREETING`],
  so if it holds it is a rule between quests, not inside one. Not changed in the runtime
  until the original answers.
- B. A conversation with the player pauses the game (GameMode, game time); SayTo and Say
  lines do not (§5, §6). Game time and an attacker were seen to hold while the menu is
  open [observed 2026-10-19, owner-decision-captures.md §7].
- C. A line needs its quest running and the quest's conditions as well as its own; a
  line of a stopped quest cannot be said (§4).
- D. Known topics are global: every actor that can say a line of one offers it (§5).
- E. Follow-up lines are said next with no player choice (§3.4).
- F. Random pools are runs of Random lines that sit together; a plain line between two
  groups splits them, sayable or not (§4 rule 4).
- G. A GREETING line with the Goodbye flag is said at the player without a conversation
  when the player talks to the actor (§5): no dialogue menu, the player keeps control,
  `MenuMode 1009` stays 0. When the actor force-greets (StartConversation, a Dialogue
  package) it is a conversation of that one line [owner statement 2026-10-11].

**Open hypotheses** (numbers kept from the first version of this note; 2 and 4 moved
up to C and F):

1. (moved to A)
2. (moved to C)
3. Whether an actor with no qualifying greeting can be activated at all. The 2026-10-19
   capture found no clean case: of three actors taken as greetingless, the ranger greeted
   (our runtime picks the same line once he is enabled; they are disabled in a new game)
   and the deputy's greeting has the Goodbye flag (owner-decision-captures.md §7).
4. Random pools, what the ruling leaves open: a Random End line the actor cannot say
   (does it still close the pool?), and a pool made of lines of two quests. Ralph's
   GOODBYE was captured on 2026-10-19, but the plugin could not see which line he said
   (owner-decision-captures.md §7).
5. Which topics are offered and in what order: hidden when the actor has no line; ties
   of equal topic priority; the conversation ending when no choice of a line can be
   said (no fallback to the topic list); the conversation ending when nothing can be offered; GOODBYE appended.
6. Known topics: permanent for the whole game? When is an Add Topics list applied
   (line start or end)? (That they are offered by every actor: D.)
7. Follow-up lines: the first qualifying of several (that they need no choice: E).
8. Timing of the begin and end results against the sound; the end result when the
   player skips a line or leaves; Run Immediately on a chosen line.
9. INFC/INFX: a "shared" line is taken as a line of the listing topic (§1.2); check it
   in game (Red Lucy's "What's this place?") and where the Info Index puts it. Her list's
   first three entries match ours [observed 2026-10-19], but a topic with its own line has
   the same prompt and the list could not be scrolled (owner-decision-captures.md §7).
10. Link From: taken as adding the topic to the links after the named topics' lines,
    answered by a line that names them (§3.5); check in game with Michael Angelo
    (031/032/034, and 004 after a greeting), Black Mountain Radio's RadioBMSegment12,
    and a child answering an NPC's hello.
11. The skill tag's numbers: taken from the line's condition? What is shown for a perk?
12. Inside the pause (B): what `IsTalking` reads while the player chooses;
    `Begin MenuMode 1009` blocks and the MenuMode quest delay; whether magic effects
    and other actors' SayTo lines stand still; a message box over a conversation; the
    quest conditions' subject and target (C).
13. A greeting with the Goodbye flag (G), what the statement leaves open: GetTalkedToPC
    afterwards; whether its Add Topics list is applied; SayToDone; activating the actor
    again while it speaks (cut and restart, or ignored); and whether a Goodbye line of
    a topic named by StartConversation opens the menu.
13. Say Once across two actors sharing a line (one case answered: not said again,
    observer check 31; built, #1113); Say Once a Day's clock; Say Once and
    Say Once a Day counted from the line's start or end.
14. Response order where numbers and stored order disagree.
15. Whether SayTo and Say obey Random stacks like menu dialogue.
16. Conversations between actors (§5.1): the walk to the target before the first line
    (not needed for Joe Cobb and Trudy, owner statement); whether the player is free
    and the game runs on during them; what Target, Self and Either mean; follow-ups in
    them; whether random conversations get a GOODBYE line added (a scripted one does
    not, owner statement); HELLO when no topic is named; SayToDone after their lines;
    refusal while either actor is talking.
17. Hellos and random conversations (§5.2): the hello distance and when an actor says
    hello again; whether the actor turns to the player; the meaning of the
    fAISocial… settings (timer per actor? chance per cent?) and the outdoor radius;
    the walk towards each other (does the partner wait, or do both walk? the distance
    they talk from; how long the starter tries); whether the player's presence matters for
    random conversations; general bit 12 as the switch between the default and the
    package's own Fallout behaviour (packages.md §1).
18. A chosen Goodbye line said on in the world (§5, issues #1606 and #2157): which
    chosen Goodbye lines close the menu at once in the original, if any (the runtime:
    none, every one holds the player until it is said; the switch closes all); whether it
    closes the menu at once also for a conversation through a talking activator; whether
    the end result runs when the line ends or as the menu closes; whether the actor stops
    to finish the line or walks on; what ends the line early (another line, a fight, the
    player out of earshot); whether a StartConversation from its end result waits for the
    line.
19. Fallout 3 percentage speech challenges in the New Vegas engine (§3.3, #3406): what
    the original shows on the choice row (exact `[Speech NN%]` wording, rounding vs
    truncation, whether chances above `fSpeechAutoSuccessThreshold` show `100%` or the
    raw percentage, and whether low chances use the red failure highlight), and whether
    `fSpeechChallengeFailureBase`/`fSpeechChallengeFailureMultiplier` have any runtime
    effect in New Vegas.

## 8. Tool contracts

`fnvtool dialogue <cmd> <Data> <plugins…> --schema schemas/fnv [--functions <file>]`.
Texts are game data: listings give lengths; `--text` adds the texts for reading at
home (output with texts is never committed).

**`dialogue topics [--quest Q] [--topic T] [--speaker A] [--top-level] [--type T]
[--all] [--text]` → `fnv.dialogue.topics/1`**: `load_order`; `summary` (the counts of
§1.3, `quest_mix` of §3.2, `subrecords`: per record type the signatures read, ignored
and unread over every plugin); `listed`; `topics`: for each selected topic `form`,
`edid`, `type`, `flags`, `priority`, `name_chars` (`name` with `--text`), `quests`,
`lines`, `line_quests`, `shared_infos`, `definitions`; with `--topic` also
`line_forms` (the order lines are tried in), `shared_info_list` and `removed_quests`.
Without a filter or `--all` only the summary is printed. `--quest` and `--speaker`
select topics that have such lines.

**`dialogue lines [--quest Q] [--topic T] [--speaker A] [--info <hex>] [--all]
[--text]` → `fnv.dialogue.lines/1`**: `summary` (the counts of §2.2,
`previous_info` of §3.1 and `quest_conditions` of §4); `lines`, topics by FormID and within a topic in the order
lines are tried: `form`, `plugin`, `definitions`, `topic`, `position`, `quest`,
`quest_priority`, `type`, `next_speaker`, `flags`, `record_flags`, `speaker`,
`prompt_chars` (`prompt`), `responses` in the order said (`number`, `chars`, `emotion`,
`emotion_value`, `sound`, idles, `seconds_fitted`; `text`, `notes`), `conditions`
(function, arguments, comparison, value, OR, run-on), `begin` / `end` (unit keys),
`choices`, `link_from`, `follow_ups`, `add_topics`, `challenge`, `previous`
(`"absent"`, null or a FormID). `--speaker` takes an actor base or a placed reference
and selects lines whose fixed speaker it is.

**`dialogue talk --actions <file> [--text]`: `fnv.dialogue.actions/1` →
`fnv.dialogue.trace/1`.** The input has the fields of `fnv.script.actions/1`
(scripts.md §8): `seed`, `frame_seconds`, `new_game`, `given` (the game state the
conversation runs on) and `actions`, any of that contract's operations plus:

| op | does |
|---|---|
| `talk {actor, topic?, label?}` | `Game::start_dialogue`: the player activating the actor, or the actor's dialogue package; observation `info` |
| `choose {topic \| prompt \| index, max_frames?, label?}` | waits until the actor has finished speaking, then picks the entry by topic (Editor ID or FormID), by its text (ignoring case; a part of it if no entry matches whole) or by 0-based index; observation `info` |
| `wait_talk {start?, max_frames?, label?}` | waits until choices are offered or the conversation is over; with `start`, first until one is running (a script's StartConversation); observations `state` (`choosing`, `ended`), `choices`, `actor` |
| `end_talk {}` | `Game::end_dialogue` |

`max_frames` defaults to 7,200. Another menu open while waiting (`Game::menu`, a message
box over the conversation) is an error (the input has to close it). During a
conversation `get_menus` and `wait_menu` read menu 1009 as open, and the stopwatch
(script time) stands still. The same operations are accepted by `fnvtool script run`.

The trace: `format`, `seed`, `frame_seconds`, `title`, `observations`, `steps`,
`given_errors` as in `fnv.script.trace/1`; `events`, each with its `frame`, of these
kinds only, in the order they happened:

| kind | fields |
|---|---|
| `dialogue_started` | `actor`, `topic`, `topic_edid` |
| `line_started` | `speaker`, `target`, `topic`, `topic_edid`, `info`, `seconds`, `chars` (all responses), `responses`, `flags` (also for SayTo and Say lines) |
| `response_said` | `speaker`, `info`, `number`, `chars`, `emotion`, `seconds` (`text` with `--text`); conversation lines only |
| `result_script` | `unit` (`… INFO 0x… begin` / `end`), `on` |
| `line_ended` | `speaker`, `info`, `cut` |
| `choices_offered` | `actor`, `choices`: `topic`, `topic_edid`, `info`, `chars`, `prompt`, `goodbye`, `darkened`, `challenge` {`form`, `difficulty`, `needed`, `have`} (`text` with `--text`) |
| `choice_made` | `actor`, `index`, `topic`, `topic_edid`, `info` |
| `topic_added` | `topic`, `topic_edid`, `by` (the line; absent for AddTopic) |
| `dialogue_ended` | `actor`, `reason` (`goodbye`, `no_choices`, `left`, `actor_gone`), `lines` |
| `no_line` | `speaker`, `topic` |
| `stage_set`, `menu_opened`, `menu_closed` | as in `fnv.script.trace/1` |

and `final`: `frames`, `dialogue` (the conversation still running, or null), `topics`
(known topics), `quests`. The same events appear in `fnv.script.trace/1`, whose `final`
gains `dialogue`.

## 9. The intro walk-through's conversations

`crates/fnv-game/walkthrough/intro.actions.json` (scripts.md §9) now talks:

- **The psych test.** Stand-ins: objective 40 completed and the couch trigger's
  `bRunTimer` set (its OnTrigger needs IsCurrentFurnitureRef: sitting is AI). From
  there the trigger's own script counts 2 s, sets `bGiveTest` and calls
  `StartConversation player`; Doc greets with the test's opening line (begin: the test
  has begun; end: `bGiveTest` 0), and fourteen `choose` steps answer six word
  associations, five statements and three inkblots; each answer's begin script adds to
  the scores; the last line (Goodbye) sets `bDoFinalCalculation` at its start and stage
  85 at its end. 15 lines; tag skills Guns, Science, Sneak as before. The test up to
  the last answer happens in paused time (real time passes for the lines, script time
  does not). That last line is the fourteenth `choose` (15 lines: the opening line and
  fourteen answers), a chosen Goodbye line: it
  holds the player until it is said (§5, issue #2157) and stage 85 is set by its end
  script as the conversation ends. VCG01Test's quest script does the tag-skill
  calculation in GameMode once `bDoFinalCalculation` is 1, which that line's begin
  script sets: the calculation runs in the first GameMode frames after the line, still
  before stage 90 opens the tag-skill menu 1 s later [design; the real-data test
  `dialogue_data.rs` checks the tag skills and stages]. (Between #1606 and #2157 the
  line closed the conversation at once and the calculation ran while Doc finished it in
  the world; as long as conversations did not pause, while it was being said. The
  result is the same.)
- **The farewell.** Stand-in: `talk` for Doc's dialogue package after stage 115 (and 4 s
  for his walk). Then the game's: the greeting that returns the player's things,
  two follow-up lines (the Pip-Boy with its message, the suit by the player's sex), the
  player's one answer, its follow-up, the Goodbye line whose end script starts
  VGenericTimer's event 3 → the hardcore box → VCG01 200. 5 lines, in paused time;
  nothing in it waits on GameMode.

Stand-ins that remain in the walk-through and why: the menus' OK and the hardcore
answer (the player's); walking into the two triggers (physics sensors); sitting on the
couch (furniture, AI); Doc's dialogue package for the farewell (AI packages); and all
of VCG02/VCG03: Sunny Smiles's and Trudy's lines are still `say_info` / `say_first`
inputs, because between them stand travel packages, combat and the crafting menu, and
because her conversations start from packages and activations that part of the
walk-through does not set up (the player's position, her travel). The real-data test
`crates/fnv-game/tests/dialogue_data.rs` runs the walk-through and checks both
conversations and the end at VCG01 200.
