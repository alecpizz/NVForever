# Script stubs: the audit of 2026-10-12

The owner's list 2 asked to "go through the placeholder stubs on scripting functions
against everything we have added up till this point and check/add any core engine
functionality missing" (workstream K, docs/handoff/archive/playtest-2026-10-12.md §3). This note
is that audit: every stubbed function, how often the shipped scripts use it, the system it
needs, its status, and what this round did.

## 1. What a stub is

`crates/fnv-game/src/functions/mod.rs` registers every script function by opcode:

- `Kind::Impl`: implemented;
- `Kind::Stub(system)`: recorded in the trace (`stub` events, `Game::stubbed`), no effect,
  returns 0;
- `Kind::NoOp(why)` (new): deliberately nothing to do on this engine; returns 0, not traced;
- not registered: traced once per unit as `unhandled`, returns 0.

`functions::stubs` (new) gives each stub its status: **assigned** to another workstream
(`ASSIGNED`), **implementable** (its system exists; left for later, `IMPLEMENTABLE` says
what is missing), else **needs a system** not built yet (the `Stub` text names it).

| | before | after |
|---|---|---|
| `Kind::Stub` | 123 | 94 (23 assigned, 10 implementable, 61 need a system); 88 (17 assigned) after karma and crime (§3.1); 81 (17 assigned, 1 implementable, 63 need a system) after the second round (§3.2); 80 (16 assigned) after the followers (§3.3); 77 (16 assigned, 1 implementable, 60 need a system) after Autosave, ShowRecipeMenu and SwapTexture (§3.4); 76 (16 assigned, 1 implementable, 59 need a system) after NPC sneaking (§3.5); 71 (16 assigned, 1 implementable, 54 need a system) after effect shaders (§3.6); 60 (1 assigned, 1 implementable, 58 need a system) after the combat round (§3.7); 59 (1 assigned, 1 implementable, 57 need a system) after UseWeapon (§3.8); 58 (1 assigned, 1 implementable, 56 need a system) after the function audit (§3.9); 52 (1 assigned, 1 implementable, 50 need a system) after roulette and the function probes (§3.11); 48 (1 assigned, 1 implementable, 46 need a system) after the used items (§3.12); 47 (1 implementable, 46 need a system) after GetOwnerLastTarget (unconscious actors, 2026-10-17); 46 (1 implementable, 45 need a system) after ShowRepairMenu (repair services, 2026-10-17); 44 (1 implementable, 43 need a system) after GetVATSValue and GetVATSMode (VATS mode, 2026-10-18); 43 (1 implementable, 42 need a system) after ShowBarberMenu (the barber, #414); 41 (1 implementable, 40 need a system) after ShowPlasticSurgeonMenu and PushActorAway (knockdowns, 2026-10-18); 38 (1 implementable, 37 need a system) after SystemSave, ForceSave and PlaceAtReticle (2026-10-18); 37 (1 implementable, 36 need a system) after IsPlayerActionActive (§3.13); 33 (33 need a system) after the menu hooks (§3.15); 30 (30 need a system) after EnableNavMesh, DisableNavmesh and setAllReachable (§3.16); 23 (23 need a system) after the actor changes (§3.17); 18 (18 need a system) after the films and screen splatter (§3.18); 16 (16 need a system) after Caravan (docs/spec-notes/caravan.md); 11 (11 need a system) after IsSwimming, PickIdle, SetAllVisible, SetNoAvoidance and EnableLoadingMenu (§3.19); 10 after dismemberment (IsLimbGone); 9 (9 need a system) after the ash piles (AttachAshPile, docs/spec-notes/ash-piles.md); 8 (8 need a system) after the grab (IsPlayerGrabbedRef, docs/spec-notes/grab.md); 7 (7 need a system) after the slot machines (§3.20); 6 (6 need a system) after environment radiation (GetRadiationLevel, docs/spec-notes/radiation.md); 5 (5 need a system) after pickpocketing (GetPlantedExplosive, docs/spec-notes/pickpocket.md); 1 (trapUpdate, needs a system) after SetVatsTarget, GetVATSTargetHeight, GetForceHitReaction and IsWaterObject (§3.21); 0 after trapUpdate (the swinging traps, spec-notes/traps.md §4) |
| deliberate no-ops | 4 (`misc::zero` / `one`: IsXBox, IsPS3, IsWin32, IsGoreDisabled) | those 4 + 4 `Kind::NoOp`; + GetVampire (#3615): 5 `Kind::NoOp` |
| uses in the shipped scripts that reach a stub | 4,185 (calls + condition uses) | 2,207 (1,552 calls, 655 condition uses); 2,075 (1,452 calls, 623 condition uses) after karma and crime; 2,032 (1,418 calls, 614 condition uses) after the second round; 2,006 (1,392 calls, 614 condition uses) after the followers; 1,534 (920 calls, 614 condition uses) after §3.4; 1,531 (917 calls, 614 condition uses) after NPC sneaking (§3.5); 1,071 (457 calls, 614 condition uses) after effect shaders (§3.6); 795 (313 calls, 482 condition uses) after the combat round (§3.7); 782 (300 calls, 482 condition uses) after UseWeapon (§3.8); 737 (256 calls, 481 condition uses) after the function audit (§3.9); 645 (242 calls, 403 condition uses) after roulette and the function probes (§3.11); 619 (242 calls, 377 condition uses) after the used items (§3.12); 614 (237 calls, 377 condition uses) after GetOwnerLastTarget; 599 (222 calls, 377 condition uses) after ShowRepairMenu; 345 (220 calls, 125 condition uses) after VATS mode; 341 (216 calls, 125 condition uses) after ShowBarberMenu; 304 (179 calls, 125 condition uses) after ShowPlasticSurgeonMenu and PushActorAway (35 calls); 294 (169 calls, 125 condition uses) after SystemSave, ForceSave and PlaceAtReticle (10 calls); 292 (167 calls, 125 condition uses) after IsPlayerActionActive (§3.13); 283 (158 calls, 125 condition uses) after the menu hooks (§3.15); 162 (136 calls, 26 condition uses) after the actor changes (§3.17); 145 (119 calls, 26 condition uses) after the films and screen splatter (§3.18); 112 (86 calls, 26 condition uses) after Caravan (docs/spec-notes/caravan.md); 93 (74 calls, 19 condition uses) after §3.19; 42 (23 calls, 19 condition uses) after dismemberment (IsLimbGone); 38 (19 calls, 19 condition uses) after the ash piles (AttachAshPile); 34 (16 calls, 18 condition uses) after the grab (IsPlayerGrabbedRef); 29 (11 calls, 18 condition uses) after the slot machines (§3.20); 22 (11 calls, 11 condition uses) after environment radiation (GetRadiationLevel); 21 (11 calls, 10 condition uses) after pickpocketing (GetPlantedExplosive); 1 (1 call: trapUpdate) after §3.21; 0 after the swinging traps |

22 stubs were implemented here (§3); 4 became `NoOp` (§5); 3 more were implemented by the
combat workstream while this audit ran (StopCombatAlarmOnActor, GetAttacked, GetFriendHit,
PR #235), merged before this branch's PR.

## 2. How it is counted

`fnvtool script stubs <Data> <the ten masters> --schema schemas/fnv` (fnv.script.stubs/1)
decodes every script unit of the load order (object, quest and effect scripts; quest stage,
dialogue, package and terminal result scripts: 14,907 units over the ten masters) and
counts each stub's calls; condition uses (CTDA) are the catalogue's
(`schemas/script/functions.json`, from xEdit's table and the masters' scan). It lists every
stub and no-op with its status and catalogue origin (`GECK1`, `VEGAS1`, … or an extender's,
with the stub count per origin in `summary.by_origin`), sorted by use, and the functions
used but not registered at all (§6). The smoke scenario `script-stubs` (tools/smoke/scenarios.toml) checks the counts
of §1, kept in tools/smoke/expected/script-stubs.toml (a branch that implements a stub
changes them there; `cargo test -p fnv-game` checks the stub and status counts against the
registry).

[observed] The decoded call counts can differ from the catalogue's `script_calls` (e.g.
IsLimbGone: 51 decoded calls against 12 in the catalogue): the catalogue counted compiled
SCPT records, the survey also decodes result scripts.

## 3. Implemented this round

Ordered by use (calls + condition uses over the ten masters). The behaviour notes and their
evidence tags are on each function (`functions/ai_state.rs`, `senses.rs`, `ui.rs`,
`crate::weather`); the engine hooks are the visible part.

| Function | Opcode | Calls | Conditions | Over | Engine hook |
|---|---|---|---|---|---|
| ResetAI | 0x11FA | 711 | 0 | packages (`crate::ai`): the script package dropped, goal and unreachable packages forgotten, a Look let go, packages chosen again; the AI is asked to take the package up afresh even when the same one is chosen (`RefExtra::ai_reset`, issue #2119) | the residents take the package up afresh: off its furniture by the exit clip, goal and walk gone, begun again (sitting down again when the package says so), a Dialogue package that had talked greets once more [design, actor-behaviour.md §5]; the original's seated RingoRef read GET_UP one loop after ResetAI, then TRAVEL [observed: owner-decision-captures.md od_06]; sleepers not established (od_06 (c)) |
| StopLook / Look | 0x105D / 0x105C | 242 / 159 | 0 | `Game::script_look` | `nvforever` `script_look.rs`: a `LookOverride` over every other look (the player, an actor, or a marker's position); with `rotateBody` 1 the whole body turns to the target until StopLook [documented: GECK wiki Look]: `fnv_ai` turns an actor it runs at 145°/s and keeps it on the target to within a degree (`social::LOOK_TURN_RATE`, `Game::body_looks`); a walker walks on along its route with its body turned to the target and does not sit down until StopLook; after StopLook nothing turns the body back [observed: RingoRef, AI observer campaign `knockdown_look` 2026-10-19: 0° to 89° at 140–145°/s from the first frame, the body within 1° of the player while he walked 120 units to his chair, seated 9 samples after StopLook]. Without the AI (no `--play`) `script_look.rs` turns a standing actor on the spot with a conversation's turn (`dialogue_look::BodyTurn`, again whenever the target is 30° off its front) [design] |
| IncrementScriptedChallenge | 0x1275 | 134 | 0 | `Game::challenge_step`: progress against the CHAL DATA threshold; a recurring one starts again from 0; completion runs the reward script and shows a corner message (challenges.md) | none |
| GetCurrentAIProcedure | 0x108F | 32 | 64 | package type, what the AI layer has the actor do (`fnv_game::ai::AiPhase`: on the way 0, a sandbox's doing), goal met (17 DONE for the package types that end), getting up 21, combat (13), search (36), conversation (4); the codes from the AI observer's samples of the original (§3.16, packages.md §4, procedure codes) | `fnv_ai` tells the phase each update |
| ForceActiveQuest | 0x11DD | 82 | 0 | `UiRequests::active_quest` | the Pip-Boy DATA page takes it (`pipboy data: active quest … (ForceActiveQuest)`) |
| ForceTerminalBack | 0x1223 | 80 | 0 | `Terminals::back`, handed on by `terminal::choose` | the terminal goes back one level or closes; counted in the terminal summary |
| IsSneaking | 0x111E | 35 | 33 | `Senses::motion` | `control::tell_scripts` (the player, every frame), `detection.rs` (everyone measured) |
| GetLineOfSight | 0x101B | 49 | 0 | the detection last measured for the pair (`Detection::seen`: line of sight within ±95°); the player on a body: `Senses::camera_sees` (#3232) | `detection.rs` (already; bodies from the camera) |
| IsMoving | 0x1019 | 0 | 49 | `Senses::anim` (the way told, §3.13), else `Senses::motion` (moving reads 1) | `anim_state.rs`, `detection.rs` |
| GetCurrentAIPackage | 0x106E | 16 | 15 | package type codes; 5 combat, 29 search, 6 conversation | none |
| AddSPECIALPoints | 0x1219 | 21 | 0 | `SpecialSheet::for_menu`: more points, the old values the floor; opens 1048 as its attributes page (levelup.md §8) | the attributes page opens with those rules (Intense Training) |
| AlwaysShowActorSubtitles | 0x1277 | 16 | 0 | `Senses::always_subtitles` | `script_hud.rs` shows the subtitle out of hearing too |
| SetWeather / ForceWeather | 0x1104 / 0x112D | 14 / 13 | 0 | `WeatherState::request` (a fade or at once, held or not) | `main.rs` `script_weather` → `SkyState::script_weather` |
| IsPC1stPerson | 0x1188 | 2 | 9 | `Senses::first_person` | `control::tell_scripts` |
| ReleaseWeatherOverride | 0x1158 | 11 | 0 | `WeatherState::release` | the climate chooses again |
| IsRaining | 0x103E | 6 | 3 | the sky's weather is Rainy (WTHR DATA) and fully faded in | `script_weather` tells the game |
| IsRunning | 0x111F | 0 | 4 | `Senses::motion` | `detection.rs` |
| AddTagSkills | 0x121B | 3 | 0 | `TagSheet::for_menu`: the count raised, the old tags locked; opens 1048 | the tag-skill menu ("SKILLS: n/4 Selected") |
| SetSPECIALPoints | 0x1218 | 2 | 0 | `SpecialSheet::for_menu`: the total; opens 1074 | the Vigor Tester's menu opens with those rules |
| GetIsCurrentWeather | 0x1095 | 2 | 0 | the weather shown, or one just asked for | `script_weather` |

In all, 1,807 uses moved from stubs to implementations.

The test aid `NVF_PLAY_TEST_SCRIPT` gained `call:<reference or ->:<function>[:<argument>…]`,
which calls any function by name as a script would, an extender's too; the
`script-stubs-play` scenario uses it for Look, StopLook, SetWeather and
ReleaseWeatherOverride, `tiles-trait-pointer-done` for ClickMenuButton.

### 3.1 Implemented by karma and crime (workstream J, claude/karma-crime)

Taken off the assigned list (docs/spec-notes/crime-karma.md §7): SendAssaultAlarm (0x105F,
93 calls; it was assigned to E2, the assault rules are provocation's and the alarm is a
crime), SendTrespassAlarm (0x115C, 6), GetTrespassWarningLevel (0x1090, 29 condition
uses), IsTrespassing (0x1091, 1), GetAlarmed (0x103D, 1 call), GetMajorCrimeCount
(0x1075, 2); and GetIsAlignment (0x11DA, 3 condition uses), no longer unregistered.
GetOwnerLastTarget (0x124E) is implemented since 2026-10-17 (the actor the weapon's holder
last hit, docs/spec-notes/unconscious.md §2.8). Also registered there: GetMinorCrimeCount, GetActorCrimePlayerEnemy,
IsActorAVictim, IsPCAMurderer, IsCellOwner, IsInMyOwnedCell, and RewardKarma with its
corner message.

### 3.2 Second round (claude/script-stubs-2, 2026-10-12)

The implementable stubs of §4 and the unregistered functions of §6. Evidence tags are on
each function; call sites are from `fnvtool script decompile` over the ten masters
[observed].

| Function | Opcode | Calls | Conditions | Over | Engine hook |
|---|---|---|---|---|---|
| SetPackDuration | 0x1122 | 12 | 0 | `RefExtra::package_ends`: the current package ends that many game minutes after the call (counts as done, end script, then the actor chooses again) [hypothesis: from the call; what it ends into] | none (`Game::end_timed_packages` in the periodic choice) |
| IsPlayerMovingIntoNewSpace | 0x1166 | 6 | 8 | `Game::loading` (the travel's loading screen) | `nvf_play::script` sets it already |
| SetGlobalTimeMultiplier | 0x1186 | 7 | 0 | `UiRequests::time_multiplier`; ≤ 0 refused | `nvf_play::script::time_multiplier`: `Time<Virtual>` relative speed; sound speed unchanged |
| Rotate | 0x1004 | 4 | 0 | one call turns the reference for one frame (speed × frame time), the frame's last call counts [hypothesis, from the wiki's note and the GameMode callers] | `moved.rs`, `script_draw.rs` redraw `Game::turned` |
| SetActorsAI | 0x118B | 3 | 0 | `RefExtra::ai_off`: no periodic package choice | `fnv_ai` does not steer or turn the actor; one that was walking walks on straight ahead through the character controller (`Ai::coast`, set-actors-ai.md) [observed: owner-decision-captures.md §7]; `detection.rs` skips it as a detector; animation and collision not stopped |
| ShowAllMapMarkers | 0x11F8 | 2 | 0 | `PlayerState::all_markers` (show, travel kept, skip hidden); `Game::map_marker_shown` | the Pip-Boy world map (FNAM bit 4 "Show All" Hidden) |
| IsTurning | 0x101A | 0 | 1 | `Motion::turning` | `detection.rs`: heading change over 0.5 rad/s (left 1, right 2) [hypothesis: threshold] |
| AddItemToLeveledList | 0x1236 | 20 | 0 | `fnv_sim::World::add_to_leveled_list`: entries added after the authored ones, in snapshots; health not kept | containers and AddItem through `leveled::resolve_with`; actors' drawn equipment (`fnv-actor`) does not see them |
| IsDLCInstalled | 0x126F | 8 | 0 | a load-order plugin whose name matches, spaces, case and extension ignored [hypothesis: "tribal pack" ↔ TribalPack.esm] | none |
| GetXPForNextLevel | 0x1215 | 3 | 0 | the next level's total, (n − 1)((n − 2)/2 · iXPBumpBase + iXPBase) [documented: GECK wiki Experience Settings], less the player's XP [hypothesis: remaining, from the Here and Now perk's stage] | levelling up by experience: experience.md |

Left, with the reason (§4): PushActorAway (since built, docs/spec-notes/unconscious.md
§5), SetForceSneak (since built,
§3.5), AutoDisplayObjectives
(objectives already show as scripts set them; what the switch adds is not documented
beyond the summary), GetIsObjectType (its condition parameter is a form type code, and
the code table is not in the local GECK wiki export; one condition use).

The test aid `call:` also takes `x`/`y`/`z` (an axis) and `"text"` (a string). Smoke:
`script-stubs-2-play` (the time multiplier and ShowAllMapMarkers on the Pip-Boy map).

### 3.3 The followers (H, 2026-10-12)

OpenTeammateContainer (0x11C8, 26 calls) is implemented (docs/design/followers.md §5): the
container menu on the companion's inventory, once the conversation is over. No stub is
assigned to H any more. Wait, StopWaiting and IsWaiting are not registered and used by no
script of the masters [observed]; the companions wait by their `Waiting` variable and Guard
packages.

### 3.4 Saving, crafting and rendering (2026-10-15 and 16)

Three stubs that needed a system are implemented, all without condition uses:
Autosave (0x115E, 74 calls; saving, 2026-10-15), ShowRecipeMenu (0x1250, 15 calls; the
crafting menus, claude/l5-crafting, PR #292) and SwapTexture (0x1269, 383 calls; the
texture swap on a reference's shape, claude/l5-render-fx, §4 notes). With the followers'
OpenTeammateContainer (§3.3, 26 calls, the last stub assigned to H) the audit's counts went
from 81 stubs and 1,418 calls to 77 and 920; the smoke scenario `script-stubs` checks these.

### 3.5 NPC sneaking (claude/l5-npc-ai, 2026-10-16)

SetForceSneak (0x10D3, 3 calls) is implemented: `RefExtra::force_sneak`, read by
`Game::sneaks` together with the current package's Always Sneak; the engine crouches the
actor in its `sneakmt…` locomotion clips, walks it at fMoveSneakMult and measures it as
sneaking (nvforever `sneaking.rs`; owner's playtest list 5 items 36–37). GetForceSneak has
no opcode in the catalogue and stays unregistered.

### 3.6 Effect shaders (claude/effect-shaders, 2026-10-16)

Five rendering stubs are implemented, none with condition uses (460 calls): 76 → 71 stubs,
917 → 457 calls. Facts and evidence: docs/spec-notes/effect-shaders.md.

| Function | Opcode | Calls | Over | Engine hook |
|---|---|---|---|---|
| PlayMagicShaderVisuals / StopMagicShaderVisuals | 0x1123 / 0x1125 | 205 / 79 | `Game::script_looks` (`functions::misc::ScriptLooks`): one play per reference and shader, with its duration | nvforever `effect_shaders.rs`: the EFSH membrane (fill and edge) as overlays on every mesh of the reference, timed by the record's cycle; also the shaders of active magic effects (MGEF "effect shader"). Particles, holes and add-on models not drawn |
| SetActorAlpha | 0x1129 | 127 | `ScriptLooks::alpha` | `effect_shaders.rs`: blended copies of the actor's materials |
| SetSecuritronExpression | 0x1248 | 37 | a texture swap of `Screen01:0` to `textures\creatures\securitron\<face>_<expression>.dds` | `texture_swap.rs`, which now also finds actors' part shapes by name (`nvf_assets::actor::ShapeName`) |
| Reset3DState | 0x1146 | 12 | `ObjectAnims::reset`: the group no longer counts as playing, requests dropped | `object_anims.rs`: the object's group stops where it is |

Smoke scenarios `effect-shaders` (a script's shader on Sunny, a timed one, SetActorAlpha) and
`securitron-expression` (a street Securitron by the Strip's north gate turns from its police face
to Victor's alert one).
Not saved: the shaders, alphas and expressions are lost on load [design, as SwapTexture].

### 3.7 Combat (2026-10-16)

The fifteen stubs assigned to E2 combat AI, over the merged combat AI (`crate::awareness`,
`crate::combat_group`, `crate::combat_style`, `crate::combat`); the functions are in
`functions/fighting.rs` with their evidence tags. Call sites are from `fnvtool script
decompile` over the six masters [observed].

| Function | Opcode | Calls | Conditions | Over | Engine hook |
|---|---|---|---|---|---|
| SetCombatStyle | 0x10B1 | 92 | 0 | `RefExtra::combat_style`, read first by `Game::combat_style` [documented: the reference only, not its base] | the combat AI reads `combat_style` already (melee only, attack chances, flee by personal survival) |
| GetGroupMemberCount | 0x11A0 | 0 | 66 | `Game::combat_group`; 0 outside a combat state | none |
| GetKnockedState | 0x106B | 5 | 35 | 1 while unconscious, else 0 [documented codes; fatigue −1 and knockdown 2 not built] | none |
| FireWeapon | 0x11E2 | 24 | 0 | `CombatRequest::Fire` | `nvf_play::combat`: an `ActorFire` from the reference's origin (an actor's raised 64) along its rotation, no spread [hypothesis: the angles' sense]; the model's projectile node not used |
| Exists | 0x119F | 0 | 21 | the condition's target (`Game::condition_target`) differs from the subject; in a combat state, another member in its combat group [hypothesis: our combat lines target the foe, the game's a group member] | none |
| GetShouldAttack | 0x1042 | 7 | 4 | `Game::would_attack` [hypothesis: no wiki page; the six decompiled uses are `GetShouldAttack player == 0`] | none |
| ForceFlee | 0x10F5 | 8 | 0 | `Game::start_flight` from the combat target or the first foe; a destination reference's place as the spot; a conversation with the actor ends; no threat, nothing | `fnv_ai` runs the flight |
| IsCombatTarget | 0x1203 | 6 | 1 | the target list while in a combat state | none |
| GetGroupTargetCount | 0x11A1 | 0 | 3 | the group's targets and alarm owners, once each | none |
| GetArmorRating | 0x1051 | 1 | 1 | equipped armour's DNAM DR (`fnv_sim::forms::Item::armor_dr`, stored × 100), rounded down | none |
| GetCauseofDeath | 0x118D | 1 | 1 | the killing hit's weapon animation type (`functions::cause_code`) [hypothesis: the type → code map]; −1 alive or dead without a hit | none |

Left, with the reason (§4): UseWeapon (13 calls: the combat AI fights only the hostiles it
chose; a scripted stand spot, shot count, always-hit and no-damage shots were not built;
implemented since, §3.8), IsLimbGone (implemented since with dismemberment, 2026-10-20), GetForceHitReaction and GetPlantedExplosive (no hit
reactions forced by power attacks, no live explosives planted on actors; no shipped
calls).

**Begin blocks** (docs/spec-notes/script-functions.md §3.3), fired since this round:

- **OnStartCombat** (0x19, 147 scripts) when an actor reaches Combat, its combat target
  the block's subject (86 of the blocks name the player); once a fight: Lost and back is
  the same fight, Alert alone runs nothing [hypothesis]. `Game::combat_blocks`, at the
  start of each tick.
- **OnCombatEnd** (0x0C, 36) when such an actor is back to Normal; not on death or
  disabling [hypothesis].
- **OnReset** (0x1E, 45) on every present scripted reference of a cell that resets
  (`crate::respawn`), before OnLoad [documented: GECK wiki Reference, a cell's reset
  "trigger[s] OnReset blocks in scripts attached to the reference"; hypothesis: the
  order].

Tests: `crates/fnv-game/tests/combat_blocks.rs`. Smoke: `script-stubs` (the counts of §1).
No in-game scenario: the blocks need a fight with a scripted actor, and the tests drive
the same `Game` path the engine calls each frame.

### 3.8 UseWeapon (claude/use-weapon, 2026-10-16)

UseWeapon (0x119C, 13 calls) is implemented with the Use Weapon package
(docs/design/use-weapon.md): the actor walks to where it is to stand, turns to its target
and fires the number of shots, with the always-hit, no-damage, crouch-to-reload and
hold-fire flags. 60 → 59 stubs, 313 → 300 calls. Melee, unarmed and creatures' own
attacks followed on 2026-10-20 (claude/q5-melee-useweapon, use-weapon.md §3.1): the actor
closes in and swings the number of blows.

### 3.9 The script function audit (2026-10-16)

The audit of every registered function (docs/design/script-function-audit.md) implemented
GetAnimAction (0x10DB, 44 calls, 1 condition use): −1 for none, 0 while an NPC the engine
draws takes out its weapon and 1 while it puts it away [observed in the original, captures
`fa_batch1_queries` and `fa_batch3_actions`]; attacks, reloads and the player's actions are
not told yet. 59 → 58 stubs, 300 → 256 calls, 482 → 481 condition uses.

### 3.10 Blackjack (2026-10-17)

ShowBlackJackMenuParams (0x1254, 6 calls) opens the blackjack menu at a casino's table
(docs/spec-notes/blackjack.md); the casino games' winnings now set GetCasinoWinningsLevel's
levels. 58 → 57 stubs, 256 → 250 calls.

### 3.11 Roulette and the function probes (2026-10-17)

ShowRouletteMenuParams (0x1256, 6 calls) opens the roulette table (claude/roulette); the
smoke expectations had not followed it. The function probes (tools/capture/function_probe.py,
docs/design/script-function-coverage.md) implemented four more from what the original game
answered: GetDisposition (0x104C, 15 condition uses) and SetDisposition (0x1184, 2 calls):
the record's Disposition Base plus `fDispTargetCharMult` × (the target's Charisma − 5),
SetDisposition's value instead; IsFacingUp (0x106A, 41) and IsLeftUp (0x111D, 22): 1 on a
standing actor [observed]. 57 → 52 stubs, 250 → 242 calls, 481 → 403 condition uses.

### 3.12 Used items (claude/package-errands, 2026-10-17)

With Find and Use Item At packages carried out (docs/spec-notes/packages.md §3.1), the four
"AI (used items)" functions are implemented: GetIsUsedItem (0x10F6, 16 condition uses),
GetIsUsedItemType (0x10F7, 6), GetIsUsedItemEquipType (0x11E0, 1) and GetUsedItemActivate
(0x1103, 3). What an actor uses is the furniture it is on, else what its Find or Use Item
At package last had it take or use [hypothesis]. 52 → 48 stubs, 403 → 377 condition uses.

### 3.13 Animation state (2026-10-18)

The engine tells the rules what each drawn actor's body and the player's do, every frame
(`fnv_game::anim_state::AnimState`, `Senses::anim`; written by `nvforever` `anim_state.rs`;
evidence in docs/spec-notes/anim-state.md): GetAnimAction (0x10DB) now answers attacks (2,
3, 4), reloads (9), blocks (7), a blocked blow's recoil (8) and special idles (13) by the
New Vegas table, checked against the AI observer's gunfight and melee runs; IsMoving
(0x1019, 49 condition uses) the way (2 backward, 3 left, 4 right); IsAnimPlaying (0x1128)
an actor's own movement group (`Idle` standing, observed); IsIdlePlaying (0x1070) an
actor on furniture (observed). IsPlayerActionActive (0x1170, 2 calls) is implemented for
action 9, iron sights, the only one the shipped scripts ask; the wiki export lacks the
ActionIndex list, so the other indices read 0. Not answered: GetAnimAction 5, 6 (throw
phases), 10–12, and 17 (seen on .357 revolvers before a reload, meaning unknown). 38 → 37
stubs, 169 → 167 calls (after the knockdowns and the script saves).

### 3.14 Miscellaneous statistics and SetScale (claude/blockers-miscstat-scale, 2026-10-18)

Not stubs, but half-built (outstanding.md §1 row 16): GetPCMiscStat now answers every New
Vegas statistic by code, 26 of them counted by the rules and the engine, and SetScale's
scale is drawn and collided ([misc-stats](../spec-notes/misc-stats.md)). Tests:
`functions.rs` `misc_stats_count_and_set_scale_is_kept_on_the_reference`; smoke
`misc-stats-scale`.

### 3.15 Menu hooks (claude/menu-hooks, 2026-10-18)

outstanding.md §1 rows 6, 7, 9 and 18; the menu codes in `fnv_game::game::menus`:

- **MenuMode**: `MenuMode 1` (call and `Begin MenuMode 1` block) answers for any of the
  Pip-Boy's menus, and the Pip-Boy reports its page's code (STATS 1003, ITEMS 1002, DATA
  1023, the repair list 1035, the weapon mod menu 1061) [documented: GECK wiki MenuMode,
  GetSelectedItemRef, GetMenuTargetRef]. A
  loading screen is 1007: `MenuMode 1007` holds and only the `Begin MenuMode 1007` blocks
  run under it, also in the two frames after a save is loaded [hypothesis, the DLC
  scripts' "from load" flags]. NVForever's VATS marking is 1056 beside GameMode
  [design]. The pause menu's 1013 is not reported (scripts do not run under it). The
  blackjack table moved from 1035 (the repair list's) to its own 1102.
  The companion wheel reports 1075 (2026-10-19, [hypothesis], docs/spec-notes/companion-wheel.md §3).
- **ShowTutorialMenu** (2 calls, CGTutorial's crippled-limb help from its
  `Begin MenuMode 1` block): the tutorial menu, MenuMode 1059 [observed: capture mh1_tutorial], over the menu
  that is open, which it gives back when closed (docs/spec-notes/tutorial-menu.md).
- **ResetPipboyManager** (4 calls: Doc Mitchell's Pip-Boy, the three DLC starts): the
  Pip-Boy's pages go back to their defaults and its light (NVForever's flashlight) goes
  off [documented: the look and the light; hypothesis: the pages]. NVForever's Pip-Boy
  needs no reset to be used (the wiki's "NPC Pip-Boy" state is not modelled: test runs
  open the inventory without a Pip-Boy).
- **AutoDisplayObjectives** (2 calls, VCG01 stages 55 and 200): kept with the player;
  while on, the HUD's objective list stays on screen until another quest line waits
  [hypothesis].
- **ExitGame** (1 call, the ending's fade-out): back to the main menu as the pause menu's
  Quit to the main menu; a run a test aid drives quits instead. The credits are not
  built.

Tests: `synthetic.rs` `coded_menu_mode_blocks_run_for_their_menus`,
`the_tutorial_menu_opens_over_the_pipboy_and_gives_it_back`,
`pipboy_reset_exit_and_objective_display_requests`; nvf-play `tutorial_menu` and
`quest_messages` tests. Smoke: `menu-hooks-tutorial`, `menu-hooks-pipboy-codes`,
`menu-hooks-exit-game`.

### 3.16 Navmesh switches and AI procedure codes (claude/nav-knockdown, 2026-10-18)

Playthrough blockers row 18 (docs/outstanding.md §1).

- **EnableNavMesh, DisableNavmesh** (`functions/navigation.rs`): the rules keep each
  navmesh a script switched (`Game::nav_switches`, saved); `fnv_ai::Ai::apply_navmesh_switches`
  brings the nav graph in line (`fnv_nav::NavGraph::apply_switches`: the switched navmeshes
  as the scripts left them, the others as the load order has them) whenever the switches
  change or a game is loaded, and every actor whose way crosses a navmesh disabled now
  takes up its package afresh, planning a way round or setting the package aside
  [design]. The engine applies them before the residents think (`residents.rs`, log
  `nav: navmeshes switched by scripts: …`). The shipped calls [observed: decompiled
  scripts]: Red Rock Canyon's bridge (`RCExBridge1SCRIPT`, SCPT 0x00072021) enables four
  navmeshes once its "forward" animation has played; the Lonesome Road elevator
  (LonesomeRoad.esm SCPT 0x04003217) disables its two navmeshes
  (`NVDLC04ElevNav01REF`/`02REF`, NAVM records) while it rides and enables one 75 s
  later. The wiki says these calls skip the load order's fix-up of FormIDs [documented:
  GECK wiki EnableNavMesh/DisableNavmesh notes]; here they are fixed up like any other
  reference [design, owner-review].
- **setAllReachable** (`functions/navigation.rs`): kept and saved. Its two calls are the
  Hoover Dam battle's optimisations beside SetAllVisible and SetNoAvoidance
  (`VHDBattleOptimizerScript`, SCPT 0x001418DB; `ComplexSceneScriptPS3SCRIPT`, whose quest
  stops unless on a PS3) [observed]; its meaning, places counted reachable without the
  test [hypothesis]. Ways are always planned in full here, so it changes nothing.
- **GetCurrentAIProcedure** for a Sandbox and a Travel package at its goal: from about
  430,000 actor samples of the AI observer captures (docs/spec-notes/packages.md §4, procedure codes)
  [observed]: a sandboxing actor reads what it does (7 walking to or at a spot, 46 at an
  idle marker, 48 seated, 10 eating, 8 asleep, 21 up from furniture until the next thing,
  0 between things or on the way to something), 45 only out of the high process; a Travel
  package reads 0 on the way and 17 at its goal, seated or in bed too. 17 is kept to the
  package types that end (Travel, Patrol, Dialogue, Use Item At, Flee Not Combat
  [observed]; Find, Escort, Ambush, Use Weapon [hypothesis]); a Follow package reads 11
  throughout (it used to read 17 once begun). `fnv_ai` tells the rules what each actor it
  runs does (`Game::set_ai_phase`). Tests: `package_choice.rs`
  `reset_ai_drops_the_script_package_and_the_codes_follow_the_package`,
  `navmesh_switches_are_kept_and_saved`; `fnv-ai` `carry.rs`
  `a_navmesh_disabled_under_a_walk_sends_the_actor_to_plan_again`,
  `a_sandboxer_tells_the_game_what_it_does`; `fnv-nav`
  `navmesh_switches_block_and_reopen_the_way`. Smoke: `navmesh-switches`,
  `ai-procedure-codes`.

### 3.17 Actor changes, perks' activate choices, talking activators (claude/actor-changes, 2026-10-18)

Playthrough blockers rows 8, 11, 13, 14 and 18 (docs/outstanding.md). Implemented:
AgeRace (0x11E4, 2 calls) and SexChange on any actor (it had refused NPCs;
[appearance-changes](../spec-notes/appearance-changes.md) §2), SetLevel keeping an actor's
items, as the original does (§3, capture ac1), SetPCCanUsePowerArmor gating power armour (§4); the perks' activate
choices with StartCannibal (0x107E, 4 calls), GetCannibal (0x10E3, 1 condition use),
HasBeenEaten (0x107F, 6 calls, 4 condition uses), StartMisterSandman (0x107C, 1 call) and
GetSandman (0x10E2, 2 condition uses) ([perk-activation](../spec-notes/perk-activation.md));
talking activators with IsActorTalkingThroughActivator (0x11AE, 92 condition uses) and the
unregistered IsTalkingActivatorActor (0x1172)
([talking-activators](../spec-notes/talking-activators.md)). 30 → 23 stubs (after main's
script saves, PlaceAtReticle, IsPlayerActionActive, the menu hooks and the navmesh
switches), 149 → 136 calls, 125 → 26 condition uses [observed: the branch's `fnvtool
script stubs`, merged with main 3ca600ff, on the ten masters]. Tests: `appearance.rs`, `perk_talk.rs`; smoke `agerace-sexchange-npc`,
`sexchange-player-reset`, `setlevel-npc-reroll`, `power-armor-training`,
`perk-cannibal-devour`, `perk-sandman-refused-awake`, `talking-activator-intercom`; capture probes
`tools/capture/probes/actor_changes.toml`.

### 3.18 Films, cell image spaces and screen splatter (claude/screen-video, 2026-10-18)

PlayBink plays a loose Bink film full screen with its sound over the held game, through
the intro's decoder ([bink-films](../spec-notes/bink-films.md)); SetCellImageSpace keeps
the cell's new image space in the save and grades the cell with it when it loads again
(bink-films.md §4); TriggerScreenSplatter, SetScreenSplatterFade and ClearScreenSplatter
put the game's gore textures over the screen with the `fBloodSplatter*` settings
([screen-splatter](../spec-notes/screen-splatter.md)). Every shipped call of the five is
the opening's but SetCellImageSpace's (Old World Blues' Think Tank). Tests: `functions.rs`
`films_splatter_and_cell_image_space`, `nvf_play` `intro` and `screen_splatter` unit
tests; smoke `film-opening`, `film-console`, `screen-splatter`, `cell-image-space`, and
an expectation more in `main-menu`.

### 3.19 Swimming, PickIdle, the AI's frame switches and EnableLoadingMenu (2026-10-19)

Five stubs over systems that already existed (16 → 11 stubs, 86 → 74 calls, 26 → 19
condition uses [observed: the calls decoded from the ten masters with `fnvtool script
decompile`, 2026-10-19]):

- **IsSwimming** (0x10B9, 7 condition uses; `functions/senses.rs`): 1 while the actor
  swims [documented: GECK wiki IsSwimming]. The engine tells it with the rest of an
  actor's movement every time detection measures (`Motion::swimming`, nvforever
  `detection.rs`): the swimming state of the actor's physics character (`fnv_phys`
  character `swimming`, physics.md §6.6), the player's and every drawn NPC's. On the
  player it always reads 0 [documented: GECK wiki IsSwimming notes, "will always
  return 0 if called on the player"; the extender's IsPlayerSwimming exists for that],
  so the player's state is told but not answered. An actor not measured (not drawn,
  far) reads 0 [engineering choice, as IsMoving]. The uses are idle conditions
  (`CowerIdles` wants IsSwimming 0, `crate::idles::Game::flee_idle`).
- **PickIdle** (0x1064, 2 calls; `crate::idles::pick_idle`): the actor "choose[s] and
  play[s] an Idle Animation", and nothing happens when none fits [documented: GECK wiki
  PickIdle]. The rules do not know an actor's animation folder, so the call queues the
  actor (`Game::idle_picks`); the engine's idle system (nvforever `idles.rs`) walks the
  tree for the drawn actor (`Game::pick_idle`: the actor's folder's roots in chain order,
  the first node whose conditions pass gone into, the first leaf with a file whose
  conditions pass; the fidgets' walk with no branch kept out [hypothesis]) and asks for
  the pick as `IdleSource::Picked`, which GetAnimAction counts as a special idle, 13
  [hypothesis: as PlayIdle's]. Log: `pick idle: <actor> picks <idle>` or `finds no
  idle`. Both calls are one object script's OnStartCombat (SCPT 0x0017491E) [observed].
- **SetAllVisible** (0x115A, 5 calls) and **SetNoAvoidance** (0x115B, 3 calls;
  `functions/navigation.rs` `FrameSwitches`): two of the original's four performance
  switches for crowded fights, "cleared every frame" [documented: GECK wiki
  SetAllVisible, SetNoAvoidance]. The rules clear them at each `Game::tick` before the
  scripts run; not saved. SetAllVisible: actors skip line-of-sight tests and see what
  they look at [documented]; the engine's detection takes an NPC's line of sight as
  clear while it is on (no rays), the field of view still applies, the player's camera
  sight keeps its rays [hypothesis]. SetNoAvoidance: actors skip "avoid box movement",
  "do not open doors or angle away from each other" [documented]; kept and read by
  nothing: there is no avoid-box steering here, and `fnv_ai::apart` stands for the
  character controllers' blocking [hypothesis]; NPCs still open doors [design]. Calls
  [observed: decompiled scripts]: `VHDBattleOptimizerScript` (quest, SCPT 0x001418DB,
  beside setAllReachable), an object script that starts a fight with the player (SCPT
  0x0015E8F9, beside SetSceneIsComplex), `ComplexSceneScriptPS3SCRIPT` (PS3 only) and
  two Lonesome Road object scripts (SetAllVisible alone).
- **EnableLoadingMenu** (0x1270, 2 calls; `functions/ui.rs`): "forces a loading screen
  to appear for a few seconds", one that does not use the location's own screens
  [documented: GECK wiki EnableLoadingMenu]. Both calls are `EnableLoadingMenu 1` in a
  door activator's OnActivate just before `player.MoveTo` a marker (SCPT 0x00168A8B,
  0x00141144) [observed]. The move brings its own loading screen here
  (`nvf_play::loading`), so the call makes that screen one for anywhere
  (`Loading::generic_next`; log `script: EnableLoadingMenu: the travel's loading
  screen is one for anywhere`). A call with no move after it shows nothing (not built,
  no shipped call does that; logged).

Tests: `functions.rs` `audited_stubs_answer_from_the_engine_state_and_leave_requests`
(IsSwimming), `script_stubs_round_3_requests` (PickIdle's queue, the frame switches
cleared by the tick, EnableLoadingMenu's request); `idles.rs` `pick_idle_walks_the_tree`;
nvf-play `loading` `enable_loading_menu_screen_is_for_anywhere`.

### 3.20 Slot machines (claude/q22-slot-machines, 2026-10-21)

ShowSlotMachineMenuParams (0x1252, 5 calls, in the slot machines' activator scripts of
The Tops, Gomorrah, the Atomic Wrangler and Vikki & Vance [observed]) asks for the slot
machine menu (`functions/misc.rs` → `Game::slots_requests`; the engine opens it,
nvf-play `slots`): the casino's chips carried and no ban, as roulette's [documented: GECK
wiki ShowSlotMachineMenuParams]. The reels come from the casino record's reel stops, the
payouts from the machine model's payout screen; MenuMode 1103 [hypothesis]. Everything
in docs/spec-notes/slots.md. Tests: `fnv_game::slots`, `casino` unit tests; smoke `slots`.

### 3.21 The last four with a system to answer from (claude/last-stubs, 2026-10-25)

Every use in the masters [observed: their CTDA function indexes and decoded scripts]:

- **SetVatsTarget** (0x11EB, 10 calls): the traps' OnActivate blocks (bear traps,
  tripwires, grenade bouquets: `setvatstarget 1`, one on its linked reference) make the
  trap a VATS target. Kept on the reference (`RefExtra::vats_target`, saved) over the
  base's DEST flag "VATS Targetable" [documented: GECK wiki SetVatsTarget]; VATS mode
  marks placed objects that `Game::vats_targetable` allows (vats-mode.md §20).
- **GetVATSTargetHeight** (0x10EB, 7 condition uses): the VATS melee idles
  `VATS1HMLow`, `VATS2HMLow`, `VATSH2HLow` … ask `< 30` to `< 50`. The marked point of
  the shot leaving now less the player's feet [documented: "the delta z from the player
  to the targeted limb"; hypothesis: from the feet], 0 otherwise. NVForever's VATS mode
  swings with combat's own attacks, so no idle reads it yet.
- **GetForceHitReaction** (0x1259, 2 condition uses: the idles
  `3rdPersonHitReactionsForce`, `3rdPCrouchedHitForce`): 1 while the actor plays a
  reaction a power attack forced (Khan Trick, `nvf_play::combat::ForcedHitReaction`),
  told with the body's state (`AnimState::forced_hit`) [documented: GECK wiki
  GetForceHitReaction; hypothesis: until the reaction's clip ends].
- **IsWaterObject** (0x1130, 1 condition use: the idle `ActWaterSource`): 1 if what the
  actor activated within 2 s is an activator with a Water Type (ACTI WNAM, the drinking
  sources) [documented: "the object currently being activated is Placeable Water";
  hypothesis: placeable water is that activator type; design: the 2 s].

Tests: `functions.rs` `forced_reactions_vats_targets_and_water_objects`. Left: trapUpdate
(docs/spec-notes/traps.md §4).

## 4. Every remaining stub

Statuses: **needs a system not built yet**; **assigned to workstream** (another agent is
building it now or next: E2 combat AI (none left since §3.7), H followers, J karma and crime; the aid workstream's
functions, AddSpell, RemoveSpell, CastImmediateOnSelf and Dispel, were implementations
before this audit and are not stubs); **implementable, left for later** (the system
exists; the note says what is missing); **deliberately no-op**.

| Function | Opcode | Calls | Conditions | Status | System / note |
|---|---|---|---|---|---|
| SwapTexture | 0x1269 | 383 | 0 | implemented 2026-10-16 (claude/l5-render-fx) | rendering |
| GetVATSValue | 0x1198 | 0 | 250 | implemented 2026-10-18 for VATS mode (docs/design/vats-mode.md §12) | combat (VATS) |
| PlayMagicShaderVisuals | 0x1123 | 205 | 0 | implemented 2026-10-16 (claude/effect-shaders, §3.6) | rendering (effect shaders) |
| SetActorAlpha | 0x1129 | 127 | 0 | implemented 2026-10-16 (§3.6) | rendering |
| SetCombatStyle | 0x10B1 | 92 | 0 | implemented 2026-10-16 (§3.7) | `RefExtra::combat_style` over the record's |
| IsActorTalkingThroughActivator | 0x11AE | 0 | 92 | implemented 2026-10-18 (§3.17) | dialogue: docs/spec-notes/talking-activators.md |
| StopMagicShaderVisuals | 0x1125 | 79 | 0 | implemented 2026-10-16 (§3.6) | rendering (effect shaders) |
| Autosave | 0x115E | 74 | 0 | implemented 2026-10-15 | saving: asks the engine for an autosave (`nvf_play::save`) |
| GetGroupMemberCount | 0x11A0 | 0 | 66 | implemented 2026-10-16 (§3.7) | `Game::combat_group` |
| IsLimbGone | 0x118E | 51 | 0 | implemented 2026-10-20 | dismemberment (docs/spec-notes/dismemberment.md §5) |
| GetAnimAction | 0x10DB | 44 | 1 | implemented 2026-10-16 (§3.9), attacks and reloads 2026-10-18 (§3.13) | the engine's snapshot: drawing, holstering, shots, blows, reloads, blocks, special idles |
| IsFacingUp | 0x106A | 0 | 41 | implemented 2026-10-17 (§3.11) | 1 on a standing actor [observed]; knockdowns not built |
| GetKnockedState | 0x106B | 5 | 35 | implemented 2026-10-16 (§3.7) | 1 unconscious, else 0 (no fatigue or live knockdown) |
| SetSecuritronExpression | 0x1248 | 37 | 0 | implemented 2026-10-16 (§3.6) | rendering (a texture swap) |
| PushActorAway | 0x118A | 35 | 0 | built 2026-10-18 | knockdowns: docs/spec-notes/unconscious.md §5 |
| PurgeCellBuffers | 0x1189 | 32 | 0 | deliberately no-op | memory: the original's cell buffers; cells here load and unload by distance |
| ShowCaravanMenu | 0x125A | 29 | 0 | implemented 2026-10-19 (claude/caravan) | menus (Caravan): docs/spec-notes/caravan.md |
| FireWeapon | 0x11E2 | 24 | 0 | implemented 2026-10-16 (§3.7) | `CombatRequest::Fire`: the engine fires it as an actor's shot |
| IsLeftUp | 0x111D | 0 | 22 | implemented 2026-10-17 (§3.11) | as IsFacingUp |
| Exists | 0x119F | 0 | 21 | implemented 2026-10-16 (§3.7) | the condition's target; in combat, another group member |
| SetRumble | 0x11FB | 17 | 0 | deliberately no-op | controller rumble: no controller rumble on the PC version |
| GetDisposition | 0x104C | 0 | 15 | implemented 2026-10-17 (§3.11) | Disposition Base and the target's Charisma [observed] |
| ShowRepairMenu | 0x117A | 15 | 0 | implemented 2026-10-17 (claude/repair-services-menu) | menus (repair services): docs/spec-notes/repair.md |
| ShowRecipeMenu | 0x1250 | 15 | 0 | implemented 2026-10-16 (claude/l5-crafting) | menus (crafting) |
| UseWeapon | 0x119C | 13 | 0 | implemented 2026-10-16 (claude/use-weapon) | combat AI: docs/design/use-weapon.md |
| Reset3DState | 0x1146 | 12 | 0 | implemented 2026-10-16 (§3.6) | animation state |
| GetShouldAttack | 0x1042 | 7 | 4 | implemented 2026-10-16 (§3.7) | `Game::would_attack` |
| HasBeenEaten | 0x107F | 6 | 4 | implemented 2026-10-18 (§3.17) | perks: docs/spec-notes/perk-activation.md |
| SetVatsTarget | 0x11EB | 10 | 0 | implemented 2026-10-25 (§3.21) | VATS mode marks the objects it allows (vats-mode.md §20) |
| ForceFlee | 0x10F5 | 8 | 0 | implemented 2026-10-16 (§3.7) | `Game::start_flight` from the combat target |
| TriggerScreenSplatter | 0x1240 | 8 | 0 | implemented 2026-10-18 (§3.18) | screen splatter: docs/spec-notes/screen-splatter.md |
| IsSwimming | 0x10B9 | 0 | 7 | implemented 2026-10-19 (§3.19) | the physics character's swimming, told by the engine; 0 on the player (wiki) |
| GetVATSTargetHeight | 0x10EB | 0 | 7 | implemented 2026-10-25 (§3.21) | the shot leaving now: its point over the player's feet |
| GetRadiationLevel | 0x11F7 | 0 | 7 | implemented 2026-10-21 (claude/q26-env-radiation) | radiation: the player's level from markers and water, 0 on others (docs/spec-notes/radiation.md §4) |
| IsCombatTarget | 0x1203 | 6 | 1 | implemented 2026-10-16 (§3.7) | the target list in a combat state |
| ShowBlackJackMenuParams | 0x1254 | 6 | 0 | implemented 2026-10-17 (claude/blackjack) | menus (casino) |
| ShowRouletteMenuParams | 0x1256 | 6 | 0 | implemented 2026-10-17 (claude/roulette, §3.11) | menus (casino) |
| SetAllVisible | 0x115A | 5 | 0 | implemented 2026-10-19 (§3.19) | detection: NPCs' line of sight taken as clear for the frame |
| EnableNavMesh | 0x11C0 | 5 | 0 | implemented 2026-10-18 (§3.16) | navigation (the nav graph's navmesh switches) |
| ShowSlotMachineMenuParams | 0x1252 | 5 | 0 | implemented 2026-10-21 (claude/q22-slot-machines) | menus (casino): docs/spec-notes/slots.md |
| SystemSave | 0x127B | 5 | 0 | implemented 2026-10-18 (spec-notes/script-saves-and-reticle.md) | saving |
| StartCannibal | 0x107E | 4 | 0 | implemented 2026-10-18 (§3.17) | perks: docs/spec-notes/perk-activation.md |
| GetVATSMode | 0x10E0 | 2 | 2 | implemented 2026-10-18 for VATS mode (docs/design/vats-mode.md §12) | combat (VATS) |
| PlayBink | 0x1114 | 4 | 0 | implemented 2026-10-18 (§3.18) | films: docs/spec-notes/bink-films.md |
| IsPlayerGrabbedRef | 0x11D0 | 3 | 1 | implemented 2026-10-21 (claude/q24-grab) | physics (grab): docs/spec-notes/grab.md §4; the grab also runs OnGrab / OnRelease |
| ShowBarberMenu | 0x11F1 | 4 | 0 | implemented 2026-10-18 (the barber, #414) | menus (appearance) |
| ResetPipboyManager | 0x1201 | 4 | 0 | implemented 2026-10-18 (claude/menu-hooks, §3.15) | Pip-Boy: default page and tabs, the light off |
| AttachAshPile | 0x1211 | 4 | 0 | implemented 2026-10-21 (claude/q20-ash-piles) | death effects: docs/spec-notes/ash-piles.md |
| AddCardToPlayer | 0x125B | 4 | 0 | implemented 2026-10-19 (claude/caravan) | Caravan: docs/spec-notes/caravan.md §2 |
| SetNoAvoidance | 0x115B | 3 | 0 | implemented 2026-10-19 (§3.19): kept for the frame, nothing to skip here | AI |
| GetGroupTargetCount | 0x11A1 | 0 | 3 | implemented 2026-10-16 (§3.7) | the group's foes |
| PlaceAtReticle | 0x1263 | 3 | 0 | implemented 2026-10-18 (spec-notes/script-saves-and-reticle.md §4) | HUD (reticle) |
| GetArmorRating | 0x1051 | 1 | 1 | implemented 2026-10-16 (§3.7) | equipped ARMO DNAM DR |
| PickIdle | 0x1064 | 2 | 0 | implemented 2026-10-19 (§3.19) | animation: the idle tree's pick by conditions |
| GetSandman | 0x10E2 | 0 | 2 | implemented 2026-10-18 (§3.17) | perks: docs/spec-notes/perk-activation.md |
| setAllReachable | 0x1159 | 2 | 0 | implemented 2026-10-18 (§3.16): kept and saved, nothing to skip here | navigation |
| SetSceneIsComplex | 0x115D | 2 | 0 | deliberately no-op | performance switch of the original's AI |
| IsPlayerActionActive | 0x1170 | 2 | 0 | implemented 2026-10-18 (§3.13) | 9 (iron sights) only; the other indices are not known |
| SetDisposition | 0x1184 | 2 | 0 | implemented 2026-10-17 (§3.11) | GetDisposition reads its value [observed] |
| GetCauseofDeath | 0x118D | 1 | 1 | implemented 2026-10-16 (§3.7) | the killing hit's weapon type |
| SetCellImageSpace | 0x1196 | 2 | 0 | implemented 2026-10-18 (§3.18) | image space: bink-films.md §4 |
| DisableNavmesh | 0x11BF | 2 | 0 | implemented 2026-10-18 (§3.16) | navigation (the nav graph's navmesh switches) |
| ShowTutorialMenu | 0x11E3 | 2 | 0 | implemented 2026-10-18 (claude/menu-hooks, §3.15) | menus (tutorial): docs/spec-notes/tutorial-menu.md |
| AgeRace | 0x11E4 | 2 | 0 | implemented 2026-10-18 (§3.17) | appearance: docs/spec-notes/appearance-changes.md |
| ShowPlasticSurgeonMenu | 0x11F2 | 2 | 0 | implemented 2026-10-18 (claude/plastic-surgeon) | menus (appearance): the character creator as the surgeon |
| ForceSave | 0x1217 | 2 | 0 | implemented 2026-10-18 (spec-notes/script-saves-and-reticle.md) | saving |
| AutoDisplayObjectives | 0x1225 | 2 | 0 | implemented 2026-10-18 (claude/menu-hooks, §3.15) | the HUD keeps the objective list on screen while it is on |
| SetScreenSplatterFade | 0x1243 | 2 | 0 | implemented 2026-10-18 (§3.18) | screen splatter |
| GetForceHitReaction | 0x1259 | 0 | 2 | implemented 2026-10-25 (§3.21) | the engine's body state: a forced hit reaction playing |
| EnableLoadingMenu | 0x1270 | 2 | 0 | implemented 2026-10-19 (§3.19) | the following travel's loading screen is one for anywhere |
| StartMisterSandman | 0x107C | 1 | 0 | implemented 2026-10-18 (§3.17) | perks: docs/spec-notes/perk-activation.md |
| GetCannibal | 0x10E3 | 0 | 1 | implemented 2026-10-18 (§3.17) | perks: docs/spec-notes/perk-activation.md |
| trapUpdate | 0x110C | 1 | 0 | implemented 2026-10-25 (claude/swinging-traps) | physics: the swinging traps' jointed bodies let go, their hits (spec-notes/traps.md §4) |
| IsWaterObject | 0x1130 | 0 | 1 | implemented 2026-10-25 (§3.21) | what the actor activated: an activator with a Water Type |
| PreloadMagicEffect | 0x1163 | 1 | 0 | deliberately no-op | loading: effect particles load when drawn |
| GetPlantedExplosive | 0x11AC | 0 | 1 | implemented 2026-10-24 (docs/spec-notes/pickpocket.md) | live explosives planted by reverse pickpocketing |
| ExitGame | 0x11B5 | 1 | 0 | implemented 2026-10-18 (claude/menu-hooks, §3.15) | back to the main menu; the credits are not built |
| ClearScreenSplatter | 0x1241 | 1 | 0 | implemented 2026-10-18 (§3.18) | screen splatter |

Notes on the most used:

- **SwapTexture** (383 calls) [observed: decoded scripts]: `SwapTexture <ref> "##<node>:<n>"
  "<texture>"`, the reference a parameter and no calling reference (all 242 calls in
  FalloutNV.esm; first implemented as `ref.SwapTexture "shape" "texture"`, so every call
  failed on its first parameter until 2026-10-16, claude/rorschach-cards), mostly the end slides (`interface\endgame\endscrn_*` on `##gs01`), the
  Rorschach cards of Doc Mitchell's test (`pipboycard01:0`) and the Euclid's C-Finder's
  screen. No GECK wiki page. Implemented 2026-10-16: the game keeps the last texture per
  shape of each reference (`fnv_game::functions::misc::swap_texture`), the engine puts
  `textures\<texture>.dds` on the drawn shape of that name (nvforever `texture_swap.rs`);
  `##` is dropped [hypothesis], on both sides: the model keeps it on its shape names, so the end slides' `##gs01:0` matches gs01.nif's `##GS01:0` only with it dropped there too [observed: fnvtool nif decode; until 2026-11-12 it was dropped from the call only, and the model's own first slide stayed up, issue #2226]. Not part of saves.
- **GetVATSValue** (250 conditions): implemented 2026-10-18 for VATS mode, NVForever's own
  mark and fire (docs/design/vats-mode.md §12): the marked shot leaving now, 0 otherwise.
  GetVATSMode with it (0 off, 1 marking, 4 firing).
- **PlayMagicShaderVisuals / StopMagicShaderVisuals** (284 calls) and **SetActorAlpha**
  (127): implemented 2026-10-16, §3.6.
- ~~**IsActorTalkingThroughActivator**~~ (92 conditions): implemented 2026-10-18 (§3.17):
  conversations through talking activators (docs/spec-notes/talking-activators.md).
- **SystemSave / ForceSave** (7): implemented 2026-10-18
  (docs/spec-notes/script-saves-and-reticle.md): the engine writes the system slot and a
  new save by hand, as for Autosave.
- ~~**PushActorAway** (35)~~: built 2026-10-18 (docs/spec-notes/unconscious.md §5): the
  pushed actor falls as a ragdoll thrown from the source and gets up again.

## 5. Deliberately no-op

| Function | Why |
|---|---|
| SetRumble | controller rumble: the PC version has none |
| PurgeCellBuffers | the original's cell buffers; cells here load and unload by distance |
| PreloadMagicEffect | effect particles load when drawn |
| SetSceneIsComplex | a performance switch of the original's AI |
| GetVampire | a leftover from Oblivion that never returns anything but 0 [documented: GECK wiki GetVampire] (#3615) |
| IsXBox, IsPS3 (0), IsWin32 (1), IsGoreDisabled (0) | already `Impl`: the PC version, gore on |

## 6. Used but not registered

| Function | Opcode | Calls | Conditions | Note |
|---|---|---|---|---|
None since #3615 (2026-11-23), which registered GetIsObjectType (0x11B1; one condition
use in the masters) by the form type codes of `fnv_script::vm::form_type_code`, with
seven more base-game functions players' games call: SameFaction, SameSex,
HasFriendDisposition, GetFatiguePercentage, GetTimeDead, GetLastHitCritical (all
`Impl`) and GetVampire (`NoOp`). SameSexAsPC (0x1087, condition index 135) followed in #3787,
seen in one player's modded game (25 calls).

Condition indices with no page in the catalogue were not counted here until #3614
(2026-11-23): 123 and 258 reached every player (as `0x107B` and `0x1102` in the player
reports) through three base-game IDLE records. xEdit's wbCTDAFunctions names them
IsGreetingPlayer and GetUsedItemLevel (no parameters); `Game::condition_value` answers
them (`cond::IS_GREETING_PLAYER`, `cond::GET_USED_ITEM_LEVEL`), and `fnvtool script
stubs` now scans every CTDA of the load order and lists, under `unhandled` with
`"counted_from": "CTDA"`, any index neither the registry nor `cond::answered_without_opcode`
knows. Over the ten masters it finds none [observed: 2026-11-23].

AddItemToLeveledList, IsDLCInstalled and GetXPForNextLevel were registered in §3.2.
GetOffersServicesNow (0x10FF; no use in the masters, for mods' vendor conditions) was
registered 2026-10-21 ([vendor-services](../spec-notes/vendor-services.md) §3).

## 7. Checks

- `cargo test -p fnv-game --release`: `functions.rs`
  `audited_stubs_answer_from_the_engine_state_and_leave_requests`, `package_choice.rs`
  `reset_ai_drops_the_script_package_and_the_codes_follow_the_package`, the `chargen.rs`
  unit tests for added points and tags, and the `functions::stubs` tests (the tables name
  stubs only; every stub has a status); for §3.2 `functions.rs` `second_round_stubs`,
  `package_choice.rs` `set_pack_duration_ends_the_package_and_set_actors_ai_stops_the_choice`
  and `fnv-sim` `synthetic.rs` `entries_added_to_a_leveled_list_count_and_persist`.
- `python tools/smoke run`: `script-stubs` (the survey's counts) and `script-stubs-play`
  (Look and the weather in Goodsprings), `script-stubs-2-play` (§3.2).
