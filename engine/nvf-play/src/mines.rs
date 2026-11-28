//! Mines in play (docs/spec-notes/combat-rules.md §11a): the plugins' placed mines (PGRE)
//! and those the player puts down, live in the loaded cells. The rules are
//! `fnv_combat::mine`'s; this module applies them to the world.
//!
//! - **Which.** A reference whose base is a mine PROJ (`fnv_combat::mine::is_mine`): the
//!   plugins' PGRE (about 300, listed once), and references created at run time when a
//!   thrown mine comes to rest (`crate::combat`'s `SettledMine`): put in the world at once
//!   where it lies, owned by whoever put it down, armed from then on [owner 2026-10-19:
//!   "your own mines arm immediately after leaving the player hand"]. A C-4 charge at
//!   rest is placed the same way (a reference, saved, of its cell), but is no mine: it
//!   waits for `DetonatePlacedExplosives` (`crate::combat`, #1986). A mine counts while
//!   its cell is loaded (it has collision: `PhysState::ref_cell`, `PhysState::created`).
//! - **Set off.** A live actor (not dead) within the trigger radius that the mine does not
//!   spare (`fnv_game::functions::mine_spares`: its owner, the owner's faction and
//!   allies) starts its fuse, as long as that actor's Explosives skill makes it
//!   (`MineRules::fuse`); the fuse burns on whoever stays. Distance is from the mine to
//!   the nearest point of the actor's upright body (feet to head) [design].
//! - **Goes off.** At the fuse's end the mine is disabled (the capture saw a mine disabled
//!   after it went off, simulation.md §1) and its PROJ's explosion goes off
//!   [`DESTRUCTION_BLAST_HEIGHT`] above it, the blast being the hits of whoever put it
//!   down (the player for the player's own) and nobody's for the plugins' mines [design].
//! - **Shot or caught in a blast.** The mine PROJs are destructible (DEST: 5 health, a
//!   stage that disables and destroys it with an explosion, `MediumExplosion` for the frag
//!   mine) [observed: the PROJ records], so a shot or another explosion sets one off
//!   through `crate::combat`'s destructibles, as any destructible [owner 2026-10-19: "you
//!   can shoot a mine to blow it up"].
//! - **Disarmed.** The player's activation makes it a loose item (`fnv_sim::World::
//!   activate`), even while the fuse burns; the PROJ's disable sound plays where it lay.
//! - **Tick and light.** A burning fuse plays the PROJ's countdown sound at each tick; the
//!   model's `##LightOn` part (its red glow) shows on a live mine and blinks while the fuse
//!   burns, its `##LightOff` part shows on a loose or disarmed one ([`show_lights`];
//!   the drawing code marks those meshes, [`MineLight::of`])
//!   [observed: the frag mine model's nodes; owner 2026-10-19: live mines show the light,
//!   it blinks once set off; hypothesis: which part shows when].
//! - **DisableAllMines** (Fallout 3's): the live mines of the player's cell are disabled
//!   at once, a burning fuse with them (`fnv_game::functions::fo3`).
//! - Not built: NPCs putting mines down, NPCs seeing and avoiding mines, the HUD's mine
//!   marker, mines owned by an actor base (XOWN naming an NPC_).

use crate::activate::drawn;
use crate::combat::{actor_av, Combat, DESTRUCTION_BLAST_HEIGHT};
use crate::phys::cells::Space;
use crate::phys::PhysState;
use crate::player::Player;
use crate::script::{with_world, ScriptState};
use crate::sfx::{PlaySound, Sfx};
use crate::sim::SimState;
use crate::{PlaySet, RefChanged, RefVisual};
use bevy::prelude::*;
use fnv_combat::mine::{is_mine, Fuse, FuseStep, MineRules};
use fnv_combat::vec::{self, V3};
use fnv_sim::FormId;
use std::collections::{HashMap, HashSet};

pub struct MinesPlugin;

impl Plugin for MinesPlugin {
    fn build(&self, app: &mut App) {
        // `tick` runs in `crate::combat`'s chain, after the projectiles fly.
        app.init_resource::<Mines>().add_systems(Update, show_lights.in_set(PlaySet::Sync).after(crate::activate::sync_visuals));
    }
}

