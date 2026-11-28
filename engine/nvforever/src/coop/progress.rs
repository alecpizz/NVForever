//! Shared progress between the link and `nvf_play::coop_progress`
//! (docs/design/coop-shared-progress.md): what one side's game gives the other's travels
//! as `nvf_coop::progress` items on the reliable stream.
//!
//! - **The host** passes its quest rewards, hand-outs, quest ends, stage offers, kill and
//!   hit credit and saves to its guests (an offer, a catch-up and a credit only to the
//!   players they are for), and its active quest, level and story every while
//!   (`nvf_coop::progress::HostViews`). Each stage offer it sends opens an answer for
//!   the guests it names (`HostChecks::offered`), and a guest that leaves closes its own
//!   (`HostChecks::left`).
//! - **What the host takes from a guest** passed the guard's rules first
//!   (`nvf_coop::progress::HostChecks`, registered when the lobby opens). A guest's save
//!   never makes a save of the host's kind: at most it brings the host's own autosave
//!   forward, once per `SAVE_FORWARD_EVERY` [design].
//! - **A guest** takes these items from the host only. The host's active quest replaces
//!   its own on the HUD and in the Pip-Boy (`Game::coop_set_host_quest`), its quest
//!   messages are shown as the host's game showed them, and its story pages go to
//!   `super::story`.
//!
//! What this part leaves to the world sync: each other player's actor in this world and
//! whether it is down (`CoopProgress::peer_actors`, `everyone_down`, `alone_down`,
//! `syncing`), and the guest flag on the game (`Game::coop_guest`).
//!
//! Log: `co-op: the host's quest: <notice>`, `co-op: the host is level <n> with <x> XP`,
//! `co-op: player <n> saved: the host's autosave comes forward`.

use super::*;
use fnv_game::coop::HostQuest;
use nvf_coop::progress::{self as wire, HostChecks, HostViews, Item, LevelView, Progress, QuestView, SaveForward};
use nvf_play::coop_progress::{CoopProgress, Role, Shared as Out};
use nvf_play::script_events::{Objective, QuestNotice};

/// Shared progress's state on this side.
#[derive(Default)]
pub(super) struct Shared {
    /// Items heard (sender, item), not yet taken.
    pub(super) heard: Vec<(u8, Vec<u8>)>,
    /// The host's checks on what guests send (module notes).
    pub(super) checks: HostChecks,
    /// The host's quest, level and story, for the guests.
    pub(super) views: HostViews,
    forward: SaveForward,
    /// A guest's: the host's level and XP as last heard.
    pub(super) host_level: Option<LevelView>,
    /// A guest's: the host's active quest as last heard, and the one last told (its
    /// messages wait until this game's scripts are there to name them).
    host_quest: Option<HostQuest>,
    quest_untold: Option<Option<HostQuest>>,
}

impl Shared {
    /// A lobby opened or closed: nothing of the last one is kept.
    pub(super) fn reset(&mut self) {
        *self = Shared::default();
    }
}

/// The save kind a heard [`Progress::Save`] names.
fn kind_in(k: u8) -> fnv_save::SaveKind {
    match k {
        Progress::QUICK => fnv_save::SaveKind::Quick,
        Progress::AUTO => fnv_save::SaveKind::Auto,
        Progress::SYSTEM => fnv_save::SaveKind::System,
        _ => fnv_save::SaveKind::Manual,
    }
}

/// A heard [`Progress`] as `nvf_play::coop_progress` takes it.
fn shared_of(what: Progress) -> Out {
    match what {
        Progress::Reward { quest, xp, caps } => Out::Reward { quest, xp, caps },
        Progress::Save { stamp, kind } => Out::Save { stamp, kind: kind_in(kind) },
        Progress::Reload { stamp } => Out::Reload { stamp },
        Progress::Gift { batch, item, count, quest, stage } => Out::Gift { batch, item, count, quest, stage },
        Progress::QuestEnded { quest, failed } => Out::QuestEnded { quest, failed },
        Progress::JointChoice { character, on, sat_out } => Out::JointChoice { character, on, sat_out },
        Progress::Stage { quest, stage, line, line_end, prior, to } => Out::Stage { quest, stage, line, line_end, prior, to },
        Progress::StageResult { quest, stage, taken } => Out::StageResult { quest, stage, taken },
        Progress::KillCredit { to, target, target_base, npc, xp, karma, killer, infamy } => Out::KillCredit { to, target, target_base, npc, xp, karma, killer, infamy },
        Progress::HitCredit { to, target, weapon, part, dealt, critical, crippled } => Out::HitCredit { to, target, weapon, part, dealt, critical, crippled },
        Progress::For { to, what } => Out::For { to, what: Box::new(shared_of(*what)) },
    }
}

