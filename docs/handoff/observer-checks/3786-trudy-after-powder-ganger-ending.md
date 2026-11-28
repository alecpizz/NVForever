# After Ghost Town Gunfight ends on the Powder Gangers' side, does Trudy keep her gunfight guard post, and what does GetQuestVariable read on a stopped quest?

Priority: P2
Needs: a save before Ghost Town Gunfight's fight with Trudy's help agreed (or the console to set VMS16 there); a person at the keyboard; the console.
Spec: #3786; `crates/fnv-game/src/functions/quest.rs` `get_quest_variable`.

What NVForever does [observed: FalloutNV.esm, `fnvtool esm decode` and `script decompile`]:
`GSTrudyGunfightPackage` (0x00116810, a Guard package, weapon drawn) runs while
`GetQuestVariable VMS16 bTrudyHelp == 1` and `GetQuestVariable VMS16 bGunFightStart == 1`.
Only stage 100 (Goodsprings wins) sets `bGunFightStart` back to 0; stage 110 (the Powder
Gangers' side) only calls `StopQuest VMS16`. NVForever reads a stopped quest's variables as
they were, so a player who sided with the Powder Gangers finds Trudy standing guard outside
with her gun drawn for good (player report R-P5F7K9EB).

In the original:

1. Side with the Powder Gangers after agreeing to Trudy's help, finish the fight, and wait a
   game day. Where is Trudy, and is her weapon out? (`prid` her, `GetCurrentAIPackage`.)
2. In the console: `GetQuestRunning VMS16`, then `ShowQuestVars VMS16`. Does
   `bGunFightStart` still read 1?
3. If Trudy is back to her routine though the variable reads 1, note what she runs instead.
