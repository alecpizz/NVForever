//! Ending a lobby [design].
//!
//! - **The host ends it** when it goes back to the main menu or quits the game, as well as
//!   with the lobby screen's Close ([`host_ends`]): it says goodbye to every guest
//!   (`nvf_coop::net`, the goodbye sent [`nvf_coop::net::BYE_COPIES`] times, as a hit is)
//!   and stops serving. The run that brings up the main menu does not host again, so the
//!   new-game world behind the menu never reaches a guest (the new run would take the
//!   lobby over and the guests would see a default Courier there).
//! - **A guest whose host ended the lobby** ([`guest_leaves`]), told so or after
//!   `nvf_coop::net::GRACE` without a word (the host's game stopped or the connection is
//!   gone): this game autosaves first. The save is the guest's own game with its
//!   character as it is now (`nvf_play::save::SoloBase`). Then it goes to the main menu,
//!   where a message box says [`ENDED_TEXT`] ([`tell_ended`]). Continue loads that save.
//!   The same words for a host that was lost [design: one line, to be worded
//!   separately later if wanted].
//! - **A guest that goes to the main menu itself** autosaves the same way before the
//!   hand-over to the menu's run ([`HoldLeave`], waited for by `crate::handoff`). A guest
//!   that quits the game is not saved [design: quitting does not save in the original
//!   either].
//!
//! Log: `co-op: the host ends the lobby (<why>)`, `co-op: the host ended the lobby:
//! autosaving this player's own game`, `co-op: autosaved (<path>): to the main menu`,
//! `co-op: the main menu says "<text>"`.

use super::*;
use fnv_util::words::tr_str;

/// What a guest is told at the main menu after its host ended the lobby [design]: the
/// English key, shown through `tr` when the box is made (`tell_ended`).
pub const ENDED_TEXT: &str = "Host has ended the lobby.";
/// Carries [`ENDED_TEXT`] into the main menu's run (`crate::handoff` keeps the variables).
pub const ENDED_ENV: &str = "NVF_COOP_ENDED";
/// Set when a lobby ends, for the main menu's run: it neither hosts nor joins
/// (`--host` and `--join` stay on its command line).
pub const NO_LOBBY_ENV: &str = "NVF_COOP_NO_LOBBY";
/// The longest a leaving guest waits for its autosave (seconds, real time) [design].
const SAVE_WAIT: f64 = 10.0;

/// A guest's way out, while it saves.
#[derive(Default)]
pub(super) enum Leaving {
    #[default]
    No,
    /// The host ended the lobby: autosave, then the main menu with a message.
    Ended { since: f64, asked: bool },
    /// The player chose the main menu: autosave, then let the hand-over go on.
    Quit { since: f64, asked: bool },
}

/// Whether the hand-over to the main menu's run waits (`crate::handoff::leave`): while
/// this game is a guest in a host's world (`guest`), until its autosave is written or
/// given up (`saved`).
#[derive(Resource, Default)]
pub struct HoldLeave {
    pub guest: bool,
    pub saved: bool,
}

impl HoldLeave {
    pub fn holds(&self) -> bool {
        self.guest && !self.saved
    }
}

/// The host ends its lobby as it goes to the main menu or quits (module notes).
pub(super) fn host_ends(
    mut coop: ResMut<Coop>,
    (mut to_menu, mut exit): (MessageReader<nvf_play::pause::MainMenuRequest>, MessageReader<AppExit>),
) {
    let why = match (to_menu.read().last(), exit.read().last()) {
        (Some(_), _) => "back to the main menu",
        (_, Some(_)) => "the game quits",
        _ => return,
    };
    if coop.is_host() {
        info!("co-op: the host ends the lobby ({why}): the guests are told");
        coop.stop_hosting();
        std::env::set_var(NO_LOBBY_ENV, "1");
    }
}

