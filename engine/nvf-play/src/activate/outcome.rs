//! What activating a target will do (the prompt) and what an activation's outcome does
//! to the game (messages, travel, menus) — pure, testable without Bevy's app.
//!
//! Prompt and message wording is NVForever's own; it is not a claim about the original's
//! interface text.

use fnv_sim::world::{Activation, Outcome};
use fnv_sim::FormId;
use fnv_util::words::trf;
use fnv_world::WorldIndex;

/// Lock level meaning "requires a key" [hypothesis, simulation.md §5].
pub const KEY_ONLY_LEVEL: u8 = 255;

/// What E would do to the target, for the prompt.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Pick up an item stack.
    Take { count: i32 },
    /// Open a container, or a door that opens in place.
    Open,
    /// Close a door that is open.
    Close,
    /// A door that teleports; the destination's name when known.
    Travel { to: Option<String> },
    /// Locked, and the player carries the key.
    Unlock,
    /// Locked, no key.
    Locked { level: u8 },
    /// A locked terminal the player's password note opens (the Show Use Password tweak,
    /// docs/design/tweaks.md).
    UsePassword,
    Talk,
    /// A living actor the sneaking player may pickpocket (`crate::pickpocket`).
    Steal,
    /// A dead actor: its inventory opens (combat C5).
    Search,
    /// Activators and furniture: the base form's script runs.
    Activate,
    /// A terminal: its screen opens (`crate::terminal`).
    Access,
    /// An activator that gives water (ACTI WNAM): the player drinks from it
    /// (`fnv_game::Game::drink_water`, docs/spec-notes/activators.md §9).
    Drink,
    /// A placed explosive (PGRE).
    Disarm,
    /// A creature with nothing to say, not hostile to the player. The prompt shows only
    /// its name and E does nothing (`crate::activate`).
    Name,
    /// A living actor hostile to the player: no prompt is drawn, the crosshair stays the
    /// plain one and E does nothing (issue #2536; `Targeted::prompted`).
    Hostile,
    /// A door whose reference is flagged Inaccessible (header bit 8): it shows its name
    /// with "Inaccessible" and E does nothing (docs/spec-notes/cells-refs.md §2).
    Inaccessible,
    /// Another player down in co-op, aimed at where the body lies: E revives them
    /// (`nvforever`'s `coop.rs`), not the world's activation, using `with` (its name, a
    /// Stimpak); `None`: this player has nothing to revive with. Never offered outside
    /// co-op [design].
    Revive { with: Option<String> },
    /// A knocked-out companion a Stimpak gets up (the player's Revive Unconscious
    /// Companions tweak, docs/design/tweaks.md).
    ReviveCompanion,
}

/// A lock level by the name the game gives it: the levels the editor offers, 0 Very Easy,
/// 25 Easy, 50 Average, 75 Hard, 100 Very Hard and Requires Key [documented: GECK wiki
/// "Non-NPC population", docs/spec-notes/level-design.md §7; the only levels placed locks
/// use, simulation.md §5 [observed]]; 255 is Requires Key [hypothesis, simulation.md §5].
/// Any other level is shown as a number.
pub fn lock_text(level: u8) -> String {
    match level {
        0 => "Very Easy".into(),
        25 => "Easy".into(),
        50 => "Average".into(),
        75 => "Hard".into(),
        100 => "Very Hard".into(),
        KEY_ONLY_LEVEL => "Requires Key".into(),
        _ => format!("level {level}"),
    }
}

/// A terminal's hacking difficulty (0 Very Easy … 4 Very Hard, 5 Requires Key,
/// `fnv_game::hacking::Difficulty`) as the lock level of the same name, so a terminal
/// still to hack is prompted as a lock is (the owner, 2026-10-22).
pub fn terminal_lock_level(code: u8) -> u8 {
    match code {
        0..=4 => code * 25,
        _ => KEY_ONLY_LEVEL,
    }
}

