# Settings reference

The settings page (the pause menu's Settings) shows each row with a short note: what it
does, in one or two sentences [design: #2377, docs/design/settings-page.md §10]. This page
keeps the rest: for each row, by its INI key (`[Settings]` of `NVForever.ini`), where the
page shows it (tab and heading), its label, the note the page shows, the longer explanation
the note had before #2377 where it said more, and the command-line switches whose state the
row replaces (a test run that gives one keeps the command line's state over the INI's,
docs/design/settings-page.md §1). Rows are listed in the page's order, tab by tab and
heading by heading, as the menu overhaul placed them
(`crates/nvf-settings-layout/src/lib.rs`, `SECTIONS`; a test in
`engine/nvf-play/src/settings_layout.rs` fails while a row is missing here or under another
tab or category). The launcher shows these labels and notes until the game has written its
own menu (docs/design/launcher.md). The words are NVForever's own; the
facts are the rows' (each row's module, named under its key).

The labels here are as the rows register them. The page shows them in Title Case in English
(also where another language's file has no line for one): it title-cases them itself
(`engine/nvf-play/src/pause.rs`, `shown`).

A row whose module is not part of a run (lip sync without lip data, the draw distances, the
quality presets without the install's preset files) is not on the page then. Some rows are
hidden while they cannot apply: the upscaler's own rows (Upscaling mode shows only with an
upscaler on, DLSS model only with DLSS, FSR library only with FSR, Anti-aliasing only with
the upscaler off) and Generated frames (only with DLSS frame generation). Others stay on the
page greyed out while the row they need is off, their value kept and the reason in the note
(`crates/nvf-settings-layout/src/lib.rs`, `NEEDS`; Shadow distance needs Sun shadows, for
one).

The Controls tab's own rows (Input Device, Automatic Input Switching, Rumble, Invert Y, the
sensitivities, the controls) stand under the Controls headings the page shows them in. The
Tweaks tab's rows (#1892) are listed in docs/design/settings-tweaks-layout.md (including
`tweak-keep-chosen-ammo` — Keep Chosen Ammo, under Tweaks › Combat › Weapons and Armour,
off by default, saved in `NVForever.ini` `[Settings]` [design: #3305]).

Android-specific rows and HUD compatibility are documented in [the Android settings supplement](settings-reference-android.md); they are not part of the desktop launcher's built-in table.

## Gameplay › Game

### `difficulty`: Difficulty

- On the page: You deal / take / kill XP: Very Easy 200/50/100%, Easy 150/75/100%, Normal
  100/100/100%, Hard 75/150/100%, Very Hard 50/200/100%. (In a game the numbers are the load
  order's own: a plugin's `fDiffMult*` shows.)
- In full: How hard the fights are: from Very Easy (the player's hits do double damage and
  the player takes half) to Very Hard (half and double); Normal as they are. Custom lets
  the three sliders under the list, Player Damage, NPC Damage and Kill XP, set each part
  yourself; scripts' GetGameDifficulty sees Custom as Normal (2). (console:
  SetGameDifficulty 0-4; a script's SetGameDifficulty is kept as the chosen level)
- Shown as a list since the menu overhaul; an older slider number in the INI still loads as
  the nearest of the five levels.
- Registered in `engine/nvf-play/src/difficulty.rs`

### `difficulty-player-damage`: Player damage

- On the page: Custom difficulty: how much damage your hits do, from 50% to 300%.
- In full: The damage the player's hits do to anyone, 50% to 300% in steps of 5% (100% until
  moved). Greyed unless Difficulty is Custom; kept as the slider's part
  (0.20 = 100%).
- Registered in `engine/nvf-play/src/difficulty.rs`

### `difficulty-npc-damage`: NPC damage

- On the page: Custom difficulty: how much damage you take from NPCs and creatures (and falls
  and blasts), from 50% to 300%.
- In full: The damage the player takes from every source (as the five levels scale it), 50%
  to 300% in steps of 5% (100% until moved). NPCs' hits on each other and on companions
  are not scaled, as at every level. Greyed unless Difficulty is Custom.
- Registered in `engine/nvf-play/src/difficulty.rs`

### `difficulty-kill-xp`: Kill XP

- On the page: Custom difficulty: how much XP a kill earns, from 50% to 300%.
- In full: The XP the player earns for a kill, 50% to 300% in steps of 5% (100% until
  moved); other XP (quests, locks, terminals) is not scaled. Greyed unless
  Difficulty is Custom.
- Registered in `engine/nvf-play/src/difficulty.rs`

### `hardcore`: Hardcore mode

- On the page: On: you need water, food and sleep, stimpaks heal over time, ammo has weight,
  crippled limbs need a doctor's bag or a doctor. Off: none of that.
- In full: On: you need water, food and sleep, stimpaks heal over time, ammunition has
  weight and crippled limbs heal only with a doctor's bag or a doctor. Off: none of that;
  turning it off clears your thirst, hunger and tiredness and their effects. (console:
  SetHardcore 1/0)
- Registered in `engine/nvf-play/src/hardcore.rs`

### `reload-slows`: Slower while reloading

- On the page: On: you and the NPCs move at 75% of your speed while a reload runs. Off:
  reloading does not slow anyone, as in the original.
- Registered in `engine/nvf-play/src/combat.rs`

### `UsePipBoy`: Use Pip-Boy

- On the page: On: the Pip-Boy is worn and the menus come up on the arm. Off: it is taken
  off and the menus show full screen, as always without a Pip-Boy.
- In full: On: the Pip-Boy and its glove are worn and the menus come up on the arm. Off:
  both are taken off and the menus show full screen. Without a Pip-Boy worn the menus are
  always full screen.
- Registered in `engine/nvf-play/src/pipboy.rs`

### `sound-cover`: Sounds of the world cover your noise

- On the page: On: while a louder sound plays near you (machines, voices, shots, creatures),
  the noise you make is not heard. Not in the original.
- In full: On: when the noise you make (footsteps, your shots) is less than the loudest
  sound of the world heard where you are (sound markers, machines' and creatures' loops,
  other people's footsteps, shots and voices, radios), no one hears you; your Sneak still
  counts against anyone who sees you. The sneak gauges' noise bar marks the cover with a
  bright square. Off: detection is the original's. Not in the original (#2688).
- Registered in `engine/nvf-play/src/sound_cover.rs`

### `npc-weapon-mods`: NPC weapon mods

- On the page: On: NPCs have a level-scaled chance (10%-60%, +2%/level) for their main weapon to spawn with random weapon mods. Off: NPC weapons have no extra mods, as in the original.
- In full: Off by default. When on, each non-player NPC (`NPC_`, including leveled `LVLN` picks) rolls once when its main weapon is outfitted for a `Min(60, Max(10, Level * 2))`% chance to attach 1-3 random weapon mods from the weapon's valid `WMI1`-`WMI3` slots, without overwriting already-modded weapons or re-rolling across ticks. Never reads `jip_nvse.ini` (`docs/design/extender-compat.md` §8.4). Follows JIP LN NVSE Plugin `57.30` (`https://www.nexusmods.com/newvegas/mods/58277`) `jip_nvse.ini` `[GamePatches]` `bNPCWeaponMods` (`uWMChancePerLevel=2`, `uWMChanceMin=10`, `uWMChanceMax=60`; no DLL or source consulted): "When enabled, all NPCs will have a chance for their main weapon to include weapon mods. The chance is based on each NPC's level - the higher the level, the greater the chance. The type of mod(s) is selected randomly from the ones available for the particular weapon. Weapons have a chance to include multiple mods, though 2 mods is rare, and a fully-modded weapon is uncommon." NVForever's deterministic per-NPC seed, multi-mod probability curve, and single-roll tracking are NVForever's own (`[design]`, `docs/spec-notes/npc-weapon-mods.md`, #3876).
- Registered in `engine/nvf-play/src/tweaks.rs`

### `sneak-bounding-box`: Crouch height collision

- On the page: On: crouching shrinks collision height for you and crouching human companions
  to fit under low spaces, holding sneak if headroom is blocked. Off: fixed standing height.
- In full: Default Off (vanilla keeps the standing character collider height while sneaking).
  When On, crouching scales the collision capsule height for the player and crouching human
  companions so they can move under low clearances, and keeps sneak active if overhead
  geometry blocks standing back up. Optional NVForever setting following the public
  `bSneakBoundingBoxFix` behaviour in JIP LN NVSE Plugin v57.30
  (`docs/spec-notes/sneak-bounding-box.md`, #3845).
- Registered in `engine/nvf-play/src/player.rs`

## Gameplay › V.A.T.S.

### `vats-style`: V.A.T.S. style

- On the page: What the V.A.T.S. key starts. Classic: the world stops and you pick body
  parts to hit. Mark and Fire: time runs very slowly while you mark targets.
- In full: What the V.A.T.S. key starts. Classic: the world stops, you pick a target and its
  body parts from their chances to hit, then the queued attacks play. Mark and Fire: time
  runs very slowly while you mark targets under the crosshair.
- Registered in `engine/nvf-play/src/vats_classic.rs`

### `vats-slowdown`: V.A.T.S. time slowdown

- On the page: How far the world slows while you choose targets in Classic: 100% stops it, 50%
  runs at half speed, 0% not at all. Enemies attack at anything less than a stop.
- In full: How far the world slows while you choose targets in the Classic style: 100% stops
  it, 50% runs it at half speed. Enemies keep attacking at anything less than a full stop.
- Shown as a list since the menu overhaul; an older slider number in the INI still loads as
  the nearest choice. Its choices: 0%, 25%, 50%, 75% and 100%; 0% leaves time running.
- Registered in `engine/nvf-play/src/vats_classic.rs`

### `kill-camera`: Kill camera

- On the page: When a VATS shot kills its last mark, a camera shows the kill. Last enemy:
  only when no other enemy still fights you. Every kill: always. Off: never.
- In full: When a VATS shot kills its last mark, a cinematic camera can show the kill. Last
  enemy: only when no other enemy is still fighting you. Every kill: whoever is left. Off:
  never.
- Registered in `engine/nvf-play/src/kill_cam.rs`

## Gameplay › Camera

### `detached-camera`: Detached third-person camera

- On the page: On: in third person with your weapon away the mouse turns the camera around
  you and you run whichever way you push. Off: the camera stays behind you.
- In full: On: in third person with your weapon away the mouse turns the camera around you,
  not your character, and your character turns to run whichever way you push, as the camera
  sees it. Drawing your weapon turns you to where the camera looks. Off: the camera stays
  behind you, as in the original.
- Registered in `engine/nvf-play/src/player.rs`

### `true-iron-sights`: True iron sights

- On the page: On: aiming a gun lines its sights up with your eye and hides the crosshair.
  Off: the gun is held low with the crosshair, as in the original.
- In full: On: aiming a gun lines its sights up with your eye. The crosshair is hidden while
  you aim down them. Off: the gun is held low with the crosshair, as in the original. Guns
  with no sights to look down (laser rifle, minigun) aim as the original either way.
- Command-line switches: `--no-true-iron-sights`
- Registered in `engine/nvforever/src/settings.rs`

### `first-person-body`: Body in first person

- On the page: On: in first person you see your own body and legs when you look down. Off:
  only the arms and the weapon are drawn, as in the original.
- Command-line switches: `--first-person-body`
- Registered in `engine/nvforever/src/settings.rs`

### `weapon-lower`: Lower weapon near walls

- On the page: On: in first person a drawn weapon lowers at a wall or a friend and comes
  back up when you aim or fire. Off: it stays up, as in the original.
- In full: On: in first person a drawn weapon is lowered when a wall or a friendly person is
  right in front of you, and comes straight back up when you aim or fire. Off: it stays up
  and can poke through them, as in the original. Lower Weapon (H) lowers or raises it by
  hand either way.
- Command-line switches: `--no-weapon-lower`
- Registered in `engine/nvforever/src/settings.rs`

### `gore-shake`: Gore shake

- On the page: On: a body part blown apart near you (a gory kill) shakes the view too. Off:
  only real explosions shake it.
- In full: A body part blown apart or severed sets off its own small blast, which shook the
  view like an explosion's whenever it went off within about 1,400 units, so many
  critical kills shook the screen. Off by default; real explosions shake the view either
  way (the Tweaks tab's Scale Explosion Shake sizes that shake).
- Registered in `engine/nvf-play/src/shake.rs`

### `field-of-view`: Field of view

- On the page: The game's field of view in degrees across a 4:3 picture (75 as the
  original's default); wider screens see more at the sides.
- In full: Scripts read the angle set here (GetCameraFOV 0, GetPlayerCamFOV 0) from the start
  and after each change, as the original's scripts read its INI's; a console `fov` world
  angle wins until the slider is moved again.
- Command-line switches: `--fov`
- Registered in `engine/nvforever/src/settings.rs`

### `viewmodel-field-of-view`: Viewmodel field of view

- On the page: The angle the first-person arms and weapon are drawn with, in degrees (55 as
  the original's default): lower draws them bigger and closer.
- In full: The field of view the first-person arms and weapon are drawn with, from 30 to 90 in
  steps of 5, in degrees as the field of view counts them (across a 4:3 picture; 55 as the
  original's `fDefault1stPersonFOV`). Lower draws them bigger and closer, higher smaller and
  further off; it takes effect at once. The world's view, the Pip-Boy's own 47 and the
  console's `fov` first-person angle are left as they are. Scripts read it as the
  first-person field of view (GetCameraFOV 1, GetPlayerCamFOV 1, and GetPlayerCamFOV 2 in
  first person) from the start and after each change; a console `fov` first-person angle
  wins there too. Not on the original's settings page.
- Command-line switches: `--viewmodel-fov`
- Registered in `engine/nvforever/src/settings.rs`

## Gameplay › Dialogue

### `dialogue-zoom`: Close In on the Speaker

- On the page: On: in a conversation the view closes in on the speaker, the background out
  of focus; the mouse moves only the cursor. Off: you can look around.
- In full: On: in a conversation the view narrows onto the speaker, with what is behind them
  out of focus; from third person the camera comes to the eyes. The view stays on the
  speaker and the mouse moves only the cursor. Off: no close-up; you can look around.
- Command-line switches: `--no-dialogue-zoom`
- Registered in `engine/nvf-play/src/dialogue_look.rs`

### `dialogue-look`: Turn to Each Other

- On the page: On: at the start of a conversation the actor turns to the player and the view
  turns to the actor.
- Command-line switches: `--no-dialogue-look`
- Registered in `engine/nvf-play/src/dialogue_look.rs`

### `dialogue-recentre`: Bring the View Back on Choices

- On the page: On: when choices appear the view returns to the speaker (with the close-up
  off). Needs "Turn to Each Other".
- In full: On: with "Close In on the Speaker" off, after you look away the view
  returns to the speaker when choices appear. Needs "Turn to Each Other".
- Command-line switches: `--dialogue-recentre`
- Registered in `engine/nvf-play/src/dialogue_look.rs`

### `menu-pause`: Freeze the picture in dialogue and menus

- On the page: On: animations, wind, water and sky stand still in conversations, the
  inventory and other menus, as in the original. Off: only game logic waits.
- In full: On: while a conversation, the inventory, a container or a message box is open,
  animations, wind, water and sky stand still, as in the original. Off: only game logic
  waits and the world lives on. This menu always holds everything.
- Command-line switches: `--menu-pause`
- Registered in `engine/nvf-play/src/pause.rs`

### `dialogue-goodbye-hint`: Goodbye hint

- On the page: On: "[Tab] Goodbye" stands at the left of the dialogue box while a Goodbye
  choice is offered. Off: hidden; Tab still says goodbye.
- In full: On: while a conversation offers a Goodbye choice, "[Tab] Goodbye" stands at the
  left of the dialogue box. Off: the words are hidden; Tab still says goodbye.
- Registered in `engine/nvf-play/src/dialogue_menu.rs`

### `lip-sync`: Lip sync

- On the page: On: a speaker's face moves with the line. Off: a face that is moving eases
  back to rest.
- Command-line switches: `--no-lip`
- Registered in `engine/nvforever/src/settings.rs`

## Gameplay › Saving

### `autosave-travel`: Autosave after travel

- On the page: On: the game saves to the autosave slot after each loading screen (doors,
  coc, a script moving you); fast travel has its own row.
- Registered in `engine/nvf-play/src/save.rs`

### `autosave-fast-travel`: Save on fast travel

- On the page: On: the game saves to the autosave slot when a fast travel arrives.
- Registered in `engine/nvf-play/src/save.rs`

### `autosave-wait`: Save on wait

- On the page: On: the game saves to the autosave slot when a wait ends with at least one
  hour passed.
- Registered in `engine/nvf-play/src/save.rs`

### `autosave-sleep`: Save on sleep

- On the page: On: the game saves to the autosave slot when the player wakes from a sleep of
  at least one hour.
- Registered in `engine/nvf-play/src/save.rs`

### `autosave-scripts`: Save when a script asks

- On the page: On: a script's Autosave saves to the autosave slot (quest milestones),
  SystemSave to the system slot, ForceSave as a new save.
- Registered in `engine/nvf-play/src/save.rs`

## Gameplay › Combat

### `localized-dt-dr`: Localized armor DT/DR

- On the page: On: head hits use only worn head armor DT/DR and body hits only worn body
  armor; explosions use all worn armor and innate DT/DR still applies. Off: full worn armor
  everywhere.
- In full: Default Off (vanilla sums all worn armor DT/DR across head and body slots on
  every hit). When On, head hits benefit only from worn head-slot armor DT/DR and body hits
  benefit only from worn body-slot armor DT/DR, while explosions still apply full worn armor
  and actor innate DT/DR (AV 76/18) still applies everywhere. Optional NVForever setting
  following the public `bLocalizedDTDR` behaviour in JIP LN NVSE Plugin v57.30
  (`docs/spec-notes/localized-dt-dr.md`, #3843).
- Registered in `engine/nvf-play/src/combat.rs`

## Gameplay › Menus

### `quantity-select-short-keys`: Quantity select shortcuts

- On the page: In inventory, container and barter menus, Shift transfers or drops the whole
  stack, Ctrl acts on one item, and weightless container stacks open the quantity prompt.
- In full: Off by default (unmodded menu behaviour unchanged). On: in Inventory, Container
  and Barter menus, holding Shift transfers or drops the entire stack and holding Ctrl
  transfers or drops a single item without opening the quantity prompt (Shift takes
  precedence if both are held); picking up a stack of more than one weightless item (such
  as ammo outside Hardcore mode) from a container opens the quantity prompt instead of
  taking without asking. Optional NVForever setting following the public `bQttSelectShortKeys`
  behaviour in JIP LN NVSE Plugin v57.30 (`docs/spec-notes/quantity-select-short-keys.md`,
  #3799).
- Registered in `engine/nvf-play/src/menu.rs`

## Gameplay › Bug Reports

### `send-reports`: Send reports to help fix NVForever

- On the page: After each session: build, hardware, settings, mod list, how it ran and the
  log; no personal details, no saves. From the next start
- More: docs/design/player-reports.md
- Registered in `engine/nvforever/src/reports/mod.rs`

### `reports-delete`: Reports already sent

- On the page: Delete them now asks the server to delete every report this PC sent (within 7
  days) and drops unsent ones; later reports get a new ID.
- In full: Delete them now asks the server to delete every report this PC sent (done within
  7 days) and drops the ones not sent yet; later reports, this session's too, get a new ID
- More: docs/design/player-reports.md
- Registered in `engine/nvforever/src/reports/delete.rs`

## Gameplay › Co-op

### `coop`: Co-op

- On the page: On: the main menu shows Join Lobby and the pause menu a Co-op screen to open
  a lobby. It stays on while a lobby is open or joined.
- Off by default; kept in `NVForever.ini` `[Settings]` [design].
- Greyed while a lobby is open or this game is a guest in one.
- Registered in `engine/nvf-play/src/lobby.rs`

### `coop-connection`: Co-op connection

- On the page: Automatic: online codes, and through another player when needed. Direct only:
  never through another player. LAN only: players on this network only.
- In full: Automatic: online codes through the matchmaking service, the router asked for a
  port, and a relay through another player when no direct path is found. Direct only: the
  same, never through another player. LAN only: no matchmaking service, router port or relay.
- More: docs/design/coop-networking.md
- Registered in `engine/nvforever/src/coop/net_settings.rs`

### `coop-router-port`: Open router port automatically

- On the page: On: your router is asked to let co-op players in (UPnP, NAT-PMP or PCP), and
  the port closes when you leave. Applies to the next lobby or join.
- Greyed with LAN only.
- Registered in `engine/nvforever/src/coop/net_settings.rs`

### `coop-relay`: Relay for other players

- On the page: On: a player in your lobby who cannot reach the host may play through your
  game (at most 2, about 15 KB/s each). Their traffic stays encrypted.
- Greyed with LAN only.
- Registered in `engine/nvforever/src/coop/net_settings.rs`

### `coop-ask`: Ask before players join

- On the page: On: when you host, a player joining by online code waits until you let them in
  from the Co-op Lobby. Players on your network join at once.
- Registered in `engine/nvforever/src/coop/net_settings.rs`

## Display › Screen

### `monitor`: Monitor

- On the page: The display the game opens on in full screen (the main display until you
  pick one).
- Command-line switches: `--window`
- Registered in `engine/nvforever/src/settings.rs`

### `resolution`: Resolution

- On the page: The window's size, or in full screen the monitor's (its own size:
  borderless). A new one goes back after 20 seconds unless you keep it.
- Command-line switches: `--size`
- Registered in `engine/nvforever/src/settings.rs`

### `hdr-output`: HDR output

- On the page: On: the game is drawn for an HDR screen (Windows HDR on). Takes effect at once.
  Calibrate... sets the screen's peak and the brightness.
- In full: A choice list: Off, On, Calibrate.... On draws the game in HDR on a screen with Windows
  HDR on; with no HDR screen or no fp16 surface it stays SDR. It switches live, with no restart.
  Calibrate... opens the calibration pictures for the screen's peak and the brightness.
- More: docs/design/hdr-output.md
- Registered in `engine/nvforever/src/hdr.rs`

### `borderless`: Full screen

- On the page: On: the game fills its monitor without a frame, at the full screen resolution
  above. Off: a window of the window resolution.
- In full: On: the game fills the monitor it is on, without a frame, at the resolution
  chosen above for full screen. Off: a window of the resolution chosen for a window.
- Command-line switches: `--window`
- Registered in `engine/nvforever/src/settings.rs`

## Display › Frame Rate

### `display-sync`: Display sync

- On the page: Vertical sync: frames wait for the display (no tearing). Variable refresh:
  for a G-Sync or FreeSync screen. Off: frames show at once (may tear).
- In full: Vertical sync: each frame waits for the display's refresh (no tearing). Variable
  refresh: for a G-Sync or FreeSync screen, vertical sync with the frame rate held a few
  frames under the screen's refresh rate, so frames are shown as soon as they are drawn
  without tearing; with DLSS frame generation, vertical sync is off and the limit is kept
  (NVIDIA's frame generation does not take the game's vsync on Vulkan; for no tearing set
  Vertical sync On for the game in the NVIDIA Control Panel).
  Off: frames are shown as soon as they are drawn (may tear).
- Registered in `engine/nvforever/src/settings.rs`

### `frame-cap`: Frame rate limit

- On the page: At most this many frames a second (Off: no limit of its own). A lower limit
  keeps the computer cooler and quieter.
- In full: At most this many frames a second (Off: as many as the computer draws, or the
  display's refresh with vertical sync; with variable refresh the lower of this and its own
  limit). It counts the frames shown: with frame generation the real frames are this divided
  by the frames shown for each. With vertical sync a limit at or above the display's refresh
  is left to vsync. A lower limit keeps the computer cooler and quieter.
- Shown as a list since the menu overhaul; an older slider number in the INI still loads as
  the nearest choice.
- Registered in `engine/nvforever/src/settings.rs`

### `physics-rate`: Physics rate

- On the page: How many times a second the physics moves the world on. Higher answers the
  keys sooner and collides more finely, but costs the processor more.
- In full: How many times a second the physics moves the world on: the player, the people,
  what falls and what is pushed. A higher rate answers the keys sooner and meets collisions
  more finely; its cost to the processor grows with the rate. Match FPS: as many as the
  frames a second the game draws, from 60 up to what the display sync and the frame rate
  limit allow (at most 160); it follows the frame rate down when it stays low and back up
  when it recovers. When the physics takes over half of each frame, the rate goes down a
  step (never under 60) until there is room again. While `--physics-hz` sets the rate for a
  run, the row shows "Set by --physics-hz for this run".
- Command-line switches: `--physics-hz`
- Registered in `engine/nvf-play/src/phys_rate.rs`

## Display › Upscaling and Anti-Aliasing

### `upscaler`: Upscaler

- On the page: FSR 3.1 or NVIDIA DLSS rebuilds the world at display resolution, at the mode
  below. DLSS needs an NVIDIA RTX card and its files in the dlss folder.
- In full: Reconstruct the world at display resolution with FSR 3.1 or NVIDIA DLSS, at the
  Upscaling mode below. Disables MSAA, FXAA, SMAA and TAA while active. Menus stay sharp.
  Requires Vulkan; unsupported devices keep native rendering. DLSS needs an NVIDIA RTX
  graphics card and NVIDIA's DLSS files in the dlss folder beside nvforever.exe (NVForever
  ships none). With the files there when the game starts, every choice takes effect at once;
  files put there later are found at the next start, so DLSS chosen until then reads DLSS
  (restart) (loading a save starts the game again). Sharpening controls FSR sharpening.
  Frame generation needs an upscaler.
- Command-line switches: `--upscaler`, `--upscaler-quality`, `--fsr`
- More: docs/dlss.md, docs/design/fsr-upscaling.md, docs/design/dlss-upscaling.md
- Registered in `engine/nvforever/src/settings.rs`

### `upscaler-quality`: Upscaling mode

- On the page: The share of the screen the world is drawn at; the upscaler rebuilds the
  rest. Native AA: full size. Quality to Ultra Performance: ever faster.
- In full: The share of the screen's width and height the world is drawn at; the upscaler
  rebuilds the rest. Native AA: full resolution, anti-aliasing only (DLSS: DLAA). Quality:
  the sharpest upscaled look, for 1440p and 4K. Balanced: more speed with little visible
  loss. Performance: the highest frame rate, for 4K or heavy scenes. Ultra Performance: for
  very high resolutions only. Takes effect at once. With Upscaler Off there is nothing to
  set.
- Command-line switches: `--upscaler-quality`, `--upscaler`, `--fsr`
- More: docs/design/fsr-upscaling.md, docs/design/dlss-upscaling.md
- Registered in `engine/nvforever/src/settings.rs`

### `dlss-model`: DLSS model

- On the page: Which DLSS model rebuilds the picture (NVIDIA calls them presets). Default,
  recommended: NVIDIA picks per mode. Only while DLSS draws.
- In full: Which of NVIDIA's DLSS models rebuilds the picture (NVIDIA calls them presets).
  Default (recommended): NVIDIA picks per mode: K for Native AA, Quality and Balanced, M for
  Performance, L for Ultra Performance. K: transformer model, best detail, costs more. L:
  sharper and steadier, less ghosting, costs the most. M: close to L in looks, closer to K
  in speed. J: like K with a little less ghosting and more flicker. Takes effect at once.
  Only while DLSS draws.
- Command-line switches: `--dlss-model`
- More: docs/dlss.md, docs/design/dlss-upscaling.md
- Registered in `engine/nvforever/src/settings.rs`

### `fsr-library`: FSR library

- On the page: What FSR upscales with: NVForever's built-in FSR 3.1, or a FidelityFX library
  in the game's folder (for tools that replace the upscaler).
- In full: What FSR upscales with: NVForever's built-in FSR 3.1, or a FidelityFX library in
  the game's folder (amd_fidelityfx_loader_vk.dll or amd_fidelityfx_vk.dll; not part of
  NVForever), which tools that replace the upscaler use. The choice names the library found;
  with none there the built-in FSR runs.
- More: docs/design/fsr-upscaling.md
- Registered in `engine/nvforever/src/settings.rs`

### `anti-aliasing`: Anti-aliasing

- On the page: How edges are smoothed: multisampling 2x, 4x or 8x (dearer at each step) or
  FXAA, SMAA or TAA afterwards. Shown while no upscaler is chosen.
- In full: How edges are smoothed: multisampling Off, 2x, 4x or 8x (--msaa; the cut-out
  edges of plants too), or the picture smoothed afterwards: FXAA (cheapest, softest), SMAA
  (sharper, a little dearer) or TAA (over the frames; plants too, soft while moving;
  --graphics-only fxaa, smaa or taa). Multisampling is the dearest, more at each step: at
  1920x1080 on an RTX 4070, 2x added about 0.8 ms a frame, 4x about 1.3 ms and 8x about 2.3
  ms (Goodsprings and Novac at noon); FXAA, SMAA and TAA cost less than 4x. Where the
  processor sets the pace (the Strip at night) the choice costs next to nothing. While an
  upscaler (FSR 3.1 or DLSS) is on it does the anti-aliasing itself, so this row reads Off
  (upscaling active); your choice comes back when Upscaler is Off.
- Shown as a list since the menu overhaul; an older slider number in the INI still loads as
  the nearest choice.
- Command-line switches: `--msaa`, `--graphics-only`
- Registered in `engine/nvforever/src/settings.rs`

### `sharpen`: Sharpening

- On the page: On: the final picture is sharpened where it has little contrast
  (contrast-adaptive sharpening).
- Command-line switches: `--graphics`
- More: docs/design/graphics-enhanced.md
- Registered in `engine/nvforever/src/settings.rs`

## Display › Frame Generation and Latency

### `frame-generation`: Frame generation

- On the page: Frames made between the real ones: smoother motion, input a little later. FSR
  3.1 needs FSR upscaling; DLSS an RTX 40 or newer. Not in the original.
- In full: Frames made between the real ones, so motion looks smoother; what you do reaches
  the screen a little later. FSR 3.1: one frame between each two real frames; needs FSR 3.1
  upscaling (any mode; not with DLSS) and vertical sync or variable refresh, and real frames
  are held to just under half the refresh rate. DLSS: NVIDIA DLSS Frame Generation, one to
  three frames for each real frame (Generated frames, below); needs an upscaler (FSR 3.1 or
  DLSS), Display sync Off or Variable refresh, an NVIDIA RTX 40 series or newer graphics
  card, Windows' hardware-accelerated GPU scheduling and NVIDIA's frame generation files in
  the dlss folder beside nvforever.exe; frames are generated only while the game's window is in front.
  Latency reduction turns on with either. The HUD and menus are not generated; menus and
  loading screens show real frames only. Best from 60 real frames a second. Not in the
  original.
- Command-line switches: `--frame-generation`
- More: docs/design/frame-generation.md, docs/design/dlss-frame-generation.md
- Registered in `engine/nvforever/src/settings.rs`

### `frame-generation-frames`: Generated frames

- On the page: How many frames DLSS frame generation makes per real frame: 2x one, 3x two,
  4x three (3x and 4x on RTX 50 series). Not in the original.
- In full: How many frames DLSS frame generation makes for each real frame: 2x one, 3x two,
  4x three. More looks smoother and costs a little more latency and graphics memory; 3x and
  4x only where the graphics card offers them (RTX 50 series). Takes effect at once. Only
  with DLSS frame generation. Not in the original.
- In its place on the page while the frame rate limit divided by the frames shown per real
  frame is under 60 (#2731), e.g.: 4x at a limit of 144: 36 real frames a second, which can
  feel choppy. Fewer generated frames, or a limit up to the display's refresh rate, give more.
- More: docs/design/dlss-frame-generation.md
- Registered in `engine/nvforever/src/settings.rs`

### `latency-reduction`: Latency reduction

- On the page: The graphics driver keeps the game from preparing frames ahead (AMD Anti-Lag,
  NVIDIA low latency), meant to show what you do sooner. Not in the original.
- In full: The graphics driver keeps the game from preparing frames ahead of the graphics
  card: AMD Anti-Lag on AMD cards (Windows: Adrenalin 24.9.1 or later; Linux: Mesa 26.3 or
  later), NVIDIA low latency on NVIDIA cards whose Vulkan driver has it. It is meant to make
  what you do reach the screen sooner; that has not been measured yet. The log says which of
  the two this graphics card uses, or why neither. Not in the original.
- Command-line switches: `--latency-reduction`
- Registered in `engine/nvforever/src/settings.rs`

## Display › Interface

### `language`: Language

- On the page: The language of NVForever's own words (menus, messages). The game's own text
  is the install's.
- Registered in `engine/nvf-play/src/language.rs`


### `menu-font`: Menu font

- On the page: The menus' letters. Original: the game's own font (Sharp for a language it
  has no letters for). Sharp: Roboto Condensed, sharp at any resolution, titles bold.
- In full: Sharp draws the main menu's entries and the pause menu's text (the column, the
  settings page, the load page) with Roboto Condensed (SIL Open Font License 1.1, Fallout 4's
  own interface font), laid out by Bevy's text at the screen's resolution, without kerning or
  ligatures so it is as wide as the page measures it: the rows at an em of 31 interface units,
  the small print at 24, the titles (the column, the categories, the tab rail, Defaults and
  Back) bold (weight 700, measured from the font's `HVAR` table), each line over a dark glow
  and a drop shadow as the game's own fonts have in their pictures. Original keeps the game's
  raster font, whose letters are stretched at high resolutions. Default Original [design: owner
  2026-11-15, the menu overhaul and its second test; 2026-11-16: the vanilla font by default].
  The game's font has only its code page's 256 letters: where the chosen language's are not
  among them (Russian on an English install, whose page is Windows-1252) Original draws
  sharp too, else the words would read as `?`; Sharp draws every letter the font has
  (Latin and Cyrillic), whatever the install's code page (owner 2026-11-16). The other
  menus keep the game's font.
- Registered in `engine/nvf-play/src/pause.rs` (`MenuFont`), the font in
  `engine/nvf-play/src/vector_font.rs`
### `sneak-gauges`: Light and noise gauges while sneaking

- On the page: On: two bars under the sneak word show how lit you are and how much noise you
  make, as detection counts them. Not in the original.
- Registered in `engine/nvf-play/src/sneak_gauges.rs`

### `caravan-tips`: Caravan tips

- On the page: On: the Caravan table says what the chosen card does and why a card can't be
  played, and the menu has a Tips button. Off: only the game's short messages. Not in
  the original.
- Registered in `engine/nvf-play/src/caravan.rs`

### `dynamic-crosshair`: Dynamic crosshair

- On the page: On: with a gun or thrown weapon drawn the crosshair opens with how wide your
  shots spread now. Off: the arrows stay together.
- In full: On: with a gun or thrown weapon drawn the crosshair opens to show how wide your
  shots spread now (wider running, after shots, with crippled arms; narrower aimed or
  crouched); Crosshair style below picks how it looks. Off: the arrows stay together
  whatever is drawn.
- Registered in `engine/nvf-play/src/reticle.rs`

### `crosshair-style`: Crosshair style

- On the page: With Dynamic crosshair on. Cross: four bars that part with the spread.
  Arrows: the plain crosshair's two arrows part instead.
- In full: With Dynamic crosshair on and a gun or thrown weapon drawn. Cross: four bars that
  part with the spread. Arrows: the plain crosshair's two arrows part instead.
- Registered in `engine/nvf-play/src/reticle.rs`

### `location-load-screens`: Location-specific loading screens

- On the page: On: prefer loading screens matching the destination cell or worldspace over
  generic screens. Off: mix matching and generic loading screens as in the original.
- In full: Default Off (vanilla mixes generic loading screens into the same pool as
  location-specific `LSCR` records). When On, destination-matching location-specific `LSCR`
  records are always preferred over generic loading screens, falling back to generic screens
  only when no location-specific screen matches. Optional NVForever setting following the
  public `bLoadScreenFix` behaviour in JIP LN NVSE Plugin v57.30
  (`docs/spec-notes/location-load-screens.md`, #3844).
- Registered in `engine/nvf-play/src/loading.rs`

### `ui-scale`: HUD scale

- On the page: Size of the in-game HUD, independent of menu scale. Takes effect at the next start.
- In full: The in-game HUD keeps each piece at its screen edge, from 50% to 100% in steps of 10% (150% before the menu overhaul).
  The existing `ui-scale` INI key is retained. Menu scale is independent. Loading a
  save starts the game again and applies the next-start settings.
- Shown as a list since the menu overhaul; an older slider number in the INI still loads as
  the nearest choice. Its choices: 50% to 100% in 10% steps.
- Registered in `engine/nvf-play/src/interface.rs`

### `menu-scale`: Menu scale

- On the page: Size of the main, pause and other menus, independent of HUD scale. Takes effect at the next start.
- In full: Main, pause/settings and other menus stay centred, from 50% to 100%.
  This retains the existing fixed-layout cap; larger menus would need reflow.
  A missing key starts with the old effective menu size. Any settings edit saves
  that independent value, including when only HUD scale changed; it will not
  follow the HUD on restart. Explicit menu values win. See
  [HUD/menu scale](spec-notes/hud-menu-scale.md) for normalized INI slider units.
- Shown as a list since the menu overhaul; an older slider number in the INI still loads as
  the nearest choice. Its choices: 50% to 100% in 10% steps.
- Registered in `engine/nvf-play/src/menu_scale.rs`

## Display › Colours

### `hud-colour`: HUD colour

- On the page: The colour of the HUD and the menus: Amber (the default) or another; the
  sliders below set it to any colour.
- Registered in `engine/nvf-play/src/ui_colour.rs`

### `hud-red`: HUD red

- On the page: One part of the colour above, 0 to 255; drag the meter or use Left and Right.
- Registered in `engine/nvf-play/src/ui_colour.rs`

### `hud-green`: HUD green

- On the page: One part of the colour above, 0 to 255; drag the meter or use Left and Right.
- Registered in `engine/nvf-play/src/ui_colour.rs`

### `hud-blue`: HUD blue

- On the page: One part of the colour above, 0 to 255; drag the meter or use Left and Right.
- Registered in `engine/nvf-play/src/ui_colour.rs`

### `pipboy-colour`: Pip-Boy colour

- On the page: The colour of the Pip-Boy's screens (items, stats, data): Amber (the default)
  or another; the sliders below set it to any colour.
- Registered in `engine/nvf-play/src/ui_colour.rs`

### `pipboy-red`: Pip-Boy red

- On the page: One part of the colour above, 0 to 255; drag the meter or use Left and Right.
- Registered in `engine/nvf-play/src/ui_colour.rs`

### `pipboy-green`: Pip-Boy green

- On the page: One part of the colour above, 0 to 255; drag the meter or use Left and Right.
- Registered in `engine/nvf-play/src/ui_colour.rs`

### `pipboy-blue`: Pip-Boy blue

- On the page: One part of the colour above, 0 to 255; drag the meter or use Left and Right.
- Registered in `engine/nvf-play/src/ui_colour.rs`

## Display › Performance Overlay

### `fps-counter`: Frame rate counter

- On the page: On: a panel of performance readings at the top right; the Counter rows below
  choose what it shows. Not in the original.
- In full: On: a small panel of performance readings at the top right of the screen, the
  Counter rows below choosing what it shows; with only its frame rate row on it is a frame
  rate counter. Not in the original.
- Command-line switches: `--fps`
- Registered in `engine/nvforever/src/settings.rs`

### `fps-counter-fps`: Counter: frame rate

- On the page: Frames a second, the mean frame time and the 1% low or the slowest frame (the
  row below chooses), from the time between frames.
- In full: The frames a second and the mean frame time of the last half second, and the 1%
  low (the mean of the slowest hundredth of the frames of the last 10 seconds) or the
  slowest frame of the last 10 seconds, as the row below chooses. Measured from the time
  between frames. While frame generation shows generated frames, the frames shown a second
  come first and the rendered ones in brackets; the frame times are the rendered frames'.
- Registered in `engine/nvforever/src/settings.rs`

### `fps-counter-graph`: Counter: frame graph

- On the page: The frame times of the last 5 seconds as a line under the frame rate, the
  newest at the right, a faint line at 60 Hz (16.7 ms).
- In full: The frame times of the last 5 seconds as a line under the frame rate, the newest
  at the right, with a faint line at a 60 Hz frame (16.7 ms); its top is the slowest frame
  drawn, or two 60 Hz frames when every frame was faster, so a spike reaches the top.
  Measured from the time between frames, drawn again twice a second.
- Registered in `engine/nvforever/src/settings.rs`

### `fps-counter-split`: Counter: frame split

- On the page: What a frame took on the main thread, the render thread (its wait for the
  window in brackets) and the GPU. The GPU part needs this row on at the start.
- In full: What a frame took on the main thread (the game's own work), on the render thread
  (preparing the picture, including the wait for the window's next image, shown in brackets:
  with vertical sync that wait is the display's time, not work) and on the graphics card
  (the render passes it times, from GPU timestamps), each the mean over half a second. A
  frame held up by the processor has a long main time, or a long render time beside a short
  wait; the GPU time is what tells a frame held up by the graphics card. The GPU part needs
  this row and the counter on when the game starts (it takes effect at the next start), and
  a graphics card that offers timestamps; without them it is left out. Unless a measuring
  aid also uses them, the timestamps are taken only while the row shows.
- Registered in `engine/nvforever/src/settings.rs`

### `fps-counter-cpu`: Counter: CPU use

- On the page: Machine and game CPU use, read once a second. Game use is cores kept busy:
  1.0 is one full core.
- In full: Machine and game CPU use, read once a second while shown. Windows uses
  performance counters; Linux uses /proc. Missing machine readings say Not available. Game
  use is cores kept busy: 1.0 is one full core.
- More: docs/design/settings-page.md (Performance usage availability)
- Registered in `engine/nvforever/src/settings.rs`

### `fps-counter-gpu`: Counter: GPU use

- On the page: GPU use, read once a second: on Windows the busiest engine across programs;
  on Linux the whole device's, or the game's own where the driver tells.
- In full: GPU use, read once a second while shown. Windows: busiest engine across programs.
  Linux: NVIDIA NVML, then amdgpu, then DRM client time. DRM reads game use and says game;
  the other Linux sources read whole-device use. Missing readings say Not available.
- More: docs/design/settings-page.md (Performance usage availability)
- Registered in `engine/nvforever/src/settings.rs`

### `fps-counter-vram`: Counter: GPU memory

- On the page: The graphics memory the game holds against the driver's budget, read every 2
  seconds. Marked ! while near it the game keeps fewer unused assets.
- In full: The graphics memory the game holds against the budget the driver gives it
  (Vulkan's memory budget, read every 2 seconds). It is the driver's figure: it counts the
  blocks kept empty for reuse, so it is what the game holds, not the size of what is drawn.
  Left out on a graphics chip that shares the machine's RAM, which has no budget of its own.
  Marked ! while the game keeps fewer unused models and textures because it is near the
  budget.
- Registered in `engine/nvforever/src/settings.rs`

### `fps-counter-ram`: Counter: RAM

- On the page: The game's RAM and the machine's RAM in use. Marked ! while under a tenth is
  free and the game keeps fewer unused assets. Windows only.
- In full: The game's memory in RAM (its working set, from Windows once a second while the
  counter shows) and the share of the machine's RAM in use (read every 2 seconds). Marked !
  while the game keeps fewer unused models and textures because the machine has under a
  tenth of its RAM free. Windows only.
- Registered in `engine/nvforever/src/settings.rs`

### `fps-counter-loaded`: Counter: loaded

- On the page: What the game holds now: the cells in the physics world, placed objects with
  a model, actors with a skeleton and engine entities.
- In full: What the game holds now: the cells loaded in the physics world, the placed
  objects with a model, the actors with a skeleton and every entity of the engine, counted
  twice a second while the row shows (the stress tests' counts).
- Registered in `engine/nvforever/src/settings.rs`

### `fps-counter-upscaling`: Counter: upscaling

- On the page: The upscaler drawing now: its name and mode, and the sizes the world is drawn
  at and rebuilt to. Off and the window's size while nothing upscales.
- In full: The upscaler that draws now: its name and mode, the size the world is drawn at
  and the size it is rebuilt to (with FSR 3.1 from its mode, with DLSS the size DLSS chose),
  and with DLSS the model asked for. Reads Off and the window's size while nothing upscales,
  also when the upscaler asked for cannot run on this computer. Follows a change within a
  frame or two, showing the upscaler without sizes until then.
- Registered in `engine/nvforever/src/settings.rs`

### `fps-counter-low`: Counter: last figure

- On the page: The frame rate row's last figure, over 10 seconds: 1% low (the mean of the
  slowest hundredth of frames) or the slowest frame.
- In full: The frame rate row's last figure, over the last 10 seconds: 1% low, the mean of
  the slowest hundredth of the frames; Slowest frame, the single slowest.
- Registered in `engine/nvforever/src/settings.rs`

## Audio › Volume

### `volume-master`: Master

- On the page: How loud all sound is: each volume below is a part of this one.
- Registered in `engine/nvf-play/src/audio_out.rs`

### `volume-music`: Music

- On the page: The music of the place you are in and the main menu's theme.
- Registered in `engine/nvf-play/src/audio_out.rs`

### `volume-footstep`: Footstep

- On the page: Footsteps, jumps and landings, yours and everyone's.
- Registered in `engine/nvf-play/src/audio_out.rs`

### `volume-voice`: Voice

- On the page: Spoken lines: conversations and what people say around you.
- Registered in `engine/nvf-play/src/audio_out.rs`

### `volume-effects`: Effects

- On the page: Every other sound: weapons, impacts, menus, doors and the sounds of places.
- Registered in `engine/nvf-play/src/audio_out.rs`

### `volume-radio`: Radio

- On the page: Radio stations, from the Pip-Boy and from radios in the world.
- Registered in `engine/nvf-play/src/audio_out.rs`

## Audio › Sound

### `music`: Play Music

- On the page: On: the music of the place you are in plays. Off: it is silent and goes on
  unheard.
- Command-line switches: `--no-music`
- Registered in `engine/nvf-play/src/music.rs`

### `voice`: Dialogue voice

- On the page: On: lines are spoken. Off: they are silent from the next line on, and still
  last as long as their sound.
- Command-line switches: `--no-voice`
- Registered in `engine/nvf-play/src/voice.rs`

### `reverb`: Reverb

- On the page: World sounds echo as the place you are in does (a cave, a hall, a small
  room). Off: they do not.
- Command-line switches: `--no-reverb`
- Registered in `engine/nvf-play/src/audio_out.rs`

### `voice-reverb`: Voice reverb

- On the page: What people say around you echoes in the place you are in, as Reverb does for
  world sounds. Conversations stay dry.
- In full: What people say around you echoes in the place you are in, up to the end of each
  line, as Reverb does for world sounds. Conversations stay dry.
- Registered in `engine/nvf-play/src/audio_out.rs`

## Controls › Input

The Controls tab's own rows (Input Device, Automatic Input Switching and Rumble here; Invert
Y and the sensitivities under Look; the controls under Movement, Combat, Actions and
Hotkeys) have no key in `[Settings]`; each entry says where NVForever.ini keeps it. They are
in `engine/nvf-play/src/pause.rs` (`row_note`, `AUTOMATIC_INPUT_NOTE`) and
`engine/nvf-play/src/bindings.rs`; docs/design/controller.md, docs/design/settings-page.md
§4. None of these rows has a command-line switch.

### Input Device

- On the page: Enter or a click: work the controls with the keyboard and mouse or with a
  controller. Kept as "Input Device" in [Controls] of NVForever.ini.
- In full: with a controller the left stick moves, the right stick looks, Start opens the
  pause menu, and in menus the D-pad, A and B are the arrow keys, Enter and Esc.

### `button-style`: Button style

- On the page: The names and pictures of the controller's buttons in prompts and hints.
  Auto: a PlayStation's when the controller says it is one, else an Xbox's.
- In full: Which names and pictures the controller's buttons go by in prompts, menu hints
  and the Controls tab. Auto: a PlayStation controller's (Cross, Circle, Square, Triangle,
  L1, R2, Select…) when the controller in use says it is one, else an Xbox controller's.
  Xbox or PlayStation: always those. The INI keeps the Xbox names either way.
- Registered in `engine/nvf-play/src/pad.rs`

### Automatic Input Switching

- On the page: On: a controller button, stick or trigger selects Controller; a key or mouse
  click selects Keyboard and Mouse (mouse motion does not). Off: choose Input Device yourself.
- In full: a controller button, or a stick or trigger past its dead zone, selects Controller;
  a key or mouse click selects Keyboard and Mouse. First release the current device's buttons
  and centre its sticks and triggers, then press again to switch on a later frame. Only the
  switching press is ignored; later holds and repeats and all releases work. Mouse motion does
  not switch. Off: choose Input Device by hand. Kept in [Controls] of NVForever.ini.

### Rumble

- On the page: A or a click: the controller shakes when you fire, take damage, land a hard
  fall or an explosion goes off near you. Kept as "Rumble" in [Controller].
- In full: harder for a heavier weapon. Kept in [Controller] of NVForever.ini.

## Controls › Look

### Invert Y

- On the page: Enter or a click: look up and down the other way round (the mouse and the
  right stick). Kept as "Invert Y" in [Controls].
- In full: kept in [Controls] of NVForever.ini.

### Mouse Sensitivity / Stick Sensitivity

- On the page: Left/Right or a click on the meter: how far the view turns as the mouse moves
  (how fast the right stick turns it), between the least and the most, as a part of the
  game's. Kept as "Mouse Sensitivity" in [Controls] ("Stick Sensitivity" in [Controller]).
- In full: a step is about a tenth. The mouse's value is kept as radians a mouse count.

### Vertical Sensitivity

- On the page: Left/Right or a click on the meter: how far (how fast, with the right stick)
  the view turns up and down, as a part of how it turns left and right.
- In full: 100% is the same as left and right, as the game has it. Kept as "Mouse Vertical
  Sensitivity" in [Controls] and "Stick Vertical Sensitivity" in [Controller] of
  NVForever.ini.

### Aim Sensitivity

- On the page: Left/Right or a click on the meter: how far the view turns as the mouse moves
  (how fast the right stick turns it) while you aim down the sights or through a scope,
  between the least and the most. 100%: as without aiming.
- In full: each device's own, on the same steps as the sensitivities (5% to 300%). It
  follows the aim's zoom: the chosen speed once the view is zoomed in (down the sights, or a
  scope up), the unzoomed look with no zoom, in step while the view zooms in or out. Aiming
  with a gun that does not zoom (no Sight FOV), blocking with a melee weapon, and the hip look
  are left as they are. Kept as "Mouse Aim Sensitivity" in [Controls] and "Stick Aim Sensitivity" in
  [Controller] of NVForever.ini [design: owner 2026-11-15].

## Controls › Aim Assist

### `aim-assist`: Aim assist

- On the page: Controller only. On: the right stick turns slower over a nearby hostile, and
  aiming down the sights pulls the view onto one near the crosshair.
- In full: Controller only, never the mouse. On: the right stick turns slower while the
  crosshair is on a nearby hostile, and aiming down the sights turns the view onto one near
  the crosshair for a moment. Off: neither.
- Registered in `engine/nvf-play/src/aim_assist.rs`

### `aim-slowdown`: Aim slowdown

- On the page: How much slower the right stick turns while the crosshair is on a nearby
  hostile (0: not at all).
- Registered in `engine/nvf-play/src/aim_assist.rs`

### `aim-lock`: Aim lock-on

- On the page: How hard aiming down the sights turns the view onto a hostile near the
  crosshair (0: not at all). Push the right stick away to break it.
- Registered in `engine/nvf-play/src/aim_assist.rs`

## Controls › Movement, Combat, Actions and Hotkeys

Each control is one of the Controls tab's own rows (`controls:bind:` and its name in
`SECTIONS`), under Movement, Combat, Actions or Hotkeys, in that order. The two entries
below cover them all.

### A control (keyboard and mouse)

- On the page: Enter or a click: bind a new key or mouse button to the control (a control
  that had it gets the old one). Delete clears its key.
- In full: a key replaces the control's key, a mouse button its mouse button. Kept under the
  control's name in [Controls] of NVForever.ini. Sprint (NVForever's own, at the end of
  Movement) is greyed and cannot be rebound while its NVF Features › Advanced Movement switch
  is off; its note says so. It has no key until the player binds one, and Run keeps Left
  Shift whether Sprint is on or off; a key bound to Sprint that another control has is
  swapped as for any two controls (`sprint` under NVF Features › Gameplay below; #3000, the
  maintainer's decision on #3002).

### A control (controller)

- On the page: A or a click: bind a new controller button to the control (a control that had
  it gets the old one). X or Delete clears it.
- In full: kept under the control's name in [Controller] of NVForever.ini.

## Graphics › Quality

### `quality`: Quality preset

- On the page: Sets the rows the quality presets cover to the game folder's low.ini to
  VeryHigh.ini, and Smooth terrain (Off at Low and Medium). Custom: none match.
- In full: Sets the rows the original's quality presets cover to the values of the game
  folder's low.ini, medium.ini, high.ini or VeryHigh.ini: Sun shadows, Shadow filtering,
  Light glow, Mirrored reflections and the object, actor, item, shadow and grass
  distances. It also sets Smooth terrain, a row of NVForever's own that no preset file
  names [design, #2864]: Off at Low and Medium (on an integrated GPU 4 x 4 costs about 9 ms
  of a 73 ms frame), 4 x 4, the default, at High and Very High; like the row itself the
  change applies at the next start. Custom: those rows match none of them, Smooth terrain
  included. A preset whose file is missing is not offered. A fresh install's default stays
  4 x 4 whatever the Quality row reads. The effects, anti-aliasing and the upscaler keep
  their own rows (the Enhanced effects row is gone since the menu overhaul).
- More: docs/spec-notes/quality-presets.md
- Registered in `engine/nvforever/src/quality.rs`

## Graphics › Shadows

### `sun-shadows`: Sun shadows

- On the page: The sun's shadows: Off, Performance (cheaper outdoors; moving shadows update
  at half the frame rate) or Quality. The original casts none from the world.
- In full: The sun's shadows, out to --shadow-distance: Off; Performance (the shadow maps
  drawn every other frame in three cascades, the farthest every fourth, trees casting
  shadows only near you, the first-person arms taking the world's shadows: saves frame time
  outdoors, shadows of moving things update at half the frame rate); or Quality (four
  cascades, the farther ones drawn less often). The original casts none from the world.
  Quality by default in the game.
- Shown as a list since the menu overhaul; an older slider number in the INI still loads as
  the nearest choice.
- Command-line switches: `--shadow-distance`, `--sun-shadows`, `--no-sun-shadows`
- Registered in `engine/nvforever/src/settings.rs`

### `shadow-filter`: Shadow filtering

- On the page: How the sun's shadow edges are softened: Fast (harder, cheapest), Smooth (as
  vanilla) or Temporal (grainy unless Anti-aliasing is TAA).
- In full: How the edges of the sun's shadows are softened: Fast (hardware 2x2, harder
  edges, cheapest), Smooth (as vanilla) or Temporal (soft but grainy unless Anti-aliasing is
  TAA).
- Shown as a list since the menu overhaul; an older slider number in the INI still loads as
  the nearest choice.
- Registered in `engine/nvforever/src/settings.rs`

### `shadow-resolution`: Shadow resolution

- On the page: Size of the sun's and the flashlight's shadow maps: 4096 draws sharper
  shadows than 2048. It takes more video memory and a little frame rate.
- In full: The side of the shadow maps of the directional lights and the shadowed spot
  lights, in texels: 2048 (the default) or 4096, changed while the game runs. They share
  one texture: the sun's cascades, the first-person arms' shade light (on Quality) and the
  flashlight while it is on. On Sun shadows Quality the cascades are laid out for the size:
  with 4096 and Longer sun shadows they end at 1,500, 3,500 and 7,000 of the default 8,000
  units (blended over a wider band) and stay sharp near the player; with 4096 alone at
  1,500, 2,400 and 4,800; with 2048 nearer (700, 2,000 and 5,000). By arithmetic 4096
  takes about 240 MiB more video memory on Quality by day in first person (five layers of 64 MiB against 16 MiB),
  144 MiB more on Performance, and 48 MiB more again with the flashlight on. No quality
  preset sets this row.
- A list of its two choices, kept in the INI by name (2048 or 4096); an older slider number
  in the INI still loads as the nearest choice.
- Registered in `engine/nvforever/src/settings.rs`

### `shadow-distance`: Shadow distance

- On the page: How far from you the sun casts shadows, as a part of the farthest shadow
  distance; 0%: none.
- Shown as a list since the menu overhaul; an older slider number in the INI still loads as
  the nearest choice. Its choices: 0% to 100% in 20% steps; 0% casts no sun shadows.
- Registered in `engine/nvforever/src/settings.rs`

### `fine-shadows`: Longer sun shadows

- On the page: On: the sun's shadows reach half as far again as the shadow distance. Costs a
  little frame rate.
- Command-line switches: `--shadow-distance`, `--graphics`
- More: docs/design/graphics-enhanced.md
- Registered in `engine/nvforever/src/settings.rs`

### `cloud-shadows`: Cloud shadows (experimental)

- On the page: On: the weather's clouds cast soft shadows that drift over the land, only
  where the cover is heavier than around it. Still being tuned. Not in the original.
- In full: The weather's cloud layers, as the sky draws them (both weathers during a
  change), are composed into one transmittance image that darkens the sun's (and at night
  the moon's) direct light where it falls, projected along the light from a plane high
  above the ground and drifting with the wind. The image is relative to its own clearest
  part (its 90th percentile reads fully lit), so even cover, an overcast sky, casts nothing
  and only the patches heavier than the rest darken. Ambient light, lamps, fog and
  reflections are left alone. Experimental: off by default and in every preset; its
  strength, size, height and the relative rule's reference are not calibrated yet. Switching it rebuilds the world's material pipelines (Continue waits for
  them). Works with Sun shadows off too.
- Command-line switches: `--graphics`, `--graphics-only`
- More: docs/design/sun-visibility.md
- Registered in `engine/nvforever/src/settings.rs`

### `terrain-shadows`: Terrain shadows (experimental)

- On the page: On: at a low sun, hills and mountains shade the land beyond the reach of the
  sun's shadows. Still being tuned. Not in the original.
- In full: A height map of the land around you (32 by 32 cells, 256 units a texel) is
  marched towards the sun on the GPU into a shadow-height map, which darkens the sun's (and
  at night the moon's) direct light on everything below a ridge's shadow, softened with the
  ridge's distance. Near you the sun shadows already show the land's own shadows, and the
  darker of the two counts; beyond their reach the terrain's shade alone, and object
  shadows end where the sun shadows end, as with the row off. Ambient light,
  lamps, fog and reflections are left alone. Experimental: off by default and in every
  preset; its softness and its bias against speckle on slopes are not tuned yet. Switching
  it rebuilds the world's material pipelines (Continue waits for them). Works with Sun
  shadows off too.
- Command-line switches: `--graphics`, `--graphics-only`
- More: docs/design/terrain-shadows.md, docs/design/sun-visibility.md
### `light-shadows`: ALCS (experimental)

- On the page: ALCS (All Lights Cast Shadows): lamps, fires and spot lights cast shadows
  in the shadow distance; Performance casts fewer. Costs frame rate. Not in the original.
- In full: ALCS stands for All Lights Cast Shadows; the row was called ALC shadows until
  2026-11-24 and Lamp shadows until 2026-11-19. The INI key `light-shadows`, the switch
  `--light-shadows` and the log lines starting `light shadows:` keep the old name, so saved
  settings, scripts and the smoke scenarios go on working. Off (the default), Performance
  or Quality, as Sun shadows. Every drawn placed light whose light reaches within the sun's
  shadow distance (the Shadow distance and Longer sun shadows rows) casts, nearest first,
  up to 32 lights on Quality and 16 on Performance; grass fires' lights and lights a script
  attached count among them. Muzzle flashes and explosions cast too, apart from them: the
  nearest whose light reaches within 3,000 units (about 43 m) and into view, 2 at once on
  Quality and 1 on Performance, each drawn in the frame it appears. A light's maps are
  drawn when it starts casting and then kept; a part of them is drawn again in the frame
  something there moved more than about 7 mm (a shape, or any joint of an actor), appeared
  or vanished, all of them once the light itself moved that far. Up to 8 lights redraw in
  a frame on Quality, 3 on Performance, those that waited longest first, so a moving
  thing's shadow moves with it. The world and your body cast, the first-person arms do
  not; what lies within 8 units of a light casts nothing from it, and lamp housings, shades
  and bulbs cast none from their own light. Quality also gives the point lights' shadows
  soft edges (sharp where a thing touches the surface, a little softer away from it, by the
  lamp's size: a candle's or a bare bulb's crisper than a shade's or a chandelier's; never
  so wide that a shadow loses its shape) and, while TAA,
  FSR or DLSS is on, small contact shadows where things touch; Performance smooths the
  edges by a fixed width. A point light's maps are in two sizes on Quality: the 8 lights
  nearest the camera take 1024², the rest 512²; a light changes size as you move, drawn anew
  in that frame. On Performance all are 512² (1024² with Shadow resolution 4096). The video
  memory is logged at the start (`light shadows: … cast`): about 0.7 GB with 28 lights on
  Quality. Under a loading screen every casting
  light's maps are drawn before play, those in view first, within 3 seconds. The flashlight
  casts as before.
  A lamp's shadow weighs 1.1: where a lamp is blocked in full the place loses that lamp's
  light and a tenth of it more, off the cell's ambient light and the other lamps there
  (light_shadows.rs `SHADOW_STRENGTH`).
  The original's placed lights cast none [design].
- A list kept in the INI by name (`Off`, `Performance`, `Quality`).
- Command-line switches: `--light-shadows`
- More: docs/design/graphics-enhanced.md §3.17
- Registered in `engine/nvforever/src/settings.rs`

## Graphics › Lighting and Effects

### `ambient-occlusion`: Ambient occlusion

- On the page: Creases, corners and the ground under things darken by how enclosed they are:
  Off, Performance (cheaper, a little grainier) or Quality. Not in the original.
- In full: Screen-space ambient occlusion from the depth, on the ambient light, on the lamps
  that cast no shadow and by half on those that do (the flashlight, the ALC shadows row's
  lamps), and on the lamps' highlights (a shiny surface no longer glints in a crease as in the
  open): Off; Performance (one slice direction a pixel instead of two: half the horizon march,
  the same denoise); or Quality (ground-truth ambient occlusion, 2 slices x 4 steps a side at
  half resolution, depth-aware denoise). Under TAA, FSR or DLSS its sample pattern moves each
  frame, so what the denoise leaves on leaves and outlines is averaged away. Quality in the
  enhanced look.
- Shown as a list since 2026-11-19; an INI written while it was an on/off switch (`1`/`0`,
  true/false, on/off, yes/no) still loads as Quality or Off.
- Command-line switches: `--graphics`, `--ambient-occlusion`
- More: docs/design/graphics-enhanced.md
- Registered in `engine/nvforever/src/settings.rs`

### `volumetric-clouds`: Volumetric clouds (experimental)

- On the page: The weather's clouds drawn with depth and lit by the sun: Off, or
  Performance, Normal or High. A prototype. Not in the original.
- In full: The weather's own cloud layers, as Cloud shadows composes them, become a
  layer of cloud with thickness above the world, worked out per pixel: lit by the weather's
  sunlight (by night its night light, from the moon), bright toward the sun, darker
  underneath, coloured by the weather's cloud colours, fading into the sky toward the
  horizon. Thick cloud hides the sun and the moon. The clouds follow the weather's cover,
  drift and transitions, and Cloud shadows on the ground, where that row is on, are theirs while they
  are drawn. High draws them into a buffer of two
  fifths of the render size, Normal a third, Performance a quarter; Normal and Performance
  also look for cloud along each ray at twice the spacing. A prototype. The water's
  reflection, the scope's lens and a mod's extra cameras keep the weather's flat clouds while
  it is drawn. With Cloud shadows on, switching it rebuilds the world's material pipelines.
  Off in every preset; not in the original. Outdoors only.
- Shown as a list since 2026-11-23; an INI written while it was an on/off switch (`1`/`0`,
  true/false, on/off, yes/no) still loads as Normal or Off.
- Command-line switches: `--graphics-only` (`volumetric-clouds` is Normal;
  `volumetric-clouds-performance`, `volumetric-clouds-high`)
- More: docs/design/graphics-enhanced.md §3.23, docs/design/volumetric-medium.md
- Registered in `engine/nvforever/src/settings.rs`

### `light-shafts`: Sun shafts

- On the page: On: sunlight streams past what stands in front of the sun, stronger the
  hazier the weather's fog. Not in the original.
- Command-line switches: `--graphics`
- More: docs/design/graphics-enhanced.md
- Registered in `engine/nvforever/src/settings.rs`

### `heat-haze`: Heat haze (experimental)

- On the page: On: the far ground near the horizon shimmers slightly on hot, clear days, by
  the sun and the weather. A prototype. Not in the original.
- In full: On: beyond about 6,000 units, close to the horizon, the picture is moved a pixel
  or two up and down by a slowly rising pattern that drifts with the wind; as strong as the
  sun is high (a little stronger in the afternoon) and the weather's sunlight bright, its
  air calm and free of haze; none at night, indoors or in rain or snow. The base game's
  overcast is not told from clear yet. Near things, the weapon and the HUD stay sharp. Off
  in both presets; not in the original.
- Command-line switches: `--graphics-only`

### `desert-haze`: Desert haze (experimental)

- On the page: On: a thin dust haze thickens with distance and toward the ground, warmer
  toward the sun, as far as the weather's fog allows. Not in the original.
- In full: On: a dust haze is added to the weather's own distance fog, worked out per
  pixel in the fog itself: thicker with distance and toward the ground under the camera,
  warmer looking into the sun by day. A clear weather gets all of it, a short or dense fog
  (a hazy weather, a dust storm) none; a weather with a very far fog a thinner one. Off in
  every preset; not in the original. Outdoors only.
- Command-line switches: `--graphics`, `--graphics-only`
- More: docs/design/graphics-enhanced.md §3.16
- Registered in `engine/nvforever/src/settings.rs`

### `aerial-perspective`: Aerial perspective (experimental)

- On the page: On: far land takes on the sky's colours with distance, bluer at noon, warmer
  at dusk, brighter toward the sun, inside the weather's fog. Not in the original.
- In full: On: the weather's own distance fog fades far land toward the sky colour behind it
  (its horizon and lower-sky colours) instead of the one fog colour, each colour channel at
  its own pace set by the sky's hue (a blue sky fogs blue in first, a dusk sky warm), and
  toward the weather's sun colour looking into the sun by day. The amount of fog stays the
  weather's; a short or dense fog (a hazy weather, a dust storm) is left as it is. Off in
  every preset; not in the original. Outdoors only.
- Command-line switches: `--graphics`, `--graphics-only`
- More: docs/design/graphics-enhanced.md §3.18
- Registered in `engine/nvforever/src/settings.rs`

### `night-sky-detail`: Night sky detail (experimental)

- On the page: On: stars twinkle a little, more near the horizon; the moon gets a glow and
  lights the clouds facing it, wider in hazy weather. Not in the original.
- In full: On: each star's light rises and falls a little, smoothly, one to four times a
  second, hardly at all overhead and most near the horizon, a little more in hazier air; a
  soft glow surrounds the moon, in the weather's night light colour, wider and a little
  stronger the hazier the weather's fog, and the clouds near the moon take some of its light.
  The glow and the moonlit clouds follow the moon's phase (none at the new moon) and show only
  while the moon does; nothing by day or indoors. Built from the climate's own stars and
  moon and the weather's own colours and fog, so mods' skies work too. Off in every preset;
  not in the original.
- Command-line switches: `--graphics`, `--graphics-only`
- More: docs/design/graphics-enhanced.md §3.19
- Registered in `engine/nvforever/src/settings.rs`

### `volumetric-light`: Volumetric light (experimental)

- On the page: On: sunlight and moonlight show in the air as rays and a glow toward a low
  sun. An addition of NVForever's.
- While it is on at a strength above 0, Sun shafts are greyed on the game's page (they give way to it where
  it draws). A prototype. Outdoors only, by the sun by day and by
  the moon at night (nothing at night in a climate without one), in the main view only. Best
  with Cloud shadows and Terrain shadows on. Off in every preset; not in the original.
