# Spec notes: actors placed dead

Clean-room description in plain language. Tags: **[documented]** cites a public source,
**[observed]** was read from the owner's masters (FNV 1.4.0.525) with `fnvtool`,
**[hypothesis]** needs verification, **[design]** is our own choice.

Code: `fnv_game::Game::starts_dead` and `Game::lay_out_corpses` (`crates/fnv-game/src/combat.rs`,
called by `Game::new_game`), `ActorDef::zero_health` (`data.rs`). In the game the dead
flag does the rest: `engine/nvforever/src/deaths.rs` shows an actor that is dead when it
is drawn at the end of a death pose and hands it to its ragdoll (`ragdoll.rs`,
docs/spec-notes/ragdoll.md), the AI leaves dead actors alone (`fnv_ai`), and activating a
dead actor searches it (`nvf_play::activate`).

## 1. The rule

- "If Base Health is set to 0 the actor will spawn as a corpse, and can additionally be
  ragdolled with the GECK Havok Sim button" [documented: GECK wiki, Stats Tab - NPC, Notes;
  the same note on Category:NPC adds: "However the 'Respawns' flag must not be ticked or the
  actor will respawn alive after 3 days with 0 HP"].
- Base Health is a field of the NPC's Stats tab ("Health is calculated with Endurance and
  level. This value is then added to that result") [documented: Stats Tab - NPC]. Each
  template checkbox stands for a tab whose data then come from the template [documented:
  Category:NPC, Template Data], so with the template flag "Stats" the Base Health read is
  the template's. NVForever reads it from the actor that supplies the stats (the same chain
  `Game::max_health` follows).
- The field is NPC_ `DATA` Base Health [documented: xEdit field name; schema
  `gameplay.json`]. A record without a `DATA` is not taken for a corpse [design].
- **Creatures** [hypothesis]: CREA `DATA` Health stands where an NPC's Base Health does
  (a creature's health is that value, `Game::max_health`); a placed creature whose Health is
  0 starts dead too. The masters' creatures named as dead have Health 0 (§2).
- **The placed reference** carries no flag for it [observed: the record flags of the
  tribal village's corpses are 0; the other placed corpses have 0, 0x400 Persistent or 0xC00
  Persistent + Initially Disabled]. GetDeadCount does not count these corpses and they have
  no killer and run no OnDeath [hypothesis: nobody killed them].

## 2. What the masters hold [observed, FalloutNV.esm]

- `TCFiend01DEAD` (NPC_ 0x00157384, "Fiend"): ACBS flags Respawn, Auto-calc stats, No Low
  Level Processing; template flags Factions, Actor Effect List, AI Packages, Base Data and
  Script from TPLT 0x000F1C44 (not Stats); DATA Base Health 0; two packages, inventory and a
  death item.
- The tribal village (`GNDeadTribalVillage`, WastelandNV cell -19,7, about (-74300, 30300,
  7810)) places `TCFiend01DEAD`, `TCFiend02DEAD` and `TCFiend03DEAD` seven times and the
  burned bodies `TCBurnedBody01DEAD`/`02DEAD` round it; two `TCGreatKhan0?DEAD` lie in the
  persistent cell nearby.
- Other corpses follow the same rule: `DeadGhoul2` (Base Health 0, its own stats),
  `VForlornHopeNCRTrooperDeadAM` (stats from the template
  `VForlornHopeNCRTrooperDeadTEMPLATE`, Base Health 0), `RangerMoralesDEAD`; creatures
  `DeadMoleRat`, `DeadBrahmin`, `NightkinMeleeDEAD`, `QJDeathclawDead` have CREA Health 0.
- A new game lays out 889 placed actors dead with the ten masters (the
  log line "new game: … placed actors start dead").
- The tribal village's corpse references carry `XRGD` "Ragdoll Bones" (a byte, a position
  and three angles per entry) [observed: 0x001578F7 decoded]: the pose the designer left
  them in (§5).

