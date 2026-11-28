//! Conversation data used by the Classic touch view.
use super::appearance::{Appearance, Theme};
use crate::dialogue::{self, ListId};
use crate::script::{with_world, ScriptState};
use crate::sim::SimState;
use bevy::prelude::*;
use fnv_game::{services, Game};
use fnv_sim::FormId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BarterTarget {
    pub merchant: u32,
    pub list: ListId,
}

#[derive(Resource, Default, Debug)]
pub(crate) struct State {
    pub spoken: Option<String>,
    pub speaking: bool,
    pub caps: Option<i32>,
    pub barter: Option<BarterTarget>,
    conversation: Option<(u32, FormId)>,
    response: Option<(u32, FormId, usize)>,
}

impl State {
    pub(crate) fn observe(&mut self, game: &Game, text: impl FnOnce(&Game) -> Option<String>) {
        let Some(c) = game.dialogue.as_ref() else {
            *self = Self::default();
            return;
        };
        let conversation = Some((game.dialogue_serial, c.actor));
        if self.conversation != conversation {
            *self = Self { conversation, ..Default::default() };
        }
        self.speaking = c.line.is_some();
        let response = c.line.as_ref().map(|l| (c.lines, l.info, l.response));
        if response.is_some() && response != self.response {
            self.spoken = text(game);
        }
        self.response = response;
    }
}

#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct Menus<'w> {
    active: Option<Res<'w, crate::menu::ActiveMenu>>,
    wait: Option<Res<'w, crate::wait::WaitMenu>>,
    creator: Option<Res<'w, crate::creator::CreatorMenu>>,
    repair: Option<Res<'w, crate::repair::RepairServices>>,
    recipe: Option<Res<'w, crate::crafting::RecipeMenu>>,
}

impl Menus<'_> {
    pub(crate) fn blocked(&self) -> bool {
        self.active.as_ref().is_some_and(|m| m.0.is_some())
            || self.wait.as_ref().is_some_and(|m| m.is_open())
            || self.creator.as_ref().is_some_and(|m| m.is_open())
            || self.repair.as_ref().is_some_and(|m| m.is_open())
            || self.recipe.as_ref().is_some_and(|m| m.is_open())
    }
}

fn merchant(game: &mut Game) -> Option<FormId> {
    let c = game.dialogue.as_ref()?;
    let actor = c.actor;
    if dialogue::held(game) || c.line.is_some() || c.ending || c.choices.is_empty() {
        return None;
    }
    let goods = services::WEAPONS | services::ARMOR | services::ALCOHOL | services::BOOKS
        | services::FOOD | services::CHEMS | services::STIMPAKS | services::MISC;
    if game.services(actor) & goods == 0 && game.merchant_container(actor).is_none() {
        return None;
    }
    if !game.world.get(actor).ok()?.enabled() || game.is_dead(actor) || game.fighting_target_of(actor) == Some(FormId::PLAYER_REF)
        || game.would_attack(actor, FormId::PLAYER_REF) {
        return None;
    }
    Some(actor)
}

/// Called after the touch release has been consumed. Revalidate the actor and list
/// in the current world before handing ownership to the normal barter menu.
pub(crate) fn request(game: &mut Game, list: Option<&ListId>) {
    let Some(list) = list else { return };
    if dialogue::list_id(game).as_ref() != Some(list) {
        return;
    }
    let Some(merchant) = merchant(game) else { return };
    game.barter_requests.push(fnv_game::game::BarterRequest { merchant, discount: 0 });
    game.open_menu(fnv_game::game::menus::BARTER);
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<State>().add_systems(
        Update,
        refresh.in_set(crate::PlaySet::Ui).before(crate::dialogue_menu::DrawMenu).before(super::draw),
    );
}

fn refresh(
    mut state: ResMut<State>,
    script: Option<ResMut<ScriptState>>,
    sim: Option<ResMut<SimState>>,
    play: Option<Res<bevy::prelude::State<crate::PlayState>>>,
    appearance: Option<Res<Appearance>>,
    paused: Option<Res<crate::pause::Paused>>,
    menus: Menus,
) {
    if play.as_ref().is_some_and(|s| *s.get() == crate::PlayState::Loading) {
        *state = State::default();
        return;
    }
    let Some(rt) = script.filter(|s| !s.held).and_then(|s| s.into_inner().rt.as_mut()) else {
        *state = State::default();
        return;
    };
    dialogue::observe_classic(&mut state, &rt.game);
    if rt.game.dialogue.is_none() || !appearance.is_some_and(|a| a.theme == Theme::ClassicGlass) {
        state.barter = None;
        state.caps = None;
        return;
    }
    let Some(mut sim) = sim else {
        state.barter = None;
        state.caps = None;
        return;
    };
    state.caps = sim.world.item_count(FormId::PLAYER_REF, crate::menu::model::CAPS).ok();
    state.barter = if play.is_some_and(|s| *s.get() == crate::PlayState::Dialogue)
        && !paused.is_some_and(|p| p.0) && !menus.blocked() {
        with_world(rt, &mut sim, |rt| {
            let merchant = merchant(&mut rt.game)?;
            Some(BarterTarget { merchant: merchant.0, list: dialogue::list_id(&rt.game)? })
        })
    } else {
        None
    };
}

#[cfg(test)]
pub(crate) fn refresh_fixture(world: &mut World) {
    use bevy::ecs::system::RunSystemOnce;
    world.run_system_once(refresh).unwrap();
}