/// An [`Out`] to send.
fn net_of(what: Out) -> Progress {
    match what {
        Out::Reward { quest, xp, caps } => Progress::Reward { quest, xp, caps },
        Out::Gift { batch, item, count, quest, stage } => Progress::Gift { batch, item, count, quest, stage },
        Out::QuestEnded { quest, failed } => Progress::QuestEnded { quest, failed },
        Out::JointChoice { character, on, sat_out } => Progress::JointChoice { character, on, sat_out },
        Out::Stage { quest, stage, line, line_end, prior, to } => Progress::Stage { quest, stage, line, line_end, prior, to },
        Out::StageResult { quest, stage, taken } => Progress::StageResult { quest, stage, taken },
        Out::KillCredit { to, target, target_base, npc, xp, karma, killer, infamy } => Progress::KillCredit { to, target, target_base, npc, xp, karma, killer, infamy },
        Out::HitCredit { to, target, weapon, part, dealt, critical, crippled } => Progress::HitCredit { to, target, weapon, part, dealt, critical, crippled },
        Out::Save { stamp, kind } => Progress::Save {
            stamp,
            kind: match kind {
                fnv_save::SaveKind::Quick => Progress::QUICK,
                fnv_save::SaveKind::Auto => Progress::AUTO,
                fnv_save::SaveKind::System => Progress::SYSTEM,
                fnv_save::SaveKind::Manual => Progress::MANUAL,
            },
        },
        Out::Reload { stamp } => Progress::Reload { stamp },
        Out::For { to, what } => Progress::For { to, what: Box::new(net_of(*what)) },
    }
}

/// The one player a host's item is for (`None`: every guest).
fn addressee(what: &Progress) -> Option<u8> {
    match what {
        Progress::For { to, .. } | Progress::KillCredit { to, .. } | Progress::HitCredit { to, .. } => Some(*to),
        _ => None,
    }
}

/// The stage offers in a host's item, for the checks: (players' bits, quest, stage).
fn offers(what: &Progress) -> Option<(u8, u32, i32)> {
    match what {
        Progress::Stage { quest, stage, to, .. } => Some((*to, *quest, *stage)),
        Progress::For { to, what } => match &**what {
            Progress::Stage { quest, stage, .. } => Some((1 << *to, *quest, *stage)),
            _ => None,
        },
        _ => None,
    }
}

/// The host's active quest as its guests show it ([`QuestView`]).
fn quest_view(st: Option<&nvf_play::pipboy::data::DataState>, g: &fnv_game::Game) -> QuestView {
    let Some(q) = nvf_play::pipboy::data::active_quest(st, g) else { return QuestView::default() };
    let Some(s) = g.quests.get(&q) else { return QuestView::default() };
    let objectives = s.objectives.iter().filter(|(_, o)| o.displayed || o.completed).map(|(i, o)| (*i, o.completed)).collect();
    QuestView { quest: q.0, completed: s.completed, failed: s.failed, objectives }
}

/// What a guest's game shows of a [`QuestView`] (`None`: the host has no quest).
fn host_quest(q: &QuestView) -> Option<HostQuest> {
    (q.quest != 0).then(|| HostQuest { quest: FormId(q.quest), objectives: q.objectives.iter().copied().collect(), completed: q.completed, failed: q.failed })
}

/// The quest messages a guest shows as the host's quest changes, as the host's own game
/// showed them (`nvf_play::script_events`): a new quest is added with its open
/// objectives; on the same quest, each objective shown or done. A quest's completed and
/// failed lines come with its own message, in the order the host's quests ended
/// (`Progress::QuestEnded`). `def`: the new quest's record.
fn host_quest_notices(def: Option<&fnv_game::quest::QuestDef>, old: Option<&HostQuest>, new: Option<&HostQuest>) -> Vec<QuestNotice> {
    let (Some(n), Some(def)) = (new, def) else { return Vec::new() };
    let name = def.name.clone().unwrap_or_default();
    let same = old.filter(|o| o.quest == n.quest);
    let mut out = Vec::new();
    if same.is_none() && !n.completed && !n.failed {
        out.push(QuestNotice::Added(name));
    }
    let mut objectives = Vec::new();
    for (i, &done) in &n.objectives {
        let before = same.and_then(|o| o.objectives.get(i).copied());
        let tell = match before {
            None => same.is_some() || !done,
            Some(was) => done && !was,
        };
        let text = def.objectives.get(i).and_then(|o| o.text.as_deref()).map(str::trim).unwrap_or("");
        if tell && !text.is_empty() {
            objectives.push(Objective { quest: n.quest.0, text: text.to_string(), completed: done });
        }
    }
    if !objectives.is_empty() {
        out.push(QuestNotice::Objectives(objectives));
    }
    out
}

