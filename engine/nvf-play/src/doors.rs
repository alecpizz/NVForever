//! Doors that open in place (DOOR references without XTEL whose model has "Open" and
//! "Close" sequences): whether each is open and where its animation is. One clock for
//! everything that follows a door: the renderer poses the door's nodes from it
//! (nvforever), the physics moves the door's keyframed bodies with it ([`crate::phys`]),
//! and actors waiting to walk through check it (nvforever's travelling NPCs).
//! docs/design/navmesh.md §6.1, docs/spec-notes/doors.md.
//!
//! In `--play`, the simulation's open state (`fnv_sim` `RefState::open`, toggled by
//! activation and `SetOpenState`) drives the doors; a door opened here (by an actor) is
//! written back to it. Without the simulation, a door starts as its placement says (ONAM
//! "Open by Default"). A load puts each door at rest as the save has it, silently
//! ([`Doors::rest_as_restored`]); what changes after that moves it as any change does.
//!
//! Sounds: the sequences' `sound:` text keys name them; passing one sends a [`DoorSound`],
//! which nvforever plays at the door (`anim_sounds.rs`). A sequence without sound keys
//! asks for the DOOR record's open (SNAM) or close (ANAM) sound as it starts [design;
//! docs/spec-notes/audio.md §10].

use crate::sim::SimState;
use bevy::prelude::*;
use fnv_collide::anim::{sequence_end, AnimSequence, DoorMotion};
use fnv_sim::FormId;
use std::collections::HashMap;

/// The timing of a door model's two sequences.
#[derive(Debug, Clone, PartialEq)]
pub struct DoorTiming {
    /// Seconds from the start of "Open" (and "Close") to its `end` text key.
    pub open_end: f32,
    pub close_end: f32,
    /// Sounds named by the sequences' text keys: (seconds from the start, SOUN editor ID).
    pub open_sounds: Vec<(f32, String)>,
    pub close_sounds: Vec<(f32, String)>,
}

impl DoorTiming {
    /// From each sequence's start and stop times and text keys.
    pub fn new(open: (f32, f32, &[(f32, String)]), close: (f32, f32, &[(f32, String)])) -> DoorTiming {
        let sounds = |(start, _, keys): (f32, f32, &[(f32, String)])| {
            keys.iter().filter_map(|(t, k)| Some((t - start, AnimSequence::sound_of(k)?.to_string()))).collect()
        };
        DoorTiming {
            open_end: sequence_end(open.0, open.1, open.2),
            close_end: sequence_end(close.0, close.1, close.2),
            open_sounds: sounds(open),
            close_sounds: sounds(close),
        }
    }

    pub fn from_sequences(open: &AnimSequence, close: &AnimSequence) -> DoorTiming {
        DoorTiming::new((open.start, open.stop, &open.text_keys), (close.start, close.stop, &close.text_keys))
    }
}

/// One door.
#[derive(Debug, Clone, PartialEq)]
pub struct Door {
    pub motion: DoorMotion,
    pub timing: DoorTiming,
    /// The simulation's open flag when last read.
    sim_seen: Option<bool>,
    /// Opened or closed here (not by the simulation): to write to the simulation.
    to_sim: Option<bool>,
}

/// Every door seen so far, by FormID. Doors are kept when their cells unload, so a door
/// left open stays open.
#[derive(Resource, Default, Debug)]
pub struct Doors {
    map: HashMap<u32, Door>,
}

impl Doors {
    /// Know door `id`, `open` (or closed) at rest when first seen; later calls keep the
    /// state it has.
    pub fn register(&mut self, id: u32, open: bool, timing: DoorTiming) {
        self.map.entry(id).or_insert_with(|| Door {
            motion: DoorMotion::new(open, timing.open_end, timing.close_end),
            timing,
            sim_seen: None,
            to_sim: None,
        });
    }

    pub fn get(&self, id: u32) -> Option<&Door> {
        self.map.get(&id)
    }

    /// Open (`true`) or close door `id`. Returns whether its animation started (false for
    /// an unknown door or one already headed there).
    pub fn set_open(&mut self, id: u32, open: bool) -> bool {
        let Some(d) = self.map.get_mut(&id) else { return false };
        let started = d.motion.set_open(open);
        if started {
            d.to_sim = Some(open);
            info!("door 0x{id:08X} {}", if open { "opening" } else { "closing" });
        }
        started
    }

    /// Whether door `id` is fully open (its "Open" sequence has ended); `None` for a door
    /// not known here.
    pub fn is_open(&self, id: u32) -> Option<bool> {
        self.map.get(&id).map(|d| d.motion.is_open())
    }

