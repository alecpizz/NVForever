//! The pure parts of running Layer 5 scripts in the game (`script.rs`), testable without
//! Bevy's app or game data: what the runtime's trace events (`fnv_game::trace`) mean for
//! the engine, the player-inventory comparison that turns menu changes into OnAdd /
//! OnEquip / OnUnequip / OnDrop, trigger-volume geometry, and the script test aid's
//! parser. docs/design/layer5-play.md has the rules and their evidence tags.

use crate::activate::outcome::{Destination, Effect};
use crate::menu::model::CAPS;
use crate::Travel;
use fnv_game::trace::{Event, EventKind};
use fnv_sim::inventory::Inventory;
use fnv_sim::world::{Activation, Outcome};
use fnv_sim::FormId;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

/// What the engine does after script events.
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptEffect {
    /// A reference's presence changed (enabled, disabled, picked up by a scripted
    /// activation): visuals and collision follow (`crate::RefChanged`).
    RefChanged(u32),
    /// A reference enabled or disabled with a fade (Enable's FadeIn, Disable's FadeOut):
    /// it fades in, or out, instead of popping (`crate::fade`). Follows its
    /// `RefChanged`.
    Fade { form_id: u32, fade_in: bool },
    /// The player was moved by a script (`MoveTo`).
    Travel(Travel),
    /// The player went through a load door a script activated (the player's own trip,
    /// `crate::loading::Loading::arriving`).
    DoorTravel(Travel),
    /// A scripted activation opened a container for the player.
    OpenContainer(u32),
    /// A short line on screen (a message without the message-box flag, items a script
    /// gave the player, a default activation's message).
    Notify(Note),
    /// Caps a script gave the player with the message shown (not picked up by an
    /// activation, whose pick-up `crate::world_sounds` sounds already): the caps'
    /// pick-up sound at the listener (#3095).
    ItemAddedSound(FormId),
    /// A quest added, completed or failed, or objectives shown or completed: drawn at
    /// the left of the view (`crate::quest_messages`).
    Quest(QuestNotice),
    /// A message box (`MESG` flagged "Message Box", or one with buttons).
    MessageBox(BoxMessage),
    /// A script's `Activate` ran a piece of furniture's default activation for the
    /// player: the player uses it (`crate::furniture`).
    UseFurniture(u32),
    /// A script's `Activate` ran a terminal's default activation for the player: its
    /// screen opens (`crate::terminal`), as by hand. The Lucky 38 penthouse terminals'
    /// OnActivate blocks end in `Activate` on every branch [observed: FalloutNV.esm SCPT
    /// 0x001746C1 on Lucky38MrHouseTerminal]; before, nothing opened (#1532).
    OpenTerminal(u32),
}

/// A short line for the top left of the view (`crate::corner_messages`). A message
/// record's line has the record's icon and time; the others have neither.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Note {
    pub text: String,
    /// The icon's texture file as its Menu Icon record names it (under the textures
    /// folder); none for the default icon.
    pub icon: Option<String>,
    /// The seconds it stays (MESG Display Time); none for the default time.
    pub seconds: Option<f32>,
}

impl From<String> for Note {
    fn from(text: String) -> Note {
        Note { text, ..Note::default() }
    }
}

impl From<&str> for Note {
    fn from(text: &str) -> Note {
        text.to_string().into()
    }
}

