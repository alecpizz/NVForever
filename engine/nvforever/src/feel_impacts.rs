//! Modern Feel › Impact effects (docs/design/modern-feel-vfx.md): touches laid over the
//! game's own shot effects (`shot_fx`'s IPCT models and their particles, `decals`)
//! [design: NVForever's own; every constant here an engineering choice tuned by eye].
//! Gated by `ModernFeel::impact_effects()`; switched off, every particle goes at once and
//! nothing more is made.
//!
//! - **Muzzle smoke.** Each shot of a gun (skill Guns or Explosives, not an energy,
//!   thrown or placed weapon) puffs a little smoke forward from the muzzle, and heats its
//!   shooter's barrel ([`HEAT_PER_SHOT`] by the weapon's damage). While hot, thin wisps
//!   rise from the muzzle and drift with the air, more of them and thicker the hotter it
//!   is, so sustained fire leaves a smoking barrel; the heat halves every
//!   [`HEAT_HALF_LIFE`] s. The player's muzzle is the weapon's `ProjectileNode` as drawn
//!   (first person: where the widened arms rig shows it, `player_body::place_lens`); another
//!   actor's its muzzle marker (`weapons::Muzzle`), else where the shot left. Only within
//!   [`RANGE`] of the player.
//! - **By surface.** A shot that lands on the world (not on an actor) is looked at with a
//!   short ray along the surface's normal for its collision material, read as one of the
//!   twelve impact materials (`fnv_combat::fx::impact_material`; terrain counts as dirt):
//!   - dirt and grass: a dust puff the colour of sand and a spray of dark clods;
//!   - stone: a grey dust puff and stone chips, now and then a spark;
//!   - wood: a little brown dust and splinters;
//!   - metal and hollow metal: a shower of sparks that bounce, and a wisp of smoke.
//!   Glass, cloth, organic and water keep the game's own effects alone.
//! - **Physical bits.** Chips and sparks fly ballistically under the world's gravity and
//!   bounce off what they meet (a ray from each moving one per frame, the world only), then
//!   lie still and fade.
//! - **Drawing.** Four pools (smoke, dust, chips, sparks), each one mesh of squares facing
//!   the camera rebuilt each frame, with the game's own effect textures
//!   (`textures\effects\fxsmokesub.dds`, `fxdustsmoke01.dds`, `fxdebrisgen01.dds`,
//!   `fxtinysparkwht.dds`). Smoke, dust and chips are unlit, their colour shaded by the place's
//!   ambient light ([`AMBIENT_FULL`]) so they do not glow at night; sparks glow (added) and
//!   stretch along their flight. Each pool has a
//!   cap ([`CAPS`]); past it no more are born.
//!
//! Log: `impact effects: <material> at (x, y, z): …` for the first impacts of each
//! material, `impact effects: <weapon> smokes …` once per weapon, and at exit `impact
//! effects summary: …`.

use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::math::Affine3A;
use bevy::mesh::Indices;
use bevy::prelude::*;
use fnv_combat::rng::SplitMix;
use fnv_phys::{layers, Filter, Owner};
use fnv_sim::FormId;
use nvf_assets::model::ModelCtx;
use nvf_assets::nif_material::nif_material;
use nvf_assets::particles::particle_mesh;
use nvf_play::combat::{Combat, ImpactFx, ShotFired};
use nvf_play::modern_feel::ModernFeel;
use nvf_play::phys::PhysState;
use nvf_play::player::Player;
use std::collections::{HashMap, HashSet};

use crate::player_body::PlayerBody;
use crate::weapons::Muzzle;
use crate::world::WorldRoot;

