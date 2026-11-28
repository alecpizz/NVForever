//! Scripted weapon use (docs/design/use-weapon.md): an actor with a weapon use
//! (`fnv_game::use_weapon`: the UseWeapon function, a Use Weapon package) that the AI has
//! brought to its place and turned to its target fires its shots at it, as the combat
//! AI's gunmen do (`npc_combat`: the weapon's own cycle, aimed from the eyes at the
//! target's chest, drawn from the muzzle), counting them on the game's use.
//!
//! - **When it fires**: with its weapon drawn (`alert.rs` draws it for a weapon use),
//!   facing its target (a gun within [`shot_facing`], a blow within [`USE_FACING`]), not restrained (SetRestrained
//!   "blocks weapon use from Use Weapon packages and UseWeapon" [documented: catalogue
//!   notes on SetRestrained]), its target where the use wants it (the function's last
//!   argument, within `fnv_game::use_weapon::TARGET_HERE_RADIUS`), and, with Hold Fire
//!   When Blocked, nobody else standing in its line of fire (`combat_tactics`) [design:
//!   what "blocked" covers is not documented; walls are not looked at]. Between volleys
//!   it waits its pause.
//! - **The weapon**: the use's weapon when it is a gun, else the gun drawn on it; any
//!   other weapon, or none, swings (**Melee** below) [design]. Its drawn model is the one it was spawned with: a use
//!   naming another weapon fires that weapon's shots from the model it holds (models are
//!   not swapped live yet).
//! - **Always Hit**: no spread at all, aimed at the chest or object aim point [design: the
//!   shot can still meet cover between them]. **Do No Damage**: the shots are harmless
//!   (`nvf_play::combat::ActorFire::harmless`). **Crouch To Reload**: it crouches
//!   (`Game::sneaks`) while its weapon reloads.
//! - **The end**: all shots fired, or its target dead or gone: the game ends the use
//!   (`Game::end_weapon_use`).
//!
//! **Melee** (docs/design/use-weapon.md §3.1): a use naming a melee or unarmed weapon,
//! or naming none on an actor with no gun drawn (its melee weapon drawn, else its fists,
//! a creature's own attack), swings instead of firing: each of its "shots" is one blow
//! [documented: the page counts "how many times to use the weapon"]. As the combat AI's
//! melee fighters (`npc_combat`): the reach and spacing of `npc_combat::melee_spacing` /
//! `creature_spacing`, which this sets on the game's use (`WeaponUse::reach`) for the AI
//! to close in by (a script's use; a package's swings from where it stands); a blow every
//! `npc_combat::STRIKE_SECONDS` over the weapon's attack speed. Scripted swings do not
//! establish an attack pose yet (`attack_anims::start` requires a fighter pitch or an
//! existing attack), so no clip timing reaches them in play [design, #2535: open].
//! If a clip is published, a creature's next blow waits for its boundary
//! (`SwingClip::ready_in`) [design, #2535]; a person's keeps the base wait, brought
//! forward by an earlier `a:` key (`SwingClip::person_ready_in`; the maintainer's
//! decision of 2026-11-18 on #2971). Volley pauses also hold the next blow, on their own
//! clock for a creature [design: Swinger::advance, Swinger::pause]. With a published
//! clip a combo uses `attack_anims::SwingPick` and its `Hit` key; otherwise the blow
//! keeps the existing Hit delay (fallback `DEFAULT_HIT`) [design: swing]. It lands while the
//! target is within reach. No block or attack-chance roll: the script wants the blows
//! [hypothesis]. **Always Hit**: a blow lands even when the target stepped out of reach
//! [hypothesis]. **Do No Damage**: the blow strikes nothing [design: a
//! harmless blow has no sound or impact yet]. Hold Fire When Blocked and the target's
//! place hold the swings as they hold shots; Crouch To Reload has nothing to reload.
//! Thrown weapons fire their projectile as guns do (`is_gun`: they have one).

use crate::attack_anims::{AttackTimes, Move, PlayAttack, SwingClip, SwingPick, DEFAULT_HIT};
use crate::combat_tactics as tactics;
use crate::limbs::ActorRig;
use crate::locomotion::Dying;
use crate::npc_combat::{creature_reach, creature_spacing, engage, facing_off, is_gun, melee_spacing, next_unit, CombatBodies, CHEST, CREATURE_REACH, MUZZLE_AHEAD, STRIKE_SECONDS};
use crate::ragdoll::Corpse;
use crate::weapons::{HeldWeapon, Muzzle, WeaponHold};
use crate::world::bevy_to_game;
use bevy::prelude::*;
use fnv_actor::weapon::Stage;
use fnv_combat::attack::{ClipTiming, Cycle, Event, Input, Phase, Timing};
use fnv_combat::data::WeaponStats;
use fnv_combat::tuning::CombatTuning;
use fnv_combat::spread::{self, Stance};
use fnv_combat::vec::{self, V3};
use fnv_game::use_weapon::{After, Source, TARGET_HERE_RADIUS};
use fnv_sim::FormId;
use nvf_play::blocking::ActorBlocks;
use nvf_play::combat::{ActorFire, ActorStrike, Combat, CombatStats};
use nvf_play::phys::PhysState;
use nvf_play::player::{Player, EYE_HEIGHT};
use nvf_play::script::{with_world, ScriptState};
use nvf_play::sim::SimState;
use std::collections::{BTreeMap, HashMap};

mod targets;
pub(crate) use targets::target_place;
#[cfg(test)]
mod object_tests;

/// A scripted weapon use swings only while its target is within this angle of straight
/// ahead, radians (30°: the combat AI's arc before it held its gunmen to 3°, kept for
/// blows) [design]. A gun's shots have [`shot_facing`].
pub const USE_FACING: f32 = 30.0 * std::f32::consts::PI / 180.0;

/// How far off its heading a weapon use's shooter may have its target and fire, radians:
/// the combat gunmen's rule (`npc_combat::fire_facing`: 3°, or the weapon's Aim Arc when
/// above 0) [design: issue #2725. A use's shot flies from the eyes straight at the
/// target whatever way the body points, so at the 30° of [`USE_FACING`] Orris shot his
/// thugs with the revolver pointing well aside while he was still turning to the next
/// one; the original's gunmen fire only once turned to their target, observed for combat
/// (campaign fiend_facing), a hypothesis for a package's use]. The in-place turn
/// (`fnv_ai::carry`) brings the shooter round.
pub fn shot_facing(aim_arc: f32) -> f32 {
    crate::npc_combat::fire_facing(aim_arc)
}

pub struct UseWeaponPlugin;

impl Plugin for UseWeaponPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Shooters>()
            .add_systems(Update, shoot.after(nvf_play::PlaySet::Scripts).run_if(resource_exists::<Combat>))
            .add_systems(Last, report);
    }
}

/// One actor carrying out a weapon use.
struct Shooter {
    /// The use's number (`WeaponUse::seq`) and the gun it fires.
    seq: u64,
    weapon: FormId,
    cycle: Cycle,
    pulled: bool,
    /// Seconds before it may pull the trigger again: a semi-automatic gun's delay, or
    /// the pause between volleys.
    wait: f32,
    /// What it last waited for, told once each time it changes.
    held: Option<&'static str>,
    /// Its first shot of this use has been told (how far off its heading the target was).
    told: bool,
    /// Its cycle's reload takes as long as its rig's reload clips (`AttackTimes::reloads`).
    reload_timed: bool,
    /// Seconds since it last had a use; 0 while it has one.
    idle: f32,
    /// The clock at its last shot.
    last_shot: Option<f32>,
}

