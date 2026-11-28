//! The quest pilot (issue #2365): a headless walkthrough chain
//! (`crates/fnv-game/walkthrough/*.actions.json`, `fnv_game::runner`) played in the game
//! by the player character, to find what the headless run stands in for and a player
//! trips over. All of it is [design].
//!
//! `--pilot <chain> [--pilot-expect <trace>] [--pilot-out <report>]`, with `--play`.
//! Both may be given several times: the chains are played one after the other in the one
//! game, each compared with the trace at its place in the list.
//!
//! - **A campaign** (`--pilot-campaign`, with `--new-game`): the chains go on from the
//!   game's own state, as a player's do, instead of each from its `given` state. A
//!   chain's `given` is then a precondition: a quest entry is compared with the game and
//!   a difference kept as a `precondition` finding, never set; what a player would have
//!   had to earn or carry (actor values, items, skill points, tag skills, reputations,
//!   globals) is given, so the chain's own checks can pass. The first chain's `given` is
//!   left out altogether: the game is where New Game put it. A chain started from a thin
//!   `given` gave false alarms: Doc Mitchell, activated with his opening only marked
//!   done, started the opening again.
//! - `--pilot-given` with `--pilot-campaign`: the chains are played as a campaign plays
//!   them (the player's means everywhere), but each from its own `given` state, set as
//!   the headless run sets it. For a chain far into the game that no campaign has
//!   reached yet; what it finds carries the false alarms a thin `given` brings.
//! - **Scenes are watched**: NPCs near the player saying scripted lines to one another
//!   (SayTo, a conversation between them) stop its walk, up to [`SCENE_SECS`] a walk.
//!   At the Silver Rush's entrance a trigger turns the Van Graffs hostile on a player who
//!   walks on through Jean-Baptiste's scene, and Birds of a Feather fails.
//! - **An actor the player cannot walk to is waited for** (`Phase::Await`, up to
//!   [`AWAIT_SECS`]): it may come out and start the conversation itself, or come near
//!   enough to walk to. The Nellis gate guard is behind the locked gate and comes out to
//!   the player; Raquel then walks out to take the player to Pearl. A `move_player` to an
//!   actor ends there too, and is not asked the crosshair.
//! - **A line the headless run says for an actor** (`say_info` with a speaker) is, in a
//!   campaign, the player bringing that line about: the chains use it for what an actor
//!   says of its own accord, often at the end of a walk (Sunny goes behind the saloon
//!   before her line about the bottles: 2,774 units), and for the answer to the
//!   player's own pick (all of Boulder City Showdown is written so: Monroe's and
//!   Jessup's lines are what the player's choices get). In a conversation with the
//!   speaker the pilot picks the choice that has the actor say the line; when the line
//!   is further down, the choice from which the fewest picks lead to the line's topic
//!   (`choice_toward`, over the lines' Choices and Add Topics in the data; the lines
//!   between are said and their scripts run, as for a player who asks his way there).
//!   The game's rule for unattended runs, which leaves a conversation nobody scripted by
//!   its Goodbye after a second, is off while the pilot plays
//!   (`nvf_play::dialogue::set_piloted`): it left Monroe's before any pick. The player stays within
//!   [`NEAR_TARGET`] of the actor wherever it goes; once the actor has stood still for
//!   [`STILL_SECS`] s and said nothing, the player talks to it, once. The game picks
//!   the line. When the line named has not been begun [`LINE_SECS`] s on, a
//!   `line_not_said` finding is kept and the line said as the headless run says it.
//!   `say_first` and `say_to_done` are left to the game. A `trigger_enter` of another
//!   actor is the AI's to bring about, not the player's, and so is a `call` that moves
//!   another actor (`X.MoveTo`).
//! - **A save after each chain** of a campaign, `pilot-after-<chain>`, in NVForever's
//!   saves folder (run the pilot with `NVFOREVER_HOME` set to a folder of its own: the
//!   game's autosaves after travel go there too, and a run in the player's home
//!   overwrote the player's autosave slot). A later run goes on from one with `--load
//!   pilot-after-<chain> --pilot <the next chains> --pilot-campaign`, so the chains
//!   before it need not be played again. The next chain begins [`SAVE_SECS`] s later.
//! - **A terminal is used as a player uses it** [owner, 2026-11-13: the Stealth Boy
//!   shipment in REPCONN's basement is read off a terminal]. A `terminal` step was the
//!   headless call until then: no walk, no screen, no menu. Now the player walks to the
//!   terminal, activates it (the crosshair is asked), and works its screen through the
//!   screen's own input (`nvf_play::terminal::TerminalInput`): on the hacking page it
//!   enters the password (a stand-in for the word game: the game still refuses a
//!   terminal beyond the player's Science, and a locked-out one stays so); in the menu
//!   it takes the entry the step names, first going into the sub menu the step names
//!   (the listed entry that opens it) or back out of another one. An entry not listed,
//!   a sub menu no listed entry opens, a screen that does not come up in
//!   [`TERMINAL_SECS`] s: the step fails and says which. The terminal is left (Exit)
//!   when a step that is no `terminal` step follows. Not yet: a terminal found by the
//!   pilot itself for a locked door.
//! - **Quest items are picked up** [owner, 2026-11-14]. After a fight the player goes to
//!   the body and takes what it carries that is a quest item (the record's flag, or a
//!   script's SetQuestObject) or a key. An `add_item` to the player, which in the chains
//!   stands for the player finding something, is in a campaign the player getting it:
//!   already carried, nothing to do; else from whoever in the cells loaded holds it (a
//!   body, a container) or where it lies, walked to and taken; found nowhere, an
//!   `item_not_found` finding and the headless gift, so that the chain goes on.
//! - **The player waits** [owner, 2026-11-14: some stages only go on at certain hours].
//!   A chain's `X.MoveTo marker` stands for an actor arriving of its own accord, often
//!   by the clock (Beyond the Beef: "waiting until 4 pm and Chauncey's walk to the steam
//!   room"). In a campaign the player goes to that marker and, while the actor is not
//!   within [`ARRIVED`] units of it, waits an hour at a time on the game's own Wait menu
//!   (`nvf_play::wait`), up to [`WAIT_HOURS`] hours. An actor that has not come by then
//!   is a `never_came` finding (issue #2445 is one), and the headless MoveTo is done so
//!   that the chain goes on. A wait the game refuses (enemies near) is tried again.
//!   A call whose `label` begins `script:` is a script's own MoveTo, done at once.
//! - **Errands** are what the pilot does of its own accord between a chain's steps: a
//!   body to search, and a terminal that opens a locked door (an entry whose result
//!   script names the door: walked to, opened, the entry taken). The step's own time
//!   stands still meanwhile.
//! - **Save and load at milestones** (`--pilot-save-load`): before each labelled
//!   milestone (`milestone…`, `final…`) is read, the game's state (`fnv_game`'s snapshot,
//!   not the engine's) goes through a save and a load, as the headless `save_load` step
//!   does; what a save loses then shows as a divergence. The drawn world is not rebuilt
//!   from it: a reference the load moved is drawn where it was until it moves again.
//!   A finding `save_load` when the round trip fails.
//! - **Known differences** (`--pilot-known <file>`, the batch's
//!   `tools/smoke/pilot/known-differences.json`): a list of `{chain, label, reason}`; a
//!   labelled read that differs from the headless one and is on it is reported apart
//!   (`known_differences`), not as a divergence. Each entry says why it is no bug.
//! - **What is still done the headless way** is counted, by kind of step, in the
//!   report's `headless` (`pilot: headless: <op>` in the log): steps with no player
//!   means yet (`set_stage`, `set_quest_var`, `enable`, `hit`, ...), and every
//!   fallback named above. A chain whose count is high tests less than it seems to.
//! - Menus: `wait_menu`, `get_menus`, `close_menus`, `press_button` and
//!   `dismiss_message_boxes` act on the game's menu as the headless run does. A message
//!   box no step is waiting for is answered after [`BOX_SECS`] s (headless such a box
//!   holds nothing up): one with a single button is closed, as a player presses OK; of
//!   several buttons the last one shown is picked, which is the way on in the boxes
//!   seen so far (leaving Goodsprings: "Edit Name", "Rebuild Character", "Finished -
//!   Travel Onward"; the first button there opened the name menu again and again).
//!
//! - **The player is driven like an NPC and acts directly.** It walks the navmesh paths
//!   the NPCs walk (`fnv_nav`, through doors) on its own movement controls, so the
//!   script's control locks hold it as they hold the keys. It talks, activates and
//!   chooses through the calls the E key and the dialogue screen end in, with no key
//!   pressed [owner decision 2026-11-13].
//! - **Could a real player have done it?** Before it talks to or activates something it
//!   stands where a player would, turns the view onto it and asks the game's own
//!   crosshair (`nvf_play::activate::Target`). When the crosshair does not take it, the
//!   step goes on and a `could_not_target` finding is kept [owner decision 2026-11-13].
//! - **What the headless run stands in for is left to the game.** `package`,
//!   `patrol_point` and `dialogue_package` (an actor's package made to arrive) are not
//!   carried out: the AI has to do it. `wait_talk` with `start` waits for the actor to
//!   come and greet.
//! - **The headless run is the expected result.** With `--pilot-expect` (the trace
//!   `fnvtool script run --actions <chain>` prints) every labelled read is compared with
//!   the headless one of the same step; a `get_quest` that differs is read again for
//!   [`MILESTONE_SECS`] (the game needs real time for what the headless run does in a
//!   frame) and then kept as a divergence.
//! - **The player is never put where it could not go** [owner, 2026-11-13: the pilot
//!   had put itself beside Chris Haversam behind REPCONN's locked door, and so past the
//!   intercom that unlocks it]. With no way to a place (a locked door the player has no
//!   key for, no navmesh), or a door that does not take the player through, the step
//!   fails with an `unreachable` finding and the player stays where it is: a later step
//!   walks from there, by when the game may have opened the way. The paths know the
//!   doors scripts have locked and unlocked, the keys the player carries and the doors
//!   standing open. Only a walk that stops within [`PUT_WITHIN`] units of its goal, in
//!   the same space, ends with the player set on the goal (the pilot's own clumsiness
//!   round furniture, a `stuck` finding).
//! - **Up to a locked door, and what a player tries there.** When the only way is
//!   through a locked door, the player walks as far as it can, to that door, and tries
//!   it. When it stays locked the player uses what opens it, once each, at most
//!   [`SWITCHES`] a door [owner, 2026-11-13]:
//!   1. a reference of the door's cell whose script has an OnActivate block and names
//!      the door (the switch the data says works it), up to [`SWITCH_RANGE`] units off;
//!   2. a talking activator within [`BESIDE_DOOR`] units: REPCONN's research labs are
//!      opened by the intercom beside their door (REPCONMidToTopDoorIntercom), which
//!      has no trigger and answers only the player's own activation (its OnActivate sets
//!      the flag Chris's "I'm letting you in" greeting is conditioned on, and that
//!      line's end script unlocks the door); no step of the headless chain goes near it;
//!   3. any other reference within [`BESIDE_DOOR`] whose script has an OnActivate block
//!      and that is no actor: the button a player would press.
//!
//!   One further off than [`NEAR_TARGET`] is walked to first; a nearer one is used from
//!   where the player stands (the crosshair is not asked). Not yet: a terminal that
//!   opens a door (its menu would have to be worked). A conversation that begins on
//!   such a way is heard out and left by its Goodbye, and the walk goes on with a new
//!   path (the door may be open now). A door that stays locked [`LOCKED_SECS`] s after
//!   it was tried is left, and the next way round is tried (the shortest way to
//!   REPCONN's labs is through a door with a lock to pick, which the pilot does not).
//!   With no way left the step ends with an `unreachable` finding that names each door
//!   tried, its lock level and its key.
//! - A long way out of doors is cut short: when a path is over [`WALK_MAX`] units, the
//!   player starts [`WALK_TAIL`] units before the end of the path's last stretch out of
//!   doors (a stand-in for fast travel, logged). What lies indoors after that, and every
//!   door on the way, is walked: no interior is skipped into.
//! - **A gap no navmesh link joins inside one interior is stepped across** when the two
//!   walkable areas lie within [`STEP_ACROSS`] units of each other on the ground and
//!   [`STEP_RISE`] in height: the player walks to the near end and then straight over on
//!   its own movement, checked to arrive within [`STEP_ACROSS_SECS`] (else `unreachable`).
//!   SSHQ01's prize room is a navmesh of its own whose stair top lies 17 to 31 units from
//!   the main one [observed, #3473]; the original's followers do not cross there (observer
//!   check 279), a player walks the stairs.
//! - **A conversation is left as a player leaves it** [owner, 2026-11-13; #2157: a
//!   Goodbye line holds the player until it is said, and a player walking off mid-line
//!   broke result scripts]. The pilot never breaks a conversation off (it did, with
//!   `Game::end_dialogue`, and REPCONN's intercom never ran the end script that unlocks
//!   the door). To leave, it hears the line out, picks the Goodbye choice and hears that
//!   line out too; on a topic list with no Goodbye it leaves as Tab does (#1595). A
//!   conversation offering choices, none of them a Goodbye, and not a topic list, has no
//!   way out for a player: a `no_way_out` finding, and only then is it broken off so
//!   that the chain goes on. `end_talk` is this too.
//! - A conversation that begins while the player walks: where the steps that follow
//!   deal with it (a `trigger_enter`, an actor kept company, a walk followed by
//!   `wait_talk`, `choose` or `talk`), the walk ends there and they hear it out
//!   (REPCONN's lobby intercom, as its trigger is entered). Anywhere else nothing in the
//!   chain answers it: the player goes along with it once (its first choice that is no
//!   Goodbye: Simon's search at the Silver Rush door lets the player in), then leaves and
//!   walks on.
//! - Lines are skipped after [`SKIP_AFTER`] s, as a player in a hurry does. A conversation
//!   still open when the next step is a walk is left first, unless the walk is to the
//!   actor talking; an actor that came and spoke first is the conversation a `talk`
//!   step asks for.
//! - **`kill` is a fight** [owner decision 2026-11-13]: the player walks to within
//!   [`FIGHT_RANGE`] of the actor, draws ([`PILOT_WEAPON`], given once with its ammo),
//!   keeps the view on the actor's trunk and pulls the trigger until it is dead, closer
//!   after [`FIGHT_CLOSER_SECS`] in which its shots did not lower the actor's health
//!   (hits landing: it keeps its distance, as from the Chimera tank's shells). The shots are the player's own (`nvf_play::combat`):
//!   the weapon's cycle, spread, damage and the target's armour decide. An actor still
//!   alive after [`FIGHT_SECS`] is a `fight_not_won` finding and is killed as the
//!   headless run kills it, so the chain goes on. The line of fire is the NPCs' own
//!   judgement (`combat_tactics::shot_clear`: the eye to the target's trunk, else its
//!   head): with none for [`NO_SHOT_SECS`] s the player goes nearer, by a third each
//!   time, instead of firing into what is in the way. Someone standing between the player
//!   and its target is not shot through: the player steps aside until the line is free. Not yet: the NPCs' reach by
//!   weapon and their cover (`npc_combat::gun_reach`, `combat_tactics::find_cover`),
//!   melee.
//! - **The player defends itself and whoever it is with.** An actor in combat whose
//!   target is the player, or the actor the step walks to or keeps company, within
//!   [`THREAT_RANGE`], is fought first, as a `kill` step fights, and the step goes on
//!   afterwards with its time not counted. Without it Sunny, kept company to the second
//!   well, was beaten to death by three geckos while the player stood by and waited for
//!   her next line. A threat alive after [`DEFEND_SECS`] is a `fight_not_won` finding.
//! - **The player levels up.** The level-up menu holds the game until it is answered
//!   (in the first Primm run it came up on arrival and every step behind it timed out):
//!   after [`BOX_SECS`] the pilot spends the level's skill points evenly down the list,
//!   takes the first perk on offer at a perk level, and presses Done
//!   (`fnv_game::levelup`, `nvf_play::chargen::finish`). Any other menu the scripts
//!   opened that no step waits for and that is still up after [`MENU_SECS`] is closed,
//!   with a `menu_closed` finding.
//! - **The weapon goes up as a fight lasts** ([`ARMS`]): the 9mm pistol, a riot
//!   shotgun after 20 s, an anti-materiel rifle after 45 s, each with its ammunition.
//! - **The weapon is kept in repair.** God mode keeps the magazine full but not the
//!   weapon whole: some 700 shots into a campaign the pistol broke, the game unequipped
//!   it and the player fought on with its fists. Every [`WEAPON_SECS`] s of a fight the
//!   pilot sets the condition of what the player holds to full, and a player found
//!   without the pilot's weapon in hand gets a new one and equips it (unless
//!   `--pilot-no-god`: then wear is part of what the run asks).
//! - **God mode is on** unless `--pilot-no-god` [owner decision 2026-11-13]: the player
//!   takes no damage and its magazine stays full (`nvf_play::combat::set_god_mode`). The
//!   pilot looks for quests that cannot go on, not for fights that cannot be won; a run
//!   without it says whether a fight can be.
//! - A `kill` whose killer is another actor is that actor's fight: the player waits up to
//!   [`AI_KILL_SECS`] for the death (`ai_kill_not_done` otherwise, and the headless kill).
//! - Not yet: `hit` (the script call of the headless run) and the samplers
//!   (`not_carried_out` findings).
//!
//! **Limits.** No step lasts longer than [`STEP_SECS`]: it is given up with a
//! `step_timed_out` finding and the chain goes on. A chain with [`FAILED_IN_A_ROW`]
//! failed steps one after the other is abandoned (`chain_abandoned`): what is left of it
//! would only fail the same way; a campaign goes on with the next chain. So is a chain
//! whose player dies (`player_died`): a player would load a save there. The pilot's
//! clock runs only while it can act (in play, in a conversation, in a menu the scripts
//! opened); when it has not been able to for [`HELD_SECS`] s of real time (a menu of the
//! engine's own, a loading screen that does not end), the run ends with a `pilot_held`
//! finding and its report, as it does at `--run-seconds`.
//!
//! Log: `pilot: step <n> <op> ...`, `pilot: finding <kind>: ...`, `pilot: divergence at
//! step <n> ...`, and at the end `pilot: done: <n> steps, <d> divergences, <f> findings,
//! <s> soft locks`, after which the game exits. The report (`fnv.pilot.report/1`) holds
//! the same.

use bevy::prelude::*;
use fnv_game::game::Game;
use fnv_game::runner::{self, Action, Applier};
use fnv_nav::follow::{heading_delta, heading_of, turn_towards};
use fnv_nav::{Follower, Place, Space, Steer};
use fnv_sim::FormId;
use nvf_play::activate::Target;
use nvf_play::control::{ControlLocks, Crosshair, PlayerControl};
use nvf_play::loading::{Loading, TravelRequest};
use nvf_play::phys::PhysState;
use nvf_play::player::{MoveIntent, Player};
use nvf_play::script::{with_world, ByHand, ScriptState};
use nvf_play::sim::SimState;
use nvf_play::{Location, PlaySet, PlayState, Travel};
use serde_json::{json, Map, Value as Json};
use std::path::PathBuf;

pub const REPORT_FORMAT: &str = "fnv.pilot.report/1";
/// A path longer than this is cut to its last [`WALK_TAIL`] units.
pub const WALK_MAX: f32 = 12_000.0;
pub const WALK_TAIL: f32 = 3_000.0;
/// Seconds a differing `get_quest` is read again before it is a divergence.
pub const MILESTONE_SECS: f64 = 45.0;
/// How many picks down a conversation the pilot looks for a line.
pub const PICKS_DEEP: usize = 5;
/// Seconds a conversation with no way out is waited on before the finding.
pub const NO_WAY_OUT_SECS: f64 = 5.0;
/// Picks made in a conversation with no Goodbye before it counts as having no way out:
/// Ulysses' talks through ED-E in Lonesome Road take 13 and more (sweep 2 [observed]).
pub const TALK_PICKS: u8 = 24;
/// Seconds of a line the pilot hears before it skips it.
pub const SKIP_AFTER: f64 = 0.4;
/// Seconds before the first step: the world has loaded and settled.
const SETTLE_SECS: f64 = 3.0;
/// How near the player stands to what it talks to or activates, and to a marker.
const NEAR_TARGET: f32 = 100.0;
const NEAR_MARKER: f32 = 48.0;
/// How far from a locked door a talking activator or a button is tried.
pub const BESIDE_DOOR: f32 = 400.0;
/// How far from a locked door a switch whose script names it is looked for, and how many
/// things are tried for one door.
pub const SWITCH_RANGE: f32 = 4000.0;
pub const SWITCHES: usize = 3;
/// Seconds a locked door on the way is waited at, after the player tried it.
pub const LOCKED_SECS: f64 = 8.0;
/// Seconds after a fast-travel stand-in that the player's teammates are brought along.
pub const FOLLOWERS_AFTER: f64 = 2.0;
/// How far a door moves the player, within its own space, to count as gone through.
pub const DOOR_JUMP: f32 = 500.0;
/// How near its goal a walk that got stuck may end with the player set on the goal.
pub const PUT_WITHIN: f32 = 400.0;
/// Seconds without [`STUCK_UNITS`] of movement on a walk before a new path is planned.
const STUCK_SECS: f64 = 4.0;
const STUCK_UNITS: f32 = 24.0;
/// A walk held up this close to the middle of the load door its leg ends at uses the door
/// from where it stands, game units [design: half the Northern Passage door's 512-unit width
/// plus a step].
const DOOR_PRESS_REACH: f32 = 320.0;
const REPLANS: u8 = 3;
/// Hops along its own path a stuck walk may take, and how far each.
const SKIPS: u8 = 3;
const SKIP_UNITS: f32 = 200.0;
/// How far on from where it stood stuck the player has got away from it.
const UNSTUCK_UNITS: f32 = 300.0;
/// The heights above a reference's origin the view is turned to, one after the other,
/// to find it under the crosshair.
const AIM_HEIGHTS: [f32; 4] = [80.0, 40.0, 110.0, 8.0];
const AIM_SECS: f64 = 0.25;
/// The longest any one step lasts, seconds: then it is given up, whatever it waits for.
pub const STEP_SECS: f64 = 240.0;
/// How far away an item an `add_item` step wants is gone for; past it, given as the
/// headless run gives it.
pub const ITEM_FAR: f32 = 12000.0;
/// Failed steps one after the other that end a chain.
pub const FAILED_IN_A_ROW: u32 = 12;
/// Real seconds without the pilot being able to act that end the run.
pub const HELD_SECS: f64 = 120.0;
/// The longest any one wait lasts, seconds: under [`STEP_SECS`]. Honest Hearts' intro
/// slides set the arrival stage (NVDLC02MQ00 100) 140 s after the slide room's trigger.
const WAIT_CAP: f64 = 220.0;
const ADVANCE_CAP: f64 = 20.0;
const TURN_RATE: f32 = 6.0;
/// Seconds a terminal's screen may take to come up, and between two inputs to it.
pub const TERMINAL_SECS: f64 = 15.0;
pub const TERMINAL_INPUT_SECS: f64 = 0.6;
/// How near its marker an awaited actor has arrived, and the hours waited for it.
pub const ARRIVED: f32 = 500.0;
pub const WAIT_HOURS: u32 = 24;
/// How long the walk to a beeping speaker may take before it is switched off as a stand-in
/// (a player shoots one out of reach): the collar goes off 10 s after the beeping starts
/// (DeadMoney.esm GLOB NVDLC01RadiusTimer 10.0 [observed]). Standing by a locked door 4
/// units short of a speaker the player died at Christine's floor (round 22); walked back
/// to a quiet spot instead, the way back lay behind a locked door too (round 23 [observed]).
pub const SILENCE_SECS: f64 = 5.0;
/// How long after walking out of an unswitchable speaker's range the player does not do it
/// again: going back in, the collar's 10 s start afresh and the step's walk crosses it.
pub const AWAY_AGAIN_SECS: f64 = 30.0;
/// Seconds an errand may take.
pub const ERRAND_SECS: f64 = 90.0;
/// Seconds a message box nobody waits for stays up before the pilot closes it.
pub const BOX_SECS: f64 = 1.5;
/// How far off NPCs playing a scene stop the player, and for how long at most a walk.
pub const SCENE_RANGE: f32 = 1500.0;
pub const SCENE_SECS: f64 = 90.0;
/// Seconds without a line that end a scene being watched.
pub const SCENE_GAP: f64 = 20.0;
/// Fewer navmesh nodes than this expanded from the start of a failed search: the player
/// stands on a scrap of navmesh, not in a place a way leads out of.
pub const STRANDED_EXPANDED: usize = 16;
/// Seconds an actor the player cannot walk to is waited for, and how many times the walk
/// is tried again when it comes near.
pub const AWAIT_SECS: f64 = 60.0;
pub const AWAIT_TRIES: u8 = 3;
/// Seconds the next chain waits for the save after the one before it.
pub const SAVE_SECS: f64 = 3.0;
/// Seconds a menu the pilot has no answer for stays up before it is closed.
pub const MENU_SECS: f64 = 20.0;
/// Seconds in which the line a `say_info` names must be begun, the player keeping its
/// speaker company.
pub const LINE_SECS: f64 = 120.0;
/// Within this of the speaker the player counts as beside it for [`LINE_SECS`], game
/// units [design].
pub const LINE_NEAR: f32 = 500.0;
/// Seconds the speaker has stood still, silent, before the player talks to it.
pub const STILL_SECS: f64 = 6.0;
/// The weapon and ammo the pilot fights with (Editor IDs of FalloutNV.esm's records).
pub const PILOT_WEAPON: &str = "WeapNV9mmPistol";
pub const PILOT_AMMO: &str = "Ammo9mm";
/// The weapons a fight goes up to as it lasts: (seconds into the fight, weapon, ammo).
/// The pilot tests quests, not the player's build: a Nightkin outlived 75 s of a 9mm
/// pistol in REPCONN's basement and the fight was settled the headless way.
pub const ARMS: [(f64, &str, &str); 3] = [(0.0, PILOT_WEAPON, PILOT_AMMO), (20.0, "WeapNVRiotShotgun", "Ammo12Ga"), (45.0, "WeapNVAntiMaterielRifle", "Ammo50MG")];
/// How near the player goes to shoot, then nearer when the target still lives.
pub const FIGHT_RANGE: f32 = 700.0;
pub const FIGHT_NEAR: f32 = 220.0;
pub const FIGHT_CLOSER_SECS: f64 = 6.0;
/// The farthest the player shoots from where it stands without walking nearer: a 9mm
/// pistol's spread missed Joe Cobb with every one of 25 shots from the saloon's door.
pub const SHOOT_FROM: f32 = 450.0;
/// The longest shot the player takes at a target no walk reaches, game units [design].
pub const LONG_SHOT: f32 = 6000.0;
pub const FIGHT_SECS: f64 = 75.0;
/// Seconds a kill by another actor is waited for.
pub const AI_KILL_SECS: f64 = 60.0;
/// Seconds between looks at the weapon in a fight.
pub const WEAPON_SECS: f64 = 2.0;
/// Seconds without a clear line of fire before the player goes nearer, and how near at
/// the least.
pub const NO_SHOT_SECS: f64 = 1.5;
pub const FIGHT_NEAREST: f32 = 120.0;
/// How far off an attacker is fought, and for how long at most.
pub const THREAT_RANGE: f32 = 2500.0;
pub const DEFEND_SECS: f64 = 60.0;

pub struct PilotPlugin {
    pub chains: Vec<PathBuf>,
    pub expect: Vec<PathBuf>,
    pub out: Option<PathBuf>,
    pub campaign: bool,
    /// With `campaign`: each chain's given state is set, not checked.
    pub set_given: bool,
    pub god: bool,
    /// Differences from the headless runs known not to be bugs (a JSON list of
    /// `{chain, label, reason}`).
    pub known: Option<PathBuf>,
    /// The game's state goes through a save and a load before each milestone is read.
    pub save_load: bool,
    /// Minutes of real time after which the run ends, report written.
    pub minutes: Option<f64>,
    /// The first chain begins at this step (`--pilot-from`).
    pub from: Option<usize>,
}

impl Plugin for PilotPlugin {
    fn build(&self, app: &mut App) {
        let loaded: Result<Vec<Chain>, String> = self.chains.iter().enumerate().map(|(k, c)| Chain::load(c, self.expect.get(k).map(PathBuf::as_path)).map_err(|e| format!("{}: {e}", c.display()))).collect();
        match loaded {
            Ok(chains) if !chains.is_empty() => {
                for c in &chains {
                    info!("pilot: chain {}: {} steps{}", c.name, c.actions.len(), if c.expect.is_empty() { "" } else { ", compared with a headless trace" });
                }
                if self.god {
                    nvf_play::combat::set_god_mode(true);
                    info!("pilot: god mode on: the player takes no damage and its magazine stays full (--pilot-no-god for a run without)");
                }
                if self.campaign {
                    info!("pilot: a campaign: the chains go on from the game's own state; their given states are preconditions");
                }
                nvf_play::dialogue::set_piloted(true);
                let mut pilot = Pilot::new(chains, self.out.clone(), self.campaign);
                pilot.god = self.god;
                pilot.set_given = self.set_given;
                pilot.save_load = self.save_load;
                pilot.deadline = self.minutes.map(|m| m * 60.0);
                if let Some(from) = self.from {
                    info!("pilot: the first chain begins at step {from} (--pilot-from)");
                    pilot.i = from;
                }
                if let Some(path) = &self.known {
                    match std::fs::read_to_string(path).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str::<Json>(&t).map_err(|e| e.to_string())) {
                        Ok(Json::Array(list)) => pilot.known = list,
                        Ok(_) => error!("pilot: {}: expected a list of known differences", path.display()),
                        Err(e) => error!("pilot: {}: {e}", path.display()),
                    }
                }
                app.insert_resource(pilot).add_systems(Update, (drive.in_set(PlaySet::Input).after(nvf_play::control::read_controls).before(nvf_play::control::enforce_locks), held, close_engine_menus, out_of_time)).add_systems(Last, report_on_exit);
            }
            Ok(_) => error!("pilot: no chain"),
            Err(e) => error!("pilot: {e}"),
        }
    }
}

/// One chain to play.
struct Chain {
    name: String,
    title: String,
    /// The plugins the chain's FormIDs count from (its `plugins`), and the file as read,
    /// to be read again with the FormIDs moved to the run's load order (`remap_chain`).
    plugins: Vec<String>,
    raw: Json,
    given: Vec<Json>,
    actions: Vec<Action>,
    ops: Vec<String>,
    expect: std::collections::HashMap<usize, Json>,
}

impl Chain {
    fn load(chain: &std::path::Path, expect: Option<&std::path::Path>) -> Result<Chain, String> {
        let text = std::fs::read_to_string(chain).map_err(|e| e.to_string())?;
        let raw: Json = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        let input: runner::Input = serde_json::from_value(raw.clone()).map_err(|e| e.to_string())?;
        if input.format != runner::ACTIONS_FORMAT {
            return Err(format!("format {:?}, want {:?}", input.format, runner::ACTIONS_FORMAT));
        }
        let ops = raw["actions"].as_array().map(|a| a.iter().map(|x| x["op"].as_str().unwrap_or("?").to_string()).collect()).unwrap_or_default();
        let mut expected = std::collections::HashMap::new();
        if let Some(p) = expect {
            let t: Json = serde_json::from_str(&std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?).map_err(|e| format!("{}: {e}", p.display()))?;
            for o in t["observations"].as_array().into_iter().flatten() {
                if let Some(step) = o["step"].as_u64() {
                    expected.insert(step as usize, o.clone());
                }
            }
        }
        let name = chain.file_name().map(|n| n.to_string_lossy().trim_end_matches(".actions.json").to_string()).unwrap_or_default();
        Ok(Chain { name, title: input.title.clone().unwrap_or_default(), plugins: input.plugins.clone(), raw: raw.clone(), given: raw["given"].as_array().cloned().unwrap_or_default(), actions: input.actions, ops, expect: expected })
    }
}

/// A chain whose `plugins` count load-order indices otherwise than the run (Honest
/// Hearts is 01 in its chains, 02 in a game with Dead Money before it): its FormIDs
/// (`0x01xxxxxx`, and bare `01XXXXXX` in console lines) are moved to the run's indices
/// and the chain read again.
fn remap_chain(p: &mut Pilot, order: &[String]) {
    let map: Vec<Option<u8>> = p.chain_plugins.iter().map(|name| order.iter().position(|o| o.eq_ignore_ascii_case(name)).and_then(|i| u8::try_from(i).ok())).collect();
    if map.iter().enumerate().all(|(i, m)| *m == u8::try_from(i).ok()) {
        return;
    }
    let raw = remap_json(&p.chain_raw, &map);
    match serde_json::from_value::<runner::Input>(raw.clone()) {
        Ok(input) => {
            p.actions = input.actions;
            p.given = raw["given"].as_array().cloned().unwrap_or_default();
            let moved: Vec<String> = p.chain_plugins.iter().zip(&map).enumerate().filter(|(i, (_, m))| **m != u8::try_from(*i).ok()).map(|(i, (n, m))| format!("{n} {i:02X} → {}", m.map_or("not loaded".to_string(), |m| format!("{m:02X}")))).collect();
            info!("pilot: chain {}: its FormIDs moved to this load order: {}", p.chain, moved.join(", "));
        }
        Err(e) => error!("pilot: chain {}: its FormIDs moved to this load order do not read: {e}", p.chain),
    }
}

fn remap_json(v: &Json, map: &[Option<u8>]) -> Json {
    match v {
        Json::String(s) => Json::String(remap_ids(s, map)),
        Json::Array(a) => Json::Array(a.iter().map(|x| remap_json(x, map)).collect()),
        Json::Object(o) => Json::Object(o.iter().map(|(k, x)| (k.clone(), remap_json(x, map))).collect()),
        other => other.clone(),
    }
}