- Command-line switches: `--graphics-only`
- More: docs/design/volumetric-medium.md
- Registered in `engine/nvforever/src/settings.rs`

### `volumetric-light-strength`: Volumetric light strength

- On the page: How strongly the light in the air brightens lit air and darkens shaded air:
  100% is the look as tuned, 0% none.
- 0 % to 200 % in steps of 10 %, 100 % by default; greyed while Volumetric light is off.
- Registered in `engine/nvforever/src/settings.rs`

### `physical-pbr`: Physical lighting (experimental)

- On the page: On: light, sky, fog and exposure follow physical rules instead of the
  original's display values. Still being tuned. Takes effect at the next start. Not in the
  original.
- In full: The world is lit with Oren-Nayar diffuse and Cook-Torrance GGX specular with
  Schlick Fresnel, lamps fall off with the square of the distance, the sky lights everything
  as image-based light, the game's fog fades into the physical sky's horizon, the picture
  tonemaps with Tony McMapface, and exposure and bloom are tuned for linear light. Read at the
  start: the row changes the next start's lighting. Off by default and in every preset;
  the original's display-value lighting stays the default look. The volumetric clouds and
  Volumetric light run with it, lit by the same sun and sky. With a material pack and
  Reflections on authored materials, the pack's materials keep their reflections and other
  surfaces take flat ambient light in place of the sky's.
