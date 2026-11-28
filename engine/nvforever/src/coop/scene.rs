//! The host's script scenes, shared with the guests [design].
//!
//! A scene on the host (a DLC's opening slideshow, Elijah's broadcast, the Sink's
//! arrival) holds the host's player with DisablePlayerControls, plays fades with
//! ApplyImageSpaceModifier and moves the host with MoveTo. A guest's game does not run
//! the host's trigger scripts (`nvf_play::script_trigger`, coop-world-sync's gate), so
//! without this it would stand free while the host is held, or (before the gate) run the
//! scene on its own and stay locked after it. The host sends what its scripts do to its
//! player ([`net::SceneView`]) and each guest shares it:
//!
//! - **Held as the host is.** The control groups the host's scripts hold
//!   (`Game::controls_locked`: DisablePlayerControls and the NVSE Alt locks; not the
//!   host's own menus or what it uses [design: only script scenes]) hold the guest too
//!   (`fnv_game::Game::coop_host_locks`), and let go when the host's do. A host that has
//!   not been heard for [`STALE`] seconds holds nothing: a guest is never left locked by a
//!   lost host.
//! - **The same fades.** While the host is held, the image space modifiers on its screen
//!   are put on the guest's (`Game::apply_image_modifier`), again when the host's starts
//!   again, and taken off when the host's goes. Outside a scene they are not sent: the
//!   host's drugs, hits and the like are its own.
//! - **Not its own opening's.** The modifiers a guest's own scripts put on its screen
//!   before it played as a guest are taken off once its game is kept as the guest's own
//!   [design]. A fresh guest's new game runs FalloutNV.esm's VCG00 stage 0, which puts
//!   WhiteISFX and MQ08AllBlackedOutISFX on the screen; VCG00 stage 100 takes the second
//!   off and sets VCG01 stage 0, which takes the first off [observed: the ESM's quest
//!   stage result scripts].
//!   A guest never reaches VCG00 stage 100 (the host's story goes on, not its own
//!   opening), so its screen stayed white. The host's modifiers it is sent at that moment
//!   stay on.
//! - **The same slides.** While the host is held, the textures its scripts swapped on
//!   references' shapes (`SwapTexture`: each slideshow line's begin script puts its slide
//!   on the screen) are swapped here too (`TextureSwaps::follow_host`): a guest runs none
//!   of the host's line scripts. They stay after the scene, as the host's do.
//! - **Beside the host.** A guest whose movement the host's scene has held for [`SETTLE`]
//!   seconds and that stands more than [`NEAR`] units from the host in the same place is
//!   brought beside it (the host's right, [`BESIDE`] units off), as the leash brings it
//!   to another place (`super::leash`); so every player watches the scene from the same
//!   spot.
//! - **Not inside the host.** A guest that stands in the host's body ([`in_body`]) for
//!   [`SETTLE`] seconds while no scene holds the view steps to its place beside the host,
//!   in place. Both new games start on the opening's one spot in Doc Mitchell's house,
//!   and a scene that held the view put the guest on the host's spot; the guest's camera
//!   then looked out of the host's head [observed: buckallobill, in game, FNV 1.4.0.525].
//!
//! - **On the slide marker.** While a slideshow has this guest's view (its own cell is a
//!   slideshow's room, `nvf_play::hud::slideshow_room`: the ending's `Endgame` or an
//!   add-on's intro room), it stands on the room's slide marker (its one XMarkerHeading,
//!   [`slide_marker`]) and looks along it, as the host's MoveTo put the host
//!   ([`slide_spot`]). The guest came in beside the host (the summon's [`beside_slot`])
//!   or wherever a console move put it, and the slide stood off to the left of its
//!   screen with black down the right [observed: buckallobill, in game, FNV 1.4.0.525]. Each game finds the marker in its
//!   own data, so nothing more is sent; the view is the player's again when a script
//!   moves it out of the room.
//!
//! - **Bodies hidden.** While a scene holds a player's movement and looking, an idle's
//!   camera node drives its view or a script drives its camera, the other players' bodies
//!   are not drawn on its screen ([`hide_players_in_scene`]).
//!
//! Not shared: what the host's scripts put on or take from the host's player (story gear
//! and checkpoints are the progress side's; [`SceneView::worn`] goes empty), and the
//! player packages its scripts give the host's
//! player (the guests' copies of the host are drawn where the host is, and a guest held
//! still needs no package).
//!
//! Log: `co-op: the host's scene holds this player (…)`, `co-op: the host's scene lets
//! this player go`, `co-op: the host's scene: image space modifier 0x… on this screen`,
//! `co-op: the host's scene: <texture> on 0x… (<shape>)`,
//! `co-op: scene: beside the host at …`, `co-op: scene: out of the host's body: …`,
//! `co-op: slideshow: this player watches from the slide marker 0x… …`,
//! `co-op: slideshow: over; the view is this player's again`.