/// How long an actor between two uses keeps its gun's cycle: the time left before its
/// next shot and what its magazine holds [design: chosen; the original's is not known].
/// Orris's three packages follow one another within seconds (#2763).
const KEEP_IDLE: f32 = 30.0;

impl Shooter {
    fn new(seq: u64, weapon: FormId, ws: &WeaponStats, timing: Timing) -> Shooter {
        Shooter { seq, weapon, cycle: Cycle::new(ws, timing, false, ws.clip_size.max(1)), pulled: false, wait: 0.0, held: None, told: false, reload_timed: false, idle: 0.0, last_shot: None }
    }

    /// It goes on to another use with the same gun: the gun's cycle (the time left
    /// before its next shot, its magazine, a reload under way) and the wait before its
    /// next pull stay as they are, as a combat gunman's do when its target changes
    /// (`npc_combat`'s `kept`) (#2763) [design].
    fn next_use(&mut self, seq: u64) {
        self.seq = seq;
        self.held = None;
        self.told = false;
        self.idle = 0.0;
    }

    /// A frame without a use: the cycle and the wait run on. Whether it is kept.
    fn rest(&mut self, t: &CombatTuning, dt: f32) -> bool {
        self.idle += dt;
        self.wait = (self.wait - dt).max(0.0);
        self.pulled = false;
        let _ = self.cycle.step(t, Input { trigger: false, reload: false, dt, carried: u32::MAX / 2 });
        self.idle < KEEP_IDLE
    }

    /// Seconds before it can fire again: the wait before its next pull, or what is left
    /// of the cycle's time between shots, whichever is longer.
    fn next_shot_in(&self) -> f32 {
        match self.cycle.phase {
            Phase::Cooldown { left } => self.wait.max(left),
            _ => self.wait,
        }
    }
}

/// The wait before a semi-automatic gun's next pull: a roll `u` in the weapon's
/// semi-automatic fire delay, each end times the combat style's multiplier, as the
/// combat AI's gunmen wait (`npc_combat`); 0 for an automatic weapon (#2763).
fn pull_wait(ws: &WeaponStats, mult: (f32, f32), u: f32) -> f32 {
    if ws.automatic {
        return 0.0;
    }
    let (lo, hi) = (ws.semi_auto_fire_delay[0] * mult.0, ws.semi_auto_fire_delay[1] * mult.1);
    (lo + (hi - lo).max(0.0) * u).max(0.0)
}

/// One actor carrying out a melee weapon use (module notes).
struct Swinger {
    seq: u64,
    /// Seconds before its next blow may begin (a person's volley pause included, as
    /// before #2535).
    strike_in: f32,
    /// A creature's volley pause, independent of its clip's keys [design:
    /// WeaponUse::shot volley pauses, #2535]; 0 for a person.
    pause_in: f32,
    /// Blows begun, with the seconds until each lands.
    blows: Vec<(f32, ActorStrike)>,
    /// Its last blow's swing, for a combo.
    swing: Option<Swing>,
    held: Option<&'static str>,
    /// All its blows are begun: the use ends once they have landed.
    spent: bool,
}

impl Swinger {
    /// Advance the actual clip and volley clocks, retaining Hit scheduling
    /// [design: swing, AttackTimes, #2535]. A creature's next blow waits for its clip
    /// (`SwingClip::ready_in`); a person's keeps the base wait, brought forward once by an
    /// earlier `a:` key when the clip arrives (`SwingClip::person_ready_in`; the
    /// maintainer's decision of 2026-11-18 on #2971).
    fn advance(&mut self, dt: f32, clip: Option<SwingClip>) {
        self.strike_in = (self.strike_in - dt).max(0.0);
        self.pause_in = (self.pause_in - dt).max(0.0);
        if let Some(sg) = self.swing.as_mut() {
            sg.t += dt;
            if let Some(clip) = clip.filter(|_| sg.clip.is_none()) {
                sg.clip = Some(clip);
                if let Some(b) = self.blows.last_mut() {
                    b.0 = (clip.hit - sg.t + dt).max(0.0);
                }
                if !sg.creature {
                    self.strike_in = clip.person_ready_in(sg.t, self.strike_in);
                }
            }
            if let Some(clip) = sg.clip.filter(|_| sg.creature) {
                self.strike_in = clip.ready_in(sg.t, self.strike_in, sg.speed);
            }
            if sg.t > sg.clip.map_or(STRIKE_SECONDS * 2.0, |c| c.end_at(sg.rate())) {
                self.swing = None;
            }
        }
    }

    /// The volley pause after a blow: its own clock for a creature (its clip's key must not
    /// cut it), within the base wait for a person (as before #2535).
    fn pause(&mut self, p: f32, creature: bool) {
        if creature {
            self.pause_in = p;
        } else {
            self.strike_in = self.strike_in.max(p);
        }
    }

    /// Both clocks must expire before starting a blow [design, #2535].
    fn ready(&self) -> bool {
        self.strike_in <= 0.0 && self.pause_in <= 0.0
    }
}

/// A use's last melee blow: seconds since it began, its clip once `attack_anims` started
/// it, and the blows of its combo so far.
#[derive(Clone, Copy)]
struct Swing {
    t: f32,
    clip: Option<SwingClip>,
    run: u32,
    /// The blow's attack speed (the weapon's Attack Mult): a creature's clip key and end
    /// are divided by it (`SwingClip::ready_in`, #2535 review); a person's clip keeps its
    /// own times.
    speed: f32,
    /// A creature's own attack (`Arm::Creature`): its next blow waits for its clip.
    creature: bool,
}

impl Swing {
    /// The clock its clip's times are divided by: the attack speed for a creature, 1 for
    /// a person (as `npc_combat::NpcSwing`).
    fn rate(&self) -> f32 {
        if self.creature { self.speed } else { 1.0 }
    }

    /// What a blow begun now goes on with: the group its clip's `a:` key named, between
    /// that key and the clip's end, both at the blow's rate (as `npc_combat`'s fighters;
    /// `SwingClip::next_at`).
    fn next(&self) -> Option<fnv_combat::melee::NextAttack> {
        self.clip?.next_at(self.t, self.rate())
    }
}

/// What a use is carried out with.
enum Arm {
    Gun(FormId),
    /// A melee or unarmed weapon (`None`: the Fists).
    Melee(Option<FormId>),
    /// A creature's own attack: its damage and reach.
    Creature(f32, f32),
}

#[derive(Resource, Default)]
pub struct Shooters {
    by: HashMap<u32, Shooter>,
    swingers: HashMap<u32, Swinger>,
    stats: Stats,
    rng: u64,
}

impl Shooters {
    /// Whether `actor`'s melee use `seq` has begun its last blow and that blow has yet to
    /// land: the use is done (`WeaponUse::shot`), but `shoot` goes on with it until then.
    pub fn landing(&self, actor: u32, seq: u64) -> bool {
        self.swingers.get(&actor).is_some_and(|x| x.spent && x.seq == seq)
    }
}

/// Whether [`shoot`] acts on the weapon use `w` this frame: the actor is in place
/// (`WeaponUse::in_place`) and its target has an aim point (`target_available`: drawn
/// actor/player in `places`, else a present reference resolved by [`target_place`]).
/// `npc_combat::fight` keeps its attacks off a package's use in a combat state only while
/// this holds (`npc_combat::weapon_use_holds_fight`), so one of the two always has an
/// actor in Combat, though either may still hold its fire (not facing, someone in the way)
/// (#2145 review) [design].
pub fn acts_on(w: &fnv_game::use_weapon::WeaponUse, target_available: bool) -> bool {
    w.in_place && target_available
}

