//! Fast travel from the World Map (docs/spec-notes/fast-travel.md).
//!
//! - A click on a marker of the World Map, or the controller's A on one (or the test
//!   aid's `travel=`) is a [`FastTravelAsk`]; [`answer`] tries the refusals (§2,
//!   [`refusal`]) and puts up the
//!   question box ([`Popup`]): "Travel to …?" with Yes and No, or the
//!   refusal's text with OK. The DATA page draws the box over the map and takes its keys
//!   and clicks (`super::take_input`); Yes is a [`FastTravelGo`].
//! - [`go`] puts the Pip-Boy away and keeps the trip; once the game runs again
//!   ([`depart`]) the trip goes out as a door's does (`crate::loading::TravelRequest`),
//!   the clock moves on by the travel time ([`travel_hours`]) and the player's teammates
//!   come along (`fnv_game::Game::bring_teammates`).

use super::{read, DataState};
use crate::hitvol::HitVolumes;
use crate::loading::TravelRequest;
use crate::menu::model::Command;
use crate::menu::MenuInput;
use crate::music::MusicCues;
use crate::phys::PhysState;
use crate::script::ScriptState;
use crate::sim::SimState;
use crate::wait::{HourPassed, HOSTILE_DISTANCE};
use crate::{Location, PlayState, PlayWorld, Travel};
use bevy::prelude::*;
use fnv_sim::settings::Setting;
use fnv_sim::FormId;

/// A click on a map marker of the World Map, or the controller's A on one (or the test
/// aid's `travel=`): ask to go there.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct FastTravelAsk(pub u32);

/// The player answered Yes: travel to this marker.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct FastTravelGo(pub u32);

/// Why the player cannot travel now (fast-travel.md §2), in the order they are tried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Undiscovered,
    Script,
    Place,
    InAir,
    Combat,
    HostilesNear,
    Encumbered,
    /// The player's No Travel on a Crippled Leg tweak [design: docs/design/tweaks.md].
    CrippledLeg,
    /// The player's Fast Travel Costs a Nuka-Cola tweak, with no bottle carried [design: docs/design/tweaks.md].
    NoBottle,
}

impl Refusal {
    pub const ALL: [Refusal; 9] = [Refusal::Undiscovered, Refusal::Script, Refusal::Place, Refusal::InAir, Refusal::Combat, Refusal::HostilesNear, Refusal::Encumbered, Refusal::CrippledLeg, Refusal::NoBottle];

    /// The game setting holding the refusal's text [documented: GECK wiki, game settings
    /// list].
    pub fn setting(self) -> &'static str {
        match self {
            Refusal::Undiscovered => "sNoFastTravelUndiscovered",
            Refusal::Script => "sNoFastTravelScriptBlock",
            Refusal::Place => "sNoFastTravelCell",
            Refusal::InAir => "sNoFastTravelInAir",
            Refusal::Combat => "sNoFastTravelCombat",
            Refusal::HostilesNear => "sNoFastTravelHostileActorsNear",
            Refusal::Encumbered => "sNoFastTravelOverencumbered",
            // No game setting holds these: NVForever's words only.
            Refusal::CrippledLeg | Refusal::NoBottle => "",
        }
    }

    /// NVForever's own text where no plugin sets the setting [design: our own words,
    /// #2984]; the script one is FalloutNV.esm's `sNoFastTravelScriptBlock` [observed].
    fn own_text(self) -> &'static str {
        match self {
            Refusal::Undiscovered => "That place has not been found yet.",
            Refusal::Script => "Fast travel is currently unavailable from this location.",
            Refusal::Place => "Fast travel cannot start from here.",
            Refusal::InAir => "Fast travel needs ground under your feet.",
            Refusal::Combat => "Fast travel waits until the fight is over.",
            Refusal::HostilesNear => "Fast travel cannot start with enemies close by.",
            Refusal::Encumbered => "Too much weight to fast travel.",
            Refusal::CrippledLeg => "Fast travel is not possible on a crippled leg.",
            Refusal::NoBottle => "Fast travel needs a Nuka-Cola Quantum, Quartz or Victory.",
        }
    }
}