/// What the quest area at the left of the view shows (docs/spec-notes/quest-messages.md).
#[derive(Debug, Clone, PartialEq)]
pub enum QuestNotice {
    /// "New quest" over the quest's name: its first objective was shown.
    Added(String),
    Completed(String),
    Failed(String),
    /// "Place found" over a map marker's name: the player came within its radius
    /// (`crate::compass`).
    Discovered(String),
    /// Objectives shown or completed, in the order it happened.
    Objectives(Vec<Objective>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Objective {
    /// The quest's FormID: a waiting list takes only its own quest's objectives.
    pub quest: u32,
    pub text: String,
    pub completed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BoxMessage {
    pub message: u32,
    pub text: String,
    /// Every button of the message, in record order.
    pub buttons: Vec<String>,
    /// The indices of the buttons whose conditions failed when it was shown: not drawn
    /// (`fnv_game::Game::hidden_buttons`).
    pub hidden: Vec<usize>,
}

impl BoxMessage {
    /// The runtime waits for a button (`fnv_game::game::MessageBox`, GetButtonPressed).
    pub fn waits(&self) -> bool {
        !self.buttons.is_empty()
    }

    /// The buttons drawn, top to bottom: each one's index in the record (what
    /// GetButtonPressed returns) and its text.
    pub fn shown(&self) -> Vec<(usize, &str)> {
        self.buttons.iter().enumerate().filter(|(i, _)| !self.hidden.contains(i)).map(|(i, b)| (i, b.as_str())).collect()
    }
}

/// What [`effects`] needs to know about the game.
pub trait EffectCtx {
    /// Display name of a base form or reference, for messages.
    fn name(&self, form: FormId) -> String;
    fn objective_text(&self, quest: FormId, index: i32) -> Option<String>;
    /// The quest's name (FULL) when it has one.
    fn quest_title(&self, quest: FormId) -> Option<String>;
    /// Whether an objective of the quest other than those in `besides` is shown or
    /// completed: the quest is in the player's list already.
    fn quest_listed(&self, quest: FormId, besides: &[i32]) -> bool;
    /// `MESG` DNAM "Message Box" flag.
    fn is_message_box(&self, message: FormId) -> bool;
    /// The player's tweaks (`fnv_game::tweaks`), when there is a game.
    fn tweaks(&self) -> Option<&fnv_game::tweaks::Tweaks> {
        None
    }
    /// The icon file and the seconds of a message that is not a box, as [`Note`] has
    /// them.
    fn message_look(&self, message: FormId) -> (Option<String>, Option<f32>);
    /// A skill's display name (its AVIF record's FULL, else the canonical actor-value
    /// name), for the line of a skill book read.
    fn skill_name(&self, code: u16) -> String {
        fnv_game::functions::AV_NAMES.get(code as usize).copied().unwrap_or("?").to_string()
    }
    /// References whose enable parent chain leads to `r` (their presence follows it).
    fn enable_children(&self, r: FormId) -> Vec<FormId>;
    /// Whether the reference's enable parent data has the "Pop In" flag.
    fn pops_in(&self, _r: FormId) -> bool {
        false
    }
    /// Whether `base` is an actor base (`fnv_game::ai::is_actor_base`).
    fn is_actor_base(&self, _base: FormId) -> bool {
        false
    }
    /// The player's compass heading in degrees (kept across a scripted MoveTo).
    fn player_heading_deg(&self) -> f32;
    /// The view's pitch in degrees a scripted MoveTo leaves the player with (negative
    /// looks down; the marker's, `fnv_game` `move_ref`); `None` keeps the view's.
    fn player_pitch_deg(&self) -> Option<f32> {
        None
    }
    fn destination(&self, cell: Option<FormId>) -> Destination;
    /// The effects of an activation's outcome (`activate::outcome::effects`).
    fn activation_effects(&self, a: &Activation, target: FormId) -> Vec<Effect>;
    /// Whether the reference's base is a piece of furniture.
    fn is_furniture(&self, _r: FormId) -> bool {
        false
    }
    /// Whether the reference's base is a terminal.
    fn is_terminal(&self, _r: FormId) -> bool {
        false
    }
    /// A string game setting's text (GMST), by Editor ID.
    fn setting_text(&self, _name: &str) -> Option<String> {
        None
    }
}

/// "<Name> is unconscious.": the name, then `sNoTalkUnConscious` ("is unconscious."
/// [observed: FalloutNV.esm]), that text without the masters.
pub fn unconscious_line(ctx: &dyn EffectCtx, actor: FormId) -> String {
    let tail = ctx.setting_text("sNoTalkUnConscious").map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).unwrap_or_else(|| "is unconscious.".into());
    format!("{} {tail}", ctx.name(actor))
}

/// `fnv_sim::world::Activation` as the trace serialises it (`EventKind::Activated`
/// carries the outcome as JSON); read back so that a scripted activation's outcome goes
/// through the same effects as one by hand.
#[derive(Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
enum OutcomeJson {
    PickedUp {
        item: FormId,
        count: i32,
        #[serde(default)]
        owner: Option<FormId>,
    },
    OpenedContainer {
        container: FormId,
    },
    Locked {
        level: u8,
    },
    Teleported {
        to_door: FormId,
        #[serde(default)]
        cell: Option<FormId>,
        pos: [f32; 3],
        rot: [f32; 3],
    },
    Disarmed {
        item: FormId,
        #[serde(default)]
        placed: Option<FormId>,
    },
    DoorToggled {
        open: bool,
    },
    Talk {
        actor: FormId,
    },
    Activated,
    Disabled,
    NoDefault {
        sig: String,
    },
}

#[derive(Deserialize)]
struct ActivationJson {
    #[serde(flatten)]
    outcome: OutcomeJson,
    #[serde(default)]
    unlocked_with: Option<FormId>,
    #[serde(default)]
    script: Option<FormId>,
}

/// An activation outcome from its trace JSON.
pub fn activation_from_json(v: &serde_json::Value) -> Option<Activation> {
    let a: ActivationJson = serde_json::from_value(v.clone()).ok()?;
    let outcome = match a.outcome {
        OutcomeJson::PickedUp { item, count, owner } => Outcome::PickedUp { item, count, owner },
        OutcomeJson::OpenedContainer { container } => Outcome::OpenedContainer { container },
        OutcomeJson::Locked { level } => Outcome::Locked { level },
        OutcomeJson::Teleported { to_door, cell, pos, rot } => Outcome::Teleported { to_door, cell, pos, rot },
        OutcomeJson::Disarmed { item, placed } => Outcome::Disarmed { item, placed },
        OutcomeJson::DoorToggled { open } => Outcome::DoorToggled { open },
        OutcomeJson::Talk { actor } => Outcome::Talk { actor },
        OutcomeJson::Activated => Outcome::Activated,
        OutcomeJson::Disabled => Outcome::Disabled,
        OutcomeJson::NoDefault { sig } => Outcome::NoDefault { sig },
    };
    Some(Activation { outcome, unlocked_with: a.unlocked_with, script: a.script })
}

/// The engine effects of a batch of trace events, in order. `skip` holds indices (into
/// `events`) whose effects the caller applies itself (a default activation by hand,
/// which `activate.rs` turns into effects as before scripts existed): they are traced
/// but not applied twice.
pub fn effects(events: &[Event], skip: &BTreeSet<usize>, ctx: &dyn EffectCtx) -> Vec<ScriptEffect> {
    effects_with_corner_clear(events, skip, 0, ctx)
}

/// Like [`effects`], dropping corner-message [`ScriptEffect::Notify`] items from events
/// at indices below `clear_before` (emitted before a same-frame `ClearMessageQueue` call)
/// while preserving all other effects and notifications emitted at or after `clear_before`
/// [design].
pub fn effects_with_corner_clear(events: &[Event], skip: &BTreeSet<usize>, clear_before: usize, ctx: &dyn EffectCtx) -> Vec<ScriptEffect> {
    let mut out = Vec::new();
    let mut added: Vec<(FormId, i32)> = Vec::new();
    let player = FormId::PLAYER_REF;
    let push_notify = |out: &mut Vec<ScriptEffect>, idx: usize, note: Note| {
        if idx >= clear_before {
            out.push(ScriptEffect::Notify(note));
        }
    };
    // Objectives this batch shows, by quest: the game's state already has them, so
    // whether a quest was listed before is asked without them.
    let shown = |q: FormId| -> Vec<i32> {
        let on = |e: &Event| match e.kind {
            EventKind::ObjectiveDisplayed { quest, index, on: true } if quest == q => Some(index),
            _ => None,
        };
        events.iter().filter_map(on).collect()
    };
    // Items the player picked up by an activation in this batch (by hand or by a
    // script's Activate): `crate::world_sounds` sounds those pick-ups itself.
    let picked_up: BTreeSet<FormId> = events
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::Activated { by, outcome: Some(o), .. } if *by == player => match activation_from_json(o)?.outcome {
                Outcome::PickedUp { item, .. } => Some(item),
                _ => None,
            },
            _ => None,
        })
        .collect();
    let mut caps_sound = false;
    let mut announced = BTreeSet::new();
    let changed = |out: &mut Vec<ScriptEffect>, r: FormId| {
        out.push(ScriptEffect::RefChanged(r.0));
        for c in ctx.enable_children(r) {
            out.push(ScriptEffect::RefChanged(c.0));
        }
    };
    for (i, e) in events.iter().enumerate() {
        if skip.contains(&i) {
            continue;
        }
        match &e.kind {
            // An item a script made (Drop, DropMe, PlaceAtMe) gets its collision
            // (`crate::phys`), as one dropped from the inventory menu does; created actors
            // get theirs from the residents.
            EventKind::Placed { reference, base, .. } if !ctx.is_actor_base(*base) => out.push(ScriptEffect::RefChanged(reference.0)),
            // A new scale changes the reference's collision too (`crate::phys`).
            // Enable fades its reference in when asked (its default) and every enable child
            // too unless the child has "Pop In": the parent's flag is not passed on, the
            // children are enabled as by a plain Enable [documented: GECK wiki Enable].
            // Disable fades only its own reference: "The children will still pop out when
            // disabled" [documented: GECK wiki Disable].
            EventKind::RefEnabled { reference, fade } => {
                changed(&mut out, *reference);
                if *fade {
                    out.push(ScriptEffect::Fade { form_id: reference.0, fade_in: true });
                }
                for c in ctx.enable_children(*reference) {
                    if !ctx.pops_in(c) {
                        out.push(ScriptEffect::Fade { form_id: c.0, fade_in: true });
                    }
                }
            }
            EventKind::RefDisabled { reference, fade } => {
                changed(&mut out, *reference);
                if *fade {
                    out.push(ScriptEffect::Fade { form_id: reference.0, fade_in: false });
                }
            }
            EventKind::Scaled { reference, .. } => changed(&mut out, *reference),
            EventKind::Moved { reference, cell, pos } if *reference == player => {
                let (world, interior) = match ctx.destination(*cell) {
                    Destination::Interior(c) => (None, Some(c)),
                    Destination::Exterior { world } => (Some(world), None),
                    Destination::Unknown => {
                        push_notify(&mut out, i, "A script moved the player to an unknown cell".into());
                        continue;
                    }
                };
                out.push(ScriptEffect::Travel(Travel { world, interior, feet: *pos, heading_deg: ctx.player_heading_deg(), pitch_deg: ctx.player_pitch_deg() }));
            }
            EventKind::Activated { reference, by, outcome: Some(o) } => {
                let Some(a) = activation_from_json(o) else { continue };
                // Furniture's default activation, as by hand (`activate.rs`).
                if *by == player && matches!(a.outcome, Outcome::Activated) && ctx.is_furniture(*reference) {
                    out.push(ScriptEffect::UseFurniture(reference.0));
                    continue;
                }
                // A terminal's default activation, as by hand (`activate.rs`).
                if *by == player && matches!(a.outcome, Outcome::Activated) && ctx.is_terminal(*reference) {
                    out.push(ScriptEffect::OpenTerminal(reference.0));
                    continue;
                }
                for eff in ctx.activation_effects(&a, *reference) {
                    match eff {
                        Effect::RefChanged(id) => changed(&mut out, FormId(id)),
                        // Only the player travels and opens menus; another actor's door or
                        // container use moves only that actor (its state is the world's).
                        Effect::Travel { world, interior, feet, heading_deg } if *by == player => {
                            out.push(ScriptEffect::DoorTravel(Travel { world, interior, feet, heading_deg, pitch_deg: None }))
                        }
                        Effect::OpenContainer(c) if *by == player => out.push(ScriptEffect::OpenContainer(c)),
                        // A pickup's "added" line comes from its ItemAdded event
                        // (`Game::default_activation` emits both), not twice.
                        Effect::Message(_) if matches!(a.outcome, Outcome::PickedUp { .. }) => {}
                        Effect::Message(m) if *by == player => push_notify(&mut out, i, m.into()),
                        _ => {}
                    }
                }
            }
            // An essential actor knocked out, or the player trying to talk to an
            // unconscious one: "<Name> is unconscious." (docs/spec-notes/unconscious.md
            // §4); a fatigue knockout shows nothing [owner request 2026-10-17].
            // The player's tweak keeps this line off [design: docs/design/tweaks.md].
            EventKind::KnockedOut { cause: "essential", .. } | EventKind::TalkedToUnconscious { .. } if ctx.tweaks().is_some_and(|t| t.no_unconscious_message) => {}
            EventKind::KnockedOut { actor, cause: "essential" } | EventKind::TalkedToUnconscious { actor } => push_notify(&mut out, i, unconscious_line(ctx, *actor).into()),
            EventKind::MessageShown { message, text, buttons, hidden } => {
                let text = text.clone().unwrap_or_default();
                if !buttons.is_empty() || ctx.is_message_box(*message) {
                    out.push(ScriptEffect::MessageBox(BoxMessage { message: message.0, text, buttons: buttons.clone(), hidden: hidden.clone() }));
                } else if !text.trim().is_empty() {
                    let (icon, seconds) = ctx.message_look(*message);
                    push_notify(&mut out, i, Note { text, icon, seconds });
                }
            }
            EventKind::NoteShown { text, icon, seconds } if !text.trim().is_empty() => push_notify(&mut out, i, Note { text: text.clone(), icon: icon.clone(), seconds: Some(*seconds) }),
            // The first objective of a quest to be shown adds the quest [documented: GECK
            // wiki SetObjectiveDisplayed]; an objective without text or a quest without a
            // name is not shown [hypothesis] (quest-messages.md §5).
            EventKind::ObjectiveDisplayed { quest, index, on: true } => {
                let Some(text) = ctx.objective_text(*quest, *index).filter(|t| !t.trim().is_empty()) else { continue };
                let Some(name) = ctx.quest_title(*quest).filter(|n| !n.trim().is_empty()) else { continue };
                if announced.insert(*quest) && !ctx.quest_listed(*quest, &shown(*quest)) {
                    out.push(ScriptEffect::Quest(QuestNotice::Added(name)));
                }
                out.push(ScriptEffect::Quest(QuestNotice::Objectives(vec![Objective { quest: quest.0, text, completed: false }])));
            }
            EventKind::ObjectiveCompleted { quest, index, on: true, silent: false } => {
                if ctx.quest_title(*quest).is_none_or(|n| n.trim().is_empty()) {
                    continue;
                }
                if let Some(text) = ctx.objective_text(*quest, *index).filter(|t| !t.trim().is_empty()) {
                    out.push(ScriptEffect::Quest(QuestNotice::Objectives(vec![Objective { quest: quest.0, text, completed: true }])));
                }
            }
            EventKind::QuestCompleted { quest } => out.extend(ctx.quest_title(*quest).map(|n| ScriptEffect::Quest(QuestNotice::Completed(n)))),
            EventKind::QuestFailed { quest } => out.extend(ctx.quest_title(*quest).map(|n| ScriptEffect::Quest(QuestNotice::Failed(n)))),
            // The player's tweak: no line for caps [design: docs/design/tweaks.md]; the
            // sound still plays.
            EventKind::ItemAdded { to, item: CAPS, silent: false, .. } if *to == player && ctx.tweaks().is_some_and(|t| t.no_caps_added_message) => {
                caps_sound |= !picked_up.contains(&CAPS);
            }
            EventKind::ItemAdded { to, item, count, silent: false } if *to == player => {
                // Caps a script gave: their pick-up sound, as caps taken by hand
                // [hypothesis: the original plays it; player report R-BR8PUBD2, #3095].
                caps_sound |= *item == CAPS && !picked_up.contains(&CAPS);
                if i >= clear_before {
                    match added.iter_mut().find(|(i, _)| i == item) {
                        Some((_, n)) => *n += count,
                        None => added.push((*item, *count)),
                    }
                }
            }
            // A skill book read: "<Skill> increased to <value>" with the default corner
            // icon (`icon: None` -> `glow_message_vaultboy_neutral.dds`) rather than the
            // 256x256 Pip-Boy `AVIF` skill illustration [hypothesis: wording and icon
            // NVForever's, no MESG or GMST in the masters names a book read;
            // docs/spec-notes/skill-books.md §5].
            // The player's tweak: no line for a book that gave nothing [design: docs/design/tweaks.md].
            EventKind::BookRead { actor, points, .. } if *actor == player && *points == 0.0 && ctx.tweaks().is_some_and(|t| t.no_book_message_without_gain) => {}
            EventKind::BookRead { actor, value, skill, .. } if *actor == player => {
                let name = ctx.skill_name(*skill);
                push_notify(&mut out, i, Note { text: format!("{name} increased to {}", value.round()), icon: None, seconds: None });
            }
            EventKind::ItemRemoved { from, item, count, silent: false } if *from == player => {
                let n = ctx.name(*item);
                push_notify(&mut out, i, if *count > 1 { format!("{n} ({count}) removed") } else { format!("{n} removed") }.into());
            }
            _ => {}
        }
    }
    if caps_sound {
        out.push(ScriptEffect::ItemAddedSound(CAPS));
    }
    // Items a script gave the player, one line each, or one line for many (the add-on
    // packs give dozens at once).
    const MANY: usize = 4;
    // The player's tweak sets how long these lines stay [design: docs/design/tweaks.md].
    let seconds = ctx.tweaks().map(|t| t.item_added_seconds).filter(|s| *s > 0.0);
    let line = |text: String| ScriptEffect::Notify(Note { text, icon: None, seconds });
    if added.len() >= MANY {
        let total: i32 = added.iter().map(|(_, n)| n).sum();
        out.push(line(format!("{} items added ({total} in all)", added.len())));
    } else {
        for (item, n) in added {
            let name = ctx.name(item);
            out.push(line(if n > 1 { format!("{name} ({n}) added") } else { format!("{name} added") }));
        }
    }
    out
}

