//! Locomotion playback on actors (workstream 5 part G, docs/spec-notes/actors.md §8):
//! each animated actor carries its clip set and a [`Blend`] state; every frame the move
//! is chosen from its velocity and turn rate (`fnv_actor::locomotion::choose`) and up to
//! two clips are blended onto its skeleton (`nvf_assets::anim_play::BlendPlayer`).
//! Actors are moved by the test aid `--actor-walk-test` ([`WalkTest`]) and by `nav.rs`
//! (travellers on the navmesh); Layer 6 will set [`Locomotion::velocity`] and
//! [`Locomotion::turn`] from AI. A standing actor is turned on the spot by [`TurnTo`]
//! (actors.md §8.9), which plays its turn clip; `--actor-turn-test` ([`TurnTest`]) tries it.

use bevy::prelude::*;
use bevy::tasks::{ComputeTaskPool, ParallelSliceMut};
use fnv_actor::locomotion::{choose, direction, hurt_move_in, wrap01, Blend, Body, Choice, ClipInfo, LocomotionSet, Move, Slot, HURT_FOLDER, RATE_MAX, RATE_MIN};
use fnv_nav::follow::{heading_delta, SPOT_TURN_RATE, SPOT_TURN_START};
use nvf_assets::actor::{bind_clip, clip_binding, ClipSource, PreparedSkeleton};
use nvf_assets::anim_play::{BlendPlayer, BoundClip, Layer, PoseMask, MAX_LAYERS};
use std::collections::BTreeMap;
use std::f32::consts::{FRAC_PI_2, PI};
use std::sync::Arc;

/// Stance prefix of the clips played (no weapon) [observed: actors.md §8.1].
pub const STANCE: &str = "mt";
/// … while sneaking: `sneakmtidle.kf`, `sneakmtforward.kf` … in the same folders
/// [observed: the `_male\locomotion` file list; the player's sneak clips are named so too,
/// `fnv_actor::player_clips`].
pub const SNEAK_STANCE: &str = "sneakmt";
/// … of a creature's alert stance: `h2hforward.kf`, `h2hfastforward.kf` (the cazador's
/// flight; it scurries along the ground on its `mt…` clips while calm [observed: the
/// owner's playtest 2026-10-16; the `creatures\nvcazadores\locomotion` file list]).
pub const ALERT_STANCE: &str = "h2h";
/// … while swimming: `swimmtidle.kf`, `swimmtforward.kf`, `swimmtfastforward.kf` … in the
/// skeleton's folder itself (people: `characters\_male`, the four directions walking and
/// running, the idle and the turns; the mirelurk, giant rat and spinebreaker: idle,
/// forward walking and running, the turns), and `swim…` without `mt` where a folder names
/// them so (the dog's `swimforward.kf`, the mirelurk king's `swimidle.kf`) [observed: the
/// base meshes archive listing, 2026-10-18]. Loaded together ([`ClipSet::swim_from`]).
pub const SWIM_STANCES: [&str; 2] = ["swimmt", "swim"];

/// One clip of a set: its tracks bound to the skeleton and what choosing needs to know
/// about it.
#[derive(Clone)]
pub struct SetClip {
    pub bound: Arc<BoundClip>,
    pub info: ClipInfo,
    /// The file (VFS path), for the log.
    pub path: String,
}

/// The locomotion clips of one skeleton and body, shared by the actors that use them.
#[derive(Default, Clone)]
pub struct ClipSet {
    pub clips: BTreeMap<Move, SetClip>,
    /// Combat (step C5): death poses held once played (`idleanims\deathpose1.kf`,
    /// `deathpose2.kf`), and the hit reactions by where the hit landed (step C8, [`FLINCHES`]),
    /// when the skeleton's folder has them [observed: the `_male` folder does].
    pub death: Vec<Arc<BoundClip>>,
    pub flinch: [Option<Arc<BoundClip>>; 6],
    /// Getting up from a knockout (docs/spec-notes/unconscious.md §4): lying face up, and
    /// face down ([`GET_UPS`]).
    pub getup: [Option<Arc<BoundClip>>; 2],
    /// The stance's jump clips (docs/spec-notes/actor-jumps.md, [`JumpClips`]).
    pub jump: JumpClips,
}

/// A stance's jump clips beside its skeleton: looped in the air (`[0]`) and landing
/// (`[1]`), each without a direction and for the four ways
/// (`fnv_actor::jump::dir_index`): `mtjumploop.kf`, `mtjumplandforward.kf`,
/// `1hpjumploopleft.kf` … Empty for creatures (no folder has any [observed]).
#[derive(Default, Clone)]
pub struct JumpClips {
    pub clips: [[Option<Arc<BoundClip>>; 5]; 2],
}

impl JumpClips {
    /// The clips of `stance` in `folder` (the skeleton's), bound to `skel`.
    pub fn load(ctx: &mut impl ClipSource, folder: &str, skel: &PreparedSkeleton, stance: &str) -> JumpClips {
        let mut out = JumpClips::default();
        for (k, land) in [false, true].into_iter().enumerate() {
            for (i, name) in fnv_actor::jump::clip_names(stance, land).iter().enumerate() {
                let path = format!("{folder}\\{name}");
                out.clips[k][i] = ctx.data().vfs.exists(&path).then(|| ctx.clip(&path)).flatten().map(|c| Arc::new(bind_clip(skel, c)));
            }
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.clips.iter().flatten().all(Option::is_none)
    }

    /// `self` with `top`'s clips in place of its own where `top` has one.
    fn over(&self, top: &JumpClips) -> JumpClips {
        let mut out = self.clone();
        for (k, row) in top.clips.iter().enumerate() {
            for (i, c) in row.iter().enumerate() {
                if c.is_some() {
                    out.clips[k][i] = c.clone();
                }
            }
        }
        out
    }

    /// The clip for a direction (`fnv_actor::jump::dir_index`), else the plain one.
    pub fn get(&self, land: bool, dir: usize) -> Option<&Arc<BoundClip>> {
        let row = &self.clips[usize::from(land)];
        row[dir].as_ref().or(row[0].as_ref())
    }
}

impl ClipSet {
    /// The hit reaction for a hit location (`GetHitLocation` numbering), the torso's when
    /// the part has none.
    pub fn flinch_for(&self, location: i32) -> Option<Arc<BoundClip>> {
        let k = match location {
            1 | 2 | 13 => 1,
            3 | 4 => 2,
            5 | 6 => 3,
            7..=9 => 4,
            10..=12 => 5,
            _ => 0,
        };
        self.flinch[k].clone().or_else(|| self.flinch[0].clone())
    }
}

/// Death poses and hit reaction looked for in a skeleton's folder (combat C5).
const DEATH_POSES: [&str; 2] = ["idleanims\\deathpose1.kf", "idleanims\\deathpose2.kf"];
/// Get-up clips, face up then face down: the first of each that a skeleton's folder has
/// [observed: the `_male` IDLE records GetUpFaceUp / GetUpFaceDown name
/// `mt_knockdownface*.kf`; the creature folders' file names; hypothesis: a quadruped's
/// "right" clip is its face-up one, unconscious.md §1].
const GET_UPS: [[&str; 3]; 2] = [
    ["idleanims\\mt_knockdownfaceup.kf", "idleanims\\specialidle_getupfaceup.kf", "idleanims\\mtspecialidle_getupright.kf"],
    ["idleanims\\mt_knockdownfacedown.kf", "idleanims\\specialidle_getupfacedown.kf", "idleanims\\mtspecialidle_getupleft.kf"],
];
/// Hit reactions: torso, head, left and right arm, left and right leg, the first of each
/// list that a skeleton's folder has [observed: the `_male\idleanims` names; the creature
/// folders' `idleanims` names in `Fallout - Meshes.bsa`, 2026-10-23: `specialidle_hit…`
/// (ghoul, deathclaw, super mutant, mantis …; some spell `hitleftarm` for `hitarmleft`),
/// `mtspecialidle_hit…` (dog, brahmin, protectron, yao guai; the dog's and brahmin's
/// `hitleft`/`hitright` for a side), `hitreaction_…` (gecko, mole rat, giant rat,
/// bighorner), the cazador's `h2hhitlegs`; design: which location plays which]. Before,
/// only the people's `mt_hit…` names were looked for and no creature ever staggered
/// (owner's playtest list 7 item 13).
const FLINCHES: [&[&str]; 6] = [
    &["idleanims\\mt_hittorso.kf", "idleanims\\specialidle_hittorso.kf", "idleanims\\mtspecialidle_hittorso.kf", "idleanims\\hitreaction_torso.kf"],
    &["idleanims\\mt_hithead.kf", "idleanims\\specialidle_hithead.kf", "idleanims\\mtspecialidle_hithead.kf", "idleanims\\hitreaction_head.kf"],
    &[
        "idleanims\\mt_hitarmleft.kf",
        "idleanims\\specialidle_hitarmleft.kf",
        "idleanims\\specialidle_hitleftarm.kf",
        "idleanims\\mtspecialidle_hitarmleft.kf",
        "idleanims\\mtspecialidle_hitleftarm.kf",
        "idleanims\\hitreaction_leftarm.kf",
        "idleanims\\specialidle_hitarms.kf",
        "idleanims\\mtspecialidle_hitleft.kf",
    ],
    &[
        "idleanims\\mt_hitarmright.kf",
        "idleanims\\specialidle_hitarmright.kf",
        "idleanims\\specialidle_hitrightarm.kf",
        "idleanims\\mtspecialidle_hitarmright.kf",
        "idleanims\\mtspecialidle_hitrightarm.kf",
        "idleanims\\hitreaction_rightarm.kf",
        "idleanims\\specialidle_hitarms.kf",
        "idleanims\\mtspecialidle_hitright.kf",
    ],
    &[
        "idleanims\\mt_hitlegleft.kf",
        "idleanims\\specialidle_hitlegleft.kf",
        "idleanims\\specialidle_hitleftleg.kf",
        "idleanims\\mtspecialidle_hitlegleft.kf",
        "idleanims\\mtspecialidle_hitleftleg.kf",
        "idleanims\\hitreaction_leftleg.kf",
        "idleanims\\specialidle_hitlegs.kf",
        "idleanims\\h2hhitlegs.kf",
        "idleanims\\mtspecialidle_hitleft.kf",
    ],
    &[
        "idleanims\\mt_hitlegright.kf",
        "idleanims\\specialidle_hitlegright.kf",
        "idleanims\\specialidle_hitrightleg.kf",
        "idleanims\\mtspecialidle_hitlegright.kf",
        "idleanims\\mtspecialidle_hitrightleg.kf",
        "idleanims\\hitreaction_rightleg.kf",
        "idleanims\\specialidle_hitlegs.kf",
        "idleanims\\h2hhitlegs.kf",
        "idleanims\\mtspecialidle_hitright.kf",
    ],
];

/// A dead actor (combat C5): the death pose plays once and holds its last frame; without
/// one, the pose freezes and the body is laid down [design].
#[derive(Component)]
pub struct Dying {
    pub clip: Option<Arc<BoundClip>>,
    pub t: f32,
}

/// A hit reaction playing once over the actor's other clips (combat C5). It stays on the
/// actor after the clip's end: `deaths.rs` reads from it how long ago the actor's last
/// reaction ended.
#[derive(Component)]
pub struct Flinch {
    pub clip: Arc<BoundClip>,
    /// When it started (`Time::elapsed_secs`).
    pub at: f32,
    /// A power attack forced it (GetForceHitReaction, `crate::anim_state`).
    pub forced: bool,
}

impl ClipSet {
    pub fn info(&self, m: Move) -> Option<ClipInfo> {
        self.clips.get(&m).map(|c| c.info)
    }

    /// This set with script replacements (kNVSE's SetActorAnimationPath): a file whose
    /// stem is the standing stance's name of a move (`mtidle.kf` for Idle) takes that
    /// move [hypothesis: the move by the file's name; observed: `h2hidle.kf`, group
    /// H2HIdle, did not change Idle]. Paths are under meshes. Also returns how many moves
    /// were replaced.
    pub fn with_replacements(&self, ctx: &mut impl ClipSource, skel: &PreparedSkeleton, paths: &[String]) -> (ClipSet, usize) {
        let mut set = self.clone();
        let mut n = 0;
        for p in paths {
            let p = p.replace('/', "\\");
            let stem = p.rsplit('\\').next().unwrap_or("").to_ascii_lowercase();
            let stem = stem.strip_suffix(".kf").unwrap_or(&stem).to_string();
            let Some(m) = Move::ALL.into_iter().find(|m| stem == format!("{STANCE}{}", m.group().to_ascii_lowercase())) else { continue };
            let path = format!("meshes\\{p}");
            let Some(clip) = ctx.clip(&path) else { continue };
            let info = ClipInfo::from_parts(clip.start, clip.stop, &clip.text_keys, clip.accum_track());
            set.clips.insert(m, SetClip { bound: Arc::new(bind_move(skel, clip, m)), info, path });
            n += 1;
        }
        (set, n)
    }

    /// The alert stance's set: the `h2h` clips of `stance` (loaded as a set of their
    /// own), the moves they lack taken from `calm` (the cazador has `h2hforward`,
    /// `h2hfastforward` and `h2hidle`, the dog only the two movement clips [observed: the
    /// base meshes archive, 2026-10-16]). `None` when the stance has no movement clip.
    pub fn alert_from(stance: ClipSet, calm: &ClipSet) -> Option<ClipSet> {
        if !stance.clips.contains_key(&Move::Forward) && !stance.clips.contains_key(&Move::FastForward) {
            return None;
        }
        let mut set = calm.clone();
        set.clips.extend(stance.clips);
        Some(set)
    }