/// Effects farther than this from the player are not made, game units (about 57 m).
pub const RANGE: f32 = 4000.0;
/// Barrel heat: a shot adds this much per point of the weapon's damage (between the
/// bounds), heat stops at [`HEAT_MAX`], and halves every [`HEAT_HALF_LIFE`] seconds.
pub const HEAT_PER_SHOT: (f32, f32, f32) = (0.006, 0.08, 0.45);
pub const HEAT_MAX: f32 = 2.5;
pub const HEAT_HALF_LIFE: f32 = 1.6;
/// Wisps a second at heat 1.
pub const WISPS_PER_HEAT: f32 = 9.0;
/// The air's drift, units/s (game axes) [design: a light breeze].
pub const DRIFT: Vec3 = Vec3::new(6.0, 3.5, 0.0);
/// Particles at once, at most: smoke, dust, chips, sparks.
/// The ambient light (luminance of its display values) at which smoke and dust show
/// their full colour; darker places shade them down to a fifth.
pub const AMBIENT_FULL: f32 = 0.35;
pub const CAPS: [usize; 4] = [420, 220, 200, 260];

const SMOKE: usize = 0;
const DUST: usize = 1;
const CHIPS: usize = 2;
const SPARKS: usize = 3;

/// The pools' textures, frames across × down, and whether they glow.
const LOOKS: [(&str, u32, bool); 4] = [
    ("textures\\effects\\fxsmokesub.dds", 1, false),
    ("textures\\effects\\fxdustsmoke01.dds", 2, false),
    ("textures\\effects\\fxdebrisgen01.dds", 4, false),
    ("textures\\effects\\fxtinysparkwht.dds", 1, true),
];

#[derive(Clone, Copy)]
struct P {
    pos: Vec3,
    vel: Vec3,
    age: f32,
    life: f32,
    /// Width at birth and at death, game units.
    size: (f32, f32),
    angle: f32,
    spin: f32,
    frame: u32,
    /// Linear colour and the alpha at its fullest.
    color: Vec4,
    /// Slowing, per second, and the share of gravity it falls by.
    drag: f32,
    fall: f32,
    /// It meets the world (chips, sparks) and has come to rest.
    solid: bool,
    resting: bool,
    bounces: u8,
}

/// A barrel's heat and where its muzzle was last seen (game space).
struct Heat {
    level: f32,
    at: Vec3,
    owed: f32,
    player: bool,
}

#[derive(Default)]
struct Pool {
    ps: Vec<P>,
    entity: Option<Entity>,
    mesh: Option<Handle<Mesh>>,
}

#[derive(Resource)]
struct FeelFx {
    pools: [Pool; 4],
    /// The world root the pools' entities hang under (a load makes a new one).
    root: Option<Entity>,
    heat: HashMap<FormId, Heat>,
    rng: SplitMix,
    /// No texture for a pool: nothing drawn for it (reported once).
    missing: [bool; 4],
    told_weapons: HashSet<FormId>,
    told_materials: HashMap<&'static str, u32>,
    impacts: u32,
    shots: u32,
    born: [u64; 4],
    most: [usize; 4],
    was_on: bool,
    told_shade: bool,
}

impl Default for FeelFx {
    fn default() -> Self {
        FeelFx {
            pools: Default::default(),
            root: None,
            heat: HashMap::new(),
            rng: SplitMix(0xf1e1_d5e7),
            missing: [false; 4],
            told_weapons: HashSet::new(),
            told_materials: HashMap::new(),
            impacts: 0,
            shots: 0,
            born: [0; 4],
            most: [0; 4],
            was_on: false,
            told_shade: false,
        }
    }
}

pub struct FeelImpactsPlugin;

impl Plugin for FeelImpactsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FeelFx>()
            .add_systems(Update, (spawn, simulate).chain().in_set(crate::shot_fx::ShotFxSet).after(nvf_play::PlaySet::Activation).run_if(resource_exists::<Combat>))
            .add_systems(Last, report);
    }
}

fn spread(rng: &mut SplitMix) -> f32 {
    rng.next_f32() * 2.0 - 1.0
}

