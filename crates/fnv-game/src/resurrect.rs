//! ResurrectActor: a dead actor alive again (docs/spec-notes/resurrect.md).
//!
//! - **The rules** [observed: function probes fp5, 2026-10-17: after `KillActor player 0
//!   1` then `ResurrectActor 0` on Ringo, GetDead 0, GetCauseofDeath −1,
//!   GetHealthPercentage 1.0]: not dead or unconscious, its killer and cause of death
//!   forgotten, unharmed (every actor value's damage gone, the limbs' too [hypothesis:
//!   the owner's "generated again from its record", below]). With Anim 0 its severed and
//!   exploded limbs come back ([`RefExtra::limbs_gone`](crate::game::RefExtra::limbs_gone)
//!   cleared, so the engine draws it whole); with 1 they stay gone [documented: "body data
//!   will not be updated"].
//! - **Generated again** [observed: owner, 2026-10-17: "resurrect rerolls any level list
//!   or random gen on the actor record so if it comes from a template with an item list
//!   or its a generic that can be male female all this will be re rolled on the
//!   resurrected actor"]: a dead actor's random choices are rolled again
//!   ([`RefExtra::rolls`](crate::game::RefExtra::rolls), the seed
//!   `fnv_sim::leveled::roll_seed`): the entry a leveled actor list stands for (which can
//!   be a man or a woman), and, when its carried items come through such a pick, the
//!   template they come from and the items themselves (`fnv_sim::World::reset_inventory`).
//!   An actor with its own item list keeps what it carries, looted or given [observed:
//!   capture rs1, 2026-10-17: Ringo given three Stimpaks, two taken from his corpse, one
//!   left after ResurrectActor 0], and the items of that list are made up again: the
//!   list is rolled anew and what the actor has fewer of is added
//!   (`fnv_sim::World::restock_inventory`, issue #2683) [hypothesis: the owner's "rerolls
//!   any level list" read for an actor's own list; rs1 gave Ringo none of his own items
//!   to lose, so it does not say. Design: made up, not replaced, so the given Stimpak of
//!   rs1 stays]. The faction hit squads are four placed references the shipped script
//!   resurrects for every visit (VFactionSquadListSCRIPT: `rActor01.Resurrect`), each
//!   with its weapons in leveled lists of its own item list: without this the second
//!   squad came with fists, its guns left beside the first one's bodies [observed: owner
//!   playtest 2026-11-15]. What scripts gave it otherwise stays (factions,
//!   packages, actor values set, essential, AI off, dispositions) [hypothesis: the quests
//!   that resurrect companions and Yes Man rely on it].
//! - **Where**: where its body lies [observed: rs1: Ringo's reference followed his
//!   ragdoll from (685, -5602) to (730, -5609) and he stood up there, at once and still
//!   after 180 frames], which is where it died or where a script moved the corpse
//!   [observed: the shipped scripts move the corpse first, e.g. Yes Man's `MoveTo` to a
//!   marker and `ResurrectActor` after it, VMQYesManSupport, and the robot squads'
//!   `bot01.MoveTo` then `bot01.ResurrectActor` a frame later, decompiled 2026-10-17].
//!   The engine tells where the body came to rest ([`Game::stand_where_the_body_lies`]).
//!   It stands there in its default standing pose [observed: owner, 2026-10-17: "they
//!   respawn in their default standing position"; rs1: standing in the first frame after
//!   ResurrectActor 0, GetKnockedState 0 throughout].
//! - **The animate flag** (0 just standing, 1 standing up [documented: GECK wiki
//!   ResurrectActor]) is kept with the resurrection for the engine; both stand at once
//!   [design: the owner, 2026-10-17, "we shouldnt need a getup animation for
//!   resurrect"]. (With 1 the original showed a torn body in the air for a second or two,
//!   GetKnockedState 0, then the actor walking on [observed: rs1 screenshots]; not
//!   copied.)
//! - **A living actor** (scripts call it from OnHit when Health is below 0, decompiled)
//!   is only healed: nothing is rolled again [hypothesis: generating a living actor again
//!   would strip a companion of its gear].
//! - **Not loaded**: the rules are the same; it stays where it was and is drawn alive
//!   when its cell is [observed: rs1: Chet killed and raised in the unloaded general
//!   store, GetDead 0, position unchanged, standing when the player arrived].
//! - It leaves combat (`Game::stop_combat`) [hypothesis: a new actor starts calm].

use crate::functions::base_of;
use crate::game::Game;
use fnv_sim::leveled::roll_seed;
use fnv_sim::FormId;
use serde::Serialize;

/// One ResurrectActor for the engine (`Game::take_resurrections`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Resurrection {
    pub actor: FormId,
    /// The animate flag (1: stand up; module notes).
    pub animate: bool,
    /// It was dead and has been generated again (its look may have changed): the engine
    /// draws it anew.
    pub regenerated: bool,
    /// Its rolls after this one (`RefExtra::rolls`).
    pub rolls: u32,
    /// How many items of its own list were made up again (module notes, #2683).
    pub restocked: u32,
}

