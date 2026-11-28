//! Actors other than the player swimming (owner's request 2026-10-18): while an actor's
//! character swims (`fnv_phys::Character::swimming`, the rules of
//! docs/spec-notes/physics.md §6.6), its locomotion plays the swimming clips of its
//! skeleton's folder in place of the standing ones (`locomotion::Locomotion::set_swimming`),
//! and walks again once its character is back on the ground.
//!
//! - **Which clips**: people use `characters\_male\swimmt…` (idle, the four directions
//!   walking and running, the turns), the clips the player's body swims on too
//!   (`fnv_actor::player_clips`, `Stance::Swim`); the mirelurks, giant rat, spinebreaker
//!   and dog have their own (`locomotion::SWIM_STANCES`) [observed: the base meshes
//!   archive listing]. A move without a swimming clip keeps its standing clip; skeletons
//!   without any swimming movement clip swim on their standing clips.
//! - **Weapons**: a swimmer moves unarmed, as the player's body holsters in water
//!   [design].
//! - **Standing on the bed** (#2980): an actor whose character swims but stands still
//!   with its feet on the bed ([`rests_now`]) plays its standing clips, not its
//!   swimming idle. Actors give no vertical input, so an actor with a live character on
//!   a lake bed deeper than `wade_depth` swims from that character's first step, held
//!   at the bed with no buoyancy (physics.md §6.6, *Walking in again*), and played its
//!   swimming idle (the mirelurk king's `swimidle.kf` for a lakelurk) whenever it stood
//!   still there. Only an actor on its way keeps a character: `residents.rs` takes it
//!   from one that stands idle, and from a fighter holding its ground in range
//!   (`fnv_ai` alert: `Search::Holding`, activity There) unless an attack carries it;
//!   such an actor plays its standing set whatever the water. A swimmer stands still
//!   with a character only for a moment (waiting at the end of its way for a target
//!   that moves, `Search::Closing`). So this is not shown to be the T-pose of #2980 (a lakelurk
//!   idle until it fought) [hypothesis]; the pose watch below and the base layer
//!   warning (`locomotion.rs`) are there to tell. "Swimming is when your feet do not
//!   touch solid ground" [documented: GECK wiki fSwimRadiationDamageMult]; whether the
//!   original stands such an actor on the bed is observer check 269 [hypothesis]. It
//!   comes to rest only once its stop has faded into the swimming idle, and sets off on
//!   its standing walk before it changes to the swimming walk, so no clip still showing
//!   is cut off ([`rests_now`]) [design]. Moving, it swims on its swimming clips as before, so
//!   actors that follow the player into the water still swim [design]. The physics
//!   (`fnv_phys`, the player's and every swimmer's thresholds) is unchanged.
//! - **Pose watch** (test aid `NVF_PLAY_TEST_POSE_WATCH=[<seconds>:]<text>[,<text>...]`):
//!   from `<seconds>` into the run on (default 0), for each actor whose name
//!   (`Creature <ref> <EDID>`, `Actor <ref> <EDID>`) contains one of the texts (any
//!   case), a line whenever what it plays changes: the clips on its base layer (layer 0)
//!   and the one fading out (layer 1) with their weights to a tenth, the set it plays
//!   from, its idle layers, whether it has a character (`PhysState::npcs`) and whether
//!   that swims and rests on the bed ([`pose_line`]). A fade thus writes a few lines and
//!   its end one with the base clip alone at weight 1.
//! - **Co-op players**: the other players' bodies here are no characters of this game's
//!   physics; they swim while their own games say so (`nvf_coop::net::more_flag::SWIMMING`),
//!   handed over in [`Swimmers`].

use crate::limbs::ActorRig;
use crate::locomotion::Locomotion;
use bevy::prelude::*;
use nvf_assets::anim_play::BlendPlayer;
use fnv_actor::locomotion::{Move, STAND_SPEED};
use nvf_play::phys::PhysState;
use std::collections::BTreeSet;