use super::{place_of, Coop};
use bevy::prelude::*;
use fnv_sim::FormId;
use nvf_coop::net::{self, Place, SceneView};
use nvf_play::loading::{Loading, TravelRequest};
use nvf_play::player::Player;
use nvf_play::{Location, Travel};
use std::collections::HashMap;

/// A host's scene not heard for this long (seconds) holds nothing.
const STALE: f32 = 3.0;
/// A guest is brought beside the host once its movement has been held this long
/// (seconds): a lock of a moment does not move anyone.
const SETTLE: f32 = 1.0;
/// Farther than this from the host (game units) in the host's place, a held guest is
/// brought beside it.
const NEAR: f32 = 400.0;
/// A guest further than this (units) from the host's feet while a scene holds the whole
/// view is put on them [design].
const ON_SPOT: f32 = 8.0;
/// How far to the host's right a guest is put (game units).
const BESIDE: f32 = 72.0;
/// Two players' feet nearer than this across (game units) stand in one body: about two
/// bodies' radii [design].
const IN_BODY: f32 = 40.0;
/// ... and nearer than this up or down (a floor above or below is not one body).
const IN_BODY_UP: f32 = 100.0;
/// A modifier on the host's screen whose seconds went back by more than this started
/// again.
const RESTARTED: f32 = 0.1;
/// What co-op keeps of the host's scene ([`Coop::scene`]).
#[derive(Default)]
pub(super) struct Shared {
    /// A guest's: the host's scene as last heard, and when (seconds of `Time`).
    pub(super) heard: Option<(SceneView, f32)>,
    /// The control groups held here for the host, as last set.
    held: [bool; 7],
    /// Since when the host has held this player's movement, and the host's place and yaw
    /// this player was last turned to face as (since the hold began).
    moving_held_since: Option<f32>,
    turned: Option<(Place, f32)>,
    /// Since when this player has stood in the host's body ([`in_body`]).
    in_host_since: Option<f32>,
    /// The modifiers put on this screen for the host, with the host's seconds for each as
    /// last heard.
    mods: HashMap<u32, f32>,
    /// A guest's: its own scripts' modifiers were taken off (module notes).
    own_mods_dropped: bool,
    /// A guest's: the slide marker it watches a slideshow from ([`slide_spot`]), while it
    /// does.
    slide: Option<FormId>,
}

/// The groups held, as [`SceneView::locks`] bits.
pub(super) fn lock_bits(locks: [bool; 7]) -> u8 {
    locks.iter().enumerate().filter(|(_, l)| **l).fold(0, |b, (i, _)| b | 1 << i)
}

fn locks_of(bits: u8) -> [bool; 7] {
    std::array::from_fn(|i| bits & 1 << i != 0)
}

/// A host's scene now: its scripts' locks, and its screen's modifiers while one is held.
/// (A host holds nothing for another: `coop_host_locks` is clear.)
pub(super) fn scene_view(g: &fnv_game::Game) -> SceneView {
    // Its scripts' locks only [design]: the host's own menus and the things it uses hold
    // the host alone. (On a host `coop_host_locks` is clear, so these are its own.)
    let locks = lock_bits(g.controls_locked());
    // Held: every modifier on its screen; else those its scripts put there (the
    // REPCONN launch's RepconLaunch, RepRocketLaunch and RepconSwoop flashes, with the
    // players free), which the guests in the host's place show.
    let mods = g.image_modifiers.iter().filter(|m| locks != 0 || g.script_image_modifiers.contains(&m.modifier)).map(|m| (m.modifier.0, m.elapsed as f32)).take(net::SCENE_MODS_MAX).collect();
    // Story gear is the progress side's (piece 3): none sent.
    let worn = Vec::new();
    let swaps = if locks == 0 { Vec::new() } else { scene_swaps(&g.texture_swaps) };
    SceneView { locks, mods, worn, swaps }
}

/// The host's texture swaps a held scene sends, at most [`net::SCENE_SWAPS_MAX`]: by
/// reference, each one's last calls first. `TextureSwaps` keeps the last call per shape,
/// so a slideshow's screen, swapped again and again, sends its current slide.
fn scene_swaps(swaps: &fnv_game::functions::misc::TextureSwaps) -> Vec<(u32, String, String)> {
    let mut all: Vec<(u32, String, String)> = swaps.by_ref.iter().flat_map(|(r, l)| l.iter().rev().map(move |(s, t)| (r.0, s.clone(), t.clone()))).collect();
    all.truncate(net::SCENE_SWAPS_MAX);
    all
}