/// The items heard, taken: a guest's from the host, the host's from its guests (each
/// passed the guard's rules).
fn take_heard(coop: &mut Coop, p: &mut CoopProgress, now: std::time::Instant) {
    let is_host = coop.is_host();
    for (id, item) in std::mem::take(&mut coop.progress.heard) {
        let got = if is_host { wire::decode(&item) } else { wire::from_host(id, &item) };
        let Some(got) = got else {
            warn!("co-op: player {} sent a progress item this game cannot read", id + 1);
            continue;
        };
        match got {
            Item::Progress(Progress::Save { stamp, .. }) if is_host => {
                if coop.progress.forward.take(now) {
                    info!("co-op: player {} saved: the host's autosave comes forward", id + 1);
                    p.inbox.push((id, Out::Save { stamp, kind: fnv_save::SaveKind::Auto }));
                } else {
                    info!("co-op: player {} saved: the host autosaved a moment ago", id + 1);
                }
            }
            Item::Progress(what) => p.inbox.push((id, shared_of(what))),
            Item::Quest(q) if !is_host => {
                // Told from the quest last told, even when messages for a later one wait.
                let told = coop.progress.quest_untold.take().unwrap_or_else(|| coop.progress.host_quest.clone());
                coop.progress.host_quest = host_quest(&q);
                coop.progress.quest_untold = Some(told);
            }
            Item::Level(l) if !is_host => {
                if coop.progress.host_level.is_none_or(|h| h.level != l.level) {
                    info!("co-op: the host is level {} with {} XP", l.level, l.xp);
                }
                coop.progress.host_level = Some(l);
            }
            Item::Story(entries) if !is_host => coop.story.heard.extend(entries),
            // A host's kind on the host: the guard dropped it already.
            Item::Quest(_) | Item::Level(_) | Item::Story(_) => {}
        }
    }
}

/// What `nvf_play::coop_progress` has to send, sent.
fn send_out(coop: &mut Coop, p: &mut CoopProgress) {
    let out = std::mem::take(&mut p.outbox);
    let Some(link) = coop.link.as_ref() else { return };
    let is_host = link.is_host();
    for what in out.into_iter().map(net_of) {
        // A guest sends only its own kinds; the host's are its alone.
        if !is_host && what.host_only() {
            continue;
        }
        if let Some((to, quest, stage)) = offers(&what).filter(|_| is_host) {
            coop.progress.checks.offered(to, quest, stage);
        }
        let to = if is_host { addressee(&what) } else { None };
        if !link.send_reliable(to, what.encode()) {
            warn!("co-op: a progress item was not sent: the stream to {} is full", to.map_or("the others".to_string(), |t| format!("player {}", t + 1)));
        }
    }
}

