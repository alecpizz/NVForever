//! Pickpocketing in play (docs/spec-notes/pickpocket.md; the rules are
//! `fnv_game::pickpocket`).
//!
//! - E on a living actor while the player sneaks shows **Steal** (`crate::activate`) and
//!   opens [`OpenPickpocket`]: the container menu on the actor's inventory in its steal
//!   mode (`crate::menu::model::Steal`), equipped items not listed.
//! - Each take or placement rolls against its chance, shown under the menu. A failed
//!   roll closes the menu: the victim says Pickpocket and turns on the player, and will
//!   not be pickpocketed again ("<name> has already caught you.", `sNoPickPocketAgain`).
//! - A take that succeeds is theft (karma, Pockets Picked). A grenade or mine placed in
//!   the pocket (a weapon whose projectile explodes and has Alt. Trigger [documented: GECK
//!   wiki Projectile notes]) goes live, but not on a child or an essential actor: after
//!   `fProjectileInventoryGrenadeTimer` its explosion goes off where the victim stands
//!   (Pants Exploded), killing it when the explosion hurts its kind
//!   (`Game::explosive_harms`: a pulse grenade kills a robot, not a man); its blast hurts
//!   others as a thrown one's does [hypothesis]. Until then the victim panics: it plays
//!   the idle tree's panic idle (`Game::panic_idle`, `nvforever`'s idles) and stands
//!   where it is (`nvforever`'s residents). Each plant reserves its unit in the pocket
//!   (`Extra::live`): the pocket lists it as "<item> (Armed)" (`menu::model::LIVE_NOTE`)
//!   and nobody takes, uses or drops it until its blast (#1640 item 5).
//!
//! Logged: `pickpocket: open on …`, `pickpocket: took …`, `pickpocket: caught by …`,
//! `pickpocket: planted …` (or `placed … not live on …`), then `pickpocket live check: <item>
//! on …: row "<the open pocket's row, as listed> x<count>", reserved=<live units>
//! ordinary=<other units>` (`row "no row"` when the pocket does not list it),
//! `pickpocket: … explodes in …'s
//! pocket: it dies` (or `: it lives, the explosion does not hurt its kind`), and at exit
//! `pickpocket summary: …`.

use fnv_util::words::trf;
use crate::activate::HudMessage;
use crate::combat::{Combat, DESTRUCTION_BLAST_HEIGHT};
use crate::script::ScriptState;
use crate::sim::SimState;
use crate::PlaySet;
use bevy::prelude::*;
use fnv_sim::FormId;

/// Open the container menu on `victim`'s pocket (steal mode).
#[derive(Message, Clone, Copy, Debug)]
pub struct OpenPickpocket {
    pub victim: u32,
}

/// What a command of the steal menu did (`crate::menu`): items taken and placed, or the
/// item whose roll failed.
#[derive(Message, Clone, Debug)]
pub struct Pickpocketed {
    pub victim: FormId,
    pub taken: Vec<(FormId, i32)>,
    pub given: Vec<(FormId, i32)>,
    /// What each of `given` did, in its order ([`arm_given`], run by the menu as the item
    /// went in).
    pub placed: Vec<Placed>,
    pub caught: Option<FormId>,
}

/// What placing an item in a pocket did ([`arm_given`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Placed {
    /// A live explosive, its explosion: each unit planted, and reserved in the pocket.
    Live(FormId),
    /// An explosive that did not go live (a child, an essential actor).
    NotLive(fnv_game::pickpocket::NotLive),
    /// Not an explosive.
    Plain,
}

/// A live explosive's explosion and blast, when `item` is one once placed in a pocket
/// ([`live_explosive`], [`blast_of`]).
/// Whether the player's skill is under the requirement of the weapon `item` while the
/// Planting Needs the Skill tweak is on: such an explosive goes in as a plain item
/// [design: docs/design/tweaks.md].
pub(crate) fn under_skill(rt: &mut crate::script::Runtime, sim: &mut SimState, c: &Combat, item: FormId) -> bool {
    if !rt.game.tweaks.planting_needs_skill {
        return false;
    }
    let need = c.forms.weapons.get(&item).and_then(|w| Some((fnv_game::functions::av_by_name(w.skill.as_deref()?)?, w.skill_req)));
    let Some((skill, req)) = need else { return false };
    let has = crate::script::with_world(rt, sim, |rt| fnv_game::functions::actor_value_capped(&mut rt.game, FormId::PLAYER_REF, skill));
    let short = has < f64::from(req);
    if short {
        info!("pickpocket: {} goes in not live (skill {has:.0} under {req}, tweak)", item_name(sim, item));
    }
    short
}

pub(crate) fn explosive_of(c: &Combat, item: FormId) -> Option<(FormId, fnv_game::pickpocket::Explosive)> {
    let explosion = live_explosive(c, item)?;
    Some((explosion, blast_of(c, item, explosion)))
}

