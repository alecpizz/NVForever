//! Physics cells prepared ahead of a door's travel (seamless doors, step 1:
//! docs/design/seamless-doors.md §2).
//!
//! When the player comes near a load door, the cells its travel would load (the
//! interior, or the squares around the arrival outdoors) are prepared here on a thread of
//! their own, as the loader prepares the cells around the player ([`super::loader`]), and
//! kept until the travel takes them ([`Warm::take`]) or they go stale. The travel then
//! inserts prepared cells (about a millisecond each) instead of reading and building
//! them on the main thread in its frame (3–60 ms each, nine outdoors) [design].
//!
//! A thread of its own, not the loader's: the loader hands cells back in request order
//! and the main thread inserts only the front of its queue, so a door's cells queued
//! there would hold up the cells around the player [design].
//!
//! Staleness: a cell is prepared with the simulation's poses and scales of the moment
//! (`with_poses`). A reference of a warm cell named by `RefChanged` drops the cell
//! ([`Warm::forget_ref`]); a cell not taken within [`KEEP_SECS`] is dropped too
//! [engineering choice: the cells around a door the player stands by are prepared again
//! after that].

use super::cells::PhysCell;
use super::loader::Work;
use super::prepare::Prepared;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::Mutex;

/// Real seconds a prepared cell is kept for a travel that has not come [engineering
/// choice: long enough to stand at a door and read a note, short enough that what changed
/// meanwhile is built again].
pub const KEEP_SECS: f64 = 60.0;

/// The worldspace a cell belongs to as a key: an interior is the same in any (0).
pub type Key = (u32, PhysCell);

pub fn key(worldspace: u32, cell: PhysCell) -> Key {
    match cell {
        PhysCell::Interior(_) => (0, cell),
        PhysCell::Exterior(..) => (worldspace, cell),
    }
}

struct Ready {
    prepared: Prepared,
    /// The job's references (to drop the cell when one changes).
    refs: HashSet<u32>,
    /// Real time it was asked for last.
    touched: f64,
}

struct Worker {
    tx: Sender<(Key, Work)>,
    rx: Receiver<(Key, Prepared)>,
}

/// Cells prepared ahead (module notes).
#[derive(Default)]
pub struct Warm {
    worker: Option<Mutex<Worker>>,
    /// Asked for and not back yet, with their references and when they were asked for.
    pending: HashMap<Key, (HashSet<u32>, f64)>,
    ready: HashMap<Key, Ready>,
    /// Cells taken by a travel, and dropped stale or changed (for the exit line).
    pub taken: u64,
    pub dropped: u64,
}

impl Warm {
    /// Whether `k` is prepared or being prepared; marks it wanted at `now`.
    pub fn has(&mut self, k: Key, now: f64) -> bool {
        if let Some(r) = self.ready.get_mut(&k) {
            r.touched = now;
            return true;
        }
        if let Some(p) = self.pending.get_mut(&k) {
            p.1 = now;
            return true;
        }
        false
    }

    /// Whether `k` is prepared and waiting to be taken.
    pub fn ready(&self, k: Key) -> bool {
        self.ready.contains_key(&k)
    }

    /// Prepare `k` with `work` on the warm thread. `refs`: the job's references.
    pub fn ask(&mut self, k: Key, refs: HashSet<u32>, work: Work, now: f64) {
        let worker = self.worker.get_or_insert_with(|| Mutex::new(spawn()));
        let worker = worker.get_mut().unwrap_or_else(std::sync::PoisonError::into_inner);
        if worker.tx.send((k, work)).is_ok() {
            self.pending.insert(k, (refs, now));
        }
    }