## 3. In NVForever

- `Game::new_game` marks every placed actor that starts dead as dead (`RefExtra::dead`)
  before the start-game quests run. Saves keep it like any death.
- Drawn dead: with an `XRGD` that fits its skeleton, in that pose, at rest (§5); else the
  end of a death pose, then the ragdoll drops the body and it comes to rest (ragdoll.md
  §4); with the ground not loaded the death pose holds until it is. A corpse drawn at a
  reference placed under the ground (the leveled spawns' invisible `nvvoid` placeholders
  laid out dead, 3 and 360 units under it) starts its ragdoll lifted over the terrain, or
  it fell through it; a stored pose (`XRGD`) only when it lies more than 20 units under,
  and then it starts awake (physics.md §4.3, #2753).
- Corpses whose base respawns come back alive after three days away (§4).

## 4. Cell resets: Respawn [2026-10-12]

Code: `crates/fnv-game/src/respawn.rs` (from `Game::set_loaded`),
`engine/nvforever/src/cell_resets.rs`; tests `crates/fnv-game/tests/respawn.rs`; scenario
`corpse-respawn`.

- The flag: "Respawn — References of this NPC in the world will be recreated after three
  days" [documented: GECK wiki Category:NPC, Configuration]. On a corpse: "the 'Respawns'
  flag must not be ticked or the actor will respawn alive after 3 days with 0 HP"
  [documented: same page, Notes]. ACBS flag bit 3 [documented: xEdit; schema
  `gameplay.json`]. With the template flag "Base Data" it comes from the template: "Base
  Data does not include the Quest Item, No Knockdowns, No VATS Melee, Is CharGen Face
  Preset, or Can Be All Races flags" [documented: Category:NPC, Template Data], so it
  includes Respawn. `TCFiend01DEAD` and its template `Fiend1MeleeAAFTEMPLATENV` both have
  it [observed].
- When: a cell "has expired and going to be reset on the next load (happens 3 days after
  the last visit)" [documented: GECK wiki IsCellExpired]; the delay is the game setting
  `iHoursToRespawnCell`, 72 hours by default [documented: GECK wiki SetZoneRespawns;
  observed in game: 72.0, and no master sets it, `fnv_sim::world::DEFAULT_RESPAWN_HOURS`];
  "The cell has to be loaded in order to get cleaned up" [documented: ResetInteriorAlt].
  The time-based reset resurrects actors, the scripted ResetInterior does not
  [documented: ResetInterior, Notes].
- In NVForever: the game's loaded cells (`Game::loaded_cells`: the player's cell, and the
  5 × 5 around it outdoors) are the visit. A cell leaving that set starts its clock
  (GameDaysPassed, `fnv_sim::World::detach_cell`, kept in saves); coming back into it
  after at least `iHoursToRespawnCell` hours, the cell is reset before its OnLoad blocks
  run: each dead actor reference placed in it (its authored cell) whose base respawns is
  recreated: alive, at its authored place and facing, its run-time state dropped (actor
  values and damage, killer, factions and packages set by scripts), its inventory rolled
  again [hypothesis: "recreated" read as starting again as authored]. Respawning
  containers in the cell get their contents back (`World::attach_cell`, simulation.md §3;
  until now nothing in the engine called it). The engine takes the corpse away (death
  pose, ragdoll, the remembered resting pose) and the residents start the actor again
  from its authored place.
- The wiki's "with 0 HP" is not copied: the actor comes back with its full health
  [design: an actor alive at 0 health has no meaning here].
- In the masters the village's seven Fiends and the burned bodies around it have the flag
  and come back, and so do killed creatures of leveled spawns (`VSpawnTier3CazadorSmall50`
  …) [observed: `corpse-respawn` run, 2026-10-12].
