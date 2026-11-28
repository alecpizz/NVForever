//! Card game highlights: a short stamp and a sound for the big moments of a
//! game. There are no camera moves [design].
//!
//! - **The stamp** [design: icons too, like a little animation]: a Vault
//!   Boy picture in the HUD's colour, which hops twice for a hand won (the Luck Implant's
//!   cards), with a ring that bursts out behind it, and the moment's name under it in the
//!   HUD's font over a line that draws itself out. Then all of it rises a little and fades.
//!   Which picture for which moment is a [design] choice.
//! - **The pictures**: first a mod's own (`interface\icons\nvforever\highlight_*.dds`,
//!   a mod, not in this repository), else the game's Vault Boy art [observed: the files
//!   below are in Fallout - Textures2.bsa]; with neither, the words alone.
//! - **Which moments** [design]: blackjack and poker, a hand won.
//! - **The sounds** are SOUN records of the base game, found by Editor ID. A missing one
//!   falls back to `UIPopUpExperienceUp` [observed: the Editor IDs are in FalloutNV.esm;
//!   which one fits each moment: design, to be tuned by ear].
//! - **The words** are NVForever's own (LEGAL.md).
//!
//! A `flourish:` line in the log for each one.

use crate::casino_hud::CasinoFaces;
use crate::interface::{amber, at, bc_supported, set_marked, Picture, Pictures};
use crate::sfx::{PlaySound, Sfx};
use crate::PlaySet;
use bevy::asset::RenderAssetUsages;
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use fnv_sim::FormId;
use fnv_util::words::tr;
use nvf_assets::GameData;
use std::collections::HashMap;
use std::f32::consts::PI;

/// A big moment of a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Highlight {
    HandWon,
}

impl Highlight {
    /// The SOUN Editor IDs that may play for it (one picked at a time).
    pub fn sounds(self) -> &'static [&'static str] {
        match self {
            Highlight::HandWon => &["AMBCasinoChipsOS"],
        }
    }

    /// Its pictures, first choice first (paths under `textures\`).
    pub fn pictures(self) -> [&'static str; 2] {
        match self {
            Highlight::HandWon => [r"interface\icons\nvforever\highlight_hand_won.dds", r"interface\icons\pipboyimages\perks\perk_probability_calculator.dds"],
        }
    }

    /// When its rings burst, seconds after the stamp comes.
    fn rings(self) -> &'static [f32] {
        match self {
            Highlight::HandWon => &[0.0],
        }
    }

    fn parse(s: &str) -> Option<Highlight> {
        match s.trim().to_ascii_lowercase().as_str() {
            "handwon" => Some(Highlight::HandWon),
            _ => None,
        }
    }

    fn words(self) -> &'static str {
        match self {
            Highlight::HandWon => tr("YOU WIN THE HAND"),
        }
    }
}

/// Show a highlight: its stamp and its sound.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct Flourish {
    pub kind: Highlight,
    pub text: String,
}

const FALLBACK: &str = "UIPopUpExperienceUp";
/// How long a stamp stays, seconds: it comes in, holds, then rises and fades [design].
const IN: f32 = 0.35;
const HOLD: f32 = 1.6;
const OUT: f32 = 0.5;
pub const LIFE: f32 = IN + HOLD + OUT;
/// How long a ring takes to burst out and fade.
const RING: f32 = 0.5;

/// The stamp's layout, interface units: the picture's square, the box it sits in, and
/// how far from the view's top.
const ICON: f32 = 170.0;
const CARD: f32 = 640.0;
const TOP: f32 = 120.0;

/// Where every part of the stamp is at one moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    /// The picture's size (1 its own), turn (degrees) and offset (interface units).
    pub scale: f32,
    pub turn: f32,
    pub dx: f32,
    pub dy: f32,
    /// How strongly all of it is drawn.
    pub alpha: f32,
    /// The ring's size (times the picture's) and strength, while one bursts.
    pub ring: Option<(f32, f32)>,
    /// The words' size, and how much of the line under them is drawn (0 to 1).
    pub words: f32,
    pub rule: f32,
}

/// Rises past 1 and settles back (a "back" ease out).
fn overshoot(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0) - 1.0;
    let c = 1.70158;
    1.0 + (c + 1.0) * x * x * x + c * x * x
}