#[derive(Default)]
struct Stats {
    actors: BTreeMap<u32, String>,
    from_scripts: usize,
    from_packages: usize,
    done: usize,
    shots: usize,
    sure: usize,
    harmless: usize,
    reloads: usize,
    crouched: usize,
    /// Times it held fire: someone in its line of fire, its target not where the use
    /// wants it, restrained.
    blocked: usize,
    target_away: usize,
    restrained: usize,
    /// Melee: blows begun, landed (harmless ones included), harmless, out of reach when
    /// they would have landed, and gone on a combo.
    blows: usize,
    landed: usize,
    harmless_blows: usize,
    harmless_blow_scripts: usize,
    missed: usize,
    chained: usize,
    /// The farthest off its heading a shooter had its target at a shot, radians.
    widest_shot: f32,
    /// Uses that went on with the cycle of the actor's last use (the same gun).
    went_on: usize,
    /// The shortest time between two shots of one actor's semi-automatic gun, seconds,
    /// with the gun and the actor.
    quickest: Option<(f32, String, String)>,
}

/// The bones a shot aims at, in order of preference: the target's chest as it is posed,
/// so a kneeling or seated target is aimed at where its body is, not at a standing
/// actor's chest height above it. The Silver Rush volunteer kneels restrained
/// (`DynamicIdle_RestrainedSit`) for Jean-Baptiste's Always Hit shot, which went over
/// him into the wall and left the scene waiting for its OnHit (#1550) [design].
const CHEST_BONES: [&str; 3] = ["Bip01 Spine1", "Bip01 Spine", "Bip01 Pelvis"];

type Shooting<'w> = (&'w ActorRig, &'w GlobalTransform, &'w InheritedVisibility, Option<&'w WeaponHold>, Has<Dying>, Has<Corpse>);