fn range(rng: &mut SplitMix, (a, b): (f32, f32)) -> f32 {
    a + (b - a) * rng.next_f32()
}

fn random_dir(rng: &mut SplitMix) -> Vec3 {
    let z = spread(rng);
    let a = rng.next_f32() * std::f32::consts::TAU;
    let r = (1.0 - z * z).max(0.0).sqrt();
    Vec3::new(r * a.cos(), r * a.sin(), z)
}

/// An sRGB colour as linear, with the alpha.
fn lin(r: f32, g: f32, b: f32, a: f32) -> Vec4 {
    let l = Color::srgb(r, g, b).to_linear();
    Vec4::new(l.red, l.green, l.blue, a)
}

/// Whether a weapon's shots smoke: a gun that is not energy, thrown or placed.
fn smokes(w: &fnv_combat::data::WeaponStats) -> bool {
    let anim = w.anim_type.as_deref().unwrap_or("").to_ascii_lowercase();
    matches!(w.skill.as_deref(), Some("Guns" | "Explosives")) && w.projectile.is_some() && !["throw", "mine", "melee", "energy", "hand to hand"].iter().any(|k| anim.contains(k))
}

impl FeelFx {
    fn push(&mut self, pool: usize, p: P) {
        if self.pools[pool].ps.len() < CAPS[pool] {
            self.pools[pool].ps.push(p);
            self.born[pool] += 1;
        }
    }

    fn clear(&mut self) {
        for p in &mut self.pools {
            p.ps.clear();
        }
        self.heat.clear();
    }

    /// A puff of smoke at a muzzle, thrown along `fwd`.
    fn shot_puff(&mut self, at: Vec3, fwd: Vec3, scale: f32) {
        let n = 2 + (self.rng.next_f32() * 2.0) as usize;
        for _ in 0..n {
            let rng = &mut self.rng;
            let p = P {
                pos: at + fwd * range(rng, (1.0, 6.0)) * scale,
                vel: fwd * range(rng, (50.0, 130.0)) + random_dir(rng) * 14.0 + Vec3::Z * 8.0,
                age: 0.0,
                life: range(rng, (0.9, 1.6)),
                size: (4.0 * scale, range(rng, (18.0, 28.0)) * scale),
                angle: rng.next_f32() * std::f32::consts::TAU,
                spin: spread(rng) * 0.8,
                frame: 0,
                color: lin(0.78, 0.77, 0.75, 0.26),
                drag: 3.2,
                fall: -0.02,
                solid: false,
                resting: false,
                bounces: 0,
            };
            self.push(SMOKE, p);
        }
    }

    /// A rising wisp from a hot barrel.
    fn wisp(&mut self, at: Vec3, heat: f32, scale: f32) {
        let rng = &mut self.rng;
        let thick = (heat / 1.2).min(1.0);
        let p = P {
            pos: at + random_dir(rng) * 0.6,
            vel: Vec3::Z * range(rng, (12.0, 24.0)) + random_dir(rng) * 3.0 + DRIFT * 0.4,
            age: 0.0,
            life: range(rng, (1.6, 2.8)),
            size: (2.0 * scale, range(rng, (10.0, 16.0)) * scale * (0.7 + 0.3 * thick)),
            angle: rng.next_f32() * std::f32::consts::TAU,
            spin: spread(rng) * 0.6,
            frame: 0,
            color: lin(0.8, 0.8, 0.79, 0.08 + 0.16 * thick),
            drag: 0.6,
            fall: -0.015,
            solid: false,
            resting: false,
            bounces: 0,
        };
        self.push(SMOKE, p);
    }