    /// The swimming set: the moves of the swim stances (`stances`, each loaded as a set,
    /// the first that has a move winning), the moves they lack taken from `calm` (a
    /// creature without a swimming idle floats in its standing idle [design]). `None`
    /// when no stance has a movement clip: the actor swims on its standing clips.
    pub fn swim_from(stances: Vec<ClipSet>, calm: &ClipSet) -> Option<ClipSet> {
        if !stances.iter().any(|s| s.clips.keys().any(|m| m.is_moving())) {
            return None;
        }
        let mut set = calm.clone();
        for s in stances.into_iter().rev() {
            set.clips.extend(s.clips);
        }
        Some(set)
    }

    /// The set for the skeleton at `skeleton` (archive path) and `body`: the moves found
    /// by `LocomotionSet::find` in the skeleton's folder, each loaded and bound to `skel`.
    /// Also returns a report line.
    /// `stance` is the clips' prefix ([`STANCE`], [`SNEAK_STANCE`]).
    pub fn load_stance(ctx: &mut impl ClipSource, skeleton: &str, skel: &PreparedSkeleton, body: Body, stance: &str) -> (ClipSet, String) {
        let s = fnv_vfs::normalize(skeleton);
        let folder = s.rfind('\\').map_or("", |i| &s[..i]).to_string();
        let found = LocomotionSet::find(&folder, body, stance, |p| ctx.data().vfs.exists(p));
        let (mut set, report) = ClipSet::bind_moves(ctx, skel, found);
        set.jump = JumpClips::load(ctx, &folder, skel, stance);
        if stance == SNEAK_STANCE {
            // Crouched jumps have no plain loop (`sneakmtjump{loop,land}<dir>.kf` only,
            // animation.md §9.1): the standing ones fill in.
            set.jump = JumpClips::load(ctx, &folder, skel, STANCE).over(&set.jump);
        }
        for p in DEATH_POSES {
            let path = format!("{folder}\\{p}");
            if let Some(clip) = ctx.data().vfs.exists(&path).then(|| ctx.clip(&path)).flatten() {
                set.death.push(Arc::new(bind_clip(skel, clip)));
            }
        }
        for (k, names) in FLINCHES.iter().enumerate() {
            set.flinch[k] = names.iter().find_map(|f| {
                let path = format!("{folder}\\{f}");
                ctx.data().vfs.exists(&path).then(|| ctx.clip(&path)).flatten().map(|c| Arc::new(bind_clip(skel, c)))
            });
        }
        for (k, names) in GET_UPS.iter().enumerate() {
            set.getup[k] = names.iter().find_map(|f| {
                let path = format!("{folder}\\{f}");
                ctx.data().vfs.exists(&path).then(|| ctx.clip(&path)).flatten().map(|c| Arc::new(bind_clip(skel, c)))
            });
        }
        let missing: Vec<&str> = Move::ALL.iter().filter(|m| !set.clips.contains_key(m)).map(|m| m.group()).collect();
        let line = format!("{folder} {body:?} {stance}: {} clips, missing {missing:?}: {}", set.clips.len(), report.join("; "));
        (set, line)
    }

    /// The clips of `found` loaded and bound to `skel`, with a report entry per move.
    fn bind_moves(ctx: &mut impl ClipSource, skel: &PreparedSkeleton, found: LocomotionSet) -> (ClipSet, Vec<String>) {
        let mut set = ClipSet::default();
        let mut report = Vec::new();
        for (m, path) in found.clips {
            let Some(clip) = ctx.clip(&path) else {
                report.push(format!("{}: {path} does not load", m.group()));
                continue;
            };
            let info = ClipInfo::from_parts(clip.start, clip.stop, &clip.text_keys, clip.accum_track());
            let (bound, unbound) = clip_binding(skel, &clip);
            // Bound tracks whose rotation adds up from cycle to cycle (the Securitron's
            // wheel in its walk, docs/spec-notes/animation.md §9.12).
            let adding: Vec<&str> = clip.tracks.iter().filter(|(n, t)| t.per_cycle.is_some() && bound.contains(&n.as_str())).map(|(n, _)| n.as_str()).collect();
            report.push(format!(
                "{} {:.2}s {:.1}u/s blend {:.2}s foot {:.2} ({} tracks bound{}{})",
                m.group(),
                info.duration,
                info.speed,
                info.blend_time,
                info.foot_phase,
                bound.len(),
                if unbound.is_empty() { String::new() } else { format!(", unbound {unbound:?}") },
                if adding.is_empty() { String::new() } else { format!(", rotation adding up per cycle {adding:?}") }
            ));
            set.clips.insert(m, SetClip { bound: Arc::new(bind_move(skel, clip, m)), info, path });
        }
        (set, report)
    }

    /// `base` with the clips of `top` in place of its own for the moves `top` has that
    /// `keep` says yes to; `base` itself when there are none.
    fn overlay(base: &Arc<ClipSet>, top: &ClipSet, keep: impl Fn(Move) -> bool) -> Arc<ClipSet> {
        let mut over = top.clips.iter().filter(|(m, _)| keep(**m)).peekable();
        if over.peek().is_none() && top.jump.is_empty() {
            return base.clone();
        }
        let mut set = ClipSet::clone(base);
        set.clips.extend(over.map(|(m, c)| (*m, c.clone())));
        set.jump = base.jump.over(&top.jump);
        Arc::new(set)
    }

    /// The limping set of a skeleton (a leg crippled): `base` with each move that the
    /// skeleton's `locomotion\hurt\` folder has a clip for, in one of `stances`, replaced
    /// by that clip (`fnv_actor::locomotion::hurt_move_in`), the others kept; a later
    /// stance's clip wins over an earlier one's. `None` when the folder has none. Also
    /// returns a report line. The calm set takes `["mt"]`; a creature's alert set
    /// (`h2h` moves over the calm ones, [`ClipSet::alert_from`]) takes `["mt", "h2h"]`,
    /// so that a dog in a fight limps on its `h2hfastforward_hurt.kf` [design].
    pub fn load_hurt(ctx: &mut impl ClipSource, skeleton: &str, skel: &PreparedSkeleton, base: &ClipSet, stances: &[&str]) -> Option<(ClipSet, String)> {
        let s = fnv_vfs::normalize(skeleton);
        let folder = s.rfind('\\').map_or("", |i| &s[..i]).to_string();
        let dir = format!("{folder}\\{HURT_FOLDER}\\");
        let files = ctx.data().vfs.paths(|p| p.strip_prefix(&dir).is_some_and(|f| f.ends_with(".kf") && !f.contains('\\')));
        let mut set = base.clone();
        let mut found: Vec<(Move, String)> = Vec::new();
        for stance in stances {
            let mut mine: Vec<Move> = Vec::new();
            for path in &files {
                let stem = path[dir.len()..path.len() - 3].to_string();
                let Some(clip) = ctx.clip(path) else { continue };
                // The first file (in name order) of a move wins within a stance.
                let Some(m) = hurt_move_in(&stem, Some(&clip.name), stance).filter(|m| !mine.contains(m)) else { continue };
                mine.push(m);
                let info = ClipInfo::from_parts(clip.start, clip.stop, &clip.text_keys, clip.accum_track());
                found.retain(|(f, _)| *f != m);
                found.push((m, format!("{} {stem} {:.1}u/s", m.group(), info.speed)));
                set.clips.insert(m, SetClip { bound: Arc::new(bind_move(skel, clip, m)), info, path: path.clone() });
            }
        }
        if found.is_empty() {
            return None;
        }
        let moves: Vec<String> = found.into_iter().map(|(_, l)| l).collect();
        let what = if stances.len() > 1 { format!("limping ({})", stances.join("+")) } else { "limping".to_string() };
        let line = format!("{folder} {what}: {} of {} moves from hurt\\: {}", moves.len(), set.clips.len(), moves.join("; "));
        Some((set, line))
    }
}

/// [design, #2352] Apply the dart correction only to moving clips, including script
/// replacements and hurt/weapon sets (`ClipSet` loaders).
fn bind_move(skel: &PreparedSkeleton, clip: Arc<nvf_assets::anim_play::Clip>, mv: Move) -> BoundClip {
    let bound = bind_clip(skel, clip);
    if mv.is_moving() { bound.locomotion() } else { bound }
}

/// A weapon stance's movement clips bound to one skeleton
/// (`fnv_actor::locomotion::weapon_moves`): standing (`locomotion\1hpfastforward.kf`,
/// `1hpturnleft.kf`), crouched (`sneak2hhforward.kf`) and limping
/// (`locomotion\hurt\1hpforward_hurt.kf`). While the weapon is drawn they play in place
/// of the unarmed moves, the stance's aim clip over them ([`Locomotion::set_armed`]).
/// Loaded with the stance (`weapons::Stance::load`).
#[derive(Default)]
pub struct WeaponMoves {
    pub prefix: String,
    pub stand: ClipSet,
    pub sneak: ClipSet,
    pub hurt: ClipSet,
}

impl WeaponMoves {
    /// The moves of stance `prefix` in the folder of the skeleton at `skeleton` (archive
    /// path), bound to `skel`; also a report line.
    pub fn load(ctx: &mut impl ClipSource, skeleton: &str, skel: &PreparedSkeleton, prefix: &str) -> (WeaponMoves, String) {
        let s = fnv_vfs::normalize(skeleton);
        let folder = s.rfind('\\').map_or("", |i| &s[..i]).to_string();
        let mut find = |stance: &str| {
            let found = fnv_actor::locomotion::weapon_moves(&folder, stance, |p| ctx.data().vfs.exists(p));
            ClipSet::bind_moves(ctx, skel, found).0
        };
        let mut stand = find(prefix);
        let sneak = find(&format!("sneak{prefix}"));
        // The weapon's own jump clips (`1hpjumploopforward.kf`), over the unarmed ones.
        stand.jump = JumpClips::load(ctx, &folder, skel, prefix);
        let hurt = ClipSet::load_hurt(ctx, skeleton, skel, &ClipSet::default(), &[prefix]).map(|(s, _)| s).unwrap_or_default();
        let list = |set: &ClipSet| set.clips.keys().map(|m| m.group()).collect::<Vec<_>>();
        let line = format!("{folder} {prefix} moves: standing {:?}, crouched {:?}, limping {:?}", list(&stand), list(&sneak), list(&hurt));
        (WeaponMoves { prefix: prefix.to_string(), stand, sneak, hurt }, line)
    }

