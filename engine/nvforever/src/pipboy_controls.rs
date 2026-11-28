//! The Pip-Boy's needle and knobs on the first-person arms (docs/spec-notes/pipboy.md §2):
//! `RadNeedle` follows the player's total rads, `TabKnob` the page's tab, and `ScrollKnob`
//! the list's row. All the numbers are here, so a closer capture can replace them.
//!
//! The needle (its two shapes) turns about its local Z through its origin, out of the dial,
//! and the wheel about its local X through its origin, its axle: their vertices lie around
//! their origins [observed: `fnvtool nif decode`]. The tab knob turns through its origin too,
//! about its node's Z, which is not its shape's ([`KNOB_SHAFT`]), and stands leaned off the
//! plate as the original's does ([`KNOB_LEAN_AXIS`]).
//! The model's X runs right across the device in the view and its Y up [observed: where the
//! nodes are against the shots]. The shapes are rigid, each a child of the bone it is
//! attached to, so an absolute `bind × rotation` on its own transform turns it, after the arm
//! is posed and before transforms propagate. No rotation accumulates, and a rebuilt rig's new
//! shapes take a bind of their own.
//!
//! Only the shapes this was measured on turn: rigid, and of the size they are in the game's
//! male and female arm ([`KNOWN`]). A replacement model can share the names with other
//! pivots, axes or skinning, so any other shape is left as it is, said once for each control
//! (`pipboy: <control> left still: ...`). Only the arms rig's shapes are tagged
//! (`player_body.rs`): the third-person model's controls stay still, and NPCs' arms are not
//! touched.
//!
//! Off the arm no reading comes: a control finishes travelling to the last one, then holds
//! [hypothesis: nothing was captured down]. The first reading is taken at once, so a loaded
//! game's needle does not sweep up. While the Pip-Boy is up the needle quivers a little about
//! where it rests ([`NEEDLE_QUIVER`]); the travel and the log are without the quiver.
//!
//! [design: Android scrollbar request] The wheel now reads the list view's actual scroll
//! value, independent of selected row: a native list's value, STATS top or the hand-built
//! list's first row. This is NVForever's synchronization policy, not a new measurement of
//! the original device's mechanical step size.

use crate::pipboy_arm::{PartMesh, PipBoyPart};
use bevy::mesh::skinning::SkinnedMesh;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use fnv_sim::FormId;
use nvf_play::menu::model::Side;
use nvf_play::menu::ActiveMenu;
use nvf_play::pipboy::data::DataState;
use nvf_play::pipboy::stats::StatsView;
use nvf_play::pipboy::{Page, PipBoy};
use nvf_play::script::ScriptState;

/// Total rads, the actor value the STATS page's RAD meter shows (`stats/read.rs`).
const RADS: u16 = 54;

/// The needle in the model's bind pose is at 500 on the dial, and 100 rads turn it 18°: 0
/// straight down, 500 to the right, 1000 straight up, through the right side, and not the
/// rate: the same at a rate of 9 with 58 rads in total [observed: private job
/// pipboy_special_device_motion: on the shots 0 reads 178°, 500 86° and 990 2° clockwise
/// from up, against 179°, 89° and 1° from this]. Held at the dial's ends [hypothesis].
const NEEDLE_BIND_RADS: f64 = 500.0;
const NEEDLE_MAX_RADS: f64 = 1000.0;
const NEEDLE_DEGREES_PER_RAD: f32 = 0.18;
/// Degrees a second, the same for a long way as a short one. [observed: private job
/// pipboy_special_device_motion: about 0.6° a game loop in the drops from 990 and 600; a
/// drop of 690 rads is still on its way two wall seconds on.] The capture has no clock finer
/// than a whole second nor the loops' rate, so this is [hypothesis] within about 20 to 40.
/// It shows no start-up delay either: the first shot to differ comes 3 to 15 samples after
/// the change, which the shots' own stalls can account for.
const NEEDLE_SPEED: f32 = 30.0;

