# Owner review: open gameplay decisions

Only decisions that change how the game plays and that need **you**: design choices,
feel, and places where NVForever chose to differ from the original. Most important first.
Anything that asks "what does the original do here?" is not here: the observer can
answer it, and it is in [observer-checks.md](handoff/observer-checks.md) or, the newer ones,
in [observer-checks/](handoff/observer-checks/README.md).

Condensed 2026-10-24 from the full running log (8,300 lines, 2026-10-09 to 2026-10-23).
Merge records, tooling, look-only guesses and items already ruled were dropped. The full
log is in git history: `git show 85dcaace:docs/owner-review.md`. Each spec note still
lists its own guesses.

The #691 merge (2026-10-25) put the full log back above this list by mistake; it was taken
out again on 2026-10-25, and the decisions from the eleven log entries written on 2026-10-24
and 2026-10-25 were folded in below (items 27-32). That last full copy is
`git show 2622e6ce:docs/owner-review.md`. Their questions about what the original does stay
in their spec notes.

**Adding an item:** one bullet in the right tier, what was chosen and what changes if you
rule otherwise, with its spec link. If the original game can answer it, add a file to
[observer-checks/](handoff/observer-checks/README.md) instead. **Ruling on one:** delete
the bullet and note the ruling in the spec it links to.

## Tier 1: felt in every fight or every session

- **A melee creature barely faster than you cannot keep hitting you when you back away
  (#3256).** [observed: runs on the Searchlight Airport runway, main 069ef9bde,
  2026-11-21, the player backing away at a run for 12 s, 20,000 health, no god mode]
  A feral ghoul (runs 384 units/s against your 273) lands 11 to 12 blows, a coyote (366)
  6 to 7, about what they land on a standing player. A young gecko (286) lands 2 to 3,
  with 4 to 5.5 s between blows, against 10 on a standing player. The cause is not the
  chase: every blow that lands knocks you back about 50 units (the knockback rule), and
  at a 13 units/s advantage the gecko needs about 4 s to make that up. Three ways to
  change it, none built: (a) such a creature begins its forward lunge from as far as the
  lunge carries it (tried: it then lunges every 1.7 s, but half the lunges miss and the
  blows that land do not go up); (b) no knockback on the player from a creature's
  ordinary blow; (c) leave it, if the original is the same. Observer check 253 asks the
  original. Until you rule, nothing changes
  ([combat rules](spec-notes/combat-rules.md), "Knockback").