/// Each 8-hex-digit word (with or without `0x`) whose top byte is a chain plugin's index
/// gets the run's index of that plugin.
fn remap_ids(s: &str, map: &[Option<u8>]) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        let word = |at: usize| at + 8 <= b.len() && b[at..at + 8].iter().all(u8::is_ascii_hexdigit) && b.get(at + 8).is_none_or(|c| !c.is_ascii_alphanumeric());
        let starts = i == 0 || !b[i - 1].is_ascii_alphanumeric();
        let at = if starts && b[i..].starts_with(b"0x") { i + 2 } else { i };
        if starts && word(at) {
            let id = u32::from_str_radix(&s[at..at + 8], 16).unwrap_or(0);
            let top = (id >> 24) as usize;
            out.push_str(&s[i..at]);
            match map.get(top).copied().flatten() {
                Some(m) if top > 0 => out.push_str(&format!("{:08X}", (u32::from(m) << 24) | (id & 0x00FF_FFFF))),
                _ => out.push_str(&s[at..at + 8]),
            }
            i = at + 8;
        } else {
            let c = s[i..].chars().next().expect("in the string");
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

/// Something the pilot does of its own accord (module notes).
#[derive(Debug, Clone, PartialEq)]
enum Errand {
    /// Search a body for quest items and keys.
    Search(FormId),
    /// Take an item from a holder, or where it lies (`from` is then the item's own
    /// reference).
    Take { from: FormId, item: FormId, count: i32, lying: bool },
    /// Use the entry of a terminal that works a locked door.
    Terminal { reference: FormId, base: FormId, item: usize, door: u32 },
    /// Switch off a speaker whose noise sets the player's collar counting down: Dead
    /// Money's radios and speakers ([`beeping_speaker`]).
    Silence(FormId),
    /// Walk out of the range of a speaker no player can switch off (the Tampico's
    /// invisible ones, destroyed at start: [`beeping_speaker`]): to a point past its radius.
    Away(FormId),
}

/// A walk to a place.
struct Walk {
    goal: Place,
    /// The reference walked to: a new path when it has moved.
    target: Option<FormId>,
    radius: f32,
    follower: Option<Follower>,
    /// Since when the door at the end of the leg, or the one in the way, is waited for.
    door_since: Option<f64>,
    /// Where the player stood when it went through the door at the end of the leg: a load
    /// door into the same space (the Strip's gates) shows only as a jump from here.
    door_from: [f32; 3],
    /// Where the player was, and when, for the stuck rule.
    seen: (f64, [f32; 3]),
    replans: u8,
    cut: bool,
    /// Whether this walk was already put past a gap no navmesh joins (once a walk: a
    /// second time is the same gap, Paradise Falls' 1,853 puts in a row).
    gapped: bool,
    /// Whether a conversation was open when the walk began (`None` until its first frame).
    talking_at_start: Option<bool>,
    /// A conversation that begins on the way ends the walk: the steps after it deal with
    /// it. Else it is left by its Goodbye and the walk goes on.
    keep_talk: bool,
    /// The path goes through a locked door the player cannot open: walked up to it.
    locked_way: bool,
    /// Locked doors tried and left: the next path keeps off them. With what each said.
    tried: Vec<(u32, String)>,
    /// Hops taken along the path when stuck.
    skips: u8,
    /// Times the actor walked to was waited for (`Phase::Await`).
    awaited: u8,
    /// Where the player last stood stuck: [`UNSTUCK_UNITS`] on from there the count of
    /// new paths starts again (a long walk with fights on it got stuck three times in
    /// three places and was given up).
    stuck_at: Option<[f32; 3]>,
    /// What was used to open a locked door, with the door.
    poked: Vec<(u32, u32)>,
    /// An errand the walk asks for (a terminal that works a door): the pilot takes it.
    errand: Option<(Errand, Place)>,
    /// A switch being walked to, for the door, before the walk goes on.
    side: Option<(FormId, u32, Box<Walk>)>,
    /// Since when a door not in the world, its switch used, is waited for (a lift's ride).
    enable_wait: Option<f64>,
    /// Since when the walk has waited at a locked door for its owner to open it.
    owner_wait: Option<f64>,
    /// A walk to a spot to fire from: a gap no navmesh joins inside one interior is not
    /// crossed by a put, the player fires from where it stands (the BB gun range: put by
    /// target 1, the player was where Dad could not walk up to talk, CG02 stopped at 55).
    stay: bool,
    /// With `stay`, in a fight: the player keeps where it is and the foe comes to it (a
    /// shot from afar hit Dad, who stood between the player and the radroach).
    hold: bool,
    /// Put back once where a path was last found (`Pilot::walked_from`).
    stranded: bool,
    /// A step from one walkable area of an interior onto another that no navmesh link
    /// joins, as a player walks it (SSHQ01's stairs, #3473): the walk to its near end,
    /// then straight across.
    step: Option<StepAcross>,
    /// Steps across taken on this walk ([`STEPS_ACROSS`] at most).
    steps: u8,
    /// The locked door whose owner the player keeps by, while it does.
    owner_door: Option<u32>,
    /// When the walk last said where the player is on its path (every [`WALK_REPORT_SECS`]).
    reported: f64,
    /// Since when a travel a script asked for has been waited for.
    travel_since: Option<f64>,
    /// Seconds stood watching scenes on this walk, and when a scene's line was last seen.
    scene_held: f64,
    scene_last: Option<f64>,
    /// Said that the actor walked to is talking (once a conversation).
    told_talking: bool,
    /// Stepping aside (to this side, until then) round what the player walked into.
    dodge: Option<(f64, f32)>,
    /// The end of the planned path: the goal on the navmesh, where a put goes.
    snapped: Option<[f32; 3]>,
}

/// Turning the view onto a reference and asking the crosshair for it.
struct Aim {
    target: FormId,
    height: usize,
    since: f64,
    /// What the crosshair had instead, last.
    had: Option<String>,
}

/// A fight with an actor.
struct Fight {
    target: FormId,
    /// Since when the player has been shooting from where it stands.
    shooting: Option<f64>,
    near: bool,
    /// Since when there has been no clear line of fire from where the player stands.
    blocked: Option<f64>,
    /// When the fight began: the weapon goes up a tier as it lasts ([`ARMS`]).
    began: f64,
    /// Someone standing in the line of fire, last seen.
    in_the_way: Option<u32>,
    /// The target's health when this spell of shooting began: hits landing mean the
    /// player stays where it is (module notes).
    health: Option<f64>,
    /// No way on foot to the target, but a clear line of fire to it: shot from afar
    /// (Honest Hearts' White Legs on the cliffs over the ambush).
    afar: bool,
}

enum Phase {
    /// The next step has not begun.
    Begin,
    Fight(Fight, Walk),
    Walk(Walk),
    Aim(Aim),
    /// Until `until` (seconds), or sooner by the step's own test.
    Wait { until: f64 },
    /// An actor the player cannot walk to now is waited for: it may come out and start
    /// the conversation itself (the Nellis gate guard behind the locked gate, then Raquel
    /// coming out to take the player to Pearl), or come near enough to walk to. Until
    /// `until`; `why` is what the walk said.
    Await { target: FormId, until: f64, why: String, tries: u8 },
    /// A `hit` step the player makes: walks within range of the object, aims and fires the
    /// step's weapon until a shot runs its hit script.
    Shoot(Shoot, Walk),
}

/// A shot the player fires at an object ([`shoot_object`]).
struct Shoot {
    target: FormId,
    /// The player's hits on it that ran its scripts when the step began.
    from: u32,
    /// Since when it has been firing.
    firing: Option<f64>,
    /// The walk is a step back from it, to [`SHOOT_OBJECT_NEAR`] or more.
    backing: bool,
}

#[derive(Resource)]
struct Pilot {
    /// The chains still to play, the one being played and how many are done.
    queue: std::collections::VecDeque<Chain>,
    chain: String,
    chains_done: usize,
    campaign: bool,
    /// Since when a message box nobody waits for has been up.
    box_since: Option<f64>,
    /// A button of that box was picked and waits to be read.
    box_picked: bool,
    /// When the terminal screen was last given an input.
    terminal_at: f64,
    /// What the pilot does of its own accord before the step goes on, the one in hand
    /// (with its walk, when it began and how far it is), and the bodies searched.
    errands: std::collections::VecDeque<Errand>,
    errand: Option<(Errand, Walk, f64, u8)>,
    searched: Vec<FormId>,
    /// Hours waited in this step, and the wait under way: when the menu was asked for.
    waited: u32,
    /// The stall watchdog's last sign of progress: when, where the player stood, the step,
    /// the hours waited and the lines said then ([`STALL_SECS`]).
    stall: (f64, [f32; 3], usize, u32, u64),
    /// Where the player stood, every few seconds over the last [`STALL_SECS`]: walking
    /// counts as progress by the ground it gained over that time, not by single moves
    /// (the stuck walk's 200-unit puts along its path each looked like 150 units walked
    /// while it went round a refused door for many minutes, Vault 112's hatch).
    trail: std::collections::VecDeque<(f64, [f32; 3])>,
    waiting: Option<f64>,
    /// Steps done the headless way, by kind.
    headless: std::collections::BTreeMap<String, u32>,
    /// Failed steps one after the other, and the real time the pilot last could act.
    failed_in_a_row: u32,
    acted_at: Option<f64>,
    /// The conversation (its serial) last gone along with in the player's way.
    went_along: Option<u32>,
    /// Since when the open conversation has offered no way out, and the picks made in it
    /// looking for one.
    stuck_talk: Option<f64>,
    talk_picks: u8,
    /// The conversation `talk_picks` and `stuck_talk` count for (`Game::dialogue_serial`).
    talk_serial: u32,
    /// A `say_info` with no known way to its line: the step and the topics already tried
    /// in that conversation, as a player tries what is on offer.
    explored: (usize, Vec<FormId>),
    /// A talk target whose own spot no walk reaches: the step's points round it tried so
    /// far (`across_point`), as a player talks to someone across a counter.
    across: Option<(FormId, usize)>,
    /// How often this step's walk into a trigger ended outside it and went on.
    trigger_rewalks: u8,
    /// When this step's player first stood beside the speaker a `say_info` waits on.
    near_since: Option<f64>,
    /// Since when the player was put on a long way (the fast-travel stand-in): its
    /// teammates follow a moment later, as fast travel brings them.
    followers_due: Option<f64>,
    /// The current chain's plugins and file (`Chain::plugins`).
    chain_plugins: Vec<String>,
    chain_raw: Json,
    /// Seconds of real time after which the run ends (`--pilot-minutes`).
    deadline: Option<f64>,
    /// A campaign's chains have their given state set all the same (`--pilot-given`).
    set_given: bool,
    /// God mode is on, and when the weapon is looked at next.
    god: bool,
    weapon_check: f64,
    /// The chain's save was asked for, and when the next chain may begin.
    saved: bool,
    resume_at: f64,
    /// The other menu up, and since when.
    menu_since: Option<(u16, f64)>,
    /// The game's state goes through a save and a load before each milestone is read.
    save_load: bool,
    /// Differences known not to be bugs (`--pilot-known`), and the ones met.
    known: Vec<Json>,
    known_seen: Vec<Json>,
    /// Times the actor of a `talk` step was woken in this step.
    woke: u8,
    /// Attackers the player could not kill: left alone for the rest of the chain.
    given_up: Vec<FormId>,
    /// How many of an `add_item`'s item the player had when the step began.
    mark_items: i32,
    /// For a `say_info` kept company: how many lines had been begun when the chain (in
    /// a campaign, the first chain) began, and since when the speaker has stood where it stands.
    mark: u64,
    /// The lines begun when a `dialogue_package` step began (`Seen::lines_begun`).
    step_lines: u64,
    /// Where the player last stood when a path was found from there: put back there when
    /// it ends up on a scrap of navmesh that leads nowhere (`Walk::stranded`).
    walked_from: Option<Place>,
    /// What steps left to the AI still owe ([`AiDue`]): checked every frame.
    ai_due: Vec<AiDue>,
    /// For a `package` step gone along with: the space the player stood in on the last
    /// frame, and the actors' space a script moved the player out of while it stood
    /// beside them (MQ08's capture fades out and wakes the player in Raven Rock).
    pkg_was_in: Option<Space>,
    scene_left: Option<Space>,
    still: (f64, [f32; 3]),
    /// An attacker being fought before the step goes on, since when, and when the
    /// attackers are looked for next.
    defend: Option<(Fight, Walk)>,
    defend_from: f64,
    threat_check: f64,
    /// The references whose script counts the collar's radios ([`beeping_speaker`]),
    /// found once; and when to look for a beeping one next.
    speakers: Option<Vec<FormId>>,
    speaker_check: f64,
    /// When the player last walked out of each unswitchable speaker's range: once in
    /// [`AWAY_AGAIN_SECS`], then the step's walk dashes through, as a player does.
    away_from: Vec<(FormId, f64)>,
    /// The packages each actor of a coming `package … end` step has been on, in order: one
    /// it has already left is not waited for, nor its end replayed ([`Action::Package`]).
    pkg_history: std::collections::HashMap<FormId, Vec<FormId>>,
    title: String,
    given: Vec<Json>,
    actions: Vec<Action>,
    ops: Vec<String>,
    /// The headless observation of each step.
    expect: std::collections::HashMap<usize, Json>,
    out: Option<PathBuf>,
    applier: Applier,
    given_applied: bool,
    armed: bool,
    i: usize,
    phase: Phase,
    /// The step's part: 0 getting there, 1 aimed, 2 acted.
    part: u8,
    /// Seconds the game has run under the pilot, and when the step and the line began.
    now: f64,
    step_from: f64,
    line: Option<(u64, f64)>,
    /// The last line heard in a conversation.
    heard: Option<FormId>,
    /// The chains in which the player already stored things to meet a line's carry-weight
    /// limit ([`stash_weight`]).
    stashed: Vec<String>,
    /// Objects no shot of the player's landed on: the next hits on them are headless.
    no_shot: Vec<FormId>,
    /// The step to go back to once the conversation is left, after storing things.
    retalk: Option<usize>,
    steps: Vec<Json>,
    observations: Vec<Json>,
    divergences: Vec<Json>,
    findings: Vec<Json>,
    done: bool,
}

impl Pilot {
    fn new(chains: Vec<Chain>, out: Option<PathBuf>, campaign: bool) -> Pilot {
        let mut p = Pilot {
            queue: chains.into(),
            chain: String::new(),
            chains_done: 0,
            campaign,
            box_since: None,
            box_picked: false,
            terminal_at: 0.0,
            errands: Default::default(),
            errand: None,
            searched: Vec::new(),
            waited: 0,
            stall: (0.0, [0.0; 3], usize::MAX, 0, 0),
            trail: Default::default(),
            waiting: None,
            headless: Default::default(),
            failed_in_a_row: 0,
            acted_at: None,
            went_along: None,
            stuck_talk: None,
            talk_picks: 0,
            talk_serial: 0,
            explored: (usize::MAX, Vec::new()),
            across: None,
            trigger_rewalks: 0,
            near_since: None,
            followers_due: None,
            chain_plugins: Vec::new(),
            chain_raw: Json::Null,
            deadline: None,
            set_given: false,
            god: false,
            weapon_check: 0.0,
            saved: false,
            resume_at: 0.0,
            menu_since: None,
            save_load: false,
            known: Vec::new(),
            known_seen: Vec::new(),
            woke: 0,
            given_up: Vec::new(),
            mark_items: 0,
            mark: 0,
            step_lines: 0,
            walked_from: None,
            ai_due: Vec::new(),
            pkg_was_in: None,
            scene_left: None,
            still: (0.0, [0.0; 3]),
            defend: None,
            defend_from: 0.0,
            threat_check: 0.0,
            speakers: None,
            speaker_check: 0.0,
            away_from: Vec::new(),
            pkg_history: std::collections::HashMap::new(),
            title: String::new(),
            given: Vec::new(),
            actions: Vec::new(),
            ops: Vec::new(),
            expect: Default::default(),
            out,
            applier: Applier::default(),
            given_applied: false,
            armed: false,
            i: 0,
            phase: Phase::Begin,
            part: 0,
            now: 0.0,
            step_from: 0.0,
            line: None,
            heard: None,
            stashed: Vec::new(),
            no_shot: Vec::new(),
            retalk: None,
            steps: Vec::new(),
            observations: Vec::new(),
            divergences: Vec::new(),
            findings: Vec::new(),
            done: false,
        };
        p.next_chain();
        p
    }

    /// Take the next chain of the queue. Whether there was one.
    fn next_chain(&mut self) -> bool {
        let Some(c) = self.queue.pop_front() else { return false };
        (self.chain, self.title, self.given, self.actions, self.ops, self.expect) = (c.name, c.title, c.given, c.actions, c.ops, c.expect);
        (self.chain_plugins, self.chain_raw) = (c.plugins, c.raw);
        (self.i, self.phase, self.part, self.given_applied, self.step_from) = (0, Phase::Begin, 0, false, self.now);
        true
    }

    fn finding(&mut self, kind: &str, text: String) {
        warn!("pilot: finding {kind}: step {} {}: {text}", self.i, self.ops.get(self.i).map_or("", String::as_str));
        self.findings.push(json!({"kind": kind, "chain": self.chain, "step": self.i, "op": self.ops.get(self.i), "text": text}));
    }

    /// The walk's own "unreachable" finding for this step taken back: the step that walked
    /// deals with it another way (a shot from afar, a kill the headless way said as its own).
    fn take_back_unreachable(&mut self) {
        let i = self.i;
        if self.findings.last().is_some_and(|f| f["kind"] == "unreachable" && f["step"] == json!(i)) {
            self.findings.pop();
        }
    }

    /// The step is over: the next one begins.
    fn next(&mut self, error: Option<String>) {
        let op = self.ops.get(self.i).cloned().unwrap_or_default();
        let secs = self.now - self.step_from;
        match &error {
            Some(e) => warn!("pilot: step {} {op} failed after {secs:.1} s: {e}", self.i),
            None => info!("pilot: step {} {op} done in {secs:.1} s", self.i),
        }
        let mut s = json!({"chain": self.chain, "step": self.i, "op": op, "seconds": (secs * 10.0).round() / 10.0});
        // A read, a note or a wait that passes does not end a run of failures: a chain whose
        // player never reached its add-on (Honest Hearts' Northern Passage) failed every walk,
        // talk and fight for 40 minutes while the get_quest steps between them passed.
        let passive = op.starts_with("get_") || matches!(op.as_str(), "note" | "advance_seconds" | "wait_talk" | "end_talk");
        if error.is_some() {
            self.failed_in_a_row += 1;
        } else if !passive {
            self.failed_in_a_row = 0;
        }
        // Done after all: a way the step found no path for on the way (a locked door's
        // switch, a trigger's middle) was a detour, not a place a player cannot reach
        // (Lonesome Road's blast door controls, reached after the silo door's switch was
        // not [observed]).
        if error.is_none() {
            let i = self.i;
            for f in self.findings.iter_mut().filter(|f| f["kind"] == "unreachable" && f["step"] == json!(i)) {
                f["kind"] = json!("detour");
            }
        }
        if let Some(e) = error {
            s["error"] = json!(e);
        }
        self.steps.push(s);
        if self.failed_in_a_row >= FAILED_IN_A_ROW {
            self.failed_in_a_row = 0;
            let left = self.actions.len().saturating_sub(self.i + 1);
            self.finding("chain_abandoned", format!("{FAILED_IN_A_ROW} steps failed one after the other: the {left} steps left of the chain are not played"));
            self.i = self.actions.len().saturating_sub(1);
        }
        self.i += 1;
        (self.phase, self.part, self.step_from) = (Phase::Begin, 0, self.now);
        (self.waited, self.waiting, self.woke) = (0, None, 0);
        self.across = None;
        self.trigger_rewalks = 0;
        self.near_since = None;
    }

    /// A step, or a fallback, done as the headless run does it: counted.
    fn headless(&mut self, op: &str) {
        info!("pilot: headless: {op} (step {})", self.i);
        *self.headless.entry(op.to_string()).or_default() += 1;
    }

    /// A labelled read, against the headless one of the same step. Whether they agree.
    fn observe(&mut self, label: &str, fields: &Map<String, Json>, keep: bool) -> bool {
        let differing: Vec<Json> = match self.expect.get(&self.i) {
            Some(want) => fields.iter().filter(|(k, v)| want.get(k.as_str()).is_some_and(|w| !same(w, v))).map(|(k, v)| json!({"field": k, "headless": want[k.as_str()], "game": v})).collect(),
            None => Vec::new(),
        };
        if !differing.is_empty() && !keep {
            return false;
        }
        let mut o = json!({"chain": self.chain, "label": label, "step": self.i});
        for (k, v) in fields {
            o[k.as_str()] = v.clone();
        }
        self.observations.push(o);
        if differing.is_empty() {
            return true;
        }
        // A difference known not to be a bug (`--pilot-known`): kept apart, with its reason.
        let known = self.known.iter().find(|k| k["chain"].as_str() == Some(self.chain.as_str()) && k["label"].as_str() == Some(label)).and_then(|k| k["reason"].as_str()).map(str::to_string);
        if let Some(reason) = known {
            info!("pilot: known difference at step {} ({label}): {reason}", self.i);
            self.known_seen.push(json!({"chain": self.chain, "step": self.i, "label": label, "fields": differing, "reason": reason}));
            return false;
        }
        warn!("pilot: divergence at step {} ({label}): {}", self.i, Json::Array(differing.clone()));
        self.divergences.push(json!({"chain": self.chain, "step": self.i, "label": label, "fields": differing}));
        false
    }

    fn report(&self, softlocks: u32) -> Json {
        json!({
            "format": REPORT_FORMAT, "campaign": self.campaign, "chains": self.chains_done, "last_title": self.title, "seconds": self.now.round(),
            "steps": self.steps, "observations": self.observations, "divergences": self.divergences, "findings": self.findings, "softlocks": softlocks,
            "headless": self.headless, "known_differences": self.known_seen,
        })
    }
}

/// Numbers alike within a hair, anything else equal.
fn same(a: &Json, b: &Json) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => (x - y).abs() <= 1e-6 * x.abs().max(1.0),
        _ => a == b,
    }
}

/// What an activation by hand came to, in a word.
fn by_hand(b: &ByHand) -> &'static str {
    match b {
        ByHand::Script => "its script ran",
        ByHand::Default(_) => "its default action",
        ByHand::Talk => "a conversation",
        ByHand::Greeted => "a greeting",
        ByHand::Choices => "a choice of actions",
        ByHand::Wheel => "the companion wheel",
        ByHand::Refused => "refused",
        ByHand::Woken => "woken",
    }
}

fn outcome(r: Option<&Result<ByHand, String>>) -> String {
    match r {
        Some(Ok(b)) => by_hand(b).to_string(),
        Some(Err(e)) => format!("error: {e}"),
        None => "the scripts are not running".to_string(),
    }
}

