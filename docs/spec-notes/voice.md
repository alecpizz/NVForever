# Spec note: dialogue voice files

Clean-room description in plain language (docs/design/dialogue.md step D3). The reader is
`crates/fnv-voice`, playback is `engine/nvf-play/src/voice.rs`. Confidence tags:
**[documented]** cites a public source, **[observed]** was checked on the owner's install
(FNV 1.4.0.525, Steam, English, the ten masters and their archives), **[hypothesis]**
needs verification.

How the observations were made (2026-10-10): the file listings of all archives
(`fnvtool bsa list`) were matched against the DIAL, INFO, QUST, VTYP, NPC_, CREA and TACT
records of the ten masters, first with throw-away scripts and then by
`fnvtool voice coverage` (`fnv.voice.coverage/1`), which reproduces every count below
and is repeated as a test (`crates/fnv-voice/tests/real_data.rs`, run when
`NVFOREVER_DATA` is set). No listing, sound or text is kept in the repository.

Sources: xEdit record definitions as recorded in `schemas/fnv/gameplay.json` and
`placed.json` (field names and offsets); RFC 3533 (the Ogg container); the Vorbis I
specification (xiph.org); Microsoft's "Multimedia Programming Interface and Data
Specifications 1.0" (RIFF WAVE); GECK wiki Category:Dialogue as quoted in
quests-dialogue.md §4.

## 1. Where the files are

