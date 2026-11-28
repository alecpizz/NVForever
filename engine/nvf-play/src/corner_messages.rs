//! The messages at the top left of the view, drawn as the original draws them
//! (docs/spec-notes/corner-messages.md): a bracket, an icon and the text beside it.
//!
//! - What is shown: messages (MESG) that are not message boxes, with the icon and the
//!   time their record gives, and the other short lines of `ScriptEffect::Notify`
//!   (items, default activations) and the notes the player is given (issue #1163)
//!   with the default icon (§5). Quests and objectives
//!   are `crate::quest_messages`'.
//! - One at a time, in the order they came, each for its record's Display Time [owner's
//!   rule, 2026-10-11]; [`CornerMessages`] is the queue (pure, tested).
//! - [`layout`] sets the text (pure, tested); the numbers are the HUD menu file's and
//!   those measured on the owner's screenshot (§2, §4).
//! - The bracket and the icons come from the interface's texture atlas where its index
//!   has them, else from their own files (§2.2). Without the game's files the text is
//!   drawn with Bevy's font beside a plain bracket.
//! - While a message box or a script's menu is open, and in any state but
//!   `PlayState::Playing` (a terminal, the Pip-Boy, a container: every HUD piece goes
//!   then [observed: hud.md §14.3]), the area is hidden and its clock stands still
//!   [design], so a message is not used up behind a menu ([`held`], issue #1239).

use crate::interface::{amber, at, bc_supported, face, file_name, plain_font, set_text, u, Face, Picture, Pictures, Set};
use crate::message_menu::MessagePanel;
use crate::script_events::Note;
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use fnv_font::Font;
use nvf_assets::GameData;
use std::collections::VecDeque;

// ---- the HUD menu file's numbers [documented: menus\prefabs\hudtemplates.xml, §2] ------

/// The bracket and the icon, from the area's corner: left, top, width, height. The icon
/// is drawn over the bracket.
const BRACKET: [f32; 4] = [0.0, -20.0, 470.0, 192.0];
const ICON: [f32; 4] = [0.0, -25.0, 160.0, 160.0];
/// The area's width [documented: menus\main\hud_main_menu.xml].
const AREA_WIDTH: f32 = 460.0;

// ---- measured on the owner's screenshot [observed, §4] ---------------------------------

/// The area's corner in the view.
const AREA: (f32, f32) = (25.0, 55.0);
/// Where the text starts, and how far under the area's top its first line stands.
pub(crate) const TEXT_X: f32 = 130.0;
const TEXT_BASE: f32 = 30.0;
/// Lines are the font's size and this much apart.
const LEADING: f32 = 2.0;
/// The text is broken at what is left of the area's width [hypothesis: the screenshot's
/// text breaks between 228 and 336 units, §4].
pub(crate) const WRAP: f32 = AREA_WIDTH - TEXT_X;
/// The bracket is drawn fainter than the icon and the text.
pub(crate) const BRACKET_STRENGTH: f32 = 0.65;

// ---- NVForever's own [design, §5] -------------------------------------------------------

/// The seconds a line without a Display Time stays.
pub const DEFAULT_SECONDS: f32 = 2.0;
/// The seconds a message takes to come and, after its time, to go.
const FADE: f32 = 0.25;
/// Messages waiting at once; more are left out (a script that shows one every frame).
const MAX_WAITING: usize = 64;

const BRACKET_FILE: &str = "interface\\icons\\mood\\glow_messages_seperator_left.dds";
/// The icon of a message without one [documented: GECK wiki, Message].
pub(crate) const DEFAULT_ICON: &str = "interface\\icons\\message icons\\glow_message_vaultboy_neutral.dds";
/// Font 7 [documented: the file's text template; Fallout_default.ini `sFontFile_7`].
const FONT: &str = "textures\\fonts\\baked-in_monofonto_large.fnt";

/// How long a message stays at full strength.
fn seconds(note: &Note) -> f32 {
    note.seconds.filter(|s| *s > 0.0).unwrap_or(DEFAULT_SECONDS)
}

/// The messages waiting and the one on screen.
#[derive(Resource, Default)]
pub struct CornerMessages {
    waiting: VecDeque<Note>,
    /// The message on screen and the seconds it has been there.
    showing: Option<(Note, f32)>,
    /// True after [`CornerMessages::clear`] dropped an active message whose HUD entity
    /// still needs despawning on the next draw pass [design].
    despawn_needed: bool,
}

