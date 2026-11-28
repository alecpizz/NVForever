//! The player at a slot machine in the world (where and when:
//! `nvf_play::slots::seat`): the body's slot machine idles and the win celebration.
//!
//! - **The idles** [observed: FalloutNV.esm IDLE records, findings §4]: `NVSlotMachine`
//!   (0x0015AB98) is a group with no file of its own under the stool-sitting tree; under it
//!   `NVSlotMachineInsertCoin` (0x0015AB99, `SpecialIdle_NVinsertCoin.kf`, 5.33 s) and
//!   `NVSlotMachineLoose` (0x0015CC26, `SpecialIdle_NVLoose.kf`, 3.33 s). So the body sits
//!   on the stool's own loop (the furniture's, `crate::furniture`) or stands, plays
//!   InsertCoin as each spin starts and Loose when a spin loses.
//! - **The win celebration** (the cheer, [`CHEER`]) plays for the length the spin's result
//!   gives (`fnv_game::slots::celebration`, 0.8 to 3 s) and is then faded out.
//! - **Upper body only** [design: "torso only (stays seated)", "legs planted"]: all
//!   three play on the upper body ([`upper_clip`]): the legs keep the stool's pose or the
//!   standing idle, so the body neither floats off the stool nor slides (the InsertCoin
//!   and Loose files are made for the no-lean stool and would set their own seat height
//!   on the others).
//! - **The hook** (`nvf_play::slots::SlotsSpin`): InsertCoin on `Started`, Loose on a
//!   losing `Result`, the cheer for a winning one's `celebrate` seconds.
//! - **Holding the seat**: while the menu is open the menu's keys (W spins, S bets the
//!   most) are not movement: they do not get the player up (`PlayerSeat::leave` cleared).
//! - **Getting up**: when the menu closes on the stool (`SeatPhase::Ending`) the player
//!   gets up by the getting-off clip a moment later ([`END_WAIT`]).
//!
//! Log lines start `slots seat:`.

use crate::idles::PlayerIdles;
use bevy::prelude::*;
use fnv_actor::idle_play::{IdleKeys, IdlePlay};
use fnv_game::idles::IdleSource;
use nvf_assets::actor::{ActorCtx, ActorParts};
use nvf_assets::anim_play::Clip;
use nvf_assets::model::ModelCtx;
use nvf_play::slots::seat::SeatPhase;
use nvf_play::slots::{SlotsScreen, SlotsSeat, SlotsSpin, SpinPhase};
use std::collections::HashMap;
use std::sync::Arc;

/// [observed: IDLE `NVSlotMachineInsertCoin` MODL].
pub const INSERT_COIN: &str = "meshes\\characters\\_male\\idleanims\\specialidle_nvinsertcoin.kf";
/// [observed: IDLE `NVSlotMachineLoose` MODL].
pub const LOOSE: &str = "meshes\\characters\\_male\\idleanims\\specialidle_nvloose.kf";
/// The win's cheer [observed: IDLE `LooseCheeringBothArmsUp` (0x00033CDC) MODL, 4.67 s].
pub const CHEER: &str = "meshes\\characters\\_male\\idleanims\\cheeringbotharmsup.kf";

/// What the body does at the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Act {
    InsertCoin,
    Loose,
    Cheer,
}

impl Act {
    pub fn path(self) -> &'static str {
        match self {
            Act::InsertCoin => INSERT_COIN,
            Act::Loose => LOOSE,
            Act::Cheer => CHEER,
        }
    }
}

/// This player's slot idle.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct SlotIdle {
    /// The act, the idle's serial on the third-person body.
    playing: Option<(Act, u32)>,
    /// The celebration is faded out then (real s).
    cheer_until: Option<f32>,
    /// The clips are loaded for the machine now played.
    loaded: bool,
}

/// The upper-body clips, by path.
#[derive(Resource, Default)]
pub struct SlotClips {
    clips: HashMap<&'static str, Option<Arc<Clip>>>,
}

/// `clip` with its lower body left out, its other tracks above the walk and the weapon's
/// stance (`npc_stimpaks::UPPER_PRIORITY`), and no root of its own.
pub fn upper_clip(clip: &Clip) -> Clip {
    let mut c = clip.clone();
    let keep: Vec<bool> = c.tracks.iter().map(|(n, _)| !crate::npc_stimpaks::lower_body(n)).collect();
    let mut k = keep.iter();
    c.tracks.retain(|_| *k.next().unwrap_or(&true));
    let mut k = keep.iter();
    c.priorities.retain(|_| *k.next().unwrap_or(&true));
    for p in c.priorities.iter_mut() {
        *p = (*p).max(crate::npc_stimpaks::UPPER_PRIORITY);
    }
    c.accum_root = None;
    c
}

impl SlotClips {
    fn get(&mut self, ctx: &mut ActorCtx, act: Act) -> Option<Arc<Clip>> {
        let path = act.path();
        self.clips
            .entry(path)
            .or_insert_with(|| {
                let c = ctx.clip(path).map(|c| Arc::new(upper_clip(&c)));
                match &c {
                    Some(c) => info!("slots seat: {path}: {:.2}s, {} upper-body tracks", c.stop - c.start, c.tracks.len()),
                    None => warn!("slots seat: {path} does not load; that idle is left out"),
                }
                c
            })
            .clone()
    }
}