/// What the place and the player are like when a travel is asked for.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Checks {
    /// The marker is a travel target (found, its Can Travel To flag, ShowMap's or
    /// ShowAllMapMarkers' canTravel).
    pub target: bool,
    /// EnableFastTravel 0 holds.
    pub script_block: bool,
    /// The place forbids it: an interior, or a worldspace flagged against it
    /// (`fnv_game::fast_travel::place_forbids`).
    pub place: bool,
    pub in_air: bool,
    pub combat: bool,
    pub hostiles_near: bool,
    pub encumbered: bool,
    /// A leg is crippled and the tweak refuses travel then.
    pub crippled_leg: bool,
    /// The Nuka-Cola tweak is on and the player carries none of its bottles.
    pub no_bottle: bool,
}

/// The player's fast-travel tweaks [design: docs/design/tweaks.md]: No Fast Travel
/// refuses as a script's block does; the others drop their refusal.
pub fn tweak_checks(c: &mut Checks, t: &crate::tweaks::TweakSettings) {
    c.script_block |= t.disable_fast_travel;
    c.in_air &= !t.midair_fast_travel;
    c.hostiles_near &= !t.fast_travel_enemies_nearby;
    c.encumbered &= !t.fast_travel_overencumbered;
    c.place &= !t.game.unsafe_fast_travel;
}

/// The first refusal that applies, if any.
pub fn refusal(c: &Checks) -> Option<Refusal> {
    let applies = |r: Refusal| match r {
        Refusal::Undiscovered => !c.target,
        Refusal::Script => c.script_block,
        Refusal::Place => c.place,
        Refusal::InAir => c.in_air,
        Refusal::Combat => c.combat,
        Refusal::HostilesNear => c.hostiles_near,
        Refusal::Encumbered => c.encumbered,
        Refusal::CrippledLeg => c.crippled_leg,
        Refusal::NoBottle => c.no_bottle,
    };
    Refusal::ALL.into_iter().find(|&r| applies(r))
}

/// The bottles the Fast Travel Costs a Nuka-Cola tweak takes, by editor ID [observed:
/// FalloutNV.esm] [design: docs/design/tweaks.md].
const TRAVEL_BOTTLES: [&str; 3] = ["NukaColaQuantum", "NukaColaQuartz", "NukaColaVictory"];

/// One of [`TRAVEL_BOTTLES`] the player carries.
fn travel_bottle(sim: &mut SimState) -> Option<FormId> {
    let bottles: Vec<FormId> = TRAVEL_BOTTLES.iter().filter_map(|e| sim.forms.by_edid(e)).collect();
    bottles.into_iter().find(|&b| sim.world.item_count(FormId::PLAYER_REF, b).is_ok_and(|n| n > 0))
}

/// The question [design: NVForever's own words, #2984]; a plugin's `sTravelQuestion` with `%s` takes the name there, one
/// without it is followed by the name [design].
pub const OWN_QUESTION: &str = "Travel to %s?";

pub fn question(template: &str, name: &str) -> String {
    if template.contains("%s") {
        template.replacen("%s", name, 1)
    } else {
        format!("{} {name}?", template.trim_end().trim_end_matches('?'))
    }
}

/// The trip's game hours: the straight line walked at `walk` units a second at
/// `time_scale` game seconds a real second [hypothesis, fast-travel.md §4]; none within
/// a cell's width [design].
pub fn travel_hours(distance: f32, walk: f32, time_scale: f64) -> f64 {
    if distance <= super::model::CELL || walk <= 0.0 {
        return 0.0;
    }
    distance as f64 / walk as f64 * time_scale / 3600.0
}

