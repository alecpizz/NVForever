//! The player on furniture (docs/spec-notes/furniture.md). Activating a piece of
//! furniture asks for it ([`UseFurniture`]); whoever draws the player's body (nvforever's
//! `furniture.rs`) plays the clips and holds the player meanwhile ([`PlayerSeat`]):
//! movement, jumping and the character's collision are off, and pressing a movement
//! control asks to get up [design: how the original game ends sitting is not captured;
//! moving is what its players know], and so does Activate with nothing under the
//! crosshair [design: lying on a bed the furniture itself is out of view].

use bevy::prelude::*;

/// The player activated a piece of furniture (its default activation).
#[derive(Message, Clone, Copy, Debug)]
pub struct UseFurniture {
    pub reference: u32,
}

/// Furniture holding the player.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
pub struct PlayerSeat {
    /// The feet (game space) while furniture holds the player.
    pub hold: Option<[f32; 3]>,
    /// A movement control was pressed while held; cleared by whoever acts on it.
    pub leave: bool,
    /// The mouse's look since the holder last took it, while held: the heading and pitch
    /// it asked for, radians, the pitch not stopped at straight up or down. For a holder
    /// that turns the view in a frame of its own (a body lying on a bed).
    pub look: [f32; 2],
    /// The furniture is a bed and the player lies on it (from lying down until getting
    /// up starts): the Wait menu is a Sleep menu meanwhile (`crate::wait`).
    pub bed: bool,
    /// The furniture reference holding the player.
    pub on: Option<u32>,
    /// On it: neither getting on nor getting off (a slot machine's menu opens then,
    /// `crate::slots::seat`).
    pub seated: bool,
    /// Getting off it (its getting-off clip plays): co-op sends the player as off from
    /// now, so the other screens' puppet starts its own getting-off clip with this one
    /// (nvforever `coop.rs`). Set by nvforever's furniture; read only by co-op.
    pub getting_off: bool,
}

/// Tell the script runtime which furniture `actor` uses, so GetSitting and the other
/// furniture functions answer for it: `using` is the furniture reference, the marker ID
/// the actor entered by, and the phase value (`fnv_actor::furniture::state`); `None`
/// when the actor is off it.
pub fn set_use(script: &mut crate::script::ScriptState, actor: u32, using: Option<(u32, u8, u8)>) {
    use fnv_game::game::FurnitureUse;
    use fnv_sim::FormId;
    let using = using.map(|(reference, marker_id, state)| FurnitureUse { reference: FormId(reference), marker_id, state });
    // No runtime (scripts off or still loading): nothing to tell.
    if let Some(rt) = script.rt.as_mut() {
        rt.game.set_furniture_use(FormId(actor), using);
    }
}
