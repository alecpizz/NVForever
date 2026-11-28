# The classic VATS style

Status: built on branch `vats` (2026-11-09 to 2026-11-11), played through by the fork
owner; §9b lists what is planned. The other style, "Dead Eye"
(docs/design/vats-mode.md), shares what §6 and §13 name and is one row away on the
settings page ("V.A.T.S. style"). This one is what V starts: the owner ruled on
2026-11-24 (owner-review item 31a) that Classic is the default.

Tags as everywhere: [documented] cites a public page, [observed] the game's data files,
[hypothesis] an inference, [design] NVForever's own choice. Combat is not a parity
target the project measures, so where the public pages give no number the choice is
[design] and says so.

Sources read 2026-11-09 (public pages only; no executable was read):

- fallout.wiki, "Fallout: New Vegas V.A.T.S.", "Fallout 3 V.A.T.S." (the game settings
  table; Fallout 3's defaults), "Action Points (Fallout: New Vegas)", "Special Attack
  (Fallout: New Vegas)", "Fallout: New Vegas perks", "Fallout: New Vegas traits".
- fallout.fandom.com "VATS" and wikihow.com "Use V.A.T.S. in Fallout 3 and New Vegas"
  refused automated reading; nothing here rests on them.

## 1. The owner's decisions (fork owner, 2026-11-09)

- The original's flow replaces Dead Eye as what V starts; Dead Eye stays as a style the
  settings page offers. Upstream too since the owner's ruling of 2026-11-24 (item 31a);
  until then Dead Eye was the default there.
- A slider sets how far the world slows while targets are chosen: 50 % to 100 %, 100 %
  a full stop and the default, as the original.
- The slider acts on choosing targets only. The attacks then play as before.
- Slowed rather than stopped: chances are worked out every frame, an attack queued on a
  target that dies or leaves sight is dropped with its AP back, and the player is not
  protected while choosing.
- While the queued attacks play the player takes the original's share of a hit.
- Controls as the original's; where the pages do not give them, ours, rebindable.
- The rules (hit chance, AP, perks, helpers) are the ones VATS mode already has.
- V with nothing to target does nothing but play the refusal sound.

From the playtests of 2026-11-10 (each is cited where it is built): the enemy bar with
the damage in sight; the selected part lit as a HUD element, pulsing, in the HUD's
colour; [design, #2659] classic now places the bar at bottom centre and keeps it through
playback, with the corner HUD hidden; no darkening or colour filter;
the cameras' angles (§8); an attack on a dead target is not played and the mode ends
soon after (§8.1); explosives hold the mode until they have gone off (§8.2); slow motion
keeps the physics' pace (§8.3); the zoom's times (§5).

## 2. Time

- Choosing targets, the clock runs at `1 - slowdown` (`vats_classic::clock_scale`):
  0 at the default, at once, with no ease [documented: the game is paused while targets
  are chosen; design: the range, owner].
- At 0 the virtual clock is not paused, it runs at no speed, so nothing takes the
  picture and menus still open over it. The clocks that must not stop with the world
  count real time then (`pause::stands_still`): the test aids' clock and
  `--run-seconds`. The scripts' frame time stays the world's, none.
- The mode's own timing (the turn of the view, the scans) is in real seconds.
- Playing the attacks: `FIRE_SCALE`, as Dead Eye [design, until §8].
- Sounds follow the clock as they do for Dead Eye (`audio_out::slowed_sound`, floored).

## 3. Targets

Every living actor with a hit volume that is

- within the weapon's reach and `fVATSMaxEngageDistance` (5000) [documented:
  fallout.wiki "Fallout 3 V.A.T.S.", settings table]. A melee or unarmed weapon's reach
  is the swing's (`fCombatDistance` 107 x the weapon's Reach) times `fVATSMeleeReachMult`
  (2), at most `fVATSMeleeMaxDistance` (300), from the body [documented: GECK wiki "VATS
  Settings"; neither setting is in FalloutNV.esm, so these defaults apply, observed; that
  New Vegas keeps the page's Fallout 3 defaults is a hypothesis]: the Fists 214, a
  machete 107, a lead pipe 268, plus the player's capsule radius from the eye, to the
  target's body (its radius off the distance to its middle), as the swing itself reaches
  (`fnv_combat::damage::vats_melee_reach`, issue #2656). Until then it was Dead Eye's flat
  350 (`MELEE_MARK_REACH`), which Dead Eye keeps [design: the maintainer's choice for that
  style];
- within `CLASSIC_FRONT` (55 degrees) of the line the player looked along on entering
  [design: about what the view shows];
- in sight from the eye: its aim point or the middle of any of its body part capsules,
  the weapon in its hand among them (the world in the way hides a point, another actor
  does not) [design: in a fight the enemies stand behind cover, and one whose chest is
  hidden while its head and gun show is a target; a tester's report of 2026-11-10, V
  did nothing in a fight, which was not reproduced];
- not barred as for a Dead Eye mark: dead, cloaked without Enhanced Sensors, No VATS
  Melee with a melee weapon, SetVATSTargetable 0;
- not one VATS never targets (`Game::vats_untargetable`): on the game's
  `BannedVATSTargets` list, a ghost, or a flying creature that does not fight (a raven)
  [design: the fork owner 2026-11-10].

Hostile or not [documented: any actor can be targeted]. They are ordered left to right
across the entering view; the one nearest the crosshair is selected first [documented
for the first; design for the order]. With none, V is refused (`UIVATSEnterFail`), and
the log says how many actors each rule passed over (`vats_classic::Missed`), so a
report can be read from the run's log.

The list is made again every 0.25 real seconds. A queued attack whose target is no
longer in the list is dropped (its AP was only reserved). When the selected target is
gone the selection moves to the one nearest the entering line; with none left the mode
ends.

## 4. Choosing and queueing

- Slide Left / Slide Right: the target before or after, round the ends.
- Forward / Back: the part before or after on the selected target.
- Attack (Use): queue an attack on the selected part. Each reserves the weapon's AP and
  rounds exactly as a Dead Eye mark (`MarkQueue`).
- Block: take the last queued attack back; with none queued, leave.
- Activate: play the queue. V: leave.
- Ready / Reload: queue the weapon's special attack on the selected part (§9).
- Any other action leaves, as Dead Eye (§11.8 there).

[documented: the game's menu file gives E for Accept, the right mouse button for Return
and R for the special attack (spec-notes/vats-menu.md §3); design: the controls those
keys are by default, and the target and part keys, which the file does not name.]

A new target starts on the part with the best chance [design]. Each part's label shows
its name and chance, the selected one bracketed, with "xN" when N attacks are queued on
it; with Concentrated Fire the label shows the chance the next attack on it would have.
Under each label a bar shows the part's condition (its condition actor value of 100; a
part without one has no bar). [design, #2659] At bottom centre the target's name stands
under the HUD's health bar [documented: the original shows each part's chance and condition;
design: this layout, the engine-drawn screen that stays as the fallback, §11].

## 5. The player and the view

While targets are chosen and while the attacks play the player does not move and the
mouse does not turn the view [documented: the player cannot move or dodge]. The view
turns onto the selected target in real time (`TURN_RATE`). Left without firing, the view
turns back to where it was on entering at the same rate, and the mouse has it again once
it is there; after firing it stays on the last target. It is not turned back behind a
loading screen or once the player is elsewhere (a load, a door: `restore_stands`).

The field of view narrows to frame the selected target whole, at
`dialogue_zoom::VATS_TARGET_SHARE` (0.6) of the view's height [design: no page gives a
number]. It is the conversation zoom's machinery (`dialogue_zoom::track`): the same
ownership of the field of view, the same depth of field, the third-person camera
travelling to the eye. Its ease runs in real time, since the world's clock may stand
still: 0.5 s in (`VATS_IN_SECONDS`) and 0.5 s out (`VATS_OUT_SECONDS`) when the attacks
start to play or the mode ends [design: the fork owner 2026-11-10, first "slow down the
VATS zoom in/out animation by 50%", from 0.25 and 0.3 s, then "the same length 0.5sec";
the view's turn was halved with it, `TURN_RATE` 7].

## 6. What it shares with Dead Eye

Entering (weapon, AP and perk checks), the queue, each part's chance, the firing
sequence (shots, bursts, throws, melee runs), rolls and misses, the helpers, Grim
Reaper's Sprint, the kill cameras, the sounds, what the scripts see (GetVATSMode,
GetVATSValue). One set of rules for both styles.

