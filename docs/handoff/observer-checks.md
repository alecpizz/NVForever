# Observer checks: gameplay guesses the original game can settle

**This file takes no new checks** (#3674). A new check is a file of its own in
[observer-checks/](observer-checks/README.md), so that pull requests adding checks do not
conflict here. The numbered checks below stay until they are settled, keep their numbers,
and get their partial answers here.

Guesses about how the original behaves that a capture can settle without the owner.
The observer can run scripts, take screenshots, record video, and tap keys, so it can
also drive menus. Sorted by how much of the game each check moves, most first.
Taken from the old owner-review log on 2026-10-24 (full log:
`git show 85dcaace:docs/owner-review.md`); decisions that need the owner are in
[owner-review.md](../owner-review.md).

Tools: [ai-observer](../spec-notes/ai-observer.md) (campaigns, fight field set, events,
timed actions, shots), [xnvse-capture](../spec-notes/xnvse-capture.md) (jobs and short
scripts), [owner-decision-captures](../spec-notes/owner-decision-captures.md) (what
earlier rounds settled). Start from the `nvfstrip` save unless a check needs an early
game state. When a check is settled, write the answer into the linked spec, fix the
engine if needed, and delete the item here.

## What the open checks need (2026-10-31)

- **Unattended, from the saves we have** (a session can run them): 1, 2, the rest of 3,
  5 (weapon health around scripted shots), 6 (nvftest, Sunny), the rest of 8, 11 (72 h at a raised
  TimeScale), 13, 15, 20, 21, 22 (dialogue events), the rest of 33, 35, 36, 52, 53, and
  #1076 (melee reach; measured 2026-11-01, o14_reach: combat-rules §10; built in #1334),
  92.
- **A save from the owner** (quest or character state no save has yet):
  - Hardcore on: done (nvfHardcore).
  - Boone, Rex and ED-E hired (19, 22's complaints, 18's wheel): Boone done (nvfBoone).
  - Old World Blues just started in the Think Tank (43, 37); X-8 done, EM pulse module
    (38). Honest Hearts at Flight from Zion with Daniel (49).
  - VMS46 at Anders' thanks (39); Stone on his night route, VMS31 (50); HELIOS One before
    the west terminal (46, 25 at Intelligence 1); Oliver on House's path (48); a finished
    game at the end slides (28 and 29 answered).
- **A save made inside the capture** (allowed, probe-saves rule): 41 (the emergency radio,
  timed). 24 answered 2026-11-01 (save-load.md §5 item 4).
- **Observer limits met 2026-11-01** (o18-o24): the runtime compiler takes no Editor ID
  in a raw field expression (`GetItemCount WeapGrenadeFrag`, `VStreetFluffNPC.iPlayerInArea`
  read nothing; pass forms as `names`); `let x := <ref>.GetItemCount <form>` did not compile
  even with a valid form from `names`; Variable11 and up are not actor values (01-10 are);
  `SetNumericINISetting "bGeneralSubtitles:Interface" 1` showed no subtitles; a timed
  cleanup by sample count can miss a session's end (samples run 15-60 a second) and
  `GameHour >= 10.13` fired at once (sessions start near 10.1, not 10.0).
- **Observer notes from o26 (2026-11-06):** sessions of one campaign run in one game, one
  after another: the save is loaded once, so each session starts where the last left
  off (reset in a `timed` item). A `timed` script naming a ref variable `rW` or `rO`
  did not compile ("script did not compile" for every hook), the same lines with
  `rItem`, `rBase` did (like `fW` before). Values a script works out go to the header
  through the player's `Variable01`-`Variable10` (`player.SetAV VariableNN x`, read as
  `player.GetAV VariableNN`); they come back as whole numbers. Real time from a header:
  `floor(days) × 24 + hour` (GameDaysPassed is fractional). `IsHardcore` and
  `SetHardcore` need `player.` in a script. The capture job's `query` steps with a full
  script all failed again (plugin version 10, o26_setav steps), so read through the
  observer.
- **Screenshots or video** (window in front, both locks): 7 (casino menus by key taps),
  12 (Ringo, Nash, Red Lucy, Doc Mitchell: nvftest/nvfdoc have them), 16, 17, 26, 27, 54.

## P1: changes many fights or the whole run

1. **Which weapon NPCs carry and draw.** Ours ranks weapons by base damage plus
   explosion damage, so about 40 NPCs now draw grenade rifles or launchers. Any AddItem
   (even caps) makes an NPC equip its best weapon and armour again. Melee Only / Ranged
   Only styles switch weapons only in combat. To check: read GetEquippedObject 5 on
   those 40 NPCs and on a sample of armed NPCs at first load, and again after AddItem
   and after a combat start. ([npc-behaviour-compare §9.1](../spec-notes/npc-behaviour-compare.md),
   [combat-ai §18](../design/combat-ai.md), [companion-wheel §4](../spec-notes/companion-wheel.md))
   Answered in part 2026-11-01 (o13_reequip, nvftest at noon, observer `weapon` field
   every 5 loops, nobody in combat or with a weapon out) [observed]. At load: Sunny a
   Varmint Rifle, Easy Pete and Settler 1 a .357 Revolver, Ringo a 9mm Pistol (Trudy
   not in the loaded area). Then, to all of them in turn:
   - `AddItem` 10 caps: nobody's weapon changed.
   - a Hunting Rifle and 20 .308: **Easy Pete and Ringo equipped it at once** (holstered,
     out of combat); Sunny kept her Varmint Rifle, Settler 1 his .357.
   - a Grenade Rifle and 10 40mm grenades: **nobody** took it.
   - a Sledgehammer: no change.
   So an AddItem can re-equip outside combat, but a Grenade Rifle did not outrank a
   Hunting Rifle, a .357 or a Varmint Rifle, as it does in ours (base plus explosion
   damage): ours' ranking is the divergence. Why Sunny and Settler 1 kept theirs (skill,
   a weapon of their own class, condition) is open. The first tries (o11_02, o11_03,
   steps jobs) failed on the plugin compiling only one full script per job
   (xnvse-capture.md §6).
   In combat (o15_round, nvfairport, 2026-11-01) [observed]: a trooper holding a 10mm
   Pistol and one holding a Hunting Rifle, each also carrying a Grenade Rifle and 20
   40mm grenades, kept their own gun drawn through 42 and 15 shots; neither drew the
   Grenade Rifle. Same divergence as above (ours would draw it).
