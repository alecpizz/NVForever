# Spec notes: idle animations played on request (scripts, packages, dialogue)

Clean-room notes in plain language on the idles an actor is *told* to play: by a script's
`PlayIdle`, by an AI package (its begin, end and change idles and its idle collection), by
an idle marker, and by a dialogue response. The IDLE records, the idle tree and the `.kf`
files are in docs/spec-notes/animation.md; the editor documentation on packages, idle
collections and markers is paraphrased in docs/spec-notes/actor-behaviour.md §1.4, §1.5
and §5. Of the idle manager (idles chosen from the tree by their conditions with nobody
asking) the drawn-weapon fidgets (§11) and the talking gestures (§12) are built.

Tags: **[documented]** cites a public source, **[observed]** was read from the owner's
install (FNV 1.4.0.525, `FalloutNV.esm`, `Fallout - Meshes.bsa` and `Update.bsa` only, with
`fnvtool esm decode`, `fnvtool ai show`, `fnvtool dialogue lines`, `fnvtool script
decompile`, `fnvtool nif decode` and `fnvtool nif anim --filter`), **[hypothesis]** needs
checking against the original game, **[design]** is our own choice.

Code: `crates/fnv-game/src/idles.rs` (who is asked to play what, headless),
`crates/fnv-actor/src/idle_play.rs` (one idle playing: loops, fades),
`engine/nvforever/src/idles.rs` (playing them on actors and on the player),
`engine/nvforever/src/player_body.rs` (the player's rigs and view).

## 1. Where idles are asked for

- **Scripts**: `PlayIdle <name>` takes the idle's Editor ID as text and plays it on the
  calling actor [documented: catalogue; observed: 197 calls in `FalloutNV.esm`'s scripts,
  318 in the ten masters, scripts.md §3]. The catalogue's note on `PickIdle` says the
  forced function plays an idle "regardless of conditions" [documented].
- **Packages**: each of the three state changes (begin, end, change) can name an idle
  (PACK `INAM` after `POBA` / `POEA` / `POCA`); the result script runs first, so the idle
  sees what it set [documented: GECK wiki, Category:Packages]. 51 / 25 / 23 packages name
  a begin / end / change idle [observed: packages.md §2]. A package can also carry an
  **idle collection** (`IDLF` flags, `IDLC` count, `IDLT` timer, `IDLA` idles): 267
  packages do [observed].
- **Idle markers** (IDLM) carry a collection of the same layout; a marker's idles
  override the package's [documented].
- **Dialogue**: each response can name an idle for the speaker (`SNAM`) and one for the
  listener (`LNAM`) [documented]; 1,684 responses name a speaker idle and 1 a listener
  idle [observed: dialogue.md §2].
- **Furniture**: getting on, staying on and getting off furniture are idles chosen from
  the tree by their conditions (furniture.md §3). One of them belongs here, §6.

## 2. The opening as data [observed]

The test case: Doc Mitchell's house at the start of a new game (quest VCG01).

| Moment | What the data does |
|---|---|
| VCG01 stage 0 | `player.AddScriptPackage VCG01PlayerSection0` |
| 0.2 s, then 2.8 s later (quest script timer) | stages 1 and 3; stage 3: Doc Mitchell `SayTo player VCG01Intro` |
| end of the first line | its end script sets stage 5: `player.AddScriptPackage VCG01PlayerSection1` |
| 3.25 s later | stage 7 gives the same package again, stage 8 the next line, whose response names the speaker idle `VCG01DocWhoaThere` |
| later lines | speaker idles `SitChairTalkPlayerD`, `SitChairTalkPlayerA`, `VCG01DocGiveMirror` |
| stage 30 | `DocMitchellREF.PlayIdle VCG01DocGiveMirror` |
| end of line 0x00104BF9 | `player.AddScriptPackage VCG01PlayerSection2`, and the next line |
| end of the next line (0x00104BFA) | stage 45 and `DocMitchellREF.EvaluatePackage` |
| stage 50 (3 s later) | `player.RemoveScriptPackage`; stage 55 gives the controls back |

The three player packages are Travel packages "near editor location", radius 0, Must
Complete, with these idles:

| Package | Begin idle | Collection (Run In Sequence, timer 0) | Change idle |
|---|---|---|---|
| `VCG01PlayerSection0` | `LooseVCG01PlayerWakeup` | Wakeup, Situp | none |
| `VCG01PlayerSection1` | `LooseVCG01PlayerSitup` | Bedsit | `LooseVCG01PlayerBedsit` |
| `VCG01PlayerSection2` | `LooseVCG01PlayerStandup` | Situp | none |

The idles (all childless roots without conditions, group byte 0x47: Special Idle with
the 0x40 bit):

| Idle | Loop min / max | Clip length | Text keys |
|---|---|---|---|
| `LooseVCG01PlayerWakeup` | 255 / 255 | 6.00 s | `StartLoop` 0.03 s, `EndLoop` 5.97 s, `BlendOut:10`, two `Sound:` keys |
| `LooseVCG01PlayerSitup` | 0 / 0 | 5.40 s | `BlendOut:10`, a `Sound:` key |
| `LooseVCG01PlayerBedsit` | 255 / 255 | 8.17 s | `BlenIn:30` (so spelled), `BlendOut:10` at 1.47 s, `StartLoop` 2.10 s, `EndLoop` at the end |
| `LooseVCG01PlayerStandup` | 0 / 0 | 11.13 s | `Blend:10`, two `Sound:` keys, `Enum: Left` / `Right` |

All four clips lie in `meshes\characters\_male\idleanims\` but drive nodes of the
**first-person** skeleton: `Camera1st` (priority 95), `Bip`, `Bip01 NonAccum`, the
spine, neck (`Bip01 Neck1` at 55) and head, the clavicles, upper arms and forearms (20);
15 tracks each, none for `Bip01`. `Camera1st` and `Bip` exist only in
`characters\_1stperson\skeleton.nif` (animation.md §9.5).

Doc Mitchell's packages (`VCG01DocMitchellFirstPosition`, `…BedsideStandingPackage` and
four more) each list one idle in a Random collection: `NPCSmokingIdle`, an idle of the
tree with conditions (not running, a faction, an item not equipped, a random percentage
below 30).

## 3. Playing one idle

- **Loop counts**: the editor offers "Loop Forever" (the animation loops continually)
  and a minimum and a maximum "number of times an animation will loop", all three only
  for animations that can loop [documented: GECK wiki, Idle Animations Window, §9].
  **Loop Forever is stored as minimum and maximum both 255** [observed: the owner's GECK,
  2026-10-10: `LooseVCG01PlayerWakeup` shows Loop Forever ticked with both fields at 255
  and greyed out; ticking the box sets both to 255; with the box clear any minimum and
  maximum can be entered, a maximum of 255 included]. So only 255 / 255 loops for ever
  (93 idles of the ten masters, animation.md §1.1), and any other pair is a number of
  times from the minimum to the maximum. Both 0 (1,462 idles) play the idle once
  [hypothesis: at least one pass]. The same window shows "Must return a file" ticked on
  that record, whose group byte is 0x47: bit 0x80 clear, as xEdit reads the flag
  [observed].
- **Loop stretch** [hypothesis]: a looping idle plays to its `EndLoop` key and goes back
  to its `StartLoop` key; without the keys the whole clip repeats. On its last pass it
  plays on to the end of the clip.
- **Fades** [hypothesis]: `Blend:N` and `BlendIn:N` give the fade in and `BlendOut:N` the
  fade out, N in frames at 30 a second (as for movement clips, animation.md §9.3); the
  time a key sits at means nothing (the Bedsit clip's `BlendOut` key comes before its
  loop). Without a key: 0.2 s [design].
- **End** [design]: a one-shot idle ends at the end of its clip, holds its last pose
  and fades out over its fade-out time; an idle that starts meanwhile fades in over it,
  so two idles in a row cross-fade (the sit-up into sitting on the bed).
- **Asked again** [hypothesis: owner report 2026-10-18, the game hands the mirror over
  once]: the opening asks Doc Mitchell for `VCG01DocGiveMirror` twice, by stage 30's
  `PlayIdle` and 3 s later (the stage's `fQuestTimer`, then stage 35's `SayTo`) as the
  speaker idle of "How'd I do?" (INFO 0x00104BF8) [observed: VCG01 stage scripts, INFO
  0x00104BF7 end script `SetStage VCG01 30`]. The clip lasts 3.00 s, so the second ask
  comes while the first holds its last pose and fades out; started over, Doc handed the
  mirror over a second time. Ours: an idle asked by a script or a dialogue line while
  the same idle still shows is not started over: one under way plays on, one that ended
  shows in full again and holds its last pose for another clip's length
  (`IdlePlay::again`; log `idle: … plays … again: not started over`). Collections,
  fidgets and gestures that pick the same idle again still play it again [design].
- Not read: the `Sound:` keys and every other text key.

## 4. The player

- **Which rig** [hypothesis from §2]: an idle whose clip has a track for `Camera1st`
  plays on the first-person rig; any other on the third-person body. The game's scripts
  ask the player for both kinds side by side (`player.PlayIdle Loose1stPBrahminPush` and
  `Loose3rdPBrahminPush` in one script, neither with conditions [observed]), which fits
  each going to its own rig.
- **The view** [hypothesis]: while such an idle plays, the first-person camera takes the
  `Camera1st` node's place *and rotation*, so the animation turns the view (lying on the
  back looks at the ceiling). The camera looks along the node's +Y with +Z up, the
  actor's own axes [observed in ours: the wake-up shows the ceiling fan, the seated view
  matches the owner's capture of the game to within a few pixels at 1280 × 720 once the
  two errors below were fixed].
- **The rig's other clips** [hypothesis]: the clips share priority 20 with the standing
  idle on most bones, so both cannot play; a special idle takes the place of what the
  body does by itself. Ours keeps the body's own clips under an idle only while the first
  one fades in.
- **Looking round** [design: the owner's wish, playtest of 2026-10-11]: while an idle
  drives the first-person view, the camera node alone places the view; mouse look, the
  look test aid and the movement keys are ignored (`nvf_play::player::ViewDriven`, set by
  nvforever `player_body.rs` while `PlayerIdles::held` is set; logged as "player view:
  driven by an idle's camera node …" and "… back to the player"). The first-person rig
  keeps the heading and pitch the view had when the idle started. When the last idle is
  over, the view's heading and pitch are taken from where the animation left it. The
  original's scripts leave looking on during the wake-up (VCG01 stage 0
  `EnablePlayerControls 0 0 0 0 1`, then `DisablePlayerControls 1 1 1 1 0 0 1`
  [observed]), so the original may let the mouse turn the view there; until 2026-10-11
  ours added the view's own turning to what the node gave.
- **Where the scene plays** [design, from an observation]: the opening's start marker
  (`VCG01PlayerStartMarkerREF`, 2257.0, 2318.2, 7360.0, facing south) is in Doc
  Mitchell's bed, and our collision pushes the character about 10 units off it before
  stage 0. A view-driving idle therefore plays where the game last put the player
  (`Player::placed`: the start of a game, a door, a script's move) unless the player has
  moved by themselves since, and the character is held there, colliding with nothing,
  while such idles play. What the game does with the player's collision in this scene is
  not known.
- **Letting go** [design: owner report 2026-10-16]: when the last such idle is over, the
  character stands where it stood when the first one began (where the collision had
  pushed it, clear of the bed), not where it was held. The stand-up ends with the view
  over the marker (eye 0.0, 3.7, 118 units from it [observed in ours]), but a character
  let go at the marker rose 20 units onto the bed frame (z 7360.7 to 7380.6, nothing
  sideways) [observed in ours]; let go at the earlier spot, 22 units west, it stays on the
  floor, and the view steps that far aside as the idle ends. Logged as "player view: idle
  over; the character stands at …" and, a second later, "… a second after the idle the
  character stands at …" (smoke `idles-opening`).
- Errors found against the owner's capture (2026-10-10) and fixed: the hand-over of the
  view at the end of an idle fired in the one frame between the sit-up and the sitting
  loop and turned the view 11° to the right; and the character was 10 units off the
  marker.

## 5. Rules as built (`fnv_game::idles`)

- An idle asked for is queued for whoever animates actors and traced: `idle_asked`
  {`actor`, `idle`, `source`}, `source` one of `script`, `package_begin`, `package_end`,
  `package_change`, `package_idles`, `marker_idles`, `speaker`, `listener`. An IDLE that
  names no `.kf` (an inner node of the tree) is not played (`idle_not_played`)
  [hypothesis: only idles with a file can be listed in a collection, documented; that
  `PlayIdle` on an inner node plays nothing is ours].
- `PlayIdle` plays whatever its conditions say [documented]. `IsIdlePlaying` answers
  what the animating layer reports (`Game::set_idle_playing`); headless, nothing is ever
  playing (as before, when the function was a stub returning 0).
- A package's begin, end or change idle is asked for after its result script and before
  the actor's `OnPackage…` block [the first documented; the second ours]. They are not
  tested against the idle's conditions, nor are dialogue idles [hypothesis].
- **One script package** [documented: actor-behaviour.md §1.4]: `AddScriptPackage`
  replaces the actor's script package. (Before, ours stacked them, so the opening's
  `RemoveScriptPackage` brought the previous one back and the player sat up again.)
- A dialogue response's idles are asked for when the response starts: in a conversation
  the listener is the player; for `SayTo` and `Say` lines the responses after the first
  start as the time of each before passes.
- **Collections** (`IdlePicker`) [documented unless tagged]: Random picks any listed
  idle, with the idle timer between two idles; Run In Sequence goes from the top, with
  the timer between whole sequences; Do Once plays the sequence once (a random collection
  then plays one idle [hypothesis]). The first idle is not waited for [hypothesis].
- **A collection's idle must pass its own conditions** [hypothesis]: every package of
  Doc Mitchell lists `NPCSmokingIdle`, whose conditions he does not meet, and he does not
  smoke in the game [owner's capture: he sits with his hands on his knees]. Conditions
  are evaluated like any others (`Game::conditions_pass`); a function the runtime does
  not have answers 0.
- **When a collection plays** [design, simpler than the documented per-type rules of
  actor-behaviour.md §1.5]: while the actor stands where its package has it with nothing
  else to do (not walking, not on furniture, not talking) and plays no other idle; the
  idles of the marker it stands at come before its package's. The player's collection
  plays while they have a script package. A package flagged No Idle Anims plays none.
  A collection idle ends when its actor walks off or is drawn into a conversation (the
  conversation's gestures, §12, take its place; before #1745 Chet's marker sweep played on
  and held him from turning to the player).
- **Seated by a Find or Use Item At package** (2026-11-08) [documented: Category:Packages,
  "Find: Idles play when 'activating' the target of the find", Use Item At "at its
  location", actor-behaviour.md §1.5]: an actor seated on furniture (fully on it) whose
  package is Find or Use Item At plays that package's collection there, laid over the
  sitting clip by track priority as a seated speaker's gestures are (§7), instead of the
  tree's furniture fidgets; it ends when the actor starts getting off. Fallout 3's
  G.O.A.T. (CG03, Tale of Two Wastelands) seats its students by Find packages listing
  `LooseSitTakingTestA`, `LooseSitTakingTestB` and `LooseSitCheatingOnTest` (FalloutNV.esm
  leftovers, Special Idle, loop 20 / 20, `SitTakingTest*.kf`) [observed: `fnvtool ai show`,
  `esm decode`]; before, they leaned back and fidgeted in their chairs (owner report: "none
  of the NPCs look to be sitting at the school desks"). Other package types keep the rule
  above [hypothesis: Eat and Sleep are documented too, not built here].
- **A package's idles end with the package** [design]: an idle that loops for ever and
  was asked for by a package or marker leaves its loop (plays out to the end of its clip)
  when that is no longer the actor's package or marker. A one-shot idle plays to its end
  whatever happens to the package [hypothesis from §2: the stand-up is 11.1 s and
  `RemoveScriptPackage` comes about 9.4 s into it; the stand-up plainly finishes in the
  game].
- With these rules the opening needs no special case: Section 0's begin idle loops until
  Section 1's begin idle (the sit-up) replaces it; after the sit-up the collection plays
  Bedsit, which loops until Section 2's begin idle (the stand-up); the package is
  removed before the stand-up ends, so Section 2's collection (Situp) never plays.
  Why Section 0 and Section 2 list the idles they do is not explained by this reading.

## 6. Doc Mitchell getting up from his chair [observed unless tagged]

- The idle `NVchairStandDoc` names `SpecialIdle_NVDocChairFrontExit.kf` (12.7 s as
  played) and sits in the chair part of the idle tree (parent `StandUp`), with the
  conditions `GetStage VCG01 >= 10`, `GetStage VCG01 <= 50` and `GetFurnitureMarkerID ==
  14`: while the opening is between those stages, an actor getting up from a seat entered
  from the front plays it instead of the plain exit.
- Doc Mitchell leaves his chair when his package changes to
  `VCG01DocMitchellBedsideStandingPackage` (condition `GetStage VCG01 >= 40`).
- Two more records look made for the same moment and are dead data: `LooseNVDocHelpPlayerUp`
  names `SpecialIdle_NVDocHelpPlayerup.kf`, which is in neither archive, and
  `LooseNVDocChairExitLeft` names an existing file; no record of `FalloutNV.esm` refers to
  either by FormID, and neither name occurs in any script.
- Built: the furniture idles' conditions can ask the game for a quest's stage
  (`fnv_actor::furniture::UseIdles::pick_in`, `GetStage` only), so Doc Mitchell gets up by
  his own clip. Before, a condition on anything but the furniture's user failed and he got
  up by the plain exit.
- Open [hypothesis needed]: *when* he gets up. In ours his package is chosen again by the
  periodic check, which falls about 0.5 s after stage 40, some 3 s before the player's
  stand-up starts. The script calls `EvaluatePackage` on him only at the end of the line
  said during the stand-up. Whether the game holds an actor's package while it says a
  line is not known.

## 7. In the engine

- NPCs: the idle is laid over the actor's locomotion clips, or over its furniture clip
  (a seated speaker's gesture), by track priority, on two more blend layers (the idle and
  the one it replaces while it fades in); `nvf_assets::anim_play::MAX_LAYERS` is 6.
- The player: §4.
- The same slot plays the animation groups scripts ask of actors with `PlayGroup`
  (nvforever `actor_groups.rs`, activators.md §7): such a clip fades in and out as an
  idle does, keeps the actor's collection waiting while it plays, and is not reported to
  the rules as an idle (`IsIdlePlaying`); an idle asked for meanwhile replaces it.
- Log lines: `idle: <actor> plays <idle> (<length>, loop <from>-<to>, fades <in> / <out>)
  (<source>)`; for the player also the rig, the place and the facing; at the end `idle
  summary: N idles asked for (… by scripts, … by packages' begin, end and change, … from
  collections, … by dialogue lines); … started on actors, … on the player; … for actors
  not drawn, … without a usable clip`.
- Smoke test (the opening; CLAUDE.md):
  `NVF_PLAY_TEST_SCRIPT_MENUS=0.05 nvforever --data <Data> --play --new-game --no-voice
  --run-seconds 90` → exit 0, "run finished", `VCG01 stage 55 set`, and the summary "10
  idles asked for (1 by scripts, 4 by packages' begin, end and change, 1 from
  collections, 4 by dialogue lines); 5 started on actors, 5 on the player; 0 for actors
  not drawn, 0 without a usable clip"; in the "furniture:" lines Doc Mitchell (00104C0F)
  "gets off 001059B0 by clip SpecialIdle_DocSitLeftExit".

## 8. Not done, and questions for the owner

1. The idle manager: fidgets other than the drawn-weapon, weapon-away (§11, §11.4) and
   furniture ones, hit reactions from the tree; how often the original asks its tree
   (observer check 116). (Talking gestures: §12.)
2. Sounds and other text keys of idle clips; the response flag "Use Emotion Animation"
   and facial emotions. (Animated objects: §10.)
3. Package begin / end / change **topics** (TNAM), and the idles of actors that are not
   residents (not handed to `fnv_ai`).
4. The per-type rules for when package idles play (actor-behaviour.md §1.5); ours is one
   rule for all types (§5).
5. Third person: a first-person idle does not move the third-person body (and the
   opening's clips have no leg tracks).
6. Captures that would settle hypotheses: the loop counts (an idle with loop 2–4 played
   by `PlayIdle`, counting repeats); whether `PlayIdle` on an idle with failing conditions
   plays; whether an actor with a collection plays an idle whose conditions fail; when
   Doc Mitchell gets up (§6).

## 9. The editor's Idle Animations window [documented]

From the GECK wiki page "Idle Animations Window" (the owner's paste, 2026-10-10), in our
words, with where ours stands:

- **Animation group section**: only one animation file plays on an animation group at a
  time, so an idle on a group ends what played on that group. Whole Body ends all other
  animations; Upper Body ends those of the upper body's groups; most idles use Special
  Idle. Ours: an NPC's idle is laid over its other clips by track priority whatever its
  group; the player's idle stops the rig's other clips once it covers the rig, also for
  the Special Idle group (§4). Neither follows the group yet.
- **Looping**: Loop Forever, minimum, maximum (§3). **Replay delay**: the time between
  replays; not used by ours.
- **No Attacking**: no attacking while the idle plays. Built 2026-10-25 for fighters
  (`IdleDef::no_attacking`, DATA flags bit 0; `npc_combat.rs`): while an idle with the
  flag plays, the fighter starts no shot or blow, and the idle is not cut short by one
  (§14.2). 67 IDLEs set it [observed: the ten masters]: the people's `MTHit*` hit
  reactions (played by `deaths.rs` as staggers, which hold attacks already), the
  first-person `1stPHit*` ones, and creatures' combat idles: the gecko's hiss (not its
  hops), the giant rat's growl and alarm cry, the cazador's dodges, wing hits and high
  flight, the Dead Money ghosts' hops, the spore carrier's get-ups and the behemoth's
  bash.
- **Picking** (the idle manager; it confirms the tree walk of animation.md §2.2): the top
  item matching the actor's skeleton, then its first child; an idle whose conditions fail
  is passed over for its next sibling; one whose conditions pass has its children tried
  the same way; the idle whose conditions pass and none of whose children's do is the
  one to play.
- **Must Return a File**: when the idle picked has no file, the flag decides: set, it
  is skipped as if its conditions had failed and its next sibling is tried; not set,
  nothing is played. Ours (`fnv_actor::furniture::UseIdles`, the only tree walk built)
  always goes on to the next sibling, as if the flag were set on every idle.
- **When the idle manager is asked**: `PickIdle`; getting up after a knock-down or
  unconsciousness; yielding in combat; constantly while sleeping or eating; by the
  speaker and by the listener each time a new line of a conversation is said; and at
  random times by the actor's energy level while its lower body is idle (low energy
  idles more often). None of these is built (§8 item 1).

## 10. Animated objects (ANIO): props held while an idle plays

- **The record** [documented: xEdit wbDefinitionsFNV.pas ANIO; schemas/fnv/placed.json]:
  EDID, MODL (the model) and DATA "Animation", an IDLE.
- **Counts** [observed, FalloutNV.esm with the owner's load order through NVForever's
  reader]: 161 ANIO records, 141 with a model and a DATA that names a loaded IDLE, for
  116 IDLEs; the other 20 include the 18 whose DATA lies outside the master list
  (esm.md item 7).
- **Nothing in the idle clip names the object** [observed]: `sweeping.kf`
  (`SpecialIdle_Sweeping`) drives only `Bip01` nodes and has the text keys start,
  StartLoop, five `Sound: NPCHumanSweep`, `Blend:10`, EndLoop, end. The link is the
  ANIO's DATA: `aoSweepingBroom` (0x0008945D, `AnimObjects\aoBroom.NIF`) names
  `LooseSweeping` (0x0008945C, `sweeping.kf`); `aoSmokingStanding` (0x00083519) names
  `NPCSmokingIdle` (0x00071EE3) and `aoLooseSmokingStanding` (0x00071F91) names
  `LooseSmokingStanding` (0x00083518), two IDLEs that play the same
  `smokingstanding.kf`, each with an ANIO of its own, both with `aoCigarette.NIF`
  [observed]. Reading: an ANIO is drawn on the actor while the IDLE its DATA names plays
  [hypothesis].
- **Where it hangs** [observed]: the model's root (`AnimObjectRoot`, identity
  transform) carries a `Prn` string extra data naming a skeleton bone, as worn rigid
  parts do (actors.md §1.2): `aobroom.nif` `Bip01 L Hand`, `aocigarette.nif`
  `Bip01 R Hand`. `aobroom.nif` also has a transform controller on its root (not played
  here) and `BSPriority#50#` in its `UPB`.
- **In the engine** (`engine/nvforever/src/anim_objects.rs`) [design, from the above]:
  while an actor's idle slot plays an IDLE that has animated objects (until it has faded
  out), each object's complete model hierarchy is drawn under its `Prn` bone (root fallback); when another idle starts
  or the idle is over, the objects are taken away. Log: `anim object: <actor> holds
  <model> on <bone> (<ANIO>, idle <IDLE>)`, and `anim objects summary: … objects drawn
  for … idles played, … could not be drawn` at the end.
