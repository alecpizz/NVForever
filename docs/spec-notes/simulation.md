# Spec notes: core simulation (Layer 4) — forms, references, inventories, activation

Clean-room description in plain language. Tags: **[documented]** cites a public source,
**[observed]** was measured on the owner's install (FNV 1.4.0.525; load order
FalloutNV, DeadMoney, HonestHearts, OldWorldBlues, LonesomeRoad, GunRunnersArsenal,
ClassicPack, MercenaryPack, TribalPack, CaravanPack — "the ten masters" below) with
`fnvtool sim forms`, **[hypothesis]** needs verification against the original game.

Record layouts come from the Layer 1 schema (`schemas/fnv`, re-expressed from xEdit
`wbDefinitionsFNV.pas` as documentation); this note covers what the simulation does with
them. Behaviour of the running game (what activation does, how leveled lists roll) is
not visible in the data and stays **[hypothesis]** until the in-game captures of §6 exist.

## 1. Forms and references

- A **base form** is a record that describes a kind of thing (a weapon, a container
  type, an NPC). A **reference** (REFR, ACHR for NPCs, ACRE for creatures) places one
  instance of a base form in a cell; its NAME subrecord names the base form, DATA holds
  position (x, y, z) and rotation (radians, x, y, z) [documented: xEdit].
- All FormIDs inside the simulation are global (load-order) FormIDs: the plugin-local
  top byte is replaced by the load-order index of the plugin it designates
  (docs/spec-notes/esm.md "FormIDs and load order"). The last plugin to define a FormID
  wins; a winning record with the Deleted flag (0x20) removes the form [documented: UESP
  "Tes4Mod:Mod File Format" record flags; observed: 67 placed references are deleted by
  their winning definition, no base form is].
- A reference's **cell** is the label of the innermost cell-children group (types 8
  persistent, 9 temporary, 10 visible-distant) that contains it [documented: UESP group
  types]. The simulation takes membership and cell from Layer 3's world index
  (docs/spec-notes/world.md), so both layers apply one rule.
- Besides REFR, ACHR and ACRE, cell groups hold 297 placed explosives (PGRE) across
  FalloutNV, DeadMoney, HonestHearts, OldWorldBlues and LonesomeRoad; their bases are
  projectiles (PROJ), e.g. `MineFragProjectile` "Frag Mine", `MinePlasmaProjectile`,
  `MineNVPowderChargeProjectile`, a C4 projectile [observed]. All 297 decode completely
  with the REFR subrecord layout [observed]. Each projectile is fired by a weapon (WEAP
  DNAM `projectile` [documented: xEdit]); for 290 placed explosives exactly one weapon
  fires the base projectile — Frag Mine 219, Powder Charge 33, Satchel Charge 25, Plasma
  Mine 10, Bottlecap Mine 3, all with the Land Mine or Mine Drop animation type — and 7
  use projectiles that several weapons fire [observed]. They are armed mines and charges;
  the player's activation disarms one into that weapon, left lying where it was [owner
  2026-10-19; how they trigger, burn and go off: combat-rules.md §11a]. In game [observed 2026-10-08, `bl_placed_mines`, Nipton house 8,
  god mode on]: the four frag mines stayed enabled and did not move for 600 game loops
  after the player arrived, and a scripted `<mine>.Activate player` changed nothing (no
  Frag Mine 0x0000433C in the inventory, the mine still enabled, same position); the
  same again as the scenario `L4.sim.trace.nipton_placed_mines` (2026-10-09, mine
  0x0013A656: no menu, no Frag Mine, GetDisabled 0, position unchanged) [observed]. The
  simulation's scripted activation of a placed explosive therefore does nothing
  (`World::activate_scripted`); the player's own activation still disarms [hypothesis].
  **They are live explosives, not decoration** [observed 2026-10-09, `cl_d_mines_live`,
  same house, no god mode, Health raised to 5000]: with the player placed 70 units from
  mine 0x00139551 it went off within 30 game loops (GetDisabled 1 afterwards, the
  reference keeps its position) and Health fell by 82.5; standing on mine 0x0013A656 it
  was still enabled after 0.5 s and disabled after 1.5 s, Health −94.5. The player at the
  arrival point, 237 units from the nearest mine, set none off (`bl_placed_mines`). So a
  placed frag mine detonates when the player comes within some radius between 70 and
  237 units, after a delay under 1.5 s, and is disabled by the explosion. Whether the
  player's own activation disarms one in time is not seen: the Activate control
  (`TapControl 5`) in `cl_d_mines_live` met a coffee table in front of the mine
  (`GetCrosshairRef` 0x0013952F); see `cl_d2_mines_disarm` (docs/spec-notes/
  layer4-captures.md §2). In the follow-up `cl_d2_mines_disarm` the crosshair was on
  mine 0x0013A655 (`GetCrosshairRef`) from 70 units away and the Activate control was
  tapped twice: nothing happened in 4 s — no Frag Mine, the mine enabled, Health
  unchanged [observed]. So this mine did not go off at 70 units (the trigger radius is
  near 70, or depends on more than distance), and the tapped control did not disarm it;
  whether `TapControl 5` reaches the game's activation at all is untested, so disarming
  by the player stays [hypothesis]. Proximity, timer and damage are Layer 6 (combat);
  the simulation keeps the mine's state only.