- A recreated actor runs its OnLoad with the rest of the cell (`Game::set_loaded`):
  after the reset, the cell's OnReset blocks and the outfitting of its actors, before
  the cell's next frame and before the engine draws the reset. A script that kills it
  there (the placeholders of §4.1) leaves it dead again before any per-frame block, AI
  or combat step runs [design: the order of `set_loaded`]. A recreated actor whose new
  pick runs another script (template flag "Script", a mixed list; compared with the
  script it ran before the reset, a SetScript change included) starts that script's
  instance afresh; a ScriptWait is dropped unless it is of the script it runs now
  [design].

### 4.1 The leveled spawns' placeholder creature [2026-11-21, #3228; #3139]

What the records hold [observed: `fnvtool esm decode`, FalloutNV.esm]:

- The leveled creature lists behind the `VSpawnTier…` bases mix a real creature with
  `VCrILikeOblivionChancedSpawns` (CREA 0x00157B25) and set no chance none (LVLD 0):
  `VEncTier1BigHornerSmall75` (0x0015C6CD) holds three entries of the bighorner calf
  (0x0014F47E) and one of the placeholder, all at level 1; `VEncTier1BloatFlyMed50`
  (0x0015C6C9) four bloatflies and three placeholders. The number in the Editor ID is
  about the creature's share of the entries. The spawn bases (`VSpawnTier1BigHornerSmall75`
  0x0015C648 …) take Traits, Stats, Factions, Actor Effect List, AI Data,
  Model/Animation, Base Data, Inventory and Script from the list (template flags 991).