/// The game's TimeScale when the plugins lack the global [documented: GECK wiki
/// TimeScale, default 30].
const OWN_TIME_SCALE: f64 = 30.0;

/// What the question box asks.
#[derive(Debug, Clone, PartialEq)]
pub enum Ask {
    /// "Travel to …?": the marker.
    Travel(u32),
    /// A refusal: OK only.
    Refused,
    /// Place Marker's question (`super::marker`, #1954).
    Marker(super::marker::MarkerAsk),
}

/// The question box over the map: fast travel's on the World Map, Place Marker's on
/// either map.
#[derive(Debug, Clone, PartialEq)]
pub struct Popup {
    pub ask: Ask,
    pub text: String,
    pub buttons: Vec<String>,
    pub selected: usize,
}

impl Popup {
    pub fn travel(marker: u32, text: String) -> Popup {
        Popup { ask: Ask::Travel(marker), text, buttons: vec!["Yes".into(), "No".into()], selected: 0 }
    }

    pub fn refused(text: String) -> Popup {
        Popup { ask: Ask::Refused, text, buttons: vec!["OK".into()], selected: 0 }
    }

    /// Place Marker's question (`super::marker::Words`), the first answer chosen.
    pub fn marker(ask: super::marker::MarkerAsk, text: String, buttons: Vec<String>) -> Popup {
        Popup { ask: Ask::Marker(ask), text, buttons, selected: 0 }
    }

    /// Move the choice, stopping at the ends.
    pub fn step(&mut self, by: i32) {
        self.selected = (self.selected as i32 + by).clamp(0, self.buttons.len() as i32 - 1) as usize;
    }

    /// The answer `i` was taken: the marker to travel to, if it was Yes.
    pub fn take(&self, i: usize) -> Option<u32> {
        match self.ask {
            Ask::Travel(m) if i == 0 => Some(m),
            _ => None,
        }
    }
}

fn setting_str(sim: &SimState, name: &str) -> Option<String> {
    match sim.forms.settings.get(&name.to_ascii_lowercase()) {
        Some(Setting::Str(s)) if !s.trim().is_empty() => Some(s.clone()),
        _ => None,
    }
}

fn setting_f32(sim: &SimState, name: &str) -> Option<f32> {
    match sim.forms.settings.get(&name.to_ascii_lowercase()) {
        Some(Setting::Float(v)) => Some(*v),
        _ => None,
    }
}

/// Whether the player may travel to `m` by the scripts' and the markers' word: found,
/// flagged Can Travel To, or made a target by ShowMap or ShowAllMapMarkers.
fn travel_target(st: &DataState, script: Option<&ScriptState>, m: &read::Marker) -> bool {
    if let Some(rt) = script.and_then(|s| s.rt.as_ref()) {
        let id = FormId(m.form);
        if rt.game.marker_flags(id).is_some() {
            if rt.game.marker_flag_override(id, fnv_game::nvse_lu2::MARKER_TRAVEL) == Some(false) {
                return false;
            }
            return st.discovered.contains(&m.form) || rt.game.marker_travel(id);
        }
    }
    st.discovered.contains(&m.form) || (m.flags & 3 == 3)
}

/// What the refusals look at: the player's body, where they are, the actors near, the
/// music's battle test and the plugins.
type Around<'w> = (Res<'w, PhysState>, Res<'w, Location>, Res<'w, HitVolumes>, Option<Res<'w, MusicCues>>, Res<'w, PlayWorld>);