- **Enable parents** (XESP: parent reference, flag bit 0 "Set Enable State to Opposite of
  Parent" [documented: xEdit]): 5,171 references have one, 759 with the opposite flag;
  162 chains are two deep, none deeper; no chain loops or names a missing reference
  [observed]. The simulation derives a child's enabled state from its parent's
  (inverted for "opposite") and ignores the child's own flag while it has a parent
  [observed: a session of the original, 2026-11-05, `GetDisabled` read by script on
  persistent references: children flagged Initially Disabled read as their parent's state
  or its opposite, and follow the parent's Enable and Disable; issue #1620]; a broken
  chain falls back to the own flag [hypothesis: GECK wiki "Enable Parent"]. Render
  streaming's start rule and the physics at a cell load go by the same rule [design:
  `engine/nvforever/src/world.rs` `enabled`, `engine/nvf-play/src/phys.rs` `in_world`];
  that the original draws these references and gives them collision by it is
  [hypothesis]: only `GetDisabled` was read, no frame was taken.
- Record flag **0x400** on a reference coincides exactly with placement in a
  persistent-children group: 20,344 references have both, none has only one [observed].
  It is the "Persistent" flag [documented: UESP].
- Record flag **0x800** ("Initially Disabled" [documented: UESP]) is set on 4,723
  references [observed]. The simulation starts such references disabled [hypothesis].
  A disabled reference is not drawn, has no collision and cannot be interacted with by
  the player [observed by the owner in game, 2026-10-08] (scripted `Activate` on a disabled item still
  gives the item, §5).
- The ten masters place 427,022 references: 419,341 REFR, 3,942 ACHR, 3,739 ACRE
  [observed]. 1,492 REFR name base FormIDs that no plugin defines, all in the
  engine-reserved range below 0x800: 0x20 (1,228; every one carries portal data XPOD)
  and 0x17 (264; occlusion-plane data XOCP) [observed]; these are culling primitives
  provided by the engine [hypothesis] and matter to Layer 3 only.

## 2. The player

- The player's base form is NPC_ **0x00000007**, Editor ID `Player` [observed].
- No master places reference **0x00000014**, although 4,566 FormID fields point at it
  (docs/design/layer1.md §2.4) [observed]. It is the player's reference, created by the
  engine [hypothesis]. The simulation creates it on first use from the base form, with
  the base form's carried items, in no cell, at the origin.
- The base form carries the Pip-Boy Glove 0x00025B83 and the Pip-Boy 3000 0x00015038,
  one each (CNTO) [observed: `fnvtool esm decode`]. In game both are **equipped**
  (`player.GetEquipped` 1, one of each held) in the saves `nvftest` and `nvfdoc`, made
  after the intro [observed 2026-10-09, `cl_a_pipboy_*`]; in `nvfnew`, a new game before
  VCG01 stage 0, the player holds neither (5 stacks, weight 19) [observed]: the intro's
  scripts hand them over (Layer 5). The simulation's player starts wearing the
  non-playable armour its base form carries, i.e. both [observed for these two;
  hypothesis that the rule is "non-playable armour" rather than these two forms].

## 3. Items, containers, carried items

- **Inventory item** record types: WEAP, ARMO, AMMO, ALCH, BOOK, KEYM, MISC, NOTE, IMOD,
  CHIP, CCRD, CMNY, and LIGH when its DATA flags have bit 1 "Can be Carried"
  [documented: xEdit lists these as valid CNTO targets; flag name from xEdit LIGH DATA].
- **Value and weight** [documented: xEdit layouts]: WEAP, ARMO, BOOK DATA (`value` i32,
  `weight` f32 among other fields); MISC, KEYM DATA (value i32, weight f32); AMMO value in
  DATA, weight in DAT2; ALCH weight is DATA (f32), value in ENIT; LIGH DATA; IMOD DATA
  (value u32, weight f32); CCRD DATA (value u32, no weight). NOTE and CHIP have no value
  or weight field; CMNY has only DATA "Absolute Value" u32 (byte offsets: §7 `sim
  items`). CCRD also holds two INTV u32, suit then card value (xEdit enums: suit 1 Hearts,
  2 Spades, 3 Diamonds, 4 Clubs, 5 Joker; value 1 Ace, 2–10, 12 Jack, 13 Queen, 14 King,
  15 Joker) [documented: xEdit]; `CardClubs2Tops` ("2 of Clubs") has INTV 4 then 2
  [observed], consistent with that reading. Notes, casino chips, caravan money and
  caravan cards weigh nothing in the inventory, like caps [observed by the owner in game, 2026-10-08];
  carried-weight totals count them as 0 (their `sim items` weight stays `null`, as the
  records have no field). Schema layouts for IMOD, CHIP, CCRD, CMNY were
  added 2026-10-07 and decode every instance in the ten masters (`fnvtool esm coverage`)
  [observed]. Five AMMO records, all flagged Non-Playable creature ammunition
  (e.g. `AmmoBloatflyDart`), have no DAT2 and therefore no weight [observed].
- **Carried items** (CNTO, repeated: item FormID, count i32) appear on CONT, NPC_ and
  CREA [documented: xEdit]. In the ten masters [observed]: 30,414 entries; 7,365 on
  containers and 15,993 on actors name a leveled item list (LVLI) instead of an item;
  **105 entries have count 0** (none negative; none on containers, so all on NPC_/CREA,
  e.g. the Vault 21 gamblers' templates); 6 entries name non-items (three STAT/MSTT in the
  test container `TestQAMisc`, three uncarriable LIGH on the Gomorrah bouncers).
  The simulation gives nothing for a count-0 or non-item entry: NPCs with count-0
  entries carry nothing for them [observed by the owner in game, 2026-10-08]; non-item entries stay
  [hypothesis].
- **CONT DATA**: flags u8 (bit 1 "Respawns"), weight f32 [documented: xEdit]. 3,417
  container bases; 177 respawn; 845 have no CNTO [observed]. Session-only
  `SetContainerRespawns` / `GetContainerRespawns` (`World::set_container_respawns`,
  `World::container_respawns`, `nvse_gaps.rs`) override a `CONT` base's Respawns
  flag for the current session (non-containers are ignored; carried across a
  same-run load via `WorldSession` / `World::take_session` / `World::put_session`,
  not saved in `Snapshot`) [documented: GECK wiki `SetContainerRespawns` /
  `GetContainerRespawns`, base-form flag toggle; observed: `gaps8`, value-first
  returns 0 and reads back; hypothesis: session-only lifetime, like `base_edits`]
  and are read by `World::reset_containers` (timed cell respawn and `ResetInterior`)
  as well as `World::restocks` (merchant container restocking) [hypothesis: live
  container-respawn consumers read the updated `CONT` Respawns flag].
- **Respawn**: when a cell is loaded again after staying unloaded for at least the
  respawn delay, its containers whose base has the Respawns flag return to their
  authored state (new leveled roll, anything left inside lost; lock restored
  [hypothesis]). No game setting holds the delay in the masters (§5b); the running game
  answers `GetGameSetting "iHoursToRespawnCell"` with **72** in all three test saves
  [observed 2026-10-07 `aa_respawn_delay`, again 2026-10-09 `cl_a_pipboy_*`], and the
  simulation uses that (`SimConfig.respawn_hours`, else the setting if a plugin defines
  it, else `DEFAULT_RESPAWN_HOURS` = 72). Other references are not reset yet. The engine
  reports unloading/loading (`detach_cell` / `attach_cell` ops).
  **Captured as a scenario** [observed 2026-10-09, `L4.sim.trace.goodsprings_cell_respawn`,
  save `nvftest`, unattended: leaving = the console's `coc` to the gas station, time by
  GameHour]: the General Store fridge 0x00105824 (Respawns, leveled contents, not a
  merchant box) and toolbox 0x001057FF (no Respawns), each emptied and given one teddy
  bear (an item neither leveled list can yield, so its count is exact). After a first
  74 h absence (which resets whatever the save remembers about the store), the fridge
  kept its teddy bear after absences of 48 h, of 1 h following 30 h spent inside (79 h
  after the last reset, 31 h after planting), of 40 h twice in a row (80 h away in
  total, store visited in between), and of **71 h** — which crossed three midnights —
  and lost it after **73 h** away (4 new leveled stacks instead); the toolbox kept its
  teddy bear throughout. With `bl_container_respawn` (70 h no, 74 h yes): the delay is
  counted from the last time the player **left** the cell, as hours (not calendar days,
  not time since the last reset, not accumulated absences), with the boundary between
  71.0 and 73.1 hours — consistent with "at least `iHoursToRespawnCell` = 72"; an item
  put into a respawning container by script is lost at the reset. The candidate passed
  16/16 as written before the capture.
  **In game** [observed 2026-10-07, `L4.sim.trace.goodsprings_respawn`, since retired]: Ringo's cash
  container 0x001629C0 (base CaravanCashRingo, Respawns, 200 caps) in GSGasStation, emptied
  with `RemoveAllItems`, held **200 caps again after the player left, waited 24 hours
  outside and came back**, and again after 96 hours. So the delay for this container is at
  most about 24 game hours (plus the minutes spent walking), well under the 72 h default;
  A follow-up (`aa_respawn_delay`) read **`iHoursToRespawnCell` = 72 in the running game**
  (an engine default; no master defines it) [observed], and the same box stayed empty
  after 1 h and after 6 h away and was full after 12 h away (about 19 h after it was first
  emptied). So this merchant cash box restocks on a rule of its own, faster than the
  72-hour cell respawn [observed]. `aa_vendor_restock` (clock = GameDaysPassed): emptied at
  6.377, the player stayed inside 24 h → full at 7.379 (no cell unload needed); emptied
  at 7.379, 5 h away across midnight (back at 8.095) → empty; 10 h away without
  midnight (back at 9.476) → empty, i.e. empty for 50 h [observed]. Not hours away, not
  hours since emptying, not midnight. `aa_vendor_period` (player inside the whole time,
  never opening a menu; only scripted `GetItemCount` reads): full again at 7.379
  (emptied at 6.377); emptied at 7.379 and one stimpak put in (a sold item); still empty
  with the stimpak at 8.380, 9.382, 9.883; **full (200 caps) and the stimpak gone at
  10.384**, and full at 10.886 and 11.387 [observed]. So a restock **replaces** the
  contents (sold items are lost), needs no barter menu, and the second restock came
  between 2.50 and 3.01 days after the restock observed at 7.379; a fixed 3-day grid from
  day 0 is ruled out (no refill at day 9). Rule consistent with every capture
  [hypothesis]: when a merchant container is accessed and at least 72 game hours
  (`iHoursToRespawnCell`, 72 in game) have passed since its last restock, it is reset to a
  fresh roll of its base contents and the restock time becomes the access time. A save
  carries each container's last restock time, which the plugins do not, so captures that
  start from a save depend on that hidden state (the retired `L4.sim.trace.goodsprings_respawn`'s
  24 h case did; `L4.sim.trace.goodsprings_merchant_restock` avoids it). Merchant containers can be identified from the data: an
  ACHR/ACRE `XMRC` "Merchant Container" names them [documented: xEdit]; Ringo's
  reference 0x00104C7D points at 0x001629C0, and FalloutNV.esm has 77 XMRC links
  [observed].
  **Superseded by the 2026-10-08 captures** (`bl_restock_timing`, `bl_restock_schedule`,
  unattended, `GameHour` and TimeScale substitutes giving the same results; save `nvftest`
  inside GSGasStation, the box's last restock before the captures at midnight of day 11):
  the restock time is **not** the access time. Emptied at 12.388 and left unread for
  100 h, the box was full at 16.556; emptied again at once, it was **full again at
  17.556** (24 h later). Read and emptied every 2 hours, it refilled just after the
  midnights of days 14, 21 and 24 (first reads at 14.063, 21.077, 24.007), and was full at
  the first read (20.658) after 100 unread hours [observed]. Every refill seen in any
  capture (also days 7 and 10 in `aa_vendor_period`, day 17 here) came just after a
  midnight (GameDaysPassed crossing a whole number). Rule consistent with all of them
  [hypothesis]: **at each midnight d, a merchant container whose contents changed since its
  last restock L, with d − L ≥ 3 days, is restocked (contents replaced) and L = d.** Here
  L = 11 → 14 → 17 (in the unread gap) → 20 skipped (untouched and full since 17) → 21
  (emptied at 20.658, 4 days after 17) → 24. The "changed since" condition rests on that
  one skipped midnight; "skipped when full" or "skipped when untouched" fit equally. The
  period matches `iHoursToRespawnCell` (72 h) [hypothesis that it is the source].
  **Refuted the same day; restocks come on fixed days** (later 2026-10-08 captures, all
  unattended from `nvftest` with the player inside and time moved by `GameHour`; jobs from
  `tools/capture/jobs/make_restock_jobs.py` and the scenario
  `L4.sim.trace.goodsprings_merchant_midnight_restock`). The save's calendar: GameDaysPassed
  12.388 is 25 October 2281 (GameMonth 9), day 19 is 1 November [observed,
  `mr_diag_calendar`]. Restocked at day 14 and left full and untouched, the box emptied at
  19.4 was **not** refilled at midnight 20 (6 days after its last restock) but at 21
  (`goodsprings_merchant_midnight_restock`, and again with 2-hourly reads in
  `mr_diag_reads`: reads change nothing). Restocked at 17 and emptied at once, it was not
  refilled at 20 but at 21 (`mr_diag_month_short`, `mr_diag_ringo`), while restocked at 14
  or 21 and emptied at once it refilled 3 days later (17, 24). Restocked at 21 and left
  untouched, it was emptied at 24.4, 25.4 or 26.4 and every time refilled at **28**
  (`mr_diag_untouched_3d`, `_4d`, `mr_diag_long_untouched`). Ringo (0x00104C7D) stayed in
  GSGasStation, enabled, with the same daily AI packages throughout (`mr_diag_ringo`), so
  his state does not explain the skipped midnights. Across every capture of both saves
  (also `bl_restock_timing`, `bl_restock_schedule`, `aa_vendor_restock`,
  `aa_vendor_period`) refills came only just after midnights 7, 10, 14, 17, 21, 24 and 28,
  and an emptied box was passed over at midnights 8, 9, 13, 15, 16, 18, 19, 20, 22, 23, 25,
  26 and 27 [observed]: refills happen exactly on days whose **GameDaysPassed is 0 or 3
  modulo 7** (twice a week, 3 and 4 days apart; equally, the first midnight at or after
  each point 3 + 3.5k), whatever the last restock or access. A held-out run made after the
  rule was stated (`mr_predict_days`: emptied on days 29, 31, 35 and 38, read daily)
  refilled on exactly 31, 35, 38 and 42 and on none of the nine days between [observed].
  Rule [hypothesis, consistent with every capture]: **at each midnight that starts a day d
  with d mod 7 ∈ {0, 3}, every merchant container whose base has the Respawns flag is
  restocked (contents replaced).** No hidden last-restock time takes part, so a capture
  depends on the save only through where its start falls in the 7-day cycle (the earlier
  "save's last restock time" readings were this phase). Open: whether the schedule is
  global (two saves agree; other merchants not captured), whether an untouched box is
  re-rolled (invisible with fixed contents; a merchant with leveled stock would tell), and
  why GameDaysPassed modulo 7 (no week or restock setting appears in the masters;
  `GetGameSetting "iDaysToRespawnVendor"` returned 0 in game [observed], which may only
  mean that no such setting exists). The "changed since the last restock" condition and
  the link to `iHoursToRespawnCell` are dropped: the one skipped midnight they rested on
  (day 20) is not a restock day.
  **A second merchant does not follow that schedule** [observed 2026-10-09, diagnostic
  fields of `L4.sim.trace.goodsprings_cell_respawn`, save `nvftest`]: Chet's box
  0x00104C7A (`VendorContainerChet`, Respawns flag, leveled stock, in
  `NVVendorChestsCell`) was emptied and given one teddy bear at days 15.48, 17.48, 18.78,
  22.12 and 25.08 and read at each return to the General Store. It was restocked (teddy
  bear gone, 37–38 stacks) over midnights 16–17 and over 26–28, but **not** over 19–21
  (read at 20.45 and 22.12, day 21 is 0 mod 7) nor over 23–25 (day 24 is 3 mod 7). Under
  the rule above both should have restocked. So the schedule is not one global calendar
  for every merchant: Ringo's box and Chet's differ. What drives Chet's (e.g. a time since
  his last restock longer than Ringo's 3–4 days, or the merchant's own state) is open; the
  simulation still applies the Ringo rule to every merchant container
  (`world::is_merchant_restock_day`) [hypothesis, refuted for Chet], to be revisited
  with a capture that reads Chet's box daily.
  A merchant box whose base lacks the Respawns flag (Primm, Prospector's Den,
  0x0017409C, base `VLootSackVendorWastelandT2RSPros`; `bl_primm_restock`, both
  substitutes) was emptied and **never refilled in 7 days** of daily reads [observed]; so
  the flag gates merchant restocks too. After its `RemoveAllItems` it reported
  `GetItemCount` of caps 1 with `GetNumItems` 0, every read [observed; unexplained].
  Its contents on arrival differed between two loads of the same save (540 and 380 caps,
  42 stacks each) [observed]: the save did not hold them (cell never visited by that
  character) and they were rolled when the cell first loaded [hypothesis].
  **Cell respawn of ordinary containers** (`bl_container_respawn`, both substitutes;
  General Store, the player leaving and returning with the console's `coc`): the fridge
  0x00105824 (base `VLootFridgeClosedTier1WastelandRS`, Respawns flag), emptied, was still
  empty after 24 h and after 70 h away and held new contents (4–5 stacks) after 74 h away,
  twice; the toolbox 0x001057FF (no Respawns flag) stayed empty throughout [observed]. So
  the 72 h cell respawn (`iHoursToRespawnCell`) applies to containers with the Respawns
  flag, measured from leaving the cell, between 70 and 74 hours [observed]. Narrowed by
  the scenario above (2026-10-09): between 71.0 and 73.1 hours, hours not midnights.
- **AddItem on a container nothing has read yet**: a placed container's inventory is
  rolled from its CNTO the first time anything touches it, and an AddItem is such a
  touch, so the item joins the authored contents and nothing rolls them again over it
  later. Only a respawn or a merchant restock (both above) replaces them [design;
  test `add_item_before_a_container_is_read_is_kept`]. Issue #3021 looked like a lost
  AddItem and was not one: in Old World Blues the Basic Infiltration Test terminal
  (OldWorldBlues.esm TERM 0x0100BB5A item 0) puts the X-13 Document in
  `X13TestSafeREF`. When a test robot detects the player, the quest script
  (SCPT 0x01010918) sets `bDetected` and activates `X13FAILREF`, whose
  `NVDLC03X13FailureSwitchSCRIPT` (SCPT 0x01014FFC) does `X13TestSafeREF.RemoveItem` of
  the document [observed: decompiled]. The quest pilot's run of 2026-11-19 was seen by
  a robobrain 9 s after the AddItem ("Test failed. You have been spotted by a patrol.")
  and that script then ran [observed: the pilot log, 02:34:07.84]. The empty safe was
  the authored result of a failed test.
- **Non-playable items**: ARMO BMDT general flag bit 6 is "Non-Playable" [documented:
  xEdit wbBMDT, `schemas/fnv/objects.json`]. `PipBoy` (Pip-Boy 3000, 0x00015038) and
  `PipBoyGlove` (Pip-Boy Glove, 0x00025B83) carry it [observed: `fnvtool esm decode`].
  The game never shows the glove in the inventory, so it cannot be dropped, and it is
  always equipped [observed by the owner, 2026-10-08]. The engine's menus hide
  non-playable armour everywhere (`fnv_sim::forms::Item::non_playable`; weapons and ammo
  below). Both are equipped in played
  saves (§2) and **`player.RemoveAllItems` leaves both, still equipped**, and takes
  everything else (27 stacks → 2, weight 93 → 0; `EquipItem` of the glove afterwards
  changes nothing) [observed 2026-10-09, `cl_a_pipboy_nvftest`]. The simulation starts the
  player wearing them and `World::remove_all_items` / `transfer_all` (the same script
  function with a target) keep non-playable items [observed for the player and these two
  items; hypothesis for other holders and items]. Both weigh 0 (ARMO DATA) [observed].
- **Non-playable weapons and ammo; Can't Drop.** WEAP DNAM flags 1 bit 7 and AMMO DATA
  flags bit 1 are "Non-Playable", WEAP DNAM flags 1 bit 3 is "Can't Drop" [documented:
  xEdit `wbDefinitionsFNV.pas`, `schemas/fnv/objects.json`]. Boone's rifle
  `WeapBooneSniperRifle` 0x000CE549 and machete `WeapNVMacheteBoone` 0x00176E59 carry both
  weapon flags, and `AmmoCompanion` ("Magical Companion Ammo", 0x00176E54) is
  non-playable [observed: `fnvtool esm decode`]. The owner saw both in Boone's inventory
  in NVForever and says neither should be shown or taken [observed by the owner,
  2026-10-20]. The GECK wiki's "Unplayable Items" text: non-playable items do not show in
  inventories that are not the player's; non-playable armour is not shown in the
  player's Pip-Boy either, while the player's own non-playable weapons and ammo are, and
  can be equipped; lying in the world, neither weapons nor ammo can be picked up, and a
  weapon shows no activation prompt (ammo does) [documented]. RemoveAllItems and
  RemoveAllTypedItems keep "Unplayable Biped items" only [documented: GECK wiki], so
  non-playable weapons and ammo move with the rest. A Can't Drop weapon stays in the
  actor's hands at death and cannot be shot out of them, but can still break [documented:
  GECK wiki Weapon]. The engine: `Item::listed`, `Item::non_playable_biped`,
  `Item::cant_drop`; a broken Can't Drop weapon is unequipped and kept rather than
  dropped [hypothesis]; scripted `Activate` still gives a non-playable item [hypothesis:
  the wiki covers picking up by hand].
- **Armour models: which one lies on the ground.** ARMO holds up to four model paths:
  MODL "Male biped model", MOD2 "Male world model", MOD3 "Female biped model", MOD4
  "Female world model" [documented: xEdit `wbDefinitionsFNV.pas` ARMO]. In the ten
  masters [observed, 489 winning ARMO]: MODL 478, MOD2 430, MOD3 308, MOD4 126; 414 of
  the 430 MOD2 paths end in `GO.NIF` ("ground object", e.g. leather armour MODL
  `Armor\LeatherArmor\M\OutfitM.NIF`, MOD2 `...\M\GO.NIF`), the rest are other
  ground models (`GOM.NIF`) or the Pip-Boy's; only 1 MODL does; 30 armours have a MOD4
  that differs from MOD2 (e.g. `GamblerMHat01`: MOD2 a fedora `_GO`, MOD4 a pillbox hat
  `_GO`); 49 have no MOD2 (36 of them non-playable). In game [observed 2026-10-09,
  `cl_c_armor_models`, pictures in `C:\fnv-private\capture\shots\cl_c_armor_models.*`,
  save `nvftest`, male player, armours made with `PlaceAtMe` on open ground in
  Goodsprings at noon]: the outfits lie as **folded ground objects**, not as the
  body-shaped biped suits of MODL, and `GamblerMHat01` lies as a **fedora** (its MOD2;
  MOD4 would be a pillbox). After the console's `sexchange` (GetIsSex Female 1) the same
  fedora reference still showed a fedora; a second row placed while female showed the
  folded outfits again, but its hats were hidden by the yard fence, so whether a female
  player sees MOD4 is **not settled**, nor what an armour without MOD2 shows (Raul's hat
  was not identifiable). Two armours dropped from the inventory fell out of the picture.
  Rule for the engine: draw MOD2 for placed and dropped armour [observed for a male
  player]; MOD4 for a female player [hypothesis, open]; MODL when there is no MOD2
  [hypothesis].
- **Item models: which one lies in the world, per record type** (`FalloutNV.esm` alone,
  winning records; `cargo run --release -p fnv-world --example item_models`). Only ARMO
  and WEAP name more than one model; AMMO, MISC, ALCH, BOOK, KEYM, NOTE, IMOD, CHIP, CMNY
  and CCRD have MODL only [observed], which is then the model in the world.
  - ARMO (389): MODL 381, MOD2 345, MOD3 245, MOD4 107. 330 of the MOD2 paths end in
    `GO.NIF`; the other 15 are the `GOm.NIF` of nine children's, doctors' and mercenary outfits, the four Pip-Boys
    and the psychic nullifier (MOD2 = MODL) and `MountedVaultSuit101Armored`
    (`ArmorMounted.NIF`); 44 have no MOD2 (none of them has a MOD4, 8 have no MODL
    either) [observed].
    An ARMO with no MOD2 that is flagged Non-Playable (BMDT general flags bit 6) is armour
    only ever worn: it has no world model and a placed or dropped reference of it is not
    drawn or collided (`fnv_world::placed::worn_only`) [design, #2439: the Fort's two
    placed NVSlaveBackpack references, 0x0013732C and 0x0013732D, sit 565 and 913 units
    above the ground with nothing under them, and their biped model, skinned to Bip01
    Spine2 and authored at body height, hung in the sky; observer check 142]. Playable
    ARMO without MOD2 still lies as its MODL.
  - WEAP (261): MODL "Model", MOD2 "Shell Casing Model", MOD3 "Scope Model", MOD4 "World
    Model", MWD1–MWD7 "Model - Mod 1" … "Model - Mod 1, 2 and 3" [documented: xEdit
    `wbDefinitionsFNV.pas` WEAP]. Counts: MODL 207, MOD2 78, MOD3 26, MOD4 57, MWD1 34
    [observed]. 64 of the MOD2 paths name a casing or shell
    (`weapons\2handautomatic\assaultrifleshellcasing.nif`; the rest are `GrenadeFrag.NIF`
    13 times and a minigun projectile) and all 26 MOD3 paths are scope overlays in an
    `interface` folder (`interface\HUD\varmitscope.NIF`), so neither is a world model
    [observed]. Of the 57 MOD4: 35 end in `GO.NIF` (`44magnumrevolvernoscopeGO.nif`,
    `MinigunGO.NIF`, `boxingglove_GO.NIF`), 6 in `WorldObject.NIF` (fist weapons: MODL
    `PowerFistRigid.NIF`, MOD4 `PowerFistWorldObject.NIF`), 15 repeat MODL and one names
    another model (`WeapNV357RevolverUnique`: `357revolverCustom.nif`); 8 fist weapons
    (`WeapBrassKnuckles`, `WeapSpikedKnuckles`, `WeapDeathclawGauntlet`, the bladed and
    mantis gauntlets, …) have a MOD4 and **no MODL** [observed]. The other 46 without
    MODL have no model at all (`Fists`, creature, robot and turret weapons). 204 weapons
    have no MOD4: MODL is their only model that can lie in the world.
  - Rule for the engine (`fnv_world::placed::world_model_sub`,
    `BaseObject::world_model_path`): an item placed in the world or dropped is drawn with,
    and collides by, ARMO MOD2 / WEAP MOD4 when the record names one, else MODL
    [ARMO: observed in game, above; WEAP: documented field name and the paths, not
    captured in game: hypothesis]. Open: which model a dropped weapon with mods attached
    shows (a weapon with both MOD4 and MWD models, e.g. `WeapNV44Revolver`); the engine
    draws MOD4. `fnvtool world refs` keeps reporting MODL as `model` (docs/cli-contract.md).