/// What the modifiers on the host's screen ask of this one: the modifiers to put on
/// (new, or started again on the host's) and those to take off (gone from the host's).
/// `given` is updated to the host's list.
fn follow_mods(given: &mut HashMap<u32, f32>, host: &[(u32, f32)]) -> (Vec<u32>, Vec<u32>) {
    let mut on = Vec::new();
    for &(m, at) in host {
        if given.get(&m).is_none_or(|&was| at + RESTARTED < was) {
            on.push(m);
        }
        given.insert(m, at);
    }
    let off: Vec<u32> = given.keys().copied().filter(|m| !host.iter().any(|(h, _)| h == m)).collect();
    for m in &off {
        given.remove(m);
    }
    (on, off)
}

/// The modifiers a guest's own scripts put on its screen (`script_mods`) that are taken
/// off when it starts playing as a guest: all of them but those the host's screen has
/// (`host`, as [`follow_mods`] keeps them).
fn own_mods_to_drop(script_mods: &std::collections::BTreeSet<FormId>, host: &HashMap<u32, f32>) -> Vec<FormId> {
    script_mods.iter().copied().filter(|m| !host.contains_key(&m.0)).collect()
}

/// How far the host may turn within a scene's hold before a held player is turned to face
/// as the host again (radians; about 10 degrees, past the jitter of a head turn).
const TURN_AGAIN: f32 = 0.17;

/// The smaller angle between two yaws, in radians.
fn angle_apart(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(std::f32::consts::TAU);
    d.min(std::f32::consts::TAU - d)
}

/// Whether two players' feet stand in one body ([`IN_BODY`], [`IN_BODY_UP`]).
fn in_body(a: [f32; 3], b: [f32; 3]) -> bool {
    let (dx, dy, dz) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
    dx * dx + dy * dy < IN_BODY * IN_BODY && dz.abs() < IN_BODY_UP
}

/// Where guest `slot` (its player id, 1..=`MAX_GUESTS`) is put beside the host: a 3 x 2
/// grid one [`BESIDE`] apart round the host's feet, so with 5 guests (the 6-player lobby)
/// nobody stands more than one step and a diagonal (about 102 units) away. A row of 5 out
/// to one side reached 216 units, through the walls of a tight interior (Doc Mitchell's
/// house, the Lucky 38 elevator) [design]. Slots 1-2 are right and left, as before;
/// 3-4 behind those, 5 straight behind.
pub(crate) fn beside_slot(feet: [f32; 3], yaw: f32, slot: u8) -> [f32; 3] {
    let (side, back) = match slot.max(1) {
        1 => (1.0, 0.0),
        2 => (-1.0, 0.0),
        3 => (1.0, 1.0),
        4 => (-1.0, 1.0),
        5 => (0.0, 1.0),
        // Not reached with `MAX_GUESTS` 5: further rows behind.
        n => (if n.is_multiple_of(2) { -1.0 } else { 1.0 }, f32::from(n.div_ceil(3))),
    };
    let right = yaw + std::f32::consts::FRAC_PI_2;
    let (r, b) = (BESIDE * side, BESIDE * back);
    [feet[0] + right.sin() * r - yaw.sin() * b, feet[1] + right.cos() * r - yaw.cos() * b, feet[2]]
}

