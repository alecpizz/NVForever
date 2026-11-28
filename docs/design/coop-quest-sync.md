# Co-op: a guest's quest progression sync, and questlines to sit out

[design] A co-op guest chooses whether the host's quest progress also counts in its own
save, and can sit out whole questlines: for example, share the main story up to Benny and
the Platinum Chip, then keep Mr. House's quests out of its own save. Turning sharing off,
or sitting a line out, while a shared quest is part way through warns first. It builds on
joint progression (`docs/design/coop-shared-progress.md` §3).

Code: `crates/fnv-game/src/coop_questlines.rs` (the lines, the guest's choices, the checks),
`coop_joint.rs` (the host's offers), `engine/nvf-play/src/coop_progress.rs` (the guest's
filter, the choices sent to the host), `engine/nvf-play/src/lobby.rs` (the menus),
`engine/nvf-coop/src/progress.rs` (`Progress::JointChoice` carries the lines sat out),
`engine/nvforever/src/coop.rs` (the lobby's player list).

## 1. What the guest chooses

- **Quest Sync** (On/Off), in the guest's Co-op screen (pause menu › Co-op). It is the
  join question's answer (`coop_progress::CoopProgress::joint_ask`) kept: once a character
  has answered either way, the question is not asked again and the toggle changes it at
  any time, also mid-session. Until the screen that asks the question lands, the toggle
  (or `NVF_COOP_JOINT`) is how a guest answers.
- **Questlines**, a sub-menu of the same screen: every line in §2, each "Synced" or
  "Sitting out". The sub-menu stays usable while sync is off (the choices wait for it).
- Both are saved **with the character** (`PlayerState::quest_sync`, carried by
  `coop::Character` like the claim ledger), so they hold across sessions, saves and hosts.
  Lines are saved by key (`"house"`), not by position, so a later table keeps old saves.

## 2. Questlines: how they are grouped and where they split

A line is a short list of **roots**, its split points (editor IDs [observed:
FalloutNV.esm and the DLC masters' QUST records, `fnvtool esm inventory`]), or a DLC
plugin. Everything else comes from the data (`coop_questlines::compute`):

1. **Roots** by editor ID.
2. **Families**: a quest whose editor ID is a root's plus a suffix (the timers, the
   support and fail quests: `VMQHouse` → `VMQHouse1`…`VMQHouse8`, `VMQHouseFail`,
   `VMQHouseLockdown`; `VMS46` → `VMS46Timer`). Where the root ends in a digit the suffix
   may not start with one (`VMS2` must not take `VMS20`). A quest named as another line's
   root is that line's, not a family member (`VMS29a`, Birds of a Feather, is Cass's).
3. **DLC plugins**: every quest of the line's master (`DeadMoney.esm` …).
4. **What they start**: a quest started (`StartQuest`, or `SetStage` to its first stage)
   by a member's quest script, stage result scripts or dialogue lines joins the line,
   to a fixed point. It never takes a quest that is a root, family or plugin member of
   another line, nor a quest that runs from the game's start (start-game enabled: the
   dialogue holders and timers every line touches).

A quest can belong to several lines (it is a root of each). It is **sat out when any of
its lines is**: the guest's own save never takes a stage it chose to keep out of.

The data's prerequisite reading (`coop_prereqs`) links only one base-game quest (VMS51 to
VMS05) [observed: `fnvtool coop prereqs` over the six masters], so it cannot group lines:
hence roots plus the start graph. `fnvtool coop questlines` prints the table for review.

| Line (key) | Roots, the split points | Notes |
|---|---|---|
| Main story: to Benny and the chip (`main`) | VCG02, VCG03, VMQ01, VMQTops | Back in the Saddle, By a Campfire on the Trail, They Went That-a-Way, Ring-a-Ding-Ding! The trunk every branch shares. |
| Mr. House (`house`) | VMQHouse (+family), vfreeformlucky38 | The House Always Wins (all eight and Lockdown/Fail), The Moon Comes Over the Tower. |
| NCR (`ncr`) | VMQ03a, VMQ05, VMQNCRFail, VHDKimballSpeech, VHDAssassinationDialog | You'll Know It When It Happens, For the Republic Part 2. |
| Caesar's Legion (`legion`) | VMQ02, VMQ03b, VMS35, VMQCaesarFail | Render Unto Caesar, Arizona Killer, Et Tumor, Brute? |
| Yes Man / Independent (`yes-man`) | VMQYesMan01 (+01a), 02, 03, Failsafe, Support | Wild Card. |
| (the four above) | vHDBattleController, VHDHouseBattle, VHDLegionBattle, VHDIndependentBattle, VMQ06, VMQEndStates (+family), VLegateShowdown | The Battle of Hoover Dam and the ending: listed under the branch it ends (VHDHouseBattle: House …) and the shared ones (Eureka!, Viva Las Vegas!, end states, the Legate) under all four, so they sync only when no branch is sat out. |
| Dead Money, Honest Hearts, Old World Blues, Lonesome Road, Gun Runners' Arsenal | their master | All of each DLC, main line included. OWB's implant quest stays each player's own (`coop_joint::PER_PLAYER`). |
| Brotherhood of Steel (`bos`) | VMS55 (+a, b) | Still in the Dark, Tend to Your Business, Eyesight to the Blind. |
| Boomers (`boomers`) | VMS23, VMS15, VNellisJack, VNellisInfestation, NellisSunshineBoogie | Things That Go Boom, Volare!, Young Hearts, Ant Misbehavin', Sunshine Boogie. |
| Great Khans (`khans`) | VMS45, VMS46 | Oh My Papa, Aba Daba Honeymoon. |
| The Kings (`kings`) | VMS19, VMS29 | G.I. Blues, Kings' Gambit. |
| Omertas (`omertas`) | VMS21 (+a) | How Little We Know, Bye Bye Love. |
| White Glove Society (`white-glove`) | VMS18 | Beyond the Beef. |
| Powder Gangers (`powder-gangers`) | VMS02 | I Fought the Law. |
| Goodsprings (`goodsprings`) | VMS16 (+b) | Ghost Town Gunfight, Run Goodsprings Run. |
| Companions: Boone, Cass, Arcade, Veronica, Rex, ED-E | VMS53, VCassCompanion + VMS29a, VMS54, VMS49, VMS24, vDialogueEDE | One for My Baby, Heartache by the Number and Birds of a Feather, For Auld Lang Syne, I Could Make You Care, Nothin' But a Hound Dog, ED-E My Love. |