/// The tab knob's five detents, one a tab of whichever page (tab 0 of each is the same, and
/// five steps come back round): the turn that brings the fin's tip onto each of the dial's five
/// marks on the screen, with the lean below, is 67°, 37°, 10°, −16°, −49° [observed: the model's
/// fin and marks (`fnvtool nif decode`, the plate's UVs onto `PipBoyArm01.dds`) through a camera
/// fitted to the private job pipboy_special_device_motion, 2026-11-05]. A constant 28° a tab is
/// within 3° of those; that the turn is constant is [hypothesis].
const KNOB_BIND_TAB: i32 = 2;
const KNOB_DEGREES_PER_TAB: f32 = 28.0;
const KNOB_BIND_OFFSET: f32 = 10.0;
/// The knob's shaft in its shape's own frame: the Z of the `TabKnob` node, through the shape's
/// origin, in both arms. In that frame the knob is straight: its base flat (height within
/// 0.02), its fins opposite each other, and the dial's disc centred on the origin
/// [observed: `fnvtool nif decode`]. The shape's own local Z leans 28° off it.
const KNOB_SHAFT: [f32; 3] = [-0.0378, -0.4754, 0.8790];
/// The whole knob then leans 17.6° off the plate, its top towards the top of the device: stood
/// square to the plate, it was seen tilted too far against the original (playtest 2026-11-05).
/// The lean is what dropping the node's own tilt would give, so that the knob stands on the
/// model's Z rather than the node's; that the original does so is [hypothesis]. Silhouettes
/// against the private job's five tabs do not settle it (it scores 0.145 where square scores
/// 0.147), though a free fit of the lean landed within 3° of it. Measured on the male arm; the
/// female's node differs by about 3°. The axis is in the shape's frame.
const KNOB_LEAN_AXIS: [f32; 3] = [-0.9922, 0.1223, 0.0235];
const KNOB_LEAN_DEGREES: f32 = 17.61;
/// Degrees a second. [observed: private job pipboy_special_device_motion: a step is over
/// within about 8 game loops; hypothesis: the rate of those loops, and that the wrap from tab
/// 4 to 0 swings back at the same speed.]
const KNOB_SPEED: f32 = 180.0;

/// The wheel moves while a long list is moved through by key, and is back in the same place
/// at the top of the list; the mouse wheel at the top of the list moved neither [observed:
/// private job pipboy_special_device_motion: its ridges come back to within 0.3 pixels].
/// [hypothesis: that it follows the selected row, which the capture does not separate from
/// the list's scroll; the ridges' spacing, about 17 pixels, hides whole ridges moved between
/// shots, so the capture does not give the step; which way it turns, the front going down
/// with the list, is not resolved.] The wheel has 26 ridges: its outermost vertices come
/// every 13.85° about its axle [observed: `fnvtool nif decode`, `ScrollKnob:0`]. One ridge a
/// row [hypothesis; the first reading of the capture, 2° a row, was judged far too little
/// against the original, playtest 2026-11-05].
const WHEEL_RIDGES: f32 = 26.0;
const WHEEL_DEGREES_PER_ROW: f32 = 360.0 / WHEEL_RIDGES;
/// Degrees a second: a row's ridge in about the time a key repeat takes, so a held key keeps
/// the wheel turning rather than lagging [hypothesis].
const WHEEL_SPEED: f32 = 200.0;

/// The needle quivers about where it rests while the Pip-Boy is up: a small shiver [observed:
/// playtest 2026-11-05, against the original: "an idle bounce of sorts"]. Two sines of
/// unrelated rates, degrees and hertz, so it does not visibly repeat; the amounts and rates are
/// [hypothesis], judged by eye (twice these rates was too quick, the same playtest). It is
/// shown only: the logged angles and the travel are without it.
const NEEDLE_QUIVER: [(f32, f32); 2] = [(1.2, 3.65), (0.8, 5.95)];

/// The shapes these numbers were measured on: their bounds, least and most corner, in
/// `meshes\\pipboy3000\\pipboyarm.nif` and `pipboyarmfemale.nif` [observed: `fnvtool nif
/// decode`; the two differ by up to 0.012 at the wheel].
const KNOWN: [(PipBoyPart, [f32; 3], [f32; 3]); 4] = [
    (PipBoyPart::Needle, [-0.018, -0.027, 0.022], [0.388, 0.028, 0.022]),
    (PipBoyPart::Needle, [-0.051, -0.044, 0.0], [0.051, 0.044, 0.052]),
    (PipBoyPart::TabKnob, [-0.418, -0.492, -0.19], [0.411, 0.404, 0.606]),
    (PipBoyPart::ScrollKnob, [-0.147, -1.197, -1.201], [0.147, 1.197, 1.201]),
];
const KNOWN_WITHIN: f32 = 0.02;