// ---- inventory changes made outside scripts ------------------------------------------

/// The player's inventory as script events see it: counts and equipped items.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InvView {
    pub counts: BTreeMap<FormId, i32>,
    pub equipped: BTreeSet<FormId>,
}

impl InvView {
    pub fn of(inv: Option<&Inventory>) -> InvView {
        let Some(inv) = inv else { return InvView::default() };
        let mut counts = BTreeMap::new();
        for (item, n) in inv.iter() {
            *counts.entry(item).or_insert(0) += n;
        }
        InvView { counts, equipped: inv.equipped().into_iter().collect() }
    }
}

/// What changed between two views.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InvChanges {
    pub added: Vec<(FormId, i32)>,
    pub removed: Vec<(FormId, i32)>,
    pub equipped: Vec<FormId>,
    pub unequipped: Vec<FormId>,
}

impl InvChanges {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.equipped.is_empty() && self.unequipped.is_empty()
    }
}

pub fn inventory_changes(before: &InvView, after: &InvView) -> InvChanges {
    let mut c = InvChanges::default();
    let items: BTreeSet<FormId> = before.counts.keys().chain(after.counts.keys()).copied().collect();
    for item in items {
        let (a, b) = (before.counts.get(&item).copied().unwrap_or(0), after.counts.get(&item).copied().unwrap_or(0));
        if b > a {
            c.added.push((item, b - a));
        } else if a > b {
            c.removed.push((item, a - b));
        }
    }
    c.equipped = after.equipped.difference(&before.equipped).copied().collect();
    c.unequipped = before.equipped.difference(&after.equipped).copied().collect();
    c
}

// ---- trigger volumes ---------------------------------------------------------------------

/// A trigger volume: a scripted reference's placed primitive (`XPRM`), box or sphere, at
/// the reference's pose. Bounds are read as half extents / radius, the reading
/// `fnv_phys::primitive` uses for collision boxes [hypothesis, physics.md §4.1].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TriggerShape {
    pub pos: [f32; 3],
    /// Row-major rotation (`fnv_world::transform::ref_rotation`): world = pos + R·local·scale.
    pub rot: [f32; 9],
    pub scale: f32,
    pub kind: TriggerKind,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TriggerKind {
    Box { half_extents: [f32; 3] },
    Sphere { radius: f32 },
}

impl TriggerShape {
    pub fn contains(&self, p: [f32; 3]) -> bool {
        let l = self.local(p);
        match self.kind {
            TriggerKind::Box { half_extents: h } => (0..3).all(|i| l[i].abs() <= h[i]),
            TriggerKind::Sphere { radius } => l.iter().map(|v| v * v).sum::<f32>() <= radius * radius,
        }
    }

    /// The point of the volume nearest `p`, `inset` units inside its faces (a box) or its
    /// surface (a sphere) where it is that large: where a walk into it from `p` aims.
    pub fn nearest_inside(&self, p: [f32; 3], inset: f32) -> [f32; 3] {
        let l = self.local(p);
        let s = self.scale_or_one();
        let n = match self.kind {
            TriggerKind::Box { half_extents: h } => std::array::from_fn(|i| {
                let e = (h[i] - inset / s).max(0.0);
                l[i].clamp(-e, e)
            }),
            TriggerKind::Sphere { radius } => {
                let len = l.iter().map(|v| v * v).sum::<f32>().sqrt();
                let r = (radius - inset / s).max(0.0);
                if len <= r || len < 1e-6 { l } else { l.map(|v| v * r / len) }
            }
        };
        let r = &self.rot;
        [
            self.pos[0] + s * (r[0] * n[0] + r[1] * n[1] + r[2] * n[2]),
            self.pos[1] + s * (r[3] * n[0] + r[4] * n[1] + r[5] * n[2]),
            self.pos[2] + s * (r[6] * n[0] + r[7] * n[1] + r[8] * n[2]),
        ]
    }
}

/// An actor's collision capsule for trigger tests: upright, from the feet up to `height`,
/// of `radius` (the hemispheres included) [hypothesis: the game tests the actor's
/// collision object against the volume, docs/spec-notes/level-design.md §11.2 T2; the
/// sizes are `fnv_phys::CharacterParams`'s placeholders].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Capsule {
    pub feet: [f32; 3],
    pub radius: f32,
    pub height: f32,
}

impl Capsule {
    /// The capsule's axis: the centres of its two hemispheres (one point when it is no
    /// taller than it is wide).
    pub fn axis(&self) -> ([f32; 3], [f32; 3]) {
        let r = self.radius.max(0.0);
        let (lo, hi) = if self.height > 2.0 * r { (r, self.height - r) } else { (self.height * 0.5, self.height * 0.5) };
        let at = |h: f32| [self.feet[0], self.feet[1], self.feet[2] + h];
        (at(lo), at(hi))
    }
}

impl TriggerShape {
    /// A point in the volume's own frame (unscaled).
    fn local(&self, p: [f32; 3]) -> [f32; 3] {
        let d = [p[0] - self.pos[0], p[1] - self.pos[1], p[2] - self.pos[2]];
        let s = self.scale_or_one();
        let r = &self.rot;
        [
            (r[0] * d[0] + r[3] * d[1] + r[6] * d[2]) / s,
            (r[1] * d[0] + r[4] * d[1] + r[7] * d[2]) / s,
            (r[2] * d[0] + r[5] * d[1] + r[8] * d[2]) / s,
        ]
    }

    fn scale_or_one(&self) -> f32 {
        if self.scale > 0.0 {
            self.scale
        } else {
            1.0
        }
    }

    /// Whether an actor's capsule overlaps the volume (touching counts).
    pub fn touches(&self, c: &Capsule) -> bool {
        let (a, b) = c.axis();
        let (a, b) = (self.local(a), self.local(b));
        let r = c.radius.max(0.0) / self.scale_or_one();
        let at = |t: f32| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
        match self.kind {
            TriggerKind::Sphere { radius } => {
                // Closest point of the axis to the centre (the local origin).
                let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let len2: f32 = ab.iter().map(|v| v * v).sum();
                let t = if len2 > 0.0 { (-(a[0] * ab[0] + a[1] * ab[1] + a[2] * ab[2]) / len2).clamp(0.0, 1.0) } else { 0.0 };
                let p = at(t);
                let d2: f32 = p.iter().map(|v| v * v).sum();
                let reach = radius + r;
                d2 <= reach * reach
            }
            TriggerKind::Box { half_extents: h } => {
                // The distance from a point of the axis to the box is convex along the
                // axis: a golden-section search finds its minimum.
                let dist2 = |t: f32| -> f32 {
                    let p = at(t);
                    (0..3).map(|i| (p[i].abs() - h[i]).max(0.0)).map(|v| v * v).sum()
                };
                let (mut lo, mut hi) = (0.0f32, 1.0f32);
                const G: f32 = 0.618_034;
                let (mut x1, mut x2) = (hi - G * (hi - lo), lo + G * (hi - lo));
                let (mut f1, mut f2) = (dist2(x1), dist2(x2));
                for _ in 0..40 {
                    if f1 <= f2 {
                        hi = x2;
                        (x2, f2) = (x1, f1);
                        x1 = hi - G * (hi - lo);
                        f1 = dist2(x1);
                    } else {
                        lo = x1;
                        (x1, f1) = (x2, f2);
                        x2 = lo + G * (hi - lo);
                        f2 = dist2(x2);
                    }
                }
                let best = [dist2(0.0), dist2(1.0), f1, f2].into_iter().fold(f32::INFINITY, f32::min);
                best <= r * r + 1e-4
            }
        }
    }
}

impl TriggerShape {
    /// Whether the segment `a`–`b` passes through the volume (an end inside counts): a
    /// shot's path through a gas cloud (`crate::gas_traps`).
    pub fn crossed_by(&self, a: [f32; 3], b: [f32; 3]) -> bool {
        let (a, b) = (self.local(a), self.local(b));
        let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        match self.kind {
            TriggerKind::Sphere { radius } => {
                let len2: f32 = d.iter().map(|v| v * v).sum();
                let t = if len2 > 0.0 { (-(a[0] * d[0] + a[1] * d[1] + a[2] * d[2]) / len2).clamp(0.0, 1.0) } else { 0.0 };
                let p = [a[0] + d[0] * t, a[1] + d[1] * t, a[2] + d[2] * t];
                p.iter().map(|v| v * v).sum::<f32>() <= radius * radius
            }
            TriggerKind::Box { half_extents: h } => {
                // Slabs: the part of the segment inside each pair of faces.
                let (mut lo, mut hi) = (0.0f32, 1.0f32);
                for i in 0..3 {
                    if d[i].abs() < 1e-6 {
                        if a[i].abs() > h[i] {
                            return false;
                        }
                        continue;
                    }
                    let (t0, t1) = ((-h[i] - a[i]) / d[i], (h[i] - a[i]) / d[i]);
                    lo = lo.max(t0.min(t1));
                    hi = hi.min(t0.max(t1));
                    if lo > hi {
                        return false;
                    }
                }
                true
            }
        }
    }