impl Action {
    /// The prompt's verb: plain functional words of NVForever's own ("Take", "Open",
    /// "Talk", ...), no game setting naming them in the data files [design, #4495; LEGAL.md:
    /// the engine's short functional phrases]. A lock's level is named as the editor's
    /// documentation names it ([`lock_text`]).
    pub fn label(&self) -> String {
        match self {
            Action::Take { .. } => "Take".into(),
            Action::Open => "Open".into(),
            Action::Close => "Close".into(),
            Action::Travel { to: Some(to) } => format!("Open: {to}"),
            Action::Travel { to: None } => "Open".into(),
            Action::Unlock => "Unlock (key)".into(),
            Action::Locked { level } => format!("Locked ({})", lock_text(*level)),
            // Our words for the tweak's prompt.
            Action::UsePassword => "Locked (Use Password)".into(),
            Action::Talk => "Talk".into(),
            // [hypothesis: the original's word for picking a pocket]
            Action::Steal => "Steal".into(),
            Action::Search => "Search".into(),
            Action::Activate => "Activate".into(),
            // [hypothesis, hacking.md §8: the original's word for a terminal]
            Action::Access => "Access".into(),
            // [hypothesis: the original's word; activators.md §9]
            Action::Drink => "Drink".into(),
            Action::Disarm => "Disarm".into(),
            Action::Name | Action::Hostile => String::new(),
            // [hypothesis: the flag's name, cells-refs.md §2]
            Action::Inaccessible => "Inaccessible".into(),
            // [design: the game's own rollover with the verb]
            Action::Revive { .. } => "Revive".into(),
            // Our word for the tweak's prompt.
            Action::ReviveCompanion => "Revive".into(),
        }
    }
}

/// The two prompt lines under the crosshair, as the original sets them: the action (the
/// key goes before it), then what it is done to [observed: the two lines' order on the
/// owner's screenshots, docs/spec-notes/reticle.md §3; the words are [`Action::label`]'s]. A stack is named with its count; a door that leads
/// elsewhere by where it leads, with the action "Open" [hypothesis, reticle.md §5].
pub fn prompt(name: &str, action: &Action) -> (String, String) {
    match action {
        Action::Take { count } if *count > 1 => (action.label(), format!("{name} ({count})")),
        Action::Travel { to: Some(to) } => ("Open".into(), to.clone()),
        // [design: "E) Revive <name> (Stimpak)"]
        Action::Revive { with: Some(with) } => (action.label(), format!("{name} ({with})")),
        _ => (action.label(), name.to_string()),
    }
}

/// Where a door's destination cell is, from the world index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Destination {
    Interior(u32),
    /// An exterior cell (or a persistent cell) of this worldspace.
    Exterior { world: u32 },
    /// No cell, or a cell the index does not know.
    Unknown,
}

/// Interior cells are those without a worldspace (`fnv_world::Cell::world`), as
/// nvforever's `--cell` decides it.
pub fn destination(index: &WorldIndex, cell: Option<FormId>) -> Destination {
    match cell.and_then(|c| index.cells.get(&c.0)) {
        Some(c) => match c.world {
            None => Destination::Interior(c.form_id),
            Some(world) => Destination::Exterior { world },
        },
        None => Destination::Unknown,
    }
}

/// Compass heading in degrees (0 north, 90 east, in [0, 360)) on arrival through a door,
/// from the XTEL rotation's z angle: references store radians, and a z rotation turns
/// clockwise seen from above (docs/spec-notes/world.md §4 [observed]), so a z of 0 faces
/// north (+Y) and the angle in degrees is the compass heading [hypothesis: that the
/// actor faces the model's +Y; not captured for teleports].
pub fn heading_deg(rot: [f32; 3]) -> f32 {
    rot[2].to_degrees().rem_euclid(360.0)
}

/// What the game must do after an activation.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// The reference left (or came back into) the world: visuals and collision follow
    /// (`crate::RefChanged`).
    RefChanged(u32),
    /// `crate::Travel`: into an interior (`interior`), or out into the worldspace `world`
    /// (the one being played or another).
    Travel { world: Option<u32>, interior: Option<u32>, feet: [f32; 3], heading_deg: f32 },
    OpenContainer(u32),
    /// A short on-screen message.
    Message(String),
}