/// The mines' state.
#[derive(Resource, Default)]
pub struct Mines {
    /// The mine PROJs and the plugins' placed mines, read on the first frame.
    kinds: Option<HashSet<FormId>>,
    /// The weapons that are mines (their projectile is one).
    items: HashSet<FormId>,
    placed: Vec<FormId>,
    /// Mines to set off at once (a failed disarm under the player's Disarm Requires
    /// Skill tweak) [design: docs/design/tweaks.md].
    pub set_off: Vec<FormId>,
    rules: MineRules,
    /// Mines whose fuse burns.
    burning: HashMap<FormId, Burning>,
    /// The live mines of the loaded cells at the last frame, where they lay.
    live: HashMap<FormId, V3>,
    /// Whether each live mine's light is lit now (by FormID).
    lit: HashMap<u32, bool>,
    /// (mine, actor) pairs whose perks kept the mine from going off, logged once.
    spared: HashSet<(FormId, FormId)>,
    /// The game's DisableAllMines count last acted on.
    disabled_count: u64,
}

struct Burning {
    fuse: Fuse,
    /// Who set it off, when someone did.
    by: Option<FormId>,
}

impl Mines {
    /// Live mines in the loaded cells now.
    pub fn live(&self) -> usize {
        self.live.len()
    }

    /// Whether `mine` is a live mine of the loaded cells.
    pub fn is_live(&self, mine: FormId) -> bool {
        self.live.contains_key(&mine)
    }
}

/// A point's distance from an actor's upright body, feet to head.
/// The Contact Mines tweak: a mine this close to an actor's upright axis (game units)
/// is stood on, and goes off with no fuse [design: docs/design/tweaks.md]; about a body's half width, our choice.
const CONTACT_DISTANCE: f32 = 24.0;

/// The sound level of a mine's blast as a detection event (the Mine Blasts Are Heard
/// tweak): the detection settings' loud sound [design: docs/design/tweaks.md].
const MINE_BLAST_LEVEL: f64 = 100.0;

fn body_distance(p: V3, feet: V3, height: f32) -> f32 {
    let z = p[2].clamp(feet[2], feet[2] + height.max(0.0));
    vec::len(vec::sub(p, [feet[0], feet[1], z]))
}

/// The compass heading of a horizontal direction (0 north, +y; clockwise), radians.
fn heading(dir: V3) -> f32 {
    if dir[0].abs() + dir[1].abs() < 1e-4 {
        0.0
    } else {
        dir[0].atan2(dir[1])
    }
}

/// Disable a mine that went off (or DisableAllMines turned off) for good: its own flag
/// counts from now on, not its enable parent's state. A placed mine with an enable parent
/// (XESP) otherwise stays enabled whatever its own flag says (`World::is_enabled`), so it
/// stayed live and went off again every couple of seconds [observed: player report
/// R-P9A6CD42, bottlecap mine 0x0008424C (enable parent 0x00126F4E, FalloutNV.esm) going
/// off 26 times in a minute, #3881]. This is the engine taking the mine away, not a
/// script's Disable, which a reference with an enable parent ignores [documented: GECK
/// wiki Enable and Disable].
fn put_out(world: &mut fnv_sim::World, id: FormId) -> Result<(), fnv_sim::world::SimError> {
    world.keep_own_enable(id);
    world.set_enabled(id, false)
}