fn ease_out(x: f32) -> f32 {
    1.0 - (1.0 - x.clamp(0.0, 1.0)).powi(3)
}

/// The stamp of `kind` at `age` seconds.
pub fn pose(kind: Highlight, age: f32) -> Pose {
    let after = age - IN;
    let mut scale = overshoot(age / IN);
    let (mut turn, dx, mut dy) = (0.0, 0.0, 0.0);
    match kind {
        Highlight::HandWon => {
            if (0.0..0.7).contains(&after) {
                let fall = 1.0 - after / 0.7;
                let w = after / 0.35 * PI;
                dy = -26.0 * w.sin().abs() * fall;
                turn = 6.0 * w.sin() * fall;
            }
        }
    }
    let out = ((age - IN - HOLD) / OUT).clamp(0.0, 1.0);
    scale *= 1.0 - 0.15 * out;
    dy -= 30.0 * out;
    let alpha = (age / 0.08).clamp(0.0, 1.0) * (1.0 - out);
    let ring = kind.rings().iter().rev().map(|&t| age - t).find(|&r| r >= 0.0).filter(|&r| r < RING).map(|r| {
        let e = r / RING;
        (0.6 + 1.6 * ease_out(e), 0.8 * (1.0 - e))
    });
    let words = overshoot((age - 0.1) / 0.3).max(0.0) * (1.0 - 0.1 * out);
    let rule = ease_out((age - 0.15) / 0.4);
    Pose { scale, turn, dx, dy, alpha, ring, words, rule }
}

#[derive(Component)]
struct Stamp {
    born: f32,
    kind: Highlight,
}

/// The stamp's parts, each moved by [`animate`].
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Part {
    Icon,
    Ring,
    Words,
    Glyph,
    Rule,
}

/// What stamps are drawn with: the pictures read so far and the ring.
#[derive(Default)]
struct Kit {
    pictures: Option<Pictures>,
    icons: HashMap<Highlight, Option<Picture>>,
    ring: Option<Handle<Image>>,
}

/// A soft ring, white on clear, `n` texels across.
fn ring_image(n: u32) -> Image {
    let mut px = Vec::with_capacity((n * n * 4) as usize);
    let c = (n as f32 - 1.0) / 2.0;
    for y in 0..n {
        for x in 0..n {
            let r = ((x as f32 - c).hypot(y as f32 - c)) / c;
            // Brightest at 0.86 of the radius, gone by 0.72 and at the edge.
            let a = (1.0 - ((r - 0.86) / 0.14).abs()).clamp(0.0, 1.0);
            px.extend_from_slice(&[255, 255, 255, (a * a * 255.0) as u8]);
        }
    }
    Image::new(Extent3d { width: n, height: n, depth_or_array_layers: 1 }, TextureDimension::D2, px, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default())
}

/// Poker: the hands won whose time came (`poker::Session::wins`, aid seconds).
fn poker_wins(time: crate::pause::AidTime, mut poker: ResMut<crate::poker::PokerScreen>, mut out: MessageWriter<Flourish>) {
    let Some(s) = poker.0.as_mut() else { return };
    let now = time.elapsed_secs();
    if s.wins.is_empty() {
        return;
    }
    let (due, later): (Vec<_>, Vec<_>) = s.wins.drain(..).partition(|w| w.0 <= now);
    s.wins = later;
    for (_, text) in due {
        out.write(Flourish { kind: Highlight::HandWon, text });
    }
}