/// Answer the asks: put up the question, or the refusal.
pub fn answer(
    mut asks: MessageReader<FastTravelAsk>,
    state: Option<ResMut<DataState>>,
    mut sim: ResMut<SimState>,
    mut script: Option<ResMut<ScriptState>>,
    (phys, location, actors, cues, world): Around,
    tweaks: Option<Res<crate::tweaks::TweakSettings>>,
    limbs: Option<Res<crate::combat::PlayerLimbs>>,
) {
    let Some(&FastTravelAsk(marker)) = asks.read().last() else { return };
    let Some(mut st) = state else { return };
    // The World Map reads the markers when first drawn; an ask from the test aid can come
    // before the map ever was.
    if st.markers.is_none() {
        st.markers = Some(read::markers(world.lo, world.schema, sim.refs));
    }
    let Some(m) = st.markers.iter().flatten().find(|m| m.form == marker).cloned() else {
        warn!("fast travel: {} is not a named map marker", FormId(marker));
        return;
    };
    let indoors = location.interior.is_some();
    let player = phys.player.as_ref();
    let feet = player.map(|c| c.feet);
    let outdoors = (!indoors).then_some(location.world);
    let cell = location.interior.or_else(|| sim.refs.world_cell_at(FormId(location.world), feet?).map(|c| c.0));
    let place = match script.as_ref().and_then(|s| s.rt.as_ref()) {
        // As the scripts left the flags (SetCellFlag, SetWorldspaceFlag).
        Some(rt) => rt.game.place_forbids_travel(cell.map(FormId), outdoors.map(FormId)),
        None => fnv_game::fast_travel::place_forbids(
            cell.and_then(|c| world.index.cells.get(&c)).map_or(0, |c| c.data_flags.into()),
            outdoors.map(|w| read::world_flags(world.lo, world.schema, w).into()),
        ),
    };
    let distance = setting_f32(&sim, ["fHostileActorExteriorDistance", "fHostileActorInteriorDistance"][usize::from(indoors)]).unwrap_or(HOSTILE_DISTANCE[usize::from(indoors)]);
    let script_block = script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.player.fast_travel.travel_disabled);
    let mut hostile = None;
    if let (Some(rt), Some(feet)) = (script.as_mut().and_then(|s| s.rt.as_mut()), feet) {
        let near: Vec<u32> = actors.living.iter().filter(|(_, p)| (0..3).map(|k| (p[k] - feet[k]).powi(2)).sum::<f32>() <= distance * distance).map(|(id, _)| *id).collect();
        hostile = crate::script::with_world(rt, &mut sim, |rt| {
            near.iter().copied().find(|&a| fnv_game::rest::hostile_to_player(&mut rt.game, FormId(a))).map(|a| rt.game.data.name(FormId(a)))
        });
    }
    // Long Haul's Has Fast Travel Always: over-encumbered does not stop it
    // (docs/spec-notes/perk-entry-points.md §7).
    let over = sim.world.encumbrance(FormId::PLAYER_REF).is_ok_and(|e| e.over) && !crate::combat::god_mode();
    let long_haul = over
        && script.as_mut().and_then(|s| s.rt.as_mut()).is_some_and(|rt| crate::script::with_world(rt, &mut sim, |rt| rt.game.player_has_entry(fnv_game::perks::HAS_FAST_TRAVEL_ALWAYS)));
    if long_haul {
        info!("fast travel: over-encumbered, but Has Fast Travel Always (Long Haul) lets the player travel");
    }
    let encumbered = over && !long_haul;
    let tw = tweaks.as_deref().cloned().unwrap_or_default();
    let mut checks = Checks {
        target: travel_target(&st, script.as_deref(), &m),
        script_block,
        place,
        // In water the player is not falling [hypothesis].
        in_air: player.is_some_and(|c| !c.grounded && !c.swimming),
        combat: cues.is_some_and(|c| c.combat),
        hostiles_near: hostile.is_some(),
        encumbered,
        crippled_leg: tw.no_travel_crippled_leg && limbs.as_ref().is_some_and(|l| l.limping()),
        no_bottle: tw.game.travel_costs_bottle && travel_bottle(&mut sim).is_none(),
    };
    tweak_checks(&mut checks, &tw);
    // The player's tweak: an undiscovered marker refuses quietly [design: docs/design/tweaks.md].
    if tw.quiet_undiscovered && refusal(&checks) == Some(Refusal::Undiscovered) {
        info!("fast travel: to {} refused quietly (undiscovered, tweak)", FormId(marker));
        return;
    }
    st.popup = Some(match refusal(&checks) {
        Some(r) => {
            let text = setting_str(&sim, r.setting()).unwrap_or_else(|| r.own_text().to_string());
            let why = match (r, &hostile) {
                (Refusal::HostilesNear, Some(n)) => format!("{n} is hostile and within {distance:.0} units"),
                _ => format!("{r:?}"),
            };
            info!("fast travel: to {} ({}) refused ({why}); the box says {text:?}", m.name, FormId(marker));
            Popup::refused(text)
        }
        None => {
            let text = question(&setting_str(&sim, "sTravelQuestion").unwrap_or_else(|| OWN_QUESTION.to_string()), &m.name);
            info!("fast travel: asks {text:?} ({})", FormId(marker));
            Popup::travel(marker, text)
        }
    });
}