/// Place thrown mines, set off, burn and explode mines (module notes).
#[allow(clippy::too_many_arguments)]
pub(crate) fn tick(
    time: Res<Time>,
    mut mines: ResMut<Mines>,
    mut c: ResMut<Combat>,
    mut sim: ResMut<SimState>,
    phys: Res<PhysState>,
    player: Option<Res<Player>>,
    mut script: Option<ResMut<ScriptState>>,
    sfx: Option<Res<Sfx>>,
    mut sounds: MessageWriter<PlaySound>,
    mut changed: MessageWriter<RefChanged>,
) {
    let dt = time.delta_secs();
    let m = &mut *mines;
    for id in std::mem::take(&mut m.set_off) {
        info!("mines: mine {id} set off by a failed disarm (tweak)");
        m.burning.insert(id, Burning { fuse: Fuse::new(0.0), by: Some(FormId::PLAYER_REF) });
    }
    if m.kinds.is_none() {
        let kinds: HashSet<FormId> = c.forms.projectiles.iter().filter(|(_, p)| is_mine(p)).map(|(&id, _)| id).collect();
        m.placed = sim.refs.refs.values().filter(|r| &r.sig == b"PGRE" && kinds.contains(&r.base)).map(|r| r.id).collect();
        m.items = c.forms.weapons.iter().filter(|(_, w)| w.projectile.is_some_and(|p| kinds.contains(&p))).map(|(&id, _)| id).collect();
        m.rules = MineRules::from_forms(&c.forms);
        info!("mines: {} mine projectile(s), {} placed mine(s)", kinds.len(), m.placed.len());
        m.kinds = Some(kinds);
    }
    let kinds = m.kinds.as_ref().expect("read above");

    // Thrown mines that came to rest: in the world, owned by who put them down.
    for s in c.take_settled() {
        let cell = sim.world.peek(s.actor).and_then(|a| a.cell).or_else(|| sim.world.peek(FormId::PLAYER_REF).and_then(|a| a.cell));
        match sim.world.place(s.proj, cell, s.point, [0.0, 0.0, heading(s.dir)], 1) {
            Ok(id) => {
                if let Ok(r) = sim.world.get(id) {
                    r.extra.owner = Some(s.actor);
                }
                changed.write(RefChanged::new(id.0));
                // A charge that waits for its detonator (C-4, #1986): a reference too, so
                // it is saved and stays in its cell; `crate::combat` sets it off.
                if c.forms.projectiles.get(&s.proj).is_some_and(|p| p.has("Detonates")) {
                    info!("explosives: {} put down charge {id} ({}) at ({:.0}, {:.0}, {:.0}), waiting for its detonator", s.actor, s.proj, s.point[0], s.point[1], s.point[2]);
                    continue;
                }
                info!("mines: {} put down mine {id} ({}) at ({:.0}, {:.0}, {:.0}), armed", s.actor, s.proj, s.point[0], s.point[1], s.point[2]);
            }
            Err(e) => warn!("mines: a thrown mine could not be placed: {e}"),
        }
    }

    // The live mines of the loaded cells.
    let created: Vec<FormId> = sim.world.created().filter(|(id, r)| !r.deleted() && kinds.contains(&r.base) && phys.created.contains_key(&id.0)).map(|(id, _)| id).collect();
    let mut live: HashMap<FormId, V3> = HashMap::new();
    for id in m.placed.iter().copied().filter(|id| phys.ref_cell.contains_key(&id.0)).chain(created) {
        if !sim.present(id.0) {
            continue;
        }
        let pos = sim.world.peek(id).map(|r| r.pos).or_else(|| sim.refs.get(id).map(|p| p.pos));
        if let Some(pos) = pos {
            live.insert(id, pos);
        }
    }
    // Fallout 3's DisableAllMines (`fnv_game::functions::fo3`): the live mines of the
    // player's cell are disabled, fuses and all.
    let asked = script.as_deref().and_then(|s| s.rt.as_ref()).map(|rt| rt.game.ui.mines_disabled);
    if let Some(n) = asked.filter(|&n| n != m.disabled_count) {
        m.disabled_count = n;
        let here = sim.world.peek(FormId::PLAYER_REF).and_then(|a| a.cell);
        let cell_of = |id: FormId| sim.world.peek(id).and_then(|r| r.cell).or_else(|| sim.refs.get(id).and_then(|p| p.cell));
        let off: Vec<FormId> = live.keys().copied().filter(|&id| here.is_some() && cell_of(id) == here).collect();
        for &id in &off {
            if let Err(e) = put_out(&mut sim.world, id) {
                warn!("mines: mine {id} could not be disabled: {e}");
            }
            changed.write(RefChanged::new(id.0));
            live.remove(&id);
            m.burning.remove(&id);
        }
        info!("mines: {} mine(s) of the player's cell disabled (DisableAllMines)", off.len());
    }
    // Gone since the last frame: taken by the player's activation (disarmed), or
    // disabled (it went off, or a blast or a shot destroyed it).
    for (&id, &pos) in &m.live {
        if live.contains_key(&id) {
            continue;
        }
        m.burning.remove(&id);
        if sim.world.peek(id).is_some_and(|r| r.deleted()) {
            let sound = sim.base_of(id.0).and_then(|b| sfx.as_deref()?.forms.projectiles.get(&b)?.sound_disable);
            if let Some(sound) = sound {
                sounds.write(PlaySound { sound, at: Some(pos) });
            }
            info!("mines: mine {id} disarmed");
        }
    }
    m.burning.retain(|id, _| live.contains_key(id));

    // Who is near: the actors' bodies (not the dead) and the player's.
    let mut bodies: Vec<(FormId, V3, f32)> = phys
        .world
        .actor_volumes()
        .filter_map(|id| phys.world.actor_volume(id).filter(|v| !v.dead).map(|v| (FormId(id), v.feet, v.height)))
        .collect();
    if let Some(p) = player.as_deref() {
        bodies.push((FormId::PLAYER_REF, p.feet, p.eye_height));
    }
    let exterior = phys.space == Space::Exterior;
    let mut lit: HashMap<u32, bool> = HashMap::new();
    for (&id, &pos) in &live {
        let Some(base) = sim.base_of(id.0) else { continue };
        let Some(p) = c.forms.projectiles.get(&base).cloned() else { continue };
        if !m.burning.contains_key(&id) {
            let owner = sim.world.peek(id).and_then(|r| r.extra.owner).or_else(|| sim.refs.get(id).and_then(|r| r.owner));
            let radius = m.rules.trigger_radius(&p, exterior);
            let rt = script.as_deref().and_then(|s| s.rt.as_ref());
            let near: Vec<(FormId, f32)> = bodies
                .iter()
                .map(|&(a, feet, h)| (a, body_distance(pos, feet, h)))
                .filter(|&(_, d)| d <= radius)
                .filter(|&(a, _)| match rt {
                    Some(rt) => !rt.game.is_dead(a) && !fnv_game::functions::mine_spares(&rt.game, owner, a),
                    None => owner != Some(a),
                })
                .collect();
            // Light Step's Calculate Mine Explode Chance: an actor whose chance is 0 does
            // not set it off (the player, and teammates with ED-E's Light Step;
            // docs/spec-notes/perk-entry-points.md §8) [design: the masters' entries all
            // Set 0, so no part chance is rolled].
            let mut by = None;
            for (a, d) in near {
                // The player's tweak: a teammate is spared when the player would be [design: docs/design/tweaks.md].
                let chance = script.as_deref_mut().and_then(|s| s.rt.as_mut()).map_or(1.0, |rt| {
                    with_world(rt, &mut sim, |rt| {
                        let who = if rt.game.tweaks.light_step_companions && rt.game.is_teammate(a) { FormId::PLAYER_REF } else { a };
                        rt.game.entry_point(who, fnv_game::perks::CALCULATE_MINE_EXPLODE_CHANCE, 1.0).0
                    })
                });
                if chance <= 0.0 {
                    if m.spared.insert((id, a)) {
                        info!("mines: {a} does not set off mine {id} at {d:.0} units (Calculate Mine Explode Chance {chance}: Light Step)");
                    }
                    continue;
                }
                by = Some((a, d));
                break;
            }
            if let Some((a, d)) = by {
                // Without scripts there are no actor values: a middling skill [design].
                let skill = actor_av(script.as_deref_mut(), &mut sim, a, fnv_game::actor_values::EXPLOSIVES).unwrap_or(50.0);
                // The player's tweak: a mine stood on goes off at once [design: docs/design/tweaks.md].
                let contact = script.as_deref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| if a == FormId::PLAYER_REF { rt.game.tweaks.contact_mines_player } else { rt.game.tweaks.contact_mines_npcs });
                let secs = if contact && d <= CONTACT_DISTANCE { 0.0 } else { m.rules.fuse(&p, skill) };
                info!("mines: {a} sets off mine {id} at {d:.0} units (trigger radius {radius:.0}): fuse {secs:.2} s at Explosives {skill:.0}");
                m.burning.insert(id, Burning { fuse: Fuse::new(secs), by: Some(a) });
            }
        }
        let Some(b) = m.burning.get_mut(&id) else {
            lit.insert(id.0, true);
            continue;
        };
        match b.fuse.step(dt) {
            FuseStep::Burning => {}
            FuseStep::Tick => {
                if let Some(sound) = sfx.as_deref().and_then(|s| s.forms.projectiles.get(&base)).and_then(|f| f.sound_countdown) {
                    sounds.write(PlaySound { sound, at: Some(pos) });
                }
            }
            FuseStep::Done => {
                let by = m.burning.remove(&id).and_then(|b| b.by);
                if let Err(e) = put_out(&mut sim.world, id) {
                    warn!("mines: mine {id} could not be disabled: {e}");
                }
                changed.write(RefChanged::new(id.0));
                // The blast is the hits of whoever put it down (an actor reference).
                let owner = sim.world.peek(id).and_then(|r| r.extra.owner).filter(|o| sim.world.peek(*o).is_some() && (sim.is_actor(o.0) || *o == FormId::PLAYER_REF));
                let weapon = p.default_weapon;
                match p.explodes() {
                    Some(e) => {
                        c.mine_explodes(e, [pos[0], pos[1], pos[2] + DESTRUCTION_BLAST_HEIGHT], owner.zip(weapon));
                        info!("mines: mine {id} goes off");
                        // The player's tweak: the blast is an explosion event the actors
                        // near measure as any other, its owner whoever put the mine down,
                        // else whoever set it off [design: docs/design/tweaks.md].
                        if let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()).filter(|rt| rt.game.tweaks.mine_blasts_heard) {
                            if let Some(by) = owner.or(by) {
                                rt.game.queue_detection_event(fnv_game::awareness::DetectionEvent { owner: by, at: pos, level: MINE_BLAST_LEVEL, kind: fnv_game::awareness::EventType::Explosion });
                            }
                        }
                    }
                    None => warn!("mines: mine {id} has no explosion"),
                }
                continue;
            }
        }
        lit.insert(id.0, b.fuse.lit);
    }
    if live.len() != m.live.len() {
        info!("mines: {} live mine(s) in the loaded cells", live.len());
    }
    // Scripts' GetMineArmed (`fnv_game::nvse_gaps`): live mines not going off and not
    // the player's.
    if let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) {
        // GetMineArmedEx (`fnv_game::nvse_jg6`): the player's live mines too.
        rt.game.other.jg6.armed_mines_all = live.keys().filter(|id| !m.burning.contains_key(id)).copied().collect();
        rt.game.gaps.armed_mines = live
            .keys()
            .filter(|id| !m.burning.contains_key(id) && sim.world.peek(**id).and_then(|r| r.extra.owner) != Some(FormId::PLAYER_REF))
            .copied()
            .collect();
    }
    m.live = live;
    m.lit = lit;
}