/// Why a tagged shape does not turn; `None` for one this was measured on.
fn unsupported(part: PipBoyPart, mesh: &Mesh, skinned: bool) -> Option<&'static str> {
    if skinned {
        return Some("skinned");
    }
    let Some(VertexAttributeValues::Float32x3(points)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { return Some("no positions") };
    let (lo, hi) = points.iter().fold((Vec3::INFINITY, Vec3::NEG_INFINITY), |(lo, hi), p| (lo.min(Vec3::from_array(*p)), hi.max(Vec3::from_array(*p))));
    let known = KNOWN.iter().any(|(k, a, b)| *k == part && (lo - Vec3::from_array(*a)).abs().max_element() <= KNOWN_WITHIN && (hi - Vec3::from_array(*b)).abs().max_element() <= KNOWN_WITHIN);
    (!known).then_some("not of the game's own model")
}

/// Where the needle turns to for `rads`, degrees about its local Z from its bind pose.
pub fn needle_angle(rads: f64) -> f32 {
    (rads.clamp(0.0, NEEDLE_MAX_RADS) - NEEDLE_BIND_RADS) as f32 * NEEDLE_DEGREES_PER_RAD
}

/// Where the tab knob turns to for tab `tab`, degrees about its shaft from its bind pose.
pub fn knob_angle(tab: usize) -> f32 {
    (KNOB_BIND_TAB - tab as i32) as f32 * KNOB_DEGREES_PER_TAB + KNOB_BIND_OFFSET
}

/// Where the wheel turns to for row `row`, degrees about its axle from its bind pose.
pub fn wheel_angle(row: usize) -> f32 {
    row as f32 * WHEEL_DEGREES_PER_ROW
}

/// The knob's rotation in its shape's frame for a turn of `degrees` about its shaft.
fn knob_rotation(degrees: f32) -> Quat {
    Quat::from_axis_angle(Vec3::from_array(KNOB_LEAN_AXIS).normalize(), KNOB_LEAN_DEGREES.to_radians()) * Quat::from_axis_angle(Vec3::from_array(KNOB_SHAFT).normalize(), degrees.to_radians())
}

/// A step of a control towards `target` at `speed` degrees a second, never past it.
fn towards(angle: f32, target: f32, speed: f32, dt: f32) -> f32 {
    angle + (target - angle).clamp(-speed * dt, speed * dt)
}

/// The needle's quiver at `seconds` on the arm, degrees from where it rests ([`NEEDLE_QUIVER`]).
pub fn needle_quiver(seconds: f32) -> f32 {
    NEEDLE_QUIVER.iter().map(|(degrees, hertz)| degrees * (std::f32::consts::TAU * hertz * seconds).sin()).sum()
}

/// The most the quiver takes the needle from where it rests.
#[cfg(test)]
fn needle_quiver_most() -> f32 {
    NEEDLE_QUIVER.iter().map(|(d, _)| d).sum()
}

/// A control's place and where it is going.
#[derive(Clone, Copy, Debug)]
struct Turn {
    angle: f32,
    target: f32,
    /// A reading has come: until then a first one is taken at once.
    read: bool,
    /// Its move has not been said to be over.
    moving: bool,
}

impl Turn {
    fn at(angle: f32) -> Turn {
        Turn { angle, target: angle, read: false, moving: false }
    }

    /// Go towards `target`, if there is a reading; true when it has just come to rest.
    fn advance(&mut self, target: Option<f32>, speed: f32, dt: f32, name: &str) -> bool {
        if let Some(t) = target {
            if !self.read {
                *self = Turn { angle: t, target: t, read: true, moving: true };
            } else if (t - self.target).abs() > f32::EPSILON {
                // Rads creeping up in a radiated place would say this every frame.
                if !self.moving && (t - self.angle).abs() >= 1.0 {
                    info!("pipboy: {name} moving from {:.1} deg to {t:.1} deg", self.angle);
                    self.moving = true;
                }
                self.target = t;
            }
        }
        self.angle = towards(self.angle, self.target, speed, dt);
        let rest = self.moving && self.angle == self.target;
        if rest {
            self.moving = false;
        }
        rest
    }
}

/// The controls' places, which outlive the rig.
#[derive(Resource, Clone, Copy, Debug)]
pub struct ControlAngles {
    needle: Turn,
    knob: Turn,
    wheel: Turn,
    /// The last reading of each, what a control is travelling to.
    last: Readings,
    /// A shape of each was said to be left still: needle, knob, wheel.
    skips_said: [bool; 3],
    /// Seconds the Pip-Boy has been up on the arm, the quiver's clock; `None` while it is
    /// down, when the needle rests.
    up_for: Option<f32>,
    /// The needle quivers ([`NEEDLE_QUIVER`]); tests of the travel turn it off.
    pub quiver: bool,
}

impl Default for ControlAngles {
    fn default() -> Self {
        ControlAngles { needle: Turn::at(needle_angle(0.0)), knob: Turn::at(knob_angle(0)), wheel: Turn::at(wheel_angle(0)), last: Readings::default(), skips_said: [false; 3], up_for: None, quiver: true }
    }
}

