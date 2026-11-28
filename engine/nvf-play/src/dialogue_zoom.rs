//! The view narrows onto the speaker in a conversation (docs/design/dialogue-play.md
//! §8.8): the same conversations `crate::dialogue_look` turns the view for
//! (`Game::dialogue`). In the original the speaker's head comes out the same size from any
//! distance, about [`HEAD_SHARE`] of the view's height with the face's centre about
//! [`FACE_FROM_TOP`] of it from the top, so it is a framing and not a fixed zoom [observed:
//! the original, 2026-11-06, Doc Mitchell from about 90 and about 300 units].
//!
//! - **Field of view.** Worked out from the distance to the face and an assumed head
//!   ([`framed_vfov`]); never wider than the view the player has, never narrower than
//!   [`MIN_VFOV`]. It follows the speaker's distance slowly ([`FOLLOW_SECONDS`]), not the
//!   mouse [design: `DialogueZoom::step`].
//! - **Creatures and robots.** They are framed by how tall they are drawn, not by a human's
//!   head ([`creature_framing`]): the view grows with it, and the point lands lower so most
//!   of it shows, as with the original's Marcus, Victor and Mr. House [observed, 2026-11-06].
//!   Human-style actors (ACHR) keep the numbers above whatever their hit volume
//!   says: a hat or a seat changes that, and not their framing.
//! - **Framing.** The opening turn (`crate::dialogue_look`) aims [`face_drop`] under the
//!   face, so the face lands at [`FACE_FROM_TOP`], and the zoom locks the view: it follows
//!   the face for the whole conversation and the mouse moves only the cursor [owner
//!   decision 2026-11-13, #2254: "want it split so free look camera does not zoom and when
//!   zoom lock the camera"; `crate::dialogue_look::view_locked`, `Ease::follow`].
//! - **Looking around as the zoom eases out** is scaled with the zoom
//!   ([`DialogueZoom::look_scale`]), so the mouse moves the picture as far as it does
//!   unzoomed.
//! - **Who owns the field of view** (as `chargen/vigor.rs`'s `fov_before`). [`DialogueZoom`]
//!   keeps what was on the camera underneath its own value and gives it back. A value on
//!   the camera that is not the one it last wrote is someone else's (the settings slider,
//!   the console's SetCameraFOV, the sights' zoom): the new base, zoomed from at once. Who
//!   else reads a base asks [`DialogueZoom::underlying`] first, so a zoomed value never
//!   becomes one (`crate::aim_zoom`, `crate::free_cam`, `crate::chargen::vigor`).
//! - **Third person** ends in the same close-up: the camera travels to the first-person eye
//!   with the zoom and the body goes on the way ([`crate::control::TALK_CAMERA_CLOSE`]),
//!   and comes back when the conversation ends [observed: the original, 2026-11-06, the
//!   same framing and no body]. `PlayerControl::pov` is not touched: the player's own view
//!   is still what scripts and saves see.
//! - **No zoom**: through a talking activator (not in this version), a dead speaker, one
//!   that is not drawn, the free camera, the character creator, a kill camera; the Vigor
//!   Tester has the field of view for itself and gets it at once. A conversation that ends
//!   or stops being zoomable eases out.
//! - **Pausing**: the eases run on the virtual clock, so the Esc pause menu holds them.
//!   With `--menu-pause original` the picture stands still, so they complete at once.
//!
//! - **The classic VATS style** uses the same zoom for its selected target
//!   (`crate::vats_mode`, docs/design/vats-classic.md §5): the whole target framed
//!   ([`vats_framing`]), eased in real time ([`VATS_IN_SECONDS`]) since the world's clock
//!   may stand still, with the same ownership of the field of view. A conversation ends
//!   VATS, so the two never want the camera together.
//!
//! The depth of field that goes with it is nvforever's `imagespace.rs` ([`DialogueZoom::blend`],
//! [`DialogueZoom::face`]).
//!
//! Log, starting `dialogue zoom:`: one line as each ease ends, with the distance and the
//! angle reached; one line per conversation that gets no zoom, and why.

use crate::combat::is_dead;
use crate::control::PlayerControl;
use crate::dialogue_look::{smooth, FaceFrom, SpeakerFace};
use crate::free_cam::FreeCam;
use crate::pause::{MenuPause, Paused};
use crate::phys::PhysState;
use crate::player::{Player, MAX_PITCH};
use crate::script::ScriptState;
use crate::sim::SimState;
use crate::PlayerCamera;
use bevy::prelude::*;
use fnv_game::game::menus;
use fnv_sim::FormId;

