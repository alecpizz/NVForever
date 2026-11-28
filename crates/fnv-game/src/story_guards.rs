//! Story guards: NVForever's own fallbacks for scripted scenes that were seen to stall,
//! whose cause is not known yet. They
//! are not in the original game [design].
//!
//! A guard watches a stretch of a quest: from a stage that starts the scene to the stage
//! that ends it. When the scene does not reach its end in time, the guard first asks the
//! scene's actors to choose their packages again (EvaluatePackage), and later, if that
//! did not help, sets the end stage itself, which is what the player would do from the
//! console. Each step logs a warning line, so a stall that the guard covered still shows
//! in the log.
//!
//! - **The clock.** Game time ([`Game::time`]): it stands still in menus, conversations
//!   and loading screens. It also stands still while anyone says a line (SayTo, Say), so
//!   that a scene that is slow but going on is never cut short [design].
//! - **Where it runs.** Only in a co-op game ([`Game::coop_game`]): single player behaves
//!   as the game does, stalls included [design: co-op only, maintainer review of #3723].
//!   And only where the world's logic runs: not on a co-op guest, which follows its
//!   host's story [design].
//! - Not saved: a game loaded in the middle of a guarded stretch starts its clock again
//!   at the load [design].
//!
//! The guards:
//! - **Honest Hearts' arrival** [design]. After the intro slideshow,
//!   NVDLC02MQ00 stage 100 moves the player into Zion; the arrival box's script then
//!   lets Jed's force greet run (box variable iJedConvo 1 after 2 s), and his lines set
//!   NVDLC02MQ01 stage 5, whose result script gives the controls back and moves the
//!   scene on [observed: HonestHearts.esm SCPT 0x01010006, QUST 0x01008893 stage 5;
//!   the greet came about 2.5 s after stage 100 in every test run]. In a co-op game Jed never greeted and the controls stayed locked;
//!   `SetStage NVDLC02MQ01 5` got the player out [design: a found bug]. The guard asks Jed to choose again after 8 s,
//!   and sets stage 5 after 30 s. Setting it skips Jed's greeting lines: stage 5 moves
//!   his conversation state past them.

use crate::Game;

/// One guarded stretch of a quest (see the module notes).
#[derive(Debug, Clone, Copy)]
pub struct Guard {
    /// What the log calls it.
    pub name: &'static str,
    /// The quest and stage that start the stretch (Editor IDs).
    pub from: (&'static str, i32),
    /// The quest and stage that end it: the guard's last step sets it.
    pub to: (&'static str, i32),
    /// Clock seconds before the actors choose their packages again.
    pub nudge_after: f64,
    /// The actors (placed references' Editor IDs).
    pub nudge: &'static [&'static str],
    /// Clock seconds before the guard sets the end stage.
    pub give_up_after: f64,
}

/// Every guard.
pub const GUARDS: &[Guard] = &[Guard {
    name: "Honest Hearts' arrival (Jed's greeting)",
    from: ("NVDLC02MQ00", 100),
    to: ("NVDLC02MQ01", 5),
    nudge_after: 8.0,
    nudge: &["NVDLC02JedREF"],
    give_up_after: 30.0,
}];

/// Where a guard stands.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum Step {
    /// The stretch has not started.
    #[default]
    Waiting,
    /// Under way: the clock's seconds, and whether the actors were nudged.
    Running { seconds: f64, nudged: bool },
    /// Over: ended on its own or by the guard.
    Over,
}

/// The guards' steps, by index into [`GUARDS`]. Not saved.
#[derive(Debug, Clone, Default)]
pub struct StoryGuards {
    pub steps: Vec<Step>,
}

impl Game<'_> {
    fn stage_done_by_edid(&self, quest: &str, stage: i32) -> Option<bool> {
        let q = self.data.by_edid(quest)?;
        Some(self.quests.get(&q).is_some_and(|s| s.stages_done.contains(&stage)))
    }

    /// One frame of the story guards (from [`Game::tick`], outside menus, where the
    /// world's logic runs).
    pub(crate) fn story_guards_tick(&mut self, dt: f64) {
        if self.story_guards.steps.len() != GUARDS.len() {
            self.story_guards.steps = vec![Step::Waiting; GUARDS.len()];
        }
        let speaking = !self.speech.is_empty();
        for (i, guard) in GUARDS.iter().enumerate() {
            let step = self.story_guards.steps[i];
            if step == Step::Over {
                continue;
            }
            // A load order without the guard's quests: nothing to watch.
            let (Some(started), Some(ended)) = (self.stage_done_by_edid(guard.from.0, guard.from.1), self.stage_done_by_edid(guard.to.0, guard.to.1)) else {
                self.story_guards.steps[i] = Step::Over;
                continue;
            };
            if ended {
                if let Step::Running { seconds, .. } = step {
                    tracing::info!("story guard: {}: ended on its own after {seconds:.1} s", guard.name);
                }
                self.story_guards.steps[i] = Step::Over;
                continue;
            }
            if !started {
                continue;
            }
            let (mut seconds, mut nudged) = match step {
                Step::Running { seconds, nudged } => (seconds, nudged),
                _ => (0.0, false),
            };
            if !speaking {
                seconds += dt;
            }
            if !nudged && seconds >= guard.nudge_after {
                nudged = true;
                for edid in guard.nudge {
                    if let Some(actor) = self.data.by_edid(edid) {
                        let package = self.evaluate_package(actor);
                        tracing::warn!(
                            "story guard: {}: {} stage {} not set {seconds:.1} s after {} stage {}: {edid} chooses its package again ({})",
                            guard.name,
                            guard.to.0,
                            guard.to.1,
                            guard.from.0,
                            guard.from.1,
                            package.map_or_else(|| "none".to_string(), |p| p.to_string())
                        );
                    }
                }
            }
            if seconds >= guard.give_up_after {
                tracing::warn!(
                    "story guard: {}: {} stage {} not set {seconds:.1} s after {} stage {}: the guard sets it",
                    guard.name,
                    guard.to.0,
                    guard.to.1,
                    guard.from.0,
                    guard.from.1
                );
                self.story_guards.steps[i] = Step::Over;
                if let Some(q) = self.data.by_edid(guard.to.0) {
                    self.set_stage(q, guard.to.1);
                }
                continue;
            }
            self.story_guards.steps[i] = Step::Running { seconds, nudged };
        }
    }
}