    /// Every door known here (co-op shares the ones that move, nvforever `coop.rs`).
    pub fn ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.map.keys().copied()
    }

    /// Follow the simulation's open flags (activation, scripts) and write back the doors
    /// opened here.
    fn follow_sim(&mut self, sim: &mut SimState) {
        for (&id, d) in self.map.iter_mut() {
            if let Some(open) = d.to_sim.take() {
                if let Ok(s) = sim.world.get(FormId(id)) {
                    s.open = open;
                }
                d.sim_seen = Some(open);
                continue;
            }
            let open = sim_open(sim, id);
            match d.sim_seen {
                // First sight: the door is as the simulation says, at rest.
                None => d.motion = DoorMotion::new(open, d.timing.open_end, d.timing.close_end),
                Some(seen) if seen != open => {
                    d.motion.set_open(open);
                    info!("door 0x{id:08X} {}", if open { "opening" } else { "closing" });
                }
                _ => {}
            }
            d.sim_seen = Some(open);
        }
    }

    /// A save was just put into the simulation: every door known so far (they register
    /// from the plugins' state while the world streams in under the loading screen) stands
    /// at rest as the save has it, and what it saw or meant to write of the world before is
    /// forgotten. Called before the load's own scripts run, so a door one of them changes
    /// moves as any other change does.
    pub(crate) fn rest_as_restored(&mut self, sim: &SimState) {
        for (&id, d) in self.map.iter_mut() {
            let open = sim_open(sim, id);
            d.motion = DoorMotion::new(open, d.timing.open_end, d.timing.close_end);
            d.sim_seen = Some(open);
            d.to_sim = None;
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Advance every moving door by `dt` seconds; returns the sounds asked for.
    pub fn advance(&mut self, dt: f32) -> Vec<DoorSound> {
        let mut sounds = Vec::new();
        for (&id, d) in self.map.iter_mut() {
            let (opening, _) = d.motion.pose();
            let Some(before) = d.motion.update(dt) else { continue };
            let list = if opening { &d.timing.open_sounds } else { &d.timing.close_sounds };
            // A key at the very start is passed by the first step.
            let from = if before <= 0.0 { -1.0 } else { before };
            sounds.extend(list.iter().filter(|(t, _)| *t > from && *t <= d.motion.time).map(|(_, s)| DoorSound { door: id, opening, key: Some(s.clone()) }));
            if list.is_empty() && before <= 0.0 {
                sounds.push(DoorSound { door: id, opening, key: None });
            }
            if !matches!(d.motion.phase, fnv_collide::anim::DoorPhase::Opening | fnv_collide::anim::DoorPhase::Closing) {
                debug!("door 0x{id:08X} {:?}", d.motion.phase);
            }
        }
        sounds.sort();
        sounds
    }
}

/// A sound a moving door asks for: at the door, by nvforever.
#[derive(Message, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DoorSound {
    pub door: u32,
    /// Opening, else closing.
    pub opening: bool,
    /// The SOUN editor ID a sequence's sound key names; `None`: the sequence names none
    /// and the DOOR record's open or close sound is meant.
    pub key: Option<String>,
}

/// Runs before physics, so a door's bodies and nodes are posed from this frame's time.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DoorSystems;

/// Added by nvforever with or without `--play`.
pub struct DoorsPlugin;

impl Plugin for DoorsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Doors>()
            .add_message::<DoorSound>()
            .configure_sets(Update, DoorSystems.before(crate::PlaySet::Physics))
            .add_systems(Update, (sync_sim.run_if(resource_exists::<SimState>), advance, tell_scripts).chain().in_set(DoorSystems));
    }
}

fn sync_sim(mut doors: ResMut<Doors>, mut sim: ResMut<SimState>) {
    doors.follow_sim(&mut sim);
}

/// Tell the scripts the open state of every door and animated container known here
/// (GetOpenState: 1 open, 2 opening, 3 closed, 4 closing).
fn tell_scripts(doors: Res<Doors>, script: Option<ResMut<crate::script::ScriptState>>) {
    use fnv_collide::anim::DoorPhase;
    let Some(mut script) = script else { return };
    let code = |d: &Door| match d.motion.phase {
        DoorPhase::Open => 1,
        DoorPhase::Opening => 2,
        DoorPhase::Closed => 3,
        DoorPhase::Closing => 4,
    };
    let Some(states) = script.rt.as_ref().map(|rt| &rt.game.senses.open_states) else { return };
    if states.len() == doors.map.len() && doors.map.iter().all(|(&id, d)| states.get(&FormId(id)) == Some(&code(d))) {
        return;
    }
    if let Some(rt) = script.rt.as_mut() {
        rt.game.senses.open_states = doors.map.iter().map(|(&id, d)| (FormId(id), code(d))).collect();
    }
}

fn advance(time: Res<Time>, mut doors: ResMut<Doors>, mut out: MessageWriter<DoorSound>) {
    for s in doors.advance(time.delta_secs()) {
        debug!("door 0x{:08X} sound {:?}", s.door, s.key);
        out.write(s);
    }
}

/// The simulation's open flag of door `id`. The simulation keeps a reference's state only
/// once something has touched it; until then the door is as placed (ONAM "Open by
/// Default", fnv-sim's starting state).
fn sim_open(sim: &SimState, id: u32) -> bool {
    match sim.world.peek(FormId(id)) {
        Some(s) => s.open,
        None => sim.refs.get(FormId(id)).is_some_and(|p| p.open_by_default),
    }
}