/// Each frame: the mailbox between the link and `nvf_play::coop_progress` (module notes).
pub(super) fn progress(
    mut coop: ResMut<Coop>,
    mut p: ResMut<CoopProgress>,
    mut script: Option<ResMut<nvf_play::script::ScriptState>>,
    data: Option<Res<nvf_play::pipboy::data::DataState>>,
    main_menu: Option<Res<nvf_play::main_menu::MainMenu>>,
    mut quest_lines: Option<ResMut<nvf_play::quest_messages::QuestMessages>>,
) {
    let coop = &mut *coop;
    let p = &mut *p;
    let role = match coop.link.as_ref() {
        None => Role::Solo,
        Some(l) if l.is_host() => Role::Host,
        Some(_) => Role::Guest,
    };
    p.role = role;
    p.my_id = coop.link.as_ref().and_then(Link::id);
    // Who is here, who is heard from now, and their names (the host's catch-up for a
    // player that drops and comes back, `nvf_play::coop_progress::Returns`).
    p.present = coop.peers.keys().copied().collect();
    p.connected = coop.peers.iter().filter(|(_, q)| !q.lost).map(|(id, _)| *id).collect();
    // Kept after a player leaves: its drop is told by its name, not "Player 2".
    let names: Vec<(u8, String)> = coop.peers.iter().map(|(id, q)| (*id, label(*id, &q.name))).collect();
    p.peer_names.extend(names);
    let now = std::time::Instant::now();
    take_heard(coop, p, now);
    send_out(coop, p);
    let g = script.as_deref_mut().and_then(|s| s.rt.as_mut()).map(|rt| &mut rt.game);
    let Some(g) = g else { return };
    match role {
        Role::Host if !main_menu.is_some_and(|m| m.open) => {
            coop.progress.views.set_quest(quest_view(data.as_deref(), g));
            coop.progress.views.set_level(LevelView { level: g.player_level().clamp(0.0, f64::from(u16::MAX)) as u16, xp: g.player.xp.clamp(0.0, f64::from(u32::MAX)) as u32 });
            let Some(link) = coop.link.as_ref() else { return };
            for item in coop.progress.views.due(now) {
                // A full stream sends the whole view again once it starts again.
                let _ = link.send_reliable(None, item);
            }
        }
        Role::Guest => {
            // The host's quest messages, once this game's scripts are there to name them.
            if let Some(told) = coop.progress.quest_untold.take() {
                let new = coop.progress.host_quest.as_ref();
                for n in host_quest_notices(new.and_then(|n| g.data.quests.get(&n.quest)), told.as_ref(), new) {
                    info!("co-op: the host's quest: {}", nvf_play::quest_messages::describe(&n));
                    if let Some(l) = quest_lines.as_deref_mut() {
                        l.push(n);
                    }
                }
            }
            if g.coop_guest && g.coop_host_quest != coop.progress.host_quest {
                g.coop_set_host_quest(coop.progress.host_quest.clone());
            }
            let host_name = coop.peers.get(&0).map(|p| p.name.clone()).filter(|n| !n.is_empty());
            if g.coop_host_name != host_name {
                g.coop_host_name = host_name;
            }
        }
        // Not a guest (any more): the host's quests and story are this game's no longer.
        _ if g.coop_host_quest.is_some() || !g.coop_host_ended.is_empty() || g.coop_host_name.is_some() => {
            g.coop_forget_host_quests();
            g.coop_host_story = Default::default();
            g.coop_host_name = None;
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_progress_maps_to_the_game_and_back() {
        let all = [
            Progress::Reward { quest: 1, xp: 2, caps: 3 },
            Progress::Save { stamp: 4, kind: Progress::QUICK },
            Progress::Reload { stamp: 5 },
            Progress::Gift { batch: 1, item: 2, count: 3, quest: 4, stage: Progress::NO_STAGE },
            Progress::QuestEnded { quest: 6, failed: true },
            Progress::JointChoice { character: 7, on: true, sat_out: 0b100 },
            Progress::Stage { quest: 8, stage: 10, line: 9, line_end: false, prior: vec![1], to: 0b10 },
            Progress::StageResult { quest: 8, stage: 10, taken: true },
            Progress::KillCredit { to: 1, target: 2, target_base: 3, npc: true, xp: 4, karma: -5, killer: false, infamy: vec![(6, 7)] },
            Progress::HitCredit { to: 2, target: 3, weapon: 0, part: u32::MAX, dealt: 4, critical: false, crippled: true },
            Progress::For { to: 1, what: Box::new(Progress::Reward { quest: 1, xp: 2, caps: 3 }) },
        ];
        for p in all {
            assert_eq!(net_of(shared_of(p.clone())), p);
        }
    }

    #[test]
    fn an_offer_a_catch_up_and_a_credit_go_only_to_their_players() {
        let stage = Progress::Stage { quest: 8, stage: 10, line: 0, line_end: false, prior: vec![], to: 0b0110 };
        assert_eq!(addressee(&stage), None);
        assert_eq!(offers(&stage), Some((0b0110, 8, 10)));
        let back = Progress::For { to: 3, what: Box::new(stage) };
        assert_eq!(addressee(&back), Some(3));
        assert_eq!(offers(&back), Some((0b1000, 8, 10)));
        assert_eq!(addressee(&Progress::HitCredit { to: 2, target: 0, weapon: 0, part: 0, dealt: 0, critical: false, crippled: false }), Some(2));
        assert_eq!(offers(&Progress::Reward { quest: 1, xp: 1, caps: 1 }), None);
    }

    #[test]
    fn a_new_host_quest_is_added_and_an_objective_done_is_told_once() {
        let q = |objectives: &[(i32, bool)]| HostQuest { quest: FormId(9), objectives: objectives.iter().copied().collect(), completed: false, failed: false };
        let mut def = fnv_game::quest::QuestDef { name: Some("Q".into()), ..Default::default() };
        def.objectives.insert(10, fnv_game::quest::Objective { index: 10, text: Some("Do it".into()), ..Default::default() });
        let def = Some(&def);
        let first = host_quest_notices(def, None, Some(&q(&[(10, false)])));
        assert!(matches!(&first[0], QuestNotice::Added(n) if n == "Q"));
        let done = host_quest_notices(def, Some(&q(&[(10, false)])), Some(&q(&[(10, true)])));
        assert!(matches!(&done[..], [QuestNotice::Objectives(o)] if o[0].completed));
        assert!(host_quest_notices(def, Some(&q(&[(10, true)])), Some(&q(&[(10, true)]))).is_empty());
    }
}