    pub fn is_empty(&self) -> bool {
        self.stand.clips.is_empty() && self.sneak.clips.is_empty() && self.hurt.clips.is_empty()
    }
}

/// An actor's sets with a weapon's moves in them ([`Locomotion::set_armed`]).
struct Armed {
    moves: Arc<WeaponMoves>,
    stand: Arc<ClipSet>,
    sneak: Option<Arc<ClipSet>>,
    hurt: Option<Arc<ClipSet>>,
}

/// An actor's locomotion state.
#[derive(Component)]
pub struct Locomotion {
    pub set: Arc<ClipSet>,
    pub blend: Blend,
    /// The set the blend's fading-out clip (`blend.prev`) belongs to, while a change of
    /// set (crouching, standing up, starting or stopping a limp) fades the old set's pose
    /// out ([`Locomotion::switch_set`]); `None`: the current set.
    pub prev_set: Option<Arc<ClipSet>>,
    /// Velocity in the actor's axes (x right, y forward; units per second, in the actor's
    /// own scale) and turn rate (radians per second, positive = left).
    pub velocity: [f32; 2],
    pub turn: f32,
    /// Keep the pose (`--actor-pose-time`).
    pub hold: bool,
    /// The standing set, the sneaking one (people whose folder has a sneaking idle) and
    /// the limping one (a leg crippled; skeletons with a `locomotion\hurt\` folder);
    /// `set` is one of them ([`Locomotion::set_sneaking`], [`Locomotion::set_limping`]).
    pub stand: Arc<ClipSet>,
    pub sneak: Option<Arc<ClipSet>>,
    pub hurt: Option<Arc<ClipSet>>,
    /// The limping set while alerted (creatures with an alert stance: the alert set with
    /// the `mt` and `h2h` limping clips over it, `ClipSet::load_hurt`); `None`: `hurt`.
    pub hurt_alert: Option<Arc<ClipSet>>,
    pub sneaking: bool,
    pub limping: bool,
    /// The swimming set (skeletons whose folder has swimming clips, [`SWIM_STANCES`]) and
    /// whether it plays: while the actor's character swims ([`Locomotion::set_swimming`]).
    pub swim: Option<Arc<ClipSet>>,
    pub swimming: bool,
    /// The alert stance's set (creatures whose folder has `h2h` movement clips: the
    /// cazador flies when alert, [`ClipSet::alert_from`]) and whether it plays
    /// ([`Locomotion::set_alert`]).
    pub alert: Option<Arc<ClipSet>>,
    pub alerted: bool,
    /// How much faster than its clips' root motion the actor moves, its clips playing
    /// that much faster too: a creature's Speed Mult × its skeleton's share
    /// (`creature_pace::skeleton_share`), 1 for people. The move is chosen for the velocity divided by
    /// it, so a fast creature's walk is still its walk clip.
    pub pace: f32,
    /// The speed (at the clips' pace) smoothed over [`GAIT_SMOOTHING`], which decides
    /// between walking and running.
    pub gait_speed: f32,
    /// In the air this frame (set by what moves the actor: the walker's flight over a
    /// ledge, `nav::show_actor`, or its character off the ground, `actor_jumps.rs`), and
    /// the jump it is in (docs/spec-notes/actor-jumps.md).
    pub in_air: bool,
    pub jump: fnv_actor::jump::JumpState,
    /// The sets with the drawn weapon's moves in them, built for the last weapon drawn,
    /// and whether they play ([`Locomotion::set_armed`]).
    armed: Option<Armed>,
    pub armed_on: bool,
    /// The clip files logged while armed ([`Locomotion::armed_clip_news`]).
    armed_logged: Vec<String>,
    /// [design, #2352] The idle under movement and upper clips ([`BlendPlayer::pose_only`]): creatures only.
    pub idle_under: bool,
    under: Under,
    /// Times "moves with no movement clip" was logged for this actor (once).
    clipless_logged: u8,
    /// Modern Feel's grounded footsteps (`npc_feel.rs`): the turn rate its planted feet
    /// turn at (radians per second, positive = left), chosen by instead of [`Self::turn`],
    /// the turn clip played at the rate that matches it; `None`: [`Self::turn`] at rate 1.
    pub feel_turn: Option<f32>,
}

/// [design, #2352] The creature set's pose-only idle below movement, stance, aim,
/// attack and special-idle clips, only on bones those clips omit. The accumulation
/// root is excluded. [documented: issue #2352] The supplied bloatfly survey names
/// wing tracks in `mtidle.kf` that its move, aim and attack clips omit.
/// [hypothesis] The original keeps those bones animating from its idle; observer
/// check 169 asks for a capture. [design] People retain their existing layering.
/// [design, #2352] Fallback priority: below ordinary clip tracks and above
/// `anim_play`'s undriven-root identity.
pub const UNDER_PRIORITY: i32 = i32::MIN + 1;

/// [design, #2352] One playback identity per real Idle, with a changing track mask
/// and the idle's independent phase (`Locomotion::step`).
#[derive(Default)]
struct Under {
    idle: Option<Arc<BoundClip>>,
    clip: Option<Arc<BoundClip>>,
    mask: Option<Arc<PoseMask>>,
    checked: [usize; MAX_LAYERS],
    needed: bool,
    phase: f32,
}

/// [design, #2352] Root-free idle at UNDER_PRIORITY. The timing-only source and
/// bound tracks stay allocated while this Idle plays; masks live beside playback.
/// The pose-only slot isolates it from every non-pose consumer.
fn under_clip(idle: &BoundClip) -> BoundClip {
    let c = &idle.clip;
    BoundClip {
        // [design, #2352] Copy timing only; all controller/event payloads start empty.
        clip: Arc::new(nvf_assets::anim_play::Clip {
            name: String::new(), start: c.start, stop: c.stop, frequency: c.frequency,
            cycle_type: c.cycle_type, tracks: Vec::new(), priorities: Vec::new(),
            skipped: 0, accum_root: None, text_keys: Vec::new(), visibility: Vec::new(),
            emitters: Vec::new(), materials: Vec::new(), free: Vec::new(),
            free_visibility: Vec::new(), lights: Vec::new(),
        }),
        tracks: idle.tracks.iter().filter(|(i, _, _)| Some(*i) != idle.root).map(|(i, t, _)| (*i, t.clone(), UNDER_PRIORITY)).collect(),
        root: None,
        root_track: None,
        carried: None,
        hold_xy: None,
        unbound: Arc::new(idle.unbound.iter().map(|(n, t, _)| (n.clone(), t.clone(), UNDER_PRIORITY)).collect()),
    }
}

impl Locomotion {
    pub fn new(set: Arc<ClipSet>, phase: f32) -> Locomotion {
        Locomotion { stand: set.clone(), set, blend: Blend::new(Move::Idle, phase), prev_set: None, velocity: [0.0; 2], turn: 0.0, hold: false, sneak: None, hurt: None, hurt_alert: None, sneaking: false, limping: false, swim: None, swimming: false, alert: None, alerted: false, pace: 1.0, gait_speed: 0.0, in_air: false, jump: Default::default(), armed: None, armed_on: false, armed_logged: Vec::new(), idle_under: false, under: Under::default(), clipless_logged: 0, feel_turn: None }
    }

    /// The standing set now (kNVSE's SetActorAnimationPath swaps it, anim_paths.rs).
    pub fn stand_set(&self) -> Arc<ClipSet> {
        self.stand.clone()
    }

    /// Take `set` as the standing set and play from it now if it is the one in use.
    pub fn replace_stand(&mut self, set: Arc<ClipSet>) {
        self.stand = set;
        self.switch_set(self.posture_set());
    }

    /// The upright set: the alert stance's while alerted, else the standing one.
    fn upright(&self) -> Arc<ClipSet> {
        match (&self.alert, self.alerted) {
            (Some(a), true) => a.clone(),
            _ => self.stand.clone(),
        }
    }

    /// The set for the actor's posture: swimming wins over everything, unarmed (the
    /// player's body holsters in water and no weapon has swimming clips [observed: the
    /// `_male` file list]; NPCs alike [design]); sneaking wins over limping (there are no
    /// crouched limping clips [observed]); limping while alerted takes the alert limping
    /// set where there is one, else the calm limping set wins over the alert stance
    /// [design]; either falls back to the upright set.
    /// With a weapon drawn each takes the armed form of the set where there is one
    /// ([`Locomotion::set_armed`]).
    fn posture_set(&self) -> Arc<ClipSet> {
        if let (true, Some(s)) = (self.swimming, &self.swim) {
            return s.clone();
        }
        let armed = self.armed.as_ref().filter(|_| self.armed_on);
        let limp = if self.alerted { self.hurt_alert.as_ref().or(self.hurt.as_ref()) } else { self.hurt.as_ref() };
        let pick = if self.sneaking {
            armed.and_then(|a| a.sneak.as_ref()).or(self.sneak.as_ref())
        } else if self.limping {
            armed.and_then(|a| a.hurt.as_ref()).or(limp)
        } else {
            None
        };
        pick.cloned().unwrap_or_else(|| armed.map_or_else(|| self.upright(), |a| a.stand.clone()))
    }

    /// Move with the drawn weapon's own clips (`moves`, `Some` while it is drawn) or
    /// unarmed again (`None`), blending from the old pose as between postures
    /// ([`Locomotion::switch_set`]). The armed sets are the actor's own with the weapon's
    /// moves in place of the unarmed ones it has clips for ([`WeaponMoves`]): standing,
    /// crouched (the stance's `sneak…` clips over the crouched set), and limping, where
    /// the limping clips of the weapon go over the unarmed limping set, and the weapon's
    /// turns too, as the player's body chooses (`player_clips::limping_move_clips` and
    /// `limping_turn_clips`: the weapon's limp, the unarmed limp, then the weapon's
    /// walk). Built from the standing set, not a creature's alert set (no creature draws
    /// a weapon stance). Returns whether anything changed; a stance without movement
    /// clips changes nothing.
    pub fn set_armed(&mut self, moves: Option<&Arc<WeaponMoves>>) -> bool {
        match moves.filter(|m| !m.is_empty()) {
            None if !self.armed_on => return false,
            None => self.armed_on = false,
            Some(m) => {
                let built = self.armed.as_ref().is_some_and(|a| Arc::ptr_eq(&a.moves, m));
                if built && self.armed_on {
                    return false;
                }
                if !built {
                    self.armed = Some(Armed {
                        moves: m.clone(),
                        stand: ClipSet::overlay(&self.stand, &m.stand, |_| true),
                        sneak: self.sneak.as_ref().map(|s| ClipSet::overlay(s, &m.sneak, |_| true)),
                        hurt: self.hurt.as_ref().map(|h| ClipSet::overlay(&ClipSet::overlay(h, &m.stand, Move::is_turn), &m.hurt, |_| true)),
                    });
                }
                self.armed_on = true;
            }
        }
        self.switch_set(self.posture_set());
        true
    }

    /// While armed, the weapon prefix, move and file of the clip the actor plays now, the
    /// first time that file plays for it (for the log).
    pub fn armed_clip_news(&mut self) -> Option<(&str, Move, String)> {
        let a = self.armed.as_ref().filter(|_| self.armed_on)?;
        let mv = self.blend.cur.mv;
        let path = &self.set.clips.get(&mv)?.path;
        if self.armed_logged.contains(path) {
            return None;
        }
        self.armed_logged.push(path.clone());
        Some((&a.moves.prefix, mv, path.clone()))
    }

    /// Change to another set (crouching, standing up, a limp starting or ending), fading
    /// from the old set's pose as the player's body does between its stances: the move
    /// goes on in the new set (a walk at the old walk's footfall phase), or the new set's
    /// idle where it has no such clip, and the old pose fades out over the new clip's
    /// blend time (its `Blend: N` key, else [`fnv_actor::locomotion::DEFAULT_BLEND`]).
    /// Changing set used to snap to the new pose in one frame [observed in NVForever: the
    /// owner's playtest 2026-10-17, "NPC crouching to standing and vice versa look
    /// unnaturally fast compared to the player's"]. A fade between two moves of the old
    /// set still running is cut short (its older clip dropped) [design].
    fn switch_set(&mut self, set: Arc<ClipSet>) {
        if Arc::ptr_eq(&set, &self.set) {
            return;
        }
        let old = std::mem::replace(&mut self.set, set);
        let cur = self.blend.cur;
        let mv = if self.set.clips.contains_key(&cur.mv) { cur.mv } else { Move::Idle };
        let (from, to) = (old.info(cur.mv), self.set.info(mv));
        let phase = match (from, to) {
            (Some(f), Some(t)) if mv == cur.mv && mv.is_moving() => wrap01(cur.phase - f.foot_phase + t.foot_phase),
            _ if mv == cur.mv => cur.phase,
            _ => 0.0,
        };
        let fade_time = to.map_or(0.0, |t| t.blend_time);
        let fading = from.is_some() && fade_time > 0.0;
        self.blend = Blend { cur: Slot { mv, phase, rate: cur.rate }, prev: fading.then_some(cur), fade: 0.0, fade_time, set_off_at_once: self.blend.set_off_at_once, stride_match: self.blend.stride_match };
        self.prev_set = fading.then_some(old);
    }

    /// Limp (`true`, a leg crippled) or walk normally again: the set's clips change and
    /// the current move goes on in the new set, blending from the old pose
    /// ([`Locomotion::switch_set`]). Without a limping set nothing changes. Returns
    /// whether anything changed.
    pub fn set_limping(&mut self, on: bool) -> bool {
        if self.limping == on || self.hurt.is_none() {
            return false;
        }
        self.limping = on;
        self.switch_set(self.posture_set());
        true
    }

    /// Take the alert stance (`true`) or the calm one: the set's clips change and the
    /// move goes on at its phase in the other set's clip, with no cross-fade between the
    /// two sets [design]. Without an alert set nothing changes. Returns whether anything
    /// changed.
    pub fn set_alert(&mut self, on: bool) -> bool {
        if self.alerted == on || self.alert.is_none() {
            return false;
        }
        self.alerted = on;
        self.set = self.posture_set();
        true
    }

    /// Crouch (`true`) or stand up: the set's clips change and the body goes down or up
    /// by a cross-fade from the old pose ([`Locomotion::switch_set`]); a walk goes on as
    /// the other set's walk. Without a sneaking set the actor stays upright. Returns
    /// whether anything changed.
    pub fn set_sneaking(&mut self, on: bool) -> bool {
        if self.sneaking == on {
            return false;
        }
        if on && self.sneak.is_none() {
            return false;
        }
        self.sneaking = on;
        self.switch_set(self.posture_set());
        true
    }

    /// Swim (`true`: the actor's character swims, `fnv_phys`) or move on land again: the
    /// set changes by a cross-fade from the old pose ([`Locomotion::switch_set`]). Without
    /// a swimming set nothing changes. Returns whether anything changed.
    pub fn set_swimming(&mut self, on: bool) -> bool {
        if self.swimming == on || self.swim.is_none() {
            return false;
        }
        self.swimming = on;
        self.switch_set(self.posture_set());
        true
    }

    /// Choose the move for the current velocity and turn rate, and advance the blend.
    pub fn step(&mut self, dt: f32) {
        let set = &self.set;
        let pace = if self.pace > 0.0 { self.pace } else { 1.0 };
        let v = [self.velocity[0] / pace, self.velocity[1] / pace];
        let speed = v[0].hypot(v[1]);
        self.gait_speed += (speed - self.gait_speed) * (1.0 - (-dt.max(0.0) / GAIT_SMOOTHING).exp());
        let mut choice = choose(|m| set.info(m), v, self.feel_turn.unwrap_or(self.turn));
        if let (Some(t), Move::TurnLeft | Move::TurnRight) = (self.feel_turn, choice.mv) {
            choice.rate = crate::npc_feel::turn_clip_rate(t);
        }
        let cur = self.blend.cur.mv;
        if let Some(kept) = held_gait(set, cur, choice.mv, v, self.gait_speed) {
            choice = kept;
        }
        if choice.mv.is_moving() {
            choice.rate *= pace;
        }
        // A blend reset from outside (a held pose) leaves no clip of another set.
        if self.blend.prev.is_none() {
            self.prev_set = None;
        }
        let before = self.blend.cur.mv;
        self.blend.step(choice, dt, |m| set.info(m));
        let dir = fnv_actor::jump::dir_index(fnv_actor::jump::jump_dir(self.velocity));
        let land_len = self.set.jump.get(true, dir).map(|c| c.clip.length());
        self.jump.step(self.in_air, dt, land_len);
        // The other set's pose has faded out, or a fade within this set has taken over
        // (a new move starts only once a fade has ended).
        if self.blend.prev.is_none() || self.blend.cur.mv != before {
            self.prev_set = None;
        }
        // The idle's own phase: the blend's while the idle plays, else running on.
        if self.blend.cur.mv == Move::Idle {
            self.under.phase = self.blend.cur.phase;
        } else if let Some(p) = self.blend.prev.filter(|p| p.mv == Move::Idle) {
            self.under.phase = p.phase;
        } else if let Some(len) = self.set.info(Move::Idle).map(|c| c.duration).filter(|l| *l > 1e-3) {
            self.under.phase = wrap01(self.under.phase + dt.max(0.0) / len);
        }
    }

