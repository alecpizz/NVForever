# Actor flags and actor data from the ESM (ESM gap audit, 2026-10-22)

The ESM gap audit's medium group "Actor flags"
([esm-gap-audit-2026-10-21](../handoff/esm-gap-audit-2026-10-21.md)) lists NPC_ and CREA
flags and related records that were decoded but never used. Meanings are from the GECK
wiki pages Category:NPC, Class, Stats Tab - NPC, VoiceType, LeveledItem and
Category:Factions [documented]; bit layouts from xEdit (schema `gameplay.json`). Counts
are from FalloutNV.esm [observed: a raw read of the master, 2026-10-22]: 3,816 NPC_ and
1,578 CREA records.

## 1. No VATS Melee (ACBS bit 20)

"The player cannot go into VATS with this NPC if he has a melee weapon equipped"
[documented: Category:NPC]. One NPC (`vl38SuiteVendor`) and ten creatures, all ceiling
turrets. "Base Data does not include … No VATS Melee" [documented: Template Data], so the
flag is read off the actor's own record (the leveled entry it resolved to). In VATS mode
(docs/design/vats-mode.md) a mark on such an actor with a melee or unarmed weapon is
refused and logged; with a gun it is marked as before.

## 2. No Blood Spray, No Blood Decal (ACBS bits 11, 12)

Seven creatures have both: the three tumbleweeds, two of the explosive Nellis ants and two
spawn tests. A hit on such an actor plays its impact's sounds, but its impact model (the
spray) and its decal are not shown. The flags are read from the definition that supplies
the actor's model ("Use Model/Animation includes the Blood … tabs" [documented:
Category:NPC]; hypothesis: the two flags go with that tab). Logged at exit: `shot fx: …
impacts on actors flagged No Blood Spray not shown`, `decals: … hits on actors flagged No
Blood Decal left none`.

## 3. CREA ACBS bits 27–31 (corrected 2026-10-24)