    /// Dust thrown off a surface.
    fn dust(&mut self, at: Vec3, n: Vec3, count: usize, color: Vec4, size: (f32, f32), life: (f32, f32)) {
        for _ in 0..count {
            let rng = &mut self.rng;
            let out = (n + random_dir(rng) * 0.55).normalize_or(n);
            let p = P {
                pos: at + n * 2.0,
                vel: out * range(rng, (30.0, 85.0)) + Vec3::Z * 12.0,
                age: 0.0,
                life: range(rng, life),
                size: (size.0 * range(rng, (0.7, 1.1)), size.1 * range(rng, (0.75, 1.15))),
                angle: rng.next_f32() * std::f32::consts::TAU,
                spin: spread(rng) * 0.5,
                frame: (rng.next_f32() * 4.0) as u32,
                color,
                drag: 2.4,
                fall: 0.03,
                solid: false,
                resting: false,
                bounces: 0,
            };
            self.push(DUST, p);
        }
    }

    /// Chips thrown out of a surface: `reflect` the shot's way bounced off it.
    fn chips(&mut self, at: Vec3, n: Vec3, reflect: Vec3, count: usize, color: Vec4, size: (f32, f32)) {
        for _ in 0..count {
            let rng = &mut self.rng;
            let out = (n * 0.8 + reflect * 0.35 + random_dir(rng) * 0.55).normalize_or(n);
            let p = P {
                pos: at + n * 1.0,
                vel: out * range(rng, (110.0, 260.0)),
                age: 0.0,
                life: range(rng, (1.6, 2.6)),
                size: (range(rng, size), 0.0),
                angle: rng.next_f32() * std::f32::consts::TAU,
                spin: spread(rng) * 18.0,
                frame: (rng.next_f32() * 16.0) as u32,
                color,
                drag: 0.4,
                fall: 1.0,
                solid: true,
                resting: false,
                bounces: 0,
            };
            self.push(CHIPS, p);
        }
    }

    /// Sparks off metal (or stone).
    fn sparks(&mut self, at: Vec3, n: Vec3, reflect: Vec3, count: usize, speed: (f32, f32)) {
        for _ in 0..count {
            let rng = &mut self.rng;
            let out = (reflect * 0.6 + n * 0.6 + random_dir(rng) * 0.5).normalize_or(n);
            let p = P {
                pos: at + n * 0.8,
                vel: out * range(rng, speed),
                age: 0.0,
                life: range(rng, (0.18, 0.5)),
                size: (range(rng, (0.35, 0.6)), 0.0),
                angle: 0.0,
                spin: 0.0,
                frame: 0,
                color: Vec4::new(4.0, 2.0, 0.7, 1.0),
                drag: 1.6,
                fall: 0.55,
                solid: true,
                resting: false,
                bounces: 0,
            };
            self.push(SPARKS, p);
        }
    }
}

/// The collision material under an impact, as an impact material; `None`: no surface
/// found (or an actor).
fn surface(phys: &mut PhysState, at: Vec3, n: Vec3) -> Option<&'static str> {
    let filter = Filter { no_actors: true, exclude_layers: layers::character_ignores(), ..Default::default() };
    let n = n.normalize_or(Vec3::Z);
    let hit = phys.ray((at + n * 4.0).to_array(), (-n).to_array(), 10.0, filter)?;
    match hit.owner {
        Owner::Terrain { .. } => Some("dirt"),
        _ => Some(fnv_combat::fx::impact_material(hit.material)),
    }
}

/// A muzzle in game space: the player's drawn `ProjectileNode`, else another actor's
/// muzzle marker.
fn muzzle_at(root: &Affine3A, g: &GlobalTransform) -> (Vec3, Vec3) {
    let m = root.inverse() * g.affine();
    (m.translation.into(), m.transform_vector3(Vec3::Y).normalize_or(Vec3::Y))
}