- The placeholder: model `creatures\NVVoid\skeleton.nif` + `NVVoid.NIF` (no shapes,
  #2753), base scale 0.1, Health 0, Damage 0, ACBS flags Respawn, Walks, No Low Level
  Processing, No Blood Spray/Decal, Can't Open Doors and Is Ghost, Aggression
  Unaggressive, Assistance Helps Nobody, Sound Level Silent, one faction
  `VCrVoidCritterFaction` (FULL "Faction for Void Creatures Used in Chanced Spawns",
  flagged Hidden from PC) and the script `OblivionSpawnTestScript` (0x00159603), whose
  only block with a statement is `Begin OnLoad`: `Kill`, `SetDestroyed 1`, `Set bDead to
  1` (a `MoveTo VVoidCreatureMarker01` and a `ResetCell VDoNotDelete` beside them are
  commented out; the cell `VDONOTDELETE`, "Cell for Dead \"Void Creatures\"", and its
  markers exist).
- A leveled creature is used only as a template, and its dialog offers the calculate
  flags, level and count, no chance none [documented: GECK wiki LeveledCreature, Leveled
  Character Dialog].
- So the placeholder is the list's "nothing" [hypothesis, from all of the above: a share
  of no creature in a list that cannot give none, "chanced spawns" in the faction's name
  and the Oblivion-style chance in the creature's]: whenever it is the pick, its OnLoad
  kills it as the cell loads; it is a corpse of no shapes, Health 0 also laying it out dead
  at a new game (§1), and its Respawn flag brings it back alive at a cell reset (§4) only
  for the same OnLoad to kill it again.
- **In NVForever** (`GameData::script_through_template`, scripts.md §7.1): a placed
  actor whose base has no script of its own but may run one through its template chain
  (a list's pick, a list placed directly, or an actor on the way kept when its list
  yields no entry) gets the lifecycle blocks OnLoad and OnReset, so the placeholder's
  OnLoad runs at each load of its cell, the reset one included. Before, only references
  whose own base has `SCRI` got them: the placeholder's OnLoad never ran, a reset left
  it alive at health 1, and as a herd mate (the same base) it heard its herd's alarms,
  ran OnStartCombat, hit the player for 0.3 and kept the detection meter at Caution,
  unseen and, a ghost, unhurt (#3228). Its corpse is drawn and ragdolled as before (§3,
  physics.md §4.3).
- **A loaded save** keeps its cells loaded, so no cell load runs OnLoad: after a restore
  the living actors now in a loaded cell (where they were placed, or moved to) that are
  scripted only through their template and whose script has an OnLoad block run it at
  the first frame (`Game::queue_template_onloads`, `Game::restored_onload`), each
  checked again then (present, in a loaded cell, alive) and skipped if a cell load ran
  its OnLoad first. A save from an earlier build that holds a
  placeholder alive so has it dead and destroyed at once [design; hypothesis: the
  original runs OnLoad when a reference's 3D loads, a loaded save's too]. References
  scripted by their own base get no OnLoad on a load, as before.
- **Per-frame blocks** [design, #3228]: the template index schedules OnLoad and
  OnReset. Moved actors keep their usual GameMode, MenuMode and eligible far-frame
  runs (`Game::active_refs`, `Game::run_far_frame`). The blocks sent to one reference
  (OnDeath, OnStartCombat, OnHit …) already read the template script (`Game::ref_script`).
- How far the rule reaches [observed: data test
  `spawn_placeholders::the_camp_golf_herd_runs_its_placeholder_script`, FalloutNV.esm
  alone]: 1,359 references are indexed through a template only (7,597 by their own
  base), 1,157 of them whose chain may give `OblivionSpawnTestScript`; the rest may give
  17 other scripts, six of them with an OnLoad block (`VHDLegionaryScript` 32
  references, `VBSRefugeeSCRIPT` 11, `CrNightkinSCRIPT` 4, `SlaveScript` 2,
  `VVault21FluffNPCSelf01` 1) and three with a GameMode block
  (`vGlowingOneSCRIPT` 81, `CrNightkinSCRIPT` 4, `GunRunnerGuardScript` 3). At Camp Golf
  the herd's spawns 0x00168D45, 47, 4C, 4D, 4F and 50 pick the placeholder (the pick is
  seeded by the reference and every entry is level 1, so a new game and a reset pick the
  same).
- #3139's attached log does not show a placeholder at work: its Caution came from two
  ravens (`VCrTier0Raven` 0x0016177A and 0x0016177D, in the same faction, Unaggressive)
  in a combat state carried over in the save ("Lost → Combat"), a separate cause not
  fixed here [observed: the issue's log]. The placeholder OnLoads in it ran on
  references already dead. The name the player gives ("vControlOblivionChanceSpawns") is
  most likely the placeholder's Editor ID [hypothesis].
- What a ghost does is unchanged: nobody attacks or detects it as an enemy, and it may
  still attack [documented: GECK wiki SetGhost notes, "Enemies of this calling ref will
  not go into combat or detect an actor with this flag", "*Can* inflict damage";
  `Game::would_attack`]. Not checked against the original: that it runs this OnLoad for a
  reference recreated by a reset, and the frame the corpse is back.
- An encounter zone with "Never Resets" keeps its cells from resetting (2026-10-21,
  [encounter-zones](encounter-zones.md) §4).
- Not done [documented, not built]: the bodies of
  killed actors that do not respawn being removed at the reset ("Quest Item: Dead body
  isn't removed when the cell regenerates", Category:NPC); ResetInterior, IsCellExpired.

## 5. The pose the designer left a corpse in (`XRGD`) [2026-10-12]

Code: `engine/nvforever/src/authored_pose.rs`, used by `deaths.rs`; ragdoll.md §4 for the
resting pose it feeds. Surveyed with a private script over FalloutNV.esm (not committed).

- `XRGD` "Ragdoll Bones" on placed references: entries of 28 bytes: a byte, three bytes of
  padding, a position and three angles [documented: xEdit wbRagdoll; the schema calls the
  byte the bone id]. `XRGB` "Ragdoll Biped Rotation", a vector [documented: same]. The
  GECK's Havok Sim button ragdolls a corpse with Base Health 0 [documented: Category:NPC,
  Notes]; `XRGD` is where the resulting pose is kept [hypothesis, supported by what
  follows].
- In FalloutNV.esm: 278 ACHR carry `XRGD`, every one with 18 entries; 109 ACRE with 10 to
  37; about 1,570 REFR (clutter skeletons) with 1 to 21 [observed].
- **Which entry is which bone** [observed]: the human skeleton
  (`characters\_male\skeleton.nif`) has 18 ragdoll bodies (ragdoll.md §1); entry k is the
  k-th body in the file's order (`Bip01 NonAccum`, the left leg, the right leg, three spine
  bones, `Neck1`, `Head`, the left arm, the right arm). The byte called bone id does not
  give that order: it runs 2, 8, 9, 10, 14, 15, 16, 2, 3, 4, 0, 1, 5, 6, 7, 11, 12, 13 on all
  278 (the first and the eighth both 2), so it is not used.
- **Entries 1 to 17** are their bone's transform under its parent node in the skeleton
  (the node, not the parent body): the positions are the bind pose's local translations to
  within a unit or so (thigh to calf 31.94 against 31.97; spine to spine1 7.48 against
  7.48; neck to neck1 0.63 against 0.65, `Neck1` being under `Bip01 Neck`, which has no
  body) [observed]. The angles use the convention of a reference's rotation (world.md §4,
  `fnv_world::transform::ref_rotation`: `M = Rx(−x) Ry(−y) Rz(−z)`) [observed, against the
  other orders and signs]: under it the calves (hinge joints) turn about one axis only
  (0.5° off it, median of 556), and laid out from their references the bodies lie flat:
  the bone heights of a body spread 24 units (median), against 39 to 47 under the other
  orders and signs.
