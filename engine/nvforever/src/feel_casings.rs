//! Modern Feel › Shell casings (docs/design/modern-feel-vfx.md): spent cases thrown from
//! the guns as physical bodies that tumble, bounce, clink and roll, then fade
//! [design: NVForever's own; every constant here an engineering choice tuned by eye].
//! Gated by `ModernFeel::shell_casings()`; while it is on, `shot_fx`'s own casings (the
//! original's: drawn only, a point bouncing off the world) are not thrown.
//!
//! - **Which shots throw one** ([`Eject`]): a gun (skill Guns, or Explosives for the
//!   grenade launchers) that is not thrown, placed, swung or an energy weapon:
//!   - a weapon with a casing model (WEAP) throws one each shot, the player's at the
//!     attack clip's `Eject` key where it has one (lever, bolt and pump actions: combat's
//!     `CasingFx`), other actors' at the shot;
//!   - a break action (not automatic, a clip of 1 or 2: the single, caravan and sawed-off
//!     shotguns) and a revolver (a pistol with no casing model and at most 6 rounds) keep
//!     their cases until the player reloads: then the shots fired since the last reload
//!     fall out of the gun together;
//!   - a self-loading gun with no casing model (the silenced .22 pistol) throws the
//!     small pistol case each shot.
//!   Energy weapons with a casing model (the laser rifle's and the gatling laser's
//!   `AssaultRifleShellCasing.NIF`, left from the earlier game) throw none.
//! - **Model**: the WEAP's casing model; for the reload drops the case model named
//!   by the weapon's calibre (.357, .44, 45-70, the shotgun hull), else the 10mm/.45
//!   case [design: by names; no record links a revolver to a case model].
//! - **From**: the player's from the weapon's `ShellCasingNode` as drawn
//!   (`PlayerBody::casing_entity`: in first person the arms rig is drawn widened about the
//!   eye, `player_body::place_lens`, so the node's drawn place is where the gun is seen;
//!   the case starts there and leaves the drawn gun); without one, from beside the
//!   breech, behind the muzzle. Other actors' from their weapon's `ShellCasingNode`
//!   ([`CasingPort`], under its muzzle marker), else behind the muzzle; only within
//!   [`NPC_RANGE`] of the player.
//! - **Throw**: along the node's +z (the way the original throws its cases: combat-rules.md
//!   §16) at `fGunShellEjectSpeed` ± [`SPEED_SPREAD`], turned up to [`CONE_DEG`] at random,
//!   lifted [`LIFT`] and pushed back [`BACK`] (against the shot), plus the player's own
//!   motion; tumbling end over end and spinning about its length.
//! - **Physics**: each case is a small rapier cylinder sized from its model
//!   (`fnv_phys::casings`): it meets the world and other cases, never actors, and no
//!   query sees it.
//! - **Sound**: a bounce whose speed changes by [`KNOCK_DV`] or more plays the casing's
//!   PHY sound (`shot_fx::casing_sound`: `PHYCasingPistol`, `PHYCasingRifle`,
//!   `PHYCasingShotgun`) where it struck, louder for a harder knock, up to
//!   [`KNOCKS`] times a case.
//! - **Cap and fade**: at most [`MAX_LIVE`] cases; the oldest fades first. A case lies
//!   [`LIFE`] seconds (a random time between) and shrinks away over [`FADE`] s.
//!
//! Log: `shell casings: <weapon> throws <model> …` once per weapon and way, and at exit
//! `shell casings summary: …`.

use bevy::light::NotShadowCaster;
use bevy::math::Affine3A;
use bevy::prelude::*;
use fnv_combat::rng::SplitMix;
use fnv_phys::casings::CasingStart;
use fnv_sim::FormId;
use nvf_assets::model::{ModelCtx, PreparedModel};
use nvf_play::combat::{CasingFx, Combat, ShotFired, WeaponAnim};
use nvf_play::modern_feel::ModernFeel;
use nvf_play::phys::PhysState;
use nvf_play::player::Player;
use nvf_play::sfx::{ScaledSound, Sfx};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use crate::player_body::PlayerBody;
use crate::weapons::Muzzle;
use crate::world::WorldRoot;

/// Where a weapon drawn on an actor throws its cases: its model's `ShellCasingNode`,
/// placed under the actor's muzzle marker (`weapons::spawn_muzzle`).
#[derive(Component, Clone, Copy, Debug)]
pub struct CasingPort {
    pub reference: u32,
}