#[allow(clippy::too_many_arguments)]
pub(crate) fn shoot(
    time: Res<Time>,
    mut shooters: ResMut<Shooters>,
    combat: Res<Combat>,
    mut script: Option<ResMut<ScriptState>>,
    mut sim: ResMut<SimState>,
    player: Option<Res<Player>>,
    actors: Query<Shooting>,
    held: Query<&HeldWeapon>,
    muzzles: Query<(&Muzzle, &GlobalTransform)>,
    (mut fire, mut strike, mut anims): (MessageWriter<ActorFire>, MessageWriter<ActorStrike>, MessageWriter<PlayAttack>),
    mut times: ResMut<AttackTimes>,
    (blocks, phys): (Option<Res<ActorBlocks>>, Option<Res<PhysState>>),
    bones: Query<&GlobalTransform>,
) {
    let dt = time.delta_secs();
    let clock = time.elapsed_secs();
    let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else { return };
    if rt.game.menu.is_some() || rt.game.dialogue.is_some() {
        return;
    }
    let c = &*combat;
    let weapons: HashMap<u32, FormId> = held.iter().map(|h| (h.reference, h.weapon)).collect();
    let muzzle_at: HashMap<u32, V3> = muzzles.iter().map(|(m, g)| (m.reference, bevy_to_game(g.translation()).to_array())).collect();
    // Feet and eyes (game space) of everyone standing, and where each one's chest is as
    // posed now ([`CHEST_BONES`]).
    let mut places: HashMap<u32, (V3, V3)> = HashMap::new();
    let mut chests: HashMap<u32, V3> = HashMap::new();
    for (rig, g, vis, _, dying, corpse) in &actors {
        if vis.get() && !dying && !corpse {
            let feet = bevy_to_game(g.translation()).to_array();
            let scale = g.affine().transform_vector3(Vec3::Z).length().max(0.1);
            places.insert(rig.reference, (feet, [feet[0], feet[1], feet[2] + EYE_HEIGHT * scale]));
            let bone = CHEST_BONES.iter().find_map(|n| rig.skel.by_name.get(*n)).and_then(|&i| rig.nodes.get(i)).and_then(|&e| bones.get(e).ok());
            if let Some(b) = bone {
                chests.insert(rig.reference, bevy_to_game(b.translation()).to_array());
            }
        }
    }
    if let Some(p) = player.as_deref() {
        places.insert(FormId::PLAYER_REF.0, (p.feet, p.eye()));
    }
    // Share walking and standing body lookup with ordinary combat
    // [design: npc_combat::CombatBodies, #2535].
    let bodies = CombatBodies::new(phys.as_deref(), blocks.as_deref());
    let s = &mut *shooters;
    let mut still: Vec<u32> = Vec::new();
    with_world(rt, &mut sim, |rt| {
        let g = &mut rt.game;
        for (rig, global, _, hold, _, _) in &actors {
            let id = rig.reference;
            let me = FormId(id);
            if g.ai_off(me) {
                continue;
            }
            let Some(&(feet, eyes)) = places.get(&id) else { continue };
            // A melee use whose last blow is begun is done, but goes on here until that
            // blow has landed.
            let landing = |w: &fnv_game::use_weapon::WeaponUse| s.landing(id, w.seq);
            let Some(w) = g.active_weapon_use(me).or(g.weapon_use(me).filter(|w| landing(w))).filter(|w| w.in_place).cloned() else { continue };
            let name = g.data.name(me);
            let target_name = if w.target == FormId::PLAYER_REF { "the player".to_string() } else { g.data.name(w.target) };
            // Its target dead or gone: the use is over [design].
            if g.extra.get(&w.target).is_some_and(|e| e.dead) || !g.present(w.target) {
                info!("use weapon: {name} {me} stops: {target_name} is dead or gone ({} shots fired)", w.fired);
                g.end_weapon_use(me);
                s.stats.done += 1;
                continue;
            }
            // Nothing to fire at, or not there yet: the test `npc_combat::fight` keeps its
            // attacks off by ([`acts_on`]).
            let object_at = (!places.contains_key(&w.target.0)).then(|| target_place(g, me, w.target)).flatten();
            let target_at = places.get(&w.target.0).copied().or(object_at.map(|(feet, _)| (feet, feet)));
            if !acts_on(&w, target_at.is_some()) {
                continue;
            }
            let Some((tfeet, teyes)) = target_at else { continue };
            // What it uses: the weapon named, else the one drawn on it, else its fists or
            // a creature's own attack (module notes).
            let scale = global.affine().transform_vector3(Vec3::Z).length().max(0.1);
            let arm = match w.weapon.filter(|f| c.forms.weapons.contains_key(f)).or(weapons.get(&id).copied()) {
                Some(f) if c.forms.weapons.get(&f).is_some_and(|ws| is_gun(c, ws, f)) => Arm::Gun(f),
                Some(f) => Arm::Melee(Some(f)),
                None => match g.creature_attack(me) {
                    Some((damage, reach)) => Arm::Creature(damage as f32, creature_reach(reach, Some(&rig.skeleton), scale)),
                    None => Arm::Melee(None),
                },
            };
            let gun = match arm {
                Arm::Gun(gun) => gun,
                melee => {
                    still.push(id);
                    let x = MeleeCtx { c, me, name: &name, target_name: &target_name, global, hold, feet, eyes, tfeet, teyes, places: &places, dt, scale };
                    swing(g, s, &mut times, &x, &w, melee, &bodies, (&mut strike, &mut anims));
                    continue;
                }
            };
            let Some(ws) = c.forms.weapons.get(&gun) else { continue };
            still.push(id);
            let weapon_name = ws.edid.clone().unwrap_or_else(|| gun.to_string());
            let fresh = s.by.get(&id).is_none_or(|x| x.seq != w.seq || x.weapon != gun);
            if fresh {
                // The same gun as its last use: its cycle goes on (#2763). Else a new
                // one, timed as a combat gunman's (`npc_combat`).
                match s.by.get_mut(&id).filter(|x| x.weapon == gun) {
                    Some(x) => {
                        x.next_use(w.seq);
                        s.stats.went_on += 1;
                        info!(
                            "use weapon: {name} {me} goes on with its {weapon_name} from its last use: {:.2} s before its next shot, {} of {} rounds loaded",
                            x.next_shot_in(),
                            x.cycle.magazine,
                            x.cycle.clip_size
                        );
                    }
                    None => {
                        let rate = crate::npc_combat::teammate_attack_rate(g, me, Some(gun));
                        let timing = Timing::of(ws, &ClipTiming::default(), &c.tuning).with_attack_rate(rate);
                        s.by.insert(id, Shooter::new(w.seq, gun, ws, timing));
                    }
                }
                s.stats.actors.insert(id, name.clone());
                match w.source {
                    Source::Script => s.stats.from_scripts += 1,
                    Source::Package(_) => s.stats.from_packages += 1,
                }
                let shots = match w.volleys {
                    Some(v) if w.shots_per_volley[0] == w.shots_per_volley[1] => format!("{} shots", v * w.shots_per_volley[0]),
                    Some(v) => format!("{v} volleys of {} to {} shots", w.shots_per_volley[0], w.shots_per_volley[1]),
                    None => format!("volleys of {} to {} shots, on and on", w.shots_per_volley[0], w.shots_per_volley[1]),
                };
                let flags: Vec<&str> = [(w.always_hit, "always hit"), (w.no_damage, "no damage"), (w.crouch_to_reload, "crouch to reload"), (w.hold_fire_when_blocked, "hold fire when blocked")].into_iter().filter(|f| f.0).map(|f| f.1).collect();
                let from = match w.source {
                    Source::Script => "a script".to_string(),
                    Source::Package(p) => format!("its package {}", g.data.name(p)),
                };
                info!(
                    "use weapon: {name} {me} uses {weapon_name} on {target_name} ({from}): {shots}{}{}",
                    if flags.is_empty() { String::new() } else { format!("; {}", flags.join(", ")) },
                    if w.weapon.is_some_and(|x| x != gun) { "; not the weapon named, which is no gun" } else { "" }
                );
            }
            // The style's multipliers of the semi-automatic fire delay, as the fight reads
            // them.
            let delay_mult = g.combat_style(me).as_deref().map_or((1.0, 1.0), |st| st.semi_auto_delay_mult);
            let shooter = s.by.get_mut(&id).expect("inserted");
            shooter.idle = 0.0;
            shooter.wait = (shooter.wait - dt).max(0.0);
            // Its reload takes as long as its reload clips in its rig, once known, between
            // reloads, and a semi-automatic gun's shots follow its attack clip there: the
            // same handoff as `npc_combat`'s gunmen (`AttackTimes::gun_timing`, #2739), so
            // its cadence does not change between a fight and a use. The volley pauses and
            // the fire-delay wait stay on top (`wait`).
            if !shooter.reload_timed && matches!(shooter.cycle.phase, Phase::Ready | Phase::Cooldown { .. }) {
                if let Some((_, timing)) = times.gun_timing(id, gun, ws, &c.tuning, || crate::npc_combat::teammate_attack_rate(g, me, Some(gun))) {
                    shooter.cycle.timing = timing;
                    shooter.reload_timed = true;
                }
            }
            let tchest = object_at.map(|(_, aim)| aim).or_else(|| chests.get(&w.target.0).copied()).unwrap_or([tfeet[0], tfeet[1], tfeet[2] + (teyes[2] - tfeet[2]) * CHEST]);
            // What holds its fire this frame.
            let restrained = g.extra.get(&me).is_some_and(|e| e.restrained);
            let away = w.target_here.and_then(|r| fnv_ai::goal::place_of_ref(g, r)).is_some_and(|p| (p.pos[0] - tfeet[0]).hypot(p.pos[1] - tfeet[1]) > TARGET_HERE_RADIUS);
            let blocker = if w.hold_fire_when_blocked {
                places
                    .iter()
                    .filter(|(k, _)| **k != id && **k != w.target.0)
                    .find(|(_, p)| tactics::in_line_of_fire(eyes, tchest, [p.0[0], p.0[1], p.0[2] + (p.1[2] - p.0[2]) * CHEST], tactics::LINE_OF_FIRE_WIDTH))
                    .map(|(k, _)| *k)
            } else {
                None
            };
            let why = if restrained {
                Some("restrained")
            } else if away {
                Some("its target is not where the use wants it")
            } else if blocker.is_some() {
                Some("someone stands in its line of fire")
            } else {
                None
            };
            if why != shooter.held {
                if let Some(why) = why {
                    match why {
                        "restrained" => s.stats.restrained += 1,
                        "someone stands in its line of fire" => s.stats.blocked += 1,
                        _ => s.stats.target_away += 1,
                    }
                    info!("use weapon: {name} {me} holds fire: {why}");
                }
                shooter.held = why;
            }
            let drawn = hold.is_none_or(|h| h.anim.stage == Stage::Drawn);
            let off = facing_off(global, feet, tfeet);
            let arc = shot_facing(ws.aim_arc);
            let facing = off <= arc;
            let ready = why.is_none() && drawn && facing;
            // No pull while it reloads (a pull would close a round-by-round reload after a
            // round); an empty magazine is reloaded at once, as the combat AI's gunmen do.
            let reloading = shooter.cycle.reloading();
            let trigger = ready && !reloading && shooter.wait <= 0.0 && (ws.automatic || !shooter.pulled);
            shooter.pulled = trigger;
            let empty = shooter.cycle.magazine < shooter.cycle.ammo_use && shooter.cycle.regen.is_none();
            for e in shooter.cycle.step(&c.tuning, Input { trigger, reload: empty && !reloading, dt, carried: u32::MAX / 2 }) {
                match e {
                    Event::Fired { .. } => {
                        if !shooter.told {
                            shooter.told = true;
                            info!("use weapon: {name} {me} fires at {target_name} facing it: {:.1} degrees off its heading (it may be {:.0})", off.to_degrees(), arc.to_degrees());
                        }
                        s.stats.widest_shot = s.stats.widest_shot.max(off);
                        // A semi-automatic gun: how long after its last shot, this use's
                        // or the one before (#2763).
                        if let Some(gap) = shooter.last_shot.filter(|_| !ws.automatic).map(|t| clock - t) {
                            info!("use weapon: {name} {me} fires its {weapon_name} {gap:.2} s after its last shot (the gun's time between shots is {:.2} s)", shooter.cycle.timing.interval);
                            if s.stats.quickest.as_ref().is_none_or(|q| gap < q.0) {
                                s.stats.quickest = Some((gap, weapon_name.clone(), name.clone()));
                            }
                        }
                        shooter.last_shot = Some(clock);
                        let muzzle = muzzle_at.get(&id).copied();
                        let to = vec::sub(tchest, eyes);
                        let aim = vec::scale(to, 1.0 / vec::len(to).max(1.0));
                        let origin = vec::add(eyes, vec::scale(aim, MUZZLE_AHEAD));
                        let cone = if w.always_hit {
                            0.0
                        } else {
                            let skill = ws.skill.as_deref().and_then(fnv_game::functions::av_by_name).map_or(50.0, |code| fnv_game::functions::actor_value_capped(g, me, code)) as f32;
                            let strength = fnv_game::functions::actor_value_capped(g, me, fnv_game::actor_values::STRENGTH) as f32;
                            let stance = Stance { aiming: false, moving: false, running: false, sneaking: false, airborne: false, skill, strength, aiming_move_divisor: 1.0, ..Stance::default() };
                            spread::cone_deg(spread::actor_spread(&c.rules, &c.tuning, ws, &stance), 0.0, ws, &[])
                        };
                        fire.write(ActorFire { shooter: me, weapon: gun, origin, aim, cone, muzzle, harmless: w.no_damage, sure: w.always_hit.then_some(w.target) });
                        anims.write(PlayAttack { actor: id, weapon: Some(gun), melee: false, sights: false, kind: Default::default(), power: None, swing: None });
                        s.stats.shots += 1;
                        s.stats.sure += usize::from(w.always_hit);
                        s.stats.harmless += usize::from(w.no_damage);
                        let roll = next_unit(&mut s.rng);
                        // The gun's own delay before its next pull, whatever the use does
                        // next: more shots, a pause, or its end and another use (#2763).
                        let delay = pull_wait(ws, delay_mult, next_unit(&mut s.rng));
                        let Some(u) = g.weapon_uses.get_mut(&me) else { break };
                        match u.shot(roll) {
                            After::More => shooter.wait = delay,
                            After::Pause(p) => {
                                shooter.wait = p.max(delay);
                                // A volley is over: the trigger is let go.
                                shooter.pulled = true;
                            }
                            After::Done => {
                                shooter.wait = delay;
                                info!("use weapon: {name} {me} has fired its {} shots at {target_name}", u.fired);
                                g.end_weapon_use(me);
                                s.stats.done += 1;
                                break;
                            }
                        }
                    }
                    Event::ReloadStarted => {
                        s.stats.reloads += 1;
                        if let Some(u) = g.weapon_uses.get_mut(&me).filter(|u| u.crouch_to_reload) {
                            u.crouched = true;
                            s.stats.crouched += 1;
                        }
                    }
                    Event::Reloaded { .. } => {
                        if let Some(u) = g.weapon_uses.get_mut(&me) {
                            u.crouched = false;
                        }
                    }
                    _ => {}
                }
            }
        }
        // Those no longer using a weapon stand up; a gun's cycle is kept a while for
        // the actor's next use ([`KEEP_IDLE`]).
        s.by.retain(|id, x| still.contains(id) || x.rest(&c.tuning, dt));
        s.swingers.retain(|id, _| still.contains(id));
        for (a, u) in g.weapon_uses.iter_mut() {
            if !still.contains(&a.0) {
                u.crouched = false;
            }
        }
    });
}