The names and editor IDs are [observed] (the masters' FULL and EDID). Which quest belongs to
which faction or companion is [hypothesis: from the Fallout Wiki's quest pages], to check
with `fnvtool coop questlines`; editing a row of `coop_questlines::LINES` is the whole change.
Raul and Lily have no quest record of their own named in the masters (their quests live in
`VDialogueRaul` / `vDialogueLily`, dialogue holders), so they have no line yet. Quests in
no line follow the main toggle only.

**An example.** Main story synced, Mr. House sat out: Ring-a-Ding-Ding! (Benny,
the Platinum Chip) is `main` and keeps syncing; the first House quest is a `house` root, so
from The Moon Comes Over the Tower / The House Always Wins, I on nothing is taken.

**The base-game main story is shareable.** Joint progression alone keeps it host only
(`coop_joint::MAIN_STORY`). With the questlines it is offered like any other quest, so two
players can play the main story together and keep the same decisions in their own saves;
a guest that wants it host only sits out `main` and the four branches.

## 3. What a sat-out line (or sync off) means

For a quest whose lines are sat out, or every quest while sync is off, the guest:

- claims no stage (the host leaves it out of the offer, and the guest turns down any that
  still comes: `StageResult` not taken), so its own save keeps its own stage;
- takes no quest XP, caps or hand-outs the host's quest brings (`Progress::Reward`,
  `Progress::Gift` naming that quest; with sync off, also the host's script XP and
  hand-outs that name no quest), and no "Quest completed / failed" line;
- keeps the quest in its Pip-Boy list as the host's world shows it (it plays that world);
  a quest of a sat-out line (sync on) has "Sitting out: not shared with your own save"
  under its objectives instead of the shared line.

**Change from before:** answering No to the join question used to stop only the stage
claims; the host's quest XP and caps still came. Off now means off for all quest
progress (the toggle's meaning). Kill XP and hit credit are not quest progress and still
come.

Stages already claimed before the choice stay in the ledger and replay at the next load
of the guest's own save: they happened while it shared.

## 4. The warning

Turning sync off, or sitting out a line, while one of the guest's shared quests is part
way through opens a confirm box. "Part way" ([`coop_questlines::in_progress`]): the
character's ledger has a claimed stage of the quest (waiting or replayed), and the quest
is not completed or failed in its own game nor ended in the host's. The box names those
quests (in the line, for a line) and says they stay at their current stage in the guest's
own save and stop following the host; turning sync back on later only picks a quest up
again if no stage was missed (`coop_joint::check` continuity). Confirm or Cancel.

## 5. What the host sees

`Progress::JointChoice` carries the lines sat out (a bit per row of `LINES`; all players run
the same build). It is sent at join and again whenever the guest changes either choice.
The host's lobby list shows each guest's state: "(sharing quests)", "(sharing quests,
sitting out Mr. House, NCR +2)" (two by name, so six players fit the screen), or "(not
sharing quests)". The host's offers skip a guest for a
quest it sits out, and no presence is recorded for it.

## 6. Shared world state: what the guest sees

The host is authoritative over the world the guest plays in, so a sat-out line does not
change what the guest sees there:

- **In the host's world** the guest sees the host's choices as they are: NPCs the host's
  path killed are dead, factions are as hostile or friendly as the host's world has them,
  dialogue follows the host's quest stages. A guest that sat out House still meets a
  Lucky 38 that is open to the host. Nothing hides or alters this: there is one world.
- **In the guest's own save** nothing of a sat-out line arrives: no stages, so no stage
  scripts run on replay (the scripts are what kill, move, or set factions in the original:
  `SetStage` results), no rewards. Its NPCs are alive and its factions as the guest left
  them. Reputation already follows this (`coop_joint` §3.6): the host's reputation stays in
  the host's world, and the guest's own acts' reputation is kept for its own save only while
  a claimed quest is open, which a sat-out quest never is.
- **The guest's own acts in a sat-out quest** (it kills an NPC the host's quest wants dead):
  the kill happens in the host's world (world state), XP from the kill comes as kill credit,
  and its own save does not get the stage that kill sets. [design: the most reasonable
  reading of "sit it out"; the alternative, rolling a sat-out quest's world effects back in
  the host's world, would break the host's game.]
- **Picking a line back up**: claims resume only where continuity holds, so a guest that sat
  out House for three stages is told the host went on without it and the quest stays its
  own (the existing "unshared" notice).
