//! Android-only cover admission. A primary-surface submission is not physical scanout.
//! See docs/design/android-continuous-handoff.md. No vendor or game-state mutation.
use bevy::input::{mouse::AccumulatedMouseMotion, InputSystems};
use bevy::render::render_resource::{CachedPipelineState, PipelineCache, PipelineDescriptor};
use bevy::render::view::{DeferredSwapchain, ExtractedWindow, SWAPCHAIN_WAITS};
use bevy::render::{Render, RenderApp, RenderSystems};
use bevy::window::{AppLifecycle, PrimaryWindow, WindowFocused, WindowResized};
use bevy::{ecs::system::SystemParam, prelude::*};
use nvf_play::pad::{PlatformInputGate, PlatformInputSystems};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Ticket {
    request: String,
    epoch: u64,
}
impl Ticket {
    fn parse(text: &str) -> Option<(Self, bool)> {
        let mut parts = text.split(':');
        let request = parts.next()?;
        let epoch = parts.next()?.parse::<u64>().ok()?;
        let eligible = match parts.next()? {
            "0" => false,
            "1" => true,
            _ => return None,
        };
        if epoch == 0
            || parts.next().is_some()
            || request.len() != 32
            || !request
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return None;
        }
        Some((
            Self {
                request: request.into(),
                epoch,
            },
            eligible,
        ))
    }
    fn text(&self) -> String {
        format!("{}:{}", self.request, self.epoch)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Menu,
    Loading,
    Film,
    World,
}
#[derive(Default)]
struct Witness {
    ticket: Option<Ticket>,
    eligible: bool,
    phase: Option<Phase>,
    consecutive: u8,
    ready: Option<Ticket>,
    sent: bool,
}
impl Witness {
    fn request(&mut self, ticket: Option<(Ticket, bool)>) {
        let (next, eligible) = ticket.map_or((None, false), |(t, e)| (Some(t), e));
        if self.ticket != next || self.eligible != eligible {
            self.consecutive = 0;
            self.ready = None;
            self.sent = false;
        }
        self.ticket = next;
        self.eligible = eligible;
    }
    fn phase(&mut self, phase: Option<Phase>) {
        if self.phase != phase {
            self.consecutive = 0;
            self.ready = None;
            self.sent = false;
        }
        self.phase = phase;
    }
    fn submission(&mut self, ticket: &Ticket, phase: Option<Phase>, actual_primary: bool) {
        if !actual_primary
            || !self.eligible
            || self.phase.is_none()
            || self.phase != phase
            || self.ticket.as_ref() != Some(ticket)
        {
            self.consecutive = 0;
            self.ready = None;
            return;
        }
        self.consecutive = self.consecutive.saturating_add(1);
        if self.consecutive >= 2 && !self.sent {
            self.ready = Some(ticket.clone());
        }
    }
}
#[derive(Resource, Clone, Default)]
struct Shared(Arc<Mutex<Witness>>);
impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, Witness> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}
#[derive(Resource, Default)]
struct Before {
    ticket: Option<Ticket>,
    phase: Option<Phase>,
    window: Option<Entity>,
    presents: u64,
}
#[derive(Resource, Default)]
struct Channel {
    #[cfg(test)]
    test_mode: bool,
    #[cfg(test)]
    state: String,
    #[cfg(test)]
    submitted: Vec<String>,
    #[cfg(test)]
    begun: usize,
    #[cfg(test)]
    cover_ready: String,
    #[cfg(test)]
    back_calls: usize,
}
impl Channel {
    fn state(&mut self) -> Result<String, String> {
        #[cfg(test)]
        if self.test_mode {
            return Ok(self.state.clone());
        }
        #[cfg(target_os = "android")]
        {
            crate::android::activity_string("loadingState", None)
        }
        #[cfg(not(target_os = "android"))]
        {
            Err("Native loading API unavailable".into())
        }
    }
    fn submit(&mut self, ticket: &str) -> Result<(), String> {
        #[cfg(test)]
        if self.test_mode {
            self.submitted.push(ticket.into());
            return Ok(());
        }
        #[cfg(target_os = "android")]
        {
            crate::android::activity_string("nativeContentSubmitted", Some(ticket)).map(|_| ())
        }
        #[cfg(not(target_os = "android"))]
        {
            Err(format!("Native loading API unavailable: {}", ticket.len()))
        }
    }
    fn begin_cover(&mut self) -> Result<String, String> {
        #[cfg(test)]
        if self.test_mode {
            self.begun += 1;
            return Ok(Ticket::parse(&self.state).map_or(String::new(), |(t, _)| t.text()));
        }
        #[cfg(target_os = "android")]
        {
            crate::android::activity_string("beginNewGameCover", None)
        }
        #[cfg(not(target_os = "android"))]
        {
            Err("Native cover API unavailable".into())
        }
    }
    fn cover_ready(&mut self) -> Result<String, String> {
        #[cfg(test)]
        if self.test_mode {
            return Ok(self.cover_ready.clone());
        }
        #[cfg(target_os = "android")]
        {
            crate::android::activity_string("contentCoverReady", None)
        }
        #[cfg(not(target_os = "android"))]
        {
            Err("Native cover API unavailable".into())
        }
    }
    fn back(&mut self) {
        #[cfg(test)]
        if self.test_mode {
            self.back_calls += 1;
            #[cfg(target_os = "android")]
            return;
        }
        #[cfg(target_os = "android")]
        {
            let _ = crate::android::activity_string("loadingBack", None);
        }
    }
}
pub(crate) struct AndroidLoadingPlugin;
impl Plugin for AndroidLoadingPlugin {
    fn build(&self, app: &mut App) {
        let shared = Shared::default();
        let mut gate = PlatformInputGate::default();
        gate.set_blocked(true);
        app.insert_resource(shared.clone())
            .insert_resource(gate)
            .init_resource::<Channel>()
            .init_resource::<nvf_play::main_menu::NewGameCover>()
            .add_systems(
                PreUpdate,
                poll.before(crate::touch::TouchInputSystems)
                    .before(PlatformInputSystems::Publish),
            )
            .add_systems(
                PreUpdate,
                consume_motion_and_back
                    .after(InputSystems)
                    .after(PlatformInputSystems::Remember)
                    .before(bevy::ui::UiSystems::Focus),
            )
            .add_systems(PostUpdate, publish_scene);
    }
    fn finish(&self, app: &mut App) {
        let shared = app.world().resource::<Shared>().clone();
        // This plugin is admitted before DefaultPlugins; RenderApp exists after all builds.
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render
                .insert_resource(shared)
                .init_resource::<Before>()
                .add_systems(
                    Render,
                    before_present
                        .in_set(RenderSystems::Render)
                        .before(bevy::render::renderer::render_system),
                )
                .add_systems(Render, after_present.in_set(RenderSystems::Cleanup));
        }
    }
}
#[derive(SystemParam)]
struct Changes<'w, 's> {
    focus: MessageReader<'w, 's, WindowFocused>,
    lifecycle: MessageReader<'w, 's, AppLifecycle>,
    resized: MessageReader<'w, 's, WindowResized>,
}
fn poll(
    (mut channel, mut new_game): (ResMut<Channel>, ResMut<nvf_play::main_menu::NewGameCover>),
    shared: Res<Shared>,
    mut gate: ResMut<PlatformInputGate>,
    time: Res<Time<Real>>,
    mut due: Local<f64>,
    mut changes: Changes,
    mut started_from: Local<Option<String>>,
) {
    let forced =
        !changes.focus.is_empty() || !changes.lifecycle.is_empty() || !changes.resized.is_empty();
    changes.focus.clear();
    changes.lifecycle.clear();
    changes.resized.clear();
    let now = time.elapsed_secs_f64();
    if gate.allows_actions() && !forced && !new_game.requested() && now < *due {
        return;
    }
    *due = now + 0.125;
    if new_game.requested() {
        gate.set_blocked(true);
        if started_from.is_none() {
            if let Ok(previous) = channel.begin_cover() {
                *started_from = Some(previous);
            }
            shared.lock().request(None);
            return;
        }
        if let Ok(ready) = channel.cover_ready() {
            if started_from.as_deref() != Some(ready.as_str())
                && Ticket::parse(&format!("{ready}:1")).is_some()
            {
                new_game.allow_start();
            }
        }
    }
    let result = channel.state();
    let valid_empty = result.as_ref().is_ok_and(|s| s.is_empty());
    let ticket = result.as_ref().ok().and_then(|s| Ticket::parse(s));
    gate.set_blocked(!valid_empty || new_game.requested());
    if valid_empty && new_game.active() && !new_game.requested() {
        new_game.finish();
        *started_from = None;
    }
    let ready = {
        let mut witness = shared.lock();
        witness.request(ticket);
        witness.ready.take()
    };
    if let Some(ready) = ready {
        if channel.submit(&ready.text()).is_ok() {
            shared.lock().sent = true;
        }
    }
}
fn consume_motion_and_back(
    gate: Res<PlatformInputGate>,
    motion: Option<ResMut<AccumulatedMouseMotion>>,
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut channel: ResMut<Channel>,
) {
    if gate.allows_actions() {
        return;
    }
    if let Some(mut motion) = motion {
        motion.delta = Vec2::ZERO;
    }
    if gate.native_blocked() && (keys.just_pressed(KeyCode::Escape)
        || pads.iter().any(|p| p.just_pressed(GamepadButton::East)))
    {
        channel.back();
    }
}
#[derive(SystemParam)]
struct Scene<'w, 's> {
    loading: Option<Res<'w, nvf_play::loading::Loading>>,
    menu: Option<Res<'w, nvf_play::main_menu::MainMenu>>,
    intro: Option<Res<'w, nvf_play::intro::Intro>>,
    new_game: Option<Res<'w, nvf_play::main_menu::NewGameCover>>,
    state: Option<Res<'w, State<nvf_play::PlayState>>>,
    streamer: Option<Res<'w, crate::world::Streamer>>,
    physics: Option<Res<'w, nvf_play::phys::PhysState>>,
    script: Option<Res<'w, nvf_play::script::ScriptState>>,
    restoring: nvf_play::save::Restoring<'w>,
    window: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
}
fn publish_scene(scene: Scene, shared: Res<Shared>) {
    let visible = scene
        .window
        .single()
        .is_ok_and(|w| w.focused && w.physical_width() > 0 && w.physical_height() > 0);
    let phase = if !visible {
        None
    } else if scene.menu.as_ref().is_some_and(|m| m.open) {
        if scene.new_game.as_ref().is_some_and(|c| c.active()) {
            None
        } else {
            Some(Phase::Menu)
        }
    } else if let Some(intro) = scene.intro.as_ref().filter(|i| i.is_playing()) {
        intro.frame_ready().then_some(Phase::Film)
    } else if scene.loading.as_ref().is_some_and(|l| l.active()) {
        Some(Phase::Loading)
    } else if scene.state.as_ref().is_some_and(|s| {
        // Menus/conversations still render the settled world. Simulation pause
        // must not strand a fresh resume epoch behind the Android cover.
        matches!(
            s.get(),
            nvf_play::PlayState::Playing
                | nvf_play::PlayState::Dialogue
                | nvf_play::PlayState::Menu
        )
    }) && scene.streamer.as_ref().is_some_and(|s| s.settled)
        && scene.physics.as_ref().is_none_or(|p| p.loading_done())
        && !scene.restoring.busy()
        && scene
            .script
            .as_ref()
            .is_none_or(|s| !s.loading() && !s.pending_game_waiting())
    {
        Some(Phase::World)
    } else {
        None
    };
    shared.lock().phase(phase);
}
fn screen_pipelines_ready(cache: &PipelineCache) -> bool {
    let mut ui = false;
    let mut blit = false;
    for cached in cache.pipelines() {
        let label = match &cached.descriptor {
            PipelineDescriptor::RenderPipelineDescriptor(d) => d.label.as_deref(),
            _ => None,
        };
        if matches!(label, Some("ui_pipeline" | "blit pipeline")) {
            if !matches!(cached.state, CachedPipelineState::Ok(_)) {
                return false;
            }
            ui |= label == Some("ui_pipeline");
            blit |= label == Some("blit pipeline");
        }
    }
    ui && blit
}
fn before_present(
    shared: Res<Shared>,
    mut before: ResMut<Before>,
    cache: Res<PipelineCache>,
    windows: Query<(
        Entity,
        &ExtractedWindow,
        Has<PrimaryWindow>,
        Has<DeferredSwapchain>,
    )>,
) {
    before.ticket = None;
    before.phase = None;
    before.window = None;
    let witness = shared.lock();
    if !witness.eligible
        || witness.phase.is_none()
        || witness.sent
        || !screen_pipelines_ready(&cache)
    {
        return;
    }
    // The existing counter is aggregate: require exactly one surface and it must be primary.
    let Ok((entity, window, primary, deferred)) = windows.single() else {
        return;
    };
    if !primary
        || deferred
        || window.physical_width == 0
        || window.physical_height == 0
        || window.swap_chain_texture.is_none()
    {
        return;
    }
    before.ticket = witness.ticket.clone();
    before.phase = witness.phase;
    before.window = Some(entity);
    before.presents = SWAPCHAIN_WAITS.totals().1;
}
fn after_present(
    shared: Res<Shared>,
    before: Res<Before>,
    windows: Query<&ExtractedWindow, With<PrimaryWindow>>,
) {
    let Some(ticket) = &before.ticket else {
        let mut witness = shared.lock();
        witness.consecutive = 0;
        witness.ready = None;
        return;
    };
    let presented = before
        .window
        .and_then(|e| windows.get(e).ok())
        .is_some_and(|w| w.swap_chain_texture.is_none())
        && SWAPCHAIN_WAITS.totals().1 > before.presents;
    shared.lock().submission(ticket, before.phase, presented);
}