/// What the game says the controls should show this frame; `None` keeps a control's place.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Readings {
    pub rads: Option<f64>,
    pub tab: Option<usize>,
    pub row: Option<usize>,
}

impl ControlAngles {
    fn advance(&mut self, r: Readings, dt: f32) {
        // A tab comes with every reading on the arm; none comes down.
        self.up_for = r.tab.map(|_| self.up_for.unwrap_or(0.0) + dt);
        self.last = Readings { rads: r.rads.or(self.last.rads), tab: r.tab.or(self.last.tab), row: r.row.or(self.last.row) };
        if self.needle.advance(r.rads.map(needle_angle), NEEDLE_SPEED, dt, "needle") {
            info!("pipboy: needle settled at {:.1} deg for {:.0} rads", self.needle.angle, self.last.rads.unwrap_or_default());
        }
        if self.knob.advance(r.tab.map(knob_angle), KNOB_SPEED, dt, "tab knob") {
            info!("pipboy: tab knob settled at {:.1} deg (tab {})", self.knob.angle, self.last.tab.unwrap_or_default());
        }
        if self.wheel.advance(r.row.map(wheel_angle), WHEEL_SPEED, dt, "scroll wheel") {
            info!("pipboy: scroll wheel settled at {:.1} deg (row {})", self.wheel.angle, self.last.row.unwrap_or_default());
        }
    }

    /// Say once for each control that a shape of it is left still.
    fn say_skip(&mut self, part: PipBoyPart, why: &str) {
        let (i, name) = match part {
            PipBoyPart::Needle => (0, "needle"),
            PipBoyPart::TabKnob => (1, "tab knob"),
            _ => (2, "scroll wheel"),
        };
        if !self.skips_said[i] {
            warn!("pipboy: {name} left still: its shape is {why}");
            self.skips_said[i] = true;
        }
    }

    /// The rotation (in the shape's own space) of a part, if it is a control: the needle with
    /// its quiver while the Pip-Boy is up.
    fn of(&self, part: PipBoyPart) -> Option<Quat> {
        match part {
            PipBoyPart::Needle => Some(Quat::from_rotation_z((self.needle.angle + self.up_for.filter(|_| self.quiver).map_or(0.0, needle_quiver)).to_radians())),
            PipBoyPart::TabKnob => Some(knob_rotation(self.knob.angle)),
            PipBoyPart::ScrollKnob => Some(Quat::from_rotation_x(self.wheel.angle.to_radians())),
            _ => None,
        }
    }
}

/// A control's own transform as the rig spawned it; `None` for a shape left still.
#[derive(Component, Clone, Copy, Debug)]
pub struct ControlBind(Option<Transform>);