/// A trip waiting for the Pip-Boy to go away.
#[derive(Resource, Default)]
pub struct Pending(Option<Trip>);

/// A short fast travel under the player's Improved Weather tweak: the sky keeps its weather
/// through the next change it would make (nvforever `sky`), then clears this.
#[derive(Resource, Default, Debug)]
pub struct WeatherHold(pub bool);

#[derive(Debug, Clone, Copy)]
struct Trip {
    travel: Travel,
    /// How far the trip goes, units (the player's Improved Weather tweak).
    distance: f32,
    /// The destination reference (the marker's linked one, or the marker) and its cell.
    to: u32,
    cell: Option<u32>,
    rot: [f32; 3],
    hours: f64,
}

/// Yes: work out where the player arrives and how long it takes; put the Pip-Boy away.
#[allow(clippy::too_many_arguments)]
pub fn go(
    mut goes: MessageReader<FastTravelGo>,
    state: Option<Res<DataState>>,
    mut sim: ResMut<SimState>,
    phys: Res<PhysState>,
    location: Res<Location>,
    mut pending: ResMut<Pending>,
    mut menu: MessageWriter<MenuInput>,
    tweaks: Option<Res<crate::tweaks::TweakSettings>>,
) {
    let Some(&FastTravelGo(marker)) = goes.read().last() else { return };
    let Some(m) = state.as_ref().and_then(|st| st.markers.iter().flatten().find(|m| m.form == marker).cloned()) else { return };
    let placed = sim.refs.refs.get(&FormId(marker));
    // The marker's linked reference is where the player arrives [documented: GECK wiki
    // SetOnFastTravelEventHandler], else the marker [hypothesis].
    let dest = placed.and_then(|r| r.linked).and_then(|l| sim.refs.refs.get(&l)).or(placed);
    let Some(dest) = dest else {
        warn!("fast travel: {} has no placed reference", FormId(marker));
        return;
    };
    let world = dest.cell.and_then(|c| sim.refs.cell_world.get(&c)).map(|w| w.0);
    let interior = if world.is_some() { None } else { dest.cell.map(|c| c.0) };
    // Where the walk starts: the player's feet outdoors, indoors where they were last
    // outdoors in the destination's worldspace [design]; across worldspaces the places
    // are compared as they stand [design].
    let from = match location.interior {
        None => phys.player.as_ref().map(|c| c.feet),
        Some(_) => state.as_ref().and_then(|st| st.outdoors).filter(|o| Some(o.0) == world).map(|o| o.1),
    };
    let distance = from.map_or(0.0, |f| (dest.pos[0] - f[0]).hypot(dest.pos[1] - f[1]));
    let scale = sim.world.time_scale().unwrap_or(OWN_TIME_SCALE);
    // The player's tweak: no time passes on the trip (and so no needs) [design: docs/design/tweaks.md].
    let hours = if tweaks.as_ref().is_some_and(|t| t.no_travel_time) { 0.0 } else { travel_hours(distance, crate::player::MoveSpeeds::OBSERVED.walk, scale) };
    let travel = Travel { world, interior, feet: dest.pos, heading_deg: crate::activate::outcome::heading_deg(dest.rot), pitch_deg: None };
    info!(
        "fast travel: to {} ({}): arrive at {} {:?}, {:.0} units away, {hours:.2} hours on the clock",
        m.name,
        FormId(marker),
        if dest.id.0 == marker { "the marker".to_string() } else { format!("its linked reference {}", dest.id) },
        dest.pos,
        distance
    );
    // The player's tweak: the trip takes a bottle [design: docs/design/tweaks.md].
    if tweaks.as_ref().is_some_and(|t| t.game.travel_costs_bottle) {
        if let Some(b) = travel_bottle(&mut sim) {
            let _ = sim.world.remove_item(FormId::PLAYER_REF, b, 1);
            info!("fast travel: one {} taken for the trip (tweak)", sim.name(b).unwrap_or("bottle"));
        }
    }
    pending.0 = Some(Trip { travel, distance, to: dest.id.0, cell: dest.cell.map(|c| c.0), rot: dest.rot, hours });
    menu.write(MenuInput(Command::Close));
}