- Command-line switches: `--graphics-only`
- More: docs/design/graphics-enhanced.md §3.25

### `tonemap-preset`: Tonemapping preset

- On the page: A look laid on the tonemapper: exposure, contrast and highlight roll-off.
  Default keeps the shipped look. Presets are files in the Tonemapping folder of
  NVForever's home. Not in the original.
- In full: Default and the presets found in `<NVForever home>\Tonemapping\` at the start,
  one TOML file each (docs/design/tonemapping-presets.md has the format). The pick takes
  effect at once, in either lighting mode and with HDR output, and is kept by its name; a
  name no longer on disk falls back to Default. The eye adaptation still applies on top.
  HDR output's calibration stays the display's own.
- Command-line switches: none
- More: docs/design/tonemapping-presets.md (#4690)
### `bloom`: Bloom

- On the page: On: light brighter than a sunlit white wall (the sun, lamps, windows) glows
  into its surroundings, on top of the game's own image space.
- Command-line switches: `--graphics`
- More: docs/design/graphics-enhanced.md
- Registered in `engine/nvforever/src/settings.rs`

### `grade`: Colour grade

- On the page: On: the picture is graded by the worldspace's, the weather's and the cell's
  image space. Off: the picture as lit, for comparisons.
- Command-line switches: `--no-grade`
- Registered in `engine/nvforever/src/settings.rs`

### `glow`: Light glow (HDR)

- On the page: On: neon, lamps, beams and fire glow softly, as the original's HDR glow does,
  as much as the place's image space asks. Bloom adds to it.
- In full: On: neon, lamps, glowing beams and fire glow softly into their surroundings, as
  the original's HDR glow does; how much follows the place's image space (the Strip's neon
  at night most). Bloom, when on, adds to it.
- Command-line switches: `--graphics-only`
- More: docs/design/graphics-enhanced.md
- Registered in `engine/nvforever/src/settings.rs`

### `eye-adaptation`: Eye adaptation

- On the page: On: dark views brighten after a moment, as the eye gets used to the dark, by
  the place's image space. Needs the light glow.
- Command-line switches: `--graphics-only`
- More: docs/design/graphics-enhanced.md
- Registered in `engine/nvforever/src/settings.rs`

### `neon-glow`: Neon glow

- On the page: On: where the image space asks for a strong glow (the Strip at night) neon
  burns with a wide halo, as in the original. Needs the light glow.
- In full: On: where the place's image space asks for a strong glow (the Strip at night)
  neon tubes and signs burn with a wide halo, as in the original. Needs the light glow.
- Command-line switches: `--graphics-only`
- More: docs/design/graphics-enhanced.md
- Registered in `engine/nvforever/src/settings.rs`

## Graphics › Water

### `water-reflections`: Screen-space reflections

- On the page: On: water the mirrored reflection does not reach (another height, interiors)
  reflects what is on the screen instead of only the sky.
- Command-line switches: `--graphics`
- More: docs/design/graphics-enhanced.md
- Registered in `engine/nvforever/src/settings.rs`

### `water-mirror`: Mirrored reflections

- On the page: On: water mirrors the world above it, as in the original, drawing the scene a
  second time. Off: it reflects the sky.
- In full: On: water mirrors the world above it, as in the original; the scene is drawn a
  second time for it while water is on screen. Off: water reflects the sky, and what is on
  the screen with screen-space reflections.
- Command-line switches: `--graphics-only`
- More: docs/design/graphics-enhanced.md
- Registered in `engine/nvforever/src/settings.rs`

### `water-effects`: Enhanced water effects

Sunlight shifts across shallow beds. Deeper water looks murkier from shore and
underwater, using each water type's own colours and fog distances. Selected river
stretches drift downstream, and patches of foam lap at the shore. Shots and
near-surface explosions send out waves from their points of contact. The default
graphics preset leaves the switch off. Enhanced turns it on.

- Command-line switches: `--graphics`, `--graphics-only water-effects`
- More: docs/design/graphics-enhanced.md
- Registered in `engine/nvforever/src/settings.rs`

## Graphics › Detail and Distance

### `distant-lod`: Distant terrain and objects

- On the page: On: distant land, buildings and trees are drawn beyond the loaded cells
  (outdoors). Off: the world ends at the loaded cells.
- Command-line switches: `--no-lod`
- Registered in `engine/nvforever/src/settings.rs`

### `full-detail`: Full-detail scenery

- On the page: Draws buildings, rocks and cliffs out to 7 x 7 or 9 x 9 cells as full models,
  not distant forms. Costs GPU time. Not in the original.
- In full: [design, #2701] A list: Off, 7 x 7, 9 x 9; Off by default and in the vanilla
  preset, 7 x 7 in the enhanced preset (maintainer decision 2026-11-21, #3048).
  Outdoors, the full models of statics, static collections, moveable statics and trees,
  and of every other reference flagged visible when distant, are drawn in the cells 3
  (7 x 7) or 4 (9 x 9) grid steps from yours, with their landscape at full resolution,
  where the distant LOD blocks stood. Draw-only: the game still loads the 5 x 5 cells it
  always does, so scripts, AI, physics, triggers and saves see no difference. Switches in
  play: cells out of the new reach fade out to the LOD, new ones stream in. Kept in the
  INI as `Off`, `7 x 7` or `9 x 9`.
- Command-line switches: `--graphics-only` (`--graphics-only full-detail-7x7`,
  `full-detail-9x9`)
- More: docs/design/graphics-enhanced.md §3.20
- Registered in `engine/nvforever/src/settings.rs`

### `distant-objects`: Distant objects

- On the page: Draws the rocks, shacks, pylons and signs the distant LOD leaves out, up to
  8 or 12 cells away. Costs GPU time. Not in the original.
- In full: [design, #3014] A list: Off, 8 cells, 12 cells; Off by default and in both
  presets. Outdoors, beyond the loaded cells and the full-detail scenery, statics and
  static collections the distant LOD blocks do not hold are drawn out to 8 or 12 grid
  steps from yours: from their own `_lod` model when the load order has one beside the
  full model (as LOD resource packs supply), else from the full model when it is large
  for its distance. References a block may hold (flagged visible when distant) are left
  to it. Draw-only: no collision, scripts or AI. Greyed while Distant terrain and objects
  is off. Switches in play. Kept in the INI as `Off`, `8 cells` or `12 cells`.
- Command-line switches: `--graphics-only` (`--graphics-only distant-objects-8`,
  `distant-objects-12`)
- More: docs/design/graphics-enhanced.md §3.21
- Registered in `engine/nvforever/src/settings.rs`

### `grass`: Grass

- On the page: On: grass grows on the landscape; the cells around you get theirs over the
  next frames. Off: it is hidden and none grows on new cells.
- Command-line switches: `--no-grass`
- Registered in `engine/nvforever/src/settings.rs`

### `anisotropic`: Anisotropic filtering 16x

- On the page: On: textures seen at a slant stay sharp further (16x instead of 8x). For
  textures loaded from now on (the cells loaded next).
- Command-line switches: `--graphics`
- More: docs/design/graphics-enhanced.md
- Registered in `engine/nvforever/src/settings.rs`

### `authored-materials`: Authored materials

- On the page: On: a material pack's maps (textures\pbr\) shade its surfaces. Off: the
  game's own values. Takes effect at the next start.
- In full: [design, #2981] A texture pack may give a game texture a set of material maps
  under `textures\pbr\` (occlusion, roughness and metallic in `_orm`, a normal map in `_n`,
  scalars in a `.toml`); the shapes drawn with that texture then take their roughness,
  highlight strength, occlusion and normal from the set (docs/spec-notes/pbr-materials.md).
  Lit NIF shapes with texture coordinates take a set; unlit, additive, multiplied and
  refracting shapes, particles, hair, skin and FaceGen heads, landscape LOD and the Pip-Boy's
  and interface textures never do. A set without `_n` (or a shape whose drawn mesh has no
  tangents) keeps the game's own normal map, its alpha no longer a highlight mask. On the
  experimental height branch (#2981 stage 4) a set's height map (`_p`, also alone: a
  height-only set) is drawn as parallax on opaque, single-sided, non-decal shapes, never on
  cut-outs, blended or double-sided shapes. On in both looks: without a pack nothing
  changes. Off: the material path reads no file under
  `textures\pbr\` and every material is the game's own. Read once at the start, so a change
  applies at the next start. Not in the original.
- Command-line switches: `--graphics-only` (`--graphics-only no-authored-materials`)
- More: docs/spec-notes/pbr-materials.md
- Registered in `engine/nvforever/src/settings.rs`

### `authored-reflections`: Reflections on authored materials

- On the page: On: pack materials reflect the sky outside and nearby lights inside. Needs
  Authored materials on. Takes effect at the next start.
- In full: [design, #2981] Surfaces shaded from an authored set reflect a small cube of the
  world camera's surroundings, blurred by their roughness, and the `_orm`'s metallic channel
  takes effect. Outdoors it reflects the sky, ground and a broad glow toward the sun or
  moon. Nearby lamps can light the ground in the cube at night. Indoors it approximates
  the room with a box lit by the cell's ambient light and nearby lamps. The cube omits
  cloud shapes, the sun disc and nearby objects. A metal's base colour
  is lifted by its set's metal gain (12 by default) so it reflects
  as a metal. Only authored materials reflect: the game's own materials, terrain, trees, grass and water are
  unchanged. Nothing changes without a pack, or with Authored materials off. Off: metallic
  stays held at 0 and no cube is made. Read once at the start, so a change applies at the
  next start. Not in the original.
- Command-line switches: `--graphics-only` (`--graphics-only no-authored-reflections`)
- More: docs/spec-notes/pbr-materials.md
- Registered in `engine/nvforever/src/settings.rs`

### `terrain-height-blend`: Terrain height blending

- On the page: On: where ground textures meet, the new one shows through its brighter stones
  and clumps first instead of an even fade. Fully painted ground is unchanged.
- In full: On: where landscape texture layers meet, the change follows each texture's own
  relief, judged by its brightness against the texture's mean, instead of an even fade
  along the triangle grid. Ground painted fully, or not at all, looks the same. Off in
  both presets; not in the original.
- Command-line switches: `--graphics`, `--graphics-only`
- More: docs/design/graphics-enhanced.md §3.12
- Registered in `engine/nvforever/src/settings.rs`

### `terrain-anti-tiling`: Terrain anti-tiling

- On the page: On: ground textures stop repeating as a visible grid, for some GPU time.
  Off: each repeats every 256 units, as in the original.
- In full: On: every landscape texture (diffuse and normal map) is read at two or three
  places offset by a hash of where the ground lies in the world and blended, so its
  256-unit repeat no longer lines up into a grid; nearby the ground looks like the same
  texture. Off in both presets; not in the original. Costs extra texture reads where the
  ground is drawn (layers that do not show here are not read).
- Command-line switches: `--graphics`, `--graphics-only`
- More: docs/design/graphics-enhanced.md §3.13
- Registered in `engine/nvforever/src/settings.rs`

### `terrain-parallax`: Terrain parallax

- On the page: On: ground textures a texture pack gives height maps for (textures\pbr\)
  show depth near you, for some GPU time. Without such a pack nothing changes.
- In full: [design, #3322] On: where a texture pack gives a landscape layer's texture a
  height map (`_p` in its set under `textures\pbr\`, the format of
  docs/spec-notes/pbr-materials.md), the ground near the camera shows that relief: one
  march over the layers' heights, blended as the layers are, moves the texture coordinate
  every ground texture is read at, fading out by 2048 units; each layer moves as far as its
  own set's depth asks, and so does the base texture of a quadrant (#3846). A base or layer
  without a height map counts as flat. Not drawn while Terrain anti-tiling is on. The shadows, the depth prepass, the
  collision, the mesh terrain blend and the distant landscape keep the plain surface, and
  so do layers drawn on their own (GPUs without bindless textures; a texture repeat that
  does not divide the cell). A layer or base whose texture has no set takes the height from its
  diffuse's alpha where that is a varying DXT5 alpha, as the ground packs for the original's
  terrain parallax shaders have it (depth 0.03; read only while the switch is on, over some
  frames when it is switched on in a game). Needs
  Authored materials: in the game, as this run has it
  (greyed until a restart when it was off at the start; still available after turning it
  off for the next start); in the launcher, as saved. The original draws no relief on its
  landscape [observed: the maintainer's run of observer check 268, 2026-11-22, by eye], so this
  is an enhancement that acts only with a ground pack. Off by
  default and in both presets until its cost is measured (the maintainer's call); switches
  at once, no restart.
- Command-line switches: `--graphics-only` (`--graphics-only terrain-parallax`)
- More: docs/spec-notes/pbr-materials.md "Terrain layers: parallax"
- Registered in `engine/nvforever/src/settings.rs`

### `mesh-terrain-blend`: Blend meshes into ground

- On the page: On: softens where outdoor roads, rocks and cliffs meet the landscape, giving
  them the ground's colour and light there. Not in the original.
- In full: On: rigid outdoor statics and static collections (roads, rocks, cliffs) take the
  landscape's colour and shading over the lowest 8 units above the ground (fading out on
  faces steeper than 60 degrees, none on near-vertical ones), from a cache of the landscape
  around the camera. Off by default and in both presets; switches at once, no restart.
  The test aid `NVF_PLAY_TEST_MESH_BLEND` overrides it.
- Command-line switches: `--graphics-only` (`--graphics-only mesh-terrain-blend`)
- More: docs/design/graphics-enhanced.md §3.14
- Registered in `engine/nvforever/src/settings.rs`

### `occlusion-culling`: Occlusion culling

- On the page: On: hidden objects are skipped before drawing. Helps when the GPU limits
  the frame rate; costs some CPU. Takes effect at the next start.
- In full: [design] Bevy's two-phase GPU occlusion culling on the main camera: meshes
  hidden behind nearer ones (by last frame's depth, then this frame's) are not drawn.
  First-person shapes are never culled. [observed 2026-11-24, 4K DLSS Quality, maxed
  settings, 9950X3D + RTX 5090] Novac's GPU time 4.96 → 4.31 ms, but its frame 6.96 → 7.18
  ms, as the second culling phase costs render-thread time: it helps where the GPU holds
  the frame rate and costs a little where the CPU does (docs/design/frame-rate-audit.md
  §13). Off by default. Read once at the start, so a change applies at the next start. The
  test aid `NVF_OCCLUSION_CULLING` (`1` on, `0` off) overrides the saved row for a run.
- Registered in `engine/nvforever/src/gpu_occlusion.rs`

### `smooth-terrain`: Smooth terrain

- On the page: Smooths height steps while keeping bumps and ridges: 2 x 2 or 4 x 4 per
  landscape square, or 4 x 4 near and 2 x 2 further. Applies at the next start.
- In full: [design, #2149, #2676] A list: Off, 2 x 2, 4 x 4 and 4 x 4 near; 4 x 4 by
  default (smoothing on by the maintainer's decision of 2026-11-13; 4 × 4 and the recipe,
  detail, bound 8 units and thresholds 2:6, are the contributor's pick from in-game
  comparisons of 2026-11-15, for the maintainer to confirm). Detail evens out height steps while keeping
  bumps and ridges; 4 × 4 gives rounder silhouettes, 2 × 2 a quarter of the geometry for
  weaker machines. 4 x 4 near collides on the 4 × 4 surface and draws each loaded quadrant
  4 × 4 within 2,048 units of the camera, 2 × 2 (the same surface, every other vertex)
  beyond 2,560, with no gaps where the two meet. The surface stays within one height step
  (8 units) of the stored triangles to keep placed objects in contact (the test aids below
  allow up to 16); actors and objects stand on what is drawn. Off: the landscape's
  triangles as stored. It takes effect at the next start (loading a save starts the game
  again), so drawn ground and collision always change together. The Quality preset row
  sets it too (Off at Low and Medium, 4 x 4 at High and Very High, #2864), from the next
  start as well. The INI's older `1`/`0`
  (#2149's on/off) reads as 4 x 4 and Off. Not in the original. Kept in the INI as `off`,
  `2x2`, `4x4` or `4x4-near`. For comparing recipes, the test aids
  `NVF_PLAY_TEST_SMOOTH_MODE` (`interpolate`, `fit`, `detail`: a fit that evens out the
  height steps but keeps bumps and ridges), `NVF_PLAY_TEST_SMOOTH_SUBDIV` (`2`, `4`, over
  the row's level), `NVF_PLAY_TEST_SMOOTH_BOUND` (0 to 16 units),
  `NVF_PLAY_TEST_SMOOTH_DETAIL` (detail's thresholds, `2:6` by default) and
  `NVF_PLAY_TEST_SMOOTH_NEAR` (4 x 4 near's distance, 2048 by default) override how it
  smooths for one run without changing this row or its saved default.
- Command-line switches: `--smooth-terrain <off|2x2|4x4|4x4-near>`, `--no-smooth-terrain`
- More: docs/spec-notes/world.md §3 "Smoothing"
- Registered in `engine/nvforever/src/settings.rs` (state in `engine/nvf-play/src/smooth_terrain.rs`)

### `object-distance`: Object distance

- On the page: How far from you objects of the loaded cells (buildings, rocks, furniture,
  clutter) are drawn. Lower draws fewer and runs faster.
- In full: How far from you objects (buildings, rocks, furniture, clutter) of the loaded
  cells are drawn, as a part of the farthest those cells reach. Lower draws fewer and runs
  faster.
- Registered in `engine/nvforever/src/settings.rs`

### `actor-distance`: Actor distance

- On the page: How far from you people and creatures are drawn, as a part of the farthest
  the loaded cells reach.
- Registered in `engine/nvforever/src/settings.rs`

### `item-distance`: Item distance

- On the page: How far from you items lying in the world are drawn, as a part of the
  farthest the loaded cells reach.
- In full: How far from you items lying in the world (weapons, ammo, aid, notes, caps,
  clutter you can take) are drawn, as a part of the farthest the loaded cells reach.
- Registered in `engine/nvforever/src/settings.rs`

### `grass-distance`: Grass distance

- On the page: How far from you grass is drawn, as a part of the farthest grass distance and
  its fade range.
- Registered in `engine/nvforever/src/settings.rs`

### `tree-distance`: Tree distance

- On the page: How far from you trees beyond the loaded cells are drawn as flat pictures, at
  most as far as distant objects are drawn. Farther runs slower.
- In full: Shown as a distance in game units, in steps of 2,500 up to how far distant object
  blocks are drawn (125,000 units by default), so the billboards stop where the buildings and
  rocks around them stop; distant land reaches farther (250,000 units). It starts at 40,000
  units, as before the row.
- Command line: `--tree-distance` sets where the slider starts (40,000 units by default, at
  most `--lod-distance`; 0 draws no distant trees whatever the slider); `--lod-distance` sets
  the slider's end
- Registered in `engine/nvforever/src/settings.rs`

### `distant-lights`: Distant lights (experimental)

- On the page: On: at night, lamps in the places beyond the loaded area show as small
  glows, so towns can be seen from afar. They light nothing. Not in the original.
- In full: On: the placed lights of the cells up to 12 cells away that are not loaded, and
  the objects there that glow by themselves (lamps, glow cards, light beams, lit sign
  panels, neon), are drawn as small soft glows in their own colour, at most 400 of them,
  the nearest and brightest first, lights standing close together as one. They fade out
  toward the edge of that reach and into the weather's fog, give way to the real lights as their cells load,
  and fade away as the sun rises. A lamp that is switched off, or disabled by the game,
  shows none. Sprites only: nothing is lit and no shadow is cast. Off in both presets; not
  in the original. Outdoors only.
- Command-line switches: `--graphics`, `--graphics-only`
- More: docs/design/graphics-enhanced.md §3.22
- Registered in `engine/nvforever/src/settings.rs`

## Graphics › World and Animation

### `head-tracking`: Head tracking

- On the page: On: actors turn their heads to the player and to others near them. Off: heads
  ease back to their animation.
- Command-line switches: `--no-head-tracking`
- Registered in `engine/nvforever/src/settings.rs`

### `foot-ik`: Feet follow the ground

- On the page: On: on slopes and steps actors bend their knees and set each foot on the
  ground under it. Off: the legs play the animation as it is.
- Command-line switches: `--no-foot-ik`
- Registered in `engine/nvforever/src/settings.rs`

### `sky-clock`: Sky follows the game clock

- On the page: On: the sky shows the game's time of day as it passes. Off: the sky keeps its
  hour while the game's clock runs on ([ and ] still set it).
- Command-line switches: `--still-sky`
- Registered in `engine/nvforever/src/settings.rs`

## NVF Features › Advanced Movement

### `advanced-movement`: Global Toggle

- On the page: On: the switches below shape how your character, view and weapon move. Off: none of them acts, as in the original; each keeps its own choice.
- Command-line switches: `--no-advanced-movement`
- On by default: with the parts' own defaults only Advanced Recoil acts.
- Registered in `engine/nvf-play/src/player.rs`

### `movement-weight`: Movement Weight

- On the page: How heavy it all feels: 100% as made; lower makes speeding up, stopping and crouching quicker and cuts the extra time drawing and aiming take.
- In full: a dropdown of 25%, 50%, 75% and 100% (saved as its name; 100% by default).
  Movement Inertia's times (speeding up, gliding to a stop, crouching, standing up after a
  landing, the speed a hard landing costs) are that share of the full ones, so 25% is four
  times as quick. Of Weapon Handling's draw, put-away, aim and hand-back after a reload only
  the time over the original's shrinks, so a weapon is never quicker than the original's.
- Registered in `engine/nvf-play/src/player.rs`

## NVF Features › Gameplay

### `movement-inertia`: Movement Inertia

- On the page: On: you speed up, glide to a stop and brake before turning; crouching eases and landings dip and slow you by the fall. Off: as in the original.
- Command-line switches: `--movement-inertia`, `--no-movement-inertia`
- Off by default (the owner, 2026-11-16: every Advanced Movement part off by default but Advanced Recoil).
- Registered in `engine/nvf-play/src/player.rs`

### `weapon-handling`: Weapon Handling

- On the page: On: drawing, putting away and aiming a weapon take a moment longer, and longer still for heavier weapons. Off: the original's timing.
- Command-line switches: `--weapon-handling`, `--no-weapon-handling`
- Off by default (the owner, 2026-11-16: every Advanced Movement part off by default but Advanced Recoil).
- Registered in `engine/nvf-play/src/player.rs`

### `advanced-recoil`: Advanced Recoil

- On the page: On: a shot's kick eases the view up and part of it settles back down, and the view nudges back. Off: the whole kick stays, as in the original.
- Command-line switches: `--no-advanced-recoil`
- On by default, the one Advanced Movement part that is.
- Registered in `engine/nvf-play/src/player.rs`

### `sprint`: Sprint

- On the page: On: hold the Sprint key (set it in Controls) to run faster, spending AP every second; stopping low on AP leaves you out of breath. Off: as in the original.
- In full: the Sprint control (Controls › Movement) has no key until the player binds one,
  and is greyed and does nothing while this is off or the Global Toggle is; Run keeps Left
  Shift whatever this is (#3000, the maintainer's decision on #3002). A full sprint is
  1.45 times the run speed, walking (Always Run off) too, and costs 10 AP a second (none
  come back meanwhile; it ends at 0 AP); forward only, half a sprint diagonally; aiming,
  attacking, reloading, sneaking, lying down,
  over-encumbered or in water stop it, eased out. Stopping under 20 % AP leaves you out of
  breath, and sprinting the view sways a little to the sides (both shown by Camera Motion).
  While sprinting and while the AP comes back after it, a quiet heartbeat beats: slower under
  40 % AP, faster under 20 % (the low-health heartbeat wins). The speed and AP perks apply to the sprint
  (docs/design/player-body.md §1.4a.4).
- Command-line switches: `--sprint`, `--no-sprint`
- Off by default.
- Registered in `engine/nvf-play/src/player.rs`

## NVF Features › Visual Only

### `weapon-inertia`: Weapon Inertia

- On the page: On: in first person the arms and weapon lag behind turns and steps, bob, dip, breathe and recoil, heavier weapons more. Off: they stay still.
- Command-line switches: `--weapon-sway`, `--no-weapon-sway` (a saved `weapon-sway` is read as this row)
- Off by default (the owner, 2026-11-16: every Advanced Movement part off by default but Advanced Recoil).
- Registered in `engine/nvforever/src/settings.rs`

### `animation-smoothing`: Animation Smoothing

- On the page: On: in first person drawing, aiming, attacks, reloads and the Pip-Boy ease in and settle softly, taking no longer. Off: the original's clips.
- Command-line switches: `--animation-smoothing`, `--no-animation-smoothing`
- Off by default (the owner, 2026-11-16: every Advanced Movement part off by default but Advanced Recoil).
- Registered in `engine/nvf-play/src/player.rs`

### `camera-motion`: Camera Motion

- On the page: On: in first person the view bobs with each step, breathes, gets out of breath after running and jolts when you are hit. Off: it stays still.
- Command-line switches: `--head-bob`, `--no-head-bob` (a saved `head-bob` is read as this row)
- Off by default (the owner, 2026-11-16: every Advanced Movement part off by default but Advanced Recoil).
- Registered in `engine/nvforever/src/settings.rs`

## NVF Features › World Interaction

### `grass-fire`: Grass fires

- On the page: On: flamers, incendiary rounds and fire blasts set grass alight; the fire
  spreads and burns whoever stands in it. Off: fire stays where it hits.
- In full: On: flamers, incendiary rounds and fire blasts set grass alight; the fire spreads
  a short way, burns anyone standing in it and goes out, and the grass grows back once you
  leave the area. Off: fire stays where it hits, as in the original.
- Registered in `engine/nvf-play/src/grass_fire.rs`

## NVF Features › Quick Loot

### `quick-loot`: Enabled

- On the page: On: a container, body or (sneaking) pocket under the crosshair lists its items
  without pausing. Activate takes, Ready Item takes all, Wait opens the menu.
- In full: a Fallout 4 style list beside the crosshair for whatever the crosshair is on that
  holds items. Activate (E by default) takes the selected stack, Ready Item (R) takes everything
  (not from a pocket), Wait (T) opens the usual container or pickpocket menu, the mouse wheel
  moves the selection; the game goes on meanwhile. A pocket lists each item's chance to steal it. Drawn in the HUD colour, at its own
  scale (Scale), in the Menu Font row's font. Off by default [design, #2933].
- Registered in `engine/nvf-play/src/quick_loot.rs`

### `quick-loot-scale`: Scale

- On the page: Size of the Quick Loot list, independent of HUD scale.
- A list: 50% to 100% in steps of 10%; 100% by default. Greyed while Quick Loot is off.
- Registered in `engine/nvf-play/src/quick_loot.rs`

### `quick-loot-background`: Background opacity

- On the page: How dark the box behind the Quick Loot list's words is.
- A slider: 0% to 100% in steps of 5%; 70% by default. Greyed while Quick Loot is off.
- Registered in `engine/nvf-play/src/quick_loot.rs`

### `quick-loot-weight`: Show weight

- On the page: Quick Loot lists each item's weight.
- Greyed while Quick Loot is off. Off by default.
- Registered in `engine/nvf-play/src/quick_loot.rs`

### `quick-loot-value`: Show value

- On the page: Quick Loot lists each item's value.
- Greyed while Quick Loot is off. Off by default.
- Registered in `engine/nvf-play/src/quick_loot.rs`

### `quick-loot-value-per-weight`: Show value per weight

- On the page: Quick Loot lists each item's value divided by its weight.
- Greyed while Quick Loot is off. Off by default.
- Registered in `engine/nvf-play/src/quick_loot.rs`

## Debug

### `overlay`: Text overlay

- On the page: On: the position, loading counts and key help are written over the picture.
  Off by default in the game.
- Command-line switches: `--overlay`, `--no-overlay`
- Registered in `engine/nvforever/src/settings.rs`

### `hit-markers`: Shot markers

- On the page: On: a cross is drawn at each impact and a line from the muzzle along each
  hitscan shot.
- Command-line switches: `--hit-markers`
- Registered in `engine/nvforever/src/settings.rs`

### `god-mode`: God mode

- On the page: On: the player takes no damage from hits and cannot be killed by them.
- Command-line switches: `--god`
- Registered in `engine/nvforever/src/settings.rs`

### `navmesh`: Draw the navmesh

- On the page: On: the navmesh near the camera and the walking NPCs' paths are drawn.
- Command-line switches: `--show-navmesh`
- Registered in `engine/nvforever/src/settings.rs`

### `terrain-wireframe`: Terrain wireframe

- On the page: On: draw triangle edges over the loaded landscape to inspect its shape.
  Changes immediately. Distant terrain is not outlined.
- In full: Off by default. On: every triangle edge of the loaded landscape, as it is drawn
  and collided (smoothed or stored), is drawn as a thin magenta line over the ground, for
  looking at the shape smoothing gives it. Cells that load while it is on are outlined as
  they come; nothing is built or kept while it is off. Not in the original.
- Command-line switches: `--terrain-wireframe`
- More: docs/spec-notes/world.md §3 "Smoothing"
- Registered in `engine/nvforever/src/settings.rs` (drawn by `engine/nvforever/src/terrain_wireframe.rs`)

### `scope-3d`: 3D scopes (test)

- On the page: On: a scoped gun stays on screen and shows the zoomed view in its lens. Off:
  the full-screen scope picture, as in the original.
- In full: On: aiming a scoped gun keeps the gun on screen and shows the zoomed view in its
  scope's lens, with the reticle; the view around it zooms only a little. Off: the
  full-screen scope picture with its black border, as in the original. Guns whose scope lens
  is not known yet keep the picture.
- Command-line switches: `--scope-3d`
- Registered in `engine/nvforever/src/settings.rs`
