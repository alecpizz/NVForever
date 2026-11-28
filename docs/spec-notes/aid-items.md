# Spec notes: aid items (chems, food, drink, medicine)

What happens when an actor uses an ingestible (ALCH): which effects apply, what they do
to actor values, how strong they are, how long they last, and addiction; and what time
does to the player's body: effect timers while waiting, Hardcore's needs, the stages of
rads and needs. Code: `crates/fnv-game/src/aid.rs` (the rules), `crates/fnv-game/src/game.rs`
(`apply_magic_item_with`, the effects ticking), `crates/fnv-game/src/functions/actor.rs`
(the temporary pool, the carry capacity), `crates/fnv-game/src/perks.rs` (perk entry
points, §10), `crates/fnv-game/src/needs.rs` (needs, stages, hours passing, §6, §11),
`engine/nvf-play/src/aid.rs` (the player's use, the log), `engine/nvf-play/src/wait.rs`
(hours passing), `engine/nvforever/src/stealth_field.rs` (the Stealth Boy's look). Tool:
`fnvtool actor aid` (§8). Builds on actor-values.md (the pools) and scripts.md §7.6 (active
effects and effect scripts).

Sources, and what each tag means here:

- **[documented]**: the owner's GECK wiki export (pages Ingestible, Ingestible Settings,
  Addiction Formula, ActorValue, Equip Type, and the function pages GetSpellUsageNum,
  IsSpellTarget, EquipItem, CastImmediateOnSelf, GetBaseEffectFlag, GetRadiationLevel,
  GetIngestibleFlag) or xEdit's record definitions (through `schemas/fnv`).
- **[observed]**: counted in the owner's FalloutNV.esm with `fnvtool actor aid` (§8) on
  2026-10-12, or captured in game where a capture is named.
- **[hypothesis]**: our reading, to be settled by a capture (§9).

## 1. The record

ALCH, as xEdit defines it [documented: xEdit ALCH, `schemas/fnv/objects.json`]:

| Field | Meaning |
|---|---|
| `ETYP` | equip type: 10 Chems, 11 Stimpak, 12 Food, 13 Alcohol (the hotkey icon) [documented: Equip Type] |
| `DATA` | weight |
| `ENIT` value | the base price ("Potion Value") |
| `ENIT` flags | 1 No Auto-Calc (unused), 2 Food Item, 4 Medicine [documented: GetIngestibleFlag gives 2 Food, 4 Medicine] |
| `ENIT` withdrawal effect | the actor effect (SPEL) an actor gets when it becomes addicted |
| `ENIT` addiction chance | a number folded into the addiction formula (§5) |
| `ENIT` sound - consume | the sound played on use (audio.md §12 plays it) |
| `YNAM`, `ZNAM` | pick-up and drop sounds |
| `EFID` + `EFIT` + `CTDA`… | the effects, each a base effect (MGEF) with magnitude, area, duration (seconds), range type and actor value, and the conditions that follow it |

- Only base effects with "Self" may be used in ingestibles [documented: Ingestible].
- An ingestible all of whose effects are Detrimental and Hostile is a poison [documented:
  Ingestible]; poisons are not modelled here.

The load order (FalloutNV.esm alone) [observed]: 189 ALCH; equip types Food 129, Chems 34,
Alcohol 15, Stimpak 6, and 5 with code 0 ("Big Guns", which reads as a left default); the
Food flag on 120, Medicine on 10; 34 name a withdrawal effect, 33 have a non-zero
addiction chance; 614 effects.

## 2. The base effect

MGEF `DATA` [documented: xEdit MGEF, `schemas/fnv/gameplay.json`; flag numbers also the
GECK wiki's GetBaseEffectFlag]: flags (0 Hostile, 1 Recover, 2 Detrimental, 4 Self, …,
7 No Duration, 8 No Magnitude), the archetype, and an actor value.

The archetypes of the 614 effects [observed]: Value Modifier 564, Script 26, Value And
Parts 4, Limb Condition 3, Chameleon 2, and 15 effects whose `EFID` 0x0000014F has no MGEF
record in any master (§5.1).

- The `EFIT` actor value and the MGEF's never differ on a Value Modifier or Value And Parts
  effect [observed: 0 of 568]. The code reads the `EFIT`'s, else the MGEF's
  [hypothesis: the same either way in the masters].
- Script effects (26: Nuka-Cola's cap, Cateye, Psycho, Fixer's addiction removal, Super
  Stimpak, Doctor's Bag without Hardcore …) run their effect scripts as scripts.md §7.6
  says; nothing here changes that.
- **Chameleon and Invisibility** (the Stealth Boy's "Stealth Field": Chameleon 75 for
  120 s, or 240 s with a HasPerk condition, and Sneak +100 [observed]) change their actor
  value as a Value Modifier does: with Recover, a modifier while the effect lasts
  [hypothesis: the archetype names the actor value Chameleon]. Detection reads it: a
  target whose Invisibility or Chameleon is 1 or more is seen × fSneakStealthboyMult
  (docs/design/detection.md). The player's body is drawn refracting meanwhile, a bent,
  shimmering outline of what lies behind it [hypothesis, `stealth_field.rs`,
  refraction.md §5].

## 3. What an effect does to an actor value

The ActorValue page's three pools (actor-values.md §1) [documented: ActorValue]:

- A Value Modifier effect **with Recover** puts its magnitude in the **temporary pool**
  while it lasts, and takes it back when it ends (Buffout's Strength +2 for 240 s).
  - Exception: an Ability (SPEL type 4) given to the **player** whose effect has **no
    conditions** puts it in the **permanent pool** [documented: ActorValue, "used for
    example by the Life Giver perk"]. RemoveSpell takes it back [hypothesis].
  - A negative temporary modifier does not take S.P.E.C.I.A.L. or a skill below 0; the
    magnitude is cut when the effect starts and not updated afterwards [documented:
    ActorValue]. The code gives back exactly what it took.
- A Value Modifier effect **without Recover** goes to the **damage pool**: a Detrimental
  one damages the value, the others restore it (never past the permanent value)
  [documented: ActorValue, "base effects … which don't use the Recover flag"].
  - With duration 0 it acts at once [observed: duration-0 effects finish inside the call,
    scripts.md §7.6].
  - With a duration it acts **magnitude per second** while it lasts [hypothesis: the
    Hardcore Stimpak restores 5 for 6 s where the normal one restores 30 at once; Nuka-Cola
    2 for 25 s]. An ability (no duration) without Recover acts once [hypothesis], but for
    a regeneration:
  - **Regeneration** (#3471). The base effect `Regen` (0x000F3D42, named "Regeneration":
    Health, flags Self and No Duration, no Recover) is the only Health one with No Duration
    in FalloutNV.esm and the add-ons [observed: `fnvtool esm decode` of every MGEF in the ten
    masters, 2026-11-24]. Its users: the
    stages of radiation sickness `Radiation200` to `Radiation800` (Disease; 2, 4, 6, 8, on
    `HasPerk RadChild`), and the Abilities `PerkSolarPowered` (1, outdoors from 6 to 18),
    `RegenLow` (3) and `RegenMed` (5; `VLegateScript`, God), Dead Money's
    `NVDLC01RegenDean` (1), `…02` (3) and `…03` (5) and `NVDLC01HologramRegen` (100), and
    Old World Blues's `NVDLC03HologramRegen` (100) [observed]. The perks' words are "you
    will regenerate more and more health" and "slowly regenerate lost Health" [observed:
    PERK DESC of `RadChild`, `SolarPowered`], though the GECK wiki calls a No Duration
    effect "instantaneous" [documented: Base Effect]. So in a permanent effect (an ability,
    a stage's) such an effect that is neither Hostile nor Detrimental gives Health back
    every frame while it is active, never past the maximum, magnitude ×
    `fnv_game::aid::REGENERATION_RATE` (1) per real second [observed:
    obs284_rad_child_regen, obs284_stage250: 2, 4, 6 and 8 at the stages, 1 for Solar
    Powered, 5 for the Legate, 100 for Dead Money's holograms], and not on the dead
    [hypothesis]. Cast once it acts at once. A stage change swaps the rate in the frame
    [observed: obs284_rad_child_regen]; a perk added or lost is noticed within a second
    (0.3 to 0.9 s [observed: obs284_rad_child_regen, obs284_stage250]), here at the next
    condition check (§13). A wait gives the seconds its hours stand for (§6)
    [hypothesis: observer check 3471-rad-child-regeneration-rate, still open for the Wait
    menu; moving `GameHour` or `TimeScale` gave nothing extra [observed:
    obs284_rad_child_regen]]. A game saved before
    2026-11-23 holds it as a single restoration, which a load turns into the rate.
  - Left acting once, for want of evidence that they repeat [hypothesis]: "Restore Health"
    (without the flag) in `GhoulRadHeal` (10, with the subject in radiation), Lonesome
    Road's `NVDLC04MarkedMenRadHeal` (15) and its worn `NVDLC04ArmorPowerNCRScorchedOE` (2),
    and the limb restorations of `MS03PerkMutation` (1 each, from 400 rads) and
    `NVDLC03TorsoRestoration` (100). "Damage Health" 10,000 of the fatal stages
    (`Radiation1000`, `Dehydration1000`, `Starvation1000`, `SleepDeprivation1000`) is
    Hostile and Detrimental and stays at once [observed: their records]. The Monocyte
    Breeder and Old World Blues's regenerating helmets are Script effects [observed].
- The **sign**: Detrimental lowers, the rest raise or restore [hypothesis: the effect names,
  "Increase …"/"Restore …" without the flag, "Reduce …"/"Damage …" with it].
- **Values that grow** — Rad Level (rads), Dehydration, Hunger, Sleep Deprivation — go the
  other way: a "Damage" effect raises them and a "Restore" effect lowers them, never below
  0 [hypothesis: RadAway's effect is "Restore Rads", Nuka-Cola's "Damage Rads"; the GECK
  wiki's GetRadiationLevel reads the player's rads with GetAV]. The change goes to the
  value's base, where SetActorValue would put it [hypothesis].
- **God mode** (`tgm`, ToggleGodMode): an effect that damages the player (a negative
  change to the damage pool, or a need or rads raised) does nothing [hypothesis fitting
  the original: Beer's and Nuka-Cola's Dehydration +20 / +10, held on `IsHardcore == 1`
  [observed: FalloutNV.esm], did not raise Dehydration when drunk in Hardcore with tgm on
  (o7_00, 2026-10-31, #1167), while the Hardcore Stimpak, same condition, worked]. Without
  god mode the drinks raise it, as the help says "some drinks, such as alcohol and soda,
  can increase dehydration" (§11).
- **Value And Parts** ("Restore Health & Conditions", the normal Stimpak and Super
  Stimpak) restores the value by the whole amount and each limb condition by
  `fnv_game::aid::PARTS_LIMB_SHARE` (1/6) of it [design: owner's request 2026-10-23, "a
  small amount of limb health across all limbs"; the share is in no record or setting of
  FalloutNV.esm, so the number stands until the formula is found]. Used on one limb from
  the Pip-Boy's CND page (pipboy.md §5.1, `Game::ingest_on`), that limb gets the whole
  amount and the others nothing [design]. (Before 2026-10-23 every limb got the whole
  amount [hypothesis from the effect's name].)
- **Limb Condition** ("NVRestoreLimbs": Doctor's Bag at once, Hydra 10 a second for 60 s)
  names no actor value: it restores every limb condition [hypothesis]. The Doctor's Bag
  outside Hardcore also runs `RestoreAllBodyPartsSCRIPT` (effect `RestoreAllLimbs`): with
  it every limb is whole after the use; in Hardcore its limbs get 36 × potency
  [observed: `fnvtool actor aid --use NVDoctorBag`, 2026-10-23, Medicine 15: a left arm
  at 0 went to 100, and to 46.8 with `--hardcore`]. Used on one limb from the Pip-Boy's
  CND page in Hardcore (pipboy.md §5.1, `Game::ingest_on` with `limb: Some(code)` on an
  instant Limb Condition effect; duration effects like Hydra stay un-aimed), only that
  limb is restored to full (`amount.max(100.0)`) and the others are untouched [design:
  #3431; hypothesis: exact original-game Hardcore single-limb vs all-limbs magnitudes
  remain to be captured].
- **Hardcore on and off while the game runs** (SetHardcore, `Game::set_hardcore`):
  the needs and the weight of ammunition follow it; the simulation counts ammunition's
  weight by its own switch (`fnv_sim::World::set_hardcore`), which SetHardcore now
  sets too (before 2026-10-23 only the start's configuration set it, so ammunition
  stayed weightless after Doc Mitchell's question turned Hardcore on). RadAway is
  6.5 rads a second for 10 s in Hardcore, 65 at once otherwise [observed: `fnvtool
  actor aid --use RadAway`, Medicine 15].
- The value changes of the masters' aid items [observed]: 257 at once, 173 modifiers while
  they last, 138 by the second.

What reads the temporary pool now: `GetActorValue` and conditions, the Pip-Boy's STATS page
(values, HP and AP), the HUD's health and AP bars (their maximum includes it), the carry
capacity the menus show (§12). Not yet: combat's limb pools (they read the permanent
Health).

## 4. Potency

The beneficial effects of an ingestible flagged Medicine or Food grow with the user's skill
[documented: Ingestible Settings; Ingestible]:

```
Medicine:  fMagicMedicineSkillBase + fMagicMedicineSkillMult × Medicine / 100   (1, 2)
Food:      fMagicSurvivalSkillBase + fMagicSurvivalSkillMult × Survival / 100   (1, 2)
```

- It multiplies the magnitude of Value Modifier effects "not flagged as Hostile"
  [documented: Ingestible, with a {{check}} on the page]; the code also leaves Detrimental
  ones alone [hypothesis], and the durations [documented: EquipItem, "effects … will
  scale"; hypothesis that only magnitudes do].
- The skill is the user's current value within 0–100 [hypothesis: the wiki says "the
  Player's"; an NPC's own skill is used for NPCs].
- With both flags the two multiply [hypothesis: the page says only that they are "NOT
  mutually exclusive"].
- The scaled magnitude is rounded down to a whole number [hypothesis fitting the original:
  in Hardcore at Medicine 15 the Stimpak's 5 a second healed about 6 a second for 6 s,
  36 in all, not 6.5 and 39 (observed 2026-10-31, o7_05, observer-checks.md item 9,
  #1167); the normal Stimpak, 30 at Medicine 17's potency 1.34, restored 40 and with
  Better Healing 48, not 40.2 and 48.24 (captures pk_better_healing*)]. Before
  2026-11-01 the product was kept whole.
- A new character (Medicine 15) heals 30 × 1.3 = 39 with a Stimpak, and 6 × 6 = 36 with
  the Hardcore one.

## 5. Usage, addiction and withdrawal

### 5.1 The usage monitor

- Chems carry an effect `EFID` 0x0000014F that no master defines [observed: 15 items;
  magnitude 30 and duration 108,000 s on 12 (Buffout, Jet, Mentats, Psycho, Med-X's
  Morphine, Rocket, Slasher, Rebound, Steady …), 12 / 2,880 on two, 20 / 3,600 on Ant
  Nectar]. It is the "UMON" usage monitor of the Addiction Formula page [hypothesis:
  GetSpellUsageNum's example returns "the number of times Psycho has been used, multiplied
  by 30", the magnitude these effects have].
- GetSpellUsageNum returns the uses while the monitor is active, times its magnitude
  [documented]. Modelled: the monitor is an active effect of the item; a use while it is
  active adds the magnitude to its count and its duration to its time (the rule for the
  same item again, scripts.md §7.6) [hypothesis]; when it ends the count is gone.
  `GetSpellUsageNum` (opcode 0x122B [documented: catalogue]) is implemented.
- Not modelled: monitors "set to decrease with time" [documented: the function's note
  that it does not account for them].

### 5.2 The addiction roll

[documented: Addiction Formula; Ingestible]

- No withdrawal effect, or an Addiction Chance of 0: no addiction.
- Without a usage monitor: the chance is the Addiction Chance (0.1 = 10 %).
- With one: no chance while GetSpellUsageNum is below `fAddictionUsageMonitorThreshold`
  (default 70); from there `(usage − threshold) / 100`. The record's Addiction Chance is
  then ignored. The usage counted is the value after this use [hypothesis], so Buffout's
  third dose within 30 hours (90) addicts with 20 %, the sixth (180) for sure.
- The roll is made on every use while the actor is not already addicted (seeded game RNG).
- The chance passes through the "Modify Addiction Chance" perk entry point [documented:
  "Addiction Chance accounts for the Modify Addiction Chance Perk Entry Point"]: Chem
  Resistant halves it (§10).

### 5.3 Withdrawal

- Becoming addicted gives the item's withdrawal effect (a SPEL of type Addiction, 10)
  as AddSpell does. `IsSpellTarget Withdrawal…` is 1 from then on, also while the chem is
  active [documented: IsSpellTarget's notes].
- The addiction's value effects are held back while an effect (other than the usage
  monitor) of a chem whose withdrawal it is is active on the actor, and apply when it ends
  [hypothesis: IsSpellTarget's note, an addicted player who has just had the chem "is not
  presently undergoing withdrawal"].
- Withdrawal effects are Value Modifiers with Recover and Detrimental (Buffout Withdrawal:
  Strength −1, Endurance −1) [observed], so they sit in the temporary pool.
- Their conditions (e.g. GetActorValue Ignore Crippled Limbs == 0 on Buffout's) are read
  when the spell is added, as for any effect (scripts.md §7.6). An effect whose
  conditions fail then is not started; one that started has them checked again every
  second as an ability's (§13), and is held while they fail [hypothesis; since
  2026-10-18, for the player's `PlayerConcussed`, crippled-limbs.md §2].
- An effect held by its conditions changes nothing when the chem starts or ends. When they
  pass it looks at the chem then: held back if the chem is active, otherwise applied, once.
  A cure gives back what the effect holds, so nothing while it is held or held back
  [design: `crates/fnv-game/src/aid.rs` `refresh_suspended`, `start_change`; #1580]. A point
  a save lost to the double application of #1580 stays lost: nothing in the save records it
  [design].
- Addictions do not wear off with time here [hypothesis: nothing in the export says they
  do].

### 5.4 Cures

- Fixer removes addictions with a script effect (`AddictionRemovalEffectSCRIPT`)
  [observed], so RemoveSpell (scripts.md §7.6) does it.
- The Cure Addiction archetype (31) removes every addiction of the target [hypothesis
  from its name]; no aid item of the masters uses it [observed].

## 6. Effect timers

- Effect durations are seconds of game-mode time: they stop under menus and in
  conversations and do not count under loading screens [observed: scripts.md §7.6,
  Cateye's 120 s].
- **Waiting and sleeping advance them**: each hour that passes in the Sleep / Wait menu
  counts as the seconds of play in which an hour passes at the TimeScale, 3600 / 30 =
  120 s (`Game::pass_hours`) [hypothesis: the game's help says "waiting, sleeping, and
  Fast Travel will also affect your needs" (HelpHardcoreNeeds, observed in
  FalloutNV.esm); that effects go with them, and at the TimeScale's pace, is ours].
  Buffout (240 s) wears off in the second hour waited; the usage monitors (108,000 s)
  last 900 hours waited. Each effect runs one ScriptEffectUpdate with the hour's
  seconds, a value changed by the second changes by the seconds of its duration left
  (a regeneration, §3, by all of them), and an effect whose time is up finishes
  [hypothesis]. Conditions are checked before each step, with the clock already moved
  [design: the game's own time stands still in the menu, so the once-a-second check
  would not come round, and Solar Powered would heal all night]. **Known limit**: a
  jump of several hours in one step (SetGameHour, fast travel) checks conditions once,
  at the destination, and then advances the whole jump under that answer. Solar Powered
  from 17:00 to 20:00 heals nothing, and from 3:00 to 9:00 the whole six hours. The
  Sleep / Wait menu steps hour by hour, so its error is at most an hour. Not split into
  intervals [design].
- An effect with a duration whose conditions fail when it is applied (a Stimpak's
  Hardcore heal over time used outside Hardcore) is held inactive, as a spell's is
  (§5.3), and its time runs all the same: it goes when its duration has passed, with no
  update or finish of its own, and does not take hold later if its conditions come to
  pass after that (#1438). If they come to pass inside its duration it takes hold at
  the next condition check (once a second, `CONDITION_INTERVAL`) for the time left
  [observed: o26_stim, 2026-11-06, `nvfairport`, Hardcore turned on with
  `player.SetHardcore 1` (without a reference the call fails in a script): a Stimpak used
  outside Hardcore healed 40 at once; with Hardcore on 2.8 s after the use, Health rose
  from 0.9 s later at the Hardcore rate (36 over 6 s, as when used in Hardcore) until
  5.9 s after the use, 12 in all; with Hardcore on 9.3 s after, Health did not move;
  issue #1743]. Before 2026-11-03 an inactive effect's time stood still here, so the
  Stimpak's Hardcore effects stayed listed on STATS EFF for good and would heal whenever
  Hardcore was turned on. Whether STATS EFF lists the held effect in the original was
  not looked at (no screenshot).
- The same item used again while its effect is active extends that effect (one effect,
  one end) [observed: scripts.md §7.6]; a modifier is therefore not doubled by a second
  dose [hypothesis follows from it].

## 7. The player's use in the engine

- The inventory menu removes one, plays the consume sound (`ui_sounds`), and sends
  `ItemUsed`; the script runtime calls `Game::ingest` for the player (§4–§5).
  `EquipItem` of an aid item by a script does the same [documented: EquipItem, "treated
  as though it were used normally"].
- Log lines (`engine/nvf-play/src/aid.rs`): `aid: the player used <item> (potency p):
  <changes>; <value> before → after, …`, `aid: <item>'s effects on the player ended: …`,
  `aid: the player is addicted to …`, and at exit `aid summary: …`.
  The summary's `effects started` and `ended` count the same effects, those a use applied [design]:
  an effect held inactive at the use counts in neither, even if it takes hold later or runs out (#1494).
- The Pip-Boy's STATS EFF list shows each aid item with active value changes, with the
  amounts applied (potency included). Inactive effects (conditions failing) are not listed (#1438).
- Hardcore mode's Stimpak and RadAway effects are chosen by their conditions (IsHardcore,
  HasPerk); the conditions of the masters' aid effects [observed]: IsHardcore 179,
  HasPerk 147, and a few others (§8).
- Not done: NPCs using stimpaks in combat (the combat workstream's); the HUD's H20, FOD
  and SLP codes and the Pip-Boy's needs (the log says the stages, §11); refusing a wait
  that would kill (§11).

## 8. Tool

```
fnvtool actor aid --schema schemas/fnv <Data> FalloutNV.esm [--item Stimpak …]
    [--use Stimpak --use Buffout --damage Health:60 --seconds 10 --hardcore]
```

`fnv.actor.aid/1`: every ALCH with its equip type, flags, withdrawal, addiction chance,
consume sound, usage monitor and its effects decoded (archetype, flags, actor value, how the
value changes, conditions by function name); the counts of §1–§5 (`summary`); the
withdrawal spells and their effects. With `--use`: the player in the authored game takes
the damage, uses the items in turn, then plays `--seconds`; the values at each step, the
potency, the perks applied, the usage and the addiction roll. More options: `--perk P`
(the player has the perk), `--set <actor value>:<value>`, `--wait <hours>` (hours passed
one at a time after the seconds, `--sleep` to sleep them: values after and the stages
changed), `--perks` (every perk's entry point effects, §10, and the stages of §11). With
Hardcore the needs are among the values; with Strength, Carry Weight and the carry
capacity the menus show.

Checked 2026-10-12 (FalloutNV.esm, Medicine 15 on the new player, normal mode): Stimpak
after 60 damage restores 39 at once; Buffout gives Strength +2, Endurance +3 and Health +60
while it lasts, usage 30, no chance of addiction on the first dose.

Checked 2026-10-12 (aid-2): three Buffouts with Chemist and Chem Resistant last 480 s
each (1,440 s merged), the third's chance 0.2 halved to 0.1, Carry Weight and the menus'
capacity 220 while Buffout lasts; a Stealth Boy gives Chameleon 75 and Sneak +100 for 120 s
and ends in the first hour slept; in Hardcore, Purified Water takes Dehydration 195 to 130,
Nuka-Cola gives 10 back and takes 15 of Sleep Deprivation, 3 hours slept take Sleep
Deprivation 235 to 0 (its stage effect Minor goes) while Dehydration rises 9.6 and Hunger
4.8 an hour.

## 9. To capture

1. A Stimpak at Medicine 15 and at another value: the health restored (§4), and whether
   crippled limbs heal by the same amount (§3, Value And Parts).
2. Rads before and after RadAway and Nuka-Cola, and GetAV RadiationRads (§3, values that
   grow); in Hardcore, H2O after purified water and Nuka-Cola.
3. Buffout three times within a few minutes: GetSpellUsageNum after each, and whether the
   Strength bonus doubles or the effect lengthens (§5.1, §6).
4. An addiction: the withdrawal's Strength while Buffout is active and after it ends
   (§5.3), and after Fixer.
5. A wait of a few hours with Buffout active: whether its effect ends, and after how many
   hours (§6: 2 here).
6. Health while Buffout is active and when it ends at low health (whether the drop can
   kill).
7. Hardcore: H2O, FOD and SLP (GetAV Dehydration, Hunger, SleepDeprivation) at the start
   and after 1, 6 and 24 hours waited, and after 8 hours slept (§11: the rates' unit, the
   dehydration rate, what sleep takes off).
8. A Stealth Boy: how long it lasts with Chemist (§10: doubled here, it is a chem by its
   equip type), and the look of the field (§2).

## 10. Perk entry points

A PERK's effects follow its header fields, each from PRKE to PRKF [documented: xEdit PERK
record order]. PRKE holds the effect type (0 quest and stage, 1 ability, 2 entry point),
the rank it belongs to and its priority [documented: `schemas/fnv/gameplay.json`]. The rest
[observed: all 176 PERK records of FalloutNV.esm, `fnvtool esm decode`; `fnvtool actor aid
--perks` lists them]:

- An entry point's DATA is three bytes: the entry point's code, the function and the
  number of condition tabs; the conditions follow as PRKC (the tab, 0 = the perk owner)
  and CTDA; EPFT 1 is followed by EPFD, one float. 109 perks have entry point effects.
- The function: 1 with 0 or 1 (Light Step, Computer Whiz: flags), 2 with additions
  (Educated +2, Toughness +3 and +6 by rank), 3 with factors (Chem Resistant 0.5) —
  read as Set, Add and Multiply [hypothesis from those uses].
- The codes used for aid items, each named by the only perks that use it [observed;
  the meanings documented on the GECK wiki's Entry Point page]:

| Code | Entry point | Perks (number, function) |
|---|---|---|
| 23 | Modify Addiction Chance | Chem Resistant (× 0.5) |
| 25 | Modify Positive Chem Duration | Chemist (× 2), the chem challenge perk (× 1.33) |
| 44 | Modify Radiation Consumed | Lead Belly (× 0.5) |

Rules (`crates/fnv-game/src/perks.rs`) [hypothesis unless said]:

- Only the player has perks here (AddPerk on another actor is refused).
- Of a perk with ranks, only the entries of the highest rank owned apply (the masters'
  ranked entries each hold the whole bonus: Demolition Expert 1.2, 1.4, 1.6).
- Entries apply in ascending priority, then perk FormID, each on the value the previous
  one left.
- The owner tab's conditions are evaluated with the owner as subject; an entry with
  conditions on another tab is not applied (no aid entry point of the masters has any).
- Modify Positive Chem Duration lengthens the beneficial effects (neither Hostile nor
  Detrimental) with a duration of an item whose equip type is Chems, not the usage
  monitor [hypothesis: Chemist's "any chems you take last twice as long"; the wiki says
  "any ingestible"]. The Stealth Boy's equip type is Chems [observed], so Chemist doubles
  it here.
- Modify Radiation Consumed multiplies the rads an ingestible's effects give, any
  ingestible [documented: "Modifies the radiation from ingestibles"].
- The other entry points are read where their systems are (combat, VATS, locks, weight,
  movement: perk-entry-points.md); perks' ability effects and quest-stage effects are held
  too (levelup.md §9, status.md "Character progression").

## 11. Rads and Hardcore's needs over time

Records [observed: FalloutNV.esm]: the actor values Dehydration (H2O), Hunger
("Starvation") and Sleep Deprivation; the settings `fHCStarvationRate` 25 and
`fHCSleepDeprivationRate` 50 (no dehydration rate in any master); RADS, DEHY, HUNG and
SLPD records, five each, whose DATA is a threshold (200, 400, 600, 800, 1000) and a SPEL
(Radiation200 … Dehydration1000: Minor, Advanced, Critical, Deadly, Fatal; the Fatal
ones damage Health by 10,000). The game's help ("HelpHardcoreNeeds") [observed]: the needs
"are continually ticking away"; "Dehydration increases at a faster pace than starvation
or sleep deprivation"; starvation is "slower than dehydration, but quicker than sleep
deprivation"; "waiting, sleeping, and Fast Travel will also affect your needs. If
performing any of these activities would result in death, you will be prevented"; "Some
drinks, such as alcohol and soda, can increase dehydration" — which fits "Damage
Dehydration" raising it (§3); "a few chems and drinks can help offset sleep deprivation".
The JIP functions say the needs build up "as game time passes" [documented: GECK wiki
ToggleHardcoreTracking].

Rules (`crates/fnv-game/src/needs.rs`):

- In Hardcore, each need rises one point every `rate` seconds of play, `rate` its
  setting [hypothesis: the unit; the larger setting is the slower need, so it is time
  per point]. At TimeScale 30: hunger 4.8 an hour (Minor after 42 hours), sleep
  deprivation 2.4 an hour (Minor after 83 hours). Dehydration: `fHCDehydrationRate` if a
  plugin sets it [hypothesis: the name], else 10 s a point, 12 an hour [documented: the
  owner's Hardcore table, 2026-10-23, "~12/h (1 every 10s)"; it gives hunger ~4.75/h and
  sleep ~2.4/h as the settings do; before, a guess of 12.5]. Seen in the original
  2026-10-31 (save nvfHardcore): Dehydration about 11.8, Hunger 4.7 and Sleep Deprivation
  2.2 an hour at TimeScale 30 [observed; observer check 4].
- An hour waited raises them by an hour's worth in whole points: the hour's 120 s of play
  (TimeScale 30) over the rate, rounded down, 12, 4 and 2 [observed: the owner,
  2026-10-31, Hardcore save from all needs at 0, Pip-Boy values: 3 hours waited gave H2O
  36, FOD 12, SLP 6, observer-checks.md item 4, #1177; hypothesis: the rounding down].
  An hour slept does the same for dehydration and hunger; for sleep deprivation it
  first takes 60 away (never below 0) [documented: the owner's Hardcore table, "~ -60/h
  Sleep"; before, a guess of 100] and then adds the hour's 2 [observed: 3 hours slept
  from 0 gave SLP 2, H2O 36, FOD 12; hypothesis: the order, one case] (§6). Hours passed
  together count hour by hour. A share under one point an hour (a TimeScale above 72
  for sleep deprivation, 144 for hunger, 360 for dehydration) would round to nothing, so
  it builds up from hour to hour and gives a point whenever a whole one is reached
  [design: keeps the long-run rate; `Player::need_carry`]. Before 2026-11-01 the waits added 12, 4.8 and 2.4 an hour
  and a sleep ended sleep deprivation at 0.
- The stage effects are the records' [observed: FalloutNV.esm SPEL Dehydration200 …
  SleepDeprivation1000, 2026-10-23]; they agree with the owner's table but for Deadly
  Dehydration (800), whose record has Endurance −3, Agility −2, Perception −2 and no
  Intelligence −1 (the table lists −1 INT). The record is what is applied.
- Restore / Damage on a need lowers / raises it (items, and the script functions
  RestoreActorValue and DamageActorValue, e.g. the Cannibal perk's `RestoreActorValue
  Hunger 50`), never below 0 (§3).
- The stage: the player has the SPEL of the highest stage whose threshold the value has
  reached, added as AddSpell adds it; when the value falls below, it goes (RemoveSpell)
  and the lower stage's comes [hypothesis]. Rads' stages hold in every mode; the needs'
  only in Hardcore, and go when Hardcore is turned off [hypothesis].
- Turning Hardcore off (SetHardcore 0, or the settings page's Gameplay switch "Hardcore
  mode", `hardcore` in `NVForever.ini`) also sets the three needs back to 0 at once, so
  their stages' effects go with them and the needs start from nothing if it is turned
  on again [design: owner 2026-10-25]. The switch follows the game when a script, the
  console or a loaded save sets Hardcore, and a new game that never set it plays by the
  switch (`nvf_play::hardcore`).
- Only the player has needs [design].
- The HUD's words H20, FOD and SLP show each need's stage (hud.md §13; words
  [observed: hud.md §14.2]).
- Not done: the refusal of a wait or sleep that would kill; the messages
  `sDehydrationIncrease` and the like (the log says the stage instead).

## 12. Carry weight

The capacity is `fAVDCarryWeightsBase + fAVDCarryWeightMult × Strength` (150 + 10 ×
Strength, simulation.md §3). The Strength in it is the current one, temporary modifiers
included, within 1–10, and Carry Weight's own modifiers add to it [hypothesis: GECK wiki
GetCalculatedMaxCarryWeight calls CarryWeight an actor value; the formulas read current
values, actor-values.md]. The game layer keeps the world's capacity (the menus' Wg line,
`fnv_sim::World::encumbrance`) at the current Carry Weight whenever Strength or Carry
Weight changes: Buffout's +2 Strength gives 220 at Strength 5 while it lasts.

## 13. Authored abilities and worn enchantments

An actor's own abilities and the enchantments of what it wears are active effects like
an added spell's, so IsSpellTarget, HasMagicEffect, GetActorValue and RemoveSpell see them
(code: `crates/fnv-game/src/abilities.rs`; tests: `crates/fnv-game/tests/abilities.rs`).

What the records hold:

- **NPC_ / CREA `SPLO`** "Actor Effect" [documented: schema `gameplay.json`]: 838 actor
  records of FalloutNV.esm list 1414 of them, 1388 of type Ability (RadImmunity 383 times,
  PoisonImmunity 198, PerkToughness 121, GhoulRadHeal 86, the creatures' DT abilities) and
  26 of type Actor Effect (Ruthless, LegionExplorerAura, SporeCarrierRegen: effects with an
  area or a duration) [observed, 2026-10-17]. With the ACBS template flag "Actor Effect
  List" the template's list counts [documented: schema template flags; observed:
  GSVictorRef's base Victor sets it, its template VSecuritronMk1TEMPLATE lists
  SecuritronDT, RadImmunity and PoisonImmunity].
- **RACE `SPLO`**: no race of FalloutNV.esm has one [observed]; read as NPC_'s for plugins
  that add them [hypothesis].
- **ARMO `EITM`**: the armour's object effect, an ENCH [documented: GECK wiki ActorValue,
  "the various object effects found on armor pieces"; observed: 186 ARMO of FalloutNV.esm,
  each naming an ENCH of ENIT type 3 Apparel]. WEAP `EITM` names one of type 2 Weapon (59
  of them), applied by a hit instead (hit-effects.md).

The rules (capture `abilities-1`, 2026-10-17: the probe file sits in the coordinator's
requests, the outputs in the private capture folder `probe\abilities-1`; all 79 probes both
sides answered agree with ours):

1. **Authored abilities join the spell list once per actor**, the first time it is
   processed: every frame for the player and the actors in the loaded cells, and before a
   script asks IsSpellTarget, HasMagicEffect, GetActorValue, GetPermanentActorValue,
   AddSpell, RemoveSpell or Dispel of it. Only type Ability joins; an SPLO of type Actor
   Effect is a spell the actor could cast [hypothesis]. The actor is marked so they are
   not added again (saved, `RefExtra::abilities_given`).
2. **Abilities are active only while the actor is loaded** (the player always): the
   abilities in its spell list start when it is in a loaded cell and end when it is not
   [observed: GSVictorRef seen from the gas station, RexREF and LilyRef far away read
   IsSpellTarget 0, HasMagicEffect 0 and no modifier (Victor's DT 0, Lily's RadResist 14);
   Victor next to the player read 1 and DT 20 = his SecuritronDT; hypothesis: a disabled
   actor counts as not loaded, and an ability AddSpell gives an actor out of the loaded
   area waits for it].
3. **Race abilities follow the race**: the player's race is the character creator's
   choice; when it changes, the old race's abilities go and the new race's come
   [hypothesis; no race of FalloutNV.esm has any].
4. **A worn enchantment lasts while the item is equipped** on a loaded actor: its effects
   start when the item is equipped (EquipItem at once, otherwise in the frame) and end,
   with their finish blocks, when it is unequipped or leaves the inventory [observed: the
   gambler hat: IsSpellTarget of its enchantment 0 carried, 1 worn, 0 unequipped and 0
   once removed from the inventory while worn; Perception 5, 6, 5; reinforced metal
   armour's Reduce Agility 5 to 4 and back; documented: GECK wiki ActorValue]. The effect
   records the item (`ActiveEffect::worn`), saved with it. IsSpellTarget of the ARMO
   itself is 0 [observed]. An actor never loaded (no inventory yet) wears nothing.
5. **Pools**: a worn enchantment's Recover value modifier is in the temporary pool; an
   ability's without conditions is in the permanent pool for the player and the temporary
   pool for anyone else [documented: GECK wiki ActorValue; observed: the hat leaves
   GetBaseAV and GetPermanentActorValue Perception at 5; Victor's DT ability leaves his
   permanent DT at 0; the player's added PerkToughness raises GetPermanentActorValue
   DamageResist to 10] (§3).
6. **Conditions**: an effect whose conditions fail is listed but inactive: no value
   change, no script; IsSpellTarget and HasMagicEffect both count it [observed: the Four
   Eyes glasses worn without the trait: IsSpellTarget 1, HasMagicEffect IncreasePerception
   1, Perception 5]. Its conditions are checked again every second and it takes hold (or
   lets go, its modifier leaving the pool and its finish block running) when they change
   [hypothesis: the interval, and suspended rather than removed].
7. **Removing them**:
   - RemoveSpell takes an authored ability away for good; AddSpell puts it back
     [observed: Victor's SecuritronDT: IsSpellTarget 0, DT 0, RadImmunity kept; AddSpell:
     1, DT 20].
   - Dispel of an ability ends its effects [observed: Victor's SecuritronDT, DT 20 to 0];
     it stays in the list and comes back when the actor is loaded again [hypothesis].
   - Dispel of a worn item's enchantment ends it until the item is equipped again
     [observed: the hat: 0 and Perception 5 after Dispel, 1 and 6 after unequip and
     equip].
   - DispelAllSpells leaves abilities [observed: Victor's RadImmunity and the player's
     PerkToughness stayed] and worn enchantments [hypothesis: the probe's hat had already
     been dispelled].
   - RemoveSpell of an enchantment leaves a worn item's effect [hypothesis, as above].

8. **Conditional AddSpell admission**: a spell is already in the actor's list while
   its conditions are evaluated. During immediate effect application, the existing
   actor-effect synchronization guard prevents a condition's actor-value read from
   synchronizing that same listed spell a second time. The previous guard state is
   restored afterward, including when admission was nested inside synchronization.
   A repeated AddSpell still refuses an existing list entry. Removing the spell
   releases its single effect and modifier through the normal removal path.

The authored regression uses a conditionally active ability with one 50-point
modifier: AddSpell must create one active effect and contribute 50, repeated reads
and another AddSpell must leave that count unchanged, and RemoveSpell must return
the temporary modifier to zero. Separate controls cover a false condition becoming
true, an empty ability, invalid magic-item refusal and later synchronization of a
different actor. These are NVForever implementation regressions, not native timing
or original-mod acceptance claims.

9. **Live `SetObjectEffect` (`EITM`) overrides**: `SetObjectEffect form [effect]` sets or
   clears (when `effect` is omitted or null `0`) the object effect (`EITM`) of an `ARMO`,
   `WEAP`, or `EXPL` base form [documented: GECK wiki SetObjectEffect], rejecting
   mismatched enchantment types (`ENIT` type 3 Apparel for `ARMO`, type 2 Weapon for
   `WEAP`/`EXPL`), invalid non-form second arguments, and unsupported base form types
   [observed: gaps6, apparel enchantment refused on a pistol; hypothesis: `EXPL` requires
   Weapon type and other record types or non-form second arguments are refused].
   `Game::object_effect` and `Game::object_effect_override` unify `SetObjectEffect`
   overrides ahead of authored `EITM` data for `GetObjectEffect`, `Game::worn_enchantments`
   (filtered to `ARMO` + Apparel), and `Game::item_effect_text` / `GetItemEffectString`.
   `TempCloneForm` snapshots effective `EITM` via `Game::snapshot_object_effect` so
   pre-clone edits carry onto the clone while post-clone edits on the source or clone stay
   isolated, including clone-of-clone chains [design: copy-at-clone snapshot isolation].
   When an `ARMO` override changes, stale `worn_dispelled` entries for that item are
   cleared and loaded actors' worn enchantments resync immediately [design: immediate
   resync on live edit]. Overrides on loaded base forms carry across `Game::take_session`
   / `Game::put_session` so startup (`gr_`) edits survive save loads within the same
   session [hypothesis: session-scoped base-form edits are not save-baked; transient
   `TempCloneForm` overrides are excluded]. Engine-side weapon/explosion hit-effect
   consumers (`hit_effects.rs`) are not modified by this core-only change and can query
   `Game::object_effect_override` in a follow-up.

Not settled: GetActorValueModifier did not compile in the game's probe script (the
function needs a different form), and the script name `PoisonResist` (the original answers
125 on Victor: 25 + PoisonImmunity's 100) is missing from our decoder's actor value
names (`crates/fnv-script/data/encodings.json` lists the names seen in compiled scripts).

## 14. The EFFECTS figure (the Pip-Boy, barter and container menus)

An aid item's EFFECTS card (pipboy.md §5) is what using it would do to the player now
(`fnv_game::Game::aid_preview`), not the record's list of effects:

- Only the effects whose conditions pass for the player (subject and target the player, as
  `Game::ingest`) [observed: Programmer's Digest `MagazineNVScience` holds four Science
  effects, +10/+20 for 60/180 s, behind HasPerk Comprehension (0x31DE1) and Retention
  (0x10F09E) checks; the Blood Pack's +4 Health for 5 s needs HasPerk Hematophage
  (0x3131), its +1 has no condition; the Stimpak's four Health effects split by IsHardcore
  and HasPerk 0x94EBF; `fnvtool esm decode`].
- One line per actor value, the applying effects on it added up [owner's playtest list 7
  item 5: "it should only show a single Science +10"; design: adding up is what using it
  does]. An effect over time counts whole: its amount a second times its seconds
  (Chemist-lengthened for chems, §10) [hypothesis: how the original writes one is not
  known; a total reads like the instant ones].
- With the item's potency (§4) [hypothesis: the original's Stimpak shows "HP +40" on the
  owner's screenshot, its record's instant effect 30, which fits a Medicine potency near
  1.33].
- Without the scripts' game the record's effects are listed as before (no conditions).