## 7. Damage while the attacks play

A hit on the player is multiplied by `fVATSPlayerDamageMult` while the classic style's
attacks play: 0.75 without the setting [documented: fallout.wiki "Fallout: New Vegas
V.A.T.S.", 75 % in New Vegas, 10 % in Fallout 3]. Not while targets are chosen
[design: owner]. Dead Eye does not apply it, as before.

## 8. The cameras while the attacks play

Each queued attack plays the game's own camera shots (CAMS), chosen by its camera paths
(CPTH) exactly as a kill camera's are (`Game::pick_camera_path`, vats-mode.md §19): the
conditions run on the player with GetVATSValue answering about that attack.

- As the attack leaves: the path's shots whose Camera Action is Shoot [documented:
  GECK wiki CameraShot, "Camera Action": when the command starts]. The attack is let
  go 0.25 s into its camera (`SHOOT_LEAD`) and is seen in it: the camera stays until
  the attack is out whole (a burst's rounds, a throw's wind-up) and 0.45 s more
  [observed: the fork owner's playtest 2026-11-10; let go only after its camera, the
  camera showed the player standing]. A path with no Shoot shot gets NVForever's own
  clocks (`kill_cam::own_shot`), so an attack is always shown.
- After it: the shots whose action is Hit, held 2 s longer (`CLASSIC_HOLD`). A burst's
  later rounds go on meanwhile; the next attack waits. The last attack asks for none:
  if it kills, the kill camera plays (its setting applies), so one hit is never shown
  twice.
