# Spec note: actor behaviour (AI packages, detection, factions, combat, idles) as the editor documentation describes it

Layer 6 groundwork. The source is a private export of 7 GECK wiki pages (geckwiki.com)
the owner made on 2026-10-09, kept at `C:\fnv-private\geckwiki\GECK-20261009092949.xml`
and never committed: Category:Combat, Category:Detection, Category:Dialogue,
Category:Factions, Category:Packages, Idle Collection and Idle Markers. Category:Dialogue
is summarised in quests-dialogue.md §4; only what that note lacks is here (§6).
Everything below is our own summary; no wiki text is copied (checked mechanically: no
run of 8 lower-cased words in common with the export).

Tags:
- **[documented: P]** stands for "[documented: GECK wiki P, owner export 2026-10-09]";
  **[documented: catalogue notes]** is our paraphrase of a function's wiki page in
  `schemas/script/functions.json` (script-functions.md §1);
- **[observed]**: the AI observer runs in the original game (ai-observer.md §7) or the
  Layer 5 captures (layer5-captures.md);
- **[hypothesis]**: neither.

"Runtime" means `crates/fnv-game` (package choice, factions; scripts.md §7.3, §7.5,
§7.6) and `crates/fnv-actor` (IDLE records; animation.md) as of `main` 8c56374, read
only. Differences and gaps are collected in §7.

## 1. AI packages

### 1.1 What a package is

- A package is a set of instructions for an actor plus the conditions under which it
  applies; nearly all actor behaviour comes from packages, the rest is built-in default
  behaviour [documented: Category:Packages].
- Each package has an ID, a **type** and an optional **combat style** used while it runs
  [documented: Category:Packages].
- **Types** in the editor: Accompany, Ambush, Dialogue, Eat, Escort, Find, Flee (not
  combat), Follow, Guard, Patrol, Sandbox, Sleep, Travel, Use Item At, Use Weapon. The
  first tab holds the type's own settings [documented: Category:Packages]. In the game the
  observer also saw packages created at run time (FormIDs 0xFF……) of kinds outside this
  list, GET_UP whenever a seated actor changed package, and COMBAT during the gunfight
  [observed: ai-observer.md §7].
- **Standard location** (used by many types): near the editor location, near the current
  location, in a named cell (no radius then), or near a reference, with a radius
  [documented: Category:Packages].
- **Standard search** (types that look for something): a search location plus "allow
  buying / killing / stealing" and "replace after use" (put a picked-up object back at the
  end; not for Eat and Find) [documented: Category:Packages].

### 1.2 Flags

Flags that do not apply to the chosen type are greyed out [documented: Category:Packages].
Paraphrased [all documented: Category:Packages]:

| Flag | Meaning |
|---|---|
| Must Complete | the package does not end until its goal is met (target found, location reached …) |
| Must Reach Location | the package does not end before the location goal is met |
| Continue if PC Near | does not finish while the player is near (keeps shops open past their hours) |
| Once Per Day | not chosen again within 24 hours of the moment the actor first chose it, even if its conditions still pass |
| Always Run / Always Sneak | movement style for the whole package |
| Allow Swimming / Allow Falls | without them the actor will not swim to a destination, or drop from a height that hurts |
| Offers Services | a service provider offers services only while this package runs, and tries to keep the player in view when idle |
| No Idle Anims / HeadTracking Off | no idle animations; head fixed forward |
| No Combat Alert | the actor skips the search behaviour for enemies detected between −20 and 0 (§2.3) and fights only on full detection (> 0) |
| Weapon Drawn / Weapons Unequipped | weapon out for the package; or no weapon shown although one is equipped |
| Pretend in Combat | the actor enters combat when the package starts and moves between cover points toward the destination; a real target interrupts the trip until it is dead |
| Continue During Combat | if combat starts, the package goes on (walking to the destination while firing and using cover). What that means differs by package type [documented: "Combat Package Flags"; observed 2026-11-12]: docs/design/alert.md §5.2 |
| Defensive Combat | the actor attacks no one unless attacked first (normally it attacks those it hates and friends who steal from it); for Accompany, Escort and Follow it still joins when its target is attacked |
| Fallout behaviour switches | hellos to the player; random conversations (only Sandbox, Guard, Eat and Patrol packages start them); observing nearby combat (moving away from fighters, which interrupts the package); reacting to player actions through the default topics; friendly-fire comments (Hit topic); aggro radius; idle chatter (IdleChatter topic); avoiding radiation |
| Door flags | lock or unlock all doors in the cell when the package starts, keep them so while at the location (undoing changes by others), or at the end |