- **Quest items**: an item marked as a quest item cannot be dropped from the player's
  inventory and cannot be sold, and a dead body holding one is not cleaned up until it is
  taken out [documented: GECK wiki Category:Items and SetQuestObject, owner exports
  2026-10-09 and 2026-10-07]. The mark is bit 10 (0x400) of the item record's header
  flags, named "Quest Item" per record type in `schemas/fnv` (`header_flags`; sources and
  the 339 flagged records of the ten masters in `docs/spec-notes/esm.md`). Scripts set or
  clear it with `SetQuestObject form flag`, which changes the form's flag [documented:
  GECK wiki SetQuestObject]. Implemented:
  - `Item::quest_item` is the record's flag; `World::is_quest_item` is the one answer the
    engine asks: a script's value (`World::set_quest_object`, kept in `fnv.sim.state/1`
    snapshots as `quest_objects`) when one was set, else the record's. That a script's
    value replaces the authored one for every reference of the form is [hypothesis: the
    flag is the base form's]. fnv-game's SetQuestObject writes there (it used to keep its
    own map, `FormChanges::quest_items`, now unused).
  - The player's own drop (`World::player_drop_item`, the inventory menu's right click)
    refuses a quest item with `SimError::QuestItem` and moves nothing [documented].
  - A **scripted** drop does not: in the game, `player.Drop` from a script removed a quest
    key and a stimpak made a quest item with `SetQuestObject 1` from the player's
    inventory like any other item [observed 2026-10-09, save `nvftest`, job
    `tools/capture/jobs/qi_drop_quest_item.steps.json`, output
    `C:\fnv-private\capture\out\qi_drop_quest_item.1791540717.jsonl`: Doc Hoff's Supply
    Key (KEYM 0x00068E8D, header bit 10) read `IsQuestItem` 1 and its count went 1 → 0
    after `player.Drop`; the stimpak read `IsQuestItem` 0, then 1 after `SetQuestObject`,
    and its count went 10 → 9 on the drop; after `SetQuestObject 0` it read 0 again and
    9 → 8]. So `World::drop_item`, which the script functions `Drop` and `DropMe` and the
    action scripts' `drop_item` use, ignores the flag. The same capture confirms that the
    game reads header bit 10 as the quest-item flag (`IsQuestItem`) [observed].
  - Other holders (NPCs, containers) may drop one [hypothesis: the pages name only the
    player; scripted drops ignore the flag anyway].
  - Moving a quest item into a container (`World::transfer`) is allowed [hypothesis: the
    pages say nothing about containers].
  - Selling: no barter exists yet; when it does, it must refuse quest items the same way
    [documented].
  - Corpses: nothing cleans up dead bodies yet, so the "not cleaned up while holding a
    quest item" rule has nothing to act on [documented, not implemented].
  - fnv-game's RemoveAllTypedItems leaves the player's quest items (as before, now read
    from `World::is_quest_item`); its RemoveAllItems does not exempt them yet although
    the catalogue says it should (`script-functions-catalogue.md`) [open].