/// The effects of activating `target` (whose name is `name`). `name_of` names other
/// forms (items picked up, keys).
pub fn effects(a: &Activation, target: u32, name: &str, index: &WorldIndex, name_of: impl Fn(FormId) -> String) -> Vec<Effect> {
    let mut out = Vec::new();
    if let Some(k) = a.unlocked_with {
        out.push(Effect::Message(format!("Unlocked with {}", name_of(k))));
    }
    let nothing = || Effect::Message(format!("{name}: nothing happens"));
    match &a.outcome {
        Outcome::PickedUp { item, count, .. } => {
            out.push(Effect::RefChanged(target));
            let n = name_of(*item);
            out.push(Effect::Message(if *count > 1 { trf("{name} ({count}) added", &[("name", &n), ("count", count)]) } else { trf("{name} added", &[("name", &n)]) }));
        }
        Outcome::Disarmed { item, placed } => {
            out.push(Effect::RefChanged(target));
            out.extend(placed.map(|p| Effect::RefChanged(p.0)));
            out.push(Effect::Message(format!("{} disarmed", name_of(*item))));
        }
        Outcome::OpenedContainer { container } => out.push(Effect::OpenContainer(container.0)),
        Outcome::Locked { level } => out.push(Effect::Message(format!("{name} is locked ({})", lock_text(*level)))),
        Outcome::Teleported { cell, pos, rot, .. } => {
            // Outdoors the destination always names its worldspace; whether that is a
            // change of worldspace is for the receivers to see (`crate::Travel::destination`).
            let (world, interior) = match destination(index, *cell) {
                Destination::Interior(c) => (None, Some(c)),
                Destination::Exterior { world } => (Some(world), None),
                Destination::Unknown => {
                    out.push(Effect::Message(format!("{name}: the destination cell is unknown")));
                    return out;
                }
            };
            out.push(Effect::Travel { world, interior, feet: *pos, heading_deg: heading_deg(*rot) });
        }
        Outcome::DoorToggled { open } => out.push(Effect::Message(format!("{name} {}", if *open { "opened" } else { "closed" }))),
        // An actor with a greeting is talked to before this is reached (`script::by_hand`,
        // `crate::dialogue`); this is the actor that has none, or a game without scripts.
        Outcome::Talk { .. } => out.push(Effect::Message(format!("{name} has nothing to say"))),
        // Its script's OnActivate block does whatever it does (`crate::script`).
        Outcome::Activated if a.script.is_some() => {}
        Outcome::Activated | Outcome::Disabled | Outcome::NoDefault { .. } => out.push(nothing()),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use fnv_world::Cell;

    const WORLD: u32 = 0xDA726; // any FormID: the synthetic worldspace
    const OTHER_WORLD: u32 = 0x1_0000;
    const INTERIOR: u32 = 0x100;
    const EXTERIOR: u32 = 0x200;
    const ELSEWHERE: u32 = 0x300;

    fn index() -> WorldIndex {
        let mut ix = WorldIndex::default();
        for (id, world) in [(INTERIOR, None), (EXTERIOR, Some(WORLD)), (ELSEWHERE, Some(OTHER_WORLD))] {
            ix.cells.insert(id, Cell { form_id: id, world, ..Default::default() });
        }
        ix
    }

    fn act(outcome: Outcome) -> Activation {
        Activation { outcome, unlocked_with: None, script: None }
    }

    fn names(id: FormId) -> String {
        format!("form {:X}", id.0)
    }

    fn run(a: &Activation) -> Vec<Effect> {
        effects(a, 0x55, "Thing", &index(), names)
    }

    #[test]
    fn interior_detection() {
        let ix = index();
        assert_eq!(destination(&ix, Some(FormId(INTERIOR))), Destination::Interior(INTERIOR));
        assert_eq!(destination(&ix, Some(FormId(EXTERIOR))), Destination::Exterior { world: WORLD });
        assert_eq!(destination(&ix, Some(FormId(0x999))), Destination::Unknown);
        assert_eq!(destination(&ix, None), Destination::Unknown);
    }

    #[test]
    fn pick_up_hides_the_reference() {
        let e = run(&act(Outcome::PickedUp { item: FormId(0x15169), count: 3, owner: None }));
        assert_eq!(e, vec![Effect::RefChanged(0x55), Effect::Message("form 15169 (3) added".into())]);
        let e = run(&act(Outcome::Disarmed { item: FormId(0x77), placed: None }));
        assert_eq!(e[0], Effect::RefChanged(0x55));
    }

    #[test]
    fn container_opens_the_menu() {
        let mut a = act(Outcome::OpenedContainer { container: FormId(0x55) });
        assert_eq!(run(&a), vec![Effect::OpenContainer(0x55)]);
        a.unlocked_with = Some(FormId(0x42));
        assert_eq!(run(&a), vec![Effect::Message("Unlocked with form 42".into()), Effect::OpenContainer(0x55)]);
    }

    #[test]
    fn doors_travel_into_interiors_and_out() {
        let half_turn = [0.0, 0.0, std::f32::consts::PI];
        let a = act(Outcome::Teleported { to_door: FormId(1), cell: Some(FormId(INTERIOR)), pos: [1.0, 2.0, 3.0], rot: half_turn });
        assert_eq!(run(&a), vec![Effect::Travel { world: None, interior: Some(INTERIOR), feet: [1.0, 2.0, 3.0], heading_deg: 180.0 }]);
        let a = act(Outcome::Teleported { to_door: FormId(1), cell: Some(FormId(EXTERIOR)), pos: [4.0, 5.0, 6.0], rot: [0.1, 0.2, -std::f32::consts::FRAC_PI_2] });
        assert_eq!(run(&a), vec![Effect::Travel { world: Some(WORLD), interior: None, feet: [4.0, 5.0, 6.0], heading_deg: 270.0 }]);
        // An unknown cell: no travel, a message.
        for cell in [Some(FormId(0x999)), None] {
            let e = run(&act(Outcome::Teleported { to_door: FormId(1), cell, pos: [0.0; 3], rot: [0.0; 3] }));
            assert!(matches!(e.as_slice(), [Effect::Message(_)]), "{cell:?}: {e:?}");
        }
    }

    #[test]
    fn doors_travel_into_another_worldspace() {
        // A gate whose destination cell belongs to another worldspace (the Mojave's north
        // gate into FreesideNorthWorld is such a door in FalloutNV.esm).
        let a = act(Outcome::Teleported { to_door: FormId(2), cell: Some(FormId(ELSEWHERE)), pos: [8288.0, 6896.0, 1008.0], rot: [0.0; 3] });
        assert_eq!(
            run(&a),
            vec![Effect::Travel { world: Some(OTHER_WORLD), interior: None, feet: [8288.0, 6896.0, 1008.0], heading_deg: 0.0 }]
        );
        let mut unlocked = a.clone();
        unlocked.unlocked_with = Some(FormId(0x42));
        let e = run(&unlocked);
        assert_eq!(e[0], Effect::Message("Unlocked with form 42".into()));
        assert!(matches!(e[1], Effect::Travel { world: Some(OTHER_WORLD), interior: None, .. }));
    }

    #[test]
    fn everything_else_is_a_message() {
        for o in [
            Outcome::Locked { level: 50 },
            Outcome::DoorToggled { open: true },
            Outcome::Talk { actor: FormId(9) },
            Outcome::Activated,
            Outcome::Disabled,
            Outcome::NoDefault { sig: "LVLI".into() },
        ] {
            let e = run(&act(o.clone()));
            assert!(matches!(e.as_slice(), [Effect::Message(_)]), "{o:?}: {e:?}");
        }
        assert_eq!(run(&act(Outcome::Locked { level: 255 })), vec![Effect::Message("Thing is locked (Requires Key)".into())]);
    }

    #[test]
    fn a_scripted_activation_says_nothing() {
        let a = Activation { script: Some(FormId(7)), ..act(Outcome::Activated) };
        assert_eq!(run(&a), vec![]);
    }

    #[test]
    fn prompts() {
        let p = |a: &str, b: &str| (a.to_string(), b.to_string());
        assert_eq!(prompt("Stimpak", &Action::Take { count: 3 }), p("Take", "Stimpak (3)"));
        assert_eq!(prompt("Stimpak", &Action::Take { count: 1 }), p("Take", "Stimpak"));
        assert_eq!(prompt("Door", &Action::Travel { to: Some("Goodsprings".into()) }), p("Open", "Goodsprings"));
        assert_eq!(prompt("Door", &Action::Travel { to: None }), p("Open", "Door"));
        assert_eq!(prompt("Safe", &Action::Locked { level: 50 }), p("Locked (Average)", "Safe"));
        assert_eq!(Action::Locked { level: 255 }.label(), "Locked (Requires Key)");
        // A terminal still to hack reads as a lock of its difficulty's name.
        assert_eq!(Action::Locked { level: terminal_lock_level(1) }.label(), "Locked (Easy)");
        assert_eq!(Action::Locked { level: terminal_lock_level(4) }.label(), "Locked (Very Hard)");
        assert_eq!(Action::Locked { level: terminal_lock_level(5) }.label(), "Locked (Requires Key)");
    }

    #[test]
    fn headings_wrap() {
        assert_eq!(heading_deg([0.0; 3]), 0.0);
        assert!((heading_deg([0.0, 0.0, 7.0]) - (7f32.to_degrees() - 360.0)).abs() < 1e-3);
    }
}
