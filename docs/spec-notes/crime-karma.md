# Spec note: karma, ownership and crime

Tags as in `nif.md`: `[documented]` cites a page or file, `[observed]` says what was
looked at, `[hypothesis]` is a reading nothing has confirmed yet, `[design]` marks what is
NVForever's own. Owner's requests (playtest list 2, 2026-10-12): karma for killing evil
and good actors and for crimes such as stealing and breaking into owned places; ownership
of cells and items so that stealing, trespassing and murder are hostile acts against the
owners and the victims' allies.

Code: `crates/fnv-game/src/crime.rs` (the rules, the script functions), the records in
`fnv_game::data` (`cells`, `faction_flags`, `owner_ranks`), the hooks in
`Game::kill` (combat.rs), `Game::attacked_by` (provocation.rs), `Game::would_attack` and
`Game::stop_combat` (awareness.rs). Engine: `engine/nvf-play/src/crime.rs`, the prompt in
`activate.rs` and `reticle.rs`, taken items in `menu.rs`. Survey: `fnvtool actor karma`
(§8).

## 1. Sources

- **GECK wiki** (owner's export, `C:\fnv-private\geckwiki`): Category:Factions (the
  flags Evil and Track Crime, "Crime Gold Multiplier … Unused in Fallout"), Reference
  (ownership, "Faction Rank"), Container (owners of items in a container), Guard Package
  (warnings), Combat Tab (the topics Steal, Murder, GuardTrespass, Assault and their
  NoCrime forms), Reputation, and the functions RewardKarma, GetIsAlignment, IsOwner,
  SetOwnership, ClearOwnership, SetCellOwnership, SetCellPublicFlag, IsCellOwner,
  IsInMyOwnedCell, GetOwnerOfCell, IsTrespassing, GetTrespassWarningLevel,
  SendTrespassAlarm, SendAssaultAlarm, SendStealingAlarm (an NVSE plugin's),
  GetMinor/MajorCrimeCount, GetActorCrimePlayerEnemy, GetActorFactionPlayerEnemy,
  SetPCEnemyOfFaction, ClearFactionPlayerEnemyFlag, IgnoreCrime, IsActorAVictim,
  IsPCAMurderer, GetFactionFlags (an NVSE plugin's: the flag numbers), IsCrimeOrEnemy and
  HudBarSetSystemColor (NVSE plugins': the crime colour of the prompt). The page
  Category:Crime itself is not in the export.
- **GECK wiki, offline snapshot** (`E:\Games\geck-wiki`, 2026-10-28): Category:Crime
  (major and minor crimes, the faction's "Player is an Enemy" flag, crimes expiring after
  `iCoolDownTimerSetting` days, 3 by default), Complete List of GameSettings
  (`iCoolDownTimerSetting` 3), Category:Factions ("Hidden from PC … has no code
  associated with it in Fallout"; Track Crime), Reputation ("no actual impact on the
  combat reactions of actors").
- **The game's own help** [documented: FalloutNV.esm MESG `HelpCrime` 0x0007234D and
  `HelpKarma` 0x0006B250, read with `fnvtool esm decode`; paraphrased here]. HelpCrime:
  crimes are minor or major. Minor crimes make only the victim hostile: *theft* (the
  owner who sees it tries to take the item back, attacks or runs for help if the player
  escapes or goes on stealing) and *trespass* (the owner who sees the player open a
  locked door, container or computer attacks at once). Major crimes make the victim and
  the victim's friends hostile: *assault* (attacking without provocation, or fighting
  back against someone who attacks for a minor crime) and *murder* (witnesses who care
  about the victim turn hostile). Holstering is a yield. HelpKarma: karma starts
  neutral, goes up to Good and Very Good or down to Evil and Very Evil; killing a
  particularly saintly or terrible enemy changes it; most people go by reputation, not
  karma.
- **FalloutNV.esm** [observed 2026-10-12, `fnvtool actor karma --survey`, §8]: the
  settings and globals of §2, actor karma, faction flags, owned cells and references.
- **xEdit** through the schemas [documented: `schemas/fnv/world.json` CELL DATA flags,
  `common.json` XOWN/XRNK, `gameplay.json` FACT DATA and ACBS karma].
- **Fallout_default.ini** in the game folder [observed]: `iSystemColorHUDAlt*` 255, 67, 42.

## 2. Records and settings

| What | Where | Value in FalloutNV.esm |
|---|---|---|
| Karma | actor value 23; NPC_/CREA ACBS `karma` (f32) [documented: schema] | the player 0 |
| Alignment bounds | globals `KarmaVeryEvil`, `KarmaEvil`, `KarmaGood`, `KarmaVeryGood` | −750, −250, 250, 750 [observed] |
| Kill of an evil actor | GMST `fKarmaModKillingEvilActor` | 100 [observed] |
| Stealing | GMST `fKarmaModStealing` | −5 [observed] |
| Trespass warnings | GMST `iGuardWarnings` | 2 [observed] |
| Seconds between warnings | GMST `fAITrespassWarningTimer` | not in the plugin; 5 by the wiki [documented: Guard Package] |
| Karma limits | GMST `iKarmaMin`, `iKarmaMax` [documented: RewardKarma] | not in the plugin: no limit applied [observed] |
| Days a crime lasts | GMST `iCoolDownTimerSetting` [documented: Category:Crime] | not in the plugin [observed: `fnvtool esm decode --edid` finds no record]; 3 by the wiki [documented: Category:Crime; Complete List of GameSettings] |
| Messages | GMST `sKarmaMajorGained`, `sKarmaMinorGained` ("You've gained Karma!"), `sKarmaMajorLost`, `sKarmaMinorLost` ("You've lost Karma!") and their `…Image` icons (`glow_karma_saintly`, `_good`, `_evil`, `_bad`) | [observed] |
| Sounds | SOUN `UIKarmaUp` 0x0005B53F (`fx\ui\ui_karma_up.wav`), `UIKarmaDown` 0x0005B540 | [observed] |
| Crime dialogue | global `GenericCrimeKarmaDialogueChance` | 25 [observed]; not used |

- Only these karma and crime settings are in FalloutNV.esm (18 GMSTs name karma, crime,
  stealing, trespass, guards or pickpocketing) [observed]. Settings the executable keeps
  for itself (other kills, murder) cannot be read: its code is encrypted (charter).
- **Alignment** [documented: GetIsAlignment's names VeryGood, Good, Neutral, Evil,
  VeryEvil]: karma at or beyond a bound is that side [hypothesis: the globals' names; the
  Pip-Boy's karma line uses them too, pipboy.md]. Records by their own karma [observed]:
  NPC_ 30 Very Evil, 568 Evil, 2,979 Neutral, 229 Good, 10 Very Good; CREA 398 Evil, 1,168
  Neutral, 12 Good. Placed NPCs: 17 Very Evil (Cook-Cook, the Jackal and Viper bosses,
  Vulpes Inculta), 575 Evil, 2,579 Neutral, 205 Good, 10 Very Good (Sunny Smiles, Julie
  Farkas, Keely); placed creatures 540 Evil, 2,442 Neutral, 17 Good. An actor made from a
  template whose Traits it takes reads the template's karma (`Game::alignment`, actor value
  Karma) [hypothesis: which template flag carries Karma is not documented]: by their own
  records 15 / 236 / 3,026 / 99 / 10, the leveled Powder Gangers and NCRCF convicts
  (`LvlPowderGangerGunVar`, `LvlNCRCFPowderGangerMelee`, Primm's deputies) Neutral, so
  killing one counted as murder (the owner's playtest list 5 item 22); through their
  templates they are Evil, and the Nipton Jackal bosses Very Evil [observed: `fnvtool actor
  karma --survey`, 2026-10-16]. Violet (`1EFiendViolet`) −500, Evil; Doc Mitchell 500, Good; Chet and
  Easy Pete 0, Neutral.
- **GetIsAlignment's code** in conditions: 0 Good, 1 Neutral, 2 Evil, 3 Very Good, 4 Very
  Evil [hypothesis: xEdit's enumeration as remembered]. The three conditions in
  FalloutNV.esm fit it [observed]: code 3 on a respectful answer and code 4 on a
  contemptuous one of the same speaker, both run on the player.
- **Faction flags** [documented: xEdit FACT DATA; numbering GetFactionFlags]: 1 Hidden, 2
  Evil, 4 Special Combat, 256 Track Crime, 512 Allow Sell. 682 factions: 44 Evil, 188
  Track Crime, 11 both [observed]; many are Fallout 3's, left in the plugin.
- **Ownership** [documented: xEdit XOWN (a FACT or NPC_) and XRNK; GECK wiki Reference,
  "Faction Rank: Faction rank required to own the reference"]. References with their own
  owner [observed]: 2,711 items, 773 containers, 64 doors, 461 furniture, 293 activators;
  43 with a rank. Through their cell: 8,971 items, 2,077 containers, 721 doors.
- **Cells** [documented: xEdit CELL DATA "Public place" 0x20, XOWN, XRNK; GECK wiki
  GetFormFlag, form flag 17 "Off limits (Interior cell)"]: 158 owned interiors (128 by a
  faction, 30 by an NPC), 204 public interiors, 23 owned and not public (Primm's and North
  Vegas's houses, Novac's motel rooms …), 1 Off Limits [observed]. Goodsprings' general
  store, saloon, Doc Mitchell's house and the homes are public [observed]; the store is
  owned by Chet (`GSChet`) and the saloon by `0x00104C6C`.

## 3. Crimes

### 3.1 Theft

- Taking an item someone else owns: a placed item picked up, or an item taken out of a
  container in the container menu [documented: GECK wiki Container, "Stealing the object
  is considered an act of aggression against the NPC / the faction"].
- Who owns it (`Game::ownership`): the reference's own owner (as scripts left it:
  SetOwnership, ClearOwnership), else its cell's (SetCellOwnership, else the CELL's XOWN)
  [documented: IsOwner, "This only checks actual ownership on the reference not the
  inherited ownership from the cell"]. A container's items are its owner's; the items'
  own owners (COED) are not read [design]. A dead actor's inventory is nobody's
  [hypothesis]. A reference made at run time (a dropped item) inherits nothing [design].
- The player is an owner when the owner is the player's base (SetOwnership's default),
  or a faction the player is in at the rank asked for [documented: Reference, HasOwnership].
- Karma: `fKarmaModStealing` (−5) per item or stack taken [hypothesis: per take], unless
  the owner is an Evil faction or a faction that does not track crime [documented:
  Category:Factions, "Evil: … stealing from members of this faction will not incur Karma
  loss"; "Track Crime: … Karma modifiers are also awarded when a faction is flagged to
  track crime"], or an owning NPC of Evil alignment [hypothesis: the Reference page's
  "karma gain/loss is affected by the karma level of the owning actor"]. Seen or not: an
  unseen theft costs karma too [documented: SendStealingAlarm, "If the actor is not being
  detected by the owner of an item, it will result in a karma loss"].
- Seen by the owner (the owning NPC, or a member of the owning faction when it tracks
  crime [hypothesis: any rank]): that witness turns on the player and says **Steal**
  [documented: Combat Tab]. Others who see it do nothing (a minor crime). The game's
  owner first "tries to take it back"; NVForever goes straight to the fight [design: the
  take-back is not built]. In one captured theft the owner ran procedure 47
  (TAKE_BACK_ITEM) for 20 s before starting combat [observed:
  npc-behaviour-compare.md §15.5].

### 3.2 Opening an owned lock ("trespass" in the game's help)

Picking or forcing the lock of an owned door or container (the lockpicking menu's crime
hook, lockpicking.md §4): the owner's witnesses attack at once [documented: HelpCrime],
with no line of their own (their NormalToCombat line is said). Karma as for theft
[design: the owner asked for breaking in to cost karma]. Hacking an owned terminal is
the same crime [documented: HelpCrime, "computer"]: winning the hacking game sends the
same hook (`terminal.rs`), with the same karma and witnesses [the owner, 2026-10-22];
a terminal that opens at its menu (unlocked, hacked before, password held) is no crime.

### 3.3 Trespassing in a cell

- The player trespasses while in an owned cell that is not public and that the player
  does not own [documented: SetCellPublicFlag, "Cells which are public cannot have
  trespassing"; hypothesis: owned and not public is the whole rule; the Off Limits flag
  is not used, one cell has it]. IsTrespassing reads it.
- An owner's witness warns (GuardTrespass) and warns again after `fAITrespassWarningTimer`
  until it has warned `iGuardWarnings` times; the next time it attacks [documented: Guard
  Package, "warn … wait 5 seconds … either issue another warning (iGuardWarnings) or
  enter combat"; Combat Tab, GuardTrespass "when the player is trespassing"; hypothesis:
  the same count for owned cells]. GetTrespassWarningLevel reads the warnings. Leaving
  the cell clears them [design].
- SendTrespassAlarm turns the actor at once ("all the way to fully trespassed on one
  call") [documented: notes].
- The doors' "No Alarm" flag (REFR XTEL): "No trespass alarm will be sent when the
  player uses the door" [documented: GECK wiki Reference, Teleport]; 5 doors, Dead Money
  2, Lonesome Road 3 [observed]. As built (`Game::arrived_through`, for the player's own
  use of a door and a script's alike [design]): arriving through such a door, the player is not trespassing in the cell arrived in (no warnings, no attack,
  IsTrespassing 0) until leaving that cell; any other door ends it [hypothesis: the alarm
  is the owners' trespass reaction; owner-review.md]. Log: `crime: the player came
  through … a No Alarm door …`.

### 3.4 Murder

- The player kills a non-Evil NPC (not a creature) that was not attacking the player of
  its own accord [documented: IsPCAMurderer, "murdered a non-Evil NPC"; HelpCrime, assault
  is "without provocation"]. Self-defence: it was hostile by its records, StartCombat or
  SetPCEnemyOfFaction; a fight the player started (an assault grudge) or a crime of the
  player's is no defence [design].
- IsPCAMurderer becomes 1. Witnesses (they see the player) who care about the victim —
  an ally or friend of it, a member of one of its factions or of its base
  (`Game::reaction_to`) — turn on the player and say **Murder** [documented: HelpCrime,
  Combat Tab].
- A murder such a witness saw makes the player an enemy of the victim's crime-tracking
  factions [documented: SetPCEnemyOfFaction, "the enemy flag … when the player commits a
  major crime against a crime-tracking faction"; hypothesis: when a witness saw it]:
  every member attacks on sight (GetActorFactionPlayerEnemy), whatever its aggression
  and the faction reactions [documented: Category:Crime, either indicator "will override
  any other faction reaction"], until the crime expires (§3.7) or a script clears the
  flag (ClearFactionPlayerEnemyFlag, SetPCEnemyOfFaction 0).
- **Every crime-tracking faction, the armour faction too** (#2456, #2498) [observed:
  docs/handoff/observer-checks.md check 203, captures `karl_murder` and `karl_murder2`,
  2026-11-15]: in the original a Cottonwood Cove Legionary killed in front of Aurelius
  made the player an enemy of both VCaesarsLegionFaction and ArmorVCaesarsLegionFaction
  0x00134B6E, the Hidden armour faction that also holds the four NCR troopers of the
  Strip scene VStripSS3 and Karl at Red Rock (their members' factions are enemies of
  each other: a tag, not a side). Both flags read 0 again about 0.18 game hours later
  (§3.7). NVForever flags every crime-tracking faction of the victim, as the original
  does; the flag is short-lived (owner's ruling on #2498, owner-review item 65). The
  rule #2498 first had, never flagging a faction that is no side, is dropped: the
  original sets that flag.

### 3.5 Assault

The player's hit turning an actor on the player (combat-ai.md §10.1) is a major crime
against it (GetMajorCrimeCount) [documented: HelpCrime]. Its friends join through
assistance (§10.3); the faction enemy flag is **not** set for an assault [design: one
punch would make a whole town hostile everywhere; owner to decide]. SendAssaultAlarm
turns the named victim (or, with the player named, the first present member of the
faction given) on the player and makes it say Assault [documented]. With a faction given
and no member present and alive, nobody turns and no crime is counted: the calling actor
is no member of the faction, so it is never the stand-in [documented: the wiki's "it will
pick an NPC in the … faction"; observed 2026-11-25, observer check 241: in Techatticup
with all 17 members killed, nobody went into combat for 10 s and the second hostage's box
still opened]. With members alive the original turned every member within about 1,900
units on the player (9 of 17, within 1.5 s), not one [observed, same check]. Only the form with no faction falls back to the calling actor [design,
#2926]; a freed Techatticup hostage's `SendAssaultAlarm Player
CaesarsLegionTechMineFaction` turned the hostage itself before.

Techatticup (#2926): each NCR hostage (TechMineNCRHostage2REF 0x0012319C,
TechMineNCRHostage1REF 0x0012319B, SCPT 0x00123191) ends its "Untie him." branch with
`SendAssaultAlarm Player 0x001400F9` (CaesarsLegionTechMineFaction) [observed:
FalloutNV.esm, script decompile and fnvtool esm decode]. With the mine's raiders alive the
pick works: in a headless run of the untie path, 17 members of the faction are present and
alive and the victim is TecLegionaryRaider5REF (ACHR 0x0011F654, base 0x00123192)
[observed: our headless walkthrough techatticup-hostages-2926, its `assault alarm:` log
line; the reference from fnvtool esm decode]. The reporter's hostage turned because the
mine had been cleared first, the usual order: no member was alive and the old fallback
picked the caller [hypothesis, fits the reporter's log]. The walkthrough
techatticup-hostages-dead-2926 kills the faction's actors first and expects nobody to
turn.

### 3.6 Who sees, and what turning means

- A witness is an actor that has the player detected (band Detected, measured within the
  last second, `fnv_game::detection`), alive, conscious, not the player's teammate, not
  ignoring crime [documented: Steal "if they notice you stealing", HelpCrime "sees";
  IgnoreCrime; hypothesis: "sees" is the band Detected].
- An actor that turns becomes a **crime enemy** (GetActorCrimePlayerEnemy): it attacks the
  player whatever its aggression and whatever the faction reactions say [design, as an
  assaulted actor's grudge]. Unaggressive owners fight too; fleeing for help is combat
  AI's (not built here). StopCombat ends it [design, as for grudges], and so does the
  crime expiring (§3.7); a crime is not yieldable [design: the wiki's Yielding speaks of
  actors the player attacked].
- IgnoreCrime 1: the actor neither turns nor attacks for crimes [documented].
- GetAlarmed reads 1 for a crime enemy [hypothesis: the wiki says only "alarmed"].
  IsActorAVictim is the latest crime's victim [documented: "one NPC at a time"].
  GetMinorCrimeCount and GetMajorCrimeCount count per actor; a theft nobody saw counts
  against the owning base or faction [design].

### 3.7 Crimes expire (#2467)

- **The rule** [documented: Category:Crime]: the faction's "Player is an Enemy" flag "is
  updated via the crime list, so it is cleared when all the crimes causing it to be set
  expire"; a crime expires after `iCoolDownTimerSetting` days (3 by default; the wiki's
  examples: a theft victim stops attacking three days after the theft, the BOS three days
  after the last murder). The setting is read from the load order, 3 when no plugin sets
  it (FalloutNV.esm does not) [documented: Complete List of GameSettings; observed].
- **The faction flag is short** [observed: observer check 203, capture `karl_murder2`,
  2026-11-15]: after the Cove murder both Legion flags held while the player moved to
  the Searchlight airport, then read 0 there about 0.18 game hours (11 game minutes)
  after the murder, with no cell change and no script clearing them (FalloutNV.esm's
  scripts never clear those flags [observed: script text]). Aurelius, the witness, was
  still in combat with the player after that: his own grudge outlasts the flag. What
  clears the flag (a timer, the crime list's update, the witnesses' reports) is not
  known; that the same 0.18 hours holds for every crime's flag (a theft's, the Evil
  faction's area alert, a murder with more witnesses) is a [hypothesis]: only that one
  murder's was timed.
- **What NVForever keeps** [design: the wiki's crime list reduced to what it decides]:
  for each faction a crime made the player an enemy of (a seen murder, §3.4, also one
  seen only by witnesses already hostile; the Evil faction's area alert, combat-ai.md),
  the game day (GameDaysPassed) of the last such crime; for each crime enemy (§3.6: theft, opened lock, trespass, murder witnessed,
  SendTrespassAlarm, SendAssaultAlarm), the day of the last crime that made it one. A
  later crime restarts the count. The faction flag is cleared 0.18 game hours after the
  last crime that set it (`crime::FACTION_FLAG_DAYS`, owner's ruling on #2498: "Short-lived
  flag"); a crime enemy forgives the player when the count reaches
  `iCoolDownTimerSetting` days (`Game::expire_crimes`).
- **A script's flag stays** [design]: SetPCEnemyOfFaction 1 sets a flag no crime list
  holds; it stays until a script clears it (SetPCEnemyOfFaction 0,
  ClearFactionPlayerEnemyFlag, ClearActorsFactionsPlayerEnemyFlag), even when a crime set
  the same flag too. Whether the original's crime-list update also clears a script's
  flag is not known [hypothesis: scripts use it for lasting hostility].
- **When it is read** [design]: against GameDaysPassed whenever hostility is asked afresh
  (a detection round's start, a combat AI frame's, the crime tick, the Wait menu's hourly
  check) and by the script functions that read the flags (GetPCEnemyOfFaction,
  GetActorFactionPlayerEnemy, GetActorCrimePlayerEnemy, GetAlarmed). So time passing by
  any route counts: play, waiting, sleeping, fast travel, a script setting the clock.
  Moving the clock back ends nothing.
- **Saves** [design]: the dates are saved with the player. A save from before #2467 has
  none: at the load its crime enemies (actors) are dated at the load, so they end three
  days after it; its faction flags are kept as a script's and never end by the
  cooldown when a script of the load order can set them, because such a save cannot
  tell a crime's flag from a quest's SetPCEnemyOfFaction, and a quest's hostility must
  not end. A flag on a faction no script names to SetPCEnemyOfFaction
  (`Game::script_flaggable_factions`: FalloutNV.esm's 19 calls name 8 factions
  [observed: `fnvtool script decompile`, 2026-11-25]) can only be a crime's: it is dated
  at the load and ends 0.18 game hours later (#3335). A save made between #2467 and
  #3335 holds such flags among the script's; at the load they end at once, the crime
  behind them being older than the save that first kept them (#3335: the player report
  R-833D3F5X held ArmorVCaesarsLegionFaction so, and Karl fought Veronica in the Great
  Khan longhouse; with it ended he holsters and sits). Log: `load: the player's flag with
  faction <f> was an older save's crime, kept as a script's, and no script sets it: it
  ends (#3335)`.
- **Log**: `crime: the player is no longer an enemy of <faction>: the last crime against
  it is <h> game hours old`, `crime: <actor> no longer counts the player's crimes: the last
  is <d> game days old`.
- **Why it mattered** (#2467) [observed: `fnvtool esm decode`, FalloutNV.esm; the cause a
  hypothesis, the owner's run left no log]: Karl (`VRRCKarl`, the Legion's envoy at Red
  Rock) is in `ArmorVCaesarsLegionFaction` (Track Crime, the faction the Legion armour's
  disguise allies with the player), not in `VCaesarsLegionFaction`. Of the 242 NPC_
  bases in either faction (factions taken through Use Factions templates), 241 are in the
  armour faction; 52 of those are also in an Evil faction (VCaesarsLegionSnifferFaction
  33, VHooverDamCaesarsLegionFaction 13, CaesarsLegionTechMineFaction 4, two others), 189
  are in none [observed: a scan of FalloutNV.esm's NPC_ and FACT records, 2026-11-14].
  Killing one of the 189 unprovoked while a fellow Legionary sees the player is a murder
  (§3.4) that flags the victim's crime-tracking factions, the armour faction among them;
  without expiry Karl attacked on sight for good, and with the three days' expiry for
  three days. In the original the flag is gone within minutes, and Karl, entering his
  longhouse after it, stays seated; with the armour flag up in front of him he attacks at
  once [observed: observer check 203]. Killing one of the 52 takes the Evil
  faction's area alert (`crate::provocation`), which flags that Evil faction only, and
  the kill then counts as self-defence (the victim is a member of a faction the player
  is now an enemy of), so it never flags the armour faction [observed: smoke run of
  2026-11-14, NVLegionaryAlexusREF killed: only CaesarsLegionTechMineFaction flagged, no
  murder].
- **Open**: the minor crimes' timer. An AI observer check saw Chet calm one day after a
  theft (npc-behaviour-compare.md §16.9, GetActorCrimePlayerEnemy 0 after 1 day and after
  4), where the wiki has the victim attack for three; NVForever uses the three days for
  every crime until a check times it [hypothesis]. Whether a murder of a member while the
  faction's flag is up is a new crime (the wiki's BOS example) or self-defence (§3.4, as
  NVForever has it) is open too.

## 4. Karma

- Every change: the corner message with the game's string and icon, minor or major by
  its size (major from `fKarmaModKillingEvilActor`, 100, up) [hypothesis: the split], and
  `UIKarmaUp` or `UIKarmaDown` [hypothesis: by their names]; RewardKarma does the same
  [documented: "A corner message is shown in game indicating the change"].
- Killing [documented for evil: the setting's name]: an Evil NPC
  +`fKarmaModKillingEvilActor` (+100) [observed: §4.1, capture o6_02], a Very Evil one
  the same [hypothesis: the setting's name; not observed]; a Very Good one −100
  [observed: §4.1], a Good one −100 [hypothesis: the same setting]; a Neutral NPC
  murdered (§3.4) −25, a quarter [design: the owner asked that killing innocents cost
  karma; HelpKarma says only saintly or terrible enemies count]; a Neutral actor in a
  fight it started 0. The alignment the karma reads is the **placed base's own ACBS
  karma** (a levelled base as drawn for the reference), not a template's whose traits it
  takes (`Game::kill_alignment`) [hypothesis, §4.1: the NCRCF convict]; whether it was
  a murder is judged by the alignment scripts read (GetIsAlignment, the template's)
  [design]. Only kills by the player count; the share of damage (kill XP's rule) is not
  asked [design]. Creatures give none [design: their karma values include plain
  animals, a dead radroach placed in FalloutNV.esm is Evil, observed].
- Stealing and opening owned locks: §3.1, §3.2.

### 4.1 Killing an Evil convict in the original (2026-10-19) [observed]

The owner's playtest list 5 item 22 (karma lost for killing a convict outside the NCR
highway patrol station; "should be good"). AI observer campaigns `functions_round` and
`player_kills` (save `nvftest`, the player at karma 0, god mode):

- A spawned **LvlNCRCFPowderGangerMelee** reads GetAV Karma **−500** and GetIsAlignment
  Evil 1 (VeryEvil 0, Neutral 0): its template's karma, as `Game::alignment` has it. It
  attacked the player at once, set on the player or not.
- **Shot dead by the player** (a plasma rifle aimed by a script, outdoors in Goodsprings):
  OnDeath named the player as the killer, no OnMurder, and the player's GetAV Karma read
  **0 before and for the 15 s after**: no karma either way.
- Killed by `KillActor player` (twice): OnDeath with the player as killer, no karma.
- NVForever took karma for it before item 22's fix (it read the convict as Neutral),
  gave +100 after it (Evil), then none for any Evil NPC (2026-10-19 to 2026-10-31).
  Since #1131 an Evil kill gives +100 again, judged by the placed base's own karma, so
  the convict (0 of its own) gives none.
- **2026-10-31 (capture o6_02, nvfairport, karma 0, god mode)**: copies placed with
  PlaceAtMe and killed at once by `KillActor player`: NorthVegasFiendCF (GetAV Karma −500)
  **+100**; a copy of GSSunnySmiles (900) **−100**; VESNCRTrooperCMa (0) **0**;
  GoodspringsPowderGangerHM (−500) **+100** [observed]. So an Evil-range kill can give
  +100, unlike the convict above. The difference found: both +100 victims hold −500 in
  their **own** ACBS (an NPC_ template besides), the convict 0 in its own and −500 only
  from its levelled-list template (VarNCRCFPowderGangerMelee) [hypothesis: the kill's
  karma reads the placed base's own value, as the extender's base getters ignore
  levelled templates, actors.md §2.2]. The Neutral trooper's 0 is a KillActor kill
  without a crime, so the "murder −25" rule is still unobserved.
- Limits: iKarmaMin/iKarmaMax when a plugin defines them [documented]; FalloutNV.esm does
  not.

## 5. Bounties and reputation

- There are no bounties: "Crime Gold Multiplier … Unused in Fallout" [documented:
  Category:Factions]; none of the Oblivion crime-gold functions are in the catalogue
  [observed: `schemas/script/functions.json`].
- New Vegas tracks **reputation** (REPU: fame and infamy per reputation, ranks Accepted …
  Vilified) [documented: GECK wiki Reputation]; it "has no actual impact on the combat
  reactions of actors". The reputation functions are built (`functions/reputation.rs`).
  Scripts change reputation, and a seen theft or kill adds infamy with the victim's
  reputations: 2 and 30 points [documented: Fallout wiki "Reputation (Fallout: New
  Vegas)"; hypothesis: that "caught" and "openly" mean seen, and that the points are
  infamy]. A faction owner, an NPC owner's base and a killed reference each find their
  reputations differently (reputation.md §3).

## 6. In play (`nvf-play` `crime.rs`)

- Theft: an item reference that leaves the world as the player picks it up, and items
  taken in the container menu; the lock hook; trespass checked four times a second.
- The crosshair and its prompt are drawn in the crime colour over an owned item,
  container or lock (a terminal still to hack among them), and an owned item's action
  reads **Steal** [documented: IsCrimeOrEnemy,
  "red color of the crosshair or the activation prompt"; HudBarSetSystemColor, the
  "Crime" system colour of the prompt; hypothesis: the colour is the HUD's alternative
  one, 255, 67, 42 (Fallout_default.ini), and the word "Steal"].
- Log: `crime: …` per crime, warning, faction made enemy and karma change; `crime
  summary: …` at exit.
- The Pip-Boy's STATS General page already shows the karma (pipboy.md).

## 7. Script functions

Built (crime.rs): RewardKarma, GetIsAlignment, IsPCAMurderer, GetMinorCrimeCount,
GetMajorCrimeCount, GetActorCrimePlayerEnemy, GetAlarmed, IsActorAVictim, IsCellOwner,
IsInMyOwnedCell, IsTrespassing, GetTrespassWarningLevel, SendTrespassAlarm,
SendAssaultAlarm. Already built before: SetOwnership, ClearOwnership, IsOwner,
SetCellOwnership, SetCellPublicFlag, SetPCEnemyOfFaction, GetPCEnemyOfFaction,
ClearFactionPlayerEnemyFlag, GetActorFactionPlayerEnemy, IgnoreCrime, GetIgnoreCrime.
Without an opcode in the catalogue (not callable): GetOwner, GetParentCellOwner,
GetOwnerOfCell, HasOwnership, SetIsPCAMurderer, ClearActorsFactionsPlayerEnemyFlag.

## 8. Checks

- `fnvtool actor karma --schema schemas/fnv <Data> FalloutNV.esm --survey --actor
  FiendVioletRef --actor SunnyRef --actor GSChetRef --cell GSGeneralStore`
  (`fnv.actor.karma/1`): the settings of §2, the rules' numbers, per actor its karma,
  alignment and what a kill does, per cell its owner and owned references, and the survey
  counts of §2. Smoke scenario `karma-survey`.
- Unit tests: `crates/fnv-game/tests/crime.rs` (records, ownership through the cell and
  ranks, unseen and seen theft, kills and murder with the faction flag, trespass
  warnings, RewardKarma; crimes expiring, §3.7: after three days of waiting, a script's
  clock jump, the Wait menu's check, a script's flag kept, a later crime restarting the
  count, an older save dated at the load).
- Smoke scenarios `crime-karl-flag-clears` and `crime-expiry-legion-calm` (§3.4, §3.7,
  #2467, #2456, #2498): a Cottonwood Cove Legionary in no Evil faction murdered while
  another, at its post, detects the player, then the longhouse at Red Rock. The murder
  flags VCaesarsLegionFaction and the armour faction; half a game hour later both flags
  are cleared, and Karl does not fight; after a clock jump of four days the witnesses
  forgive too.
- In play, smoke scenarios `crime-steal` (a carton of cigarettes taken in front of Chet in
  his store: "crime: theft against GSChet … turned on the player: [GSChetRef …]", the
  karma line and his fight) and `karma-fiend` (Violet shot dead: "crime: karma +100 for
  killing 1EFiendViolet (Evil)").

## 9. Not built

Taking a stolen item back, running for help; pickpocketing; items' own owners in
containers (COED); hacking an owned terminal as a crime; alarms on entering
through a door (beyond the No Alarm excuse, §3.3); the Off Limits flag; guard packages' warnings;
"NoCrime" topics; crimes by actors other than the player; stolen items marked in the
inventory; the karma title; saving the trespass warnings.