The schema followed an xEdit list that names bit 27 twice ("Not Pushable / Allow
Pickpocket"), so every later name sat one bit too low. The GECK wiki's ActorHasBaseFlag
table gives, for creatures, 27 Not Pushable, 28 Allow Pickpocket, 29 Is Ghost, 30 No
Rotating To Head-track, 31 Invulnerable [documented: owner's export, functions.xml]. The
records agree [observed: raw read of the ten masters, 2026-10-24]:

| Bit | Name | FalloutNV.esm | Examples |
|---|---|---|---|
| 28 | Allow Pickpocket | 124 | wild dogs, giant rats, big horners, ED-E's upgrade forms |
| 29 | Is Ghost | 6 (DM 4, OWB 4) | the tumbleweeds, Festus, the Gun Runners' protectron, Dead Money holograms, OWB's brain tank |
| 30 | No Rotating To Head-track | 4 | the tumbleweeds, CG02Andy |
| 31 | Invulnerable | 10 (DM 20, OWB 4) | Mr. House, Yes Man and his mainframe, House's securitrons, Festus, ARCHIMEDES, the Dead Money holograms |

What this changes:

- **Invulnerable** (`Game::is_immune`) now holds for the creatures above; before, the
  tumbleweeds and CG02Andy (bit 30) were invulnerable and Mr. House was not.
- **Is Ghost** is applied: the actor counts as SetGhost 1 from the start ("cannot be
  harmed … Projectiles pass through them … Enemies … will not go into combat or detect"
  [documented: GECK wiki SetGhost notes]): `Game::is_ghost` reads the flag from the
  definition supplying the base data, so shots and blows pass through such creatures
  (`nvf_play::combat::ghosts`), they take no damage and enemies ignore them. SetGhost 0
  clears it for that reference [hypothesis: the script wins over the base flag];
  GetIsGhost reads both. The earlier note here said "Is Ghost" (then read off bit 28)
  could not mean SetGhost's ghost because dogs and rats had it: that was bit 28, Allow
  Pickpocket.
- **No Rotating To Head-track** moves from bit 29 to bit 30 (actor-fields.md §1).
- **Allow Pickpocket**: docs/spec-notes/pickpocket.md.

Smoke: `ghost-tumbleweed` (shots pass through a tumbleweed), `blood-none-ant` (the No
Blood checks of §2 now on an explosive Nellis ant, as the tumbleweed is a ghost).

## 4. Allow PC Dialogue (CREA ACBS bit 21)

170 creatures have it, among them the securitrons, Mister Handys, ED-E's upgrade forms,
Yes Man's mainframe and some feral ghouls. A creature is offered for talk (the activate
prompt says Talk) when it has the flag and a greeting to say. Before, the greeting alone
decided ([hypothesis: both]; the CREA page is not in the wiki export). It is read from the
definition supplying its base data. As before, a hostile creature only shows its name.

A living non-hostile creature shown by name only (`Action::Name`) whose script carries an
`OnActivate` block still runs that block on E (`script_activate_by_hand`, #3669; e.g.
`CRBrahminScript` brahmin pushing [documented: GECK wiki PlayIdle uses the Brahmin Push as
its activation example]). The name-only prompt stays, and the default activation behind
the block stays "nothing": if the block falls through to plain `Activate` on the living
creature (e.g. standing or with a weapon out), any resulting `Outcome::Talk` on that
creature is suppressed so it neither opens a conversation nor posts a "has nothing to say"
message [design: preserves playtest list 4 item 19 and #3669]. Dead creatures keep
`Action::Search`, creatures without an `OnActivate` block are unchanged, and hostile
actors keep #2536's rule.

## 5. NPC_ NAM6 Height

0 on 2,601 NPCs and 1.0 on 1,124; the rest lie between 0.85 and 1.1. The drawn scale of an
NPC is the reference's scale × the race's height for its sex (actors.md §3) × its own
height, where 0 counts as 1 [hypothesis: the NVSE functions GetNPCHeight / SetNPCHeight
read and write it; the wiki does not say how it is applied]. Hit volumes and the
character capsule are not scaled by it.

## 6. Chance none on leveled actor lists (LVLN, LVLC)

"The chance that the leveled list will not generate an item" [documented: LeveledItem];
actor lists are taken to work the same [hypothesis]. Before it picks an entry, a list
with a Chance none above 0 rolls 1–100 against it and stands for no actor when the roll
is within it. The engine's appearance resolve (`fnv_actor::resolve`) and the game's
(`fnv_game::templates`) roll it the same way from the reference's seed, so they agree. A
list with 0 draws nothing for it, so its picks are unchanged. Every list in FalloutNV.esm
has 0, so nothing changes in vanilla.

## 7. Classes (CLAS) and SetClass admission

Auto-calc NPCs (2,283 in the master) carry the attributes and skills the editor
calculated in their own DNAM and ACBS. The wiki gives no formula ("The level and class of
the NPC is used to determine the final values" [documented: Stats Tab - NPC]), so the
stored values stand. The class's services already serve Autocalc Service (vendor-services).
The Guard flag has no documented effect besides the NVSE GetClassFlag. Tag skills,
attributes and the Guard flag are not decoded.

`actor.SetClass class` changes the actor's class; the page also requires recalculation
for actors with Auto-calc stats [documented:
[SetClass](https://geckwiki.com/index.php?title=SetClass), catalogue entry]. Recalculation
remains open: class/level-dependent attributes and skills must be measured before a
numeric policy can be implemented. `DuplicateNPCStats` likewise requires class, level,
and recalculated stats, with Auto-calc stats on the documentation's "target actor"
[documented:
[DuplicateNPCStats](https://geckwiki.com/index.php?title=DuplicateNPCStats), catalogue entry].
It has no implementation; the target's identity and recalculation inputs need a probe.
Copying a class alone would not satisfy that contract.

NVForever's existing SetClass policy is retained: the calling reference's base receives
the class override, ahead of its Traits template; references sharing that base see the
override, and the override is saved in `.fos` [observed: Fallout: New Vegas 1.4.0.525 +
xNVSE 6.4.9, 2026-11-24, 10 official base masters, `GSDocMitchellHouse` (`0x00103DF9`),
isolated clean-room save/reload probe `w4f_04_set_class`: sibling `PlaceAtMe
DoctorBarrows` (`0x000156F0`) `s1.SetClass ArcadeClass (0x0017657B)` updated `s1`, `s2`,
and base `00015679 -> 0017657B` (base-form shared); `DocMitchellREF` (`0x00104C0F`, base
`0x00104C0C`) baseline `00015679`, mutated `0017657B`, same-process baseline reload
`00015679`, same-process mutated reload `0017657B`, fresh-process with `.nvse` `0017657B`,
and fresh-process `.fos`-only `0017657B`; zero DLL/PDB/source consulted]. Its real
service-category consumer is `Game::services` for Autocalc Service actors
(vendor-services.md). Auto-calc stats and Autocalc Service are separate flags; exercising
the latter does not demonstrate stat recalculation.

[observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-24,
GSDocMitchellHouse (`0x00103DF9`), 10 official base masters, isolated staging copy
(read-only), committed probe `w4g_05_isactordetected_and_setclass_invalid` in
`tools/capture/nvse/catalogs/wave4g_f2_extenders.json` (companion catalog PR #3803);
no DLL or source consulted] `SetClass` on a non-actor door (`rDoor.SetClass rArcade`)
and on an explicit null receiver (`rNull.SetClass rArcade`) BOTH continue script
execution safely without mutating any actor (`scDoor=1`, `scNull=1`). Conversely,
passing a non-`CLAS` form (`rDoc.SetClass rStim`, `0x00015169`) or a null class
argument (`rDoc.SetClass rNull`, `0`) BOTH abort the script at runtime (`cmp=1`,
`ok=0`), leaving Doc Mitchell's class unchanged (`00015679`). Verbatim probe output:
- `PROBE|w4g_05_isactordetected_and_setclass_invalid|invalid(detDoor=1:0,detNull=1:0|scDoor=1,scNonClas=0,docCls=00015679->00015679,scNull=1)`
- `PROBE|w4g_05_isactordetected_and_setclass_invalid|setclassArg(nonClas:cmp=1,ok=0,cls=00015679->00015679|nullClas:cmp=1,ok=0,cls=00015679)`

An omitted source-call receiver still selects the calling reference. An explicit null
or non-form receiver logs `no_reference_call`, returns `0` without falling back to
implicit `self`, changes no class override, and continues script execution. A non-`CLAS`
or null class argument returns `VmError::RuntimeAbort` in both strict and default modes,
aborting the script immediately while leaving class state unchanged. A nonzero unknown
FormID or nonactor receiver with a valid `CLAS` argument returns `0`, changes no class
override, and continues [design: non-form numeric receiver and unknown-FormID safety].
Actor admission resolves a placed LVLN/LVLC using its seed and encounter level without
following any template field, then retains SetClass's existing base override key and
Traits precedence. This avoids rejecting valid leveled actors or attaching classes to
nonactors. Actual service selection, placed NPC/creature, template and JSON restore
controls remain in `tests/common/set_class_receivers.rs`; these are authored controls,
not original-game observations. Large numeric source controls use decimal-point literals
with source-local value readback because integer-only tokens saturate before receiver
admission.

`tools/capture/actor_state_observers.py` generates read-only snapshots for controlled
original-game comparisons. The capture protocol is in its help text; no results have
been collected for the new comparisons.

## 8. Special Combat (FACT flag bit 2)

"If a faction is flagged as Special Combat, members can attack one another without
sending a combat alarm" [documented: Category:Factions]. Four factions:
`VPitFighter01Faction`, `VPitFighter02Faction`, `VThornDwellers`,
`vHDNCREngineerDialogueFaction`. When an actor starts fighting or notices one with whom it
shares such a faction, it sends its group no alarm (docs/design/combat-ai.md §12.3). Logged:
`combat group: … sends no alarm about …: both in …, a Special Combat faction`. Not built:
the wiki's other effect, ignoring fSneakSeenMin and iCombatTargetPlayerSoftCap.

## 9. Allow Default Dialog (VTYP DNAM bit 0)

"If checked, voice type includes access to generic dialogue … If unchecked, a line of
dialogue must include one of the following condition functions for it to be valid for
this voice type: GetIsVoiceType, GetIsID, GetInFaction, GetIsClass, GetIsRace, IsInList"
[documented: VoiceType]. The bit is read raw from VTYP DNAM (bit 1 is Female) [documented:
xEdit; observed: every generic adult voice has bit 0; the unique, super mutant, robot and
four MaleAdult09–12 voices do not; 61 of 100 lack it]. An actor whose voice type (from the
definition supplying its traits) lacks the bit says no line unless the line has one of
those six functions among its conditions, or its quest has one among the quest's
conditions (which gate every line of the quest and have to pass too) [observed: ED-E's
greetings 0x001579D7 and 0x001579D8 have no conditions of their own, their quest
vDialogueEDE has GetIsID on the three ED-Es, and the voice type RobotEDE lacks the bit;
hypothesis: the original counts the quest's conditions, since its ED-E says these lines.
Without it the first greeting, whose result script gives the controls back after the
repair in Primm, was never said: #2378; checks: unit test `voice_gate`, smoke
`ede-repair-greeting` and `mq-companion-ede`; what the original does for the Generic
quest's voice-type list is observer check 127]. A line whose INFO names its speaker (ANAM) is
allowed too, since it is said by that actor only [hypothesis]. A VTYP without DNAM
(9 records) is not restricted [hypothesis]. The player's own lines are not gated.

## 11. Fixtures that never fight (Immobile, Unaggressive, Helps Nobody, unarmed)

`Game::is_fixture` (fnv-game awareness.rs; issue #2431): a creature flagged Immobile
whose AI data is Aggression Unaggressive and Assistance Helps Nobody, carrying no
weapon (its inventory, or before it is loaded its base's CNTO, a leveled item list
counting as a weapon). Such an actor is never hostile: the hostility rule answers no
before StartCombat, grudges, the player's crimes and faction relations; StartCombat
does nothing on it; each settle of the awareness drops whatever alarms, targets or
calls for help it was given; and the engine fixes its heading as for a Turning Speed
of 0. [documented: GECK wiki Stats Tab for Creatures and AI Data; observed: in the
masters the placed fixtures are MrHouse and MrHouseOFFLINE (Mr. House's screens),
NVCRMrHouse (his life-support chamber, Turning Speed 30) and NVCRFestus; the turrets,
also Immobile and Unaggressive, carry their guns (tests/fixtures_data.rs);
hypothesis: the original lets such an actor take no part in a fight; owner ruling
2026-11-13 for the screens and the chamber.] Taking damage is left to the records
and scripts: the screens are Invulnerable, the chamber a ghost until its terminal
unseals it (NVCRMrHouseSCRIPT's SetGhost 1, the terminal items' SetGhost 0), after
which House's body can be killed as the owner ruled. Before, a screen became hostile
after House's death: the murder made it a crime enemy (`Game::crime_enemy` comes
before Aggression) or a script's StartCombat or a hit did [hypothesis: which one the
owner's game took is not in the report]. Observer check 181.

## 10. Checks

Unit test: `fnv_sim::leveled::rolls_none`. Smoke: `vats-no-melee-turret` (a melee mark on
a ceiling turret is refused), `blood-none-ant` (hits on an explosive Nellis ant leave no
spray and no decal), `ghost-tumbleweed`, plus the dialogue, creature talk (`creature-no-talk`,
`creature-names`) and VATS scenarios already there.