/// Cases in the world at once, at most.
pub const MAX_LIVE: usize = 48;
/// How long a case lies, seconds (a random time between), and how long it takes to go.
pub const LIFE: (f32, f32) = (8.0, 15.0);
pub const FADE: f32 = 0.8;
/// A case thrown by another actor farther than this from the player is not thrown,
/// game units (about 30 m).
pub const NPC_RANGE: f32 = 2100.0;
/// The throw: the eject speed's spread (a share), the cone its way is turned in
/// (degrees), the lift and the push back against the shot (units/s).
pub const SPEED_SPREAD: f32 = 0.2;
pub const CONE_DEG: f32 = 14.0;
pub const LIFT: f32 = 45.0;
pub const BACK: f32 = 35.0;
/// Tumble end over end and spin about the length, radians/s (random up to).
pub const TUMBLE: f32 = 28.0;
pub const SPIN: f32 = 12.0;
/// A case's mass, kg (a 9 mm case is about 4 g, a rifle case 12 g: the mass matters only
/// against loose objects).
pub const MASS: f32 = 0.01;
/// A change of speed that is heard, units/s, and how many knocks one case makes.
pub const KNOCK_DV: f32 = 40.0;
pub const KNOCKS: u8 = 3;
/// Without a `ShellCasingNode`: the breech this far behind the muzzle (one hand, two
/// hands), game units.
pub const BREECH_BACK: (f32, f32) = (9.0, 35.0);
/// The reload drop starts this long after the reload does (the cylinder or the action
/// open), and the cases follow each other by this much, seconds.
pub const DROP_DELAY: f32 = 0.35;
pub const DROP_STEP: f32 = 0.04;

/// How a weapon's shots give up their cases.
#[derive(Debug, Clone, PartialEq)]
pub enum Eject {
    /// None at all.
    Never,
    /// One each shot, this model.
    EachShot(String),
    /// All at the reload, this model.
    AtReload(String),
}

/// The model of the case a revolver or break action drops, by the weapon's name
/// [design: no record links them].
pub fn reload_case_model(edid: &str) -> &'static str {
    let e = edid.to_ascii_lowercase();
    if e.contains("shotgun") {
        "projectiles\\shotgunshellcasing.nif"
    } else if e.contains("357") {
        "weapons\\2handrifle\\357magnumshellcasing.nif"
    } else if e.contains("44") {
        "weapons\\2handrifle\\44magnumshellcasing.nif"
    } else if e.contains("sequoia") || e.contains("huntingrevolver") || e.contains("4570") {
        "weapons\\2handrifle\\4570shellcasing.nif"
    } else {
        "projectiles\\45calcasing.nif"
    }
}

/// The small pistol case, for a self-loader with no casing model of its own.
pub const SMALL_CASE: &str = "projectiles\\45calcasing.nif";

/// How a weapon ejects (module notes).
pub fn eject_rule(w: &fnv_combat::data::WeaponStats, casing_model: Option<&str>) -> Eject {
    let anim = w.anim_type.as_deref().unwrap_or("").to_ascii_lowercase();
    let skill = w.skill.as_deref().unwrap_or("");
    let gun = matches!(skill, "Guns" | "Explosives") && !["throw", "mine", "melee", "energy", "hand to hand"].iter().any(|k| anim.contains(k));
    if !gun || w.projectile.is_none() {
        return Eject::Never;
    }
    let edid = w.edid.as_deref().unwrap_or("");
    if skill == "Guns" && !w.automatic && w.clip_size > 0 && w.clip_size <= 2 {
        return Eject::AtReload(reload_case_model(edid).to_string());
    }
    match casing_model {
        Some(m) => Eject::EachShot(m.to_string()),
        None if skill == "Guns" && anim.contains("pistol") && w.clip_size <= 6 => Eject::AtReload(reload_case_model(edid).to_string()),
        None if skill == "Guns" => Eject::EachShot(SMALL_CASE.to_string()),
        None => Eject::Never,
    }
}

/// A case's collision cylinder from its model: half length, radius, axis and centre.
#[derive(Clone, Copy, Debug)]
struct Shape {
    half_length: f32,
    radius: f32,
    axis: Vec3,
    center: Vec3,
}