/// The speaker's head, crown to chin, in game units [observed: our run of `dialogue-zoom`,
/// 2026-11-06: Chet's head as drawn came out about 185 of 720 px from 394 units at 8.7°, in
/// a view 59.9 units tall there, so about 15.4; one head measured by eye]. Creatures and
/// robots get the same number: the rigs have bones, not the head mesh's size, so a head
/// measured as drawn would need the skinned shape's bounds.
pub const HEAD_HEIGHT: f32 = 15.5;
/// The head's share of the view's height [observed: about 215 of 720 px in the original,
/// near and far].
pub const HEAD_SHARE: f32 = 0.30;
/// Where the point the view aims at (`SpeakerFace`, the head bone as drawn) lands, as a share
/// of the view's height from the top. The original has the face's centre at about 0.32
/// [observed: captures of 2026-11-06]; the bone lies about 0.06 of the view under that centre
/// at this framing [observed: Chet at 0.32 showed the face's centre at 0.26], hence 0.38.
pub const FACE_FROM_TOP: f32 = 0.38;
/// How tall the view is where a human-style speaker stands, game units: the head at its
/// share.
pub const HUMAN_VIEW_HEIGHT: f32 = HEAD_HEIGHT / HEAD_SHARE;
/// A creature this tall or less is framed in [`HUMAN_VIEW_HEIGHT`], game units [design: about
/// a human's drawn height, `crate::hitvol`].
pub const HUMAN_HEIGHT: f32 = 128.0;
/// A taller creature's view grows by this times the square root of what it has over
/// [`HUMAN_HEIGHT`]. Fitted to the original's three big speakers, their views measured
/// against their meshes: Marcus's about 97 units (goggle lenses 48 px apart, 6.5 units in the
/// mesh), his meshes 173 tall in the bind pose; Victor's about 125 (screen 210 px, 35.7 units;
/// shoulders 785 px, 136 units), his 187; Mr. House's about 150 (magnified 3.58 from the
/// default view by template match, so vertical 18.3° from about 466 units), his 300 [observed:
/// shots of 2026-11-06, meshes in the data]. It gives 102 (+5 %), 109 (-13 %) and 150. A fit
/// over three points, not the original's rule. Neither a proportion to height nor a power of
/// it fits: Marcus is 1.35 times a human with a view 1.9 times, House 2.3 times with 2.9.
pub const SIZE_GROWTH: f32 = 7.5;
/// Where a creature's point lands, as [`FACE_FROM_TOP`] does for a human: its head bone lies
/// nearer the middle of its head than a human's, and Marcus's lies about 0.31 from the top
/// [observed: his head 145-323 px, the bone about 0.41 down the head in the mesh].
pub const CREATURE_FROM_TOP: f32 = 0.31;
/// A creature whose top stands far above its point keeps this share of that in the view,
/// which puts the point lower: so most of it shows [fitted: gives Victor's bone 0.46, 0.457
/// in the original, with his antenna (about 49 over the bone) in view].
pub const TOP_SHARE: f32 = 0.9;
/// A creature's point lands no lower than this, for one much taller than the view [observed:
/// about 0.55 for Mr. House's bone, worked out from his eyes in the shots].
pub const CREATURE_FROM_TOP_MAX: f32 = 0.55;
/// Seconds of the ease in [observed: fitted, rough; settled in about 0.6-1 s in the
/// original at samples every 4 frames].
pub const IN_SECONDS: f32 = 0.8;
/// Seconds of the ease out [observed: fitted, rough; about 0.3 s, 4-6 samples].
pub const OUT_SECONDS: f32 = 0.3;
/// The narrowest vertical field of view, radians, for a speaker far away (6°: a head at
/// the share above from about 490 units; the original's far spot was about 300) [design].
pub const MIN_VFOV: f32 = 6.0 * std::f32::consts::PI / 180.0;
/// How slowly the zoom follows the distance to the face (a speaker walking up), seconds;
/// a head's idle sway would otherwise breathe in the field of view [design].
pub const FOLLOW_SECONDS: f32 = 0.5;

/// The classic VATS style's selected target fills this share of the view's height
/// [design: the original's target view shows the whole target with room around it; no
/// page gives a number].
pub const VATS_TARGET_SHARE: f32 = 0.6;
/// Seconds of the ease onto a VATS target and of the ease back out after it, real time
/// [design: quicker than a conversation's, the player is choosing; the fork owner
/// 2026-11-10, "slow down the VATS zoom in/out animation by 50%, it is too fast now":
/// they were 0.25 and 0.3 ([`OUT_SECONDS`]); then "make the zoom in/out vats animation the
/// same length 0.5sec"].
pub const VATS_IN_SECONDS: f32 = 0.5;
pub const VATS_OUT_SECONDS: f32 = 0.5;