/// What [`swing`] reads of the actor and its target this frame.
struct MeleeCtx<'a> {
    c: &'a Combat,
    me: FormId,
    name: &'a str,
    target_name: &'a str,
    global: &'a GlobalTransform,
    hold: Option<&'a WeaponHold>,
    feet: V3,
    eyes: V3,
    tfeet: V3,
    teyes: V3,
    places: &'a HashMap<u32, (V3, V3)>,
    dt: f32,
    scale: f32,
}

/// Scripted melee uses the same body gap and radii as ordinary combat, even
/// when a widened creature stops or resumes walking [design: CombatBodies, #2535].
fn melee_range(bodies: &CombatBodies, me: u32, target: u32, feet: V3, tfeet: V3, creature: bool, reach: f32) -> (f32, f32) {
    let away = (feet[0] - tfeet[0]).hypot(feet[1] - tfeet[1]);
    let room = bodies.gap(me, target, tfeet).zip(bodies.radii(me, target));
    match (creature, room) {
        (true, room) => creature_spacing(room.map(|(gap, _)| away - gap), reach),
        (false, Some((gap, radii))) => melee_spacing(away - gap, radii, reach),
        (false, None) => engage(Some(reach), None),
    }
}

/// A melee weapon use this frame (module notes): its reach for the AI, its blows
/// landing, and the next blow when it may strike.
#[allow(clippy::too_many_arguments)]
fn swing(
    g: &mut fnv_game::game::Game<'_>,
    s: &mut Shooters,
    times: &mut AttackTimes,
    x: &MeleeCtx,
    w: &fnv_game::use_weapon::WeaponUse,
    arm: Arm,
    bodies: &CombatBodies,
    (strike, anims): (&mut MessageWriter<ActorStrike>, &mut MessageWriter<PlayAttack>),
) {
    let (c, me, id) = (x.c, x.me, x.me.0);
    let (weapon, creature_damage, reach) = match arm {
        Arm::Melee(wp) => (wp, None, wp.or(c.fists_id()).and_then(|f| c.forms.weapons.get(&f)).map_or(CREATURE_REACH, |ws| fnv_combat::damage::melee_reach(&c.rules, ws, x.scale))),
        Arm::Creature(d, r) => (None, Some(d), r),
        Arm::Gun(_) => return,
    };
    let away = (x.feet[0] - x.tfeet[0]).hypot(x.feet[1] - x.tfeet[1]);
    let (stop, within) = melee_range(bodies, id, w.target.0, x.feet, x.tfeet, creature_damage.is_some(), reach);
    if let Some(u) = g.weapon_uses.get_mut(&me) {
        u.reach = Some([stop, within]);
    }
    let near = away <= within;
    if s.swingers.get(&id).is_none_or(|x| x.seq != w.seq) {
        s.swingers.insert(id, Swinger { seq: w.seq, strike_in: 0.0, pause_in: 0.0, blows: Vec::new(), swing: None, held: None, spent: false });
        s.stats.actors.insert(id, x.name.to_string());
        match w.source {
            Source::Script => s.stats.from_scripts += 1,
            Source::Package(_) => s.stats.from_packages += 1,
        }
        let blows = match w.volleys {
            Some(v) if w.shots_per_volley[0] == w.shots_per_volley[1] => format!("{} blows", v * w.shots_per_volley[0]),
            Some(v) => format!("{v} volleys of {} to {} blows", w.shots_per_volley[0], w.shots_per_volley[1]),
            None => format!("volleys of {} to {} blows, on and on", w.shots_per_volley[0], w.shots_per_volley[1]),
        };
        let flags: Vec<&str> = [(w.always_hit, "always hit"), (w.no_damage, "no damage"), (w.hold_fire_when_blocked, "hold fire when blocked")].into_iter().filter(|f| f.0).map(|f| f.1).collect();
        let from = match w.source {
            Source::Script => "a script".to_string(),
            Source::Package(p) => format!("its package {}", g.data.name(p)),
        };
        let with = match (weapon, creature_damage) {
            (Some(f), _) => c.forms.weapons.get(&f).and_then(|ws| ws.edid.clone()).unwrap_or_else(|| f.to_string()),
            (None, Some(_)) => "its own attack".to_string(),
            (None, None) => "its fists".to_string(),
        };
        let flags = if flags.is_empty() { String::new() } else { format!("; {}", flags.join(", ")) };
        info!("use weapon: {} {me} uses {with} (melee, reach {within:.0}) on {} ({from}): {blows}{flags}", x.name, x.target_name);
    }
    let sw = s.swingers.get_mut(&id).expect("inserted");
    sw.advance(x.dt, times.swings.remove(&id));
    let tchest = [x.tfeet[0], x.tfeet[1], x.tfeet[2] + (x.teyes[2] - x.tfeet[2]) * CHEST];
    // Blows whose clip reached its `Hit` key land: within reach, or always with Always
    // Hit.
    for b in &mut sw.blows {
        b.0 -= x.dt;
    }
    let (due, wait): (Vec<_>, Vec<_>) = std::mem::take(&mut sw.blows).into_iter().partition(|b| b.0 <= 0.0);
    sw.blows = wait;
    for (_, b) in due {
        if !(near || w.always_hit) {
            s.stats.missed += 1;
            info!("use weapon: {}'s blow misses {}: out of reach ({away:.0} units)", x.name, x.target_name);
        } else if w.no_damage {
            s.stats.landed += 1;
            s.stats.harmless_blows += 1;
            // Its OnHit and OnHitWith blocks run, as for a harmless shot (#2433).
            let info = fnv_game::combat::HitInfo { attacker: Some(me), weapon, location: -1, ..Default::default() };
            let ran = g.harmless_hit(b.target, info);
            s.stats.harmless_blow_scripts += usize::from(ran);
            info!("use weapon: {}'s harmless blow lands on {}{}", x.name, x.target_name, if ran { ": its hit blocks ran" } else { "" });
        } else {
            s.stats.landed += 1;
            strike.write(ActorStrike { point: tchest, ..b });
        }
    }
    if sw.spent {
        if sw.blows.is_empty() {
            info!("use weapon: {} {me} has dealt its {} blows at {}", x.name, w.fired, x.target_name);
            s.swingers.remove(&id);
            g.end_weapon_use(me);
            s.stats.done += 1;
        }
        return;
    }
    // What holds its swings this frame (as it holds shots).
    let restrained = g.extra.get(&me).is_some_and(|e| e.restrained);
    let target_away = w.target_here.and_then(|r| fnv_ai::goal::place_of_ref(g, r)).is_some_and(|p| (p.pos[0] - x.tfeet[0]).hypot(p.pos[1] - x.tfeet[1]) > TARGET_HERE_RADIUS);
    let blocked = w.hold_fire_when_blocked
        && x.places
            .iter()
            .filter(|(k, _)| **k != id && **k != w.target.0)
            .any(|(_, p)| tactics::in_line_of_fire(x.eyes, tchest, [p.0[0], p.0[1], p.0[2] + (p.1[2] - p.0[2]) * CHEST], tactics::LINE_OF_FIRE_WIDTH));
    let why = if restrained {
        Some("restrained")
    } else if target_away {
        Some("its target is not where the use wants it")
    } else if blocked {
        Some("someone stands in its line of fire")
    } else {
        None
    };
    if why != sw.held {
        if let Some(why) = why {
            match why {
                "restrained" => s.stats.restrained += 1,
                "someone stands in its line of fire" => s.stats.blocked += 1,
                _ => s.stats.target_away += 1,
            }
            info!("use weapon: {} {me} holds its swings: {why}", x.name);
        }
        sw.held = why;
    }
    let drawn = x.hold.is_none_or(|h| h.anim.stage == Stage::Drawn);
    let facing = facing_off(x.global, x.feet, x.tfeet) <= USE_FACING;
    if why.is_some() || !drawn || !facing || !near || !sw.ready() || (x.feet[2] - x.tfeet[2]).abs() >= 128.0 {
        return;
    }
    // A blow: on a combo when the last clip's `a:` key has passed.
    let next = sw.swing.and_then(|sg| sg.next());
    let run = next.map_or(1, |_| sw.swing.map_or(0, |sg| sg.run) + 1);
    if next.is_some() {
        s.stats.chained += 1;
    }
    times.swings.remove(&id);
    let speed = weapon.or(c.fists_id()).and_then(|f| c.forms.weapons.get(&f)).map_or(1.0, |ws| ws.attack_mult.max(0.25));
    sw.swing = Some(Swing { t: 0.0, clip: None, run, speed, creature: creature_damage.is_some() });
    let delay = times.hit.get(&id).copied().unwrap_or(DEFAULT_HIT);
    let dir = vec::normalize(vec::sub(tchest, x.eyes));
    sw.blows.push((delay, ActorStrike { attacker: me, target: w.target, weapon, creature_damage, point: tchest, dir, anim: None }));
    let pick = SwingPick { next, rolls: [next_unit(&mut s.rng), next_unit(&mut s.rng)] };
    anims.write(PlayAttack { actor: id, weapon, melee: true, sights: false, kind: Move::Attack, power: None, swing: Some(pick) });
    s.stats.blows += 1;
    sw.strike_in = STRIKE_SECONDS / speed;
    let roll = next_unit(&mut s.rng);
    let Some(u) = g.weapon_uses.get_mut(&me) else { return };
    let after = u.shot(roll);
    let combo = next.map_or(String::new(), |n| format!(", on its combo with {n:?}"));
    info!("use weapon: {} {me} swings at {} from {away:.0} units (blow {}{combo})", x.name, x.target_name, u.fired);
    match after {
        After::More => {}
        After::Pause(p) => sw.pause(p, creature_damage.is_some()),
        After::Done => sw.spent = true,
    }
}

