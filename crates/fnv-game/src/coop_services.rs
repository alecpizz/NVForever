//! A co-op guest's doctor [design: the heal and the rad removal a guest pays for apply to the guest, not to nothing or to the host]. The services a guest buys
//! act on the guest, the player in the dialogue, and are its own.
//!
//! **How the masters' doctors work** [observed: FalloutNV.esm's script records]:
//! - Rad removal: the line's result sets a stage of a doctor quest and takes the caps
//!   (INFO 0x000CE87F `SetStage VDoctors 30`, `player.RemoveItem Caps 100`); the stage's
//!   result is `player.RestoreAV RadiationRads 1000` (QUST 0x000CE959 stage 30).
//! - Healing and the addiction cure: the line sets the doctor's own Variable01 (1 heals,
//!   2 cures) and takes the caps (INFO 0x000CE882 `SetActorValue Variable01 1`,
//!   `player.RemoveItem Caps 75`). The doctor's object script then holds the player's
//!   controls, fades the screen and, 8 s later (10 s for the cure), sets the quest's
//!   stage 10 (`player.ResetHealth`) or 20 (the addictions' spells removed) and puts
//!   Variable01 to 3 (SCPT 0x0013A7F6, and a dozen alike).
//!
//! **Why a guest got nothing.** A guest was offered only lines that trade, deal Caravan,
//! exchange chips or call ResetHealth themselves; and a replication guest's game runs no
//! world reference's script (`Game::world_logic_off`), so a doctor's Variable01 set in
//! the guest's game set nothing going there, and the host's doctor never heard of it.
//!
//! **What a guest does now** [design]:
//! - It is offered a doctor's service lines ([`Services::lines`]): lines whose scripts
//!   only set a stage of a doctor quest ([`Services::quests`]), set the speaker's actor
//!   value, and take the payment, said by a doctor ([`Services::doctors`]).
//! - The line's scripts run in the guest's own game as before: `player` there is the
//!   guest's character, so the guest pays and the stage's result acts on it.
//! - The doctor's object script then runs in the guest's game for [`DOCTOR_RUN`] seconds
//!   (`Game::guest_own_refs`), as the original runs it for the player who paid: the fade,
//!   the controls, and the stage that heals or cures the guest. The host's doctor is not
//!   told; the host's game is untouched.
//! - What these scripts change of the guest's items and spells is the guest's own, not
//!   the host's story (`crate::coop_story`): the caps it paid stay paid and a cure stays
//!   cured in its own save (`Game::guest_service`). So do a cashier's chips.

use crate::data::{GameData, UnitId};
use crate::game::Game;
use fnv_script::unit::RefSlot;
use fnv_sim::FormId;
use std::collections::{BTreeMap, BTreeSet};

const SET_STAGE: u16 = 0x1039;
const RESET_HEALTH: u16 = 0x1150;
const RESTORE_AV: u16 = 0x1182;
const REMOVE_SPELL: u16 = 0x101D;
const SHOW_MESSAGE: u16 = 0x1059;
const SET_AV: u16 = 0x100F;
const REMOVE_ITEM: u16 = 0x1052;

/// How long a doctor's script runs in a guest's game after the guest's line: its
/// procedures take 8 s and 10 s and wait for the fade's end [observed: SCPT 0x0013A7F6
/// timers], with room to spare [design].
pub const DOCTOR_RUN: f32 = 30.0;

/// The masters' (and any mod's) doctors, found once from the data.
#[derive(Debug, Default)]
pub struct Services {
    /// Quests with no objectives whose stages only see to the player: heal it, restore
    /// its values, remove its spells (VDoctors 0x000CE959, Doctors 0x000963D4,
    /// 0x0015F0C0).
    pub quests: BTreeSet<FormId>,
    /// Object scripts that set a stage of one of [`Services::quests`].
    pub scripts: BTreeSet<FormId>,
    /// The base forms running one of [`Services::scripts`].
    pub doctors: BTreeSet<FormId>,
    /// The lines a guest may buy a doctor's service with.
    pub lines: BTreeSet<FormId>,
}

fn calls(data: &GameData, u: UnitId) -> BTreeMap<u16, u64> {
    let mut n = BTreeMap::new();
    crate::intro::calls(&data.unit(u).program, &mut n);
    n
}

/// The quests a unit's reference list names.
fn quests_named(data: &GameData, u: UnitId) -> Vec<FormId> {
    data.unit(u)
        .unit
        .refs
        .iter()
        .filter_map(|r| match r {
            RefSlot::Form(f) if data.is_quest(FormId(*f)) => Some(FormId(*f)),
            _ => None,
        })
        .collect()
}

