//! What the host's story gives or takes from a co-op guest [design]: it belongs to that host's world, so it is undone when the
//! guest leaves and never written into the guest's own save.
//!
//! - **What counts.** While this game is a guest ([`Game::coop_guest`]), every change a
//!   script makes to the player's items (AddItem, RemoveItem, RemoveAllItems), perks
//!   (AddPerk, RemovePerk) and spells (AddSpell, RemoveSpell) is recorded here as a net
//!   change ([`CoopStory`]). So are the companion perks the host's companions give
//!   (nvf-play `coop_companions`). Dead Money's collar and jumpsuit, the gear
//!   it takes, Old World Blues' gown: all come from scripts [observed: DM SCPT
//!   0x0100C59E `player.RemoveAllItems`, OWB adds and equips the Patient Gown].
//! - **What does not.** What the player picks up, buys, loots or is paid, its XP, and
//!   the rewards shared when the host finishes a quest (`crate::coop_rewards`): none of
//!   that comes through a script function on the player [design: normal loot and rewards the guest earns stay]. The console's lines do not count either
//!   (the player's own), except under the test aid that makes them (`console_counts`).
//! - **Undoing it** ([`CoopStory::undo_on`]): the guest's own save is its game as it was
//!   when it joined with the character as it is now (`GameSnapshot::with_character_of`).
//!   Before that, the character has the story's changes taken back: items given are
//!   removed (up to what is still carried), items taken are given back with their
//!   condition and mods, perks go back to the rank they had, spells added are removed and
//!   spells removed come back. Nothing is undone in the running game: the guest is still
//!   in the host's world [design].
//! - Not kept in saves: a guest's game that loads starts with nothing recorded, and the
//!   save it loaded had none of it [design].

use crate::Game;
use fnv_sim::inventory::Extra;
use fnv_sim::FormId;
use std::collections::{BTreeMap, BTreeSet};

/// The host's story's net changes to the guest's player (see the module notes).
#[derive(Debug, Clone, Default)]
pub struct CoopStory {
    /// Items by base form and extra data (not equipped), net count added (taken: < 0).
    pub items: Vec<(FormId, Extra, i32)>,
    /// Perks changed, with the rank the player had before the first change.
    pub perks: BTreeMap<FormId, u8>,
    /// Spells added (true) or removed (false), net.
    pub spells: BTreeMap<FormId, bool>,
    /// Items the story put on the player (equipped, maybe locked on) that the guest
    /// carried before: only taken off and unlocked in the guest's save ([`CoopStory::story_gear`]).
    pub unequip: BTreeSet<FormId>,
    /// Test aid: the console's lines count as the story's.
    pub console_counts: bool,
}

/// The player's items now, by base form and extra data (not equipped).
pub fn player_groups(g: &Game) -> Vec<(FormId, Extra, i32)> {
    let mut out: Vec<(FormId, Extra, i32)> = Vec::new();
    if let Some(inv) = g.world.peek(FormId::PLAYER_REF).and_then(|r| r.inventory.as_ref()) {
        for (item, extra, n) in inv.groups() {
            add_group(&mut out, item, Extra { equipped: false, ..extra }, n);
        }
    }
    out
}

fn add_group(list: &mut Vec<(FormId, Extra, i32)>, item: FormId, extra: Extra, n: i32) {
    match list.iter_mut().find(|(i, e, _)| *i == item && *e == extra) {
        Some(g) => g.2 += n,
        None => list.push((item, extra, n)),
    }
    list.retain(|g| g.2 != 0);
}

