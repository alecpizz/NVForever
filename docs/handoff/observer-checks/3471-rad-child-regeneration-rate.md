# How fast does Rad Child give Health back at each stage of radiation sickness?

Priority: P1
Needs: the observer, unattended; no particular save (outside Hardcore)
Spec: #3471, [aid-items.md](../../spec-notes/aid-items.md) §3

The four stage spells carry "Regeneration" of magnitude 2, 4, 6 and 8 behind
`HasPerk RadChild`; no record gives the unit. Ours: magnitude Health a second
(`fnv_game::aid::REGENERATION_RATE` 1), evenly, from at most 1 s after the perk, stopping at
the maximum [observed: obs284_rad_child_regen, obs284_stage250; the answer below]. Open:
what the Wait menu gives.

- Set-up: outside Hardcore, indoors, no fight, god mode off, rads not rising, no Solar
  Powered, implant, food or chem effect. `ModActorValue Health 1000`, then
  `DamageActorValue Health 900`, so that no run reaches the maximum; log the maximum
  too, as the stages' Endurance penalties may move it.
- For rads at 250, 450, 650 and 850 (`SetActorValue RadiationRads`), without the perk
  at the start: wait 2 s and log `GetAV Health` every frame for 10 s (it should not
  move); `player.AddPerk RadChild` and log `GetAV Health`, `GetAV RadiationRads`,
  `IsSpellTarget` of the stage's spell and the seconds passed for **60 s**;
  `player.RemovePerk RadChild` and log 10 s more.
- Report per stage: the rise over the 60 s, the size and spacing of the steps (every
  frame, every second, or longer), the delay after AddPerk and after RemovePerk.
- Then with the perk, still logging: rads 450 to 650 and back (does the rate swap at
  once, is there a jump at the swap); rads 199 (does it stop); Health close under the
  maximum at one stage (does it stop there); one hour waited at rads 250 with Health
  damaged by 900 (Health before and after).
- If cheap, the same base effect elsewhere: Solar Powered (magnitude 1) outdoors at
  noon for 60 s and indoors for 10 s.
- If cheap, on other actors (the engine's change makes them regenerate too): the
  Legate (`RegenMed`, magnitude 5, given by `VLegateScript`) and one Dead Money
  hologram (`NVDLC01HologramRegen`, 100). Out of combat, log `IsSpellTarget` of the
  spell (AddSpell it if 0, and say so), `DamageActorValue Health` by half of its
  Health, then log `GetAV Health` every frame for **30 s**. Report the rise, the
  steps and where it stops.

## Answer, 2026-11-25 (partial: the Wait menu is open)

[observed: capture jobs `obs284_rad_child_regen` and `obs284_stage250`, save `nvfairport`
(outdoors, Searchlight Airport), not in Hardcore, TimeScale 30, one sample per frame at about
60 fps; full numbers in the comment on #3564]

- Rad Child gives back the magnitude in Health per **real** second, a little every frame
  (magnitude × the frame's time), not in whole-second steps: 2.00, 4.00, 6.00 and
  8.00 a second at rads 250, 450, 650 and 850 (+119.2, +236.4, +354.6, +472.9 over 60 s
  after the first step). `REGENERATION_RATE` 1 is right. The maximum
  (`GetPermanentActorValue Health`, 1200) did not move with the stages' Endurance (4, 3, 2, 2).
- The perk is noticed late in both directions. Healing starts 0.4 to 0.9 s after AddPerk
  and goes on for 0.3 to 0.8 s after RemovePerk. Nothing moves in the 10 s without the
  perk. One unexplained jump of +15.06 in a single frame at stage 450, once in about
  15,000 frames; the rate is exact without it.
- Stage swap 450 → 650 → 450: the rate changes in the first frame, with no jump. Rads 199:
  healing stops in the first frame. Near the maximum: healing stops at exactly 1200.0.
- One hour: the Wait menu could not be opened unattended, so this was not measured.
  Setting `GameHour` one hour on added nothing, and `TimeScale` 3600 for a game hour
  (0.96 real s) added 1.93. So the clock moving faster does not speed regeneration up.
  Whether the real Sleep/Wait menu gives the hour's seconds (§6) is **still open**: it
  needs an attended run.
- Solar Powered (1): 1.00 a second outdoors at noon, every frame. Indoors nothing, though
  `IsSpellTarget PerkSolarPowered` is still 1.
- The Legate: `RegenMed` was not on him (his script casts it only in the showdown); with
  it added by AddSpell, 5.00 a second. The Dead Money holograms (Clinic and Vendor 1, the
  spell already on them): 100 a second, full again in about 0.45 s. At the maximum their
  Health went 1.5 to 3.2 over it for a frame and was back at 100 the next.