#[derive(Component)]
struct FeelCasing {
    id: u32,
    age: f32,
    life: f32,
    /// Its velocity at the last frame, game units/s.
    last_vel: Vec3,
    knocks: u8,
    since_knock: f32,
    sound: Option<FormId>,
    /// Gone at once when pushed out by the cap: its time is up then.
    fading: bool,
}

/// A case waiting to be thrown (a reload's drop).
struct Waiting {
    at: f32,
    model: String,
    weapon: FormId,
}

#[derive(Resource)]
struct Casings {
    next: u32,
    rng: SplitMix,
    /// Live cases, oldest first: (id, entity).
    live: VecDeque<(u32, Entity)>,
    shapes: HashMap<String, Option<(Arc<PreparedModel>, Shape)>>,
    sounds: HashMap<String, Option<FormId>>,
    waiting: Vec<Waiting>,
    /// The player's shots since the last reload, by weapon.
    since_reload: HashMap<FormId, u32>,
    reloads_seen: Option<u32>,
    /// The player's feet last frame and the motion it gives cases.
    feet: Option<Vec3>,
    player_vel: Vec3,
    told: HashSet<(FormId, &'static str)>,
    /// The player's last weapon fired, and cases the cap sends away.
    last_weapon: Option<FormId>,
    fade_ids: HashSet<u32>,
    thrown: u32,
    thrown_npc: u32,
    dropped: u32,
    knocks: u32,
    capped: u32,
    most_live: usize,
    was_on: bool,
}

impl Default for Casings {
    fn default() -> Self {
        Casings {
            next: 1,
            rng: SplitMix(0xca51_6e5d),
            live: VecDeque::new(),
            shapes: HashMap::new(),
            sounds: HashMap::new(),
            waiting: Vec::new(),
            since_reload: HashMap::new(),
            reloads_seen: None,
            feet: None,
            player_vel: Vec3::ZERO,
            told: HashSet::new(),
            last_weapon: None,
            fade_ids: HashSet::new(),
            thrown: 0,
            thrown_npc: 0,
            dropped: 0,
            knocks: 0,
            capped: 0,
            most_live: 0,
            was_on: false,
        }
    }
}

pub struct FeelCasingsPlugin;

impl Plugin for FeelCasingsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Casings>()
            .add_systems(Update, (throw, move_casings).chain().in_set(crate::shot_fx::ShotFxSet).after(nvf_play::PlaySet::Activation).run_if(resource_exists::<Combat>))
            .add_systems(Last, report);
    }
}

fn spread(rng: &mut SplitMix) -> f32 {
    rng.next_f32() * 2.0 - 1.0
}

fn random_dir(rng: &mut SplitMix) -> Vec3 {
    let z = spread(rng);
    let a = rng.next_f32() * std::f32::consts::TAU;
    let r = (1.0 - z * z).max(0.0).sqrt();
    Vec3::new(r * a.cos(), r * a.sin(), z)
}

/// `d` turned by up to `deg` degrees at random.
fn jitter(rng: &mut SplitMix, d: Vec3, deg: f32) -> Vec3 {
    let d = d.normalize_or(Vec3::Z);
    let side = d.any_orthonormal_vector();
    let a = rng.next_f32() * std::f32::consts::TAU;
    let off = Quat::from_axis_angle(side, (rng.next_f32() * deg).to_radians());
    (Quat::from_axis_angle(d, a) * off * d).normalize_or(d)
}

