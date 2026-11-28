//! The settings page's Co-op rows (docs/design/coop-networking.md §10): how this game
//! connects to other players. They set `nvf_coop::prefs`, which the network code reads when
//! a lobby opens or a join starts, so a change applies to the next lobby or join.

use bevy::prelude::*;
use nvf_coop::prefs::{self, Connection};
use nvf_play::settings::{add_choice_list, add_setting, ChoiceList, Setting};

pub const CONNECTION_KEY: &str = "coop-connection";
pub const PORT_KEY: &str = "coop-router-port";
pub const RELAY_KEY: &str = "coop-relay";
pub const ASK_KEY: &str = "coop-ask";

fn connection_pick(_: &mut World, name: &str) -> bool {
    match Connection::by_name(name) {
        Some(c) => {
            prefs::set_connection(c);
            true
        }
        None => false,
    }
}

pub fn register(app: &mut App) {
    let group = nvf_play::pause::GROUP_MENUS;
    add_choice_list(
        app,
        Setting {
            key: CONNECTION_KEY,
            group,
            label: "Co-op connection",
            note: "Automatic: online codes, and through another player when needed. Direct only: never through another player. LAN only: players on this network only.",
            switches: "",
            get: |_| false,
            set: |_, _| {},
            slider: None,
        },
        ChoiceList {
            names: |_| Connection::ALL.iter().map(|c| c.name().to_string()).collect(),
            chosen: |_| prefs::connection().name().to_string(),
            pick: connection_pick,
            save: |_| prefs::connection().name().to_string(),
            load: connection_pick,
            ask_keep: false,
        },
    );
    add_setting(
        app,
        Setting {
            key: PORT_KEY,
            group,
            label: "Open router port automatically",
            note: "On: your router is asked to let co-op players in (UPnP, NAT-PMP or PCP), and the port closes when you leave. Applies to the next lobby or join.",
            switches: "",
            get: |_| prefs::port_mapping(),
            set: |_, on| prefs::set_port_mapping(on),
            slider: None,
        },
    );
    add_setting(
        app,
        Setting {
            key: RELAY_KEY,
            group,
            label: "Relay for other players",
            note: "On: a player in your lobby who cannot reach the host may play through your game (at most 2, about 15 KB/s each). Their traffic stays encrypted.",
            switches: "",
            get: |_| prefs::relay_for_others(),
            set: |_, on| prefs::set_relay_for_others(on),
            slider: None,
        },
    );
    add_setting(
        app,
        Setting {
            key: ASK_KEY,
            group,
            label: "Ask before players join",
            note: "On: when you host, a player joining by online code waits until you let them in from the Co-op Lobby. Players on your network join at once.",
            switches: "",
            get: |_| prefs::ask_before_join(),
            set: |_, on| prefs::set_ask_before_join(on),
            slider: None,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_coop_rows_register_with_short_notes() {
        let mut app = App::new();
        register(&mut app);
        let settings = app.world().resource::<nvf_play::settings::Settings>();
        for key in [CONNECTION_KEY, PORT_KEY, RELAY_KEY, ASK_KEY] {
            let i = settings.index(key).unwrap_or_else(|| panic!("{key} registered"));
            assert!(nvf_play::settings::note_over_budget(settings.list()[i].note).is_none(), "{key}: note within budget");
        }
        let i = settings.index(CONNECTION_KEY).unwrap();
        let list = settings.choices(i).unwrap();
        assert_eq!((list.names)(app.world()), ["Automatic", "Direct only", "LAN only"]);
    }
}