2. **Who attacks you, and who joins a fight.** Ours:
   - 618 placed Aggressive (not Very Aggressive) animals ignore you until attacked.
   - Inside an aggro radius, an actor warns twice 5 s apart, then attacks.
   - Actors of a leveled template take the factions of our level-1 entry (192 types).
   - Sharing a faction or base NPC makes actors allies.
   - Onlookers who would be attacked by the attacker join at once.
   - Cowardly actors never help.
   - An escort fights its charge's attackers.
   - Anyone hostile to you is hostile to your teammates, and teammates ignore your hits.
   - People on Find with Allow Killing attack only actors carrying the item (creatures
     hunt anything; already captured).
   To check: run campaigns near coyotes, rats and bighorners at Goodsprings, the Primm
   ambushers, a gecko attack on a settler, an escort, and shots at Boone, with the fight
   field set and OnStartCombat events. ([factions](../spec-notes/factions.md),
   [combat-ai §10, §12, §13](../design/combat-ai.md), [followers](../design/followers.md),
   [packages §3.1](../spec-notes/packages.md))
   Tried 2026-11-01 (o16_air, nvfairport runway, mortal player healed by script, nothing
   set on the player) [observed]: two spawned CrCoyote (GetAV Aggression 1, Aggressive)
   700 units off detected the player and fought it unprovoked (one in combat at once,
   the other OnStartCombat about 1 s in; 13 bites); two more at 250 units the same (15
   bites). Two VCrTier2GeckoMed (Aggression 2) were in combat at once. Two CrDogWild
   (Aggression 1) at 250 units never detected the player (GetDetected 0) and started no
   combat in 0.2 game hours. So Aggressive coyotes do not wait to be attacked: ours'
   first rule does not hold for them (#1341). Open: why the wild dogs never detected
   the player, placed (not spawned) animals, warnings, allies, onlookers, escorts.
   (killall between sessions left the earlier spawns standing: later sessions had them
   around too.)
   Placed against spawned, 2026-11-06 (o25_coyote, Goodsprings at VERGSCoyote6REF
   0x00157B38, the player in god mode MoveTo'd 700 units off, one run each) [observed]:
   the placed coyote warned for 14.9 s (0.124 game h under a package made at run time,
   FF0011EA, GetCurrentAIPackage 35, procedure 44, drawing and holstering about every
   2 s), then OnStartCombat and bit the player 7 times. A CrCoyote spawned at the same
   spot by the placed one's PlaceAtMe, the placed one disabled, was already in combat
   (package type 5, procedure 13) at the first sample and bit 12 times, with no warning.
   The CrCoyote base has Aggro Radius Behavior on with radius 1,024 and the same two
   packages as the coyotes placed around it [observed: `fnvtool esm decode`], so the
   difference is placed against spawned, not the spot or the record. Why [hypothesis]:
   a PlaceAtMe'd actor has no editor location for the aggro radius to be kept around,
   or it starts alerted. Answers #1341's question.
3. **Damage numbers.** Ours:
   - Power attacks do 3× standing and 2× moving.
   - A damage type uses its resistance (capped at 85 %, after DT) as well as DR. That DR
     cuts laser and plasma hits is answered [observed 2026-11-21: damage-armour.md §4,
     #3218]; whether the resistance itself cuts them is still open.
   - Rippers, Chainsaw and Thermic Lance ignore DT/DR.
   - `fCombatPlayerLimbDamageMult` halves damage to the player's limbs.
   - Difficulty multipliers are 2/1.5/1/0.75/0.5 dealt and the inverse taken, including
     limb, burn and poison damage and your own grenades.
   - Your own explosives hurt you.
   - A second burn or poison hit restarts the timer rather than stacking.
   - Every weapon's Crit Effect applies.
   - Fall damage is 0.0252 × (h − 600)^1.65, with no leg crippling — answered
     2026-10-31 (o6_00): the formula holds within 1 %, but each fall also hurts one leg
     (+700: 24.9 of 100, half the Health lost; +1000 and +1300: crippled). Built since
     (#1129): one random leg loses half the damage (physics.md §6.12).
   To check: OnHit events plus Health and limb values for each case.
   ([combat-rules §10](../spec-notes/combat-rules.md),
   [weapon-data-fields](../spec-notes/weapon-data-fields.md),
   [crippled-limbs](../spec-notes/crippled-limbs.md), [difficulty](../spec-notes/difficulty.md),
   [hit-effects](../spec-notes/hit-effects.md), [physics §6.12](../spec-notes/physics.md))
   Player limbs, 2026-11-01 (o17_air limbs_10mm, nvfairport, mortal player, a trooper's
   10mm Pistol, three runs to the player's death, 38 hits) [observed]: each body hit
   took 16.06 Health; the arm or leg hit lost the same 16.06 of its condition, the torso
   (EnduranceCondition) 6.69 (0.4167 of it); a head hit took 32.12 Health and 20.075
   PerceptionCondition (0.625 of the Health lost); one larger head hit took 76.12
   Health and 47.575 PerceptionCondition. Maximum Health 200 remains [hypothesis]:
   the run did not record base Health. At 200, with DefaultBodyPartData's shares
   (arms and legs 25 %, torso 60 %, head 20 %, ×2 Health) and Limb Dmg Mult 1,
   `damage before the part multiplier × 1 × 0.5 × 100 / (200 × part health % / 100)`
   gives 16.06, 6.69, 20.075 and 47.575 [design: `Game::damage_limb`, `limb_hit`,
   crippled-limbs.md §5]. The part ratios follow from the shares alone: nothing
   cancels the halving, and the existing formula needs no change under that Health
   hypothesis. Pinned by a synthetic 200-Health player test (#1345).
   Still open: **how does the original choose the part on the player's capsule,
   and what condition and Health do blows take?** Record base Health, hit point,
   facing, GetHitLocation and all six conditions around shots and blows. NVForever's
   capsule bands and unchanged melee Health multiplier are [hypothesis]
   (combat-rules.md §12). The lead-pipe trooper never landed a blow in 0.15 h
   [observed: o17_air], so that run gave no melee data.
4. **Hardcore needs and waiting.** Partly answered 2026-10-31 (o7_00, o7_05, owner save
   nvfHardcore, TimeScale 30, about 59 frames a second):
   - The needs rise at our rates [observed]: Dehydration 2 every 0.168 game hours
     (about 11.8 an hour, one point every 10 s of play), Hunger 15 and SleepDeprevation
     7 over 3.2 hours (4.7 and 2.2 an hour, whole points). The "9.6" once written here was
     stale; ours uses 10 s, 25 s and 50 s a point (needs.rs).
   - Dehydration once fell from 27 to 5 between two samples, with nothing done
     [observed, unexplained; one case].
   - `IsHardcore` read 0 in this Hardcore save, so it does not tell whether Hardcore is
     on. SetHardcore 1 on nvfairport still did nothing: the needs stayed 0 for two hours
     (o6_00-01). Explained 2026-11-06 (o26_stim): both need the player as calling
     reference in a script. `IsHardcore` alone does not compile ("requires a calling
     reference") and `SetHardcore 1` alone fails; `player.IsHardcore` reads 1 on
     `nvfHardcore`, and `player.SetHardcore 1` / `0` switches it on `nvfairport` and
     `nvfHardcore`. The needs-over-two-hours run could be redone that way.
   - Beer and Nuka-Cola, drunk by EquipItem, raised Dehydration by no more than the
     normal rate over 11 s (14, 14, 15 and 15, 15, 16) [observed]. The job had `tgm` on
     at the time (o7_00 steps 6 to 101; it went off before the Stimpak) [observed: the
     job file], and their Dehydration +20 / +10 holds only on `IsHardcore == 1`, the
     same condition the Hardcore Stimpak passed [observed: FalloutNV.esm CTDA]. So god
     mode most likely kept it off; ours now does the same in god mode and keeps the
     rise without it (#1167, aid-items §3). Still open: a drink in Hardcore without tgm.
   - Waits and sleeps (owner, 2026-10-31, from all needs at 0, Pip-Boy values)
     [observed]: 3 hours waited give H2O 36, FOD 12, SLP 6; 3 hours slept give H2O 36,
     FOD 12, SLP 2. So 12, 4 and 2 an hour, whole points; sleep did not bring SLP below
     2. In Hardcore, sleep heals neither Health nor crippled limbs; outside it heals
     both, in full (owner). Ours does all of this since #1177 (needs.rs).
   - Still open: Buffout.
   ([aid-items §2, §6, §11](../spec-notes/aid-items.md),
   [wait-menu](../spec-notes/wait-menu.md))
5. **Weapon wear.** Ours:
   - A melee swing wears the weapon only when it lands (once per swing).
   - A record's Damage-to-Weapon Mult replaces 0.2 rather than multiplying it, so the
     Flamer, Incinerator, chainsaw and Rippers now wear 5× faster.
   - NPC weapons wear like the player's.
   To check: weapon health before and after N swings and shots, per weapon.
   ([combat-rules §17–§18](../spec-notes/combat-rules.md))
   Answered in part 2026-11-01 (o14_reach, nvfairport, god-mode player) [observed]: NPC
   melee weapons (knife, sledgehammer, super sledge, lead pipe, bumper sword) stayed at
   GetWeaponHealthPerc 99.9 through 12-16 swings and about 13 landed blows each; ours
   would take a lead pipe to 94.8. Repeated on a mortal player (o15_round, no god mode,
   healed every 20 samples): GetEquippedCurrentHealth 5 stayed exact, lead pipe 49.95
   over 10 swings with blows landing, sledgehammer 149.85 over 12; and NPC guns did not
   wear either: a 10mm Pistol 79.92 over 42 shots, a Hunting Rifle 299.7 over 15. So
   these NPCs' weapons do not wear from their own attacks (Cook-Cook's Incinerator did,
   §17: the override weapons may differ). Open: the player's own swings and shots.
6. **Detection.** Ours:
   - Light level is ambient + sun + lamps; sunlight counts only where unshaded.
   - FOV is 95° each side, and sneak-running counts as running.
   - A lit flashlight doesn't raise your light.
   - Impacts and explosions alert at their own sound levels, and creature footsteps
     count ×2 Loud / ×0.2 Silent.
   - Lying on a bed counts as asleep.
   To check: GetDetected player while walking up to Sunny at noon and at night, with the
   flashlight on, on a porch, and with a missed shot near a sleeper.
   ([detection §8](../design/detection.md))
   Tried 2026-11-01 (o14_detect, nvfairport, a spawned NCR trooper 500/1000/2000 units
   off, player still, noon and midnight), not settled: `player.SetForceSneak 1` from the
   console left IsSneaking 0, and no trooper ever had GetDetected player 1, even at 500
   units at noon (240-280 samples each; package type 1, procedures 11/17). Next time:
   sneak with a key tap (the plugin's tap) and check IsSneaking, and use Sunny or a
   settler of the save rather than a spawned trooper, whose detection may not run while
   it is idle out of combat [hypothesis].
   Retried 2026-11-01 (o16_town, nvftest, player still, GetDetected player on every
   nearby actor every loop) [observed]:
   - `TapControl 8` toggles sneak: IsSneaking went 0 to 1 (and, in a session that began
     sneaking, 1 to 0). Sessions do not reload the save, so sneak carries over.
   - The save's own residents do detect: outdoors at noon, 400 units from Settler 1,
     three residents had GetDetected 1 at times while the player stood; at midnight five
     of eight had it 1 while standing, and after the tap to sneak four of them dropped to
     0 within 14-50 loops, two coming back to 1 for a while later.
   - Indoors at noon 600 units from Sunny's spot (cell 00106185), none of the four
     actors there detected the player standing or sneaking. Sunny's ref was not among
     them [hypothesis: she was elsewhere at noon].
   So the spawned troopers' 0 in o14_detect was the trooper, not the readout. Not yet
   enough for the light, FOV or flashlight rules: next, sneak at noon vs midnight from
   the same spot and distances, by settler, with the flashlight on.
   Added 2026-11-11 (#2121), what scripts read once the target is out of sight. Ours:
   GetIsAlerted 0 in a fight (SetAlert's alone); GetCombatTarget keeps the target it
   fought, unseen, through Combat and Lost, until the target dies or is forgotten, StopCombat,
   a new target or Normal (our Lost lasts until its alarms run out, `Timers::alarm`, 20 s); Lost reads procedure 13
   and package 5; an Alert that never fought reads procedure 36, package 29, no target
   (alert.md §6). To check, on a lone hostile with no other actor near (a bighorner took
   the gecko's attention in Goodsprings, #2121's comment): (a) how long GetCombatTarget
   stays the player with GetDetected player 0 before it reads 0, and whether IsInCombat
   drops with it (73 stretches of under 10 to 293 real s are recorded, 8 that ended unseen
   after 1.1-11.9 real s; no rule yet, #2010); (b) an Alert without combat, a hostile
   given a sound with no sight line at all (npc-behaviour-compare.md §11): its
   GetIsAlerted, procedure, package and combat target.
7. **Casino payouts.**
   - Slots: ours pays back about 1.35 per chip, so max winnings come fast. Check the
     reading of "= BET × N", the cherry rule and the middle row.
   - Blackjack: the dealer doesn't peek, split aces play on, one split only, surrender
     returns half rounded up, comps come at ¼ / ½ / ¾ of max winnings.
   To check: drive many hands and spins by key taps and read caps and chips.
   ([slots](../spec-notes/slots.md), [blackjack](../spec-notes/blackjack.md))
8. **Package timing.**
   - SetPackDuration 60 switched every 10–70 game minutes in the original. What does
     the number count?
   - A schedule duration with no start hour ends only when the goal is met.
   - Sandbox sit and stand times are our own numbers. Tried 2026-11-01 (o20_town
     saloon_sandbox, nvftest, the Prospector Saloon 18-20 h, every 0.5 s) [observed]: only
     three actors were in the scan; Settler 4 (GoodspringsSettler04Ref) sat the whole two
     game hours without standing, the other two never sat. Too few for the spans; needs a
     busier sandbox (the Strip's casino floors from nvfstrip, 6+ h).
   - An unreachable package is set aside for 1 h.
   - MP A gives up her escort half-way in the original.
   - Random NPC chats start only with you within 300 units.
   - Force greets need a clear line, but facing doesn't matter.
   - A woken sleeper talks within 8 s and stays up 30 game-seconds.
   - People during a wait walk an hour's worth each hour.
   To check: observer timelines, which already record packages.
   ([actor-behaviour §3, §5.5–§5.6, §5.9](../design/actor-behaviour.md),
   [owner-decision-captures §7](../spec-notes/owner-decision-captures.md))

## P2: rules met often

9. **Aid, perks and stats.** Ours:
   - Medicine/Food potency is 1 + 2 × the user's skill / 100, and Buffout boosts food
     healing.
   - Addiction: +30 usage per use, chance (usage − 70)/100, and it never wears off.
   - The Hardcore Stimpak heals 5/s for 6 s, and a normal Stimpak heals every limb.
     In Hardcore at Medicine 15 (o7_05, 2026-10-31) [observed]: about 6 Health a second
     for about 6 s, 36 in all; a crippled leg (LeftMobilityCondition 0) stayed at 0
     (o7_00). Ours now rounds the potency-scaled 6.5 down to 6 (#1167, aid-items §4);
     a second Medicine value would tell rounding from another base.
   - A second dose doesn't re-run Start.
   - Only the highest rank of a ranked perk applies, and perks are the player's only.
   - Chemist doubles the Stealth Boy.
   - Skill books give 3 points (4 with Comprehension).
   - Worn bonuses (Vault Jumpsuit Speech +2) are not in actor values.
   - GetBaseActorValue includes ModActorValue — answered 2026-10-31: in the original it
     does not (Strength base 5, ModAV 2: GetBaseAV 5, GetAV 7), and neither does ours
     (`get_base_actor_value`, actor-values.md §1); the note here was stale.
   - Calm Heart's +50 Health goes to every teammate.
   - Four Eyes ignores "PER > 1".
   To check: read actor values in scripts around each use.
   ([aid-items](../spec-notes/aid-items.md), [levelup §9](../spec-notes/levelup.md),
   [skill-books](../spec-notes/skill-books.md))
10. **Karma, reputation and crime.** Ours:
    - Killing Very Evil gives +100, Good −100, and murdering a Neutral −25 — partly
      answered 2026-10-31 (o6_02): −500 victims gave +100 and a +900 one −100 (ours gives
      0 for −500, "Evil"); a 0 victim by KillActor 0. The 2026-10-19 convict (−500 only
      through a levelled template) gave 0, so the reward may read the base's own karma
      (crime-karma.md §4.1). Still open: a real murder of a Neutral.
    - Each owned take or lock is −5.
    - A seen theft is +2 infamy and any seen kill +30, including hostile Powder Gangers.
    - An owner who sees a theft attacks at once.
    - Trespass: two warnings 5 s apart, then attack.
    - Crime fights can't be yielded.
    - Templated NPCs take their template's karma.
    - Unclamped AddReputationExact; 15/50/100 % levels for the eight reputations not
      captured.
    To check: read GetAV Karma (already in the header) and GetReputation around each act.
    ([crime-karma](../spec-notes/crime-karma.md))
11. **Vendors and loot.** Ours:
    - Vendors sell only their flagged categories and their vendor chest, but buy
      anything.
    - Restock follows Ringo's days for everyone.
    - Death items are rolled at death; the template's list wins.
    - Spent casings are player only, doubled by Hand Loader.
    - Respawn-flagged corpses return after 72 h at full health.
    - An encounter zone's level is fixed when its cell enters the loaded area (Easy to
      Boss is 50/75/125/200 %).
    - The three placed weapons that name their ammo give it once.
    To check: barter screenshots, inventory dumps, and timelines over 72 h.
    Respawn tried 2026-11-01 (o23_town respawn_72h, nvftest, TimeScale 2000) [observed]:
    v19 Geckos 1 and 2 (base flagged Respawn) killed by script while unloaded, the player
    in the gas station, then moved to Gecko 3's cell after 66.7 game hours (the wait was
    meant to be 3.2 days, but the start day was stored as a whole 13): both still dead, at
    their death spots. Consistent with a 72 h rule, not a test of it; run again with the
    start stored to the hour (GameDaysPassed * 24 into a Variable) and 80 h. (A first run,
    o20_town, moved the player to the geckos at once by a wrong condition: corpses in the
    loaded cell stayed dead over 80 h.)
    ([vendor-services](../spec-notes/vendor-services.md),
    [death-items](../spec-notes/death-items.md), [encounter-zones](../spec-notes/encounter-zones.md),
    [dead-on-spawn §4](../spec-notes/dead-on-spawn.md))
12. **Dialogue rules.** Ours:
    - Greetings flagged Goodbye are said at you; when an actor starts the conversation
      they hold you for one line.
    - A generic Goodbye is added where the only Goodbye has an end script (100+ lists).
    - Tab leaves only where a Goodbye is offered, or at any topic list (#1595: Julie
      Farkas's has none; how does the original leave her?).
    - A skipped line still runs its end script.
    - Spent Say Once questions vanish.
    - No pause is added between voiced lines (scenes run 0.3–0.4 s short per line).
    - Shared (Info Connection) lines count as the topic's own.
    - Corner and quest messages wait until the conversation ends.
    - Voices without Allow Default Dialog get no generic lines.
    To check: screenshots and video of Ringo, Nash, Red Lucy and Doc Mitchell's stage 40.
    ([dialogue-play](../design/dialogue-play.md), [dialogue](../spec-notes/dialogue.md),
    [voice](../spec-notes/voice.md))
13. **Speeds and turning.** Settled (actors §8.10): people's walk and run with their
    armour and two-handed weapons, and creatures' walks and runs (clip pace × scale ×
    Speed Mult × their skeleton's share). Still open, ours:
    - People turn ≤180°/s walking, 120°/s standing; creatures 300°/s × scale (measured
      on cazadors only); the CREA Turning Speed field is unused. People's combat turns
      measured 2026-11-01 and 2026-11-06 (o25_turn): about 1.1 s after StartCombat, a
      steady 225°/s for 45° and 90° and 275°/s at first for 135° and 180°, slower only once
      it steps forward (actors §8.9, issue #1352); a package turn out of combat is still
      unmeasured.
    - Speed Mult on people: answered 2026-11-01 (o18_air): a trooper ran 305 units/s at
      100 and about 460 at 150, proportional as ours (actors §8.10); Cook-Cook (110, ran
      as if 100) stays the one exception, asked in owner-review.md.
    - People walking with a weapon drawn: built only for the measured case (#2124), a
      person in light armour or clothes with a pistol out, outside combat: its walk /
      fMoveNoWeaponMult, 77.5 (77.5–78.3 seen with a 10mm pistol). Still open, ours: every
      other stance and armour class walks drawn at the holstered pace. A capture that
      tells the three candidates apart (the setting, the stance's walk clip, fMoveBaseSpeed
      77): people walking a package outside combat with a rifle (`2hr` clip 85.26; the
      setting gives 77.5), a knife or fists (`1hm`/`h2h` 77.5), and a pistol in medium
      armour (the setting gives 71.7), each next to a holstered walker. Walking drawn in
      combat (about 100) is not built. Whether a follower walking after its leader takes
      the drawn walk is not known either (ours: a follower in step keeps its leader's pace, the
      drawn walk showing only where that pace is not known or as the slowest it eases to,
      actors §8.10): a person with a
      pistol drawn given a Follow package after a holstered walker (85): does it keep
      step, or drop back at 77.5?
    - Follow distances: stand within distance + 64, run beyond 512. Where a follower
      stops once its leader stops: ours 150 inside the distance, not nearer than 128
      (#2122, from two arrivals: Boone 177 of 330, the pack brahmin about 300 of 450).
      To tell a fixed inset from a share of the distance (or a game setting such as
      fFollowStopZoneMinMult, whose meaning is not documented): two followers with
      different distances (DefaultFollowPlayerFar 512 and a 256 one) on open ground, the
      player walking 2,000 units and standing; the settled gap of each, a few seconds
      after it stops, several runs each, with the follower's speed as it comes in (does
      it slow before it stops?).
    - Aiming or blocking slows you to a walk.
    - Swimming starts at 115 units deep.
    - The step-up is 24 units with a 45° slope limit.
    To check: the fight field set at every loop gives positions and headings; flat open
    ground and a cleared area: the `nvfairport` save (campaign `creature_runs`).
    ([actors §8.9–§8.10](../spec-notes/actors.md), [physics §6.4.2](../spec-notes/physics.md))
14. **Script rules that differ from the wiki.**
    - A call on an empty ref is skipped — partly answered 2026-10-31 (o5_00): in a
      function script run through xNVSE's CallFunction, `rE.GetDisabled` on an unset
      `ref`, alone or as an `if` condition, let the script go on (the next line ran). Ours
      stopped the script (scripts.md §4.2). An object script run by the game goes on too:
      the Nellis artillery trigger's script makes such a call every frame and the
      original's barrage fires (issue #1600). Ours goes on since 2026-11-05. Still open:
      a quest script.
    - Return ends one block only.
    - Scripted items in other inventories don't run.
    - Message boxes don't freeze the player, and MenuMode 1002 is our guess.
    - The GetWeaponAnimType table (Sunny's tutorial depends on it).
    - tgm lifts carry weight and keeps magazines full.
    - Scripted UseWeapon runs before the combat AI; melee UseWeapon walks up to the
      target.
    - Quest items can be stored in containers.
    To check: short-script jobs for each.
    ([script-language](../spec-notes/script-language.md), [scripts §10](../spec-notes/scripts.md),
    [use-weapon](../design/use-weapon.md))
15. **Survival hazards.**
    - Environment radiation: strongest marker wins, and water gives 1 rad/s (2 if
      radioactive).
    - The Fat Man zone fades from 10 to 0 rads/s over 12 s within 616 units.
    - Turbo slows the world to 0.5.
    - A crippled head means a 40 s concussion.
    - A crippled arm makes an NPC drop its weapon.
    - Knockdowns last 3 s.
    - Only BMDT Power Armor needs training.
    - Traps give 2 XP (your note said 5).
    To check: actor values and events.
    ([radiation](../spec-notes/radiation.md), [magic-archetypes](../spec-notes/magic-archetypes.md),
    [unconscious §5.4](../spec-notes/unconscious.md), [traps](../spec-notes/traps.md))
    Tried 2026-11-01 (o14_hazards, nvfairport, god-mode player) [observed]:
    - Crippled arm: a trooper firing a 10mm Pistol kept it drawn and fired on for 40 s
      after `DamageAV RightAttackCondition 200`; both arms at 0 the same. So a scripted
      crippling does not disarm (crippled-limbs §3); a crippling **shot** is still to try.
    - Knockdown length: not captured. `player.PushActorAway <spawned> 60` knocked
      neither a free-standing trooper nor a gecko (GetKnockedState stayed 0); next time
      use a placed explosion on a free-standing spawned actor (blast_knockdown's setup).
      Tried that too (o15_round): `rA.PlaceAtMe GrenadeFragExplosion 1` and
      `WeaponKnockdownExplosion` from a timed script on a free-standing spawned trooper
      gave no OnHit and no GetKnockedState change. Placed by the spawned actor's own
      PlaceAtMe in a timed script, the explosion seems not to go off [hypothesis]; next
      time place it on a save's own actor as blast_knockdown did on Ringo.
      Retried 2026-11-01 (o16_town, nvftest): `RingoRef.PlaceAtMe GrenadeFragExplosion 1`
      (in GSGasStation) and the same on Settler 1 outdoors: no OnHit, no Health change, no
      GetKnockedState 2, so the placed explosion did not go off on the save's own actors
      either. The 2026-10 knockdown_look timeline (PushActorAway 10/40/120 on Ringo) also
      has GetKnockedState 0 throughout. Script-placed explosions and PushActorAway knock
      nobody down in these captures; next: a grenade thrown by an NPC (o12's dynamite
      ganger) at a spawned actor, or the player firing a grenade rifle by TapControl 4
      (the tap fires: see 53).

## P3: smaller or rarer

16. **HUD and menus.**
    - The enemy health bar and red crosshair show only for hostiles, or for anyone hit in
      the last 5 s.
    - Sneak-indicator states. Does a following companion (or an actor on a Follow
      package on the player) seeing the player leave the word at [HIDDEN]? Does a
      neutral (non-hostile) NPC seeing the player give [DETECTED]? (#1159)
    - Compass ticks within PER × 300 (markers: check 227).
    - "How many?" asks only above 5 (barter) or above 1 (crafting).
    - The crafting filter arrows.
    - Wait pace: 1 hour per 0.5 s, and it can be cancelled.
    - Wait refusals: within 2,048 units outdoors, 1,024 indoors.
    - Hacking word lengths by difficulty; a bracket resets attempts 1 in 5.
    - Holotapes keep playing with the Pip-Boy closed.
    - A second ShowTutorialMenu adds a page.
    - Aid effects are shown as totals.
    - The order of misc stats after #16, and what Barter Amount Traded counts.
    - Caravan currencies: caps first, then the least valued notes.
    - Cannibal and Sandman show a choice box.
    Screenshots. ([hud §8–§10](../spec-notes/hud.md), [wait-menu §9](../spec-notes/wait-menu.md),
    [hacking](../spec-notes/hacking.md), [misc-stats](../spec-notes/misc-stats.md))
17. **Weapons and bodies, by video.**
    - Minigun spin-up on every press and after a reload.
    - The Grenade Launcher reloads round by round.
    - Bodies go limp at once, with no death clip.
    - Shove speeds: shots 140, blows 220, bottles about 3 m/s.
    - Corpses: the player walks through them.
    - Small creatures block the player.
    - Bodies over 250 mass can't be pushed.
    - Loose items rest until touched.
    - Pushed NPCs move at half speed and return after 2 s (the `pc_actor_push` capture is
      ready).
    ([animation §9.8](../spec-notes/animation.md), [ragdoll §4](../spec-notes/ragdoll.md),
    [physics §4.3.1, §6.8, §6.10–§6.11](../spec-notes/physics.md))
18. **NPC odds and ends.**
    - Doors stay open behind NPCs.
    - No ledge links.
    - Locked doors are closed to NPCs unless they carry the key.
    - Idle conditions ignored for PlayIdle.
    - Sitting is instant for the player and holsters the weapon.
    - Followers arrive 160 units to your left (captured) and every teammate follows
      through doors.
    - Package-less actors walk home after a fight.
    - Creatures with Can't Open Doors never use load doors (the Searchlight ghouls
      following Edwards).
    - The companion-wheel details.
    - Challenges count every kill.
    - Quest targets indoors are marked at one door.
    ([navmesh §7](../design/navmesh.md), [idles](../spec-notes/idles.md),
    [furniture §5, §7](../spec-notes/furniture.md), [packages §7](../spec-notes/packages.md),
    [challenges](../spec-notes/challenges.md))

19. **The companion perks' highlights.** Ours: Spotter (Boone) and Search and Mark
    (Rex) glow only while the player aims with a weapon out; Spotter marks the actors
    the compass shows red within its range; Search and Mark marks chems, Stimpaks, guns,
    energy weapons and ammunition within 1,500 units; Enhanced Sensors (ED-E) sets the
    compass's actor range to Perception 10's. To check: with each companion hired, a
    frame before and while aiming near hostiles and near dropped items (a Stimpak, a
    pistol, a melee weapon, food) at a few distances; the compass with ED-E at low
    Perception. ([perk-entry-points §13](../spec-notes/perk-entry-points.md))
    Spotter answered 2026-10-31 by the owner (save nvfBoone) [observed]: the red outline
    shows only while aiming (none with the weapon drawn and not aiming) and goes when
    aiming stops; it marks every hostile actor or creature, whether or not it is in
    combat with the player; and it seems to reach every enemy in render distance. Ours
    matches the first two; its reach was the detection reach (5,000 units outdoors),
    shorter than the loaded area, and is now as far as actors are drawn (#1171). The owner's screenshot: distant
    enemies at night look tinted red all over; Boone is not marked. Two more shots show
    the red pulsing (faint, then strong) [observed], as the record says: edge pulse
    amplitude 0.5 at 1 a second, edge fall-off 0.5 (EFSH 0x0015FF60), which ours plays
    through effect-shaders.md's pulse rule. Search and Mark (Rex) and
    Enhanced Sensors (ED-E) are still open.

## Added 2026-10-25 (from the ESM-gap work)

The numbers above stay put so links to them hold; each item here gives its own priority.

20. **NPC grenades** (P1). Ours: the first throw comes 4–10 s after the NPC sees its
    target, then one every 12–25 s, only between 1.5× the blast radius and the throw's
    reach (at most 1,500 units), and each throw takes one from the inventory.
    To check:
    - When do NPCs throw grenades or dynamite: the distance to the target, how often,
      and how soon into the fight?
    - Do they visibly switch to the grenade and back?
    - Does their count drop?
    Setup: Powder Gangers with dynamite at NCRCF, or a Legion squad.
    ([combat-ai §20](../design/combat-ai.md))
    Answered in part 2026-11-01 (o12_grenades, o13_airport gangers_far, nvfairport
    runway, god-mode player) [observed]:
    - Over 90 s at 426-1,595 units, two LvlPowderGangerGunVar carrying 4 and 5 dynamite
      **never threw** and never drew it; neither did three at 76-667 units over 24 s.
    - The third of the far run spawned with dynamite as its drawn weapon: it threw one at
      about 1,225 units within the first second (count 5 to 4), drew a gun at 4 s and
      never threw again.
    So the original does not throw carried grenades on a timer: they are thrown when
    they are the drawn weapon. Ours throws a carried grenade 4-10 s into the fight and
    every 12-25 s after (combat-ai §20): a divergence, not yet changed.
    Frag grenades 2026-11-01 (o18_air, o24_air) [observed]: held, they are thrown every
    1.6-3.6 s from the first second (5-6 throws from 800 and 1,200 units, then fists or
    a knife), none at 300 units (the trooper closes in and strikes); carried beside a
    10mm, none in 27 s at 600 units. Ours waits 4-10 s, then 12-25 s, and never throws
    within 990 units: issue #1351 (combat-ai §20). Still open: the minimum distance
    (between 300 and 800) and whether they switch back to a grenade they still carry.
21. **Packages that send NPCs through player-only load doors** (P2). Some door
    scripts' OnActivate blocks call Activate for the player only. `fnvtool ai reach`
    (routes_through_door) still finds FalloutNV.esm packages routed through them:
    - the Strip gates (VStripGate, refs 0016A14D–0016A150), about 420 actor/package
      pairs, among them the securitrons' VStripSecuritronGuardGate*/Patrol*, the NCR
      MPs' VStripMPBPatrolEmb and the Omerta thugs' VMS33ThugLeavePackage;
    - Gomorrah's elevator doors (0010DA76, 0010D6AC, 0010D6AE, 0010DDB4), 15 pairs, e.g.
      VMS21NeroCasinoTravel and VMS21BigSalHomeTravel.

    Ours lets an NPC through a load door whatever the block did (the block still runs)
    and refuses only at doors that open in place.
    To check: does the original move these NPCs through those doors? Does a Strip
    securitron's gate patrol cross the gate? Do Nero and Big Sal go between their rooms
    and the casino floor through the elevator, or never get there?
    ([doors §5.2](../spec-notes/doors.md))
    Not settled by the `strip` timeline of 2026-10-19 (6 evening hours): no tracked actor
    left the Strip, but none was on a gate-crossing package (the MP B patrolled the
    Lucky 38, VStripMPABPatrol38, not VStripMPBPatrolEmb; the gate securitrons there are
    the Lucky 38's). Needs a campaign tracking the Strip-gate securitrons themselves.
    2026-11-01 (o19_strip, nvfstrip) [observed, doors §5.2]: with the player south of
    both gates for 1.5 game hours MP B stayed north, unloaded and unmoved, on 00151E3E,
    never on VStripMPBPatrolEmb; with the player north he patrolled the north section
    (VStripMPABPatrol38). No actor crossed a gate. Not settled: the patrols hang on
    VStreetFluffNPC iPlayerInArea (52) and bSecuritronPatrol (65), set by the quest's
    triggers, and the observer could not read them (Editor IDs in raw expressions do
    not resolve). Next: read them through `names` (or GetQuestVariable by form) and put
    the player at each area's trigger; the gate securitrons (VLVBStationBot01REF's
    VStripSecuritronGuardGate*) were not tracked.
22. **Combat answer lines and companion complaints** (P2). To check:
    - When a group-mate says NormalToAlert or NormalToCombat, does a nearby mate answer
      with NormalToAlertResponse or StartCombatResponse?
    - Are AttackResponse (BoS: "On it!") and FleeResponse (super mutants: "Coward!")
      said after Attack and Flee lines?
    - Does DeathResponse play without JIP? The wiki says it doesn't.
    - When does a companion say WeaponIneffective or ArmorIneffective? Ours says:
      - WeaponIneffective when DT cut the hit to its floor;
      - ArmorIneffective when one hit took 25 % of health with DT stopping 10 % or less;
      - each at most once a minute.

      Test: Boone with a 9mm against a high-DT target, and Boone in light armour taking
      a heavy hit.
    Read dialogue events in the observer timeline, with audio or subtitles.
    ([combat-dialogue §5](../spec-notes/combat-dialogue.md))
    Tried 2026-11-01 (o21_air, nvfairport runway) [observed, IsTalking only]: three NCR
    troopers set on the god-mode player from 1,000 units; one spoke at 4.1 s, another
    at 12.9 s (no answer within 8 s of the first line); the first was killed (Kill) at
    about 33 s and the survivor next spoke about 10 s later, so no line followed the death
    at once. Screenshots taken while one talked show no subtitle: general subtitles are
    off in the owner's INI and `SetNumericINISetting "bGeneralSubtitles:Interface" 1` did
    not turn them on, so no line was identified. Two super mutants hurt by DamageAV 300 to
    make them flee died instead. Next: turn general subtitles on in FalloutPrefs.ini for
    the run (and back), mutants hurt by a share of their Health.
23. **Actor Effect spells in NPC and creature spell lists** — answered 2026-10-30 (issue
    #916): the original does not apply the type-0 entries (Tunnelers' DT, Ruthless,
    SporeCarrierRegen read IsSpellTarget 0 and base values; the Ability beside them 1), as
    ours. ([area-effects §3](../spec-notes/area-effects.md))
25. **Dialogue at low Intelligence** (P3; few players run Intelligence 3 or below).
    Ours (PR #743), with the player's Intelligence at or below
    iDialogueDummySpeakThisIntOrBelow (3 in FalloutNV.esm):
    - a topic's Dumb Response (DIAL TDUM) replaces the player's line, e.g. "Pizza." for
      "Peace." in Helios One's ARCHIMEDES conversation;
    - INFO lines flagged Low Intelligence (acting note "{Dummy}") replace their High
      Intelligence twins ("{Non-Dummy}").

    To check: `player.setav intelligence 1`, then screenshots of those conversations.
    Check Intelligence 4 too: is the player dim there?
    ([dialogue §2](../spec-notes/dialogue.md))

## Added 2026-10-26 (UI tile tree)

26. **How the original's menu engine reads its files** (P2: every UI mod and MCM).
    Ours (fnv-ui):
    - `&true;` is 1 and `&false;` 0;
    - other constants are kept by name;
    - an end tag closes the innermost element of its name, else the innermost element;
    - a value a script set holds until what its expression reads changes;
    - SetUIFloat makes no trait that is not there;
    - `name:N` counts from 0 among the children of that name.

    To check: GetUIFloat on traits the files set to constants (`HUDMainMenu/locus`,
    `_CrosshairVisible`, a `justify`), DumpTileInfo on QuestAddedMenu and on MCM's
    StartMenu tree, SetUIFloat on a trait with an expression and on a missing one, and
    SetUIFloat against SetUIFloatAlt on a templated list row (the "children tiles" bug
    MCM depends on). ([menu-xml §8](../spec-notes/menu-xml.md))

    Partly answered 2026-11-01 (o11_00, nvftest, HUD in game mode) [observed]:
    - SetUIFloat makes no trait that is not there: `HUDMainMenu/_NvfNewTrait` read
      -999 (GetUIFloat's "not found") before and after `SetUIFloat … 5`, by GetUIFloat
      and by GetUIFloatAlt. As ours.
    - Constants other than `&true;`/`&false;` read as numbers: `&center;` 2
      (`HUDMainMenu/SneakMeter/sneak_nif/justify`) and `&right;` 4
      (`HUDMainMenu/XPMeter/XPAmount/justify`). Ours keeps them by name: check what our
      GetUIFloat gives on a `justify`.
    - The deprecated GetUIFloat and GetUIFloatAlt read the same on eight HUD paths,
      `*:N` paths included (`HitPoints/*:2/brightness` 175, `ActionPoints/*:5/brightness`
      255, `HitPoints/*:0/width` 345, `ActionPoints/*:0/width` 60), so `*:N` counts the
      same way for both (last matching child first, menu-xml §8 item 6).
    Still open: DumpTileInfo, a set trait with an expression, SetUIFloat against
    SetUIFloatAlt on a templated row.

## Added 2026-10-27 (first-person depth)

27. **Does the original's first-person weapon go into walls?** (P3: one look in every
    close fight indoors.) Ours draws the arms and weapon in front of the world whatever
    is nearer (first-person-depth.md §1, [hypothesis] that the original does the same).
    To check: in the Prospector Saloon, a varmint rifle drawn nose to the east wall
    (the `first-person-depth` smoke spot, feet at 469, 244, 3457 facing 80°), a screenshot
    hip-fired and one at the sights; then the same against a standing NPC. If the barrel
    is cut by the wall there, the original draws it with the world.
    ([first-person-depth](../design/first-person-depth.md))

## Added 2026-10-27 (main-quest walkthroughs)

28. **Does a SayTo that finds no line still end with SayToDone?** — answered 2026-10-31
    (o10_00, o10_01, a third party's 100% save nvfSlides at the end slides, VStoryState
    65): **yes**. The narrator's chain (SCPT 0x00161EB4) ends with VEndingConclusion's
    SayToDone starting FadeToCreditsTimer (0x0017A17C). As loaded, it started about 6.7
    minutes in. With VStoryEventKhans set to 9, which none of VEndingGreatKhans' seven
    lines accepts (they need 0 to 4), the chain still reached FadeToCreditsTimer
    [observed]. Every path of the chain passes through the Khans topic, so its SayTo found
    no line and its SayToDone still ran. Ours runs the SayToDone blocks the next frame, which
    matches. ([mq-walkthrough](mq-walkthrough-2026-10-27.md) §2)
29. **Does a line's fixed speaker (INFO ANAM) gate who says it?** — answered for the
    non-ending case 2026-10-31 (owner, from nvf38door): **no, the GetIsID condition
    decides**. After the first meeting with House, the elevator Victor in the penthouse
    (Lucky38VictorElevator, the line's GetIsID) greeted with 0x00165102, "comp you to the
    High-Roller Suite", whose fixed speaker is the entrance Victor (Lucky38VictorEntrance)
    [observed]; later talks to him open with his usual elevator greeting, as a Say
    Once line should. Ours does the same (dialogue.rs `can_say_as`). Still open: lines with a
    fixed speaker and no GetIsID condition (the Dead Money intro and end slides, #965);
    ours lets a script's Say/SayTo say them when no line of the topic passes the check
    (dialogue.md §4, [hypothesis]); finishing Dead Money (and starting it, IntroSlide02)
    and seeing whether Elijah's and Vera's narration plays, and in whose voice, would
    settle it. The
    Boomers' end slide (VEndingBoomers, ANAM NellisPete, GetIsID Narrator) plays in the
    child Pete's voice, not the narrator's (owner, nvfSlides) [observed]; ours says it
    as the narrator with the fixed speaker's file (voice.rs), which matches. Also: the penthouse elevator can only be
    taken through this Victor's dialogue until House is dead (owner).
    ([mq-walkthrough](mq-walkthrough-2026-10-27.md) §2)
30. **Reputation axes outside 0-2** — answered 2026-10-31 (o5_00-02, RepNVNCR, scale 80):
    GetReputation axes 2 and 3 read 0; GetReputationThreshold axis 3 read 0 at every value
    tried (so Benny's "axis 3 >= 3" greeting is never chosen in the original either), and
    axes 0-2 matched ours (`reputation::tiers`) at all ten infamy/fame pairs. SetReputation
    with axis 2 or 3 is a failed call that **stops the script** (the line after it did not
    run); ours does the same since #1127 (reputation.rs `write_axis`). (scripts.md §7.6)
31. **Say Once on a line two actors share** — answered 2026-10-31 (owner, save
    nvf31mainframe, issue #1113): **no repeat**. After the upload and the Mark II upgrade
    the Yes Man mainframe greeted with 0x001164F2 (GREETING position 2424, conditions
    GetIsID YesManMainframe OR VMQTopsYesMan, VMQTops variable 32 < 1), not the Tops
    first-meeting greeting 0x001164F1 (position 2398, Say Once, only the same GetIsID OR)
    that the Tops Yes Man had said [observed]. So Say Once holds per line across speakers
    in the original [hypothesis: one case; the mainframe and the Tops Yes Man may also
    count as one speaker]. Built (#1113): ours holds a Say Once line back from every
    actor once any has said it (dialogue.rs `can_say_as`). (dialogue.md question 13)
32. **An expression that leaves extra values** — answered 2026-10-30 (issue #908): the
    original stops the script at `if GetStage Q N == 1` and runs neither arm, as ours.
    ([scripts §4.2](../spec-notes/scripts.md))

## Added 2026-10-29 (radiation falloff, issue #894; HUD states capture)

33. **Radiation falloff and Rad Resistance** — answered 2026-10-31 (o6_00): linear in
    the 3-D distance (346 units 11.412178, 602 units 9.647502, as ours to 1e-5) and
    GetRadiationLevel × (1 − Rad Resistance ÷ 100) (Rad-X: 6.291849, as ours).
    (radiation.md §2, §4) Still open (P3): overlapping markers (the Camp Searchlight
    crater's three, 1000 units from their point: ours 8.28, summed 20.09) and a Fat Man's
    zone at its centre and 300 units out. MoveTo a marker with offsets moves the player
    where SetPos did not; at two of the four points it left the player in place
    [hypothesis: points it cannot stand the player on].
34. **The corner bars' ticks' brightness** — answered 2026-10-31 (o5_00-01): the full-row
    meters read `brightness` 175 in both bars (`HUDMainMenu/HitPoints/*:2` and
    `ActionPoints/*:2`, each 296 wide and visible; the bars' other visible children 255),
    as the tile HUD writes. Read with a dialogue open over the HUD (the nvfstrip save loads
    into one). (hud.md §14.9)

## Added 2026-10-30 (issues #907, #915)

35. **A condition with a null form parameter** (GetQuestVariable on INFO 0x0014B595-7,
    GetIsID on 0x000FF5BA) — answered 2026-10-30 (issue #907): the original reads 0 for
    both; ours now does without a failed call. Open: other functions given a null form
    in a condition (ours: a failed call, also 0). ([dialogue §4](../spec-notes/dialogue.md))
    Tried 2026-11-01 from a script (o18_air, o24_air): GetItemCount, GetInFaction,
    GetDetected and HasPerk given a null ref variable, each in its own script with a
    marker before and after. Not answered: none of the five scripts compiled in the
    observer, the control (`let fR := player.GetItemCount <Caps001 from names>`) neither,
    so the fault is the `let` of GetItemCount in the runtime compiler, not the null form.
    Next: `set fR to ...`, or a condition-side test (a dialogue line or package whose
    condition names a null form, e.g. found by `fnvtool ai show` / dialogue dumps).
36. **Which of a creature's turn rates applies when** (P3; issue #915). Steady turns are
    TNAM × 1.25, × 3 or × 0.075 °/s on every skeleton tried, whatever the base scale
    (actors.md §8.10 "TNAM"); × 0.075 is aim tracking within about 6°, but × 1.25 and × 3
    both came on big turns towards a target. To check: GetAnimAction, the movement
    speed, IsInCombat and the combat target's distance against the rate, with more turns
    per creature (e.g. a mongrel set on the player again and again from behind), and
    whether a standing turn without combat (a Travel package to a marker behind it)
    takes × 1.25 or × 3. ([actors §8.10](../spec-notes/actors.md))
    Tried 2026-11-01 (o17_air, nvfairport runway, one creature spawned 400 units behind a
    still god-mode player, heading every loop) [observed]:
    - CrCoyote: in combat at once, it turned about 125° onto the player at a steady
      187-189°/s (standing or creeping, GetAnimAction -1, 115 units off), stopping
      exactly on the bearing. Then, attacking (GetAnimAction 2) and moving 85-270 u/s,
      its heading tracked the bearing at 41-42°/s one way and 85-99°/s the other.
    - VCrTier2GeckoMed: in combat at once, it turned at a steady 599-603°/s for about
      0.15 s (the long way round, away from the bearing), then the rate fell smoothly
      from about 560 to 85°/s over the next 0.25 s while it moved off at 50-250 u/s.
    The coyote's and gecko's TNAM were not read here, so the ×1.25 / ×3 question is not
    settled; next: read their TNAM (fnvtool), set the ratio, and add a non-combat turn.

## Added 2026-10-30 (add-on walkthroughs, issue #918)

37. **Is a line that another line names as its follow-up (TCFU) said when its topic is
    chosen?** (P2: Old World Blues' Sink.) Six topics of the ten masters store a
    follow-up line first and another line after it, e.g. the Think Tank's "Topic001"
    (`NVDLC03DialogueKleinNVDLC03DoctorOLVLTopic001`): 0x01013555 (the five-line
    technologies chain that Topic000's line 0x0101432E leads into) and 0x01013559
    ("FINE. SO... YES."), whose choice Topic002 is the only way to the Sink's chip.
    Ours now passes over follow-up lines of a topic the player chooses while another line
    of it can be said (greetings and SayTo not: [dialogue §3.4](../spec-notes/dialogue.md))
    [hypothesis]. To check: in the first
    Think Tank talk, after agreeing to help, pick the technologies question and see which
    line answers, and whether "a place to stay" is offered next. The others: Doc
    Mitchell's VCG01DocMitchellTopic076 (never chosen), Mr. House's MrHouseTopic154,
    VMS46 Diane's VRRCDianeTopic003, the Think Tank's OLVLTopic006 (Dala's 0x01011333 now
    leads into Klein's 0x01011335) and Dr. O's OLVLTopic036.

38. **Does a block whose parameter is a form list run for any member?** (P3: Old World
    Blues' force fields.) `Begin OnHitWith NVDLC03SonicWeaponList` (SCPT 0x01016AB4,
    0x01016953, 0x0101571D) names a FLST of the five Sonic Emitters; ours runs the block
    when the hitting weapon is in the list (game.rs `block_accepts`) [hypothesis, fit to
    the data]. To check: with the EM pulse module downloaded (X-8), shoot a Big MT force
    field with the Sonic Emitter: Sonic Emitter Upgrade completes and the field drops.

39. **Can a Goodbye line's end script start a new conversation?** (P2: 8 end scripts, 4
    of them of Goodbye lines, e.g. Anders' thanks in VMS46, INFO 0x00142A9C.) Ours now
    holds the StartConversation and starts it once the line has closed the conversation
    [design, issue #1107]. In the original, after the line, does the new speaker
    force-greet? (mq-walkthrough §9 stall 1.)
40. **Does re-displaying a completed objective un-complete it?** — answered 2026-10-31
    (o5_00, issue #1108): no. VMS19 65 and VFSAtomicPimp 34: displayed, completed, then
    SetObjectiveDisplayed 1 again: still completed. Only showing a **hidden** objective
    clears the flag (displayed 0, then 1: not completed), which is what the VMS16 capture
    of layer5-captures.md §5.2 saw. Ours clears it on every display, so G.I. Blues can
    stall. (scripts.md §5)
41. **Does ResetQuest keep a SetQuestDelay?** — answered 2026-10-31 (o41 on nvftest, issue
    #1109): yes. VMQ03aTimer at SetQuestDelay 2, ResetQuest, StartQuest: its timer still
    dropped every 2.0 s (DATA delay 0.5 s). ResetQuest also zeroed the variables and
    stopped the running quest (GetQuestRunning 0). (layer5-captures.md §5.7)
42. **Does a stage result script's reference-less RemoveSpell act on the player?** —
    answered 2026-10-31 (o5_03 on nvfairport, issue #1110): yes. Addicted to Jet (10 doses;
    IsSpellTarget WithdrawalJet 1 over a game hour), `SetStage VDoctors 20` made it 0 within
    10 frames. The doctors' cure works in the original; ours too since #1110.
    (script-language.md §1.2) Traps for the next capture: the first addiction opens a
    message box (menu 1001) and the nvfstrip save loads into a dialogue (menu 1009); both
    pause the game, and effects do not change while it is paused.
43. **Does a newly started quest's script run before its first delay?** (P2.) Old World
    Blues: wake in the Think Tank and talk to Dr. Klein at once. Do the Big Empty's random
    encounters (NVDLC03Enc 10) start? (§10 stall 2.) — answered 2026-11-26 (o34_43a/b/c,
    save nvftest, #1113): **no**. After a console `StartQuest NVDLC03MQ01` its script first
    ran 98-99 game loops later (about 1.65 s), in 4 launches of 4, counted from the start;
    started and set to 100 on the next loop, the stopped quest's script never ran in 600
    loops, so Act1XP and NVDLC03Enc 10 stayed unset [observed]. The older chains' stall is
    the original's too: the data's own. Ours waits the quest's delay (5 s for 0) before the
    first run; the original's first wait differs from quest to quest (VMS40Timer about
    3.9 s, scripts.md "Script delay"), so it stays unmodelled.
44. **What does `RemoveItem <item> 0` do?** — answered 2026-10-31 (o5_00, issue #1113):
    nothing, and the script goes on (12 Stimpaks and 21 caps stayed; the next line ran).
    Built (#1113): ours returns without removing anything (functions/items.rs
    `remove_item`).
45. **Does a second Nelson explorer stop the player right after the first?** (P3; §10
    stall 4.) — answered 2026-11-26 (o34_45_nelson, o34_45b_probe, o34_45c_nelson, save
    nvftest, VDeadSea 0, #1113): **only when the player walks within 512 units of him**,
    and that conversation closes at once with nothing said (his only greeting, Say Once,
    was said by explorer 01). With the player 1,224 units from explorer 01 nobody came in
    200 s, though nvNelsonExplorerBarkSCRIPT calls `StartConversation Player` and `evp`
    on its first GameMode; inside 512 units (nvNelsonExplorerBarkPackage, a Dialogue
    package to the player) he stopped the player within 17 loops [observed]. Ours walks
    the speaker up from any distance (`Game::walk_up_greet`, as the GECK wiki's
    StartConversation says), so the far explorers' call differs: #4919.
46. **Does the HELIOS One west-terminal password go to the player from Fantastic's line?**
    (P3; §10 stall 5.) — answered 2026-11-26 (o34_46_fantastic, save nvftest, VMS03 10,
    #1113): **yes**. The line is GOODBYE 0x000E360D (Goodbye, Say Once; GetIsID Fantastic,
    VMS03 >= 10); its end script `AddNote VMS03HeliosTerminalWestPassword` names no
    reference, and GetHasNote read 0 before it and 1 after, with or without `player.`
    [observed]. Ours does the same (`functions/player.rs` `add_note` files the note with
    the player whoever runs it). The walkthrough's stall was its own: it closed the talk
    without the goodbye and counted the note with GetItemCount; mq-that-lucky-old-sun now
    says goodbye and reads GetHasNote.
47. **Does a talking activator's scripted `Activate player` start its conversation?** —
    answered 2026-10-31 (o9_02, owner save nvfTopsAmbush: Benny's own invitation taken,
    VMQTops 20, BennyAmbush 2, BennyGoToPresidential 0): **yes**. The trigger's script
    (SCPT 0x0006919D) sets bElevatorDisabled and a 15 s timer on entry; once it runs out
    and the player is within 250 units of the intercom (0x00129556), it calls
    `SetTalkingActivatorActor Benny` and `<intercom>.Activate player`. With the player
    walked through the trigger and then standing 144 units from the intercom, the
    dialogue menu (1009) opened by itself about 14 s after the trigger (GameHour 4.80 to
    4.91 at TimeScale 30) [observed]. Ours opens nothing (#1111). An earlier try with a
    Speech-persuaded Benny (nvf47suite) had no intercom at all: Benny waits in the suite
    for the player instead (owner). (§11 stall 1.)
48. **Can Oliver be talked into retreating on House's path?** (P3.) After "You will because
    the alternative is worse", which reply does he give (Topic065 has two lines with the same
    conditions, 0x00159932 and 0x001596BD)? (§11 stall 2.) — answered 2026-11-26
    (o34_48_oliver, save nvftest, #1113): **0x00159932, three times of three** (the topic
    started by script; neither line is Random) [observed]. As ours: the first line whose
    conditions pass wins, so 0x001596BD's retreat lines are unreachable from Topic065 in
    the original too, the data's own. Not checked: reaching the topic from the earlier
    line by hand, on House's path.
49. **Does attacking Daniel during Flight from Zion start Chaos in Zion at once?** (P3; a hit
    starts no AI combat headless.) Also: attacked at the Northern Passage, does Jed go down
    unconscious, and does Happy Trails fail? (§11 stall 3.)
50. **Does walking behind Stone on his night route set VMS31 60 as soon as he is in view?**
    (P3; no line of sight headless.) (§11 stall 3.)
51. **Does the script language short-circuit `&&`?** — answered 2026-10-31 (o5_01, issue
    #1112): no. `if 0 && player.ModAV Variable09 10`, `if 1 || …`, `if 0 == 1 && … == 0`,
    `if 0 && 1 && …` and four more forms all ran the ModAV (+10 each time). EDEScript's
    empty-reference call each frame is the original's too; check 14 asks whether it stops
    the script there. (script-language.md §4)

## Added 2026-10-30 (the enemy bar's show rule, issue #935)

52. **How long a neutral's enemy bar lingers, and what ends it** (P3: every hurt actor
    looked at.) The hurt settler's bar was still up 16-18 s after the player turned away
    in hud_enemy5 a1-a3a, where the watch ended; a restrained hostile gecko's went after
    about 5.5 s (b2). Ours lingers 5.5 s for every actor (`hud_target::LINGER_SECONDS`):
    a known divergence for a neutral. To check: hurt a neutral settler by script (DamageAV
    Health), turn the player onto him from 350 units (not the activation target, no
    prompt), then 180 degrees away, and read `EnemyHealth`'s three children every 50 ms
    for 40-60 s with no MoveTo; then the same on a hurt restrained gecko, and on each
    after showing the bar 1 s against 10 s before the turn (hostility or the time shown
    are the candidates). ([hud §8](../spec-notes/hud.md))
    Answered in part 2026-11-01 (o12_enemybar s2, o13_town bar_end; nvftest, settler 1
    hurt by DamageAV 20, 300-350 units, no prompt) [observed]: once the bar was up (all
    three children visible) it **stayed up to the end of each session: 46 s, then 176 s
    after the player turned 180 degrees away**, while the settler walked about out of
    sight, and in the second run for 87 s after he was healed to full (RestoreAV, Health
    45 of 45). So for a hurt neutral it does not time out, and full Health does not end it
    either; the likeliest end is another actor taking the bar (not captured). Ours drops it
    after 5.5 s (`hud_target::LINGER_SECONDS`); left as is (owner, 2026-11-01). The bar
    took 10-15 s of facing to appear in both runs (the aim's pitch, most likely).
    Second actor tried 2026-11-01 (o20_town, o23_town bar_handoff; nvftest, Settler 1
    hurt 20 at 350 units, a spawned NCR trooper hurt 40 on the other side) [observed]:
    re-faced on the settler every 5 s, the bar showed only once, 39 s in, for 6 s (meter
    width 269 of 500), and went while the player was still being turned onto him as he
    walked; faced on the hurt trooper for 30 s and then turned away, it never showed (the
    first run, faced every 5 s for 20 s each, showed none at all). So the hand-over was not
    seen. The bar's appearing hangs on the crosshair being on the actor, which script
    facing at 350 units hits only now and then. Next: a standing target (a spawned actor
    with `SetRestrained 1`) at 200 units, aim pitch set from the height difference.
53. **Does the player's own hit show the enemy bar, and for how long?** (P2: every fight
    the player starts.) No capture had the weapon out (IsWeaponOut read 0 after the
    script's Ready taps), so no shot hit. Ours keeps its earlier guess: 5 s after the
    player hits an actor it has the bar, aimed at or not and prompt or not, while it is
    hurt (`hud_target::HIT_SECONDS`) [hypothesis]. To check: with a weapon drawn (EquipItem
    and a Ready tap confirmed by IsWeaponOut 1), shoot a neutral settler once from 350
    units with the crosshair on him, turn away at once, and read the three children every
    50 ms for 20 s; then the same with the crosshair off him at the moment of the hit (a
    shot from a script, or a grenade), and at 120 units where he is the activation target.
    ([hud §8](../spec-notes/hud.md))
    Tried 2026-11-01 (o16_town hit53_settler, nvftest, god mode) [observed]: the weapon
    can be drawn by script: `player.EquipItem Weap10mmPistol 0 1` then `TapControl 7`
    gave IsWeaponOut 1 within 17 loops; `TapControl 4` then fired one shot. But the
    shot, after a scripted SetAngle Z toward Settler 1 (GetHeadingAngle) and X 0, hit
    another actor (OnHit target 0017B64E, not in the nearby scan), not him: no settler's
    Health changed. `EnemyHealth`'s own `visible` went from 8 to 1 at the shot and its
    three children stayed 0, so no bar showed. Not settled; next: aim by GetHeadingAngle
    with pitch from the height difference (SetAngle X), the settler alone, and confirm
    the hit by his Health before reading the bar.

## Added 2026-10-30 (UI tile tree T8: the pointer and keys)

54. **The pointer on menu tiles** (P2: every menu drawn from its file, MCM's page, 16
    plugins of the modlist scan). Ours (fnv-ui `input`):
    - where two targets overlap, the highest summed `depth` takes the pointer (a tie to
      the one drawn later), and a menu opened later is over an earlier one;
    - `clicked` is 1 for the frame of the press, then 0 (the tutorial), not a count (the
      Clicked page);
    - `mouseover` is written on the tile taking the event only, not on the tiles above;
    - GetActiveUIComponentName/FullName/ID with no menu open give "", "" and -1.

    To check: GetActiveUIComponentName over an overlap of two tiles injected with
    InjectUIComponent at different depths; two injected tiles, one copying `clicked` and
    one adding it to itself, clicked twice with GetUIFloat after each click; GetUIFloat on
    a text_box button's hotrect, text and parent `mouseover` with the cursor on the text;
    the three functions in game mode and over a target without an `<id>`.
    ([menu-xml §3.2, §8 items 7-10](../spec-notes/menu-xml.md))

    Partly answered 2026-11-01 (o11_00, nvftest): in game mode with no menu open,
    GetActiveUIComponentName and GetActiveUIComponentFullName gave "" and
    GetActiveUIComponentID -1 [observed], as ours. The rest (overlap, `clicked`,
    `mouseover`, a target without an `<id>`) is still open.

## Added 2026-10-31 (the Pip-Boy Glove and outfit gloves, issue #1067)

55. **Which left hand the player shows in an outfit with gloves** (P3: the player's own
    hand in every outfit with glove addons). NVForever lets the Pip-Boy Glove give way to
    an outfit's glove addon (leather, metal, raider, ranger armour, power armour) and a
    vault suit's bare-hand addon give way to the Pip-Boy Glove. To check: the player in
    Leather Armor and in T-45d Power Armor, first and third person: is the left hand the
    outfit's glove or the Pip-Boy Glove's hand? ([actors §2.3](../spec-notes/actors.md))

## Added 2026-10-31 (radio loudness, issue #1077)

56. **Pip-Boy radio music against the DJ** (owner: the music was too quiet). Ours
    (`engine/nvf-play/src/radio.rs` `SONG_GAIN`): a song a station's line plays is at full
    level, its SOUN static attenuation left out. Most radio songs' records take 12.33 dB
    off, four take none, and all the song files are about equally loud (−30 to −34 dB
    mean), so applying it would make those four 12 dB louder than the rest [hypothesis].
    The files themselves still sit about 13 dB under the voice files (RMS as played 0.019
    against 0.080).

    To check: on Radio New Vegas, record the original's output across a song by Mr. New
    Vegas's line and across MUS_StreetsOfNewReno (0 dB) and MUS_Big_Iron (12.33 dB), at
    100% master and Radio volume; compare the songs' loudness with each other and with
    the DJ line.
    ([audio.md](../spec-notes/audio.md); radio.md)

## Added 2026-10-31 (idles that hold their actor in place, issue #1072)

57. **Whether an idle with leg tracks holds its actor in place** (P2: every scripted
    idle on a patrol or walk). NVForever holds an NPC still while an idle asked for by a
    script, a package or a dialogue line shows whose leg tracks outrank the walk clip's
    (not Upper Body), and breaks out of it (fading it out) as soon as its AI has
    somewhere to go (owner 2026-10-31). To check: Nipton, Oliver Swanick (vNiptonSurvivorREF 0x001349B6)
    on his celebrate patrol: at each marker his patrol idle time is 1 s and the marker's
    script plays `LooseCheeringBothArmsUp` (4.7 s, 1-2 passes). Sample his position
    every game loop from the marker: does he stand until the cheer ends, walk off after
    1 s with the cheer cut short, or slide with it playing? And `IsIdlePlaying` at the
    moment he sets off. ([idles §15](../spec-notes/idles.md))

## Added 2026-10-31 (coyotes' warning and charge, issue #1073)

58. **What a warning coyote plays, and how it bites on the run** (P3: every coyote and
    dog fight). NVForever plays the dog folder's `h2hequip.kf` then holds `h2haim.kf`
    while a coyote warns the player, puts it away when it attacks, and shows a blow
    begun on the run on the head and neck only, over the run. To check: VERGSCoyote6REF
    (0x00157B38) at its warning spot, the player 600-700 units off: `GetAnimAction` and
    `IsWeaponOut` sampled every game loop through its two warnings and the charge; does
    it stand with its head down (a frame), and do its legs keep running through a blow
    begun on the run or does it stop to bite? ([actors §8.10](../spec-notes/actors.md))

    Tried 2026-11-01 (o12_coyote at 650 units; o13_coyote, a walk from 1,500 that did not
    move the player): the coyote neither warned nor charged; it was in combat for a few
    loops, then OnCombatEnd, GetAnimAction 1 for about 0.9 s [observed]. Next try: place
    the player by MoveTo steps from 1,200 to 300 units instead of a walk.
## Added 2026-10-31 (robots' talking lights, issue #1081)

59. **What sets a robot's `Talking` sequence while it speaks** (P3: Victor, Yes Man, Mr.
    House's screen, protectrons). NVForever plays `talking.kf` while a line is said with
    the sequence's time taken from the line's `.lip` (the widest mouth shape now, 0 to 1
    of the 0.17 s), so Victor's chest light (`##VoiceBox_talk:0`) flickers with the
    words and goes dark between lines (lipsync.md §6.1). To check: Goodsprings, talk to
    Victor (VictorREF 0x00103DFE) and record his chest light frame by frame over one
    line; compare its brightness with the `.lip`'s mouth weights and with the sound's
    loudness over the same frames (a pause between words, a long vowel). Does it go
    dark in pauses, follow the loudness or the mouth shapes, or loop by itself? Same on
    Mr. House's screen (`##TalkButtons:0`). ([lipsync §6.1](../spec-notes/lipsync.md))

## Added 2026-10-31 (Observe Combat spectators running away, issue #960)

60. **How close a fight comes before a spectator runs, and what it does then** (P3: the
    806 packages with Observe Combat [observed: esm-audit-2026-10-25.md]; the wiki says
    only "run away if they get too close"). NVForever runs a spectator from the nearest
    fighter within 600 units [design: stand-in], by the combat flee's spot search, out
    of a combat state, and brings it back to its package once no fighter is within
    1,500 (packages.md §7.6b). To check: Goodsprings by day, Easy Pete (EasyPeteRef) on
    his own package, the player in god mode starting a fight with a placed cazador
    (`PlaceAtMe NVCrCazador`) at about 1,200 units from Pete, then backing towards Pete
    in steps of about 100 units every few seconds. Sample every game loop: the distance
    from Pete to the player and to the cazador, Pete's `GetCurrentAIProcedure`,
    `GetCurrentAIPackage`, combat state (`IsInCombat`, `GetCombatTarget`) and
    `IsWeaponOut`, and his position. At what distance does he set off, from whom (the
    player or the cazador), how far does he go (a door, cover, straight away), does he
    turn to watch from there, and does he go back to his package when the fight ends or
    only later? ([packages §7.6b](../spec-notes/packages.md))

    Tried 2026-11-01 (o12_spectator, o13_town pete): not settled. In the second run Easy
    Pete had **no package** at noon (GetCurrentPackage none, procedure 0) and did nothing
    with a cazador 354 units off; in the first the cazador spawned beside him. Next try:
    an NPC read to be on an Observe Combat package first.
## Added 2026-10-31 (melee knockback, issue #1070)

61. **How far a melee blow pushes a living actor, and for how long** (P2: every melee
    fight). NVForever reads the Melee Knockback Formula's force (fKnockbackDamageMult 10,
    fKnockbackDamageBase 50, fKnockbackAgilMult -0.008, fKnockbackAgilBase 1, at most
    fKnockbackForceMax 512) as a speed in units/s that falls evenly to rest over
    fKnockbackTime (1 s), so a blow pushes force / 2 units: a Sledgehammer blow of 14
    damage on Agility 5 pushes 57 units [design]. To check: in Goodsprings, hit a
    standing settler (Agility known) with a Sledgehammer, normal and power attack, from
    the front; sample its position every game loop for 1.5 s after the hit: the distance,
    how long it moves, and whether the speed falls off. The same with a deathclaw's
    swipe on the player (god mode). ([combat-rules, Knockback](../spec-notes/combat-rules.md))
    Deathclaw on the player 2026-11-01 (o12_reach, o13_airport deathclaw_long,
    nvfairport, god mode, 28 blows) [observed]: each pushed the standing player **15-27
    units** sideways (15 most often), still moving 1.0-1.8 s after the blow, 1-6 units up,
    never knocked down (GetKnockedState 0); one blow in the first run tossed the player
    65 units up (a one-off). God mode may change the damage the formula reads. The
    settler and Sledgehammer cases are still open.
62. **Does AddReputation with axis 2 or 3 stop the script, as SetReputation does?** —
    answered 2026-11-01 (o11_00, o11_05 on nvftest): **yes**, and RemoveReputation and
    AddReputationExact too: the call changes nothing and the script's next line does not
    run, in a function script and in VMS20 stage 100's result script (the betrayal
    entry: NCR fame +1 from the line before, Khans unchanged, StopQuest not run). The
    stage's Complete Quest flag still completes the quest. Ours now stops the same way
    (reputation.rs `write_axis`). (scripts.md §7.6, issue #1127)

## Added 2026-10-31 (UI tile tree T15a: the message box and the prompt from their files)

63. **What `screen()` reports, and `resolutionconverter` — Answered 2026-10-31**.
    Width is 960 × window width/height, height 960 and `resolutionconverter` 960 /
    window height [observed: hud_menus capture 2026-10-31, three window sizes:
    1280 x 720, 1920 x 1080 and 1280 x 1024]. The capture read `screen()` through
    copied traits. NVForever keeps those units gated on a switched-on T15 menu
    (`MessageMenu` or `QuantityMenu`); applying them unconditionally is a follow-up
    [design: preserve default play].
    ([menu-xml §4.2](../spec-notes/menu-xml.md), [message-menu §5a](../spec-notes/message-menu.md))
64. **The message box's and the prompt's code-set traits** (P2: every message box and
    stack prompt drawn from its file). Ours (`tiles::menus`): the list as wide as the
    widest button's text and 80, each row its text's height and 20, the box's width by
    its text, title and list, the empty title 0 high, the backgrounds' alpha 237, the
    meter's `user3`/`user4` 1 and 10 [hypothesis or design each].
    To check: with a message box of a title, two lines and three buttons open, GetUIFloat
    `MessageMenu/NOGLOW_BRANCH/MM_MainRect/width`, `…/MM_ButtonList/width`, `height`,
    `_highlight_y`, `_selected_height`, `…/MM_Title/height` (and again for a box without
    a title), `…/MM_Background/alpha`, the rows' `_y` and `height`
    (`…/MM_ButtonList/MM_Button:0` …); with the prompt open over a stack of 20,
    `QuantityMenu/NOGLOW_BRANCH/QM_MainRect/QM_AmountMeter/user0` … `user4`, and a
    screenshot of each at 1280 x 720.
    ([message-menu §5a](../spec-notes/message-menu.md), [barter-menu §7](../spec-notes/barter-menu.md))

    Message box answered 2026-11-01 (o11_01, nvftest, GetUIFloatAlt 30 frames after
    opening; the window's size as the owner's INI has it) [observed]; the prompt is
    still open:
    - `MessageBoxEx "<two lines>|Alpha|Beta|Gamma"` (no title): box (`MM_MainRect`)
      396 x 306, text 316 x 67, title 14 x 38 (the empty title keeps its room, as the
      2026-10-31 capture read), list 155 x 156, rows 118 wide and 52 high at `_y` 0, 52,
      104, `MM_Button:0` the **last** row (104), `MM_Button:3` absent (-999).
    - `ShowMessage CG03GoatQuestion01` (title "G.O.A.T. Question 1", a long text, four
      long buttons): box 700 x 603 (the file's `_MaxMenuWidth`), text 618 x 137, title
      264 x 38, list 620 x 383 (the box less 80), rows 583 wide (the list less 37),
      wrapped rows 87, 157, 52 and 87 high: one line 52 and each further line 35 more,
      as message-menu §5a's line pitch gives.
    - Both: `MM_Background` is the box × 1.1 wide and × 1.3 high (435.6 x 397.8;
      770 x 783.9) with alpha 193.8 (ours 237, already listed in §5a);
      `_selected_height` 52 and `lb_highlight_box` 51 high; `_highlight_y` 0 and 87 (a
      row was chosen by then, likely by the pointer at the window's middle, so this does
      not contradict §5a's "no row chosen when a box opens").
    So ours' row height (text + 20 = 52 for one line) and list = box − 80 hold; the
    background's size and alpha are to compare with ours.

## Added 2026-10-31 (follow-ups and choices, issue #964)

65. **What a line with both follow-ups and choices offers after its follow-up** (P2: 15
    lines of FalloutNV.esm, e.g. INFO 0x00158A3D of Topic056, whose choice Topic068 is
    otherwise never offered). NVForever passes over a follow-up with the Goodbye flag
    when the line has choices and offers the choices (Topic067, whose line is that
    Goodbye follow-up, and Topic068); other follow-ups are said first [design, issue
    #964]. To check: at Yes Man's final report (VStoryEventSecuritronsFort 1) pick
    Topic056 and record whether the conversation ends after the line (Topic067's
    goodbye said with no pick) or Topic067/068 are offered. A second sample: Arcade's
    Topic065 (0x0016338D, non-Goodbye Say Once follow-ups): is 0x00163380 said before
    the choices? ([dialogue §3.4](../spec-notes/dialogue.md))

66. **Does a quest's ResetQuest on itself stop it?** (P2, issue #1232.) o41 reset VMQ03aTimer
    from a job script. The Emergency Radio (VEFR01NCRGood2EmergencyRadio) calls ResetQuest
    on itself in its MenuMode and then sets iSpawnSupplyDelay; if that stopped it, the
    drop would not come. Use the radio (button 1) and read GetQuestRunning right after,
    and whether the drop arrives. Ours keeps a self-reset running. (scripts.md §5)

## Added 2026-10-31 (the note notice, issue #1163)

67. **Does the original name a note in the corner when a script gives it?** (P3: every
    AddNote and AddItem of a note in quest scripts.) NVForever shows its own corner line,
    "Note: <the note's name>", for every note the player gets the first time: picked up,
    taken from a container, AddItem, AddNote, a terminal's Add Note item
    [hypothesis for the script-given ones]. To check: with a note the player lacks,
    `AddNote` it from the console, then `player.AddItem` another; screenshot the top left
    after each, and note any sound. A picked-up note in the same way for comparison.
    (issue #1163, `Game::file_note`)

## Added 2026-11-01 (actors at their place on arrival, issue #1208)

68. **Where the destination's actors are on arrival** (P2, issue #1208). NVForever puts
    every actor not in the loaded area before a fast travel or a load door at the end of
    its package's walk (on its furniture, seated) behind the loading screen, and moves
    nobody that was loaded [hypothesis, packages.md §8]. To check, with nvf_capture
    recording position, current package and GetSitting just before and just after:
    (a) Goodsprings → Novac at 5am (Boone: at the dinosaur exactly, part-way, or within
    the package's radius?); (b) a fast travel to a marker a few metres away inside the
    loaded area (does anyone move?); (c) entering the Mojave Outpost (Cass) and the
    Prospector Saloon (the settler at the bar): GetSitting 3 on the first frame?

## Added 2026-11-01 (MCM's shorthands, issue #976)

69. **Where MCM's shorthands point** (P3, issue #976; menu-xml.md §8 item 11, §10).
    NVForever reads `GetMCMFloat child grandchild` as the `MCM` rect's children counted
    from 1 in file order, 0 stopping at the level above [hypothesis, from MCM's files],
    while its `*:N` counts from the last child [observed on the HUD]. With MCM and one
    MCM mod installed, its page open and the pointer on the first row: GetUIFloat
    `StartMenu/MCM/*:1/*:1/_optionID`, GetUIFloat
    `StartMenu/MCM/MCM_Options/Option1/_optionID`, `GetMCMFloat 1 1 "_optionID"` and
    `GetMCMFloat 1 0 "_optionID"`.

## Added 2026-11-01 (Speech Successes and Failures, issue #967)

70. **Do New Vegas skill checks count Speech Successes / Speech Failures** (P3, issue
    #967). NVForever counts stat 12 when the player picks a Speech check's pass line
    and stat 31 for its fail line, and nothing for checks of other skills or perks
    [hypothesis: by the stats' names; `Game::choose`, misc-stats.md §3]. To check, with
    nvf_capture reading `GetPCMiscStat "Speech Successes"` and `"Speech Failures"` just
    before and after each choice: (a) one Speech check passed and one failed (Primm or
    Goodsprings, e.g. a [Speech 35] line with Speech above and below it); (b) one
    Barter or Science check passed and failed (do they move either stat?); (c) one perk
    line (e.g. [Lady Killer]). If (a) moves nothing, the count goes; if (b) moves them,
    other skills count too.

## Added 2026-11-01 (combat groups, issue #972)

71. **Does a fighter keep a hidden target while a member of its group detects it?** (P2,
    issue #972). NVForever keeps a target neither lost nor forgotten while a comrade in
    Combat with the same target detects it [hypothesis, combat-groups.md §3, from
    observer-jip-audit §3.3: five Fiends fought a hidden player for half an hour while
    often only 1 or 2 detected it]. To check, with nvf_capture recording IsInCombat,
    GetCombatTarget and GetDetected/GetDetectionValue against the player for each Fiend
    at Cook-Cook's camp: (a) the player hidden from all of them for over 15 s (does every
    Fiend drop out of combat, and when?); (b) one Fiend kept detecting the player (a
    TestDetection placement, or the player in its sight only): do the others that do not
    detect it stay in combat past 15 s? Also: does an alarmed townsperson that has not
    seen the first Ganger (GSPG03Ref) still name it in OnStartCombat (combat-groups.md §2)?

## Added 2026-11-01 (a conversation with nothing left to offer, issue #1113)

72. **Does a conversation close when the actor has no topic left?** (P3; mq-walkthrough §10
    stall 4.) Cold, Cold Heart: tell Sergeant Kilborn at the Mojave Outpost about Nipton
    (vNiptonVulpesvMojaveNCRTrooperTopic003, INFO 0x00138E1A: no Goodbye flag, no choices;
    its end script sets vNiptonVulpes 100, which stops the quest). Every line Kilborn has
    as a fixed speaker is vNiptonVulpes' [observed: `fnvtool dialogue lines --speaker
    vMOSgtKilborn`], and talked to again he greets with 0x001754C6 and offers nothing.
    NVForever closes the conversation when the line ends (dialogue.md "When a line ends":
    with nothing to offer the conversation ends [hypothesis]). Does the original close it,
    or does the menu stay open with nothing but a way out? — answered 2026-11-26
    (o34_72_kilborn, save nvftest, #1113): **it closes by itself.** Started on Topic003 by
    script, Kilborn said both responses; by 22 s the quest was at 100 and stopped and the
    menu showed his face with no topics and no way out; by 26 s the game was back (menu 0),
    with no input [observed]. As ours (dialogue.md "When a line ends").

## Added 2026-11-01 (the pause column MCM waits for, issue #976 T11)

73. **The pause column's code-written traits** (P3, issue #976 T11; ui-tile-tree-tasks.md
    T11). MCM's script shows MCM only when `GetUIFloat
    "StartMenu/NOGLOW_BRANCH/main_container/childcount"` is 8, and places its trigger from
    main_container's `x`, `y`, `width` and `_alpha`, all written by the game's code (the
    file leaves them unset). NVForever's pause column is hand-built and writes none of
    them; no rule is guessed. In the original, with the pause menu open (no MCM), and
    again from the main menu: GetUIFloat `StartMenu/NOGLOW_BRANCH/main_container/childcount`,
    `…/x`, `…/y`, `…/width`, `…/_alpha`, `…/_enabled`, and `StartMenu/_isPauseMenu`; then
    the same with the Settings page open.

    Answered for the pause menu 2026-11-12 (job mcm976e, nvftest, MCM 1.5 and Just
    Assorted Mods active) [observed]: `childcount` 8, `x` 1301.67, `y` 336, `width` 375,
    `_alpha` 255, `_enabled` 1, `_isPauseMenu` 0; the column's six rows and MCM's trigger
    are in [menu-xml §10.1](../spec-notes/menu-xml.md). Still open: the main menu's
    column and the Settings page's.

## Added 2026-11-01 (explosions' limb damage, issue #1338; open part since 2026-11-06, #1741)

74. **How much of a blast reaches each limb, at stock Health and on the player?** (P2;
    crippled-limbs.md §5.) Answered in part 2026-11-06 (o25_limbs) [observed]: on Ringo at
    `SetAV Health 5000` the torso lost 1/30 of the Health damage and the legs 0.24 of it,
    nothing else; NVForever takes those as condition points. Still open: rerun on an NPC at
    its stock Health (100–200) and once on the player, to tell condition points straight
    off the condition from a share of the limb's pool (of maximum Health), and whether the
    player's limbs take `fCombatPlayerLimbDamageMult`.

## Added 2026-11-03 (collision of animated objects, issue #1396)

77. **Does an animated activator's collision move with it, keep its pose after a load,
    and push the player?** (P3; activators.md §5.3, Collision follows the drawn model.)
    NVForever now poses the keyframed bodies a sequence moves from the group the object
    plays, keeps them solid while they move [engine choice], and after a load shows and
    poses the last group a script asked of it (a group played once at its end) [design].
    In the original, find an activator gate or lift a script opens with `PlayGroup` (one
    whose model has a keyframed collision body under a node its sequence moves): stand in
    its path while it moves, walk where it stood closed once it has opened, then save,
    load and walk there again. The answer: whether the player is pushed, stopped or passed
    through while it moves, and whether it is open (drawn and solid) after the load.

## Added 2026-11-03 (localized installs' code page, issue #1439)

78. **What code page and glyphs does a Russian install use?** (P3; fonts.md §2.1, needs a
    Russian install, not the English one.) NVForever reads all 8-bit text in one code page
    per run (`[Text] CodePage=` in NVForever.ini, or `NVF_CODE_PAGE`; 1252 by default) and
    draws a letter with the glyph at its code in it. In a Russian install: the bytes of a
    few `FULL` names and dialogue lines (Windows-1251 or something else?); whether its
    `.fnt` files are still 14 632 bytes with Cyrillic glyphs at their 1251 codes; whether
    any archive or model file name uses bytes above 0x7F; which setting (INI key or file)
    names the language, so the code page can be detected. Then with `CodePage=1251`:
    menus, dialogue, the Pip-Boy and the HUD show Cyrillic, not `?` or wrong letters.

## Added 2026-11-03 (sounds decoded on worker threads, issue #1428)

82. **Does `PlaySound X; StopSound X` in one frame play anything?** (P3; audio.md §11.5.)
    NVForever reads a sound's file on a worker, and a StopSound covers the sounds of X
    asked for before it, also in the same frame, so the pair plays nothing; `StopSound X;
    PlaySound X` plays X [hypothesis]. In the original (`nvfstrip`, a probe like sound1):
    one script block that runs `PlaySound` then `StopSound` of a short one-shot never
    played before in the session (its file not loaded yet) and of one played a moment
    earlier; the same with the order turned round; then the same with a Loop-flag SOUN
    (EMTMedicalBeep01). Log `IsSoundPlaying` of it each frame for 10 frames and record
    the audio. The answer: whether any of it is heard, and IsSoundPlaying's values per
    frame for each order.

## Added 2026-11-03 (volume sliders, issue #1450)

79. **How does the original's Music slider turn its music down, and does the main menu
    theme follow it?** (P2; audio.md §17.) Needs a hand on the in-game Settings slider:
    JIP's `SetGameVolume` from the console does not reach the playing mix. With the PC
    otherwise silent, a session records the loopback (scratch tool: fit the recording to
    the decoded file, as audio.md §17 did for the theme) while the owner sets Music to
    30 and 0 and goes back to the main menu each time: the theme at 0.30 of the file
    (linear), lower (a decibel curve), or 0.61 throughout (the menu ignores the slider).
    The same at a place whose music type names a shipped file (Kimble's hail,
    `special\mus_hailchief.mp3`, −5.36 dB; console `PlayMusic` did not start it over the
    cell's music) gives the in-game level against NVForever's.
## Added 2026-11-03 (JIP Script Runner, issue #1460)

80. **When does each Script Runner prefix run, and in what order?** (P2;
    ext-script-runner.md §2, §5.) Only `gr_` (game restart) is documented in the wiki
    export; the introduction page that lists the others is missing from it. With JIP LN
    installed, files under `Data\NVSE\plugins\scripts\` named `gr_a.txt`, `gr_b.txt`,
    `gl_x.txt`, `ln_x.txt`, `gn_x.txt`, `gx_x.txt`, each printing its name and
    GetGameRestarted / GetGameLoaded to the console (or a log file): which run at the main
    menu, at a new game, at a load from the main menu, at a load from the pause menu, and at
    quit; the order of `gr_a` and `gr_b` (and with a capital letter in one name); whether
    `gr_` runs again on a load from the pause menu; and whether `gl_` and `ln_` run before
    or after a LoadGame/PostLoadGame handler. Also: does a lambda handler a `gr_` file
    registered still fire after a save is loaded? The answers fix NVForever's moments
    (`fnv_game::script_runner::Prefix`). The files go in the original game's Data, so the
    owner places them (sessions don't write into the game folder).

## Added 2026-11-03 (surplus arguments on an extender call line, issue #1470)

81. **Does the extender's compiler drop arguments past those a call line's function takes,
    and does it read them first?** (P3; ext-script-runner.md §6, `xcall_line` in
    `crates/fnv-script/src/compile/gen.rs`.) NVForever drops them with a
    `surplus_arguments` warning, as the vanilla compiler drops them, but still fails the
    line when a dropped one cannot be read. Since then, a function whose catalogue lists
    too few parameters loses a real argument with only a warning, where it used to be an
    error. Unattended, with JohnnyGuitar and JIP LN loaded: `RunScriptSnippet 0` on
    (a) `SetJohnnyOnRenderUpdateEventHandler 1 ({} => PrintC "nvf a") 0 0` (ln_B42Optics.txt
    does this: four arguments to three parameters), (b) the same with `0 Nobody.GetAV Luck`
    in place of the last `0`, and (c) `Player.IsAnimPlayingEx 5 0 0 1` printed with
    `PrintC "%g"` (four arguments where the page lists three; a fourth parameter would show
    up as a different result from `5 0 0`). Then see which compile (console error or not),
    whether (a)'s handler prints, and what (c) returns. If (a) is refused, the drop goes
    and ln_B42Optics.txt is the mod's own error. If (b) compiles, dropped arguments are not
    read. If (c) differs from `5 0 0`, the catalogue needs the fourth parameter.

## Added 2026-11-04 (the tutorial menu from its file, issue #978)

196. **What does `clipwindow` do?** (P3; menu-xml.md §3.1.1.) Answered in part (private
    capture hud_tutorial_more, 1280 x 720) [observed]: in the drawing it is a rectangular
    crop of the descendants, a scrolled line stopping part way at the column's top and bottom
    edges. Still open: whether a click below the column's edge reaches a tile inside it (a
    list's row scrolled out, in a menu with `list_box.xml`).
83. **Does a picture's `alpha` reach the tiles inside it?** (P3; tutorial-menu.md §5.) The
    tutorial menu's content under its background of alpha 193.8 reads full strength, so
    NVForever marks that one tile; other tiles still multiply through a picture. A menu or
    a test tile with a text inside a half-alpha picture answers whether this holds for
    pictures generally.
84. **The tutorial menu with two pages.** (P3; tutorial-menu.md §4, §5.) Answered (private
    capture hud_tutorial_more, 1280 x 720) [observed]: a second `ShowTutorialMenu` replaces the
    first, `TM_page` stays hidden, Next and Previous stay hidden and not targets, and
    `ClickMenuButton` on them returned 1 with the text unchanged. The keys A and S were not
    tried: the job taps them only when that call returns 0. NVForever's pages differ;
    owner-review.md item 45 asks which to keep. Close's text reads alpha 255 and brightness 255
    [observed], which rules out those two traits of its own for its (206, 147, 53), not an
    opacity from above it or the compositing; font 2's texels [hypothesis].

## Added 2026-11-05 (items without a name, issue #1603)

85. **Does the original list an item that has no name?** (P3; engine/nvf-play/src/menu/model.rs,
    `shown`.) NVForever leaves an item out of every inventory list (the player's, containers,
    barter) when its record has no FULL or a blank one [hypothesis]. The vanilla
    DONOTUSEBookGeneric07 (0x000199BB, BOOK, no FULL) is in SublistBooks100, under the
    household and library clutter lists. A capture: `player.additem 000199BB 1`, then open the
    Pip-Boy's Misc page and a container holding one. Is it absent, a blank row, or shown by
    another name?

## Added 2026-11-05 (creatures and furniture, issue #1522)

86. **Does a sandboxing creature near free furniture ever use it?** (P2; furniture.md §9.)
    Answered in part, minutes of watching and not days [observed, 2026-11-04 and 05, the
    original over Moonlight]: no creature read as on furniture in any of these.
    - Super mutants `001744C1` to `C4` among `SuperMutantBed` and `FloorSitMarker`: 534 s,
      hours 12 and 23; `DefaultSandboxEditorLocation512` for at least the first, and `C4` was
      under Sandbox in 0 of 111 samples.
    - Colmillo and GibsonDog01 under `Sandbox256` the whole time: 358 s, within 98 to 168
      units of `OldLadyGibsonChair`, probably free at hour 20 [hypothesis]. People in the
      loaded area did read as on furniture, so the reads fire.
    - Three rats near `Outhouse0000`: 355 s, partly in combat, so weak.
    - Neil: 533 s, never under a sandbox package, so weak.
    - The Nipton mongrels `0016B483` and `0016B484` under `DefaultSandboxEditorLocation1024`:
      596 s, hours 12 to 15 and 18 to 20, no combat, one of them 226 units from the free cross
      `00139FD6` for most of it. The cross's record carries XIBS (Ignored By Sandbox) [documented:
      `fnvtool` over the record], so this no may be the flag and not the creature; the dogs and mutants above, whose furniture has no XIBS,
      carry the question.

    **Pending**: a person sandboxing near an empty cross. NVForever
    keeps a creature off furniture the tree gives it no clip for, and leaves crosses out of a
    sandbox's choices [design]. If a creature does use furniture, say which, on what, and
    with which clip: the rule in §9 changes.

## Added 2026-11-05 (placed particle scale, issue #1547)

87. **Does a placed reference's scale size its particles?** (P2; particles.md §7.7, #3673.) The
    Ultra-Luxe fountain's ULFountainFX (0x0016566E) is placed at scale 0.5. Take a frame of the
    original at hour 12 from about (1176, -700, 1080), facing north, and the same frame in
    NVForever. Check the size of the drops against the basin, how high the spray goes, and
    whether it comes out of the spout. If the original's spray is the file's full size, the
    rule in §7.7 is wrong and the cause of the misplaced spray lies elsewhere.
    Also compare placed smoke in FalloutNV.esm (#3673): `dlc03fxdarksmoke.nif` at scale
    0.23 (`0x000E72EF`, WastelandNV 28255, 32694, 5595) against the one at scale 1
    (`0x00137F86`, -31401, -95650, 6918), measuring puff size beside nearby geometry in
    the original. If that comparison supports radius scaling, re-check the Ultra-Luxe
    fountain (`0x0016566E`) and tyre smoke (`fxtiresmoke.nif`) before and after testing
    the candidate rule.

## Added 2026-11-05 (leveled rolls and locks, issue #1567)

88. **Does a leveled door lock change with the player's level?** (P3; simulation.md §5,
    `fnv_sim::World::lock_read_level`.) NVForever hardens a leveled lock only from a level
    fixed at its cell's load, so a door keeps its authored level [design]; the GECK tutorial
    calls the checkbox deprecated [documented, encounter-zones.md §7], and nothing says the
    original hardens at all [hypothesis]. The 26 leveled locks are all in the add-ons (Dead
    Money 22, Old World Blues 3, Honest Hearts 1; simulation.md §5). A capture at a door with
    the flag, never activated: `GetLockLevel` on it at the save's level, `player.SetLevel 31`,
    `GetLockLevel` again, then the same on a flagged container, and on both after leaving the
    area and coming back. Does the number move, by how much, and from which level?

## Added 2026-11-05 (DamageActorValue past zero, issue #1568)

89. **Does DamageAV past zero kill at all, and what of the player, an essential actor and the
    kill's credit?** (P3; unconscious.md §2.2.) NVForever kills [hypothesis]. Check 22's two
    super mutants died after DamageAV 300 with NCR troopers firing, no GetDead read and their
    Health not read, so it does not isolate the cause. In the original, first: a lone
    non-essential NPC with nobody shooting, `getav health`, `damageav health 9999`, `getdead`
    at once and 5 s later, `getav health`. Then, each in its own run: `player.damageav health
    9999` (does the player die?); on an essential NPC the same (knocked out, or dead, or
    standing at a negative Health, and GetDead); on the lone NPC, the player's XP and karma
    before and after (is the kill anyone's?). Also: does Health stay negative or stop at 0?
    The answer: the logs.
    Answered in part [observed: campaign o29_dmg, 2026-11-08, `nvfairport`, issue #1858]:
    a lone trooper and the player both die in the next game loop, Health stays at the
    negative value (-9949), OnDeath names no killer and karma does not move, as ours does
    (unconscious.md §2.2). Still open: an essential NPC (SetEssential on a spawned leveled
    trooper's base did not take; needs a save with a real essential NPC near) and the XP.
    Trap: a session that kills the player loops, since the death reload reloads the save and
    the plugin starts the pending job again; give it `--retries 0` and take its job file out
    of `jobs\` once the first death is logged.

## Added 2026-11-04 (the container menu from its file, issue #1468)

90. **The container menu: what the reads could not show.** (P3; barter-menu.md §8; the capture
    `hud_container_*`, parts c, d and g retaken since, with shots.) With a long list on both
    sides at 1280 x 720: (a) does a selected row show a faint fill beside its lines, and how
    faint against NVForever's `_NVFHotrectFill`? (b) DAM and DPS on a weapon's card: do they
    fade into each other at one place, how fast, and is DPS ever above DAM? (c) what does
    `CNDArrows` (`repair_meter_pips.dds`) show at a weapon's and armour's condition, and does it
    move with it? (d) keys: do the arrow keys and W/S move a row, A the Take All only?
    (e) the wheel: does `_wheelmoved` move the list and not the selection, and by what? (f)
    the marker's drag: `dragstarty`, `dragy` and `dragoffsety` through a drag and after it.
    (g) how dark is the backing at 193.8 compared with NVForever's at that value?

## Added 2026-11-06 (the trait menu from its file, issue #1519)

91. **The trait menu: what the capture could not take.** (P3; chargen.md §4.1.) In character
    creation at 1280 x 720: (a) take a trait, then a second: how do `_CurrPoints`, the row's mark
    and the counter follow? (b) what does Reset restore with traits taken? (c) `_TextAlpha` and
    `_enabled` on a trait that cannot be taken. (d) a real wheel over the list. (e) the
    description scrolled to value 8. (f) why did the capture's list lack two traits?
92. **The player's melee reach, and the deathclaw's.** (P2; combat-rules.md §10,
    npc-behaviour-compare.md §3.) Ours now reaches `fCombatDistance` 107 × Reach from the
    player's body (fitted to the NPCs' swings), so long weapons reach less than before (a
    lead pipe 156 units from the eye, was 188). In `nvfairport`, a standing settler: the
    farthest centre-to-centre distance at which the player's knife, sledgehammer and lead
    pipe swings still land (GetHealthPercentage or OnHit per swing, the player backing off
    5 units a swing). And a deathclaw (VCrTier4DeathClawMed) on the standing god-mode
    player again: o12_reach had one start at 88 units, far nearer than its RNAM 192 gives.

## Added 2026-11-06 (observer batch o27: issues #970, #986, #1401)

Answered [observed, 2026-11-06, `nvfairport`]: the light at an actor (#970, o27_light:
the brightest channel of the sunlight outdoors, of the cell's ambient indoors;
detection.md §3) and the tumbleweeds (#1401, o27_tumble: awake after a load, rolling
north-north-west until they rest; wind.md §2). Open:

93. **The light at an actor where something shades it, and a lamp's share.** (P3;
    detection.md §3.) `player.GetActorLightAmount` at noon under a roof outdoors (the
    Goodsprings gas station's canopy, SCOLgsGasStation at (-75584, 4562)), and indoors
    at set distances from one lamp of known radius (a dark interior such as
    SLGoodspringsCaveINT, which reads 36.08 away from its lamps). NVForever gives a
    shaded target the weather's ambient and adds the lamps' brightest channel ×
    (1 − (d/r)²) [hypothesis].
    Lamp answered [observed: job o29_light, 2026-11-08]: a RedLight of radius 384 reads 100
    to 192 units, about 96 at 256, 68 at 320 and only the ambient from 384 out; ours comes
    within about 5 (detection.md §3). Shade still open: 14 spots around the gas station read
    100, but which stood under the canopy is not known; take a frame at each next time.
94. **Tumbleweeds against the wind's speed, and whether they start again.** (P3; wind.md
    §4.) Force HVDustStorm (127) and then load the save (or `coc` away and back) so the
    tumbleweeds wake under it; compare their speeds with o27_tumble's 150–210 units a
    second under 50. Then watch one at rest for an hour of game time at TimeScale 30.
95. **Holotapes: subtitles and the radio.** (P2; holotapes.md §3, #986.) Tried
    2026-11-06 (o27_holotape, `nvfairport`): the job gives VMQ01BeagleJournalNote and
    waits; the Pip-Boy was to be driven by hand (computer use) with the game's sound
    recorded by loopback (the `soundcard` module records the default output device), but
    the input was interrupted before the Pip-Boy opened, so the run was stopped. Still to
    do: tune the Pip-Boy radio to Radio New Vegas, play the note from DATA > Misc, close
    the Pip-Boy; screenshots during the tape show whether a subtitle is drawn (the
    owner's INI has dialogue subtitles on, general subtitles off), and the recording
    whether the radio goes quiet under the tape, ducks or plays on, and comes back
    after. The plugin cannot pick a menu entry, so this needs the owner or hand input.

## Added 2026-11-06 (observer batch o28, sound: issues #989, #990, #992)

Answered in this batch (audio.md §15.5, §16.1): talking activators' lines are not
filtered by the original and their files are already processed (#992); a creature's Idle,
Aware and Attack sounds play only from clip keys, and the Securitrons' siren played in
none of the five situations tried (#989); no shipped master sets an acoustic space's walla
(#990). Tools for this batch (scratchpad, not committed): JIP `IsSoundPlaying <sound>
<ref>` read every sample through the player's `Variable01`-`09`, and the WASAPI loopback
recorded with Python `soundcard` (ffmpeg has no loopback device on this machine), fitted
to the decoded file by cross-correlation and a per-band transfer estimate.

96. **When does a Securitron sound its siren (`NPCSecuritronSiren`, CREA sound type 21)?**
    (P3; audio.md §16.1.) Not on spawning, standing 20 s, `StartCombat`, an assault or
    trespass alarm, nor while hurt to death (o28_creature_sounds, o28_creature_sounds2).
    Left to try: a Strip Securitron reacting to a crime it sees for itself in `nvfstrip`
    (the player shooting a Strip NPC by `FireWeapon`, IsSoundPlaying without a ref), a
    Securitron switching to its Mk II (Lucky 38 upgrade), and the Hoover Dam battle's
    Securitrons (VHDSecuritronBattle…). If none, it is unused data.
97. **Does the original filter a helmet wearer's voice?** (P3; audio.md §15.5.) A line
    said by an NPC given a power armour helmet (Modulates Voice) recorded on the loopback
    beside the same line bare-headed, fitted to the file as o28_intercom was. Needs a
    line the observer can make an NPC say with a known file (`SayTo player <topic>` with
    a topic whose line has conditions only on the speaker).

## Added 2026-11-06 (exploding pants, issue #1640)

98. **A planted explosive: who dies, who panics, who sees.** (P3; pickpocket.md §5.)
    Reverse-pickpocket while sneaking and unseen, each in its own run from a fresh load: (a)
    a Pulse Grenade on an ordinary NPC (a Goodsprings settler): does it die? Note its
    Health before and after; NVForever keeps it alive [hypothesis]. (b) The same on a
    robot (a Protectron or Mister Handy): does it die? (c) A Frag Grenade on a child (Freeside
    or Westside) and on an essential NPC (Sunny Smiles before Back in the Saddle, or Doc
    Mitchell): does it go live (GetPlantedExplosive 1 after 0.1 s) and go off? Is it still
    in their inventory afterwards? NVForever: neither goes live. (d) During the panic,
    stand within 2 m in view with Sneak low: does the victim detect the player
    (`GetDetected` on it, then hostility after the blast)? (e) The panic on a creature that
    can be pickpocketed (a giant rat): does it play anything, does it stop, or does it go
    on walking? (f) Seen by a third NPC halfway through the fuse, then hidden at the blast:
    does that NPC turn hostile? NVForever counts it as a witness. (g) The live item
    (#1640 item 5): plant one Frag Grenade on an NPC who already carries Frag Grenades,
    then reopen the pocket before the blast. Is the planted one a row of its own, and what
    does that row say? Does the count of the victim's own row change? Can the player take
    the planted one back, and does that roll? NVForever: a separate "(Armed)" row, refused,
    no roll. (h) During the fuse, run `<victim>.RemoveItem <grenade> 1`, `<victim>.Drop
    <grenade> 1` and `<victim>.EquipItem <grenade>` from the console, then
    `GetItemCount`: which unit goes, the planted one or the victim's own? Does the blast
    still happen? NVForever: only the victim's own units move; the blast takes the planted
    unit. (i) The same NPC with the planted grenade its only grenade, in combat with
    someone else during the fuse: does it throw the planted one? NVForever: no.

## Added 2026-11-06 (the power fist's hand, issue #1586)

99. **The power fist's hand on Veronica: what is left open.** (P3; actors.md §9.2.) Capture
    `pf_h2h_rest3` (nvfairport, Veronica at her placed spot at the 188, power fist drawn
    with SetAlert, AI off, first-person camera 110 units off in front, to her right and to
    her side; brass knuckles the same way): the gauntlet sits on the forearm where
    NVForever puts it. At its front the original shows the weapon's metal hand (the WEAP's
    BIPL addon `ARMAPowerfist`), not her bare hand. Still to look at: the same with the
    weapon holstered, and in the player's first person.

## Added 2026-11-06 (burning in first person, issue #1772)

100. **What does burning draw on the first-person arms?** (P2; effect-shaders.md §4.)
    NVForever puts shader overlays only on the third-person body and leaves the
    first-person arms alone [design: `effect_shaders.rs::play`]. Whether the original
    puts Flames01's membrane, flames or add-on models on the arms is unconfirmed
    [hypothesis]. Capture the same first-person view before and during
    `player.PlayMagicShaderVisuals 0005F9B6`, with a weapon lowered and then raised;
    switch to third person while it plays for comparison. Do the arms change colour,
    carry flame sprites, or emit particles, and does switching perspective change that?
    Answered in part 2026-11-06 (private job ov_burn, the Prospector Saloon) [observed]: with
    the weapon holstered first person shows nothing of the burn, third person shows flames at
    the head, and back in first person nothing again. The raised weapon's arms are still open.

## Added 2026-11-07 (interior light direction, issue #1645)

101. **Interior lighting: which way does the cell's directional light point, and does it
    cast shadows?** (P3; world.md §8 "Interior cells".) NVForever reads XCLL's pitch as
    the direction the light shines in (270 straight down) [hypothesis, from the templates'
    values]; the azimuth's zero and sense are unknown, and whether the original draws
    shadows from it at all is too (outdoors it casts no sun shadows from the world, world.md
    §6). Capture GSGasStation (azimuth 180, pitch 250) at hour 12 from the door, looking
    down at the tiled floor under the hanging lamps, then up at the ceiling: (a) are there
    shadows under the lamps, the crates and the counter, or on the ceiling beside the
    lamps, or none; (b) which side of the crates and counter is lit brighter (that gives
    the azimuth). The same frame of a cell with azimuth 0 and pitch 270 (a cave on
    CavesDefaultTemplate) shows whether straight down holds.

## Added 2026-11-09 (NVSE probe runs of the new batches and the merged sets, issue #1920)

102. **Extender calls the runtime compiler refuses: HSVtoRGB, RGBtoHSV,
    HudBarSetValueScriptVar.** (P3; ext-jg7.md, ext-sup7.md.) In the original none of
    them compiles in a runtime-compiled probe script (xNVSE 6.4.9): HSVtoRGB/RGBtoHSV in 7
    forms (literal inputs, float or int output variables, `let fA :=`, `let aR :=`, a bare
    statement) and HudBarSetValueScriptVar in 7 (GameHour, a literal, a local float, 2 or 3
    arguments, a quest variable, a missing bar) [observed: probe set jg7 and two
    diagnostic sets, 2026-11-08]. NVForever's compiler takes all of them, so their answers
    (channel rounding, hue in degrees, whether the bar follows the variable) are still
    [hypothesis]. They need a script compiled into an ESP (the GECK, or NVFExtProbe.esp's
    next version): for HSVtoRGB 120 0.5 0.5 and RGBtoHSV 0 0 255 and 64 128 64, print the
    three outputs; for HudBarSetValueScriptVar, bind a bar to a quest float, change it, and
    read the bar.
    Also from these runs: HideItemBarterEx never made IsItemBarterHiddenEx read 1, on the
    caps (F) or a 10mm pistol (434F), with and without the vendor argument [observed: jg7,
    two runs]. Does HideItemBarterEx hide the item in Doc Mitchell's barter menu at all?
    That needs the menu open (a capture with screenshots).

103. **Setup the probe game lacks (owner step).** (P2; extender-compat.md.) On
    2026-11-09 the probe game's load order gave NVFExtProbe.esp and The Mod Configuration
    Menu.esp mod index 255 (not active), and the DLC packs load as GunRunnersArsenal 5,
    ClassicPack 6, MercenaryPack 7, TribalPack 8, CaravanPack 9, not in the lab's order
    [observed: GetModIndex in the original]. **Done 2026-11-10, except the order:** the owner
    installed ITR NVSE, copied the test INIs of `tools/capture/jip-probes/config` into
    `Data\Config`, and `plugins.txt` now lists both plugins. The sets itr, ini-files,
    nvse-vars and the five xNVSE core `ext/*` sets were then recorded and labbed
    (docs/handoff/l5ext-final-rerun-2026-11-10.md §3). ppNVSE and JIP++ are not installed
    and must not stay installed: ppNVSE needs JIP++, and JIP++ replaces JIP LN, which
    JohnnyGuitar checks for. They go in only for a single run with `bJIPFixes=0`, as
    ext-jippp.md §3 describes. The pp and jipppln goldens from 2026-11-01 are on the other
    machine. Still open: the plugin order, or the word that the lab should take the
    game's order; and recapturing the rows that read NVFExtProbe.esp as missing
    (nvse-fourth, plugin-odds, gaps1, showoff4, mcm).

## Added 2026-11-09 (a walking Securitron's wheel)

104. **Does a walking Securitron's wheel roll on smoothly, or jump back once a cycle?**
    (P3; animation.md §9.12.) The walk `nvsecuritron\locomotion\mtforward.kf` turns
    `Bip01 Wheel` 661° in its 2.67 s loop with a `BSRotAccumTransfInterpolator`, so its
    last key is 59° on from its first. NVForever adds that turn up from cycle to cycle
    [hypothesis, from the name and the keys]; played as an ordinary loop the wheel would
    snap back 59° every 2.67 s. Record Victor walking beside the player out of Goodsprings
    (the wheel side on, 30 fps or better, 10 s or more) and step through the frames at
    each 2.67 s mark: a steady turn says it adds up, a jump says it does not.

## Added 2026-11-11 (Mr. House's screen, issue #2150)

105. **Does Mr. House's screen enter combat, and does it ever turn?** (P3; packages.md
     §7.1b.) NVForever keeps an Immobile creature with a Turning Speed of 0 at its placed
     heading always [hypothesis for the rule; the owner saw the original's screen stay
     still]. Not observed: whether the original's `Lucky38MrHouseREF` (0x001264C5) takes
     a combat target at all when a fight starts in the penthouse or it is shot
     (`IsInCombat`, `GetCombatTarget`), and its `GetAngle Z` through a conversation, a
     goodbye line and a fight. Here it can still be in combat, only without turning.

## Added 2026-11-11 (a regenerating weapon's ammo counter)

106. **What do the HUD counter's two figures show with a Recharger Rifle out?** (P3;
    hud.md §5, #2170.) NVForever shows the weapon's charge as the first figure and the
    carried count of its ammo type, with nothing taken off, as the second [hypothesis].
    Equip a Recharger Rifle, screenshot the counter at full charge, after three shots and
    again once it has recharged; the same with a Recharger Pistol and the MF Hyperbreeder
    Alpha. Note whether the second figure is 0, the clip size or something else, and
    whether an ammo label stands left of the counter.

## Added 2026-11-12 (Continue During Combat in combat, issue #2155)

Asked by #2155: NVForever read the package flag as "the actor stays on its package in
every combat state", so a melee fighter on such a package never reached its enemy. What
does the original do? Measured 2026-11-11 and 12 with the AI observer
(`tools/capture/observe/cdc_leash.campaign.json`, `cdc_lanius.campaign.json`; private
outputs `obs_cdc_leash_*`, `obs_cdc_lanius_*`). Distances are game units, flat (X, Y).
The rule the engine fix should follow is in alert.md §5.

How the leash sessions work: on the Searchlight Airport runway (save `nvfairport`) one
`LvlNCRTrooper` at a time is spawned 500, 1,000 or 2,000 units from the standing player
(god mode), armed with a knife or a service rifle, and given its package there by
AddScriptPackage, so the place of a "near current location" package (its **post**
below) is that spot. Eight seconds later it is set on the player with StartCombat, and
37 s after that stopped with StopCombat. Sampled 30 times a second. A script package
and a forced StartCombat are not an NPC's own package list and its own hostility: where
that could matter it is said.

The packages [observed: `fnvtool ai show` over FalloutNV.esm]:
`VFSOrrisThugGetShotPackage` (Travel, near current location, radius 0; Skip Fallout
Behavior, Weapon Drawn, Continue During Combat; no conditions), the kind Lanius is on;
`DefaultGuardinCombatInterior` (Guard, near current location, radius 512; Continue
During Combat only); `FFEU03NPCGuard` (Guard, near current location, radius 0; Skip
Fallout Behavior, Weapon Drawn, Continue During Combat).

107. **Legate Lanius in the final battle: does he leave his marker?** (P1; alert.md §5,
    issue #2155.) **Answered [observed, 2026-11-12, campaign `cdc_lanius` from the
    owner's save `nvfLanius`]: yes. He runs to a player he detects, fights there, and
    goes back to his marker when the fight ends. No leash showed, up to 2,557 units.**
    - *The data* [observed: `fnvtool ai show`, `esm decode`]. His list, in order:
      `VHDLegionOliverFightLegateWalkOut` and `VHDLegatePlayerDialogPackage` (Dialogue),
      `VHDLegatePreConversationPackage` (Travel, editor location, before the battle),
      `VHDLegateShowdownWaitPackage01` (Travel to `VLegateWaitPlayerMarker`, radius 0,
      **without** the flag; conditions `VLegateShowdown.ShowdownStart == 1` and
      `vHDBattleController.iBattleState == 1`, the NCR ending),
      `VHDLegateShowdownWaitPackage` (the same Travel **with** Continue During Combat,
      also Must Reach Location, Must Complete, Always Run, Allow Falls; `iBattleState >=
      3`, the House and Independent endings), `DefaultStayAtEditorLocation`. His scripts
      add script packages: `VLegateRunToRanger01` (Travel to the same marker, radius 256,
      the flag), `VLegateWaitPlayerSpot` (radius 32, the flag), `VLegateHide01` / `02`
      (Travel to his two healing spots, the flag) and `VLegateShowdownGreet` (Dialogue).
      Combat style `CSNVLegateLanius`: combat radius 10,000, cover search radius 2,048,
      no weapon restriction, attack chance 90, rushing attack chance 45 at 1.5 times the
      distance. Speed multiplier 180, Aggressive, Foolhardy.
    - *The save* (`cdc_lanius_state`): the real battle of an Independent ending,
      `iBattleState` 4, `ShowdownStart` 1, `LegateRuns` 1, `LegateFightsPlayer` 0; he
      stands 20 units from the marker on `VLegateRunToRanger01`, procedure DONE, the
      player 1,461 units from him. So no staging was needed. What the capture adds: god
      mode, and his hostility by the lines of his own OnHit block (`AddToFaction
      VCaesarsLegionFaction 0`, `StartCombat player`), since it can neither answer his
      talk nor hit him; `LegateFightsPlayer` stays 0, so no reinforcement wave comes, and
      the player never strikes, so his healing and fury stages do not run. About 13
      samples a second there.
    - *Player 1,461 units off, as saved* (`as_saved_1461`): 1.2 s after turning hostile
      he was 630 units from the marker, after 3.8 s within 86 units of the player. He
      fought there for the two minutes the session gave him (1,311 of 1,560 samples in
      an attack phase), 1,295 to 1,660 units from the marker. Calmed (out of the
      faction, StopCombat), he had `VHDLegateShowdownWaitPackage` at the next sample,
      procedure TRAVEL, and stood 16 units from the marker 4.7 s later.
    - *Player 760 units off* (`at_760`): the same; he fought 575 to 830 units from the
      marker and was back within 19 units 3.2 s after the fight ended.
    - *Player 2,360 and 3,482 units off, down in the lower camp* (`at_2360`, `at_3482`):
      he never detected the player there (GetDetected 0 throughout). In combat he walked,
      not ran, 1,659 and 1,661 units from the marker (the same spot both times, about
      350 units from where the first session's fight had been), left combat by himself
      10 s after it began and was back on the marker 5 to 7 s later. A target he could
      not find, not a leash.
    - *Player teleported from 760 to 3,482 units mid-fight* (`at_760_then_3482`): he lost
      his target at that sample, was out of combat 0.5 s later and on the marker (14
      units) 3.3 s after that; he did not follow a player he no longer detected.
    - *Made hostile on his way back to the marker* (`en_route`: put 3,482 units from it,
      the player 2,360 units from the marker to one side, hostile when he was 1,225
      units from the marker and did not detect the player): for 10 s he kept on toward
      the marker, to within 201 units, away from the player; he dropped out of combat
      once and came back into it, then went for the player, detected it at 651 units and
      fought it 2,235 to 2,557 units from the marker until calmed 65 s later, when he
      set off for the marker again. Whether those first 10 s were his Travel going on
      (what the GECK wiki says of the flag) or a search for a target he did not see is
      not told apart [hypothesis either way].
    - **Not measured**: the fight begun by his talk or by the player's blow, with the
      reinforcement waves; a detected player further than 2,557 units from the marker;
      the NCR ending (his package there has no flag) and the House ending (the same
      package as here).

108. **A melee NPC on a Guard or Travel package with the flag, an enemy at 500, 1,000 and
    2,000 units.** (P1; alert.md §5.) **Answered [observed, 2026-11-11 and 12, campaign `cdc_leash`, sessions
    `knife_*`]: on Travel it closes in and attacks at every distance, with no leash. On
    Guard it depends on the package: two held the trooper when the enemy was outside
    the guard radius the GECK wiki gives, three did not hold it at all.**
    - *Travel* (`VFSOrrisThugGetShotPackage`; picked because it is a plain stand-in-place
      Travel like Lanius's, with no condition, no script and a place a spawned actor can
      have). The trooper stood on its post, procedure DONE. Set on the player 500, 1,000
      and 2,000 units off, it ran in and struck first after 3.2, 5.1 and 8.3 s, and fought
      40 to 220 units from the player to the end (609, 645 and 585 of 1,101 samples in an
      attack phase), up to 863, 1,226 and 2,043 units from its post. It never stopped or
      turned back: no leash, and no different from the trooper with no package (check
      111). A script package does not come back after the fight, so "does it return" is
      answered by Lanius (check 107), not here.
    - *Guard on a fixed post* (near editor location: a spawned actor's editor location
      is where PlaceAtMe put it, as `knife_guarded128_1000` showed: sent 1,000 units off,
      it walked back there. In the `knife_ed*_walk*` sessions the trooper is spawned
      beside the player and the player walks away down the runway before the fight; the
      distances are the player's from that post when combat began):
      - `EvergreenMillsSniperStayAtPosition01` (radius 0; Weapon Drawn, No Warn/Attack
        Behavior). Player **167** units off: it closed in and fought, up to 412 units
        from the post, and stood 9 units from the post again after the fight. Player
        **654** units off: in combat for the whole 35 s with the player as its target and
        detected, it **stood on its post (0 units) and never attacked**.
      - `DefaultGuardStayAtPosition01` (radius 128; Weapon Drawn, No Warn/Attack Behavior;
        LvlRaiderGunGuard's and the Great Khans' package). Player 366, 456 and 621 units
        off: it closed in and fought every time, up to 596, 735 and 899 units from the
        post, and was 12 to 180 units from it after the fight.
      - `DefaultGuardLinkedRef512StayAtEdLoc400` (radius 400; also Defensive Combat).
        Player 212, 587 and 932 units off: it closed in and fought every time, up to 549,
        897 and 1,250 units from the post, and was 86 to 474 units from it afterwards.
      - The trooper with no package, player 943 units off (`knife_none_walk750`): closed
        in, fought up to 1,144 units from where it stood, and stayed where the fight
        ended.
    - *Guard near the current location* (the post is where the trooper got the package;
      not a fixed point, it seems: after a fight the trooper stayed where it was):
      - `DefaultGuardinCombatInterior` (radius 512; the flag and nothing else). Before
        the fight the trooper wandered up to about 770 units from its post. With the
        player **1,001, 1,005 and 2,000 units from the post** (3 sessions) it was in
        combat with the player as its target, detected, for the whole 37 s (80 s in one),
        **stood where it was and never attacked**, the player 810 to 1,898 units from
        it. With the player **154, 301, 301, 501 and 502 units from the post** (5
        sessions) it closed in from 419 to 831 units away and fought, staying within 472
        to 769 units of the post. In one more session with the player 501 units from
        the post it stood, 763 units from the player, and did not attack [not
        explained].
      - `FFEU03NPCGuard` (radius 0; Skip Fallout Behavior, Weapon Drawn): from 1,000
        units it struck first after 4.2 s and fought up to 1,180 units from its post.
    - *Reading.* The two packages that held their trooper match the GECK wiki's rule for
      a Guard with the flag ("will not move outside the guard radius in order to pursue
      the target", the radius being the package radius × 1.5 or 384, whichever is
      higher): 384 lies between 167 and 654, 768 between 502 and 1,001. What decides is
      where the **target** is, measured from the post; a held actor does not walk to the
      edge, it stands. The three packages that did not hold theirs at 1.6 to 4.9 times
      that radius all have bit 2 (value 4) of the Guard type-specific flags set, and the
      two that held have not (raw values 6, 6, 4 against 2, 2) [observed over these five
      packages; that the bit is the cause is a hypothesis: xEdit names no Guard flag
      there, and the packages differ in other fields too].
    - **Run without their walk** (2026-11-12, the last run): in `knife_ed0plain_walk300`
      and `_walk750` (`RHSAmbusherGuard`: radius 0, type-specific flags 0, as most
      Guard packages with the flag have), `knife_ed0_walk450` and `_walk550` and
      `knife_ed128_walk1900` the player's walk did not happen (the drive ended at its
      frame limit, the Forward key having no effect in that run), so the player stood
      166 to 259 units from the post and all five troopers closed in and fought, as a
      near enemy let every Guard do. They settle nothing more: whether a Guard with no
      type-specific flags holds its actor, where between 167 and 654 units the radius-0
      line is, and whether the radius-128 Guard is held further out than 621 units,
      are open. Run them again with the game window in front.
    - **Not measured**: an engaged guard whose target then leaves the radius on foot
      (the teleport sessions end the fight for any trooper, check 111); a Guard near a
      reference or the linked reference; an NPC hostile by its own faction instead of
      StartCombat.

109. **Control: does a ranged NPC on the same packages hold its place and shoot?** (P2;
    alert.md §5.) **Answered [observed, 2026-11-11, sessions `rifle_*`]:** it shoots, and
    it does not hold its place: it moves as a rifleman with no package does.
    - On the Guard package (radius 512), the player 1,000 and 2,000 units from the post:
      first shot 2.4 s after combat began, firing through the 37 s (742 and 748 of 1,101
      samples in an attack phase) from 928 to 1,065 and from 1,331 to 1,494 units, while
      it moved from 138 to 1,059 and from 647 to 1,212 units from its post.
    - On the Travel package (radius 0), 1,000 and 2,000 units: first shot after 2.2 s,
      from 892 to 1,056 and from 1,895 to 2,040 units, moving up to 547 and 1,346 units
      from its post.
    - No package given, 2,000 units: first shot after 2.4 s, from 1,882 to 1,995 units,
      moving up to 565 units from where it stood.
    So the flag does not pin a gunman either; our "sniper posts stay put" reading rests
    on the flag's name only. Not measured: a gunman whose target is out of its weapon's
    range, and one on a Guard package whose target is outside the guard radius of a fixed
    post (the troopers' posts here were "near current location").

110. **A melee creature on a package with the flag.** (P2; alert.md §5.) The data has
    them: 85 creature bases carry at least one such package [observed: `fnvtool ai show`
    over every CREA of FalloutNV.esm; of the 399 packages 251 are on some actor's list,
    291 NPC bases and 85 creature bases]. Most are Fallout 3's; New Vegas's own melee
    ones are `V22FemaleMantis` (its only package: `V22MantisFemaleEggGuardPackage`, Guard
    on its linked reference, the flag alone), `Lily` and `Rex` (their wait and go-home
    packages) and `VStreetCrBrahmin` (the brahmin rush). **Answered in part [observed,
    2026-11-11 and 12, sessions `coyote_*`, `mantis_own_*`]:** a coyote given the Guard
    (radius 512) or the Travel package, and a spawned `V22FemaleMantis` on its own
    package, attacked the player at once and fought in melee, 81 to 264 units away, as
    the coyote with no package did; the package made no difference at that distance.
    **Not measured: how far they go.** A creature spawned beside the player attacks before
    it can be put anywhere, and moving the player 1,000 units off mid-fight ended the
    fight within 0.6 s for all three coyotes, the one with no package too (it stood
    1,009 units from the player for two minutes): the teleport, not a leash. Afterwards
    only the one on the Travel package found the player again and attacked; one run each,
    so that is no rule. To settle it: a creature placed in the data with such a package
    (the Vault 22 mantis mothers), the player walking up and backing off on foot.

111. **Baseline: how far does the same melee NPC chase with no such package?** (P2.)
    **Answered [observed, 2026-11-11, `knife_none_2000`]:** the knife trooper with no
    package given (its own list) ran 2,000 units to the player, struck first 8 s after
    combat began and fought 40 to 125 units from it for the rest of the 37 s (590 of
    1,101 samples in an attack phase), 2,178 units from where it started. The trooper on
    the flagged Travel package did the same from the same distance (`knife_travel0_2000`:
    first blow after 8.3 s, 585 samples, 2,043 units from its post).
    A limit of these sessions: when the player was teleported 1,500 units away mid-fight,
    the trooper lost its target at that sample and was out of combat 2 s later, with no
    package (`knife_none_500_away`) as with the Guard package
    (`knife_guard512_500_away`), and did not follow. The troopers are not hostile to
    the player by themselves, so a teleport ends a StartCombat fight; it measures no
    leash.

## Added 2026-11-11 (a Use Weapon package with Continue During Combat, issue #2120)

112. **Attacked, does a Use Weapon package's actor with Continue During Combat keep firing at
    its package's target, or turn on its attacker?** (P3; use-weapon.md §3, spec note.) The
    GECK wiki's Use Weapon Package page says the actor "will continue to fire and stand at the
    designated position, even during Combat" [documented], not at whom. NVForever keeps it at
    its stand firing the package's volleys at the package's target, ignoring its attacker,
    while the use is one the shooter can act on (in place, its target among the drawn
    standing actors); when it is not (a marker target, a target not drawn, not yet at its
    stand) it fights its attacker with the ordinary attacks while its package still moves
    it
    [design, #2145]; the other reading is that it holds its stand and fights its attacker
    (owner-review item 57). Pick a package with the flag whose target 2 is an actor and whose
    conditions hold from a save (`fnvtool ai packages` lists the flag), watch the actor fire at
    its target, then attack it from the side: (a) does it keep firing
    at the package's target, turn to the player and fight from its stand, or leave its stand;
    (b) for a Repeat Fire package, does that last, or does it go back to the package's target
    after a while? A package aiming at a marker (the Ultra-Luxe kitchen's) is worth the same
    look: does it fire at the marker in combat at all?

## Added 2026-11-11 (sandbox meals and seats, issue #2011)

113. **Does a sandboxer that may eat ever enter EAT, and does a free seat decide it?**
    (P3; actor-behaviour.md §5.5 Choosing step 2.) Under Sandbox packages that allow eating
    the original reported no EAT procedure: GoodspringsSettler01Ref
    (GSSettlerAAMSandbox6x16) and GoodspringsSettler03Ref (GSSettlerSandbox) over 24 game
    hours from 06:00 (AI observer, 2026-11-09), and none in about 240 actor-hours under
    Sandbox records (npc-behaviour-compare.md §6a) [observed]. NVForever now takes a
    sandbox meal only on a free seat and otherwise goes on with its choices [design,
    #2011]. Put an eating-allowed sandboxer whose area has a free chair (an interior with
    a table, e.g. the Prospector Saloon's sandboxers) through its breakfast, lunch and
    dinner windows (06:00-09:00, 11:00-14:00, 17:00-20:00) and log GetCurrentAIProcedure
    with the furniture it sits on: EAT on the chair says seated meals happen; SITTING only
    says the original's sandbox does not run meals as a procedure at all, and step 2 should
    go.

## Added 2026-11-11 (a sandbox's only object, issue #1505)

114. **Does a sandboxer with one seat and nothing else to use sit on it again, and when?**
    (P3; actor-behaviour.md §5.5, "The only object, used again"; packages.md §3.) NVForever
    lets the object just used come back once it is the only one open and the actor has
    stood or wandered for 60 s [design, #1505]. The GECK wiki documents
    `iSandBoxPreventRepeatedActionTime`, default 30: no repeated action on the same object
    until that many seconds have passed [documented: GECK wiki], without saying when the
    seconds start. In the original: an NPC whose sandbox area holds a single free chair
    (idle markers and wandering off or absent, outside meal and sleep windows), watched for
    about 10 minutes with GetSitting (or the furniture it uses) sampled each second: does it
    sit again, and how long after getting up (or after sitting down)? Never, a fixed wait
    near 30 s from either end, or random gaps each settle the number; record the package's
    flags and the cell's other objects.

## Added 2026-11-11 (a guard's wander, #2027)

115. **How does a Guard package with a location radius wander: how often, how far, and
    round what?** (P3; actor-behaviour.md §5.5, packages.md §3.) The wiki says only that a
    guard whose location has a radius over 0 "will wander within that radius" [documented:
    GECK wiki, Guard Package]. The 2026-11-10 capture (issue #2027) saw two placed
    GSSettlerAAM on DefaultGuardStayAtPosition01 (radius 128) moving in 20 % and 39 % of
    the samples, one walking 4,212 units in 317 s, spread 235 × 206 units, at most 152 from
    its average spot [observed]. NVForever wanders as a sandbox does: a random point of the
    navmesh within the radius, a walk there, a pause of 4 to 12 s, the walk not kept inside
    the circle [design, #2027]. Record a placed (editor-located) settler given that package,
    10:00 to 13:00, sampling position and IsMoving every 10 game loops: the pauses' lengths,
    the walks' lengths, whether any sample lies past the radius from the editor location
    (and by how much), and whether a radius round a reference that moves (the player, a
    walking actor as the location) follows it or stays where the package began.

## Added 2026-11-11 (fidgets with the weapon away, issue #2026)

116. **How often does the original ask its idle tree for a standing NPC's fidget, and
    what cuts one short?** (P2; idles.md §11.4, §11.3.) NVForever asks every 2 s
    [design, not calibrated]; with `NPCGeneralIdles`' random < 1 that gives a general
    fidget about every 200 s, where the 2026-11-10 capture saw a travelling merchant
    (GeneralIdleFaction only) begin 3 in 360 real s [observed]. The GECK wiki says an
    actor picks an idle when its "energy level timer has run down", and that actors
    with low Energy idle more often [documented: Idle Animations], with no numbers.
    Place NVMerchantMale and GSSettlerAAM by AddScriptPackage
    DefaultSandboxNoMoveCurrentLocation200 (as in #2026), sample IsIdlePlaying and JIP
    GetPlayedIdle every game loop for 20 real minutes, and count the general fidgets
    begun per minute; repeat with an NPC of low and of high Energy (AI data) to see
    whether the rate moves. In the same run, walk the player into one of them mid-fidget
    and start a conversation: does the fidget stop, or play out?

## Added 2026-11-11 (NPCs at doors and the extender's OnActivate, issue #2123)

117. **Which doors fire the extender's OnActivate for an NPC, and in which order?** (P3;
    doors.md §5.2.) NVForever dispatches it at every door an NPC walks through, before the
    door's block, once per asking [documented: NVSE Event Handling; design for the rest].
    With an unfiltered OnActivate handler logging both references and the time: (a) a door
    with no script at all on a resident's way (the Goodsprings gas station's door, base
    OffDoorMetalSmL01, which Ringo uses): does an event name the NPC? (b) a door whose
    block refuses NPCs (VStripGate, GomorraElevatorDoor): one event per crossing, or one
    per try? (c) at a door whose block calls `Activate` for NPCs, one event or two? (d) a
    `Begin OnActivate player` door (IndDoorSmAnim01): does the block run for the NPC (a
    variable it sets), as the GECK wiki's OnActivate page (Bugs) has it, or not, as
    NVForever does?

## Added 2026-11-12 (MCM's list functions, issue #976)

118. **Why did `SetMCMFloatMass 1 36 37` leave ArrowUp alone?** (P3; menu-xml.md §10.2.)
     In the original `SetMCMFloatMass 1 36 37 "depth" 33` set Option36's depth but not
     ArrowUp's, though ArrowUp is MCM_Options' 37th child in MCM.xml, while MCM's own
     `5 24 33` reaches MCM_ModList's 24th to 33rd children [observed: job mcm976e;
     hypothesis: the function counts only children of the first row's kind, or its range
     stops at a row without the trait]. NVForever counts every child. To check, with the
     pause menu and MCM's window open: `SetMCMFloatMass 1 37 38 "depth" 33` and `1 36 37
     "visible" 0`, reading ArrowUp, ArrowDown and Option36 before and after in one script;
     and `5 23 24 "_enable" 1` reading Mod23 and SubMenu1.

## Added 2026-11-12 (the versions the built-in extender plugins report)

119. **What do GetPluginVersion and IsDLLLoaded answer for the plugins NVForever builds
    but never read a version of?** (P2; ext-version.md §5, §6.) NVForever now reports SUP,
    JIP PP LN, AnhNVSE, NX, UIO, CacheUI and hNVSE as installed, the first five at a
    placeholder version (9999) because no page names one and none was read [design]. With
    each plugin installed, one runtime query per name: `GetPluginVersion "SUP NVSE Plugin"`,
    `"JIP PP LN"`, `"AnhNVSE"`, `"Extensions for NVSE"`, `"UI Organizer Plugin"`, `"CUI"`,
    `"hNVSE"`; and for the plugins whose registered name no page gives (ITR, One111,
    ppNVSE, vheyes-nvse), the name each prints in `nvse.log` and its version. In the same
    run `IsDLLLoaded` with each plugin's file name without `.dll`, for
    `fnv_game::nvse_plugins::DLLS`, which still lists only the five plugins of the first
    probe install.

## Added 2026-11-12 (created actors' editor location, issue #2278)

120. **Where is a created actor's editor location?** (P2; crates/fnv-ai/src/goal.rs
     `editor_place_of`.) NVForever takes the cell and position where a PlaceAtMe'd or
     spawned reference was created as its editor location [hypothesis], so its packages
     near the editor location (DefaultSandboxEditorLocation256/512, the player reports'
     spawned coyotes and Legion recruits) have a place. To check: PlaceAtMe a CrCoyote
     (DefaultSandboxEditorLocation512) beside the player at the Goodsprings coyotes' spot,
     MoveTo it 2,000 units off, and sample its position for 60 s: does it walk back to the
     creation spot and wander there, or sandbox where it now stands? And
     `GetStartingPos X` on it before and after the move.

## Added 2026-11-12 (dialogue zoom lock, issue #2254)

121. **Does the original keep the camera on the speaker throughout dialogue, and can
     the mouse turn the view?** (P2; dialogue-play.md §8.) Players report a locked camera
     [hypothesis: player reports, not captured]. NVForever's dialogue zoom locks the view
     and follows the face; without the zoom the player looks around freely [owner
     decision 2026-11-13, #2254]; the split stays whatever the answer, which informs the
     zoom's framing and follow. Record a conversation held beyond five seconds, first with
     no input as the speaker finishes turning and idles, then moving the mouse during a
     line and while choices wait.
     Does the view keep following the face as the actor steps or walks? Does mouse
     motion move only the cursor, or also the view? Repeat across a choices-to-line
     transition without touching the mouse; record any camera motion on cursor capture.

## Added 2026-11-12 (Lost fighters, issue #2010)

122. **Why do some fighters remain in COMBAT for minutes without searching?** (P2;
     alert.md section 2.) [observed: issue #2010 capture, 2026-11-10] Three placed Fiends
     detected the player for 4.5 real seconds; after a 2,500-unit player move two
     remained IsInCombat 1, procedure COMBAT, for the remaining 235 real seconds,
     stationary with no reported target; the third ended combat immediately. In
     contrast, eight actors ended combat 1.1-11.9 real seconds after detection loss
     [observed: npc-behaviour-compare.md section 11]; later timer experiments are in
     sections 15.6 and 16.1. Repeat with hostile actors placed by the data, both walking
     away and teleporting, recording GetDetected, GetCombatTarget, IsInCombat,
     procedure, positions, combat allies and the timer settings. Do stationary survivors
     retain a target or an ally's fight, and do any actually complete searches? Our
     two-sweep design and 300 s stall bound do not assert parity with those durations
     [design, #2010]; relate any answer to observer check 6's target-retention question.

## Added 2026-11-13 (the extender lab's engine sets, #2131, #2133, #2135)

From the owner's save nvfairport (the airport runway, the player at 37000, −117160, 6057,
heading 75) with NVFExtProbe.esp loaded, as the jip-probes sets run.

123. **Does an explosion the player places hurt the player, and what decides it?** (P3;
     jip-hit-info.md §2, #2131.) The jip-hits set registers eleven SetOnHitEventHandler
     lambdas on the player, then `player.PlaceAtMe MineFragExplosion` (0x7078B). In the
     2026-11-08 capture (the golden) nothing fired and Health stayed 1200; on 2026-10-26 the
     same explosion fired twice and took 100 [observed]. NVForever: one hit, about 75 damage.
     To check: Health and OnHit right after and 60 frames later; then the same explosion
     placed from a marker beside the player instead of from the player [hypothesis: who
     places the explosion owns it, or it went off after the job's 35 frames].
124. **The player's pitch at the save's place** (P3; #2135.) GetReticleRange read 9384 in
     the original, and after `player.SetAngle X 80` GetAngle X read 22.7151, while the
     save's place rotation x reads 0 [observed: fnv_save::fos]. To check: player.GetAngle X
     right after loading, with no input; then after SetAngle X 80 and SetAngle X −80 [hypothesis:
     the camera's pitch is kept outside the place record, or SetAngle X is clamped].
125. **What did GetP2PRayCastRange pass through above the runway?** (P3; #2135.) From 100
     above the player to 1000 below, it read 998 in the original; NVForever hits the runway
     (100). To check: the same cast 200 units off the runway over plain ground [hypothesis:
     the cast leaves out the runway's static as it leaves out terrain, engine1].
126. **The case of GetPlayingAnimSequencePaths' paths** (P3; #2133.) On the player the
     first slot reads `Characters\_1stPerson\mtidle.kf` [observed]: mixed case, no
     Locomotion folder. To check: the same on two NPCs (one standing, one walking) and on
     the player in third person, beside the clips' names as the BSA stores them and as the
     IDLE records spell them.

## Added 2026-11-13 (voice types without Allow Default Dialog, #2378)

127. **Do a quest's conditions open its lines to a voice type without Allow Default
     Dialog?** (P1; #2378, actor-flags.md §9.) Ours since #2378: yes [hypothesis from the
     data: ED-E's greetings, the securitrons' "Engaging target." and the first-generation
     super mutants' death lines have no conditions of their own, only their quests name
     the speaker]. One consequence to check: the Generic quest's conditions list
     FemaleAdult11, so Julie Farkas (Old Mormon Fort) now has Generic's Goodbye
     0x00177392 ("...") at her top level, where ours offered none before (#1595). Talk to
     Julie Farkas and record her topic list: is a Goodbye offered at the top level, and
     what does she say on it? The same for one actor each of MaleAdult10-12 and
     FemaleAdult12 without a Goodbye line of their own. If she offers none, the gate
     takes a quest's GetIsID (and what else?) but not its voice-type list.

## Added 2026-11-14 (map markers without XRDS, #2338)

128. **How near must the player come to a map marker without `XRDS` to find it?** (P2;
     pipboy.md §5.3, hud.md §11, #2338.) 65 of WastelandNV's 186 named markers carry no
     radius (Coyote Den 0x00154A69, the four Powder Ganger Camps 0x0015354F-0x00153552).
     Ours since #2338: `iMapMarkerRevealDistance`, 1000 when no plugin sets it [documented:
     the GECK wiki's list of FNV defaults; hypothesis: that this is the distance]. To check:
     walk straight towards Coyote Den from the south-west (flat ground) and from two other
     sides, and note the player's distance from the marker (GetDistance to the marker
     reference, or the player's GetPos) when "You have discovered" appears: about 1000
     expected. Then `setgs iMapMarkerRevealDistance 3000` before approaching a Powder
     Ganger Camp, and see whether the distance follows.

## Added 2026-11-14 (bloatflies' distance, shots and bite, issue #2351)

129. **How does a bloatfly fight the player: its distance, its shots and its bite?** (P2;
     combat-ai.md §12.6 and §19, #2351.) A player reports that in the original bloatflies
     keep their distance, fire on a cooldown and bite when in range [hypothesis: a
     player's report in #2351, not yet captured]. CrBloatFly 0x0009189C carries
     Bloatfly1HP (Min Range 100, Max Range 250, semi-auto delay 0.1-0.75 s) and has its
     own attack (DATA damage 7, reach 32); its style CSBloatfly is Ranged Only
     [documented: fnvtool esm decode, FalloutNV.esm]. Ours since #2351: it holds about
     225 units away and steps back only from a target within 100, to 164 [design, #2351];
     it never bites (its folder has no h2hattack clips) [hypothesis: whether and how it
     bites]. AI-observer capture: one bloatfly attacking the standing player, then the
     player walking in on it: its distance over time (about 10 samples a second), the
     time between its darts, whether it backs away and from how near, and whether it
     bites, from how near, and which clip plays then (`1hpattackright_2`, which moves it
     about 30 units forward, is one candidate [hypothesis]).

## Added 2026-11-14 (shots through the Transparent layer, #2340)

130. **Does a 9mm round pass a chain link on the plain Transparent layer (3)?** (P2;
     #2340, combat-rules.md "The world".) The road gate door 0x0008D5B8 (CLFenceGateRoad01,
     WastelandNV, -4576, -30616, 6819, not rotated) has its chain link on layer 3 east of
     the gate leaves (model x 45..246, 13..224 high); the gate leaves themselves are on
     Transparent Small Anim (28). Ours since #2340: every projectile passes layer 3
     [hypothesis from the GECK wiki "Collision layer": "nothing can pass through except
     projectiles"]. To check: stand at (-4420, -30766), face north, look about 10 degrees
     down and fire one 9mm pistol round: does it strike the ground beyond the link, or
     leave a metal impact on the link? Then the same at the gate leaves (28, where a 9mm
     should stop) as a control. The same question for a utility room's ceiling grate
     (layer 3 at 183 units, e.g. utlrmwall01.nif), shot from below.

## Added 2026-11-14 (the drinking idle, #2349)

131. **How does the original play drinking from placeable water?** (P2; #2349, idles.md
     §16.) Drink from Goodsprings' water pump, once in first person and once in third
     person. Note: how long the animation runs (ours: the clips are 2.37 s and 2.70 s);
     whether movement and mouse look are held while it plays (ours holds neither); and
     when the healing and the rads apply (ours: at the activation, before the animation),
     read from the health bar and the RAD meter frame by frame. Ours until then:
     [design, #2349].

## Added 2026-11-14 (a script moving the clock, issue #2353)

132. **Where are actors after a script sets GameHour?** (P2; packages.md §9.) NVForever,
     on a script's GameHour write of more than an hour, has everybody choose its package
     at once and puts the actors out of sight at their new package's place; those in view
     walk there [design, #2353]. To check, the issue's way: on a new game skip Doc
     Mitchell's last line, answer the Hardcore box and go through the door at once. Note
     the hour on arrival (GetCurrentTime every frame for the first ten seconds), and Easy
     Pete's place, GetCurrentAIPackage and GetSitting at arrival and 1, 5 and 15 s after
     the clock reads 8:00; then the same waiting inside for the window to turn light. Also:
     at 06:00 on the street by Easy Pete's porch, with him in bed in his house,
     `set GameHour to 8` from the console and the same samples: is he on his porch chair
     at once, does he walk out of his door, or does he stay in bed until his next choice?

## Added 2026-11-14 (Hidden Valley dust storm, #2450)

133. **Which way and how fast does the Hidden Valley dust storm blow?** (P3; #2450,
     activators.md §6.1.) Ours: all 18 dust sheets of `nvsandstormnight.nif` scroll their
     texture by SpecialIdle, U 0 to 2 in 4 s, along +U [hypothesis: the sign of the
     offset; every sheet of a property moves, as Gamebryo properties are shared by
     reference]. To check: `ForceWeather HVDustStorm 1` at Hidden Valley (-17863, -4814)
     at 22:00 with the storm references enabled (`000E18D7.enable`), two screenshots of
     the same view 1 s apart: do all the dust banks move, in which direction across the
     view, and about how far? Also whether the weather's own cloud layer 3
     (`NVCloudlight.dds`, ONAM speed 0) stands still there.

## Added 2026-11-14 (harmless Use Weapon hits, #2433)

134. **Does a Do No Damage hit run the target's OnHit block?** (P1; #2433,
     use-weapon.md §3.) Ours since #2433: yes, OnHit and OnHitWith run with the shooter as
     attacker, and nothing else happens (no Health, limb or fatigue damage, no hit line)
     [hypothesis from the data: G.I. Blues' thugs fall only in their OnHit block for
     Orris (SCPT 0x00118A77), and his three Use Weapon packages VFSOrrisShootThug01-03Package
     are Always Hit and Do No Damage, so the staged ambush goes on only if such a hit runs
     it]. To check: hire Orris and follow him into the ambush; record each thug's Health
     and fatigue before and after his shot, how many shots he fires at each, and whether he
     raises the revolver to aim first. Also whether a hit the target was not meant to take
     (another actor in the line of fire) runs that actor's OnHit too.

## Added 2026-11-14 (scripts of actors out of the loaded cells, #2445)

135. **Do the scripts of actors out of the loaded cells run, and how often?** (P1; #2445,
     `fnv_ai::process`.) Ours since #2445: an actor the AI processes out of the loaded
     cells (the cell buffer, interiors behind the loaded cells' doors, farther away) runs
     its GameMode block at each of its low-process updates (about every 18 s, or 120 s
     beyond the cell buffer) and once in each hour of a wait, when that block tests
     "not in the player's cell" (eight object scripts in FalloutNV.esm); the others run
     in the loaded cells only [hypothesis from the data:
     Beyond the Beef's Chauncey (SCPT 0x0011689F) warps to the bathhouse and re-evaluates
     his package only in a GameMode block that tests "not in the player's cell", and his
     casino patrol is Must Complete, so nothing else moves him]. To check: with the
     Matchbook note and objective 40 shown, wait in the steam room from 14:00 (by the
     Wait menu, and once in real time) and record when Chauncey appears (GetPos, his
     cell) and whether he fades in at VMS18ChaunceyBathhouseWarpMarkerREF. A second job:
     an actor with a script that counts GameMode runs (a variable bumped each run, read
     with GetScriptVariable) in a cell the player left, read after 1, 5 and 30 real
     minutes, in the cell buffer and beyond it.

## Added 2026-11-14 (blasts on corpses, #2346)

136. **Does a blast beside a settled corpse take its parts, and how many?** (P2; #2346,
     dismemberment.md §4.2.) NVForever now lets a blast reach a corpse whose ragdoll has been
     built and judges the one part facing it by the weapon's On Hit [design]. Two gates
     stand: a placed explosion or a mine with no actor owner has no source weapon and rolls
     nothing (`nvf_play::mines` keeps the weapon only for an actor-owned mine); an
     actor-owned mine reaches the check, where its No Dismember/Explode (every base-game
     mine, dismemberment.md §2.1) gives no odds without Bloody Mess and half the odds with
     it (§4.2). To check: kill an actor, let it settle, set off a frag
     mine and then a frag grenade next to it, without and with Bloody Mess, several times each.
     Record how often a limb explodes or is severed and how many parts go per blast. The
     answer fits the mine source-weapon gate (`nvf_play::mines`) and the On Hit rule for
     blasts; it goes into dismemberment.md §4.

## Added 2026-11-14 (the Boomers' barrage after Pearl, #2449)

137. **Does the barrage stop for a player standing in the field when Volare! starts?**
     (P3; #2449.) NellisArtilleryScript (SCPT 0x000FF29C) counts the player in on
     OnTriggerEnter and out on OnTriggerLeave, but passes over the player in both blocks
     once VMS15 runs or is done [observed: the record's source text]; VMS15 starts with
     stage 20, set by the end script of Pearl's Goodbye line 0x001050FD
     (VFreeformNellisNellisPearlTopic010) [observed]. So a player counted in before stage 20
     and still inside when it is set should be shelled on and on [hypothesis]. To check, on
     a save before Pearl's talk: stand in the field (trigger 0x0015A378) with `tgm`, then
     `setstage VMS15 20` from the console; walk out and back. Does the shelling stop, at
     once, on leaving, or never? And does a save and load inside the field reset it?
     Also, before Pearl: fast travel out of the field while shelled and back. Does the
     trigger run its OnTriggerLeave for a player who travels away (ours does since #2449,
     engine/nvf-play/src/script_trigger.rs `leave_reported`, [design]): does the barrage
     start afresh on the return, or never stop?

## Added 2026-11-14 (OnTrigger timing, #2455)

138. **How does an OnTrigger block count time?** (P2; #2455, scripts.md §5.) Ours: it
     runs once per reference inside each frame, only the first run of a frame gets the
     frame's GetSecondsPassed, and no trigger block runs under a menu or message box
     [hypothesis]. To check: start Ibsen's purge (or set HVHuntingVirusTriggerREF's
     VirusTimer to 20 and VDialogueHiddenValley's HuntingVirus to 2) with Ibsen and the
     two scribes in the room and record VirusTimer every frame; then again with a
     message box open for 5 s (ShowMessage) in the middle: does VirusTimer move while the
     box is up, and does Ibsen's 10-seconds line come 10 s of game time after the start?

## Added 2026-11-14 (Goodbye lines between NPCs, #2451)

139. **Does a conversation between NPCs go on past a Goodbye line that links on?** (P2;
     #2451, dialogue.md §5.1.) Ours: yes, a Goodbye line ends it only when it has no
     Link To or follow-up [hypothesis]. To check: with Veronica hired, walk into
     VHiddenValleyVeronicaEnterTrigger (0015B6D7) at the Hidden Valley bunker and record
     which of INFO 0015B6D1-0015B6D4 are said, by whom, and when the inner door
     HVOuterLockedDoor opens; or watch Jason Bright's speech at REPCONN (are parts 5-9,
     INFO 000CD8A9 and after, said after part 4, 0008D401?).

## Added 2026-11-14 (power-armour footfalls, PR #2430)

140. **What sounds at a power-armour wearer's footfalls?** (P3; audio.md §10.1.) The data
     has `FSTArmorPowerWalkL`/`R` beside the ground's `FST<material>` sets, and no record
     links them to anything [observed]; the Power audio template's run sound
     (`FOLArmorPowerRun`) names `fx\fol\powerarmor_walkrun\`, a folder no archive of
     the base game, its DLCs or the Blue Moon list holds [observed: archive listings,
     2026-11-14]. NVForever plays the ground's footstep and the template's `(Armor)`
     rustle and nothing for the missing folder. Walk and run in T-45d power armour on
     dirt and on concrete, first and third person, and record which SOUN records play
     at each footfall (the sound log): does `FSTArmorPowerWalkL`/`R` play, instead of
     or beside the ground's step, and does anything play for the run rustle?
     **Answered for NPCs 2026-11-14** (#2541; Blue Moon sandbox stock game, save
     nvfairport on concrete, `IsSoundPlaying <rec> <actor>` sampled on 130 frames)
     [observed]: Doc Mitchell walking in ArmorPowerT45d read FSTArmorPowerL 49 and R 95
     frames beside FSTConcSolidWalkL 59 and R 95; without armour only the ground step;
     LvlNCRHeavyTrooper (ArmorPowerNCRSalvaged, Heavy but not Power Armor) FSTArmorL/R
     beside the ground step; FSTArmorPowerWalkL/R never; standing still nothing. So the
     armour's footfall plays beside the ground's step, power armour's by the Power Armor
     flag and heavy armour's by Heavy, and IsSoundPlaying sees the game's own footsteps.
     Still open: the player (an unattended run could not walk the player), first
     person, running, dirt, medium armour.

## Added 2026-11-14 (the Fort's placed slave backpacks, #2439)

142. **Are the Fort's two placed slave backpacks drawn, and where?** (P3; #2439,
     simulation.md §3.) References 0x0013732C and 0x0013732D (NVSlaveBackpack, ARMO
     0x00127C6A: Non-Playable, no ground model MOD2) are placed at (92047, 45622, 8260)
     and (92159, 45555, 8608), by the blacksmith's grinding wheel, 565 and 913 units above
     the ground [observed: the data]. Ours draws neither since #2439. To check: stand at
     (89242, 46819, 8095) at 17:00, look south-east over the camp; are two backpacks in
     the sky, on the ground by the grinding wheel, or nowhere? `prid 0013732C` then
     `getpos z` and `getdisabled`.

## Added 2026-11-14 (Ranger Station Charlie's door and tripwire, #2420)

143. **What does a sprung tripwire look like after a cell load, and does it fire again?**
     (P3; #2420, activators.md §5.) In RSCharlieNVDESTROYED (after the station's trigger
     has enabled CharlieEnableParentREF 0x00126F4E), walk through the tripwire 0x00084246
     behind the left door (OffDoorSmR02a 0x000CD068), leave the cell and come back. Is the
     wire shown sprung or whole, does it fire its shotgun (0x00084247) again, and does
     anything explode on a second pass (the bottle-cap mine 0x0008424C, the frag mines
     0x00084257 and 0x00084258 by the entrance)? Ours since #2420: shown sprung, fires
     once.

## Added 2026-11-14 (HELIOS One's guard dogs and troopers, #2547)

144. **Do HELIOS One's guard dogs fight the troopers in the original, and what holds
     them if not?** (P2; #2547, factions.md §4-5, combat-ai.md §13.1.) The three dogs
     0x000A695E, 0x000A695F and 0x000A6944 (CrDogNCRAggressive 0x000A692B, "NCR Guard
     Dog", placed at about (25542, 1960), linked to patrol markers there) are in no NCR
     faction: their own factions are GeneralHostileToPlayerFaction (Enemy of
     PlayerFaction), CreatureHitDog, DogFaction and AnimalFriendFaction; the template
     CrDogNCR, which is in ArmorNCRFactionNV and VNCRSnifferFaction, does not supply
     the factions or the AI data (template flags Traits, Stats, Actor Effect List,
     Model/Animation, Base Data, Inventory, Script). They are Very Aggressive, Foolhardy,
     Aggro Radius Behavior on with radius 2,000 [observed: the records]. The troopers
     TestTrooperREF1-5 (HeliosExtLvlNCRTrooperGun: VNCRHeliosFaction, NCRFactionNV)
     stand about 2,800 units west at (22778, 1293). No faction of either side lists one
     of the other's, so they are Neutral both ways, and by the documented rule a Very
     Aggressive actor attacks Neutrals it detects. Ours: the dogs notice the troopers at
     2,800-3,200 units and attack, again and again [observed: the log of #2547]. To
     check, the player standing in the solar field for two game minutes (`tdetect` so
     the dogs leave the player out of it): on each dog `GetDetected TestTrooperREF4`,
     `IsInCombat`, `GetCombatTarget`, `GetDistance TestTrooperREF4`,
     `GetFactionRelation TestTrooperREF4`, `GetAV Aggression`; then `MoveTo` one dog to
     1,900 and to 900 units from TestTrooperREF4 and read the same. That tells apart: no
     detection at that range or through the yard's fence; an aggro radius that bounds
     who a Very Aggressive actor attacks on sight (a fight at 1,900 and none at 2,800);
     a relation that is not Neutral; or a fight in the original too.

## Added 2026-11-14 (unseen actors whose package place cannot be walked to, #2405)

145. **Where does the original put an actor nobody sees whose package place has no way
     to it?** (P2; #2405, packages.md §9, actor-behaviour.md "processing levels".)
     `GomezREF` (0x0015A02E) is placed in a side room of SafehouseNCRINT at (7691, 742,
     6127), on a navmesh island of two triangles; from a new game the quest script holds
     `vSafehouse.GomezGoToSleep` at 1 on every day but his visiting day, so his Leave
     package (Travel to `GomezDoor` 0x0015A02B at (2811, 951, 6438), on another island
     of the cell, no door between) is the one that is valid [observed: the records,
     `fnvtool nav path`]. Ours: he cannot walk there and stays, reporting it every hour.
     To check, on a new game, far from the safehouse, after three game hours and again
     after a day: `GomezREF.GetPos x`, `GetPos y`, `GetUnconscious`, `GetCurrentAIPackage`,
     `GetActorProcessingLevel`. At (2811, 951) or at `GomezSleep` and unconscious: the
     original places unseen actors at their package's place (and the door trigger
     0x0015A031 then fires). Still at (7691, 742): it leaves them. The same for
     `AtticusREF` (0x0015A04E, SafehouseLegionINT). And `SSHQMalcolmHolmesREF`
     (0x0010651A): after he has met the player outdoors and walked out of sight, is he
     back in RandomEncounterNPCs at (1825, 2079, 6976)?
     Ours since #2405 (packages.md §11): out of the high process, an actor whose
     package's place no door at all joins to where it stands is put there; in the loaded
     area (the player in the safehouse) it is still set aside every hour.

160. **Where is Loyal once Volare! has failed, and can the player reach him?** (P2; #2566,
     navmesh.md §4 "Disabled load doors".) Kill Argyll (NellisArgyllREF) at Nellis
     before the B-29 is raised: VMS15 stage 116 fails Volare!, which GetQuestCompleted
     reads as 1, so Loyal's first package, NellisLoyalB29SandboxAIPackage, wants him in
     NellisHangar1b, whose only doors (0x001093DB/0x001093DC) stay disabled until the
     B-29 is up. Where is Loyal then (NellisHangar2, NellisHangar1, NellisHangar1b), and
     where does Render Unto Caesar's Boomer-leadership marker point? [hypothesis: the
     original does not move him through a disabled door; ours keeps him where he is.]

## Added 2026-11-14 (observer batch o30: Say / SayTo on a speaker already talking, #1858)

Answered for the talking state (scripts.md §7.7): a Say or SayTo on a speaker that is
already talking takes over at once, same topic or another; nothing is queued or ignored
[observed: o30_say, o30b_say]. Open:

161. **Do the voice and the subtitle restart with each call, as the talking state does?**
    (P2; the Strip gate's every-frame SayTo, scripts.md §7.7.) Needs the game in front (its
    sound and subtitles stop without focus), so the owner: stand in the gate kill zone at
    the Freeside -> Strip gate (VFSVegasAnnihilationVolumeREF, FreesideWorld near
    -6362, -9779) without a passport, in god mode (`tgm`), for about 30 s. Is one Gatekeeper
    line heard and shown at a time, each to its end, or does the voice stutter and the
    subtitle flicker? The campaign `o30c_say` (tools/capture/observe) does the same by
    script with shots, if run with the window in front.

## Added 2026-11-15 (Cottonwood Cove's explorers, #2548)

162. **Does a fixed speaker give way to a faction condition?** (P2; #2548, dialogue.md,
     the open case of check 29.) "Halt! What business have you in Cottonwood Cove,
     outsider?" (0x0013AC97, VFreeformCottonwoodCove) names NVCCLegionaryGuardAAMExplorer
     as its fixed speaker and has only `GetInFaction VCaesarsLegionCCoveExplorerFaction
     == 1` [observed: the data]. At Cottonwood Cove with the Mark of Caesar, `prid
     001364C3` then `disable` (the AAM explorer), and let NVCCExplorer2REF (0x00138A92)
     walk up. Does he say the Halt line, voiced or silent, with the Mark of Caesar
     choices, or "Move along."? Ours since #2548: the Halt line, in the AAM explorer's
     voice [hypothesis].

## Added 2026-11-15 (Very Aggressive actors, Neutrals and the aggro radius, #2547)

163. **Does the aggro radius bound whom a Very Aggressive actor attacks on sight?** (P2;
     #2547, factions.md §4; check 144 asks it of HELIOS One's dogs.) Ours since #2547: a
     Very Aggressive actor with Aggro Radius Behavior on attacks a Neutral non-player
     actor only inside its aggro radius [hypothesis]. A second pair that tells it apart
     from the HELIOS fence: `player.placeatme CrDogNCRAggressive` (Very Aggressive,
     radius 2,000) in the open at Goodsprings (`tdetect`), then `MoveTo` it 2,600 and
     then 1,500 units from Sunny Smiles (SunnyREF, Neutral to the dog). At each distance
     for 30 s: `IsInCombat`, `GetCombatTarget`, `GetDetected SunnyREF`. A fight at 1,500
     and none at 2,600 (with detection at both) agrees with ours; a fight at both says
     the bound is wrong. A spawned actor may start fights at once where a placed one
     waits (aggro.rs, check 2); if the spawned dog fights at both, `MoveTo` one of
     HELIOS's placed dogs instead (check 144).

## Added 2026-11-15 (That Lucky Old Sun's ARCHIMEDES strike, #2544)

164. **How does ARCHIMEDES's armed strike play out?** (P3; #2544, use-weapon.md §2.) With
     VMS03 at objective 70, `set VMS03.bArchimedesArmed to 1` and activate the Reflector
     Control Panel 0x000E07DF on the observation deck between 9 and 15 h. Note the time
     from the activation to the first trooper's death and between deaths (TestTrooperREF1
     0x000E35D5, then 0x000E35D4, 0x000E35D1, 0x000E35D3, 0x000E35D2: `GetDead`), whether
     ARCHIMEDESMarkerREF2 (0x000E07DD) visibly turns between targets, and whether the
     troopers fire at the player before the first shot. Ours since #2544: the first
     death about 37 s after the activation, one every 1-4 s after, the turret turning
     to each [hypothesis: the script's timers, ours turns at the combat rate].

## Added 2026-11-15 (companions outside Hardcore, #2623)

165. **Is an unflagged teammate knocked out, not killed, outside Hardcore?** (P2; #2623,
     unconscious.md §2.2a.) Ours since #2623: outside Hardcore the player's teammate is
     spared death as an essential actor, though no companion's ACBS has Essential; in
     Hardcore it dies [hypothesis]. With Hardcore off: hire a companion who is not
     flagged (Cass, or Easy Pete given `SetPlayerTeammate 1`), then `DamageAV Health
     9999` and, on another try, `KillActor` on it. Each time read `GetDead`, `IsEssential`
     and `GetKnockedState` for 15 s and whether it gets up and is still a teammate
     (`GetPlayerTeammate`). Then the same with `SetHardcore 1`. Knocked out, up again and
     still hired outside Hardcore, dead in it agrees with ours; IsEssential reading 1 for
     the teammate says the engine counts teammates as essential by IsEssential too.

## Added 2026-11-14 (actors at thin walls, #2511)

166. **Which thin walls can a person-sized actor and a small creature cross?** (P2;
     #2511, physics.md §6.9.2.) [design] NVForever uses a 56° actor slope limit, below
     the cone face's roughly 57.7°, to cross a 24-unit zero-thickness wall and block
     25/26/28, with the same 24-unit step height for people and small creatures.
     Original actors blocking the higher walls is [hypothesis]. To check: in a
     controlled test cell with a level floor, lead a person-sized actor and a small
     creature straight into thin walls of 24, 25, 26 and 28 units at walk and run
     pace, without jumping. Record whether each crosses, the feet heights through
     the crossing or stop, and whether it first detours; keep a clear run-up and
     prevent routes around the ends. Also check a 46–49° ramp so the actor's slope
     behavior can be separated from its stepping over an edge.

## Added 2026-11-14 (NPC launcher round visibility, #2512)

167. **When does an NPC launcher show its loaded round?** (P2; #2512,
     animation.md section 9.7.) [documented: issue #2512 and animation.md section 9.7]
     The supplied missile launcher/Fat Man/dart-gun clips hide `##NifRound` in their
     attack and show it during reload; the third-person launcher aim has no round
     track. [hypothesis] The original retains the attack's hidden state through aim
     until reload shows it, and restores the model default on equip/holster.
     [design: #2512 settled owner decision] NVForever implements that provisional
     rule. Record an NPC firing and reloading each weapon, then interrupt before
     reload, wait in aim, and holster/draw again. Capture when the round disappears
     and returns, including a weapon redrawn before reload. No original capture was
     made in this writer run.

## Added 2026-11-14 (Oliver's rigid cap frame, #2529)

168. **Does Oliver's cap follow the full head frame, idle and speaking?** (P2; #2529,
     actors.md §1.2.) [documented: #2529 writer brief, fnvtool FalloutNV.esm +
     Fallout - Meshes.bsa] `VHDGeneralOliver` 0x001300A9 carries `NVGenOliverCap`
     0x00134490; its worn `GenOliverCap.NIF` is unskinned, identity-root, with no
     bone-naming extra data and vertices centred about +13 along X. [hypothesis]
     It is authored in `Bip01 Head`'s local frame. [design, #2529] NVForever preserves
     that frame when the bounds pass `BONE_LOCAL_HEAD_ALIGNMENT` (0.9), retaining
     skeleton-axis attachment for other layouts. Check the original while Oliver is
     idle, entering conversation, looking sideways/up/down while speaking, and leaving
     conversation. Compare the cap's centre and orientation to the head throughout;
     also compare a cowboy hat, beret and helmet using the skeleton-axis layout from
     actors.md §1.2. Record which heads/items clip or shift, and the valid greeting's
     quest/stage conditions for a repeatable conversation smoke setup. No original
     capture or game run was made in the writer session.

## Added 2026-11-14 (bloatfly side clips and combat wings, #2352)

169. **Do bloatfly side darts move the actor, and do wings beat under combat clips?**
     (P2; #2352, actors.md §§8.2–8.3.) [documented: supplied issue #2352 clip-root
     survey] `mtleft.kf`/`mtright.kf` reach 225/243 units sideways before returning
     to net travel 100. [hypothesis] NVForever provisionally removes the full drawn
     root x/y excursion for moves past a 1.5 peak/net ratio; whether the original
     moves the actor by that root is unknown. [design] Capture a bloatfly fighting
     for about 20 seconds, including sideways movement, aim, firing and hover/special
     idle. Record its actor x/y and body position frame by frame through a side dart:
     does the actor follow the outward and return root motion, or does the body stay
     at the actor while it strafes? Also compare wing beats during ordinary idle,
     forward flight, aim, attack and hover, especially bones absent from the upper
     clip. [hypothesis] NVForever keeps the set's idle on bones the active clips omit;
     confirm whether those beats continue and whether their phase restarts on a
     transition. Record clip names/times with the capture where available.

## Added 2026-11-14 (actor stealth fields, #2557)

170. **How do cloaked Nightkin and nightstalkers look and change state?** (P2; #2557,
     refraction.md section 5.) [observed: owner's 2026-11-14 REPCONN playtest, issue #2557
     comment] Nightkin there were drawn plain even as their uncloak sound/effect played.
     [design, #2557] NVForever now refracts loaded living actors at Chameleon >= 1,
     restoring the body, outfit and weapon when the value drops or the actor dies.
     [hypothesis] The player's existing 0.03 screen shift and 2 s shimmer provisionally
     apply to other actors. To check in the original: at REPCONN and in the caves near
     Jacobstown, record the outline, weapon and outfit while cloaked, during an attack,
     after combat and on death; compare the bend/shimmer to a player's Stealth Boy.
     Check that Jacobstown's residents stay plain. Also record whether nightstalkers
     cloak, which encountered variant does so, and under what conditions; the owner's
     comment leaves that uncertain, and no nightstalker record facts were supplied.

## Added 2026-11-14 (helipad ladder pair, #2528)

171. **Which helipad ladder faces are usable, and can the upper endpoint be targeted
     from the deck?** (P2; #2528, cells-refs.md §5.) [documented: supplied writer
     brief #2528 round w4, fnvtool ESM decode of FalloutNV.esm] The nearly coincident
     lower pair is teleporting `00164C64` (`VHDExteriorLadder`) at
     (67942.66,19400.51,4663.61), z rotation 3.442, and plain `00164C65` (DOOR
     `00060730`, `UtlHallBgEndSewerExLad01`, FULL Ladder) at
     (67941.33,19400.33,4664.01), z rotation 0.300, with no XTEL/XATO/XESP or
     reference flags. [observed: supplied main-session run 2026-11-14 23:14,
     `ov15/smoke-2528b/*.log`] Ours at spot 3, feet near (67950,19440,4802),
     selected plain `00164C65` at yaw 236, pitches 0/-30/-60, drawing Open / Ladder.
     [hypothesis, #2528] Ours retains ordinary non-teleport Open/Close toggling for
     that member; whether the original offers it at all is unknown. To check:
     approach this face from that landing and the opposite face from the south
     (the separate lower smoke approach is near (67942.7,19320.5,4806)). Record
     actual feet/view, targeted reference (distinguish the two IDs), prompt or its
     absence, and the result of pressing E twice: travel, visible movement, sound,
     state-only toggle or nothing. Does the plain back-to-back ladder have a prompt
     and activation, and does the opposite face select the teleporting member?

     [observed: same supplied run] Only original sweep looks 11/23 on the deck
     targeted upper `00164C63`: from (67900,19440,5101) yaw 121, pitch -60, and
     (67915,19385,5101) yaw 54, pitch -60. Other sampled deck looks hit deck STAT
     `0012CE4B` or nothing. To check in the original: try these views and small
     yaw/pitch changes, and look straight down from the deck near the upper
     endpoint (67939.5,19439.3,5028). Record target ID, exact prompt/absence and E
     result, with actual feet and view. Is the top usable from the deck, only from
     a narrow angle, or not there at all? Compare with activating the lower
     teleporting member as a control. [design, #2528] Do not infer original
     behavior from NVForever's successful teleport smoke scenarios.

## Added 2026-11-15 (a script's Disable of the player, #2617)

172. **Can a script disable the player, and can a disabled player use a scripted load
     door?** (P2; #2617, doors.md §6.) VHDIntakeTowerLegionaryDespawnScript (SCPT
     0x0013CA2C) disables whoever enters its trigger (VHDIntakeTowerLegionaryDespawnTriggerREF,
     0x0013CA72) once VHDLegionBattle.IntakeTowerStrategy is 2 [observed: the data].
     In any interior: `player.disable`, then `player.getdisabled` (1 or 0?), walk, and use a
     load door, an unscripted one and one whose OnActivate ends in a bare Activate (the
     Power Plant 01 elevator, VHDODeckElevREF 0x0013151C, in HooverDamIntODeck). Then, on a
     save there, `set VHDLegionBattle.IntakeTowerStrategy to 2`, `player.moveto 0013CA72`
     and `player.getdisabled`. Ours since #2617: Disable of the player does nothing
     [hypothesis]; GetDisabled 1 or a door that stays shut in the original says otherwise.

## Added 2026-11-15 (faction disguises, #2611)

173. **Does the NCR armour fool the NCR when the player is an enemy of NCRFactionNV?**
     (P2; #2611, disguises.md §3.) Ours since #2611: a faction enemy flag yields to the
     disguise's friendship (ArmorNCRFactionNV, which every NCR soldier is in); only
     sniffers (VNCRSnifferFaction) within the pulse's 25 ft (about 533 units) see
     through [hypothesis]. At Hoover Dam by day, stand on the road at
     VHDAssassinationSetupREF (0x0012FCF0), `player.additem 00129254 1`, equip it, then
     `SetPCEnemyOfFaction 000A46E7 1`. For 30 s note whether the troopers and engineers
     near (VHDPreBattleNCRTrooper4REF 0x0014633D, VHDPreBattleNCREngineer02REF
     0x0014695E) attack, and read `VHDPreBattleNCRTrooper4REF.GetFactionRelation player`
     and `GetCombatTarget` on the rangers by the vertibird pad (VHDPreBattleRanger04REF
     0x0014694C, a sniffer about 2,000 units off). Then walk up to that ranger (under
     500 units, in sight) and note whether it attacks and the others follow. Nobody
     attacking until the ranger is near agrees with ours; everyone attacking at once
     says the flag beats the disguise (then the disguise works only through the
     reputation reset, reputation.md §4).

## Added 2026-11-14 (a murder's faction enemies, #2456)

184. **Which of a murder victim's factions does the player become an enemy of?** (P2;
     #2456, crime-karma.md §3.4.) Ours: every crime-tracking faction of the victim, once a
     witness who cares turned, except one whose members include enemies of each other
     (ArmorVCaesarsLegionFaction, which also holds the four NCR troopers of the Strip
     scene VStripSS3) [hypothesis]. To check, with a character neutral to the Legion: kill
     a Legionary in front of another (Cottonwood Cove), then read
     `GetPCEnemyOfFaction VCaesarsLegionFaction` and `GetPCEnemyOfFaction
     ArmorVCaesarsLegionFaction` (0x00134B6E); then enter the Strip by the north gate and
     watch the four troopers (VStripSS3Girl01REF/02REF, VStripSS3Watcher01REF/02REF) and
     the Securitrons. If neither flag is set, the original sets no faction enemy on
     murder and only the witnesses turn. **Answered by check 203** [observed, 2026-11-15]:
     both flags are set by the murder and both clear by themselves about 0.18 game hours
     later; the Strip part was not run. NVForever now flags every crime-tracking faction
     for 0.18 game hours (#2498).

## Added 2026-11-15 (Anders's head on the cross, #2471)

174. **Does a crucified actor turn his head to the player, and is he ever seen climbing
     onto the cross?** (P3; #2471, anders-cross-head.md.) At Cottonwood Cove, talk to
     Anders (VRRCAndersREF 0x001413C3, cross 0x0013A4BA) from the front and from 90° to
     his side. Note whether his head turns to the player in each, how far to the side it
     goes, and whether it stays tilted down. Then leave the cell and come back (and, on
     a new game, arrive by fast travel) and note whether he is ever seen getting onto the
     cross (`GetSitting` reading 2) or always found on it (3). Ours since #2471: the head
     turns up to 60° from his chest and up to 30° from the level; he is put on the cross
     at once when he stands at it [design].

## Added 2026-11-15 (Anders freed before Diane, #2472)

175. **Can Anders be freed before Diane sends the player after him?** (P3; #2472,
     anders-cross.md.) In the data the freeing choice (AndersFree 0x00142A8A) is a
     choice of AndersJackandDiane 0x00142A89 alone, a topic that is not top-level and is
     learnt only from Diane's lines 0x00146698 and 0x00144651. From a save that has not
     met Diane (`GetObjectiveDisplayed VMS46 10` reads 0), go to
     Cottonwood Cove and talk to VRRCAndersREF: list the topics offered, then pick each.
     Only Who did this to you? / the drugs topic / Goodbye agrees with ours; a choice
     that cuts him down says the original offers the topic some other way (then note
     which and whether `GetQuestVariable VMS46 AndersFreed` goes to 1).

## Added 2026-11-15 (Cachino finds the player in Gomorrah, #2473)

176. **How does Cachino's Find package bring him to the player?** (P2; #2473,
     packages.md §10.) From a game where VMS21 has not started, stand at the Brimstone
     bar on Gomorrah's main level (Gomorrah01), more than 1,000 units from Cachino's
     spot (7159, 1281), and ask a Gomorrah courtesan or guest where Cachino is
     (topic `vDialogueGomorrahGenericTopic005`; his ref 0x0011118D is enabled by that line's end script). Note the time
     from the end of that conversation to Cachino setting off, to his first line (his
     Say Once greeting INFO 0x001133A0, about the player's questions), and how far from the
     player he stands when he speaks (`GetDistance player` on him). Then once more with
     the player on the floor below (Gomorrah00): does he come through the doors to
     another cell? Ours since #2473: he sets off at once, walks to the player wherever
     they are in the four Gomorrah cells, and the conversation opens when he is within
     about 3x the arrival distance and can see the player [hypothesis]. Not seen
     coming, or coming only within some distance of his spot, says the search area is
     wrong; walking up and standing without a word says the talk is not the Find's.

## Added 2026-11-15 (Vulpes Inculta in Caesar's tent, #2560)

177. **Does a shared ally faction win over an enemy one?** (P2; #2560, factions.md §4.)
     FortVulpesInculta 0x00122B9A is in VCaesarsLegionFaction (an Ally of itself) and in
     ArmorNCRFactionNVEnemy, which VCaesarsLegionFaction names an Enemy [observed: the
     data]. In Caesar's tent (TheFortCaesarTents) at 21:00, after a load there: do
     Caesar, Lucius or the Praetorians ever go into combat with Vulpes? `prid 00121FF0`
     then `getfactionrelation 00122B9A` (2 ally, 1 enemy). Ours since #2560: 2, and they
     stay calm [hypothesis: the original agrees, as the owner never saw the fight].

## Added 2026-11-14 (children in fights, #2438)

178. **Does a child ever fight back?** (P3; #2438, combat-ai.md §12.2.) Ours, by the
     owner's decision: a child (a race with the Child flag) set on the player flees at
     once and never attacks. To check: `player.placeatme 0011E506` (a Legion child,
     FortChildCM), `prid` it and `startcombat player`; then attack one with fists. Record whether it
     draws a weapon, swings or runs, and its Flee line, if any.

## Added 2026-11-14 (quest markers several doors deep, #2458)

179. **Where does the original mark a target several doors deep, seen from another
     worldspace?** (P2; #2458, quests-dialogue.md §2, hud.md §11.) Ours: the World Map
     marks it at the door outdoors that starts the fewest-doors way into its interior,
     and the compass and Local Map at the door of the player's own place that starts the
     way there [hypothesis]. To check: with For the Republic, Part 2 (VMQ05) objective 10
     shown and the quest active, stand on the Strip (NCR embassy) and note where the
     World Map's quest marker sits (which door of Hoover Dam, or elsewhere) and which
     door the compass marker points at (the gate to Freeside, or another); then again
     from the Mojave outside Freeside and inside the dam's visitor centre. Also: does
     the World Map draw a second marker at the Strip's gate while the player is on the
     Strip?

## Added 2026-11-14 (allies of an actor given StartCombat, #2520)

180. **Do an actor's allies join a fight a script gave it against a player who did
     nothing?** (P1; #2520, combat-ai.md §10.) Ours: no; Assistance answers an ally being
     attacked (hit by the player, or fought by someone it did not start on), so a
     StartCombat on a player it would leave alone draws neither its allies nor an alarm
     [hypothesis]. Also ours: a Cowardly actor in a fight flees from its start, hurt or
     not [hypothesis from GECK wiki Confidence, "will always flee from combat"]. To check:
     in a game where VMS29a has not started, walk into the Silver Rush (FreesideSilverRush),
     stand still by the door and watch the intro to its end (INFO 0012423E, the customer's
     `startcombat player`): does Jean-Baptiste (0010D8E5) or a Van Graff thug draw on the
     player, what stage is VMS29a afterwards (`GetStage VMS29a`), and does the customer
     (00124121) run out without swinging? Then again, hitting the customer once as he
     runs: do the Van Graffs turn on the player then?

## Added 2026-11-14 (Mr. House's screens after his death, #2431)

181. **Is Mr. House's penthouse screen ever hostile?** (P3; #2431, actor-flags.md §11.)
     Ours: an Immobile, Unaggressive, Helps-Nobody, unarmed creature never fights
     [hypothesis]. To check: kill Mr. House in his chamber (after the terminal unseals
     it), go back up to the penthouse and look at the screen (Lucky38MrHouseREF
     0x001264C5): red crosshair, red compass tick, does `GetCombatTarget` on it read the
     player, does it turn? Then shoot it: does it take damage? The same for Festus
     (NVCRFestus) when shot.

## Added 2026-11-14 (Mr. House's chamber, #2425)

182. **Does a special idle replace the base idle on its bones, whatever the priorities?**
     (P3; #2425, animation.md §9.4.) Ours: an idle of a standing actor shows on every bone
     it drives, above the base idle (`mtidle.kf`), though Mr. House's sleeping idle (40)
     and opening (30) are under his base idle (50) [hypothesis]. To check: in
     Lucky38ControlRoom before the terminal is used, a screenshot of the chamber from the
     catwalk (sealed, House hidden?); then a Protectron playing `mtspecialidle_lookleft`
     (11 of its 35 tracks under its base idle's priority): does its whole upper body turn,
     or only the bones above the base idle's priority?

## Added 2026-11-14 (Lucky 38 elevator, #2424)

183. **Does AddScriptPackage of a package an actor has just finished start it again?**
     (P3; #2424, packages.md.) Ours: the same package added again after its goal was met,
     before the actor chose another, begins afresh; added while it still runs, nothing
     changes [hypothesis]. To check: on the Lucky 38 casino floor, talk to the elevator
     Victor (Lucky38VictorElevatorREF 0x0015725F), pick Stay Here, and talk to him again
     at once (within a few seconds): does he greet again and show "Choose A Floor"? And
     in the console, `Lucky38VictorElevatorREF.AddScriptPackage
     Lucky38VictorElevatorTalkToPlayer` twice, the second right after his greeting ends:
     does he greet twice?

## Added 2026-11-14 (talking activators with no talking actor set, #2454)

185. **Who speaks through a talking activator no script has given an actor?** (P2; #2454,
     talking-activators.md §2.) Ours: the one actor the load order's scripts name for it
     in SetTalkingActivatorActor (Ramos for HVIntercomREF), else the activator itself
     [hypothesis]. On a save from before the Hidden Valley bunker, get in without
     touching HVIntercomREF (sneak in), go through Ramos's escort, accept McNamara's
     ranger task: when the screen fades and the player stands at the inner door, record
     whether the dialogue menu opens, which INFO is said (Ramos's 000E3664 or none),
     and when the controls come back. Also `HVIntercomREF.IsTalkingActivatorActor
     RamosREF` just before McNamara's line.

## Added 2026-11-14 (Dialogue packages that control the target, #2453)

186. **Does a Dialogue package walk the player into its trigger location?** (P2; #2453,
     packages.md §3.) The GECK wiki says the target "will walk towards that location when
     the dialogue package starts" when Do Not Control Target Movement is off [documented];
     ours walks the player, once within the activate distance and in sight, to 48 units
     inside the trigger box at walking pace and greets there [hypothesis]. On a save before the Hidden Valley bunker, sneak past the
     intercom into HiddenValley01 and stop on the stairs about 300 units from Ramos's
     RamosWaitingMarker: record the player's position every frame, whether the controls
     are taken, the pace, and when the dialogue menu opens, relative to the box
     HVRamosTargetDialogTriggerREF (y from -6132 to -5408).

## Added 2026-11-12 (the player's marker on the compass)

187. **How does the compass show the custom map marker?** (P3; hud.md §11.) NVForever
    marks it only in the player's own interior or worldspace, holds it at the band's
    end when it is off the band (as a quest target), and draws it over the quest marks
    [hypothesis]. Place a marker on the Local Map 20 m behind the player, close the
    Pip-Boy and screenshot the compass; then walk through a door into an interior and
    screenshot it again. Held at an end or gone behind the player, and shown or not
    (at the door?) from the interior, settle both.

## Added 2026-11-13 (companions on the World Map)

188. **From how far does the World Map name a companion?** (P3; `pipboy::data::map_companions`.)
    NVForever writes a teammate's name where it stands on the World Map, in font 2, only
    4096 units (a cell) or more from the player, and not while it is knocked out
    [documented: Stewie's Tweaks' readme says there is a minimum distance, font 2 and no
    knocked-out companions; hypothesis: the distance, and that companions indoors are not
    shown]. Tell a companion to wait, walk away in steps of about 20 m, and open the World
    Map at each: the step where its name first shows gives the distance. Then send it home
    (an interior) and check whether its name shows at all, and where.

## Added 2026-11-13 (weapon poisons)

189. **How do weapon poisons work?** (P3; `fnv_game::weapon_poison`.) NVForever puts a poison
    (an aid item whose effects are all Hostile and Detrimental [documented: GECK wiki
    Ingestible]) on a Melee Weapons or Unarmed weapon in hand for 5 landed hits, the same
    poison used twice to confirm [hypothesis: which weapons, how many hits; the question
    is ours; the original asks, Stewie's Tweaks' readme says]. With a knife in hand, use
    Bleak Venom: note the prompt's words and buttons. Then hit a target and count the hits
    until the poison's effect stops coming (watch its Health or the Pip-Boy's weapon
    card). Try it with a pistol in hand too: refused (sPoisonNoWeaponMessage) or not?
190. **Where does the grenade indicator stand?** (P3; `nvf_play::grenade_indicator`.) The menu
    files give its picture (`template_explosive_indicator`, an arc with a point) and an
    empty `Explosive_positioning_rect` the game's code moves [documented]. NVForever puts
    it on the grenade upright while the grenade is in view, else on the view's edge
    turned to point at it, for a grenade whose blast radius + 64 reaches the player
    [hypothesis: all of it]. Throw a frag grenade near your feet, then one behind you, and
    take a frame of each: where is the mark, is it turned, does it show for one landing
    far off, and does it show for your own grenade?
191. **What does the idle vanity camera do?** (P3; `nvf_play::vanity`.) The master sets
    `fVanityModeAutoDelay` to 120 [observed]; NVForever takes it as the seconds without
    input before the camera circles the player, turning 0.15 rad/s, ended by any input
    with the view left in third person [hypothesis: all of it]. Stand still in first
    person for two minutes: does the camera circle, how fast (time a full turn), which way,
    at what distance and height, does mouse motion alone end it, and is the view first or
    third person afterwards?
192. **When does the low-health heartbeat play?** (P3; `nvf_play::heartbeat`.) The master has
    two looping heartbeats, `UIHealthHeartbeatALP` and `UIHealthHeartbeatBLP` [observed].
    NVForever plays A while the player's health is under 25 % [hypothesis: the threshold,
    and that B is not used]. Take damage down slowly (falls): at what health share does
    a heartbeat start, does a second, faster one take over lower down, and does it stop
    the moment health rises past the line?
193. **Does the Pip-Boy mark the loaded ammo?** (P3; `nvf_play::menu::model`, `Row::loaded`.)
    NVForever now gives the ammo the equipped weapon loads the equipped square in the
    Pip-Boy's ammo list [hypothesis: as the original; Stewie's Tweaks' bContainersShowEquippedAmmo
    speaks of "equipped ammos" in containers]. With a 10mm pistol in hand, open the
    Pip-Boy's AMMO list: does 10mm Round have the filled square, and does it move to JHP
    after switching ammo?
194. **Which need messages show when?** (P3; `fnv_game::needs`.) FalloutNV.esm holds
    sDehydrationIncrease / Decrease / NotSick and the same three for Sleep Deprivation
    [observed]. NVForever shows Increase when the stage goes up (from none too), Decrease
    when it goes down to another stage, NotSick when it leaves the last [hypothesis]; hunger
    and radiation say nothing (no such settings). In Hardcore, let dehydration rise past a
    stage and drink back down: which lines show, and is there one for hunger or rads?
195. **How does a border region stop the player?** (P3; `fnv_game::borders`,
    `nvf_play::borders`.) NVForever reads REGN header flag 0x40 (Border Region
    [documented: GECK wiki form flags]) as the area the player is kept inside, puts a
    player who steps out back where they last stood inside and shows
    `sPlayerLeavingBorderRegion` as a corner message, at most every 3 s [hypothesis: all
    of it]. No master flags a region of a played worldspace [observed], so this needs a
    test plugin: flag a region around Goodsprings as Border Region, walk out of it. Is the
    player stopped at the edge like a wall, or pushed back; is the line a corner message
    or a message box, and how often does it repeat? Does Edge Fall-off (RPLI) move the
    edge?
197. **What does a resurrected actor carry?** (P2; `fnv_game::resurrect`,
    `fnv_sim::World::restock_inventory`, issue #2683.) NVForever makes the items of a
    dead actor's own item list up again when it is resurrected (rolled anew; only what
    it has fewer of is added) and keeps everything else it carries [hypothesis; capture
    rs1 only showed that a given Stimpak stays and looted given ones do not come back].
    Kill a Legionary Assassin of the reputation hit squad (or any NPC whose weapon is in
    a leveled list of its own record, console `kill`), take everything from the body and
    pick up its dropped weapon, then `resurrect` (and again with `resurrect 1`), kill it
    and look: does it carry its weapons, armour and loot again? Same once more without
    looting: is anything doubled, and is the weapon that fell beside the first body
    still on the ground? Then with an NPC whose items are all fixed entries (Ringo):
    do his own looted items come back?

## Added 2026-11-15 (REPCONN test-site load-door lock, #2538)

198. **Is a load door without lock data locked as its other side is?** (P1; #2538,
     cells-refs.md section 6.) **Answered [observed: capture run15b_a, 2026-11-15;
     xNVSE 6.4.9, JIP LN 57.30]: yes for reads and player activation.** Fresh
     `coc RocketLabBasement` (0x0007EA61, REPCONN Basement) from a Goodsprings save
     that never visited it: 0x000CE1AD, without XLOC, and its XTEL partner
     0x0008F97C, with XLOC 255 / no key / flags 0, both read `GetLocked` 1 and
     `GetLockLevel` 255. The tunnel-side door is at (9812, -2504, 7180); from
     (9902, -2414, 7180), 130 units in front, `000CE1AD.Activate player` did not
     move the player or teleport. Activating 0x0008F97C from inside the room was
     refused too, with all four reads unchanged after each attempt. The hatch pair
     0x000CD777 / 0x001694E6 read 0 / 0 on both sides. [observed: FalloutNV.esm,
     fnvtool esm decode / world refs, 2026-11-15] The two load doors share base
     UtlDoorBg01 0x0003D205 (no script); 0x0008F97C is at (9760, -2572, 7176),
     85 units from 0x000CE1AD. The hatches use REPCONHatch2 0x001694DD (no name
     or script), at (7756, -3644, 7316) / (8222, -3377, 6615); neither has XLOC.
     No FalloutNV.esm script names any of these four references. [hypothesis,
     #2538] Key/pick/script unlocking through either side changes the source lock;
     creating an own lock leaves the partner independent. Those mutations, key
     inheritance, broken-lock sharing and NPC use were not captured.

## Answered 2026-11-15 (weather region lookup timing, #2531)

199. **When does the weather follow a weather region: by its outline or the cell's
     region list, and when is it looked up?** (P2; #2531, world.md §6.) **Answered
     [observed: captures run15b_w and run15b_w2, 2026-11-15; first person, GameHour 12,
     xNVSE 6.4.9, JIP LN 57.30 GetCurrentWeather/GetCurrentWeatherPercent]:** script
     MoveTo to (110686, 19125), cell 27,4: NVWastelandClear; (109775, 18882), cell 26,4,
     260 units outside NVLegateRegion's outline: clear for 40 s despite that cell's
     XCLR listing the region [observed: FalloutNV.esm, fnvtool esm decode, 2026-11-15].
     MoveTo (109330, 18852), inside: NVlegateCampWeather;
     back outside: clear. All complete (percent 1.0) at the first sample 4 s after
     moving. Walking east at y 18849 kept camp weather past outline x 109513 and
     fall-off distance x 110537: last camp sample x 110567.9, first clear x 110596.0
     at percent 0.000, across cell border x 110592. Fade rose 0.033/s, complete in
     about 30 s. Walking west entered cell 26,4 outside the outline and kept clear
     to (109067, 18849), 450 units inside, for another 49 s standing. Thus membership
     uses the outline, sampled on exterior cell entry; walking inside a cell holds
     the result, while script moves resample and snap. [hypothesis: extend that
     placement refresh to load doors, other teleports, fast travel, loaded games,
     worldspace changes and coming outdoors; only script MoveTo was captured.]

## Answered 2026-11-15 (combat distraction, #2573)

200. **Does a fighter detect its own foe better than a bystander? Answered: yes.**
     (P2; #2573, [actor-behaviour §2.1a](../spec-notes/actor-behaviour.md#21a-combat-distraction-2573-answered-observer-check-200), detection.md §9.)

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

     [design, #2573; tools/smoke/scenarios.toml] `detection-combat-target-sneak`
     uses the cleared Searchlight Airport runway at hour 12 with god mode. The
     player sneaks from 2 s and stands still. Fighter GoodspringsSettler01Ref
     (00104F03) and watcher GoodspringsSettler03Ref (00104F0A) have Perception 5 and
     Assistance 0; the watcher is removed from all factions. A gecko is created at
     the watcher with Aggression 0 and Perception 0, then positioned 150 units
     toward the player. Both settlers reset to mirrored 690-unit offsets at 5.8 s
     and the gecko resets at 5.9 s. At 6 s the fighter starts combat with the player
     and the watcher with the gecko. Five paired GetDetectionValue/GetDistance reads
     run every 0.5 s from 6.6 to 8.6 s. Every fighter value must exceed its paired
     watcher value; distances are logged without absolute ranges.

     [observed: smoke runs on the combined build, 2026-11-15] With the fix, both
     detectors at 690 units read fighter 64 / watcher 56; the fighter closing to 610
     read 69 (PASS). With both detection.rs files reverted to upstream main, fighter
     84-88 / watcher 91 in all five pairs (FAIL).

## Added 2026-11-15 (Gomorrah Suites wall-sconce glow, #2435)

201. **How strong and how wide is the glow on the wall above a lit Gomorrah Suites
     sconce?** (P3; #2435, nif.md §6 "Fog on added light".) Ours: the card
     `NeoclassicalSconce:9` (ONE / ONE, no texture, vertex colour 0.34 at the bowl to
     black on its outline) adds its colour in display values, fog only dimming it; next
     to the bowl it is about 100 grey levels over the wall, near the outline 10 to 25
     [observed in NVForever; hypothesis for the game]. In Gomorrah03 by any hour, stand
     at 5900, 2250, 6384 facing east (heading 90, level) towards the sconce 0x0011A90A,
     take one frame, then one from heading 50; note the wall's grey level beside the
     glow, the glow's level next to the bowl and halfway up, and whether an outline of
     the card can be seen anywhere. Softer and dimmer than ours points at the
     display-value sum ("Additive blend") on a ONE / ONE card; the same agrees with ours.

## Added 2026-11-15 (gas traps, #2687)

202. **What sets a gas cloud off: a ballistic gun fired inside it, one fired through it,
     a frag grenade?** (P1; #2687, `engine/nvf-play/src/gas_traps.rs`.) Ours since
     #2687: a shot that passes through a cloud hits it with its weapon, which sets it off
     only for the weapons of `ExplodesGasTrapsOnHit` (FLST 0x0007627F: energy, fire,
     launchers, no plain guns). A gun fired from inside a cloud, and any blast that
     reaches one, also hit it with `GasTrapDummy` (WEAP 0x000001F6), which sets it off
     [hypothesis; GECK wiki Traps documents "fired inside" and "explosions"]. In
     Vault22e, on a save taken after Keely's line INFO 0x000F43F8 (or after
     `0F0908.enable`), stand outside `V22SporePurgeGasRef` (0x000F0908, at 3592, 2469,
     8469) and fire a 10mm pistol through the gas at the wall behind it. Then reload,
     step inside it and fire the 10mm at the floor. Then reload and throw a frag grenade
     in. Each time, read `0F0908.getdestructionstage` and `GetObjectiveCompleted VMS06 50`.
     Our rule holds if the shot through it does nothing and the shot from inside and the
     grenade set it off. If the shot from outside sets it off too, a ballistic hit counts
     anywhere in the cloud. If the shot from inside does nothing, the "fired inside" rule
     is wrong for New Vegas, and the dummy hit for a gun fired inside goes; the owner
     asked in #2687 that any spark set the gas off, so that would be a ruling for them.

## Answered 2026-11-15 (Karl at Red Rock and the Legion, #2728)

203. **Does Karl at Red Rock turn on the player because of the Legion?** (P1; #2728, #2467,
     PR #2498, owner-review item 65 there; crime-karma.md §3.4, §3.7.) Karl is VRRCKarlREF
     0x00140E71 (base VRRCKarl 0x00140E60, Aggressive). His records put him in three
     factions, not one: ArmorVCaesarsLegionFaction 0x00134B6E (Track Crime, no reputation),
     vCaesarsLegionMilitaryDialogueFaction 0x00137BB7 and VRRCKarlFaction 0x0017B5B6 (Track
     Crime, reputation RepNVCaesarsLegion, Ally of VCaesarsLegionFaction and of the Red Rock
     ambush factions) [observed: FalloutNV.esm, fnvtool esm decode, 2026-11-15]. **Answered
     [observed: nvf_capture observer campaigns `karl_murder`, `karl_murder2`,
     `karl_legion_rep`, `karl_legion_rep2`, `karl_legion_rep3`, `karl_legion_rep_cove`,
     `karl_flags` and `karl_flags2` (tools/capture/observe/), save nvfairport, god mode,
     xNVSE 6.4.9, plugin version 10, 2026-11-15; outputs `obs_karl_*` in the private capture
     folder]:**
     - **The setup and baseline.** Every sample reads GetPCEnemyOfFaction on the three
       factions, Karl's GetFactionRelation player, GetActorFactionPlayerEnemy and
       GetActorCrimePlayerEnemy, IsPCAMurderer and the Legion's infamy and fame. With no
       crime, Karl detects the player, sits (GetSitting 3), stays at relation 0 and never
       fights, on entry and at noon on each of the next four days.
     - **1. Seen murder.** The staging is #2498's `crime-karl-no-side` (since renamed `crime-karl-flag-clears`): the player at
       (63363, -89150) at Cottonwood Cove, hour 12, NVCCAureliusREF 0x0012D6E2 about 400
       units off. Shots by TapControl never fired in the unfocused game, and FireWeapon
       never hit. The kill that worked is `NVCCLegionary4REF.Kill player`, with the
       Legionary moved 120 units in front of the player first. The original counted it
       as a murder: IsPCAMurderer 1 at once. About 40 samples later **both
       VCaesarsLegionFaction and ArmorVCaesarsLegionFaction read GetPCEnemyOfFaction 1**
       (VRRCKarlFaction 0). Legion infamy went from 0 to 60, Aurelius's relation to the
       player became 1 (enemy), and the Cove's Legionaries started combat with the player.
       So the original does flag the armour faction, which #2498 calls "no side".
       But the flags did not last. In `karl_murder2` the player went by `cow` to the
       Searchlight airport. The flags still read 1 there, then went to 0 at 12.45, about
       0.18 game hours (11 minutes) after the murder, with no cell change and no script of
       ours. Back at the Cove, Aurelius was still in combat with the player: his own grudge,
       with the flags at 0. In Karl's longhouse (by MoveTo, 0.3 to 0.5 h after the murder,
       and at noon one, two, three and four days later), all three flags read 0,
       GetActorFactionPlayerEnemy 0, relation 0. **Karl detected the player, stayed seated
       and never fought.** The murder stayed on record (IsPCAMurderer 1, infamy 60).
     - **2. Legion infamy, no murder.** SetReputation RepNVCaesarsLegion (infamy 100,
       fame 0) and AddReputation infamy deeds: 6 gave 72, and 15 gave 100 with
       GetReputationThreshold axis 2 at 6, the top rank. **Karl stayed seated and calm on
       entry and for 0.4 to 0.5 h each time**, relation 0, all flags 0. He spoke once but
       never fought. The same reputation turned no Legionary at the Cove either: Aurelius
       detected the player and stayed calm for a game hour. In the original, Vilified alone
       puts no Legion member in combat at once; the scripts that read the Legion's rank
       only start encounters, such as VEFR02LegionAssassinSCRIPT
       [observed: FalloutNV.esm script text].
     - **3. What does turn him.** Each case is a flag set by script, then Karl's cell
       entered:
       - SetPCEnemyOfFaction VRRCKarlFaction 1: the flag held through the move, Karl read
         GetActorFactionPlayerEnemy 1 and relation 1, and he attacked on entry.
       - SetEnemy ArmorVCaesarsLegionFaction PlayerFaction (the faction reaction, no crime
         flag): relation 1, and he attacked on entry.
       - SetPCEnemyOfFaction ArmorVCaesarsLegionFaction 1 **set with the player already in
         front of him**: GetActorFactionPlayerEnemy 1, relation 1, and he went to combat
         (OnStartCombat Karl → player) and hit the player.

       Set outdoors instead, the armour flag and the VCaesarsLegionFaction flag each held
       for 0.3 h at the airport, then read 0 as soon as MoveTo put the player in Karl's
       cell, and Karl stayed calm. So the armour faction's flag does turn him while it is up.
       Nothing in FalloutNV.esm's scripts clears those flags (no SetPCEnemyOfFaction … 0
       or ClearFactionPlayerEnemyFlag on them) [observed: script text]. What clears them is
       not known: after the murder it was a timer or something in the engine; for a flag
       set by script it was the move [hypothesis].

     **For item 65:** the original does not agree that a murder flags no faction that is
     "no side": the Cove murder flagged ArmorVCaesarsLegionFaction too. Karl stays calm in
     the original because that flag was gone within about 11 game minutes, long before
     anyone could reach Red Rock, and not because it was never set. The armour flag is
     enough to turn him while it is up. NVForever keeps a crime's flag for
     iCoolDownTimerSetting (3) days (crime-karma.md §3.7), so it flags the armour faction
     and keeps it, and Karl fights; that is what #2467 saw. Not checked: a murder by the
     player's own shot (only the scripted Kill worked here), travel on foot or by fast
     travel instead of MoveTo, and a murder seen by more witnesses or by Karl himself.

## Added 2026-11-15 (the REPCONN ghouls' walk to the basement, #2405)

204. **Do the REPCONN ghouls set off for the basement while its door is locked?** (P2;
     #2405, packages.md §11.) `REPCONGhoulRocketWalkPACKAGE` (Travel to
     `JasonBrightDialogMarker` 0x0008D47A in RocketLabBasement, valid at VMS01
     variable 25 == 3) sends Jason Bright's ghouls from RocketLabTop through the Mid
     floor and `REPCONMidToBasementDoor` (0x000840AC, lock level 25, key
     `REPCONLowerKey` 0x000810E8 that no ghoul carries); `REPCONTopIntercomTrigger`
     (0x000CD683) by the door out of Top moves every actor entering it to the basement
     while `nightkinquest` is 3 [observed: the data]. Ours: no walk starts, since the
     whole way is not open, so they never reach the trigger [observed in NVForever,
     `fnvtool ai run --force Ghoul5=REPCONGhoulRocketWalkPACKAGE`]. In RocketLabTop
     during Come Fly With Me, once the Nightkin are dealt with (`nightkinquest` 3,
     before the player opens the basement door), watch Ghoul5 (0x00087AEF):
     `GetCurrentAIPackage`, `GetPos x/y` every few seconds, and `0840AC.GetLocked`.
     If he walks to the door out of Top and is moved on, the original starts a walk
     whose way ends at a locked door (at least as far as the door); if he stands still
     until the door is unlocked, ours is right and the trigger only serves after it.

## Added 2026-11-16 (how far a spoken line carries, #2750)

205. **How loud is a line between actors at 500 to 4,000 units?** (P1; #2750, voice.md
     "Heard where it is said".) Ours: full level to 600 units (`fDialogMinDistance`),
     600 ÷ distance beyond, faded to nothing at 2,000 (`fDialogMaxDistance`)
     [hypothesis]. The owner hears President Kimball's speech in the original from the
     sniper tower, 3,551 units from the podium, and nothing in the speech's records sets
     it apart from other lines [observed: the data]. This check needs the game's sound
     output recorded (a loopback recording; the observer has no such tool yet). In the
     open, away from other sounds, music and radio off, voice volume 100%: have one
     actor say the same long line (`Say` or `SayTo` of one topic) with the player put at
     500, 1,000, 1,500, 2,000, 2,500, 3,000, 3,500 and 4,000 units from it
     (`GetDistance` logged), facing it, and record the output's level over the line at
     each distance. Then the same at Kimball's speech from the podium's foot, the
     vertibird drop-off and the sniper tower. What to read off: whether the level
     falls as min ÷ distance, where it stops falling or is cut, whether 2,000 units
     means anything, and whether the speech differs from the ordinary line. Also
     whether the subtitle shows at each distance.


## Added 2026-11-15 (Use Weapon at objects and markers, #2652)

207. **How does UseWeapon aim at objects and markers?** (P1; #2652, #2715 shooter
     half; `docs/spec-notes/use-weapon.md` section 6.) [documented: #2652 supplied record
     facts] The Hopeville silo mainframe's ACTI OnHit ED-E block sets NVDLC04MQ01a
     55 after its package requests one Always Hit / Do No Damage arc-welder shot;
     the commissary terminals also wait on ED-E's hit. Capture activation and button
     1 at the mainframe, then a commissary unlock: does OnHit receive ED-E, and where
     does the beam land? Capture a finite script use at an XMarker: do its shots fire
     and does ED-E resume the next package without ResetAI? In G.I. Blues, do the
     barkers keep firing at VFSPacerBusShelterREF while their Repeat Fire package
     holds, including during combat? [hypothesis] NVForever aims at placed position
     plus OBND centre (marker: position alone), counts misses toward finite uses, and
     dispatches harmless object hits with the shooter. Check that placement on an
     asymmetric or rotated object as well. [documented: #2652 w4 supplied world refs]
     The barkers and Pacer stand in FreesideNorthWorld, while the shelter belongs
     to FreesideWorld; both have parent WastelandNV and share coordinates.
     [hypothesis, #2652] NVForever treats a non-actor placed Use Weapon / UseWeapon
     target in a sibling worldspace (same nonempty immediate parent) as coordinates
     in the shooter's worldspace, with its OBND centre added. Capture whether the
     barkers turn and shoot at this shelter from the fight markers, and compare an
     unrelated-worldspace object and a sibling-worldspace actor target (neither
     receives this projection). Pacer's cover pose is outside this check.

## Added 2026-11-15 (old-head hair outside the race list, #2714)

210. **Which hair does the original use when an NPC's HAIR is outside its race's
     list?** (P3; #2714; actors.md section 4.4.) [documented: #2714, spooknik's
     2026-11-15 record/archive audit at main 1790ab94a] Roy, VFSRoyREF
     0x0011365D / base VFSLocalVictim03 0x00113649, is CaucasianOldAged
     0x000987DF (`headold.nif`) with HairAfricanAmericanBase 0x000306BE
     (`hairbase.nif`, `hairafricanamericanbase.dds`, HCLR 192,192,192, no hat).
     This hair is outside the race's old-hair-only HNAM list; the default male
     hair is HairBaldingOld. Old-head alternatives exist: HairBaseOld 0x00044528
     (`hairbaseold.nif`, `hairbaseoldnohat.egm` / `hairbaseoldhat.egm`) and
     HairAfricanAmericanBaseOld 0x000987D9 (`hairafricanamericanbaseold.nif`).
     The second case, VNovacDoctorMerc2, uses `hairraiderspike.nif` on an old
     head and has no old version [documented: the same audit]. In the original,
     at Old Mormon Fort courtyard during VMS19 objective 60, capture Roy from
     behind and from the side, close enough to compare the scalp/hair seam;
     also capture VNovacDoctorMerc2 from those angles. Does Roy retain the
     record's hair with the same gap, wear the old-head version, or wear the
     race default? Does the second case retain its hair or use a default when
     no old version exists? [hypothesis, #2714] NVForever provisionally keeps
     the record's model and its own EGM; a young-head model on an old head
     could explain the reported gap. Resolve this before changing hair selection.

## Added 2026-11-15 (Mick secret door, #2716)

206. **Does an NPC path through an open locked door in the original?** (P2;
    `fnv_nav::NavGraph::door_tri_open`.) [hypothesis, #2716] NVForever treats an
    open in-place door as no barrier to any actor, even while locked. [documented:
    issue #2716, spooknik comment 2026-11-15, supplied record findings] In
    `FreesideMickandRalphs`, Mick reaches `SecretDoorMarkerREF`; his Travel end script
    opens `MickSecretDoorREF` (0x00121FF2, Requires Key, no key) with SetOpenState 1
    and chooses the secret-sales package. Watch whether he then walks through the
    still-locked door to `SecretSalesMarkerREF` and starts the sales conversation.
    Check another NPC without a key through an open locked door too, then close it
    and compare. Record whether the live lock changes when the door opens.

## Added 2026-11-16 (Pacer's cover, #2715)

208. **Does Pacer keep cowering throughout the staged NCR fight, including after a load?**
    (P2; `fnv_ai::sandbox`, #2715 part 3.) His VFSPacerCowerNCRFightPackage
    (PACK 0x00129434) permits only idle markers; VFSNCRFightPacerMarkerREF
    (REFR 0x001293EC) uses DefaultCowerIdleMarker (IDLM 0x000C47CC, timer 0),
    with LooseCowerIdles (0x000C0A9A, looping 3rdP_Cowering.kf)
    [documented: #2715 issue comment, supplied record findings]. With VMS19 ncrfight
    at 1, watch Pacer from the start for at least 120 seconds without talking or
    starting combat. Does he ever stand upright between cower loops? Save while
    he cowers, reload, and measure the time until he cowers again. NVForever reuses
    the sole marker at each timer expiry when every other behaviour is disabled
    [design, #2715: `Allowed::only_idle_markers`, `Sandbox::next`]; continuous cower
    in the original is provisional [hypothesis]. This leaves the general sole-object
    60-second rule and observer check 114 intact [design, #1505, #2715].

## Added 2026-11-15 (held weather across worldspace doors, #1997)

209. **Does held weather survive a door into another worldspace?** (P3; #1997;
    `worldspace.rs::carry_sky`, `sky.rs::SkyState::carry_weather_from`.) In Freeside,
    use the console to `ForceWeather` a clearly different weather with override flag 1,
    then go through the gate into the Strip. Does that weather remain shown? Repeat
    with `SetWeather` and its override flag, allowing its fade to finish first; then
    call `ReleaseWeatherOverride` and check that normal weather returns. Save after
    the door and load: is it still held? Both setters say the weather will not change
    until release [documented: offline GECK wiki ForceWeather / SetWeather /
    ReleaseWeatherOverride, snapshot 2026-10-28]; keeping it across a worldspace door
    is provisional [hypothesis]. NVForever carries it without destination filtering
    [design: `sky.rs::SkyState::carry_weather_from`].

## Added 2026-11-16 (controller hotkey assignment, #2312)

211. **How does a controller choose a hotkey slot on ITEMS?** (P3; controller.md §3,
    weapon-wheel.md §3.) [documented: issue #2312's supplied inventory_menu.xml facts]
    `IM_HotkeyButton` uses RB and `sInventoryHotKey`, opening `IM_HotKeyWheel`.
    [hypothesis] The original's slot-selection method is unknown; its prefab facts
    supplied in #2312 bind no pad directions. Select a weapon on ITEMS, press RB, and
    test D-pad taps/holds and both sticks in cardinal and diagonal directions. Record
    which box is chosen on opening, whether selection points spatially or steps,
    whether it wraps/repeats, and whether A assigns and B cancels. Include Ammo Swap
    and an occupied slot. [design, #2312] NVForever provisionally opens at slot 1,
    steps right/down forward and left/up backward through eight slots with wrapping,
    assigns with A (closing on success), and cancels with B; Ammo Swap refuses items.

## Added 2026-11-15 (House's chamber steam, glow and ground fog, #2685)

212. **Do the steam, panel glow and ground fog of House's chamber linger, and do they survive a
     load?** (P2; #2685, spec-notes/idles.md §10.1, particles.md §7.8.) [hypothesis] From a sealed
     chamber in Lucky38ControlRoom, use the unseal terminal item and record the opening, steam
     bursts, panel glow and ground fog at start, after 20 s and after 55 s. Check that glow and
     haze linger, then save/load at the held end and compare. Also check whether replacing or
     stopping the idle removes its effect model. The provisional engine rule holds CLAMP effect
     tracks until replacement, using authored values throughout; actor idle playback is
     currently not serialized. No original capture was performed in this run.

## Added 2026-11-15 (Kimball's speech crowd heading, #2526)

213. **Does Kimball's speech crowd face its editor heading?** (P2; #2526;
    `fnv_ai::carry::marker_heading`, `docs/spec-notes/packages.md` §3.)
    [documented: #2526 writer brief, main session's fnvtool ai show of FalloutNV.esm]
    `VHDSpeechNPC01REF` (0012FCDC) and `VHDSpeechNPC02REF` (0012FCE2) have Travel
    `VHDCivilianIdlePackage` (00133EC3), near editor location, radius 0.
    [hypothesis, #2526] NVForever makes Travel and standing Guard at an editor location
    below the existing 16-unit guard-wander cutoff face their placed z rotation.
    In the original, during Kimball's speech at Hoover Dam cell 16,5, record one crowd
    member's current package, position and z angle against its placed position/rotation.
    Move it off its editor spot and reevaluate: does it return and turn to the placed
    heading? Also check a small-radius standing Guard if a test plugin is available.

## Added 2026-11-15 (the Ranger assassins' drop at the Legate's camp, #2532)

214. **Legate showdown rangers and harmless drops without Allow Falls (#2532).**
     [documented: supplied main-session fnvtool record brief, 2026-11-15] In the NCR
     showdown, SCPT 0x001438CA enables 0x001438DA then 0x001438DB when battle state 1,
     showdownstart 1 and controller 0x001438E3.spawnheroes 1. Their intended Patrol
     0x001438E8 lacks Allow Falls and starts at VLegateCampRangerRunMarker01REF
     (0x001438D3), then 02 (0x001438D4), 03 (0x001438D1), 04 (0x001438D2), 05
     (form ID not supplied). [hypothesis, #2532] An authored downward link within
     fJumpFallHeightMin is safe without Allow Falls; NVForever now uses that rule
     if the ordinary route is unavailable (packages.md; actor-jumps.md). In the
     original, sample both rangers' packages, positions and procedures from enable
     through marker 01; record where they leave the ledge, land and enter combat.
     With a synthetic test plugin, compare authored drops at, below and above the
     damage threshold, with and without Allow Falls. Do harmless drops work without
     the flag, do damaging ones require it, and is the height measured per link?
     Separately ask the main session for both rangers' NPC_ PKID/conditions and
     NAVM triangle/link/drop heights to marker 01 and between markers; no camp
     navmesh facts or original captures were available to this writer.

## Added 2026-11-15 (classic VATS screen layout, #2659)

215. **Classic VATS unresolved comparisons (#2659, items 6–9).**
    [documented: issue #2659, original capture description; quoted below with its
    numbering and wording.] [hypothesis] Keep the existing AP meter, AP cost and
    target-change rules until these checks settle them. The bottom enemy bar during
    playback is implemented under item 3; the wider playback comparison stays open.

    6. **AP meter while queueing.** NVForever takes the reserved AP off the HUD's AP bar as attacks are queued. In the original's pictures (1280 wide) the bar did not visibly drop with four .357 attacks queued on 80 AP; a closer capture (a crop of the bar, or `GetUIFloatAlt` reads of the AP meter's tiles before and after each queue) should say whether it drops, dims a share or stays.
    7. **AP cost of a shot.** The original queued four .357 shots on 80 AP, so 20 AP or less each; NVForever charges 26 AP a shot (`vats mode: on (WeapNV357Revolver, 26 AP a shot ...)`) and allows three. The capture's save (a level 1 character) may have perks or stats that lower the cost; that was not checked. Compare `GetCalculatedAPCost` [documented: GECK wiki] in the original with NVForever's `shot_ap_cost` for the same character.
    8. **Playback HUD.** In the original, the whole HUD but the enemy bar (bottom centre) is hidden while the queued attacks play, and comes back after with the XP gained. NVForever's playback was not shot in this pass.
    9. **After a target change.** The original showed the new target with no part boxes at once (the capture moves no pointer); whether they come with the pointer on the body, or after a moment, was not settled.

## Added 2026-11-15 (L'il Devil's crown, issue #2350)

216. **Does L'il Devil's crown follow the hair colour?** (P3; nif.md §6 "Hair",
     actors.md §3; #2350.) [documented: issue #2350, supplied `fnvtool esm decode`,
     `fnvtool nif` and texture measurements] HAIR 0x000987DB `HairRaiderFCat`
     "L'il Devil" uses `Characters\Hair\HairRaiderFCat.NIF`; Playable and Not Male
     are set, Fixed Color is not. Its `NoHat` and `Hat` shapes have identical
     BSShaderPPLightingProperty flags 0x82040101 (Specular, Alpha texture, Hair,
     Remappable, Z-test), NiAlphaProperty blend + test, and textures
     `HairRaider01.dds`, `HairRaider01_n.dds`, `HairRaider01_hl.dds`, matching the
     other ten raider hairs' properties. At the crown, 9% of `NoHat`'s area samples
     black, opaque diffuse texels (RGB ≈ 0.001, alpha ≈ 0.99); other raider hairs
     have at most 1% there. The `_hl` texture carries strands there: 2,084 of the
     2,506 black, opaque diffuse texels have `_hl` luma above 0.08; crown-region
     luma is about 0.17 against diffuse 0.001, with overall luma correlation 0.97.
     NVForever multiplies the diffuse by doubled HCLR and draws the model's solid
     core at alpha ≥ 0.8 [design; `engine/nvforever/src/actors.rs::part_uses`,
     `engine/nvf-assets/src/nif_spawn.rs::hair_model_cut_out`; provisional tint
     hypothesis in actors.md §3]. The issue has no reproduction in NVForever or
     observation of the original [documented: #2350]. The original may take crown
     colour or brightness from `_hl` [hypothesis, #2350]. In the original's face
     editor at Doc Mitchell's, select L'il Devil and capture the crown at two very
     different hair colours, keeping the view and lighting fixed; compare with
     NVForever at the same colours and view. Does the crown tint with the strands,
     or stay black? Record both colours and frames in nif.md §6. Consider a shader
     change only if the original tints it; then compare three or four other hairs
     before choosing a rule [design, #2350].

## Added 2026-11-16 (the Legate's camp Securitrons and General Oliver, #2794)

217. **At the Legate's camp (House route), once their scripted guards are down, do the
     four Securitrons fire on General Oliver and the fifth guard?** (P1; #2794,
     combat-ai.md §10.3, owner-review item 85.) Ours: a faction ally of the player whose
     Assistance helps allies takes up any fight against the player it sees, so the
     Securitrons go on to Oliver and Guard05 [hypothesis: whether Assistance counts the
     player as an ally to help is not documented; the maintainer expects them to help].
     The scene: after the Legate, the camp trigger enables Oliver
     (`vHDGeneralOliverRef2` 0x001483F1) and `LegateCampRangerOliverGuard01REF`..`05REF`;
     `VHDLegateCampSecuritronMarker` 0x00155DC9 enables `LegateCampSecuritron01REF`..`04REF`
     (0x00155DC2, C3, C4, C1; VHDSecuritronFaction, PlayerFaction Ally; Unaggressive,
     Brave, Helps Allies). Pick the attack topic (INFO 0x0015993C): its end script runs
     `StartCombat player` on Oliver and gives each Securitron one of Guard01-04, none
     Guard05 or Oliver [documented: issue #2794, cited records]. Watch each Securitron's
     `GetCombatTarget` every second once its guard is dead, and who damages Oliver and
     Guard05 (their health). Also: does a Securitron help when the player alone fights
     Oliver before the guards are down (it has its guard then)?

## Answered 2026-11-25 (SayTo's noTargetLookFlag, #2796)


218. **Does `SayTo` with `noTargetLookFlag` 1 turn the speaker to its target?** (P2; #2796,
     dialogue-play.md §9.2.) Answered [observed, base game, 2026-11-25: capture jobs `o31_218_sayto_flag`, `o31_218b_sayto_flag` (outputs in the private capture folder)]:
     **yes, the flag does not stop the speaker turning.** Ringo (nvftest, GSGasStation)
     was moved 150 units in front of the player and faced away (GetHeadingAngle player
     180), then `RingoRef.SayTo player GREETING 0 <flag>`: standing, he turned his body
     to the player in about 0.8 s (≈ 30° per 6 frames, GetHeadingAngle 180 → 0) with flag 1
     in 3 of 3 runs and with flag 0 in 2 of 2, the same rate; with no SayTo he stayed at 180.
     Walking on his own package (his position changing every sample, procedure 48) he did
     not turn with either flag (2 runs each). So the turn depends on whether the speaker is
     free to turn, not on the flag; the flag can only be the wiki's (the *target* is not
     made to look) [documented: GECK wiki SayTo]. Ours turns nobody with flag 1
     (`crates/fnv-game/src/functions/misc.rs` `say_to`, test
     `say_to_with_no_target_look_turns_nobody`): it differs. Still open: why Curtis does
     not turn for VMS08CurtisRadio1 in the original (#2796's case: his package or idle at
     the walkway, not the flag), and what the flag does to the target.

## Added 2026-11-16 (a talking activator's Say and SayTo, #2768)

219. **When a talking activator given a talking actor says a line whose conditions test the
     speaker (GetIsID), are the conditions tested on the talking actor?** (P2; #2768,
     talking-activators.md §2, `Game::say`.) Ours: yes; the line is chosen with the actor
     as the speaker and IsActorTalkingThroughActivator 1, said in the actor's voice from
     the activator, its SayToDone blocks run on the activator and its result scripts on
     the actor; with no line for the actor the activator's own lines are tried
     [hypothesis]. Tale of Two Wastelands relies on it (Vault 101's intercom given Jonas,
     CG02IntercomConv; the Pitt's Ashur intercom), FalloutNV.esm's talking activators may
     not. With a talking activator placed near the player and a test topic whose one line
     has GetIsID <an actor> (and nothing else), run `<activator>.SetTalkingActivatorActor
     <that actor's ref>` then `<activator>.Say <topic>`: is the line said (subtitle,
     voice)? Then without SetTalkingActivatorActor (cleared): nothing? While it plays,
     read `<activator>.IsTalking` and `<actor>.IsTalking`, and note whether a SayToDone
     block in the activator's script runs when it ends (a script print).

## Answered 2026-11-25 (an NPC saved while seated, #2797)


220. **Does an NPC saved while seated come back seated after a load?** (P2; #2797,
     save-load.md §3.4, furniture.md.) Answered [observed, base game, 2026-11-25: capture jobs `o31_220a_pete_save`, `o31_220b_pete_load` (outputs in the private capture folder)]:
     **yes, the original keeps the seat in the save.** Easy Pete (nvftest, GameHour set to
     9, `GetSitting` 3 on his porch chair at -67968.8, 3450.4, 8387.3) was saved
     (`save nvfo31pete`); on loading that save his `GetSitting` read 3 at the same position
     to the tenth of a unit from the first sample (about 2 s after the load, when the
     capture job could start) through 40 samples over 20 s: no 0, no getting-on (2), no
     step. Ours stands every actor up on a load and lets the AI seat them again
     (`Game::stand_everyone_up`): it differs. Not covered: the first 2 s after the load
     (a re-seat that fast would also have moved him), Colonel Hsu's chair.

## Added 2026-11-16 (sentry bot missile pause, #2739)


221. **How long does a sentry bot wait between two missile shots, and does it switch to its
     gun between them?** (P1; #2739, combat-rules.md §4.1, combat-ai.md §19.) Answered
     [observed, base game, 2026-11-25: capture jobs `o31_221_sentry_missiles`, `o31_221b_sentry_hits` (outputs in the private capture folder)], no video: nvfairport,
     player in god mode standing on the runway, each bot placed 1,500 units off with
     StartCombat, `GetEquippedObject 5` every 2 frames, the hits on the player by weapon
     from xNVSE's OnHitWith. **It fires one missile, then switches to its gun.**
     - `CrSentryBotGLHard` (launcher and laser gatling), 40 s: launcher and gatling take
       turns, 4.4-4.6 s with the launcher (the first turn 6.9 s) and 5.7-5.8 s with the
       gatling (switches at
       1.1, 8.0, 13.7, 18.3, 24.0, 28.6, 34.4, 38.8 s). Each launcher turn gives **one**
       missile hit, 1.6-4.3 s after it was equipped (hits at 5.4, 15.8, 25.7, 36.0 s:
       a missile every 9.9-10.4 s). Each gatling turn gives two bursts of 5-11 hits about
       1.1-1.5 s apart.
     - `CrSentryBotMGArmy` (launcher and minigun), 40 s: launcher 0-6.3 s (no hit seen),
       minigun 6.3-12.5, launcher 12.5-17.1 (one missile, 14.4 s), minigun 17.1-35.0
       (bursts every 4.5-5.5 s), launcher 35.0 (one missile, 36.9 s): missiles 22.5 s apart.
     - Neither bot's `GetItemCount AmmoMissile` (4 and 2), Electron Charge Pack or 5mm count
       went down: the fight spends none of its inventory, so it does not run out in 40 s.
     - Both stopped closing in at about 1,110-1,330 units and held there.
     Ours fires a missile every 0.967 s and keeps the launcher: it differs. Not covered:
     a crippled gun arm, Mister Gutsy's guns, what plays between the missile and the switch
     (the video).

## Answered 2026-11-16 (sky and grade, Capital Wasteland against the Mojave)

222. **Why is our Capital Wasteland sky dark brown where the original's is pale green?**
     (Owner 2026-11-16, Tale of Two Wastelands.) Answered [observed: 54 frames of the
     original through `capture_frames.py`, the TTW game started from Mod Organizer 2's
     "Tale of Two Wastelands" entry]: the adapter's tint was dropped where the base image
     space has none, the high-noon colour column was not used, the dome's green weight
     was read as the horizon colour, and Mojave-fitted daytime factors darkened and
     warmed the sky. world.md section 6 "Sky and grade across two worldspaces" has the
     figures. How to run it again: the capture plugin starts a queued job only after a
     save has loaded, so a `coc` from the main menu is not enough; TTW's "missing
     dependency" box pauses the game until it is clicked; the game can hang on its quit
     line and has to be closed before the next run.
     Sweep 2026-11-17 (27 worldspaces, world.md section 6 "Sweep of the other
     worldspaces"): most match; Point Lookout, Anchorage, The Pitt's bridge and the
     nuked Divide do not. To run a TTW capture: Mod Organizer 2 must be opened by the
     owner first (started from a session it cannot launch the game: error 5), the
     launch then goes through it (`moshortcut://:Tale of Two Wastelands`); the base
     game's own load order has no add-ons, so their worldspaces need this route too.
     Left to check: the sky level against "LUM Ramp No Tex" in a third worldspace
     (Point Lookout, Zion, The Pitt); the Capital Wasteland's ground level; the sunrise
     sky; the sky near the sun at noon.

## Answered 2026-11-17 (ResetAI right after AddScriptPackage, #2865)

223. **Does the original keep a script package added just before ResetAI in the same
     script run?** (P2; #2865; `crates/fnv-game/src/functions/ai_state.rs` `reset_ai`.)
     Answered [observed, base game]: no. GSVictorRef given VES01aVictorNovacGoHomePackage
     alone holds it; given it and ResetAI in one script he is on his own package two
     loops later and stays there (owner-decision-captures.md, od_06, "AddScriptPackage
     and ResetAI in one script run"). The runtime already does this. A job script cannot
     name a reference, package or quest by Editor ID: build each with `BuildRef 0
     <decimal form ID>`.
224. **Do Gibson's dogs take up a mole rat's fight against the player?** (P2; #2927;
     `crates/fnv-game/src/provocation.rs` `call_allies`, `combat_group.rs` `raise_alarms`.)
     Answered [observed, base game, three runs]: yes. With three mole rats attacking a
     passive player beside the yard, three of four dogs (run 2) and four of four (run 3)
     went into combat with the player before any mole rat was harmed and stayed in it
     after all three were dead; `GetShouldAttack player` stayed 0. In run 1 the mole rats
     lost the player early and no dog joined. The runtime already does this
     (combat-groups.md §4). To count placed creatures in a job: walk `GetFirstRef 200 1`
     and compare `GetBaseObject`.
225. **In a child worldspace with Use Land Data, which landscape is drawn where the child
     has its own LAND and it differs from the parent's?** (P2; #2345;
     `crates/fnv-world/src/index.rs` `take_parent_data`.) Tale of Two Wastelands:
     MegatonWorld squares (-1, -7) and (1, -6), ParadiseFalls (-9, 15) and (-9, 16),
     GNRroofWorld (flat at about 12,280 over the parent's ground at about 10,700). Stand
     in each and read the player's z on the ground (`player.GetPos Z`): the child's
     height or the parent's. NVForever takes the parent's LAND on every square where the
     parent has one, the child's own too (#3755; before, it kept the child's own). Also: per-cell water in ParadiseFalls (Use Water
     Data), which NVForever does not take from the parent's cells.
226. **What do SetStage and the objective functions do on a quest already completed or
     failed?** (P2; #2958; `crates/fnv-game/src/game.rs` `set_stage`, `set_objective`.)
     Answered [observed, base game, four jobs]: SetStage marks the stage done and runs its
     script but does not start the quest again; SetObjectiveDisplayed and
     SetObjectiveCompleted do nothing, even after StartQuest; CompleteAllObjectives (in
     the stage's script) still completes. A quest stopped without being completed starts
     again on SetStage and on SetObjectiveDisplayed (layer5-captures.md §5.8). Not
     separated: whether CompleteAllObjectives or the stage's Complete Quest flag completed
     the objective; FailAllObjectives; what the Pip-Boy lists for such a quest.

## Added 2026-11-14 (the compass's map marker range, #2348)

227. **How far does the compass show map markers, and does Perception change it?** (P2;
     #2348, hud.md §11.) Ours since #2348: within `iMapMarkerVisibleDistance` (10000 by
     the executable's default [documented: GECK wiki "Complete List of GameSettings"]),
     found and unfound alike, whatever the Perception [hypothesis: the setting's name,
     and Oblivion's setting of the same name]. Before it, Perception × 2048. To check:
     walk straight away from a known marker and note where it leaves the compass (about
     10,000 units expected; `player.getpos x` / `y` at that point and the marker's place
     give the distance). Then `setgs iMapMarkerVisibleDistance 3000` and check that the
     cut-off follows. Repeat with one found and one unfound marker, and at Perception 1
     and 10 (`player.setav perception`), to confirm Perception plays no part.


## Answered 2026-11-25 (OK on a message box without buttons, #2925)


240. **What does GetButtonPressed give after OK on a message box that has no buttons?**
     (P2; #2925, scripts.md §7.2 engine API.) Answered [observed, base game, 2026-11-25: capture jobs `o31_240_buttonless_ok` (outputs in the private capture folder)]:
     **0.** In BlackMountainTreasure with Science 100, `RhondaREF.Activate player 1` opened
     BMRhondaBrokenMsg (MenuMode 1001, two rows); JIP's ClickMenuButton on
     `MessageMenu/NOGLOW_BRANCH/MM_MainRect/MM_ButtonList/MM_Button:0` (the lower row,
     `_y` 52: "[Science] Check the robot's circuitry.", button 1) opened BMRhondaFixMsg
     (one row, the default OK); a click on its OK closed it and Rhonda's GameMode block
     took button 0: `RhondaREF.GetDead` went from 1 to 0 within a second. Ours gives 0:
     the same. Two traps for the next run: a scripted `Activate player` skips the
     OnActivate block (a dead creature's loot menu, 1008, opened); `Activate player 1` runs
     it. The rows' names count from the bottom: `MM_Button:0` is the last row.

## Answered 2026-11-25 (SendAssaultAlarm on a faction with no member present, #2926)


241. **What does `SendAssaultAlarm Player <faction>` do when no member of the faction is
     present and alive?** (P3; #2926, crime-karma.md §3.5.) Answered
     [observed, base game, 2026-11-25: capture jobs `o31_241a_alarm_dead`, `o31_241b_alarm_alive` (outputs in the private capture folder)]: nvftest, god mode, `coc
     TechatticupMineInterior`, then `TechMineNCRHostage2REF.SendAssaultAlarm player
     CaesarsLegionTechMineFaction` (the hostage script's line, run from a job).
     - **All 17 members killed first** (`Kill`, no killer; two runs): nothing. For 10 s no
       member, the hostage nor the player was in combat; afterwards
       `TechMineNCRHostage1REF.Activate player 1` opened its message box (MenuMode 1001),
       so the second hostage can still be freed. Ours: the same.
     - **Members alive**: not one member but every one within about 1,900 units. 7 of the 17
       were in combat 0.5 s later (6 on the player), 9 by 1.5 s, and the same 9 for the
       rest of the 10 s, all with the player as their target: those 145-1,872 units from the
       player (TecLegionaryRaider5REF 0x0011F654 among them); none of the 8 at 2,038 units
       and more joined. None had been in combat before the call. Ours turns the first member
       in FormID order (0x0011F654) and lets the combat-group alarm spread it: whether that
       gives the same 9 is to be checked in a headless run.
     Not covered: crime gold (the job's `player.GetCrimeGold` did not compile), the
     guards at the entrance outside the cell.

## Added 2026-11-15 (radscorpion spacing and melee cadence, #2535)

244. **How close do two large creatures come, and how often does a creature attack?**
     (P2; physics.md §6.8, combat-rules.md §10.) **Answered in part
     [observed: capture run15b_b2, 2026-11-15; xNVSE 6.4.9, JIP LN 57.30]:**
     VCrTier2RadscorpionMed and VCrTier3GiantRadscorpionMed on Searchlight Airport's
     runway against a standing god-mode player, GetDistance between the pair and
     GetAnimAction of each at 30 samples/s for 17 s. While both were within 300 units
     of the player (452 samples), centre spacing was median 131, tenth percentile 93,
     ninetieth 283, minimum 32 briefly. Each entered action 2 seven times in about
     13 s. Successive attack-start gaps: 2.82, 1.47, 1.39, 2.20, 2.67, 1.28 and
     2.05, 3.74, 1.93, 2.15, 1.37, 2.00 s (median 2.0, least 1.28). Action 2 lasted
     0.77–1.18 s, then action 4 for 0.44–1.10 s. No attack began during action 2.
     The shortest gaps (1.28, 1.37, 1.39, 1.47) are just past the ordinary clips'
     `a:` keys (1.267/1.300) and the sting's end (1.267).

     [design, #2535] Ours now holds a creature's next melee decision until the playing
     clip's `a:` key, or its end without one; earlier keys still shorten the base timer.
     Creatures only (the maintainer's decision of 2026-11-18 on #2971): people and NPCs
     keep the 1.2 s base timer (÷ attack speed), brought forward to an earlier key,
     until their pace is measured here. Walking creatures use a BBX
     half-width × actor scale circle when larger than their previous block radius
     [hypothesis: fits the observed spacing; the original's shape was not seen].
     The world capsule is unchanged [design: `phys::npc_params`].
     [design: `hitvol::sync_volumes`, #2535] The wider circle also stays when a
     fighter stops and its walking character is removed; standing attack blocks
     therefore retain both creatures' widths.

     **Still open:** the original's extra pause after action 4 (3/14 attacks at once,
     others after 0.1–2.1 s) is not built; what chooses its duration? Against a player
     backing away for 10 s, one creature attacked three times (3.5 s apart), the
     other never [observed: capture run15b_b2, 2026-11-15]. Backing-away cadence
     parity and the unseen collision shape remain open [design]. **People's pace:** does
     a person's next melee swing (one-handed, two-handed, fists; with late `a:` keys or
     none) also wait for its clip's key or end, or come on a fixed base? Measure the
     attack-start gaps of NPCs as above; until then people keep the base timer (#2971).
     **A creature in a lunge's way:** does a dog or other creature standing in front
     of the player stop a Legion Assault lunge (a forward power attack's clip carrying
     the player), or does the player pass or push it? Ours: the creature's BBX circle
     stops the lunge after about 25 of its 332 units [observed: the main session's
     runs of NVForever, 2026-11-18, smoke `perk-legion-lunge` with fiend dogs rushing
     the player, 4 runs of 6; on main, with the narrower block, 0 of 6].
     [observed: the main session's runs of NVForever, 2026-11-15, reproduction on
     Searchlight Airport's runway: the Radscorpion + Giant Radscorpion pair against
     a god-mode player who backs away from 16 s to 28 s; pair distance read once
     a second; three runs of upstream main 99faa23b7 and six of this branch]
     Standing and fighting, the pair stays 66–84 units apart on upstream main and
     113 on this branch (the designed sum). Chasing, both builds stay mostly
     62–64 units apart. On this branch, in four of six runs one reading late in
     the chase, as the pair comes back to a stop, fell to 7–39 units for about a
     second. Upstream main showed no such dip in its runs.
     [design, #2535] The widened block keeps standing creatures apart but not
     moving ones, and the change from walking to standing can let one creature
     pass briefly into the other. Both are left for the follow-up on the character
     controller's movement path.

## Added 2026-11-21 (script effects shown by name, #3119)

245. **How do the ITEMS card and STATS > EFF write a Script effect flagged Display Name
     Only?** (P3; #3119, pipboy.md §5.) The GECK wiki says such an effect shows its name
     and that the names of several are put together [documented: GECK wiki Base Effect];
     it does not say where or how. Ours: after the value changes, comma-separated
     ("STR +2, Damage +25%"), the EFF line titled with the item's name [hypothesis]. With a
     Psycho, a Cateye and a Steady carried, read the ITEMS > Aid card of each; use the
     Psycho and read STATS > Status > EFF while it lasts; a screenshot of each answers it.

## Added 2026-11-21 (Ammo Use 0 on a ranged weapon, #3023)

246. **Does a ranged weapon with Ammo Use 0 fire without spending or carrying its ammo,
     and does it ever reload?** (P2; #3023.) The GECK wiki calls Ammo Use "the amount of
     ammo used per shot" [documented: GECK wiki Weapons] and says nothing of 0. Of the
     base game's and the add-ons' 502 WEAP records, 25 have Ammo Use 0; all but one are
     melee weapons or detonators without a projectile; the one ranged weapon is Lonesome
     Road's Laser Detonator (NVDLC04WeapSonicDetonator, automatic, clip 1, Fire Rate 10,
     Microfusion Breeder ammo) [observed: `fnvtool combat form` over the six masters].
     Ours: no round is spent, so it fires at its rate with no reload and needs none carried
     [hypothesis]. In the Divide, hold the trigger on one of the laser targets for 5 s with
     and without Microfusion Breeder rounds carried: count the beams (a video at 30 fps
     does it), watch for a reload, and read the ammo count before and after.

## Added 2026-11-21 (what GetDestructionStage counts, #3053)

247. **Does GetDestructionStage give the stage counted from 1 (DSTD index + 1) or the
     stage's DSTD damage-stage field?** (P2; #3053, scripts.md.) The wiki says it returns
     "the current Damage Stage" [documented: GECK wiki GetDestructionStage]. Counting from
     0 cannot be it: Zeta's generator core tests 8 with eight stages, the Brain waits for 7
     with seven [observed: their scripts and DEST]. Ours: index + 1 [hypothesis]; the
     damage-stage field fits every case found too. They differ on Point Lookout's
     DLC04DarMuckBubble (0x0A012C5E: index 1, damage stage 3, at 40 %): damage one to under
     40 % with the console (`DamageObject`) and print `GetDestructionStage` (2 or 3).
     A second case, in the base game and needing no console (#3298): the mantis egg in the
     Goodsprings schoolhouse (0x0010AC54, V22MantisOotheca: health 1; stages index 0 at
     100 %, index 1 at 99 % with Cap Damage and Destroy and 50 points a second of its own
     damage, index 2 at 0 %; damage-stage fields 0, 1, 2 [observed: fnvtool esm decode]).
     Its script places 12 nymphs (four `PlaceAtMe … 3`) on each OnDestructionStageChange
     where `GetDestructionStage` is 1 or 2 [observed: fnvtool script decompile, SCPT
     0x000EF406]. Shoot the egg once and count the nymphs. 12 says index + 1 (our two
     stage changes, to index 1 and then 2, read 2 and 3). 24 does not settle it: the
     damage-stage field gives it (the changes read 1 and 2), and so does index + 1 if
     the original also runs the block for index 0 (reading 1, 2, 3). Ours: 12 since
     #3053; the smoke scenario `mantis-egg-spawn` expected at least 20 before it.

## Added 2026-11-21 (greetings of two quests at equal priority, #3199, #3061)

248. **Which greeting does an actor say when two quests of the same priority both have
     one it can say?** (P1: All or Nothing cannot be finished on House's path without the
     answer; #3199, #3061, dialogue.md §7 item A.) The runtime takes the one stored first
     (rule 2, an owner statement of 2026-10-10). Two cases where the shipped quest needs
     the later one [observed: records]:
     - On House's path, install the override chip in the Hoover Dam control room
       (VHDControlOverrideBoxSCRIPT runs `VHDMainControlPanel1REF.StartConversation
       Player, Greeting`). Does the panel say 0x001554D8 ("I knew you'd make it.
       Resourceful, as always."; VHDHouseBattle, Say Once) or House's generic Lucky 38
       greeting 0x00125EBD ("The Legion's assault could begin at any moment.";
       VDialogueMrHouse)? Both quests are priority 55; the generic line is stored first.
     - In Fallout 3 (or TTW), talk to Doctor Zimmer in Rivet City before The Replicated
       Man starts: the hire line 0x0002026A (MS08, Say Once) or MS08Fin's goodbye
       0x000C84FF ("Our dealings are done...")? Both priority 50; the goodbye is stored first.
     Our [hypothesis]: at equal priority a Say Once line of one quest wins over another
     quest's plain line. If both come out as the quests need, any further pair that tells
     "Say Once first" from "later quest first" (the second case has the earlier quest win)
     is worth a frame.

## Added 2026-11-15 (the Camp McCarran observation window, #2462)

249. **How does the observation window at Camp McCarran look from each side?** (P3;
     #2462, nif.md §6 "Window glass and environment maps".) Ours since #2462: the pane
     (VWindow01 0x000FB2F4 in CampMCTermInt02, `vwindow01.nif`) draws its environment
     cube `ShinyBright_e.dds` added over its blend, not weighted by its texture alpha
     (about 0.05 in the middle), so grey reflection streaks show from both sides
     [hypothesis]. In CampMCTermInt02 take one frame from the observation side
     (`player.setpos x 112`, `y 1200`, `z 18672`, facing north) and one from inside the
     interrogation room (`x 112`, `y 1700`, `z 18672`, facing south), then a third from
     the room side while strafing past, to see whether the streaks move with the view.
     Streaks moving with the view as bright as the frame agree with ours; a pane that is
     only a faint grey film, or nothing, says the reflection is weighted by alpha.

## Added 2026-11-21 (the angles SetNifBlockRotation sets on a drawn node, #3135, #3138)

250. **Which way does SetNifBlockRotation turn a drawn node?** (P3; #3138,
     [nif-blocks](../design/nif-blocks.md) Conventions.) NVForever takes a node's angles
     as those of the transpose of the rotation it is drawn with [observed: the 10mm
     pistol's ProjectileNode reads 90 0 90 in GetNifBlockRotation, its file matrix's rows
     taken as columns], and so draws a set angle turned the other way from the plain
     right-handed rotation [hypothesis: B42 True Leaning's lean right steps the view +22
     along X and turns its nodes -18° about Y, a roll right only so]. In the original,
     with a pistol drawn in first person, read `player.GetNifBlockTranslationAlt "Bip01 R
     Hand" x y z 1 2` (the world place), run `player.SetNifBlockRotation "Bip01 Translate"
     0 0 30 0 2`, wait a frame and read it again; do the same in NVForever from the same
     place and heading. The hand moving the same way in both settles it. The reading
     back, `player.GetNifBlockRotation "Bip01 Translate" "Z" 0 2`, should be 30 in both.

## Added 2026-11-21 (talking through cell bars, #3233)

251. **Does the crosshair pass collision on the Transparent layer (3), and on Transparent
     Small (26/28): can the player talk to, take, open or search what stands behind it?**
     (P2; #3233, simulation.md §5 "Talking through bars".) The wiki says nothing but
     projectiles passes Transparent and that actors see through it [documented: GECK wiki
     "Collision layer"]; it says the player activates and loots through Invisible Wall
     (27) only. Ours: Talk alone passes the see-through layers, when nothing else is
     offered [design]. In Vault 3 (Vault3b), stand at the cell bars (JailCellDoorway01,
     0x000EF3FA, layer 3) with Rick Lancer (V3Prisoner01Ref 0x000F47B0) behind them: does
     the prompt show Talk and does E open the conversation? With an item dropped behind
     the bars (`player.drop` then nudge it in, or one placed there with `PlaceAtMe`), is it
     offered? A screenshot of each prompt answers it; `GetCrosshairRef` printed at the
     bars says which reference the crosshair holds.

## Added 2026-11-21 (GetMapMarkerVisible on a marker authored Visible, #3268)

252. **What does GetMapMarkerVisible return for a map marker the player has not found
     whose record is authored Visible?** (P2; #3268, #3338.) **Answered 2026-11-22**
     [observed: base game + xNVSE 6.4.9 + JIP LN, plugin version 10, save `nvfdoc`;
     campaign `o31_mapmarker_visible` as a control (`o31_ctrl`) and again with a probe
     plugin that overrode three FalloutNV.esm markers' `FNAM` (`o31_esp`); outputs
     `obs_o31_ctrl_*`, `obs_o31_esp_*` in the private capture folder]: the authored flags
     count before the player finds the marker. Visible alone reads 1, Visible | Can Travel
     To reads 2, Can Travel To alone reads 0, no flags 0. The full rule and the readings
     are in pipboy.md §5.3. #3268 is right for Visible alone and gives 1 where the
     original gives 2 for Visible | Can Travel To. In the base game it matters for one
     marker only: no FalloutNV.esm marker sets any `FNAM` flag (189 of 189), and of the
     add-ons only Honest Hearts' Southern Passage (0x01008885 file-local, `FNAM` 3) does
     [observed: every MapMarker reference of the six masters, 2026-11-22]. Not covered:
     the Hidden flag (4).

## Added 2026-11-21 (melee creatures and a player who backs away, #3256)

253. **How often is a player who backs away hit by a melee creature?** (P1: the owner's
     playtest of 2026-11-21 found it far too easy to back away; #3256, combat-rules.md
     "Knockback".) Ours [observed: main 069ef9bde, 12 s of backing away at a run]: a feral
     ghoul 11 to 12 blows, a coyote 6 to 7, a young gecko 2 to 3 (10 on a standing
     player), because each blow knocks the player back about 50 units and the gecko gains
     only 13 units a second. In the original, on open flat ground (the Searchlight Airport
     runway serves), let one creature at a time attack: a young gecko, a coyote, a feral
     ghoul and a person with a melee weapon. For each, count the blows that take health
     over 12 s (a) standing still, (b) holding the back key at a run, (c) at a walk
     (Caps Lock). Note also whether a blow moves the player (knockback), and whether the
     creature stops and starts on the way at a walk. Counts near ours for the gecko mean
     the difference from a standing player is the original's too; counts near the standing
     ones mean the original closes the gap in a way ours does not (a lunge from further
     off, a faster chase, or no knockback on the player).

## Added 2026-11-22 (terminal text typing out, #3327)

254. **How fast and in what order does a terminal's text type out?** (P3: #3327; hacking.md
     §7 "Typing out".) Ours [design]: each page's lines one after another, each left to
     right, at 600 characters a second; a page starts again only when another page comes
     up (not when the selection moves); no key skips it; no sound. In the original, record
     at 60 fps: (a) an unlocked terminal with a long welcome text and several entries
     (Goodsprings' schoolhouse or any office terminal), from activation to the last
     character; (b) taking an entry that opens a note, and Tab back to the list; (c) a
     locked terminal's hacking screen as it comes up, and after a wrong word. Count the
     frames each page takes and the characters it shows, and say whether lines type one
     after another or together, whether the selection box types with its row, whether a
     key or click finishes it at once, and which sound plays while it types
     (`UIHackingCharSingle`, `…Multiple` or the loop `UIHackingCharScrollLP`).

## Added 2026-11-22 (a terminal note longer than the screen, #3409)

255. **How does the player read on in a terminal entry longer than the screen?** (P3: #3409;
     hacking.md §7.) Ours [hypothesis]: E, Enter or a click turns a page (the last page
     shows the last lines), nothing on the last page; Up / Down scroll a line. In the
     original, open a long entry (a long log, e.g. Vault 11's overseer entries or a
     Lucky 38 terminal) and note how many lines a page holds, what E and a click do on the
     first and on the last page (next page, back to the list, nothing), whether Up / Down
     or the mouse wheel scroll, and any "more" mark the screen shows.

## Added 2026-11-22 (Blake's books for Bitter Springs Infirmary Blues, #3350)

256. **Does a merchant sell the owned items lying at its stand?** (P2: #3350, a player could
     not buy the two books VMS40 asks for; barter-menu.md §4.) Ours [design]: the barter
     list is the merchant container's stock alone. Blake's two books lie on his stand,
     owned by him, and can only be taken as a theft. In the original, with VMS40 at stage
     10 (`setstage VMS40 10`), go to Blake at the Crimson Caravan Company:
     (a) open his barter menu and look for "Stress and the Modern Refugee: A Primer" and
     "Tiny, Tiny Babies: All You Need to Know About Pediatric Medicine", and note their
     prices;
     (b) look at his stand for the two books (the upper shelf by the first aid boxes,
     the lower shelf by the water bottles) and say whether the crosshair shows Take or
     Steal;
     (c) if they are in the list, buy one and see whether the one on the stand goes.
     In the list: the rule is wider than the merchant container (owned loose items near
     the merchant, or these two only), and a scan of owned items at other merchants'
     stands sizes it. Not in the list: the original's way is the theft (or a speech or
     barter line not in our list).

## Added 2026-11-23 (two navmeshes of one cell with no link, #3473)

279. **Do actors walk up the stairs to the Sunset Sarsaparilla HQ's prize room?** (P2:
     #3473, the quest pilot cannot reach Allen Marks' terminal in Legend of the Star;
     navmesh.md §1.2.) The stairs and the two rooms are a navmesh of their own with no
     link to the rest of the cell [observed: records]. In the original, with VMS09a at
     stage 10 or later (the door unlocked) and a follower (Boone, or ED-E), walk from the
     lobby up the stairs into the prize room and wait there 30 s: does the follower come
     up after the player, wait at the foot of the stairs, or appear in the room some other
     way (a jump to the player)? Then wait outside the room and `prid` the follower,
     `MoveTo` it into the prize room, and see whether it walks back down to the player.
     If it climbs, the original joins such navmeshes by a rule the records do not hold
     (say how near the two edges are: 17 to 31 units across, a step of 31 or more); if
     it does not, the rooms are the player's alone in the original too.

## Added 2026-11-23 (Harland's greeting as the player leaves, #3464)

280. **Does Harland greet a player who leaves his room in the REPCONN basement?** (P2:
     #3464, a player could not leave; packages.md §3.) His Dialogue package has an
     activate distance of 1200 and the doorway trigger as its trigger location; a second
     trigger 370 units out on the way to the stairs calls `ResetAI` on him. In the
     original, with VMS01 at stage 30 (the ghouls' friend not found yet): talk to Harland
     and say goodbye, then walk out of his room towards the stairs and on past the reset
     trigger. Say whether he greets again on the way out (where: in the doorway, past the
     reset trigger, not at all), whether the player is walked back towards the doorway
     or held, and whether coming back into the doorway greets again. If he greets past
     the reset trigger without walking the player back, NVForever's 30 s rule matches;
     if he never greets on the way out, the activate distance or sight is read
     differently by the original.

## Added 2026-11-22 (ammo the equipped weapon cannot fire, #3442)

288. **Does the Pip-Boy's Ammo tab dim the ammo the equipped weapon cannot fire?** (P3:
     #3442, a player expected it; with check 193, the mark on the loaded ammo.) Ours
     [design]: every row at full strength; the loaded ammo's row carries the equipped
     square. In the original, carry four kinds of ammo (12 gauge, 20 gauge, 9mm, .357)
     and equip a 12 gauge shotgun, then open ITEMS, Ammo and take a frame; equip the 9mm
     pistol and take another; holster the weapon and take a third; equip nothing (fists)
     and take a fourth. For each frame say which rows are dimmer than the others (sample
     the text's colour of a full-strength row and of a dim one), which row carries the
     square, and whether a dim row can still be selected, dropped or put on a hotkey.
     With a weapon that takes several kinds (12 gauge and its slugs, or the .357 with
     .38 Special), say whether the other kinds it can fire are at full strength.
     **Answered 2026-11-22** (PR #3466's comments): dim at about half, the weapon's other
     kinds full, every ammo row dim with the fists, a dim row still selectable; built in
     pipboy.md §5 ITEMS (#3442).

## Added 2026-11-21 (placing the custom map marker with the mouse, #3324)

262. **What does a left click on an empty spot of the Pip-Boy's World Map and Local Map do
     with the keyboard and mouse?** (P2; #3324, pipboy.md Place Marker.) The capture of
     2026-11-10 showed placement with the X button [observed]; a player reports the
     original places it with the mouse, and how is [hypothesis]. Ours [design]: a click
     (press and release within 6 units) away from any marker does what X does (the same
     question box, or under the No Place Marker Popup tweak an answer at once), of the place
     under the highlight box; a click with a World Map marker under the box asks to travel;
     a right click does nothing. On the World Map at 1280x720, click once an
     empty spot well away from any marker, once a spot about 20 px from one (inside the
     highlight box's reach), and once with the right button; repeat on the Local Map, once
     on a door icon. Screenshot each result: whether a box asks (and its words) or the
     marker appears at once, where it lands against the pointer, and whether a press dragged
     a few pixels still counts as a click.

## Added 2026-11-21 (blood on the world behind a shot actor, #3318)

263. **Where does a shot on an actor leave blood on the walls and floor near it, and
     which impact is it?** (P2; #3318, impact-decals.md §4a.) The engine rolls
     `fCombatEnvironmentBloodChance` (default 0.75, FalloutNV.esm does not set it) and
     casts up to 512 units from the hit in the projectile's direction, first surface only,
     with the hit body part's impact [documented: GECK wiki ITR:OnWoundSpray,
     FCombatEnvironmentBloodChance]. Ours [hypothesis]: a second line straight down when
     the shot's line meets nothing; the body part's severable blood impact data set,
     else its explodable one. Shoot an NPC in the chest ten times with a BB gun
     (`resurrect` if it dies): in the Goodsprings general store from the door (counter
     and wall behind Chet), and in the open street with nothing behind the target. Count
     the splatters on the counter or wall (about 7 in 10 at 0.75), and whether any appear
     on the ground below the target in the open (a frame after each shot does it); the
     splatters' size tells the spurt's decal (`BloodSpoutRedData`, 70-90 wide) from the
     bullet's (`BallisticOrganicImpact`, 12-19).
     **Partly answered 2026-11-22** [observed: base game + xNVSE 6.4.9 + JIP LN, plugin
     version 10, save `nvfdoc`, first person, the window in front; job `o263_blood_10`].
     Setup:
     - `fCombatEnvironmentBloodChance` read 0.75.
     - The player, at the spot `coc GSGeneralStore` arrives at (near the door), aimed at
       Chet's chest (his position + 95, eye + 120) and fired a BB gun with
       `TapControl 4`, 10 times. `tgm` and `tcai` were on, and a screenshot was taken
       45 loops after each shot.
     - Chet was then moved outside onto the road at (-18, 0), 200 units in front of the
       player, and shot 10 more times.
     - Chet's health shows which shots hit (0.4–0.5 off per hit). The aim was a
       script's, not a person's.
     Results:
     - **Store: one splatter in 6 hits.** Hits fell on shots 2, 3, 5, 6, 8 and 9. After
       shot 6 a single cluster of about five dark-red drops appeared on the plank wall
       just right of Chet (from the shooter), about level with his chest. No other shot
       left a mark visible at 1280 × 720 from about 580 units, on the wall or the
       counter.
     - **Open street: blood on the ground below the target.** The road was clean before.
       After the first street shot, a hit, one red splatter lay on the road at Chet's
       feet. The line of fire was level there (pitch −0.1°), with open road beyond.
       Chet walked away down the road during the next shots, so whether later hits mark
       the ground was not seen.
     - **Not measured: the splatters' size**, so which decal they are is still open.

## Added 2026-11-21 (is the HUD's condition bar see-through, #3309)

264. **Does the world show through the HUD's weapon condition bar in the original?** (P3;
     #3309, hud.md §4, §14.3 and §14.7.) A player report says the original draws the bar
     with transparent parts and NVForever's is solid. The report's screenshot is
     NVForever's: its fill reads (175, 125, 45) and its ground (100, 71, 26) over a bright
     sky [observed: the screenshot attached to #3309]; that both are opaque there is
     inferred from the colours they are drawn with and the sky beside them [hypothesis].
     Static evidence, which does not prove the original never shows the bar translucent:
     the ground's template `MeterBackground` is the plain picture `solid.dds` with no
     `alpha` trait [documented: hudtemplates.xml], and the fill's picture `vats_bar.dds`
     has an opaque block [documented: the texture]. On the private shots `hud_corners2` c0
     and c4 the original's fill reads 175/255 and its ground 100/255 of the HUD's colour
     [observed: hud.md §14.7]. Both NVForever drawings draw that. A fill translucent over
     an opaque ground reads the same whatever lies behind the bar, so one background is
     not enough. At 1280 x 720, a weapon at about three quarters' condition:
     - four frames: the bright sky behind the bottom right corner and dark ground there,
       each with HUD Opacity (the Display page's slider [documented: GECK wiki, Complete
       List of GameSettings, `sHUDOpacity`]) at its highest and at a lower setting. In
       each, sample the filled part and the uncovered ground right of the fill
       separately, and the world just outside the bar;
     - if xNVSE can read them, `GetUIFloat` of `alpha` on the fill
       (`HUDMainMenu/ActionPoints/*:1`), on `ActionPoints/MeterBackground` and on
       `ActionPoints` itself, at both slider settings;
     - a low-condition sequence: bring the weapon to about 10 % (hud.md §14.3 records the
       fill's `alpha` moving between samples there) and take several frames a fraction
       of a second apart, with the fill's `alpha` read beside each, to see whether the
       world shows through the fill while it flashes and from what condition it starts.
     And what is the slider's default?

## Added 2026-11-21 (the game's own parallax height maps, #2979)

265. **How deep is the original's parallax on shapes with a slot-3 height map, and is it one
     offset or stepped?** (P2; #2979, nif.md §6 "Parallax height maps", pbr-materials.md
     "The game's own height maps".) What NVForever implements since #2979 [design]: a lit
     shape with BSShaderFlags bit 11 and a slot-3 texture draws it as parallax, on by
     default, at 0.015 UV units, with relief mapping (at most 8 layers, then 4 refinement
     steps), the step's divisor held at a view steepness of at least 0.15 (about 81 degrees
     from the normal; the grazing clamp), no fade with distance, and only on opaque,
     single-sided, non-decal, lit shapes; cut-outs (alpha-tested shapes) stay flat unless
     the development switch `NVF_PBR_PARALLAX_MASK=1` is set. Whether the original draws
     them anything like this is the [hypothesis] this check settles; the GECK wiki calls bit
     11 alone "pretty weak" [documented: GECK wiki "NIF Shader Properties"]. In the
     original, take frames of (a) a McCarran flagstone floor and (b) a rubble pile (one of
     the seven `_p` texture sets nif.md §6 "Normal maps" lists), each at a steep view
     (looking almost straight down), a shallow one (eye height, the surface running away
     from the camera) and a grazing one (the eye a few centimetres above the floor, looking
     along it), close (under 2 m) and far (about 15 m). Compare with NVForever at the same
     views, once at the default and once with `NVF_GAME_PARALLAX_DEPTH=0` (flat). Read: how
     far a stone's edge moves between the steep and shallow frames (in texels of the floor
     texture), whether stones hide the grout behind them at the shallow view (stepped or
     occlusion mapping) or only slide (a single offset), what the grazing view does (the
     texture smeared, the relief flattened, or nothing odd), whether the relief is gone in
     the far frames (a distance fade), whether a cut-out with a height map shows relief, and
     whether the concrete impact decal shows any. The depth switch (0 to 0.08) and
     `NVF_PBR_PARALLAX_METHOD=occlusion|relief:<n>` let the default be fitted to the frames.

## Added 2026-11-21 (the music under the Pip-Boy radio, #3311)

266. **What does the Pip-Boy radio do to the music?** (P2; #3311, audio.md §13.2 item 9,
     radio.md §8.) A player's report says the place's music should stop while the radio
     is on. Ours: while the Pip-Boy plays its station (a line or song sounding now or
     within 8 s) the place's music is turned down to silence over 3 s and plays on
     unheard, battle music keeps its level over the radio, a placed radio leaves the
     music alone, and the place's music fades back in over 3 s where it has got to when
     the radio is turned off [hypothesis]. In Goodsprings at noon
     (`player.setpos x -69000`, `y 3500`, `z 8900`), with the music playing, turn the
     Pip-Boy radio on to Mojave Music Radio: does the music fade or cut, and how fast?
     Turn the radio off: does the music come back at once, after a wait, or only on the
     next place, and does it resume or start its track again? With the radio on, let
     Violet's dogs see the player (north of town): does the battle music play over the
     radio, and does the radio go quiet under it? Then two cases apart, which ours
     treats differently: (a) at the edge of a Radius station's range, where the station
     is still received but heard as nearly all static (Black Mountain Radio near
     `x -47300`, `y 5300`, `z 9000`, radio.md §8), does the music stay away (ours: yes
     while any of the song is heard under the static, no once it is all static)? (b) walked out of reception
     altogether, the Pip-Boy still on, does the music come back (ours: yes)? Last, with
     the Radio volume at 0 and a station playing: is the music still turned down (ours:
     no)?

## Added 2026-11-21 (relief on the landscape's texture layers, #3322)

268. **Does the original draw any parallax or relief on the landscape's texture layers?**
     (P3; #3322, pbr-materials.md "Terrain layers: parallax".) NVForever's terrain
     parallax is an addition of its own until this is answered; ours assumes the original
     draws none, as no landscape texture set is known to name a height texture
     [hypothesis]. Outside Goodsprings at noon, stand on a stony layer (gravel or rocks
     painted over the dirt base), look down at about 30 degrees and strafe slowly sideways
     for a few seconds, then crouch and repeat closer: stones whose painted bumps slide
     against each other as the view moves say the original draws relief; texels that move
     rigidly with the ground say it draws none. The data offers none: no LTEX texture set
     of FalloutNV.esm names a `TX04` map [documented: counted with fnvtool, 2026-11-21;
     pbr-materials.md "Terrain layers: parallax"].

## Added 2026-11-21 (a lakelurk standing on a deep lake bed, #2980)

269. **How does a creature that swims stand on a lake bed deeper than it is tall?**
     (P3; #2980, physics.md §6.6 "Actors standing on the bed".) Ours since #2980: an
     actor whose character swims (on the ground with its feet 115 or more under the
     surface; only an actor on its way has a character, not one that stands idle or
     holds its ground in a fight) but stands still
     with its feet on the bed plays its standing idle (`mirelurkking\mtidle.kf` for a
     lakelurk); moving, its swimming clips [hypothesis]. Before, it held its swimming
     idle (`swimidle.kf`) there; that this is the T-pose a player reported for idle
     lakelurks is [hypothesis] too (an idle one has no character, so never swims). At
     Boulder Beach (Lake Mead, surface 2600), from the shore at (56992, 37536) walk east
     to about x 58300 (feet about 250 under the surface), `tgm` on, `placeatme 00145EDB`
     (CrLakelurkCG), and look at it for 10 s (with `tai` if it moves at once); then `tai`
     again and let it come at you, and watch it whenever it stands still on the bottom.
     A lakelurk standing on the bottom in its standing idle agrees with ours; one held in
     the floating swim pose (limbs out, slow loop) says the original keeps the swimming
     idle at rest, and one that rises to float near the surface says swimming actors
     have buoyancy. Note also how an idle lakelurk on the shore looks before it is alerted
     (a T-pose there in the original would make #2980 the original's own look).
     **Partly answered 2026-11-22** [observed: base game + xNVSE 6.4.9 + JIP LN, plugin
     version 10, save `nvfdoc`, the window in front; campaigns `o33_lakelurk_bed`,
     `o34_lakelurk_near`, `o35_lakelurk_deep`, `o36_lakelurk_view`, fight field set every
     game loop]:
     - **Where it lands.** With the player at (58300, 37536, 2380), `PlaceAtMe`
       CrLakelurkCG at 250 units put the lakelurk on the deep bed about 3,000 units east,
       at about (61200–61300, 38700–38800), in both runs. A `MoveTo` beside the player
       afterwards did not take.
     - **No buoyancy.** With AI on, it rose from where it was placed (z 1302–1332) to
       z 1409–1413 within half a second and stayed there. That was the bed: the player,
       put at z 1420 on the same spot, stood upright on the sand in the screenshot. It
       never rose toward the surface while it wandered (procedure 7, about 13 s), stood
       idle (procedure 0, about 25 s), or held its ground in combat without moving
       (procedure 13, about 35 s).
     - **While it wandered** over about 130 units, its z stayed within 2 units (1408.7–1410.7).
       The terrain's `LAND` heights under those points read 1306–1336. Why they lie
       75–105 units below the bed the player stood on was not looked into.
     - **Not seen: its pose.** In every screenshot the lakelurk was out of view, behind
       the bed's rocks. `GetAnimAction` read -1 throughout. The shore idle was not
       observed: the placed lakelurk again landed on the deep bed.
     Still open: the pose at rest on the bed (standing idle or swim idle) and the
     T-pose question, which need someone at the controls.

## Added 2026-11-21 (Dead Money's hologram colours, #3017)

270. **How does a Dead Money hologram look in each threat state, and does its membrane
     fade with its flicker?** (P2; #3017, effect-shaders.md §4, §5, §9.) The three shaders
     (`NVDLC01HologramNeutral` blue, `NVDLC01HologramAttention` yellow,
     `NVDLC01HologramAggressive` red) are additive membranes with "Edge Effect Inverse"
     [observed: the maintainer's decode in #3017]. Ours since #3017: the edge darkens the
     rim instead of adding a pale one, the membrane stays on through SetActorAlpha and
     fades with the actor's alpha (gone at 0), a threat colour played on a hologram ends
     its other colour, and a neutral play over a threat colour is not played [hypothesis,
     each]. In the Sierra Madre casino (or the Villa),
     take one 1280 x 720 frame of one hostile hologram calm (blue, the player beyond 600
     units), noticing (yellow, within 600) and attacking (red); then (a) whether the rim
     of each is darker or brighter than the middle; (b) during a flicker (the hologram
     blinks out), whether the coloured glow stays visible while the body is gone; (c) after
     a save loaded with the hologram in combat (its ability script plays the red shader
     without stopping the blue one), whether one colour shows or a blue-red mix. Ours
     shows red alone [hypothesis]; (d) after a save loaded with the hologram alerted but
     not fighting, whether it stays yellow or turns blue about a second later (ours: stays
     yellow) [hypothesis].
     **Partly answered 2026-11-22** [observed: the maintainer's run in #3423, base game + all
     DLC + xNVSE + JIP LN, save `nvfdoc`, `coc NVDLC01Vault`, 1280 x 720; job
     `o270_holograms`]. No Dead Money save: a hologram (CREA `NVDLC01CrHologramVaultMale`)
     was placed with `PlaceAtMe` in the Sierra Madre vault and never turned hostile.
     - **Calm look:** at 200 units pale cyan, about (167, 233, 241) over its pixels; by eye
       its edges read brighter than its middle, which carries horizontal scan lines.
       Whether the blue membrane or the model's own texture makes the bright edge was not
       separated, so it neither confirms nor rules out ours (a darkening edge).
     - **Yellow and red: not seen.** With `pms` of `NVDLC01HologramAttention` and then
       `NVDLC01HologramAggressive` (after `sms` of the others) it stayed pale cyan, only
       dimmer or brighter between frames.
     - **SetActorAlpha:** at 0.5 unchanged by eye, at 0.1 still plainly visible (about 0.7
       of its brightness), at 0 nothing drawn and no glow left without the body, also
       after `sms`. Ours agrees at 0; ours fades the membrane in step with the alpha
       between, which the 0.1 frame does not show.
     Still open: (b) the colours during its own flicker, (c) and (d) after a save loaded
     in combat or alert, and the threat colours; they need a hostile security hologram
     with its emitter, in the casino or the Villa, from a Dead Money save.

## Added 2026-11-16 (environment maps on other shapes, #2841)

Each can be compared with ours from one build: the debug switches
`NVF_PLAY_DEBUG_ENV_*` (engine/nvf-assets/src/env_map.rs) draw the variants named
below; frames at the same view and window size on both sides. The items' FormIDs come
from the GECK or `fnvtool`; they are placed with `player.placeatme` in a lit interior
such as the Prospector Saloon (GSProspectorSaloonInterior).

Status lines below come from captures of the original, jobs envmap, envmap2 … envmap5
(run 2026-11-16 at Primm, first person, 1280 × 720; nif.md §6 "Environment maps on
other shapes" has the numbers). They are exteriors only: interior shots fail in the
capture setup, so every part of a check that needs an interior is still open.

271. **How strong is the reflection on a masked prop without the window flag, and does it
     follow the view?** (P3; #2841, nif.md §6 "Environment maps on other shapes".) Ours:
     the cube read along the view reflected about the normal, × the shape's
     `environment_map_scale` × its slot-5 mask, added to the lit colour [hypothesis]. Place
     an empty Sunset Sarsaparilla bottle (SSBottleEmpty, `ssbottle02.nif`, scale 1.2) and
     a full one (SSBottleFull, `ssbottle01.nif`, scale 0.5) side by side about 100 units
     in front of the player; take one frame, then strafe slowly past them and take two
     more. Highlights that move across the glass with the view agree with ours; how much
     brighter the empty bottle's are than the full one's says whether the scale
     multiplies as ours does (about 2.4 times). Compare with `NVF_PLAY_DEBUG_ENV_STRENGTH`
     at 0.5 and 2.
     *Status: partly answered.* The bright patches are fixed in the world (a pot turned
     in place, the held pistol at four headings) and a streak's place does not follow the
     light's direction [observed]; that the view sets it is an inference (the camera did
     not move) and that the patches are the cube is a hypothesis. A fit asks for 1-8.5
     times our strength-1 term by day depending on the object [hypothesis, medium]; ours
     stays at 1 by day (the scene light can only dim it). Still open: the absolute
     strength, and whether the scale
     multiplies: the two bottles' glass differs in colour (4.0-4.6× under moonlight), so
     they cannot show it; a pair with the same texture and different scales is needed.
272. **Does a shape with Environment_Mapping and a cube but no slot-5 mask reflect?** (P3;
     #2841, nif.md §6.) Ours: at full strength over the whole shape [hypothesis]. Place
     an empty whiskey bottle (WhiskeyBottle01Empty01, `whiskeybottle01empty.nif`,
     `shinydull_e.dds`, no mask) and a cafeteria drinking glass (DrinkingGlass01,
     `clutter\cafeteria\drinkingglass01.nif`, no mask) in front of the player and take a
     frame, then one strafing past. Even streaks over the whole bottle agree with ours;
     none at all, or streaks only where the normal map's alpha is bright, match
     `NVF_PLAY_DEBUG_ENV_NO_MASK=none` or `=normal-alpha`.
     *Status: open.* The whiskey bottle is patterned with one glossy patch, not evenly
     washed; the cafeteria glass is evenly milky [observed]; their own textures cannot be
     told from an unmasked term.
273. **Which way is the cube turned on a shape without the window flag, and on a window?**
     (P3; #2841, nif.md §6.) The GECK wiki says the window flag "simply flips the cubemap
     orientation" without saying which axis. Ours until #4392: both read the game-space reflection
     (x, y, z) as (x, z, y); since then only windows do [hypothesis]. In Lucky 38, frame a shape that reflects
     `textures\effects\nv\nvl38reflect_e.dds` (a distinctive cube; 9 shapes in 6 models
     set it without the window flag) from a level view and from looking down at it, and
     do the same for a window shape (both flags) under any cube, e.g. the observation
     window of check 249. Compare where the bright parts of the cube land with
     `NVF_PLAY_DEBUG_ENV_AXES` (`base`, `flip-x`, `flip-y`, `flip-z`, `swap-yz`) and
     `NVF_PLAY_DEBUG_ENV_AXES_WINDOW`.
     *Status: partly answered.* The bright patches are fixed in the world, not in the
     object or the view [observed, firm: envmap4's pot and held pistol]; whether they are
     the cube or the sun's own highlight cannot be told, so that the cube lookup is
     world-fixed is a hypothesis. Which axis is up and what the window flag flips are
     open: no cube content could be recognised. For shapes without the window flag the Goodsprings camper, captured
     2026-11-25, fits the direction (x, y, z) unchanged and the interpolated normal
     [hypothesis; nif.md §6, #4392], now the default (`NVF_PLAY_DEBUG_ENV_AXES=swap-yz`, `NVF_PLAY_DEBUG_ENV_NORMAL=vertex`). Windows and Lucky 38 stay open. Lucky 38 is an interior: not captured.
     The wiki also says Lighting30ShaderProperty flips the cube orientation and is "Used
     by the engine for … advanced effect combinations (e.g. parallax + cube mapping)": if
     the engine switches a PPLighting shape with parallax and a cube to it at run time,
     such shapes would show a flipped cube too [hypothesis]; frame one if any is found.
274. **On blended glass without the window flag, is the reflection weighted by the glass's
     alpha?** (P3; #2841, nif.md §6.) Ours: the shape keeps its blend, so its reflection
     fades with its alpha like the rest of its colour; window shapes are drawn with the
     reflection over the blend (check 249) [hypothesis]. Place a drinking glass
     (DrinkingGlass02, `clutter\junk\drinkingglass01.nif`, masked) in front of a dark
     wall and take a frame close up (about 60 units) and one strafing. A faint film whose
     streaks are no brighter than the glass itself agrees with ours; bright streaks over a
     nearly clear glass match `NVF_PLAY_DEBUG_ENV_BLEND=premultiplied`.
     *Status: open.* The glass tried (the cafeteria glass placed in the air) drew opaque
     in the original, transmission below about 0.1 [observed], so it had no faint alpha
     to compare; the junk drinking glass named here is the one to use.
     **Partly answered 2026-11-22** [observed: job `oenv2_reflections`]. The junk drinking
     glass (DrinkingGlass02, 0005B633, placed) was held 60 units ahead of the flying
     player, the view 20° down onto the rocky ground. It was framed at 23:00 and at
     12:00, then moved 20 units to each side.
     - The same screen patch (75 x 140 pixels) was read with the glass over it and
       without it (the glass moved aside).
     - At night the glass reads brighter and bluer than the ground behind it: mean RGB
       (47, 48, 55) over (20, 19, 21). Its brightest 2 % reads 107 over 37, as pale
       bands up its side.
     - At noon it reads darker than the ground behind it: (78, 72, 55) over
       (133, 119, 88), brightest 2 % 109 over 150.
     - So this glass is far from clear: by day it hides about half of what is behind it.
       By night it carries a pale, bluish film over 2× as bright as the ground. That fits
       a term not weighted down by a faint alpha, but with an alpha this high, ours and
       the premultiplied reading may not differ much [hypothesis].
275. **Does the reflection depend on the light, with and without Env Map Fade?** (P3;
     #2841, nif.md §6.) The wiki: Env Map Fade (Shader Flags 2 bit 15) "Fades the
     cubemap's brightness based on the distance to the nearest light". Ours (since the
     captures below): shapes without the window flag are scaled by the scene light, the
     ambient colour plus the sun's or moon's (an interior's ambient plus directional),
     without N·L, held to 1; point lights are left out; the flag changes nothing
     [hypothesis]. Place a Nuka-Cola bottle (NukaColaBottle, `sodabottle01.nif`, sets the
     bit) and a Sunset Sarsaparilla bottle (SSBottleEmpty, `ssbottle02.nif`, which does
     not: its shader flags 2 are 1) side by side, first under a lamp, then in a dark
     corner of the same interior (or with the lamp's light switched off), and frame both
     each time; then the same in two interiors with dark and bright cell lighting.
     Streaks that follow the cell's light and not the lamp agree with ours; streaks that
     follow the lamp match `NVF_PLAY_DEBUG_ENV_LIGHT=all` (or `=fade-flag` if only the
     Nuka-Cola bottle's do; both rough stand-ins); streaks as bright in the dark as in
     the light match `=off`.
     *Status: partly answered (exteriors).* A streak on a static Sarsaparilla bottle kept
     its place under sun, fire and moon, took the light's colour and fell to 0.33× from
     day to night while the ground fell to 0.07-0.10× [observed for that bottle;
     hypothesis as a rule]; the fit needs about none of the term at night. The flag
     showed no difference: fire-made highlights vanished with the fire on flagged and
     unflagged bottles alike [observed]. No clear glow in the dark: at night the
     env-mapped bottles kept about the share of their day brightness that the no-cube wine
     bottle (Environment_Mapping flag, no cube) kept, some somewhat more: the
     Sarsaparilla bottles 1.1-2.0×, the Nuka-Cola bottle 0.7-1.1× (confounded by the night
     light's direction, and on the blended bottle by the ground seen through it)
     [observed]. Open: interiors, the
     role of point lights, and what L is exactly (sun intensity, ambient, or a light sum
     at the surface).
     **Interiors, 2026-11-22** [observed: job `oenv_reflections`]. A Nuka-Cola bottle and
     an empty Sarsaparilla bottle were held side by side 50 units ahead, in GSGeneralStore
     (lit by its windows) and in Vault34a (dark). Each was framed three ways: alone, with
     a placed light (LIGH VaultWhite256, 00004034) 20 units above them, and after that
     light was disabled.
     - The placed light raised the mean brightness of both bottles' boxes by about the
       same share: the Nuka-Cola bottle 1.45× in the store and 1.41× in the vault, the
       Sarsaparilla bottle 1.52× and 1.33×. Their brightest 2 % rose by 1.4-1.6×.
       Disabling the light gave back the first frame's numbers exactly. So a point light
       changes how the bottles look, whether it is their diffuse light or the
       reflection's.
     - The Env Map Fade flag again made no difference that could be seen.
     - Between the rooms, the wall behind the bottles read 127 in the store and 12.5 in
       the vault. The bottles read 24 and 19 (Nuka-Cola) and 24 and 22 (Sarsaparilla).
       They kept about 0.8-0.9 of their brightness in a room about ten times darker.
     - Rough: the boxes take in some floor and background, and the frames are single
       views, so streaks were not told apart from the glass's own colour.
276. **Do reflections fade out between 1,500 and 1,800 units from the camera?** (P3;
     #2841, nif.md §6.) The INI settings `fEnvMapLOD1` 1500 and `fEnvMapLOD2` 1800
     [documented: GECK wiki "INI Settings"]. Ours: no fade by default [design, kept out of
     #2841 because it would change window shapes too]; under
     `NVF_PLAY_DEBUG_ENV_DISTANCE_FADE=1`, whole up to 1,500 units, none from 1,800,
     linear between, on window shapes too [hypothesis for the curve]. In a long
     room or outdoors at night, frame a shiny prop (e.g. a cooking pot, MSTT CookingPot01,
     `metalpot.nif`) from 1,200, 1,500, 1,650, 1,800 and 2,000 units, with the same view
     direction. A reflection that is whole at 1,500, about half at 1,650 and gone at
     1,800 agrees with the switch's fade; no change agrees with the default; a sudden cut
     says neither is right. The switch's fade goes by the distance to
     the shape node's origin (usually the reference's), so a long shape fades as a
     whole and a model whose parts sit under nodes of their own fades part by part.
     *Status: open.* The running original reported 1500 and 1800 (envmap); setting both
     to 1 in memory changed no picture at 51-1,200 units (envmap5), so either the setting
     does not apply live or the fade does not act there; a crate at 600-1,900 units
     showed no sheen to lose (envmap4). Use an object with a strong visible sheen, the
     camera fixed and the object moved, at a fixed game time.
     **Tried 2026-11-22: no fade seen, weak** [observed: jobs `oenv_reflections`,
     `oenv2_reflections`].
     - The setup: the player flying (`tcl`) 9,000 units above Goodsprings with
       `fov 15`, the view fixed due north and level. A placed cooking pot (CookingPot01)
       was moved along the view to 300, 600, 1,200, 1,500, 1,650, 1,800 and 2,000 units,
       at its height, at 12:00 and 23:00, with the sky behind it.
     - At noon its pixels darker than the sky read: mean 80, 78, 84, 88, 88, 91, 92;
       95th percentile 132, 124, 130, 136, 136, 139, 140. There is no step down between
       1,500 and 1,800; the slow rise fits the sky mixing in at its edges as it shrinks.
     - But the pot showed no distinct moving sheen at any distance, only a matte grey
       with a pale rim, so this does not rule a fade out. The night frames look the same
       by eye.
277. **Does a shape with Environment_Mapping but no cube in slot 4 reflect anything?**
     (P3; #2841, nif.md §6.) 214 such shapes in the data (the wine bottle Wine,
     `clutter\food\winebottle01.nif`, 238 placements; the Pimp-Boy). Ours: no reflection
     (no default cube) [hypothesis]. Frame a wine bottle close up and strafing. A plain
     bottle agrees with ours; moving highlights say the game supplies a cube of its own
     (as the wiki says it does for Eye Environment Mapping).
     *Status: partly answered.* The wine bottle's glass is near black in every view, with
     no streak like the env-mapped bottles' [observed]; its glass texture is very dark,
     so a weak default cube would be hard to see. A lighter bit-7 shape without a cube
     (the Pimp-Boy's) would settle it.
     **Tried 2026-11-22, not settled** [observed: job `oenv2_reflections`; base game +
     xNVSE + JIP LN, save `nvfdoc`]. The Pimp-Boy (ARMO PimpBoy 0011BACB, placed with
     `PlaceAtMe`) was framed at noon 45 units ahead, the player flying (`tcl`) 9,000
     units above Goodsprings. An empty Sarsaparilla bottle stood beside it, and the pair
     was moved 15 units left and right of the view between frames. Its gold body and top
     face read bright, and its silver rings show bright and dark bands with white glints
     in all three frames. Whether the bands move across the rings with the view, as a
     cube's would, or are the sun's highlight, could not be told from three frames
     [hypothesis: either]. A turntable of frames in a dim interior, where the sun's
     highlight is gone, would separate them.

## Added 2026-11-22 (the grab key on pickable plants, #3389)

278. **Can the grab key pick up a White Horse Nettle or a Broc Flower?** (P3: #3389;
     grab.md §3.) Ours [hypothesis]: no, the grab ray passes through a body on the
     non-collidable layer, and these two plants' only body (a dynamic box of 45 kg) is
     there. In the original, by Goodsprings (the nettles south of the town, 0016AF22 at
     -68223, -3810), harvest one and not another, then hold Z on each; also shoot or throw
     a grenade at one. Say whether either moves.
     **Partly answered 2026-11-22** [observed: base game + xNVSE 6.4.9 + JIP LN, plugin
     version 10, save `nvfdoc`, first person, the window in front; jobs
     `o278_grab_plants` and `o278b_grab_broc`]. For each plant, the player stood 70
     units west of it facing east. At pitches 30–82° in 4° steps the job read
     `GetCrosshairRef`, held the grab control (`GetControl 27`: key 44, Z) for 40 game
     loops while lifting the view 25°, read JIP `GetPlayerGrabbedRef` and the plant's
     position, then released the key. One of each pair was first harvested with
     `Activate player 1`. A screenshot confirmed this for the nettle: no prompt on the
     picked one, "E) Pick White Horse Nettle" on the other.
     - **White Horse Nettle: yes, it can be grabbed, harvested or not.** Both 0016AF22
       (harvested) and 0016C86D (not) were named by `GetPlayerGrabbedRef` at pitches 30°
       and 38°. While held they rose 20–26 units and shifted 2–10 sideways, and they
       stayed displaced after release.
       - At 34° and 42° the crosshair named the nettle but nothing was grabbed. From 46°
         on, no grab took for either nettle.
       - After the 30°–38° grabs, the harvested nettle was read at z 7932–7941, about
         170 units below where it was placed, through the ground. At the 42° release it
         read its placed position again.
     - **Broc Flower: no grab.** Both 0016D423 (harvested) and 0016D425 (not), in cell
       (-15, -1), were named by the crosshair at 30–70°. `GetPlayerGrabbedRef` stayed 0
       at every aim, and neither plant moved.
     - **Shot and explosion: neither plant moved** [observed: job `o278c_shots_blast`].
       The console's `placeatme 179DA` after `prid` did not go off in the first two runs
       (no blast in the screenshots, and the first run crashed at that step). A script's
       `rR.PlaceAtMe` of the same frag grenade explosion (000179DA) did go off: there is a
       flash three frames after it. It was placed at the unharvested nettle 0016C86D and
       at the unharvested Broc Flower 0016D425, the player 400 units away. Each plant's
       position (x, y, z) and heading read the same before and 3 s after, to 0.1.
       Before that, three 9mm shots were fired at each from 150 units, aimed by script at
       its base. A bullet mark appeared on the ground at the Broc Flower's foot, and
       neither plant's reading changed. Whether a bullet struck the plant itself was not
       confirmed: the crosshair read no reference at 150 units. These positions are
       meaningful because the same reading followed the grabbed nettles above.

## Added 2026-11-23 (enemies left behind in another cell, #3390)

281. **How long does an enemy left in an unloaded cell stay in combat with the player?**
     (P2: #3390; alert.md §2.) Ours [design]: it is hidden 1 s after it was last measured,
     Lost at 15 s without detecting the player (fCombatDetectionLostTime), Normal once
     hidden for 30 s (fCombatTargetLostRemoveTime), since nothing searches there. In the
     original, have an interior enemy (the Broc Flower cave's rats) fight the player, read
     its `IsInCombat`, `GetCombatTarget` and `GetCurrentAIProcedure` and the player's
     `IsInCombat` once a second, leave by the load door and go on reading for 5 minutes,
     sneaking now and then to see the sneak word. Say when each ends, whether the enemy
     follows through the door, and whether it fights on at once when the player goes back
     in after 20 s and after 2 minutes. Then the same outdoors: outrun an enemy until its
     cell unloads.

## Added 2026-11-23 (the GOODBYE topic without the Goodbye flag, #3477)

283. **Does a GOODBYE line without the Goodbye flag end the conversation?** (P2; #3477,
     dialogue.md §5.) Ours since #3477 [hypothesis]: yes, when the line is over, if
     nothing it links to can be said (none in the data links anywhere). Talk to Keith (NPC
     1EKeith 000F839C, reference 000F83A5, Aerotech Suite 200, cell
     AerotechOfficeInterior02) and pick the generic Goodbye at the end of his topic list
     (INFO 0014E019): does the dialogue menu close after the line, or does his topic list
     come back? Note also whether the generic Goodbye is offered at all, as his only
     flagged entry is the game of Caravan. Jerry the Punk (reference 001413BF) has the
     other line, 0014C483; in Fallout 3, Pronto (Paradise Falls) has 000384CF.

## Added 2026-11-23 (fast travel from an interior, #3434)

285. **Does an interior refuse fast travel, and does CELL DATA bit 2 turn that round?**
     (P2: #3434; fast-travel.md §2.) Ours: every interior refuses with the map's box and
     `sNoFastTravelCell`'s text [documented: GECK wiki EnableFastTravel]; with bit 2 set
     an interior allows travel and an exterior refuses it [hypothesis: "Invert Fast
     Travel Behavior" on the wiki's GetCellFlag page, against the editor's "Can't Travel
     From Here"]. No cell of the masters sets the bit, so set it by script.
     - Needs xNVSE, JIP LN and ShowOff; a save in Goodsprings with no hostile near, the
       player not over-encumbered. First `EnableFastTravel 1` and `ShowMap` with
       canTravel 1 on Primm's marker (000CDF46), then `coc GSGeneralStore` (001057F1).
     - (a) **Unflagged interior**: read `GetCellFlag` 2 and 7 of the cell (expect 0, 0)
       and `GetPCCanFastTravel`; then open the Pip-Boy's World Map, click Primm, and take
       a screenshot: the question, or a refusal? If a refusal, its text, whether it is a
       box with a button or a corner message, and whether the map stays up; compare with
       `GetGameSetting sNoFastTravelCell`. Expect 0 and that text.
     - (b) **Flagged interior**: `SetCellFlag GSGeneralStore 2 1`, read both again and
       click Primm again. Expect 1 and the question; 0 and the refusal mean the bit does
       not invert. Answer No. If (b) reads as (a), leave by the door, come back with the
       flag still set and read once more (it may be read only on entry). Clear the flag.
     - (c) **Flagged exterior**: in Goodsprings read `GetPCCanFastTravel` (expect 1), set
       the bit on the player's parent cell, read again, click Primm and take a
       screenshot; clear it. Expect 0 and the same text.
     - (d) **Fake exterior**: in GSGeneralStore with bit 2 clear, `SetCellFlag
       GSGeneralStore 7 1` (leave and re-enter if nothing changes) and read
       `GetPCCanFastTravel`. Ours: 1 [hypothesis]. With Dead Money loaded,
       `coc NVDLC01stealthTest` (bit 7 in its record) reads the same with no script.

## Added 2026-11-23 (unlit shapes faded by vertex alpha alone, #3443)

286. **Is an unlit shape with a fading vertex alpha, no texture and no NiAlphaProperty
     see-through?** (P3: #3443; nif.md §6 "Unlit shapes faded by vertex alpha alone".)
     Ours [hypothesis]: yes, blended by its vertex alpha. Two places in the original:
     - base game: a bare gurney in the Camp McCarran Concourse (`CampMCTermInt01`,
       `gurney01.nif`, 000FBA8F at 4032, -616, 18240), from standing height beside it. Its
       shape `Gurney01:2` is a black sheet of 144 × 64 units lying under it, alpha 0.6 to 0
       towards the edges. Say whether the floor under the gurney shows a soft dark patch, a
       hard-edged black rectangle or nothing.
     - Lonesome Road: the end of the silo lift (`NVDLC04Silo2Elevator`), from -87, 3502,
       2069 facing 167°. Ride the lift, or play `Right` on
       `NVDLC04SiloElevatorShaft01Ref` to `06Ref` and enable
       `NVDLC04SiloElevatorLightsEndREF`. Say whether the cone of light (350 units wide,
       500 high, from 0, 3126, 2575 down) lets the hall behind it through, how bright it
       is, and whether it lightens the floor under it.

## Added 2026-11-23 (a thrown Lobber with gravity 0, #3441)

289. **Does a thrown Nuka-grenade arc like a frag grenade?** (P2; #3441, combat-rules.md
     "A Lobber whose gravity is 0".) Its PROJ (`NVDLC05GrenadeNukaProjectile`, GRA) is a
     Lobber with gravity 0.0; the frag grenade's (`GrenadeFragProjectile` 000043F9) has
     1.0. Ours [hypothesis]: a Lobber of 0 falls under the full gravity, so both arcs
     match at the same speed. On flat open ground (the Goodsprings playground), aim level
     and throw one of each: does the Nuka-grenade drop and land at about the frag's
     distance, or fly straight on? Throw a plasma mine (`MinePlasmaProjectile`, also 0)
     and a frag mine (1.0) the same way and say whether they land at the same distance.

## Added 2026-11-23 (the meal on the table eating idles, #3610)

290. **Where is the plate of a table eating idle?** (P3; #3610, idles.md §10 "Objects
     hung from the accumulation root".) Ours [hypothesis]: `aogenericmeal.nif` hangs from
     `Bip01` in its bind pose, so the plate sits about 70 units in front of the seated
     actor and 71 above the floor. In Primm's NCR tent (cell PrimmTentsHayes), when Lt.
     Hayes (PrimmHayesREF) sits at the table and plays `SitTableChairEatB` (or
     `PrimmHayesREF.PlayIdle SitTableChairEatB` once he is seated), say whether a plate
     of food appears on the table in front of him, and roughly where it sits relative to
     his chest and to the table top. A student's `aostudenttestpaper.nif` hangs the same
     way, if one is easier to find.

## Added 2026-11-23 (how long the quest lines stay, #3436)

291. **How long do "Quest added", "Quest completed" and "Location discovered" stay on
     screen, and how do they come and go?** (P2; #3436, quest-messages.md §5–§6, hud.md
     "Location discovered".) Ours [design]: each banner 4 s, an objective list 4 s and
     1.5 s more for each objective after the first, each fading in and out over 0.25 s,
     the name's letters coming in over 0.6 s; a player found them too fast against the
     original (#3436). The data does not say: `hud_main_menu.xml` gives the HUD's
     `menufade` 0.25 and no times for `QuestReminder`, and `fDefaultNoticeTextDisplayTime`
     (0.5) is not known to apply. Record at 60 fps, from the first frame a line shows to
     the last: (a) the first "Quest added" of a new game (Ain't That a Kick in the Head, at
     Doc Mitchell's); (b) its
     objective line after the banner, and a list of two or more objectives; (c) "Quest
     completed"; (d) "Location discovered" with its name and "+10 XP" (walk into
     Goodsprings Source from the town). For each: when the words appear, when the name
     has fully come in (and whether it comes letter by letter or fades), when it starts
     to go and how long the fade lasts, and the gap before the next line.

## Added 2026-11-23 (HasFriendDisposition's friend level, #3615)

292. **At what disposition does HasFriendDisposition turn 1, and towards whom?** (P3;
     #3615, `crates/fnv-game/src/functions/actor.rs` `FRIEND_DISPOSITION`.) Ours
     [hypothesis]: 1 when the actor's disposition towards the player is 70 or more. The
     GECK wiki speaks of a "friend disposition game setting" without naming it, and no
     GMST in FalloutNV.esm is one. In the console, select an NPC (Sunny Smiles in the
     Prospector Saloon), `SetDisposition player 100`, then `GetDisposition player` and
     `HasFriendDisposition`; lower it in steps (`SetDisposition player 80`, 70, 60, 50 ...)
     and note the lowest value at which it still reads 1. Then `SetDisposition` towards
     another actor at 100 with the player's at 0: does it read 1 (not towards the player
     only)?