/// A mesh of a mine model's light: part of its `##LightOn` group (lit), else of its
/// `##LightOff` group. The drawing code puts it on such meshes as they are spawned
/// ([`MineLight::of`]); [`show_lights`] shows the one or the other.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct MineLight {
    on: bool,
}

impl MineLight {
    /// The light group of a model shape, by its name and its parents' (nearest first).
    pub fn of(name: Option<&str>, ancestors: &[String]) -> Option<MineLight> {
        name.into_iter().chain(ancestors.iter().map(String::as_str)).find_map(light_group).map(|on| MineLight { on })
    }
}

/// Whether a model shape is part of a mine's lit light (`##LightOn`: the glow and the lit
/// lens), by its name and its parents' names. A mine in the hand shows its dark lens only
/// [observed: the game's first-person frames of the frag mine, owner 2026-10-23].
pub fn lit_part(name: Option<&str>, ancestors: &[String]) -> bool {
    MineLight::of(name, ancestors).is_some_and(|l| l.on)
}

/// The light group a node name opens: `##LightOn…` lit, `##LightOff…` dark.
fn light_group(name: &str) -> Option<bool> {
    let n = name.to_ascii_lowercase();
    if n.starts_with("##lightoff") {
        Some(false)
    } else if n.starts_with("##lighton") {
        Some(true)
    } else {
        None
    }
}