- Seen in NVForever, 2026-10-11 [observed in NVForever]: Chet (GSChetRef) at his
  `GobSweep` idle marker in GSGeneralStore plays `LooseSweeping` and holds the broom in
  both hands, its head on the floor; Sunny Smiles asked to play `NPCSmokingIdle` (test
  aid `play_idle:SunnyRef:NPCSmokingIdle`) is given `aocigarette.nif` on `Bip01 R Hand`
  (log; the frame taken shows her greeting the player, a dialogue idle having replaced
  the smoking one and the cigarette gone with it).
- **Nodes the idle's clip moves** [observed: files; hypothesis for the rule]: Doc
  Mitchell's `VAODocGiveReflectron` (0x0010A433) names `VCG01DocGiveMirror` and
  `animobjects\NVPropReflectron.NIF`. That model's root `Dummy01_Reflectron` has `Prn`
  `Bip01 R Hand`; below it `##Dummy01_Reflectron01` carries the rest (`##NV_Reflectron`,
  the screen and glow shapes) and is stored about 196 units from the root
  (86.9, 160.5, -71.2), with a transform controller of its own. The idle's clip
  `SpecialIdle_NVCG01DocGiveMirror.kf` (3.00 s, text keys start and end only) has,
  beside the body tracks, a `Weapon` track and a track for `##Dummy01_Reflectron01`
  (B-spline translation, rotation and scale). Drawn rigid, the mirror hung 2 m from his
  hand, out of sight (owner report 2026-10-18: not shown before "How'd I do?"). Ours:
  the node of an object's model that the idle's clip has a track for is posed by that
  track every frame, under the `Prn` bone with the transforms above it in the model
  (`anim_objects.rs`; log `anim object: … node ##Dummy01_Reflectron01 follows the
  idle's track`).