/// How a VATS target `height` tall as drawn is framed: all of it, at
/// [`VATS_TARGET_SHARE`] of the view, the point aimed at in the middle.
pub fn vats_framing(height: f32) -> Framing {
    Framing { view_height: height.max(1.0) / VATS_TARGET_SHARE, from_top: 0.5 }
}

/// Whether a conversation zooms (`nvforever --no-dialogue-zoom` switches it off).
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZoomSettings {
    pub enabled: bool,
}

impl Default for ZoomSettings {
    fn default() -> Self {
        ZoomSettings { enabled: true }
    }
}

/// How a speaker is framed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Framing {
    /// How tall the view is where the speaker stands, game units.
    pub view_height: f32,
    /// Where the point aimed at lands, as a share of the view's height from the top.
    pub from_top: f32,
}

/// A human-style speaker.
pub const HUMAN_FRAMING: Framing = Framing { view_height: HUMAN_VIEW_HEIGHT, from_top: FACE_FROM_TOP };

/// A creature or a robot `height` tall as drawn, whose top is `top_above` over the point
/// aimed at (its head bone, or a point of its hit volume without one). No smaller than a
/// human's view, however small it is.
pub fn creature_framing(height: f32, top_above: f32) -> Framing {
    let view_height = HUMAN_VIEW_HEIGHT + SIZE_GROWTH * (height - HUMAN_HEIGHT).max(0.0).sqrt();
    let from_top = (TOP_SHARE * top_above / view_height).clamp(CREATURE_FROM_TOP, CREATURE_FROM_TOP_MAX);
    Framing { view_height, from_top }
}

/// The vertical field of view (radians) in which the view is `view_height` tall at
/// `distance`, no narrower than [`MIN_VFOV`]. The view is `2 d tan(v/2)` tall at distance
/// `d`, so the aspect ratio does not enter.
pub fn framed_vfov(distance: f32, view_height: f32) -> f32 {
    (2.0 * (view_height / (2.0 * distance.max(1.0))).atan()).max(MIN_VFOV)
}

/// How far under the point the view looks (radians) for it to sit at `from_top` in a view
/// `vfov` tall.
pub fn face_drop(vfov: f32, from_top: f32) -> f32 {
    (2.0 * (0.5 - from_top) * (vfov * 0.5).tan()).atan()
}

/// What [`DialogueZoom::apply`] asks of the camera this frame.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Applied {
    /// The field of view to write.
    pub write: Option<f32>,
    /// The ease out is over: the field of view underneath is back (or still) on the camera.
    pub back: Option<f32>,
}

/// The zoom of the conversation now, or easing out of the last one.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct DialogueZoom {
    /// 0 (the player's view) to 1 (framed), before [`smooth`].
    weight: f32,
    /// The field of view wanted this frame; none when the zoom is not wanted.
    want: Option<f32>,
    /// Where the point aimed at lands ([`Framing::from_top`]).
    from_top: f32,
    /// The field of view last wanted (held while easing out).
    target: Option<f32>,
    /// What is on the camera underneath the zoom.
    base: Option<f32>,
    /// What this module last wrote.
    applied: Option<f32>,
    /// The distance to the face, followed slowly ([`FOLLOW_SECONDS`]).
    distance: f32,
    /// The face, game space (held while easing out).
    face: Option<[f32; 3]>,
    /// The line for the end of the ease in has been said.
    told_in: bool,
    /// The conversation told it gets no zoom.
    told_none: Option<FormId>,
    /// The zoom is (or was last) a VATS target's: eased in real time.
    vats: bool,
}

impl DialogueZoom {
    #[cfg(test)]
    pub(crate) fn for_test_conversation() -> Self {
        let mut zoom = Self::default();
        zoom.step(Some(0.2), IN_SECONDS, false);
        zoom.apply(1.0);
        zoom
    }

    /// The field of view underneath `fov`, what is on the camera now: `fov` itself unless
    /// this module wrote it. Who keeps a base goes by this (the module notes).
    pub fn underlying(&self, fov: f32) -> f32 {
        match (self.applied, self.base) {
            (Some(a), Some(b)) if a == fov => b,
            _ => fov,
        }
    }

    /// How far the zoom has come, eased: 0 off, 1 framed.
    pub fn blend(&self) -> f32 {
        smooth(self.weight)
    }

    /// The speaker's face, game space, while there is a zoom.
    pub fn face(&self) -> Option<[f32; 3]> {
        self.face.filter(|_| self.weight > 0.0)
    }

    /// How far under the face the opening turn should aim (0 without a zoom).
    pub fn face_drop(&self) -> f32 {
        self.want.map_or(0.0, |v| face_drop(v, self.from_top))
    }

    /// Whether a zoom is wanted this frame.
    pub fn wanted(&self) -> bool {
        self.want.is_some()
    }