pub struct SlotsSeatPlugin;

impl Plugin for SlotsSeatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SlotIdle>()
            .init_resource::<SlotClips>()
            .add_systems(Update, (get_up, hold).chain().in_set(nvf_play::player::BodySet).before(crate::furniture::drive_player))
            .add_systems(Update, own.after(nvf_play::PlaySet::Ui));
    }
}

/// The menu's keys are not movement while it is open, nor while the player waits on the
/// stool after it.
fn hold(screen: Res<SlotsScreen>, slots_seat: Res<SlotsSeat>, mut seat: ResMut<nvf_play::furniture::PlayerSeat>) {
    let at = screen.0.is_some() || matches!(slots_seat.phase, SeatPhase::Sitting | SeatPhase::Ending);
    if at && seat.leave && slots_seat.phase != SeatPhase::Leaving {
        seat.leave = false;
    }
}

/// How long the player stays on the stool after the menu closed, real seconds [design: a
/// moment, then the getting-off clip].
const END_WAIT: f32 = 0.5;

/// The menu closed on the stool (`SeatPhase::Ending`): after [`END_WAIT`] the player gets
/// up by the getting-off clip.
fn get_up(real: Res<Time<Real>>, time: nvf_play::pause::AidTime, mut slots_seat: ResMut<SlotsSeat>, mut seat: ResMut<nvf_play::furniture::PlayerSeat>, mut ending: Local<Option<f32>>) {
    if slots_seat.phase != SeatPhase::Ending {
        *ending = None;
        return;
    }
    let now = real.elapsed_secs();
    let since = *ending.get_or_insert(now);
    if now - since < END_WAIT {
        return;
    }
    let stool = match slots_seat.stance {
        Some(nvf_play::slots::Stance::Seated { stool }) => stool,
        _ => 0,
    };
    info!("slots seat: the player gets up from stool {stool:08X}");
    seat.leave = true;
    *slots_seat = SlotsSeat { phase: SeatPhase::Leaving, since: time.elapsed_secs(), ..*slots_seat };
    *ending = None;
}

/// This player's body: the idle for each event.
#[allow(clippy::too_many_arguments)]
fn own(
    real: Res<Time<Real>>,
    mut spins: MessageReader<SlotsSpin>,
    slots_seat: Res<SlotsSeat>,
    mut idle: ResMut<SlotIdle>,
    mut clips: ResMut<SlotClips>,
    pi: Option<ResMut<PlayerIdles>>,
    (mut parts, mut models): (ResMut<ActorParts>, ModelCtx),
) {
    let now = real.elapsed_secs();
    let Some(mut pi) = pi else { return };
    // The slot machine menu holds the game (`idles::drive`), and with it the idles: the
    // cheer stood on its first frame, arms down [observed: slots-170-B camera-win-warm,
    // 43.25 to 46.2 s]. At the machine the body is seen, so its idles play on.
    let under = slots_seat.busy();
    if pi.under_menu != under {
        pi.under_menu = under;
    }
    let mut ctx = ActorCtx { cache: &mut parts, models: &mut models };
    // At a machine: load them now, so the first spin does not wait for the files.
    if slots_seat.busy() && !idle.loaded {
        for a in [Act::InsertCoin, Act::Loose, Act::Cheer] {
            clips.get(&mut ctx, a);
        }
        idle.loaded = true;
    }
    idle.loaded &= slots_seat.busy();
    let fade = |idle: &mut SlotIdle, pi: &mut PlayerIdles| {
        idle.cheer_until = None;
        if let Some((a, serial)) = idle.playing.take() {
            if let Some(c) = pi.third.cur.as_mut().filter(|c| c.serial == serial && !c.play.ended()) {
                c.play.stop();
                info!("slots seat: {a:?} faded out");
            }
        }
    };
    if idle.cheer_until.is_some_and(|t| now >= t) {
        fade(&mut idle, &mut pi);
    }
    for ev in spins.read() {
        let (act, passes, cheer) = match ev.phase {
            SpinPhase::Started { .. } => (Act::InsertCoin, 1, None),
            SpinPhase::Result { multiple: 0, .. } => (Act::Loose, 1, None),
            // Two passes at most; it is cut at its length.
            SpinPhase::Result { celebrate, .. } if celebrate > 0.0 => (Act::Cheer, 2, Some(now + celebrate)),
            SpinPhase::Result { .. } => continue,
            SpinPhase::Opened => continue,
            SpinPhase::Closed => {
                fade(&mut idle, &mut pi);
                continue;
            }
        };
        let Some(clip) = clips.get(&mut ctx, act) else { continue };
        let play = IdlePlay::new(IdleKeys::read(clip.start, clip.stop, &clip.text_keys), Some(passes));
        let serial = pi.third.start_clip(IdleSource::Script, clip, play);
        idle.playing = Some((act, serial));
        idle.cheer_until = cheer;
        info!("slots seat: the player plays {act:?} ({})", act.path());
    }
    // The act's idle ended by itself.
    if let Some((_, serial)) = idle.playing {
        if !pi.third.cur.as_ref().is_some_and(|c| c.serial == serial && !c.play.ended()) {
            idle.playing = None;
        }
    }
}
