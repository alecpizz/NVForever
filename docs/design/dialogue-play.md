# The dialogue screen in `nvforever --play` (step D4)

Status 2026-10-10: built on `claude/dialogue-screen` (#122); the owner's decisions on it
(one pause switch for all menus, the debug switch) on `claude/menu-pause`, §6.1; the two
turning to each other in a conversation (step D7) on `claude/dialogue-facing`, §8. The plan is docs/design/dialogue.md
(step D4, and its Pausing section); the rules a conversation follows are the runtime's
(`crates/fnv-game/src/dialogue.rs`, docs/spec-notes/dialogue.md §4–§6). This note is the
engine side: `engine/nvf-play/src/dialogue.rs`.

The keys are NVForever's own. The menu is drawn as the original's since 2026-10-11
(`dialogue_menu.rs`, docs/spec-notes/dialogue-menu.md); nothing here is a claim about the
original unless it carries a tag.

## 1. What happens

1. **Start.** E on a living actor runs its default action, talking
   (`fnv_sim::Outcome::Talk`): the conversation starts with the actor's greeting
   (`Game::start_dialogue`). The same happens when the actor's OnActivate block calls
   `Activate`, and when any script activates an actor for the player
   (`script::Runtime::talk_after` reads the frame's `activated` events). Scripts also
   start conversations themselves with `StartConversation` (Doc Mitchell's psych test),
   and an actor living by its packages (`--npc-packages`) starts one with its Dialogue
   package: the force greet, as Sunny Smiles's in the Prospector Saloon
   (docs/design/actor-behaviour.md §5.6).
   An actor with nothing to say starts nothing: the line "… has nothing to say" shows as
   before and the log says why (`dialogue: no conversation: …`).
   **An actor whose greeting has the Goodbye flag starts no conversation either** when
   the player talks to it: it says the line at the player and the player stays free to
   move, look and act [owner statement 2026-10-10; the runtime's rule, dialogue.md §5].
   **When the actor force-greets** (StartConversation, its Dialogue package) such a
   greeting is a conversation of one line: the screen opens and holds the player until
   the line is over (`Game::force_greet`) [owner statement 2026-10-11, Sunny Smiles's
   gecko line]. The line is heard
   and shown as a SayTo line is (the subtitle of `script_hud.rs`, the voice of
   `voice.rs`); the state stays `Playing`, nothing pauses and the view is not turned
   (§8 is for conversations). **The actor stops and faces the player while it says
   it**, also when its package had it on its way: the engine holds it where it stands
   (`Game::greets_player`, `residents.rs`), the AI turns it to the player (§9.3), and
   its package goes on when the line is over [owner statement 2026-10-27, of the
   Goodsprings settler's water line 0010B3D6 (VCG02GSSettler, walking home): "make her
   stop and look at the player", the player staying free]. **Such a line is never cut**:
   whatever stops it early (the speaker drawn into a fight (Alert, Combat, Lost), dying,
   disabled or deleted, talked to again, another line of its own) ends it at once as if
   said to the end, its end result script and Add Topics applying, so a reward is not
   lost (`Game::cut_line`) [owner statement 2026-10-27: "skip to the end of the line as
   if she finished speaking", "all the code runs", "for any reason as a catch all"]. A
   line still going on when the player fast travels or changes cells ends on its own
   time there. Log: `dialogue: <actor> says
   its greeting <line> at the player (a Goodbye line: no conversation)`;
   `script::ByHand::Greeted`.
   Checked 2026-10-10: `--cell Goodsprings --play --no-voice --run-seconds 40` with
   `NVF_PLAY_TEST_TALK="8:GoodspringsSettler04Ref=;18:EasyPeteRef=leave"`: the settler
   said 0x0015D8AF (2.21 s) with no `conversation … started` line and the run went on;
   Easy Pete's conversation then started and ended by Goodbye (2 lines); exit 0, "run
   finished".
2. **The actor speaks.** The response being said is shown on a dark band, the actor's
   name at the top right, for as long as the runtime says it (its voice file's length;
   `voice.rs` plays the file); the log has it as a `subtitle: Speaker: text` line, as
   the subtitles of SayTo lines (`script_hud.rs`).
3. **The player chooses.** When the actor has finished, the offered choices are listed
   between two brackets, as many as fit (a longer list scrolls with the selection; the
   number keys pick by place in the whole list). A choice the actor has answered before is dimmed (`Choice::darkened`:
   the runtime's rule [documented: GECK wiki Category:Dialogue, Info Refusal and Always
   Darken]). A skill or perk line carries its tag, "[Speech 40]", or "[Speech 37/40]"
   when the player falls short [documented: Category:Dialogue "AV/Perk"; where the two
   numbers come from is the runtime's hypothesis, dialogue.md §3.3]; the frame of one
   the player falls short of is red (dialogue-menu.md §3).
4. **The end.** A chosen line with the Goodbye flag (or the GOODBYE topic's line without
   it, #3477, dialogue.md §5) is said on the dialogue screen like
   the lines before it, the player held, and ends the conversation when it is over
   [owner statement 2026-11-11, issue #2157: the last line of a conversation must hold
   the player; said in the world, a quest's line can be interrupted in play]. The
   runtime keeps the rule of #1606 behind a switch that is off and that nothing here
   sets (`GameConfig::goodbye_at_once`, dialogue.md §5): with it a Goodbye choice ends
   the conversation at once and the actor finishes the line [observed: owner's playtest
   2026-11-05, against the original: choosing a Goodbye closes the menu at once and the
   actor finishes the line as the player walks away]. Then the screen
   goes as the line starts, the player is free and the game runs again; the line is
   heard where the actor stands and its subtitle drawn as a SayTo line's [design: a line
   said in the world], and its end script runs when it is over [hypothesis,
   dialogue.md §7 item 18]. The log says `dialogue: the conversation ends on the Goodbye choice; <actor>
   finishes the line <line> in the world`. A Goodbye line the player did not choose (a
   force greet's Goodbye greeting, a Goodbye follow-up) and any Goodbye line through a
   talking activator still end the conversation when they have been said; so does an
   actor with nothing more to offer (the runtime's rules).

## 2. Keys

| When | Input | Does |
|---|---|---|
| The actor speaks | Space, Enter, Activate (E), left click | skips to the next response (`Game::skip_response`); after the last one the line ends as if it had been said |
| The actor speaks (`living`) | mouse | looks around (the view turned to the actor when the conversation started and comes back onto them when the choices appear, §8.2) |
| The player chooses | 1–9 (also the number pad) | picks that choice |
| | Up / Down, W / S, mouse wheel | move the selection |
| | Enter, Space, Activate (E) | pick the selected choice |
| | left click on a choice (hovering selects it) | picks it |
| | Tab | says goodbye: picks the offered Goodbye choice; the actor says its goodbye line and the conversation ends when it is over (§1 item 4); at a topic list (the top level) without one, leaves it (#1595); refused at a line's own choices without one; refused when no Goodbye choice is offered |
| Any time | Esc | the pause menu (Resume goes back to the conversation) |

- The original leaves a conversation with Tab [hypothesis: the coordinator's brief of
  2026-10-10, from play; when it allows leaving is not captured]. Ours: Tab only where
  the data offers a goodbye, so a forced conversation (the psych test) cannot be left by
  accident. The topic list is never forced: one without a Goodbye is left by Tab, and the
  log names the actor (#1595: Julie Farkas's top level offers none; her Goodbye lines are
  all answers on sub-menus, and the generic GOODBYE lines' conditions leave out her voice
  type FemaleAdult11) [design]. Since #2378 a quest's conditions count for a voice type
  without Allow Default Dialog, and the Generic quest's list names FemaleAdult11: Julie's
  top level now offers Generic's Goodbye 0x00177392, so she is no longer a case of this
  (observer check 127 asks the original). The "TAB) Goodbye" hint stands there too.
- Breaking a conversation off (cutting the line, no end script) is not a key of the
  game: it is a debug switch, off by default (§4) [owner decision 2026-10-10].
- The cursor: free look while the actor speaks, the cursor released while the player
  chooses (§3) [owner decision 2026-10-10]; with the dialogue zoom the view is locked on
  the speaker and the cursor free throughout (§8.2) [owner decision 2026-11-13, #2254].
- A message box a result script shows during a conversation takes the keys first: it is
  drawn and answered as any other box (`script_hud.rs`), the conversation's lines wait
  for it (the runtime's rule), and the dialogue menu is hidden meanwhile.
- Skipping: the response ends at once and the next one starts; the voice of the
  skipped response stops (`voice.rs` stops a sound whose line moved on). The end result
  script still runs when the last response is skipped [hypothesis:
  `Game::skip_response`, dialogue.md §5; capture question §7.8].

## 3. State and pausing

While a conversation runs the game is in `PlayState::Dialogue` (`dialogue::sync_state`),
a third state beside `Playing` and `Menu`.

- **Held, as in the container and inventory menus and under a message box**: the
  player's movement and actions (activation, weapons, the inventory key), the player's
  character and the physics world (they step only while `Playing`), the game clock
  (`sim.rs`), combat (`combat.rs`), trigger blocks (`script.rs` skips the trigger test,
  so no OnTrigger runs every frame), and travelling NPCs (`--npc-travel`; `WorldHeld`,
  read by `engine/nvforever/src/nav.rs`: they stand and blend to their idle, since they
  are given no velocity). Scripts are in menu mode by the runtime's own rule
  (`Game::in_menu`: no GameMode blocks, game time and quest delays stand still, MenuMode
  blocks run with code 1009) [owner ruling 2026-10-10: a conversation with the player
  pauses the game; lines said outside one do not].
- **Running**: the conversation's own lines (`Game::tick` advances them in real time),
  subtitles and voice; and the two turning to each other (§8), which is presentation.
- **The picture** is one switch for every menu, `--menu-pause living|original`
  (`pause::MenuPause`; `--dialogue-pause` is its old name and still works;
  docs/design/dialogue.md §8):
  - `living` (default): everything that is only presentation runs on (actors' idle
    animation, wind in the trees, water, the sky's clouds, effects), nothing snaps to a
    frozen frame. A zoomed conversation keeps the view on the speaker and the cursor
    free throughout [owner decision 2026-11-13, #2254, §8.2]. Without the zoom the
    player looks around with the mouse while the actor speaks (`dialogue::FreeLook`);
    when choices are offered the cursor is released for clicking and the view holds
    still; it is grabbed again when the next line starts [owner decision 2026-10-10, of
    the two options offered]. [owner preference
    2026-10-10 for `living`: a deliberate departure from the original's look that keeps
    what scripts see.]
  - `original`: Bevy's virtual clock is paused while the menu is open
    (`pause::freeze_picture`), so animations, wind, water and clouds stand still too;
    the cursor is free throughout. This holds for the dialogue screen, the inventory
    and container menus, message boxes and the menus scripts open alike.
- **The Esc pause menu holds everything in both modes**: the picture (the virtual
  clock), scripts and the conversation (`script.rs` does not tick under it) and the
  voice of the line being said, which is paused and resumed with it (`voice.rs`).
- **What goes on while the picture stands still** reads `pause::AidTime` (the virtual
  clock plus the real time it stood still): the scripts' frame time (so the
  conversation's lines and MenuMode blocks run in real time under an `original` menu),
  every timed test aid and the menus' timers. `--run-seconds` counts real time while the
  virtual clock is paused, so an unattended run always ends.

## 4. Test aid `NVF_PLAY_TEST_TALK`

`NVF_PLAY_TEST_TALK=<when>:<who>[=<step>|<step>…][;<when>:<who>…]`

- `<when>`: seconds after the start, or `<seconds>@<quest>=<stage>`: that long after
  the stage was first seen done (as in `NVF_PLAY_TEST_SCRIPT`, layer5-play.md §5).
- `<who>`: a reference (Editor ID, hex FormID, `placed:<base>`): the player activates it
  then, as E would, wherever the crosshair is. Or `*`: nothing is started, and the steps
  answer the next conversation a script starts.
- Steps, each taken once the actor has finished speaking and the choices have been on
  screen for `NVF_PLAY_TEST_TALK_DELAY` seconds (default 0.25):
  - `<n>`: the n-th offered choice, 0-based (as `index` of `fnvtool dialogue talk`; the
    screen numbers from 1);
  - a topic's Editor ID or hex FormID: that topic's choice;
  - any other text: the choice that reads so, ignoring case; a part of it if exactly one
    choice contains it; quotes force text (`"3"`, `'leave'`);
  - `leave`: as Tab; `skip`: skips the response being said (taken while the actor
    speaks).
- When the steps run out, or a step names nothing that is offered (a warning), the aid
  leaves: by the Goodbye choice if one is offered, else by breaking off (this safety net
  exists only in aid-driven runs and does not need the debug switch below).
- `break`: Shift+Tab as the player would press it, once the response or the choices
  have been on screen a moment: it does something only with the debug switch.

**Debug switch `NVF_PLAY_DEBUG_TALK_BREAK=1`** (unset, empty, `0`, `off`, `false` and
`no` leave it off; it does not make a run unattended): Shift+Tab breaks the conversation
off at any point (`Game::end_dialogue`: the line being said is cut and no end script
runs, so a quest result can be lost). Without the switch Shift+Tab does nothing, and no
on-screen hint names it [owner decision 2026-10-10: "this should not be in the release
version"].

Debug switches (`NVF_PLAY_DEBUG_*`) and test aids (`NVF_PLAY_TEST_*`) are development
tools: they are to be compiled out of packaged releases once packaging exists
[coordinator's note 2026-10-10; no packaging work yet].

**Unattended runs never wait for input** (any `NVF_PLAY_TEST_*` variable, or
`--auto-walk`): a conversation no script answers is left the same way after 1 s of
choosing, and any conversation is broken off after 600 s. Message boxes inside a
conversation are answered by `NVF_PLAY_TEST_SCRIPT_MENUS` as elsewhere.

**Log**, one line each, all starting `dialogue:` (`dialogue::log_event`, from the trace
events, so they are the runtime's account):

```
dialogue: conversation with <actor> started (topic <topic>)
dialogue: <actor> says [<topic>] <line> response k/n (<seconds> s): "<text>"
dialogue: <actor> offers N choices: 0) "<text>" [<topic>]; 1) … (goodbye) (dimmed)
dialogue: choice <index> made: "<text>" [<topic>], <actor> answers with <line>
dialogue: topic <topic> added by line <line>
dialogue: conversation with <actor> ended (<goodbye|no_choices|left|actor_gone>, N lines)
dialogue: the conversation ends on the Goodbye choice; <actor> finishes the line <line> in the world
```

The aid's own lines start `test aid NVF_PLAY_TEST_TALK:`. With `NVF_PLAY_SCRIPT_TRACE`
the trace file has the same as trace events (`dialogue_started`, `response_said`,
`choices_offered`, `choice_made`, `dialogue_ended`, dialogue.md §8) and a `talk_input`
line for each thing the player or the aid did (`op`: `activate`, `choose`, `skip`,
`leave`, `break_off`; `by`; `error`).

`NVF_PLAY_TEST_SCRIPT`'s `var:` step now takes names with a colon
(`var:scripted:0x001055B4:bRunTimer:1`).

`NVF_PLAY_TEST_PAUSE=<seconds>:<seconds>[,…]` (`pause.rs`) opens the Esc pause menu at
the first time and resumes at the second, as Esc would.

## 5. The unattended intro with real conversations

`nvforever --data <Data> --play --new-game --no-voice --run-seconds 360` with
`NVF_PLAY_TEST_SCRIPT_MENUS=1` and

```
NVF_PLAY_TEST_SCRIPT=2@VCG01=55:walk:scripted:0x00104C02,12@VCG01=60:activate:VCG01VigorTesterREF,
1@VCG01=80:objective_done:VCG01:40,1@VCG01=80:var:scripted:0x001055B4:bRunTimer:1,
2@VCG01=110:walk:scripted:0x0010722F
NVF_PLAY_TEST_TALK=0@VCG01=80:*=VCG01DocMitchellTopic000|VCG01DocMitchellTopic009|VCG01DocMitchellTopic013|
VCG01DocMitchellTopic024|VCG01DocMitchellTopic029|VCG01DocMitchellTopic032|VCG01DocMitchellTopic040|
VCG01DocMitchellTopic045|VCG01DocMitchellTopic051|VCG01DocMitchellTopic055|VCG01DocMitchellTopic060|
VCG01DocMitchellTopic065|VCG01DocMitchellTopic070|VCG01DocMitchellTopic075;
4@VCG01=115:DocMitchellREF=VCG01DocMitchellTopic083
```

(each on one line). These are the inputs of the headless walk-through
(`crates/fnv-game/walkthrough/intro.actions.json`, dialogue.md §9):

- **The psych test.** Stand-ins: objective 40 completed and the couch trigger's
  `bRunTimer` set (sitting on the couch is furniture, AI). The trigger's own script then
  calls `StartConversation player`; the screen opens; the `*` script answers fourteen
  times by topic; the last answer's Goodbye line sets stage 85 at its end. (dialogue.md
  §9 describes a 2 s count by the trigger's script before the call; in the engine runs
  the conversation started on the frame the variable was set. Reported, not explained.)
- **The farewell.** Stand-in: the aid activates Doc 4 s after stage 115 (his dialogue
  package would walk him to the player). The greeting, its follow-up lines and the
  Pip-Boy message are the game's; the one answer is the aid's; the Goodbye line's
  end script leads to the hardcore box and VCG01 200.

The stand-ins of layer5-play.md §6.2 for the psych test's scores, `bBeganTest`,
`bDoFinalCalculation`, the verdict line and the two farewell lines are gone: the
conversations set them.

## 6. Checks (2026-10-10)

All windowed runs unattended, on main c7ced81 merged in (voice #119, the runtime's menu
mode in conversations #120), `RUST_LOG=info`, each exit 0 with "run finished".

- **The intro** (§5; 360 s asked, done after about 260 s; 58.8 frames per second):
  **VCG01 completed at stage 200** (stopped), VMQ01 10, VCG02 5, CGTutorial 2, VCG00
  0, 90, 95, 100 and VCG01 0, 1, 3, 5, 7, 8, 10, 15, 25, 30, 35, 36, 40, 45, 50, 55, 60,
  65, 70, 75, 76, 79, 80, 85, 90, 95, 98, 100, 102, 105, 110, 115, 200: the stage set of
  layer5-play.md §6.2. Tag skills Science, Guns, Sneak, as the headless walk-through.
  - Psych test: the couch trigger's `StartConversation` opened the screen 1.0 s after
    stage 80 (on the frame after the aid set `bRunTimer`); greeting (two responses), fourteen choices picked by topic
    ("Cat.", "Target.", "Dream.", "Reasonable.", "Inspiration.", "Genes.", four times
    "No opinion." and once "Disagree." for the statements, three inkblots), 15 lines,
    ended by Goodbye; stage 85 on the frame it ended, 90 one second later. 54.7 s of
    real time during which the runtime's game time stood still (trace `gt` 109.53
    before and after).
  - Farewell: the aid's activation started Doc's greeting 4 s after stage 115
    (0x001057E8); two follow-up lines without a choice, one choice offered and picked
    ("Thanks for patching me up, Doc."), its follow-up Goodbye line of four responses,
    5 lines, 62.5 s; the hardcore box opened on the frame the conversation ended and
    VCG01 200 came 0.4 s after it was answered.
  - Trace: 2 `dialogue_started`, 29 `response_said`, 15 `choices_offered`, 15
    `choice_made`, 2 `dialogue_ended`, 16 `talk_input` with no error; one
    `voice: playing …` log line per response (silent with `--no-voice`).
  - No message box opened *during* either conversation (the Pip-Boy line's message
    is a line on screen, not a box), so a box on top of a conversation is covered only by the unit
    tests of the runtime, not by a run.
- **Easy Pete** (`--cell Goodsprings`, no new game, so VCG01 has not run):
  `NVF_PLAY_TEST_TALK="12:EasyPeteRef=VFreeformGoodspringsGSEasyPeteTopic000|victor|leave"`,
  `NVF_PLAY_TEST_TALK_DELAY=2`. Greeting "Howdy. What can Easy Pete do for you?"; offered:
  the people who attacked me / why Easy Pete / Victor / Goodbye. Chosen by topic: why
  Easy Pete; then offered six (the attackers, prospecting, the NCR, the Bighorners,
  Victor, Goodbye); chosen by a part of the text: Victor; then five; `leave` took
  Goodbye, whose line ended the conversation (4 lines). A question that was asked is
  gone from the next list, and nothing was ever dimmed (a rule to capture, §7).
- **With sound** (the same without `--no-voice`, greeting and goodbye): two
  `voice: playing sound\voice\falloutnv.esm\maleold02\…ogg` lines (2.97 s and 9.10 s),
  no audio warning or error in the log. Whether it was audible is the owner's to say.
- **`--dialogue-pause original`** with `12:EasyPeteRef=skip|1|skip|leave|skip`: the
  virtual clock stopped when the conversation started and ran again when it ended; the
  three `skip` steps cut each response after 0.3 s (the lines still ended uncut, the
  conversation by Goodbye); the run counted its 30 s of virtual time and finished.
- **A traveller during a conversation** (`--npc-travel DocMitchellREF=GSChetRef`, the
  conversation started by `NVF_PLAY_TEST_SCRIPT=12:activate:EasyPeteRef`, no talk
  script): Doc had walked 989 units when it started and 989 when it ended 13 s later,
  then walked on. With no script to answer, the unattended rule took Goodbye after 1 s.
- **Goodsprings `--play`, 40 s, no aid**: exit 0 (the first pack box stays open, as
  before).
- **Travel smoke test** of CLAUDE.md (nav.rs changed): "nav summary: 2 of 2 travellers
  arrived".
- Not run: the moving-camera smoke test and the time-lapse (no rendering, sky or
  lighting change).

### 6.1 Follow-up: one pause switch for all menus (2026-10-10, `claude/menu-pause`)

Owner decisions of 2026-10-10 applied: Shift+Tab behind a debug switch (§4), the cursor
as built (§3), `--menu-pause` for every menu with the Esc pause menu holding everything
(§3). Found and fixed on the way: a SayTo or Say line's sound played on in real time
while a menu held the line's timer, so it ran ahead of the timer and the subtitle
outlasted it after the menu (`voice::line_held`; the mouth of the lip sync holds with
the sound, `Spoken::held_secs`).

Windowed runs, unattended, on main 95a90f6 merged in (lip sync #125), each exit 0 with
"run finished" and no WARN or ERROR line:

- **The intro** (§5, `living`, `--no-voice`, 300 s): **VCG01 completed at stage 200**,
  both conversations as before (15 and 5 lines, ended by Goodbye). Doc's SayTo line
  0x00104BF9, started while the race menu was open, was held for the second the menu
  stayed and went on when it closed (`voice: … held` / `goes on`).
- **`--menu-pause original`** (Goodsprings, `NVF_PLAY_TEST_SCRIPT_MENUS=1`,
  `NVF_PLAY_TEST_MENU=nearest` with `NVF_PLAY_TEST_DELAY=20`,
  `NVF_PLAY_TEST_TALK="40:EasyPeteRef=0|leave"`): the picture stood still eleven times:
  under the pack and add-on message boxes (1 s each), the container menu (the Mailbox,
  6 s), the inventory (5 s) and the conversation (29 s), and ran again after each. The
  aids went on throughout (they read `AidTime`) and the run counted its 85 s and
  finished. Screenshots: two taken 0.5 s apart in the inventory are identical pixel for
  pixel, and so are two taken 2 s apart during one response of the conversation.
- **For comparison, `living`**: two screenshots 2 s apart during one response differ in
  about 41,500 of 1,024,000 pixels (the picture lives).
- **Shift+Tab**, by the aid's `break` step, which goes through the same switch as the
  key (key presses cannot be injected): without `NVF_PLAY_DEBUG_TALK_BREAK` the log says
  "Shift+Tab does nothing" and the conversation goes on to its Goodbye (3 lines); with
  `NVF_PLAY_DEBUG_TALK_BREAK=1` the conversation ends at once (`left`, 2 lines, the
  line's voice stopped 7 s before its end). The key's own branch (Shift held with Tab)
  was not pressed by a hand.
- **The Esc pause menu** (`NVF_PLAY_TEST_PAUSE=17:21` during Easy Pete's answer, with
  sound and lip sync, under the old flag name `--dialogue-pause living`): on opening,
  the line was held and the picture stood still; two screenshots 2 s apart under the
  menu are identical; on resuming, the line went on, and its next response started
  4.0 s later than its length alone would give (7.48 s + the 4.0 s held). No audio
  warning or error. That the sound was silent for those 4 s, and that the mouth stood
  still, was not heard or seen by this session (the speaker was not in view).
- **Goodsprings `--play`, 40 s, no aid**: exit 0.

Under `original` the game is `Playing` for a frame or two between two message boxes
shown one after the other, so the picture moves by that much between them (§7).

## 7. Open

- **Played by hand: not done by this session** (the unattended runs cannot hold the
  cursor, so free look and mouse clicks on choices were not exercised; keys and clicks
  go through the same code path as the aid from `Act` on). Owner item.
- The actor and the player's view turn to each other since step D7 (§8), and the view
  closes in on the speaker (§8.8). The eyes do not move in the head. The mouth moves since step D6 (docs/spec-notes/lipsync.md); the head turns
  a `.lip` file asks for are still not applied (their unit is unknown, lipsync.md §3.3).
- The menu does not show the player's own line being said, nor the actor's last line
  while choosing.
- A choice's skill tag names the actor value by display name when the simulation knows
  one, else by Editor ID.
- NPC idle animation is presentation and runs on in `living`; an NPC that was walking
  (`--npc-travel`) stops and stands. AI packages do not exist yet; when they do they
  have to honour `WorldHeld` too.
- Between two message boxes shown one after the other the game is `Playing` for a frame
  or two (the next box opens when the first has closed), so under `original` the
  picture moves by that much between them.
- Barter, repair and the companion wheel are not conversations here (dialogue.md §7).

Seen in the runs of §6, for the rules' owner (nothing was changed in `fnv-game` for
them):

- Every question asked of Easy Pete was gone from the next list, and no choice was ever
  dimmed. The data explains the first: the answer to "Why are you called Easy Pete?"
  (INFO 0x0010515C) has Say Once and no choices, so the topic has no line left and the
  runtime hides it. Whether the original hides such a topic or shows it dimmed is the
  capture question of dialogue.md §7.5; the dimmed look is so far exercised only by the
  runtime's unit tests.
- The psych test opens on the frame `bRunTimer` is set, without the 2 s that
  dialogue.md §9 describes (§5).
- A response skipped by the player still runs the line's end script
  [hypothesis, §2]; every quest result in the runs came from lines said to the end.
- After a held traveller walks on, its 5-second progress line is logged a few times in
  a row (it catches up with the time it stood); cosmetic.

## 8. Facing each other (step D7, 2026-10-10)

Owner request 2026-10-10: "i need the player and npc that are talking to each other to
look at each other; this is for when they are in a back and forth dialogue; this doesnt
apply to spoken conversation dialogue that an npc just says out loud without expecting a
response".

Built on `claude/dialogue-facing`: `engine/nvforever/src/dialogue_look.rs` (the actor) and
`engine/nvf-play/src/dialogue_look.rs` (the player's view). Both act only while a
conversation is open (`Game::dialogue` is set: E on an actor, a script's
`StartConversation`; the dialogue screen is up). A line an actor just says (SayTo, Say)
sets no conversation. Since 2026-10-11 a line said *to someone* (SayTo, a hello, a line
between two actors) turns the speaker to its listener: §9.

**The angles and timings in §8.1–8.3 are NVForever's design choices**, not the
original's values, unless a line carries another tag. What is known of the original is
§8.5.

### 8.1 The actor turns to the player

- **Head.** The head bone is turned towards the player's eye on top of the frame's
  animation (after `locomotion::drive` has posed the skeleton), so the idle goes on
  underneath. It eases in over 0.45 s and, when the conversation ends, back to the
  animation's own pose over 0.6 s (a smoothstep, no snap).
  - Which bone and how far come from the game's data: the body part with the
    "IK Data - Headtracking" flag in the actor's body part data names the bone (its IK
    start node) and the largest angle (its Tracking Max Angle) [documented: GECK wiki
    "Body Part", owner export]. For NPCs that is `Bip01 Head` and 60
    [observed: `DefaultBodyPartData`, docs/spec-notes/actors.md §7.6]. The angle is used
    as the largest turn to either side [hypothesis: degrees, and that it bounds the turn
    to a side]; a value above 80° would be cut to 80° [design]. Up and down the head
    goes at most 30° [design].
  - The turn is spread over the neck: `Bip01 Neck` is turned by 20 % of it, `Bip01
    Neck1` by 40 % and the head by all of it, so the neck does not twist at one joint
    [design; whether the original turns anything but the IK start node is not known].
  - The turn has no roll: it is a turn to the side and a tilt up or down in the actor's
    own space.
  - **The animation's head movement stays under the turn** (2026-10-27): the turn aims
    the animation's *average* head direction at the player, the average following the
    animation over 0.8 s (`KEEP_SECONDS`), so the talk and listen clips' nods and
    glances (0.3 to 1 s) still show on the turned head, at most 20° of them each way
    [design]. Before, the head was aimed exactly at the eye every frame, which cancelled
    every nod the clips carry: measured against the owner's video of the original's
    Sunny Smiles conversation (face tracked frame by frame, in eye distances), our head
    moved up and down at a third of the original's speed (90th percentile 0.08 against
    0.25 eye distances a second) [observed 2026-10-27].
  - A player behind the actor (more than 135° from its front) keeps the side the head
    first turned to, so the head does not flip from one shoulder to the other [design].
  - What hangs from the head bone or is skinned to it follows: the face and its lip
    sync (`lips.rs` moves vertices, not bones, so the two do not meet), hair, eyes,
    beards, hats.
- **Body.** An actor standing in its idle whose body faces more than 30° away from the
  player turns on the spot to face them: eased, at about 150° a second (0.5 s at the
  least, 1.4 s at the most), the head leading. While it turns, `Locomotion::turn` is
  set, which asks the locomotion blend for the skeleton's turn clips (`mtturnleft`,
  `mtturnright`, as for the walk test and travellers, actors.md §8.3) so that the feet
  step [the feet were not in a screenshot: not checked]. The body keeps its new heading when
  the conversation ends [design: AI packages will take over later]. Only the drawn
  actor turns: the simulation's reference keeps its angle (what `GetAngle` reads), as
  actors' drawn poses are the engine's own until AI exists (`moved.rs`).
- **Bodies of actors the AI runs** (`--play`: every actor of the loaded cells;
  `--npc-packages`) are turned by `fnv_ai::Ai::update_held`, on the spot at the
  standing turn rate, for the whole conversation however it started (the player's
  activation, StartConversation, a Dialogue package), in the walker's heading, which the
  engine draws and the game keeps; so the actor still faces the player after the
  conversation until a package turns it. `dialogue_look.rs` turns only their heads
  ("the AI turns its body to the player (fnv_ai)"). Until 2026-10-11 this file turned
  the drawn body of such an actor too, and the next frame's drawing at the walker's
  heading undid it: the owner saw the actor turn on its greeting and turn back
  (playtest 2026-10-11). Not turned: an actor on furniture, one whose Dialogue package
  has No Headtracking.
- **Head only**, the body left alone, for an actor whose body something else places:
  - an actor the AI runs (above);
  - a `--npc-travel` traveller (its heading is its walker's; it stands while the
    conversation holds the world, turns its head, and walks on afterwards);
  - an actor under the walk test (`--actor-walk-test`);
  - an actor that is not standing in its idle;
  - an actor marked `dialogue_look::BodyHeld`: in furniture or lying down.
    **Nothing marks actors so yet: the engine seats nobody** (furniture use is AI,
    Layer 6; `GetSitting` is a stub). What the engine can tell today is whether an actor
    is dead or a ragdoll, whether the navmesh walker or the walk test moves it, and
    whether it plays its standing idle. Every living actor is drawn standing, also
    where the original would have it on a chair (Doc Mitchell during the psych test),
    so a body that turns is a standing body. Whoever seats actors later has to insert
    `BodyHeld`.
- **Nobody turns** (the view still turns to the actor, §8.2) when the actor
  - is dead or a ragdoll: never touched, and dropped at once if it dies while facing;
  - is not a human-style NPC: a creature, a robot, a talking activator [design: their
    skeletons and limits are not looked at yet];
  - has no such bone, or its body part data has no part that allows head tracking;
  - is not drawn.

### 8.2 The player's view turns to the actor

- **Opening.** When the conversation starts the view eases onto the actor's face over
  0.4 s (smoothstep). The face is a point 5 units in front of and 6 above the drawn
  head bone [design; the eyes are about 9 and 7, `player_body.rs`]; for an actor
  without a drawn head, a point at 90 % of its hit volume's height, else 112 units
  above its reference.
- **The zoom locks the view; free look does not zoom.** **Owner decision 2026-11-13**
  (issue #2254): "want it split so free look camera does not zoom and when zoom lock
  the camera". While the conversation is zoomed (§8.8) the view follows the speaker's
  face for the whole conversation, choices included; the mouse moves only the cursor
  and neither the mouse nor the right stick turns the view
  (`dialogue_look::view_locked`, `Ease::follow`);
  the world keeps animating under `menu-pause living`. There is no zoomed free look and
  no setting for one. With the zoom off (`dialogue-zoom` off, or a conversation the
  zoom does not take, §8.8) the 2026-10-10 free look below holds unchanged. The
  original is reported to keep the view on the speaker [hypothesis: player reports,
  not captured; observer check 121].
- **Free look, without the zoom** [owner decision 2026-10-10, §3]: while the actor
  speaks the mouse looks around as before. An ease never fights the mouse: if anything
  else moved the view since the ease last wrote it (the mouse, a test aid), the ease is
  dropped and the view is left where it was put.
- **Choices, without the zoom.** The view does not come back by itself. **Owner
  decision 2026-10-10:** "never come back i just want the initial look at npc to happen
  then allow the player to free look the rest of the time they are in dialogue". Only
  the opening turn happens. With `--dialogue-recentre` the view eases back onto the
  actor's face over 0.35 s each time the choices appear, if it is more than 0.5° off
  (the behaviour first built; kept as an option).
- **View angles** [design: `dialogue_look.rs`, `dialogue_zoom.rs`]: this turn changes
  heading and pitch, with no roll. Zoom controls the field of view and the third-person
  camera's travel separately (§8.8).
- **The player's body** (third person, the first-person arms) follows the view's
  heading as it always does; the player's own head is not turned separately.

### 8.3 Pausing

- `living` (default): the turns are presentation and run while game logic is held.
- `--menu-pause original`: the picture stands still for the whole conversation, so the
  turns are completed at once when it starts: the frozen picture shows the two facing
  each other. (The speaker's mouth still moves with the voice: lip sync follows the
  sound, which plays in real time in every mode; seen now that the view is on the
  speaker.) When the conversation ends and the clock runs again, the head eases back.
- The Esc pause menu holds the turns like everything else (they run on the virtual
  clock): two screenshots 1.2 s apart under it, opened in the middle of a body turn,
  are identical; the turn goes on when the menu is closed.

### 8.4 Switches, test aid, log

- `nvforever --no-dialogue-look`: nobody turns (actor and view).
- `nvforever --dialogue-recentre`: the view comes back onto the actor each time the
  choices appear (off by default; `--no-dialogue-recentre` is still accepted and does
  nothing).
- `nvforever --no-dialogue-zoom`: no close-up and no blur (§8.8).
- No switch for the zoom's lock: the zoom is the lock (§8.2, owner decision 2026-11-13).
- Cursor transition guard [design, #2254; `player::DialogueMouseGuard`]: in dialogue,
  motion on a mouse-look capture transition and the next input frame is ignored;
  subsequent motion is accepted in free look (without the zoom). Code reading of Bevy
  0.20.1 `bevy_winit/src/state.rs::device_event`, `system.rs::changed_cursor_options` and
  winit 0.30.13 `platform_impl/windows/{window,window_state,event_loop}.rs` found raw
  mouse motion forwarded separately from cursor capture. The backend notes that cursor
  clipping can generate `WM_MOUSEMOVE` (pointer-position events); no explicit raw
  `MouseMotion` emission from capture was found. This does not establish platform/driver
  behavior at runtime. Synthetic
  transition deltas are covered by `cursor_release_and_regrab_motion_do_not_take_over_the_view`.
- Test aid `NVF_PLAY_TEST_TALK_HEAD_ONLY=1`: no actor turns its body (as if all were
  `BodyHeld`), to see the head at its limit. Not a game behaviour.
- Log, each line starting `dialogue look:`:

```
dialogue look: <actor> faces the player with head and body (standing in its idle): the player is <a>° off its front, the body turns <a>° in <s> s from heading <h>°; the head turns up to <l>° to a side (<where the limit is from>) and 30° up or down, on <bones>
dialogue look: <actor> faces the player with its head (standing in its idle; the body stays: the player is <a>° off its front, within 30°); …
dialogue look: <actor> faces the player with its head only (<why>): the player is <a>° off its front; …
dialogue look: <actor> does not turn to the player: <why>
dialogue look: <actor> is not drawn as an actor: nobody turns to the player
dialogue look: <actor> stops facing the player: the head (yaw <y>°, pitch <p>°) eases back over 0.60 s, the body keeps its heading <h>°
dialogue look: the view turns to <actor> (<what the face point is>): heading <h0>° to <h1>°, pitch <p0>° to <p1>°, over 0.40 s
dialogue look: choices: the view comes back onto <actor>: heading … over 0.35 s
dialogue look: the zoom locks the view on <actor>: heading … over 0.35 s
dialogue look: the view was moved (the mouse, or a test aid): it is left where the player put it
dialogue look: the conversation with <actor> is over: the view is the player's own again
```

### 8.5 What the original does

From the owner's GECK wiki export unless tagged otherwise; nothing here was captured.

- Actors turn their heads to targets as a standing behaviour ("head tracking"). A body
  part of an actor's body part data can carry IK data: "Is Head", "Headtracking" (the
  head will turn towards its target), a "Tracking Max Angle" (the largest angle the
  actor will turn its head to look at something) and a start node [documented: "Body
  Part"]. The values for NPCs are in actors.md §7.6 [observed].
- A package can switch it off: with "HeadTracking Off" the actor's head is locked
  looking straight ahead while the package runs [documented: "Category:Packages"]. An
  actor base can carry "No Rotating to Head-track": it will not rotate its head to look
  at the player or other NPCs [documented: "Animation Tab"]. Neither is read yet
  (packages are Layer 6; §8.7).
- A script can force it: `Look <target>` makes the actor head-track the target until
  `StopLook`, overriding normal head tracking; with its optional second parameter the
  actor rotates its whole body to the target [documented: "Look", "StopLook"]. So the
  original has both a head turn and a body turn towards a target. **That an actor turns
  its body to the player in a conversation, and how fast, is not in the export**
  [hypothesis].
- `StartConversation` takes, after the topic and two location references, a head-track
  parameter (1 enables head tracking during the conversation, 0 disables it) and an
  allow-movement parameter [documented: "StartConversation"]. The runtime reads neither
  (`fnv_game::functions::misc::start_conversation`); what the original does when they
  are left out is not in the export [hypothesis: head tracking is on].
- A game setting `fAIMaxHeadTrackDistance` exists [documented: listed on "Settings",
  no description]; the console's `ToggleLookIK` switches actors moving their heads to
  face other actors [documented: "ToggleLookIK"].
- Skeletons carry a float named `HeadTrack` that a clip can animate
  [documented: "NIF Block Types", "NIF Animation Controllers"]; the human idle has such
  a track on `Bip01 Head` [observed: actors.md §7.1 and §7.6]. Not read.
- An actor's sitting state is told apart by `GetSitting`: not sitting, loading the
  sitting idle, getting ready to sit, sitting, getting ready to stand up [documented:
  "GetSitting"]. Whether a sitting actor turns only its head in the original is not in
  the export [hypothesis: it does not swivel in its chair; the owner's brief].
- **The dialogue camera.** The export does not describe what the original's camera does
  in a conversation. Its only trace is a third-party plugin's page (itr-nvse,
  "SetDialogueCameraEnabled") that says the plugin replaces "the vanilla
  over-the-shoulder dialogue view". Captures of the original (2026-11-06) show what it
  does [observed, the shots stay private]:
  - The view eases onto the speaker while the HUD goes and the subtitle appears, settled
    in about 0.6-1 s (samples every 4 frames: fitted, rough); closing, it is back in
    about 0.3 s, the HUD back at the first frame of it.
  - The head comes out the same size from any distance, so it is a framing and not a
    fixed zoom: from about 90 units and from about 300 (Doc Mitchell), crown to chin is
    about 0.30 of the view's height, the face's centre about 0.32 of it from the top,
    centred across.
  - From third person it ends in the same close-up with no player body, the camera having
    travelled from behind the player, and returns on closing.
  - The far background is strongly blurred when it is far behind the speaker, a little
    when it is close behind him; the speaker stays sharp. That fits a depth of field
    focused on the speaker [hypothesis]. In one view the whole picture also looks dimmer;
    not explained, left out.
  What NVForever does with this: §8.8.

### 8.6 Checks (2026-10-10)

Windowed runs, unattended (`--no-voice`), each exit 0 with "run finished" and no WARN
or ERROR line; screenshots and logs kept outside the repository. The conversations are
started by `NVF_PLAY_TEST_TALK` with the player put somewhere by `NVF_PLAY_TEST_GOTO`
and turned away by `NVF_PLAY_TEST_LOOK`.

- **Already facing** (Easy Pete on the saloon's porch, the player 70 units in front of
  him looking 83° away): he "faces the player with its head (… the body stays: the
  player is -0° off its front, within 30°)"; the view went from heading 180.0° to
  262.6° and pitch -5.0° to 5.5° in 0.4 s. Screenshots: 0.2 s before the conversation
  the view is on the road; 0.4 s into it his face is in the middle of the picture; at
  the choices the view came back by 0.6° (the head sways with the idle).
- **From the side** (the player on the porch 47° off his front, looking away): "head
  and body (standing in its idle) … the body turns +47° in 0.50 s from heading 86°";
  the view went the short way round, 150.0° to 316.9°. Screenshots 0.2 s before (the
  town), 0.2 s in (the view sweeping past the windmill), 1.5 s in (his face to the
  camera and in the middle, the mouth open on the line, hat, hair and beard in place,
  the collar closed around the neck) and 5 s after the end (his body still turned to
  the player, heading 132°, the head back in its animation pose). From below the porch
  at 97° and 104°: the body turned in 0.65 and 0.69 s, the head tilted down to the
  player (pitch -12°).
- **From straight behind** (Sunny Smiles in the Prospector Saloon, the player 80 units
  behind her looking the other way): "the player is +175° off its front, the body turns
  +175° in 1.17 s from heading 90°"; the view 270.0° to 85.5°. Screenshots: 0.4 s in she
  is seen from the side, the head ahead of the body; 0.7 s in three quarters round; 2 s
  in she faces the camera, level, in the middle, hair and eyes in place, speaking.
- **The head at its limit** (test aid `NVF_PLAY_TEST_TALK_HEAD_ONLY=1`, the player 109°
  off Easy Pete's front, `--fov 28` for a close view): "with its head only", and at the
  end "the head (yaw +60°, pitch -13°)", "the body keeps its heading 86°". Screenshots:
  a three-quarter profile turned to the camera over the shoulder, no kink or gap at
  the neck, the mouth closed in one shot and open in another; 3 s after the end the
  back of his head (the head is back to the front).
- **From outside**: third person (`NVF_PLAY_TEST_CONTROL=8:view`), and the camera
  circled round the pair for the one frame of a screenshot (`13:orbit=150:10`,
  `20:orbit=-120:15`; the `orbit` press holds for the frame it is made in). Before: the
  two side by side, both facing east. During: Easy Pete turned to the player (heading
  86° to 190°) and looking down at them (the player stands on the ground below the
  porch), the player's body turned to him. After: both still facing each other.
- **`--menu-pause original`**: the turns are complete on the conversation's first
  frames; no coming back was needed at the choices. Three screenshots 0.3, 1 and 2 s
  into the conversation differ only in a box of 108 × 153 pixels in the middle of the
  picture, his face (the lip sync, which follows the sound). Found on the way: the eye stands in front
  of the player's feet and moves as the body turns with the view, so a view put on the
  face in one frame ended 3° beside it; the view is now put on it for three frames.
- **The Esc pause menu** (`NVF_PLAY_TEST_PAUSE=12.25:14.25`, opened a quarter of a
  second into a body turn of 0.69 s): two screenshots under it, 1.2 s apart, are
  identical in all 1,024,000 pixels; after resuming the turn finished.
- **The view moved while it turns, and free look** (the test aid's look standing in for
  the mouse: `NVF_PLAY_TEST_LOOK` 0.2 s into the opening ease, and again 1.5 s in while
  he speaks): "the view was moved (the mouse, or a test aid): it is left where the
  player put it"; then at the choices "the view comes back onto EasyPeteRef: heading
  200.0° to 310.7°, pitch -10.0° to 12.3°, over 0.35 s", and the screenshot with the
  choices has him in the middle. With `--no-dialogue-recentre` the same run logs no
  coming back, and the choices are shown over the landscape the view was left on.
  (These runs were made when coming back was the default. Since the owner's decision of
  2026-10-10 the second behaviour is the default and the first needs
  `--dialogue-recentre`; the runs in this section whose logs show "the view comes back"
  correspond to that flag now.)
- **`--no-dialogue-look`**: the only `dialogue look` line is "dialogue look is off"; the
  conversation ran as before.
- **A traveller** (`--npc-travel EasyPeteRef=GSChetRef`, talked to on his way at 14 s):
  "faces the player with its head only (its heading is its route's (--npc-travel))"; he
  stood at 1,041 to 1,044 units walked while the conversation ran, walked on, went
  through the store's door and arrived ("nav summary: 1 of 1 travellers arrived").
- **The intro** (§5, 360 s): **VCG01 completed at stage 200**; the trace as in §6 (2
  `dialogue_started`, 29 `response_said`, 15 `choices_offered`, 15 `choice_made`, 2
  `dialogue_ended`, 16 `talk_input`). 52 lines were said; the 23 that are SayTo lines
  logged no facing, and the screenshot during "What do you say you take a seat on my
  couch…" shows the view where the player left it (on the Vigor Tester). The two
  conversations logged facing:
  - Psych test: "DocMitchellREF faces the player with head and body (standing in its
    idle): the player is -159° off its front, the body turns -159° in 1.06 s from
    heading 23°"; the view 180.0° to 44.2°. **Doc Mitchell is not sitting**: the engine
    draws him standing at his placement across the room, by the bed (a chair stands
    beside him); so no body turned in a chair, and a standing body turned on the
    spot. At the end "the body keeps its heading 224°".
  - Farewell: "the body turns -46° in 0.50 s from heading 224°"; the view 44.4° to
    358.4°. He is still drawn in the other room (his dialogue package would walk him to
    the player), so from the house's door the view turned to him through a wall (§8.7).
- **Goodsprings `--play`, 40 s, no aid**: exit 0. **Moving camera** (`--auto-fly 4000
  --run-seconds 40`): exit 0, "run finished", 212 cells loaded.
- Not run: the travel smoke test of CLAUDE.md (`nav.rs` is untouched and no traveller's
  heading is written; the traveller run above covers a conversation with one) and the
  time-lapse (no sky or lighting change).
- Unit tests: the view's ease (the short way round, no snap, dropped when the view was
  moved, a frozen picture, the pause menu), the face point, the head's limits and side,
  the shares of the neck, bones written down a chain, a bone nothing posed again, the
  body's turn.
- Frame rates of these runs ranged from 11 to 54 per second with other sessions
  building alongside; the 40 s runs gave 41.8 (`--play`) and 52.1 (`--auto-fly`).
- The runs above predate the merge of main's LEGAL.md and owner-review note (no code)
  and the move to `claude/dialogue-facing`. After it, on the rebuilt binary: the check
  list of CLAUDE.md again (all clean), and the conversation from the side once more
  (the same log lines: the body +47° in 0.50 s, the view 150.0° to 316.8°; exit 0).

### 8.7 Open

- **Played by hand: not done by this session.** The eases were seen in screenshots a
  few frames apart, not in motion; the mouse dropping an ease is covered by a unit test
  and by the test aid's look, not by a hand. Owner item.
- The eyes do not move in the head: at the head's limit (a player far to the side of
  an actor that keeps its body) the face is turned towards the player but the eyes look
  straight out of it.
- The close-up is unplayed (§8.8): its numbers are fitted and its checks are logs and
  unit tests.
- The turn clips are asked for while a body turns, but not checked in a picture, and
  their length is not matched to the turn: a turn takes 0.5 to 1.4 s whatever the
  clip's own pace is.
- Only the drawn actor turns; the simulation's reference keeps its angle. When AI
  packages place actors, the heading has to become the simulation's.
- An actor is turned to where the player is, and the view to where the actor is
  *drawn*. Actors are drawn at their placement until AI moves them, so a conversation a
  script starts from afar (Doc Mitchell's farewell, which his dialogue package would
  walk him over for) turns the view to him through a wall.
- Not read: the package flag "HeadTracking Off", the actor flag "No Rotating to
  Head-track", `StartConversation`'s head-track parameter, `Look` and `StopLook`
  (stubs), `fAIMaxHeadTrackDistance`, the `HeadTrack` track of clips.
- Creatures, robots and talking activators do not turn (the view turns to them).
- **Hand-over to general turning and head tracking** (other work in progress, not
  merged and not used here: a component that turns a standing actor on the spot with
  its turn clip, general head tracking, AI packages). The facing code is laid out for
  it: one place decides and one applies, for each of the two.
  - Body: `BodyTurn::plan` decides whether and how far the body turns (from where the
    player is in the actor's own space); the "body" block of `turn_speaker` applies it
    (the root's rotation and `Locomotion::turn`). When a shared turn-on-the-spot exists,
    that block hands the target heading to it instead of stepping the turn itself, and
    the head-only cases (`BodyHeld`, travellers) stay decided in the same `match`.
  - Head: `sided` and `limited` decide the head's target; `chain_turns` and
    `local_turn` apply it to the bones. When actors track heads all the time, the
    conversation becomes one more source of a target (the player's eye, with priority
    while the screen is open) for that system, and this file's part shrinks to naming
    the target; the bone chain and the limits from the body part data move with it.
- Head tracking outside conversations (actors looking at the player or each other as
  they pass) was not built here: it is AI. It was built since (`nvforever` `head_look.rs`,
  docs/status.md §3 Animation).
- The `.lip` files' own head and look channels are still not applied (lipsync.md §3.3).

### 8.8 The close-up (2026-11-06)

`engine/nvf-play/src/dialogue_zoom.rs`; the original's side is §8.5. Setting
`dialogue-zoom` (Gameplay, on by default), `--no-dialogue-zoom`; it does not need
`dialogue-look`, whose turning it only aims (with that off the renderer still says where
the face is, and the view narrows where it looks).

- **Field of view.** The vertical angle in which a head of `HEAD_HEIGHT`, 15.5 units, at
  the distance to the face fills 0.30 of the view's height: `2 atan(15.5 / (0.6 d))`, so the
  aspect ratio does not enter [observed: 0.30 in the original; 15.5 from Chet's head as
  drawn in our run, about 185 of 720 px from 394 units at 8.7°]. About 32° from 90 units,
  10° from 300. Never wider than the view the player has (nearer than about 45 units at the
  default, the view stays), never narrower than 6° [design]. It follows a speaker walking
  up over 0.5 s, not the mouse.
- **Times.** In 0.8 s, out 0.3 s [observed: fitted, rough]; smoothstep.
- **Framing.** The opening turn (and `--dialogue-recentre`'s) aims under the face so that
  the head bone sits 0.38 of the height from the top, which puts the face's centre at
  about 0.32: `atan(0.24 tan(v/2))` below it [design: `dialogue_zoom::face_drop`]. After
  the 0.4 s opening ease, follow continues throughout the conversation, including while
  choices wait. Moving-face follow uses an exponential time constant of 0.08 s
  (`FACE_FOLLOW_SECONDS`) to soften frame-to-frame movement, taking the short way
  across heading wrap [design, #2254]. The zoom locks the view: mouse and stick cannot
  turn it and the cursor stays free; there is no timed hold expiry and no free look
  while zoomed [owner decision 2026-11-13, #2254, §8.2]. With `dialogue-look` off the
  zoom still locks: nobody turns and the view stays where it was, narrowed.
- **Looking around as the zoom eases out** (after the conversation) is scaled by the zoom
  (`tan(v/2)` against the unzoomed view's), mouse and stick, so the picture moves as far
  as it does unzoomed, and is back to the usual rate as the zoom goes [design].
- **Who owns the field of view.** The zoom keeps what is on the camera underneath and
  gives it back. A value it did not write is someone else's (the settings slider, the
  console's SetCameraFOV, the sights' zoom) and the new base. The sights' zoom, the console
  and the Vigor Tester ask for the value underneath before keeping one, so a zoomed angle
  never becomes a base. The zoom runs after the sights' in the frame.
- **Third person.** The camera travels from its place behind the player to the first-person
  eye with the zoom and the body goes at 0.85 of the way (`TALK_CAMERA_CLOSE`, before the
  camera is in the head; [design]); closing, the same back. `PlayerControl::pov` is not
  touched, so a script's IsPC1stPerson and a save see the player's own view. The detached
  camera (`control.rs`) gives its angle to the heading first; the zoom's lock then holds
  the view (§8.2).
- **No zoom**: through a talking activator (not in this version; logged once), a dead
  speaker, one that is not drawn, the free camera, the character creator, a kill camera.
  The Vigor Tester sets its own field of view: the zoom lets go at once. All else eases out.
- **Pausing.** On the virtual clock: the Esc pause menu holds it. With `--menu-pause
  original` both eases complete at once.
- **Blur.** A depth of field on the face's distance (image-modifiers.md §8.4), behind the
  speaker only, faded with the zoom, following the setting. No dimming.
- Log: `dialogue zoom: in on <actor> (<what the face point is>): <d> units, vertical <a>° to
  <b>°, over 0.80 s; view <v> units tall (<a human-style actor | drawn <h> tall>), the point <s> from its top[; third person: the camera at the eye, no body]` as the ease in ends;
  `dialogue zoom: out from <d> units: the view is back at vertical <a>°, over 0.30 s` as
  the ease out ends; `dialogue zoom: no zoom in the conversation with <actor>: <why>`;
  `dialogue zoom is off (--no-dialogue-zoom)`.
- Framing diagnostic [design, #2254]: at the end of a conversation lasting at least 6 seconds,
  `dialogue look: framing after <s> s: <actor> offset <degrees> degrees (locked=<bool>)`
  reports the last frame's larger heading/pitch error from the framing target after follow.
- Smoke [design: `tools/smoke/scenarios.toml`]: `dialogue-zoom-lock-hold`,
  `dialogue-zoom-lock-motion` (the look test aid obeys the same lock as the mouse; not
  raw mouse injection), `dialogue-zoom`, `dialogue-zoom-off`, `dialogue-zoom-no-look`,
  `dialogue-zoom-third`, `dialogue-zoom-frozen`, `dialogue-zoom-victor`, `dialogue-zoom-marcus`, `dialogue-zoom-house`, and `talking-activator-intercom` for the
  activator's line.
- **Creatures and robots** (ACRE; `creature_framing`). Every actor's head here is a head
  bone, `Bip01 Head`, on the securitron and the super mutant skeletons too, so a robot was
  aimed at sensibly but framed as a human head: 52 units of view for a machine 187 tall.
  From the original's shots, measured against the meshes in the data [observed, 2026-11-06]:

  | Speaker | Drawn height | View's height | Head or face share | Point from the top |
  |---|---|---|---|---|
  | Human (Doc Mitchell, Chet) | about 128 | 51.7 | head 0.30 | 0.38 (bone + face offset) |
  | Marcus (super mutant) | 173 | about 97 | head 0.25, face's centre 0.325 | bone about 0.31 |
  | Victor (securitron) | 187 | about 125 | screen 0.22, its centre 0.44 | bone 0.457, antenna 0.055 |
  | Mr. House (a screen) | 300 | about 150 (vertical 18.3° from about 466) | face well over half | bone about 0.55 |

  The view is `51.7 + 7.5 sqrt(height - 128)`, which gives 102 (+5 %), 109 (-13 %) and 150,
  with a creature's point at `0.9 * top above it / view`, held to 0.31 to 0.55: Marcus's at
  0.31, Victor's 0.46, House's 0.55. One rule over three big speakers, no table and no
  exception for House (his head bone is low on his machine, in front of the screen, and the
  rule puts it low in the view), and a fit, not the original's: a proportion to height or a
  power of it fits none of the three together. Height is the hit volume's, the union of the
  drawn meshes' boxes (`crate::hitvol`); 173, 187 and 300 are worked out from the meshes in the
  bind pose, the `in on` line logs what a run has. A creature no taller than a human
  gets a human's view. Human-style actors (ACHR) keep the numbers above, whatever their hit
  volume says, as a hat or a seat changes that and not their framing: in the original Raul,
  a ghoul seated, shows a head about 0.24, not looked into. A creature with no head bone
  aims at 0.9 of its hit volume's height (`face_point`), under the same rule; one with no
  hit volume is not drawn and gets no zoom.
- Open: not compared with
  the shots yet: the times, the blur's strength and range (its picture is the fixed radius of
  image-modifiers.md §8.5, which may be too little for the far hall view).

## 9. Turning to whom a line is said (2026-10-11, `claude/sayto-facing`)

Owner request 2026-10-11: "when an actor is saying something to an npc or character i
need them to turn and face the actor they are speaking to"; owner decision the same day:
"speaker turns head and body". This widens §8, which covered only conversations with the
player; §8's note that lines said out loud turn nobody now holds only for lines said to
nobody (Say).

### 9.1 What the original says

- **SayTo** takes `targetActor topic [forceSubtitleFlag] [noTargetLookFlag]`. With the
  last flag set "Joe is not forced to look at the speaker during the SayTo"
  [documented: GECK wiki SayTo, owner export `functions.xml`]: so by default the target
  is made to look at the speaker. Whether the speaker turns to the target is not
  written.
- **StartConversation** takes `[headTrack]` after the two location references: "Enter 1
  to enable head tracking during conversation. Enter 0 to disable" [documented: GECK
  wiki StartConversation]. What it does when left out is not written.
- **Say** has no listener (topic, subtitles, an actor, 2D, voice) [documented: GECK wiki
  Say].
- The dialogue lines themselves carry no look setting: INFO's Speaker (ANAM) only limits
  who may say the line, SNAM/LNAM are the speaker's and listener's idles
  (docs/spec-notes/dialogue.md §2).

### 9.2 The rule (`fnv_game::game::Game::facings`)

For each line being said outside a conversation with the player that has a listener
(SayTo, a hello to the player, a Goodbye greeting at the player, the rest of a Goodbye
line the player chose when the runtime's switch is on (§1 item 4), a line of a
conversation between actors):

- the **speaker** turns its head and body to the listener [owner decision 2026-10-11];
- the **listener** looks at the speaker [documented for SayTo, §9.1]: head only for a
  SayTo line [design]; head and body in a conversation between actors, where it speaks
  next [design: `fnv_ai::social` already turned both bodies];
- SayTo with `noTargetLookFlag` 1: the speaker turns as without it, the listener is not
  made to look (`LineLook::Speaker`) [observed: observer check 218, base game
  2026-11-25: a standing Ringo turned his body to the player in about 0.8 s with flag 1
  as with flag 0, 3 of 3 and 2 of 2 runs; walking on his package he turned with
  neither flag; #3989]. From #2796 (2026-11-16) to #3989 the flag turned nobody, read
  from its name and Curtis's radio call in the McCarran tower, which only works with his
  back to the player; by the observation that comes from his package or idle there, not
  the flag (still open in check 218);
- StartConversation with `headTrack` 0: nobody turns, for every line of that
  conversation (`LineLook::Nobody`, carried from line to line) [hypothesis: left out, it
  is on];
- Say turns nobody. The player is never turned (the view is the player's own).

`headTrack` of a StartConversation with the player is not read: §8 applies there.

### 9.3 Who turns the actors

- **Heads**: the engine (`engine/nvforever/src/speech_look.rs`) gives each turning actor
  a forced head-tracking target (`head_look::LookOverride`: the listener's eyes or the
  player's) for as long as the line lasts; head tracking turns the head within its
  usual limits and eases it back afterwards. `--no-head-tracking` turns no head.
- **Bodies of NPCs the AI runs** (with `--play` every actor of the loaded cells, or
  `--npc-packages`): `fnv_ai` turns them on the spot (`Ai::face`, from `Ai::social`), as
  it did for hellos and conversations; not on furniture, not on its way (except for its
  Goodbye greeting at the player, §1, which stops it), not when a Dialogue package is
  already turning it.
- **Other bodies** (`--no-npc-packages`): turned on the spot by `speech_look.rs` with
  §8.1's `BodyTurn` (same speed and turn clips), once per line, when the NPC stands in
  its idle and is not in furniture, travelling, under the walk test, dead or a creature.
  The body keeps its heading afterwards.
- `--no-dialogue-look` switches §9 off along with §8.

Log: `speech look:` lines per actor and line (what turns; why not the body; at the end
of the line how far the listener is off its front), and `speech look summary:` at exit.
Test aid `NVF_PLAY_TEST_SCRIPT=<t>:say_to:<speaker>:<target>:<topic>[:<flag>]` calls
SayTo from the speaker's script.

### 9.4 Checks (2026-10-11)

- `cargo test -p fnv-game --test integration dialogue::lines_said_to_someone_turn_the_two_to_each_other`:
  SayTo without the flag, Say, a conversation between actors with and without
  `headTrack` 0 (and its reply); `dialogue::say_to_with_no_target_look_turns_nobody`:
  SayTo with the flag to the player and to an actor (#2796).
- Prospector Saloon, `--play`, Sunny Smiles (`SunnyRef`) saying GREETING lines by the test
  aid: to Cheyenne 157° behind her, the AI turned her body and the line ended with
  Cheyenne +0° off her front; Cheyenne (a dog) was given Sunny as her head's target
  (-67° off, beyond a head's reach; a SayTo listener's body stays). Whether a dog's head
  turns depends on its skeleton and was not checked. The same with `--no-npc-packages`: `speech_look.rs`
  turned her -160° in 1.06 s, ending at +0°. To the player 96° off: ended at +0°. With
  `noTargetLookFlag` 1 to GoodspringsSettler04Ref and to Cheyenne: only Sunny turned
  (ended at -1° and +0°); from #2796 to #3989 nobody turned for those lines, since then
  Sunny turns again.
- The unattended opening (`--new-game`): Doc Mitchell's 13 SayTo lines to the player in
  the first 90 s turn his head; while he sits on his chair (001059B0) the body stays, as
  §8.1 has it for sitting actors. The opening's idle summary and stage 55 are as before.
- Not looked at in a picture: the turns were checked in the log's angles only.

### 9.5 Open

- Whether the original turns the speaker's body (or only its head) is not known; the
  owner chose head and body.
- What `headTrack` left out means, and what it does in a conversation with the player.
- A listener's body stays for SayTo lines: with the speaker behind it, its head stops
  at its limit (60°).

## 10. Fort equipment return regression (#2534)

[documented: #2534 owner brief, fnvtool ESM decode/script decompile,
FalloutNV.esm, main session 2026-11-14] The weather-station guard's GREETING
`0x0015BCA2` offers Topic000 (INFO `0x0015BCA1`). That choice and Topic010's
"Done." INFO `0x00165712` set `FortEquipmentConfiscationQuest.bGimme` in their
begin result and start the equipment quest in their end result. The quest
(`0x00164762`, script `0x00164763`, delay 0.1 seconds) transfers all eligible
items from persistent `NVCCPlayerStorageREF` (`0x00139B6B`, Cottonwood Cove)
to the player when `bFortEquipmentStored` and `bGimme` are 1, clears both flags
and stops. Its return notification follows the transfer, except in
`FortificationHillWeatherStation` (`0x0012428F`), where it is omitted.

[observed: main session's authorized `fnvtool script run` and `script roundtrip`
results on upstream main, 2026-11-14, supplied in the #2534 owner brief] One
`Weap10mmPistol` moves from the player into storage at the entrance and returns
after Caesar's bunker task and the weather guard's Topic000. A save/load before
the middle step also succeeds. This is headless evidence; the reported engine
failure has not been reproduced in this worktree.

[design: `engine/nvf-play/src/dialogue.rs` input, `script.rs` `with_world`,
`Runtime::player_action` and `tick`; `crates/fnv-game/src/dialogue.rs`
`run_line_result` and `finish_line`] Engine dialogue choices and skipped
responses call the shared `Game` implementation while the played simulation
world is swapped in. Begin and end results run through that shared code; the
runtime's inventory snapshot records the outcome rather than restoring an older
inventory. The engine tick likewise calls `Game::tick` on the played world.

[design: `crates/fnv-game/src/game.rs` `tick`, `start_quest`, `stop_quest`]
Only running quests are scheduled. Their configured delay advances on separate
GameMode and MenuMode counters. Dialogue and message boxes hold GameMode; after
leaving dialogue or closing a box, the equipment quest can run its GameMode
return branch. Starting a stopped quest resets its counters; stopping it prevents
later ticks. No separate engine quest scheduler was found in this path.

[documented: GECK wiki **RemoveAllTypedItems**, offline snapshot 2026-10-28,
revision 31990] Omitting the type code transfers all eligible items; the
suppress-messages argument hides the item notification. [design:
`crates/fnv-game/src/functions/items.rs` `remove_all_typed_items`, `remove_all`;
`crates/fnv-sim/src/world.rs` `get`, `inventory_mut`; `refs.rs` `from_index`;
`engine/nvf-play/src/sim.rs` `SimState::new`] The transfer directly changes
the shared `World` inventory. References are indexed independently of rendered
cells, and inventory access materializes reference state on demand; the transfer
does not require the storage container's cell to be streamed or its lock opened.

[design: `engine/nvf-play/src/menu.rs` `open_menus`;
`menu/model.rs` `MenuModel::open_inventory`; `pipboy/items.rs` `draw`]
Opening the Pip-Boy builds its inventory model from the played world, and the
ITEMS page renders that model. A return made before opening it therefore needs
no separate inventory-copy notification. No supported engine cause was identified
by this source inspection, so #2534 adds regression coverage without changing
runtime behavior. An in-game pass remains necessary before claiming the report
is resolved.

[design, #2534] Smoke coverage:

- `mq-fort-gear-returned` runs
  `crates/fnv-game/walkthrough/fort-gear-returned.actions.json`, preserving the
  supplied actions with rewritten explanatory notes. It checks the return
  greeting/choice, player/storage pistol counts 0/1 then 1/0, and stopped quest
  with both flags reset.
- `fort-gear-returned` isolates the engine bridge in Goodsprings while the storage
  is remote. Console aids confiscate a test pistol, seed both quest flags and start
  the quest. The console closes before GameMode is allowed to return it. Later
  console reads check the same four counts, stopped quest and cleared flags. This
  scenario bypasses the long dialogue route; it does not establish that the
  reported weather-guard conversation succeeds in-game.

[design, #2534] If both checks pass but the report persists, reproduce the full
guard conversation in the engine and inspect the chosen INFO, its begin/end
results, the two quest variables, running state and any menu still holding
GameMode. The existing `mq-legion` and `console-quest-info` scenarios provide
related chain and console coverage. No new parity value, setting, or maintainer
choice is introduced.