impl CornerMessages {
    /// Add a message behind those waiting. One that says what the last one says (the
    /// same text, icon and time) is left out, and so is any when too many wait; `false`
    /// then.
    pub fn push(&mut self, note: Note) -> bool {
        let last = self.waiting.back().or(self.showing.as_ref().map(|s| &s.0));
        if last == Some(&note) || self.waiting.len() >= MAX_WAITING {
            return false;
        }
        self.waiting.push_back(note);
        true
    }

    /// Clear all queued and currently displayed corner messages (`ClearMessageQueue`)
    /// [documented: GECK wiki ClearMessageQueue; observed: hud1, 0 in its frame and the
    /// next; design: clears both waiting and active messages and marks the HUD area for
    /// immediate despawn]. Returns the total number of messages cleared.
    pub fn clear(&mut self) -> usize {
        let had_showing = self.showing.take().is_some();
        let dropped = self.waiting.len() + usize::from(had_showing);
        self.waiting.clear();
        self.despawn_needed |= had_showing;
        dropped
    }

    /// Let `dt` seconds pass; `true` when another message (or none) is now on screen.
    pub fn step(&mut self, dt: f32) -> bool {
        let mut changed = false;
        if let Some((note, age)) = &mut self.showing {
            *age += dt;
            if *age >= seconds(note) + FADE {
                self.showing = None;
                changed = true;
            }
        }
        if self.showing.is_none() {
            if let Some(next) = self.waiting.pop_front() {
                self.showing = Some((next, 0.0));
                changed = true;
            }
        }
        changed
    }

    pub fn showing(&self) -> Option<&Note> {
        self.showing.as_ref().map(|s| &s.0)
    }

    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }

    /// Total corner messages queued, including the one currently on screen
    /// (`GetNumQueuedCornerMessages`) [documented: GECK wiki GetNumQueuedCornerMessages;
    /// observed: hud1].
    pub fn total(&self) -> usize {
        self.waiting.len() + usize::from(self.showing.is_some())
    }

    pub(crate) fn needs_despawn(&self) -> bool {
        self.despawn_needed
    }

    pub(crate) fn take_despawn(&mut self) -> bool {
        std::mem::take(&mut self.despawn_needed)
    }

    /// How strongly the message on screen is drawn: it comes during its first moment
    /// and goes after its time.
    pub fn strength(&self) -> f32 {
        let Some((note, age)) = &self.showing else { return 0.0 };
        let left = seconds(note) + FADE - age;
        (age / FADE).min(left / FADE).clamp(0.0, 1.0)
    }
}

/// The text's lines, from the area's corner.
pub fn layout(text: &str, font: &Font) -> Vec<Set> {
    let pitch = font.size + LEADING;
    let lines = font.lines(text.trim(), Some(WRAP));
    lines.into_iter().enumerate().map(|(i, line)| Set { line, x: TEXT_X, base: TEXT_BASE + pitch * i as f32 }).collect()
}

// ---- the game's files -------------------------------------------------------------------

/// What the area is drawn with, read when the first message comes.
struct Kit {
    face: Face,
    pictures: Pictures,
}

impl Kit {
    fn load(data: Option<&GameData>, images: &mut Assets<Image>, bc: bool) -> Kit {
        Kit { face: face(FONT, plain_font(36.0, 17.0), data, images, bc), pictures: Pictures::load(data) }
    }

    fn picture(&mut self, path: &str, data: Option<&GameData>, images: &mut Assets<Image>, bc: bool) -> Option<Picture> {
        self.pictures.picture(path, data, images, bc)
    }
}

// ---- drawing ----------------------------------------------------------------------------

pub struct CornerMessagesPlugin;

impl Plugin for CornerMessagesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CornerMessages>().add_systems(Update, (note_notices, draw).chain().in_set(crate::PlaySet::Ui));
    }
}

/// A note given to the player (picked up, taken, added by a script or a terminal) is
/// named in the corner, with the default icon (`fnv_game::Game::file_note`, issue #1163).
/// One given behind a menu waits for it to close ([`held`]).
fn note_notices(mut script: Option<ResMut<crate::script::ScriptState>>, mut corner: ResMut<CornerMessages>) {
    let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else { return };
    if rt.game.hud.take_corner_clear() {
        let dropped = corner.clear();
        info!("corner messages: cleared {dropped} message(s) (ClearMessageQueue)");
    }
    for note in rt.game.take_note_notices() {
        let text = rt.game.note_notice_text(note);
        info!("note notice: {text}");
        corner.push(Note::from(text));
    }
    let total = corner.total();
    if rt.game.events.is_empty() && (rt.game.hud.corner_queue != total || !rt.game.hud.corner_queue_synced) {
        rt.game.hud.sync_corner_queue(total);
    }
}