/// The player placed `given` in `victim`'s pocket (the menu's command, `crate::menu`):
/// each live explosive (`explosive`, [`explosive_of`] in play) is planted at once
/// (`Game::plant_explosive`, which reserves its unit, `Extra::live`), on the played world,
/// and the open pocket menu `menu` is listed again before any further input, so its row
/// shows "(Armed)" and a take on it is refused before any roll [design, #1640]. Logs
/// `pickpocket: planted …` and `pickpocket live check: <row as listed> …`.
pub(crate) fn arm_given(
    rt: &mut crate::script::Runtime,
    sim: &mut SimState,
    menu: Option<&mut crate::menu::model::MenuModel>,
    explosive: impl Fn(FormId) -> Option<(FormId, fnv_game::pickpocket::Explosive)>,
    victim: FormId,
    given: &[(FormId, i32)],
) -> Vec<Placed> {
    let name = sim.ref_name(Some(&rt.game), victim.0).map_or_else(|| victim.to_string(), str::to_string);
    let mut placed = Vec::new();
    let mut live = Vec::new();
    for &(item, n) in given {
        let Some((explosion, blast)) = explosive(item) else {
            info!("pickpocket: placed {} x{n} on {name} {victim}", item_name(sim, item));
            placed.push(Placed::Plain);
            continue;
        };
        let refused = crate::script::with_world(rt, sim, |rt| (0..n).find_map(|_| rt.game.plant_explosive(victim, item, blast).err()));
        match refused {
            Some(why) => {
                info!("pickpocket: placed {} x{n} not live on {name} {victim} ({})", item_name(sim, item), match why {
                    fnv_game::pickpocket::NotLive::Child => "a child",
                    fnv_game::pickpocket::NotLive::Essential => "essential",
                });
                placed.push(Placed::NotLive(why));
            }
            None => {
                info!("pickpocket: planted {} x{n} (live, explosion {explosion}) on {name} {victim}", item_name(sim, item));
                placed.push(Placed::Live(explosion));
                live.push(item);
            }
        }
    }
    if live.is_empty() {
        return placed;
    }
    let mut menu = menu.filter(|m| m.mode == crate::menu::model::Mode::Container(victim));
    if let Some(m) = menu.as_deref_mut() {
        if let Err(e) = m.refresh(&mut sim.world) {
            warn!("pickpocket: the pocket cannot be listed again: {e}");
        }
    }
    for item in live {
        let (reserved, ordinary) = sim.world.peek(victim).and_then(|s| s.inventory.as_ref()).map_or((0, 0), |inv| (inv.live_count(item), inv.free_count(item)));
        // The row as the open pocket shows it now, read from the menu itself.
        let row = menu.as_deref().and_then(|m| m.list(crate::menu::model::Side::Container).rows.iter().find(|r| r.item == item && r.live).map(|r| format!("{} x{}", r.name, r.count)));
        let row = row.unwrap_or_else(|| "no row".into());
        info!("pickpocket live check: {} on {name} {victim}: row \"{row}\", reserved={reserved} ordinary={ordinary}", item_name(sim, item));
    }
    placed
}

#[derive(Resource, Default)]
struct Stats {
    opened: u32,
    taken: u32,
    caught: u32,
    planted: u32,
    exploded: u32,
    killed: u32,
}

pub struct PickpocketPlugin;

impl Plugin for PickpocketPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Stats>()
            .add_message::<OpenPickpocket>()
            .add_message::<Pickpocketed>()
            .add_systems(Update, (results, blasts.run_if(resource_exists::<Combat>)).chain().in_set(PlaySet::Activation))
            .add_systems(Update, count_opened)
            .add_systems(Last, summary);
    }
}

/// The text after a victim's name when it caught the player before: `sNoPickPocketAgain`
/// ("has already caught you." [observed: FalloutNV.esm]).
pub fn caught_before_line(sim: &SimState, name: &str) -> String {
    match sim.forms.settings.get("snopickpocketagain") {
        Some(fnv_sim::settings::Setting::Str(s)) if !s.trim().is_empty() => format!("{name} {}", s.trim()),
        // Without the setting, our own words as one text (#3146).
        _ => trf("{name} has already caught you.", &[("name", &name)]),
    }
}

/// Whether `item` is a live explosive once placed in a pocket: a weapon whose projectile
/// explodes and has the Alt. Trigger flag [documented: GECK wiki Projectile notes].
fn live_explosive(c: &Combat, item: FormId) -> Option<FormId> {
    let w = c.forms.weapons.get(&item)?;
    let p = c.forms.projectiles.get(&w.projectile?)?;
    p.explodes().filter(|_| p.has("Alt. Trigger"))
}