fn report(mut exit: MessageReader<AppExit>, shooters: Res<Shooters>, stats: Option<Res<CombatStats>>) {
    if exit.read().next().is_none() {
        return;
    }
    let s = &shooters.stats;
    let (harmless_hits, harmless_scripts) = stats.map_or((0, 0), |c| (c.harmless_hits, c.harmless_scripts));
    info!(
        "use weapon summary: {} actors used a weapon ({}); {} uses ({} from scripts, {} from packages), {} over; {} shots ({} always hit, {} harmless; {harmless_hits} harmless hits), {} reloads ({} crouched); held fire: {} line of fire, {} target away, {} restrained; {} blows ({} landed, {} harmless, {} out of reach, {} went on a combo)",
        s.actors.len(),
        s.actors.values().cloned().collect::<Vec<_>>().join(", "),
        s.from_scripts + s.from_packages,
        s.from_scripts,
        s.from_packages,
        s.done,
        s.shots,
        s.sure,
        s.harmless,
        s.reloads,
        s.crouched,
        s.blocked,
        s.target_away,
        s.restrained,
        s.blows,
        s.landed,
        s.harmless_blows,
        s.missed,
        s.chained,
    );
    info!("use weapon summary: harmless hits whose target's hit blocks ran: {harmless_scripts} shots, {} blows", s.harmless_blow_scripts);
    info!("use weapon summary: shots fired with the target at most {:.1} degrees off the shooter's heading", s.widest_shot.to_degrees());
    match &s.quickest {
        Some((gap, gun, who)) => info!("use weapon summary: {} uses went on with the gun's cycle from the actor's last use; the shortest time between two shots of a semi-automatic gun: {gap:.2} s ({gun}, {who})", s.went_on),
        None => info!("use weapon summary: {} uses went on with the gun's cycle from the actor's last use; no semi-automatic gun fired twice", s.went_on),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #2725: a use's shooter fires within the combat gunmen's arc, not the 30° its blows
    /// keep: 3°, or the weapon's own Aim Arc.
    #[test]
    fn a_use_fires_only_within_the_gunmens_arc() {
        use super::{shot_facing, USE_FACING};
        assert_eq!(shot_facing(0.0), crate::npc_combat::FIRE_FACING);
        assert!((shot_facing(0.0).to_degrees() - 3.0).abs() < 1e-4);
        assert!(shot_facing(0.0) < USE_FACING);
        assert!((shot_facing(45.0).to_degrees() - 45.0).abs() < 1e-4, "a weapon's Aim Arc is its own");
    }
    use fnv_game::use_weapon::{Source, WeaponUse};
    use fnv_sim::FormId;

    /// #2763: a second use with the same gun goes on with the first one's cycle: the
    /// time before the next shot and the magazine, not a fresh gun ready at once.
    #[test]
    fn a_second_use_keeps_the_guns_cycle() {
        use super::{pull_wait, Shooter, KEEP_IDLE};
        use fnv_combat::attack::{ClipTiming, Event, Input, Timing};
        use fnv_combat::data::WeaponStats;
        use fnv_combat::tuning::CombatTuning;
        // The hunting revolver's numbers: 1.5 shots a second, five rounds.
        let ws = WeaponStats { attack_shots_per_sec: 1.5, clip_size: 5, ammo_use: 1, attack_mult: 1.0, anim_mult: 1.0, semi_auto_fire_delay: [0.0, 0.3], ..WeaponStats::default() };
        let t = CombatTuning::default();
        let timing = Timing::of(&ws, &ClipTiming::default(), &t);
        assert!((timing.interval - 1.0 / 1.5).abs() < 1e-4);
        let mut sh = Shooter::new(1, FormId(0x10), &ws, timing);
        let pull = |sh: &mut Shooter, trigger: bool, dt: f32| sh.cycle.step(&t, Input { trigger, reload: false, dt, carried: 100 }).into_iter().filter(|e| matches!(e, Event::Fired { .. })).count();
        assert_eq!(pull(&mut sh, true, 0.016), 1, "the first use's shot");
        assert_eq!(sh.cycle.magazine, 4);
        // The use is over; a frame later the next one begins.
        assert!(sh.rest(&t, 0.016), "kept between uses");
        sh.next_use(2);
        assert_eq!(sh.seq, 2);
        assert_eq!(sh.cycle.magazine, 4, "the magazine is the same gun's");
        assert!(sh.next_shot_in() > 0.6, "the time between shots still runs: {}", sh.next_shot_in());
        assert_eq!(pull(&mut sh, true, 0.016), 0, "no shot at once at the next target");
        let mut waited = 0.032;
        let mut fired = 0;
        while fired == 0 && waited < 2.0 {
            fired = pull(&mut sh, false, 0.016) + pull(&mut sh, true, 0.016);
            waited += 0.032;
        }
        assert_eq!(fired, 1);
        assert!(waited >= 1.0 / 1.5, "the second use's shot comes the gun's interval after the first: {waited}");
        assert_eq!(sh.cycle.magazine, 3);
        // Long without a use, it is let go.
        assert!(!sh.rest(&t, KEEP_IDLE));
        // The delay before the next pull takes the style's multipliers.
        assert_eq!(pull_wait(&ws, (1.0, 1.0), 0.5), 0.15);
        assert_eq!(pull_wait(&ws, (1.0, 2.0), 1.0), 0.6);
        assert_eq!(pull_wait(&WeaponStats { automatic: true, ..ws.clone() }, (1.0, 1.0), 0.5), 0.0);
    }

    #[test]
    fn scripted_knife_reaches_a_creature_that_walks_stops_and_resumes() {
        use nvf_play::blocking::{clip_pushing_nobody, Block};
        use nvf_play::phys::NpcBody;
        // Synthetic pair/weapon [design, #2535]; widths and scales supplied in
        // #2535 [observed: FalloutNV.esm and Fallout - Meshes.bsa, fnvtool esm
        // decode / nif decode, main session 2026-11-15].
        let (person, creature) = (123, 456);
        let rules = fnv_combat::tuning::Rules { combat_distance: 107.0, ..fnv_combat::tuning::Rules::defaults() };
        let weapon = fnv_combat::data::WeaponStats { reach: 0.5, ..Default::default() };
        let reach = fnv_combat::damage::melee_reach(&rules, &weapon, 1.0);
        assert_eq!(reach, 53.5);
        for scale in [0.6, 1.0] {
            let mut phys = PhysState::default();
            let width = 70.56 * scale;
            let combined = 22.0 + width;
            let height = fnv_phys::CharacterParams::default().height;
            phys.actor_block_radii.insert(creature, width);
            phys.actor_heights.insert(creature, height);
            let mut blocks = ActorBlocks::default();
            for (step, standing) in [false, true, false].into_iter().enumerate() {
                // Move both roots on every transition [design: synthetic fixture].
                let offset = step as f32 * 20.0;
                let start = [offset, 0.0, 0.0];
                let tfeet = [offset + 200.0, 0.0, 0.0];
                let block = Block::character(creature, tfeet, width, height, false);
                let v = clip_pushing_nobody(person, [240.0, 0.0], start, 22.0, height, 1.0, &mut [block]);
                let feet = [start[0] + v[0], 0.0, 0.0];
                let away = tfeet[0] - feet[0];
                assert!((away - combined).abs() < 1e-4);
                phys.remove_npc(person);
                phys.remove_npc(creature);
                blocks.list.clear();
                for (id, at) in [(person, feet), (creature, tfeet)] {
                    if id == creature && standing {
                        blocks.list.push(block);
                    } else {
                        let ch = phys.world.add_character(id, fnv_phys::CharacterParams::default(), at);
                        let standing_height = ch.params.height;
                        phys.npcs.insert(id, NpcBody { ch, prev_feet: at, velocity: [25.0, 0.0], jump: false, sink: 0.0, lift: Default::default(), standing_height, sneak_crouch: false });
                    }
                }
                let bodies = CombatBodies::new(Some(&phys), Some(&blocks));
                assert!(bodies.gap(person, creature, tfeet).unwrap().abs() < 1e-4);
                assert!((bodies.radii(person, creature).unwrap() - combined).abs() < 1e-4);
                let (stop, within) = melee_range(&bodies, person, creature, feet, tfeet, false, reach);
                assert!((within - (combined + reach)).abs() < 1e-4);
                assert!(away <= stop && stop < within, "standing={standing}");
                assert!(away <= within, "scripted blow must start and land at the blocking stop");
                for (distance, near) in [(within - 0.01, true), (within + 0.01, false)] {
                    let moved_feet = [tfeet[0] - distance, 0.0, 0.0];
                    phys.npcs.get_mut(&person).unwrap().ch.feet = moved_feet;
                    let bodies = CombatBodies::new(Some(&phys), Some(&blocks));
                    let within = melee_range(&bodies, person, creature, moved_feet, tfeet, false, reach).1;
                    assert_eq!(distance <= within, near);
                }
                let creature_reach = creature_reach(Some(64.0), Some(r"Creatures\Radscorpion\Skeleton.nif"), scale);
                assert!(away <= melee_range(&bodies, creature, person, tfeet, feet, true, creature_reach).1);
                assert_eq!(phys.npcs[&person].ch.params.radius, 22.0);
            }
        }
    }

    #[test]
    fn scripted_melee_without_a_clip_keeps_upstream_timer_and_volley_pauses() {
        // Compare the production gate with upstream 63db0209c's single clock:
        // max(base interval, volley pause), then decrement [design, #2535]. People and
        // creatures alike.
        for creature in [false, true] {
            for speed in [0.25, 0.5, 1.0, 2.0, 4.0] {
                for pause in [0.0_f32, 0.4, 1.2, 3.0, 6.0] {
                    let base = STRIKE_SECONDS / speed;
                    for dt in [0.01, 0.125, 0.7] {
                        let mut old = base.max(pause);
                        let mut sw = Swinger {
                            seq: 1, strike_in: base, pause_in: 0.0, blows: Vec::new(),
                            swing: Some(Swing { t: 0.0, clip: None, run: 1, speed, creature }), held: None, spent: false,
                        };
                        sw.pause(pause, creature);
                        // Include frames after the no-clip swing's expiry [design].
                        for _ in 0..(8.0 / dt) as usize {
                            old = (old - dt).max(0.0);
                            sw.advance(dt, None);
                            assert_eq!(sw.ready(), old <= 0.0, "creature={creature}, speed={speed}, pause={pause}, dt={dt}");
                            assert_eq!(sw.strike_in.max(sw.pause_in), old);
                            assert!(sw.swing.is_none_or(|s| s.clip.is_none() && s.next().is_none()));
                        }
                    }
                }
            }
        }
    }

    /// The maintainer's decision of 2026-11-18 on #2971: a person's scripted blows keep
    /// upstream's timing with a clip too, bit for bit: the base wait over the attack speed
    /// (with the volley pause in it), brought forward once to an earlier `a:` key when the
    /// clip arrives; the combo window and the swing's end at the clip's own times.
    #[test]
    fn scripted_person_melee_keeps_upstream_timing_with_clips() {
        use fnv_combat::melee::NextAttack;
        // Upstream's clock (main before #2535), restated for the comparison [design].
        #[derive(Clone, Copy)]
        struct Upstream {
            strike_in: f32,
            t: f32,
            clip: Option<SwingClip>,
            live: bool,
        }
        impl Upstream {
            fn advance(&mut self, dt: f32, clip: Option<SwingClip>) {
                self.strike_in = (self.strike_in - dt).max(0.0);
                if self.live {
                    self.t += dt;
                    if let Some(c) = clip.filter(|_| self.clip.is_none()) {
                        self.clip = Some(c);
                        if let Some((at, _)) = c.chain {
                            self.strike_in = self.strike_in.min((at - self.t).max(0.0));
                        }
                    }
                    if self.t > self.clip.map_or(STRIKE_SECONDS * 2.0, |c| c.len) {
                        self.live = false;
                    }
                }
            }
            fn next(&self) -> Option<NextAttack> {
                let c = self.clip.filter(|_| self.live)?;
                c.chain.filter(|(at, _)| self.t >= *at && self.t <= c.len).map(|(_, n)| n)
            }
        }
        // Synthetic clips: late, early and no keys [design].
        for (key, len) in [(Some(1.3), 1.5), (Some(1.55), 1.7), (Some(0.6), 1.5), (Some(0.63), 1.43), (None, 1.5), (None, 0.8)] {
            let clip = SwingClip { hit: 0.4, chain: key.map(|at| (at, NextAttack::Left)), len };
            for speed in [0.25, 0.5, 1.0, 2.0, 4.0] {
                for pause in [0.0_f32, 0.4, 2.0] {
                    for (arrival, dt) in [(1, 0.01), (2, 0.0166), (5, 0.05), (1, 0.125)] {
                        let base = STRIKE_SECONDS / speed;
                        let mut up = Upstream { strike_in: base.max(pause), t: 0.0, clip: None, live: true };
                        let mut sw = Swinger {
                            seq: 1, strike_in: base, pause_in: 0.0, blows: Vec::new(),
                            swing: Some(Swing { t: 0.0, clip: None, run: 1, speed, creature: false }), held: None, spent: false,
                        };
                        sw.pause(pause, false);
                        for frame in 0..(6.0 / dt) as usize {
                            let published = (frame + 1 == arrival).then_some(clip);
                            up.advance(dt, published);
                            sw.advance(dt, published);
                            let at = format!("key={key:?}, len={len}, x{speed}, pause={pause}, dt={dt}, frame={frame}");
                            assert_eq!(sw.strike_in, up.strike_in, "{at}");
                            assert_eq!(sw.pause_in, 0.0, "{at}");
                            assert_eq!(sw.ready(), up.strike_in <= 0.0, "{at}");
                            assert_eq!(sw.swing.is_some(), up.live, "{at}");
                            assert_eq!(sw.swing.and_then(|s| s.next()), up.next(), "{at}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn scripted_melee_waits_for_clip_boundaries_and_volley_pauses() {
        use fnv_combat::melee::NextAttack;
        // A creature's scripted blows (#2971: creatures only). Synthetic clips, clocks and
        // pauses [design, #2535], the clips published at their own speed as `attack_anims`
        // publishes them; the swing carries the attack speed. Exercise the same
        // advancement and readiness gate used by swing, including late timing.
        for (key, len, earliest) in [(Some(1.267), 1.5, 1.267), (Some(1.3), 1.5, 1.3), (None, 1.5, 1.5), (None, 0.8, STRIKE_SECONDS), (Some(0.6), 1.5, 0.6)] {
            for speed in [0.5, 1.0, 2.0] {
                let clip = SwingClip { hit: 0.4, chain: key.map(|at| (at, NextAttack::Left)), len };
                for pause in [0.0_f32, 0.4, 2.0] {
                    for arrival in [0.02, 0.1] {
                        let mut sw = Swinger {
                            seq: 1, strike_in: STRIKE_SECONDS / speed, pause_in: 0.0,
                            blows: Vec::new(), swing: Some(Swing { t: 0.0, clip: None, run: 1, speed, creature: true }), held: None, spent: false,
                        };
                        sw.pause(pause / speed, true);
                        let expected = earliest.max(pause) / speed;
                        sw.advance(arrival / 2.0, None);
                        assert!(!sw.ready());
                        sw.advance(arrival / 2.0, Some(clip));
                        assert!((sw.strike_in - (earliest / speed - arrival)).abs() < 1e-5);
                        let mut elapsed = arrival;
                        // Small frames cross clip end while a longer pause remains
                        // [design: synthetic clock].
                        while elapsed < expected - 0.01 {
                            let dt = 0.01_f32.min(expected - 0.01 - elapsed);
                            sw.advance(dt, None);
                            elapsed += dt;
                            assert!(!sw.ready(), "key={key:?}, len={len}, speed={speed}, pause={pause}, t={elapsed}");
                        }
                        sw.advance(0.02, None);
                        elapsed += 0.02;
                        assert!(sw.ready(), "key={key:?}, len={len}, speed={speed}, pause={pause}");
                        // The combo window and the swing's end are at the attack speed, as
                        // the decision is [design: SwingClip::next_at, end_at]: a decision at
                        // the key goes on the combo at every speed.
                        match sw.swing {
                            Some(s) => {
                                assert!(elapsed <= len / speed, "key={key:?}, len={len}, speed={speed}, pause={pause}");
                                assert_eq!(s.next(), key.filter(|&at| elapsed >= at / speed).map(|_| NextAttack::Left));
                                if pause == 0.0 {
                                    assert_eq!(s.next(), key.map(|_| NextAttack::Left), "key={key:?}, len={len}, speed={speed}: chains");
                                }
                            }
                            None => assert!(elapsed > len / speed, "key={key:?}, len={len}, speed={speed}, pause={pause}"),
                        }
                    }
                }
            }
        }
    }

    /// [design, #2652] Eligibility needs an aim point, including placed objects/markers;
    /// an actor still walking to its stand leaves the fight in charge (#2145).
    #[test]
    fn the_shooter_acts_only_in_place_on_an_available_target() {
        let walking = WeaponUse { source: Source::Package(FormId(0x9)), ..WeaponUse::of_script(None, None, FormId(0x7), 3, true, true, false, false, None) };
        let there = WeaponUse { in_place: true, ..walking.clone() };
        assert!(acts_on(&there, true), "in place with a drawn or placed target");
        assert!(!acts_on(&there, false), "no target position");
        assert!(!acts_on(&walking, true), "still on its way to its stand");
        assert!(!acts_on(&walking, false));
    }
}

#[cfg(test)]
mod critical_damage_hit_tests;