/// Find the doctors (module notes).
pub fn services(data: &GameData) -> Services {
    let cares = [RESET_HEALTH, RESTORE_AV, REMOVE_SPELL];
    let quests: BTreeSet<FormId> = data
        .quests
        .iter()
        .filter(|(_, d)| d.objectives.is_empty())
        .filter(|(_, d)| {
            let units: Vec<UnitId> = d.stages.values().flatten().filter_map(|e| e.unit).collect();
            let all: Vec<BTreeMap<u16, u64>> = units.iter().map(|&u| calls(data, u)).collect();
            !all.is_empty()
                && all.iter().any(|c| c.keys().any(|k| cares.contains(k)))
                && all.iter().all(|c| c.keys().all(|k| cares.contains(k) || *k == SHOW_MESSAGE))
        })
        .map(|(q, _)| *q)
        .collect();
    let scripts: BTreeSet<FormId> = data
        .scripts
        .iter()
        .filter(|(_, &u)| calls(data, u).contains_key(&SET_STAGE) && quests_named(data, u).iter().any(|q| quests.contains(q)))
        .map(|(s, _)| *s)
        .collect();
    let doctors: BTreeSet<FormId> = data.form_script.iter().filter(|(_, s)| scripts.contains(s)).map(|(b, _)| *b).collect();
    let lines = data
        .infos
        .iter()
        .filter(|(_, d)| {
            let units: Vec<UnitId> = d.begin.into_iter().chain(d.end).collect();
            if units.is_empty() {
                return false;
            }
            let mut n: BTreeMap<u16, u64> = BTreeMap::new();
            let mut named = Vec::new();
            for &u in &units {
                n.extend(calls(data, u));
                named.extend(quests_named(data, u));
            }
            let only = n.keys().all(|k| [SET_STAGE, SET_AV, REMOVE_ITEM].contains(k));
            let named_ok = named.iter().all(|q| quests.contains(q));
            let stage = n.contains_key(&SET_STAGE) && !named.is_empty();
            let doctor = n.contains_key(&SET_AV) && crate::coverage::line_actor(data, d).is_some_and(|a| doctors.contains(&a));
            only && named_ok && (stage || doctor)
        })
        .map(|(i, _)| *i)
        .collect();
    Services { quests, scripts, doctors, lines }
}

/// The actor a line belongs to (`crate::coverage::line_actor`): its fixed speaker, else
/// its own GetIsID condition's.
pub fn line_speaker(data: &GameData, info: &crate::data::InfoDef) -> Option<FormId> {
    crate::coverage::line_actor(data, info)
}

impl Game<'_> {
    /// The doctors of the load order ([`services`]), found once.
    pub fn coop_services(&self) -> &Services {
        self.coop_services.get_or_init(|| services(self.data))
    }

    /// Run a dialogue line's result script `unit`, said by `actor`. In a co-op guest's
    /// game it is the guest's own doing (`Game::guest_service`), and a doctor's script it
    /// sets going runs here ([`Game::guest_own_refs`]).
    pub fn run_info_result(&mut self, actor: FormId, unit: UnitId, quest: Option<FormId>) {
        let guest = self.coop_guest;
        if guest {
            self.guest_service += 1;
        }
        self.run_result(unit, crate::game::RunCtx { self_ref: Some(actor), action_ref: None, quest });
        if guest {
            self.guest_service -= 1;
            let script = self.ref_script(actor);
            if script.is_some_and(|s| self.coop_services().scripts.contains(&s)) && !self.guest_own_refs.contains_key(&actor) {
                tracing::info!("co-op: this guest's line set {}'s doctor script going: it runs here for {DOCTOR_RUN} s", self.data.name(actor));
                self.guest_own_refs.insert(actor, DOCTOR_RUN);
            }
        }
    }

    /// A guest's: the references of [`Game::guest_own_refs`] whose scripts run this frame,
    /// their time counted down by `dt` (gone when it runs out).
    pub(crate) fn guest_own_refs_due(&mut self, dt: f64) -> Vec<FormId> {
        if !self.coop_guest {
            self.guest_own_refs.clear();
            return Vec::new();
        }
        self.guest_own_refs.retain(|_, left| {
            *left -= dt as f32;
            *left > 0.0
        });
        self.guest_own_refs.keys().copied().filter(|&r| self.present(r)).collect()
    }
}