/// The open state a door should start in when first seen: the simulation's, else its
/// placement's ONAM "Open by Default".
pub fn initial_open(sim: Option<&SimState>, id: u32, open_by_default: bool) -> bool {
    sim.map_or(open_by_default, |s| sim_open(s, id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fnv_collide::anim::DoorPhase;

    fn timing() -> DoorTiming {
        let open = vec![(0.0, "start".to_string()), (0.00001, "sound: DRSOpen".to_string()), (1.5, "end".to_string())];
        let close = vec![(0.0, "start".to_string()), (0.9, "Sound: DRSClose node".to_string()), (1.0, "end".to_string())];
        DoorTiming::new((0.0, 1.5, &open), (0.0, 1.0, &close))
    }

    #[test]
    fn doors_open_close_and_pass_their_sounds() {
        let mut doors = Doors::default();
        doors.register(7, false, timing());
        doors.register(7, true, timing()); // kept as first seen
        assert_eq!(doors.is_open(7), Some(false));
        assert_eq!(doors.is_open(8), None);
        assert!(doors.set_open(7, true));
        assert!(!doors.set_open(7, true));
        let key = |opening, k: &str| DoorSound { door: 7, opening, key: Some(k.to_string()) };
        assert_eq!(doors.advance(0.5), vec![key(true, "DRSOpen")]);
        assert_eq!(doors.is_open(7), Some(false));
        assert!(doors.advance(1.0).is_empty());
        assert_eq!(doors.is_open(7), Some(true));
        doors.set_open(7, false);
        assert!(doors.advance(0.5).is_empty());
        assert_eq!(doors.advance(0.5), vec![key(false, "DRSClose")]);
        assert_eq!(doors.get(7).unwrap().motion.phase, fnv_collide::anim::DoorPhase::Closed);
        // A door whose sequences name no sound asks for its record's as it starts.
        let silent = DoorTiming::new((0.0, 1.0, &[]), (0.0, 1.0, &[]));
        doors.register(9, false, silent);
        doors.set_open(9, true);
        assert_eq!(doors.advance(0.1), vec![DoorSound { door: 9, opening: true, key: None }]);
        assert!(doors.advance(0.1).is_empty());
    }

    /// Door references (id, placed open) over one DOOR base.
    fn door_sim(placements: &[(u32, bool)]) -> SimState {
        use crate::sim::test_world::{door_base, placed, sim};
        const DOOR: u32 = 0x130;
        let refs = placements.iter().map(|&(id, open_by_default)| fnv_sim::refs::PlacedRef { open_by_default, ..placed(id, DOOR) }).collect();
        sim(vec![door_base(DOOR)], refs)
    }

    fn phase(doors: &Doors, id: u32) -> DoorPhase {
        doors.get(id).unwrap().motion.phase
    }

    #[test]
    fn a_door_saved_open_rests_open_and_a_real_close_after_the_load_still_moves_it() {
        let mut sim = door_sim(&[(7, false)]);
        let mut doors = Doors::default();
        // Registered and seen from the plugins' state, closed, under the loading screen.
        doors.register(7, false, timing());
        doors.follow_sim(&mut sim);
        // The restore: the saved flag is in the simulation.
        sim.world.get(FormId(7)).unwrap().open = true;
        doors.rest_as_restored(&sim);
        assert_eq!(phase(&doors, 7), DoorPhase::Open);
        doors.follow_sim(&mut sim);
        assert_eq!(phase(&doors, 7), DoorPhase::Open, "no animation of the restored flag");
        assert!(doors.advance(2.0).is_empty(), "no sound of it");
        // A real change right after the load: the same frames as any close.
        sim.world.get(FormId(7)).unwrap().open = false;
        doors.follow_sim(&mut sim);
        assert_eq!(phase(&doors, 7), DoorPhase::Closing);
        assert_eq!(doors.advance(1.0), vec![DoorSound { door: 7, opening: false, key: Some("DRSClose".to_string()) }]);
        assert_eq!(phase(&doors, 7), DoorPhase::Closed);
    }

    #[test]
    fn a_door_saved_closed_rests_closed_and_a_write_back_from_before_the_load_is_dropped() {
        // 7 placed open and saved closed; 8 placed closed, opened by an actor in the frames
        // before the restore (its write to the simulation still waiting), not open in the save.
        let mut sim = door_sim(&[(7, true), (8, false)]);
        let mut doors = Doors::default();
        doors.register(7, true, timing());
        doors.register(8, false, timing());
        doors.follow_sim(&mut sim);
        assert!(doors.set_open(8, true));
        sim.world.get(FormId(7)).unwrap().open = false;
        doors.rest_as_restored(&sim);
        doors.follow_sim(&mut sim);
        assert!(!sim_open(&sim, 8), "the old write does not reach the restored world");
        assert!(doors.advance(2.0).is_empty());
        assert_eq!((phase(&doors, 7), phase(&doors, 8)), (DoorPhase::Closed, DoorPhase::Closed));
        // A door opened here after the load is written back and heard as ever.
        assert!(doors.set_open(8, true));
        doors.follow_sim(&mut sim);
        assert!(sim_open(&sim, 8));
        assert_eq!(doors.advance(0.5), vec![DoorSound { door: 8, opening: true, key: Some("DRSOpen".to_string()) }]);
    }
}