/// Feet at most this far above the support under them stand on the bed, game units: the
/// character's `ground_gap`, which is 0 for a swimmer held against the bed (within the
/// contact shell) and at least 9 for one floating at `float_depth` over a bed deep enough
/// to keep it swimming (`wade_depth` 115 less `float_depth` 102.5, less the 3.5 shell)
/// [design].
pub const BED_GAP: f32 = 2.0;

/// Whether a swimming actor stands still on the bed and so plays its standing clips: its
/// feet within [`BED_GAP`] of the support under them (`ground_gap`, `None`: nothing in
/// reach), slower than [`STAND_SPEED`] and with no walk or run playing (`mv`; a turn on
/// the spot counts as standing) [design, #2980].
pub fn rests_on_bed(ground_gap: Option<f32>, speed: f32, mv: Move) -> bool {
    ground_gap.is_some_and(|g| g <= BED_GAP) && speed < STAND_SPEED && !mv.is_moving()
}

/// Whether a swimming actor (`swims`) rests on the bed now. The set changes only with no
/// fade running (`blend.prev`), as `Locomotion::switch_set` cuts a running fade short and
/// drops its older clip [design]:
/// - not resting, it comes to rest by [`rests_on_bed`] (its character's `ground_gap`, its
///   locomotion's speed and move) once its stop has faded into the swimming idle;
/// - resting (`was_resting`), it stays so while it is on the bed and plays no walk or run
///   (a turn on the spot and its fade keep it), and while any fade runs: setting off, it
///   first walks on its standing walk until that has faded in, then changes to the
///   swimming walk at the same footfall (one cross-fade, no idle between).
pub fn rests_now(swims: bool, ground_gap: Option<f32>, lo: &Locomotion, was_resting: bool) -> bool {
    if !swims {
        return false;
    }
    let fading = lo.blend.prev.is_some();
    if was_resting {
        let stays = ground_gap.is_some_and(|g| g <= BED_GAP) && !lo.blend.cur.mv.is_moving();
        stays || fading
    } else {
        rests_on_bed(ground_gap, lo.velocity[0].hypot(lo.velocity[1]), lo.blend.cur.mv) && !fading
    }
}

/// What [`follow_water`] did to an actor's locomotion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaterChange {
    None,
    /// It went into the water: the swimming set plays.
    Swims,
    /// It left the water: the standing set plays.
    Leaves,
    /// Its character swims, standing still on the bed: the standing set plays.
    Rests,
    /// It sets off from the bed, still in the water: the swimming set plays again.
    SetsOff,
    /// Its character swims and its skeleton has no swimming clips.
    NoClips,
}

/// Follow the actor's character into and out of the water (`swims`: it swims; `rests`:
/// [`rests_on_bed`]; `was_resting`: it rested last time).
pub fn follow_water(lo: &mut Locomotion, swims: bool, rests: bool, was_resting: bool) -> WaterChange {
    let want = swims && !rests;
    if want == lo.swimming {
        return WaterChange::None;
    }
    if !lo.set_swimming(want) {
        return if want { WaterChange::NoClips } else { WaterChange::None };
    }
    // A change of set by the bed rest: a creature's walk waits for its fade's end
    // (`Locomotion::bed_fade`).
    lo.bed_fade = rests || was_resting;
    match (want, rests || was_resting) {
        (true, false) => WaterChange::Swims,
        (false, false) => WaterChange::Leaves,
        (true, true) => WaterChange::SetsOff,
        (false, true) => WaterChange::Rests,
    }
}

pub struct SwimmingPlugin;

impl Plugin for SwimmingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SwimStats>()
            .init_resource::<Swimmers>()
            .add_systems(Update, swim.after(nvf_play::PlaySet::Scripts).before(crate::locomotion::walk_test))
            .add_systems(Update, pose_watch.after(crate::locomotion::drive).run_if(|| watch_texts().is_some()))
            .add_systems(Last, report);
    }
}