/// Show a live mine's lit part (blinking while its fuse burns) and a loose or disarmed
/// one's dark part. Only on mines and the weapons they are: other models' parts of these
/// names are left alone.
fn show_lights(mines: Res<Mines>, sim: Res<SimState>, script: Option<Res<ScriptState>>, mut lights: Query<(&RefVisual, &MineLight, &mut Visibility)>, mut told: Local<HashSet<u32>>) {
    let Some(kinds) = mines.kinds.as_ref() else { return };
    let script = script.as_deref();
    for (r, l, mut v) in &mut lights {
        let Some(base) = sim.base_of(r.0) else { continue };
        let lit = if kinds.contains(&base) {
            mines.lit.get(&r.0).copied().unwrap_or(true)
        } else if mines.items.contains(&base) {
            false
        } else {
            continue;
        };
        if told.insert(r.0) {
            info!("mines: 0x{:08X} drawn with its light parts", r.0);
        }
        if !drawn(&sim, script, r.0) {
            continue;
        }
        v.set_if_neq(if l.on == lit { Visibility::Inherited } else { Visibility::Hidden });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #3881: a placed mine with an enable parent is gone once it went off; the parent and
    /// what else hangs on it are not touched.
    #[test]
    fn a_mine_with_an_enable_parent_stays_off_after_it_goes_off() {
        use fnv_sim::refs::EnableParent;
        const PARENT: u32 = 0x0012_6F4E;
        const MINE: u32 = 0x0008_424C;
        const SIBLING: u32 = 0x0008_4250;
        let child = |id| fnv_sim::refs::PlacedRef { enable_parent: Some(EnableParent { reference: FormId(PARENT), opposite: false, pop_in: false }), ..crate::sim::placed_ref(id) };
        let mut sim = SimState::over_placed([crate::sim::placed_ref(PARENT), child(MINE), child(SIBLING)]);
        // The plain disable has no effect: the parent decides.
        sim.world.set_enabled(FormId(MINE), false).unwrap();
        assert!(sim.present(MINE), "the bug: the mine stayed live");
        put_out(&mut sim.world, FormId(MINE)).unwrap();
        assert!(!sim.present(MINE));
        assert!(sim.present(PARENT) && sim.present(SIBLING));
    }

    #[test]
    fn distance_is_to_the_upright_body() {
        // A mine on the floor 50 units in front of a standing man: 50 units.
        assert!((body_distance([50.0, 0.0, 0.0], [0.0, 0.0, 0.0], 120.0) - 50.0).abs() < 1e-4);
        // Under a man's feet from 10 units below.
        assert!((body_distance([0.0, 0.0, -10.0], [0.0, 0.0, 0.0], 120.0) - 10.0).abs() < 1e-4);
        // Level with his head.
        assert!((body_distance([30.0, 40.0, 100.0], [0.0, 0.0, 0.0], 120.0) - 50.0).abs() < 1e-4);
    }

    #[test]
    fn light_parts_by_name() {
        // The frag mine's: the glow is under ##LightOn, the dark lens is ##LightOff:0.
        let on = Some(MineLight { on: true });
        assert_eq!(MineLight::of(Some("##Glow:0"), &["##LightOn".into(), "##FMine".into()]), on);
        assert_eq!(MineLight::of(Some("##LightOn:0"), &["##LightOn".into(), "##FMine".into()]), on);
        assert_eq!(MineLight::of(Some("##LightOff:0"), &["##FMine".into()]), Some(MineLight { on: false }));
        assert_eq!(MineLight::of(Some("##FMine:0"), &["##FMine".into()]), None);
    }

    #[test]
    fn heading_is_compass() {
        assert!(heading([0.0, 1.0, 0.0]).abs() < 1e-5);
        assert!((heading([1.0, 0.0, 0.0]) - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
    }
}