/// The stamps and the sounds.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn show(
    mut commands: Commands,
    time: Res<Time<Real>>,
    mut asked: MessageReader<Flourish>,
    sfx: Option<Res<Sfx>>,
    (data, formats, casino): (Option<Res<GameData>>, Option<Res<CompressedImageFormatSupport>>, Option<Res<CasinoFaces>>),
    mut images: ResMut<Assets<Image>>,
    mut kit: Local<Kit>,
    mut cache: Local<HashMap<&'static str, Option<FormId>>>,
    mut turn: Local<u32>,
    mut sounds: MessageWriter<PlaySound>,
    old: Query<Entity, With<Stamp>>,
) {
    let now = time.elapsed_secs();
    let bc = bc_supported(formats.as_deref());
    let plain;
    let faces = match casino.as_deref() {
        Some(c) => c,
        None => {
            plain = CasinoFaces::plain();
            &plain
        }
    };
    // Several asked in one frame: the last one only (a stamp spawned this frame is not in
    // `old` yet, so they would lie over each other).
    let all: Vec<Flourish> = asked.read().cloned().collect();
    if all.len() > 1 {
        info!("flourish: {} at once, the last shown", all.len());
    }
    if let Some(f) = all.last() {
        info!("flourish: {:?} {:?}", f.kind, f.text);
        // A new stamp takes the place of one still showing.
        for e in &old {
            commands.entity(e).despawn();
        }
        let data = data.as_deref();
        let icon = match kit.icons.get(&f.kind) {
            Some(i) => i.clone(),
            None => {
                let pictures = kit.pictures.get_or_insert_with(|| Pictures::load(data));
                let i = f.kind.pictures().into_iter().find_map(|p| pictures.picture(p, data, &mut images, bc));
                match &i {
                    Some(_) => info!("flourish: picture for {:?}", f.kind),
                    None => warn!("flourish: no picture for {:?}", f.kind),
                }
                kit.icons.insert(f.kind, i.clone());
                i
            }
        };
        let ring = kit.ring.get_or_insert_with(|| images.add(ring_image(128))).clone();
        let title = &faces.title;
        let words_w = title.font.width(&f.text);
        let glow = amber(255.0).with_alpha(0.0);
        let x0 = (CARD - ICON) / 2.0;
        commands
            .spawn((
                Stamp { born: now, kind: f.kind },
                Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), top: crate::interface::u(TOP), justify_content: JustifyContent::Center, ..default() },
                // Over the card games' screens (106), under message boxes (108) and the
                // pause menu (110).
                GlobalZIndex(107),
                Pickable::IGNORE,
            ))
            .with_children(|root| {
                root.spawn(Node { width: crate::interface::u(CARD), height: crate::interface::u(ICON + 90.0), ..default() }).with_children(|card| {
                    card.spawn((at(x0, 0.0, ICON, ICON), ImageNode { color: glow, ..ImageNode::new(ring) }, UiTransform::default(), Part::Ring));
                    if let Some((image, rect)) = icon {
                        card.spawn((at(x0, 0.0, ICON, ICON), ImageNode { rect, color: glow, image_mode: NodeImageMode::Stretch, ..ImageNode::new(image) }, UiTransform::default(), Part::Icon));
                    }
                    card.spawn((at(0.0, ICON + 8.0, CARD, 60.0), UiTransform::default(), Part::Words)).with_children(|w| {
                        if let Some(line) = title.font.lines(&f.text, None).into_iter().next() {
                            set_marked(w, title, &line, (CARD - words_w) / 2.0, 44.0, glow, Part::Glyph);
                        }
                    });
                    let rw = words_w + 60.0;
                    card.spawn((at((CARD - rw) / 2.0, ICON + 66.0, rw, 2.0), BackgroundColor(glow), UiTransform::default(), Part::Rule));
                });
            });
        let names = f.kind.sounds();
        *turn = turn.wrapping_add(1);
        let name = names[*turn as usize % names.len()];
        let Some(sfx) = sfx.as_deref() else { return };
        let mut find = |n: &'static str| *cache.entry(n).or_insert_with(|| sfx.forms.sounds.iter().find(|(_, s)| s.edid.as_deref().is_some_and(|e| e.eq_ignore_ascii_case(n))).map(|(id, _)| *id));
        match find(name).or_else(|| find(FALLBACK)) {
            Some(sound) => {
                info!("flourish: sound {name}");
                sounds.write(PlaySound { sound, at: None });
            }
            None => warn!("flourish: sound {name} not found"),
        }
    }
}

