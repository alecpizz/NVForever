# Spec notes: AI packages (PACK)

AI packages are "a bundle of instructions for actors with conditions for when and how to
execute it"; they drive most actor behaviour beyond a few defaults [documented: GECK wiki,
Category:Packages]. This file covers steps 1 to 4 of the AI package work (Layer 6): the
record layout, what the masters hold, what the documentation says about how the game
uses packages, the typed reader (`fnv_game::package`, §5), the package choice
(`fnv_game::ai`) and carrying packages out (`fnv-ai`), with their checks against the
game (§4). The design, and the list of notes to look into, is
docs/design/actor-behaviour.md.

Sources:
- **xEdit** `Core/wbDefinitionsFNV.pas` (PACK, NPC_, CREA) and `Core/wbDefinitionsCommon.pas`
  (package flags, type enum, schedule enums, idle data), branch `dev-4.1.6`, read as
  documentation; the layout is re-expressed in `schemas/fnv/ai.json`. xEdit names flags
  per game with helpers; its "FO3" names apply to New Vegas as well (`wbIsFallout3`
  covers both games).
- **GECK wiki** pages from the owner's private exports (`C:\fnv-private\geckwiki`, not
  committed): Category:Packages, How Are Packages Evaluated, one page per package type
  (Accompany, Ambush, Dialogue, Eat, Escort, Find, Flee not Combat, Follow, Guard,
  Patrol, Sandbox, Sleep, Travel, Use Item At, Use Weapon; the Ambush, Escort, Find, Flee
  and Use Item At pages are not in the export as it is on 2026-10-17, §3.1), Combat
  Package Flags, Combat Flee, Idle
  Markers, Idle Collection, XMarkerHeading, and the function pages (AddScriptPackage,
  EvaluatePackage, RemoveScriptPackage, GetCurrentAIPackage, GetCurrentAIProcedure,
  GetIsCurrentPackage, JIP's GetPackageFlag). Cited as [documented: GECK wiki, page].
- **Observations** on the owner's install (six masters, 2026-10-09 and 2026-10-10) with
  `fnvtool ai survey`, `ai packages`, `ai show` and `ai observed` (§5) and
  `fnvtool esm coverage`, and in-game captures (Layer 5; the AI observer's eleven
  Goodsprings runs, 354,983 actor samples, §4).

## 1. The record [documented: xEdit PACK]

| Subrecord | Content |
|---|---|
| EDID | Editor ID |
| PKDT | general flags u32; type u8; 1 unused byte; Fallout behaviour flags u16; type-specific flags u16; 2 unused bytes. The last three fields are optional (8-byte PKDT) |
| PLDT | location 1: type i32, location (4 bytes, meaning by type), radius i32 |
| PLD2 | location 2, same layout. xEdit lists it twice, and every master record that has one stores it twice, the second copy later in the record, both copies identical [observed: 236 packages, `location2_copies`] |
| PSDT | schedule: month i8, day of week i8, date i8, hour i8, duration in hours u32 |
| PTDT | target 1: type i32, target (4 bytes, meaning by type), count / distance i32, an unknown f32 (optional) |
| CTDA | conditions (the shared condition layout) |
| IDLF, IDLC, IDLT, IDLA | the package's idle collection: flags (Run In Sequence, Do Once), count, idle timer, IDLE FormIDs |
| CNAM | combat style (CSTY) |
| PKED, PUID, PKAM | markers: eat, use item, ambush (empty) |
| PKE2 | escort distance u32 |
| PKFD | follow: start-location trigger radius f32 |
| PKPT | patrol: repeatable (bool u8), 1 unused byte |
| PKW3 | use weapon data: flags (Always Hit, Do No Damage, Crouch To Reload, Hold Fire When Blocked), fire rate, fire count, bursts, shots per volley min/max, pause between volleys min/max |
| PTD2 | target 2, as PTDT |
| PKDD | dialogue data: FOV f32, topic, flags (No Headtracking, Don't Control Target Movement), then optional unused 4 bytes, dialogue type (Conversation / Say To), 4 unknown bytes |
| POBA, POEA, POCA | begin / end / change groups, each: the marker, INAM idle, an embedded script (SCHR, SCDA, SCTX, SCRO…), TNAM topic |

Enumerations and flags:
- **Package type** (PKDT): 0 Find, 1 Follow, 2 Escort, 3 Eat, 4 Sleep, 5 Wander, 6 Travel,
  7 Accompany, 8 Use Item At, 9 Ambush, 10 Flee Not Combat, 12 Sandbox, 13 Patrol,
  14 Guard, 15 Dialogue, 16 Use Weapon (11 is Oblivion's Cast Magic). **This is not the
  numbering `GetCurrentAIPackage` returns** (0 EXPLORE … 14 TRAVEL … 36 SANDBOX, 37 PATROL
  [documented: GECK wiki, GetCurrentAIPackage]), which the AI observer records. The two
  are paired by name (`PackageType::runtime_code`): Follow 1, Escort 2, Eat 3, Sleep 4,
  Wander 13, Travel 14, Accompany 15, Use Item At 16, Ambush 17, Flee Not Combat 18,
  Use Weapon 33, Dialogue 34, Guard 35, Sandbox 36, Patrol 37; ten of the fifteen pairs
  were seen in the game (§4), and Escort and Use Item At since 2026-10-17. Find has no
  entry of its name in the function's table; a Find package ran as 0 ("EXPLORE"), which
  `runtime_code` now gives it [observed 2026-10-17, §4].
- **General flags** (PKDT): 0 Offers Services, 1 Must Reach Location, 2 Must Complete,
  3–5 Lock Doors At Package Start / End / Location, 6–8 Unlock Doors At Package Start /
  End / Location, 9 Continue If PC Near, 10 Once Per Day, 12 Skip Fallout Behavior,
  13 Always Run, 17 Always Sneak, 18 Allow Swimming, 19 Allow Falls, 20 Head-Tracking Off,
  21 Weapons Unequipped, 22 Defensive Combat, 23 Weapon Drawn, 24 No Idle Anims,
  25 Pretend In Combat, 26 Continue During Combat, 27 No Combat Alert, 28 No Warn/Attack
  Behavior. JIP's GetPackageFlag list agrees bit for bit except bit 20, which it calls
  "Armor Unequipped" (Oblivion's name in xEdit), adds 11 "Created" and 14 "Script Package"
  [documented: GECK wiki, GetPackageFlag]. Bits 11, 14–16 and 29–31 are set in no
  master package [observed: `general_flag_bits_unnamed` empty].
- **Fallout behaviour flags**: 0 Hellos To Player, 1 Random Conversations, 2 Observe
  Combat Behavior, 3 Unknown, 4 Reaction To Player Actions, 5 Friendly Fire Comments,
  6 Aggro Radius Behavior, 7 Allow Idle Chatter, 8 Avoid Radiation.
  **They count only with general bit 12** [hypothesis, from the data and an owner
  statement]: of the 4,163 packages of FalloutNV.esm, the 1,685 with bit 12 hold varied
  behaviour flags (812 some, 873 none), while the 2,478 without it hold none (2,237) or
  always the same 0x1CD (241) [observed: `fnvtool ai show` over every PACK]: an edited
  set against an untouched default. Vault 3's Fiends, whose packages
  (DefaultSandboxEditorLocation512, DefaultPatrolCasual) have bit 12 clear and no
  behaviour flag, "talk a lot" [owner statement 2026-10-11]. So bit 12 ("Skip Fallout
  Behavior" in xEdit; the GECK's "Enable Fallout Behavior" box, Category:Packages) means
  the package replaces the default with its own flags, and without it the default
  allows hellos and random conversations. Used by `fnv_ai::social` (dialogue.md §5.2).
- **Type-specific flags**, by package type: Find, Escort, Eat: 8–10 Allow Buying / Killing
  / Stealing; Wander and Sandbox: 0–5 No Eating / Sleeping / Conversation / Idle Markers /
  Furniture / Wandering; Use Item At: 2 Sit Down plus 8–10; Ambush: 0 Hide While
  Ambushing; Guard: 3 Remain Near Reference to Guard. Kept raw in the schema
  (`type_specific_flags_raw`): the schema language cannot pick a layout by the type.
- **Location type**: 0 near reference, 1 in cell, 2 near current location, 3 near editor
  location, 4 object ID, 5 object type, 6 near linked reference, 7 at package location.
  The 4-byte location is a reference (0), a cell (1), a base object (4) or an object-type
  code (5), else unused; kept raw (`location_raw`).
- **Target type**: 0 specific reference, 1 object ID, 2 object type, 3 linked reference;
  the target is a reference, a base object or an object-type code (`target_raw`).
- **Object type** (locations and targets) [documented: xEdit wbObjectTypeEnum]: 0 None,
  1 Activators, 2 Armor, 3 Books, 4 Clothing, 5 Containers, 6 Doors, 7 Ingredients,
  8 Lights, 9 Misc, 10 Flora, 11 Furniture, 12 Weapons: Any, 13 Ammo, 14 NPCs,
  15 Creatures, 16 Keys, 17 Alchemy, 18 Food, 19 All: Combat Wearable, 20 All: Wearable,
  21 Weapons: Ranged, 22 Weapons: Melee, 23 Weapons: None, 24–27 Actor Effects (Any,
  Range Target, Range Touch, Range Self), 28 Actors: Any.
- **Schedule**: month −1 any, 0–11 January–December, 12–15 spring, summer, autumn,
  winter (zero-based for form versions below 122, i.e. every FNV record); day of week −1
  any, 0–6 Sunday–Saturday, 7 weekdays, 8 weekends, 9 Monday/Wednesday/Friday,
  10 Tuesday/Thursday; date 0 any, else the day of the month; hour −1 any, else 0–23;
  duration in hours.
- **Package lists**: NPC_ and CREA hold their packages as repeated PKID, in order (not
  sorted). An actor whose template flags include "AI Packages" takes its template's list
  (already read by `fnv-game`, `Game::package_list`).

`schemas/fnv/ai.json` holds the layout (and IDLM's idle data, which shares it). With it,
every PACK and IDLM subrecord of the six masters decodes with nothing left over
(`fnvtool esm coverage`; `fnvtool ai survey` reports no subrecord not OK) [observed].

## 2. What the masters hold [observed: `fnvtool ai survey`, six masters]

- **4,885 packages** (FalloutNV.esm 4,160, DeadMoney 144, HonestHearts 367,
  OldWorldBlues 72, LonesomeRoad 142). 4,872 have a 12-byte PKDT, 13 an 8-byte one
  (which ends after the Fallout behaviour flags: no type-specific flags).
- **By type**: Travel 1,486, Sandbox 791, Patrol 629, Sleep 406, Dialogue 405, Eat 347,
  Guard 242, Use Weapon 173, Follow 166, Find 88, Ambush 44, Wander 32, Use Item At 30,
  Accompany 18, Flee Not Combat 15, Escort 13.
- **Schedules**: month is always "any" and date always 0. 1,483 packages set an hour
  (most often 22, 6, 8, 0, 18, 20); 53 a day of the week. Duration: median 0, 90th
  percentile 9 hours, maximum 9,999. Packages with any schedule: Sleep 379, Travel 371,
  Eat 334, Sandbox 325, Patrol 281, others fewer.
- **Locations**: 4,464 packages have a location 1 (PLDT) and 236 a location 2 (472 PLD2
  subrecords: two identical copies each; the second copy follows PKDD, PKED, PKAM, PTD2
  or, in 8 packages, PKDT). Location 2 is on Dialogue (114), Eat (69, mostly "at package
  location"), Ambush (44) and Use Weapon (9) packages. Location references are placed
  references (2,238 REFR, 34 ACHR, 18 ACRE), cells (163), and 38 references that no
  master defines. Radius: median 10, 90th percentile 800, maximum 50,000. Travel
  locations are mostly "near reference" (898) or "near editor location" (414); Sandbox
  "near editor location" (439); Patrol "near reference" (453) or "near linked
  reference" (112).
- **Patrol routes**: of the 453 patrols located near a reference, that reference starts
  a chain of linked references (XLKR) in 394 (281 chains end, 113 loop back; median 4
  points, at most 29) and has no linked reference in 59. 112 patrols are located "near
  linked reference" (the actor's own), 61 near the editor location, 1 near the current
  location, 2 have no location [observed: `fnvtool ai packages` patrol_routes]. 488
  patrols are repeatable, 141 not (PKPT).
- **Targets**: 1,503 PTDT, 165 PTD2. Specific-reference targets are actors (238 ACHR,
  33 ACRE), references (170), and 511 times `0x00000014`, the player's reference, which
  no plugin defines (the game provides it). Use Weapon's second target is always a
  specific reference. Object-type targets: Eat 335 (260 "None", 75 Food), Find 50
  (Furniture 16, Food 12, Flora 7, …), Use Weapon 52 (32 "Weapons: None", 10 "None",
  7 melee, …), Use Item At 13 (all Furniture), Guard 3 ("None").
- **Conditions**: 3,801 CTDA; a package has 0 (median) to 12. The most used functions:
  GetQuestVariable 1,373, GetScriptVariable 683, GetIsReference 363, GetStage 284,
  GetDistance 112, GetInCell 109, GetDead 100, GetStageDone 92, GetCurrentTime 88,
  GetRandomPercent 64. Run on: subject 3,540, reference 197, target 25.
- **Begin / end / change**: 4,872 packages carry all three groups; compiled result
  scripts: begin 162, end 339, change 57; idles 51 / 25 / 23; topics 59 / 18 / 3.
- **Flags in use** (examples): Dialogue packages mostly Skip Fallout Behavior (228) and
  Must Complete (209); Follow mostly Defensive Combat (143); Eat often Must Reach
  Location (100). The type-specific bits 0–5 (Sandbox's No Eating … No Wandering) also
  appear on Travel (up to 86), Guard (65), Dialogue, Follow, Sleep and Patrol packages,
  where xEdit names no type-specific flags: probably values kept from another package
  type in the editor [hypothesis]. Counted only where documented for the type: Sandbox
  No Sleeping 486, No Eating 326, No Wandering 302, No Conversation 137, No Furniture
  123, No Idle Markers 85; Eat Allow Buying 49; Ambush Hide While Ambushing 30; Find
  Allow Killing 21; Use Item At Sit Down 4; Guard Remain Near Reference 3.
- **Dialogue packages** [observed: `fnvtool ai show` over the 332 of FalloutNV.esm,
  2026-10-10]:
  - **Whom**: every one targets a specific reference: the player in 293 (277
    conversations, 16 Say To), another reference in 39 (35 conversations, 4 Say To).
    Every Say To package names a topic. Of the 277 conversations with the player, 111
    name GREETING and 67 no topic.
  - **Count / distance** (the "Activate Distance", §3) of the 293: 0 in 54; else 64 to
    5,000, most often 256 (39), 512 (30), 128 (16), 1,024 (16), 500 (15).
  - **Locations** of the 293: neither in 156; location 1 only in 44; both in 93 (never
    location 2 alone). With both, location 1 has radius 0 in 66 and a radius in 27.
  - **What location 2 is** (the "Trigger Location"): near a reference in 61, near the
    editor location in 18, near the current location in 9, in a cell in 4, near the
    linked reference in 1. Of the references, **those with radius 0 are box primitives
    in 32 of 33** (activators without a model or script, placed around the spot, such
    as SunnySmilesTrigger, CaesarDialogueTrigger, GomorrahCasinoEnterTrigger; the 33rd
    is a creature); of the 29 with a radius, 16 are markers, 5 box primitives, 3 the
    player, 3 a creature, 1 furniture and 1 an idle marker.
  - **Flags**: Must Complete 150 and Always Run 132 of the 293; PKDD "Don't Control
    Target Movement" 26 and "No Headtracking" 1 of the 332.
  - **Sunny Smiles** (the first package of GSSunnySmiles's list,
    SunnyMeetPlayerDialoguePackage 0x00105CC7): a conversation with the player, topic
    GREETING, distance 256; location 1 the editor location, radius 0; location 2 the
    reference 0x0015D9BA, radius 0: a SunnySmilesTrigger box of half extents
    243 × 269 × 111 centred 101 units from where she stands (230.9, −492.7, 3456) in
    GSProspectorSaloonInterior; flag Skip Fallout Behavior; one condition,
    VFreeformGoodsprings.bMetSunny (variable 48) == 0. The saloon's front door puts
    the player at (133.0, −821.7, 3456): inside the box, 343 units from her. The line
    she then says (INFO 0x00104E7E, "Cheyenne, stay…", Say Once) sets bMetSunny to 1
    in its end script, so the package is no longer valid afterwards. No script and no
    trigger block is involved: the box has no script.
- **Other data**: 267 packages carry an idle collection (25 run in sequence, 22 do
  once); 412 dialogue data (336 name a topic, 35 are "Say To"); 175 use-weapon data;
  169 a follow trigger radius; 13 an escort distance; 16 a combat style.
- **Package lists**: 4,220 NPC_ (median 2 packages, at most 37) and 2,235 CREA (median 2,
  at most 16) bases; 1,871 take their template's list. Every list entry is a PACK.
  2,015 packages are in no list (script packages, dialogue packages added by scripts,
  unused). The most listed: DefaultSandboxEditorLocation512 (1,853 lists),
  DefaultPatrolCasual (1,092), DefaultPatrolWeaponDrawn (850),
  DefaultSandboxEditorLocation1024 (319). The most common first package is
  DefaultPatrolCasual (1,056 bases), then DefaultPatrolWeaponDrawn (772): a patrol from
  the actor's linked reference, which the game skips for actors without one (§3).
- 7,681 placed actors (ACHR, ACRE); the base of 6,465 lists packages itself.
- Game settings the wiki names for sandboxing, trespass warnings and ambushes are not in
  the masters (only `iGuardWarnings` = 2 is): the game's own defaults apply, which are in
  the executable and are not read [observed: `fnvtool esm decode`].

## 3. How the game uses packages [documented: GECK wiki]

**Choosing** (How Are Packages Evaluated, Category:Packages):
- The package list is searched top to bottom; the first package whose **schedule** and
  **conditions** are satisfied is used. The schedule is an implicit condition; the
  conditions apply only when the schedule is valid. "Run on Target" in conditions is valid
  only when the package has a target.
- Packages "seem to be re-evaluated every 10 seconds", when a package completes, after the
  player waits (actors may then be moved to their package's location by unknown rules),
  and on `EvaluatePackage` / `ResetAI`.
- Schedules are in whole hours; either day and month, or day of the week.
- Patrol (and Ambush with a linked-reference location) packages whose actor has no
  linked reference are invalid and are skipped. This holds for patrols located "near
  linked reference" (the default patrols); a patrol located near a named reference
  starts there and does not need the actor's own link [observed: GSVictorRef has no
  linked reference and ran VGSVictorCircuitPatrol, whose location is a patrol marker
  starting a chain of 11 linked references, for 13,225 samples, §4].
- A script package (`AddScriptPackage`) takes precedence over all other packages until
  it is done; an actor has one at a time; without Must Complete / Must Reach Location it
  is removed at the next evaluation; it is saved with the game.
- The same package added again while it is still the actor's current one and its goal
  was met (no choice has dropped it yet) begins afresh: the goal is forgotten and the AI
  takes it up anew, as after ResetAI; added while it still runs, nothing changes
  [hypothesis: the GECK wiki's "until it is done" read per call; observer check 183].
  The Lucky 38 elevator Victor needs it: his OnActivate adds his Dialogue package on
  every talk and waits for the conversation before showing the floor box, and a second
  talk soon after Stay Here otherwise opened none, for good [observed:
  Lucky38VictorElevatorScript, SCPT 0x0015725D in FalloutNV.esm; issue #2424].
- `EvaluatePackage` drops even a Must Complete / Must Reach Location package and chooses
  again.
- Flags that shape choosing: **Must Complete** (does not end until its conditions are met:
  the target found, the location reached…), **Must Reach Location**, **Continue If PC
  Near** (does not end while the player is in the same cell: shops stay open),
  **Once Per Day** (not run again within 24 hours of first being selected).

In `fnv-game` (`ai.rs`; the rules one by one with their sources:
docs/design/actor-behaviour.md §3): a script package first; else the first package of
the list (the actor's own, or that of its "AI Packages" template, followed through
templates and leveled actor lists with the pick the actor is drawn with, as its factions
are: factions.md §3; before 2026-10-11 a leveled base or template gave no list, which
left 450 placed actors of FalloutNV.esm without packages) that is in its schedule, has the linked reference it needs, is not a Once Per
Day package chosen within the last 24 hours, and whose conditions pass (the actor as
the subject, the package's target as the target); with none valid the actor keeps its
current list package (§4). `EvaluatePackage` always chooses; the periodic choice leaves
a Must Complete / Must Reach Location package that has not met its goal, and a Continue
If PC Near package while the player is in the actor's cell.

**Locations** ("Standard Location"): near editor location (default), near current
location, in cell (radius ignored), near reference; the radius is editable.

**Per type** (one page each), the parts that matter for movement:
- **Travel**: go to the destination, using navmesh preferred triangles "but not in
  interiors"; stop at the edge of the destination radius; the destination must be on the
  navmesh. Package idles play at the destination.
  - **At an xMarkerHeading** (built 2026-11-08) [documented: GECK wiki, NPC population,
    level-design.md: the heading version "shows which way an actor standing there will
    face"]: an actor whose package location resolves to a placed xMarkerHeading (the
    engine's base 0x34) turns, once there, to the marker's heading at the standing turn
    rate (`fnv_ai` `marker_heading`, set in `settle`, for every package type that
    settles there; a sandbox keeps its own). Owner report (TTW, the G.O.A.T.): Mr.
    Brotch at `CG03MrBrotchTeachMarker` (heading 22°, towards the desks) faced the lab
    bench, the heading he walked in with. Whether the original turns for a radius other
    than 0 (his `CG03MrBrotchGiveGOAT` has 20) is not known; ours turns for any.
    [design, #2526: `fnv_ai::goal::location_reference`, `carry::marker_heading`]
    Near reference, near linked reference, near object ID and near object type share
    the destination-reference resolver. `Goal::Near` retains the selected reference
    alongside the travel position; arrival facing reads it without repeating the
    nearest-object search from the actor's new position. Guard with Remain Near
    Reference to Guard retains its effective target, so an unrelated location marker
    cannot supply its heading. Plain XMarkers
    keep the walker's arrival heading; furniture, sandbox and guard-wander handling
    retain precedence. [documented: #2526 writer brief, main session's fnvtool esm
    decode / ai show of FalloutNV.esm] Kimball's guards `VHDRangerKimballGuard01REF`
    (0012FCD4) and `02REF` (0012FCD5) use Travel `VHDKimballGuardSpeechIdlePackage`
    (00131F13), near linked reference, radius 0; their linked stage markers 00131F0E/F
    are xMarkerHeading base 00000034, z rotation 3.3832 rad.
  - **At the editor position** [hypothesis, #2526, observer check 213]: Travel and
    standing Guard with a near-editor location radius below `GUARD_WANDER_MIN_RADIUS`
    (16 units, the existing #2027 cutoff) face the actor's placed editor z rotation.
    [design: `carry::settle`, `marker_heading`] This excludes sandbox/wander and Guard
    remaining near its target; it turns at the existing standing rate. A runtime-created
    actor without a placed editor rotation has no arrival heading supplied by this rule.
    [documented: #2526 supplied ai-show evidence] The speech crowd's
    `VHDCivilianIdlePackage` (00133EC3) is Travel near editor location, radius 0.
  - **Guard package selection audit** [documented: #2526 supplied `2526-ai.json`,
    `2526-guard-packages.json` and w2 brief; main-session fnvtool ai show and quest-script
    SLSD/SCVR decode of FalloutNV.esm, 2026-11-15]: both guards' list order is StageFront
    (00135004), KimballDead (00134B20), SpeechIdle (00131F13), Saferoom01/02
    (00133EB6/7), then Follow01/02 (0013259E / 00131F12). All conditions are AND,
    run on subject. VMQ03a (00131F08, script 00131F0A) variable indices are
    `iKimballFlee` 2, `bKimballSaferoom` 3, `AssassinAttack` 5,
    `iVertibirdBombActive` 9, `iDetonatorFound` 11, `bKimballDead` 26;
    VHDKimballSpeech (00130C78, script 00130C77) `iKimballStage` is index 4.
    With `iKimballStage = 2` and these VMQ03a variables at 0:
    - StageFront requires flee != 1, detonator < 2, bomb < 3, attack == 2 and
      GetIsReference(guard02) == 1. It fails attack == 2 for both, and the reference
      condition also excludes guard01.
    - KimballDead requires dead == 1 and fails; SpeechIdle requires dead == 0,
      flee == 0 and speech stage == 2, so passes for both and wins before the rest.
    - Saferoom packages require saferoom == 1, flee == 1 and their own guard reference;
      Follow packages require only their own guard reference. Neither can displace a
      valid earlier SpeechIdle. During attack == 2, StageFront can win for guard02
      when its other conjuncts pass; it never wins for guard01.
    [documented: supplied `2526-records.md`, SCPT 00145EC2] Moore's introduction sets
    the speech variable to 2 and reevaluates both guards. [design: `fnv_game::ai`
    `select_package`, `check_package`; `game::eval_conditions`; `functions::quest`
    `get_quest_variable`; `functions::refs::get_is_reference`] The evaluator reads
    quest instance variables by index, compares the subject for GetIsReference and
    chooses the first valid package. Synthetic `fnv-ai/tests/carry.rs` coverage checks
    the supplied conjunctions/order for both guards, including attack, death, flee and
    saferoom transitions: no condition-evaluation bug is established. The selected
    SpeechIdle destination is each guard's linked stage marker; retaining its heading
    fixes the demonstrated facing defect. [design: tools/smoke/scenarios.toml]
    `scene-kimball-guard-natural-heading` stages these quest variables and asserts both
    natural SpeechIdle selections and arrival headings without AddScriptPackage;
    `scene-kimball-guard-heading` remains the focused forced-package arrival check.
- **Sandbox**: ambient behaviour within the location radius: it scores nearby objects
  (food, furniture, idle markers, other actors), weights them by a per-kind probability
  that drops to zero for the kind just chosen and rises for the others; sleep start
  19:00–01:00 and duration 6–10 h rolled when the package starts; breakfast 6–9, lunch
  11–14, dinner 17–20, meals 0.5–2 h (defaults of the fSandbox… settings); a sandbox
  near the start of a linked patrol route sandboxes at each point for its idle time.
  Repeating an action: the setting `iSandBoxPreventRepeatedActionTime` (default 30)
  keeps a sandboxing NPC from repeating an action on the same object until that many
  seconds have passed [documented: GECK wiki, iSandBoxPreventRepeatedActionTime]. The
  page does not say when the seconds start (the action's start or its end) or what the
  NPC does in the meantime when nothing else is open. NVForever does not read this
  setting: its own rule for the object just used, and for the only object open
  (60 s, issue #1505), is a design choice under the maintainer's review
  (docs/design/actor-behaviour.md §5.5, docs/owner-review.md; observer check 114 asks the original).
  The action-type rule permits repetition “unless it has no other options”
  [documented: GECK wiki, [Sandbox Package, Action Selection](https://geckwiki.com/index.php?title=Sandbox_Package),
  offline revision 32636]. An idle-marker-only package with just its last marker open
  reuses that marker at expiry without Standing [design, #2715:
  `fnv_ai::sandbox::Allowed::only_idle_markers`, `Sandbox::next`]. All five other
  behaviours must be disabled; permitting one retains the default even when it has
  no available choices [design, #2715: `Sandbox::next`]. Pacer's cower package permits only idle markers and names
  VFSNCRFightPacerMarkerREF (REFR 0x001293EC), DefaultCowerIdleMarker (IDLM 0x000C47CC,
  timer 0), with LooseCowerIdles (0x000C0A9A, looping 3rdP_Cowering.kf)
  [documented: #2715 issue comment, supplied record findings]. That he stays cowering
  throughout the original's staged fight is unmeasured [hypothesis; observer check 208].
- **Patrol**: walk the chain of linked references; repeatable patrols loop (or go back
  and forth) and start at the nearest point; non-repeatable ones start at the first point
  and are done at the last; data on the points (Idle Markers, XMarkerHeading: idle time,
  topic, result script, facing). In New Vegas a patrolling actor never re-evaluates its
  package list.
- **Sleep**: find a bed within the location radius and sleep for the duration.
- **Eat**: travel to the eat location and eat (sitting if furniture is free), search for
  food if allowed, else wander there.
- **Guard**: stand at the guard location (wander within its radius), warn intruders near
  the guarded reference (topic GuardTrespass, `fAITrespassWarningTimer` 5 s,
  `iGuardWarnings`), then attack. Built 2026-10-24 (`fnv_game::aggro`, "Guard
  packages"; combat-ai.md §13.1): the guarded reference is the target (a reference or the
  linked reference), the area its Count / Distance, else the location radius
  [hypothesis]; "No Warn/Attack Behavior" turns it off. **Taken out 2026-10-25**: in the
  original, Gomorrah's room guards on vGomorrahGuardPackage did not warn a player by
  their door (capture `warnings2`, combat-ai.md §13.1). Of the 175 Guard packages of
  FalloutNV.esm 114 set that flag; the 61 that warn include vGomorrahGuardPackage (the
  Gomorrah bouncers', room guards' and cash-room guard's: each stands 140–210 units from
  the door it guards, its linked reference, with a target distance of 25),
  DefaultGuardNPC (Strip securitrons, super mutant guards; 128), the Nellis Boomer tower
  guards (256 around a tower marker), PowderGangerGuardMixedPackage (linked reference,
  distance 0, radius 256 around the editor location) and V22MantisFemaleEggGuardPackage
  (300) [observed: `fnvtool ai show`]. **The wander, built 2026-11-11 (#2027)**: "If it
  has a radius greater than 0, the actor will wander within that radius" [documented:
  GECK wiki, Guard Package, Guard Location]; the location's radius, not the guarded
  reference's guard radius (Count / Distance, for intruders). Of FalloutNV.esm's Guard
  packages 115 have a location radius, used by 149 actor bases and 430 placed actors
  [observed: the issue's census]. In the original two settlers on
  DefaultGuardStayAtPosition01 (near editor location, 128) kept moving inside an area of
  about that size, procedure 44 GUARD throughout [observed: AI observer 2026-11-10, issue
  #2027]. Here (`fnv_ai::carry::guard_wander_radius`, `Sandbox::guard`) a guard that has
  arrived wanders as a sandbox with only wandering on (actor-behaviour.md §5.5); a radius
  of 0, and a Guard with Remain Near Reference to Guard (§7.7), stand as before
  [design, #2027]. How often and how far it moves is ours (observer check 115).
- **Follow / Accompany / Escort**: keep a distance to a target actor (follow walks
  behind it; accompany paths to its destination; escort leads it to a destination);
  follow can have start and end locations.
- **Find / Use Item At / Use Weapon / Ambush / Flee Not Combat / Wander**:
  search for and pick up / activate objects; go and use an item or furniture; shoot at
  a target with the PKW3 fire pattern; wait at a location until enemies enter another;
  flee to or from somewhere; wander.
- **Dialogue** [documented: GECK wiki, Dialogue Package]: gets an actor "to engage in
  a specific conversation with the player or another NPC".
  - The **target** is whom to talk to; its distance is the **Activate Distance**: "how
    close to the target does the actor needs to be before initiating dialogue. The
    standard distance is 150, as defined in the iActivatePickLength game setting".
  - **Dialogue type**: Conversation ("if the target is the player, the NPC will
    force-greet the player") or Say To (one line, no conversation and no dialogue menu).
  - Location 1 is the **Start/Wait Location**: the actor moves there when the package
    starts and "will wander within that radius while waiting". Location 2 is the
    **Trigger Location**: "where the target must be to trigger the actor to begin
    moving towards the target to initiate dialogue"; its radius: "when the target gets
    this close to the trigger reference".
  - **Behaviour**, once the actor is at the wait location: (1) a trigger location and a
    wait location with a radius: the actor stays until the target enters the trigger
    location, then moves towards the target and talks at the activate distance; (2) a
    trigger location and a wait location of radius 0: the actor "will stay at the Wait
    Location until the target reaches the Activate Distance" and talks; (3) no trigger
    location: the actor "will immediately begin moving towards the target" and talks at
    the activate distance.
  - **Do Not Control Target Movement** off: with a trigger location the target "will
    walk towards that location when the dialogue package starts", without one it "will
    stand still and wait for the actor to reach the activation distance". NVForever
    walks the player for case 2 (a wait location of radius 0) with the flag off and
    the player as the target: once the player is within the activate distance and in
    the actor's sight but outside the trigger location (a box or a radius), the player
    walks, at walking pace and with the movement keys set aside, to the nearest point
    48 units inside the box (or within three quarters of the radius), and the
    conversation opens there (`fnv_ai` `lead_player`, `Game::player_lead`, nvf-play
    `dialogue_lead_walks`) [hypothesis: when the walk starts, its pace and where it
    ends; observer check 186]. A walk that gets no nearer its point (8 units) for 1.5 s,
    something solid in the way, ends and the actor talks where the player stands
    (`fnv_ai::carry::LEAD_STUCK_SECONDS`) [our choice: the player is never held walking
    into a wall]. A trigger location that is a cell, or a reference with
    neither radius nor box, keeps the plain case 2. Within `LEAD_AGAIN_SECONDS` (30 s) of a conversation
    from its Dialogue package the same actor does not walk the player in again; it
    talks where the player stands [our choice, #3464; observer check 280]. Harland in
    the REPCONN basement (REPCONHarlandDialogPackage 0x00080773: wait location the
    XMarker 0x0008F0C2 radius 0, trigger location the doorway trigger 0x00080775,
    activate distance 1200, its end script sets the trigger's `HarlandTalked` to 2) has
    a second trigger on the way out, REPCONHarlandResetTrigger 0x0008F725, 370 units past
    the doorway: `OnTriggerEnter player` calls `HarlandREF.ResetAI` and sets
    `HarlandTalked` to 0 [observed: `fnvtool script decompile --form 0x0008F6FE`]. The
    package taken up afresh found the leaving player within 1200 units and outside the
    doorway box, walked them back into it, greeted, and so on for every try: the player
    could not leave. Scripts count on it: Paladin Ramos's "Follow me" (INFO
    000A3CE0, HVRamosEntryDialogPackage, activate distance 300) disables the controls
    and leaves the escort to the OnTrigger block of HVRamosTargetDialogTrigger, which
    moves Ramos and the player to HiddenValley02 and gives the controls back only
    while the player is inside the box; the box ends 218 units from Ramos's wait
    marker, so a player greeted at 296 units stood outside it for good (issue #2453)
    [observed: the scripts, `fnvtool script decompile --form 15E57A`]. **No
    HeadTracking**: speaker and target "will not be forced to look at each other by the
    package". FOV: the dialogue menu's zoom when the target is the player. Topic: the
    first topic of a conversation between two NPCs, or the Say To line.
  - **A target other than the player** (an actor, or a talking activator, which speaks
    for its talking actor): the actor goes up to it as to the player and starts a
    conversation between actors (`Game::start_actor_conversation`, dialogue.md §5.1);
    the package lasts while the two talk (issue #2451: Veronica's
    FollowersVeronicaForceGreetIntercom, target HVIntercomREF). Only the player is
    measured for a clear line. Before, such packages were parked "not carried out"; the
    masters' others are the Strip's VStripSS1* street scenes and
    CaptainParkerArrestKeith [observed: `fnvtool ai show`].
  - Must Complete: the package "will not reevaluate until actor has engaged in
    dialogue with the target".
  - **A wait location that is furniture**: the actor sits on it and talks from its
    seat; it does not go up to the target, even without a trigger location (case 3),
    and waits until the target is within the activate distance [observed: the owner's
    frame from the original, 2026-11-03, issue #1474: Benny on his bar stool in the
    Tops' presidential suite greets the player, who is close by with objective 20 (go
    to the suite) just completed, and stays seated and facing the bar while the
    greeting plays; the owner: "he never gets up".
    His package VTopsBennyPresidentialTalk: wait location near the reference
    0x001110C4 (DinerStool01L, FURN) with radius 128, no trigger location, the player
    within 256, Must Complete, PKDD Don't Control Target Movement; the stool is about
    630 units from the ambush trigger the player arrives in. hypothesis: that every
    actor waiting on furniture does the same, and that only its head turns to the
    target; the wiki text quoted above does not mention furniture.]

**Doors and packages**: the door flags lock or unlock all doors of the cell at the start,
the end, or while at the location. Known New Vegas bugs listed on the wiki: door flags
"can behave unexpectedly" and door locking packages "do not work correctly"; "cell
orientation matters when pathing through doors" (north markers); unloaded actors "may
not run their packages"; z height checks for destinations are broken; actors with "No
Low Level Processing" travel only while the player is near.

**Procedures**: a package runs procedures (`GetCurrentAIProcedure`, 0 TRAVEL … 45 SANDBOX,
46 USE_IDLE_MARKER, 48 SITTING …): a Travel package runs TRAVEL while moving, then WAIT
at the destination; Fallout behaviour can start a procedure without changing the package
[documented: GECK wiki, GetCurrentAIProcedure].

### 3.1 Escort, Find, Use Item At, Ambush and Flee Not Combat

The owner's GECK wiki export has no page of its own for these five types (the links of
Category:Packages, "Escort Package", "Find Package" and so on, lead to pages that are not
in it) [observed 2026-10-17: no page titled so in `C:\fnv-private\geckwiki`]. What it
says of them is on Category:Packages, Combat Package Flags, Combat Flee, the function
pages and the records themselves; the rest is marked [hypothesis] or [our choice].
Carried out in `fnv_ai::errand` (goals in `fnv_ai::goal`: `Escort`, `Find`, `UseAt`,
`Ambush`, `Flee`); the in-game checks are §4.

**What the masters hold** [observed: `fnvtool ai show` over every PACK of the five
masters, 2026-10-17]:
- **Escort** (13): target always a specific reference (the player in 9, captives in 4);
  location 1 the destination (near a reference 8, near the editor location 4, in a cell
  1); PKE2 escort distance 200 (7), 300 (2), 500 (1) or 600 (3); the target's count / distance
  0, 256 (4) or 800 (1); mostly Defensive Combat (11).
- **Find** (88): targets by object type (50: Food 12, Furniture 16, Flora 7, Containers 2,
  NPCs 4, weapons 2, ammo, chems, "Actors: Any", ingredients, and "None" 3), by base form
  (23: meats, leveled actor lists, form lists, a faction) or a specific reference (14:
  the player, actors, items, doors); the count is the target's count / distance (0 to 100,
  mostly 1 to 3); the search location mostly near the current location (28) or the editor
  location (30), or near a reference (22); 8 have none. Flags: Allow Killing 21, Allow
  Stealing 5, Allow Buying 0. Some are Fallout 3 leftovers (the 14 CG03 school packages,
  "ArmitageSleep22x8": Furniture or "None" with count 0).
- **Use Item At** (30): the object as a specific reference (12: chairs, markers, a
  repair marker), an object type (13, all Furniture) or a base form (5: a gambling stool,
  a face-down marker, the Stealth Boy 3 times); location 1 the place, mostly near a
  reference (18), most with radius 0. Sit Down on 4 (one with a specific chair, one with a
  marker, two by object type).
- **Ambush** (44): location 1 where it waits (radius mostly 0 to 256), location 2 every
  time (the trigger: near a reference 24, often with radius 0, near the linked reference
  10, near the editor location 8, in a cell 1, near the current location 1); a target in
  15 (the player 13, a faction 1, a reference no master defines 1); Hide
  While Ambushing 30, Weapon Drawn 26, Always Run 14, No Combat Alert 9, Always Sneak 7.
- **Flee Not Combat** (15): a specific-reference target in 7 (the player 5) whose count /
  distance is 256 to 2,000; a location in 9 (in a cell 2, near the editor location 4,
  near a reference 2, near the current location 1; both in 2); one has neither
  (GoodspringsFleePackage, first in the Goodsprings settlers' lists).

**Escort** [documented: Category:Packages "Follow/Escort/Accompany: Idles play while not
moving but waiting for target to move"; Defensive Combat "The exception is for the
Accompany, Escort and Follow packages … if the target is attacked, the actor will join
into the combat"; Combat Package Flags: Pretend In Combat and Continue During Combat "do
not work" on Escort]:
- It goes to location 1 with its target and stops to wait when the target is more than
  the PKE2 escort distance away (or in another cell), facing it; it goes on once the
  target is within that distance less 64 [observed: §4, a settler 352 units from the
  player under an escort distance of 300 stood until the session ended, and one the
  player followed led the player to its location; our choice: the 64]. At the destination its goal is met. A target that is gone or dead
  is not waited for [our choice]. Without PKE2, 200 [our choice: the most common value].
  The target's own count / distance (256, 800) is not used [hypothesis: unknown meaning].
- **Joining the target's fights** (since 2026-10-20 for Escort, since 2026-11-16 for
  Follow and Accompany too, #2793; `Game::escorts_assist` in `fnv_game::provocation`,
  called with `Game::call_allies`) [documented: Category:Packages, Defensive Combat, "The
  exception is for the Accompany, Escort and Follow packages. For these three, if the
  target is attacked, the actor will join into the combat"]: while its current package is
  Accompany, Escort or Follow (`provocation::CHARGE_PACKAGES`; the charge is the package
  target, PTDT, a specific reference or the linked reference, the same field for all
  three [documented: Follow Package and Accompany Package, "Specific Reference: actors
  only", "Standard Reference selector, filtered to actors"]), the actor takes up its
  charge's fight against each actor attacking
  the charge (in a combat state with the charge on its target list, or having hurt it):
  it holds a grudge against the attacker and is alarmed at its position, as an assisting
  ally (combat-ai.md §10.3), whatever its Assistance and whether or not the package is
  Defensive Combat. Never against its own allies or friends [documented: they "cannot
  attack each other under any circumstances"]; a Cowardly escort stays out (combat-ai.md
  §10.3, owner's list 4 item 14). Choices [hypothesis]: the escort must at least notice
  its charge (band Noticed or Detected: it walks ahead with its back to the charge, which
  it then only hears [observed: smoke run, the settler noticed the player at 270 units,
  out of view]), with no other range; a fight the charge starts is joined only
  once the other side fights back (then it attacks the charge); a charge that provoked
  the attacker is helped all the same; it holds from the package's start to its end, also
  once the destination is reached; an escort that is the player's teammate is left to
  the teammate rule (followers.md §3). A Defensive Combat actor still starts no fight of
  its own (`Game::hostility`): it only joins its charge's. Before #2793 only Escort was
  built, so the Hoover Dam escort Securitron (`VHDSecuritronEscort01REF` 0x001554DB on
  `DefaultFollowPlayerFar` 0x001554DC: Follow, target the player, Defensive Combat)
  never fought the Legion attacking the player [observed: a tester's events file, issue
  #2793]; smoke scenario `house-escort-assist`. No placed actor runs an Escort package without a quest
  stage (NellisRaquelIntroEscortAIPackage, GomezEscortTo*), so the smoke scenario
  `package-escort-fights` gives one by script. Log: `provocation: <escort> helps <charge>
  against <attacker>`.

**Find** [documented: Category:Packages "Standard Search Data" (a search location with a
radius; Allow Buying, Allow Killing, Allow Stealing; "Replace after use?" greyed out on
Find), Must Complete "the target must be found with a find", "Find: Idles play when
'activating' the target of the find"; Combat Package Flags "The actor will perform the
travel behavior on the way to the object, then will drop out of combat and perform the
normal package behavior"]:
- It goes to the search location, then looks in the area (the radius around it; the
  whole cell for "in cell"; around where it stands for "near current location"; 512 when
  the radius is 0 [our choice]) for what the target names: a reference, references of a
  base form or of the bases of a form list, actors of a faction, or references of an
  object type. Nearest first, it goes to each and acts on it, until it has found the
  target's count (at least 1) [hypothesis for every part of this paragraph]:
  - an item (ALCH, AMMO, ARMO, BOOK, INGR, KEYM, MISC, WEAP, NOTE, CHIP, IMOD, CCRD,
    CMNY): it picks it up (the default activation); flora: it harvests it; furniture: it
    sits (or lies) on it; an actor, a container or anything else: it goes up to it (for a
    specific reference, within the target's distance);
  - furniture another actor sits on, or is on its way to, is passed over, and the
    furniture it sets off to is kept for it, as for Use Item At (since 2026-11-08)
    [hypothesis, from an owner report: in Tale of Two Wastelands nobody looked to sit at
    the school desks in the G.O.A.T.]. Fallout 3's classroom (CG03, `Fallout3.esm`) seats
    its students by Find packages (`CG03AmataTakeTest`, `CG03ButchTakeTest`,
    `CG03PaulTakeTest`, `CG03WallyTakeTest`, `CG03SusieMackTakeTest`,
    `CG03FreddieTakeTest`, `CG03ChristineKendallTakeTest`; target object type Furniture,
    search near `CG03ClassroomMarker` radius 350, Run In Sequence `LooseSitTakingTest*`
    idles) [observed: `fnvtool ai show`, TTW 3.4 load order]. Before, Amata and Wally both
    went to Christine Kendall's desk and Butch and Paul to Susie Mack's, each pair beside
    one already seated ("cannot use …: in use"), and only three students sat [observed in
    ours]. Furniture the player or any other actor the engine seats is on is not free
    either (`Game::furniture_used_by_other`, also for Use Item At): the player has no
    desk of their own in the data (eight desks, seven students; the classroom trigger
    `CG03ClassroomTriggerSCRIPT` sets stage 50 once the player sits and stage 60, the
    test, only once all seven students report `GetSitting` 3), and Paul went to the desk
    the player sat at and stood by it, so the test never began [observed in ours]. The 16
    Find packages of the New Vegas masters that look for furniture follow the same rule;
  - a dead actor carrying what it looks for: it takes one from the body;
  - **Allow Killing**: it attacks a living actor it looks for, or one carrying what it
    looks for (the geckos' and stalkers' "Find Food" packages with object type Food and
    Allow Killing hunt in this reading) — the fight is the combat AI's;
  - **Allow Stealing**: items owned by another (the reference's owner, not the actor or
    one of its factions) are taken only with it [documented name; hypothesis: the
    owner test];
  - **Allow Buying** [documented: the checkbox's name only, Category:Packages "Standard
    Search Data"; no page says what it does, and no Find package of the masters has it
    (Allow Buying 0 above; Eat has it 49 times), so nothing could be watched in the
    original; every point below is a hypothesis]: a living vendor of the area counts as
    an object it looks for when its merchant container (the XMRC of its reference, the
    stock the barter menu sells from, barter-menu.md §4) holds an item the target names
    (a base form or form list, or an object type) that the actor can pay for. An actor
    without a merchant container is not a vendor. Nearest first, as every other object.
    It walks up to the vendor as to any actor it looks for (the vendor may move; it is
    reached within three times the arrival distance), then buys one: the cheapest such
    item it can pay for (lowest form ID on a tie), among what the vendor sells by its
    services (vendor-services.md §4; 2026-10-21). The price is what the player would
    pay at the actor's own Barter skill: the item's value (SetItemValue's, else the
    record's; condition not counted) times `fBarterBuyBase + Barter × fBarterBuyMult /
    100`, never under the value, rounded to the nearest cap, halves up. Its caps go into
    the merchant container, where the vendor's caps are, and the item comes out of it.
    Each purchase counts one found; the vendor stays a candidate while it has more to
    sell, so a count of 3 buys up to three from one vendor. A living vendor is bought
    from, not attacked, when the package also allows killing. Not done: buying from a
    vendor's own inventory, barter between actors in the barter menu, a specific
    reference as the target (a reference in a container is not a reference). Test aid
    `NVF_PLAY_TEST_FIND_BUYING=1` (engine) / `AiConfig::find_buying`: every Find package
    allows buying, since no shipped one does; smoke `package-find-buying`.
- With nothing (more) to find its goal is met and it stays; it looks again every 10
  seconds [our choice: else a Must Complete Find with nothing in reach would hold its
  actor for ever]. Object type "None" finds nothing: it goes to the location and stays.
- What it took is GetIsUsedItem's (below).

**Use Item At** [documented: Category:Packages "Use Item: Idles play at idle location
(instead of pulling from the idle manager)"; Standard Search Data's "Replace after
use?"; Combat Package Flags "travel behavior on the way to the fire location, then
… the normal package behavior"]:
- It chooses what to use: the target's reference; else the nearest object of the
  target's base form or object type in the area around location 1 that no other actor
  uses or is going to (with Sit Down, only furniture to sit on); a base form that is an
  ingestible (the Stealth Boys of Lonesome Road) it uses from its inventory where it
  stands, if it carries one. It goes there and uses it: furniture it sits or lies on (the
  engine's furniture code); anything else (a marker) it stands at, facing the way the
  object faces. Its goal is met [hypothesis for all of this].
- **Sit Down** [hypothesis: only furniture to sit on is chosen; the masters set it on
  HVMcNamaraWaitingPackage but not on HVMcNamaraDeskPackage, which name the same chair,
  so it may mean more].
- Nothing to use: it goes to location 1 and stays.

**Ambush** [documented: Category:Packages "wait at a location until enemies enter
another" (§3); the Patrol and Ambush packages located near a linked reference need one
(§3); Combat Package Flags "The actor will perform the travel behavior on the way to the
ambush location, then will drop out of combat and perform the normal package behavior"]:
- It goes to location 1 and lies in wait there; the first enemy inside location 2 (the
  trigger location, read as a Dialogue package's: `Trigger`, §3) sets it off: it starts
  combat with it and its goal is met [hypothesis]. An enemy is the package's target (a
  reference, always; a base form or faction: its members) or, with no target, an actor it
  would attack (`Game::would_attack`) [hypothesis]. Without a location 2, within 1,024 of
  where it waits [our choice; every master Ambush has one].
- In the original an ambusher on QJDeathclawAmbushPackage did not spring on the player
  hidden inside its ambush location, nor on the player detected outside it; it sprang
  when the player was both detected and inside [observed: npc-behaviour-compare.md
  §16.2]. The rule above has no detection test.
- **Hide While Ambushing** does not make it sneak [observed: §4, IsSneaking 0 in every
  sample of GoodspringsSettler04Ref waiting under GSSettlerAmbushPackage, which sets
  it]; what it does is not known, and here it is only reported.
- The ambush settings the wiki names (§2) are not in the masters and not used.

**Flee Not Combat** [documented: Category:Packages "Flee: Idles play when at flee
destination"; IsFleeing (JIP) "Returns whether the calling actor is either fleeing from
combat, or is running a Flee-type AI package"; Combat Flee (the combat AI's flight, a
different thing: `fnv_ai::alert`); ForceFlee]:
- Its threat is the package's target within the target's count / distance (2,048 when
  0 [our choice: fCombatFleeNormalDistance's default]), else the nearest actor within that
  distance, and either only when it is an enemy: one of the two would attack the other
  [observed: a settler 200 units from the player, who was no enemy, did not run, §4;
  hypothesis: that an enemy makes it run]. While it has one it runs: to location 1
  when the package has one, else to a spot the flee distance plus 256 beyond the threat,
  straight away from it (turning up to 90° when the navmesh ends) [our choice: as the
  combat flight's third try]. Arrived, or out of reach of the threat, its goal is met and
  it stands; a threat that comes near again sends it off again. It never starts combat.
- Nothing threatens it: it stands and its goal is met [observed: §4, Easy Pete].

**Flags these packages bring up** (`fnv_ai::carry`, `fnv_game`):
- **Pretend In Combat** (on 4 Find and 1 Dialogue package of the masters): "The Pretend
  In Combat flag causes the actor to enter combat when the package is begun … to move
  from cover point to cover point on their way to a destination" [documented: Combat
  Package Flags]. Here it only runs and has its weapon out [our choice]: the cover
  moves are not built.
- **Allow Swimming**: "Actors will not swim to reach destinations unless this flag is
  set" [documented: Category:Packages]: without it a way through water costs 50 times a
  way on land (it is still taken when there is no other) [our choice]; with it 3 times,
  as before.
- **Allow Falls**: "Actors will not jump off of an elevation that will cause damage
  unless this flag is set" [documented: GECK wiki Category:Packages, Flags Tab].
  [design, #2532: `fnv_ai::Ai::path_params`, `fnv_nav::PathParams::safe_drop_max`]
  With it the existing unrestricted ledge policy applies. Without it, an authored
  downward link may connect an otherwise unreachable route when its planned takeoff
  to landing is no higher than `fJumpFallHeightMin` (fallback `SAFE_DROP_HEIGHT`, 600,
  following `engine/nvf-play/src/falls.rs`; physics.md section 6.10).
  [hypothesis, #2532] This individual-link height predicts a harmless fall; original
  scene parity, collision along the arc and cumulative falls remain unverified
  (observer check 214). No new links or ranger-specific package overrides are created.
- **No Warn/Attack Behavior** (4 Ambush, 18 Dialogue, 6 Eat and others): no aggro-radius
  warnings or attack (`fnv_game::aggro`) [hypothesis: the flag's name, xEdit; no page
  describes it].
- **GetIsUsedItem / GetIsUsedItemType / GetIsUsedItemEquipType / GetUsedItemActivate**
  [documented: GECK wiki, each page]: what an actor "uses" is the furniture it is on, or
  the item its Find or Use Item At package last had it take or use, until its package
  changes [hypothesis]. GetIsUsedItemType takes a form type code (ACTI 21, TACT 22,
  TERM 23, CONT 27, DOOR 28, FLOR 38, FURN 39 added to the inventory codes RemoveAllTypedItems
  uses [hypothesis: the same numbering]) or a type name.

## 4. In-game observations so far

- The AI observer (ai-observer.md) records, per actor and sample, the current package
  (form), `GetCurrentAIPackage` and `GetCurrentAIProcedure`. In Goodsprings, Doc Mitchell
  ran VCG01DocMitchellSandbox (Sandbox; location near current location, radius 1,024;
  no schedule; one condition) for all 33 game hours observed, in his house; Sunny Smiles
  switched from DIALOGUE to DefaultStayAtEditorLocation (a Travel package to her editor
  location, radius 0) when a conversation ended [observed: `goodsprings.timeline.json`,
  2026-10-08]. These timelines are the reference for checking package choice (step 5 of
  the plan).
- **The reader against the game** [observed: `fnvtool ai observed` over the eleven
  observer runs in Goodsprings, 354,983 actor samples, game days 6.4–14.5, 2026-10-10]:
  - **Type codes**: every sample of a package the masters define reported the run-time
    code paired with its record type (§1): Accompany 15, Ambush 17, Dialogue 34, Eat 3,
    Flee Not Combat 18, Guard 35, Patrol 37, Sandbox 36, Sleep 4, Travel 14; 236,027
    samples over 30 packages, no disagreement. Not seen running: Find, Follow, Escort,
    Wander, Use Item At, Use Weapon.
  - **Run-time packages** (forms 0xFF…, made by the game): codes 5 COMBAT (19,490
    samples), 6 DIALOGUE (2,944), 1 FOLLOW (302), 21 GET_UP (62). 96,158 samples had no
    package (code 0).
  - **Package lists**: 39 of the 42 (actor, package) pairs were a package of the actor's
    list (234,335 samples). The three others are packages scripts add: RingoTravelPackage
    and GSRingoAfterVMS16DialoguePackage on RingoRef, SunnyTravelPackage on SunnyRef
    (1,692 samples).
  - **Targets** (`GetPackageTarget`): for packages with a specific-reference target the
    game reported that reference (21,704 samples) or none (79, at the start of
    SunnyMeetPlayerDialoguePackage), never another. Packages without a target in the
    record reported none (191,546 samples) or one the game chose while running (22,446:
    sleep, sandbox and patrol packages; the bed, the object in use, the patrol point
    [hypothesis for what each is]). An object-type target (EasyPeteEat12x2) reported
    none.

- **A Dialogue package waiting and starting** [observed: the owner's captures with
  SunnyRef and SunnyMeetPlayerDialoguePackage, §2; read again 2026-10-10]:
  - **The player elsewhere** (`town_day`, 24 game hours, 2,904 samples): she ran the
    package throughout, procedure 43 DIALOGUE_ACTIVATE, target the player, and stood
    on her editor location (0.0 units from it in every sample).
  - **The player in the same cell, outside the trigger box and far off**
    (`saloon_evening`: the player at (−3.9, 243.6, 3460.6) in the saloon for six game
    hours, 773 units from her and outside the box, which ends at y = −325): she did
    not move (1,451 samples at her editor location, heading 90°), reporting the
    run-time package 0xFF001309, type DIALOGUE, procedure DIALOGUE_ACTIVATE, target
    the player. So being in the cell does not send her to the player.
  - **The player next to her** (`meet_sunny`, `player.MoveTo SunnyRef`): the
    conversation had started by the first sample; she stood 40 units from the player
    and had turned from heading 90° to 178°, towards the player (who was south of
    her). During it: procedure 17 DONE. After it, in the rerun: the list package with
    procedure DONE (78 samples), then DefaultStayAtEditorLocation.
  - **Not captured**: the player coming in by the door (inside the box, 343 units
    away), which is the case the game is played in (§6).

- **Schedules, list order and the choice** [observed: the same eleven runs; `fnvtool ai
  observed`, `schedules` and `choice_replay`]:
  - **The window**: the nine scheduled packages seen ran only from their starting hour
    for their duration (35,668 samples; for example EasyPeteChairPackage8x4 0.01 to 4.00
    hours after 08:00, the three sleep packages 0.00 to 8.00 hours after 22:00, across
    midnight), apart from the minutes a switch takes at the end (53 samples, at most
    0.2 game hours) and the next point.
  - **Nothing valid: the package stays.** GoodspringsSettler01Ref's list is a flee
    package, a gunfight package and a sleep package (22:00, 8 hours), all with
    conditions, then GSSettlerAAMSandbox6x16 (06:00, 16 hours) without. It ran the
    sandbox all night (1,466 samples outside its window, up to the full 24 hours): with
    no valid package the actor kept the one it was running [hypothesis: "the current
    one" rather than "the last of the list"; they are the same package here].
  - **List order**: of 234,335 samples of a list package, 232,755 show a package in its
    window with no earlier unconditional package in its window; 1,519 are the two cases
    above; 61 are EasyPeteRef still in his sleep package up to 8 game minutes after
    08:00, when the chair package before it in the list had become valid.
  - **The linked reference**: VERGSCoyote4REF and VERGSCoyote5REF, who have none, ran
    DefaultSandboxEditorLocation512 past DefaultPatrolCasual, the first of their list
    (15,518 samples).
  - **When actors switch**: after a schedule boundary, actors took 0.2 to 12.8 game
    minutes to change package (timescale 30: 0.4 to 25.6 real seconds; 15 switches to a
    scheduled package, 9 away from one). Three actors in unloaded interiors switched in
    the same sample, 12.8 game minutes after 22:00. Four switches came 2.4 to 2.7 game
    hours after the starting hour (EasyPeteRef three times, TrudyRef once): not
    boundary switches; the actor came out of another package then [hypothesis].
  - **Script packages last**: RingoTravelPackage (neither Must Complete nor Must Reach
    Location) ran on RingoRef for 887 samples, which does not fit "removed at the next
    evaluation" (§3) unless that means the evaluation after it is done.
  - **The choice code against the samples** (`choice_replay`: `fnv_game`'s
    `select_package` at each sample's hour, the actor keeping the package of its
    previous sample, in a game where no quest has started): the same package in 186,563
    samples; in 47,721 a package with conditions lies between the two in the list (the
    capture's quest state is not known to the replay); 51 differ with no condition
    involved, all within the minutes of a switch (GSChetRef 25, EasyPeteRef 26).

- **Carrying packages out against the samples** [observed: `fnvtool ai run --samples`,
  three of the captures, 2026-10-10]: with each actor started where the capture first
  saw it and no quest state, the simulated actor ran the same package in 98.7–100% of
  samples and was then in the same cell in 91% (EasyPeteRef), 97% (GoodspringsSettler04Ref)
  and 100% (GSChetRef, RingoRef, GoodspringsSettler01Ref, CheyenneREF) of them.
  GSVictorRef, given his patrol package, was in the same place 36% of the time: the
  game's patrol stops at its points (design §7, note 6).

- **Escort, Find, Use Item At, Ambush and Flee Not Combat** [observed 2026-10-17: the
  `package_kinds` campaign, `tools/capture/observe/package_kinds.campaign.json`, save
  `nvftest`, the packages given to Goodsprings residents with AddScriptPackage from a
  compiled script (a console line naming a reference by FormID changed nothing), every
  15 game loops; and the three gunfight runs of `goodsprings_fight`]. GoodspringsSettler02Ref
  and TrudyRef are disabled at this point of the game (no package, in no scan).
  - **Escort** (GomezEscortToAtrium on GoodspringsSettler01Ref, put 352 units from the
    player; escort distance 300): run-time type 2 ESCORT, procedure 12 ESCORT, package
    target the player. With the player standing still it did not move for the whole
    0.4 game hours. With the player following it (staying within 200), it walked to its
    editor location 1,700 units off in 0.21 game hours, the player 186 to 201 units
    behind, and the package ended there (its sandbox came back).
  - **Find** (FFER24FindFood on CheyenneREF, mole rat meat placed by the saloon door
    about 300 units away): run-time type 0, the code the function's table calls EXPLORE
    (§1: Find's code, not known before), procedure 7 WANDER, package target none. She
    wandered about the saloon (1,400 units in 0.3 game hours) and did not go to the
    meat. Here the actor goes straight to the nearest object it looks for (§3.1): the
    original's searching by wandering, and when it then finds, are open (§6). In the open,
    a settler and a copy of Cheyenne went straight to the meat (5 to 9 s) [observed
    2026-11-10: npc-behaviour-compare.md §15.3].
  - **Use Item At** (DefaultUseNearbyFurniturePackage on GoodspringsSettler04Ref, at his
    editor location, a stool 46 units off): type 16 USEITEMAT, procedure 17 DONE at once,
    standing, not sitting, for 0.12 game hours; then the script package was gone and his
    own Travel package sat him on that stool. Here he sits on it (§3.1, after the
    packages that name a chair or throne): open (§6).
  - **Ambush** (GSSettlerAmbushPackage on GoodspringsSettler04Ref in the gunfight runs,
    3 of 3): procedure TRAVEL to his location in the saloon, then WAIT_AMBUSH, IsSneaking
    0 throughout although the package sets Hide While Ambushing, weapon out on the way
    and holstered while waiting; it ended (DONE, then no package) when the quest moved on.
    Given by AddScriptPackage in the Goodsprings save it did not become his package (its
    condition, a quest variable, presumably failed).
  - **Flee Not Combat**: VTerrifyingPresencePackage (flee the player, 768; near the current
    location, 2,000) on GoodspringsSettler03Ref 200 units from the player: the game ran a
    run-time package (0xFF00131F, type 10 FLEE, procedure 16 FLEE, target the player) and
    he did not move in 0.4 game hours; the player was no enemy of his.
    EasyPeteHideInSaloonPackage (in the saloon's cell, no target) on EasyPeteRef outside,
    nothing threatening him: type 18 FLEE_NON_COMBAT, procedure 17 DONE at once, and he
    stayed (as in the three gunfight runs, until the fight came to him and he fought).

- **Procedure codes: what GetCurrentAIProcedure reads by what the actor does** [observed
  2026-10-18: every sample of the AI observer captures in `C:\fnv-private\capture\out`
  (82 sessions: the Goodsprings, Strip, casino, conversation, package-kind, owner-decision
  and fight campaigns), about 430,000 actor samples, each with GetCurrentAIPackage,
  GetCurrentAIProcedure, GetSitting, GetSleeping, IsMoving, IsInCombat and IsTalking;
  counted not in combat and not talking; numbers are samples (actors)]:

  | Package (code) | Procedure codes seen |
  |---|---|
  | Sandbox (36), moving | 7 WANDER 70,273 (52); 0 TRAVEL 12,548 (68); 21 GET_UP 3,072 (36) |
  | Sandbox, standing | 7 39,571 (45); 0 35,888 (81); 46 USE_IDLE_MARKER 14,767 (16); 21 3,575 (38); 45 SANDBOX 188 (1) |
  | Sandbox, getting on or seated | 48 SITTING 21,778 (≈30); 10 EAT 192 (≈8); 0 479 |
  | Sandbox, asleep | 8 SLEEP 1,754 (2) |
  | Travel (14) | 0 on the way 6,105 moving (15), 1,913 standing (16); 17 DONE 36,777 standing (32), 9,193 seated (5), 1,409 asleep (1) |
  | Patrol (37) | 41 PATROL 46,576 (37, moving, standing and seated); 17 236 (1) |
  | Follow (1) | 11 FOLLOW 4,090 (7), never 17 |
  | Escort (2) | 12 ESCORT 1,519 (1) |
  | Eat (3) | 10 6,686 standing and 1,191 seated; 0 723 moving; 7 3,284 seated (1) |
  | Sleep (4) | 8 1,820 asleep, 258 moving; 0 76 moving |
  | Wander (13) | 7 1,434 (7) |
  | Accompany (15) | 27 ACCOMPANY 3,374; 19 TRAVEL_TARGET 773 (1 each) |
  | Use Item At (16) | 28 USE_ITEM_AT 3,509 seated; 2 ACQUIRE 1,102 seated; 17 114; 0 80 |
  | Flee Not Combat (18) | 17 182; 34 FLEE_NON_COMBAT 72 |
  | Use Weapon (33) | 0 4,182 moving; 42 USE_WEAPON 36; 2 90 seated |
  | Dialogue package (34) | 17 3,171; 43 DIALOGUE_ACTIVATE 773; 0 16 moving |
  | Guard (35) | 44 GUARD 5,503 (12) |

  The 45 a Sandbox reads elsewhere is an actor out of the loaded cells (Chet, function
  probes `fp7_packages_locks`, §3.2 of docs/design/script-function-coverage.md); the
  observer's earlier timeline counts held 27,313 samples of 45 from five actors sampled
  far away. So the procedure of a loaded actor is what it is doing inside the package (the
  wiki's note, "a single package can contain several possible procedures" [documented:
  GECK wiki GetCurrentAIProcedure]); out of the high process it reads the package's own.
  How NVForever maps its AI's doings onto these codes: `fnv_game::functions::ai_state::package_procedure`
  (walking to a spot of the sandbox 7, to an object 0; the getting-up 21 held until the
  next thing begins) [hypothesis for the split between 7 and 0 while walking, which the
  samples do not tell apart by destination].

  Types with no done state. Sandbox, Sleep, Eat, Guard, Wander, Follow and Accompany
  never read 17 above; Travel, Patrol, Dialogue, Use Item At, Flee Not Combat (and Find,
  Escort, Ambush and Use Weapon, which finish at their goal) do. A script-added Sandbox
  package is not done on arrival [observed: issue #2012, the original's GSChetRef given
  DefaultSandboxEditorLocation256 by AddScriptPackage kept it for 1.5 game hours, 1,290
  samples, with no OnPackageStart, OnPackageChange or OnPackageDone], and "On End idle
  only works for packages which have a 'done' state" [documented: GECK wiki,
  Category:Packages]. In NVForever (`PackageType::has_done_state`,
  `Game::package_goal_met`) a package of those types never meets its goal:
  - no end script or OnPackageDone on arrival;
  - a script package of that type stays until it is removed or replaced;
  - Must Complete keeps holding it.

  A schedule duration still ends one of them on time, with its change script and no end
  script [design: the original's end of a timed Sandbox is not captured].

## 5. The reader and the CLI contract

`fnv_game::package` holds a package as typed data (`PackageDef`): type, general /
Fallout / type-specific flags (the last only count for the types that document them),
locations and targets with their FormIDs mapped into the load order by kind, the
schedule, the conditions, the idle collection, combat style, patrol / escort / follow /
use-weapon / dialogue data and the begin / end / change idles and topics. It reads the
schema's decoded values (`schemas/fnv/ai.json`), never byte offsets;
`GameData::package_defs` holds every package, `GameData::base_package_list` an actor
base's list. Tests: `crates/fnv-game/tests/packages.rs` (packages built in memory).

All commands take `<data_dir> <plugins>... --schema <dir> [--functions <file>]`.

`fnvtool ai packages` → `fnv.ai.packages/1`: the typed reader's tallies (by type,
location and target kinds by type and slot, scheduled packages, conditions, general and
documented type-specific flags by type, object types, packages that use the actor's
linked reference, patrol routes and their point counts, forms that do not map, extras,
the run-time type codes) and `agrees_with_survey`: whether the package count, types,
location kinds, target kinds, scheduled packages and condition count equal the raw
survey's (`all_agree`; true on the six masters), plus `reader_issues` (none).

`fnvtool ai show --package <EditorID|FormID>... --actor <EditorID|FormID>...` →
`fnv.ai.show/1`: each package in full with forms named and, for a patrol located near a
reference, its route (the chain of linked references); each actor (a placed reference
or a base) with its linked reference and its package list in order, a package that
needs a linked reference the actor lacks marked.

`fnvtool ai observed --samples <obs_*.jsonl>...` → `fnv.ai.observed/1`: AI observer
samples (private captures, ai-observer.md) against the reader: the observed run-time
code per record type with the expected code (`types_confirmed`,
`type_code_disagreements`), run-time packages and samples without a package by code,
whether each current package is in the actor's list, and the reported package target
against the record's first target; `schedules` (per scheduled package where in its
window the samples fall, list order, packages skipped for want of a linked reference,
the delays of switches after a schedule boundary); and `choice_replay` (the choice code
against the samples, §4). Only summaries are printed. The day of the week, month and
date of schedules are not checked (the samples carry no calendar date).

`fnvtool ai day --actor <EditorID|FormID>...` → `fnv.ai.day/1`: for each placed actor,
the package it chooses at every hour of the authored calendar day (`chosen`, `by_hour`)
and each list package's verdict per hour (`packages[].hours_0_to_23`: V valid, `.`
outside its schedule, c conditions fail, l needs a linked reference, o once per day), in
a game where nothing has happened (no quest started); two days are run and the second
reported, so an actor with no valid package shows the one it keeps.

`fnvtool ai run --actor <EditorID|FormID>... [--start-hour H] [--hours N] [--step S]
[--timescale T] [--force ACTOR=PACKAGE]... [--samples <obs_*.jsonl>...]` →
`fnv.ai.run/1`: the actors carrying their packages out (`fnv_ai`) over the load order's
navmeshes in the authored game: per actor the distance walked, counts of events, its
state after every game hour (space, position, package, activity) and the events
(package, trip with its doors and length, door, arrived, already there, unreachable,
not carried out). Each actor's `furniture` (`starts` per piece, `excluded_pieces` per
reason, `unknown`) covers the whole run, whatever the cap on `events`; `furniture_facts`
is null when the assets did not open, and `unknown` with it (furniture.md §9). `--force` gives an actor a package
as AddScriptPackage would. With
`--samples` each actor starts where a capture first saw it, at the capture's hour, and
is compared at every sample (`files[].actors[]`: samples, same package, same cell, the
share in the same cell among same-package samples, the distance when in the same cell,
the package differences); outdoors "the same cell" also holds within 1,024 units.

`fnvtool ai reach` → `fnv.ai.reach/1`: every placed actor and every package of its list:
whether the place the package sends it to can be reached from where the actor is placed
(`verdicts`, `by_package_type`, `doors_on_reachable_routes`, `most_doors`, `examples`
of each kind of failure). Doors and locks as authored; schedules, conditions and quest
state are ignored.

In the engine: `nvforever --npc-packages ACTOR[,ACTOR…] [--npc-timescale T]
[--follow-npc]` (docs/design/actor-behaviour.md §5.4).

Sandboxing: `fnv_ai::sandbox` (`Allowed`, `Doing`, `Sandbox`), rules and flags in
docs/design/actor-behaviour.md §5.5; `fnvtool ai packages` reports `sandbox` (per
Sandbox / Wander package: behaviours allowed and the other flags that bear on them)
and `sandbox_radius`; `fnvtool ai run` shows each actor's `using` (furniture) and
`sandbox` (what it is doing) and the events `sandbox`, `furniture`, `patrol_wait`.

Dialogue packages: `fnv_ai::goal::Talk` and `Trigger`, `fnv_ai::Talking` (how far an
actor is with it), the events `approach`, `talk`, `talk_over`, `no_talk`; a
reference's box or sphere is `fnv_sim::refs::PlacedRef::primitive`. Rules:
docs/design/actor-behaviour.md §5.6. Tests: `crates/fnv-ai/tests/talk.rs`. `fnvtool ai
reach` judges the wait location and reports packages without one as "talks: goes up
to its target from where it is".

Carrying out: `fnv_ai::goal::resolve` (a package's goal) and `fnv_ai::Ai` (`add`,
`update`, `update_with`, `update_moved`, `events`), `crates/fnv-ai`; `Game::block_package`, `Game::set_actor_place` and
`PackageCheck::Unreachable` in `fnv-game`. Tests: `crates/fnv-ai/tests/carry.rs`.

The choice in the game data: `Game::check_package`, `select_package`,
`evaluate_package`, `update_package`, `package_held`, `package_goal_met`
(`crates/fnv-game/src/ai.rs`); `GameConfig::package_interval` turns on the periodic
choice and the packages' begin / end / change scripts (off by default, so Layer 5 runs
behave as before apart from the validity rules). Tests:
`crates/fnv-game/tests/package_choice.rs`.

`fnvtool ai survey` →
`fnv.ai.survey/1`: packages and per-plugin counts; subrecords present and any not
decoded OK; PKDT sizes; counts by type with examples; general flags by type; Fallout
behaviour flags; type-specific bits by type; location types by package type and slot
(location 2 counted once per package, `location2_copies` says how many copies records
hold, whether they are identical and which subrecord precedes each);
what location and target FormIDs are (record signature, or `missing`); radius spread;
the most common unresolved target values; target types; schedule histograms (month, day
of week, date, hour) and duration spread, packages with any schedule by type; conditions
per package, the 30 most used functions (named from `schemas/script/functions.json`)
and their run-on values; begin / end / change idles, topics and compiled scripts; actor
package lists (length spread by NPC_ / CREA, first package types, the most common first
packages and the most listed packages, template-supplied lists, entries that are not
packages, packages in no list, placed actors whose base lists packages).

## 6. Open questions

- **The five package types of §3.1** (captures `package_kinds`, §4): how a Find package
  searches (the original's dog wandered and did not go to meat 300 units away) and when
  it finds; why DefaultUseNearbyFurniturePackage left its actor standing, DONE, next to
  a stool; what Hide While Ambushing does (not sneaking); whether a Flee package runs from
  a target that is an enemy (the case not captured) and how far; what an Escort's
  target count / distance means; whether AddScriptPackage checks a package's
  conditions. Captures to make: a Find package for an item in plain sight, watched for
  longer; an Ambush package given to a creature the player is an enemy of; a Flee
  package whose target is hostile.
- The re-evaluation interval (the wiki's "seem to be every 10 seconds"; switches were
  seen 0.4 to 25.6 real seconds after a boundary, §4), whether it runs on real or game
  time (every capture used timescale 30), and how the game treats actors in unloaded
  cells (three switched in one sample).
- Schedules: a starting hour with duration 0 (12 master packages, which our rule never
  lets run [hypothesis]) and a duration without a starting hour (358 packages: since
  2026-10-13 treated as how long the package runs from its choice [hypothesis built on
  the documented SetPackDuration "duration of the actor's current package";
  docs/design/actor-behaviour.md §3]; since 2026-10-19 only once the package's goal was
  met, as the Goodsprings geckos showed in three observer sessions, owner-decision-captures.md
  §4 and §7); the day a window that crosses
  midnight belongs to; months, seasons and dates (no master package uses them).
- With no valid package: does the actor keep its current package or take the last of
  its list (§4), and what does an actor that has run none do?
- How Once Per Day, Must Complete and Continue If PC Near interact with the periodic
  choice (implemented as documented; not yet seen in a capture), and whether a package
  needing a linked reference is skipped for types other than Patrol.
- Script packages: **answered in part.** One at a time (od_08, owner-decision-captures.md
  §4). One whose goal was met is let go at a later choice: Ringo's travel package read
  DONE for at least 600 game loops and his list package 1,500 loops after it was given,
  with no ResetAI (od_06, owner-decision-captures.md §7), as the Use Item At package
  above did after 0.12 game hours [observed]. The runtime now drops it at the next
  choice (`Game::select_package`). Whether one that is not done ever goes without
  ResetAI or RemoveScriptPackage is open: EvaluatePackage kept a sandbox one (od_08).
  One with a schedule duration and no starting hour stays after its goal until the
  duration has run out, counted from when it was given (2026-10-20: the Strip's SS2
  troopers kept their 3 h Must Complete wall travels 0.7 h after arriving,
  npc-behaviour-compare.md §10.1 [observed]; that the duration is the reason
  [hypothesis]). Open from the same capture: what ended MP A's untimed escort patrol and
  three troopers' patrols in one sample mid-way (§10.1).
- The meaning of the type-specific bits on package types without such flags (§2), and of
  Fallout behaviour bit 3.
- What the package target is for packages whose record names none (§4). (The run-time
  type codes of Follow, Wander and Use Weapon, once listed here, were seen running in
  the §4 procedure table: Follow 1, Wander 13, Use Weapon 33; Find runs as 0, Escort as
  2, Use Item At as 16 [observed, §4].)
- Which patrol point a patrol located near a reference with no linked reference (59
  packages) uses, and how a patrol located near the editor location (61) finds its route.
- **Dialogue packages**: what the game does when the target is inside a trigger
  location of radius 0 (a box) but further off than the activate distance: Sunny
  Smiles as the player comes into the saloon. Capture: enter by the front door on a
  save where she has not been met, touch nothing, and record her position, package and
  procedure and when the dialogue menu opens. Does she walk up (what `fnv_ai` does),
  or does the conversation open from 343 units? Is the player held meanwhile ("Do Not
  Control Target Movement" off)? Also: what a stored distance of 0 means, how fast an
  actor goes up to its target, whether a package whose conversation is over starts
  again while it stays valid, and whether "radius 0" tests the reference's box (the
  data says so, §2; the documentation does not).
- The sandbox and ambush settings' values in the game (capture with `GetGameSetting`).
- Low-level processing: whether and how unloaded actors advance their packages (the
  observer's tracked actors show where the game moves persistent NPCs while the player
  is elsewhere).

## 7. ESM gap audit: AI and packages group (2026-10-21, branch claude/em1-ai-packages)

The audit (`docs/handoff/esm-gap-audit-2026-10-21.md`, medium impact, "AI and packages")
listed record data the engine ignored. What each piece means, and what is built:

### 7.1 Creatures that cannot open doors (built)

- CREA ACBS flag bit 22 is "Can't Open Doors"; on NPC_ the same bit is "Can Be All
  Races" [documented: GECK wiki GetActorBaseFlagsHigh, flag table, Creature column;
  `schemas/fnv/gameplay.json` ACBS]. 563 of the 1,578 CREA records in FalloutNV.esm carry
  it: brahmin, geckos, night stalkers, cazadores, turrets, tumbleweeds [observed: scan of
  the master 2026-10-21].
- The wiki says no more than the name. What it means for a path [hypothesis]: such a
  creature goes through no teleport door (load doors): its paths stay in its own space,
  and a place only a load door leads to is unreachable to it, as behind a locked door it
  has no key for. Doors that open in place (hinged doors and gates without XTEL) are left
  as they were: the owner saw a Novac pack brahmin (flag set) expected to follow its
  merchant through the chain-link gate by the motel (smoke `novac-brahmin-gate`), so
  whether such a creature waits at a shut hinged door, goes round it, or passes one
  someone else opened is open; not built.
- The flag is read from the definition supplying the creature's base data (ACBS
  template flag "Base Data") [hypothesis: the wiki's list of flags Base Data leaves out
  does not name it].

### 7.1b Creatures' movement flags: Swims, Walks, Flies, Immobile, No Combat in Water (built 2026-10-25)

- CREA ACBS bits 4 Swims, 5 Flies, 6 Walks, 18 No Combat in Water, 23 Immobile
  [documented: xEdit CREA ACBS, `schemas/fnv/gameplay.json`]. Meanings [documented: GECK
  wiki Animation Tab]: Swims "allows for swimming and moving across water marked navmesh
  triangles"; Walks "allows for movement. Actors placed off of the navmesh will be
  teleported onto it"; Immobile "prevents movement … (Overrides Flies, Walks and
  Swims)"; No Combat in Water "will not perform attacks in water". And [documented: GECK
  wiki Navmesh Creation Workflow] creatures "that do not have the flag will avoid using
  water triangles".
- In the masters [observed: scan of the six masters' CREA ACBS, 2026-10-25]: 269 walk
  and swim (dogs, coyotes, rats, night stalkers, lakelurks, mirelurks), 1,714 walk only
  (geckos, deathclaws, ants …), 157 fly and walk (cazadores, Mister Gutsies, bloatflies),
  4 do all three (ED-E), 79 are Immobile (turrets, Mr. House), 3 tumbleweeds have No
  Combat in Water, and 12 carry no movement flag (template creatures: their flags come
  from the template).
- Built:
  - A creature that walks or flies without Swims keeps off water triangles
    (`Game::keeps_out_of_water` → `PathParams::water` false, fnv-ai `path_params`): its
    paths close them. A goal on water is out of its reach at once; standing in water, it
    may walk out through it [design]. People are left to their packages' Allow Swimming
    (§3); a creature with none of the three flags is left as it was.
  - No Combat in Water: the creature holds its shots and blows while its character swims
    (nvforever `npc_combat`, the `ready` gate).
  - Immobile: the AI keeps the creature where it is, as one without movement clips
    (nvforever `residents`, log `ai: <name> cannot move (no movement animations, or
    flagged Immobile)`).
  - Immobile with a Turning Speed (CREA TNAM) of 0: the creature never turns either, not
    for a conversation, a line it says to someone, a script's Look or a combat target,
    and the engine does not draw it aimed at a target (fnv-ai `Ai::fix_heading`,
    nvforever `residents` `never_turns`, log `ai: <name> never turns`). In FalloutNV.esm
    4 of the 50 Immobile creatures have 0: MrHouse, MrHouseOFFLINE and YesManMainframe
    (the penthouse and basement screens) and CRVoid; the 46 others (turrets, spore
    plants, Festus, NVCRMrHouse, NVCRPenthousemaincomputer) have 30 to 300 [observed:
    the master's records]. An Immobile creature with a Turning Speed above 0 turns in
    place to its combat target (a turret). [hypothesis: that the original reads the two
    fields so; the owner saw the screen stay still in the original and turn here after a
    conversation and in a fight, issues #1543 and #2150.] Whether the original's screen
    enters combat at all is not observed (observer-checks.md).
  - The flags come through a template with "Use Model/Animation" [hypothesis: the
    Animation tab goes with the model]; Immobile is read off the base record only.
- Not built: a creature that swims but neither walks nor flies keeping to water (none in
  the masters); "teleported onto" the navmesh for Walks (actors are snapped to the
  nearest triangle when a path starts, as before).
- Checks: fnv-nav `water_triangles_close_to_actors_that_do_not_swim`, fnv-game
  `walkers_and_fliers_without_swims_keep_out_of_water`; smoke `nav-water-closed`
  (`fnvtool nav path --no-water`, Lake Mead from the Boulder Beach shore).

### 7.2 Package Begin / End / Change topics (built)

- "For each of the possible state changes of a package (Begin, End/Done, Change) you
  can specify an Idle to be played, a Topic to be said, and a Results Script to be run.
  The script is run first, so if you set a quest stage or variable in the results script,
  the topic and idle will react to it" [documented: GECK wiki Category:Packages,
  Begin/End/Change tab]. The topic is the TNAM after POBA, POEA or POCA [documented:
  xEdit PACK].
- The actor says the topic as `Say` would (no listener: the player is the conditions'
  target) after the result script and before the idle is asked for [hypothesis: the
  order of topic and idle; both follow the script]. 59 master packages have a Begin
  topic, 18 an End topic, 3 a Change topic [observed: scan 2026-10-21], e.g. the Vault 21
  tour packages `VVault21SarahTourTo00`–`05`.

### 7.3 Package combat style (built)

- "Combat Style: Select the Combat Style the actor will use when the package is being
  run" [documented: GECK wiki Category:Packages]; PACK CNAM [documented: xEdit]. 14
  master packages name one (the Goodsprings gunfight packages among them) [observed].
- While its current package names a style, an actor fights with it, over the style
  SetCombatStyle gave the reference and the record's own [hypothesis: the package's
  choice is the narrower, time-bound one, so it wins while it runs].

### 7.4 Patrol points: topic and result script (built)

- "Patrol Data … Topic: Dialog topic for the actor to say when arriving at this patrol
  point. Result Script: Result script to run when the actor arrives at this patrol
  point" [documented: GECK wiki Idle Markers, Patrol Data]. On the reference: XPPA, then
  the script (SCHR … SCRO), then TNAM, the topic [documented: xEdit placed-reference
  patrol data]. A REFR's TNAM of 4 bytes is that topic; the map marker's TNAM (inside
  XMRK) is 2 bytes [documented: xEdit]; the schema tells them apart by size.
- When a patrolling actor arrives at a point it runs the point's script (the actor as
  `self`), then says the topic, then waits out the idle time [hypothesis: the order, as
  for package events]. 107 master patrol points have a script and 14 a topic [observed].
- The patrol point's INAM ("Patrol Idle" in the schema) is in no master reference
  [observed] and no page says what it does: not built.

### 7.5 Conditions run on "Combat Target" (built)

- "Combat Reference: If the owning actor is in combat, the target is the actor's current
  combat target. If the owning actor is the player, the target may also be any actor the
  player is currently aiming at, even outside of combat" [documented: GECK wiki
  Category:Conditions, Run On]. A condition run on it is evaluated on the subject's
  combat target as GetCombatTarget gives it (kept while out of sight, through Lost,
  docs/design/alert.md §6); with none (out of combat, or the player,
  whose aim the engine does not hand to the conditions) the condition is run on nothing,
  as before.

### 7.6 Friendly Fire Comments (built)

- "Friendly fire comments: Allow the actor to use the Hit combat topic to react to
  friendly fire" [documented: GECK wiki Category:Packages, Enable Fallout Behavior].
  Fallout behaviour flags apply only to packages with "Skip Fallout Behavior" set
  (§3; `fnv_ai::social`). A hit from an ally or friend of the victim (its faction combat
  reaction, `crate::provocation`) is friendly fire; while the victim's package does not
  allow these comments it says no Hit line for it (Crippled and the health lines are
  other topics and stay).

### 7.6b The Fallout behaviours' lines and Avoid Radiation (built 2026-10-25)

Each is allowed by the package as hellos are (`fnv_ai::social::allows`: a package
without "Skip Fallout Behavior" allows all; the default flags 0x1CD include Observe
Combat, Idle Chatter and Avoid Radiation [observed: FalloutNV.esm]). Lines are said in
the world (`Game::say`, the player as the conditions' target), never by an actor in a
combat state; the topics' lines in the masters [observed: `fnvtool dialogue lines`]:
IdleChatter 82, ObserveCombat 39, PLAYERFIREWEAPON 56, PLAYERTHROWGRENADE 57,
PLAYERLAYMINE 56; the other PLAYER… topics have none.

- **Allow Idle Chatter** ("Allow idle chatter using the IdleChatter topic"
  [documented: Category:Packages]; "said by actors on a random basis who aren't in
  combat" [documented: Conversation Tab]): one timer for the loaded area, drawn between
  `fIdleChatterCommentTimer` and `…Max` (15 and 60) [hypothesis: the settings'
  meaning]; when due, an actor within 200 units of the player (the hello distance) with a
  clear line to it is drawn and says its line, under the waits and the memory of lines
  that hellos have (dialogue.md §5.2; 300 units and neither until 2026-11-11, #2156). Log `ai … <actor>: chatters (<d> units from the
  player), line <info>`.
- **Reaction To Player Actions** (the Topics Tab's "special topics used to get actors
  responding to player actions" [documented]): when the player's body starts a gun's
  shot (WEAP animation types 3–9), a throw or a mine (`Game::anim_state`), the nearest
  actor within 1000 units [stand-in] with a line says PLAYERFIREWEAPON,
  PLAYERTHROWGRENADE or PLAYERLAYMINE, at most once per `fCommentOnPlayerActionsTimer`
  (60 s) for the area [hypothesis: the setting's meaning]. Walking actors react too. Log
  `… comments on the player (<topic>, <d> units away)`.
- **Observe Combat Behavior** (the ObserveCombat topic: "actors in 'spectator' mode --
  watching a combat but not joining in" [documented: Misc Tab]): an actor within 1000
  units of the player and 1500 of a fighter (a resident in Combat, or the player while
  hunted) [stand-ins] says its line, each actor at most once per
  `fCommentOnPlayerActionsTimer` [stand-in]. Log `… comments on the fight of <who>`.
  Spectators "run away if they get too close" [documented: GECK wiki Packages; no
  distance is documented] (#960): an actor whose package allows the behaviour, not in a
  combat state, not asleep or talking, within 600 units of a fighter
  (`fnv_ai::social::SPECTATOR_FLEE_DISTANCE`) [design: stand-in, observer-checks.md
  check 60] runs from the nearest one, wherever the player is, by the combat flee's
  spot search (`fnv_ai::alert`: a door, cover between the flee cover distances, else
  fCombatFleeNormalDistance straight away; combat-ai.md §12.2) at its run speed. It
  does not enter a combat state, draw a weapon or join the fight. At its spot it stands
  facing the fight; the fight within 600 units again, it runs again (at most once per
  5 s, `SPECTATOR_REFLEE_SECONDS`); with no fighter within 1,500 units it goes back to
  its package [design]. Log `… runs from the fight of <who> (<d> units away): too close
  to watch`, then the flee lines (`flees from …`, `reached its flee spot …`). A fighter
  is a resident in Combat, or the player while someone in Combat (not Alert or Lost: a
  search is no fight) fights it within 1,500 units of it. The player's teammates, those
  following someone (Follow, Accompany) and allies or friends of the fighter by faction
  take a side and do not run. It runs from the middle of the fighters within 1,500
  units, not only the nearest, so as not to run into the fight's far side [design for
  each: review of #1216; tests `spectators_run_only_from_a_real_fight_and_not_the_players_side`].
- **Avoid Radiation** ("Allow actors to avoid entering radiated areas" [documented:
  Category:Packages]): for an actor whose Rad Resistance is below
  `fRadiationResistanceAvoidLevel` (50) [hypothesis: the setting's name], the radiation
  markers (XRAD/XRDS, `crate::radiation`) within 20,000 units are places to avoid in its
  paths (`PathParams::avoid`): a triangle whose centre lies within a marker's radius
  costs 8 times as much [design], and the shortcut pass does not straighten a way back
  into one. A goal inside stays reachable. The radiation zones explosions leave count as
  well while they still give rads, at their radiation radius [design]; a way planned
  before the zone appeared is not planned again for it (the Fat Man's fades in 12 s).
- Checks: fnv-ai tests `an_actor_next_to_the_player_chatters_now_and_then`,
  `a_spectator_runs_from_a_fight_that_comes_too_close`; fnv-nav
  `places_to_avoid_cost_more_and_are_gone_round`; smoke `player-fire-comment` (Easy
  Pete: "What in tarnation?") and `observe-combat-comment` (Easy Pete: "Look at 'em
  go!" while a cazador fights the player, then runs from it).

### 7.7 Not built, and why

- **Observe Combat Behavior**'s running away from a fight that comes too close: built
  on a stand-in distance (§7.6b, #960); the original's distance, and whether its
  spectators come back, are for the AI observer (observer-checks.md check 60).
- **Guard "Remain Near Reference to Guard"**: no page in the wiki export documents the
  Guard package's tab. Built on the flag's name (#961) [hypothesis]: a Guard with it
  stays near its target reference (or linked reference), within the location's radius,
  else the target's distance. The masters set it on 3 packages:
  LilyPsychoticBreakPACKAGE (a reference, 1,500), VHDMPStageGuardPackage (linked
  reference, 512) and VHDOliverNCRHeavyTrooperFireSupportHuntPlayerPackage (object type
  None: no reference, so its location stands) [observed: PKDT/PTDT of FalloutNV.esm].
- **Guard type-specific flags, the unnamed bits**: of the 175 Guard packages in
  FalloutNV.esm the raw value is 0 in 110, 2 in 31, 6 in 10, 4 in 9, 1 in 7 and 5, 16,
  18, 21, 29 (twice) and 59 in the rest [observed: PKDT]. xEdit names bit 3 only. In the
  original, of five Guard packages with Continue During Combat given to a knife trooper,
  the two with raw value 2 held it inside the guard radius and the three with bit 2 set
  (raw 6, 6, 4) did not [observed: AI observer 2026-11-12, observer-checks.md check 108;
  that bit 2 is the cause is a hypothesis]. A spawned actor's editor location is the
  spot PlaceAtMe put it at [observed, the same check].
- **Stay-at locations by Object ID, Object Type, "At package location"**: the Standard
  Location page names only Near Editor Location, Near Current Location, In Cell and Near
  Reference; how the other kinds choose a place is undocumented. Built (#961)
  [hypothesis]: Object ID / Object Type stay near the nearest present matching reference
  within 4,096 units of the actor (`goal::OBJECT_LOCATION_SEARCH`, ties by FormID);
  "At package location" as location 1 is where the actor is. No master package has
  these as location 1; "At package location" is Eat's location 2 in 47 [observed:
  `fnvtool ai packages` location_kinds].
- **XIBS Ignored By Sandbox**: "Appears not to work in New Vegas" [documented: GECK wiki
  Reference Data Flags]; left alone on purpose.
- **Header bit 25 No AI Acquire** ("AI will not attempt to pick up containers or objects
  marked with this flag" [documented: Reference Data Flags]): honoured (#962,
  `fnv_sim::refs::FLAG_NO_AI_ACQUIRE`). Re-arming (`fnv_ai::rearm`, combat-ai.md §22)
  takes no weapon or ammunition from a flagged reference, body or container; a Find
  package's search leaves flagged objects, containers and bodies out unless the package
  names that one reference. A living actor so flagged stays a target for Allow Killing and
  Allow Buying, since the flag speaks of picking up [hypothesis for both exceptions]. 67
  REFR and 12 actor references carry it [observed].
- **DOOR Minimal Use**: the wiki says only that SetMinimalUse sets the flag; no source
  says what it changes.
- **AIDT Responsibility in crime reactions**: the wiki documents Responsibility in the
  disposition formula and in combat's iAvoidHurtingNonTargetsResponsibility, not in
  crime reactions.

## 8. Actors already at their package's place when the player arrives (issue #1208)

- **What the original shows** [observed, owner, issue #1208]: fast travelling from
  Goodsprings to Novac at 5am, Boone is already at his post by the dinosaur on arrival;
  entering the Mojave Outpost, Cass is already drinking at the bar, and in the
  Prospector Saloon the settler is already seated at the bar. Ours had them walk there
  (out of Boone's motel room, up to the bar stools) in front of the player, because an
  actor of a cell that was not loaded joined the AI where it was placed (or where the
  low process left it, §1 of `fnv_ai::process`) and only then set off.
- **What is built** [hypothesis: the original is seen with the actors at the place,
  not how it gets them there]: when the player goes through a load door or fast
  travels (not when a script moves the player: `Loading::arriving`), the residents the
  player saw in the loaded area within the last game hour stay where they are (out of
  a house and straight back, the street is as it was [design]). Every other resident
  near the destination, in the loaded area, the cell buffer or an interior behind a
  door of the loaded cells, whose package's walk starts there, takes up its current
  package (chosen again for the hour it is now: a fast travel's hours have passed) and
  is put where the walk ends instead of walking: at the package's place, on the
  furniture the package gives it (seated at once, no walk up and no sitting-down clip),
  at the furniture or marker its sandbox chooses there, at the patrol point it sets off
  for when the patrol goes round (the nearest, as when its package begins), at the last
  point when it goes once (the waits at the points on the way over: Boone's night
  patrol out of his room, through the gift shop, to the dinosaur's mouth)
  (`fnv_ai::arrive`, engine `residents.rs` `Core::arrive`). An actor found already at
  its place on its furniture (the low process had it there behind a door) is seated at
  once too, instead of walking up and playing the sitting-down clip in sight. Actors
  farther away come as the low process brings them, stopping at the loaded area's
  edge (`fnv_ai::process`, Malcolm Holmes). Left alone: actors dead, unconscious,
  restrained, rooted, with their AI off, in a combat state, or put somewhere by a
  script in the last 10 seconds; those whose package's place is the player or another
  actor (a hunter would land beside the player); walks that depend on someone else
  (following, escorting, searching, fleeing, a conversation). Each resident is looked
  at once per trip, after the cells around the player have loaded; those put somewhere
  join the AI and are drawn under the loading screen, so the screen waits for their
  bodies.
- **Open** (docs/handoff/observer-checks.md, check 68): whether the original puts the
  actor exactly at the walk's end or somewhere part-way / within the package's radius;
  whether a short fast travel inside the loaded area moves anybody (ours: nobody,
  [design]); whether the hours a fast travel passes matter beyond choosing the package.

## 9. A script moving the clock (issue #2353)

- **The case** [documented: `fnvtool script decompile`, FalloutNV.esm]: Doc Mitchell's last
  line (INFO 0x001057E7) ends by showing the Hardcore box (activating 0x0011F293, whose
  script shows the message and reads the button in GameMode), giving the controls back and
  starting the quest VGenericTimer (0x00168A86) with a 0.1 s timer counted with
  GetSecondsPassed; at its end VGenericTimerSCRIPT sets VCG01 to stage 200, whose script
  does `set GameHour to 8`. A player who answers the box and goes straight out of the door
  arrives in Goodsprings before that stage runs, so the residents are placed on arrival
  (§8) for the chargen clock's early morning (Easy Pete in his bed: EasyPeteSleepPackage,
  00:00-08:00, in GSHouseInterior03 [documented: `fnvtool ai day`, `ai show`]), and then
  the clock jumps to 08:00. A write to GameHour only stores the value
  (`fnv_sim::World::set_global`); before #2353 nobody chose again until its periodic turn
  (§3, up to 10 s on) and then walked from its night place [hypothesis from the code:
  the issue's chain, not seen in a run; players reported Easy Pete out of place].
- **What the original does**: not known. The GECK wiki's "How Are Packages Evaluated"
  lists the periodic choice, EvaluatePackage, ResetAI and the end of a wait, and says
  nothing about a script setting GameHour [documented: GECK wiki]. Observer check 132.
- **What is built** [design, #2353]: a script's write to GameHour (`set GameHour to`, the
  console's `set`, JIP's SetGameHour) that moves the clock face by more than
  `fnv_game::ai::SCRIPT_HOUR_JUMP` (1 game hour; 23:00 to 01:00 is 2) is noted for the
  engine (`Game::set_global_by_script`, `Game::take_hour_jump`; several writes before the
  engine looks are one jump, from the first hour to the last). The engine handles it like
  the end of a wait: every actor the periodic choice covers chooses its package now
  (`Game::choose_packages_now`), and every resident the player cannot see (in the cell
  buffer or an interior behind a door of the loaded cells, not in a drawn cell) is put
  where its new package's walk ends, on its furniture, by §8's rule (`fnv_ai::Ai::hour_jumped`,
  engine `residents.rs` `Core::after_hour_jump`). Residents in view are not moved: they set
  off for their new packages in sight; a walk that would start in view is not skipped
  either. Those left alone by §8 (dead, in combat, just moved by a script, …) are left
  alone here too.
- **Not changed**: a package's schedule duration ends by GameDaysPassed
  (`Game::end_timed_packages`), which a GameHour write does not move, and a held package
  (Must Complete, Must Reach Location until its goal) stays through the new choice. So the
  Goodsprings night stalkers of #2279 are not obviously fixed by this.
- **Follow-up: Goodsprings night stalkers at 08:00 (issue #3041)** [observed: `fnvtool ai show`, `script decompile`, FalloutNV.esm]:
  `VERStalkersSouth01` (`0x001617A2`, script `0x001616E8`, Start game enabled) moves 18
  night stalkers from `VStalkerSouthRestMarker` (`0x001690EA`, a closed holding interior
  cell with no doors) to exterior markers at 23:00 (`StalkersOn = 1`, `quest_delay = 18`).
  Their night packages (`VSpecialIvanpahStalkerHuntersRaidMWF3`,
  `VSpecialWBStalkerHuntersRaidMWF3`/`MWF4`/`MWF5`, scheduled 23:00–03:00, and
  `ReturnMWF`, 03:00–08:00) all require `VERStalkersSouth01.StalkersOn == 1` and carry
  `Must Reach Location`. When `VCG01` stage 200 sets `GameHour` to 8, none of the stalkers'
  packages is in schedule, and up to 18 s later `VERStalkersSouth01`'s day branch moves
  each stalker back to `VStalkerSouthRestMarker` (`MoveToMarkerWithFade`, one per 0.1 s)
  and 1.8 s later sets `StalkersOn = 0` without calling `EvaluatePackage`. Three gaps kept
  the 23:00–01:00 raid packages running from the holding cell on later Low-process updates
  (`FAR_PROCESS_SECONDS = 120 s`):
  1. `Game::select_package` kept the current list package when `check_package` returned
     anything other than `ConditionsFail`; because `check_package` tests the schedule
     first, a package that was both out of schedule (08:00) and failing its conditions
     (`StalkersOn == 0`) returned `OutOfSchedule` and was kept [design: keep the existing
     check and also evaluate the current package's own conditions directly. A package
     ruled out by its schedule or a target problem stays current only while its own
     conditions pass].
  2. `Game::package_held` still held an unfinished `Must Complete` / `Must Reach Location`
     package after a script relocated the actor (`MoveTo` / `MoveToMarkerWithFade`)
     [design: `RefExtra::package_relocated` releases that hold once relocated; if the
     package is still valid at the new spot, the choice keeps it unchanged].
  3. `Ai::low_update` walked without first running `Game::update_package`, and
     `Ai::relocate` left a Low-process actor's `owed` timer un-clamped so a far update
     could fire during the 1.8 s `MoveTo` loop before `StalkersOn` was cleared [design:
     `low_update` updates the package first when `runs_packages()` is true, and `relocate`
     caps a non-`Coming` low-process actor's owed time to keep at least
     `SCRIPT_MOVE_HOLD` (10 s) before its next far walk].

## 10. A Find package for the player: Cachino in Gomorrah (issue #2473)

- **The data** [observed: `fnvtool ai show --actor VMS21CachinoREF`, `script decompile`,
  `dialogue lines`, `world refs --cell Gomorrah01`, FalloutNV.esm]:
  - Cachino (`VMS21CachinoREF` 0x0011118D, base `VMS21Cachino` 0x0010C6F5) is placed
    initially disabled in Gomorrah01 ("Gomorrah Main Level") at (7159, 1281, 6400).
  - The lines where the player asks Gomorrah's people about him enable him: the end
    scripts of INFO 0x001750C7 / 0x001750A9 (topics `vDialogueGomorrahGenericTopic003` /
    `005`, the player's two prompts asking where Cachino is), 0x001750CD (`Topic007`), 0x001750B4, 0x0016318A, 0x0016318B, 0x0011663F
    (`Topic006`) and the receptionist's 0x001537FF all run `0x0011118D.enable` (and
    `setobjectivedisplayed 0x00136166 27 0`; 0x001750C7 also sets VMS21's
    `icachinoquestions` to 1, 0x001537FF sets VMS21's objectives 1 done, 10 shown). Two of
    those lines tell the player that Cachino will come to them: 0x001750CD (he will be sent
    to the player, who should wait at Brimstone, the bar of the main level) and 0x001750B4
    (he will find the player).
    No quest stage is set and no script calls StartConversation, EvaluatePackage or
    AddScriptPackage on him at that point.
  - His package list, in order: `VMS21CachinoNeroSit`, `VMS21CachinoSalSit`,
    `VMS21CachinoDialoguePackage2` (a Dialogue package of the boss-room scene, VMS21
    variables 5 >= 2 and 28 == 0), `VMS21CachinoBossRoomTravelWait`, then
    **`vGomorrahCachinoDialoguePackage`** (PACK 0x00119485), then
    `DefaultSandboxLinkedMarker256NoSleepNoWander`. The first four read quest variables
    of the later scenes; the fifth is the one for this moment.
  - `vGomorrahCachinoDialoguePackage` is a **Find** package (type 0, not Dialogue), no
    flags, schedule any time, location "near editor location" radius 0, target the
    specific reference PlayerRef with count / distance 0, no begin / end / change topic
    or script. Conditions: `GetTalkedToPC == 0` (subject) AND (target `GetInCell
    Gomorrah00 == 1` OR `Gomorrah01` OR `Gomorrah02` OR `Gomorrah03`): valid while he has
    not talked to the player and the player is in any of the four Gomorrah cells.
  - His GREETING lines (quest `vDialogueGomorrah`) start, for VMS21 before stage 10 and
    objective 31 not shown, with the Say Once INFO 0x001133A0 (about the player asking
    questions about him), then 0x001164B1 (for meeting the player again).
- **What follows from it** [hypothesis]: nothing but the package can bring him to the
  player, and nothing but a conversation ends it (GetTalkedToPC turns 1 only once he has
  talked to the player). So the original's Find, for a living actor named as its target,
  goes to that actor where it is, and on reaching the player it talks: the documented
  "Find: Idles play when 'activating' the target of the find" (Category:Packages) reads
  as the actor activating its target, and an actor activated is talked to. The owner saw
  it in the original: he walks up soon after the player asks and asks why they are
  asking about the family [owner statement 2026-11-14, issue #2473]. The masters'
  `DefaultFindPlayerPackage` (0x00140D3E) has the same shape (near editor location,
  radius 0, PlayerRef), named for finding the player wherever they are.
- **Before #2473** [observed in ours, smoke `cachino-find-greet` on main]: a radius 0
  search area was 512 units around the location (§3.1, our choice), for a named
  reference too; the player asking at the bar or the floor, farther than 512 from his
  spot, was not in the area, so he found nothing and stood still ("finds nothing more"),
  looking again every 10 s. Reaching the player only stood him there.
- **What is built** [design, #2473]: a Find whose target is one reference and whose
  location has no radius (0) has no search area: it goes after that reference wherever it
  is (another cell too, over the navmeshes and through doors). A location with a radius
  keeps its area (`VMS18HeadWaiterGetFood` 128 around the pie, `NellisFindJanet` 128).
  A Find that reaches **the player**, without Allow Killing and not in combat, starts a
  conversation as a force greet (`Game::force_greet`, its GREETING; a clear line to the
  player as for Dialogue packages, `Game::greet_sight`, and held while a menu or a
  loading screen is up). With nothing to say the package is over as before. Of the 14
  Find packages of FalloutNV.esm with a specific reference target, five name the player:
  this one, `DefaultFindPlayerPackage`, `BMTabithaPissedFindPackage` (Weapon Drawn; no
  actor lists it and no script adds it) and Fallout 3's `FFEU02/05GreetPlayer`, named
  for the greeting.
- **Open**: how near the original's actor comes before it talks, how long it takes to set
  off, and whether a Weapon Drawn Find to the player talks too (observer check 176).

## 11. Package places no walk reaches: the five actors of issue #2405

Players' reports of build 0.7.0 had five named actors log "cannot reach the place of …:
the goal cannot be reached (0 triangles searched)" every game hour. That text is
`fnv_nav::PathError::Unreachable` with nothing searched: the door search
(`NavGraph::door_route`) found no chain of usable doors between the two walkable areas,
so no triangle was searched; both places were on the navmesh [observed: the code,
`crates/fnv-nav/src/path.rs`]. Per actor [observed: `fnvtool ai show`, `ai run`, `nav path`,
`esm decode`, `script decompile`, `world refs`, FalloutNV.esm]:

- **GomezREF** (0x0015A02E, SafehouseNCRINT). Placed in a hidden side room at (7691, 742,
  6127), on a navmesh island of two triangles. `SafehouseNCRRangerLeave` (Travel, near
  `GomezDoor` 0x0015A02B at (2811, 951, 6438), by the cave exit, another island of the
  same cell) is valid while `vSafehouse` variable 10 is 1; that variable is
  `GomezGoToSleep` (SCRI 0x00159A99, SLSD index 10). No door joins the islands; the data
  moves him between them only by scripts (the side room's trigger `MoveTo GomezDoor` when
  his Enter patrol walks into it; the door trigger `MoveTo GomezSleep` and
  `SetUnconscious 1` while `GomezGoToSleep` is 1). So whenever Leave is chosen while he
  stands in the side room, no walk gets him there.
- **AtticusREF** (0x0015A04E, SafehouseLegionINT): built the same way
  (`SafehouseLegionVeteranLeave` to `AtticusDoor`; `AtticusGoToSleep` is variable 14).
  His base `SafehouseLegionVeteran` (0x0015A029) has "No Low Level Processing", so out of
  the loaded area he is never updated (`fnv_ai::process`): his reports can only come
  from the high process, with the player in the safehouse.
- **SSHQMalcolmHolmesREF** (0x0010651A). `SSHQMalcolmHolmesWaitsPackage` travels to his
  editor location in the holding cell `RandomEncounterNPCs` while `VFreeformSSHQ`
  variable 34 is 0 and the player (run on reference 0x14) is not in WastelandNV
  (GetInWorldspace != 1). The holding cell has no door. So once he has met the player in
  the wasteland, the player going into any interior or other worldspace (the reports:
  SLJeanSkyDivingINT, SLniptonRoadMarketINT) sends him to a place no door leads to; he
  is then out of the loaded area.
- **GibsonDogReyRef** (0x00084204, GibsonScrapYardInterior). `DefaultFollowLinkedClose`
  follows his linked reference `OldLadyGibsonRef`, whose day packages
  (OldLadyGibsonPackageSit) take her out of the shack's door (0x000841F5) to her chair in
  WastelandNV. His base `GibsonDogRey` (0x000CD230) has "Can't Open Doors" (and "No Low
  Level Processing"), so by §7.1 no load door is open to him: reproduced headless, an
  Unreachable at every hour she sits outside.
- **Ghoul5** (0x00087AEF) and the other REPCONN ghouls in RocketLabTop.
  `REPCONGhoulRocketWalkPACKAGE` (valid at VMS01 variable 25 == 3) travels to
  `JasonBrightDialogMarker` (0x0008D47A) in RocketLabBasement. The way is Top → Mid
  (0x000829EB) → Basement through `REPCONMidToBasementDoor` (0x000840AC), which is locked
  (level 25, key `REPCONLowerKey` 0x000810E8, which no script gives to the ghouls; the
  only AddItem of it is to the player). No script unlocks that door. But the trigger
  `REPCONTopIntercomTrigger` (0x000CD683, script 0x000CE1BB) stands at (512, 2734) in
  RocketLabTop, just in front of the door to Mid at (512, 2940): while VMS01
  `nightkinquest` is 3, any actor but the player and teammates entering it is moved on
  by its script (Chris Haversam to 0x000CE1C5 and disabled; Jason Bright `MoveTo
  JasonBrightDialogMarker`; the rest given clothes and `MoveToMarkerWithFade
  REPCONRocketPadDestinationMarker` 0x000CE1B1, all in the basement). So the data expects
  the ghouls to *start* their walk and be taken off it by the trigger, two doors short
  of the lock; ours never starts a walk whose whole way is not open.

**What is built** (#2405):

- **Out of sight, no door at all** [hypothesis: the data moves such actors only by
  scripts or not at all; observer check 145 asks the original]: an actor out of the
  high process (no body drawn) whose walk to its package's own place (a Travel's, a
  Sleep's, a Sandbox's centre; not a patrol point, not a walk of its own) has no way, and
  between whose place and that place there is no door at all, locked and disabled ones
  counted (`NavGraph::joined_by_doors`), is put at the place and has arrived
  (`AiEvent::PutThere`, engine log "put at the place of … out of sight, no door joining
  the two"). Gomez out of sight lands at GomezDoor, inside the door trigger that puts him
  to sleep when the player comes in and the cell loads (`script_trigger`: an actor
  touching a trigger in a loaded cell fires OnTriggerEnter); Holmes goes back to his
  holding cell. A disabled door between the two still joins them, so Loyal behind the
  B-29 hangar's disabled doors (#2566, check 160) is not moved. In the loaded area
  nothing changed: the package is set aside as before.
- **A creature that cannot open doors** whose way (to its package's place, or after
  the one it follows or goes up to talk to) needs a load door stays, and the package is
  set aside as for no way, but it is told apart (`AiEvent::DoorsClosedToIt`, engine log
  "stays: the way to the place of … needs a load door, and it cannot open doors"), not
  reported as a path failure: Rey stays in the shack while Old Lady Gibson sits outside.
- `fnvtool ai run --out-of-sight` runs the named actors at the Low level; smoke
  `ai-no-door-out-of-sight` (headless) and `ai-no-door-out-of-sight-game`.
- `ai reach` is unchanged: it answers whether an actor can walk there, and its verdicts
  ("no way there", "only through a door the creature cannot open") are as before.

**Left open**:

- Gomez and Atticus with the player in their safehouse (the high process): still set
  aside and reported every hour. Putting a drawn actor somewhere needs its body moved
  as a MoveTo does; not built, pending check 145. Atticus, with No Low Level
  Processing, is only ever in that case.
- The REPCONN ghouls: a way closed only by a locked door is still no way, so the walk
  that would take them into the intercom trigger never starts. Walking as far as the
  locked door would change what every actor behind a locked door does (54 actor-package
  pairs in FalloutNV.esm, `ai reach` "only through a locked door the actor has no key
  for") and the engine's settled rule (tests in `fnv-ai/tests/carry.rs`); observer check
  204 asks the original first.

### 11.1 The quest pilot's cases: Yes Man, Desmond, Sally (2026-11-19)

The quest pilot's sweep (PR #2401) logged the same "0 triangles searched" for three
quest actors. "0" did not mean what §11 says: `NavGraph::search_path` dropped the
triangles of a walk leg whose search failed, so a way inside one area closed by a door
that opens in place also read "0 triangles searched" [observed: the code; headless
`fnvtool ai run --actor DLC04DesmondREF` said 186 where `nav path` said 0 for the same
way]. Per actor [observed: `fnvtool ai show`, `nav path`, `nav graph`, `esm decode`,
`script decompile`, the pilot's logs under `target/pilot-rerun-*/logs`]:

- **Yes Man, `VMQTopsYesManREF2`** (dam-panel, the Independent ending). Stage 90 of
  `VHDIndependentBattle` gives him `VHDYesManEndDialogue` (Dialogue, Must Reach Location,
  Must Complete, target PlayerRef). He stands at the Legate's camp in WastelandNV
  (102641, 18724), beside `vHDGeneralOliverRef2` (0x001483F1, at (103040, 18736)). The
  pilot had the player talk to the other placed Oliver, `VHDGeneralOliverRef`
  (0x00134DB4, same base `VHDGeneralOliver`), in `HooverDamIntOliverArea`, putting the
  player there as a fast travel would because no navmesh way leads in. No door chain joins
  that cell to WastelandNV, locked or disabled doors counted: its one door
  `VHDOliverIntDoorREF` leads to `VHDOliverDoorREF` in `HooverDamIntPowerPlant04` (locked
  255), which no way from Yes Man reaches either. So the engine was right that he cannot
  walk to the player there. The Oliver of this ending is the one at the camp
  [hypothesis: from the placements; the scene's lines are the base's, so either reference
  says them]. **A pilot chain fix, not an engine one**: talk to `vHDGeneralOliverRef2`.
- **Desmond, `DLC04DesmondREF`** (The Local Flavor; TTW). His waits and greetings lie
  beyond a doorway of `DLC04Mansion01` with two doors in it, one of them initially
  disabled and locked (navmesh.md, "In-place doors not in the world"). Our graph counted
  the disabled one as a barrier and a door stop. In the game its lock was already left out
  (`Ai::apply_door_locks` withholds a disabled door's lock), so the stop was the only live
  effect. The pilot's run still reports the place unreachable after the spawn door opened
  (`target/pilot-rerun-1/logs/ttw-dlc04mq01-the-local-flavor.log`, 10:11), and the logs
  do not say which door closed it [observed]. With the reports below, the next run
  will say.
- **Sally, `DLC05SallyREF`** (Mothership Zeta 1 and 2; TTW). In MZ2 the force field
  `0x0F00C8C8` (`DLC05MZDoorSmFieldDown`) refused her by its script, and its door
  opened in the same moment; her package was set aside for a game hour all the same
  [observed: rerun-0 log, 04:09:46]. In MZ1 the holding cell's door `0F0068DE` stays
  locked because the chain's `DLC05MZ1SallyDoor` variable never got to 2
  (pilot divergence at step 56): quest state, not navigation.

**What is built** (#2405, 2026-11-19):

- The count of a failed search includes the leg that failed: "0 triangles searched" again
  means no search ran (no chain of doors, a refusing door at an endpoint, a goal in water)
  [design].
- An unreachable report says why [design]: `fnv_nav::NavGraph::doors_closing` looks for a
  way with every door open (locked, opening in place, refused by a script; not doors that
  are not in the world). The doors on it that are closed to the actor are named, each with
  its lock ("the way is closed by door 0x0E0035BB (locked, level 255)", "… (its script
  refused it)"). With no way even then, and no door at all between the two places, the
  report says "no door at all joins the two places".
- A door that opens in place and is not in the world bars nothing and gets no stop
  (navmesh.md).
- **A genuinely unreachable place** [hypothesis: the original's handling is not
  documented; docs/design/actor-behaviour.md, "No way there"]: as before, the package is
  set aside for a game hour and the actor takes the next valid package of its list (§11
  keeps its out-of-sight rule). Its actor need not wait out the hour when the way opens
  sooner: a door unlocked, or a door enabled or disabled, already brought the package back
  (`Ai::apply_door_locks`, `Ai::apply_door_presence`). Now a door that opens in place,
  named by the report and then opened (by a script's SetOpenState or OnActivate, the player,
  another actor), does too (`Ai::apply_doors_opened`) [design]. Sally behind the force
  field is the case. A Dialogue package whose target the actor cannot reach is not
  carried out from afar: no observation says the original talks across spaces, and the
  one case (Yes Man) was the chain's [design]. StartConversation from afar keeps its own
  rule (dialogue.md).
- Tests: `fnv-nav` `a_door_that_opens_in_place_and_is_not_in_the_world_bars_nothing`;
  `fnv-ai` `a_door_that_opens_in_place_bars_the_way_only_while_it_is_in_the_world`,
  `a_door_that_closed_the_way_opening_brings_the_package_back`. `ai reach` over the ten
  base-game masters gives the same verdicts as main (16,935 pairs).

**Left open**: what closed Desmond's way at 10:11 in the pilot's run (rerun the chain
and read the named door); whether the original's actors wait at a refusing door or set
the package aside; the REPCONN ghouls (§11, observer check 204).

### 11.2 Issue #2756's actors: goals off the navmesh and islands (2026-11-25)

Players' reports of builds 0.7.0 and 0.8.0 named four more actors [observed: `fnvtool ai
reach`, `ai show`, `ai run`, `nav near`, `nav path`, `esm decode`, FalloutNV.esm]:

- **VFSJeanBaptisteCuttingREF**, `VFSJeanBaptisteStandInPlacePackage` (Travel, Must Reach
  Location, the last of his list). Its marker `VFSJeanBaptisteMarkerREF` (0x0012411E) is
  placed at exactly (0, 0, 0) in FreesideSilverRush; that cell's nearest navmesh triangle
  is 3,007 units away. The goal is looked for in the marker's cell (the place of a
  reference is its cell's space, `goal::place_of_ref`), not in Freeside where he stood
  when the reports logged it; the line gave only the coordinates, which read as the
  other. The log now names the space of a place with no navmesh near it ("… near the
  goal (0, 0, 0) in FreesideSilverRush") [design]; smoke `ai-goal-off-navmesh-named`.
  Nothing to walk to, so he stays where he is, as the package's name says [hypothesis].
- **VHDPreBattleNCRTrooper3REF**, `VHDPreBattleExtGuardBarricade3`/`4` (Guard, radius 0).
  Both markers stand at z 4825, 350 units above and 570 to 810 units across from the
  nearest triangle (the barricade's top, off the navmesh); trooper 1's marker is on it.
  The data's own.
- **VRRCJerrythePunkREF**, `VRRCJerrySleep` (near his linked reference 0x00167FE2). The
  marker's navmesh 0x00167FF2 is one triangle, every edge a boundary, 41 units from the
  Red Rock navmesh 0x0014E72D: an island no edge or door joins.
- **Boomer00FREF**, 0x00108061, 0x00108067, `NellisBoomerSideTowerGuard1/2/3AIPackage`
  (Patrol at 14:00). `NellisSideTowerGuardMarkerREF` is on a tower's top (z 4685 to
  4702, about 350 above the ground navmesh), a 26-triangle island with no door or link
  down.

Out of sight such an island gets them put there (§11, the no-door rule); in the loaded
area they stay and say so. Whether the original's actors climb onto islands that the
records do not join is the question of observer check 279 (#3473, the Sunset
Sarsaparilla HQ's stairs); these follow its answer.
