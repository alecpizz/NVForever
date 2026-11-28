//! One-shot touch envelope, resolved inside the existing MenuInput consumer.
//! Exact data stays here; the Copy command carries only a bounded live token.
use super::*;

pub(super) enum Expected {
    Row { row: RowToken, stage: Option<bool> },
    Scroll(Side, i32),
    Command(Command),
    Accept(Quote),
    Amount { prompt: Prompt, row: RowToken, amount: Option<i32>, command: Command },
}
struct Armed { token: u64, id: u64, frame: Frame, identity: Identity, revision: u64, expected: Expected }
#[derive(Resource, Default)]
pub(super) struct Guards { next: u64, armed: Option<Armed> }
impl Guards {
    pub(super) fn arm(&mut self, id: u64, frame: Frame, identity: Identity, revision: u64, expected: Expected) -> u64 {
        self.next = self.next.wrapping_add(1).max(1);
        self.armed = Some(Armed { token: self.next, id, frame, identity, revision, expected });
        self.next
    }
}

/// Optional at old/headless consumers. Importantly this has no ScriptState read:
/// apply_input already owns its mutable runtime, which it lends to resolve.
#[derive(SystemParam)]
pub(crate) struct Commits<'w, 's> {
    guards: Option<ResMut<'w, Guards>>,
    state: Option<Res<'w, TouchBarter>>,
    options: Option<Res<'w, Options>>,
    live: LiveAdmission<'w, 's>,
}
impl Commits<'_, '_> {
    pub(crate) fn discard(&mut self, token: u64) {
        if let Some(guards) = self.guards.as_mut() {
            if guards.armed.as_ref().is_some_and(|armed| armed.token == token) { guards.armed = None; }
        }
    }
    pub(crate) fn resolve(&mut self, token: u64, m: &mut MenuModel, world: &mut fnv_sim::World<'_>,
        script: Option<&crate::script::ScriptState>) -> Option<[Option<Command>; 2]> {
        // An obsolete queued token must not discard a newer live proposal.
        let guards = self.guards.as_mut()?;
        if guards.armed.as_ref()?.token != token { return None; }
        // Consume the matching token before validation, so failed, canceled and
        // duplicate copies can never replay against a later context.
        let armed = guards.armed.take()?;
        let state = self.state.as_ref()?;
        let valid = self.options.as_ref().is_some_and(|o| o.enabled)
            && state.visible() && state.frame == Some(armed.frame) && state.identity == Some(armed.identity)
            && state.revision == armed.revision
            && session(m) == Some(armed.identity.session) && m.category == armed.identity.category
            && prompt(m) == armed.identity.prompt && self.live.allows(armed.frame, armed.id)
            && menu_allows(script, self.live.creator.as_deref(), m);
        if !valid { return None; }
        let command = match armed.expected {
            Expected::Row { row, stage } => {
                if m.quantity.is_some() || m.refresh(world).is_err() || !row.same(m)
                    || stage.is_some_and(|all| !m.row_is(&row.guard(m, true))
                        || !available(world, m, row, if all { row.count } else { 1 })) { None }
                else { Some(stage.map_or(Command::Hover(row.side, row.index), |all| if all { Command::Stack } else { Command::One })) }
            }
            Expected::Command(command) => Some(command),
            Expected::Scroll(side, by) => {
                return m.quantity.is_none().then_some([Some(Command::Focus(side)), Some(Command::Scroll(by))]);
            }
            Expected::Accept(expected) => {
                if m.quantity.is_some() || m.refresh(world).is_err() || Quote::of(m).as_ref() != Some(&expected) { None }
                else { Some(Command::Accept) }
            }
            Expected::Amount { prompt: expected, row, amount, command } => {
                if prompt(m) != Some(expected) || !row.same(m)
                    || amount.is_some_and(|n| m.quantity.is_none_or(|q| q.amount != n))
                    || (command == Command::AmountOk && !available(world, m, row, m.quantity?.amount)) { None }
                else { Some(command) }
            }
        };
        if command.is_none() { m.status = Some("Trade changed. Review the item and amount again.".into()); }
        command.map(|command| [Some(command), None])
    }
}