- **Inventory model**: one stack per base item, split into groups of identical extra data
  (condition, owner, equipped). Removing more than is held removes what is there and
  reports that number [hypothesis: GECK `RemoveItem` behaviour as commonly described; to
  be captured]. Removal takes plain items first, then unequipped items with extra data in
  arrival order, equipped items last; items leaving an inventory are unequipped; extra
  data travels with transferred and dropped items, and a drop makes one reference per
  group [hypothesis]. A live planted explosive is a group of its own (`live`, saved as
  `"live": true`) that no removal, transfer, drop or equip takes; only its blast does
  [design, #1640; pickpocket.md §5].
- **Per-item extra data in the data** [observed, six masters]: COED (owner FormID, a u32
  whose meaning depends on the owner type, condition f32 [documented: xEdit]) appears
  1,661 times, always directly after a CNTO (1,419 on NPC_, 122 on CONT, 120 on CREA);
  its condition values are fractions between 0 and 1 (most common 0.5, 0.85, 0.65); only 2
  set an owner. Placed weapons and armour carry **XHLP** "Health" (460 WEAP, 217 ARMO
  references in FalloutNV.esm, also fractions), never other types. Condition is therefore
  a fraction of the item's full health (WEAP/ARMO DATA `health`) [hypothesis]. A COED on
  an entry that names a leveled list applies to whatever the list yields [hypothesis].
- **Equipping** [hypothesis: GECK wiki `EquipItem`; to be captured]: weapons replace the
  equipped weapon; armour replaces equipped armour sharing any body slot (ARMO BMDT biped
  flags [documented: xEdit]); aid items (ALCH) are consumed; other items cannot be
  equipped.
- **Encumbrance**: capacity = `fAVDCarryWeightsBase` + `fAVDCarryWeightMult` × Strength
  (150 + 10 × Strength in the masters); carried weight includes equipped items;
  ammunition weighs nothing outside Hardcore mode — all three [observed in game,
  2026-10-07, `L4.sim.trace.goodsprings_weight_and_time`: GetAV CarryWeight 200 at
  Strength 5 and 160 at Strength 1; GetAV InventoryWeight 18.5 for a 9mm pistol (1.5),
  leather armour (15), brahmin-skin outfit (2) and 100 9mm rounds (0.029 each), unchanged
  by equipping]. Weights add exactly (199 teddy bears at 1.0 → 199, plus spoons at 0.5 → 200.5, 201) [observed, `bl_encumbrance`]. Neither `IsPlayerOverencumbered` nor `GetIsPlayerOverencumbered` exists in the runtime's script compiler ("unknown variable", with and without `player.`) [observed], so the threshold was captured through running speed instead [observed 2026-10-09, `cl_b_encumbrance_speed`, `cl_b2_encumbrance_speed`, save `nvftest` in Goodsprings, no god mode, capacity 200, Forward held 90 loops]: the player **ran** (342–343 units/s, `IsRunning` 1) carrying 199.5, 200.0 and **200.5**, and **walked** (84–86 units/s, the walking speed of physics.md §6.3; `IsRunning` 0) carrying **201.0**, 201.5, 202, 203, 205 and 210.5. So the player is over-encumbered when the weight's whole part exceeds the capacity (equivalently, for whole capacities, weight ≥ capacity + 1), not as soon as the weight exceeds it; `World::encumbrance` applies this (which of the two readings holds for a fractional capacity is [hypothesis]). Over-encumbered running is refused; walking speed stays normal [observed]. Hardcore
  ammunition: 100 9mm rounds weigh 0 normally and **2.9 with Hardcore on** (0.029 each, the AMMO DAT2 weight), 0 again when switched off [observed, `bl_encumbrance`, `player.SetHardcore`]; `IsHardcore` read 0 throughout, even while the ammunition weighed 2.9 [observed; unexplained]. Strength comes from NPC_ DATA attributes
  (S.P.E.C.I.A.L. as seven u8 in that order [documented: xEdit]); the player base form
  authors 5 for every attribute [observed]. NPCs that take stats from a template are not
  followed yet.

## 4. Leveled item lists (LVLI)

Data [documented: xEdit]: LVLD chance none (u8, percent), LVLF flags (bit 0 "Calculate
from all levels <= player's level", bit 1 "Calculate for each item in count", bit 2
"Use All"), optional LVLG global, repeated LVLO entries (level u16, item FormID, count
u16 — the count is optional in the layout but present in all 16,950 entries [observed]).
3,479 lists; 7,871 entries name another list; flags: all-levels 1,526, each 2,042,
use-all 723; 41 lists have LVLG [observed].

Selection rules, implemented in `fnv_sim::leveled`; first derived from the flag names
only, now **consistent with the game** for the cases below [observed in game,
2026-10-07, `L4.sim.leveled.basic`: 300 `AddItem` draws per case on the owner's install
against 20,000 candidate draws; chi-square p ≥ 0.15 in every case, alpha 0.01, so only
differences of several percentage points would have shown]. Covered: chance none (50,
65), "each item in count" with count 3 (outcomes 0–3 stimpaks: 43/103/125/29 in game,
consistent with three independent rolls), all-levels vs highest eligible level
(`VFactionSquadStimpaks`: always 1 stimpak at player level 10, always 3 at level 30, in
game and candidate), Use All (`VendorChestStimpaksBoomer`), a uniform pick of 18
(`ClutterHouseholdJunkNV`). Console `AddItem` with a leveled list resolves it against
the player's level [observed, same captures]. `World::add_item` reads
`World::roll_level`, which `Game::sync_player_level` keeps at the game's player's level;
`SimConfig::player_level` stays the configured value (encounter-zones.md) [design].

Also **consistent with the game** for backlog B8 [observed in game, 2026-10-08,
`L4.sim.leveled.b8`, `parity-lab/inputs/layer4/leveled_b8.cases.json`: 300 `AddItem`
draws per case, unattended, save `nvftest`; chi-square p 0.16–1.0 in all ten cases]:
- **LVLG replaces chance none, at the global's current value.** The 41 lists with LVLG
  [observed] use `ScroungerOdds` (0x000645CD), `FortuneFinderOdds` (0x000645CC), both
  authored 100 and 100 in the save, or one-list vendor globals. With the global set from
  the console (`set ScroungerOdds to …`; read back unchanged before and after the
  draws): `AmmoMissilelauncherScrounger` (LVLD 0) at 100 gave nothing in 300/300;
  `Ammo20GaScrounger` (LVLD 100) at 25 gave the ammo in 238/300 (75 % expected);
  `Ammo10mmScrounger` (LVLD 0, each item) ×4 at 50 gave 0–4 rounds 14/78/122/70/16
  (binomial 4, ½: 19/75/113/75/19). So the global overrides LVLD in both directions
  (not a maximum or minimum of the two), and a value set at run time counts. Not
  covered: fractional or out-of-range values (the candidate truncates and clamps to
  0–100 [hypothesis]).
- **All levels, player level between entry levels:** `CapsFortuneFinder` (entries at
  1–6, 8, 8, 9; global set to 0) at level 7 gave only the six entries of levels ≤ 6,
  41–62 each; `VendorArmorRare75` (chance none 25, entries at 5, 5, 5, 7, 7, 12, 12) at
  level 10 gave nothing 69 and the five entries of levels ≤ 7, 36–54 each.
- **Level equal to an entry's level is eligible:** `VendorFlakShrapnelMiniNukes`
  (entries ×1 at 1, ×2 at 5, ×3 at 8, ×3 at 11) at level 8: ×1/×2/×3 = 100/97/103.
- **Count without "each item":** `NVDLC01ChipsChangeFortuneFinder` ×2 (all levels, global
  40) at level 12: nothing 110, 40/50/60 chips 66/71/53 (twice the entries of levels 1, 5,
  10); no mixed sums, so one roll times the count.
- **Nesting:** `VendorBasicMedicalSupplies100` (3 deep; sublists with entries at levels 1
  and 2/3; an entry ×5 naming the "each item" list `LootChemsStimpak75`, chance none 25)
  at level 2 gave 2–5 stimpaks as well as 1 (2/12/16/16 for ×2–×5), so a nested entry's
  count reaches an "each item" sublist as that many rolls; `LootChemsNVAll50` (3 deep,
  chance none 50 at the top and in a sublist; 17 outcomes) and `LL2Tier4ExplosivesMixed50`
  (5 deep, Use All at the top, chance none in sublists; 36 outcomes seen) agree too.

And for a count given to a Use All list over "each item" sublists [observed in game,
2026-10-26, `L4.sim.leveled.useall`, `parity-lab/inputs/layer4/leveled_useall.cases.json`:
300 `AddItem` draws per case, unattended, save `nvftest`, `ScroungerOdds` read back 100;
chi-square pass in all three cases]. `LootAmmo10mm` (Use All, not "each item": one 10mm
round, `Ammo10mm75` ×12, `Ammo10mmScrounger` ×12, both "each item") ×10 gave 50–130 rounds,
**every draw a multiple of 10** (mean 98.0, sd 16.2; ×1 gave 5–13, mean 10.0, sd 1.6). So
the count multiplies what one roll of the list gives, sublists included; it does not reach
the "each item" sublists as 120 rolls (the rule built before, mean 100 but sd 4.7, never
below 79). The Goodsprings ammo box on Barton Thorn's hill (`LootAmmoBox10mmGS`:
`WithAmmo10mmPistolLoot` ×1, `LootAmmo10mm` ×10) is where this shows: about 108 rounds on
average either way, but 60–140 in the original. A container's CNTO count is taken to act as
`AddItem`'s count [hypothesis: only `AddItem` was captured].
1. Roll 0–99; below chance none (or the LVLG global's current value) yields nothing.
2. Eligible entries have level ≤ the requesting level (the player's level; for
   containers [hypothesis], for AddItem [observed]).
3. Use All: every eligible entry. Otherwise one entry uniformly from all eligible entries
   (all-levels flag) or from those of the highest eligible level.
4. "For each item in count": N separate rolls of one; otherwise one roll times N.
5. Nested lists: an entry naming another list asks it for the entry's count (rolls, if
   that list is "each item"); a list without "each item" asked for N multiplies everything
   its entries give by N, and the multipliers of the lists above apply to all of it.

## 5. Locks, doors, activation

- **XLOC** on a reference [documented: xEdit]: level u8, key FormID, flags (bit 2
  "Leveled Lock"). Observed only on CONT (321) and DOOR (341) references; levels
  0 (63), 25 (120), 50 (182), 75 (91), 100 (34), 255 (172); 161 name a key, always a
  KEYM; 104 level-255 locks have no key [observed]. Level 255 is "requires key"
  [hypothesis; a level-255 trunk with a key refused scripted activation without the key
  and showed no lockpick menu, see "Locks in game" below]. A reference with XLOC starts
  locked [observed for three containers: `GetLocked` 1 on arrival].
