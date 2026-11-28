//! A blank world contact may look after dragging; it never emits a fire button.
use bevy::{
    ecs::system::SystemParam,
    input::touch::{TouchInput, TouchPhase},
    prelude::*,
    window::PrimaryWindow,
};
use nvf_play::{
    activate::touch::{Confirmation, ScreenPoint, TouchActivation},
    settings::{self, ChoiceList, Setting},
};
const DOUBLE_SECONDS: f64 = 0.35;
const DRAG: f32 = 12.0;
#[derive(Resource)]
pub(super) struct Options {
    mode: Confirmation,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            mode: Confirmation::Hold,
        }
    }
}
#[derive(Clone, Copy)]
struct Contact {
    id: u64,
    origin: Vec2,
    dragged: bool,
    second: bool,
    target_seen: bool,
}
#[derive(Clone, Copy)]
struct FirstTap {
    time: f64,
    position: Vec2,
    generation: u64,
}
#[derive(Resource, Default)]
struct GestureState {
    contact: Option<Contact>,
    first: Option<FirstTap>,
    size: Option<Vec2>,
}
#[derive(SystemParam)]
pub(super) struct Bridge<'w> {
    state: ResMut<'w, GestureState>,
    probe: ResMut<'w, TouchActivation>,
    options: Res<'w, Options>,
    time: Option<Res<'w, Time<Real>>>,
}
impl Bridge<'_> {
    fn now(&self) -> f64 {
        self.time.as_ref().map_or(0., |t| t.elapsed_secs_f64())
    }
    pub(super) fn cancel(&mut self) {
        self.state.contact = None;
        self.state.first = None;
        self.probe.cancel();
    }
    pub(super) fn prepare(&mut self, allowed: bool, size: Vec2) {
        self.probe.configure(self.options.mode);
        if !allowed
            || self.options.mode == Confirmation::Off
            || self.state.size.is_some_and(|s| s != size)
        {
            self.cancel();
        }
        self.state.size = Some(size);
        if let Some(mut contact) = self.state.contact {
            contact.target_seen |= self.probe.target().is_some();
            self.state.contact = Some(contact);
        }
        if self
            .state
            .first
            .is_some_and(|first| self.now() - first.time > DOUBLE_SECONDS)
        {
            self.state.first = None;
            if self.state.contact.is_none() {
                self.probe.cancel();
            }
        }
        if self.state.contact.is_none() && self.state.first.is_none() {
            self.probe.cancel();
        }
    }
    /// `eligible` is the parent's actual unclaimed world/role admission, never UI.
    /// Return only whether the parent's independent VATS tap gesture must yield.
    pub(super) fn event(&mut self, event: &TouchInput, eligible: bool) -> bool {
        if self.options.mode == Confirmation::Off {
            return false;
        }
        if event.phase == TouchPhase::Started {
            if let Some(mut old) = self.state.contact {
                if old.id != event.id {
                    old.dragged = true;
                    self.probe.cancel();
                    self.state.contact = Some(old);
                }
                return old.id == event.id && old.target_seen;
            }
            if !eligible || !event.position.is_finite() {
                self.probe.cancel();
                self.state.first = None;
                return false;
            }
            let expected = self
                .state
                .first
                .filter(|first| {
                    self.now() - first.time <= DOUBLE_SECONDS
                        && first.position.distance(event.position) <= DRAG * 2.
                        && first.generation == self.probe.generation()
                })
                .and_then(|_| self.probe.target().map(|t| t.form_id));
            self.probe.begin(
                ScreenPoint {
                    window: event.window,
                    id: event.id,
                    position: event.position,
                },
                expected,
            );
            self.state.contact = Some(Contact {
                id: event.id,
                origin: event.position,
                dragged: false,
                second: expected.is_some(),
                target_seen: false,
            });
            self.state.first = None;
            return true;
        }
        let Some(mut contact) = self.state.contact.filter(|c| c.id == event.id) else {
            return false;
        };
        contact.target_seen |= self.probe.target().is_some();
        contact.dragged |=
            !event.position.is_finite() || event.position.distance(contact.origin) > DRAG;
        // Hold confirmation cannot act on a quick release. Until an actual
        // target has been sampled, its pending ray must not claim the separate
        // optional VATS tap gesture. Tap/double-tap still reserve their pending
        // confirmation, and a sampled Hold target keeps priority.
        let suppress = contact.target_seen
            || (self.options.mode != Confirmation::Hold && self.probe.might_activate());
        if contact.dragged || event.phase == TouchPhase::Canceled {
            self.probe.cancel();
        }
        if matches!(event.phase, TouchPhase::Ended | TouchPhase::Canceled) {
            if !contact.dragged && event.phase == TouchPhase::Ended {
                match self.options.mode {
                    Confirmation::Tap => self.probe.end(true),
                    Confirmation::DoubleTap if contact.second => self.probe.end(true),
                    Confirmation::DoubleTap => {
                        self.probe.end(false);
                        self.state.first = Some(FirstTap {
                            time: self.now(),
                            position: event.position,
                            generation: self.probe.generation(),
                        });
                    }
                    Confirmation::Hold | Confirmation::Off => self.probe.cancel(),
                }
            } else {
                self.state.first = None;
            }
            self.state.contact = None;
        } else {
            self.state.contact = Some(contact);
        }
        suppress
    }
}