    /// What looking around is multiplied by: the share of the unzoomed view's height the
    /// zoomed view has, 1 without a zoom [design: the sights' zoom has no such scale to
    /// follow].
    pub fn look_scale(&self) -> f32 {
        match (self.applied, self.base) {
            (Some(a), Some(b)) if b > 0.0 => ((a * 0.5).tan() / (b * 0.5).tan()).clamp(0.0, 1.0),
            _ => 1.0,
        }
    }

    /// Another owner has the camera now (the Vigor Tester): off at once, and what this
    /// module last wrote, if still there, is taken back. Whatever else is there is left.
    fn let_go(&mut self, seen: f32) -> Applied {
        let write = self.base.filter(|b| self.applied == Some(seen) && *b != seen);
        (self.weight, self.want, self.applied, self.told_in) = (0.0, None, None, false);
        Applied { write, back: None }
    }

    /// [`Self::step`], for other modules' tests.
    #[cfg(test)]
    pub(crate) fn test_step(&mut self, want: Option<f32>, dt: f32) {
        self.step(want, dt, false);
    }

    /// Advance the ease by `dt` towards `want` (none: out). `instant` completes it at once.
    fn step(&mut self, want: Option<f32>, dt: f32, instant: bool) {
        self.want = want;
        if want.is_some() {
            self.target = want;
        }
        self.weight = match (want.is_some(), instant) {
            (true, true) => 1.0,
            (false, true) => 0.0,
            (true, false) => (self.weight + dt.max(0.0) / IN_SECONDS).min(1.0),
            (false, false) => (self.weight - dt.max(0.0) / OUT_SECONDS).max(0.0),
        };
    }

    /// What to do with the camera, whose field of view is `seen` now (the module notes).
    fn apply(&mut self, seen: f32) -> Applied {
        // Not what this module wrote: someone else's, the new base.
        if self.applied != Some(seen) {
            self.base = Some(seen);
        }
        let base = self.base.unwrap_or(seen);
        let s = self.blend();
        if s <= 0.0 {
            let was = self.applied.take().is_some();
            return Applied { write: (was && base != seen).then_some(base), back: was.then_some(base) };
        }
        // A very near speaker does not widen the view.
        let target = self.target.unwrap_or(base).min(base);
        let fov = base + (target - base) * s;
        self.applied = Some(fov);
        Applied { write: (fov != seen).then_some(fov), back: None }
    }
}

/// Where the speaker's face is, as `crate::dialogue_look::face_speaker` finds it.
fn speaker_face(actor: FormId, drawn: Option<(u32, [f32; 3])>, phys: &PhysState, sim: &SimState) -> Option<([f32; 3], FaceFrom)> {
    let volume = phys.world.actor_volume(actor.0).map(|v| (v.feet, v.height));
    let reference = sim.world.peek(actor).map(|s| s.pos).or_else(|| sim.refs.get(actor).map(|p| p.pos));
    crate::dialogue_look::face_point(actor.0, drawn, volume, reference)
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1]).hypot(a[2] - b[2])
}