- **Leveled locks** (XLOC flag bit 2): none in FalloutNV.esm; 26 in the add-ons, Dead
  Money 22, Old World Blues 3, Honest Hearts 1, on containers, gates and doors of levels
  0, 25 and 50 [observed 2026-10-24, private survey script]. Such a lock "will start at
  the base level specified and become harder as the player levels" [documented: GECK
  wiki Reference, Lock tab; GetLockLevel: "If the terminal is leveled, the value you get
  will be the leveled value (same as locks)"]; by how much is not documented. As built
  (`fnv_sim::refs::Lock::level_at`, `World::lock_of`): one step of 25 for every 10
  levels past the first, up to Very Hard (100), never Requires Key [hypothesis: a guess,
  listed as one in docs/owner-review.md until 2026-10-25; no decision on it is recorded].
  A leveled lock hardens from a fixed level only (`World::lock_read_level`) [design]: a
  container's, fixed when its cell loads, if its state is not made before that. A door
  gets no fixed level, so it keeps its authored level whatever the player's level or its
  zone [design]. Whether the original hardens at all is [hypothesis]: the GECK tutorial
  calls the checkbox deprecated [documented, encounter-zones.md §7]; observer check 88
  asks. GetLockLevel, the crosshair's lock line and the lockpick game read the level
  built here.
  The nav graph's door locks keep the authored level (it only asks locked or not).
- **XTEL** (door FormID, position, rotation, flag) makes a door a teleport [documented:
  xEdit]. The flag is "No Alarm": "No trespass alarm will be sent when the player uses
  the door" [documented: GECK wiki Reference, Teleport]; 5 doors, Dead Money 2 and
  Lonesome Road 3 [observed]; what it does here: crime-karma.md §3.3. 1,378 references have XTEL; 1,376 pairs are reciprocal (the destination door's
  XTEL points back); 0x00087A6B → 0x000827BA and 0x000CAEB5 → 0x000CAFA5 point at doors
  that teleport elsewhere [observed]. Every destination exists and is a DOOR reference
  [observed]. By where they lead (`fnvtool sim doors`, §7; the destination cell as
  `activate` resolves it, below): 274 interior → interior, 488 interior → worldspace,
  488 worldspace → interior, 22 within one worldspace and **106 into another
  worldspace**, between 22 worldspaces in 18 pairs, all 106 reciprocal [observed, ten
  masters]. Examples: the Mojave's north and east gates ↔ FreesideNorthWorld
  (0x00116420 ↔ 0x00116424, 0x0011641F ↔ 0x00116423), FreesideWorld ↔ TheStripWorldNew
  (0x00116425 ↔ 0x0013BB0D), FreesideWorld ↔ FreesideNorthWorld (bus and side doors),
  22 doors each way Mojave ↔ GreenhouseWorld01, 11 Mojave ↔ WastelandNVmini, the Dead
  Money districts, Big MT ↔ Higgs Village (0x0300BF95 ↔ 0x0300BEBB) and Lonesome Road's
  worldspaces. 6 of the 106 are locked, among them the Freeside → Strip gate 0x00116425
  (level 255, key 0x00119875) [observed]; there is no Mojave ↔ FreesideWorld door
  (FreesideWorld is reached through FreesideNorthWorld or the Strip) [observed].
- **Default activation by base type** [hypothesis, GECK wiki's description of object
  types; to be captured]: item → the activator picks up the stack (XCNT count, default 1)
  and the reference leaves the world; container → opens its inventory; door → teleports
  the activator to XTEL position/rotation and the destination door's cell, or toggles
  open/closed; NPC/creature → dialogue (Layer 6); activator, talking activator,
  furniture → no default effect beyond the script event. Locked doors and containers
  refuse activation unless the activator carries the key, in which case they unlock
  first (a locked container with its key in the inventory opens directly, no prompt
  [observed by the owner in game, 2026-10-08]). A base form's script (SCRI) receives OnActivate; whether an OnActivate block
  suppresses the default is Layer 5 [hypothesis].