/// References swimming by another game's word: the co-op players whose own games have them
/// in the water (`coop.rs` fills it each frame).
#[derive(Resource, Default)]
pub struct Swimmers(pub std::collections::HashSet<u32>);

#[derive(Resource, Default)]
struct SwimStats {
    /// Times an actor started swimming (into the water, or setting off from the bed), and
    /// left the water; actors that swam without swimming clips (counted once each).
    started: usize,
    stopped: usize,
    no_clips: BTreeSet<u32>,
    /// Actors that stood still on the bed while their character swam (counted once
    /// each), and those standing so now.
    rested: BTreeSet<u32>,
    resting: BTreeSet<u32>,
}

fn swim(phys: Option<Res<PhysState>>, swimmers: Res<Swimmers>, mut actors: Query<(&ActorRig, &mut Locomotion)>, mut stats: ResMut<SwimStats>) {
    let Some(phys) = phys else { return };
    for (rig, mut lo) in &mut actors {
        let r = rig.reference;
        let ch = phys.npcs.get(&r).map(|n| &n.ch);
        let swims = swimmers.0.contains(&r) || ch.is_some_and(|c| c.swimming);
        let was_resting = stats.resting.contains(&r);
        let rests = rests_now(swims, ch.and_then(|c| c.ground_gap), &lo, was_resting);
        if rests {
            stats.resting.insert(r);
        } else {
            stats.resting.remove(&r);
        }
        if rests && stats.rested.insert(r) {
            info!("swimming: {r:08X} rests on the bed under water: it stands on its standing clips");
        }
        match follow_water(&mut lo, swims, rests, was_resting) {
            WaterChange::Swims => {
                stats.started += 1;
                info!("swimming: {r:08X} swims");
            }
            WaterChange::Leaves => {
                stats.stopped += 1;
                info!("swimming: {r:08X} is out of the water");
            }
            WaterChange::SetsOff => {
                stats.started += 1;
                debug!("swimming: {r:08X} sets off from the bed, swimming");
            }
            WaterChange::Rests => debug!("swimming: {r:08X} stands still on the bed"),
            WaterChange::NoClips => {
                if stats.no_clips.insert(r) {
                    info!("swimming: {r:08X} swims but has no swimming clips: it keeps its standing clips");
                }
            }
            WaterChange::None => {}
        }
    }
}

/// `NVF_PLAY_TEST_POSE_WATCH`: from when (seconds into the run), and the texts, lower
/// case (`None`: the aid is off).
fn watch_texts() -> Option<&'static (f32, Vec<String>)> {
    static WATCH: std::sync::OnceLock<Option<(f32, Vec<String>)>> = std::sync::OnceLock::new();
    WATCH.get_or_init(|| parse_watch(&std::env::var("NVF_PLAY_TEST_POSE_WATCH").ok()?)).as_ref()
}

/// `[<seconds>:]<text>[,<text>...]` ([`watch_texts`]).
pub fn parse_watch(v: &str) -> Option<(f32, Vec<String>)> {
    let (from, rest) = match v.split_once(':') {
        Some((t, rest)) if t.trim().parse::<f32>().is_ok() => (t.trim().parse::<f32>().unwrap_or(0.0), rest),
        _ => (0.0, v),
    };
    let texts: Vec<String> = rest.split(',').map(|t| t.trim().to_ascii_lowercase()).filter(|t| !t.is_empty()).collect();
    (!texts.is_empty()).then_some((from, texts))
}