- Every voice file is in an archive, under `sound\voice\`. No loose voice file exists in
  the owner's `Data` folder [observed].
- 122,032 voice files in five archives [observed]:

  | Archive | Plugin folder | `.ogg` | `.lip` | `.wav` |
  |---|---|---|---|---|
  | `Fallout - Voices1.bsa` | `falloutnv.esm` | 52,896 | 52,581 | 40 |
  | `DeadMoney - Main.bsa` | `deadmoney.esm` | 2,795 | 2,558 | – |
  | `HonestHearts - Main.bsa` | `honesthearts.esm` | 2,277 | 2,263 | – |
  | `OldWorldBlues - Main.bsa` | `oldworldblues.esm` | 2,199 | 2,177 | – |
  | `LonesomeRoad - Main.bsa` | `lonesomeroad.esm` | 1,123 | 1,123 | – |
  | total | | 61,290 | 60,702 | 40 |

  `Fallout - Voices1.bsa` holds nothing else. The add-ons keep their voices in
  `<plugin> - Main.bsa`, next to meshes and textures; their `<plugin> - Sounds.bsa` hold
  `sound\fx` and `sound\songs` only [observed]. Gun Runners' Arsenal and the four pack
  plugins have no voice files; they add no dialogue line of their own (Gun Runners'
  Arsenal overrides ten base-game lines, §3.4) [observed].
- One more file sits under `sound\voice`: `sound\voice\empty.txt` in four of the add-on
  `Main` archives (a placeholder; not a voice file) [observed].
- The archives are found by the usual precedence (bsa.md, "Precedence"); no voice path
  is in more than one archive [observed].

## 2. The path

`sound\voice\<plugin>\<voice type>\<quest>_<topic>_<form id>_<response>.<ext>`, all in
lower case in the archives [observed].

Of the 61,290 sounds, 61,109 are named exactly this way by a response of a line of the
ten masters; the other 181 are leftovers (§3.3). The parts:

### 2.1 Folders

- `<plugin>`: the file name of the plugin that owns the line's FormID, with its
  extension (`falloutnv.esm`) [observed: all five folders; every sound of an add-on line
  is in the add-on's folder, also when the line sits in a base-game topic such as
  GREETING].
- `<voice type>`: the Editor ID of a voice type record (VTYP). All 130 voice folders
  (81 base game, 6, 14, 24 and 5 in the add-ons; 129 different names) are Editor IDs of
  VTYP records; 24 of the 153 voice types have no folder [observed].

### 2.2 Quest and topic

`<quest>` is the Editor ID of the line's quest (INFO `QSTI`), `<topic>` the Editor ID of
the topic (DIAL) whose "topic children" group holds the line [observed]:

- When `<quest>_<topic>` is at most 26 characters long, both are kept whole
  [observed: 15,142 sounds, among them 12,020 whose quest ID is longer than 10
  characters; 5,700 of those are exactly 26 long].
- Otherwise the quest ID is cut to its first 10 characters and the topic ID to what is
  left of the 26 (15 characters after a 10-character quest, more after a shorter one)
  [observed: 45,990 sounds; with a quest ID longer than 10, totals of 27 characters
  (1,221 sounds) are already cut; a quest ID of 5 characters leaves the topic 20].
- Nothing else occurs: of the 61,142 sounds whose FormID is a line of their plugin and
  whose name has the pattern's shape, 61,132 follow these two rules and ten carry a stale
  name (§3.3) [observed]. Why 26 and 10 is not known [hypothesis: a fixed-size name
  buffer in the original tools].

Different topics of one quest can share the cut text; the FormID keeps the names apart.

### 2.3 FormID and response number

- `<form id>`: the line's FormID as eight lower-case hex digits **with the load-order
  byte zeroed**: add-on lines (FormIDs 01…–04… in the default load order) are stored as
  `00……` [observed: all 5,647 different FormIDs in add-on file names start with `00`]. So a file
  name does not depend on where its plugin loads.
- `<response>`: the response number of the INFO's `TRDT` (byte 12; xEdit "Response
  number") in decimal, without padding [documented: the field; observed: as the last part
  of the name]. Numbers run from 1 to 31; 120 sounds have a two-digit number [observed].
  The number is not the position in the record: 874 of 28,887 lines with responses store
  them in another order than 1…n, or skip numbers (for example 2, 1) [observed].

### 2.4 Extension

`.ogg` for the sound, `.lip` for its lip-sync data (same name), and for 40 base-game
sounds a `.wav` copy next to the `.ogg` (§4).

## 3. Lines against files

28,896 lines (INFO, winning definitions) with 37,403 responses; 9 lines have no response
[observed].

### 3.1 How many resolve

A response "has a sound" when at least one voice folder of its plugin holds its `.ogg`
[observed]:

| Plugin | Lines | Responses | With a sound | Share |
|---|---|---|---|---|
| FalloutNV.esm | 23,247 | 29,674 | 28,767 | 96.9 % |
| DeadMoney.esm | 2,126 | 2,765 | 2,729 | 98.7 % |
| HonestHearts.esm | 1,236 | 1,664 | 1,653 | 99.3 % |
| OldWorldBlues.esm | 1,620 | 2,165 | 2,134 | 98.6 % |
| LonesomeRoad.esm | 667 | 1,135 | 1,120 | 98.7 % |
| all ten masters | 28,896 | 37,403 | 36,403 | **97.3 %** |

For the 22,589 responses of lines with a fixed speaker (INFO `ANAM`), the file can be
checked in that speaker's own voice folder (§7): 22,252 have it there, 317 have no file
in any voice, 15 have a file only in another voice's folder, and 5 belong to speakers
(creatures) without a voice type [observed].

### 3.2 Several voices, or none

- A response has one file per voice type that can say it, each in that voice type's
  folder. No file is shared between voice types: there is no common folder and no
  redirection [observed]. 31,861 responses have a sound in one voice folder, 4,542 in two
  or more (lines many actors share; up to 32 voices) [observed].
- 1,000 responses have no sound in any voice. They are spread over 108 quests, most in
  test and debug quests (`vDialogueTestaclesDebug` 289, `VDialogueTEST` 64), robots and
  radios (`HD00RobotsDialog` 50, `RadioBlackMountain` 38) [observed]. 165 of them name a
  sound form instead (§6). What the game does for a line without a file (how long it
  shows the subtitle) is [hypothesis: not captured]; the runtime keeps its fitted length
  (§8).

### 3.3 Sounds no line names

181 of the 61,290 sounds [observed]:

- 138 carry a FormID that is no line of their plugin (lines deleted after recording);
- 33 carry the FormID of a line but another name: 10 with the quest and topic the line
  had before it was moved (for example a Freeside line recorded under its old quest), and
  23 with a response number the line no longer has;
- 10 have names outside the pattern (suffixes such as `_alt`, `_og`, `old`, a trailing
  space, a missing response number).

The reader never finds these, as the game presumably does not [hypothesis].

### 3.4 Lines a later plugin overrides

37 base-game lines are redefined by a later master (Dead Money 25, Gun Runners' Arsenal
10, Honest Hearts 2). All 37 have their sounds in the base game's folder; the overriding
plugins' folders hold none for them [observed]. The
reader looks in the folder of the plugin that owns the FormID only
[hypothesis: whether the game would prefer a file in the overriding plugin's folder is
untested, since none exists].

## 4. File types

- **`.ogg`**: Ogg Vorbis. All 61,109 sounds that lines name were read: 51,927 at 24,000
  Hz, 7,842 at 22,050 Hz, 1,162 at 48,000 Hz and 178 at 44,100 Hz; 59,674 mono and 1,435
  stereo; together 253,944 s (70.5 hours); the longest 40.85 s, the shortest 0 samples
  [observed].
- **`.wav`**: 40 files, all in `maleuniquepresident` (the `VHDKimballSpeech` quest), each
  next to an `.ogg` of the same name, with the same sample rate, channel count and
  length as its `.ogg` (within 1 ms) [observed]. The one looked at closely is 16-bit PCM
  with a `bext` chunk before `fmt ` [observed]. Which of the two the game plays is [hypothesis: unknown]; the reader takes
  the `.ogg` [decision: same length, and the engine's audio decodes Ogg Vorbis].
- **`.lip`**: lip-sync data, one per sound for 60,702 of the 61,290 sounds; none without
  its sound [observed]. Its layout and what it drives: docs/spec-notes/lipsync.md
  (dialogue step D6; reader `fnv_voice::lip`).

## 5. Length from the headers

Nothing is decoded to measure a file (`fnv_voice::length`):

- **Ogg Vorbis.** An Ogg file is a chain of pages: the capture pattern `OggS`, a version
  byte, a header-type byte, a 64-bit granule position, the stream serial number, a page
  sequence number, a checksum, a segment count and table, then the segments
  [documented: RFC 3533 §6]. The first packet is the Vorbis identification header: packet
  type 1, the letters `vorbis`, a 32-bit version (0), the channel count (1 byte) and the
  sample rate (32 bits) [documented: Vorbis I specification §4.2.2]. A page's granule
  position is the count of samples decoded when the last packet ending on that page is
  done; −1 means no packet ends on the page [documented: Vorbis I §A.2, RFC 3533 §6].
  **Length = granule position of the last page that has one ÷ sample rate.** Checksums
  are not verified; a file cut short keeps the last whole page's count.
- **WAV.** RIFF chunks; `fmt ` gives format, channels, sample rate, bytes per second and
  block align, `data` the samples [documented: RIFF WAVE]. PCM: `data` size ÷ block align
  ÷ sample rate. Other formats: the `fact` chunk's sample count when present, else `data`
  size ÷ bytes per second. Chunks before `data` (such as `bext`) are skipped.

Check [observed]: both readers give 125,359 samples (5.223 s) for the first Kimball line's
`.wav` and `.ogg`; the 40 pairs agree.

## 6. The sound field of a response

- `TRDT` bytes 16–19 name a sound form (xEdit "Sound", SOUN) [documented: xEdit,
  `gameplay.json`]. 228 responses of the masters set it, pointing at 97 SOUN records,
  whose file name is a folder for 108 of the responses, an `.mp3` for 111 and a `.wav`
  for 9 (among them creature and robot noises) [observed]. 63 of the 228 also have
  a voice file [observed].
- The editor describes it as an optional sound played instead of the voice
  [documented: GECK wiki Category:Dialogue, quests-dialogue.md §4]. Whether it replaces
  the voice file when both exist, and how long such a line lasts, is [hypothesis: not
  captured]. The reader reports the form (`sound_form` in `fnv.voice.line/1`) and does
  not play or measure it.
- `SNDD` ("Unused" in xEdit) occurs in no INFO of the masters [observed].

## 7. Who speaks with which voice

- An NPC's or creature's voice type is its `VTCK`; a talking activator's (TACT) is its
  `VNAM` [documented: xEdit "Voice" / "Voice Type"]. All 4,220 NPC_ records, 1,542 of
  2,235 creatures and 101 of 132 talking activators have one [observed].
- An actor whose template supplies its Traits (`ACBS` template flags bit 0, with a
  `TPLT`) takes the template's voice type, as it takes race and sex
  [documented: xEdit hides these fields under the flag, actors.md §1]. When the template
  is a levelled list the actual actor is chosen at run time. The game (`Game::voice_base`,
  and the engine's playback) uses the voice type of the entry picked for that reference,
  the same entry whose race and sex it shows and that `GetIsVoiceType` reads
  [hypothesis: an actor speaks in the voice of the sex it shows; owner's playtest
  2026-10-30, generic female actors spoke with male voices while the record's own voice
  type stood in, issue #1051]. Tools that see only a base form (`fnvtool voice`,
  `VoiceIndex::voice_of`) still use the record's own voice type there.
- The game chooses the folder from the speaker's voice type at run time. The reader does
  the same and does not fall back to another voice's file (§3.1's 15 responses then have
  no sound) [hypothesis]. A recording is the exception: a Voice note's response missing
  from its speaker's folder is taken from the line's fixed speaker's (ANAM), the one case
  observed being the Dog Command Tape (holotapes.md §3; #3022) [hypothesis].

## 8. Line length in the runtime

`fnv-game` has a hook (`fnv_game::voice::VoiceLengths`, `Game::voice`): when set, a
response lasts as long as its voice file for the speaker's voice type; a response
without a file, and every response when the hook is not set, keeps the fitted length
(1.1 s + 1/16 s per character, scripts.md §7.7). Every response is timed in one place
(`Game::response_seconds`), so SayTo and Say lines (which last the sum of their
responses) and the lines of a conversation (docs/spec-notes/dialogue.md §5) follow alike.
`fnv_voice::VoiceBank` implements the hook; `fnvtool script run` and `fnvtool dialogue
talk` set it by default (`--no-voice-lengths` or `FNVTOOL_VOICE_LENGTHS=0` keep the
fitted length) and `nvforever --play` sets it once the index is built
(`NVF_PLAY_VOICE_LENGTHS=0` keeps the fitted length).

**Placeholder files** [design, #4825]: a file under 0.25 s that is also under a quarter
of the response's fitted length (`fnv_game::voice::is_placeholder`) is a stand-in, not a
reading, and the response keeps its fitted length. The "{no VO}" description lines ship
with such files: Rex's examine branch (INFO 0015BA61, 89 characters, a 0.12 s file
[observed: a player report's log, R-T37VBHCR]) was gone in 0.12 s before the text could
be read, and now stays its fitted 6.7 s; a click still skips it. The file still plays.
`fnvtool voice coverage --lengths` counts these files (`placeholder_sounds`, with up to
20 examples). Whether the original keeps such lines up for their text's length is
[hypothesis], for an observer check.

**Doc Mitchell's intro lines** (the 23 he says by SayTo in the headless walk-through,
`crates/fnv-game/walkthrough/intro.actions.json`; all are single responses in
`maleuniquedocmitchell`) [observed: file headers]:

| Line (INFO) | Characters | Fitted s | Sound s | | Line (INFO) | Characters | Fitted s | Sound s |
|---|---|---|---|---|---|---|---|---|
| 0x00104BEE | 29 | 2.91 | 2.61 | | 0x00104BFC | 34 | 3.23 | 2.78 |
| 0x00104BEF | 63 | 5.04 | 5.59 | | 0x001074A2 | 122 | 8.72 | 9.55 |
| 0x00104BF0 | 53 | 4.41 | 3.04 | | 0x00104BED | 102 | 7.47 | 5.79 |
| 0x00104BF1 | 77 | 5.91 | 5.87 | | 0x0010749C | 35 | 3.29 | 1.95 |
| 0x00104BF2 | 92 | 6.85 | 5.70 | | 0x001055AD | 84 | 6.35 | 3.50 |
| 0x00104BF3 | 41 | 3.66 | 3.14 | | 0x001055AE | 117 | 8.41 | 6.25 |
| 0x00104BF6 | 113 | 8.16 | 6.97 | | 0x0010586A | 56 | 4.60 | 3.07 |
| 0x00104BF7 | 88 | 6.60 | 5.77 | | 0x0010586B | 79 | 6.04 | 3.98 |
| 0x00104BF8 | 11 | 1.79 | 0.88 | | 0x0010586C | 99 | 7.29 | 6.09 |
| 0x00104BF9 | 58 | 4.72 | 3.61 | | 0x001057EE | 38 | 3.48 | 2.35 |
| 0x00104BFA | 84 | 6.35 | 7.03 | | 0x001057EF | 31 | 3.04 | 1.48 |
| 0x00104BFB | 94 | 6.97 | 6.44 | | all 23 | | 125.29 | 103.45 |

**Stage timings** (seconds from the stage before, headless walk-through; capture:
scripts.md §9, layer5-captures.md §10.2–10.3). `nvforever --play --new-game` gives the
same steps within 0.05 s for both lengths (docs/design/layer5-play.md §6.2, §6.4),
except 60 → 65, which its test aid times:

| Span (lines) | Capture | Fitted | Sound | Sound − capture |
|---|---|---|---|---|
| VCG01 3 → 5 (1 line) | 2.9 | 2.90 | 2.60 | −0.3 |
| 8 → 10 (3 lines) | 15.4 | 15.37 | 14.52 | −0.9 |
| 15 → 25 (2 lines) | not timed | 10.50 | 8.85 | |
| 25 → 30 (2 lines) | not timed | 14.77 | 12.77 | |
| 35 → 36 (1 line) | a line | 1.78 | 0.87 | |
| 40 → 45 (2 lines) | not timed | 11.13 | 10.70 | |
| 50 → 55 (2 lines) | about 10 | 10.20 | 9.22 | about −0.8 |
| 60 → 65 (1 line; trigger by input) | about 10 | 8.75 | 9.57 | about −0.4 |
| 70 → 75 (1 line) | not timed | 7.47 | 5.78 | |
| 79 → 80 (3 lines) | about 13 | 18.07 | 11.73 | about −1.3 |
| 95 → 98 (3 lines) | not timed | 17.93 | 13.15 | |
| 105 → 110 (1 line) | not timed | 3.47 | 2.35 | |

What this shows [observed]: the sound lengths remove the fit's large errors (the psych
intro, 18.1 s against about 13 s), and every timed span now comes out **short by about
0.3 to 0.4 s per line** (−0.3 for one line, −0.9 for three, about −0.8 for two, about
−0.4 for one, about −1.3 for three). The fit had absorbed that. The cause is [hypothesis]: a pause the game leaves
after a sound before the end result script runs, or the time it takes to start one.
Nothing was tuned to close it: it needs a capture that times single lines (owner review;
docs/design/dialogue.md §6 question 4). Which stage follows which, and by what, is unchanged. The
walk-through's conversations with Doc Mitchell (the psych test and the farewell, 20
more lines) run on the sound lengths as well: all 44 lines of the run take 219.1 s
instead of 271.7 s.

**Lab.** The four `L5.intro.*` scenarios pass with both lengths, with identical
observations: their timed steps are timers, not spans of lines [observed].

## 9. Playback (`nvforever --play`)

`engine/nvf-play/src/voice.rs`: each response of a line being said (`Game::speech`:
SayTo and Say; and the line of a conversation, `Game::dialogue`, once the dialogue
screen starts conversations in the game) has its file read from the archive and played
through Bevy's audio when it starts, the responses in response-number order; subtitles
switch with the responses; a line that is cut (another line by
the same speaker, the speaker disabled, deleted or dead) has its sound stopped. One log
line per sound: `voice: playing <path> (<seconds> s) <speaker> line <INFO> response <n>`;
a response without a file logs `voice: no file ...`. `--no-voice` plays nothing (lengths
and log stay), `--voice-volume <x>` scales the volume.

A sound is held with its line's timer (2026-10-10, `voice::line_held`): a SayTo or Say
line stands still in menu mode (the inventory, a container, a message box, a
conversation with the player), so its sound is paused there and resumed after; a line of
the conversation itself goes on in real time and is held only by a message box on top
of it; the pause menu holds every line. Before, the sound played on while the timer
stood still, ran ahead of it, and the subtitle outlasted the sound after the menu.

**Heard where it is said** (2026-10-11, owner request after a playtest: greetings
between NPCs and alerted enemies were heard "like they are stood right next to me"; "if
the NPC is out of hearing range we shouldn't show the subtitle"). A line said outside
the conversation with the player (SayTo, Say, hellos, lines between actors, detection
barks) is heard from its speaker's place, 110 units above its feet [design]: full
volume within 600 units of the player's eye, falling as 600 ÷ distance beyond it and
faded to nothing at 2,000 (`voice::voice_gain`; the shape is `sfx.rs`'s for sound
effects [design]), and panned to its side (`sfx::pan_gains`), both followed while it
plays. The two distances are `fDialogMinDistance=600.00` and `fDialogMaxDistance=2000.00`
of the [Audio] section of the install's `Fallout_default.ini` [observed; hypothesis:
that they bound dialogue's attenuation, as the names say; nothing documents them]. A
speaker in another cell space (another interior, or inside while the player is
outside) is not heard. While a line is out of hearing (2,000 units or more, or another
cell) it is silent and its subtitle is not shown (`voice::audible`,
`script_hud.rs`); both come back if the two come nearer while it lasts. The
conversation with the player stays flat and whole. Log: `voice: playing … at <d> units,
gain <g>` and `[out of hearing, …]`; at exit `voice summary: … N started out of the
player's hearing`. Checked: Violet's NormalToAlert from 3,018 units, the Fiends'
lines near Cook-Cook from 2,268 units, no subtitle; from 1,596 units gain 0.11 and a
subtitle (alert.md §8).