/// The case model and its cylinder (read once per model).
fn case_shape(models: &mut ModelCtx, meshes_shape: &mut HashMap<String, Option<(Arc<PreparedModel>, Shape)>>, model: &str) -> Option<(Arc<PreparedModel>, Shape)> {
    let path = fnv_actor::resolve::mesh_path(model);
    if let Some(s) = meshes_shape.get(&path) {
        return s.clone();
    }
    let pm = models.model(&path);
    let out = pm.map(|pm| {
        let mut lo = Vec3::splat(f32::MAX);
        let mut hi = Vec3::splat(f32::MIN);
        for s in &pm.shapes {
            let Some(m) = s.cpu.as_deref().or_else(|| models.meshes.get(&s.mesh)) else { continue };
            let Ok(Some(bevy::mesh::VertexAttributeValues::Float32x3(ps))) = m.try_attribute_option(Mesh::ATTRIBUTE_POSITION) else { continue };
            for p in ps {
                let v = s.transform.transform_point3(Vec3::from(*p));
                lo = lo.min(v);
                hi = hi.max(v);
            }
        }
        if lo.x > hi.x {
            (lo, hi) = pm.bounds.unwrap_or((Vec3::splat(-0.4), Vec3::splat(0.4)));
        }
        let e = (hi - lo).max(Vec3::splat(0.2));
        let (axis, len, across) = if e.x >= e.y && e.x >= e.z {
            (Vec3::X, e.x, (e.y + e.z) * 0.25)
        } else if e.y >= e.z {
            (Vec3::Y, e.y, (e.x + e.z) * 0.25)
        } else {
            (Vec3::Z, e.z, (e.x + e.y) * 0.25)
        };
        let shape = Shape { half_length: len * 0.5, radius: across.max(0.15), axis, center: (lo + hi) * 0.5 };
        info!("shell casings: model {path}: {:.2} units long, {:.2} across, along {axis}", len, across * 2.0);
        (pm, shape)
    });
    meshes_shape.insert(path, out.clone());
    out
}

/// The casing's PHY sound by its model (`shot_fx::casing_sound`), found once.
fn case_sound(sfx: &Sfx, cache: &mut HashMap<String, Option<FormId>>, model: &str) -> Option<FormId> {
    let edid = crate::shot_fx::casing_sound(model);
    *cache.entry(edid.to_string()).or_insert_with(|| sfx.forms.sounds.iter().find(|(_, s)| s.edid.as_deref() == Some(edid)).map(|(&id, _)| id))
}

/// A place to throw from (game space): where, its rotation, the way out and the gun's
/// forward.
#[derive(Clone, Copy)]
struct Origin {
    at: Vec3,
    turn: Quat,
    out: Vec3,
    forward: Vec3,
}

/// A transform under the world root as game space: place, turn (scale taken out), and the
/// axes.
fn game_place(root: &Affine3A, g: &GlobalTransform) -> (Vec3, Quat, Affine3A) {
    let m = root.inverse() * g.affine();
    let (_, q, t) = m.to_scale_rotation_translation();
    (t, q.normalize(), m)
}

