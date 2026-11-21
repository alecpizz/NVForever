//! Modern Feel's damage direction (`ModernFeel::damage_direction`;
//! docs/design/modern-feel-kick.md) [design: every constant an engineering choice, tuned by
//! eye; none of it is the original's]:
//!
//! - **The mark**: a hit on the player by an attacker (`crate::combat::HitFeedback`'s
//!   `player_struck`, with who struck) puts an arc around the crosshair on the side the attacker is,
//!   as seen from the view: ahead at the top, behind at the bottom, turning as the player
//!   turns or the attacker moves. It fades over [`FADE_SECONDS`], brighter and longer for
//!   a bigger share of the player's health; another hit by the same attacker renews it;
//!   up to [`MOST`] at once. In the HUD's colour (`crate::interface::hud_rgb`), drawn with
//!   the HUD (hidden when it is). The arc's picture is made here, no file.
//! - **The kill**: the player's attack killing an actor plays a short, quiet tick made here
//!   ([`kill_tick`], no shipped file), on the effects volume.
//!
//! `damage direction: …` and `kill confirm: …` lines (smoke runs).

use crate::audio_out::SoundCategory;
use crate::combat::ActorHit;
use crate::hud::{HudSet, HudView};
use crate::interface::u;
use crate::modern_feel::ModernFeel;
use crate::sfx::{PcmSound, SfxSettings};
use crate::sim::SimState;
use crate::{space, PlaySet, PlayerCamera};
use bevy::asset::RenderAssetUsages;
use bevy::audio::{PlaybackSettings, Volume};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use fnv_sim::FormId;

/// Seconds a mark takes to fade (a full-strength one; weaker ones a little less).
pub const FADE_SECONDS: f32 = 1.5;
/// Marks at most.
pub const MOST: usize = 6;
/// The arc: its radius from the crosshair and its thickness, interface units (960 high);
/// half its sweep, degrees.
const RADIUS: f32 = 118.0;
const THICKNESS: f32 = 16.0;
const HALF_SWEEP_DEG: f32 = 24.0;
/// The picture's side, texels.
const TEXELS: u32 = 256;
/// The attacker's place is aimed at this high above its feet (units): its chest.
const CHEST: f32 = 90.0;
/// The kill tick's loudness (linear, on the effects volume).
const TICK_GAIN: f32 = 0.35;

/// One mark.
#[derive(Debug, Clone, Copy)]
struct Hit {
    by: FormId,
    /// The attacker's chest, game space, as last known.
    at: [f32; 3],
    strength: f32,
    age: f32,
    logged: bool,
}

#[derive(Resource, Default)]
struct Marks {
    hits: Vec<Hit>,
    picture: Option<Handle<Image>>,
    tick: Option<Handle<PcmSound>>,
    shown: usize,
    /// For the exit line.
    marked: u32,
    kills: u32,
}

#[derive(Component)]
struct Mark(usize);

/// The arc's picture: white, its alpha a ring segment with soft edges, brightest in its
/// middle (`size` texels square; the ring's middle at `radius` of the half side).
fn arc_picture(size: u32) -> Image {
    let half = size as f32 / 2.0;
    let outer = half - 2.0;
    let ring = outer * (RADIUS / (RADIUS + THICKNESS));
    let width = outer - ring;
    let half_sweep = HALF_SWEEP_DEG.to_radians();
    let mut px = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (dx, dy) = (x as f32 + 0.5 - half, half - (y as f32 + 0.5));
            let r = (dx * dx + dy * dy).sqrt();
            // Angle from straight up, clockwise.
            let a = dx.atan2(dy).abs();
            let across = ((r - ring) / width).clamp(-1.0, 2.0);
            // Soft at both edges, a little heavier on the outside.
            let radial = crate::ease::smoothstep(-0.05, 0.35, across) * (1.0 - crate::ease::smoothstep(0.7, 1.0, across));
            let along = 1.0 - crate::ease::smoothstep(0.55 * half_sweep, half_sweep, a);
            let middle = 0.6 + 0.4 * (1.0 - a / half_sweep).clamp(0.0, 1.0);
            let alpha = (radial * along * middle).clamp(0.0, 1.0);
            px.extend_from_slice(&[255, 255, 255, (alpha * 255.0).round() as u8]);
        }
    }
    Image::new(Extent3d { width: size, height: size, depth_or_array_layers: 1 }, TextureDimension::D2, px, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD)
}