**President Kimball's speech at Hoover Dam is heard beyond that range in the original**
(#2750, owner 2026-11-15) [observed: owner, base game]: from the vertibird drop-off and
from the sniper tower. Ours is silent there (`gain 0.00 [out of hearing, 2088 units
away…]` in the owner's log), and already at 0.43 of full level 1,000 units out. What the
data holds [observed: FalloutNV.esm, `fnvtool dialogue lines --quest VHDKimballSpeech`,
`fnvtool ai show`, `fnvtool script decompile`, `fnvtool world radio`]:

- The speech is 42 INFO lines of quest `VHDKimballSpeech` (0x00130C78), 40 in topic
  `VHDKimballSpeech01` (0x00130C6D) and 2 in `VHDKimballSpeech02`, all of type
  Conversation, flagged Say Once (two Goodbye). No response names a sound (SNDD); the
  fixed speakers are `VHDKimball` (4 lines) and `VHDColonelMoore` (2), the first line
  (0x00140A1A) has no fixed speaker and a GetIsID VHDKimball condition. Record flags 0.
- No script names either topic: no Say or SayTo carries the speech, so Say's `is2D`
  does not enter. Kimball stands at the podium on `VHDKimballSpeechPackage`
  (0x001377CC, a Travel package to `VHDKimballSpeechMarkerREF` 0x00130C75, Must
  Complete, Head-Tracking Off; its begin script only sets `bSpeechFailsafe`).
- No radio station or talking activator carries it (the only Hoover Dam radio is
  `vHDVintageRadioNVStartON` on the NCR emergency station).
- So nothing in the records marks these lines as louder or farther-carrying than any
  other line between actors.
- Distances from the podium marker (68098, 22628, 4579): the Ranger's place on the
  sniper tower, `VHDKimballRangerAmbushedRef` 0x0014280C at (68346, 26101, 5277), is
  3,551 units away; Kimball's own start by the vertibird, `VHDKimballREF` at (67300,
  19299, 5100), is 3,463.

With nothing special in the data, the owner's observation says a line between actors is
heard at 3,500 units in the original, so the 600 / 2,000 curve above is wrong, at least
in its cut to nothing at `fDialogMaxDistance` [hypothesis: for every such line, since
the records do not tell these apart]. What the level is at each distance is not known:
observer check 205 (observer-checks.md) measures it. The curve is not changed until
then.

Reverb (2026-11-01, #987): a line heard where it is said plays through the room's
reverb, cut at the line's end, at a send of 0.85 from `fDialogReverbAttenuation=0.15`
[hypothesis: the unit is undocumented]; the conversation stays dry (audio.md §14.6).

Not done [decision]: the response's sound form (§6) and emotions; occlusion. Lip sync follows the voice from
`engine/nvforever/src/lips.rs` (`VoiceState::spoken`; docs/spec-notes/lipsync.md §5),
and holds with it.

### 9.1 One cry of a long generic death file

[design, #1539] FalloutNV.esm has three Generic Death lines (quest `Generic`, topic
`Death`, INFO 00175180 to 00175182, flag Random, one response each) [observed: `fnvtool
dialogue lines --topic Death --quest Generic`]. Each has files in 22 voice types, of 0.68
to 4.08 s, except MaleAdult07's of 00175181 and 00175182, which last 9.80 and 15.23 s and
hold several cries back to back [observed: `fnvtool voice line`; that the short files are
one cry each: hypothesis, checked on three below]. Played
whole, a Powder Ganger killed in Goodsprings groaned on for 10 to 15 s as a ragdoll
[observed: playtest 2026-11-05, issue #1539]. Owner design: start at a random point so a
different cry is heard each time, and cut after one cry; named NPCs' death lines are not
cut.

The rules (`fnv_voice::cry`, played by `nvf-play`'s `voice.rs`):

- **Which files.** A file whose name is `<quest>_death_<FormID>_<n>` with a quest ending
  in `generic` (`cry::is_generic_death`), longer than 5.0 s (`cry::LONG_SECONDS`), of a
  line with one response, said outside the conversation with the player. Every other
  file plays whole: the short one-cry files, and a named NPC's death line, which lives
  in its own quest's files. [design: the file name tells the generic line apart; a named
  NPC that says the Generic line with a voice type whose file is long is cut too.]
- **Split at silences.** The file is decoded and measured in 10 ms windows (RMS of all
  channels). A window below 5 % of the loudest window (about -26 dB), or below 0.002, is
  silence; silence of 0.25 s or more separates two cries, a shorter one (a breath) stays
  inside a cry; a loud run under 80 ms is a click and dropped. Each cry is played from
  30 ms before its first loud window to 150 ms after its last, never past the middle of
  the silence beside it. [design: all these numbers]
- **One cry.** One of the cries is picked at random (uniformly) and only it plays, with a
  40 ms fade-out. A picked cry still longer than 5.0 s (no silence found in it) plays
  1.5 s of itself from a random point, faded in and out over 40 ms.
- **The line ends with the cry.** The game's line (`Game::speech`) ends when the cry does
  (`Game::cut_line_after`), so its subtitle goes with it and the DeathResponse of a
  nearby group member comes then rather than after the whole file.
- **Lip sync** follows the file's `.lip` from the cry's place in the file and stops at
  the cut (`VoiceState::spoken` reports the file's start as the time the cry began minus
  its offset in the file).

Checked on the files [observed 2026-11-06, the split run on the decoded files]:
MaleAdult07's 00175182 (15.24 s) splits into 5 cries of about 1 s (0.98-1.92,
4.34-5.34, 7.78-8.74, 12.15-13.13, 13.97-15.08 s) and 00175181 (9.81 s) into 6 (0.37-1.69,
2.82-3.77, 4.21-4.80, 5.62-6.29, 7.16-8.00, 8.58-9.54 s); MaleGenericGhoul's 00175182
(2.53 s), FemaleGhoul02's (3.44 s) and MaleAdult02's (0.78 s) are one cry each and play
whole.

Log: `voice: one cry of <path>: cry <k> of <n>, <start>-<end> s of its <total> s; <who>
line <INFO> ends with it` (or `a stretch (no silence found)`); at exit the voice summary
counts `N long generic death files cut to one cry`. Smoke scenario `death-cry-cut`.

## 10. Tools

- `fnvtool voice line <data> <plugins…> --info <FormID> [--speaker <Editor ID|FormID>]`
  (`fnv.voice.line/1`): quest, topic, the file name of each response, every voice type
  that has it (path, archive, sample rate, length, lip file) and the file the speaker
  says it with (default: the line's fixed speaker); the line's length when every response
  has a file.
- `fnvtool voice coverage <data> <plugins…> [--lengths]` (`fnv.voice.coverage/1`): the
  counts of this note; `--lengths` reads every sound's headers (about 10 s).
- `fnvtool voice length <file…>` (`fnv.voice.length/1`): the headers of loose `.ogg` or
  `.wav` files.

## 11. Open

- The 0.3 to 0.4 s per line between a sound's length and the captured spans (§8).
- `.ogg` or `.wav` first when both exist (§4); the sound form of a response (§6); a line
  without any file (§3.2).
- Voice type of actors templated on a levelled list (§7); whether another voice type's
  file is ever used (§3.1).
- Whether the game looks in an overriding plugin's folder (§3.4), and whether loose
  files replace archive files for voices (bsa.md, "Precedence").
- Say's `is2D` and `isVoice` parameters (script-functions-catalogue.md) and distance
  attenuation: not read.
