# Spec note: the companion wheel and the companion behaviour it sets

**NVForever's words** [design, #2984]: the buttons read Aggressive/Passive, Far/Near,
Ranged/Melee, Wait/Follow, Heal, Talk, Step Back and Inventory
(`fnv_game::companion_wheel::Command::label`); the lines under the wheel are "HP
<now>/<full>" and "Stimpak <n>" for Heal and "Weight <carried>/<capacity>" for
Inventory. Words of the original's screen quoted below only say what it shows.

Tags as in `nif.md`: `[documented]` cites a page or file, `[observed]` says what was looked
at, `[hypothesis]` is a reading nothing has confirmed yet, `[design]` is NVForever's own
choice. Code: `crates/fnv-game/src/companion_wheel.rs` (rules), `engine/nvf-play/src/companion_wheel.rs`
(the menu), `crates/fnv-ai/src/carry.rs` (`Ai::back_up`), `Game::sneaks`
(`functions/senses.rs`), `Game::would_attack` (`awareness.rs`). Tests:
`crates/fnv-game/tests/companion_wheel.rs`, `crates/fnv-ai/tests/carry.rs`
(`a_companion_told_to_back_up_steps_away_from_the_player`), the engine module's unit
tests. Smoke scenario `companion-wheel-boone`. Follows `docs/design/followers.md`.

## 1. Sources