#[allow(clippy::too_many_arguments)]
pub(super) fn share_scene(
    mut coop: ResMut<Coop>,
    time: Res<Time>,
    script: Option<ResMut<nvf_play::script::ScriptState>>,
    player: Option<ResMut<Player>>,
    loc: Option<Res<Location>>,
    loading: Option<Res<Loading>>,
    mut travel: MessageWriter<TravelRequest>,
    solo: Option<Res<nvf_play::save::SoloBase>>,
    mut phys: Option<ResMut<nvf_play::phys::PhysState>>,
) {
    let now = time.elapsed_secs();
    let coop = &mut *coop;
    let Some(mut script) = script else { return };
    let Some(rt) = script.rt.as_mut() else { return };
    if coop.is_host() {
        rt.game.coop_host_locks = [false; 7];
        coop.scene = Shared::default();
        if let Some(link) = coop.link.as_ref() {
            link.set_scene(scene_view(&rt.game));
        }
        return;
    }
    let (guest, lost) = (coop.is_guest(), coop.host_lost);
    // In the host's place (its interior, or its worldspace): a script's modifiers on the
    // host's screen show here too while nobody is held.
    let with_host = loc.as_deref().map(super::place_of).is_some_and(|p| coop.host_place.is_some_and(|(h, _)| h == p));
    let s = &mut coop.scene;
    // The host's scene now: none when not a guest (left the lobby), the host is lost or
    // not heard lately, or this game is not yet kept as the guest's own.
    let heard = s.heard.as_ref().filter(|(_, at)| guest && !lost && solo.is_some() && now - at < STALE).map(|(v, _)| v.clone());
    if !guest {
        s.heard = None;
    }
    let view = heard.unwrap_or_default();
    let g = &mut rt.game;
    let held = locks_of(view.locks);
    if held != s.held {
        if held.iter().any(|h| *h) {
            info!("co-op: the host's scene holds this player (controls {held:?})");
        } else {
            info!("co-op: the host's scene lets this player go");
        }
        s.held = held;
    }
    g.coop_host_locks = held;
    let mods: &[(u32, f32)] = if view.locks != 0 || with_host { &view.mods } else { &[] };
    let (on, off) = follow_mods(&mut s.mods, mods);
    for m in on {
        info!("co-op: the host's scene: image space modifier {m:#010X} on this screen");
        g.apply_image_modifier(FormId(m));
    }
    for m in off {
        g.remove_image_modifier(FormId(m));
    }
    // This guest's own opening's modifiers: off once its game is its own.
    if !guest {
        s.own_mods_dropped = false;
    } else if solo.is_some() && !s.own_mods_dropped {
        s.own_mods_dropped = true;
        for m in own_mods_to_drop(&g.script_image_modifiers, &s.mods) {
            info!("co-op: image space modifier {} ({:#010X}) off: this guest's own scripts put it on", g.data.name(m), m.0);
            g.remove_image_modifier(m);
            g.script_image_modifiers.remove(&m);
        }
    }
    // The host's slides: its line scripts swapped them, which this game does not run.
    for (on, shape, texture) in &view.swaps {
        if g.texture_swaps.follow_host(FormId(*on), shape.clone(), texture.clone()) {
            info!("co-op: the host's scene: {texture} on {on:#010X} ({shape})");
        }
    }
    // Beside the host while its movement is held.
    s.moving_held_since = if held[0] { s.moving_held_since.or(Some(now)) } else { None };
    if s.moving_held_since.is_none() {
        s.turned = None;
    }
    let settled = s.moving_held_since.is_some_and(|t| now - t >= SETTLE);
    let (Some(host), Some(mut player), Some(loc)) = (coop.host, player, loc) else { return };
    // Looking held too (a slideshow, a speech): this player faces the way the host does,
    // once, so it sees what the host is shown.
    // Again whenever the host is moved or turned within the hold (a guest turned once
    // at a talk, then summoned into a slideshow room, kept that heading, so its view
    // never reached the screen the host faced [observed: buckallobill, in game, FNV 1.4.0.525]).
    let turn_again = s.turned.is_none_or(|(place, yaw)| place != host.place || angle_apart(yaw, host.yaw) > TURN_AGAIN);
    if settled && held[4] && turn_again && place_of(&loc) == host.place {
        player.heading = host.yaw;
        player.pitch = host.pitch;
        s.turned = Some((host.place, host.yaw));
        info!("co-op: scene: this player faces as the host ({:.0} deg)", host.yaw.to_degrees());
    }
    // Out of the host's body, when no scene holds the view (module notes).
    let loads = loading.as_deref().is_some_and(|l| l.active());
    // On a slide marker the players stand in one another ([`slide_spot`]).
    let on_slide = s.slide.is_some();
    let in_host = !on_slide && !scene_holds_view(held) && place_of(&loc) == host.place && in_body(player.feet, host.feet);
    s.in_host_since = if in_host { s.in_host_since.or(Some(now)) } else { None };
    if s.in_host_since.is_some_and(|t| now - t >= SETTLE) && !loads && coop.downed_at.is_none() {
        let feet = beside_slot(host.feet, host.yaw, coop.link.as_ref().and_then(|l| l.id()).unwrap_or(1));
        if let Some(phys) = phys.as_deref_mut().filter(|p| p.player.is_some() && p.ready_at(feet)) {
            let was = player.feet;
            phys.place_player(feet, 60.0, 200.0);
            let at = phys.player.as_ref().map_or(feet, |c| c.feet);
            player.feet = at;
            player.placed = Some(at);
            coop.scene.in_host_since = None;
            coop.last_leash = Some(now);
            info!(
                "co-op: scene: out of the host's body: this player stood in it at {:?} ({:.0} units from the host's feet); put beside it at {at:?}",
                host.place,
                ((was[0] - host.feet[0]).powi(2) + (was[1] - host.feet[1]).powi(2)).sqrt()
            );
            return;
        }
    }
    // A slideshow's room: the slide marker is this player's spot ([`slide_spot`]).
    if !settled || loads || coop.downed_at.is_some() || on_slide {
        return;
    }
    let (dx, dy) = (player.feet[0] - host.feet[0], player.feet[1] - host.feet[1]);
    // The scene holds the whole view (a slideshow): watched from the host's own spot.
    // From its place beside the host the slides stood off-centre on a guest's screen,
    // black down one side [observed: buckallobill, in game, FNV 1.4.0.525]. The players' bodies are hidden and in nobody's way
    // meanwhile ([`hide_players_in_scene`]).
    if scene_holds_view(held) && place_of(&loc) == host.place && dx * dx + dy * dy > ON_SPOT * ON_SPOT {
        coop.last_leash = Some(now);
        // Already in the host's place: put there at once, with no loading screen (a
        // second loading screen for a step of a few hundred units in the same cell
        // otherwise).
        if let Some(phys) = phys.as_deref_mut().filter(|p| p.player.is_some() && p.ready_at(host.feet)) {
            phys.place_player(host.feet, 60.0, 200.0);
            let feet = phys.player.as_ref().map_or(host.feet, |c| c.feet);
            player.feet = feet;
            player.placed = Some(feet);
            player.heading = host.yaw;
            info!("co-op: scene: on the host's spot at {:?} {feet:?}: the scene holds the view (put there, same place)", host.place);
            return;
        }
        let (world, interior) = match host.place {
            Place::Interior(c) => (None, Some(c)),
            Place::Exterior(w) => (Some(w), None),
        };
        info!("co-op: scene: on the host's spot at {:?} {:?}: the scene holds the view", host.place, host.feet);
        travel.write(TravelRequest(Travel { world, interior, feet: host.feet, heading_deg: host.yaw.to_degrees(), pitch_deg: None }));
        return;
    }
    if coop.last_leash.is_some_and(|t| now - t < super::LEASH_EVERY) {
        return;
    }
    // Another place is the leash's (`super::leash`).
    if place_of(&loc) != host.place || dx * dx + dy * dy <= NEAR * NEAR {
        return;
    }
    let feet = beside_slot(host.feet, host.yaw, coop.link.as_ref().and_then(|l| l.id()).unwrap_or(1));
    let (world, interior) = match host.place {
        Place::Interior(c) => (None, Some(c)),
        Place::Exterior(w) => (Some(w), None),
    };
    info!("co-op: scene: beside the host at {:?} {feet:?}", host.place);
    travel.write(TravelRequest(Travel { world, interior, feet, heading_deg: host.yaw.to_degrees(), pitch_deg: None }));
    coop.last_leash = Some(now);
}