/// The kill tick: a soft, short wooden "tock" falling in pitch, with a faint click on top
/// (mono samples at `rate`).
pub fn kill_tick(rate: u32) -> Vec<f32> {
    let len = (0.14 * rate as f32) as usize;
    let mut phase = [0.0f32; 3];
    (0..len)
        .map(|i| {
            let t = i as f32 / rate as f32;
            let attack = 1.0 - (-t / 0.0015).exp();
            // The body: 880 Hz gliding to 620, gone in about 0.1 s.
            let f = 620.0 + 260.0 * (-t / 0.025).exp();
            phase[0] += std::f32::consts::TAU * f / rate as f32;
            let body = phase[0].sin() * (-t / 0.032).exp();
            // An octave and a fifth over it, shorter: the "tick".
            phase[1] += std::f32::consts::TAU * f * 2.98 / rate as f32;
            let over = 0.25 * phase[1].sin() * (-t / 0.012).exp();
            // A faint click at the start.
            phase[2] += std::f32::consts::TAU * 3_100.0 / rate as f32;
            let click = 0.12 * phase[2].sin() * (-t / 0.004).exp();
            0.5 * attack * (body + over + click)
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn collect(
    mut commands: Commands,
    feel: Option<Res<ModernFeel>>,
    sim: Option<Res<SimState>>,
    sfx: Option<Res<SfxSettings>>,
    mut sounds: ResMut<Assets<PcmSound>>,
    mut hits: MessageReader<ActorHit>,
    mut marks: ResMut<Marks>,
    feedback: Option<ResMut<crate::combat::HitFeedback>>,
) {
    // The hits the player took (`HitFeedback::player_struck`), taken every frame.
    let struck = feedback.map(|mut f| std::mem::take(&mut f.player_struck)).unwrap_or_default();
    if !feel.as_deref().is_some_and(ModernFeel::damage_direction) {
        hits.clear();
        marks.hits.clear();
        return;
    }
    for (by, share) in struck {
        let Some(by) = by.filter(|b| *b != FormId::PLAYER_REF) else { continue };
        let place = sim.as_deref().and_then(|s| s.world.peek(by).map(|p| p.pos)).map(|p| [p[0], p[1], p[2] + CHEST]);
        let strength = 0.7 + 0.3 * crate::ease::smoothstep(0.0, 0.25, share);
        if let Some(m) = marks.hits.iter_mut().find(|m| m.by == by) {
            m.strength = m.strength.max(strength);
            m.age = 0.0;
            m.logged = false;
            if let Some(p) = place {
                m.at = p;
            }
        } else if let Some(at) = place {
            if marks.hits.len() >= MOST {
                // The oldest goes.
                if let Some(i) = marks.hits.iter().enumerate().max_by(|a, b| a.1.age.total_cmp(&b.1.age)).map(|(i, _)| i) {
                    marks.hits.remove(i);
                }
            }
            marks.hits.push(Hit { by, at, strength, age: 0.0, logged: false });
        }
        marks.marked += 1;
    }
    let mut killed = None;
    for h in hits.read() {
        if h.by == Some(FormId::PLAYER_REF) && h.died && h.form_id != FormId::PLAYER_REF.0 {
            killed = Some(h.form_id);
        }
    }
    let Some(victim) = killed else { return };
    marks.kills += 1;
    if !sfx.as_deref().is_none_or(|s| s.play) {
        return;
    }
    let tick = marks.tick.get_or_insert_with(|| sounds.add(PcmSound::from_samples(kill_tick(44_100), 1, 44_100))).clone();
    let volume = sfx.as_deref().map_or(1.0, |s| s.volume);
    commands.spawn((AudioPlayer::<PcmSound>(tick), PlaybackSettings::DESPAWN.with_volume(Volume::Linear(TICK_GAIN * volume)), SoundCategory::Effects));
    info!("kill confirm: the player killed {victim:08X}, a tick played");
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn draw(
    mut commands: Commands,
    time: Res<Time>,
    view: Res<HudView>,
    sim: Option<Res<SimState>>,
    mut images: ResMut<Assets<Image>>,
    cameras: Query<(&Camera, &GlobalTransform), With<PlayerCamera>>,
    mut nodes: Query<(&Mark, &mut UiTransform, &mut Visibility, &Children)>,
    mut pictures: Query<&mut ImageNode>,
    mut marks: ResMut<Marks>,
) {
    let dt = time.delta_secs();
    for m in &mut marks.hits {
        m.age += dt;
        // The attacker's place follows it while it is there.
        if let Some(p) = sim.as_deref().and_then(|s| s.world.peek(m.by).map(|p| p.pos)) {
            m.at = [p[0], p[1], p[2] + CHEST];
        }
    }
    marks.hits.retain(|m| m.age < FADE_SECONDS * (0.6 + 0.4 * m.strength));
    if marks.picture.is_none() {
        if marks.hits.is_empty() {
            return;
        }
        let _hud = crate::interface::hud_units();
        let picture = images.add(arc_picture(TEXELS));
        let side = 2.0 * (RADIUS + THICKNESS);
        for i in 0..MOST {
            commands
                .spawn((
                    Mark(i),
                    Node { position_type: PositionType::Absolute, left: Val::Percent(50.0), top: Val::Percent(50.0), width: Val::Px(0.0), height: Val::Px(0.0), ..default() },
                    UiTransform::default(),
                    Visibility::Hidden,
                    GlobalZIndex(3),
                    Pickable::IGNORE,
                ))
                .with_children(|p| {
                    p.spawn((
                        Node { position_type: PositionType::Absolute, left: u(-side / 2.0), top: u(-side / 2.0), width: u(side), height: u(side), ..default() },
                        ImageNode { color: Color::NONE, image_mode: NodeImageMode::Stretch, ..ImageNode::new(picture.clone()) },
                        Pickable::IGNORE,
                    ));
                });
        }
        info!("damage direction: arc picture made ({TEXELS} texels), {MOST} marks");
        marks.picture = Some(picture);
        return;
    }
    let camera = cameras.single().ok();
    let [r, g, b] = crate::interface::hud_rgb();
    let mut shown = 0;
    let mut lines = Vec::new();
    for (mark, mut turn, mut vis, children) in &mut nodes {
        let hit = marks.hits.get(mark.0).copied().filter(|_| view.shown);
        let place = hit.zip(camera).map(|(h, (_, t))| {
            let local = t.affine().inverse().transform_point3(space::to_bevy(h.at));
            // Ahead is up the screen, clockwise to the right: the camera's x right, -z ahead.
            (h, local.x.atan2(-local.z))
        });
        let Some((h, angle)) = place else {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        };
        let life = FADE_SECONDS * (0.6 + 0.4 * h.strength);
        let left = 1.0 - (h.age / life).clamp(0.0, 1.0);
        // Quickly in, then out along a curve that holds a moment.
        let alpha = h.strength * (h.age / 0.06).min(1.0) * left * left * (3.0 - 2.0 * left);
        turn.rotation = Rot2::radians(angle);
        for c in children.iter() {
            if let Ok(mut img) = pictures.get_mut(c) {
                img.color = Color::srgba_u8(r, g, b, (alpha.clamp(0.0, 1.0) * 255.0) as u8);
            }
        }
        vis.set_if_neq(Visibility::Inherited);
        shown += 1;
        if !h.logged {
            lines.push(format!("damage direction: hit by {:08X}, {:.0} deg from the view (clockwise), strength {:.2}", h.by.0, angle.to_degrees(), h.strength));
            if let Some(m) = marks.hits.get_mut(mark.0) {
                m.logged = true;
            }
        }
    }
    for l in lines {
        info!("{l}");
    }
    if shown != marks.shown {
        debug!("damage direction: {shown} shown");
        marks.shown = shown;
    }
}

fn report(mut exit: MessageReader<AppExit>, marks: Res<Marks>) {
    if exit.read().next().is_some() && (marks.marked > 0 || marks.kills > 0) {
        info!("damage direction summary: {} hits marked, {} kills confirmed", marks.marked, marks.kills);
    }
}

pub struct DamageDirectionPlugin;

impl Plugin for DamageDirectionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Marks>().add_message::<ActorHit>().add_systems(Update, (collect, draw).chain().after(HudSet).in_set(PlaySet::Ui)).add_systems(Last, report);
    }
}