- **In game** [observed 2026-10-07, `L4.sim.trace.goodsprings_doc_mitchell`, scripted
  `Activate` with the player as activator, Doc Mitchell's house]: three stimpak and five
  cap references each gave their item (counts 3 and 5); **activating an already-taken
  stimpak reference (0x00105BF3) again gave a fourth stimpak** — so after a scripted
  pick-up the reference can still be activated (whether it stays enabled in the world is
  not captured yet); `AddItem` 10 then `RemoveItem` 3 caps gave 12 (from 5);
  `RemoveItem` 5 stimpaks with 4 held left 0. The front door
  (`GSDocMitchellHouseIntDoorREF`) put the player at x −73162.92, y 1312.39 (the XTEL
  position within 0.05) but **z 8693.87, 7.44 above the XTEL z 8686.43**, and in cell
  **0x000DAEBB "Goodsprings" (WastelandNV grid −18, 0)**, not in 0x000846EA, the
  WastelandNV persistent cell that holds the destination door: after a teleport into a
  worldspace the actor is in the exterior grid cell containing its position. The z offset
  is the arrival rest height: the destination lies 3.6 units under the terrain, and the
  player is pushed up to 3.84 units above the LAND surface, on the terrain's contact
  shell (3.5) and within the band the player rests in after any door [observed: captures
  `dt_teleport_rest`, `dt_rest_detail`, `dt_door_arrivals`; rule and evidence in
  physics.md §6.2; `fnvtool sim run` applies the rule, §7].
- **Scripted `Activate` on item references** [observed 2026-10-07, two follow-up captures
  in Doc Mitchell's house]: `ref.Activate player` adds the item every time (one item,
  even from a stack; see "Dropping and taking back" below) — three
  activations of stimpak 0x00105BF3 gave 1, 2, 3 stimpaks; a stimpak taken by hand
  (0x00105BF5) still gave one more; a cap reference that was never taken but disabled with
  `Disable` (GetDisabled 1) also gave its cap and stayed disabled. `GetDisabled` stays 0
  and `GetParentCell` keeps reporting the original cell after a pick-up by script or by
  hand, so neither shows whether an item was taken. (The player found 2 of the 3
  stimpaks still on the shelf after the scripted activations, i.e. the scripted pick-up
  did take the first one [observed, owner].) `GetParentCell` of a persistent
  exterior reference (door 0x00103E69) asked from an interior returns no cell
  ("00000000"); asked outdoors it returns **0x000DAEBB, the WastelandNV grid cell (−18, 0)
  containing the door's position** (x −73349.5, y 1296.9), not the persistent cell that
  stores the door; the player's own cell outdoors was likewise the grid cell containing
  their position (0x000DAEBD, grid −19, 0, at x −73989.5, y 3750.2) [observed].
- **Created references** (dropped items) get FormIDs from 0xFF000000 upwards, a load
  slot no plugin can use [hypothesis for the starting number; observed in game: dropped
  stimpaks got 0xFF001315, 0xFF001316, 0xFF001317 in turn, `GetSourceModIndex` 255].
- **Locks in game** [observed 2026-10-08, unattended captures from `nvftest`,
  `L4.sim.trace.goodsprings_locked_safe_key` / `_no_key` and `ld_lock_levels`; scripted
  `<container>.Activate player`, then `MenuMode 1008` (container menu [documented: GECK
  wiki GetMenuTargetRef]) and `CloseAllMenus`]:
  - A locked container activated by script without its key opens nothing: no container
    menu, and at level 255 no lockpick menu either (MenuMode 0); contents and lock are
    unchanged. With its key in the activator's inventory it opens the container menu at
    once and is **unlocked for good** (`GetLocked` 0 afterwards, `GetLockLevel` still 255
    for a level-255 trunk), the key stays in the inventory; locked again with `Lock` and
    the key removed it refuses again, with the key back it opens again. Scripted
    activation therefore follows the same key rule as the owner's interactive test.
  - After `Unlock` the container opens on activation; `Lock` without a level relocks it
    at its old level, `Lock 255` / `Lock 100` set the level (`GetLockLevel` 255 / 100).
  - **`GetLocked` reads 1 for every locked container, also at level 255** — the
    authored "requires key" trunk 0x0015D0D0 (BlackMountainRadio2, `BMLootTrunk`) and a
    level-25 safe after `Lock 255` — not the 2 ("locked out") the GECK wiki lists for an
    impossible level. The simulation reports 0 / 1; 2 stays for broken locks (Layer 6).
  - A container in a cell the player is not in (Ringo's gas station seen from another
    interior) did not open its menu when activated by script, though `GetLocked`,
    `Unlock` and `RemoveAllItems` worked on it [observed, a mis-ordered first run]; the
    scenarios therefore activate containers in the player's cell only.
- **Placed ammunition without a count** [documented: GECK wiki Ammo, "Clip rounds: the
  number of rounds of ammo the object contains when placed in the world", a placed
  reference's Count overriding it]: a REFR of an AMMO base with no XCNT stands for the
  record's DATA Clip Rounds, not one round. The two `Ammo9mm` references on the Goodsprings
  gas station's counter (0x0010ABA6, 0x0010ABA7) carry no XCNT and `Ammo9mm`'s Clip
  Rounds is 20 [observed: `fnvtool esm decode`]; the owner reports 20 each in the game
  (2026-10-15). The simulation's starting count of a placed item is XCNT, else the AMMO's
  Clip Rounds, else 1 (`FormStore::placed_count`). Scripted activation still gives one
  (below).
- **Dropping and taking back** [observed 2026-10-08, `nvfdoc`, the player just outside Doc
  Mitchell's front door, `L4.sim.trace.goodsprings_drop_and_retake`, `ld_drop_refs`,
  `ld_drop_stack`]: `player.Drop <item> n` makes **one** reference holding all n (count 1,
  2, 3 by `GetRefCount`), placed **50 units in front of the player along its heading and
  30 units above its feet** (player at x −73162.92, y 1312.39, z 8693.86, `GetAngle Z`
  72.81°; item at −73115.16, 1327.17, 8723.86: offset 47.77 / 14.78 / 30.00, i.e. 50 ×
  (sin, cos) of the heading). It stayed exactly there for 10 s (no fall observed). A
  scripted `<dropped>.Activate player` gives **one** item, whatever the reference's count:
  a dropped pair gave 1 (player 3 → 4) and a dropped three gave 1 (1 → 2); afterwards the
  reference counts 1, is no longer listed by `GetFirstRef 201 0 0` (taken) but still by
  `GetFirstRef 201 0 1`, and is not disabled [observed]. The GECK wiki notes the same
  loss of a stack for scripted activation of a stacked item [documented: GECK wiki
  Activate, bugs]. A **placed** stack behaves the same (`ld_placed_stack`, BB rounds
  0x001730EA with XCNT 12 in GSHouseInterior03): one round, `GetRefCount` 1 afterwards,
  a second scripted activation one more round [observed]. The simulation's scripted
  activation (`World::activate_scripted`, the trace op `activate`) gives one item and
  leaves the reference at count 1; the player's own activation (`World::activate`, the
  engine) still takes the whole stack [hypothesis; not captured]. The simulation places drops at that offset (holder heading from its z
  rotation, §5 crosshair notes) [hypothesis for other actors and items].
- **After the drop: does a dropped item fall?** **Yes** [observed 2026-10-09, capture
  `pc_drop_fall`: a scripted `Drop`, and a `Drop` with the Pip-Boy open once it closed,
  fell from the drop point and came to rest lying down within 1 s; the new reference
  starts with GetAngle Z = the holder's heading; readings in physics.md §4.3]. The
  earlier readings, kept for the record:
  - The scripted drop above (`ld_drop_refs`, tgm and tdetect on) read `GetPos Z` 8723.86
    right after `player.Drop` and again 600 frames (10 s) later: the drop point, 30 units
    above the feet [observed, `C:\fnv-private\capture\out\ld_drop_refs.*.jsonl`].
  - Clutter does fall under Havok, and `GetPos` follows it: a whiskey bottle placed with
    `PlaceAtMe` and moved by `SetPos` bounced or rolled 50–300 units (B11 clutter row,
    physics.md §4.2) [observed].
  - Possible readings, none tested: a scripted `Drop` creates the item's Havok body at
    rest (asleep until something touches it), while `SetPos` wakes a body; or the
    dropped stimpak was held by something we do not model [hypothesis]. A drop from the
    inventory menu (what the engine offers) is not captured at all.
  - **What the engine does** [hypothesis] (`engine/nvf-play/src/dropped.rs`,
    `phys.rs`): an item dropped from the inventory menu gets its model's collision; when
    the model says dynamic it falls under gravity and settles. The stimpak's model
    (`meshes\clutter\health\stimpak01.nif`) has one dynamic object on the Clutter layer
    [observed: `fnvtool phys cell --cell GSDocMitchellHouse --list-refs`, 0x00105BF3]. A
    drop starts with rotation 0 (`World::drop_item`) [hypothesis]. While it moves, its
    pose (and, outdoors, the grid cell containing it) is written back to the reference, so
    reloads and snapshots find it at rest. `fnvtool sim run` applies no physics to created
    references, so scripted traces keep a drop at the drop point, as the capture did.
  - **Models with several dynamic bodies** (issue #1604). Clothing and armour draw their
    MOD2 ground object when dropped (`BaseObject::world_model_path`). Those `GO.NIF`
    models are two or four dynamic bodies, joined by limited hinges, with the mesh skinned
    to them [observed: `fnvtool collide ragdoll`, `armor\vaultsuit\m\go.nif` Bone01 and
    Bone02; 2026-11-05 drops of the Pre-War Spring Outfit, Leather Armor, Merc Charmer
    Outfit and Vault 21 Jumpsuit made 2, 2, 4 and 2 bodies]. Only a single body's pose is
    written back, so the drawn item hung at the drop point while its bodies fell. Now a
    created reference whose model's bodies are all dynamic gets one body that holds every
    shape at its place in the file (`phys::one_dynamic_body`). The item falls and comes
    to rest in its folded pose, like any other drop [design: the hinge is not simulated;
    whether the original lets the cloth flop is not captured]. Models where any body is
    static or keyframed keep their bodies as they are.
  - **Capture for the owner** (not run: other sessions are using the game; about 2 min,
    the last part needs a person). From `nvfdoc`, outside the front door, tgm:
    (1) `player.Drop` a stimpak, find the new reference with the `GetFirstRef 201 0 0`
    scan (as `ld_drop_refs` does) and read `GetPos X/Y/Z` and `GetAngle X/Y/Z` after 0,
    1, 3 and 10 s; (2) `<ref>.SetPos Z (<ref>.GetPos Z + 1)` and read the position again
    after 3 s: if it now falls, scripted drops start at rest; (3) by hand: Pip-Boy →
    drop one stimpak → close, wait 10 s, then in the console `prid <ref>` and
    `getpos z`, `getangle x`, `getangle y`: whether a menu drop falls, where it rests
    (above the porch floor or the terrain, x/y spread from the drop point) and whether
    it lies on its side. If (3) stays put, `crate::phys::PhysState::add_created` should
    add created references' dynamic objects at rest (or as keyframed) instead.
- **A dead actor's weapon** (`engine/nvf-play/src/combat.rs` `death_drops`,
  `iDeathDropWeaponChance` 100 in FalloutNV.esm [observed]): it leaves the body's
  inventory as a dropped reference (as above) that remembers the actor
  (`RefState::dropped_by`). The corpse's loot menu lists that reference beside the body's
  own items while it lies in the world, and taking it there picks the reference up
  (`World::activate`: condition and mods kept), so the weapon on the ground and the row
  are one item; picked up from the ground, it is no longer listed (#1032) [hypothesis:
  the original's corpses list the weapon lying beside them, from the owner's report;
  not captured]. Such a drop gets its collision at once and falls (#1033).
- **Crosshair activation in the game** (`engine/nvf-play/src/activate.rs`; values and
  rules to be captured, none observed yet):
  - **Reach**: 150 game units from the eye [hypothesis: placeholder, about two metres at
    ~70 units per metre]. No game setting in the masters names an activation distance
    [observed: FalloutNV.esm has no GMST whose Editor ID contains "Activat" or "Pick"
    other than script and lockpicking-XP settings]; the original's value is presumably
    built into the executable [hypothesis]. Capture idea: with the prompt showing, step
    back until it disappears and log the player-to-reference distance.
  - **Targeting**: the activatable reference nearest along the view within reach.
    Candidates are the reference whose collision the view ray hits first, and references
    whose render bounds (each mesh's box in its own space) the ray enters before the first
    collision hit of any kind, so walls and table tops hide what is behind them; a box the
    eye is inside does not count; ties go to the collision hit [hypothesis: many small
    items have no collision, yet the game lets the player pick them up]. An actor's render
    box is rough (a skinned mesh's bind-pose bounds, which do not follow a body lying on
    the floor [hypothesis]): any other box the ray enters before the first collision hit
    wins over it, nearer or not, so an item beside a body is taken rather than the body
    searched; with no such box the body is picked by its box as before (#3125) [design].
    **A bed yields to what lies in it**: when a box of a bed (furniture whose FURN marker
    flags include Sleep, bit 31; furniture.md §1) holds, along the ray, the whole stretch
    through another reference's precise box (not an actor's rough one) that the player
    can activate and that the ray enters before the first collision hit as it met it, or
    within the 2 units behind it that pick.rs's `SURFACE_SLACK` allows (#3333; not a door
    found behind that hit), every box of that bed is set aside, and the rest is
    picked by the nearest rule above. Full containment decides only this setting aside;
    nothing else changes. A bed's box runs from the floor to the top of its headboard and
    encloses what lies on the mattress, so the ray entered it before a suitcase lying
    there and the bed won (#3323: the Suitcase 0x000AFA24 on BedQueenRight 0x000D72C7 in
    BisonSteve02 [documented: FalloutNV.esm]) [design; reported: the player's issue says
    the suitcase can be looted in the original, not captured; observer check
    3323-suitcase-on-a-bed]. The render boxes are each
    drawn mesh's bounds, not the base's OBND (SuitCase02's OBND is all zeros
    [documented: FalloutNV.esm]). When no bed box wholly holds the suitcase's stretch
    (the suitcase hanging over the bed's edge), nothing is set aside and the nearest rule
    decides, whichever box that is; the bed's collision hit before the suitcase's box, and
    the bare mattress, pick the bed. Another bed's box sets nothing aside either. Chairs, benches and activators do not yield: a chair's back keeps
    its prompt over a switch behind it. A wall before the suitcase still hides it, two
    boxes over the same stretch both stay, and an actor's rough box sets nothing aside
    (a sleeper's bed still yields to the sleeper, below). Beds are known from the script
    runtime's records; without it nothing is set aside.
    The console's click keeps the plain nearest rule. Collision on the trigger layer (12)
    does not block the ray [hypothesis]. Nor does the Invisible Wall layer (27): "the
    player can activate / loot objects through this layer" [documented: GECK wiki
    "Collision layer", INVISIBLE WALL; #1602, docs/spec-notes/cells-refs.md].
  - **Inside the target** (#3351; `activate/pick.rs` `choose_inside`): when the target
    chosen so has a render box, an activatable reference the ray reaches within that box
    takes its place: its collision hit lying between the box's entry and exit, or its own
    box (not a rough one) lying wholly between them, the nearest first, and so on inwards
    [design]. What a collision hit hides stays hidden, so a closed door or lid covers
    what is behind it. The case: the open locker in the Sloan barracks (QJLocker01
    0x000E7610, `clutter\lockercabinet02.nif`, a CONT at 2736.6, -735.1, 16282.8) with a
    first aid kit (0x000E7612, a CONT with collision of its own, at 2774.2, -736.6,
    16359.6) on its upper shelf and Duck and Cover! (0x000E7613, at 2771.6, -731.4,
    16324.6) with burned books on the lower one [observed: `fnvtool world refs --cell
    0x000E5CCE`, FalloutNV.esm]. The locker's collision is hollow: rays from 135 units
    in front, straight in, hit the kit at 132.5 units, a burned book (0x000E7615) at
    125.3, the back wall at 149.2 where a shelf is empty, and the closed half's door at
    114.6 [observed: `fnvtool phys cell --cell 0x000E5CCE --ray 2774,-600,16362,0,-1,0,400`
    and the same at x 2756 and 2771 with z 16327, and x 2700]. The ray enters the
    locker's own render box at its front, before anything on the shelves, so by the
    nearest rule alone the locker was always the target [observed: player reports
    R-BHSP2X8C and R-C6VJTJFT]. What the original picks there is not captured
    [hypothesis: the things on an open shelf, as the reports expect]. Only a container
    gives way to anything inside it; an activator gives way only to an item (a reference
    whose prompt is Take) lying in its box, never to another activator (#3323) [design,
    #3937]. The case: a note on top of a table radio (0x0301057F) in an Old World Blues
    interior (0x030086CB); the radio's box reaches over the note, so the radio was the
    target wherever the crosshair was on the note [observed: player report R-FQ39PUHZ,
    every pick `target 0x0301057F Radio (Activate) by Bounds at 55-87`].
  - **Talking through bars** (#3233; `engine/nvf-play/src/activate/see_through.rs`): when
    the ray's first hit is a reference's collision on a see-through layer, Transparent (3),
    Transparent Small (26) or Transparent Small Anim (28), and nothing is offered (no
    activatable collision hit, no render box entered before it), every hit along the ray
    within reach is read in order, past the colliders on those layers; if the first other
    thing is a living actor whose prompt is Talk, that actor is the target (by collision,
    at its own distance; the log line says "through" and the bars' reference) [design].
    A closed door hides what is behind it on any layer, a see-through one included; an
    open one on those layers is passed [design]. E on such a target only opens the
    conversation (`script::talk_by_hand`, `fnv_game::Game::start_dialogue`): it is not an
    activation, so no OnActivate block, perks' choices (Cannibal, Mister Sandman),
    companion wheel or default action runs, and an actor asleep is not woken through the
    bars [design]. The Vault 3 captives stand behind JailCellDoorway01 (0x000EF3FA, a STAT
    with one collision object on layer 3): three rays from the player's eye at the
    report's spot toward Rick Lancer (V3Prisoner01Ref 0x000F47B0, placed at -2773, 4664,
    5888) hit it 25.2 to 27.5 units out [observed: `fnvtool phys cell --cell Vault3b
    --ray -2723,4553,6005,-50,110.7,-25,200` and the same with the direction's z -60 and
    -10, FalloutNV.esm, 2026-11-21]; started again along the first ray at 26 and 30 units
    it hit the same bars 6.4 and 2.4 units on, and at 35, 45 and 60 units it hit nothing
    within its 200 units [observed: the same command with those five origins]. The
    layers are the ones actors see through ("Actors can see through this layer type"
    [documented: GECK wiki "Collision layer", TRANSPARENT, TRANSPARENT SMALL];
    Transparent Small Anim by its name [hypothesis]); for the activation pick the wiki
    says only that nothing but projectiles passes Transparent, so the original's
    crosshair on these layers is an observer question (observer-checks.md #251). The
    rule is kept narrow on purpose [design]: only Talk passes, so items, containers,
    doors, a body to search and a pocket to pick behind bars, a fence or a window stay
    out of reach (render-bounds picks still count only before the first collision hit,
    and the activation ray itself still stops on the bars); anything else between the
    bars and the actor (a wall, a closed door) hides the actor; GetCrosshairRef and
    PlaceAtReticle's far ray keep the first collision hit. Shots (`layers::shot_ignores`)
    and movement are not changed. Starting the conversation checks no line of sight or
    distance (`fnv_game::Game::start_dialogue`; the walk-up distance is for force greets
    only), so the bars do not refuse it.
  - **A bed someone lies on yields to the sleeper**: when the target chosen above is a bed
    an actor other than the player is on (a bed marker, IDs 1 to 9, at any stage of
    getting on, sleeping or getting off; furniture.md §2) and that actor's body is within
    the reach of the eye, the target is that actor, logged `by Occupant`, its distance
    the body's: the nearest of its posed bones (the parts the renderer publishes for hit
    volumes, which follow the lying body), else of its render boxes. No hit on the body
    itself is asked for. A sleeper beyond the reach (the bed aimed at by its far end), or
    one that cannot be activated now, leaves the bed the target. A free bed is the target
    as before, and a bed in use offers the player nothing, being one user per reference
    (furniture.md §5, item 3) [design, #3231: the bed's box encloses the mattress, so the
    ray enters it before the sleeper's rough box, and the bed won; what the original
    targets on a sleeper is not captured]. Chairs and benches are not included: a bench's
    free seat stays usable beside a sitter. Scripts still see the bed: GetCrosshairRef
    reports the raw collision hit.
    - What activating the sleeper does: only one fully settled on the bed (GetSleeping 3,
      `Game::asleep`) takes the wake-and-talk path (`ByHand::Woken`): it gets up and the
      conversation starts once it is off the bed, or after `WAKE_TALK_SECONDS` (8 s of
      game time) when the exit is slower, then with it still on the bed. One still
      getting on or already getting off is talked to at once, as any actor, by the
      ordinary dialogue [design].
    - Squire Maxson (a child, TTW's CitadelBRing) was seen lying across the middle of a
      wide bed rather than on one side (#3231's second report) [observed: the report's
      screenshot]. Unresolved: whether the bed's entry marker and the clips' offsets put
      him there as the original would, or the clip choice, which asks the IDLE tree for
      an adult body (`child: false`, engine/nvforever/src/furniture.rs `user`), gives a
      child an adult's bed clips [hypothesis; to settle by comparing the marker and the
      pose with the original].
  - **What is offered**: references the simulation considers present (not taken, enabled)
    whose base is an item, container, door, actor or placed explosive with a weapon to
    give; activators and furniture only with a display name (FULL) [hypothesis: unnamed
    activators are trigger volumes and markers].
  - **Arrival heading through a door**: the XTEL rotation's z angle in degrees. Reference
    angles are radians and a z rotation turns clockwise seen from above (world.md §4
    [observed]), so z = 0 faces north (+Y) and z is the compass heading [hypothesis: that
    an actor faces its model's +Y; the heading after a teleport is not captured].
  - A door whose destination is in another worldspace is not followed yet (the player's
    simulated position is put back).

## 5b. Game settings and game time

- **GMST** DATA is a union chosen by the Editor ID's first letter: `f` f32, `i` i32,
  `s` NUL-terminated string [documented: xEdit]. The masters hold 649 settings (459 `f`,
  61 `i`, 130 `s` in FalloutNV.esm alone), all matching the rule [observed]. Examples:
  `fAVDCarryWeightsBase` 150.0, `fAVDCarryWeightMult` 10.0 [observed]. There is no
  respawn-delay setting in the masters (no `iHoursToRespawnCell`) [observed]; the running
  game still answers `GetGameSetting "iHoursToRespawnCell"` with 72 [observed, three
  saves, §3], so the setting and its value are built into the engine.
- **Time** is kept in globals [observed, FalloutNV.esm]: GameYear 0x35 = 2281, GameMonth
  0x36 = 9, GameDay 0x37 = 18, GameHour 0x38 = 12, GameDaysPassed 0x39 = 5.0, TimeScale
  0x3A = 30. Advancing time [hypothesis]: TimeScale is game seconds per real second;
  h game hours add h to GameHour and h/24 to GameDaysPassed; each wrap past 24 hours
  advances GameDay with Gregorian month lengths, GameMonth counting from 0. In game
  [observed, same scenario]: waiting 24 + 6 hours from day 18, month 9 gives day 19,
  month 9; from day 31, month 9 at 20:00, waiting 6 hours gives day 1, month 10. This
  fits month 9 = October (31 days) but does not yet distinguish it from a rollover of an
  out-of-range day 31 in a 30-day month; a capture across the end of a 30-day month would.
  **Settled** [observed 2026-10-08, `bl_calendar`: day 27 of every month of 2281 and of
  February 2280, then +24 h five times]: months count from 0 with Gregorian lengths 31,
  28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31 (GameMonth 9 = October); day 31 of month 11
  rolls to day 1, month 0 of the next year; **February 2280 has 28 days** — no leap days,
  although 2280 is a Gregorian leap year.
  While the player is in menus the clock keeps running at TimeScale (a first capture read
  GameHour 14.136 for 14), so fractional time is not compared
  (docs/spec-notes/xnvse-capture.md §5).
- Globals changed at run time are part of the simulation state (`Snapshot.globals`).
- **Frame-sized time** [design, #1377; not a claim about the original]: the running clock
  adds a frame's time to f64 copies of GameHour and GameDaysPassed
  (`fnv_sim::clock::FineClock`) and writes the f32 globals from them, because an f32 sum
  drops a step under half its own step (GameDaysPassed stopped near day 128 at TimeScale
  30 and 60 frames a second, and never left 5.0 at TimeScale 1). The globals stay the
  truth, each of the two on its own: a write to one drops its copy even when the value is
  the same (`World::set_global`, which script assignment, SetGlobalValue and the console's
  `set` use), and a stored value other than the one last written replaces it. The copies
  are saved beside the globals as optional fields (`Snapshot::clock_hour`,
  `clock_days_passed`; `SNAPSHOT_FORMAT` is unchanged), so a restored run goes on exactly as
  the uninterrupted one; an older save, a copy that no longer rounds to its global, or an
  hour copy of 24 or more seeds from the global. A stored GameHour stays below 24 until the day wraps. Commands
  that name an hour (SetGameHour) land on it (`World::advance_to_hour`) rather than adding
  time computed from the rounded global, which would stop short of midnight. A GameHour
  that is no number counts as hour 0 at the next advance, and a GameDaysPassed that is no
  number counts as 0 [design]. From a GameHour a script stored out of 0..24, SetGameHour
  counts as an advance does: 50 is two days on at 2:00, so 10 is eight hours on and the
  date rolls two days (three past midnight when the hour has gone by); a negative hour
  rolls no days back [design].

## 6. Capturing the oracle (in-game)

Layer 4 scenarios compare *observations* (item counts, position, cell) that the original
game can report through console functions [documented: GECK wiki function pages
`GetItemCount`, `GetPos`, `GetParentCell`, `AddItem`, `RemoveItem`, `RemoveAllItems`,
`Activate`]. Procedure by hand (or automated, below):
1. Load the pinned save at the scenario's start location (Goodsprings: inside Doc
   Mitchell's house).
2. Run the scenario's steps as console commands (`player.removeallitems`,
   `<ref>.activate player`, `player.additem <id> n`, …); record each query's value.
3. Write the values into the golden `sim/<scenario>.trace.json` with the same `label`s.
Until then the scenarios report `error` (missing golden), as Layer 1's xEdit scenarios did.

**Automated capture**: the `nvf_capture` xNVSE plugin runs a scenario's steps in the
original game and records the query results (docs/spec-notes/xnvse-capture.md;
tools/capture/README.md §10). `capture_steps.py` writes its jobs from the files in
`parity-lab/inputs/layer4/` using the console equivalents below (ops without one, such as
waiting or changing cells, become prompts) and turns its output into the goldens.

**Console equivalents of the trace ops** [documented: GECK wiki function pages; the
mapping itself is this project's]: `remove_all_items` → `player.removeallitems`;
`add_item` → `player.additem <id> <n>`; `set_av … Strength v` → `player.setav strength
<v>`; `get_weight` → `player.getav InventoryWeight` (`weight`) and `player.getav
CarryWeight` (`capacity`); `equip_item` → `player.equipitem <id>`; `set_global` → `set
<Global> to <v>`; `get_global` → `show <Global>`; `advance_time h` → waiting h hours
(Wait menu, at most 24 at a time); `get_disabled` → `<ref>.getdisabled`; `enable` /
`disable` → `<ref>.enable` / `<ref>.disable`; `detach_cell` / `attach_cell` → leaving
the cell through its door / coming back (wait outside, in another cell, in between);
`get_item_count` on a container → `<ref>.getitemcount <id>`; `remove_all_items` on a
container → `<ref>.removeallitems`. FormIDs in the inputs are load-order
FormIDs of the ten masters in the order of this note, which is also the console's
numbering when the game loads them in that order (Old World Blues = 03). Use a new
level-1 character without perks or implants, so no modifier changes carry weight
(`goodsprings_weight_and_time`).

**Leveled-list samples** (distribution scenarios, golden `sim/<name>.samples.json` in the
`fnv.sim.samples/1` shape of §7, with `outcomes` only). Per case of the scenario's
`fnv.sim.cases/1` file, on a test character whose level is set to the case's `level`
(`player.setlevel <n>`), repeat `samples` times: `player.removeallitems`, then
`player.additem <list> <count>`, then note every item and count the inventory now holds
(the inventory screen, or `player.getitemcount <id>` for the list's possible items).
A case with `globals` first sets each one from the console (`set <global> to <value>`)
and reads it back before and after the draws (the golden keeps `globals_read` and
`globals_read_after` next to the counts; a value that drifted means the draws did not
run with the value asked for), then restores it.
Record each draw's outcome key (§7) and add up the counts per key. That console
`AddItem` resolves a leveled list, against the player's level, the way containers and
actors do is a [hypothesis] [`AddItem` documented: GECK wiki]; a resolution inside a
spawned container (`placeatme` of a container base holding the list) is the
cross-check. A few hundred draws per case are enough for the comparator to see
differences of a few percentage points; `capture_steps.py cases` makes this a job for the
`nvf_capture` plugin.

## 7. CLI contract (Layer 4)

All documents are single JSON objects on stdout; FormIDs are strings `"0x%08X"`
(global); exit codes as for every `fnvtool` command. `<lo>` is
`<data_dir> <plugin>...` (load order) and `--schema <dir>` the Layer 1 schema directory.

### `fnvtool sim containers <lo> --schema <dir>` → `fnv.sim.containers/1`

```json
{"format": "fnv.sim.containers/1", "count": 3417,
 "containers": [
   {"formid": "0x0002FCC3", "edid": "DresserAbandoned", "name": "Dresser",
    "respawns": false, "script": null,
    "entries": [{"item": "0x0016BB08", "count": 1, "class": "leveled_item"}]}]}
```

- One object per winning, non-deleted CONT record, ordered by global FormID.
- `edid` (EDID), `name` (FULL) and `script` (SCRI, global FormID) are `null` when absent.
  `respawns`: bit 1 of the first DATA byte (false without DATA).
- `entries`: the CNTO subrecords in record order, with the item FormID made global and
  the count as stored (i32). Entries with a null item (0) or an item whose master index is
  outside the plugin's master list are omitted.
- `class` of an entry's item: `item` if the item's winning record is one of the item types
  of §3 (LIGH only with "Can be Carried"); `leveled_item` for LVLI; `other:<SIG>` for any
  other existing record type (including uncarriable LIGH); `engine_builtin` for a FormID
  below 0x800 that no plugin defines; `missing` otherwise. A deleted winning record
  counts as not defined.
- Byte layouts used [documented: xEdit]: EDID and FULL are NUL-terminated Windows-1252
  strings; SCRI is a u32 FormID; CNTO is u32 FormID + i32 count (8 bytes); CONT DATA is
  flags u8 + weight f32; LIGH DATA is time i32, radius u32, color (4 bytes), flags u32 at
  byte offset 12 ("Can be Carried" = bit 1), then further fields. All little-endian.

Lab comparator: **keyed** on `formid` over `/containers`, all fields.

### `fnvtool sim items <lo> --schema <dir>` → `fnv.sim.items/1`

```json
{"format": "fnv.sim.items/1", "count": 2,
 "items": [
   {"formid": "0x0000000F", "sig": "MISC", "edid": "Caps001", "name": "Bottle Cap",
    "value": 1, "weight": 0.0, "health": null, "biped_flags": null}]}
```

- One object per winning, non-deleted record of an inventory item type (§3: WEAP, ARMO,
  AMMO, ALCH, BOOK, KEYM, MISC, NOTE, IMOD, CHIP, CCRD, CMNY, and LIGH only with "Can be
  Carried"), ordered by global FormID. Winners, deletion and FormID globalisation as for
  `sim containers`.
- `sig`: the record type. `edid`, `name`: EDID, FULL (NUL-terminated Windows-1252),
  `null` when absent.
- `value` (integer), `weight` (f32 widened to a JSON number), `health` (integer) and
  `biped_flags` (integer) are read from these fields [documented: xEdit
  `wbDefinitionsFNV.pas`; byte offsets within the subrecord, little-endian]:

  | Type | value | weight | health | biped_flags |
  |---|---|---|---|---|
  | WEAP | DATA +0 i32 | DATA +8 f32 | DATA +4 i32 | — |
  | ARMO | DATA +0 i32 | DATA +8 f32 | DATA +4 i32 | BMDT +0 u32 |
  | AMMO | DATA +8 i32 | DAT2 +8 f32 | — | — |
  | ALCH | ENIT +0 i32 | DATA +0 f32 | — | — |
  | BOOK | DATA +2 i32 | DATA +6 f32 | — | — |
  | KEYM, MISC | DATA +0 i32 | DATA +4 f32 | — | — |
  | LIGH | DATA +24 u32 | DATA +28 f32 | — | — |
  | IMOD | DATA +0 u32 | DATA +4 f32 | — | — |
  | CCRD | DATA +0 u32 | — | — | — |
  | NOTE, CHIP, CMNY | — | — | — | — |

  "—" means the record format has no such field: the value is `null`. A field is also
  `null` when its subrecord is absent or shorter than offset + 4 bytes, and a weight that
  is not finite (NaN, ±infinity; none occur) is `null`. When a subrecord occurs more than
  once, the first occurrence is used (none does in the masters). A record that cannot be
  decoded is still listed, with every field `null`. CMNY DATA is named "Absolute Value"
  by xEdit and is not treated as the item's value [hypothesis: it is the denomination in
  the Caravan game]. Known difference, not exercised by the data: `fnvtool` reads fields
  through the Layer 1 schema, which decodes a subrecord whole or not at all, so a
  truncated subrecord makes *all* its fields `null` there, not only those past the end
  (no truncated item subrecord exists in the ten masters [observed]).
- In the ten masters [observed, `oracle_sim.py items`]: 3,972 items (NOTE 1,225, MISC 603,
  WEAP 496, ARMO 489, CCRD 324, KEYM 313, ALCH 226, AMMO 145, IMOD 112, BOOK 27, CHIP 6,
  CMNY 6, carriable LIGH 0 — all 728 LIGH lack "Can be Carried"); the only `null` among
  fields the format has are the five AMMO weights of §3; every CCRD has value 2 and every
  IMOD weight 0.0.

Lab comparator: **keyed** on `formid` over `/items`, all fields (`L4.sim.items`, against
the independent oracle).

### `fnvtool sim run <lo> --schema <dir> --actions <file> [--nif-schema <dir>] [--no-physics]` → `fnv.sim.trace/1`

Input `fnv.sim.actions/1` (`parity-lab/inputs/layer4/*.actions.json`): `seed`,
`player_level`, `actions` (ops: `activate`, `add_item`, `remove_item`, `transfer`,
`transfer_all`, `drop_item`, `remove_all_items`, `get_item_count`, `get_pos`,
`get_disabled`, `get_global`, `set_global`, `advance_time`, `equip_item`,
`unequip_item`, `get_av`, `set_av`, `get_weight`, `get_locked`, `detach_cell`,
`attach_cell`, `lock`, `unlock`, `enable`, `disable`), optional `hardcore` (default false)
and `report`.
Queries (`get_*`) produce observations; `get_disabled` reports `disabled` (the in-game
`GetDisabled`), `get_global` and `get_av` report `value` (`get_av` knows the seven
S.P.E.C.I.A.L. attributes), `get_weight` reports `weight`, `capacity` and `over`
(in-game: `GetAV InventoryWeight`, `GetAV CarryWeight` [hypothesis on the actor value
names]). `get_locked` reports `locked` (the in-game `GetLocked`: 0 not locked, 1 locked,
§5). `activate` is the scripted `Activate` (an item reference gives one item of its
stack, §5). An `activate` step with a `label` is also an observation: `opened` is true when the
activation opened the target's inventory (in game: `MenuMode 1008` right after it; the
capture then closes every menu with `CloseAllMenus`). `drop_item` reports `references`
(one per group of identical extra data). A reference named `created:<item>` (item by
Editor ID or FormID) is the newest reference created at run time from that base form that
is still in the world (not taken, enabled); in game it is found by scanning the item
references of the player's cell (`GetFirstRef 201 0 0` / `GetNextRef`) for one of that
base with `GetSourceModIndex` 255 — the last one met when there are several, so a
scenario keeps at most one such reference in the world at a time.
`detach_cell {cell}` / `attach_cell {cell}` (cell by global FormID or Editor ID) mark the
cell unloaded / loaded again; `attach_cell` reports `reset`, the references whose
contents respawned (§3 "Respawn"). `activate` on a placed explosive (PGRE) reports
`"result": "disarmed"` with the `item` (weapon) added to the activator (§1). In `final`, a
stack without extra data is a count; otherwise `{"count": n, "groups": [{"count": k,
"condition": c, "owner": id, "equipped": true}]}`. References and forms are named by
global FormID, Editor ID, or `"player"`.

**Teleports come to rest by physics** (2026-10-08): when `activate` moves an actor through
a teleport door, `sim run` builds the destination's collision world the way `fnvtool phys
cell` does (an interior cell, or the exterior cell containing the destination with the
squares around it; terrain, collision primitives and model collision read with the NIF
schema from `--nif-schema`, default `nif` next to `--schema`; references present by the
simulation's state, the plugins' flags for references it has not touched) and puts the
actor where `fnv_phys`'s arrival rule says it comes to rest (physics.md §6.2; the same
function the game uses after a door). The step's result then carries
`"settled": {"pos": [x, y, z], "support": {"owner": {"kind": "terrain", "grid": [x, y]} |
{"kind": "ref", "formid", "object", "layer"}, "z": <bare surface>, "shell": <units>} | null}`
beside the stored destination `pos`, and later `get_pos` reports the settled position.
`--no-physics` keeps the stored destination (the contract before this change). Without a
NIF schema only terrain and collision primitives support the actor.

```json
{"format": "fnv.sim.trace/1", "seed": 0, "player_level": 1,
 "steps": [{"step": 0, "op": "activate", "result": {"result": "picked_up", "...": "..."}}],
 "observations": [{"label": "caps after taking five", "step": 10, "count": 5},
                  {"label": "player after leaving", "step": 19, "cell": "0x000DAEBB",
                   "pos": [-73162.93, 1312.43, 8693.88], "rot": [0.0, 0.0, 1.27]}],
 "final": {"0x00000014": {"base": "0x00000007", "inventory": {"0x0000000F": 12}, "...": "..."}}}
```

Lab comparator: **keyed** on `label` over `/observations` (`count`, `cell`, `pos` with an
absolute tolerance). `steps` and `final` are diagnostics, not compared: their outcome
names are this engine's own.

### `fnvtool sim sample <lo> --schema <dir> --cases <file>` → `fnv.sim.samples/1`

Leveled-list outcome histograms: the candidate side of the distribution scenarios
(spec section 8.1). Input `fnv.sim.cases/1` (`parity-lab/inputs/layer4/*.cases.json`):

```json
{"format": "fnv.sim.cases/1", "seed": 0,
 "cases": [{"label": "stimpak 50 %", "list": "LootChemsStimpak50", "level": 1,
            "count": 1, "samples": 20000}]}
```

(Counts in the examples are illustrative.) `list` is an LVLI named by Editor ID or global FormID (`"0x…"`); `level` (default 1) is
the requesting level; `count` (default 1) the number requested, as in a CNTO entry or
`AddItem`; `samples` (> 0) the number of independent draws; `globals` (optional) maps
globals, by Editor ID or global FormID, to the values they hold during the case's draws
(as if set from the console first; e.g. a list's LVLG global), the others keeping their
authored values. Output:

```json
{"format": "fnv.sim.samples/1", "seed": 0,
 "cases": [{"label": "stimpak 50 %", "list": "0x00177FAF", "level": 1, "count": 1,
            "samples": 20000,
            "outcomes": {"nothing": 10012, "0x00015169 x1": 9988}}]}
```

- Cases in input order; `list` as a global FormID string; `globals` echoed as given
  (only when the case has any). A name that is not a global is a case error.
- One draw resolves `count` of `list` at `level` with the leveled-list rules of §4
  (nested lists included). The items drawn are added up per item FormID; the outcome key
  is the items in ascending FormID order, each written `"0x%08X x<n>"`, joined by `", "`;
  a draw that yields no item is `"nothing"`.
- `outcomes` maps each key that occurred to its number of draws (their sum is `samples`).
- Determinism: one RNG stream seeded with `seed`, consumed by the cases in order and the
  draws in order, so the same file gives the same document.
- A case whose `list` is not an LVLI, or whose resolution fails (nesting too deep), gets
  `"error": "<text>"` and no `outcomes`; the other cases still run (exit code 0).
- Optional, not required: `"probabilities": {key: p}` with the exact distribution; when
  present, the comparator tests goodness of fit against it instead of comparing samples.

Lab comparator: **distribution** over `/cases`, keyed on `label`, `field` `outcomes`
(parity-lab/README.md). Goldens are in-game samples (§6).

### `fnvtool sim forms <lo> --schema <dir>` → `fnv.sim.forms/1`

Observational report behind §1–§5 (counts per base type, item value/weight coverage,
carried-item targets, leveled-list statistics, reference flags, locks, teleport
reciprocity, loader issues). Not a scenario. `quest_items` (2026-10-09): `by_type` and
`items` (formid, sig, edid, name) of the items the loader reads as quest items, and
`item_record_header_bits`: per item record type, how many winning records set each
header flag bit, whatever the schema names (the observation behind §3 "Quest items").

### `fnvtool sim doors <lo> --schema <dir>` → `fnv.sim.doors/1`

Every teleport door (XTEL) by where it leads, behind the counts in §5. The destination
cell is the one `World::activate` moves the actor into (`RefTable::teleport_cell`: the
destination door's cell, or in a worldspace the exterior cell containing the XTEL
position); a cell's worldspace is `RefTable::world_of`. Not a scenario.

- `teleport_doors`: count; `by_kind`: counts of `interior_to_interior`,
  `interior_to_world`, `world_to_interior`, `same_world`, `cross_world`, `unresolved`
  (destination door not placed).
- `cross_world_by_pair`: `"<from WRLD EDID> -> <to WRLD EDID>"` → count.
- `cross_world`: one object per door into another worldspace: `door`, `edid`,
  `from_world`, `from_cell`, `from_cell_name` (EDID, else `"x,y"`), `pos`, `to_door`,
  `to_world`, `to_cell`, `to_cell_name`, `to_pos`, `to_rot` (XTEL), `back` (the
  destination door's XTEL points back), `lock` (`{level, key}` or null).