- A shot plays for its Min Time, else its Max Time, within 0.5 s and 1.2 s (Shoot) or
  1.8 s (Hit) [design: the original cuts on the attack's own events; these bounds keep a
  queue moving]. The world's clock runs at the shot's Global time multiplier; the
  player, its weapon's cycle and the queue go at the shot's Player time multiplier
  times that, held to 1.1 of real time [documented: GECK wiki CameraShot, "Player
  Multiplier: how fast the player moves in comparison to the modified global time"].
- **Where an attack's camera stands**: one of three angles, not the shot's model: both
  in the frame (behind the attacker's shoulder), the target alone, first person
  (`kill_cam::Angle`). Each stay starts on the next angle; within a stay the angle
  changes only after 6 s of camera, at an attack's start, and holds the view between
  two attacks' cameras [design: the fork owner 2026-11-10, four angles "not too fast";
  the fourth, the player alone, was removed the same day so that a kill is always in
  the frame]. The camera stands on the side of the line between the two that has more
  room, in front of walls; its rays do not take actors for walls. An attack's shot's
  image space modifier is not played (it would start again with every attack).
- The outcome is not known before the attack lands, so the paths are asked with a hit
  that neither kills nor cripples [design]. The Hit stage knows the roll.
- Fly shots are not played. Melee queues play without attack cameras.

The kill camera keeps the masters' shots and their models (vats-mode.md §19); the held
controls and the hidden HUD are its own (`kill_cam.rs`). Its last shot is held 3 s
longer in all [design: the fork owner 2026-11-10].

### 8.1 A target that dies

An attack still to play whose target has died is dropped (its AP was only reserved),
and a burst under way on it is cut short; attacks queued on other targets play on. With
none left to play the mode ends 2 real seconds later (`AFTER_KILL`), the cameras going
on meanwhile [design: the fork owner 2026-11-10, "when the target dies, vats should
automatically end ... currently the player character keeps shooting the dead body"].
Dead Eye does the same. A kill by the last queued attack is the kill camera's.

### 8.2 Explosives

The mode stays until an explosive the queue sent has gone off: 2 s after a thrown one,
1 s after a launched one, and no longer than 12 s after the last one left or the last
one went off (not after a long queue's first); one that flies off from its mark
(384 units past its nearest point) no longer holds it [design: the fork owner
2026-11-10] (`VatsMode::blast_holds`, `combat::VatsBlast`). A thrown weapon's clip
starts at the pull and the throw leaves at its `Hit` key, once.

### 8.3 Slow motion and the physics

The physics takes 60 steps a second of the game's clock. With the clock slowed a step
came every few frames, and a falling corpse, its severed parts and a head's pieces moved
in jerks. The step now follows the clock down (`phys::pace_steps`, to a fiftieth of
full speed), so there are as many steps a real second as at full speed, whatever slows
the clock (either style, a kill camera, the weapon wheel, a script's time multiplier)
[design: the fork owner 2026-11-10]. A corpse's bones are drawn between its last two
steps, as characters are (`ragdoll.rs`, `Corpse::drawn`).

[design, #2659] From Accept until playback ends, classic keeps the target name and
health bar at bottom centre, above the compass gap, including during attack cameras.
The other meters, compass, crosshair and prompt, corner messages, quest notices and
queue marks stay hidden (`VatsMode::playing_back`, `hud::HudView::enemy_shown`).
Dead Eye retains its previous playback HUD rule. The wider playback comparison is
still observer check 215, item 8.

## 9. Special attacks

A melee weapon's special attack is its record's own (WEAP `VATS`: the effect it puts on
the target, the weapon skill it needs, the damage multiplier, its AP) [documented: xEdit
wbDefinitionsFNV.pas WEAP; fallout.wiki "Special Attack (Fallout: New Vegas)"; observed:
FalloutNV.esm, the baseball bat skill 50, x2.0, 35 AP beside its swing's 25; the machete
x0.7, 16 beside 20; the sledgehammer x0.5, 48 beside 38 with an effect; a weapon without
one has zeros]. So the AP is the special attack's whole cost, not an addition.

- Offered in the classic style, for a melee weapon, when the player's skill with the
  weapon is at least the one it needs.
- Ready / Reload (R) queues the special attack on the selected part, as Attack queues
  the weapon's own [documented: vats_menu.xml binds R to its special attack button;
  design: the press queues at once]. While targets are chosen that control does not
  leave the mode. The button reads "special attack: damage xN, M AP" [design: our own
  words; the moves' names are not in the data files]; on the engine-drawn screen the
  header says the same after the target's name.
- A queued special attack reserves and spends its own AP, scaled by the Action Point
  Cost perks as the weapon's attack is [design].
- Its blow's damage is multiplied (with a power attack's multiplier, in `HitInput::power`)
  and its effect goes on the target with the blow's other effects.
- Not built: the unarmed moves the page lists for bare fists (Uppercut, Stomp, Cross),
  which are not in the weapon records read here; the record's Silent and Mod Required
  fields are read by the schema and not used.

## 13. Dead Eye's parts on the same screen (2026-11-10)

Dead Eye's labels for the parts of the actor under or nearest the crosshair
(vats-mode.md §17.5) are the same element as the classic style's: the menu file's
bracket boxes with the chance and the condition meter [design: the fork owner
2026-11-10]. The file is built while Dead Eye marks. Only the boxes are drawn: no
arrows, buttons, queue list or pointer. The part under the crosshair is lit on the body
as in the classic style (§12), its lines as thin as the target is small in the view,
since Dead Eye does not zoom onto it. While Dead Eye's shots play nothing of the HUD is
drawn, as in the classic style. Dead Eye keeps its amber filter, its swipe and its
firing from the eye, and has no per-attack cameras [design: the fork owner 2026-11-10,
"leave Dead Eye as is"]. The HUD's enemy
bar names the previewed actor, hurt or not, and stands at the top of the view while
Dead Eye is on, as in the classic style. The part under the crosshair is named and drawn whole, the
others fainter. Dead Eye's marks ("x" and chance) and everything else of it are
unchanged. With `[UI] VatsMenuFile=0` or without the file its amber labels are drawn
as before.

On the settings page the style is "Mark and Fire" [design: NVForever's own words; the
owner's review of #2102]. "Dead Eye" names the mechanic it is modelled on, another
publisher's term, and stays in these notes, the code's names and the test aid's
`dead-eye` only. What it shares with the classic style, and so does differently from
before this branch: marks on a target that has died are dropped and the mode ends 2 s
later (§8.1), hiding the corner HUD from the first shot, the parts' labels from the
menu file, explosives holding the mode's end (§8.2), the kill
camera's last shot held 3 s longer and the playing shot's Player time multiplier on
the player's own attack (§8).

## 9a. Frame time (2026-11-10)

The chances are worked out every frame. Measured on the owner's machine at the
Goodsprings smoke spot, display sync off (`--log-frame-times`), 40 s runs: 198.0 frames
a second with VATS never opened; 209.5 with the classic style open at a full stop for
the last 28 s with two attacks queued (six labels on a dog). Opening it did not lower
the frame rate; the stopped world costs less than the labels add. Not measured: a
crowded scene, the top graphics settings, a slowdown under 100 %.

## 9b. Planned

- Mines as targets. The unarmed special moves.
- The menu file's Accept and Return buttons as click targets (§11).
- A camera that follows the projectile (the paths' Fly shots).
- A hidden part's chance: a part behind cover shows its usual chance, and a shot at it
  hits the cover.
- While a part is lit the systems that find the view with `Query::single` rest (§12);
  at less than a full stop no cell loads until the mode is left.

## 10. Test aids and smoke

- `NVF_PLAY_TEST_VATS_STYLE=classic|dead-eye`, `NVF_PLAY_TEST_VATS_SLOWDOWN=0.5..1`.
- Every Dead Eye scenario sets the first to `dead-eye`; their expectations are
  untouched.
- The classic scenarios are named `vats-classic-*`.
- `NVF_PLAY_TEST_VATS_MENU_FILE=0` keeps the engine-drawn target screen (§11).

## 11. The target screen from the game's menu file (2026-11-10)

By default the target screen is the game's own `menus\vats_menu.xml`, drawn by the tile
renderer (`engine/nvf-play/src/tiles/vats_menu.rs`); what the file holds and what is
written into it: spec-notes/vats-menu.md.

- **From the file**: a bracket on each body part of the selected target with its chance
  and its condition meter, the selected part named; the arrows at the view's edges;
  Accept (dim until an attack is queued), Return and the special attack button with
  their key hints; the queue's list of queued attacks with its divider.
- **Still the engine's**: the target's name and health, which are the HUD's own enemy
  bar (its bracket, ticks and name), shown for the selected target hurt or not and
  kept at bottom centre while choosing and through playback [design, #2659;
  documented: issue #2659, item 3, original capture description], with blinking ticks
  for what the queued attacks and the selected part would take off, a weapon's explosion counted
  (`combat::vats_hit_estimate`); the HUD's HP and AP meters. The picture is neither
  darkened nor tinted in the classic style [design: the fork owner 2026-11-10].
- **The switch**: on unless `[UI] VatsMenuFile=0` is in `NVForever.ini` [design: the
  fork owner's decision, the game's file first]. Upstream's convention is the other
  way round (`[UI] TileMenus=` lists the menus drawn from files, none by default); to
  go upstream this would become a name in that list.
- **The fallback**: a missing file, or one without a path the code writes, keeps the
  engine-drawn screen of §4 for the run and warns once with the reason.
- **Life**: VATS is no menu to the runtime (`Game::menu`), so the file is built on the
  frame the screen comes up (`Game::ui_vats`) and dropped on the frame it goes. While
  it is up `screen()` is in interface units and its `cropx` and `cropy` are 15; a mod's
  HUD tiles that read `screen()` see those values for that time.
- **The pointer** is shown while targets are chosen. Coming onto a part's box, or
  within 45 units of the part's point on the target, selects that part; the file's
  Select button shows then, with the left button's hint. A click queues an attack on
  the selected part; a click on one of the two arrows changes the target instead. The
  keys of §4 go on working, and the pointer selects only when it moves. A click on a
  button does what the button says: Accept plays the queue (it is no click target with
  nothing queued, the file's own rule), Return takes the last attack back as the right
  button does, the special attack's queues it (`vats_menu::click`).
- **The player's arms and weapon** are not drawn in first person while targets are
  chosen.
- **The UI scale** (the settings page's "UI scale", 50 to 150 %): the screen is drawn
  in the view's own frame as the HUD is, with `screen()` written for a view 960 / scale
  units high, so the arrows, the buttons and the queue keep to the window's edges and
  the boxes stay on the target. It shrinks with the scale and, like the menus, does
  not grow above 100 %: the file sets its buttons and queue out for a view 960 units
  high, and at 150 % they ran into each other [observed: the engine's screenshot]. The
  enemy bar is the HUD's and takes the whole scale; the engine-drawn labels and marks
  (the fallback, Dead Eye's marks) do too.
- **The two arrows** stand halfway down the view, on one line at the left and right
  edges, at any UI scale [design: the fork owner 2026-11-10; the file, and the
  original on the owner's screenshots, put them a quarter of the way down].
- **The mouse buttons' hints** on this screen are NVForever's own short words:
  "[RClick]" after Return and "[LClick]" after Select (`vats_menu::RETURN_HINT`,
  `SELECT_HINT`). The built-in `sPCMenuHintRMB` stays "[Right Click]" for the other
  menus: the file gives a hint its button's width as room, and that one ran off the
  view after "Return". The original's on-screen abbreviations are not used (LEGAL.md;
  the owner's review of #2102).

## 12. The selected part lit on the target (2026-11-10)

The part the next attack would be queued on is lit on the target's own body, and its
box on the screen is drawn fainter than the others [observed: the owner's screenshots
of the original's VATS, 2026-11-10] (`engine/nvforever/src/vats_highlight.rs`, whose
notes are the full account).

- A part's triangles are those carried mostly by the bones of its chain, the bones the
  hit volumes use; a bone in no chain goes with the chain above it [design]. The weapon
  in the target's hand is a part too. [design, #2659] For mixed meshes, a triangle is
  selected when its mean bone-weight share reaches `PART_TRIANGLE_SHARE` (0.5). All
  selected vertices use full mask alpha, as whole-part meshes do: weights select the
  geometry instead of weakening its fill. Previously `glow` put the weights in alpha
  on mixed meshes (such as legs), then `vats_mask.wesl::cover` thresholded the blended
  result; there is no area or triangle-count normalization. The fill strength, HUD
  colour, pulse and scanlines stay unchanged. The boundary follows whole triangles,
  so coarse meshes need picture review (owner-review, #2659).
- It is a HUD element, not a glow in the world: the part's shapes are drawn white (the
  rest of the actor black) on a layer of their own into a mask by a camera at the
  player's, and one node over the view draws from the mask a fill with fine level lines
  and a rounded outline, both in the HUD's colour. So depth of field and bloom do not
  touch it. Hair is in the mask through its texture's alpha.
- It pulses with the enemy bar's ticks for the damage in sight.
- The mask's camera lives only while a part is lit and half a second after. The world's
  systems find the view with a query for the one 3D camera that is not the reflection's
  (`world::stream`, `lod`, `grass`, `nav`, ...); kept for good, a second one stopped
  them all, no cell loaded after the first lit part and a door's loading screen waited
  out its 120 s [observed: the fork owner's playtest 2026-11-10]. While a part is lit
  they still rest (§9b).

## 13. Target screen alignment (#2659)

- [design, #2659] Queue rows join the target's data name and the part's data name as
  `<target>: <part>` (`vats_mode::queued_text`); a target without parts keeps its name
  alone. The colon is NVForever's joiner. [documented: issue #2659, item 1] Its capture
  description reports both data names with a colon in the original's queue.
- [design, #2659] In classic targeting, both HUD implementations show loaded rounds
  minus `MarkQueue::rounds_needed`, saturating at zero, and keep the spare count.
  Undo or clearing the queue returns those shown rounds. No inventory or weapon
  state changes until firing; playback reads the actual remaining rounds. Dead Eye
  retains its prior display rule. [documented: issue #2659, item 2] The capture
  description reports the displayed loaded count decreasing per queued shot.
- [design, #2659] The tile HUD keeps its own existing bottom anchor for `EnemyHealth`
  (`fnv-game/src/ui_hud.rs::write`); it accepts the engine's visibility decision,
  including full-health classic targets. The hand-built HUD gives the enemy piece
  an independent root so hiding the corner root during playback cannot hide it.
  [design, #2659; `free_cam.rs::hide_menus`, `hud.rs::draw`] Console `tm` also hides
  this independent piece; undoing it lets normal HUD eligibility restore the bar.
- [design, #2659] Pointer and click handling are outside this change (PR #2711).
  AP display, AP cost, the wider playback HUD and target-change timing remain open
  in observer check 215; no changes are made for those comparisons. Body-part keys
  (issue item 10) remain open as well.

## 14. Optional direct touch selection [design]

The independently saved Off/Tap/Hold presentation is specified in
[Android VATS touch](android-vats-touch.md). It uses actual posed-body ray hits and
the existing AP/legality queue consumer, without changing the physical/controller
rules above. Selecting queues; only a separate touch Fire confirmation starts the
existing execution path. The separate look-space double tap only enters VATS.
The source and authored App controls await compiler/runtime/phone qualification;
this addition does not alter the currently served APK.