/// What the rules need of `item`'s blast to judge its victim (`fnv_game::pickpocket::Explosive`):
/// the explosion's damage and object effect, the weapon's damage type.
fn blast_of(c: &Combat, item: FormId, explosion: FormId) -> fnv_game::pickpocket::Explosive {
    let x = c.forms.explosions.get(&explosion);
    let resist = c.forms.weapons.get(&item).and_then(fnv_combat::damage::resistance_of).and_then(fnv_game::functions::av_by_name);
    fnv_game::pickpocket::Explosive { damage: x.map_or(0.0, |x| f64::from(x.damage)), resist, enchantment: x.and_then(|x| x.enchantment) }
}

fn results(mut done: MessageReader<Pickpocketed>, mut sim: ResMut<SimState>, mut script: Option<ResMut<ScriptState>>, mut stats: ResMut<Stats>, mut hud: Option<ResMut<HudMessage>>) {
    for p in done.read() {
        let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else { continue };
        let victim = p.victim;
        let name = sim.ref_name(Some(&rt.game), victim.0).map_or_else(|| victim.to_string(), str::to_string);
        for &(item, n) in &p.taken {
            stats.taken += 1;
            crate::script::with_world(rt, &mut sim, |rt| rt.game.pickpocket_took(victim, item, n));
            // The player's Pickpocket Overhaul tweak's experience [design: docs/design/tweaks.md].
            if rt.game.tweaks.pickpocket_overhaul && rt.game.tweaks.pp_reward_xp {
                let it = sim.forms.item(item);
                let (value, weight) = it.map_or((0.0, 0.0), |i| (f64::from(i.value.unwrap_or(0).max(0)) * f64::from(n), f64::from(i.weight.unwrap_or(0.0).max(0.0)) * f64::from(n)));
                let xp = crate::script::with_world(rt, &mut sim, |rt| rt.game.pickpocket_overhaul(victim).map_or(0.0, |o| o.xp(value, weight)));
                if xp > 0.0 {
                    crate::script::with_world(rt, &mut sim, |rt| fnv_game::experience::award(&mut rt.game, fnv_game::experience::Source::Pickpocket, xp));
                }
            }
            info!("pickpocket: took {} x{n} from {name} {victim} unseen", item_name(&sim, item));
        }
        // The placements were armed by the menu as they went in (`arm_given`).
        for (&(_, n), placed) in p.given.iter().zip(&p.placed) {
            if matches!(placed, Placed::Live(_)) {
                stats.planted += n.max(0) as u32;
            }
        }
        if let Some(item) = p.caught {
            stats.caught += 1;
            let turned = crate::script::with_world(rt, &mut sim, |rt| rt.game.pickpocket_caught(victim));
            info!("pickpocket: caught by {name} {victim} with {}{}", item_name(&sim, item), if turned { ": it turns on the player" } else { "" });
            if let Some(h) = hud.as_deref_mut() {
                h.show(trf("{name} caught you", &[("name", &name)]));
            }
        }
    }
}

/// Planted explosives whose time is up go off where their victims stand.
fn blasts(mut sim: ResMut<SimState>, mut script: Option<ResMut<ScriptState>>, mut c: ResMut<Combat>, mut stats: ResMut<Stats>) {
    let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else { return };
    if rt.game.pickpocket.planted.is_empty() {
        return;
    }
    let due = crate::script::with_world(rt, &mut sim, |rt| rt.game.tick_planted());
    for b in due {
        stats.exploded += 1;
        let fate = if b.killed {
            stats.killed += 1;
            "it dies"
        } else {
            "it lives, the explosion does not hurt its kind"
        };
        let at = sim.world.peek(b.victim).map(|s| s.pos);
        match (live_explosive(&c, b.item), at) {
            (Some(e), Some(p)) => {
                c.mine_explodes(e, [p[0], p[1], p[2] + DESTRUCTION_BLAST_HEIGHT], Some((FormId::PLAYER_REF, b.item)));
                info!("pickpocket: {} explodes in {}'s pocket: {fate}", item_name(&sim, b.item), b.victim);
            }
            _ => info!("pickpocket: {} explodes in {}'s pocket: {fate} (no explosion drawn)", item_name(&sim, b.item), b.victim),
        }
    }
}

fn summary(mut exit: MessageReader<AppExit>, stats: Res<Stats>) {
    if exit.read().next().is_some() && stats.opened + stats.caught + stats.taken > 0 {
        info!(
            "pickpocket summary: {} pockets opened, {} takes unseen, caught {} times, {} explosives planted, {} went off, {} killed",
            stats.opened, stats.taken, stats.caught, stats.planted, stats.exploded, stats.killed
        );
    }
}

fn count_opened(mut open: MessageReader<OpenPickpocket>, mut stats: ResMut<Stats>) {
    stats.opened += open.read().count() as u32;
}

fn item_name(sim: &SimState, id: FormId) -> String {
    sim.name(id).map_or_else(|| id.to_string(), str::to_string)
}