    /// The blend's clips and weights as player layers; the fading-out clip from the set
    /// it belongs to ([`Locomotion::prev_set`]).
    pub fn layers(&mut self, player: &mut BlendPlayer) {
        let layer = |set: &ClipSet, mv: Move, phase: f32, weight: f32| set.clips.get(&mv).map(|c| Layer { clip: c.bound.clone(), phase, weight });
        let w = self.blend.weight();
        player.layers[0] = layer(&self.set, self.blend.cur.mv, self.blend.cur.phase, w);
        let prev_set = self.prev_set.as_deref().unwrap_or(&self.set);
        player.layers[1] = self.blend.prev.and_then(|p| layer(prev_set, p.mv, p.phase, 1.0 - w));
        // In the air or landing: the jump clip over the movement clip.
        if let Some(clip) = self.jump_clip() {
            let len = clip.clip.length();
            if let Some((_, phase, jw)) = self.jump.clip(len) {
                player.layers[1] = player.layers[0].take().map(|l| Layer { weight: l.weight * (1.0 - jw), ..l });
                player.layers[0] = Some(Layer { clip: clip.clone(), phase, weight: jw });
            }
        }
        player.pose_only = None;
        player.pose_only_mask = None;
    }

    /// [design, #2352] The creature's idle under movement, aim, attack and special
    /// idles, after those layers are assembled: only bones none of those clips drive.
    /// [hypothesis] Original-game layering remains to be checked (observer check 169).
    fn under_layer(&mut self, player: &mut BlendPlayer) -> Option<Layer> {
        player.pose_only_mask = None;
        if !self.idle_under {
            return None;
        }
        let idle = self.set.clips.get(&Move::Idle)?.bound.clone();
        let others: [Option<&BoundClip>; MAX_LAYERS] = std::array::from_fn(|k| {
            player.layers[k].as_ref().filter(|l| l.weight.is_finite() && l.weight > 0.0 && !Arc::ptr_eq(&l.clip, &idle)).map(|l| l.clip.as_ref())
        });
        let checked = others.map(|c| c.map_or(0, |c| c as *const BoundClip as usize));
        let changed_idle = !self.under.idle.as_ref().is_some_and(|c| Arc::ptr_eq(c, &idle));
        if changed_idle {
            self.under.clip = Some(Arc::new(under_clip(&idle)));
            self.under.idle = Some(idle);
        }
        let clip = self.under.clip.as_ref().expect("made with Idle");
        if changed_idle || self.under.mask.is_none() || self.under.checked != checked {
            let active = checked.iter().any(|k| *k != 0);
            let mask = PoseMask {
                tracks: clip.tracks.iter().map(|(i, _, _)| active && !others.iter().flatten().any(|c| c.tracks.iter().any(|(j, _, _)| i == j))).collect(),
                unbound: clip.unbound.iter().map(|(n, _, _)| active && !others.iter().flatten().any(|c| c.unbound.iter().any(|(m, _, _)| n.eq_ignore_ascii_case(m)))).collect(),
            };
            self.under.needed = mask.tracks.iter().chain(&mask.unbound).any(|b| *b);
            self.under.mask = Some(Arc::new(mask));
            self.under.checked = checked;
        }
        player.pose_only_mask = self.under.mask.clone();
        // [design, #2352] Even a fully masked fallback follows phase wraps in Loops.
        // Zero weight preserves the existing audible/contribution gates.
        Some(Layer { clip: clip.clone(), phase: self.under.phase, weight: if self.under.needed { 1.0 } else { 0.0 } })
    }

    /// [design, #2352] The real Idle's source and continuing phase. Pose fallback
    /// masks and timing-only views never become sound/event identities.
    pub(crate) fn idle_timeline(&self) -> Option<(&Arc<nvf_assets::anim_play::Clip>, f32)> {
        self.idle_under.then_some(())?;
        Some((&self.set.clips.get(&Move::Idle)?.bound.clip, self.under.phase))
    }

    /// The jump clip of the actor's set that plays now (the loop in the air, the landing
    /// after), for the way it moves; `None` on the ground or without one.
    pub fn jump_clip(&self) -> Option<&Arc<BoundClip>> {
        let land = match self.jump.phase {
            fnv_actor::jump::JumpPhase::Ground => return None,
            fnv_actor::jump::JumpPhase::Air(_) => false,
            fnv_actor::jump::JumpPhase::Land(_) => true,
        };
        self.set.jump.get(land, fnv_actor::jump::dir_index(fnv_actor::jump::jump_dir(self.velocity)))
    }
}

/// How far past the walk/run switch point the speed must go before the gait changes,
/// as a ratio [design: a speed hovering at the switch point, from uneven frame times or a
/// slow-down at a corner, swapped the clips back and forth].
const GAIT_HYSTERESIS: f32 = 1.15;

/// Time constant of [`Locomotion::gait_speed`], seconds [design: a creature's drawn speed
/// jumped to two or three times its walk for a frame (455 to 1,363 units/s and back, the
/// Tribal Village cazadors), which started a cross-fade to the run clip and back: a
/// hitch].
const GAIT_SMOOTHING: f32 = 0.2;

/// The walk or run clip `cur` kept instead of `chosen`, when the two are the same move at
/// the other gait and the smoothed speed `gait` has not gone [`GAIT_HYSTERESIS`] past the
/// switch point; its rate for the speed of `v`.
fn held_gait(set: &ClipSet, cur: Move, chosen: Move, v: [f32; 2], gait: f32) -> Option<Choice> {
    if !cur.is_moving() || !chosen.is_moving() || cur == chosen {
        return None;
    }
    let dir = direction(v);
    let (walk, run) = (Move::moving(dir, false), Move::moving(dir, true));
    if !((cur == walk && chosen == run) || (cur == run && chosen == walk)) {
        return None;
    }
    let (w, r) = (set.info(walk)?, set.info(run)?);
    if w.speed <= 0.0 || r.speed <= 0.0 {
        return None;
    }
    let switch = (w.speed * r.speed).sqrt();
    let speed = v[0].hypot(v[1]);
    let keep = if cur == walk { gait < switch * GAIT_HYSTERESIS } else { gait > switch / GAIT_HYSTERESIS };
    let c = if cur == walk { w } else { r };
    keep.then(|| Choice { mv: cur, rate: (speed / c.speed).clamp(RATE_MIN, RATE_MAX) })
}

/// Time spent posing actors, for the `--run-seconds` summary (actors.md §8.6).
#[derive(Resource, Default)]
pub struct ActorAnimCost {
    pub total: std::time::Duration,
    pub frames: u32,
    /// Actors posed and layers sampled, summed over frames; tracks sampled.
    pub actor_frames: u64,
    pub layer_frames: u64,
    pub tracks: u64,
    /// Frames in which an actor sampled more than one clip, summed over actors.
    pub blend_frames: u64,
    /// Frames in which an actor's clip was a turn on the spot, summed over actors.
    pub turn_frames: u64,
    /// Frames in which an actor's idle was laid under another clip ([`BlendPlayer::pose_only`]),
    /// summed over actors.
    pub under_frames: u64,
    /// Frames in which an actor moved (at least `STAND_SPEED`) with no movement clip
    /// showing: drawn on its idle, or under a clip that keys its legs over the walk (a
    /// creature's whole-body blow), summed over actors (issue #1073: the slide).
    pub clipless_frames: u64,
    /// Frames in which a dead actor's death pose stood as it was and nothing was sampled
    /// for it (`BlendPlayer::pose_held`), summed over actors: at the clip's end, or part
    /// way through it with time stopped. Such an actor is not counted as posed in
    /// `actor_frames` and `peak`.
    pub held_frames: u64,
    /// The part of `held_frames` at the end of the clip.
    pub finished_held_frames: u64,
    /// Time spent on the frames of `held_frames` (part of `total`), measured around the pose
    /// check only. `total` less this is approximately the time of the `actor_frames`: the
    /// loop's own overhead on held actors stays in it.
    pub held_time: std::time::Duration,
    /// Frames in which a dead actor at the end of its death clip was sampled all the same
    /// (the first frame there, or something changed), summed over actors.
    pub finished_sampled_frames: u64,
    /// The part of `tracks` sampled on dead actors.
    pub dead_tracks: u64,
    /// Frames in which a creature's blow showed on its head and neck only, over its run.
    pub head_blow_frames: u64,
    pub peak: usize,
}

/// What [`drive`] reads and writes of an actor.
type ActorAnimParts = (
    &'static mut Locomotion,
    &'static mut BlendPlayer,
    Option<&'static mut crate::weapons::WeaponHold>,
    Option<&'static mut Dying>,
    Option<&'static Flinch>,
    Option<&'static crate::furniture::OnFurniture>,
    Option<&'static crate::idles::ActorIdle>,
    Option<&'static mut crate::attack_anims::CombatPose>,
    Option<&'static mut crate::creature_stance::CreatureStance>,
    Option<&'static Name>,
    Option<&'static crate::talk_clips::TalkClip>,
);

/// Actors posed in one task of [`drive`] on the compute pool.
const DRIVE_CHUNK: usize = 8;

/// Choose, blend and pose every actor with a [`Locomotion`], with its weapon clips
/// (`weapons.rs`) and the idles it plays (`idles.rs`) layered on top.
///
/// Each actor's clips are chosen and its bones sampled on the compute pool, several
/// actors at once: that reads and writes only the actor's own components. The bones'
/// transforms are written afterwards, one actor after another in the query's order, so
/// the frame's poses do not depend on the threads (issue #1799).
pub fn drive(
    time: Res<Time>,
    mut actors: Query<ActorAnimParts>,
    mut transforms: Query<&mut Transform>,
    mut cost: ResMut<ActorAnimCost>,
) {
    let t0 = std::time::Instant::now();
    let (dt, now) = (time.delta_secs(), time.elapsed_secs());
    let mut items: Vec<_> = actors.iter_mut().collect();
    // A dead actor's pose stands only while nothing else wrote its bones
    // (`BlendPlayer::pose_held`): asked here, before anything is written.
    let written: Vec<bool> = items.iter().map(|it| it.3.is_some() && it.1.written_since(&mut transforms)).collect();
    let mut jobs: Vec<(_, bool)> = items.iter_mut().zip(written).collect();
    let one_chunk = |chunk: &mut [(&mut ActorAnimItem, bool)]| {
        let mut c = ActorAnimCost::default();
        let (mut scratch, mut out) = (Vec::new(), Vec::new());
        for (item, written) in chunk.iter_mut() {
            drive_one(item, *written, dt, now, &mut scratch, &mut out, &mut c);
        }
        (c, out)
    };
    let parts = if jobs.len() <= DRIVE_CHUNK { vec![one_chunk(&mut jobs)] } else { jobs.par_chunk_map_mut(ComputeTaskPool::get(), DRIVE_CHUNK, |_, chunk| one_chunk(chunk)) };
    let mut n = 0;
    for (c, out) in parts {
        for (e, tr) in out {
            if let Ok(mut t) = transforms.get_mut(e) {
                t.set_if_neq(tr);
            }
        }
        n += c.actor_frames as usize;
        cost.absorb(&c);
    }
    cost.total += t0.elapsed();
    cost.frames += 1;
    cost.peak = cost.peak.max(n);
}

/// One item of [`drive`]'s query.
type ActorAnimItem<'w, 's> = bevy::ecs::query::QueryItem<'w, 's, ActorAnimParts>;

/// One actor of [`drive`]: its clips chosen and blended and its bones sampled into
/// `out` (written by the caller); `written` is [`BlendPlayer::written_since`] for a dead
/// one. Only the actor's own components are touched.
fn drive_one(item: &mut ActorAnimItem<'_, '_>, written: bool, dt: f32, now: f32, scratch: &mut Vec<(usize, Transform)>, out: &mut Vec<(Entity, Transform)>, cost: &mut ActorAnimCost) {
    let (lo, player, weapon, dying, flinch, seated, idle, pose, creature, name, talk) = item;
    let (flinch, seated, idle, name, talk) = (*flinch, *seated, *idle, *name, *talk);
    // Set below for a standing actor's idle only.
    player.over_movement = 0;
    player.pose_only = None;
    player.pose_only_mask = None;
    if let Some(d) = dying.as_mut() {
        // Dead: the death pose to its end and held; without one the pose stays (and
        // whoever has the bones now, a ragdoll, has written them).
        let Some(clip) = d.clip.clone() else {
            player.forget_pose();
            return;
        };
        // Approximate: this starts after the clip clone, and the pose check ends before
        // `cost` is updated, so those stay in `total` less `held_time`, which the per
        // actor figure divides by sampled actor frames only.
        let dead_t0 = std::time::Instant::now();
        let len = clip.clip.length().max(1e-3);
        d.t = (d.t + dt).min(len);
        let phase = (d.t / len).min(0.999);
        let finished = phase >= 0.999;
        player.layers = Default::default();
        player.layers[0] = Some(Layer { clip, phase, weight: 1.0 });
        // A weapon still on the body stays where its stance hung it.
        if let Some(w) = weapon.as_deref() {
            w.weapon_layers(player);
        }
        // The layers stay on the player for those who read them (`actor_materials`,
        // `anim_sounds`); the bones are sampled only if that makes a different pose.
        match player.sample_held_into(written, scratch, out) {
            None => {
                cost.held_frames += 1;
                cost.finished_held_frames += u64::from(finished);
                cost.held_time += dead_t0.elapsed();
            }
            Some(tracks) => {
                cost.tracks += tracks as u64;
                cost.dead_tracks += tracks as u64;
                cost.finished_sampled_frames += u64::from(finished);
                cost.actor_frames += 1;
            }
        }
        return;
    }
    if let Some((clip, phase)) = seated.and_then(|s| s.pose.clone()) {
        // Getting onto, on, or getting off furniture (`furniture.rs`): its clip, and
        // over it the idle the actor plays there (a seated speaker's gesture). The
        // weapon stays where its stance hangs it, following that bone.
        player.layers = Default::default();
        player.layers[0] = Some(Layer { clip, phase, weight: 1.0 });
        if let Some(w) = weapon.as_mut() {
            if !w.hold {
                w.step(dt);
            }
            w.weapon_layers(player);
        }
        if let Some(i) = idle {
            i.layers(player);
        }
        cost.tracks += player.sample_into(scratch, out) as u64;
        cost.actor_frames += 1;
        return;
    }
    if !lo.hold {
        // A drawn weapon's own walks, runs and turns (`WeaponMoves`).
        let drawn = weapon.as_deref().filter(|w| crate::attack_anims::drawn(Some(w)));
        lo.set_armed(drawn.map(|w| &w.stance.moves));
        lo.step(dt);
        if let Some((prefix, mv, path)) = lo.armed_clip_news() {
            info!("armed locomotion: {} {prefix} {} plays {path}", name.map_or("actor", |n| n.as_str()), mv.group());
        }
    }
    cost.turn_frames += u64::from(lo.blend.cur.mv.is_turn());
    lo.layers(player);
    let drawn = crate::attack_anims::drawn(weapon.as_deref());
    if let Some(w) = weapon.as_mut() {
        if !w.hold {
            // Sneaking: the stance's crouched equip, aim and unequip clips
            // (`weapons::WeaponHold::set_crouched`); in water the body is upright.
            w.set_crouched(lo.sneaking && !lo.swimming);
            w.step(dt);
            if let Some((clip, path)) = w.crouched_clip_news() {
                info!("crouched weapon: {} {} {clip:?} plays {path}", name.map_or("actor", |n| n.as_str()), w.stance.prefix);
            }
        }
        w.layers(player);
    }
    // A creature's unarmed stance (`creature_stance.rs`) on the same layers.
    if let Some(c) = creature.as_mut() {
        if !lo.hold {
            c.step(dt);
        }
        c.layers(player);
    }
    let idles = crate::idles::IDLE_LAYER;
    (player.layers[idles], player.layers[idles + 1]) = (None, None);
    if let Some(i) = idle {
        i.layers(player);
        // A standing actor's idle shows in place of its base idle on every bone it drives,
        // whatever the priorities of their tracks (`BlendPlayer::over_movement`): Mr.
        // House's chamber is closed by DynamicIdle_Sleeping (priority 40) and opened by
        // WakeUp (30), both under his mtidle.kf (50), the pod open [observed: the
        // ControlledBlock priorities of the clips in `creatures/nvmrhouse`; owner, #2425: the original
        // shows the chamber sealed until the terminal opens it]. 129 of the 556 idle files
        // under an `IdleAnims` folder of the base archive have some tracks under their
        // base idle's [observed: survey 2026-11-14]. Not while it moves: an idle shown
        // over a walk keeps the walk's legs [hypothesis] (animation.md §9.4).
        let standing = lo.velocity[0].hypot(lo.velocity[1]) < fnv_actor::locomotion::STAND_SPEED && !lo.blend.cur.mv.is_moving();
        if standing {
            player.over_movement = (1 << idles) | (1 << (idles + 1));
        }
    }
    if let Some(f) = flinch {
        let (len, t) = (f.clip.clip.length().max(1e-3), now - f.at);
        if t < len {
            player.layers[3] = Some(Layer { clip: f.clip.clone(), phase: t / len, weight: 1.0 });
        }
    }
    // A fighter's aim and attack (`attack_anims.rs`) over all of it; a creature's blow
    // shows on its head and neck while it moves.
    let moving = lo.velocity[0].hypot(lo.velocity[1]) >= fnv_actor::locomotion::STAND_SPEED;
    if let Some(p) = pose.as_mut() {
        p.moving = moving;
        p.layers(player, drawn, dt);
        if moving {
            let blow = p.attack.as_ref().filter(|a| a.melee && player.layers[crate::attack_anims::ATTACK_LAYER].is_some());
            if blow.is_some_and(|a| a.head_blow == Some(true)) {
                cost.head_blow_frames += 1;
            }
        }
    }
    if moving && !lo.hold && !lo.blend.cur.mv.is_moving() && lo.blend.prev.is_none_or(|p| !p.mv.is_moving()) && lo.jump_clip().is_none() {
        cost.clipless_frames += 1;
        if lo.clipless_logged < 1 {
            lo.clipless_logged += 1;
            info!("actor moves with no movement clip: {} at {:.0} units/s, {:?} playing", name.map_or("actor", |n| n.as_str()), lo.velocity[0].hypot(lo.velocity[1]), lo.blend.cur.mv);
        }
    }
    // A stimpak's upper-body idle over the fight's pose too (`npc_stimpaks.rs`).
    if let Some(i) = idle.filter(|i| i.over_pose()) {
        i.layers(player);
    }
    // A robot's `Talking` while it says a line (`talk_clips.rs`).
    if let Some(t) = talk {
        t.layers(player);
    }
    player.pose_only = lo.under_layer(player);
    let under = player.pose_only.as_ref().is_some_and(|l| l.weight > 0.0);
    cost.under_frames += u64::from(under);
    let layers = player.layers.iter().flatten().count() as u64 + u64::from(under);
    cost.layer_frames += layers;
    cost.blend_frames += u64::from(layers > 1);
    cost.tracks += player.sample_into(scratch, out) as u64;
    cost.actor_frames += 1;
}

impl ActorAnimCost {
    /// Add the per-actor counts of `o` (one task of [`drive`]); the totals of time and
    /// frames, and the peak, are the caller's.
    fn absorb(&mut self, o: &ActorAnimCost) {
        self.actor_frames += o.actor_frames;
        self.layer_frames += o.layer_frames;
        self.tracks += o.tracks;
        self.blend_frames += o.blend_frames;
        self.turn_frames += o.turn_frames;
        self.under_frames += o.under_frames;
        self.clipless_frames += o.clipless_frames;
        self.held_frames += o.held_frames;
        self.finished_held_frames += o.finished_held_frames;
        self.held_time += o.held_time;
        self.finished_sampled_frames += o.finished_sampled_frames;
        self.dead_tracks += o.dead_tracks;
        self.head_blow_frames += o.head_blow_frames;
    }
}

/// A standing actor turning on the spot to a compass heading (radians: 0 north, +y;
/// π/2 east, +x), at [`SPOT_TURN_RATE`]; its turn clip plays meanwhile (actors.md §8.9).
/// Put it on an actor's root (the entity with its [`Locomotion`]); [`turn_to`] takes it
/// off again once the actor faces the heading. Not for actors something else turns every
/// frame (`nav.rs` travellers: `fnv_nav::Walker::face`; the walk test).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct TurnTo {
    pub heading: f32,
}