/// Whether a scene holds this player's view: its movement and its looking are both held
/// (a DLC's opening slideshow, Elijah's broadcast; the host's own locks on the host, the
/// host's shared ones on a guest).
fn scene_holds_view(locks: [bool; 7]) -> bool {
    locks[0] && locks[4]
}

/// Why the other players' bodies are not drawn on this screen, if they are not: while
/// something other than the player drives this player's camera, every other body is
/// hidden. In the original only one player stands in such a shot, and a co-op player in
/// it filled the screen with a face [observed: buckallobill, in game, Dead Money in co-op:
/// players should be invisible so they do not see each other's faces]:
///
/// - a scene holds the view: its movement and its looking held (a DLC's opening or
///   ending slideshow, Elijah's broadcast; every player watches from the scene's spot in
///   first person, and a body on that spot would fill the screen; in the original the
///   camera stays on the slideshow's screen with no body in view [design]);
/// - a slideshow has the view (`nvf_play::hud::in_slideshow`), its controls held
///   or not: every player stands on the room's one slide marker ([`slide_spot`]);
/// - an idle's camera node drives the view (`nvf_play::player::ViewDriven`: the opening's
///   and the Sierra Madre villa's wake-up);
/// - a script places or turns the camera (JohnnyGuitar's SetCameraTranslate,
///   SetCameraRotate, SetCameraRotateToRef: `CameraRequests`' overrides).
///
/// They are drawn again when it ends.
fn hidden_because(locks: [bool; 7], slideshow: bool, idle_drives_view: bool, script_camera: bool) -> Option<&'static str> {
    if scene_holds_view(locks) {
        Some("the scene holds the view")
    } else if slideshow {
        Some("a slideshow has the view")
    } else if idle_drives_view {
        Some("an idle's camera node drives the view")
    } else if script_camera {
        Some("a script drives the camera")
    } else {
        None
    }
}

/// Whether a script has placed or turned the camera (`CameraRequests`).
fn script_camera(c: &fnv_game::functions::camera::CameraRequests) -> bool {
    c.translate_override.is_some() || c.rotate_override.iter().any(Option::is_some) || c.rotate_track.is_some()
}