#[allow(clippy::too_many_arguments)]
fn spawn(
    feel: Option<Res<ModernFeel>>,
    mut st: ResMut<FeelFx>,
    mut fired: MessageReader<ShotFired>,
    mut landed: MessageReader<ImpactFx>,
    combat: Res<Combat>,
    player: Option<Res<Player>>,
    body: Option<Res<PlayerBody>>,
    mut phys: ResMut<PhysState>,
    root: Query<&GlobalTransform, With<WorldRoot>>,
    globals: Query<&GlobalTransform>,
    muzzles: Query<(&Muzzle, &GlobalTransform)>,
) {
    let on = feel.as_deref().is_some_and(ModernFeel::impact_effects);
    let st = &mut *st;
    if !on {
        fired.clear();
        landed.clear();
        if st.was_on {
            st.clear();
            info!("impact effects: switched off, the particles are gone");
        }
        st.was_on = false;
        return;
    }
    st.was_on = true;
    let Ok(root_at) = root.single() else {
        fired.clear();
        landed.clear();
        return;
    };
    let root_aff = root_at.affine();
    let eye = player.as_deref().map(|p| Vec3::from_array(p.eye()));
    let near = |at: Vec3| eye.is_none_or(|e| e.distance(at) <= RANGE);

    for shot in fired.read() {
        let Some(w) = combat.forms.weapons.get(&shot.weapon) else { continue };
        if !smokes(w) {
            continue;
        }
        let is_player = shot.shooter == FormId::PLAYER_REF;
        let place = if is_player {
            body.as_deref().and_then(PlayerBody::muzzle_entity).and_then(|e| globals.get(e).ok()).map(|g| muzzle_at(&root_aff, g))
        } else {
            muzzles.iter().find(|(m, _)| m.reference == shot.shooter.0).map(|(_, g)| muzzle_at(&root_aff, g)).or(shot.at.map(|a| (Vec3::from_array(a), shot.dir.map_or(Vec3::Y, Vec3::from_array))))
        };
        let Some((at, fwd)) = place else { continue };
        if !near(at) {
            continue;
        }
        let fwd = shot.dir.filter(|_| !is_player).map_or(fwd, Vec3::from_array).normalize_or(fwd);
        // The player's own smoke a little smaller: it is right before the eye.
        let scale = if is_player { 0.7 } else { 1.0 };
        st.shot_puff(at, fwd, scale);
        let add = (w.base_damage.max(1) as f32 * HEAT_PER_SHOT.0).clamp(HEAT_PER_SHOT.1, HEAT_PER_SHOT.2) * (w.projectile_count.max(1) as f32).sqrt();
        let h = st.heat.entry(shot.shooter).or_insert(Heat { level: 0.0, at, owed: 0.0, player: is_player });
        h.level = (h.level + add).min(HEAT_MAX);
        h.at = at;
        st.shots += 1;
        if st.told_weapons.insert(shot.weapon) {
            info!("impact effects: {} smokes at its muzzle ({:.0}, {:.0}, {:.0}), heat {:.2} a shot", w.edid.as_deref().unwrap_or("?"), at.x, at.y, at.z, add);
        }
    }

    for hit in landed.read() {
        if hit.actor.is_some() {
            continue;
        }
        let at = Vec3::from_array(hit.point);
        if !near(at) {
            continue;
        }
        let n = Vec3::from_array(hit.normal).normalize_or(Vec3::Z);
        let dir = Vec3::from_array(hit.dir).normalize_or(-n);
        let reflect = (dir - 2.0 * dir.dot(n) * n).normalize_or(n);
        let Some(mat) = surface(&mut phys, at, n) else { continue };
        let rng = &mut st.rng;
        let k = |rng: &mut SplitMix, a: usize, b: usize| a + (rng.next_f32() * (b - a + 1) as f32) as usize;
        let what = match mat {
            "dirt" | "grass" => {
                let (d, c) = (k(rng, 3, 5), k(rng, 4, 7));
                st.dust(at, n, d, lin(0.60, 0.52, 0.40, 0.5), (7.0, 42.0), (1.4, 2.4));
                st.chips(at, n, reflect, c, lin(0.30, 0.24, 0.18, 1.0), (0.9, 1.7));
                format!("{d} dust, {c} clods")
            }
            "stone" => {
                let (d, c, s) = (k(rng, 2, 3), k(rng, 5, 8), k(rng, 0, 2));
                st.dust(at, n, d, lin(0.64, 0.63, 0.60, 0.45), (6.0, 30.0), (1.0, 1.9));
                st.chips(at, n, reflect, c, lin(0.55, 0.54, 0.52, 1.0), (0.8, 1.6));
                st.sparks(at, n, reflect, s, (200.0, 380.0));
                format!("{d} dust, {c} chips, {s} sparks")
            }
            "wood" => {
                let (d, c) = (k(rng, 1, 2), k(rng, 4, 6));
                st.dust(at, n, d, lin(0.55, 0.45, 0.32, 0.35), (5.0, 22.0), (0.8, 1.4));
                st.chips(at, n, reflect, c, lin(0.62, 0.48, 0.30, 1.0), (1.0, 2.2));
                format!("{d} dust, {c} splinters")
            }
            "metal" | "hollow_metal" => {
                let s = k(rng, 8, 14);
                st.sparks(at, n, reflect, s, (260.0, 560.0));
                let rng = &mut st.rng;
                let p = P {
                    pos: at + n * 2.0,
                    vel: n * 20.0 + Vec3::Z * 10.0,
                    age: 0.0,
                    life: range(rng, (0.8, 1.3)),
                    size: (3.0, 16.0),
                    angle: rng.next_f32() * std::f32::consts::TAU,
                    spin: spread(rng) * 0.6,
                    frame: 0,
                    color: lin(0.45, 0.44, 0.43, 0.3),
                    drag: 1.5,
                    fall: -0.02,
                    solid: false,
                    resting: false,
                    bounces: 0,
                };
                st.push(SMOKE, p);
                format!("{s} sparks, a wisp")
            }
            _ => continue,
        };
        st.impacts += 1;
        let told = st.told_materials.entry(mat).or_default();
        *told += 1;
        if *told <= 2 {
            info!("impact effects: {mat} at ({:.0}, {:.0}, {:.0}): {what}", at.x, at.y, at.z);
        }
    }
}