### 1.3 Schedule and conditions

- The schedule says when the package may run, in blocks of at least one hour; it should
  use either a day and month or a day of the week, not both [documented:
  Category:Packages].
- The conditions must all hold for the package to be chosen, and they count only while the
  schedule allows the package. "Run on Target" is allowed only when the package names a
  target [documented: Category:Packages]. The condition Run On meanings for packages
  (subject = the actor, target = the package target) are in quests-dialogue.md §3.
- **Choosing among packages** is not spelled out in the export beyond "schedule valid and
  conditions true". The runtime takes the **first package of the actor's list whose
  conditions pass** [observed for three settlers and Sunny Smiles,
  `L5.quest.condition_groups`; list order deciding in general is a hypothesis,
  scripts.md §7.5]. It ignores the schedule (PSDT is not decoded).
- **When** the game chooses again is not documented either. Observed: schedule changes
  showed 0–13 game minutes after the hour, and actors in unloaded interiors changed
  together in one sample [observed: ai-observer.md §7; a re-evaluation interval is the
  hypothesis there].

### 1.4 Begin, End and Change; script functions

- Each package has three state changes: **Begin** (the OnPackageStart block), **End**
  (also called Done; OnPackageDone) and **Change** (OnPackageChange). Each can play an
  idle, say a topic and run a result script; **the result script runs first**, so the
  topic and idle see what it set [documented: Category:Packages]. These are the PACK
  `begin`/`end`/`change` units after POBA/POEA/POCA, and `OnPackageEnd` in source is
  OnPackageDone [observed: scripts.md §1.3, §3.4]: agrees.
- The End idle plays only for package types that have a done state; a Must Complete
  package lets its End idle finish before the next package starts; a Begin script does
  not run again when a save is loaded with the package already running [documented:
  Category:Packages]. The last point is Layer 7 (saves).
- The export has no page on the package functions. From the catalogue notes:
  - **EvaluatePackage** makes the actor choose from its list again, dropping even a
    must-complete package; it does not touch combat or pathing (ResetAI does)
    [documented: catalogue notes].
  - **AddScriptPackage** gives the actor a package that overrides all others until it
    finishes; an actor holds **one** script package and a second call replaces the first;
    without must-complete or must-reach-location it is dropped at the next re-evaluation;
    it is saved with the game [documented: catalogue notes]. Captured: after
    AddScriptPackage A then B, one RemoveScriptPackage brings back the actor's list
    package, not A; EvaluatePackage with a script package on keeps it (Ringo,
    DefaultStayAtEditorLocation then DefaultSandboxEditorLocation256, which was still
    current 10 and 300 loops after EvaluatePackage) [observed: capture job
    `od_08_script_packages`, 2026-10-18, owner-decision-captures.md]. So the second rule
    ("dropped at the next re-evaluation") does not hold for EvaluatePackage, at least for
    a sandbox package.
  - **RemoveScriptPackage** removes the running script package so the actor chooses
    again; it cannot remove packages from the editor list [documented: catalogue notes].
  - Runtime: §7.2 rows 1 and 2.
- The runtime does not start packages itself: the begin result script and OnPackageStart
  run only through the runner's `package` input (scripts.md §7.5, §8).

### 1.5 Package idles

- A package may carry an idle collection (§5.1), so its actor does not need the idle
  manager for those idles [documented: Category:Packages].