/// The pose watch's line for one actor after its layers were assembled (weights to a
/// tenth, so the line changes when a fade moves on by that much). `character`: `None`
/// without a character, else whether it swims and whether it rests on the bed.
pub fn pose_line(lo: &Locomotion, player: &BlendPlayer, character: Option<(bool, bool)>) -> String {
    let clip = |k: usize| player.layers[k].as_ref().map(|l| (lo.clip_path(&l.clip).map_or_else(|| format!("sequence {:?}", l.clip.clip.name), str::to_string), l.weight));
    let base = clip(0).map_or_else(|| "none".to_string(), |(p, _)| p);
    let base_weight = clip(0).map_or_else(String::new, |(_, w)| format!(" (weight {w:.1})"));
    let fading = clip(1).map_or_else(String::new, |(p, w)| format!(", layer 1 {p} (weight {w:.1})"));
    let idle_layers = crate::idles::IDLE_LAYER..crate::idles::IDLE_LAYER + 2;
    let idles: Vec<String> = idle_layers.filter_map(|k| player.layers[k].as_ref().map(|l| format!("{:?} (weight {:.1})", l.clip.clip.name, l.weight))).collect();
    let character = match character {
        None => "no character",
        Some((false, _)) => "a character, not swimming",
        Some((true, false)) => "a character, swimming",
        Some((true, true)) => "a character, swimming, resting on the bed",
    };
    let set = lo.posture_name();
    let idle_text = if idles.is_empty() { "none".to_string() } else { idles.join(", ") };
    format!("layer 0 {base}{base_weight}{fading} from its {set} set; idle layers {idle_text}; {character}")
}

/// `NVF_PLAY_TEST_POSE_WATCH` ([`watch_texts`]): after the actors are posed, a line for
/// each watched actor whenever its [`pose_line`] changes.
fn pose_watch(
    time: Res<Time>,
    phys: Option<Res<PhysState>>,
    actors: Query<(Entity, &ActorRig, &Name, &Locomotion, &BlendPlayer)>,
    stats: Res<SwimStats>,
    mut last: Local<std::collections::HashMap<Entity, String>>,
) {
    let Some((from, texts)) = watch_texts() else { return };
    if time.elapsed_secs() < *from {
        return;
    }
    for (e, rig, name, lo, player) in &actors {
        let lower = name.as_str().to_ascii_lowercase();
        if !texts.iter().any(|t| lower.contains(t.as_str())) {
            continue;
        }
        let character = phys.as_deref().and_then(|p| p.npcs.get(&rig.reference)).map(|n| (n.ch.swimming, stats.resting.contains(&rig.reference)));
        let line = pose_line(lo, player, character);
        if last.get(&e) != Some(&line) {
            info!("pose watch: {}: {line}", name.as_str());
            last.insert(e, line);
        }
    }
}