fn name(mode: Confirmation) -> &'static str {
    match mode {
        Confirmation::Off => "Off",
        Confirmation::Tap => "Tap",
        Confirmation::DoubleTap => "Double tap",
        Confirmation::Hold => "Hold",
    }
}
fn pick(world: &mut World, value: &str) -> bool {
    let Some(mode) = [
        Confirmation::Off,
        Confirmation::Tap,
        Confirmation::DoubleTap,
        Confirmation::Hold,
    ]
    .into_iter()
    .find(|m| name(*m) == value) else {
        return false;
    };
    world.resource_mut::<Options>().mode = mode;
    world.resource_mut::<TouchActivation>().configure(mode);
    true
}
#[derive(Component)]
struct Root;
#[derive(Component)]
struct Label;
#[derive(Component)]
struct Segment(usize);
fn spawn(mut commands: Commands) {
    commands
        .spawn((
            Root,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(180.),
                height: Val::Px(100.),
                ..default()
            },
            Visibility::Hidden,
            GlobalZIndex(90),
            Pickable::IGNORE,
        ))
        .with_children(|p| {
            p.spawn((
                Label,
                Text::default(),
                TextFont {
                    font_size: FontSize::Px(16.),
                    ..default()
                },
                TextColor(Color::WHITE),
                Pickable::IGNORE,
            ));
            for i in 0..24 {
                let angle = i as f32 * std::f32::consts::TAU / 24.;
                p.spawn((
                    Segment(i),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(88. + angle.sin() * 24.),
                        top: Val::Px(68. - angle.cos() * 24.),
                        width: Val::Px(4.),
                        height: Val::Px(6.),
                        ..default()
                    },
                    UiTransform {
                        rotation: Rot2::radians(angle),
                        ..default()
                    },
                    BackgroundColor(Color::WHITE),
                    Pickable::IGNORE,
                ));
            }
        });
}
fn show(
    state: Res<GestureState>,
    probe: Res<TouchActivation>,
    play: Option<Res<State<nvf_play::PlayState>>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut root: Query<(&mut Node, &mut Visibility), With<Root>>,
    mut labels: Query<&mut Text, With<Label>>,
    mut segments: Query<(&Segment, &mut Visibility), Without<Root>>,
) {
    let Some((point, target)) = probe
        .point()
        .zip(probe.target())
        .filter(|_| state.contact.is_some() || state.first.is_some())
        .filter(|_| {
            play.as_ref()
                .is_none_or(|s| *s.get() == nvf_play::PlayState::Playing)
        })
    else {
        if let Ok((_, mut visible)) = root.single_mut() {
            *visible = Visibility::Hidden;
        }
        return;
    };
    let Ok(window) = windows.single() else {
        return;
    };
    if let Ok((mut node, mut visible)) = root.single_mut() {
        *visible = Visibility::Inherited;
        node.left = Val::Px((point.position.x - 90.).clamp(0., (window.width() - 180.).max(0.)));
        node.top = Val::Px((point.position.y - 90.).clamp(0., (window.height() - 100.).max(0.)));
    }
    for mut text in &mut labels {
        text.0 = format!("{}\n{}", target.name, target.action.label());
    }
    let steps = (probe.progress() * 24.).floor() as usize;
    for (segment, mut visible) in &mut segments {
        *visible = if segment.0 < steps {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}
pub(super) fn install(app: &mut App) {
    app.init_resource::<Options>()
        .init_resource::<GestureState>()
        .init_resource::<TouchActivation>()
        .add_systems(Startup, spawn)
        .add_systems(Update, show);
    settings::add_choice_list(
        app,
        Setting {
            key: "touch-world-activation-v1",
            group: "Controls",
            label: "World touch confirmation",
            note: "Touch a nearby target outside controls to interact. Drag cancels; movement and look keep their areas. Hold shows a confirmation ring. Range, walls, locks and scripts still apply.",
            switches: "",
            get: |_| false,
            set: |_, _| {},
            slider: None,
        },
        ChoiceList {
            names: |_| {
                [
                    Confirmation::Off,
                    Confirmation::Tap,
                    Confirmation::DoubleTap,
                    Confirmation::Hold,
                ]
                .into_iter()
                .map(|m| name(m).into())
                .collect()
            },
            chosen: |w| name(w.resource::<Options>().mode).into(),
            pick,
            save: |w| name(w.resource::<Options>().mode).into(),
            load: pick,
            ask_keep: false,
        },
    );
}
