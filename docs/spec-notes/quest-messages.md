# Spec note: the quest lines at the left (quest added, completed, objectives)

**NVForever's words** [design, #2984]: the original's labels are its program's own text,
not in the data files, so NVForever shows its own: "New quest", "Quest done", "Quest
failed" and, for a map marker found, "Place found" (`quest_messages::label`). The
original's words below are kept only to say where its lines stand.

Tags as in `nif.md`: `[documented]` cites a page or file, `[observed]` says what was
looked at, `[hypothesis]` is a reading that nothing has confirmed yet. `[design]` marks
what is NVForever's own. Implementation: `engine/nvf-play/src/quest_messages.rs` (the
queue, the layout, the drawing), `script_events.rs` (`QuestNotice`: what the script
events become), `script.rs` (`PlayCtx::quest_listed`), `interface.rs` (fonts, pictures).

## 1. What this is

When a quest is added, completed or failed, the original shows "Quest added" (or
"Quest completed") in small letters with the quest's name under it in large capitals,
at the left of the view, a little above the middle. When a script shows or completes an
objective, the objective is listed in the same place behind a small square, filled for
a completed one, with "COMPLETED: " before its text [observed: the owner's screenshots].

Before this, NVForever put these lines in the corner messages at the top left
(`corner-messages.md`), which is not where the original has them. They now come here
only; the corner messages keep messages, items and default activations' lines.

## 2. Sources

- **Four screenshots from the owner**, 2026-10-11 (kept privately; the first two in
  `C:\fnv-private\references\quest_messages\`): "Quest completed / RING-A-DING-DING!"
  at 768 x 431 (a web capture), "Quest added / COME FLY WITH ME" at 1280 x 720 during a
  conversation, one objective shown ("Track down the source of the Sierra Madre
  invitation.") at 1280 x 720, and one completed objective with a new one under it
  ("COMPLETED: Talk to Sunny Smiles in the Prospector Saloon." / "Meet Sunny Smiles
  behind the Prospector Saloon.") at 1280 x 720. Every "[observed: screenshot]" is
  measured on them; the two objective screenshots reached the session only as pictures
  in the conversation, so their numbers are read by eye (to about a pixel).
- **The game's interface files** in `Fallout - Misc.bsa`: `menus\main\hud_main_menu.xml`
  and `menus\prefabs\hudtemplates.xml`. Described here in plain words; nothing of them
  is shipped.
- **The fonts** of `Fallout - Textures2.bsa` (`fonts.md`) and `Fallout_default.ini`'s
  `sFontFile_N` lines; the box pictures `textures\interface\shared\marker\
  glow_square_small.dds` and `glow_square_filled_small.dds`.
- **The GECK wiki** (the owner's export): the pages SetObjectiveDisplayed, SetStage and
  JohnnyGuitar's SetUIUpdateSound.
- **The records of `FalloutNV.esm`**, listed with `fnvtool esm inventory`.

## 3. What the files say

[documented: `hud_main_menu.xml`] The HUD has an area "QuestReminder", 400 wide and 200
high, with two areas inside: "QuestStages" (100 by 50, 40 down) and "QuestAdded" (100 by
150, 80 down), both in the HUD's colour. Their places in the view and their contents
are left to the program. Inside QuestReminder are two picture patterns, a filled square
(`Interface\Shared\Marker\glow_square_filled_small.dds`, 26 by 26) and an empty one
(`glow_square_small.dds`, 16 by 16), both 20 left of and 3 below the text they go with,
invisible at first. The atlas index names neither [observed], so their own files are
used: each is 32 x 32 texels with its square from texel 4 to 19 [observed].

[documented: `hudtemplates.xml`] The HUD's text patterns use font 7
(`Baked-in_Monofonto_Large`).

[documented: GECK wiki, SetObjectiveDisplayed] "If this is the first objective of the
quest to be displayed, the player will receive a 'Quest Added' message on screen."
[documented: GECK wiki, SetStage] the same for "the first stage of the quest to be added
to the Pip-Boy".

[documented: GECK wiki, SetUIUpdateSound] The game has four fixed interface sounds:
quest failed, quest added, quest completed, location discovered.

[observed: `FalloutNV.esm`] Sound records `UIPopUpQuestNew` (0x0001EA39),
`UIPopUpQuestComplete` (0x00022582) and `UIQuestUpdate` (0x0004FB02). No game setting
holds the words "Quest added" or "Quest completed" (the record list has none named so).

## 4. What the screenshots show

[observed: screenshots; lengths in the interface's units, 960 to the view's height:
0.75 pixels each at 1280 x 720, 0.449 at 768 x 431]

- **The words over the name** are font 7 at its own size: "Quest added" is 156 units
  across its lit pixels, "Quest completed" 211, as the font sets them (the glow aside;
  font 1 would fit too). Their letters start 52 to 53 units from the left edge and
  stand on a line 247 below the top, in both screenshots.
- **The name** is font 8 (`Glow_Futura_Caps_Large`), in capitals: the font has no small
  letters, and "RING-A-DING-DING!" is 367 units across, as the font sets it. Its
  letters start at 52 to 53 and stand on 312; their tops are at 273 to 274.
- **The name comes in from the left.** In the "Quest added" screenshot "COME FLY" is at
  full strength, "WITH" fainter and "ME" fainter still; in the "Quest completed" one
  the whole name is at full strength. So the name is drawn letter by letter from left to
  right, or fades from right to left [hypothesis: NVForever does the first].
- **Objectives** are font 7 too. The first line stands at about 263 (pixel 197 at
  720); the next lines are 38 lower, the font's 36 and 2, and a second objective
  follows straight on. The text starts at about 51; a wrapped line starts there too.
  The square's lit part is about 12 units, from 32 to 44 across and 245 to 257 down,
  the same size for the filled and the empty one.
- **Where a line breaks.** "Meet Sunny Smiles behind the Prospector" (about 557 units)
  stays on one line, and "Track down the source of the Sierra Madre" (about 590) does
  not. So the break comes between 557 and 590; NVForever uses 570 [hypothesis].
- **During a conversation** the lines are shown (the "Quest added" screenshot is in the
  dialogue view).

NVForever's frames at 1280 x 720 agree: "Quest added" on pixel row 184 starting at
column 39, the name on 233 at 39 (the owner's: 184 and 233, 39 and 39); the objectives
of the second objective screenshot break at the same words.

## 5. NVForever's rules

- **When.** An objective shown (SetObjectiveDisplayed ... 1) is listed. When it is
  the first of its quest to be shown (no other objective of the quest was shown or
  completed before), "Quest added" with the quest's name comes first
  [documented: SetObjectiveDisplayed]. An objective completed is listed with
  "COMPLETED: ". CompleteQuest and a stage's Complete Quest flag give "Quest
  completed"; a failed quest gives "Quest failed" [the words are NVForever's by
  analogy].
- **Not shown** [hypothesis]: objectives without text, and the banner of a quest without
  a name. SetStage's own "Quest Added" (for a stage added to the Pip-Boy) is not built:
  New Vegas's Pip-Boy lists objectives, and which stages count is not known.
- **One at a time** [design]. Lines wait in the order they came. Objectives that come
  before their list is on screen join that list when it is the last one waiting and of
  the same quest (so a completed objective and the next one show together, as in the
  screenshot); one completed while still waiting is shown completed, once. Another
  quest's objectives wait in a list of their own. A line that says what the one before it says is left out.
- **Times** [design]. "Quest added" and its kind stay 4 seconds; a list 4 seconds, and
  1.5 more for each objective after the first. Each comes and goes over 0.25 seconds
  (the HUD menu's fade time); the name's letters come one after another over 0.6
  seconds from the left.
- **Sounds** [hypothesis: by the records' names]. "Quest added" plays `UIPopUpQuestNew`,
  "Quest completed" `UIPopUpQuestComplete`, a list `UIQuestUpdate`, when the line comes
  on screen. "Quest failed" plays none (its record is not known).
- **Squares.** Both drawn 26 by 26 (the filled one's size in the HUD file), 16 left of
  where the text starts and 21 above the line it stands on, as measured; the empty
  one's 16 by 16 of the file would draw its square half as large as the screenshots.
- **Behind a message box or a script's menu** the area is hidden and its clock stands
  still, as the corner messages'.
- **Without the game's files** the text is drawn in Bevy's font and the squares as plain
  squares.

## 6. Not built

- Where the original puts the lines at other view shapes than 16:9 (all measurements are
  at 16:9).
- The original's exact times and its way of coming and going.
- "Location discovered" (the same fixed sounds name it; where its line is drawn is not
  looked at).
- The objective markers on the compass and in the Pip-Boy.

## 7. Checking

- Unit tests: `cargo test -p nvf-play quest_messages script_events` (in `engine`).
- By eye and by measure, at the screenshots' size:
  `NVF_PLAY_TEST_SCRIPT_MENUS=0.3 NVF_PLAY_TEST_SCRIPT=3:objective:VCG02:3,9:objective_done:VCG02:3,9.1:objective:VCG02:5`
  `nvforever --data <Data> --cell GSHouseInterior03 --play --no-voice --no-overlay --size 1280x720 --run-seconds 45`
  with `NVF_PLAY_TEST_SHOTS=4.2:<a.png>,8:<b.png>,15:<c.png>`. The log has, in this order,
  `quest message on screen: Quest added: BACK IN THE SADDLE`,
  `quest message on screen: Talk to Sunny Smiles in the Prospector Saloon.` and
  `quest message on screen: COMPLETED: Talk to Sunny Smiles in the Prospector Saloon. | Meet Sunny Smiles behind the Prospector Saloon.`
  (the add-on packs' quests are added before, between or after them, each quest's lines
  together), and the sound summary counts their sounds.