impl Game<'_> {
    /// `actor.ResurrectActor animate` (module notes).
    pub fn resurrect(&mut self, r: FormId, animate: bool) {
        let regenerated = self.is_dead(r);
        let e = self.extra.entry(r).or_default();
        e.dead = false;
        e.unconscious = false;
        e.knocked_out = None;
        e.killer = None;
        e.death_cause = None;
        e.av_damage.clear();
        e.last_hit = None;
        e.killing_hit = None;
        e.player_damage = 0.0;
        e.total_damage = 0.0;
        let was_culled = crate::functions::critical_stage_culls(e.critical_stage);
        e.critical_stage = 0;
        e.furniture = None;
        e.weapon_out = false;
        // With 1 "body data will not be updated" [documented: GECK wiki ResurrectActor]: a
        // severed limb stays gone; with 0 the actor comes back whole [hypothesis: read
        // from that note].
        if !animate {
            e.limbs_gone.clear();
        }
        if regenerated {
            e.rolls += 1;
        }
        let rolls = e.rolls;
        if was_culled {
            self.script_looks.critical_changed.push(r);
        }
        self.remove_ash_pile(r);
        let mut restocked = 0;
        if regenerated {
            restocked = self.roll_again(r, rolls);
            self.stop_combat(r);
        }
        self.resurrections.push(Resurrection { actor: r, animate, regenerated, rolls, restocked });
    }

    /// Roll the carried items of `r` again when they come through a leveled pick (its
    /// base or a template in its Inventory chain is a leveled actor list); an actor with
    /// its own item list keeps what it carries and has the items of that list made up
    /// again (module notes): how many were added.
    fn roll_again(&mut self, r: FormId, rolls: u32) -> u32 {
        let Some(base) = base_of(self, r) else { return 0 };
        if !self.inventory_chain_has_list_edit(base) && self.data.template_sources_any_pick(base, |d| d.template_inventory).len() < 2 {
            let added = self.world.restock_inventory(r).unwrap_or_default();
            if !added.is_empty() {
                let said = |items: &[(FormId, i32)]| items.iter().map(|(i, n)| format!("{n} {}", self.data.name(*i))).collect::<Vec<_>>().join(", ");
                let all: Vec<(FormId, i32)> = self.world.peek(r).and_then(|s| s.inventory.as_ref()).map(|inv| inv.iter().filter(|(_, n)| *n > 0).collect()).unwrap_or_default();
                tracing::info!("resurrect: {r} has its own items made up again: {}; it carries {}", said(&added), said(&all));
            }
            return added.iter().map(|(_, n)| *n as u32).sum();
        }
        let source = self.runtime_template_source(base, roll_seed(r, rolls), self.leveled_at(r), |d| d.template_inventory).map(|(s, _)| s).filter(|&s| s != base);
        self.world.set_inventory_source(r, source);
        // A source with no carried items: it carries nothing (but a planted explosive's
        // live unit, `Extra::live`, #1640).
        if !self.world.reset_inventory(r).unwrap_or(true) {
            let _ = self.world.clear_inventory_but_live(r);
        } else {
            self.clear_npc_weapon_mods_roll(r);
            self.maybe_apply_npc_weapon_mods(r);
        }
        0
    }

    /// Only an edited actor list reachable through this actor's Inventory chain can
    /// change its regeneration policy. An unrelated list must not discard carried gear.
    fn inventory_chain_has_list_edit(&self, base: FormId) -> bool {
        if self.forms.actor_list_additions.is_empty() && self.other.lev_actors.is_empty() { return false; }
        fn visit(g: &Game, id: FormId, depth: usize, seen: &mut std::collections::HashSet<FormId>) -> bool {
            if depth > 32 || !seen.insert(id) { return false; }
            if let Some(actor) = g.data.actors.get(&id) {
                return actor.template.filter(|_| actor.template_inventory).is_some_and(|next| visit(g, next, depth + 1, seen));
            }
            let Some(list) = g.data.leveled_actors.get(&id) else { return false };
            g.forms.actor_list_additions.contains_key(&id) || g.other.lev_actors.contains_key(&id)
                || list.entries.iter().any(|(_, entry)| visit(g, *entry, depth + 1, seen))
        }
        visit(self, base, 0, &mut std::collections::HashSet::new())
    }

    /// The engine found the body of resurrected `r` lying at `pos` (game units): the
    /// reference is put there and the actor stands up from there (module notes), its
    /// walker started again (`Game::teleported`).
    pub fn stand_where_the_body_lies(&mut self, r: FormId, pos: [f32; 3]) {
        if let Ok(s) = self.world.get(r) {
            s.pos = pos;
        }
        self.teleported.insert(r);
    }

    /// How many times ResurrectActor rolled the actor's random choices again.
    pub fn rolls(&self, r: FormId) -> u32 {
        self.extra.get(&r).map_or(0, |e| e.rolls)
    }

    /// The resurrections since the last call (the engine stands the actors up).
    pub fn take_resurrections(&mut self) -> Vec<Resurrection> {
        std::mem::take(&mut self.resurrections)
    }
}