/// Births from hot barrels, motion, then the pools' meshes.
#[allow(clippy::too_many_arguments)]
fn simulate(
    mut commands: Commands,
    time: Res<Time>,
    mut st: ResMut<FeelFx>,
    mut phys: ResMut<PhysState>,
    mut models: ModelCtx,
    body: Option<Res<PlayerBody>>,
    root: Query<(Entity, &GlobalTransform), With<WorldRoot>>,
    globals: Query<&GlobalTransform>,
    muzzles: Query<(&Muzzle, &GlobalTransform)>,
    camera: Query<&Transform, crate::fly::WorldCamera>,
    ambient: Option<Res<GlobalAmbientLight>>,
) {
    let st = &mut *st;
    if !st.was_on && st.pools.iter().all(|p| p.ps.is_empty()) {
        return;
    }
    let dt = time.delta_secs();
    let Ok((world_root, root_at)) = root.single() else { return };
    let root_aff = root_at.affine();

    // Hot barrels: wisps from where each muzzle is now.
    let decay = 0.5f32.powf(dt / HEAT_HALF_LIFE);
    let player_muzzle = body.as_deref().and_then(PlayerBody::muzzle_entity).and_then(|e| globals.get(e).ok()).map(|g| muzzle_at(&root_aff, g).0);
    let mut hot: Vec<(Vec3, f32, f32)> = Vec::new();
    st.heat.retain(|who, h| {
        h.level *= decay;
        if h.level < 0.04 {
            return false;
        }
        let now = if h.player { player_muzzle } else { muzzles.iter().find(|(m, _)| m.reference == who.0).map(|(_, g)| muzzle_at(&root_aff, g).0) };
        if let Some(at) = now {
            h.at = at;
        }
        h.owed += h.level * WISPS_PER_HEAT * dt;
        while h.owed >= 1.0 {
            h.owed -= 1.0;
            hot.push((h.at, h.level, if h.player { 0.7 } else { 1.0 }));
        }
        true
    });
    for (at, heat, scale) in hot {
        st.wisp(at, heat, scale);
    }

    // Motion.
    let g = phys.world.params.gravity;
    let world_only = Filter { no_actors: true, no_dynamic: true, exclude_layers: layers::character_ignores(), ..Default::default() };
    for (i, pool) in st.pools.iter_mut().enumerate() {
        for p in &mut pool.ps {
            p.age += dt;
            if p.resting {
                continue;
            }
            p.vel *= (1.0 - p.drag * dt).max(0.0);
            p.vel.z -= g * p.fall * dt;
            p.angle += p.spin * dt;
            // Smoke and dust go with the air.
            if i == SMOKE || i == DUST {
                p.vel += (DRIFT - p.vel.with_z(0.0)) * (0.25 * dt).min(1.0);
            }
            let step = p.vel * dt;
            let len = step.length();
            if p.solid && len > 1e-3 {
                if let Some(h) = phys.ray(p.pos.to_array(), step.to_array(), len, world_only.clone()) {
                    let n = Vec3::from_array(h.normal);
                    p.pos = Vec3::from_array(h.point) + n * 0.3;
                    let v = p.vel - 2.0 * p.vel.dot(n) * n;
                    // Off the surface it keeps a little; along it, friction.
                    let vn = v.dot(n) * n;
                    p.vel = vn * 0.3 + (v - vn) * 0.5;
                    p.spin *= 0.5;
                    p.bounces += 1;
                    if p.vel.length() < 30.0 || p.bounces > 3 {
                        p.resting = true;
                        p.vel = Vec3::ZERO;
                    }
                    continue;
                }
            }
            p.pos += step;
        }
        pool.ps.retain(|p| p.age < p.life);
        st.most[i] = st.most[i].max(pool.ps.len());
    }

    // The pools' render entities, under the world root (a new root after a load).
    if st.root != Some(world_root) {
        st.root = Some(world_root);
        for pool in &mut st.pools {
            pool.entity = None;
            if let Some(m) = pool.mesh.take() {
                models.meshes.remove(&m);
            }
        }
    }
    let Ok(cam) = camera.single() else { return };
    // Unlit squares darkened with the place's ambient light (display values), so they do
    // not glow at night or indoors.
    let amb = ambient.as_deref().map_or(LinearRgba::WHITE, |a| a.color.to_linear());
    let lum = 0.3 * amb.red + 0.59 * amb.green + 0.11 * amb.blue;
    let shade = (lum / AMBIENT_FULL).clamp(0.22, 1.0);
    if !st.told_shade {
        st.told_shade = true;
        info!("impact effects: ambient light {lum:.3}, the smoke and dust shaded {shade:.2}");
    }
    let [right, up, back] = crate::particles::camera_basis(cam.rotation, &root_aff.inverse());
    for (i, pool) in st.pools.iter_mut().enumerate() {
        if pool.entity.is_none() {
            if pool.ps.is_empty() || st.missing[i] {
                continue;
            }
            let (path, _, glow) = LOOKS[i];
            let m = &mut models;
            let Some(texture) = m.textures.get(path, true, &m.data, &mut m.images, m.formats.as_deref()) else {
                st.missing[i] = true;
                warn!("impact effects: texture {path} not found; that pool is not drawn");
                continue;
            };
            let material = models.materials.add(nif_material(StandardMaterial {
                base_color: Color::WHITE,
                base_color_texture: Some(texture),
                unlit: true,
                alpha_mode: if glow { AlphaMode::Add } else { AlphaMode::Blend },
                perceptual_roughness: 1.0,
                reflectance: 0.0,
                double_sided: true,
                cull_mode: None,
                ..default()
            }));
            let mesh = models.meshes.add(particle_mesh());
            pool.entity = Some(commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material), Transform::IDENTITY, Visibility::Inherited, NotShadowCaster, NoFrustumCulling, ChildOf(world_root))).id());
            pool.mesh = Some(mesh);
        }
        let Some(mut mesh) = pool.mesh.as_ref().and_then(|h| models.meshes.get_mut(h)) else { continue };
        let frames = LOOKS[i].1;
        let n = pool.ps.len();
        let (mut pos, mut nor, mut uvs, mut col, mut idx) = (Vec::with_capacity(n * 4), Vec::with_capacity(n * 4), Vec::with_capacity(n * 4), Vec::with_capacity(n * 4), Vec::with_capacity(n * 6));
        let fw = 1.0 / frames as f32;
        for (k, p) in pool.ps.iter().enumerate() {
            let f = (p.age / p.life).clamp(0.0, 1.0);
            let (x, y) = if i == SPARKS {
                // Stretched along the flight as seen.
                let along = p.vel - back * p.vel.dot(back);
                let dirn = along.normalize_or(up);
                let len = (p.vel.length() * 0.022).clamp(0.8, 9.0);
                let side = back.cross(dirn).normalize_or(right);
                (side * p.size.0 * 0.5, dirn * len * 0.5)
            } else {
                let width = if p.solid { p.size.0 } else { p.size.0 + (p.size.1 - p.size.0) * f.sqrt() };
                let half = 0.5 * width;
                let (s, c) = p.angle.sin_cos();
                ((right * c + up * s) * half, (up * c - right * s) * half)
            };
            for corner in [-x - y, x - y, x + y, -x + y] {
                pos.push((p.pos + corner).to_array());
            }
            let (u, v) = ((p.frame % frames) as f32 * fw, (p.frame / frames % frames) as f32 * fw);
            uvs.extend([[u, v + fw], [u + fw, v + fw], [u + fw, v], [u, v]]);
            // In quickly, out slowly; chips hold until their last fifth.
            let a = match i {
                CHIPS => ((1.0 - f) * 5.0).min(1.0),
                SPARKS => (1.0 - f).powf(0.7),
                _ => (f / 0.08).min(1.0) * (1.0 - f).powf(1.4),
            };
            let c = if i == SPARKS { (p.color.truncate() * a).extend(1.0) } else { (p.color.truncate() * shade).extend(p.color.w * a) };
            col.extend([c.to_array(); 4]);
            nor.extend([back.to_array(); 4]);
            let b = (k * 4) as u32;
            idx.extend([b, b + 1, b + 2, b, b + 2, b + 3]);
        }
        if n == 0 {
            // An empty mesh: one degenerate triangle.
            pos = vec![[0.0; 3]; 3];
            nor = vec![[0.0, 0.0, 1.0]; 3];
            uvs = vec![[0.0; 2]; 3];
            col = vec![[0.0; 4]; 3];
            idx = vec![0, 1, 2];
        }
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nor);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
        mesh.insert_indices(Indices::U32(idx));
    }
}

fn report(mut exit: MessageReader<AppExit>, st: Res<FeelFx>) {
    if exit.read().last().is_some() && (st.shots > 0 || st.impacts > 0) {
        let mats: Vec<String> = st.told_materials.iter().map(|(m, n)| format!("{m} {n}")).collect();
        info!(
            "impact effects summary: {} shots smoked, {} impacts dressed ({}); born smoke {}, dust {}, chips {}, sparks {}; at most {}/{}/{}/{} at once",
            st.shots,
            st.impacts,
            mats.join(", "),
            st.born[SMOKE],
            st.born[DUST],
            st.born[CHIPS],
            st.born[SPARKS],
            st.most[SMOKE],
            st.most[DUST],
            st.most[CHIPS],
            st.most[SPARKS]
        );
    }
}