impl CoopStory {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty() && self.perks.is_empty() && self.spells.is_empty() && self.unequip.is_empty()
    }

    /// Record the change from `before` to `after` (the player's items around a script call).
    pub fn record_items(&mut self, before: &[(FormId, Extra, i32)], after: &[(FormId, Extra, i32)]) {
        for &(item, extra, n) in after {
            let was = before.iter().find(|(i, e, _)| *i == item && *e == extra).map_or(0, |g| g.2);
            if n != was {
                add_group(&mut self.items, item, extra, n - was);
            }
        }
        for &(item, extra, was) in before {
            if !after.iter().any(|(i, e, _)| *i == item && *e == extra) {
                add_group(&mut self.items, item, extra, -was);
            }
        }
    }

    /// Story gear put on the guest without a script function (scene-stalls'
    /// `Game::follow_story_gear`, the host's DLC outfit): `added` when this game gave the
    /// item (one is taken back in the guest's save), else the guest had one already and
    /// it is only taken off and unlocked there.
    pub fn story_gear(&mut self, item: FormId, added: bool) {
        if added {
            add_group(&mut self.items, item, Extra::default(), 1);
        }
        self.unequip.insert(item);
    }

    /// Record a perk change: `before` is the rank the player had just before it.
    pub fn record_perk(&mut self, perk: FormId, before: u8) {
        self.perks.entry(perk).or_insert(before);
    }

    /// Record a spell added (`added`) or removed.
    pub fn record_spell(&mut self, spell: FormId, added: bool) {
        match self.spells.get(&spell) {
            Some(&was) if was != added => {
                self.spells.remove(&spell);
            }
            _ => {
                self.spells.insert(spell, added);
            }
        }
    }

    /// This record without what `keep` keeps (P5: the unique rewards of quests the guest
    /// claimed a stage of, `crate::coop_uniques`): items given and perks and spells added
    /// that are kept are left out, so [`CoopStory::undo_on`] leaves them on the character.
    /// What the story took is still given back.
    pub fn without(&self, keep: impl Fn(FormId) -> bool) -> CoopStory {
        let mut s = self.clone();
        s.items.retain(|&(item, _, n)| n <= 0 || !keep(item));
        s.unequip.retain(|item| !keep(*item));
        s.perks.retain(|&perk, _| !keep(perk));
        s.spells.retain(|&spell, &mut added| !added || !keep(spell));
        s
    }

    /// Take the story's changes back from the player of `s` (a snapshot of the game as
    /// it is now; see the module notes). What was undone, for the log.
    pub fn undo_on(&self, s: &mut crate::game::GameSnapshot) -> Vec<String> {
        let p = FormId::PLAYER_REF;
        let mut done = Vec::new();
        if let Some(r) = s.sim.refs.get_mut(&p) {
            for item in self.unequip.iter().chain(self.items.iter().filter(|g| g.2 > 0).map(|g| &g.0)) {
                r.equip_locks.remove(item);
                if let Some(inv) = r.inventory.as_mut() {
                    let n = inv.count(*item);
                    for (e, c) in inv.take(*item, n) {
                        inv.add_extra(*item, c, Extra { equipped: false, ..e });
                    }
                }
            }
            if !self.unequip.is_empty() {
                done.push(format!("{} story items taken off", self.unequip.len()));
            }
        }
        if let Some(inv) = s.sim.refs.get_mut(&p).and_then(|r| r.inventory.as_mut()) {
            for &(item, extra, n) in &self.items {
                if n > 0 {
                    // Given: take it back, those units first, then any of the item.
                    let mut left = n;
                    let carried = inv.count(item).min(n);
                    for (_, x) in inv.take(item, carried) {
                        left -= x;
                    }
                    if left < n {
                        done.push(format!("{} x {item} given by the story taken back", n - left));
                    }
                } else if n < 0 {
                    inv.add_extra(item, -n, extra);
                    done.push(format!("{} x {item} the story took given back", -n));
                }
            }
        }
        for (&perk, &before) in &self.perks {
            // A perk a script took and gave back (or gave at the rank it had) is as it was.
            if s.player.perks.get(&perk).copied().unwrap_or(0) == before {
                continue;
            }
            match before {
                0 => {
                    s.player.perks.remove(&perk);
                }
                r => {
                    s.player.perks.insert(perk, r);
                }
            }
            done.push(format!("perk {perk} back to rank {before}"));
        }
        if let Some(x) = s.extra.get_mut(&p) {
            for (&spell, &added) in &self.spells {
                if added {
                    x.spells.retain(|&q| q != spell);
                } else if !x.spells.contains(&spell) {
                    x.spells.push(spell);
                }
            }
        }
        for (&spell, &added) in &self.spells {
            if added {
                s.effects.retain(|e| !(e.target == p && e.source == spell && e.worn.is_none()));
            }
            done.push(format!("spell {spell} {}", if added { "the story added removed" } else { "the story removed given back" }));
        }
        done
    }
}

impl Game<'_> {
    /// Whether a script call in `ctx` on the player counts as the host's story (module notes).
    pub fn story_counts(&self, ctx: &crate::functions::Ctx) -> bool {
        // A guest's own purchase is its own (`Game::guest_service`).
        self.coop_guest && self.guest_service == 0 && (self.coop_story.console_counts || &*ctx.unit != "console")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_story_that_takes_and_gives_back_leaves_nothing() {
        let (gear, collar) = (FormId(0x10), FormId(0x20));
        let worn = Extra { condition: Some(0.5), ..Extra::default() };
        let mut s = CoopStory::default();
        // Dead Money takes the gear ...
        s.record_items(&[(gear, worn, 1)], &[]);
        // ... gives the collar ...
        s.record_items(&[], &[(collar, Extra::default(), 1)]);
        assert_eq!(s.items, vec![(gear, worn, -1), (collar, Extra::default(), 1)]);
        // ... and gives the gear back.
        s.record_items(&[(collar, Extra::default(), 1)], &[(collar, Extra::default(), 1), (gear, worn, 1)]);
        assert_eq!(s.items, vec![(collar, Extra::default(), 1)]);
        // A perk keeps the rank before its first change; a spell added then removed is gone.
        s.record_perk(FormId(0x30), 0);
        s.record_perk(FormId(0x30), 1);
        assert_eq!(s.perks[&FormId(0x30)], 0);
        s.record_spell(FormId(0x40), true);
        s.record_spell(FormId(0x40), false);
        assert!(s.spells.is_empty());
    }

    #[test]
    fn without_leaves_kept_gifts_out_but_still_gives_back_what_was_taken() {
        let (unique, other, taken, perk, spell) = (FormId(0x10), FormId(0x11), FormId(0x12), FormId(0x30), FormId(0x40));
        let mut s = CoopStory::default();
        s.record_items(&[(taken, Extra::default(), 1)], &[(unique, Extra::default(), 1), (other, Extra::default(), 2)]);
        s.record_perk(perk, 0);
        s.record_spell(spell, true);
        s.story_gear(unique, false);
        let keep = |f: FormId| f == unique || f == perk || f == spell || f == taken;
        let w = s.without(keep);
        // The kept unique, perk and spell are not undone; the other gift is, and the
        // item the story took comes back even though `keep` names it.
        assert_eq!(w.items, vec![(other, Extra::default(), 2), (taken, Extra::default(), -1)]);
        assert!(w.perks.is_empty() && w.spells.is_empty() && w.unequip.is_empty());
        // Keeping nothing is the record as it was.
        let all = s.without(|_| false);
        assert_eq!((all.items, all.perks, all.spells, all.unequip), (s.items.clone(), s.perks.clone(), s.spells.clone(), s.unequip.clone()));
    }
}