// The area's root; the marker lives with the tile HUD's, which hides it while the HUD is
// drawn from tiles (`crate::tiles::vanilla::hide_hand_built`).
pub(crate) use crate::tiles::vanilla::MessagesArea as AreaRoot;

/// A picture or text that carries the interface's colour, to come and go with the
/// message, and how strongly it is drawn when the message is fully there.
#[derive(Component, Clone)]
struct Part(f32);

/// A plain line of the stand-in bracket, coloured by its fill.
#[derive(Component)]
struct Fill;

fn spawn_area(commands: &mut Commands, note: &Note, kit: &mut Kit, data: Option<&GameData>, images: &mut Assets<Image>, bc: bool, strength: f32) {
    let color = amber(255.0).with_alpha(strength);
    let faint = amber(255.0).with_alpha(strength * BRACKET_STRENGTH);
    let bracket = kit.picture(BRACKET_FILE, data, images, bc);
    let icon = note.icon.as_deref().and_then(|i| kit.picture(i, data, images, bc)).or_else(|| kit.picture(DEFAULT_ICON, data, images, bc));
    let lines = layout(&note.text, &kit.face.font);
    let picture = |(image, rect): Picture, color: Color| ImageNode { rect, color, image_mode: NodeImageMode::Stretch, ..ImageNode::new(image) };
    let area = Node { position_type: PositionType::Absolute, left: u(AREA.0), top: u(AREA.1), ..default() };
    commands.spawn((AreaRoot, area, GlobalZIndex(50))).with_children(|a| {
        let [x, y, w, h] = BRACKET;
        match bracket {
            Some(p) => {
                a.spawn((at(x, y, w, h), picture(p, faint), Part(BRACKET_STRENGTH)));
            }
            // Without the game's picture: a line across and a shorter one down.
            None => {
                a.spawn((at(x + 12.0, y + 14.0, 400.0, 2.0), BackgroundColor(faint), Fill));
                a.spawn((at(x + 12.0, y + 14.0, 2.0, 120.0), BackgroundColor(faint), Fill));
            }
        }
        if let Some(p) = icon {
            let [x, y, w, h] = ICON;
            a.spawn((at(x, y, w, h), picture(p, color), Part(1.0)));
        }
        for line in &lines {
            set_text(a, &kit.face, line, Vec2::ZERO, color, Part(1.0));
        }
    });
}