    /// Whether a ball (centre `c`, `radius`) overlaps the volume: a blast reaching a gas
    /// cloud (`crate::gas_traps`).
    pub fn overlaps_ball(&self, c: [f32; 3], radius: f32) -> bool {
        let r = radius.max(0.0);
        // A capsule no taller than it is wide is its centre's ball.
        self.touches(&Capsule { feet: [c[0], c[1], c[2] - r], radius: r, height: 2.0 * r })
    }
}

/// Trigger transitions between two frames, per (trigger, actor) pair: entered, still
/// inside (the entering frame included), left.
#[derive(Debug, Clone, PartialEq)]
pub struct TriggerChanges<T> {
    pub entered: Vec<T>,
    pub inside: Vec<T>,
    pub left: Vec<T>,
}

pub fn trigger_changes<T: Ord + Copy>(before: &BTreeSet<T>, now: &BTreeSet<T>) -> TriggerChanges<T> {
    TriggerChanges {
        entered: now.difference(before).copied().collect(),
        inside: now.iter().copied().collect(),
        left: before.difference(now).copied().collect(),
    }
}

// ---- the script test aid -------------------------------------------------------------------

/// One step of `NVF_PLAY_TEST_SCRIPT` (stand-ins for what Layer 6 and the player will
/// drive: dialogue lines, quest starts, message-box buttons, menus, walking into a
/// trigger).
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptAid {
    /// `start:<quest>`: StartQuest.
    Start(String),
    /// `combat:<actor>:<target>`: actor.StartCombat target (`player` for the player).
    Combat(String, String),
    /// `moveto:<reference>:<marker>[:<x>:<y>:<z>]`: the reference's script calls
    /// `MoveTo marker x y z` (offsets 0 when left out; `player` for the player as the
    /// marker): to put actors where a fight can be watched (docs/design/combat-ai.md §12.7).
    MoveTo(String, String, [f32; 3]),
    /// `stop:<quest>`: StopQuest.
    Stop(String),
    /// `stage:<quest>:<stage>`: SetStage.
    Stage(String, i32),
    /// `objective:<quest>:<index>`: SetObjectiveDisplayed 1.
    Objective(String, i32),
    /// `objective_done:<quest>:<index>`: SetObjectiveCompleted 1.
    ObjectiveDone(String, i32),
    /// `info:<INFO>[:<speaker>]`: a dialogue line is said (its result scripts run;
    /// conditions are not required), as `fnvtool script run`'s `say_info` with `force`.
    Info(String, Option<String>),
    /// `button:<n>`: the player picks button n (0 = first) of the open message box.
    Button(i32),
    /// `close`: the player closes the open menu or message box (OK / Done).
    Close,
    /// `message:<MESG>`: the message is shown as ShowMessage shows it, for looking at a
    /// message box outside the script that shows it (no script reads its button).
    Message(String),
    /// `menu:<code>`: the menu with that MenuMode code opens as a script's function opens
    /// it (1051 name, 1074 S.P.E.C.I.A.L., 1048 tag skills, 1084 traits), for looking at a
    /// character-creation menu outside the opening.
    Menu(u16),
    /// `var:<quest or reference>:<variable>:<value>`: sets a script variable (the name
    /// may be `scripted:<SCPT>` or `placed:<base>`).
    Var(String, String, f64),
    /// `walk:<reference>`: the player's feet are put on the floor under the reference (a
    /// trigger volume's centre), a stand-in for walking there.
    Walk(String),
    /// `activate:<reference>`: the player activates the reference by hand (E), wherever
    /// the crosshair is.
    Activate(String),
    /// `enable:<reference>`: the reference is enabled as a script's `Enable` does.
    Enable(String),
    /// `kill:<actor>`: the player kills the actor (`Game::kill` with the player as the
    /// killer: karma and murder are judged, `fnv_game::crime`).
    Kill(String),
    /// `challenge:<challenge>:<progress>`: the challenge's progress is set (below its
    /// threshold), to reach a completion in a short run (`fnv_game::challenges`).
    Challenge(String, u32),
    /// `say_to:<speaker>:<target>:<topic>[:<noTargetLookFlag>]`: the speaker's script
    /// calls `SayTo target topic 0 flag`.
    SayTo(String, String, String, bool),
    /// `play_group:<reference>:<group>[:<flag>]`: a script calls `ref.PlayGroup group
    /// flag` (the group by name, as source spells it, or by code; flag 0 when left out).
    PlayGroup(String, String, i32),
    /// `play_idle:<reference>:<idle>`: the actor is asked to play an IDLE (by Editor ID or
    /// FormID) as a script's `PlayIdle` asks it.
    PlayIdle(String, String),
    /// `call:<reference or ->:<function>[:<argument>…]`: a script calls the function (by
    /// its catalogue name) on the reference (`-`: none); an argument that reads as a
    /// number is one, `x`/`y`/`z` an axis, `"text"` a string, any other is a form (Editor
    /// ID, FormID or `player`).
    Call(String, String, Vec<String>),
    /// `damage:<reference>:<actor value>:<amount>`: the actor value takes damage as
    /// `DamageActorValue` gives it (a stand-in for a hit, for checking aid items).
    Damage(String, String, f64),
    /// `set_av:<reference>:<actor value>:<value>`: SetActorValue (a need's level, for
    /// checking Hardcore's stages).
    SetAv(String, String, f64),
    /// `hardcore:<0|1>`: SetHardcore.
    Hardcore(bool),
    /// `perk:<PERK>`: the player gets the perk (AddPerk), for checking perk entry points.
    Perk(String),
}

/// When a test-aid step runs: `at` seconds after the start, or, with `after`, `at`
/// seconds after the frame in which that quest stage was first seen done.
#[derive(Debug, Clone, PartialEq)]
pub struct TimedAid {
    pub at: f32,
    pub after: Option<(String, i32)>,
    pub aid: ScriptAid,
}

/// The scripts first run at `now` seconds (the aids' clock). When that is after the first
/// timed step's time (the script data took long to load on a busy machine), every timed
/// step is moved later by the difference: the first runs now and each keeps its distance
/// from the others, instead of all the overdue ones running in this one frame [design;
/// a check that shows an objective at 3 s and completes it at 9 s otherwise sees both in
/// one frame]. Steps after a quest stage count from the stage and are left alone.
/// Returns the seconds moved, if any.
pub fn delay_late_aids(aids: &mut [TimedAid], now: f32) -> Option<f32> {
    let first = aids.iter().filter(|a| a.after.is_none()).map(|a| a.at).reduce(f32::min)?;
    let late = now - first;
    if late <= 0.0 {
        return None;
    }
    for a in aids.iter_mut().filter(|a| a.after.is_none()) {
        a.at += late;
    }
    Some(late)
}

/// The first piece of a reference name that holds a colon (`fnv_game::runner::resolve`:
/// `scripted:<SCPT>`, `placed:<base>`, and `created:<base>`, the newest reference made at
/// run time from a base, whose FormID is not known ahead).
const NAME_PREFIXES: [&str; 3] = ["scripted", "placed", "created"];