/// Hides the other players' bodies on this screen while [`hidden_because`] says so (a
/// scene, an idle's camera, a script's camera), and draws them again after.
#[allow(clippy::too_many_arguments)]
pub(super) fn hide_players_in_scene(
    coop: Res<Coop>,
    script: Option<Res<nvf_play::script::ScriptState>>,
    driven: Option<Res<nvf_play::player::ViewDriven>>,
    (loc, sim): (Option<Res<Location>>, Option<Res<nvf_play::sim::SimState>>),
    mut bodies: Query<&mut Visibility, With<super::Puppet>>,
    mut hidden: Local<Option<&'static str>>,
    mut blocks: Option<ResMut<nvf_play::blocking::ActorBlocks>>,
) {
    let game = script.as_ref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game);
    let why = (coop.is_host() || coop.is_guest())
        .then(|| {
            hidden_because(
                game.map_or([false; 7], |g| g.controls_locked()),
                in_slideshow(loc.as_deref(), sim.as_deref()),
                driven.as_deref().is_some_and(|d| d.0),
                game.is_some_and(|g| script_camera(&g.ui.camera)),
            )
        })
        .flatten();
    if why != *hidden {
        let (was, now) = (hidden.is_some(), why.is_some());
        *hidden = why;
        match why {
            Some(w) => info!("co-op: scene: the other players' bodies hidden while {w}"),
            None => info!("co-op: scene: the other players' bodies drawn again"),
        }
        // Nor in anybody's way: the guests stand on the host's spot meanwhile, and a
        // body there pushed the host aside [observed: buckallobill, in game, FNV 1.4.0.525].
        if was != now {
            if let Some(b) = blocks.as_deref_mut() {
                for r in coop.peers.iter().flat_map(|(&id, p)| p.references(id)) {
                    if now {
                        b.in_nobodys_way.insert(r);
                    } else {
                        b.in_nobodys_way.remove(&r);
                    }
                }
            }
        }
    }
    let want = if why.is_some() { Visibility::Hidden } else { Visibility::Inherited };
    for mut v in &mut bodies {
        v.set_if_neq(want);
    }
}

/// Whether this player's cell is a slideshow's room (`nvf_play::hud::in_slideshow`).
fn in_slideshow(loc: Option<&Location>, sim: Option<&nvf_play::sim::SimState>) -> bool {
    loc.zip(sim).is_some_and(|(l, s)| nvf_play::hud::in_slideshow(l.interior, &s.refs.cell_edids))
}

/// The engine's heading marker base (`XMarkerHeading`, `meshes\markerxheading.nif`).
const X_MARKER_HEADING: FormId = FormId(0x0000_0034);

/// A slideshow room's slide marker, from its references (each one's FormID, base,
/// position and angles): its XMarkerHeading. Each of the five rooms (the ending's and the
/// four add-ons') has exactly one, the spot its script moves the player to, facing the
/// slide, heading 0 and level, beside one COCMarkerHeading 180 units behind and one
/// AudioMarker [observed: the ESM and add-on cell references]. The lowest FormID should
/// a room ever have more.
fn slide_marker(refs: impl IntoIterator<Item = (FormId, FormId, [f32; 3], [f32; 3])>) -> Option<(FormId, [f32; 3], [f32; 3])> {
    refs.into_iter().filter(|(_, base, ..)| *base == X_MARKER_HEADING).map(|(id, _, pos, rot)| (id, pos, rot)).min_by_key(|(id, ..)| id.0)
}

/// Whether feet at `feet` are off the slide marker at `marker`: more than [`ON_SPOT`]
/// across, or on another floor.
fn off_the_marker(feet: [f32; 3], marker: [f32; 3]) -> bool {
    let (dx, dy, dz) = (feet[0] - marker[0], feet[1] - marker[1], feet[2] - marker[2]);
    dx * dx + dy * dy > ON_SPOT * ON_SPOT || dz.abs() > IN_BODY_UP
}

/// The view along a slide marker with angles `rot` (radians, X down positive): its
/// heading, and its pitch up positive, as the MoveTo of the player gives the host
/// (`fnv_game::functions::refs::move_ref`).
fn marker_view(rot: [f32; 3]) -> (f32, f32) {
    (rot[2], -rot[0])
}