- **Automatic device switching (#2200).** [design, #2200] Switch the saved two-device
  choice on fresh use, default ON with a Controls/INI opt-out; ignore only the switching
  press edge, preserve all releases, and require the old device idle at frame start with
  no release that frame (cancellations remain a backstop). Keep the current device on
  simultaneous activity. Alternatives
  from the issue: an Auto choice, ask once, or discoverability only. Ruling otherwise
  changes this selection policy ([controller section 1](design/controller.md)).
- **NPC semi-automatic guns fire at their attack clip's pace (#2739).** [design, #2739]
  An NPC's or creature's semi-automatic gun, in a fight and in a Use Weapon, is now timed
  by its attack clip in its own rig as the player's is: the clip's `a:` key, else its end,
  ÷ the weapon's attack multipliers (before: always 1 ÷ Attack Shots/Sec). Where the clip
  has an `a:` key this matches the record's rate for the people's guns (combat-rules.md
  §4 table); a gun whose clip has no `a:` key fires more slowly (the player's 5–20 %).
  The sentry bot's launcher goes from 0.63 s to 0.967 s between missiles; the original's
  interval is observer check 221. Ruling otherwise: back to 1 ÷ Attack Shots/Sec for
  NPCs (`fnv_combat::attack::npc_clip_timing`)
  ([combat-rules section 4.1](spec-notes/combat-rules.md)).
- **Pip-Boy pages from their menu files by default (#2215).** [design, #2215] An absent
  `[UI] TileMenus` selects StatsMenu, InventoryMenu, RepairMenu, ItemModMenu, MapMenu and
  QuantityMenu, keeping the hand-built fallback for missing or cut-down files. An empty
  `TileMenus=` restores the hand-built pages. Keep this default at PR review? Reverting
  `DEFAULT_PIPBOY_MENUS` to an empty list restores the old default; other menus and the
  existing maintainer choices stay as they are. ([ui-tile-tree section 6.2](design/ui-tile-tree.md#62-one-menu-at-a-time-nothing-broken))

1. **Crippled torso staggers on every hit.** An NPC with a crippled torso staggers on
   any hit (3 s cooldown), which goes further than your "crippling hit and later hits
   to that part" rule. The player flinches the same way: the view jumps up to 4°, the
   swing breaks, and there is no attack for 0.6 s. This happens even in god mode.
   If you rule otherwise, torso crippling stops causing repeated staggers.
   ([crippled-limbs](spec-notes/crippled-limbs.md), [combat-ai §18](design/combat-ai.md))
2. **NPC gunfire is maybe too deadly.** NPCs react in 0.6 s, their first shots are 4°
   wider, and the spread settles over 3 s. They aim from the eyes. Violet hit 12 of 15
   shots at 1,600 units. If you rule otherwise, openers get slower or less accurate.
   ([combat-ai](design/combat-ai.md))
3. **Combat AI tuning, all our own numbers.** Combat is not a parity target, so play and
   say which of these feels wrong ([combat-ai §12–§17](design/combat-ai.md)):
   - Crouch beyond 600 units, rolled every 1.5–4 s at 25–90 % by temperament. Up to
     40 % extra cover chance.
   - Melee block 80 %/40 % with a 0.12–0.35 s reaction. A blocked blow does 60 % less
     damage and the attacker recoils for 1 s.
   - Dodges about half the time while holding, about 87 units. Combos run 2–3 blows
     (55 % chance to chain).
   - Gunmen stop at 80 % of their reach (400–2,500 units). Flamers stop at 320, shotguns
     at about 480, and launchers just clear of their own blast; launchers never fire into
     it. Cover is at most 1,024 units away.
   - Alarm calls allies within 4,096 units. A fled NPC re-checks after 30 s. Group
     searches give up after two sweeps.
   - [design, #2010] A Lost search with no completed share for 300 s gives up, even
     with shares still pending; each completed share renews that safety deadline.
     A different bound changes how long unavailable navigation/AI holds combat.
     Out of the loaded area nobody searches: it gives up once its target has been
     hidden for 30 s (#3390).
     ([alert section 2](design/alert.md))
   - Fire only with the target within 30° ahead, turning at 240°/s.
   - Creatures start blows on the run from up to 200 units beyond reach (minimum
     closing speed 100 units/s).
   - Comrades and the player block an NPC's shot; animals in the way do not.
   - A fighter in cover hides from whoever shoots at it but fires at its own target.
   - Combat barks: only the 5–30 s timer, no group cooldown (a pack of dogs all yelp).
4. **Melee with a crippled arm.** Uses the game formula's numbers: 40 % less damage
   with a fully crippled arm, 20 % at half. Your notes said −10 % / −5 %. Which one?
   ([crippled-limbs](spec-notes/crippled-limbs.md))
5. **Crippled arms and head, the player's side.** Aim sways in a figure-eight: 1.5° with
   one crippled arm, 3° with both, every 2.5 s. A crippled head adds 0.2 spread on top of
   Perception −4. All numbers are mine. ([crippled-limbs](spec-notes/crippled-limbs.md))
6. **Weapon jamming.** Below 75 % condition a gun may jam: up to 4 % per shot at 0 %
   (0.77 % at 50 %), on a 1.5-power curve. NPCs never jam. More, less, or NPCs too?
   (`crates/fnv-combat/src/tuning.rs`)
7. **Lockpicking and Force Lock tuning.** The sweet spot is 30/20/12/8/5° from Very Easy
   to Very Hard, +1 % per skill point over the lock's need. A pin lasts 1 s of strain
   (+0.02 s per point over). The sweet spot is rolled again each time the menu opens.
   Force Lock succeeds 3 % per point over the need, at most 95 %; failing breaks the lock
   for good, and forcing gives no XP. ([lockpicking](spec-notes/lockpicking.md))
8. **Melee controls.** Holding Use for 0.4 s makes a power attack (you described "about
   a second"). During a round-by-round reload, a press only closes the weapon; a press
   during closing is kept for 0.25 s and fires if closing ends in time.
   ([combat-rules §4, §10](spec-notes/combat-rules.md))
9. **Every bullet is drawn.** The original draws only the record's tracer share (minigun
   30 %, 10mm 0 %). Ours draws every projectile at the record's speed, plus a 0.08 s line.
   Should the original's shares come back? ([combat §5.6](design/combat.md))
10. **True iron sights on by default.** Guns with a sight node aim down the real sights,
    and the crosshair is hidden while aiming. A Gameplay switch gives the original aim.
    ([camera §4.1](spec-notes/camera.md))
11. **Scope sway.** Sways by `scopewobble.nif`, at most half of spread × 5.955°, halved
    unless you fired in the last 0.5 s (about 1° for a new character's sniper rifle).
    The scope comes up 0.25 s after Block. ([weapon-scopes](spec-notes/weapon-scopes.md))
12. **Blood on the screen when hurt.** ceil(lost/max × 25) splats for 20 s, at most 25,
    and it can cover the view in fights. Less? ([screen-splatter §4](spec-notes/screen-splatter.md))
27. **NPCs use their stimpaks.** A hurt NPC (in a fight, or hit within the last 15 s) uses
    a stimpak it carries when at most 25 % of the heal would be wasted, or at any amount
    below 35 % health, 3 s apart at least; the idle plays on the upper body so it keeps
    moving. All our numbers: should NPCs heal more, less, or not at all?
    ([combat-ai §21](design/combat-ai.md))
36. **Lunges carry the attacker.** A forward power attack (a lunge) whose clip moves its
    root more than 100 units carries the actor that far (a Deathclaw's 523 units, your
    forward power attacks 240-280, Legion Assault 332), stopped by walls and by whoever it
    lunges at; a lunging actor pushes nobody aside, as you ruled for bites. Other blows
    still step in place (carried, the dogs ended up biting from inside the player's
    reach); the yao guai's standing power attack (305 units) so still slides back. If you
    rule otherwise: every travelling blow carries, and the AI must step back after it.
    NPCs start their lunges once they touch their target, so an NPC's lunge carries it
    little or nothing until the AI lunges from farther out. ([actors §8.13](spec-notes/actors.md))
37. **Melee knockback distance.** A melee blow that leaves its target standing pushes it
    back through its collision: the knockback formula's force read as a speed falling to
    rest over 1 s, so force / 2 units (a Sledgehammer hit about 57, a punch 24, a
    deathclaw swipe up to 256). A blocked or 0-damage blow still pushes 15 units (the
    formula's base force). Our reading of the formula, not measured in the original:
    more, less, only on power attacks, and should a block take the push too?
    ([combat-rules, Knockback](spec-notes/combat-rules.md))
46. **NPC automatic fire comes in bursts (#1448).** An NPC's automatic gun (Securitrons'
    and other embedded weapons too) fires a V.A.T.S.-sized burst (0.4 s of its fire rate,
    3 to 8 rounds), then lets go for 0.5–1.5 s (scaled by the combat style's semi-auto
    delay multipliers) before the next. Before, it held the trigger for as long as it saw
    its target. The original's rule is unknown (the WEAP Short/Long Burst flags are not
    understood). Longer or shorter bursts, longer pauses, or Long Burst weapons firing
    longer? ([combat-ai](design/combat-ai.md), `fnv_combat::burst`)
84. **Cowards flee every fight from its start (#2520).** An NPC or creature whose
    Confidence is Cowardly (and was not raised at a flee spot) now runs from every fight
    at once, unhurt, whatever the odds; before, it fled only once hurt and its threat
    ratios said so, and it still fought whoever attacked it. The GECK wiki says a coward
    "will always flee from combat"; from the very start is our reading, not seen in the
    original (observer check 180 asks about the Silver Rush customer this was found on).
    It changes every Cowardly actor in every fight. Should cowards still fight back when
    cornered, or wait until hurt? ([combat-ai §12](design/combat-ai.md))
85. **Faction allies of the player take up fights against the player (#2794).** An actor
    whose faction names one of the player's an Ally (or a Friend, with Helps Friends and
    Allies) and whose Assistance helps it now joins against whoever fights the player
    when it detects the player, as it would for any other ally; before, Assistance never
    counted the player, so the Legate's camp Securitrons (PlayerFaction Ally, Helps
    Allies) stood by once their scripted guards were dead and never fought General
    Oliver. The documentation does not say Assistance counts the player
    [hypothesis: your expectation in the issue thread]; cowards, Helps Nobody, actors
    allied to the attacker and anyone after the player itself (a crime, a grudge) stay out, teammates keep their own rule. It reaches faction
    disguises too: in NCR armour ArmorNCRFactionNV is the player's friend, so NCR
    soldiers with Helps Friends and Allies join the player's fights. If allies now pile
    into fights you start (they join once the other side fights back), say where.
    Observer check 217 asks the original. ([combat-ai §10.3](design/combat-ai.md))

- **While VATS targets are chosen, world sounds and voices slow only as far as while
  VATS fires (0.71 speed), no longer to 0.4 (#3784).** A player took 17 s to mark in the
  NCRCF fight and read the voices, played more than an octave down all that time, as
  broken sound. Your "more drawn out and lower pitch like the base game" still holds;
  rule otherwise for the old 0.4 drone, or for silence (or ducking) while marking.
  [docs/design/vats-mode.md](design/vats-mode.md) §4.

## Tier 2: movement and controls feel

- **Harmless authored drops without Allow Falls (#2532).** [design, #2532]
  Use `fJumpFallHeightMin` (`SAFE_DROP_HEIGHT` fallback 600) as the per-link safety
  limit when the ordinary route is unavailable; no invented ledge links. A lower
  limit would leave more patrol routes unreachable. [hypothesis] This predicts
  harmless falls; parity is pending observer check 214
  ([navmesh](design/navmesh.md#3-paths), [packages](spec-notes/packages.md)).

- **Darting movement clips (#2352).** [design, #2352] `ROOT_EXCURSION_RATIO = 1.5`
  selects moves whose sampled root excursion exceeds 1.5 times net travel; their
  drawn x/y stays at the track's start. A different ratio changes which moves lose
  their in-cycle sway. Actor movement is unchanged; original dart behavior awaits
  observer check 169. ([actors §8.2](spec-notes/actors.md#82-root-motion))

13. **Grab.** Z held, or E held for 0.4 s, picks up an object or a corpse. It drops when
    lagging 90 units and is thrown at up to 600 units/s. Nothing over 100 kg can be
    picked up, and you can only walk while holding over 25 kg. All guessed.
    ([grab](spec-notes/grab.md))
14. **Steps and road bumps.** Body and view follow the ground through a 0.2 s speed
    prediction and a three-stage ease (2026-11-07, after "every bump" on roads; eased
    2026-11-12 after feedback that curbs felt jarring): road ripples are about 2.5-3x
    smoother, a curb starts and ends without a jolt, settling in about 0.24 s, and walking
    off a rock or a road edge (up to 24 units down) is eased the same way instead of drawn
    as a fall; walking, steps ease more slowly (over about the same distance walked)
    and the view no longer twitches with the drawn floor of broken roads, but
    running onto a steep hill the body is drawn up to 13 units below the feet for a
    moment (up to 24 units allowed; over 48 counts as a teleport). A step-up counts from
    0.25 units of rise. Where a slope or stairs end the body carries on up: up to 13.3
    units over the top of a 30° slope and 2.6 over the top stair at a run (9.1 and 0.5
    walking) [observed: replays, physics §6.11]; cutting it halves the slope's overshoot
    but doubles the road roughness. Smooth enough, or too floaty?
    ([physics §6.8.1, §6.9.1](spec-notes/physics.md))
15. **Third-person aim.** Shots and the activate prompt go to what is under the
    third-person crosshair, but the shot still leaves from the eye, so cover near you
    blocks it. Shoot from the camera's line instead?
16. **Releasing camera pan.** Letting go of the free-look key returns you straight to
    first person if you were in it. Did your item mean "stay in third person"?
17. **Controller.** Grab is on the right-stick press, Toggle POV on LB, and the D-pad is
    Hotkey 1 / Ammo Swap / Hotkey 3 / Hotkey 4. Look speed is 1.5/1.2 rad/s, squared, and
    you walk under 0.6 stick push. Keyboard and mouse are off when Controller is chosen:
    should both work? Rumble strengths are guesses. ([controller](design/controller.md))
18. **Seated view limits.** With "Body in first person" on, a seated view turns at most
    80° to each side and 75° down; with it off, all the way round. Limit both?
    ([player-body §1.5](design/player-body.md))
28. **NPC clips speed up with Speed Mult.** NPCs move at their Speed Mult (Legion raiders
    130-150, Lanius 180) and their clips play faster so the feet keep up. The wiki says
    the original's clips stay at 100 % (sliding feet), and one AI observer capture
    (Cook-Cook, Speed Mult 110) ran at the people's speed without his Speed Mult. Keep the
    matched feet, take the sliding feet, or have people ignore Speed Mult?
    ([actor-flags §3](spec-notes/actor-flags.md), [actors §8.10](spec-notes/actors.md))
55. **People walk slower with a pistol drawn outside combat (#2124).** A person in light
    armour or clothes walking a package with a pistol out, not in combat, walks its walk
    divided by fMoveNoWeaponMult (1.1): 85.3 / 1.1 = 77.5 units/s (the original's were
    77.5-78.3). The other two ways to reach 77-78, the `1hm`/`h2h` walk clip (77.5) or
    fMoveBaseSpeed (77), were not taken, and every other weapon stance, medium and heavy
    armour and walks in combat keep the holstered walk until measured. If you rule
    otherwise, the drawn walk takes the clip's or the setting's number, or reaches the
    unmeasured stances too. (`fnv_ai::carry::drawn_walk_share`;
    [actors §8.10](spec-notes/actors.md))
38. **Variable refresh on your screen.** Display sync has a new "Variable refresh" choice
    for G-Sync: vsync stays on and the frame rate is held under your monitor's refresh rate
    (138 at 144 Hz). Does G-SYNC engage in borderless and in exclusive full screen (the
    NVIDIA G-SYNC indicator), and does Primm feel smoother than plain vsync? Should it be
    the default? ([display-sync](spec-notes/display-sync.md))
39. **Controller aim assist strengths.** With a controller, the stick turns 45% slower
    while the crosshair is on a hostile within 1800 units, and aiming down the sights
    eases the view onto the hostile nearest the crosshair (within about 8.7°) for 0.3 s.
    On by default, both strengths 0.6 on the settings page's Controller group. Too
    strong, too weak, or should either be off by default?
    ([controller §5](design/controller.md))

44. **No keyboard drop key on the Pip-Boy's ITEMS page (#1393).** Dropping is a right
    click on a row (Shift + right click the stack), and X is only the mod menu's, as the
    original's PC file gives it; the controller drops with X. A keyboard-only player can't
    drop. If you want a key, name it and it drops one (Shift with it the stack); free on
    that page are, for example, G or Delete. ([pipboy §6](spec-notes/pipboy.md))
48. **Controller buttons for menus whose files are not read (#1662).** Menu hints now
    name the controller button that presses their key. Where no menu file gives a button,
    these are guesses: Caravan bet Accept A, Auto Match Y, Raise RT, Exit B; deck Add A,
    Remove X, Randomize Y, Play Caravan RT (B leaves); game Select A, Discard Card X,
    Discard Track Y, Cancel B. Poker Check/Call A, Raise RT, All In Y, Fold X, Next Hand
    A, Leave B. The pause menu's X deletes (a save on the Load page, a binding on the
    Controls tab). The STATS page's RAD view: RadAway A, Rad-X X. Right buttons?
    ([controller §3](design/controller.md))
49. **PlayStation button names (#1452).** With a controller, Button style Auto (settings
    page, Controller group) shows Cross/Circle/Square/Triangle, L1-R3, Select/Start when
    the pad says it is a PlayStation one (DualSense, DualShock, "Wireless Controller",
    PS3/PS4/PS5, Sony), else the Xbox names; Xbox or PlayStation forces them. The INI keeps
    the Xbox names. The crosshair prompt draws NVForever's own outline glyphs for the four
    face buttons; menu hints write the names (the game fonts have no symbols). Select and
    Start rather than Share/Create and Options. Right choices, and do the glyphs look
    right? ([controller §3a](design/controller.md))
65. **Held menu keys repeat; explosions shake the view (#1891).** Both are on for
    everyone, with no tweak turned on: an arrow key, W/A/S/D or Page Up/Down held in a
    menu (or the D-pad) presses again after 0.4 s and then 10 times a second; an
    explosion near the player turns the view up to about 0.8° for the same time the pad
    rumbles. The Tweaks tab's Key Repeat Acceleration and Scale Explosion Shake (0 turns
    it off) build on them. Our own numbers; if you rule otherwise, either goes back to off
    and its tweak waits. (`engine/nvf-play/src/key_repeat.rs`, `shake.rs`)
66. **A grenade indicator on the HUD (#1891).** NVForever had none; it now draws the
    menu files' own explosive indicator for every live grenade whose blast would reach
    you, on the grenade or on the view's edge pointing at it, for everyone. Where the
    original puts it is a guess (observer check 190). The Tweaks tab's Hide Grenade
    Indicator turns it off. (`engine/nvf-play/src/grenade_indicator.rs`)
67. **An idle vanity camera (#1891).** After 120 s with no input (the master's
    `fVanityModeAutoDelay`), the camera goes to third person and slowly circles you (a
    turn in about 42 s, our number); any input ends it and the view stays third person,
    as after Toggle POV held. New for everyone. The Tweaks tab's Vanity Camera Returns to
    First Person puts a first-person view back. (`engine/nvf-play/src/vanity.rs`)
68. **A heartbeat at low health (#1891).** Under a quarter of your health, the master's
    `UIHealthHeartbeatALP` loops until you heal, for everyone. The threshold is ours (no
    setting names one); observer check 192 asks the original's. The Tweaks tab's
    Heartbeat Fades lets it fade. (`engine/nvf-play/src/heartbeat.rs`)
69. **Loaded ammo marked (#1891).** The Pip-Boy's ammo list now gives the ammo your weapon
    loads the equipped square, for everyone (observer check 193); the Tweaks tab's
    Containers Show Equipped Ammo shows it in containers too. Terminals also fade out as
    they close now (the menu file's 0.25 s). (`menu/model.rs`, `terminal.rs`)
70. **Critical-hit corner message (#1891).** A critical by the player now shows "Critical
    hit on <name>" in the corner with the masters' critical icon (`sCriticalStrikeIcon`),
    for everyone; the words are ours (the original's are not in the data). The Tweaks
    tab's Critical Hit Messages Name the Limb adds the part. (`combat.rs`)
82. **"Map marker added" (#1891).** When a script puts a place on your map (ShowMap), a
    corner message now says "Map marker added", for everyone (our words; the original's
    pop-up is only known from a mod's readme). The Tweaks tab's Map Marker Added Names
    the Place adds the name. (`engine/nvf-play/src/marker_added.rs`)
83. **Hardcore need messages (#1891).** In Hardcore, a dehydration or sleep stage changing
    now shows the masters' own line ("Your dehydration level has increased." and the
    rest), for everyone; which change says which is our reading (observer check 194). The
    Tweaks tab can turn each off. (`crates/fnv-game/src/needs.rs`)
73. **World borders (#1891).** A region flagged Border Region now keeps the player inside
    its areas: step past and you are put back with the masters' "You cannot proceed in
    that direction" line in the corner. No master flags a region of a played worldspace,
    so only plugins meet it; how the original stops you is a guess (observer check 195).
    The Tweaks tab's World Borders: Warning Only keeps the line without the stop.
    (`crates/fnv-game/src/borders.rs`, `engine/nvf-play/src/borders.rs`)
58. **Followers come in once you stop (#2122).** A follower on its way now stops 150
    units inside its package's distance (not nearer than 128): Boone 180 of his 330,
    DefaultFollowPlayerFar 362 of 512, the usual 256 at 128; before, it stood near the
    distance. The original's two recorded arrivals were about 150 inside (Boone 177, the
    pack brahmin about 300 of 450); its rule is not known. Keep the fixed 150, take a
    share of the distance, or the old stop at the distance?
    ([actor-behaviour §5.1](design/actor-behaviour.md), `FOLLOW_STOP_INSET`)

## Tier 3: rules where we chose to differ, or that the original has no answer for

- **Killing soldiers of a side you started a fight with costs karma (#3401).** [design:
  your earlier ask that killing innocents cost karma] A Neutral NPC killed in a fight the
  player started is a murder, −25 karma each (a quarter of `fKarmaModKillingEvilActor`).
  Legion soldiers are Neutral in the data (karma 0; VCaesarsLegionFaction has no Evil
  flag), so opening fire on Nipton's Legion costs 25 karma per legionary, while Vulpes
  (Very Evil) gives +100. Players on Discord and in #3401 expect no karma for Neutral
  enemies, as they believe the original does (not observed yet). Choices: (a) keep it;
  (b) a Neutral NPC already fighting the player when it dies is no murder, whoever
  started the fight, which keeps the cost for shooting bystanders; (c) Neutral kills give
  no karma at all, as HelpKarma's rule reads. Until you rule, (a) stays
  ([crime-karma §4](spec-notes/crime-karma.md)).
- **Chinese and other multi-byte game text, and a bundled font (#1443 step 4).** [design,
  #1443] NVForever's own words have a Simplified Chinese file (`lang/zh.toml`) and the
  menus draw it with a CJK font only when the system has one or `[Interface] CjkFontPath`
  names one; nothing is bundled. Game text in Chinese needs plugins in a multi-byte
  encoding (`[Text] CodePage=936` reads GBK/GB18030; which encoding community
  translations use is not known) and the game's 256-glyph `.fnt` fonts cannot draw it.
  Nothing more is built until you choose: (a) leave it as now (a font the player
  supplies), (b) bundle an open-licence CJK font (about 10-20 MB per weight; licence and
  size to pick), or (c) also draw every game-font text through it when the code page is
  936. ([fonts §2](spec-notes/fonts.md), lang/README.md "Simplified Chinese and fonts")
- **Switching the game's language means another game folder (#1443 step 2).** [design,
  #1443] A language is a different set of data files, and Steam keeps one install of the
  game, so a second language is a copy of the folder. The launcher's Game folder page
  already restarts into another folder; no in-game Language-of-the-game row is built (the
  settings page's Language row switches NVForever's own words live). Keep it in the
  launcher, or add an in-game row that lists game folders and restarts into one?
  (lang/README.md, `crates/nvf-launcher` Game folder page)

- **Saving while a mod's temporary clone is placed (#3147).** [design, #3147] While a
  reference whose base was made by `TempCloneForm` is alive, every save is refused
  whole (`Game::checked_snapshot`; a failed save now says so in a corner message). A mod
  that keeps such a clone placed stops all saving for as long as it exists. The other
  choice: save without those references, so the game is saved and the clones are gone
  after a load (dropping them silently was rejected in review; nothing records your
  choice). Keep refusing, or save without them?
  ([temporary scene bases](spec-notes/temporary-scene-bases.md))
- **Corpses in a crowd held while a head or hand still turns (#2083).** [observed, #2083]
  In battle-50, 34 corpses were held still moving at their time limit: 26 shot corpses
  at the 1 s `MAX_AWAKE_SHOVED` (your item 51), 8 fresh ones (7 at the 3 s
  `MAX_AWAKE_CROWDED`, an engineering choice kept as a cost guard when `MAX_AWAKE` went
  to 8 s), with a head, hand or foot turning 0.7-4 rad/s. Neither changed. Longer limits
  would let more settle, at the solver cost they guard against. Keep both, or raise the
  crowded one? ([ragdoll §4](spec-notes/ragdoll.md), "Corpses in a crowd")
- [design, #2659] **Selected-part mesh boundary:** mixed-mesh triangles are selected at
  a mean bone-weight share of `PART_TRIANGLE_SHARE = 0.5`, then filled at the same mask
  opacity as whole parts. A different threshold changes the boundary, not its strength;
  coarse meshes may need finer clipping. [Classic VATS §12](design/vats-classic.md#12-the-selected-part-lit-on-the-target-2026-11-10).
- **SayTo's "no target look" flag turns the speaker again (#3989).** [observed, check 218]
  The original turns a standing speaker to its target with the flag as without it; only
  the target is not made to look. NVForever now does the same, so Curtis may turn to the
  player for his radio call in the McCarran tower again (#2796), until what keeps him
  still in the original (his package or idle there, check 218's open part) is matched.
  ([dialogue-play §9.2](design/dialogue-play.md))
- **Hold CLAMP effect idles (#2685).** [design, #2685] `HOLD_CLAMP_EFFECT_IDLES` keeps
  a CLAMP actor idle with material/emitter tracks at its exact end until replacement or
  stop, overriding repeat counts. This keeps attached chamber effects and pod glow;
  disabling it restores finite-idle fade-out. Review the generalization to other
  effect idles ([idles section 10.1](spec-notes/idles.md#101-chamber-effects-and-external-kf-controllers-2685)).
- **Terrain smoothing: which recipe by default (#2149, #2676).** [design, #2149]
  The default is the contributor's pick from nine variants compared in game and
  screenshots on 2026-11-15, for you to confirm or change: **detail, 4 × 4, bound 8,
  thresholds 2:6**. Interpolate keeps the 8-unit
  height steps; fit flattens real features; detail evens the steps while keeping bumps,
  crests and ridges (moves up to 2 units in full, fading out by 6). The 4 × 4 grid gives
  rounder silhouettes, and bound 8 keeps placed objects in contact by holding the
  surface within one height step of the stored triangles. Smoothing stays on by default
  with collision following it (your 2026-11-13 decision); the setting applies at the
  next start so drawn ground and collision change together. Measured 2026-11-15 (1920x1080,
  unpaced, RTX 5070 Ti) [observed]: standing frame time unchanged (3.78 ms, 2 × 2 3.74, off
  3.78), worst frame in a fast flight over Goodsprings unchanged (32.9 ms, 2 × 2 32.4);
  about 255 MB more RAM and 130 MB more video memory than 2 × 2. Since #2676 the row is a
  list, Off / 2 x 2 / 4 x 4 (still the default) / 4 x 4 near (4 × 4 within 2,048 units of
  the camera, 2 × 2 further, collision on 4 × 4, no cracks between the levels); the open
  question is which by default: 4 × 4 (now), 2 × 2 or 4 x 4 near. Measured 2026-11-16 (#2801:
  same hillside and flight): frame time the same at every level; 4 x 4 near keeps most of
  2 × 2's memory saving (about 250 MB RAM standing, up to 260 MB video memory in flight). The test aids
  still choose the other recipes, and the Debug row Terrain wireframe shows the triangles.
  Smoke `terrain-smoothing` covers the default; `terrain-smoothing-fit-s2-b8`,
  `-interpolate-s4-b8`, `-fit-s4-b12`, `-detail-s4-b8` cover explicit recipes.
  ([world §3 "Smoothing"](spec-notes/world.md#3-landscape-land))

- **An intercom nobody has set up speaks for the actor its script names (#2454).**
  [hypothesis, #2454] When a talking activator starts a conversation before any script
  told it who speaks, ours lets the one actor its scripts name speak (Ramos for the
  Hidden Valley intercom), instead of saying nothing. Without it, sneaking into the
  bunker and being sent out for your gear by McNamara left your controls off for good.
  Check in game that Ramos speaks on the intercom there on a sneak-in save.
  ([talking-activators §2](spec-notes/talking-activators.md))
- **An intercom whose talking actor has no line says its own (#2768).** [design, #2768]
  A script's Say or SayTo on a talking activator now says its talking actor's line
  (observer check 219 asks whether the original does). When that actor has no line of
  the topic, ours falls back to the activator's own lines, as before #2768, so no line
  an intercom said before is lost. The alternative: no fallback, so a talking activator
  with an actor set says only that actor's lines, nothing otherwise (closer to the
  catalogue's "use the dialogue of a given actor"). Ruling otherwise changes
  `Game::say_with` alone. ([talking-activators §2](spec-notes/talking-activators.md))
- **Grass fires: size, speed and regrowth (#2567).** [design] Burning weapons and blasts
  light generated grass; a fire spreads in 64-unit patches (0.5-1.5 s a step, faster
  downwind), each patch burns 6 s, and a fire stops at 1,024 units from its start or 500
  patches, at most 8 fires at once. Anyone standing in it catches the source's burn,
  credited to whoever started it. Burnt grass grows back whenever its cell reloads (not
  kept in saves). Bigger reach, a slower front, fires that leave the ground scorched for a
  game day, or trees and shrubs that burn too are each a figure or a follow-up away.
  [grass-fire](design/grass-fire.md)

- **Rigid headgear frame detection (#2529).** [design, #2529] Choose bone-local axes
  when a rigid head part's combined authored bounds straddle skeleton Z=0 and their
  centre aligns at least 0.9 with bone-local up (`BONE_LOCAL_HEAD_ALIGNMENT`);
  otherwise retain the skeleton-axis rule. This fixes Oliver's supplied +X cap layout
  without naming a model. Keep this conservative geometry rule, or require explicit
  model-frame metadata for ambiguous mod meshes? ([actors §1.2](spec-notes/actors.md#12-meshes))

- **NPC conversations go on past a Goodbye line that links on (#2451).** [hypothesis,
  #2451] The editor's documentation says a Goodbye line ends a conversation between
  NPCs; ours ends it only when the line links nowhere, because the data needs it:
  Veronica and Ramos's exchange at the Hidden Valley intercom (all its lines Goodbye)
  opens the bunker only at its third line, and Jason Bright's speech continues past a
  Goodbye part. NPCs' Dialogue packages that talk to another NPC or an intercom are now
  carried out too (also the Strip's street scenes). Check in game that Veronica walks
  to the intercom, the exchange plays out and the inner door opens.
  ([dialogue §5.1](spec-notes/dialogue.md))
- **Trigger countdowns count game time once a frame, and wait under menus (#2455).**
  [hypothesis, #2455] A trigger script runs once for each actor inside it; now only the
  first of those runs in a frame counts the frame's time, and no trigger script runs
  while a menu, message box or conversation holds the game. Ibsen's virus purge now
  lasts its 20 s (his warning at 10 s left). Check in game that the purge gives you the
  time to reach the terminals. ([scripts §5](spec-notes/scripts.md))
- **Linux overlay GPU source priority (#2249).** [design, #2249] `GPU_SOURCE_ORDER`
  prefers whole-device NVML, then amdgpu, then game-only DRM client time. Choosing
  DRM first would make supporting Linux drivers show the game's contribution consistently.
  [Settings usage note](design/settings-page.md#performance-usage-availability-2249).
- **Dialogue packages walk you into their trigger area (#2453).**
  [hypothesis, #2453] With "Do Not Control Target Movement" off the original walks the
  player into the package's trigger location; ours does it once you are close enough
  and in the NPC's sight: you walk (your movement keys set aside) to a point 48 units
  inside the area, then the NPC greets you. Ramos at Hidden Valley needs it (his escort
  runs only with you inside his box). Check in game that the walk feels right as you
  come down into the bunker's first room, and at Davison's door in REPCONN's basement. ([packages §3](spec-notes/packages.md))
- **Explosion-hit handlers on corpses (#2346).** [design, #2346] A blast now reaches a corpse
  whose ragdoll has taken over, so ShowOff's OnExplosionHit handlers also run for corpses
  it catches (no damage is taken). Whether the original calls them for a corpse is not
  observed. Leave them out for corpses instead?
  ([combat-rules §11](spec-notes/combat-rules.md))
- **A script jumping the clock (#2353).** [design, #2353] A script's GameHour write of more
  than `SCRIPT_HOUR_JUMP` (1 game hour) is handled like the end of a wait: everyone chooses
  its package at once, residents out of sight are put at their new place, those in view
  walk. The original's handling is unknown (observer check 132); a smaller threshold would
  catch shorter jumps, a larger one fewer. ([packages §9](spec-notes/packages.md))
- **Drinking from water holds nothing (#2349).** [design, #2349] Drinking from a sink or
  pump now plays the tree's drinking idle (first or third person), but the player keeps
  moving, looking and the HUD while it plays, and the water's healing and rads apply at
  the activation, before it. Ruling otherwise would hold the player for the clip's 2.4 s
  (as scripted first-person idles do) or apply the effect at its end. The original's
  behaviour is observer check 131. ([idles section 16](spec-notes/idles.md))

- **Goodbye lines finished in the world run their end script late (#2449).** [design,
  #2449] With `goodbye_at_once` (the v0.7.0 rule, a switch that is off since #2157) a
  chosen Goodbye line's end result script still runs when the actor finishes the line,
  not when the menu closes. That is what kept the Boomers shelling the player in v0.7.0:
  Pearl's last line (0x001050FD) starts Volare! from its end script about 40 s after the
  menu closed, and a player back in the artillery field by then is never counted out
  (NellisArtilleryScript passes over the player on leaving once Volare! runs). Kept as it
  is because the switch is off; if you turn it back on, the end script should run as the
  menu closes ([dialogue §5](spec-notes/dialogue.md)).
- **Texture scrolls drive every shape sharing the property (#2450).** [design, #2450]
  A sequence's texture offset, scale or rotation track now moves every shape that lists
  the same `NiTexturingProperty`, as material tracks already did for a shared
  `NiMaterialProperty`: the Hidden Valley storm's 18 dust sheets all scroll (only the
  first did). This applies to every model whose sequences scroll a shared texture, not
  only the storm; if some model should scroll one shape only, it would need its own
  property, and ruling otherwise means a per-model exception. Check the storm in game at
  night in HVDustStorm ([activators section 6.1](spec-notes/activators.md)).

- **Map markers on the compass use a fixed range (#2348).** [design, owner 2026-11-18]
  The maintainer chose `iMapMarkerVisibleDistance` (10,000 units by default) for found
  and unfound places, replacing the Perception-based range from playtest list 5 item 12.
  Quest targets remain visible at any distance. ([hud §11](spec-notes/hud.md); observer check 227)

19. **Walking away from a conversation.** The world freezes while you talk, as in the
    original, and walking off does not end it. Should walking off end it where the line
    allows leaving? ([dialogue](design/dialogue.md))
20. ~~NPCs opening scripted doors skip the door's OnActivate~~ (closed 2026-10-25: NPCs
    run the door's OnActivate first and go through only when it activates the door for
    them, as the original does; #729, [doors §5](spec-notes/doors.md)).
21. **Hidden Valley intercom.** Ours says Ramos's "[You hear nothing.]" and waits; the
    original says nothing and ends the conversation. Keep ours?
    ([talking-activators §3](spec-notes/talking-activators.md))
22. **Character revision traits.** Traits the script does not name (Skilled, add-on
    traits) stay, so the trait menu can grant up to two more. Clear them instead?
    ([chargen §9](spec-notes/chargen.md))
23. **Difficulty is an NVForever setting, not saved per game.** Companions are not
    scaled by it. ([difficulty](spec-notes/difficulty.md))
24. **Loading a save with missing plugins.** It loads with a notice and drops the
    missing content. It is refused only when you stand in a missing plugin's cell. Prompt
    first? ([save-load §4.1](design/save-load.md))
25. **Delete-save key.** Delete on the load page asks Yes/No. Which key do you want?
    ([save-load §6.1](design/save-load.md))
26. **Cloaked actors in VATS mode.** A Stealth Boy user or a cloaked Nightkin
    (Invisibility or Chameleon at least 1) can no longer be marked in VATS mode unless
    you have Enhanced Sensors (ED-E), which the perk's entry point names. Before, VATS
    mode marked them like anyone. Keep the refusal? ([perk-entry-points §13](spec-notes/perk-entry-points.md))
29. **VATS kill cameras on every last kill.** When the last mark of a VATS-mode run kills
    its actor, the game's camera paths play a kill camera every time: no chance roll and
    no setting to turn it off; the HUD stays up. Add a roll or a switch?
    ([vats-mode §19](design/vats-mode.md))
30. **The game's death cameras.** The masters' death cameras are loaded but not played,
    because your own death camera plays instead. Should the game's replace yours?
    ([vats-mode §19](design/vats-mode.md))
31. **VATS special attacks.** The weapons' special VATS attacks (the Machete's Back
    Slash, Mauler, Lights Out, Grand Slam: WEAP VATS/VANM) need a choice per mark, which
    VATS mode does not have, so they are not built. Wanted, and how would you pick one
    (a key cycling the mark's attack)? ([weapon-data-fields §7](spec-notes/weapon-data-fields.md))
31a. **The classic VATS style's other choices (a contribution from the fork h9x000,
    2026-11-11).** You ruled on 2026-11-24 that Classic is what V starts; Dead Eye
    ("Mark and Fire" on the settings page, a name of ours for you to change) stays one
    row away. What Dead Eye shares with Classic and so changes too: marks on a dead target
    are dropped and the mode ends 2 s later, the HUD hides from the first shot, the
    enemy bar is at the top, explosives hold the mode's end, and every kill camera's
    last shot is held 3 s longer (vats-classic.md §13).
    Decided by the fork's owner in play, for you to keep or change:
    the target screen drawn from the game's own menu file unless
    `[UI] VatsMenuFile=0` (your convention is the other way, a name in `[UI] TileMenus=`);
    three camera angles of NVForever's own for the attacks instead of the shots' models;
    a queue that stops when its target dies; and the physics keeping 60 steps a real
    second whenever the clock runs slow, in any mode. It also builds the special
    attacks of item 31 (R queues one). ([vats-classic](design/vats-classic.md))
32. **No message on a sneak attack critical.** Sneak attacks (your rule: always a
    critical on an unseen target) show nothing on screen; the game files have only the
    icon setting `sCriticalStrikeIcon`. Show a message or the icon?
    ([combat-rules §4](spec-notes/combat-rules.md))
33. **3D scopes (test version built, off by default).** The gun stays on screen and the
    zoomed world shows in its scope's lens: "3D scopes (test)" on the settings page, or
    `--scope-3d`. It works for 17 scope models (sniper, hunting, varmint/Ratslayer, 9mm,
    laser, trail carbine); other scoped guns keep the original overlay. It costs about
    1.7 ms a frame while aiming (5.7 → 7.4 ms in Goodsprings). Keep it, make it the default, and should the view around
    the lens zoom a little (now 0.2 of the way) or not at all? ([scope-3d-lens](design/scope-3d-lens.md))
33. **3D scopes (not built).** You asked about the mod look: the gun stays on screen and
    the zoomed world shows in the scope's lens. It can be built. Weapons with no lens in
    their model get a measured disc, and a few scopes keep the original overlay. It
    costs a second world render while aiming. Planned as a Gameplay switch, off = the
    original overlay. Should it be on by default, and should the view around the lens
    zoom a little or not at all? ([scope-3d-lens](design/scope-3d-lens.md))
34. **MCM and the pause menu (UI tile tree).** Built 2026-11-12 and on by default (#976):
    with MCM installed and an MCM mod loaded, the pause menu's Mods entry opens MCM's own
    window, drawn from MCM's files where the original draws it; look at it beside the
    original (your screenshots of 2026-11-12 are the reference). MCM lives in the original's StartMenu,
    which is where our own pause and settings pages are. Chosen: our
    column and settings stay as they are. The game's StartMenu is built from its files
    behind them while the pause or main menu is open, so MCM's scripts find their
    tiles. A "Mods" entry joins our column only when a mod added one, and opens MCM's
    page drawn from the game's files. If you rule otherwise, the whole pause menu is
    drawn from the game's (or a mod's) start_menu.xml and our settings become one entry
    in it. ([ui-tile-tree §7](design/ui-tile-tree.md))

35. **Small corpses pass through each other.** Corpses under 64 units (rats, geckos,
   radroaches, bloatflies, mantis nymphs) no longer collide with each other, only with
   the ground, objects and bigger corpses; people, dogs and bigger still pile up. It
   cuts the physics cost of a heap of 20+ small corpses (#910). Look at a nest of nymphs
   killed in one spot: if corpses lying inside each other look wrong, the size limit can
   come down or the rule go. [ragdoll.md](spec-notes/ragdoll.md), "Corpse against corpse".

40. **Hotkeyed weapons come in holstered (your ruling, #1106).** A weapon picked by a
   number key or the weapon wheel is equipped holstered; a weapon in hand is put away
   first, and Ready Item draws the new one. The original may draw it. Kept here only so
   it is on record as a difference; if you want a drawn weapon to stay drawn on a swap,
   it is one function (`holster_for_new_weapon`). [weapon-wheel.md §4](design/weapon-wheel.md)
42. **Cheaper moon shadows (#1369, #1371): a look by eye.** By night the moon's shadows
   reach 3,000 units in one cascade at 2048² (by day: the settings' cascades and reach,
   2048² since #1367), and while the light is under a tenth of its strength at the
   horizon there are none. It saves about 2.5 ms on the Strip at 22:00. Not checked by
   eye: walk the Strip or open desert at night and see whether shadows ending at 3,000
   units or their softer edges look wrong, and watch sunrise and sunset for a visible pop
   when shadows switch off or back on at the fade. If they do, the reach can grow or the
   moon keep the day's cascades. [graphics-enhanced.md §3.2](design/graphics-enhanced.md)

43. **The first-person arms' shadows on themselves cost about 0.75 ms (#1374).** The
   arms' shade light now draws only what lies by the eye toward the sun, and far
   shadows (hills, buildings) come from the sun's own map: 15.4 → 12.4 ms at Novac
   07:00 with finer shadows. What is left is the light's own pass, about 0.75 ms
   whatever it draws, at 4096² maps (less at 2048² since #1367); it is what lets the weapon shade the hand and a
   roof right at the head shade the arms. If you rule it out, the light goes (12.4 →
   about 11.4 ms there) and the arms take only the sun's shadows from past the body.
   See [player-body.md §1.6](design/player-body.md).
45. **A second tutorial message: pages or replace.** NVForever keeps each message
    `ShowTutorialMenu` shows as a page, with a count, Next and Previous, so none goes
    unread. In the original the second replaces the first, with none of those. Keep the pages,
    or follow the original? ([tutorial-menu §4](spec-notes/tutorial-menu.md))
47. **Nobody picks a cross on their own (#1522).** Sandboxes and Sleep or Eat searches leave
    the crucifix (marker 10) out for everyone, people as well as creatures. A package that
    names a cross or links to one still works, so the crucified stay on theirs. This is
    [design]: whether a person in the original sandboxes onto an empty cross is still
    unanswered ([observer check 86](handoff/observer-checks.md)). If you want people choosing
    crosses again, the exclusion can apply to creatures alone.
    ([furniture.md §9](spec-notes/furniture.md))
50. **Every thrown grenade with a fuse rolls (#1785).** You said only thrown frag
    grenades roll. The rule takes every hand-thrown projectile with a fuse (a Lobber
    with an Alt. Trigger timer and no proximity), not mines, C-4 or launcher rounds, since
    nothing in the data sets the frag grenade apart. If you want the frag grenade alone, the rule can key on
    its PROJ. ([combat-rules §5](spec-notes/combat-rules.md))

51. **Corpses shot again move for 1 s (#1798), decided 2026-11-08.** A corpse moves for at
   most 3 s and is then held. A shot or blow on a held corpse now shoves it and lets it
   move for at most 1 s (`MAX_AWAKE_SHOVED`; it was 3 s). A blast, a fresh death, a body
   in water or one carried keep their limits. In the 50-a-side battle the ragdoll solver
   went from 4.4–6.2 to 3.4–3.6 ms a frame. Say if re-shot corpses should move longer
   (or not at all). ([ragdoll §4](spec-notes/ragdoll.md),
   [frame-rate-audit §5](design/frame-rate-audit.md))
60. **Guards wander inside a location radius of 16 or more (#2027).** The GECK wiki says a
   guard "will wander within that radius" when it is greater than 0, and guards now do. A
   radius of 1 to 15 is left standing, as 0 is (`GUARD_WANDER_MIN_RADIUS`): its spots
   would lie within a step of the guard, so the walk would only twitch it on the spot.
   The pauses and walk pace are the sandbox's own. Say if every positive radius should
   wander (that needs a finer arrival tolerance for guard walks), or if the floor should
   differ. ([actor-behaviour §5.5](design/actor-behaviour.md),
   [packages](spec-notes/packages.md))

59. **A sandbox's only object comes back after 60 s (#1505).** Your handed-off rule: the
    object just used stays out while any other seat or idle marker is open, and when it is
    the only one it comes back 60 s after the actor began standing or wandering
    (`SOLE_OBJECT_AGAIN_AFTER`). The GECK wiki documents a setting for this,
    `iSandBoxPreventRepeatedActionTime`, default 30 s, which reads as a wait on every object
    rather than a rule for the last one left; NVForever does not read it. Keep 60 s and the
    sole-object rule, or follow the setting (30 s, from the data's GMST when a load order
    sets one)? ([actor-behaviour §5.5](design/actor-behaviour.md),
    [packages §3](spec-notes/packages.md); what the original does:
    [observer check 114](handoff/observer-checks.md))
    - [design, #2715] Idle-marker-only packages now reuse their sole eligible last marker
      without Standing; every other package retains this default (observer check 208).
57. **A Use Weapon package with Continue During Combat keeps firing at its own target
    when attacked (#2120, #2145).** With the flag, an actor attacked by the player (or
    anyone) stays at its stand and goes on firing its package's volleys at the package's
    target, not at its attacker, for as long as the use is one the shooter can act on (in
    place, its target among the drawn standing actors); with Repeat Fire that can be for
    good. When it is not (a marker target, a target not drawn, not yet at its stand) it
    turns on its attacker with the ordinary attacks, while its package still moves it (on
    to its stand, or keeping it there), not the ordinary combat movement.
    The alternative: it holds its position and fights its attacker from there. This is
    [design]: the GECK wiki's Use Weapon Package page says only "Actor will continue to
    fire and stand at the designated position, even during Combat. Great for setting up
    immobile enemies", not at whom. 43 of FalloutNV.esm's 82 Use Weapon packages set the
    flag, by the PR's count (sniper posts, scripted kills, the Ultra-Luxe kitchen's
    flamers); one whose target the shooter does not fire at (a marker) fights its attacker
    either way. The question is for the maintainer; what the original does is
    [observer check 112](handoff/observer-checks.md).
    ([use-weapon spec §3](spec-notes/use-weapon.md), [use-weapon design §3](design/use-weapon.md))
56. **A planted explosive is "Armed" and stays in the pocket (#1640).** Each live plant
    reserves one unit of the item. The victim's other units of that item stay usable.
    Nothing takes the reserved unit until its blast: not the victim (equip, use, drop,
    a grenade throw), not a script's RemoveItem or RemoveAllItems, and not the player
    taking it back from the pocket (refused, no roll). The pocket lists it as
    "<item> (Armed)". The data has no game setting for this word, and "Live" is seen
    only on the original's screen, so the word is ours (`LIVE_NOTE`). If you rule
    otherwise, the player could take it back (with or without a roll), scripts could
    remove it, or the word could change. The victim looking around and spotting you
    (item 5, second part) is not built yet.
    ([pickpocket §5](spec-notes/pickpocket.md))

70. **Children never fight (#2438).** As you decided: every actor whose race has the
    Child flag flees a fight at once and never attacks or draws, whatever its record's
    confidence, aggression or combat style; it still turns hostile, so crime and faction
    rules stay. The race's flag, not the actor's name, says who is a child. Two calls
    of ours: the child flees even before it is hurt (children take no damage, so the "once hurt" rule would never start it), and a
    scripted UseWeapon on a child in a fight no longer draws its weapon either. Observer
    check 178 asks what the original does. ([combat-ai §12.2](design/combat-ai.md))
71. **Armour only ever worn has no look on the ground (#2439).** The two floating
    backpacks at the Fort were not the slaves' packs. They are two placed references of
    the Slave Backpack armour (0x0013732C and 0x0013732D), 565 and 913 units up with
    nothing under them; the record has no ground model, so its worn model was drawn there.
    Ours now: Non-Playable armour without a ground model is not drawn or collided as a
    world object (the player can never pick it up). It is still drawn on the slaves who
    wear it. Playable armour without a ground model still lies as its worn model. If you
    would rather see these two packs, they would have to be dropped to the ground (they
    have no collision of their own). Observer check 142 asks what the original shows.
    ([simulation §3 Item models](spec-notes/simulation.md))
72. **A sprung trap stays sprung when drawn again (#2420).** Ranger Station Charlie's
    door and tripwire come in with an enable parent the station's trigger script turns
    on. The door was drawn without its door animation, so it looked shut while it was
    open and its collision had swung away; it now opens and closes as a door. The
    tripwire went off once only, but each cell load drew it armed again: its script asks
    an "Open" sequence its model does not have, and that was what it showed. Ours now
    forgets a group the model lacks and shows the one before (the sprung wire). We did
    not see the wire fire twice or a mine re-arm in our runs; if you still see fresh
    explosions on a second pass, note where you stand (the frag mines at the entrance
    and the bottle-cap mine behind the door are separate placed mines). Observer check
    143. ([doors §2](spec-notes/doors.md), [activators §5](spec-notes/activators.md))
74. **An actor whose package lies behind a disabled door stays where it is (#2566).**
    With Volare! failed (a Boomer elder killed), Loyal's only valid package is the
    B-29 hangar (NellisHangar1b), whose doors stay disabled until the B-29 is raised;
    ours had put him in there, out of the player's reach, and the quest marker looped
    between the hangar door and its inside. Now no path and no quest marker goes
    through a disabled door: Loyal sets that package aside and stays in NellisHangar2
    (the mess hall hangar), where the marker leads. If you would rather the engine
    let him in anyway and open the way for the player, say so. Observer check 160.
    ([navmesh §4](spec-notes/navmesh.md), [quests §2](spec-notes/quests-dialogue.md))
61. **Holstered fidgets come rarer than in the original (#2026).** NPCs standing with the
    weapon away now bite their nails, smoke and so on, by the idle tree's factions and
    chances. The tree is asked every 2 s, the drawn-weapon fidgets' pace
    (`FIDGET_CHECK_SECONDS`); a merchant then begins a general fidget about every 200 s,
    where the original's capture saw 3 in 360 s. The original's pace (an Energy timer) is
    observer check 116. Until it is answered: keep 2 s, or ask more often (1 s would
    double every tree fidget, the drawn-weapon and furniture ones too)?
    ([idles §11.4](spec-notes/idles.md))
64. **Scripts written for "not in the player's cell" now run there (#2445).** An actor
    the AI processes out of the loaded cells runs its GameMode block at each low-process
    update (about every 18 s, every 120 s beyond the cell buffer) and once in each waited
    hour. This applies only when the block tests that the actor is not in the player's
    cell (`GetInSameCell … == 0`). Eight object scripts in FalloutNV.esm do: Chauncey,
    ED-E, the Hidden Valley scribe and Watkins, the Vault 22 spore carriers,
    TecMineHostage and an SDO handler. Every other script runs in the loaded cells only,
    as before. Beyond the Beef needs it: Chauncey reaches the steam room only through his
    own script's warp. Running all far residents' scripts was the wider option, set aside
    for now; observer check 135 asks what the original does. (`fnv_ai::process`)
63. **Harmless hits run the target's hit scripts (#2433).** A Use Weapon package's or
    UseWeapon's Do No Damage shot or blow now runs the target's OnHit and OnHitWith blocks,
    and still deals no damage. G.I. Blues' ambush needs this: the thugs fall in their OnHit
    block for Orris, and without it he fired once and stood for good. The other harmless
    Use Weapon packages' targets now run their hit scripts too. Observer check 134 asks
    whether the original does. His aim pose (the issue's "does not aim") was not changed.
    ([use-weapon §3](spec-notes/use-weapon.md))
62. **Freeside's missing residents (#2432, #2434) taken as fixed by #2239; please
    retest on a build from 2026-11-13 or later.** Both reproduce in the engine only with
    #2239's reset of the handed-over grid squares on a change of worldspace taken out
    (Freeside's three worldspaces lie on the same squares): then the fort has 0 people
    drawn and Orris and his bodyguard are missing at the north gate, as reported. Main
    has them all, on your way there too (fast travel, King's School, bus door). v0.7.0
    (2026-11-11) predates #2239 [hypothesis: your build was older than it; the log names
    no version]. No other engine change was made; the smoke scenarios
    `freeside-fort-residents-door` and `freeside-north-residents-door` keep it fixed.
75. **A shared friend or ally faction now keeps the peace over an enemy one (#2560).**
    Vulpes Inculta at the Fort is in VCaesarsLegionFaction and in ArmorNCRFactionNVEnemy,
    and the Legion faction names ArmorNCRFactionNVEnemy an Enemy; we let the enemy pair
    win, so Caesar, Lucius and the guards killed him on sight. Ours now: when any of one
    actor's factions names one of the other's Friend or Ally, it cannot fight the other by
    faction (one direction; the
    wiki: allies "cannot attack each other under any circumstances"); crimes against the
    player still come first. This also calms other mixed groups the data builds the same
    way: the 188's shared faction, NCR troopers wearing Legion armour (both in
    ArmorVCaesarsLegionFaction), Vulpes's VCrVoidCritterFaction (Friend of animals,
    raiders and super mutants), and any actor whose disguise faction is shared with its
    watchers. A hit or a murder still starts a fight. If a fight you expect is gone (two
    sides that should fight in a group they share), say where. Observer check 177 asks
    the original. ([factions §4](spec-notes/factions.md))
78. **A turret that cannot move fires a script's weapon use from where it stands
    (#2544).** ARCHIMEDES at HELIOS One (an Immobile turret) is told by the control
    panel's script to shoot each NCR trooper in turn; ours never let a rooted actor's
    weapon use start, so it drew and holstered forever and the strike never came. Ours
    now: such a use starts at once where the turret is, and it turns to the target at
    its combat turn rate (ARCHIMEDES turns 375 degrees a second) before firing; the
    troopers' places do not matter to the script. The NCR troopers turning on the player
    before the beam is the script's own SetEnemy (23 intervals after arming), not a
    fault. Check the strike's pace looks right to you: one shot every 1-4 s, five
    troopers, then the big beam. Observer check 164 asks the original's timing.
    ([use-weapon §2](design/use-weapon.md))
76. **A line's group condition on the speaker wins over its fixed speaker (#2548).**
    Cottonwood Cove's "Halt! What business have you in Cottonwood Cove, outsider?"
    (0x0013AC97) names the AAM explorer as its fixed speaker but tests the explorers'
    faction, which all four explorers are in; the other three force-greeted the player
    with the generic "Move along." and could never be shown the Mark of Caesar. Ours
    now: a condition on the speaker naming a group (faction, class, race, voice type,
    form list) decides who may say the line, as GetIsID already did. Nineteen lines
    change: the explorers' greeting and its six answers, the Tops' random bartender (3)
    and the Strip's Securitron messenger (9, still said by the messenger). The other
    explorers speak the AAM explorer's recorded voice (the only file there is), so one
    voice comes from four men; the other choice is silent subtitles. Say if you would
    rather keep the fixed speaker. Observer check 162 asks the original.
    ([dialogue §4](spec-notes/dialogue.md))
77. **A Very Aggressive actor attacks a Neutral on sight only inside its aggro radius
    (#2547).** HELIOS One's guard dogs are in no NCR faction (Neutral to the troopers)
    and Very Aggressive, so by the wiki's rule they attack any Neutral they see; they
    went for the troopers 2,800 units away again and again and pulled them off their
    marks. Ours now: an actor with Aggro Radius Behavior on (the dogs' radius is 2,000)
    starts such a fight only inside that radius; once fighting it chases anywhere, and a
    hit or an Enemy still starts a fight at any range. This reaches 227 actor records of
    FalloutNV.esm (226 creatures: night stalkers, geckos, Mister Gutsys, fire ant queens,
    Legion mongrels …) that are Very Aggressive with a radius on; the 796 without one
    still attack every Neutral they see. The player is not affected.
    If you saw wild creatures and patrols ignore each other more than in the original,
    say where. Observer checks 144 and 163 ask the original.
    ([factions §4](spec-notes/factions.md))
79. **Companions are knocked out, not killed, outside Hardcore (#2623).** No companion
    carries the Essential flag in the data and no script sets it, so Cass, Veronica and
    Boone died in the quest pilot's fights outside Hardcore (and Veronica's death left
    the Hidden Valley bunker locked). Ours now: outside Hardcore every player teammate
    falls unconscious at zero health and gets up after the fight, still hired; in
    Hardcore it dies, as before. A mod's JIP ToggleTeammateKillable 1 makes one killable
    again. IsEssential still reads only the flag (0 for Cass). If a companion should
    still die in some case outside Hardcore, say which. Observer check 165 asks the
    original. ([unconscious §2.2a](spec-notes/unconscious.md))
82. **Anders can be cut down only after Diane's errand (#2472).** In FalloutNV.esm the
    choice "Let's get you down from there." is reached only through "Are you Anders?
    Jack and Diane sent me…", a topic that is not top-level and is learnt only from
    Diane's two lines at Red Rock Canyon (taking her errand in VMS46, or a VMS45 line).
    Before that he offers "Who did this to you?" and the drugs topic, as your log shows;
    no condition tests a Great Khans' quest stage, and no script or activator frees him
    any other way. Ours follows the data and is unchanged; after her errand the choice is
    offered, frees him and he leaves the cross. You remember freeing him at any stage: if
    you did so without having spoken to Diane, say so; observer check 175 asks the
    original. ([anders-cross](spec-notes/anders-cross.md))
83. **Anders on the cross: his head no longer turns over, and he is found on the cross
    (#2471).** His cross clips hang his head past straight down; ours read that head's
    direction from its forward alone, got the opposite way, and turned it to you upside
    down. The 60° limit did hold, measured from his chest (his body faces away from
    his reference's heading on the cross; the logged yaw was the root's). Ours now: the
    head is lifted to you right side up, within 60° of his chest (plus the talk clip's
    own nods); the same reading fixes head tracking for any hanging head. He also used
    to sidestep 8 units on a limping clip and climb onto the cross in front of you; an
    actor standing at its cross is now put on it at once. Say if the crucified should
    not turn their heads to you at all, or if you have seen the climb in the original.
    Observer check 174 asks the original. ([anders-cross-head](spec-notes/anders-cross-head.md))
81. **A faction disguise outranks the player's bad standing with that faction (#2611).**
    In NCR armour on the Legion route, every NCR soldier at Hoover Dam attacked: the
    player's enemy flag with NCRFactionNV (a crime, SetPCEnemyOfFaction) won over the
    friendship the armour's script sets up with ArmorNCRFactionNV, so nobody was fooled.
    Ours now: such a flag does not count while the actor is also in a faction that is
    the player's ally or friend (the disguise faction); only sniffers (the rangers, MPs,
    Captain Graham) within the disguise pulse's 25 ft, about 533 units, and in sight see
    through it, and then a fight they start blows the cover for the dam. A flag on the
    disguise faction itself (a murder seen while disguised) still counts. The soldiers
    it fools take the player for a friend, so a sniffer's alarm (a glimpse or a fight)
    no longer turns them on the player. If the disguise now seems too
    forgiving (it fools after any crime but one done in disguise), say so. Observer
    check 173 asks the original. ([disguises §3, §4](spec-notes/disguises.md))

80. **Scripts cannot disable the player (#2617).** On the Legion route the intake tower
    trigger VHDIntakeTowerLegionaryDespawnScript disables whoever walks in once the
    Legion's intake tower strategy is 2, the player too; a disabled player's scripted
    load doors do nothing, so the Power Plant 01 elevator never took the player through.
    Ours now: Disable of the player does nothing, and a save holding the player disabled
    loads with the player enabled. The other choice is to let the player be disabled and
    only lift the Activate rule for the player (the player would then be hidden and
    "gone" to scripts that test it). Observer check 172 asks the original.
    ([doors §6](spec-notes/doors.md))

65. **A murder's faction flag is short-lived (#2456, #2498).** A witnessed murder makes
    the player an enemy of every crime-tracking faction of the victim's, the "no side"
    armour faction ArmorVCaesarsLegionFaction too, as the original does; the flag clears
    by itself 0.18 game hours (11 game minutes) after the crime instead of after
    iCoolDownTimerSetting's 3 days. The witnesses' own grudges still last 3 days. In the
    original a Cove murder flagged both VCaesarsLegionFaction and the armour faction, and
    both read 0 again 0.18 game hours later; Karl at Red Rock then stayed seated, and
    attacks only while the armour flag is up [observed: observer check 203, 2026-11-15].
    That every crime's flag (a theft's, the Evil faction's area alert) lasts as long is a
    [hypothesis]. #2498 first proposed never flagging a faction that is no side; that
    rule is dropped. **Ruled 2026-11-16 (owner, on #2498, after check 203): "Short-lived
    flag".** Smoke `crime-karl-flag-clears`, `crime-expiry-legion-calm`.
    ([crime-karma §3.4, §3.7](spec-notes/crime-karma.md))
66. **A script package added again after it is done starts over (#2424).** The Lucky
    38 elevator Victor's script adds his talk package on every talk and waits for the
    conversation before it shows the floor box; after Stay Here a second talk opened
    nothing and the floor menu never came back. Now the same package added again once
    its goal was met begins afresh (a call while it still runs changes nothing). This
    holds for every script that re-adds a finished package, so an actor whose script
    adds a Travel package every frame after arriving would begin it again each time
    [hypothesis: no shipped script was found doing so; the original's rule is not
    documented, observer check 183]. A save made in the stuck state stays stuck: the
    script's waiting flag (delaycounter 1) is saved and only a conversation menu clears
    it, so NVForever does not mend it on load (that would mean naming this script).
    ([packages §1](spec-notes/packages.md))
67. **Idles now show over a standing actor's base idle whatever their priorities
    (#2425).** Mr. House's chamber was drawn open from the start because his sealed and
    opening idles carry lower track priorities than his base idle, whose pose is the open
    pod; the blend by priority kept the base idle. Now an idle of an actor standing still
    shows on every bone it drives (a drawn weapon's or an attack's clip still keeps its
    own priority; over a walk nothing changes). That also changes 128 other idle files of
    the base archive (Protectron, Robobrain, the penthouse screens' idles, the Hill Folk
    dances' fingers): bones they drive at a lower priority than the base idle now move
    with the idle. Please look at House's chamber (closed until the terminal, then
    opening) and, if you see one, a Protectron or Robobrain playing an idle.
    ([animation §9.4](spec-notes/animation.md))
68. **Fixtures never fight: Mr. House's screens and chamber, and Festus (#2431).** An
    Immobile creature that is Unaggressive, Helps Nobody and carries no weapon is never
    hostile, takes no combat target and never turns, whatever the player did (murder,
    hits, faction enmity). That rule, from the records alone, covers MrHouse,
    MrHouseOFFLINE, NVCRMrHouse and also NVCRFestus; turrets are untouched (they carry
    guns). The screens take no damage (their records are Invulnerable); House's body
    can still be killed once the terminal unseals the chamber, as you ruled. Please
    check Festus behaves as you expect, and the penthouse screen after House's death.
    ([actor flags §11](spec-notes/actor-flags.md))
69. **Quest targets several doors deep: which door the World Map marks (#2458).** Colonel
    Moore (For the Republic, Part 2) stands in a power plant room of Hoover Dam that no
    outdoor door leads into directly, so the World Map had no marker for her. The same
    was true of 213 quest targets indoors in the master [observed: the count the data
    test crates/fnv-game/tests/quest_marker_data.rs prints]. Now the World Map marks the
    outdoor door that starts the way in with the fewest load doors. For Moore that is
    the hatch of intake tower 3 on top of the dam (tower 3, then its elevator), not the
    observation deck's door that the quest marks while you are in the Mojave. From the
    Strip, the compass and the Local Map mark the gate to Freeside, the first door of
    the way. Fewest doors may not be the way the original picks (walking distance, or
    the doors players use); observer check 179 asks. If you prefer, the World Map could
    follow the player's own way from where they stand instead (the last outdoor door
    of that way), which can differ for buildings with several entrances.
    ([quests-dialogue §2](spec-notes/quests-dialogue.md))
- **Ragdolls for shapeless placeholder corpses (#2753).** [design, #2753] A leveled
  spawn whose choice is the invisible `nvvoid` placeholder (scale 0.1, no shapes, Health 0)
  is laid out dead by a new game and gets a two-body ragdoll like any corpse; it is now
  lifted out of the ground before it starts instead of falling through it. Nothing of it is
  seen. Ruling otherwise: such corpses are shown dead without a ragdoll (no physics, no
  fall-through warnings from them) ([physics §4.3](spec-notes/physics.md),
  [dead-on-spawn §3](spec-notes/dead-on-spawn.md))

## Not a gameplay decision: what the extender work needs from you (2026-10-25)


The extender plan (docs/handoff/extender-core-tasks.md, docs/design/extender-compat.md)
takes its facts only from the wiki and from captures of our own scripts in the original
game. Nothing here is urgent; each item unblocks part of the work.
1. **The rest of the wiki export.** `NVSE.xml` (2026-10-25) brought String Formatting,
   Hexadecimal Notation, User Defined Function, String/Array Variable, NVSE Error, the
   event priority and filtering pages and User-Defined Event, and `FOSE.xml` settled
   GetType (it is GetObjectType's alias), and you pasted Form Type IDs, the DirectX
   scancodes, the controller button and trigger codes, the known plugin names,
   GetCurrentEventName, DispatchEventAlt, sUActn and Player Character String Codes:
   thank you.
2. *Done 2026-10-25/26:* the GECK compile cases (results in ext-bytecode.md, #812), the
   probe sets with `ExtProbeList` (goldens collected; the L5.ext.nvse lab now matches
   225 of 228). Two newer GECK cases (24, 25) are in #831.
4. *Answered 2026-10-26:* target xNVSE 6.4.9.
5. *Done 2026-10-26:* the N2 scan ran over your MO2 profile `Talon` (128 plugins):
   docs/handoff/modlist-scan-2026-10-26.md. NVForever builds 44% of the extender uses
   there. The most used functions not built are the UI ones (SetUIFloat and friends,
   16 plugins). *Answered 2026-10-26:* a tile tree (§5.6 option A), to make the UI
   modular for modders; its own session writes docs/design/ui-tile-tree.md and the tasks. Your saves with their `.nvse` co-saves
   (`F:\Modding\MO2\profiles\Talon\saves`) may be read, read-only, for save rows.

## xNVSE probe results: decisions taken while you were away (2026-10-26)

The probe sets ran unattended in your game (tools/capture/ext_probe_run.py, 3 launches);
results are in the ext-* spec notes under "Observed in the original game".
- **Two probes crash xNVSE 6.4.9**, and NVForever does not copy a crash. *Decision*
  (session "NVSE core functionality tasks"): `Sv_Replace` with an empty old text replaces
  nothing and returns 0; `Ar_Range` with a step of 0 is a run-time error the script
  survives (an extender error line, as other failed calls). *Check:* say if you would
  rather mirror something else.
- **Three results differ from the GECK wiki; NVForever follows the game:** `Ar_Sort` of
  mixed types keeps the elements of the first element's type (the wiki says empty);
  `Ar_SortAlpha` sorts numbers by value (the wiki says as text); under `Begin _GameMode`
  a call's argument takes following arithmetic but not a comparison (the wiki's
  `Sin 14 == 1` example says the comparison is taken).
- **`Return 7`** compiles but returns nothing (the Call gives 0), so NVForever ignores a
  `Return` value as the game does.

## JIP LN: decisions taken while you were away (2026-10-26)

The JIP plan is docs/handoff/jip-tasks.md; read-only JIP getters were probed in your game
by runtime-compiled queries (tools/capture/jip_probes.py), results in the jip-* spec notes.
- **File functions (J9).** JIP reads and writes files in the game folder; NVForever never
  writes there. *Proposed:* writers write only under NVForever's own engine-home folder,
  mirroring the relative path; readers prefer that copy, then the game folder (read-only).
  docs/spec-notes/jip-math-utility.md §4. The same rule would cover mods' INI writes
  (SetINIFloat, the MCM settings files under `Data\Config`) and Con_SCOF's console log
  (docs/handoff/nvse-next-batch.md N3). *Check:* say if you want something else.
- **Arc tangents are approximate in JIP:** `fatan 1` and `fatan2 1 1` give 44.9998, not 45.
  *Default:* NVForever computes the exact value and the lab accepts a small tolerance on
  these two functions only, unless a sweep of probe points lets us fit the game's curve
  from observations alone. jip-math-utility.md §1.
- **`GetLNVersion` returns 57.3** in your game, where the wiki says it always returns 0;
  NVForever follows the game.
- **UI functions (J8):** *answered 2026-10-26*, a tile tree (extender-compat.md §5.6 A).
- **Two wiki pages missing from the export:** *Auxiliary Variables* and *RefMap Array*
  (export them by exact title: the 2026-11-08 category exports held only the function
  pages). *Template:ModIndexOverride* came in 2026-11-08: a private variable belongs to its
  plugin's mod index. The `*` name prefix makes a variable temporary [observed]; what makes
  it public is still open (docs/spec-notes/jip-variable-stores.md §1).
- **JIP implementation choices (J4–J7, branch claude/ext-compiler, 2026-10-26).**
  *Check* any you would rather have differently:
  - An invalid regular expression is an extender run-time error (the statement fails,
    the script goes on); the wiki says the game crashes.
  - JIP event registrations are not saved and a load starts with none (documented for
    the ragdoll event only). A second identical registration is kept once.
  - A lambda made inside a function script reads that script's variables **as they were
    when the lambda was made** (the function has ended by the time an event runs the
    lambda). The game showed the variables are seen; whether later changes are seen
    too was not observed.
  - SetGameMainLoopCallback counts frames (as documented), not time.
  - Not built yet: projectile and explosion hit data (GetHitProjectile, GetHit3DData),
    explosion damage in headless runs, the fire/reload/menu/key event firing points
    (engine side), GetFontFile (fonts, J8), the file functions (J9 waits on the
    proposal above).
- **xNVSE choices made while you were away (branch claude/ext-compiler, 2026-10-26).**
  *Check* any you would rather have differently:
  - The base-form setters (SetWeapon…, SetWeight, SetBaseItemValue, SetName …) change
    what the getters read for the session only. Combat, the menus and saves keep the
    record's values. Whether the original saves them was not observed.
  - DisableKey/DisableControl hide a key from the player's controls (the engine's
    ControlInput) and from IsKeyPressed. GetControl of an unbound control gives 255
    (the INI's "none"). Neither is observed.
  - The delayed calls count game seconds, which stop while a menu is open, so a
    CallAfterSeconds with runWhen 1 does not count down in a menu. The page says
    the time counts there too.
  - GetFirstRef and the …InCell functions treat the player's cell, and outside the
    cells within two squares of it, as loaded (uGridsToLoad 5). Elsewhere only
    persistent references count, which matches your save's rows for the saloon.
  - The remaining xNVSE functions, by family, are in docs/handoff/xnvse-remaining.md.
    The UI ones wait on the tile tree (§5.6 A, chosen 2026-10-26).

## Extender lab: rows that depend on your save (2026-10-26)

The L5.ext.nvse.* lab run of the extender branch (ext-compiler 1d2ee60b) matched 220 of
228 in-game probe rows. Three of the eight left depend on the save the probes ran in
(`nvfairport`): ToString of the player (your character's name) and the two counts of
the player's inventory (27 item kinds). `fnvtool ext probes` loads no save, so it cannot
match them. Nothing is marked or loosened in the lab: they stay failing until the lab
can start from a save. *Answered 2026-10-26:* the lab may read your save (read-only,
private, like the goldens). *Done 2026-11-11 (#1023):* every `L5.ext.*` scenario now
starts from the save its golden was recorded in: `--save {save_path}`, a documented input
of the probe sets like the load order, with the path in `lab.local.json` (gitignored;
nvfairport.fos, only read, never copied into the repo). A machine without `save_path`
gets those scenarios as errors naming the variable; nothing else changes.

## Two GECK compile cases for when you are back (2026-10-26)

Runtime-compiled scripts in your game refuse two things the GECK may accept:
`QFromEulerEx` and the other xNVSE 6.4.8 "Ex" quaternion functions, and a variable named
like a command alias (`ref rK`, ReleaseKey's alias). NVForever refuses the alias name
and accepts the Ex forms until the GECK says otherwise. Cases 24 and 25 in
`tools/capture/ext-compile/` (README there says how): note whether each saves.

## INI-file probes wrote into your game's Data folder, then cleaned up (2026-10-26)

To probe the INI-file functions (next batch N3) in your game, the probe session created
`Data\Config\` (it did not exist), put two test files there (`NVFExtProbe.ini`,
`NVFExtProbe\probe.ini`), and let the game's LU functions write to them and create
`NVFExtProbe\created.ini`. After the run the probe files were removed;
copies of what the game wrote are in `C:\fnv-private\ext\ini-files-after`. Nothing else
in Data was touched; NVForever itself still never writes there. *Answered 2026-10-26:*
probing the base game install (including writes the original game makes there) is
fine; only the NVForever application must never write into it.

*Answered 2026-10-26:* you installed MCM (MCM.dll and its ESP, not in the load order);
its script functions are probed through the DLL. MCM.dll made `Data\Config\MCM.ini` on
its own at start-up, so `Data\Config` now stays, holding only that file; the probe files
are still removed after each run.

## Probing extender state across saves needs a save in your saves folder (2026-10-26)

The extender core asked for N12 probes: set aux variables, ref maps, arrays and string
variables in the game, save, load, and read them back, to learn what the extenders keep in
a save. In the original game a save is written to `Documents\My Games\FalloutNV\Saves`,
which you have not cleared for probes the way you cleared the game install. Not done.
*Answered 2026-10-26:* yes, one save named `nvfprobe`, removed after the run, the owner's
own saves untouched.

## Code mods: the script host's language (2026-11-02)

Not a gameplay decision, and not needed before phase M5. The script host (an authoring
route with no compiler) runs either Lua or GECK/xNVSE-style script compiled by
NVForever's own compiler. Hooks for ESP scripts are a third option.
[code-mods](design/code-mods.md) §12.1, §17 item 8.