- **Entry 0** is the root body, `Bip01 NonAccum`: its position is (0, 0, 0) within 0.05
  units on 276 of the 278 [observed: 0x0015A878 and 0x0016A9AE have about 66 units, not
  looked into]: the reference's position is where the pelvis lies. Its rotation is taken relative to the reference's rotation
  [hypothesis]: so read, 185 of the 278 bodies have their head behind where the reference
  faces (fallen backwards); read as a world rotation the heads point every way, and read
  under `Bip01`'s rotation (`XRGB`) they nearly all point to one side [observed].
- `XRGB` is (0, 0, −π/2) on all 291 references that have it: in the same convention, the
  bind rotation of `Bip01` (90° about z) [observed]. Not used.
- **In NVForever** [design]: a corpse laid out by a new game (`Game::starts_dead`, no
  killer) whose reference has an `XRGD` with one entry per body of its skeleton's ragdoll
  gets its bones' world poses from it (the reference's position, rotation and drawn
  scale; nodes without a body keep their bind pose) as its resting pose: the ragdoll goes
  into the world in that pose, at rest, and does not fall. A corpse killed in the game
  falls as before. In the village 16 corpses lay in their stored pose and none was lifted
  out of the ground [observed: `dead-on-spawn` run, 2026-10-12]; before, they fell from the
  death pose.
- Not checked against the original: whether its bodies show exactly this pose (no capture
  taken). `REFR` ragdolls (clutter skeletons): §5.1.