/// Moves the stamp's parts to their [`pose`]; gone when its time is up.
#[allow(clippy::type_complexity)]
fn animate(
    mut commands: Commands,
    time: Res<Time<Real>>,
    stamps: Query<(Entity, &Stamp)>,
    mut parts: Query<(&Part, Option<&mut UiTransform>, Option<&mut ImageNode>, Option<&mut BackgroundColor>, Option<&mut TextColor>)>,
) {
    let now = time.elapsed_secs();
    let Some((e, s)) = stamps.iter().next() else { return };
    let age = now - s.born;
    if age >= LIFE {
        commands.entity(e).despawn();
        return;
    }
    let p = pose(s.kind, age);
    let lit = |a: f32| amber(255.0).with_alpha(a.clamp(0.0, 1.0));
    for (part, transform, image, fill, text) in &mut parts {
        let (color, look) = match part {
            Part::Icon => (lit(p.alpha), Some(UiTransform { translation: Val2::new(crate::interface::u(p.dx), crate::interface::u(p.dy)), scale: Vec2::splat(p.scale.max(0.0)), rotation: Rot2::degrees(p.turn) })),
            Part::Ring => {
                let (size, strength) = p.ring.unwrap_or((1.0, 0.0));
                (lit(strength * p.alpha), Some(UiTransform { translation: Val2::new(crate::interface::u(0.0), crate::interface::u(p.dy)), scale: Vec2::splat(size), ..default() }))
            }
            Part::Words => (lit(p.alpha), Some(UiTransform { translation: Val2::new(crate::interface::u(0.0), crate::interface::u(p.dy * 0.5)), scale: Vec2::splat(p.words.max(0.0)), ..default() })),
            Part::Glyph => (lit(p.alpha), None),
            Part::Rule => (lit(0.8 * p.alpha), Some(UiTransform { translation: Val2::new(crate::interface::u(0.0), crate::interface::u(p.dy * 0.5)), scale: Vec2::new(p.rule, 1.0), ..default() })),
        };
        if let (Some(mut t), Some(l)) = (transform, look) {
            *t = l;
        }
        if let Some(mut i) = image {
            i.color = color;
        }
        // Every node has a background colour (transparent): only the rule is drawn by it.
        if let Some(mut b) = fill.filter(|_| *part == Part::Rule) {
            b.0 = color;
        }
        if let Some(mut t) = text {
            t.0 = color;
        }
    }
}

/// `NVF_PLAY_TEST_FLOURISH=<seconds>:<kind>[,...]` (kind: handwon): show that highlight
/// at each time, on the aids' clock, as a game would. A
/// test aid to see the stamps; not a game behaviour.
fn test_aid(time: crate::pause::AidTime, mut due: Local<Option<Vec<(f32, Highlight)>>>, mut out: MessageWriter<Flourish>) {
    let due = due.get_or_insert_with(|| std::env::var("NVF_PLAY_TEST_FLOURISH").map(|v| aid_items(&v)).unwrap_or_default());
    let now = time.elapsed_secs();
    due.retain(|&(t, kind)| {
        if t > now {
            return true;
        }
        info!("test aid: flourish {kind:?} at {t}s");
        out.write(Flourish { kind, text: kind.words().into() });
        false
    });
}

fn aid_items(v: &str) -> Vec<(f32, Highlight)> {
    v.split(',').filter_map(|i| i.split_once(':')).filter_map(|(t, k)| Some((t.trim().parse().ok()?, Highlight::parse(k)?))).collect()
}

pub struct FlourishPlugin;

impl Plugin for FlourishPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Flourish>().add_systems(Update, (test_aid, poker_wins, show, animate).chain().after(PlaySet::Ui));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stamp_comes_in_holds_and_goes() {
        let kind = Highlight::HandWon;
        assert_eq!(pose(kind, 0.0).alpha, 0.0);
        let held = pose(kind, IN + 1.0);
        assert_eq!((held.alpha, held.scale, held.turn, held.rule), (1.0, 1.0, 0.0, 1.0));
        assert!(pose(kind, LIFE).alpha <= 0.0);
        // The pop rises past full size before it settles.
        assert!(pose(kind, IN * 0.6).scale > 1.0);
        // Its ring bursts as it comes.
        assert!(pose(kind, 0.1).ring.is_some());
        assert!(pose(kind, RING + 0.01).ring.is_none());
    }

    #[test]
    fn the_aid_reads_its_times_and_kinds() {
        assert_eq!(aid_items("5:handwon, 8.5:HandWon,9:nope,x:handwon"), [(5.0, Highlight::HandWon), (8.5, Highlight::HandWon)]);
    }
}
