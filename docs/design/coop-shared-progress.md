# Co-op shared progress (design)

How a co-op session's story progress, experience and rewards reach the players, and what
each player's own save keeps from it. NVForever's own design: the original has one player
[design]. It builds on the save groundwork in `coop-save.md` and on the world sync. Single
player is unchanged: nothing here runs without a co-op lobby (`Game::coop_game`,
`coop_session`, the `Role::Solo` checks).

Code: `fnv_game::coop` (the host's story as a guest reads it, story gate, checkpoints),
`coop_joint` (stage claims), `coop_prereqs`, `coop_rewards`, `coop_uniques`, `coop_kills`,
`coop_perks`, `coop_story`, `coop_services`, `story_guards`; the engine side is
`nvf_play::coop_progress`, `nvf_coop::story` and nvforever `coop/story.rs`.

## 1. Terms

- **Host world**: what lives only in the host's game and save (the world, quests, globals).
- **Character**: what travels with a player and lands in its own save (`coop::Character`).
- **Own world**: a guest's own game as its save had it when it joined. While the guest is in
  the host's world it does not run; its save keeps it (`nvf_play::save::KeepSolo`).

## 2. What is shared and what is each player's

| Part | Rule |
|---|---|
| Base-game main story | Host world. Claimable like a side quest; a guest that wants it host only sits out its questlines (`coop-quest-sync.md` §2). |
| Side quests, every DLC quest | Host world. With joint progression on, a guest that qualifies claims each stage the host sets (§3). |
| World state | Host world. |
| XP and level | Character: quest XP, script XP and a share of kill XP (§4.2). |
| Perks, skills, S.P.E.C.I.A.L. | Character. Perks the host's story gives a guest are taken back when it leaves, unless they are a unique reward of a claimed stage (§4.1). |
| Karma | Character: a kill moves the killer's karma. |
| Reputation | The host's applies in the host's world. A guest's own changes through the stages it claims and through its own acts inside those quests (§3.6). |
| Caps, inventory | Character. Unique rewards go only to claimers (§4.1); other story items are taken back when it leaves. |
| Map markers found | Host world, and also written into the guest's own save. |
| Challenges, achievements | Each player's own acts only; a claimed stage gives no achievement. |
| Clock, weather | Host world. |

## 3. Joint progression, per stage

### 3.1 The rules

A guest is asked once per join whether to share quest progress (the join question). A guest
claims a stage the host sets when all of these hold:

1. it said yes (its Quest Sync is on, `coop-quest-sync.md` §1);
2. it does not sit out a questline the quest belongs to (`coop-quest-sync.md` §2-3), and
   the quest is not one of each player's own (`coop_joint::PER_PLAYER`);
3. it is connected when the host sets the stage;
4. the quest's prerequisites hold in its own world (§3.3);
5. continuity: it has every earlier stage of the host's path, claimed or done in its own world.

Joining late fills in nothing already set.

### 3.2 Identity and presence

- A character has a stable random ID (`coop_joint::CharacterId`), made the first time it
  joins another player's game and sent with its join choice (`Progress::JointChoice`); a
  solo save never carries one. A slot number never names a character across sessions.
- The host's save keeps which characters were connected for each stage it set
  (`coop_joint::SessionPresence`, an optional field of the save, absent in a solo save). A
  guest that left and came back resumes claiming from it.
- The quest record has no main-story flag [observed: `schemas/fnv/gameplay.json` QUST], so the
  base-game main story is told by editor ID (`coop_joint::MAIN_STORY`, kept for the logs
  and tools) and its questlines by roots (`coop_questlines::LINES`).

### 3.3 Prerequisites, from the data

A side quest starts from a dialogue line or a script. The conditions on the line or the script
branch that starts it are the original's own prerequisites; `coop_prereqs` reads them from the
load order at first use, and `fnvtool coop prereqs` prints them for review. They are checked
against the guest's own world at its first claimed stage of a quest.

### 3.4 Missed stages

| Case | Result |
|---|---|
| Not connected for stage N, or sharing off | N is missed; continuity breaks for the later stages of that path unless the own world already did N. |
| Prerequisites fail at the first stage | Nothing of that quest is claimed this time. |
| A unique reward's own condition fails | The stage is claimed, the reward is not. |
| Own world already past the stage | Nothing is written; continuity holds. |
| Own world ended the quest | Nothing more of it is claimed. |

A stopped claim leaves the own world at the last claimed stage, which is a state the original
can be in, so the guest can go on solo from there.

### 3.5 Claims into the own world

Each claimed stage goes at once into the character's claim ledger
(`coop_joint::ClaimLedger`), so it is in the guest's save. When the character's own world next
loads (solo or hosting), `Game::replay_claims` applies the ledger before the first frame: each
stage in order, through the dialogue line that set it on the host when one did (its result
script carries choices no stage records), else `SetStage`, so the stages' own scripts make the
world match. Rewards given live are muted while it runs (`Game::coop_replaying`): RewardXP,
items and caps to the player, AddAchievement, ShowMessage, StartConversation. A SayTo says
nothing, but the line the speaker would say (chosen as a SayTo chooses it, its conditions read
in the guest's world) has its result scripts run at once, and so on through the SayTos they make,
at most `coop_joint::REPLAY_SAY_DEPTH` deep. That is how a companion a stage hands over is hired
in the guest's world: the King's line sets Nothin' But a Hound Dog's stage 10 and SayTos Rex,
and the line said then marks Rex available and has him SayTo the player the followers' hire
topic, whose line for Rex hires him [observed: FalloutNV.esm INFO result scripts and
conditions, `fnvtool script decode` and `dialogue lines`]. No list of lines is kept. One
line per quest shows ("From co-op: <quest>"). A ledger is never applied in another host's world.

### 3.6 Reputation and karma inside claimed quests

Stage scripts' reputation changes reach the own world through the replay. A guest's own acts
inside a quest it has an open claim on (a seen murder's infamy) are kept as a reputation change
in the ledger and replayed with the stages. Karma moves for the killer at the kill.

## 4. Rewards

### 4.1 Always and unique, from the data

- **Always** (every connected player): quest XP, script XP, caps, and items that are not unique.
- **Unique**: an item, perk or spell that only the quest's own scripts give and no other source
  in the load order provides (no leveled list, placed reference, container, inventory, other
  script or recipe). `coop_uniques` computes it from the load order at first use, so add-ons
  and mods are covered; `fnvtool coop uniques` prints it. A unique goes live only to guests that
  claimed the stage that gives it.

### 4.2 Kill credit

The killer keeps the kill's whole XP, as for the player's own kill in the original. Each other
player in the fight (the target fought it, or it hit the target within the share window) gets a
share. Karma and the kill count go to the killer; damage and hit challenges count each player's
own hits [design; the share and the window are numbers to tune in play].

## 5. Saving, leaving, coming back

| Case | What happens |
|---|---|
| A guest leaves, or the host ends the lobby | The guest's own game is saved with its character as it is now; the host's story grants are taken back except the uniques of claimed stages. The ledger and found markers are in that save. |
| The same guest rejoins | It is asked again; the host's presence record says which stages it was there for. |
| A guest joins another host | Only the character carries over; the ledger waits. |
| A guest plays solo later | Its own world loads and the ledger replays (§3.5). |

A guest's save never makes the host write a save of its own (`Progress::Save`, host checks in
`nvf_coop::guard`).

## 6. Story guards and the story gate

- **Story gate.** A host's talk with a story NPC waits until every other player is near it
  (`Game::story_gate`, `STORY_NEAR`); a script's own StartConversation is not held.
- **Story locks.** Only a story script's DisablePlayerControls (a quest's script or stage, a
  dialogue result, a trigger volume's script) holds the guests (`Game::story_locks`).
- **Story guards.** NVForever's fallbacks for scenes seen to stall with several players
  (`crate::story_guards`); they run only in a co-op game.
