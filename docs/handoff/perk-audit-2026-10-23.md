# Perk audit (2026-10-23)

Status 2026-10-25: every gap this audit found is built (perk-entry-points.md §7-§14); the
appendix rows say so.

Every PERK record of the ten masters checked against what NVForever evaluates. Asked by the
owner: "check for gaps in our code for handling all the perks … confirm and we'll build the
functionality". This note is the confirmation; §4 is the proposed build order.

## 1. Sources and method

In order of trust (owner's rule: the fandom wiki only where nothing better says otherwise):

1. **The game's records** [observed]: every PERK of FalloutNV.esm, the four story add-ons,
   Gun Runners' Arsenal and the four packs, read with a private script: each effect's kind
   (entry point, ability, quest stage), entry point code, function, value and condition
   tabs; each ability's magic effects and archetypes; every script (SCRO) and condition
   (HasPerk) that names the perk; the GMSTs of the VATS helpers. 257 PERK records: 90
   playable perks and 10 traits in FalloutNV.esm, 32 playable perks and 6 traits in the
   add-ons, and 119 others (challenge rewards, companion perks, implants, add-on rewards).
2. **The GECK wiki** [documented]: GetNthPerkEntryFunction, "Entry Function Codes" (the 74
   entry point codes and names); Entry Point (what each one changes and when it runs).
3. **The fandom wiki** (Fallout: New Vegas perks page, saved in
   `C:\fnv-private\geckwiki`) for player-facing descriptions only. Every perk it lists is a
   PERK record above, except the companions' upgrades Faithful Protector, Blood of the Legion,
   Unshakeable Tracker (Rex) and Ravenous Hunger (Dog), which are not PERK records
   [observed: no PERK has those names]. Nothing in it contradicted the records.

What NVForever evaluates [observed: the code at origin/main 2fafe678]: entry points 0, 1, 2,
6, 9, 10, 11, 21, 23, 25, 27 (Activate choices), 29, 34, 36, 40, 44, 56, 57, 58 and 68
(`fnv_game::perks`, `activate_choices.rs`, `magic_archetypes.rs`, `nvf-play` `combat.rs`);
abilities of every archetype the masters' perk abilities use (value modifier and script
effects, conditions rechecked: levelup.md §9); quest-stage effects; HasPerk in conditions
and scripts. Script functions are all built but 6 stubs (`tools/smoke/expected/script-stubs.toml`),
so perks that act through scripts are taken as working; none of them was played through.

A perk is **missing** when every effect it has is an entry point NVForever never asks for,
**partly** when some are.

## 2. Totals

| NVForever | Records |
|---|---|
| Works (entry points evaluated, abilities, quest stages) | 160 |
| Works through scripts or conditions only (Wild Wasteland, Terrifying Presence, Nuka Chemist, Retention, add-on rewards …) | 29 |
| **Missing** | 44 |
| **Partly** | 9 |
| Its only gap is VATS's hit chance, which VATS mode does not have (§3.9) | 10 |
| No effect in the records (Deathclaw Omelet, Spicy Casserole, Brain-full, Heart-full, Spine-full: markers) | 5 |

The appendix lists all 257.

## 3. What is missing, by the system it needs

Values are the records' [observed]; what the entry point changes is the GECK wiki's Entry
Point page [documented] unless tagged.

### 3.1 VATS helpers and VATS mode's AP (the owner's example; built 2026-10-24, vats-mode.md §18)

| Perk | Entry point | Record |
|---|---|---|
| Mysterious Stranger (lvl 10) | 28 Mysterious Stranger: "run when the player enters VATS; if non-zero the VATS helper Mysterious Stranger may appear" | Set 1 |
| Miss Fortune (lvl 10) | 41 Miss Fortune: the same for Miss Fortune | Set 1 |
| Friendly Help (challenge reward: 15 helper visits) | 67 VATS Helper Chance: "modifies the chance of a VATS helper" | Multiply 2 |
| Grim Reaper's Sprint (lvl 20) | 35 Player Kill AP Reward: "run when VATS finishes if the player killed at least one enemy; modifies the AP reward from 0" | Add 20 |
| Nerves of Steel (lvl 26), Atomic! (×1.1–1.5 by rads), Ain't Like That Now (×1.25) | 39 Action Point Regen | Multiply |

What the masters hold for the helpers [observed: FalloutNV.esm]: the NPC_ `Stranger`
("Mysterious Stranger", 0005F9CE) with `MysteriousStranger44Magnum` and its own projectile,
faction and combat style; `MissFortune` (0013568A) with "Miss Fortune's Bad Luck Bringer",
whose enchantment casts `MissFortuneGunSpell` (script NVMissFortuneGunSpellScript) and the
knockdown, paralysis, blow-up and plasma spells; the music stingers
`MUSMysteriousStrangerA01`/`A02`; the camera path `MysteriousStrangerCameras` and 13
`MystStrangerCam*` shots; the GMSTs `fVATSStrangerOdds` 0.1, `fVATSFortuneOdds` 0.1,
`fVATSStrangerDistance` 164, `iVATSStrangerMaxHP` 150; the challenge "He Moves in
Mysterious Ways". NVForever already counts "Mysterious Stranger Visits" in the misc
statistics (`misc_stats.rs`) but nothing raises it.