/// Once the game runs again: the trip goes out behind the loading screen, the clock moves
/// on and the teammates come along.
#[allow(clippy::too_many_arguments)]
pub fn depart(
    state: Res<State<PlayState>>,
    mut pending: ResMut<Pending>,
    mut sim: ResMut<SimState>,
    mut script: Option<ResMut<ScriptState>>,
    mut travel: MessageWriter<TravelRequest>,
    mut passed: MessageWriter<HourPassed>,
    landing: Option<ResMut<crate::save::FastTravelLanding>>,
    screen: Option<ResMut<crate::loading::Loading>>,
    (tweaks, hold): (Option<Res<crate::tweaks::TweakSettings>>, Option<ResMut<WeatherHold>>),
) {
    if *state.get() != PlayState::Playing {
        return;
    }
    let Some(t) = pending.0.take() else { return };
    // The player's own trip (`Loading::arriving`): the destination's actors are put at
    // their package's place behind the screen.
    if let Some(mut s) = screen {
        s.arriving = true;
    }
    travel.write(TravelRequest(t.travel));
    // The player's Improved Weather tweak: a short trip keeps the weather [design:
    // docs/design/tweaks.md].
    if tweaks.as_ref().is_some_and(|tw| tw.improved_weather && t.distance < tw.weather_trip_distance) {
        info!("fast travel: {:.0} units, short: the weather stays (tweak)", t.distance);
        if let Some(mut h) = hold {
            h.0 = true;
        }
    }
    if let Some(mut l) = landing {
        l.0 = true;
    }
    if t.hours > 0.0 {
        match sim.world.advance_hours(t.hours) {
            Ok(c) => info!("fast travel: {:.2} hours passed (GameHour {:.3}, GameDaysPassed {:.3})", t.hours, c.hour, c.days_passed),
            Err(e) => warn!("fast travel: the game clock cannot advance: {e}"),
        }
        for _ in 0..t.hours.floor() as u32 {
            passed.write(HourPassed { sleep: false });
        }
    }
    let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) else { return };
    crate::script::with_world(rt, &mut sim, |rt| {
        let g = &mut rt.game;
        if t.hours > 0.0 {
            g.pass_hours(t.hours, false);
        }
        // The player's reference is where the trip ends, so that the teammates are put
        // beside them there (as a load door does).
        let cell = t.cell.map(|c| {
            let c = FormId(c);
            g.world.placed.exterior_cell_at(c, t.travel.feet).unwrap_or(c)
        });
        let from = g.world.peek(FormId::PLAYER_REF).map(|p| (p.cell, p.pos));
        if let Ok(p) = g.world.get(FormId::PLAYER_REF) {
            (p.cell, p.pos, p.rot) = (cell, t.travel.feet, [0.0, 0.0, t.rot[2]]);
        }
        g.player_entered(cell);
        // GetPCFastTravelled (`fnv_game::nvse_jip12`).
        g.note_fast_travel();
        let brought = g.bring_teammates(from);
        info!("fast travel: under way to {}; {} teammate(s) or follower(s) come along", FormId(t.to), brought.len());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tweaks_drop_or_add_refusals() {
        let all = Checks { target: true, in_air: true, combat: true, hostiles_near: true, encumbered: true, ..Checks::default() };
        let mut c = all;
        tweak_checks(&mut c, &crate::tweaks::TweakSettings::default());
        assert_eq!(c, all, "no tweak changes nothing");
        let t = crate::tweaks::TweakSettings { midair_fast_travel: true, fast_travel_enemies_nearby: true, fast_travel_overencumbered: true, ..Default::default() };
        tweak_checks(&mut c, &t);
        assert_eq!(refusal(&c), Some(Refusal::Combat), "combat itself still refuses");
        let mut indoors = Checks { target: true, place: true, ..Checks::default() };
        tweak_checks(&mut indoors, &t);
        assert_eq!(refusal(&indoors), Some(Refusal::Place));
        let anywhere = fnv_game::tweaks::Tweaks { unsafe_fast_travel: true, ..Default::default() };
        tweak_checks(&mut indoors, &crate::tweaks::TweakSettings { game: anywhere, ..Default::default() });
        assert_eq!(refusal(&indoors), None, "Fast Travel From Anywhere");
        let mut open = Checks { target: true, ..Checks::default() };
        tweak_checks(&mut open, &crate::tweaks::TweakSettings { disable_fast_travel: true, ..Default::default() });
        assert_eq!(refusal(&open), Some(Refusal::Script));
    }

    #[test]
    fn the_refusals_go_in_order() {
        let open = Checks { target: true, ..Checks::default() };
        assert_eq!(refusal(&open), None);
        assert_eq!(refusal(&Checks::default()), Some(Refusal::Undiscovered));
        let all = Checks { target: true, script_block: true, place: true, in_air: true, combat: true, hostiles_near: true, encumbered: true, crippled_leg: true, no_bottle: true };
        assert_eq!(refusal(&all), Some(Refusal::Script));
        assert_eq!(refusal(&Checks { script_block: false, place: false, ..all }), Some(Refusal::InAir));
        assert_eq!(refusal(&Checks { target: true, hostiles_near: true, ..Checks::default() }), Some(Refusal::HostilesNear));
        assert_eq!(refusal(&Checks { target: true, crippled_leg: true, ..Checks::default() }), Some(Refusal::CrippledLeg), "the tweak's refusal, last");
    }

    #[test]
    fn the_question_names_the_marker() {
        assert_eq!(question(OWN_QUESTION, "Goodsprings"), "Travel to Goodsprings?");
        assert_eq!(question("Travel to", "Primm"), "Travel to Primm?");
    }

    #[test]
    fn travel_time_is_the_walk_at_timescale() {
        // 86.8 units a second for 3600 real seconds is one game hour at TimeScale 1.
        assert!((travel_hours(86.8 * 3600.0, 86.8, 1.0) - 1.0).abs() < 1e-5);
        assert!((travel_hours(86.8 * 120.0, 86.8, 30.0) - 1.0).abs() < 1e-5);
        assert_eq!(travel_hours(1000.0, 86.8, 30.0), 0.0);
    }

    #[test]
    fn the_box_answers() {
        let mut p = Popup::travel(7, "Travel to X?".into());
        assert_eq!(p.take(p.selected), Some(7));
        p.step(1);
        p.step(1);
        assert_eq!(p.selected, 1);
        assert_eq!(p.take(p.selected), None);
        let r = Popup::refused("No.".into());
        assert_eq!((r.buttons.len(), r.take(0)), (1, None));
    }
}