- `menus\companion_wheel_menu.xml`, `menus\prefabs\button_companion_wheel.xml` and
  `menus\prefabs\icon_companion_wheel.xml` from `Fallout - Misc.bsa`, and the 35 pictures
  of `textures\interface\companion_wheel\` [observed: `fnvtool bsa list`; extracted with
  `fnvtool bsa extract` to `C:\fnv-private\menus-extract\companion`, not shipped].
  `menus\globals.xml` for `_background_fill_alpha` (32).
- FalloutNV.esm: the quest VNPCFollowers' topics and lines, their conditions and result
  scripts [observed: `fnvtool dialogue lines --quest VNPCFollowers --text` with
  `fnvtool script decompile`]; the combat styles FollowersCombatStyleRanged (0x000C80B6)
  and FollowersCombatStyleMelee (0x00033DE3) [observed: `fnvtool esm decode`]; the setting
  sCWheelUseStimPack, "Use StimPak" [observed].
- GECK wiki (the owner's export `C:\fnv-private\geckwiki`): SetPlayerTeammate, "Enables
  the Companion Wheel (New Vegas)" and "The teammate will imitate the player's sneak
  state"; the Combat dialogue page, "Regenerating: Said by actor given a stimpak at the
  companion wheel" [documented]. The export has no MenuMode page.
- The owner's two screenshots of the original's wheel, shown in chat on 2026-10-19 (not
  kept as files): the hub's Exit, the picked button's words in a filled bar under the
  title, "HP 94/285" and "Stimpak 84" at the bottom for Use Stimpak, the icons. And the
  owner's word that Open Inventory shows the companion's carry weight, "95/100"
  [observed].
- `Fallout_default.ini` sFontFile_1 and sFontFile_3 for the fonts [documented].

## 2. The menu file

[documented: companion_wheel_menu.xml and its prefabs]

- `CWM_MainRect`, 760 × 760, centred across, its top `(height − 760) / 2 − 55` ("making
  room for the subtitle"); a matte 15 beyond it on each side (`hud_comp_bkg.dds`,
  brightness 128, alpha `_background_fill_alpha`), the pie (`hud_comp_pie.dds`, alpha 255).
- Eight `radial` buttons, ids 0 to 7: Aggressive/Passive, Use Stim, Stay/Follow, Talk, Back
  Up, Near/Far, Open Inventory, Ranged/Melee. Each covers the whole rectangle with its
  slice picture (`hud_comp_slice1` … `8`), drawn at alpha `user10 × _background_fill_alpha`;
  a point picks it between `user2` and `user3` radians and between 76 and 332 (`user4`,
  `user5`) from `user0`, `user1` (the screen's middle). The angles are the multiples of
  π/6 from 1 to 5 and 7 to 11: four slices to the right, four to the left, none at the
  top and bottom. Measured from the rectangle's middle, each icon's middle lies in its own
  slice when the angle is taken clockwise from straight up [observed: computed for all
  eight; unit test `the_slices_follow_the_file`].
- Icons 180 × 180 at (`user6`, `user7` − the rectangle's top); `user8`/`user9` the off and
  on icons, `user12`/`user13` the alternative ones, shown while `user11` is 1; the on icon
  at alpha `255 × user10`, the off one at `255 × (1 − user10)`.
- Texts: the title `&-sCWheelTitle;` ("Companion Commands", the file's comment) at y 110;
  `CWM_ButtonPreviewText` (font 3) at 205 and `CWM_ButtonContextText` (font 3) at 570,
  "Should be Set in code"; Exit (`&-sExit;`) centred on (385, 425), the hub; a subtitle
  rect at 740, 800 wide; three Xbox callouts shown only with a controller.
- Keys: `_PCButton_E` is `SWM_CancelButton`, `_PCButton_W` `SWM_WaitButton`; neither name
  is an element of this file (the sleep/wait menu's names) [observed].
- The class is `&CompanionWheelMenu;`; the file gives no number for it.

## 3. When it opens; its menu code

- The player activates a teammate (SetPlayerTeammate 1): the wheel opens instead of the
  conversation [documented: SetPlayerTeammate]. `Runtime::talk_after`,
  `Game::open_companion_wheel`. With a menu or a conversation already open it is talked to
  instead [design].
- **MenuMode 1075** [hypothesis: the GECK wiki's MenuMode list as remembered, between
  the Vigor Tester's 1074 and the trait menu's 1084, both observed; no shipped script asks
  for it, the export lacks the page]. While open, the game is held as behind every menu:
  no game time, MenuMode blocks run.
- Not saved; `Game::companions` holds it.

## 4. What each command does

Every companion's command lines are lines of VNPCFollowers' topics, and their result
scripts do the work [observed, FalloutNV.esm]:

| Button | Topic said (state → topic) | What the lines' scripts do [observed] |
|---|---|---|
| Stay / Follow | following → FollowersWait; waiting → FollowersLetsGo | Wait: `set <ref>.Waiting to 1`, EvaluatePackage, `VNPCFollowers.fFollowerWaitingLeaveDay`, Boone's faction; Let's Go: in its **end** script `Waiting 0`, EvaluatePackage |
| Aggressive / Passive | passive → FollowersTacticsCombatAggressive; aggressive → …Passive | `FollowerSwitchAggressive` 1 / 0 |
| Near / Far | near → FollowersTacticsDistanceLong; far → …DistanceDefault | `IsFollowingLong` 1, `IsFollowingDefault` 0, EvaluatePackage (and back) |
| Ranged / Melee | ranged → FollowersTacticsCombatMelee; melee → …Ranged | SetCombatStyle FollowersCombatStyleMelee / Ranged, `CombatStyleRanged` / `CombatStyleMelee` |
| Open Inventory | FollowersTrade | OpenTeammateContainer (Boone's only line, INFO 0x000B16BF, also asks VFreeformNovac.nSnipersRevengeStage == 30, which his hire chain's INFO 0x0015F30C sets) |
| Use Stimpak | Regenerating | — (said when given a stimpak [documented]) |
| Talk | — | the conversation, from the greeting |
| Back Up | — | — |

- That the wheel says these topics, rather than setting the variables itself, is a
  [hypothesis]: it gets each companion's own replies and the quest's bookkeeping. The
  first line of the topic the companion can say is the one (`Game::select_line`); a
  companion with no such line cannot use the button (it is dimmed) [hypothesis].
- A command takes effect as the companion starts its reply: the line's begin and end
  result scripts both run then (Let's Go's `Waiting 0` too), and the wheel's icons and words
  show the new state at once; the end script does not run again when the line ends
  [owner, 2026-10-27] (`Game::wheel_command`, log `companion wheel: <command> now shows
  <label>`).
- The reply goes on in real time behind the wheel: its voice plays, its text stands in the
  wheel's subtitle place (`Game::update_wheel_line`) [design: as the conversation's own
  lines go on behind it]. The wheel stays open after a command, but for Talk (the
  conversation opens) and Open Inventory (the container menu opens once it is closed)
  [hypothesis]. Open Inventory's line ("Fine by me.") goes on while the inventory is open,
  heard as it comes up [owner, 2026-10-27] (`Companions::trading`).
- **What the variables then do**:
  - `Waiting`, `IsFollowingLong`: the companion's Wait (Guard) and Follow LONG/DEFAULT
    packages are chosen by their own conditions on them [observed: Boone's
    FollowersBooneFollowPlayerWAIT/LONG/DEFAULT; followers.md §2].
  - The combat styles carry Melee Only and Ranged Only [observed]; the combat AI honours
    Melee Only (nvforever `npc_combat::arm_of`). The style also picks the weapon held
    [documented: GECK wiki Combat Style, "If Melee Only is selected, the combatant will
    always use melee/hand-to-hand as their primary weapon", and Ranged Only likewise]:
    while it fights, an NPC (a companion or any other) holding a weapon its style rules
    out takes up the carried weapon its style allows with the highest damage (a gun's
    explosion counted, actors.md §9.3), the first carried on ties [hypothesis: which one is not documented; the outfitting's own
    rule]; with none allowed it keeps what it holds (a Melee Only actor with only guns
    fights with its fists). Melee is Hand to Hand, Melee (1 Hand) and Melee (2 Hand);
    ranged the pistols, rifles, Handle and Launcher; grenades, mines and thrown weapons are
    never taken up [hypothesis]. An NPC fighting with no weapon equipped takes up its best
    allowed one too [hypothesis: the GECK wiki's UnequipItem page says companions ignore
    NoEquipFlag, i.e. do not stay unarmed; generalised to every fighting NPC]; a weapon
    locked off by UnequipItem's NoEquip flag, or broken, is not taken. Every frame of a
    fight, so a wheel switch mid-fight swaps at once; out of combat nothing changes
    [hypothesis]. A swap is an ordinary equip (OnUnequip, OnEquip) and the engine draws
    what the rules hold (`fnv_game::style_weapon`, log line `style weapon: …`).
  - `FollowerSwitchAggressive`: read by the engine [hypothesis: no shipped script turns it
    into behaviour; the companions' OnHit blocks set it to 1, the hire lines to 0]. A
    passive teammate (the variable declared and 0) attacks only an actor in combat with
    the player or one of the player's teammates, its own enmities included; a grudge (a
    hit) still counts [hypothesis] (`Game::is_passive_teammate`,
    `Game::fights_the_players_side`, `Game::would_attack`). An aggressive one, or one
    whose script lacks the variable, attacks whoever would attack the player, as before.
- **Use Stimpak**: one of the player's stimpaks (an ingestible of equip type Stimpak,
  ETYP 11, the one named Stimpak first) is taken and used on the companion
  (`Game::ingest`, its own Medicine for the potency) [hypothesis: which item, whose
  Medicine]; it can be used while the player carries one and the companion is hurt
  [hypothesis].
- **Back Up**: the companion walks 192 units straight away from the player (along the
  player's view when it stands where the player does), onto the navmesh, at a walk, and
  stands; its Follow package does not call it back meanwhile, and after 5 s the walk ends
  where it is (`Ai::back_up`, log `… backs up from the player`) [hypothesis: the
  distance; design: the rest].

## 5. What it shows

- The icon shows the state the companion is in: the alternative icon while it waits, is
  aggressive, keeps far, fights at range [observed: the owner's screenshots show a
  passive companion's peaceful icon and a ranged companion's rifle].
- The words under the title are what the button does now ("Wait"/"Follow",
  "Aggressive"/"Passive", "Far"/"Near", "Melee"/"Ranged", "Use Stimpak", "Talk", "Back Up",
  "Open Inventory") in a filled amber bar with dark letters; over the hub the bar says
  "Exit" [observed: the bar, "Use Stimpak", "Exit"; hypothesis: the other words].
- The bottom lines: Use Stimpak's "HP <now>/<full>" and "Stimpak <n>" [observed]; Open
  Inventory's "WG <carried>/<capacity>" [observed: the figures, the owner's word;
  hypothesis: the "WG"]; none for the others [hypothesis].
- A picked slice is lit at 0.3 of the interface's colour and the matte darkens the world to
  0.8 inside the wheel [observed: fitted by eye to the screenshots; the file's fill alpha of
  32 would hardly show either]. A button that cannot be used is drawn at half strength
  [design].
- The pointer picks by angle and distance from the drawn wheel's middle, not the screen's
  (`user0`, `user1` are 55 lower) [design: the drawn slice and the picked one agree].
  Left/Right (A/D) step round the usable buttons; a click, Enter or Space uses one; E, Esc
  or Exit closes [documented: E is the cancel button; design: the rest]. The screenshot's
  small arrow from the hub towards the picked slice is not drawn.

## 6. Companions sneaking with the player

- A teammate whose current package follows the player sneaks while the player sneaks
  [documented: SetPlayerTeammate, "imitate the player's sneak state"]; in a combat state it
  stands to fight [hypothesis, as for Always Sneak] (`Game::sneaks_with_player`,
  `Game::sneaks`; the engine crouches it, `nvforever` `sneaking.rs`). A companion told to
  wait does not. Seen in the original: a teammate's IsSneaking followed the player's in the
  same sample, on and off; with the teammate flag cleared, the same NPC on its Follow
  package did not sneak [observed: npc-behaviour-compare.md §15.1].
- As it starts it says FollowersStealthing ("Going stealthy…", Cass's line) when it has a
  line and is not saying another [hypothesis: when the topic is said]
  (`Game::companions_sneak`).

## 7. Not built

- The Xbox controller callouts and the controller's stick on the wheel; the menu's
  sounds; the blur behind it; the arrow from the hub.
- Hardcore's essential teammates, the companions' perks' effects, the Pip-Boy map
  marker for teammates.