- **Objects hung from the accumulation root** (#3610) [observed: `fnvtool nif decode`;
  hypothesis for the rule]. Four of the 60 models in `meshes\animobjects` have `Prn`
  `Bip01`:
  - `aogenericmeal.nif` (`aoTableChairEatingGenericMeal…`, the table eating idles) and
    `aostudenttestpaper.nif` share one root rotation (about 90° about z and 50° about
    x). Their one node is stored at (-1.2, 41.6, 55.6) and (2.5, 50.9, 55.6).
  - `hchamberfx.nif` and `hchamberfxlso.nif` have an identity root (§10.1).

  The animator holds a clip's accumulation root (`Bip01`) at the identity (actors.md
  §7.2). The eating clip `SitTableChairEatA.kf` has `Bip01` as that root and no track
  for it. Under the identity, the meal lay 69 units to the side of the seated actor and
  3 units above the floor: Lt. Hayes at Primm ate from no plate (player report). Under
  `Bip01`'s bind in `characters\_male\skeleton.nif` (turned 90° about z, 67.8 units
  up), the same numbers put the plate 70 units in front of him and 71 up, at the
  table. Ours: an object whose `Prn` is the playing clip's accumulation root gets that
  node's bind put back between the node and the object (`held_root_bind`). Mr House's
  skeleton has an identity `Bip01`, so the chamber effects do not move. An object whose
  `Prn` names a node the skeleton lacks logs `anim object: <model>: its parent node
  <name> is not in the skeleton; hung from the root` (warning). Observer check 290.
- Not done: the player's idles (`VAOPlayerTakeReflectron`, 0x0010BAF1, gives the same
  model to `LooseVCG01PlayerTakeReflectron`, which no script or dialogue line of
  FalloutNV.esm names [observed]), the objects' own controllers, and checking against
  the game how long an object stays (the whole idle, as here, or only between keys).

### 10.1 Chamber effects and external KF controllers (#2685)

- [observed: data, main session 2026-11-15, issue #2685, FalloutNV.esm script/record
  and Fallout - Meshes.bsa NIF/KF decode, upstream 1790ab94a] ANIO `hChamberFXAmin`
  (0x0016125F) names `animobjects\hchamberFX.NIF` and DATA IDLE `MrHouseOpen`
  (0x0013A23A); `hChamberFXAmin02` (0x001618FB) names
  `animobjects\hchamberFXLSO.NIF` and DATA `MrHouseOverloadLifeSupport` (0x0013A818).
  These two links support the reading DATA = idle.
  [documented: offline GECK wiki 2026-10-28 snapshot, search.py --title "Animated Object" --read]
  The requested wiki title was tried and is absent.