/// Parse `<when>:<op>[:<args>]` items separated by commas; `<when>` is `<seconds>` or
/// `<seconds>@<quest>=<stage>`. Names are Editor IDs, hex FormIDs, `player` or the
/// runner's `scripted:<SCPT>` / `placed:<base>` / `created:<base>`, resolved when the
/// step runs.
pub fn parse_aids(v: &str) -> Result<Vec<TimedAid>, String> {
    v.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|item| {
            let (when, rest) = item.split_once(':').ok_or_else(|| format!("{item:?}: expected <seconds>:..."))?;
            let (t, after) = match when.split_once('@') {
                Some((t, gate)) => {
                    let (q, s) = gate.split_once('=').ok_or_else(|| format!("{item:?}: expected <seconds>@<quest>=<stage>"))?;
                    let s = s.trim().parse::<i32>().map_err(|_| format!("{item:?}: bad stage {s:?}"))?;
                    (t, Some((q.trim().to_string(), s)))
                }
                None => (when, None),
            };
            let at: f32 = t.trim().parse().map_err(|_| format!("{item:?}: bad seconds {t:?}"))?;
            let rest = rest.trim();
            // A reference name may hold a colon itself (`scripted:<SCPT>`, `placed:<base>`,
            // `created:<base>`): its two pieces are one part, in any step.
            let pieces: Vec<&str> = rest.split(':').map(str::trim).collect();
            let mut joined: Vec<String> = Vec::with_capacity(pieces.len());
            let mut k = 0;
            while k < pieces.len() {
                if k > 0 && k + 1 < pieces.len() && NAME_PREFIXES.contains(&pieces[k]) {
                    joined.push(format!("{}:{}", pieces[k], pieces[k + 1]));
                    k += 2;
                } else {
                    joined.push(pieces[k].to_string());
                    k += 1;
                }
            }
            let parts: Vec<&str> = joined.iter().map(String::as_str).collect();
            let int = |s: &str| s.parse::<i32>().map_err(|_| format!("{rest:?}: bad number {s:?}"));
            // A reference name may hold colons itself (`scripted:<SCPT>`, `placed:<base>`).
            let whole = rest.split_once(':').map(|(_, r)| r.trim().to_string());
            let aid = match parts.as_slice() {
                ["walk", _, ..] => ScriptAid::Walk(whole.unwrap_or_default()),
                ["activate", _, ..] => ScriptAid::Activate(whole.unwrap_or_default()),
                ["start", q] => ScriptAid::Start(q.to_string()),
                ["combat", a, t] => ScriptAid::Combat(a.to_string(), t.to_string()),
                ["moveto", r, m] => ScriptAid::MoveTo(r.to_string(), m.to_string(), [0.0; 3]),
                ["moveto", r, m, x, y, z] => {
                    let f = |s: &str| s.parse::<f32>().map_err(|_| format!("{rest:?}: bad offset {s:?}"));
                    ScriptAid::MoveTo(r.to_string(), m.to_string(), [f(x)?, f(y)?, f(z)?])
                }
                ["enable", r] => ScriptAid::Enable(r.to_string()),
                ["kill", r] => ScriptAid::Kill(r.to_string()),
                ["challenge", c, p] => ScriptAid::Challenge(c.to_string(), p.parse().map_err(|_| format!("{rest:?}: bad progress {p:?}"))?),
                ["stop", q] => ScriptAid::Stop(q.to_string()),
                ["stage", q, s] => ScriptAid::Stage(q.to_string(), int(s)?),
                ["objective", q, i] => ScriptAid::Objective(q.to_string(), int(i)?),
                ["objective_done", q, i] => ScriptAid::ObjectiveDone(q.to_string(), int(i)?),
                ["info", i] => ScriptAid::Info(i.to_string(), None),
                ["info", i, sp] => ScriptAid::Info(i.to_string(), Some(sp.to_string())),
                ["button", b] => ScriptAid::Button(int(b)?),
                ["close"] => ScriptAid::Close,
                ["message", m] => ScriptAid::Message(m.to_string()),
                ["menu", c] => ScriptAid::Menu(c.parse().map_err(|_| format!("{item:?}: menu code"))?),
                ["say_to", sp, t, topic] => ScriptAid::SayTo(sp.to_string(), t.to_string(), topic.to_string(), false),
                ["say_to", sp, t, topic, flag] => ScriptAid::SayTo(sp.to_string(), t.to_string(), topic.to_string(), int(flag)? != 0),
                ["play_group", r, g] => ScriptAid::PlayGroup(r.to_string(), g.to_string(), 0),
                ["play_group", r, g, flag] => ScriptAid::PlayGroup(r.to_string(), g.to_string(), int(flag)?),
                ["play_idle", r, i] => ScriptAid::PlayIdle(r.to_string(), i.to_string()),
                ["call", on, f, args @ ..] => ScriptAid::Call(on.to_string(), f.to_string(), args.iter().map(|a| a.to_string()).collect()),
                ["damage", r, v, amount] => {
                    ScriptAid::Damage(r.to_string(), v.to_string(), amount.parse().map_err(|_| format!("{rest:?}: bad amount {amount:?}"))?)
                }
                ["set_av", r, v, value] => {
                    ScriptAid::SetAv(r.to_string(), v.to_string(), value.parse().map_err(|_| format!("{rest:?}: bad value {value:?}"))?)
                }
                ["hardcore", on] => ScriptAid::Hardcore(int(on)? != 0),
                ["perk", p] => ScriptAid::Perk(p.to_string()),
                // The owner's name may hold a colon too (`var:scripted:<SCPT>:<variable>:<value>`).
                ["var", of @ .., var, value] if !of.is_empty() => {
                    ScriptAid::Var(of.join(":"), var.to_string(), value.parse().map_err(|_| format!("{rest:?}: bad value {value:?}"))?)
                }
                _ => {
                    return Err(format!(
                        "{rest:?}: expected start:, stop:, combat:, moveto:, enable:, kill:, challenge:, stage:, objective:, objective_done:, info:, button:, close, message:, var:, walk:, activate:, say_to:, play_group:, play_idle:, call:, damage:, set_av:, hardcore: or perk:"
                    ))
                }
            };
            Ok(TimedAid { at, after, aid })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fnv_world::transform::ref_rotation;

    const QUEST: FormId = FormId(0x104C1C);
    const DOOR: FormId = FormId(0x116425);
    const PARENT: FormId = FormId(0x500);
    const CHILD: FormId = FormId(0x501);
    const GRANDCHILD: FormId = FormId(0x502);
    const INTERIOR: u32 = 0x103DF9;
    const WORLD: u32 = 0xDA726;
    const EXTERIOR: u32 = 0x200;
    const TERMINAL: FormId = FormId(0x14B0A7);

    struct Fake;

    impl EffectCtx for Fake {
        fn name(&self, form: FormId) -> String {
            format!("form {:X}", form.0)
        }
        fn objective_text(&self, quest: FormId, index: i32) -> Option<String> {
            match (quest, index) {
                (QUEST, 10) => Some("Talk to Sunny Smiles.".into()),
                (QUEST, 30) => Some("Use the Vit-o-matic Vigor Tester".into()),
                (QUEST, 35) => Some("Follow Sunny.".into()),
                _ => None,
            }
        }
        fn quest_title(&self, quest: FormId) -> Option<String> {
            (quest == QUEST).then(|| "Back in the Saddle".into())
        }
        fn quest_listed(&self, quest: FormId, besides: &[i32]) -> bool {
            // Objective 10 was shown before this batch.
            quest == QUEST && !besides.contains(&10)
        }
        fn is_message_box(&self, message: FormId) -> bool {
            message.0 == 0x0BB
        }
        fn message_look(&self, message: FormId) -> (Option<String>, Option<f32>) {
            match message.0 {
                0x0AA => (Some("icons\\pain.dds".into()), Some(10.0)),
                _ => (None, None),
            }
        }
        fn enable_children(&self, r: FormId) -> Vec<FormId> {
            if r == PARENT {
                vec![CHILD, GRANDCHILD]
            } else {
                Vec::new()
            }
        }
        fn pops_in(&self, r: FormId) -> bool {
            r == GRANDCHILD
        }
        fn player_heading_deg(&self) -> f32 {
            90.0
        }
        fn player_pitch_deg(&self) -> Option<f32> {
            Some(0.0)
        }
        fn destination(&self, cell: Option<FormId>) -> Destination {
            match cell.map(|c| c.0) {
                Some(INTERIOR) => Destination::Interior(INTERIOR),
                Some(EXTERIOR) => Destination::Exterior { world: WORLD },
                _ => Destination::Unknown,
            }
        }
        fn activation_effects(&self, a: &Activation, target: FormId) -> Vec<Effect> {
            let mut ix = fnv_world::WorldIndex::default();
            ix.cells.insert(INTERIOR, fnv_world::Cell { form_id: INTERIOR, ..Default::default() });
            ix.cells.insert(EXTERIOR, fnv_world::Cell { form_id: EXTERIOR, world: Some(WORLD), ..Default::default() });
            crate::activate::outcome::effects(a, target.0, "Gate", &ix, |id| self.name(id))
        }
        fn is_terminal(&self, r: FormId) -> bool {
            r == TERMINAL
        }
    }

    fn ev(kind: EventKind) -> Event {
        Event { frame: 1, kind }
    }

    fn run(events: Vec<EventKind>) -> Vec<ScriptEffect> {
        let events: Vec<Event> = events.into_iter().map(ev).collect();
        effects(&events, &BTreeSet::new(), &Fake)
    }

    #[test]
    fn enable_and_disable_change_the_reference_and_its_children() {
        assert_eq!(
            run(vec![EventKind::RefDisabled { reference: PARENT, fade: false }]),
            vec![ScriptEffect::RefChanged(PARENT.0), ScriptEffect::RefChanged(CHILD.0), ScriptEffect::RefChanged(GRANDCHILD.0)]
        );
        assert_eq!(run(vec![EventKind::RefEnabled { reference: GRANDCHILD, fade: false }]), vec![ScriptEffect::RefChanged(GRANDCHILD.0)]);
    }

    #[test]
    fn fades_follow_the_flags_and_children_fade_in_unless_they_pop_in() {
        use ScriptEffect::{Fade, RefChanged};
        // Disable 1 fades the parent alone; the children pop out.
        assert_eq!(
            run(vec![EventKind::RefDisabled { reference: PARENT, fade: true }]),
            vec![RefChanged(PARENT.0), RefChanged(CHILD.0), RefChanged(GRANDCHILD.0), Fade { form_id: PARENT.0, fade_in: false }]
        );
        // Enable 0: the parent pops in, the children fade in as by a plain Enable, but
        // the grandchild has "Pop In".
        assert_eq!(
            run(vec![EventKind::RefEnabled { reference: PARENT, fade: false }]),
            vec![RefChanged(PARENT.0), RefChanged(CHILD.0), RefChanged(GRANDCHILD.0), Fade { form_id: CHILD.0, fade_in: true }]
        );
        assert_eq!(
            run(vec![EventKind::RefEnabled { reference: PARENT, fade: true }]),
            vec![RefChanged(PARENT.0), RefChanged(CHILD.0), RefChanged(GRANDCHILD.0), Fade { form_id: PARENT.0, fade_in: true }, Fade { form_id: CHILD.0, fade_in: true }]
        );
    }

    #[test]
    fn moving_the_player_travels_and_others_only_move() {
        let e = run(vec![EventKind::Moved { reference: FormId::PLAYER_REF, cell: Some(FormId(INTERIOR)), pos: [1.0, 2.0, 3.0] }]);
        assert_eq!(e, vec![ScriptEffect::Travel(Travel { world: None, interior: Some(INTERIOR), feet: [1.0, 2.0, 3.0], heading_deg: 90.0, pitch_deg: Some(0.0) })]);
        let e = run(vec![EventKind::Moved { reference: FormId::PLAYER_REF, cell: Some(FormId(EXTERIOR)), pos: [4.0, 5.0, 6.0] }]);
        assert_eq!(e, vec![ScriptEffect::Travel(Travel { world: Some(WORLD), interior: None, feet: [4.0, 5.0, 6.0], heading_deg: 90.0, pitch_deg: Some(0.0) })]);
        assert!(run(vec![EventKind::Moved { reference: FormId(0x77), cell: Some(FormId(EXTERIOR)), pos: [0.0; 3] }]).is_empty());
        assert!(matches!(
            run(vec![EventKind::Moved { reference: FormId::PLAYER_REF, cell: None, pos: [0.0; 3] }]).as_slice(),
            [ScriptEffect::Notify(_)]
        ));
    }

    #[test]
    fn activation_outcomes_read_back_from_the_trace() {
        let tp = Outcome::Teleported { to_door: FormId(0x13BB0D), cell: Some(FormId(EXTERIOR)), pos: [1004.3, 8722.3, 1024.0], rot: [0.0, 0.0, 0.0] };
        for a in [
            Activation { outcome: tp.clone(), unlocked_with: Some(FormId(0x119875)), script: Some(FormId(0x119787)) },
            Activation { outcome: Outcome::PickedUp { item: FormId(0x15169), count: 3, owner: None }, unlocked_with: None, script: None },
            Activation { outcome: Outcome::Locked { level: 255 }, unlocked_with: None, script: None },
            Activation { outcome: Outcome::NoDefault { sig: "LVLI".into() }, unlocked_with: None, script: None },
            Activation { outcome: Outcome::Activated, unlocked_with: None, script: None },
        ] {
            let v = serde_json::to_value(&a).unwrap();
            assert_eq!(activation_from_json(&v), Some(a), "{v}");
        }
        // A scripted Activate of the Strip gate by the player: the player travels.
        let a = Activation { outcome: tp, unlocked_with: None, script: None };
        let o = Some(serde_json::to_value(&a).unwrap());
        let e = run(vec![EventKind::Activated { reference: DOOR, by: FormId::PLAYER_REF, outcome: o.clone() }]);
        assert_eq!(e, vec![ScriptEffect::DoorTravel(Travel { world: Some(WORLD), interior: None, feet: [1004.3, 8722.3, 1024.0], heading_deg: 0.0, pitch_deg: None })]);
        // Another actor going through the same door does not move the player.
        assert!(run(vec![EventKind::Activated { reference: DOOR, by: FormId(0x99), outcome: o }]).is_empty());
        // A locked door says so.
        let locked = Activation { outcome: Outcome::Locked { level: 255 }, unlocked_with: None, script: None };
        let e = run(vec![EventKind::Activated { reference: DOOR, by: FormId::PLAYER_REF, outcome: Some(serde_json::to_value(&locked).unwrap()) }]);
        assert_eq!(e, vec![ScriptEffect::Notify("Gate is locked (Requires Key)".into())]);
    }

    #[test]
    fn a_scripted_activate_of_a_terminal_opens_it_for_the_player_only() {
        // Lucky38MrHouseTerminal's OnActivate ends in `Activate` (#1532): the default
        // activation of a scripted terminal, by the player.
        let a = Activation { outcome: Outcome::Activated, unlocked_with: None, script: Some(FormId(0x1746C1)) };
        let o = Some(serde_json::to_value(&a).unwrap());
        let e = run(vec![EventKind::Activated { reference: TERMINAL, by: FormId::PLAYER_REF, outcome: o.clone() }]);
        assert_eq!(e, vec![ScriptEffect::OpenTerminal(TERMINAL.0)]);
        assert!(run(vec![EventKind::Activated { reference: TERMINAL, by: FormId(0x99), outcome: o.clone() }]).is_empty());
        // Another scripted activator opens nothing.
        assert!(run(vec![EventKind::Activated { reference: DOOR, by: FormId::PLAYER_REF, outcome: o }]).is_empty());
    }

    #[test]
    fn messages_objectives_and_items() {
        let e = run(vec![
            EventKind::MessageShown { message: FormId(0xAA), text: Some("To jump, press Space.".into()), buttons: vec![], hidden: vec![] },
            EventKind::MessageShown { message: FormId(0xBB), text: Some("Old World Blues has been loaded.".into()), buttons: vec![], hidden: vec![] },
            EventKind::MessageShown { message: FormId(0xCC), text: Some("Hardcore?".into()), buttons: vec!["No".into(), "Yes".into()], hidden: vec![] },
            EventKind::ObjectiveDisplayed { quest: QUEST, index: 30, on: true },
            EventKind::ObjectiveCompleted { quest: QUEST, index: 10, on: true, silent: false },
            EventKind::ObjectiveDisplayed { quest: QUEST, index: 40, on: false },
            EventKind::QuestCompleted { quest: QUEST },
            EventKind::ItemAdded { to: FormId::PLAYER_REF, item: FormId(0x15169), count: 5, silent: false },
            EventKind::ItemAdded { to: FormId(0x77), item: FormId(0x15169), count: 5, silent: false },
            EventKind::ItemRemoved { from: FormId::PLAYER_REF, item: FormId(0x15169), count: 1, silent: false },
        ]);
        assert_eq!(
            e,
            vec![
                // A message's line has its record's icon and time; the others have neither.
                ScriptEffect::Notify(Note { text: "To jump, press Space.".into(), icon: Some("icons\\pain.dds".into()), seconds: Some(10.0) }),
                ScriptEffect::MessageBox(BoxMessage { message: 0xBB, text: "Old World Blues has been loaded.".into(), buttons: vec![], hidden: vec![] }),
                ScriptEffect::MessageBox(BoxMessage { message: 0xCC, text: "Hardcore?".into(), buttons: vec!["No".into(), "Yes".into()], hidden: vec![] }),
                // The quest was listed already: no "New quest".
                ScriptEffect::Quest(QuestNotice::Objectives(vec![Objective { quest: QUEST.0, text: "Use the Vit-o-matic Vigor Tester".into(), completed: false }])),
                ScriptEffect::Quest(QuestNotice::Objectives(vec![Objective { quest: QUEST.0, text: "Talk to Sunny Smiles.".into(), completed: true }])),
                ScriptEffect::Quest(QuestNotice::Completed("Back in the Saddle".into())),
                ScriptEffect::Notify("form 15169 removed".into()),
                ScriptEffect::Notify("form 15169 (5) added".into()),
            ]
        );
        // Many items at once: one line.
        let many: Vec<EventKind> = (0..10).map(|i| EventKind::ItemAdded { to: FormId::PLAYER_REF, item: FormId(0x100 + i), count: 2, silent: false }).collect();
        assert_eq!(run(many), vec![ScriptEffect::Notify("10 items added (20 in all)".into())]);
        // A script's hide flag (the opening's `player.RemoveItem PipBoy 1 1`): no line.
        let silent = vec![
            EventKind::ItemRemoved { from: FormId::PLAYER_REF, item: FormId(0x15038), count: 1, silent: true },
            EventKind::ItemAdded { to: FormId::PLAYER_REF, item: FormId(0x15169), count: 1, silent: true },
        ];
        assert_eq!(run(silent), vec![]);
    }

    #[test]
    fn the_first_objective_shown_adds_the_quest() {
        let objective = |text: &str, completed| ScriptEffect::Quest(QuestNotice::Objectives(vec![Objective { quest: QUEST.0, text: text.into(), completed }]));
        // Objective 10 is the quest's first: "New quest" comes before it, once, though
        // two are shown in the same frame. Objectives without text and quests without a
        // name show nothing.
        let e = run(vec![
            EventKind::ObjectiveDisplayed { quest: QUEST, index: 10, on: true },
            EventKind::ObjectiveDisplayed { quest: QUEST, index: 35, on: true },
            EventKind::ObjectiveDisplayed { quest: QUEST, index: 99, on: true },
            EventKind::ObjectiveDisplayed { quest: FormId(0x77), index: 10, on: true },
            EventKind::QuestFailed { quest: FormId(0x77) },
        ]);
        assert_eq!(e, vec![ScriptEffect::Quest(QuestNotice::Added("Back in the Saddle".into())), objective("Talk to Sunny Smiles.", false), objective("Follow Sunny.", false)]);
        // Shown again later, with 10 listed before: no "New quest".
        let e = run(vec![EventKind::ObjectiveDisplayed { quest: QUEST, index: 35, on: true }, EventKind::QuestFailed { quest: QUEST }]);
        assert_eq!(e, vec![objective("Follow Sunny.", false), ScriptEffect::Quest(QuestNotice::Failed("Back in the Saddle".into()))]);
    }

    #[test]
    fn scripted_pickup_says_added_once() {
        let pick = Activation { outcome: Outcome::PickedUp { item: FormId(0x15169), count: 1, owner: None }, unlocked_with: None, script: None };
        let events = vec![
            ev(EventKind::Activated { reference: FormId(0x55), by: FormId::PLAYER_REF, outcome: Some(serde_json::to_value(&pick).unwrap()) }),
            ev(EventKind::ItemAdded { to: FormId::PLAYER_REF, item: FormId(0x15169), count: 1, silent: false }),
        ];
        let lines = effects(&events, &BTreeSet::new(), &Fake).into_iter().filter(|e| matches!(e, ScriptEffect::Notify(_))).count();
        assert_eq!(lines, 1);
    }

    #[test]
    fn corner_clear_drops_earlier_notifications_and_keeps_other_effects_and_later_notices() {
        let events = vec![
            ev(EventKind::MessageShown { message: FormId(0), text: Some("before clear".into()), buttons: vec![], hidden: vec![] }),
            ev(EventKind::NoteShown { text: "alt before clear".into(), icon: None, seconds: 2.0 }),
            ev(EventKind::ItemAdded { to: FormId::PLAYER_REF, item: CAPS, count: 5, silent: false }),
            ev(EventKind::MessageShown { message: FormId(0xBB), text: Some("Box survives".into()), buttons: vec![], hidden: vec![] }),
            ev(EventKind::QuestCompleted { quest: QUEST }),
            // ClearMessageQueue ran here (index 5).
            ev(EventKind::MessageShown { message: FormId(0), text: Some("after clear".into()), buttons: vec![], hidden: vec![] }),
            ev(EventKind::ItemAdded { to: FormId::PLAYER_REF, item: FormId(0x15169), count: 1, silent: false }),
        ];
        let got = effects_with_corner_clear(&events, &BTreeSet::new(), 5, &Fake);
        assert_eq!(
            got,
            vec![
                ScriptEffect::MessageBox(BoxMessage { message: 0xBB, text: "Box survives".into(), buttons: vec![], hidden: vec![] }),
                ScriptEffect::Quest(QuestNotice::Completed("Back in the Saddle".into())),
                ScriptEffect::Notify("after clear".into()),
                ScriptEffect::ItemAddedSound(CAPS),
                ScriptEffect::Notify("form 15169 added".into()),
            ]
        );
    }

    /// #3095: caps a script gives the player sound once per batch; caps picked up by an
    /// activation (sounded by `world_sounds`) and silent adds do not.
    #[test]
    fn caps_a_script_gives_sound_once() {
        let caps = |count, silent| ev(EventKind::ItemAdded { to: FormId::PLAYER_REF, item: CAPS, count, silent });
        let sounds = |events: &[Event]| effects(events, &BTreeSet::new(), &Fake).into_iter().filter(|e| matches!(e, ScriptEffect::ItemAddedSound(_))).collect::<Vec<_>>();
        assert_eq!(sounds(&[caps(100, false), caps(25, false)]), vec![ScriptEffect::ItemAddedSound(CAPS)]);
        assert_eq!(sounds(&[caps(100, true)]), vec![]);
        assert_eq!(sounds(&[ev(EventKind::ItemAdded { to: FormId::PLAYER_REF, item: FormId(0x15169), count: 1, silent: false })]), vec![]);
        let pick = Activation { outcome: Outcome::PickedUp { item: CAPS, count: 5, owner: None }, unlocked_with: None, script: None };
        let picked = [ev(EventKind::Activated { reference: FormId(0x55), by: FormId::PLAYER_REF, outcome: Some(serde_json::to_value(&pick).unwrap()) }), caps(5, false)];
        assert_eq!(sounds(&picked), vec![]);
    }

    /// #3433: skill-book and iconless `NoteShown` (e.g. critical-hit) corner notifications
    /// use `icon: None` (default Vault Boy corner icon), while explicit `MESG` icons are
    /// preserved.
    #[test]
    fn book_read_and_iconless_notes_use_default_corner_icon() {
        let e = run(vec![
            EventKind::BookRead { actor: FormId::PLAYER_REF, book: FormId(0x1234), skill: 41, points: 3.0, value: 53.0, perks: vec![] },
            EventKind::NoteShown { text: "Critical hit on Giant Soldier Ant".into(), icon: None, seconds: 2.0 },
            EventKind::MessageShown { message: FormId(0xAA), text: Some("With icon.".into()), buttons: vec![], hidden: vec![] },
        ]);
        assert_eq!(
            e,
            vec![
                ScriptEffect::Notify(Note { text: "Guns increased to 53".into(), icon: None, seconds: None }),
                ScriptEffect::Notify(Note { text: "Critical hit on Giant Soldier Ant".into(), icon: None, seconds: Some(2.0) }),
                ScriptEffect::Notify(Note { text: "With icon.".into(), icon: Some("icons\\pain.dds".into()), seconds: Some(10.0) }),
            ]
        );
    }

    #[test]
    fn hidden_or_late_objectives_say_nothing() {
        let e = effects(&[ev(EventKind::ObjectiveCompleted { quest: QUEST, index: 10, on: true, silent: true })], &BTreeSet::new(), &Fake);
        assert_eq!(e, vec![]);
    }

    #[test]
    fn empty_objective_text_and_nameless_quests_show_no_objective_notices() {
        const NAMELESS: FormId = FormId(0x78);
        const BLANK_TITLE: FormId = FormId(0x79);
        struct EmptyCtx;
        impl EffectCtx for EmptyCtx {
            fn name(&self, form: FormId) -> String {
                Fake.name(form)
            }
            fn objective_text(&self, quest: FormId, index: i32) -> Option<String> {
                match (quest, index) {
                    (QUEST, 1) => Some("".into()),
                    (QUEST, 2) => Some("   \t\n ".into()),
                    (NAMELESS, 1) => Some("Hidden behind nameless quest".into()),
                    (BLANK_TITLE, 1) => Some("Hidden behind blank-title quest".into()),
                    _ => Fake.objective_text(quest, index),
                }
            }
            fn quest_title(&self, quest: FormId) -> Option<String> {
                match quest {
                    NAMELESS => None,
                    BLANK_TITLE => Some("   ".into()),
                    _ => Fake.quest_title(quest),
                }
            }
            fn quest_listed(&self, quest: FormId, besides: &[i32]) -> bool {
                Fake.quest_listed(quest, besides)
            }
            fn is_message_box(&self, message: FormId) -> bool {
                Fake.is_message_box(message)
            }
            fn message_look(&self, message: FormId) -> (Option<String>, Option<f32>) {
                Fake.message_look(message)
            }
            fn enable_children(&self, r: FormId) -> Vec<FormId> {
                Fake.enable_children(r)
            }
            fn pops_in(&self, r: FormId) -> bool {
                Fake.pops_in(r)
            }
            fn player_heading_deg(&self) -> f32 {
                Fake.player_heading_deg()
            }
            fn player_pitch_deg(&self) -> Option<f32> {
                Fake.player_pitch_deg()
            }
            fn destination(&self, cell: Option<FormId>) -> Destination {
                Fake.destination(cell)
            }
            fn activation_effects(&self, a: &Activation, target: FormId) -> Vec<Effect> {
                Fake.activation_effects(a, target)
            }
            fn is_terminal(&self, r: FormId) -> bool {
                Fake.is_terminal(r)
            }
        }
        let events = [
            ev(EventKind::ObjectiveDisplayed { quest: QUEST, index: 1, on: true }),
            ev(EventKind::ObjectiveDisplayed { quest: QUEST, index: 2, on: true }),
            ev(EventKind::ObjectiveCompleted { quest: QUEST, index: 2, on: true, silent: false }),
            ev(EventKind::ObjectiveDisplayed { quest: NAMELESS, index: 1, on: true }),
            ev(EventKind::ObjectiveCompleted { quest: NAMELESS, index: 1, on: true, silent: false }),
            ev(EventKind::ObjectiveDisplayed { quest: BLANK_TITLE, index: 1, on: true }),
            ev(EventKind::ObjectiveCompleted { quest: BLANK_TITLE, index: 1, on: true, silent: false }),
        ];
        assert!(effects(&events, &BTreeSet::new(), &EmptyCtx).is_empty());
    }

    #[test]
    fn skipped_events_are_not_applied_twice() {
        let pick = Activation { outcome: Outcome::PickedUp { item: FormId(0x15169), count: 1, owner: None }, unlocked_with: None, script: None };
        let events = vec![
            ev(EventKind::Activated { reference: FormId(0x55), by: FormId::PLAYER_REF, outcome: Some(serde_json::to_value(&pick).unwrap()) }),
            ev(EventKind::ItemAdded { to: FormId::PLAYER_REF, item: FormId(0x15169), count: 1, silent: false }),
            ev(EventKind::RefDisabled { reference: FormId(0x66), fade: false }),
        ];
        let e = effects(&events, &BTreeSet::from([0, 1]), &Fake);
        assert_eq!(e, vec![ScriptEffect::RefChanged(0x66)]);
    }

    #[test]
    fn inventory_changes_between_views() {
        let mut a = InvView::default();
        a.counts.insert(FormId(1), 2);
        a.counts.insert(FormId(2), 1);
        a.equipped.insert(FormId(2));
        let mut b = InvView::default();
        b.counts.insert(FormId(1), 5);
        b.counts.insert(FormId(3), 1);
        b.equipped.insert(FormId(3));
        let c = inventory_changes(&a, &b);
        assert_eq!(c.added, vec![(FormId(1), 3), (FormId(3), 1)]);
        assert_eq!(c.removed, vec![(FormId(2), 1)]);
        assert_eq!(c.equipped, vec![FormId(3)]);
        assert_eq!(c.unequipped, vec![FormId(2)]);
        assert!(inventory_changes(&b, &b).is_empty());
        let mut inv = Inventory::new();
        inv.add(FormId(7), 3);
        inv.set_equipped(FormId(7));
        let v = InvView::of(Some(&inv));
        assert_eq!(v.counts.get(&FormId(7)), Some(&3));
        assert!(v.equipped.contains(&FormId(7)));
    }

    #[test]
    fn the_nearest_point_inside_a_volume() {
        // A box turned a quarter, 200 x 50 x 100 half extents: from outside its long side
        // the walk aims just inside that side, level with the player.
        let t = TriggerShape {
            pos: [0.0, 0.0, 0.0],
            rot: ref_rotation([0.0, 0.0, std::f32::consts::FRAC_PI_2]),
            scale: 1.0,
            kind: TriggerKind::Box { half_extents: [200.0, 50.0, 100.0] },
        };
        let from = [300.0, 20.0, 0.0];
        let at = t.nearest_inside(from, 10.0);
        assert!(t.contains(at), "{at:?}");
        assert!((at[0] - 40.0).abs() < 0.5 && (at[1] - 20.0).abs() < 0.5, "{at:?}");
        // From inside it stays where it is.
        assert_eq!(t.nearest_inside([5.0, 5.0, 5.0], 10.0).map(|v| v.round()), [5.0, 5.0, 5.0]);
        let ball = TriggerShape { kind: TriggerKind::Sphere { radius: 50.0 }, ..t };
        let on = ball.nearest_inside([200.0, 0.0, 0.0], 10.0);
        assert!(ball.contains(on) && (on[0] - 40.0).abs() < 0.5, "{on:?}");
    }

    #[test]
    fn trigger_volumes() {
        // VCG01VigorTesterTrigger: a box of half extents 216 x 92.8 x 100 at
        // (1888.5, 1834.9, 7460), not rotated.
        let t = TriggerShape {
            pos: [1888.46, 1834.89, 7460.0],
            rot: ref_rotation([0.0; 3]),
            scale: 1.0,
            kind: TriggerKind::Box { half_extents: [216.0, 92.8, 100.0] },
        };
        assert!(t.contains([1888.0, 1834.0, 7400.0]));
        assert!(!t.contains([1888.0, 1950.0, 7400.0]));
        assert!(!t.contains([2110.0, 1834.0, 7400.0]));
        // Turned a quarter: the long side now runs along y.
        let q = TriggerShape { rot: ref_rotation([0.0, 0.0, std::f32::consts::FRAC_PI_2]), ..t };
        assert!(q.contains([1888.0, 1834.0 + 200.0, 7400.0]));
        assert!(!q.contains([1888.0 + 200.0, 1834.0, 7400.0]));
        // Scale grows the box.
        let s = TriggerShape { scale: 2.0, ..t };
        assert!(s.contains([1888.0, 1834.0 + 150.0, 7400.0]));
        let ball = TriggerShape { kind: TriggerKind::Sphere { radius: 50.0 }, ..t };
        assert!(ball.contains([1888.46, 1834.89, 7500.0]));
        assert!(!ball.contains([1888.46, 1834.89, 7520.0]));
        let before = BTreeSet::from([FormId(1), FormId(2)]);
        let now = BTreeSet::from([FormId(2), FormId(3)]);
        let ch = trigger_changes(&before, &now);
        assert_eq!(ch.entered, vec![FormId(3)]);
        assert_eq!(ch.inside, vec![FormId(2), FormId(3)]);
        assert_eq!(ch.left, vec![FormId(1)]);
    }

    #[test]
    fn capsules_against_volumes() {
        let t = TriggerShape {
            pos: [1888.46, 1834.89, 7460.0],
            rot: ref_rotation([0.0; 3]),
            scale: 1.0,
            kind: TriggerKind::Box { half_extents: [216.0, 92.8, 100.0] },
        };
        let cap = |x: f32, y: f32, z: f32| Capsule { feet: [x, y, z], radius: 22.0, height: 128.0 };
        // Standing on the floor in the middle.
        assert!(t.touches(&cap(1888.0, 1834.0, 7360.0)));
        // The axis is 10 units outside the long side, the capsule's side 12 inside: the
        // three points on the axis missed this one.
        let y = 1834.89 + 92.8 + 10.0;
        assert!(t.touches(&cap(1888.0, y, 7360.0)));
        assert!(!t.contains([1888.0, y, 7365.0]) && !t.contains([1888.0, y, 7416.0]));
        // 23 units outside: no contact.
        assert!(!t.touches(&cap(1888.0, 1834.89 + 92.8 + 23.0, 7360.0)));
        // Under the box: the head reaches it only from 128 units below its floor.
        assert!(t.touches(&cap(1888.0, 1834.0, 7360.0 - 127.0)));
        assert!(!t.touches(&cap(1888.0, 1834.0, 7360.0 - 129.0)));
        // Past a corner: the hemispheres are round.
        let (cx, cy) = (1888.46 + 216.0, 1834.89 + 92.8);
        assert!(t.touches(&cap(cx + 15.0, cy + 15.0, 7360.0)));
        assert!(!t.touches(&cap(cx + 16.0, cy + 16.0, 7360.0)));
        // A turned box.
        let q = TriggerShape { rot: ref_rotation([0.0, 0.0, std::f32::consts::FRAC_PI_2]), ..t };
        assert!(q.touches(&cap(1888.0, 1834.0 + 230.0, 7360.0)));
        assert!(!q.touches(&cap(1888.0 + 230.0, 1834.0, 7360.0)));
        // A sphere of radius 50 at the box's centre.
        let ball = TriggerShape { kind: TriggerKind::Sphere { radius: 50.0 }, ..t };
        assert!(ball.touches(&cap(1888.46 + 71.0, 1834.89, 7400.0)));
        assert!(!ball.touches(&cap(1888.46 + 73.0, 1834.89, 7400.0)));
        // From above: the capsule's lowest point is its feet.
        assert!(ball.touches(&cap(1888.46, 1834.89, 7460.0 + 49.0)));
        assert!(!ball.touches(&cap(1888.46, 1834.89, 7460.0 + 51.0)));
        // Scale grows both the volume and how far the capsule reaches into it.
        let s = TriggerShape { scale: 2.0, ..ball };
        assert!(s.touches(&cap(1888.46 + 121.0, 1834.89, 7400.0)));
        assert!(!s.touches(&cap(1888.46 + 123.0, 1834.89, 7400.0)));
        // Pairs of (trigger, actor).
        let before = BTreeSet::from([(FormId(1), FormId(7)), (FormId(1), FormId(8))]);
        let now = BTreeSet::from([(FormId(1), FormId(8)), (FormId(2), FormId(7))]);
        let ch = trigger_changes(&before, &now);
        assert_eq!(ch.entered, vec![(FormId(2), FormId(7))]);
        assert_eq!(ch.inside, vec![(FormId(1), FormId(8)), (FormId(2), FormId(7))]);
        assert_eq!(ch.left, vec![(FormId(1), FormId(7))]);
    }

    #[test]
    fn script_aids_parse() {
        let a = parse_aids(
            "3:start:VCG00, 4.5:stage:VCG01:60,5:objective:0x104C1C:30,6:info:0x00105599,7:button:1,8:var:0x0010E195:playerpassport:1,9:stop:VCG00,             2@VCG01=55:walk:scripted:0x00104C02, 0.5@VCG01=60:activate:VCG01VigorTesterREF, 1:close, 1:objective_done:VCG01:40, 2:info:0x001057E8:DocMitchellREF",
        )
        .unwrap();
        let at = |t: f32, aid: ScriptAid| TimedAid { at: t, after: None, aid };
        assert_eq!(
            a,
            vec![
                at(3.0, ScriptAid::Start("VCG00".into())),
                at(4.5, ScriptAid::Stage("VCG01".into(), 60)),
                at(5.0, ScriptAid::Objective("0x104C1C".into(), 30)),
                at(6.0, ScriptAid::Info("0x00105599".into(), None)),
                at(7.0, ScriptAid::Button(1)),
                at(8.0, ScriptAid::Var("0x0010E195".into(), "playerpassport".into(), 1.0)),
                at(9.0, ScriptAid::Stop("VCG00".into())),
                TimedAid { at: 2.0, after: Some(("VCG01".into(), 55)), aid: ScriptAid::Walk("scripted:0x00104C02".into()) },
                TimedAid { at: 0.5, after: Some(("VCG01".into(), 60)), aid: ScriptAid::Activate("VCG01VigorTesterREF".into()) },
                at(1.0, ScriptAid::Close),
                at(1.0, ScriptAid::ObjectiveDone("VCG01".into(), 40)),
                at(2.0, ScriptAid::Info("0x001057E8".into(), Some("DocMitchellREF".into()))),
            ]
        );
        // A variable of a reference named with a colon (the couch trigger of the opening).
        assert_eq!(
            parse_aids("1@VCG01=80:var:scripted:0x001055B4:bRunTimer:1").unwrap(),
            vec![TimedAid { at: 1.0, after: Some(("VCG01".into(), 80)), aid: ScriptAid::Var("scripted:0x001055B4".into(), "bRunTimer".into(), 1.0) }]
        );
        assert!(parse_aids("3:var:bRunTimer:1").is_err());
        // A name with a colon in any step: a target made by PlaceAtMe a moment before
        // (tools/capture/damage_parity.py), a placed reference by its base.
        let aids = parse_aids("5:set_av:created:0x0002E29D:Health:5000,5:moveto:created:0x0002E29D:player:0:300:0,6:call:created:NVCrGecko:SetAngle:z:0,7:damage:placed:GSTrudy:Health:0,8:kill:scripted:0x001055B4").unwrap();
        assert_eq!(aids[0].aid, ScriptAid::SetAv("created:0x0002E29D".into(), "Health".into(), 5000.0));
        assert_eq!(aids[1].aid, ScriptAid::MoveTo("created:0x0002E29D".into(), "player".into(), [0.0, 300.0, 0.0]));
        assert_eq!(aids[2].aid, ScriptAid::Call("created:NVCrGecko".into(), "SetAngle".into(), vec!["z".into(), "0".into()]));
        assert_eq!(aids[3].aid, ScriptAid::Damage("placed:GSTrudy".into(), "Health".into(), 0.0));
        assert_eq!(aids[4].aid, ScriptAid::Kill("scripted:0x001055B4".into()));
        assert!(parse_aids("3:stage:VCG01").is_err());
        assert_eq!(parse_aids("2:message:VMS18VancesGunMessage").unwrap()[0].aid, ScriptAid::Message("VMS18VancesGunMessage".into()));
        assert_eq!(parse_aids("12:play_idle:0x00104C6D:NPCSmokingIdle").unwrap()[0].aid, ScriptAid::PlayIdle("0x00104C6D".into(), "NPCSmokingIdle".into()));
        assert_eq!(
            parse_aids("3:call:SunnyRef:Look:player:1").unwrap()[0].aid,
            ScriptAid::Call("SunnyRef".into(), "Look".into(), vec!["player".into(), "1".into()])
        );
        assert_eq!(parse_aids("4:call:-:ReleaseWeatherOverride").unwrap()[0].aid, ScriptAid::Call("-".into(), "ReleaseWeatherOverride".into(), vec![]));
        assert_eq!(parse_aids("3:damage:player:Health:60").unwrap()[0].aid, ScriptAid::Damage("player".into(), "Health".into(), 60.0));
        let aids = parse_aids("1:hardcore:1,2:perk:Chemist,3:set_av:player:Dehydration:195").unwrap();
        assert_eq!(aids[0].aid, ScriptAid::Hardcore(true));
        assert_eq!(aids[1].aid, ScriptAid::Perk("Chemist".into()));
        assert_eq!(aids[2].aid, ScriptAid::SetAv("player".into(), "Dehydration".into(), 195.0));
        assert_eq!(parse_aids("3:menu:1074").unwrap()[0].aid, ScriptAid::Menu(1074));
        assert!(parse_aids("3:jump").is_err());
        assert!(parse_aids("x:start:VCG00").is_err());
        assert!(parse_aids("3@VCG01:close").is_err());
        assert!(parse_aids("3@VCG01=x:close").is_err());
    }

    #[test]
    fn late_scripts_keep_the_steps_apart() {
        let at = |a: &[TimedAid]| a.iter().map(|x| x.at).collect::<Vec<_>>();
        // The quest lines' check on a busy machine: the scripts first run at 17 s.
        let mut a = parse_aids("3:objective:VCG02:3,9:objective_done:VCG02:3,9.1:objective:VCG02:5,2@VCG02=10:close").unwrap();
        assert_eq!(delay_late_aids(&mut a, 17.0), Some(14.0));
        for (got, want) in at(&a).into_iter().zip([17.0, 23.0, 23.1, 2.0]) {
            assert!((got - want).abs() < 1e-4, "{got} {want}");
        }
        // In time: nothing moves.
        let mut b = parse_aids("3:start:VCG02,9:close").unwrap();
        assert_eq!(delay_late_aids(&mut b, 1.5), None);
        assert_eq!(at(&b), vec![3.0, 9.0]);
        // Only steps after a stage: nothing to move.
        let mut c = parse_aids("1@VCG02=10:close").unwrap();
        assert_eq!(delay_late_aids(&mut c, 30.0), None);
    }
}