/// What the zoom wants this frame, and the third-person camera's travel with it. Before
/// `crate::dialogue_look::face_speaker`, which aims under the face by the result.
#[allow(clippy::too_many_arguments)]
pub(crate) fn track(
    (time, real): (Res<Time>, Res<Time<Real>>),
    (settings, vats): (Res<ZoomSettings>, Option<Res<crate::vats_mode::VatsMode>>),
    (mode, paused): (Res<MenuPause>, Res<Paused>),
    script: Option<Res<ScriptState>>,
    face: Res<SpeakerFace>,
    (phys, sim): (Res<PhysState>, Res<SimState>),
    free: Option<Res<FreeCam>>,
    player: Option<ResMut<Player>>,
    mut control: ResMut<PlayerControl>,
    cams: Query<&Projection, With<PlayerCamera>>,
    mut zoom: ResMut<DialogueZoom>,
) {
    // The pause menu holds everything; the settings slider still reaches [`apply`].
    if paused.0 {
        return;
    }
    let Some(mut player) = player else { return };
    let Ok(Projection::Perspective(p)) = cams.single() else { return };
    let base = zoom.underlying(p.fov);
    let game = script.as_ref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game);
    let conv = game.and_then(|g| g.dialogue.as_ref());
    // The Vigor Tester has the field of view for itself (`crate::chargen::vigor`).
    let vigor = game.is_some_and(|g| g.menu == Some(menus::LOVE_TESTER));
    // Something else has the camera.
    let taken = vigor || free.is_some_and(|f| f.on) || control.portrait.is_some() || control.cinematic;
    let found = match conv.filter(|_| settings.enabled && !taken) {
        None => Err(None),
        Some(c) if c.through.is_some() => Err(Some("it goes through a talking activator")),
        Some(c) if is_dead(script.as_deref(), c.actor.0) => Err(Some("the speaker is dead")),
        Some(c) => match speaker_face(c.actor, face.0, &phys, &sim) {
            Some((_, FaceFrom::Reference)) | None => Err(Some("the speaker is not drawn")),
            Some(found) => Ok((c.actor, found)),
        },
    };
    // The classic VATS style's selected target, framed whole (the module notes).
    let vats_target = vats.as_deref().and_then(|v| v.selected()).filter(|_| conv.is_none() && !taken);
    let found = match vats_target.and_then(|a| phys.world.actor_aim_point(a).map(|p| (FormId(a), (p, FaceFrom::Volume)))) {
        Some(t) => Ok(t),
        None => found,
    };
    if conv.is_none() {
        zoom.told_none = None;
    }
    if let (Err(Some(why)), Some(c), Some(g)) = (&found, conv, game) {
        if zoom.told_none != Some(c.actor) {
            zoom.told_none = Some(c.actor);
            info!("dialogue zoom: no zoom in the conversation with {}: {why}", g.data.name(c.actor));
        }
    }
    let instant = vigor || (*mode == MenuPause::Original && vats_target.is_none());
    // A VATS target's ease is in real time and at its own pace: the world's clock may
    // stand still. The ease out after it is real time too, until it is over.
    let vats_ease = vats_target.is_some() || (zoom.vats && conv.is_none());
    zoom.vats = vats_ease && (vats_target.is_some() || zoom.weight > 0.0);
    let dt = if vats_ease { real.delta_secs().min(0.25) * if vats_target.is_some() { IN_SECONDS / VATS_IN_SECONDS } else { OUT_SECONDS / VATS_OUT_SECONDS } } else { time.delta_secs() };
    let mut said = None;
    let want = found.ok().map(|(actor, (point, from))| {
        let d = distance(player.eye(), point);
        // From the start of a zoom the distance is taken as it is, then followed.
        zoom.distance = if zoom.want.is_none() || instant { d } else { zoom.distance + (d - zoom.distance) * (1.0 - (-dt.max(0.0) / FOLLOW_SECONDS).exp()) };
        zoom.face = Some(point);
        // Human-style actors keep their numbers (the module notes); a reference the placed
        // list does not know was made by a script: as a human without a hit volume.
        let creature = sim.refs.get(actor).is_some_and(|p| &p.sig == b"ACRE");
        let drawn = phys.world.actor_volume(actor.0).filter(|_| creature || vats_target.is_some());
        let framing = match (vats_target, drawn) {
            (Some(_), v) => vats_framing(v.map_or(HUMAN_HEIGHT, |v| v.height)),
            (None, v) => v.map_or(HUMAN_FRAMING, |v| creature_framing(v.height, v.feet[2] + v.height - point[2])),
        };
        zoom.from_top = framing.from_top;
        said = Some((actor, from, framing, drawn.map(|v| v.height)));
        framed_vfov(zoom.distance, framing.view_height).min(base)
    });
    // The detached camera looks off the player's heading: the close-up looks along it, as
    // `crate::control::detach_camera` does when it stops (it runs only while playing). Here,
    // before the opening turn, so the turn starts from the view the player sees.
    if want.is_some() && zoom.weight == 0.0 && control.orbit.free && (control.orbit.yaw != 0.0 || control.orbit.pitch != 0.0) {
        let o = &mut control.orbit;
        player.heading = (player.heading + o.yaw).rem_euclid(std::f32::consts::TAU);
        player.pitch = (player.pitch + o.pitch).clamp(-MAX_PITCH, MAX_PITCH);
        (o.yaw, o.pitch) = (0.0, 0.0);
    }
    zoom.step(want, dt, instant);
    if let (Some(fov), Some((actor, from, framing, tall)), Some(g), true, false) = (want, said, game, zoom.weight >= 1.0, zoom.told_in) {
        zoom.told_in = true;
        let source = match from {
            FaceFrom::Head => "its head as drawn",
            FaceFrom::Volume => "the top of its hit volume: it has no drawn head",
            FaceFrom::Reference | FaceFrom::Activator => "?",
        };
        let close = if control.own_third_person() { "; third person: the camera at the eye, no body" } else { "" };
        let close = if vats_target.is_some() { format!("{close}; a VATS target, framed whole") } else { close.to_string() };
        let size = tall.map_or_else(|| "a human-style actor".to_string(), |h| format!("drawn {h:.0} tall"));
        info!(
            "dialogue zoom: in on {} ({source}): {:.0} units, vertical {:.1}° to {:.1}°, over {IN_SECONDS:.2} s; view {:.0} units tall ({size}), the point {:.2} from its top{close}",
            g.data.name(actor),
            zoom.distance,
            base.to_degrees(),
            fov.to_degrees(),
            framing.view_height,
            framing.from_top
        );
    }
    // The third-person camera travels with the zoom.
    let camera = if control.own_third_person() { zoom.blend() } else { 0.0 };
    if control.talk_camera != camera {
        control.talk_camera = camera;
    }
}