/// The compass heading an actor's root faces: its forward (+y) turned by `rotation`.
pub fn heading_of_rotation(rotation: Quat) -> f32 {
    let f = rotation * Vec3::Y;
    f.x.atan2(f.y)
}

/// One step of a turn on the spot from `heading` to `target` over `dt` seconds: how far
/// to turn (radians, positive = clockwise seen from above, as headings grow) and whether
/// that reaches the target.
pub fn spot_turn(heading: f32, target: f32, dt: f32) -> (f32, bool) {
    let (d, max) = (heading_delta(heading, target), SPOT_TURN_RATE * dt.max(0.0));
    (d.clamp(-max, max), d.abs() <= max)
}

/// FaceObject and Turn (`fnv_game::nvse_jip13`): the actor turns on the spot to the
/// heading, playing its turn clip.
pub fn script_turns(mut commands: Commands, script: Option<ResMut<nvf_play::script::ScriptState>>, actors: Query<(Entity, &crate::limbs::ActorRig), With<Locomotion>>) {
    let Some(rt) = script.and_then(|s| s.into_inner().rt.as_mut()) else { return };
    for (r, heading) in rt.game.take_turns() {
        match actors.iter().find(|(_, rig)| rig.reference == r.0) {
            Some((e, _)) => {
                commands.entity(e).insert(TurnTo { heading });
                info!("locomotion: a script turns {r} to {:.0}°", heading.to_degrees());
            }
            None => debug!("locomotion: a script turns {r}, which is not drawn"),
        }
    }
}

/// Turn the actors with a [`TurnTo`]: the root about its parent's z, and the turn rate
/// that makes [`drive`] choose the turn clip. An actor whose `TurnTo` is taken away
/// mid-turn stops turning.
pub fn turn_to(
    time: Res<Time>,
    mut commands: Commands,
    mut turning: Query<(Entity, &TurnTo, &mut Transform, &mut Locomotion), Without<Dying>>,
    mut ended: RemovedComponents<TurnTo>,
    mut others: Query<&mut Locomotion, Without<TurnTo>>,
    test: Option<ResMut<TurnTest>>,
) {
    for e in ended.read() {
        if let Ok(mut lo) = others.get_mut(e) {
            lo.turn = 0.0;
        }
    }
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let mut done_now = 0;
    for (e, to, mut tr, mut lo) in &mut turning {
        if lo.hold {
            continue;
        }
        let (step, done) = spot_turn(heading_of_rotation(tr.rotation), to.heading, dt);
        // Headings grow clockwise; rotations about z and `Locomotion::turn` the other way.
        tr.rotation = Quat::from_rotation_z(-step) * tr.rotation;
        lo.turn = if done { 0.0 } else { -step / dt };
        if done {
            commands.entity(e).remove::<TurnTo>();
            done_now += 1;
        }
    }
    if let Some(mut t) = test {
        t.finished += done_now;
    }
}

/// `--actor-turn-test <radius>`: standing actors within `radius` game units of the
/// camera turn on the spot to face it whenever it is more than [`SPOT_TURN_START`] off
/// their heading (actors.md §8.9).
#[derive(Resource, Debug, Clone, Copy)]
pub struct TurnTest {
    pub radius: f32,
    /// Turns begun and finished, and the degrees asked for in all.
    pub started: u32,
    pub finished: u32,
    pub degrees: f32,
}

type TurnTestActors = (Without<TurnTo>, Without<WalkTest>, Without<Dying>);

/// Give the actors near the camera a [`TurnTo`] towards it.
pub fn turn_test(
    mut commands: Commands,
    mut test: ResMut<TurnTest>,
    camera: Query<&GlobalTransform, crate::fly::WorldCamera>,
    actors: Query<(Entity, &GlobalTransform, &Transform, &Locomotion), TurnTestActors>,
) {
    let Ok(cam) = camera.single() else { return };
    let eye = crate::world::bevy_to_game(cam.translation());
    for (e, at, tr, lo) in &actors {
        let d = (eye - crate::world::bevy_to_game(at.translation())).truncate();
        if lo.hold || lo.velocity != [0.0; 2] || d.length() > test.radius || d.length() < 1.0 {
            continue;
        }
        let heading = d.x.atan2(d.y);
        let off = heading_delta(heading_of_rotation(tr.rotation), heading);
        if off.abs() > SPOT_TURN_START {
            commands.entity(e).insert(TurnTo { heading });
            test.started += 1;
            test.degrees += off.abs().to_degrees();
        }
    }
}

/// The turn test's outcome when the app exits.
pub fn turn_test_report(mut exit: MessageReader<AppExit>, test: Res<TurnTest>) {
    if exit.read().next().is_some() {
        info!("turn test: {} turns on the spot begun, {} finished, {:.0} degrees in all", test.started, test.finished, test.degrees);
    }
}

/// The walk test's line length (units) and turn speed (radians per second), its pause,
/// and the backward walk's share of the speed (actors.md §8.4; test choices).
pub const WALK_TEST_LENGTH: f32 = 300.0;
pub const WALK_TEST_TURN: f32 = FRAC_PI_2;
pub const WALK_TEST_PAUSE: f32 = 1.0;
pub const WALK_TEST_BACK: f32 = 0.75;

/// Where the walk test puts an actor at a time: distance along its placement's forward
/// axis, heading (radians, positive = left) and the velocity and turn rate that go with
/// them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WalkSample {
    pub offset: f32,
    pub heading: f32,
    pub velocity: [f32; 2],
    pub turn: f32,
}

/// One leg of the script: length in seconds, offset and heading at its start, velocity
/// in the actor's axes and turn rate.
fn legs(speed: f32) -> [(f32, f32, f32, [f32; 2], f32); 10] {
    let (l, v) = (WALK_TEST_LENGTH, speed.max(1.0));
    let (walk, back, turn, pause) = (l / v, l / (v * WALK_TEST_BACK), PI / WALK_TEST_TURN, WALK_TEST_PAUSE);
    [
        (walk, 0.0, 0.0, [0.0, v], 0.0),
        (pause, l, 0.0, [0.0; 2], 0.0),
        (turn, l, 0.0, [0.0; 2], WALK_TEST_TURN),
        (walk, l, PI, [0.0, v], 0.0),
        (pause, 0.0, PI, [0.0; 2], 0.0),
        (turn, 0.0, PI, [0.0; 2], -WALK_TEST_TURN),
        (walk, 0.0, 0.0, [0.0, v], 0.0),
        (pause, l, 0.0, [0.0; 2], 0.0),
        (back, l, 0.0, [0.0, -v * WALK_TEST_BACK], 0.0),
        (pause, 0.0, 0.0, [0.0; 2], 0.0),
    ]
}