/// A guest in a slideshow's room stands on its slide marker and looks along it while the slideshow has the view
/// (module notes, "On the slide marker"), so its screen shows the slide centred and
/// head-on as the host's does; let go once it is out of the room.
///
/// Local to each guest, not sent by the host: the guest's own cell already says a
/// slideshow has its view (`nvf_play::hud::in_slideshow`), and the marker is in the
/// guest's own copy of the same plugins. However the guest came in (the host's MoveTo's
/// summon, a console move) and whether or not the host's controls hold its looking, it
/// watches from the same spot, with nothing new on the wire for another machine's build
/// to agree on [design].
pub(super) fn slide_spot(
    mut coop: ResMut<Coop>,
    loc: Option<Res<Location>>,
    sim: Option<Res<nvf_play::sim::SimState>>,
    loading: Option<Res<Loading>>,
    mut player: Option<ResMut<Player>>,
    mut phys: Option<ResMut<nvf_play::phys::PhysState>>,
) {
    let room = loc.as_deref().and_then(|l| l.interior).filter(|_| coop.is_guest() && in_slideshow(loc.as_deref(), sim.as_deref()));
    let marker = room.and_then(|cell| {
        let sim = sim.as_deref()?;
        let ids = sim.refs.by_cell.get(&FormId(cell))?;
        slide_marker(ids.iter().filter_map(|id| sim.refs.get(*id)).map(|r| (r.id, r.base, r.pos, r.rot)))
    });
    let Some((id, pos, rot)) = marker else {
        if coop.scene.slide.take().is_some() {
            info!("co-op: slideshow: over; the view is this player's again");
        }
        return;
    };
    let Some(player) = player.as_deref_mut() else { return };
    if loading.as_deref().is_some_and(|l| l.active()) {
        return;
    }
    let first = coop.scene.slide != Some(id);
    if off_the_marker(player.feet, pos) {
        // The room is streamed in by now; until it is, try again next frame.
        let Some(phys) = phys.as_deref_mut().filter(|p| p.player.is_some() && p.ready_at(pos)) else { return };
        let was = player.feet;
        phys.place_player(pos, 60.0, 200.0);
        let at = phys.player.as_ref().map_or(pos, |c| c.feet);
        player.feet = at;
        player.placed = Some(at);
        let off = ((was[0] - pos[0]).powi(2) + (was[1] - pos[1]).powi(2)).sqrt();
        if first {
            info!("co-op: slideshow: this player watches from the slide marker {:#010X} at {at:?}, heading {:.0} deg (it stood {off:.0} units off it)", id.0, rot[2].to_degrees());
        } else {
            debug!("co-op: slideshow: back on the slide marker {:#010X} ({off:.0} units off it)", id.0);
        }
    } else if first {
        info!("co-op: slideshow: this player watches from the slide marker {:#010X} at {pos:?}, heading {:.0} deg (already on it)", id.0, rot[2].to_degrees());
    }
    coop.scene.slide = Some(id);
    // Its looking too, every frame: the host's controls may not hold it.
    (player.heading, player.pitch) = marker_view(rot);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_player_on_the_hosts_feet_stands_in_its_body() {
        let host = [2257.0, 2318.0, 7360.0];
        // Both new games start on one spot (Doc Mitchell's house).
        assert!(in_body(host, host));
        assert!(in_body([2257.0 + 30.0, 2318.0, 7362.0], host));
        // Beside it, as a guest is put: not in it.
        assert!(!in_body(beside_slot(host, 0.0, 1), host));
        assert!(!in_body(beside_slot(host, 1.0, 2), host));
        // A floor above.
        assert!(!in_body([2257.0, 2318.0, 7360.0 + 200.0], host));
    }

    #[test]
    fn a_guest_in_a_slide_room_watches_from_its_slide_marker() {
        // The ending's room: its COCMarkerHeading, the slide marker the ending's script
        // moves the host to, and its AudioMarker.
        let room = [
            (FormId(0x0016_1EB0), FormId(0x0000_0032), [2622.2, 287.8, 10112.0], [0.0; 3]),
            (FormId(0x0016_39F6), X_MARKER_HEADING, [2802.5, 256.6, 10112.0], [0.0; 3]),
            (FormId(0x0017_4AA0), FormId(0x0000_0015), [2897.4, 226.3, 10335.3], [0.0; 3]),
        ];
        let (id, pos, rot) = slide_marker(room).expect("the room's slide marker");
        assert_eq!(id, FormId(0x0016_39F6));
        // The summon put the guest beside the host (guest place 1)
        // and the console 120 units to the side; either is off the slide's axis.
        assert!(off_the_marker(beside_slot(pos, 0.0, 1), pos));
        assert!(off_the_marker([pos[0] + 120.0, pos[1], pos[2]], pos));
        // On it (the ground's rest gap, a step's slack): left where it is.
        assert!(!off_the_marker([pos[0] + 3.0, pos[1] - 2.0, pos[2] + 1.5], pos));
        // It looks along the marker, level: straight at the slide, as the host does.
        assert_eq!(marker_view(rot), (0.0, 0.0));
        // A marker turned and tipped down 0.1 rad: that heading, the pitch 0.1 down.
        assert_eq!(marker_view([0.1, 0.0, 1.5]), (1.5, -0.1));
        // No slide marker (a room with none): nothing to watch from, the view stays free.
        assert_eq!(slide_marker([room[0], room[2]]), None);
        // Two (none of the five rooms has): the lower FormID.
        let twin = (FormId(0x0016_39F0), X_MARKER_HEADING, [0.0; 3], [0.0; 3]);
        assert_eq!(slide_marker([room[1], twin]).map(|m| m.0), Some(FormId(0x0016_39F0)));
    }

    #[test]
    fn other_players_are_hidden_while_anything_but_the_player_drives_the_camera() {
        // Slides, the villa wake-up's idle camera, a script's camera.
        let free = [false; 7];
        assert_eq!(hidden_because(free, false, false, false), None, "the player's own view: drawn");
        let mut slides = free;
        slides[0] = true;
        slides[4] = true;
        assert_eq!(hidden_because(slides, false, false, false), Some("the scene holds the view"));
        // A slideshow's room with nothing held (a console or teleport move into it).
        assert_eq!(hidden_because(free, true, false, false), Some("a slideshow has the view"));
        assert_eq!(hidden_because(free, false, true, false), Some("an idle's camera node drives the view"));
        assert_eq!(hidden_because(free, false, false, true), Some("a script drives the camera"));
        // Movement alone held (a talk): the others are seen.
        let mut talk = free;
        talk[0] = true;
        assert_eq!(hidden_because(talk, false, false, false), None);
        let mut c = fnv_game::functions::camera::CameraRequests::default();
        assert!(!script_camera(&c));
        c.rotate_override[2] = Some(90.0);
        assert!(script_camera(&c));
        c = Default::default();
        c.rotate_track = Some(FormId(0x14));
        assert!(script_camera(&c));
        c = Default::default();
        c.translate_override = Some([0.0, -200.0, 50.0]);
        assert!(script_camera(&c));
    }

    #[test]
    fn lock_bits_round_trip() {
        let l = [true, false, true, false, false, false, true];
        assert_eq!(lock_bits(l), 0b100_0101);
        assert_eq!(locks_of(lock_bits(l)), l);
        assert_eq!(lock_bits([false; 7]), 0);
    }

    #[test]
    fn mods_follow_the_host() {
        let mut given = HashMap::new();
        // A fade starts on the host's screen: on here.
        assert_eq!(follow_mods(&mut given, &[(0x0101_013B, 0.1)]), (vec![0x0101_013B], vec![]));
        // It plays on: nothing to do.
        assert_eq!(follow_mods(&mut given, &[(0x0101_013B, 0.6)]), (vec![], vec![]));
        // It starts again (the next line's fade): on again.
        assert_eq!(follow_mods(&mut given, &[(0x0101_013B, 0.05)]), (vec![0x0101_013B], vec![]));
        // It is gone from the host's: off here.
        assert_eq!(follow_mods(&mut given, &[]), (vec![], vec![0x0101_013B]));
        assert!(given.is_empty());
    }

    #[test]
    fn a_guest_drops_its_own_openings_modifiers_but_not_the_hosts() {
        // A fresh guest's opening has two image modifiers on; the host's scene sends a
        // third, which this guest also has on.
        let (white, black, strain) = (FormId(0x00AB_C10B), FormId(0x00AB_C10C), FormId(0x00AB_C10D));
        let own: std::collections::BTreeSet<FormId> = [white, black, strain].into_iter().collect();
        let mut given = HashMap::new();
        follow_mods(&mut given, &[(strain.0, 0.2)]);
        assert_eq!(own_mods_to_drop(&own, &given), vec![white, black]);
        assert_eq!(own_mods_to_drop(&own, &HashMap::new()).len(), 3, "the host sends none: all off");
        assert!(own_mods_to_drop(&Default::default(), &given).is_empty());
    }

    #[test]
    fn beside_is_to_the_right() {
        // Facing north, the right is east (+X).
        let f = beside_slot([0.0, 0.0, 10.0], 0.0, 1);
        assert!((f[0] - BESIDE).abs() < 1e-3 && f[1].abs() < 1e-3 && f[2] == 10.0);
        // Facing east, the right is south (-Y).
        let f = beside_slot([0.0, 0.0, 0.0], std::f32::consts::FRAC_PI_2, 1);
        assert!(f[0].abs() < 1e-3 && (f[1] + BESIDE).abs() < 1e-3);
    }

    #[test]
    fn each_guest_has_its_own_place_beside_the_host() {
        // Six players: the host and five guests (`MAX_GUESTS`), as the lobby screen says.
        assert_eq!(nvf_play::lobby::LOBBY_PLAYERS, nvf_coop::net::MAX_GUESTS + 1);
        let spots: Vec<[f32; 3]> = (1..=nvf_coop::net::MAX_GUESTS as u8).map(|k| beside_slot([0.0, 0.0, 0.0], 0.0, k)).collect();
        assert_eq!(spots.len(), 5);
        assert!((spots[0][0] - BESIDE).abs() < 1e-3);
        assert!((spots[1][0] + BESIDE).abs() < 1e-3);
        // Facing north, behind is south (-Y).
        assert!((spots[2][0] - BESIDE).abs() < 1e-3 && (spots[2][1] + BESIDE).abs() < 1e-3);
        assert!((spots[3][0] + BESIDE).abs() < 1e-3 && (spots[3][1] + BESIDE).abs() < 1e-3);
        assert!(spots[4][0].abs() < 1e-3 && (spots[4][1] + BESIDE).abs() < 1e-3);
        let host = [0.0f32, 0.0, 0.0];
        for (i, a) in spots.iter().enumerate() {
            // Close to the host, for tight interiors, and never in its body.
            assert!(a[0].hypot(a[1]) <= BESIDE * std::f32::consts::SQRT_2 + 1e-3);
            assert!(!in_body(*a, host));
            for b in &spots[i + 1..] {
                assert!((a[0] - b[0]).hypot(a[1] - b[1]) >= BESIDE - 1e-3);
            }
        }
    }
}