/// Put the zoom on the camera. After `crate::aim_zoom::zoom`, the other writer of a frame,
/// which goes by [`DialogueZoom::underlying`]; a write of its own here is read as the base.
pub(crate) fn apply(script: Option<Res<ScriptState>>, mut zoom: ResMut<DialogueZoom>, mut cams: Query<&mut Projection, With<PlayerCamera>>) {
    let Ok(mut proj) = cams.single_mut() else { return };
    // Read first: a write every frame would mark the projection changed.
    let Projection::Perspective(p) = &*proj else { return };
    // The Vigor Tester may have written its own this frame, before [`track`] saw it open.
    let vigor = script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.menu == Some(menus::LOVE_TESTER));
    let done = if vigor { zoom.let_go(p.fov) } else { zoom.apply(p.fov) };
    if let Some(fov) = done.write {
        if let Projection::Perspective(p) = &mut *proj {
            p.fov = fov;
        }
    }
    if let Some(back) = done.back {
        zoom.told_in = false;
        info!("dialogue zoom: out from {:.0} units: the view is back at vertical {:.1}°, over {OUT_SECONDS:.2} s", zoom.distance, back.to_degrees());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The share of the view's height a head at `distance` fills in a view `vfov` tall.
    fn share(distance: f32, vfov: f32) -> f32 {
        HEAD_HEIGHT / (2.0 * distance * (vfov * 0.5).tan())
    }

    const BASE: f32 = 1.044; // About 59.8°: `vertical_fov(75)`.

    /// One frame of the camera as `apply` leaves it.
    fn frame(z: &mut DialogueZoom, fov: &mut f32) -> Applied {
        let a = z.apply(*fov);
        if let Some(w) = a.write {
            *fov = w;
        }
        a
    }

    #[test]
    fn the_head_comes_out_the_same_size_from_any_distance() {
        for d in [60.0, 90.0, 150.0, 300.0] {
            assert!((share(d, framed_vfov(d, HUMAN_VIEW_HEIGHT)) - HEAD_SHARE).abs() < 1e-4, "at {d}");
        }
    }

    #[test]
    fn a_human_is_framed_as_before_whatever_its_size() {
        // What the numbers came to before creatures had a rule of their own.
        for d in [60.0f32, 90.0, 300.0, 394.0] {
            let before = (2.0 * (HEAD_HEIGHT / (2.0 * HEAD_SHARE * d)).atan()).max(MIN_VFOV);
            assert!((framed_vfov(d, HUMAN_FRAMING.view_height) - before).abs() < 1e-5, "at {d}");
        }
        assert_eq!(HUMAN_FRAMING.from_top, FACE_FROM_TOP);
        // A creature of a human's height sees the same view, and a small one no less.
        for height in [HUMAN_HEIGHT, 90.0, 30.0] {
            assert_eq!(creature_framing(height, 10.0).view_height, HUMAN_VIEW_HEIGHT);
        }
    }

    #[test]
    fn big_speakers_are_framed_as_the_original_frames_them() {
        // Marcus: the view about 97 units tall, his head (about 24) about 0.25 of it, the
        // bone where the original has it.
        let marcus = creature_framing(173.0, 20.0);
        assert!((marcus.view_height - 97.0).abs() < 8.0, "{marcus:?}");
        assert!((24.0 / marcus.view_height - 0.25).abs() < 0.02, "{marcus:?}");
        assert_eq!(marcus.from_top, CREATURE_FROM_TOP);
        // Victor: the rule's largest miss, -13 % on a view about 125 units tall, his screen
        // (26 units) 0.24 of it against 0.22. His meshes reach 56 over the bone, his antenna 49.
        let victor = creature_framing(187.0, 56.0);
        assert!((victor.view_height - 125.0).abs() < 17.0, "{victor:?}");
        assert!((26.0 / victor.view_height - 0.22).abs() < 0.02, "{victor:?}");
        assert!(victor.from_top > 49.0 / victor.view_height, "the antenna is in the view: {victor:?}");
        assert!((victor.from_top - 0.457).abs() < 0.02, "the original's bone: {victor:?}");
        // The clamps hold at his size too, from far off (the capture's was about 1000 units).
        assert!(framed_vfov(1000.0, victor.view_height) > MIN_VFOV);
        // Mr. House, 300 tall, the bone 139 under his top: vertical 18.3° from about 466 units,
        // no exception for him.
        let house = creature_framing(300.0, 139.0);
        assert!((house.view_height - 150.0).abs() < 5.0, "{house:?}");
        assert!((framed_vfov(466.0, house.view_height).to_degrees() - 18.3).abs() < 1.0, "{house:?}");
        assert_eq!(house.from_top, CREATURE_FROM_TOP_MAX);
    }

    #[test]
    fn a_very_tall_point_stays_clear_of_the_subtitle() {
        assert_eq!(creature_framing(190.0, 400.0).from_top, CREATURE_FROM_TOP_MAX);
    }

    #[test]
    fn the_point_lands_where_the_framing_says() {
        let z = DialogueZoom { from_top: 0.45, want: Some(0.3), ..Default::default() };
        assert_eq!(z.face_drop(), face_drop(0.3, 0.45));
        let y = 0.5 - 0.5 * z.face_drop().tan() / 0.15f32.tan();
        assert!((y - 0.45).abs() < 1e-5, "{y}");
    }

    #[test]
    fn a_far_speaker_does_not_zoom_absurdly() {
        // The original's far spot, about 300 units, is still framed; beyond the limit the
        // head stays small.
        assert!(framed_vfov(300.0, HUMAN_VIEW_HEIGHT) > MIN_VFOV);
        assert_eq!(framed_vfov(100_000.0, HUMAN_VIEW_HEIGHT), MIN_VFOV);
    }

    #[test]
    fn a_very_near_speaker_does_not_widen_the_view() {
        assert!(framed_vfov(20.0, HUMAN_VIEW_HEIGHT) > BASE, "the framing alone would widen it");
        let mut z = DialogueZoom::default();
        let mut fov = BASE;
        z.step(Some(framed_vfov(20.0, HUMAN_VIEW_HEIGHT).min(BASE)), 1.0, false);
        frame(&mut z, &mut fov);
        assert_eq!(fov, BASE);
        // And even if the target were wider, the base is the limit.
        z.target = Some(BASE * 1.5);
        frame(&mut z, &mut fov);
        assert_eq!(fov, BASE);
    }

    #[test]
    fn the_face_lands_at_its_place_in_the_view() {
        for v in [8f32, 30.0, 59.8] {
            let v = v.to_radians();
            // Seen `drop` above the view's axis, on a screen of half-height tan(v/2).
            let y = 0.5 - 0.5 * face_drop(v, FACE_FROM_TOP).tan() / (v * 0.5).tan();
            assert!((y - FACE_FROM_TOP).abs() < 1e-5, "{y} at {v}");
        }
        // No zoom, no drop: the opening turn aims at the face itself.
        assert_eq!(DialogueZoom::default().face_drop(), 0.0);
    }

    #[test]
    fn the_ease_takes_its_times_and_gives_the_view_back_exactly() {
        let mut z = DialogueZoom::default();
        let mut fov = BASE;
        // Nothing wanted: the camera is not touched.
        z.step(None, 0.1, false);
        assert_eq!(frame(&mut z, &mut fov), Applied::default());
        let target = 0.6;
        z.step(Some(target), IN_SECONDS * 0.5, false);
        frame(&mut z, &mut fov);
        assert!((fov - (BASE + target) / 2.0).abs() < 1e-5, "halfway at half the time: {fov}");
        z.step(Some(target), IN_SECONDS, false);
        frame(&mut z, &mut fov);
        assert!((fov - target).abs() < 1e-6);
        // Settled: no more writes.
        z.step(Some(target), 0.1, false);
        assert_eq!(frame(&mut z, &mut fov).write, None);
        z.step(None, OUT_SECONDS * 0.5, false);
        frame(&mut z, &mut fov);
        assert!(fov > target && fov < BASE);
        z.step(None, OUT_SECONDS, false);
        let a = frame(&mut z, &mut fov);
        assert_eq!((fov, a.back), (BASE, Some(BASE)));
        // Over: not said again, not written again.
        z.step(None, 0.1, false);
        assert_eq!(frame(&mut z, &mut fov), Applied::default());
    }

    #[test]
    fn a_frozen_picture_gets_the_end_of_the_ease_at_once() {
        let mut z = DialogueZoom::default();
        let mut fov = BASE;
        z.step(Some(0.5), 0.0, true);
        frame(&mut z, &mut fov);
        assert!((fov - 0.5).abs() < 1e-6);
        z.step(None, 0.0, true);
        let a = frame(&mut z, &mut fov);
        assert_eq!((fov, a.back), (BASE, Some(BASE)));
        // The Esc pause menu: no time, not instant: held where it is.
        let mut z = DialogueZoom::default();
        let mut fov = BASE;
        z.step(Some(0.5), IN_SECONDS / 2.0, false);
        frame(&mut z, &mut fov);
        let held = fov;
        z.step(Some(0.5), 0.0, false);
        frame(&mut z, &mut fov);
        assert_eq!(fov, held);
    }

    #[test]
    fn a_zoomed_value_is_never_a_base() {
        let mut z = DialogueZoom::default();
        let mut fov = BASE;
        z.step(Some(0.5), IN_SECONDS * 0.5, false);
        frame(&mut z, &mut fov);
        assert_ne!(fov, BASE);
        // The sights' zoom, the console and the Vigor Tester see what is underneath.
        assert_eq!(z.underlying(fov), BASE);
        // A value this module did not write is not a zoomed one.
        assert_eq!(z.underlying(0.77), 0.77);
        // With nothing applied everything is itself.
        assert_eq!(DialogueZoom::default().underlying(fov), fov);
    }

    #[test]
    fn someone_elses_value_is_the_new_base() {
        // The settings slider (or the console) during the zoom: the zoom keeps on from it
        // and gives it back.
        let mut z = DialogueZoom::default();
        let mut fov = BASE;
        z.step(Some(0.5), IN_SECONDS * 2.0, false);
        frame(&mut z, &mut fov);
        assert!((fov - 0.5).abs() < 1e-6);
        let slider = 1.2;
        fov = slider;
        frame(&mut z, &mut fov);
        assert!((fov - 0.5).abs() < 1e-6, "framed again from the new base");
        assert_eq!(z.underlying(fov), slider);
        z.step(None, OUT_SECONDS * 2.0, false);
        let a = frame(&mut z, &mut fov);
        assert_eq!((fov, a.back), (slider, Some(slider)));
    }

    #[test]
    fn the_sights_zoom_going_out_under_the_ease_ends_on_its_own_base() {
        // The sights' zoom writes its own fading value each frame, the zoom lays over it.
        let mut z = DialogueZoom::default();
        let mut fov = 0.5f32;
        z.step(Some(0.3), IN_SECONDS / 4.0, false);
        frame(&mut z, &mut fov);
        for sights in [0.7f32, 0.9, BASE] {
            z.step(Some(0.3), IN_SECONDS / 4.0, false);
            fov = sights;
            frame(&mut z, &mut fov);
            assert_eq!(z.underlying(fov), sights);
        }
        z.step(None, OUT_SECONDS * 2.0, false);
        frame(&mut z, &mut fov);
        assert_eq!(fov, BASE);
    }

    #[test]
    fn looking_around_is_scaled_with_the_zoom_and_only_then() {
        let mut z = DialogueZoom::default();
        let mut fov = BASE;
        z.step(None, 0.1, false);
        frame(&mut z, &mut fov);
        assert_eq!(z.look_scale(), 1.0);
        z.step(Some(0.15), IN_SECONDS * 2.0, false);
        frame(&mut z, &mut fov);
        let want = (0.15f32 * 0.5).tan() / (BASE * 0.5).tan();
        assert!((z.look_scale() - want).abs() < 1e-5, "{}", z.look_scale());
        z.step(None, OUT_SECONDS * 2.0, false);
        frame(&mut z, &mut fov);
        assert_eq!(z.look_scale(), 1.0, "restored with the field of view");
        // A speaker too near to narrow the view: no change.
        z.step(Some(BASE), IN_SECONDS * 2.0, false);
        frame(&mut z, &mut fov);
        assert_eq!(z.look_scale(), 1.0);
    }

    #[test]
    fn letting_go_for_the_vigor_tester() {
        // It wrote its own first: left alone, and not reported as the view coming back.
        let mut z = DialogueZoom::default();
        let mut fov = BASE;
        z.step(Some(0.5), IN_SECONDS / 2.0, false);
        frame(&mut z, &mut fov);
        assert_eq!(z.let_go(0.9), Applied::default());
        assert_eq!((z.blend(), z.underlying(0.9)), (0.0, 0.9));
        // It has not drawn yet: the zoomed value goes, so it finds what lay under it.
        let mut z = DialogueZoom::default();
        let mut fov = BASE;
        z.step(Some(0.5), IN_SECONDS / 2.0, false);
        frame(&mut z, &mut fov);
        assert_eq!(z.let_go(fov), Applied { write: Some(BASE), back: None });
    }

    #[test]
    fn another_owner_at_the_end_of_the_ease_keeps_the_camera() {
        // The Vigor Tester wrote its own value as the zoom lets go at once: left alone.
        let mut z = DialogueZoom::default();
        let mut fov = BASE;
        z.step(Some(0.5), IN_SECONDS * 2.0, false);
        frame(&mut z, &mut fov);
        let vigor = 0.9;
        fov = vigor;
        z.step(None, 0.0, true);
        let a = frame(&mut z, &mut fov);
        assert_eq!((fov, a.write), (vigor, None));
    }
}