/// The script's period in seconds.
pub fn walk_test_period(speed: f32) -> f32 {
    legs(speed).iter().map(|l| l.0).sum()
}

/// The walk test at time `t` (seconds, repeating): forward out, stand, turn left 180°,
/// forward back, stand, turn right 180°, forward out, stand, backward back, stand.
pub fn walk_test_at(t: f32, speed: f32) -> WalkSample {
    let mut t = t.rem_euclid(walk_test_period(speed));
    for (len, offset, heading, velocity, turn) in legs(speed) {
        if t < len {
            // Velocity is in the actor's axes; along the line it is reversed when the
            // actor faces back.
            let along = if heading.abs() > FRAC_PI_2 { -velocity[1] } else { velocity[1] };
            return WalkSample { offset: offset + along * t, heading: heading + turn * t, velocity, turn };
        }
        t -= len;
    }
    WalkSample { offset: 0.0, heading: 0.0, velocity: [0.0; 2], turn: 0.0 }
}

/// An actor moved by the walk test (`--actor-walk-test`).
#[derive(Component, Debug, Clone)]
pub struct WalkTest {
    /// The actor's placement (parent space).
    pub base: Transform,
    pub speed: f32,
    /// Script time.
    pub t: f32,
}

impl WalkTest {
    /// The actor's transform for a sample: along the placement's +y, turned about its z.
    pub fn transform(&self, s: &WalkSample) -> Transform {
        self.base * Transform::from_xyz(0.0, s.offset, 0.0).with_rotation(Quat::from_rotation_z(s.heading))
    }

    /// Advance by `dt`: the new transform, with the actor's velocity and turn rate set.
    pub fn step(&mut self, dt: f32, lo: &mut Locomotion) -> Transform {
        self.t += dt;
        let s = walk_test_at(self.t, self.speed);
        lo.velocity = s.velocity;
        lo.turn = s.turn;
        self.transform(&s)
    }
}

/// Run the walk test and the blend `secs` seconds ahead in fixed 1/60 s steps (for
/// `--actor-pose-time` with the walk test: a repeatable pose); returns the transform.
pub fn simulate(wt: &mut WalkTest, lo: &mut Locomotion, secs: f32) -> Transform {
    const DT: f32 = 1.0 / 60.0;
    let mut tr = wt.transform(&walk_test_at(wt.t, wt.speed));
    let mut left = secs;
    while left > 0.0 {
        let dt = left.min(DT);
        tr = wt.step(dt, lo);
        lo.step(dt);
        left -= dt;
    }
    tr
}