#[allow(clippy::too_many_arguments)]
fn throw(
    mut commands: Commands,
    time: Res<Time>,
    feel: Option<Res<ModernFeel>>,
    mut st: ResMut<Casings>,
    mut ejected: MessageReader<CasingFx>,
    mut fired: MessageReader<ShotFired>,
    combat: Res<Combat>,
    anim: Option<Res<WeaponAnim>>,
    sfx: Option<Res<Sfx>>,
    player: Option<Res<Player>>,
    body: Option<Res<PlayerBody>>,
    mut phys: ResMut<PhysState>,
    mut models: ModelCtx,
    root: Query<(Entity, &GlobalTransform), With<WorldRoot>>,
    globals: Query<&GlobalTransform>,
    (ports, muzzles): (Query<(&CasingPort, &GlobalTransform)>, Query<(&Muzzle, &GlobalTransform)>),
) {
    let on = feel.as_deref().is_some_and(ModernFeel::shell_casings);
    let st = &mut *st;
    if !on {
        ejected.clear();
        fired.clear();
        // Switched off: every case goes at once.
        if st.was_on {
            for (id, e) in st.live.drain(..) {
                phys.world.remove_casing(id);
                commands.entity(e).try_despawn();
            }
            st.waiting.clear();
            info!("shell casings: switched off, the cases are gone");
        }
        st.was_on = false;
        return;
    }
    st.was_on = true;
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    // The player's motion, smoothed, for the cases' start.
    if let Some(p) = player.as_deref() {
        let feet = Vec3::from_array(p.feet);
        if let (Some(last), true) = (st.feet, dt > 0.0) {
            let v = (feet - last) / dt;
            let v = if v.length() > 1500.0 { Vec3::ZERO } else { v };
            st.player_vel = st.player_vel.lerp(v, (dt * 12.0).min(1.0));
        }
        st.feet = Some(feet);
    }
    let (Ok((world_root, root_at)), Some(sfx)) = (root.single(), sfx) else {
        ejected.clear();
        fired.clear();
        return;
    };
    let root_aff = root_at.affine();
    let eye = player.as_deref().map(|p| Vec3::from_array(p.eye()));
    let view_fwd = player.as_deref().map_or(Vec3::Y, |p| Vec3::from_array(p.look_dir()));
    // The player's way out: the drawn `ShellCasingNode`, else beside the drawn breech.
    let player_from = |st_back: f32| -> Option<Origin> {
        let b = body.as_deref()?;
        if let Some(g) = b.casing_entity().and_then(|e| globals.get(e).ok()) {
            let (at, turn, m) = game_place(&root_aff, g);
            return Some(Origin { at, turn, out: m.transform_vector3(Vec3::Z).normalize_or(Vec3::Z), forward: view_fwd });
        }
        let g = b.muzzle_entity().and_then(|e| globals.get(e).ok())?;
        let (at, turn, m) = game_place(&root_aff, g);
        let fwd = m.transform_vector3(Vec3::Y).normalize_or(view_fwd);
        let right = fwd.cross(Vec3::Z).normalize_or(Vec3::X);
        Some(Origin { at: at - fwd * st_back + right * 1.5, turn, out: (right + Vec3::Z * 0.6).normalize(), forward: fwd })
    };
    let two_hands = |w: FormId| combat.forms.weapons.get(&w).is_some_and(|w| w.anim_type.as_deref().is_some_and(|a| a.contains("2 Hand")));
    let mut want: Vec<(Origin, String, FormId, bool, f32)> = Vec::new();

    // The player's casings at combat's time (the shot, or the clip's Eject key).
    for c in ejected.read() {
        let Some(w) = combat.forms.weapons.get(&c.weapon) else { continue };
        let model = sfx.forms.weapons.get(&c.weapon).and_then(|f| f.shell_casing_model.as_deref());
        if let Eject::EachShot(m) = eject_rule(w, model) {
            let back = if two_hands(c.weapon) { BREECH_BACK.1 } else { BREECH_BACK.0 };
            if let Some(f) = player_from(back) {
                want.push((f, m, c.weapon, true, Vec3::from_array(c.velocity).length()));
            }
        }
    }
    let base_speed = combat.rules.shell_eject_speed.max(60.0);
    for shot in fired.read() {
        let Some(w) = combat.forms.weapons.get(&shot.weapon) else { continue };
        let model = sfx.forms.weapons.get(&shot.weapon).and_then(|f| f.shell_casing_model.as_deref());
        let rule = eject_rule(w, model);
        let back = if two_hands(shot.weapon) { BREECH_BACK.1 } else { BREECH_BACK.0 };
        if shot.shooter == FormId::PLAYER_REF {
            *st.since_reload.entry(shot.weapon).or_default() += 1;
            st.last_weapon = Some(shot.weapon);
            // A self-loader with no casing model: combat sends no `CasingFx` for it.
            if let (Eject::EachShot(m), None) = (&rule, model) {
                if let Some(f) = player_from(back) {
                    want.push((f, m.clone(), shot.weapon, true, base_speed));
                }
            }
            continue;
        }
        let Eject::EachShot(m) = rule else { continue };
        let at = shot.at.map(Vec3::from_array);
        if let (Some(at), Some(eye)) = (at, eye) {
            if at.distance(eye) > NPC_RANGE {
                continue;
            }
        }
        let port = ports.iter().find(|(p, _)| p.reference == shot.shooter.0).map(|(_, g)| game_place(&root_aff, g));
        let muzzle = muzzles.iter().find(|(p, _)| p.reference == shot.shooter.0).map(|(_, g)| game_place(&root_aff, g));
        let fwd = shot.dir.map(Vec3::from_array).or(muzzle.map(|m| m.2.transform_vector3(Vec3::Y))).unwrap_or(Vec3::Y).normalize_or(Vec3::Y);
        let from = match (port, muzzle) {
            (Some((p, q, m)), _) => Origin { at: p, turn: q, out: m.transform_vector3(Vec3::Z).normalize_or(Vec3::Z), forward: fwd },
            (None, Some((p, q, _))) => {
                let right = fwd.cross(Vec3::Z).normalize_or(Vec3::X);
                Origin { at: p - fwd * back + right * 1.5, turn: q, out: (right + Vec3::Z * 0.6).normalize(), forward: fwd }
            }
            (None, None) => {
                let Some(at) = at else { continue };
                let right = fwd.cross(Vec3::Z).normalize_or(Vec3::X);
                Origin { at: at - fwd * back + right * 2.0, turn: Quat::from_rotation_arc(Vec3::Y, fwd), out: (right + Vec3::Z * 0.6).normalize(), forward: fwd }
            }
        };
        want.push((from, m, shot.weapon, false, base_speed));
    }

    // A revolver's or a break action's cases fall out at the player's reload.
    if let Some(a) = anim.as_deref() {
        let seen = st.reloads_seen.replace(a.reloads);
        if seen.is_some_and(|s| s != a.reloads) {
            let held = st.last_weapon.map(|w| (w, st.since_reload.get(&w).copied().unwrap_or(0)));
            if let Some((weapon, n)) = held.filter(|(_, n)| *n > 0) {
                if let Some(w) = combat.forms.weapons.get(&weapon) {
                    let model = sfx.forms.weapons.get(&weapon).and_then(|f| f.shell_casing_model.as_deref());
                    if let Eject::AtReload(m) = eject_rule(w, model) {
                        let n = n.min(w.clip_size.max(1));
                        let rate = if a.reload_rate > 0.0 { a.reload_rate } else { 1.0 };
                        for i in 0..n {
                            st.waiting.push(Waiting { at: now + (DROP_DELAY + i as f32 * DROP_STEP) / rate, model: m.clone(), weapon });
                        }
                        info!("shell casings: {} reloads: {n} spent cases fall out ({m})", w.edid.as_deref().unwrap_or("?"));
                    }
                }
            }
            st.since_reload.clear();
        }
    }
    let due: Vec<Waiting> = {
        let (due, keep): (Vec<Waiting>, Vec<Waiting>) = std::mem::take(&mut st.waiting).into_iter().partition(|w| w.at <= now);
        st.waiting = keep;
        due
    };
    for d in due {
        let back = if two_hands(d.weapon) { BREECH_BACK.1 * 0.8 } else { BREECH_BACK.0 };
        let Some(mut f) = player_from(back) else { continue };
        // They fall: out and down, slowly.
        f.out = (Vec3::NEG_Z * 0.8 + (f.out - f.forward * 0.3) * 0.4).normalize_or(Vec3::NEG_Z);
        want.push((f, d.model, d.weapon, true, 35.0));
        st.dropped += 1;
    }

    for (from, model, weapon, by_player, speed) in want {
        let Some((pm, shape)) = case_shape(&mut models, &mut st.shapes, &model) else { continue };
        let sound = case_sound(&sfx, &mut st.sounds, &model);
        let rng = &mut st.rng;
        let speed = speed * (1.0 + spread(rng) * SPEED_SPREAD);
        let dir = jitter(rng, from.out, CONE_DEG);
        let mut vel = dir * speed + Vec3::Z * LIFT * (0.6 + 0.8 * rng.next_f32()) - from.forward * BACK * (0.5 + rng.next_f32());
        if by_player {
            vel += st.player_vel;
        }
        // End over end about an axis across its length and its flight, and about itself.
        let long = from.turn * shape.axis;
        let across = long.cross(dir).normalize_or(random_dir(rng));
        let angvel = across * TUMBLE * (0.5 + 0.5 * rng.next_f32()) * spread(rng).signum() + long * SPIN * spread(rng);
        let id = st.next;
        st.next = st.next.wrapping_add(1).max(1);
        phys.world.add_casing(&CasingStart {
            id,
            translation: from.at.to_array(),
            rotation: Mat3::from_quat(from.turn).transpose().to_cols_array(),
            half_length: shape.half_length,
            radius: shape.radius,
            axis: shape.axis.to_array(),
            center: shape.center.to_array(),
            linvel: vel.to_array(),
            angvel: angvel.to_array(),
            mass: MASS,
        });
        let holder = commands
            .spawn((
                Transform::from_translation(from.at).with_rotation(from.turn),
                Visibility::Inherited,
                ChildOf(world_root),
                FeelCasing { id, age: 0.0, life: LIFE.0 + (LIFE.1 - LIFE.0) * rng.next_f32(), last_vel: vel, knocks: 0, since_knock: 1.0, sound, fading: false },
            ))
            .id();
        for s in &pm.shapes {
            commands.spawn((Mesh3d(s.mesh.clone()), MeshMaterial3d(s.material.clone()), Transform::from_matrix(Mat4::from(s.transform)), Visibility::Inherited, NotShadowCaster, ChildOf(holder)));
        }
        st.live.push_back((id, holder));
        if by_player {
            st.thrown += 1;
        } else {
            st.thrown_npc += 1;
        }
        let way = if by_player { "the player's" } else { "another actor's" };
        if st.told.insert((weapon, way)) {
            info!(
                "shell casings: {} throws {model} ({way}) at {:.0} units/s, from ({:.0}, {:.0}, {:.0}); {} cases live",
                combat.forms.weapons.get(&weapon).and_then(|w| w.edid.as_deref()).unwrap_or("?"),
                vel.length(),
                from.at.x,
                from.at.y,
                from.at.z,
                st.live.len()
            );
        }
    }
    // The cap: the oldest go first (quickly).
    let mut over = st.live.len().saturating_sub(MAX_LIVE);
    let oldest: Vec<u32> = st.live.iter().map(|(id, _)| *id).filter(|id| !st.fade_ids.contains(id)).collect();
    for id in oldest {
        if over == 0 {
            break;
        }
        st.fade_ids.insert(id);
        over -= 1;
        st.capped += 1;
    }
    st.most_live = st.most_live.max(st.live.len());
}