- [observed: data, same main-session decode, #2685] `WakeUp.kf` has one CLAMP sequence,
  `SpecialIdle_Wakeup`, 0..19.4 seconds. Its SELF_ILLUM tracks target the creature's
  NIFZ parts (heart, helmet lights, back light, shell buttons and bed console).
  Its smoke shapes and five particle systems instead belong to the ANIO model,
  whose root `Effects` has `Prn` = `Bip01`. Alpha/UV, emitter birth/on-off, modifier
  on-off and particle emittance tracks belong to the same KF.
- [design, #2685; engine/nvforever/src/anim_objects.rs, nvf-assets/src/model.rs]
  Prepare the ANIO against the external KF, so its target shapes get the material
  features and hierarchy they require. Play transforms and material tracks with the
  ordinary placed-model players, synchronized to the actor idle's clock. Preserve the
  model's authored root transform below Prn; fall back to the skeleton root when Prn
  is absent or unavailable. Replacement/stop removes the complete attachment tree
  after the existing cross-fade. An object's independent controllers remain outside
  this external-KF playback rule.
- [design, #2685; idles.rs::HOLD_CLAMP_EFFECT_IDLES, fnv-actor/src/idle_play.rs]
  A CLAMP idle carrying material/emitter tracks plays once and keeps its exact final
  sample at full weight until replaced or stopped. Ordinary finite idles still fade
  away. This keeps glow tracks and attached effects alive; keyed emitter-off values
  suppress new births. This provisional generalization is listed in owner-review.
- [hypothesis, #2685] The visible steam/glow envelope and lingering haze follow these
  authored tracks, with no invented fog density or emitter rates. Original-game
  timing, final appearance and reload behavior still need observer check **212**.
- [design: crates/fnv-game/src/game.rs::object_groups, engine/nvforever/src/object_anims.rs]
  Enabled ground fog (REFR 0x001629DD, model `effects\NV\hChamberGroundFog.NIF`,
  `Forward` CLAMP 0..52.33 s [observed: data, main session #2685]) already draws through
  `script_draw`, plays its sequence on `PlayGroup Forward 1`, and restores the saved
  group at its end. [design, #2685; nvf-assets/src/morph.rs::play_sequence_morphs]
  Its manager morph now binds `Forward` by shape/frame name and follows the same
  `Playing::sequence_time` as UV/alpha, including the held or restored endpoint.
  Each rig receives a private morph mesh; other copies keep their own pose.
- [design: engine/nvforever/src/idles.rs::ActorIdle; crates/fnv-game/src/game.rs saved
  state] Actor idle slots and their playback times are not serialized. Consequently
  this change retains glow/ANIO state during the live session, but cannot restore a
  held actor idle after load or actor respawn. Implementing that requires a separate
  idle snapshot/reconstruction path; no new save fields are introduced here. Fog's
  existing saved group state continues to work independently.

## 11. Drawn-weapon fidgets from the IDLE tree

The owner asked (2026-10-16) for NPCs with a weapon out to fidget (a rifle held up
one-handed to rest the arm), from the game's own idles, and for the player too while
standing still.

### 11.1 What the data holds [observed: `fnvtool actor anims`, FalloutNV.esm]

The people's branch with the weapon out, from the root `GeneralIdles` (conditions in
record order; `GetRandomPercent` in percent):

| Node | Section | Loops | Replay delay | Conditions |
|---|---|---|---|---|
| `GeneralIdles` | Special Idle | | | `GetEquipped` 0x168A7E != 1, `IsInCombat` != 1, `IsSneaking` == 0 |
| `NPCOnlyIdles` | Special Idle | | | package / procedure tests, `GetIsID` player == 0 |
| `NPCWeaponIdles` | Special Idle | | | `IsWeaponOut` == 1 |
| `NPC2hrIdles` | Special Idle | | | `GetWeaponAnimType` == 5 (rifles) |
| `2hrLoiter` (`2hrloiter.kf`, 18.4 s) | Upper Body | 1–5 | 10 | random < 5 |
| `2hrLoiterOnehanded` (9.7 s) | Upper Body | 0 | 0 | random < 5 |
| `2hrShiftWeight` (5.5 s) | Upper Body | 0 | 10 | random < 5 |
| `2hrCrouch` (34.9 s) | Whole Body | 1–3 | 45 | random < 0 (never) |
| `NPC2haIdles` | Special Idle | | | `GetWeaponAnimType` == 6 (automatic rifles) |
| `2haAlertScan` (12.9 s) | Upper Body | 0 | 0 | random < 5 |

- All five clips are in `meshes\characters\_male\idleanims\` with 62 tracks, all bound to
  the people's skeleton (body and legs, though the section says Upper Body).
- No other weapon type has a branch: pistols, melee and heavy weapons have no drawn-weapon
  fidgets in the tree. Childless `Loose…` roots with a drawn weapon exist (`3rdP1HPShrugs`,
  `3rdP2HRSearch`, `LOOSE2hrLoiter`, `LooseCheering2HA` …) and are for scripts.
- The player's branch `PlayerOrNPCIdles` (`IsWeaponOut` == 1, and third person or not the
  player) has one child, `3rdPMTIdles`, which wants the weapon away: the player has no
  drawn-weapon fidget in the tree.
- The creatures' branches share the roots with the people's (`CommonIdle`, `SBIdles`, …);
  each inner node's `MODL` names the folder of its idles (`Characters\_Male\IdleAnims`,
  `Creatures\…`).

### 11.2 Rules as built (`Game::weapon_fidget`, `engine/nvforever/src/idles.rs`)

- **The walk** [hypothesis, animation.md §2.2]: roots in chain order, those of the
  actor's folder only [hypothesis: the inner nodes' `MODL`] and with children (childless
  roots are played on request). The first node whose conditions pass is gone into; its
  children in chain order the same way; the first leaf that passes is the pick. A node
  whose children all fail is left for its next sibling [hypothesis].
- **Only the branches built** [design]: the pick is played only when a node on its way
  gates the kind asked for: `IsWeaponOut == 1` for the drawn-weapon fidgets, the weapon
  away for an NPC with its weapon away (§11.4), the furniture states; any other leaf
  (the rest of the idle manager) is not built.
- **The player** [design, owner 2026-10-16]: the walk starts at each node that requires
  the weapon out below the roots (`NPCWeaponIdles`, `PlayerOrNPCIdles`), judging its own
  conditions and those below, so the player gets the people's rifle fidgets. Only in
  third person (a fidget would sway the first-person view, which follows the body), with
  the feet still, the weapon ready and not aiming, firing or reloading, and no script
  package.
- **When** [design]: an NPC standing still (no walking velocity) with its weapon in the
  hand, not in an idle collection and not playing another idle, is checked every 2 s;
  with the rifle branch's three 5% leaves that is a fidget about every 14 s, an automatic
  rifle's about every 40 s.
- **Replay delay** [hypothesis: seconds]: a leaf is skipped for its replay delay after
  the same actor last started it.
- **Ends** [design]: a fidget fades out when its actor moves or puts the weapon away
  (the player also: aims, fires, reloads, goes to first person, or a script package
  starts).
- **IsWeaponOut and GetWeaponAnimType on NPCs**: the rules are now told each drawn
  NPC's weapon state (in the hand once the equip clip has put it there), as they were
  already told the player's [documented: the value changes when the weapon is attached
  to the weapon node, catalogue notes of IsWeaponOut], and which weapon the engine
  draws for it: NPCs' weapons are not equipped in the rules' inventories, so
  `GetWeaponAnimType` said unarmed (1) for every NPC before. It now falls back to that
  weapon when nothing is equipped [design].
- Checks: `fnvtool actor fidgets` (headless: an actor's weapon put out, the tree asked
  1,000 times; with a varmint rifle about 15% of checks give a fidget, split between
  the three rifle idles, with a service rifle about 5% the scan, with a 9mm pistol
  none, for an NPC and the player alike [observed]); smoke tests `weapon-fidgets-tree`
  and `weapon-fidgets` (tools/smoke/scenarios.toml).
- In a game where nothing has happened, Sunny Smiles is in the dialogue procedure
  (`GetCurrentAIProcedure` 4, she is to greet the player) and the walk enters
  `DialogueIdles` for her: no fidget [observed with `fnvtool actor fidgets`].

- The same walk gives the fidgets on furniture (`FidgetKind::OnFurniture`): the
  branch below a node requiring `GetSitting == 3` or `GetSleeping == 3`
  (furniture.md §8).

### 11.3 Open

- How often the original looks at its tree, and the unit of the replay delay (observer
  check 116).
- Whether the original walks on after a branch without a passing leaf (here: yes).
- Whether a fidget is cut by moving or plays out on the upper body (here: cut).

### 11.4 Fidgets with the weapon away (#2026)

In the original a standing NPC with its weapon away plays fidgets now and then (biting
its nails, picking its nose, swatting, smoking), and its factions decide which.

**What the data holds** [observed: `fnvtool actor anims`, FalloutNV.esm]. Below
`NPCOnlyIdles` (§11.1), after `NPCWeaponIdles`:

| Node | Section | Loops | Replay delay | Conditions |
|---|---|---|---|---|
| `NPCNonWeaponIdles` | Special Idle | | | `GetEquipped` 0x168A7E != 1, `IsWeaponOut` == 0 |
| `NPCPipboyIdles` | Special Idle | | | `GetStageDone` > 0, `GetEquipped` 0x236D8 == 1, `IsMoving` == 0, random < 5 |
| `PipboyExamine`, `PipboyManipulate` | Movement | 0 | 45, 75 | random < 80; none |
| `NPCSmokingIdle` (`SmokingStanding.kf`, 8.4 s, a leaf) | Special Idle | 2–6 | 120 | `IsRunning` == 0, `GetInFaction` SmokerFaction 0x000711A4, `GetEquipped` 0xCC824 != 1, random < 30 |
| `NPCGeneralIdles` | Special Idle | | | `GetInFaction` GeneralIdleFaction 0x000711A6, random < 1, `GetEquipped` 0x8945E != 1, `GetEquipped` 0xCC824 != 1 |
| `Swatting`, `PickingNose`, `PickingNose02`, `BitingNails`, `ScratchingSelf`, `ScratchingSelf02` | Special Idle | 0 | 300, 25, 55, 35, 25, 25 | random < 12 (and `IsLastIdlePlayed` 0x6B8F9 != 1), < 28, < 42, < 70, < 84, none |

and the root `EatIdles`, before `GeneralIdles` in the chain: `IsWeaponOut` != 1,
`GetEquipped` 0xCC824 != 1, `GetCurrentAIProcedure` == 10 (EAT), with
`BeerDrinkingStanding` (`GetIsUsedItem` 0x5C365 == 1) and `SodaDrinkingStanding`, both
`DrinkingStandingA.kf`, loops 3–8. The player's `3rdPMTIdles` (`IsWeaponOut` == 0) lies
under `PlayerOrNPCIdles`, which wants the weapon out: never reached.

**The original** [observed: AI observer capture of 2026-11-10, Goodsprings by Easy
Pete's chair, 360 real s; four placed NPCs on `DefaultSandboxNoMoveCurrentLocation200`,
none drew a weapon]: an NCR trooper and a wastelander (both factions) began 4 and 6
fidgets (smoking, nails, nose), a travelling merchant (general only) 3 (swatting, nose,
nails) and never smoked, a Goodsprings settler (neither) none.

**Rules as built** (`FidgetKind::WeaponAway`, `engine/nvforever/src/idles.rs`):

- **The branch** [design]: the §11.2 walk, the pick kept when a node on its way requires
  `IsWeaponOut == 0` or `IsWeaponOut != 1` (not joined by OR): `NPCNonWeaponIdles` and
  `EatIdles`. No faction, idle or form is named in the code: the tree's own conditions
  (factions, random percentages, worn items, the eat procedure, the NPC-only node)
  decide.
- **Who** [design]: an NPC (not a creature, not the player [design: the player's
  standing fidgets are the drawn-weapon ones, owner 2026-10-16]) standing still, its
  weapon away (the rules' `IsWeaponOut` 0 and its hand not drawing, holding or putting
  one away), not alerted or warning, not dead, not in a conversation, not cowering, not
  in an idle collection, and nothing with idles of its own (`Game::fidget_held_off`): not
  on furniture, no current package with an idle collection, none flagged No Idle Anims
  ("Turns off Idle animations during package" [documented: GECK wiki,
  Category:Packages]).
- **Random numbers** [engineering choice]: the walk's `GetRandomPercent` draws from the
  idle tree's own generator, as the talking gestures' does (§12.2), so polling every
  holstered NPC does not move the scripts' rolls (random dialogue, addiction checks).
  The drawn-weapon and furniture walks still draw from the scripts' generator (as
  before #2026).
- **When** [design, #2026]: every 2 s with the other fidgets (§11.2), not calibrated: the
  general node passes one check in a hundred, so an NPC in GeneralIdleFaction alone
  begins a general fidget about every 200 s here, against three in 360 s in the original
  capture. The replay delays (§11.2) space the smoking out (120 s). The original picks
  an idle when the actor's "energy level timer has run down", low Energy idling more
  often [documented: GECK wiki, Idle Animations], with no numbers given; Energy is not
  used here. Observer check 116 asks the original's cadence; `docs/owner-review.md`
  has the choice.
- **Ends** [design]: the fidget fades out when its actor moves, draws its weapon (or
  starts to), is alerted, dies, is drawn into a conversation (so it can turn to the
  other and gesture, as a collection's idle does), or something with idles of its own
  takes over: furniture, a package with an idle collection or flagged No Idle Anims, an
  idle marker with a collection. One still waiting for its clip is dropped then, so the
  collection's idle starts at once.
- **First look** [engineering choice]: in the second half of the 2 s period after the
  actor stands still, spread by its ID, so a town's NPCs that stop together do not all
  walk the tree in one frame. The other fidget kinds keep one full period, as before.
- Checks: unit tests (`crates/fnv-game/tests/idles_holstered.rs`, written fixtures;
  the engine's `idles::tests`), `fnvtool actor fidgets --weapon-away` (headless), smoke
  tests `holstered-fidgets-tree`, `holstered-fidgets` and `holstered-fidgets-playback`
  (tools/smoke/scenarios.toml); the run summary names each NPC looked at and the
  fidgets it began (`holstered fidgets: …`), counted once the clip has started.
- Test aid `NVF_PLAY_TEST_HOLSTERED_ROLL=N` (0 to 99): every `GetRandomPercent` in the
  weapon-away walks reads N (`Game::fixed_random_percent`, set around those walks only),
  so a run can make the random branches pass and check the general fidgets' clips play
  (`holstered-fidgets-playback`) without changing the chances a player gets.

## 12. Talking gestures from the IDLE tree

The owner noticed (2026-10-16) that NPCs' lips move while they talk but their heads and
bodies stand still; in the original they nod, shift and gesture. The head and body
movement comes from the idle manager's dialogue groups of the IDLE tree, which were not
walked. (The `.lip` file's head and look channels, lipsync.md §3.3, are a separate,
smaller part of it and are left for later.)

### 12.1 What the data holds [observed: `fnvtool actor anims`, `fnvtool esm decode`, FalloutNV.esm]

The people's roots, in chain order (those with children): `KnockedDownIdles`,
`HitReactionIdles`, `GreetIdles`, `MenuModeIdles`, `FurnitureIdles`,
`PanicExplosivePlanted`, `VATSMeleeAttacks`, `ActivateIdles`, **`DialogueIdles`**,
`EatIdles`, `GeneralIdles`. So a seated or sleeping actor goes into `FurnitureIdles`
before the dialogue root is looked at, and that branch has its own dialogue groups.

`DialogueIdles` (0x00003BDF): its conditions want both leg conditions (actor values 29
and 30) above 0 and the dialogue procedure (`GetCurrentAIProcedure == 4`) OR the dialogue
menu (`MenuMode 1009 == 1`). Below it, in order (conditions abridged):

| Node | Conditions | What is below |
|---|---|---|
| `TalkCombat` | `IsWeaponOut == 1` OR in combat | a leaf, `SIRelaxed.kf` |
| `BartenderDialogue`, `PushUpsTalk` | heading to the player within 30°, last idle a bartender's / push-ups | leaves |
| `TalkToPlayer` | dialogue menu, `IsWeaponOut != 1` | Colonel Autumn's group; `DialogTalking`; `DialogListen` |
| `NPCtoNPC` | the target is not the player, `IsWeaponOut != 1` | `NPCTalking` (`IsTalking == 1`) and `NPCListening` |

- **`DialogTalking`** (no conditions) holds the groups of the dialogue screen, in order:
  the bartender's loop; `BulkyArmorDialogue` (power armour equipped: its own listen and
  talk clips, `DialogPANoBodyShift…`); `ArmsCrossedLoop` (the last idle played was one of
  the arms-crossed ones: go on with arms crossed, its own listen clip while not
  talking); `FemaleWeightShiftRightLoop` (the same for the weight shift);
  `FemaleWeightShiftRight` (random < 10, female, not of four races, not a child, not of
  six classes); `ArmsCrossed` (no particular outfit, mood not Happy, random < 5, replay
  delay 240); and `NoWeightShift`, which ends the list for everybody else: its first
  child `NoWeightShiftListen` (`IsTalking != 1`) gives `DialogNoBodyShiftListenA` (loops
  for ever), then mood groups by `GetDialogueEmotion` (Anger, Happy, Surprise, Sad or
  Disgust, each also random < 10), then `NeutralNoWeightShift` (no conditions):
  `DialogNoBodyShiftTalkSubtle` (random < 90, the listen clip played twice) and
  `DialogNoBodyShiftTalkA`–`E` (random < 20, 40, 60, 80, none).
- **`DialogListen`** holds only the bartender's listen loop: everyone else has their
  listen clip inside `DialogTalking`.
- **`NPCTalking`** has Colonel Autumn's group and the mood groups `NeutralTalk`,
  `AngerTalk`, `DisgustTalk`, `FearTalk`, `SadTalk`, `HappyTalk`, `SurprisedTalk`
  (`GetDialogueEmotion` 0–6), each with 4–9 `ttnpc…` and `Talk…Casual…` leaves chosen by
  rising random thresholds (Neutral: < 11, 22, 33, 44, 55, 66, 77, 88, none), and two
  `IgnoreThis` nodes that can never pass (random < 0). **`NPCListening`** has the same
  seven moods with listen clips.
- **Furniture** (below `FurnitureIdles` > `Sitting`): `ChairDialogueIdles` (chairs, an
  NPC, `GetCurrentAIProcedure == 4`: eating or drinking talk, then `ChairTalk`
  (`IsTalking == 1`, the `SitChairTalkPlayerA`–`D` clips that Doc Mitchell's responses
  also name as speaker idles, §2) and `ChairListen`); `SittIngDialogIdlesStool` (stools);
  `WallLeanDialogIdles`; `FloorSitDialogueIdles`; `NVCrucifixDialogueIdles`. Each asks
  for the dialogue procedure.
- **Other folders**: `RobotDialogueIdles` (protectrons), `MRRobotsDialogueIdles`
  (Mister Gutsy), `RoboDialogueIdles` (robobrains) and `NVSDialogueIdles` (securitrons)
  want `GetCurrentAIPackage == 6` and then the dialogue menu; `MrHousedialogueIdles` the
  dialogue procedure; Festus' `TalkingIdles` `IsTalking == 1`.
- **The clips**: the people's talk and listen clips lie in
  `meshes\characters\_male\idleanims\`, are 3–10 s long (`ttnpcNTRLHandsDownTalkA` 3.03 s,
  `DialogNoBodyShiftTalkA` 3.33 s, `DialogArmsCrossedTalkC` and `SitChairTalkA` 6.73 s,
  `DialogNoBodyShiftListenA` 7.07 s, `ttpNTRLHandsDownListenA` 10.03 s), clamp at their
  end (cycle type 2), and have 58–59 tracks, all bound to the people's skeleton,
  `Bip01 Head`, `Bip01 Neck` and `Bip01 Neck1` among them: that is the head movement the
  owner missed. The talk clips play once (loop 0–0); most listen clips and the
  arms-crossed and weight-shift talk clips loop for ever (255–255).
- **`IsLastIdlePlayed`** (condition index 451, 86 uses on IDLE) has no page in the script
  catalogue; xEdit names it and gives it an IDLE parameter [documented: xEdit
  wbCTDAFunctions; animation.md §1.2]. The dialogue groups use it to keep a pose going.
- Loose head nods and shakes (`Talk_HeadNod_Neutral.kf` and others) are childless roots
  for scripts, not part of the walk.

### 12.2 Rules as built (`Game::talk_gesture`, `Game::talk_parts`, `engine/nvforever/src/idles.rs`)

- **Who** [design]: the NPC the player talks with in the dialogue screen (talking while
  it says a line, listening while the player chooses), and both sides of a conversation
  between actors (§5.1 of dialogue.md: the one saying the line talks, the one it is said
  to listens). Not the player; not lines said outside a conversation (SayTo, Say,
  hellos): the editor's documentation asks the idle manager "by the speaker and by the
  listener each time a new line of a conversation is said" [documented: §9], and the
  dialogue root wants the dialogue procedure or the dialogue menu anyway.
- **The walk** [hypothesis: the fidgets' walk, §11.2, the editor's picking of §9]: the
  roots of the actor's folder in chain order; the first node whose conditions pass is
  gone into, its children the same way, the first leaf that passes is the pick; a node
  whose children all fail is left for its next sibling. Conditions are judged with the
  one the actor talks with as their target (`NPCtoNPC` asks whether the target is the
  player). Whether the original walks on after a branch without a passing leaf is not
  known. Of all the picks only those under a **dialogue node** are played: a node that
  asks for the dialogue procedure, the dialogue menu or `IsTalking == 1` [design: the
  rest of the idle manager is not built; any other leaf the walk reaches (a chair's
  relax clip, say) is not played].
- **The tree decides the rest**: talking or listening (`IsTalking`), the mood
  (`GetDialogueEmotion` of the response being said; the listener's is Neutral
  [hypothesis: the function is about the actor's own line]), power armour, sitting or
  standing, and whether a pose goes on (`IsLastIdlePlayed`).
- **`IsLastIdlePlayed`** [hypothesis from the name]: 1 when the last idle the actor was
  reported playing (`Game::set_idle_playing`) is the one named. It is evaluated inside
  condition evaluation by its index, since it has no script opcode. With it the
  arms-crossed and weight-shift poses, once started, carry on for the rest of the
  conversation (`fnvtool actor gestures`, 500 picks: after the first arms-crossed pick
  every later talk pick is one of the arms-crossed loops and every listen pick the
  arms-crossed listen clip [observed in ours]).
- **Conversations between actors use the dialogue procedure** [hypothesis]: both sides
  of a conversation between actors now answer `GetCurrentAIProcedure` 4 and
  `GetCurrentAIPackage` 6 (as the actor in the dialogue screen already did). The tree's
  `NPCtoNPC` branch lies under a root that wants the dialogue procedure or the dialogue
  menu, so it could not be reached otherwise.
- **`GetRandomPercent`** in the walk draws from a generator of its own (seeded from the
  game's seed) [engineering choice: so that gestures do not move the scripts' random
  numbers]. Each condition is a new roll [documented: the function "returns a random
  integer" each time it is called; that conditions call it once each is ours]. The rising thresholds of the mood groups (11, 22, …, 88) would split the
  picks evenly if one roll served the whole list; with a roll per condition the later
  clips come up more often. Which the original does is not known [hypothesis].
- **Replay delay** [hypothesis: seconds, as for fidgets]: a leaf is skipped for its
  replay delay after the same actor last started it
  (`ArmsCrossed` 240, `DialogBodyShiftRightFemaleTalkA` 120).
- **When a gesture is picked** [design]: when the actor starts a response (a new line,
  or the next response of the same line), when it stops talking (the player chooses),
  and whenever its gesture ends while it is still in the same part, so a long response
  gets one clip after another (the talk clips are 3–7 s). A listen clip that loops for
  ever plays until the part changes. When the tree gives nothing the walk is tried
  again after 1 s.
- **What comes first** [design]: an idle the response names for its speaker or listener
  (INFO `SNAM`, `LNAM`), or any other idle asked for (a script's `PlayIdle`), replaces
  the gesture as idles replace each other (cross-faded), and no gesture is picked while
  it plays; when it ends the next gesture is picked. The gestures wait while the actor
  walks, turns on the spot to face the other faster than 15°/s, the rate at which
  the locomotion plays its turn clips (they step the feet), or gets onto or off
  furniture (`GetSitting` / `GetSleeping` state other than 3), and end when it walks,
  the conversation ends, or its furniture posture changes (getting off a seat or
  sitting down) [design, #3407]. A seated response idle (`Speaker` / `Listener`)
  likewise fades out when its actor starts getting off furniture so its full-body seated
  clip (e.g. `SitChairTalkPlayerA`, priority 50 on pelvis and legs) does not outlive the
  getting-off clip and snap the standing actor back into a seated pose [design, #3407].
  (The AI's walker reports a turn rate from frame to frame; a test against exactly 0
  kept Chet waiting a whole conversation.)
- **Furniture** [from the data]: a seated actor goes into `FurnitureIdles` first, so it
  gets the chair's talk and listen clips, laid over its sitting clip as the speaker
  idles already are (§7). An actor whose furniture branch gives a leaf outside the
  dialogue groups gets no gesture.
- **Lip sync and head tracking** [design]: lip sync moves the face's vertices, not
  bones, so it plays on top of whatever the head does. Head tracking (dialogue-play.md
  §8.1) turns the head bone after the frame's animation and aims only the clip's slow
  average head direction at the player, so the clip's nods and glances go on
  underneath the turn (until 2026-10-27 the turn cancelled them).
- Log: one line per gesture picked, `talk gesture: <actor> talks|listens (<the other>,
  line <INFO> response <n>): <idle> by <root> > … > <idle>`; the idle itself is logged at
  debug level (`idle: … plays …`). When a new part starts and the gesture must wait:
  `talk gesture: <actor> waits: another idle plays | it uses furniture | it walks | it turns on the spot`.
  At exit: `talk gesture summary: N gestures picked
  while talking, M while listening; K walks of the tree gave none`. Gestures are not
  counted in the `idle summary` line.
- Checks: `fnvtool actor gestures` (headless: the actor put in a conversation with the
  player, the tree asked many times while it talks and while it listens; `--line` sets
  the mood from a line); smoke tests `talk-gestures-menu`, `talk-gestures-npcs` and
  `talk-gestures-tree` (tools/smoke/scenarios.toml).
- Seen in NVForever, 2026-10-16 [observed in ours, from the logs]: Chet (GSChetRef) in
  the dialogue screen got a talk clip at each of his greeting's three responses and
  another when one ended mid-response (`TalkToPlayerLHCasualG090`,
  `DialogNoBodyShiftTalkSubtle` twice, then `DialogArmsCrossedTalkA` by `ArmsCrossed`),
  then `DialogArmsCrossedListenA` while the player chose (by `ArmsCrossedLoop`, the pose
  kept), and an arms-crossed talk loop for his goodbye. Joe Cobb's angry line to Trudy
  gave `ttnpcANGRHandsFreeTalkB` (`NPCtoNPC` > `NPCTalking` > `AngerTalk`) and Trudy's
  answer had him listen (`NTRLHandsDownListenB`); Trudy herself played her lines' own
  idles (`LooseStandingArmsFolded` as listener, `LoosettnpcANGRHandsFreeTalkA` as
  speaker) and no gesture. Joe Cobb's first gesture came 7 s into his line, after the
  AI had turned him to Trudy. Not looked at in a picture.

### 12.3 Open

- How often the original asks the tree during a long response (here: whenever a
  gesture ends), and whether it cuts a talk clip when the next response starts (here:
  cross-faded at once).
- One roll of `GetRandomPercent` per walk or per condition (above).
- What the listener's `GetDialogueEmotion` is (here Neutral).
- The robots' and Mr. House's groups want `GetCurrentAIPackage == 6`; ours answers 6 in
  conversations, but the robots' skeletons were not tried.
- The `.lip` head and look channels (lipsync.md §3.3) are not played.

## 13. Fleeing: the run and the cower (owner request 2026-10-17)

The owner asked for fleeing NPCs to run with the original's flee animations ("hands
covering the head") and to hide crouched when they stop.

### 13.1 What the data holds

- **No flee run set** [observed: `fnvtool bsa list` of `Fallout - Meshes.bsa`, 2026-10-17].
  `characters\_male\locomotion\` has the stance sets, `male\`, `female\`, `child\` and the
  limping `hurt\` set, and nothing for fleeing. No path in the base archive, `Update.bsa`,
  `Fallout - Misc.bsa` or the five add-on `Main` archives contains `flee`, `panic` or
  `afraid` as a clip of people. The only `scared` clips are the add-ons' robot idles
  (`nvdlc03\creatures\brainbot`, `nvdlc04\…\nvdlc04edeclone`). The IDLE tree has no idle
  for a moving fleeing actor (below). A fleeing person therefore runs with its ordinary
  run (`mtfastforward` of its stance) [hypothesis: nothing else is in the data; no game
  capture was taken].
- **The cower clip**: `characters\_male\idleanims\3rdp_cowering.kf` [observed]. Other
  frightened clips in the same folder are played by scripts and dialogue only, never by
  the tree's flee conditions: `handsupnoharm.kf` (`LooseHandsUpNoHarm`, no conditions),
  `ms11shieldseyes.kf` (`MS11ShieldsEyes`, no conditions), `scared*`, `ttpfear*`,
  `dlcpittflinchinga.kf`, `dlcpittsitfloorcoweringa.kf` [observed: `fnvtool actor
  anims`, FalloutNV.esm].
- **IDLE records naming the flee procedure** [observed: `fnvtool actor anims`,
  FalloutNV.esm]. The values are GetCurrentAIProcedure codes: 16 FLEE, 34
  FLEE_NON_COMBAT [documented: GECK wiki GetCurrentAIProcedure, owner's export].

| IDLE | Where | File | Conditions |
|---|---|---|---|
| `NVKimbalCower` 0x0016AC8F | root | none (folder) | GetIsReference (one ref), a quest variable == 3, IsMoving == 0, procedure == 16 OR == 34, IsSwimming == 0 |
| `CowerIdles` 0x0006CB4A | root, after `NVKimbalCower` in the chain | `3rdP_Cowering.kf`, Special Idle, loops 2–12 | IsMoving == 0, procedure == 16 OR == 34, IsSwimming == 0 |
| `3rdPCrouchedHit` 0x00020EEE | `HitReactionIdles` > `3rdPersonHitReactions` | (inner node) | IsSneaking == 1 OR procedure == 16 OR == 34, IsMoving == 0 |
| `3rdPCrouchedHitForce` 0x0014CC2A | `HitReactionIdles` > `3rdPersonHitReactionsForce` | (inner node) | as above, and GetForceHitReaction == 1 |
| `LooseCowerIdles` 0x000C0A9A | root | `3rdP_Cowering.kf`, Loop Forever | none (asked for by scripts) |
| `VanGraffCustomerCower` 0x001251F7 | root | `3rdp_cowering.kf` | GetIsID (one actor) |

The creatures' combat idles (mole rat, giant rat, mantis, gecko) want procedure 13 OR
16 while not moving. Their taunts want "not 16", and the Cazador's `NVCazFlyHigh` wants
16. So the combat AI's flight runs as procedure 16 [hypothesis, from these conditions].

### 13.2 Rules as built (`Game::flee_idle`, `engine/nvforever/src/idles.rs`)

- `GetCurrentAIProcedure` reads 16 for an actor in a combat state with a flight
  (`combat_group::Flight`), and `GetCurrentAIPackage` reads 5 COMBAT (ai_state.rs).
- A drawn NPC that flees and stands still (as when its flight begins, or at its flee
  spot) is checked every 0.5 s. The tree's roots
  that are idles with a file of its folder and whose conditions name procedure 16 are
  tried in chain order with their own conditions, and the first that passes plays
  (`IdleSource::Cower`). In FalloutNV.esm that is `CowerIdles`. It is not stopped
  when the actor runs, only when it no longer flees (fading out). After its loops it
  is picked again the next time the actor stands still.
- **Cowering while running, crouching when stopped** [owner 2026-10-17: "they'll cower
  whilst running but they only crouch when stopped"; "they holster their weapon"]. The
  priorities of the clips give this without a run clip of its own [observed: `fnvtool
  nif decode`]. `3rdp_cowering.kf` has priority 23 on `Bip01`, the pelvis and legs, 30
  on `Bip01 Spine` and 33 on the spine above, neck, head and arms.
  `locomotion\male\mtfastforward.kf` (the unarmed run) has 30 on every bone, so the run's
  legs win and the cower's arms and head win: it runs with its hands over its head.
  `sneakmtidle.kf` has 20 on every bone, so stopped and crouched (§12.2.1 of
  combat-ai.md) the whole cower shows. A drawn rifle's run (`2hrfastforward.kf`) has 36
  on the upper body and would cover the cower: the weapon is holstered while the actor
  flees (`Game::wants_weapon_out`). How the original keeps the idle on the run is a
  hypothesis: it starts by its conditions when the flight begins (the actor still
  stands) and loops 2 to 12 times of 12 s, and movement does not end an idle.
- Hit reactions are not read from the tree (the staggers of `locomotion.rs` and
  `npc_combat` follow their own rules), so the `3rdPCrouchedHit` branch is not played
  while cowering.
- Not built: Flee Not Combat packages (procedure 34) are not carried out
  (`fnv_ai::goal`), so their actors never cower.

### 13.3 Open

- Whether the original ends the cower before its loops run out, and whether it starts
  it again on the run. A capture of `GetCurrentAIProcedure`, `IsMoving`, `IsSneaking`,
  `IsIdlePlaying` and `IsWeaponOut` on a fleeing NPC would settle it
  (`tools/capture/observe.py`); none was taken.

## 14. Creatures' combat idles: alerted and standing (owner's playtest 2026-10-24)

The owner saw young geckos give no sign before they attacked. The gecko's idle tree has a
combat branch that the engine never played.

### 14.1 What the data holds [observed: FalloutNV.esm, `fnvtool esm decode`]

- `NVGeckoCombatIdles` 0x0011D9E9, a root of the `creatures\NVGecko\IdleAnims` branches,
  wants `IsMoving != 1` AND (`GetIsAlerted == 1` OR `GetCurrentAIProcedure == 13` OR
  `== 16`), i.e. standing while alerted, fighting or fleeing. Its children:
  `NVgeckoHiss` (CombatIdle_Hiss.kf, GetRandomPercent < 50), `NVgeckoHopLeft`
  (CombatIdle_HopLeft.kf, < 50) and `NVgeckoHopRight` (CombatIdle_HopRight.kf, none).
- The gecko roots in chain order: `NVGeckoHitReactions`, `NVGeckoGetupAnims`,
  `NVGeckoCombatIdles`, `NVGeckoGeneralIdles` (IsInCombat == 0: scratching, eye licks).
  The combat branch comes before the general one.
- Other creatures' warning idles want `GetIsAlerted == 1` alone: `MWarning` 0x00072DC2
  (mirelurk SpecialIdle_Warning.kf), `StBWarning` 0x00099837 (sentry bot). The giant
  rat's `NVgrGrowl` and the mole rat's growl sit under their own combat nodes.

### 14.2 Rules as built (`FidgetKind::Alerted`, `engine/nvforever/src/idles.rs`)

- A creature (a skeleton folder with a `creatures` part, DLC folders included) standing
  still with its stance drawn (`Game::draws_weapon`: a combat state, SetAlert, or a
  warning to the player, `Game::warning_to`) is checked every 2 s, as the drawn-weapon
  fidgets are (§11), for the first leaf of its tree below a node naming
  `GetIsAlerted == 1`, alone or joined by OR [hypothesis: the folder match and walk of
  §11]. The tree's own conditions decide: a warning gecko reads GetIsAlerted 0 and
  procedure 44, so it does not hiss until it fights (procedure 13) [observed in the
  original, §14.3]. A fight does not set GetIsAlerted (#2121; GetIsAlerted read 0 in the
  original's fights, npc-behaviour-compare.md §11): the gecko's combat idles pass on
  procedure 13, which Lost reads too, and the warnings gated on `GetIsAlerted == 1` alone
  (`MWarning`, `StBWarning`) play only for an actor SetAlert alerted [design: the tree's
  own conditions, read as scripts read them].
- It ends when the creature moves or is calm again [design]; an attack stops it as it
  stops any idle (`attack_anims.rs`), except that an idle flagged No Attacking (the
  hiss, the rat's growl) holds the attack until it has played (§9). Log: `npc combat:
  … holds its attacks while its idle … plays (No attacking)`. People keep their drawn-weapon fidgets: their tree
  is not walked for this [design].
- Log: `idle: … plays … (CombatIdle)`; `creature combat idle summary` at exit. Smoke
  `aggro-warning-look`.

### 14.3 The original's warning [observed: capture `warnings`, session young_gecko, save nvfstrip, 2026-10-24, plugin version 7]

The player walked up to Young Gecko VERGSGecko6REF 0x00157B3C (aggro radius 500,
sandboxing) from 900 units, sampled every 5 game loops:
- Outside 500: it wandered (procedure 7), detecting the player from about 900.
- At 467 (the first sample inside 500): it stopped (IsMoving 0) and faced the player
  (within 1 degree, from 9.5 off the sample before); its package read a form made at
  run time (FF001319), GetCurrentAIPackage 35 (GUARD), GetCurrentAIProcedure 44 (GUARD);
  GetAnimAction 0 ("equip weapon"), IsWeaponOut 0, GetIsAlerted 0.
- 21 game seconds later (0.7 s at TimeScale 30), the player at 246: combat (procedure
  13, package type 5); IsWeaponOut 1 a sample later; GetIsAlerted stayed 0 in the fight.
- So a warning creature draws but is not alerted, and the combat idles that want the
  alerted state or the combat procedure come with the fight. IsIdlePlaying and the
  warning count need plugin version 9 (not installed); not read.

Sessions gecko_stay_400 and gecko_stay_150 (run by the observer session, same save and
plugin; the player put at a distance and left there):
- Goodsprings Young Gecko Hunter 0x00165186 (radius 650) stopped at 607 units, faced
  the player within 1 degree, package a run-time form (type 35, procedure 44), and drew
  and put away its weapon in turn: IsWeaponOut 1 every 0.0167 game hours (60 game
  seconds, 2 s at TimeScale 30), GetAnimAction cycling 0 / 1 / -1, GetIsAlerted 0, never
  moving. The owner, watching, saw it hiss: the gecko's draw clip is the hiss. It
  attacked 451 game seconds (15 s) after it stopped, the player still at 606. Its
  companion 0x00165185 warned from 618 without the cycle and attacked after 500 game
  seconds (16.7 s).
- A Young Gecko (radius 500) detecting the player first at 196 units attacked in the
  same sample, with no warning.
- Built: the 15 s and the half-radius attack (`fnv_game::aggro`, "The timing"). Not the
  cycle: the owner judged it an unintended quirk of the original (2026-10-25); a warning
  creature draws its stance once and holds it (`creature_stance.rs`).

Gomorrah sessions (same day): the cash-room guard 0x00127950, aggro radius 128 of his own,
warned a player 66-85 units from him the same way: a run-time Guard package (type 35,
procedure 44), faced the player, drew and holstered his gun every 2.5 s (IsWeaponOut,
GetAnimAction 0 / 1), GetIsAlerted 0, and fought 21 s later. A player 129-135 from him
(24-33 from the door his Guard package guards) got a brief run-time Follow package and a
turn of his head, no warning. For people too the owner chose draw and hold over the cycle
(2026-10-25, `alert.rs`); they say their GuardTrespass line at each warning. The bouncer sessions saw nothing (no detection; the
player was moved away by something in Gomorrah).

Campaign `warnings2` (2026-10-25, run by the observer session, plugin version 7, 23
sessions; the frame rate was lower, the clock normal):
- Attack delay: creatures 15 s after the warning began (the young gecko at 387-393 of
  500, the coyote VERGSCoyote6REF at 727 of 1024, the gecko hunters), the cash-room guard
  21-22 s (at 70-90 of 128, four runs). The setgs runs (fAITrespassWarningTimer 2 / 10,
  iGuardWarnings 1 / 4, TimeScale 60) showed no difference: their start distances
  varied between 90 and 98 and that, not the settings, decided whether he attacked.
- Inside half the radius: the young gecko at 217 of 500 fought on detection; the guard at
  32 and 62 of 128 fought after one draw (about 3 s). Open: the guard at 98-121 of 128
  warned for 68 s and never attacked.
- The draw cycle, sampled every game loop: the guard's weapon out 1.33 s, holstered
  1.17 s (2.5 s); the young gecko out 0.81 s, holstered 1.20 s (2.0 s), GetAnimAction 0
  for 1.08 s and 1 for 0.88 s. Not built (owner: draw and hold).
- Gomorrah's room guards (vGomorrahGuardPackage, no aggro radius) did not warn a player
  15 units from the door each guards, nor 60 units from them (a run-time Follow
  package, procedure 11, instead). Guard packages' own warnings were taken out.
- Coyote at 300-560 of 1024: it took up the warning's Guard package within 0.6 s, then
  lost sight of the player and stood without warning or fighting for 28 s.

## 15. Idles that hold their actor in place (issue #1072, 2026-10-31)

**The case** [observed in the data, `fnvtool esm decode`, FalloutNV.esm]: Oliver Swanick
(NPC_ `vNiptonSurvivor` 0x00134985, placed as `vNiptonSurvivorREF` 0x001349B6) is given
`vNiptonSurvivorCelebrate01` (0x0016B11A, Patrol, Always Run) by
`VNiptonSceneControlScript` when the player comes within 3,700 units of him. Its patrol
starts at `VNiptonOliverCelebrate01MarkerREF` (0x0016B098), linked to 02 and 03; each
marker has a patrol idle time (XPRD) of 1 s and a patrol script: 01 plays
`LooseCheeringBothArmsUp` (IDLE 0x00033CDC, `CheeringBothArmsUp.kf`, 4.67 s, loops 1-2,
section byte 0x47 Special Idle), 02 `LooseMiddleFinger`, 03 `LooseCheeringOneArmUp`
(`CheeringOneArmUp.kf`, 4.67 s). So the patrol wants him on his way after 1 s while the
cheer has 4.7 to 9.3 s to run. (Before 2026-10-31 he walked off with it playing.)

**Why he slid** [observed in the code]: the idle is laid over the locomotion by track
priority (§7). `CheeringBothArmsUp.kf` keys every bone, `Bip01`, the pelvis and both
legs included, at priority 50 [observed: `fnvtool nif decode`]; the people's walk and
run clips (`mtforward.kf`, `mtfastforward.kf`, `h2hfastright.kf`, `2hrfastright.kf`)
key their legs at 30. Walking on, his legs showed the cheer while the body moved.

**The rule** [design; the original's rule is not documented: observer check 57]: an
idle *poses the legs* when it

- was asked for by a script (`PlayIdle`, `PickIdle`), a package's begin, end or change,
  a dialogue line, or a pick-up;
- is not of the Upper Body section (IDLE DATA byte 0, low six bits, 21; `IdleDef::section`);
- has a leg track (a bone named `...Thigh`, `...Calf`, `...Foot` or `...Leg...`) of a
  higher priority than the legs of the actor's walk clip (its stance's Forward, else
  FastForward). An idle whose legs lose to the walk does not: the cower's (23) lose to
  the run's (30), and a fleeing actor runs with its arms over its head (§13).

**Standing, and breaking out** [design: owner 2026-10-31, "for the idles we probably
need a way for npcs to break out of the idle then when moving instead of locking in
place"]: while such an idle shows (playing, or fading out) the actor's walk waits as for
a stagger (`residents.rs`, the held set): its character stops, it neither moves nor
turns, so the legs never slide. The legs stay still only while the AI has nowhere to go.
As soon as it has somewhere to go (nothing else holds it and its AI travels: its package
walks on, a patrol wait ends, it follows or flees, a conversation that moves), it breaks
out of the idle: the idle fades out at once over its blend-out (a looping one is cut the
same way), and the actor walks once the idle has gone (`IdleHolds::breaking`). So
Oliver Swanick cheers for his marker's 1 s of patrol idle time, then breaks out and runs
to the next marker; no idle can hold an actor for ever. Collection idles, fidgets,
gestures, combat idles, cowering and stimpaks are not held: they end (or play on) when
their actor moves, as before.

**What else ends such an idle** [design]: combat (the actor alert, fighting or
searching: it breaks out at once); `StopIdle` / `ForceStopIdle`; another idle asked for
(it replaces it); a stagger (the hit reaction plays over it and holds the actor anyway).
A conversation that does not move holds the actor anyway, and its dialogue idles play.

**Log**: `idle: <name> plays <idle> (...) (<source>), its legs held`, then `idle hold:
<name> (<ref>) stands while its idle shows`, `idle hold: <name> (<ref>) breaks out of its
idle to walk` (or `for combat`) and `idle hold: <name> (<ref>) may walk again: its idle
has gone`. Smoke `idle-hold-oliver`.

## 16. Activating: the drinking idles (issue #2349, 2026-11-14)

A player noted that drinking from a sink, toilet, pump or fountain plays no animation;
the original plays one. The tree holds it under a root of its own that nothing asked.

### 16.1 What the data holds [documented: `fnvtool esm decode`, `fnvtool actor anims`, FalloutNV.esm]

- `ActivateIdles` 0x00003BDC: a people's root (folder `Characters\_Male\IdleAnims`;
  chain order §12.1), with one condition, GetUsedItemActivate (index 259) == 1. Its
  children in chain order:
  - `ActStimpakSource` 0x00023035 (GetIsUsedItem Stimpak == 1): the NPCs' stimpak idles
    (`fnv_game::npc_stimpaks`, which asks its children directly).
  - **`ActWaterSource`** 0x000221B4 (IsWaterObject (304) == 1), no file. Below it:
    `1stPersonActWater` 0x00026EB4 (GetIsID 7 == 1 and IsPC1stPerson == 1;
    `1stPMT_ActWaterSource.kf`, one sequence of 2.37 s, 14 tracks, `Camera1st` among them,
    so it plays on the first-person rig, §4) then `3rdPersonActWater` 0x00026EB5 (no
    conditions; `ActWaterSource.kf`, 2.70 s, 50 tracks). Both play once (loop 0-0),
    Special Idle section, no replay delay.
  - `NPCPickUpItem` 0x00069B6E (GetIsID 7 != 1 and GetIsUsedItemType 27 == 0): the
    pick-up leaves, never the player's.
- Two more nodes name GetUsedItemActivate: `SitActivateIdles` (under the seated NPCs'
  chair branch: `Chair_UseComputer.kf` for a terminal) and the spinebreakers'
  `SBGatherAnims` root (`Creatures\SMSpineBreaker\IdleAnims`).
- "Returns 1 if the object currently being activated is Placeable Water. Used in vanilla
  game for drinking animation" [documented: GECK wiki IsWaterObject]. GetUsedItemActivate
  "is only used in the default game as a conditional for Idle Animations, to check
  whether an actor is activating something" [documented: GECK wiki GetUsedItemActivate,
  Notes; the page is marked incomplete].
- `Camera1st` hangs under `Bip01 Looking` in `meshes\characters\_1stperson\skeleton.nif`
  [documented: `fnvtool nif decode`], so the rig's pitch turn (§4, `player_body.rs`)
  applies on top of what the clip does with the camera node.

### 16.2 Rules as built (`Game::activate_idle`, `engine/nvforever/src/idles.rs`)

- **GetUsedItemActivate** answers 1 while the actor uses something (furniture, a
  package's item), as before. Only inside the walk for the player's activation idle
  (`Game::activate_idle`, which sets `Game::activate_walk`) does it also answer 1 while
  that actor activates something: the reference it activated within
  `ACTIVATING_SECONDS` (2 s), the window IsWaterObject reads [documented: the wiki note
  above; design, #2349: the window, and the scope]. Everywhere else (scripts, quests,
  dialogue, packages, the NPCs' fidget and `PickIdle` walks) its answer is what it was:
  an NPC that has just activated something answers 0 and picks as before (test
  `activating_placeable_water_picks_the_drinking_idle`).
- **Who asks** [design, #2349]: the player's activation that reaches its default action
  (`Game::run_default_activation`: by hand, or a script's `Activate` with the player as
  the action reference) queues the player in `Game::activate_idles`. The animating
  layer, which knows the player's folder, walks the tree (`Game::activate_idle`): the
  roots of that folder whose conditions name GetUsedItemActivate == 1, in chain order,
  gone into as for `PickIdle` (the first leaf with a file whose way down passes)
  [hypothesis: the original's idle manager asks this root when the player activates].
  The tree's own conditions choose; no other activation idle is built.
- **Playing**: asked as `IdleSource::Activate` and played like any idle asked for:
  `1stPersonActWater` in first person on the first-person rig, `3rdPersonActWater` in
  third person on the body.
- **Nothing held** [design, #2349; observer check 131]: unlike a scripted idle that
  drives the camera node (§4), an activation's idle does not hold the player
  (`PlayerIdles::held` stays empty): mouse look, movement and the HUD stay; the camera
  node moves the view from where the player looks.
- **The effect** [design, #2349; observer check 131]: the water's actor effect still
  applies at the activation (activators.md §9), not at the idle's start or end.
- **Log**: `activate idle: the player activating 0x…: 1stPersonActWater by
  ActivateIdles > ActWaterSource > 1stPersonActWater`, then `idle: the player plays
  1stPersonActWater (…) on the first-person rig (Activate)`. Smoke `water-drink-idle`.

### 16.3 Open

- Observer check 131: the idle's length in the original, whether movement and looking
  are held while it plays, and when the healing and the rads apply.
- NPCs drinking (no package makes them drink from placeable water here).