/// A guest leaves (module notes): autosave, then the main menu.
#[allow(clippy::too_many_arguments)]
pub(super) fn guest_leaves(
    mut commands: Commands,
    real: Res<Time<Real>>,
    mut coop: ResMut<Coop>,
    (mut to_menu, mut hold): (MessageReader<nvf_play::pause::MainMenuRequest>, ResMut<HoldLeave>),
    (mut saves, mut done): (MessageWriter<nvf_play::save::SaveRequest>, MessageReader<nvf_play::save::SaveDone>),
    solo: Option<Res<nvf_play::save::SoloBase>>,
) {
    let now = real.elapsed_secs_f64();
    let quit = to_menu.read().last().is_some();
    let saved = done.read().find(|d| d.result.is_ok()).map(|d| d.path.clone());
    // Only a guest that was in a host's world has a game of its own to keep.
    let in_world = solo.as_deref().is_some_and(|s| s.0.is_some());
    if matches!(coop.leaving, Leaving::No) {
        hold.guest = coop.is_guest() && in_world;
        hold.saved = false;
        if coop.ended {
            coop.ended = false;
            if in_world {
                info!("co-op: the host ended the lobby: autosaving this player's own game");
                coop.leaving = Leaving::Ended { since: now, asked: false };
            }
        } else if quit && coop.is_guest() && in_world {
            info!("co-op: leaving the host's lobby for the main menu: autosaving this player's own game first");
            coop.leaving = Leaving::Quit { since: now, asked: false };
        }
    }
    let (since, asked) = match &mut coop.leaving {
        Leaving::No => return,
        Leaving::Ended { since, asked } | Leaving::Quit { since, asked } => (*since, asked),
    };
    if !*asked {
        *asked = true;
        saves.write(nvf_play::save::SaveRequest { name: String::new(), kind: fnv_save::SaveKind::Auto });
        return;
    }
    let late = now - since >= SAVE_WAIT;
    if saved.is_none() && !late {
        return;
    }
    match &saved {
        Some(p) => info!("co-op: autosaved ({}): to the main menu", p.display()),
        None => warn!("co-op: no autosave after {SAVE_WAIT} s: to the main menu without it"),
    }
    // Still in the host's world until the main menu is up (`follow_npcs`).
    coop.parting = true;
    match std::mem::take(&mut coop.leaving) {
        Leaving::Ended { .. } => {
            std::env::set_var(ENDED_ENV, ENDED_TEXT);
            std::env::set_var(NO_LOBBY_ENV, "1");
            coop.stop_joining();
            // Through the commands: this system reads the same messages (one borrow).
            commands.write_message(nvf_play::pause::MainMenuRequest);
        }
        Leaving::Quit { .. } => {
            std::env::set_var(NO_LOBBY_ENV, "1");
            coop.stop_joining();
            hold.saved = true;
        }
        Leaving::No => {}
    }
}

/// The main menu's run after a host ended the lobby: the game's message box says so, once
/// the menu is up and the game's scripts run behind it (module notes).
pub(super) fn tell_ended(
    mut text: Local<Option<Option<String>>>,
    menu: Option<Res<nvf_play::main_menu::MainMenu>>,
    mut script: Option<ResMut<nvf_play::script::ScriptState>>,
) {
    let t = text.get_or_insert_with(|| {
        let t = std::env::var(ENDED_ENV).ok().filter(|t| !t.trim().is_empty());
        std::env::remove_var(ENDED_ENV);
        t
    });
    let Some(words) = t.clone() else { return };
    if !menu.is_some_and(|m| m.open) {
        return;
    }
    let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else { return };
    // The game's own box, as a script's MessageBoxEx shows it: its look and its OK.
    // In the language chosen now, not the one the text was made in.
    let shown = tr_str(&words).to_string();
    let line = format!("MessageBoxEx \"{}|{}\"", shown.replace('"', "'"), tr("OK").replace(['"', '|'], "'"));
    match rt.game.run_source(&line, Default::default()) {
        Ok(_) => info!("co-op: the main menu says \"{words}\""),
        Err(e) => warn!("co-op: the main menu cannot say \"{words}\": {e}"),
    }
    *t = None;
}