type ControlShapes<'w, 's> = Query<'w, 's, (Entity, &'static PipBoyPart, &'static mut Transform, &'static PartMesh, Option<&'static SkinnedMesh>, Option<&'static ControlBind>)>;

/// `bind × rotation`, in the shape's own frame, through its origin.
fn posed(bind: &Transform, rotation: Quat) -> Transform {
    bind.mul_transform(Transform::from_rotation(rotation))
}

/// Move the controls on and pose every control of the rig from its own bind.
fn pose_controls(commands: &mut Commands, r: Readings, dt: f32, angles: &mut ControlAngles, shapes: &mut ControlShapes) {
    angles.advance(r, dt);
    for (e, part, mut t, mesh, skin, bind) in shapes {
        let Some(rotation) = angles.of(*part) else { continue };
        let bind = match bind {
            Some(b) => b.0,
            None => {
                // First seen: what it has now is the rig's. A rebuilt rig comes this way.
                let why = unsupported(*part, &mesh.0, skin.is_some());
                if let Some(why) = why {
                    angles.say_skip(*part, why);
                }
                let bind = why.is_none().then_some(*t);
                if bind.is_some() {
                    let (x, y, z) = t.rotation.to_euler(EulerRot::XYZ);
                    let bounds = match mesh.0.attribute(Mesh::ATTRIBUTE_POSITION) {
                        Some(VertexAttributeValues::Float32x3(p)) => {
                            let (lo, hi) = p.iter().fold((Vec3::INFINITY, Vec3::NEG_INFINITY), |(lo, hi), v| (lo.min(Vec3::from_array(*v)), hi.max(Vec3::from_array(*v))));
                            format!("{} vertices, bounds {lo:?} to {hi:?}, first {:?}", p.len(), p.first())
                        }
                        _ => "no positions".to_string(),
                    };
                    info!("pipboy: {part:?} bound at {:?}, rotation XYZ ({:.1}, {:.1}, {:.1}) deg, scale {:?}; mesh {bounds}", t.translation, x.to_degrees(), y.to_degrees(), z.to_degrees(), t.scale);
                }
                commands.entity(e).insert(ControlBind(bind));
                bind
            }
        };
        let Some(bind) = bind else { continue };
        let want = posed(&bind, rotation);
        if *t != want {
            *t = want;
        }
    }
}

/// The row of the list the page shows; `None` where it has none (the maps).
fn list_row(pip: &PipBoy, stats: Option<&StatsView>, data: Option<&DataState>, active: &ActiveMenu,
    inventory: Option<&nvf_play::tiles::inventory_menu::InventoryTiles>, map: Option<&nvf_play::tiles::map_menu::MapTiles>,
    items: Option<&nvf_play::pipboy::scrollbar::ItemsScroll>) -> Option<usize> {
    #[cfg(target_os = "android")]
    {
        list_row_scroll(pip, stats, data, active, inventory, map, items)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (inventory, map, items);
        let tab = pip.tab();
        match pip.page {
            Page::Stats => stats?.selected.get(tab).copied(),
            Page::Items => active.0.as_ref().map(|m| m.selected[Side::Player as usize]),
            Page::Data if tab >= 2 => Some(data?.selection(tab).0),
            Page::Data => None,
        }
    }
}

#[cfg(any(target_os = "android", test))]
fn list_row_scroll(pip: &PipBoy, stats: Option<&StatsView>, data: Option<&DataState>, active: &ActiveMenu,
    inventory: Option<&nvf_play::tiles::inventory_menu::InventoryTiles>, map: Option<&nvf_play::tiles::map_menu::MapTiles>,
    items: Option<&nvf_play::pipboy::scrollbar::ItemsScroll>) -> Option<usize> {
    let tab = pip.tab();
    match pip.page {
        Page::Stats => stats?.top.get(tab).copied(),
        Page::Items => inventory.filter(|t| t.shown.is_some() && !t.others).map(|t| t.view.value(Side::Player).max(0.0) as usize)
            .or_else(|| active.0.as_ref().and_then(|m| items.filter(|s| s.matches(m)).and_then(|s| s.position()).or(Some(m.scroll[Side::Player as usize])))),
        Page::Data if tab >= 2 => map.filter(|t| t.shown.is_some() && !t.others).and_then(|t| t.list_scroll()).map(|v| v.max(0.0) as usize)
            .or_else(|| data.and_then(|d| d.list_scroll(tab))),
        Page::Data => None,
    }
}

/// The player's rads as the STATS page shows them, without the wear of apparel, which has
/// none on them.
fn rads(script: Option<Mut<ScriptState>>) -> Option<f64> {
    let mut script = script?;
    // Reading needs the game mutably; nothing is changed.
    let game = &mut script.bypass_change_detection().rt.as_mut()?.game;
    let [base, modifier, damage] = fnv_game::functions::actor_value_parts(game, FormId::PLAYER_REF, RADS).ok()?;
    let temporary = fnv_game::functions::actor_value_temporary(game, FormId::PLAYER_REF, RADS);
    Some((base + modifier + temporary - damage).max(0.0))
}

/// The needle and knobs: read the game while the Pip-Boy shows on the arm, then pose.
type ControlLists<'w> = (Option<Res<'w,StatsView>>, Option<Res<'w,DataState>>, Option<Res<'w,nvf_play::tiles::inventory_menu::InventoryTiles>>, Option<Res<'w,nvf_play::tiles::map_menu::MapTiles>>, Option<Res<'w,nvf_play::pipboy::scrollbar::ItemsScroll>>);
#[allow(clippy::too_many_arguments)]
pub(crate) fn turn_controls(
    mut commands: Commands,
    time: Res<Time<Real>>,
    pip: Res<PipBoy>,
    (stats, data, inventory, map, items): ControlLists,
    active: Res<ActiveMenu>,
    script: Option<ResMut<ScriptState>>,
    mut angles: ResMut<ControlAngles>,
    mut shapes: ControlShapes,
) {
    let readings = if pip.on_arm() {
        Readings { rads: rads(script.map(Into::into)), tab: Some(pip.tab()), row: list_row(&pip, stats.as_deref(), data.as_deref(), &active, inventory.as_deref(), map.as_deref(), items.as_deref()) }
    } else {
        Readings::default()
    };
    pose_controls(&mut commands, readings, time.delta_secs(), &mut angles, &mut shapes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_needle_follows_total_rads_linearly_within_the_dial() {
        assert_eq!(needle_angle(500.0), 0.0);
        assert!((needle_angle(0.0) + 90.0).abs() < 1e-3, "straight down");
        assert!((needle_angle(1000.0) - 90.0).abs() < 1e-3, "straight up");
        assert!((needle_angle(300.0) + 36.0).abs() < 1e-3);
        assert_eq!(needle_angle(-5.0), needle_angle(0.0));
        assert_eq!(needle_angle(5000.0), needle_angle(1000.0));
    }

    #[test]
    fn the_knob_has_five_detents_a_step_apart() {
        assert_eq!([0, 1, 2, 3, 4].map(knob_angle), [66.0, 38.0, 10.0, -18.0, -46.0]);
    }

    #[test]
    fn a_move_goes_at_its_speed_and_stops_on_the_target() {
        assert_eq!(towards(0.0, 90.0, 30.0, 1.0), 30.0);
        assert_eq!(towards(0.0, -90.0, 30.0, 1.0), -30.0);
        assert_eq!(towards(80.0, 90.0, 30.0, 1.0), 90.0);
        assert_eq!(towards(90.0, 90.0, 30.0, 1.0), 90.0);
    }

    #[test]
    fn the_first_reading_is_taken_at_once_then_the_needle_travels_and_off_the_arm_it_stays() {
        let mut a = ControlAngles::default();
        let up = |rads| Readings { rads: Some(rads), tab: Some(0), row: Some(0) };
        a.advance(up(300.0), 0.01);
        assert_eq!(a.needle.angle, needle_angle(300.0), "a loaded game's needle does not sweep");
        // 600 more rads: 108° at its speed.
        a.advance(up(900.0), 1.0);
        assert!((a.needle.angle - (needle_angle(300.0) + NEEDLE_SPEED)).abs() < 1e-3);
        // Down: it finishes travelling to the last reading and holds there, which the log
        // still gives (not 0 rads).
        for _ in 0..10 {
            a.advance(Readings::default(), 1.0);
        }
        assert_eq!(a.needle.angle, needle_angle(900.0));
        assert_eq!(a.last.rads, Some(900.0));
        // A drop travels the other way at the same speed, however far.
        a.advance(up(0.0), 1.0);
        assert!((a.needle.angle - (needle_angle(900.0) - NEEDLE_SPEED)).abs() < 1e-3);
        // A row and a tab move their own control only.
        let before = a;
        a.advance(Readings { rads: None, tab: Some(4), row: None }, 0.1);
        assert_ne!(a.knob.angle, before.knob.angle);
        assert_eq!(a.wheel.angle, before.wheel.angle);
    }

    #[derive(Resource, Default)]
    struct Now(Readings, f32);

    fn box_mesh(lo: [f32; 3], hi: [f32; 3]) -> std::sync::Arc<Mesh> {
        let mut m = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default());
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![lo, hi]);
        std::sync::Arc::new(m)
    }

    /// The measured shape of a control.
    fn known(part: PipBoyPart) -> PartMesh {
        let (_, lo, hi) = KNOWN.iter().find(|(k, ..)| *k == part).unwrap();
        PartMesh(box_mesh(*lo, *hi))
    }

    fn app() -> App {
        let mut app = App::new();
        app.insert_resource(ControlAngles { quiver: false, ..default() }).init_resource::<Now>().add_systems(
            Update,
            |mut commands: Commands, now: Res<Now>, mut angles: ResMut<ControlAngles>, mut shapes: ControlShapes| {
                pose_controls(&mut commands, now.0, now.1, &mut angles, &mut shapes);
            },
        );
        app
    }

    fn bind() -> Transform {
        Transform::from_xyz(6.5, 3.2, 3.7).with_rotation(Quat::from_rotation_y(0.3))
    }

    fn tip(app: &App, e: Entity, local: Vec3) -> Vec3 {
        app.world().get::<Transform>(e).unwrap().transform_point(local)
    }

    #[test]
    fn a_shape_turns_about_its_own_origin_and_axis_absolutely() {
        let mut app = app();
        let needle = app.world_mut().spawn((PipBoyPart::Needle, bind(), known(PipBoyPart::Needle))).id();
        let wheel = app.world_mut().spawn((PipBoyPart::ScrollKnob, bind(), known(PipBoyPart::ScrollKnob))).id();
        let other = app.world_mut().spawn((PipBoyPart::Screen, bind(), known(PipBoyPart::Needle))).id();
        app.world_mut().resource_mut::<Now>().0 = Readings { rads: Some(500.0), tab: Some(3), row: Some(0) };
        for _ in 0..5 {
            app.update();
        }
        // At the bind's own value, nothing has moved, however many frames.
        assert_eq!(*app.world().get::<Transform>(needle).unwrap(), bind());
        app.world_mut().resource_mut::<Now>().0 = Readings { rads: Some(1000.0), tab: Some(3), row: Some(13) };
        app.world_mut().resource_mut::<Now>().1 = 100.0;
        for _ in 0..5 {
            app.update();
        }
        // The tip along local X is now where local Y was: 90° about Z, once, not five times.
        let want = bind().transform_point(Vec3::Y);
        assert!((tip(&app, needle, Vec3::X) - want).length() < 1e-4);
        assert_eq!(app.world().get::<Transform>(needle).unwrap().translation, bind().translation, "about its origin");
        // The wheel about X: 13 rows are half its 26 ridges, 180°: its Y to −Y, the axle staying.
        assert!((tip(&app, wheel, Vec3::Y) - bind().transform_point(Vec3::NEG_Y)).length() < 1e-4);
        assert!((tip(&app, wheel, Vec3::X) - bind().transform_point(Vec3::X)).length() < 1e-4);
        assert_eq!(*app.world().get::<Transform>(other).unwrap(), bind(), "not a control");
    }

    #[test]
    fn the_knob_turns_about_its_leaned_shaft_and_stays_on_it() {
        let shaft = Vec3::from_array(KNOB_SHAFT).normalize();
        let b = bind();
        let at = |tab: usize| posed(&b, knob_rotation(knob_angle(tab)));
        // The leaned shaft stays where it is at every tab, its origin too; a point off it goes round.
        let leaned = knob_rotation(0.0) * shaft;
        for tab in 1..5 {
            assert!((at(tab).transform_point(shaft) - at(0).transform_point(shaft)).length() < 1e-5, "tab {tab}");
            assert_eq!(at(tab).translation, b.translation, "through its origin");
            assert!((at(tab).transform_point(Vec3::X) - at(0).transform_point(Vec3::X)).length() > 0.1, "tab {tab}");
        }
        // It leans by the lean, whatever the turn.
        assert!((leaned.angle_between(shaft).to_degrees() - KNOB_LEAN_DEGREES).abs() < 1.0, "{leaned}");
    }

    #[test]
    fn the_wheel_turns_a_ridge_a_row() {
        assert!((wheel_angle(26) - 360.0).abs() < 1e-4, "26 ridges round");
        assert!((wheel_angle(1) - 13.846).abs() < 1e-2);
    }

    #[test]
    fn actual_model_wheel_transform_follows_view_scroll_instead_of_selected_row() {
        let mut pip=PipBoy::default();pip.page=Page::Stats;pip.tabs[0]=2;
        // These are distinct authoritative view fields: the selection can remain offscreen.
        let mut stats=StatsView::default();stats.top[2]=3;stats.selected[2]=11;
        let row=list_row_scroll(&pip,Some(&stats),None,&ActiveMenu::default(),None,None,None);
        assert_eq!(row,Some(3));
        #[cfg(not(target_os = "android"))]
        assert_eq!(list_row(&pip,Some(&stats),None,&ActiveMenu::default(),None,None,None),Some(11));
        let mut app=app();
        let wheel=app.world_mut().spawn((PipBoyPart::ScrollKnob,bind(),known(PipBoyPart::ScrollKnob))).id();
        app.world_mut().resource_mut::<Now>().0=Readings{row,..default()};
        app.world_mut().resource_mut::<Now>().1=100.0;
        app.update();app.update();
        let want=bind().transform_point(Quat::from_rotation_x(wheel_angle(3).to_radians())*Vec3::Y);
        let wrong=bind().transform_point(Quat::from_rotation_x(wheel_angle(11).to_radians())*Vec3::Y);
        assert!((tip(&app,wheel,Vec3::Y)-want).length()<1e-4,"the actual rig shape poses from current list scroll");
        assert!((tip(&app,wheel,Vec3::Y)-wrong).length()>0.1,"selected row cannot drive the scrollbar wheel");
    }

    #[test]
    fn the_needle_quivers_a_little_on_the_arm_and_rests_down() {
        // Within its amount, never still from one frame to the next, and nothing in the
        // travel angle or the log's numbers.
        let most = needle_quiver_most();
        assert!(most > 0.5 && most < 5.0, "{most}");
        let mut seen = Vec::new();
        for i in 0..60 {
            let q = needle_quiver(i as f32 / 60.0);
            assert!(q.abs() <= most + 1e-4);
            seen.push(q);
        }
        assert!(seen.windows(2).all(|w| (w[0] - w[1]).abs() > 1e-4), "it moves every frame");
        assert!(seen.iter().any(|&q| q > 0.3) && seen.iter().any(|&q| q < -0.3), "both ways");
        let mut a = ControlAngles::default();
        a.advance(Readings { rads: Some(500.0), tab: Some(0), row: Some(0) }, 0.1);
        assert_eq!(a.needle.angle, 0.0, "the travel angle is the reading's");
        assert_eq!(a.up_for, Some(0.1));
        let shown = a.of(PipBoyPart::Needle).unwrap();
        assert!(shown.angle_between(Quat::from_rotation_z(needle_quiver(0.1).to_radians())) < 1e-3 && shown != Quat::IDENTITY);
        assert_eq!(a.of(PipBoyPart::TabKnob).unwrap(), knob_rotation(knob_angle(0)), "the knob does not quiver");
        // Down: no reading, the clock is dropped and the needle rests on its angle.
        a.advance(Readings::default(), 0.1);
        assert_eq!(a.up_for, None);
        assert_eq!(a.of(PipBoyPart::Needle).unwrap(), Quat::IDENTITY);
        // Up again, the clock starts afresh.
        a.advance(Readings { rads: Some(500.0), tab: Some(0), row: Some(0) }, 0.05);
        assert_eq!(a.up_for, Some(0.05));
    }

    #[test]
    fn a_rebuilt_rig_takes_its_own_bind_and_the_control_its_place() {
        let mut app = app();
        let needle = app.world_mut().spawn((PipBoyPart::Needle, bind(), known(PipBoyPart::Needle))).id();
        app.world_mut().resource_mut::<Now>().0 = Readings { rads: Some(1000.0), ..default() };
        app.update();
        app.update();
        // A new rig elsewhere, down: the place kept, on the new bind.
        app.world_mut().despawn(needle);
        let other_bind = Transform::from_xyz(1.0, 2.0, 3.0);
        let rebuilt = app.world_mut().spawn((PipBoyPart::Needle, other_bind, known(PipBoyPart::Needle))).id();
        app.world_mut().resource_mut::<Now>().0 = Readings::default();
        app.update();
        app.update();
        let t = *app.world().get::<Transform>(rebuilt).unwrap();
        assert_eq!(t.translation, other_bind.translation);
        assert!((t.transform_point(Vec3::X) - other_bind.transform_point(Vec3::Y)).length() < 1e-4, "at 1000 rads still");
    }

    #[test]
    fn only_the_measured_rigid_shapes_turn() {
        // Both of the needle's shapes, the knob and the wheel are known, in either arm.
        for (part, lo, hi) in KNOWN {
            assert_eq!(unsupported(part, &box_mesh(lo, hi), false), None, "{part:?}");
        }
        assert_eq!(unsupported(PipBoyPart::ScrollKnob, &box_mesh([-0.144, -1.202, -1.195], [0.159, 1.202, 1.195]), false), None, "the female arm's");
        assert_eq!(unsupported(PipBoyPart::TabKnob, &box_mesh([-0.418, -0.492, -0.19], [0.411, 0.404, 0.606]), true), Some("skinned"));
        // The right size for another control is not enough.
        assert!(unsupported(PipBoyPart::Needle, &box_mesh([-0.418, -0.492, -0.19], [0.411, 0.404, 0.606]), false).is_some());
        let mut app = app();
        let replaced = app.world_mut().spawn((PipBoyPart::Needle, bind(), PartMesh(box_mesh([-2.0; 3], [2.0; 3])))).id();
        let skinned = app.world_mut().spawn((PipBoyPart::TabKnob, bind(), known(PipBoyPart::TabKnob), SkinnedMesh { inverse_bindposes: Handle::default(), joints: Vec::new() })).id();
        let also = app.world_mut().spawn((PipBoyPart::Needle, bind(), PartMesh(box_mesh([-3.0; 3], [3.0; 3])))).id();
        app.world_mut().resource_mut::<Now>().0 = Readings { rads: Some(1000.0), tab: Some(0), row: Some(0) };
        for _ in 0..3 {
            app.update();
        }
        for e in [replaced, skinned, also] {
            assert_eq!(*app.world().get::<Transform>(e).unwrap(), bind(), "left as it is");
            assert!(app.world().get::<ControlBind>(e).unwrap().0.is_none());
        }
        assert_eq!(app.world().resource::<ControlAngles>().skips_said, [true, true, false], "said once for each control");
    }
}