/// Cases follow their bodies, knock, and fade at the end of their time.
fn move_casings(
    mut commands: Commands,
    time: Res<Time>,
    mut st: ResMut<Casings>,
    mut cases: Query<(Entity, &mut FeelCasing, &mut Transform)>,
    mut phys: ResMut<PhysState>,
    mut sounds: MessageWriter<ScaledSound>,
) {
    if st.live.is_empty() {
        return;
    }
    let dt = time.delta_secs();
    let g = phys.world.params.gravity;
    let alpha = phys.draw_alpha();
    let mut gone: Vec<(u32, Entity)> = Vec::new();
    for (e, mut c, mut t) in &mut cases {
        let Some(s) = phys.world.casing_state(c.id) else {
            // The physics world was built again (a load): the case is gone.
            gone.push((c.id, e));
            continue;
        };
        c.age += dt;
        c.since_knock += dt;
        if !c.fading && st.fade_ids.contains(&c.id) {
            c.fading = true;
        }
        if c.fading && c.age < c.life {
            c.life = c.age;
        }
        let d = phys.world.drawn_casing_state(c.id, alpha).unwrap_or(s);
        t.translation = Vec3::from(d.translation);
        t.rotation = Quat::from_mat3(&Mat3::from_cols_array(&d.rotation).transpose());
        let fade = if c.fading { FADE * 0.4 } else { FADE };
        let left = c.life + fade - c.age;
        if left <= 0.0 {
            gone.push((c.id, e));
            continue;
        }
        t.scale = Vec3::splat((left / fade).clamp(0.0, 1.0));
        // A knock: the velocity changed by more than gravity's share this frame.
        let v = Vec3::from(s.linvel);
        let dv = (v - (c.last_vel - Vec3::Z * g * dt)).length();
        c.last_vel = v;
        if dv >= KNOCK_DV && c.knocks < KNOCKS && c.since_knock > 0.05 && c.age > 0.04 {
            if let Some(sound) = c.sound {
                c.knocks += 1;
                c.since_knock = 0.0;
                st.knocks += 1;
                sounds.write(ScaledSound { sound, at: Some(d.translation), gain: (dv / 160.0).clamp(0.25, 1.0) });
            }
        }
    }
    if gone.is_empty() {
        return;
    }
    for &(id, e) in &gone {
        commands.entity(e).try_despawn();
        phys.world.remove_casing(id);
        st.fade_ids.remove(&id);
    }
    st.live.retain(|(id, _)| !gone.iter().any(|g| g.0 == *id));
}

fn report(mut exit: MessageReader<AppExit>, st: Res<Casings>) {
    if exit.read().last().is_some() && (st.thrown + st.thrown_npc) > 0 {
        info!(
            "shell casings summary: {} thrown by the player ({} dropped at reloads), {} by other actors, {} knocks heard, at most {} live, {} pushed out by the cap of {MAX_LIVE}",
            st.thrown, st.dropped, st.thrown_npc, st.knocks, st.most_live, st.capped
        );
    }
}