fn report(mut exit: MessageReader<AppExit>, stats: Res<SwimStats>) {
    if exit.read().next().is_some() {
        info!(
            "swimming summary: actors started swimming {} times and left the water {} times; {} swam without swimming clips; {} stood still on the bed",
            stats.started,
            stats.stopped,
            stats.no_clips.len(),
            stats.rested.len()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::locomotion::{ClipSet, SetClip};
    use fnv_actor::locomotion::ClipInfo;
    use nvf_assets::anim_play::{BoundClip, Layer};
    use std::sync::Arc;

    const DT: f32 = 1.0 / 60.0;

    /// A set of an idle, a walk and the turns, its files named `<prefix><move>.kf`, each
    /// clip its own (infos only: choosing and blending need no tracks).
    fn clips(prefix: &str) -> Arc<ClipSet> {
        let mut s = ClipSet::default();
        for (m, duration, speed) in [(Move::Idle, 3.3, 0.0), (Move::Forward, 1.6, 30.16), (Move::TurnLeft, 1.0, 0.0), (Move::TurnRight, 1.0, 0.0)] {
            let bound = Arc::new(BoundClip {
                clip: Arc::new(nvf_assets::anim_play::Clip {
                    name: String::new(), start: 0.0, stop: 1.0, frequency: 1.0, cycle_type: 0, tracks: Vec::new(), priorities: Vec::new(),
                    skipped: 0, accum_root: None, text_keys: Vec::new(), visibility: Vec::new(), emitters: Vec::new(), materials: Vec::new(),
                    free: Vec::new(), free_visibility: Vec::new(), lights: Vec::new(),
                }),
                tracks: Vec::new(), root: None, root_track: None, carried: None, hold_xy: None, unbound: Default::default(),
            });
            s.clips.insert(m, SetClip { bound, info: ClipInfo { duration, speed, foot_phase: 0.0, blend_time: 0.2 }, path: format!("{prefix}{}.kf", m.group().to_ascii_lowercase()) });
        }
        Arc::new(s)
    }

    fn idle_path(lo: &Locomotion) -> &str {
        &lo.set.clips[&Move::Idle].path
    }

    /// One frame of [`swim`] for one actor, `resting` its state from the frame before.
    fn frame(lo: &mut Locomotion, swims: bool, ground_gap: Option<f32>, resting: &mut bool) -> WaterChange {
        let rests = rests_now(swims, ground_gap, lo, *resting);
        let change = follow_water(lo, swims, rests, *resting);
        *resting = rests;
        change
    }

    /// The files and weights on layers 0 and 1 after the layers are assembled.
    fn shown(lo: &mut Locomotion, player: &mut BlendPlayer) -> Vec<(String, f32)> {
        lo.layers(player);
        (0..2).filter_map(|k| player.layers[k].as_ref().map(|l| (lo.clip_path(&l.clip).expect("a clip of the sets").to_string(), l.weight))).collect()
    }

    fn weight_of(shown: &[(String, f32)], path: &str) -> f32 {
        shown.iter().filter(|(p, _)| p == path).map(|(_, w)| w).sum()
    }

    #[test]
    fn the_pose_watch_reads_its_start_and_texts() {
        assert_eq!(parse_watch("lakelurk"), Some((0.0, vec!["lakelurk".to_string()])));
        assert_eq!(parse_watch("9:Lakelurk, Gecko"), Some((9.0, vec!["lakelurk".to_string(), "gecko".to_string()])));
        assert_eq!(parse_watch("9:"), None);
        assert_eq!(parse_watch(""), None);
    }

    #[test]
    fn on_the_bed_only_a_still_actor_stands() {
        assert!(rests_on_bed(Some(0.0), 0.0, Move::Idle));
        assert!(rests_on_bed(Some(BED_GAP), 4.9, Move::TurnLeft), "a turn on the spot stands");
        assert!(!rests_on_bed(Some(0.0), STAND_SPEED, Move::Idle), "moving");
        assert!(!rests_on_bed(Some(0.0), 0.0, Move::Forward), "a walk still playing");
        // Floating at float_depth over a bed deep enough to swim on: 9 units or more up.
        assert!(!rests_on_bed(Some(9.0), 0.0, Move::Idle));
        assert!(!rests_on_bed(None, 0.0, Move::Idle), "no bed in reach");
    }

    /// `n` frames on the bed (the swim system, the step, the layers), each checked: the
    /// two layers share the whole weight and no clip's weight moves more than a fade's
    /// steepest step. Returns the set changes by frame, and the last layers in `before`.
    fn on_bed(lo: &mut Locomotion, player: &mut BlendPlayer, resting: &mut bool, n: usize, before: &mut Vec<(String, f32)>) -> Vec<(usize, WaterChange)> {
        let mut changes = Vec::new();
        for i in 0..n {
            let change = frame(lo, true, Some(0.0), resting);
            if change != WaterChange::None {
                changes.push((i, change));
            }
            lo.step(DT);
            let now = shown(lo, player);
            let total: f32 = now.iter().map(|(_, w)| w).sum();
            assert!((total - 1.0).abs() < 1e-4, "frame {i}: {now:?}");
            for path in ["swimforward.kf", "swimidle.kf", "mtidle.kf", "mtforward.kf"] {
                let (a, b) = (weight_of(before, path), weight_of(&now, path));
                assert!((a - b).abs() <= 0.15, "frame {i}: {path} from {a} to {b}: {before:?} -> {now:?}");
            }
            *before = now;
        }
        changes
    }

    fn alone(path: &str) -> Vec<(String, f32)> {
        vec![(path.to_string(), 1.0)]
    }

    /// #2980: an actor with a live character on a lake bed deeper than `wade_depth` swims
    /// from that character's first step, held at the bed; standing still it played its
    /// swimming idle there. It now stands on its standing idle, swims on its swimming
    /// clips once it moves, and stands again when it stops, with no clip cut off.
    #[test]
    fn a_creature_standing_on_a_deep_bed_plays_its_standing_idle() {
        let (stand, swim) = (clips("mt"), clips("swim"));
        let mut lo = Locomotion::new(stand.clone(), 0.0);
        lo.swim = Some(swim.clone());
        lo.blend.set_off_at_once = true;
        let mut player = BlendPlayer::new(Vec::new(), Arc::new(Vec::new()));
        let mut resting = false;
        let mut before = shown(&mut lo, &mut player);
        // Its character swims at the bed (ground gap 0), standing still.
        assert!(on_bed(&mut lo, &mut player, &mut resting, 10, &mut before).is_empty());
        assert!(resting && !lo.swimming);
        assert!(Arc::ptr_eq(&lo.set, &stand));
        assert_eq!(idle_path(&lo), "mtidle.kf", "the standing idle, not swimidle.kf");
        assert_eq!(before, alone("mtidle.kf"));
        // It sets off along the bed: the standing walk fades in, then the swimming walk
        // takes over from it (no idle between, nothing dropped).
        lo.velocity = [0.0, 30.0];
        let changes = on_bed(&mut lo, &mut player, &mut resting, 60, &mut before);
        assert_eq!(changes.len(), 1, "{changes:?}");
        assert_eq!(changes[0].1, WaterChange::SetsOff);
        assert!(lo.swimming && Arc::ptr_eq(&lo.set, &swim));
        assert_eq!(before, alone("swimforward.kf"));
        // It stops. The walk fades into the swimming idle first, then the swimming idle
        // into the standing one.
        lo.velocity = [0.0; 2];
        let mut at = None;
        for i in 0..120 {
            let was = before.clone();
            let changes = on_bed(&mut lo, &mut player, &mut resting, 1, &mut before);
            if let Some((_, c)) = changes.first() {
                assert_eq!((*c, at), (WaterChange::Rests, None), "rests once");
                assert_eq!(was, alone("swimidle.kf"), "comes to rest from the swimming idle alone");
                at = Some(i);
            }
        }
        assert!(at.is_some());
        assert!(!lo.swimming && Arc::ptr_eq(&lo.set, &stand));
        assert_eq!(before, alone("mtidle.kf"));
        // Turning on the spot while resting, and the turn's fade back to the idle, keep
        // it standing.
        lo.turn = 2.0;
        assert!(on_bed(&mut lo, &mut player, &mut resting, 30, &mut before).is_empty());
        lo.turn = 0.0;
        assert!(on_bed(&mut lo, &mut player, &mut resting, 30, &mut before).is_empty());
        assert!(resting && Arc::ptr_eq(&lo.set, &stand));
        // Out of the water nothing changes: it stands already.
        assert_eq!(frame(&mut lo, false, Some(0.0), &mut resting), WaterChange::None);
        assert!(!resting);
        // Another floating off the bed (nothing in reach below): the swimming idle, and
        // out of the water the standing one.
        let mut lo = Locomotion::new(stand.clone(), 0.0);
        lo.swim = Some(swim.clone());
        let mut resting = false;
        assert_eq!(frame(&mut lo, true, None, &mut resting), WaterChange::Swims);
        assert_eq!(idle_path(&lo), "swimidle.kf");
        assert_eq!(frame(&mut lo, true, Some(9.0), &mut resting), WaterChange::None, "floating 9 above a bed");
        assert_eq!(frame(&mut lo, false, Some(0.0), &mut resting), WaterChange::Leaves);
        assert_eq!(idle_path(&lo), "mtidle.kf");
    }

    /// Setting off again half way through the swimming idle's fade into the standing one:
    /// the fade runs on (nothing dropped), the standing walk follows, then the swimming
    /// walk.
    #[test]
    fn setting_off_half_way_through_coming_to_rest_cuts_nothing_off() {
        let (stand, swim) = (clips("mt"), clips("swim"));
        let mut lo = Locomotion::new(stand.clone(), 0.0);
        lo.swim = Some(swim.clone());
        lo.blend.set_off_at_once = true;
        let mut player = BlendPlayer::new(Vec::new(), Arc::new(Vec::new()));
        // Swimming at the bed, the swimming idle alone.
        let mut resting = false;
        assert_eq!(follow_water(&mut lo, true, false, false), WaterChange::Swims);
        for _ in 0..30 {
            lo.step(DT);
        }
        let mut before = shown(&mut lo, &mut player);
        assert_eq!(before, alone("swimidle.kf"));
        // It comes to rest; six frames in (half the 0.2 s fade) it sets off.
        let changes = on_bed(&mut lo, &mut player, &mut resting, 6, &mut before);
        assert_eq!(changes, vec![(0, WaterChange::Rests)]);
        let (a, b) = (weight_of(&before, "mtidle.kf"), weight_of(&before, "swimidle.kf"));
        assert!(a > 0.2 && b > 0.2, "half way: {before:?}");
        lo.velocity = [0.0, 30.0];
        let changes = on_bed(&mut lo, &mut player, &mut resting, 60, &mut before);
        assert_eq!(changes.iter().map(|(_, c)| *c).collect::<Vec<_>>(), vec![WaterChange::SetsOff], "{changes:?}");
        assert!(lo.swimming && Arc::ptr_eq(&lo.set, &swim));
        assert_eq!(before, alone("swimforward.kf"));
    }

    #[test]
    fn a_swimmer_without_swimming_clips_keeps_its_standing_set() {
        let stand = clips("mt");
        let mut lo = Locomotion::new(stand.clone(), 0.0);
        let mut resting = false;
        lo.velocity = [0.0, 30.0];
        assert_eq!(frame(&mut lo, true, None, &mut resting), WaterChange::NoClips);
        assert!(!lo.swimming && Arc::ptr_eq(&lo.set, &stand));
    }

    #[test]
    fn the_pose_watch_names_the_base_clip_and_the_character() {
        let (stand, swim) = (clips("mt"), clips("swim"));
        let mut lo = Locomotion::new(stand.clone(), 0.0);
        lo.swim = Some(swim);
        let mut player = BlendPlayer::new(Vec::new(), Arc::new(Vec::new()));
        lo.layers(&mut player);
        assert_eq!(pose_line(&lo, &player, None), "layer 0 mtidle.kf (weight 1.0) from its standing set; idle layers none; no character");
        let resting = pose_line(&lo, &player, Some((true, true)));
        assert!(resting.ends_with("; a character, swimming, resting on the bed"), "{resting}");
        player.layers[0].as_mut().expect("idle").weight = 0.98;
        assert_eq!(pose_line(&lo, &player, Some((true, true))), resting, "under a tenth is no change");
        player.layers[0].as_mut().expect("idle").weight = 0.5;
        player.layers[1] = Some(Layer { clip: stand.clips[&Move::Forward].bound.clone(), phase: 0.0, weight: 0.5 });
        assert!(pose_line(&lo, &player, None).starts_with("layer 0 mtidle.kf (weight 0.5), layer 1 mtforward.kf (weight 0.5) from"));
        player.layers[0] = None;
        assert!(pose_line(&lo, &player, None).starts_with("layer 0 none, layer 1 mtforward.kf"));
    }
}