- When they play depends on the type: Travel at the destination; Patrol at a patrol point
  (unless that point is an idle marker, furniture or an activator); Eat and Sleep while
  eating or sleeping; Follow, Escort and Accompany while waiting for the target to move;
  Dialogue while talking (instead of the idle manager's talking idles); Sandbox and Wander
  when not moving or using anything; Flee at the flee destination; Find while activating
  the found target; Use Item At at its location; never for Use Weapon [documented:
  Category:Packages].
- Only two things override package idles: the idles of an idle marker the actor is at,
  and an idle named on the dialogue line being spoken [documented: Category:Packages].

### 1.6 Known problems in New Vegas

[documented: Category:Packages, bugs and notes]
- Packages may behave differently from Fallout 3.
- Unloaded actors may not run their packages at all, whatever their persistence or quest
  status. Observed: unloaded actors kept their position for hours, but appeared at their
  new package's location when the package changed [observed: ai-observer.md §7].
- Door flags, door-locking packages and height (Z) checks for destinations and marker
  distances are unreliable.
- An actor with "No Low Level Processing" travels only while the player is near.
- Many "Fallout behaviour" switches cost performance with many actors around.
- Paths through doors depend on how each cell is turned, so cells need north markers.

## 2. Detection and the stealth model

The page is marked incomplete [documented: Category:Detection]. Defaults below are the
game-setting defaults the page gives; the masters may set other values (§8 item 8).

### 2.1 The detection value

An actor's detection value for a target is the sum of a flat base, a sound term, a
visual term, and the detector's skill minus the target's skill times a factor
[documented: Category:Detection]. In our notation:

    D = b + S + V + m·(Kd − Kt)          b = fSneakBaseValue (−35), m = fSneakSkillMult (0.5)

- **Distance falloff** for each term: `a = ((R − d) / R) ^ p`, with `R` =
  fSneakMaxDistance (2500, times fSneakExteriorDistanceMult = 2 outdoors) and `p` =
  fSneakDistanceAttenuationExponent (2). Without line of sight the sound term is further
  multiplied by fSneakSoundLosMult (0.1).
- **Sound**: `S = a · fSneakSoundsMult (1.6) · (move + act)`.
  `move` is 0 for a target standing still, else `(fSneakBootWeightBase (12) +
  fSneakBootWeightMult (0.5) · armour weight) · r`, where `r` = fSneakRunningMult (1.5)
  when running and 1 otherwise; a perk entry point ("Ignore Running During Detection")
  removes the running factor. `act` = the weapon's action sound times fSneakActionMult
  (2); firing is 100 loud, 50 normal, 10 silent (iSoundLevelLoud/Normal/Silent).
- **Visual**: `V = a · seen · L · M · B · I`, where `seen` is 0 if the detector cannot see
  the target and 1 otherwise; `M = 1 + 0.01·moving + 0.2·running`
  (fSneakLightMoveMult, fSneakLightRunMult); `L = (fDetectionSneakLightMod (0) + light ·
  n) · fSneakLightMult (1.4)`, with `n` = fDetectionNightEyeBonus (3, the product capped at
  100) when the detector has a non-zero NightEye value and 1 otherwise; `B` (blindness) is
  always 1 in New Vegas; `I` = fSneakStealthboyMult (0) when the target's Invisibility or
  Chameleon is at least 1, else 1.
- **Detector skill** comes from Perception, not Sneak: `P = 10 + 80 · PER / 10`
  (fSneakPerceptionSkillMin 10, Max 90), then `Kd = a · P · (1 + 0.2·inCombat −
  0.2·asleep − 0.4·inCombatWithSomeoneElse)` (fSneakAlertMod, fSneakSleepBonus,
  fSneakCombatMod) [documented: Category:Detection, setting descriptions; observed:
  captures run15b_t1 and run15b_t2, 2026-11-15, §2.1a]. The page's formula instead
  writes the last term `IsCombatTarget * fSneakCombatMod`, and IsCombatTarget is 1
  for the detector's own combat targets [documented: GECK wiki IsCombatTarget].
  The Fallout Wiki's Sneak (New Vegas) page also gives 0.8 against its own targets,
  1.2 otherwise [documented: owner's saved copy, 2026-10-24]. The captures settle
  this disagreement in favour of the GECK setting description: 1.2 against its own
  targets, 0.8 against anyone else while in Combat [observed: §2.1a]. Lost remains
  1.2 [design: docs/design/detection.md §9; not State::Combat].
- **Target skill** (New Vegas): `Kt = sneaking · (Sneak + 5·(Lt − Ld) + max(50 − 10·Lt,
  0) − armour)`, with `Lt`, `Ld` the levels of target and detector (iSneakLevelBonus 5,
  iSneakStartBonus 50, iSneakStartBonusLevelPenatly 10), `armour` 10 for medium and 20
  for heavy armour in the torso slot, and `sneaking` 0 or 1. (Fallout 3 used `Sneak ·
  sneaking` only.)
- Observed: a wandering settler detected the player at 499–652 units at some moments and
  not at others, not as a function of distance alone [observed: ai-observer.md §7]; the
  visual term (`seen`, light) would explain it [hypothesis].

### 2.1a Combat distraction (#2573, answered observer check 200)

[observed: captures run15b_t1 and run15b_t2, 2026-11-15] xNVSE 6.4.9 and JIP LN
57.30 `GetDetectionValue`, Searchlight Airport's runway, GameHour 12; the player
sneaking, standing still, in god mode. Detectors GoodspringsSettler01Ref 0x00104F03
and GoodspringsSettler03Ref 0x00104F0A both had Perception 5 and line of sight.
"Fighter" used `StartCombat player` (IsInCombat 1, IsCombatTarget player 1);
"watcher" used `StartCombat` on a restrained gecko after RemoveFromAllFactions and
Assistance 0 (IsInCombat 1, IsCombatTarget player 0). Run 2 swapped the NPCs' roles.

[observed: captures run15b_t1 and run15b_t2, 2026-11-15] Distances and readings:

| State of the detector | Distance to the player | GetDetectionValue player |
|---|---|---|
| Not in combat, facing the player (both NPCs, both runs) | 695 | 79 |
| Not in combat, walking (both NPCs) | 669-674 / 650 / 633-640 / 621-627 / 601 / 567 | 80 / 82 / 83 / 84 / 85 / 86-87 |
| In Combat, the player its combat target (run 1) | 687-693 / 672-685 / 650 / 627 | 83 / 83-84 / 85 / 87 |
| In Combat, the player its combat target (run 2, the other NPC) | 695 / 675-681 / 650 / 618-627 | 83 / 84-86 / 86 / 87-88 |
| In Combat with the gecko, the player not its target (run 1) | 609-636 | 81 |
| In Combat with the gecko, the player not its target (run 2) | 547-591 | 83-84 |

[observed: captures run15b_t1 and run15b_t2, 2026-11-15] At comparable distances,
a fighter read its own target 3-4 higher than the same NPC out of combat, in both
role assignments. Fighting someone else read the player 2-4 lower; those samples
were fewer because line of sight came and went. This settles the direction in
favour of the setting's description: fSneakCombatMod applies to a detector in
Combat against actors outside its combat target list, giving 0.8 at defaults;
its own targets retain 1 + fSneakAlertMod = 1.2. The wiki formula's IsCombatTarget
reading and the Fallout Wiki's 0.8 / 1.2 table are reversed (§2.1).

[observed: captures run15b_t1 and run15b_t2, 2026-11-15] `SetRestrained 1` on a
combatant ended combat (IsInCombat 0 from the next sample), as `tcai` did.
GetDetectionValue read -100 for a detector in combat without line of sight.
These captures did not time detection loss or the search after breaking sight.

[design, #2573; tools/smoke/scenarios.toml] `detection-combat-target-sneak` uses the
cleared Searchlight Airport runway at hour 12 with god mode. The player sneaks from
2 s and stands still. Fighter GoodspringsSettler01Ref (00104F03) and watcher
GoodspringsSettler03Ref (00104F0A) have Perception 5 and Assistance 0; the watcher
is removed from all factions. A gecko is created at the watcher with Aggression 0
and Perception 0, then positioned 150 units toward the player. Both settlers reset
to mirrored 690-unit offsets at 5.8 s and the gecko resets at 5.9 s. At 6 s the
fighter starts combat with the player and the watcher with the gecko. Five paired
GetDetectionValue/GetDistance reads run every 0.5 s from 6.6 to 8.6 s. Every fighter
value must exceed its paired watcher value; distances are logged without absolute
ranges.

[observed: smoke runs on the combined build, 2026-11-15] With the fix, both
detectors at 690 units read fighter 64 / watcher 56; the fighter closing to 610 read
69 (PASS). With both detection.rs files reverted to upstream main, fighter 84-88 /
watcher 91 in all five pairs (FAIL).

### 2.2 Detection events

- An event has an owner (the actor who caused it), an action rating (its loudness, which
  enters the formula in place of the action sound) and a location [documented:
  Category:Detection].
- Events come from projectile impacts (grenades, rockets …), exploding world objects
  (owner: whoever set them off) and CreateDetectionEvent [documented]. The catalogue adds
  that CreateDetectionEvent takes an owner, a level and an optional type (bullet 0, beam
  1, explosion 2, generic 3) [documented: catalogue notes].
- Actors within fSneakMaxDistance test the event like a target. They ignore it if they
  would not act aggressively toward its owner, or if they are already in Combat; one that
  detects it (above −20) becomes Alert, and an Alert one adds the owner to its target list
  [documented: Category:Detection].
- Without an event, an Alert actor re-centres its search on an enemy that stays within
  the alert band after each full search, and an actor hit by an unseen enemy searches
  anyway and finishes one full search before leaving combat [documented].

### 2.3 Detection states

- Four states: **Normal**, **Alert** (aware of an event; weapon out, perhaps searching,
  higher Perception), **Combat** (fighting someone on its target list) and **Lost** (in
  combat but every target is undetected; like Alert) [documented: Category:Detection].
  Alert, Combat and Lost are all combat states [documented].
- Thresholds: −20 (fSneakNoticedMin) is the alert threshold; a target counts as detected
  above 0 [documented].
- Transitions [documented: Category:Detection], for a hostile actor (one the detector
  would attack if it saw it):
  - Normal → Alert: the hostile passes −20, makes an event the detector notices, or hurts
    the detector; an event is added to the alert list (at the hostile's position, with the
    current detection as its size, in the first and last case).
  - Normal → Combat: a hostile jumps straight to detected without passing through Alert.
  - Alert → Combat: any hostile detected. Alert → Normal: the alert list is empty (all
    events expired).
  - Combat → Normal: no targets left (dead or no longer hostile) and nothing on the alert
    list. Combat → Lost: every target undetected (below 0) with its timer run out, and
    events on the alert list.
  - Lost → Combat: any target detected again. Lost → Normal: every target below −20 and
    the alert timer run out.
- **Target list**: a target that drops below −20 starts a timer; when it runs out and the
  target is still undetected and not being searched for, it leaves the list [documented].
- Alert and Lost actors search (Combat Search page, not in the export) if their package
  allows; a Lost actor that may not search goes back to its package location still alert
  [documented].
- **Barks**: Detection topics AlertIdle and LostIdle while searching, and one topic per
  transition, named `<From>To<To>`: into Combat (NormalToCombat, AlertToCombat,
  LostToCombat), into Normal (AlertToNormal, LostToNormal, CombatToNormal), NormalToAlert
  and CombatToLost [documented].
- Observed: after the Goodsprings gunfight every surviving actor left combat in the same
  sample, about 1 game minute after the last enemy died [observed: ai-observer.md §7].

### 2.4 IsActorDetected source calls

[documented: [GECK IsActorDetected](https://geckwiki.com/index.php?title=IsActorDetected)]
With no explicit arguments, this query asks whether the actor receiver is detected
by any other actor. It returns an integer boolean. A player receiver always reads
zero, and detection by the player does not count. This direction is distinct from
[GetDetected](https://geckwiki.com/index.php?title=GetDetected), whose receiver is
the detector. The documented contract is available in the primary page's indexed
text; direct retrieval returned403 during preparation.

Implementation context, not original-game evidence: the existing catalogue row has
no opcode or console category. Source compilation uses the unnumbered named VM
path. The handler reads incoming typed Detected-band pairs, excluding the player
and the receiver itself. Other actors count regardless of teammate membership;
the sneak HUD's extra follower exclusion is not this query. The allocation-free
scan stops at its first matching pair and costs O(cached pairs) in the worst case.
No native opcode, sensing pass, cache or save field is added.

[observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-24,
GSDocMitchellHouse (`0x00103DF9`), 10 official base masters, isolated staging copy
(read-only), committed probe `w4g_05_isactordetected_and_setclass_invalid` in
`tools/capture/nvse/catalogs/wave4g_f2_extenders.json` (companion catalog PR #3803);
no DLL or source consulted] `IsActorDetected` on a non-actor door (`0x00103E61`) returns
`0` and continues (`detDoor=1:0`), on an explicit null ref logs an xNVSE NULL-reference
error while returning `0` and continuing (`detNull=1:0`), and on `Player` returns `0`
even while two actors detect the player (`nDet=2`, `dP=1`, `a1P=1`). Killing a detecting
actor with `KillActor` immediately drops it from `GetDetectingActors` on the **same
tick** (`nDet=2 -> 1`), rather than waiting for deferred cache expiry. Verbatim probe
output:
- `Attempting to call a function on a NULL reference`
- `PROBE|w4g_05_isactordetected_and_setclass_invalid|invalid(detDoor=1:0,detNull=1:0|scDoor=1,scNonClas=0,docCls=00015679->00015679,scNull=1)`
- `PROBE|w4g_05_isactordetected_and_setclass_invalid|detLive(nDet=2,p=0,d=0,a1=0|gd:dP=1,a1P=1,dA1=0,a1D=0,pD=0|killSame:nDet=1,p=0,d=0,a1=0)`
- `PROBE|w4g_05_isactordetected_and_setclass_invalid|detNext(nDet=1,p=0,d=0,a1=0)`

[design] On actor death (`Game::die`, called by `KillActor` and fatal combat damage),
NVForever immediately clears detections made by the dead actor. `GetDetectingActors`,
`GetDetected`, `IsActorDetected`, and sneak-detection queries exclude the dead detector
on the same tick. This cleanup preserves living observers' cached detections of
that actor. Explicit null/non-form receivers on `IsActorDetected` log
`no_reference_call` and return `0` without falling back to `self_ref`, continuing
script execution. Missing receivers and cold/restored detection caches continue
to read `0`. Normal cache expiry remains owned by the engine's measurement round
(every quarter-second, forgetting pairs older than one game-time second).

Process qualification: six authored shared VM controls cover explicit receiver
precedence, incoming/outgoing pairs, player/self exclusion, teammates, mixed typed
bands and numeric values, overwrite/forget/expiry, cold restore and existing
GetDetected/GetDetectionLevel/GetLineOfSight neighbors. They use the real compiler,
decoder and VM; diagnostic-free zeros are required so a missing handler cannot
masquerade as a negative result. Coverage does not imply execution; results must
name their exact source and host. Historical skipped calls in CASM with MCM are
reported in [#2271](https://github.com/LootJunkieAzza/NVForever/issues/2271), not a
current CASM save-scheduling or full-mod compatibility result. Installed versions,
caller branches and native/physical execution remain separately unqualified.

## 3. Factions

- Factions decide how actors react to each other and whether members help one another:
  members of a faction allied with itself defend each other; actors in no faction stand
  by [documented: Category:Factions].
- **Flags** [documented: Category:Factions]: Hidden from PC (not on the player's sheet; no
  effect in Fallout); Evil (stealing from members costs no Karma); Allow Sell (vendors of
  the faction may sell items the faction owns); Special Combat (members may fight each
  other without raising a combat alarm, and ignore fSneakSeenMin and
  iCombatTargetPlayerSoftCap); Track Crime (members remember crimes against the faction,
  and Karma changes apply). The crime gold multiplier and rank insignia are unused. The
  schema's FACT DATA flags carry the same names [documented: xEdit, `gameplay.json`]:
  agrees.
- **Ranks**: an index, a male title and a female title (the male one is used when the
  female one is empty) [documented]. FACT RNAM/MNAM/FNAM [documented: xEdit]. A faction
  can link a reputation (WMI1).
- **Relations** to other factions: a disposition modifier from −100 to +100 and a group
  combat reaction [documented: Category:Factions]:
  - **Enemy**: any actor whose aggression is Aggressive or higher attacks it as soon as
    it sees it;
  - **Neutral**: the default for any pair not listed; only Very Aggressive actors attack
    it;
  - **Friend** and **Ally**: can never attack each other; whether one helps the other in a
    fight depends on its Assistance setting.
  Aggression and Assistance are actor AI settings (the AI Window page, not in the export;
  AIDT in `gameplay.json`).
- Runtime (scripts.md §7.6): reactions from FACT XNAM, SetEnemy/SetAlly overrides,
  neutral when unlisted (agrees); an actor's factions are its template's with the
  template flag "Factions", through leveled lists, and a reaction is read from the
  actor's factions towards the other's (factions.md §2–§5, which also has the counts of
  who is an enemy of the player); faction ranks with −1 for a non-member [hypothesis];
  crime-enemy flags kept for GetActorFactionPlayerEnemy; GetDisposition is a stub.
  How several factions of one actor combine in GetFactionRelation: a friend or an ally
  pair wins over an enemy pair, as allies "cannot attack each other under any
  circumstances" [documented: GECK wiki GetFactionCombatReaction]; friend before ally is
  a [hypothesis] (factions.md §4, #2560).
- Observed: in the gunfight, Goodsprings animals fought on the settlers' side
  [observed: ai-observer.md §7]; which faction relation or Assistance setting caused it is
  not known.

## 4. Combat

- The export's combat page covers only gun sway: the aiming animation moves the weapon
  around the crosshair and a shot leaves along the weapon's actual axis, so the crosshair
  is not the true aim (the spread formula page is not in the export) [documented:
  Category:Combat].
- Combat styles are set per package (§1.1) [documented: Category:Packages]; the CSTY
  record is not described in the export and not in our schema yet.
- Combat-related package flags (Pretend in Combat, Continue During Combat, Defensive
  Combat, No Combat Alert, Weapon Drawn) are in §1.2.
- The gunfight observations (start order, targets, retargeting, deaths, end) are in
  ai-observer.md §7; nothing in the export contradicts them.

## 5. Idles

### 5.1 Idle collections

- An idle collection is a list of idles played instead of letting the idle manager
  choose; idle markers and packages use them [documented: Idle Collection].
- Only idles that name an animation file can be listed; each entry may be flagged
  Begin/End, which counts only on package idles [documented].
- Picking: **Random** (any listed idle), **Run in Sequence** (from the top), and **Do
  Once** (play the sequence once instead of starting it over after the timer)
  [documented].
- The **idle timer** is the wait before a new pick: between idles when random, between
  whole sequences when in sequence [documented].

### 5.2 Idle markers

- An idle marker is a spot that tells an actor how to behave there; its arrow gives the
  facing [documented: Idle Markers]. Patrol, Sandbox and Use Item At packages use them
  [documented]. Observed: a sandboxing settler alternated between wandering and using two
  idle markers (12–26 game minutes each) [observed: ai-observer.md §7].
- A marker has an idle collection and an optional **patrol data** part: how long to wait
  there before going on to the next linked marker or back to the package, a topic to say
  on arrival, and a result script run on arrival [documented: Idle Markers]. That result
  script is the REFR `patrol` unit after XPPA [observed: scripts.md §1.3]: agrees.
- Idle collections of packages and markers are read and played
  (docs/spec-notes/idles.md §5).

### 5.3 The idle manager

- The export mentions the idle manager only as what idle collections bypass. Our reading
  of the IDLE tree as the idle manager's decision tree (animation.md §2.2) remains a
  [hypothesis]; nothing in the export contradicts it. `fnv-actor` reads the IDLE records,
  their conditions and the tree; the furniture clips are chosen from it (furniture.md
  §3), nothing else is. Idles asked for by scripts, packages and dialogue lines are
  played (docs/spec-notes/idles.md).

## 6. From Category:Dialogue, beyond quests-dialogue.md

- **Random infos, conflicting statements on one page** [documented: Category:Dialogue]:
  the flag description says Random infos stack up until a non-Random info or a Random End;
  the worked example and the notes say non-Random infos between Random ones do not end the
  stack, only Random End does. quests-dialogue.md §4 follows the first statement. A
  capture must decide (§8 item 9).
- The examples also say: a set ended by Random End is used whenever any of its infos
  qualifies, and later Random infos are reached only when none of that set qualifies
  [documented].
- Each response can name an idle for the speaker and one for the listener [documented].
- Topic links added under "Add Topics" are not merged between plugins at run time
  [documented]. Load-order rule for Layer 6.
- A dialogue result script that declares variables overwrites quest-script variables
  (script-language.md §2.5).
- Text in curly braces in a response is a note, not shown in subtitles [documented].

## 7. Cross-check with the runtime and the observations

### 7.1 Agreements

Package Begin/End/Change units and the OnPackageEnd spelling; patrol-marker result
scripts; FACT flag names and ranks; neutral as the default faction reaction; unloaded
actors standing still; the condition grouping of package conditions (quests-dialogue.md
§3).

### 7.2 Documented rules the runtime contradicts or lacks

| # | Documented | Runtime | Where |
|---|---|---|---|
| 1 | An actor holds one script package; a second AddScriptPackage replaces the first (catalogue notes; [observed: `od_08_script_packages`]) | agrees since 2026-10-10: the second replaces the first, RemoveScriptPackage returns to the list (test `a_second_script_package_replaces_the_first`) | `functions/actor.rs` add/remove_script_package |
| 2 | EvaluatePackage drops even a must-complete package; a script package without must-complete or must-reach-location is dropped at the next re-evaluation (catalogue notes) | a script package wins every evaluation until RemoveScriptPackage; the capture agrees with the runtime for EvaluatePackage on a sandbox script package [observed: `od_08_script_packages`] | `Game::select_package` |
| 3 | A package is chosen only while its schedule is valid (Category:Packages) | done 2026-10-10: schedules applied (`fnv_game::ai`, packages.md §3–§4) | scripts.md §7.5 |
| 4 | Package conditions may Run on Target, the package's target (Category:Packages) | done 2026-10-10: the package's reference or linked-reference target is the conditions' target | `Game::package_target` |
| 5 | Once Per Day, Must Complete, Continue if PC Near and the other flags | done 2026-10-10 for these three (`fnv_game::ai`); the other flags are read, not used yet | Layer 6 |
| 6 | Begin result script, then topic and idle; End idle only with a done state | the runner's `package` input runs the result script and the OnPackage* blocks; order between them not documented | runner |
| 7 | Detection formula, states, events | the formula and its bands done 2026-10-11 (`fnv_game::detection`, measured in the engine; GetDetected and GetDetectionLevel read it); the states, alert and target lists, events (CreateDetectionEvent, impacts), barks, IsInCombat done 2026-10-11 (`fnv_game::awareness`, timers are engineering choices); GetLineOfSight, SetAllVisible and explosions as events not yet | docs/design/detection.md, docs/design/alert.md |
| 8 | Faction disposition modifier; Track Crime; Evil (Karma); Allow Sell | not used (GetDisposition stub; crime only as flags) | Layer 6 |
| 9 | Idle collections, idle markers, package idles | not read | Layer 6 |

No observation contradicts the export. Two observations go beyond it: run-time packages
(GET_UP, DIALOGUE copies, COMBAT) and the timing of schedule changes (ai-observer.md §7).

## 8. Open questions to capture

Candidates for the capture plugin and the AI observer (ai-observer.md §8 lists the
broader questions).

1. **List order vs schedule**: an actor whose earlier package is out of schedule and
   whose later one is in (EasyPete's chair and sleep packages): which runs right after
   `EvaluatePackage`, and does list order alone decide among valid packages?
2. **Re-evaluation interval**: how long after a schedule boundary loaded and unloaded
   actors switch, compared with the PSDT hours (observed 0–13 game minutes).
3. **Two script packages**: AddScriptPackage A, then B, then RemoveScriptPackage; which
   package is current (`is_current_package`)? (§7.2 row 1.) Settled: the list package
   [observed: `od_08_script_packages`].
4. **EvaluatePackage after AddScriptPackage** on a package without must-complete: does
   the script package survive? (§7.2 row 2.) Settled: it survives [observed:
   `od_08_script_packages`].
5. **Order of package start effects**: the begin result script against the actor's
   OnPackageStart block (a package whose result script and the actor's block both write
   variables).
6. **Once Per Day**: when the 24-hour window starts and what the actor runs instead.
7. **Detection thresholds**: GetDetected for the player standing, walking, running and
   sneaking at set distances, indoors and outdoors, lit and dark; the Lost → Normal timer
   after the player breaks line of sight.
8. **Game settings**: read the fSneak*, iSneak* and fDetection* values the game uses
   (`GetNumericGameSetting`) against the defaults in §2.1.
9. **Random dialogue stacks**: whether a non-Random info between Random infos ends the
   stack (§6). Needs dialogue driving (Layer 6).
10. **Faction relation of multi-faction actors**: GetFactionRelation between actors whose
    factions have mixed reactions (also scripts.md §10).
11. **Idle collections**: idle timer and pick order on one idle marker (Random against
    Run in Sequence), with the observer's idle readings.