    /// Take in what the thread finished; drop what has not been wanted for [`KEEP_SECS`].
    pub fn poll(&mut self, now: f64) {
        if let Some(w) = self.worker.as_mut() {
            let w = w.get_mut().unwrap_or_else(std::sync::PoisonError::into_inner);
            loop {
                match w.rx.try_recv() {
                    Ok((k, prepared)) => {
                        // Forgotten meanwhile (a reference of it changed): not kept.
                        if let Some((refs, touched)) = self.pending.remove(&k) {
                            self.ready.insert(k, Ready { prepared, refs, touched });
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        // The thread panicked: everything loads as before from now on.
                        self.worker = None;
                        self.pending.clear();
                        break;
                    }
                }
            }
        }
        let before = self.ready.len();
        self.ready.retain(|_, r| now - r.touched < KEEP_SECS);
        self.dropped += (before - self.ready.len()) as u64;
    }

    /// The prepared cell `k`, if it is ready; it is no longer kept.
    pub fn take(&mut self, k: Key) -> Option<Prepared> {
        let r = self.ready.remove(&k)?;
        self.taken += 1;
        Some(r.prepared)
    }

    /// A reference changed: a cell prepared with it is no longer what loading it would
    /// build. Dropped (a pending one is not kept when it comes back).
    pub fn forget_ref(&mut self, id: u32) {
        let before = self.ready.len();
        self.ready.retain(|_, r| !r.refs.contains(&id));
        self.dropped += (before - self.ready.len()) as u64;
        self.pending.retain(|_, (refs, _)| !refs.contains(&id));
    }

    /// Forget everything (a change of worldspace: its exterior cells are another's).
    pub fn clear(&mut self) {
        self.dropped += self.ready.len() as u64;
        self.ready.clear();
        self.pending.clear();
    }

    /// Cells prepared and waiting, and cells being prepared.
    pub fn counts(&self) -> (usize, usize) {
        (self.ready.len(), self.pending.len())
    }
}

fn spawn() -> Worker {
    let (tx, work_rx) = channel::<(Key, Work)>();
    let (done_tx, rx) = channel::<(Key, Prepared)>();
    let spawned = std::thread::Builder::new().name("phys-warm".into()).spawn(move || {
        while let Ok((k, work)) = work_rx.recv() {
            if done_tx.send((k, work())).is_err() {
                break;
            }
        }
    });
    if let Err(e) = spawned {
        bevy::log::warn!("physics: the door read-ahead thread did not start ({e}); door destinations load as before");
    }
    Worker { tx, rx }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phys::LoadTimes;

    fn prepared(cell: PhysCell) -> Prepared {
        Prepared { cell, heights: None, terrain: None, refs: Vec::new(), times: LoadTimes::default() }
    }

    fn wait_ready(w: &mut Warm, k: Key) {
        for _ in 0..500 {
            w.poll(0.0);
            if w.ready(k) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        panic!("not prepared");
    }

    #[test]
    fn a_cell_asked_for_is_prepared_and_taken_once() {
        let mut w = Warm::default();
        let k = key(7, PhysCell::Interior(0x10));
        assert_eq!(k.0, 0, "an interior is the same in any worldspace");
        w.ask(k, HashSet::from([1, 2]), Box::new(|| prepared(PhysCell::Interior(0x10))), 0.0);
        assert!(w.has(k, 0.0));
        wait_ready(&mut w, k);
        assert!(w.take(k).is_some());
        assert!(w.take(k).is_none());
        assert_eq!(w.taken, 1);
    }

    #[test]
    fn a_changed_reference_drops_its_cell() {
        let mut w = Warm::default();
        let k = key(7, PhysCell::Exterior(-18, 0));
        w.ask(k, HashSet::from([0x55]), Box::new(|| prepared(PhysCell::Exterior(-18, 0))), 0.0);
        wait_ready(&mut w, k);
        w.forget_ref(0x66);
        assert!(w.ready(k));
        w.forget_ref(0x55);
        assert!(!w.ready(k));
    }

    #[test]
    fn a_cell_not_wanted_goes_stale() {
        let mut w = Warm::default();
        let k = key(7, PhysCell::Exterior(1, 1));
        w.ask(k, HashSet::new(), Box::new(|| prepared(PhysCell::Exterior(1, 1))), 0.0);
        wait_ready(&mut w, k);
        w.poll(KEEP_SECS + 1.0);
        assert!(!w.ready(k));
        assert_eq!(w.dropped, 1);
    }
}
