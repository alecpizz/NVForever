//! A playable, one-shot demonstration of the ordinary SetWeather transition.
use bevy::prelude::*;
use nvf_play::{console::no_console, pause::Paused, script::ScriptState, PlayState, PlayWorld};

use crate::sky::SkyState;

pub fn install(app: &mut App, target: &str) {
    app.insert_resource(Preview { target: target.to_owned(), legs: None })
        .add_systems(Update, drive.before(crate::script_weather).run_if(resource_exists::<Preview>).run_if(in_state(PlayState::Playing)).run_if(crate::world::world_not_held).run_if(no_console));
}

#[derive(Resource)]
struct Preview {
    target: String,
    legs: Option<Legs>,
}

struct Legs {
    start: u32,
    target: u32,
    stage: Stage,
}

enum Stage {
    Start(f32),
    Target(f32),
    Returning,
}

#[derive(Debug, PartialEq)]
enum Action {
    Wait,
    Set(u32, bool),
    Finished,
    Interrupted,
}

impl Legs {
    fn advance(&mut self, shown: u32, settled: bool, delta: f32) -> Action {
        let expected = match self.stage { Stage::Target(_) => self.target, _ => self.start };
        if shown != expected {
            return Action::Interrupted;
        }
        if !settled {
            return Action::Wait;
        }
        match &mut self.stage {
            Stage::Start(left) | Stage::Target(left) if *left > delta => {
                *left -= delta;
                Action::Wait
            }
            Stage::Start(_) => {
                self.stage = Stage::Target(7.0);
                Action::Set(self.target, true)
            }
            Stage::Target(_) => {
                self.stage = Stage::Returning;
                // --weather already pins the starting climate. Leave no held script override.
                Action::Set(self.start, false)
            }
            Stage::Returning => Action::Finished,
        }
    }
}

fn drive(
    mut commands: Commands, time: Res<Time>, paused: Res<Paused>, world: Res<PlayWorld>,
    sky: Res<SkyState>, mut script: ResMut<ScriptState>, mut preview: ResMut<Preview>,
) {
    if paused.0 || time.delta_secs() <= 0.0 || sky.interior.is_some() {
        return;
    }
    let Some(rt) = script.rt.as_mut() else { return };
    if rt.game.weather.request.is_some() || rt.game.weather.release {
        info!("weather preview: stopped for another weather request");
        commands.remove_resource::<Preview>();
        return;
    }
    if preview.legs.is_none() {
        let id = fnv_game::runner::resolve(&rt.game, &preview.target).ok();
        let Some(id) = id.filter(|id| fnv_world::sky::weather(world.lo, world.schema, id.0).is_some()) else {
            warn!("weather preview: unknown weather {}", preview.target);
            commands.remove_resource::<Preview>();
            return;
        };
        if id.0 == sky.weather.form_id {
            warn!("weather preview: target is already the starting weather");
            commands.remove_resource::<Preview>();
            return;
        }
        preview.legs = Some(Legs { start: sky.weather.form_id, target: id.0, stage: Stage::Start(8.0) });
    }
    let Some(legs) = preview.legs.as_mut() else { return };
    match legs.advance(sky.weather.form_id, sky.prev.is_none(), time.delta_secs()) {
        Action::Wait => return,
        Action::Set(id, hold) => {
            let args = [format!("{id:08X}"), u8::from(hold).to_string()];
            match fnv_game::functions::console::call_words(&mut rt.game, None, "SetWeather", &args, "weather preview") {
                Ok(_) => {
                    info!("weather preview: SetWeather {} {}", args[0], args[1]);
                    return;
                }
                Err(e) => warn!("weather preview: SetWeather failed: {e}"),
            }
        }
        Action::Finished => info!("weather preview: return fade settled"),
        Action::Interrupted => info!("weather preview: stopped after the weather changed independently"),
    }
    commands.remove_resource::<Preview>();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_holds_only_settled_weather_and_issues_each_leg_once() {
        let mut legs = Legs { start: 1, target: 2, stage: Stage::Start(8.0) };
        assert_eq!(legs.advance(1, true, 7.0), Action::Wait);
        assert_eq!(legs.advance(1, true, 1.0), Action::Set(2, true));
        assert_eq!(legs.advance(2, false, 40.0), Action::Wait);
        assert_eq!(legs.advance(2, true, 6.0), Action::Wait);
        assert_eq!(legs.advance(2, true, 1.0), Action::Set(1, false));
        assert_eq!(legs.advance(1, false, 40.0), Action::Wait);
        assert_eq!(legs.advance(1, true, 0.1), Action::Finished);
        assert_eq!(legs.advance(3, true, 0.1), Action::Interrupted);
    }
}