/// Whether the corner messages and the quest lines (`crate::quest_messages`) are hidden
/// with their clocks stopped, so a line comes back when the hold ends. Behind a message
/// box or a script's menu (`panel_open`); in any state but `Playing`: a conversation
/// (every HUD element goes then: the owner's playtest list 5, item 74), a terminal, the
/// Pip-Boy, a container, barter, lockpicking, a loading screen, as the rest of the HUD
/// (`crate::hud`) [observed: hud.md §14.3, every HUDMainMenu piece `visible` 0 with a
/// menu open; for a terminal in particular a hypothesis; that the original's timers stop
/// there is not observed: decision, issue #1239]; while an idle holds the camera (the
/// opening's wake-up, `crate::player::ViewDriven`): only subtitles then [owner
/// 2026-10-24]; and while a kill camera plays (`crate::kill_cam`) [owner's playtest
/// request #1034].
pub(crate) fn held(panel_open: bool, state: Option<crate::PlayState>, driven: bool, kill_cam: bool) -> bool {
    panel_open || state.is_some_and(|s| s != crate::PlayState::Playing) || driven || kill_cam
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn draw(
    mut commands: Commands,
    time: Res<Time>,
    (mut corner, mut script): (ResMut<CornerMessages>, Option<ResMut<crate::script::ScriptState>>),
    panel: Res<MessagePanel>,
    data: Option<Res<GameData>>,
    mut images: ResMut<Assets<Image>>,
    formats: Option<Res<CompressedImageFormatSupport>>,
    mut roots: Query<(Entity, &mut Visibility), With<AreaRoot>>,
    mut pictures: Query<(&mut ImageNode, &Part)>,
    mut texts: Query<(&mut TextColor, &Part)>,
    mut fills: Query<&mut BackgroundColor, With<Fill>>,
    mut kit: Local<Option<Kit>>,
    state: Option<Res<State<crate::PlayState>>>,
    driven: Option<Res<crate::player::ViewDriven>>,
    (kill_cam, vats, photo): (Option<Res<crate::kill_cam::KillCam>>, Option<Res<crate::vats_mode::VatsMode>>, Option<Res<crate::photo_mode::PhotoMode>>),
) {
    // The HUD's lengths at the UI scale (`crate::interface::hud_units`, #1631).
    let _hud = crate::interface::hud_units();
    if let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) {
        if rt.game.hud.take_corner_clear() {
            let dropped = corner.clear();
            info!("corner messages: cleared {dropped} message(s) (ClearMessageQueue)");
        }
    }
    let mut despawned = false;
    if corner.needs_despawn() {
        corner.bypass_change_detection().take_despawn();
        for (e, _) in &roots {
            commands.entity(e).despawn();
        }
        despawned = true;
    }
    // (As under a kill camera while the classic VATS style plays its queued attacks.)
    let cut = kill_cam.is_some_and(|k| k.active()) || vats.is_some_and(|v| v.playing_back());
    // Photo mode holds them as a menu does, hidden with their clocks stopped, like the
    // rest of the HUD (`crate::photo_mode`; its "Photo saved" is its own).
    let photo = photo.is_some_and(|p| p.on);
    let held = held(panel.view.is_some(), state.map(|s| *s.get()), driven.is_some_and(|d| d.0), cut || photo);
    if !despawned {
        for (_, mut v) in &mut roots {
            v.set_if_neq(if held { Visibility::Hidden } else { Visibility::Inherited });
        }
    }
    let sync_count = |script: &mut Option<ResMut<crate::script::ScriptState>>, total: usize| {
        if let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) {
            if rt.game.events.is_empty() && (rt.game.hud.corner_queue != total || !rt.game.hud.corner_queue_synced) {
                rt.game.hud.sync_corner_queue(total);
            }
        }
    };
    if held {
        sync_count(&mut script, corner.total());
        return;
    }
    let changed = corner.bypass_change_detection().step(time.delta_secs());
    sync_count(&mut script, corner.total());
    let strength = corner.strength();
    if changed {
        if !despawned {
            for (e, _) in &roots {
                commands.entity(e).despawn();
            }
        }
        if let Some(note) = corner.showing() {
            let bc = bc_supported(formats.as_deref());
            let kit = kit.get_or_insert_with(|| Kit::load(data.as_deref(), &mut images, bc));
            info!(
                "corner message: \"{}\" for {} s, icon {} ({} waiting)",
                note.text.replace('\n', " / "),
                seconds(note),
                note.icon.as_deref().map_or("default".into(), file_name),
                corner.waiting()
            );
            spawn_area(&mut commands, note, kit, data.as_deref(), &mut images, bc, strength);
        }
        return;
    }
    let color = |part: f32| amber(255.0).with_alpha(strength * part);
    for (mut p, part) in &mut pictures {
        if p.color != color(part.0) {
            p.color = color(part.0);
        }
    }
    for (mut t, part) in &mut texts {
        t.set_if_neq(TextColor(color(part.0)));
    }
    for mut f in &mut fills {
        f.set_if_neq(BackgroundColor(color(BRACKET_STRENGTH)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(text: &str, seconds: Option<f32>) -> Note {
        Note { text: text.into(), icon: None, seconds }
    }

    /// Letters that take 17, spaces 13, lines of 36: about the game's font 7.
    fn font() -> Font {
        let mut f = plain_font(36.0, 17.0);
        f.glyphs[b' ' as usize].after = 13.0;
        f
    }

    /// Issue #1239: behind a terminal, the Pip-Boy or any menu (`PlayState::Menu`) the
    /// areas are held as behind a message box; in the world with nothing else open, not.
    #[test]
    fn held_outside_play_and_behind_boxes() {
        use crate::PlayState::*;
        assert!(!held(false, Some(Playing), false, false));
        assert!(!held(false, None, false, false));
        for s in [Menu, Dialogue, Loading] {
            assert!(held(false, Some(s), false, false), "{s:?}");
        }
        assert!(held(true, Some(Playing), false, false));
        assert!(held(false, Some(Playing), true, false));
        assert!(held(false, Some(Playing), false, true));
    }

    #[test]
    fn the_screenshots_text_is_set_as_measured() {
        // Too wide for one line: broken after "Well", as on the screenshot.
        let l = layout("You are now Well Rested!", &font());
        assert_eq!(l.len(), 2);
        assert_eq!((l[0].line.glyphs.len(), l[0].line.width), (13, 260.0));
        assert_eq!((l[0].x, l[0].base), (130.0, 30.0));
        assert_eq!((l[1].x, l[1].base, l[1].line.width), (130.0, 68.0, 119.0));
        // A text that fits is one line; the room around it is not set.
        let one = layout("  Bomb disarmed.\n", &font());
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].line.width, 13.0 * 17.0 + 13.0);
    }

    #[test]
    fn messages_are_shown_one_at_a_time_for_their_own_time() {
        let mut c = CornerMessages::default();
        assert!(c.push(note("first", Some(10.0))));
        assert!(c.push(note("second", None)));
        assert!(c.push(note("third", Some(0.0))));
        assert!(c.showing().is_none());
        // The first comes at once and stays its ten seconds, then goes.
        assert!(c.step(0.0));
        assert_eq!(c.showing().unwrap().text, "first");
        assert_eq!(c.strength(), 0.0);
        assert!(!c.step(0.125));
        assert_eq!(c.strength(), 0.5);
        assert!(!c.step(9.875));
        assert_eq!((c.showing().unwrap().text.as_str(), c.strength(), c.waiting()), ("first", 1.0, 2));
        assert!(!c.step(0.125));
        assert_eq!(c.strength(), 0.5);
        // Only then the second, for the time of a line without one; a time of 0 is
        // taken as none.
        assert!(c.step(0.125));
        assert_eq!(c.showing().unwrap().text, "second");
        assert!(!c.step(DEFAULT_SECONDS));
        assert!(c.step(FADE));
        assert_eq!(c.showing().unwrap().text, "third");
        assert!(!c.step(DEFAULT_SECONDS));
        assert!(c.step(FADE));
        assert!(c.showing().is_none());
        assert_eq!(c.strength(), 0.0);
        assert!(!c.step(1.0));
    }

    #[test]
    fn a_message_said_twice_running_is_shown_once() {
        let mut c = CornerMessages::default();
        assert!(c.push(note("a", None)));
        assert!(!c.push(note("a", None)));
        assert!(c.push(note("b", None)));
        assert!(c.push(note("a", None)));
        assert_eq!(c.waiting(), 3);
        // The one on screen counts as the last when none waits.
        let mut c = CornerMessages::default();
        c.push(note("a", None));
        c.step(0.0);
        assert!(!c.push(note("a", None)));
        assert!(c.push(note("a", Some(4.0))));
        // No more than the most that may wait.
        let mut c = CornerMessages::default();
        for i in 0..MAX_WAITING + 5 {
            c.push(note(&i.to_string(), None));
        }
        assert_eq!(c.waiting(), MAX_WAITING);
    }

    #[test]
    fn clear_drops_showing_and_waiting_and_allows_new_messages() {
        let mut c = CornerMessages::default();
        assert_eq!(c.total(), 0);
        assert!(c.push(note("first", Some(5.0))));
        assert!(c.push(note("second", Some(5.0))));
        assert_eq!(c.total(), 2);
        assert!(c.step(0.0));
        assert_eq!(c.showing().unwrap().text, "first");
        assert_eq!(c.waiting(), 1);
        assert_eq!(c.total(), 2);

        // Clearing drops both active and waiting messages and flags visual despawn.
        assert_eq!(c.clear(), 2);
        assert_eq!(c.total(), 0);
        assert!(c.showing().is_none());
        assert_eq!(c.waiting(), 0);
        assert!(c.needs_despawn());
        // A second clear when already empty preserves the pending despawn flag.
        assert_eq!(c.clear(), 0);
        assert!(c.needs_despawn());
        assert!(c.take_despawn());
        assert!(!c.needs_despawn());

        // Pushing the same or a new message after clear displays normally.
        assert!(c.push(note("first", Some(2.0))));
        assert_eq!(c.total(), 1);
        assert!(c.step(0.0));
        assert_eq!(c.showing().unwrap().text, "first");
    }

    fn run_query_num(world: &mut World, body: &str) -> f64 {
        let mut script = world.resource_mut::<crate::script::ScriptState>();
        let rt = script.rt.as_mut().unwrap();
        let src = format!("Begin Function {{}}\n{body}\nEnd\n");
        match rt.game.run_query(&src, fnv_game::game::RunCtx::default()).unwrap() {
            fnv_script::vm::XVal::Num(x) => x,
            other => panic!("expected number, got {other:?}"),
        }
    }

    fn corner_test_app() -> App {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<crate::PlayState>()
            .init_resource::<Time>()
            .init_resource::<Assets<Image>>()
            .init_resource::<MessagePanel>()
            .insert_resource(crate::script::ScriptState::over_no_plugins())
            .add_plugins(CornerMessagesPlugin);
        app
    }

    #[test]
    fn engine_messages_sync_and_clear_message_queue_clears_live_hud() {
        let mut app = corner_test_app();

        // Engine gameplay system pushes two corner notices directly to CornerMessages.
        {
            let mut corner = app.world_mut().resource_mut::<CornerMessages>();
            assert!(corner.push(note("Notice one", Some(4.0))));
            assert!(corner.push(note("Notice two", Some(4.0))));
        }

        // First update promotes "Notice one" onto screen, spawns AreaRoot, and syncs count=2.
        app.update();
        assert_eq!(app.world().resource::<CornerMessages>().showing().unwrap().text, "Notice one");
        assert_eq!(app.world().resource::<CornerMessages>().waiting(), 1);
        assert_eq!(app.world_mut().query_filtered::<Entity, With<AreaRoot>>().iter(app.world()).count(), 1);
        assert_eq!(run_query_num(app.world_mut(), "SetFunctionValue (GetNumQueuedCornerMessages)"), 2.0);

        // Even while held behind a menu, ClearMessageQueue clears CornerMessages, despawns
        // AreaRoot immediately, and reports 0 in its frame and the next.
        app.world_mut().resource_mut::<NextState<crate::PlayState>>().set(crate::PlayState::Menu);
        app.update();
        assert_eq!(run_query_num(app.world_mut(), "SetFunctionValue (GetNumQueuedCornerMessages)"), 2.0);
        assert_eq!(
            run_query_num(app.world_mut(), "ClearMessageQueue\nSetFunctionValue (GetNumQueuedCornerMessages)"),
            0.0
        );
        app.update();
        assert_eq!(app.world().resource::<CornerMessages>().total(), 0);
        assert!(app.world().resource::<CornerMessages>().showing().is_none());
        assert_eq!(app.world_mut().query_filtered::<Entity, With<AreaRoot>>().iter(app.world()).count(), 0);
        assert_eq!(run_query_num(app.world_mut(), "SetFunctionValue (GetNumQueuedCornerMessages)"), 0.0);

        // Returning to Playing and queuing a new message displays normally and drops back
        // to 0 once its display time + fade finishes.
        app.world_mut().resource_mut::<NextState<crate::PlayState>>().set(crate::PlayState::Playing);
        app.update();
        app.world_mut().resource_mut::<CornerMessages>().push(note("After clear", Some(2.0)));
        app.update();
        assert_eq!(app.world().resource::<CornerMessages>().showing().unwrap().text, "After clear");
        assert_eq!(app.world_mut().query_filtered::<Entity, With<AreaRoot>>().iter(app.world()).count(), 1);
        assert_eq!(run_query_num(app.world_mut(), "SetFunctionValue (GetNumQueuedCornerMessages)"), 1.0);

        // Advance past display time + fade: message expires and GetNumQueuedCornerMessages returns 0.
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(2.5));
        app.update();
        assert!(app.world().resource::<CornerMessages>().showing().is_none());
        assert_eq!(app.world_mut().query_filtered::<Entity, With<AreaRoot>>().iter(app.world()).count(), 0);
        assert_eq!(run_query_num(app.world_mut(), "SetFunctionValue (GetNumQueuedCornerMessages)"), 0.0);
    }

    #[test]
    fn vanilla_corner_messages_without_script_calls_behave_identically() {
        let mut app = corner_test_app();
        {
            let mut corner = app.world_mut().resource_mut::<CornerMessages>();
            corner.push(note("Vanilla one", Some(2.0)));
            corner.push(note("Vanilla two", Some(2.0)));
        }
        app.update();
        assert_eq!(app.world().resource::<CornerMessages>().showing().unwrap().text, "Vanilla one");
        assert_eq!(app.world().resource::<CornerMessages>().waiting(), 1);
        assert_eq!(app.world_mut().query_filtered::<Entity, With<AreaRoot>>().iter(app.world()).count(), 1);

        // Held behind a menu keeps the message paused and hidden without clearing it.
        app.world_mut().resource_mut::<NextState<crate::PlayState>>().set(crate::PlayState::Menu);
        app.update();
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(10.0));
        app.update();
        assert_eq!(app.world().resource::<CornerMessages>().showing().unwrap().text, "Vanilla one");
        assert_eq!(app.world().resource::<CornerMessages>().waiting(), 1);

        // Back in Playing, reset the frame delta before unpausing, then advance past
        // display time + fade so "Vanilla two" takes over.
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::ZERO);
        app.world_mut().resource_mut::<NextState<crate::PlayState>>().set(crate::PlayState::Playing);
        app.update();
        assert_eq!(app.world().resource::<CornerMessages>().showing().unwrap().text, "Vanilla one");
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(2.5));
        app.update();
        assert_eq!(app.world().resource::<CornerMessages>().showing().unwrap().text, "Vanilla two");
        assert_eq!(app.world().resource::<CornerMessages>().waiting(), 0);
    }

    fn run_scripts_test_app() -> App {
        use crate::script::{run_scripts, ScriptState};
        use crate::sim::SimState;
        use std::sync::LazyLock;

        static SCHEMA: LazyLock<fnv_esm::Schema> = LazyLock::new(|| {
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/fnv");
            fnv_esm::Schema::load_dir(root).unwrap()
        });
        static LO: LazyLock<fnv_esm::LoadOrder<'static>> = LazyLock::new(|| fnv_esm::LoadOrder::new(&[]).unwrap());
        static INDEX: LazyLock<fnv_world::WorldIndex> = LazyLock::new(|| fnv_world::WorldIndex::build(&LO, &SCHEMA));

        let script_state = ScriptState::over_no_plugins();
        let (forms, refs) = {
            let rt = script_state.rt.as_ref().unwrap();
            (rt.game.world.forms, rt.game.world.placed)
        };
        let sim = SimState {
            forms,
            refs,
            world: fnv_sim::World::new(forms, refs, fnv_sim::SimConfig::default()),
        };

        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<crate::PlayState>()
            .insert_resource(crate::PlayWorld(crate::PlayData {
                lo: &LO,
                schema: &SCHEMA,
                index: &INDEX,
                world: 0,
                interior: None,
                eye: [0.0; 3],
                heading_deg: 0.0,
                pitch_deg: 0.0,
                auto_walk: 0.0,
                start_at_cell_spot: false,
                packages: false,
            }))
            .insert_resource(crate::Location { world: 0, interior: None })
            .insert_resource(sim)
            .insert_resource(script_state)
            .init_resource::<Time>()
            .init_resource::<Assets<Image>>()
            .init_resource::<MessagePanel>()
            .init_resource::<crate::pause::PlayClock>()
            .init_resource::<crate::pause::Paused>()
            .init_resource::<crate::pause::Page>()
            .init_resource::<crate::loading::Loading>()
            .init_resource::<crate::tiles::mods::ModsPage>()
            .init_resource::<crate::phys::PhysState>()
            .init_resource::<crate::script_hud::ScriptHud>()
            .init_resource::<CornerMessages>()
            .init_resource::<crate::quest_messages::QuestMessages>()
            .add_message::<crate::RefChanged>()
            .add_message::<crate::fade::RefFade>()
            .add_message::<crate::RefRepose>()
            .add_message::<crate::loading::TravelRequest>()
            .add_message::<crate::OpenContainer>()
            .add_message::<crate::ItemUsed>()
            .add_message::<crate::furniture::UseFurniture>()
            .add_message::<crate::terminal::OpenTerminal>()
            .add_message::<crate::save::LoadRefused>()
            .add_systems(Update, (run_scripts, note_notices, draw).chain());

        let _ = app.world_mut().resource_mut::<crate::phys::PhysState>().place_player([0.0; 3], 60.0, 1000.0);
        app
    }

    #[test]
    fn run_scripts_syncs_engine_messages_and_intra_frame_clear_message_queue() {
        use crate::script::ScriptState;

        let mut app = run_scripts_test_app();

        // Frame 1: an engine system queues a corner notice before scripts run.
        app.world_mut().resource_mut::<CornerMessages>().push(note("Engine notice", Some(5.0)));
        app.update();
        assert_eq!(app.world().resource::<CornerMessages>().showing().unwrap().text, "Engine notice");
        assert_eq!(run_query_num(app.world_mut(), "SetFunctionValue (GetNumQueuedCornerMessages)"), 1.0);

        // Frame 2: console runs MessageEx -> ClearMessageQueue -> MessageEx in one frame.
        let call = |line: &str| crate::console::ConsoleCall { line: line.into(), selected: None, active_quest: None };
        {
            let mut script = app.world_mut().resource_mut::<ScriptState>();
            script.console.push(call("MessageEx \"pre-clear\""));
            script.console.push(call("ClearMessageQueue"));
            script.console.push(call("MessageEx \"post-clear\""));
        }
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_millis(16));
        app.update();

        // Only "post-clear" survives in CornerMessages and GetNumQueuedCornerMessages reads 1.
        let corner = app.world().resource::<CornerMessages>();
        assert_eq!(corner.total(), 1);
        assert_eq!(corner.showing().unwrap().text, "post-clear");
        assert_eq!(corner.waiting(), 0);
        assert_eq!(run_query_num(app.world_mut(), "SetFunctionValue (GetNumQueuedCornerMessages)"), 1.0);
    }

    #[test]
    fn ui_clear_preserves_event_watermark_and_count_until_run_scripts_drains_events() {
        let mut app = run_scripts_test_app();

        // Frame 1: an existing corner message is showing on screen.
        app.world_mut().resource_mut::<CornerMessages>().push(note("Old message", Some(5.0)));
        app.update();
        assert_eq!(app.world().resource::<CornerMessages>().showing().unwrap().text, "Old message");
        assert_eq!(app.world_mut().query_filtered::<Entity, With<AreaRoot>>().iter(app.world()).count(), 1);

        // Simulate a PlaySet::Ui script (e.g. terminal result script) running
        // MessageEx "pre" -> ClearMessageQueue -> MessageEx "post" while leaving
        // Game::events undrained until next frame's run_scripts.
        let count = run_query_num(
            app.world_mut(),
            "MessageEx \"pre\"\nClearMessageQueue\nMessageEx \"post\"\nSetFunctionValue (GetNumQueuedCornerMessages)",
        );
        assert_eq!(count, 1.0);

        // Run only note_notices + draw (before run_scripts): "Old message" is cleared and
        // its AreaRoot despawned, while the event watermark and pending count=1 stay intact.
        let mut ui_schedule = Schedule::default();
        ui_schedule.add_systems((note_notices, draw).chain());
        ui_schedule.run(app.world_mut());
        assert!(app.world().resource::<CornerMessages>().showing().is_none());
        assert_eq!(app.world_mut().query_filtered::<Entity, With<AreaRoot>>().iter(app.world()).count(), 0);
        assert_eq!(run_query_num(app.world_mut(), "SetFunctionValue (GetNumQueuedCornerMessages)"), 1.0);
        assert_eq!(app.world().resource::<crate::script::ScriptState>().rt.as_ref().unwrap().game.hud.corner_clear_event_index, Some(1));

        // Next frame's run_scripts drains Game::events: "pre" is suppressed and only "post" shows.
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_millis(16));
        app.update();
        let corner = app.world().resource::<CornerMessages>();
        assert_eq!(corner.total(), 1);
        assert_eq!(corner.showing().unwrap().text, "post");
        assert_eq!(corner.waiting(), 0);
        assert_eq!(run_query_num(app.world_mut(), "SetFunctionValue (GetNumQueuedCornerMessages)"), 1.0);
    }

    #[test]
    fn run_scripts_preserves_pre_script_clear_and_message_counts() {
        use crate::script::ScriptState;

        let mut app = run_scripts_test_app();
        let call = |line: &str| crate::console::ConsoleCall { line: line.into(), selected: None, active_quest: None };

        // Frame 1: two messages sit in CornerMessages from the prior frame.
        {
            let mut corner = app.world_mut().resource_mut::<CornerMessages>();
            corner.push(note("Stale one", Some(5.0)));
            corner.push(note("Stale two", Some(5.0)));
        }
        app.update();
        assert_eq!(app.world().resource::<CornerMessages>().total(), 2);
        assert_eq!(run_query_num(app.world_mut(), "SetFunctionValue (GetNumQueuedCornerMessages)"), 2.0);

        // Simulate a pre-run_scripts script (PlaySet::Activation / dialogue) calling
        // ClearMessageQueue before run_scripts runs in the same frame.
        assert_eq!(
            run_query_num(app.world_mut(), "ClearMessageQueue\nSetFunctionValue (GetNumQueuedCornerMessages)"),
            0.0
        );
        app.world_mut().resource_mut::<ScriptState>().console.push(call("SetModLocalData \"after_clear\" (GetNumQueuedCornerMessages)"));
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_millis(16));
        app.update();
        assert_eq!(run_query_num(app.world_mut(), "SetFunctionValue (GetModLocalData \"after_clear\")"), 0.0);
        assert_eq!(app.world().resource::<CornerMessages>().total(), 0);

        // Now push one message into CornerMessages, then simulate a pre-run_scripts
        // MessageEx before run_scripts runs: run_scripts must read 2, not stale 1.
        app.world_mut().resource_mut::<CornerMessages>().push(note("Existing", Some(5.0)));
        app.update();
        assert_eq!(run_query_num(app.world_mut(), "SetFunctionValue (GetNumQueuedCornerMessages)"), 1.0);
        assert_eq!(
            run_query_num(app.world_mut(), "MessageEx \"from activate\"\nSetFunctionValue (GetNumQueuedCornerMessages)"),
            2.0
        );
        app.world_mut().resource_mut::<ScriptState>().console.push(call("SetModLocalData \"after_msg\" (GetNumQueuedCornerMessages)"));
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_millis(16));
        app.update();
        assert_eq!(run_query_num(app.world_mut(), "SetFunctionValue (GetModLocalData \"after_msg\")"), 2.0);
        assert_eq!(app.world().resource::<CornerMessages>().total(), 2);
    }
}