- **Creature corpses** (#3408): the reading above does not hold for every skeleton
  [observed 2026-11-25, the brahmin, `creatures\brahmin\skeleton.nif`, 31 bodies, root body
  `Bip01 Spine0`, corpses 0x00094BCF in FalloutNV.esm and 0x0601D3CA in TTW's
  Fallout3.esm]: entries 1 to 30 match their bone's bind translation under its parent node
  within 0.2 units, except the two thighs (stored (0, ∓9.2, 0) against a bind (0, 0, ±9.3)
  under `Bip01 Pelvis`, a body-less node, as if turned 90° about x) and `Bip01 Tail1`
  (under `Bip01 Tail`, also body-less). So read, the Arefu dead brahmin (TTW) hung in the
  air upside down and the Mojave's started up to 51 units under the ground. What the
  entries of a bone under a body-less node mean is not worked out [hypothesis: another
  frame than the parent node's]. **In NVForever** [design, #3408]: a stored pose is held
  at rest only on a skeleton whose root body is `Bip01 NonAccum` (the human one, where
  the reading was checked: `authored_pose::CHECKED_ROOT`); on any other the corpse starts
  awake in that pose and settles under physics, as one lifted out of the ground already
  did. The three Arefu brahmin then lie on their sides on the ground (trunk 22 to 24 units
  over the lowest bone) [observed: same day].

### 5.1 Clutter skeletons (`REFR` with `XRGD`) [2026-10-16, playtest list 5 item 71]

Code: `engine/nvf-play/src/clutter_pose.rs`. The Goodsprings cave's three clutter
skeletons (`SLGoodspringsCaveINT`: 0x00153170, 0x00153171 `Skeleton50sCasualFemale`,
0x001531F5 `Skeleton50sCasualMale`, MSTT) stood upright with the arms out, where the
owner's screenshot of the original shows one lying on the floor.

- Their models (`meshes\clutter\skeleton\skeletonfemale50scasual.nif`,
  `skeletonmale50scasual.nif`) are not skinned: 17 body-part nodes, each with a
  collision object and its shapes, in five unjoined groups (a left arm, the pelvis with
  the right leg, the left leg, a loose skull, the torso with the head, right arm and a
  skirt piece), 12 constraints within the groups [observed: `fnvtool nif decode`].
- Their `XRGD` has 17 entries, entry k for the k-th collision object in block order
  [observed: positions under a body node repeat the file's local translations: forearm
  under upper arm 23.81 in both, head under torso 45.09 against 45.10, right upper arm
  under torso (40.72, −11.59, −2.33) against (40.76, −11.68, −2.21)].
- Each entry is the body node's transform under its parent node, a node under the file's
  root taken under the reference [hypothesis: so read, the bodies of the three lie within
  15 to 35 units of height, scattered as a fallen skeleton's, the first at the reference's
  position; read under the first body instead, the thighs came 30 units below the arm].
  The same rule as the corpses' (§5): there the first body's parent is a node under the
  root, here the root itself.
- **In NVForever** [design]: an MSTT, STAT or MISC reference whose model's collision
  objects match its `XRGD` one for one is drawn with those nodes so posed (nodes without
  a body keep their place under their parent).
- **Its collision follows the same pose** [design, 2026-10-20, playtest list 5 item 71:
  before, the drawing lay on the floor while the bodies stood upright in the file's pose,
  so shots and the player met an invisible standing skeleton]. Each collision object
  hangs on a node; its place under the model's root is worked out again along the node
  chain with each body node's local transform replaced by its entry (position and
  rotation; the node's own scale kept, so the shapes keep their size), every other node
  keeping the file's. The body's own offset under its node is unchanged. The same rule
  and the same entries as the drawing (`fnv_world::xrgd`), so the two cannot part.
  The posed bodies are held where they lie (keyframed, `hold_jointed`) like any jointed
  clutter: the game's constraints are not modelled [hypothesis that the original keeps
  them at rest until touched; not checked]. Such a reference has no drawn-floor
  surfaces (`DrawnFloors`, read from the file's unposed geometry). Tallied
  `xrgd_posed` in the physics loader, which logs `phys: 0x… <model>: collision posed from
  XRGD (N bodies)` for each. When the entries do not fit (count or a body without
  a node) neither the drawing nor the collision is posed.
- Not checked: whether the original's bodies are solid in exactly this pose (no capture
  taken); the 1,570 REFR were not sorted by base type, so `XRGD` on other types is not
  posed.

### 5.2 The sweep over every ragdoll-like placed reference [2026-11-05, issue #1400]

`fnvtool world posed-clutter` (a new subcommand) lists every placed reference whose model
has two or more dynamic collision objects or any constraint, or whose model path names a
`skeleton`, or which carries `XRGD`, over a load order; `--summary` gives the counts only
(the per-reference rows are game data and stay out of the repo). Run over FalloutNV.esm
and the nine DLC masters [observed]:

| Branch | References | What NVForever does |
|---|---|---|
| actor (ACHR/ACRE) | 7,681 (441 with `XRGD`) | the corpse path, not clutter (§3, §5) |
| posed | 1,719 | `posable` type, `XRGD` fits (one entry per body, or a part's first entries) |
| refused — does not fit | 452 | `posable` type, `XRGD` refused; see the reasons below |
| not posable type | 309 | `XRGD` on a base type outside MSTT/STAT/MISC/CONT |
| STAT fixed | 236 | STAT/SCOL, ragdoll-like, no `XRGD` (physics.md §4.3.2) |
| no `XRGD`, jointed | 168 | held in the file's pose (`hold_jointed`) |
| `skeleton` path, no ragdoll | 161 | a single static body or none (a skull, a ribcage) |

The 452 refusals, by reason [observed]: 289 "a body's node is not in the model" (a body
hung on the mesh root, whose name the drawing's below-root node list does not hold — the
globes, the hanging office lights, some weapons), 151 "one body, but more XRGD entries
than it can hold" (a single-body item with extra entries), 12 "the model has no bodies"
(an `XRGD` on a model with no collision). The engine's `clutter_pose::pose` can also
refuse with "fewer XRGD entries than bodies", "no XRGD entries", "a body's node is
unnamed", "an entry is not finite" or "a node's parent comes after it"; none of those
occur in the ten masters.

The `not posable type` rows, by base type, with how many have a *fitting* `XRGD` (the
count rule holds, every body on a named node below the root) [observed]:

| Base type | References | Fitting | The models |
|---|---|---|---|
| ARMO | 139 | 138 | skinned ground objects (`…GO.NIF`, two bodies) and single-body hats/helmets |
| WEAP | 111 | 87 | single-body weapons lying in the world |
| ACTI | 37 | 33 | swinging traps (16/8 bodies) and single-body static plants/benches |
| ALCH | 19 | 17 | single-body bottles |
| PROJ | 3 | 0 | single-body mines (their body is on the mesh root) |

The skeleton clutter itself [observed: `fnvtool collide ragdoll`, `world posed-clutter`]:

- `skeletonwholebodyclothed.nif` (15 bodies, 14 constraints): 270 references, all posed
  (269 MSTT/STAT/MISC and one placed as a CONT — now posed, CONT being a posable type).
- `skeletontorsonaked.nif` (5 bodies): 249 references. 88 carry a 5-entry `XRGD` and pose;
  161 carry a 15-entry `XRGD`. `skeletontorsomaleclothes.nif` and `…02.nif` add one each
  of those 15-entry cases: 163 torso references in all.
- The arm and leg parts (`skeletonleftarmnaked.nif` 3 bodies, `…leftleg…` 3, `…rightleg…` 4,
  and their `maleclothes` copies) each carry an `XRGD` with one entry per body and pose.
- `skeletonmale50scasual.nif` (16 bodies) and `skeletonfemale50scasual.nif` (17 bodies) pose.

Every skeleton part now poses. The 163 torso references were refused before this change
only because their `XRGD` has 15 entries for a five-body model; [observed: the torso's five
body nodes — `Torso`, `Head`, `RightUpArm`, `RightForeArm`, `RightHand` — are, by name and
order, the first five of the whole clothed skeleton's fifteen]. That the torso's `XRGD` was
copied from the whole skeleton is a [hypothesis] for how the data came to be, not something
needed to pose it.

**The fix** [design, issue #1400]:

- `posable` widens to the one base type the sweep shows carrying a fitting `XRGD` on a
  ragdoll-like model: CONT joins MSTT, STAT and MISC (the whole clothed skeleton placed
  as a container). ACTI is left out (a swinging trap's model is drawn by `swing_traps`,
  its own rig, so posing its collision from `XRGD` would part it from the drawing), ARMO
  too (its fitting references are skinned ground objects — case E), and the single-body
  items ALCH, WEAP and PROJ (their one-entry `XRGD` is a single-body pose, a different
  concern from the ragdoll-like clutter pose).
- The drawing and the collision now share one pose condition: the `xrgd::matching` count
  rule holds, every entry is finite, and every body hangs on a named node below the mesh
  root (`clutter_pose::pose` on the drawing side, `NodeTree::pose_objects` on the
  collision side). Before, the drawing refused a body on the root while the collision
  posed it — the 289 references above were drawn and collided in different poses. A
  reference whose `XRGD` has more entries than its model has bodies poses from the first
  `bodies` entries when the model has at least two bodies (the torso case). A single body
  with extra entries is refused, and so is fewer entries than bodies.
- A refusal is logged once per model with the reason (`clutter pose: <model>: its XRGD
  does not fit (…)`), instead of the drawing's silent `debug!` and the physics loader's
  nothing.
- A STAT with a usable `XRGD` is still posed (its bodies fixed by `static_base_fixed`
  first, then posed, physics.md §4.3.2), as before.

Not done here (compared against the original later): case C (a jointed model with no
`XRGD` — settle as a ragdoll on load, or stay as authored), case D (a STAT skeleton with
no usable `XRGD` stays fixed), and case E (skinned skeleton models placed as clutter).

### 5.3 References enabled at run time [2026-11-14, issue #2339]

A player's report showed the Wild Wasteland refrigerator skeleton (0x000ED565, MSTT
`Skeleton50sCasualMale`, model `skeletonmale50scasual.nif`) standing upright beside the
open fridge 0x000ED566. Its `XRGD` has 16 entries for the model's 16 collision objects
and `fnvtool world posed-clutter` puts it on the "posed" branch [documented: `fnvtool esm
decode`, `fnvtool world posed-clutter`]. Its enable parent is 0x000ED563
`WildWastelandEnabler`, which is Initially Disabled and which the trait's quest script
`WildWastelandQuestScript` 0x000ED510 enables [documented: `fnvtool esm decode` of
0x000ED565 (XESP 0x000ED563), 0x000ED563 (record flags 0xC00) and 0x000ED510 (it
references 0x000ED563)].

- Render streaming (nvforever `world.rs`) draws only references enabled at the start of
  a game by the plugins; the rest are drawn by `nvf_play::script_draw` once the
  simulation says they are present (a script's Enable, or a save that holds the parent
  enabled). That second path drew every shape at the file's own transform and never read
  `XRGD`, so such a skeleton stood in the file's pose while its collision, built by the
  same physics loader as at a cell load, lay in the stored pose [design: before #2339].
- **In NVForever** [design, #2339]: both drawing paths pose through one function,
  `nvf_play::clutter_pose::drawn` (the §5.2 condition: a `posable` type, a fitting `XRGD`,
  every body on a named node below the root) and `shape_transform` (`p[n] * own` for a
  shape under a node). `clutter_pose` moved from the nvforever crate down to nvf-play,
  which both paths can reach. `script_draw` logs `clutter pose: 0x… <model> drawn in its
  stored pose, enabled at run time (N bodies)` once per reference.
- Which references this covers: every posable reference with a fitting `XRGD` whose
  enable parent chain does not leave it enabled at the start (Wild Wasteland enablers,
  quest enable parents, other initially disabled parents). Those with a rig (an idle, a
  door, script groups) stay unposed on both paths, as before.
- Smoke: `wild-wasteland-fridge-skeleton` enables the enabler from the console and expects
  the line above and the collision's.
