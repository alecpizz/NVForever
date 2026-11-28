# Runtime idle-file replacement

[observed: historical private replay, separate source] The private armor/Pip-Boy replay kept the sprint IDLE active after
movement stopped. Its installed script data changes that IDLE's model path to a
stop clip and asks to play the same record again. The setter stores the change in
Game.field_overrides, but baseline19c renderer's clip_path reads only immutable IdleDef
model data. IdleSlot also identifies repeat requests by IDLE record alone, so even
a newly loaded file would be treated as the previous clip being requested again.
Installed script/animation data remains private and is not copied into this patch.

[design] Resolve a valid runtime model override before the record's original path.
Use the normalized source-file identity when deciding whether a repeated IDLE
request is the same animation. A different file on the same IDLE starts a new
clip; unchanged-file repeated dialogue requests retain the existing idempotence
and pose handoff behavior. Apply this to player and NPC slots, including the
asynchronous readiness path. Owned tests must exercise the actual setter plus
renderer path lookup and changed-file versus unchanged-file slot transitions.

[hypothesis: unqualified boundary] This does not change section classification, IsIdlePlaying semantics, IDLE loop
counts or clamp policy. Whether the corrected stop clip clears the wrist-HUD and
normal Pip-Boy failures needs another live replay. Full native lifetime and
explicit view-selection parity remain unqualified.

[observed: current port] Initial candidate on main `19c0f08a`. Two owned regressions are authored but current-port compilation, listing, execution, strict lint, required broader checks and smoke remain pending. Earlier private results are not this head's qualification. No renderer/native probe has been run for this port. Temporary clone admission and scene placement are separate changes.

## Temporary-cloned IDLE admission (`TempCloneForm`)

- [documented] [TempCloneForm](https://geckwiki.com/index.php/TempCloneForm) returns a
  session-scoped clone FormID inheriting the original record's type and data, while
  keeping its own FormID for per-clone overrides (`SetIdleAnimPath` /
  `GetIdleAnimPath`) and playback queries (`ForcePlayIdle`, `PlayIdleEx`,
  `IsIdlePlayingEx`, `GetPlayedIdle`).
- [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-24,
  `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, isolated staging copy,
  read-only, `w4g_13_isidleplaying_and_idle_replace`
  (`tools/capture/nvse/catalogs/wave4g_f2_extenders.json`, companion catalog PR #3803):
  printed `play(pl=0/0/00000000->1/1/0001E5F2|doc=0/0/00000000->1/1/0001E5F2)`,
  `clone1(orig=Characters\_Male\IdleAnims\Talk_HandToEarpiece.kf|origModl=Characters\_Male\IdleAnims\Talk_HandToEarpiece.kf|afterSetModl=Characters\_Male\IdleAnims\Talk_HeadNod_Neutral.kf)`,
  `clone2(cl1=Characters\_Male\IdleAnims\Talk_HeadNod_Neutral.kf|clModl1=Characters\_Male\IdleAnims\Talk_HeadNod_Neutral.kf|src2=Characters\_Male\IdleAnims\WriteOnBlackBoardA.kf|cl2=Characters\_Male\IdleAnims\Talk_HeadNod_Neutral.kf)`,
  and `next(pl=1/1/0001E5F2|doc=1/1/0001E5F2)|playClone(cl=FF001344:play=1,exCl=1,exOrig=0,get=FF001344)`;
  no DLL or source consulted] `SetModelPath` and `SetIdleAnimPath` mutate the same
  underlying `MODL` path on an `IDLE` form, and `TempCloneForm` snapshots that effective
  `MODL` path at clone creation time: pre-clone edits carry onto the clone, while
  post-clone `SetIdleAnimPath` edits on the original stay isolated from the existing
  clone. Playing a cloned `IDLE` via `PlayIdleEx` sets `IsIdlePlaying=1`,
  `IsIdlePlayingEx(clone)=1`, `IsIdlePlayingEx(orig)=0`, and `GetPlayedIdle=clone`.
- [design] `Game::idle_def` resolves `TempCloneForm` aliases back to their authored
  `IdleDef` via `Game::unclone`. At `TempCloneForm` creation time,
  `Game::snapshot_model_path` copies any active `MODL/` session override from `source`
  onto `clone`; `Game::idle_animation_path` then checks only `idle`'s own `MODL/`
  override before falling back to the authored `IdleDef::model` (no live `unclone()`
  walk for `MODL/` overrides), and `Game::idle_has_file` checks the resolved path
  (ending in `.kf`) so `ask_idle` admits cloned IDLEs while still refusing non-IDLE
  clones (`"not an idle animation"`) and empty/folder paths (`"names no animation file"`).
- [design] `IdleRequest`, `Game::set_idle_playing`, `IsIdlePlayingEx`, and
  `GetPlayedIdle` keep the exact cloned FormID so playback queries distinguish a clone
  from its original (matching `playClone` above). Engine consumers
  (`engine/nvforever/src/idles.rs` `load`/`drive` section holds and
  `engine/nvforever/src/npc_combat.rs` `no_attacking`) resolve metadata through
  `Game::idle_def`.
- [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-24,
  `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters,
  `w4d_43_nif_selector_and_ui_entities` (`tools/capture/nvse/catalogs/wave4d_extenders.json`,
  #3679): on `IDLE` `0x00002FD2`, `rCl1 := TempCloneForm rIdle`,
  `rCl2 := TempCloneForm rCl1`, then `SetModelPath "...\\WriteOnBlackBoardB.kf" rCl1`
  mutated `rCl1` without mutating `rCl2` or `rIdle` (`idle(1,0,1)`);
  no DLL or source consulted] Post-clone `MODL/` edits on a parent clone do not leak into
  an existing child clone or back into the original `IDLE`. [hypothesis] Clone-of-clone
  pre-clone `MODL/` snapshot inheritance onto a newly created child clone (mirroring the
  observed original-to-clone rule in `w4g_02` and `w4g_13`) remains to be probed directly.
  Full save/load lifetime of cloned IDLEs remains session-only (`ext-save.md`).