#[cfg(test)]
mod tests {
    use super::*;
    const REQUEST: &str = "0123456789abcdef0123456789abcdef";
    fn scene() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::input::InputPlugin))
            .add_message::<WindowFocused>()
            .add_message::<WindowResized>()
            .add_message::<AppLifecycle>()
            .init_resource::<nvf_play::main_menu::MainMenu>()
            .add_plugins(AndroidLoadingPlugin);
        app.world_mut()
            .resource_mut::<nvf_play::main_menu::MainMenu>()
            .open = true;
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        let mut channel = app.world_mut().resource_mut::<Channel>();
        channel.test_mode = true;
        channel.state = format!("{REQUEST}:1:1");
        (app, window)
    }
    // CPU injection tests the real main-world protocol and phase route. It does not
    // execute the renderer or claim a physical presentation/scanout measurement.
    fn evidence(app: &mut App, epoch: u64, phase: Option<Phase>, primary: bool) {
        app.world().resource::<Shared>().lock().submission(
            &Ticket {
                request: REQUEST.into(),
                epoch,
            },
            phase,
            primary,
        );
    }
    #[test]
    fn current_scene_and_lifecycle_never_replace_two_primary_submission_witnesses() {
        let (mut app, window) = scene();
        app.update();
        app.world_mut().write_message(WindowFocused {
            window,
            focused: true,
        });
        for _ in 0..4 {
            app.update();
        }
        assert!(!app.world().resource::<PlatformInputGate>().allows_actions());
        assert!(app.world().resource::<Channel>().submitted.is_empty());
        evidence(&mut app, 1, Some(Phase::Menu), false);
        app.update();
        evidence(&mut app, 99, Some(Phase::Menu), true);
        app.update();
        evidence(&mut app, 1, Some(Phase::World), true);
        app.update();
        evidence(&mut app, 1, Some(Phase::Menu), true);
        app.update();
        assert!(app.world().resource::<Channel>().submitted.is_empty());
        evidence(&mut app, 1, Some(Phase::Menu), true);
        app.update();
        assert_eq!(
            app.world().resource::<Channel>().submitted,
            [format!("{REQUEST}:1")]
        );
        // Java must separately acknowledge its cover commit and accept this epoch.
        assert!(!app.world().resource::<PlatformInputGate>().allows_actions());
        evidence(&mut app, 1, Some(Phase::Menu), true);
        app.update();
        assert_eq!(app.world().resource::<Channel>().submitted.len(), 1);
        app.world_mut().resource_mut::<Channel>().state.clear();
        app.update();
        assert!(app.world().resource::<PlatformInputGate>().allows_actions());
    }
    #[test]
    fn phase_change_and_surface_epoch_invalidate_ready_without_replaying_old_submission() {
        let (mut app, _) = scene();
        app.update();
        evidence(&mut app, 1, Some(Phase::Menu), true);
        app.world_mut()
            .resource_mut::<nvf_play::main_menu::MainMenu>()
            .open = false;
        app.update();
        evidence(&mut app, 1, Some(Phase::Menu), true);
        app.update();
        assert!(app.world().resource::<Channel>().submitted.is_empty());
        app.world_mut()
            .resource_mut::<nvf_play::main_menu::MainMenu>()
            .open = true;
        app.update();
        evidence(&mut app, 1, Some(Phase::Menu), true);
        evidence(&mut app, 1, Some(Phase::Menu), true);
        app.world_mut().resource_mut::<Channel>().state = format!("{REQUEST}:2:0");
        app.update();
        assert!(app.world().resource::<Channel>().submitted.is_empty());
        evidence(&mut app, 1, Some(Phase::Menu), true);
        app.update();
        app.world_mut().resource_mut::<Channel>().state = format!("{REQUEST}:2:1");
        app.update();
        evidence(&mut app, 2, Some(Phase::Menu), true);
        app.update();
        evidence(&mut app, 2, Some(Phase::Menu), true);
        app.update();
        assert_eq!(
            app.world().resource::<Channel>().submitted,
            [format!("{REQUEST}:2")]
        );
    }
    #[test]
    fn malformed_native_state_fails_closed_and_cover_discards_physical_motion_only() {
        let (mut app, _) = scene();
        app.world_mut().resource_mut::<Channel>().state = format!("{REQUEST}:0:1");
        // Run the actual post-InputSystems consumer against a physical event.
        app.world_mut()
            .write_message(bevy::input::mouse::MouseMotion {
                delta: Vec2::new(41.0, -17.0),
            });
        app.update();
        assert!(!app.world().resource::<PlatformInputGate>().allows_actions());
        assert_eq!(
            app.world().resource::<AccumulatedMouseMotion>().delta,
            Vec2::ZERO
        );
        assert!(app.world().resource::<Channel>().submitted.is_empty());
        assert!(app.world().resource::<Shared>().lock().ticket.is_none());
    }
    #[test]
    fn new_game_cover_waits_for_a_new_committed_epoch_and_rejects_old_menu_presentations() {
        let (mut app, _) = scene();
        app.update();
        app.world_mut()
            .resource_mut::<nvf_play::main_menu::NewGameCover>()
            .request();
        app.update();
        assert_eq!(app.world().resource::<Channel>().begun, 1);
        app.world_mut().resource_mut::<Channel>().cover_ready = format!("{REQUEST}:1");
        app.update();
        assert!(app
            .world()
            .resource::<nvf_play::main_menu::NewGameCover>()
            .requested());
        assert!(!app.world().resource::<PlatformInputGate>().allows_actions());
        evidence(&mut app, 1, Some(Phase::Menu), true);
        evidence(&mut app, 1, Some(Phase::Menu), true);
        app.update();
        assert!(app.world().resource::<Channel>().submitted.is_empty());
        {
            let mut channel = app.world_mut().resource_mut::<Channel>();
            channel.state = format!("{REQUEST}:2:1");
            channel.cover_ready = format!("{REQUEST}:2");
        }
        app.update();
        assert_eq!(
            app.world().resource::<Channel>().begun,
            1,
            "one cover request despite repeated updates"
        );
        assert!(
            app.world().resource::<Channel>().submitted.is_empty(),
            "menu cannot satisfy intro readiness"
        );
        assert_eq!(app.world().resource::<Shared>().lock().phase, None);
        // The actual menu close/PlayIntro route is covered in nvf_play::main_menu tests.
        // Main-world admission needs decoded film evidence, never the old menu's frames.
    }

    fn settled_world(app: &mut App, state: nvf_play::PlayState) {
        app.world_mut().insert_resource(State::new(state));
        app.world_mut().insert_resource(crate::world::Streamer {
            radius: 0,
            loaded: default(),
            fades: default(),
            fade_secs: 0.,
            settled: true,
            arrival: None,
            per_frame: 1,
            spawning: default(),
            dying: default(),
            ahead: default(),
            hide: default(),
            interior: None,
            light_scale: 1.,
            managed: default(),
            manage_actors: false,
            stats: default(),
            buffer: default(),
            centre: None,
            waiting: default(),
            model_overrides: default(),
            coop_areas: Vec::new(),
            travels: 0,
        });
        app.world_mut()
            .resource_mut::<nvf_play::main_menu::MainMenu>()
            .open = false;
    }
    #[test]
    fn background_resume_of_game_dialogue_and_menus_requires_fresh_epoch_content() {
        use nvf_play::PlayState;
        for (state, paused, main_menu) in [
            (PlayState::Playing, false, false),
            (PlayState::Dialogue, false, false),
            (PlayState::Menu, false, false),
            (PlayState::Menu, true, false),
            (PlayState::Menu, true, true),
        ] {
            let (mut app, window) = scene();
            settled_world(&mut app, state);
            app.world_mut()
                .insert_resource(nvf_play::pause::Paused(paused));
            app.world_mut()
                .resource_mut::<nvf_play::main_menu::MainMenu>()
                .open = main_menu;
            let phase = if main_menu { Phase::Menu } else { Phase::World };
            app.update();
            assert_eq!(app.world().resource::<Shared>().lock().phase, Some(phase));
            evidence(&mut app, 1, Some(phase), true);
            evidence(&mut app, 1, Some(phase), true);
            app.update();
            assert_eq!(
                app.world().resource::<Channel>().submitted,
                [format!("{REQUEST}:1")]
            );
            app.world_mut().resource_mut::<Channel>().state.clear();
            app.update();
            assert!(app.world().resource::<PlatformInputGate>().allows_actions());

            app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
            app.world_mut().resource_mut::<Channel>().state = format!("{REQUEST}:2:0");
            app.world_mut().write_message(AppLifecycle::Suspended);
            app.world_mut().write_message(WindowFocused {
                window,
                focused: false,
            });
            app.update();
            assert_eq!(app.world().resource::<Shared>().lock().phase, None);
            assert!(!app.world().resource::<PlatformInputGate>().allows_actions());
            evidence(&mut app, 1, Some(phase), true);
            evidence(&mut app, 2, Some(phase), true);
            app.update();
            assert_eq!(app.world().resource::<Channel>().submitted.len(), 1);

            app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
            app.world_mut().resource_mut::<Channel>().state = format!("{REQUEST}:3:1");
            app.world_mut().write_message(AppLifecycle::Running);
            app.world_mut().write_message(WindowFocused {
                window,
                focused: true,
            });
            app.update();
            assert_eq!(app.world().resource::<Shared>().lock().phase, Some(phase));
            assert_eq!(app.world().resource::<Channel>().submitted.len(), 1);
            evidence(&mut app, 1, Some(phase), true);
            evidence(&mut app, 3, Some(phase), false);
            app.update();
            evidence(&mut app, 3, Some(phase), true);
            app.update();
            assert_eq!(app.world().resource::<Channel>().submitted.len(), 1);
            evidence(&mut app, 3, Some(phase), true);
            app.update();
            assert_eq!(
                app.world().resource::<Channel>().submitted,
                [format!("{REQUEST}:1"), format!("{REQUEST}:3")]
            );
            assert!(
                !app.world().resource::<PlatformInputGate>().allows_actions(),
                "native submission alone cannot stand in for Java's cover acknowledgement"
            );
            app.world_mut().resource_mut::<Channel>().state.clear();
            app.world_mut().write_message(WindowFocused {
                window,
                focused: true,
            });
            app.update();
            assert!(app.world().resource::<PlatformInputGate>().allows_actions());
        }
    }
    #[test]
    fn resumed_modal_content_still_requires_settled_visible_world_and_not_loading_state() {
        for state in [nvf_play::PlayState::Dialogue, nvf_play::PlayState::Menu] {
            let (mut app, window) = scene();
            settled_world(&mut app, state);
            app.world_mut().resource_mut::<Channel>().state = format!("{REQUEST}:3:1");
            app.world_mut()
                .resource_mut::<crate::world::Streamer>()
                .settled = false;
            app.update();
            assert_eq!(app.world().resource::<Shared>().lock().phase, None);
            evidence(&mut app, 3, Some(Phase::World), true);
            evidence(&mut app, 3, Some(Phase::World), true);
            app.update();
            assert!(app.world().resource::<Channel>().submitted.is_empty());
            app.world_mut()
                .resource_mut::<crate::world::Streamer>()
                .settled = true;
            app.world_mut()
                .insert_resource(State::new(nvf_play::PlayState::Loading));
            app.update();
            assert_eq!(app.world().resource::<Shared>().lock().phase, None);
            app.world_mut().insert_resource(State::new(state));
            app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
            app.update();
            assert_eq!(app.world().resource::<Shared>().lock().phase, None);
            app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
            app.update();
            assert_eq!(
                app.world().resource::<Shared>().lock().phase,
                Some(Phase::World)
            );
            assert!(app.world().resource::<Channel>().submitted.is_empty());
            assert!(!app.world().resource::<PlatformInputGate>().allows_actions());
        }
    }

    #[test]
    fn arm_only_back_never_calls_native_loading_recovery_but_overlapping_loading_still_does() {
        use bevy::input::{keyboard::{Key,KeyboardInput},ButtonState};
        let (mut app,window)=scene();
        app.world_mut().resource_mut::<Channel>().state.clear();
        app.world_mut().resource_mut::<PlatformInputGate>().set_arm_blocked(true);
        app.update();
        assert!(!app.world().resource::<PlatformInputGate>().native_blocked());
        assert!(!app.world().resource::<PlatformInputGate>().allows_actions());
        let key=|down| KeyboardInput { key_code:KeyCode::Escape,logical_key:Key::Escape,
            state:if down {ButtonState::Pressed} else {ButtonState::Released},
            text:None,repeat:false,window };
        app.world_mut().write_message(key(true));
        app.update();
        assert_eq!(app.world().resource::<Channel>().back_calls,0);
        app.world_mut().write_message(key(false));
        app.update();
        app.world_mut().resource_mut::<Channel>().state=format!("{REQUEST}:2:1");
        app.world_mut().write_message(key(true));
        app.update();
        assert!(app.world().resource::<PlatformInputGate>().native_blocked());
        assert_eq!(app.world().resource::<Channel>().back_calls,1);
        app.world_mut().resource_mut::<Channel>().state.clear();
        app.update();
        assert!(!app.world().resource::<PlatformInputGate>().native_blocked());
        assert!(!app.world().resource::<PlatformInputGate>().allows_actions(),"native ack cannot release the arm reason");
        app.world_mut().resource_mut::<PlatformInputGate>().set_arm_blocked(false);
        assert!(app.world().resource::<PlatformInputGate>().allows_actions());
    }
}