/// Move the actors under the walk test.
pub fn walk_test(time: Res<Time>, mut q: Query<(&mut WalkTest, &mut Locomotion, &mut Transform)>) {
    let dt = time.delta_secs();
    for (mut wt, mut lo, mut tr) in &mut q {
        if lo.hold {
            continue;
        }
        *tr = wt.step(dt, &mut lo);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_test_script() {
        let v = 100.0;
        // 3 s out, 1 s stand, 2 s turn, 3 s back, 1 s, 2 s, 3 s out, 1 s, 4 s back, 1 s.
        assert!((walk_test_period(v) - 21.0).abs() < 1e-4);
        let at = |t: f32| walk_test_at(t, v);
        assert_eq!(at(1.5), WalkSample { offset: 150.0, heading: 0.0, velocity: [0.0, 100.0], turn: 0.0 });
        assert_eq!(at(3.5).velocity, [0.0; 2]);
        let s = at(5.0);
        assert!((s.heading - FRAC_PI_2).abs() < 1e-5 && s.turn > 0.0 && s.offset == 300.0);
        // Walking back facing back: forward in the actor's axes, decreasing offset.
        let s = at(7.5);
        assert!((s.offset - 150.0).abs() < 1e-3 && s.velocity == [0.0, 100.0] && (s.heading - PI).abs() < 1e-5);
        let s = at(11.0);
        assert!((s.heading - FRAC_PI_2).abs() < 1e-5 && s.turn < 0.0 && s.offset == 0.0);
        // The backward leg: facing out, moving back.
        let s = at(18.0);
        assert!((s.offset - 150.0).abs() < 1e-3 && s.velocity == [0.0, -75.0] && s.heading == 0.0);
        // It repeats, and positions join up between legs.
        assert_eq!(at(1.5 + 21.0).offset, at(1.5).offset);
        for k in 1..10 {
            let edge: f32 = legs(v)[..k].iter().map(|l| l.0).sum();
            let (a, b) = (at(edge - 1e-3), at(edge + 1e-3));
            assert!((a.offset - b.offset).abs() < 0.2 && (a.heading - b.heading).abs() < 0.01, "leg {k}: {a:?} {b:?}");
        }
    }

    #[test]
    fn turning_on_the_spot_to_a_heading() {
        // Facing north; east is a quarter turn to the right at 120°/s: 0.75 s.
        let mut rot = Quat::IDENTITY;
        assert!(heading_of_rotation(rot).abs() < 1e-6);
        let (dt, mut steps) = (0.05, 0);
        loop {
            let (step, done) = spot_turn(heading_of_rotation(rot), FRAC_PI_2, dt);
            assert!(step > 0.0 && step <= SPOT_TURN_RATE * dt + 1e-6);
            rot = Quat::from_rotation_z(-step) * rot;
            steps += 1;
            if done {
                break;
            }
            assert!(steps < 100);
        }
        assert!((15..=16).contains(&steps), "{steps}");
        assert!((heading_of_rotation(rot) - FRAC_PI_2).abs() < 1e-4);
        // The actor's forward now points east (+x), and it stands upright.
        assert!((rot * Vec3::Y - Vec3::X).length() < 1e-4 && (rot * Vec3::Z - Vec3::Z).length() < 1e-5);
        // West from north is to the left, through north-west rather than round by south.
        let (step, done) = spot_turn(0.0, -FRAC_PI_2, dt);
        assert!(step < 0.0 && !done);
        let (step, _) = spot_turn(0.9 * PI, -0.9 * PI, dt);
        assert!(step > 0.0);
        // The turn rate that goes with a step makes the chooser play the turn clips.
        let s = set();
        let turn = |step: f32| choose(|m| s.info(m), [0.0; 2], -step / dt).mv;
        assert_eq!(turn(SPOT_TURN_RATE * dt), Move::TurnRight);
        assert_eq!(turn(-SPOT_TURN_RATE * dt), Move::TurnLeft);
    }

    #[test]
    fn swimming_takes_the_swim_set_unarmed_and_fills_missing_moves() {
        let stand = set();
        // A creature with a swimming walk and no swimming idle: the idle is the standing one.
        let mut only_walk = (*set()).clone();
        only_walk.clips.retain(|m, _| *m == Move::Forward);
        only_walk.clips.get_mut(&Move::Forward).expect("walk").path = "swimmtforward.kf".into();
        assert!(ClipSet::swim_from(vec![ClipSet::default(), ClipSet::default()], &stand).is_none());
        let swim = Arc::new(ClipSet::swim_from(vec![only_walk.clone(), ClipSet::default()], &stand).expect("swim set"));
        assert_eq!(swim.clips[&Move::Idle].path, "mtIdle.kf");
        assert_eq!(swim.clips[&Move::Forward].path, "swimmtforward.kf");
        let mut lo = Locomotion::new(stand.clone(), 0.0);
        assert!(!lo.set_swimming(true), "no swimming set: nothing changes");
        lo.swim = Some(swim.clone());
        lo.sneaking = true;
        assert!(lo.set_swimming(true));
        assert!(Arc::ptr_eq(&lo.set, &swim), "swimming wins over sneaking");
        assert!(!lo.set_swimming(true));
        assert!(lo.set_swimming(false));
        assert!(!Arc::ptr_eq(&lo.set, &swim));
    }

    #[test]
    fn crouching_and_standing_fade_from_the_old_pose() {
        let (stand, sneak) = (set(), set());
        let mut lo = Locomotion::new(stand.clone(), 0.0);
        lo.sneak = Some(sneak.clone());
        // Walking in the standing set, the walk's fade-in over.
        lo.velocity = [0.0, 85.26];
        for _ in 0..30 {
            lo.step(1.0 / 60.0);
        }
        assert_eq!((lo.blend.cur.mv, lo.blend.prev), (Move::Forward, None));
        let phase = lo.blend.cur.phase;
        // Crouching: the walk goes on as the sneaking walk at the same footfall, and the
        // standing walk fades out from the old set over the clip's blend time (0.2 s).
        assert!(lo.set_sneaking(true));
        assert!(Arc::ptr_eq(&lo.set, &sneak));
        assert_eq!(lo.blend.cur.mv, Move::Forward);
        assert!((lo.blend.cur.phase - phase).abs() < 1e-6);
        assert_eq!(lo.blend.prev.map(|p| p.mv), Some(Move::Forward));
        assert!(lo.prev_set.as_ref().is_some_and(|s| Arc::ptr_eq(s, &stand)));
        assert_eq!(lo.blend.weight(), 0.0);
        lo.step(0.1);
        let w = lo.blend.weight();
        assert!(w > 0.0 && w < 1.0, "{w}");
        assert!(lo.prev_set.is_some());
        lo.step(0.15);
        assert_eq!((lo.blend.prev, lo.blend.weight()), (None, 1.0));
        assert!(lo.prev_set.is_none());
        // Standing up while still: from the crouched idle, faded the same way.
        lo.velocity = [0.0; 2];
        for _ in 0..60 {
            lo.step(1.0 / 60.0);
        }
        assert_eq!((lo.blend.cur.mv, lo.blend.prev), (Move::Idle, None));
        assert!(lo.set_sneaking(false));
        assert!(Arc::ptr_eq(&lo.set, &stand) && lo.prev_set.as_ref().is_some_and(|s| Arc::ptr_eq(s, &sneak)));
        assert_eq!(lo.blend.prev.map(|p| p.mv), Some(Move::Idle));
        // A move asked for during the fade waits for it to end, then fades within the set.
        lo.velocity = [0.0, 85.26];
        lo.step(0.1);
        assert_eq!(lo.blend.cur.mv, Move::Idle);
        lo.step(0.15);
        assert_eq!(lo.blend.cur.mv, Move::Forward);
        assert!(lo.prev_set.is_none(), "the idle fading out now is the standing set's");
    }

    /// A corpse with `clip` on the one bone it has (bind transform: the identity); `drive`
    /// as a system that keeps its state from run to run, as in the schedule.
    fn corpse(clip: &Arc<BoundClip>) -> (World, bevy::ecs::system::SystemId, Entity, Entity) {
        let mut world = World::new();
        world.insert_resource(Time::<()>::default());
        world.init_resource::<ActorAnimCost>();
        let bone = world.spawn(Transform::IDENTITY).id();
        let player = BlendPlayer::new(vec![bone], Arc::new(vec![Transform::IDENTITY]));
        let actor = world.spawn((Locomotion::new(set(), 0.0), player, Dying { clip: Some(clip.clone()), t: 0.0 })).id();
        let id = world.register_system(drive);
        (world, id, actor, bone)
    }

    /// A clip one second long with one track, on the corpse's bone.
    fn death_clip(x: f32) -> Arc<BoundClip> {
        use fnv_nif::anim::{Channel, TransformTrack};
        let track = Arc::new(TransformTrack { translation: Channel::Static(Some([x, 0.0, 0.0])), rotation: Channel::Static(None), euler: None, scale: Channel::Static(None), per_cycle: None });
        let clip = nvf_assets::anim_play::Clip {
            name: String::new(), start: 0.0, stop: 1.0, frequency: 1.0, cycle_type: 0, tracks: Vec::new(), priorities: Vec::new(), skipped: 0, accum_root: None, text_keys: Vec::new(), visibility: Vec::new(), emitters: Vec::new(), materials: Vec::new(), free: Vec::new(), free_visibility: Vec::new(), lights: Vec::new(),
        };
        Arc::new(BoundClip { clip: Arc::new(clip), tracks: vec![(0, track, 30)], root: None, root_track: None, carried: None, hold_xy: None, unbound: Default::default() })
    }

    /// One frame of `secs`: (tracks sampled so far, frames held so far, frames at the
    /// clip's end and sampled so far, tracks sampled on the dead so far).
    fn frame(world: &mut World, id: bevy::ecs::system::SystemId, secs: f32) -> (u64, u64, u64, u64) {
        world.resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(secs));
        world.run_system(id).unwrap();
        let c = world.resource::<ActorAnimCost>();
        (c.tracks, c.held_frames, c.finished_sampled_frames, c.dead_tracks)
    }

    #[test]
    fn a_finished_death_pose_is_not_sampled_again() {
        let clip = death_clip(9.0);
        let (mut world, id, actor, bone) = corpse(&clip);
        // [design, #2352] A living fallback must be discarded on the death path too.
        world.get_mut::<BlendPlayer>(actor).unwrap().pose_only = Some(Layer { clip: death_clip(3.0), phase: 0.0, weight: 1.0 });
        let posed = Transform::from_xyz(9.0, 0.0, 0.0);
        // Playing: a pose a frame; the frame it reaches its end, the last.
        assert_eq!(frame(&mut world, id, 0.4), (1, 0, 0, 1));
        assert!(world.get::<BlendPlayer>(actor).unwrap().pose_only.is_none());
        assert_eq!(frame(&mut world, id, 0.4), (2, 0, 0, 2));
        assert_eq!(frame(&mut world, id, 0.4), (3, 0, 1, 3));
        assert_eq!(*world.get::<Transform>(bone).unwrap(), posed);
        // Held: nothing sampled, the bone as it was, and not counted as posed.
        let posed_frames = world.resource::<ActorAnimCost>().actor_frames;
        for held in 1..=3 {
            assert_eq!(frame(&mut world, id, 0.1), (3, held, 1, 3));
            assert_eq!(*world.get::<Transform>(bone).unwrap(), posed);
        }
        let c = world.resource::<ActorAnimCost>();
        assert_eq!(c.actor_frames, posed_frames);
        assert_eq!(c.finished_held_frames, 3);
        assert_eq!(c.frames, 6);
    }

    #[test]
    fn a_pose_held_part_way_through_its_clip_is_not_counted_as_finished() {
        let clip = death_clip(9.0);
        let (mut world, id, ..) = corpse(&clip);
        assert_eq!(frame(&mut world, id, 0.4), (1, 0, 0, 1));
        // Time stopped: the same phase, nothing sampled, and the clip not at its end.
        assert_eq!(frame(&mut world, id, 0.0), (1, 1, 0, 1));
        assert_eq!(world.resource::<ActorAnimCost>().finished_held_frames, 0);
        // Time on: it plays on.
        assert_eq!(frame(&mut world, id, 0.2), (2, 1, 0, 2));
    }

    #[test]
    fn a_finished_death_pose_gives_way_at_once() {
        let clip = death_clip(9.0);
        let (mut world, id, actor, bone) = corpse(&clip);
        let posed = Transform::from_xyz(9.0, 0.0, 0.0);
        let moved = Transform::from_xyz(1.0, 2.0, 3.0);
        assert_eq!(frame(&mut world, id, 2.0), (1, 0, 1, 1));
        assert_eq!(frame(&mut world, id, 0.1), (1, 1, 1, 1));

        // Another system wrote the bone (foot IK easing out): put back the next frame, as
        // before, then held again.
        world.clear_trackers();
        *world.get_mut::<Transform>(bone).unwrap() = moved;
        assert_eq!(frame(&mut world, id, 0.1), (2, 1, 2, 2));
        assert_eq!(*world.get::<Transform>(bone).unwrap(), posed);
        assert_eq!(frame(&mut world, id, 0.1), (2, 2, 2, 2));

        // A new clip on the actor (a get-up): sampled from its first frame.
        let other = death_clip(7.0);
        world.entity_mut(actor).insert(Dying { clip: Some(other), t: 0.0 });
        assert_eq!(frame(&mut world, id, 0.1).0, 3);
        assert_eq!(*world.get::<Transform>(bone).unwrap(), Transform::from_xyz(7.0, 0.0, 0.0));

        // A ragdoll takes over: no clip, no pose, and what it writes stays.
        let (tracks, held, ..) = frame(&mut world, id, 0.1);
        assert_eq!(tracks, 4);
        world.get_mut::<Dying>(actor).unwrap().clip = None;
        world.clear_trackers();
        *world.get_mut::<Transform>(bone).unwrap() = moved;
        assert_eq!(frame(&mut world, id, 0.1).0, tracks);
        assert_eq!(frame(&mut world, id, 0.1).1, held);
        assert_eq!(*world.get::<Transform>(bone).unwrap(), moved);
        // The death pose back with the very clip it held (no system does this today; the
        // memory must not outlive the ragdoll): sampled, long after that write.
        world.entity_mut(actor).insert(Dying { clip: Some(clip.clone()), t: 1.0 });
        assert_eq!(frame(&mut world, id, 0.1).0, tracks + 1);
        assert_eq!(*world.get::<Transform>(bone).unwrap(), posed);
        assert_eq!(frame(&mut world, id, 0.1).0, tracks + 1);

        // Resurrected or reset by its cell: alive, on clips with nothing for this bone,
        // which goes back to its bind transform.
        world.entity_mut(actor).remove::<Dying>();
        assert_eq!(frame(&mut world, id, 0.1).0, tracks + 1);
        assert_eq!(*world.get::<Transform>(bone).unwrap(), Transform::IDENTITY);
        // Dead again on the same clip: its pose, not what it held before.
        world.entity_mut(actor).insert(Dying { clip: Some(clip.clone()), t: 1.0 });
        assert_eq!(frame(&mut world, id, 0.1).0, tracks + 2);
        assert_eq!(*world.get::<Transform>(bone).unwrap(), posed);

        // Drawn again (a new player on the bones, a cell reloaded): sampled too.
        world.entity_mut(actor).insert(BlendPlayer::new(vec![bone], Arc::new(vec![Transform::IDENTITY])));
        assert_eq!(frame(&mut world, id, 0.1).0, tracks + 3);
        assert_eq!(frame(&mut world, id, 0.1).0, tracks + 3);
    }

    fn set() -> Arc<ClipSet> {
        // Infos only (no bound tracks are needed to choose and blend).
        let bound = Arc::new(BoundClip { clip: Arc::new(nvf_assets::anim_play::Clip {
            name: String::new(), start: 0.0, stop: 1.0, frequency: 1.0, cycle_type: 0, tracks: Vec::new(), priorities: Vec::new(), skipped: 0, accum_root: None, text_keys: Vec::new(), visibility: Vec::new(), emitters: Vec::new(), materials: Vec::new(), free: Vec::new(), free_visibility: Vec::new(), lights: Vec::new(),
        }), tracks: Vec::new(), root: None, root_track: None, carried: None, hold_xy: None, unbound: Default::default() });
        let mut s = ClipSet::default();
        for (m, duration, speed) in [(Move::Idle, 4.0, 0.0), (Move::Forward, 1.2, 85.26), (Move::Backward, 1.133, 67.44), (Move::TurnLeft, 1.0, 0.0), (Move::TurnRight, 1.0, 0.0)] {
            s.clips.insert(m, SetClip { bound: bound.clone(), info: ClipInfo { duration, speed, foot_phase: 0.3, blend_time: 0.2 }, path: format!("mt{}.kf", m.group()) });
        }
        Arc::new(s)
    }

    // [design, #2352] Synthetic idle: an animated wing (node 1), head (0), and root
    // (2). The other clips drive the head; no game tracks are used.
    fn wing_idle_set() -> Arc<ClipSet> {
        use fnv_nif::anim::{Channel, Key, KeyKind, Keys};
        let mut idle = Arc::try_unwrap(death_clip(1.0)).unwrap();
        let mut wing = (*idle.tracks[0].1).clone();
        wing.translation = Channel::Keys(Keys {
            kind: KeyKind::Linear,
            keys: [(0.0, [2.0, 0.0, 0.0]), (0.5, [6.0, 0.0, 0.0]), (1.0, [2.0, 0.0, 0.0])]
                .map(|(time, value)| Key { time, value, forward: [0.0; 3], backward: [0.0; 3], tbc: [0.0; 3] }).to_vec(),
        });
        idle.tracks.push((1, Arc::new(wing), 45));
        idle.tracks.push((2, idle.tracks[0].1.clone(), 30));
        idle.root = Some(2);
        let mut s = (*set()).clone();
        let c = s.clips.get_mut(&Move::Idle).unwrap();
        c.bound = Arc::new(idle);
        c.info.duration = 1.0;
        s.clips.get_mut(&Move::Forward).unwrap().bound = death_clip(3.0);
        Arc::new(s)
    }

    #[test]
    fn creature_idle_under_aim_attack_and_special_idle_only_drives_missing_bones() {
        for slot in [2, crate::attack_anims::ATTACK_LAYER, crate::idles::IDLE_LAYER + 1] {
            for moving in [false, true] {
                let mut lo = Locomotion::new(wing_idle_set(), 0.0);
                lo.idle_under = true;
                if moving {
                    lo.blend = Blend::new(Move::Forward, 0.0);
                    lo.velocity = [0.0, 85.26];
                }
                let mut player = BlendPlayer::new(vec![Entity::PLACEHOLDER; 3], Arc::new(vec![Transform::IDENTITY; 3]));
                lo.step(0.25);
                lo.layers(&mut player);
                let mut upper = Arc::try_unwrap(death_clip(9.0)).unwrap();
                upper.tracks[0].2 = 60;
                player.layers[slot] = Some(Layer { clip: Arc::new(upper), phase: 0.0, weight: 1.0 });
                let under = lo.under_layer(&mut player).expect("idle under an upper clip");
                assert_eq!(under.phase, 0.25);
                assert_eq!(under.clip.tracks.iter().zip(&player.pose_only_mask.as_ref().unwrap().tracks).filter(|(_, enabled)| **enabled).map(|((i, _, _), _)| *i).collect::<Vec<_>>(), [1]);
                assert!(under.clip.root.is_none());
                let cached = under.clip.clone();
                player.pose_only = Some(under);
                assert!(Arc::ptr_eq(&cached, &lo.under_layer(&mut player).unwrap().clip));
                let mut out = Vec::new();
                player.sample(&mut out);
                assert_eq!(out.iter().find(|(i, _)| *i == 0).unwrap().1.translation.x, 9.0);
                assert_eq!(out.iter().find(|(i, _)| *i == 1).unwrap().1.translation.x, 4.0);
                lo.step(0.25);
                lo.layers(&mut player);
                assert_eq!(lo.under_layer(&mut player).unwrap().phase, 0.5);
            }
        }
    }

    #[test]
    fn creature_idle_under_special_idle_keeps_its_materials_and_animates_missing_bones() {
        use fnv_nif::anim::{BoolTrack, Channel, ValueTrack};
        use nvf_assets::anim_play::{EmitterTrack, LightTarget, LightTrack, MaterialTarget, MaterialTrack, OwnCycle};
        use nvf_assets::material_anim::actor_material_values;

        // [design, #2352] Synthetic equal-priority materials compete while the special
        // idle omits the wing. No game tracks or material names are used.
        let material = |alpha| MaterialTrack {
            shape: "SyntheticPanel".into(),
            target: MaterialTarget::Alpha,
            values: Arc::new(ValueTrack::Float(Channel::Static(Some([alpha])))),
            cycle: None,
            priority: 20,
        };
        let mut set = (*wing_idle_set()).clone();
        let idle = &mut set.clips.get_mut(&Move::Idle).unwrap().bound;
        let bound_idle = Arc::get_mut(idle).unwrap();
        let track = bound_idle.tracks[0].1.clone();
        let source = Arc::make_mut(&mut bound_idle.clip);
        source.materials.push(material(0.0));
        // [design, #2352] Populate every non-pose payload with synthetic controllers
        // so a future clone-and-strip fallback cannot silently retain one.
        let shows = BoolTrack { value: Some(false), keys: Vec::new() };
        let cycle = OwnCycle { start: 0.0, stop: 1.0, frequency: 1.0, phase: 0.0, cycle_type: 0 };
        source.tracks.push(("SyntheticWing".into(), track.clone()));
        source.priorities.push(45);
        source.text_keys.push((0.1, "Sound: SyntheticBuzz".into()));
        source.visibility.push(("SyntheticPanel".into(), 20, shows.clone()));
        source.free_visibility.push(("SyntheticPanel".into(), shows.clone(), cycle));
        source.free.push(("SyntheticWing".into(), track, cycle));
        source.emitters.push(EmitterTrack { system: "SyntheticEmitter".into(), priority: 20, rate: None, active: Some(shows), modifiers: Vec::new() });
        source.lights.push(LightTrack { light: "SyntheticLight".into(), target: LightTarget::Dimmer, values: material(0.0).values });
        let set = Arc::new(set);
        for slot in [crate::idles::IDLE_LAYER, crate::idles::IDLE_LAYER + 1] {
            for moving in [false, true] {
                let mut lo = Locomotion::new(set.clone(), 0.0);
                lo.idle_under = true;
                if moving {
                    lo.blend = Blend::new(Move::Forward, 0.0);
                    lo.velocity = [0.0, 85.26];
                }
                let mut player = BlendPlayer::new(vec![Entity::PLACEHOLDER; 3], Arc::new(vec![Transform::IDENTITY; 3]));
                lo.step(0.25);
                lo.layers(&mut player);
                let mut special = Arc::try_unwrap(death_clip(9.0)).unwrap();
                Arc::make_mut(&mut special.clip).materials.push(material(1.0));
                player.layers[slot] = Some(Layer { clip: Arc::new(special), phase: 0.0, weight: 1.0 });
                {
                    let mut materials = Vec::new();
                    actor_material_values(&player, &mut materials);
                    assert_eq!(materials, [("SyntheticPanel", MaterialTarget::Alpha, [1.0; 3], 1.0)]);
                }

                let under = lo.under_layer(&mut player).expect("idle fills the omitted wing");
                let view = &under.clip.clip;
                assert!(view.materials.is_empty());
                assert!(view.text_keys.is_empty());
                assert!(view.visibility.is_empty());
                assert!(view.free_visibility.is_empty());
                assert!(view.emitters.is_empty());
                assert!(view.lights.is_empty());
                assert!(view.free.is_empty());
                assert!(view.tracks.is_empty());
                assert!(view.priorities.is_empty());
                player.pose_only = Some(under);
                let mut materials = Vec::new();
                actor_material_values(&player, &mut materials);
                assert_eq!(materials, [("SyntheticPanel", MaterialTarget::Alpha, [1.0; 3], 1.0)]);
                assert_eq!(lo.set.clips[&Move::Idle].bound.clip.materials.len(), 1);
                assert_eq!(player.layers[slot].as_ref().unwrap().clip.clip.materials.len(), 1);
                let mut bones = Vec::new();
                player.sample(&mut bones);
                assert_eq!(bones.iter().find(|(i, _)| *i == 1).unwrap().1.translation.x, 4.0);
            }
        }
    }

    #[test]
    fn creature_idle_under_layer_gates_and_refreshes_for_upper_clip_changes() {
        let mut lo = Locomotion::new(wing_idle_set(), 0.0);
        let mut player = BlendPlayer::new(Vec::new(), Arc::new(Vec::new()));
        player.layers[2] = Some(Layer { clip: death_clip(9.0), phase: 0.0, weight: 1.0 });
        assert!(lo.under_layer(&mut player).is_none(), "people have no under-layer");
        lo.idle_under = true;
        let old = lo.under_layer(&mut player).unwrap().clip;
        let mut upper = Arc::try_unwrap(death_clip(8.0)).unwrap();
        upper.tracks.push((1, upper.tracks[0].1.clone(), 60));
        player.layers[4] = Some(Layer { clip: Arc::new(upper), phase: 0.0, weight: 1.0 });
        assert_eq!(lo.under_layer(&mut player).unwrap().weight, 0.0, "all non-root idle bones are driven");
        assert_eq!(player.pose_only_mask.as_ref().unwrap().tracks, [false, false]);
        for weight in [0.0, f32::NAN] {
            player.layers[4].as_mut().unwrap().weight = weight;
            let next = lo.under_layer(&mut player).unwrap();
            assert_eq!(next.weight, 1.0);
            assert!(Arc::ptr_eq(&old, &next.clip), "mask changes preserve the BoundClip allocation");
            assert!(Arc::ptr_eq(&old.clip, &next.clip.clip), "mask changes preserve the timing-only source");
            assert_eq!(player.pose_only_mask.as_ref().unwrap().tracks, [false, true]);
        }
        player.layers = Default::default();
        assert_eq!(lo.under_layer(&mut player).unwrap().weight, 0.0, "no upper clip needs fallback poses");
        player.layers[2] = Some(Layer { clip: death_clip(9.0), phase: 0.0, weight: 1.0 });
        lo.set = wing_idle_set();
        let next = lo.under_layer(&mut player).unwrap().clip;
        assert!(!Arc::ptr_eq(&old, &next), "a different idle refreshes playback identity");
        assert!(!Arc::ptr_eq(&old.clip, &next.clip), "a different idle refreshes the timing-only source");
        let mut no_idle = (*lo.set).clone();
        no_idle.clips.remove(&Move::Idle);
        lo.set = Arc::new(no_idle);
        assert!(lo.under_layer(&mut player).is_none(), "sets without Idle get no fallback");
        assert!(player.pose_only_mask.is_none());
    }

    #[test]
    fn creature_idle_under_accumulating_rotation_survives_upper_and_mask_changes() {
        use fnv_nif::anim::{Channel, Key, KeyKind, Keys};
        // [design, #2352] Synthetic 100-degree cycle on a wing and a part node.
        // Three wraps leave a visible 300-degree accumulation, unlike cycle zero.
        let q = |degrees: f32| Quat::from_rotation_z(degrees.to_radians());
        let mut set = (*wing_idle_set()).clone();
        let idle = Arc::get_mut(&mut set.clips.get_mut(&Move::Idle).unwrap().bound).unwrap();
        let mut spin = (*idle.tracks[1].1).clone();
        spin.rotation = Channel::Keys(Keys {
            kind: KeyKind::Linear,
            keys: (0..=4).map(|k| {
                let r = q(25.0 * k as f32);
                Key { time: k as f32 / 4.0, value: [r.w, r.x, r.y, r.z], forward: [0.0; 4], backward: [0.0; 4], tbc: [0.0; 3] }
            }).collect(),
        });
        let delta = q(100.0);
        spin.per_cycle = Some([delta.w, delta.x, delta.y, delta.z]);
        let spin = Arc::new(spin);
        idle.tracks[1].1 = spin.clone();
        idle.unbound = Arc::new(vec![("SyntheticPart".into(), spin, 45)]);
        let set = Arc::new(set);
        let mut lo = Locomotion::new(set, 0.0);
        lo.idle_under = true;
        let mut player = BlendPlayer::new(vec![Entity::PLACEHOLDER; 3], Arc::new(vec![Transform::IDENTITY; 3]));
        player.add_part_node("syntheticpart", Entity::PLACEHOLDER, Transform::IDENTITY);
        let aim = death_clip(9.0);
        let mut attack = Arc::try_unwrap(death_clip(8.0)).unwrap();
        attack.unbound = Arc::new(vec![("SYNTHETICPART".into(), attack.tracks[0].1.clone(), 60)]);
        let attack = Arc::new(attack);
        let mut covers_all = Arc::try_unwrap(death_clip(7.0)).unwrap();
        covers_all.tracks.push((1, covers_all.tracks[0].1.clone(), 60));
        covers_all.unbound = attack.unbound.clone();
        let covers_all = Arc::new(covers_all);
        let mut out = Vec::new();
        let mut identity = None;
        let same = |a: Quat, b: Quat| a.dot(b).abs() > 1.0 - 1e-5;
        let mut sample = |lo: &mut Locomotion, player: &mut BlendPlayer, upper: Option<&Arc<BoundClip>>, elapsed: f32| {
            lo.under.phase = elapsed.fract();
            player.layers[2] = upper.map(|c| Layer { clip: c.clone(), phase: 0.0, weight: 1.0 });
            player.pose_only = lo.under_layer(player);
            let clip = player.pose_only.as_ref().unwrap().clip.clone();
            if let Some(old) = &identity {
                assert!(Arc::ptr_eq(old, &clip));
            } else {
                identity = Some(clip);
            }
            player.sample(&mut out);
            out.clone()
        };
        // [design, #2352] Quarter-cycle samples let Loops see every wrap.
        for step in 3..=12 {
            let elapsed = step as f32 / 4.0;
            let pose = sample(&mut lo, &mut player, Some(&aim), elapsed);
            for node in [1, 3] {
                assert!(same(pose.iter().find(|(i, _)| *i == node).unwrap().1.rotation, q(100.0 * elapsed)));
            }
        }
        let pose = sample(&mut lo, &mut player, Some(&attack), 3.25);
        assert_eq!(player.pose_only_mask.as_ref().unwrap().unbound, [false]);
        assert!(same(pose.iter().find(|(i, _)| *i == 1).unwrap().1.rotation, q(325.0)));
        assert!(same(pose.iter().find(|(i, _)| *i == 3).unwrap().1.rotation, Quat::IDENTITY));
        // [design, #2352] A fully covered interval and an interval without uppers
        // retain the same timeline even though fallback tracks contribute nothing.
        for step in 14..=16 {
            sample(&mut lo, &mut player, Some(&covers_all), step as f32 / 4.0);
            assert_eq!(player.pose_only.as_ref().unwrap().weight, 0.0);
        }
        for step in 17..=20 {
            sample(&mut lo, &mut player, None, step as f32 / 4.0);
            assert_eq!(player.pose_only.as_ref().unwrap().weight, 0.0);
        }
        let pose = sample(&mut lo, &mut player, Some(&aim), 5.25);
        for node in [1, 3] {
            assert!(same(pose.iter().find(|(i, _)| *i == node).unwrap().1.rotation, q(525.0)));
        }
        let before = player.pose_only.as_ref().unwrap().clip.clone();
        lo.set = wing_idle_set();
        player.pose_only = lo.under_layer(&mut player);
        assert!(!Arc::ptr_eq(&before, &player.pose_only.as_ref().unwrap().clip));
        player.sample(&mut out);
        assert!(same(out.iter().find(|(i, _)| *i == 1).unwrap().1.rotation, Quat::IDENTITY));
    }

    /// `base` with the clips of `moves` (move, file) put in, at the same infos.
    fn with(base: &ClipSet, moves: &[(Move, &str)]) -> ClipSet {
        let mut s = base.clone();
        for (m, path) in moves {
            let c = base.clips.get(m).or_else(|| base.clips.get(&Move::Forward)).expect("a clip to copy").clone();
            s.clips.insert(*m, SetClip { path: path.to_string(), ..c });
        }
        s
    }

    fn path(lo: &Locomotion, m: Move) -> &str {
        &lo.set.clips[&m].path
    }

    #[test]
    fn a_drawn_weapon_moves_with_its_own_clips() {
        let stand = set();
        let only = |moves: &[(Move, &str)]| {
            let mut s = with(&stand, moves);
            s.clips.retain(|m, _| moves.iter().any(|(k, _)| k == m));
            s
        };
        let pistol = Arc::new(WeaponMoves {
            prefix: "1hp".into(),
            stand: only(&[(Move::Forward, r"locomotion\1hpforward.kf"), (Move::TurnLeft, "1hpturnleft.kf")]),
            sneak: ClipSet::default(),
            hurt: only(&[(Move::Backward, r"hurt\1hpbackward_hurt.kf")]),
        });
        let sneak = Arc::new(with(&stand, &[(Move::Idle, "sneakmtidle.kf"), (Move::Forward, "sneakmtforward.kf")]));
        let hurt = Arc::new(with(&stand, &[(Move::Forward, r"hurt\mtforward_hurt.kf"), (Move::TurnLeft, r"hurt\mtturnleft.kf")]));
        let mut lo = Locomotion::new(stand.clone(), 0.0);
        lo.sneak = Some(sneak.clone());
        lo.hurt = Some(hurt.clone());
        // Walking unarmed.
        lo.velocity = [0.0, 85.26];
        for _ in 0..30 {
            lo.step(1.0 / 60.0);
        }
        assert_eq!((lo.blend.cur.mv, path(&lo, Move::Forward)), (Move::Forward, "mtForward.kf"));
        assert_eq!(lo.armed_clip_news(), None);
        let phase = lo.blend.cur.phase;
        // The weapon drawn: the walk goes on as the pistol's walk, faded in from the
        // unarmed one; the moves the pistol has no clip for stay unarmed (the idle and the
        // walk back).
        assert!(lo.set_armed(Some(&pistol)));
        assert!(!lo.set_armed(Some(&pistol)), "drawn already");
        assert_eq!(path(&lo, Move::Forward), r"locomotion\1hpforward.kf");
        assert_eq!(path(&lo, Move::TurnLeft), "1hpturnleft.kf");
        assert_eq!(path(&lo, Move::Idle), "mtIdle.kf");
        assert_eq!(path(&lo, Move::Backward), "mtBackward.kf");
        assert_eq!(lo.blend.cur.mv, Move::Forward);
        assert!((lo.blend.cur.phase - phase).abs() < 1e-6);
        assert!(lo.prev_set.as_ref().is_some_and(|s| Arc::ptr_eq(s, &stand)));
        assert_eq!(lo.blend.weight(), 0.0);
        lo.step(0.25);
        assert_eq!((lo.blend.prev, lo.blend.weight()), (None, 1.0));
        assert_eq!(lo.armed_clip_news(), Some(("1hp", Move::Forward, r"locomotion\1hpforward.kf".to_string())));
        assert_eq!(lo.armed_clip_news(), None, "once per file");
        // Crouched: the crouched set, the pistol having no crouched clips.
        assert!(lo.set_sneaking(true));
        assert!(Arc::ptr_eq(&lo.set, &sneak));
        assert!(lo.set_sneaking(false));
        // Limping: the unarmed limp wins over the pistol's walk, the pistol's own limp and
        // turn over the unarmed ones.
        assert!(lo.set_limping(true));
        assert_eq!(path(&lo, Move::Forward), r"hurt\mtforward_hurt.kf");
        assert_eq!(path(&lo, Move::Backward), r"hurt\1hpbackward_hurt.kf");
        assert_eq!(path(&lo, Move::TurnLeft), "1hpturnleft.kf");
        // Put away: the unarmed limp, faded from the armed one.
        assert!(lo.set_armed(None));
        assert!(!lo.set_armed(None));
        assert!(Arc::ptr_eq(&lo.set, &hurt));
        assert!(lo.set_limping(false));
        assert!(Arc::ptr_eq(&lo.set, &stand));
        // A stance without movement clips changes nothing.
        assert!(!lo.set_armed(Some(&Arc::new(WeaponMoves::default()))));
        assert!(Arc::ptr_eq(&lo.set, &stand));
    }

    #[test]
    fn walk_test_drives_the_moves() {
        let mut lo = Locomotion::new(set(), 0.0);
        let mut wt = WalkTest { base: Transform::IDENTITY, speed: 85.26, t: 0.0 };
        let mut seen = Vec::new();
        for _ in 0..(walk_test_period(85.26) * 60.0) as usize {
            simulate(&mut wt, &mut lo, 1.0 / 60.0);
            if seen.last() != Some(&lo.blend.cur.mv) {
                seen.push(lo.blend.cur.mv);
            }
        }
        assert_eq!(seen, [Move::Forward, Move::Idle, Move::TurnLeft, Move::Forward, Move::Idle, Move::TurnRight, Move::Forward, Move::Idle, Move::Backward, Move::Idle]);
        // Walking at the clip's speed plays it at rate 1.
        let mut lo = Locomotion::new(set(), 0.0);
        let mut wt = WalkTest { base: Transform::IDENTITY, speed: 85.26, t: 0.0 };
        let tr = simulate(&mut wt, &mut lo, 1.0);
        assert_eq!(lo.blend.cur.mv, Move::Forward);
        assert!((lo.blend.cur.rate - 1.0).abs() < 1e-4);
        assert!((tr.translation.y - 85.26).abs() < 0.01);
        // Deterministic.
        let (mut lo2, mut wt2) = (Locomotion::new(set(), 0.0), WalkTest { base: Transform::IDENTITY, speed: 85.26, t: 0.0 });
        simulate(&mut wt2, &mut lo2, 1.0);
        assert_eq!(lo.blend, lo2.blend);
    }
}
