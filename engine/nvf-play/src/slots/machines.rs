//! The slot machine in use now, by placed machine: the open menu's
//! [`super::Session::machine`]. nvforever `slots_view.rs` turns the reels on the machine
//! in the world, so a passer-by sees the same spin and result as the player at it.

use super::{reel_positions, Phase, SlotsScreen};
use bevy::prelude::*;
use fnv_game::slots::{self as rules, REELS};
use fnv_sim::FormId;
use std::collections::HashMap;

/// A machine in use and where its reels are.
#[derive(Debug, Clone, PartialEq)]
pub struct MachineSpin {
    pub casino: FormId,
    /// The symbols' pictures (CSNO ICON), under `textures\`, symbol 1 to W.
    pub symbols: Vec<String>,
    /// The reel's stops: the symbol (0 to 6) at each.
    pub reel: Vec<u8>,
    /// Where the reels stand (or will stop).
    pub stops: [usize; REELS],
    /// The spin turning: when it started (aid seconds) and the stops it left.
    pub spinning: Option<(f32, [usize; REELS])>,
    /// Spins made so far (a new one starts the reels).
    pub spin: u32,
    /// What the last spin's line paid, times the bet, once its reels stopped.
    pub paid: Option<u32>,
}

impl MachineSpin {
    /// Where each reel stands at `now`, in stops.
    pub fn positions(&self, now: f32) -> [f32; REELS] {
        match self.spinning {
            Some((since, from)) => reel_positions(self.reel.len(), from, self.stops, now - since),
            None => self.stops.map(|s| s as f32),
        }
    }

    /// The symbols on the line once stopped.
    pub fn line(&self) -> [u8; REELS] {
        let n = self.reel.len().max(1);
        self.stops.map(|s| self.reel.get(s % n).copied().unwrap_or(0))
    }
}

/// The machines in use, by placed reference.
#[derive(Resource, Default, Debug)]
pub struct SlotMachines(pub HashMap<FormId, MachineSpin>);

/// Keep [`SlotMachines`]: this player's machine from the open menu.
pub fn track(screen: Res<SlotsScreen>, mut machines: ResMut<SlotMachines>) {
    let mut keep: Vec<FormId> = Vec::new();
    if let Some((m, s)) = screen.0.as_ref().and_then(|s| s.machine.map(|m| (m, s))) {
        keep.push(m);
        let spinning = match s.phase {
            Phase::Spinning { since, from, .. } => Some((since, from)),
            _ => None,
        };
        let paid = matches!(s.phase, Phase::Paid { .. }).then(|| rules::multiple(s.line()));
        let entry = MachineSpin { casino: s.casino, symbols: s.symbols.clone(), reel: s.reel.clone(), stops: s.stops, spinning, spin: s.spins, paid };
        if machines.0.get(&m) != Some(&entry) {
            machines.0.insert(m, entry);
        }
    }
    if machines.0.len() != keep.len() {
        machines.0.retain(|m, _| keep.contains(m));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spin_turns_then_stands() {
        let reel: Vec<u8> = (0..14).map(|i| (i % 7) as u8).collect();
        let mut m = MachineSpin { casino: FormId(1), symbols: vec![], reel, stops: [3, 4, 5], spinning: Some((10.0, [0, 0, 0])), spin: 1, paid: None };
        assert_eq!(m.positions(10.0), [0.0, 0.0, 0.0]);
        let end = m.positions(20.0);
        assert_eq!(end.map(|p| p.round() as usize % 14), [3, 4, 5]);
        m.spinning = None;
        assert_eq!(m.positions(20.0), [3.0, 4.0, 5.0]);
        assert_eq!(m.line(), [3, 4, 5]);
    }
}