fn hdist(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Where a reference is now: its cell and position.
fn where_is(g: &Game, r: FormId) -> Option<(Option<FormId>, [f32; 3])> {
    g.world.peek(r).map(|s| (s.cell, s.pos)).or_else(|| g.world.placed.get(r).map(|p| (p.cell, p.pos)))
}

/// Whether two references' cells are in one space: the same interior, or the same
/// worldspace out of doors (an actor 147 units off can stand in the next grid cell:
/// Boone at Bitter Springs came and was not counted).
fn same_space(world: &crate::world::WorldData, a: Option<FormId>, b: Option<FormId>) -> bool {
    let space = |c: Option<FormId>| c.and_then(|c| crate::nav::space_of_cell(world, c.0));
    a == b || space(a).is_some_and(|s| Some(s) == space(b))
}

fn space_of(loc: &Location) -> Space {
    match loc.interior {
        Some(c) => Space::Interior(c),
        None => Space::World(loc.world),
    }
}

fn travel_to(p: Place, heading_deg: f32) -> Travel {
    match p.space {
        Space::World(w) => Travel { world: Some(w), interior: None, feet: p.pos, heading_deg, pitch_deg: None },
        Space::Interior(c) => Travel { world: None, interior: Some(c), feet: p.pos, heading_deg, pitch_deg: None },
    }
}

/// The Mojave's own worldspace (WastelandNV), where the map's fast travel goes [observed:
/// FalloutNV.esm WRLD 0x000DA726].
const MOJAVE: u32 = 0x000D_A726;

/// Whether a player in `here` could fast travel into `to` (the stand-in for a long way):
/// within its own worldspace, or out to the Mojave's. Never into another worldspace
/// (a DLC's: a long way round a locked door once put the player in Dead Money's Villa).
fn fast_travel_to(here: Space, to: Space) -> bool {
    match to {
        // The Mojave is matched by its FormID; Tale of Two Wastelands' Capital Wasteland
        // has no fixed one, and a long way into it is only cut when it is the player's own.
        // Or a worldspace of the base game itself (the Strip, Freeside: map markers a
        // player fast travels to); never an add-on's. From Goodsprings the way to Tops
        // ran 165,000 units, all of it walked, into the Strip's worldspace.
        Space::World(w) => here == to || w == MOJAVE || w >> 24 == 0,
        Space::Interior(_) => false,
    }
}

/// The point `tail` units before the end of a leg's points.
fn tail_point(points: &[[f32; 3]], tail: f32) -> [f32; 3] {
    let mut left = tail;
    for w in points.windows(2).rev() {
        let d = hdist(w[0], w[1]);
        if d >= left && d > 1e-3 {
            let t = left / d;
            return [w[1][0] + (w[0][0] - w[1][0]) * t, w[1][1] + (w[0][1] - w[1][1]) * t, w[1][2] + (w[0][2] - w[1][2]) * t];
        }
        left -= d;
    }
    points.first().copied().unwrap_or_default()
}

/// What the systems lend the pilot for a frame.
struct Hands<'a, 'w> {
    script: &'a mut ScriptState,
    sim: &'a mut SimState,
    phys: &'a mut PhysState,
    player: &'a mut Player,
    intent: &'a mut MoveIntent,
    control: &'a mut PlayerControl,
    locks: &'a ControlLocks,
    location: &'a Location,
    target: &'a Target,
    nav: Option<&'a crate::nav::NavState>,
    world: &'a crate::world::WorldData,
    doors: &'a nvf_play::doors::Doors,
    /// A menu holds the game this frame: the player cannot move.
    in_menu: bool,
    /// The Wait menu and the ways to ask for it and work it.
    wait_menu: &'a nvf_play::wait::WaitMenu,
    wait_open: &'a mut MessageWriter<'w, nvf_play::wait::OpenWait>,
    wait_input: &'a mut MessageWriter<'w, nvf_play::wait::WaitInput>,
    /// The terminal screen that is up, and the way to work it.
    terminal: &'a nvf_play::terminal::TerminalScreen,
    terminal_input: &'a mut MessageWriter<'w, nvf_play::terminal::TerminalInput>,
    travel: &'a mut MessageWriter<'w, TravelRequest>,
    /// The player's shots on objects that ran their hit scripts, by object.
    object_hits: Option<&'a std::collections::BTreeMap<u32, u32>>,
    dt: f32,
}

impl Hands<'_, '_> {
    fn game<R>(&mut self, f: impl FnOnce(&mut Game<'static>) -> R) -> Option<R> {
        let rt = self.script.rt.as_mut()?;
        Some(with_world(rt, self.sim, |rt| rt.player_action(f)))
    }

    fn feet(&self) -> [f32; 3] {
        self.phys.player.as_ref().map_or(self.player.feet, |c| c.feet)
    }

    fn here(&self) -> Place {
        Place { space: space_of(self.location), pos: self.feet() }
    }

    /// The place of a reference (a marker, an actor), by the runner's names.
    fn place_of(&mut self, name: &str) -> Result<(FormId, Place), String> {
        let world = self.world;
        self.game(|g| {
            let r = runner::resolve(g, name)?;
            let (cell, pos) = where_is(g, r).ok_or_else(|| format!("{name}: nowhere in the world"))?;
            let cell = cell.ok_or_else(|| format!("{name}: in no cell"))?;
            let space = crate::nav::space_of_cell(world, cell.0).ok_or_else(|| format!("{name}: its cell {cell} is not indexed"))?;
            Ok((r, Place { space, pos }))
        })
        .unwrap_or_else(|| Err("the scripts are not running".into()))
    }

    /// Put the player at a place without walking (a stand-in, logged by the caller).
    fn put(&mut self, p: Place) {
        if p.space == space_of(self.location) {
            self.phys.place_player(p.pos, 60.0, 1000.0);
            if let Some(c) = self.phys.player.as_ref() {
                self.player.feet = c.feet;
            }
        } else {
            self.travel.write(TravelRequest(travel_to(p, self.player.heading.to_degrees())));
        }
    }
}

/// A step across a gap between two walkable areas of one interior ([`Walk::step`]).
struct StepAcross {
    /// The near end, on the outline of the player's area, and the far end, on the goal's.
    from: [f32; 3],
    to: [f32; 3],
    /// The walkable area `to` lies on: the step is over when the player stands on it.
    area: u32,
    /// The walk to `from`.
    walk: Box<Walk>,
    /// Since when the player walks straight from `from` to `to`.
    since: Option<f64>,
}

/// How far apart two walkable areas of one interior may lie, on the ground and in height,
/// for the player to step from one onto the other where no navmesh link joins them. SSHQ01's
/// stair tops: 17 to 31 units across, 31 in height [observed, #3497]. A player's step
/// is not measured here [hypothesis: the physics' own step-up decides, which is why the
/// step is walked and checked, not put].
const STEP_ACROSS: f32 = 48.0;
const STEP_RISE: f32 = 48.0;
/// Seconds the player has to get from the near end of a step across to the far end.
const STEP_ACROSS_SECS: f64 = 6.0;
/// Steps across on one walk, at most: a third would be the same gap again.
const STEPS_ACROSS: u8 = 2;

/// A step the headless run carries out and the pilot leaves to the AI, checked afterwards
/// (#2365: `patrol_point` and an actor entering a trigger succeeded at once and nothing
/// looked whether the actor did it; #2445 Chauncey, #2451 Veronica and #2433 Orris broke
/// there). Kept until the actor is there, or a `ai_not_done` finding after [`AI_DUE_SECS`]
/// or at the chain's end.
struct AiDue {
    /// The step that left it to the AI.
    step: usize,
    actor: FormId,
    /// The reference the actor has to reach (a patrol point, a trigger).
    at: FormId,
    /// How near counts, game units.
    within: f32,
    what: String,
    since: f64,
}

/// How long the AI has to do what a step left to it, seconds of the pilot's clock.
const AI_DUE_SECS: f64 = 180.0;
/// How near a patrol point an actor counts as having reached it, and a trigger's
/// reference as having entered it [hypothesis: a trigger's box is not read here; most of
/// the chains' triggers are smaller than this].
const AI_DUE_NEAR: f32 = 256.0;

/// Check what the AI still owes: say each that is done, and give up on each overdue one.
fn check_ai_due(p: &mut Pilot, h: &mut Hands, chain_over: bool) {
    if p.ai_due.is_empty() {
        return;
    }
    let world = h.world;
    let due = std::mem::take(&mut p.ai_due);
    for d in due {
        let near = h
            .game(|g| {
                let (ca, a) = where_is(g, d.actor)?;
                let (cb, b) = where_is(g, d.at)?;
                Some(same_space(world, ca, cb) && hdist(a, b) <= d.within && (a[2] - b[2]).abs() < 256.0)
            })
            .flatten()
            .unwrap_or(false);
        if near {
            info!("pilot: step {}: the AI did what step {} left to it: {} ({:.0} s later)", p.i, d.step, d.what, p.now - d.since);
        } else if chain_over || p.now - d.since > AI_DUE_SECS {
            let why = if chain_over { "the chain ended first".to_string() } else { format!("not within {AI_DUE_SECS:.0} s") };
            p.finding("ai_not_done", format!("step {} left to the AI: {}, {why}", d.step, d.what));
        } else {
            p.ai_due.push(d);
        }
    }
}

enum Walked {
    Going,
    There,
    /// Put there without walking.
    Put,
    /// The player cannot get there: why.
    No(String),
}

/// One frame of a walk.
fn walk(p: &mut Pilot, w: &mut Walk, h: &mut Hands) -> Walked {
    let feet = h.feet();
    let here = h.here();
    // A script has moved the player to another space and the travel has not happened yet
    // (Lonesome Road's narrator moves the player from the end slides to the Canyon
    // Wreckage): planned from where the player was, the way was "unreachable". Waited for,
    // up to [`TRAVEL_WAIT`].
    let moving_to = h.game(|g| g.cell_of(FormId::PLAYER_REF)).flatten().and_then(|c| crate::nav::space_of_cell(h.world, c.0)).filter(|s| *s != here.space);
    match moving_to {
        Some(to) if w.travel_since.is_none_or(|t| p.now - t < TRAVEL_WAIT) => {
            if w.travel_since.is_none() {
                info!("pilot: step {}: a script has moved the player to {to:?}: waits for the travel", p.i);
                w.travel_since = Some(p.now);
            }
            w.follower = None;
            w.seen = (p.now, feet);
            return Walked::Going;
        }
        None => w.travel_since = None,
        Some(_) => {}
    }
    // The player's teammates come along a fast-travel stand-in, a moment after it (once
    // the player stands there): Boone was left 107,000 units behind and "never came".
    if p.followers_due.is_some_and(|t| p.now - t > FOLLOWERS_AFTER) {
        p.followers_due = None;
        let mut applier = std::mem::take(&mut p.applier);
        let moved = h
            .game(|g| {
                let mut moved = Vec::new();
                for r in g.teammates() {
                    if runner::apply_now(g, &mut applier, &Action::Call { line: format!("{:08X}.MoveTo player", r.0), label: None }).is_ok() {
                        moved.push(g.data.name(r));
                    }
                }
                moved
            })
            .unwrap_or_default();
        p.applier = applier;
        if !moved.is_empty() {
            info!("pilot: step {}: the player's teammates come along the fast-travel stand-in: {}", p.i, moved.join(", "));
        }
    }
    // An errand the walk asked for: handed to the pilot, and a new path afterwards.
    if let Some((errand, place)) = w.errand.take() {
        p.errand = Some((errand, walk_to(place, None, NEAR_TARGET, p.now, feet), p.now, 0));
        (w.follower, w.door_since, w.seen) = (None, None, (p.now, feet));
        return Walked::Going;
    }
    // To a switch for a locked door first; then the walk goes on with a new path.
    if let Some((switch, door, mut side)) = w.side.take() {
        match walk(p, &mut side, h) {
            Walked::Going => {
                w.side = Some((switch, door, side));
                w.seen = (p.now, feet);
                return Walked::Going;
            }
            Walked::No(why) => info!("pilot: step {}: the switch for door {door:08X} cannot be reached ({why})", p.i),
            Walked::There | Walked::Put => {
                let name = h.game(|g| g.data.name(switch)).unwrap_or_default();
                let done = h.script.rt.as_mut().map(|rt| with_world(rt, h.sim, |rt| rt.activate_as_player(switch)));
                info!("pilot: step {}: the player uses {name} for door {door:08X}: {}", p.i, outcome(done.as_ref()));
            }
        }
        (w.follower, w.door_since, w.seen) = (None, None, (p.now, feet));
        return Walked::Going;
    }
    // A step across a gap no navmesh link joins: to its near end, then straight over, as a
    // player walks up SSHQ01's stairs. Not put: the player's own physics has to take it.
    if let Some(mut st) = w.step.take() {
        match st.since {
            None => match walk(p, &mut st.walk, h) {
                Walked::Going => {
                    w.step = Some(st);
                    w.seen = (p.now, feet);
                    return Walked::Going;
                }
                Walked::No(why) => return Walked::No(why),
                Walked::There | Walked::Put => {
                    info!("pilot: step {}: steps across from ({:.0}, {:.0}, {:.0}) to ({:.0}, {:.0}, {:.0}), where no navmesh link joins the two, as a player walks it", p.i, st.from[0], st.from[1], st.from[2], st.to[0], st.to[1], st.to[2]);
                    st.since = Some(p.now);
                    w.step = Some(st);
                    w.seen = (p.now, feet);
                    return Walked::Going;
                }
            },
            Some(since) => {
                let on_it = h.nav.and_then(|nav| nav.graph.locate(here.space, feet, 24.0).map(|t| nav.graph.area[t as usize])) == Some(st.area);
                if on_it && hdist(feet, st.to) < 32.0 {
                    info!("pilot: step {}: stepped across in {:.1} s, at ({:.0}, {:.0}, {:.0})", p.i, p.now - since, feet[0], feet[1], feet[2]);
                    (w.follower, w.door_since, w.seen) = (None, None, (p.now, feet));
                    return Walked::Going;
                }
                if p.now - since > STEP_ACROSS_SECS {
                    let why = format!("could not step across from ({:.0}, {:.0}, {:.0}) to ({:.0}, {:.0}, {:.0}) in {STEP_ACROSS_SECS:.0} s, where no navmesh link joins the two (stopped at ({:.0}, {:.0}, {:.0})): a player could not get there; the player stays where it is", st.from[0], st.from[1], st.from[2], st.to[0], st.to[1], st.to[2], feet[0], feet[1], feet[2]);
                    p.finding("unreachable", why.clone());
                    return Walked::No(why);
                }
                let want = heading_of([st.to[0] - feet[0], st.to[1] - feet[1]]);
                h.player.heading = turn_towards(h.player.heading, want, TURN_RATE * h.dt);
                h.player.pitch = turn_towards(h.player.pitch, 0.0, TURN_RATE * h.dt);
                let on = heading_delta(h.player.heading, want).abs() < 0.7;
                (h.intent.forward, h.intent.right, h.intent.walk) = (if on { 1.0 } else { 0.0 }, 0.0, h.control.sneak);
                w.step = Some(st);
                w.seen = (p.now, feet);
                return Walked::Going;
            }
        }
    }
    // The actor walked to has come and is talking to the player: the player is with it.
    let talker = h.game(|g| g.dialogue.as_ref().map(|c| c.actor)).flatten();
    if talker.is_some() && talker == w.target {
        if !w.told_talking {
            info!("pilot: step {}: the actor walked to is talking to the player: the walk is over", p.i);
        }
        w.told_talking = true;
        return Walked::There;
    }
    w.told_talking = false;
    // A conversation that begins on the way: the steps after the walk hear it, or
    // nothing in the chain answers it and it is left as a player leaves it.
    let began_talking = *w.talking_at_start.get_or_insert(talker.is_some());
    if talker.is_some() && !began_talking {
        if w.keep_talk {
            info!("pilot: step {}: a conversation began on the way: the walk ends here", p.i);
            return Walked::There;
        }
        if !w.dodge.is_some_and(|(t, _)| t < 0.0) {
            info!("pilot: step {}: a conversation began on the way that no step answers: the player goes along with it once, then leaves", p.i);
            // (Said once: the marker is a dodge that is over.)
            w.dodge = Some((-1.0, 0.0));
        }
        w.seen = (p.now, feet);
        if leave_talk(p, h, true) {
            (w.follower, w.door_since, w.dodge) = (None, None, None);
        }
        return Walked::Going;
    }
    // A scene being played near the player (NPCs saying scripted lines to one another, or
    // a conversation between them): the player stops and watches, up to [`SCENE_SECS`] a
    // walk. The Silver Rush's entrance plays Jean-Baptiste's scene with the volunteer;
    // walking on through it crosses a trigger that turns the Van Graffs hostile and fails
    // Birds of a Feather. An activator's lines (an intercom, a talking activator) are no scene (MQ11's intercom nags
    // the player to start the purifier while its timer runs: watching it lets the timer
    // run out, the fail ending).
    if w.scene_held < SCENE_SECS {
        let scene = h
            .game(|g| {
                let here_cell = g.world.peek(FormId::PLAYER_REF).and_then(|s| s.cell);
                g.speech.iter().find(|sp| {
                    (sp.say_to || g.in_actor_conversation(sp.speaker)) && !sp.greeting && sp.target != Some(FormId::PLAYER_REF) && !g.chatter_topic(sp.topic)
                        && fnv_game::functions::base_of(g, sp.speaker).and_then(|b| g.data.sig(b)).is_some_and(|s| [b"NPC_", b"CREA", b"LVLN", b"LVLC"].contains(&&s))
                        && where_is(g, sp.speaker).is_some_and(|(c, at)| c == here_cell && hdist(at, feet) < SCENE_RANGE)
                }).map(|sp| sp.speaker)
            })
            .flatten()
            // Not the one the walk goes to: a player does not stand and watch the man it
            // is fighting. Joe Cobb's scene with Trudy held the Goodsprings fight's walk to
            // him while the saloon shot at the player (pilot-goodsprings-fight, the merge's
            // smoke run 2026-11-23 [observed]).
            .filter(|who| Some(*who) != w.target);
        if let Some(who) = scene {
            if w.scene_held == 0.0 {
                let name = h.game(|g| g.data.name(who)).unwrap_or_default();
                info!("pilot: step {}: a scene plays near the player ({name} speaking): the player stops and watches", p.i);
            }
            w.scene_last = Some(p.now);
        }
        // Between its lines a scene goes on (Jean-Baptiste walks to fetch a volunteer
        // after Gloria's last line): the player keeps watching while a line was said in the
        // last [`SCENE_GAP`] s.
        if w.scene_last.is_some_and(|t| p.now - t < SCENE_GAP) {
            w.scene_held += h.dt as f64;
            (h.intent.forward, h.intent.right) = (0.0, 0.0);
            w.seen = (p.now, feet);
            return Walked::Going;
        }
    }
    // One already open when the walk began holds the player: it is left first, as a
    // player leaves one to go on (`leave_talk`). (Not gone along with: it is the chain's
    // own, left open by an earlier step.)
    if talker.is_some() {
        w.seen = (p.now, feet);
        if leave_talk(p, h, false) {
            w.talking_at_start = Some(false);
        }
        return Walked::Going;
    }
    w.talking_at_start = Some(false);
    // The reference walked to has moved: a new path.
    if let Some(now) = w.target.and_then(|t| h.game(|g| where_is(g, t)).flatten()) {
        if hdist(now.1, w.goal.pos) > 96.0 {
            w.goal.pos = now.1;
            if let Some(space) = now.0.and_then(|c| crate::nav::space_of_cell(h.world, c.0)) {
                w.goal.space = space;
            }
            w.follower = None;
        }
    }
    if here.space == w.goal.space && hdist(feet, w.goal.pos) <= w.radius && (feet[2] - w.goal.pos[2]).abs() < 160.0 {
        return Walked::There;
    }
    let Some(nav) = h.nav else {
        p.finding("unreachable", "no navmesh graph in this run (--no-npc-packages?)".into());
        return Walked::No("no navmesh graph".into());
    };
    if w.follower.is_none() {
        // The keys the player carries and the doors standing open, as an NPC's path has
        // them (fnv_ai `path_params`); the graph knows the locks as scripts left them.
        let (keys, open_doors) = h.game(|g| {
            let keys: Vec<u32> = g.world.peek(FormId::PLAYER_REF).and_then(|s| s.inventory.as_ref()).map(|inv| inv.iter().map(|(item, _)| item.0).collect()).unwrap_or_default();
            let open: Vec<u32> = g.world.changed().filter(|(_, s)| s.open).map(|(id, _)| id.0).collect();
            (keys, open)
        })
        .unwrap_or_default();
        let mut params = fnv_nav::PathParams { opens_doors: true, ..Default::default() };
        params.keys = keys;
        params.open_doors.extend(open_doors);
        params.refused_doors.extend(w.tried.iter().map(|(d, _)| *d));
        // A locked door that opens in place and is out of the world (disabled) stands in
        // no one's way: Broken Steel's 0x0D009B07 on the way to the Citadel's lab, locked
        // at level 255 and disabled, held the whole of Death From Above up.
        let gone: Vec<u32> = h.game(|g| nav.graph.door_locks.keys().copied().filter(|&d| is_disabled(g, d)).collect()).unwrap_or_default();
        params.owned_doors.extend(gone.iter().copied());
        params.open_doors.extend(gone);
        // Standing off the navmesh (a given state's place, a ledge): onto the nearest
        // point of it within 500 units first, as the pilot's own clumsiness.
        // Or, none within the search's own reach of the start (512 units), within 4,096:
        // the Jefferson Memorial's intake pipe left the player with no triangle to start
        // from after the Enclave's arrival [observed: its vertibirds' obstacle cut, #2846],
        // and every later step failed there.
        if nav.graph.locate(here.space, feet, 96.0).is_none() || nav.graph.nearest(here.space, feet, fnv_nav::path::SNAP_RADIUS).is_none() {
            if let Some((_, on)) = nav.graph.nearest(here.space, feet, 500.0).or_else(|| nav.graph.nearest(here.space, feet, 4096.0)) {
                // Off by height too: under the navmesh of the Jefferson Memorial's intake
                // pipe the player stood 92 units below it, 0 units off across [observed].
                let off = hdist(on, feet).hypot(on[2] - feet[2]);
                // A few units off is left to the path search, which snaps its start from
                // up to SNAP_RADIUS: put back from 6 units off every frame, the player never
                // walked (Broken Steel's Powerworks, beside a triangle an obstacle cuts,
                // 2026-11-19 [observed]).
                if off > OFF_MESH_PUT {
                    info!("pilot: step {}: off the navmesh at ({:.0}, {:.0}, {:.0}): set on its nearest point, {off:.0} units off", p.i, feet[0], feet[1], feet[2]);
                    if off > 500.0 {
                        // Not clumsiness: the ground went from under the player (#2846).
                        p.finding("no_navmesh_under", format!("no usable navmesh within 512 units of ({:.0}, {:.0}, {:.0}) in {:?}: set on the nearest, {off:.0} units off", feet[0], feet[1], feet[2], here.space));
                    }
                    h.put(Place { space: here.space, pos: on });
                    return Walked::Going;
                }
            }
        }
        // The last stretch out of doors of a long way: only that is cut short.
        let outdoors = |path: &fnv_nav::Path| path.legs.iter().rposition(|l| matches!(l.space, Space::World(_)));
        // No walk without them: the way with the navmesh's drops and climbs (ledge links),
        // as a player jumps down a ledge. Lonesome Road's Divide is ten walkable areas
        // joined by ledges: from its east door to the Temple of the Union none other leads.
        let found = match nav.graph.find_path(here, w.goal, &params) {
            Err(e) => match nav.graph.find_path(here, w.goal, &fnv_nav::PathParams { ledges: true, ..params.clone() }) {
                Ok(path) => {
                    info!("pilot: step {}: no way without the navmesh's ledges: the way takes its drops and climbs", p.i);
                    Ok(path)
                }
                Err(_) => Err(e),
            },
            found => found,
        };
        // No navmesh under the goal (a trigger's middle over a table, a control box on a
        // wall: Velvet Curtain's escape trigger, Anchorage's gun boxes [observed]): the goal
        // is moved to the navmesh's nearest point within GOAL_SNAP, as the start is, and
        // planned again.
        let found = match found {
            Err(fnv_nav::PathError::NoTriangle { which: "goal", .. }) => match nav.graph.nearest(w.goal.space, w.goal.pos, GOAL_SNAP) {
                Some((_, on)) => {
                    info!("pilot: step {}: no navmesh under the goal ({:.0}, {:.0}, {:.0}): it moves {:.0} units to the nearest point on it", p.i, w.goal.pos[0], w.goal.pos[1], w.goal.pos[2], hdist(on, w.goal.pos).hypot(on[2] - w.goal.pos[2]));
                    w.goal.pos = on;
                    nav.graph.find_path(here, w.goal, &params)
                }
                None => found,
            },
            found => found,
        };
        // Never cut past a door whose script does something when used (it sets a stage,
        // starts a scene): the cut stops short of that door, or there is none when the leg
        // before it is indoors. Lonesome Road's Divide east door (script 0x01003E2A) sets
        // NVDLC04MQ02 50 and takes the player's controls, which Ulysses' trigger beyond it
        // gives back; cut past it, the trigger came first and the controls never came back.
        let cut_at = match &found {
            Ok(path) => {
                let scripted: Vec<bool> = h.game(|g| path.legs.iter().map(|l| l.door.is_some_and(|d| door_scripted(g, d.door))).collect()).unwrap_or_default();
                match (scripted.iter().position(|&x| x), outdoors(path)) {
                    (Some(j), Some(i)) if j < i => matches!(path.legs[j].space, Space::World(_)).then_some(j),
                    (_, i) => i,
                }
            }
            Err(_) => None,
        };
        match found {
            // Not while a script holds the player's movement: a player cannot fast travel then
            // either. Moira's medical check-up (her script 0x0A02D2C0) takes the controls and
            // gives them back 8 s later from her own script; cut away from Megaton before that,
            // her script stopped running and the controls never came back (Wasteland Survival
            // Guide, 13 soft locks [observed]). The walk waits where it is until they are back.
            Ok(path) if path.length > WALK_MAX && !w.cut && !h.locks.movement && cut_at.is_some_and(|i| fast_travel_to(here.space, path.legs[i].space) || capital_world(h, path.legs[i].space)) => {
                w.cut = true;
                let leg = &path.legs[cut_at.expect("checked")];
                let at = Place { space: leg.space, pos: tail_point(&leg.points, WALK_TAIL) };
                info!("pilot: step {}: the way is {:.0} units: starts {WALK_TAIL:.0} units before the end of its last stretch out of doors (a stand-in for fast travel)", p.i, path.length);
                h.put(at);
                p.followers_due = Some(p.now);
                w.seen = (p.now, at.pos);
                return Walked::Going;
            }
            Ok(path) => {
                // A door on the way that is out of the world (disabled), with no switch whose
                // script enables it: another way round, as a player finds one. Lonesome Road's
                // direct door into Ashton silo (0x0100CB7D) stays disabled until the player is
                // down there; the way in is the lift, whose far door the lift's button enables.
                let gone = h.game(|g| path.legs.iter().filter_map(|l| l.door).map(|d| d.door).find(|&d| is_disabled(g, d) && !door_switch(g, d, &[]).is_some_and(|s| s.4 == WORKS_DOOR))).flatten();
                if let Some(d) = gone {
                    info!("pilot: step {}: door {d:08X} on the way is not in the world and nothing the player can use enables it: another way round", p.i);
                    w.tried.push((d, "not in the world, no switch".into()));
                    return Walked::Going;
                }
                let legs: Vec<String> = path.legs.iter().map(|l| format!("{:?} {} pts{}", l.space, l.points.len(), l.door.map_or(String::new(), |d| format!(" door {:08X}", d.door)))).collect();
                info!("pilot: step {}: walks {:.0} units over {} leg(s) to ({:.0}, {:.0}, {:.0}): {}", p.i, path.length, path.legs.len(), w.goal.pos[0], w.goal.pos[1], w.goal.pos[2], legs.join("; "));
                w.locked_way = false;
                w.snapped = path.end();
                p.walked_from = Some(here);
                w.follower = Some(Follower::new(path));
                w.seen = (p.now, feet);
            }
            Err(e) => {
                // Through a locked door, perhaps: then up to that door, as a player goes.
                let mut forced = params.clone();
                forced.open_locked = true;
                match nav.graph.find_path(here, w.goal, &forced) {
                    // A long way up to the locked door: its stretch out of doors before the
                    // door is cut short as any long way is (from Goodsprings to the Nellis
                    // gate the player walked it all, fighting all the way).
                    Ok(path) if path.length > WALK_MAX && !w.cut && !h.locks.movement => {
                        w.cut = true;
                        let locked = |d: u32| nav.graph.door_locks.contains_key(&d);
                        let mut upto: Option<(Space, Vec<[f32; 3]>)> = None;
                        for leg in &path.legs {
                            if let Some(stop) = leg.door_stops.iter().find(|st| locked(st.door)) {
                                upto = Some((leg.space, leg.points[..=stop.point.min(leg.points.len() - 1)].to_vec()));
                                break;
                            }
                            if leg.door.is_some_and(|d| locked(d.door)) {
                                upto = Some((leg.space, leg.points.clone()));
                                break;
                            }
                        }
                        match upto.filter(|(space, pts)| (fast_travel_to(here.space, *space) || capital_world(h, *space)) && pts.len() > 1) {
                            Some((space, pts)) => {
                                let at = Place { space, pos: tail_point(&pts, WALK_TAIL) };
                                info!("pilot: step {}: the way to the locked door is {:.0} units: starts {WALK_TAIL:.0} units before the door (a stand-in for fast travel)", p.i, path.length);
                                h.put(at);
                                p.followers_due = Some(p.now);
                                w.seen = (p.now, at.pos);
                            }
                            // Nothing to cut that a player could fast travel to: the
                            // way is walked (`w.cut` stays set, so the next frame plans it).
                            None => info!("pilot: step {}: the way to the locked door is {:.0} units, none of it a fast travel away: walked", p.i, path.length),
                        }
                        return Walked::Going;
                    }
                    Ok(path) => {
                        if !w.locked_way {
                            info!("pilot: step {}: the only way to ({:.0}, {:.0}, {:.0}) is through a locked door: the player goes as far as it can", p.i, w.goal.pos[0], w.goal.pos[1], w.goal.pos[2]);
                        }
                        w.locked_way = true;
                        w.snapped = path.end();
                        w.follower = Some(Follower::new(path));
                        w.seen = (p.now, feet);
                    }
                    Err(_) => {
                        // A door out of the world was refused: another one into the same place
                        // that a switch enables is the way (a lift: its button, then its ride).
                        let gone: Vec<u32> = w.tried.iter().filter(|(_, l)| l.starts_with("not in the world")).map(|(d, _)| *d).collect();
                        if let Some((door, (r, cell, pos, away, _))) = h.game(|g| gone.iter().find_map(|&d| door_instead(g, d))).flatten() {
                            if !w.poked.iter().any(|(_, x)| *x == door) {
                                w.poked.push((r.0, door));
                                let space = cell.and_then(|c| crate::nav::space_of_cell(h.world, c.0)).unwrap_or(space_of(h.location));
                                let name = h.game(|g| g.data.name(r)).unwrap_or_default();
                                info!("pilot: step {}: the way there is door {door:08X}, out of the world until {name} is used ({away:.0} units from the door): the player goes to it", p.i);
                                w.side = Some((r, door, Box::new(walk_to(Place { space, pos }, Some(r), NEAR_TARGET, p.now, feet))));
                                return Walked::Going;
                            }
                            w.enable_wait.get_or_insert_with(|| {
                                info!("pilot: step {}: door {door:08X} is not in the world yet after the player used its switch: waits for it (up to {ENABLE_WAIT:.0} s)", p.i);
                                p.now
                            });
                        }
                        // Waited for, the way planned again each frame: the door comes into the
                        // world a frame or more before the navmesh its ride enables reaches the
                        // graph (the silo lift's far door and navmesh 0x0100A556).
                        if w.enable_wait.is_some_and(|since| p.now - since < ENABLE_WAIT) {
                            (h.intent.forward, h.intent.right) = (0.0, 0.0);
                            w.seen = (p.now, feet);
                            return Walked::Going;
                        }
                        // Two walkable areas of one interior that no navmesh link joins, lying
                        // close: the player steps across, as it walks up stairs (SSHQ01's prize
                        // room, #3473: the original's followers do not cross there, observer
                        // check 279; the room is the player's alone).
                        if here.space == w.goal.space && matches!(here.space, Space::Interior(_)) && w.steps < STEPS_ACROSS && w.tried.is_empty() && matches!(e, fnv_nav::PathError::Unreachable { .. }) {
                            let area = |at: [f32; 3]| nav.graph.locate(here.space, at, 96.0).or_else(|| nav.graph.nearest(here.space, at, GOAL_SNAP).map(|(t, _)| t)).map(|t| nav.graph.area[t as usize]);
                            if let (Some(mine), Some(theirs)) = (area(feet), area(w.goal.pos)) {
                                if let Some((from, to)) = nav.graph.step_between(here.space, mine, theirs, STEP_ACROSS, STEP_RISE) {
                                    w.steps += 1;
                                    info!("pilot: step {}: no navmesh link joins the player's part of {:?} with the goal's: steps across where they lie close, ({:.0}, {:.0}, {:.0}) to ({:.0}, {:.0}, {:.0})", p.i, here.space, from[0], from[1], from[2], to[0], to[1], to[2]);
                                    w.step = Some(StepAcross { from, to, area: theirs, walk: Box::new(walk_to(Place { space: here.space, pos: from }, None, 24.0, p.now, feet)), since: None });
                                    w.seen = (p.now, feet);
                                    return Walked::Going;
                                }
                            }
                        }
                        // Tale of Two Wastelands: no door joins the Mojave and the Capital
                        // Wasteland; its travel moves the player between them by script. Into the
                        // other side the player is put where it goes (a worldspace), or outside
                        // the interior's door that leads to that side, as the mod list's travel
                        // and a fast travel from there would put it (counted in `headless`).
                        let here_side = match here.space {
                            Space::World(a) => h.game(|g| capital_side(g, a)).unwrap_or(false),
                            // An interior whose doors lead out onto the Capital Wasteland's side.
                            Space::Interior(c) => h.game(|g| outside_door(g, c).is_some()).unwrap_or(false),
                        };
                        // Or a way across the Capital Wasteland longer than the search goes
                        // (Vault 112 to the Jefferson Memorial: 400,955 triangles searched and
                        // no end [observed]): a fast travel from the side the player is on.
                        let too_far = !w.gapped && matches!(e, fnv_nav::PathError::TooFar { .. });
                        // No navmesh joins the two at all, locked doors or not, and no locked
                        // door was met: a gap a player crosses by something the navmesh does
                        // not hold (Rivet City's swing bridge, RCExBridge1Ref, swung by the
                        // Security Intercom; `fnvtool nav path` finds no way from the shore to
                        // the deck [observed], #2847): a fast travel there, with a finding.
                        let gap = here_side && !too_far && !w.gapped && w.tried.is_empty() && matches!(e, fnv_nav::PathError::Unreachable { .. }) && h.game(|g| match w.goal.space {
                            Space::World(b) => capital_side(g, b),
                            Space::Interior(c) => outside_door(g, c).is_some(),
                        }).unwrap_or(false);
                        // The same between any two places, on the Mojave's side too: a ride a
                        // script gives (the Lucky 38's elevator, a message box whose buttons
                        // MoveTo the player to each floor, SCPT 0x0011206A) joins places no
                        // navmesh or door does. Put at the goal, with the same finding.
                        // Not across the two wastelands: that trip is the mod list's travel below,
                        // with no finding (it was logged as a gap ~35 times a sweep).
                        let goal_side = h.game(|g| match w.goal.space {
                            Space::World(b) => capital_side(g, b),
                            Space::Interior(c) => outside_door(g, c).is_some(),
                        }).unwrap_or(false);
                        let ride = !gap && !too_far && !w.gapped && w.tried.is_empty() && here.space != w.goal.space && here_side == goal_side && matches!(e, fnv_nav::PathError::Unreachable { .. });
                        // A gap inside one interior is crossed inside it: put out of Vault 101's
                        // door by the rule's way out, every later step of the chain failed.
                        let inside = gap && here.space == w.goal.space;
                        let gap = gap || ride;
                        let arrive = if ride || inside { nav.graph.nearest(w.goal.space, w.goal.pos, 4096.0).map(|(_, on)| Place { space: w.goal.space, pos: on }) } else if here_side && !too_far && !gap { None } else {
                            match w.goal.space {
                                Space::World(b) if too_far || h.game(|g| capital_side(g, b)).unwrap_or(false) => nav.graph.nearest(w.goal.space, w.goal.pos, 4096.0).map(|(_, on)| Place { space: w.goal.space, pos: on }),
                                Space::Interior(cell) => h.game(|g| if too_far { outside_door_to(g, cell, |_, _| true) } else { outside_door(g, cell) }).flatten().and_then(|(cell, pos)| {
                                    let space = crate::nav::space_of_cell(h.world, cell)?;
                                    nav.graph.nearest(space, pos, 1024.0).map(|(_, on)| Place { space, pos: on })
                                }),
                                _ => None,
                            }
                        };
                        if w.stay && gap && here.space == w.goal.space {
                            if !w.gapped {
                                info!("pilot: step {}: no navmesh way to the spot to fire from ({:?}): the player {} where it stands", p.i, w.goal.space, if w.hold { "waits for its foe" } else { "fires from" });
                                w.gapped = true;
                            }
                            if w.hold {
                                (h.intent.forward, h.intent.right) = (0.0, 0.0);
                                return Walked::Going;
                            }
                            return Walked::There;
                        }
                        if let Some(at) = arrive {
                            info!("pilot: step {}: {} ({:?}): the player is put at ({:.0}, {:.0}, {:.0}) in {:?}, as the mod list's travel and a fast travel would put it", p.i, if too_far { "a way longer than the search goes" } else if gap { "no navmesh way there" } else { "no way to the Capital Wasteland side" }, w.goal.space, at.pos[0], at.pos[1], at.pos[2], at.space);
                            w.gapped |= too_far || gap;
                            if gap {
                                p.finding("no_navmesh_way", format!("no navmesh way from ({:.0}, {:.0}, {:.0}) in {:?} to {:?}, locked doors or not: put there as a fast travel would", feet[0], feet[1], feet[2], here.space, w.goal.space));
                            }
                            p.headless(if too_far || gap { "move_player (a fast travel)" } else { "move_player (between the two wastelands)" });
                            h.put(at);
                            p.followers_due = Some(p.now);
                            w.seen = (p.now, at.pos);
                            return Walked::Going;
                        }
                        // From a scrap of navmesh that leads nowhere (the search found next to
                        // nothing to expand: a ledge the fight led onto, a bad snap): back where
                        // the last path began, once, as a player drops down or walks back. Old
                        // World Blues' X-8 chased an attacker up a gantry and stopped there
                        // (Unreachable { expanded: 0 }, round 7 [observed]).
                        if matches!(e, fnv_nav::PathError::Unreachable { expanded } if expanded < STRANDED_EXPANDED) && !w.stranded && here.space == w.goal.space && w.tried.is_empty() {
                            // Only to where the goal can be reached from: a goal no way leads to
                            // from anywhere (an attacker on Gomorrah's balcony; the nav's door
                            // search answers Unreachable { expanded: 0 } for it too) is no scrap
                            // of navmesh, and putting the player back undid the way it had
                            // come (ncr step 74, rerun 4 [observed]).
                            if let Some(back) = p.walked_from.filter(|b| b.space == here.space && hdist(b.pos, feet) > 64.0 && nav.graph.find_path(*b, w.goal, &forced).is_ok()) {
                                w.stranded = true;
                                p.finding("stranded", format!("no way on from ({:.0}, {:.0}, {:.0}) in {:?} ({e:?}): put back at ({:.0}, {:.0}, {:.0}), where the last path began", feet[0], feet[1], feet[2], here.space, back.pos[0], back.pos[1], back.pos[2]));
                                p.headless("move_player (back off a scrap of navmesh)");
                                h.put(back);
                                w.seen = (p.now, back.pos);
                                return Walked::Going;
                            }
                        }
                        let tried = if w.tried.is_empty() { "locked doors or not".to_string() } else { format!("the doors tried: {}", w.tried.iter().map(|(d, l)| format!("{d:08X} ({l})")).collect::<Vec<_>>().join(", ")) };
                        let why = format!("no way from ({:.0}, {:.0}, {:.0}) in {:?} to ({:.0}, {:.0}, {:.0}) in {:?} ({e:?}), {tried}: a player could not get there; the player stays where it is", feet[0], feet[1], feet[2], here.space, w.goal.pos[0], w.goal.pos[1], w.goal.pos[2], w.goal.space);
                        p.finding("unreachable", why.clone());
                        return Walked::No(why);
                    }
                }
            }
        }
    }
    // Kept by a locked door's owner until it opens the door (below, Steer::OpenDoor): the
    // path is taken up again then, from wherever following it led the player.
    if let Some(door) = w.owner_door {
        if h.doors.is_open(door) == Some(true) {
            info!("pilot: step {}: door {door:08X} is open now: the player goes through", p.i);
            (w.owner_door, w.owner_wait, w.follower, w.door_since) = (None, None, None, None);
            return Walked::Going;
        }
        match owner_opens(p, &mut w.owner_wait, h, door) {
            Some(owner) => {
                keep_by(h, feet, owner);
                return Walked::Going;
            }
            None => (w.owner_door, w.follower, w.door_since) = (None, None, None),
        }
        return Walked::Going;
    }
    let f = w.follower.as_mut().expect("planned above");
    // In another space than the leg's: through its door, or sent elsewhere.
    if f.space().is_some_and(|s| s != here.space) {
        match f.path.legs.get(f.leg + 1).map(|l| l.space) {
            Some(next) if next == here.space => {
                f.door_used();
                w.door_since = None;
                w.seen = (p.now, feet);
            }
            _ => w.follower = None,
        }
        return Walked::Going;
    }
    match f.steer(feet) {
        Steer::Arrived => return Walked::There,
        Steer::OffPath => {
            w.follower = None;
            w.replans += 1;
        }
        Steer::Walk { dir, target, .. } => {
            w.door_since = None;
            if p.now - w.reported > WALK_REPORT_SECS {
                w.reported = p.now;
                let (leg, next, of) = w.follower.as_ref().map_or((0, 0, 0), |f| (f.leg, f.next, f.path.legs.get(f.leg).map_or(0, |l| l.points.len())));
                info!("pilot: step {}: walking: at ({:.0}, {:.0}, {:.0}), leg {leg} point {next} of {of} at ({:.0}, {:.0}, {:.0}), {:.0} units on (movement locked {}, a menu {}, forward {:.1}, heading off by {:.2})", p.i, feet[0], feet[1], feet[2], target[0], target[1], target[2], hdist(feet, target), h.locks.movement, h.in_menu, h.intent.forward, heading_delta(h.player.heading, heading_of(dir)));
            }
            let want = heading_of(dir);
            h.player.heading = turn_towards(h.player.heading, want, TURN_RATE * h.dt);
            h.player.pitch = turn_towards(h.player.pitch, 0.0, TURN_RATE * h.dt);
            if heading_delta(h.player.heading, want).abs() < 0.7 {
                (h.intent.forward, h.intent.right, h.intent.walk) = (1.0, 0.0, h.control.sneak);
            }
            // Round what it walked into: a step to the side, as a player takes.
            match w.dodge {
                Some((until, side)) if p.now < until => h.intent.right = side,
                _ => w.dodge = None,
            }
            // Held by a script: a wait, and the watchdog's to judge.
            // Freed and well on: the skips are counted afresh too, for the next place it is
            // caught at. Gomorrah's way out is caught at (5598, 2917, 6401) each time a fight
            // drew the player back past it; the walk's three skips went on earlier spots and
            // the fourth time there it gave up (ncr step 74, rerun 4 [observed]).
            if w.stuck_at.is_some_and(|at| hdist(feet, at) > UNSTUCK_UNITS) {
                (w.stuck_at, w.replans, w.skips) = (None, 0, 0);
            }
            // Held up against the load door the leg ends at, before its point on the navmesh:
            // used from here, as a player presses E on the door in front of it. Honest Hearts'
            // Northern Passage door is an invisible slab 512 units wide (DOOR 0x01011470,
            // OBND y -256..256) across the cave mouth; the player stopped 252 units from its
            // middle and replanned into it for the whole run.
            // Or a load door the navmesh does not list, standing across the way to the
            // same place: Honest Hearts puts its own "Entrance" (0x010115A1, XTEL into the
            // Northern Passage beside Jed) 960 units in front of the base game's cave door
            // the path goes to, and the player walks into it.
            let door_ahead = w.follower.as_ref().and_then(|f| f.path.legs.get(f.leg)).and_then(|l| l.door).map(|d| d.door);
            let next_space = w.follower.as_ref().and_then(|f| f.path.legs.get(f.leg + 1)).map(|l| l.space);
            let door_near = match door_ahead.filter(|_| p.now - w.seen.0 > STUCK_SECS && hdist(feet, w.seen.1) <= STUCK_UNITS) {
                Some(d) => {
                    let found = h.game(|g| {
                        let (cell, at) = where_is(g, FormId(d))?;
                        if hdist(feet, at) < DOOR_PRESS_REACH {
                            return Some((d, None));
                        }
                        let doors: Vec<(FormId, FormId)> = g.world.placed.by_cell.get(&cell?)?.iter()
                            .filter_map(|&r| Some((r, g.world.placed.get(r)?.teleport.as_ref()?.door)))
                            .filter(|&(r, _)| r.0 != d && where_is(g, r).is_some_and(|(_, at)| hdist(feet, at) < DOOR_PRESS_REACH))
                            .collect();
                        let mut near: Vec<(u32, FormId)> = Vec::new();
                        for (r, to) in doors {
                            if runner::reference_field(g, &format!("0x{:08X}", r.0), "disabled").ok() == Some(json!(true)) {
                                continue;
                            }
                            if let Some((Some(cell), _)) = where_is(g, to) {
                                near.push((r.0, cell));
                            }
                        }
                        Some((d, Some(near)))
                    }).flatten();
                    match found {
                        Some((d, None)) => Some((d, true)),
                        // Each such door once a walk: one that does not take the player is
                        // left to the replans below.
                        Some((_, Some(near))) => near.into_iter().find(|&(r, cell)| next_space.is_some() && crate::nav::space_of_cell(h.world, cell.0) == next_space && !w.poked.contains(&(r, r))).map(|(r, _)| (r, false)),
                        None => None,
                    }
                }
                None => None,
            };
            if h.locks.movement || h.in_menu || hdist(feet, w.seen.1) > STUCK_UNITS {
                w.seen = (p.now, feet);
            } else if let Some((door, own)) = door_near {
                w.seen = (p.now, feet);
                if own {
                    info!("pilot: step {}: held up short of door {door:08X}'s end of the path, within reach of the door: uses it from here", p.i);
                    if let Some(f) = w.follower.as_mut() {
                        f.next = f.path.legs[f.leg].points.len();
                    }
                } else {
                    w.poked.push((door, door));
                    let r = h.script.rt.as_mut().map(|rt| with_world(rt, h.sim, |rt| rt.activate_as_player(FormId(door))));
                    info!("pilot: step {}: held up by load door {door:08X}, which leads where the path goes: uses it: {}", p.i, outcome(r.as_ref()));
                }
            } else if p.now - w.seen.0 > STUCK_SECS && w.replans >= 2 && w.skips < SKIPS {
                // Two jumps and steps aside did not free it: moved on along its own path,
                // past what it is caught on, as the engine moves a stuck NPC on
                // (`fnv_nav::Follower::skip_ahead`). Within the leg: never past a door.
                let at = w.follower.as_mut().and_then(|f| f.skip_ahead(feet, SKIP_UNITS));
                w.seen = (p.now, feet);
                match at {
                    Some(at) => {
                        w.skips += 1;
                        p.finding("stuck", format!("caught at ({:.0}, {:.0}, {:.0}): moved {SKIP_UNITS:.0} units on along its path, as a stuck NPC is", feet[0], feet[1], feet[2]));
                        h.put(Place { space: here.space, pos: at });
                    }
                    None => w.skips = SKIPS,
                }
            } else if p.now - w.seen.0 > STUCK_SECS {
                w.seen = (p.now, feet);
                w.follower = None;
                w.replans += 1;
                w.stuck_at = Some(feet);
                let side = if w.replans % 2 == 1 { 1.0 } else { -1.0 };
                w.dodge = Some((p.now + 1.2, side));
                h.intent.jump = true;
                let ahead = door_ahead.map(|d| (d, h.game(|g| where_is(g, FormId(d))).flatten().map(|(_, at)| hdist(feet, at))));
                info!("pilot: step {}: not moving at ({:.0}, {:.0}, {:.0}): a jump, a step to the {} and a new path ({} of {REPLANS}); the leg's door {ahead:?}", p.i, feet[0], feet[1], feet[2], if side > 0.0 { "right" } else { "left" }, w.replans);
            }
        }
        Steer::OpenDoor { door, .. } => {
            if h.doors.is_open(door) == Some(true) || h.game(|g| is_disabled(g, door)).unwrap_or(false) {
                f.door_opened();
                w.door_since = None;
            } else if w.door_since.is_none() {
                w.door_since = Some(p.now);
                let r = h.script.rt.as_mut().map(|rt| with_world(rt, h.sim, |rt| rt.activate_as_player(FormId(door))));
                info!("pilot: step {}: opens door {door:08X}: {}", p.i, outcome(r.as_ref()));
            } else if h.locks.movement || h.locks.fighting {
                // A script holds the player's controls (The Pitt's Hole: the arena's script
                // takes fighting and the Pip-Boy, `DisablePlayerControls 0 1 1 0 0 0 0`, while
                // the announcer speaks, then opens the player's gate): what it is doing is
                // waited for, so the door's time counts from when they are back.
                w.door_since = Some(p.now);
            } else if lock_words(nav, door).is_some() && w.door_since.is_some_and(|t| p.now - t > PICK_AFTER_SECS) && pick_lock(p, h, door, false) {
                // Picked: the next frame opens it, as the menu activates the door it opened.
                w.door_since = None;
            } else if lock_words(nav, door).is_some() && w.door_since.is_some_and(|t| p.now - t > LOCKED_SECS) && try_beside(p, &mut w.poked, &mut w.side, &mut w.errand, h, door, feet, w.target) {
                w.door_since = Some(p.now);
                return Walked::Going;
            } else if lock_words(nav, door).is_some() && w.door_since.is_some_and(|t| p.now - t > LOCKED_SECS) && fetch_key(p, &mut w.poked, &mut w.errand, h, nav, door, feet) {
                (w.follower, w.door_since) = (None, None);
                return Walked::Going;
            } else if let Some(owner) = (lock_words(nav, door).is_some() && w.door_since.is_some_and(|t| p.now - t > LOCKED_SECS)).then(|| owner_opens(p, &mut w.owner_wait, h, door)).flatten() {
                // Its owner, near, may open it (Wernher's escort through his fence gate at
                // The Pitt): the player keeps by the owner, as a player follows the one who
                // leads it (an escort waits while the one it leads is more than its radius
                // away, 750 units for Wernher's, PKE2), and goes through when it is open.
                w.owner_door = Some(door);
                keep_by(h, feet, owner);
            } else if lock_words(nav, door).is_some() && w.door_since.is_some_and(|t| p.now - t > LOCKED_SECS) && pick_lock(p, h, door, true) {
                // A lock past the character's skill, only after a switch beside it, its key
                // and its owner were tried: picked at once, Hidden Valley's bunker door let the
                // player in past its intercom, and Ramos's intruder talk took Veronica's
                // escort another way (sweep 2 [observed]).
                w.door_since = None;
            } else if let Some(lock) = lock_words(nav, door).filter(|_| w.door_since.is_some_and(|t| p.now - t > LOCKED_SECS)) {
                info!("pilot: step {}: door {door:08X} on the way is {lock} and nothing opened it in {LOCKED_SECS:.0} s after the player tried it: another way round", p.i);
                w.tried.push((door, lock));
                (w.follower, w.door_since) = (None, None);
                return Walked::Going;
            } else if w.door_since.is_some_and(|t| p.now - t > LOCKED_SECS) && try_beside(p, &mut w.poked, &mut w.side, &mut w.errand, h, door, feet, w.target) {
                // Not locked, but opened from elsewhere: the Fort's Securitron Vault hatch
                // opens from the weather station's console ("Insert the Platinum Chip"),
                // whose script names the hatch.
                w.door_since = Some(p.now);
                return Walked::Going;
            } else if w.door_since.is_some_and(|t| p.now - t > LOCKED_SECS) {
                let parents = h.game(|g| g.world.placed.get(FormId(door)).map(|r| r.activate_parents.len())).flatten().unwrap_or(0);
                p.finding("door", format!("door {door:08X} on the way, not locked, did not open in {LOCKED_SECS:.0} s ({parents} activate parents, none taken): walks on as if it had"));
                f.door_opened();
                w.door_since = None;
            }
        }
        Steer::Door(d) => {
            // Out of the world, its switch used: the player waits at it, as for a lift's ride.
            let gone = h.game(|g| is_disabled(g, d.door)).unwrap_or(false);
            if gone && w.poked.iter().any(|(_, x)| *x == d.door) {
                let since = *w.enable_wait.get_or_insert_with(|| {
                    info!("pilot: step {}: door {:08X} is not in the world yet after the player used its switch: waits for it (up to {ENABLE_WAIT:.0} s)", p.i, d.door);
                    p.now
                });
                if p.now - since < ENABLE_WAIT {
                    (h.intent.forward, h.intent.right) = (0.0, 0.0);
                    w.seen = (p.now, feet);
                    return Walked::Going;
                }
            } else if !gone && w.enable_wait.take().is_some() {
                info!("pilot: step {}: door {:08X} is in the world now: the player goes through", p.i, d.door);
                w.door_since = None;
            }
            if w.door_since.is_none() {
                w.door_since = Some(p.now);
                w.door_from = feet;
                let r = h.script.rt.as_mut().map(|rt| with_world(rt, h.sim, |rt| rt.activate_as_player(FormId(d.door))));
                info!("pilot: step {}: goes through door {:08X}: {}", p.i, d.door, outcome(r.as_ref()));
            } else if hdist(feet, w.door_from) > DOOR_JUMP {
                // Through a load door into the same space: a new path from the far side.
                info!("pilot: step {}: door {:08X} took the player {:.0} units within its own space: a new path from there", p.i, d.door, hdist(feet, w.door_from));
                (w.follower, w.door_since, w.seen) = (None, None, (p.now, feet));
                return Walked::Going;
            } else if h.locks.movement || h.locks.fighting {
                // Controls held by a script: as for a door that opens in place.
                w.door_since = Some(p.now);
            } else if w.door_since.is_some_and(|t| p.now - t > LOCKED_SECS) {
                // Open now (a script unlocked it meanwhile): tried again on a new path.
                let lock = lock_words(nav, d.door);
                if w.locked_way && lock.is_none() {
                    (w.follower, w.door_since) = (None, None);
                    return Walked::Going;
                }
                let Some(lock) = lock else {
                    // A door opened from elsewhere (a vault door's console, a gate's
                    // switch): what names it, as for a locked one.
                    if try_beside(p, &mut w.poked, &mut w.side, &mut w.errand, h, d.door, feet, w.target) {
                        w.door_since = Some(p.now);
                        return Walked::Going;
                    }
                    // Another way round first, as for a locked door: Paradise Falls' sewer
                    // grate 0x0A034A35 turns the player away, and its main gate is the way in
                    // [observed].
                    if !w.tried.iter().any(|(t, _)| *t == d.door) {
                        info!("pilot: step {}: door {:08X} is not locked but did not take the player through in {LOCKED_SECS:.0} s: another way round", p.i, d.door);
                        w.tried.push((d.door, "not locked, refused the player".into()));
                        (w.follower, w.door_since) = (None, None);
                        return Walked::Going;
                    }
                    let why = format!("door {:08X} is not locked and did not take the player through in {LOCKED_SECS:.0} s after the player tried it (its script refuses the player?): a player could not go on; the player stays in front of it", d.door);
                    p.finding("unreachable", why.clone());
                    return Walked::No(why);
                };
                if pick_lock(p, h, d.door, false) {
                    w.door_since = None;
                    return Walked::Going;
                }
                if try_beside(p, &mut w.poked, &mut w.side, &mut w.errand, h, d.door, feet, w.target) {
                    w.door_since = Some(p.now);
                    return Walked::Going;
                }
                if fetch_key(p, &mut w.poked, &mut w.errand, h, nav, d.door, feet) {
                    (w.follower, w.door_since) = (None, None);
                    return Walked::Going;
                }
                // Past the character's skill: stood in once a switch and its key were tried.
                if pick_lock(p, h, d.door, true) {
                    w.door_since = None;
                    return Walked::Going;
                }
                info!("pilot: step {}: door {:08X} is {lock} and nothing opened it in {LOCKED_SECS:.0} s after the player tried it: another way round", p.i, d.door);
                w.tried.push((d.door, lock));
                (w.follower, w.door_since) = (None, None);
                return Walked::Going;
            }
        }
    }
    if w.replans > REPLANS {
        let end = w.snapped.unwrap_or(w.goal.pos);
        if here.space == w.goal.space && hdist(feet, end) <= PUT_WITHIN {
            p.finding("stuck", format!("could not walk the last {:.0} units to ({:.0}, {:.0}, {:.0}) from ({:.0}, {:.0}, {:.0}) after {REPLANS} new paths: set on the place", hdist(feet, end), end[0], end[1], end[2], feet[0], feet[1], feet[2]));
            h.put(Place { space: w.goal.space, pos: end });
            return Walked::Put;
        }
        let why = format!("could not walk to ({:.0}, {:.0}, {:.0}) from ({:.0}, {:.0}, {:.0}) after {REPLANS} new paths: the player stays where it is", w.goal.pos[0], w.goal.pos[1], w.goal.pos[2], feet[0], feet[1], feet[2]);
        p.finding("unreachable", why.clone());
        return Walked::No(why);
    }
    Walked::Going
}

/// The steps after this one (past notes) deal with a conversation: a talk begun on the
/// way to this step's place is theirs to answer.
fn next_answers_talk(p: &Pilot) -> bool {
    p.actions.iter().skip(p.i + 1).find(|a| !matches!(a, Action::Note { .. })).is_some_and(|a| matches!(a, Action::WaitTalk { .. } | Action::Choose { .. } | Action::Talk { .. }))
}

fn walk_to(goal: Place, target: Option<FormId>, radius: f32, now: f64, feet: [f32; 3]) -> Walk {
    Walk { goal, target, radius, follower: None, door_since: None, door_from: feet, seen: (now, feet), replans: 0, cut: false, gapped: false, talking_at_start: None, keep_talk: false, locked_way: false, tried: Vec::new(), skips: 0, awaited: 0, stuck_at: None, poked: Vec::new(), errand: None, side: None, scene_held: 0.0, scene_last: None, told_talking: false, dodge: None, snapped: None, enable_wait: None, owner_wait: None, owner_door: None, reported: now, travel_since: None, stay: false, hold: false, stranded: false, step: None, steps: 0 }
}

/// How long after trying a locked door the player picks its lock.
const PICK_AFTER_SECS: f64 = 1.0;

/// A locked door the player can pick (its Lockpick skill and a bobby pin, as the menu
/// asks, `fnv_game::lockpick::opening`): unlocked as the lockpicking menu's won game
/// unlocks it, with its XP (`fnv_game::lockpick::opened`); the game itself is stood in
/// (logged as a headless step). Vault 101's Overseer's office door, Very Easy, with
/// Amata's bobby pins: the escape stopped at it [observed, 2026-11-20].
/// With `stand_in`, a lock only the skill (or a pin) short of the bot's character is
/// stood in too; without it only one the character can pick.
fn pick_lock(p: &mut Pilot, h: &mut Hands, door: u32, stand_in: bool) -> bool {
    let picked = h.game(|g| {
        let skill = fnv_game::lockpick::skill(g);
        match fnv_game::lockpick::opening(g, FormId(door), skill) {
            fnv_game::lockpick::Opening::Pick { level, skill, pins } => {
                fnv_game::lockpick::opened(g, FormId(door), level, true);
                Some((false, format!("{level:?} lock, Lockpick {skill:.0}, {pins} bobby pin(s)")))
            }
            // Only the skill (or a pin) short: a player who levelled Lockpick, or carries
            // a pin, picks it; the bot's character is not that player, so it is stood in
            // and said (a finding, not a stop: a skill gate is no soft lock).
            fnv_game::lockpick::Opening::Refused(r @ (fnv_game::lockpick::Refusal::Skill { .. } | fnv_game::lockpick::Refusal::NoPins)) if stand_in => {
                let level = fnv_game::lockpick::Level::of(g.world.peek(FormId(door)).and_then(|s| s.lock.map(|l| l.level)).unwrap_or(100));
                fnv_game::lockpick::opened(g, FormId(door), level, true);
                Some((true, format!("{level:?} lock, Lockpick {skill:.0}, refused: {r:?}")))
            }
            _ => None,
        }
    }).flatten();
    match picked {
        Some((short, what)) => {
            if short {
                p.finding("skill_stood_in", format!("door {door:08X}'s lock ({what}): picked as a player with the skill and a bobby pin would"));
            } else {
                info!("pilot: step {}: picks door {door:08X}'s lock ({what}): the lockpicking game stood in", p.i);
            }
            p.headless("lockpick (the minigame stood in)");
            true
        }
        None => false,
    }
}

/// What a player would use to open a door that stays locked (module notes), not used
/// for it yet on this walk: the reference, where it is, how far from the door, and what
/// it is in words.
/// A switch for a door: the reference, its cell, where it is, how far from the door, and
/// what it is in words.
type Switch = (FormId, Option<FormId>, [f32; 3], f32, &'static str);
/// A terminal for a door: the terminal, the menu, the entry, its cell and where it is.
type DoorTerminal = (FormId, FormId, usize, Option<FormId>, [f32; 3]);
/// Where an item was found: how far, the holder or the item, its cell, where, lying.
type Found = (f32, FormId, Option<FormId>, [f32; 3], bool);

/// The plugins of Fallout 3's side of Tale of Two Wastelands: a worldspace whose FormID
/// belongs to one of them is the Capital Wasteland's (its DC ruins are worldspaces of
/// their own).
const CAPITAL_PLUGINS: [&str; 6] = ["Fallout3.esm", "Anchorage.esm", "ThePitt.esm", "BrokenSteel.esm", "PointLookout.esm", "Zeta.esm"];

/// A worldspace of the Capital Wasteland's side (Tale of Two Wastelands): its DC ruins
/// are worldspaces of their own with map markers a player fast travels to, so a long way
/// into one is cut as one into the Mojave is.
fn capital_world(h: &mut Hands, space: Space) -> bool {
    match space {
        Space::World(w) => h.game(|g| capital_side(g, w)).unwrap_or(false),
        Space::Interior(_) => false,
    }
}

/// Whether a worldspace is on the Capital Wasteland's side (Tale of Two Wastelands).
fn capital_side(g: &Game, w: u32) -> bool {
    g.data.load_order.get((w >> 24) as usize).is_some_and(|p| CAPITAL_PLUGINS.iter().any(|c| c.eq_ignore_ascii_case(p)))
}

/// Where the doors of an interior cell let the player out into a worldspace of the
/// Capital Wasteland's side, through up to [`OUTSIDE_DEPTH`] interiors (Underworld opens
/// into the Museum of History, which opens onto the Mall): the destination's cell and
/// position.
fn outside_door(g: &Game, cell: u32) -> Option<(u32, [f32; 3])> {
    outside_door_to(g, cell, capital_side)
}

/// [`outside_door`] into a worldspace `side` takes: any, for a way longer than the search
/// goes on the Mojave's side (Old Lady Gibson's shack to the Jacobstown lodge, 400,012
/// triangles and no end [observed]).
fn outside_door_to(g: &Game, cell: u32, side: fn(&Game, u32) -> bool) -> Option<(u32, [f32; 3])> {
    // A door whose script runs on activation is a way only when no other is (GNR's back
    // door from Dupont Circle, DuponttoGNRRef, only says it is barred:
    // MQ02GNRBackDoorScript [observed]), so a plain door out is taken first.
    let mut scripted = None;
    let mut seen = vec![cell];
    let mut cells = vec![cell];
    for _ in 0..OUTSIDE_DEPTH {
        let mut next = Vec::new();
        for c in cells {
            let Some(refs) = g.world.placed.by_cell.get(&FormId(c)) else { continue };
            for &r in refs {
                let Some(to) = g.world.placed.get(r).and_then(|p| p.teleport.as_ref()).map(|t| t.door) else { continue };
                let Some((Some(dest), pos)) = where_is(g, to) else { continue };
                match g.world.placed.world_of(dest) {
                    Some(w) if side(g, w.0) && (door_scripted(g, to.0) || door_scripted(g, r.0)) => {
                        scripted.get_or_insert((dest.0, pos));
                    }
                    Some(w) if side(g, w.0) => return Some((dest.0, pos)),
                    Some(_) => {}
                    None if !seen.contains(&dest.0) => {
                        seen.push(dest.0);
                        next.push(dest.0);
                    }
                    None => {}
                }
            }
        }
        cells = next;
    }
    scripted
}

/// How many interiors deep [`outside_door`] looks for a way out.
const OUTSIDE_DEPTH: usize = 4;

/// Whether a door has a script with an OnActivate block (it does something when used).
fn door_scripted(g: &Game, door: u32) -> bool {
    g.ref_script(FormId(door)).and_then(|s| g.data.scripts.get(&s)).is_some_and(|u| g.data.unit(*u).program.blocks.iter().any(|b| b.event == fnv_game::game::event::ON_ACTIVATE))
}

/// What [`door_switch`] calls a reference whose activation script names the door.
const WORKS_DOOR: &str = "its script works the door";

/// How long a door that is not in the world is waited for after its switch was used: a
/// lift's ride (Lonesome Road's Ashton silo lift enables its far door 75 s after the
/// button, script 0x01003217).
const ENABLE_WAIT: f64 = 150.0;

/// Whether a reference is out of the world (disabled) now.
fn is_disabled(g: &mut Game, r: u32) -> bool {
    runner::reference_field(g, &format!("0x{r:08X}"), "disabled").ok() == Some(json!(true))
}

/// Another door out of the world into the place a refused door leads to, with a switch
/// whose script enables it: the door and its switch. Lonesome Road's Ashton silo: the direct
/// door (0x0100CB7D) is disabled; the lift's far door (0x01007877, on a navmesh the ride
/// enables) leads to the same silo cell, and the lift's button names it.
fn door_instead(g: &mut Game, gone: u32) -> Option<(u32, Switch)> {
    let to = g.world.placed.get(FormId(gone))?.teleport.as_ref()?.door;
    let (cell, _) = where_is(g, to)?;
    let others: Vec<u32> = g.world.placed.by_cell.get(&cell?)?.iter().filter_map(|&r| Some(g.world.placed.get(r)?.teleport.as_ref()?.door.0)).filter(|&d| d != gone).collect();
    for d in others {
        if !is_disabled(g, d) {
            continue;
        }
        if let Some(s) = door_switch(g, d, &[]).filter(|s| s.4 == WORKS_DOOR) {
            return Some((d, s));
        }
    }
    None
}

/// Whether a script opens a menu of its own when used (a workbench's ShowRecipeMenu): not a
/// door's switch. Beside the locked door of Raul's cell at Black Mountain the pilot used a
/// workbench (SCPT 0x0013E11C) and the recipe menu held the run [observed 2026-11-18].
fn opens_a_menu(g: &Game, program: &fnv_script::ir::Program) -> bool {
    let mut calls = std::collections::BTreeMap::new();
    fnv_game::intro::calls(program, &mut calls);
    ["ShowRecipeMenu", "ShowBarterMenu", "ShowRepairMenu", "ShowEnchantment"].iter().filter_map(|n| g.data.table.by_name(n)).any(|op| calls.contains_key(&op))
}

fn door_switch(g: &Game, door: u32, used: &[(u32, u32)]) -> Option<Switch> {
    use fnv_script::unit::RefSlot;
    if used.iter().filter(|(_, d)| *d == door).count() >= SWITCHES {
        return None;
    }
    let (cell, at) = where_is(g, FormId(door))?;
    let mut found: Vec<(u8, f32, FormId, [f32; 3], &'static str)> = Vec::new();
    // The door's activate parents (XAPR) set it off: a door that only they open (XAPD,
    // Vault 112's floor hatch Vault112SecretHatchREF under the Tenpenny wall switch
    // [observed]) refuses the player. A parent that is a trigger volume is not a switch.
    let parents: Vec<FormId> = g.world.placed.get(FormId(door)).map(|p| p.activate_parents.iter().map(|a| a.reference).collect()).unwrap_or_default();
    for r in parents {
        if used.iter().any(|(u, d)| *u == r.0 && *d == door) || g.world.placed.get(r).is_none_or(|p| p.primitive.is_some()) {
            continue;
        }
        let Some((_, pos)) = where_is(g, r) else { continue };
        found.push((0, hdist(pos, at).hypot(pos[2] - at[2]), r, pos, "the door's activate parent"));
    }
    let scripted = |r: FormId| {
        let unit = g.data.scripts.get(&g.ref_script(r)?).map(|u| g.data.unit(*u))?;
        unit.program.blocks.iter().any(|b| b.event == fnv_game::game::event::ON_ACTIVATE).then_some(unit)
    };
    // What the scripts that work the door also name (their variables' holders, the
    // switches they watch): a switch whose script names one of these works the door
    // through them. Broken Steel's turret hall: its trigger's script locks and unlocks
    // the doors by `turrethallonline` of 0x0D0049D6, which only the hall switch's script
    // sets to 3 [observed: SCPT 0x0D0049D9, 0x0D0049DD]; the pilot went to the hall's
    // terminals beyond the locked doors instead.
    let mut through: Vec<u32> = Vec::new();
    for u in g.data.scripts.values() {
        let unit = g.data.unit(*u);
        if unit.unit.refs.iter().any(|s| matches!(s, RefSlot::Form(f) if *f == door)) {
            through.extend(unit.unit.refs.iter().filter_map(|s| match s {
                RefSlot::Form(f) if *f != door && *f != FormId::PLAYER_REF.0 => Some(*f),
                _ => None,
            }));
        }
    }
    through.sort_unstable();
    through.dedup();
    let refs: &[FormId] = cell.and_then(|c| g.world.placed.by_cell.get(&c)).map(|v| v.as_slice()).unwrap_or(&[]);
    for &r in refs {
        // Not a trigger box (an XPRM primitive) either, as for the activate parents: the
        // dam's Legion battle triggers, "used" as Oliver's door's switch, set the battle
        // state, Oliver fought and died, and the Lanius scene held the player's controls
        // (dam-panel, rerun 3 [observed]).
        if r.0 == door || used.iter().any(|(u, d)| *u == r.0 && *d == door) || g.is_dead(r) || g.world.placed.get(r).is_some_and(|p| p.primitive.is_some()) {
            continue;
        }
        let Some((_, pos)) = where_is(g, r) else { continue };
        let away = hdist(pos, at).hypot(pos[2] - at[2]);
        let unit = scripted(r);
        let names_door = unit.is_some_and(|u| u.unit.refs.iter().any(|s| matches!(s, RefSlot::Form(f) if *f == door)));
        let actor = g.world.placed.get(r).is_some_and(|p| g.data.actors.contains_key(&p.base));
        let works_through = unit.is_some_and(|u| !opens_a_menu(g, &u.program) && u.unit.refs.iter().any(|s| matches!(s, RefSlot::Form(f) if *f != r.0 && through.binary_search(f).is_ok())));
        if names_door && away < SWITCH_RANGE {
            found.push((0, away, r, pos, WORKS_DOOR));
        } else if works_through && !actor && away < SWITCH_RANGE {
            found.push((1, away, r, pos, "its script works the door's own script"));
        } else if g.is_talking_activator(r) && away < BESIDE_DOOR {
            found.push((1, away, r, pos, "a talking activator beside the door"));
        } else if unit.is_some_and(|u| !opens_a_menu(g, &u.program)) && !actor && away < BESIDE_DOOR {
            found.push((2, away, r, pos, "a scripted activator beside the door"));
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    found.first().map(|&(_, away, r, pos, what)| (r, cell, pos, away, what))
}

/// A terminal of the door's cell with an entry whose result script names the door: the
/// terminal, the menu the entry is on, the entry, and where the terminal is.
fn door_terminal(g: &Game, door: u32, used: &[(u32, u32)]) -> Option<DoorTerminal> {
    use fnv_script::unit::RefSlot;
    let (cell, at) = where_is(g, FormId(door))?;
    let refs = g.world.placed.by_cell.get(&cell?)?;
    let mut found: Vec<(f32, FormId, FormId, usize, [f32; 3])> = Vec::new();
    for &r in refs {
        let Some(base) = g.world.placed.get(r).map(|pl| pl.base).filter(|b| g.data.terminals.contains_key(b)) else { continue };
        if used.iter().any(|(u, d)| *u == r.0 && *d == door) {
            continue;
        }
        let Some((_, pos)) = where_is(g, r) else { continue };
        // A terminal linked to the door: an entry that unlocks the linked reference
        // (`getlinkedref().unlock`: Vault 22's food cave, Black Mountain's prison cell).
        let linked = g.linked_ref(r) == Some(FormId(door));
        // The terminal's own menu and the sub menus its entries open, one level down.
        let mut menus = vec![base];
        menus.extend((0..fnv_game::terminal::item_count(g, base)).filter_map(|i| fnv_game::terminal::item(g, base, i).and_then(|it| it.submenu)));
        for menu in menus {
            for i in 0..fnv_game::terminal::item_count(g, menu) {
                let names = fnv_game::terminal::item(g, menu, i).and_then(|it| it.unit).is_some_and(|u| {
                    let unit = g.data.unit(u);
                    unit.unit.refs.iter().any(|s| matches!(s, RefSlot::Form(f) if *f == door)) || linked && {
                        let mut calls = std::collections::BTreeMap::new();
                        fnv_game::intro::calls(&unit.program, &mut calls);
                        calls.contains_key(&UNLOCK)
                    }
                });
                if names {
                    found.push((hdist(pos, at), r, menu, i, pos));
                }
            }
        }
    }
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    found.first().map(|&(_, r, menu, i, pos)| (r, menu, i, cell, pos))
}

/// A door stays locked: what opens it is used, or walked to first (`Walk::side`).
/// Whether there was something to use.
#[allow(clippy::too_many_arguments)]
fn try_beside(p: &Pilot, poked: &mut Vec<(u32, u32)>, side: &mut Option<(FormId, u32, Box<Walk>)>, errands: &mut Option<(Errand, Place)>, h: &mut Hands, door: u32, feet: [f32; 3], target: Option<FormId>) -> bool {
    let used = poked.clone();
    let switch = h.game(|g| door_switch(g, door, &used)).flatten();
    // A terminal entry that names the door comes before an activator that only stands
    // beside it: the Temple of the Union's maintenance terminal unlocks Ulysses' door
    // (0x010084CF), and the commissary terminal and Auto-Doc beside it were tried for good.
    let terminal = if switch.as_ref().is_none_or(|s| s.4 != WORKS_DOOR) { h.game(|g| door_terminal(g, door, &used)).flatten() } else { None };
    let Some((r, cell, pos, away, what)) = switch.filter(|_| terminal.is_none()) else {
        // No switch: a terminal with an entry for the door is an errand of its own.
        let Some((terminal, menu, item, cell, pos)) = terminal else { return false };
        // The walk's own goal: going to it as an errand loops round the door it opens
        // (Beyond the Beef's freezer terminal stands 95 units from the freezer door and
        // snaps to the freezer's side). Nothing to use: the walk gives the door up and the
        // step reaches the terminal from this side (rerun 3 [observed]).
        if Some(terminal) == target {
            return false;
        }
        poked.push((terminal.0, door));
        let space = cell.and_then(|c| crate::nav::space_of_cell(h.world, c.0)).unwrap_or(space_of(h.location));
        let name = h.game(|g| g.data.name(terminal)).unwrap_or_default();
        info!("pilot: step {}: door {door:08X} stays locked: the terminal {name} has an entry for it: the player goes to it", p.i);
        *errands = Some((Errand::Terminal { reference: terminal, base: menu, item, door }, Place { space, pos }));
        return true;
    };
    poked.push((r.0, door));
    let name = h.game(|g| g.data.name(r)).unwrap_or_default();
    if hdist(pos, feet) > 2.0 * NEAR_TARGET {
        let space = cell.and_then(|c| crate::nav::space_of_cell(h.world, c.0)).unwrap_or(space_of(h.location));
        info!("pilot: step {}: door {door:08X} stays locked: the player goes to {name} ({what}, {away:.0} units from the door)", p.i);
        *side = Some((r, door, Box::new(walk_to(Place { space, pos }, Some(r), NEAR_TARGET, p.now, feet))));
        return true;
    }
    let done = h.script.rt.as_mut().map(|rt| with_world(rt, h.sim, |rt| rt.activate_as_player(r)));
    info!("pilot: step {}: door {door:08X} stays locked: the player uses {name} ({what}, {away:.0} units from the door): {}", p.i, outcome(done.as_ref()));
    true
}

/// A locked door's key the player does not carry: fetched as a player fetches it, from a
/// body, a container or where it lies (`find_item`, as an `add_item` step finds an item),
/// once a walk and door; the walk plans again after the errand. A key only a living actor
/// carries is not taken (a quest that hands it over would be the way): a finding names
/// the holder. Whether there was a key to fetch.
fn fetch_key(p: &mut Pilot, poked: &mut Vec<(u32, u32)>, errand: &mut Option<(Errand, Place)>, h: &mut Hands, nav: &crate::nav::NavState, door: u32, feet: [f32; 3]) -> bool {
    let Some(key) = nav.graph.door_locks.get(&door).and_then(|l| l.key) else { return false };
    if poked.contains(&(key, door)) {
        return false;
    }
    poked.push((key, door));
    let world = h.world;
    let look = h.game(|g| {
        if g.world.item_count(FormId::PLAYER_REF, FormId(key)).unwrap_or(0) > 0 {
            return None;
        }
        let holder = g.world.changed().find(|(r, st)| *r != FormId::PLAYER_REF && g.data.actors.contains_key(&st.base) && !g.is_dead(*r) && st.inventory.as_ref().is_some_and(|inv| inv.iter().any(|(i, n)| i == FormId(key) && n > 0))).map(|(r, _)| r);
        Some((find_item(g, FormId(key), feet), holder, g.data.name(FormId(key))))
    }).flatten();
    let Some((found, holder, name)) = look else { return false };
    match found {
        Some((from, cell, pos, lying)) => {
            let space = cell.and_then(|c| crate::nav::space_of_cell(world, c.0)).unwrap_or(space_of(h.location));
            info!("pilot: step {}: door {door:08X} wants {name} ({key:08X}): the player goes for it", p.i);
            *errand = Some((Errand::Take { from, item: FormId(key), count: 1, lying }, Place { space, pos }));
            true
        }
        None => {
            if let Some(who) = holder {
                let them = h.game(|g| g.data.name(who)).unwrap_or_default();
                p.finding("key_held", format!("door {door:08X} wants {name} ({key:08X}), which only {them} ({:08X}), alive, carries: no player way to it here", who.0));
            }
            false
        }
    }
}

/// A step towards `owner` while the player is more than [`OWNER_KEEP`] from it.
fn keep_by(h: &mut Hands, feet: [f32; 3], owner: [f32; 3]) {
    if hdist(feet, owner) > OWNER_KEEP {
        let want = heading_of([owner[0] - feet[0], owner[1] - feet[1]]);
        h.player.heading = turn_towards(h.player.heading, want, TURN_RATE * h.dt);
        if heading_delta(h.player.heading, want).abs() < 0.7 {
            (h.intent.forward, h.intent.right, h.intent.walk) = (1.0, 0.0, h.control.sneak);
        }
    }
}

/// How long a `talk` step waits while a script holds the player's movement, seconds.
const TALK_HELD_SECS: f64 = 30.0;

/// How far from a locked door its owner may be for the walk to wait for it, game units.
const OWNER_NEAR: f32 = 6000.0;
/// How long a walk waits at a locked door for its owner to open it, seconds.
const OWNER_WAIT: f64 = 120.0;
/// How near the owner the player keeps while it waits, game units.
const OWNER_KEEP: f32 = 250.0;

/// Whether the walk waits on at a locked door for its owner: an actor that counts as the
/// door's owner (its base, or a faction at the rank asked for; not the cell's), alive and
/// within [`OWNER_NEAR`] of it, passes through it as if it carried the key (the actors'
/// own rule, `fnv_ai::carry`), and a player walking behind such an owner goes through
/// while it is open (The Pitt: Wernher's escort leads the player through his locked
/// fence gate PittFenceGateRef, whose far side holds the arrival trigger). Up to
/// [`OWNER_WAIT`] s; a finding when it did not open. Where the owner is, while waited for.
fn owner_opens(p: &mut Pilot, since: &mut Option<f64>, h: &mut Hands, door: u32) -> Option<[f32; 3]> {
    if since.is_some_and(|t| p.now - t > OWNER_WAIT) {
        return None;
    }
    let owner = h.game(|g| {
        let o = g.ownership(FormId(door)).filter(|o| !o.from_cell)?;
        let (_, at) = where_is(g, FormId(door))?;
        g.world.changed().find(|(r, st)| *r != FormId::PLAYER_REF && g.data.actors.contains_key(&st.base) && !g.is_dead(*r) && hdist(st.pos, at) < OWNER_NEAR && g.counts_as_owner(*r, &o)).map(|(r, st)| (r, g.data.name(r), st.pos))
    }).flatten();
    let Some((who, name, pos)) = owner else {
        if since.take().is_some() {
            p.finding("owner_gone", format!("door {door:08X}'s owner is no longer near it: the walk goes on without it"));
        }
        return None;
    };
    if since.is_none() {
        info!("pilot: step {}: door {door:08X} is locked, and its owner {name} ({:08X}) is near: the player waits for it to open the door (up to {OWNER_WAIT:.0} s)", p.i, who.0);
        *since = Some(p.now);
    } else if p.now - since.unwrap_or(p.now) > OWNER_WAIT {
        p.finding("owner_did_not_open", format!("door {door:08X}'s owner {name} ({:08X}) did not open it in {OWNER_WAIT:.0} s", who.0));
        return None;
    }
    Some(pos)
}

/// A door's lock as the nav graph has it now, in words; `None`: not locked.
fn lock_words(nav: &crate::nav::NavState, door: u32) -> Option<String> {
    nav.graph.door_locks.get(&door).map(|l| match l.key {
        Some(k) => format!("locked, level {}, key {k:08X}", l.level),
        None => format!("locked, level {}, no key", l.level),
    })
}

fn new_walk(goal: Place, target: Option<FormId>, radius: f32, now: f64, feet: [f32; 3]) -> Phase {
    Phase::Walk(walk_to(goal, target, radius, now, feet))
}

/// The nearest living actor in combat whose target is the player or `friend`, within
/// [`THREAT_RANGE`] of `feet`, and where it is.
fn threat(g: &Game, friend: Option<FormId>, feet: [f32; 3], given_up: &[FormId]) -> Option<(FormId, Option<FormId>, [f32; 3])> {
    g.awareness
        .actors
        .iter()
        .filter(|(a, _)| **a != FormId::PLAYER_REF && Some(**a) != friend && !given_up.contains(a) && !g.is_dead(**a) && g.awareness.state(**a) == fnv_game::awareness::State::Combat)
        .filter(|(_, w)| w.combat_target().is_some_and(|(t, _)| t == FormId::PLAYER_REF || Some(t) == friend))
        // Not one running away on a Flee package: a player does not shoot a man who runs.
        // Mr. Soren's line starts combat with the player and his flee package; shot, his
        // OnHit turned the Van Graffs hostile and failed Birds of a Feather (round 13
        // [observed]).
        .filter(|(a, _)| !fleeing(g, **a))
        // Not one no weapon can hurt (Invulnerable, a ghost, a child): a player walks
        // past. Dead Money's casino holograms are Invulnerable (CREA 0x0100B04A ACBS
        // [observed]); the bot fought them, and a flamer blast on Dog set off the gas
        // trap whose script kills the player (SCPT 0x01010DAB, round 17 [observed]).
        .filter(|(a, _)| !g.is_immune(**a))
        .filter_map(|(a, _)| where_is(g, *a).map(|(cell, pos)| (*a, cell, pos)))
        .filter(|(_, _, pos)| hdist(*pos, feet) < THREAT_RANGE)
        .min_by(|a, b| hdist(a.2, feet).total_cmp(&hdist(b.2, feet)))
}

/// The placed references whose base's script counts the player into a collar radio's
/// radius: it declares `binradius`, `bactive` and `bdestroyed` (Dead Money's speaker
/// script, SCPT 0x01006E8E [observed: `fnvtool script decompile`]).
fn speakers(g: &Game) -> Vec<FormId> {
    let counts = |base: FormId| {
        g.data.script_of_base(base).and_then(|s| g.data.script(s)).is_some_and(|s| ["binradius", "bactive", "bdestroyed"].iter().all(|v| s.var_index(v).is_some()))
    };
    let mut bases: std::collections::HashMap<FormId, bool> = std::collections::HashMap::new();
    g.world.placed.refs.values().filter(|pl| *bases.entry(pl.base).or_insert_with(|| counts(pl.base))).map(|pl| pl.id).collect()
}

/// Of `speakers`, the nearest one counting the player into its radius, still on and not
/// destroyed: the one the player's collar beeps for. Its OnActivate by the player switches
/// it off and counts the player out (SCPT 0x01006E8E [observed]), as a player does.
fn beeping_speaker(g: &Game, speakers: &[FormId], feet: [f32; 3]) -> Option<FormId> {
    let on = |r: FormId, v: &str| g.ref_var_named(r, v) == Some(1.0);
    speakers
        .iter()
        .filter(|r| on(**r, "binradius") && on(**r, "bactive") && !on(**r, "bdestroyed") && g.world.is_enabled(**r).unwrap_or(true))
        .filter_map(|r| where_is(g, *r).map(|(_, pos)| (*r, hdist(pos, feet))))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(r, _)| r)
}

/// Every other speaker still counting the player in, switched off where it is: the
/// collar's 10 s run on while any one does, so a second one in range used up the rest
/// after the first one's five seconds (Christine's floor, round 24 [observed]).
fn silence_the_rest(p: &mut Pilot, h: &mut Hands) {
    let list = p.speakers.clone().unwrap_or_default();
    for _ in 0..8 {
        let feet = h.feet();
        let Some(next) = h.game(|g| beeping_speaker(g, &list, feet)).flatten() else { break };
        // One no player can switch off is walked away from instead (the next check).
        if h.game(|g| g.extra.get(&next).is_some_and(|e| e.destroyed)) == Some(true) {
            break;
        }
        let done = h.script.rt.as_mut().map(|rt| with_world(rt, h.sim, |rt| rt.activate_as_player(next)));
        info!("pilot: step {}: the speaker {next} counts the player in too: switched off where it is: {}", p.i, outcome(done.as_ref()));
        p.errands.retain(|e| !matches!(e, Errand::Silence(s) if *s == next));
        if h.game(|g| g.ref_var_named(next, "binradius") == Some(1.0)) == Some(true) {
            break;
        }
    }
}

/// Whether actor `a` is running away: on a Flee Not Combat package, or fleeing in its
/// fight (a Cowardly actor flees at once: Mr. Soren, whose `StartCombat player` is his way
/// out, was shot before his flee package came, round 16 [observed]).
fn fleeing(g: &Game, a: FormId) -> bool {
    g.awareness.tactics.flights.contains_key(&a) || g.extra.get(&a).and_then(|e| e.current_package).and_then(|pk| g.package_def(&pk)).is_some_and(|d| d.ty == fnv_game::package::PackageType::FleeNotCombat)
}

/// Answer the level-up menu for the player: the level's skill points spent one at a time
/// down the list, the first perks on offer taken at a perk level. What was done, in words.
fn level_up(g: &mut Game) -> String {
    use fnv_game::levelup::{perk_due, PerkSheet, SkillSheet};
    let level = g.player_level().round() as i32;
    let mut skills = SkillSheet::of_player(g, level);
    let n = skills.skills.len();
    let (mut i, mut refused) = (0, 0);
    while skills.remaining() > 0 && refused < n && n > 0 {
        refused = if skills.raise(i % n) { 0 } else { refused + 1 };
        i += 1;
    }
    let spent = skills.points - skills.remaining();
    skills.apply(g);
    let mut taken = 0;
    if perk_due(g, level) {
        let mut perks = PerkSheet::of_player(g, level);
        for perk in perks.perks.clone() {
            if perks.left() == 0 {
                break;
            }
            if perks.can_take(perk) && perks.toggle(perk) {
                taken += 1;
            }
        }
        perks.apply(g);
    }
    format!("level {level}, {spent} skill points spent, {taken} perk(s) taken")
}

/// Give the player the pilot's weapon and ammo, once a run.
fn arm(p: &mut Pilot, h: &mut Hands) {
    if p.armed {
        return;
    }
    p.armed = true;
    let ids = h.game(|g| (runner::resolve(g, PILOT_WEAPON), runner::resolve(g, PILOT_AMMO)));
    match ids {
        Some((Ok(weapon), Ok(ammo))) => {
            let w = &mut h.sim.world;
            let r = w.add_item(FormId::PLAYER_REF, ammo, 500).and_then(|_| w.add_item(FormId::PLAYER_REF, weapon, 1)).and_then(|_| w.equip(FormId::PLAYER_REF, weapon));
            match r {
                Ok(_) => info!("pilot: the player is given {PILOT_WEAPON} and 500 {PILOT_AMMO} to fight with"),
                Err(e) => p.finding("fight", format!("could not arm the player with {PILOT_WEAPON}: {e}")),
            }
        }
        _ => p.finding("fight", format!("this load order has no {PILOT_WEAPON} or {PILOT_AMMO}: the player fights with what it holds")),
    }
}

/// The weapon in the player's hand is whole, and it is the pilot's (module notes).
fn keep_weapon(p: &mut Pilot, h: &mut Hands, fighting_for: f64) {
    if !p.god || p.now < p.weapon_check {
        return;
    }
    p.weapon_check = p.now + WEAPON_SECS;
    let (_, name, ammo_name) = *ARMS.iter().rev().find(|(t, _, _)| fighting_for >= *t).unwrap_or(&ARMS[0]);
    let Some(Ok(weapon)) = h.game(|g| runner::resolve(g, name)) else { return };
    let ammo = h.game(|g| runner::resolve(g, ammo_name).ok()).flatten();
    let w = &mut h.sim.world;
    if let Some(ammo) = ammo.filter(|a| w.item_count(FormId::PLAYER_REF, *a).unwrap_or(0) < 50) {
        let _ = w.add_item(FormId::PLAYER_REF, ammo, 200);
    }
    let held = w.inventory_mut(FormId::PLAYER_REF).is_ok_and(|inv| inv.equipped().into_iter().any(|i| i == weapon));
    if !held {
        // A broken one stays in the inventory and cannot be equipped: a new one.
        let carried = w.item_count(FormId::PLAYER_REF, weapon).unwrap_or(0);
        let r = (if carried > 0 { w.remove_item(FormId::PLAYER_REF, weapon, carried).map(|_| ()) } else { Ok(()) }).and_then(|_| w.add_item(FormId::PLAYER_REF, weapon, 1)).and_then(|_| w.equip(FormId::PLAYER_REF, weapon));
        match r {
            Ok(_) => info!("pilot: step {}: the player now fights with {name} ({fighting_for:.0} s into the fight)", p.i),
            Err(e) => warn!("pilot: step {}: could not give the player {name}: {e}", p.i),
        }
    }
    if let Ok(inv) = w.inventory_mut(FormId::PLAYER_REF) {
        for item in inv.equipped() {
            inv.map_equipped(item, |e| fnv_sim::inventory::Extra { condition: Some(1.0), ..e });
        }
    }
}

/// The actors a shot from `eye` to `at` would meet before `target`, nearest first: those
/// standing in the line of fire. Walls are `combat_tactics::shot_clear`'s.
fn people_in_the_way(phys: &mut PhysState, eye: [f32; 3], at: [f32; 3], target: FormId) -> Vec<u32> {
    use fnv_phys::world::{Filter, Owner};
    let d = [at[0] - eye[0], at[1] - eye[1], at[2] - eye[2]];
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if len < 1.0 {
        return Vec::new();
    }
    let mut filter = Filter::default();
    filter.exclude_layers.insert(fnv_phys::layers::TRIGGER);
    filter.exclude_layers.insert(fnv_phys::layers::INVISIBLE_WALL);
    let hits = phys.ray_all(eye, [d[0] / len, d[1] / len, d[2] / len], len, filter);
    // The actors along the line, nearest first, up to the target itself.
    let mut out: Vec<u32> = Vec::new();
    for id in hits.iter().filter_map(|h| match h.owner {
        Owner::Actor { form_id, .. } => Some(form_id),
        Owner::Character { id } => Some(id),
        _ => None,
    }) {
        if id == target.0 {
            break;
        }
        if !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

/// One frame of shooting at an actor: nearer while it is out of range, the view on its
/// trunk and the trigger pulled when in range. `Some(why)`: the actor cannot be reached.
fn engage(p: &mut Pilot, h: &mut Hands, name: &str, f: &mut Fight, w: &mut Walk) -> Option<String> {
    keep_weapon(p, h, p.now - f.began);
    let target = f.target;
    h.control.weapon_out = true;
    // In range with a clear line: it shoots from where it stands, as a player does, and
    // does not chase a target that moves about (Joe Cobb ran round the saloon and the walk
    // was planned again and again, the trigger never pulled).
    let (reach, rise) = if f.afar { (LONG_SHOT, LONG_SHOT) } else { (w.radius.min(SHOOT_FROM), 400.0) };
    let in_range = |h: &mut Hands, reach: f32, rise: f32| {
        h.phys.world.actor_aim_point(target.0).is_some_and(|at| {
            let eye = h.player.eye();
            hdist(eye, at) <= reach && (eye[2] - at[2]).abs() < rise && crate::combat_tactics::shot_clear(Some(&mut *h.phys), eye, None, at, [at[0], at[1], at[2] + 40.0]).is_some()
        })
    };
    let walked = if in_range(h, reach, rise) { Walked::There } else { walk(p, w, h) };
    match walked {
        Walked::No(why) => {
            f.shooting = None;
            h.control.trigger = false;
            // No way there on foot: a player with a clear line shoots from where it
            // stands, as it would at a White Leg on a ledge over the road.
            if !f.afar && in_range(h, LONG_SHOT, LONG_SHOT) {
                f.afar = true;
                p.take_back_unreachable();
                info!("pilot: step {}: no way on foot to {name} ({why}): a clear line of fire, shot from where the player stands", p.i);
                return None;
            }
            return Some(why);
        }
        Walked::Going => {
            f.shooting = None;
            h.control.trigger = false;
        }
        Walked::There | Walked::Put => {
            if f.shooting.is_none() {
                f.health = h.game(|g| g.health_now_and_full(target).0);
            }
            let since = *f.shooting.get_or_insert(p.now);
            let at = h.phys.world.actor_aim_point(target.0).or_else(|| h.game(|g| where_is(g, target)).flatten().map(|(_, pos)| [pos[0], pos[1], pos[2] + 80.0]));
            // The line of fire, as the NPCs judge it: to the trunk, else the head.
            let eye = h.player.eye();
            let clear = at.and_then(|at| crate::combat_tactics::shot_clear(Some(&mut *h.phys), eye, None, at, [at[0], at[1], at[2] + 40.0]));
            if let Some(at) = clear.or(at) {
                let cross: Option<Crosshair> = h.control.crosshair;
                (h.player.heading, h.player.pitch) = Crosshair::aim_at(cross.as_ref(), eye, at);
            }
            if clear.is_none() && at.is_some() {
                h.control.trigger = false;
                let since = *f.blocked.get_or_insert(p.now);
                if p.now - since > NO_SHOT_SECS && w.radius > FIGHT_NEAREST {
                    w.radius = (w.radius * 0.66).max(FIGHT_NEAREST);
                    (f.blocked, f.shooting, w.follower) = (None, None, None);
                    info!("pilot: step {}: no clear shot at {name} for {NO_SHOT_SECS} s: the player goes nearer (within {:.0} units)", p.i, w.radius);
                }
                return None;
            }
            f.blocked = None;
            // Someone between the player and its target: no shot; a step to the side to
            // get a line past them (the side changes every second).
            let hostile = |h: &mut Hands, who: u32| h.game(|g| g.awareness.state(FormId(who)) == fnv_game::awareness::State::Combat).unwrap_or(false);
            // A body on the ground is shot past, as a player aims past it: a dead
            // Fiend in the line held the shot for a whole fight (Bleed Me Dry, round 16
            // [observed]).
            let between = at.map(|at| people_in_the_way(h.phys, eye, at, target)).unwrap_or_default();
            let first = between.into_iter().find(|w| h.game(|g| !g.is_dead(FormId(*w))).unwrap_or(true));
            if let Some(who) = first.filter(|w| !hostile(h, *w)) {
                h.control.trigger = false;
                let side = if (p.now as i64) % 2 == 0 { 1.0 } else { -1.0 };
                h.intent.right = side;
                if f.in_the_way != Some(who) {
                    info!("pilot: step {}: {who:08X} stands between the player and {name}: no shot, a step aside", p.i);
                    f.in_the_way = Some(who);
                }
                return None;
            }
            f.in_the_way = None;
            // The trigger is pulled and let go, as for a weapon that fires once a pull.
            h.control.trigger = ((p.now - since) * 6.0) as i64 % 2 == 0;
            // Closer only when the shots miss: a target losing health is fought from where
            // the player stands (the Chimera tank at Anchorage knocked the player down with
            // every shell as it walked in; a player keeps its distance from one).
            let hurt = h.game(|g| g.health_now_and_full(target).0).zip(f.health).is_some_and(|(now, then)| now < then);
            if !f.near && p.now - since > FIGHT_CLOSER_SECS && hurt {
                f.shooting = None;
            } else if !f.near && p.now - since > FIGHT_CLOSER_SECS {
                f.near = true;
                f.shooting = None;
                w.radius = FIGHT_NEAR;
                info!("pilot: step {}: {name} still lives after {FIGHT_CLOSER_SECS:.0} s of shooting: the player goes closer (its shots did not hurt it)", p.i);
            }
        }
    }
    None
}

/// One frame of turning the view onto a reference. `Some(true)`: the crosshair has it.
/// `Some(false)`: it never did, at any height.
fn aim(p: &Pilot, a: &mut Aim, h: &mut Hands) -> Option<bool> {
    if h.target.0.as_ref().is_some_and(|t| t.form_id == a.target.0) {
        return Some(true);
    }
    // Already talking to it: there is no crosshair in a conversation, and nothing to ask.
    let target = a.target;
    if h.game(|g| g.dialogue.as_ref().is_some_and(|c| c.actor == target)) == Some(true) {
        return Some(true);
    }
    if let Some(t) = h.target.0.as_ref() {
        a.had = Some(format!("{} {:08X} at {:.0} units", t.name, t.form_id, t.distance));
    }
    if p.now - a.since > AIM_SECS {
        a.height += 1;
        a.since = p.now;
    }
    // Every height tried: the crosshair never took it.
    let Some(&up) = AIM_HEIGHTS.get(a.height) else { return Some(false) };
    let pos = h.game(|g| where_is(g, target)).flatten()?.1;
    let cross: Option<Crosshair> = h.control.crosshair;
    (h.player.heading, h.player.pitch) = Crosshair::aim_at(cross.as_ref(), h.player.eye(), [pos[0], pos[1], pos[2] + up]);
    None
}

/// The carried weight a line asks the player to stay under: a condition
/// `GetActorValue InventoryWeight >= v` (or `> v`) run on the target, as Jed's "you ain't
/// going nowhere loaded down" line has (Honest Hearts INFO of CaravanMasterTopic000,
/// AV 46 >= 76 [observed]). The player must carry less than the value returned.
fn weight_limit(g: &Game, info: FormId) -> Option<f64> {
    use fnv_game::cond::{Cmp, Operand, RunOn};
    const GET_ACTOR_VALUE: u16 = 14;
    g.data.infos.get(&info)?.conditions.iter().find_map(|c| {
        let av = match c.params.first()? {
            fnv_script::vm::ArgValue::Enum(code) => f64::from(*code),
            v => v.value()?.num(),
        };
        let Operand::Value(v) = c.rhs else { return None };
        if c.function != GET_ACTOR_VALUE || av != f64::from(fnv_game::actor_values::INVENTORY_WEIGHT) || !matches!(c.run_on, RunOn::Target) {
            return None;
        }
        match c.cmp {
            Cmp::Ge => Some(v),
            Cmp::Gt => Some(v + 0.01),
            _ => None,
        }
    })
}

/// What the player leaves behind to carry less than `below`, as a player does at Jed's
/// "There's a chest over yonder, you can store your extra in that": its things go into the
/// nearest container of its cell, the heaviest first, weapons and ammunition last. The
/// container, how far it stands, the stacks moved and the weight carried after; `None`:
/// no container in the cell.
fn stash_weight(g: &mut Game, below: f64) -> Option<(FormId, f32, usize, f64)> {
    let player = FormId::PLAYER_REF;
    let (cell, feet) = where_is(g, player)?;
    let chest = g.world.placed.by_cell.get(&cell?)?.iter()
        .filter_map(|&r| {
            let base = g.world.peek(r).map(|s| s.base).or_else(|| g.world.placed.get(r).map(|p| p.base))?;
            (g.world.forms.sig(base) == Some(*b"CONT")).then_some(r)
        })
        .filter_map(|r| where_is(g, r).map(|(_, at)| (r, hdist(at, feet))))
        .min_by(|a, b| a.1.total_cmp(&b.1))?;
    let stacks: Vec<(FormId, i32)> = g.world.peek(player).and_then(|s| s.inventory.as_ref()).map(|inv| inv.iter().filter(|&(_, n)| n > 0).collect()).unwrap_or_default();
    let before = g.world.carried_weight(player).ok()?;
    let mut weighed: Vec<(bool, f64, FormId, i32)> = Vec::new();
    for (item, n) in stacks {
        if g.world.transfer(player, chest.0, item, n).ok() != Some(n) {
            continue;
        }
        let less = before - g.world.carried_weight(player).unwrap_or(before);
        let _ = g.world.transfer(chest.0, player, item, n);
        let kept = matches!(g.world.forms.sig(item).as_ref().map(|s| &s[..]), Some(b"WEAP" | b"AMMO"));
        if less > 0.0 {
            weighed.push((kept, less, item, n));
        }
    }
    weighed.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.total_cmp(&a.1)));
    let mut moved = 0;
    for (_, _, item, n) in weighed {
        if g.world.carried_weight(player).unwrap_or(0.0) < below {
            break;
        }
        if g.world.transfer(player, chest.0, item, n).is_ok() {
            moved += 1;
        }
    }
    Some((chest.0, chest.1, moved, g.world.carried_weight(player).unwrap_or(0.0)))
}

/// The conversation's lines are skipped after [`SKIP_AFTER`] s each. Whether a line is
/// being said.
fn hear(p: &mut Pilot, h: &mut Hands) -> bool {
    if let Some(info) = h.game(|g| g.dialogue.as_ref().and_then(|c| c.line.as_ref()).map(|l| l.info)).flatten() {
        p.heard = Some(info);
    }
    let stamp = h.game(|g| g.dialogue.as_ref().filter(|c| c.line.is_some()).map(|c| fnv_game::dialogue::conversation_stamp(g.dialogue_serial, Some(c)))).flatten();
    let Some(stamp) = stamp else {
        p.line = None;
        return false;
    };
    match p.line {
        Some((s, since)) if s == stamp => {
            if p.now - since > SKIP_AFTER {
                h.game(|g| g.skip_response());
                p.line = Some((stamp, p.now));
            }
        }
        _ => p.line = Some((stamp, p.now)),
    }
    true
}

/// The offered choice to pick so that the actor comes to say `wanted`, and how many picks
/// after it the line's topic is: the choice whose own line it is (0), else the one from
/// whose line the fewest Choices and Add Topics lead to the topic of `wanted`, at most
/// [`PICKS_DEEP`] picks away. A Goodbye is not a way there.
fn choice_toward(g: &Game, wanted: FormId) -> Option<(usize, usize)> {
    use std::collections::{HashMap, HashSet, VecDeque};
    let offered = g.dialogue_choices();
    if let Some(i) = offered.iter().position(|c| c.info == wanted) {
        return Some((i, 0));
    }
    let target = g.data.infos.get(&wanted)?.topic?;
    if let Some(i) = offered.iter().position(|c| c.topic == target && !c.goodbye) {
        return Some((i, 0));
    }
    // Topic to the topics its lines offer or add.
    let mut next: HashMap<FormId, Vec<FormId>> = HashMap::new();
    for d in g.data.infos.values() {
        if let Some(t) = d.topic {
            next.entry(t).or_default().extend(d.choices.iter().chain(&d.add_topics).copied());
        }
    }
    let mut best: Option<(usize, usize)> = None;
    for (i, c) in offered.iter().enumerate().filter(|(_, c)| !c.goodbye) {
        let Some(line) = g.data.infos.get(&c.info) else { continue };
        let mut seen: HashSet<FormId> = HashSet::new();
        let mut queue: VecDeque<(FormId, usize)> = line.choices.iter().chain(&line.add_topics).map(|t| (*t, 1)).collect();
        while let Some((t, d)) = queue.pop_front() {
            if t == target {
                if best.is_none_or(|(_, b)| d < b) {
                    best = Some((i, d));
                }
                break;
            }
            if d >= PICKS_DEEP || !seen.insert(t) {
                continue;
            }
            queue.extend(next.get(&t).into_iter().flatten().map(|n| (*n, d + 1)));
        }
    }
    best
}

/// Unlock's opcode [observed: schemas/script/command_opcodes.json].
const UNLOCK: u16 = 4211;

/// StartCombat's opcode [observed: schemas/script/command_opcodes.json, from
/// GetCommandOpcode in the original].
const START_COMBAT: u16 = 4118;

/// KillActor's (Kill's) opcode [observed: schemas/script/command_opcodes.json].
const KILL_ACTOR: u16 = 4235;

/// Whether a choice's line runs KillActor: never taken only to leave a conversation.
fn kills(g: &Game, c: &fnv_game::dialogue::Choice) -> bool {
    g.data.infos.get(&c.info).is_some_and(|d| {
        d.begin.into_iter().chain(d.end).any(|u| {
            let mut calls = std::collections::BTreeMap::new();
            fnv_game::intro::calls(&g.data.unit(u).program, &mut calls);
            calls.contains_key(&KILL_ACTOR)
        })
    })
}

/// A choice the pilot keeps off when it has another: one whose line starts a fight
/// (Jessup's Goodbye "When I kill you…" ended Boulder City in a gunfight), or a
/// companion's command (FollowersWait left Boone waiting behind; FollowersFire), or one
/// whose line kills someone (Dog's only Goodbye in the Sierra Madre kitchen, "Take that
/// chain around your neck…", is his suicide: INFO 0x010124E2's end script; taken to leave
/// the talk, his collar's countdown killed Dean, Christine and the player, round 21
/// [observed]).
fn choice_to_avoid(g: &Game, c: &fnv_game::dialogue::Choice) -> bool {
    let fights = g.data.infos.get(&c.info).is_some_and(|d| {
        d.begin.into_iter().chain(d.end).any(|u| {
            let mut calls = std::collections::BTreeMap::new();
            fnv_game::intro::calls(&g.data.unit(u).program, &mut calls);
            calls.contains_key(&START_COMBAT) || calls.contains_key(&KILL_ACTOR)
        })
    });
    fights || g.data.name(c.topic).starts_with("Followers")
}

/// One frame of leaving the open conversation as a player can (module notes). Whether no
/// conversation is open any more.
/// With `go_along`, a conversation in the player's way is first gone along with once (its
/// first choice that is no Goodbye and not shaded): a player on the way somewhere does
/// what stands in the way asks. Simon at the Silver Rush's door asks to search the player
/// ("Okay, search away" / "That's not going to happen", the Goodbye); leaving by the
/// Goodbye was refused at the door again and again. A chain's own `end_talk` does not.
fn leave_talk(p: &mut Pilot, h: &mut Hands, go_along: bool) -> bool {
    if h.game(|g| g.dialogue.is_none()) != Some(false) {
        (p.stuck_talk, p.talk_picks) = (None, 0);
        return true;
    }
    // A line is being said: heard out (skipped after a moment, the player's own key).
    if hear(p, h) {
        return false;
    }
    let serial = h.game(|g| g.dialogue_serial).unwrap_or(0);
    // A new conversation counts its picks afresh: ED-E's at step 53 of Camarader-E used
    // all twelve, and the next one at step 85 was given up at once with its Goodbye two
    // picks down (round 17 [observed]).
    if p.talk_serial != serial {
        (p.talk_serial, p.stuck_talk, p.talk_picks) = (serial, None, 0);
    }
    if go_along && p.went_along != Some(serial) {
        // When every choice is a Goodbye, the one whose line runs a script is the one that
        // does what stands in the way: The Fort's gate starts Lucullus's conversation, and
        // "I'm ready to return to Cottonwood Cove." (a Goodbye with a result script) is the
        // ride, "Goodbye." leaves the player at the gate (round 17 [observed]).
        let pick = h
            .game(|g| {
                let cs = g.dialogue_choices();
                cs.iter().position(|c| !c.goodbye && !c.darkened && !choice_to_avoid(g, c)).or_else(|| {
                    // A shaded (said before) one too, after an unshaded one: the second way
                    // out of the Fort offered the ride shaded beside "Goodbye.", and the
                    // player stood at the gate taking "Goodbye." for 240 s (rerun 4 [observed]).
                    let scripted = |c: &fnv_game::dialogue::Choice| c.goodbye && !choice_to_avoid(g, c) && g.data.infos.get(&c.info).is_some_and(|d| d.begin.is_some() || d.end.is_some());
                    cs.iter().all(|c| c.goodbye || c.darkened).then(|| cs.iter().position(|c| !c.darkened && scripted(c)).or_else(|| cs.iter().position(scripted))).flatten()
                })
            })
            .flatten();
        if let Some(i) = pick {
            p.went_along = Some(serial);
            let text = h.game(|g| g.dialogue_choices().get(i).map(|c| g.choice_text(c).to_string())).flatten().unwrap_or_default();
            h.game(|g| g.choose(i).ok());
            info!("pilot: step {}: goes along with what stands in the way: picks {text:?}", p.i);
            return false;
        }
    }
    enum Way {
        Goodbye(usize),
        TopicList,
        /// A choice whose line has no choices of its own and links nowhere: the
        /// conversation ends after it (the Ultra-Luxe's White Glove greeter offers only
        /// such answers, no Goodbye).
        Ending(usize),
        None,
        NotYet,
    }
    let way = h
        .game(|g| match g.dialogue.as_ref() {
            Some(c) if c.choices.is_empty() => Way::NotYet,
            // A Goodbye whose answer runs no script first (leaving changes nothing: the
            // Gomorrah guard's "Here you go. <Hand over all weapons>" is a Goodbye too, and
            // taken to leave it handed the weapons over outside and set iInGomorrah, so on
            // the way in the guard never greeted and the door's DisablePlayerControls was
            // never undone; "Nevermind, I'll be back." is the one that leaves), then one
            // that starts no fight and gives no companion an order, then (no topic list to
            // leave by Tab) any. Boone's only Goodbye is "Wait here.": taken to leave, he
            // waited for the rest of Forgot to Remember; Red's "Come on. Follow me." set
            // her following and Shorty's rescue line changed (round 12 [observed]).
            Some(c) => match c
                .choices
                .iter()
                .position(|x| x.goodbye && !x.darkened && !choice_to_avoid(g, x) && g.data.infos.get(&x.info).is_some_and(|d| d.begin.is_none() && d.end.is_none()))
                .or_else(|| c.choices.iter().position(|x| x.goodbye && !choice_to_avoid(g, x)))
                .or_else(|| if c.topic_list { None } else { c.choices.iter().position(|x| x.goodbye && !kills(g, x)) })
            {
                Some(i) => Way::Goodbye(i),
                None if c.topic_list => Way::TopicList,
                None => match c.choices.iter().position(|x| g.data.infos.get(&x.info).is_some_and(|d| d.choices.is_empty() && d.follow_ups.is_empty() && d.add_topics.is_empty())) {
                    Some(i) => Way::Ending(i),
                    None => Way::None,
                },
            },
            None => Way::NotYet,
        })
        .unwrap_or(Way::NotYet);
    match way {
        Way::Goodbye(i) => {
            let r = h.game(|g| g.choose(i).map(|_| ()));
            info!("pilot: step {}: leaves the conversation by its Goodbye choice{}", p.i, match r {
                Some(Err(e)) => format!(": {e}"),
                _ => String::new(),
            });
            p.stuck_talk = None;
        }
        Way::Ending(i) => {
            let text = h.game(|g| g.dialogue_choices().get(i).map(|c| g.choice_text(c).to_string())).flatten().unwrap_or_default();
            h.game(|g| g.choose(i).ok());
            info!("pilot: step {}: no Goodbye: leaves by {text:?}, a line that ends the conversation", p.i);
            p.stuck_talk = None;
        }
        Way::TopicList => {
            h.game(|g| g.end_dialogue());
            info!("pilot: step {}: leaves the topic list, which offers no Goodbye (as Tab does)", p.i);
        }
        Way::NotYet => {}
        Way::None if p.talk_picks < TALK_PICKS => {
            // No Goodbye here: a player picks something and goes on; the way out may be
            // further down (Sloan's Melissa, Ranger Milo, Cass). The first choice not
            // taken before in this conversation.
            p.talk_picks += 1;
            let i = h.game(|g| g.dialogue_choices().iter().position(|c| !c.darkened).unwrap_or(0)).unwrap_or(0);
            let text = h.game(|g| g.dialogue_choices().get(i).map(|c| g.choice_text(c).to_string())).flatten().unwrap_or_default();
            h.game(|g| g.choose(i).ok());
            info!("pilot: step {}: no Goodbye yet: picks {text:?} to go on ({} of {TALK_PICKS})", p.i, p.talk_picks);
        }
        Way::None => {
            let since = *p.stuck_talk.get_or_insert(p.now);
            if p.now - since > NO_WAY_OUT_SECS {
                let offered = h.game(|g| g.dialogue_choices().iter().map(|c| g.data.name(c.topic)).collect::<Vec<_>>().join(", ")).unwrap_or_default();
                p.finding("no_way_out", format!("the conversation offers no Goodbye and is not a topic list (offered: {offered}): a player could not leave it; broken off so that the chain goes on"));
                h.game(|g| g.end_dialogue());
                p.stuck_talk = None;
            }
        }
    }
    false
}

/// Walk to a reference, turn the view onto it and ask the crosshair; then `act`.
/// Returns when the step is over.
fn approach(p: &mut Pilot, h: &mut Hands, name: &str, what: &str) -> Result<Option<FormId>, String> {
    if matches!(p.phase, Phase::Begin) {
        let (r, place) = h.place_of(name)?;
        p.phase = new_walk(place, Some(r), NEAR_TARGET, p.now, h.feet());
    }
    let mut phase = std::mem::replace(&mut p.phase, Phase::Begin);
    let out = match &mut phase {
        Phase::Walk(w) => match walk(p, w, h) {
            Walked::Going => Ok(None),
            Walked::No(why) => {
                let target = w.target.or(p.across.map(|a| a.0)).expect("an approach walks to a reference");
                let tries = w.awaited;
                // Its own spot out of reach (behind a counter, a locked door to a work
                // spot): a point round it in reach of the crosshair, as a player talks
                // across a bar (Francine at the Atomic Wrangler, Garrett at the Talent Pool).
                if let Some(next) = across_point(p, h, target) {
                    p.take_back_unreachable();
                    info!("pilot: step {}: {name} cannot be walked up to ({why}): the player tries ({:.0}, {:.0}, {:.0}) beside it, {} of {ACROSS_TRIES}", p.i, next.pos[0], next.pos[1], next.pos[2], p.across.map_or(0, |a| a.1));
                    phase = Phase::Walk(walk_to(next, None, 24.0, p.now, h.feet()));
                    p.phase = phase;
                    return Ok(None);
                }
                if tries < AWAIT_TRIES {
                    info!("pilot: step {}: {name} cannot be walked to now: the player waits for it to come ({why})", p.i);
                    phase = Phase::Await { target, until: p.now + AWAIT_SECS, why, tries: tries + 1 };
                    Ok(None)
                } else {
                    Err(why)
                }
            }
            Walked::There | Walked::Put => {
                let target = w.target.or(p.across.map(|a| a.0)).expect("an approach walks to a reference");
                phase = Phase::Aim(Aim { target, height: 0, since: p.now, had: None });
                Ok(None)
            }
        },
        Phase::Await { target, until, why, tries } => {
            let (target, until, tries) = (*target, *until, *tries);
            let feet = h.feet();
            let world = h.world;
            let (talking, near) = h
                .game(|g| {
                    let talking = g.dialogue.as_ref().is_some_and(|c| c.actor == target);
                    let near = where_is(g, target).zip(where_is(g, FormId::PLAYER_REF)).is_some_and(|((ca, a), (cp, _))| same_space(world, ca, cp) && hdist(a, feet) < 1500.0);
                    (talking, near)
                })
                .unwrap_or((false, false));
            if talking {
                info!("pilot: step {}: {name} came and is talking to the player", p.i);
                return Ok(Some(target));
            }
            if near && p.now > until - AWAIT_SECS + 3.0 {
                // Near now: walked to again.
                let world = h.world;
                let place = h.game(|g| where_is(g, target).and_then(|(cell, pos)| Some(Place { space: crate::nav::space_of_cell(world, cell?.0)?, pos }))).flatten();
                if let Some(place) = place {
                    let mut w = walk_to(place, Some(target), NEAR_TARGET, p.now, feet);
                    w.awaited = tries;
                    phase = Phase::Walk(w);
                }
                Ok(None)
            } else if p.now > until {
                Err(format!("{why}; and it did not come in {AWAIT_SECS:.0} s of waiting"))
            } else {
                Ok(None)
            }
        }
        Phase::Aim(a) => match aim(p, a, h) {
            None => Ok(None),
            Some(ok) => {
                if !ok {
                    let feet = h.feet();
                    let had = a.had.clone().map_or("nothing".to_string(), |t| format!("{t} instead"));
                    p.finding("could_not_target", format!("standing at ({:.0}, {:.0}, {:.0}) and looking at {name}, the crosshair took {had}: a player could not {what} it from here", feet[0], feet[1], feet[2]));
                }
                Ok(Some(a.target))
            }
        },
        _ => Ok(None),
    };
    p.phase = phase;
    out
}

/// How far off the navmesh the player must be before the walk puts it back on, game
/// units [design].
const OFF_MESH_PUT: f32 = 16.0;
/// How far a goal with no navmesh under it moves to the nearest point on it, game units
/// [design].
const GOAL_SNAP: f32 = 512.0;
/// How often a walk into a trigger that ended outside it goes on [design].
const TRIGGER_REWALKS: u8 = 3;
/// How many points round a talk target out of reach the player tries [design].
const ACROSS_TRIES: usize = 16;
/// How far from the target those points are, game units: within the crosshair's reach
/// of a talk [design].
const ACROSS_RADII: [f32; 2] = [100.0, 140.0];

/// The next point round `target` on the navmesh of its space, not tried yet this step:
/// eight ways at each of [`ACROSS_RADII`], each snapped to the navmesh within 48 units and
/// near the target's floor. `None` when every one has been tried.
fn across_point(p: &mut Pilot, h: &mut Hands, target: FormId) -> Option<Place> {
    let world = h.world;
    let (space, at) = h.game(|g| where_is(g, target).and_then(|(cell, pos)| Some((crate::nav::space_of_cell(world, cell?.0)?, pos)))).flatten()?;
    let nav = h.nav?;
    let mut i = match p.across {
        Some((t, i)) if t == target => i,
        _ => 0,
    };
    while i < ACROSS_TRIES {
        let (ring, way) = (i / 8, i % 8);
        i += 1;
        let r = ACROSS_RADII[ring.min(ACROSS_RADII.len() - 1)];
        let a = way as f32 * std::f32::consts::FRAC_PI_4;
        let want = [at[0] + a.cos() * r, at[1] + a.sin() * r, at[2]];
        if let Some((_, on)) = nav.graph.nearest(space, want, 48.0) {
            // Or up to 160 units below it: a thing on a wall or a desk stands above the
            // floor. Ted's Freezer Access Terminal (0x001199DC) is placed 107 units over the
            // kitchen floor, and every point round it was refused, so the walk to it, whose
            // nearest navmesh lies behind the locked freezer door, gave up (beyond-the-beef
            // step 54, rerun 4 [observed]).
            let dz = on[2] - at[2];
            if (dz.abs() < 90.0 || (dz < 0.0 && dz > -160.0)) && hdist(on, at) > 48.0 {
                p.across = Some((target, i));
                return Some(Place { space, pos: on });
            }
        }
    }
    p.across = Some((target, i));
    None
}

/// One frame of the current step. `Some(error)` when the step is over.
fn step(p: &mut Pilot, h: &mut Hands) -> Option<Option<String>> {
    let a = p.actions[p.i].clone();
    match &a {
        Action::Note { .. } => Some(None),
        Action::MovePlayer { to } => {
            // The actor walked to came and is talking to the player: the player is with
            // it (Sunny comes to Ringo's with her own package and line; leaving that
            // conversation to walk to her lost her Say Once greeting and the fight).
            if matches!(p.phase, Phase::Begin) {
                let with_it = h.game(|g| runner::resolve(g, to).is_ok_and(|r| g.dialogue.as_ref().is_some_and(|c| c.actor == r)));
                if with_it == Some(true) {
                    info!("pilot: step {}: {to} is talking to the player: no walk", p.i);
                    return Some(None);
                }
            }
            if matches!(p.phase, Phase::Begin) {
                // The steps after the walk deal with a conversation begun on it?
                let answered = next_answers_talk(p);
                match h.place_of(to) {
                    Ok((r, place)) => {
                        let mut w = walk_to(place, Some(r), NEAR_MARKER, p.now, h.feet());
                        w.keep_talk = answered;
                        p.phase = Phase::Walk(w);
                    }
                    Err(e) => return Some(Some(e)),
                }
            }
            // An actor that cannot be walked to now is waited for, as for a `talk`.
            let is_actor = h.game(|g| runner::resolve(g, to).is_ok_and(|r| g.world.placed.get(r).is_some_and(|pl| g.data.actors.contains_key(&pl.base)))) == Some(true);
            if is_actor && !matches!(p.phase, Phase::Wait { .. }) {
                return match approach(p, h, to, "go to") {
                    // There (the crosshair is not asked for a walk): the step is over.
                    Ok(None) if matches!(p.phase, Phase::Aim(_)) => {
                        p.phase = Phase::Begin;
                        Some(None)
                    }
                    Ok(None) => None,
                    Ok(Some(_)) => Some(None),
                    Err(why) => Some(Some(why)),
                };
            }
            walk_phase(p, h)
        }
        Action::GoToCell { cell } => {
            if matches!(p.phase, Phase::Begin) {
                let world = h.world;
                let place = h.game(|g| runner::cell_arrival(g, cell).map(|(c, mut pos)| {
                    // An exterior cell with no COC marker arrives at its middle at height
                    // 0, under the ground: the height is the middle of what is placed in
                    // it (Paradise Falls' front cell put the player on a ledge the navmesh
                    // nearest to height 0 held, 130 units from the ground [observed]).
                    if pos[2] == 0.0 {
                        let mut zs: Vec<f32> = g.world.placed.by_cell.get(&c).map(|v| v.iter().filter_map(|r| g.world.placed.get(*r)).map(|p| p.pos[2]).collect()).unwrap_or_default();
                        zs.sort_by(f32::total_cmp);
                        if let Some(z) = zs.get(zs.len() / 2) {
                            pos[2] = *z;
                        }
                    }
                    (c, pos)
                })).unwrap_or_else(|| Err("the scripts are not running".into())).and_then(|(c, pos)| {
                    let space = crate::nav::space_of_cell(world, c.0).ok_or_else(|| format!("{cell}: not indexed"))?;
                    Ok(Place { space, pos })
                });
                match place {
                    Ok(place) => p.phase = new_walk(place, None, 2.0 * NEAR_MARKER, p.now, h.feet()),
                    Err(e) => return Some(Some(e)),
                }
            }
            walk_phase(p, h)
        }
        Action::TriggerEnter { trigger, actor: Some(actor), .. } if actor != "player" => {
            info!("pilot: step {}: left to the AI: {actor} entering {trigger}", p.i);
            if let Some((a, at)) = h.game(|g| runner::resolve(g, actor).ok().zip(runner::resolve(g, trigger).ok())).flatten() {
                if !p.ai_due.iter().any(|d| d.actor == a && d.at == at) {
                    p.ai_due.push(AiDue { step: p.i, actor: a, at, within: AI_DUE_NEAR, what: format!("{actor} entering {trigger}"), since: p.now });
                }
            }
            Some(None)
        }
        Action::TriggerEnter { trigger, face, .. } => {
            if matches!(p.phase, Phase::Begin) {
                match h.place_of(trigger) {
                    Ok((_, place)) => {
                        let mut w = walk_to(place, None, 24.0, p.now, h.feet());
                        // A conversation on the way ends the walk only when the steps
                        // after it answer one; otherwise it is left as a player leaves it
                        // and the walk goes on. Ended by the NCR radio operator's talk
                        // mid-fight, Hidden Valley's self-destruct trigger was counted
                        // entered and never was: VStoryEventBoS stayed 0 and House's
                        // dam battle never opened its doors (round 12 [observed]).
                        w.keep_talk = next_answers_talk(p);
                        p.phase = Phase::Walk(w);
                    }
                    Err(e) => return Some(Some(e)),
                }
            }
            // Inside the volume anywhere is in it, as the game tells a trigger (its middle
            // can lie past a gate: The Pitt's arrival trigger spans the locked gate
            // 0x0C001204, and the walk to its middle was refused).
            let r = h.game(|g| runner::resolve(g, trigger).ok()).flatten();
            let inside = r.is_some_and(|r| h.script.rt.as_ref().is_some_and(|rt| rt.triggers.inside.contains(&(r, FormId::PLAYER_REF))));
            // Inside, turned to the reference the trigger's script wants the player to face
            // (posing for Jonas's picture), as a player would turn.
            if let (true, Some(face)) = (inside, face.as_deref()) {
                if let Some((_, at)) = h.game(|g| runner::resolve(g, face).ok().and_then(|f| where_is(g, f))).flatten() {
                    let eye = h.player.eye();
                    let cross: Option<Crosshair> = h.control.crosshair;
                    h.player.heading = Crosshair::aim_at(cross.as_ref(), eye, [at[0], at[1], eye[2]]).0;
                    (h.intent.forward, h.intent.right) = (0.0, 0.0);
                }
            }
            if inside && p.part != 1 {
                (p.part, p.phase) = (1, Phase::Wait { until: p.now + 1.0 });
                return None;
            }
            // A second inside it, for its script to see the player.
            match walk_phase(p, h) {
                // Over, but not inside: a conversation on the way ended the walk (Old Ben
                // greeted the player short of the Strip's VMS23 trigger, and King's Gambit
                // never began [observed]). Walked on, a few times.
                Some(None) if p.part != 1 && !inside && r.is_some_and(|r| h.script.rt.as_ref().is_some_and(|rt| rt.triggers.shapes_of(r).is_some())) && p.trigger_rewalks < TRIGGER_REWALKS => {
                    p.trigger_rewalks += 1;
                    info!("pilot: step {}: the walk ended outside {trigger}: walks on ({} of {TRIGGER_REWALKS})", p.i, p.trigger_rewalks);
                    p.part = 0;
                    p.phase = Phase::Begin;
                    None
                }
                Some(None) if p.part != 1 => {
                    (p.part, p.phase) = (1, Phase::Wait { until: p.now + 1.0 });
                    None
                }
                // No way to its middle: once, to the nearest point inside it from here.
                Some(Some(why)) if p.part == 0 => {
                    let feet = h.feet();
                    let near = r.and_then(|r| h.script.rt.as_ref().and_then(|rt| rt.triggers.shapes_of(r).map(<[_]>::to_vec))).and_then(|v| {
                        v.iter().map(|s| s.nearest_inside(feet, TRIGGER_INSET)).min_by(|a, b| hdist(*a, feet).total_cmp(&hdist(*b, feet)))
                    });
                    match near {
                        Some(at) => {
                            info!("pilot: step {}: the trigger's middle cannot be reached ({why}): the player goes to its nearest point from here, ({:.0}, {:.0}, {:.0})", p.i, at[0], at[1], at[2]);
                            // In the trigger's own space: from out of doors toward a trigger
                            // in an interior (Honest Hearts' slide room) the interior's
                            // point was put in the worldspace [observed].
                            let space = r.and_then(|r| h.game(|g| where_is(g, r).and_then(|(cell, _)| cell))).flatten().and_then(|c| crate::nav::space_of_cell(h.world, c.0)).unwrap_or_else(|| space_of(h.location));
                            let mut w = walk_to(Place { space, pos: at }, None, 24.0, p.now, feet);
                            w.keep_talk = next_answers_talk(p);
                            (p.part, p.phase) = (2, Phase::Walk(w));
                            None
                        }
                        None => Some(Some(why)),
                    }
                }
                other => other,
            }
        }
        Action::Talk { actor, topic, label } => {
            if p.part == 0 && matches!(p.phase, Phase::Wait { until } if p.now < until) {
                return None;
            }
            if p.part == 0 && matches!(p.phase, Phase::Wait { .. }) {
                p.phase = Phase::Begin;
            }
            // Talking already (it came and greeted while waited for): the conversation.
            // The actor came and spoke first (a force greet): that is the conversation.
            if p.part == 0 && matches!(p.phase, Phase::Begin) {
                let with_it = h.game(|g| runner::resolve(g, actor).is_ok_and(|r| g.dialogue.as_ref().is_some_and(|c| c.actor == r)));
                if with_it == Some(true) {
                    info!("pilot: step {}: {actor} is already talking to the player", p.i);
                    p.part = 2;
                }
            }
            // A script holds the player's movement outside a conversation: the player stands
            // and waits (up to TALK_HELD_SECS) for the scene to give it back or the actor to
            // speak first. Moira's check-up (SCPT 0x0A02D2C0) holds the controls for 8 s and
            // then comes to say 0x0A0309E4, whose result ends the check-up; talked to at once,
            // that line ended it before her timer gave the controls back, and they never came
            // back (Wasteland Survival Guide, 21 soft locks [observed]).
            if p.part == 0 && h.locks.movement && p.now - p.step_from < TALK_HELD_SECS && h.game(|g| g.dialogue.is_none()) == Some(true) {
                if !matches!(p.phase, Phase::Wait { .. }) {
                    info!("pilot: step {}: a script holds the player's movement: waits before talking to {actor}", p.i);
                }
                p.phase = Phase::Wait { until: p.now + 0.5 };
                return None;
            }
            if p.part == 0 {
                match approach(p, h, actor, "talk to") {
                    Err(e) => return Some(Some(e)),
                    Ok(None) => return None,
                    Ok(Some(target)) => {
                        let topic = topic.clone();
                        let started = match topic {
                            // A named topic is the headless run's own opening: said as it says it.
                            Some(t) => h.game(|g| runner::resolve(g, &t).and_then(|t| g.start_dialogue(target, Some(t))).map(|_| ())),
                            None => h.script.rt.as_mut().map(|rt| {
                                with_world(rt, h.sim, |rt| rt.activate_as_player(target)).and_then(|by| match by {
                                    // A script may start the conversation itself, a moment later.
                                    // (So may its default action: the conversation is waited for.)
                                    ByHand::Talk | ByHand::Greeted | ByHand::Choices | ByHand::Script | ByHand::Default(_) => Ok(()),
                                    // Asleep: the first press wakes it; the player presses again.
                                    ByHand::Woken => Err("woken".to_string()),
                                    // A teammate opens its companion wheel: the player picks Talk on
                                    // it, as a player does (Sarah Lyons in Take it Back!).
                                    ByHand::Wheel => Err("wheel".to_string()),
                                    other => Err(format!("activating {actor} did not start a conversation: {}", by_hand(&other))),
                                })
                            }),
                        };
                        let started = match started {
                            Some(Err(e)) if e == "wheel" => {
                                let r = h.game(|g| g.wheel_command(fnv_game::companion_wheel::Command::Talk)).unwrap_or_else(|| Err("the scripts are not running".into()));
                                info!("pilot: step {}: {actor}'s companion wheel: Talk ({r:?})", p.i);
                                Some(match r {
                                    Ok(fnv_game::companion_wheel::Outcome::Talk { .. }) => Ok(()),
                                    other => Err(format!("activating {actor} opened its companion wheel and Talk did not start a conversation ({other:?})")),
                                })
                            }
                            other => other,
                        };
                        if let Some(Err(e)) = &started {
                            if e == "woken" && p.woke < 3 {
                                p.woke += 1;
                                info!("pilot: step {}: {actor} was asleep and is woken: the player talks to it again", p.i);
                                (p.part, p.phase) = (0, Phase::Wait { until: p.now + 1.5 });
                                return None;
                            }
                            return Some(Some(if e == "woken" { format!("{actor} woke but would not talk") } else { e.clone() }));
                        }
                        (p.part, p.phase) = (2, Phase::Wait { until: p.now + 5.0 });
                    }
                }
            }
            if h.game(|g| g.dialogue.is_some()) == Some(true) {
                if let Some(l) = label {
                    p.observe(l, &Map::new(), true);
                }
                return Some(None);
            }
            match p.phase {
                Phase::Wait { until } if p.now < until => None,
                _ => Some(Some(format!("no conversation with {actor} 5 s after talking to it"))),
            }
        }
        Action::PlayerActivate { target, .. } => match approach(p, h, target, "activate") {
            Err(e) => Some(Some(e)),
            Ok(None) => None,
            Ok(Some(r)) => {
                let done = h.script.rt.as_mut().map(|rt| with_world(rt, h.sim, |rt| rt.activate_as_player(r)));
                match done {
                    Some(Ok(by)) => {
                        info!("pilot: step {}: activates {target}: {}", p.i, by_hand(&by));
                        Some(None)
                    }
                    Some(Err(e)) => Some(Some(e)),
                    None => Some(Some("the scripts are not running".into())),
                }
            }
        },
        Action::WaitTalk { max_frames, start, .. } => {
            let limit = (*max_frames as f64 / 60.0).clamp(5.0, WAIT_CAP);
            let talking = h.game(|g| g.dialogue.is_some()) == Some(true);
            if *start && !talking && p.part == 0 {
                if p.now - p.step_from > limit {
                    p.finding("no_conversation", format!("nobody started a conversation with the player in {limit:.0} s (the headless run stands in for the actor's arrival)"));
                    return Some(Some("no conversation started".into()));
                }
                return None;
            }
            p.part = 1;
            if !hear(p, h) {
                return Some(None);
            }
            (p.now - p.step_from > limit).then(|| Some("the line did not end".to_string()))
        }
        Action::Choose { topic, prompt, index, max_frames, label } => {
            // Things stored for a line's weight limit: the conversation is left by its
            // Goodbye, then the chain's talk with the actor is begun again.
            if let Some(back) = p.retalk {
                if leave_talk(p, h, false) {
                    p.retalk = None;
                    p.i = back;
                    (p.phase, p.part, p.step_from) = (Phase::Begin, 0, p.now);
                }
                return None;
            }
            let limit = (*max_frames as f64 / 60.0).clamp(5.0, WAIT_CAP);
            if hear(p, h) {
                return (p.now - p.step_from > limit).then(|| Some("the line before the choice did not end".to_string()));
            }
            let picked = h.game(|g| {
                if g.dialogue.is_none() {
                    return Err("no conversation: nothing to choose".to_string());
                }
                if g.dialogue_choices().is_empty() {
                    return Ok(None);
                }
                let i = runner::choice_index(g, topic, prompt, index)?;
                g.choose(i).map(Some)
            });
            match picked {
                Some(Ok(Some(_))) => {
                    if let Some(l) = label {
                        p.observe(l, &Map::new(), true);
                    }
                    Some(None)
                }
                Some(Ok(None)) => (p.now - p.step_from > limit).then(|| Some("no choices were offered".to_string())),
                // The line said instead asks the player to carry less (Jed's weight limit):
                // the player stores things in the container beside it, as the line says
                // to, and talks to the actor again from the chain's last `talk` with it.
                // Once a chain.
                Some(Err(e)) if !p.stashed.contains(&p.chain) => {
                    let heard = p.heard;
                    let limit = heard.and_then(|i| h.game(|g| weight_limit(g, i)).flatten());
                    let back = p.actions[..p.i].iter().rposition(|a| matches!(a, Action::Talk { .. }));
                    match (limit, back) {
                        (Some(below), Some(back)) => {
                            p.stashed.push(p.chain.clone());
                            let done = h.game(|g| stash_weight(g, below)).flatten();
                            match done {
                                Some((chest, away, moved, now)) => {
                                    let name = h.game(|g| g.data.name(chest)).unwrap_or_default();
                                    info!("pilot: step {}: {e}; the line heard asks the player to carry less than {below:.0} lb: {moved} stack(s) put into {name} {chest:08X} ({away:.0} units away), {now:.0} lb carried; the talk is begun again from step {back}", p.i, chest = chest.0);
                                    p.headless("stash (a line's carry-weight limit)");
                                    p.retalk = Some(back);
                                    None
                                }
                                None => Some(Some(format!("{e}; the line heard asks for less than {below:.0} lb carried and no container is in the cell"))),
                            }
                        }
                        _ => Some(Some(e)),
                    }
                }
                Some(Err(e)) => Some(Some(e)),
                None => Some(Some("the scripts are not running".into())),
            }
        }
        Action::AdvanceSeconds { seconds } => wait_for(p, seconds.min(ADVANCE_CAP)),
        Action::WaitFrames { frames } => wait_for(p, (*frames as f64 / 60.0).min(ADVANCE_CAP)),
        // A console line read each frame until it is not 0, as the headless run reads it
        // (Lonesome Road's end slides: until the narrator's last line moves the player back
        // to the Mojave). Not skipped: the step after it walked from the slides' room.
        Action::CallUntil { line, max_frames, label } => {
            let limit = (*max_frames as f64 / 60.0).clamp(10.0, CALL_UNTIL_CAP);
            match h.game(|g| runner::call_value(g, line)) {
                Some(Ok(v)) if v != 0.0 => {
                    info!("pilot: step {}: {line:?} is {v} after {:.1} s ({label})", p.i, p.now - p.step_from);
                    Some(None)
                }
                Some(Err(e)) => Some(Some(e)),
                _ if p.now - p.step_from > limit => {
                    p.finding("wait_timed_out", format!("step {} call_until: {line:?} still 0 after {limit:.0} s", p.i));
                    Some(Some(format!("{line:?} still 0 after {limit:.0} s")))
                }
                _ => None,
            }
        }
        Action::WaitQuest { quest, field, value, max_frames, label, .. } => {
            let limit = (*max_frames as f64 / 60.0).clamp(10.0, WAIT_CAP);
            match h.game(|g| runner::quest_field(g, quest, field)) {
                Some(Ok(v)) if same(&v, value) => {
                    p.observe(label, &Map::from_iter([(field.clone(), v)]), true);
                    Some(None)
                }
                Some(Ok(v)) if p.now - p.step_from > limit => {
                    p.finding("wait_timed_out", format!("{quest} {field} is {v}, not {value}, after {limit:.0} s"));
                    p.observe(label, &Map::from_iter([(field.clone(), v)]), true);
                    Some(Some(format!("{quest} {field} did not become {value}")))
                }
                Some(Ok(_)) => None,
                Some(Err(e)) => Some(Some(e)),
                None => Some(Some("the scripts are not running".into())),
            }
        }
        Action::Terminal { target, item, menu, label } => terminal_step(p, h, target, *item, menu.as_deref(), label.as_deref()),
        Action::EndTalk { .. } => {
            if leave_talk(p, h, false) {
                return Some(None);
            }
            (p.now - p.step_from > 60.0).then(|| Some("the conversation could not be left in 60 s".to_string()))
        }
        Action::GetQuest { quest, fields, label } => {
            // A milestone with `--pilot-save-load`: the game's state goes through a save
            // and a load first, as the headless `save_load` does it, and is read after it.
            let milestone = label.as_deref().is_some_and(|l| l.starts_with("milestone") || l.starts_with("final"));
            if p.save_load && milestone && p.part == 0 {
                p.part = 1;
                let mut applier = std::mem::take(&mut p.applier);
                let r = h.game(|g| runner::apply_now(g, &mut applier, &Action::SaveLoad { label: None }));
                p.applier = applier;
                match r {
                    Some(Ok(_)) => info!("pilot: step {}: the game's state saved and loaded before the milestone is read", p.i),
                    Some(Err(e)) => p.finding("save_load", format!("the save and load before the milestone failed: {e}")),
                    None => {}
                }
            }
            let read = h.game(|g| fields.iter().map(|f| runner::quest_field(g, quest, f).map(|v| (f.clone(), v))).collect::<Result<Map<String, Json>, String>>());
            match read {
                None => Some(Some("the scripts are not running".into())),
                Some(Err(e)) => Some(Some(e)),
                Some(Ok(read)) => {
                    // A read that differs is read again: the game takes real time for
                    // what the headless run does in a frame.
                    let label = label.clone().unwrap_or_else(|| format!("step {}", p.i));
                    let last = p.now - p.step_from > MILESTONE_SECS;
                    (p.observe(&label, &read, last) || last).then_some(None)
                }
            }
        }
        Action::GetRef { fields: Some(fields), of, reference, label } => {
            let Some(name) = reference.as_ref().or(of.as_ref()) else { return Some(Some("get_ref needs 'ref'".into())) };
            let read = h.game(|g| fields.iter().map(|f| runner::reference_field(g, name, f).map(|v| (f.clone(), v))).collect::<Result<Map<String, Json>, String>>());
            match read {
                None => Some(Some("the scripts are not running".into())),
                Some(Err(e)) => Some(Some(e)),
                Some(Ok(read)) => {
                    let label = label.clone().unwrap_or_else(|| format!("step {}", p.i));
                    p.observe(&label, &read, true);
                    Some(None)
                }
            }
        }
        // Killed by someone else: the AI's fight, watched from where the player is. The
        // player does not join in (Ricky, killed by a White Leg at Zion's northern
        // passage: the pilot went to fight him and was caught in the passage).
        Action::Kill { target, actor, killer: Some(killer) } if (target.is_some() || actor.is_some()) && !killer.eq_ignore_ascii_case("player") && !killer.eq_ignore_ascii_case("playerref") => {
            let name = target.clone().or_else(|| actor.clone()).expect("checked");
            match h.game(|g| runner::resolve(g, &name).map(|r| g.is_dead(r))) {
                Some(Ok(true)) => {
                    info!("pilot: step {}: {name} is dead ({killer}'s fight, {:.0} s)", p.i, p.now - p.step_from);
                    Some(None)
                }
                Some(Ok(false)) if p.now - p.step_from < AI_KILL_SECS => None,
                Some(Ok(false)) => {
                    p.finding("ai_kill_not_done", format!("{killer} did not kill {name} in {AI_KILL_SECS:.0} s: killed as the headless run kills it"));
                    p.headless("kill (by an actor, not done)");
                    let mut applier = std::mem::take(&mut p.applier);
                    let r = h.game(|g| runner::apply_now(g, &mut applier, &a));
                    p.applier = applier;
                    Some(r.and_then(|r| r.err()))
                }
                Some(Err(e)) => Some(Some(e)),
                None => Some(Some("the scripts are not running".into())),
            }
        }
        Action::Kill { target, actor, .. } if target.is_some() || actor.is_some() => {
            let name = target.clone().or_else(|| actor.clone()).expect("checked");
            fight(p, h, &name, &a)
        }
        // The player's shot with a weapon it carries (Lonesome Road's laser detonator on the
        // camp barrier's warhead): fired, not set off headless.
        Action::Hit { target, attacker, weapon: Some(weapon), .. } if attacker.as_deref().is_none_or(|x| x.eq_ignore_ascii_case("player") || x.eq_ignore_ascii_case("playerref")) => {
            let (target, weapon) = (target.clone(), weapon.clone());
            shoot_object(p, h, &target, &weapon, &a)
        }
        Action::SayInfo { info, speaker: Some(speaker), .. } if p.campaign => {
            // Begun since the chain began, not since this step did: the game does not wait
            // for the chain's steps, and a line said while an earlier step was still being
            // carried out is said (Sunny's lines at the wells came during the fights; the
            // player then stood by her for two minutes a line, asking her what was up).
            if matches!(p.phase, Phase::Begin) {
                let (r, place) = match h.place_of(speaker) {
                    Ok(x) => x,
                    Err(e) => return Some(Some(e)),
                };
                p.still = (p.now, place.pos);
                p.part = 0;
                // A conversation with the speaker ends the walk (the walk's target is
                // talking); one with anyone else is left as a player leaves it: the
                // Fort weather station guard's talk ended the walk to House's monitor
                // 6219 times and the step stalled (legion, round 12 [observed]).
                let w = walk_to(place, Some(r), NEAR_TARGET, p.now, h.feet());
                p.phase = Phase::Walk(w);
            }
            let mark = p.mark;
            let begun = h.game(|g| runner::resolve(g, info).is_ok_and(|i| g.softlock_seen.lines_since(mark).any(|l| l == i) || g.dialogue.as_ref().and_then(|c| c.line.as_ref()).is_some_and(|l| l.info == i)));
            if begun == Some(true) {
                info!("pilot: step {}: {speaker} has said {info} ({:.1} s into the step)", p.i, p.now - p.step_from);
                p.phase = Phase::Begin;
                return Some(None);
            }
            // The wait for the line counts from when the player stood beside the speaker,
            // not from the step's start: a long walk, a fight or a stuck stretch on the way
            // used the 120 s up, and Pearl's line came at once once the player was beside
            // her (the skipped-lines audit, 2026-11-19 [observed]). A step that never gets
            // there gives up at three times the wait.
            let world = h.world;
            let feet = h.feet();
            let beside = h.game(|g| runner::resolve(g, speaker).ok().and_then(|r| where_is(g, r)).zip(where_is(g, FormId::PLAYER_REF)).is_some_and(|((cs, at), (cp, _))| same_space(world, cs, cp) && hdist(at, feet) < LINE_NEAR)) == Some(true);
            if beside && p.near_since.is_none() {
                p.near_since = Some(p.now);
            }
            // Far away: stood in before the step's own limit ends it without the line (a
            // walk to Nellis with fights on the way took the 240 s, and Pearl's line was
            // neither said nor stood in: Kings Gambit, sweep 2 [observed]).
            if p.near_since.is_some_and(|t| p.now - t > LINE_SECS) || p.now - p.step_from > (LINE_SECS * 3.0).min(STEP_SECS - 10.0) {
                p.finding("line_not_said", format!("{speaker} did not say {info} in {LINE_SECS:.0} s with the player beside it: said as the headless run says it"));
                p.headless("say_info (a line not said)");
                let mut applier = std::mem::take(&mut p.applier);
                let r = h.game(|g| runner::apply_now(g, &mut applier, &a));
                p.applier = applier;
                return Some(r.and_then(|r| r.err()));
            }
            // Its other lines are heard out (and skipped) on the way.
            let saying = hear(p, h);
            let talking = saying || h.game(|g| g.dialogue.is_some()) == Some(true);
            // In a conversation with it, choosing: the pick that brings the line.
            if talking && !saying {
                let picked = h.game(|g| {
                    let wanted = runner::resolve(g, info).ok()?;
                    let with_it = runner::resolve(g, speaker).is_ok_and(|r| g.dialogue.as_ref().is_some_and(|c| c.actor == r));
                    if !with_it || g.dialogue_choices().is_empty() {
                        return None;
                    }
                    let (i, away) = choice_toward(g, wanted)?;
                    let text = g.choice_text(&g.dialogue_choices()[i]).to_string();
                    g.choose(i).ok().map(|_| (text, away))
                });
                if let Some(Some((text, away))) = picked {
                    info!("pilot: step {}: picks {text:?} for the line {info} ({})", p.i, if away == 0 { "its own choice".to_string() } else { format!("{away} pick(s) from its topic") });
                    return None;
                }
                // No known way to the line (a result script's AddTopic, a way longer than
                // the search): what is on offer is tried, one topic at a time, as a player
                // asks around. Jessup's bribe line was never reached in 120 s otherwise.
                if p.explored.0 != p.i {
                    p.explored = (p.i, Vec::new());
                }
                let tried = p.explored.1.clone();
                let explore = h.game(|g| {
                    let with_it = runner::resolve(g, speaker).is_ok_and(|r| g.dialogue.as_ref().is_some_and(|c| c.actor == r));
                    if !with_it || tried.len() >= usize::from(TALK_PICKS) {
                        return None;
                    }
                    let offered = g.dialogue_choices();
                    let i = offered.iter().position(|c| !c.goodbye && !c.darkened && !tried.contains(&c.topic) && !choice_to_avoid(g, c))?;
                    let (topic, text) = (offered[i].topic, g.choice_text(&offered[i]).to_string());
                    g.choose(i).ok().map(|_| (topic, text))
                });
                if let Some(Some((topic, text))) = explore {
                    p.explored.1.push(topic);
                    info!("pilot: step {}: no known way to the line {info}: tries {text:?} ({} of {TALK_PICKS})", p.i, p.explored.1.len());
                    return None;
                }
            }
            let mut phase = std::mem::replace(&mut p.phase, Phase::Begin);
            if let Phase::Walk(w) = &mut phase {
                let walked = walk(p, w, h);
                if let Walked::No(why) = walked {
                    return Some(Some(why));
                }
                let there = matches!(walked, Walked::There | Walked::Put);
                let at = w.goal.pos;
                if hdist(at, p.still.1) > 16.0 || talking || !there {
                    p.still = (p.now, at);
                } else if p.part == 0 && p.now - p.still.0 > STILL_SECS {
                    p.part = 1;
                    let target = w.target.expect("walks to the speaker");
                    let r = h.script.rt.as_mut().map(|rt| with_world(rt, h.sim, |rt| rt.activate_as_player(target)));
                    info!("pilot: step {}: {speaker} stands silent: the player talks to it for the line {info}: {}", p.i, outcome(r.as_ref()));
                }
                // A walk that ended is begun again: the speaker may go on.
                if there {
                    w.follower = None;
                }
            }
            p.phase = phase;
            None
        }
        // A console line that moves another actor stands in for its coming of its own
        // accord (Beagle's run back to Primm, Chauncey at 4 pm): the player goes to the
        // place and waits for it there, an hour at a time.
        // A chain marks a quest script's own MoveTo (its label begins `script:`): done at
        // once, as the script does it. Christine and Dog reach their Sierra Madre wings by
        // NVDLC01MQ02 stage 50's MoveTo, not by a walk; waited for, they never came
        // (round 12 [observed]).
        Action::Call { line, label } if p.campaign && line.to_ascii_lowercase().contains(".moveto") && !line.to_ascii_lowercase().starts_with("player") && !label.as_deref().is_some_and(|l| l.starts_with("script:")) => {
            let mut words = line.split_whitespace();
            let (Some(who), Some(marker)) = (words.next().and_then(|w| w.split('.').next()).map(str::to_string), words.next().map(str::to_string)) else {
                info!("pilot: step {}: left to the AI: {line}", p.i);
                return Some(None);
            };
            // (A bare hex FormID in a console line has no 0x.)
            let named = |g: &Game, n: &str| runner::resolve(g, n).or_else(|_| runner::resolve(g, &format!("0x{n}")));
            let world = h.world;
            let places = h.game(|g| {
                let actor = named(g, &who).ok()?;
                let m = named(g, &marker).ok()?;
                let (cell, pos) = where_is(g, m)?;
                let space = crate::nav::space_of_cell(world, cell?.0)?;
                // Where the actor is counts only in the marker's own space: two interiors
                // share coordinates (Dad "at" MQ05ControlRoomMarker in PPurity01 while he
                // stood in PPurity02 [observed]).
                let at = where_is(g, actor).filter(|(c, _)| same_space(world, *c, cell)).map(|(_, at)| at);
                Some((actor, Place { space, pos }, at))
            });
            // `X.MoveTo player`: X comes to the player (a follower): a minute is waited,
            // with no Wait menu.
            if marker.eq_ignore_ascii_case("player") || marker.eq_ignore_ascii_case("playerref") {
                // Come: beside the player, or talking to it (Boone walks up and starts
                // his line).
                let near = h.game(|g| {
                    let r = named(g, &who).ok();
                    let talking = r.is_some_and(|r| g.dialogue.as_ref().is_some_and(|c| c.actor == r));
                    talking || r.and_then(|r| where_is(g, r)).zip(where_is(g, FormId::PLAYER_REF)).is_some_and(|((ca, a), (cp, b))| same_space(world, ca, cp) && hdist(a, b) < ARRIVED)
                }) == Some(true);
                if near {
                    info!("pilot: step {}: {who} is with the player", p.i);
                    return Some(None);
                }
                if p.now - p.step_from < 60.0 {
                    return None;
                }
                p.finding("never_came", format!("{who} did not come to the player in 60 s: moved as the headless run moves it"));
                p.headless("call (an actor that never came)");
                let mut applier = std::mem::take(&mut p.applier);
                let r = h.game(|g| runner::apply_now(g, &mut applier, &a));
                p.applier = applier;
                return Some(r.and_then(|r| r.err()));
            }
            let Some(Some((_actor, place, actor_at))) = places else {
                info!("pilot: step {}: left to the AI: {line} (the actor or the place is not known)", p.i);
                return Some(None);
            };
            if actor_at.is_some_and(|at| hdist(at, place.pos) <= ARRIVED) {
                info!("pilot: step {}: {who} is at {marker} after {} hour(s) of waiting", p.i, p.waited);
                return Some(None);
            }
            if matches!(p.phase, Phase::Begin) {
                info!("pilot: step {}: {who} is to come to {marker}: the player goes there and waits", p.i);
                p.phase = new_walk(place, None, NEAR_TARGET, p.now, h.feet());
            }
            // There first; then an hour at a time.
            if p.part == 0 {
                match walk_phase(p, h) {
                    None => return None,
                    Some(Some(why)) => info!("pilot: step {}: {marker} is not reached ({why}): the player waits where it is", p.i),
                    Some(None) => {}
                }
                p.part = 1;
            }
            if p.waited >= WAIT_HOURS {
                p.finding("never_came", format!("{who} did not come to {marker} in {WAIT_HOURS} hours of waiting there: moved as the headless run moves it"));
                p.headless("call (an actor that never came)");
                let mut applier = std::mem::take(&mut p.applier);
                let r = h.game(|g| runner::apply_now(g, &mut applier, &a));
                p.applier = applier;
                return Some(r.and_then(|r| r.err()));
            }
            if wait_an_hour(p, h) == Some(false) {
                // Refused (enemies near, or not here: Gomorrah01 is flagged Can't Wait): a
                // moment, then again, a few times; then as the headless run does it (the
                // step's time is reset below for the hours waited, so it never ran out:
                // Bye Bye Love asked the Wait menu 152 times in Gomorrah's lobby).
                p.part += 1;
                if p.part > WAIT_REFUSALS {
                    p.finding("wait_refused", format!("the Wait menu refused the player {WAIT_REFUSALS} times at {marker}, waiting for {who}: moved as the headless run moves it"));
                    p.headless("call (no waiting allowed there)");
                    let mut applier = std::mem::take(&mut p.applier);
                    let r = h.game(|g| runner::apply_now(g, &mut applier, &a));
                    p.applier = applier;
                    return Some(r.and_then(|r| r.err()));
                }
                p.phase = Phase::Wait { until: p.now + 5.0 };
            }
            p.step_from = p.now - 1.0;
            None
        }
        // An item the chain gives the player stands for the player finding it.
        Action::AddItem { to, item, count } if p.campaign && (to.eq_ignore_ascii_case("player") || to.eq_ignore_ascii_case("playerref")) => {
            let feet = h.feet();
            let world = h.world;
            let state = h.game(|g| {
                let it = runner::resolve(g, item).ok()?;
                let have = g.world.item_count(FormId::PLAYER_REF, it).unwrap_or(0);
                // Not across the map: a player gets such an item some other way (the tutorial
                // hints walked 37421 units to NCRCF for a Jet, met a guard's conversation and
                // the soft-lock watchdog, sweep 2 [observed]).
                // Nor in another worldspace or interior: the x/y distance means nothing across
                // them (Fort Gear Returned's 10mm pistol was fetched from the Divide, 314000
                // units, and the player died on the way, sweep 2 [observed]).
                let here = g.world.peek(FormId::PLAYER_REF).and_then(|s| s.cell);
                Some((it, have, find_item(g, it, feet).filter(|f| hdist(f.2, feet) < ITEM_FAR && same_space(world, here, f.1))))
            });
            let Some(Some((it, have, found))) = state else { return Some(Some(format!("{item}: no such item"))) };
            if p.part == 0 {
                // What the player had when the step began decides when it has enough.
                p.mark_items = have;
                p.part = 1;
            }
            if have >= p.mark_items + *count || (p.part == 1 && have >= *count && find_item_none(&found)) {
                info!("pilot: step {}: the player carries {have} {item}", p.i);
                return Some(None);
            }
            match found {
                Some((from, cell, pos, lying)) if p.part == 1 => {
                    let space = cell.and_then(|c| crate::nav::space_of_cell(world, c.0)).unwrap_or(space_of(h.location));
                    info!("pilot: step {}: {item} is to be found: the player goes for it", p.i);
                    p.errand = Some((Errand::Take { from, item: it, count: *count, lying }, walk_to(Place { space, pos }, None, NEAR_TARGET, p.now, feet), p.now, 0));
                    p.part = 2;
                    None
                }
                _ => {
                    if p.part == 1 {
                        p.finding("item_not_found", format!("{item} is not on any body, in any container or lying anywhere in the cells loaded: given as the headless run gives it"));
                    }
                    p.headless("add_item (not found in the world)");
                    // The walk for it, if one is still going, is over too: it outlived the
                    // step by 90 s in the tutorial hints (sweep 2 [observed]).
                    if matches!(&p.errand, Some((Errand::Take { item: i, .. }, ..)) if *i == it) {
                        p.errand = None;
                    }
                    let mut applier = std::mem::take(&mut p.applier);
                    let r = h.game(|g| runner::apply_now(g, &mut applier, &a));
                    p.applier = applier;
                    Some(r.and_then(|r| r.err()))
                }
            }
        }
        // The player's own stance: its control, not the script's flag.
        Action::PlayerSneak { sneaking } => {
            h.control.sneak = *sneaking;
            info!("pilot: step {}: the player {}", p.i, if *sneaking { "sneaks" } else { "stands up" });
            Some(None)
        }
        Action::SayFirst { .. } | Action::SayToDone { .. } if p.campaign => {
            info!("pilot: step {}: left to the game: a line the headless run says for an actor", p.i);
            Some(None)
        }
        Action::WaitMenu { menu, max_frames, label, .. } => {
            let limit = (*max_frames as f64 / 60.0).clamp(5.0, WAIT_CAP);
            let open = h.game(|g| g.menu_open(*menu)) == Some(true);
            if open || p.now - p.step_from > limit {
                if !open {
                    p.finding("wait_timed_out", format!("menu {menu} did not open in {limit:.0} s"));
                }
                p.observe(label, &Map::from_iter([("found".to_string(), json!(open))]), true);
                return Some((!open).then(|| format!("menu {menu} did not open")));
            }
            None
        }
        Action::GetMenus { menus, label } => {
            let read = h.game(|g| menus.iter().map(|c| (format!("menu.{c}"), json!(g.menu_open(*c)))).collect::<Map<String, Json>>()).unwrap_or_default();
            p.observe(label, &read, true);
            Some(None)
        }
        Action::PressButton { button } => {
            // As the play loop answers a box: the button is set, the box stays a moment so
            // that MenuMode blocks read it, then it closes.
            if p.part == 0 {
                let set = h.game(|g| match g.message_box.as_ref() {
                    Some(m) => m.can_press(*button).map(|_| true),
                    None => Ok(false),
                });
                return match set {
                    Some(Ok(true)) => {
                        // As a player's click (#2459).
                        h.script.answer(Some(*button), "quest pilot");
                        (p.part, p.phase) = (1, Phase::Wait { until: p.now + 0.3 });
                        None
                    }
                    Some(Ok(false)) => (p.now - p.step_from > 10.0).then(|| Some("no message box opened in 10 s".to_string())),
                    Some(Err(e)) => Some(Some(e)),
                    None => Some(Some("the scripts are not running".into())),
                };
            }
            match p.phase {
                Phase::Wait { until } if p.now < until => None,
                _ => Some(None),
            }
        }
        Action::DismissMessageBoxes { max, wait_frames, label } => {
            // `part` counts the boxes closed; the wait for each starts again in `phase`.
            let wait = (*wait_frames as f64 / 60.0).clamp(1.0, 30.0);
            if matches!(p.phase, Phase::Begin) {
                p.phase = Phase::Wait { until: p.now + wait };
            }
            let up = h.game(|g| g.menu == Some(fnv_game::game::menus::MESSAGE) && g.message_box.as_ref().is_none_or(|m| m.waiting())) == Some(true);
            if up && h.script.answer(None, "quest pilot") {
                p.part += 1;
                p.phase = Phase::Wait { until: p.now + wait };
            }
            let over = p.part as u32 >= *max || matches!(p.phase, Phase::Wait { until } if p.now >= until);
            if over {
                if let Some(l) = label {
                    p.observe(l, &Map::from_iter([("closed".to_string(), json!(p.part))]), true);
                }
                return Some(None);
            }
            None
        }
        // An AI walk the chain stands in for by ending its package (Liberty Prime's march
        // to Project Purity, MQ11PrimeToGatePKG04): in a campaign the player goes along with
        // the actor, as a player escorts it, until the package has run and ended. Left
        // behind, the player ran ahead past the force fields Prime breaks, and the march's
        // end never came (Take It Back, 2026-11-18).
        Action::Package { actor, package, event } if p.campaign && event.eq_ignore_ascii_case("end") => {
            let (r, place) = match h.place_of(actor) {
                Ok(x) => x,
                Err(e) => return Some(Some(e)),
            };
            let wanted = h.game(|g| runner::resolve(g, package).ok()).flatten();
            let now_running = h.game(|g| g.extra.get(&r).and_then(|e| e.current_package)).flatten();
            // Already run and left before the step came: neither waited for nor replayed.
            // General Chase's move-to-marker package was over and his briefing on when the
            // step began; its end script, replayed after 900 s, undid OA3Explain and the
            // tent door never opened (Paving the Way, rerun 3 [observed]). Liberty Prime's
            // march was never his package before its step, so it is still waited for.
            if p.part == 0 && wanted.is_some() && now_running != wanted && p.pkg_history.get(&r).is_some_and(|v| v.iter().position(|x| Some(*x) == wanted).is_some_and(|i| i + 1 < v.len())) {
                info!("pilot: step {}: {actor}'s {package} already ran and ended: not replayed", p.i);
                (h.intent.forward, h.intent.right) = (0.0, 0.0);
                return Some(None);
            }
            if wanted.is_some() && now_running == wanted && p.part == 0 {
                p.part = 1;
                info!("pilot: step {}: {actor} is on {package}: the player goes along until it ends", p.i);
            }
            if p.part == 1 && now_running != wanted {
                info!("pilot: step {}: {actor}'s {package} ended {:.0} s into the step", p.i, p.now - p.step_from);
                (h.intent.forward, h.intent.right) = (0.0, 0.0);
                return Some(None);
            }
            if p.now - p.step_from > PACKAGE_FOLLOW_SECS {
                p.finding("package_not_ended", format!("{actor}'s {package} did not {} in {PACKAGE_FOLLOW_SECS:.0} s with the player alongside: ended as the headless run ends it", if p.part == 0 { "begin" } else { "end" }));
                p.headless("package (an AI walk not over)");
                let mut applier = std::mem::take(&mut p.applier);
                let r = h.game(|g| runner::apply_now(g, &mut applier, &a));
                p.applier = applier;
                return Some(r.and_then(|r| r.err()));
            }
            // A script that moves the player out of the actors' space while it stands by
            // them (not a door it walks through) has taken it on in the scene: the walks
            // left behind can no longer be watched, and walking back is no player's way.
            // The guards' MovetoPosition in MQ08 runs on in Vault 87 while the capture
            // wakes the player in Raven Rock behind a level 255 door (Finding the Garden
            // of Eden, round 8, 2026-11-20).
            let here = space_of(h.location);
            if p.pkg_was_in == Some(place.space) && here != place.space && !matches!(p.phase, Phase::Walk(_)) {
                p.scene_left = Some(place.space);
            }
            p.pkg_was_in = Some(here);
            if p.scene_left == Some(here) {
                p.scene_left = None;
            }
            // Out of the scene, a walk that is not running is one the scene already had: Colonel
            // Autumn walks up to the player in Vault 87 before the fade, then a script puts
            // her in Raven Rock beside it (the same run, step 34, 13 minutes waited).
            let over = p.scene_left.is_some() && p.part == 0 && now_running != wanted;
            if (p.scene_left == Some(place.space) && here != place.space) || over {
                info!("pilot: step {}: a script moved the player away from {actor}: its {package} is left to the AI", p.i);
                p.headless("package (a script moved the player away)");
                (h.intent.forward, h.intent.right) = (0.0, 0.0);
                p.phase = Phase::Begin;
                return Some(None);
            }
            let mut phase = std::mem::replace(&mut p.phase, Phase::Begin);
            let away = here != place.space || hdist(h.feet(), place.pos) > PACKAGE_FOLLOW_NEAR * 1.5;
            match &mut phase {
                Phase::Walk(w) => {
                    if !matches!(walk(p, w, h), Walked::Going) {
                        phase = Phase::Begin;
                    }
                }
                _ if away => phase = Phase::Walk(walk_to(place, Some(r), PACKAGE_FOLLOW_NEAR, p.now, h.feet())),
                _ => (h.intent.forward, h.intent.right) = (0.0, 0.0),
            }
            p.phase = phase;
            None
        }
        Action::Package { actor, package, event } => {
            info!("pilot: step {}: left to the AI: {actor}'s package {package} ({event})", p.i);
            Some(None)
        }
        Action::PatrolPoint { actor, point } => {
            info!("pilot: step {}: left to the AI: {actor} reaching {point}", p.i);
            if let Some((a, at)) = h.game(|g| runner::resolve(g, actor).ok().zip(runner::resolve(g, point).ok())).flatten() {
                p.ai_due.push(AiDue { step: p.i, actor: a, at, within: AI_DUE_NEAR, what: format!("{actor} reaching {point}"), since: p.now });
            }
            Some(None)
        }
        Action::DialoguePackage { actor, .. } => {
            if p.part == 0 {
                info!("pilot: step {}: left to the AI: {actor}'s dialogue package", p.i);
                p.part = 1;
                p.step_lines = h.game(|g| g.softlock_seen.lines_begun).unwrap_or(0);
            }
            // When the chain answers it next (a choice, the talk's end), the actor walks up
            // and opens the conversation first: the player stands by until it does, as a
            // player would, instead of choosing in no conversation.
            let answered = p.actions.iter().skip(p.i + 1).find(|a| !matches!(a, Action::Note { .. })).is_some_and(|a| matches!(a, Action::WaitTalk { .. } | Action::Choose { .. }));
            if !answered {
                return Some(None);
            }
            let who = h.game(|g| runner::resolve(g, actor).ok()).flatten();
            let open = h.game(|g| g.dialogue.as_ref().map(|c| c.actor)).flatten();
            // Or it says its line to the player with no conversation (a Say To package, a
            // Goodbye greeting: Dad's radroach warning from 643 units [observed]).
            // A line begun in this step: the line before it may still be said (Dad's
            // radroach warning was taken for his call to the picture [observed]).
            let mark = p.step_lines;
            let said = who.is_some_and(|w| h.game(|g| g.speech.iter().any(|s| s.speaker == w && s.target == Some(FormId::PLAYER_REF) && g.softlock_seen.lines_since(mark).any(|l| l == s.info))) == Some(true));
            match open {
                Some(o) if who.is_none_or(|w| w == o) => {
                    info!("pilot: step {}: {actor} opened the conversation after {:.1} s", p.i, p.now - p.step_from);
                    Some(None)
                }
                None if said => {
                    info!("pilot: step {}: {actor} says its line to the player after {:.1} s: no conversation to answer", p.i, p.now - p.step_from);
                    Some(None)
                }
                _ if p.now - p.step_from > DIALOGUE_PACKAGE_SECS => {
                    p.finding("wait_timed_out", format!("{actor}'s dialogue package opened no conversation with the player in {DIALOGUE_PACKAGE_SECS:.0} s"));
                    Some(None)
                }
                _ => None,
            }
        }
        _ if !runner::applies_now(&a) => {
            p.finding("not_carried_out", "the pilot has no way to do this step yet".into());
            Some(None)
        }
        // Everything else is the headless run's own step on the running game.
        _ => {
            // Reads change nothing; the rest is a step with no player means yet.
            let op = p.ops.get(p.i).cloned().unwrap_or_default();
            if !(op.starts_with("get_") || op == "note" || op == "start_stopwatch" || op == "query") {
                p.headless(&op);
            }
            // A chain's damage to the player stands in for a fall or a fight a player
            // takes (Wasteland Survival Guide's serious injury, MS03b3): the pilot's god
            // mode, which keeps scripts from hurting the player (#2792), is lifted for that
            // step alone, or the injury never comes (round 8, 2026-11-20: Health 205 before
            // and after).
            let hurts = matches!(&a, Action::DamageAv { of, .. } if of.as_deref().is_none_or(|o| o.eq_ignore_ascii_case("player")));
            if hurts && nvf_play::combat::god_mode() {
                info!("pilot: step {}: god mode lifted for the chain's damage to the player", p.i);
            }
            let mut applier = std::mem::take(&mut p.applier);
            let r = h.game(|g| {
                let god = g.toggles.god_mode;
                g.toggles.god_mode &= !hurts;
                let r = runner::apply_now(g, &mut applier, &a);
                g.toggles.god_mode = god;
                r
            });
            p.applier = applier;
            match r {
                None => Some(Some("the scripts are not running".into())),
                Some(Err(e)) => {
                    if let Some(l) = runner::label_of(&a) {
                        p.observe(&l, &Map::from_iter([("errors".to_string(), json!({"action": e}))]), true);
                    }
                    Some(Some(e))
                }
                Some(Ok(None)) => Some(None),
                Some(Ok(Some((label, fields)))) => {
                    p.observe(&label, &fields, true);
                    Some(None)
                }
            }
        }
    }
}

/// One frame of waiting an hour on the game's Wait menu. `Some(true)`: the hour has
/// passed; `Some(false)`: the game would not have it (refused, or the menu never came).
fn wait_an_hour(p: &mut Pilot, h: &mut Hands) -> Option<bool> {
    use nvf_play::wait::{OpenWait, WaitInput};
    match p.waiting {
        None => {
            h.wait_open.write(OpenWait::default());
            p.waiting = Some(p.now);
            None
        }
        Some(since) if h.wait_menu.is_open() => {
            // Asked for an hour once the menu is up; it closes when the hour has passed.
            if p.now - since < 100.0 {
                h.wait_input.write(WaitInput(nvf_play::wait::model::Command::Hours(1)));
                h.wait_input.write(WaitInput(nvf_play::wait::model::Command::Accept));
                p.waiting = Some(p.now - 100.0);
            } else if p.now - since > 160.0 {
                h.wait_input.write(WaitInput(nvf_play::wait::model::Command::Cancel));
                p.waiting = None;
                return Some(false);
            }
            None
        }
        // The menu is gone again: the hour passed, or it never opened.
        Some(since) if p.now - since >= 100.0 => {
            p.waiting = None;
            p.waited += 1;
            Some(true)
        }
        Some(since) if p.now - since > 4.0 => {
            p.waiting = None;
            Some(false)
        }
        Some(_) => None,
    }
}

/// What a body carries that a player would take: quest items and keys.
fn worth_taking(g: &Game, body: FormId) -> Vec<(FormId, i32)> {
    let Some(inv) = g.world.peek(body).and_then(|s| s.inventory.as_ref()) else { return Vec::new() };
    inv.iter().filter(|&(item, n)| n > 0 && (g.world.is_quest_item(item) || g.world.forms.item(item).is_some_and(|it| it.kind == fnv_sim::forms::ItemKind::Key))).collect()
}

/// Who holds `item` in the cells the game has materialised, nearest first, or where it
/// lies: the holder (or the item's own reference), its cell and place, and whether it
/// lies in the world.
fn find_item(g: &Game, item: FormId, feet: [f32; 3]) -> Option<(FormId, Option<FormId>, [f32; 3], bool)> {
    let mut found: Vec<Found> = Vec::new();
    // A disabled reference is not in the world for the player (TTW keeps a slave outfit
    // in a container 0x0C0088AB, enable parent 0x10008DEE, in a cell without navmesh).
    let there = |r: FormId| g.world.is_enabled(r).unwrap_or(true);
    for (r, state) in g.world.changed() {
        if r == FormId::PLAYER_REF || !there(r) {
            continue;
        }
        let holds = state.inventory.as_ref().is_some_and(|inv| inv.iter().any(|(i, n)| i == item && n > 0));
        let lies = state.base == item && state.inventory.is_none();
        // A living actor's things are not there for the taking.
        let living_actor = g.data.actors.contains_key(&state.base) && !g.is_dead(r);
        if (holds && !living_actor) || lies {
            found.push((hdist(state.pos, feet), r, state.cell, state.pos, lies));
        }
    }
    for placed in g.world.placed.refs.values().filter(|pl| pl.base == item && g.world.peek(pl.id).is_none() && there(pl.id)) {
        found.push((hdist(placed.pos, feet), placed.id, placed.cell, placed.pos, true));
    }
    // On the player's side of the two wastelands first: the x/y distance means nothing
    // across them (Legion's and Bitter Springs' doctor's bag was fetched from the Capital
    // Wasteland, and no travel brought the player back, rounds 13 and 15 [observed]). A
    // cell's side is that of the plugin that defines it, as for worldspaces.
    let side = |cell: Option<FormId>| cell.is_some_and(|c| capital_side(g, c.0));
    let mine = side(g.world.peek(FormId::PLAYER_REF).and_then(|s| s.cell));
    found.sort_by(|a, b| (side(a.2) != mine).cmp(&(side(b.2) != mine)).then(a.0.total_cmp(&b.0)));
    found.first().map(|&(_, r, cell, pos, lies)| (r, cell, pos, lies))
}

/// One frame of the errand in hand. Whether it is over.
fn run_errand(p: &mut Pilot, h: &mut Hands) -> bool {
    use nvf_play::terminal::{AidPick, TerminalInput};
    let Some((errand, mut w, began, mut stage)) = p.errand.take() else { return true };
    // A speaker not reached at once is left: the collar's count runs on meanwhile, and a
    // player out of its reach walks out of its range instead. Gala's walk to one in the
    // Villa was 8651 units round a building; the collar went off on the way (round 19
    // [observed]).
    if let Errand::Silence(speaker) = errand {
        if p.now - began > SILENCE_SECS {
            let done = h.script.rt.as_mut().map(|rt| with_world(rt, h.sim, |rt| rt.activate_as_player(speaker)));
            info!("pilot: step {}: the speaker {speaker} is not reached in {SILENCE_SECS:.0} s: switched off where it is (a stand-in for a shot at it): {}", p.i, outcome(done.as_ref()));
            p.headless("silence (a speaker out of reach)");
            silence_the_rest(p, h);
            return true;
        }
    }
    if p.now - began > ERRAND_SECS {
        info!("pilot: step {}: errand given up after {ERRAND_SECS:.0} s: {errand:?}", p.i);
        if h.terminal.0.is_some() {
            h.terminal_input.write(TerminalInput::Exit);
        }
        return true;
    }
    if stage == 0 {
        match walk(p, &mut w, h) {
            Walked::Going => {
                p.errand = Some((errand, w, began, stage));
                return false;
            }
            Walked::No(why) => {
                info!("pilot: step {}: errand not reached ({why}): {errand:?}", p.i);
                // A speaker with no way to it is switched off where it is, as one past
                // its time ([`SILENCE_SECS`]).
                if let Errand::Silence(speaker) = errand {
                    let done = h.script.rt.as_mut().map(|rt| with_world(rt, h.sim, |rt| rt.activate_as_player(speaker)));
                    info!("pilot: step {}: switches off the speaker {speaker} where it is (a stand-in for a shot at it): {}", p.i, outcome(done.as_ref()));
                    p.headless("silence (a speaker out of reach)");
                }
                return true;
            }
            Walked::There | Walked::Put => stage = 1,
        }
    }
    match &errand {
        Errand::Away(speaker) => {
            info!("pilot: step {}: out of the range of speaker {speaker}", p.i);
            true
        }
        Errand::Silence(speaker) => {
            let speaker = *speaker;
            let done = h.script.rt.as_mut().map(|rt| with_world(rt, h.sim, |rt| rt.activate_as_player(speaker)));
            info!("pilot: step {}: switches off the beeping speaker {speaker}: {}", p.i, outcome(done.as_ref()));
            true
        }
        Errand::Search(body) => {
            let body = *body;
            let taken = h.game(|g| {
                let take = worth_taking(g, body);
                take.iter().filter(|(item, n)| g.world.remove_item(body, *item, *n).is_ok() && g.world.add_item(FormId::PLAYER_REF, *item, *n).is_ok()).map(|(item, n)| format!("{n} {}", g.data.name(*item))).collect::<Vec<_>>()
            });
            let taken = taken.unwrap_or_default();
            if !taken.is_empty() {
                let name = h.game(|g| g.data.name(body)).unwrap_or_default();
                info!("pilot: step {}: searches the body of {name} and takes {}", p.i, taken.join(", "));
            }
            true
        }
        Errand::Take { from, item, count, lying } => {
            let (from, item, count, lying) = (*from, *item, *count, *lying);
            let name = h.game(|g| g.data.name(item)).unwrap_or_default();
            if lying {
                let done = h.script.rt.as_mut().map(|rt| with_world(rt, h.sim, |rt| rt.activate_as_player(from)));
                info!("pilot: step {}: picks up {name} where it lies: {}", p.i, outcome(done.as_ref()));
            } else {
                let ok = h.game(|g| {
                    let have = g.world.item_count(from, item).unwrap_or(0).min(count.max(1));
                    have > 0 && g.world.remove_item(from, item, have).is_ok() && g.world.add_item(FormId::PLAYER_REF, item, have).is_ok()
                });
                let holder = h.game(|g| g.data.name(from)).unwrap_or_default();
                info!("pilot: step {}: takes {name} from {holder}: {}", p.i, if ok == Some(true) { "taken" } else { "not there any more" });
            }
            true
        }
        Errand::Terminal { reference, base, item, door } => {
            let (reference, base, item, door) = (*reference, *base, *item, *door);
            if p.now - p.terminal_at < TERMINAL_INPUT_SECS {
                p.errand = Some((errand, w, began, stage));
                return false;
            }
            p.terminal_at = p.now;
            match (stage, screen_of(h.terminal, reference)) {
                (1, Screen::None) | (1, Screen::Other) => {
                    let done = h.script.rt.as_mut().map(|rt| with_world(rt, h.sim, |rt| rt.activate_as_player(reference)));
                    info!("pilot: step {}: uses the terminal that works door {door:08X}: {}", p.i, outcome(done.as_ref()));
                    stage = 2;
                }
                (_, Screen::Hacking) => h.terminal_input.write(TerminalInput::Aid(AidPick::Right)).into_unit(),
                (_, Screen::Menu(_, _, _, true)) => h.terminal_input.write(TerminalInput::Back).into_unit(),
                (_, Screen::Menu(shown, listed, _, false)) if stage < 3 => {
                    let row = if shown == base {
                        listed.iter().position(|i| *i == item)
                    } else {
                        h.game(|g| listed.iter().position(|i| fnv_game::terminal::item(g, shown, *i).is_some_and(|it| it.submenu == Some(base)))).flatten()
                    };
                    match row {
                        Some(row) => {
                            h.terminal_input.write(TerminalInput::Aid(AidPick::Item(row)));
                            if shown == base {
                                info!("pilot: step {}: takes the entry that works door {door:08X}", p.i);
                                stage = 3;
                            }
                        }
                        None => {
                            info!("pilot: step {}: the entry that works door {door:08X} is not listed on the terminal now", p.i);
                            h.terminal_input.write(TerminalInput::Exit);
                            return true;
                        }
                    }
                }
                (3, Screen::Menu(..)) => h.terminal_input.write(TerminalInput::Exit).into_unit(),
                (3, _) => return true,
                (_, Screen::LockedOut) => {
                    h.terminal_input.write(TerminalInput::Exit);
                    return true;
                }
                _ => {}
            }
            p.errand = Some((errand, w, began, stage));
            false
        }
    }
}

/// `()` from a message's id, for the match arms above.
trait IntoUnit {
    fn into_unit(self);
}
impl<T> IntoUnit for T {
    fn into_unit(self) {}
}

/// What the terminal screen shows, as far as the pilot needs it.
enum Screen {
    None,
    Other,
    Hacking,
    LockedOut,
    /// The menu up (its terminal form), the entries listed, whether it has a parent, and
    /// whether a note is open over it.
    Menu(FormId, Vec<usize>, bool, bool),
}

fn screen_of(t: &nvf_play::terminal::TerminalScreen, reference: FormId) -> Screen {
    use nvf_play::terminal::Page;
    match &t.0 {
        None => Screen::None,
        Some(s) if s.reference != reference => Screen::Other,
        Some(s) => match &s.page {
            Page::Hacking(_) => Screen::Hacking,
            Page::LockedOut(_) => Screen::LockedOut,
            Page::Menu(v) => Screen::Menu(v.terminal, v.items.clone(), !v.parents.is_empty(), v.note.is_some()),
        },
    }
}

/// One frame of a `terminal` step worked at the terminal's screen (module notes).
fn terminal_step(p: &mut Pilot, h: &mut Hands, target: &str, item: usize, menu: Option<&str>, label: Option<&str>) -> Option<Option<String>> {
    use nvf_play::terminal::{AidPick, TerminalInput};
    let Some(Ok(reference)) = h.game(|g| runner::resolve(g, target)) else { return Some(Some(format!("{target}: no such reference"))) };
    // Its screen is up already (the step before used it): no walk.
    if p.part == 0 && !matches!(screen_of(h.terminal, reference), Screen::None | Screen::Other) {
        (p.part, p.phase) = (1, Phase::Wait { until: p.now + TERMINAL_SECS });
    }
    if p.part == 0 {
        match approach(p, h, target, "use") {
            Err(e) => return Some(Some(e)),
            Ok(None) => return None,
            Ok(Some(r)) => {
                let done = h.script.rt.as_mut().map(|rt| with_world(rt, h.sim, |rt| rt.activate_as_player(r)));
                info!("pilot: step {}: activates the terminal {target}: {}", p.i, outcome(done.as_ref()));
                p.terminal_at = p.now;
                (p.part, p.phase) = (1, Phase::Wait { until: p.now + TERMINAL_SECS });
                return None;
            }
        }
    }
    let timed_out = !matches!(p.phase, Phase::Wait { until } if p.now < until);
    if p.part == 2 {
        // The entry was taken: a moment for its script and its page, then on.
        if timed_out {
            if let Some(l) = label {
                p.observe(l, &Map::new(), true);
            }
            return Some(None);
        }
        return None;
    }
    if p.now - p.terminal_at < TERMINAL_INPUT_SECS {
        return None;
    }
    let send = |p: &mut Pilot, h: &mut Hands, input: TerminalInput| {
        h.terminal_input.write(input);
        p.terminal_at = p.now;
    };
    match screen_of(h.terminal, reference) {
        Screen::None if timed_out => Some(Some(format!("the screen of {target} did not come up in {TERMINAL_SECS:.0} s (refused: locked beyond the player's Science, or its script took the activation)"))),
        // Nothing came up and nothing is in the way now: the player presses again (a
        // message box up at that moment had the game drop the request).
        Screen::None if p.now - p.terminal_at > 3.0 && h.game(|g| g.menu.is_none() && g.dialogue.is_none()) == Some(true) => {
            let done = h.script.rt.as_mut().map(|rt| with_world(rt, h.sim, |rt| rt.activate_as_player(reference)));
            info!("pilot: step {}: no screen yet: activates {target} again: {}", p.i, outcome(done.as_ref()));
            p.terminal_at = p.now;
            None
        }
        Screen::None => None,
        Screen::Other => {
            send(p, h, TerminalInput::Exit);
            None
        }
        Screen::LockedOut => {
            send(p, h, TerminalInput::Exit);
            Some(Some(format!("{target} is locked out: a player could not use it")))
        }
        Screen::Hacking => {
            if timed_out {
                return Some(Some(format!("{target} was still on its hacking page after {TERMINAL_SECS:.0} s")));
            }
            info!("pilot: step {}: {target} is locked: the player enters the password (a stand-in for the word game)", p.i);
            send(p, h, TerminalInput::Aid(AidPick::Right));
            None
        }
        Screen::Menu(_, _, _, true) => {
            // A note is open over the menu: back to the list.
            send(p, h, TerminalInput::Back);
            None
        }
        Screen::Menu(shown, listed, has_parent, false) => {
            let wanted = h.game(|g| match menu {
                Some(m) => runner::resolve(g, m),
                None => g.world.placed.get(reference).map(|pl| pl.base).ok_or_else(|| format!("{target} is not a placed reference")),
            });
            let wanted = match wanted {
                Some(Ok(w)) => w,
                Some(Err(e)) => return Some(Some(e)),
                None => return Some(Some("the scripts are not running".into())),
            };
            if shown == wanted {
                return match listed.iter().position(|i| *i == item) {
                    Some(row) => {
                        let text = h.game(|g| fnv_game::terminal::item(g, shown, item).map(|it| it.text)).flatten().unwrap_or_default();
                        info!("pilot: step {}: takes entry {item} of {target}: {text:?}", p.i);
                        send(p, h, TerminalInput::Aid(AidPick::Item(row)));
                        (p.part, p.phase) = (2, Phase::Wait { until: p.now + TERMINAL_INPUT_SECS });
                        None
                    }
                    None => {
                        send(p, h, TerminalInput::Exit);
                        Some(Some(format!("entry {item} of {target} is not listed now (listed: {listed:?}): a player could not take it")))
                    }
                };
            }
            // Another menu of this terminal is up: into the sub menu wanted, or back out.
            let opener = h.game(|g| listed.iter().position(|i| fnv_game::terminal::item(g, shown, *i).is_some_and(|it| it.submenu == Some(wanted)))).flatten();
            match opener {
                Some(row) => {
                    info!("pilot: step {}: {target}: into the sub menu the step names (row {row})", p.i);
                    send(p, h, TerminalInput::Aid(AidPick::Item(row)));
                    None
                }
                None if has_parent => {
                    send(p, h, TerminalInput::Back);
                    None
                }
                None => {
                    send(p, h, TerminalInput::Exit);
                    Some(Some(format!("no entry listed on {target} opens the menu the step names: a player could not get to entry {item}")))
                }
            }
        }
    }
}

/// A body with something worth taking is searched before the step goes on, once.
fn search_later(p: &mut Pilot, h: &mut Hands, body: FormId) {
    if p.searched.contains(&body) || h.game(|g| worth_taking(g, body).is_empty()) != Some(false) {
        return;
    }
    p.searched.push(body);
    p.errands.push_back(Errand::Search(body));
}

/// One frame of a `kill` step fought out (module notes).
fn fight(p: &mut Pilot, h: &mut Hands, name: &str, a: &Action) -> Option<Option<String>> {
    if matches!(p.phase, Phase::Begin) {
        let (r, place) = match h.place_of(name) {
            Ok(x) => x,
            Err(e) => return Some(Some(e)),
        };
        arm(p, h);
        let Phase::Walk(mut w) = new_walk(place, Some(r), FIGHT_RANGE, p.now, h.feet()) else { unreachable!() };
        // A foe close by is fought from where the player stands when no navmesh joins the
        // two inside one interior (Vault 101's radroach in the BB gun range: put beside it,
        // the player was where Dad could not walk up for the picture, CG02 stopped at 70).
        w.stay = hdist(h.feet(), place.pos) < FIGHT_STAY_NEAR;
        w.hold = w.stay;
        p.phase = Phase::Fight(Fight { target: r, shooting: None, near: false, blocked: None, began: p.now, in_the_way: None, health: None, afar: false }, w);
    }
    let Phase::Fight(mut f, mut w) = std::mem::replace(&mut p.phase, Phase::Begin) else { return Some(None) };
    let target = f.target;
    // Not in the world (disabled, or not yet enabled by the quest): nothing a player could
    // fight. Shooting at where it would stand hit only the saloon's walls.
    if h.game(|g| runner::reference_field(g, &format!("0x{:08X}", target.0), "disabled").ok() == Some(json!(true))) == Some(true) {
        h.control.trigger = false;
        return Some(Some(format!("{name} is disabled: not in the world for the player to fight")));
    }
    if h.game(|g| g.is_dead(target)) == Some(true) {
        info!("pilot: step {}: {name} is dead after {:.1} s of the fight", p.i, p.now - p.step_from);
        h.control.trigger = false;
        search_later(p, h, target);
        return Some(None);
    }
    if p.now - p.step_from > FIGHT_SECS {
        p.finding("fight_not_won", format!("{name} still lives after {FIGHT_SECS:.0} s of shooting at it with {PILOT_WEAPON}: killed as the headless run kills it"));
        p.headless("kill (a fight not won)");
        h.control.trigger = false;
        let mut applier = std::mem::take(&mut p.applier);
        let r = h.game(|g| runner::apply_now(g, &mut applier, a));
        p.applier = applier;
        return Some(r.and_then(|r| r.err()));
    }
    if let Some(why) = engage(p, h, name, &mut f, &mut w) {
        // Nowhere a player could reach and no line of fire either: the finding says so,
        // and the kill is the headless one, so the quest goes on and the rest is tested.
        p.take_back_unreachable();
        p.finding("target_unreachable", format!("{name}: {why}: killed as the headless run kills it"));
        p.headless("kill (a target out of reach)");
        h.control.trigger = false;
        let mut applier = std::mem::take(&mut p.applier);
        let r = h.game(|g| runner::apply_now(g, &mut applier, a));
        p.applier = applier;
        return Some(r.and_then(|r| r.err()));
    }
    p.phase = Phase::Fight(f, w);
    None
}

/// The longest the player goes along with an actor whose package a step waits to end,
/// seconds [design]: Liberty Prime's march takes minutes.
const PACKAGE_FOLLOW_SECS: f64 = 900.0;
/// How long the player stands by for a Dialogue package's actor to walk up and open the
/// conversation the chain answers next [hypothesis: a force-greet walk across a room].
const DIALOGUE_PACKAGE_SECS: f64 = 60.0;
/// How close a foe is for the fight to keep off a put across a gap inside one interior.
const FIGHT_STAY_NEAR: f32 = 1500.0;
/// How near the player keeps to that actor, game units [design].
const PACKAGE_FOLLOW_NEAR: f32 = 600.0;

/// How near the player goes to an object it shoots, game units [design].
const SHOOT_OBJECT_FROM: f32 = 600.0;
/// The longest a `call_until` waits, seconds: past [`STEP_SECS`], as end slides run for
/// minutes (the headless chains give them up to 30).
const CALL_UNTIL_CAP: f64 = 1800.0;
/// How long the run may go without a sign of progress before the stall watchdog gives the
/// step up, seconds: the step moving on, the player walking [`STALL_MOVE`] units, a line
/// said, an hour waited or the trigger pulled. Steps that wait on purpose (`call_until`,
/// `advance_seconds`, `wait_quest`, `wait_menu`, `wait_frames`) are left to their own limits.
const STALL_SECS: f64 = 90.0;
/// How far the player has to be from where it stood a while ago for its walking to count
/// as progress.
const STALL_MOVE: f32 = 150.0;
/// How often the stall watchdog notes where the player stands, seconds.
const STALL_SAMPLE: f64 = 5.0;

/// The stall watchdog (owner 2026-11-16: "a better way for the bot to detect when its
/// stalled"). The step's own limit ([`STEP_SECS`]) is reset by a step that keeps trying
/// (a refused Wait menu asked 152 times in Gomorrah's lobby), and a walk that cannot move
/// sat out its 240 s step after step: this one is reset by progress alone. On a stall it
/// says what the run looked like, a `stalled` finding, and gives the step up. Whether it
/// gave one up this frame.
fn watch_stall(p: &mut Pilot, h: &mut Hands) -> bool {
    let feet = h.feet();
    let said = h.game(|g| g.softlock_seen.lines_begun).unwrap_or(0);
    let (since, _, step, waited, lines) = p.stall;
    if p.trail.back().is_none_or(|(t, _)| p.now - t >= STALL_SAMPLE) {
        p.trail.push_back((p.now, feet));
    }
    while p.trail.len() > 1 && p.trail.get(1).is_some_and(|(t, _)| p.now - t >= STALL_SECS) {
        p.trail.pop_front();
    }
    // Ground gained: away from every place it stood between half and the whole of
    // STALL_SECS ago (going back and forth round a door comes back near one of them).
    let old: Vec<[f32; 3]> = p.trail.iter().filter(|(t, _)| p.now - t >= STALL_SECS * 0.5).map(|(_, at)| *at).collect();
    let moved = !old.is_empty() && old.iter().all(|at| hdist(feet, *at) > STALL_MOVE || (feet[2] - at[2]).abs() > STALL_MOVE);
    if moved {
        p.trail.clear();
    }
    if step != p.i || moved || waited != p.waited || lines != said || h.control.trigger {
        p.stall = (p.now, feet, p.i, p.waited, said);
        return false;
    }
    // Waits with limits of their own: a step's (`call_until`, `advance_seconds`), a walk's
    // for a door's owner (up to OWNER_WAIT) or for a door to come (up to ENABLE_WAIT).
    let walk_waits = match &p.phase {
        Phase::Walk(w) | Phase::Fight(_, w) | Phase::Shoot(_, w) => w.owner_wait.is_some() || w.enable_wait.is_some(),
        _ => false,
    };
    // A `wait_quest` / `wait_menu` / `wait_frames` waits out its own `max_frames` too: Vault
    // 101's birth waits up to 120 s for each of Dad's lines and CG00's timers, and the
    // watchdog gave them up at 90 s [observed].
    // A line waited for beside its speaker waits out LINE_SECS: given up at 90 s, the dam
    // panel's Yes Man line was not stood in and the chain went on without it (sweep 2,
    // 2026-11-22 [observed]).
    // So does one whose speaker is out of reach: its stand-in comes before the step's limit,
    // and given up at 90 s, Trouble on the Homefront's radio line behind Vault 101's locked
    // door failed the next 12 steps and the chain was abandoned (sweep 2 [observed]).
    let beside = matches!(p.actions.get(p.i), Some(Action::SayInfo { .. }));
    let purposeful = beside || walk_waits || matches!(p.actions.get(p.i), Some(Action::CallUntil { .. } | Action::AdvanceSeconds { .. } | Action::WaitQuest { .. } | Action::WaitMenu { .. } | Action::WaitFrames { .. } | Action::Package { .. }));
    if purposeful || p.i >= p.actions.len() || p.now - since < STALL_SECS {
        return false;
    }
    let op = p.actions.get(p.i).map(|a| format!("{a:?}").chars().take(80).collect::<String>()).unwrap_or_default();
    let phase = match &p.phase {
        Phase::Begin => "begin",
        Phase::Walk(_) => "walking",
        Phase::Wait { .. } => "waiting",
        Phase::Fight(..) => "fighting",
        Phase::Shoot(..) => "shooting",
        _ => "other",
    };
    let (menu, talk) = h.game(|g| (g.menu, g.dialogue.is_some())).unwrap_or((None, false));
    let why = format!(
        "no progress in {STALL_SECS:.0} s on step {} ({op}): the player at ({:.0}, {:.0}, {:.0}) in {:?}, {phase}; movement locked {}, fighting locked {}, menu {menu:?}, a conversation {talk}, an errand {}",
        p.i, feet[0], feet[1], feet[2], space_of(h.location), h.locks.movement, h.locks.fighting, p.errand.is_some()
    );
    warn!("pilot: stalled: {why}");
    p.finding("stalled", why);
    h.control.trigger = false;
    (h.intent.forward, h.intent.right) = (0.0, 0.0);
    p.errand = None;
    p.next(Some(format!("stalled: no progress in {STALL_SECS:.0} s")));
    p.stall = (p.now, feet, p.i, p.waited, said);
    true
}

/// How many refusals of the Wait menu a step that waits for an actor takes before it
/// gives up waiting.
const WAIT_REFUSALS: u8 = 6;
/// How far inside a trigger's faces the walk into it from its near side aims, game units.
const TRIGGER_INSET: f32 = 24.0;
/// How long a walk waits for a travel a script asked for, seconds.
const TRAVEL_WAIT: f64 = 10.0;
/// How often a walk says where the player is on its path, seconds.
const WALK_REPORT_SECS: f64 = 20.0;
/// Nearer than this the player steps back before it fires: from on top of the warhead
/// (the step before walks to it) none of the detonator's shots met it [observed 2026-11-15].
const SHOOT_OBJECT_NEAR: f32 = 150.0;
/// How long the player fires at an object before the step is done the headless way.
const SHOOT_OBJECT_SECS: f64 = 20.0;

/// A `hit` step the player makes (module notes): the step's weapon in hand, within
/// [`SHOOT_OBJECT_FROM`] of the object, aimed at it, the trigger held until one of its
/// shots runs the object's OnHit / OnHitWith blocks. A weapon the player does not carry,
/// or a shot that does not land in [`SHOOT_OBJECT_SECS`], is the headless hit.
fn shoot_object(p: &mut Pilot, h: &mut Hands, name: &str, weapon: &str, a: &Action) -> Option<Option<String>> {
    let headless = |p: &mut Pilot, h: &mut Hands, why: &str| {
        h.control.trigger = false;
        p.headless(why);
        let mut applier = std::mem::take(&mut p.applier);
        let r = h.game(|g| runner::apply_now(g, &mut applier, a));
        p.applier = applier;
        Some(r.and_then(|r| r.err()))
    };
    if matches!(p.phase, Phase::Begin) {
        let (r, place) = match h.place_of(name) {
            Ok(x) => x,
            Err(e) => return Some(Some(e)),
        };
        if p.no_shot.contains(&r) {
            return headless(p, h, "hit (no shot landed before)");
        }
        let Some(Ok(id)) = h.game(|g| runner::resolve(g, weapon)) else { return Some(Some(format!("no weapon {weapon}"))) };
        let w = &mut h.sim.world;
        if w.item_count(FormId::PLAYER_REF, id).unwrap_or(0) == 0 {
            info!("pilot: step {}: the player does not carry {weapon}: the hit is set off as the headless run does", p.i);
            return headless(p, h, "hit (the player does not carry the weapon)");
        }
        let held = w.inventory_mut(FormId::PLAYER_REF).is_ok_and(|inv| inv.equipped().into_iter().any(|i| i == id));
        if !held {
            match w.equip(FormId::PLAYER_REF, id) {
                Ok(_) => info!("pilot: step {}: the player takes {weapon} in hand to fire at {name}", p.i),
                Err(e) => warn!("pilot: step {}: could not take {weapon} in hand: {e}", p.i),
            }
        }
        // The pilot's own weapon goes back in hand at the next fight (`keep_weapon`).
        p.weapon_check = 0.0;
        let from = h.object_hits.and_then(|m| m.get(&r.0)).copied().unwrap_or(0);
        let mut w = walk_to(place, Some(r), SHOOT_OBJECT_FROM, p.now, h.feet());
        w.stay = true;
        p.phase = Phase::Shoot(Shoot { target: r, from, firing: None, backing: false }, w);
    }
    let Phase::Shoot(mut s, mut w) = std::mem::replace(&mut p.phase, Phase::Begin) else { return Some(None) };
    let hits = h.object_hits.and_then(|m| m.get(&s.target.0)).copied().unwrap_or(0);
    if hits > s.from {
        h.control.trigger = false;
        info!("pilot: step {}: a shot with {weapon} hit {name} after {:.1} s", p.i, p.now - p.step_from);
        return Some(None);
    }
    if s.firing.is_some_and(|t| p.now - t > SHOOT_OBJECT_SECS) {
        p.no_shot.push(s.target);
        p.finding("shot_not_landed", format!("{SHOOT_OBJECT_SECS:.0} s of firing {weapon} at {name} ran none of its hit scripts: hit as the headless run hits it"));
        return headless(p, h, "hit (no shot landed)");
    }
    let Some((_, pos)) = h.game(|g| where_is(g, s.target)).flatten() else { return Some(Some(format!("{name} is not in the world"))) };
    let eye = h.player.eye();
    let away = hdist(eye, pos);
    let in_range = (SHOOT_OBJECT_NEAR..=SHOOT_OBJECT_FROM).contains(&away) && (eye[2] - pos[2]).abs() < 400.0;
    if s.firing.is_none() && away < SHOOT_OBJECT_NEAR && !s.backing {
        // Back along the line from the object through the player (or the way it faces).
        let feet = h.feet();
        let (dx, dy) = if away > 1.0 { ((feet[0] - pos[0]) / away, (feet[1] - pos[1]) / away) } else { (-h.player.heading.sin(), -h.player.heading.cos()) };
        let back = SHOOT_OBJECT_NEAR + 100.0;
        let to = Place { space: space_of(h.location), pos: [pos[0] + dx * back, pos[1] + dy * back, feet[2]] };
        info!("pilot: step {}: {away:.0} units from {name}: steps back to fire at it", p.i);
        w = walk_to(to, None, 60.0, p.now, feet);
        w.stay = true;
        s.backing = true;
    }
    if s.firing.is_none() && !in_range {
        match walk(p, &mut w, h) {
            Walked::Going => {
                h.control.trigger = false;
                p.phase = Phase::Shoot(s, w);
                return None;
            }
            Walked::No(why) => {
                h.control.trigger = false;
                return Some(Some(why));
            }
            Walked::There | Walked::Put => {}
        }
    }
    (h.intent.forward, h.intent.right) = (0.0, 0.0);
    h.control.weapon_out = true;
    let cross: Option<Crosshair> = h.control.crosshair;
    // At the middle of its collision, as a player aims at what is drawn; its origin can
    // lie off it (Classic Inspiration's camera targets: a thin wall ~97 units off the
    // origin, and no photo ever landed, round 13 [observed]). Without collision, just
    // above the origin.
    let aim = h.phys.world.ref_collision_middle(s.target.0).unwrap_or([pos[0], pos[1], pos[2] + 20.0]);
    (h.player.heading, h.player.pitch) = Crosshair::aim_at(cross.as_ref(), eye, aim);
    let since = *s.firing.get_or_insert_with(|| {
        info!("pilot: step {}: fires {weapon} at {name} from {:.0} units (aimed at ({:.0}, {:.0}, {:.0}))", p.i, hdist(eye, pos), aim[0], aim[1], aim[2]);
        p.now
    });
    // A moment to turn onto it, then the trigger held (an automatic weapon fires on).
    h.control.trigger = p.now - since > 0.3;
    p.phase = Phase::Shoot(s, w);
    None
}

/// Whether nothing was found (for the `add_item` rule's reading).
fn find_item_none(found: &Option<(FormId, Option<FormId>, [f32; 3], bool)>) -> bool {
    found.is_none()
}

fn wait_for(p: &mut Pilot, seconds: f64) -> Option<Option<String>> {
    (p.now - p.step_from >= seconds).then_some(None)
}

/// A step that is one walk: over when the player is there.
fn walk_phase(p: &mut Pilot, h: &mut Hands) -> Option<Option<String>> {
    let mut phase = std::mem::replace(&mut p.phase, Phase::Begin);
    let out = match &mut phase {
        Phase::Walk(w) => match walk(p, w, h) {
            Walked::Going => None,
            Walked::No(why) => Some(Some(why)),
            Walked::There | Walked::Put => Some(None),
        },
        Phase::Wait { until } => (p.now >= *until).then_some(None),
        _ => Some(None),
    };
    p.phase = phase;
    out
}

/// A menu of the engine's own up (the container menu Simon's search at the Silver Rush
/// opens): the pilot cannot act behind it, and no step works it. After [`BOX_SECS`] it is
/// closed as a player closes it, with a finding (the run was held for good before).
fn close_engine_menus(real: Res<Time<Real>>, mut pilot: ResMut<Pilot>, active: Res<nvf_play::menu::ActiveMenu>, mut input: MessageWriter<nvf_play::menu::MenuInput>, mut since: Local<Option<f64>>) {
    if pilot.done || active.0.is_none() {
        *since = None;
        return;
    }
    let now = real.elapsed_secs_f64();
    let t = *since.get_or_insert(now);
    if now - t > BOX_SECS * 2.0 {
        input.write(nvf_play::menu::MenuInput(nvf_play::menu::model::Command::Close));
        let p = &mut *pilot;
        info!("pilot: step {}: closes a menu of the engine's own (inventory or container) that no step works", p.i);
        *since = Some(now);
    }
}

/// `--pilot-minutes` of real time are up: the game exits, and [`report_on_exit`] writes
/// the report with where the run stopped (`run_ended`). The batch's hard kill a few
/// minutes later wrote nothing.
fn out_of_time(real: Res<Time<Real>>, pilot: Res<Pilot>, mut exit: MessageWriter<AppExit>, mut told: Local<bool>) {
    if pilot.done || *told || pilot.deadline.is_none_or(|d| real.elapsed_secs_f64() < d) {
        return;
    }
    *told = true;
    info!("pilot: {:.0} minutes of real time are up (--pilot-minutes): the run ends at step {}", real.elapsed_secs_f64() / 60.0, pilot.i);
    exit.write(AppExit::Success);
}

/// The game exits before the pilot is done (`--run-seconds`, a script's ExitGame at the
/// end slides, the window closed): the report is written all the same, with where it
/// stopped.
fn report_on_exit(mut exits: MessageReader<AppExit>, mut pilot: ResMut<Pilot>, watch: Option<Res<nvf_play::softlock::SoftlockWatch>>) {
    if exits.read().next().is_none() || pilot.done {
        return;
    }
    let p = &mut *pilot;
    p.done = true;
    let (i, total) = (p.i, p.actions.len());
    p.finding("run_ended", format!("the game exited at step {i} of {total} of chain {} (the time limit, or a script's ExitGame): the steps after it were not played", p.chain));
    let softlocks = watch.map_or(0, |w| w.found);
    if let Some(out) = &p.out {
        if let Err(e) = serde_json::to_string_pretty(&p.report(softlocks)).map_err(|e| e.to_string()).and_then(|t| std::fs::write(out, t).map_err(|e| e.to_string())) {
            error!("pilot: report {}: {e}", out.display());
        } else {
            info!("pilot: report written to {} (the game exited at step {i})", out.display());
        }
    }
}

/// The run ends when the pilot has not been able to act for [`HELD_SECS`] of real time.
fn held(real: Res<Time<Real>>, mut pilot: ResMut<Pilot>, watch: Option<Res<nvf_play::softlock::SoftlockWatch>>, state: Option<Res<State<PlayState>>>, mut exit: MessageWriter<AppExit>) {
    let p = &mut *pilot;
    let now = real.elapsed_secs_f64();
    let since = *p.acted_at.get_or_insert(now);
    if p.done || now - since < HELD_SECS {
        return;
    }
    p.done = true;
    p.finding("pilot_held", format!("the pilot could not act for {HELD_SECS:.0} s of real time (the game's state: {:?}): the run ends here", state.map(|s| *s.get())));
    let softlocks = watch.map_or(0, |w| w.found);
    if let Some(out) = &p.out {
        if let Err(e) = serde_json::to_string_pretty(&p.report(softlocks)).map_err(|e| e.to_string()).and_then(|t| std::fs::write(out, t).map_err(|e| e.to_string())) {
            error!("pilot: report {}: {e}", out.display());
        }
    }
    info!("pilot: done (held): {} chain(s), {} steps, {} divergences, {} findings, {softlocks} soft locks in {:.0} s", p.chains_done, p.steps.len(), p.divergences.len(), p.findings.len(), p.now);
    exit.write(AppExit::Success);
}

/// Where things are, for [`drive`].
type Places<'w> = (Res<'w, Location>, Res<'w, Target>, Option<Res<'w, crate::nav::NavState>>, Res<'w, crate::world::WorldData>, Res<'w, nvf_play::doors::Doors>);

#[allow(clippy::too_many_arguments)]
fn drive(
    real: Res<Time<Real>>,
    state: Res<State<PlayState>>,
    (loading, paused): (Res<Loading>, Res<nvf_play::pause::Paused>),
    mut pilot: ResMut<Pilot>,
    (mut script, mut sim, mut phys): (ResMut<ScriptState>, ResMut<SimState>, ResMut<PhysState>),
    (mut player, mut intent, mut control, locks): (ResMut<Player>, ResMut<MoveIntent>, ResMut<PlayerControl>, Res<ControlLocks>),
    (location, target, nav, world, doors): Places,
    (watch, combat): (Option<Res<nvf_play::softlock::SoftlockWatch>>, Option<Res<nvf_play::combat::CombatStats>>),
    (mut travel, mut save): (MessageWriter<TravelRequest>, MessageWriter<nvf_play::save::SaveRequest>),
    (terminal, mut terminal_input): (Res<nvf_play::terminal::TerminalScreen>, MessageWriter<nvf_play::terminal::TerminalInput>),
    (wait_menu, mut wait_open, mut wait_input): (Res<nvf_play::wait::WaitMenu>, MessageWriter<nvf_play::wait::OpenWait>, MessageWriter<nvf_play::wait::WaitInput>),
    mut exit: MessageWriter<AppExit>,
) {
    let p = &mut *pilot;
    // In play, in a conversation, and in a menu the scripts opened (the opening's menus,
    // a message box): not behind the pause menu or a loading screen.
    let scripts_menu = *state.get() == PlayState::Menu && !paused.0 && (wait_menu.is_open() || script.rt.as_ref().is_some_and(|rt| rt.game.menu.is_some()));
    if p.done || loading.active() || !(matches!(*state.get(), PlayState::Playing | PlayState::Dialogue) || scripts_menu) || script.rt.is_none() {
        return;
    }
    let dt = real.delta_secs().min(0.25);
    p.now += dt as f64;
    p.acted_at = Some(real.elapsed_secs_f64());
    if p.now < SETTLE_SECS {
        return;
    }
    let mut h = Hands {
        script: &mut script,
        sim: &mut sim,
        phys: &mut phys,
        player: &mut player,
        intent: &mut intent,
        control: &mut control,
        locks: &locks,
        location: &location,
        target: &target,
        nav: nav.as_deref(),
        world: &world,
        doors: &doors,
        in_menu: scripts_menu,
        wait_menu: &wait_menu,
        wait_open: &mut wait_open,
        wait_input: &mut wait_input,
        terminal: &terminal,
        terminal_input: &mut terminal_input,
        travel: &mut travel,
        object_hits: combat.as_deref().map(|c| &c.player_object_hits),
        dt,
    };
    // A dead player plays no more of the chain: a player would load a save here. Played
    // on, every later step stalled with the movement locked (Dead Money's collar, round 15
    // [observed]); the death is the finding.
    if p.given_applied && p.i < p.actions.len() && h.game(|g| g.is_dead(FormId::PLAYER_REF)) == Some(true) {
        let left = p.actions.len() - p.i;
        p.finding("player_died", format!("the player died at step {} of chain {}: the {left} steps left of it are not played (a player loads a save here)", p.i, p.chain));
        p.i = p.actions.len();
        (p.phase, p.part, p.step_from) = (Phase::Begin, 0, p.now);
        return;
    }
    if !p.given_applied {
        p.given_applied = true;
        if let Some(order) = h.game(|g| g.data.load_order.clone()) {
            remap_chain(p, &order);
        }
        let all = p.given.clone();
        // In a campaign the given state is a precondition (module notes).
        let (given, checked): (Vec<Json>, Vec<Json>) = match (p.campaign && !p.set_given, p.chains_done) {
            (false, _) => (all, Vec::new()),
            (true, 0) => (Vec::new(), Vec::new()),
            (true, _) => all.into_iter().filter(|e| e["kind"] != "menu" && e["kind"] != "place" && e["kind"] != "ref" && e["kind"] != "disabled").partition(|e| e["kind"] != "quest"),
        };
        for e in &checked {
            let Some(quest) = e["quest"].as_str() else { continue };
            let differing: Vec<String> = ["running", "completed", "stage"]
                .iter()
                .filter(|f| !e[**f].is_null())
                .filter_map(|f| match h.game(|g| runner::quest_field(g, quest, f)) {
                    Some(Ok(v)) if !same(&v, &e[*f]) => Some(format!("{f} is {v}, the chain starts from {}", e[*f])),
                    _ => None,
                })
                .collect();
            if !differing.is_empty() {
                p.finding("precondition", format!("{quest}: {}", differing.join("; ")));
            }
        }
        let errors = h.game(|g| runner::apply_given_entries(g, &given)).unwrap_or_default();
        // A chain played in Hardcore (its top-level `hardcore`, as the headless run's
        // SimConfig takes it): turned on as a player turns it on. Left off, Boulder City's
        // Hardcore chain played a normal game (round 13 [observed]).
        if p.chain_raw["hardcore"].as_bool() == Some(true) {
            h.game(|g| g.set_hardcore(true));
            info!("pilot: chain {}: Hardcore mode on", p.chain);
        }
        // In a campaign a line said in an earlier chain is said (ghost-town-gunfight opens
        // with the line intro ends on): the count is taken once, at the first chain.
        if !p.campaign || p.chains_done == 0 {
            p.mark = h.game(|g| g.softlock_seen.lines_begun).unwrap_or(0);
        }
        info!("pilot: chain {}: {} given entries applied, {} checked, {} errors", p.chain, given.len(), checked.len(), errors.len());
        for e in errors {
            p.finding("given", e.to_string());
        }
        p.step_from = p.now;
        return;
    }
    // Crouched, the player walks, as a player sneaking past a patrol does: at a run the
    // detection counts it running, and the X-13 robobrain heard the pilot at 400 units
    // (Old World Blues, round 9, 2026-11-20: "sneaking, running", sound 41.5).
    h.intent.walk = h.control.sneak;
    if watch_stall(p, &mut h) {
        return;
    }
    // A message box no step waits for: closed as a player presses OK.
    let waited_for = matches!(p.actions.get(p.i), Some(Action::PressButton { .. } | Action::DismissMessageBoxes { .. } | Action::WaitMenu { .. } | Action::GetMenus { .. } | Action::CloseMenus { .. }));
    let boxed = h.game(|g| g.menu == Some(fnv_game::game::menus::MESSAGE)) == Some(true);
    match (boxed && !waited_for, p.box_since) {
        (true, None) => p.box_since = Some(p.now),
        // Answered as a player's click answers it (`ScriptState::answer`): the box closes
        // after the frame's scripts and the player is back in the world before the box's
        // script reads the button. Closing the box from here instead had its script ask for
        // the pump station's terminal while the game was still in the menu (#2459).
        (true, Some(t)) if !p.box_picked && p.now - t > BOX_SECS => {
            let last = h.game(|g| g.message_box.as_ref().filter(|m| m.buttons > 1 && m.waiting()).and_then(|m| (0..m.buttons).rev().find(|b| !m.hidden.contains(b)))).flatten();
            let answered = h.script.answer(last.map(|b| b as i32), "quest pilot");
            match last {
                Some(b) => info!("pilot: step {}: a message box no step waits for has several buttons: picks the last, button {b}", p.i),
                None => info!("pilot: step {}: closes a message box no step waits for", p.i),
            }
            p.box_picked = answered;
            if !answered {
                p.box_since = None;
            }
        }
        // Still up long after the answer (another box in its place): looked at afresh.
        (true, Some(t)) if p.box_picked && p.now - t > BOX_SECS + 2.0 => (p.box_since, p.box_picked) = (None, false),
        (false, _) => (p.box_since, p.box_picked) = (None, false),
        _ => {}
    }
    // Another menu the scripts opened that no step waits for: the level-up menu is
    // answered as a player answers it, anything else closed after a while.
    // Not one a step soon to come waits for: the game is often ahead of the chain (the
    // opening's name menu was up 20 s before its wait_menu step, was closed here, and
    // the step then waited a minute for it).
    // Nor one the chain itself is about to close.
    let coming = |m: u16| {
        p.actions.iter().skip(p.i).take(24).any(|a| matches!(a, Action::WaitMenu { menu, .. } if *menu == m) || matches!(a, Action::GetMenus { menus, .. } if menus.contains(&m)))
            // A coming close_menus holds off any menu but the level-up menu: the walk before it
            // cannot go on with that one up (Wasteland Survival Guide levelled up in the
            // Super-Duper Mart two steps before a close_menus, and stalled there for good).
            || (m != fnv_game::game::menus::LEVEL_UP && p.actions.iter().skip(p.i).take(6).any(|a| matches!(a, Action::CloseMenus { .. })))
    };
    // (A terminal's pages are the `terminal` steps' and the rule above's.)
    let menu = h.game(|g| g.menu).flatten().filter(|m| *m != fnv_game::game::menus::MESSAGE && *m != nvf_play::script::MENU_SLEEP_WAIT && *m != nvf_play::terminal::MENU_HACKING && *m != nvf_play::terminal::MENU_TERMINAL && !waited_for && !coming(*m));
    match (menu, p.menu_since) {
        (Some(m), Some((was, t))) if m == was => {
            let up = p.now - t;
            if m == fnv_game::game::menus::LEVEL_UP && up > BOX_SECS {
                let mut told = String::new();
                let done = nvf_play::chargen::finish(h.script, h.sim, m, "the pilot's Done", |g| told = level_up(g));
                info!("pilot: step {}: level-up menu answered ({}): {told}", p.i, if done { "done" } else { "not open" });
                p.menu_since = None;
            } else if up > MENU_SECS {
                h.game(|g| g.close_all_menus());
                p.finding("menu_closed", format!("menu {m} was up for {MENU_SECS:.0} s with no step waiting for it: closed"));
                p.menu_since = None;
            }
        }
        (Some(m), _) => p.menu_since = Some((m, p.now)),
        (None, _) => p.menu_since = None,
    }
    // The packages of the actors the chain's `package … end` steps escort, as they change.
    if p.campaign && *state.get() != PlayState::Loading {
        let actors: Vec<String> = p.actions.iter().skip(p.i).filter_map(|a| match a {
            Action::Package { actor, event, .. } if event.eq_ignore_ascii_case("end") => Some(actor.clone()),
            _ => None,
        }).collect();
        if !actors.is_empty() {
            let now: Vec<(FormId, Option<FormId>)> = h.game(|g| actors.iter().filter_map(|n| runner::resolve(g, n).ok()).map(|r| (r, g.extra.get(&r).and_then(|e| e.current_package))).collect()).unwrap_or_default();
            for (r, pk) in now {
                let Some(pk) = pk else { continue };
                let v = p.pkg_history.entry(r).or_default();
                if v.last() != Some(&pk) {
                    v.push(pk);
                }
            }
        }
    }
    // A speaker that sets the collar counting down is switched off first, as a player
    // does when the collar beeps (Dead Money); before a fight too: the collar kills in
    // 10 s, and fighting Dean in the Tampico the player died of it (round 27 [observed]).
    if *state.get() == PlayState::Playing && p.errand.is_none() && p.now >= p.speaker_check {
        p.speaker_check = p.now + 1.0;
        if p.speakers.is_none() {
            p.speakers = h.game(|g| speakers(g));
        }
        let feet = h.feet();
        let list = p.speakers.clone().unwrap_or_default();
        let beeping = h.game(|g| beeping_speaker(g, &list, feet)).flatten();
        let fixed = beeping.filter(|s| h.game(|g| g.extra.get(s).is_some_and(|e| e.destroyed)) == Some(true));
        if let Some(speaker) = fixed {
            // One no player can switch off: out of its range, once a while.
            let now = p.now;
            p.away_from.retain(|(_, t)| now - *t < AWAY_AGAIN_SECS);
            if !p.away_from.iter().any(|(s, _)| *s == speaker) {
                info!("pilot: step {}: the collar beeps near speaker {speaker}, which no player can switch off: the player walks out of its range", p.i);
                p.away_from.push((speaker, now));
                p.errands.push_front(Errand::Away(speaker));
                p.defend = None;
            }
        } else if let Some(speaker) = beeping {
            if !p.errands.iter().any(|e| matches!(e, Errand::Silence(s) if *s == speaker)) {
                info!("pilot: step {}: the collar beeps near speaker {speaker}: switched off before the step goes on", p.i);
                p.errands.push_front(Errand::Silence(speaker));
                p.defend = None;
            }
        }
    }
    // Something attacks the player or the actor it is with: fought first (module notes).
    if *state.get() == PlayState::Playing && !matches!(p.phase, Phase::Fight(..) | Phase::Shoot(..)) {
        // Not while the collar's errand is in hand or waiting: it comes first.
        let collar = p.errands.iter().chain(p.errand.as_ref().map(|e| &e.0)).any(|e| matches!(e, Errand::Silence(_) | Errand::Away(_)));
        if p.defend.is_none() && !collar && p.now >= p.threat_check {
            p.threat_check = p.now + 0.5;
            let friend = match &p.phase {
                Phase::Walk(w) => w.target,
                Phase::Aim(a) => Some(a.target),
                _ => None,
            };
            let feet = h.feet();
            let world = h.world;
            let mut given_up = p.given_up.clone();
            // Not one the chain still talks to or hears a line from: a staged brawl's partner
            // throws the first punch, and killing her stalled the quest (Somah in Mothership
            // Zeta's cells: Not of This World needs her alive for DLC05MZ1Fight 6 [observed]).
            // A player does not kill someone the story still needs.
            let later: Vec<String> = p.actions.iter().skip(p.i).filter_map(|a| match a {
                Action::Talk { actor, .. } => Some(actor.clone()),
                Action::SayInfo { speaker: Some(s), .. } => Some(s.clone()),
                // Nor one a coming step strikes with a given weapon: shot dead in a fight
                // first, Wasteland Survival Guide's second mole rat was never struck with
                // the Repellent Stick and MS03b1 stayed at two of three (round 9, 2026-11-20).
                Action::Hit { target, weapon: Some(_), .. } => Some(target.clone()),
                _ => None,
            }).collect();
            if let Some(needed) = h.game(|g| later.iter().filter_map(|n| runner::resolve(g, n).ok()).collect::<Vec<FormId>>()) {
                given_up.extend(needed);
            }
            if let Some((who, cell, pos)) = h.game(|g| threat(g, friend, feet, &given_up)).flatten() {
                let space = cell.and_then(|c| crate::nav::space_of_cell(world, c.0)).unwrap_or(space_of(h.location));
                arm(p, &mut h);
                info!("pilot: step {}: {who} attacks {}: fought before the step goes on", p.i, if friend.is_some() { "the player or the actor it is with" } else { "the player" });
                p.defend = Some((Fight { target: who, shooting: None, near: false, blocked: None, began: p.now, in_the_way: None, health: None, afar: false }, walk_to(Place { space, pos }, Some(who), FIGHT_RANGE, p.now, feet)));
                p.defend_from = p.now;
            }
        }
        if let Some((mut f, mut w)) = p.defend.take() {
            let who = f.target;
            let dead = h.game(|g| g.is_dead(who)) == Some(true);
            // One that turns to run is let go, as a player lets a fleeing man go.
            if !dead && h.game(|g| fleeing(g, who)) == Some(true) {
                info!("pilot: step {}: the attacker {who} runs away: let go", p.i);
                h.control.trigger = false;
                p.given_up.push(who);
                return;
            }
            if dead || p.now - p.defend_from > DEFEND_SECS {
                h.control.trigger = false;
                if dead {
                    info!("pilot: step {}: the attacker {who} is dead after {:.1} s", p.i, p.now - p.defend_from);
                    search_later(p, &mut h, who);
                } else {
                    p.finding("fight_not_won", format!("the attacker {who} still lives after {DEFEND_SECS:.0} s of shooting at it: left alone from now on, the step goes on"));
                    p.given_up.push(who);
                }
            } else {
                if engage(p, &mut h, "the attacker", &mut f, &mut w).is_some() {
                    // Out of reach: left alone from now on.
                    p.given_up.push(who);
                    h.control.trigger = false;
                    return;
                }
                // Only through a door (a locked one, or another room's): left alone, as a
                // player does not go round by another floor after one who shoots from it.
                // Gomorrah's hostile casino drew the player back through the elevator and
                // up to a locked door after an attacker, behind the spot its walk out was
                // caught at, until the walk gave up (fail-kimball-graham step 75, rerun 4
                // [observed]).
                let through_door = w.locked_way || w.cut || w.follower.as_ref().is_some_and(|fl| fl.path.legs.len() > 1);
                if through_door && !f.afar {
                    info!("pilot: step {}: the way to the attacker {who} goes through a door: left alone, the step goes on", p.i);
                    p.given_up.push(who);
                    h.control.trigger = false;
                    return;
                }
                p.defend = Some((f, w));
                // The step's own time stands still meanwhile.
                p.step_from += dt as f64;
                p.still.0 += dt as f64;
                return;
            }
        }
    }
    // Errands (module notes): the one in hand, else the next, before the step goes on.
    if *state.get() != PlayState::Dialogue && p.defend.is_none() {
        if p.errand.is_none() {
            if let Some(next) = p.errands.pop_front() {
                let feet = h.feet();
                let world = h.world;
                let (place, radius) = match &next {
                    // Straight away from the speaker, to 200 units past its radius.
                    Errand::Away(s) => {
                        let s = *s;
                        let out = h.game(|g| {
                            let (_, at) = where_is(g, s)?;
                            let reach = g.ref_var_named(s, "iradius").filter(|r| *r > 0.0).unwrap_or(768.0) as f32 + 200.0;
                            let (dx, dy) = (feet[0] - at[0], feet[1] - at[1]);
                            let len = dx.hypot(dy).max(1.0);
                            Some([at[0] + dx / len * reach, at[1] + dy / len * reach, feet[2]])
                        });
                        (out.flatten().map(|pos| Place { space: space_of(h.location), pos }), 64.0)
                    }
                    Errand::Search(r) | Errand::Take { from: r, .. } | Errand::Terminal { reference: r, .. } | Errand::Silence(r) => {
                        let target = *r;
                        (h.game(|g| where_is(g, target)).flatten().and_then(|(cell, pos)| Some(Place { space: crate::nav::space_of_cell(world, cell?.0)?, pos })), NEAR_TARGET)
                    }
                };
                if let Some(place) = place {
                    p.errand = Some((next, walk_to(place, None, radius, p.now, feet), p.now, 0));
                }
            }
        }
        if p.errand.is_some() && !run_errand(p, &mut h) {
            p.step_from += dt as f64;
            return;
        }
    }
    // A terminal left open by a `terminal` step is left when another kind of step follows,
    // or a `terminal` step at another terminal: Agatha's Song's three mainframe stations
    // (MFLock01-03, SCPT 0x0A02FB51) stalled at the first one's screen [observed].
    let other_terminal = match (p.actions.get(p.i), terminal.0.as_ref()) {
        (Some(Action::Terminal { target, .. }), Some(open)) => h.game(|g| runner::resolve(g, target).ok()).flatten().is_some_and(|r| r != open.reference),
        _ => false,
    };
    if terminal.0.is_some() && (other_terminal || !matches!(p.actions.get(p.i), Some(Action::Terminal { .. }))) {
        if p.now - p.terminal_at >= TERMINAL_INPUT_SECS {
            info!("pilot: step {}: leaves the terminal", p.i);
            h.terminal_input.write(nvf_play::terminal::TerminalInput::Exit);
            p.terminal_at = p.now;
        }
        return;
    }
    check_ai_due(p, &mut h, false);
    // Steps that take no time follow one another within the frame.
    for _ in 0..64 {
        if p.i >= p.actions.len() {
            break;
        }
        match step(p, &mut h) {
            Some(error) => p.next(error),
            // No step holds the run up for good, whatever went wrong in it.
            // (An escorted package step keeps its own, longer limit: PACKAGE_FOLLOW_SECS.)
            None if p.now - p.step_from > STEP_SECS && !matches!(p.actions[p.i], Action::CallUntil { .. }) && !(p.campaign && matches!(&p.actions[p.i], Action::Package { event, .. } if event.eq_ignore_ascii_case("end"))) => {
                p.finding("step_timed_out", format!("the step was not over after {STEP_SECS:.0} s: given up"));
                h.control.trigger = false;
                p.next(Some("the step timed out".into()));
            }
            None => return,
        }
    }
    if p.i < p.actions.len() {
        return;
    }
    // The save after the chain is asked for once nothing holds the game, then waited for.
    if p.campaign && !p.saved {
        if *state.get() != PlayState::Playing {
            leave_talk(p, &mut h, false);
            return;
        }
        let name = format!("pilot-after-{}", p.chain);
        info!("pilot: chain {} over: saving {name}", p.chain);
        save.write(nvf_play::save::SaveRequest { name, kind: fnv_save::SaveKind::Manual });
        (p.saved, p.resume_at) = (true, p.now + SAVE_SECS);
        return;
    }
    if p.now < p.resume_at {
        return;
    }
    p.saved = false;
    check_ai_due(p, &mut h, true);
    p.chains_done += 1;
    info!("pilot: chain {} over at {:.0} s: {} divergences and {} findings so far", p.chain, p.now, p.divergences.len(), p.findings.len());
    if p.next_chain() {
        return;
    }
    p.done = true;
    let softlocks = watch.map_or(0, |w| w.found);
    let failed = p.steps.iter().filter(|s| s.get("error").is_some()).count();
    info!("pilot: done: {} chain(s), {} steps ({failed} failed), {} divergences, {} findings, {softlocks} soft locks in {:.0} s", p.chains_done, p.steps.len(), p.divergences.len(), p.findings.len(), p.now);
    info!("pilot: done the headless way: {}", if p.headless.is_empty() { "nothing".to_string() } else { p.headless.iter().map(|(op, n)| format!("{op} x{n}")).collect::<Vec<_>>().join(", ") });
    if let Some(out) = &p.out {
        match serde_json::to_string_pretty(&p.report(softlocks)).map_err(|e| e.to_string()).and_then(|t| std::fs::write(out, t).map_err(|e| e.to_string())) {
            Ok(()) => info!("pilot: report written to {}", out.display()),
            Err(e) => error!("pilot: report {}: {e}", out.display()),
        }
    }
    exit.write(AppExit::Success);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_formids_move_to_the_runs_load_order() {
        // FalloutNV 00, HonestHearts 01 in the chain; 00 and 02 in the run.
        let map = [Some(0), Some(2)];
        assert_eq!(remap_ids("0x0100F898", &map), "0x0200F898");
        assert_eq!(remap_ids("0100F89C.MoveTo 0100F898", &map), "0200F89C.MoveTo 0200F898");
        assert_eq!(remap_ids("0x00124121 and placed:VFSSimon", &map), "0x00124121 and placed:VFSSimon", "the base game's stay");
        assert_eq!(remap_ids("x0100F898 0100F8981 12.5", &map), "x0100F898 0100F8981 12.5", "only whole words");
        assert_eq!(remap_ids("0x0300F898", &map), "0x0300F898", "an index the chain does not name stays");
    }

    #[test]
    fn the_tail_point_is_that_far_before_the_end() {
        let pts = [[0.0, 0.0, 0.0], [100.0, 0.0, 10.0], [100.0, 50.0, 10.0]];
        assert_eq!(tail_point(&pts, 20.0), [100.0, 30.0, 10.0]);
        let p = tail_point(&pts, 100.0);
        assert!((p[0] - 50.0).abs() < 1e-3 && p[1] == 0.0 && (p[2] - 5.0).abs() < 1e-3, "{p:?}");
        assert_eq!(tail_point(&pts, 1000.0), [0.0, 0.0, 0.0], "a short path starts at its start");
    }

    #[test]
    fn numbers_compare_within_a_hair_and_the_rest_exactly() {
        assert!(same(&json!(10), &json!(10.0)));
        assert!(!same(&json!(10), &json!(20)));
        assert!(same(&json!(true), &json!(true)));
        assert!(!same(&json!("a"), &json!("b")));
    }
}