How the original uses them [hypothesis from the names and the fandom page: "10% chance that
the Mysterious Stranger will finish off a target in V.A.T.S."; Miss Fortune "will
incapacitate a target"]: on a VATS attack, odds 0.1 (× Friendly Help's 2) that the helper
appears about 164 units from the player and kills the target (the Stranger) or knocks it
down, paralyses it or blows up its weapon (Miss Fortune); the 150 HP is presumably the
most health a target may have left for the Stranger to be sent. These need checking in the
original before building (an AI observer capture), or a decision to adapt them to VATS
mode, which is NVForever's own (vats-mode.md): VATS mode fires every mark at once, so the
roll would be one per mark as it fires.

### 3.2 Attack, reload, equip and run speed (built 2026-10-24: run speed perk-entry-points.md §8; attack, reload, equip §10; the teammates' attack speed 2026-10-25, §12)

| Entry point | Perks |
|---|---|
| 37 Reload Speed | Rapid Reload × 1.25 |
| 38 Equip Speed | Quick Draw × 1.5 |
| 43 Modify Attack Speed | Fast Shot × 1.2, Trigger Discipline × 0.8, Loose Cannon × 1.3, Slayer × 1.3, Melee Hacker × 1.05/1.1, Old Vaquero × 1.33 and Causeless Rebel × 1.3 (teammate perks), Elijah's Last Words × 1.5, Atomic! × 1.25, Ain't Like That Now × 1.2, NVDLC04RushingWaterEffectsPerk × 1.5 |
| 42 Modify Run Speed | Travel Light × 1.1 (light or no armour), Tunnel Runner × 1.25 (sneaking, light armour), Implant M-5 × 1.2 (sneaking), Atomic! × 1.25, Stealth Suit Mk II × 1.2, NVDLC03LobotimiteReduceSpeedPERK × 0.75 |

perk-entry-points.md §2 left 37/38/43 until the player body's clips can play faster; the
run speed has no such block.

### 3.3 Weight, carrying and fast travel (built 2026-10-24, perk-entry-points.md §7)

| Entry point | Perks |
|---|---|
| 55 Modify Light Items | Pack Rat (Set 1): items at most `fPackRatThreshold` weigh × `fPackRatModifier` [documented: GECK wiki]; neither GMST is in FalloutNV.esm [observed], the perk's text says "2 or less weigh half" |
| 73 Adjust Heavy Weapon Weight | Heavyweight × 0.5: weapons of 10 lb or more after weight mods |
| 22 Get Max Carry Weight | Hoarder + 25 (its −1 SPECIAL ability works) |
| 51 Has Fast Travel Always | Long Haul: fast travel while over-encumbered |

### 3.4 Locks, terminals, sneaking (built 2026-10-24, perk-entry-points.md §7)

| Entry point | Perks |
|---|---|
| 32 Ignore Broken Lock | Infiltrator: another try at a broken lock |
| 20 Ignore Locked Terminal | Computer Whiz: another try at a locked-out terminal |
| 31 Ignore Running During Detection | Silent Running: running no longer counts against sneaking (`fSneakRunningMult` is read in detection) |

### 3.5 Explosives, throwing, mines (built 2026-10-24, perk-entry-points.md §8)

| Entry point | Perks |
|---|---|
| 72 Adjust Explosion Radius | Splash Damage × 1.25 |
| 59 Modify Throwing Velocity | Heave, Ho! × 1.5, Loose Cannon × 0.75 (with a condition) |
| 4 Calculate Mine Explode Chance | Light Step (Set 0), In My Footsteps (Set 0, Dead Money), ED-E's Light Step (Lonesome Road, for ED-E) |

### 3.6 Melee, unarmed and knockdowns (knockdowns and the special moves built 2026-10-24, perk-entry-points.md §11; Weapon Handling and Run 'n Gun 2026-10-25, §12; 3 has nothing to build, §12)

| Entry point | Perks |
|---|---|
| 52 Knockdown Chance | Super Slam 0.15/0.30, And Stay Back 0.1, Elijah's Last Words 0.25 |
| 61, 62, 64, 65, 66 Has Unarmed … Power Attack / Counter | Legion Assault, Ranger Takedown, Scribe Counter, Khan Trick (the trainers' special moves) |
| 53 Modify Weapon Strength Req | Weapon Handling Set 2 |
| 54 Modify Aiming Move Speed | Run 'n Gun × 2 |
| 3 Calculate Weapon Attack AP Cost | Ain't Like That Now × 0.8 (attacks outside VATS cost AP in the original only through this) |

### 3.7 Kills, health, repair, the HUD (built 2026-10-24, perk-entry-points.md §9; Better Healing and Cardiac Arrest 2026-10-25 after captures of the original, §14)

| Entry point | Perks |
|---|---|
| 46 Has Meltdown | Meltdown: foes killed by energy weapons explode |
| 47 See Enemy Health | Living Anatomy: the target's health and DT on its bar (its damage bonus works) |
| 48 Has Jury Rigging | Jury Rigging: repair with roughly similar items |
| 12 Modify Recovered Health | Better Healing × 1.2, Cardiac Arrest × 1.25: the health ingestibles restore (the original game captured: there is no natural healing) |
| 26 Adjust Drinking Radiation | Implant Y-3 × 0.01: water sources (environment radiation is built) |

### 3.8 Companion perks (built 2026-10-25, perk-entry-points.md §13)

| Entry point | Perks |
|---|---|
| 69 Has Improved Detection | Enhanced Sensors (ED-E, base and Lonesome Road): compass range, cloaked targets in VATS |
| 70 Has Improved Spotting | Spotter (Boone): enemies highlighted while aiming |
| 71 Has Improved Item Detection | Search and Mark (Rex): items highlighted while aiming |

### 3.9 VATS's hit chance (built 2026-10-23)

Entry point 8 Calculate To Hit Chance (Commando, Gunslinger, Sniper, Wired Reflexes, Hobbler,
the to-hit half of Fast Shot, Trigger Discipline, Sneering Imperialist, Camarader-E,
Lonesome Road) and 33 Has Concentrated Fire change VATS's hit chance. When this audit was
written VATS mode never missed, so these perks had nothing to change. The owner chose a
hit chance (2026-10-23): VATS mode now has one, and both entry points apply to it
(vats-mode.md §17). The appendix's statuses are as audited, before that.

## 4. Proposed build order

1. **VATS helpers and AP** (§3.1): Mysterious Stranger, Miss Fortune, Friendly Help, Grim
   Reaper's Sprint, Nerves of Steel. Five player perks, one system; the helpers want an
   observer look at the original first.
2. **Locks, terminals, sneaking, weight, fast travel** (§3.3, §3.4): eight perks, each a
   small change where the system already exists.
3. **Explosives, throws, mines, run speed** (§3.5, §3.2's 42): Splash Damage, Heave Ho!,
   Light Step and the run-speed perks.
4. **Kills and HUD** (§3.7): Meltdown, Living Anatomy, Jury Rigging, Implant Y-3.
5. **Clip speeds** (§3.2's 37, 38, 43): needs the player body's playback rate.
6. **Knockdowns and the unarmed moves** (§3.6): needs melee knockdown and the special
   moves (and their VATS forms).
7. **Companion perks** (§3.8) and natural healing (Better Healing).
8. §3.9: decided and built 2026-10-23, a hit chance in VATS mode (vats-mode.md §17).

## Appendix: every PERK record

Statuses below are as audited on 2026-10-23, updated 2026-10-25: every entry point a
row named as missing has been built since (sections 3.1-3.9 say where), entry 3 has
nothing to act on (perk-entry-points.md §12).


"How it acts" is from the records [observed]; entry point names from the GECK wiki
[documented]. The codes in brackets after a status are the entry points not evaluated.

| Perk | Plugin | Kind | How it acts [observed] | NVForever |
|---|---|---|---|---|
| Built to Destroy | FalloutNV | trait | entry points 68 Item Damage; ability IncreaseCritChance | works |
| Fast Shot | FalloutNV | trait | entry points 8 To Hit Chance, 34 Gun Spread, 40 AP Cost, 43 Attack Speed | built by 2026-10-25 (8, 43) |
| Four Eyes | FalloutNV | trait | ability ReducePerception; conditions in ENCH | works |
| Good Natured | FalloutNV | trait | ability IncreaseSkillBarter, IncreaseSkillMedicine, IncreaseSkillRepair, IncreaseSkillScience, IncreaseSkillSpeech, ReduceSkillEnergyWeapons, ReduceSkillExplosives, ReduceSkillGuns, ReduceSkillMeleeWeapons, ReduceSkillUnarmed | works |
| Heavy Handed | FalloutNV | trait | entry points 0 Weapon Damage, 2 My Crit Damage | works |
| Kamikaze | FalloutNV | trait | entry points 56 DT defender; ability IncreaseActionPoints, ReduceDamageResistance | works |
| Loose Cannon | FalloutNV | trait | entry points 40 AP Cost, 43 Attack Speed, 59 Throwing Velocity | built by 2026-10-25 (43, 59) |
| Small Frame | FalloutNV | trait | entry points 6 Limb Damage; ability IncreaseAgility | works |
| Trigger Discipline | FalloutNV | trait | entry points 8 To Hit Chance, 34 Gun Spread, 40 AP Cost, 43 Attack Speed | built by 2026-10-25 (8, 43) |
| Wild Wasteland | FalloutNV | trait | conditions in INFO, RCPE; scripts GenericWildWastelandTriggerSCRIPT, NVDLC02WWTwoBearsTriggerScript, NVDLC03SinkAutoDocSCRIPT … | works through scripts / conditions |
| Action Boy | FalloutNV | perk | ability IncreaseActionPoints | works |
| Action Girl | FalloutNV | perk | ability IncreaseActionPoints | works |
| Adamantium Skeleton | FalloutNV | perk | entry points 6 Limb Damage | works |
| Animal Friend | FalloutNV | perk | quest stage Generic@20, Generic@30; conditions in INFO | works |
| Better Criticals | FalloutNV | perk | entry points 2 My Crit Damage | works |
| Black Widow | FalloutNV | perk | entry points 0 Weapon Damage; conditions in INFO | works |
| Bloody Mess | FalloutNV | perk | entry points 0 Weapon Damage; ability BloodyMessEffect | works |
| Bug Stomper | FalloutNV | perk | entry points 0 Weapon Damage | works |
| Cannibal | FalloutNV | perk | entry points 27 Activate; conditions in INFO, PERK | works |
| Center of Mass | FalloutNV | perk | entry points 0 Weapon Damage | works |
| Chem Resistant | FalloutNV | perk | entry points 23 Addiction Chance | works |
| Chemist | FalloutNV | perk | entry points 25 Positive Chem Duration | works |
| Cherchez La Femme | FalloutNV | perk | entry points 0 Weapon Damage; conditions in INFO | works |
| Commando | FalloutNV | perk | entry points 8 To Hit Chance | nothing to change (VATS mode never misses) (8) |
| Comprehension | FalloutNV | perk | entry points 11 Book Skill Points; conditions in ALCH | works |
| Computer Whiz | FalloutNV | perk | entry points 20 Ignore Locked Terminal | built by 2026-10-25 (20) |
| Concentrated Fire | FalloutNV | perk | entry points 33 Concentrated Fire | nothing to change (VATS mode never misses) (33) |
| Confirmed Bachelor | FalloutNV | perk | entry points 0 Weapon Damage; conditions in INFO | works |
| Cowboy | FalloutNV | perk | entry points 0 Weapon Damage | works |
| Demolition Expert | FalloutNV | perk | entry points 0 Weapon Damage | works, on blasts too (perk-entry-points.md §2, "Blasts") |
| Educated | FalloutNV | perk | entry points 10 Gained Skill Points | works |
| Entomologist | FalloutNV | perk | entry points 0 Weapon Damage | works |
| Explorer | FalloutNV | perk | quest stage Generic@50; conditions in INFO | works |
| Fast Metabolism | FalloutNV | perk | conditions in ALCH, SPEL | works through scripts / conditions |
| Ferocious Loyalty | FalloutNV | perk | quest stage FerociousLoyaltyQuest@10 | works |
| Finesse | FalloutNV | perk | ability IncreaseCritChance | works |
| Fortune Finder | FalloutNV | perk | quest stage Generic@60 | works |
| Friend of the Night | FalloutNV | perk | quest stage FriendOfTheNightQuest@10 | works |
| Ghastly Scavenger | FalloutNV | perk | entry points 27 Activate; conditions in INFO | works |
| Grim Reaper's Sprint | FalloutNV | perk | entry points 35 Kill AP Reward | built by 2026-10-25 (35) |
| Gunslinger | FalloutNV | perk | entry points 8 To Hit Chance | nothing to change (VATS mode never misses) (8) |
| Hand Loader | FalloutNV | perk | entry points 57 Ammo Item Chance; conditions in RCPE | works |
| Heave, Ho! | FalloutNV | perk | entry points 59 Throwing Velocity | built by 2026-10-25 (59) |
| Hit the Deck | FalloutNV | perk | entry points 56 DT defender | works; not against a blast with no weapon behind it [hypothesis: a placed mine; perk-entry-points.md §2, "Blasts"] |
| Hunter | FalloutNV | perk | entry points 2 My Crit Damage | works |
| Infiltrator | FalloutNV | perk | entry points 32 Ignore Broken Lock | built by 2026-10-25 (32) |
| Intense Training | FalloutNV | perk | quest stage Generic@101, Generic@102, Generic@103, Generic@104, Generic@105, Generic@106, Generic@107, Generic@108, Generic@109, Generic@110 | works |
| Jury Rigging | FalloutNV | perk | entry points 48 Jury Rigging | built by 2026-10-25 (48) |
| Lady Killer | FalloutNV | perk | entry points 0 Weapon Damage; conditions in INFO | works |
| Laser Commander | FalloutNV | perk | entry points 0 Weapon Damage, 1 My Crit Chance | works |
| Lead Belly | FalloutNV | perk | entry points 44 Radiation Consumed | works |
| Life Giver | FalloutNV | perk | ability IncreaseHealth | works |
| Light Step | FalloutNV | perk | entry points 4 Mine Explode Chance | built by 2026-10-25 (4) |
| Living Anatomy | FalloutNV | perk | entry points 0 Weapon Damage, 47 See Enemy Health | built by 2026-10-25 (47) |
| Long Haul | FalloutNV | perk | entry points 51 Fast Travel Always | built by 2026-10-25 (51) |
| Math Wrath | FalloutNV | perk | entry points 40 AP Cost; conditions in INFO | works |
| Meltdown | FalloutNV | perk | entry points 46 Meltdown | built by 2026-10-25 (46) |
| Miss Fortune | FalloutNV | perk | entry points 41 Miss Fortune | built by 2026-10-25 (41) |
| Mister Sandman | FalloutNV | perk | entry points 27 Activate | works |
| Mysterious Stranger | FalloutNV | perk | entry points 28 Mysterious Stranger | built by 2026-10-25 (28) |
| Nerd Rage! | FalloutNV | perk | ability IncreaseDamageThreshold, IncreaseStrength | works |
| Nerves of Steel | FalloutNV | perk | entry points 39 AP Regen | built by 2026-10-25 (39) |
| Night Person | FalloutNV | perk | ability IncreaseIntelligence, IncreasePerception | works |
| Ninja | FalloutNV | perk | entry points 1 My Crit Chance, 2 My Crit Damage | works |
| Nuka Chemist | FalloutNV | perk | conditions in RCPE | works through scripts / conditions |
| Pack Rat | FalloutNV | perk | entry points 55 Light Items; conditions in INFO | built by 2026-10-25 (55) |
| Paralyzing Palm | FalloutNV | perk | entry points 29 Paralyzing Palm; conditions in IDLE | works |
| Piercing Strike | FalloutNV | perk | entry points 58 DT attacker | works |
| Plasma Spaz | FalloutNV | perk | entry points 40 AP Cost | works |
| Purifier | FalloutNV | perk | entry points 0 Weapon Damage | works |
| Pyromaniac | FalloutNV | perk | entry points 0 Weapon Damage | works |
| Quick Draw | FalloutNV | perk | entry points 38 Equip Speed | built by 2026-10-25 (38) |
| Rad Absorption | FalloutNV | perk | ability DLC03RadAbsorb | works |
| Rad Child | FalloutNV | perk | conditions in SPEL; scripts VCG04ActivatorScript | works through scripts / conditions (regeneration by the second since #3471, 2026-11-23) |
| Rad Resistance | FalloutNV | perk | ability IncreaseRadiationResistance | works |
| Rapid Reload | FalloutNV | perk | entry points 37 Reload Speed | built by 2026-10-25 (37) |
| Retention | FalloutNV | perk | conditions in ALCH; scripts VCG04ActivatorScript | works through scripts / conditions |
| Robotics Expert | FalloutNV | perk | entry points 0 Weapon Damage, 27 Activate | works |
| Run 'n Gun | FalloutNV | perk | entry points 54 Aiming Move Speed | built 2026-10-25 (perk-entry-points.md §12) |
| Scrounger | FalloutNV | perk | quest stage Generic@70 | works |
| Set Lasers for Fun | FalloutNV | perk | entry points 1 My Crit Chance | works |
| Shotgun Surgeon | FalloutNV | perk | entry points 58 DT attacker | works |
| Silent Running | FalloutNV | perk | entry points 31 Ignore Running During Detection | built by 2026-10-25 (31) |
| Slayer | FalloutNV | perk | entry points 43 Attack Speed | built by 2026-10-25 (43) |
| Sniper | FalloutNV | perk | entry points 8 To Hit Chance | nothing to change (VATS mode never misses) (8) |
| Solar Powered | FalloutNV | perk | ability IncreaseStrength, Regen | works |
| Splash Damage | FalloutNV | perk | entry points 72 Explosion Radius | built by 2026-10-25 (72) |
| Spray and Pray | FalloutNV | perk | entry points 0 Weapon Damage | works |
| Stonewall | FalloutNV | perk | entry points 56 DT defender | works |
| Strong Back | FalloutNV | perk | ability IncreaseCarryWeight; conditions in INFO | works |
| Super Slam | FalloutNV | perk | entry points 52 Knockdown Chance | built by 2026-10-25 (52) |
| Swift Learner | FalloutNV | perk | entry points 9 XP | works |
| Tag! | FalloutNV | perk | quest stage Generic@40 | works |
| Terrifying Presence | FalloutNV | perk | conditions in INFO | works through scripts / conditions |
| The Professional | FalloutNV | perk | entry points 2 My Crit Damage | works |
| Toughness | FalloutNV | perk | entry points 56 DT defender | works, on blasts too (perk-entry-points.md §2, "Blasts") |
| Travel Light | FalloutNV | perk | entry points 42 Run Speed | built by 2026-10-25 (42) |
| Unstoppable Force | FalloutNV | perk | entry points 0 Weapon Damage | works |
| Vigilant Recycler | FalloutNV | perk | entry points 57 Ammo Item Chance; conditions in RCPE | works |
| Weapon Handling | FalloutNV | perk | entry points 53 Weapon Str Req | built 2026-10-25 (§12) |
| Abominable | FalloutNV | other | entry points 0 Weapon Damage | works |
| Agility Implant | FalloutNV | other | ability IncreaseAgility | works |
| Animal Control | FalloutNV | other | entry points 0 Weapon Damage | works |
| Ant Might | FalloutNV | other | ability IncreaseFireResistance, IncreaseStrength | works |
| Ant Sight | FalloutNV | other | ability IncreaseFireResistance, IncreasePerception | works |
| Barkskin | FalloutNV | other | ability IncreaseDamageResistance | works |
| Beautiful Beatdown | FalloutNV | other | entry points 40 AP Cost | works |
| Better Healing | FalloutNV | other | entry points 12 Recovered Health | built 2026-10-25 (perk-entry-points.md §14) |
| Bonds of Steel | FalloutNV | other | entry points 56 DT defender | works |
| Calm Heart | FalloutNV | other | ability CalmHeartAbilityEffect | works |
| Calm Heart | FalloutNV | other | ability CalmHeartAbilityEffect | works |
| Camel of the Mojave | FalloutNV | other | conditions in ALCH, SPEL; scripts ChallengeWaterSCRIPT | works through scripts / conditions |
| Causeless Rebel | FalloutNV | other | entry points 43 Attack Speed | built 2026-10-25 (§12, the teammate's gun and blows) |
| Charisma Implant | FalloutNV | other | ability IncreaseCharisma | works |
| Child at Heart | FalloutNV | other | conditions in INFO; scripts PressDemoQuestScript, VCG04ActivatorScript | works through scripts / conditions |
| Companion Suite | FalloutNV | other | entry points 58 DT attacker; ability IncreaseDamageResistance, IncreaseSkillSneak, ReduceCarryWeight | works |
| Day Tripper | FalloutNV | other | entry points 25 Positive Chem Duration | works |
| Deathclaw Omelet | FalloutNV | other | — | no effect in the records |
| Dine and Dash | FalloutNV | other | entry points 27 Activate | works |
| Dream Crusher | FalloutNV | other | entry points 36 Enemy Crit Chance | works |
| Endurance Implant | FalloutNV | other | ability IncreaseEndurance | works |
| Enhanced Sensors | FalloutNV | other | entry points 69 Improved Detection | built 2026-10-25 (§13) |
| Fast Times | FalloutNV | other | conditions in ALCH; scripts ChallengeTurboSCRIPT | works through scripts / conditions |
| Free Radical | FalloutNV | other | conditions in ALCH; scripts ChallengeRadAwaySCRIPT | works through scripts / conditions |
| Friendly Help | FalloutNV | other | entry points 67 VATS Helper Chance | built by 2026-10-25 (67) |
| Full Maintenance | FalloutNV | other | entry points 68 Item Damage | works |
| Gun Strength Fail | FalloutNV | other | entry points 34 Gun Spread | works |
| Hand of Vengeance | FalloutNV | other | entry points 0 Weapon Damage | works |
| Hematophage | FalloutNV | other | conditions in ALCH | works through scripts / conditions |
| Intelligence Implant | FalloutNV | other | ability IncreaseIntelligence | works |
| Junior Survivor | FalloutNV | other | ability IncreaseCritChance, IncreasePoisonResistance, IncreaseRadiationResistance | works |
| Junior Survivor | FalloutNV | other | ability IncreasePoisonResistance, IncreaseRadiationResistance, IncreaseSkillSneak, IncreaseSkillSpeech | works |
| Junior Survivor | FalloutNV | other | ability IncreaseDamageResistance, IncreasePoisonResistance, IncreaseRadiationResistance | works |
| Junior Survivor | FalloutNV | other | ability IncreasePoisonResistance, IncreaseRadiationResistance, IncreaseSkillMedicine, IncreaseSkillScience | works |
| Junior Survivor | FalloutNV | other | ability IncreaseHealth, IncreasePoisonResistance, IncreaseRadiationResistance | works |
| Khan Trick | FalloutNV | other | entry points 65 Unarmed Left PA, 66 Unarmed Right PA | built by 2026-10-25 (65, 66) |
| Legion Assault | FalloutNV | other | entry points 61 Unarmed Fwd PA | built by 2026-10-25 (61) |
| Lord Death | FalloutNV | other | entry points 0 Weapon Damage | works |
| Luck Implant | FalloutNV | other | ability IncreaseLuck | works |
| Machine Head | FalloutNV | other | entry points 0 Weapon Damage | works |
| Meat of Champions | FalloutNV | other | scripts MeatOfChampionsScript | works through scripts / conditions |
| Melee Hacker | FalloutNV | other | entry points 43 Attack Speed | built by 2026-10-25 (43) |
| Monocyte Breeder | FalloutNV | other | ability ImplantRegenEffect | works |
| Mutant Massacrer | FalloutNV | other | entry points 0 Weapon Damage | works |
| Old Vaquero | FalloutNV | other | entry points 43 Attack Speed | built 2026-10-25 (§12) |
| Perception Implant | FalloutNV | other | ability IncreasePerception | works |
| Power Armor Training | FalloutNV | other | quest stage Generic@80; conditions in INFO | works |
| PsychoPerk | FalloutNV | other | entry points 0 Weapon Damage | works |
| Rad Regeneration | FalloutNV | other | ability RestoreLimbChest, RestoreLimbHead, RestoreLimbLeftArm, RestoreLimbLeftLeg, RestoreLimbRightArm, RestoreLimbRightLeg | works |
| Ranger Takedown | FalloutNV | other | entry points 62 Unarmed Back PA | built by 2026-10-25 (62) |
| Regular Maintenance | FalloutNV | other | entry points 68 Item Damage | works |
| Scribe Assistant | FalloutNV | other | scripts NVDLC01NewVegasFollowerFireQuestSCRIPT, NVDLC02ZionDoorSCRIPT, NVDLC03TeleportEffectTimerSCRIPT … | works through scripts / conditions |
| Scribe Counter | FalloutNV | other | entry points 64 Unarmed Counter; conditions in INFO | built by 2026-10-25 (64) |
| Search and Mark | FalloutNV | other | entry points 71 Improved Item Detection | built 2026-10-25 (§13) |
| Spicy Casserole | FalloutNV | other | — | no effect in the records |
| Spotter | FalloutNV | other | entry points 70 Improved Spotting | built 2026-10-25 (§13) |
| Steady Chem | FalloutNV | other | entry points 34 Gun Spread | works |
| Stealth Girl | FalloutNV | other | entry points 2 My Crit Damage; conditions in ALCH | works |
| Strength Implant | FalloutNV | other | ability IncreaseStrength | works |
| Sub-Dermal Armor | FalloutNV | other | entry points 56 DT defender | works, on blasts too (perk-entry-points.md §2, "Blasts") |
| Survival Expert | FalloutNV | other | ability IncreaseCritChance, IncreasePoisonResistance, IncreaseRadiationResistance | works |
| Survival Expert | FalloutNV | other | ability IncreasePoisonResistance, IncreaseRadiationResistance, IncreaseSkillSneak, IncreaseSkillSpeech | works |
| Survival Expert | FalloutNV | other | ability IncreaseDamageResistance, IncreasePoisonResistance, IncreaseRadiationResistance | works |
| Survival Expert | FalloutNV | other | ability IncreasePoisonResistance, IncreaseRadiationResistance, IncreaseSkillMedicine, IncreaseSkillScience | works |
| Survival Expert | FalloutNV | other | ability IncreaseHealth, IncreasePoisonResistance, IncreaseRadiationResistance | works |
| Survival Guru | FalloutNV | other | ability IncreaseCritChance, IncreasePoisonResistance, IncreaseRadiationResistance | works |
| Survival Guru | FalloutNV | other | ability IncreasePoisonResistance, IncreaseRadiationResistance, IncreaseSkillSneak, IncreaseSkillSpeech | works |
| Survival Guru | FalloutNV | other | ability IncreaseDamageResistance, IncreasePoisonResistance, IncreaseRadiationResistance | works |
| Survival Guru | FalloutNV | other | ability IncreasePoisonResistance, IncreaseRadiationResistance, IncreaseSkillMedicine, IncreaseSkillScience | works |
| Survival Guru | FalloutNV | other | ability IncreaseHealth, IncreasePoisonResistance, IncreaseRadiationResistance | works |
| Tough Guy | FalloutNV | other | entry points 6 Limb Damage | works |
| Well Rested | FalloutNV | other | entry points 9 XP | works |
| Whiskey Rose | FalloutNV | other | conditions in ALCH, SPEL; scripts NVDLC01NewVegasFollowerFireQuestSCRIPT, NVDLC02ZionDoorSCRIPT, NVDLC03TeleportEffectTimerSCRIPT … | works through scripts / conditions |
| Wired Reflexes | FalloutNV | other | entry points 8 To Hit Chance | nothing to change (VATS mode never misses) (8) |
| YaoGuaiMeatPerk | FalloutNV | other | entry points 0 Weapon Damage | works |
| Claustrophobia | OldWorldBlues | trait | ability IncreaseAgility, IncreaseCharisma, IncreaseEndurance, IncreaseIntelligence, IncreaseLuck, IncreasePerception, IncreaseStrength, ReduceAgility, ReduceCharisma, ReduceEndurance, ReduceIntelligence, ReduceLuck, ReducePerception, ReduceStrength | works |
| Early Bird | OldWorldBlues | trait | ability IncreaseAgility, IncreaseCharisma, IncreaseEndurance, IncreaseIntelligence, IncreaseLuck, IncreasePerception, IncreaseStrength, ReduceAgility, ReduceCharisma, ReduceEndurance, ReduceIntelligence, ReduceLuck, ReducePerception, ReduceStrength | works |
| Hoarder | OldWorldBlues | trait | entry points 22 Max Carry Weight; ability ReduceAgility, ReduceCharisma, ReduceEndurance, ReduceIntelligence, ReduceLuck, ReducePerception, ReduceStrength | built by 2026-10-25 (22) |
| Hot Blooded | OldWorldBlues | trait | entry points 0 Weapon Damage; ability ReduceAgility, ReducePerception | works |
| Logan's Loophole | OldWorldBlues | trait | entry points 9 XP, 10 Gained Skill Points, 23 Addiction Chance, 25 Positive Chem Duration | works |
| Skilled | OldWorldBlues | trait | entry points 9 XP; ability NVDLC03TraitSkilledBE | works |
| Ain't Like That Now | LonesomeRoad | perk | entry points 3 Attack AP Cost, 36 Enemy Crit Chance, 39 AP Regen, 43 Attack Speed; quest stage NVDLC04GenericPerkQUEST@30 | built by 2026-10-25 (39, 43; 3 has nothing to act on, perk-entry-points.md §12) |
| Alertness | LonesomeRoad | perk | ability IncreasePerception | works |
| And Stay Back | DeadMoney | perk | entry points 52 Knockdown Chance | built by 2026-10-25 (52) |
| Atomic! | OldWorldBlues | perk | entry points 39 AP Regen, 42 Run Speed, 43 Attack Speed, 56 DT defender; ability IncreaseStrength, NVDLC03AtomicPerkMessageBE | built by 2026-10-25 (39, 42, 43) |
| Broad Daylight | LonesomeRoad | perk | ability IncreaseSkillSneak | works |
| Burden to Bear | LonesomeRoad | perk | ability IncreaseCarryWeight | works |
| Certified Tech | LonesomeRoad | perk | entry points 1 My Crit Chance, 21 LL On Death | works |
| Eye For Eye | HonestHearts | perk | entry points 0 Weapon Damage | works |
| Fight the Power! | HonestHearts | perk | entry points 1 My Crit Chance, 56 DT defender | works |
| Grunt | HonestHearts | perk | entry points 0 Weapon Damage | works |
| Heavyweight | DeadMoney | perk | entry points 73 Heavy Weapon Weight | built by 2026-10-25 (73) |
| Here and Now | HonestHearts | perk | quest stage Generic@10 | works |
| Hobbler | DeadMoney | perk | entry points 8 To Hit Chance | nothing to change (VATS mode never misses) (8) |
| Home on the Range | HonestHearts | perk | scripts CraftingCampfireRecipesScript | works through scripts / conditions |
| Implant GRX | OldWorldBlues | perk | quest stage NVDLC03GenericPerkQUEST@10, NVDLC03GenericPerkQUEST@11 | works |
| In Shining Armor | DeadMoney | perk | entry points 56 DT defender; conditions in PERK | works |
| Irradiated Beauty | LonesomeRoad | perk | scripts NVDLC04GenericPerkQuestSCRIPT | works through scripts / conditions |
| Junk Rounds | DeadMoney | perk | conditions in RCPE | works through scripts / conditions |
| Just Lucky I'm Alive | LonesomeRoad | perk | entry points 2 My Crit Damage, 36 Enemy Crit Chance | works |
| Lessons Learned | LonesomeRoad | perk | entry points 9 XP | works |
| Light Touch | DeadMoney | perk | entry points 1 My Crit Chance, 36 Enemy Crit Chance | works |
| Mad Bomber | GunRunnersArsenal | perk | conditions in RCPE | works through scripts / conditions |
| Mile in Their Shoes | OldWorldBlues | perk | conditions in ALCH | works through scripts / conditions |
| Old World Gourmet | DeadMoney | perk | entry points 23 Addiction Chance; conditions in ALCH | works |
| Roughin' It | LonesomeRoad | perk | scripts NVDLC04GenericPerkQuestSCRIPT | works through scripts / conditions |
| Sneering Imperialist | HonestHearts | perk | entry points 0 Weapon Damage, 8 To Hit Chance; conditions in INFO | works, but its to-hit part has nothing to change (8) |
| Them's Good Eatin' | OldWorldBlues | perk | entry points 21 LL On Death | works |
| Thought You Died | LonesomeRoad | perk | entry points 0 Weapon Damage, 36 Enemy Crit Chance; ability IncreaseHealth; quest stage NVDLC04GenericPerkQUEST@30 | works |
| Tribal Wisdom | HonestHearts | perk | entry points 6 Limb Damage, 27 Activate; ability IncreasePoisonResistance | works |
| Tunnel Runner | LonesomeRoad | perk | entry points 42 Run Speed | built by 2026-10-25 (42) |
| Voracious Reader | LonesomeRoad | perk | conditions in RCPE; scripts NVDLC04GenericPerkQuestSCRIPT | works through scripts / conditions |
| Walker Instinct | LonesomeRoad | perk | ability IncreaseAgility, IncreasePerception | works |
| Big Brained | OldWorldBlues | other | entry points 23 Addiction Chance, 56 DT defender | works |
| Big-Brained | OldWorldBlues | other | entry points 23 Addiction Chance; ability IncreaseDamageResistance | works |
| Brain-full | OldWorldBlues | other | — | no effect in the records |
| Brainless | OldWorldBlues | other | entry points 23 Addiction Chance, 56 DT defender | works |
| Camarader-E | LonesomeRoad | other | entry points 0 Weapon Damage, 8 To Hit Chance, 56 DT defender | works, but its to-hit part has nothing to change (8) |
| Camarader-E | LonesomeRoad | other | entry points 0 Weapon Damage, 8 To Hit Chance, 56 DT defender; quest stage NVDLC04GenericPerkQUEST@10, NVDLC04GenericPerkQUEST@12 | works, but its to-hit part has nothing to change (8) |
| Cardiac Arrest | OldWorldBlues | other | entry points 12 Recovered Health, 36 Enemy Crit Chance; ability IncreasePoisonResistance | works (12 built 2026-10-25, §14) |
| Cardiac Arrest | OldWorldBlues | other | entry points 36 Enemy Crit Chance; ability IncreasePoisonResistance | works |
| Coin Operator | DeadMoney | other | conditions in INFO, RCPE | works through scripts / conditions |
| DNAgent | OldWorldBlues | other | entry points 0 Weapon Damage | works |
| DNAvenger | OldWorldBlues | other | entry points 0 Weapon Damage | works |
| Dead Man's Burden | LonesomeRoad | other | quest stage NVDLC04GenericPerkQUEST@40, NVDLC04GenericPerkQUEST@50 | works |
| Divide Survivor | LonesomeRoad | other | quest stage NVDLC04GenericPerkQUEST@40, NVDLC04GenericPerkQUEST@60 | works |
| Elijah's Last Words | DeadMoney | other | entry points 43 Attack Speed, 52 Knockdown Chance; conditions in INFO | built by 2026-10-25 (43, 52) |
| Elijah's Ramblings | DeadMoney | other | entry points 2 My Crit Damage; conditions in INFO | works |
| Enhanced Sensors | LonesomeRoad | other | entry points 69 Improved Detection | built 2026-10-25 (§13) |
| Ghost Hunter | DeadMoney | other | scripts NVDLC01GhostSCRIPT | works through scripts / conditions |
| Heart-full | OldWorldBlues | other | — | no effect in the records |
| Heartless | OldWorldBlues | other | entry points 36 Enemy Crit Chance; ability IncreasePoisonResistance | works |
| Implant C-13 | OldWorldBlues | other | entry points 0 Weapon Damage | works |
| Implant M-5 | OldWorldBlues | other | entry points 42 Run Speed | built by 2026-10-25 (42) |
| Implant Y-3 | OldWorldBlues | other | entry points 26 Drinking Radiation | built by 2026-10-25 (26) |
| Implant Y-7 | OldWorldBlues | other | scripts NVDLC03AuralStealthSuitObjectBACKUPSCRIPT, NVDLC03GenericPerkQuestSCRIPT, NVDLC03SinkAutoDocSCRIPT | works through scripts / conditions |
| In My Footsteps | DeadMoney | other | entry points 4 Mine Explode Chance; ability IncreaseSkillSneak | built by 2026-10-25 (4) |
| Light Step | LonesomeRoad | other | entry points 4 Mine Explode Chance | built by 2026-10-25 (4) |
| Lonesome Road | LonesomeRoad | other | entry points 0 Weapon Damage, 8 To Hit Chance | works, but its to-hit part has nothing to change (8) |
| Marked | LonesomeRoad | other | entry points 0 Weapon Damage, 56 DT defender | works |
| NVDLC01InShiningArmorEyewear | DeadMoney | other | entry points 56 DT defender | works |
| NVDLC03LobotimiteReduceSpeedPERK | OldWorldBlues | other | entry points 42 Run Speed | works: a player perk (§12), run speed built 2026-10-24 |
| NVDLC04RushingWaterEffectsPerk | LonesomeRoad | other | entry points 43 Attack Speed | built by 2026-10-25 (43) |
| Quiet as the Waters | HonestHearts | other | conditions in SPEL; scripts NVDLC02FireTriggerSCRIPT, NVDLC02HireTriggerSCRIPT, NVDLC02WakingCloudSCRIPT … | works through scripts / conditions |
| Reinforced Spine | OldWorldBlues | other | entry points 56 DT defender; ability IncreaseStrength, RestoreLimbChest | works |
| Reinforced Spine | OldWorldBlues | other | entry points 56 DT defender; ability IncreaseStrength; conditions in SPEL | works |
| Robot Compliance | OldWorldBlues | other | entry points 27 Activate | works |
| Scourge of the East | LonesomeRoad | other | quest stage NVDLC04GenericPerkQUEST@40, NVDLC04GenericPerkQUEST@80 | works |
| Sierra Madre Martini | DeadMoney | other | conditions in RCPE | works through scripts / conditions |
| Signal Interference | DeadMoney | other | scripts NVDLC01ChristineSCRIPT, NVDLC01FireScript, NVDLC01HireScript | works through scripts / conditions |
| Spine-full | OldWorldBlues | other | — | no effect in the records |
| Spineless | OldWorldBlues | other | entry points 56 DT defender; ability IncreaseStrength; conditions in SPEL | works |
| Stealth Suit Mk II | OldWorldBlues | other | entry points 42 Run Speed | built by 2026-10-25 (42) |
| The Bear-Slayer | LonesomeRoad | other | quest stage NVDLC04GenericPerkQUEST@40, NVDLC04GenericPerkQUEST@70 | works |
| Unclean Living | DeadMoney | other | scripts NVDLC01DeanScript, NVDLC01FireScript, NVDLC01HireScript | works through scripts / conditions |
| Way of the Canaanite | HonestHearts | other | entry points 1 My Crit Chance, 34 Gun Spread | works |
| Well-Stacked Cairns | HonestHearts | other | scripts NVDLC02FireTriggerSCRIPT, NVDLC02FollowsChalkSCRIPT, NVDLC02HireTriggerSCRIPT … | works through scripts / conditions |
